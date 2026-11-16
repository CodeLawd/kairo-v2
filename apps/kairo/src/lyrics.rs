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
    LyricsSnapshot, OnlineSource, Reply, SongFile,
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

/// The main output's lyrics-theme background, for the cards.
fn card_background_of(all: &Value) -> ui_lyrics::backend::CardBackground {
    let Ok(settings) = serde_json::from_value::<kairo_domain::settings::AppSettings>(all.clone())
    else {
        return Default::default();
    };
    let b = crate::scripture::card_background(
        &settings.overlay.outputs,
        kairo_domain::overlay::OverlayContentKind::Lyrics,
    );
    ui_lyrics::backend::CardBackground {
        color: b.color,
        image: b.image,
        opacity: b.opacity,
    }
}

/// `lyrics.glossColor`, normalised.
fn gloss_of(all: &Value) -> String {
    kairo_domain::lyrics::style::normalize_gloss_color(
        all.pointer("/lyrics/glossColor").and_then(Value::as_str),
    )
}

/// Reads song files for the import queue, one key per name, size and time.
fn read_song_files(paths: &[std::path::PathBuf]) -> Vec<SongFile> {
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let meta = std::fs::metadata(path).ok();
            let size = meta.as_ref().map_or(0, |m| m.len());
            let modified = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis());
            let content = std::fs::read(path)
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                .map_err(|e| e.to_string());
            SongFile {
                key: format!("{name}:{size}:{modified}"),
                name,
                content,
            }
        })
        .collect()
}

/// `stt.llmProvider` picks the translation model (managed either way).
fn llm_of(all: &Value) -> kairo_lyrics::online::translate::LlmProvider {
    use kairo_lyrics::online::translate::LlmProvider;
    match all.pointer("/stt/llmProvider").and_then(Value::as_str) {
        Some("deepseek") => LlmProvider::Deepseek,
        _ => LlmProvider::Anthropic,
    }
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
        let all = services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        LyricsSnapshot {
            card_background: card_background_of(&all),
            // The library is read off the UI thread and arrives as a feed.
            songs: Vec::new(),
            libraries: services.libraries.snapshot().songs,
            setlists: services.setlist.snapshot(),
            gloss_color: gloss_of(
                &services
                    .settings_wiring
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_all(),
            ),
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
        let gloss = {
            let mut values = services.core.watch::<crate::services::SettingsChanged>();
            let sink = sink.clone();
            services.runtime().spawn(async move {
                while let Some(value) = values.next().await {
                    let crate::services::SettingsChanged(value) = &*value;
                    sink(LyricsFeed::GlossColor(gloss_of(value)));
                    sink(LyricsFeed::CardBackground(card_background_of(value)));
                }
            })
        };
        let import = {
            let mut requests = services.core.watch::<crate::services::ImportRequested>();
            let sink = sink.clone();
            services.runtime().spawn(async move {
                while let Some(request) = requests.next().await {
                    if request.0 == kairo_domain::import_menu::ImportKind::Lyrics {
                        sink(LyricsFeed::OpenImport);
                    }
                }
            })
        };
        // A `.kairo` import changed the library (`commitKairoImport`).
        let committed = {
            let mut done = services.core.watch::<crate::transfer::TransferCommitted>();
            let sink = sink.clone();
            let services = services.clone();
            services.clone().runtime().spawn(async move {
                while done.next().await.is_some() {
                    match services.lyrics.get_library().await {
                        Ok(library) => sink(LyricsFeed::Songs(library)),
                        Err(error) => tracing::error!(%error, "[Lyrics] library read failed"),
                    }
                }
            })
        };
        let libraries = sink.clone();
        services
            .libraries
            .on_changed(move |state| libraries(LyricsFeed::Libraries(state.songs.clone())));
        let setlists = sink;
        services
            .setlist
            .on_changed(move |state| setlists(LyricsFeed::Setlists(state.clone())));
        struct Feed(Vec<tokio::task::JoinHandle<()>>);
        impl Drop for Feed {
            fn drop(&mut self) {
                for task in &self.0 {
                    task.abort();
                }
            }
        }
        Box::new(Feed(vec![task, gloss, import, committed]))
    }

    fn misspelled(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        crate::spell::misspelled(text)
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
                KairoExport::Pick(ids) => {
                    services
                        .core
                        .publisher()
                        .emit(crate::transfer::PickSongs(ids));
                    return Ok(LyricsAnswer::Done);
                }
                KairoExport::Songs(song_ids) => TransferExportRequest::Songs {
                    song_ids,
                    name: None,
                },
                KairoExport::Setlist(list_id) => TransferExportRequest::Setlist { list_id },
                KairoExport::Library => TransferExportRequest::Library,
            };
            crate::transfer::export(services, request).await;
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::ImportKairo => {
            crate::transfer::pick(services).await;
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::PushSlide { song_id, index } => {
            services
                .actions()
                .push_lyrics(&song_id, index as f64)
                .await?;
            Ok(LyricsAnswer::Done)
        }
        LyricsCall::Update(song) => {
            let id = song.id.clone();
            lyrics
                .update_song(&id, song)
                .await
                .map_err(|e| text(&e))?
                .map(LyricsAnswer::Song)
                .ok_or_else(String::new)
        }
        LyricsCall::Import(source) => {
            services
                .telemetry
                .track(kairo_domain::usage::UsageFeature::SongImport, 1.0)
                .await;
            match source {
                kairo_domain::lyrics::LyricsImportSource::Online {
                    provider,
                    url,
                    title,
                    artist,
                } => {
                    lyrics
                        .import_online(
                            online,
                            &kairo_lyrics::online::OnlineSource {
                                provider,
                                url,
                                title,
                                artist,
                            },
                            &review_credential(services),
                        )
                        .await
                }
                other => lyrics.import_song(other).await,
            }
            .map(LyricsAnswer::Song)
            .map_err(|e| text(&e))
        }
        LyricsCall::PreviewFile(source) => lyrics
            .preview_import(source)
            .await
            .map(LyricsAnswer::ImportPreview)
            .map_err(|e| text(&e)),
        LyricsCall::GetSong(id) => lyrics
            .get_song(&id)
            .await
            .map_err(|e| text(&e))?
            .map(LyricsAnswer::Song)
            .ok_or_else(|| format!("Song not found: {id}")),
        LyricsCall::Translate {
            sections,
            source_language,
            title,
            artist,
        } => {
            let all = services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_all();
            let options = kairo_lyrics::online::translate::TranslateOptions {
                target: Some("en".into()),
                source_language: Some(source_language),
                title: Some(title),
                artist: Some(artist),
                llm: Some(llm_of(&all)),
            };
            online
                .translate_sections(&sections, &options)
                .await
                .map(LyricsAnswer::Sections)
        }
        LyricsCall::PickSongFiles => {
            let request = kairo_core::dialogs::DialogRequest {
                title: Some("Import song files"),
                button_label: Some("Import"),
                directory: false,
                create_directory: false,
                multiple: true,
                save: false,
                filters: vec![kairo_core::dialogs::FileFilter {
                    name: "Song files",
                    extensions: &["usr", "txt", "rtf"],
                }],
                default_path: None,
            };
            let paths = services.ui.dialog(request).await?;
            Ok(LyricsAnswer::Files(
                tokio::task::spawn_blocking(move || read_song_files(&paths))
                    .await
                    .map_err(|e| e.to_string())?,
            ))
        }
        LyricsCall::ReadSongFiles(paths) => Ok(LyricsAnswer::Files(
            tokio::task::spawn_blocking(move || read_song_files(&paths))
                .await
                .map_err(|e| e.to_string())?,
        )),
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
            // The page asks this itself (its discard dialog).
            LyricsEvent::ConfirmDiscard => {}
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
