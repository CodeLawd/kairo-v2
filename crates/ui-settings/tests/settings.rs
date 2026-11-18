//! UI integration tests for the Settings window: navigation, each pane's
//! saves and failure paths, the meter's lifetime and shortcut conflicts.

use std::sync::{Arc, Mutex};

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px,
    size,
};
use kairo_domain::cloud::{
    CloudResult, CloudSessionState, CloudUser, DevicePairingState, LicenseSnapshot,
    SessionSnapshot, SignInInput, SignUpInput,
};
use kairo_domain::documents::{SlideConverterState, SlideConverterStatus};
use kairo_domain::program::PresentationSettings;
use kairo_domain::scripture::{
    InstalledLocalBiblePack, LocalBiblePackStatus, ScriptureTranslationOption, TranslationAccess,
};
use kairo_domain::scripture_trace::ScriptureTraceRecord;
use kairo_domain::settings::{
    AppSettings, ChurchProfile, OnboardingState, OverlaySettings, ShortcutCommand,
};
use kairo_domain::status::{AudioDevice, AudioDeviceKind, AudioLevel, UpdateStatus};
use kairo_domain::workspace::{MediaFolderMigration, WorkspaceInfo, WorkspaceSongSync};
use serde_json::{Value, json};
use ui_account::{Account, AccountBackend, OnboardingWrite};
use ui_settings::panes::ids as pane_ids;
use ui_settings::{
    BackendEvent, InputDevice, MeterEvent, MeterHandle, Reply, Section, SettingsBackend,
    SettingsEvent, SettingsWindow, UpdateAction, ids,
};

fn ready<T: Send + 'static>(value: T) -> Reply<T> {
    Box::pin(async move { value })
}

fn defaults() -> AppSettings {
    AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone())
}

// ── Fakes ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct Calls {
    sets: Vec<(&'static str, Value)>,
    meters: Vec<String>,
    tests: usize,
    updates: Vec<UpdateAction>,
    downloads: Vec<String>,
    resets: usize,
    resyncs: usize,
}

struct Fake {
    calls: Mutex<Calls>,
    settings: Mutex<Result<AppSettings, String>>,
    inputs: Mutex<Vec<InputDevice>>,
    meter_error: Mutex<Option<String>>,
    meter_tx: Mutex<Option<UnboundedSender<MeterEvent>>>,
    meter_alive: Arc<Mutex<usize>>,
    send_test: Mutex<Result<bool, String>>,
    set_error: Mutex<Option<String>>,
    workspace: Mutex<(WorkspaceInfo, MediaFolderMigration)>,
    download: Mutex<Result<LocalBiblePackStatus, String>>,
    events: Mutex<Vec<UnboundedSender<BackendEvent>>>,
}

/// Counts live meters: dropping the handle stops capture.
struct Alive(Arc<Mutex<usize>>);
impl Drop for Alive {
    fn drop(&mut self) {
        *self.0.lock().unwrap() -= 1;
    }
}

fn device(id: &str, label: &str, default: bool) -> InputDevice {
    InputDevice {
        device: AudioDevice {
            id: id.into(),
            label: label.into(),
            kind: AudioDeviceKind::AudioInput,
            is_default: default,
        },
        kind: "External".into(),
    }
}

fn pack_status(id: &str, installed: bool) -> LocalBiblePackStatus {
    LocalBiblePackStatus {
        translation: id.into(),
        verse_count: if installed { 31_102.0 } else { 0.0 },
        chapter_count: if installed { 1_189.0 } else { 0.0 },
        installed,
        installation: None,
    }
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            calls: Mutex::default(),
            settings: Mutex::new(Ok(defaults())),
            inputs: Mutex::new(vec![
                device("default", "System Default", true),
                device("usb", "USB Mic", false),
            ]),
            meter_error: Mutex::new(None),
            meter_tx: Mutex::new(None),
            meter_alive: Arc::default(),
            send_test: Mutex::new(Ok(true)),
            set_error: Mutex::new(None),
            workspace: Mutex::new((
                WorkspaceInfo {
                    root: "/Users/op/Kairo".into(),
                    songs_dir: "/Users/op/Kairo/Songs".into(),
                    media_dir: "/Users/op/Kairo/Media".into(),
                    is_default: true,
                    ready: true,
                    error: None,
                },
                MediaFolderMigration {
                    current_folder: "/Volumes/Old/Backgrounds".into(),
                    target_folder: "/Users/op/Kairo/Media".into(),
                    needed: true,
                    file_count: 3.0,
                },
            )),
            download: Mutex::new(Ok(pack_status("NKJV", true))),
            events: Mutex::default(),
        }
    }
}

impl SettingsBackend for Fake {
    fn settings(&self) -> Reply<Result<AppSettings, String>> {
        ready(self.settings.lock().unwrap().clone())
    }
    fn get_section(&self, key: &'static str) -> Reply<Result<Value, String>> {
        let value = match key {
            "presentation" => json!({ "audio": { "outputLabel": "", "volume": 0.5 }, "props": [] }),
            _ => Value::Null,
        };
        ready(Ok(value))
    }
    fn set_section(&self, key: &'static str, value: Value) -> Reply<Result<(), String>> {
        self.calls.lock().unwrap().sets.push((key, value));
        ready(match self.set_error.lock().unwrap().clone() {
            Some(error) => Err(error),
            None => Ok(()),
        })
    }
    fn input_devices(&self) -> Reply<Vec<InputDevice>> {
        ready(self.inputs.lock().unwrap().clone())
    }
    fn output_devices(&self) -> Reply<Vec<String>> {
        ready(vec!["Speakers".into()])
    }
    fn start_meter(
        &self,
        device_id: String,
    ) -> Reply<Result<(MeterHandle, UnboundedReceiver<MeterEvent>), String>> {
        self.calls.lock().unwrap().meters.push(device_id);
        if let Some(error) = self.meter_error.lock().unwrap().clone() {
            return ready(Err(error));
        }
        let (tx, rx) = unbounded();
        *self.meter_tx.lock().unwrap() = Some(tx);
        *self.meter_alive.lock().unwrap() += 1;
        let alive = Alive(self.meter_alive.clone());
        ready(Ok((MeterHandle(Box::new(alive)), rx)))
    }
    fn send_test(&self) -> Reply<Result<bool, String>> {
        self.calls.lock().unwrap().tests += 1;
        ready(self.send_test.lock().unwrap().clone())
    }
    fn clear_all(&self) -> Reply<Result<bool, String>> {
        ready(Ok(true))
    }
    fn update_status(&self) -> UpdateStatus {
        UpdateStatus::Idle {
            current_version: Some("1.2.0".into()),
        }
    }
    fn update(&self, action: UpdateAction) {
        self.calls.lock().unwrap().updates.push(action);
    }
    fn workspace(&self) -> Reply<Result<(WorkspaceInfo, MediaFolderMigration), String>> {
        ready(Ok(self.workspace.lock().unwrap().clone()))
    }
    fn choose_workspace(&self, _: bool) -> Reply<Result<Option<WorkspaceInfo>, String>> {
        ready(Ok(None))
    }
    fn resync_songs(&self) -> Reply<Result<WorkspaceSongSync, String>> {
        self.calls.lock().unwrap().resyncs += 1;
        ready(Ok(WorkspaceSongSync {
            imported: 12.0,
            removed: 1.0,
            exported: 0.0,
        }))
    }
    fn adopt_media(&self, _: bool) -> Reply<Result<(), String>> {
        ready(Err("The disk is full.".into()))
    }
    fn reveal_workspace(&self, _: bool) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
    fn converter_status(&self) -> Reply<SlideConverterStatus> {
        ready(SlideConverterStatus {
            state: SlideConverterState::Missing,
            version: "24.8".into(),
            size_bytes: 150.0 * 1_048_576.0,
            received_bytes: 0.0,
            error: None,
            other_converter: None,
        })
    }
    fn converter_install(&self) {}
    fn converter_cancel(&self) {}
    fn converter_remove(&self) {}
    fn translations(&self) -> Reply<Result<Vec<ScriptureTranslationOption>, String>> {
        ready(Ok(vec![
            ScriptureTranslationOption {
                id: "KJV".into(),
                name: "King James Version".into(),
                access: TranslationAccess::Local,
                available: true,
                requires_api_key: false,
                downloadable: None,
                download_approx: None,
            },
            ScriptureTranslationOption {
                id: "NKJV".into(),
                name: "New King James Version".into(),
                access: TranslationAccess::Local,
                available: false,
                requires_api_key: false,
                downloadable: Some(true),
                download_approx: Some("about 4 MB".into()),
            },
        ]))
    }
    fn installed_packs(&self) -> Reply<Result<Vec<InstalledLocalBiblePack>, String>> {
        ready(Ok(Vec::new()))
    }
    fn pack_status(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        ready(Ok(pack_status(&translation, false)))
    }
    fn download_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        self.calls.lock().unwrap().downloads.push(translation);
        ready(self.download.lock().unwrap().clone())
    }
    fn remove_pack(&self, translation: String) -> Reply<Result<LocalBiblePackStatus, String>> {
        ready(Ok(pack_status(&translation, false)))
    }
    fn recent_traces(&self, _: usize) -> Reply<Result<Vec<ScriptureTraceRecord>, String>> {
        ready(Err("trace store unavailable".into()))
    }
    fn reset_onboarding(&self) -> Reply<Result<(), String>> {
        self.calls.lock().unwrap().resets += 1;
        ready(Ok(()))
    }
    fn events(&self) -> UnboundedReceiver<BackendEvent> {
        let (tx, rx) = unbounded();
        self.events.lock().unwrap().push(tx);
        rx
    }
}

struct FakeAccount;

impl AccountBackend for FakeAccount {
    fn sign_in(&self, _: SignInInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        ready(Err("not used".into()))
    }
    fn sign_up(&self, _: SignUpInput) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        ready(Err("not used".into()))
    }
    fn sign_out(&self) -> Reply<Result<SessionSnapshot, String>> {
        ready(Ok(ui_account::model::signed_out()))
    }
    fn request_password_reset(&self, _: String) -> Reply<Result<(), String>> {
        ready(Ok(()))
    }
    fn resend_verification(&self) -> Reply<Result<CloudResult<()>, String>> {
        ready(Err("not used".into()))
    }
    fn verify_email_code(&self, _: String) -> Reply<Result<CloudResult<SessionSnapshot>, String>> {
        ready(Err("not used".into()))
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
    fn onboarding(&self, _: OnboardingWrite) -> Reply<Result<OnboardingState, String>> {
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

fn signed_in() -> SessionSnapshot {
    SessionSnapshot {
        state: CloudSessionState::Active,
        user: Some(CloudUser {
            id: "u1".into(),
            email: "op@church.test".into(),
            name: "Ada Byron".into(),
            email_verified: true,
        }),
        ..ui_account::model::signed_out()
    }
}

// ── Harness ─────────────────────────────────────────────────────────────

struct Opened {
    handle: AnyWindowHandle,
    view: Entity<SettingsWindow>,
    account: Entity<Account>,
    fake: Arc<Fake>,
    events: Arc<Mutex<Vec<SettingsEvent>>>,
}

fn open_with(
    cx: &mut TestAppContext,
    fake: Fake,
    section: Option<Section>,
    session: SessionSnapshot,
) -> Opened {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(kairo_ui_kit::Appearance::Dark, cx);
    });
    let fake = Arc::new(fake);
    let backend: Arc<dyn SettingsBackend> = fake.clone();
    let account = cx.update(|cx| {
        cx.new(|cx| {
            let mut account = Account::new(Arc::new(FakeAccount), "UTC");
            account.set_session(session, cx);
            account
        })
    });
    let model = account.clone();
    let (handle, view) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(1280.), px(900.)),
                ))),
                ..Default::default()
            },
            cx,
            move |window, cx| {
                cx.new(|cx| {
                    SettingsWindow::new(backend, model, section, (760.0, 820.0), window, cx)
                })
            },
        )
        .expect("open")
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let seen = events.clone();
    cx.update(|cx| {
        cx.subscribe(&view, move |_, event: &SettingsEvent, _| {
            seen.lock().unwrap().push(*event)
        })
        .detach()
    });
    let opened = Opened {
        handle,
        view,
        account,
        fake,
        events,
    };
    draw(cx, &opened);
    opened
}

fn open(cx: &mut TestAppContext, section: Option<Section>) -> Opened {
    open_with(cx, Fake::default(), section, signed_in())
}

#[gpui_kit::test]
fn account_sync_age_invalidates_the_retained_settings_pane(cx: &mut TestAppContext) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    let mut session = signed_in();
    session.last_synced_at = Some(1.);
    let opened = open_with(cx, Fake::default(), Some(Section::Account), session);
    let notifications = Arc::new(AtomicUsize::new(0));
    let seen = notifications.clone();
    let _observer = cx.update(|cx| {
        cx.observe(&opened.view, move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
    });
    cx.executor().advance_clock(Duration::from_secs(61));
    cx.run_until_parked();
    assert!(
        notifications.load(Ordering::SeqCst) > 0,
        "Last synced must age without a service push or input"
    );
}

fn draw(cx: &mut TestAppContext, opened: &Opened) {
    cx.run_until_parked();
    cx.update_window(opened.handle, |_, window, cx| {
        window.render_frame(cx);
        kairo_ui_kit::a11y_audit::check(window);
    })
    .unwrap();
    cx.run_until_parked();
}

fn shows(cx: &mut TestAppContext, opened: &Opened, id: impl Into<gpui_kit::ElementId>) -> bool {
    let id = id.into();
    cx.update_window(opened.handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

/// Scrolls the pane until `id` sits wholly inside it.
fn reveal(cx: &mut TestAppContext, opened: &Opened, id: &gpui_kit::ElementId) {
    for _ in 0..60 {
        let step = cx
            .update_window(opened.handle, |_, window, _| {
                let pane = window.find("settings-pane").bounds();
                let Some(el) = window.try_find(id.clone()) else {
                    return 0.0;
                };
                let b = el.bounds();
                if b.top() < pane.top() {
                    120.0
                } else if b.bottom() > pane.bottom() {
                    -120.0
                } else {
                    0.0
                }
            })
            .unwrap();
        if step == 0.0 {
            return;
        }
        cx.update_window(opened.handle, |_, window, cx| {
            window.scroll(
                "settings-pane",
                gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(step))),
                cx,
            )
        })
        .unwrap();
        draw(cx, opened);
    }
}

fn click(cx: &mut TestAppContext, opened: &Opened, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    reveal(cx, opened, &id);
    cx.update_window(opened.handle, |_, window, cx| window.click(id, cx))
        .unwrap();
    draw(cx, opened);
}

fn press(cx: &mut TestAppContext, opened: &Opened, key: &str) {
    cx.update_window(opened.handle, |_, window, cx| window.press(key, cx))
        .unwrap();
    draw(cx, opened);
}

fn sets(opened: &Opened, key: &str) -> Vec<Value> {
    opened
        .fake
        .calls
        .lock()
        .unwrap()
        .sets
        .iter()
        .filter(|(k, _)| *k == key)
        .map(|(_, v)| v.clone())
        .collect()
}

// ── Tests ───────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn opens_at_the_asked_pane_and_moves_back_and_forward(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Audio));
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.section()),
        Section::Audio
    );
    // Account sits in the card, not the list, until searched for.
    assert!(!shows(cx, &opened, ids::nav("account")));
    assert!(shows(cx, &opened, ids::ACCOUNT_CARD));

    click(cx, &opened, ids::nav("scripture"));
    click(cx, &opened, ids::ACCOUNT_CARD);
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.section()),
        Section::Account
    );
    click(cx, &opened, ids::BACK);
    click(cx, &opened, ids::BACK);
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.section()),
        Section::Audio
    );
    click(cx, &opened, ids::FORWARD);
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.section()),
        Section::Scripture
    );
    // Picking a new pane drops the forward history.
    click(cx, &opened, ids::nav("general"));
    let can_forward = cx
        .update_window(opened.handle, |_, window, _| {
            window.find(ids::FORWARD).disabled()
        })
        .unwrap();
    assert_ne!(can_forward, Some(false));
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.section()),
        Section::General
    );
}

#[gpui_kit::test]
fn search_filters_the_sidebar_and_shows_no_matches(cx: &mut TestAppContext) {
    let opened = open(cx, None);
    cx.update_window(opened.handle, |_, window, cx| {
        window.click(ids::SEARCH, cx);
        window.input("team sync", cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert!(shows(cx, &opened, ids::nav("account")));
    assert!(!shows(cx, &opened, ids::nav("general")));
    cx.update_window(opened.handle, |_, window, cx| window.input("zzz", cx))
        .unwrap();
    draw(cx, &opened);
    assert!(shows(cx, &opened, ids::NO_MATCHES));
}

#[gpui_kit::test]
fn a_failed_read_shows_defaults_with_a_notice(cx: &mut TestAppContext) {
    let fake = Fake {
        settings: Mutex::new(Err("settings.json is unreadable".into())),
        ..Fake::default()
    };
    let opened = open_with(cx, fake, None, signed_in());
    assert!(!shows(cx, &opened, ids::LOADING));
    assert!(shows(cx, &opened, "settings-load-error"));
}

#[gpui_kit::test]
fn general_saves_display_and_lyrics_and_live_controls_save_at_once(cx: &mut TestAppContext) {
    let opened = open(cx, None);
    // Dark mode and privacy apply the moment they change.
    click(cx, &opened, pane_ids::general::DARK);
    assert_eq!(sets(&opened, "display")[0]["theme"], "light");
    click(cx, &opened, pane_ids::general::SHARE);
    assert_eq!(sets(&opened, "usage")[0]["shareStats"], false);

    // A gloss colour is saved only once it is a valid hex.
    reveal(cx, &opened, &pane_ids::general::GLOSS.into());
    cx.update_window(opened.handle, |_, window, cx| {
        window.click(pane_ids::general::GLOSS, cx);
        window.press("secondary-a", cx);
        window.input("#12", cx);
    })
    .unwrap();
    draw(cx, &opened);
    assert!(sets(&opened, "lyrics").is_empty());
    cx.update_window(opened.handle, |_, window, cx| window.input("3456", cx))
        .unwrap();
    draw(cx, &opened);
    assert_eq!(sets(&opened, "lyrics")[0]["glossColor"], "#123456");

    click(cx, &opened, ids::save("general"));
    assert_eq!(sets(&opened, "display").len(), 2);
    assert!(shows(cx, &opened, "settings-saved-general"));
}

#[gpui_kit::test]
fn a_failed_save_says_so_and_does_not_flash_saved(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Scripture));
    *opened.fake.set_error.lock().unwrap() = Some("disk full".into());
    click(cx, &opened, ids::save("scripture"));
    assert!(shows(cx, &opened, "settings-save-error"));
    assert!(!shows(cx, &opened, "settings-saved-scripture"));
}

#[gpui_kit::test]
fn overlay_saves_only_its_fields_and_reports_a_test_nobody_took(cx: &mut TestAppContext) {
    let fake = Fake {
        send_test: Mutex::new(Ok(false)),
        ..Fake::default()
    };
    let opened = open_with(cx, fake, Some(Section::Overlay), signed_in());
    click(cx, &opened, pane_ids::overlay::SEND);
    assert_eq!(opened.fake.calls.lock().unwrap().tests, 1);
    let message = opened.view.read_with(cx, |v, _| v.test_message());
    assert_eq!(message, "Nothing took the test verse — check Screens");

    // Max verses: text that is not a number reads as 0.
    reveal(cx, &opened, &pane_ids::overlay::MAX_VERSES.into());
    cx.update_window(opened.handle, |_, window, cx| {
        window.click(pane_ids::overlay::MAX_VERSES, cx);
        window.press("secondary-a", cx);
        window.input("abc", cx);
    })
    .unwrap();
    draw(cx, &opened);
    click(cx, &opened, ids::save("overlay"));
    let saved = sets(&opened, "overlay");
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0]["maxVerses"], 0.0);
    // Only the four fields: the theme and routing are never sent.
    assert_eq!(saved[0].as_object().unwrap().len(), 4);
}

#[gpui_kit::test]
fn the_meter_runs_only_while_audio_is_open(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Audio));
    assert_eq!(opened.fake.calls.lock().unwrap().meters, vec!["default"]);
    assert_eq!(*opened.fake.meter_alive.lock().unwrap(), 1);
    let tx = opened.fake.meter_tx.lock().unwrap().clone().unwrap();
    tx.unbounded_send(MeterEvent::Level(AudioLevel {
        rms: 0.5,
        peak: 0.6,
        clipping: false,
        timestamp: 0.0,
    }))
    .unwrap();
    draw(cx, &opened);
    assert!(shows(cx, &opened, pane_ids::audio::LIVE));

    // Picking another input restarts the meter on it.
    click(cx, &opened, pane_ids::audio::input(1));
    assert_eq!(
        opened.fake.calls.lock().unwrap().meters,
        vec!["default", "usb"]
    );
    assert_eq!(*opened.fake.meter_alive.lock().unwrap(), 1);

    // Leaving the pane stops capture.
    click(cx, &opened, ids::nav("general"));
    assert_eq!(*opened.fake.meter_alive.lock().unwrap(), 0);
}

#[gpui_kit::test]
fn audio_shows_capture_failure_and_an_empty_device_list(cx: &mut TestAppContext) {
    let fake = Fake {
        meter_error: Mutex::new(Some("Microphone access was denied.".into())),
        ..Fake::default()
    };
    let opened = open_with(cx, fake, Some(Section::Audio), signed_in());
    assert!(shows(cx, &opened, pane_ids::audio::ERROR));
    assert!(!shows(cx, &opened, pane_ids::audio::LIVE));

    let fake = Fake {
        inputs: Mutex::new(Vec::new()),
        ..Fake::default()
    };
    let opened = open_with(cx, fake, Some(Section::Audio), signed_in());
    assert!(shows(cx, &opened, pane_ids::audio::NO_INPUTS));
}

#[gpui_kit::test]
fn audio_save_merges_the_output_into_the_stored_presentation(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Audio));
    opened
        .view
        .update(cx, |v, cx| v.pick_output("Speakers".into(), cx));
    click(cx, &opened, ids::save("audio"));
    let presentation = sets(&opened, "presentation");
    assert_eq!(presentation[0]["audio"]["outputLabel"], "Speakers");
    // What was stored stays.
    assert_eq!(presentation[0]["audio"]["volume"], 0.5);
    assert_eq!(sets(&opened, "audio").len(), 1);
    assert!(shows(cx, &opened, "settings-saved-audio"));
}

#[gpui_kit::test]
fn shortcut_conflicts_name_the_other_command(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Shortcuts));
    let search = pane_ids::shortcuts::binding(ShortcutCommand::Search);
    click(cx, &opened, search.clone());
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.recording()),
        Some(ShortcutCommand::Search)
    );
    // Space presents the selected suggestion.
    press(cx, &opened, "space");
    assert!(shows(cx, &opened, pane_ids::shortcuts::ERROR));
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.shortcut_error()),
        Some("Already assigned to Present selected suggestion. Clear that binding first.".into())
    );
    // Still listening; Escape cancels.
    press(cx, &opened, "escape");
    assert_eq!(opened.view.read_with(cx, |v, _| v.recording()), None);

    // Clearing the other binding frees the key.
    click(
        cx,
        &opened,
        pane_ids::shortcuts::clear(ShortcutCommand::Approve),
    );
    click(cx, &opened, search);
    press(cx, &opened, "space");
    assert_eq!(opened.view.read_with(cx, |v, _| v.recording()), None);
    assert!(!shows(cx, &opened, pane_ids::shortcuts::ERROR));

    click(cx, &opened, pane_ids::shortcuts::SAVE);
    let display = sets(&opened, "display");
    assert_eq!(display[0]["shortcuts"]["search"], json!(["Space"]));
    assert_eq!(display[0]["shortcuts"]["approve"], json!([]));
}

#[gpui_kit::test]
fn restore_defaults_clears_custom_bindings(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Shortcuts));
    click(
        cx,
        &opened,
        pane_ids::shortcuts::clear(ShortcutCommand::Next),
    );
    click(cx, &opened, pane_ids::shortcuts::RESTORE);
    click(cx, &opened, pane_ids::shortcuts::SAVE);
    assert_eq!(sets(&opened, "display")[0]["shortcuts"], json!({}));
}

#[gpui_kit::test]
fn storage_reports_errors_rescans_and_keeps_backgrounds(cx: &mut TestAppContext) {
    let fake = Fake::default();
    fake.workspace.lock().unwrap().0.ready = false;
    fake.workspace.lock().unwrap().0.error = Some("permission denied".into());
    let opened = open_with(cx, fake, None, signed_in());
    assert!(shows(cx, &opened, pane_ids::general::FOLDER_ERROR));

    click(cx, &opened, pane_ids::general::RESCAN);
    assert_eq!(opened.fake.calls.lock().unwrap().resyncs, 1);
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.workspace_note()),
        Some("Songs folder read — 12 in the library, 1 removed.".into())
    );

    // Adopting fails: the error is the note and the offer stays.
    click(cx, &opened, pane_ids::general::ADOPT_SWITCH);
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.workspace_note()),
        Some("The disk is full.".into())
    );
    assert!(shows(cx, &opened, pane_ids::general::ADOPT_KEEP));
    click(cx, &opened, pane_ids::general::ADOPT_KEEP);
    assert!(!shows(cx, &opened, pane_ids::general::ADOPT_KEEP));
}

#[gpui_kit::test]
fn update_and_converter_rows(cx: &mut TestAppContext) {
    let opened = open(cx, None);
    click(cx, &opened, pane_ids::general::UPDATE);
    assert_eq!(
        opened.fake.calls.lock().unwrap().updates,
        vec![UpdateAction::Check]
    );
    // A pushed status replaces the row and frees the button.
    let tx = opened.fake.events.lock().unwrap()[0].clone();
    tx.unbounded_send(BackendEvent::Update(UpdateStatus::Available {
        version: "1.3.0".into(),
        notes: None,
    }))
    .unwrap();
    draw(cx, &opened);
    click(cx, &opened, pane_ids::general::UPDATE);
    assert_eq!(
        opened.fake.calls.lock().unwrap().updates,
        vec![UpdateAction::Check, UpdateAction::Download]
    );
    assert!(shows(cx, &opened, pane_ids::general::CONVERTER));
}

#[gpui_kit::test]
fn a_failed_pack_download_shows_the_error_and_can_retry(cx: &mut TestAppContext) {
    let fake = Fake {
        download: Mutex::new(Err("Checksum mismatch".into())),
        ..Fake::default()
    };
    let opened = open_with(cx, fake, Some(Section::Scripture), signed_in());
    assert!(shows(cx, &opened, pane_ids::scripture::pack("NKJV")));
    click(cx, &opened, pane_ids::scripture::pack_action("NKJV"));
    assert_eq!(opened.fake.calls.lock().unwrap().downloads, vec!["NKJV"]);
    assert!(shows(cx, &opened, pane_ids::scripture::pack_detail("NKJV")));
    assert_eq!(
        opened.view.read_with(cx, |v, _| v.pack_detail("NKJV")),
        "Checksum mismatch"
    );

    // Download all stops at the first failure and says why.
    click(cx, &opened, pane_ids::scripture::DOWNLOAD_ALL);
    assert!(shows(cx, &opened, pane_ids::scripture::BULK_ERROR));
    // A failed trace read is the empty state.
    assert!(shows(cx, &opened, pane_ids::scripture::TRACE_EMPTY));
}

#[gpui_kit::test]
fn account_offers_setup_and_tips_and_signed_out_shows_sign_in(cx: &mut TestAppContext) {
    let opened = open(cx, Some(Section::Account));
    assert!(shows(cx, &opened, pane_ids::account::NAME));
    click(cx, &opened, pane_ids::account::SHOW_TIPS);
    click(cx, &opened, pane_ids::account::RUN_SETUP);
    assert_eq!(opened.fake.calls.lock().unwrap().resets, 1);
    assert_eq!(
        *opened.events.lock().unwrap(),
        vec![SettingsEvent::ShowTips, SettingsEvent::RunSetup]
    );

    click(cx, &opened, pane_ids::account::SIGN_OUT);
    let state = opened.account.read_with(cx, |a, _| a.session().state);
    assert_eq!(state, CloudSessionState::SignedOut);
    assert!(shows(cx, &opened, ui_account::auth::ids::EMAIL));
}

#[gpui_kit::test]
fn escape_and_the_red_light_ask_to_close(cx: &mut TestAppContext) {
    let opened = open(cx, None);
    let close = gpui_kit::ElementId::NamedChild(
        Arc::new(gpui_kit::ElementId::from("settings-lights")),
        "close".into(),
    );
    click(cx, &opened, close);
    assert_eq!(*opened.events.lock().unwrap(), vec![SettingsEvent::Close]);
}
