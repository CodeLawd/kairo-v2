//! The sermon notes review (`SermonNotesReviewModal.tsx`): edit the notes,
//! see each detected reference highlighted, filter and jump through them,
//! name the playlist, then create it.
//!
//! The notes are a Kit `EditorState` (IME, undo, paste and selection come
//! with it) drawn without its code-editor chrome, in the always-light
//! theme. Highlights are Kit text decorations, which follow edits until
//! the next scan replaces them. Edits rescan after 300 ms, off the UI
//! thread, through [`ScriptureBackend`].

use kairo_ui_kit::ControlSize;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::base::input::InputEditorStyle;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{
    EditorState, Input, InputEvent, InputState, TextDecoration, TextDecorationCollection,
};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, HighlightStyle,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, deferred, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::scripture::{SermonNotesAnalysis, SermonPlanDraft, SermonReferenceMatch};
use kairo_ui_kit::tokens::{KairoColors, TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{ScriptureAnswer, ScriptureBackend, ScriptureCall};
use crate::model::ask;

/// The rescan pause after an edit.
pub const RESCAN_AFTER: Duration = Duration::from_millis(300);

pub mod ids {
    pub const DIALOG: &str = "sermon-review";
    pub const TITLE: &str = "sermon-playlist-title";
    pub const NOTES: &str = "sermon-notes";
    pub const SEARCH: &str = "sermon-reference-search";
    pub const CLEAR_SEARCH: &str = "sermon-reference-search-clear";
    pub const PREV: &str = "sermon-prev-reference";
    pub const NEXT: &str = "sermon-next-reference";
    pub const STATUS: &str = "sermon-scan-status";
    pub const SCAN_ERROR: &str = "sermon-scan-error";
    pub const RETRY: &str = "sermon-retry-scan";
    pub const CREATE: &str = "sermon-create-playlist";
    pub const CANCEL: &str = "sermon-cancel";
    pub const CLOSE: &str = "sermon-close";
    pub const EMPTY: &str = "sermon-no-references";
    pub const NO_MATCH: &str = "sermon-no-matching-references";

    pub fn reference(index: usize) -> String {
        format!("sermon-reference-{index}")
    }
}

/// What the dialog asks of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewEvent {
    Cancel,
    /// Create playlist: the name, the edited notes and the last scan.
    Confirm {
        title: String,
        text: String,
        analysis: SermonNotesAnalysis,
    },
}

/// A UTF-16 offset (the scan's unit) as a UTF-8 byte offset into `text`.
pub fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

/// A match's byte range in `text`, or `None` when it does not fit.
pub fn match_range(text: &str, found: &SermonReferenceMatch) -> Option<Range<usize>> {
    if found.start < 0.0 || found.end < found.start {
        return None;
    }
    let start = utf16_to_byte(text, found.start as usize);
    let end = utf16_to_byte(text, found.end as usize);
    (start < end).then_some(start..end)
}

/// The reference filter's key: lower case, en and em dashes as hyphens.
fn filter_key(value: &str) -> String {
    value.trim().to_lowercase().replace(['–', '—'], "-")
}

/// Matches whose reference or translations contain `query`, with their
/// original numbers.
pub fn visible_matches<'a>(
    matches: &'a [SermonReferenceMatch],
    query: &str,
) -> Vec<(usize, &'a SermonReferenceMatch)> {
    let needle = filter_key(query);
    matches
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            filter_key(&format!("{} {}", m.reference, m.translations.join(" "))).contains(&needle)
        })
        .collect()
}

/// ↑/↓ over the filtered list, wrapping (`navigateMatches`).
pub fn step_match(visible: &[usize], active: usize, forward: bool) -> Option<usize> {
    if forward {
        visible
            .iter()
            .copied()
            .find(|i| *i > active)
            .or(visible.first().copied())
    } else {
        visible
            .iter()
            .copied()
            .rev()
            .find(|i| *i < active)
            .or(visible.last().copied())
    }
}

pub struct SermonReview {
    backend: Arc<dyn ScriptureBackend>,
    draft: SermonPlanDraft,
    title: Entity<InputState>,
    notes: Entity<EditorState>,
    search: Entity<InputState>,
    marks: Option<TextDecorationCollection>,
    /// The ranges and active flag last painted (read by tests).
    painted: Vec<(Range<usize>, bool)>,
    analysis: SermonNotesAnalysis,
    scanning: bool,
    scan_error: Option<String>,
    active: usize,
    query: String,
    sequence: u64,
    confirming: bool,
    focus: FocusHandle,
    _scan: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ReviewEvent> for SermonReview {}

impl SermonReview {
    pub fn new(
        backend: Arc<dyn ScriptureBackend>,
        draft: SermonPlanDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let light = KairoColors::light();
        let title_text = draft.title.clone();
        let title = cx.new(|cx| InputState::new(window, cx).default_value(title_text));
        let body = draft
            .text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\t', "    ");
        let notes = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(false)
                .folding(false)
                .searchable(false)
                .soft_wrap(true)
                .auto_close(false)
                .smart_indent(false)
                .default_value(body);
            state.set_editor_style(InputEditorStyle {
                foreground: light.text_primary,
                muted_foreground: light.text(TextStep::T500),
                background: light.paper,
                border: light.surface_border,
                selection: light.sermon_mark_active,
                caret: light.text_primary,
                ..Default::default()
            });
            state
        });
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search detected scripture"));
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&notes, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.edited(cx);
            }
        }));
        subscriptions.push(cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.query = this.search.read(cx).value().to_string();
                cx.notify();
            }
        }));
        let mut this = Self {
            backend,
            draft,
            title,
            notes,
            search,
            marks: None,
            painted: Vec::new(),
            analysis: SermonNotesAnalysis {
                matches: Vec::new(),
                items: Vec::new(),
            },
            scanning: true,
            scan_error: None,
            active: 0,
            query: String::new(),
            sequence: 0,
            confirming: false,
            focus: cx.focus_handle(),
            _scan: None,
            _subscriptions: subscriptions,
        };
        // Import-time matches index the raw file text; scan what is shown.
        this.analysis.items = this.draft.items.clone();
        this.scan_now(cx);
        this.title.update(cx, |s, cx| s.focus(window, cx));
        this
    }

    // ─── Readers ────────────────────────────────────────────────────────────

    pub fn analysis(&self) -> &SermonNotesAnalysis {
        &self.analysis
    }
    pub fn scanning(&self) -> bool {
        self.scanning
    }
    pub fn scan_error(&self) -> Option<&str> {
        self.scan_error.as_deref()
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn confirming(&self) -> bool {
        self.confirming
    }
    pub fn painted(&self) -> &[(Range<usize>, bool)] {
        &self.painted
    }
    pub fn notes(&self) -> &Entity<EditorState> {
        &self.notes
    }
    pub fn title_input(&self) -> &Entity<InputState> {
        &self.title
    }
    pub fn search_input(&self) -> &Entity<InputState> {
        &self.search
    }
    pub fn text(&self, cx: &gpui_kit::App) -> String {
        self.notes.read(cx).value().to_string()
    }
    /// Create playlist is offered.
    pub fn can_confirm(&self) -> bool {
        !self.confirming
            && !self.scanning
            && self.scan_error.is_none()
            && !self.analysis.items.is_empty()
    }

    // ─── Scanning ───────────────────────────────────────────────────────────

    fn edited(&mut self, cx: &mut Context<Self>) {
        self.sequence += 1;
        self.scanning = true;
        let timer = cx.background_executor().timer(RESCAN_AFTER);
        let sequence = self.sequence;
        self._scan = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.sequence == sequence {
                    this.scan_now(cx);
                }
            });
        }));
        cx.notify();
    }

    /// Scans the notes as they are now (also Retry scan).
    pub fn scan_now(&mut self, cx: &mut Context<Self>) {
        self.sequence += 1;
        let sequence = self.sequence;
        self.scanning = true;
        self.scan_error = None;
        let text = self.text(cx);
        let call = ask(&self.backend, ScriptureCall::ScanSermonNotes(text));
        self._scan = Some(cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if this.sequence != sequence {
                    return;
                }
                this.scanning = false;
                match answer {
                    Ok(ScriptureAnswer::Analysis(next)) => {
                        this.active = this.active.min(next.matches.len().saturating_sub(1));
                        this.analysis = next;
                        this.paint(cx);
                    }
                    _ => {
                        this.scan_error = Some(
                            "Could not scan your edits. Try scanning again before creating the playlist."
                                .into(),
                        )
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn paint(&mut self, cx: &mut Context<Self>) {
        let k = KairoColors::light();
        let text = self.text(cx);
        self.painted = self
            .analysis
            .matches
            .iter()
            .enumerate()
            .filter_map(|(i, m)| match_range(&text, m).map(|r| (r, i == self.active)))
            .collect();
        let decorations: Vec<TextDecoration> = self
            .painted
            .iter()
            .map(|(range, active)| {
                TextDecoration::new(
                    range.clone(),
                    HighlightStyle {
                        background_color: Some(if *active {
                            k.sermon_mark_active
                        } else {
                            k.sermon_mark
                        }),
                        ..Default::default()
                    },
                )
            })
            .collect();
        match &self.marks {
            Some(marks) => marks.set(decorations, cx),
            None => {
                let marks = self
                    .notes
                    .update(cx, |s, cx| s.create_decorations_collection(decorations, cx));
                self.marks = Some(marks);
            }
        }
    }

    // ─── Navigation ─────────────────────────────────────────────────────────

    /// Selects a reference in the notes (`goToMatch`).
    pub fn go_to(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.analysis.matches.len();
        if count == 0 {
            return;
        }
        let index = index % count;
        let text = self.text(cx);
        let Some(range) = match_range(&text, &self.analysis.matches[index]) else {
            return;
        };
        self.active = index;
        self.paint(cx);
        self.notes.update(cx, |s, cx| {
            s.set_selected_range(range, cx);
            s.focus(window, cx);
        });
        cx.notify();
    }

    pub fn step(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let visible: Vec<usize> = visible_matches(&self.analysis.matches, &self.query)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        if let Some(next) = step_match(&visible, self.active, forward) {
            self.go_to(next, window, cx);
        }
    }

    // ─── Ending ─────────────────────────────────────────────────────────────

    pub fn confirm(&mut self, cx: &mut Context<Self>) {
        if !self.can_confirm() {
            return;
        }
        let title = self.title.read(cx).value().trim().to_owned();
        let title = if title.is_empty() {
            self.draft.title.clone()
        } else {
            title
        };
        self.set_confirming(true, cx);
        cx.emit(ReviewEvent::Confirm {
            title,
            text: self.text(cx),
            analysis: self.analysis.clone(),
        });
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if !self.confirming {
            cx.emit(ReviewEvent::Cancel);
        }
    }

    /// While creating, nothing can be edited or dismissed.
    pub fn set_confirming(&mut self, confirming: bool, cx: &mut Context<Self>) {
        self.confirming = confirming;
        self.notes
            .update(cx, |s, cx| s.set_readonly(confirming, cx));
        self.title
            .update(cx, |s, cx| s.set_disabled(confirming, cx));
        cx.notify();
    }

    pub fn draft(&self) -> &SermonPlanDraft {
        &self.draft
    }
}

impl Focusable for SermonReview {
    fn focus_handle(&self, _: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SermonReview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = KairoColors::light();
        let dark = *cx.kairo();
        let count = self.analysis.matches.len();
        let visible = visible_matches(&self.analysis.matches, &self.query);
        let filtering = !filter_key(&self.query).is_empty();
        let busy = self.confirming || self.scanning || self.scan_error.is_some();
        let viewport = window.viewport_size();
        let width = px(1180.).min(viewport.width - px(40.));
        let height = px(860.).min(viewport.height - px(40.));
        let status = if self.scanning {
            "Scanning notes…"
        } else if self.scan_error.is_some() {
            "Scan needs attention"
        } else {
            "References up to date"
        };
        let list_status = if filtering {
            format!("{} of {count} references match", visible.len())
        } else if count > 0 {
            format!("Reference {} of {count}", self.active + 1)
        } else {
            "No references yet".into()
        };
        let items = self.analysis.items.len();
        let footer_text: SharedString = if self.scanning {
            "Updating scripture references…".into()
        } else if self.scan_error.is_some() {
            "Scan your notes again to continue.".into()
        } else {
            format!(
                "Your playlist will include {items} scripture {}.",
                if items == 1 { "passage" } else { "passages" }
            )
            .into()
        };

        let header = h_flex()
            .flex_none()
            .items_start()
            .justify_between()
            .gap_5()
            .px_6()
            .py_5()
            .bg(k.surface_elevated)
            .border_b_1()
            .border_color(k.surface_border)
            .child(
                h_flex()
                    .gap_3()
                    .min_w_0()
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size_10()
                            .rounded(radius::xl())
                            .bg(k.surface_secondary)
                            .child(Icon::new(KairoIcon::ScanSearch).text_color(k.text(TextStep::T400))),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().text_size(text::LG.size).font_weight(FontWeight::SEMIBOLD).text_color(k.text_primary).child("Review sermon notes"))
                            .child(div().mt_1().text_size(text::SM.size).text_color(k.text(TextStep::T500)).child("Check the notes and scripture references, then create your playlist.")),
                    ),
            )
            .child(
                Button::new(ids::CLOSE)
                    .ghost()
                    .small()
                    .icon(Icon::new(KairoIcon::X))
                    .accessibility_label("Close review")
                    .disabled(self.confirming)
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
            );

        let notes = v_flex()
            .min_h_0()
            .min_w_0()
            .flex_1()
            .bg(k.surface)
            .child(
                v_flex()
                    .flex_none()
                    .gap_3()
                    .px_6()
                    .py_4()
                    .bg(k.surface_elevated)
                    .border_b_1()
                    .border_color(k.surface_border)
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T300))
                                    .child("Playlist name"),
                            )
                            .child(
                                h_flex()
                                    .id(ids::STATUS)
                                    .test_support()
                                    .role(Role::Status)
                                    .gap_1p5()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .when(self.scanning, |d| {
                                        d.child(Icon::new(KairoIcon::Loader).size_3())
                                    })
                                    .child(status),
                            ),
                    )
                    .child(
                        div().id(ids::TITLE).test_support().child(
                            Input::new(&self.title)
                                .kairo_size()
                                .small()
                                .aria_label("Playlist name"),
                        ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(format!(
                                "{} · Click the notes below to edit",
                                self.draft.source_file_name
                            )),
                    )
                    .when_some(self.scan_error.clone(), |d, error| {
                        d.child(
                            h_flex()
                                .id(ids::SCAN_ERROR)
                                .test_support()
                                .role(Role::Alert)
                                .items_start()
                                .justify_between()
                                .gap_3()
                                .text_size(text::XS.size)
                                .text_color(k.destructive)
                                .child(div().flex_1().child(error))
                                .child(
                                    Button::new(ids::RETRY)
                                        .outline()
                                        .small()
                                        .label("Retry scan")
                                        .disabled(self.scanning || self.confirming)
                                        .on_click(cx.listener(|this, _, _, cx| this.scan_now(cx))),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .min_h_0()
                    .flex_1()
                    .bg(k.surface_secondary.opacity(0.4))
                    .p_5()
                    .child(
                        div()
                            .id(ids::NOTES)
                            .test_support()
                            .aria_label("Editable sermon notes")
                            .mx_auto()
                            .max_w(px(768.))
                            .h_full()
                            .overflow_hidden()
                            .rounded(radius::sm())
                            .bg(k.paper)
                            .border_1()
                            .border_color(k.text_primary.opacity(0.15))
                            .px_8()
                            .py_7()
                            .font_family(text::FAMILY)
                            .text_size(px(15.))
                            .line_height(px(28.))
                            .text_color(k.text_primary)
                            .child(self.notes.clone()),
                    ),
            );

        let list: gpui_kit::AnyElement =
            if count == 0 {
                v_flex()
                    .id(ids::EMPTY)
                    .test_support()
                    .m_1()
                    .rounded(radius::lg())
                    .bg(k.surface_secondary)
                    .px_4()
                    .py_6()
                    .items_center()
                    .child(
                        div()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T300))
                            .child(if self.scanning {
                                "Scanning for references…"
                            } else {
                                "No references found"
                            }),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_center()
                            .text_size(text::XS.size)
                            .text_color(k.text(TextStep::T500))
                            .child(if self.scanning {
                                "Highlights appear once the notes finish scanning."
                            } else {
                                "Add one such as “John 3:16” directly in the notes."
                            }),
                    )
                    .into_any_element()
            } else if visible.is_empty() {
                div()
                    .id(ids::NO_MATCH)
                    .test_support()
                    .role(Role::Status)
                    .px_4()
                    .py_6()
                    .text_center()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T400))
                    .child("No matching references. Try a book, verse or translation.")
                    .into_any_element()
            } else {
                v_flex()
                    .gap_0p5()
                    .children(visible.iter().map(|(index, m)| {
                        let index = *index;
                        let on = index == self.active;
                        h_flex()
                            .id(SharedString::from(ids::reference(index)))
                            .test_support()
                            .role(Role::Button)
                            .aria_label(if m.translations.is_empty() {
                                m.reference.clone()
                            } else {
                                format!("{}, {}", m.reference, m.translations.join(" · "))
                            })
                            .aria_selected(on)
                            .items_start()
                            .gap_3()
                            .rounded(radius::lg())
                            .border_1()
                            .px_3()
                            .py_3()
                            .map(|d| {
                                if on {
                                    d.border_color(
                                        k.accent(kairo_ui_kit::tokens::Step::S400).opacity(0.4),
                                    )
                                    .bg(k.accent(kairo_ui_kit::tokens::Step::S50))
                                } else {
                                    d.border_color(gpui_kit::transparent_black())
                                        .hover(|s| s.bg(k.surface_secondary))
                                }
                            })
                            .when(busy, |d| d.opacity(0.6).on_click(|_, _, _| {}))
                            .when(!busy, |d| {
                                d.on_click(cx.listener(move |this, _, window, cx| {
                                    this.go_to(index, window, cx)
                                }))
                            })
                            .child(
                                div()
                                    .w_4()
                                    .flex_none()
                                    .text_right()
                                    .text_size(text::XS.size)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if on {
                                        k.accent(kairo_ui_kit::tokens::Step::S600)
                                    } else {
                                        k.text(TextStep::T500)
                                    })
                                    .child((index + 1).to_string()),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::SM.size)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(if on {
                                                k.text_primary
                                            } else {
                                                k.text(TextStep::T300)
                                            })
                                            .child(m.reference.clone()),
                                    )
                                    .when(!m.translations.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .mt_0p5()
                                                .truncate()
                                                .text_size(text::XS.size)
                                                .text_color(k.text(TextStep::T500))
                                                .child(m.translations.join(" · ")),
                                        )
                                    }),
                            )
                    }))
                    .into_any_element()
            };

        let aside = v_flex()
            .w(px(320.))
            .flex_none()
            .min_h_0()
            .bg(k.surface_elevated)
            .border_l_1()
            .border_color(k.surface_border)
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .px_4()
                    .py_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child("Detected scripture")
                                    .child(
                                        div()
                                            .text_size(text::XS.size)
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(k.text(TextStep::T500))
                                            .child(count.to_string()),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(text::XS.size)
                                    .text_color(k.text(TextStep::T500))
                                    .child("Select a reference to find it in the notes."),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .child(
                                Button::new(ids::PREV)
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(KairoIcon::ArrowUp))
                                    .accessibility_label("Previous reference")
                                    .disabled(busy || visible.is_empty())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.step(false, window, cx)
                                    })),
                            )
                            .child(
                                Button::new(ids::NEXT)
                                    .ghost()
                                    .small()
                                    .icon(Icon::new(KairoIcon::ArrowDown))
                                    .accessibility_label("Next reference")
                                    .disabled(busy || visible.is_empty())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.step(true, window, cx)
                                    })),
                            ),
                    ),
            )
            .child(
                div().id(ids::SEARCH).test_support().mx_3().mb_2().child(
                    Input::new(&self.search)
                        .kairo_size()
                        .small()
                        .prefix(Icon::new(KairoIcon::ScanSearch).size_3())
                        .when(filtering, |i| {
                            i.suffix(
                                Button::new(ids::CLEAR_SEARCH)
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(KairoIcon::X))
                                    .accessibility_label("Clear scripture search")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.search.update(cx, |s, cx| {
                                            s.set_value("", window, cx);
                                            s.focus(window, cx);
                                        });
                                        this.query.clear();
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
            )
            .child(
                div()
                    .id("sermon-list-status")
                    .role(Role::Status)
                    .px_4()
                    .pb_2()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(list_status),
            )
            .child(
                div()
                    .id("sermon-detected-references")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .p_2()
                    .child(list),
            );

        let footer = h_flex()
            .flex_none()
            .flex_wrap()
            .justify_between()
            .gap_4()
            .px_6()
            .py_4()
            .bg(k.surface_elevated)
            .border_t_1()
            .border_color(k.surface_border)
            .child(
                div()
                    .text_size(text::XS.size)
                    .text_color(k.text(TextStep::T500))
                    .child(footer_text),
            )
            .child(
                h_flex()
                    .gap_2p5()
                    .child(
                        Button::new(ids::CANCEL)
                            .outline()
                            .label("Cancel")
                            .disabled(self.confirming)
                            .on_click(cx.listener(|this, _, _, cx| this.cancel(cx))),
                    )
                    .child(
                        Button::new(ids::CREATE)
                            .primary()
                            .when(self.confirming, |b| b.icon(Icon::new(KairoIcon::Loader)))
                            .label(if self.confirming {
                                "Creating playlist…"
                            } else {
                                "Create playlist"
                            })
                            .disabled(!self.can_confirm())
                            .on_click(cx.listener(|this, _, _, cx| this.confirm(cx))),
                    ),
            );

        deferred(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(dark.scrim())
                .occlude()
                .child(kairo_ui_kit::motion::enter(
                    v_flex()
                        .id(ids::DIALOG)
                        .test_support()
                        .role(Role::Dialog)
                        .aria_label("Review sermon notes")
                        .track_focus(&self.focus)
                        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                this.cancel(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .w(width)
                        .h(height)
                        .overflow_hidden()
                        .rounded(radius::xl2())
                        .border_1()
                        .border_color(k.neutral(kairo_ui_kit::tokens::Step::S500))
                        .bg(k.surface)
                        .text_color(k.text_primary)
                        .child(header)
                        .child(
                            h_flex()
                                .items_stretch()
                                .min_h_0()
                                .flex_1()
                                .child(notes)
                                .child(aside),
                        )
                        .child(footer),
                    "scripture-review-enter",
                    cx,
                )),
        )
        .with_priority(2)
    }
}
