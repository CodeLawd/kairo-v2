//! `.kairo` transfer over the application's services (prompt 091):
//! `transfer.export`, `pickAndPreview`, `commit`, `discard`, `onOpened`,
//! and File → Import → Kairo file.

use std::sync::Arc;

use kairo_domain::import_menu::ImportKind;
use kairo_domain::kairo_bundle::{
    TransferCommitRequest, TransferCommitResult, TransferExportRequest, TransferExportResult,
};
use kairo_transfer::KairoOpened;
use ui_transfer::model::{Notice, export_notice, open_error_notice};
use ui_transfer::{FeedSink, Listening, Reply, SongRow, TransferFeed};

use crate::services::{ApplicationServices, ImportRequested};

/// A result line for the transfer notice.
#[derive(Clone, Debug)]
pub struct TransferNotice(pub Notice);

/// "Export songs…": open the picker with these ticked.
#[derive(Clone, Debug)]
pub struct PickSongs(pub Vec<String>);

/// An import changed the song library or the sermon plans; pages reload them.
#[derive(Clone, Debug)]
pub struct TransferCommitted;

/// `exportKairo`: the save dialog and the file; the outcome goes to the
/// notice, so callers have nothing to show.
pub async fn export(services: &ApplicationServices, request: TransferExportRequest) {
    let notice = match services.transfer.export(request, &services.ui).await {
        Ok(result) if result.saved => export_notice(result.file_name.as_deref().unwrap_or("")),
        Ok(_) => return,
        Err(error) => Notice::error(error.to_string()),
    };
    services.core.publisher().emit(TransferNotice(notice));
}

/// `importKairo`: the open dialog, then the review.
pub async fn pick(services: &ApplicationServices) {
    match services.transfer.pick_and_preview(&services.ui).await {
        Ok(Some(preview)) => services
            .core
            .publisher()
            .emit(KairoOpened::Preview(Box::new(preview))),
        Ok(None) => {}
        Err(error) => services
            .core
            .publisher()
            .emit(TransferNotice(Notice::error(error.to_string()))),
    }
}

pub struct ServicesTransfer {
    services: Arc<ApplicationServices>,
}

impl ServicesTransfer {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        Self { services }
    }
}

struct Tasks(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Tasks {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}

fn rows(songs: Vec<kairo_domain::lyrics::LyricsSong>) -> Vec<SongRow> {
    songs
        .into_iter()
        .map(|s| SongRow {
            id: s.id,
            title: s.title,
            artist: s.artist,
        })
        .collect()
}

impl ui_transfer::TransferBackend for ServicesTransfer {
    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let runtime = services.runtime();
        let mut tasks = Vec::new();
        tasks.push(runtime.spawn({
            let mut opened = services.core.watch::<KairoOpened>();
            let sink = sink.clone();
            async move {
                while let Some(value) = opened.next().await {
                    sink(match &*value {
                        KairoOpened::Preview(preview) => TransferFeed::Opened(preview.clone()),
                        KairoOpened::Error { error, file_name } => {
                            TransferFeed::Notice(open_error_notice(file_name, error))
                        }
                    });
                }
            }
        }));
        tasks.push(runtime.spawn({
            let mut notices = services.core.watch::<TransferNotice>();
            let sink = sink.clone();
            async move {
                while let Some(value) = notices.next().await {
                    sink(TransferFeed::Notice(value.0.clone()));
                }
            }
        }));
        tasks.push(runtime.spawn({
            let mut picks = services.core.watch::<PickSongs>();
            let sink = sink.clone();
            async move {
                while let Some(value) = picks.next().await {
                    sink(TransferFeed::PickSongs(value.0.clone()));
                }
            }
        }));
        // File → Import → Kairo file (`useImportRequest(['kairo'])`).
        tasks.push(runtime.spawn({
            let mut requests = services.core.watch::<ImportRequested>();
            let services = services.clone();
            async move {
                while let Some(request) = requests.next().await {
                    if request.0 == ImportKind::Kairo {
                        pick(&services).await;
                    }
                }
            }
        }));
        tasks.push(runtime.spawn({
            let mut committed = services.core.watch::<TransferCommitted>();
            let services = services.clone();
            async move {
                while committed.next().await.is_some() {
                    if let Ok(songs) = services.lyrics.get_library().await {
                        sink(TransferFeed::Songs(rows(songs)));
                    }
                }
            }
        }));
        Box::new(Tasks(tasks))
    }

    fn songs(&self, reply: Reply<Vec<SongRow>>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(
                services
                    .lyrics
                    .get_library()
                    .await
                    .map(rows)
                    .map_err(|e| e.to_string()),
            );
        });
    }

    fn export(&self, request: TransferExportRequest, reply: Reply<TransferExportResult>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            reply(
                services
                    .transfer
                    .export(request, &services.ui)
                    .await
                    .map_err(|e| e.to_string()),
            );
        });
    }

    fn commit(&self, request: TransferCommitRequest, reply: Reply<TransferCommitResult>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let result = services
                .transfer
                .commit(request, |_| {})
                .await
                .map_err(|e| e.to_string());
            if result.is_ok() {
                services.core.publisher().emit(TransferCommitted);
            }
            reply(result);
        });
    }

    fn discard(&self, token: String) {
        self.services.transfer.discard(&token);
    }
}

/// Mounts the host over the operator window.
pub fn install(
    services: &Arc<ApplicationServices>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    use gpui_kit::AppContext;
    let backend: Arc<dyn ui_transfer::TransferBackend> =
        Arc::new(ServicesTransfer::new(services.clone()));
    let host = cx.new(|cx| ui_transfer::TransferHost::new(backend, window, cx));
    let target = shell.downgrade();
    cx.observe(&host, move |host, cx| {
        let open = host.read(cx).is_open();
        let _ = target.update(cx, |s, cx| s.set_covered(open, cx));
    })
    .detach();
    shell.update(cx, |s, cx| s.set_transfer(Some(host.into()), cx));
}
