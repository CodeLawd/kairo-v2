// Packaged Windows builds are windowed apps: as console programs, Windows
// opened a terminal beside Kairo and printed the log into it. Dev builds
// keep the console for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
//! Native UI bootstrap window.
//!
//! Startup follows `src/main/index.ts`: find the profile's folders, start
//! logging, install crash handling, then (packaged builds only) make sure
//! this is the one Kairo for the profile, forwarding the launch otherwise.

use gpui_kit::*;
use kairo_domain::settings::{DisplaySettings, DisplayTheme, WhatsNewSettings};
use kairo_store::crash::{CrashConfig, install_panic_hook};
use kairo_store::instance::{Forwarded, Instance, PrimaryInstance, acquire};
use kairo_store::launch::{LaunchQueue, kairo_paths_from_urls};
use kairo_store::paths::{AppPaths, Os};
use kairo_store::settings::SettingsStore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ui_shell::{Shell, ShellOptions, ShellRequest, ShellState};

// Keep application and embedded-webview types on the owner's selected backend.
const _: () = assert!(gpui_kit::GPUI_FAST, "Kairo requires the GPUI Fast backend");

mod account;
pub mod actions;
mod audience;
#[cfg(feature = "dev-hooks")]
mod benchmark;
mod booth;
mod capture;
pub mod dialogs;
mod display_reporting;
mod dock;
mod documents;
pub mod handlers;
mod imagine;
mod lyrics;
mod media;
mod motion;
mod operator;
mod power;
mod quote_detector;
mod screens;
mod scripture;
pub mod services;
mod settings;
mod spell;
mod startup_error;
mod theme;
mod tracks;
mod transfer;
mod web;

/// When `main` began, for the launch-to-first-paint log line.
static LAUNCHED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// Logs launch to first paint once: the callback after the first frame runs
/// when that frame has been presented.
fn report_first_paint(window: &mut Window) {
    window.on_next_frame(|window, _| {
        window.on_next_frame(|_, _| {
            if let Some(launched) = LAUNCHED.get() {
                let ms = launched.elapsed().as_secs_f64() * 1000.0;
                tracing::info!(first_paint_ms = ms, "[Startup] first frame presented");
                if std::env::var_os("KAIRO_REPORT_STARTUP").is_some() {
                    eprintln!("kairo-first-paint-ms {ms:.1}");
                }
            }
        });
    });
}

/// `.kairo` files the operating system handed over before the window opened.
static LAUNCH_FILES: LaunchQueue = LaunchQueue::new();
/// macOS open events after the window opened, for the main thread to collect.
static LATE_FILES: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// The window's default and smallest sizes (`createWindow`).
const DEFAULT_SIZE: (f32, f32) = (1280., 800.);
const MIN_SIZE: (f32, f32) = (960., 600.);
/// `com.kairo.app`, as `setAppUserModelId` set it.
const APP_ID: &str = "com.kairo.app";

/// The settings store, opened once and shared.
type Settings = Arc<Mutex<SettingsStore>>;

/// The operator window, once it exists.
#[derive(Default)]
struct OperatorWindow {
    handle: Option<AnyWindowHandle>,
    shell: Option<WeakEntity<Shell>>,
    pending: Vec<PathBuf>,
}

impl Global for OperatorWindow {}

fn receive_files(files: Vec<PathBuf>, cx: &mut App) {
    if files.is_empty() {
        return;
    }
    tracing::info!(count = files.len(), "[Open] .kairo files received");
    let shell = cx
        .default_global::<OperatorWindow>()
        .shell
        .clone()
        .and_then(|s| s.upgrade());
    match shell {
        Some(shell) => shell.update(cx, |shell, cx| shell.open_kairo_files(files, cx)),
        None => cx.default_global::<OperatorWindow>().pending.extend(files),
    }
}

/// Settings the shell shows, read from the store.
fn read_shell_settings(settings: &Settings, state: &mut ShellState) -> DisplaySettings {
    let store = settings.lock().unwrap_or_else(|e| e.into_inner());
    let display: DisplaySettings = store.section("display").unwrap_or_default();
    let whats_new: WhatsNewSettings = store.section("whatsNew").unwrap_or_default();
    state.shortcuts = display.shortcuts.clone().unwrap_or_default();
    state.whats_new_last_seen = Some(whats_new.last_seen_version.into());
    display
}

/// Applies the stored interface and transcript sizes, keeping the View
/// menu's zoom.
fn apply_display_sizes(display: &DisplaySettings, cx: &mut App) {
    let zoom = kairo_ui_kit::theme::display_sizes(cx).zoom;
    kairo_ui_kit::theme::set_display_sizes(
        kairo_ui_kit::theme::DisplaySizes::from_settings(
            display.font_size,
            display.transcription_font_size,
            zoom,
        ),
        cx,
    );
}

/// The Settings window's remembered size (`kairo.frame.settings.v2`),
/// clamped to the operator window.
fn settings_frame(state: &dyn ui_operator::UiState, window: &Window) -> (f32, f32) {
    use ui_operator::layout::{FrameSize, clamp_frame_size, read_frame_size};
    let viewport = window.viewport_size();
    let size = read_frame_size(state, "kairo.frame.settings.v2").unwrap_or(FrameSize {
        w: ui_settings::FRAME.0,
        h: ui_settings::FRAME.1,
    });
    let size = clamp_frame_size(
        size,
        FrameSize {
            w: ui_settings::FRAME_MIN.0,
            h: ui_settings::FRAME_MIN.1,
        },
        (f32::from(viewport.width), f32::from(viewport.height)),
    );
    (size.w, size.h)
}

/// Requests the shell raises. Services composed by prompt 067 take over the
/// ones logged here as waiting.
fn handle_request(
    request: &ShellRequest,
    services: &Arc<services::ApplicationServices>,
    cx: &mut App,
) {
    let settings = &services.settings;
    match request {
        ShellRequest::OpenExternal(url) => cx.open_url(url),
        ShellRequest::WindowFocused => {
            // The system setting may have changed while Kairo was away.
            motion::apply(cx);
            services.request(request.clone());
        }
        ShellRequest::MarkWhatsNewSeen(version) => {
            let mut store = settings.lock().unwrap_or_else(|e| e.into_inner());
            let value = WhatsNewSettings {
                last_seen_version: version.to_string(),
                ..Default::default()
            };
            if let Err(error) = store.set_section("whatsNew", &value) {
                tracing::warn!(%error, "[Shell] could not record What's New as seen");
            }
        }
        // Handled beside the account flow in `open_operator_window`.
        ShellRequest::ShowTips | ShellRequest::SignOut => {}
        ShellRequest::FeaturePanicked { route, message } => {
            tracing::error!(route = route.as_str(), %message, "[Shell] workspace panic contained");
            services.request(request.clone());
        }
        other => services.request(other.clone()),
    }
}

/// What this launch's first-run migration wants the operator to see.
static MIGRATION_NOTES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();

fn open_operator_window(
    engine: kairo_core::engine::EngineHandle,
    settings: Settings,
    paths: &AppPaths,
    services: Arc<services::ApplicationServices>,
    cx: &mut App,
) {
    let mut state = ShellState::default();
    let display = read_shell_settings(&settings, &mut state);
    let theme = display.theme;
    apply_display_sizes(&display, cx);
    kairo_ui_kit::theme::set_follow_system(theme == DisplayTheme::System, cx);
    kairo_ui_kit::set_appearance(
        match theme {
            DisplayTheme::Light => kairo_ui_kit::Appearance::Light,
            DisplayTheme::Dark => kairo_ui_kit::Appearance::Dark,
            DisplayTheme::System => kairo_ui_kit::theme::appearance_of(cx.window_appearance()),
        },
        None,
        cx,
    );
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(DEFAULT_SIZE.0), px(DEFAULT_SIZE.1)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_SIZE.0), px(MIN_SIZE.1))),
        titlebar: Some(TitlebarOptions {
            title: Some("Kairo".into()),
            // macOS: the window buttons sit in the header (`hiddenInset`).
            appears_transparent: cfg!(target_os = "macos"),
            traffic_light_position: Some(point(px(18.), px(24.))),
        }),
        app_id: Some(APP_ID.into()),
        ..Default::default()
    };
    let shell_options = ShellOptions {
        version: env!("CARGO_PKG_VERSION").into(),
        dev: !paths.packaged,
        user_data: Some(paths.user_data.clone()),
        splash: true,
    };
    let operator_backend: Arc<dyn ui_operator::OperatorBackend> =
        Arc::new(operator::ServicesBackend::new(services.clone()));
    let ui_state: Arc<dyn ui_operator::UiState> =
        Arc::new(operator::UiStateStore::open(&paths.user_data));
    // One account model for the app's life: a reopened window reuses it.
    let account = match ui_account::model::account(cx) {
        Some(account) => account,
        None => {
            let backend: Arc<dyn ui_account::AccountBackend> =
                Arc::new(account::ServicesAccount(services.clone()));
            let model = cx.new(|_| ui_account::Account::new(backend, account::local_zone()));
            ui_account::model::install(model.clone(), cx);
            account::feed(&services);
            model
        }
    };
    // One settings backend for the app's life: it subscribes to the
    // services once and fans pushes out to each Settings window.
    let settings_backend: Arc<dyn ui_settings::SettingsBackend> =
        match cx.try_global::<SettingsBackendHandle>() {
            Some(handle) => handle.0.clone(),
            None => {
                let backend: Arc<dyn ui_settings::SettingsBackend> =
                    Arc::new(settings::ServicesSettings::new(services.clone()));
                cx.set_global(SettingsBackendHandle(backend.clone()));
                backend
            }
        };
    let screens_backend: Arc<dyn ui_screens::ScreensBackend> = Arc::new(
        screens::ServicesScreens::new(services.clone(), ui_state.clone()),
    );
    let settings_ui_state = ui_state.clone();
    let booth_backend: Arc<dyn ui_operator::BoothBackend> =
        Arc::new(booth::ServicesBooth::new(services.clone()));
    let media_backend: Arc<dyn ui_media::MediaBackend> = Arc::new(media::ServicesMedia::new(
        services.clone(),
        ui_state.clone(),
    ));
    let lyrics_backend: Arc<dyn ui_lyrics::LyricsBackend> =
        Arc::new(lyrics::ServicesLyrics::new(services.clone()));
    let scripture_backend: Arc<dyn ui_scripture::ScriptureBackend> = Arc::new(
        scripture::ServicesScripture::new(services.clone(), ui_state.clone()),
    );
    let tracks_services = services.clone();
    let audio_services = services.clone();
    let audio_backend = settings_backend.clone();
    let documents_backend: Arc<dyn ui_documents::DocumentsBackend> = Arc::new(
        documents::ServicesDocuments::new(services.clone(), ui_state.clone()),
    );
    let audience_backend: Arc<dyn ui_audience::AudienceBackend> = Arc::new(
        audience::ServicesAudience::new(services.clone(), ui_state.clone()),
    );
    let sidebar_state = ui_state.clone();
    let web_backend: Arc<dyn ui_web::WebBackend> =
        Arc::new(web::ServicesWeb::new(services.clone()));
    let imagine_backend: Arc<dyn ui_imagine::ImagineBackend> =
        Arc::new(imagine::ServicesImagine::new(services.clone()));
    let theme_backend: Arc<dyn ui_theme::ThemeBackend> =
        Arc::new(theme::ServicesTheme::new(services.clone()));
    let transfer_services = services.clone();
    let teleprompter_services = services.clone();
    let teleprompter_ui_state = ui_state.clone();
    let opened = gpui_kit::open_window(options, cx, move |window, cx| {
        // "Match system" follows macOS switching between light and dark.
        window
            .observe_window_appearance(|window, cx| {
                if kairo_ui_kit::theme::follows_system(cx) {
                    let appearance = kairo_ui_kit::theme::appearance_of(window.appearance());
                    kairo_ui_kit::set_appearance(appearance, Some(window), cx);
                }
            })
            .detach();
        {
            use kairo_ui_kit::components::page_sidebar::{PageSidebar, STORAGE_KEY, saved_width};
            let store = sidebar_state.clone();
            // Electron kept `String(width)`; `Number(...)` read it back.
            let width = saved_width(|key| match store.get(key)? {
                serde_json::Value::Number(n) => n.as_f64(),
                serde_json::Value::String(s) => s.trim().parse().ok(),
                _ => None,
            });
            let store = sidebar_state.clone();
            PageSidebar::install(cx, width, move |w| {
                store.set(STORAGE_KEY, serde_json::json!(w))
            });
        }
        let shell = cx.new(|cx| Shell::new(shell_options, window, cx));
        if let Some(notes) = MIGRATION_NOTES.get().filter(|n| !n.is_empty()) {
            let notes = notes.clone();
            shell.update(cx, |s, cx| s.set_migration_notes(notes, cx));
        }
        let flow = cx.new(|cx| ui_account::AccountFlow::new(account.clone(), window, cx));
        {
            // The Account popover's Show tips again and Sign out.
            let (flow, account) = (flow.downgrade(), account.clone());
            cx.subscribe(&shell, move |_, request: &ShellRequest, cx| match request {
                ShellRequest::ShowTips => {
                    let _ = flow.update(cx, |flow, cx| flow.replay_tips(cx));
                }
                ShellRequest::SignOut => {
                    let request = account.read(cx).backend().sign_out();
                    let account = account.clone();
                    cx.spawn(async move |cx| {
                        if let Ok(session) = request.await {
                            account.update(cx, |a, cx| a.set_session(session, cx));
                        }
                    })
                    .detach();
                }
                _ => {}
            })
            .detach();
        }
        let booth_account = account.clone();
        install_audio_rail(&shell, audio_backend, &audio_services, cx);
        install_screens(&shell, screens_backend, window, cx);
        install_settings(
            &shell,
            &flow,
            account,
            settings_backend,
            settings_ui_state,
            cx,
        );
        let target = shell.downgrade();
        cx.subscribe(&flow, move |_, event: &ui_account::FlowEvent, cx| {
            let _ = target.update(cx, |shell, cx| match *event {
                ui_account::FlowEvent::Navigate(route) => {
                    use ui_account::wizard::QuickStart;
                    shell.navigate(
                        match route {
                            None => ui_shell::Route::Operator,
                            Some(QuickStart::Scripture) => ui_shell::Route::Scripture,
                            Some(QuickStart::Lyrics) => ui_shell::Route::Lyrics,
                            Some(QuickStart::Documents) => ui_shell::Route::Documents,
                        },
                        cx,
                    )
                }
                ui_account::FlowEvent::OpenScreens => {
                    shell.open_overlay(ui_shell::Overlay::Screens, cx)
                }
            });
        })
        .detach();
        shell.update(cx, |shell, cx| shell.set_front(Some(flow.into()), cx));
        let workspace = cx
            .new(|cx| ui_operator::OperatorWorkspace::new(operator_backend, ui_state, window, cx));
        let screens = shell.downgrade();
        cx.subscribe(
            &workspace,
            move |_, event: &ui_operator::OperatorEvent, cx| match event {
                ui_operator::OperatorEvent::OpenScreens => {
                    let _ = screens.update(cx, |shell, cx| {
                        shell.open_overlay(ui_shell::Overlay::Screens, cx)
                    });
                }
            },
        )
        .detach();
        let booth_views =
            booth::install(booth_backend, &workspace, &shell, booth_account, window, cx);
        let tracks_backend: Arc<dyn ui_media::tracks::TracksBackend> =
            Arc::new(tracks::ServicesTracks::new(tracks_services.clone()));
        tracks::install(tracks_backend, &tracks_services, &booth_views, window, cx);
        let scripture_page = scripture::install(scripture_backend, &workspace, &shell, window, cx);
        shell.update(cx, |shell, cx| {
            shell.set_workspace(
                ui_shell::Route::Scripture,
                scripture_page.clone().into(),
                cx,
            )
        });
        let lyrics_page = lyrics::install(lyrics_backend, &workspace, &shell, window, cx);
        // One media dock, drawn under whichever booth page is showing.
        let documents_media = media_backend.clone();
        let media_dock = media::install(media_backend, &shell, window, cx);
        workspace.update(cx, |w, cx| w.set_dock(Some(media_dock.clone().into()), cx));
        scripture_page.update(cx, |p, cx| p.set_dock(Some(media_dock.clone().into()), cx));
        lyrics_page.update(cx, |p, cx| p.set_dock(Some(media_dock.clone().into()), cx));
        shell.update(cx, |s, cx| {
            s.set_media_dock(Some(media_dock.clone().into()), cx)
        });
        let documents_page = documents::install(documents_backend, &workspace, window, cx);
        documents_page.update(cx, |p, cx| {
            p.set_dock(Some(media_dock.clone().into()), cx);
            p.set_media(Some(documents_media), cx);
        });
        shell.update(cx, |shell, cx| {
            shell.set_workspace(
                ui_shell::Route::Documents,
                documents_page.clone().into(),
                cx,
            )
        });
        {
            // A clicker pages the slides whenever Slides is open, not only
            // while the slide list has focus: the shell turns the operator's
            // Next and Previous keys into workspace commands, and this is
            // where they land when nothing on the page took them.
            // A live deck owns the clicker everywhere: on any other screen,
            // Page Down and Page Up (what clickers send) step the deck that is
            // on the screens; the arrow keys stay with the open screen.
            let live_slides = documents_page.downgrade();
            let live_shell = shell.downgrade();
            cx.intercept_keystrokes(move |event, window, cx| {
                let k = &event.keystroke;
                let delta = match k.key.as_str() {
                    "pagedown" => 1,
                    "pageup" => -1,
                    _ => return,
                };
                let m = k.modifiers;
                if m.platform || m.control || m.alt || m.shift {
                    return;
                }
                let Some(shell) = live_shell.upgrade() else {
                    return;
                };
                let s = shell.read(cx);
                // Slides handles its own keys; a field or a dialog keeps them.
                let typing = window.context_stack().iter().any(|c| c.contains("Input"));
                if s.route() == ui_shell::Route::Documents
                    || s.overlay().is_some()
                    || s.quit_open()
                    || typing
                {
                    return;
                }
                if live_slides
                    .update(cx, |page, cx| page.step_live(delta, cx))
                    .unwrap_or(false)
                {
                    cx.stop_propagation();
                }
            })
            .detach();
            let slides = documents_page.downgrade();
            let shell_weak = shell.downgrade();
            cx.on_action(move |action: &ui_shell::keymap::WorkspaceCommand, cx| {
                let on_slides = shell_weak
                    .upgrade()
                    .is_some_and(|s| s.read(cx).route() == ui_shell::Route::Documents);
                if on_slides {
                    let _ = slides.update(cx, |page, cx| page.clicker(action.0.0, cx));
                }
            });
        }
        {
            let lyrics_model = lyrics_page.read(cx).model().clone();
            let scripture_model = scripture_page.read(cx).model().clone();
            let docs_page = documents_page.clone();
            let shell_weak = shell.downgrade();
            shell.update(cx, |shell, _| {
                shell.set_overlay_builder(
                    ui_shell::Overlay::SongSearch,
                    Box::new(move |_, window, cx| {
                        let mut entries = Vec::new();
                        for doc in docs_page.read(cx).documents() {
                            let id = doc.id.clone();
                            let page = docs_page.clone();
                            let nav = shell_weak.clone();
                            entries.push(ui_lyrics::SearchEntry {
                                id: format!("doc:{id}"),
                                scope: ui_lyrics::SearchScope::Slides,
                                title: doc.name.clone(),
                                subtitle: format!("{} slides", doc.pages.len()),
                                on_open: std::rc::Rc::new(move |cx| {
                                    page.update(cx, |p, cx| p.open_document(&id, cx));
                                    let _ = nav.update(cx, |s, cx| {
                                        s.close_overlay(cx);
                                        s.navigate(ui_shell::Route::Documents, cx);
                                    });
                                }),
                            });
                        }
                        for list in &lyrics_model.read(cx).setlists().lists {
                            let id = list.id.clone();
                            let model = lyrics_model.clone();
                            let nav = shell_weak.clone();
                            entries.push(ui_lyrics::SearchEntry {
                                id: format!("setlist:{id}"),
                                scope: ui_lyrics::SearchScope::Playlists,
                                title: list.name.clone(),
                                subtitle: format!(
                                    "Lyrics playlist · {} songs",
                                    list.song_ids.len()
                                ),
                                on_open: std::rc::Rc::new(move |cx| {
                                    model.update(cx, |m, cx| m.select_setlist(id.clone(), cx));
                                    let _ = nav.update(cx, |s, cx| {
                                        s.close_overlay(cx);
                                        s.navigate(ui_shell::Route::Lyrics, cx);
                                    });
                                }),
                            });
                        }
                        for plan in scripture_model.read(cx).plans() {
                            let id = plan.id.clone();
                            let model = scripture_model.clone();
                            let nav = shell_weak.clone();
                            entries.push(ui_lyrics::SearchEntry {
                                id: format!("plan:{id}"),
                                scope: ui_lyrics::SearchScope::Playlists,
                                title: plan.title.clone(),
                                subtitle: "Scripture playlist".into(),
                                on_open: std::rc::Rc::new(move |cx| {
                                    model.update(cx, |m, cx| m.open_plan(&id, cx));
                                    let _ = nav.update(cx, |s, cx| {
                                        s.close_overlay(cx);
                                        s.navigate(ui_shell::Route::Scripture, cx);
                                    });
                                }),
                            });
                        }
                        for lib in &lyrics_model.read(cx).libraries().libraries {
                            let id = lib.id.clone();
                            let model = lyrics_model.clone();
                            let nav = shell_weak.clone();
                            entries.push(ui_lyrics::SearchEntry {
                                id: format!("song-lib:{id}"),
                                scope: ui_lyrics::SearchScope::Libraries,
                                title: lib.name.clone(),
                                subtitle: "Lyrics library".into(),
                                on_open: std::rc::Rc::new(move |cx| {
                                    model.update(cx, |m, cx| m.select_library(id.clone(), cx));
                                    let _ = nav.update(cx, |s, cx| {
                                        s.close_overlay(cx);
                                        s.navigate(ui_shell::Route::Lyrics, cx);
                                    });
                                }),
                            });
                        }
                        for lib in &scripture_model.read(cx).libraries().libraries {
                            let id = lib.id.clone();
                            let model = scripture_model.clone();
                            let nav = shell_weak.clone();
                            entries.push(ui_lyrics::SearchEntry {
                                id: format!("scripture-lib:{id}"),
                                scope: ui_lyrics::SearchScope::Libraries,
                                title: lib.name.clone(),
                                subtitle: "Scripture library".into(),
                                on_open: std::rc::Rc::new(move |cx| {
                                    model.update(cx, |m, cx| m.select_library(id.clone(), cx));
                                    let _ = nav.update(cx, |s, cx| {
                                        s.close_overlay(cx);
                                        s.navigate(ui_shell::Route::Scripture, cx);
                                    });
                                }),
                            });
                        }
                        let model = lyrics_model.clone();
                        let finder = cx.new(|cx| {
                            ui_lyrics::QuickOpen::new(model, window, cx).with_entries(entries, cx)
                        });
                        let close = shell_weak.clone();
                        cx.subscribe(&finder, move |_, _: &ui_lyrics::QuickOpenEvent, cx| {
                            let _ = close.update(cx, |s, cx| s.close_overlay(cx));
                        })
                        .detach();
                        finder.into()
                    }),
                )
            });
        }
        let imagine_page = cx.new(|cx| ui_imagine::ImaginePage::new(imagine_backend, window, cx));
        {
            // A deck saved from Imagine shows up in Slides at once.
            let slides = documents_page.downgrade();
            let shell = shell.downgrade();
            cx.subscribe(
                &imagine_page,
                move |_, event: &ui_imagine::ImagineEvent, cx| match event {
                    ui_imagine::ImagineEvent::SlidesSaved => {
                        let _ = slides.update(cx, |page, cx| page.refresh(cx));
                    }
                    // The Media dock lives beside the operator booth.
                    ui_imagine::ImagineEvent::OpenMedia => {
                        let _ = shell.update(cx, |s, cx| s.navigate(ui_shell::Route::Operator, cx));
                    }
                },
            )
            .detach();
        }
        {
            // Imagine opens on Templates each time it is picked from the rail.
            let page = imagine_page.downgrade();
            let last = std::cell::Cell::new(shell.read(cx).route());
            cx.observe(&shell, move |shell, cx| {
                let route = shell.read(cx).route();
                if route != last.replace(route) && route == ui_shell::Route::Imagine {
                    let _ = page.update(cx, |p, cx| p.show_templates(cx));
                }
            })
            .detach();
        }
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Imagine, imagine_page.into(), cx)
        });
        let audience_page = audience::install(audience_backend, &workspace, window, cx);
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Audience, audience_page.into(), cx)
        });
        let web_page = web::install(web_backend, &workspace, &shell, window, cx);
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Web, web_page.into(), cx)
        });
        let teleprompter = operator::install_teleprompter(
            teleprompter_services,
            teleprompter_ui_state,
            window,
            cx,
        );
        let teleprompter_view = teleprompter.downgrade();
        cx.observe(&shell, move |shell, cx| {
            let shell = shell.read(cx);
            let active = shell.route() == ui_shell::Route::Teleprompter && !shell.covered();
            let _ = teleprompter_view.update(cx, |view, _| view.set_active(active));
        })
        .detach();
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Teleprompter, teleprompter.into(), cx)
        });
        let theme_picker = cx.new(|cx| ui_theme::ThemePicker::new(theme_backend.clone(), cx));
        let theme_page = theme::install(theme_backend, window, cx);
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Theme, theme_page.into(), cx);
            let picker = theme_picker.clone();
            shell.set_screen_themes(
                theme_picker.into(),
                std::rc::Rc::new(move |lyrics, _, cx| {
                    picker.update(cx, |p, cx| p.opened(lyrics, cx))
                }),
                cx,
            );
        });
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Lyrics, lyrics_page.clone().into(), cx)
        });
        shell.update(cx, |shell, cx| {
            shell.set_workspace(ui_shell::Route::Operator, workspace.into(), cx)
        });
        transfer::install(&transfer_services, &shell, window, cx);
        capture::install(&transfer_services, &shell, cx);
        #[cfg(feature = "dev-hooks")]
        benchmark::install(
            &shell,
            &media_dock,
            &lyrics_page,
            transfer_services.clone(),
            window,
            cx,
        );
        let reporter = display_reporting::ControlDisplayReporter::new(window, cx, engine);
        cx.set_global(DisplayReporter {
            _reporter: reporter,
        });
        shell
    });
    let (handle, shell) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            tracing::error!(%error, "[Shell] could not open the operator window");
            if cfg!(target_os = "linux") {
                // Without a window there is nothing else to show this on.
                startup_error::report(
                    startup_error::NO_WINDOW_TITLE,
                    &startup_error::no_window_text(&format!("{error:#}")),
                );
                std::process::exit(1);
            }
            cx.quit();
            return;
        }
    };
    let requests_services = services.clone();
    cx.subscribe(&shell, move |_, request: &ShellRequest, cx| {
        handle_request(request, &requests_services, cx)
    })
    .detach();
    let pending = {
        let window = cx.default_global::<OperatorWindow>();
        window.handle = Some(handle);
        window.shell = Some(shell.downgrade());
        std::mem::take(&mut window.pending)
    };
    let close_shell = shell.downgrade();
    let _ = handle.update(cx, |_, window, cx| {
        shell.update(cx, |shell, cx| {
            shell.set_state(state, cx);
            shell.begin_launch(cx);
            shell.open_kairo_files(pending, cx);
            shell.mark_ready(window, cx);
            // Builds with the `dev-hooks` feature only (never shipped):
            // `KAIRO_OPEN_SETTINGS=<pane>` drops setup and the sign-in wall
            // for this launch and opens Settings, for measuring and
            // screenshots (074).
            #[cfg(feature = "dev-hooks")]
            if std::env::var_os("KAIRO_OPEN_SCREENS").is_some() {
                cx.spawn(async move |shell, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(5))
                        .await;
                    let _ = shell.update(cx, |shell, cx| {
                        shell.set_front(None, cx);
                        shell.open_overlay(ui_shell::Overlay::Screens, cx);
                    });
                })
                .detach();
            }
            #[cfg(feature = "dev-hooks")]
            if let Some(section) = std::env::var("KAIRO_OPEN_SETTINGS")
                .ok()
                .and_then(|id| ui_settings::Section::from_id(&id))
            {
                // After the launch screen, as an operator would open it.
                cx.spawn(async move |shell, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(5))
                        .await;
                    // Three opens: the first pays one-time costs (glyphs,
                    // icons); the others are what an operator sees later.
                    for _ in 0..3 {
                        let _ = shell.update(cx, |shell, cx| {
                            shell.set_front(None, cx);
                            shell.open_settings(section.id(), cx);
                        });
                        cx.background_executor()
                            .timer(std::time::Duration::from_secs(1))
                            .await;
                        let _ = shell.update(cx, |shell, cx| shell.close_overlay(cx));
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                    }
                    let _ = shell.update(cx, |shell, cx| {
                        shell.open_settings(section.id(), cx);
                    });
                })
                .detach();
            }
        });
        shell.read(cx).focus_handle(cx).focus(window, cx);
        report_first_paint(window);
        // Every close asks first; confirming quits, as `respondToQuit` does.
        window.on_window_should_close(cx, move |window, cx| {
            let Some(shell) = close_shell.upgrade() else {
                return true;
            };
            if shell.update(cx, |shell, cx| shell.request_quit(window, cx)) {
                cx.quit();
            }
            false
        });
    });
}

/// The app's Settings backend, kept for a reopened window.
struct SettingsBackendHandle(Arc<dyn ui_settings::SettingsBackend>);
impl Global for SettingsBackendHandle {}

/// Gives the shell the Settings window (074) in place of its placeholder.
fn install_settings(
    shell: &Entity<Shell>,
    flow: &Entity<ui_account::AccountFlow>,
    account: Entity<ui_account::Account>,
    backend: Arc<dyn ui_settings::SettingsBackend>,
    ui_state: Arc<dyn ui_operator::UiState>,
    cx: &mut App,
) {
    let target = shell.downgrade();
    let flow = flow.downgrade();
    let build = move |section: Option<&'static str>, window: &mut Window, cx: &mut App| {
        let frame = settings_frame(&*ui_state, window);
        let initial = section.and_then(ui_settings::Section::from_id);
        let backend = backend.clone();
        let account = account.clone();
        let view = cx.new(|cx| {
            ui_settings::SettingsWindow::new(backend, account, initial, frame, window, cx)
        });
        let target = target.clone();
        let flow = flow.clone();
        cx.subscribe(
            &view,
            move |view, event: &ui_settings::SettingsEvent, cx| {
                view.update(cx, |view, cx| view.close(cx));
                let _ = target.update(cx, |shell, cx| shell.close_overlay(cx));
                if *event == ui_settings::SettingsEvent::ShowTips {
                    let _ = flow.update(cx, |flow, cx| flow.replay_tips(cx));
                }
                // `RunSetup`: the reset onboarding state reaches the account
                // model, and the flow opens setup over the booth.
            },
        )
        .detach();
        AnyView::from(view)
    };
    shell.update(cx, |shell, _| {
        shell.set_overlay_builder(ui_shell::Overlay::Settings, Box::new(build))
    });
}

/// Installs the Screens configuration window over the existing shell overlay.
fn install_screens(
    shell: &Entity<Shell>,
    backend: Arc<dyn ui_screens::ScreensBackend>,
    _window: &mut Window,
    cx: &mut App,
) {
    let target = shell.downgrade();
    shell.update(cx, |shell, _| {
        shell.set_overlay_builder(
            ui_shell::Overlay::Screens,
            Box::new(move |initial, window, cx| {
                let viewport = window.viewport_size();
                let saved = backend
                    .read_ui("kairo.frame.screens.v4")
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
                let width = saved
                    .as_ref()
                    .and_then(|s| s["w"].as_f64())
                    .unwrap_or(ui_screens::FRAME.0 as f64) as f32;
                let height = saved
                    .as_ref()
                    .and_then(|s| s["h"].as_f64())
                    .unwrap_or(ui_screens::FRAME.1 as f64) as f32;
                let frame = (
                    width
                        .max(ui_screens::FRAME_MIN.0)
                        .min(f32::from(viewport.width) - 32.),
                    height
                        .max(ui_screens::FRAME_MIN.1)
                        .min(f32::from(viewport.height) - 32.),
                );
                let view = cx.new(|cx| {
                    ui_screens::ScreensWindow::new(backend.clone(), initial, frame, window, cx)
                });
                let target = target.clone();
                let window_handle = window.window_handle();
                cx.subscribe(&view, move |view, _: &ui_screens::ScreensEvent, cx| {
                    let _ = window_handle.update(cx, |_, window, cx| {
                        view.update(cx, |v, cx| v.close(window, cx))
                    });
                    let _ = target.update(cx, |shell, cx| shell.close_overlay(cx));
                })
                .detach();
                AnyView::from(view)
            }),
        )
    });
}

/// What reopening the window from the Dock needs.
struct Reopen {
    engine: kairo_core::engine::EngineHandle,
    settings: Settings,
    paths: AppPaths,
    services: Arc<services::ApplicationServices>,
}
impl Global for Reopen {}

/// Keeps the control-display reporter alive with the window.
struct DisplayReporter {
    _reporter: display_reporting::ControlDisplayReporter,
}
impl Global for DisplayReporter {}

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

/// Main-thread work is delivered explicitly; services never retain a GPUI
/// context or block a worker on the event loop.
fn watch_services(events: std::sync::mpsc::Receiver<services::UiEvent>, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
            while let Ok(event) = events.try_recv() {
                cx.update(|cx| match event {
                    services::UiEvent::OpenWindow { services, ready } => {
                        let paths = services.paths().clone();
                        open_operator_window(
                            services.engine.clone(),
                            services.settings.clone(),
                            &paths,
                            services.clone(),
                            cx,
                        );
                        if cx.default_global::<OperatorWindow>().handle.is_some() {
                            cx.set_global(Reopen {
                                engine: services.engine.clone(),
                                settings: services.settings.clone(),
                                paths,
                                services: services.clone(),
                            });
                            power::observe(services.clone(), cx);
                            receive_files(LAUNCH_FILES.mark_ready(), cx);
                            let _ = ready.send(Ok(()));
                            cx.activate(true);
                        } else {
                            let _ = ready.send(Err("The operator window did not open.".into()));
                        }
                    }
                    services::UiEvent::MainThread(job) => job(),
                    services::UiEvent::State(state) => {
                        if let Some(shell) = cx
                            .default_global::<OperatorWindow>()
                            .shell
                            .as_ref()
                            .and_then(|s| s.upgrade())
                        {
                            shell.update(cx, |shell, cx| shell.set_state(*state, cx));
                        }
                    }
                    services::UiEvent::Bootstrap { generation, report } => {
                        if let Some(shell) = cx
                            .default_global::<OperatorWindow>()
                            .shell
                            .as_ref()
                            .and_then(|s| s.upgrade())
                        {
                            shell.update(cx, |shell, cx| match report {
                                services::BootstrapReport::Progress(progress) => {
                                    shell.bootstrap_progress(generation, progress, cx)
                                }
                                services::BootstrapReport::Finished(result) => {
                                    shell.bootstrap_finished(generation, result.map(|s| *s), cx)
                                }
                            });
                        }
                    }
                    services::UiEvent::Feedback(result) => {
                        if let Some(shell) = cx
                            .default_global::<OperatorWindow>()
                            .shell
                            .as_ref()
                            .and_then(|s| s.upgrade())
                        {
                            shell.update(cx, |shell, cx| {
                                shell.feedback_result(result.err().map(Into::into), cx)
                            });
                        }
                    }
                    services::UiEvent::ConfirmInstall(services) => {
                        if let Some(handle) = cx.default_global::<OperatorWindow>().handle {
                            let _ = handle.update(cx, |_, window, cx| {
                                let response = window.prompt(
                                    PromptLevel::Warning,
                                    "Install the update and restart Kairo?",
                                    Some("Live output is running. Restarting stops the output."),
                                    &["Install and restart", "Cancel"],
                                    cx,
                                );
                                let updater = services.updater.clone();
                                cx.spawn(async move |_| {
                                    if response.await == Ok(0) {
                                        services.spawn(async move {
                                            updater.install(true).await;
                                        });
                                    }
                                })
                                .detach();
                            });
                        }
                    }
                    services::UiEvent::Account(update) => {
                        if let Some(model) = ui_account::model::account(cx) {
                            model.update(cx, |model, cx| match *update {
                                account::AccountUpdate::Session(v) => model.set_session(v, cx),
                                account::AccountUpdate::Pairing(v) => model.set_pairing(v, cx),
                                account::AccountUpdate::License(v) => model.set_license(*v, cx),
                                account::AccountUpdate::Onboarding(v) => {
                                    model.set_onboarding(v, cx)
                                }
                                account::AccountUpdate::Settings(v) => {
                                    // `display.fontSize` and `transcriptionFontSize`
                                    // take effect once saved (074).
                                    apply_display_sizes(&v.settings.display, cx);
                                    model.set_settings(v.settings, cx)
                                }
                                account::AccountUpdate::Displays(v) => model.set_displays(v, cx),
                            });
                        }
                    }
                    services::UiEvent::Quit => cx.quit(),
                });
            }
        }
    })
    .detach();
}

fn main() {
    let _ = LAUNCHED.set(std::time::Instant::now());
    #[cfg(feature = "dev-hooks")]
    let detected = benchmark::paths();
    #[cfg(not(feature = "dev-hooks"))]
    let detected = kairo_store::paths::detect();
    let paths = match detected {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("Kairo cannot find its data folder: {error}");
            std::process::exit(1);
        }
    };
    let _log = start(&paths);
    // Reading the system root certificates takes about 100 ms on macOS;
    // do it beside window setup instead of in the first service that
    // builds an HTTP client (prompt 107).
    std::thread::Builder::new()
        .name("kairo-tls-warm".into())
        .spawn(kairo_net::transport::warm_tls)
        .ok();

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

    // Linux runs on X11 only (XWayland in a Wayland session): the WebKitGTK
    // views behind web pages, Slido and PowerPoint import need it (DECISIONS,
    // "Replace CEF with gpui-webview"). Without a display GPUI would panic.
    #[cfg(target_os = "linux")]
    let application = {
        if std::env::var_os("DISPLAY").is_none_or(|d| d.is_empty()) {
            startup_error::report(
                startup_error::NO_DISPLAY_TITLE,
                startup_error::NO_DISPLAY_TEXT,
            );
            std::process::exit(1);
        }
        platform::linux(WindowingModes::X11)
    };
    #[cfg(not(target_os = "linux"))]
    let application = application();
    let application = application.with_assets(kairo_ui_kit::KairoAssets);
    // macOS can deliver the launching file before the window exists.
    application.on_open_urls(|urls| {
        let late = LAUNCH_FILES.push(kairo_paths_from_urls(&urls));
        LATE_FILES
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend(late);
    });
    // macOS: the Dock icon brings the window back if it was closed.
    application.on_reopen(|cx| {
        if !cx.windows().is_empty() || !cx.has_global::<Reopen>() {
            return;
        }
        let reopen = cx.global::<Reopen>();
        let (engine, settings, paths, services) = (
            reopen.engine.clone(),
            reopen.settings.clone(),
            reopen.paths.clone(),
            reopen.services.clone(),
        );
        open_operator_window(engine, settings, &paths, services.clone(), cx);
        let core = services.core.clone();
        services.spawn(async move {
            let _ = core.call(kairo_output::actor::ReopenScreens {}).await;
        });
    });
    application.run(move |cx| {
        cx.set_app_identity(APP_ID, "Kairo");
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
        motion::apply(cx);
        ui_shell::keymap::bind(cx);
        cx.set_menus(ui_shell::menus::app_menus(
            cfg!(target_os = "macos"),
            ui_shell::menus::ScreensMenu::default(),
            ui_shell::Route::Operator,
        ));
        dock::set_icon(include_bytes!("../resources/icon.png"));

        let core = match kairo_core::AppCore::new() {
            Ok(core) => core,
            Err(error) => {
                tracing::error!(%error, "Could not start application services");
                cx.quit();
                return;
            }
        };
        let binary = std::env::var_os("KAIRO_ENGINE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let exe = std::env::current_exe().expect("Kairo executable");
                #[cfg(target_os = "macos")]
                if let Some(dir) = exe.parent() {
                    let helper =
                        dir.join("../Helpers/Kairo Engine.app/Contents/MacOS/kairo-engine");
                    if helper.is_file() {
                        return helper;
                    }
                }
                exe.with_file_name(if cfg!(windows) {
                    "kairo-engine.exe"
                } else {
                    "kairo-engine"
                })
            });
        let engine =
            kairo_core::engine::EngineHandle::new(kairo_core::engine::EngineOptions::new(binary));
        let (bridge, events) = services::UiBridge::channel();
        let bridge = bridge.with_slido(ui_web::SlidoWebHost::install(
            cx,
            ui_web::HostOptions::new(paths.user_data.join("Partitions/audience-slido")),
        ));
        watch_services(events, cx);
        let boot_core = core.clone();
        let boot_engine = engine.clone();
        let boot_paths = paths.clone();
        let failure_bridge = bridge.clone();
        cx.background_spawn(async move {
            let run_core = boot_core.clone();
            let result = boot_core
                .run(async move {
                    let profile = boot_paths.user_data.clone();
                    let default_workspace = boot_paths.default_workspace_root();
                    let settings = tokio::task::spawn_blocking(move || {
                        // First native launch on an Electron profile (100):
                        // before any store opens.
                        let protector = kairo_store::secrets::KeyringProtector;
                        let devices = || {
                            kairo_stt::devices::list_audio_input_devices()
                                .into_iter()
                                .map(|d| kairo_store::migration::devices::InputDevice {
                                    id: d.id,
                                    label: d.label,
                                })
                                .collect()
                        };
                        let report =
                            kairo_store::migration::run(&kairo_store::migration::MigrationInput {
                                user_data: &profile,
                                default_workspace,
                                protector: &protector,
                                electron_app: "Kairo",
                                devices: &devices,
                                now: chrono::Local::now(),
                            });
                        let _ = MIGRATION_NOTES
                            .set(report.attention().into_iter().map(str::to_owned).collect());
                        let mut new_id = kairo_store::settings::random_uuid;
                        Arc::new(Mutex::new(SettingsStore::open(
                            &profile,
                            &mut kairo_store::settings::LaunchContext {
                                now_ms: kairo_store::settings::now_ms(),
                                new_id: &mut new_id,
                            },
                        )))
                    })
                    .await
                    .map_err(|e| e.to_string())?;
                    services::Startup::new(run_core, boot_paths, settings, boot_engine, bridge)?
                        .start()
                        .await
                })
                .await;
            if !matches!(&result, Ok(Ok(()))) {
                tracing::error!(?result, "Application startup failed");
                let _ = failure_bridge.send(services::UiEvent::Quit);
            }
        })
        .detach();
        let shutdown = engine.clone();
        cx.on_app_quit(move |cx| {
            let shutdown = shutdown.clone();
            let services = cx.try_global::<Reopen>().map(|r| r.services.clone());
            cx.background_spawn(async move {
                if let Some(services) = services {
                    services.updater.on_quit();
                    let core = services.core.clone();
                    if let Err(error) = core.shutdown(services).await {
                        tracing::warn!(%error, "Application teardown failed");
                    }
                }
                shutdown.shutdown();
            })
        })
        .detach();
        watch_incoming(instance, cx);
        cx.activate(true);
    });
}

/// The rail's Audio popover: the saved input and its choices from the
/// settings backend, and the capture's level from the core.
fn install_audio_rail(
    shell: &Entity<Shell>,
    backend: Arc<dyn ui_settings::SettingsBackend>,
    services: &Arc<services::ApplicationServices>,
    cx: &mut App,
) {
    let rail = cx.new(|_| ui_settings::AudioRail::new(backend));
    // The core publishes levels only while capturing; the latest is kept
    // here and read on the UI thread when it changes.
    let latest = Arc::new(std::sync::Mutex::new(
        None::<kairo_domain::status::AudioLevel>,
    ));
    let writer = latest.clone();
    let mut levels = services.core.watch::<kairo_domain::status::AudioLevel>();
    services.spawn(async move {
        while let Some(level) = levels.next().await {
            *writer.lock().unwrap_or_else(|e| e.into_inner()) = Some((*level).clone());
        }
    });
    let target = rail.downgrade();
    cx.spawn(async move |cx| {
        let mut seen = f64::NAN;
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(50))
                .await;
            let level = latest.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let Some(level) = level.filter(|l| l.timestamp != seen) else {
                if target.upgrade().is_none() {
                    break;
                }
                continue;
            };
            seen = level.timestamp;
            if target.update(cx, |r, cx| r.set_level(level, cx)).is_err() {
                break;
            }
        }
    })
    .detach();
    let settings = shell.downgrade();
    cx.subscribe(&rail, move |_, _: &ui_settings::OpenAudioSettings, cx| {
        let _ = settings.update(cx, |shell, cx| shell.open_settings("audio", cx));
    })
    .detach();
    let opened = rail.clone();
    shell.update(cx, |shell, cx| {
        shell.set_audio(
            rail.into(),
            std::rc::Rc::new(move |open, _, cx| {
                opened.update(cx, |r, cx| if open { r.opened(cx) } else { r.closed(cx) })
            }),
            cx,
        )
    });
}
