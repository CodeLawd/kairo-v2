//! The Scripture tool under the monitor: the queue search
//! (`OperatorQueueSearch.tsx`), the live sermon's passages and the staged
//! queue (`Operator.tsx`, the rail's `search`).

use kairo_ui_kit::ControlSize;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, FocusHandle, Focusable, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, deferred, div,
    prelude::FluentBuilder, px,
};
use kairo_domain::scripture::ScriptureResult;
use kairo_domain::scripture::query::{
    get_book_completion, get_book_completions, resolve_submitted_scripture_query,
    should_live_suggest_scripture_query,
};
use kairo_ui_kit::tokens::{Step, TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use super::text::{display_plan_title, queue_chip_label, verse_reference};
use super::widgets::{eyebrow, icon_button};
use super::{Booth, BoothAnswer, BoothCall};

/// Element ids, for tests.
pub mod ids {
    pub const INPUT: &str = "queue-search-input";
    pub const ERROR: &str = "queue-search-error";
    pub const SUGGESTIONS: &str = "queue-suggestions";
    pub const CLEAR_ALL: &str = "queue-clear-all";
    pub const EMPTY: &str = "queue-empty";
    pub fn suggestion(index: usize) -> String {
        format!("queue-suggestion-{index}")
    }
    pub fn row(id: &str) -> String {
        format!("queue-row-{id}")
    }
    pub fn present(id: &str) -> String {
        format!("queue-present-{id}")
    }
    pub fn chip(id: &str, index: usize) -> String {
        format!("queue-chip-{id}-{index}")
    }
    pub fn remove(id: &str) -> String {
        format!("queue-remove-{id}")
    }
    pub fn plan_item(id: &str) -> String {
        format!("sermon-item-{id}")
    }
}

const SUGGEST_DELAY: Duration = Duration::from_millis(250);

/// The compact search: live matches as you type, Tab completes a book,
/// Enter stages the hits into the queue.
pub struct QueueSearch {
    booth: Entity<Booth>,
    input: Entity<InputState>,
    loading: bool,
    error: Option<String>,
    suggestions: Vec<ScriptureResult>,
    open: bool,
    active: usize,
    suggesting: bool,
    request: u64,
    _pending: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl QueueSearch {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("John 3:16 · or \u{201c}love is patient\u{201d}")
        });
        let subscriptions = vec![cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, _, cx| match event {
                InputEvent::Change => this.changed(cx),
                InputEvent::Focus if !this.suggestions.is_empty() => {
                    this.open = true;
                    cx.notify();
                }
                _ => {}
            },
        )];
        Self {
            booth,
            input,
            loading: false,
            error: None,
            suggestions: Vec::new(),
            open: false,
            active: 0,
            suggesting: false,
            request: 0,
            _pending: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn query(&self, cx: &gpui_kit::App) -> String {
        self.input.read(cx).value().to_string()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn suggestions(&self) -> &[ScriptureResult] {
        &self.suggestions
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// The live suggestion request, debounced; a later keystroke wins.
    fn changed(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.error = None;
        self.request += 1;
        let request = self.request;
        if !should_live_suggest_scripture_query(&query) {
            self.suggestions.clear();
            self.open = false;
            self.suggesting = false;
            self._pending = None;
            cx.notify();
            return;
        }
        self.open = true;
        self.suggesting = true;
        let translation = self.booth.read(cx).default_translation.clone();
        self._pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SUGGEST_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                let search = cx.entity().downgrade();
                this.booth.update(cx, |booth, cx| {
                    booth.call(
                        BoothCall::Search {
                            query: query.trim().to_owned(),
                            translation,
                        },
                        cx,
                        move |_, result, cx| {
                            let _ = search.update(cx, |this, cx| {
                                if this.request != request {
                                    return;
                                }
                                this.suggesting = false;
                                this.open = true;
                                match result {
                                    Ok(BoothAnswer::Results(mut results)) => {
                                        results.truncate(5);
                                        this.suggestions = results;
                                        this.active = 0;
                                        this.error = None;
                                    }
                                    _ => this.suggestions.clear(),
                                }
                                cx.notify();
                            });
                        },
                    );
                });
            });
        }));
        cx.notify();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.request += 1;
        self.suggestions.clear();
        self.open = false;
        self.suggesting = false;
        self.error = None;
        cx.notify();
    }

    /// `stageResults`: ranges stay one queue row.
    fn stage(
        &mut self,
        results: Vec<ScriptureResult>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if results.is_empty() {
            return;
        }
        self.booth
            .update(cx, |b, cx| b.enqueue_results(results, cx));
        self.reset(window, cx);
    }

    /// `runSearch`: Enter with no suggestion open.
    pub fn run(&mut self, submitted: String, window: &mut Window, cx: &mut Context<Self>) {
        let q = submitted.trim().to_owned();
        if q.is_empty() || self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        let translation = self.booth.read(cx).default_translation.clone();
        let search = cx.entity().downgrade();
        let window_handle = window.window_handle();
        self.booth.update(cx, |booth, cx| {
            booth.call(BoothCall::Search { query: q, translation }, cx, move |_, result, cx| {
                let _ = window_handle.update(cx, |_, window, cx| {
                    let _ = search.update(cx, |this, cx| {
                        this.loading = false;
                        match result {
                            Ok(BoothAnswer::Results(results)) if results.is_empty() => {
                                this.error = Some(
                                    "No match — try \u{201c}John 3:16\u{201d} or a short phrase.".into(),
                                );
                                this.open = false;
                            }
                            Ok(BoothAnswer::Results(results)) => this.stage(results, window, cx),
                            Ok(_) => this.error = Some("Search failed".into()),
                            Err(error) => {
                                this.error = Some(if error.is_empty() {
                                    "Search failed".into()
                                } else {
                                    error
                                })
                            }
                        }
                        cx.notify();
                    });
                });
            });
        });
    }

    /// `handleKeyDown`. Returns whether the key was used.
    pub fn key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let query = self.query(cx);
        if key == "tab" {
            if let Some(done) = get_book_completion(&query) {
                self.input
                    .update(cx, |input, cx| input.set_value(done.value, window, cx));
                self.changed(cx);
                return true;
            }
            return false;
        }
        if key == "escape" && self.open {
            self.open = false;
            cx.notify();
            return true;
        }
        if self.open && !self.suggestions.is_empty() {
            let n = self.suggestions.len();
            match key {
                "down" => {
                    self.active = (self.active + 1) % n;
                    cx.notify();
                    return true;
                }
                "up" => {
                    self.active = (self.active + n - 1) % n;
                    cx.notify();
                    return true;
                }
                "enter" => {
                    let pick = self.suggestions[self.active.min(n - 1)].clone();
                    self.stage(vec![pick], window, cx);
                    return true;
                }
                _ => {}
            }
        }
        if key == "enter" {
            // "phi 4 19" could be Philemon or Philippians: the book list is
            // open, so Enter waits for a choice instead of searching.
            if get_book_completions(&query).len() > 1 {
                return true;
            }
            let submitted = resolve_submitted_scripture_query(&query);
            self.input.update(cx, |input, cx| {
                input.set_value(submitted.clone(), window, cx)
            });
            self.run(submitted, window, cx);
            return true;
        }
        false
    }
}

impl Focusable for QueueSearch {
    fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for QueueSearch {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        let query = self.query(cx);
        let completion = get_book_completion(&query);
        let completions = get_book_completions(&query);
        let live = self.open && should_live_suggest_scripture_query(&query);
        let field = h_flex().relative().child(
            Input::new(&self.input)
                .kairo_size()
                .small()
                .prefix(
                    Icon::new(KairoIcon::Search)
                        .size(px(13.))
                        .text_color(k.text(TextStep::T500)),
                )
                .when(self.loading, |i| {
                    i.suffix(
                        Icon::new(KairoIcon::Loader)
                            .size(px(12.))
                            .text_color(k.accent(Step::S400)),
                    )
                })
                .when(!query.is_empty() && !self.loading, |i| {
                    i.suffix(
                        icon_button("queue-search-clear", KairoIcon::X, "Clear queue search", cx)
                            .on_click(cx.listener(|this, _, window, cx| this.reset(window, cx))),
                    )
                })
                .disabled(self.loading),
        );
        let mut root = v_flex()
            .id(ids::INPUT)
            .test_support()
            .relative()
            .gap_1p5()
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let m = event.keystroke.modifiers;
                if m.platform || m.control || m.alt || m.shift {
                    return;
                }
                if this.key(event.keystroke.key.as_str(), window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(field);
        if let Some(done) = completion.as_ref().filter(|_| !self.open) {
            root = root.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .bg(k.surface_elevated)
                    .px_2p5()
                    .py_1p5()
                    .text_size(text::MICRO.size)
                    .child(
                        h_flex()
                            .gap_1()
                            .text_color(k.text(TextStep::T400))
                            .child("Complete")
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T200))
                                    .child(done.book.clone()),
                            ),
                    )
                    .child(
                        div()
                            .rounded_sm()
                            .bg(k.surface_tertiary)
                            .px_1p5()
                            .py_0p5()
                            .font_family(text::MONO_FAMILY)
                            .text_size(px(9.))
                            .text_color(k.accent(Step::S300))
                            .child("Tab"),
                    ),
            );
        }
        let popup = || {
            v_flex()
                .absolute()
                .left_0()
                .right_0()
                .top(px(44.))
                .rounded_lg()
                .border_1()
                .border_color(k.surface_border)
                .bg(k.surface_elevated)
                .shadow_2xl()
                .overflow_hidden()
                .occlude()
        };
        let choosing_book = completions.len() > 1 && !self.open;
        if choosing_book {
            // Deferred: drawn over the error line and the queue below it.
            root = root.child(
                deferred(
                    popup()
                        .id("queue-books")
                        .child(
                            div()
                                .px_2p5()
                                .py_1p5()
                                .text_size(text::MICRO.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(k.text(TextStep::T500))
                                .child("Choose a Bible book"),
                        )
                        .children(completions.into_iter().map(|c| {
                            let value = c.value.clone();
                            h_flex()
                                .id(SharedString::from(format!("queue-book-{}", c.book)))
                                .test_support()
                                .justify_between()
                                .px_2p5()
                                .py_2()
                                .cursor_pointer()
                                .text_size(text::MICRO.size)
                                .hover(|s| s.bg(k.surface_tertiary))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let submitted = resolve_submitted_scripture_query(&value);
                                    this.input.update(cx, |input, cx| {
                                        input.set_value(submitted.clone(), window, cx)
                                    });
                                    this.run(submitted, window, cx);
                                }))
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(k.accent(Step::S300))
                                        .child(c.book),
                                )
                                .child(div().text_color(k.text(TextStep::T500)).child(c.value))
                        })),
                )
                .with_priority(1),
            );
        }
        if live {
            let mut list = popup()
                .id(ids::SUGGESTIONS)
                .test_support()
                .max_h(px(224.))
                .overflow_y_scroll()
                .child(
                    h_flex()
                        .justify_between()
                        .px_2p5()
                        .py_1p5()
                        .bg(k.surface_elevated)
                        .text_size(text::MICRO.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T500))
                        .child("Scripture matches")
                        .child(if self.suggesting {
                            "Searching…"
                        } else {
                            "Enter to queue"
                        }),
                );
            let note = |copy: &str| {
                div()
                    .px_2p5()
                    .py_3()
                    .text_size(text::MICRO.size)
                    .text_color(k.text(TextStep::T500))
                    .child(copy.to_owned())
            };
            if self.suggestions.is_empty() {
                list = list.child(note(if self.suggesting {
                    "Searching your Bible translation…"
                } else {
                    "No verse matched that phrase. Try another wording, or a reference like Psalm 23:1."
                }));
            }
            for (index, result) in self.suggestions.iter().enumerate() {
                let selected = index == self.active;
                let pick = result.clone();
                list = list.child(
                    h_flex()
                        .id(SharedString::from(ids::suggestion(index)))
                        .test_support()
                        .role(gpui_kit::Role::ListBoxOption)
                        .items_start()
                        .gap_2()
                        .px_2p5()
                        .py_2()
                        .cursor_pointer()
                        .when(selected, |s| s.bg(k.row_selected()))
                        .when(!selected, |s| s.hover(|s| s.bg(k.surface_tertiary)))
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.active != index {
                                this.active = index;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.stage(vec![pick.clone()], window, cx)
                        }))
                        .child(
                            v_flex()
                                .w(px(96.))
                                .flex_shrink_0()
                                .text_size(text::MICRO.size)
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(k.accent(Step::S300))
                                .child(result.reference.clone())
                                .child(
                                    div()
                                        .mt_0p5()
                                        .font_weight(FontWeight::NORMAL)
                                        .text_color(k.text(TextStep::T600))
                                        .child(result.translation.clone()),
                                ),
                        )
                        .child(
                            div()
                                .line_clamp(2)
                                .text_size(text::MICRO.size)
                                .text_color(k.text(TextStep::T400))
                                .child(
                                    result
                                        .verses
                                        .iter()
                                        .map(|v| v.text.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" "),
                                ),
                        ),
                );
            }
            root = root.child(deferred(list).with_priority(1));
        }
        if let Some(error) = self.error.as_ref().filter(|_| !choosing_book) {
            root = root.child(
                div()
                    .id(ids::ERROR)
                    .test_support()
                    .px_0p5()
                    .text_size(text::MICRO.size)
                    .text_color(k.danger_text_soft)
                    .child(error.clone()),
            );
        }
        root
    }
}

/// The Scripture tab: search, the live sermon's passages, the queue.
pub struct QueuePanel {
    booth: Entity<Booth>,
    search: Entity<QueueSearch>,
    _subscriptions: Vec<Subscription>,
}

impl QueuePanel {
    pub fn new(booth: Entity<Booth>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| QueueSearch::new(booth.clone(), window, cx));
        let subscriptions = vec![cx.observe(&booth, |_, _, cx| cx.notify())];
        Self {
            booth,
            search,
            _subscriptions: subscriptions,
        }
    }

    pub fn search(&self) -> &Entity<QueueSearch> {
        &self.search
    }

    fn sermon(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let plan = booth.live_playlist()?.clone();
        let projection = booth.projection.clone().map(|p| p.0);
        let next = booth
            .live_plan
            .as_ref()
            .and_then(|p| p.next_plan_item_id.clone().flatten());
        let busy = booth.queue_busy.clone();
        let heard = booth.matched_plan_items();
        let title = display_plan_title(&plan.title);
        let mut section = v_flex()
            .id("sermon-passages")
            .aria_label("Sermon passages")
            .mb_3()
            .child(
                h_flex()
                    .items_baseline()
                    .justify_between()
                    .gap_3()
                    .px_1()
                    .pb_1p5()
                    .pt_1()
                    .child(eyebrow("Sermon", cx))
                    .child(
                        div()
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T600))
                            .child(plan.items.len().to_string()),
                    ),
            )
            .child(
                div()
                    .id("sermon-title")
                    .truncate()
                    .px_1()
                    .pb_1p5()
                    .text_size(text::LABEL.size)
                    .text_color(k.text(TextStep::T400))
                    .tooltip(super::widgets::tip(title.clone()))
                    .child(title),
            );
        if plan.items.is_empty() {
            return Some(
                section
                    .child(
                        div()
                            .px_2()
                            .py_3()
                            .text_center()
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T600))
                            .child("No passages in this sermon."),
                    )
                    .into_any_element(),
            );
        }
        for (index, item) in plan.items.iter().enumerate() {
            let live = projection.as_deref() == Some(item.reference.as_str())
                || item
                    .verses
                    .iter()
                    .any(|v| Some(verse_reference(v).as_str()) == projection.as_deref());
            let is_next = next.as_deref() == Some(item.id.as_str());
            let is_busy = busy.as_deref() == Some(item.id.as_str());
            let pick = item.clone();
            section = section.child(
                h_flex()
                    .id(SharedString::from(ids::plan_item(&item.id)))
                    .test_support()
                    .role(gpui_kit::Role::Button)
                    .tooltip(super::widgets::tip(format!("Show {}", item.reference)))
                    .w_full()
                    .items_start()
                    .gap_2()
                    .rounded_md()
                    .px_2p5()
                    .py_2()
                    .when(!is_busy, |s| s.cursor_pointer())
                    .when(live, |s| s.bg(k.tints.live))
                    .when(!live, |s| s.hover(|s| s.bg(k.surface)))
                    .when(!is_busy, |s| {
                        s.on_click(cx.listener(move |this, _, _, cx| {
                            let pick = pick.clone();
                            this.booth.update(cx, |b, cx| b.present_plan_item(pick, cx))
                        }))
                    })
                    .child(
                        div()
                            .w_4()
                            .flex_shrink_0()
                            .text_size(text::MICRO.size)
                            .text_color(k.text(TextStep::T600))
                            .child(format!("{:02}", index + 1)),
                    )
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(text::LABEL.size)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(if live {
                                                k.live
                                            } else {
                                                k.text(TextStep::T300)
                                            })
                                            .child(item.reference.clone()),
                                    )
                                    .when(is_next && !live, |this| {
                                        this.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(text::MICRO.size)
                                                .text_color(k.text(TextStep::T400))
                                                .child("Up next"),
                                        )
                                    }),
                            )
                            .when_some(
                                item.verses.first().filter(|v| !v.text.is_empty()),
                                |this, v| {
                                    this.child(
                                        div()
                                            .mt_0p5()
                                            .line_clamp(2)
                                            .text_size(text::MICRO.size)
                                            .text_color(k.text(TextStep::T500))
                                            .child(v.text.clone()),
                                    )
                                },
                            ),
                    )
                    .child(div().flex_shrink_0().map(|s| {
                        if is_busy {
                            s.child(
                                Icon::new(KairoIcon::Loader)
                                    .size(px(12.))
                                    .text_color(k.text(TextStep::T500)),
                            )
                        } else if heard.contains(&item.id) {
                            s.child(
                                div()
                                    .id(SharedString::from(format!("heard-{}", item.id)))
                                    .aria_label("Heard")
                                    .child(
                                        Icon::new(KairoIcon::Check)
                                            .size(px(12.))
                                            .text_color(k.accent(Step::S500)),
                                    ),
                            )
                        } else {
                            s
                        }
                    })),
            );
        }
        Some(section.into_any_element())
    }

    fn queue(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let booth = self.booth.read(cx);
        let queue = booth.queue.clone();
        let projection = booth.projection.clone().map(|p| p.0);
        let busy = booth.queue_busy.clone();
        let picks = booth.queue_picks.clone();
        let header = h_flex()
            .items_baseline()
            .justify_between()
            .gap_3()
            .px_1()
            .pb_1p5()
            .pt_1()
            .child(eyebrow("Queue", cx))
            .when(!queue.is_empty(), |this| {
                this.child(
                    Button::new(ids::CLEAR_ALL)
                        .cursor_pointer()
                        .ghost()
                        .xsmall()
                        .label("Clear all")
                        .text_color(k.text(TextStep::T600))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.booth.update(cx, |b, cx| b.clear_queue(cx))
                        })),
                )
            });
        if queue.is_empty() {
            return v_flex()
                .child(header)
                .child(
                    div()
                        .id(ids::EMPTY)
                        .test_support()
                        .px_2()
                        .py_6()
                        .text_center()
                        .text_size(text::MICRO.size)
                        .text_color(k.text(TextStep::T600))
                        .child("Stage verses here with + on a detection, or search above. Click a row to put it on screen."),
                )
                .into_any_element();
        }
        let rows = queue.iter().map(|entry| {
            let passage = entry.verses.len() > 1;
            let live_index = entry
                .verses
                .iter()
                .position(|v| Some(verse_reference(v).as_str()) == projection.as_deref());
            let live =
                live_index.is_some() || projection.as_deref() == Some(entry.reference.as_str());
            let picked = picks
                .get(&entry.id)
                .copied()
                .or(live_index)
                .unwrap_or(0)
                .min(entry.verses.len().saturating_sub(1));
            let shown = entry
                .verses
                .get(picked)
                .map(|v| v.text.clone())
                .unwrap_or_default();
            let is_busy = busy.as_deref() == Some(entry.id.as_str());
            let heading = h_flex()
                .min_w_0()
                .text_size(text::LABEL.size)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(if live { k.live } else { k.text(TextStep::T200) })
                .child(div().truncate().child(entry.reference.clone()))
                .child(
                    div()
                        .ml_1p5()
                        .font_weight(FontWeight::NORMAL)
                        .text_color(k.text(TextStep::T600))
                        .child(entry.translation.clone()),
                )
                .when(passage, |this| {
                    this.child(
                        div()
                            .ml_1p5()
                            .font_weight(FontWeight::NORMAL)
                            .text_color(k.text(TextStep::T600))
                            .child(format!("· {} verses", entry.verses.len())),
                    )
                });
            let body = div()
                .mt_0p5()
                .line_clamp(2)
                .text_size(text::MICRO.size)
                .text_color(k.text(TextStep::T500))
                .child(shown);
            let present_id = entry.id.clone();
            let present = div()
                .id(SharedString::from(ids::present(&entry.id)))
                .test_support()
                .role(gpui_kit::Role::Button)
                .aria_label(format!("Present {} {}", entry.reference, entry.translation))
                .w_full()
                // Busy: a press from assistive technology does nothing, as the pointer does.
                .when(is_busy, |s| s.on_click(|_, _, _| {}))
                .when(!is_busy, |s| {
                    s.cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = present_id.clone();
                            this.booth
                                .update(cx, |b, cx| b.present_queued(&id, picked, cx))
                        }))
                });
            let content = if passage {
                let first = entry.verses.first().cloned();
                let chips = h_flex()
                    .id(SharedString::from(format!("queue-chips-{}", entry.id)))
                    .mt_1()
                    .flex_wrap()
                    .gap_1()
                    .aria_label(format!("Verses of {}", entry.reference))
                    .children(entry.verses.iter().enumerate().map(|(index, verse)| {
                        let chip_live = Some(index) == live_index;
                        let reference = verse_reference(verse);
                        let id = entry.id.clone();
                        div()
                            .id(SharedString::from(ids::chip(&entry.id, index)))
                            .test_support()
                            .role(gpui_kit::Role::Button)
                            .aria_label(format!(
                                "Present {reference}{}",
                                if chip_live { ", on screen" } else { "" }
                            ))
                            .tooltip(super::widgets::tip(format!("Present {reference}")))
                            .h_5()
                            .min_w_5()
                            .px_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_sm()
                            .text_size(text::MICRO.size)
                            .font_weight(FontWeight::MEDIUM)
                            .map(|s| {
                                if chip_live {
                                    s.bg(k.live).text_color(k.white)
                                } else if index == picked {
                                    s.bg(k.accent(Step::S500)).text_color(k.on_accent)
                                } else {
                                    s.text_color(k.text(TextStep::T400)).hover(|s| {
                                        s.bg(k.surface_tertiary).text_color(k.text_primary)
                                    })
                                }
                            })
                            .when(!is_busy, |s| {
                                s.cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let id = id.clone();
                                        this.booth
                                            .update(cx, |b, cx| b.present_queued(&id, index, cx))
                                    }))
                            })
                            .child(queue_chip_label(verse, first.as_ref()))
                    }));
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(heading)
                    .child(chips)
                    .child(present.child(body))
            } else {
                v_flex()
                    .min_w_0()
                    .flex_1()
                    .child(present.child(heading).child(body))
            };
            let remove_id = entry.id.clone();
            h_flex()
                .id(SharedString::from(ids::row(&entry.id)))
                .test_support()
                .group("queue-row")
                .items_start()
                .gap_2()
                .rounded_md()
                .px_2p5()
                .py_2()
                .when(live, |s| s.bg(k.tints.live))
                .when(!live, |s| s.hover(|s| s.bg(k.surface_rail)))
                .child(content)
                .child(
                    div()
                        .mt_0p5()
                        .opacity(0.)
                        .group_hover("queue-row", |s| s.opacity(1.))
                        .child(
                            icon_button(
                                SharedString::from(ids::remove(&entry.id)),
                                KairoIcon::X,
                                format!("Remove {} from the queue", entry.reference),
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let id = remove_id.clone();
                                    this.booth.update(cx, |b, cx| b.remove_from_queue(&id, cx))
                                },
                            )),
                        ),
                )
        });
        v_flex()
            .child(header)
            .child(
                v_flex()
                    .gap_0p5()
                    .rounded_lg()
                    .bg(k.surface)
                    .p_1()
                    .children(rows),
            )
            .into_any_element()
    }
}

impl Render for QueuePanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sermon = self.sermon(cx);
        let queue = self.queue(cx);
        v_flex()
            .id("queue-panel")
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .pb_2p5()
                    .pt_3()
                    .child(self.search.clone()),
            )
            .child(
                v_flex()
                    .id("queue-scroll")
                    .min_h_0()
                    .flex_1()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .children(sermon)
                    .child(queue),
            )
    }
}
