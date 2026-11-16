//! The operator window's root view (`App.tsx`, `AppShell.tsx`, `main.tsx`).

use crate::bootstrap::{Launch, SPLASH_FALLBACK_MS, SPLASH_MIN_VISIBLE_MS};
use crate::crash::CrashGuard;
use crate::keymap;
use crate::route::{Route, import_route};
use crate::state::{FeedbackKind, ShellRequest, ShellState, UpdateState};
use crate::ui_scale::UiScale;
use crate::whats_new::{self, ChangelogEntry};
use gpui_kit::component::Theme;
use gpui_kit::component::input::TextareaState;
use gpui_kit::{
    AnyView, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent,
    SharedString, Subscription, Task, Window,
};
use kairo_core::quit::QuitConfirmation;
use kairo_domain::import_menu::ImportKind;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The update toast's time on screen.
pub const TOAST_FOR: Duration = Duration::from_millis(12_000);
/// The feedback popover closes this long after the thank-you.
pub const FEEDBACK_CLOSE_AFTER: Duration = Duration::from_millis(2_200);
/// The longest feedback message (`FEEDBACK_MAX_CHARS`).
pub const FEEDBACK_MAX_CHARS: usize = 4_000;

/// A window the operator opens over the workspace. Each is built by the
/// prompt named, which replaces the placeholder content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    /// Settings (074).
    Settings,
    /// Screens (075).
    Screens,
    /// Props editor (088).
    Props,
    /// Capture Settings (091).
    Capture,
    /// Song quick open (080).
    SongSearch,
}

impl Overlay {
    /// The window's title.
    pub fn title(self) -> &'static str {
        match self {
            Overlay::Settings => "Settings",
            Overlay::Screens => "Screens",
            Overlay::Props => "Props",
            Overlay::Capture => "Capture Settings",
            Overlay::SongSearch => "Search songs",
        }
    }

    /// The prompt that builds it.
    pub fn owner_prompt(self) -> &'static str {
        match self {
            Overlay::Settings => "074",
            Overlay::Screens => "075",
            Overlay::Props => "088",
            Overlay::Capture => "091",
            Overlay::SongSearch => "080",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedbackPhase {
    Idle,
    Sending,
    Sent,
}

/// The shell's retained state.
pub struct Shell {
    pub(crate) focus: FocusHandle,
    pub(crate) route: Route,
    pub(crate) mounted: HashSet<Route>,
    pub(crate) state: ShellState,
    pub(crate) version: SharedString,
    pub(crate) dev: bool,
    pub(crate) mac: bool,
    pub(crate) user_data: Option<PathBuf>,
    pub(crate) scale: UiScale,
    pub(crate) quit: QuitConfirmation,
    pub(crate) quit_open: bool,
    /// Focus for the quit question and What's New (Radix dialog behaviour).
    pub(crate) quit_focus: kairo_ui_kit::components::ModalFocus,
    pub(crate) whats_new_focus: kairo_ui_kit::components::ModalFocus,
    /// "Keep Kairo open" takes initial focus (DESIGN.md, Quit confirmation).
    pub(crate) keep_open_focus: gpui_kit::FocusHandle,
    pub(crate) overlay: Option<Overlay>,
    pub(crate) whats_new: Option<ChangelogEntry>,
    pub(crate) whats_new_auto: Option<ChangelogEntry>,
    pub(crate) toast_version: Option<SharedString>,
    pub(crate) announced: Option<SharedString>,
    pub(crate) toast_task: Option<Task<()>>,
    pub(crate) keychain_dismissed: bool,
    pub(crate) update_confirming: bool,
    pub(crate) feedback_open: bool,
    pub(crate) feedback_kind: FeedbackKind,
    pub(crate) feedback_input: Entity<TextareaState>,
    pub(crate) pending_feedback_clear: bool,
    pub(crate) feedback_phase: FeedbackPhase,
    pub(crate) feedback_error: Option<SharedString>,
    pub(crate) feedback_task: Option<Task<()>>,
    pub(crate) ready: bool,
    pub(crate) pending_import: Option<ImportKind>,
    pub(crate) pending_files: Vec<PathBuf>,
    pub(crate) crashes: HashMap<Route, (CrashGuard, SharedString, bool)>,
    pub(crate) launch: Launch,
    pub(crate) launch_epoch: Option<Instant>,
    pub(crate) step_since: Instant,
    pub(crate) launch_tasks: Vec<Task<()>>,
    pub(crate) seen_openings: u64,
    pub(crate) opened_at: Option<Instant>,
    /// Workspaces whose prompts have built them; the rest show a placeholder.
    pub(crate) workspaces: HashMap<Route, AnyView>,
    /// Drawn over everything but the quit question once launched: setup,
    /// the sign-in wall and the coach tips (073).
    pub(crate) front: Option<AnyView>,
    /// Builds an overlay window's view when it opens (074 onward): the
    /// pane to open first, if one was asked for.
    pub(crate) overlay_builders: HashMap<Overlay, OverlayBuilder>,
    /// The open overlay's built view.
    pub(crate) overlay_view: Option<AnyView>,
    /// The media dock is open (the Media button shows pressed).
    pub(crate) media_open: bool,
    /// What the dock has on screen, for the button's dot and tooltip.
    pub(crate) media_live: Option<SharedString>,
    /// The Settings pane asked for by the last [`Shell::open_settings`].
    pub(crate) settings_section: Option<&'static str>,
    /// The Screen Themes popover's body and what to do when it opens (the
    /// argument says whether the Lyrics page is showing).
    pub(crate) screen_themes: Option<(AnyView, ScreenThemesOpened)>,
    pub(crate) screen_themes_open: bool,
    /// The `.kairo` transfer host: review, export picker, notice (091).
    pub(crate) transfer: Option<AnyView>,
    /// A window the shell does not own covers the workspace.
    pub(crate) covered: bool,
    /// The Live button's popover body (091).
    pub(crate) live_menu: Option<AnyView>,
    pub(crate) live_open: bool,
    pub(crate) live_tick: Option<Task<()>>,
    /// First-run migration results that need the operator (100).
    pub(crate) migration_notes: Vec<SharedString>,
    pub(crate) _subscriptions: Vec<Subscription>,
}

/// Called as the Screen Themes popover opens; `true` on the Lyrics page.
pub type ScreenThemesOpened = std::rc::Rc<dyn Fn(bool, &mut gpui_kit::Window, &mut gpui_kit::App)>;

impl EventEmitter<ShellRequest> for Shell {}

/// Builds an overlay window's view; the argument is the Settings pane asked
/// for (`openSettings(section)`), if any.
pub type OverlayBuilder = Box<dyn Fn(Option<&'static str>, &mut Window, &mut App) -> AnyView>;

/// What the app supplies at startup.
#[derive(Clone, Debug)]
pub struct ShellOptions {
    /// The running version (`CARGO_PKG_VERSION`).
    pub version: SharedString,
    /// A development build: What's New previews drafts and never records.
    pub dev: bool,
    /// The profile's user-data folder, for the interface scale.
    pub user_data: Option<PathBuf>,
    /// Show the launch screen until [`Shell::begin_launch`]'s bootstrap
    /// answers. Without it the shell opens at once with nothing loaded.
    pub splash: bool,
}

impl Shell {
    /// The shell for a window.
    pub fn new(options: ShellOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let feedback_input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(5)
                .placeholder(placeholder(FeedbackKind::Idea))
        });
        let scale = options
            .user_data
            .as_deref()
            .map(UiScale::load)
            .unwrap_or_default();
        apply_scale(scale, cx);
        let subscriptions = vec![
            // Coming back to the app: pull secrets, wake uploaders.
            cx.observe_window_activation(window, |_, window, cx| {
                if window.is_window_active() {
                    cx.emit(ShellRequest::WindowFocused);
                }
            }),
            // Full screen changes the header's corner.
            cx.observe_window_bounds(window, |_, _, cx| cx.notify()),
        ];
        let mut mounted = HashSet::new();
        mounted.insert(Route::Operator);
        Self {
            focus: cx.focus_handle(),
            route: Route::Operator,
            mounted,
            state: ShellState::default(),
            version: options.version,
            dev: options.dev,
            mac: cfg!(target_os = "macos"),
            user_data: options.user_data,
            scale,
            quit: QuitConfirmation::default(),
            quit_open: false,
            quit_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            whats_new_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            keep_open_focus: cx.focus_handle(),
            overlay: None,
            whats_new: None,
            whats_new_auto: None,
            toast_version: None,
            announced: None,
            toast_task: None,
            keychain_dismissed: false,
            update_confirming: false,
            feedback_open: false,
            feedback_kind: FeedbackKind::Idea,
            feedback_input,
            pending_feedback_clear: false,
            feedback_phase: FeedbackPhase::Idle,
            feedback_error: None,
            feedback_task: None,
            ready: false,
            pending_import: None,
            pending_files: Vec::new(),
            crashes: HashMap::new(),
            launch: if options.splash {
                Launch::new()
            } else {
                Launch::skipped()
            },
            launch_epoch: None,
            step_since: cx.background_executor().now(),
            launch_tasks: Vec::new(),
            seen_openings: if options.splash { 0 } else { 1 },
            opened_at: None,
            workspaces: HashMap::new(),
            front: None,
            overlay_builders: HashMap::new(),
            overlay_view: None,
            media_open: false,
            screen_themes: None,
            screen_themes_open: false,
            transfer: None,
            covered: false,
            live_menu: None,
            live_open: false,
            live_tick: None,
            migration_notes: Vec::new(),
            media_live: None,
            settings_section: None,
            _subscriptions: subscriptions,
        }
    }

    /// The media dock's state, for the header's Media button.
    pub fn set_media(&mut self, open: bool, live: Option<String>, cx: &mut Context<Self>) {
        self.media_open = open;
        self.media_live = live.map(Into::into);
        cx.notify();
    }

    /// Whether the header shows the media dock open, and what is live.
    pub fn media(&self) -> (bool, Option<&SharedString>) {
        (self.media_open, self.media_live.as_ref())
    }

    // ── Reading ─────────────────────────────────────────────────────────

    /// The workspace showing.
    pub fn route(&self) -> Route {
        self.route
    }

    /// The open overlay window, if any.
    /// Mounts the `.kairo` transfer host over the window (091).
    pub fn set_transfer(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.transfer = view;
        cx.notify();
    }

    /// A window the shell does not own (the transfer host) covers the workspace.
    pub fn set_covered(&mut self, covered: bool, cx: &mut Context<Self>) {
        if self.covered != covered {
            self.covered = covered;
            cx.notify();
        }
    }

    /// Whether anything covers the workspace: an overlay or another window.
    pub fn covered(&self) -> bool {
        self.covered || self.overlay.is_some()
    }

    /// Gives the Live button its popover: capture and Syphon (091).
    pub fn set_live_menu(&mut self, view: AnyView, cx: &mut Context<Self>) {
        self.live_menu = Some(view);
        cx.notify();
    }

    /// Shows what the upgrade from the previous version needs the operator
    /// to do; empty hides it.
    pub fn set_migration_notes(&mut self, notes: Vec<String>, cx: &mut Context<Self>) {
        self.migration_notes = notes.into_iter().map(Into::into).collect();
        cx.notify();
    }

    pub fn migration_notes(&self) -> &[SharedString] {
        &self.migration_notes
    }

    /// The Live button's detail line.
    pub fn live_detail(&self) -> SharedString {
        self.state.live_text().1
    }

    /// Opens or closes the Live popover.
    pub fn set_live_open(&mut self, open: bool, cx: &mut Context<Self>) {
        if self.live_open != open {
            self.live_open = open;
            cx.notify();
        }
    }

    /// Opens Screens at a target (`openScreens(target)`): `add:syphon`,
    /// `kind:syphon`, or an output id.
    pub fn open_screens(&mut self, target: &'static str, cx: &mut Context<Self>) {
        self.overlay_view = None;
        self.overlay = Some(Overlay::Screens);
        self.settings_section = Some(target);
        cx.notify();
    }

    pub fn overlay(&self) -> Option<Overlay> {
        self.overlay
    }

    /// Whether the quit question is showing.
    pub fn quit_open(&self) -> bool {
        self.quit_open
    }

    /// The What's New entry showing, if any.
    pub fn whats_new(&self) -> Option<&ChangelogEntry> {
        self.whats_new.as_ref()
    }

    /// The interface scale.
    pub fn scale(&self) -> UiScale {
        self.scale
    }

    /// The state the app last supplied.
    pub fn state(&self) -> &ShellState {
        &self.state
    }

    /// The launch screen's state.
    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    // ── Launch ──────────────────────────────────────────────────────────

    /// Starts the launch: asks for the startup snapshot
    /// ([`ShellRequest::Bootstrap`]) and starts the splash's clock. Once per
    /// window; later calls do nothing.
    pub fn begin_launch(&mut self, cx: &mut Context<Self>) {
        let Some(generation) = self.launch.start() else {
            return;
        };
        let now = cx.background_executor().now();
        self.launch_epoch = Some(now);
        self.step_since = now;
        cx.emit(ShellRequest::Bootstrap { generation });
        // The splash's floor under reduced motion, otherwise the fallback in
        // case the intro never reports (a hidden window draws no frames).
        let hold = if kairo_ui_kit::KairoThemeExt::reduced_motion(&**cx) {
            SPLASH_MIN_VISIBLE_MS
        } else {
            SPLASH_FALLBACK_MS
        };
        self.schedule_hold(hold, cx);
        cx.notify();
    }

    /// Lets the splash close after `ms`.
    pub(crate) fn schedule_hold(&mut self, ms: u64, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.launch.hold_splash() {
                    cx.notify();
                }
            });
        });
        self.launch_tasks.push(task);
    }

    /// Progress from the load `generation` asked for.
    pub fn bootstrap_progress(
        &mut self,
        generation: u64,
        progress: kairo_domain::status::BootstrapProgress,
        cx: &mut Context<Self>,
    ) {
        let step_changed = self.launch.current_progress().step != progress.step;
        if self.launch.progress(generation, progress) {
            if step_changed {
                self.step_since = cx.background_executor().now();
            }
            cx.notify();
        }
    }

    /// The result of the load `generation` asked for. An error opens the
    /// app with a warning naming settings.
    pub fn bootstrap_finished(
        &mut self,
        generation: u64,
        result: Result<kairo_domain::status::AppBootstrapSnapshot, String>,
        cx: &mut Context<Self>,
    ) {
        if self.launch.finish(generation, result) {
            cx.notify();
        }
    }

    /// Retry from the warning bar: the progress screen shows again.
    pub fn retry_bootstrap(&mut self, cx: &mut Context<Self>) {
        let generation = self.launch.retry();
        self.step_since = cx.background_executor().now();
        cx.emit(ShellRequest::Bootstrap { generation });
        cx.notify();
    }

    /// Hides the warning bar until the next load.
    pub fn dismiss_startup_warning(&mut self, cx: &mut Context<Self>) {
        self.launch.dismiss_warning();
        cx.notify();
    }

    // ── Feeding ─────────────────────────────────────────────────────────

    /// Replaces the service state.
    pub fn set_state(&mut self, state: ShellState, cx: &mut Context<Self>) {
        let first_settings =
            self.state.whats_new_last_seen.is_none() && state.whats_new_last_seen.is_some();
        self.state = state;
        // The Live label counts the recording up once a second.
        if !self.state.recording {
            self.live_tick = None;
        } else if self.live_tick.is_none() {
            self.live_tick = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            }));
        }
        if first_settings {
            self.check_whats_new_after_update(cx);
        }
        self.announce_update(cx);
        self.show_pending_whats_new(cx);
        if self.state.is_live() {
            self.toast_version = None;
        }
        cx.notify();
    }

    /// The UI is ready: deliver anything the menu or the OS sent before.
    pub fn mark_ready(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ready = true;
        if let Some(kind) = self.pending_import.take() {
            self.deliver_import(kind, window, cx);
        }
        if !self.pending_files.is_empty() {
            cx.emit(ShellRequest::OpenKairoFiles(std::mem::take(
                &mut self.pending_files,
            )));
        }
    }

    /// `.kairo` files from Finder, Explorer or a second launch.
    pub fn open_kairo_files(&mut self, files: Vec<PathBuf>, cx: &mut Context<Self>) {
        if files.is_empty() {
            return;
        }
        if self.ready {
            cx.emit(ShellRequest::OpenKairoFiles(files));
        } else {
            self.pending_files.extend(files);
        }
    }

    /// The answer to a [`ShellRequest::SendFeedback`]: `None` when sent,
    /// else the message to show.
    pub fn feedback_result(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        match error {
            Some(error) => {
                self.feedback_phase = FeedbackPhase::Idle;
                self.feedback_error = Some(error);
            }
            None => {
                self.feedback_phase = FeedbackPhase::Sent;
                self.feedback_task = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(FEEDBACK_CLOSE_AFTER).await;
                    let _ = this.update(cx, |this, cx| this.close_feedback(cx));
                }));
            }
        }
        cx.notify();
    }

    // ── Navigation ──────────────────────────────────────────────────────

    /// Hands a route its real workspace view in place of the placeholder.
    pub fn set_workspace(&mut self, route: Route, view: AnyView, cx: &mut Context<Self>) {
        self.workspaces.insert(route, view);
        cx.notify();
    }

    /// Gives the header's Screen Themes button its popover (prompt 087).
    pub fn set_screen_themes(
        &mut self,
        view: AnyView,
        opened: ScreenThemesOpened,
        cx: &mut Context<Self>,
    ) {
        self.screen_themes = Some((view, opened));
        cx.notify();
    }

    /// Opens or closes the Screen Themes popover.
    pub fn set_screen_themes_open(
        &mut self,
        open: bool,
        window: &mut gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        if open == self.screen_themes_open {
            return;
        }
        self.screen_themes_open = open;
        if open && let Some((_, opened)) = &self.screen_themes {
            let opened = opened.clone();
            let lyrics = self.route == Route::Lyrics;
            cx.defer_in(window, move |_, window, cx| opened(lyrics, window, cx));
        }
        cx.notify();
    }

    pub fn screen_themes_open(&self) -> bool {
        self.screen_themes_open
    }

    /// Sets the layer drawn in front of the booth.
    pub fn set_front(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.front = view;
        cx.notify();
    }

    /// Shows a workspace.
    pub fn navigate(&mut self, route: Route, cx: &mut Context<Self>) {
        self.route = route;
        self.mounted.insert(route);
        cx.notify();
    }

    /// Installs the view an overlay window shows in place of its placeholder.
    pub fn set_overlay_builder(&mut self, overlay: Overlay, builder: OverlayBuilder) {
        self.overlay_builders.insert(overlay, builder);
    }

    /// Opens an overlay window (one at a time).
    pub fn open_overlay(&mut self, overlay: Overlay, cx: &mut Context<Self>) {
        if self.overlay != Some(overlay) {
            self.overlay_view = None;
        }
        if matches!(overlay, Overlay::Settings | Overlay::Screens) {
            self.settings_section = None;
        }
        self.overlay = Some(overlay);
        cx.notify();
    }

    /// Opens Settings at a pane (`openSettings(section)`): `general`,
    /// `apikeys`, `account`, `audio`, `scripture`, …
    pub fn open_settings(&mut self, section: &'static str, cx: &mut Context<Self>) {
        self.overlay_view = None;
        self.overlay = Some(Overlay::Settings);
        self.settings_section = Some(section);
        cx.notify();
    }

    /// Closes the overlay window.
    pub fn close_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = None;
        self.overlay_view = None;
        cx.notify();
    }

    pub(crate) fn deliver_import(
        &mut self,
        kind: ImportKind,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Settings closes so the import is seen.
        if self.overlay == Some(Overlay::Settings) {
            self.overlay = None;
            self.overlay_view = None;
        }
        if let Some(route) = import_route(kind) {
            self.navigate(route, cx);
        }
        cx.emit(ShellRequest::Import(kind));
    }

    // ── Quit ────────────────────────────────────────────────────────────

    /// A quit or window close: asks once, and returns whether it may go
    /// ahead now.
    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut prompted = false;
        let allowed = self.quit.confirm(|| prompted = true);
        if prompted {
            self.quit_open = true;
            window.activate_window();
            cx.notify();
        }
        allowed
    }

    /// The operator's answer to the quit question.
    pub fn respond_to_quit(&mut self, confirmed: bool, cx: &mut Context<Self>) {
        let quit = self.quit.respond(confirmed);
        self.quit_open = false;
        cx.notify();
        if quit {
            cx.quit();
        }
    }

    /// Lets the next quit through without asking (the updater's install).
    pub fn allow_quit(&mut self) {
        self.quit.allow();
    }

    // ── Scale ───────────────────────────────────────────────────────────

    pub(crate) fn set_scale(&mut self, scale: UiScale, cx: &mut Context<Self>) {
        if scale == self.scale {
            return;
        }
        self.scale = scale;
        apply_scale(scale, cx);
        if let Some(dir) = &self.user_data {
            scale.save(dir);
        }
        cx.notify();
    }

    // ── What's New ──────────────────────────────────────────────────────

    pub(crate) fn check_whats_new_after_update(&mut self, cx: &mut Context<Self>) {
        // Development builds never auto-show or record anything.
        if self.dev {
            return;
        }
        let last_seen = self.state.whats_new_last_seen.clone().unwrap_or_default();
        match whats_new::entry_after_update(&last_seen, &self.version, whats_new::changelog()) {
            Some(entry) => self.whats_new_auto = Some(entry.clone()),
            // Fresh install, or an update with no notes: nothing to show, just remember.
            None if last_seen != self.version => {
                cx.emit(ShellRequest::MarkWhatsNewSeen(self.version.clone()))
            }
            None => {}
        }
    }

    pub(crate) fn show_pending_whats_new(&mut self, cx: &mut Context<Self>) {
        // The automatic sheet waits out a live session.
        if self.whats_new.is_some() || self.state.is_live() {
            return;
        }
        if let Some(entry) = self.whats_new_auto.take() {
            self.whats_new = Some(entry);
            // Seen the moment it shows, so quitting with it open does not bring it back.
            if !self.dev {
                cx.emit(ShellRequest::MarkWhatsNewSeen(self.version.clone()));
            }
        }
    }

    /// Opens What's New from the menu.
    pub fn open_whats_new(&mut self, cx: &mut Context<Self>) {
        self.whats_new =
            whats_new::entry_on_request(&self.version, self.dev, whats_new::changelog()).cloned();
        cx.notify();
    }

    pub(crate) fn close_whats_new(&mut self, cx: &mut Context<Self>) {
        self.whats_new = None;
        self.whats_new_auto = None;
        if !self.dev {
            cx.emit(ShellRequest::MarkWhatsNewSeen(self.version.clone()));
        }
        cx.notify();
    }

    // ── Updates ─────────────────────────────────────────────────────────

    pub(crate) fn announce_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Downloaded { version } = &self.state.update else {
            return;
        };
        if self.state.is_live() || self.announced.as_ref() == Some(version) {
            return;
        }
        self.announced = Some(version.clone());
        self.toast_version = Some(version.clone());
        self.toast_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            let _ = this.update(cx, |this, cx| {
                this.toast_version = None;
                cx.notify();
            });
        }));
    }

    // ── Feedback ────────────────────────────────────────────────────────

    pub(crate) fn close_feedback(&mut self, cx: &mut Context<Self>) {
        self.feedback_open = false;
        self.feedback_error = None;
        // After a thank-you the next open starts fresh; a draft survives a stray click away.
        if self.feedback_phase == FeedbackPhase::Sent {
            self.feedback_phase = FeedbackPhase::Idle;
            self.feedback_kind = FeedbackKind::Idea;
            self.feedback_task = None;
            self.reset_feedback_text(cx);
        }
        cx.notify();
    }

    pub(crate) fn reset_feedback_text(&mut self, cx: &mut Context<Self>) {
        // Editing the field needs a window; the next render does it.
        self.pending_feedback_clear = true;
        cx.notify();
    }

    pub(crate) fn send_feedback(&mut self, cx: &mut Context<Self>) {
        let text: String = self
            .feedback_input
            .read(cx)
            .value()
            .trim()
            .chars()
            .take(FEEDBACK_MAX_CHARS)
            .collect();
        if !self.state.account.signed_in
            || text.is_empty()
            || self.feedback_phase != FeedbackPhase::Idle
        {
            return;
        }
        self.feedback_phase = FeedbackPhase::Sending;
        self.feedback_error = None;
        cx.emit(ShellRequest::SendFeedback {
            kind: self.feedback_kind,
            message: text,
            context: self.route,
        });
        cx.notify();
    }

    // ── Keys ────────────────────────────────────────────────────────────

    pub(crate) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Escape closes the topmost shell window; on the quit question it
        // keeps Kairo open.
        if event.keystroke.key == "escape" && self.quit_open {
            self.respond_to_quit(false, cx);
            cx.stop_propagation();
            return;
        }
        if event.keystroke.key == "escape" {
            if self.whats_new.is_some() {
                self.close_whats_new(cx);
                cx.stop_propagation();
                return;
            }
            if self.overlay.is_some() {
                self.close_overlay(cx);
                cx.stop_propagation();
                return;
            }
        }
        if self.whats_new.is_some() && event.keystroke.key == "enter" {
            self.close_whats_new(cx);
            cx.stop_propagation();
            return;
        }
        // Workspace commands never fire while a text field has focus.
        let typing = window.context_stack().iter().any(|c| c.contains("Input"));
        if typing || self.quit_open || self.overlay.is_some() || self.whats_new.is_some() {
            return;
        }
        if let Some(command) =
            keymap::command_for_keystroke(&event.keystroke, &self.state.shortcuts)
        {
            window.dispatch_action(
                Box::new(keymap::WorkspaceCommand(keymap::CommandValue(command))),
                cx,
            );
        }
    }
}

pub(crate) fn placeholder(kind: FeedbackKind) -> &'static str {
    match kind {
        FeedbackKind::Idea => "What would make Sunday easier? A feature, a shortcut, anything.",
        FeedbackKind::Problem => {
            "What happened, and what did you expect? Steps help us fix it fast."
        }
        FeedbackKind::Praise => "What do you love? It tells us what to keep.",
    }
}

/// The View menu's zoom on top of the operator's interface size
/// (`display.fontSize`, 074).
fn apply_scale(scale: UiScale, cx: &mut App) {
    let sizes = kairo_ui_kit::theme::DisplaySizes {
        zoom: scale.factor(),
        ..kairo_ui_kit::theme::display_sizes(cx)
    };
    kairo_ui_kit::theme::set_display_sizes(sizes, cx);
    let base = sizes.interface_rem();
    Theme::update(cx, |theme| theme.font_size = base);
}

impl Focusable for Shell {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
