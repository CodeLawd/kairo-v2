//! The editor's screen: the library and layers on the left, the slide with
//! its canvas in the middle, and the inspector on the right.

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverButtonTriggerExt;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Bounds, Context, CursorStyle, DragMoveEvent, Focusable, FontWeight,
    Hsla, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, PathBuilder, Pixels, Point, Render, RenderImage, Role,
    SharedString, StatefulInteractiveElement, Styled, Window, black, canvas, div, hsla, img,
    linear_color_stop, linear_gradient, point, prelude::FluentBuilder, px, relative, white,
};
use kairo_domain::overlay::{
    CustomOverlayTheme, OverlayBackgroundType, OverlayContentKind, OverlayElement,
    OverlayElementKind, OverlayTextAlign, OverlayTheme, OverlayVerticalAlign,
};
use kairo_domain::overlay_boxes::ResizeHandle;
use kairo_domain::overlay_elements::{MAX_OVERLAY_ELEMENTS, overlay_element_label};
use kairo_domain::overlay_outputs::{content_kind_label, is_rendered_kind};
use kairo_domain::theme_library::{built_in_themes_for_kind, output_uses_theme, themes_for_kind};
use kairo_scene::color::css_with_opacity;
use kairo_ui_kit::components::{SegmentOption, SegmentedControl};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::MediaKind;
use crate::canvas::{self, CanvasLayer, Hit, PixelBox, is_deletable};
use crate::editor::{CanvasDrag, ThemeEditor};
use crate::gallery::{self, ShapePick, Tile};
use crate::model;
use crate::pen::PenDraft;
use gpui_kit::StyledImage;

pub mod ids {
    pub const ROOT: &str = "theme-editor";
    pub const KIND: &str = "theme-kind";
    pub const NEW: &str = "theme-new";
    pub const NAME: &str = "theme-name";
    pub const UNDO: &str = "theme-undo";
    pub const REDO: &str = "theme-redo";
    pub const SAMPLE: &str = "theme-sample";
    pub const STAGE: &str = "theme-stage";
    pub const FRAME: &str = "theme-frame";
    pub const CANVAS: &str = "theme-canvas";
    pub const TOOLBAR: &str = "theme-add-toolbar";
    pub const ADD_SHAPE: &str = "theme-add-shape";
    pub const ADD_IMAGE: &str = "theme-add-image";
    pub const ADD_VIDEO: &str = "theme-add-video";
    /// The Pen: draw a path of your own.
    pub const ADD_PEN: &str = "theme-add-pen";
    /// The Pen's instructions over the canvas.
    pub const PEN_HINT: &str = "theme-pen-hint";
    /// A text box of its own.
    pub const ADD_TEXT: &str = "theme-add-text";
    /// Coming soon: stickers.
    pub const ADD_STICKER: &str = "theme-add-sticker";
    pub const SHAPES: &str = "theme-shapes";
    pub const DELETE: &str = "theme-layer-delete";
    pub const LABEL: &str = "theme-layer-label";
    pub const GUIDE_V: &str = "theme-guide-vertical";
    pub const GUIDE_H: &str = "theme-guide-horizontal";
    pub const STATUS: &str = "theme-status";
    pub const READOUT: &str = "theme-readout";
    pub const SAVED: &str = "theme-saved";
    pub const NOTICE: &str = "theme-notice";
    pub const RESET: &str = "theme-reset";
    pub const SAVE: &str = "theme-save";
    pub const EMPTY: &str = "theme-empty";
    pub const EMPTY_BLANK: &str = "theme-empty-blank";
    pub const NO_PREVIEW: &str = "theme-no-preview";
    pub const ASIDE: &str = "theme-aside";
    pub fn tile(id: &str) -> String {
        format!("theme-tile-{id}")
    }
    pub fn tile_menu(id: &str) -> String {
        format!("theme-tile-menu-{id}")
    }
    pub fn layer(id: &str) -> String {
        format!("theme-layer-{id}")
    }
    pub fn shape(key: &str) -> String {
        format!("theme-shape-{key}")
    }
    pub fn screen(id: &str) -> String {
        format!("theme-screen-{id}")
    }
    pub fn template(id: &str) -> String {
        format!("theme-template-{id}")
    }
}

/// A gallery group with each tile's picture.
type ShapeGroup = (&'static str, Vec<(Tile, Option<Arc<RenderImage>>)>);

const FRAME_PAD: (f32, f32, f32, f32) = (24.0, 64.0, 24.0, 24.0);
const ASIDE_PX: f32 = 300.0;

fn kind_key(kind: OverlayContentKind) -> &'static str {
    match kind {
        OverlayContentKind::Scripture => "scripture",
        OverlayContentKind::Lyrics => "lyrics",
    }
}

fn kind_from_key(key: &str) -> OverlayContentKind {
    if key == "lyrics" {
        OverlayContentKind::Lyrics
    } else {
        OverlayContentKind::Scripture
    }
}

/// A theme's CSS colour as GPUI paints it.
fn css(color: &str, opacity: f64) -> Option<Hsla> {
    css_with_opacity(color, opacity).map(|c| {
        gpui_kit::Rgba {
            r: f32::from(c.r) / 255.0,
            g: f32::from(c.g) / 255.0,
            b: f32::from(c.b) / 255.0,
            a: f32::from(c.a) / 255.0,
        }
        .into()
    })
}

fn rel(frame: Point<Pixels>, p: (f64, f64)) -> Point<Pixels> {
    point(frame.x + px(p.0 as f32), frame.y + px(p.1 as f32))
}

fn polygon(
    origin: Point<Pixels>,
    points: &[(f64, f64)],
    stroke: Option<f32>,
) -> Option<gpui_kit::Path<Pixels>> {
    let mut builder = match stroke {
        Some(w) => PathBuilder::stroke(px(w)),
        None => PathBuilder::fill(),
    };
    let (first, rest) = points.split_first()?;
    builder.move_to(rel(origin, *first));
    for p in rest {
        builder.line_to(rel(origin, *p));
    }
    builder.close();
    builder.build().ok()
}

/// What the canvas draws over the slide, taken before painting.
#[derive(Clone)]
struct Overlay {
    selected: Option<(PixelBox, bool)>,
    hover: Option<PixelBox>,
    pen: Option<PenDraft>,
}

/// The Pen's path so far, its points, and the line to the pointer.
fn paint_pen(origin: Point<Pixels>, pen: &PenDraft, window: &mut Window) {
    let ink = white();
    let accent = hsla(0.0, 0.8, 0.6, 1.0);
    let at = |p: (f64, f64)| rel(origin, p);
    if pen.points.len() >= 2 {
        let mut b = PathBuilder::stroke(px(2.0));
        b.move_to(at(pen.points[0].at));
        for pair in pen.points.windows(2) {
            let (a, c) = (pair[0], pair[1]);
            if a.out.is_none() && c.out.is_none() {
                b.line_to(at(c.at));
            } else {
                b.cubic_bezier_to(
                    at(c.at),
                    at(a.out.unwrap_or(a.at)),
                    at(c.inward().unwrap_or(c.at)),
                );
            }
        }
        if let Ok(path) = b.build() {
            window.paint_path(path, ink);
        }
    }
    // Where the next click lands, while not dragging out a curve.
    if let (Some(last), Some(cursor), false) = (pen.points.last(), pen.cursor, pen.dragging)
        && let Some(path) = polygon(origin, &[last.at, cursor, last.at], Some(1.5))
    {
        window.paint_path(path, ink.opacity(0.55));
    }
    // The newest point's curve handles.
    if let Some(last) = pen.points.last()
        && let (Some(out), Some(inward)) = (last.out, last.inward())
        && let Some(path) = polygon(origin, &[inward, out, inward], Some(1.0))
    {
        window.paint_path(path, ink.opacity(0.7));
    }
    let closing = pen.cursor.is_some_and(|c| pen.closes_at(c));
    for (i, p) in pen.points.iter().enumerate() {
        let r = if i == 0 && closing { 7.0 } else { 5.0 };
        let ring: Vec<(f64, f64)> = (0..24)
            .map(|n| {
                let a = f64::from(n) / 24.0 * std::f64::consts::TAU;
                (p.at.0 + r * a.cos(), p.at.1 + r * a.sin())
            })
            .collect();
        if let Some(path) = polygon(origin, &ring, None) {
            window.paint_path(path, ink);
        }
        if let Some(path) = polygon(origin, &ring, Some(1.5)) {
            window.paint_path(path, accent);
        }
    }
}

fn paint_overlay(origin: Point<Pixels>, overlay: &Overlay, window: &mut Window) {
    if let Some(pen) = &overlay.pen {
        paint_pen(origin, pen, window);
        return;
    }
    if let Some(b) = overlay.hover
        && let Some(path) = polygon(origin, &b.corners(), Some(1.0))
    {
        window.paint_path(path, white().opacity(0.5));
    }
    let Some((b, rotatable)) = overlay.selected else {
        return;
    };
    if let Some(path) = polygon(origin, &b.corners(), Some(2.0)) {
        window.paint_path(path, white().opacity(0.9));
    }
    let half = canvas::HANDLE_PX / 2.0;
    for handle in ResizeHandle::ALL {
        let c = b.handle_local(handle);
        let square = [
            (c.0 - half, c.1 - half),
            (c.0 + half, c.1 - half),
            (c.0 + half, c.1 + half),
            (c.0 - half, c.1 + half),
        ]
        .map(|p| b.to_frame(p));
        if let Some(path) = polygon(origin, &square, None) {
            window.paint_path(path, white());
        }
    }
    if rotatable {
        let from = b.to_frame((0.0, b.h / 2.0));
        let to = b.to_frame((0.0, b.h / 2.0 + 20.0));
        if let Some(path) = polygon(origin, &[from, to, from], Some(1.0)) {
            window.paint_path(path, white().opacity(0.7));
        }
        let c = b.rotate_knob_local();
        let r = canvas::ROTATE_KNOB_PX / 2.0;
        let ring: Vec<(f64, f64)> = (0..24)
            .map(|i| {
                let a = f64::from(i) / 24.0 * std::f64::consts::TAU;
                b.to_frame((c.0 + r * a.cos(), c.1 + r * a.sin()))
            })
            .collect();
        if let Some(path) = polygon(origin, &ring, None) {
            window.paint_path(path, black().opacity(0.6));
        }
        if let Some(path) = polygon(origin, &ring, Some(2.0)) {
            window.paint_path(path, white());
        }
    }
}

fn cursor_for(hit: Option<&Hit>, dragging: bool) -> CursorStyle {
    match hit {
        Some(Hit::Resize(_, h)) => match h {
            ResizeHandle::N | ResizeHandle::S => CursorStyle::ResizeUpDown,
            ResizeHandle::E | ResizeHandle::W => CursorStyle::ResizeLeftRight,
            ResizeHandle::Ne | ResizeHandle::Sw => CursorStyle::ResizeUpRightDownLeft,
            ResizeHandle::Nw | ResizeHandle::Se => CursorStyle::ResizeUpLeftDownRight,
        },
        Some(Hit::Rotate(_)) | Some(Hit::Body(_)) => {
            if dragging {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            }
        }
        Some(Hit::Delete(_)) => CursorStyle::PointingHand,
        None => CursorStyle::Arrow,
    }
}

impl ThemeEditor {
    /// The shape thumbnail for `tile` in `color`, drawn once per colour.
    fn thumb(&self, tile: &Tile, color: Hsla) -> Option<Arc<RenderImage>> {
        cached_thumb(&self.thumbs, tile, color)
    }
}

/// The shape thumbnail for `tile` in `color`, drawn once per colour.
pub(crate) fn cached_thumb(
    thumbs: &std::cell::RefCell<crate::editor::ThumbCache>,
    tile: &Tile,
    color: Hsla,
) -> Option<Arc<RenderImage>> {
    {
        let key = (
            format!("{}:{:?}:{}", tile.key, tile.pick.kind, tile.pick.radius_px),
            (color.l * 255.0) as u32 | (((color.a * 255.0) as u32) << 8),
        );
        thumbs
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| gallery::thumb(tile, color))
            .clone()
    }
}

impl ThemeEditor {
    pub(crate) fn element_tile(el: &OverlayElement) -> Tile {
        Tile {
            key: el
                .shape
                .clone()
                .unwrap_or_else(|| el.kind.as_str().to_owned()),
            label: overlay_element_label(el.kind, el.shape.as_deref()).to_owned(),
            line_only: kairo_domain::overlay_elements::is_line_element(
                el.kind,
                el.shape.as_deref(),
            ),
            pick: ShapePick {
                kind: el.kind,
                shape: el.shape.clone(),
                radius_px: 0.0,
            },
        }
    }
}

impl Render for ThemeEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_preview(window, cx);
        let k = *cx.kairo();
        h_flex()
            .id(ids::ROOT)
            .test_support()
            .size_full()
            .min_h_0()
            .items_stretch()
            .overflow_hidden()
            .bg(k.surface)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let m = e.keystroke.modifiers;
                if m.platform || m.control {
                    let typing = this.name_input.focus_handle(cx).is_focused(window);
                    if !typing && !m.alt && this.history_key(&e.keystroke.key, m.shift, cx) {
                        cx.stop_propagation();
                    }
                }
            }))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        h_flex()
                            .flex_1()
                            .w_full()
                            .min_h_0()
                            .items_stretch()
                            .child(self.render_library(cx))
                            .child(kairo_ui_kit::components::page_sidebar::handle(
                                "theme-sidebar-resize",
                                None,
                                cx,
                            ))
                            .child(self.render_main(window, cx)),
                    )
                    .children(self.media_dock.clone()),
            )
            .child(self.render_aside(cx))
    }
}

// ─── The library ───────────────────────────────────────────────────────────

impl ThemeEditor {
    fn render_library(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let kind = self.kind;
        let themes: Vec<CustomOverlayTheme> = themes_for_kind(&self.settings.theme_library, kind)
            .into_iter()
            .cloned()
            .collect();
        let defaults = match kind {
            OverlayContentKind::Scripture => self.settings.theme_defaults.scripture.clone(),
            OverlayContentKind::Lyrics => self.settings.theme_defaults.lyrics.clone(),
        };
        let tiles: Vec<AnyElement> = themes
            .iter()
            .enumerate()
            .map(|(i, t)| {
                self.render_tile(i + 1, t, defaults.as_deref() == Some(t.id.as_str()), cx)
            })
            .collect();
        let kinds = SegmentedControl::new(ids::KIND, "Theme kind")
            .options(
                Self::kinds()
                    .into_iter()
                    .map(|(k, label)| SegmentOption::new(kind_key(k), label)),
            )
            .selected(kind_key(kind))
            .on_change({
                let editor = editor.clone();
                move |key, window, cx| {
                    let next = kind_from_key(key);
                    editor.update(cx, |this, cx| this.switch_kind(next, window, cx));
                }
            });
        let new_menu = {
            let editor = editor.clone();
            let templates: Vec<(String, String, OverlayTheme)> = built_in_themes_for_kind(kind)
                .into_iter()
                .map(|t| (t.id.to_owned(), t.name.to_owned(), t.theme.clone()))
                .collect();
            Button::new(ids::NEW)
                .cursor_pointer()
                .ghost()
                .xsmall()
                .icon(Icon::new(KairoIcon::Plus))
                .accessibility_label("New theme")
                .tooltip("New theme")
                .popover_trigger()
                .dropdown_menu(move |mut menu, _, _| {
                    let blank = editor.clone();
                    menu = menu.item(PopupMenuItem::new("Blank").on_click(move |_, window, cx| {
                        blank.update(cx, |this, cx| this.create_blank(window, cx))
                    }));
                    if !templates.is_empty() {
                        menu = menu.separator();
                    }
                    for (_, name, theme) in &templates {
                        let (editor, name, theme) = (editor.clone(), name.clone(), theme.clone());
                        menu = menu.item(PopupMenuItem::new(name.clone()).on_click(
                            move |_, window, cx| {
                                editor.update(cx, |this, cx| this.create(&theme, &name, window, cx))
                            },
                        ));
                    }
                    menu
                })
        };
        let sidebar_width = kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx);
        v_flex()
            .flex_none()
            .w(px(sidebar_width))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.surface_line)
            .child(div().flex_none().p_2p5().pb_1p5().child(kinds))
            .child(heading("Themes", Some(new_menu.into_any_element()), cx))
            .child(
                v_flex()
                    .id("theme-tiles")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .px_1p5()
                    .pb_2()
                    .children(tiles),
            )
            .children(self.selected_id.is_some().then(|| {
                v_flex()
                    .flex_none()
                    .max_h(relative(0.38))
                    .border_t_1()
                    .border_color(k.surface_line)
                    .child(heading("Layers", None, cx))
                    .child(self.render_layers(cx))
            }))
            .into_any_element()
    }

    fn render_tile(
        &self,
        index: usize,
        item: &CustomOverlayTheme,
        is_default: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let selected = self.selected_id.as_deref() == Some(item.id.as_str());
        let editor = cx.entity();
        let id = item.id.clone();
        // Only the selected tile follows the draft, so the rest never
        // redraw while editing.
        let (name, theme) = if selected {
            (
                if self.name.is_empty() {
                    "Untitled theme".to_owned()
                } else {
                    self.name.clone()
                },
                self.draft.clone(),
            )
        } else {
            (item.name.clone(), item.theme.clone())
        };
        let menu = {
            let (editor, id) = (editor.clone(), id.clone());
            move |menu: gpui_kit::component::menu::PopupMenu, default: bool| {
                let (e1, i1) = (editor.clone(), id.clone());
                let (e2, i2) = (editor.clone(), id.clone());
                let (e3, i3) = (editor.clone(), id.clone());
                menu.item(
                    PopupMenuItem::new(if default {
                        "Default theme"
                    } else {
                        "Make default"
                    })
                    .disabled(default)
                    .on_click(move |_, window, cx| {
                        e1.update(cx, |this, cx| {
                            this.select_theme(&i1, window, cx);
                            this.make_default(&i1, cx);
                        })
                    }),
                )
                .item(
                    PopupMenuItem::new("Duplicate").on_click(move |_, window, cx| {
                        e2.update(cx, |this, cx| this.duplicate(&i2, window, cx))
                    }),
                )
                .separator()
                .item(PopupMenuItem::new("Delete").on_click(move |_, window, cx| {
                    e3.update(cx, |this, cx| this.remove(&i3, window, cx))
                }))
            }
        };
        let group = SharedString::from(format!("tile-{}", item.id));
        let context = menu.clone();
        let actions = menu;
        let select = {
            let (editor, id) = (editor.clone(), id.clone());
            move |_: &gpui_kit::ClickEvent, window: &mut Window, cx: &mut gpui_kit::App| {
                editor.update(cx, |this, cx| this.select_theme(&id, window, cx))
            }
        };
        div()
            .id(SharedString::from(ids::tile(&item.id)))
            .test_support()
            .role(Role::Button)
            .aria_label(if name.trim().is_empty() {
                "Untitled theme".into()
            } else {
                name.clone()
            })
            .aria_selected(selected)
            .group(group.clone())
            .relative()
            .rounded(radius::md())
            .p_1p5()
            .when(selected, |d| d.bg(k.row_selected()))
            .when(!selected, |d| d.hover(|s| s.bg(k.surface_tertiary)))
            .cursor_pointer()
            .on_click(select)
            .context_menu(move |m, _, _| context(m, is_default))
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .rounded(radius::sm())
                    .border_1()
                    .border_color(if selected {
                        k.accent(Step::S500)
                    } else {
                        k.surface_line
                    })
                    .child(slide_picture(
                        &theme,
                        self.kind,
                        kairo_ui_kit::components::page_sidebar::PageSidebar::width(None, cx)
                            - 12.0
                            - 2.0,
                        &k,
                        &self.thumbs,
                    )),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .px_0p5()
                    .pt_1()
                    .text_size(text::XS.size)
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(if selected {
                                k.text_primary
                            } else {
                                k.text(TextStep::T300)
                            })
                            .child(format!("{index}. {name}")),
                    )
                    .children(is_default.then(|| {
                        div()
                            .flex_none()
                            .rounded(radius::sm())
                            .bg(k.surface_tertiary)
                            .px_1p5()
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T300))
                            .child("Default")
                    })),
            )
            .child(
                div()
                    .absolute()
                    .top_2p5()
                    .right_2p5()
                    .opacity(0.0)
                    .group_hover(group, |s| s.opacity(1.0))
                    .child(
                        Button::new(SharedString::from(ids::tile_menu(&item.id)))
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::MoreHorizontal))
                            .accessibility_label(format!("{name} actions"))
                            .popover_trigger()
                            .dropdown_menu(move |m, _, _| actions(m, is_default)),
                    ),
            )
            .into_any_element()
    }
}

/// The theme's slide, small: its background, elements and sample words
/// at the tile's size. The slide in the middle is drawn by the engine;
/// these only help tell themes apart.
pub(crate) fn slide_picture(
    theme: &OverlayTheme,
    kind: OverlayContentKind,
    width: f32,
    k: &kairo_ui_kit::tokens::KairoColors,
    thumbs: &std::cell::RefCell<crate::editor::ThumbCache>,
) -> AnyElement {
    let height = width * 9.0 / 16.0;
    let scale = width / 1920.0;
    let bg = &theme.background;
    let sample = model::sample(kind, false);
    let mut slide = div()
        .relative()
        .w(px(width))
        .h(px(height))
        .overflow_hidden()
        .bg(k.surface_stage);
    slide = match bg.kind {
        OverlayBackgroundType::Color => match css(&bg.color, bg.opacity) {
            Some(c) => slide.bg(c),
            None => slide,
        },
        OverlayBackgroundType::Gradient => {
            let from = css(&bg.color, bg.opacity);
            let to = css(bg.color2.as_deref().unwrap_or(&bg.color), bg.opacity);
            match (from, to) {
                (Some(a), Some(b)) => slide.bg(linear_gradient(
                    bg.angle_deg.unwrap_or(0.0) as f32,
                    linear_color_stop(a, 0.0),
                    linear_color_stop(b, 1.0),
                )),
                _ => slide,
            }
        }
        OverlayBackgroundType::Image | OverlayBackgroundType::Video => slide.bg(k.surface_tertiary),
        _ => slide,
    };
    // A still image shows itself, as on the canvas; a video keeps the fill.
    if bg.kind == OverlayBackgroundType::Image
        && let Some(path) = bg.media_path.as_deref().filter(|p| !p.is_empty())
    {
        slide = slide.child(
            gpui_kit::img(std::path::PathBuf::from(path))
                .absolute()
                .size_full()
                .object_fit(gpui_kit::ObjectFit::Cover)
                .opacity(bg.opacity.clamp(0.0, 1.0) as f32),
        );
    }
    let place = |frame: &kairo_domain::overlay::OverlayBox| {
        div()
            .absolute()
            .left(relative((frame.x_pct / 100.0) as f32))
            .top(relative((frame.y_pct / 100.0) as f32))
            .w(relative((frame.width_pct / 100.0) as f32))
            .h(relative((frame.height_pct / 100.0) as f32))
    };
    let element = |el: &OverlayElement| -> AnyElement {
        let fill = css(&el.fill, el.fill_opacity * el.opacity);
        let base = place(&el.frame);
        match el.kind {
            OverlayElementKind::Rectangle | OverlayElementKind::Ellipse => base
                .when_some(fill, |d, c| d.bg(c))
                .when(el.kind == OverlayElementKind::Ellipse, |d| d.rounded_full())
                .when(el.kind == OverlayElementKind::Rectangle, |d| {
                    d.rounded(px((el.radius_px * f64::from(scale)) as f32))
                })
                .into_any_element(),
            OverlayElementKind::Shape => {
                let tile = ThemeEditor::element_tile(el);
                let image = cached_thumb(
                    thumbs,
                    &tile,
                    fill.unwrap_or_else(|| hsla(0.0, 0.0, 1.0, 1.0)),
                );
                base.children(image.map(|i| img(i).size_full()))
                    .into_any_element()
            }
            OverlayElementKind::Image | OverlayElementKind::Video => {
                base.bg(k.surface_elevated).opacity(0.8).into_any_element()
            }
            OverlayElementKind::Text => base
                .flex()
                .items_center()
                .justify_center()
                .text_center()
                .when_some(fill, |d, c| d.text_color(c))
                .text_size(px(
                    (el.font_size_px.unwrap_or(72.0) * f64::from(scale)) as f32
                ))
                .font_weight(FontWeight(el.font_weight.unwrap_or(600.0) as f32))
                .child(el.text.clone().unwrap_or_default())
                .into_any_element(),
            // The engine's preview draws pen paths; this sketch leaves them out.
            OverlayElementKind::Path => base.into_any_element(),
        }
    };
    for el in theme.elements.iter().filter(|e| !e.above_text) {
        slide = slide.child(element(el));
    }
    // The sizes the output draws at (auto-fit included), from the engine's
    // own layout, so "fit to box" shows here as on the canvas.
    let (verse_px, reference_px) = fitted_sizes(theme, &sample.reference, &sample.text);
    let text_box =
        |style: &kairo_domain::overlay::OverlayTextStyle, words: String, fitted: Option<f32>| {
            let color = css(&style.color, style.color_opacity).unwrap_or(k.text_primary);
            let family = style
                .font_family
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['\'', '"'])
                .to_owned();
            let size =
                (fitted.map_or(style.font_size_px, f64::from) * f64::from(scale)).max(2.0) as f32;
            place(&style.frame)
                .overflow_hidden()
                .flex()
                .flex_col()
                .when(style.vertical_align == OverlayVerticalAlign::Middle, |d| {
                    d.justify_center()
                })
                .when(style.vertical_align == OverlayVerticalAlign::Bottom, |d| {
                    d.justify_end()
                })
                .when(style.align == OverlayTextAlign::Center, |d| d.text_center())
                .when(style.align == OverlayTextAlign::Right, |d| d.text_right())
                .text_color(color)
                .text_size(px(size))
                .when(!family.is_empty(), |d| d.font_family(family))
                .font_weight(FontWeight(style.font_weight as f32))
                .line_height(relative(style.line_height.max(1.0) as f32))
                .child(words)
        };
    slide = slide.child(text_box(&theme.verse, sample.text.clone(), verse_px));
    if theme.reference.show {
        slide = slide.child(text_box(
            &theme.reference.text,
            sample.reference.clone(),
            reference_px,
        ));
    }
    for el in theme.elements.iter().filter(|e| e.above_text) {
        slide = slide.child(element(el));
    }
    slide.into_any_element()
}

impl ThemeEditor {
    fn render_layers(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let theme = &self.draft;
        let kind = self.kind;
        let mut rows: Vec<AnyElement> = Vec::new();
        let row = |id: &str, icon: AnyElement, label: String, hidden: bool| {
            let selected = self.selected_layer.as_deref() == Some(id);
            let (editor, key) = (editor.clone(), id.to_owned());
            h_flex()
                .id(SharedString::from(ids::layer(id)))
                .test_support()
                .role(Role::Button)
                .aria_label(label.clone())
                .h_7()
                .w_full()
                .gap_2()
                .px_2()
                .rounded(radius::md())
                .text_size(text::XS.size)
                .cursor_pointer()
                .when(selected, |d| {
                    d.bg(k.row_selected()).text_color(k.text_primary)
                })
                .when(!selected, |d| {
                    d.text_color(k.text(TextStep::T300))
                        .hover(|s| s.bg(k.surface_tertiary))
                })
                .on_click(move |_, window, cx| {
                    editor.update(cx, |this, cx| {
                        this.select_layer(Some(key.clone()), cx);
                        // A text layer's look is on the Text tab.
                        window.focus(&this.focus, cx);
                    })
                })
                .child(icon)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .when(hidden, |d| d.opacity(0.5))
                        .child(label),
                )
                .children(hidden.then(|| {
                    div()
                        .flex_none()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Hidden")
                }))
                .into_any_element()
        };
        let element_icon = |el: &OverlayElement| -> AnyElement {
            match el.kind {
                OverlayElementKind::Image => {
                    Icon::new(KairoIcon::Image).size_3p5().into_any_element()
                }
                OverlayElementKind::Video => {
                    Icon::new(KairoIcon::Film).size_3p5().into_any_element()
                }
                _ => {
                    let tile = match el.kind {
                        OverlayElementKind::Shape => Self::element_tile(el),
                        kind => Tile {
                            key: kind.as_str().into(),
                            label: String::new(),
                            line_only: false,
                            pick: ShapePick {
                                kind,
                                shape: None,
                                radius_px: 0.0,
                            },
                        },
                    };
                    match self.thumb(&tile, k.text(TextStep::T500)) {
                        Some(image) => img(image).size_4().into_any_element(),
                        None => div().size_4().into_any_element(),
                    }
                }
            }
        };
        let text_icon = || Icon::new(KairoIcon::TextT).size_3p5().into_any_element();
        for el in theme.elements.iter().rev().filter(|e| e.above_text) {
            rows.push(row(
                &el.id,
                element_icon(el),
                model::element_name(el),
                false,
            ));
        }
        rows.push(row(
            canvas::REFERENCE,
            text_icon(),
            if kind == OverlayContentKind::Lyrics {
                "Section label"
            } else {
                "Reference"
            }
            .into(),
            !theme.reference.show,
        ));
        rows.push(row(
            canvas::VERSE,
            text_icon(),
            if kind == OverlayContentKind::Lyrics {
                "Lyrics"
            } else {
                "Verse"
            }
            .into(),
            false,
        ));
        for el in theme.elements.iter().rev().filter(|e| !e.above_text) {
            rows.push(row(
                &el.id,
                element_icon(el),
                model::element_name(el),
                false,
            ));
        }
        v_flex()
            .id("theme-layers")
            .min_h_0()
            .overflow_y_scroll()
            .px_1p5()
            .pb_2()
            .children(rows)
            .into_any_element()
    }
}

fn heading(
    title: &'static str,
    action: Option<AnyElement>,
    cx: &mut Context<ThemeEditor>,
) -> AnyElement {
    let k = *cx.kairo();
    h_flex()
        .flex_none()
        .h_8()
        .justify_between()
        .pl_3()
        .pr_1p5()
        .child(
            div()
                .text_size(text::LABEL.size)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.text(TextStep::T500))
                .child(title.to_uppercase()),
        )
        .children(action)
        .into_any_element()
}

// ─── The slide ─────────────────────────────────────────────────────────────

impl ThemeEditor {
    fn render_main(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let column = v_flex().flex_1().min_w_0().min_h_0().bg(k.surface);
        if self.selected_id.is_some() {
            column
                .child(self.render_bar(cx))
                .child(self.render_stage(window, cx))
                .child(self.render_status(cx))
                .into_any_element()
        } else {
            column.child(self.render_empty(cx)).into_any_element()
        }
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let kind = self.kind;
        let blank = {
            let editor = editor.clone();
            Button::new(ids::EMPTY_BLANK)
                .cursor_pointer()
                .small()
                .label("Blank")
                .on_click(move |_, window, cx| {
                    editor.update(cx, |this, cx| this.create_blank(window, cx))
                })
        };
        let templates = built_in_themes_for_kind(kind).into_iter().map(|t| {
            let (editor, name, theme) = (editor.clone(), t.name.to_owned(), t.theme.clone());
            Button::new(SharedString::from(ids::template(t.id)))
                .cursor_pointer()
                .small()
                .label(t.name)
                .on_click(move |_, window, cx| {
                    editor.update(cx, |this, cx| this.create(&theme, &name, window, cx))
                })
        });
        v_flex()
            .id(ids::EMPTY)
            .test_support()
            .m_auto()
            .max_w(px(384.))
            .gap_4()
            .p_8()
            .items_center()
            .child(
                div()
                    .text_size(text::BASE.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child(format!(
                        "No {} themes yet",
                        content_kind_label(kind).to_lowercase()
                    )),
            )
            .child(
                div()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child("Start from a template."),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_center()
                    .gap_2()
                    .child(blank)
                    .children(templates),
            )
            .into_any_element()
    }

    fn render_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let history = |id: &'static str,
                       label: &'static str,
                       icon: KairoIcon,
                       enabled: bool,
                       run: fn(&mut ThemeEditor, &mut Context<ThemeEditor>)| {
            let editor = editor.clone();
            Button::new(id)
                .cursor_pointer()
                .ghost()
                .small()
                .icon(Icon::new(icon))
                .accessibility_label(label)
                .tooltip(label)
                .disabled(!enabled)
                .on_click(move |_, _, cx| editor.update(cx, run))
        };
        let sample = SegmentedControl::new(ids::SAMPLE, "Sample text")
            .options([
                SegmentOption::new("short", "Short"),
                SegmentOption::new("long", "Long"),
            ])
            .selected(if self.long_sample { "long" } else { "short" })
            .fit()
            .on_change({
                let editor = editor.clone();
                move |key, _, cx| {
                    let long = key.as_ref() == "long";
                    editor.update(cx, |this, cx| this.set_long_sample(long, cx));
                }
            });
        let status = if let Some(notice) = self.notice.clone() {
            div()
                .id(ids::NOTICE)
                .test_support()
                .role(Role::Alert)
                .text_size(text::XS.size)
                .text_color(k.danger_text)
                .child(notice)
        } else {
            div()
                .id(ids::SAVED)
                .test_support()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T400))
                .opacity(if self.saved { 1.0 } else { 0.0 })
                .child("Saved")
        };
        h_flex()
            .flex_none()
            .h(px(44.))
            .gap_2()
            .px_4()
            .border_b_1()
            .border_color(k.surface_line)
            .child(
                div().flex_1().min_w_0().child(
                    Input::new(&self.name_input)
                        .kairo_size()
                        .appearance(false)
                        .aria_label("Theme name"),
                ),
            )
            .child(status)
            .child(
                h_flex()
                    .child(history(
                        ids::UNDO,
                        "Undo",
                        KairoIcon::Undo,
                        self.can_undo(),
                        |e, cx| e.undo(cx),
                    ))
                    .child(history(
                        ids::REDO,
                        "Redo",
                        KairoIcon::Redo,
                        self.can_redo(),
                        |e, cx| e.redo(cx),
                    )),
            )
            .child(sample)
            .into_any_element()
    }

    fn render_stage(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let stage = self.stage_size.get();
        let width = model::fit_frame_width(stage, FRAME_PAD);
        let frame = self.render_frame(width, window, cx);
        let measure = {
            let size = self.stage_size.clone();
            let weak = cx.entity().downgrade();
            canvas(
                move |bounds, _, cx| {
                    let next = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                    if size.get() != next {
                        size.set(next);
                        // The frame is sized from this, so draw again.
                        cx.defer(move |cx| {
                            let _ = weak.update(cx, |_, cx| cx.notify());
                        });
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        div()
            .id(ids::STAGE)
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(k.surface_canvas)
            .on_mouse_down(MouseButton::Left, {
                let editor = editor.clone();
                move |event: &MouseDownEvent, _, cx| {
                    editor.update(cx, |this, cx| {
                        // The slide's own clicks are the canvas's; this is
                        // the pasteboard around it.
                        if !this.frame_bounds.get().contains(&event.position) {
                            this.select_layer(None, cx);
                        }
                    })
                }
            })
            .child(measure)
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pl(px(FRAME_PAD.0))
                    .pt(px(FRAME_PAD.1))
                    .pr(px(FRAME_PAD.2))
                    .pb(px(FRAME_PAD.3))
                    .child(frame),
            )
            .child(self.render_insert_toolbar(cx))
            .into_any_element()
    }

    fn render_frame(
        &mut self,
        width: f32,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let height = width * 9.0 / 16.0;
        let editor = cx.entity();
        let preview: AnyElement = match &self.preview_view {
            Some(view) => div().size_full().child(view.clone()).into_any_element(),
            None => div()
                .id(ids::NO_PREVIEW)
                .test_support()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(k.surface_stage)
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child(if self.preview_failed() {
                    "The preview is unavailable. The output engine is not running."
                } else {
                    ""
                })
                .into_any_element(),
        };
        let layers = self.canvas_layers();
        let frame_size = (f64::from(width), f64::from(height));
        let box_of = |id: &str| -> Option<(PixelBox, &CanvasLayer)> {
            layers.iter().find(|l| l.id == id).map(|l| {
                (
                    PixelBox::new(&l.frame, l.rotation.unwrap_or(0.0), frame_size),
                    l,
                )
            })
        };
        let selected = self.selected_layer.as_deref().and_then(box_of);
        let hover_id = self.hover.as_ref().map(|h| h.id().to_owned());
        let hovered = hover_id
            .as_deref()
            .filter(|id| Some(*id) != self.selected_layer.as_deref())
            .and_then(box_of);
        let overlay = Overlay {
            selected: selected.as_ref().map(|(b, l)| (*b, l.rotation.is_some())),
            hover: hovered.as_ref().map(|(b, _)| *b),
            pen: self.pen.clone(),
        };
        // Names above the hovered and the selected layer.
        let mut labels: Vec<AnyElement> = Vec::new();
        for (b, layer) in selected.iter().chain(hovered.iter()) {
            let (x, y) = b.to_frame((-b.w / 2.0, -b.h / 2.0 - canvas::DELETE_ABOVE_PX));
            labels.push(
                div()
                    .id(SharedString::from(format!("{}-{}", ids::LABEL, layer.id)))
                    .test_support()
                    .absolute()
                    .left(px(x as f32))
                    .top(px(y as f32))
                    .rounded(radius::sm())
                    .bg(black().opacity(0.75))
                    .px_1p5()
                    .py_0p5()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(white())
                    .child(layer.label.clone())
                    .into_any_element(),
            );
        }
        let remover = editor.clone();
        let delete = selected
            .as_ref()
            .filter(|(_, l)| is_deletable(&l.id))
            .map(|(b, l)| {
                let (remover, layer_id) = (remover.clone(), l.id.clone());
                let (x, y) = b.to_frame(b.delete_local());
                let half = canvas::DELETE_PX as f32 / 2.0;
                div()
                    .id(ids::DELETE)
                    .test_support()
                    .role(Role::Button)
                    .aria_label(format!("{} {}", canvas::delete_verb(&l.id), l.label))
                    // The canvas deletes on a pointer press over this handle;
                    // assistive technology presses without a pointer.
                    .on_a11y_action(gpui_kit::AccessibleAction::Click, move |_, _, cx| {
                        let id = layer_id.clone();
                        remover.update(cx, |e, cx| e.delete_layer(&id, cx))
                    })
                    .absolute()
                    .left(px(x as f32 - half))
                    .top(px(y as f32 - half))
                    .size(px(canvas::DELETE_PX as f32))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(radius::sm())
                    .bg(black().opacity(0.75))
                    .text_color(white())
                    .child(Icon::new(KairoIcon::X).size_2p5())
            });
        let guide = |id: &'static str, vertical: bool| {
            let d = div()
                .id(id)
                .test_support()
                .absolute()
                .bg(white().opacity(0.7));
            if vertical {
                d.top_0().bottom_0().left(relative(0.5)).w(px(1.0))
            } else {
                d.left_0().right_0().top(relative(0.5)).h(px(1.0))
            }
        };
        let bounds_cell = self.frame_bounds.clone();
        let cursor = if self.pen.is_some() {
            CursorStyle::Crosshair
        } else {
            cursor_for(self.hover.as_ref(), self.drag.is_some())
        };
        let pointer = {
            let cell = self.frame_bounds.clone();
            move |position: Point<Pixels>| {
                let b: Bounds<Pixels> = cell.get();
                (
                    f64::from(f32::from(position.x - b.origin.x)),
                    f64::from(f32::from(position.y - b.origin.y)),
                )
            }
        };
        let overlay_root = div()
            .id(ids::CANVAS)
            .test_support()
            .role(Role::Group)
            .aria_label("Slide canvas")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, {
                let (editor, pointer) = (editor.clone(), pointer.clone());
                move |event: &MouseDownEvent, window, cx| {
                    editor.update(cx, |this, cx| {
                        window.focus(&this.focus, cx);
                        this.pointer_down(pointer(event.position), cx);
                    })
                }
            })
            .on_mouse_move({
                let (editor, pointer) = (editor.clone(), pointer.clone());
                move |event: &MouseMoveEvent, _, cx| {
                    editor.update(cx, |this, cx| {
                        this.pointer_move(pointer(event.position), event.modifiers.shift, cx)
                    })
                }
            })
            .on_drag(CanvasDrag, |d, _, _, cx| cx.new(|_| d.clone()))
            .on_drag_move({
                let (editor, pointer) = (editor.clone(), pointer.clone());
                move |event: &DragMoveEvent<CanvasDrag>, _, cx| {
                    editor.update(cx, |this, cx| {
                        this.pointer_move(
                            pointer(event.event.position),
                            event.event.modifiers.shift,
                            cx,
                        )
                    })
                }
            })
            .on_drop({
                let editor = editor.clone();
                move |_: &CanvasDrag, _, cx| editor.update(cx, |this, cx| this.pointer_up(cx))
            })
            .on_mouse_up(MouseButton::Left, {
                let editor = editor.clone();
                move |_, _, cx| editor.update(cx, |this, cx| this.pointer_up(cx))
            })
            .on_mouse_up_out(MouseButton::Left, {
                let editor = editor.clone();
                move |_, _, cx| editor.update(cx, |this, cx| this.pointer_up(cx))
            })
            .on_hover({
                let editor = editor.clone();
                move |hovered, _, cx| {
                    if !*hovered {
                        editor.update(cx, |this, cx| this.pointer_left(cx))
                    }
                }
            })
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                let m = e.keystroke.modifiers;
                if !m.platform
                    && !m.control
                    && !m.alt
                    && this.canvas_key(&e.keystroke.key, m.shift, cx)
                {
                    cx.stop_propagation();
                }
            }))
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(bounds);
                        (bounds, overlay)
                    },
                    |_, (bounds, overlay), window, _| {
                        paint_overlay(bounds.origin, &overlay, window)
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(labels)
            .children(delete)
            .children(self.guides.0.then(|| guide(ids::GUIDE_V, true)))
            .children(self.guides.1.then(|| guide(ids::GUIDE_H, false)))
            .children(self.pen.as_ref().map(|pen| {
                let close = pen.points.len() >= 2;
                h_flex()
                    .id(ids::PEN_HINT)
                    .test_support()
                    .role(Role::Status)
                    .absolute()
                    .top_3()
                    .left_0()
                    .right_0()
                    .justify_center()
                    .child(
                        v_flex()
                            .items_center()
                            .px_4()
                            .py_2()
                            .rounded(radius::lg())
                            .bg(black().opacity(0.7))
                            .text_size(text::XS.size)
                            .text_color(white())
                            .child(if close {
                                "Click the first point to close the path."
                            } else {
                                "Click to add points. Drag while clicking to curve."
                            })
                            .child(
                                div()
                                    .text_color(white().opacity(0.7))
                                    .child("Press Esc to finish the line."),
                            ),
                    )
            }));
        div()
            .id(ids::FRAME)
            .test_support()
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .overflow_hidden()
            .border_1()
            .border_color(black().opacity(0.4))
            .child(preview)
            .child(overlay_root)
            .into_any_element()
    }

    fn render_insert_toolbar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let full = self.draft.elements.len() >= MAX_OVERLAY_ELEMENTS;
        let full_tip = format!("A theme holds up to {MAX_OVERLAY_ELEMENTS} elements");
        // ProPresenter-style insert bar: roomy buttons, filled marks, dividers.
        let button =
            |id: &'static str, icon: KairoIcon, label: &'static str, tip: String, off: bool| {
                Button::new(id)
                    .cursor_pointer()
                    .ghost()
                    .with_size(gpui_kit::component::Size::Medium)
                    // An icon-only Kit button is a fixed square; padding on top of
                    // that squeezes the icon out of sight, so size it outright.
                    .h_10()
                    .w_12()
                    .icon(Icon::new(icon.fill()).size_5())
                    .accessibility_label(label)
                    .tooltip(tip)
                    .disabled(off)
            };
        let soon = |id: &'static str, icon: KairoIcon, label: &'static str| {
            button(id, icon, label, format!("{label} — coming soon"), true)
        };
        let divider = || div().w(px(1.)).h_5().bg(k.surface_border);
        let shapes = {
            let color = k.text(TextStep::T300);
            let open = self.shapes_open;
            let groups: Vec<ShapeGroup> = if open {
                gallery::groups(true)
                    .into_iter()
                    .map(|(label, tiles)| {
                        (
                            label,
                            tiles
                                .into_iter()
                                .map(|t| {
                                    let i = self.thumb(&t, color);
                                    (t, i)
                                })
                                .collect(),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let (pick_editor, open_editor) = (editor.clone(), editor.clone());
            Popover::new(ids::SHAPES)
                .open(open)
                .on_open_change(move |open, _, cx| {
                    let open = *open;
                    open_editor.update(cx, |this, cx| {
                        this.shapes_open = open;
                        cx.notify();
                    })
                })
                .button_trigger(
                    // The mark and its menu arrow as one centred group, as
                    // ProPresenter draws it; a Kit dropdown caret would sit
                    // against the far edge.
                    Button::new(ids::ADD_SHAPE)
                        .cursor_pointer()
                        .ghost()
                        .with_size(gpui_kit::component::Size::Medium)
                        .h_10()
                        .px_3()
                        .accessibility_label("Add a shape")
                        .tooltip(if full {
                            full_tip.clone()
                        } else {
                            "Shape".into()
                        })
                        .disabled(full)
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1()
                                .child(Icon::new(KairoIcon::Shapes.fill()).size_5())
                                .child(Icon::new(KairoIcon::ChevronDown).size_3()),
                        ),
                )
                .content(move |_, _, _| {
                    v_flex()
                        .id("theme-shape-grid")
                        .max_h(px(420.))
                        .overflow_y_scroll()
                        .w(px(304.))
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
                                        let (editor, pick) =
                                            (pick_editor.clone(), tile.pick.clone());
                                        div()
                                            .id(SharedString::from(ids::shape(&tile.key)))
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
                                            .on_click(move |_, window, cx| {
                                                let pick = pick.clone();
                                                editor.update(cx, |this, cx| {
                                                    this.shapes_open = false;
                                                    this.add_element(pick, cx);
                                                    window.focus(&this.focus, cx);
                                                })
                                            })
                                            .children(
                                                image.clone().map(|i| img(i).w(px(18.)).h(px(18.))),
                                            )
                                    },
                                )))
                        }))
                })
        };
        let media = |id: &'static str,
                     icon: KairoIcon,
                     label: &'static str,
                     tip: &'static str,
                     kind: MediaKind| {
            let editor = editor.clone();
            button(
                id,
                icon,
                label,
                if full { full_tip.clone() } else { tip.into() },
                full,
            )
            // Focus the canvas first: when the file picker closes, Delete and
            // the arrows act on the new picture or video.
            .on_click(move |_, window, cx| {
                editor.update(cx, |this, cx| {
                    window.focus(&this.focus, cx);
                    this.add_media(kind, cx);
                })
            })
        };
        let add_text = {
            let editor = editor.clone();
            button(
                ids::ADD_TEXT,
                KairoIcon::TextT,
                "Add text",
                if full {
                    full_tip.clone()
                } else {
                    "Text".into()
                },
                full,
            )
            // Focus goes to the canvas, so Delete and the arrows act on the
            // new element rather than on the toolbar button.
            .on_click(move |_, window, cx| {
                editor.update(cx, |this, cx| {
                    this.add_text(cx);
                    window.focus(&this.focus, cx);
                })
            })
        };
        let pen = {
            let editor = editor.clone();
            let on = self.pen.is_some();
            button(
                ids::ADD_PEN,
                KairoIcon::PenNib,
                if on {
                    "Finish the path"
                } else {
                    "Draw with the pen"
                },
                if full && !on {
                    full_tip.clone()
                } else if on {
                    "Pen (Esc to finish)".into()
                } else {
                    "Pen".into()
                },
                full && !on,
            )
            .toggled(on)
            .on_click(move |_, window, cx| {
                editor.update(cx, |this, cx| {
                    this.toggle_pen(cx);
                    window.focus(&this.focus, cx);
                })
            })
        };
        h_flex()
            .absolute()
            .top_3()
            .left_0()
            .right_0()
            .justify_center()
            // The band beside the toolbar is pasteboard: clicking it deselects.
            .on_mouse_down(MouseButton::Left, {
                let editor = editor.clone();
                move |_, _, cx| editor.update(cx, |this, cx| this.select_layer(None, cx))
            })
            .child(
                h_flex()
                    .id(ids::TOOLBAR)
                    .test_support()
                    .role(Role::Toolbar)
                    .aria_label("Add to slide")
                    .gap_1()
                    .p_1()
                    .rounded(radius::xl())
                    .bg(k.surface_tertiary)
                    .border_1()
                    .border_color(k.ink(0.08))
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(add_text)
                    .child(divider())
                    .child(shapes)
                    .child(divider())
                    .child(pen)
                    .child(divider())
                    .child(media(
                        ids::ADD_IMAGE,
                        KairoIcon::Image,
                        "Add an image",
                        "Image",
                        MediaKind::Image,
                    ))
                    .child(divider())
                    .child(media(
                        ids::ADD_VIDEO,
                        KairoIcon::Video,
                        "Add a video",
                        "Video",
                        MediaKind::Video,
                    ))
                    .child(divider())
                    .child(soon(ids::ADD_STICKER, KairoIcon::Sticker, "Stickers")),
            )
            .into_any_element()
    }

    fn render_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let Some(entry) = self.selected_theme().cloned() else {
            return div().into_any_element();
        };
        let screens: Vec<_> = self
            .settings
            .overlay
            .outputs
            .iter()
            .filter(|o| is_rendered_kind(o.kind))
            .cloned()
            .collect();
        let chips = screens.iter().map(|output| {
            let on = output_uses_theme(output, &entry.id, entry.kind);
            // Scripture always needs a theme: pick another theme to move a
            // screen off this one.
            let locked = on && entry.kind == OverlayContentKind::Scripture;
            let (editor, id) = (editor.clone(), output.id.clone());
            h_flex()
                .id(SharedString::from(ids::screen(&output.id)))
                .test_support()
                .role(Role::Button)
                .aria_label(output.name.clone())
                .aria_toggled(if on {
                    gpui_kit::Toggled::True
                } else {
                    gpui_kit::Toggled::False
                })
                .h_6()
                .gap_1()
                .px_2p5()
                .rounded_full()
                .text_size(text::LABEL.size)
                .when(on, |d| d.bg(k.neutral(Step::S200)).text_color(k.surface))
                .when(!on, |d| {
                    d.bg(k.surface_tertiary).text_color(k.text(TextStep::T300))
                })
                // Locked: a press from assistive technology does nothing, as the pointer does.
                .when(locked, |d| d.on_click(|_, _, _| {}))
                .when(!locked, |d| {
                    d.cursor_pointer().on_click(move |_, _, cx| {
                        editor.update(cx, |this, cx| this.toggle_output(&id, cx))
                    })
                })
                .children(on.then(|| Icon::new(KairoIcon::Check).size_2p5()))
                .child(output.name.clone())
        });
        let readout = self
            .selected_layer
            .as_deref()
            .and_then(|id| canvas::layer_box(&self.draft, id))
            .map(|b| {
                h_flex()
                    .id(ids::READOUT)
                    .test_support()
                    .gap_3()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T400))
                    .children(model::frame_readout(&b).into_iter().map(|(label, value)| {
                        h_flex()
                            .gap_1()
                            .child(div().text_color(k.text(TextStep::T500)).child(label))
                            .child(value.to_string())
                    }))
            });
        h_flex()
            .id(ids::STATUS)
            .test_support()
            .flex_none()
            .h(px(36.))
            .gap_1p5()
            .px_3()
            .bg(k.surface_secondary)
            .border_t_1()
            .border_color(k.surface_line)
            .child(
                div()
                    .mr_1()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T400))
                    .child("Used on"),
            )
            .children(screens.is_empty().then(|| {
                div()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T500))
                    .child("no screens yet — add one in Screens")
            }))
            .children(chips)
            .child(div().flex_1())
            .children(readout)
            .into_any_element()
    }

    fn render_aside(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let editor = cx.entity();
        let selected = self.selected_id.is_some();
        let is_default = self.is_default();
        let body: AnyElement = match &self.inspector {
            Some(inspector) if selected => div().size_full().child(inspector.clone()).into_any_element(),
            _ => v_flex()
                .size_full()
                .gap_2()
                .p_4()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child("Click a layer on the slide to select it.")
                .child("Drag to move, drag a handle to resize, drag the round handle to turn an element. Shift keeps the shape while resizing and snaps a turn to 15°.")
                .child("Arrow keys nudge the selection, Delete removes it, Esc deselects.")
                .into_any_element(),
        };
        v_flex()
            .id(ids::ASIDE)
            .test_support()
            .flex_none()
            .w(px(ASIDE_PX))
            .h_full()
            .min_h_0()
            .bg(k.surface_secondary)
            .border_l_1()
            .border_color(k.surface_line)
            .child(div().flex_1().min_h_0().child(body))
            .children(selected.then(|| {
                let (reset, save) = (editor.clone(), editor.clone());
                h_flex()
                    .flex_none()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(k.surface_line)
                    .child(
                        Button::new(ids::RESET)
                            .cursor_pointer()
                            .small()
                            .flex_1()
                            .icon(Icon::new(KairoIcon::RotateCcw))
                            .label("Reset")
                            .tooltip("Undo every change since the default was set")
                            .disabled(is_default)
                            .on_click(move |_, _, cx| {
                                reset.update(cx, |this, cx| this.reset_to_default(cx))
                            }),
                    )
                    .child(
                        Button::new(ids::SAVE)
                            .cursor_pointer()
                            .small()
                            .primary()
                            .flex_1()
                            .label("Save theme")
                            .tooltip("Save these changes — Reset will return to them")
                            .disabled(is_default)
                            .on_click(move |_, _, cx| {
                                save.update(cx, |this, cx| this.set_as_default(cx))
                            }),
                    )
            }))
            .into_any_element()
    }
}

/// The verse and reference sizes the output uses at 1920 × 1080 for this
/// theme and sample, from `kairo_scene`'s layout; cached, since shaping is
/// too slow to repeat for every thumbnail on every frame.
fn fitted_sizes(theme: &OverlayTheme, reference: &str, text: &str) -> (Option<f32>, Option<f32>) {
    use std::cell::RefCell;
    use std::collections::HashMap;
    type Sizes = (Option<f32>, Option<f32>);
    thread_local! {
        static SIZES: RefCell<HashMap<u64, Sizes>> = RefCell::new(HashMap::new());
    }
    let scene = kairo_scene::SlideScene::build(theme, reference, text, (1920, 1080), &[]);
    let key = scene.content_hash();
    if let Some(hit) = SIZES.with(|s| s.borrow().get(&key).copied()) {
        return hit;
    }
    let mut sizes = (None, None);
    for b in kairo_scene::analyse(&scene) {
        match b.kind {
            kairo_scene::BoxKind::Verse => sizes.0 = Some(b.size),
            kairo_scene::BoxKind::Reference => sizes.1 = Some(b.size),
        }
    }
    SIZES.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() > 1024 {
            s.clear();
        }
        s.insert(key, sizes);
    });
    sizes
}
