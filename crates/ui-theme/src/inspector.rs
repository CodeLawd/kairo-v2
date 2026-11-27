//! The Theme editor's inspector (prompt 087): the Background, Text and
//! Layout tabs for the theme, and the element panel for a shape, picture
//! or video. Sources: `ThemeTypePanel.tsx`, `ThemeLayoutPanel.tsx`,
//! `ThemeElementPanel.tsx`, `fields.tsx`, `NumberField.tsx` and the
//! inspector part of `ThemeEditor.tsx`.
//!
//! Every edit goes through [`ThemeEditor::edit_theme`], so it is undoable,
//! shown on the slide and saved after the editor's pause. Fields keep their
//! own text while they are being typed in: the field that made an edit is
//! not written back from the draft until something else changes it (undo,
//! another control, another theme).

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverButtonTriggerExt;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::{
    AnyElement, App, AppContext, Context, Entity, EventEmitter, Focusable, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, div, img, px,
};
use kairo_domain::overlay::{
    MediaFit, OverlayBackgroundType, OverlayContentKind, OverlayElement, OverlayElementKind,
    OverlayLayoutPosition, OverlayOutlinePosition, OverlayOutlineStyle, OverlayTextAlign,
    OverlayTextDecoration, OverlayTextTransform, OverlayTheme, OverlayVerticalAlign,
    ReferencePosition,
};
use kairo_domain::overlay_boxes::{
    ELEMENT_BOX_MIN, TEXT_BOX_MIN, apply_layout_preset, clamp_overlay_box,
};
use kairo_domain::overlay_elements::{
    ElementAlignment, ReorderTarget, align_overlay_element, element_is_line, is_media_element,
    overlay_element_label, reorder_overlay_element, update_overlay_element,
};
use kairo_domain::overlay_outputs::{OverlayLayer, overlay_layer_label};
use kairo_domain::overlay_shapes::overlay_shape;
use kairo_ui_kit::components::{
    NumberField, NumberFieldChanged, NumberModel, SegmentOption, SegmentedControl,
};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::MediaKind;
use crate::canvas::{REFERENCE, VERSE, is_text_layer};
use crate::editor::{ThemeEditor, ThemeEditorEvent};
use crate::gallery;
use crate::panels::{self, Adjustments, Choice};

pub mod ids {
    pub const ROOT: &str = "theme-inspector";
    pub const TABS: &str = "theme-inspector-tabs";
    pub const DONE: &str = "theme-element-done";
    pub const DUPLICATE: &str = "theme-element-duplicate";
    pub const DELETE: &str = "theme-element-delete";
    pub const FONT: &str = "theme-font";
    pub const WEIGHT: &str = "theme-weight";
    pub const SIZE: &str = "theme-size";
    pub const SIZE_PRESETS: &str = "theme-size-presets";
    pub const SCALING: &str = "theme-scaling";
    pub const POSITION: &str = "theme-reference-position";
    pub const SHOW_REFERENCE: &str = "theme-show-reference";
    pub const SHADOW: &str = "theme-shadow";
    pub const STROKE: &str = "theme-stroke";
    pub const SPACING: &str = "theme-spacing";
    pub const BACKDROP_EYE: &str = "theme-backdrop-eye";
    pub const BG_TYPE: &str = "theme-bg-type";
    pub const MEDIA: &str = "theme-media-pick";
    pub const SHAPE: &str = "theme-element-shape";
    pub const STROKE_EYE: &str = "theme-element-stroke-eye";
    pub const ADJUST_RESET: &str = "theme-adjust-reset";
    /// A field by its key (`text.size`, `layout.x`…).
    pub fn field(key: &str) -> String {
        format!("theme-inspector-{key}")
    }
    pub fn preset(position: &str) -> String {
        format!("theme-preset-{position}")
    }
    pub fn layer_switch(layer: &str) -> String {
        format!("theme-box-{layer}")
    }
    pub fn align(key: &str) -> String {
        format!("theme-align-{key}")
    }
    pub fn element_align(key: &str) -> String {
        format!("theme-element-align-{key}")
    }
    pub fn reorder(key: &str) -> String {
        format!("theme-element-reorder-{key}")
    }
    pub fn flip(key: &str) -> String {
        format!("theme-element-flip-{key}")
    }
    pub fn decoration(key: &str) -> String {
        format!("theme-decoration-{key}")
    }
    pub fn section(key: &str) -> String {
        format!("theme-section-{key}")
    }
}

/// The inspector's tabs (`InspectorTab`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Background,
    Text,
    Layout,
}

impl Tab {
    pub fn key(self) -> &'static str {
        match self {
            Tab::Background => "background",
            Tab::Text => "type",
            Tab::Layout => "layout",
        }
    }
    fn from_key(key: &str) -> Self {
        match key {
            "type" => Tab::Text,
            "layout" => Tab::Layout,
            _ => Tab::Background,
        }
    }
}

type Edit<T> = Rc<dyn Fn(&OverlayTheme, T) -> OverlayTheme>;
/// A dropdown's or toggle group's change.
type Change = Rc<dyn Fn(String, &mut Window, &mut App)>;
type Thumb = Option<std::sync::Arc<gpui_kit::RenderImage>>;
type ShapeGroups = Vec<(&'static str, Vec<(gallery::Tile, Thumb)>)>;
/// One adjustment slider: key, label, value, range, shown value, setter.
type AdjustRow = (
    &'static str,
    &'static str,
    f64,
    (f64, f64, f64),
    String,
    fn(&mut Adjustments, f64),
);

// ─── The colour field ──────────────────────────────────────────────────────

/// What a [`ColorField`] reports.
#[derive(Clone, Debug, PartialEq)]
pub enum ColorFieldEvent {
    Color(String),
    Opacity(f64),
}

/// `ColorOpacityField` (swatch, hex, opacity %) and, with `free` text,
/// `ColorField` (swatch and any CSS colour, for `rgba()` backgrounds).
pub struct ColorField {
    free: bool,
    color: String,
    opacity: Option<f64>,
    picker: Entity<ColorPickerState>,
    hex: Entity<InputState>,
    percent: Option<Entity<InputState>>,
    label: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ColorFieldEvent> for ColorField {}

/// `#rrggbb` as GPUI paints it.
pub fn hex_hsla(color: &str) -> Option<Hsla> {
    let hex = panels::expand_hex(color)?;
    let channel = |i: usize| f32::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0;
    Some(
        gpui_kit::Rgba {
            r: channel(1),
            g: channel(3),
            b: channel(5),
            a: 1.0,
        }
        .into(),
    )
}

fn hsla_hex(color: Hsla) -> String {
    let c = color.to_rgb();
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
}

fn percent_text(opacity: f64) -> String {
    kairo_domain::js::number_to_string(kairo_domain::js::math_round(opacity * 100.0))
}

impl ColorField {
    pub fn new(
        label: impl Into<SharedString>,
        color: &str,
        opacity: Option<f64>,
        free: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let swatch = hex_hsla(color).unwrap_or_else(gpui_kit::black);
        let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(swatch));
        let shown = if free {
            color.to_owned()
        } else {
            panels::hex_display(color)
        };
        let hex = cx.new(|cx| InputState::new(window, cx).default_value(shown));
        let percent = opacity
            .map(|o| cx.new(|cx| InputState::new(window, cx).default_value(percent_text(o))));
        let mut subs = vec![
            cx.subscribe(&picker, |this, _, e: &ColorPickerEvent, cx| {
                if let ColorPickerEvent::Change(Some(c)) = e {
                    let hex = hsla_hex(*c);
                    this.color = hex.clone();
                    cx.emit(ColorFieldEvent::Color(hex));
                }
            }),
            cx.subscribe_in(&hex, window, Self::on_hex),
        ];
        if let Some(p) = &percent {
            subs.push(cx.subscribe_in(p, window, Self::on_percent));
        }
        Self {
            free,
            color: color.to_owned(),
            opacity,
            picker,
            hex,
            percent,
            label: label.into(),
            _subscriptions: subs,
        }
    }

    fn on_hex(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let raw = input.read(cx).value().to_string();
                let next = if self.free {
                    Some(raw)
                } else {
                    panels::typed_hex(&raw)
                };
                if let Some(next) = next
                    && next != self.color
                {
                    self.color = next.clone();
                    cx.emit(ColorFieldEvent::Color(next));
                }
            }
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                if !self.free
                    && let Some(full) = panels::settled_hex(&self.color)
                    && full != self.color
                {
                    self.color = full.clone();
                    cx.emit(ColorFieldEvent::Color(full));
                }
                self.show_text(window, cx);
            }
            InputEvent::Focus => {}
        }
    }

    fn on_percent(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let raw = input.read(cx).value().to_string();
                let Some(pct) = kairo_ui_kit::components::number_field::parse_number(&raw) else {
                    return;
                };
                let next = pct.clamp(0.0, 100.0) / 100.0;
                if Some(next) != self.opacity {
                    self.opacity = Some(next);
                    cx.emit(ColorFieldEvent::Opacity(next));
                }
            }
            InputEvent::Blur | InputEvent::PressEnter { .. } => self.show_text(window, cx),
            InputEvent::Focus => {}
        }
    }

    fn show_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = if self.free {
            self.color.clone()
        } else {
            panels::hex_display(&self.color)
        };
        self.hex.update(cx, |i, cx| {
            if i.value() != shown.as_str() {
                i.set_value(shown, window, cx);
            }
        });
        if let (Some(p), Some(o)) = (&self.percent, self.opacity) {
            let shown = percent_text(o);
            p.update(cx, |i, cx| {
                if i.value() != shown.as_str() {
                    i.set_value(shown, window, cx);
                }
            });
        }
    }

    /// The colour from outside (undo, another control). Does not emit.
    pub fn sync(
        &mut self,
        color: &str,
        opacity: Option<f64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if color == self.color && opacity == self.opacity {
            return;
        }
        self.color = color.to_owned();
        self.opacity = opacity;
        if let Some(c) = hex_hsla(color) {
            self.picker.update(cx, |p, cx| p.set_value(c, window, cx));
        }
        self.show_text(window, cx);
        cx.notify();
    }

    pub fn color(&self) -> &str {
        &self.color
    }
}

impl Render for ColorField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        h_flex()
            .h_8()
            .gap_2()
            .px_2()
            .rounded(radius::md())
            .bg(k.surface_tertiary)
            .text_size(text::SM.size)
            .child(
                ColorPicker::new(&self.picker)
                    .accessibility_label(format!("{} swatch", self.label)),
            )
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.hex)
                        .kairo_size()
                        .appearance(false)
                        .small()
                        .aria_label(if self.free {
                            format!("{} value", self.label)
                        } else {
                            format!("{} hex", self.label)
                        }),
                ),
            )
            .children(self.percent.as_ref().map(|p| {
                h_flex()
                    .flex_none()
                    .w(px(52.))
                    .gap_0p5()
                    .child(
                        Input::new(p)
                            .kairo_size()
                            .appearance(false)
                            .small()
                            .aria_label(format!("{} opacity", self.label)),
                    )
                    .child(
                        div()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child("%"),
                    )
            }))
    }
}

// ─── The inspector ─────────────────────────────────────────────────────────

pub struct ThemeInspector {
    editor: Entity<ThemeEditor>,
    tab: Tab,
    placement_open: bool,
    box_open: bool,
    backdrop_open: bool,
    spacing_open: bool,
    shapes_open: bool,
    font_open: bool,
    numbers: HashMap<String, Entity<NumberField>>,
    colors: HashMap<String, Entity<ColorField>>,
    sliders: HashMap<String, Entity<SliderState>>,
    size: Option<Entity<InputState>>,
    /// A text element's words, for the element it was made for.
    text_input: Option<(String, Entity<TextareaState>)>,
    field_subs: Vec<Subscription>,
    /// The field whose edit made the current draft; not written back.
    origin: Option<String>,
    own_draft: Option<OverlayTheme>,
    scope: (Option<String>, Option<String>),
    _subscriptions: Vec<Subscription>,
}

impl ThemeInspector {
    /// Builds the inspector and puts it in the editor's right column.
    pub fn attach(editor: &Entity<ThemeEditor>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        let inspector = cx.new(|cx| Self::new(editor.clone(), window, cx));
        let view = inspector.clone().into();
        editor.update(cx, |e, cx| e.set_inspector(Some(view), cx));
        inspector
    }

    pub fn new(editor: Entity<ThemeEditor>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let sub = cx.subscribe(&editor, |this, editor, e: &ThemeEditorEvent, cx| {
            match e {
                ThemeEditorEvent::DraftChanged => {
                    if this.own_draft.as_ref() != Some(editor.read(cx).draft()) {
                        this.origin = None;
                        this.own_draft = None;
                    }
                }
                ThemeEditorEvent::SelectionChanged => {
                    this.clear_fields();
                    // A text layer's look is on the Text tab; Background
                    // says nothing about it.
                    let selected = editor.read(cx).selected_layer().map(str::to_owned);
                    if this.tab == Tab::Background && selected.as_deref().is_some_and(is_text_layer)
                    {
                        this.tab = Tab::Text;
                    }
                }
            }
            cx.notify();
        });
        Self {
            editor,
            tab: Tab::Background,
            placement_open: true,
            box_open: true,
            backdrop_open: true,
            spacing_open: false,
            shapes_open: false,
            font_open: false,
            numbers: HashMap::new(),
            colors: HashMap::new(),
            sliders: HashMap::new(),
            size: None,
            text_input: None,
            field_subs: Vec::new(),
            origin: None,
            own_draft: None,
            scope: (None, None),
            _subscriptions: vec![sub],
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.clear_fields();
            cx.notify();
        }
    }

    fn clear_fields(&mut self) {
        self.numbers.clear();
        self.colors.clear();
        self.sliders.clear();
        self.size = None;
        self.text_input = None;
        self.field_subs.clear();
        self.origin = None;
        self.own_draft = None;
        self.spacing_open = false;
        self.shapes_open = false;
    }

    /// Applies an edit made by the field `key`.
    pub fn edit(
        &mut self,
        key: &str,
        f: impl FnOnce(&OverlayTheme) -> OverlayTheme,
        cx: &mut Context<Self>,
    ) {
        let editor = self.editor.clone();
        if editor.update(cx, |e, cx| e.edit_theme(f, cx)) {
            self.origin = Some(key.to_owned());
            self.own_draft = Some(editor.read(cx).draft().clone());
        }
        cx.notify();
    }

    fn edit_element(
        &mut self,
        key: &str,
        id: &str,
        patch: impl Fn(&mut OverlayElement),
        cx: &mut Context<Self>,
    ) {
        let id = id.to_owned();
        self.edit(
            key,
            move |t| {
                let mut next = t.clone();
                next.elements = update_overlay_element(&t.elements, &id, &patch);
                next
            },
            cx,
        );
    }

    // ─── Fields kept between frames ────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    fn number(
        &mut self,
        key: &str,
        label: &str,
        value: f64,
        (step, min, max): (f64, f64, f64),
        suffix: Option<&str>,
        on: impl Fn(&OverlayTheme, f64) -> OverlayTheme + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = if let Some(f) = self.numbers.get(key).cloned() {
            if self.origin.as_deref() != Some(key) && f.read(cx).value() != value {
                f.update(cx, |f, cx| f.set_value(value, window, cx));
            }
            f
        } else {
            let id = SharedString::from(ids::field(key));
            let model = NumberModel::new(value, step).min(min).max(max);
            let suffix = suffix.map(str::to_owned);
            let label = label.to_owned();
            let f = cx.new(|cx| {
                let n = NumberField::new(id, label, model, window, cx);
                match suffix {
                    Some(s) => n.with_suffix(s),
                    None => n,
                }
            });
            let on: Edit<f64> = Rc::new(on);
            let k = key.to_owned();
            self.field_subs.push(
                cx.subscribe(&f, move |this, _, e: &NumberFieldChanged, cx| {
                    let (on, v) = (on.clone(), e.0);
                    this.edit(&k, move |t| on(t, v), cx);
                }),
            );
            self.numbers.insert(key.to_owned(), f.clone());
            f
        };
        div().min_w_0().child(field).into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn color(
        &mut self,
        key: &str,
        label: &str,
        color: &str,
        opacity: Option<f64>,
        free: bool,
        on_color: impl Fn(&OverlayTheme, String) -> OverlayTheme + 'static,
        on_opacity: impl Fn(&OverlayTheme, f64) -> OverlayTheme + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = if let Some(f) = self.colors.get(key).cloned() {
            if self.origin.as_deref() != Some(key) {
                f.update(cx, |f, cx| f.sync(color, opacity, window, cx));
            }
            f
        } else {
            let label = label.to_owned();
            let f = cx.new(|cx| ColorField::new(label, color, opacity, free, window, cx));
            let (on_color, on_opacity): (Edit<String>, Edit<f64>) =
                (Rc::new(on_color), Rc::new(on_opacity));
            let k = key.to_owned();
            self.field_subs.push(cx.subscribe(
                &f,
                move |this, _, e: &ColorFieldEvent, cx| match e {
                    ColorFieldEvent::Color(c) => {
                        let (on, c) = (on_color.clone(), c.clone());
                        this.edit(&k, move |t| on(t, c), cx);
                    }
                    ColorFieldEvent::Opacity(o) => {
                        let (on, o) = (on_opacity.clone(), *o);
                        this.edit(&k, move |t| on(t, o), cx);
                    }
                },
            ));
            self.colors.insert(key.to_owned(), f.clone());
            f
        };
        div()
            .id(SharedString::from(ids::field(key)))
            .test_support()
            .min_w_0()
            .child(field)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        key: &str,
        label: &str,
        value: f64,
        (min, max, step): (f64, f64, f64),
        shown: String,
        on: impl Fn(&OverlayTheme, f64) -> OverlayTheme + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let state = if let Some(s) = self.sliders.get(key).cloned() {
            if self.origin.as_deref() != Some(key) && f64::from(s.read(cx).value().start()) != value
            {
                s.update(cx, |s, cx| s.set_value(value as f32, window, cx));
            }
            s
        } else {
            let s = cx.new(|_| {
                SliderState::new()
                    .min(min as f32)
                    .max(max as f32)
                    .step(step as f32)
                    .default_value(value as f32)
            });
            let on: Edit<f64> = Rc::new(on);
            let key2 = key.to_owned();
            self.field_subs
                .push(cx.subscribe(&s, move |this, _, e: &SliderEvent, cx| {
                    if let SliderEvent::Change(v) = e {
                        let (on, v) = (on.clone(), f64::from(v.start()));
                        this.edit(&key2, move |t| on(t, v), cx);
                    }
                }));
            self.sliders.insert(key.to_owned(), s.clone());
            s
        };
        v_flex()
            .id(SharedString::from(ids::field(key)))
            .test_support()
            .min_w_0()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(label.to_owned()),
                    )
                    .child(shown),
            )
            .child(Slider::new(&state).accessibility_label(label.to_owned()))
            .into_any_element()
    }

    // ─── Tabs ──────────────────────────────────────────────────────────────

    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let this = cx.entity();
        div()
            .flex_none()
            .p_3()
            .border_b_1()
            .border_color(cx.kairo().surface_line)
            .child(
                SegmentedControl::new(ids::TABS, "Theme settings")
                    .tabs()
                    .fit()
                    .options([
                        SegmentOption::new("background", "Background"),
                        SegmentOption::new("type", "Text"),
                        SegmentOption::new("layout", "Layout"),
                    ])
                    .selected(self.tab.key())
                    .on_change(move |key, _, cx| {
                        let tab = Tab::from_key(key);
                        this.update(cx, |t, cx| t.set_tab(tab, cx));
                    }),
            )
            .into_any_element()
    }

    // ─── Background ────────────────────────────────────────────────────────

    fn render_background(
        &mut self,
        theme: &OverlayTheme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = theme.background.clone();
        let mut col = v_flex().gap_4();
        col = col.child(labelled(
            "Type",
            select(
                ids::BG_TYPE,
                "Type",
                bg.kind.as_str(),
                vec![
                    choice("transparent", "None"),
                    choice("color", "Color"),
                    choice("gradient", "Gradient"),
                    choice("image", "Image"),
                    choice("video", "Video"),
                ],
                self.on_choice(
                    "bg.type",
                    |t, v| {
                        let mut n = t.clone();
                        if let Some(kind) = OverlayBackgroundType::from_name(&v) {
                            n.background.kind = kind;
                        }
                        n
                    },
                    cx,
                ),
            ),
            cx,
        ));
        let gradient = bg.kind == OverlayBackgroundType::Gradient;
        if matches!(
            bg.kind,
            OverlayBackgroundType::Color | OverlayBackgroundType::Gradient
        ) {
            let from = self.color(
                "bg.color",
                if gradient { "From" } else { "Color" },
                &bg.color,
                None,
                true,
                |t, c| {
                    let mut n = t.clone();
                    n.background.color = c;
                    n
                },
                |t, _| t.clone(),
                window,
                cx,
            );
            let mut row =
                h_flex()
                    .gap_4()
                    .child(labelled(if gradient { "From" } else { "Color" }, from, cx));
            if gradient {
                let to = self.color(
                    "bg.color2",
                    "To",
                    bg.color2.as_deref().unwrap_or(&bg.color),
                    None,
                    true,
                    |t, c| {
                        let mut n = t.clone();
                        n.background.color2 = Some(c);
                        n
                    },
                    |t, _| t.clone(),
                    window,
                    cx,
                );
                row = row.child(labelled("To", to, cx));
            }
            col = col.child(row);
        }
        if gradient {
            let angle = bg.angle_deg.unwrap_or(0.0);
            col = col.child(self.slider(
                "bg.angle",
                "Angle",
                angle,
                (0.0, 360.0, 1.0),
                format!("{}°", kairo_domain::js::number_to_string(angle)),
                |t, v| {
                    let mut n = t.clone();
                    n.background.angle_deg = Some(v);
                    n
                },
                window,
                cx,
            ));
        }
        let media = match bg.kind {
            OverlayBackgroundType::Image => Some(MediaKind::Image),
            OverlayBackgroundType::Video => Some(MediaKind::Video),
            _ => None,
        };
        if let Some(kind) = media {
            let editor = self.editor.clone();
            col = col.child(media_picker(
                kind,
                bg.media_path.as_deref().unwrap_or(""),
                move |_, cx| editor.update(cx, |e, cx| e.pick_background_media(kind, cx)),
                cx,
            ));
            col = col.child(self.fit_control(
                "bg.fit",
                bg.media_fit.unwrap_or(MediaFit::Cover),
                |t, fit| {
                    let mut n = t.clone();
                    n.background.media_fit = Some(fit);
                    n
                },
                cx,
            ));
        }
        if bg.kind != OverlayBackgroundType::Transparent {
            col = col.child(self.slider(
                "bg.opacity",
                "Opacity",
                bg.opacity,
                (0.0, 1.0, 0.05),
                panels::pct(bg.opacity),
                |t, v| {
                    let mut n = t.clone();
                    n.background.opacity = v;
                    n
                },
                window,
                cx,
            ));
        }
        if media.is_some() {
            col = col.child(self.adjustments(
                "bg",
                Adjustments::of_background(theme),
                Adjustments::of_background,
                |t, a| {
                    let mut n = t.clone();
                    let bg = &mut n.background;
                    bg.hue = Some(a.hue);
                    bg.saturation = Some(a.saturation);
                    bg.brightness = Some(a.brightness);
                    bg.contrast = Some(a.contrast);
                    bg.blur_px = Some(a.blur_px);
                    n
                },
                window,
                cx,
            ));
        }
        col.into_any_element()
    }

    fn fit_control(
        &self,
        key: &'static str,
        fit: MediaFit,
        on: impl Fn(&OverlayTheme, MediaFit) -> OverlayTheme + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity();
        let on = Rc::new(on);
        labelled(
            "Fit",
            SegmentedControl::new(SharedString::from(ids::field(key)), "Fit")
                .fit()
                .options([
                    SegmentOption::new("cover", "Fill"),
                    SegmentOption::new("contain", "Fit"),
                    SegmentOption::new("fill", "Stretch"),
                ])
                .selected(fit.as_str())
                .on_change(move |v, _, cx| {
                    let Some(fit) = MediaFit::from_name(v) else {
                        return;
                    };
                    let on = on.clone();
                    this.update(cx, |t, cx| t.edit(key, move |th| on(th, fit), cx));
                }),
            cx,
        )
    }

    /// `MediaAdjustments`: brightness, contrast, saturation, hue and blur,
    /// with Reset once anything is changed.
    fn adjustments(
        &mut self,
        scope: &str,
        a: Adjustments,
        get: impl Fn(&OverlayTheme) -> Adjustments + 'static,
        set: impl Fn(&OverlayTheme, Adjustments) -> OverlayTheme + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let set = Rc::new(set);
        let get = Rc::new(get);
        let this = cx.entity();
        let reset = {
            let set = set.clone();
            let key = format!("{scope}.adjust");
            a.adjusted().then(|| {
                Button::new(ids::ADJUST_RESET)
                    .cursor_pointer()
                    .ghost()
                    .xsmall()
                    .label("Reset")
                    .on_click(move |_, _, cx| {
                        let set = set.clone();
                        let key = key.clone();
                        this.update(cx, |t, cx| {
                            t.edit(&key, move |th| set(th, panels::NEUTRAL), cx)
                        });
                    })
            })
        };
        let rows: [AdjustRow; 5] = [
            (
                "brightness",
                "Brightness",
                a.brightness,
                (0.25, 1.75, 0.05),
                panels::pct(a.brightness),
                |a, v| a.brightness = v,
            ),
            (
                "contrast",
                "Contrast",
                a.contrast,
                (0.25, 1.75, 0.05),
                panels::pct(a.contrast),
                |a, v| a.contrast = v,
            ),
            (
                "saturation",
                "Saturation",
                a.saturation,
                (0.0, 2.0, 0.05),
                panels::pct(a.saturation),
                |a, v| a.saturation = v,
            ),
            (
                "hue",
                "Hue",
                a.hue,
                (-180.0, 180.0, 1.0),
                format!("{}°", kairo_domain::js::number_to_string(a.hue)),
                |a, v| a.hue = v,
            ),
            (
                "blur",
                "Blur",
                a.blur_px,
                (0.0, 40.0, 1.0),
                format!("{}px", kairo_domain::js::number_to_string(a.blur_px)),
                |a, v| a.blur_px = v,
            ),
        ];
        let mut col = v_flex().gap_4().child(
            h_flex()
                .justify_between()
                .child(field_label("Adjustments", cx))
                .children(reset),
        );
        for (name, label, value, range, shown, pick) in rows {
            let (set, get) = (set.clone(), get.clone());
            let _ = k;
            col = col.child(self.slider(
                &format!("{scope}.{name}"),
                label,
                value,
                range,
                shown,
                // Start from the theme as it is now, not as it was when this
                // slider was first drawn: other sliders may have moved since.
                move |t, v| {
                    let mut next = get(t);
                    pick(&mut next, v);
                    set(t, next)
                },
                window,
                cx,
            ));
        }
        col.into_any_element()
    }

    /// A dropdown's change as an edit by `key`.
    fn on_choice(
        &self,
        key: &'static str,
        on: impl Fn(&OverlayTheme, String) -> OverlayTheme + 'static,
        cx: &mut Context<Self>,
    ) -> Change {
        let this = cx.entity();
        let on = Rc::new(on);
        Rc::new(move |v: String, _: &mut Window, cx: &mut App| {
            let on = on.clone();
            this.update(cx, |t, cx| t.edit(key, move |th| on(th, v), cx));
        })
    }

    // ─── Text ──────────────────────────────────────────────────────────────

    fn render_text(
        &mut self,
        theme: &OverlayTheme,
        kind: OverlayContentKind,
        layer: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let is_reference = layer == REFERENCE;
        let style = panels::layer_style(theme, layer).clone();
        let reference_label = overlay_layer_label(kind, OverlayLayer::Reference);
        let automatic = panels::automatic_size(theme, layer);
        let this = cx.entity();

        // The layer and, for the reference, whether it shows.
        let mut head = section("layer", cx).child(
            div()
                .text_size(text::SM.size)
                .font_weight(FontWeight::SEMIBOLD)
                .child(overlay_layer_label(
                    kind,
                    if is_reference {
                        OverlayLayer::Reference
                    } else {
                        OverlayLayer::Verse
                    },
                )),
        );
        if is_reference {
            let this = this.clone();
            head = head.child(
                Checkbox::new(ids::SHOW_REFERENCE)
                    .checked(theme.reference.show)
                    .label(format!("Show {}", reference_label.to_lowercase()))
                    .on_click(move |on, _, cx| {
                        let on = *on;
                        this.update(cx, |t, cx| {
                            t.edit(
                                "text.show",
                                move |th| {
                                    let mut n = th.clone();
                                    n.reference.show = on;
                                    n
                                },
                                cx,
                            )
                        });
                    }),
            );
        }

        // Font, weight and size, decoration and spacing, capitalization.
        let font = self.font_picker(layer, &style.font_family, cx);
        let weight = select(
            ids::WEIGHT,
            "Weight",
            &kairo_domain::js::number_to_string(panels::nearest_weight(style.font_weight)),
            panels::WEIGHTS
                .iter()
                .map(|(l, v)| choice(&kairo_domain::js::number_to_string(*v), l))
                .collect(),
            self.on_choice(
                "text.weight",
                move |t, v| {
                    let w = v.parse().unwrap_or(400.0);
                    panels::edit_layer(t, layer, |s| s.font_weight = w)
                },
                cx,
            ),
        );
        let size = self.size_field(style.font_size_px, layer, window, cx);
        let deco = self.decoration(style.text_decoration, layer, cx);
        let spacing = self.spacing(&style, layer, cx);
        let caps = select(
            "theme-caps",
            "Capitalization",
            style.text_transform.as_str(),
            vec![
                choice("none", "None"),
                choice("uppercase", "UPPERCASE"),
                choice("capitalize", "Title Case"),
                choice("lowercase", "lowercase"),
            ],
            self.on_choice(
                "text.caps",
                move |t, v| {
                    let tr =
                        OverlayTextTransform::from_name(&v).unwrap_or(OverlayTextTransform::None);
                    panels::edit_layer(t, layer, |s| s.text_transform = tr)
                },
                cx,
            ),
        );
        let type_section = section("type", cx)
            .child(font)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(weight))
                    .child(div().w(px(104.)).child(size)),
            )
            .child(
                h_flex()
                    .justify_between()
                    .gap_2()
                    .child(deco)
                    .child(spacing),
            )
            .child(row("Capitalization", caps, cx));

        let color = self.color(
            "text.color",
            "Text color",
            &style.color,
            Some(style.color_opacity),
            false,
            move |t, c| panels::edit_layer(t, layer, |s| s.color = c),
            move |t, o| panels::edit_layer(t, layer, |s| s.color_opacity = o),
            window,
            cx,
        );
        let color_section = section("color", cx).child(row("Text color", color, cx));

        let sizing = if is_reference {
            row(
                "Position",
                select(
                    ids::POSITION,
                    "Position",
                    theme.reference.position.as_str(),
                    vec![
                        choice("above", "Above verse"),
                        choice("below", "Below verse"),
                    ],
                    self.on_choice(
                        "text.position",
                        |t, v| match ReferencePosition::from_name(&v) {
                            Some(p) => panels::set_reference_position(t, p),
                            None => t.clone(),
                        },
                        cx,
                    ),
                ),
                cx,
            )
        } else {
            row(
                "Scaling",
                select(
                    ids::SCALING,
                    "Scaling",
                    if theme.layout.auto_fit_text {
                        "fit"
                    } else {
                        "none"
                    },
                    vec![choice("none", "None"), choice("fit", "Fit to box")],
                    self.on_choice(
                        "text.scaling",
                        |t, v| {
                            let mut n = t.clone();
                            n.layout.auto_fit_text = v == "fit";
                            n
                        },
                        cx,
                    ),
                ),
                cx,
            )
        };
        let sizing_section = section("sizing", cx).child(sizing).when(automatic, |s| {
            s.child(
                div()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T400))
                    .child("Size follows the box. Enter a size to switch to manual sizing."),
            )
        });

        let h_align = icon_group(
            "h",
            "Horizontal alignment",
            style.align.as_str(),
            &[
                ("left", "Left", KairoIcon::AlignLeft),
                ("center", "Center", KairoIcon::AlignCenter),
                ("right", "Right", KairoIcon::AlignRight),
                ("justify", "Justify", KairoIcon::AlignJustify),
            ],
            self.on_choice(
                "text.align",
                move |t, v| {
                    let a = OverlayTextAlign::from_name(&v).unwrap_or(OverlayTextAlign::Center);
                    panels::edit_layer(t, layer, |s| s.align = a)
                },
                cx,
            ),
            cx,
        );
        let v_align = icon_group(
            "v",
            "Vertical alignment",
            style.vertical_align.as_str(),
            &[
                ("top", "Top", KairoIcon::ObjectAlignTop),
                ("middle", "Middle", KairoIcon::ObjectAlignMiddle),
                ("bottom", "Bottom", KairoIcon::ObjectAlignBottom),
            ],
            self.on_choice(
                "text.valign",
                move |t, v| {
                    let a =
                        OverlayVerticalAlign::from_name(&v).unwrap_or(OverlayVerticalAlign::Middle);
                    panels::edit_layer(t, layer, |s| s.vertical_align = a)
                },
                cx,
            ),
            cx,
        );
        let align_section = section("align", cx).child(h_align).child(v_align);

        // Shadow.
        let shadow = style.shadow.clone();
        let mut shadow_section = section("shadow", cx).child({
            let this = this.clone();
            Checkbox::new(ids::SHADOW)
                .checked(shadow.enabled)
                .label("Shadow")
                .on_click(move |on, _, cx| {
                    let on = *on;
                    this.update(cx, |t, cx| {
                        t.edit(
                            "text.shadow",
                            move |th| panels::edit_layer(th, layer, |s| s.shadow.enabled = on),
                            cx,
                        )
                    });
                })
        });
        if shadow.enabled {
            let x = self.number(
                "shadow.x",
                "X",
                shadow.x_px,
                (1.0, -40.0, 40.0),
                None,
                move |t, v| panels::edit_layer(t, layer, |s| s.shadow.x_px = v),
                window,
                cx,
            );
            let y = self.number(
                "shadow.y",
                "Y",
                shadow.y_px,
                (1.0, -40.0, 40.0),
                None,
                move |t, v| panels::edit_layer(t, layer, |s| s.shadow.y_px = v),
                window,
                cx,
            );
            let blur = self.number(
                "shadow.blur",
                "Blur",
                shadow.blur_px,
                (1.0, 0.0, 80.0),
                None,
                move |t, v| panels::edit_layer(t, layer, |s| s.shadow.blur_px = v),
                window,
                cx,
            );
            let color = self.color(
                "shadow.color",
                "Shadow color",
                &shadow.color,
                Some(shadow.opacity),
                false,
                move |t, c| panels::edit_layer(t, layer, |s| s.shadow.color = c),
                move |t, o| panels::edit_layer(t, layer, |s| s.shadow.opacity = o),
                window,
                cx,
            );
            shadow_section = shadow_section
                .child(grid3(x, y, blur))
                .child(row("Color", color, cx));
        }

        // Stroke.
        let outline = style.outline.clone();
        let mut stroke_section = section("stroke", cx).child({
            let this = this.clone();
            Checkbox::new(ids::STROKE)
                .checked(outline.enabled)
                .label("Stroke")
                .on_click(move |on, _, cx| {
                    let on = *on;
                    this.update(cx, |t, cx| {
                        t.edit(
                            "text.stroke",
                            move |th| panels::set_outline_enabled(th, layer, on),
                            cx,
                        )
                    });
                })
        });
        if outline.enabled {
            let position = labelled(
                "Position",
                select(
                    "theme-stroke-position",
                    "Position",
                    outline.position.as_str(),
                    vec![
                        choice("outside", "Outside"),
                        choice("center", "Center"),
                        choice("inside", "Inside"),
                    ],
                    self.on_choice(
                        "stroke.position",
                        move |t, v| {
                            let p = OverlayOutlinePosition::from_name(&v)
                                .unwrap_or(OverlayOutlinePosition::Outside);
                            panels::edit_layer(t, layer, |s| s.outline.position = p)
                        },
                        cx,
                    ),
                ),
                cx,
            );
            let width = self.number(
                "stroke.width",
                "Width",
                outline.width_px,
                (1.0, 0.0, 24.0),
                Some("px"),
                move |t, v| panels::edit_layer(t, layer, |s| s.outline.width_px = v),
                window,
                cx,
            );
            let style_select = select(
                "theme-stroke-style",
                "Stroke style",
                outline.style.as_str(),
                vec![
                    choice("solid", "Solid"),
                    choice("dashed", "Dashed"),
                    choice("dotted", "Dotted"),
                ],
                self.on_choice(
                    "stroke.style",
                    move |t, v| {
                        let st = OverlayOutlineStyle::from_name(&v)
                            .unwrap_or(OverlayOutlineStyle::Solid);
                        panels::edit_layer(t, layer, |s| s.outline.style = st)
                    },
                    cx,
                ),
            );
            let color = self.color(
                "stroke.color",
                "Stroke color",
                &outline.color,
                Some(outline.opacity),
                false,
                move |t, c| panels::edit_layer(t, layer, |s| s.outline.color = c),
                move |t, o| panels::edit_layer(t, layer, |s| s.outline.opacity = o),
                window,
                cx,
            );
            stroke_section = stroke_section
                .child(
                    h_flex()
                        .gap_2()
                        .items_end()
                        .child(div().flex_1().min_w_0().child(position))
                        .child(div().w(px(104.)).child(width)),
                )
                .child(row("Style", style_select, cx))
                .child(row("Color", color, cx));
        }

        v_flex()
            .child(head)
            .child(type_section)
            .child(color_section)
            .child(sizing_section)
            .child(align_section)
            .child(shadow_section)
            .child(stroke_section)
            .into_any_element()
    }

    /// `FontSizeField`: commits on blur or Enter, 12–200, with the preset
    /// sizes in a list beside it.
    fn size_field(
        &mut self,
        value: f64,
        layer: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let input = match &self.size {
            Some(i) => {
                let i = i.clone();
                let focused = i.read(cx).focus_handle(cx).is_focused(window);
                let shown = kairo_domain::js::number_to_string(value);
                if !focused && i.read(cx).value() != shown.as_str() {
                    i.update(cx, |i, cx| i.set_value(shown, window, cx));
                }
                i
            }
            None => {
                let i = cx.new(|cx| {
                    InputState::new(window, cx)
                        .default_value(kairo_domain::js::number_to_string(value))
                });
                self.field_subs.push(cx.subscribe_in(
                    &i,
                    window,
                    move |this, input, e: &InputEvent, window, cx| {
                        if matches!(e, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let draft = input.read(cx).value().to_string();
                            let current = panels::layer_style(this.editor.read(cx).draft(), layer)
                                .font_size_px;
                            let (shown, changed) = panels::commit_font_size(&draft, current);
                            input.update(cx, |i, cx| i.set_value(shown, window, cx));
                            if let Some(size) = changed {
                                this.edit(
                                    "text.size",
                                    move |t| panels::set_font_size(t, layer, size),
                                    cx,
                                );
                            }
                        }
                    },
                ));
                self.size = Some(i.clone());
                i
            }
        };
        let this = cx.entity();
        let presets = Button::new(ids::SIZE_PRESETS)
            .cursor_pointer()
            .ghost()
            .xsmall()
            .icon(Icon::new(KairoIcon::ChevronDown))
            .accessibility_label("Preset sizes")
            .popover_trigger()
            .dropdown_menu(move |mut menu, _, _| {
                for size in panels::SIZE_PRESETS {
                    let this = this.clone();
                    menu = menu.item(
                        PopupMenuItem::new(kairo_domain::js::number_to_string(size))
                            .checked(size == value)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |t, cx| {
                                    if size != value {
                                        t.edit(
                                            "text.size-preset",
                                            move |th| panels::set_font_size(th, layer, size),
                                            cx,
                                        );
                                    }
                                })
                            }),
                    );
                }
                menu
            });
        h_flex()
            .id(ids::SIZE)
            .test_support()
            .h_8()
            .pl_2()
            .rounded(radius::md())
            .bg(k.surface_tertiary)
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&input)
                        .kairo_size()
                        .appearance(false)
                        .small()
                        .aria_label("Size"),
                ),
            )
            .child(presets)
            .into_any_element()
    }

    /// A number box by its key (`box.w`, `element.opacity`), while shown.
    pub fn number_field(&self, key: &str) -> Option<Entity<NumberField>> {
        self.numbers.get(key).cloned()
    }

    /// The size box's input, for tests.
    pub fn size_input(&self) -> Option<Entity<InputState>> {
        self.size.clone()
    }

    /// `DecorationToggles`: underline and strikethrough; the lit one turns off.
    fn decoration(
        &self,
        value: OverlayTextDecoration,
        layer: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        h_flex()
            .h_7()
            .gap_0p5()
            .p_0p5()
            .rounded(radius::md())
            .bg(k.surface_tertiary)
            .children(
                [
                    (
                        OverlayTextDecoration::Underline,
                        "underline",
                        "Underline",
                        "U",
                    ),
                    (
                        OverlayTextDecoration::LineThrough,
                        "strike",
                        "Strikethrough",
                        "S",
                    ),
                ]
                .into_iter()
                .map(|(deco, key, label, glyph)| {
                    let active = value == deco;
                    let this = this.clone();
                    Button::new(SharedString::from(ids::decoration(key)))
                        .cursor_pointer()
                        .xsmall()
                        .when(active, |b| b.primary())
                        .when(!active, |b| b.ghost())
                        .label(glyph)
                        .toggled(active)
                        .accessibility_label(label)
                        .tooltip(label)
                        .on_click(move |_, _, cx| {
                            let next = if active {
                                OverlayTextDecoration::None
                            } else {
                                deco
                            };
                            this.update(cx, |t, cx| {
                                t.edit(
                                    "text.decoration",
                                    move |th| {
                                        panels::edit_layer(th, layer, |s| s.text_decoration = next)
                                    },
                                    cx,
                                )
                            });
                        })
                }),
            )
            .into_any_element()
    }

    /// `SpacingPopover`: line height and letter spacing behind the gear.
    fn spacing(
        &self,
        _style: &kairo_domain::overlay::OverlayTextStyle,
        layer: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity();
        let open = self.spacing_open;
        Popover::new(ids::SPACING)
            .anchor(gpui_kit::Anchor::TopRight)
            .open(open)
            .on_open_change({
                let this = this.clone();
                move |next, _, cx| {
                    let next = *next;
                    this.update(cx, |t, cx| {
                        t.spacing_open = next;
                        if !next {
                            t.numbers.remove("spacing.line");
                            t.numbers.remove("spacing.letter");
                        }
                        cx.notify();
                    })
                }
            })
            .button_trigger(
                Button::new("theme-spacing-trigger")
                    .cursor_pointer()
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(KairoIcon::Settings))
                    .dropdown_caret(true)
                    .accessibility_label("Spacing")
                    .tooltip("Line height and letter spacing"),
            )
            .content(move |_, window, cx| {
                this.update(cx, |t, cx| t.spacing_content(layer, window, cx))
            })
            .into_any_element()
    }

    fn spacing_content(
        &mut self,
        layer: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let style = panels::layer_style(self.editor.read(cx).draft(), layer).clone();
        let line = self.number(
            "spacing.line",
            "Line height",
            (style.line_height * 100.0).round() / 100.0,
            (0.05, 0.9, 2.5),
            None,
            move |t, v| panels::edit_layer(t, layer, |s| s.line_height = v),
            window,
            cx,
        );
        let letter = self.number(
            "spacing.letter",
            "Letter spacing",
            (style.letter_spacing_px * 10.0).round() / 10.0,
            (0.5, -4.0, 20.0),
            Some("px"),
            move |t, v| panels::edit_layer(t, layer, |s| s.letter_spacing_px = v),
            window,
            cx,
        );
        h_flex()
            .w(px(240.))
            .gap_2()
            .child(div().flex_1().child(line))
            .child(div().flex_1().child(letter))
            .into_any_element()
    }

    // ─── Layout ────────────────────────────────────────────────────────────

    fn render_layout(
        &mut self,
        theme: &OverlayTheme,
        kind: OverlayContentKind,
        layer: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let layout = theme.layout.clone();
        let full = layout.position == OverlayLayoutPosition::Full;

        // Placement.
        let mut placement = v_flex().gap_3();
        if self.placement_open {
            let presets = [
                (OverlayLayoutPosition::LowerThird, "Lower third"),
                (OverlayLayoutPosition::Center, "Center"),
                (OverlayLayoutPosition::Top, "Top"),
                (OverlayLayoutPosition::Full, "Fullscreen"),
            ];
            let grid = h_flex()
                .flex_wrap()
                .gap_1()
                .p_0p5()
                .rounded(radius::lg())
                .bg(k.surface)
                .children(presets.into_iter().map(|(position, label)| {
                    let this = this.clone();
                    let active = layout.position == position;
                    div().w(relative_half()).child(
                        Button::new(SharedString::from(ids::preset(position.as_str())))
                            .cursor_pointer()
                            .small()
                            .w_full()
                            .when(active, |b| b.toggled(true))
                            .when(!active, |b| b.ghost())
                            .label(label)
                            .on_click(move |_, _, cx| {
                                this.update(cx, |t, cx| {
                                    t.edit(
                                        "layout.preset",
                                        move |th| apply_layout_preset(th, position, None),
                                        cx,
                                    );
                                    t.editor
                                        .update(cx, |e, cx| e.select_layer(Some(VERSE.into()), cx));
                                })
                            }),
                    )
                }));
            placement = placement.child(labelled("Position", grid, cx));
            let mut sliders = h_flex().gap_2();
            if !full {
                sliders = sliders.child(div().flex_1().child(self.slider(
                    "layout.width",
                    "Width",
                    layout.max_width_pct,
                    (20.0, 100.0, 1.0),
                    format!(
                        "{}%",
                        kairo_domain::js::number_to_string(layout.max_width_pct)
                    ),
                    |t, v| apply_layout_preset(t, t.layout.position, Some(v)),
                    window,
                    cx,
                )));
            }
            sliders = sliders.child(div().flex_1().child(self.slider(
                "layout.padding",
                "Padding",
                layout.padding_px,
                (0.0, 200.0, 1.0),
                format!(
                    "{}px",
                    kairo_domain::js::number_to_string(layout.padding_px)
                ),
                |t, v| {
                    let mut n = t.clone();
                    n.layout.padding_px = v;
                    n
                },
                window,
                cx,
            )));
            placement = placement.child(sliders);
        }

        // The selected text box.
        let mut boxed = v_flex().gap_3();
        if self.box_open {
            let switch = h_flex()
                .gap_1()
                .p_0p5()
                .rounded(radius::lg())
                .bg(k.surface)
                .children([VERSE, REFERENCE].into_iter().map(|id| {
                    let label = overlay_layer_label(
                        kind,
                        if id == VERSE {
                            OverlayLayer::Verse
                        } else {
                            OverlayLayer::Reference
                        },
                    );
                    let this = this.clone();
                    let active = layer == id;
                    div().flex_1().child(
                        Button::new(SharedString::from(ids::layer_switch(id)))
                            .cursor_pointer()
                            .small()
                            .w_full()
                            .when(active, |b| b.toggled(true))
                            .when(!active, |b| b.ghost())
                            .label(label)
                            // A hidden reference comes back from here (the
                            // toolbar's Text button adds text boxes instead).
                            .when(id == REFERENCE && !theme.reference.show, |b| {
                                b.tooltip("Show the reference on the slide")
                            })
                            .on_click(move |_, _, cx| {
                                this.update(cx, |t, cx| {
                                    t.editor.update(cx, |e, cx| {
                                        if id == REFERENCE && !e.draft().reference.show {
                                            e.show_reference(cx)
                                        } else {
                                            e.select_layer(Some(id.into()), cx)
                                        }
                                    })
                                })
                            }),
                    )
                }));
            let frame = panels::layer_style(theme, layer).frame.clone();
            let set = move |field: fn(&mut kairo_domain::overlay::OverlayBox, f64)| {
                move |t: &OverlayTheme, v: f64| {
                    let mut b = panels::layer_style(t, layer).frame.clone();
                    field(&mut b, v);
                    let b = clamp_overlay_box(&b, TEXT_BOX_MIN);
                    panels::edit_layer(t, layer, |s| s.frame = b)
                }
            };
            let one = |v: f64| (v * 10.0).round() / 10.0;
            let x = self.number(
                "box.x",
                "X",
                one(frame.x_pct),
                (0.5, 0.0, 100.0),
                Some("%"),
                set(|b, v| b.x_pct = v),
                window,
                cx,
            );
            let y = self.number(
                "box.y",
                "Y",
                one(frame.y_pct),
                (0.5, 0.0, 100.0),
                Some("%"),
                set(|b, v| b.y_pct = v),
                window,
                cx,
            );
            let w = self.number(
                "box.w",
                "Width",
                one(frame.width_pct),
                (0.5, 8.0, 100.0),
                Some("%"),
                set(|b, v| b.width_pct = v),
                window,
                cx,
            );
            let h = self.number(
                "box.h",
                "Height",
                one(frame.height_pct),
                (0.5, 6.0, 100.0),
                Some("%"),
                set(|b, v| b.height_pct = v),
                window,
                cx,
            );
            boxed = boxed.child(switch).child(grid2(x, y)).child(grid2(w, h));
        }

        // The backdrop.
        let mut backdrop = v_flex().gap_3();
        if self.backdrop_open {
            let (hex, opacity) = panels::parse_color_parts(&layout.backdrop_color);
            let color = self.color(
                "backdrop.color",
                "Color",
                &hex,
                Some(opacity),
                false,
                move |t, c| {
                    let (_, o) = panels::parse_color_parts(&t.layout.backdrop_color);
                    let mut n = t.clone();
                    n.layout.backdrop_color = panels::color_with_opacity(&c, o);
                    n
                },
                move |t, o| {
                    let (h, _) = panels::parse_color_parts(&t.layout.backdrop_color);
                    let mut n = t.clone();
                    n.layout.backdrop_color = panels::color_with_opacity(&h, o);
                    n
                },
                window,
                cx,
            );
            backdrop = backdrop.child(
                div()
                    .when(!layout.backdrop_box, |d| d.opacity(0.4).occlude())
                    .child(labelled("Color", color, cx)),
            );
        }
        let eye = {
            let this = this.clone();
            let on = layout.backdrop_box;
            Button::new(ids::BACKDROP_EYE)
                .cursor_pointer()
                .ghost()
                .xsmall()
                .icon(Icon::new(if on {
                    KairoIcon::Eye
                } else {
                    KairoIcon::EyeOff
                }))
                .accessibility_label(if on { "Hide backdrop" } else { "Show backdrop" })
                .on_click(move |_, _, cx| {
                    this.update(cx, |t, cx| {
                        t.edit(
                            "backdrop.eye",
                            move |th| {
                                let mut n = th.clone();
                                n.layout.backdrop_box = !on;
                                n
                            },
                            cx,
                        )
                    })
                })
        };

        v_flex()
            .gap_5()
            .child(self.collapsible(
                "placement",
                "Placement",
                self.placement_open,
                None,
                placement.into_any_element(),
                cx,
            ))
            .child(self.collapsible(
                "box",
                "Selected box",
                self.box_open,
                None,
                boxed.into_any_element(),
                cx,
            ))
            .child(self.collapsible(
                "backdrop",
                "Backdrop",
                self.backdrop_open,
                Some(eye.into_any_element()),
                backdrop.into_any_element(),
                cx,
            ))
            .into_any_element()
    }

    fn collapsible(
        &self,
        key: &'static str,
        title: &'static str,
        open: bool,
        trailing: Option<AnyElement>,
        body: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .id(SharedString::from(ids::section(key)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(title)
                            .flex_1()
                            .min_w_0()
                            .gap_1p5()
                            .cursor_pointer()
                            .on_click(move |_, _, cx| {
                                this.update(cx, |t, cx| {
                                    match key {
                                        "placement" => t.placement_open = !t.placement_open,
                                        "box" => t.box_open = !t.box_open,
                                        _ => t.backdrop_open = !t.backdrop_open,
                                    }
                                    t.clear_fields();
                                    cx.notify();
                                })
                            })
                            .child(
                                Icon::new(if open {
                                    KairoIcon::ChevronDown
                                } else {
                                    KairoIcon::ChevronRight
                                })
                                .size_3()
                                .text_color(k.text(TextStep::T600)),
                            )
                            .child(
                                div()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T300))
                                    .child(title),
                            ),
                    )
                    .children(trailing),
            )
            .when(open, |d| d.child(body))
            .into_any_element()
    }

    // ─── Element ───────────────────────────────────────────────────────────

    fn render_element(
        &mut self,
        el: &OverlayElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let media = is_media_element(el.kind);
        let line = element_is_line(el);
        let label = overlay_element_label(el.kind, el.shape.as_deref());
        let id = el.id.clone();

        let header = {
            let (dup, del, done) = (this.clone(), this.clone(), this.clone());
            let (id_dup, id_del) = (id.clone(), id.clone());
            v_flex()
                .gap_3()
                .child(
                    Button::new(ids::DONE)
                        .cursor_pointer()
                        .ghost()
                        .xsmall()
                        .label("← Theme settings")
                        .on_click(move |_, _, cx| {
                            done.update(cx, |t, cx| {
                                t.editor.update(cx, |e, cx| e.select_layer(None, cx))
                            })
                        }),
                )
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            v_flex()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(if media {
                                            "Edit element"
                                        } else if el.kind == OverlayElementKind::Text {
                                            "Edit text"
                                        } else {
                                            "Edit shape"
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(text::XS.size)
                                        .text_color(k.text(TextStep::T400))
                                        .child(label),
                                ),
                        )
                        .child(
                            h_flex()
                                .child(
                                    Button::new(ids::DUPLICATE)
                                        .cursor_pointer()
                                        .ghost()
                                        .small()
                                        .icon(Icon::new(KairoIcon::Copy))
                                        .accessibility_label(format!(
                                            "Duplicate {}",
                                            label.to_lowercase()
                                        ))
                                        .tooltip("Duplicate")
                                        .on_click(move |_, _, cx| {
                                            let id = id_dup.clone();
                                            dup.update(cx, |t, cx| {
                                                t.editor.update(cx, |e, cx| {
                                                    e.duplicate_element(&id, cx)
                                                })
                                            })
                                        }),
                                )
                                .child(
                                    Button::new(ids::DELETE)
                                        .cursor_pointer()
                                        .ghost()
                                        .small()
                                        .icon(Icon::new(KairoIcon::Trash2))
                                        .accessibility_label(format!(
                                            "Delete {}",
                                            label.to_lowercase()
                                        ))
                                        .tooltip("Delete")
                                        .on_click(move |_, _, cx| {
                                            let id = id_del.clone();
                                            del.update(cx, |t, cx| {
                                                t.editor.update(cx, |e, cx| e.delete_layer(&id, cx))
                                            })
                                        }),
                                ),
                        ),
                )
        };

        let flips = h_flex().gap_2().children(
            [
                ("x", "Horizontal", KairoIcon::FlipHorizontal, el.flip_x),
                ("y", "Vertical", KairoIcon::FlipVertical, el.flip_y),
            ]
            .into_iter()
            .map(|(key, text_, icon, on)| {
                let (this, id) = (this.clone(), id.clone());
                div().flex_1().child(
                    Button::new(SharedString::from(ids::flip(key)))
                        .cursor_pointer()
                        .small()
                        .w_full()
                        .icon(Icon::new(icon))
                        .label(text_)
                        .toggled(on)
                        .when(on, |b| b.primary())
                        .accessibility_label(format!("Flip {}", text_.to_lowercase()))
                        .tooltip(format!("Mirror {}", text_.to_lowercase()))
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            this.update(cx, |t, cx| {
                                t.edit_element(
                                    "element.flip",
                                    &id,
                                    move |e| {
                                        if key == "x" {
                                            e.flip_x = !on
                                        } else {
                                            e.flip_y = !on
                                        }
                                    },
                                    cx,
                                )
                            })
                        }),
                )
            }),
        );

        let aligns = [
            (
                "left",
                "Left",
                KairoIcon::ObjectAlignLeft,
                ElementAlignment::Left,
            ),
            (
                "center",
                "Centre",
                KairoIcon::ObjectAlignCenter,
                ElementAlignment::Center,
            ),
            (
                "right",
                "Right",
                KairoIcon::ObjectAlignRight,
                ElementAlignment::Right,
            ),
            (
                "top",
                "Top",
                KairoIcon::ObjectAlignTop,
                ElementAlignment::Top,
            ),
            (
                "middle",
                "Middle",
                KairoIcon::ObjectAlignMiddle,
                ElementAlignment::Middle,
            ),
            (
                "bottom",
                "Bottom",
                KairoIcon::ObjectAlignBottom,
                ElementAlignment::Bottom,
            ),
        ];
        let align = h_flex()
            .flex_wrap()
            .gap_1()
            .children(aligns.into_iter().map(|(key, text_, icon, a)| {
                let (this, id) = (this.clone(), id.clone());
                div().w(relative_third()).child(
                    Button::new(SharedString::from(ids::element_align(key)))
                        .cursor_pointer()
                        .small()
                        .w_full()
                        .icon(Icon::new(icon))
                        .label(text_)
                        .accessibility_label(format!("Align {} to slide", text_.to_lowercase()))
                        .tooltip(format!("Align {} to slide", text_.to_lowercase()))
                        .on_click(move |_, _, cx| {
                            let id = id.clone();
                            this.update(cx, |t, cx| {
                                t.edit_element(
                                    "element.align",
                                    &id,
                                    move |e| e.frame = align_overlay_element(&e.frame, a),
                                    cx,
                                )
                            })
                        }),
                )
            }));

        let mut col = v_flex()
            .gap_4()
            .child(header)
            .child(labelled("Flip", flips, cx))
            .child(labelled("Align to slide", align, cx));

        if media {
            let kind = if el.kind == OverlayElementKind::Video {
                MediaKind::Video
            } else {
                MediaKind::Image
            };
            let (editor, mid) = (self.editor.clone(), id.clone());
            col = col.child(media_picker(
                kind,
                el.media_path.as_deref().unwrap_or(""),
                move |_, cx| editor.update(cx, |e, cx| e.replace_element_media(&mid, cx)),
                cx,
            ));
            let fid = id.clone();
            col = col.child(self.fit_control(
                "element.fit",
                el.media_fit,
                move |t, fit| {
                    let mut n = t.clone();
                    n.elements = update_overlay_element(&t.elements, &fid, |e| e.media_fit = fit);
                    n
                },
                cx,
            ));
            if el.kind == OverlayElementKind::Video {
                let (this, pid) = (this.clone(), id.clone());
                col = col.child(labelled(
                    "Playback",
                    SegmentedControl::new(
                        SharedString::from(ids::field("element.playback")),
                        "Playback",
                    )
                    .fit()
                    .options([
                        SegmentOption::new("loop", "Loop"),
                        SegmentOption::new("once", "Play once"),
                    ])
                    .selected(if el.media_loop { "loop" } else { "once" })
                    .on_change(move |v, _, cx| {
                        let on = v.as_ref() == "loop";
                        let id = pid.clone();
                        this.update(cx, |t, cx| {
                            t.edit_element("element.playback", &id, move |e| e.media_loop = on, cx)
                        })
                    }),
                    cx,
                ));
            }
        }

        if el.kind == OverlayElementKind::Shape
            && let Some(def) = overlay_shape(el.shape.as_deref())
        {
            col = col.child(labelled("Shape", self.shape_menu(el, def.label, cx), cx));
        }

        if el.kind == OverlayElementKind::Text {
            col = col.child(self.text_element(el, window, cx));
        }

        if !media {
            if !line && el.kind != OverlayElementKind::Text {
                let fid = id.clone();
                let oid = id.clone();
                col = col.child(labelled(
                    "Fill",
                    self.color(
                        "element.fill",
                        "Fill",
                        &el.fill,
                        Some(el.fill_opacity),
                        false,
                        move |t, c| element_edit(t, &fid, |e| e.fill = c.clone()),
                        move |t, o| element_edit(t, &oid, |e| e.fill_opacity = o),
                        window,
                        cx,
                    ),
                    cx,
                ));
            }
            let noun = if line {
                "line"
            } else if el.kind == OverlayElementKind::Text {
                "outline"
            } else {
                "border"
            };
            let stroke_on = el.stroke.enabled;
            let eye = {
                let (this, sid) = (this.clone(), id.clone());
                Button::new(ids::STROKE_EYE)
                    .cursor_pointer()
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(if stroke_on {
                        KairoIcon::Eye
                    } else {
                        KairoIcon::EyeOff
                    }))
                    .accessibility_label(format!(
                        "{} {noun}",
                        if stroke_on { "Hide" } else { "Show" }
                    ))
                    .on_click(move |_, _, cx| {
                        let id = sid.clone();
                        this.update(cx, |t, cx| {
                            t.edit_element(
                                "element.stroke-eye",
                                &id,
                                move |e| e.stroke.enabled = !stroke_on,
                                cx,
                            )
                        })
                    })
            };
            let (cid, oid, wid) = (id.clone(), id.clone(), id.clone());
            let color = self.color(
                "element.stroke",
                "Color",
                &el.stroke.color,
                Some(el.stroke.opacity),
                false,
                move |t, c| element_edit(t, &cid, |e| e.stroke.color = c.clone()),
                move |t, o| element_edit(t, &oid, |e| e.stroke.opacity = o),
                window,
                cx,
            );
            let width = self.number(
                "element.stroke-width",
                "Width",
                el.stroke.width_px,
                (1.0, 0.0, 200.0),
                Some("px"),
                move |t, v| element_edit(t, &wid, |e| e.stroke.width_px = v),
                window,
                cx,
            );
            col = col.child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(field_label(if line { "Line" } else { "Border" }, cx))
                            .child(eye),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .when(!stroke_on, |d| d.opacity(0.4).occlude())
                            .child(labelled("Color", color, cx))
                            .child(width),
                    ),
            );
            if el.kind == OverlayElementKind::Rectangle {
                let rid = id.clone();
                col = col.child(self.number(
                    "element.radius",
                    "Corner radius",
                    el.radius_px,
                    (1.0, 0.0, 1080.0),
                    Some("px"),
                    move |t, v| element_edit(t, &rid, |e| e.radius_px = v),
                    window,
                    cx,
                ));
            }
        }

        let (rid, oid) = (id.clone(), id.clone());
        col = col.child(self.number(
            "element.rotation",
            "Rotation",
            el.rotation_deg,
            (1.0, -180.0, 180.0),
            Some("°"),
            move |t, v| element_edit(t, &rid, |e| e.rotation_deg = v),
            window,
            cx,
        ));
        col = col.child(self.number(
            "element.opacity",
            "Opacity",
            kairo_domain::js::math_round(el.opacity * 100.0),
            (1.0, 0.0, 100.0),
            Some("%"),
            move |t, v| element_edit(t, &oid, |e| e.opacity = v / 100.0),
            window,
            cx,
        ));

        let frame = el.frame.clone();
        let set = |id: String, field: fn(&mut kairo_domain::overlay::OverlayBox, f64)| {
            move |t: &OverlayTheme, v: f64| {
                element_edit(t, &id, |e| {
                    let mut b = e.frame.clone();
                    field(&mut b, v);
                    e.frame = clamp_overlay_box(&b, ELEMENT_BOX_MIN);
                })
            }
        };
        let x = self.number(
            "element.x",
            "X",
            frame.x_pct,
            (0.5, 0.0, 100.0),
            Some("%"),
            set(id.clone(), |b, v| b.x_pct = v),
            window,
            cx,
        );
        let y = self.number(
            "element.y",
            "Y",
            frame.y_pct,
            (0.5, 0.0, 100.0),
            Some("%"),
            set(id.clone(), |b, v| b.y_pct = v),
            window,
            cx,
        );
        let w = self.number(
            "element.w",
            "Width",
            frame.width_pct,
            (0.5, ELEMENT_BOX_MIN.width_pct, 100.0),
            Some("%"),
            set(id.clone(), |b, v| b.width_pct = v),
            window,
            cx,
        );
        let h = self.number(
            "element.h",
            "Height",
            frame.height_pct,
            (0.5, ELEMENT_BOX_MIN.height_pct, 100.0),
            Some("%"),
            set(id.clone(), |b, v| b.height_pct = v),
            window,
            cx,
        );
        col = col.child(labelled(
            "Position and size",
            v_flex().gap_2().child(grid2(x, y)).child(grid2(w, h)),
            cx,
        ));

        let arrange = {
            let (this, aid) = (this.clone(), id.clone());
            SegmentedControl::new(SharedString::from(ids::field("element.arrange")), "Arrange")
                .fit()
                .options([
                    SegmentOption::new("under", "Under text"),
                    SegmentOption::new("over", "Over text"),
                ])
                .selected(if el.above_text { "over" } else { "under" })
                .on_change(move |v, _, cx| {
                    let over = v.as_ref() == "over";
                    let id = aid.clone();
                    this.update(cx, |t, cx| {
                        t.edit_element("element.arrange", &id, move |e| e.above_text = over, cx)
                    })
                })
        };
        let order = h_flex().gap_1().children(
            [
                ("back", "To back", ReorderTarget::Back),
                ("backward", "Back", ReorderTarget::Backward),
                ("forward", "Forward", ReorderTarget::Forward),
                ("front", "To front", ReorderTarget::Front),
            ]
            .into_iter()
            .map(|(key, text_, to)| {
                let (this, rid) = (this.clone(), id.clone());
                div().flex_1().child(
                    Button::new(SharedString::from(ids::reorder(key)))
                        .cursor_pointer()
                        .xsmall()
                        .w_full()
                        .label(text_)
                        .on_click(move |_, _, cx| {
                            let id = rid.clone();
                            this.update(cx, |t, cx| {
                                t.edit(
                                    "element.reorder",
                                    move |th| {
                                        let mut n = th.clone();
                                        n.elements = reorder_overlay_element(&th.elements, &id, to);
                                        n
                                    },
                                    cx,
                                )
                            })
                        }),
                )
            }),
        );
        col = col.child(
            v_flex()
                .gap_2()
                .child(labelled("Arrange", arrange, cx))
                .child(order),
        );

        if media {
            let aid = id.clone();
            let live = id.clone();
            // Per element, so another element's sliders never edit this one.
            col = col.child(self.adjustments(
                &format!("element.{id}"),
                Adjustments::of_element(el),
                move |t| {
                    t.elements
                        .iter()
                        .find(|e| e.id == live)
                        .map_or(panels::NEUTRAL, Adjustments::of_element)
                },
                move |t, a| {
                    element_edit(t, &aid, |e| {
                        e.hue = a.hue;
                        e.saturation = a.saturation;
                        e.brightness = a.brightness;
                        e.contrast = a.contrast;
                        e.blur_px = a.blur_px;
                    })
                },
                window,
                cx,
            ));
        }
        col.into_any_element()
    }

    /// The Shape list: every catalog shape but the plain box shapes.
    /// A text element's words, font, size, weight, colour and alignment.
    fn text_element(
        &mut self,
        el: &OverlayElement,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let this = cx.entity();
        let id = el.id.clone();
        let value = el.text.clone().unwrap_or_default();
        let input = match &self.text_input {
            Some((for_id, input)) if *for_id == id => {
                let input = input.clone();
                if self.origin.as_deref() != Some("element.text")
                    && input.read(cx).value().as_ref() != value
                {
                    input.update(cx, |s, cx| s.set_value(value.clone(), window, cx));
                }
                input
            }
            _ => {
                let input = cx.new(|cx| {
                    let mut s = TextareaState::new(window, cx);
                    s.set_value(value.clone(), window, cx);
                    s
                });
                let eid = id.clone();
                self.field_subs.push(cx.subscribe(
                    &input,
                    move |this, state, e: &InputEvent, cx| {
                        if matches!(e, InputEvent::Change) {
                            let text = state.read(cx).value().to_string();
                            this.edit_element(
                                "element.text",
                                &eid,
                                move |el| el.text = Some(text.clone()),
                                cx,
                            );
                        }
                    },
                ));
                self.text_input = Some((id.clone(), input.clone()));
                input
            }
        };
        let k = *cx.kairo();
        let words = div()
            .id(SharedString::from(ids::field("element.text")))
            .test_support()
            .rounded(radius::md())
            .bg(k.surface)
            .border_1()
            .border_color(k.ink(0.14))
            .child(Textarea::new(&input).appearance(false).aria_label("Text"));

        let family = el
            .font_family
            .clone()
            .unwrap_or_else(|| kairo_scene::element_text::default_family().to_owned());
        let font = {
            let (this, fid) = (this.clone(), id.clone());
            select(
                ids::field("element.font"),
                "Font",
                &family,
                panels::font_options(&family),
                Rc::new(move |next: String, _, cx| {
                    let id = fid.clone();
                    this.update(cx, |t, cx| {
                        t.edit_element(
                            "element.font",
                            &id,
                            move |e| e.font_family = Some(next.clone()),
                            cx,
                        )
                    })
                }),
            )
        };
        let sid = id.clone();
        let size = self.number(
            "element.size",
            "Size",
            el.font_size_px.unwrap_or(72.0),
            (1.0, 8.0, 400.0),
            Some("px"),
            move |t, v| element_edit(t, &sid, |e| e.font_size_px = Some(v)),
            window,
            cx,
        );
        let wid = id.clone();
        let weight = self.number(
            "element.weight",
            "Weight",
            el.font_weight.unwrap_or(600.0),
            (100.0, 100.0, 900.0),
            None,
            move |t, v| {
                element_edit(t, &wid, |e| {
                    e.font_weight = Some((v / 100.0).round() * 100.0)
                })
            },
            window,
            cx,
        );
        let (cid, oid) = (id.clone(), id.clone());
        let color = self.color(
            "element.text_color",
            "Text color",
            &el.fill,
            Some(el.fill_opacity),
            false,
            move |t, c| element_edit(t, &cid, |e| e.fill = c.clone()),
            move |t, o| element_edit(t, &oid, |e| e.fill_opacity = o),
            window,
            cx,
        );
        let align = {
            let (this, aid) = (this.clone(), id.clone());
            let current = el
                .text_align
                .unwrap_or(kairo_domain::overlay::OverlayTextAlign::Center);
            SegmentedControl::new(
                SharedString::from(ids::field("element.align_text")),
                "Alignment",
            )
            .fit()
            .options([
                SegmentOption::new("left", "Left"),
                SegmentOption::new("center", "Centre"),
                SegmentOption::new("right", "Right"),
            ])
            .selected(current.as_str())
            .on_change(move |v, _, cx| {
                let next = kairo_domain::overlay::OverlayTextAlign::from_name(v.as_ref());
                let id = aid.clone();
                this.update(cx, |t, cx| {
                    t.edit_element("element.align_text", &id, move |e| e.text_align = next, cx)
                })
            })
        };
        v_flex()
            .gap_4()
            .child(labelled("Text", words, cx))
            .child(labelled("Font", font, cx))
            .child(grid2(size, weight))
            .child(labelled("Text color", color, cx))
            .child(labelled("Alignment", align, cx))
            .into_any_element()
    }

    fn shape_menu(
        &self,
        el: &OverlayElement,
        current: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let this = cx.entity();
        let color = k.text(TextStep::T300);
        let editor = self.editor.read(cx);
        let current_thumb =
            crate::view::cached_thumb(&editor.thumbs, &ThemeEditor::element_tile(el), color);
        let open = self.shapes_open;
        let groups: ShapeGroups = if open {
            gallery::groups(false)
                .into_iter()
                .map(|(label, tiles)| {
                    (
                        label,
                        tiles
                            .into_iter()
                            .map(|t| {
                                let i = crate::view::cached_thumb(&editor.thumbs, &t, color);
                                (t, i)
                            })
                            .collect(),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let id = el.id.clone();
        Popover::new(ids::SHAPE)
            .open(open)
            .on_open_change({
                let this = this.clone();
                move |next, _, cx| {
                    let next = *next;
                    this.update(cx, |t, cx| {
                        t.shapes_open = next;
                        cx.notify();
                    })
                }
            })
            .button_trigger(
                Button::new("theme-element-shape-trigger")
                    .cursor_pointer()
                    .small()
                    .w_full()
                    .dropdown_caret(true)
                    .accessibility_label("Shape")
                    .child(
                        h_flex()
                            .gap_2()
                            .children(current_thumb.map(|i| img(i).w(px(16.)).h(px(16.))))
                            .child(current),
                    ),
            )
            .content(move |_, _, _| {
                let this = this.clone();
                let id = id.clone();
                v_flex()
                    .id("theme-element-shape-grid")
                    .max_h(px(420.))
                    .overflow_y_scroll()
                    .w(px(316.))
                    .children(groups.iter().map(|(label, tiles)| {
                        v_flex()
                            .child(
                                div()
                                    .px_1()
                                    .pb_1()
                                    .pt_2()
                                    .text_size(text::LABEL.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(k.text(TextStep::T500))
                                    .child(*label),
                            )
                            .child(h_flex().flex_wrap().gap_0p5().children(tiles.iter().map(
                                |(tile, image)| {
                                    let (this, id) = (this.clone(), id.clone());
                                    let shape = tile.pick.shape.clone();
                                    div()
                                        .id(SharedString::from(format!(
                                            "theme-element-shape-{}",
                                            tile.key
                                        )))
                                        .test_support()
                                        .role(Role::Button)
                                        .aria_label(tile.label.clone())
                                        .size_8()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(radius::md())
                                        .cursor_pointer()
                                        .hover(|s| s.bg(k.surface_menu_hover))
                                        .on_click(move |_, _, cx| {
                                            let Some(shape) = shape.clone() else {
                                                return;
                                            };
                                            let id = id.clone();
                                            this.update(cx, |t, cx| {
                                                t.shapes_open = false;
                                                t.edit_element(
                                                    "element.shape",
                                                    &id,
                                                    move |e| {
                                                        panels::set_shape(e, &shape);
                                                    },
                                                    cx,
                                                )
                                            })
                                        })
                                        .children(
                                            image.clone().map(|i| img(i).w(px(18.)).h(px(18.))),
                                        )
                                },
                            )))
                    }))
            })
            .into_any_element()
    }
}

fn element_edit(t: &OverlayTheme, id: &str, patch: impl Fn(&mut OverlayElement)) -> OverlayTheme {
    let mut n = t.clone();
    n.elements = update_overlay_element(&t.elements, id, patch);
    n
}

impl Render for ThemeInspector {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let theme_id = editor.selected_theme().map(|t| t.id.clone());
        let layer = editor.selected_layer().map(str::to_owned);
        let theme = editor.draft().clone();
        let kind = editor.kind();
        if self.scope != (theme_id.clone(), layer.clone()) {
            self.scope = (theme_id.clone(), layer.clone());
            self.clear_fields();
        }
        let root = v_flex().id(ids::ROOT).test_support().size_full().min_h_0();
        if theme_id.is_none() {
            return root.into_any_element();
        }
        let element = layer
            .as_deref()
            .and_then(|id| theme.elements.iter().find(|e| e.id == id))
            .cloned();
        let body = match &element {
            Some(el) => self.render_element(el, window, cx),
            None => {
                let text_layer = panels::text_layer(layer.as_deref());
                match self.tab {
                    Tab::Background => self.render_background(&theme, window, cx),
                    Tab::Text => self.render_text(&theme, kind, text_layer, window, cx),
                    Tab::Layout => self.render_layout(&theme, kind, text_layer, window, cx),
                }
            }
        };
        root.when(element.is_none(), |r| r.child(self.render_tabs(cx)))
            .child(
                div()
                    .id("theme-inspector-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .child(body),
            )
            .into_any_element()
    }
}

// ─── Small pieces ──────────────────────────────────────────────────────────

fn relative_half() -> gpui_kit::DefiniteLength {
    gpui_kit::relative(0.48)
}

fn relative_third() -> gpui_kit::DefiniteLength {
    gpui_kit::relative(0.31)
}

fn choice(value: &str, label: &str) -> Choice {
    Choice {
        value: value.to_owned(),
        label: label.to_owned(),
    }
}

fn field_label(label: &str, cx: &App) -> AnyElement {
    div()
        .text_size(text::LABEL.size)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.kairo().text(TextStep::T500))
        .child(label.to_owned())
        .into_any_element()
}

/// A label over its control (`FieldLabel`).
fn labelled(label: &str, control: impl IntoElement, cx: &App) -> AnyElement {
    v_flex()
        .min_w_0()
        .flex_1()
        .gap_1()
        .child(field_label(label, cx))
        .child(control)
        .into_any_element()
}

/// A label on the left and its control on the right (`Row`).
fn row(label: &str, control: impl IntoElement, cx: &App) -> AnyElement {
    h_flex()
        .gap_2()
        .child(
            div()
                .flex_none()
                .w(px(88.))
                .truncate()
                .text_size(text::XS.size)
                .text_color(cx.kairo().text(TextStep::T400))
                .child(label.to_owned()),
        )
        .child(div().flex_1().min_w_0().child(control))
        .into_any_element()
}

/// One inspector section, split from the next by a full-width line.
fn section(key: &str, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    v_flex()
        .id(SharedString::from(ids::section(key)))
        .gap_2p5()
        .py_3p5()
        .border_t_1()
        .border_color(cx.kairo().surface_line)
}

fn grid2(a: AnyElement, b: AnyElement) -> AnyElement {
    h_flex()
        .gap_2()
        .child(div().flex_1().min_w_0().child(a))
        .child(div().flex_1().min_w_0().child(b))
        .into_any_element()
}

fn grid3(a: AnyElement, b: AnyElement, c: AnyElement) -> AnyElement {
    h_flex()
        .gap_2()
        .child(div().flex_1().min_w_0().child(a))
        .child(div().flex_1().min_w_0().child(b))
        .child(div().flex_1().min_w_0().child(c))
        .into_any_element()
}

/// `CompactSelect`: a dropdown showing the current choice; a value not in
/// the list shows as written.
impl ThemeInspector {
    fn font_picker(&self, layer: &'static str, value: &str, cx: &mut Context<Self>) -> AnyElement {
        let options = panels::font_options(value);
        let current = options
            .iter()
            .find(|o| o.value == value)
            .map(|o| o.label.clone())
            .unwrap_or_else(|| value.to_owned());
        let custom = options
            .iter()
            .find(|o| !kairo_scene::FONT_STACKS.iter().any(|(_, s)| *s == o.value))
            .cloned();
        let value = value.to_owned();
        let open = self.font_open;
        let entity = cx.entity();
        let editor = self.editor.clone();
        let on_open_entity = entity.clone();
        let on_open_editor = editor.clone();
        let trigger_face = panels::stack_label(&value);
        Popover::new("theme-font-popover")
            .anchor(gpui_kit::Anchor::TopLeft)
            .offset(px(4.))
            .appearance(false)
            .open(open)
            .on_open_change(move |next, _, cx| {
                let next = *next;
                on_open_entity.update(cx, |this, cx| {
                    this.font_open = next;
                    cx.notify();
                });
                if !next {
                    on_open_editor.update(cx, |e, cx| e.preview_font(None, cx));
                }
            })
            .trigger(
                Button::new(ids::FONT)
                    .cursor_pointer()
                    .small()
                    .h_8()
                    .w_full()
                    .font_family(trigger_face)
                    .label(current)
                    .dropdown_caret(true)
                    .accessibility_label("Font")
                    .popover_trigger(),
            )
            .content(move |_, _, cx| {
                let k = *cx.kairo();
                let mut rows: Vec<AnyElement> = Vec::new();
                let make_item = |id: String, name: String, stack: String| -> AnyElement {
                    let same = stack == value;
                    let face = panels::stack_label(&stack);
                    let hover_editor = editor.clone();
                    let hover_stack = stack.clone();
                    let click_entity = entity.clone();
                    let click_editor = editor.clone();
                    h_flex()
                        .id(SharedString::from(id))
                        .w_full()
                        .items_center()
                        .gap_2()
                        .px_2p5()
                        .py_1p5()
                        .rounded(radius::md())
                        .cursor_pointer()
                        .when(same, |d| {
                            d.bg(k.surface_tertiary).text_color(k.text_primary)
                        })
                        .when(!same, |d| {
                            d.text_color(k.text(TextStep::T200))
                                .hover(|s| s.bg(k.white.opacity(0.07)).text_color(k.text_primary))
                        })
                        .on_hover(move |hovered, _, cx| {
                            if *hovered {
                                let s = hover_stack.clone();
                                hover_editor
                                    .update(cx, |e, cx| e.preview_font(Some((layer, s)), cx));
                            }
                        })
                        .on_click(move |_, _, cx| {
                            click_editor.update(cx, |e, cx| e.preview_font(None, cx));
                            click_entity.update(cx, |this, cx| {
                                this.font_open = false;
                                if !same {
                                    let next = stack.clone();
                                    this.edit(
                                        "text.font",
                                        move |t| {
                                            panels::edit_layer(t, layer, |s| {
                                                s.font_family = next.clone()
                                            })
                                        },
                                        cx,
                                    );
                                } else {
                                    cx.notify();
                                }
                            });
                        })
                        .child(
                            div()
                                .w_3p5()
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(same, |d| {
                                    d.child(
                                        Icon::new(KairoIcon::Check)
                                            .size_3p5()
                                            .text_color(k.accent(kairo_ui_kit::tokens::Step::S400)),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_family(face)
                                .text_size(text::SM.size)
                                .child(name),
                        )
                        .into_any_element()
                };
                for (gi, (group, range)) in kairo_scene::FONT_GROUPS.iter().enumerate() {
                    if gi > 0 {
                        rows.push(
                            div()
                                .my_1()
                                .h(px(1.))
                                .w_full()
                                .bg(k.panel_border)
                                .into_any_element(),
                        );
                    }
                    rows.push(
                        div()
                            .px_2p5()
                            .py_1()
                            .text_size(text::MICRO.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T500))
                            .child(*group)
                            .into_any_element(),
                    );
                    for (fi, (name, stack)) in
                        kairo_scene::FONT_STACKS[range.clone()].iter().enumerate()
                    {
                        rows.push(make_item(
                            format!("theme-font-{gi}-{fi}"),
                            (*name).to_owned(),
                            (*stack).to_owned(),
                        ));
                    }
                }
                if let Some(extra) = &custom {
                    rows.push(
                        div()
                            .my_1()
                            .h(px(1.))
                            .w_full()
                            .bg(k.panel_border)
                            .into_any_element(),
                    );
                    rows.push(
                        div()
                            .px_2p5()
                            .py_1()
                            .text_size(text::MICRO.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(k.text(TextStep::T500))
                            .child("Custom")
                            .into_any_element(),
                    );
                    rows.push(make_item(
                        "theme-font-custom".into(),
                        extra.label.clone(),
                        extra.value.clone(),
                    ));
                }
                v_flex()
                    .id("theme-font-list")
                    .w(px(236.))
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .p_1()
                    .rounded(radius::lg())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.white.opacity(0.14))
                    .shadow_2xl()
                    .children(rows)
            })
            .into_any_element()
    }
}

fn select(
    id: impl Into<SharedString>,
    label: &str,
    value: &str,
    options: Vec<Choice>,
    change: Change,
) -> AnyElement {
    let id: SharedString = id.into();
    let current = options
        .iter()
        .find(|o| o.value == value)
        .map(|o| o.label.clone())
        .unwrap_or_else(|| value.to_owned());
    let value = value.to_owned();
    Button::new(id)
        .cursor_pointer()
        .small()
        .h_8()
        .w_full()
        .label(current)
        .dropdown_caret(true)
        .accessibility_label(label.to_owned())
        .popover_trigger()
        .dropdown_menu(move |mut menu, _, _| {
            for option in &options {
                let change = change.clone();
                let next = option.value.clone();
                let same = option.value == value;
                menu = menu.item(
                    PopupMenuItem::new(option.label.clone())
                        .checked(same)
                        .on_click(move |_, w, cx| {
                            if !same {
                                change(next.clone(), w, cx)
                            }
                        }),
                );
            }
            menu
        })
        .into_any_element()
}

/// `IconToggleGroup`.
fn icon_group(
    key: &str,
    label: &str,
    value: &str,
    options: &[(&'static str, &'static str, KairoIcon)],
    change: Change,
    cx: &App,
) -> AnyElement {
    let k = *cx.kairo();
    h_flex()
        .id(SharedString::from(ids::align(key)))
        .test_support()
        .role(Role::Group)
        .aria_label(label.to_owned())
        .h_7()
        .gap_0p5()
        .p_0p5()
        .rounded(radius::md())
        .bg(k.surface_tertiary)
        .children(options.iter().map(|(v, name, icon)| {
            let active = *v == value;
            let change = change.clone();
            let next = (*v).to_owned();
            div().flex_1().child(
                Button::new(SharedString::from(ids::align(&format!("{key}-{v}"))))
                    .cursor_pointer()
                    .xsmall()
                    .w_full()
                    .icon(Icon::new(*icon))
                    .when(active, |b| b.primary())
                    .when(!active, |b| b.ghost())
                    .toggled(active)
                    .accessibility_label(*name)
                    .tooltip(*name)
                    .on_click(move |_, w, cx| change(next.clone(), w, cx)),
            )
        }))
        .into_any_element()
}

/// `MediaPicker`: the chosen file, small, with its name and a way to change it.
fn media_picker(
    kind: MediaKind,
    path: &str,
    pick: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let k = *cx.kairo();
    let video = kind == MediaKind::Video;
    let thumb = div()
        .flex_none()
        .w(px(80.))
        .h(px(45.))
        .rounded(radius::md())
        .overflow_hidden()
        .bg(gpui_kit::black())
        .flex()
        .items_center()
        .justify_center();
    let thumb = if path.is_empty() {
        thumb.child(
            Icon::new(KairoIcon::Plus)
                .size_3p5()
                .text_color(k.text(TextStep::T500)),
        )
    } else if video {
        thumb.child(
            Icon::new(KairoIcon::Video)
                .size_4()
                .text_color(k.text(TextStep::T400)),
        )
    } else {
        thumb.child(
            img(std::path::PathBuf::from(path))
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Cover),
        )
    };
    let name = if path.is_empty() {
        format!("Choose {}", if video { "a video" } else { "an image" })
    } else {
        panels::file_name(path).to_owned()
    };
    labelled(
        if video { "Video" } else { "Image" },
        h_flex()
            .id(ids::MEDIA)
            .test_support()
            .role(Role::Button)
            .aria_label(name.clone())
            .w_full()
            .gap_3()
            .p_1p5()
            .rounded(radius::lg())
            .bg(k.surface)
            .cursor_pointer()
            .hover(|s| s.bg(k.surface_tertiary))
            .on_click(move |_, w, cx| pick(w, cx))
            .child(thumb)
            .child(
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(div().truncate().text_size(text::XS.size).child(name))
                    .when(!path.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(text::LABEL.size)
                                .text_color(k.text(TextStep::T500))
                                .child("Change…"),
                        )
                    }),
            ),
        cx,
    )
}
