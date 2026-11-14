//! The Lyrics page's backend over the application's services (prompt 080):
//! `window.api.lyrics`, `setlist`, `libraries`, `songUsage` and `transfer`
//! in Electron, and the renderer stores the page read.

use std::path::Path;
use std::sync::Arc;

use kairo_core::dialogs::{DialogKind, dialog_request};
use kairo_domain::kairo_bundle::TransferExportRequest;
use kairo_lyrics::online::OnlineLyrics;
use kairo_lyrics::online::http::ReqwestHttp;
use serde_json::Value;
use ui_lyrics::backend::{
    FeedSink, KairoExport, Listening, LyricsAnswer, LyricsBackend, LyricsCall, LyricsFeed,
    LyricsSnapshot, OnlineSource, Reply,
};

use crate::services::ApplicationServices;

pub struct ServicesLyrics {
    services: Arc<ApplicationServices>,
    online: Arc<OnlineLyrics>,
}

impl ServicesLyrics {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        let online = OnlineLyrics::new(
            Arc::new(ReqwestHttp::default()),
            Arc::new(services.licensing.clone()),
        );
        Self { services, online }
    }
}

/// `store.get('stt').deepseekApiKey`: keys the review snapshot cache (the
/// managed gateway ignores caller keys, DECISIONS 041).
fn review_credential(services: &ApplicationServices) -> String {
    let all: Value = services
        .settings_wiring
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_all();
    all.pointer("/stt/deepseekApiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn source(s: OnlineSource) -> kairo_lyrics::online::OnlineSource {
    kairo_lyrics::online::OnlineSource {
        provider: s.provider,
        url: s.url,
        title: s.title,
        artist: s.artist,
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default()
}

impl LyricsBackend for ServicesLyrics {
    fn snapshot(&self) -> LyricsSnapshot {
        let services = &self.services;
        LyricsSnapshot {
            // The library is read off the UI thread and arrives as a feed.
            songs: Vec::new(),
            libraries: services.libraries.snapshot().songs,
            setlists: services.setlist.snapshot(),
        }
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = self.services.clone();
        let songs = sink.clone();
        let task = services.runtime().spawn({
            let services = services.clone();
            async move {
                match services.lyrics.get_library().await {
                    Ok(library) => songs(LyricsFeed::Songs(library)),
                    Err(error) => tracing::error!(%error, "[Lyrics] library read failed"),
                }
            }
        });
        let libraries = sink.clone();
        services
            .libraries
            .on_changed(move |state| libraries(LyricsFeed::Libraries(state.songs.clone())));
        let setlists = sink;
        services
            .setlist
            .on_changed(move |state| setlists(LyricsFeed::Setlists(state.clone())));
        struct Feed(tokio::task::JoinHandle<()>);
        impl Drop for Feed {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        Box::new(Feed(task))
    }

    fn call(&self, call: LyricsCall, reply: Reply) {
        let services = self.services.clone();
        let online = self.online.clone();
        self.services.spawn(async move {
            reply(run(&services, &online, call).await);
        });
    }
}

async fn run(
    services: &Arc<ApplicationServices>,
    online: &Arc<OnlineLyrics>,
    call: LyricsCall,
) -> Result<LyricsAnswer, String> {
    let text = |e: &dyn std::fmt::Display| e.to_string();
    let lyrics = &services.lyrics;
    match call {
        LyricsCall::Search(query) => lyrics
            .search(&query)
            .await
            .map(LyricsAnswer::Songs)
            .map_err(|e| text(&e)),
        LyricsCall::SearchOnline(query) => {
            services
                .telemetry
                .track(kairo_domain::usage::UsageFeature::SongOnlineSearch, 1.0)
                .await;
            lyrics
                .search_online(online, &query, &review_credential(services))
                .await
                .map(LyricsAnswer::Online)
                .map_err(|e| text(&e))
        }
        LyricsCall::PreviewOnline(s) => lyrics
            .preview_online(online, &source(s), &review_credential(services))
            .await
            .map(LyricsAnswer::Preview)
            .map_err(|e| text(&e)),
        LyricsCall::ImportOnline(s) => {
            services
                .telemetry
                .track(kairo_domain::usage::UsageFeature::SongImport, 1.0)
                .await;
            lyrics
                .import_online(online, &source(s), &review_credential(services))
                .await
                .map(LyricsAnswer::Song)
                .map_err(|e| text(&e))
        }
        LyricsCall::ToggleFavorite(id) => lyrics
            .toggle_favorite(&id)
            .await
            .map(LyricsAnswer::Favorite)
            .map_err(|e| text(&e)),
        LyricsCall::Delete(id) => {
            kairo_transfer::delete_song(lyrics, &services.setlist, &services.libraries, &id)
                .await
                .map_err(|e| text(&e))?;
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::Setlist(command) => services
            .setlist
            .apply(command)
            .map(LyricsAnswer::Setlists)
            .map_err(|e| text(&e)),
        LyricsCall::Libraries(command) => services
            .libraries
            .apply(command)
            .map(|state| LyricsAnswer::Libraries(state.songs))
            .map_err(|e| text(&e)),
        LyricsCall::UsageList { from, to } => Ok(LyricsAnswer::Usage(
            services.song_usage.list(Some(from), Some(to)),
        )),
        LyricsCall::UsageExport { from, to } => {
            let name = kairo_transfer::song_usage_file_name(now_ms());
            let picked = services
                .ui
                .dialog(dialog_request(
                    DialogKind::ExportSongUsage,
                    Some(name.into()),
                ))
                .await?;
            let saved = services
                .song_usage
                .export_csv(
                    Some(from),
                    Some(to),
                    &chrono::Local,
                    picked.into_iter().next(),
                )
                .await
                .map_err(|e| text(&e))?;
            Ok(LyricsAnswer::Saved(
                saved.map(|p| p.to_string_lossy().into_owned()),
            ))
        }
        LyricsCall::ExportKairo(export) => {
            let request = match export {
                KairoExport::Songs(song_ids) => TransferExportRequest::Songs {
                    song_ids,
                    name: None,
                },
                KairoExport::Setlist(list_id) => TransferExportRequest::Setlist { list_id },
                KairoExport::Library => TransferExportRequest::Library,
            };
            services
                .transfer
                .export(request, &services.ui)
                .await
                .map_err(|e| text(&e))?;
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::ImportKairo => {
            if let Some(preview) = services
                .transfer
                .pick_and_preview(&services.ui)
                .await
                .map_err(|e| text(&e))?
            {
                services
                    .core
                    .publisher()
                    .emit(kairo_transfer::KairoOpened::Preview(Box::new(preview)));
            }
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::OpenUrl(url) => {
            // Only web pages: a hit's URL comes from a remote catalogue.
            if !(url.starts_with("https://") || url.starts_with("http://")) {
                return Err("That link cannot be opened.".into());
            }
            crate::handlers::open_path(Path::new(&url), false)?;
            Ok(LyricsAnswer::Done)
        }
    }
}

/// Creates the page, shares the operator's live rail, registers the ⌘F
/// finder and answers what the page asks of the application.
pub fn install(
    backend: Arc<dyn LyricsBackend>,
    operator: &gpui_kit::Entity<ui_operator::OperatorWorkspace>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_lyrics::LyricsWorkspace> {
    use gpui_kit::AppContext;
    use gpui_kit::component::WindowExt;
    use ui_lyrics::LyricsEvent;
    let page = cx.new(|cx| ui_lyrics::LyricsWorkspace::new(backend, window, cx));
    let rail = operator.read(cx).rail().clone();
    page.update(cx, |p, cx| p.set_rail(Some(rail.into()), cx));
    let model = page.read(cx).model().clone();
    let target = shell.downgrade();
    let handle = window.window_handle();
    cx.subscribe(&model, move |_, event: &LyricsEvent, cx| {
        let event = event.clone();
        let target = target.clone();
        let _ = handle.update(cx, move |_, window, cx| match event {
            LyricsEvent::Notify(text) => window.push_notification(
                gpui_kit::component::notification::Notification::success(text),
                cx,
            ),
            LyricsEvent::Opened(_) => {
                let _ = target.update(cx, |s, cx| {
                    s.close_overlay(cx);
                    s.navigate(ui_shell::Route::Lyrics, cx);
                });
            }
            // The song editor is prompt 082's.
            LyricsEvent::NewSong => tracing::info!("[Lyrics] new song requested (editor: 082)"),
        });
    })
    .detach();
    let finder_model = model.clone();
    let close = shell.downgrade();
    shell.update(cx, |shell, _| {
        shell.set_overlay_builder(
            ui_shell::Overlay::SongSearch,
            Box::new(move |_, window, cx| {
                let model = finder_model.clone();
                let finder = cx.new(|cx| ui_lyrics::QuickOpen::new(model, window, cx));
                let close = close.clone();
                cx.subscribe(&finder, move |_, _: &ui_lyrics::QuickOpenEvent, cx| {
                    let _ = close.update(cx, |s, cx| s.close_overlay(cx));
                })
                .detach();
                finder.into()
            }),
        )
    });
    page
}
