//! UI integration tests for the launch screen, driven by a fake core: the
//! test answers the shell's bootstrap requests itself, including loads that
//! time out, fail, or never answer.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, Global, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};
use kairo_core::bootstrap::{BootstrapLoaders, neutral_snapshot, run_bootstrap};
use kairo_domain::status::{AppBootstrapSnapshot, BootstrapProgress};
use std::time::Duration;
use ui_shell::bootstrap::{SPLASH_FALLBACK_MS, SPLASH_MIN_VISIBLE_MS};
use ui_shell::launch::ids;
use ui_shell::{Shell, ShellOptions, ShellRequest};

#[derive(Default)]
struct Seen(Vec<ShellRequest>);
impl Global for Seen {}

fn setup(cx: &mut TestAppContext, reduced: bool) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
        kairo_ui_kit::set_reduced_motion(reduced, cx);
        cx.set_reduce_motion(reduced);
        cx.set_global(Seen::default());
    });
}

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Shell>) {
    let (handle, shell) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    Shell::new(
                        ShellOptions {
                            version: "1.0.0".into(),
                            dev: false,
                            user_data: None,
                            splash: true,
                        },
                        window,
                        cx,
                    )
                })
            },
        )
        .expect("open shell")
    });
    cx.update(|cx| {
        cx.subscribe(&shell, |_, request: &ShellRequest, cx| {
            cx.global_mut::<Seen>().0.push(request.clone())
        })
        .detach();
        shell.update(cx, |shell, cx| shell.begin_launch(cx));
    });
    (handle, shell)
}

fn bootstraps(cx: &mut TestAppContext) -> Vec<u64> {
    cx.update(|cx| {
        cx.global::<Seen>()
            .0
            .iter()
            .filter_map(|r| match r {
                ShellRequest::Bootstrap { generation } => Some(*generation),
                _ => None,
            })
            .collect()
    })
}

fn advance(cx: &mut TestAppContext, handle: AnyWindowHandle, ms: u64) {
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
    frame(cx, handle);
}

fn frame(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn has(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> bool {
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

fn label(cx: &mut TestAppContext, handle: AnyWindowHandle, id: &'static str) -> Option<String> {
    cx.update_window(handle, |_, window, _| {
        window.find(id).label().map(str::to_owned)
    })
    .unwrap()
}

fn finish(
    cx: &mut TestAppContext,
    shell: &Entity<Shell>,
    generation: u64,
    result: Result<AppBootstrapSnapshot, String>,
) {
    cx.update(|cx| shell.update(cx, |s, cx| s.bootstrap_finished(generation, result, cx)));
}

fn progress(cx: &mut TestAppContext, shell: &Entity<Shell>, generation: u64, p: BootstrapProgress) {
    cx.update(|cx| shell.update(cx, |s, cx| s.bootstrap_progress(generation, p, cx)));
}

/// A fake core's bootstrap: \`run_bootstrap\` with the loaders given, on a
/// paused Tokio clock so the eight-second ceiling passes at once.
fn fake_core(loaders: BootstrapLoaders) -> (Vec<BootstrapProgress>, AppBootstrapSnapshot) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .start_paused(true)
        .build()
        .expect("runtime");
    let mut reports = Vec::new();
    let snapshot = runtime.block_on(run_bootstrap(loaders, |p| reports.push(p)));
    (reports, snapshot)
}

fn loaders(account_hangs: bool) -> BootstrapLoaders {
    let neutral = neutral_snapshot();
    let onboarding = neutral.onboarding.clone();
    let account = neutral.account.clone();
    BootstrapLoaders {
        // Settings always fail here: the disk read is refused.
        settings: Box::pin(async { Err("EACCES: permission denied".into()) }),
        orchestrator: Box::pin(std::future::pending()),
        transcription: Box::pin(async { Ok(Vec::new()) }),
        translations: Box::pin(async { Ok(Vec::new()) }),
        sermon_plans: Box::pin(async { Ok(Vec::new()) }),
        live_plan: Box::pin(std::future::pending()),
        lyrics: Box::pin(async { Ok(Vec::new()) }),
        onboarding: Box::pin(async move { Ok(onboarding) }),
        account: Box::pin(async move {
            if account_hangs {
                std::future::pending::<()>().await;
            }
            Ok(account)
        }),
    }
}

#[gpui_kit::test]
fn splash_waits_for_the_intro_then_opens(cx: &mut TestAppContext) {
    setup(cx, false);
    let (handle, shell) = open(cx);
    assert_eq!(bootstraps(cx), vec![1], "one bootstrap per window");
    frame(cx, handle);
    assert!(has(cx, handle, ids::SCREEN));
    assert!(has(cx, handle, ids::BOOT), "Powering up, not progress");
    assert!(!has(cx, handle, "app-header"));

    // Loaded long before the brand beat ends: the splash stays.
    finish(cx, &shell, 1, Ok(neutral_snapshot()));
    advance(cx, handle, 1_000);
    assert!(has(cx, handle, ids::SCREEN));
    assert!(!has(cx, handle, ids::PROGRESS), "no progress once loaded");

    // The dot lands at 2550 ms; the splash closes a beat (450 ms) later.
    advance(cx, handle, 1_600);
    assert!(has(cx, handle, ids::SCREEN));
    advance(cx, handle, 460);
    assert!(!has(cx, handle, ids::SCREEN));
    assert!(has(cx, handle, "app-header"));
    assert!(!has(cx, handle, ids::WARNING));
    // Starting again does nothing.
    cx.update(|cx| shell.update(cx, |s, cx| s.begin_launch(cx)));
    assert_eq!(bootstraps(cx), vec![1]);
}

#[gpui_kit::test]
fn hidden_window_falls_back_to_the_timer(cx: &mut TestAppContext) {
    setup(cx, false);
    let (handle, shell) = open(cx);
    finish(cx, &shell, 1, Ok(neutral_snapshot()));
    // No frames drawn: the intro never reports, so the fallback decides.
    cx.executor()
        .advance_clock(Duration::from_millis(SPLASH_FALLBACK_MS - 10));
    cx.run_until_parked();
    cx.update(|cx| assert!(!shell.read(cx).launch().ready()));
    cx.executor().advance_clock(Duration::from_millis(20));
    cx.run_until_parked();
    cx.update(|cx| assert!(shell.read(cx).launch().ready()));
    frame(cx, handle);
    assert!(has(cx, handle, "app-header"));
}

#[gpui_kit::test]
fn reduced_motion_holds_for_the_minimum_only(cx: &mut TestAppContext) {
    setup(cx, true);
    let (handle, shell) = open(cx);
    finish(cx, &shell, 1, Ok(neutral_snapshot()));
    advance(cx, handle, SPLASH_MIN_VISIBLE_MS - 10);
    assert!(has(cx, handle, ids::SCREEN));
    advance(cx, handle, 20);
    assert!(!has(cx, handle, ids::SCREEN));
    assert!(has(cx, handle, "app-header"));
}

#[gpui_kit::test]
fn slow_load_shows_real_progress_then_timeout_warning(cx: &mut TestAppContext) {
    setup(cx, true);
    let (handle, shell) = open(cx);
    // Loading outlasts the splash: progress replaces "Powering up".
    advance(cx, handle, SPLASH_MIN_VISIBLE_MS + 10);
    assert!(has(cx, handle, ids::PROGRESS));
    assert!(!has(cx, handle, ids::BOOT));
    assert_eq!(label(cx, handle, ids::PROGRESS).as_deref(), Some("0%"));

    // The fake core loads, with the orchestrator, live playlist and account
    // never answering and settings failing.
    let (reports, snapshot) = fake_core(loaders(true));
    for report in reports.iter().take(reports.len() - 1).cloned() {
        progress(cx, &shell, 1, report);
    }
    frame(cx, handle);
    let last = &reports[reports.len() - 2];
    let percent = (last.completed / last.total * 100.0).round() as u32;
    assert_eq!(
        label(cx, handle, ids::PROGRESS),
        Some(format!("{percent}%"))
    );
    assert!(has(cx, handle, ids::STEP));
    // Progress for another load is ignored.
    progress(
        cx,
        &shell,
        9,
        BootstrapProgress {
            completed: 9.0,
            total: 9.0,
            step: "Ready".into(),
        },
    );
    frame(cx, handle);
    assert_eq!(
        label(cx, handle, ids::PROGRESS),
        Some(format!("{percent}%"))
    );

    assert_eq!(reports.last().map(|r| r.step.as_str()), Some("Ready"));
    let timeouts: Vec<&str> = snapshot
        .errors
        .iter()
        .map(|e| e.message.as_str())
        .filter(|m| m.contains("timed out after 8000ms"))
        .collect();
    assert_eq!(timeouts.len(), 3, "{:?}", snapshot.errors);

    progress(cx, &shell, 1, reports.last().cloned().unwrap());
    finish(cx, &shell, 1, Ok(snapshot));
    frame(cx, handle);
    assert!(
        has(cx, handle, "app-header"),
        "a failure never blocks entry"
    );
    let warning = label(cx, handle, ids::WARNING).expect("warning");
    assert!(warning.starts_with("Some data could not be loaded at startup: "));
    for name in ["settings", "automation status", "live playlist", "account"] {
        assert!(warning.contains(name), "{warning}");
    }

    // Dismiss hides it for this load.
    cx.update_window(handle, |_, window, cx| window.click(ids::DISMISS, cx))
        .unwrap();
    frame(cx, handle);
    assert!(!has(cx, handle, ids::WARNING));
}

#[gpui_kit::test]
fn failed_request_opens_with_settings_warning_and_retry_reloads(cx: &mut TestAppContext) {
    setup(cx, true);
    let (handle, shell) = open(cx);
    advance(cx, handle, SPLASH_MIN_VISIBLE_MS + 10);
    // The request itself failed (the fake core dropped it).
    finish(
        cx,
        &shell,
        1,
        Err("The startup request did not finish.".into()),
    );
    frame(cx, handle);
    assert_eq!(
        label(cx, handle, ids::WARNING).as_deref(),
        Some("Some data could not be loaded at startup: settings.")
    );
    cx.update(|cx| {
        let launch = shell.read(cx).launch();
        assert_eq!(
            launch.errors()[0].message,
            "The startup request did not finish."
        );
        assert!(launch.snapshot().is_none());
    });

    // Retry asks again and shows the progress screen meanwhile.
    cx.update_window(handle, |_, window, cx| window.click(ids::RETRY, cx))
        .unwrap();
    frame(cx, handle);
    assert_eq!(bootstraps(cx), vec![1, 2]);
    assert!(has(cx, handle, ids::PROGRESS));
    assert!(!has(cx, handle, "app-header"));

    // A late answer to the first request changes nothing.
    finish(cx, &shell, 1, Ok(neutral_snapshot()));
    frame(cx, handle);
    assert!(has(cx, handle, ids::PROGRESS));

    let (_, snapshot) = fake_core(loaders(false));
    finish(cx, &shell, 2, Ok(snapshot));
    frame(cx, handle);
    assert!(has(cx, handle, "app-header"));
    // Only the settings loader (and two that never answer) failed this time.
    let warning = label(cx, handle, ids::WARNING).expect("warning");
    assert!(
        warning.contains("settings") && !warning.contains("account"),
        "{warning}"
    );
    cx.update(|cx| assert!(shell.read(cx).launch().snapshot().is_some()));
}
