//! Installs Kairo's tokens into the GPUI Kit theme.
//!
//! Kit components read `cx.theme()`; Kairo components also read
//! [`KairoThemeExt::kairo`]. [`set_appearance`] keeps both in step, the way
//! `applyAppTheme` switches `data-theme` in Electron.

use crate::tokens::{Appearance, KairoColors, Step, TextStep, radius, text};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, Global, SharedString, Window};

/// The active Kairo tokens.
#[derive(Clone, Debug)]
pub struct KairoTheme {
    colors: KairoColors,
    reduced_motion: bool,
}

impl Global for KairoTheme {}

impl KairoTheme {
    /// The active colour tokens.
    pub fn colors(&self) -> &KairoColors {
        &self.colors
    }

    /// The active appearance.
    pub fn appearance(&self) -> Appearance {
        self.colors.appearance
    }

    /// Whether motion should be skipped (`prefers-reduced-motion`).
    pub fn reduced_motion(&self) -> bool {
        self.reduced_motion
    }
}

/// Reads the Kairo tokens from any context that can read globals.
pub trait KairoThemeExt {
    /// The active Kairo colour tokens.
    fn kairo(&self) -> &KairoColors;
    /// Whether motion should be skipped.
    fn reduced_motion(&self) -> bool;
}

impl KairoThemeExt for App {
    fn kairo(&self) -> &KairoColors {
        self.global::<KairoTheme>().colors()
    }

    fn reduced_motion(&self) -> bool {
        self.global::<KairoTheme>().reduced_motion()
    }
}

/// Installs fonts, icons and the theme. Call once, after `gpui_kit::init`.
pub fn init(appearance: Appearance, cx: &mut App) {
    crate::fonts::register(cx);
    cx.set_global(KairoTheme {
        colors: KairoColors::for_appearance(appearance),
        reduced_motion: false,
    });
    set_appearance(appearance, None, cx);
}

/// Switches appearance immediately and refreshes every window.
pub fn set_appearance(appearance: Appearance, window: Option<&mut Window>, cx: &mut App) {
    let colors = KairoColors::for_appearance(appearance);
    let reduced_motion = cx
        .try_global::<KairoTheme>()
        .is_some_and(KairoTheme::reduced_motion);
    cx.set_global(KairoTheme {
        colors,
        reduced_motion,
    });
    let mode = if appearance.is_dark() {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    Theme::change(mode, window, cx);
    Theme::update(cx, |theme| apply_tokens(theme, &colors));
}

/// Turns motion off or on (`prefers-reduced-motion: reduce`).
pub fn set_reduced_motion(reduced: bool, cx: &mut App) {
    if let Some(theme) = cx.try_global::<KairoTheme>().cloned() {
        cx.set_global(KairoTheme {
            reduced_motion: reduced,
            ..theme
        });
        cx.refresh_windows();
    }
}

/// Maps Kairo tokens onto Kit's semantic colours, following the shadcn
/// mapping in `tailwind.config.js` (`primary` is the accent, `secondary` and
/// `accent` are the tertiary surface, `popover` is the elevated surface).
fn apply_tokens(theme: &mut Theme, k: &KairoColors) {
    theme.font_family = SharedString::new_static(text::FAMILY);
    theme.font_size = text::rem();
    theme.radius = radius::md();
    theme.radius_lg = radius::lg();
    // Flat: depth comes from surface steps, never drop shadows.
    theme.shadow = false;

    let c = &mut theme.colors;
    let accent = k.control_accent;
    let hover = k.accent(Step::S600);
    let active = k.accent(Step::S700);
    let fg = k.text_primary;
    let muted_fg = k.neutral(Step::S400);

    c.background = k.surface;
    c.foreground = fg;
    c.border = k.surface_border;
    c.input = k.surface_border;
    c.ring = accent;
    c.caret = fg;
    c.selection = k.accent(Step::S500).opacity(0.35);
    c.overlay = k.scrim();
    c.window_border = k.surface_border;

    c.primary = accent;
    c.primary_hover = hover;
    c.primary_active = active;
    c.primary_foreground = k.on_accent;

    c.secondary = k.surface_tertiary;
    c.secondary_hover = k.surface_elevated;
    c.secondary_active = k.surface_elevated;
    c.secondary_foreground = fg;

    c.muted = k.surface_secondary;
    c.muted_foreground = muted_fg;
    c.accent = k.surface_tertiary;
    c.accent_foreground = fg;

    c.popover = k.surface_elevated;
    c.popover_foreground = fg;

    c.button = k.surface_secondary;
    c.button_hover = k.surface_tertiary;
    c.button_active = k.surface_tertiary;
    c.button_foreground = fg;
    c.button_primary = accent;
    c.button_primary_hover = hover;
    c.button_primary_active = active;
    c.button_primary_foreground = k.on_accent;
    c.button_secondary = k.surface_tertiary;
    c.button_secondary_hover = k.surface_elevated;
    c.button_secondary_active = k.surface_elevated;
    c.button_secondary_foreground = fg;
    // `variant="destructive"`: the red tint with red text, not a solid red.
    c.button_danger = k.tints.red;
    c.button_danger_hover = k.tints.red;
    c.button_danger_active = k.tints.red;
    c.button_danger_foreground = k.destructive;

    c.danger = k.destructive;
    c.danger_hover = k.destructive;
    c.danger_active = k.destructive;
    c.danger_foreground = k.white;
    c.success = k.success_text;
    c.success_foreground = k.white;

    c.link = accent;
    c.link_hover = hover;
    c.link_active = active;

    c.list = k.surface;
    c.list_hover = k.surface_tertiary;
    c.list_active = k.row_selected();
    c.list_active_border = k.row_selected();
    c.list_even = k.surface;
    c.list_head = k.surface_secondary;

    c.table = k.surface;
    c.table_hover = k.surface_tertiary;
    c.table_active = k.row_selected();
    c.table_active_border = k.row_selected();
    c.table_even = k.surface;
    c.table_head = k.surface_secondary;
    c.table_head_foreground = k.text(TextStep::T400);
    c.table_row_border = k.panel_border;

    c.sidebar = k.sidebar_list;
    c.sidebar_foreground = fg;
    c.sidebar_border = k.panel_border;
    c.sidebar_accent = k.sidebar_pick_list;
    c.sidebar_accent_foreground = fg;
    c.sidebar_primary = accent;
    c.sidebar_primary_foreground = k.on_accent;

    // `bg-surface p-0.5` strip with the chosen option on the elevated step.
    c.tab_bar = k.surface;
    c.tab_bar_segmented = k.surface;
    c.tab = k.surface;
    c.tab_active = k.surface_elevated;
    c.tab_foreground = k.text(TextStep::T500);
    c.tab_active_foreground = fg;

    c.title_bar = k.surface_header;
    c.title_bar_border = k.panel_border;
    c.status_bar = k.surface_header;
    c.status_bar_border = k.panel_border;

    c.switch = k.surface_border;
    c.switch_thumb = k.white;
    c.slider_bar = accent;
    c.slider_thumb = k.white;
    c.progress_bar = accent;
    c.skeleton = k.surface_tertiary;

    c.scrollbar = k.surface.opacity(0.0);
    c.scrollbar_thumb = k.neutral(Step::S700);
    c.scrollbar_thumb_hover = k.neutral(Step::S600);

    c.group_box = k.surface_secondary;
    c.group_box_foreground = fg;
    c.drag_border = accent;
    c.drop_target = k.surface_border;
    c.description_list_label = k.surface_secondary;
    c.description_list_label_foreground = k.text(TextStep::T400);

    c.red = k.destructive;
    c.blue = accent;
}

/// The window background behind every surface (`html, body { background }`).
pub fn window_background(cx: &App) -> gpui_kit::Hsla {
    cx.kairo().surface
}

/// The base text size for body copy (`body { font-size: 0.9375rem }`).
pub fn body_text_size() -> gpui_kit::Rems {
    text::BASE.size
}
