//! Every component and token on one scrolling page, in dark or light. The
//! `kairo-gallery` binary opens it; the UI tests drive it too.

use crate::brand::{KairoLockup, KairoMark, MarkSize};
use crate::components::dialogs::{self, messages};
use crate::components::{
    AddItem, CodeField, DragPill, Frost, FrostedPanel, LibraryKind, LibraryRow, LibrarySection,
    LibrarySectionEvent, Marquee, MarqueeBand, MarqueeStep, MovableDialog, MovableDialogEvent,
    NumberField, NumberModel, PrefGroup, PressTarget, RailItem, SegmentOption, SegmentedControl,
    SelectionAction, SelectionBar, TrafficLights, ZoomControl, ZoomRange,
};
use crate::control::ControlSize;
use crate::icons::KairoIcon;
use crate::motion;
use crate::theme::{KairoThemeExt, set_appearance, set_reduced_motion};
use crate::tokens::{Appearance, Hue, KairoColors, Step, TextStep, radius, text};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::select::{Select, SelectState};
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, IndexPath, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, ElementId, Entity, FocusHandle,
    Focusable, Hsla, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ParentElement, Pixels, Render, Role, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window, div, point, prelude::FluentBuilder, px, size,
};

const GRID_IDS: [&str; 8] = ["a", "b", "c", "d", "e", "f", "g", "h"];

/// The gallery view.
pub struct Gallery {
    focus: FocusHandle,
    scroll: ScrollHandle,
    segment: SharedString,
    tab: SharedString,
    switch_on: bool,
    name: Entity<InputState>,
    password: Entity<InputState>,
    disabled_input: Entity<InputState>,
    number: Entity<NumberField>,
    select: Entity<SelectState<Vec<SharedString>>>,
    slider: Entity<SliderState>,
    zoom: Entity<ZoomControl>,
    code: Entity<CodeField>,
    library: Entity<LibrarySection>,
    dialog: Option<Entity<MovableDialog>>,
    selected: Vec<SharedString>,
    marquee: Option<Marquee>,
    grid_bounds: Vec<(SharedString, Bounds<Pixels>)>,
    last_answer: SharedString,
    motion_run: usize,
    _subscriptions: Vec<Subscription>,
}

impl Gallery {
    /// The gallery.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Search songs"));
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Password")
        });
        let disabled_input = cx.new(|cx| InputState::new(window, cx).default_value("Read only"));
        disabled_input.update(cx, |input, cx| input.set_disabled(true, cx));
        let number = cx.new(|cx| {
            NumberField::new(
                "font-size",
                "Size",
                NumberModel::new(48.0, 1.0).min(8.0).max(240.0),
                window,
                cx,
            )
            .with_suffix("px")
        });
        let select = cx.new(|cx| {
            SelectState::new(
                vec!["Audience".into(), "Stage".into(), "Livestream".into()],
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let slider = cx.new(|_| SliderState::new().min(0.0).max(100.0).default_value(40.0));
        let zoom = cx.new(|cx| {
            ZoomControl::new(
                "card-size",
                "Verse card size",
                ZoomRange {
                    min: 60.0,
                    max: 200.0,
                    default: 100.0,
                    step: 5.0,
                    button_step: 10.0,
                },
                100.0,
                window,
                cx,
            )
        });
        let code = cx.new(|cx| CodeField::new(6, "Confirmation code", window, cx));
        let library = cx.new(|cx| {
            let mut section =
                LibrarySection::new(LibraryKind::Songs).with_add_items(vec![AddItem {
                    label: "New setlist".into(),
                    icon: Some(KairoIcon::ListMusic),
                }]);
            section.set_libraries(
                vec![
                    LibraryRow {
                        id: "hymns".into(),
                        name: "Hymns".into(),
                        count: 128,
                    },
                    LibraryRow {
                        id: "christmas".into(),
                        name: "Christmas".into(),
                        count: 24,
                    },
                ],
                412,
                cx,
            );
            section
        });
        let subscriptions = vec![cx.subscribe_in(&library, window, Self::on_library)];
        Self {
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            segment: "scripture".into(),
            tab: "verses".into(),
            switch_on: true,
            name,
            password,
            disabled_input,
            number,
            select,
            slider,
            zoom,
            code,
            library,
            dialog: None,
            selected: vec!["b".into()],
            marquee: None,
            grid_bounds: Vec::new(),
            last_answer: "No answer yet".into(),
            motion_run: 0,
            _subscriptions: subscriptions,
        }
    }

    /// Scrolls the page down by `y` pixels (for screenshots of lower
    /// sections).
    pub fn scroll_to(&self, y: f32) {
        self.scroll.set_offset(point(px(0.), px(-y)));
    }

    /// Opens a demo overlay by name: `confirm`, `alert` or `settings`.
    /// Unknown names do nothing.
    pub fn open_demo(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        match name {
            "confirm" => self.ask(messages::tracks_move_to_trash("Intro.mp3"), window, cx),
            "alert" => dialogs::alert(messages::screens_display_taken("Audience"), window, cx),
            "settings" => self.open_dialog(window, cx),
            _ => {}
        }
    }

    /// The selected grid items (marquee demo).
    pub fn selected(&self) -> &[SharedString] {
        &self.selected
    }

    /// The last confirm answer, as shown in the gallery.
    pub fn last_answer(&self) -> &SharedString {
        &self.last_answer
    }

    /// The library section.
    pub fn library(&self) -> &Entity<LibrarySection> {
        &self.library
    }

    /// The movable dialog, if open.
    pub fn dialog(&self) -> Option<&Entity<MovableDialog>> {
        self.dialog.as_ref()
    }

    fn on_library(
        &mut self,
        library: &Entity<LibrarySection>,
        event: &LibrarySectionEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        library.update(cx, |section, cx| match event {
            LibrarySectionEvent::Select(id) => section.set_active(id.clone(), cx),
            LibrarySectionEvent::Create => {
                // The gallery has no store; it shows the rename step.
                let mut rows = vec![
                    LibraryRow {
                        id: "hymns".into(),
                        name: "Hymns".into(),
                        count: 128,
                    },
                    LibraryRow {
                        id: "christmas".into(),
                        name: "Christmas".into(),
                        count: 24,
                    },
                ];
                rows.push(LibraryRow {
                    id: "new".into(),
                    name: "New library".into(),
                    count: 0,
                });
                section.set_libraries(rows, 412, cx);
                section.begin_rename("new", window, cx);
            }
            LibrarySectionEvent::Delete(_) => {
                section.set_error(Some("The gallery cannot delete libraries.".into()), cx)
            }
            LibrarySectionEvent::Assign { library_id, .. } => {
                section.landed(library_id.clone(), cx)
            }
            LibrarySectionEvent::Rename { .. } | LibrarySectionEvent::AddItem(_) => {}
        });
    }

    fn set_theme(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        set_appearance(Appearance::from_setting(key), Some(window), cx);
    }

    fn open_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dialog = cx.new(|cx| {
            MovableDialog::new(
                "settings-window",
                "Settings",
                size(px(520.), px(340.)),
                window,
                cx,
            )
        });
        let sub = cx.subscribe_in(
            &dialog,
            window,
            |this, dialog, event: &MovableDialogEvent, window, cx| {
                if *event == MovableDialogEvent::Dismiss {
                    dialog.update(cx, |dialog, cx| dialog.restore(window, cx));
                    this.dialog = None;
                    cx.notify();
                }
            },
        );
        self._subscriptions.push(sub);
        self.dialog = Some(dialog);
        cx.notify();
    }

    fn ask(
        &mut self,
        request: dialogs::ConfirmRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        dialogs::confirm(request, window, cx, move |yes, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.last_answer = if yes {
                    "Confirmed".into()
                } else {
                    "Cancelled".into()
                };
                cx.notify();
            });
        });
    }

    fn grid_hits(&self, band: Bounds<Pixels>) -> Vec<SharedString> {
        self.grid_bounds
            .iter()
            .filter(|(_, b)| crate::components::marquee::band_hits(band, *b))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

impl Focusable for Gallery {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn section(
    id: &'static str,
    title: &'static str,
    k: &KairoColors,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    v_flex()
        .id(id)
        .gap_3()
        .p_4()
        .rounded(radius::md())
        .bg(k.surface_secondary)
        .child(
            div()
                .text_size(text::LABEL.size)
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(k.text(TextStep::T400))
                .child(title.to_uppercase()),
        )
}

fn swatch(name: &'static str, color: Hsla, k: &KairoColors) -> AnyElement {
    v_flex()
        .w_20()
        .gap_1()
        .child(
            div()
                .h_8()
                .w_full()
                .rounded(radius::base())
                .bg(color)
                .border_1()
                .border_color(k.panel_border),
        )
        .child(
            div()
                .text_size(text::MICRO.size)
                .text_color(k.text(TextStep::T500))
                .child(name),
        )
        .into_any_element()
}

fn label(text_value: impl Into<SharedString>, k: &KairoColors) -> gpui_kit::Div {
    div()
        .text_size(text::LABEL.size)
        .font_weight(gpui_kit::FontWeight::MEDIUM)
        .text_color(k.text(TextStep::T300))
        .child(text_value.into())
}

impl Render for Gallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let appearance = k.appearance;

        let header = h_flex()
            .gap_4()
            .p_4()
            .child(KairoMark::new(MarkSize::Small))
            .child(
                div()
                    .text_size(text::XL.size)
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .text_color(k.text_primary)
                    .child("Kairo design system"),
            )
            .child(div().flex_1())
            .child(
                SegmentedControl::new("appearance", "Appearance")
                    .options([
                        SegmentOption::new("dark", "Dark"),
                        SegmentOption::new("light", "Light"),
                    ])
                    .selected(appearance.as_setting())
                    .on_change(cx.listener(|this, key: &SharedString, window, cx| {
                        this.set_theme(key, window, cx)
                    })),
            )
            .child(
                Switch::new("reduced-motion")
                    .label("Reduce motion")
                    .checked(cx.reduced_motion())
                    .on_click(|checked, _, cx| set_reduced_motion(*checked, cx)),
            );

        // Tokens.
        let surfaces = [
            ("surface", k.surface),
            ("secondary", k.surface_secondary),
            ("tertiary", k.surface_tertiary),
            ("elevated", k.surface_elevated),
            ("rail", k.surface_rail),
            ("border", k.surface_border),
            ("menu", k.surface_menu),
            ("stage", k.surface_stage),
            ("canvas", k.surface_canvas),
            ("glass", k.glass),
        ];
        let semantics = [
            ("accent", k.control_accent),
            ("on accent", k.on_accent),
            ("live", k.live),
            ("destructive", k.destructive),
            ("tint live", k.tints.live),
            ("tint red", k.tints.red),
            ("tint rose", k.tints.rose),
            ("range track", k.range_track),
            ("selected", k.row_selected()),
        ];
        let tokens = section("tokens", "Tokens", &k)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(surfaces.map(|(n, c)| swatch(n, c, &k))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(semantics.map(|(n, c)| swatch(n, c, &k))),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(Hue::ALL.map(|h| swatch(h.name(), k.hue(h), &k))),
            )
            .child(
                h_flex().flex_wrap().gap_2().children(
                    [
                        Step::S50,
                        Step::S100,
                        Step::S200,
                        Step::S300,
                        Step::S400,
                        Step::S500,
                        Step::S600,
                        Step::S700,
                        Step::S800,
                        Step::S900,
                        Step::S950,
                    ]
                    .into_iter()
                    .zip([
                        "50", "100", "200", "300", "400", "500", "600", "700", "800", "900", "950",
                    ])
                    .map(|(s, n)| swatch(n, k.accent(s), &k)),
                ),
            )
            .child(
                h_flex().gap_4().children(
                    [
                        (TextStep::T200, "fg-200"),
                        (TextStep::T300, "fg-300"),
                        (TextStep::T400, "fg-400"),
                        (TextStep::T500, "fg-500"),
                        (TextStep::T600, "fg-600"),
                    ]
                    .map(|(s, n)| {
                        div()
                            .text_size(text::SM.size)
                            .text_color(k.text(s))
                            .child(n)
                    }),
                ),
            );

        let type_scale = section("type", "Type", &k).children(
            [
                (text::XL3, "3xl 30 · Welcome to Kairo"),
                (text::XL2, "2xl 24 · Screens"),
                (text::XL, "xl 20 · Page header"),
                (text::LG, "lg 17 · Section title"),
                (text::BASE, "base 15 · Body copy for the booth. Ẹ ọ ṣ à é"),
                (text::SM, "sm 13 · Controls and rows"),
                (text::XS, "xs 12 · Captions"),
                (text::LABEL, "11 · Field labels"),
                (text::MICRO, "10 · Rail labels and counts"),
            ]
            .map(|(t, s)| {
                div()
                    .text_size(t.size)
                    .line_height(t.line_height)
                    .text_color(k.text_primary)
                    .child(s)
            }),
        );

        let icons = section("icons", "Icons", &k).child(h_flex().flex_wrap().gap_2().children(
            KairoIcon::ALL.iter().map(|icon| {
                h_flex()
                    .gap_1()
                    .p_1()
                    .rounded(radius::sm())
                    .text_color(k.text(TextStep::T300))
                    .child(Icon::new(*icon).size_4())
                    .child(Icon::new(icon.fill()).size_4())
            }),
        ));

        let buttons = section("buttons", "Buttons", &k)
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(Button::new("btn-primary").primary().label("Go live"))
                    .child(Button::new("btn-outline").outline().label("Outline"))
                    .child(Button::new("btn-secondary").secondary().label("Secondary"))
                    .child(Button::new("btn-ghost").ghost().label("Ghost"))
                    .child(Button::new("btn-danger").danger().label("Clear output"))
                    .child(Button::new("btn-link").link().label("Open Screens"))
                    .child(
                        Button::new("btn-disabled")
                            .primary()
                            .label("Disabled")
                            .disabled(true),
                    )
                    .child(
                        Button::new("btn-loading")
                            .secondary()
                            .label("Saving")
                            .loading(true),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("btn-xs")
                            .secondary()
                            .label("Extra small")
                            .xsmall(),
                    )
                    .child(Button::new("btn-sm").secondary().label("Small").small())
                    .child(Button::new("btn-md").secondary().label("Default"))
                    .child(Button::new("btn-lg").secondary().label("Large").large())
                    .child(
                        Button::new("btn-icon")
                            .ghost()
                            .icon(Icon::new(KairoIcon::Settings))
                            .tooltip("Settings"),
                    )
                    .child(
                        Button::new("btn-icon-sm")
                            .ghost()
                            .small()
                            .icon(Icon::new(KairoIcon::Plus))
                            .tooltip("Add"),
                    ),
            );

        let inputs =
            section("inputs", "Fields", &k)
                .child(
                    h_flex()
                        .gap_4()
                        .items_start()
                        .child(
                            v_flex()
                                .w_64()
                                .gap_1()
                                .child(label("Search", &k))
                                .child(Input::new(&self.name).kairo_size().id("gallery-search")),
                        )
                        .child(
                            v_flex().w_64().gap_1().child(label("Password", &k)).child(
                                Input::new(&self.password)
                                    .kairo_size()
                                    .id("gallery-password")
                                    .mask_toggle(),
                            ),
                        )
                        .child(
                            v_flex().w_48().gap_1().child(label("Disabled", &k)).child(
                                Input::new(&self.disabled_input).kairo_size().disabled(true),
                            ),
                        )
                        .child(div().w_40().child(self.number.clone())),
                )
                .child(
                    h_flex()
                        .gap_4()
                        .child(
                            v_flex()
                                .w_48()
                                .gap_1()
                                .child(label("Output", &k))
                                .child(Select::new(&self.select)),
                        )
                        .child(
                            v_flex()
                                .w_48()
                                .gap_1()
                                .child(label("Opacity", &k))
                                .child(Slider::new(&self.slider).accessibility_label("Opacity")),
                        )
                        .child(
                            Switch::new("gallery-switch")
                                .label("Show reference")
                                .checked(self.switch_on)
                                .on_click(cx.listener(|this, checked: &bool, _, cx| {
                                    this.switch_on = *checked;
                                    cx.notify();
                                })),
                        )
                        .child(self.zoom.clone()),
                )
                .child(
                    h_flex()
                        .gap_4()
                        .child(
                            SegmentedControl::new("gallery-segment", "Content")
                                .options([
                                    SegmentOption::new("scripture", "Scripture"),
                                    SegmentOption::new("lyrics", "Lyrics"),
                                    SegmentOption::new("media", "Media"),
                                ])
                                .selected(self.segment.clone())
                                .on_change(cx.listener(|this, key: &SharedString, _, cx| {
                                    this.segment = key.clone();
                                    cx.notify();
                                })),
                        )
                        .child(
                            SegmentedControl::new("gallery-tabs", "Booth tools")
                                .tabs()
                                .fit()
                                .options([
                                    SegmentOption::new("verses", "Verses"),
                                    SegmentOption::new("songs", "Songs"),
                                    SegmentOption::new("timers", "Timers"),
                                    SegmentOption::new("messages", "Messages"),
                                ])
                                .selected(self.tab.clone())
                                .on_change(cx.listener(|this, key: &SharedString, _, cx| {
                                    this.tab = key.clone();
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(label("Confirmation code", &k))
                        .child(self.code.clone()),
                );

        let pref =
            section("pref", "Settings groups", &k).child(
                h_flex()
                    .gap_4()
                    .items_start()
                    .child(div().w_80().child(PrefGroup::new().rows(
                        ["Show reference", "Fit to box", "Background video"].map(|name| {
                            h_flex()
                                .justify_between()
                                .px_3p5()
                                .py_2p5()
                                .text_size(text::SM.size)
                                .text_color(k.text_primary)
                                .child(name)
                                .child(
                                    Switch::new(SharedString::from(format!("pref-{name}")))
                                        .checked(true)
                                        .accessibility_label(name),
                                )
                        }),
                    )))
                    .child(
                        div()
                            .w_80()
                            .p_3()
                            .bg(k.surface)
                            .rounded(radius::md())
                            .child(PrefGroup::new().dim(true).rows(
                                ["NDI output", "Recording"].map(|name| {
                                    h_flex()
                                        .justify_between()
                                        .px_3p5()
                                        .py_2p5()
                                        .text_size(text::SM.size)
                                        .text_color(k.text_primary)
                                        .child(name)
                                        .child(
                                            Switch::new(SharedString::from(format!("dim-{name}")))
                                                .accessibility_label(name),
                                        )
                                }),
                            )),
                    ),
            );

        // Marquee grid.
        let selected = self.selected.clone();
        let grid = div()
            .id("marquee-grid")
            .relative()
            .w(px(4.0 * 120.0 + 3.0 * 8.0 + 24.0))
            .p_3()
            .rounded(radius::md())
            .bg(k.surface)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    let on_item = this
                        .grid_bounds
                        .iter()
                        .any(|(_, b)| b.contains(&event.position));
                    this.marquee = Marquee::press(
                        event.position,
                        px(0.),
                        event.modifiers,
                        PressTarget {
                            item: on_item,
                            ..Default::default()
                        },
                    );
                    if this.marquee.is_none()
                        && let Some((id, _)) = this
                            .grid_bounds
                            .iter()
                            .find(|(_, b)| b.contains(&event.position))
                    {
                        this.selected = vec![id.clone()];
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                let Some(marquee) = this.marquee.as_mut() else {
                    return;
                };
                if event.pressed_button != Some(MouseButton::Left) {
                    this.marquee = None;
                    cx.notify();
                    return;
                }
                let step = marquee.move_to(event.position);
                let band = marquee.band(px(0.));
                match step {
                    MarqueeStep::Pending => return,
                    MarqueeStep::Began { additive } => {
                        let mut next = if additive {
                            this.selected.clone()
                        } else {
                            Vec::new()
                        };
                        for id in this.grid_hits(band) {
                            if !next.contains(&id) {
                                next.push(id);
                            }
                        }
                        this.selected = next;
                    }
                    MarqueeStep::Moved => {
                        let additive = marquee.is_additive();
                        let hits = this.grid_hits(band);
                        if additive {
                            for id in hits {
                                if !this.selected.contains(&id) {
                                    this.selected.push(id);
                                }
                            }
                        } else {
                            this.selected = hits;
                        }
                    }
                }
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(marquee) = this.marquee.take() {
                        let _ = marquee.release();
                    }
                    cx.notify();
                }),
            )
            .child(h_flex().flex_wrap().gap_2().children(GRID_IDS.map(|id| {
                let picked = selected.iter().any(|s| s == id);
                let entity = cx.entity().downgrade();
                let key: SharedString = id.into();
                div()
                    .id(SharedString::from(format!("card-{id}")))
                    .role(Role::ListItem)
                    .aria_label(format!("Card {}", id.to_uppercase()))
                    .aria_selected(picked)
                    .w(px(120.))
                    .h(px(68.))
                    .rounded(radius::base())
                    .bg(k.surface_tertiary)
                    .border_2()
                    .border_color(if picked {
                        k.control_accent
                    } else {
                        k.surface_tertiary
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(k.text_primary)
                    .child(id.to_uppercase())
                    .child(
                        gpui_kit::canvas(
                            move |bounds, _, cx| {
                                let _ = entity.update(cx, |this, _| {
                                    if let Some(entry) =
                                        this.grid_bounds.iter_mut().find(|(i, _)| *i == key)
                                    {
                                        entry.1 = bounds;
                                    } else {
                                        this.grid_bounds.push((key.clone(), bounds));
                                    }
                                });
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
            })))
            .when_some(self.marquee.filter(Marquee::is_active), |this, m| {
                let band = m.band(px(0.));
                this.child(gpui_kit::deferred(
                    gpui_kit::anchored()
                        .position(point(px(0.), px(0.)))
                        .child(MarqueeBand::new(band)),
                ))
            });

        let selection_count = self.selected.len();
        let selection = section("selection", "Selection", &k).child(grid).child(
            SelectionBar::new(selection_count, "card")
                .hint("Drag across cards to select")
                .action(SelectionAction::new("sel-live", "Add to playlist").icon(KairoIcon::Plus))
                .action(SelectionAction::new("sel-disabled", "Export").disabled(true))
                .action(
                    SelectionAction::new("sel-delete", "Delete")
                        .icon(KairoIcon::Trash2)
                        .danger(true),
                )
                .on_clear(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.selected.clear();
                    cx.notify();
                })),
        );

        let navigation = section("navigation", "Navigation", &k).child(
            h_flex()
                .gap_6()
                .items_start()
                .child(
                    v_flex()
                        .w(crate::tokens::metrics::rail_width())
                        .gap_1()
                        .p_1()
                        .bg(k.surface_rail)
                        .rounded(radius::md())
                        .child(
                            RailItem::new(
                                "rail-operator",
                                KairoIcon::Radio,
                                "Operator",
                                Hue::Green,
                            )
                            .active(true),
                        )
                        .child(RailItem::new(
                            "rail-scripture",
                            KairoIcon::Bible,
                            "Scripture",
                            Hue::Blue,
                        ))
                        .child(RailItem::new(
                            "rail-lyrics",
                            KairoIcon::Music2,
                            "Lyrics",
                            Hue::Violet,
                        ))
                        .child(
                            RailItem::new(
                                "rail-theme",
                                KairoIcon::Palette,
                                "Theme Editor",
                                Hue::Pink,
                            )
                            .open(true),
                        ),
                )
                .child(
                    div()
                        .w_56()
                        .p_1()
                        .bg(k.sidebar_list)
                        .rounded(radius::md())
                        .child(self.library.clone()),
                )
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            TrafficLights::new("lights-zoom")
                                .on_close(|_, _, _| {})
                                .on_zoom(|_, _, _| {}),
                        )
                        .child(TrafficLights::new("lights-plain").on_close(|_, _, _| {}))
                        .child(
                            h_flex()
                                .id("row-with-menu")
                                .px_2()
                                .py_1p5()
                                .rounded(radius::base())
                                .bg(k.surface_tertiary)
                                .text_size(text::XS.size)
                                .text_color(k.text_primary)
                                .child("Right-click this row")
                                .context_menu(|menu, _, _| {
                                    menu.item(
                                        PopupMenuItem::new("Rename")
                                            .icon(Icon::new(KairoIcon::Pencil)),
                                    )
                                    .item(
                                        PopupMenuItem::new("Duplicate")
                                            .icon(Icon::new(KairoIcon::Copy)),
                                    )
                                    .separator()
                                    .item(
                                        PopupMenuItem::new("Delete")
                                            .icon(Icon::new(KairoIcon::Trash2)),
                                    )
                                }),
                        )
                        .child(DragPill::new("Amazing Grace", "♪"))
                        .child(KairoMark::new(MarkSize::Medium).tile())
                        .child(KairoLockup::new(MarkSize::Small).tagline(true)),
                ),
        );

        let answer = self.last_answer.clone();
        let dialogs_section = section("dialogs", "Dialogs", &k)
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("ask-discard")
                            .secondary()
                            .label("Discard changes")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.ask(messages::lyrics_discard_changes(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("ask-trash")
                            .secondary()
                            .label("Move track to Trash")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.ask(messages::tracks_move_to_trash("Intro.mp3"), window, cx)
                            })),
                    )
                    .child(
                        Button::new("ask-download")
                            .secondary()
                            .label("Download translation")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.ask(
                                    messages::settings_download_translation(
                                        "NKJV",
                                        "New King James Version",
                                        Some("4 MB"),
                                    ),
                                    window,
                                    cx,
                                )
                            })),
                    )
                    .child(
                        Button::new("ask-media")
                            .secondary()
                            .label("Delete backgrounds")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.ask(messages::media_delete_items(3), window, cx)
                            })),
                    )
                    .child(
                        Button::new("show-alert")
                            .secondary()
                            .label("Display taken")
                            .on_click(|_, window, cx| {
                                dialogs::alert(
                                    messages::screens_display_taken("Audience"),
                                    window,
                                    cx,
                                )
                            }),
                    )
                    .child(
                        Button::new("open-movable")
                            .secondary()
                            .label("Open Settings window")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_dialog(window, cx)),
                            ),
                    ),
            )
            .child(
                div()
                    .id("confirm-answer")
                    .role(Role::Status)
                    .aria_label(answer.clone())
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child(answer),
            )
            .child(
                h_flex()
                    .gap_3()
                    .p_4()
                    .rounded(radius::md())
                    .bg(k.control_accent.opacity(0.35))
                    .child(
                        FrostedPanel::new(Frost::Panel)
                            .child(div().p_4().text_color(k.text_primary).child(".glass")),
                    )
                    .child(
                        FrostedPanel::new(Frost::Modal)
                            .child(div().p_4().text_color(k.text_primary).child(".modal-glass")),
                    ),
            );

        let run = self.motion_run;
        let motion_section = section("motion", "Motion", &k)
            .child(
                Button::new("motion-replay")
                    .self_start()
                    .secondary()
                    .small()
                    .label("Replay")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.motion_run += 1;
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .children(motion::ALL.iter().map(|keyframes| {
                        let tile = div()
                            .w_32()
                            .h_10()
                            .rounded(radius::base())
                            .bg(k.surface_elevated)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(text::MICRO.size)
                            .text_color(k.text_primary)
                            .child(keyframes.name);
                        // Each tile moves inside its own clipped box.
                        div().w_32().h_10().overflow_hidden().child(motion::animate(
                            tile,
                            ElementId::Name(format!("{}-{run}", keyframes.name).into()),
                            *keyframes,
                            cx,
                        ))
                    })),
            );

        let dialog_overlay = self.dialog.clone().map(|dialog| {
            let content = dialog.update(cx, |dialog, cx| {
                let handle = dialog
                    .drag_handle("settings-titlebar", cx)
                    .h_10()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(TrafficLights::new("settings-lights").on_close({
                        let dialog = cx.entity();
                        move |_, _, cx| {
                            dialog.update(cx, |_, cx| cx.emit(MovableDialogEvent::Dismiss))
                        }
                    }))
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                            .child("Settings"),
                    );
                let body = v_flex()
                    .size_full()
                    .child(FrostedPanel::new(Frost::Panel).rounded(false).child(handle))
                    .child(
                        v_flex()
                            .items_start()
                            .p_4()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T300))
                            .child("Drag the title bar. Escape closes.")
                            .child(
                                Button::new("settings-done")
                                    .primary()
                                    .label("Done")
                                    .mt_3()
                                    .on_click({
                                        let dialog = cx.entity();
                                        move |_, _, cx| {
                                            dialog.update(cx, |_, cx| {
                                                cx.emit(MovableDialogEvent::Dismiss)
                                            })
                                        }
                                    }),
                            ),
                    );
                dialog.frame(body, window, cx)
            });
            gpui_kit::deferred(content).with_priority(1)
        });

        div()
            .id("gallery")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .bg(k.surface)
            .text_color(k.text_primary)
            .font_family(text::FAMILY)
            .text_size(text::BASE.size)
            .child(
                v_flex()
                    .id("gallery-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(header)
                    .child(
                        v_flex()
                            .gap_4()
                            .px_4()
                            .pb_8()
                            .child(buttons)
                            .child(inputs)
                            .child(pref)
                            .child(selection)
                            .child(navigation)
                            .child(dialogs_section)
                            .child(tokens)
                            .child(type_scale)
                            .child(icons)
                            .child(motion_section),
                    ),
            )
            .children(dialog_overlay)
    }
}

/// Opens the gallery window. The appearance is whatever [`crate::init`] set.
pub fn open(cx: &mut App) -> gpui_kit::Result<(gpui_kit::AnyWindowHandle, Entity<Gallery>)> {
    let options = gpui_kit::WindowOptions {
        window_bounds: Some(gpui_kit::WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1280.), px(900.)),
            cx,
        ))),
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: Some("Kairo design system".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    gpui_kit::open_window(options, cx, |window, cx| {
        let view = cx.new(|cx| Gallery::new(window, cx));
        view.focus_handle(cx).focus(window, cx);
        view
    })
}
