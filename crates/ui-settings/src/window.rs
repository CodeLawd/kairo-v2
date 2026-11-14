//! The Settings window (`Settings.tsx`): a movable in-app window laid out as
//! macOS System Settings. A frosted sidebar with the traffic lights, search,
//! the account card and the panes; a title bar with the back and forward
//! capsule; the pane's grouped boxes below.

use crate::backend::{BackendEvent, SettingsBackend};
use crate::model::{History, Section, nav_items};
use crate::panes::{AccountPane, AudioPane, LibraryPane, ShortcutsPane, TracePane, WorkspacePane};
use crate::widgets;
use futures_util::StreamExt;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, ParentElement, Render, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder,
    px, size,
};
use kairo_domain::documents::SlideConverterStatus;
use kairo_domain::scripture::ScriptureTranslationOption;
use kairo_domain::settings::AppSettings;
use kairo_domain::status::UpdateStatus;
use kairo_ui_kit::components::{MovableDialog, MovableDialogEvent, TrafficLights};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use ui_account::Account;

/// The window's size when nothing is remembered (`SETTINGS_FRAME`).
pub const FRAME: (f32, f32) = (760.0, 640.0);
/// The smallest it may be (`SETTINGS_MIN`).
pub const FRAME_MIN: (f32, f32) = (640.0, 480.0);
/// How long "Saved" shows.
pub const SAVED_FOR: Duration = Duration::from_secs(2);

/// Element ids, for tests.
pub mod ids {
    /// The window.
    pub const WINDOW: &str = "settings-window";
    /// The sidebar search.
    pub const SEARCH: &str = "settings-search";
    /// The account card.
    pub const ACCOUNT_CARD: &str = "settings-account-card";
    /// "No matches".
    pub const NO_MATCHES: &str = "settings-no-matches";
    /// Back.
    pub const BACK: &str = "settings-back";
    /// Forward.
    pub const FORWARD: &str = "settings-forward";
    /// The pane title.
    pub const TITLE: &str = "settings-title";
    /// "Loading settings…".
    pub const LOADING: &str = "settings-loading";
    /// A section's Save button: `settings-save-<section id>`.
    pub fn save(section: &str) -> String {
        format!("settings-save-{section}")
    }
    /// A sidebar item: `settings-nav-<section id>`.
    pub fn nav(section: &str) -> String {
        format!("settings-nav-{section}")
    }
}

/// What the window asks of its owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsEvent {
    /// Close the window (red light, Escape, the scrim).
    Close,
    /// "Run setup again" finished resetting; the window should close.
    RunSetup,
    /// "Show tips again": close and replay the Operator tips.
    ShowTips,
}

/// The result of an overlay test (`TestStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TestStatus {
    /// Nothing sent yet.
    #[default]
    Idle,
    /// Waiting for the answer.
    Testing,
    /// It worked.
    Ok,
    /// It did not.
    Fail,
}

/// The window.
pub struct SettingsWindow {
    pub(crate) backend: Arc<dyn SettingsBackend>,
    pub(crate) account: Entity<Account>,
    pub(crate) dialog: Entity<MovableDialog>,
    pub(crate) history: History,
    pub(crate) query: Entity<InputState>,
    pub(crate) scroll: ScrollHandle,
    pub(crate) settings: AppSettings,
    pub(crate) loading: bool,
    pub(crate) load_error: Option<SharedString>,
    pub(crate) saved: Option<Section>,
    pub(crate) save_error: Option<SharedString>,
    saved_timer: Option<Task<()>>,
    // General.
    pub(crate) font_size: Entity<SliderState>,
    pub(crate) transcript_size: Entity<SliderState>,
    pub(crate) gloss: Entity<InputState>,
    pub(crate) workspace: WorkspacePane,
    pub(crate) converter: Option<SlideConverterStatus>,
    pub(crate) update: UpdateStatus,
    pub(crate) update_busy: bool,
    // Verses on screen.
    pub(crate) max_verses: Entity<InputState>,
    pub(crate) test: (TestStatus, SharedString),
    // Scripture.
    pub(crate) translations: Vec<ScriptureTranslationOption>,
    pub(crate) translations_loading: bool,
    pub(crate) downloading_translation: bool,
    pub(crate) confidence: Entity<SliderState>,
    pub(crate) debounce: Entity<InputState>,
    pub(crate) context_window: Entity<InputState>,
    pub(crate) library: LibraryPane,
    pub(crate) traces: TracePane,
    // Audio.
    pub(crate) audio: AudioPane,
    // Shortcuts.
    pub(crate) shortcuts: ShortcutsPane,
    // Account.
    pub(crate) account_pane: AccountPane,
    pub(crate) focus: FocusHandle,
    opened_at: Option<std::time::Instant>,
    _events: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsWindow {}

fn slider(min: f32, max: f32, step: f32, value: f64) -> SliderState {
    SliderState::new()
        .min(min)
        .max(max)
        .step(step)
        .default_value(value as f32)
}

impl SettingsWindow {
    /// The window, opened at `initial` (`general` when `None`). `frame` is
    /// the remembered size, already clamped by the caller.
    pub fn new(
        backend: Arc<dyn SettingsBackend>,
        account: Entity<Account>,
        initial: Option<Section>,
        frame: (f32, f32),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let opened_at = std::time::Instant::now();
        let initial = initial.unwrap_or(Section::General);
        let dialog = cx.new(|cx| {
            MovableDialog::new(
                ids::WINDOW,
                "Settings",
                size(px(frame.0), px(frame.1)),
                window,
                cx,
            )
        });
        // The startup snapshot, if the account model has one, so the window
        // opens filled; the backend's fresh read replaces it.
        let seed = account.read(cx).settings().cloned();
        let loading = seed.is_none();
        let settings = seed.unwrap_or_else(|| {
            AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone())
        });
        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let font_size = cx.new(|_| slider(12.0, 24.0, 1.0, settings.display.font_size));
        let transcript_size =
            cx.new(|_| slider(12.0, 48.0, 1.0, settings.display.transcription_font_size));
        let confidence =
            cx.new(|_| slider(0.0, 1.0, 0.05, settings.scripture.confidence_threshold));
        let text_field = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value))
        };
        let gloss = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(crate::model::DEFAULT_GLOSS)
                .default_value(gloss_text(&settings))
        });
        let max_verses = text_field(
            crate::model::number_text(settings.overlay.max_verses),
            window,
            cx,
        );
        let debounce = text_field(
            crate::model::number_text(settings.scripture.debounce_interval),
            window,
            cx,
        );
        let context_window = text_field(
            crate::model::number_text(settings.scripture.context_window_size),
            window,
            cx,
        );
        let shortcuts = ShortcutsPane::new(&settings, cx);
        let account_pane = AccountPane::new(account.clone(), window, cx);
        let mut subscriptions = vec![
            cx.subscribe_in(
                &dialog,
                window,
                |_, _, event: &MovableDialogEvent, _, cx| {
                    if *event == MovableDialogEvent::Dismiss {
                        cx.emit(SettingsEvent::Close);
                    }
                },
            ),
            cx.subscribe(&query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe(&font_size, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    this.settings.display.font_size = value.start().round() as f64;
                    cx.notify();
                }
            }),
            cx.subscribe(&transcript_size, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    this.settings.display.transcription_font_size = value.start().round() as f64;
                    cx.notify();
                }
            }),
            cx.subscribe(&confidence, |this, _, event: &SliderEvent, cx| {
                if let SliderEvent::Change(value) = event {
                    // The slider steps in 0.05; keep two places as `toFixed(2)` shows.
                    this.settings.scripture.confidence_threshold =
                        (value.start() as f64 * 100.0).round() / 100.0;
                    cx.notify();
                }
            }),
            cx.subscribe_in(&gloss, window, Self::on_gloss),
            cx.observe(&account, |this, account, cx| {
                let session = account.read(cx).session().clone();
                this.account_pane.sync(&session, cx);
                cx.notify();
            }),
        ];
        for (field, apply) in [
            (max_verses.clone(), NumberKind::MaxVerses),
            (debounce.clone(), NumberKind::Debounce),
            (context_window.clone(), NumberKind::ContextWindow),
        ] {
            subscriptions.push(cx.subscribe_in(
                &field,
                window,
                move |this, field, event: &InputEvent, window, cx| {
                    this.on_number(apply, field, event, window, cx)
                },
            ));
        }

        let mut events = backend.events();
        let _events = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |this, cx| this.on_backend(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let update = backend.update_status();
        let mut this = Self {
            backend,
            account,
            dialog,
            history: History::new(initial),
            query,
            scroll: ScrollHandle::new(),
            settings,
            loading,
            load_error: None,
            saved: None,
            save_error: None,
            saved_timer: None,
            font_size,
            transcript_size,
            gloss,
            workspace: WorkspacePane::default(),
            converter: None,
            update,
            update_busy: false,
            max_verses,
            test: (TestStatus::Idle, SharedString::default()),
            translations: Vec::new(),
            translations_loading: true,
            downloading_translation: false,
            confidence,
            debounce,
            context_window,
            library: LibraryPane::default(),
            traces: TracePane::default(),
            audio: AudioPane::default(),
            shortcuts,
            account_pane,
            focus: cx.focus_handle(),
            opened_at: Some(opened_at),
            _events,
            _subscriptions: subscriptions,
        };
        let blurs = this.shortcut_blurs(window, cx);
        this._subscriptions.extend(blurs);
        this.load(window, cx);
        this.load_workspace(cx);
        this.load_converter(cx);
        this.load_translations(cx);
        this.on_section_shown(window, cx);
        this
    }

    /// The pane showing.
    pub fn section(&self) -> Section {
        self.history.current()
    }

    /// The overlay test's last message.
    pub fn test_message(&self) -> SharedString {
        self.test.1.clone()
    }

    /// The shortcut command listening for keys.
    pub fn recording(&self) -> Option<kairo_domain::settings::ShortcutCommand> {
        self.shortcuts.recording
    }

    /// The shortcut conflict or save error.
    pub fn shortcut_error(&self) -> Option<String> {
        self.shortcuts.error.clone()
    }

    /// The note under Storage.
    pub fn workspace_note(&self) -> Option<String> {
        self.workspace.note.clone()
    }

    /// A Bible pack row's detail line.
    pub fn pack_detail(&self, id: &str) -> String {
        let lib = crate::model::library(&self.translations, &self.library.installed);
        let row = lib.packs.iter().find(|p| p.id.eq_ignore_ascii_case(id));
        self.library
            .rows
            .get(&id.to_uppercase())
            .map(|s| {
                s.detail(
                    row.is_some_and(|r| r.downloadable),
                    row.and_then(|r| r.size.as_deref()),
                )
            })
            .unwrap_or_default()
    }

    /// The working copy of the settings.
    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    /// Opens `section`, as clicking it in the sidebar does.
    pub fn select(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        if self.history.current() == section {
            return;
        }
        self.history.select(section);
        self.on_section_shown(window, cx);
    }

    /// Back (`-1`) or forward (`1`).
    pub fn go(&mut self, delta: i8, window: &mut Window, cx: &mut Context<Self>) {
        let before = self.history.current();
        self.history.go(delta);
        if self.history.current() != before {
            self.on_section_shown(window, cx);
        }
    }

    fn on_section_shown(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        // Only the Audio pane listens to the microphone, as the meter lives
        // and dies with `AudioSection`.
        if self.section() == Section::Audio {
            self.open_audio(window, cx);
        } else {
            self.close_audio(cx);
        }
        if self.section() == Section::Scripture {
            self.open_library(cx);
            self.load_traces(cx);
        }
        cx.notify();
    }

    /// Stops the meter: the window is going away.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.close_audio(cx);
    }

    // ── Loading ─────────────────────────────────────────────────────────

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let request = self.backend.settings();
        cx.spawn_in(window, async move |this, cx| {
            let result = request.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(settings) => this.apply_loaded(settings, window, cx),
                    Err(error) => {
                        tracing::warn!(%error, "[Settings] could not read settings");
                        this.load_error = Some(error.into());
                    }
                }
                this.loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_loaded(&mut self, settings: AppSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.font_size.update(cx, |s, cx| {
            s.set_value(settings.display.font_size as f32, window, cx)
        });
        self.transcript_size.update(cx, |s, cx| {
            s.set_value(settings.display.transcription_font_size as f32, window, cx)
        });
        self.confidence.update(cx, |s, cx| {
            s.set_value(settings.scripture.confidence_threshold as f32, window, cx)
        });
        let gloss = gloss_text(&settings);
        self.gloss
            .update(cx, |input, cx| input.set_value(gloss, window, cx));
        for (field, value) in [
            (&self.max_verses, settings.overlay.max_verses),
            (&self.debounce, settings.scripture.debounce_interval),
            (&self.context_window, settings.scripture.context_window_size),
        ] {
            let text = crate::model::number_text(value);
            field.update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.shortcuts
            .reset(settings.display.shortcuts.clone().unwrap_or_default());
        self.settings = settings;
    }

    fn load_translations(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.translations();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(list) => this.translations = list,
                    // The picker keeps the saved default; the library falls
                    // back to the registry default.
                    Err(error) => tracing::warn!(%error, "[Settings] translations unavailable"),
                }
                this.translations_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// Re-reads the translations after a download or removal (best effort).
    pub(crate) fn refresh_translations(&mut self, cx: &mut Context<Self>) {
        let request = self.backend.translations();
        cx.spawn(async move |this, cx| {
            if let Ok(list) = request.await {
                let _ = this.update(cx, |this, cx| {
                    this.translations = list;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn on_backend(&mut self, event: BackendEvent, cx: &mut Context<Self>) {
        match event {
            BackendEvent::Update(status) => {
                self.update = status;
                self.update_busy = false;
            }
            BackendEvent::Converter(status) => self.converter = Some(status),
            BackendEvent::PackProgress(progress) => self.on_pack_progress(progress, cx),
        }
        cx.notify();
    }

    // ── Editing ─────────────────────────────────────────────────────────

    fn on_gloss(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !matches!(event, InputEvent::Change) {
            return;
        }
        let value = input.read(cx).value().to_string();
        self.set_gloss(value, cx);
    }

    /// Sets the gloss colour; a valid `#RRGGBB` is saved at once, like a
    /// live control.
    pub(crate) fn set_gloss(&mut self, value: String, cx: &mut Context<Self>) {
        if self.settings.lyrics.gloss_color == value {
            return;
        }
        self.settings.lyrics.gloss_color = value.clone();
        if crate::model::is_hex_color(value.trim()) {
            self.persist("lyrics", cx);
        }
        cx.notify();
    }

    fn on_number(
        &mut self,
        kind: NumberKind,
        field: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = field.read(cx).value().to_string();
        let value = match kind {
            NumberKind::MaxVerses => crate::model::max_verses_field(&text),
            NumberKind::Debounce => crate::model::debounce_field(&text),
            NumberKind::ContextWindow => crate::model::context_window_field(&text),
        };
        match event {
            InputEvent::Change => {
                match kind {
                    NumberKind::MaxVerses => self.settings.overlay.max_verses = value,
                    NumberKind::Debounce => self.settings.scripture.debounce_interval = value,
                    NumberKind::ContextWindow => {
                        self.settings.scripture.context_window_size = value
                    }
                }
                cx.notify();
            }
            // A controlled number input shows the value it settled on.
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                let shown = crate::model::number_text(value);
                if text != shown {
                    field.update(cx, |input, cx| input.set_value(shown, window, cx));
                }
            }
            InputEvent::Focus => {}
        }
    }

    /// Dark mode applies and saves at once.
    pub(crate) fn set_dark(&mut self, dark: bool, window: &mut Window, cx: &mut Context<Self>) {
        use kairo_domain::settings::DisplayTheme;
        self.settings.display.theme = if dark {
            DisplayTheme::Dark
        } else {
            DisplayTheme::Light
        };
        kairo_ui_kit::set_appearance(
            if dark {
                kairo_ui_kit::Appearance::Dark
            } else {
                kairo_ui_kit::Appearance::Light
            },
            Some(window),
            cx,
        );
        self.persist("display", cx);
        cx.notify();
    }

    /// A privacy choice takes effect the moment it is made, not on Save.
    pub(crate) fn set_usage(
        &mut self,
        share: Option<bool>,
        crash: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if let Some(share) = share {
            self.settings.usage.share_stats = share;
        }
        if let Some(crash) = crash {
            self.settings.usage.send_crash_reports = Some(crash);
        }
        self.persist("usage", cx);
        cx.notify();
    }

    /// Writes one section as it stands, without the Saved flash. A failure
    /// is logged, as the original's fire-and-forget `settings.set`.
    pub(crate) fn persist(&mut self, key: &'static str, cx: &mut Context<Self>) {
        let Some(value) = section_value(&self.settings, key) else {
            return;
        };
        let request = self.backend.set_section(key, value);
        cx.spawn(async move |_, _| {
            if let Err(error) = request.await {
                tracing::warn!(%error, key, "[Settings] could not save");
            }
        })
        .detach();
    }

    // ── Saving ──────────────────────────────────────────────────────────

    /// The Save button of `section`.
    pub fn save(&mut self, section: Section, cx: &mut Context<Self>) {
        self.save_error = None;
        let writes: Vec<(&'static str, Value)> = match section {
            Section::General => ["display", "lyrics"]
                .into_iter()
                .filter_map(|k| section_value(&self.settings, k).map(|v| (k, v)))
                .collect(),
            Section::Scripture => section_value(&self.settings, "scripture")
                .map(|v| vec![("scripture", v)])
                .unwrap_or_default(),
            // Only the fields this pane edits: the store merges onto the
            // stored overlay, so the Theme page's choices are kept.
            Section::Overlay => {
                let o = &self.settings.overlay;
                vec![(
                    "overlay",
                    serde_json::json!({
                        "template": o.template,
                        "showTranslation": o.show_translation,
                        "showVerseNumbers": o.show_verse_numbers,
                        "maxVerses": o.max_verses,
                    }),
                )]
            }
            Section::Audio => {
                return self.save_audio(cx);
            }
            Section::ApiKeys | Section::Shortcuts | Section::Account => return,
        };
        let requests: Vec<_> = writes
            .into_iter()
            .map(|(k, v)| self.backend.set_section(k, v))
            .collect();
        cx.spawn(async move |this, cx| {
            let mut failure = None;
            for request in requests {
                if let Err(error) = request.await {
                    failure = Some(error);
                }
            }
            let _ = this.update(cx, |this, cx| this.saved_result(section, failure, cx));
        })
        .detach();
    }

    fn save_audio(&mut self, cx: &mut Context<Self>) {
        let audio = section_value(&self.settings, "audio");
        let label = self.settings.presentation.audio.output_label.clone();
        let backend = self.backend.clone();
        let set_audio = audio.map(|v| backend.set_section("audio", v));
        let current = backend.get_section("presentation");
        cx.spawn(async move |this, cx| {
            let mut failure = None;
            if let Some(request) = set_audio
                && let Err(error) = request.await
            {
                failure = Some(error);
            }
            // Merge onto the stored presentation so nothing else is lost.
            match current.await {
                Ok(mut presentation) => {
                    if let Some(audio) = presentation
                        .as_object_mut()
                        .map(|p| p.entry("audio").or_insert_with(|| serde_json::json!({})))
                        .and_then(Value::as_object_mut)
                    {
                        audio.insert("outputLabel".into(), Value::String(label));
                    }
                    if let Err(error) = backend.set_section("presentation", presentation).await {
                        failure = Some(error);
                    }
                }
                Err(error) => failure = Some(error),
            }
            let _ = this.update(cx, |this, cx| {
                this.saved_result(Section::Audio, failure, cx)
            });
        })
        .detach();
    }

    fn saved_result(&mut self, section: Section, failure: Option<String>, cx: &mut Context<Self>) {
        match failure {
            None => {
                self.saved = Some(section);
                self.saved_timer = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SAVED_FOR).await;
                    let _ = this.update(cx, |this, cx| {
                        this.saved = None;
                        cx.notify();
                    });
                }));
            }
            Some(error) => {
                tracing::warn!(%error, section = section.id(), "[Settings] save failed");
                self.save_error = Some(format!("Couldn’t save: {error}").into());
            }
        }
        cx.notify();
    }

    // ── Layout ──────────────────────────────────────────────────────────

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let query = self.query.read(cx).value().to_string();
        let items = nav_items(&query);
        let current = self.section();
        // The sidebar and title bar move the window (`data-settings-drag`).
        let handle = self.dialog.update(cx, |dialog, cx| {
            dialog.drag_handle("settings-sidebar-drag", cx)
        });
        let session = self.account.read(cx).session().clone();
        let name = session
            .user
            .as_ref()
            .map(|u| u.name.trim().to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| session.user.as_ref().map(|u| u.email.clone()))
            .unwrap_or_else(|| "Account".into());
        let meta = if session.state == kairo_domain::cloud::CloudSessionState::Stale {
            "Offline".to_owned()
        } else {
            session
                .org
                .as_ref()
                .map(|o| o.name.clone())
                .or_else(|| session.user.as_ref().map(|u| u.email.clone()))
                .unwrap_or_else(|| "Signed in".into())
        };
        let initials = crate::model::initials(
            session.user.as_ref().map(|u| u.name.as_str()),
            session.user.as_ref().map(|u| u.email.as_str()),
        );
        let entity = cx.entity();
        handle
            .h_full()
            .bg(k.glass_panel())
            .child(
                v_flex()
                    .w(px(220.))
                    .h_full()
                    .border_r_1()
                    .border_color(k.panel_border)
                    .child(
                        div()
                            .px(px(18.))
                            .pt(px(18.))
                            .pb_3()
                            .child(TrafficLights::new("settings-lights").on_close(
                                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::Close)),
                            )),
                    )
                    .child(
                        div().px_3().pb(px(10.)).child(
                            h_flex()
                                .h_7()
                                .gap_1p5()
                                .items_center()
                                .rounded(radius::lg())
                                .bg(k.surface_secondary)
                                .px_2()
                                .child(
                                    Icon::new(KairoIcon::ScanSearch)
                                        .size_3()
                                        .text_color(k.text(TextStep::T400)),
                                )
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Input::new(&self.query)
                                            .id(ids::SEARCH)
                                            .appearance(false)
                                            .small()
                                            .aria_label("Search settings"),
                                    ),
                                ),
                        ),
                    )
                    .child(
                        h_flex()
                            .id(ids::ACCOUNT_CARD)
                            .test_support()
                            .role(Role::Button)
                            .aria_label(format!("{name}: {meta}"))
                            .tab_index(0)
                            .mx_2()
                            .mb_3()
                            .gap(px(10.))
                            .items_center()
                            .rounded(radius::lg())
                            .px_2()
                            .py_1p5()
                            .cursor_pointer()
                            .map(|this| {
                                if current == Section::Account {
                                    this.bg(k.white.opacity(0.1))
                                } else {
                                    this.hover(|s| s.bg(k.white.opacity(0.05)))
                                }
                            })
                            .child(widgets::avatar(initials, 32.0, cx))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                                            .text_color(k.text_primary)
                                            .child(name),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(meta),
                                    ),
                            )
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.select(Section::Account, window, cx)
                                    })
                                }
                            }),
                    )
                    .child(
                        v_flex()
                            .id("settings-nav")
                            .test_support()
                            .role(Role::Navigation)
                            .aria_label("Settings")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .gap_1()
                            .px_2()
                            .pb_3()
                            .when(items.is_empty(), |this| {
                                this.child(
                                    div()
                                        .id(ids::NO_MATCHES)
                                        .test_support()
                                        .px_2()
                                        .py_3()
                                        .text_size(text::XS.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child("No matches"),
                                )
                            })
                            .children(items.into_iter().map(|section| {
                                let active = section == current;
                                let entity = entity.clone();
                                h_flex()
                                    .id(SharedString::from(ids::nav(section.id())))
                                    .test_support()
                                    .role(Role::Link)
                                    .aria_label(format!(
                                        "{} settings: {}",
                                        section.label(),
                                        section.hint()
                                    ))
                                    .aria_selected(active)
                                    .tab_index(0)
                                    .w_full()
                                    .gap(px(10.))
                                    .items_center()
                                    .rounded(radius::lg())
                                    .px(px(10.))
                                    .py_2()
                                    .text_size(text::SM.size)
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .cursor_pointer()
                                    .map(|this| {
                                        if active {
                                            this.bg(k.white.opacity(0.1)).text_color(k.text_primary)
                                        } else {
                                            this.text_color(k.text(TextStep::T400)).hover(|s| {
                                                s.bg(k.white.opacity(0.05))
                                                    .text_color(k.text(TextStep::T200))
                                            })
                                        }
                                    })
                                    .child(Icon::new(section.icon()).size(px(15.)))
                                    .child(div().min_w_0().truncate().child(section.label()))
                                    .on_click(move |_, window, cx| {
                                        entity
                                            .update(cx, |this, cx| this.select(section, window, cx))
                                    })
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let can_back = self.history.can_back();
        let can_forward = self.history.can_forward();
        let chevron = |id: &'static str,
                       label: &'static str,
                       icon,
                       enabled: bool,
                       delta: i8,
                       cx: &mut Context<Self>| {
            div()
                .id(id)
                .test_support()
                .role(Role::Button)
                .aria_label(label)
                .tab_index(0)
                .h_7()
                .w_8()
                .flex()
                .items_center()
                .justify_center()
                .text_color(k.text(TextStep::T300))
                .child(Icon::new(icon).size(px(15.)))
                .map(|this| {
                    if enabled {
                        this.cursor_pointer()
                            .hover(|s| s.bg(k.surface_elevated))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.go(delta, window, cx)),
                            )
                    } else {
                        this.opacity(0.3)
                    }
                })
        };
        let handle = self.dialog.update(cx, |dialog, cx| {
            dialog.drag_handle("settings-header-drag", cx)
        });
        handle
            .bg(k.glass_panel())
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .border_b_1()
                    .border_color(k.panel_border)
                    .px_6()
                    .py_4()
                    .child(
                        // Back and forward share one capsule, as in System Settings.
                        h_flex()
                            .h_7()
                            .items_center()
                            .rounded_full()
                            .overflow_hidden()
                            .bg(k.surface_tertiary)
                            .child(chevron(
                                ids::BACK,
                                "Back",
                                KairoIcon::ChevronLeft,
                                can_back,
                                -1,
                                cx,
                            ))
                            .child(div().h_4().w_px().bg(k.surface_elevated))
                            .child(chevron(
                                ids::FORWARD,
                                "Forward",
                                KairoIcon::ChevronRight,
                                can_forward,
                                1,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id(ids::TITLE)
                            .test_support()
                            .role(Role::Heading)
                            .text_size(px(15.))
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .text_color(k.text_primary)
                            .child(self.section().label()),
                    ),
            )
            .into_any_element()
    }

    fn render_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        if self.loading {
            return h_flex()
                .id(ids::LOADING)
                .test_support()
                .role(Role::Status)
                .gap_3()
                .pt_8()
                .items_center()
                .text_color(k.text(TextStep::T500))
                .child(Icon::new(KairoIcon::Loader).size_4())
                .child(div().text_size(text::SM.size).child("Loading settings…"))
                .into_any_element();
        }
        let body = match self.section() {
            Section::General => self.render_general(window, cx),
            Section::Overlay => self.render_overlay(cx),
            Section::Scripture => self.render_scripture(window, cx),
            Section::Audio => self.render_audio(cx),
            Section::ApiKeys => self.account_pane.license.clone().into_any_element(),
            Section::Shortcuts => self.render_shortcuts(cx),
            Section::Account => self.render_account(window, cx),
        };
        v_flex()
            .gap_5()
            .when_some(self.load_error.clone(), |this, error| {
                this.child(widgets::notice(
                    "settings-load-error",
                    format!("Couldn’t read your settings ({error}). Showing defaults; saving writes them."),
                    cx,
                ))
            })
            .child(body)
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum NumberKind {
    MaxVerses,
    Debounce,
    ContextWindow,
}

fn gloss_text(settings: &AppSettings) -> String {
    if settings.lyrics.gloss_color.is_empty() {
        crate::model::DEFAULT_GLOSS.to_owned()
    } else {
        settings.lyrics.gloss_color.clone()
    }
}

/// One settings section as JSON.
pub(crate) fn section_value(settings: &AppSettings, key: &str) -> Option<Value> {
    serde_json::to_value(settings).ok()?.get(key).cloned()
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.dialog.read(cx).focus_handle(cx)
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let sidebar = self.render_sidebar(cx);
        let header = self.render_header(cx);
        let pane = self.render_pane(window, cx);
        let content = h_flex()
            .size_full()
            .overflow_hidden()
            // Electron's `.modal-glass` blurs the darker booth behind it, so the
            // pane reads darker than the `surface-secondary` boxes. Without a
            // backdrop the glass over the frame's fill matches the boxes, so
            // the pane is solid `surface` (DECISIONS 074).
            .bg(k.surface)
            .text_color(k.text_primary)
            .child(sidebar)
            .child(
                v_flex().min_w_0().flex_1().h_full().child(header).child(
                    div()
                        .id("settings-pane")
                        .test_support()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&self.scroll)
                        .px_5()
                        .pb_6()
                        .pt_3()
                        .child(pane),
                ),
            );
        let _ = &self.focus;
        // Open to first paint (the 100 ms budget), logged once.
        let content = content.when_some(self.opened_at.take(), |this, opened| {
            this.child(
                gpui_kit::canvas(
                    |_, _, _| {},
                    move |_, _, _, _| {
                        let ms = opened.elapsed().as_secs_f64() * 1000.0;
                        tracing::info!(ms, "[Settings] opened");
                        if std::env::var_os("KAIRO_REPORT_STARTUP").is_some() {
                            eprintln!("[Settings] opened in {ms:.1} ms");
                        }
                    },
                )
                .absolute()
                .size_0(),
            )
        });
        self.dialog
            .update(cx, |dialog, cx| dialog.frame(content, window, cx))
    }
}
