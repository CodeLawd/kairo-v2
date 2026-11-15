//! The media dock's backend over the application's services (prompt 083):
//! `window.api.media` in Electron, the media service, push and playback
//! actions, posters and the in-process preview decoder.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kairo_domain::media::MediaLibrary;
use serde_json::Value;
use ui_media::backend::{
    FeedSink, Frame, ImportKind, Listening, MediaAnswer, MediaBackend, MediaCall, MediaFeed,
    PreviewSource, Reply,
};
use ui_operator::layout::UiState;

use crate::services::ApplicationServices;

pub struct ServicesMedia {
    services: Arc<ApplicationServices>,
    ui_state: Arc<dyn UiState>,
}

impl ServicesMedia {
    pub fn new(services: Arc<ApplicationServices>, ui_state: Arc<dyn UiState>) -> Self {
        Self { services, ui_state }
    }
}

struct Preview(kairo_media::preview::PreviewStream);

impl PreviewSource for Preview {
    fn next_frame(&mut self) -> Result<Option<Frame>, String> {
        self.0
            .next_frame()
            .map(|f| {
                f.map(|f| Frame {
                    width: f.frame.width,
                    height: f.frame.height,
                    pixels: f.frame.pixels,
                    seconds: f.seconds,
                })
            })
            .map_err(|e| e.to_string())
    }
    fn seek(&mut self, seconds: f64) -> Result<(), String> {
        self.0.seek(seconds).map_err(|e| e.to_string())
    }
    fn duration(&self) -> Option<f64> {
        self.0.duration()
    }
    fn interval(&self) -> f64 {
        self.0.interval()
    }
}

struct Feeds(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Feeds {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}

impl MediaBackend for ServicesMedia {
    fn library(&self) -> MediaLibrary {
        self.services
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library()
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let services = &self.services;
        let library = {
            let mut values = services.core.watch::<MediaLibrary>();
            let sink = sink.clone();
            services.runtime().spawn(async move {
                while let Some(value) = values.next().await {
                    sink(MediaFeed::Library((*value).clone()));
                }
            })
        };
        let imports = {
            let mut requests = services.core.watch::<crate::services::ImportRequested>();
            services.runtime().spawn(async move {
                use kairo_domain::import_menu::ImportKind as Menu;
                while let Some(request) = requests.next().await {
                    match request.0 {
                        Menu::Image => sink(MediaFeed::Import(ImportKind::Image)),
                        Menu::Video => sink(MediaFeed::Import(ImportKind::Video)),
                        _ => {}
                    }
                }
            })
        };
        Box::new(Feeds(vec![library, imports]))
    }

    fn call(&self, call: MediaCall, reply: Reply) {
        let services = self.services.clone();
        self.services
            .spawn(async move { reply(run(&services, call).await) });
    }

    fn open_preview(&self, path: &str, max_width: u32) -> Result<Box<dyn PreviewSource>, String> {
        let allowed = self.services.actions().media_path_allowed(Path::new(path));
        if !allowed {
            return Err("That file is outside the backgrounds folder.".into());
        }
        kairo_media::preview::PreviewStream::open(Path::new(path), max_width)
            .map(|s| Box::new(Preview(s)) as Box<dyn PreviewSource>)
            .map_err(|e| e.to_string())
    }

    fn read_ui(&self, key: &str) -> Option<Value> {
        self.ui_state.get(key)
    }

    fn write_ui(&self, key: &str, value: Value) {
        self.ui_state.set(key, value);
    }
}

async fn with_media<T: Send + 'static>(
    services: &Arc<ApplicationServices>,
    f: impl FnOnce(
        &mut kairo_media::library::MediaService<crate::services::Settings>,
    ) -> Result<T, String>
    + Send
    + 'static,
) -> Result<T, String> {
    let media = services.media.clone();
    services
        .core
        .blocking(move || f(&mut media.lock().unwrap_or_else(|e| e.into_inner())))
        .await
        .map_err(|e| e.to_string())?
}

async fn run(services: &Arc<ApplicationServices>, call: MediaCall) -> Result<MediaAnswer, String> {
    let actions = services.actions();
    let library = |l: MediaLibrary| Ok(MediaAnswer::Library(l));
    let text = |e: kairo_media::library::MediaError| e.to_string();
    match call {
        MediaCall::Push(id) => {
            let r = actions.push_media(&id).await?;
            Ok(MediaAnswer::Pushed {
                applied: r.applied,
                reason: r.reason,
            })
        }
        MediaCall::ChooseFolder => library(actions.choose_media_folder().await?),
        MediaCall::CreatePlaylist(name) => {
            library(with_media(services, move |m| Ok(m.create_playlist(&name))).await?)
        }
        MediaCall::RenamePlaylist { id, name } => {
            library(with_media(services, move |m| Ok(m.rename_playlist(&id, &name))).await?)
        }
        MediaCall::DeletePlaylist(id) => {
            library(with_media(services, move |m| Ok(m.delete_playlist(&id))).await?)
        }
        MediaCall::SetPlaylistItems { id, item_ids } => {
            library(with_media(services, move |m| Ok(m.set_playlist_items(&id, item_ids))).await?)
        }
        MediaCall::SetItemOrder(ids) => {
            library(with_media(services, move |m| Ok(m.set_item_order(ids))).await?)
        }
        MediaCall::DeleteItem(id) => library(actions.delete_media(&id).await?),
        MediaCall::RenameItem { id, name } => {
            library(with_media(services, move |m| m.rename_item(&id, &name).map_err(text)).await?)
        }
        MediaCall::RevealItem(id) => {
            let path = with_media(services, move |m| m.reveal_item(&id).map_err(text)).await?;
            crate::handlers::open_path(&path, true)?;
            Ok(MediaAnswer::Done)
        }
        MediaCall::CopyItems(ids) => {
            with_media(services, move |m| m.copy_items(&ids).map_err(text)).await?;
            Ok(MediaAnswer::Done)
        }
        MediaCall::CutItems { ids, from_playlist } => {
            with_media(services, move |m| {
                m.cut_items(&ids, from_playlist.as_deref()).map_err(text)
            })
            .await?;
            Ok(MediaAnswer::Done)
        }
        MediaCall::PasteItems(playlist) => library(
            with_media(services, move |m| {
                m.paste_items(playlist.as_deref()).map_err(text)
            })
            .await?,
        ),
        MediaCall::ClipboardHasFiles => Ok(MediaAnswer::Flag(
            with_media(services, |m| Ok(m.clipboard_has_files())).await?,
        )),
        MediaCall::AddMediaToPlaylist(id) => library(actions.add_backgrounds(&id).await?),
        MediaCall::ImportFiles(kind) => library(
            actions
                .import_media(match kind {
                    ImportKind::Image => "image",
                    ImportKind::Video => "video",
                })
                .await?,
        ),
        MediaCall::ImportPaths { paths, playlist } => {
            library(actions.import_media_files(playlist, paths).await?)
        }
        MediaCall::SetPlayback {
            id,
            patch,
            next_cue,
        } => library(actions.set_media_playback(&id, &patch, next_cue)?),
        MediaCall::GetLibrary => library(with_media(services, |m| Ok(m.library())).await?),
        MediaCall::Poster(id) => {
            let path = with_media(services, move |m| {
                m.item(&id)
                    .map(|i| PathBuf::from(&i.path))
                    .ok_or_else(|| "That background is no longer in the folder.".to_owned())
            })
            .await?;
            let cache = services.paths().user_data.join("media-posters");
            let poster =
                tokio::task::spawn_blocking(move || kairo_media::poster::poster_for(&cache, &path))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| e.to_string())?;
            Ok(MediaAnswer::Poster {
                image: poster.image,
                duration: poster.duration_seconds,
            })
        }
    }
}

/// The dock entity shared by every booth page; the header's Media button
/// toggles it and shows what is live.
pub fn install(
    backend: Arc<dyn MediaBackend>,
    shell: &gpui_kit::Entity<ui_shell::Shell>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_media::MediaDockView> {
    use gpui_kit::AppContext;
    let height = f32::from(window.viewport_size().height);
    let model = cx.new(|cx| ui_media::MediaDock::new(backend, height, cx));
    let view = cx.new(|cx| ui_media::MediaDockView::new(model.clone(), window, cx));
    let target = shell.downgrade();
    let header = model.clone();
    cx.subscribe(&model, move |_, _: &ui_media::MediaEvent, cx| {
        let (open, live) = {
            let m = header.read(cx);
            (m.is_open(), m.live_name())
        };
        let _ = target.update(cx, |s, cx| s.set_media(open, live, cx));
    })
    .detach();
    let toggle = model.clone();
    cx.subscribe(
        shell,
        move |_, request: &ui_shell::ShellRequest, cx| match request {
            ui_shell::ShellRequest::ToggleMedia => toggle.update(cx, |m, cx| m.toggle(cx)),
            ui_shell::ShellRequest::WindowFocused => {
                toggle.update(cx, |m, cx| m.window_focused(cx))
            }
            _ => {}
        },
    )
    .detach();
    view
}
