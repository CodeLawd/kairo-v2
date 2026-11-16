//! The Theme editor's backend over the application's services (prompt
//! 086): `window.api.settings`, `ndi.pickOverlayMedia` and the preview
//! surface. Writes are serialised in one mailbox, each a read-modify-write
//! of the stored settings through `ui_theme::ops::apply`.

use std::sync::Arc;

use kairo_domain::settings::AppSettings;
use ui_theme::backend::{
    FeedSink, Listening, MediaKind, Reply, Sample, ThemeBackend, ThemeFeed, ThemePreview,
};
use ui_theme::ops::{Op, apply};

use crate::services::{ApplicationServices, SettingsChanged};

type Request = (Op, Reply<()>);

pub struct ServicesTheme {
    services: Arc<ApplicationServices>,
    writer: tokio::sync::mpsc::UnboundedSender<Request>,
}

fn parse(all: serde_json::Value) -> Result<AppSettings, String> {
    serde_json::from_value(all).map_err(|e| e.to_string())
}

impl ServicesTheme {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        let (writer, mut requests) = tokio::sync::mpsc::unbounded_channel::<Request>();
        let worker = services.clone();
        services.spawn(async move {
            while let Some((op, reply)) = requests.recv().await {
                let s = worker.clone();
                let result = worker
                    .core
                    .blocking(move || persist(&s, &op))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r);
                if let Err(error) = &result {
                    tracing::error!(%error, "[Theme] could not save changes");
                }
                reply(result);
            }
        });
        Self { services, writer }
    }
}

/// Applies `op` to the latest stored settings and writes the sections it
/// changed.
fn persist(services: &ApplicationServices, op: &Op) -> Result<(), String> {
    let mut wiring = services
        .settings_wiring
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut settings = parse(wiring.get_all())?;
    let before = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    apply(&mut settings, op)?;
    let after = serde_json::to_value(&settings).map_err(|e| e.to_string())?;
    let mut changed = false;
    for key in ["themeLibrary", "themeDefaults", "overlay"] {
        if before[key] != after[key] {
            wiring
                .set(key, after[key].clone())
                .map_err(|e| e.to_string())?;
            changed = true;
        }
    }
    // These writes announce nothing (as in Electron, whose pages patch their
    // own copy), so pages drawing output themes, such as the scripture
    // cards, hear about the save here.
    if changed {
        wiring.broadcast_changed();
    }
    Ok(())
}

struct Feed(tokio::task::JoinHandle<()>);
impl Drop for Feed {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl ThemeBackend for ServicesTheme {
    fn snapshot(&self) -> AppSettings {
        let all = self
            .services
            .settings_wiring
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_all();
        parse(all).unwrap_or_else(|error| {
            tracing::error!(%error, "[Theme] stored settings unreadable");
            AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone())
        })
    }

    fn listen(&self, sink: FeedSink) -> Listening {
        let mut values = self.services.core.watch::<SettingsChanged>();
        let task = self.services.runtime().spawn(async move {
            while let Some(value) = values.next().await {
                let SettingsChanged(all) = &*value;
                if let Ok(settings) = parse(all.clone()) {
                    sink(ThemeFeed::Settings(Box::new(settings)));
                }
            }
        });
        Box::new(Feed(task))
    }

    fn edit(&self, op: Op, reply: Reply<()>) {
        if let Err(error) = self.writer.send((op, reply)) {
            (error.0.1)(Err("The Theme writer has stopped.".into()));
        }
    }

    fn pick_media(&self, kind: MediaKind, reply: Reply<Option<String>>) {
        let actions = self.services.actions();
        self.services.spawn(async move {
            reply(
                actions
                    .pick_overlay_media(kind == MediaKind::Video)
                    .await
                    .map(|p| p.map(|p| p.to_string_lossy().into_owned())),
            );
        });
    }

    fn media_aspect(&self, path: String, kind: MediaKind, reply: Reply<Option<f64>>) {
        let services = self.services.clone();
        self.services.spawn(async move {
            let measured = services
                .core
                .blocking(move || aspect(&path, kind))
                .await
                .map_err(|e| e.to_string());
            reply(measured);
        });
    }

    fn open_preview(&self) -> Option<ThemePreview> {
        use kairo_core::engine::OutputEngine;
        use kairo_engine_proto::{
            Command as EngineCommand, Json, SinkSpec, SlideContent, SurfaceConfig, SurfaceSpec,
        };
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let output = format!(
            "theme-editor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let engine: Arc<dyn OutputEngine> = Arc::new(self.services.engine.clone());
        let hub = match kairo_core::preview::PreviewHub::new(engine.clone()) {
            Ok(hub) => hub,
            Err(error) => {
                tracing::error!(%error, "[Theme] preview transport unavailable");
                return None;
            }
        };
        if !engine
            .command(EngineCommand::OpenSurface(SurfaceSpec {
                output: output.clone(),
                label: "Theme editor".into(),
                sink: SinkSpec::Preview,
            }))
            .accepted
        {
            return None;
        }
        let lifetime = crate::screens::SampleLifetime {
            engine: engine.clone(),
            output: output.clone(),
        };
        engine.command(EngineCommand::Configure {
            output: output.clone(),
            config: SurfaceConfig {
                web_enabled: false,
                ..SurfaceConfig::default()
            },
        });
        let target = output.clone();
        Some(ThemePreview::new(
            hub,
            output,
            move |theme, sample: Sample| {
                engine.command(EngineCommand::ShowSlide {
                    output: target.clone(),
                    source: target.clone(),
                    instant: true,
                    slide: SlideContent {
                        reference: sample.reference,
                        text: sample.text,
                        theme: Json::encode(&crate::screens::isolated_sample_theme(
                            &theme, &target,
                        )),
                        colored_lines: None,
                    },
                });
            },
            lifetime,
        ))
    }
}

/// Width over height of a picture or a video, `None` when unreadable.
fn aspect(path: &str, kind: MediaKind) -> Option<f64> {
    let (w, h) = match kind {
        MediaKind::Video => kairo_video::decode::open(std::path::Path::new(path), false)
            .ok()?
            .size(),
        MediaKind::Image => {
            let bytes = std::fs::read(path).ok()?;
            let picture = kairo_scene::media::decode_picture(
                &bytes,
                kairo_scene::media::BoxFit {
                    width: 1024,
                    height: 1024,
                    fit: kairo_scene::media::FitMode::Contain,
                },
            )
            .ok()?;
            (picture.width, picture.height)
        }
    };
    (h > 0).then(|| f64::from(w) / f64::from(h))
}

/// Builds the editor with its inspector, for the Theme route.
pub fn install(
    backend: Arc<dyn ThemeBackend>,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) -> gpui_kit::Entity<ui_theme::ThemeEditor> {
    use gpui_kit::AppContext;
    let editor = cx.new(|cx| ui_theme::ThemeEditor::new(backend, window, cx));
    // The editor holds the inspector's view, which keeps it alive.
    ui_theme::ThemeInspector::attach(&editor, window, cx);
    editor
}
