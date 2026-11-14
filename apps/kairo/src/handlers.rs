//! Native-dialog and OS shell handlers above the composed services.
use crate::services::ApplicationServices;
use kairo_core::dialogs::{DialogKind, dialog_request};
use kairo_domain::{
    documents::{DocumentImportInfo, DocumentPrepareOptions},
    settings::ScriptureSettings,
    workspace::WorkspaceInfo,
};
use kairo_orchestrator::voice::BoxFuture;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct Handlers(pub Arc<ApplicationServices>);

impl Handlers {
    pub async fn choose_workspace(&self, move_contents: bool) -> Result<WorkspaceInfo, String> {
        let services = self.0.clone();
        let current = self
            .0
            .core
            .blocking(move || {
                services
                    .workspace
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .ensure(&services.settings.lock().unwrap_or_else(|e| e.into_inner()))
            })
            .await
            .map_err(|e| e.to_string())?;
        let selected = self
            .0
            .ui
            .dialog(dialog_request(
                DialogKind::Workspace,
                Some(current.root.clone().into()),
            ))
            .await?;
        let Some(folder) = selected.first().cloned() else {
            return Ok(current);
        };
        let services = self.0.clone();
        let info = self
            .0
            .core
            .blocking(move || {
                services
                    .workspace
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_root(
                        &mut services.settings.lock().unwrap_or_else(|e| e.into_inner()),
                        &folder,
                        move_contents,
                    )
            })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        self.0
            .lyrics
            .sync_from_folder()
            .await
            .map_err(|e| e.to_string())?;
        let media = self.0.media.clone();
        self.0
            .core
            .blocking(move || media.lock().unwrap_or_else(|e| e.into_inner()).scan())
            .await
            .map_err(|e| e.to_string())?;
        Ok(info)
    }
    pub async fn adopt_media(
        &self,
        move_contents: bool,
    ) -> Result<kairo_domain::media::MediaLibrary, String> {
        let services = self.0.clone();
        self.0
            .core
            .blocking(move || {
                let folder = services
                    .workspace
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .adopt_media_folder(
                        &mut services.settings.lock().unwrap_or_else(|e| e.into_inner()),
                        move_contents,
                    );
                services
                    .media
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .set_folder(&folder)
            })
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn reveal_workspace(&self, songs: bool) -> Result<(), String> {
        let services = self.0.clone();
        self.0
            .core
            .blocking(move || {
                let mut workspace = services.workspace.lock().unwrap_or_else(|e| e.into_inner());
                let settings = services.settings.lock().unwrap_or_else(|e| e.into_inner());
                let path = if songs {
                    workspace.reveal_songs_target(&settings)
                } else {
                    workspace.reveal_target(&settings)
                };
                open_path(&path, false)
            })
            .await
            .map_err(|e| e.to_string())?
    }
    pub async fn import_sermon_notes(
        &self,
    ) -> Result<Option<kairo_domain::scripture::SermonPlanDraft>, String> {
        let picked = self
            .0
            .ui
            .dialog(dialog_request(DialogKind::SermonNotes, None))
            .await?
            .into_iter()
            .next();
        let translation = self
            .0
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .section::<ScriptureSettings>("scripture")
            .map_err(|e| e.to_string())?
            .default_translation;
        kairo_scripture::sermon_notes::import_sermon_notes(picked, translation, |_| {})
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn prepare_document(
        &self,
        kind: Option<&str>,
        options: DocumentPrepareOptions,
        mut preparing: impl FnMut(DocumentImportInfo) + Send + 'static,
    ) -> Result<Option<kairo_documents::service::PrepareOutcome>, String> {
        let dialog_kind = match kind {
            None => DialogKind::ImportSlides,
            Some("pdf") => DialogKind::ImportPdf,
            Some("powerpoint") => DialogKind::ImportPowerpoint,
            _ => return Err("Unsupported document type.".into()),
        };
        let picked = if options.resume.is_some() {
            None
        } else {
            self.0
                .ui
                .dialog(dialog_request(dialog_kind, None))
                .await?
                .into_iter()
                .next()
        };
        let docs = self.0.documents.clone();
        let kind = kind.map(str::to_owned);
        self.0
            .core
            .blocking(move || docs.prepare(kind.as_deref(), picked, &options, &mut preparing))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }
    pub fn capture_folder(&self) -> PathBuf {
        let settings = self.0.settings.lock().unwrap_or_else(|e| e.into_inner());
        let capture = kairo_domain::capture::normalize_capture_settings(
            settings
                .data()
                .get("capture")
                .unwrap_or(&serde_json::Value::Null),
        );
        if capture.folder.is_empty() {
            self.0
                .paths()
                .videos
                .as_ref()
                .unwrap_or(&self.0.paths().home)
                .join("Kairo")
        } else {
            capture.folder.into()
        }
    }
    pub async fn choose_capture_folder(&self) -> Result<Option<PathBuf>, String> {
        let selected = self
            .0
            .ui
            .dialog(dialog_request(
                DialogKind::CaptureFolder,
                Some(self.capture_folder()),
            ))
            .await?;
        let Some(folder) = selected.first().cloned() else {
            return Ok(None);
        };
        let services = self.0.clone();
        let picked = folder.clone();
        self.0
            .core
            .blocking(move || {
                let mut wiring = services
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                wiring
                    .set("capture", serde_json::json!({"folder":picked}))
                    .map_err(|e| e.to_string())?;
                wiring.broadcast_changed();
                Ok::<_, String>(())
            })
            .await
            .map_err(|e| e.to_string())??;
        Ok(Some(folder))
    }
    pub async fn reveal_capture(&self) -> Result<(), String> {
        let last = self
            .0
            .capture_status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .path
            .clone();
        let folder = self.capture_folder();
        self.0
            .core
            .blocking(move || {
                if let Some(last) = last
                    && Path::new(&last).exists()
                {
                    return open_path(Path::new(&last), true);
                }
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                open_path(&folder, false)
            })
            .await
            .map_err(|e| e.to_string())?
    }
}

impl kairo_transfer::FileDialogs for crate::services::UiBridge {
    fn save_kairo<'a>(&'a self, default_name: &'a str) -> BoxFuture<'a, Option<PathBuf>> {
        Box::pin(async move {
            match self
                .dialog(dialog_request(
                    DialogKind::ExportKairo,
                    Some(default_name.into()),
                ))
                .await
            {
                Ok(files) => files.into_iter().next(),
                Err(error) => {
                    tracing::warn!(%error, "[Transfer] save dialog failed");
                    None
                }
            }
        })
    }
    fn open_kairo(&self) -> BoxFuture<'_, Vec<PathBuf>> {
        Box::pin(async move {
            match self
                .dialog(dialog_request(DialogKind::ImportKairo, None))
                .await
            {
                Ok(files) => files,
                Err(error) => {
                    tracing::warn!(%error, "[Transfer] open dialog failed");
                    Vec::new()
                }
            }
        })
    }
}

pub fn open_path(path: &Path, reveal: bool) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        if reveal {
            c.arg("-R");
        }
        c.arg(path);
        c
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("explorer.exe");
        if reveal {
            c.arg(format!("/select,{}", path.display()));
        } else {
            c.arg(path);
        }
        c
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(if reveal {
            path.parent().unwrap_or(path)
        } else {
            path
        });
        c
    };
    let status = command.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err("Could not open the folder.".into())
    }
}

impl Handlers {
    pub fn capture_status(&self) -> kairo_domain::capture::CaptureStatus {
        self.0
            .capture_status
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    fn refuse_capture(&self, error: String) -> kairo_domain::capture::CaptureStatus {
        let mut status = self
            .0
            .capture_status
            .write()
            .unwrap_or_else(|e| e.into_inner());
        status.error = Some(error);
        self.0.core.publisher().publish(status.clone());
        status.clone()
    }
    pub async fn start_capture(&self) -> Result<kairo_domain::capture::CaptureStatus, String> {
        if self.capture_status().recording {
            return Ok(self.capture_status());
        }
        let (mut capture, source) = {
            let settings = self.0.settings.lock().unwrap_or_else(|e| e.into_inner());
            let capture = kairo_domain::capture::normalize_capture_settings(
                settings
                    .data()
                    .get("capture")
                    .unwrap_or(&serde_json::Value::Null),
            );
            let overlay = kairo_domain::overlay_defaults::normalize_overlay_settings(
                settings.data().get("overlay"),
            );
            let source =
                kairo_domain::capture::capture_source_for(&capture, &overlay.outputs).cloned();
            (capture, source)
        };
        let Some(source) = source else {
            return Ok(self.refuse_capture(
                "Add a screen in Screens first — a recording takes its picture from one".into(),
            ));
        };
        if !source.enabled {
            return Ok(self.refuse_capture(format!(
                "{} is off — turn it on in Screens to record it",
                source.name
            )));
        }
        if !self.0.engine.is_available(&source.id) {
            return Ok(self.refuse_capture(format!(
                "{} isn’t showing — check it in Screens",
                source.name
            )));
        }
        capture.folder = self.capture_folder().to_string_lossy().into_owned();
        let mut status = self.0.core.watch::<kairo_domain::capture::CaptureStatus>();
        status.latest();
        let sent = self
            .0
            .engine
            .command(kairo_engine_proto::Command::StartCapture {
                output: source.id,
                source_name: source.name,
                settings: kairo_engine_proto::Json::encode(&capture),
            });
        if !sent.accepted {
            return Err("The output engine is unavailable.".into());
        }
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            status
                .next()
                .await
                .map(|v| (*v).clone())
                .ok_or_else(|| "The recording engine stopped.".into())
        })
        .await
        .map_err(|_| "The recording engine did not respond.".to_owned())?
    }
    pub async fn stop_capture(&self) -> Result<kairo_domain::capture::CaptureStatus, String> {
        use kairo_core::shutdown::ShutdownServices;
        self.0.finish_capture().await?;
        Ok(self.capture_status())
    }
    pub async fn capture_audio_inputs(
        &self,
    ) -> Result<Vec<kairo_domain::capture::CaptureAudioInput>, String> {
        self.0
            .core
            .blocking(|| {
                #[cfg(target_os = "macos")]
                {
                    kairo_native_sys::recorder::audio_inputs()
                        .into_iter()
                        .map(|(id, name)| kairo_domain::capture::CaptureAudioInput { id, name })
                        .collect()
                }
                #[cfg(not(target_os = "macos"))]
                {
                    Vec::new()
                }
            })
            .await
            .map_err(|e| e.to_string())
    }
}
