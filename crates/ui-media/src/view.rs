//! The media dock (`MediaDock.tsx`): a sheet under the booth page with a
//! drag-resizable top edge, the Library and Playlists sidebar (resizable,
//! hideable), and the backgrounds grid (virtualised by row) with push,
//! picks, rubber band, drag reorder, OS file drops, the card and grid
//! menus, the selection bar and the Media Inspector.

use kairo_ui_kit::ControlSize;
use kairo_ui_kit::components::PopoverTriggerExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::{
    Disableable, Icon, Sizable, VirtualListScrollHandle, h_flex, v_flex, v_virtual_list,
};
use gpui_kit::{
    AnyElement, AppContext, Bounds, Context, DragMoveEvent, Entity, ExternalPaths, Focusable,
    FontWeight, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, ParentElement, Pixels, Point, Render, Role,
    SharedString, StatefulInteractiveElement, Styled, StyledImage, Subscription, Window, canvas,
    div, img, point, prelude::FluentBuilder, px, size,
};
use kairo_domain::media::{MediaItem, MediaKind, MediaPlayback};
use kairo_domain::media_order::DropPosition;
use kairo_ui_kit::components::marquee::band_hits;
use kairo_ui_kit::components::{
    ConfirmRequest, Marquee, MarqueeBand, MarqueeStep, PressTarget, SelectionAction, SelectionBar,
    ZoomChanged, ZoomControl, ZoomRange, confirm,
};
use kairo_ui_kit::tokens::{Step, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};
use serde_json::{Map, json};

use crate::backend::MediaBackend;
use crate::inspector::{Inspector, InspectorEvent};
use crate::model::{MediaDock, MediaEvent, PickGesture, Poster};
use crate::player::PreviewPlayer;
use crate::rows::{self, Selection};

/// Element ids, for tests.
pub mod ids {
    pub const DOCK: &str = "media-dock";
    pub const HANDLE: &str = "media-dock-handle";
    pub const EMPTY: &str = "media-dock-empty";
    pub const CHOOSE: &str = "media-dock-choose";
    pub const SIDEBAR: &str = "media-dock-sidebar";
    pub const SIDEBAR_EDGE: &str = "media-dock-sidebar-edge";
    pub const SIDEBAR_TOGGLE: &str = "media-dock-sidebar-toggle";
    pub const ALL: &str = "media-dock-all";
    pub const NEW_PLAYLIST: &str = "media-dock-new-playlist";
    pub const RENAME_PLAYLIST: &str = "media-dock-rename-playlist";
    pub const GRID: &str = "media-dock-grid";
    pub const NO_ITEMS: &str = "media-dock-no-items";
    pub const ERROR: &str = "media-dock-error";
    pub const MISSING: &str = "media-dock-missing";
    pub const ADD_MEDIA: &str = "media-dock-add-media";
    pub const ADD_TILE: &str = "media-dock-add-tile";
    pub const CLOSE: &str = "media-dock-close";
    pub const TITLE: &str = "media-dock-title";
    pub const PICKED_DELETE: &str = "media-dock-picked-delete";
    pub const PICKED_REMOVE: &str = "media-dock-picked-remove";
    pub const RENAME_ITEM: &str = "media-dock-rename-item";

    pub fn card(id: &str) -> String {
        format!("media-card-{id}")
    }
    pub fn playlist(id: &str) -> String {
        format!("media-playlist-{id}")
    }
}

/// Width the selected card's preview decodes at.
const PREVIEW_WIDTH: u32 = 480;

/// A card carried to rearrange.
#[derive(Clone)]
pub struct CardDrag {
    pub id: String,
    label: SharedString,
}

impl Render for CardDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        kairo_ui_kit::components::DragPill::new(self.label.clone(), "▣").render_pill(cx)
    }
}

/// The top edge being dragged.
#[derive(Clone)]
struct HeightDrag;
impl Render for HeightDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// The sidebar edge being dragged.
#[derive(Clone)]
struct WidthDrag;
impl Render for WidthDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

pub struct MediaDockView {
    model: Entity<MediaDock>,
    backend: Arc<dyn MediaBackend>,
    zoom: Entity<ZoomControl>,
    rename: Entity<InputState>,
    renaming_for: Option<String>,
    scroll: VirtualListScrollHandle,
    grid_bounds: Rc<std::cell::Cell<Bounds<Pixels>>>,
    resize_from: Option<(f32, f32)>,
    sidebar_from: Option<(f32, f32)>,
    marquee: Option<Marquee>,
    marquee_cleared_click: bool,
    preview: Option<(String, Entity<PreviewPlayer>)>,
    inspector: Option<(String, Entity<Inspector>, Subscription)>,
    inspector_focus: kairo_ui_kit::components::ModalFocus,
    _subscriptions: Vec<Subscription>,
}

impl MediaDockView {
    pub fn new(model: Entity<MediaDock>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let backend = model.read(cx).backend().clone();
        let start = model.read(cx).zoom();
        let zoom = cx.new(|cx| {
            ZoomControl::new(
                "media-zoom",
                "Media item size",
                ZoomRange {
                    min: rows::MIN_ZOOM,
                    max: rows::MAX_ZOOM,
                    default: rows::DEFAULT_ZOOM,
                    step: 5.0,
                    button_step: 25.0,
                },
                start,
                window,
                cx,
            )
        });
        let rename = cx.new(|cx| InputState::new(window, cx));
        let mut subs = vec![cx.observe(&model, |_, _, cx| cx.notify())];
        subs.push(
            cx.subscribe(&zoom, |this, _, ZoomChanged(v): &ZoomChanged, cx| {
                let v = *v;
                this.model.update(cx, |m, cx| m.set_zoom(v, cx));
            }),
        );
        subs.push(
            cx.subscribe(&rename, |this, state, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } | InputEvent::Blur => {
                    let name = state.read(cx).value().to_string();
                    this.model.update(cx, |m, cx| {
                        if m.renaming_playlist().is_some() {
                            m.commit_rename_playlist(&name, cx)
                        } else {
                            m.commit_rename_item(&name, cx)
                        }
                    });
                }
                _ => {}
            }),
        );
        subs.push(cx.subscribe(&model, |_, _, _: &MediaEvent, cx| cx.notify()));
        // ⌘C · ⌘X · ⌘V while open, and Esc in a name field, before the field.
        let weak = cx.entity().downgrade();
        let rename_focus = rename.read(cx).focus_handle(cx);
        subs.push(cx.intercept_keystrokes(move |event, window, cx| {
            let k = &event.keystroke;
            if k.key == "escape" && rename_focus.is_focused(window) {
                let _ = weak.update(cx, |this, cx| {
                    this.model.update(cx, |m, cx| m.cancel_rename(cx))
                });
                cx.stop_propagation();
                return;
            }
            let command = k.modifiers.platform || k.modifiers.control;
            if !command
                || k.modifiers.alt
                || k.modifiers.shift
                || !matches!(k.key.as_str(), "c" | "x" | "v")
            {
                return;
            }
            // A text field keeps its own copy and paste.
            if window.focused(cx).is_some_and(|f| f == rename_focus) {
                return;
            }
            let handled = weak
                .update(cx, |this, cx| {
                    let typing = this.inspector.is_some();
                    !typing && this.model.update(cx, |m, cx| m.shortcut(&k.key, cx))
                })
                .unwrap_or(false);
            if handled {
                cx.stop_propagation();
            }
        }));
        Self {
            model,
            backend,
            zoom,
            rename,
            renaming_for: None,
            scroll: VirtualListScrollHandle::new(),
            grid_bounds: Rc::new(std::cell::Cell::new(Bounds::default())),
            resize_from: None,
            sidebar_from: None,
            marquee: None,
            marquee_cleared_click: false,
            preview: None,
            inspector: None,
            inspector_focus: kairo_ui_kit::components::ModalFocus::new(cx),
            _subscriptions: subs,
        }
    }

    pub fn model(&self) -> &Entity<MediaDock> {
        &self.model
    }

    /// The selected card's preview player, if any.
    pub fn preview(&self) -> Option<&Entity<PreviewPlayer>> {
        self.preview.as_ref().map(|(_, p)| p)
    }

    pub fn inspector(&self) -> Option<&Entity<Inspector>> {
        self.inspector.as_ref().map(|(_, i, _)| i)
    }

    /// Inputs, the preview and the inspector follow the model.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let renaming = m
            .renaming_playlist()
            .map(|id| {
                (
                    id.to_owned(),
                    m.library()
                        .playlists
                        .iter()
                        .find(|p| p.id == id)
                        .map(|p| p.name.clone()),
                )
            })
            .or_else(|| {
                m.renaming_item()
                    .map(|id| (id.to_owned(), m.item(id).map(|i| i.name.clone())))
            });
        let selected_video = m
            .is_open()
            .then(|| m.selected_item().and_then(|id| m.item(id)))
            .flatten()
            .filter(|i| i.kind == MediaKind::Video && !m.undecodable(&i.id))
            .map(|i| (i.id.clone(), i.path.clone(), m.playback(&i.id)));
        let inspecting = m
            .inspecting()
            .cloned()
            .map(|i| (i.clone(), m.playback(&i.id)));
        if renaming.as_ref().map(|(id, _)| id) != self.renaming_for.as_ref() {
            if let Some((_, Some(name))) = &renaming {
                let name = name.clone();
                self.rename.update(cx, |s, cx| {
                    s.set_value(name, window, cx);
                    s.focus(window, cx);
                    s.select_all(window, cx);
                });
            }
            self.renaming_for = renaming.map(|(id, _)| id);
        }
        match (selected_video, &self.preview) {
            (Some((id, _, playback)), Some((held, player))) if &id == held => {
                player.read(cx);
                let _ = playback;
            }
            (Some((id, path, playback)), _) => {
                let backend = self.backend.clone();
                let player =
                    cx.new(|cx| PreviewPlayer::new(backend, path, PREVIEW_WIDTH, true, cx));
                player.update(cx, |p, _| {
                    p.set_range(range_of(&playback));
                    p.set_grade(grade_of(&playback));
                });
                let failed = id.clone();
                let model = self.model.clone();
                cx.subscribe(&player, move |_, _, _: &crate::player::PlayerEvent, cx| {
                    model.update(cx, |m, cx| m.mark_undecodable(failed.clone(), cx));
                })
                .detach();
                self.preview = Some((id, player));
            }
            (None, _) => self.preview = None,
        }
        // The tile keeps focus until here, so it gets it back on close.
        self.inspector_focus.sync(inspecting.is_some(), window, cx);
        match (inspecting, &self.inspector) {
            (Some((item, _)), Some((held, _, _))) if &item.id == held => {}
            (Some((item, playback)), _) => {
                let backend = self.backend.clone();
                let id = item.id.clone();
                let handle = self.inspector_focus.handle().clone();
                let inspector = cx.new(|cx| {
                    let mut inspector = Inspector::new(backend, item, playback, window, cx);
                    inspector.set_modal_focus(handle);
                    inspector
                });
                let sub =
                    cx.subscribe(&inspector, |this, inspector, event: &InspectorEvent, cx| {
                        match event {
                            InspectorEvent::Close => {
                                this.model.update(cx, |m, cx| m.close_inspector(cx))
                            }
                            InspectorEvent::Save(patch) => {
                                let (patch, id) =
                                    (patch.clone(), inspector.read(cx).item_id().to_owned());
                                let weak = inspector.downgrade();
                                this.model.update(cx, |m, cx| {
                                    m.save_inspector(id, patch, cx, move |m, result, cx| {
                                        match result {
                                            Ok(()) => m.close_inspector(cx),
                                            Err(e) => {
                                                let _ =
                                                    weak.update(cx, |i, cx| i.save_failed(e, cx));
                                            }
                                        }
                                    })
                                });
                            }
                        }
                    });
                self.inspector = Some((id, inspector, sub));
            }
            (None, _) => self.inspector = None,
        }
        if let Some((_, player)) = &self.preview {
            player.update(cx, |p, _| p.release_retired(window));
        }
    }

    // ─── Confirms ───────────────────────────────────────────────────────────

    fn confirm_delete_item(
        &mut self,
        item: MediaItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, detail) = rows::delete_item_question(&item.name);
        let model = self.model.clone();
        confirm(
            ConfirmRequest::new(title, "Delete")
                .detail(detail)
                .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.delete_item(item.id.clone(), cx));
                }
            },
        );
    }

    fn confirm_delete_picked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.model.read(cx).picked_count();
        if n == 0 {
            return;
        }
        let (title, detail) = rows::delete_items_question(n);
        let model = self.model.clone();
        confirm(
            ConfirmRequest::new(title, "Delete")
                .detail(detail)
                .destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.delete_picked(cx));
                }
            },
        );
    }

    fn confirm_delete_playlist(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let model = self.model.clone();
        confirm(
            ConfirmRequest::new(rows::delete_playlist_question(&name), "Delete").destructive(),
            window,
            cx,
            move |ok, _, cx| {
                if ok {
                    model.update(cx, |m, cx| m.delete_playlist(id.clone(), cx));
                }
            },
        );
    }

    // ─── Marquee ────────────────────────────────────────────────────────────

    /// Card bounds in window coordinates, by layout (rows scrolled out of
    /// view count too).
    fn card_bounds(&self, cx: &gpui_kit::App) -> Vec<(String, Bounds<Pixels>)> {
        let m = self.model.read(cx);
        let grid = self.grid_bounds.get();
        let width = f32::from(grid.size.width) - 40.;
        let (columns, card) = rows::grid_layout(width, m.zoom());
        let row_h = rows::row_height(card);
        let scroll = f32::from(self.scroll.offset().y);
        m.visible()
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let (r, c) = (i / columns, i % columns);
                let x = f32::from(grid.origin.x) + 20. + c as f32 * (card + rows::GAP_X);
                let y = f32::from(grid.origin.y) + 10. + r as f32 * row_h + scroll;
                (
                    item.id.clone(),
                    Bounds::new(point(px(x), px(y)), size(px(card), px(card * 9. / 16.))),
                )
            })
            .collect()
    }

    fn grid_mouse_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let on_card = self
            .card_bounds(cx)
            .iter()
            .any(|(_, b)| b.contains(&event.position));
        let target = PressTarget {
            interactive: false,
            item: on_card,
            control: false,
        };
        self.marquee = Marquee::press(
            event.position,
            -self.scroll.offset().y,
            event.modifiers,
            target,
        );
    }

    fn grid_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(marquee) = self.marquee.as_mut() else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.marquee = None;
            return;
        }
        let step = marquee.move_to(event.position);
        let band = marquee.band(-self.scroll.offset().y);
        if let MarqueeStep::Began { additive } = step {
            self.model.update(cx, |m, cx| m.begin_marquee(additive, cx));
        }
        if step != MarqueeStep::Pending {
            let hits: Vec<String> = self
                .card_bounds(cx)
                .into_iter()
                .filter(|(_, b)| band_hits(band, *b))
                .map(|(id, _)| id)
                .collect();
            self.model.update(cx, |m, cx| m.update_marquee(hits, cx));
        }
        cx.notify();
    }

    fn grid_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(m) = self.marquee.take() {
            self.marquee_cleared_click = m.release();
            cx.notify();
        }
    }

    // ─── Pieces ─────────────────────────────────────────────────────────────

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let lib = m.library();
        let width = m.sidebar_width();
        let all_active = matches!(m.selection(), Some(Selection::All) | None);
        let model = self.model.clone();
        let row = |id: SharedString, icon: KairoIcon, label: String, count: usize, active: bool| {
            h_flex()
                .id(id)
                .test_support()
                .role(Role::Button)
                .aria_label(SharedString::from(label.clone()))
                .items_center()
                .gap_2()
                .h_7()
                .px_2()
                .rounded(radius::md())
                .cursor_pointer()
                .text_size(text::XS.size)
                .map(|d| {
                    if active {
                        d.bg(k.accent(Step::S500).opacity(0.16))
                            .text_color(k.text_primary)
                            .font_weight(FontWeight::MEDIUM)
                    } else {
                        d.text_color(k.text(TextStep::T300)).hover(|s| {
                            s.bg(k.surface_tertiary.opacity(0.6))
                                .text_color(k.text_primary)
                        })
                    }
                })
                .child(
                    Icon::new(icon)
                        .size_3p5()
                        .flex_none()
                        .text_color(if active {
                            k.accent(Step::S400)
                        } else {
                            k.text(TextStep::T500)
                        }),
                )
                .child(div().flex_1().min_w_0().truncate().child(label))
                .child(
                    div()
                        .flex_none()
                        .text_size(text::MICRO.size)
                        .text_color(if active {
                            k.accent(Step::S300)
                        } else {
                            k.text(TextStep::T600)
                        })
                        .when(count > 0, |d| d.child(count.to_string())),
                )
        };
        let heading = |label: &'static str| {
            div()
                .px_2()
                .flex_1()
                .text_size(text::MICRO.size)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(k.text(TextStep::T500))
                .child(label)
        };
        let all = {
            let m = model.clone();
            row(
                ids::ALL.into(),
                KairoIcon::Images,
                "All backgrounds".into(),
                lib.items.len(),
                all_active,
            )
            .on_click(move |_, _, cx| m.update(cx, |m, cx| m.select(Selection::All, cx)))
        };
        let renaming = m.renaming_playlist().map(str::to_owned);
        let playlists: Vec<AnyElement> = lib
            .playlists
            .iter()
            .map(|p| {
                if renaming.as_deref() == Some(p.id.as_str()) {
                    return div()
                        .id(ids::RENAME_PLAYLIST)
                        .test_support()
                        .px_1p5()
                        .child(
                            Input::new(&self.rename)
                                .kairo_size()
                                .small()
                                .aria_label("Playlist name"),
                        )
                        .into_any_element();
                }
                let active = matches!(m.selection(), Some(Selection::Playlist(id)) if *id == p.id);
                let (sel, ren, del) = (model.clone(), model.clone(), cx.entity());
                let (id1, id2, id3, name) =
                    (p.id.clone(), p.id.clone(), p.id.clone(), p.name.clone());
                let menu_model = model.clone();
                row(
                    ids::playlist(&p.id).into(),
                    KairoIcon::Layers,
                    p.name.clone(),
                    p.item_ids.len(),
                    active,
                )
                .on_click(move |event, _, cx| {
                    if event.click_count() >= 2 {
                        ren.update(cx, |m, cx| m.begin_rename_playlist(id2.clone(), cx))
                    } else {
                        sel.update(cx, |m, cx| m.select(Selection::Playlist(id1.clone()), cx))
                    }
                })
                .context_menu(move |menu, _, _| {
                    let (r, d) = (menu_model.clone(), del.clone());
                    let (ri, di, dn) = (id3.clone(), id3.clone(), name.clone());
                    menu.item(
                        PopupMenuItem::new("Rename")
                            .icon(Icon::new(KairoIcon::Edit2))
                            .on_click(move |_, _, cx| {
                                r.update(cx, |m, cx| m.begin_rename_playlist(ri.clone(), cx))
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Delete playlist…")
                            .icon(Icon::new(KairoIcon::Trash2))
                            .on_click(move |_, window, cx| {
                                d.update(cx, |v, cx| {
                                    v.confirm_delete_playlist(di.clone(), dn.clone(), window, cx)
                                })
                            }),
                    )
                })
                .into_any_element()
            })
            .collect();
        let none = lib.playlists.is_empty().then(|| {
            let m = model.clone();
            h_flex()
                .id("media-dock-new-playlist-row")
                .items_center()
                .gap_2()
                .px_2p5()
                .py_1p5()
                .rounded(radius::md())
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .hover(|s| {
                    s.bg(k.surface_tertiary.opacity(0.65))
                        .text_color(k.text_primary)
                })
                .cursor_pointer()
                .on_click(move |_, _, cx| m.update(cx, |m, cx| m.new_playlist(cx)))
                .child(Icon::new(KairoIcon::Plus).size_3())
                .child("New playlist")
        });
        let new = model.clone();
        v_flex()
            .id(ids::SIDEBAR)
            .test_support()
            .relative()
            .w(px(width))
            .flex_none()
            .h_full()
            .px_1p5()
            .pt_1p5()
            .pb_2()
            .gap_px()
            .bg(k.surface_secondary)
            .border_r_1()
            .border_color(k.panel_border)
            .overflow_y_scroll()
            .child(
                div()
                    .px_2()
                    .pt_1()
                    .pb_1p5()
                    .text_size(text::LABEL.size)
                    .font_weight(FontWeight::BOLD)
                    .text_color(k.text(TextStep::T400))
                    .child("MEDIA"),
            )
            .child(all)
            .child(
                h_flex()
                    .mt_3()
                    .mb_0p5()
                    .h_6()
                    .items_center()
                    .child(heading("Playlists"))
                    .child(
                        Button::new(ids::NEW_PLAYLIST)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Plus))
                            .accessibility_label("New playlist")
                            .tooltip("New playlist")
                            .on_click(move |_, _, cx| new.update(cx, |m, cx| m.new_playlist(cx))),
                    ),
            )
            .children(playlists)
            .children(none)
            .child(
                div()
                    .id(ids::SIDEBAR_EDGE)
                    .test_support()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .right_0()
                    .w_1p5()
                    .cursor_col_resize()
                    .hover(|s| s.bg(k.neutral(Step::S600)))
                    .on_drag(WidthDrag, |d, _, _, cx| cx.new(|_| d.clone()))
                    .on_drag_move(
                        cx.listener(|this, event: &DragMoveEvent<WidthDrag>, _, cx| {
                            let x = f32::from(event.event.position.x);
                            let (start_x, start_w) = *this
                                .sidebar_from
                                .get_or_insert((x, this.model.read(cx).sidebar_width()));
                            this.model
                                .update(cx, |m, cx| m.set_sidebar_width(start_w + x - start_x, cx));
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.sidebar_from = None),
                    )
                    .on_drop(cx.listener(|this, _: &WidthDrag, _, _| this.sidebar_from = None)),
            )
            .into_any_element()
    }

    fn render_card(&self, item: &MediaItem, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let id = item.id.clone();
        let live = m.library().live_item_id.as_deref() == Some(item.id.as_str());
        let picked = m.picks().contains(&item.id);
        let busy = m.busy() == Some(item.id.as_str());
        let undecodable = m.undecodable(&item.id);
        let playback = m.playback(&item.id);
        let renaming = m.renaming_item() == Some(item.id.as_str());
        let dragging = m.drag().dragging.as_deref() == Some(item.id.as_str());
        let over = m
            .drag()
            .over
            .as_ref()
            .filter(|(t, _)| *t == item.id)
            .map(|(_, p)| *p);
        let fit = match playback.fit.flatten() {
            Some(kairo_domain::overlay::MediaFit::Contain) => ObjectFit::Contain,
            Some(kairo_domain::overlay::MediaFit::Fill) => ObjectFit::Fill,
            _ => ObjectFit::Cover,
        };
        let picture: AnyElement = match (item.kind, &self.preview) {
            (MediaKind::Video, Some((held, player)))
                if *held == item.id && player.read(cx).image().is_some() =>
            {
                img(player.read(cx).image().cloned().unwrap())
                    .size_full()
                    .object_fit(fit)
                    .into_any_element()
            }
            (MediaKind::Video, _) => match m.poster(&item.id) {
                Some(Poster::Ready(path)) => img(path.clone())
                    .size_full()
                    .object_fit(fit)
                    .into_any_element(),
                _ => div().size_full().into_any_element(),
            },
            (MediaKind::Image, _) => img(PathBuf::from(&item.path))
                .size_full()
                .object_fit(fit)
                .into_any_element(),
        };
        let model = self.model.clone();
        let click_id = id.clone();
        let page = cx.entity();
        let menu_item = item.clone();
        let menu_playback = playback.clone();
        let drag = CardDrag {
            id: id.clone(),
            label: item.name.clone().into(),
        };
        let drop_model = self.model.clone();
        let drop_target = id.clone();
        let over_target = id.clone();
        let height = width * 9. / 16.;
        let border = if picked {
            k.white
        } else if live {
            k.live
        } else {
            k.monitor().opacity(0.)
        };
        let name_row: AnyElement = if renaming {
            div()
                .id(ids::RENAME_ITEM)
                .test_support()
                .child(
                    Input::new(&self.rename)
                        .kairo_size()
                        .xsmall()
                        .aria_label("Media name"),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_1p5()
                .px_0p5()
                .h(px(rows::LABEL_HEIGHT - 6.))
                .child(
                    Icon::new(if item.kind == MediaKind::Video {
                        KairoIcon::Film
                    } else {
                        KairoIcon::Image
                    })
                    .size_2p5()
                    .text_color(k.text(TextStep::T600)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(text::LABEL.size)
                        .map(|d| {
                            if live {
                                d.font_weight(FontWeight::MEDIUM).text_color(k.text_primary)
                            } else {
                                d.text_color(k.text(TextStep::T400))
                            }
                        })
                        .child(item.name.clone()),
                )
                .when(undecodable, |d| {
                    d.child(
                        div()
                            .text_size(text::MICRO.size)
                            .text_color(k.warning_text)
                            .child("Can’t play"),
                    )
                })
                .when(
                    item.kind == MediaKind::Video && playback.looped && !undecodable,
                    |d| {
                        d.child(
                            Icon::new(KairoIcon::Repeat)
                                .size_2p5()
                                .text_color(k.text(TextStep::T500)),
                        )
                    },
                )
                .into_any_element()
        };
        v_flex()
            .relative()
            .w(px(width))
            .gap_1p5()
            .when(dragging, |d| d.opacity(0.4))
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<CardDrag>, _, cx| {
                    let b = event.bounds;
                    if !b.contains(&event.event.position) {
                        return;
                    }
                    let before = rows::drop_before(
                        f32::from(event.event.position.x),
                        f32::from(b.left()),
                        f32::from(b.size.width),
                    );
                    let t = over_target.clone();
                    this.model.update(cx, |m, cx| {
                        m.drag_over(
                            t,
                            if before {
                                DropPosition::Before
                            } else {
                                DropPosition::After
                            },
                            cx,
                        )
                    });
                }),
            )
            .on_drop(cx.listener(move |this, drag: &CardDrag, _, cx| {
                let position = this
                    .model
                    .read(cx)
                    .drag()
                    .over
                    .as_ref()
                    .filter(|(t, _)| *t == drop_target)
                    .map_or(DropPosition::Before, |(_, p)| *p);
                let (from, to) = (drag.id.clone(), drop_target.clone());
                drop_model.update(cx, |m, cx| m.reorder(from, to, position, cx));
                cx.stop_propagation();
            }))
            .children(over.map(|p| {
                div()
                    .absolute()
                    .top_0()
                    .bottom(px(rows::LABEL_HEIGHT))
                    .w_0p5()
                    .rounded_full()
                    .bg(k.accent(Step::S400))
                    .map(|d| {
                        if p == DropPosition::Before {
                            d.left(px(-8.))
                        } else {
                            d.right(px(-8.))
                        }
                    })
            }))
            .child(
                div()
                    .id(SharedString::from(ids::card(&id)))
                    .test_support()
                    .role(Role::Button)
                    .aria_label(SharedString::from(format!("Push {} to screen", item.name)))
                    .relative()
                    .w_full()
                    .h(px(height))
                    .overflow_hidden()
                    .bg(k.monitor())
                    .border_2()
                    .border_color(border)
                    .when(!picked && !live, |d| {
                        d.hover(|s| s.border_color(k.neutral(Step::S600)))
                    })
                    .when(busy, |d| d.opacity(0.7))
                    .on_drag(drag, |d, _, _, cx| cx.new(|_| d.clone()))
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                            if std::mem::take(&mut this.marquee_cleared_click) {
                                return;
                            }
                            let mods = event.modifiers();
                            let gesture = if mods.shift {
                                Some(PickGesture::Range)
                            } else if mods.platform || mods.control {
                                Some(PickGesture::Toggle)
                            } else {
                                None
                            };
                            let id = click_id.clone();
                            model.update(cx, |m, cx| match gesture {
                                Some(g) => m.pick(&id, g, cx),
                                None => m.push(id, cx),
                            });
                        }),
                    )
                    .context_menu(move |menu, window, cx| {
                        card_menu(menu, &page, &menu_item, &menu_playback, window, cx)
                    })
                    .child(picture)
                    .when(picked, |d| {
                        d.child(
                            div()
                                .absolute()
                                .left(px(6.))
                                .top(px(6.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .size_4()
                                .rounded_full()
                                .bg(k.white)
                                .text_color(k.monitor())
                                .child(Icon::new(KairoIcon::Check).size_2p5()),
                        )
                    }),
            )
            .child(name_row)
            .into_any_element()
    }

    fn render_grid(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let items: Vec<MediaItem> = m.visible().into_iter().cloned().collect();
        let in_playlist = matches!(m.selection(), Some(Selection::Playlist(_)));
        let adding = m.adding();
        let width = f32::from(self.grid_bounds.get().size.width) - 40.;
        let (columns, card) = rows::grid_layout(width, m.zoom());
        let total = items.len() + usize::from(in_playlist);
        let row_count = total.div_ceil(columns);
        let sizes = Rc::new(vec![
            size(px(width.max(1.)), px(rows::row_height(card)));
            row_count
        ]);
        let items = Rc::new(items);
        let measured = self.grid_bounds.clone();
        let view = cx.entity();
        let empty = (items.is_empty() && !in_playlist).then(|| {
            div()
                .id(ids::NO_ITEMS)
                .test_support()
                .px_5()
                .py_2p5()
                .text_size(text::XS.size)
                .text_color(k.text(TextStep::T500))
                .child("No backgrounds yet — drop files in the folder, or open a playlist and add media.")
        });
        let page = cx.entity();
        let can_paste = m.can_paste();
        let drop_model = self.model.clone();
        let band = self
            .marquee
            .as_ref()
            .filter(|mq| mq.is_active())
            .map(|mq| mq.band(-self.scroll.offset().y));
        let list_items = items.clone();
        div()
            .id(ids::GRID)
            .test_support()
            .relative()
            .flex_1()
            .min_h_0()
            .on_mouse_down(MouseButton::Left, cx.listener(Self::grid_mouse_down))
            .on_mouse_move(cx.listener(Self::grid_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::grid_mouse_up))
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(k.white.opacity(0.04)))
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                let list = paths.paths().to_vec();
                drop_model.update(cx, |m, cx| m.drop_files(list, cx));
            })
            .context_menu(move |menu, _, _| {
                let p = page.clone();
                menu.item(
                    PopupMenuItem::new("Paste")
                        .icon(Icon::new(KairoIcon::ClipboardPaste))
                        .disabled(!can_paste)
                        .on_click(move |_, _, cx| {
                            p.update(cx, |v, cx| v.model.update(cx, |m, cx| m.paste(cx)))
                        }),
                )
            })
            .child(
                canvas(
                    move |bounds, window, _| {
                        if measured.get() != bounds {
                            let resized =
                                (measured.get().size.width - bounds.size.width).abs() > px(0.5);
                            measured.set(bounds);
                            if resized {
                                // Refreshing mid-draw is ignored, and a cached view would not redraw:
                                // notify the view once this frame ends.
                                window.on_next_frame(move |_, cx| {
                                    view.update(cx, |_, cx| cx.notify())
                                });
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .children(empty)
            .child(
                v_virtual_list(
                    cx.entity(),
                    "media-dock-rows",
                    sizes,
                    move |this, range, _, cx| {
                        range
                            .map(|r| {
                                let start = r * columns;
                                let mut cards: Vec<AnyElement> = list_items
                                    .iter()
                                    .skip(start)
                                    .take(columns)
                                    .map(|item| this.render_card(item, card, cx))
                                    .collect();
                                if in_playlist
                                    && start + columns > list_items.len()
                                    && cards.len() < columns
                                {
                                    cards.push(this.render_add_tile(card, adding, cx));
                                }
                                h_flex()
                                    .items_start()
                                    .gap(px(rows::GAP_X))
                                    .children(cards)
                                    .into_any_element()
                            })
                            .collect::<Vec<_>>()
                    },
                )
                .track_scroll(&self.scroll)
                .size_full()
                .px_5()
                .py_2p5(),
            )
            .children(band.map(|b| {
                let origin = self.grid_bounds.get().origin;
                div()
                    .absolute()
                    .left(b.left() - origin.x)
                    .top(b.top() - origin.y)
                    .w(b.size.width)
                    .h(b.size.height)
                    .child(MarqueeBand::new(Bounds::new(Point::default(), b.size)))
            }))
            .into_any_element()
    }

    fn render_add_tile(&self, width: f32, adding: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let model = self.model.clone();
        v_flex()
            .id(ids::ADD_TILE)
            .test_support()
            .role(Role::Button)
            .aria_label(if adding { "Adding…" } else { "Add media" })
            .w(px(width))
            .h(px(width * 9. / 16.))
            .items_center()
            .justify_center()
            .gap_1p5()
            .bg(k.surface_secondary)
            .text_color(k.text(TextStep::T500))
            .hover(|s| s.bg(k.surface_tertiary).text_color(k.text(TextStep::T200)))
            // While adding, a press from assistive technology does nothing, as the pointer does.
            .when(adding, |d| d.opacity(0.5).on_click(|_, _, _| {}))
            .when(!adding, |d| {
                d.on_click(move |_, _, cx| model.update(cx, |m, cx| m.add_media_to_playlist(cx)))
            })
            .child(Icon::new(KairoIcon::Plus).size_4())
            .child(
                div()
                    .text_size(text::XS.size)
                    .font_weight(FontWeight::MEDIUM)
                    .child(if adding { "Adding…" } else { "Add media" }),
            )
            .into_any_element()
    }

    fn render_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let m = self.model.read(cx);
        let label = rows::selection_label(m.selection(), m.library());
        let count = rows::count_label(m.visible().len(), "background");
        let in_playlist = matches!(m.selection(), Some(Selection::Playlist(_)));
        let adding = m.adding();
        let error = m.error().map(str::to_owned);
        let missing = m.missing();
        let picks = m.picks().len();
        let playlists = m.library().playlists.clone();
        let sidebar_open = m.sidebar_open();
        let (toggle, add, close, clear) = (
            self.model.clone(),
            self.model.clone(),
            self.model.clone(),
            self.model.clone(),
        );
        let warn = |id: &'static str, text_: String| {
            h_flex()
                .id(id)
                .test_support()
                .mx_5()
                .mt_2()
                .items_start()
                .gap_1p5()
                .border_l_2()
                .border_color(k.warning_text)
                .bg(k.surface_secondary)
                .px_2p5()
                .py_1p5()
                .text_size(text::LABEL.size)
                .text_color(k.text(TextStep::T300))
                .child(
                    Icon::new(KairoIcon::AlertTriangle)
                        .size_2p5()
                        .text_color(k.warning_text),
                )
                .child(div().flex_1().child(text_))
        };
        let selection = (picks > 0).then(|| {
            let page = cx.entity();
            let mut bar = SelectionBar::new(picks, "item");
            if !playlists.is_empty() {
                let model = self.model.clone();
                bar = bar.action(
                    Button::new("media-picked-add")
                        .cursor_pointer()
                        .xsmall()
                        .icon(Icon::new(KairoIcon::Plus))
                        .label("Add to playlist")
                        .popover_trigger()
                        .dropdown_menu(move |mut menu, _, _| {
                            menu = menu.max_h(px(288.)).scrollable(true);
                            for p in &playlists {
                                let (m, id) = (model.clone(), p.id.clone());
                                menu = menu.item(PopupMenuItem::new(p.name.clone()).on_click(
                                    move |_, _, cx| {
                                        m.update(cx, |m, cx| {
                                            m.add_picked_to_playlist(id.clone(), cx)
                                        })
                                    },
                                ));
                            }
                            menu
                        }),
                );
            }
            if in_playlist {
                let m = self.model.clone();
                bar = bar.action(
                    SelectionAction::new(ids::PICKED_REMOVE, "Remove from playlist").on_click(
                        move |_, _, cx| m.update(cx, |m, cx| m.remove_picked_from_playlist(cx)),
                    ),
                );
            }
            let p = page.clone();
            bar = bar
                .action(
                    SelectionAction::new(ids::PICKED_DELETE, "Delete")
                        .icon(KairoIcon::Trash2)
                        .danger(true)
                        .on_click(move |_, window, cx| {
                            p.update(cx, |v, cx| v.confirm_delete_picked(window, cx))
                        }),
                )
                .on_clear(move |_, _, cx| clear.update(cx, |m, cx| m.clear_picks(cx)));
            div().mx_5().mb_2p5().child(bar)
        });
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_none()
                    .flex_wrap()
                    .gap_2()
                    .px_4()
                    .pt_1p5()
                    .child(
                        Button::new(ids::SIDEBAR_TOGGLE)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::SidebarLeft))
                            .accessibility_label(if sidebar_open {
                                "Hide media sidebar"
                            } else {
                                "Show media sidebar"
                            })
                            .tooltip(if sidebar_open {
                                "Hide media sidebar"
                            } else {
                                "Show media sidebar"
                            })
                            .on_click(move |_, _, cx| {
                                toggle.update(cx, |m, cx| m.toggle_sidebar(cx))
                            }),
                    )
                    .child(
                        h_flex()
                            .id(ids::TITLE)
                            .test_support()
                            .items_baseline()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T200))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T600))
                                    .child(count),
                            ),
                    )
                    .child(div().flex_1())
                    .child(self.zoom.clone())
                    .when(in_playlist, |d| {
                        d.child(
                            Button::new(ids::ADD_MEDIA)
                                .cursor_pointer()
                                .primary()
                                .xsmall()
                                .icon(Icon::new(KairoIcon::Plus))
                                .label(if adding { "Adding…" } else { "Add media" })
                                .disabled(adding)
                                .on_click(move |_, _, cx| {
                                    add.update(cx, |m, cx| m.add_media_to_playlist(cx))
                                }),
                        )
                    })
                    .child(
                        Button::new(ids::CLOSE)
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::ChevronDown))
                            .accessibility_label("Close media panel")
                            .tooltip("Close media panel — playback continues")
                            .on_click(move |_, _, cx| {
                                close.update(cx, |m, cx| m.set_open(false, cx))
                            }),
                    ),
            )
            .children(error.map(|e| warn(ids::ERROR, e)))
            .when(!missing.is_empty(), |d| {
                d.child(warn(ids::MISSING, rows::missing_note(&missing)))
            })
            .child(self.render_grid(cx))
            .children(selection)
            .into_any_element()
    }

    fn render_handle(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        div()
            .id(ids::HANDLE)
            .test_support()
            .aria_label("Resize media panel")
            .tab_index(0)
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h_1p5()
            .flex()
            .items_center()
            .justify_center()
            .cursor_ns_resize()
            .group("media-handle")
            .on_drag(HeightDrag, |d, _, _, cx| cx.new(|_| d.clone()))
            .on_drag_move(
                cx.listener(|this, event: &DragMoveEvent<HeightDrag>, _, cx| {
                    let y = f32::from(event.event.position.y);
                    let (start_y, start_h) = *this
                        .resize_from
                        .get_or_insert((y, this.model.read(cx).height()));
                    // Dragging up grows the dock.
                    this.model
                        .update(cx, |m, cx| m.resize_to(start_h + (start_y - y), cx));
                }),
            )
            .on_drop(
                cx.listener(|this, _: &HeightDrag, window, cx| this.finish_height_drag(window, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.finish_height_drag(window, cx)),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if this
                    .model
                    .update(cx, |m, cx| m.resize_key(&event.keystroke.key, cx))
                {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .h_0p5()
                    .w_8()
                    .rounded_full()
                    .group_hover("media-handle", |s| s.bg(k.neutral(Step::S400))),
            )
            .into_any_element()
    }

    fn finish_height_drag(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((start_y, start_h)) = self.resize_from.take() else {
            return;
        };
        let y = f32::from(window.mouse_position().y);
        self.model
            .update(cx, |m, cx| m.finish_resize(start_h, y - start_y, cx));
    }

    /// Drives a top-edge drag from tests: `travel` is how far down it moved.
    pub fn drag_height(&mut self, travel: f32, cx: &mut Context<Self>) {
        let start = self.model.read(cx).height();
        self.model.update(cx, |m, cx| {
            m.resize_to(start - travel, cx);
            m.finish_resize(start, travel, cx);
        });
    }
}

/// Right-click on a card (`ItemMenu`).
fn card_menu(
    menu: PopupMenu,
    page: &Entity<MediaDockView>,
    item: &MediaItem,
    playback: &MediaPlayback,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    // Wider than the Kit default so labels never crowd the icons.
    let menu = menu.min_w(px(240.));
    let model = page.read(cx).model.clone();
    let m = model.read(cx);
    let playlists = m.library().playlists.clone();
    let can_paste = m.can_paste();
    let in_playlist = matches!(m.selection(), Some(Selection::Playlist(_)));
    let id = item.id.clone();
    // The menu's target is the selected item, for ⌘C and friends.
    model.update(cx, |m, cx| m.select_item(id.clone(), cx));
    let mk = |f: fn(&mut MediaDock, String, &mut Context<MediaDock>)| {
        let (m, id) = (model.clone(), id.clone());
        move |_: &gpui_kit::ClickEvent, _: &mut Window, cx: &mut gpui_kit::App| {
            m.update(cx, |m, cx| f(m, id.clone(), cx))
        }
    };
    let mut menu = menu
        .item(
            PopupMenuItem::new("Rename")
                .icon(Icon::new(KairoIcon::Edit2))
                .on_click(mk(MediaDock::begin_rename_item)),
        )
        .item(
            PopupMenuItem::new("Copy")
                .icon(Icon::new(KairoIcon::Copy))
                .on_click(mk(MediaDock::copy)),
        )
        .item(
            PopupMenuItem::new("Cut")
                .icon(Icon::new(KairoIcon::Scissors))
                .on_click(mk(MediaDock::cut)),
        )
        .item({
            let m = model.clone();
            PopupMenuItem::new("Paste")
                .icon(Icon::new(KairoIcon::ClipboardPaste))
                .disabled(!can_paste)
                .on_click(move |_, _, cx| m.update(cx, |m, cx| m.paste(cx)))
        })
        .item(
            PopupMenuItem::new(if cfg!(target_os = "macos") {
                "Show in Finder"
            } else {
                "Show in folder"
            })
            .icon(Icon::new(KairoIcon::FolderOpen))
            .on_click(mk(MediaDock::reveal)),
        )
        .item(
            PopupMenuItem::new("Inspect")
                .icon(Icon::new(if item.kind == MediaKind::Video {
                    KairoIcon::Film
                } else {
                    KairoIcon::Image
                }))
                .on_click(mk(MediaDock::inspect)),
        );
    if item.kind == MediaKind::Video {
        let (m, id, looped) = (model.clone(), id.clone(), playback.looped);
        menu = menu.item(
            PopupMenuItem::new("Loop")
                .icon(Icon::new(KairoIcon::Repeat))
                .checked(looped)
                .on_click(move |_, _, cx| {
                    let mut patch = Map::new();
                    patch.insert("loop".into(), json!(!looped));
                    m.update(cx, |m, cx| m.set_playback(id.clone(), patch, cx))
                }),
        );
    }
    if !playlists.is_empty() {
        // One row that opens the playlists beside it, instead of listing
        // every playlist in this menu.
        let (m, item_id) = (model.clone(), id.clone());
        menu = menu.separator().submenu_with_icon(
            Some(Icon::new(KairoIcon::Plus)),
            "Add to playlist",
            window,
            cx,
            move |mut sub, _, _| {
                sub = sub.min_w(px(200.)).max_h(px(320.)).scrollable(true);
                for p in &playlists {
                    let (m, item_id, pid) = (m.clone(), item_id.clone(), p.id.clone());
                    sub = sub.item(
                        PopupMenuItem::new(p.name.clone()).on_click(move |_, _, cx| {
                            m.update(cx, |m, cx| {
                                m.add_to_playlist(pid.clone(), item_id.clone(), cx)
                            })
                        }),
                    );
                }
                sub
            },
        );
    }
    if in_playlist {
        menu = menu.item(
            PopupMenuItem::new("Remove from playlist")
                .on_click(mk(MediaDock::remove_from_playlist)),
        );
    }
    let (p, it) = (page.clone(), item.clone());
    menu.separator().item(
        PopupMenuItem::new("Delete from folder…")
            .icon(Icon::new(KairoIcon::Trash2))
            .on_click(move |_, window, cx| {
                p.update(cx, |v, cx| v.confirm_delete_item(it.clone(), window, cx))
            }),
    )
}

pub(crate) fn range_of(p: &MediaPlayback) -> crate::player::Range {
    crate::player::Range {
        start: p.start_seconds.unwrap_or(0.),
        end: p.end_seconds.flatten(),
        looped: p.looped,
        rate: p.rate.unwrap_or(1.),
    }
}

pub(crate) fn grade_of(p: &MediaPlayback) -> crate::player::Grade {
    crate::player::Grade {
        brightness: p.brightness,
        contrast: p.contrast,
        saturation: p.saturation,
    }
}

impl Render for MediaDockView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let window_height = f32::from(window.viewport_size().height);
        self.model
            .update(cx, |m, cx| m.window_resized(window_height, cx));
        self.sync(window, cx);
        let k = *cx.kairo();
        let m = self.model.read(cx);
        if !m.is_open() {
            return div().id(ids::DOCK).h_0().into_any_element();
        }
        let height = m.height();
        let has_folder = !m.library().folder.is_empty();
        let sidebar = m.sidebar_open();
        let body: AnyElement = if !has_folder {
            let (close, choose) = (self.model.clone(), self.model.clone());
            v_flex()
                .flex_1()
                .min_h_0()
                .child(div().flex().justify_end().px_4().py_1().child(Button::new("media-dock-close-empty").cursor_pointer().ghost().xsmall().label("Close media").on_click(move |_, _, cx| close.update(cx, |m, cx| m.set_open(false, cx)))))
                .child(
                    v_flex()
                        .id(ids::EMPTY)
                        .test_support()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_3()
                        .p_6()
                        .child(div().flex().items_center().justify_center().size_11().bg(k.surface_secondary).child(Icon::new(KairoIcon::Folder).size_5().text_color(k.text(TextStep::T400))))
                        .child(div().text_size(text::BASE.size).font_weight(FontWeight::SEMIBOLD).text_color(k.text_primary).child("Choose your backgrounds folder"))
                        .child(div().max_w(px(460.)).text_center().text_size(text::XS.size).text_color(k.text(TextStep::T400)).child("Point Kairo at the folder your motion backgrounds already live in. Files already there stay put. Adding media to a playlist copies new files in."))
                        .child(Button::new(ids::CHOOSE).cursor_pointer().primary().small().label("Choose folder…").on_click(move |_, _, cx| choose.update(cx, |m, cx| m.choose_folder(cx)))),
                )
                .into_any_element()
        } else {
            h_flex()
                .flex_1()
                .min_h_0()
                .items_stretch()
                .overflow_hidden()
                .when(sidebar, |d| d.child(self.render_sidebar(cx)))
                .child(self.render_content(cx))
                .into_any_element()
        };
        v_flex()
            .id(ids::DOCK)
            .test_support()
            .flex_none()
            .h(px(height))
            .w_full()
            .relative()
            .overflow_hidden()
            .bg(k.surface)
            .border_t_1()
            .border_color(k.surface_border)
            .child(body)
            .child(self.render_handle(cx))
            .children(self.inspector.as_ref().map(|(_, i, _)| i.clone()))
            .into_any_element()
    }
}
