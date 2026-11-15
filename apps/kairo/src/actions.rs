//! Product effects formerly in IPC, above the service dependency graph.

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn song() -> kairo_domain::lyrics::LyricsSong {
        serde_json::from_value(json!({"id":"song", "title":"Grace", "artist":"", "sections":[{"type":"verse", "label":"Verse 1", "lines":["First", "", "Second"]}], "source":"text", "isFavorite":false, "createdAt":0, "updatedAt":0})).unwrap()
    }
    #[test]
    fn clicked_grid_index_builds_current_and_following_slide() {
        let push = lyric_slide(&song(), 0.0, None).unwrap();
        assert_eq!(push.reference, "Grace — Verse 1 (1/2)");
        assert_eq!(push.text, "First");
        assert_eq!(push.next.unwrap().text, "Second");
        assert_eq!(push.colored[0].text, "First");
        assert!(lyric_slide(&song(), 1.0, None).unwrap().next.is_none());
    }
    #[test]
    fn missing_fractional_negative_and_nonfinite_slide_indices_fail() {
        for index in [-1.0, 0.5, 2.0, f64::NAN, f64::INFINITY] {
            assert!(lyric_slide(&song(), index, None).is_err(), "{index}");
        }
    }
}

use kairo_core::AppCore;
use kairo_domain::{
    documents::{DocumentPlaybackStatus, DocumentVideoCommand},
    lyrics::{LyricsSong, SongPresentOptions, slides::build_slides, style::normalize_gloss_color},
    media::{MediaKind, MediaLibrary, media_filter_css},
    overlay::OverlayBackgroundType,
    overlay_defaults::normalize_overlay_settings,
    overlay_outputs::{is_local_kind, surface_outputs},
    program::ProgramSlideInfo,
};
use kairo_engine_proto::{Command, Json, Query, Reply, VideoTransport};
use kairo_orchestrator::{
    clients::{ColoredLine, EngineClient},
    engine::EngineSurfaces,
    orchestrator::Orchestrator,
};
use kairo_store::{settings::SettingsStore, workspace::Workspace};
use serde_json::Value;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

pub type SharedMedia = Arc<Mutex<kairo_media::library::MediaService<Arc<Mutex<SettingsStore>>>>>;

pub struct Actions {
    pub settings: Arc<Mutex<SettingsStore>>,
    pub lyrics: Arc<kairo_lyrics::LyricsService>,
    pub media: SharedMedia,
    pub documents: Arc<dyn ActionDocuments>,
    pub libraries: kairo_transfer::LibrariesService,
    pub song_usage: kairo_transfer::SongUsageService,
    pub program: Arc<kairo_output::ProgramService>,
    pub orchestrator: Arc<dyn Presenter>,
    pub usage: Arc<dyn kairo_orchestrator::clients::UsageClient>,
    pub scripture: kairo_scripture::ScriptureService,
    pub surfaces: Arc<dyn ActionSurfaces>,
    pub core: AppCore,
    pub dialogs: Arc<dyn crate::dialogs::NativeDialogs>,
    pub tracks: Arc<Mutex<kairo_media::tracks::TracksService<Arc<Mutex<SettingsStore>>>>>,
    pub picked: Arc<kairo_media::allowlist::PickedMediaAllowlist>,
}

impl Actions {
    pub async fn push_lyrics(&self, song_id: &str, index: f64) -> Result<(), String> {
        self.usage.track("lyrics_slide");
        let song = self
            .lyrics
            .get_song(song_id)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Song not found: {song_id}"))?;
        let gloss = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .data()
            .get("lyrics")
            .and_then(|v| v.get("glossColor"))
            .and_then(|v| v.as_str())
            .map(str::to_owned);
        let push = lyric_slide(&song, index, gloss.as_deref())?;
        self.orchestrator
            .present_lyric_slide(push.reference, push.text, Some(push.colored), push.next)
            .await?;
        self.song_usage.record(&song);
        Ok(())
    }

    pub async fn push_document(&self, id: &str, page: f64) -> Result<bool, String> {
        self.usage.track("document_page");
        let docs = self.documents.clone();
        let document_id = id.to_owned();
        let slide = self
            .core
            .blocking(move || docs.slide(&document_id, page))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let applied = self
            .orchestrator
            .present_document_page(slide.path.clone(), Some(slide))
            .await;
        self.media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_live_item(None);
        self.program.set_slide_info(
            Some(ProgramSlideInfo {
                reference: format!("Page {}", kairo_domain::js::number_to_string(page + 1.0)),
                text: String::new(),
            }),
            None,
        );
        if applied {
            let docs = self.documents.clone();
            let id = id.to_owned();
            let surfaces = self.surfaces.clone();
            // End-of-deck and decoding errors are intentionally ignored here.
            tokio::task::spawn_blocking(move || {
                if let Ok(next) = docs.page(&id, page + 1.0) {
                    for output in surfaces.all_surfaces() {
                        surfaces.engine().command(Command::PreloadPage {
                            output,
                            media_path: next.to_string_lossy().into_owned(),
                        });
                    }
                }
            });
        }
        Ok(applied)
    }

    pub async fn remove_document(
        &self,
        id: &str,
    ) -> Result<Vec<kairo_domain::documents::ProjectionDocument>, String> {
        self.libraries
            .forget_item(kairo_domain::collections::LibraryKind::Documents, id);
        let docs = self.documents.clone();
        let id = id.to_owned();
        self.core
            .blocking(move || docs.remove(&id))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub async fn push_media(&self, id: &str) -> Result<MediaPushResult, String> {
        self.usage.track("media_live");
        {
            let mut media = self.media.lock().unwrap_or_else(|e| e.into_inner());
            if media.item(id).is_none() {
                return Err("That background is no longer in the folder.".into());
            }
            media.set_live_item(Some(id));
        }
        let applied = self.orchestrator.present_live_background().await;
        Ok(MediaPushResult {
            applied,
            reason: if applied {
                None
            } else {
                Some(self.background_target_problem().await)
            },
        })
    }

    pub async fn background_target_problem(&self) -> String {
        let overlay = {
            let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            normalize_overlay_settings(settings.data().get("overlay"))
        };
        let outputs: Vec<_> = surface_outputs(&overlay.outputs)
            .into_iter()
            .filter(|o| o.show.backgrounds)
            .collect();
        if outputs.is_empty() {
            return "No screen is on".into();
        }
        if let Some(screen) = outputs.iter().find(|o| is_local_kind(o.kind)) {
            let state = self
                .core
                .call(kairo_output::actor::GetOutputState {
                    output: (*screen).clone(),
                })
                .await;
            let reason = state
                .ok()
                .map(|s| s.reason)
                .unwrap_or_else(|| "undefined".into());
            return format!("{}: {reason}", screen.name);
        }
        "The NDI feed is not available".into()
    }

    async fn clear_background(&self) {
        let mut background = {
            let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            normalize_overlay_settings(settings.data().get("overlay"))
                .theme
                .background
        };
        background.kind = OverlayBackgroundType::Transparent;
        background.media_path = Some(String::new());
        for output in self.surfaces.all_surfaces() {
            self.surfaces
                .set_background(output, background.clone())
                .await;
        }
    }

    pub async fn clear_media(&self) -> MediaLibrary {
        self.media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_live_item(None);
        self.clear_background().await;
        self.media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library()
    }

    pub async fn delete_media(&self, id: &str) -> Result<MediaLibrary, String> {
        let (live, library) = {
            let mut media = self.media.lock().unwrap_or_else(|e| e.into_inner());
            let live = media.live_item().is_some_and(|item| item.id == id);
            let library = media.delete_item(id).map_err(|e| e.to_string())?;
            (live, library)
        };
        if live {
            self.clear_background().await;
        }
        Ok(library)
    }

    pub fn set_media_playback(
        &self,
        id: &str,
        patch: &serde_json::Map<String, serde_json::Value>,
        next_cue: bool,
    ) -> Result<MediaLibrary, String> {
        let mut media = self.media.lock().unwrap_or_else(|e| e.into_inner());
        if next_cue && media.item(id).is_none() {
            return Err("This item is no longer in the Media Dock.".into());
        }
        let library = media
            .set_playback(id, patch, next_cue)
            .map_err(|e| e.to_string())?;
        if !next_cue
            && media.live_item().is_some_and(|item| item.id == id)
            && let Some(playback) = media.live_playback()
        {
            for output in self.surfaces.all_surfaces() {
                self.surfaces.engine().command(Command::PatchBackground {
                    output,
                    playback: Json::encode(&playback),
                    filter: media_filter_css(&playback),
                });
            }
        }
        Ok(library)
    }

    pub fn set_media_paused(&self, paused: bool) -> MediaLibrary {
        let library = self
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_live_paused(paused);
        for output in self.surfaces.all_surfaces() {
            self.surfaces.engine().command(Command::VideoTransport {
                output,
                transport: VideoTransport::Pause(library.live_paused),
            });
        }
        library
    }

    pub fn seek_media(&self, seconds: f64) {
        if !self
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .live_item()
            .is_some_and(|item| item.kind == MediaKind::Video)
        {
            return;
        }
        for output in self.surfaces.all_surfaces() {
            self.surfaces.engine().command(Command::VideoTransport {
                output,
                transport: VideoTransport::Seek(seconds),
            });
        }
    }

    pub async fn document_playback(&self, id: &str, page: f64) -> Option<DocumentPlaybackStatus> {
        for output in self.surfaces.all_surfaces() {
            if let Some(Reply::DocumentPlayback(Some(json))) = self
                .surfaces
                .engine()
                .query(Query::DocumentPlayback { output })
                .await
                && let Ok(state) = json.decode::<DocumentPlaybackStatus>()
                && state.id == id
                && state.page == page
            {
                return Some(state);
            }
        }
        None
    }

    pub async fn control_document(
        &self,
        id: &str,
        page: f64,
        video: f64,
        command: DocumentVideoCommand,
    ) -> Result<(), String> {
        if !page.is_finite()
            || page.fract() != 0.0
            || !video.is_finite()
            || video.fract() != 0.0
            || video < 0.0
            || video > u32::MAX as f64
            || matches!(command, DocumentVideoCommand::Seek { seconds } if !seconds.is_finite())
        {
            return Err("Invalid document video command.".into());
        }
        let mut applied = false;
        for output in self.surfaces.all_surfaces() {
            // A successful enqueue alone is not evidence this document is live.
            if let Some(Reply::DocumentPlayback(Some(json))) = self
                .surfaces
                .engine()
                .query(Query::DocumentPlayback {
                    output: output.clone(),
                })
                .await
                && let Ok(state) = json.decode::<DocumentPlaybackStatus>()
                && state.id == id
                && state.page == page
                && (video as usize) < state.videos.len()
            {
                applied |= self
                    .surfaces
                    .engine()
                    .command(Command::ControlDocumentVideo {
                        output,
                        id: id.into(),
                        page,
                        video: video as u32,
                        command: Json::encode(&command),
                    })
                    .accepted;
            }
        }
        if applied {
            Ok(())
        } else {
            Err("Push this slide before playing its video.".into())
        }
    }

    pub async fn set_auto_mode(&self, enabled: bool) {
        self.scripture.set_auto_mode(enabled);
        self.orchestrator.set_auto_mode(enabled).await;
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .merge_fields("scripture", [("autoMode", serde_json::json!(enabled))]);
    }

    pub async fn set_confidence(&self, threshold: f64) {
        self.scripture.set_confidence_threshold(threshold);
        self.orchestrator.set_confidence_threshold(threshold).await;
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .merge_fields(
                "scripture",
                [("confidenceThreshold", serde_json::json!(threshold))],
            );
    }

    pub async fn change_workspace(
        &self,
        workspace: &mut Workspace,
        folder: &Path,
        move_contents: bool,
    ) -> Result<kairo_domain::workspace::WorkspaceInfo, String> {
        let info = workspace
            .set_root(
                &mut self.settings.lock().unwrap_or_else(|e| e.into_inner()),
                folder,
                move_contents,
            )
            .map_err(|e| e.to_string())?;
        self.lyrics
            .sync_from_folder()
            .await
            .map_err(|e| e.to_string())?;
        let media = self.media.clone();
        self.core
            .blocking(move || media.lock().unwrap_or_else(|e| e.into_inner()).scan())
            .await
            .map_err(|e| e.to_string())?;
        Ok(info)
    }
}

#[derive(Debug, PartialEq)]
pub struct MediaPushResult {
    pub applied: bool,
    pub reason: Option<String>,
}

#[derive(Debug)]
pub struct LyricPush {
    pub reference: String,
    pub text: String,
    pub colored: Vec<ColoredLine>,
    pub next: Option<ProgramSlideInfo>,
}

pub fn lyric_slide(
    song: &LyricsSong,
    index: f64,
    gloss: Option<&str>,
) -> Result<LyricPush, String> {
    let gloss = normalize_gloss_color(gloss);
    let slides = build_slides(song, &SongPresentOptions::default(), Some(&gloss));
    let index_valid =
        index.is_finite() && index >= 0.0 && index.fract() == 0.0 && index < slides.len() as f64;
    if !index_valid {
        return Err(format!(
            "Slide {} not found in \"{}\"",
            kairo_domain::js::number_to_string(index + 1.0),
            song.title
        ));
    }
    let index = index as usize;
    let slide = &slides[index];
    let next = slides.get(index + 1).map(|next| ProgramSlideInfo {
        reference: format!("{} — {}", song.title, next.section_label),
        text: next.lines.join("\n"),
    });
    Ok(LyricPush {
        reference: format!("{} — {}", song.title, slide.section_label),
        text: slide.lines.join("\n"),
        colored: slide
            .lines
            .iter()
            .enumerate()
            .map(|(i, text)| ColoredLine {
                text: text.clone(),
                color: slide.line_colors.get(i).cloned().flatten(),
            })
            .collect(),
        next,
    })
}

use kairo_domain::documents::{DocumentSlide, ProjectionDocument};
use kairo_orchestrator::voice::BoxFuture;
use std::path::PathBuf;

/// Only the orchestrator operations these application handlers perform.
/// Real orchestration remains serialized by `Orchestrator`'s presentation lock.
pub trait Presenter: Send + Sync {
    fn present_lyric_slide(
        &self,
        reference: String,
        text: String,
        colored: Option<Vec<ColoredLine>>,
        next: Option<ProgramSlideInfo>,
    ) -> BoxFuture<'_, Result<(), String>>;
    fn present_document_page(
        &self,
        path: String,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool>;
    fn present_live_background(&self) -> BoxFuture<'_, bool>;
    fn set_auto_mode(&self, enabled: bool) -> BoxFuture<'_, ()>;
    fn set_confidence_threshold(&self, threshold: f64) -> BoxFuture<'_, ()>;
}
impl Presenter for Orchestrator {
    fn present_lyric_slide(
        &self,
        reference: String,
        text: String,
        colored: Option<Vec<ColoredLine>>,
        next: Option<ProgramSlideInfo>,
    ) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            Orchestrator::present_lyric_slide(self, &reference, &text, colored, next).await
        })
    }
    fn present_document_page(
        &self,
        path: String,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool> {
        Box::pin(async move { Orchestrator::present_document_page(self, &path, slide).await })
    }
    fn present_live_background(&self) -> BoxFuture<'_, bool> {
        Box::pin(Orchestrator::present_live_background(self))
    }
    fn set_auto_mode(&self, enabled: bool) -> BoxFuture<'_, ()> {
        Box::pin(Orchestrator::set_auto_mode(self, enabled))
    }
    fn set_confidence_threshold(&self, threshold: f64) -> BoxFuture<'_, ()> {
        Box::pin(Orchestrator::set_confidence_threshold(self, threshold))
    }
}

pub trait ActionDocuments: Send + Sync {
    fn slide(&self, id: &str, page: f64) -> Result<DocumentSlide, String>;
    fn page(&self, id: &str, page: f64) -> Result<PathBuf, String>;
    fn remove(&self, id: &str) -> Result<Vec<ProjectionDocument>, String>;
}
impl ActionDocuments for kairo_documents::service::DocumentsService {
    fn slide(&self, id: &str, page: f64) -> Result<DocumentSlide, String> {
        self.slide(id, page).map_err(|e| e.to_string())
    }
    fn page(&self, id: &str, page: f64) -> Result<PathBuf, String> {
        self.page(id, page).map_err(|e| e.to_string())
    }
    fn remove(&self, id: &str) -> Result<Vec<ProjectionDocument>, String> {
        self.remove(id).map_err(|e| e.to_string())
    }
}

pub trait ActionSurfaces: EngineClient {
    fn engine(&self) -> &dyn kairo_core::engine::OutputEngine;
}
impl<E: kairo_core::engine::OutputEngine> ActionSurfaces for EngineSurfaces<E> {
    fn engine(&self) -> &dyn kairo_core::engine::OutputEngine {
        EngineSurfaces::engine(self)
    }
}

#[cfg(test)]
mod effects {
    use super::*;
    use kairo_core::engine::OutputEngine;
    use kairo_output::fake::FakeEngine;
    use serde_json::json;
    use std::sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    };

    #[derive(Default)]
    struct Usage(Mutex<Vec<String>>);
    impl kairo_orchestrator::clients::UsageClient for Usage {
        fn track(&self, event: &str) {
            self.0.lock().unwrap().push(event.into());
        }
    }
    struct Presentation {
        fail_lyrics: AtomicBool,
        applied: AtomicBool,
        media: SharedMedia,
        scripture: kairo_scripture::ScriptureService,
        settings: Arc<Mutex<SettingsStore>>,
        song_usage: kairo_transfer::SongUsageService,
        calls: Mutex<Vec<String>>,
    }
    impl Presenter for Presentation {
        fn present_lyric_slide(
            &self,
            reference: String,
            _text: String,
            _colored: Option<Vec<ColoredLine>>,
            _next: Option<ProgramSlideInfo>,
        ) -> BoxFuture<'_, Result<(), String>> {
            assert!(
                self.song_usage.list(Some(0.), Some(f64::MAX)).is_empty(),
                "usage recorded before push"
            );
            self.calls.lock().unwrap().push(reference);
            let fail = self.fail_lyrics.load(Ordering::SeqCst);
            Box::pin(async move {
                if fail {
                    Err("push failed".into())
                } else {
                    Ok(())
                }
            })
        }
        fn present_document_page(
            &self,
            _path: String,
            _slide: Option<DocumentSlide>,
        ) -> BoxFuture<'_, bool> {
            self.calls.lock().unwrap().push("document".into());
            Box::pin(async { self.applied.load(Ordering::SeqCst) })
        }
        fn present_live_background(&self) -> BoxFuture<'_, bool> {
            assert!(
                self.media.lock().unwrap().live_item().is_some(),
                "live id must be set before presentation"
            );
            self.calls.lock().unwrap().push("background".into());
            Box::pin(async { self.applied.load(Ordering::SeqCst) })
        }
        fn set_auto_mode(&self, enabled: bool) -> BoxFuture<'_, ()> {
            assert_eq!(self.scripture.auto_mode(), enabled);
            assert_ne!(
                self.settings.lock().unwrap().data()["scripture"]["autoMode"],
                json!(enabled),
                "persisted before both services changed"
            );
            Box::pin(async {})
        }
        fn set_confidence_threshold(&self, threshold: f64) -> BoxFuture<'_, ()> {
            assert_eq!(self.scripture.confidence_threshold(), threshold);
            assert_ne!(
                self.settings.lock().unwrap().data()["scripture"]["confidenceThreshold"],
                json!(threshold)
            );
            Box::pin(async {})
        }
    }
    #[derive(Default)]
    struct Documents {
        fail_slide: AtomicBool,
        next_called: AtomicBool,
        release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
        fail_remove: AtomicBool,
    }
    impl ActionDocuments for Documents {
        fn slide(&self, id: &str, page: f64) -> Result<DocumentSlide, String> {
            if self.fail_slide.load(Ordering::SeqCst) {
                return Err("missing page".into());
            }
            Ok(DocumentSlide {
                id: id.into(),
                page,
                path: "/page.png".into(),
                width: 1920.,
                height: 1080.,
                videos: vec![],
            })
        }
        fn page(&self, _: &str, _: f64) -> Result<PathBuf, String> {
            self.next_called.store(true, Ordering::SeqCst);
            if let Some(release) = self.release.lock().unwrap().take() {
                release.recv().unwrap();
            }
            Err("end of deck".into())
        }
        fn remove(&self, _: &str) -> Result<Vec<ProjectionDocument>, String> {
            if self.fail_remove.load(Ordering::SeqCst) {
                Err("remove failed".into())
            } else {
                Ok(vec![])
            }
        }
    }
    #[derive(Default)]
    struct Dialogs(
        Mutex<std::collections::VecDeque<Vec<PathBuf>>>,
        Mutex<Vec<kairo_core::dialogs::DialogRequest>>,
    );
    impl crate::dialogs::NativeDialogs for Dialogs {
        fn pick(
            &self,
            request: kairo_core::dialogs::DialogRequest,
        ) -> BoxFuture<'_, Result<Vec<PathBuf>, String>> {
            self.1.lock().unwrap().push(request);
            let files = self.0.lock().unwrap().pop_front().unwrap_or_default();
            Box::pin(async move { Ok(files) })
        }
    }
    struct Rig {
        _dir: tempfile::TempDir,
        actions: Actions,
        engine: Arc<FakeEngine>,
        presenter: Arc<Presentation>,
        docs: Arc<Documents>,
        usage: Arc<Usage>,
        dialogs: Arc<Dialogs>,
    }
    fn core() -> AppCore {
        static CORE: OnceLock<AppCore> = OnceLock::new();
        CORE.get_or_init(|| AppCore::new().unwrap()).clone()
    }
    async fn rig() -> Rig {
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(Mutex::new(SettingsStore::open(
            dir.path(),
            &mut kairo_store::settings::LaunchContext {
                now_ms: 1.,
                new_id: &mut kairo_store::settings::random_uuid,
            },
        )));
        let media_root = dir.path().join("Media");
        std::fs::create_dir_all(&media_root).unwrap();
        std::fs::write(media_root.join("clip.mp4"), "video").unwrap();
        std::fs::write(media_root.join("still.png"), "image").unwrap();
        let mut media = kairo_media::library::MediaService::new(settings.clone());
        media.set_folder(&media_root.to_string_lossy());
        let media = Arc::new(Mutex::new(media));
        let lyrics = Arc::new(
            kairo_lyrics::LyricsService::open(
                dir.path(),
                dir.path().join("Workspace"),
                settings.clone(),
            )
            .await
            .unwrap(),
        );
        let song: LyricsSong = serde_json::from_value(json!({"id":"song", "title":"Grace", "artist":"", "sections":[{"type":"verse", "label":"Verse 1", "lines":["First"]}], "source":"text", "isFavorite":false, "createdAt":0, "updatedAt":0})).unwrap();
        lyrics.add_song(song).await.unwrap();
        let song_usage = kairo_transfer::SongUsageService::new(
            dir.path(),
            Arc::new(kairo_store::settings::now_ms),
        );
        let scripture = kairo_scripture::ScriptureService::new(Arc::new(
            kairo_scripture::pack::HttpPackFetcher::new().unwrap(),
        ));
        let presenter = Arc::new(Presentation {
            fail_lyrics: AtomicBool::new(false),
            applied: AtomicBool::new(true),
            media: media.clone(),
            scripture: scripture.clone(),
            settings: settings.clone(),
            song_usage: song_usage.clone(),
            calls: Mutex::default(),
        });
        let engine = Arc::new(FakeEngine::new());
        engine.command(Command::OpenSurface(kairo_engine_proto::SurfaceSpec {
            output: "screen".into(),
            label: "Screen".into(),
            sink: kairo_engine_proto::SinkSpec::Ndi {
                name: "Test".into(),
            },
        }));
        engine.take_commands();
        let surfaces = Arc::new(EngineSurfaces::new(engine.clone()));
        let core = core();
        let program = Arc::new(kairo_output::ProgramService::new(
            settings.clone(),
            core.publisher(),
            Box::new(|_| {}),
        ));
        let docs = Arc::new(Documents::default());
        let usage = Arc::new(Usage::default());
        let dialogs = Arc::new(Dialogs::default());
        let tracks = Arc::new(Mutex::new(kairo_media::tracks::TracksService::new(
            settings.clone(),
        )));
        let actions = Actions {
            dialogs: dialogs.clone(),
            tracks,
            picked: Arc::default(),
            settings,
            lyrics,
            media,
            documents: docs.clone(),
            libraries: kairo_transfer::LibrariesService::open(
                dir.path(),
                Arc::new(kairo_store::settings::now_ms),
            ),
            song_usage,
            program,
            orchestrator: presenter.clone(),
            usage: usage.clone(),
            scripture,
            surfaces,
            core,
        };
        Rig {
            _dir: dir,
            actions,
            engine,
            presenter,
            docs,
            usage,
            dialogs,
        }
    }
    #[tokio::test]
    async fn song_usage_is_recorded_only_after_success_and_missing_songs_are_reported() {
        let rig = rig().await;
        rig.presenter.fail_lyrics.store(true, Ordering::SeqCst);
        assert_eq!(
            rig.actions.push_lyrics("song", 0.).await.unwrap_err(),
            "push failed"
        );
        assert!(
            rig.actions
                .song_usage
                .list(Some(0.), Some(f64::MAX))
                .is_empty()
        );
        rig.presenter.fail_lyrics.store(false, Ordering::SeqCst);
        rig.actions.push_lyrics("song", 0.).await.unwrap();
        assert_eq!(
            rig.actions.song_usage.list(Some(0.), Some(f64::MAX)).len(),
            1
        );
        assert_eq!(
            rig.actions.push_lyrics("gone", 0.).await.unwrap_err(),
            "Song not found: gone"
        );
        assert_eq!(
            rig.usage.0.lock().unwrap().as_slice(),
            ["lyrics_slide", "lyrics_slide", "lyrics_slide"]
        );
    }
    #[tokio::test]
    async fn document_push_clears_media_sets_program_even_without_a_target_and_skips_preload() {
        let rig = rig().await;
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        rig.presenter.applied.store(false, Ordering::SeqCst);
        assert!(!rig.actions.push_document("doc", 3.).await.unwrap());
        assert!(rig.actions.media.lock().unwrap().live_item().is_none());
        assert_eq!(
            rig.actions.program.state().current.unwrap().reference,
            "Page 4"
        );
        assert!(!rig.docs.next_called.load(Ordering::SeqCst));
    }
    #[tokio::test]
    async fn document_next_page_is_not_awaited_and_end_of_deck_is_ignored() {
        let rig = rig().await;
        let (release, wait) = std::sync::mpsc::channel();
        *rig.docs.release.lock().unwrap() = Some(wait);
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                rig.actions.push_document("doc", 0.)
            )
            .await
            .unwrap()
            .unwrap()
        );
        release.send(()).unwrap();
        for _ in 0..100 {
            if rig.docs.next_called.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
        assert!(rig.docs.next_called.load(Ordering::SeqCst));
    }
    #[tokio::test]
    async fn failed_document_read_keeps_live_media_and_program() {
        let rig = rig().await;
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        rig.docs.fail_slide.store(true, Ordering::SeqCst);
        assert_eq!(
            rig.actions.push_document("doc", 0.).await.unwrap_err(),
            "missing page"
        );
        assert!(rig.actions.media.lock().unwrap().live_item().is_some());
        assert!(rig.actions.program.state().current.is_none());
        assert!(rig.presenter.calls.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn media_push_sets_live_before_presenting_and_missing_item_does_not_present() {
        let rig = rig().await;
        assert_eq!(
            rig.actions.push_media("gone").await.unwrap_err(),
            "That background is no longer in the folder."
        );
        assert!(rig.presenter.calls.lock().unwrap().is_empty());
        assert_eq!(
            rig.actions.push_media("clip.mp4").await.unwrap(),
            MediaPushResult {
                applied: true,
                reason: None
            }
        );
    }
    #[tokio::test]
    async fn clearing_and_deleting_a_live_background_send_transparent_after_library_change() {
        let rig = rig().await;
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        rig.actions.clear_media().await;
        assert!(rig.actions.media.lock().unwrap().live_item().is_none());
        assert!(
            matches!(&rig.engine.take_commands()[0], Command::SetBackground { background, .. } if background.decode::<kairo_domain::overlay::OverlayBackground>().unwrap().kind == OverlayBackgroundType::Transparent)
        );
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        rig.actions.delete_media("clip.mp4").await.unwrap();
        assert!(matches!(
            &rig.engine.take_commands()[0],
            Command::SetBackground { .. }
        ));
        rig.actions.delete_media("still.png").await.unwrap();
        assert!(rig.engine.take_commands().is_empty());
    }
    #[tokio::test]
    async fn playback_patch_changes_live_only_without_next_cue_and_missing_cues_fail() {
        let rig = rig().await;
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        let patch = json!({"brightness":1.2}).as_object().unwrap().clone();
        rig.actions
            .set_media_playback("clip.mp4", &patch, true)
            .unwrap();
        assert!(rig.engine.take_commands().is_empty());
        rig.actions
            .set_media_playback("clip.mp4", &patch, false)
            .unwrap();
        assert!(matches!(
            &rig.engine.take_commands()[0],
            Command::PatchBackground { .. }
        ));
        assert_eq!(
            rig.actions
                .set_media_playback("gone", &patch, true)
                .unwrap_err(),
            "This item is no longer in the Media Dock."
        );
    }
    #[tokio::test]
    async fn pause_reaches_outputs_and_seek_only_applies_to_a_live_video() {
        let rig = rig().await;
        rig.actions.set_media_paused(true);
        assert!(matches!(
            &rig.engine.take_commands()[0],
            Command::VideoTransport {
                transport: VideoTransport::Pause(false),
                ..
            }
        ));
        rig.actions.seek_media(5.);
        assert!(rig.engine.take_commands().is_empty());
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("still.png"));
        rig.actions.seek_media(5.);
        assert!(rig.engine.take_commands().is_empty());
        rig.actions
            .media
            .lock()
            .unwrap()
            .set_live_item(Some("clip.mp4"));
        rig.actions.set_media_paused(true);
        assert!(matches!(
            &rig.engine.take_commands()[0],
            Command::VideoTransport {
                transport: VideoTransport::Pause(true),
                ..
            }
        ));
        rig.actions.seek_media(5.);
        assert!(matches!(
            &rig.engine.take_commands()[0],
            Command::VideoTransport {
                transport: VideoTransport::Seek(5.),
                ..
            }
        ));
    }
    #[tokio::test]
    async fn auto_and_confidence_change_both_services_before_persistence() {
        let rig = rig().await;
        let current = rig.actions.settings.lock().unwrap().data()["scripture"]["autoMode"]
            .as_bool()
            .unwrap();
        rig.actions.set_auto_mode(!current).await;
        rig.actions.set_confidence(0.73).await;
        assert_eq!(
            rig.actions.settings.lock().unwrap().data()["scripture"]["autoMode"],
            json!(!current)
        );
        assert_eq!(
            rig.actions.settings.lock().unwrap().data()["scripture"]["confidenceThreshold"],
            json!(0.73)
        );
    }
    #[tokio::test]
    async fn invalid_media_import_never_opens_a_picker() {
        let rig = rig().await;
        assert_eq!(
            rig.actions.import_media("audio").await.unwrap_err(),
            "Unsupported media type."
        );
        assert!(rig.dialogs.1.lock().unwrap().is_empty());
    }
    #[tokio::test]
    async fn canceled_missing_media_folder_stops_before_file_picker() {
        let rig = rig().await;
        rig.actions.media.lock().unwrap().set_folder("");
        let library = rig.actions.import_media("image").await.unwrap();
        assert!(library.folder.is_empty());
        let requests = rig.dialogs.1.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].title,
            Some("Choose a backgrounds folder for imported files")
        );
    }
    #[tokio::test]
    async fn tracks_import_creates_default_audio_folder_before_picking_files() {
        let rig = rig().await;
        let source = rig._dir.path().join("song.mp3");
        std::fs::write(&source, "audio").unwrap();
        rig.dialogs.0.lock().unwrap().push_back(vec![source]);
        let library = rig.actions.import_tracks().await.unwrap();
        assert!(Path::new(&library.folder).is_dir());
        assert_eq!(library.items.len(), 1);
        assert_eq!(rig.dialogs.1.lock().unwrap()[0].title, Some("Import audio"));
    }
    #[tokio::test]
    async fn selected_program_image_is_allowed_before_settings_are_saved() {
        let rig = rig().await;
        let picked = rig._dir.path().join("outside.svg");
        assert!(!rig.actions.media_path_allowed(&picked));
        rig.dialogs
            .0
            .lock()
            .unwrap()
            .push_back(vec![picked.clone()]);
        assert_eq!(
            rig.actions.pick_program_image().await.unwrap(),
            Some(picked.clone())
        );
        assert!(rig.actions.media_path_allowed(&picked));
        assert!(
            !rig.actions
                .media_path_allowed(&picked.with_extension("png"))
        );
    }
    #[tokio::test]
    async fn canceled_usage_export_does_not_create_a_file() {
        let rig = rig().await;
        assert!(
            rig.actions
                .export_song_usage(None, None)
                .await
                .unwrap()
                .is_none()
        );
        let request = rig.dialogs.1.lock().unwrap()[0].clone();
        assert_eq!(request.title, Some("Export song usage"));
        assert!(
            request
                .default_path
                .unwrap()
                .to_string_lossy()
                .starts_with("song-usage-")
        );
    }
}

impl Actions {
    /// Selected image files become available immediately, before settings save.
    pub async fn pick_program_image(&self) -> Result<Option<PathBuf>, String> {
        self.pick_allowed_media(kairo_core::dialogs::DialogKind::ProgramImage)
            .await
    }
    pub async fn pick_overlay_media(&self, video: bool) -> Result<Option<PathBuf>, String> {
        self.pick_allowed_media(if video {
            kairo_core::dialogs::DialogKind::OverlayVideo
        } else {
            kairo_core::dialogs::DialogKind::OverlayImage
        })
        .await
    }
    async fn pick_allowed_media(
        &self,
        kind: kairo_core::dialogs::DialogKind,
    ) -> Result<Option<PathBuf>, String> {
        let files = self
            .dialogs
            .pick(kairo_core::dialogs::dialog_request(kind, None))
            .await?;
        let file = files.into_iter().next();
        if let Some(file) = &file {
            self.picked.allow(file);
        }
        Ok(file)
    }
    /// Any path from a remote page must pass this check before an engine call.
    pub fn media_path_allowed(&self, path: &Path) -> bool {
        let mut roots = Vec::new();
        if let Some(root) = self
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allowed_root()
        {
            roots.push(root);
        }
        if let Some(root) = self
            .tracks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .allowed_root()
        {
            roots.push(root);
        }
        let settings = self.settings.lock().unwrap_or_else(|e| e.into_inner());
        kairo_core::media_access::media_path_allowed(
            path,
            &Value::Object(settings.data().clone()),
            &|path| self.picked.is_allowed(path),
            &roots,
        )
    }
    pub async fn choose_media_folder(&self) -> Result<MediaLibrary, String> {
        let files = self
            .dialogs
            .pick(kairo_core::dialogs::dialog_request(
                kairo_core::dialogs::DialogKind::MediaFolder,
                None,
            ))
            .await?;
        let media = self.media.clone();
        self.core
            .blocking(move || {
                let mut media = media.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(path) = files.first() {
                    media.set_folder(&path.to_string_lossy())
                } else {
                    media.library()
                }
            })
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn import_media(&self, kind: &str) -> Result<MediaLibrary, String> {
        use kairo_core::dialogs::{DialogKind, dialog_request};
        if !matches!(kind, "image" | "video") {
            return Err("Unsupported media type.".into());
        }
        let missing_folder = self
            .media
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library()
            .folder
            .is_empty();
        if missing_folder {
            let folders = self
                .dialogs
                .pick(dialog_request(DialogKind::ImportMediaFolder, None))
                .await?;
            let Some(folder) = folders.first().cloned() else {
                return Ok(self
                    .media
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .library());
            };
            let media = self.media.clone();
            self.core
                .blocking(move || {
                    media
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set_folder(&folder.to_string_lossy())
                })
                .await
                .map_err(|e| e.to_string())?;
        }
        let files = self
            .dialogs
            .pick(dialog_request(
                if kind == "image" {
                    DialogKind::ImportImages
                } else {
                    DialogKind::ImportVideos
                },
                None,
            ))
            .await?;
        self.import_media_files(None, files).await
    }
    pub async fn add_backgrounds(&self, playlist_id: &str) -> Result<MediaLibrary, String> {
        let files = self
            .dialogs
            .pick(kairo_core::dialogs::dialog_request(
                kairo_core::dialogs::DialogKind::AddBackgrounds,
                None,
            ))
            .await?;
        self.import_media_files(Some(playlist_id.to_owned()), files)
            .await
    }
    pub async fn import_media_files(
        &self,
        playlist_id: Option<String>,
        files: Vec<PathBuf>,
    ) -> Result<MediaLibrary, String> {
        let media = self.media.clone();
        self.core
            .blocking(move || {
                let mut media = media.lock().unwrap_or_else(|e| e.into_inner());
                if files.is_empty() {
                    return Ok(media.library());
                }
                media
                    .import_into_playlist(playlist_id.as_deref(), &files)
                    .map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
    }
    pub async fn choose_tracks_folder(&self) -> Result<kairo_domain::media::TracksLibrary, String> {
        let files = self
            .dialogs
            .pick(kairo_core::dialogs::dialog_request(
                kairo_core::dialogs::DialogKind::TracksFolder,
                None,
            ))
            .await?;
        let tracks = self.tracks.clone();
        self.core
            .blocking(move || {
                let mut tracks = tracks.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(folder) = files.first() {
                    tracks.set_folder(&folder.to_string_lossy())
                } else {
                    tracks.library()
                }
            })
            .await
            .map_err(|e| e.to_string())
    }
    pub async fn import_tracks(&self) -> Result<kairo_domain::media::TracksLibrary, String> {
        use kairo_core::dialogs::{DialogKind, dialog_request};
        let tracks = self.tracks.clone();
        let ensured = self
            .core
            .blocking(move || {
                tracks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .ensure_folder()
            })
            .await
            .map_err(|e| e.to_string())?;
        if ensured.is_err() {
            let folders = self
                .dialogs
                .pick(dialog_request(DialogKind::ImportTracksFolder, None))
                .await?;
            let Some(folder) = folders.first().cloned() else {
                return Ok(self
                    .tracks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .library());
            };
            let tracks = self.tracks.clone();
            self.core
                .blocking(move || {
                    tracks
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set_folder(&folder.to_string_lossy())
                })
                .await
                .map_err(|e| e.to_string())?;
        }
        let files = self
            .dialogs
            .pick(dialog_request(DialogKind::ImportAudio, None))
            .await?;
        let tracks = self.tracks.clone();
        self.core
            .blocking(move || {
                let mut tracks = tracks.lock().unwrap_or_else(|e| e.into_inner());
                if files.is_empty() {
                    return Ok(tracks.library());
                }
                tracks.import_files(&files).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| e.to_string())?
    }
    pub async fn export_song_usage(
        &self,
        from: Option<f64>,
        to: Option<f64>,
    ) -> Result<Option<PathBuf>, String> {
        let entries = self.song_usage.list(from, to);
        let default = kairo_transfer::song_usage_file_name(kairo_store::settings::now_ms());
        let paths = self
            .dialogs
            .pick(kairo_core::dialogs::dialog_request(
                kairo_core::dialogs::DialogKind::ExportSongUsage,
                Some(default.into()),
            ))
            .await?;
        let Some(path) = paths.first().cloned() else {
            return Ok(None);
        };
        let saved = path.clone();
        let csv = kairo_domain::usage::song_usage_csv(&entries, &chrono::Local);
        self.core
            .blocking(move || std::fs::write(saved, csv))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        Ok(Some(path))
    }
}
