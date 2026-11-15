//! House audio's backend (prompt 084): `window.api.tracks` in Electron,
//! over the tracks service, its actions and the operator's own player.

use std::sync::Arc;

use kairo_domain::media::TracksLibrary;
use serde_json::Value;
use ui_media::tracks::{TrackTime, TracksBackend, TracksCall, TracksReply, TracksSink};

use crate::services::ApplicationServices;

pub struct ServicesTracks {
    services: Arc<ApplicationServices>,
}

impl ServicesTracks {
    pub fn new(services: Arc<ApplicationServices>) -> Self {
        Self { services }
    }
}

/// `presentation.audio.outputLabel`: the device the tracks play on.
fn output_label(all: &Value) -> String {
    all.pointer("/presentation/audio/outputLabel")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

struct Feeds(Vec<tokio::task::JoinHandle<()>>);
impl Drop for Feeds {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}

impl TracksBackend for ServicesTracks {
    fn library(&self) -> TracksLibrary {
        self.services
            .tracks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .library()
    }

    fn listen(&self, sink: TracksSink) -> Box<dyn Send> {
        let services = &self.services;
        let library = {
            let mut values = services.core.watch::<TracksLibrary>();
            services.runtime().spawn(async move {
                while let Some(value) = values.next().await {
                    sink((*value).clone());
                }
            })
        };
        // The player follows the Show panel's output device.
        let label = {
            let all = services
                .settings_wiring
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_all();
            services
                .track_player
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_output_label(&output_label(&all));
            let mut values = services.core.watch::<crate::services::SettingsChanged>();
            let player = services.track_player.clone();
            services.runtime().spawn(async move {
                while let Some(value) = values.next().await {
                    let crate::services::SettingsChanged(all) = &*value;
                    player
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .set_output_label(&output_label(all));
                }
            })
        };
        Box::new(Feeds(vec![library, label]))
    }

    fn call(&self, call: TracksCall, reply: TracksReply) {
        let services = self.services.clone();
        self.services
            .spawn(async move { reply(run(&services, call).await) });
    }

    fn time(&self) -> TrackTime {
        let player = self
            .services
            .track_player
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let t = player.time();
        TrackTime {
            current_time: t.current_time,
            duration: t.duration,
            ended: t.ended,
            error: player.error(),
        }
    }

    fn seek(&self, seconds: f64) {
        self.services
            .track_player
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .seek(seconds);
    }
}

/// One edit of the tracks service, run off the UI thread.
type TracksEdit = Box<
    dyn FnOnce(
            &mut kairo_media::tracks::TracksService<crate::services::Settings>,
        ) -> Result<TracksLibrary, String>
        + Send,
>;

async fn run(
    services: &Arc<ApplicationServices>,
    call: TracksCall,
) -> Result<TracksLibrary, String> {
    let actions = services.actions();
    let tracks = services.tracks.clone();
    let with = |f: TracksEdit| {
        let tracks = tracks.clone();
        let core = services.core.clone();
        async move {
            core.blocking(move || f(&mut tracks.lock().unwrap_or_else(|e| e.into_inner())))
                .await
                .map_err(|e| e.to_string())?
        }
    };
    match call {
        TracksCall::Play(id) => {
            with(Box::new(move |t| t.play(&id).map_err(|e| e.to_string()))).await
        }
        TracksCall::SetPaused(p) => with(Box::new(move |t| Ok(t.set_paused(p)))).await,
        TracksCall::Stop => with(Box::new(|t| Ok(t.stop()))).await,
        TracksCall::Rename { id, name } => {
            with(Box::new(move |t| {
                t.rename(&id, &name).map_err(|e| e.to_string())
            }))
            .await
        }
        TracksCall::Remove(id) => {
            with(Box::new(move |t| t.remove(&id).map_err(|e| e.to_string()))).await
        }
        TracksCall::Rescan => with(Box::new(|t| Ok(t.scan()))).await,
        TracksCall::Reveal(id) => {
            let path = {
                let t = tracks.lock().unwrap_or_else(|e| e.into_inner());
                t.reveal(&id).map_err(|e| e.to_string())?
            };
            crate::handlers::open_path(&path, true)?;
            Ok(tracks.lock().unwrap_or_else(|e| e.into_inner()).library())
        }
        TracksCall::Import => actions.import_tracks().await,
        TracksCall::ChooseFolder => actions.choose_tracks_folder().await,
    }
}

/// Puts House audio in the booth toolbox's Audio tab; File → Import →
/// Audio files shows the tab and opens the Add tracks dialog.
pub fn install(
    backend: Arc<dyn TracksBackend>,
    services: &Arc<ApplicationServices>,
    views: &ui_operator::BoothViews,
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
) {
    use gpui_kit::AppContext;
    let panel = cx.new(|cx| ui_media::tracks::TracksPanel::new(backend, window, cx));
    views
        .toolbox
        .update(cx, |t, cx| t.set_tracks(Some(panel.clone().into()), cx));
    let booth = views.booth.clone();
    cx.subscribe(&panel, move |_, _: &ui_media::tracks::TracksEvent, cx| {
        booth.update(cx, |b, cx| {
            b.set_tab(ui_operator::booth::ToolTab::Audio, cx)
        });
    })
    .detach();
    let mut requests = services.core.watch::<crate::services::ImportRequested>();
    let weak = panel.downgrade();
    cx.spawn(async move |cx| {
        while let Some(request) = requests.next().await {
            if request.0 == kairo_domain::import_menu::ImportKind::Audio
                && weak.update(cx, |p, cx| p.import_request(cx)).is_err()
            {
                break;
            }
        }
    })
    .detach();
}
