//! The Engage page (`Audience.tsx` and its parts): engagements, Slido and
//! sample sources in the sidebar; Q&A or Polls in the middle; the live rail
//! on the right. Dialogs (new engagement, engagement settings, poll editor)
//! draw over the page.

use gpui_kit::component::FocusTrapElement;
use kairo_ui_kit::ControlSize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, AnyView, App, AppContext, ClipboardItem, Context, Entity, FocusHandle, Focusable,
    FontWeight, Hsla, Image, ImageFormat, InteractiveElement, IntoElement, KeyDownEvent,
    ListAlignment, ListState, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, img, list, px, relative,
};
use kairo_domain::audience::{
    AudienceQuestion, AudienceQuestionSort, AudienceQuestionState, AudienceRoomSettings,
    AudienceSourceOptions, AudienceSourceState, AudienceState, DraftOrOpen, EngageCommand,
    EngagementIndex, EngagementInput, EngagementStatus, OpenOrClosed, PollView, ReplyAudience,
    sort_audience_questions,
};
use kairo_domain::program::{ProgramQuestionStyle, QuestionAlign, QuestionPosition};
use kairo_ui_kit::components::{SegmentOption, SegmentedControl};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{
    AccountView, AudienceAnswer, AudienceBackend, AudienceCall, AudienceFeed, FeedSink, Listening,
};
use crate::model::{self, QuestionTab};

pub mod ids {
    pub const ROOT: &str = "engage";
    pub const ERROR: &str = "engage-error";
    pub const ACTIVITY: &str = "engage-activity";
    pub const NEW_ENGAGEMENT: &str = "engage-new";
    pub const RELOAD: &str = "engage-reload";
    pub const SIGNED_OUT: &str = "engage-signed-out";
    pub const FIRST: &str = "engage-first";
    pub const LIST_LOADING: &str = "engage-list-loading";
    pub const LIST_ERROR: &str = "engage-list-error";
    pub const KAIRO_CARD: &str = "engage-kairo";
    pub const KAIRO_STARTING: &str = "engage-kairo-starting";
    pub const SHOW_CODE: &str = "engage-show-code";
    pub const COPY_JOIN: &str = "engage-copy-join";
    pub const QR: &str = "engage-qr";
    pub const AUDIENCE_PAGE: &str = "engage-audience-page";
    pub const SETTINGS: &str = "engage-settings";
    pub const END: &str = "engage-end";
    pub const END_CONFIRM: &str = "engage-end-confirm";
    pub const END_KEEP: &str = "engage-end-keep";
    pub const REOPEN: &str = "engage-reopen";
    pub const KAIRO_ERROR: &str = "engage-kairo-error";
    pub const CONNECT_SLIDO: &str = "engage-connect-slido";
    pub const SLIDO_FORM: &str = "engage-slido-form";
    pub const SLIDO_CODE: &str = "engage-slido-code";
    pub const SLIDO_SKIP: &str = "engage-slido-skip";
    pub const SLIDO_SUBMIT: &str = "engage-slido-connect";
    pub const SLIDO_CANCEL: &str = "engage-slido-cancel";
    pub const SLIDO_ERROR: &str = "engage-slido-error";
    pub const SLIDO_STEPS: &str = "engage-slido-steps";
    pub const SAMPLE: &str = "engage-sample";
    pub const SAMPLE_REMOVE: &str = "engage-sample-remove";
    pub const LOAD_SAMPLE: &str = "engage-load-sample";
    pub const TABS: &str = "engage-question-tabs";
    pub const SORT: &str = "engage-question-sort";
    pub const QUESTIONS: &str = "engage-questions";
    pub const QUESTIONS_LOADING: &str = "engage-questions-loading";
    pub const QUESTIONS_EMPTY: &str = "engage-questions-empty";
    pub const NEW_POLL: &str = "engage-new-poll";
    pub const POLLS_EMPTY: &str = "engage-polls-empty";
    pub const POLLS_NONE: &str = "engage-polls-no-engagement";
    pub const POLLS_LOADING: &str = "engage-polls-loading";
    pub const DIALOG: &str = "engage-dialog";
    pub const DIALOG_CLOSE: &str = "engage-dialog-close";
    pub const NEW_TITLE: &str = "engage-new-title";
    pub const NEW_STARTS: &str = "engage-new-starts";
    pub const NEW_ENDS: &str = "engage-new-ends";
    pub const NEW_START: &str = "engage-new-start";
    pub const NEW_SAVE: &str = "engage-new-save";
    pub const NEW_ERROR: &str = "engage-new-error";
    pub const NEW_DATES: &str = "engage-new-dates";
    pub const NEW_SLOW: &str = "engage-new-slow";
    pub const POLL_TITLE: &str = "engage-poll-title";
    pub const POLL_ADD: &str = "engage-poll-add";
    pub const POLL_MULTIPLE: &str = "engage-poll-multiple";
    pub const POLL_SAVE: &str = "engage-poll-save";
    pub const POLL_ERROR: &str = "engage-poll-error";
    pub const SET_GENERAL: &str = "engage-set-general";
    pub const SET_SCREENS: &str = "engage-set-screens";
    pub const SET_COLLAB: &str = "engage-set-collab";
    pub const SET_DONE: &str = "engage-set-done";
    pub const SET_RESET: &str = "engage-set-reset";
    pub const SET_SAVE: &str = "engage-set-save";
    pub const SET_ERROR: &str = "engage-set-error";
    pub const SET_COPY: &str = "engage-set-copy";
    pub const SET_QR: &str = "engage-set-qr";
    pub const INVITE_EMAIL: &str = "engage-invite-email";
    pub const INVITE: &str = "engage-invite";
    pub const NO_COLLABORATORS: &str = "engage-no-collaborators";
    pub const PREVIEW: &str = "engage-style-preview";
    pub fn engagement(code: &str) -> String {
        format!("engage-item-{code}")
    }
    pub fn delete(code: &str) -> String {
        format!("engage-item-delete-{code}")
    }
    pub fn delete_confirm(code: &str) -> String {
        format!("engage-item-delete-confirm-{code}")
    }
    pub fn delete_cancel(code: &str) -> String {
        format!("engage-item-delete-cancel-{code}")
    }
    pub fn slido(id: &str) -> String {
        format!("engage-slido-{id}")
    }
    pub fn slido_window(id: &str) -> String {
        format!("engage-slido-window-{id}")
    }
    pub fn slido_copy(id: &str) -> String {
        format!("engage-slido-copy-{id}")
    }
    pub fn slido_settings(id: &str) -> String {
        format!("engage-slido-settings-{id}")
    }
    pub fn slido_disconnect(id: &str) -> String {
        format!("engage-slido-disconnect-{id}")
    }
    pub fn slido_skip(id: &str) -> String {
        format!("engage-slido-skip-{id}")
    }
    pub fn slido_pending(id: &str) -> String {
        format!("engage-slido-pending-{id}")
    }
    pub fn question(id: &str) -> String {
        format!("engage-q-{id}")
    }
    pub fn action(id: &str, what: &str) -> String {
        format!("engage-q-{what}-{id}")
    }
    pub fn poll(id: &str) -> String {
        format!("engage-poll-{id}")
    }
    pub fn poll_action(id: &str, what: &str) -> String {
        format!("engage-poll-{what}-{id}")
    }
    pub fn poll_option(i: usize) -> String {
        format!("engage-poll-option-{i}")
    }
    pub fn poll_remove(i: usize) -> String {
        format!("engage-poll-remove-{i}")
    }
    pub fn room(key: &str) -> String {
        format!("engage-room-{key}")
    }
    pub fn revoke(id: &str) -> String {
        format!("engage-revoke-{id}")
    }
    pub fn moderator_menu(id: &str) -> String {
        format!("engage-moderator-menu-{id}")
    }
    pub fn preset(id: &str) -> String {
        format!("engage-preset-{id}")
    }
    pub fn style(key: &str) -> String {
        format!("engage-style-{key}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Qa,
    Polls,
}

/// A question card's icon action.
type QuestionAction = Box<dyn Fn(&mut AudiencePage, &mut Context<AudiencePage>)>;

/// Where a failed action's message goes: each form shows its own.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ErrorAt {
    Page,
    Kairo,
    Slido(String),
    Connect,
    List,
    Poll(String),
    Settings,
}

/// The dialog over the page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    NewEngagement,
    Settings(SettingsSection),
    /// A poll by id, or a new one.
    Poll(Option<String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Screens,
    Collaboration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Composer {
    Reply,
    Edit,
}

struct PollDraft {
    id: Option<String>,
    title: Entity<InputState>,
    options: Vec<(Option<String>, Entity<InputState>)>,
    multiple: bool,
}

pub struct AudiencePage {
    backend: Arc<dyn AudienceBackend>,
    state: AudienceState,
    account: AccountView,
    style: ProgramQuestionStyle,
    initial_loading: bool,
    activity: Activity,
    chosen: Option<String>,
    busy: bool,
    errors: HashMap<String, String>,
    focus: FocusHandle,
    rail: Option<AnyView>,
    // Sidebar.
    adding_slido: bool,
    slido_code: Entity<InputState>,
    slido_skip: bool,
    confirm_end: bool,
    code_shown: bool,
    copied: Option<String>,
    copied_task: Option<Task<()>>,
    slido_open: HashSet<String>,
    // Engagements.
    items: Option<Vec<EngagementIndex>>,
    list_loading: bool,
    opening: Option<String>,
    deleting: Option<String>,
    last_code: Option<String>,
    // Questions.
    tab: QuestionTab,
    sort: AudienceQuestionSort,
    composer: Option<(String, Composer)>,
    reply_text: Entity<InputState>,
    reply_audience: ReplyAudience,
    edit_text: Entity<InputState>,
    questions: ListState,
    shown_ids: Vec<String>,
    // Dialogs.
    dialog: Option<Dialog>,
    /// Focus for the open dialog (Radix dialog behaviour).
    dialog_focus: kairo_ui_kit::components::ModalFocus,
    new_title: Entity<InputState>,
    new_starts: Entity<gpui_kit::component::date_picker::DatePickerState>,
    new_ends: Entity<gpui_kit::component::date_picker::DatePickerState>,
    new_settings: AudienceRoomSettings,
    pending: Option<&'static str>,
    slow: bool,
    slow_task: Option<Task<()>>,
    poll: Option<PollDraft>,
    poll_saving: bool,
    invite_email: Entity<InputState>,
    inviting: bool,
    menu_open: Option<String>,
    draft_style: ProgramQuestionStyle,
    saving_style: bool,
    preview_join: bool,
    headline: Entity<InputState>,
    sliders: HashMap<&'static str, Entity<SliderState>>,
    colors: HashMap<&'static str, Entity<ColorPickerState>>,
    qr_cache: Option<(String, Option<Arc<Image>>)>,
    /// A field to empty on the next frame (it needs the window).
    pending_clear: Option<Entity<InputState>>,
    _tick: Task<()>,
    _feed: Task<()>,
    _listening: Listening,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for AudiencePage {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn input(
    window: &mut Window,
    cx: &mut Context<AudiencePage>,
    placeholder: &str,
) -> Entity<InputState> {
    let placeholder = placeholder.to_owned();
    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
}

fn value(input: &Entity<InputState>, cx: &App) -> String {
    input.read(cx).value().to_string()
}

fn hex_hsla(value: &str) -> Option<Hsla> {
    let d = value.trim().strip_prefix('#')?;
    let full: String = match d.len() {
        6 => d.to_owned(),
        3 => d.chars().flat_map(|c| [c, c]).collect(),
        _ => return None,
    };
    if !full.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |i: usize| f32::from(u8::from_str_radix(&full[i..i + 2], 16).unwrap_or(0)) / 255.0;
    Some(
        gpui_kit::Rgba {
            r: ch(0),
            g: ch(2),
            b: ch(4),
            a: 1.0,
        }
        .into(),
    )
}

fn hsla_hex(c: Hsla) -> String {
    let c = c.to_rgb();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
}

impl AudiencePage {
    pub fn new(
        backend: Arc<dyn AudienceBackend>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = backend.snapshot();
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<AudienceFeed>();
        let sink: FeedSink = Arc::new(move |f| {
            let _ = tx.unbounded_send(f);
        });
        let listening = backend.listen(sink);
        let feed = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(f) = rx.next().await {
                if this.update(cx, |t, cx| t.on_feed(f, cx)).is_err() {
                    break;
                }
            }
        });
        // One clock: Slido "updated …" every 5 s and question ages each minute.
        let tick = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        let slido_code = input(window, cx, "#1234567");
        let new_title = input(window, cx, model::DEFAULT_TITLE);
        // Date pickers (owner, DECISIONS 113); Electron uses text boxes.
        let new_starts = cx.new(|cx| {
            gpui_kit::component::date_picker::DatePickerState::new(window, cx)
                .date_format("%Y-%m-%d")
        });
        let new_ends = cx.new(|cx| {
            gpui_kit::component::date_picker::DatePickerState::new(window, cx)
                .date_format("%Y-%m-%d")
        });
        let reply_text = input(window, cx, "Write a reply");
        let edit_text = input(window, cx, "");
        let invite_email = input(window, cx, "Email");
        let headline = input(window, cx, "Ask a question");
        let subscriptions = vec![
            cx.subscribe_in(
                &slido_code,
                window,
                |this, _, e: &InputEvent, _, cx| match e {
                    InputEvent::PressEnter { .. } => this.connect_slido(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe_in(&new_title, window, |this, _, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::PressEnter { .. }) {
                    this.submit_new(true, cx)
                }
            }),
            cx.subscribe_in(
                &new_starts,
                window,
                |_, _, _: &gpui_kit::component::date_picker::DatePickerEvent, _, cx| cx.notify(),
            ),
            cx.subscribe_in(
                &new_ends,
                window,
                |_, _, _: &gpui_kit::component::date_picker::DatePickerEvent, _, cx| cx.notify(),
            ),
            cx.subscribe_in(&reply_text, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(&edit_text, window, |_, _, _: &InputEvent, _, cx| {
                cx.notify()
            }),
            cx.subscribe_in(
                &invite_email,
                window,
                |this, _, e: &InputEvent, _, cx| match e {
                    InputEvent::PressEnter { .. } => this.invite(cx),
                    InputEvent::Change => cx.notify(),
                    _ => {}
                },
            ),
            cx.subscribe_in(
                &headline,
                window,
                |this, input, e: &InputEvent, _, cx| match e {
                    InputEvent::Focus => {
                        this.preview_join = true;
                        cx.notify();
                    }
                    InputEvent::Change => {
                        let v: String = input.read(cx).value().chars().take(80).collect();
                        this.draft_style.join_headline = v;
                        cx.notify();
                    }
                    _ => {}
                },
            ),
        ];
        let items = if snapshot.account.signed_in {
            model::read_cache(backend.read_ui(model::CACHE_KEY).as_deref())
        } else {
            None
        };
        let style = snapshot.style.clone();
        let mut page = Self {
            backend,
            last_code: snapshot.state.session.as_ref().map(|s| s.code.clone()),
            state: snapshot.state,
            account: snapshot.account,
            draft_style: style.clone(),
            style,
            initial_loading: true,
            activity: Activity::Qa,
            chosen: None,
            busy: false,
            errors: HashMap::new(),
            focus: cx.focus_handle(),
            rail: None,
            adding_slido: false,
            slido_code,
            slido_skip: false,
            confirm_end: false,
            code_shown: false,
            copied: None,
            copied_task: None,
            slido_open: HashSet::new(),
            items,
            list_loading: false,
            opening: None,
            deleting: None,
            tab: QuestionTab::Approved,
            sort: AudienceQuestionSort::Votes,
            composer: None,
            reply_text,
            reply_audience: ReplyAudience::Everyone,
            edit_text,
            questions: ListState::new(0, ListAlignment::Top, px(400.)),
            shown_ids: vec![],
            dialog: None,
            dialog_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            new_title,
            new_starts,
            new_ends,
            new_settings: AudienceRoomSettings::default(),
            pending: None,
            slow: false,
            slow_task: None,
            poll: None,
            poll_saving: false,
            invite_email,
            inviting: false,
            menu_open: None,
            saving_style: false,
            preview_join: false,
            headline,
            sliders: HashMap::new(),
            colors: HashMap::new(),
            qr_cache: None,
            pending_clear: None,
            _tick: tick,
            _feed: feed,
            _listening: listening,
            _subscriptions: subscriptions,
        };
        page.get_state(cx);
        page.refresh_list(cx);
        page
    }

    pub fn set_rail(&mut self, rail: Option<AnyView>, cx: &mut Context<Self>) {
        self.rail = rail;
        cx.notify();
    }

    // ─── Reading (tests use these) ─────────────────────────────────────────

    pub fn state(&self) -> &AudienceState {
        &self.state
    }
    pub fn active_source(&self) -> Option<String> {
        model::active_source(&self.state, self.chosen.as_deref())
    }
    pub fn activity(&self) -> Activity {
        if model::polls_available(self.active_source().as_deref()) {
            self.activity
        } else {
            Activity::Qa
        }
    }
    pub fn error_at(&self, key: &str) -> Option<&str> {
        self.errors.get(key).map(String::as_str)
    }
    pub fn items(&self) -> Option<&[EngagementIndex]> {
        self.items.as_deref()
    }
    pub fn dialog(&self) -> Option<&Dialog> {
        self.dialog.as_ref()
    }
    pub fn copied(&self) -> Option<&str> {
        self.copied.as_deref()
    }
    pub fn draft_style(&self) -> &ProgramQuestionStyle {
        &self.draft_style
    }
    pub fn busy(&self) -> bool {
        self.busy
    }
    pub fn inputs(&self) -> PageInputs {
        PageInputs {
            slido_code: self.slido_code.clone(),
            new_title: self.new_title.clone(),
            new_starts: self.new_starts.clone(),
            new_ends: self.new_ends.clone(),
            reply: self.reply_text.clone(),
            edit: self.edit_text.clone(),
            invite: self.invite_email.clone(),
            poll_title: self.poll.as_ref().map(|p| p.title.clone()),
            poll_options: self
                .poll
                .as_ref()
                .map(|p| p.options.iter().map(|(_, i)| i.clone()).collect())
                .unwrap_or_default(),
        }
    }

    // ─── The core ──────────────────────────────────────────────────────────

    fn on_feed(&mut self, feed: AudienceFeed, cx: &mut Context<Self>) {
        match feed {
            AudienceFeed::State(state) => {
                self.set_state(*state, cx);
                self.initial_loading = false;
            }
            AudienceFeed::Account(account) => {
                let was = self.account;
                self.account = account;
                if !account.signed_in {
                    self.backend.write_ui(model::CACHE_KEY, None);
                    self.items = None;
                    self.errors.remove("list");
                }
                if was != account {
                    self.refresh_list(cx);
                }
            }
            AudienceFeed::Style(style) => {
                let dirty = self.draft_style != self.style;
                self.style = *style;
                if !dirty {
                    self.draft_style = self.style.clone();
                }
            }
        }
        cx.notify();
    }

    fn set_state(&mut self, state: AudienceState, cx: &mut Context<Self>) {
        if state.live_id.as_deref() == Some("join") {
            self.code_shown = true;
        }
        let code = state.session.as_ref().map(|s| s.code.clone());
        self.state = state;
        if code != self.last_code {
            self.last_code = code;
            self.refresh_list(cx);
        }
        cx.notify();
    }

    fn key(at: &ErrorAt) -> String {
        match at {
            ErrorAt::Page => "page".into(),
            ErrorAt::Kairo => "kairo".into(),
            ErrorAt::Slido(id) => format!("slido:{id}"),
            ErrorAt::Connect => "connect".into(),
            ErrorAt::List => "list".into(),
            ErrorAt::Poll(id) => format!("poll:{id}"),
            ErrorAt::Settings => "settings".into(),
        }
    }

    fn get_state(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            AudienceCall::GetState,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                match r {
                    Ok(AudienceAnswer::State(s)) => t.set_state(*s, cx),
                    Ok(_) => {}
                    Err(e) => {
                        t.errors.insert("page".into(), e);
                    }
                }
                t.initial_loading = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// `act`: runs a call, shows its state, and sends a failure to `at`.
    fn act(
        &mut self,
        call: AudienceCall,
        at: ErrorAt,
        then: impl FnOnce(&mut Self, bool, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.errors.remove("page");
        self.errors.remove(&Self::key(&at));
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            call,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                t.busy = false;
                let ok = match r {
                    Ok(AudienceAnswer::State(s)) => {
                        t.set_state(*s, cx);
                        true
                    }
                    Ok(_) => true,
                    Err(e) => {
                        t.errors.insert(Self::key(&at), e);
                        false
                    }
                };
                then(t, ok, cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn simple(&mut self, call: AudienceCall, at: ErrorAt, cx: &mut Context<Self>) {
        self.act(call, at, |_, _, _| {}, cx);
    }

    // ─── Engagements ───────────────────────────────────────────────────────

    pub fn refresh_list(&mut self, cx: &mut Context<Self>) {
        if !self.account.signed_in {
            return;
        }
        if !self.account.online {
            self.list_loading = false;
            self.errors.insert("list".into(), model::OFFLINE.into());
            cx.notify();
            return;
        }
        self.list_loading = true;
        self.errors.remove("list");
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            AudienceCall::ListEngagements,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                t.list_loading = false;
                match r {
                    Ok(AudienceAnswer::Engagements(items)) => {
                        t.backend
                            .write_ui(model::CACHE_KEY, model::write_cache(Some(&items)));
                        t.items = Some(items);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        t.errors.insert("list".into(), e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_engagement(&mut self, code: &str, cx: &mut Context<Self>) {
        self.chosen = Some("kairo".into());
        if self.state.session.as_ref().is_some_and(|s| s.code == code) {
            cx.notify();
            return;
        }
        self.opening = Some(code.to_owned());
        self.act(
            AudienceCall::Resume(code.to_owned()),
            ErrorAt::List,
            |t, _, _| t.opening = None,
            cx,
        );
    }

    pub fn ask_delete(&mut self, code: Option<String>, cx: &mut Context<Self>) {
        self.deleting = code;
        cx.notify();
    }

    pub fn delete_engagement(&mut self, code: &str, cx: &mut Context<Self>) {
        self.act(
            AudienceCall::DeleteEngagement(code.to_owned()),
            ErrorAt::List,
            |t, ok, cx| {
                if ok {
                    t.deleting = None;
                    t.refresh_list(cx);
                    t.get_state(cx);
                }
            },
            cx,
        );
    }

    // ─── The Kairo card ────────────────────────────────────────────────────

    pub fn toggle_join(&mut self, cx: &mut Context<Self>) {
        let call = if self.state.live_id.as_deref() == Some("join") {
            AudienceCall::Clear
        } else {
            AudienceCall::ShowJoinCode
        };
        self.simple(call, ErrorAt::Kairo, cx);
    }

    pub fn end(&mut self, confirm: bool, cx: &mut Context<Self>) {
        if !confirm {
            self.confirm_end = true;
            cx.notify();
            return;
        }
        self.act(
            AudienceCall::Stop("kairo".into()),
            ErrorAt::Kairo,
            |t, _, cx| {
                t.confirm_end = false;
                t.refresh_list(cx);
            },
            cx,
        );
    }

    pub fn keep_going(&mut self, cx: &mut Context<Self>) {
        self.confirm_end = false;
        cx.notify();
    }

    pub fn reopen(&mut self, cx: &mut Context<Self>) {
        self.act(
            AudienceCall::Reopen,
            ErrorAt::Kairo,
            |t, ok, cx| {
                if ok {
                    t.refresh_list(cx)
                }
            },
            cx,
        );
    }

    /// `openAudiencePage`; a failure is ignored, as in Electron.
    pub fn open_audience_page(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            AudienceCall::OpenAudiencePage,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(AudienceAnswer::Url(url))) = rx.await {
                cx.update(|cx| cx.open_url(&url));
            }
        })
        .detach();
    }

    /// Copy-link buttons: the clipboard, then "copied" for 1.5 s.
    pub fn copy(&mut self, key: &str, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied = Some(key.to_owned());
        self.copied_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(model::COPIED_MS))
                .await;
            let _ = this.update(cx, |t, cx| {
                t.copied = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn download_qr(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.state.session.clone() else {
            return;
        };
        let Some(png) = model::qr_png(&session.qr_data_url) else {
            return;
        };
        self.backend.save_qr(
            png,
            model::qr_file_name(&session.code),
            Box::new(|r| {
                if let Err(error) = r {
                    tracing::error!(%error, "[Engage] QR code not saved");
                }
            }),
        );
        cx.notify();
    }

    fn qr_image(&mut self) -> Option<Arc<Image>> {
        let url = self.state.session.as_ref()?.qr_data_url.clone();
        if self.qr_cache.as_ref().map(|(u, _)| u) != Some(&url) {
            let image =
                model::qr_png(&url).map(|b| Arc::new(Image::from_bytes(ImageFormat::Png, b)));
            self.qr_cache = Some((url, image));
        }
        self.qr_cache.as_ref().and_then(|(_, i)| i.clone())
    }

    // ─── Slido and sample ──────────────────────────────────────────────────

    pub fn set_adding_slido(&mut self, on: bool, cx: &mut Context<Self>) {
        self.adding_slido = on;
        self.errors.remove("connect");
        cx.notify();
    }

    pub fn set_slido_skip(&mut self, on: bool, cx: &mut Context<Self>) {
        self.slido_skip = on;
        cx.notify();
    }

    pub fn connect_slido(&mut self, cx: &mut Context<Self>) {
        let code = value(&self.slido_code, cx);
        if code.trim().is_empty() || self.busy {
            return;
        }
        let options = AudienceSourceOptions {
            code: Some(code),
            skip_review: Some(self.slido_skip),
            ..Default::default()
        };
        self.act(
            AudienceCall::Start {
                kind: "slido".into(),
                options,
            },
            ErrorAt::Connect,
            |t, ok, cx| {
                if ok {
                    t.adding_slido = false;
                    t.chosen = t
                        .state
                        .sources
                        .iter()
                        .rev()
                        .find(|s| model::is_slido(s) && s.state != AudienceSourceState::Stopped)
                        .map(|s| s.id.clone())
                        .or(t.chosen.take());
                    cx.notify();
                }
            },
            cx,
        );
    }

    /// The Slido card's switches: stop and start again with the new choice.
    pub fn restart_slido(
        &mut self,
        id: &str,
        moderator: Option<bool>,
        skip: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let Some(source) = self.state.sources.iter().find(|s| s.id == id).cloned() else {
            return;
        };
        let options = AudienceSourceOptions {
            code: Some(model::slido_restart_code(&source)),
            moderator: moderator.or(source.moderator),
            skip_review: skip.or(source.skip_review),
            ..Default::default()
        };
        let at = ErrorAt::Slido(id.to_owned());
        let id = id.to_owned();
        self.act(
            AudienceCall::Stop(id),
            at.clone(),
            move |t, ok, cx| {
                if ok {
                    t.simple(
                        AudienceCall::Start {
                            kind: "slido".into(),
                            options,
                        },
                        at,
                        cx,
                    );
                }
            },
            cx,
        );
    }

    pub fn select_source(&mut self, id: &str, cx: &mut Context<Self>) {
        self.chosen = Some(id.to_owned());
        cx.notify();
    }

    pub fn stop_source(&mut self, id: &str, cx: &mut Context<Self>) {
        let at = if id == "sample" {
            ErrorAt::Page
        } else {
            ErrorAt::Slido(id.to_owned())
        };
        self.simple(AudienceCall::Stop(id.to_owned()), at, cx);
    }

    pub fn open_source(&mut self, id: &str, cx: &mut Context<Self>) {
        self.simple(
            AudienceCall::Open(id.to_owned()),
            ErrorAt::Slido(id.to_owned()),
            cx,
        );
    }

    pub fn load_sample(&mut self, cx: &mut Context<Self>) {
        self.act(
            AudienceCall::Start {
                kind: "sample".into(),
                options: Default::default(),
            },
            ErrorAt::Page,
            |t, ok, _| {
                if ok {
                    t.chosen = Some("sample".into())
                }
            },
            cx,
        );
    }

    pub fn toggle_slido_settings(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.slido_open.remove(id) {
            self.slido_open.insert(id.to_owned());
        }
        cx.notify();
    }

    // ─── Questions ─────────────────────────────────────────────────────────

    pub fn set_activity(&mut self, activity: Activity, cx: &mut Context<Self>) {
        self.activity = activity;
        cx.notify();
    }

    pub fn set_tab(&mut self, tab: QuestionTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    pub fn set_sort(&mut self, sort: AudienceQuestionSort, cx: &mut Context<Self>) {
        self.sort = sort;
        cx.notify();
    }

    /// The questions the list shows now.
    pub fn visible_questions(&self) -> Vec<AudienceQuestion> {
        let source = self.active_source();
        let from: Vec<AudienceQuestion> = self
            .state
            .questions
            .iter()
            .filter(|q| source.as_ref().is_none_or(|s| q.source_id == *s))
            .filter(|q| self.tab.matches(q.state))
            .cloned()
            .collect();
        sort_audience_questions(&from, self.sort)
    }

    pub fn tab_count(&self, tab: QuestionTab) -> usize {
        let source = self.active_source();
        self.state
            .questions
            .iter()
            .filter(|q| source.as_ref().is_none_or(|s| q.source_id == *s))
            .filter(|q| tab.matches(q.state))
            .count()
    }

    pub fn moderate(&mut self, id: &str, state: AudienceQuestionState, cx: &mut Context<Self>) {
        self.simple(
            AudienceCall::Moderate {
                id: id.to_owned(),
                state,
            },
            ErrorAt::Page,
            cx,
        );
    }

    pub fn show_question(&mut self, id: &str, cx: &mut Context<Self>) {
        self.simple(AudienceCall::Show(id.to_owned()), ErrorAt::Page, cx);
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.simple(AudienceCall::Clear, ErrorAt::Page, cx);
    }

    pub fn open_composer(
        &mut self,
        id: &str,
        edit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if edit && let Some(q) = self.state.questions.iter().find(|q| q.id == id) {
            let text = q.text.clone();
            self.edit_text
                .update(cx, |i, cx| i.set_value(text, window, cx));
        }
        self.composer = Some((
            id.to_owned(),
            if edit {
                Composer::Edit
            } else {
                Composer::Reply
            },
        ));
        cx.notify();
    }

    pub fn close_composer(&mut self, cx: &mut Context<Self>) {
        self.composer = None;
        cx.notify();
    }

    pub fn set_reply_audience(&mut self, a: ReplyAudience, cx: &mut Context<Self>) {
        self.reply_audience = a;
        cx.notify();
    }

    pub fn send_reply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((id, Composer::Reply)) = self.composer.clone() else {
            return;
        };
        let text = value(&self.reply_text, cx).trim().to_owned();
        if text.is_empty() {
            return;
        }
        let reply = self.reply_text.clone();
        let _ = window;
        self.act(
            AudienceCall::Command(EngageCommand::Reply {
                question_id: id,
                text,
                audience: self.reply_audience,
            }),
            ErrorAt::Page,
            move |t, ok, cx| {
                if ok {
                    t.composer = None;
                    t.pending_clear = Some(reply);
                    cx.notify();
                }
            },
            cx,
        );
    }

    pub fn save_edit(&mut self, cx: &mut Context<Self>) {
        let Some((id, Composer::Edit)) = self.composer.clone() else {
            return;
        };
        let text = value(&self.edit_text, cx).trim().to_owned();
        let same = self
            .state
            .questions
            .iter()
            .any(|q| q.id == id && q.text == text);
        if text.is_empty() || same {
            return;
        }
        self.act(
            AudienceCall::Command(EngageCommand::Edit { id, text }),
            ErrorAt::Page,
            |t, ok, _| {
                if ok {
                    t.composer = None
                }
            },
            cx,
        );
    }

    // ─── Polls ─────────────────────────────────────────────────────────────

    pub fn open_poll(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let existing = id.as_ref().and_then(|id| {
            self.state
                .engage
                .as_ref()?
                .polls
                .iter()
                .find(|p| p.poll.id == *id)
                .cloned()
        });
        let title = input(window, cx, "Which song should we sing next week?");
        let mut options = vec![];
        match &existing {
            Some(p) => {
                let t = p.poll.title.clone();
                title.update(cx, |i, cx| i.set_value(t, window, cx));
                for o in &p.poll.options {
                    let field = input(window, cx, "");
                    let v = o.text.clone();
                    field.update(cx, |i, cx| i.set_value(v, window, cx));
                    options.push((Some(o.id.clone()), field));
                }
            }
            None => {
                for _ in 0..2 {
                    options.push((None, input(window, cx, "")));
                }
            }
        }
        for (_, f) in &options {
            self._subscriptions
                .push(cx.subscribe(f, |_, _, _: &InputEvent, cx| cx.notify()));
        }
        self._subscriptions
            .push(cx.subscribe(&title, |_, _, _: &InputEvent, cx| cx.notify()));
        self.poll = Some(PollDraft {
            id: existing.as_ref().map(|p| p.poll.id.clone()),
            title,
            options,
            multiple: existing.map(|p| p.poll.multiple).unwrap_or(false),
        });
        self.errors.remove("poll-editor");
        self.dialog = Some(Dialog::Poll(id));
        cx.notify();
    }

    pub fn add_poll_option(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let field = input(window, cx, "");
        self._subscriptions
            .push(cx.subscribe(&field, |_, _, _: &InputEvent, cx| cx.notify()));
        if let Some(p) = &mut self.poll
            && p.options.len() < model::MAX_POLL_OPTIONS
        {
            p.options.push((None, field));
        }
        cx.notify();
    }

    pub fn remove_poll_option(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.poll
            && p.options.len() > 2
            && i < p.options.len()
        {
            p.options.remove(i);
        }
        cx.notify();
    }

    pub fn set_poll_multiple(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(p) = &mut self.poll {
            p.multiple = on;
        }
        cx.notify();
    }

    fn poll_ready(&self, cx: &App) -> Option<serde_json::Value> {
        let p = self.poll.as_ref()?;
        let title = value(&p.title, cx).trim().to_owned();
        let filled: Vec<serde_json::Value> = p
            .options
            .iter()
            .filter_map(|(id, f)| {
                let t = value(f, cx).trim().to_owned();
                (!t.is_empty()).then(|| match id {
                    Some(id) => serde_json::json!({ "id": id, "text": t }),
                    None => serde_json::json!({ "text": t }),
                })
            })
            .collect();
        if title.is_empty() || filled.len() < 2 {
            return None;
        }
        let mut poll =
            serde_json::json!({ "title": title, "options": filled, "multiple": p.multiple });
        if let Some(id) = &p.id {
            poll["id"] = serde_json::json!(id);
        }
        Some(poll)
    }

    pub fn save_poll(&mut self, cx: &mut Context<Self>) {
        let Some(poll) = self.poll_ready(cx) else {
            return;
        };
        if self.poll_saving {
            return;
        }
        self.poll_saving = true;
        self.errors.remove("poll-editor");
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            AudienceCall::Command(EngageCommand::PollSave { poll }),
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                t.poll_saving = false;
                match r {
                    Ok(AudienceAnswer::State(s)) => {
                        t.set_state(*s, cx);
                        t.close_dialog(cx);
                    }
                    Ok(_) => t.close_dialog(cx),
                    Err(e) => {
                        t.errors.insert("poll-editor".into(), e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn set_poll(
        &mut self,
        poll: &PollView,
        state: Option<OpenOrClosed>,
        show_results: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        let state = state.unwrap_or(if poll.poll.state == EngagementStatus::Open {
            OpenOrClosed::Open
        } else {
            OpenOrClosed::Closed
        });
        self.simple(
            AudienceCall::Command(EngageCommand::PollState {
                id: poll.poll.id.clone(),
                state,
                show_results: show_results.unwrap_or(poll.poll.show_results),
            }),
            ErrorAt::Poll(poll.poll.id.clone()),
            cx,
        );
    }

    pub fn toggle_poll_screen(&mut self, id: &str, cx: &mut Context<Self>) {
        let live = self.state.live_poll_id.clone().flatten().as_deref() == Some(id);
        let call = if live {
            AudienceCall::ClearPoll
        } else {
            AudienceCall::ShowPoll(id.to_owned())
        };
        self.simple(call, ErrorAt::Poll(id.to_owned()), cx);
    }

    // ─── New engagement ────────────────────────────────────────────────────

    pub fn open_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.new_title
            .update(cx, |i, cx| i.set_value("", window, cx));
        for f in [&self.new_starts, &self.new_ends] {
            f.update(cx, |d, cx| {
                d.set_date(
                    gpui_kit::component::calendar::Date::Single(None),
                    window,
                    cx,
                )
            });
        }
        self.new_settings = AudienceRoomSettings::default();
        self.errors.remove("new");
        self.dialog = Some(Dialog::NewEngagement);
        cx.notify();
    }

    pub fn set_new_setting(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        self.new_settings = model::set_room(&self.new_settings, key, on);
        cx.notify();
    }

    fn new_dates(&self, cx: &App) -> (Option<f64>, Option<f64>) {
        (
            model::day_to_time(&day(&self.new_starts, cx), false),
            model::day_to_time(&day(&self.new_ends, cx), true),
        )
    }

    /// Start now (`start`) or Save for later.
    pub fn submit_new(&mut self, start: bool, cx: &mut Context<Self>) {
        let (starts, ends) = self.new_dates(cx);
        if model::dates_invalid(starts, ends) || self.pending.is_some() {
            return;
        }
        let typed = value(&self.new_title, cx);
        let name = if typed.trim().is_empty() {
            model::DEFAULT_TITLE.to_owned()
        } else {
            typed.trim().to_owned()
        };
        self.pending = Some(if start { "start" } else { "save" });
        self.errors.remove("new");
        self.slow = false;
        self.slow_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(model::SLOW_MS))
                .await;
            let _ = this.update(cx, |t, cx| {
                if t.pending.is_some() {
                    t.slow = true;
                    cx.notify();
                }
            });
        }));
        let call = if start {
            AudienceCall::Start {
                kind: "kairo".into(),
                options: AudienceSourceOptions {
                    title: Some(name),
                    settings: Some(self.new_settings.clone()),
                    starts_at: Some(starts),
                    ends_at: Some(ends),
                    ..Default::default()
                },
            }
        } else {
            AudienceCall::CreateEngagement(EngagementInput {
                title: name,
                starts_at: Some(starts),
                ends_at: Some(ends),
                status: Some(DraftOrOpen::Draft),
                settings: Some(self.new_settings.clone()),
            })
        };
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.call(
            call,
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                t.pending = None;
                t.slow = false;
                t.slow_task = None;
                match r {
                    Ok(answer) => {
                        if let AudienceAnswer::State(s) = answer {
                            t.set_state(*s, cx);
                        }
                        if start {
                            t.chosen = Some("kairo".into());
                        }
                        t.close_dialog(cx);
                        t.refresh_list(cx);
                    }
                    Err(e) => {
                        t.errors.insert("new".into(), e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ─── Engagement settings ───────────────────────────────────────────────

    pub fn open_settings(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.errors.remove("settings");
        self.menu_open = None;
        self.draft_style = self.style.clone();
        let h = self.draft_style.join_headline.clone();
        self.headline.update(cx, |i, cx| i.set_value(h, window, cx));
        self.sliders.clear();
        self.colors.clear();
        self.dialog = Some(Dialog::Settings(section));
        cx.notify();
    }

    pub fn set_section(&mut self, section: SettingsSection, cx: &mut Context<Self>) {
        self.errors.remove("settings");
        self.dialog = Some(Dialog::Settings(section));
        cx.notify();
    }

    pub fn set_room_setting(&mut self, key: &str, on: bool, cx: &mut Context<Self>) {
        let Some(session) = &self.state.session else {
            return;
        };
        let next = model::set_room(&session.settings, key, on);
        self.simple(AudienceCall::SetRoomSettings(next), ErrorAt::Settings, cx);
    }

    pub fn invite(&mut self, cx: &mut Context<Self>) {
        let email = value(&self.invite_email, cx).trim().to_owned();
        if email.is_empty() || self.busy || self.inviting {
            return;
        }
        self.inviting = true;
        let field = self.invite_email.clone();
        self.act(
            AudienceCall::Invite {
                email,
                name: String::new(),
            },
            ErrorAt::Settings,
            move |t, ok, cx| {
                t.inviting = false;
                if ok {
                    t.pending_clear = Some(field);
                    cx.notify();
                }
            },
            cx,
        );
    }

    pub fn toggle_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        self.menu_open = if self.menu_open.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_owned())
        };
        cx.notify();
    }

    pub fn revoke(&mut self, id: &str, cx: &mut Context<Self>) {
        self.menu_open = None;
        self.simple(AudienceCall::Revoke(id.to_owned()), ErrorAt::Settings, cx);
    }

    pub fn style_dirty(&self) -> bool {
        self.draft_style != self.style
    }

    pub fn patch_style(
        &mut self,
        f: impl FnOnce(&mut ProgramQuestionStyle),
        cx: &mut Context<Self>,
    ) {
        f(&mut self.draft_style);
        cx.notify();
    }

    pub fn reset_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.draft_style = self.style.clone();
        let h = self.draft_style.join_headline.clone();
        self.headline.update(cx, |i, cx| i.set_value(h, window, cx));
        self.sliders.clear();
        self.colors.clear();
        cx.notify();
    }

    pub fn save_style(&mut self, close: bool, cx: &mut Context<Self>) {
        self.saving_style = true;
        self.errors.remove("settings");
        let style = self.draft_style.clone();
        let (tx, rx) = futures_channel::oneshot::channel();
        self.backend.save_style(
            style.clone(),
            Box::new(move |r| {
                let _ = tx.send(r);
            }),
        );
        cx.spawn(async move |this, cx| {
            let r = rx
                .await
                .unwrap_or_else(|_| Err("The request was dropped.".into()));
            let _ = this.update(cx, |t, cx| {
                t.saving_style = false;
                match r {
                    Ok(()) => {
                        t.style = style;
                    }
                    Err(e) => {
                        t.errors.insert("settings".into(), e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        if close {
            self.dialog = None;
        }
        cx.notify();
    }

    /// Done: a changed look is saved as the dialog closes.
    pub fn settings_done(&mut self, cx: &mut Context<Self>) {
        if self.style_dirty() {
            self.save_style(true, cx);
        } else {
            self.close_dialog(cx);
        }
    }

    pub fn close_dialog(&mut self, cx: &mut Context<Self>) {
        // A dialog waiting on a start or save stays.
        if self.pending.is_some() || self.poll_saving {
            return;
        }
        if matches!(self.dialog, Some(Dialog::Poll(_))) {
            self.poll = None;
        }
        self.dialog = None;
        cx.notify();
    }
}

/// The page's text fields, for tests.
pub struct PageInputs {
    pub slido_code: Entity<InputState>,
    pub new_title: Entity<InputState>,
    pub new_starts: Entity<gpui_kit::component::date_picker::DatePickerState>,
    pub new_ends: Entity<gpui_kit::component::date_picker::DatePickerState>,
    pub reply: Entity<InputState>,
    pub edit: Entity<InputState>,
    pub invite: Entity<InputState>,
    pub poll_title: Option<Entity<InputState>>,
    pub poll_options: Vec<Entity<InputState>>,
}

// ─── Drawing ───────────────────────────────────────────────────────────────

fn heading(label: &'static str, cx: &App) -> gpui_kit::Div {
    h_flex()
        .h_7()
        .justify_between()
        .px_1()
        .text_size(text::LABEL.size)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.kairo().text(TextStep::T500))
        .child(label)
}

fn card(
    id: impl Into<SharedString>,
    cx: &App,
) -> gpui_kit::base::ObservedElement<gpui_kit::Stateful<gpui_kit::Div>> {
    v_flex()
        .id(id.into())
        .test_support()
        .rounded(radius::lg())
        .bg(cx.kairo().surface_tertiary)
        .p_3()
}

fn small(t: impl Into<SharedString>, cx: &App) -> gpui_kit::Div {
    div()
        .text_size(text::LABEL.size)
        .text_color(cx.kairo().text(TextStep::T400))
        .child(t.into())
}

fn alert(id: &'static str, t: &str, cx: &App) -> AnyElement {
    div()
        .id(id)
        .test_support()
        .role(Role::Alert)
        .mt_2()
        .text_size(text::LABEL.size)
        .text_color(cx.kairo().danger_text)
        .child(t.to_owned())
        .into_any_element()
}

fn beta(cx: &App) -> AnyElement {
    div()
        .ml_1p5()
        .rounded_full()
        .border_1()
        .border_color(cx.kairo().surface_border)
        .px_1p5()
        .text_size(text::MICRO.size)
        .text_color(cx.kairo().text(TextStep::T400))
        .child("Beta")
        .into_any_element()
}

fn dot(state: AudienceSourceState, cx: &App) -> AnyElement {
    let k = *cx.kairo();
    div()
        .size_1p5()
        .flex_none()
        .rounded_full()
        .bg(match state {
            AudienceSourceState::Live => k.success_text,
            AudienceSourceState::Error => k.danger_text,
            _ => k.text(TextStep::T500),
        })
        .into_any_element()
}

/// `Steps`: done, working or still to come.
fn steps(rows: &[(&'static str, i8)], cx: &App) -> AnyElement {
    let k = *cx.kairo();
    v_flex()
        .id(ids::SLIDO_STEPS)
        .test_support()
        .mt_3()
        .gap_1p5()
        .text_size(text::XS.size)
        .children(rows.iter().map(|(label, s)| {
            h_flex()
                .gap_2()
                .text_color(if *s < 0 {
                    k.text(TextStep::T600)
                } else {
                    k.text(TextStep::T300)
                })
                .child(
                    div()
                        .size_4()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(match s {
                            1 => Icon::new(KairoIcon::Check)
                                .size_3()
                                .text_color(k.success_text)
                                .into_any_element(),
                            0 => Icon::new(KairoIcon::Loader2).size_3().into_any_element(),
                            _ => div()
                                .size_1()
                                .rounded_full()
                                .bg(k.text(TextStep::T600))
                                .into_any_element(),
                        }),
                )
                .child(*label)
        }))
        .into_any_element()
}

fn switch_row(
    id: impl Into<SharedString>,
    label: &str,
    hint: &str,
    on: bool,
    disabled: bool,
    change: impl Fn(&bool, &mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let id: SharedString = id.into();
    h_flex()
        .justify_between()
        .gap_3()
        .px_3()
        .py_2()
        .child(
            v_flex()
                .min_w_0()
                .child(div().text_size(text::XS.size).child(label.to_owned()))
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(cx.kairo().text(TextStep::T500))
                        .child(hint.to_owned()),
                ),
        )
        .child(
            Switch::new(id)
                .small()
                .checked(on)
                .disabled(disabled)
                .accessibility_label(label.to_owned())
                .on_change(change),
        )
        .into_any_element()
}

impl AudiencePage {
    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let kairo = self
            .state
            .sources
            .iter()
            .find(|s| s.id == "kairo" && s.state != AudienceSourceState::Stopped)
            .cloned();
        let slido: Vec<_> = self
            .state
            .sources
            .iter()
            .filter(|s| model::is_slido(s) && s.state != AudienceSourceState::Stopped)
            .cloned()
            .collect();
        let sample = self
            .state
            .sources
            .iter()
            .find(|s| s.id == "sample" && s.state != AudienceSourceState::Stopped)
            .cloned();
        let running = kairo.is_some() as usize + slido.len() + sample.is_some() as usize;
        let active = self.active_source();
        let kairo_card = kairo
            .map(|s| self.render_kairo(&s, running > 1, active.as_deref() == Some("kairo"), cx));
        let engagements = self.render_engagements(
            kairo_card,
            active.is_none() || active.as_deref() == Some("kairo"),
            cx,
        );
        let slido_cards: Vec<AnyElement> = slido
            .iter()
            .map(|s| {
                let n = self
                    .state
                    .questions
                    .iter()
                    .filter(|q| q.source_id == s.id)
                    .count();
                self.render_slido(
                    s,
                    n,
                    running > 1,
                    active.as_deref() == Some(s.id.as_str()),
                    cx,
                )
            })
            .collect();
        let has_slido = !slido.is_empty();
        let integrations = v_flex()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(k.surface_line)
            .child(
                heading("INTEGRATIONS", cx).when(has_slido && !self.adding_slido, |d| {
                    d.child(
                        Button::new("engage-add-slido")
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Plus))
                            .accessibility_label("Connect Slido event")
                            .on_click(cx.listener(|t, _, _, cx| t.set_adding_slido(true, cx))),
                    )
                }),
            )
            .children(slido_cards)
            .child(if self.adding_slido {
                self.render_connect(cx)
            } else if !has_slido {
                h_flex()
                    .id(ids::CONNECT_SLIDO)
                    .test_support()
                    .role(Role::Button)
                    .aria_label("Connect Slido event")
                    .gap_2p5()
                    .px_2()
                    .py_1p5()
                    .rounded(radius::md())
                    .cursor_pointer()
                    .hover(|s| s.bg(k.surface_menu_hover))
                    .on_click(cx.listener(|t, _, _, cx| t.set_adding_slido(true, cx)))
                    .child(
                        div()
                            .size_6()
                            .flex_none()
                            .rounded(radius::sm())
                            .bg(k.surface_tertiary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(KairoIcon::MessageSquare).size_3()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Connect Slido event")
                                    .child(beta(cx)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child("Import questions from a Slido Q&A"),
                            ),
                    )
                    .child(Icon::new(KairoIcon::Plus).size_3())
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .children(sample.map(|s| {
                let selected = running > 1 && active.as_deref() == Some(s.id.as_str());
                h_flex()
                    .id(ids::SAMPLE)
                    .test_support()
                    .justify_between()
                    .rounded(radius::lg())
                    .bg(k.surface_tertiary)
                    .p_3()
                    .cursor_pointer()
                    .when(selected, |d| {
                        d.border_1().border_color(k.white.opacity(0.15))
                    })
                    .on_click(cx.listener(|t, _, _, cx| t.select_source("sample", cx)))
                    .child(div().text_size(text::SM.size).child("Sample questions"))
                    .child(
                        Button::new(ids::SAMPLE_REMOVE)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::X))
                            .accessibility_label("Remove sample questions")
                            .disabled(self.busy)
                            .on_click(cx.listener(|t, _, _, cx| {
                                cx.stop_propagation();
                                t.stop_source("sample", cx)
                            })),
                    )
            }));
        v_flex()
            .id("engage-sidebar")
            .flex_none()
            .w(px(
                kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx),
            ))
            .h_full()
            .min_h_0()
            .overflow_y_scroll()
            .gap_5()
            .p_3()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(engagements)
            .child(integrations)
            .when(running == 0 && self.state.sample_available, |d| {
                d.child(
                    Button::new(ids::LOAD_SAMPLE)
                        .cursor_pointer()
                        .ghost()
                        .small()
                        .icon(Icon::new(KairoIcon::Plus))
                        .label("Load sample questions")
                        .disabled(self.busy)
                        .on_click(cx.listener(|t, _, _, cx| t.load_sample(cx))),
                )
            })
            .into_any_element()
    }

    fn render_engagements(
        &mut self,
        active_card: Option<AnyElement>,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let current = self.state.session.as_ref().map(|s| s.code.clone());
        let signed_in = self.account.signed_in;
        let error = self.errors.get("list").cloned();
        let current_in_list = current.as_ref().is_some_and(|c| {
            self.items
                .as_ref()
                .is_some_and(|i| i.iter().any(|x| x.code == *c))
        });
        let has = active_card.is_some() || self.items.as_ref().is_some_and(|i| !i.is_empty());
        let mut active_card = active_card;
        let mut col = v_flex().gap_2().child(
            heading("ENGAGEMENTS", cx).child(
                h_flex()
                    .gap_0p5()
                    .child(
                        Button::new(ids::RELOAD)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::RefreshCw))
                            .accessibility_label("Reload")
                            .tooltip("Reload")
                            .disabled(!signed_in || self.list_loading)
                            .on_click(cx.listener(|t, _, _, cx| t.refresh_list(cx))),
                    )
                    .child(
                        Button::new(ids::NEW_ENGAGEMENT)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Plus))
                            .accessibility_label("New engagement")
                            .tooltip("New engagement")
                            .on_click(cx.listener(|t, _, w, cx| t.open_new(w, cx))),
                    ),
            ),
        );
        if !current_in_list && let Some(c) = active_card.take() {
            col = col.child(c);
        }
        let new_button = |id: &'static str, cx: &mut Context<Self>| {
            Button::new(id)
                .cursor_pointer()
                .primary()
                .small()
                .w_full()
                .mt_2p5()
                .icon(Icon::new(KairoIcon::Plus))
                .label("New engagement")
                .on_click(cx.listener(|t, _, w, cx| t.open_new(w, cx)))
        };
        if !signed_in {
            col = col.child(
                card(ids::SIGNED_OUT, cx)
                    .child(div().text_size(text::XS.size).font_weight(FontWeight::MEDIUM).child("Host live Q&A and polls"))
                    .child(small("People join with a code to ask questions, vote, and answer polls. Sign in to Kairo to see your saved engagements.", cx).mt_1())
                    .child(new_button("engage-new-signed-out", cx)),
            );
        }
        if signed_in && self.items.is_none() && error.is_none() {
            col = col.child(
                v_flex()
                    .id(ids::LIST_LOADING)
                    .test_support()
                    .role(Role::Status)
                    .aria_label("Loading engagements")
                    .gap_1p5()
                    .py_1()
                    .children((0..2).map(|_| {
                        div()
                            .h_8()
                            .rounded(radius::md())
                            .bg(k.surface_tertiary)
                            .opacity(0.6)
                    })),
            );
        }
        if signed_in && let Some(e) = &error {
            col = col.child(
                div()
                    .id(ids::LIST_ERROR)
                    .test_support()
                    .role(Role::Alert)
                    .px_1()
                    .py_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(model::list_error(e)),
            );
        }
        if signed_in && self.items.is_some() && !has {
            col = col.child(
                card(ids::FIRST, cx)
                    .child(div().text_size(text::XS.size).font_weight(FontWeight::MEDIUM).child("Start your first engagement"))
                    .child(small("People join with a code or QR to ask questions, upvote, and answer live polls.", cx).mt_1())
                    .child(new_button("engage-new-first", cx)),
            );
        }
        let items = self.items.clone().unwrap_or_default();
        for item in items {
            let is_current = current.as_deref() == Some(item.code.as_str());
            let opening = self.opening.as_deref() == Some(item.code.as_str());
            let code = item.code.clone();
            let row: AnyElement = if self.deleting.as_deref() == Some(item.code.as_str()) {
                let (a, b) = (code.clone(), code.clone());
                v_flex()
                    .px_2()
                    .py_1p5()
                    .child(div().text_size(text::XS.size).child(format!(
                        "Delete \u{201c}{}\u{201d} and its questions?",
                        item.name
                    )))
                    .child(
                        h_flex()
                            .mt_1p5()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(ids::delete_cancel(&a)))
                                    .cursor_pointer()
                                    .small()
                                    .flex_1()
                                    .label("Cancel")
                                    .on_click(cx.listener(|t, _, _, cx| t.ask_delete(None, cx))),
                            )
                            .child(
                                Button::new(SharedString::from(ids::delete_confirm(&b)))
                                    .cursor_pointer()
                                    .danger()
                                    .small()
                                    .flex_1()
                                    .label("Delete")
                                    .disabled(self.busy)
                                    .on_click(
                                        cx.listener(move |t, _, _, cx| t.delete_engagement(&b, cx)),
                                    ),
                            ),
                    )
                    .into_any_element()
            } else {
                let when = model::dates(&item);
                let open_code = code.clone();
                let del_code = code.clone();
                let disabled = self.busy || (is_current && selected);
                h_flex()
                    .group("engage-item")
                    .rounded(radius::md())
                    .when(is_current && selected, |d| d.bg(k.surface_tertiary))
                    .when(!(is_current && selected), |d| {
                        d.hover(|s| s.bg(k.surface_menu_hover))
                    })
                    .child(
                        h_flex()
                            .id(SharedString::from(ids::engagement(&item.code)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(item.name.clone())
                            .flex_1()
                            .min_w_0()
                            .items_start()
                            .gap_2()
                            .px_2()
                            .py_1p5()
                            .when(!disabled, |d| {
                                d.cursor_pointer().on_click(
                                    cx.listener(move |t, _, _, cx| {
                                        t.open_engagement(&open_code, cx)
                                    }),
                                )
                            })
                            .child(if opening {
                                Icon::new(KairoIcon::Loader2)
                                    .size_3()
                                    .mt_1()
                                    .into_any_element()
                            } else {
                                div()
                                    .mt(px(7.))
                                    .size_1p5()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(match item.status {
                                        EngagementStatus::Open => k.success_text,
                                        EngagementStatus::Draft => k.text(TextStep::T500),
                                        EngagementStatus::Closed => k.text(TextStep::T600),
                                    })
                                    .into_any_element()
                            })
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .child(item.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(format!(
                                                "{} · {}{}",
                                                item.code,
                                                if opening {
                                                    "Opening…"
                                                } else {
                                                    model::engagement_status(item.status)
                                                },
                                                match (&when, opening) {
                                                    (Some(w), false) => format!(" · {w}"),
                                                    _ => String::new(),
                                                }
                                            )),
                                    ),
                            ),
                    )
                    .when(!is_current, |d| {
                        d.child(
                            div()
                                .mr_1()
                                .invisible()
                                .group_hover("engage-item", |s| s.visible())
                                .child(
                                    Button::new(SharedString::from(ids::delete(&item.code)))
                                        .cursor_pointer()
                                        .ghost()
                                        .xsmall()
                                        .icon(Icon::new(KairoIcon::Trash2))
                                        .accessibility_label(format!("Delete {}", item.name))
                                        .on_click(cx.listener(move |t, _, _, cx| {
                                            t.ask_delete(Some(del_code.clone()), cx)
                                        })),
                                ),
                        )
                    })
                    .into_any_element()
            };
            col = col.child(row);
            if is_current && let Some(c) = active_card.take() {
                col = col.child(c);
            }
        }
        col.into_any_element()
    }

    fn render_kairo(
        &mut self,
        source: &kairo_domain::audience::AudienceSourceStatus,
        show_selection: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let Some(session) = self.state.session.clone() else {
            let reserving =
                source.step == Some(kairo_domain::audience::AudienceSourceStep::Reserving);
            return card(ids::KAIRO_STARTING, cx)
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Starting your Q&A"),
                )
                .child(steps(
                    &[
                        ("Getting a join code", if reserving { 0 } else { 1 }),
                        ("Opening the room", if reserving { -1 } else { 0 }),
                    ],
                    cx,
                ))
                .into_any_element();
        };
        let closed = self
            .state
            .engage
            .as_ref()
            .is_some_and(|e| e.engagement.status == EngagementStatus::Closed);
        let status = model::kairo_status(&self.state, source);
        let join_on = self.state.live_id.as_deref() == Some("join");
        let qr = self.qr_image();
        let copied = self.copied.as_deref() == Some("join");
        let error = self.errors.get("kairo").cloned();
        let url = session.url.clone();
        let actions: AnyElement = if closed {
            h_flex()
                .mt_3()
                .gap_1p5()
                .child(
                    Button::new(ids::REOPEN)
                        .cursor_pointer()
                        .primary()
                        .small()
                        .flex_1()
                        .icon(Icon::new(KairoIcon::RotateCcw))
                        .label("Reopen engagement")
                        .disabled(self.busy)
                        .on_click(cx.listener(|t, _, _, cx| t.reopen(cx))),
                )
                .child(
                    Button::new(ids::SETTINGS)
                        .cursor_pointer()
                        .small()
                        .icon(Icon::new(KairoIcon::Settings))
                        .accessibility_label("Settings and collaboration")
                        .tooltip("Settings & collaboration")
                        .on_click(cx.listener(|t, _, w, cx| {
                            t.open_settings(SettingsSection::General, w, cx)
                        })),
                )
                .into_any_element()
        } else if self.confirm_end {
            v_flex()
                .mt_3()
                .child(
                    div()
                        .text_size(text::XS.size)
                        .child("End this engagement for everyone?"),
                )
                .child(
                    h_flex()
                        .mt_2()
                        .gap_2()
                        .child(
                            Button::new(ids::END_KEEP)
                                .cursor_pointer()
                                .small()
                                .flex_1()
                                .label("Keep going")
                                .on_click(cx.listener(|t, _, _, cx| t.keep_going(cx))),
                        )
                        .child(
                            Button::new(ids::END_CONFIRM)
                                .cursor_pointer()
                                .danger()
                                .small()
                                .flex_1()
                                .label("End")
                                .disabled(self.busy)
                                .on_click(cx.listener(|t, _, _, cx| t.end(true, cx))),
                        ),
                )
                .into_any_element()
        } else {
            h_flex()
                .mt_3()
                .gap_1p5()
                .child(
                    Button::new(ids::SHOW_CODE)
                        .cursor_pointer()
                        .small()
                        .flex_1()
                        .when(!self.code_shown, |b| b.primary())
                        .icon(Icon::new(if join_on {
                            KairoIcon::X
                        } else {
                            KairoIcon::MonitorPlay
                        }))
                        .label(if join_on { "Hide code" } else { "Show code" })
                        .disabled(self.busy)
                        .on_click(cx.listener(|t, _, _, cx| t.toggle_join(cx))),
                )
                .child(
                    Button::new(ids::AUDIENCE_PAGE)
                        .cursor_pointer()
                        .small()
                        .icon(Icon::new(KairoIcon::ExternalLink))
                        .accessibility_label("Open the audience page")
                        .tooltip("Open the audience page")
                        .on_click(cx.listener(|t, _, _, cx| t.open_audience_page(cx))),
                )
                .child(
                    Button::new(ids::SETTINGS)
                        .cursor_pointer()
                        .small()
                        .icon(Icon::new(KairoIcon::Settings))
                        .accessibility_label("Settings and collaboration")
                        .tooltip("Settings & collaboration")
                        .on_click(cx.listener(|t, _, w, cx| {
                            t.open_settings(SettingsSection::General, w, cx)
                        })),
                )
                .child(
                    Button::new(ids::END)
                        .cursor_pointer()
                        .small()
                        .icon(Icon::new(KairoIcon::LogOut))
                        .accessibility_label("End engagement")
                        .tooltip("End engagement")
                        .disabled(self.busy)
                        .on_click(cx.listener(|t, _, _, cx| t.end(false, cx))),
                )
                .into_any_element()
        };
        card(ids::KAIRO_CARD, cx)
            .when(show_selection && !selected, |d| {
                d.opacity(0.8).cursor_pointer()
            })
            .when(show_selection && selected, |d| {
                d.border_1().border_color(k.white.opacity(0.15))
            })
            .on_click(cx.listener(|t, _, _, cx| t.select_source("kairo", cx)))
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(session.title.clone()),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1p5()
                            .text_size(text::LABEL.size)
                            .text_color(if source.state == AudienceSourceState::Error && !closed {
                                k.danger_text
                            } else {
                                k.text(TextStep::T400)
                            })
                            .child(dot(
                                if closed {
                                    AudienceSourceState::Stopped
                                } else {
                                    source.state
                                },
                                cx,
                            ))
                            .child(status),
                    ),
            )
            .when_some(
                source
                    .error
                    .clone()
                    .filter(|_| !model::first_connect(source)),
                |d, e| {
                    d.child(
                        div()
                            .mt_1()
                            .text_size(text::LABEL.size)
                            .text_color(if source.state == AudienceSourceState::Error {
                                k.danger_text
                            } else {
                                k.text(TextStep::T500)
                            })
                            .child(e),
                    )
                },
            )
            .child(
                h_flex()
                    .mt_3()
                    .gap_3()
                    .rounded(radius::lg())
                    .bg(k.surface_secondary)
                    .p_2p5()
                    .child(
                        div()
                            .id(ids::QR)
                            .test_support()
                            .role(Role::Button)
                            .aria_label("Download QR code")
                            .size(px(56.))
                            .flex_none()
                            .rounded(radius::sm())
                            .bg(k.white)
                            .p_1()
                            .cursor_pointer()
                            .on_click(cx.listener(|t, _, _, cx| t.download_qr(cx)))
                            .children(qr.map(|i| img(i).size_full())),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(22.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .font_family("Menlo")
                                    .child(session.code.clone()),
                            )
                            .child(
                                h_flex().mt_1p5().gap_2().child(
                                    div()
                                        .id(ids::COPY_JOIN)
                                        .test_support()
                                        .role(Role::Button)
                                        .aria_label("Copy the join link")
                                        .min_w_0()
                                        .truncate()
                                        .cursor_pointer()
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T500))
                                        .on_click(cx.listener(move |t, _, _, cx| {
                                            t.copy("join", url.clone(), cx)
                                        }))
                                        .child(if copied {
                                            "Link copied".to_owned()
                                        } else {
                                            model::bare_url(&session.url).to_owned()
                                        }),
                                ),
                            ),
                    ),
            )
            .child(actions)
            .children(error.map(|e| alert(ids::KAIRO_ERROR, &e, cx)))
            .into_any_element()
    }

    fn render_connect(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let connecting = self.busy
            || self.state.sources.iter().any(|s| {
                model::is_slido(s)
                    && s.state == AudienceSourceState::Connecting
                    && s.updated_at.is_none()
            });
        let empty = value(&self.slido_code, cx).trim().is_empty();
        let error = self.errors.get("connect").cloned();
        card(ids::SLIDO_FORM, cx)
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2p5()
                            .child(
                                div()
                                    .size_7()
                                    .rounded(radius::md())
                                    .bg(k.surface_elevated)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(KairoIcon::MessageSquare).size_4()),
                            )
                            .child(
                                h_flex()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Questions from Slido")
                                    .child(beta(cx)),
                            ),
                    )
                    .child(
                        Button::new(ids::SLIDO_CANCEL)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::X))
                            .accessibility_label("Cancel")
                            .on_click(cx.listener(|t, _, _, cx| t.set_adding_slido(false, cx))),
                    ),
            )
            .child(
                small(
                    "Use an event you already run in Slido. Nothing is sent back to Slido.",
                    cx,
                )
                .mt_2(),
            )
            .child(small("Event code or link", cx).mt_3())
            .child(
                div().id(ids::SLIDO_CODE).test_support().mt_1().child(
                    Input::new(&self.slido_code)
                        .kairo_size()
                        .small()
                        .disabled(self.busy)
                        .aria_label("Event code or link"),
                ),
            )
            .child(switch_row(
                ids::SLIDO_SKIP,
                "Skip approval",
                "Slido questions go straight to Open.",
                self.slido_skip,
                self.busy,
                cx.listener(|t, on: &bool, _, cx| t.set_slido_skip(*on, cx)),
                cx,
            ))
            .child(if connecting {
                steps(&[("Finding the event", 0), ("Reading questions", -1)], cx)
            } else {
                Button::new(ids::SLIDO_SUBMIT)
                    .cursor_pointer()
                    .primary()
                    .small()
                    .w_full()
                    .mt_3()
                    .label("Connect")
                    .disabled(empty)
                    .on_click(cx.listener(|t, _, _, cx| t.connect_slido(cx)))
                    .into_any_element()
            })
            .children(error.map(|e| alert(ids::SLIDO_ERROR, &e, cx)))
            .into_any_element()
    }

    fn render_slido(
        &mut self,
        source: &kairo_domain::audience::AudienceSourceStatus,
        questions: usize,
        show_selection: bool,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let now = kairo_domain::js::now_ms();
        let (label, detail) = model::slido_status(source, questions, now);
        let sign_in = model::slido_sign_in(source);
        let id = source.id.clone();
        let moderator = source.moderator == Some(true);
        let copied = self.copied.as_deref() == Some(id.as_str());
        let open = self.slido_open.contains(&id);
        let error = self.errors.get(&format!("slido:{id}")).cloned();
        let (i1, i2, i3, i4, i5, i6, i7) = (
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
            id.clone(),
        );
        let link = model::slido_link(source);
        let main: AnyElement = if sign_in || moderator {
            Button::new(SharedString::from(ids::slido_window(&id)))
                .cursor_pointer()
                .small()
                .flex_1()
                .when(sign_in, |b| b.primary())
                .icon(Icon::new(KairoIcon::ExternalLink))
                .label(if sign_in {
                    "Sign in to Slido"
                } else {
                    "Slido window"
                })
                .disabled(self.busy)
                .on_click(cx.listener(move |t, _, _, cx| t.open_source(&i1, cx)))
                .into_any_element()
        } else {
            Button::new(SharedString::from(ids::slido_copy(&id)))
                .cursor_pointer()
                .small()
                .flex_1()
                .icon(Icon::new(if copied {
                    KairoIcon::Check
                } else {
                    KairoIcon::ExternalLink
                }))
                .label(if copied {
                    "Link copied"
                } else {
                    "Copy Slido link"
                })
                .on_click(cx.listener(move |t, _, _, cx| t.copy(&i2, link.clone(), cx)))
                .into_any_element()
        };
        let state = source.state;
        card(ids::slido(&id), cx)
            .p_2p5()
            .when(show_selection && !selected, |d| {
                d.opacity(0.8).cursor_pointer()
            })
            .when(show_selection && selected, |d| {
                d.border_1().border_color(k.white.opacity(0.15))
            })
            .on_click(cx.listener(move |t, _, _, cx| t.select_source(&i3, cx)))
            .child(
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(
                        div()
                            .mt_0p5()
                            .size_5()
                            .flex_none()
                            .rounded(radius::sm())
                            .bg(k.surface_secondary)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(Icon::new(KairoIcon::MessageSquare).size_3()),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .min_w_0()
                                            .text_size(text::XS.size)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(
                                                div().truncate().child(
                                                    source
                                                        .event_name
                                                        .clone()
                                                        .unwrap_or_else(|| "Slido event".into()),
                                                ),
                                            )
                                            .child(beta(cx)),
                                    )
                                    .child(
                                        h_flex()
                                            .flex_none()
                                            .gap_1p5()
                                            .text_size(text::LABEL.size)
                                            .text_color(if state == AudienceSourceState::Error {
                                                k.danger_text
                                            } else {
                                                k.text(TextStep::T400)
                                            })
                                            .child(dot(state, cx))
                                            .child(label),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .mt_0p5()
                                    .gap_1p5()
                                    .text_size(text::LABEL.size)
                                    .text_color(if state == AudienceSourceState::Error {
                                        k.danger_text
                                    } else {
                                        k.text(TextStep::T500)
                                    })
                                    .child(
                                        div()
                                            .font_family("Menlo")
                                            .text_color(k.text(TextStep::T300))
                                            .child(model::slido_code(source)),
                                    )
                                    .child("·")
                                    .when(state == AudienceSourceState::Connecting, |d| {
                                        d.child(Icon::new(KairoIcon::Loader2).size_2p5())
                                    })
                                    .child(div().min_w_0().truncate().child(detail)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .mt_2p5()
                    .gap_1p5()
                    .child(main)
                    .child(
                        Button::new(SharedString::from(ids::slido_settings(&id)))
                            .cursor_pointer()
                            .small()
                            .icon(Icon::new(KairoIcon::Settings))
                            .toggled(open)
                            .accessibility_label("Slido settings")
                            .on_click(
                                cx.listener(move |t, _, _, cx| t.toggle_slido_settings(&i4, cx)),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(ids::slido_disconnect(&id)))
                            .cursor_pointer()
                            .small()
                            .icon(Icon::new(KairoIcon::LogOut))
                            .accessibility_label("Disconnect Slido")
                            .tooltip("Disconnect Slido")
                            .disabled(self.busy)
                            .on_click(cx.listener(move |t, _, _, cx| t.stop_source(&i5, cx))),
                    ),
            )
            .when(open, |d| {
                d.child(
                    v_flex()
                        .mt_2p5()
                        .rounded(radius::lg())
                        .bg(k.surface_secondary)
                        .child(switch_row(
                            ids::slido_skip(&id),
                            "Skip approval",
                            "New questions go straight to Open.",
                            source.skip_review == Some(true),
                            self.busy,
                            cx.listener(move |t, on: &bool, _, cx| {
                                t.restart_slido(&i6, None, Some(*on), cx)
                            }),
                            cx,
                        ))
                        .child(switch_row(
                            ids::slido_pending(&id),
                            "Include pending",
                            if moderator {
                                "Signed in as a Slido moderator."
                            } else {
                                "Needs a Slido moderator sign-in."
                            },
                            moderator,
                            self.busy,
                            cx.listener(move |t, on: &bool, _, cx| {
                                t.restart_slido(&i7, Some(*on), None, cx)
                            }),
                            cx,
                        )),
                )
            })
            .children(error.map(|e| alert("engage-slido-card-error", &e, cx)))
            .into_any_element()
    }

    fn render_questions(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let loading = model::loading_queue(
            &self.state,
            self.active_source().as_deref(),
            self.initial_loading,
        );
        let questions = self.visible_questions();
        let ids_now: Vec<String> = questions.iter().map(|q| q.id.clone()).collect();
        if ids_now != self.shown_ids {
            self.questions.reset(ids_now.len());
            self.shown_ids = ids_now;
        }
        let tabs = SegmentedControl::new(ids::TABS, "Question status")
            .tabs()
            .options(QuestionTab::ALL.map(|t| {
                let n = self.tab_count(t);
                SegmentOption::new(
                    t.key(),
                    if !loading && n > 0 {
                        format!("{} {n}", t.label())
                    } else {
                        t.label().into()
                    },
                )
            }))
            .selected(self.tab.key())
            .on_change(cx.listener(|t, key: &SharedString, _, cx| {
                t.set_tab(QuestionTab::from_key(key), cx)
            }));
        let sort = SegmentedControl::new(ids::SORT, "Sort questions")
            .options([
                SegmentOption::new("votes", "Popular"),
                SegmentOption::new("newest", "Recent"),
            ])
            .selected(self.sort.as_str())
            .on_change(cx.listener(|t, key: &SharedString, _, cx| {
                t.set_sort(
                    if key.as_ref() == "newest" {
                        AudienceQuestionSort::Newest
                    } else {
                        AudienceQuestionSort::Votes
                    },
                    cx,
                )
            }));
        let body: AnyElement = if loading {
            v_flex()
                .id(ids::QUESTIONS_LOADING)
                .test_support()
                .role(Role::Status)
                .aria_label("Loading questions")
                .flex_1()
                .gap_2()
                .p_4()
                .children((0..3).map(|_| {
                    div()
                        .h_32()
                        .rounded(radius::xl())
                        .bg(k.surface_secondary)
                        .opacity(0.6)
                }))
                .into_any_element()
        } else if questions.is_empty() {
            let (title, body) = self.tab.empty();
            v_flex()
                .id(ids::QUESTIONS_EMPTY)
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .px_8()
                .text_center()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child(body),
                )
                .into_any_element()
        } else {
            let this = cx.entity();
            let items = Arc::new(questions);
            div()
                .id(ids::QUESTIONS)
                .test_support()
                .flex_1()
                .min_h_0()
                .px_4()
                .py_2()
                .child(
                    list(self.questions.clone(), move |i, window, cx| {
                        let q = items[i].clone();
                        this.update(cx, |t, cx| t.render_question(&q, window, cx))
                    })
                    .size_full(),
                )
                .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .px_4()
                    .pb_3()
                    .border_b_1()
                    .border_color(k.surface_line)
                    .child(tabs)
                    .child(div().flex_1())
                    .child(sort),
            )
            .child(body)
            .into_any_element()
    }

    fn render_question(
        &mut self,
        q: &AudienceQuestion,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let live = self.state.live_id.as_deref() == Some(q.id.as_str());
        let now = kairo_domain::js::now_ms();
        let busy = self.busy;
        let id = q.id.clone();
        let icon = |what: &str,
                    icon: KairoIcon,
                    label: &'static str,
                    f: QuestionAction,
                    cx: &mut Context<Self>| {
            Button::new(SharedString::from(ids::action(&id, what)))
                .cursor_pointer()
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .accessibility_label(label)
                .tooltip(label)
                .disabled(busy)
                .on_click(cx.listener(move |t, _, _, cx| f(t, cx)))
        };
        let (a, b, c, d, e) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
        let mut actions = h_flex().flex_none().gap_1();
        match q.state {
            AudienceQuestionState::Review | AudienceQuestionState::Approved => {
                let review = q.state == AudienceQuestionState::Review;
                actions = actions
                    .child(icon(
                        "hide",
                        KairoIcon::EyeOff,
                        "Hide question",
                        Box::new(move |t, cx| t.moderate(&a, AudienceQuestionState::Hidden, cx)),
                        cx,
                    ))
                    .child(icon(
                        "answered",
                        KairoIcon::Check,
                        "Mark answered",
                        Box::new(move |t, cx| t.moderate(&b, AudienceQuestionState::Answered, cx)),
                        cx,
                    ));
                actions = if review {
                    actions.child(
                        Button::new(SharedString::from(ids::action(&id, "approve")))
                            .cursor_pointer()
                            .primary()
                            .small()
                            .label("Approve")
                            .disabled(busy)
                            .on_click(cx.listener(move |t, _, _, cx| {
                                t.moderate(&c, AudienceQuestionState::Approved, cx)
                            })),
                    )
                } else if live {
                    actions.child(
                        Button::new(SharedString::from(ids::action(&id, "clear")))
                            .cursor_pointer()
                            .small()
                            .icon(Icon::new(KairoIcon::X))
                            .label("Clear")
                            .disabled(busy)
                            .on_click(cx.listener(|t, _, _, cx| t.clear(cx))),
                    )
                } else {
                    actions.child(
                        Button::new(SharedString::from(ids::action(&id, "show")))
                            .cursor_pointer()
                            .primary()
                            .small()
                            .icon(Icon::new(KairoIcon::MonitorPlay))
                            .label("Show")
                            .disabled(busy)
                            .on_click(cx.listener(move |t, _, _, cx| t.show_question(&d, cx))),
                    )
                };
            }
            AudienceQuestionState::Answered => {
                actions = actions.child(icon(
                    "reopen",
                    KairoIcon::RotateCcw,
                    "Move back to open",
                    Box::new(move |t, cx| t.moderate(&e, AudienceQuestionState::Approved, cx)),
                    cx,
                ));
            }
            AudienceQuestionState::Hidden => {
                actions = actions.child(
                    Button::new(SharedString::from(ids::action(&id, "restore")))
                        .cursor_pointer()
                        .small()
                        .icon(Icon::new(KairoIcon::Eye))
                        .label("Restore")
                        .disabled(busy)
                        .on_click(cx.listener(move |t, _, _, cx| {
                            t.moderate(&e, AudienceQuestionState::Approved, cx)
                        })),
                );
            }
        }
        let replies = self.render_replies(q, cx);
        div()
            .pb_2()
            .child(
                v_flex()
                    .id(SharedString::from(ids::question(&q.id)))
                    .test_support()
                    .rounded(radius::xl())
                    .p_4()
                    .bg(if live {
                        k.tints.live
                    } else {
                        k.surface_secondary
                    })
                    .when(live, |d| d.border_1().border_color(k.live.opacity(0.6)))
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                div()
                                    .size_8()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(k.surface_tertiary)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(model::initial(q.author.as_deref())),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(
                                                q.author
                                                    .clone()
                                                    .unwrap_or_else(|| "Anonymous".into()),
                                            ),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_1p5()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(model::age(q.created_at, now))
                                            .when(live, |d| {
                                                d.child("·").child(
                                                    h_flex()
                                                        .gap_1()
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(k.live)
                                                        .child(
                                                            div()
                                                                .size_1p5()
                                                                .rounded_full()
                                                                .bg(k.live),
                                                        )
                                                        .child("On screens"),
                                                )
                                            }),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .h_8()
                                    .flex_none()
                                    .gap_1p5()
                                    .px_3()
                                    .rounded_full()
                                    .bg(k.surface_tertiary)
                                    .text_size(text::SM.size)
                                    .child(kairo_domain::js::number_to_string(q.votes))
                                    .child(Icon::new(KairoIcon::ArrowUp).size_3()),
                            ),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_size(text::BASE.size)
                            .line_height(relative(1.6))
                            .child(q.text.clone()),
                    )
                    .child(
                        h_flex()
                            .mt_3()
                            .items_end()
                            .gap_3()
                            .child(div().flex_1().min_w_0().child(replies))
                            .child(actions),
                    ),
            )
            .into_any_element()
    }

    /// `QuestionReplies`: Kairo questions only.
    fn render_replies(&mut self, q: &AudienceQuestion, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        if q.source_id != "kairo" || self.state.session.is_none() {
            return div().into_any_element();
        }
        let replies: Vec<_> = self
            .state
            .engage
            .as_ref()
            .map(|e| {
                e.replies
                    .iter()
                    .filter(|r| r.question_id == q.id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let busy = self.busy;
        let id = q.id.clone();
        let composer = self
            .composer
            .clone()
            .filter(|(qid, _)| *qid == q.id)
            .map(|(_, c)| c);
        let mut col = v_flex().mt_1p5().gap_2();
        if q.sent_by.is_some() || q.edited_by.is_some() {
            col = col.child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .text_size(text::LABEL.size)
                    .children(q.sent_by.clone().map(|s| {
                        div()
                            .text_color(k.accent[4])
                            .child(format!("Sent to you by {s}"))
                    }))
                    .children(q.edited_by.clone().map(|e| {
                        div()
                            .text_color(k.text(TextStep::T500))
                            .child(format!("Edited by {e}"))
                    })),
            );
        }
        for r in replies {
            col = col.child(
                v_flex()
                    .rounded(radius::md())
                    .bg(k.surface_tertiary)
                    .px_2p5()
                    .py_2()
                    .child(div().text_size(text::SM.size).child(r.text.clone()))
                    .child(
                        div()
                            .mt_1()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(format!(
                                "{} · {}",
                                r.by,
                                if r.audience == ReplyAudience::Asker {
                                    "Only the asker sees this"
                                } else {
                                    "Everyone sees this"
                                }
                            )),
                    ),
            );
        }
        match composer {
            Some(Composer::Reply) => {
                let empty = value(&self.reply_text, cx).trim().is_empty();
                col = col.child(
                    v_flex()
                        .rounded(radius::md())
                        .bg(k.surface_tertiary)
                        .p_2()
                        .child(
                            div()
                                .id(SharedString::from(ids::action(&id, "reply-text")))
                                .test_support()
                                .child(
                                    Input::new(&self.reply_text)
                                        .kairo_size()
                                        .small()
                                        .aria_label("Reply"),
                                ),
                        )
                        .child(
                            h_flex()
                                .mt_2()
                                .gap_2()
                                .child(
                                    SegmentedControl::new(
                                        SharedString::from(ids::action(&id, "reply-who")),
                                        "Who sees this reply",
                                    )
                                    .options([
                                        SegmentOption::new("everyone", "Everyone"),
                                        SegmentOption::new("asker", "Only the asker"),
                                    ])
                                    .selected(self.reply_audience.as_str())
                                    .on_change(cx.listener(
                                        |t, key: &SharedString, _, cx| {
                                            t.set_reply_audience(
                                                if key.as_ref() == "asker" {
                                                    ReplyAudience::Asker
                                                } else {
                                                    ReplyAudience::Everyone
                                                },
                                                cx,
                                            )
                                        },
                                    )),
                                )
                                .child(div().flex_1())
                                .child(
                                    Button::new(SharedString::from(ids::action(
                                        &id,
                                        "reply-cancel",
                                    )))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .label("Cancel")
                                    .on_click(cx.listener(|t, _, _, cx| t.close_composer(cx))),
                                )
                                .child(
                                    Button::new(SharedString::from(ids::action(&id, "reply-send")))
                                        .cursor_pointer()
                                        .primary()
                                        .xsmall()
                                        .label("Reply")
                                        .disabled(busy || empty)
                                        .on_click(cx.listener(|t, _, w, cx| t.send_reply(w, cx))),
                                ),
                        ),
                );
            }
            Some(Composer::Edit) => {
                let typed = value(&self.edit_text, cx).trim().to_owned();
                let disabled = busy || typed.is_empty() || typed == q.text;
                col = col.child(
                    v_flex()
                        .rounded(radius::md())
                        .bg(k.surface_tertiary)
                        .p_2()
                        .child(
                            div()
                                .id(SharedString::from(ids::action(&id, "edit-text")))
                                .test_support()
                                .child(
                                    Input::new(&self.edit_text)
                                        .kairo_size()
                                        .small()
                                        .aria_label("Question text"),
                                ),
                        )
                        .child(
                            h_flex()
                                .mt_2()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new(SharedString::from(ids::action(
                                        &id,
                                        "edit-cancel",
                                    )))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .label("Cancel")
                                    .on_click(cx.listener(|t, _, _, cx| t.close_composer(cx))),
                                )
                                .child(
                                    Button::new(SharedString::from(ids::action(&id, "edit-save")))
                                        .cursor_pointer()
                                        .primary()
                                        .xsmall()
                                        .label("Save")
                                        .disabled(disabled)
                                        .on_click(cx.listener(|t, _, _, cx| t.save_edit(cx))),
                                ),
                        ),
                );
            }
            None => {
                let (r, e) = (id.clone(), id.clone());
                col = col.child(
                    h_flex()
                        .gap_3()
                        .child(
                            Button::new(SharedString::from(ids::action(&id, "reply")))
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .label("Reply")
                                .disabled(busy)
                                .on_click(cx.listener(move |t, _, w, cx| {
                                    t.open_composer(&r, false, w, cx)
                                })),
                        )
                        .when(q.state == AudienceQuestionState::Review, |d| {
                            d.child(
                                Button::new(SharedString::from(ids::action(&id, "fix")))
                                    .cursor_pointer()
                                    .ghost()
                                    .xsmall()
                                    .label("Fix typo")
                                    .disabled(busy)
                                    .on_click(cx.listener(move |t, _, w, cx| {
                                        t.open_composer(&e, true, w, cx)
                                    })),
                            )
                        }),
                );
            }
        }
        col.into_any_element()
    }

    fn render_polls(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let polls = self
            .state
            .engage
            .as_ref()
            .map(|e| e.polls.clone())
            .unwrap_or_default();
        if model::loading_polls(&self.state, self.initial_loading) && polls.is_empty() {
            return v_flex()
                .id(ids::POLLS_LOADING)
                .test_support()
                .role(Role::Status)
                .aria_label("Loading polls")
                .flex_1()
                .gap_2()
                .p_3()
                .children((0..2).map(|_| {
                    div()
                        .h_32()
                        .rounded(radius::lg())
                        .bg(k.surface_secondary)
                        .opacity(0.6)
                }))
                .into_any_element();
        }
        if self.state.session.is_none() {
            return v_flex()
                .id(ids::POLLS_NONE)
                .test_support()
                .flex_1()
                .items_center()
                .justify_center()
                .text_center()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::MEDIUM)
                        .child("No engagement open"),
                )
                .child(
                    div()
                        .mt_1()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Start or open an engagement to run polls."),
                )
                .into_any_element();
        }
        let cards: Vec<AnyElement> = polls.iter().map(|p| self.render_poll(p, cx)).collect();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .px_4()
                    .pt_3()
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(if polls.is_empty() {
                                String::new()
                            } else {
                                model::count(polls.len(), "poll", "polls")
                            }),
                    )
                    .child(
                        Button::new(ids::NEW_POLL)
                            .cursor_pointer()
                            .primary()
                            .small()
                            .icon(Icon::new(KairoIcon::Plus))
                            .label("New poll")
                            .on_click(cx.listener(|t, _, w, cx| t.open_poll(None, w, cx))),
                    ),
            )
            .child(if polls.is_empty() {
                v_flex()
                    .id(ids::POLLS_EMPTY)
                    .test_support()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .text_center()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::MEDIUM)
                            .child("No polls yet"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Ask the room a question with a few answers to pick from."),
                    )
                    .into_any_element()
            } else {
                v_flex()
                    .id("engage-polls")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_2()
                    .px_3()
                    .py_3()
                    .children(cards)
                    .into_any_element()
            })
            .into_any_element()
    }

    fn render_poll(&mut self, poll: &PollView, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let live =
            self.state.live_poll_id.clone().flatten().as_deref() == Some(poll.poll.id.as_str());
        let id = poll.poll.id.clone();
        let most = model::poll_most(poll);
        let error = self.errors.get(&format!("poll:{id}")).cloned();
        let busy = self.busy;
        let (p1, p2, p3) = (poll.clone(), poll.clone(), poll.clone());
        let (i1, i2) = (id.clone(), id.clone());
        let state = poll.poll.state;
        v_flex()
            .id(SharedString::from(ids::poll(&id)))
            .test_support()
            .rounded(radius::lg())
            .px_4()
            .py_3p5()
            .bg(if live {
                k.tints.live
            } else {
                k.surface_secondary
            })
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(poll.poll.title.clone()),
                            )
                            .child(
                                h_flex()
                                    .mt_1()
                                    .gap_1p5()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T500))
                                    .when(live, |d| {
                                        d.child(
                                            h_flex()
                                                .gap_1()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(k.live)
                                                .child(div().size_1p5().rounded_full().bg(k.live))
                                                .child("On screens"),
                                        )
                                        .child("·")
                                    })
                                    .child(
                                        div()
                                            .text_color(if state == EngagementStatus::Open {
                                                k.success_text
                                            } else {
                                                k.text(TextStep::T500)
                                            })
                                            .child(model::poll_state(state)),
                                    )
                                    .child("·")
                                    .child(model::count(poll.voters as usize, "vote", "votes"))
                                    .when(poll.poll.multiple, |d| {
                                        d.child("·").child("Several answers")
                                    }),
                            ),
                    )
                    .when(state == EngagementStatus::Draft, |d| {
                        d.child(
                            Button::new(SharedString::from(ids::poll_action(&id, "edit")))
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .label("Edit")
                                .on_click(cx.listener(move |t, _, w, cx| {
                                    t.open_poll(Some(i1.clone()), w, cx)
                                })),
                        )
                    }),
            )
            .child(
                v_flex()
                    .mt_3()
                    .gap_2()
                    .children(poll.poll.options.iter().map(|o| {
                        let (n, pct) = model::poll_share(poll, &o.id);
                        v_flex()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .gap_3()
                                    .text_size(text::XS.size)
                                    .child(
                                        div()
                                            .min_w_0()
                                            .text_color(if n > 0.0 && n == most {
                                                k.white
                                            } else {
                                                k.text(TextStep::T300)
                                            })
                                            .child(o.text.clone()),
                                    )
                                    .child(
                                        div().flex_none().text_color(k.text(TextStep::T500)).child(
                                            format!(
                                                "{} · {}%",
                                                kairo_domain::js::number_to_string(n),
                                                pct
                                            ),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .h_1p5()
                                    .rounded_full()
                                    .overflow_hidden()
                                    .bg(k.surface_elevated)
                                    .child(
                                        div()
                                            .h_full()
                                            .rounded_full()
                                            .bg(k.accent[5])
                                            .w(relative((pct / 100.0) as f32)),
                                    ),
                            )
                    })),
            )
            .child(
                h_flex()
                    .mt_3p5()
                    .flex_wrap()
                    .gap_2()
                    .child(if state == EngagementStatus::Open {
                        Button::new(SharedString::from(ids::poll_action(&id, "close")))
                            .cursor_pointer()
                            .small()
                            .label("Close voting")
                            .disabled(busy)
                            .on_click(cx.listener(move |t, _, _, cx| {
                                t.set_poll(&p1, Some(OpenOrClosed::Closed), None, cx)
                            }))
                    } else {
                        Button::new(SharedString::from(ids::poll_action(&id, "open")))
                            .cursor_pointer()
                            .primary()
                            .small()
                            .label(if state == EngagementStatus::Draft {
                                "Open voting"
                            } else {
                                "Reopen voting"
                            })
                            .disabled(busy)
                            .on_click(cx.listener(move |t, _, _, cx| {
                                t.set_poll(&p2, Some(OpenOrClosed::Open), None, cx)
                            }))
                    })
                    .child(
                        Button::new(SharedString::from(ids::poll_action(&id, "screen")))
                            .cursor_pointer()
                            .small()
                            .icon(Icon::new(if live {
                                KairoIcon::X
                            } else {
                                KairoIcon::MonitorPlay
                            }))
                            .label(if live {
                                "Clear from screen"
                            } else {
                                "Show on screen"
                            })
                            .disabled(busy)
                            .on_click(
                                cx.listener(move |t, _, _, cx| t.toggle_poll_screen(&i2, cx)),
                            ),
                    )
                    .when(state != EngagementStatus::Draft, |d| {
                        d.child(div().flex_1()).child(
                            h_flex()
                                .gap_2()
                                .text_size(text::XS.size)
                                .text_color(k.text(TextStep::T400))
                                .child("Results visible to everyone")
                                .child(
                                    Switch::new(SharedString::from(ids::poll_action(
                                        &id, "results",
                                    )))
                                    .small()
                                    .checked(poll.poll.show_results)
                                    .disabled(busy)
                                    .accessibility_label("Results visible to everyone")
                                    .on_change(cx.listener(move |t, on: &bool, _, cx| {
                                        t.set_poll(&p3, None, Some(*on), cx)
                                    })),
                                ),
                        )
                    }),
            )
            .children(error.map(|e| alert("engage-poll-card-error", &e, cx)))
            .into_any_element()
    }

    // ─── Dialogs ───────────────────────────────────────────────────────────

    fn dialog_frame(
        &self,
        title: &str,
        width: f32,
        height: Option<f32>,
        body: AnyElement,
        footer: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let locked = self.pending.is_some() || self.poll_saving;
        div()
            .id(ids::DIALOG)
            .test_support()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(k.scrim())
            .occlude()
            .on_key_down(cx.listener(|t, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "escape" {
                    cx.stop_propagation();
                    t.close_dialog(cx);
                }
            }))
            .child(kairo_ui_kit::motion::enter(
                v_flex()
                    .id("engage-dialog-panel")
                    .role(Role::Dialog)
                    .aria_label(title.to_owned())
                    .w(px(width))
                    // A fixed height keeps tabbed dialogs from resizing as
                    // their sections change.
                    .when_some(height, |d, h| d.h(px(h)))
                    .max_h(relative(0.9))
                    .overflow_hidden()
                    .rounded(radius::xl())
                    .bg(k.surface_menu)
                    .border_1()
                    .border_color(k.surface_menu_line)
                    .child(
                        h_flex()
                            .justify_between()
                            .px_6()
                            .pt_6()
                            .child(
                                div()
                                    .text_size(text::LG.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title.to_owned()),
                            )
                            .child(
                                Button::new(ids::DIALOG_CLOSE)
                                    .cursor_pointer()
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(KairoIcon::X))
                                    .accessibility_label("Close")
                                    .disabled(locked)
                                    .on_click(cx.listener(|t, _, _, cx| t.close_dialog(cx))),
                            ),
                    )
                    .child(
                        div()
                            .id("engage-dialog-body")
                            .flex_1()
                            .min_h_0()
                            // A fixed-height dialog scrolls inside its body
                            // (its nav stays put); others scroll as a whole.
                            .when(height.is_none(), |d| d.overflow_y_scroll())
                            .px_6()
                            .pb_5()
                            .pt_2()
                            .child(body),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .bg(k.surface_tertiary)
                            .px_6()
                            .py_4()
                            .child(footer),
                    )
                    .focus_trap("engage-dialog-panel", self.dialog_focus.handle()),
                "engage-enter",
                cx,
            ))
            .into_any_element()
    }

    fn render_new(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let (starts, ends) = self.new_dates(cx);
        let invalid = model::dates_invalid(starts, ends);
        let pending = self.pending;
        let error = self.errors.get("new").cloned();
        let label = |t: &str| {
            div()
                .text_size(text::XS.size)
                .font_weight(FontWeight::MEDIUM)
                .text_color(k.text(TextStep::T300))
                .child(t.to_owned())
        };
        let rows: Vec<AnyElement> = model::room_rows(&self.new_settings)
            .into_iter()
            .map(|(key, l, hint, on)| {
                switch_row(
                    ids::room(&format!("new-{key}")),
                    l,
                    hint,
                    on,
                    pending.is_some(),
                    cx.listener(move |t, v: &bool, _, cx| t.set_new_setting(key, *v, cx)),
                    cx,
                )
            })
            .collect();
        let body = v_flex()
            .child(small(
                "People join with a code and send questions. You choose what goes on screen.",
                cx,
            ))
            .child(label("Title").mt_5())
            .child(
                div().id(ids::NEW_TITLE).test_support().mt_1p5().child(
                    Input::new(&self.new_title)
                        .kairo_size()
                        .disabled(pending.is_some())
                        .aria_label("Title"),
                ),
            )
            .child(
                h_flex()
                    .mt_4()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .child(label("Start date · optional"))
                            .child(
                                div().id(ids::NEW_STARTS).test_support().mt_1p5().child(
                                    gpui_kit::component::date_picker::DatePicker::new(
                                        &self.new_starts,
                                    )
                                    .placeholder("Pick a date")
                                    .cleanable(true)
                                    .disabled(pending.is_some()),
                                ),
                            ),
                    )
                    .child(
                        v_flex().flex_1().child(label("End date · optional")).child(
                            div().id(ids::NEW_ENDS).test_support().mt_1p5().child(
                                gpui_kit::component::date_picker::DatePicker::new(&self.new_ends)
                                    .placeholder("Pick a date")
                                    .cleanable(true)
                                    .disabled(pending.is_some()),
                            ),
                        ),
                    ),
            )
            .when(invalid, |d| {
                d.child(
                    div()
                        .id(ids::NEW_DATES)
                        .test_support()
                        .mt_1p5()
                        .text_size(text::XS.size)
                        .text_color(k.danger_text)
                        .child("The end date is before the start date."),
                )
            })
            .child(label("Questions").mt_5())
            .child(
                v_flex()
                    .mt_3()
                    .rounded(radius::lg())
                    .bg(k.surface_secondary)
                    .children(rows),
            )
            .children(error.map(|e| alert(ids::NEW_ERROR, &e, cx)))
            .when(self.slow, |d| {
                d.child(
                    div()
                        .id(ids::NEW_SLOW)
                        .test_support()
                        .mt_4()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Still working. The first one of the day can take up to a minute."),
                )
            })
            .into_any_element();
        let footer = h_flex()
            .gap_2()
            .child(
                Button::new(ids::NEW_SAVE)
                    .cursor_pointer()
                    .label(if pending == Some("save") {
                        "Saving…"
                    } else {
                        "Save for later"
                    })
                    .disabled(pending.is_some() || invalid)
                    .on_click(cx.listener(|t, _, _, cx| t.submit_new(false, cx))),
            )
            .child(
                Button::new(ids::NEW_START)
                    .cursor_pointer()
                    .primary()
                    .label(if pending == Some("start") {
                        "Starting…"
                    } else {
                        "Start now"
                    })
                    .disabled(pending.is_some() || invalid)
                    .on_click(cx.listener(|t, _, _, cx| t.submit_new(true, cx))),
            )
            .into_any_element();
        self.dialog_frame("New engagement", 460.0, None, body, footer, cx)
    }

    fn render_poll_editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let Some(p) = &self.poll else {
            return div().into_any_element();
        };
        let title = if p.id.is_some() {
            "Edit poll"
        } else {
            "New poll"
        };
        let count = p.options.len();
        let multiple = p.multiple;
        let ready = self.poll_ready(cx).is_some();
        let error = self.errors.get("poll-editor").cloned();
        let rows: Vec<AnyElement> = p
            .options
            .iter()
            .enumerate()
            .map(|(i, (_, f))| {
                h_flex()
                    .gap_1p5()
                    .child(
                        div()
                            .id(SharedString::from(ids::poll_option(i)))
                            .test_support()
                            .flex_1()
                            .child(
                                Input::new(f)
                                    .kairo_size()
                                    .aria_label(format!("Answer {}", i + 1)),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(ids::poll_remove(i)))
                            .cursor_pointer()
                            .ghost()
                            .small()
                            .icon(Icon::new(KairoIcon::Trash2))
                            .accessibility_label(format!("Remove answer {}", i + 1))
                            .disabled(count <= 2)
                            .on_click(cx.listener(move |t, _, _, cx| t.remove_poll_option(i, cx))),
                    )
                    .into_any_element()
            })
            .collect();
        let title_input = p.title.clone();
        let body = v_flex()
            .child(
                div()
                    .text_size(text::XS.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T300))
                    .child("Question"),
            )
            .child(
                div()
                    .id(ids::POLL_TITLE)
                    .test_support()
                    .mt_1p5()
                    .child(Input::new(&title_input).kairo_size().aria_label("Question")),
            )
            .child(
                div()
                    .mt_4()
                    .text_size(text::XS.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T300))
                    .child("Answers"),
            )
            .child(v_flex().mt_1p5().gap_1p5().children(rows))
            .when(count < model::MAX_POLL_OPTIONS, |d| {
                d.child(
                    Button::new(ids::POLL_ADD)
                        .cursor_pointer()
                        .ghost()
                        .small()
                        .mt_2()
                        .icon(Icon::new(KairoIcon::Plus))
                        .label("Add an answer")
                        .on_click(cx.listener(|t, _, w, cx| t.add_poll_option(w, cx))),
                )
            })
            .child(
                h_flex()
                    .mt_4()
                    .justify_between()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .child("People can pick more than one"),
                    )
                    .child(
                        Switch::new(ids::POLL_MULTIPLE)
                            .small()
                            .checked(multiple)
                            .accessibility_label("People can pick more than one")
                            .on_change(
                                cx.listener(|t, on: &bool, _, cx| t.set_poll_multiple(*on, cx)),
                            ),
                    ),
            )
            .children(error.map(|e| alert(ids::POLL_ERROR, &e, cx)))
            .into_any_element();
        let saving = self.poll_saving;
        let footer = h_flex()
            .gap_2()
            .child(
                Button::new("engage-poll-cancel")
                    .cursor_pointer()
                    .label("Cancel")
                    .disabled(saving)
                    .on_click(cx.listener(|t, _, _, cx| t.close_dialog(cx))),
            )
            .child(
                Button::new(ids::POLL_SAVE)
                    .cursor_pointer()
                    .primary()
                    .label(if saving { "Saving…" } else { "Save poll" })
                    .disabled(saving || !ready)
                    .on_click(cx.listener(|t, _, _, cx| t.save_poll(cx))),
            )
            .into_any_element();
        self.dialog_frame(title, 460.0, None, body, footer, cx)
    }

    fn style_slider(
        &mut self,
        key: &'static str,
        value_now: f64,
        (min, max, step): (f64, f64, f64),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = match self.sliders.get(key) {
            Some(s) => s.clone(),
            None => {
                let s = cx.new(|_| {
                    SliderState::new()
                        .min(min as f32)
                        .max(max as f32)
                        .step(step as f32)
                        .default_value(value_now as f32)
                });
                self._subscriptions
                    .push(cx.subscribe(&s, move |t, _, e: &SliderEvent, cx| {
                        if let SliderEvent::Change(v) = e {
                            let v = f64::from(v.start());
                            t.patch_style(
                                |s| match key {
                                    "size" => s.font_size_px = v,
                                    "opacity" => s.background_opacity = v,
                                    "width" => s.width_pct = v,
                                    "radius" => s.border_radius_px = v,
                                    _ => {
                                        s.qr_size_px = v;
                                    }
                                },
                                cx,
                            );
                            if key == "qr" {
                                t.preview_join = true;
                            }
                        }
                    }));
                self.sliders.insert(key, s.clone());
                s
            }
        };
        let _ = window;
        div()
            .id(SharedString::from(ids::style(key)))
            .test_support()
            .w(px(112.))
            .child(Slider::new(&state).accessibility_label(style_label(key)))
            .into_any_element()
    }

    fn style_color(
        &mut self,
        key: &'static str,
        value_now: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = match self.colors.get(key) {
            Some(s) => s.clone(),
            None => {
                let c = hex_hsla(value_now);
                let s = cx.new(|cx| {
                    let s = ColorPickerState::new(window, cx);
                    match c {
                        Some(c) => s.default_value(c),
                        None => s,
                    }
                });
                self._subscriptions.push(cx.subscribe(
                    &s,
                    move |t, _, e: &ColorPickerEvent, cx| {
                        if let ColorPickerEvent::Change(Some(c)) = e {
                            let hex = hsla_hex(*c);
                            t.patch_style(
                                |s| {
                                    if key == "color" {
                                        s.color = hex
                                    } else {
                                        s.background = hex
                                    }
                                },
                                cx,
                            );
                        }
                    },
                ));
                self.colors.insert(key, s.clone());
                s
            }
        };
        h_flex()
            .gap_2()
            .child(
                div()
                    .font_family("Menlo")
                    .text_size(text::LABEL.size)
                    .child(value_now.to_uppercase()),
            )
            .child(ColorPicker::new(&state).accessibility_label(style_label(key)))
            .into_any_element()
    }

    /// The screens' question or join card, small (`renderQuestionHTML`).
    fn style_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let s = self.draft_style.clone();
        let width = 336.0_f32;
        let scale = width / 1920.0;
        let session = self.state.session.clone();
        let qr = self.qr_image();
        let fg = hex_hsla(&s.color).unwrap_or(k.white);
        let bg = hex_hsla(&s.background)
            .unwrap_or(k.surface)
            .opacity((s.background_opacity / 100.0) as f32);
        let family = match s.font_family {
            kairo_domain::program::ProgramQuestionFont::Serif => "Georgia",
            kairo_domain::program::ProgramQuestionFont::Mono => "Menlo",
            _ => "Source Sans 3",
        };
        let card: AnyElement = if self.preview_join {
            let code = session
                .as_ref()
                .map(|s| s.code.clone())
                .unwrap_or_else(|| "2AGXHS".into());
            let url = session
                .as_ref()
                .map(|s| model::bare_url(&s.url).to_owned())
                .unwrap_or_else(|| "usekairo.com/q/2AGXHS".into());
            v_flex()
                .items_center()
                .gap(px(24.0 * scale))
                .p(px(48.0 * scale))
                .rounded(px((s.border_radius_px as f32) * scale))
                .bg(bg)
                .text_color(fg)
                .font_family(family)
                .child(div().text_size(px((s.font_size_px as f32) * scale)).child(
                    if s.join_headline.is_empty() {
                        "Ask a question".to_owned()
                    } else {
                        s.join_headline.clone()
                    },
                ))
                .child(
                    div()
                        .size(px((s.qr_size_px as f32) * scale))
                        .bg(k.white)
                        .p(px(8.0 * scale))
                        .children(qr.map(|i| img(i).size_full())),
                )
                .child(
                    div()
                        .text_size(px((s.font_size_px as f32) * 1.2 * scale))
                        .font_weight(FontWeight::BOLD)
                        .child(code),
                )
                .when(s.show_join_url, |d| {
                    d.child(
                        div()
                            .text_size(px((s.font_size_px as f32) * 0.5 * scale))
                            .child(url),
                    )
                })
                .into_any_element()
        } else {
            v_flex()
                .w(relative((s.width_pct / 100.0) as f32))
                .p(px(48.0 * scale))
                .rounded(px((s.border_radius_px as f32) * scale))
                .bg(bg)
                .text_color(fg)
                .font_family(family)
                .when(s.align == QuestionAlign::Center, |d| d.items_center().text_center())
                .when(s.show_author, |d| d.child(div().text_size(px((s.font_size_px as f32) * 0.6 * scale)).opacity(0.75).child("Sarah Jenkins")))
                .child(div().text_size(px((s.font_size_px as f32) * scale)).child("How can we support families in our community who are going through a difficult season right now?"))
                .into_any_element()
        };
        v_flex()
            .id(ids::PREVIEW)
            .test_support()
            .w(px(width))
            .h(px(width * 9.0 / 16.0))
            .mx_auto()
            .overflow_hidden()
            .rounded(radius::lg())
            .bg(k.surface_stage)
            .items_center()
            .p(px(32.0 * scale))
            .map(|d| match s.position {
                QuestionPosition::Top => d.justify_start(),
                QuestionPosition::Bottom => d.justify_end(),
                _ => d.justify_center(),
            })
            .child(card)
            .into_any_element()
    }

    fn render_settings(
        &mut self,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let Some(session) = self.state.session.clone() else {
            return div().into_any_element();
        };
        let live = self
            .state
            .sources
            .iter()
            .any(|s| s.id == "kairo" && s.state == AudienceSourceState::Live);
        let moderators: Vec<_> = self
            .state
            .engage
            .as_ref()
            .and_then(|e| e.moderators.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|m| !m.revoked)
            .collect();
        let error = self.errors.get("settings").cloned();
        let nav = |id: &'static str,
                   label: &'static str,
                   icon: KairoIcon,
                   s: SettingsSection,
                   badge: Option<usize>,
                   cx: &mut Context<Self>| {
            h_flex()
                .id(id)
                .test_support()
                .role(Role::Button)
                .aria_label(label)
                .justify_between()
                .gap_2p5()
                .px_2p5()
                .py_2()
                .rounded(radius::lg())
                .cursor_pointer()
                .text_size(text::SM.size)
                .font_weight(FontWeight::MEDIUM)
                .when(section == s, |d| d.bg(k.white.opacity(0.1)))
                .on_click(cx.listener(move |t, _, _, cx| t.set_section(s, cx)))
                .child(
                    h_flex()
                        .gap_2p5()
                        .child(Icon::new(icon).size_4())
                        .child(label),
                )
                .children(badge.filter(|n| *n > 0).map(|n| {
                    div()
                        .rounded_full()
                        .bg(k.white.opacity(0.1))
                        .px_1p5()
                        .text_size(text::MICRO.size)
                        .child(n.to_string())
                }))
        };
        let pane: AnyElement = match section {
            SettingsSection::General => {
                let rows: Vec<AnyElement> = model::room_rows(&session.settings)
                    .into_iter()
                    .map(|(key, l, hint, on)| {
                        switch_row(
                            ids::room(key),
                            l,
                            hint,
                            on,
                            self.busy || !live,
                            cx.listener(move |t, v: &bool, _, cx| t.set_room_setting(key, *v, cx)),
                            cx,
                        )
                    })
                    .collect();
                let qr = self.qr_image();
                let copied = self.copied.as_deref() == Some("settings");
                let url = session.url.clone();
                v_flex()
                    .gap_5()
                    .child(v_flex().child(div().text_size(text::XS.size).font_weight(FontWeight::MEDIUM).child("Q&A moderation")).child(small("Control how audience questions appear and whether participants can vote.", cx).mt_0p5()).child(v_flex().mt_3().rounded(radius::lg()).bg(k.surface_secondary).children(rows)))
                    .child(
                        v_flex()
                            .child(div().text_size(text::XS.size).font_weight(FontWeight::MEDIUM).child("Join code & QR code"))
                            .child(small("Share this QR code on slides or printouts so people can scan to join.", cx).mt_0p5())
                            .child(
                                h_flex()
                                    .mt_2p5()
                                    .gap_4()
                                    .rounded(radius::xl())
                                    .bg(k.surface_secondary)
                                    .p_3p5()
                                    .child(div().size_20().flex_none().rounded(radius::lg()).bg(k.white).p_1p5().children(qr.map(|i| img(i).size_full())))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .child(div().font_family("Menlo").text_size(text::XL.size).font_weight(FontWeight::SEMIBOLD).child(session.code.clone()))
                                            .child(div().mt_0p5().truncate().text_size(text::XS.size).text_color(k.text(TextStep::T400)).child(session.url.clone()))
                                            .child(
                                                h_flex()
                                                    .mt_3()
                                                    .gap_2()
                                                    .child(Button::new(ids::SET_QR).cursor_pointer().small().icon(Icon::new(KairoIcon::Download)).label("Download QR code").on_click(cx.listener(|t, _, _, cx| t.download_qr(cx))))
                                                    .child(Button::new(ids::SET_COPY).cursor_pointer().small().icon(Icon::new(if copied { KairoIcon::Check } else { KairoIcon::Copy })).label(if copied { "Copied" } else { "Copy link" }).on_click(cx.listener(move |t, _, _, cx| t.copy("settings", url.clone(), cx)))),
                                            ),
                                    ),
                            ),
                    )
                    .children(error.map(|e| alert(ids::SET_ERROR, &e, cx)))
                    .into_any_element()
            }
            SettingsSection::Screens => {
                let s = self.draft_style.clone();
                let preview = self.style_preview(cx);
                let row = |label: &str, control: AnyElement| {
                    h_flex()
                        .justify_between()
                        .gap_4()
                        .px_3p5()
                        .py_2p5()
                        .text_size(text::XS.size)
                        .child(label.to_owned())
                        .child(control)
                        .into_any_element()
                };
                let seg = |id: &'static str,
                           label: &'static str,
                           opts: Vec<(&'static str, &'static str)>,
                           sel: &'static str,
                           f: fn(&mut ProgramQuestionStyle, &str),
                           cx: &mut Context<Self>| {
                    SegmentedControl::new(SharedString::from(ids::style(id)), label)
                        .options(opts.into_iter().map(|(k, l)| SegmentOption::new(k, l)))
                        .selected(sel)
                        .on_change(cx.listener(move |t, key: &SharedString, _, cx| {
                            let key = key.to_string();
                            t.patch_style(|s| f(s, &key), cx)
                        }))
                        .into_any_element()
                };
                let presets =
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .children(model::THEME_PRESETS.into_iter().map(
                            |(id, label, fg, bg, op)| {
                                let active = s.color.eq_ignore_ascii_case(fg)
                                    && s.background.eq_ignore_ascii_case(bg);
                                v_flex()
                                    .id(SharedString::from(ids::preset(id)))
                                    .test_support()
                                    .role(Role::Button)
                                    .aria_label(label)
                                    .w(px(96.))
                                    .items_center()
                                    .gap_1p5()
                                    .rounded(radius::lg())
                                    .p_2()
                                    .cursor_pointer()
                                    .bg(if active {
                                        k.white.opacity(0.1)
                                    } else {
                                        k.surface_secondary
                                    })
                                    .when(active, |d| d.border_1().border_color(k.accent[5]))
                                    .on_click(cx.listener(move |t, _, _, cx| {
                                        t.colors.clear();
                                        t.patch_style(
                                            |s| {
                                                s.color = fg.into();
                                                s.background = bg.into();
                                                s.background_opacity = op;
                                            },
                                            cx,
                                        )
                                    }))
                                    .child(
                                        div()
                                            .h_7()
                                            .w_full()
                                            .rounded(radius::sm())
                                            .bg(hex_hsla(bg).unwrap_or(k.surface))
                                            .text_color(hex_hsla(fg).unwrap_or(k.white))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Aa"),
                                    )
                                    .child(div().text_size(text::LABEL.size).child(label))
                            },
                        ));
                let size =
                    self.style_slider("size", s.font_size_px, (24.0, 100.0, 2.0), window, cx);
                let color = self.style_color("color", &s.color, window, cx);
                let card = self.style_color("background", &s.background, window, cx);
                let opacity = self.style_slider(
                    "opacity",
                    s.background_opacity,
                    (0.0, 100.0, 5.0),
                    window,
                    cx,
                );
                let width = self.style_slider("width", s.width_pct, (50.0, 100.0, 5.0), window, cx);
                let rounding =
                    self.style_slider("radius", s.border_radius_px, (0.0, 48.0, 4.0), window, cx);
                let qr = self.style_slider("qr", s.qr_size_px, (160.0, 400.0, 20.0), window, cx);
                let n =
                    |v: f64, unit: &str| format!("{}{unit}", kairo_domain::js::number_to_string(v));
                let with = |c: AnyElement, shown: String| {
                    h_flex()
                        .gap_2p5()
                        .child(c)
                        .child(
                            div()
                                .w_9()
                                .text_right()
                                .text_size(text::XS.size)
                                .child(shown),
                        )
                        .into_any_element()
                };
                let group = |rows: Vec<AnyElement>| {
                    v_flex()
                        .rounded(radius::lg())
                        .bg(k.surface_secondary)
                        .children(rows)
                        .into_any_element()
                };
                let modes = SegmentedControl::new(ids::style("preview-mode"), "Preview type")
                    .options([
                        SegmentOption::new("question", "Question"),
                        SegmentOption::new("join", "Join code"),
                    ])
                    .selected(if self.preview_join {
                        "join"
                    } else {
                        "question"
                    })
                    .on_change(cx.listener(|t, key: &SharedString, _, cx| {
                        t.preview_join = key.as_ref() == "join";
                        cx.notify()
                    }));
                v_flex()
                    .gap_4()
                    .child(v_flex().gap_2().child(h_flex().justify_between().child(small("Screen preview", cx)).child(modes)).child(preview))
                    .child(v_flex().gap_2().child(small("Theme presets", cx)).child(presets))
                    .child(v_flex().gap_1p5().child(small("Appearance & layout", cx)).child(group(vec![
                        row("Screen position", seg("position", "Question position", vec![("top", "Top"), ("center", "Middle"), ("bottom", "Bottom")], s.position.as_str(), |s, k| if let Some(p) = QuestionPosition::from_name(k) { s.position = p }, cx)),
                        row("Text alignment", seg("align", "Question text alignment", vec![("left", "Left"), ("center", "Center")], s.align.as_str(), |s, k| if let Some(a) = QuestionAlign::from_name(k) { s.align = a }, cx)),
                        row("Font style", seg("font", "Question font family", vec![("sans", "Sans"), ("serif", "Serif"), ("mono", "Mono")], s.font_family.as_str(), |s, k| if let Some(f) = kairo_domain::program::ProgramQuestionFont::from_name(k) { s.font_family = f }, cx)),
                        row("Text size", with(size, n(s.font_size_px, "px"))),
                        row("Text color", color),
                        row("Card color", card),
                        row("Card opacity", with(opacity, n(s.background_opacity, "%"))),
                        row("Card width", with(width, n(s.width_pct, "%"))),
                        row("Corner rounding", with(rounding, n(s.border_radius_px, "px"))),
                        switch_row(ids::style("author"), "Show sender name", "Display who asked the question above the text.", s.show_author, false, cx.listener(|t, on: &bool, _, cx| { let on = *on; t.patch_style(|s| s.show_author = on, cx) }), cx),
                    ])))
                    .child(v_flex().gap_1p5().child(small("Join screen & QR code on screen", cx)).child(group(vec![
                        row("Prompt headline", div().id(ids::style("headline")).test_support().w(px(210.)).child(Input::new(&self.headline).kairo_size().small().aria_label("Prompt headline")).into_any_element()),
                        row("QR code size", with(qr, n(s.qr_size_px, "px"))),
                        switch_row(ids::style("join-url"), "Show join link", "Display the web link underneath the join code.", s.show_join_url, false, cx.listener(|t, on: &bool, _, cx| { let on = *on; t.preview_join = true; t.patch_style(|s| s.show_join_url = on, cx) }), cx),
                    ])))
                    .children(error.map(|e| alert(ids::SET_ERROR, &e, cx)))
                    .into_any_element()
            }
            SettingsSection::Collaboration => {
                let empty = value(&self.invite_email, cx).trim().is_empty();
                let inviting = self.inviting;
                let busy = self.busy;
                let rows: Vec<AnyElement> = moderators
                    .iter()
                    .map(|m| {
                        let accepted = m.last_seen_at.is_some();
                        let who = m
                            .name
                            .clone()
                            .filter(|n| !n.is_empty())
                            .unwrap_or_else(|| m.email.clone());
                        let (id1, id2) = (m.id.clone(), m.id.clone());
                        let open = self.menu_open.as_deref() == Some(m.id.as_str());
                        h_flex()
                            .relative()
                            .gap_3()
                            .py_2p5()
                            .child(if accepted {
                                div()
                                    .size_8()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(k.accent[5].opacity(0.2))
                                    .text_color(k.accent[3])
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(model::initial(Some(if who.is_empty() {
                                        "?"
                                    } else {
                                        &who
                                    })))
                                    .into_any_element()
                            } else {
                                div()
                                    .size_8()
                                    .flex_none()
                                    .rounded_full()
                                    .bg(k.surface_secondary)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(Icon::new(KairoIcon::CircleUser).size_4())
                                    .into_any_element()
                            })
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_size(text::SM.size)
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .child(who.clone()),
                                            )
                                            .child(
                                                div()
                                                    .rounded(radius::sm())
                                                    .bg(k.surface_tertiary)
                                                    .px_1p5()
                                                    .text_size(text::MICRO.size)
                                                    .child("Guest"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(
                                                if m.name.as_deref().is_some_and(|n| !n.is_empty())
                                                {
                                                    m.email.clone()
                                                } else if accepted {
                                                    "Collaborator".into()
                                                } else {
                                                    "pending invitation".into()
                                                },
                                            ),
                                    ),
                            )
                            .child(
                                Button::new(SharedString::from(ids::moderator_menu(&m.id)))
                                    .cursor_pointer()
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(KairoIcon::MoreVertical))
                                    .accessibility_label(format!("Actions for {who}"))
                                    .disabled(busy)
                                    .on_click(
                                        cx.listener(move |t, _, _, cx| t.toggle_menu(&id1, cx)),
                                    ),
                            )
                            .when(open, |d| {
                                d.child(
                                    div()
                                        .absolute()
                                        .right_0()
                                        .top_10()
                                        .rounded(radius::lg())
                                        .bg(k.surface_elevated)
                                        .p_1()
                                        .child(
                                            Button::new(SharedString::from(ids::revoke(&m.id)))
                                                .cursor_pointer()
                                                .ghost()
                                                .small()
                                                .label("Revoke access")
                                                .disabled(busy)
                                                .on_click(cx.listener(move |t, _, _, cx| {
                                                    t.revoke(&id2, cx)
                                                })),
                                        ),
                                )
                            })
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .gap_5()
                    .child(v_flex().child(div().text_size(text::SM.size).child("Invite others to help you with managing this engagement.")).child(small("They get a sign-in link by email to moderate questions and polls from the website. No account needed.", cx).mt_0p5()))
                    .child(
                        h_flex()
                            .gap_2p5()
                            .child(div().size_8().flex_none().rounded_full().bg(k.surface_secondary).flex().items_center().justify_center().child(Icon::new(KairoIcon::CircleUser).size_4()))
                            .child(div().id(ids::INVITE_EMAIL).test_support().flex_1().child(Input::new(&self.invite_email).kairo_size().disabled(busy || inviting).aria_label("Collaborator email")))
                            .child(Button::new(ids::INVITE).cursor_pointer().primary().label(if inviting { "Inviting…" } else { "Invite" }).disabled(busy || inviting || empty).on_click(cx.listener(|t, _, _, cx| t.invite(cx)))),
                    )
                    .children(error.map(|e| alert(ids::SET_ERROR, &e, cx)))
                    .child(v_flex().pt_3().border_t_1().border_color(k.white.opacity(0.1)).child(if rows.is_empty() {
                        div().id(ids::NO_COLLABORATORS).test_support().py_6().text_center().text_size(text::XS.size).text_color(k.text(TextStep::T500)).child("No collaborators invited yet.").into_any_element()
                    } else {
                        v_flex().children(rows).into_any_element()
                    }))
                    .into_any_element()
            }
        };
        let footer: AnyElement = if section == SettingsSection::Screens && self.style_dirty() {
            h_flex()
                .gap_2()
                .child(
                    Button::new(ids::SET_RESET)
                        .cursor_pointer()
                        .small()
                        .label("Reset")
                        .disabled(self.saving_style)
                        .on_click(cx.listener(|t, _, w, cx| t.reset_style(w, cx))),
                )
                .child(
                    Button::new(ids::SET_SAVE)
                        .cursor_pointer()
                        .primary()
                        .small()
                        .label(if self.saving_style {
                            "Saving…"
                        } else {
                            "Save changes"
                        })
                        .disabled(self.saving_style)
                        .on_click(cx.listener(|t, _, _, cx| t.save_style(false, cx))),
                )
                .into_any_element()
        } else {
            Button::new(ids::SET_DONE)
                .cursor_pointer()
                .primary()
                .small()
                .label("Done")
                .on_click(cx.listener(|t, _, _, cx| t.settings_done(cx)))
                .into_any_element()
        };
        let n = moderators.len();
        let body = h_flex()
            .h_full()
            .items_start()
            .gap_4()
            .child(
                v_flex()
                    .w(px(176.))
                    .flex_none()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::LABEL.size)
                            .text_color(k.text(TextStep::T500))
                            .child(session.title.clone()),
                    )
                    .child(nav(
                        ids::SET_GENERAL,
                        "General",
                        KairoIcon::SlidersHorizontal,
                        SettingsSection::General,
                        None,
                        cx,
                    ))
                    .child(nav(
                        ids::SET_SCREENS,
                        "On Screens",
                        KairoIcon::MonitorPlay,
                        SettingsSection::Screens,
                        None,
                        cx,
                    ))
                    .child(nav(
                        ids::SET_COLLAB,
                        "Collaboration",
                        KairoIcon::CircleUser,
                        SettingsSection::Collaboration,
                        Some(n),
                        cx,
                    )),
            )
            .child(
                div()
                    .id("engage-settings-pane")
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(pane),
            )
            .into_any_element();
        self.dialog_frame("Settings", 740.0, Some(640.0), body, footer, cx)
    }
}

impl Render for AudiencePage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.dialog_focus.sync(self.dialog.is_some(), window, cx);
        // A finished reply or invite empties its field.
        if let Some(field) = self.pending_clear.take() {
            field.update(cx, |i, cx| i.set_value("", window, cx));
        }
        let k = *cx.kairo();
        let sidebar = self.render_sidebar(cx);
        let polls_available = model::polls_available(self.active_source().as_deref());
        let activity = self.activity();
        let middle = if activity == Activity::Qa {
            self.render_questions(cx)
        } else {
            self.render_polls(cx)
        };
        let tabs = SegmentedControl::new(ids::ACTIVITY, "Activity")
            .tabs()
            .options(if polls_available {
                vec![
                    SegmentOption::new("qa", "Q&A"),
                    SegmentOption::new("polls", "Polls"),
                ]
            } else {
                vec![SegmentOption::new("qa", "Q&A")]
            })
            .selected(if activity == Activity::Qa {
                "qa"
            } else {
                "polls"
            })
            .on_change(cx.listener(|t, key: &SharedString, _, cx| {
                t.set_activity(
                    if key.as_ref() == "polls" {
                        Activity::Polls
                    } else {
                        Activity::Qa
                    },
                    cx,
                )
            }));
        let dialog = match self.dialog.clone() {
            Some(Dialog::NewEngagement) => Some(self.render_new(cx)),
            Some(Dialog::Poll(_)) => Some(self.render_poll_editor(cx)),
            Some(Dialog::Settings(s)) => Some(self.render_settings(s, window, cx)),
            None => None,
        };
        let error = self.errors.get("page").cloned();
        h_flex()
            .id(ids::ROOT)
            .test_support()
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .min_h_0()
            .bg(k.surface)
            .child(sidebar)
            .child(kairo_ui_kit::components::page_sidebar::handle(
                "engage-sidebar-handle",
                None,
                cx,
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(error.map(|e| {
                        div()
                            .id(ids::ERROR)
                            .test_support()
                            .role(Role::Alert)
                            .mx_4()
                            .mt_3()
                            .flex_none()
                            .rounded(radius::md())
                            .bg(k.tints.live)
                            .px_3()
                            .py_2()
                            .text_size(text::XS.size)
                            .text_color(k.danger_text)
                            .child(e)
                    }))
                    .child(
                        h_flex()
                            .flex_none()
                            .justify_center()
                            .px_4()
                            .pb_2()
                            .pt_3()
                            .child(tabs),
                    )
                    .child(middle),
            )
            .children(self.rail.clone())
            .children(dialog)
    }
}

/// The accessible name of a question-style control: its row's label.
fn style_label(key: &str) -> &'static str {
    match key {
        "size" => "Text size",
        "color" => "Text color",
        "background" => "Card color",
        "opacity" => "Card opacity",
        "width" => "Card width",
        "radius" => "Corner rounding",
        "qr" => "QR code size",
        _ => "Question style",
    }
}

/// A date picker's day as `YYYY-MM-DD`, or empty.
fn day(picker: &Entity<gpui_kit::component::date_picker::DatePickerState>, cx: &App) -> String {
    match picker.read(cx).date() {
        gpui_kit::component::calendar::Date::Single(Some(day)) => {
            day.format("%Y-%m-%d").to_string()
        }
        _ => String::new(),
    }
}
