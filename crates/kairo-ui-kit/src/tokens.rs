//! Kairo's design tokens.
//!
//! Every value comes from `../kairo-presenter/src/renderer/src/index.css`
//! (`:root` for dark, `[data-theme='light']` for light) and the scales in
//! `tailwind.config.js`. This is the only module in the workspace that holds
//! raw colour values; everything else reads them through [`KairoColors`].

use gpui_kit::{Hsla, Rgba, px, rems};
use gpui_kit::{Pixels, Rems};

/// One `r g b` triple from the stylesheet.
fn rgb(r: u8, g: u8, b: u8) -> Hsla {
    rgba(r, g, b, 1.0)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Hsla {
    Rgba {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
        a,
    }
    .into()
}

/// Light or dark, as `data-theme` sets it (`lib/appTheme.ts`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// The default (`:root`).
    #[default]
    Dark,
    /// `[data-theme='light']`.
    Light,
}

impl Appearance {
    /// Parses the stored setting (`settings.display.theme`). Anything other
    /// than `light` is dark, as `applyAppTheme` only ever writes these two.
    pub fn from_setting(value: &str) -> Self {
        if value == "light" {
            Self::Light
        } else {
            Self::Dark
        }
    }

    /// The stored setting value.
    pub fn as_setting(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Whether this is the dark appearance.
    pub fn is_dark(self) -> bool {
        self == Self::Dark
    }
}

/// The tool hues (`--hue-*`, `lib/hue.ts`): one colour per tool, so a colour
/// means the same thing in the rail, header, toolbox and docks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hue {
    /// Green.
    Green,
    /// Orange.
    Orange,
    /// Violet (songs).
    Violet,
    /// Cyan (documents).
    Cyan,
    /// Light sky blue (scripture), lighter than the accent.
    Blue,
    /// Pink.
    Pink,
    /// Coral (audience).
    Coral,
    /// Lime.
    Lime,
    /// Amber.
    Amber,
    /// Indigo.
    Indigo,
}

impl Hue {
    /// Every hue, in stylesheet order.
    pub const ALL: [Hue; 10] = [
        Hue::Green,
        Hue::Orange,
        Hue::Violet,
        Hue::Cyan,
        Hue::Blue,
        Hue::Pink,
        Hue::Coral,
        Hue::Lime,
        Hue::Amber,
        Hue::Indigo,
    ];

    /// The stylesheet name (`--hue-<name>`).
    pub fn name(self) -> &'static str {
        match self {
            Hue::Green => "green",
            Hue::Orange => "orange",
            Hue::Violet => "violet",
            Hue::Cyan => "cyan",
            Hue::Blue => "blue",
            Hue::Pink => "pink",
            Hue::Coral => "coral",
            Hue::Lime => "lime",
            Hue::Amber => "amber",
            Hue::Indigo => "indigo",
        }
    }
}

/// The solid tints (`--tint-*`): an opaque wash per theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tints {
    /// `--tint-live`: a live row.
    pub live: Hsla,
    /// `--tint-teal`.
    pub teal: Hsla,
    /// `--tint-red`: the destructive button fill.
    pub red: Hsla,
    /// `--tint-amber`.
    pub amber: Hsla,
    /// `--tint-yellow`.
    pub yellow: Hsla,
    /// `--tint-rose`: destructive hover in the selection bar.
    pub rose: Hsla,
}

/// Every Kairo colour token for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KairoColors {
    /// Which appearance these are.
    pub appearance: Appearance,
    /// Solid chrome: the system asks for reduced transparency or increased
    /// contrast (`prefers-reduced-transparency`, `prefers-contrast: more`),
    /// so glass and material tints are drawn opaque. Set through
    /// [`crate::theme::set_solid_chrome`].
    pub solid_chrome: bool,
    /// `--surface`: the workspace.
    pub surface: Hsla,
    /// `--surface-secondary`: panels and cards.
    pub surface_secondary: Hsla,
    /// `--surface-tertiary`: controls.
    pub surface_tertiary: Hsla,
    /// `--surface-elevated`: lifted controls.
    pub surface_elevated: Hsla,
    /// `--surface-rail`: the workspace rail.
    pub surface_rail: Hsla,
    /// `--surface-header`: the top bar.
    pub surface_header: Hsla,
    /// `--surface-border`.
    pub surface_border: Hsla,
    /// `--surface-menu`: dropdown menus.
    pub surface_menu: Hsla,
    /// `--surface-menu-hover`.
    pub surface_menu_hover: Hsla,
    /// `--surface-menu-line`.
    pub surface_menu_line: Hsla,
    /// `--surface-line`: hairline between regions.
    pub surface_line: Hsla,
    /// `--panel-border-color`: 75% of `--surface-border`.
    pub panel_border: Hsla,
    /// `--surface-stage`: the band behind a screen canvas.
    pub surface_stage: Hsla,
    /// `--surface-display`: a display on the stage.
    pub surface_display: Hsla,
    /// `--surface-canvas`: the editor's pasteboard.
    pub surface_canvas: Hsla,
    /// `--sidebar-sources`.
    pub sidebar_sources: Hsla,
    /// `--sidebar-list`.
    pub sidebar_list: Hsla,
    /// `--sidebar-band`.
    pub sidebar_band: Hsla,
    /// `--sidebar-pick-sources`.
    pub sidebar_pick_sources: Hsla,
    /// `--sidebar-pick-sources-line`.
    pub sidebar_pick_sources_line: Hsla,
    /// `--sidebar-pick-list`.
    pub sidebar_pick_list: Hsla,
    /// `--sidebar-pick-list-line`.
    pub sidebar_pick_list_line: Hsla,
    /// `--sidebar-field`.
    pub sidebar_field: Hsla,
    /// `--control-accent`: what is picked and the primary action.
    pub control_accent: Hsla,
    /// `--accent-50` … `--accent-950` (the `teal-*` classes).
    pub accent: [Hsla; 11],
    /// `--on-accent`: text on a solid accent fill.
    pub on_accent: Hsla,
    /// `--live`: only what is live on screen.
    pub live: Hsla,
    /// `--hue-*`, in [`Hue::ALL`] order.
    pub hues: [Hsla; 10],
    /// `--tint-*`.
    pub tints: Tints,
    /// `--range-track`.
    pub range_track: Hsla,
    /// `--text-primary`.
    pub text_primary: Hsla,
    /// `--fg-200` … `--fg-600`: grey text (`text-zinc-*`, `text-slate-*`).
    pub fg: [Hsla; 5],
    /// `--neutral-50` … `--neutral-950`: grey fills and borders.
    pub neutral: [Hsla; 11],
    /// `--glass`: the frosted tone of a modal sidebar or small dialog.
    pub glass: Hsla,
    /// `stone` in `tailwind.config.js` (`#374151`): the account avatar's
    /// fill, the same in both themes.
    pub stone: Hsla,
    /// `destructive` in `tailwind.config.js` (`#ef4444`).
    pub destructive: Hsla,
    /// `text-red-400` / `text-rose-400` (`#b91c1c` in light).
    pub danger_text: Hsla,
    /// `text-rose-300`: the selection bar's destructive action.
    pub danger_text_soft: Hsla,
    /// `text-emerald-400` (`#047857` in light).
    pub success_text: Hsla,
    /// `text-yellow-300`: the startup warning bar (stock in both themes).
    pub warning_text: Hsla,
    /// The scripture grid card's checkerboard, light squares (`#303030`), both themes.
    pub checker_light: Hsla,
    /// The checkerboard's dark squares (`#242424`), both themes.
    pub checker_dark: Hsla,
    /// `paper` (`#F9FAFB`): the sermon notes page, both themes.
    pub paper: Hsla,
    /// `text-yellow-400` (`#FACC15`): a favourite song's star, both themes.
    pub favorite: Hsla,
    /// A detected reference in the sermon notes (`#CAD5E2`), both themes.
    pub sermon_mark: Hsla,
    /// The active reference and the notes selection (`#B9D7FF`), both themes.
    pub sermon_mark_active: Hsla,
    /// `text-teal-400`: a quiet positive notice (password reset sent, Get a key).
    pub notice_text: Hsla,
    /// `rgb(113 113 122)`: the launch screen's "Powering up" line, and the
    /// muted ends of its light band (`.splash-boot`).
    pub splash_boot: Hsla,
    /// Pure white: the marquee band, slider thumbs and switch thumbs.
    pub white: Hsla,
    /// macOS traffic-light close (`#FF5F57`).
    pub traffic_close: Hsla,
    /// The glyph on the close light (`#4d0000`).
    pub traffic_close_glyph: Hsla,
    /// macOS traffic-light zoom (`#28C840`).
    pub traffic_zoom: Hsla,
    /// The glyph on the zoom light (`#0b4a14`).
    pub traffic_zoom_glyph: Hsla,
    /// Onboarding text (`--ob-text`).
    pub onboarding_text: Hsla,
    /// Onboarding secondary text (`--ob-text-deep`).
    pub onboarding_text_deep: Hsla,
}

/// Index of a `50 … 950` step in an 11-step scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 50.
    S50,
    /// 100.
    S100,
    /// 200.
    S200,
    /// 300.
    S300,
    /// 400.
    S400,
    /// 500.
    S500,
    /// 600.
    S600,
    /// 700.
    S700,
    /// 800.
    S800,
    /// 900.
    S900,
    /// 950.
    S950,
}

impl Step {
    fn index(self) -> usize {
        self as usize
    }
}

/// The grey text steps (`--fg-*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextStep {
    /// `--fg-200`.
    T200,
    /// `--fg-300`.
    T300,
    /// `--fg-400`.
    T400,
    /// `--fg-500`.
    T500,
    /// `--fg-600`.
    T600,
}

impl KairoColors {
    /// The tokens for an appearance.
    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        }
    }

    /// `:root`.
    pub fn dark() -> Self {
        let surface_border = rgb(56, 61, 74);
        Self {
            appearance: Appearance::Dark,
            solid_chrome: false,
            surface: rgb(18, 20, 24),
            surface_secondary: rgb(30, 33, 40),
            surface_tertiary: rgb(40, 44, 53),
            surface_elevated: rgb(52, 57, 68),
            surface_rail: rgb(28, 31, 38),
            surface_header: rgb(25, 27, 33),
            surface_border,
            surface_menu: rgb(36, 40, 48),
            surface_menu_hover: rgb(49, 54, 65),
            surface_menu_line: rgb(56, 61, 74),
            surface_line: rgb(10, 11, 14),
            panel_border: surface_border.opacity(0.85),
            surface_stage: rgb(10, 11, 14),
            surface_display: rgb(82, 82, 91),
            surface_canvas: rgb(82, 82, 91),
            sidebar_sources: rgb(30, 33, 40),
            sidebar_list: rgb(26, 29, 35),
            sidebar_band: rgb(30, 33, 40),
            sidebar_pick_sources: rgb(40, 44, 53),
            sidebar_pick_sources_line: rgb(40, 44, 53),
            sidebar_pick_list: rgb(40, 44, 53),
            sidebar_pick_list_line: rgb(40, 44, 53),
            sidebar_field: rgb(36, 40, 48),
            control_accent: rgb(10, 132, 255),
            accent: [
                rgb(235, 244, 255),
                rgb(214, 234, 255),
                rgb(173, 214, 255),
                rgb(128, 190, 255),
                rgb(64, 156, 255),
                rgb(10, 132, 255),
                rgb(0, 112, 232),
                rgb(0, 92, 196),
                rgb(0, 66, 140),
                rgb(0, 44, 92),
                rgb(0, 28, 58),
            ],
            on_accent: rgb(255, 255, 255),
            live: rgb(240, 68, 56),
            hues: [
                rgb(94, 204, 140),
                rgb(240, 152, 84),
                rgb(167, 139, 245),
                rgb(72, 194, 216),
                rgb(110, 172, 255),
                rgb(236, 120, 178),
                rgb(244, 124, 104),
                rgb(178, 208, 92),
                rgb(240, 190, 84),
                rgb(138, 146, 245),
            ],
            tints: Tints {
                live: rgb(74, 38, 38),
                teal: rgb(39, 39, 42),
                red: rgb(52, 26, 26),
                amber: rgb(39, 39, 42),
                yellow: rgb(39, 39, 42),
                rose: rgb(54, 26, 28),
            },
            range_track: rgb(51, 51, 56),
            text_primary: rgb(244, 244, 245),
            fg: [
                rgb(250, 250, 250),
                rgb(228, 228, 231),
                rgb(212, 212, 216),
                rgb(161, 161, 170),
                rgb(137, 137, 146),
            ],
            neutral: [
                rgb(250, 250, 250),
                rgb(244, 244, 245),
                rgb(228, 228, 231),
                rgb(212, 212, 216),
                rgb(161, 161, 170),
                rgb(113, 113, 122),
                rgb(82, 82, 91),
                rgb(63, 63, 70),
                rgb(39, 39, 42),
                rgb(24, 24, 27),
                rgb(9, 9, 11),
            ],
            glass: rgb(30, 30, 33),
            stone: rgb(0x37, 0x41, 0x51),
            destructive: rgb(0xef, 0x44, 0x44),
            // Tailwind red-400 and rose-300; dark mode keeps the stock steps.
            danger_text: rgb(0xf8, 0x71, 0x71),
            danger_text_soft: rgb(0xfd, 0xa4, 0xaf),
            success_text: rgb(0x34, 0xd3, 0x99),
            warning_text: rgb(0xfd, 0xe0, 0x47),
            checker_light: rgb(0x30, 0x30, 0x30),
            checker_dark: rgb(0x24, 0x24, 0x24),
            paper: rgb(0xf9, 0xfa, 0xfb),
            favorite: rgb(0xfa, 0xcc, 0x15),
            sermon_mark: rgb(0xca, 0xd5, 0xe2),
            sermon_mark_active: rgb(0xb9, 0xd7, 0xff),
            notice_text: rgb(0x2d, 0xd4, 0xbf),
            splash_boot: rgb(113, 113, 122),
            white: rgb(255, 255, 255),
            traffic_close: rgb(0xff, 0x5f, 0x57),
            traffic_close_glyph: rgb(0x4d, 0x00, 0x00),
            traffic_zoom: rgb(0x28, 0xc8, 0x40),
            traffic_zoom_glyph: rgb(0x0b, 0x4a, 0x14),
            onboarding_text: rgb(246, 243, 235),
            onboarding_text_deep: rgb(125, 131, 153),
        }
    }

    /// `[data-theme='light']`.
    pub fn light() -> Self {
        let surface_border = rgb(212, 212, 216);
        Self {
            appearance: Appearance::Light,
            solid_chrome: false,
            surface: rgb(250, 250, 250),
            surface_secondary: rgb(244, 244, 245),
            surface_tertiary: rgb(228, 228, 231),
            surface_elevated: rgb(255, 255, 255),
            surface_rail: rgb(228, 228, 231),
            surface_header: rgb(228, 228, 231),
            surface_border,
            surface_menu: rgb(255, 255, 255),
            surface_menu_hover: rgb(244, 244, 245),
            surface_menu_line: rgb(228, 228, 231),
            surface_line: rgb(228, 228, 231),
            panel_border: surface_border.opacity(0.75),
            surface_stage: rgb(212, 212, 216),
            surface_display: rgb(250, 250, 250),
            surface_canvas: rgb(161, 161, 170),
            sidebar_sources: rgb(244, 244, 245),
            sidebar_list: rgb(244, 244, 245),
            sidebar_band: rgb(228, 228, 231),
            sidebar_pick_sources: rgb(228, 228, 231),
            sidebar_pick_sources_line: rgb(212, 212, 216),
            sidebar_pick_list: rgb(228, 228, 231),
            sidebar_pick_list_line: rgb(212, 212, 216),
            sidebar_field: rgb(255, 255, 255),
            control_accent: rgb(0, 122, 255),
            accent: [
                rgb(0, 96, 206),
                rgb(0, 96, 206),
                rgb(0, 96, 206),
                rgb(0, 96, 206),
                rgb(26, 136, 255),
                rgb(0, 122, 255),
                rgb(0, 104, 222),
                rgb(0, 86, 186),
                rgb(48, 98, 158),
                rgb(188, 216, 250),
                rgb(224, 238, 255),
            ],
            on_accent: rgb(255, 255, 255),
            live: rgb(220, 48, 38),
            hues: [
                rgb(22, 128, 70),
                rgb(190, 82, 18),
                rgb(109, 52, 210),
                rgb(14, 116, 144),
                rgb(0, 116, 204),
                rgb(184, 36, 104),
                rgb(190, 62, 46),
                rgb(84, 120, 18),
                rgb(156, 100, 10),
                rgb(72, 64, 196),
            ],
            tints: Tints {
                live: rgb(253, 224, 221),
                teal: rgb(228, 228, 231),
                red: rgb(253, 227, 221),
                amber: rgb(228, 228, 231),
                yellow: rgb(228, 228, 231),
                rose: rgb(254, 227, 225),
            },
            range_track: rgb(212, 212, 216),
            text_primary: rgb(9, 9, 11),
            fg: [
                rgb(24, 24, 27),
                rgb(39, 39, 42),
                rgb(63, 63, 70),
                rgb(82, 82, 91),
                // The faintest step, as in dark: lighter than T500.
                rgb(113, 113, 122),
            ],
            neutral: [
                rgb(9, 9, 11),
                rgb(24, 24, 27),
                rgb(39, 39, 42),
                rgb(63, 63, 70),
                rgb(63, 63, 70),
                rgb(82, 82, 91),
                rgb(113, 113, 122),
                rgb(212, 212, 216),
                rgb(228, 228, 231),
                rgb(244, 244, 245),
                rgb(250, 250, 250),
            ],
            glass: rgb(244, 244, 245),
            stone: rgb(0x37, 0x41, 0x51),
            destructive: rgb(0xef, 0x44, 0x44),
            danger_text: rgb(0xb9, 0x1c, 0x1c),
            danger_text_soft: rgb(0xb9, 0x1c, 0x1c),
            success_text: rgb(0x04, 0x78, 0x57),
            // Pale yellow vanishes on light surfaces: amber-700.
            warning_text: rgb(0xb4, 0x53, 0x09),
            checker_light: rgb(0x30, 0x30, 0x30),
            checker_dark: rgb(0x24, 0x24, 0x24),
            paper: rgb(0xf9, 0xfa, 0xfb),
            favorite: rgb(0xf5, 0x9e, 0x0b),
            sermon_mark: rgb(0xca, 0xd5, 0xe2),
            sermon_mark_active: rgb(0xb9, 0xd7, 0xff),
            // teal-700: teal-400 is too faint on light surfaces.
            notice_text: rgb(0x0f, 0x76, 0x6e),
            splash_boot: rgb(113, 113, 122),
            white: rgb(255, 255, 255),
            traffic_close: rgb(0xff, 0x5f, 0x57),
            traffic_close_glyph: rgb(0x4d, 0x00, 0x00),
            traffic_zoom: rgb(0x28, 0xc8, 0x40),
            traffic_zoom_glyph: rgb(0x0b, 0x4a, 0x14),
            onboarding_text: rgb(10, 11, 16),
            onboarding_text_deep: rgb(86, 91, 110),
        }
    }

    /// The live rail's override (`.booth-toolbox`): whole zinc steps only.
    /// Light theme is already zinc and keeps its tokens.
    pub fn booth_toolbox(self) -> Self {
        if self.appearance == Appearance::Light {
            return self;
        }
        Self {
            surface_rail: rgb(39, 39, 42),
            surface: rgb(24, 24, 27),
            surface_secondary: rgb(24, 24, 27),
            surface_tertiary: rgb(63, 63, 70),
            surface_elevated: rgb(82, 82, 91),
            surface_border: rgb(39, 39, 42),
            panel_border: rgb(24, 24, 27),
            range_track: rgb(63, 63, 70),
            text_primary: rgb(244, 244, 245),
            fg: [
                rgb(228, 228, 231),
                rgb(212, 212, 216),
                rgb(161, 161, 170),
                rgb(161, 161, 170),
                rgb(113, 113, 122),
            ],
            ..self
        }
    }

    /// One accent step (`teal-<step>`).
    pub fn accent(&self, step: Step) -> Hsla {
        self.accent[step.index()]
    }

    /// One neutral step (`zinc-<step>` fills).
    pub fn neutral(&self, step: Step) -> Hsla {
        self.neutral[step.index()]
    }

    /// One grey text step (`text-zinc-<step>`).
    pub fn text(&self, step: TextStep) -> Hsla {
        self.fg[step as usize]
    }

    /// A see-through tone for hover fills, hairlines and rims on themed
    /// surfaces: white over dark ones, near-black over light ones. Use it
    /// instead of `white.opacity(..)`, which disappears in light mode.
    pub fn ink(&self, alpha: f32) -> Hsla {
        match self.appearance {
            Appearance::Dark => self.white.opacity(alpha),
            Appearance::Light => self.text_primary.opacity(alpha * 0.8),
        }
    }

    /// A tool's hue.
    pub fn hue(&self, hue: Hue) -> Hsla {
        self.hues[hue as usize]
    }

    /// `.row-selected`: an 18% accent wash.
    pub fn row_selected(&self) -> Hsla {
        self.accent(Step::S500).opacity(0.18)
    }

    /// The line inside a `.pref-group` on a normal (`surface-secondary`)
    /// group. `.pref-group-dim` drops it a step.
    pub fn pref_line(&self, dim: bool) -> Hsla {
        if dim {
            self.surface_tertiary
        } else {
            self.surface_elevated
        }
    }

    /// `.glass`: a modal's sidebar or title bar. Opaque with
    /// [`Self::solid_chrome`].
    pub fn glass_panel(&self) -> Hsla {
        self.translucent(self.glass, GLASS_PANEL_ALPHA)
    }

    /// `.modal-glass`: a whole small dialog, a touch more opaque. Opaque
    /// with [`Self::solid_chrome`].
    pub fn glass_modal(&self) -> Hsla {
        self.translucent(self.glass, GLASS_MODAL_ALPHA)
    }

    /// The window-material tint on the top bar and rail (62%). Under solid
    /// chrome it is the solid rail (`index.css:686`).
    pub fn material_tint(&self) -> Hsla {
        self.translucent(self.surface_rail, MATERIAL_ALPHA)
    }

    /// `color` at `alpha`, or opaque under solid chrome.
    fn translucent(&self, color: Hsla, alpha: f32) -> Hsla {
        if self.solid_chrome {
            color
        } else {
            color.opacity(alpha)
        }
    }

    /// The marquee band's border (`white/60`).
    pub fn marquee_border(&self) -> Hsla {
        self.white.opacity(0.6)
    }

    /// The marquee band's fill (`white/[0.08]`).
    pub fn marquee_fill(&self) -> Hsla {
        self.white.opacity(0.08)
    }

    /// The focus ring (`ring-teal-500/80`).
    pub fn focus_ring(&self) -> Hsla {
        self.accent(Step::S500).opacity(0.8)
    }

    /// The scrim behind a modal.
    pub fn scrim(&self) -> Hsla {
        rgba(0, 0, 0, 0.5)
    }

    /// The live monitor's empty frame (`bg-black`): the screen's black in
    /// both appearances.
    pub fn monitor(&self) -> Hsla {
        rgba(0, 0, 0, 1.0)
    }
}

/// `.glass` alpha.
pub const GLASS_PANEL_ALPHA: f32 = 0.55;
/// `.modal-glass` alpha.
pub const GLASS_MODAL_ALPHA: f32 = 0.78;
/// Window-material tint alpha on the top bar and rail.
pub const MATERIAL_ALPHA: f32 = 0.62;

/// The radius scale (`borderRadius` in `tailwind.config.js`).
pub mod radius {
    use super::*;

    /// `rounded-sm`.
    pub fn sm() -> Pixels {
        px(4.)
    }
    /// `rounded` (`--radius`, 0.375rem).
    pub fn base() -> Pixels {
        px(6.)
    }
    /// `rounded-md`.
    pub fn md() -> Pixels {
        px(8.)
    }
    /// `rounded-lg`.
    pub fn lg() -> Pixels {
        px(10.)
    }
    /// `rounded-xl`.
    pub fn xl() -> Pixels {
        px(12.)
    }
    /// `rounded-2xl`: the workspace's top-left corner.
    pub fn xl2() -> Pixels {
        px(16.)
    }
}

/// The type scale (`fontSize` in `tailwind.config.js`), size and line height.
pub mod text {
    use super::*;

    /// A size and its line height.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TextSize {
        /// Font size.
        pub size: Rems,
        /// Line height.
        pub line_height: Rems,
    }

    const fn ts(size: f32, line_height: f32) -> TextSize {
        TextSize {
            size: Rems(size),
            line_height: Rems(line_height),
        }
    }

    /// 11px: rail labels, counts.
    pub const MICRO: TextSize = ts(0.6875, 0.8125);
    /// 12px: field labels and sidebar headings.
    pub const LABEL: TextSize = ts(0.75, 1.0625);
    /// 13px (`text-xs`).
    pub const XS: TextSize = ts(0.8125, 1.125);
    /// 14px (`text-sm`).
    pub const SM: TextSize = ts(0.875, 1.25);
    /// 16px (`text-base`): body copy.
    pub const BASE: TextSize = ts(1.0, 1.5);
    /// 18px (`text-lg`).
    pub const LG: TextSize = ts(1.125, 1.625);
    /// 21px (`text-xl`): page headers.
    pub const XL: TextSize = ts(1.3125, 1.75);
    /// 25px (`text-2xl`).
    pub const XL2: TextSize = ts(1.5625, 2.0);
    /// 31px (`text-3xl`).
    pub const XL3: TextSize = ts(1.9375, 2.25);

    /// The interface font (`fontFamily.sans`).
    pub const FAMILY: &str = "Inter";
    /// Code and timecodes (`fontFamily.mono`).
    pub const MONO_FAMILY: &str = "Menlo";

    /// The rem every scale is relative to (Tailwind's 16px).
    pub fn rem() -> Pixels {
        px(16.)
    }

    /// `rems` helper for callers that need a custom size from the scale.
    pub fn r(value: f32) -> Rems {
        rems(value)
    }
}

/// Fixed geometry from the stylesheet.
pub mod metrics {
    use super::*;

    /// `--panel-border-width`.
    pub fn panel_border_width() -> Pixels {
        px(0.5)
    }
    /// `.app-header` height.
    pub fn header_height() -> Pixels {
        px(68.)
    }
    /// `.workspace-rail` width (wide enough to clear macOS traffic lights cleanly).
    pub fn rail_width() -> Pixels {
        px(86.)
    }
    /// The macOS window-button corner the header reserves.
    pub fn traffic_light_corner() -> Pixels {
        px(68.)
    }
    /// One macOS traffic light.
    pub fn traffic_light() -> Pixels {
        px(12.)
    }
    /// The gap between traffic lights.
    pub fn traffic_light_gap() -> Pixels {
        px(8.)
    }
    /// The inset of a `.pref-group` row divider from each side (0.875rem).
    pub fn pref_line_inset() -> Rems {
        rems(0.875)
    }
}

/// The lyric section kinds, for [`section_color`] (`LyricsSectionType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SectionKind {
    Verse,
    Chorus,
    PreChorus,
    Bridge,
    Tag,
    Intro,
    Outro,
    Ending,
}

/// `SECTION_COLOR` (`section-colors.ts`): one solid fill per section type,
/// the same in both themes, deep enough for [`section_text`] at 5:1.
pub fn section_color(kind: SectionKind) -> Hsla {
    match kind {
        SectionKind::Verse => rgb(0x3b, 0x63, 0xd9),
        SectionKind::Chorus => rgb(0xc8, 0x37, 0x2d),
        SectionKind::PreChorus => rgb(0x0e, 0x74, 0x90),
        SectionKind::Bridge => rgb(0x7c, 0x3a, 0xed),
        SectionKind::Tag => rgb(0xa1, 0x5c, 0x07),
        SectionKind::Intro => rgb(0x37, 0x41, 0x51),
        SectionKind::Outro => rgb(0x4f, 0x46, 0xe5),
        SectionKind::Ending => rgb(0xbe, 0x12, 0x3c),
    }
}

/// `SECTION_TEXT` (`#F9FAFB`): text on a section fill.
pub fn section_text() -> Hsla {
    rgb(0xf9, 0xfa, 0xfb)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast of two opaque colours.
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let lum = |c: Hsla| {
            let c: Rgba = c.into();
            let ch = |v: f32| {
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
        };
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn light_mode_text_reads_on_light_surfaces() {
        let k = KairoColors::light();
        for (name, c) in [
            ("warning", k.warning_text),
            ("notice", k.notice_text),
            ("danger", k.danger_text),
            ("success", k.success_text),
            ("muted", k.text(TextStep::T500)),
            ("faintest", k.text(TextStep::T600)),
        ] {
            let ratio = contrast(c, k.surface);
            assert!(ratio >= 4.5, "{name} on the page: {ratio}");
        }
        // The faintest step stays lighter than muted, as in dark.
        assert!(
            contrast(k.text(TextStep::T600), k.surface)
                < contrast(k.text(TextStep::T500), k.surface)
        );
    }

    #[test]
    fn ink_is_white_in_dark_and_dark_in_light() {
        let dark = KairoColors::dark();
        assert_eq!(dark.ink(0.1), dark.white.opacity(0.1));
        assert!(
            KairoColors::light().ink(0.1).l < 0.2,
            "a dark tone on light surfaces"
        );
    }
}
