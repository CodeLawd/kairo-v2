//! Shell and virtualised slide grid.

use crate::model::{self, Band, GridLayout, Move, card_at, grid_layout, marquee_hits, row_count};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{IconName, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use image::{Frame, RgbaImage};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

actions!(
    slide_grid,
    [
        MoveLeft, MoveRight, MoveUp, MoveDown, ZoomIn, ZoomOut, Activate
    ]
);

const THUMB_CACHE: usize = 400;
const HAIRLINE: f32 = 0.5;

pub fn bind_keys(cx: &mut App) {
    let context = Some("SlideGrid");
    cx.bind_keys([
        KeyBinding::new("left", MoveLeft, context),
        KeyBinding::new("right", MoveRight, context),
        KeyBinding::new("up", MoveUp, context),
        KeyBinding::new("down", MoveDown, context),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd-="
            } else {
                "ctrl-="
            },
            ZoomIn,
            context,
        ),
        KeyBinding::new(
            if cfg!(target_os = "macos") {
                "cmd--"
            } else {
                "ctrl--"
            },
            ZoomOut,
            context,
        ),
        KeyBinding::new("enter", Activate, context),
    ]);
}

/// The product token layer. Values are from Kairo's `DESIGN.md` and `index.css`.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub chrome: Rgba,
    pub workspace: Rgba,
    pub panel: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub muted: Rgba,
    pub accent: Rgba,
    pub live: Rgba,
}

impl Tokens {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                chrome: rgba(0x2424249e),
                workspace: rgb(0x181818),
                panel: rgb(0x202020),
                border: rgba(0x404040bf),
                text: rgb(0xececec),
                muted: rgb(0x9a9a9a),
                accent: rgb(0x3b82f6),
                live: rgb(0xef4444),
            }
        } else {
            Self {
                chrome: rgba(0xf4f4f59e),
                workspace: rgb(0xffffff),
                panel: rgb(0xf4f4f5),
                border: rgba(0xd4d4d8bf),
                text: rgb(0x18181b),
                muted: rgb(0x71717a),
                accent: rgb(0x2563eb),
                live: rgb(0xdc2626),
            }
        }
    }
}

#[derive(Clone)]
pub struct DraggedCards(pub HashSet<u32>);

struct DragGhost(usize);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(rgb(0x3b82f6))
            .text_color(white())
            .text_xs()
            .child(format!("{} slides", self.0))
    }
}

struct Marquee {
    /// Content coordinates, so the anchor stays put while the list scrolls.
    start: (f32, f32),
    pointer: (f32, f32),
    active: bool,
    additive: bool,
    base: HashSet<u32>,
}

pub struct SlideGrid {
    tokens: Tokens,
    order: Vec<u32>,
    selected: HashSet<u32>,
    live: Option<u32>,
    cursor: usize,
    min_width: f32,
    width: f32,
    scroll: UniformListScrollHandle,
    focus: FocusHandle,
    thumbs: HashMap<u32, (Arc<RenderImage>, u64)>,
    tick: u64,
    marquee: Option<Marquee>,
    modifier_held: bool,
}

impl SlideGrid {
    pub fn new(count: u32, dark: bool, cx: &mut Context<Self>) -> Self {
        Self {
            tokens: Tokens::new(dark),
            order: (0..count).collect(),
            selected: HashSet::new(),
            live: None,
            cursor: 0,
            min_width: 240.0,
            width: 700.0,
            scroll: UniformListScrollHandle::new(),
            focus: cx.focus_handle(),
            thumbs: HashMap::new(),
            tick: 0,
            marquee: None,
            modifier_held: false,
        }
    }

    pub fn order(&self) -> &[u32] {
        &self.order
    }
    pub fn selected(&self) -> &HashSet<u32> {
        &self.selected
    }
    pub fn live(&self) -> Option<u32> {
        self.live
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn min_width(&self) -> f32 {
        self.min_width
    }
    pub fn thumbnail_count(&self) -> usize {
        self.thumbs.len()
    }
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }
    pub fn scroll_handle(&self) -> &UniformListScrollHandle {
        &self.scroll
    }
    pub fn layout(&self) -> GridLayout {
        grid_layout(self.width, self.min_width)
    }

    fn thumbnail(&mut self, id: u32, window: &mut Window) -> Arc<RenderImage> {
        self.tick += 1;
        let tick = self.tick;
        if let Some(entry) = self.thumbs.get_mut(&id) {
            entry.1 = tick;
            return entry.0.clone();
        }
        let (width, height) = (160u32, 90u32);
        let shade = (id % 200) as u8;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                pixels.extend_from_slice(&[(x + u32::from(shade)) as u8, y as u8 * 2, shade, 255]);
            }
        }
        let buffer = RgbaImage::from_raw(width, height, pixels).expect("thumbnail size");
        let image = Arc::new(RenderImage::new([Frame::new(buffer)]));
        self.thumbs.insert(id, (image.clone(), tick));
        if self.thumbs.len() > THUMB_CACHE
            && let Some(oldest) = self
                .thumbs
                .iter()
                .min_by_key(|(_, entry)| entry.1)
                .map(|(id, _)| *id)
            && let Some((old, _)) = self.thumbs.remove(&oldest)
        {
            let _ = window.drop_image(old);
        }
        image
    }

    fn move_cursor(&mut self, direction: Move, cx: &mut Context<Self>) {
        let layout = self.layout();
        self.cursor = model::navigate(self.cursor, direction, layout.columns, self.order.len());
        self.scroll
            .scroll_to_item(self.cursor / layout.columns.max(1), ScrollStrategy::Top);
        cx.notify();
    }

    fn set_zoom(&mut self, direction: f32, cx: &mut Context<Self>) {
        self.min_width = model::zoom(self.min_width, direction);
        let columns = self.layout().columns.max(1);
        self.scroll
            .scroll_to_item(self.cursor / columns, ScrollStrategy::Top);
        cx.notify();
    }

    fn click_card(
        &mut self,
        position: usize,
        id: u32,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let was = self.cursor;
        self.cursor = position;
        if modifiers.shift {
            let (from, to) = (was.min(position), was.max(position));
            self.selected.extend(self.order[from..=to].iter().copied());
        } else if modifiers.secondary(cx) {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        } else {
            // A plain click sends the slide live, as in `VerseCardGrid`.
            self.live = Some(id);
        }
        cx.notify();
    }

    fn content_point(&self, position: Point<Pixels>) -> (f32, f32) {
        let handle = self.scroll.0.borrow();
        let bounds = handle.base_handle.bounds();
        let offset = handle.base_handle.offset();
        (
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y - offset.y),
        )
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let modifier = event.modifiers.secondary(cx);
        let (x, y) = self.content_point(event.position);
        // A plain press on a card is a click or a drag; leave it alone.
        if !modifier && card_at(self.layout(), self.order.len(), x, y).is_some() {
            return;
        }
        let additive = modifier || event.modifiers.shift;
        self.marquee = Some(Marquee {
            start: (x, y),
            pointer: (f32::from(event.position.x), f32::from(event.position.y)),
            active: false,
            additive,
            base: if additive {
                self.selected.clone()
            } else {
                HashSet::new()
            },
        });
        self.focus.focus(window, cx);
    }

    fn apply_marquee(&mut self, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.as_ref() else {
            return;
        };
        let point = Point {
            x: px(marquee.pointer.0),
            y: px(marquee.pointer.1),
        };
        let now = self.content_point(point);
        let hits = marquee_hits(
            self.layout(),
            self.order.len(),
            Band::between(marquee.start, now),
        );
        let mut selected = marquee.base.clone();
        selected.extend(hits.into_iter().map(|index| self.order[index]));
        self.selected = selected;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let (x, y) = self.content_point(event.position);
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        if !event.dragging() {
            self.marquee = None;
            cx.notify();
            return;
        }
        marquee.pointer = (f32::from(event.position.x), f32::from(event.position.y));
        if !marquee.active
            && (x - marquee.start.0).hypot(y - marquee.start.1) < model::START_DISTANCE
        {
            return;
        }
        let first = !marquee.active;
        marquee.active = true;
        if first && !marquee.additive {
            self.selected.clear();
        }
        self.apply_marquee(cx);
        if first {
            self.run_edge_scroll(cx);
        }
    }

    /// Scrolls while the band sits at the top or bottom edge of the list.
    fn run_edge_scroll(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let running = this.update(cx, |this, cx| {
                    let Some(marquee) = this.marquee.as_ref().filter(|m| m.active) else {
                        return false;
                    };
                    let pointer_y = marquee.pointer.1;
                    let scrolled = {
                        let handle = this.scroll.0.borrow();
                        let base = &handle.base_handle;
                        let bounds = base.bounds();
                        let (top, bottom) = (
                            f32::from(bounds.origin.y),
                            f32::from(bounds.origin.y + bounds.size.height),
                        );
                        let step = if pointer_y < top + model::EDGE {
                            model::SCROLL_STEP
                        } else if pointer_y > bottom - model::EDGE {
                            -model::SCROLL_STEP
                        } else {
                            0.0
                        };
                        if step != 0.0 {
                            let mut offset = base.offset();
                            offset.y = px((f32::from(offset.y) + step)
                                .clamp(-f32::from(base.max_offset().y), 0.0));
                            base.set_offset(offset);
                        }
                        step != 0.0
                    };
                    if scrolled {
                        this.apply_marquee(cx);
                    }
                    true
                });
                if !matches!(running, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    fn mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.marquee.take().is_some() {
            cx.notify();
        }
    }

    fn render_row(
        &mut self,
        row: usize,
        layout: GridLayout,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let start = row * layout.columns;
        let end = (start + layout.columns).min(self.order.len());
        let tokens = self.tokens;
        let drag_enabled = !self.modifier_held;
        let mut element = h_flex()
            .w_full()
            .h(px(layout.row_height))
            .gap(px(model::GAP))
            .items_start();
        for position in start..end {
            let id = self.order[position];
            let picked = self.selected.contains(&id);
            let thumbnail = self.thumbnail(id, window);
            let outline = if self.live == Some(id) {
                tokens.live
            } else if picked || position == self.cursor {
                tokens.accent
            } else {
                tokens.border
            };
            let moved = if picked {
                self.selected.clone()
            } else {
                HashSet::from([id])
            };
            let card = div()
                .id(("card", id))
                .w(px(layout.card_width))
                .h(px(layout.row_height - model::GAP))
                .flex()
                .flex_col()
                .rounded_md()
                .overflow_hidden()
                .border_1()
                .border_color(outline)
                .bg(tokens.panel)
                .hover(|style| style.opacity(0.92))
                .drag_over::<DraggedCards>(move |style, _, _, _| {
                    style.border_color(tokens.accent).opacity(0.7)
                })
                .on_drop(cx.listener(move |this, dragged: &DraggedCards, _, cx| {
                    if model::reorder(&mut this.order, &dragged.0, id) {
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.click_card(position, id, event.modifiers(), cx);
                }))
                .when(drag_enabled, |card| {
                    let count = moved.len();
                    card.on_drag(DraggedCards(moved), move |_, _, _, cx| {
                        cx.new(|_| DragGhost(count))
                    })
                })
                .child(
                    img(thumbnail)
                        .w_full()
                        .flex_1()
                        .object_fit(ObjectFit::Cover),
                )
                .child(
                    h_flex()
                        .h(px(model::FOOTER - 2.0))
                        .px_2()
                        .text_xs()
                        .text_color(tokens.muted)
                        .child(format!("Slide {}", id + 1)),
                );
            element = element.child(card);
        }
        element
    }
}

impl Render for SlideGrid {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let measured = f32::from(self.scroll.0.borrow().base_handle.bounds().size.width);
        if measured > 0.0 && (measured - self.width).abs() > 0.5 {
            self.width = measured;
            cx.notify();
        }
        let _ = window;
        let tokens = self.tokens;
        let layout = self.layout();
        let rows = row_count(self.order.len(), layout.columns);
        let band = self.marquee.as_ref().filter(|m| m.active).map(|marquee| {
            let handle = self.scroll.0.borrow();
            let bounds = handle.base_handle.bounds();
            let offset = handle.base_handle.offset();
            let start = (marquee.start.0, marquee.start.1 + f32::from(offset.y));
            let now = (
                f32::from(px(marquee.pointer.0) - bounds.origin.x),
                f32::from(px(marquee.pointer.1) - bounds.origin.y),
            );
            Band::between(start, now)
        });
        div()
            .id("slide-grid")
            .key_context("SlideGrid")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .on_action(cx.listener(|this, _: &MoveLeft, _, cx| this.move_cursor(Move::Left, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, _, cx| this.move_cursor(Move::Right, cx)))
            .on_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_cursor(Move::Up, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_cursor(Move::Down, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.set_zoom(1.0, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.set_zoom(-1.0, cx)))
            .on_action(cx.listener(|this, _: &Activate, _, cx| {
                this.live = this.order.get(this.cursor).copied();
                cx.notify();
            }))
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                if this.modifier_held != event.modifiers.secondary(cx) {
                    this.modifier_held = event.modifiers.secondary(cx);
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .when(self.order.is_empty(), |grid| {
                grid.flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(tokens.muted)
                    .child("No slides in this playlist")
            })
            .child(
                uniform_list(
                    "slide-rows",
                    rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        range
                            .map(|row| this.render_row(row, layout, window, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.scroll)
                .size_full(),
            )
            .when_some(band, |grid, band| {
                grid.child(
                    div()
                        .absolute()
                        .left(px(band.left))
                        .top(px(band.top))
                        .w(px(band.right - band.left))
                        .h(px(band.bottom - band.top))
                        .border_1()
                        .border_color(tokens.accent)
                        .bg(tokens.accent.alpha(0.18)),
                )
            })
    }
}

fn hairline<E: Styled>(mut element: E, top: bool, left: bool, bottom: bool, right: bool) -> E {
    let widths = &mut element.style().border_widths;
    let value = Some(AbsoluteLength::from(px(HAIRLINE)));
    if top {
        widths.top = value;
    }
    if left {
        widths.left = value;
    }
    if bottom {
        widths.bottom = value;
    }
    if right {
        widths.right = value;
    }
    element
}

/// Header and rail over the material, with an opaque inset workspace.
pub struct Shell {
    tokens: Tokens,
    grid: Entity<SlideGrid>,
    side: Option<AnyView>,
    verse: Entity<InputState>,
}

impl Shell {
    pub fn new(
        grid: Entity<SlideGrid>,
        side: Option<AnyView>,
        dark: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let verse = cx.new(|cx| InputState::new(window, cx).placeholder("Note, for example ẹ̀kọ́"));
        Self {
            tokens: Tokens::new(dark),
            grid,
            side,
            verse,
        }
    }

    pub fn input(&self) -> &Entity<InputState> {
        &self.verse
    }
}

impl Render for Shell {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let tokens = self.tokens;
        let rail = [
            ("operator", IconName::LayoutDashboard, "Operator"),
            ("scripture", IconName::BookOpen, "Scripture"),
            ("media", IconName::Frame, "Media"),
            ("settings", IconName::Settings, "Settings"),
        ];
        v_flex()
            .size_full()
            .text_color(tokens.text)
            .child(
                h_flex()
                    .id("header")
                    .window_control_area(WindowControlArea::Drag)
                    .h(px(60.))
                    .flex_shrink_0()
                    .bg(tokens.chrome)
                    .pl(px(88.))
                    .pr_3()
                    .gap_3()
                    .child(div().text_sm().font_semibold().child("Sunday Service"))
                    .child(
                        div()
                            .w(px(260.))
                            .child(Input::new(&self.verse).id("note").small()),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(px(68.))
                            .h_full()
                            .flex_shrink_0()
                            .bg(tokens.chrome)
                            .items_center()
                            .gap_2()
                            .pt_2()
                            .children(rail.map(|(id, icon, label)| {
                                Button::new(id)
                                    .ghost()
                                    .icon(icon)
                                    .tooltip(label)
                                    .with_size(gpui_kit::component::Size::Medium)
                            })),
                    )
                    .child(
                        hairline(
                            h_flex()
                                .flex_1()
                                .h_full()
                                .min_w_0()
                                .bg(tokens.workspace)
                                .rounded_tl(px(16.))
                                .border_color(tokens.border),
                            true,
                            true,
                            false,
                            false,
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .p_4()
                                .child(self.grid.clone()),
                        )
                        .when_some(self.side.clone(), |row, side| {
                            row.child(
                                hairline(
                                    div()
                                        .w(px(340.))
                                        .h_full()
                                        .flex_shrink_0()
                                        .bg(tokens.panel)
                                        .border_color(tokens.border),
                                    false,
                                    true,
                                    false,
                                    false,
                                )
                                .child(side),
                            )
                        }),
                    ),
            )
    }
}
