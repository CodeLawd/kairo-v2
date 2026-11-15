//! The Documents page's backend over the application's services (prompt
//! 085): `window.api.documents`, `documents.converter`, `libraries` and
//! `settings` in Electron. Pages are rendered by the documents service
//! (PDFium), not the window.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use kairo_core::dialogs::{DialogKind, dialog_request};
use kairo_documents::DocumentError;
use kairo_documents::service::PrepareOutcome;
use kairo_domain::documents::{
    DocumentPrepareOptions, DocumentsSettings, normalize_documents_settings,
};
use kairo_domain::program::ProgramState;
use kairo_domain::settings::ShortcutBindings;
use serde_json::Value;
use ui_documents::backend::{
    DocumentsAnswer, DocumentsBackend, DocumentsCall, DocumentsFeed, DocumentsSnapshot, FeedSink,
    ImportKind, ImportOutcome, ImportReply, ImportStep, Listening, ProgressSink, Reply,
};
use ui_operator::UiState;

use crate::services::{ApplicationServices, ImportRequested, SettingsChanged};

pub struct ServicesDocuments {
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn UiState>,
}

impl ServicesDocuments {
    pub fn new(services: Arc<ApplicationServices>, ui_state: Arc<dyn UiState>) -> Self {
        Self { services, ui_state }
    }
}

fn settings_of(all: &Value) -> DocumentsSettings {
    normalize_documents_settings(all.get("documents").unwrap_or(&Value::Null))
}

fn shortcuts_of(all: &Value) -> ShortcutBindings {
    all.pointer("/display/shortcuts")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn all_settings(services: &ApplicationServices) -> Value {
    services
        .settings_wiring
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_all()
}

/// A document page is what the live output shows (`Page N`).
fn shows_document(state: &ProgramState) -> bool {
    state
        .current
        .as_ref()
        .is_some_and(|c| c.reference.starts_with("Page ") && c.text.is_empty())
}

impl DocumentsBackend for ServicesDocuments {
    fn snapshot(&self) -> DocumentsSnapshot {
        let all = all_settings(&self.services);
        DocumentsSnapshot {
            libraries: self.services.libraries.snapshot().documents,
            settings: settings_of(&all),
            shortcuts: shortcuts_of(&all),
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let mut tasks = Vec::new();
        tasks.push(services.runtime().spawn({
            let (services, sink) = (services.clone(), sink.clone());
            async move {
                let docs = services.documents.clone();
                match services.core.blocking(move || docs.list()).await {
                    Ok(Ok(list)) => sink(DocumentsFeed::Documents(list)),
                    Ok(Err(error)) => tracing::error!(%error, "[Documents] library read failed"),
                    Err(error) => tracing::error!(%error, "[Documents] library read failed"),
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut values = services.core.watch::<SettingsChanged>();
            let sink = sink.clone();
            async move {
                while let Some(value) = values.next().await {
                    let SettingsChanged(all) = &*value;
                    sink(DocumentsFeed::Settings(settings_of(all)));
                    sink(DocumentsFeed::Shortcuts(shortcuts_of(all)));
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut requests = services.core.watch::<ImportRequested>();
            let sink = sink.clone();
            async move {
                use kairo_domain::import_menu::ImportKind as Menu;
                while let Some(request) = requests.next().await {
                    match request.0 {
                        Menu::Pdf => sink(DocumentsFeed::Import(ImportKind::Pdf)),
                        Menu::PowerPoint => sink(DocumentsFeed::Import(ImportKind::Powerpoint)),
                        _ => {}
                    }
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut status = services.converter.subscribe();
            let sink = sink.clone();
            async move {
                while let Ok(value) = status.recv().await {
                    sink(DocumentsFeed::Converter(value));
                }
            }
        }));
        tasks.push(services.runtime().spawn({
            let mut program = services.core.watch::<ProgramState>();
            let sink = sink.clone();
            async move {
                let mut showing = false;
                while let Some(state) = program.next().await {
                    let now = shows_document(&state);
                    if showing && !now {
                        sink(DocumentsFeed::LiveCleared);
                    }
                    showing = now;
                }
            }
        }));
        let libraries = sink;
        services
            .libraries
            .on_changed(move |state| libraries(DocumentsFeed::Libraries(state.documents.clone())));
        struct Feed(Vec<tokio::task::JoinHandle<()>>);
        impl Drop for Feed {
            fn drop(&mut self) {
                for task in &self.0 {
                    task.abort();
                }
            }
        }
        Box::new(Feed(tasks))
    }

    fn call(&self, call: DocumentsCall, reply: Reply) {
        let services = self.services.clone();
        services.clone().spawn(async move {
            reply(run(&services, call).await);
        });
    }

    fn import(
        &self,
        kind: Option<ImportKind>,
        options: DocumentPrepareOptions,
        cancel: Arc<AtomicBool>,
        progress: ProgressSink,
        reply: ImportReply,
    ) {
        let services = self.services.clone();
        services.clone().spawn(async move {
            reply(import(&services, kind, options, cancel, progress).await);
        });
    }

    fn read_ui(&self, key: &str) -> Option<Value> {
        self.ui_state.get(key)
    }

    fn write_ui(&self, key: &str, value: Value) {
        self.ui_state.set(key, value);
    }
}

fn text(error: &DocumentError) -> String {
    error.to_string()
}

async fn blocking<T: Send + 'static>(
    services: &Arc<ApplicationServices>,
    f: impl FnOnce(&kairo_documents::service::DocumentsService) -> Result<T, DocumentError>
    + Send
    + 'static,
) -> Result<T, String> {
    let docs = services.documents.clone();
    services
        .core
        .blocking(move || f(&docs).map_err(|e| text(&e)))
        .await
        .map_err(|e| e.to_string())?
}

async fn run(
    services: &Arc<ApplicationServices>,
    call: DocumentsCall,
) -> Result<DocumentsAnswer, String> {
    match call {
        DocumentsCall::List => blocking(services, |d| d.list())
            .await
            .map(DocumentsAnswer::Documents),
        DocumentsCall::Push { id, page } => services
            .actions()
            .push_document(&id, page as f64)
            .await
            .map(|applied| DocumentsAnswer::Pushed { applied }),
        DocumentsCall::Rename { id, name } => blocking(services, move |d| d.rename(&id, &name))
            .await
            .map(DocumentsAnswer::Documents),
        DocumentsCall::Remove(id) => services
            .actions()
            .remove_document(&id)
            .await
            .map(DocumentsAnswer::Documents),
        DocumentsCall::Playback { id, page } => Ok(DocumentsAnswer::Playback(
            services.actions().document_playback(&id, page as f64).await,
        )),
        DocumentsCall::Control {
            id,
            page,
            video,
            command,
        } => services
            .actions()
            .control_document(&id, page as f64, video as f64, command)
            .await
            .map(|_| DocumentsAnswer::Done),
        DocumentsCall::SaveSettings(settings) => {
            let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
            let services = services.clone();
            let core = services.core.clone();
            core.blocking(move || {
                services
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set("documents", value)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())??;
            Ok(DocumentsAnswer::Done)
        }
        DocumentsCall::Libraries(command) => services
            .libraries
            .apply(command)
            .map(|state| DocumentsAnswer::Libraries(state.documents))
            .map_err(|e| e.to_string()),
        DocumentsCall::ConverterStatus => {
            Ok(DocumentsAnswer::Converter(services.converter.status()))
        }
        DocumentsCall::ConverterInstall => {
            let converter = services.converter.clone();
            services.spawn(async move {
                converter.install().await;
            });
            Ok(DocumentsAnswer::Done)
        }
        DocumentsCall::ConverterCancel => {
            services.converter.cancel();
            Ok(DocumentsAnswer::Done)
        }
    }
}

/// `prepare`, then the service's render loop and `finish`; any failure or
/// cancel removes the half-written import.
async fn import(
    services: &Arc<ApplicationServices>,
    kind: Option<ImportKind>,
    options: DocumentPrepareOptions,
    cancel: Arc<AtomicBool>,
    progress: ProgressSink,
) -> Result<ImportOutcome, String> {
    let picked: Option<PathBuf> = if options.resume.is_some() {
        None
    } else {
        let dialog = match kind {
            Some(ImportKind::Pdf) => DialogKind::ImportPdf,
            Some(ImportKind::Powerpoint) => DialogKind::ImportPowerpoint,
            None => DialogKind::ImportSlides,
        };
        let files = services
            .actions()
            .dialogs
            .pick(dialog_request(dialog, None))
            .await?;
        match files.into_iter().next() {
            Some(file) => Some(file),
            None => return Ok(ImportOutcome::NotPicked),
        }
    };
    let prepare = {
        let progress = progress.clone();
        let kind = kind.map(ImportKind::as_str);
        blocking(services, move |d| {
            d.prepare(kind, picked, &options, &mut |info| {
                progress(ImportStep::Preparing {
                    name: info.name,
                    format: Some(info.format),
                })
            })
        })
        .await?
    };
    let prepared = match prepare {
        None => return Ok(ImportOutcome::NotPicked),
        Some(PrepareOutcome::NeedsConverter(needs)) => {
            return Ok(ImportOutcome::NeedsConverter(needs));
        }
        Some(PrepareOutcome::Prepared(prepared)) => prepared,
    };
    progress(ImportStep::Preparing {
        name: prepared.name.clone(),
        format: Some(prepared.format),
    });
    let id = prepared.id.clone();
    let outcome = render_and_finish(services, prepared, cancel.clone(), progress).await;
    match outcome {
        Ok(documents) => Ok(ImportOutcome::Imported { id, documents }),
        Err(error) => {
            let cleanup = id.clone();
            let _ = blocking(services, move |d| d.cancel(&cleanup)).await;
            if cancel.load(Ordering::SeqCst) {
                Ok(ImportOutcome::Cancelled)
            } else {
                Err(error)
            }
        }
    }
}

async fn render_and_finish(
    services: &Arc<ApplicationServices>,
    prepared: kairo_domain::documents::DocumentPrepared,
    cancel: Arc<AtomicBool>,
    progress: ProgressSink,
) -> Result<Vec<kairo_domain::documents::ProjectionDocument>, String> {
    if cancel.load(Ordering::SeqCst) {
        return Err(text(&DocumentError::Cancelled));
    }
    let id = prepared.id.clone();
    {
        let progress = progress.clone();
        let cancel = cancel.clone();
        blocking(services, move |d| {
            d.render(
                &prepared.id,
                &prepared.data,
                &cancel,
                &mut |completed, total| progress(ImportStep::Rendering { completed, total }),
            )
        })
        .await?;
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(text(&DocumentError::Cancelled));
    }
    progress(ImportStep::Finishing);
    blocking(services, move |d| d.finish(&id)).await
}

pub fn install(
    backend: Arc<dyn DocumentsBackend>,
    operator: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_documents::DocumentsPage> {
    use gpui_kit::AppContext;
    let page = cx.new(|cx| ui_documents::DocumentsPage::new(backend, window, cx));
    let rail = operator.read(cx).rail().clone();
    page.update(cx, |p, cx| p.set_rail(Some(rail.into()), cx));
    page
}
