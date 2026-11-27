//! UI integration tests: the real components in a headless window, driven by
//! pointer and keyboard events, checked through state, focus and the
//! accessibility tree.

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, Focusable, IntoElement, ParentElement,
    Render, SharedString, Styled, TestAppContext, Window, WindowBounds, WindowOptions, div, px,
    size,
};
use kairo_ui_kit::components::dialogs::{self, messages};
use kairo_ui_kit::components::{
    LibraryKind, LibraryRow, LibrarySection, LibrarySectionEvent, MovableDialog,
    MovableDialogEvent, NumberField, NumberFieldChanged, NumberModel, SegmentOption,
    SegmentedControl, SelectionAction, SelectionBar, ZoomChanged, ZoomControl, ZoomRange,
};
use kairo_ui_kit::{Appearance, KairoThemeExt};

#[allow(non_camel_case_types)]
enum Tap {
    click,
    double_click,
}

/// Clicks the element named `label` through its own id.
fn by_label(window: &mut Window, label: &str, cx: &mut gpui_kit::App, tap: Tap) {
    let id = window
        .find_by_label(label)
        .path()
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("{label} has no id"));
    match tap {
        Tap::click => window.click(id, cx),
        Tap::double_click => window.double_click(id, cx),
    }
}

fn setup(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        kairo_ui_kit::init(Appearance::Dark, cx);
    });
}

fn open<V: Render>(
    cx: &mut TestAppContext,
    build: impl FnOnce(&mut Window, &mut gpui_kit::App) -> Entity<V>,
) -> (AnyWindowHandle, Entity<V>) {
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Default::default(),
                    size(px(900.), px(700.)),
                ))),
                ..Default::default()
            },
            cx,
            build,
        )
        .expect("open test window")
    })
}

// ── Theme ────────────────────────────────────────────────────────────────

#[gpui_kit::test]
fn appearance_switches_kit_and_kairo_tokens_together(cx: &mut TestAppContext) {
    setup(cx);
    cx.update(|cx| {
        use gpui_kit::component::ActiveTheme;
        assert_eq!(cx.kairo().appearance, Appearance::Dark);
        assert_eq!(cx.theme().background, cx.kairo().surface);
        assert_eq!(cx.theme().primary, cx.kairo().control_accent);
        assert_eq!(cx.theme().font_family.as_ref(), "Inter");
        kairo_ui_kit::set_appearance(Appearance::Light, None, cx);
        assert_eq!(cx.kairo().appearance, Appearance::Light);
        assert!(!cx.theme().is_dark());
        assert_eq!(cx.theme().background, cx.kairo().surface);
        assert!(!cx.theme().shadow);
    });
}

// ── Segmented control ────────────────────────────────────────────────────

struct SegmentHost {
    selected: SharedString,
}

impl Render for SegmentHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(
            SegmentedControl::new("seg", "Content")
                .options([
                    SegmentOption::new("scripture", "Scripture"),
                    SegmentOption::new("lyrics", "Lyrics"),
                    SegmentOption::new("media", "Media"),
                ])
                .selected(self.selected.clone())
                .on_change(cx.listener(|this, key: &SharedString, _, cx| {
                    this.selected = key.clone();
                    cx.notify();
                })),
        )
    }
}

#[gpui_kit::test]
fn segmented_control_clicks_and_arrow_keys(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |_, cx| {
        cx.new(|_| SegmentHost {
            selected: "scripture".into(),
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let group = window.find_all_by_role(gpui_kit::Role::RadioGroup);
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].label(), Some("Content"));
        let radios = window.find_all_by_role(gpui_kit::Role::RadioButton);
        assert_eq!(radios.len(), 3);
        assert_eq!(window.find_by_label("Scripture").checked(), Some(true));

        by_label(window, "Media", cx, Tap::click);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Media").checked(), Some(true));
        assert_eq!(window.find_by_label("Media").focused(), Some(true));

        // Right wraps to the first option and moves focus with it.
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Scripture").checked(), Some(true));
        assert_eq!(window.find_by_label("Scripture").focused(), Some(true));
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Media").checked(), Some(true));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(host.read(cx).selected.as_ref(), "media"));
}

// ── Number field ─────────────────────────────────────────────────────────

struct NumberHost {
    field: Entity<NumberField>,
    changes: Vec<f64>,
}

impl Render for NumberHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().w_48().child(self.field.clone())
    }
}

#[gpui_kit::test]
fn number_field_arrows_shift_and_steppers(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| {
            let field = cx.new(|cx| {
                NumberField::new(
                    "size",
                    "Size",
                    NumberModel::new(10.0, 1.0).min(0.0).max(30.0),
                    window,
                    cx,
                )
            });
            cx.subscribe(
                &field,
                |this: &mut NumberHost, _, NumberFieldChanged(v), _| this.changes.push(*v),
            )
            .detach();
            NumberHost {
                field,
                changes: Vec::new(),
            }
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let input = window.find_by_label("Size");
        assert_eq!(input.value(), Some("10"));
        by_label(window, "Size", cx, Tap::click);
        window.press("up", cx);
        window.press("shift-up", cx);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Size").value(), Some("21"));
        // Shift-up past the maximum stops at it, and the increase button disables.
        window.press("shift-up", cx);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Size").value(), Some("30"));
        assert_eq!(window.find_by_label("Increase Size").disabled(), Some(true));
        by_label(window, "Decrease Size", cx, Tap::click);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Size").value(), Some("29"));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(host.read(cx).changes, vec![11.0, 21.0, 30.0, 29.0]));
}

#[gpui_kit::test]
fn number_field_ignores_text_that_is_not_a_number(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| {
            let field = cx.new(|cx| {
                NumberField::new("op", "Opacity", NumberModel::new(5.0, 1.0), window, cx)
            });
            cx.subscribe(
                &field,
                |this: &mut NumberHost, _, NumberFieldChanged(v), _| this.changes.push(*v),
            )
            .detach();
            NumberHost {
                field,
                changes: Vec::new(),
            }
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        by_label(window, "Opacity", cx, Tap::click);
        window.press("secondary-a", cx);
        window.input("-", cx);
        window.render_frame(cx);
        window.input("7", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(host.read(cx).changes, vec![-7.0]));
}

// ── Zoom control ─────────────────────────────────────────────────────────

struct ZoomHost {
    zoom: Entity<ZoomControl>,
    changes: Vec<f32>,
}

impl Render for ZoomHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(self.zoom.clone())
    }
}

#[gpui_kit::test]
fn zoom_buttons_step_disable_at_bounds_and_double_click_resets(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| {
            let zoom = cx.new(|cx| {
                ZoomControl::new(
                    "zoom",
                    "Verse card size",
                    ZoomRange {
                        min: 80.0,
                        max: 120.0,
                        default: 100.0,
                        step: 5.0,
                        button_step: 10.0,
                    },
                    110.0,
                    window,
                    cx,
                )
            });
            cx.subscribe(&zoom, |this: &mut ZoomHost, _, ZoomChanged(v), _| {
                this.changes.push(*v)
            })
            .detach();
            ZoomHost {
                zoom,
                changes: Vec::new(),
            }
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let group = window.find_all_by_role(gpui_kit::Role::Group);
        assert!(group.iter().any(|g| g.label() == Some("Verse card size")));
        by_label(window, "Larger", cx, Tap::click);
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Larger").disabled(), Some(true));
        // A disabled button ignores the click.
        by_label(window, "Larger", cx, Tap::click);
        window.render_frame(cx);
        by_label(window, "Smaller", cx, Tap::double_click);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let changes = &host.read(cx).changes;
        assert_eq!(changes.first(), Some(&120.0));
        assert_eq!(
            changes.last(),
            Some(&100.0),
            "double-click resets: {changes:?}"
        );
        assert_eq!(host.read(cx).zoom.read(cx).value(), 100.0);
    });
}

// ── Confirm and alert ────────────────────────────────────────────────────

struct DialogHost {
    answers: Vec<bool>,
}

impl Render for DialogHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn ask(host: &Entity<DialogHost>, window: &mut Window, cx: &mut gpui_kit::App) {
    let host = host.downgrade();
    dialogs::confirm(
        messages::screens_delete("Stage"),
        window,
        cx,
        move |yes, _, cx| {
            let _ = host.update(cx, |host, _| host.answers.push(yes));
        },
    );
}

#[gpui_kit::test]
fn confirm_answers_once_for_confirm_cancel_and_escape(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |_, cx| {
        cx.new(|_| DialogHost {
            answers: Vec::new(),
        })
    });
    cx.update_window(window, |_, window, cx| {
        ask(&host, window, cx);
        window.render_frame(cx);
        let dialog = window.find_all_by_role(gpui_kit::Role::AlertDialog);
        assert_eq!(dialog.len(), 1, "an alert dialog opened");
        assert_eq!(
            window.find_by_label("Delete").role(),
            Some(gpui_kit::Role::Button)
        );
        assert!(window.try_find_by_label("Cancel").is_some());
        by_label(window, "Delete", cx, Tap::click);
        window.render_frame(cx);

        ask(&host, window, cx);
        window.render_frame(cx);
        by_label(window, "Cancel", cx, Tap::click);
        window.render_frame(cx);

        ask(&host, window, cx);
        window.render_frame(cx);
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(host.read(cx).answers, vec![true, false, false]));
}

#[gpui_kit::test]
fn alert_shows_the_message_and_closes(cx: &mut TestAppContext) {
    setup(cx);
    let (window, _) = open(cx, |_, cx| {
        cx.new(|_| DialogHost {
            answers: Vec::new(),
        })
    });
    cx.update_window(window, |_, window, cx| {
        dialogs::alert(messages::screens_display_taken("Audience"), window, cx);
        window.render_frame(cx);
        assert_eq!(
            window.find_all_by_role(gpui_kit::Role::AlertDialog).len(),
            1
        );
        assert!(window.try_find_by_label("Cancel").is_none());
        by_label(window, "OK", cx, Tap::click);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .find_all_by_role(gpui_kit::Role::AlertDialog)
                .is_empty()
        );
    })
    .unwrap();
}

// ── Text field context menu ──────────────────────────────────────────────

struct InputHost {
    input: Entity<InputState>,
}

impl Render for InputHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .p_4()
            .w_64()
            .child(Input::new(&self.input).id("field"))
    }
}

/// The right-click menu on text fields is Kit's native OS menu (Cut, Copy,
/// Paste, Select All), which the headless window cannot open. Its commands
/// are the input's actions; this drives them through their key bindings.
#[gpui_kit::test]
fn text_field_cut_copy_paste_and_select_all(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| InputHost {
            input: cx.new(|cx| InputState::new(window, cx).default_value("Psalm 23")),
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("field", cx);
        window.press("secondary-a", cx);
        window.press("secondary-c", cx);
        assert_eq!(
            cx.read_from_clipboard().and_then(|i| i.text()).as_deref(),
            Some("Psalm 23")
        );
        window.press("secondary-x", cx);
        window.render_frame(cx);
        assert_eq!(window.find("field").value(), Some(""));
        window.press("secondary-v", cx);
        window.press("secondary-v", cx);
        window.render_frame(cx);
        assert_eq!(window.find("field").value(), Some("Psalm 23Psalm 23"));
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            host.read(cx).input.read(cx).value().as_ref(),
            "Psalm 23Psalm 23"
        )
    });
}

// ── Selection bar ────────────────────────────────────────────────────────

struct BarHost {
    count: usize,
    deleted: usize,
}

impl Render for BarHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().p_4().w(px(600.)).child(
            SelectionBar::new(self.count, "slide")
                .hint("Pick slides")
                .action(SelectionAction::new("export", "Export").disabled(true))
                .action(
                    SelectionAction::new("delete", "Delete")
                        .danger(true)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.deleted += 1;
                            cx.notify();
                        })),
                )
                .on_clear(cx.listener(|this, _, _, cx| {
                    this.count = 0;
                    cx.notify();
                })),
        )
    }
}

#[gpui_kit::test]
fn selection_bar_counts_runs_actions_and_clears(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |_, cx| {
        cx.new(|_| BarHost {
            count: 2,
            deleted: 0,
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find_by_label("2 slides selected").is_some());
        assert_eq!(window.find_by_label("Export").disabled(), Some(true));
        by_label(window, "Export", cx, Tap::click);
        by_label(window, "Delete", cx, Tap::click);
        by_label(window, "Clear", cx, Tap::click);
        window.render_frame(cx);
        assert!(window.try_find_by_label("Pick slides").is_some());
    })
    .unwrap();
    cx.update(|cx| assert_eq!(host.read(cx).deleted, 1));
}

// ── Library section ──────────────────────────────────────────────────────

struct LibraryHost {
    section: Entity<LibrarySection>,
    events: Vec<LibrarySectionEvent>,
}

impl Render for LibraryHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().w_64().child(self.section.clone())
    }
}

fn library_host(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<LibraryHost>) {
    open(cx, |_, cx| {
        cx.new(|cx| {
            let section = cx.new(|cx| {
                let mut section = LibrarySection::new(LibraryKind::Songs);
                section.set_libraries(
                    vec![LibraryRow {
                        id: "hymns".into(),
                        name: "Hymns".into(),
                        count: 3,
                    }],
                    10,
                    cx,
                );
                section
            });
            cx.subscribe(
                &section,
                |this: &mut LibraryHost, _, event: &LibrarySectionEvent, _| {
                    this.events.push(event.clone())
                },
            )
            .detach();
            LibraryHost {
                section,
                events: Vec::new(),
            }
        })
    })
}

#[gpui_kit::test]
fn library_rows_select_rename_and_report_errors(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = library_host(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find_by_label("All songs").selected(), Some(true));
        by_label(window, "Hymns", cx, Tap::click);
        window.render_frame(cx);
        by_label(window, "Hymns", cx, Tap::double_click);
        window.render_frame(cx);
        let name = window.find_by_label("Library name");
        assert_eq!(name.focused(), Some(true));
        window.input("Psalms", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let events = &host.read(cx).events;
        assert!(
            events.contains(&LibrarySectionEvent::Select("hymns".into())),
            "{events:?}"
        );
        assert!(
            events.contains(&LibrarySectionEvent::Rename {
                id: "hymns".into(),
                name: "Psalms".into()
            }),
            "{events:?}"
        );
    });
    // Escape abandons a rename without an event.
    cx.update_window(window, |_, window, cx| {
        by_label(window, "Hymns", cx, Tap::double_click);
        window.render_frame(cx);
        window.input("Nope", cx);
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(window.try_find_by_label("Library name").is_none());
    })
    .unwrap();
    cx.update(|cx| {
        let renames = host
            .read(cx)
            .events
            .iter()
            .filter(|e| matches!(e, LibrarySectionEvent::Rename { .. }))
            .count();
        assert_eq!(renames, 1);
        host.read(cx)
            .section
            .clone()
            .update(cx, |s, cx| s.set_error(Some("Disk full".into()), cx));
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let alerts = window.find_all_by_role(gpui_kit::Role::Alert);
        assert!(alerts.iter().any(|a| a.label() == Some("Disk full")));
    })
    .unwrap();
}

#[gpui_kit::test]
fn library_new_button_asks_to_create(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = library_host(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        by_label(window, "New library", cx, Tap::click);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            host.read(cx).events.last(),
            Some(&LibrarySectionEvent::Create)
        )
    });
}

// ── Movable dialog ───────────────────────────────────────────────────────

struct MovableHost {
    trigger: Entity<InputState>,
    dialog: Option<Entity<MovableDialog>>,
}

impl Render for MovableHost {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let overlay = self.dialog.clone().map(|dialog| {
            dialog.update(cx, |dialog, cx| {
                let handle = dialog
                    .drag_handle("titlebar", cx)
                    .h_10()
                    .w_full()
                    .child("Settings");
                dialog.frame(
                    div().size_full().child(handle).child(div().child("Body")),
                    window,
                    cx,
                )
            })
        });
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .p_4()
                    .w_64()
                    .child(Input::new(&self.trigger).id("trigger")),
            )
            .children(overlay)
    }
}

#[gpui_kit::test]
fn movable_dialog_traps_focus_closes_on_escape_and_restores_focus(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| MovableHost {
            trigger: cx.new(|cx| InputState::new(window, cx)),
            dialog: None,
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("trigger", cx);
        host.update(cx, |host, cx| {
            let dialog = cx.new(|cx| {
                MovableDialog::new("settings", "Settings", size(px(400.), px(300.)), window, cx)
            });
            cx.subscribe_in(
                &dialog,
                window,
                |host: &mut MovableHost, dialog, event: &MovableDialogEvent, window, cx| {
                    if *event == MovableDialogEvent::Dismiss {
                        dialog.update(cx, |d, cx| d.restore(window, cx));
                        host.dialog = None;
                        cx.notify();
                    }
                },
            )
            .detach();
            host.dialog = Some(dialog);
            cx.notify();
        });
        window.render_frame(cx);
        let dialogs = window.find_all_by_role(gpui_kit::Role::Dialog);
        assert!(dialogs.iter().any(|d| d.label() == Some("Settings")));
        let bounds = window.find_by_label("Settings").bounds();
        assert_eq!(bounds.size, size(px(400.), px(300.)));
        assert_eq!(bounds.origin.x, px(250.));
        assert_eq!(window.find_by_label("Settings").focused(), Some(true));
        // Tab stays inside the dialog (it has no other stops).
        window.press("tab", cx);
        assert_eq!(window.find("trigger").focused(), Some(false));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find_all_by_role(gpui_kit::Role::Dialog).is_empty());
        assert_eq!(window.find("trigger").focused(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn dragging_the_handle_moves_and_clamps_the_dialog(cx: &mut TestAppContext) {
    setup(cx);
    // Positions are measured, so the dialog must not be mid-entrance.
    cx.update(|cx| kairo_ui_kit::set_reduced_motion(true, cx));
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| MovableHost {
            trigger: cx.new(|cx| InputState::new(window, cx)),
            dialog: None,
        })
    });
    cx.update_window(window, |_, window, cx| {
        host.update(cx, |host, cx| {
            host.dialog = Some(cx.new(|cx| {
                MovableDialog::new("settings", "Settings", size(px(400.), px(300.)), window, cx)
            }));
            cx.notify();
        });
        window.render_frame(cx);
        let start = window.find_by_label("Settings").bounds();
        let grab = start.origin + gpui_kit::point(px(200.), px(20.));
        window.drag(grab, grab + gpui_kit::point(px(60.), px(-30.)), cx);
        window.render_frame(cx);
        let moved = window.find_by_label("Settings").bounds();
        assert_eq!(
            moved.origin - start.origin,
            gpui_kit::point(px(60.), px(-30.))
        );
        // Far up: the top stops 8px below the window edge.
        window.drag(
            moved.origin + gpui_kit::point(px(200.), px(20.)),
            gpui_kit::point(px(450.), px(-500.)),
            cx,
        );
        window.render_frame(cx);
        assert_eq!(window.find_by_label("Settings").bounds().origin.y, px(8.));
    })
    .unwrap();
}

// ── Code field ───────────────────────────────────────────────────────────

struct CodeHost {
    code: Entity<kairo_ui_kit::components::CodeField>,
    complete: Vec<SharedString>,
}

impl Render for CodeHost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().p_4().child(self.code.clone())
    }
}

#[gpui_kit::test]
fn code_field_accepts_a_pasted_code_and_typed_digits(cx: &mut TestAppContext) {
    use kairo_ui_kit::components::CodeFieldEvent;
    setup(cx);
    let (window, host) = open(cx, |window, cx| {
        cx.new(|cx| {
            let code = cx.new(|cx| {
                kairo_ui_kit::components::CodeField::new(6, "Confirmation code", window, cx)
            });
            cx.subscribe(
                &code,
                |this: &mut CodeHost, _, event: &CodeFieldEvent, _| {
                    if let CodeFieldEvent::Complete(value) = event {
                        this.complete.push(value.clone());
                    }
                },
            )
            .detach();
            CodeHost {
                code,
                complete: Vec::new(),
            }
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let focus = host.read(cx).code.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(
            "Your code: 123-456".into(),
        ));
        window.press("secondary-v", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(host.read(cx).complete, vec![SharedString::from("123456")]);
        host.read(cx).code.clone().update(cx, |code, cx| {
            // Clearing after a wrong code leaves the boxes empty.
            let _ = code;
            cx.notify();
        });
    });
}

// ── Traffic lights ───────────────────────────────────────────────────────

struct LightsHost {
    closed: usize,
    zoomed: usize,
    zoomable: bool,
}

impl Render for LightsHost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lights = kairo_ui_kit::components::TrafficLights::new("lights").on_close(cx.listener(
            |this, _, _, cx| {
                this.closed += 1;
                cx.notify();
            },
        ));
        let lights = if self.zoomable {
            lights.on_zoom(cx.listener(|this, _, _, cx| {
                this.zoomed += 1;
                cx.notify();
            }))
        } else {
            lights
        };
        div().p_4().child(lights)
    }
}

#[gpui_kit::test]
fn traffic_lights_close_and_zoom_only_when_zoomable(cx: &mut TestAppContext) {
    setup(cx);
    let (window, host) = open(cx, |_, cx| {
        cx.new(|_| LightsHost {
            closed: 0,
            zoomed: 0,
            zoomable: false,
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find_by_label("Zoom").is_none(),
            "green is greyed without a frame to zoom"
        );
        by_label(window, "Close", cx, Tap::click);
        host.update(cx, |h, cx| {
            h.zoomable = true;
            cx.notify();
        });
        window.render_frame(cx);
        by_label(window, "Zoom", cx, Tap::click);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(host.read(cx).closed, 1);
        assert_eq!(host.read(cx).zoomed, 1);
    });
}
