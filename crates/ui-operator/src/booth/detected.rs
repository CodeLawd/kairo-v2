//! Detected verses (`Operator.tsx`, centre column): the recovery prompt,
//! notices, the Automation switch, and one row per passage, newest on top,
//! each dropping in as it arrives (`detect-card-in`).
//!
//! The latency trace closes here: after a frame that paints a new card, the
//! booth stamps `markRendered` from a frame callback.

use kairo_ui_kit::components::PopoverTriggerExt;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, MouseButton,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    div, prelude::FluentBuilder, px,
};
use kairo_domain::scripture::ScriptureSuggestion;
use kairo_domain::scripture::live_progress::ScriptureSuggestionGroup;
use kairo_domain::scripture::query::Direction;
use kairo_domain::scripture::small::spoken_verse_text;
use kairo_ui_kit::coach::coach_anchor;
use kairo_ui_kit::motion::{self, DETECT_CARD_IN, DETECT_STATE_IN};
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::text::{DetectionState, detected_chip_label, detection_source, detection_state};
use super::widgets::icon_button;
use super::{Booth, KeyModifiers};
use crate::scroll::{Edge, PinnedScroll};

/// Element ids, for tests.
pub mod ids {
    pub const AUTOMATION: &str = "detected-automation";
    pub const CLEAR: &str = "detected-clear";
    pub const EMPTY: &str = "detected-empty";
    pub const RESTORE: &str = "recovery-restore";
    pub const DISCARD: &str = "recovery-discard";
    pub const ERROR: &str = "detected-error";
    pub const FALLBACK: &str = "detected-fallback";
    /// `detected-row-<group id>`.
    pub fn row(group: &str) -> String {
        format!("detected-row-{group}")
    }
    /// `detected-present-<suggestion id>`: the row's Present.
    pub fn present(id: &str) -> String {
        format!("detected-present-{id}")
    }
    /// `detected-text-<suggestion id>`: the verse text (presents it).
    pub fn verse_text(id: &str) -> String {
        format!("detected-text-{id}")
    }
    /// `detected-chip-<suggestion id>`.
    pub fn chip(id: &str) -> String {
        format!("detected-chip-{id}")
    }
    pub fn queue(group: &str) -> String {
        format!("detected-queue-{group}")
    }
    pub fn dismiss(group: &str) -> String {
        format!("detected-dismiss-{group}")
    }
    pub fn previous(group: &str) -> String {
        format!("detected-previous-{group}")
    }
    pub fn next(group: &str) -> String {
        format!("detected-next-{group}")
    }
    pub fn translation(group: &str) -> String {
        format!("detected-translation-{group}")
    }
}

pub struct DetectedPanel {
    booth: Entity<Booth>,
    scroll: PinnedScroll,
    /// Card ids already painted, so only new ones play the drop-in.
    painted: HashSet<String>,
    _subscriptions: Vec<Subscription>,
}

impl DetectedPanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let home = window.window_handle();
        let keys = booth.clone();
        // The operator's shortcuts fire anywhere in the window, as Electron's
        // `window.addEventListener('keydown')` did. Text fields, dialogs and
        // menus stand them down, and a key an action already used is left alone
        // (`defaultPrevented`).
        let shortcuts = cx.observe_keystrokes(move |_, event, window, cx| {
            if window.window_handle() != home
                || !window.is_window_active()
                || event.action.is_some()
            {
                return;
            }
            let stack = &event.context_stack;
            let typing = stack.iter().any(|c| c.contains("Input"));
            let modal = stack.iter().any(|c| {
                c.contains("MovableDialog") || c.contains("PopupMenu") || c.contains("Popover")
            });
            let m = event.keystroke.modifiers;
            let modifiers = KeyModifiers {
                meta: m.platform,
                ctrl: m.control,
                alt: m.alt,
                shift: m.shift,
                repeat: false,
            };
            let key = event.keystroke.key.clone();
            keys.update(cx, |b, cx| {
                b.handle_key(&key, modifiers, typing || modal, cx)
            });
        });
        let subscriptions = vec![cx.observe(&booth, |_, _, cx| cx.notify()), shortcuts];
        Self {
            booth,
            scroll: PinnedScroll::new(Edge::Top),
            painted: HashSet::new(),
            _subscriptions: subscriptions,
        }
    }

    fn banners(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let mut out = Vec::new();
        if booth
            .resilience
            .as_ref()
            .is_some_and(|r| r.recovery_session_available)
        {
            out.push(
                h_flex()
                    .id("recovery-banner")
                    .test_support()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .px_6()
                    .py_3()
                    .bg(k.tints.teal)
                    .text_size(text::XS.size)
                    .text_color(k.accent(Step::S300))
                    .child(
                        h_flex()
                            .min_w_0()
                            .items_center()
                            .gap_2p5()
                            .child(Icon::new(KairoIcon::AlertTriangle).size(px(15.)).text_color(k.accent(Step::S400)))
                            .child(
                                div()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::BOLD)
                                            .child("Active Recovery State Detected."),
                                    )
                                    .child(div().text_color(k.text(TextStep::T400)).child(
                                        "Would you like to restore your previous session transcription, settings, and queue?",
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap_2()
                            .child(
                                Button::new(ids::DISCARD)
                                    .outline()
                                    .small()
                                    .label("Discard")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.booth.update(cx, |b, cx| b.discard_session(cx))
                                    })),
                            )
                            .child(
                                super::widgets::primary_button(ids::RESTORE, "Restore Session", cx)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.booth.update(cx, |b, cx| b.restore_session(cx))
                                    })),
                            ),
                    )
                    .into_any_element(),
            );
        }
        if !booth.service_error.is_empty() {
            out.push(
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .role(gpui_kit::Role::Alert)
                    .px_4()
                    .py_2()
                    .text_size(text::XS.size)
                    .text_color(k.danger_text)
                    .child(booth.service_error.clone())
                    .into_any_element(),
            );
        }
        if let Some(r) = booth
            .resilience
            .as_ref()
            .filter(|r| r.claude_fallback_active)
        {
            out.push(
                h_flex()
                    .id(ids::FALLBACK)
                    .test_support()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_5()
                    .py_2()
                    .bg(k.tints.amber)
                    .text_size(text::LABEL.size)
                    .text_color(k.warning_text)
                    .child(
                        div().min_w_0().truncate().child(
                            r.detector_fallback_reason
                                .as_ref()
                                .map(|reason| reason.message.clone())
                                .unwrap_or_else(|| {
                                    "AI detection unavailable — local matching active.".into()
                                }),
                        ),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_family(text::MONO_FAMILY)
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T500))
                            .child("Local matching"),
                    )
                    .into_any_element(),
            );
        }
        out
    }

    fn header(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let groups = booth.groups().len();
        let auto = booth.auto_mode;
        let any = !booth.suggestions.is_empty();
        h_flex()
            .h_9()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .child(
                h_flex()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T400))
                            .child("Detected"),
                    )
                    .when(groups > 0, |this| {
                        this.child(
                            div()
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T600))
                                .child(groups.to_string()),
                        )
                    }),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_3()
                    .child(
                        h_flex()
                            .id("automation-label")
                            .relative()
                            .items_center()
                            .gap_2()
                            .text_size(text::LABEL.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T400))
                            .tooltip(super::widgets::tip(
                                "Show confident single verses automatically · Ctrl+A".to_owned(),
                            ))
                            .child(coach_anchor("automation"))
                            .child("Automation")
                            .child(
                                Switch::new(ids::AUTOMATION)
                                    .checked(auto)
                                    .accessibility_label("Toggle automation")
                                    .on_change(cx.listener(|this, _: &bool, _, cx| {
                                        this.booth.update(cx, |b, cx| b.toggle_auto_mode(cx))
                                    })),
                            ),
                    )
                    .when(any, |this| {
                        this.child(
                            Button::new(ids::CLEAR)
                                .ghost()
                                .xsmall()
                                .label("Clear")
                                .text_color(k.text(TextStep::T600))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.booth.update(cx, |b, cx| b.clear_detections(cx))
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn row(
        &mut self,
        group: &ScriptureSuggestionGroup,
        projected: &HashSet<String>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let follow = booth.follow.clone();
        let selected = booth.selected.clone();
        let retranslating = booth.retranslating.as_deref() == Some(group.id.as_str());
        let adjacent = booth.adjacent_loading.clone();
        let translations: Vec<_> = booth
            .translations
            .iter()
            .filter(|o| o.available)
            .cloned()
            .collect();
        let group_live = group.suggestions.iter().any(|s| projected.contains(&s.id));
        let following = group_live && follow.as_ref().is_some_and(|f| f.passage_id == group.id);
        let flags = |item: &ScriptureSuggestion| {
            let index = item.passage_index;
            let reading = following
                && follow
                    .as_ref()
                    .is_some_and(|f| Some(f.current_index) == index);
            let up_next = following
                && follow
                    .as_ref()
                    .is_some_and(|f| Some(f.current_index + 1.0) == index);
            let live = projected.contains(&item.id);
            (live, reading, detection_state(live, reading, up_next))
        };
        let shown = group
            .suggestions
            .iter()
            .find(|s| Some(&s.id) == selected.as_ref())
            .or_else(|| group.suggestions.iter().find(|s| flags(s).0))
            .or_else(|| group.suggestions.iter().find(|s| flags(s).1))
            .or_else(|| group.suggestions.first())?
            .clone();
        let (shown_live, _, state) = flags(&shown);
        let passage = group.suggestions.len() > 1;
        let source = detection_source(&shown);
        let gid = group.id.clone();
        let fresh = self.painted.insert(group.id.clone());
        let fg = |c| if group_live { k.white } else { c };

        let translation_button = {
            let booth = self.booth.downgrade();
            let cards = group.suggestions.clone();
            let gid = group.id.clone();
            let current = shown.translation.clone();
            Button::new(SharedString::from(ids::translation(&group.id)))
                .ghost()
                .xsmall()
                .when(retranslating, |b| b.loading(true))
                .label(shown.translation.clone())
                .dropdown_caret(true)
                .text_color(fg(k.text(TextStep::T400)))
                .disabled(retranslating)
                .accessibility_label(format!(
                    "{} in {}. Change translation",
                    group.reference, shown.translation
                ))
                .tooltip("Show this passage in another translation")
                .popover_trigger()
                .dropdown_menu(move |mut menu, _, cx| {
                    // Opening refreshes the list, so newly licensed Bibles show.
                    let _ = booth.update(cx, |b, cx| b.refresh_translations(cx));
                    for option in &translations {
                        let booth = booth.clone();
                        let cards = cards.clone();
                        let gid = gid.clone();
                        let id = option.id.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!("{}  {}", option.id, option.name))
                                .disabled(option.id == current)
                                .on_click(move |_, _, cx| {
                                    let _ = booth.update(cx, |b, cx| {
                                        b.retranslate(gid.clone(), cards.clone(), id.clone(), cx)
                                    });
                                }),
                        );
                    }
                    menu
                })
        };

        let state_chip: Option<AnyElement> = if group_live {
            Some(
                h_flex()
                    .items_center()
                    .gap_1()
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(k.white)
                    .child(div().size(px(6.)).rounded_full().bg(k.white))
                    .child("On screen")
                    .into_any_element(),
            )
        } else if state == Some(DetectionState::Reading) {
            Some(
                div()
                    .rounded_full()
                    .px_1p5()
                    .text_size(text::MICRO.size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .bg(k.accent(Step::S500))
                    .text_color(k.on_accent)
                    .child("Reading")
                    .into_any_element(),
            )
        } else {
            source.map(|s| {
                div()
                    .truncate()
                    .text_size(text::MICRO.size)
                    .text_color(k.text(TextStep::T500))
                    .child(s)
                    .into_any_element()
            })
        };
        let state_key = format!("{gid}-state-{state:?}");
        let state_chip = state_chip.map(|chip| {
            motion::animate(
                h_flex().flex_shrink_0().items_center().child(chip),
                SharedString::from(state_key),
                DETECT_STATE_IN,
                cx,
            )
        });

        let loading = |dir: &str| adjacent.as_deref() == Some(&format!("{gid}:{dir}"));
        let hover_actions = h_flex()
            .flex_shrink_0()
            .items_center()
            .gap_0p5()
            .opacity(0.)
            .group_hover("detected-row", |s| s.opacity(1.))
            .when(!shown_live, |this| {
                let id = shown.id.clone();
                this.child(
                    super::widgets::primary_button(
                        SharedString::from(ids::present(&shown.id)),
                        "Present",
                        cx,
                    )
                    .xsmall()
                    .mr_1()
                    .icon(Icon::new(KairoIcon::Play).size(px(10.)))
                    .accessibility_label(format!(
                        "Present {} {}",
                        shown.reference, shown.translation
                    ))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.booth.update(cx, |b, cx| b.approve(&id, cx))
                    })),
                )
            })
            .child({
                let gid = gid.clone();
                icon_button(
                    SharedString::from(ids::previous(&gid)),
                    if loading("previous") {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::ChevronLeft
                    },
                    format!("Preload verse before {}", group.reference),
                    cx,
                )
                .disabled(adjacent.is_some())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let gid = gid.clone();
                    this.booth
                        .update(cx, |b, cx| b.load_adjacent(&gid, Direction::Previous, cx))
                }))
            })
            .child({
                let gid = gid.clone();
                icon_button(
                    SharedString::from(ids::next(&gid)),
                    if loading("next") {
                        KairoIcon::Loader
                    } else {
                        KairoIcon::ChevronRight
                    },
                    format!("Preload verse after {}", group.reference),
                    cx,
                )
                .disabled(adjacent.is_some())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let gid = gid.clone();
                    this.booth
                        .update(cx, |b, cx| b.load_adjacent(&gid, Direction::Next, cx))
                }))
            })
            .child({
                let entry = shown.clone();
                icon_button(
                    SharedString::from(ids::queue(&gid)),
                    KairoIcon::Plus,
                    format!("Add {} to queue", shown.reference),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let entry = entry.clone();
                    this.booth.update(cx, |b, cx| b.add_to_queue(&entry, cx))
                }))
            })
            .child({
                let all: Vec<String> = group.suggestions.iter().map(|s| s.id.clone()).collect();
                icon_button(
                    SharedString::from(ids::dismiss(&gid)),
                    KairoIcon::X,
                    format!("Dismiss {}", group.reference),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let all = all.clone();
                    this.booth.update(cx, |b, cx| {
                        for id in &all {
                            b.dismiss(id, cx);
                        }
                    })
                }))
            });

        let chips = passage.then(|| {
            let first = group.suggestions.first().map(|s| s.reference.clone());
            h_flex()
                .id(SharedString::from(format!("detected-chips-{}", group.id)))
                .mt_1p5()
                .flex_wrap()
                .gap_1()
                .aria_label(format!("Verses of {}", group.reference))
                .children(group.suggestions.iter().map(|item| {
                    let (live, _, _) = flags(item);
                    let picked = item.id == shown.id;
                    let id = item.id.clone();
                    let label = detected_chip_label(&item.reference, first.as_deref());
                    div()
                        .id(SharedString::from(ids::chip(&item.id)))
                        .test_support()
                        .role(gpui_kit::Role::Button)
                        .aria_label(format!(
                            "Present {}{}",
                            item.reference,
                            if live { ", on screen" } else { "" }
                        ))
                        .tooltip(super::widgets::tip(format!("Present {}", item.reference)))
                        .h_6()
                        .min_w_6()
                        .px_1p5()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .cursor_pointer()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::MEDIUM)
                        .map(|s| {
                            if live {
                                s.bg(k.white).text_color(k.live)
                            } else if picked {
                                s.bg(k.accent(Step::S500)).text_color(k.on_accent)
                            } else {
                                s.text_color(fg(k.text(TextStep::T400)))
                                    .hover(|s| s.bg(k.surface_border).text_color(k.text_primary))
                            }
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.booth.update(cx, |b, _| b.cards_in_control = true)
                            }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = id.clone();
                            this.booth.update(cx, |b, cx| b.approve(&id, cx))
                        }))
                        .child(label)
                }))
        });

        let text_id = shown.id.clone();
        let verse_text = shown
            .verses
            .first()
            .map(|v| spoken_verse_text(&v.text))
            .unwrap_or_else(|| "Text unavailable in this translation.".into());
        let card = v_flex()
            .id(SharedString::from(ids::row(&group.id)))
            .test_support()
            .group("detected-row")
            .relative()
            .w_full()
            .min_w_0()
            .rounded_lg()
            .px(px(14.))
            .py_3()
            .map(|s| {
                if group_live {
                    s.bg(k.live.opacity(0.7)).text_color(k.white)
                } else {
                    s.bg(k.surface_tertiary).hover(|s| s.bg(k.surface_elevated))
                }
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg(k.text_primary))
                            .child(shown.reference.clone()),
                    )
                    .child(translation_button)
                    .when_some(shown.requested_translation.clone(), |this, asked| {
                        this.child(
                            div()
                                .id("requested-translation")
                                .flex_shrink_0()
                                .text_size(text::LABEL.size)
                                .text_color(fg(k.text(TextStep::T500)))
                                .tooltip(super::widgets::tip(format!(
                                    "The preacher asked for {asked}, which isn't available here. Download it or add an API.Bible key in Settings → Scripture."
                                )))
                                .child(format!("{asked} not available")),
                        )
                    })
                    .children(state_chip)
                    .child(div().flex_1())
                    .child(hover_actions),
            )
            .children(chips)
            .child(
                div()
                    .id(SharedString::from(ids::verse_text(&shown.id)))
                    .test_support()
                    .role(gpui_kit::Role::Button)
                    .aria_label(format!("Present {}", shown.reference))
                    .tooltip(super::widgets::tip(format!("Present {}", shown.reference)))
                    .mt_1()
                    .w_full()
                    .cursor_pointer()
                    .text_size(text::SM.size)
                    .line_clamp(2)
                    .text_color(fg(k.text(TextStep::T400)))
                    .hover(|s| s.text_color(fg(k.text(TextStep::T200))))
                    .when(!passage, |s| {
                        s.on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                            this.booth.update(cx, |b, _| b.cards_in_control = true)
                        }))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = text_id.clone();
                        this.booth.update(cx, |b, cx| b.approve(&id, cx))
                    }))
                    .child(verse_text),
            );
        Some(if fresh {
            motion::animate(
                card,
                SharedString::from(format!("card-in-{gid}")),
                DETECT_CARD_IN,
                cx,
            )
        } else {
            card.into_any_element()
        })
    }
}

impl Render for DetectedPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let (groups, projected, empty, transcribing) = {
            let booth = self.booth.read(cx);
            (
                booth.visible_groups(),
                booth.projected(),
                booth.suggestions.is_empty(),
                booth.transcribing,
            )
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for g in &groups {
            for s in &g.suggestions {
                (&s.id, &s.reference).hash(&mut hasher);
            }
        }
        self.scroll.follow(hasher.finish());
        // Forget cards that left, so a re-detected verse drops in again.
        let ids: HashSet<String> = groups.iter().map(|g| g.id.clone()).collect();
        self.painted.retain(|id| ids.contains(id));
        let rows: Vec<AnyElement> = groups
            .iter()
            .filter_map(|g| self.row(g, &projected, cx))
            .collect();
        // This frame paints the new cards: close the latency trace after it.
        self.booth.update(cx, |b, cx| b.flush_marks(window, cx));
        let banners = self.banners(cx);
        v_flex()
            .id("detected-panel")
            .relative()
            .size_full()
            .min_w_0()
            .bg(k.surface)
            // A press anywhere but a verse card hands the clicker back to selection.
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.booth.update(cx, |b, _| b.cards_in_control = false)
            }))
            .child(div().relative().child(coach_anchor("detected")))
            .children(banners)
            .child(self.header(cx))
            .child(
                v_flex()
                    .id("detected-scroll")
                    .track_scroll(self.scroll.handle())
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_4()
                    .py_3()
                    .child(v_flex().w_full().min_w_0().gap_2().children(rows))
                    .when(empty, |this| {
                        this.child(
                            v_flex()
                                .id(ids::EMPTY)
                                .test_support()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .text_center()
                                .child(
                                    div()
                                        .text_size(text::SM.size)
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(k.text(TextStep::T400))
                                        .child(if transcribing {
                                            "Listening for scripture"
                                        } else {
                                            "Nothing detected yet"
                                        }),
                                )
                                .child(
                                    div()
                                        .mt_1()
                                        .max_w(px(256.))
                                        .text_size(text::LABEL.size)
                                        .text_color(k.text(TextStep::T600))
                                        .child(if transcribing {
                                            "Quoted verses land here as slides. Click one to send it live."
                                        } else {
                                            "Start the speech pipeline to detect verses as they are read."
                                        }),
                                ),
                        )
                    }),
            )
    }
}
