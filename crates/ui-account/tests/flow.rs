//! UI integration tests for what sits in front of the booth: first-run setup,
//! the sign-in wall, email confirmation and the coach tips.

use std::sync::{Arc, Mutex};

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::cloud::{
    CloudResult, CloudSessionState, CloudUser, DevicePairingState, LicenseSnapshot,
    SessionSnapshot, SignInInput, SignUpInput,
};
use kairo_domain::program::PresentationSettings;
use kairo_domain::settings::{ChurchProfile, OnboardingState, OverlaySettings};
use ui_account::coach::{CoachTourId, ids as coach_ids};
use ui_account::flow::coach_tour_for;
use ui_account::wizard::{QuickStart, ids as wizard_ids};
use ui_account::{Account, AccountBackend, AccountFlow, FlowEvent, OnboardingWrite, Reply};

// Thread-local allocation counter for the explicit release benchmark only.
struct CountingAllocator;
thread_local! {
    static COUNT_ALLOCATIONS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn count_allocation() {
    let _ = COUNT_ALLOCATIONS.try_with(|enabled| {
        if enabled.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}
unsafe impl std::alloc::GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        count_allocation();
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[derive(Default)]
struct Calls {
    sign_in: Vec<SignInInput>,
    resend: usize,
    resets: usize,
    overlays: usize,
    writes: Vec<OnboardingWrite>,
}

struct Fake {
    calls: Mutex<Calls>,
    /// What sign-in answers with.
    session: SessionSnapshot,
    reset_reply: Mutex<Option<Reply<Result<(), String>>>>,
    write_reply: Mutex<Option<Reply<Result<OnboardingState, String>>>>,
    sign_out_reply: Mutex<Option<Reply<Result<SessionSnapshot, String>>>>,
    state: Mutex<OnboardingState>,
    overlay_reply: Mutex<Option<Reply<Result<(), String>>>>,
}

fn ready<T: Send + 'static>(value: T) -> Reply<T> {
    Box::pin(async move { value })
}

fn ok<T>(data: T) -> CloudResult<T> {
    CloudResult {
        ok: true,
        data: Some(data),
        error: None,
    }
}

impl AccountBackend for Fake {
    fn sign_in(&self, input: SignInInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        self.calls.lock().unwrap().sign_in.push(input);
        ready(Ok(ok(self.session.clone())))
    }
    fn sign_up(&self, _: SignUpInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        ready(Ok(ok(self.session.clone())))
    }
    fn sign_out(&self) -> Reply<Result<SessionSnapshot, String>> {
        self.sign_out_reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ready(Ok(ui_account::model::signed_out())))
    }
    fn request_password_reset(&self, _: String) -> Reply<Result<(), String>> {
        self.calls.lock().unwrap().resets += 1;
        self.reset_reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ready(Ok(())))
    }
    fn resend_verification(&self) -> Reply<Result<CloudResult<()>, String>> {
        self.calls.lock().unwrap().resend += 1;
        ready(Ok(ok(())))
    }
    fn verify_email_code(&self, _: String) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        ready(Ok(ok(session(true))))
    }
    fn start_pairing(&self) -> Reply<Result<DevicePairingState, String>> {
        ready(Ok(ui_account::model::idle_pairing()))
    }
    fn cancel_pairing(&self) -> Reply<Result<DevicePairingState, String>> {
        ready(Ok(ui_account::model::idle_pairing()))
    }
    fn open_web(&self, _: String) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
    fn activate_license(&self, _: String) -> Reply<Result<CloudResult<LicenseSnapshot>, String>> {
        ready(Err("not used".into()))
    }
    fn onboarding(&self, write: OnboardingWrite) -> Reply<Result<OnboardingState, String>> {
        self.calls.lock().unwrap().writes.push(write);
        let mut state = self.state.lock().unwrap();
        *state = match write {
            OnboardingWrite::Complete(step) => {
                kairo_domain::cloud::complete_step(state.clone(), step)
            }
            OnboardingWrite::Skip(step) => kairo_domain::cloud::skip_step(state.clone(), step),
            OnboardingWrite::SetCurrent(step) => {
                kairo_domain::cloud::set_current_step(state.clone(), step)
            }
            OnboardingWrite::Finish => kairo_domain::cloud::finish_onboarding(state.clone(), 1.),
        };
        self.write_reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ready(Ok(state.clone())))
    }
    fn save_church(&self, _: ChurchProfile) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
    fn save_overlay(&self, _: OverlaySettings) -> Reply<Result<(), String>> {
        self.calls.lock().unwrap().overlays += 1;
        self.overlay_reply
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| ready(Ok(())))
    }
    fn save_presentation(&self, _: PresentationSettings) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
}

fn session(verified: bool) -> SessionSnapshot {
    SessionSnapshot {
        state: CloudSessionState::Active,
        user: Some(CloudUser {
            id: "u1".into(),
            email: "op@church.test".into(),
            name: "Operator".into(),
            email_verified: verified,
        }),
        ..ui_account::model::signed_out()
    }
}

fn finished() -> OnboardingState {
    OnboardingState {
        completed_at: Some(1.0),
        ..Default::default()
    }
}

struct Opened {
    handle: AnyWindowHandle,
    account: Entity<Account>,
    flow: Entity<AccountFlow>,
    fake: Arc<Fake>,
    events: Arc<Mutex<Vec<FlowEvent>>>,
}

fn open(cx: &mut TestAppContext, onboarding: OnboardingState, start: SessionSnapshot) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let fake = Arc::new(Fake {
        calls: Mutex::default(),
        session: session(true),
        reset_reply: Mutex::default(),
        write_reply: Mutex::default(),
        sign_out_reply: Mutex::default(),
        state: Mutex::new(onboarding.clone()),
        overlay_reply: Mutex::default(),
    });
    let backend: Arc<dyn AccountBackend> = fake.clone();
    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(backend, "Africa/Lagos");
            account.set_onboarding(onboarding, cx);
            account.set_session(start, cx);
            account
        })
    });
    let model = account.clone();
    let (handle, flow) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(800.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| cx.new(|cx| AccountFlow::new(model, window, cx)),
        )
        .expect("open")
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&flow, move |_, event: &FlowEvent, _| {
            seen.lock().unwrap().push(*event)
        })
        .detach()
    });
    let opened = Opened {
        handle,
        account,
        flow,
        fake,
        events,
    };
    draw(cx, &opened);
    opened
}

fn draw(cx: &mut TestAppContext, opened: &Opened) {
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(window);
    })
    .unwrap();
}

fn advance(cx: &mut TestAppContext, opened: &Opened, millis: u64) {
    for _ in 0..millis.div_ceil(16) {
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(16));
        cx.run_until_parked();
        cx.update_window(opened.handle, |_, window, cx| {
            window.simulate_next_frame(cx);
            window.render_frame(cx);
        })
        .unwrap();
    }
}

/// Advances opening motion deterministically.
fn settle(cx: &mut TestAppContext, opened: &Opened) {
    advance(cx, opened, 600);
    draw(cx, opened);
}

fn shows(cx: &mut TestAppContext, opened: &Opened, id: &'static str) -> bool {
    cx.update_window(opened.handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: &'static str) {
    cx.update_window(opened.handle, |_, window, cx| window.click(id, cx))
        .unwrap();
    // Deliver fake animation frames while the page settles.
    advance(cx, opened, 700);
    draw(cx, opened);
}

fn type_into(cx: &mut TestAppContext, opened: &Opened, id: &'static str, text: &str) {
    cx.update_window(opened.handle, |_, window, cx| {
        window.click(id, cx);
        window.input(text, cx);
    })
    .unwrap();
    draw(cx, opened);
}

#[gpui_kit::test]
fn a_fresh_install_opens_setup_before_the_sign_in_wall(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    assert!(opened.flow.read_with(cx, |f, _| f.wizard_open()));
    settle(cx, &opened);
    assert!(shows(cx, &opened, wizard_ids::DIALOG));
    // Forms are reachable by role: the dialog is announced as one.
    let roles = cx
        .update_window(opened.handle, |_, window, _| {
            window.find(wizard_ids::DIALOG).role()
        })
        .unwrap();
    assert_eq!(roles, Some(gpui_kit::Role::Dialog));
    assert!(!shows(cx, &opened, ui_account::gate::ids::GATE));

    // Skip the tour: setup moves to the account step and records the skip.
    click(cx, &opened, wizard_ids::SKIP_TOUR);
    assert!(
        opened
            .fake
            .calls
            .lock()
            .unwrap()
            .writes
            .contains(&OnboardingWrite::Complete(
                kairo_domain::settings::OnboardingStepId::Welcome
            ))
    );
}

#[gpui_kit::test]
fn signed_out_after_setup_meets_the_wall_and_signing_in_opens_the_booth(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), ui_account::model::signed_out());
    assert!(opened.flow.read_with(cx, |f, _| f.gate_open()));
    assert!(shows(cx, &opened, ui_account::gate::ids::GATE));

    type_into(cx, &opened, ui_account::auth::ids::EMAIL, "op@church.test");
    type_into(
        cx,
        &opened,
        ui_account::auth::ids::PASSWORD,
        "correct horse",
    );
    click(cx, &opened, ui_account::auth::ids::SUBMIT);
    let calls = opened.fake.calls.lock().unwrap().sign_in.clone();
    assert_eq!(
        calls,
        vec![SignInInput {
            email: "op@church.test".into(),
            password: "correct horse".into()
        }]
    );
    // The confirmed session lets the operator in.
    assert!(!opened.flow.read_with(cx, |f, _| f.blocking()));
    assert!(!shows(cx, &opened, ui_account::gate::ids::GATE));
}

#[gpui_kit::test]
fn an_unconfirmed_email_keeps_the_wall_with_the_code_step(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), session(false));
    assert!(opened.flow.read_with(cx, |f, _| f.gate_open()));
    assert!(shows(cx, &opened, ui_account::verify::ids::STATUS));
    click(cx, &opened, ui_account::verify::ids::RESEND);
    assert_eq!(opened.fake.calls.lock().unwrap().resend, 1);

    // Confirmed elsewhere: the session update opens the booth.
    opened
        .account
        .update(cx, |a, cx| a.set_session(session(true), cx));
    draw(cx, &opened);
    assert!(!opened.flow.read_with(cx, |f, _| f.blocking()));
}

#[gpui_kit::test]
fn tips_replay_and_end_with_skip_or_the_last_tip(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), session(true));
    assert!(!opened.flow.read_with(cx, |f, _| f.blocking()));
    opened.flow.update(cx, |f, cx| f.replay_tips(cx));
    assert_eq!(
        *opened.events.lock().unwrap(),
        vec![FlowEvent::Navigate(None)]
    );
    assert!(opened.flow.read_with(cx, |f, _| f.coaching()));

    // A sign-out mid-tour puts the wall up and drops the tips.
    opened.account.update(cx, |a, cx| {
        a.set_session(ui_account::model::signed_out(), cx)
    });
    draw(cx, &opened);
    assert!(opened.flow.read_with(cx, |f, _| f.gate_open()));
    assert!(!opened.flow.read_with(cx, |f, _| f.coaching()));
    assert!(!shows(cx, &opened, coach_ids::CARD));
}

#[gpui_kit::test]
fn running_setup_again_brings_the_wizard_back(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), session(true));
    assert!(!opened.flow.read_with(cx, |f, _| f.wizard_open()));
    opened
        .account
        .update(cx, |a, cx| a.set_onboarding(OnboardingState::default(), cx));
    settle(cx, &opened);
    assert!(opened.flow.read_with(cx, |f, _| f.wizard_open()));
    assert!(shows(cx, &opened, wizard_ids::DIALOG));
}

#[test]
fn tips_follow_where_setup_sends_the_operator() {
    assert_eq!(coach_tour_for(None), CoachTourId::Operator);
    assert_eq!(
        coach_tour_for(Some(QuickStart::Scripture)),
        CoachTourId::Scripture
    );
    assert_eq!(
        coach_tour_for(Some(QuickStart::Lyrics)),
        CoachTourId::Lyrics
    );
    assert_eq!(
        coach_tour_for(Some(QuickStart::Documents)),
        CoachTourId::Documents
    );
}

#[gpui_kit::test]
fn tour_back_reverses_without_writing_setup(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    settle(cx, &opened);
    click(cx, &opened, wizard_ids::CONTINUE);
    assert!(shows(cx, &opened, wizard_ids::BACK));
    click(cx, &opened, wizard_ids::BACK);
    assert!(!shows(cx, &opened, wizard_ids::BACK));
    assert!(opened.fake.calls.lock().unwrap().writes.is_empty());
}

#[gpui_kit::test]
fn tour_tracks_drag_and_returns_with_back(cx: &mut TestAppContext) {
    use gpui_kit::test::TestEventExt;
    use gpui_kit::{MouseButton, point};
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    settle(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        let bounds = window.find("onboarding-pager").bounds();
        assert!(bounds.size.width > px(1000.));
        let start = bounds.center() + point(px(250.), px(0.));
        window.pointer_down(start, MouseButton::Left, cx);
        window.pointer_move(start - point(px(15.), px(0.)), Some(MouseButton::Left), cx);
        window.pointer_move(start - point(px(800.), px(0.)), Some(MouseButton::Left), cx);
        assert_eq!(
            window.find("onboarding-page-1").bounds().size.width,
            bounds.size.width
        );
        window.pointer_up(start - point(px(800.), px(0.)), MouseButton::Left, cx);
    })
    .unwrap();
    settle(cx, &opened);
    assert!(shows(cx, &opened, wizard_ids::BACK));
    click(cx, &opened, wizard_ids::BACK);
    assert!(!shows(cx, &opened, wizard_ids::BACK));
    assert!(opened.fake.calls.lock().unwrap().writes.is_empty());
}

#[gpui_kit::test]
fn stage_fills_minimum_window_and_enter_advances_in_light_reduced_motion(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    cx.update(|cx| kairo_ui_kit::set_reduced_motion(true, cx));
    cx.simulate_window_resize(opened.handle, size(px(960.), px(600.)));
    cx.update_window(opened.handle, |_, window, cx| {
        kairo_ui_kit::set_appearance(kairo_ui_kit::Appearance::Light, Some(window), cx);
        window.render_frame(cx);
        let stage = window.find(wizard_ids::DIALOG).bounds();
        assert_eq!(stage.size, size(px(960.), px(600.)));
        assert!(window.find(wizard_ids::CONTINUE).visible());
        window.press("enter", cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert!(shows(cx, &opened, wizard_ids::BACK));
    assert!(opened.fake.calls.lock().unwrap().writes.is_empty());
}

#[gpui_kit::test]
fn reduced_motion_drag_keeps_the_selected_page_visible(cx: &mut TestAppContext) {
    use gpui_kit::test::TestEventExt;
    use gpui_kit::{MouseButton, point};
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    cx.update(|cx| kairo_ui_kit::set_reduced_motion(true, cx));
    draw(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        let start = window.find("onboarding-pager").bounds().center();
        window.pointer_down(start, MouseButton::Left, cx);
        window.pointer_move(start - point(px(15.), px(0.)), Some(MouseButton::Left), cx);
        window.pointer_move(
            start - point(px(1600.), px(0.)),
            Some(MouseButton::Left),
            cx,
        );
        assert!(window.find("onboarding-page-0").visible());
        window.pointer_up(start - point(px(1600.), px(0.)), MouseButton::Left, cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert!(shows(cx, &opened, wizard_ids::BACK));
}

#[gpui_kit::test]
fn gate_create_account_uses_the_shared_heading_at_minimum_size(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), ui_account::model::signed_out());
    cx.update(|cx| kairo_ui_kit::set_reduced_motion(true, cx));
    cx.simulate_window_resize(opened.handle, size(px(960.), px(600.)));
    draw(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        window.scroll(
            "gate-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-600.))),
            cx,
        );
    })
    .unwrap();
    click(cx, &opened, ui_account::auth::ids::SWITCH);
    cx.update_window(opened.handle, |_, window, cx| {
        window.scroll(
            "gate-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(600.))),
            cx,
        );
        let title = window.find("account-gate-title");
        assert_eq!(title.role(), Some(gpui_kit::Role::Heading));
        assert_eq!(title.label(), Some("Create your account"));
        assert!(window.find(ui_account::auth::ids::NAME).visible());
    })
    .unwrap();
}

/// CPU-only baseline: run alone with --release --ignored --nocapture.
/// Headless paint traversal is included; GPU submission/display latency is not.
#[gpui_kit::test]
#[ignore = "release CPU render baseline; run explicitly"]
fn onboarding_render_baseline(cx: &mut TestAppContext) {
    use std::time::Instant;
    for (name, state) in [
        ("tour", OnboardingState::default()),
        (
            "output",
            OnboardingState {
                current_step: kairo_domain::settings::OnboardingStepId::Output,
                ..Default::default()
            },
        ),
        ("gate", finished()),
    ] {
        let opened = open(cx, state, ui_account::model::signed_out());
        if name == "output" {
            opened.account.update(cx, |a, cx| {
                a.set_session(session(true), cx);
                a.set_license(
                    LicenseSnapshot {
                        license: None,
                        activated: true,
                        lease: None,
                        providers: Default::default(),
                    },
                    cx,
                );
                a.set_settings(
                    kairo_domain::settings::AppSettings::defaults(
                        kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone(),
                    ),
                    cx,
                );
            });
        }
        settle(cx, &opened);
        ALLOCATIONS.with(|n| n.set(0));
        COUNT_ALLOCATIONS.with(|enabled| enabled.set(true));
        let start = Instant::now();
        for _ in 0..100 {
            cx.update_window(opened.handle, |_, window, cx| {
                window.refresh();
                window.render_frame(cx);
            })
            .unwrap();
        }
        COUNT_ALLOCATIONS.with(|enabled| enabled.set(false));
        eprintln!(
            "{name}: allocations/frame {:.0}",
            ALLOCATIONS.with(|n| n.get()) as f64 / 100.
        );
        eprintln!(
            "{name}: CPU render mean {:.3} ms (100 frames, headless; GPU unmeasured)",
            start.elapsed().as_secs_f64() * 10.
        );
    }
}

#[gpui_kit::test]
fn password_reset_rejects_repeated_activation_while_pending(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), ui_account::model::signed_out());
    settle(cx, &opened);
    type_into(cx, &opened, ui_account::auth::ids::EMAIL, "op@church.test");
    *opened.fake.reset_reply.lock().unwrap() = Some(Box::pin(std::future::pending()));
    click(cx, &opened, ui_account::auth::ids::FORGOT);
    click(cx, &opened, ui_account::auth::ids::FORGOT);
    assert_eq!(opened.fake.calls.lock().unwrap().resets, 1);
}

fn held<T: Send + 'static>() -> (Reply<T>, impl FnOnce(T)) {
    struct State<T> {
        value: Option<T>,
        waker: Option<std::task::Waker>,
    }
    let state = Arc::new(Mutex::new(State {
        value: None,
        waker: None,
    }));
    let reader = state.clone();
    let reply = Box::pin(std::future::poll_fn(move |cx| {
        let mut state = reader.lock().unwrap();
        if let Some(value) = state.value.take() {
            std::task::Poll::Ready(value)
        } else {
            state.waker = Some(cx.waker().clone());
            std::task::Poll::Pending
        }
    }));
    (reply, move |value| {
        let waker = {
            let mut state = state.lock().unwrap();
            state.value = Some(value);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    })
}
fn activate(cx: &mut TestAppContext, opened: &Opened) {
    opened.account.update(cx, |a, cx| {
        a.set_session(session(true), cx);
        a.set_license(
            LicenseSnapshot {
                license: None,
                activated: true,
                lease: None,
                providers: Default::default(),
            },
            cx,
        );
    });
    draw(cx, opened);
}
#[gpui_kit::test]
fn account_ready_during_tour_write_reaches_output_once(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    settle(cx, &opened);
    let (reply, release) = held();
    *opened.fake.write_reply.lock().unwrap() = Some(reply);
    click(cx, &opened, wizard_ids::SKIP_TOUR);
    click(cx, &opened, wizard_ids::SKIP_TOUR);
    assert_eq!(opened.fake.calls.lock().unwrap().writes.len(), 1);
    activate(cx, &opened);
    release(Ok(kairo_domain::cloud::complete_step(
        OnboardingState::default(),
        kairo_domain::settings::OnboardingStepId::Welcome,
    )));
    draw(cx, &opened);
    assert_eq!(
        opened
            .account
            .read_with(cx, |a, _| a.onboarding().current_step),
        kairo_domain::settings::OnboardingStepId::Output
    );
    assert_eq!(
        opened
            .fake
            .calls
            .lock()
            .unwrap()
            .writes
            .iter()
            .filter(|w| **w
                == OnboardingWrite::SetCurrent(kairo_domain::settings::OnboardingStepId::Output))
            .count(),
        1
    );
}
#[gpui_kit::test]
fn stale_sign_out_does_not_replace_another_session(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), session(false));
    let (reply, release) = held();
    *opened.fake.sign_out_reply.lock().unwrap() = Some(reply);
    click(cx, &opened, ui_account::gate::ids::DIFFERENT_EMAIL);
    let mut replacement = session(false);
    replacement.user.as_mut().unwrap().id = "replacement".into();
    opened
        .account
        .update(cx, |a, cx| a.set_session(replacement.clone(), cx));
    draw(cx, &opened);
    release(Ok(ui_account::model::signed_out()));
    draw(cx, &opened);
    assert_eq!(
        opened.account.read_with(cx, |a, _| a.session().clone()),
        replacement
    );
}

#[gpui_kit::test]
fn late_write_failure_animates_then_remains_retryable(cx: &mut TestAppContext) {
    let output = OnboardingState {
        current_step: kairo_domain::settings::OnboardingStepId::Output,
        ..Default::default()
    };
    let opened = open(cx, output, session(true));
    activate(cx, &opened);
    settle(cx, &opened);
    let (reply, release) = held();
    *opened.fake.write_reply.lock().unwrap() = Some(reply);
    click(cx, &opened, wizard_ids::CONTINUE);
    advance(cx, &opened, 5000);
    release(Err("write failed".into()));
    draw(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        assert_eq!(
            window.find(wizard_ids::ERROR).role(),
            Some(gpui_kit::Role::Alert)
        );
        assert!(
            window.simulate_next_frame(cx) > 0,
            "late error schedules frames independently of page entry"
        );
    })
    .unwrap();
    advance(cx, &opened, 400);
    let before = opened.fake.calls.lock().unwrap().writes.len();
    click(cx, &opened, wizard_ids::CONTINUE);
    assert!(opened.fake.calls.lock().unwrap().writes.len() > before);
    assert!(!shows(cx, &opened, wizard_ids::ERROR));
}

#[gpui_kit::test]
fn resetting_progress_discards_a_pending_finish_result(cx: &mut TestAppContext) {
    let output = OnboardingState {
        current_step: kairo_domain::settings::OnboardingStepId::Output,
        ..Default::default()
    };
    let opened = open(cx, output, session(true));
    activate(cx, &opened);
    settle(cx, &opened);
    let (reply, release) = held();
    *opened.fake.write_reply.lock().unwrap() = Some(reply);
    click(cx, &opened, wizard_ids::LATER);
    opened
        .account
        .update(cx, |a, cx| a.set_onboarding(OnboardingState::default(), cx));
    draw(cx, &opened);
    release(Ok(finished()));
    draw(cx, &opened);
    assert_eq!(
        opened.account.read_with(cx, |a, _| a.onboarding().clone()),
        OnboardingState::default()
    );
    assert!(!shows(cx, &opened, wizard_ids::START));
}

#[gpui_kit::test]
fn idle_tour_has_no_frame_loop_and_offscreen_story_timer_stops(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    advance(cx, &opened, 4000);
    cx.update_window(opened.handle, |_, window, cx| {
        assert_eq!(
            window.simulate_next_frame(cx),
            0,
            "settled first slide is idle"
        );
        window.click("onboarding-dot-1", cx);
    })
    .unwrap();
    advance(cx, &opened, 4000);
    cx.update_window(opened.handle, |_, window, cx| {
        assert_eq!(
            window.simulate_next_frame(cx),
            0,
            "caret uses a timer, not a frame loop"
        );
        window.click("onboarding-dot-0", cx);
    })
    .unwrap();
    advance(cx, &opened, 4000);
    cx.update_window(opened.handle, |_, window, cx| {
        assert_eq!(window.simulate_next_frame(cx), 0)
    })
    .unwrap();
}

#[gpui_kit::test]
fn signup_draft_survives_back_to_tour(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState::default(),
        ui_account::model::signed_out(),
    );
    settle(cx, &opened);
    click(cx, &opened, wizard_ids::SKIP_TOUR);
    type_into(cx, &opened, ui_account::auth::ids::NAME, "Draft operator");
    type_into(
        cx,
        &opened,
        ui_account::auth::ids::EMAIL,
        "draft@church.test",
    );
    click(cx, &opened, wizard_ids::BACK);
    click(cx, &opened, wizard_ids::SKIP_TOUR);
    cx.update_window(opened.handle, |_, window, _| {
        assert_eq!(
            window.find(ui_account::auth::ids::NAME).value(),
            Some("Draft operator")
        );
        assert_eq!(
            window.find(ui_account::auth::ids::EMAIL).value(),
            Some("draft@church.test")
        );
    })
    .unwrap();
}
#[gpui_kit::test]
fn delayed_display_save_cannot_overwrite_a_replaced_session(cx: &mut TestAppContext) {
    use kairo_domain::overlay::{DisplayBounds, DisplayInfo, Size};
    let opened = open(
        cx,
        OnboardingState {
            current_step: kairo_domain::settings::OnboardingStepId::Output,
            ..Default::default()
        },
        session(true),
    );
    opened.account.update(cx, |a, cx| {
        a.set_settings(
            kairo_domain::settings::AppSettings::defaults(
                kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone(),
            ),
            cx,
        );
        a.set_displays(
            vec![DisplayInfo {
                id: 7.,
                label: "Projector".into(),
                bounds: DisplayBounds {
                    x: 0.,
                    y: 0.,
                    width: 1920.,
                    height: 1080.,
                },
                size: Size {
                    width: 1920.,
                    height: 1080.,
                    extra: Default::default(),
                },
                scale_factor: 1.,
                primary: false,
                hosts_main_window: false,
            }],
            cx,
        );
    });
    activate(cx, &opened);
    settle(cx, &opened);
    let (reply, release) = held();
    *opened.fake.overlay_reply.lock().unwrap() = Some(reply);
    cx.update_window(opened.handle, |_, window, cx| {
        window.click("ob-audience", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert_eq!(
        opened.fake.calls.lock().unwrap().overlays,
        1,
        "picker sends a save"
    );
    opened.account.update(cx, |a, cx| {
        let mut replacement = session(false);
        replacement.user.as_mut().unwrap().id = "replacement".into();
        a.set_session(replacement, cx);
        a.update_settings(
            |s| s.overlay.outputs[0].name = "Replacement settings".into(),
            cx,
        );
    });
    draw(cx, &opened);
    release(Ok(()));
    draw(cx, &opened);
    assert_eq!(
        opened
            .account
            .read_with(cx, |a, _| a.settings().unwrap().overlay.outputs[0]
                .name
                .clone()),
        "Replacement settings"
    );
}

#[gpui_kit::test]
fn signup_is_scrollable_at_minimum_size_with_larger_interface_text(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), ui_account::model::signed_out());
    cx.update(|cx| kairo_ui_kit::set_reduced_motion(true, cx));
    cx.simulate_window_resize(opened.handle, size(px(960.), px(600.)));
    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        let initial = window.find("account-gate-title").bounds().size.height;
        gpui_kit::component::Theme::update(cx, |theme| theme.font_size = px(20.));
        window.refresh();
        window.render_frame(cx);
        assert!(
            window.find("account-gate-title").bounds().size.height > initial * 1.2,
            "interface heading scales with rem size"
        );
        window.scroll(
            "gate-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2000.))),
            cx,
        );
        window.click(ui_account::auth::ids::SWITCH, cx);
    })
    .unwrap();
    draw(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        window.scroll(
            "gate-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(2000.))),
            cx,
        );
        assert!(window.find(ui_account::auth::ids::NAME).visible());
        window.scroll(
            "gate-body",
            gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(-2000.))),
            cx,
        );
        assert!(window.find(ui_account::auth::ids::SUBMIT).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn failed_automatic_account_progression_has_an_explicit_retry(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState {
            current_step: kairo_domain::settings::OnboardingStepId::Account,
            ..Default::default()
        },
        session(true),
    );
    let (reply, release) = held();
    *opened.fake.write_reply.lock().unwrap() = Some(reply);
    activate(cx, &opened);
    release(Err("temporary write failure".into()));
    draw(cx, &opened);
    assert!(shows(cx, &opened, wizard_ids::ERROR));
    assert!(
        shows(cx, &opened, wizard_ids::CONTINUE),
        "account write failure offers retry"
    );
    click(cx, &opened, wizard_ids::CONTINUE);
    assert_eq!(
        opened
            .account
            .read_with(cx, |a, _| a.onboarding().current_step),
        kairo_domain::settings::OnboardingStepId::Output
    );
}

#[gpui_kit::test]
fn failure_from_before_reset_does_not_show_on_new_setup(cx: &mut TestAppContext) {
    let opened = open(
        cx,
        OnboardingState {
            current_step: kairo_domain::settings::OnboardingStepId::Output,
            ..Default::default()
        },
        session(true),
    );
    activate(cx, &opened);
    settle(cx, &opened);
    let (reply, release) = held();
    *opened.fake.write_reply.lock().unwrap() = Some(reply);
    click(cx, &opened, wizard_ids::LATER);
    opened
        .account
        .update(cx, |a, cx| a.set_onboarding(OnboardingState::default(), cx));
    draw(cx, &opened);
    release(Err("old request failed".into()));
    draw(cx, &opened);
    assert!(!shows(cx, &opened, wizard_ids::ERROR));
}

#[gpui_kit::test]
fn password_reset_is_reachable_and_activates_from_the_keyboard(cx: &mut TestAppContext) {
    let opened = open(cx, finished(), ui_account::model::signed_out());
    settle(cx, &opened);
    cx.update_window(opened.handle, |_, window, cx| {
        for _ in 0..10 {
            if window.find(ui_account::auth::ids::EMAIL).focused() == Some(true) {
                break;
            }
            window.press("tab", cx);
        }
        assert_eq!(
            window.find(ui_account::auth::ids::EMAIL).focused(),
            Some(true)
        );
        window.input("op@church.test", cx);
        for _ in 0..10 {
            window.press("tab", cx);
            if window.find(ui_account::auth::ids::FORGOT).focused() == Some(true) {
                break;
            }
        }
        assert_eq!(
            window.find(ui_account::auth::ids::FORGOT).focused(),
            Some(true)
        );
        window.press("enter", cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert_eq!(opened.fake.calls.lock().unwrap().resets, 1);
}

#[gpui_kit::test]
fn finished_setup_stays_closed_when_progress_loads_after_launch(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let fake = Arc::new(Fake {
        calls: Mutex::default(),
        session: session(true),
        reset_reply: Mutex::default(),
        write_reply: Mutex::default(),
        sign_out_reply: Mutex::default(),
        state: Mutex::new(finished()),
        overlay_reply: Mutex::default(),
    });
    let backend: Arc<dyn AccountBackend> = fake;
    // As at launch: the window opens before the saved progress arrives.
    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(backend, "Africa/Lagos");
            account.set_session(session(true), cx);
            account
        })
    });
    let model = account.clone();
    let (_, flow) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| AccountFlow::new(model, window, cx))
        })
        .expect("open")
    });
    cx.run_until_parked();
    assert!(!flow.read_with(cx, |f, _| f.wizard_open()));
    cx.update(|cx| account.update(cx, |a, cx| a.set_onboarding(finished(), cx)));
    cx.run_until_parked();
    assert!(!flow.read_with(cx, |f, _| f.wizard_open()));
}

#[gpui_kit::test]
fn gate_waits_for_initial_session_before_opening(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let fake = Arc::new(Fake {
        calls: Mutex::default(),
        session: session(true),
        reset_reply: Mutex::default(),
        write_reply: Mutex::default(),
        sign_out_reply: Mutex::default(),
        state: Mutex::new(finished()),
        overlay_reply: Mutex::default(),
    });
    let backend: Arc<dyn AccountBackend> = fake;
    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(backend, "Africa/Lagos");
            account.set_onboarding(finished(), cx);
            account
        })
    });
    let model = account.clone();
    let (_, flow) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| AccountFlow::new(model, window, cx))
        })
        .expect("open")
    });
    cx.run_until_parked();
    assert!(!flow.read_with(cx, |f, _| f.gate_open()));
    cx.update(|cx| account.update(cx, |a, cx| a.set_session(ui_account::model::signed_out(), cx)));
    cx.run_until_parked();
    assert!(flow.read_with(cx, |f, _| f.gate_open()));
}
