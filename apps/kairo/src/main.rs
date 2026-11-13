//! Native UI bootstrap window.
//!
//! Startup follows `src/main/index.ts`: find the profile's folders, start
//! logging, install crash handling, then (packaged builds only) make sure
//! this is the one Kairo for the profile, forwarding the launch otherwise.

use gpui_kit::component::ActiveTheme;
use gpui_kit::*;
use kairo_store::crash::{CrashConfig, install_panic_hook};
use kairo_store::instance::{Forwarded, Instance, PrimaryInstance, acquire};
use kairo_store::launch::{LaunchQueue, kairo_paths_from_urls};
use kairo_store::paths::{self, AppPaths, Os};
use std::path::PathBuf;
use std::time::Duration;

/// `.kairo` files the operating system handed over before the window opened.
static LAUNCH_FILES: LaunchQueue = LaunchQueue::new();
/// macOS open events after the window opened, for the main thread to collect.
static LATE_FILES: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// `.kairo` files waiting for the importer, which opens them (`openKairoFiles`).
#[derive(Default)]
pub struct PendingKairoFiles(pub Vec<PathBuf>);

impl Global for PendingKairoFiles {}

fn receive_files(files: Vec<PathBuf>, cx: &mut App) {
    if files.is_empty() {
        return;
    }
    tracing::info!(count = files.len(), "[Open] .kairo files received");
    cx.default_global::<PendingKairoFiles>().0.extend(files);
}

mod display_reporting;
struct HelloWorld {
    _display: display_reporting::ControlDisplayReporter,
}
impl HelloWorld {
    fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        engine: kairo_core::engine::EngineHandle,
    ) -> Self {
        Self {
            _display: display_reporting::ControlDisplayReporter::new(window, cx, engine),
        }
    }
}

impl Render for HelloWorld {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child("Hello, world!")
    }
}

fn start(paths: &AppPaths) -> Option<kairo_store::logging::LogGuard> {
    let guard = match kairo_store::logging::init(&paths.log_file, paths.packaged) {
        Ok(guard) => Some(guard),
        Err(error) => {
            eprintln!("Kairo: file logging unavailable ({error}); logging to the console only");
            None
        }
    };
    install_panic_hook(CrashConfig {
        recovery_file: paths.recovery_file(),
        ledger_file: paths.crash_ledger_file(),
        home: paths.home.to_string_lossy().into_owned(),
        windows: paths.os == Os::Windows,
        app_version: env!("CARGO_PKG_VERSION").into(),
    });
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        dev_profile = paths.dev_profile,
        "Kairo starting"
    );
    guard
}

/// Collects, on the main thread, files opened after launch and launches
/// forwarded by a second process. A forwarded launch with no files brings
/// Kairo forward, as `second-instance` does. The task owns the instance, so
/// the lock is held for the life of the app.
fn watch_incoming(instance: Option<PrimaryInstance>, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(200))
                .await;
            let late = std::mem::take(&mut *LATE_FILES.lock().unwrap_or_else(|e| e.into_inner()));
            let launches: Vec<Forwarded> = instance
                .as_ref()
                .map(|primary| primary.forwarded().try_iter().collect())
                .unwrap_or_default();
            if late.is_empty() && launches.is_empty() {
                continue;
            }
            cx.update(|cx| {
                receive_files(late, cx);
                if !launches.is_empty() {
                    for launch in launches {
                        receive_files(launch.kairo_paths(), cx);
                    }
                    cx.activate(true);
                }
            });
        }
    })
    .detach();
}

fn watch_display_rebindings(
    engine: kairo_core::engine::EngineHandle,
    user_data: PathBuf,
    cx: &mut App,
) {
    let mut events = engine.subscribe();
    let settings = std::sync::Arc::new(std::sync::Mutex::new(
        None::<kairo_store::settings::SettingsStore>,
    ));
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(100))
                .await;
            while let Ok(event) = events.try_recv() {
                if let kairo_core::engine::EngineEvent::Engine(
                    kairo_engine_proto::Event::DisplayRebound {
                        output,
                        old_id,
                        new_id,
                        stage,
                    },
                ) = event
                {
                    let settings = settings.clone();
                    let user_data = user_data.clone();
                    cx.background_spawn(async move {
                        let mut settings = settings.lock().unwrap_or_else(|e| e.into_inner());
                        let settings = settings.get_or_insert_with(|| {
                            let mut new_id = kairo_store::settings::random_uuid;
                            kairo_store::settings::SettingsStore::open(
                                &user_data,
                                &mut kairo_store::settings::LaunchContext {
                                    now_ms: kairo_store::settings::now_ms(),
                                    new_id: &mut new_id,
                                },
                            )
                        });
                        settings.persist_display_rebinding(&output, old_id, new_id, stage);
                    })
                    .await;
                }
            }
            if engine.status() == kairo_core::engine::EngineStatus::Stopped {
                break;
            }
        }
    })
    .detach();
}

fn main() {
    let paths = match paths::detect() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Kairo cannot find its data folder: {error}");
            std::process::exit(1);
        }
    };
    let _log = start(&paths);

    // One Kairo per profile; dev builds may run beside the installed app.
    let mut instance = None;
    if paths.packaged {
        match acquire(&paths.user_data, &Forwarded::current()) {
            Ok(Instance::Secondary) => {
                tracing::info!("Kairo is already running; handed this launch to it");
                return;
            }
            Ok(Instance::Primary(primary)) => instance = Some(primary),
            Err(error) => {
                tracing::warn!(%error, "[Instance] Single-instance lock unavailable; continuing")
            }
        }
    }

    // Windows and Linux pass files on the command line; macOS sends open events.
    if !cfg!(target_os = "macos") {
        LAUNCH_FILES.push(Forwarded::current().kairo_paths());
    }

    let application = application().with_assets(assets::Assets);
    // macOS can deliver the launching file before the window exists.
    application.on_open_urls(|urls| {
        let late = LAUNCH_FILES.push(kairo_paths_from_urls(&urls));
        LATE_FILES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(late);
    });
    application.run(move |cx| {
        init(cx);

        // Output supervision lives outside GPUI's event loop. The shell owns
        // this handle; feature/routing services are composed by prompt 067.
        let binary = std::env::var_os("KAIRO_ENGINE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                std::env::current_exe()
                    .expect("Kairo executable")
                    .with_file_name(if cfg!(windows) {
                        "kairo-engine.exe"
                    } else {
                        "kairo-engine"
                    })
            });
        let engine =
            kairo_core::engine::EngineHandle::spawn(kairo_core::engine::EngineOptions::new(binary));
        let window_engine = engine.clone();
        open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| HelloWorld::new(window, cx, window_engine))
        })
        .expect("failed to open the Kairo bootstrap window");
        let shutdown = engine.clone();
        cx.on_app_quit(move |cx| {
            let shutdown = shutdown.clone();
            cx.background_spawn(async move {
                shutdown.shutdown();
            })
        })
        .detach();
        watch_display_rebindings(engine, paths.user_data.clone(), cx);

        receive_files(LAUNCH_FILES.mark_ready(), cx);
        watch_incoming(instance, cx);
        cx.activate(true);
    });
}
