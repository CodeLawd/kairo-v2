//! The script list and the editor.

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenuItem};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use kairo_domain::teleprompter::Script;
use kairo_ui_kit::tokens::{TextStep, metrics, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::{Pending, Request, Teleprompter};

impl Teleprompter {
    pub(super) fn library(&mut self, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let query = self.search.read(cx).value().trim().to_lowercase();
        let scripts: Vec<Script> = self
            .scripts
            .iter()
            .filter(|script| query.is_empty() || script.title.to_lowercase().contains(&query))
            .cloned()
            .collect();
        let entity = cx.entity();
        let mut list = v_flex()
            .id("teleprompter-scripts")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_1()
            .pb_2();
        if scripts.is_empty() {
            list = list.child(
                div()
                    .px_2()
                    .py_3()
                    .text_size(text::SM.size)
                    .text_color(k.text(TextStep::T400))
                    .child(if self.scripts.is_empty() {
                        "No saved scripts yet. Write one in the editor or import a Word or PDF file."
                    } else {
                        "No scripts match the search."
                    }),
            );
        }
        for (index, script) in scripts.into_iter().enumerate() {
            let selected = self.selected.as_deref() == Some(script.id.as_str());
            let live = self.on_display.as_deref() == Some(script.id.as_str());
            let open = script.clone();
            let menu_script = script.clone();
            let menu_view = entity.clone();
            list = list.child(
                v_flex()
                    .id(("teleprompter-script", index))
                    .px_2()
                    .py_1p5()
                    .gap_0p5()
                    .rounded(radius::sm())
                    .when(selected, |row| row.bg(k.row_selected()))
                    .when(!selected, |row| {
                        row.hover(|row| row.bg(k.surface_menu_hover))
                    })
                    .aria_label(script.title.clone())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_script(Some(open.clone()), window, cx)
                    }))
                    .context_menu(move |menu, _, _| {
                        let (view, script) = (menu_view.clone(), menu_script.clone());
                        menu.item(
                            PopupMenuItem::new("Delete\u{2026}")
                                .icon(Icon::new(KairoIcon::Trash2))
                                .on_click(move |_, window, cx| {
                                    view.update(cx, |this, cx| {
                                        this.delete(script.clone(), window, cx)
                                    });
                                }),
                        )
                    })
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .when(selected, |title| title.font_weight(FontWeight::MEDIUM))
                            .truncate()
                            .child(script.title.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(edited(script.updated_at_ms))
                            .when(live, |meta| {
                                meta.child("\u{b7}")
                                    .child(div().text_color(k.live).child("On display"))
                            }),
                    ),
            );
        }
        v_flex()
            .w_64()
            .flex_none()
            .h_full()
            .min_h_0()
            .bg(k.sidebar_list)
            .border_r(metrics::panel_border_width())
            .border_color(k.panel_border)
            .child(
                h_flex()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(text::BASE.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Scripts"),
                    )
                    .child(
                        Button::new("teleprompter-new")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Plus))
                            .accessibility_label("New script")
                            .tooltip("New script")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.after_discard(Pending::Open(None), window, cx)
                            })),
                    )
                    .child(
                        Button::new("teleprompter-import")
                            .ghost()
                            .xsmall()
                            .icon(Icon::new(KairoIcon::Upload))
                            .disabled(self.busy)
                            .accessibility_label("Import Word or PDF")
                            .tooltip("Import Word or PDF\u{2026}")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.request(Request::Import, window, cx)
                            })),
                    ),
            )
            .child(
                div().px_3().pb_2().child(
                    Input::new(&self.search)
                        .small()
                        .aria_label("Search scripts"),
                ),
            )
            .child(list)
            .into_any_element()
    }

    pub(super) fn editor(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some((title, warnings)) = self.import.clone() {
            return self.import_review(title, warnings, cx);
        }
        let k = *cx.kairo();
        let dirty = self.dirty(cx);
        let saved = self.selected.is_some() && !dirty;
        let body = self.body.read(cx).value().to_string();
        let behind = self
            .published
            .as_ref()
            .filter(|published| {
                self.on_display.as_deref() == Some(published.id.as_str())
                    && self.selected.as_deref() == Some(published.id.as_str())
            })
            .is_some_and(|published| published.body != body);
        let selected = self.selected_script().cloned();
        let entity = cx.entity();
        v_flex()
            .size_full()
            .min_h_0()
            .px_4()
            .pt_3()
            .pb_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(&self.title)
                                .aria_label("Script title")
                                .disabled(self.busy),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T400))
                            .child(if saved {
                                "Saved"
                            } else if self.selected.is_some() {
                                "Unsaved changes"
                            } else {
                                "Not saved"
                            }),
                    )
                    .child(
                        Button::new("teleprompter-save")
                            .label("Save")
                            .icon(Icon::new(KairoIcon::Save))
                            .disabled(self.busy || saved)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    )
                    .child(
                        Button::new("teleprompter-more")
                            .ghost()
                            .icon(Icon::new(KairoIcon::MoreHorizontal))
                            .accessibility_label("More script actions")
                            .disabled(self.busy || selected.is_none())
                            .dropdown_menu(move |menu, _, _| {
                                let Some(script) = selected.clone() else {
                                    return menu;
                                };
                                let view = entity.clone();
                                menu.item(
                                    PopupMenuItem::new("Delete script\u{2026}")
                                        .icon(Icon::new(KairoIcon::Trash2))
                                        .on_click(move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.delete(script.clone(), window, cx)
                                            });
                                        }),
                                )
                            }),
                    ),
            )
            .when(behind, |this| {
                this.child(
                    h_flex()
                        .id("teleprompter-behind")
                        .gap_3()
                        .px_3()
                        .py_2()
                        .items_center()
                        .rounded(radius::md())
                        .bg(k.surface_secondary)
                        .child(Icon::new(KairoIcon::AlertCircle).text_color(k.warning_text))
                        .child(
                            div()
                                .flex_1()
                                .text_size(text::SM.size)
                                .child("The display shows an earlier version of this script."),
                        )
                        .child(
                            Button::new("teleprompter-update")
                                .small()
                                .label("Update display")
                                .disabled(self.busy)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.publish(window, cx)),
                                ),
                        ),
                )
            })
            .child(Textarea::new(&self.body).flex_1().aria_label("Script text"))
            .when_some(self.error.clone(), |this, error| {
                this.child(
                    h_flex()
                        .id("teleprompter-error")
                        .gap_2()
                        .items_center()
                        .text_size(text::SM.size)
                        .text_color(k.danger_text)
                        .child(Icon::new(KairoIcon::AlertCircle))
                        .child(error),
                )
            })
            .into_any_element()
    }

    fn import_review(
        &mut self,
        title: String,
        warnings: Vec<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let k = *cx.kairo();
        let dirty = self.dirty(cx);
        v_flex()
            .size_full()
            .min_h_0()
            .px_4()
            .pt_3()
            .pb_4()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(text::LG.size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("Review \u{201c}{title}\u{201d}")),
                    )
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .text_color(k.text(TextStep::T400))
                            .child("Correct the text before using it. Nothing is saved or shown on the display yet."),
                    ),
            )
            .when(!warnings.is_empty(), |this| {
                this.child(
                    v_flex()
                        .id("teleprompter-import-warnings")
                        .gap_1()
                        .px_3()
                        .py_2()
                        .rounded(radius::md())
                        .bg(k.surface_secondary)
                        .children(warnings.into_iter().map(|warning| {
                            h_flex()
                                .gap_2()
                                .items_start()
                                .text_size(text::SM.size)
                                .child(
                                    Icon::new(KairoIcon::AlertTriangle)
                                        .text_color(k.warning_text),
                                )
                                .child(div().flex_1().child(warning))
                        })),
                )
            })
            .child(
                Textarea::new(&self.preview_body)
                    .flex_1()
                    .aria_label("Imported script"),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("teleprompter-cancel-import")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.import = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("teleprompter-keep-import")
                            .primary()
                            .label(if dirty {
                                "Replace current draft"
                            } else {
                                "Use text"
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let body = this.preview_body.read(cx).value().to_string();
                                let title = title.clone();
                                this.after_discard(Pending::Import { title, body }, window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}

/// When a script was last saved, in the operator's terms.
fn edited(updated_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let minutes = (now - updated_at_ms).max(0) / 60_000;
    match minutes {
        0 => "Edited just now".into(),
        1..=59 => format!("Edited {minutes} min ago"),
        60..=1439 => format!("Edited {} h ago", minutes / 60),
        _ => chrono::DateTime::from_timestamp_millis(updated_at_ms).map_or_else(
            || "Edited earlier".into(),
            |at| format!("Edited {}", at.format("%-d %b %Y")),
        ),
    }
}
