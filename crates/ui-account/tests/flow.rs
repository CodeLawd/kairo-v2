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

#[derive(Default)]
struct Calls {
    sign_in: Vec<SignInInput>,
    resend: usize,
    writes: Vec<OnboardingWrite>,
}

struct Fake {
    calls: Mutex<Calls>,
    /// What sign-in answers with.
    session: SessionSnapshot,
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
        ready(Ok(ui_account::model::signed_out()))
    }
    fn request_password_reset(&self, _: String) -> Reply<Result<(), String>> {
        ready(Ok(()))
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
        ready(Ok(OnboardingState::default()))
    }
    fn save_church(&self, _: ChurchProfile) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
    fn save_overlay(&self, _: OverlaySettings) -> Reply<Result<(), String>> {
        ready(Ok(()))
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
    cx.update_window(opened.handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// Waits out an opening animation (real time), then draws.
fn settle(cx: &mut TestAppContext, opened: &Opened) {
    std::thread::sleep(std::time::Duration::from_millis(600));
    draw(cx, opened);
}

fn shows(cx: &mut TestAppContext, opened: &Opened, id: &'static str) -> bool {
    cx.update_window(opened.handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: &'static str) {
    cx.update_window(opened.handle, |_, window, cx| window.click(id, cx))
        .unwrap();
    // Slide changes animate on real time; let them settle.
    std::thread::sleep(std::time::Duration::from_millis(700));
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
