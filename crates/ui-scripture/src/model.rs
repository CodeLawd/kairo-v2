//! The Scripture page's state and flows (`Scripture.tsx`): search, live
//! suggestions, the chapter and verse toggle, translation reloads, playlists,
//! the live plan, adjacent verses, going live, picks and the library.
//!
//! The view reads this model and calls its methods; every request goes
//! through [`ScriptureBackend`] and answers on GPUI's executor.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui_kit::{Context, EventEmitter, Task};
use kairo_domain::collections::{
    DEFAULT_LIBRARY_ID, LibrariesCommand, LibraryKind, LibraryState, items_in_library,
    library_counts,
};
use kairo_domain::media_order::DropPosition;
use kairo_domain::scripture::offline_state::get_scripture_cache_notice;
use kairo_domain::scripture::plan::{
    AdjacentDirection, append_scripture_results_to_plan, extend_sermon_plan_item_with_adjacent,
    reorder_sermon_plan_item, resolve_sermon_plan_items, translation_fallback_order,
};
use kairo_domain::scripture::query::{
    BookCompletion, Direction, combine_scripture_results, expand_scripture_result,
    get_adjacent_verse_queries, get_book_completions, normalize_scripture_query,
    reload_passages_in_translation, resolve_submitted_scripture_query,
    should_live_suggest_scripture_query,
};
use kairo_domain::scripture::{
    LivePlanState, PassagesCommand, SavedPassage, ScriptureResult, ScriptureSuggestion,
    ScriptureTranslationOption, SermonPlan, SermonPlanDraft, SermonPlanDraftItem,
    SermonScriptureItem, SuggestionSource,
};

use crate::backend::{
    FeedSink, Listening, OptionChange, ScriptureAnswer, ScriptureBackend, ScriptureCall,
    ScriptureFeed, SlideOptions,
};
use crate::rows::{
    self, CARD_ZOOM_DEFAULT, GridKey, ResultRow, RowOptions, Scope, SendStatus, create_result_row,
};

/// Where the cards on screen came from (`cardsSource`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Search,
    Plan,
}

/// `scriptureHighlightMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Highlight {
    #[default]
    None,
    Focus,
    Live,
}

/// `scriptureViewState`: the open playlist and the card in it.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    pub plan_id: String,
    pub item_id: Option<String>,
    pub card_in_row: usize,
    pub mode: Highlight,
}

/// What the view should bring on screen after a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollTarget {
    /// A card, nearest edge.
    Card(usize),
    /// A card, centred (a chapter opened on its verse).
    CardCentred(usize),
    /// A passage's first line.
    Row(usize),
}

/// What the page asks of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptureEvent {
    /// `openSettings('scripture')` ("Get more").
    OpenSettings,
    /// Sermon notes were read; 079's review opens with the draft.
    ReviewSermonNotes(SermonPlanDraft),
    /// A short confirmation (`useTransferStore.notify({ tone: 'ok' })`).
    Notify(String),
    /// Focus the lookup field (after Clear).
    FocusQuery,
    /// The review's playlist was created (`true`) or failed (`false`).
    ImportFinished(bool),
}

/// The shift or ⌘ gesture on a card or row (`selectGesture`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickGesture {
    Toggle,
    Range,
}

/// `useMultiSelect` over an ordered set of keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picks<K: Ord + Clone> {
    selected: BTreeSet<K>,
    anchor: Option<K>,
}

impl<K: Ord + Clone> Picks<K> {
    pub fn pick(&mut self, key: K, gesture: PickGesture, order: &[K]) {
        match gesture {
            PickGesture::Toggle => {
                if !self.selected.remove(&key) {
                    self.selected.insert(key.clone());
                }
                self.anchor = Some(key);
            }
            PickGesture::Range => {
                let from = self
                    .anchor
                    .as_ref()
                    .and_then(|a| order.iter().position(|k| k == a));
                let to = order.iter().position(|k| *k == key);
                match (from, to) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = (a.min(b), a.max(b));
                        self.selected.extend(order[lo..=hi].iter().cloned());
                    }
                    _ => {
                        self.selected.insert(key.clone());
                        self.anchor = Some(key);
                    }
                }
            }
        }
    }
    pub fn select_all(&mut self, order: &[K]) {
        self.selected = order.iter().cloned().collect();
    }
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }
    pub fn contains(&self, key: &K) -> bool {
        self.selected.contains(key)
    }
    pub fn len(&self) -> usize {
        self.selected.len()
    }
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }
    pub fn selected(&self) -> &BTreeSet<K> {
        &self.selected
    }
}

/// How long a sent card shows as sent.
pub const SENT_FOR: Duration = Duration::from_millis(3000);
/// The live-suggestion debounce.
pub const SUGGEST_AFTER: Duration = Duration::from_millis(250);

pub(crate) fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default()
}

fn error_text(error: &str, fallback: &str) -> String {
    if let Some(notice) = get_scripture_cache_notice(error) {
        return format!("{} {}", notice.message, notice.hint);
    }
    if error.is_empty() {
        fallback.to_owned()
    } else {
        error.to_owned()
    }
}

fn plain_error(error: String, fallback: &str) -> String {
    if error.is_empty() {
        fallback.to_owned()
    } else {
        error
    }
}

/// A browsed verse as the suggestion `pushScriptureFromTab` presents.
pub fn manual_suggestion(result: &ScriptureResult, id: String) -> ScriptureSuggestion {
    ScriptureSuggestion {
        cache_source: None,
        quote_corpus_version: None,
        quote_evidence: None,
        quote_revision: None,
        retracted: None,
        provisional: None,
        suggestion_only: None,
        ambiguous: None,
        id,
        trigger_text: result.reference.clone(),
        reference: result.reference.clone(),
        verses: result.verses.clone(),
        translation: result.translation.clone(),
        confidence: 1.0,
        source: SuggestionSource::Manual,
        passage_id: None,
        passage_reference: None,
        passage_index: None,
        passage_length: None,
        correlation_id: None,
        preloaded_next: None,
        plan_id: None,
        plan_item_id: None,
        plan_match: None,
        requested_translation: None,
        extra: Default::default(),
    }
}

/// Runs one backend call to completion.
pub(crate) fn ask(
    backend: &Arc<dyn ScriptureBackend>,
    call: ScriptureCall,
) -> impl Future<Output = Result<ScriptureAnswer, String>> + use<> {
    let (tx, rx) = futures_channel::oneshot::channel();
    backend.call(
        call,
        Box::new(move |result| {
            let _ = tx.send(result);
        }),
    );
    async move {
        rx.await
            .unwrap_or_else(|_| Err("The request was dropped.".into()))
    }
}

/// `window.api.scripture.search`.
pub(crate) fn search(
    backend: &Arc<dyn ScriptureBackend>,
    query: String,
    translation: String,
) -> impl Future<Output = Result<Vec<ScriptureResult>, String>> + use<> {
    let call = ask(backend, ScriptureCall::Search { query, translation });
    async move {
        match call.await? {
            ScriptureAnswer::Results(results) => Ok(results),
            _ => Ok(Vec::new()),
        }
    }
}

async fn save_plan(
    backend: &Arc<dyn ScriptureBackend>,
    plan: SermonPlan,
) -> Result<SermonPlan, String> {
    match ask(backend, ScriptureCall::SavePlan(plan.clone())).await? {
        ScriptureAnswer::Plan(saved) => Ok(saved),
        _ => Ok(plan),
    }
}

pub struct Scripture {
    backend: Arc<dyn ScriptureBackend>,
    // Lookup.
    query: String,
    recent_lookups: Vec<String>,
    suggestions: Vec<ScriptureResult>,
    suggestions_open: bool,
    suppress_query: Option<String>,
    suggestion_request: u64,
    _suggest: Option<Task<()>>,
    // Cards.
    rows: Vec<ResultRow>,
    search_rows: Vec<ResultRow>,
    source: Option<Source>,
    active: usize,
    cue: bool,
    focus: bool,
    zoom: f32,
    picks: Picks<usize>,
    picks_key: String,
    scroll: Option<ScrollTarget>,
    loading: bool,
    error: Option<String>,
    /// The chosen Bible lacks the passage another Bible has.
    missing: Option<MissingPassage>,
    navigating: Option<Direction>,
    opening_chapter: bool,
    translation_reload: u64,
    sent_tokens: u64,
    // Translations and settings.
    translation: String,
    /// `tabTranslation`: the pick and the default when it was made.
    tab_translation: Option<(String, String)>,
    translations: Vec<ScriptureTranslationOption>,
    options: SlideOptions,
    api_bible_warning: bool,
    // Playlists.
    plans: Vec<SermonPlan>,
    selected_plan: Option<String>,
    pending_delete: Option<String>,
    renaming: Option<String>,
    creating: bool,
    importing: bool,
    added_all: Option<String>,
    live_plan: Option<LivePlanState>,
    view_state: Option<ViewState>,
    item_picks: Picks<String>,
    item_filter: String,
    // Library.
    passages: Vec<SavedPassage>,
    libraries: LibraryState,
    active_library: String,
    passage_picks: Picks<String>,
    library_to_rename: Option<String>,
    _listening: Option<Listening>,
    _feed: Option<Task<()>>,
}

impl EventEmitter<ScriptureEvent> for Scripture {}

impl Scripture {
    pub fn new(backend: Arc<dyn ScriptureBackend>, cx: &mut Context<Self>) -> Self {
        let snapshot = backend.snapshot();
        let translation = snapshot.options.default_translation.clone();
        let mut this = Self {
            backend: backend.clone(),
            query: String::new(),
            recent_lookups: rows::read_recent_lookups(snapshot.recent_lookups.as_ref()),
            suggestions: Vec::new(),
            suggestions_open: false,
            suppress_query: None,
            suggestion_request: 0,
            _suggest: None,
            rows: Vec::new(),
            search_rows: Vec::new(),
            source: None,
            active: 0,
            cue: false,
            focus: false,
            zoom: CARD_ZOOM_DEFAULT,
            picks: Picks::default(),
            picks_key: String::new(),
            scroll: None,
            loading: false,
            error: None,
            missing: None,
            navigating: None,
            opening_chapter: false,
            translation_reload: 0,
            sent_tokens: 0,
            translation,
            tab_translation: None,
            translations: snapshot.translations,
            options: snapshot.options,
            api_bible_warning: snapshot.api_bible_warning,
            plans: snapshot.plans,
            selected_plan: None,
            pending_delete: None,
            renaming: None,
            creating: false,
            importing: false,
            added_all: None,
            live_plan: snapshot.live_plan,
            view_state: None,
            item_picks: Picks::default(),
            item_filter: String::new(),
            passages: snapshot.passages,
            libraries: snapshot.libraries,
            active_library: DEFAULT_LIBRARY_ID.into(),
            passage_picks: Picks::default(),
            library_to_rename: None,
            _listening: None,
            _feed: None,
        };
        let (tx, mut rx) = futures_channel::mpsc::unbounded::<ScriptureFeed>();
        let sink: FeedSink = Arc::new(move |feed| {
            let _ = tx.unbounded_send(feed);
        });
        this._listening = Some(backend.listen(sink));
        this._feed = Some(cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(feed) = rx.next().await {
                if this.update(cx, |this, cx| this.apply(feed, cx)).is_err() {
                    break;
                }
            }
        }));
        this
    }

    fn apply(&mut self, feed: ScriptureFeed, cx: &mut Context<Self>) {
        match feed {
            ScriptureFeed::Options(options) => self.options = options,
            ScriptureFeed::Translations(t) => self.translations = t,
            ScriptureFeed::Plans(plans) => self.merge_plans(plans),
            ScriptureFeed::LivePlan(state) => self.live_plan = state,
            ScriptureFeed::Passages(p) => self.passages = p,
            ScriptureFeed::Libraries(l) => self.libraries = l,
            ScriptureFeed::ApiBibleWarning(w) => self.api_bible_warning = w,
            ScriptureFeed::OutputCleared => self.output_cleared(),
        }
        cx.notify();
    }

    /// New playlists and newer copies land without discarding edits held here.
    fn merge_plans(&mut self, incoming: Vec<SermonPlan>) {
        let added: Vec<SermonPlan> = incoming
            .iter()
            .filter(|p| !self.plans.iter().any(|h| h.id == p.id))
            .cloned()
            .collect();
        for plan in &mut self.plans {
            if let Some(fresh) = incoming
                .iter()
                .find(|p| p.id == plan.id && p.updated_at > plan.updated_at)
            {
                *plan = fresh.clone();
            }
        }
        if !added.is_empty() {
            let mut next = added;
            next.append(&mut self.plans);
            self.plans = next;
        }
    }

    /// Header Clear (`scriptureOutputClearToken`).
    pub fn header_cleared(&mut self, cx: &mut Context<Self>) {
        self.output_cleared();
        cx.notify();
    }

    /// Header Clear: drop the Live badges.
    fn output_cleared(&mut self) {
        let mode = self.view_state.as_ref().map(|v| v.mode).unwrap_or_default();
        self.cue = false;
        self.focus = mode == Highlight::Focus;
        for row in &mut self.rows {
            for card in &mut row.cards {
                card.send_status = SendStatus::Idle;
            }
        }
    }

    // ─── Readers ────────────────────────────────────────────────────────────

    pub fn query(&self) -> &str {
        &self.query
    }
    pub fn recent_lookups(&self) -> &[String] {
        &self.recent_lookups
    }
    pub fn suggestions(&self) -> &[ScriptureResult] {
        &self.suggestions
    }
    pub fn suggestions_open(&self) -> bool {
        self.suggestions_open
    }
    pub fn book_completions(&self) -> Vec<BookCompletion> {
        get_book_completions(&self.query)
    }
    pub fn rows(&self) -> &[ResultRow] {
        &self.rows
    }
    pub fn search_count(&self) -> usize {
        self.search_rows.len()
    }
    pub fn source(&self) -> Option<Source> {
        self.source
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn cue(&self) -> bool {
        self.cue
    }
    pub fn focus(&self) -> bool {
        self.focus
    }
    pub fn zoom(&self) -> f32 {
        self.zoom
    }
    pub fn picks(&self) -> &Picks<usize> {
        &self.picks
    }
    pub fn loading(&self) -> bool {
        self.loading
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn navigating(&self) -> Option<Direction> {
        self.navigating
    }
    pub fn opening_chapter(&self) -> bool {
        self.opening_chapter
    }
    pub fn translation(&self) -> &str {
        &self.translation
    }
    pub fn translations(&self) -> &[ScriptureTranslationOption] {
        &self.translations
    }
    pub fn options(&self) -> &SlideOptions {
        &self.options
    }
    pub fn plans(&self) -> &[SermonPlan] {
        &self.plans
    }
    pub fn selected_plan_id(&self) -> Option<&str> {
        self.selected_plan.as_deref()
    }
    pub fn selected_plan(&self) -> Option<&SermonPlan> {
        let id = self.selected_plan.as_deref()?;
        self.plans.iter().find(|p| p.id == id)
    }
    pub fn pending_delete(&self) -> Option<&str> {
        self.pending_delete.as_deref()
    }
    pub fn renaming(&self) -> Option<&str> {
        self.renaming.as_deref()
    }
    pub fn creating(&self) -> bool {
        self.creating
    }
    pub fn importing(&self) -> bool {
        self.importing
    }
    pub fn added_all(&self) -> Option<&str> {
        self.added_all.as_deref()
    }
    pub fn live_plan(&self) -> Option<&LivePlanState> {
        self.live_plan.as_ref()
    }
    pub fn view_state(&self) -> Option<&ViewState> {
        self.view_state.as_ref()
    }
    pub fn item_picks(&self) -> &Picks<String> {
        &self.item_picks
    }
    pub fn item_filter(&self) -> &str {
        &self.item_filter
    }
    pub fn passages(&self) -> &[SavedPassage] {
        &self.passages
    }
    pub fn libraries(&self) -> &LibraryState {
        &self.libraries
    }
    pub fn active_library(&self) -> &str {
        &self.active_library
    }
    pub fn passage_picks(&self) -> &Picks<String> {
        &self.passage_picks
    }
    /// Takes the pending scroll request.
    pub fn take_scroll(&mut self) -> Option<ScrollTarget> {
        self.scroll.take()
    }

    /// The open playlist's items (`openPlanItems`).
    /// The selected playlist's items, listed in the sidebar whether the
    /// playlist or a lookup is on the cards.
    pub fn open_plan_items(&self) -> &[SermonScriptureItem] {
        self.selected_plan().map_or(&[], |plan| &plan.items)
    }

    /// The open playlist's items after the filter, with their numbers.
    pub fn filtered_items(&self) -> Vec<(usize, &SermonScriptureItem)> {
        let needle = self.item_filter.trim().to_lowercase();
        self.open_plan_items()
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                needle.is_empty()
                    || item.reference.to_lowercase().contains(&needle)
                    || item.translation.to_lowercase().contains(&needle)
            })
            .collect()
    }

    /// The card's playlist item, when a playlist is on screen (`activeItemId`).
    pub fn active_item_id(&self) -> Option<&str> {
        if self.source != Some(Source::Plan) {
            return None;
        }
        let (row, _) = rows::locate(&self.rows, self.active)?;
        self.rows[row].plan_item_id.as_deref()
    }

    /// Verse | Chapter in the queue bar.
    pub fn scope(&self) -> Option<Scope> {
        if self.source != Some(Source::Search) {
            return None;
        }
        let (row, _) = rows::locate(&self.rows, self.active)?;
        Some(rows::search_scope(&self.rows[row]))
    }

    /// The queue bar's label.
    pub fn queue_label(&self) -> String {
        if self.source == Some(Source::Plan) {
            return self
                .selected_plan()
                .map_or_else(|| "Playlist".into(), |p| p.title.clone());
        }
        rows::locate(&self.rows, self.active)
            .map(|(row, _)| self.rows[row].reference.clone())
            .unwrap_or_else(|| "—".into())
    }

    /// The chosen Bible lacks the last searched passage.
    pub fn missing_passage(&self) -> Option<&MissingPassage> {
        self.missing.as_ref()
    }

    /// A translation's display name, or its id when unknown.
    pub fn translation_name(&self, id: &str) -> String {
        self.translations
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| id.to_owned(), |t| t.name.clone())
    }

    /// Show the missing passage in the Bible that has it, for this search only.
    pub fn show_missing_in_fallback(&mut self, cx: &mut Context<Self>) {
        let Some(missing) = self.missing.take() else {
            return;
        };
        self.remember(&missing.reference);
        // The picker names the Bible on screen. Set directly: the slides are
        // already in this Bible, so `set_translation` would reload for nothing.
        self.translation = missing.fallback.clone();
        self.tab_translation = Some((
            missing.fallback.clone(),
            self.options.default_translation.clone(),
        ));
        let found = missing
            .results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                create_result_row(
                    r,
                    RowOptions {
                        id: Some(format!("search-{i}-{}", r.reference)),
                        ..Default::default()
                    },
                )
            })
            .collect();
        self.show_search(found);
        cx.notify();
    }

    /// The default Bible has no text here (the yellow notice).
    pub fn translation_notice(&self) -> Option<String> {
        if !self.api_bible_warning || self.translation.is_empty() {
            return None;
        }
        let option = self.translations.iter().find(|t| t.id == self.translation);
        if option.is_some_and(|o| o.available) {
            return None;
        }
        let t = &self.translation;
        Some(if option.and_then(|o| o.downloadable).unwrap_or(false) {
            format!(
                "{t} is the default but isn't installed yet. Download it in Settings → Scripture, or check managed Bible access in Account → Licence & Devices."
            )
        } else {
            format!(
                "{t} is the default, but its text requires managed Bible access. Check Account → Licence & Devices."
            )
        })
    }

    /// The passages in the library on screen.
    pub fn visible_passages(&self) -> Vec<&SavedPassage> {
        items_in_library(
            &self.libraries,
            &self.passages,
            &self.active_library,
            |p: &SavedPassage| p.id.as_str(),
        )
    }

    pub fn library_counts(&self) -> std::collections::BTreeMap<String, usize> {
        library_counts(&self.libraries, &self.passages, |p: &SavedPassage| {
            p.id.as_str()
        })
    }

    /// A new passage or playlist starts with nothing picked.
    pub fn reconcile_picks(&mut self) {
        let key = rows::loaded_key(&self.rows);
        if key != self.picks_key {
            self.picks_key = key;
            self.picks.clear();
        }
    }

    // ─── Lookup field ───────────────────────────────────────────────────────

    /// `onQueryChange`.
    pub fn set_query(&mut self, value: String, cx: &mut Context<Self>) {
        if value == self.query {
            return;
        }
        self.suppress_query = None;
        if should_live_suggest_scripture_query(&value) {
            self.suggestions_open = true;
        }
        self.query = value;
        self.schedule_suggestions(cx);
        cx.notify();
    }

    /// The field took focus: reopen current matches.
    pub fn query_focused(&mut self, cx: &mut Context<Self>) {
        if self.suppress_query.as_deref() == Some(self.query.trim()) {
            return;
        }
        if !self.suggestions.is_empty() {
            self.suggestions_open = true;
            cx.notify();
        }
    }

    fn close_suggestions(&mut self, suppress: Option<String>) {
        if let Some(q) = suppress {
            self.suppress_query = Some(q);
        }
        self.suggestion_request += 1;
        self.suggestions_open = false;
        self.suggestions.clear();
        self._suggest = None;
    }

    /// The live matches, 250 ms after typing stops.
    fn schedule_suggestions(&mut self, cx: &mut Context<Self>) {
        self.suggestion_request += 1;
        let request = self.suggestion_request;
        let query = self.query.trim().to_owned();
        if self.suppress_query.as_deref() == Some(query.as_str())
            || !should_live_suggest_scripture_query(&self.query)
        {
            self.suggestions.clear();
            self.suggestions_open = false;
            self._suggest = None;
            return;
        }
        let lookup = search(&self.backend, query, self.translation.clone());
        let timer = cx.background_executor().timer(SUGGEST_AFTER);
        self._suggest = Some(cx.spawn(async move |this, cx| {
            timer.await;
            let found = lookup.await;
            let _ = this.update(cx, |this, cx| {
                if this.suggestion_request != request {
                    return;
                }
                match found {
                    Ok(results) => {
                        this.suggestions = results.into_iter().take(5).collect();
                        this.suggestions_open = !this.suggestions.is_empty();
                    }
                    Err(_) => {
                        this.suggestions.clear();
                        this.suggestions_open = false;
                    }
                }
                cx.notify();
            });
        }));
    }

    /// Enter in the field (`handleKeyDown`).
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        let submitted = resolve_submitted_scripture_query(&self.query);
        self.close_suggestions(Some(submitted.clone()));
        self.query = submitted.clone();
        self.run_search(submitted, cx);
    }

    /// A recent lookup was picked.
    pub fn pick_recent(&mut self, reference: String, cx: &mut Context<Self>) {
        self.query = reference.clone();
        self.run_search(reference, cx);
    }

    /// Tab or a book row: fill in the book and keep typing.
    pub fn complete_book(&mut self, value: String, cx: &mut Context<Self>) {
        self.set_query(value, cx);
    }

    pub fn clear_query(&mut self, cx: &mut Context<Self>) {
        self.set_query(String::new(), cx);
    }

    /// `handleSearch`.
    pub fn run_search(&mut self, submitted: String, cx: &mut Context<Self>) {
        let q = submitted.trim().to_owned();
        if q.is_empty() {
            return;
        }
        self.close_suggestions(Some(q.clone()));
        self.loading = true;
        self.error = None;
        self.missing = None;
        cx.notify();
        let backend = self.backend.clone();
        let translation = self.translation.clone();
        let default = self.options.default_translation.clone();
        let translations = self.translations.clone();
        cx.spawn(async move |this, cx| {
            let outcome: Result<(Vec<ScriptureResult>, Option<MissingPassage>), String> = async {
                let results = search(&backend, q.clone(), translation.clone()).await?;
                let looks_like_reference =
                    normalize_scripture_query(&q).is_some() || has_chapter_verse(&q);
                // Never swap Bibles silently: find one that has the passage,
                // default Bible first, and let the operator choose it.
                let mut missing = None;
                if results.is_empty() && looks_like_reference {
                    for fallback in
                        translation_fallback_order(&translation, &default, &translations)
                    {
                        if fallback == translation {
                            continue;
                        }
                        let found = search(&backend, q.clone(), fallback.clone()).await?;
                        if !found.is_empty() {
                            let reference = if found.len() == 1 {
                                found[0].reference.clone()
                            } else {
                                q.clone()
                            };
                            missing = Some(MissingPassage {
                                reference,
                                translation: translation.clone(),
                                fallback,
                                results: found,
                            });
                            break;
                        }
                    }
                }
                Ok((results, missing))
            }
            .await;
            let target = this
                .read_with(cx, |this, _| this.translation.clone())
                .unwrap_or_default();
            let outcome = match outcome {
                Ok((results, missing)) if target != translation && !results.is_empty() => {
                    let b = backend.clone();
                    let reloaded =
                        reload_passages_in_translation(&results, &target, |q, t| search(&b, q, t))
                            .await;
                    Ok((reloaded.results, missing))
                }
                other => other,
            };
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match outcome {
                    Ok((results, missing)) => {
                        this.missing = missing;
                        if !results.is_empty() && normalize_scripture_query(&q).is_some() {
                            let reference = if results.len() == 1 {
                                results[0].reference.clone()
                            } else {
                                q.clone()
                            };
                            this.remember(&reference);
                        }
                        let found: Vec<ResultRow> = results
                            .iter()
                            .enumerate()
                            .map(|(i, r)| {
                                create_result_row(
                                    r,
                                    RowOptions {
                                        id: Some(format!("search-{i}-{}", r.reference)),
                                        ..Default::default()
                                    },
                                )
                            })
                            .collect();
                        this.show_search(found);
                        if results.is_empty() && this.missing.is_none() {
                            this.error = Some(
                                "No results. Try a reference like “John 3:16” or “Romans 8:28”."
                                    .into(),
                            );
                        }
                    }
                    Err(error) => {
                        this.error = Some(error_text(&error, "Search failed"));
                        this.rows.clear();
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remember(&mut self, reference: &str) {
        self.recent_lookups = rows::remember_lookup(&self.recent_lookups, reference);
        self.backend
            .save_recent_lookups(rows::recent_lookups_value(&self.recent_lookups));
    }

    fn show_search(&mut self, found: Vec<ResultRow>) {
        self.search_rows = found;
        self.source = Some(Source::Search);
        self.refresh_search_view();
        self.added_all = None;
        self.active = 0;
    }

    /// A live match was picked: just that verse.
    pub fn preview_suggestion(&mut self, result: ScriptureResult, cx: &mut Context<Self>) {
        self.close_suggestions(Some(result.reference.clone()));
        self.query = result.reference.clone();
        let row = create_result_row(
            &result,
            RowOptions {
                id: Some(format!("suggest-{}", result.reference)),
                ..Default::default()
            },
        );
        self.show_search(vec![row]);
        self.error = None;
        cx.notify();
    }

    /// A kept passage, back onto the cards.
    pub fn open_passage(&mut self, passage: &SavedPassage, cx: &mut Context<Self>) {
        self.preview_suggestion(
            kairo_domain::collections::passage_to_result(passage.clone()),
            cx,
        );
    }

    /// The whole chapter, with the found verse picked.
    pub fn open_chapter(&mut self, result: ScriptureResult, cx: &mut Context<Self>) {
        let Some(verse) = result.verses.first().cloned() else {
            return;
        };
        if verse.book.is_empty() || verse.chapter == 0.0 || self.opening_chapter {
            return;
        }
        let chapter_query = format!(
            "{} {}",
            verse.book,
            kairo_domain::js::number_to_string(verse.chapter)
        );
        self.close_suggestions(Some(chapter_query.clone()));
        self.query = chapter_query.clone();
        self.opening_chapter = true;
        self.error = None;
        cx.notify();
        let lookup = search(
            &self.backend,
            chapter_query.clone(),
            result.translation.clone(),
        );
        cx.spawn(async move |this, cx| {
            let found = lookup.await;
            let _ = this.update(cx, |this, cx| {
                this.opening_chapter = false;
                match found {
                    Ok(results) => match results.into_iter().next() {
                        None => {
                            this.error = Some(format!(
                                "Could not load {chapter_query} in {}.",
                                result.translation
                            ));
                        }
                        Some(chapter) => {
                            let row = create_result_row(
                                &chapter,
                                RowOptions {
                                    id: Some(format!("chapter-{chapter_query}")),
                                    ..Default::default()
                                },
                            );
                            let index = row
                                .cards
                                .iter()
                                .position(|c| {
                                    c.result.verses.first().map(|v| v.verse) == Some(verse.verse)
                                })
                                .unwrap_or(0);
                            this.show_search(vec![row]);
                            this.active = index;
                            this.scroll = Some(ScrollTarget::CardCentred(index));
                        }
                    },
                    Err(error) => {
                        this.error = Some(plain_error(
                            error,
                            &format!("Could not load {chapter_query}."),
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Verse in the queue bar: only the selected card.
    pub fn show_verse_only(&mut self, cx: &mut Context<Self>) {
        let Some(card) = rows::card_at(&self.rows, self.active).cloned() else {
            return;
        };
        let row = create_result_row(
            &card.result,
            RowOptions {
                id: Some(format!("verse-{}", card.result.reference)),
                ..Default::default()
            },
        );
        self.search_rows = vec![row];
        self.refresh_search_view();
        self.query = card.result.reference.clone();
        self.active = 0;
        cx.notify();
    }

    /// Chapter in the queue bar.
    pub fn show_chapter(&mut self, cx: &mut Context<Self>) {
        if let Some(card) = rows::card_at(&self.rows, self.active).cloned() {
            self.open_chapter(card.result, cx);
        }
    }

    // ─── Translation ────────────────────────────────────────────────────────

    /// The page opens again: a pick made under an older default is dropped.
    pub fn page_shown(&mut self, cx: &mut Context<Self>) {
        let default = &self.options.default_translation;
        let next = match &self.tab_translation {
            Some((value, when)) if when == default => value.clone(),
            _ => default.clone(),
        };
        if next != self.translation && self.rows.is_empty() {
            self.translation = next;
            cx.notify();
        }
    }

    /// `handleTranslationChange`: a working choice for this page only.
    pub fn set_translation(&mut self, next: String, cx: &mut Context<Self>) {
        self.translation = next.clone();
        self.tab_translation = Some((next.clone(), self.options.default_translation.clone()));
        // A passage the old Bible lacked: look it up again in the new one.
        if let Some(missing) = self.missing.take() {
            self.run_search(missing.reference, cx);
            return;
        }
        self.schedule_suggestions(cx);
        cx.notify();
        if self.source != Some(Source::Search) || self.rows.is_empty() {
            return;
        }
        if self
            .rows
            .iter()
            .all(|r| r.cards.iter().all(|c| c.result.translation == next))
        {
            return;
        }
        self.translation_reload += 1;
        let request = self.translation_reload;
        let active_reference =
            rows::card_at(&self.rows, self.active).map(|c| c.result.reference.clone());
        let passages: Vec<ScriptureResult> = self
            .search_rows
            .iter()
            .filter_map(|r| {
                combine_scripture_results(
                    &r.cards.iter().map(|c| c.result.clone()).collect::<Vec<_>>(),
                )
            })
            .collect();
        self.loading = true;
        self.error = None;
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let reloaded =
                reload_passages_in_translation(&passages, &next, |q, t| search(&backend, q, t))
                    .await;
            let _ = this.update(cx, |this, cx| {
                if request != this.translation_reload {
                    return;
                }
                this.loading = false;
                // The new Bible has none of it: ask, as a fresh lookup would,
                // rather than leave the old Bible's slides under its name.
                if !reloaded.results.is_empty()
                    && reloaded.unavailable.len() == reloaded.results.len()
                {
                    this.missing = Some(MissingPassage {
                        reference: reloaded.unavailable.join(", "),
                        translation: next.clone(),
                        fallback: reloaded.results[0].translation.clone(),
                        results: reloaded.results,
                    });
                    this.search_rows.clear();
                    this.refresh_search_view();
                    this.active = 0;
                    cx.notify();
                    return;
                }
                let updated: Vec<ResultRow> = reloaded
                    .results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        create_result_row(
                            r,
                            RowOptions {
                                id: Some(this.search_rows.get(i).map_or_else(
                                    || format!("search-{i}-{}", r.reference),
                                    |row| row.id.clone(),
                                )),
                                ..Default::default()
                            },
                        )
                    })
                    .collect();
                this.search_rows = updated;
                this.refresh_search_view();
                if let Some(reference) = active_reference {
                    this.active = rows::find_flat(&this.rows, |c, _| c.result.reference == reference)
                        .unwrap_or_else(|| {
                            this.active
                                .min(rows::card_count(&this.rows).saturating_sub(1))
                        });
                }
                if !reloaded.unavailable.is_empty() {
                    this.error = Some(format!(
                        "{next} text unavailable for {}. Those verses still show the previous translation.",
                        reloaded.unavailable.join(", ")
                    ));
                } else if let Some(fallback) =
                    reloaded.results.iter().find(|r| r.translation != next)
                {
                    this.error = Some(format!(
                        "{next} text unavailable — showing {}. Change translation in the dropdown when the Bible is available.",
                        fallback.translation
                    ));
                }
                cx.notify();
            });
        })
        .detach();
    }

    // ─── Cards ──────────────────────────────────────────────────────────────

    /// Selects a card without sending it.
    pub fn set_active(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active = index.min(rows::card_count(&self.rows).saturating_sub(1));
        cx.notify();
    }

    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(rows::CARD_ZOOM_MIN, rows::CARD_ZOOM_MAX);
        cx.notify();
    }

    fn persist(&mut self, index: usize, mode: Highlight) {
        if self.source != Some(Source::Plan) {
            return;
        }
        let (Some(plan_id), Some((row, card))) =
            (self.selected_plan.clone(), rows::locate(&self.rows, index))
        else {
            return;
        };
        self.view_state = Some(ViewState {
            plan_id,
            item_id: self.rows[row].plan_item_id.clone(),
            card_in_row: card,
            mode,
        });
    }

    /// A plain click on a card: select it and send it live.
    pub fn select_card(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active = index;
        self.cue = true;
        self.focus = true;
        self.persist(index, Highlight::Live);
        self.send(index, cx);
    }

    /// ⌘ or Shift on a card: pick it for a bulk action.
    pub fn pick_card(&mut self, index: usize, gesture: PickGesture, cx: &mut Context<Self>) {
        let order: Vec<usize> = (0..rows::card_count(&self.rows)).collect();
        self.picks.pick(index, gesture, &order);
        cx.notify();
    }

    pub fn clear_picks(&mut self, cx: &mut Context<Self>) {
        self.picks.clear();
        cx.notify();
    }

    /// An arrow, Enter or Space on the grid (`handleQueueKeyboard`).
    pub fn grid_key(&mut self, key: Option<GridKey>, cx: &mut Context<Self>) {
        let Some(key) = key else {
            self.send(self.active, cx);
            return;
        };
        let Some(next) = rows::grid_move(&self.rows, self.active, key) else {
            return;
        };
        self.active = next;
        self.cue = true;
        self.focus = true;
        self.persist(next, Highlight::Live);
        self.scroll = Some(ScrollTarget::Card(next));
        self.send(next, cx);
    }

    /// `handleSend`: present the card on the screens.
    pub fn send(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(card) = rows::card_at(&self.rows, index) else {
            return;
        };
        if card.send_status == SendStatus::Sending {
            return;
        }
        let id = format!("manual-{}-{}", now_ms() as u64, self.sent_tokens);
        self.sent_tokens += 1;
        let suggestion = manual_suggestion(&card.result, id);
        rows::set_card_status(&mut self.rows, index, SendStatus::Sending);
        cx.notify();
        let call = ask(&self.backend, ScriptureCall::Present(suggestion));
        cx.spawn(async move |this, cx| {
            let sent = call.await;
            let ok = sent.is_ok();
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = &sent {
                    tracing::warn!(%error, "[Scripture] present failed");
                }
                let status = if ok {
                    SendStatus::Sent
                } else {
                    SendStatus::Error
                };
                rows::set_card_status(&mut this.rows, index, status);
                cx.notify();
            });
            if ok {
                cx.background_executor().timer(SENT_FOR).await;
                let _ = this.update(cx, |this, cx| {
                    rows::set_card_status(&mut this.rows, index, SendStatus::Idle);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Clear in the queue bar.
    pub fn clear_results(&mut self, cx: &mut Context<Self>) {
        self.rows.clear();
        self.source = None;
        self.error = None;
        self.added_all = None;
        self.active = 0;
        self.cue = false;
        self.focus = false;
        self.selected_plan = None;
        self.view_state = None;
        cx.emit(ScriptureEvent::FocusQuery);
        cx.notify();
    }

    /// The Library's "Search results" row.
    pub fn show_search_results(&mut self, cx: &mut Context<Self>) {
        self.source = Some(Source::Search);
        self.refresh_search_view();
        cx.notify();
    }

    /// Prev / Next: load the verse beside the selected passage.
    pub fn load_adjacent(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let Some((row_index, _)) = rows::locate(&self.rows, self.active) else {
            return;
        };
        if self.navigating.is_some() {
            return;
        }
        let row = self.rows[row_index].clone();
        let anchor = match direction {
            Direction::Next => row.cards.last(),
            Direction::Previous => row.cards.first(),
        };
        let Some(anchor) = anchor.cloned() else {
            return;
        };
        let queries = get_adjacent_verse_queries(std::slice::from_ref(&anchor.result), direction);
        if queries.is_empty() {
            return;
        }
        self.navigating = Some(direction);
        self.error = None;
        cx.notify();
        let backend = self.backend.clone();
        let lookup_translation = if anchor.result.translation.is_empty() {
            self.translation.clone()
        } else {
            anchor.result.translation.clone()
        };
        let page_translation = self.translation.clone();
        cx.spawn(async move |this, cx| {
            let resolve = |tx: String| {
                let backend = backend.clone();
                let queries = queries.clone();
                async move {
                    for q in queries {
                        let Some(found) = search(&backend, q, tx.clone()).await?.into_iter().next()
                        else {
                            continue;
                        };
                        if found.verses.is_empty() {
                            continue;
                        }
                        if found.verses.len() > 1 && direction == Direction::Previous {
                            let last = found.verses.last().cloned().expect("verses");
                            return Ok::<_, String>(Some(ScriptureResult {
                                reference: format!(
                                    "{} {}:{}",
                                    last.book,
                                    kairo_domain::js::number_to_string(last.chapter),
                                    kairo_domain::js::number_to_string(last.verse)
                                ),
                                translation: found.translation,
                                verses: vec![last],
                            }));
                        }
                        let expanded = expand_scripture_result(&found);
                        return Ok(match direction {
                            Direction::Previous => expanded.last().cloned(),
                            Direction::Next => expanded.first().cloned(),
                        });
                    }
                    Ok(None)
                }
            };
            let mut found = resolve(lookup_translation.clone()).await;
            if matches!(found, Ok(None)) && lookup_translation != page_translation {
                found = resolve(page_translation).await;
            }
            let Ok(entity) = this.update(cx, |this, cx| {
                this.place_adjacent(found, row_index, direction, &lookup_translation, cx)
            }) else {
                return;
            };
            if let Some(save) = entity {
                let saved = save_plan(&backend, save).await;
                let _ = this.update(cx, |this, cx| {
                    if let Ok(saved) = saved {
                        this.replace_plan(saved);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Puts the adjacent verse in place; returns the playlist to save.
    fn place_adjacent(
        &mut self,
        found: Result<Option<ScriptureResult>, String>,
        row_index: usize,
        direction: Direction,
        lookup_translation: &str,
        cx: &mut Context<Self>,
    ) -> Option<SermonPlan> {
        self.navigating = None;
        cx.notify();
        let word = match direction {
            Direction::Next => "next",
            Direction::Previous => "previous",
        };
        let adjacent = match found {
            Err(error) => {
                self.error = Some(plain_error(
                    error,
                    &format!("Could not load the {word} verse."),
                ));
                return None;
            }
            Ok(None) => {
                self.error = Some(format!(
                    "No {word} verse is available in {lookup_translation}."
                ));
                return None;
            }
            Ok(Some(adjacent)) => adjacent,
        };
        if let Some(existing) = rows::find_flat(&self.rows, |c, _| {
            c.result.reference == adjacent.reference && c.result.translation == adjacent.translation
        }) {
            self.active = existing;
            self.cue = true;
            self.focus = true;
            self.persist(existing, Highlight::Live);
            self.scroll = Some(ScrollTarget::Card(existing));
            return None;
        }
        let row = self.rows.get_mut(row_index)?;
        let plan_item_id = row.plan_item_id.clone();
        let card = rows::ResultCard {
            result: adjacent.clone(),
            send_status: SendStatus::Idle,
            plan_item_id: plan_item_id.clone(),
        };
        let insert_at = match direction {
            Direction::Next => {
                row.cards.push(card);
                row.cards.len() - 1
            }
            Direction::Previous => {
                row.cards.insert(0, card);
                0
            }
        };
        if let Some(combined) = combine_scripture_results(
            &row.cards
                .iter()
                .map(|c| c.result.clone())
                .collect::<Vec<_>>(),
        ) {
            row.reference = combined.reference;
        }
        let flat = rows::flat_index_at(&self.rows, row_index, insert_at);
        self.active = flat;
        self.cue = true;
        self.focus = true;
        self.persist(flat, Highlight::Live);
        self.scroll = Some(ScrollTarget::Card(flat));
        self.added_all = None;
        if self.source == Some(Source::Plan)
            && let (Some(item), Some(plan)) = (plan_item_id, self.selected_plan().cloned())
        {
            let direction = match direction {
                Direction::Next => AdjacentDirection::Next,
                Direction::Previous => AdjacentDirection::Previous,
            };
            return Some(extend_sermon_plan_item_with_adjacent(
                plan,
                &item,
                adjacent,
                direction,
                now_ms(),
            ));
        }
        None
    }

    // ─── Playlists ──────────────────────────────────────────────────────────

    fn replace_plan(&mut self, plan: SermonPlan) {
        if let Some(slot) = self.plans.iter_mut().find(|p| p.id == plan.id) {
            *slot = plan;
        }
    }

    /// The lookup's cards back on the grid.
    fn refresh_search_view(&mut self) {
        if self.source != Some(Source::Search) {
            return;
        }
        self.rows = self.search_rows.clone();
        self.active = self
            .active
            .min(rows::card_count(&self.rows).saturating_sub(1));
    }

    fn plan(&self, id: &str) -> Option<SermonPlan> {
        self.plans.iter().find(|p| p.id == id).cloned()
    }

    /// Picks the playlist without opening it.
    pub fn set_selected_plan(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected_plan = Some(id);
        self.added_all = None;
        cx.notify();
    }

    /// `openPlan`: fills missing text, then shows every item as a row.
    pub fn open_plan(&mut self, plan_id: &str, cx: &mut Context<Self>) {
        let Some(plan) = self.plan(plan_id) else {
            return;
        };
        let backend = self.backend.clone();
        let translation = self.translation.clone();
        let translations = self.translations.clone();
        cx.spawn(async move |this, cx| {
            let opened = resolve_and_open(&backend, plan, &translation, &translations).await;
            let _ = this.update(cx, |this, cx| {
                match opened {
                    Ok(plan) => this.show_plan(plan),
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not open that playlist."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn show_plan(&mut self, plan: SermonPlan) {
        self.replace_plan(plan.clone());
        self.selected_plan = Some(plan.id.clone());
        self.view_state = Some(ViewState {
            plan_id: plan.id.clone(),
            item_id: None,
            card_in_row: 0,
            mode: Highlight::None,
        });
        self.added_all = None;
        self.active = 0;
        self.cue = false;
        self.focus = false;
        self.error = None;
        self.item_filter.clear();
        self.item_picks.clear();
        self.source = Some(Source::Plan);
        self.rows = plan_rows(&plan);
        let missing = plan.items.iter().filter(|i| !i.available).count();
        if missing > 0 {
            self.error = Some(format!(
                "{missing} playlist item{} still have no verse text. Check installed Bible translations or your managed services.",
                if missing == 1 { "" } else { "s" }
            ));
        }
    }

    /// A sidebar item: focus and scroll only, never live.
    pub fn select_plan_item(&mut self, item_id: &str, cx: &mut Context<Self>) {
        // From a lookup, the item opens its playlist first.
        if self.source != Some(Source::Plan)
            && let Some(plan) = self.selected_plan().cloned()
        {
            self.show_plan(plan);
        }
        let Some(index) = rows::find_flat(&self.rows, |_, r| {
            r.plan_item_id.as_deref() == Some(item_id)
        }) else {
            return;
        };
        self.active = index;
        self.cue = false;
        self.focus = true;
        self.persist(index, Highlight::Focus);
        if let Some((row, _)) = rows::locate(&self.rows, index) {
            self.scroll = Some(ScrollTarget::Row(row));
        }
        cx.notify();
    }

    pub fn pick_item(&mut self, id: String, gesture: PickGesture, cx: &mut Context<Self>) {
        let order: Vec<String> = self
            .filtered_items()
            .iter()
            .map(|(_, i)| i.id.clone())
            .collect();
        self.item_picks.pick(id, gesture, &order);
        cx.notify();
    }

    pub fn select_all_items(&mut self, cx: &mut Context<Self>) {
        let order: Vec<String> = self
            .filtered_items()
            .iter()
            .map(|(_, i)| i.id.clone())
            .collect();
        self.item_picks.select_all(&order);
        cx.notify();
    }

    pub fn clear_item_picks(&mut self, cx: &mut Context<Self>) {
        self.item_picks.clear();
        cx.notify();
    }

    pub fn set_item_filter(&mut self, filter: String, cx: &mut Context<Self>) {
        self.item_filter = filter;
        cx.notify();
    }

    /// Reordering is offered on the full, unfiltered list only.
    pub fn can_reorder(&self) -> bool {
        self.source == Some(Source::Plan) && self.item_filter.trim().is_empty()
    }

    /// "Use for this service".
    pub fn use_for_service(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected_plan.clone() else {
            return;
        };
        self.error = None;
        let call = ask(&self.backend, ScriptureCall::SetLivePlan(id));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(ScriptureAnswer::LivePlan(state)) => this.live_plan = Some(state),
                    Ok(_) => {}
                    Err(error) => {
                        this.error = Some(plain_error(
                            error,
                            "Could not select this playlist for the service.",
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn request_delete(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.pending_delete = id;
        cx.notify();
    }

    pub fn delete_plan(&mut self, plan_id: String, cx: &mut Context<Self>) {
        let call = ask(&self.backend, ScriptureCall::DeletePlan(plan_id.clone()));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = answer {
                    this.error = Some(plain_error(error, "Could not delete that playlist."));
                    cx.notify();
                    return;
                }
                this.plans.retain(|p| p.id != plan_id);
                let was_selected = this.selected_plan.as_deref() == Some(plan_id.as_str());
                if was_selected {
                    this.selected_plan = this.plans.first().map(|p| p.id.clone());
                }
                this.pending_delete = None;
                if this.renaming.as_deref() == Some(plan_id.as_str()) {
                    this.renaming = None;
                }
                if this.source == Some(Source::Plan) && was_selected {
                    this.rows.clear();
                    this.source = None;
                }
                if this
                    .view_state
                    .as_ref()
                    .is_some_and(|v| v.plan_id == plan_id)
                {
                    this.view_state = None;
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn start_rename(&mut self, plan_id: String, cx: &mut Context<Self>) {
        self.pending_delete = None;
        self.renaming = Some(plan_id);
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    /// `saveRenamePlan`: optimistic, put back on failure.
    pub fn save_rename(&mut self, draft: &str, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.clone() else {
            return;
        };
        let title = draft.trim().to_owned();
        if title.is_empty() {
            self.error = Some("Playlist name cannot be empty.".into());
            cx.notify();
            return;
        }
        let Some(target) = self.plan(&id) else {
            return;
        };
        self.renaming = None;
        if target.title == title {
            cx.notify();
            return;
        }
        let mut optimistic = target.clone();
        optimistic.title = title;
        optimistic.updated_at = now_ms();
        self.replace_plan(optimistic.clone());
        cx.notify();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, optimistic).await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => this.replace_plan(saved),
                    Err(error) => {
                        this.replace_plan(target);
                        this.error = Some(plain_error(error, "Could not rename that playlist."));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn loaded_results(&self) -> Vec<ScriptureResult> {
        self.rows
            .iter()
            .filter(|r| r.plan_item_id.is_none())
            .filter_map(|r| {
                combine_scripture_results(
                    &r.cards.iter().map(|c| c.result.clone()).collect::<Vec<_>>(),
                )
            })
            .filter(|r| !r.verses.is_empty())
            .collect()
    }

    /// ＋ (empty), or Save ▸ New playlist… (with what is loaded).
    pub fn create_playlist(&mut self, include_loaded: bool, cx: &mut Context<Self>) {
        if self.creating {
            return;
        }
        self.creating = true;
        self.error = None;
        cx.notify();
        let now = now_ms();
        let empty = SermonPlan {
            source_text: None,
            id: format!("sermon-{}", now as u64),
            title: "Untitled playlist".into(),
            source_file_name: "Manual".into(),
            items: Vec::new(),
            created_at: now,
            updated_at: now,
            extra: Default::default(),
        };
        let plan = if include_loaded {
            append_scripture_results_to_plan(
                empty,
                self.loaded_results(),
                &format!("manual-{}", now as u64),
                now,
            )
        } else {
            empty
        };
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, plan).await;
            let _ = this.update(cx, |this, cx| {
                this.creating = false;
                match saved {
                    Ok(plan) => {
                        this.plans.retain(|p| p.id != plan.id);
                        this.plans.insert(0, plan.clone());
                        this.selected_plan = Some(plan.id.clone());
                        this.view_state = Some(ViewState {
                            plan_id: plan.id.clone(),
                            item_id: None,
                            card_in_row: 0,
                            mode: Highlight::None,
                        });
                        this.added_all = include_loaded.then(|| plan.title.clone());
                        if this.source != Some(Source::Search) {
                            this.rows.clear();
                            this.source = Some(Source::Plan);
                        }
                        this.active = 0;
                        this.cue = false;
                        this.focus = false;
                        this.renaming = Some(plan.id);
                    }
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not create a playlist."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The playlist item a card shows and the card's verse in it.
    pub fn card_item(&self, index: usize) -> Option<(String, usize)> {
        let (row, local) = rows::locate(&self.rows, index)?;
        Some((self.rows[row].plan_item_id.clone()?, local))
    }

    /// A selected-playlist item whose verses can take new words.
    pub fn editable_item(&self, id: &str) -> Option<&SermonScriptureItem> {
        self.selected_plan()?
            .items
            .iter()
            .find(|i| i.id == id && i.available && !i.verses.is_empty())
    }

    /// Replaces a playlist item's verses with edited ones: new words,
    /// verses split into parts, given names. The playlist keeps them in place
    /// of the Bible text from then on.
    pub fn save_item_verses(
        &mut self,
        id: &str,
        verses: Vec<kairo_domain::scripture::ScriptureVerse>,
        cx: &mut Context<Self>,
    ) {
        if verses.is_empty() || verses.iter().any(|v| v.text.trim().is_empty()) {
            return;
        }
        let Some(mut plan) = self.selected_plan().cloned() else {
            return;
        };
        let Some(item) = plan.items.iter_mut().find(|i| i.id == id) else {
            return;
        };
        if item.verses == verses {
            return;
        }
        item.verses = verses;
        plan.updated_at = now_ms();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, plan).await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => {
                        let (active, cue) = (this.active, this.cue);
                        if this.source == Some(Source::Plan) {
                            this.show_plan(saved);
                        } else {
                            this.replace_plan(saved);
                        }
                        this.active = active;
                        this.cue = cue;
                        cx.emit(ScriptureEvent::Notify("Verse text saved".into()));
                    }
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not save those verses."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Picked cards as passages: one per row, multi-verse runs kept together.
    pub fn picked_results(&self) -> Vec<ScriptureResult> {
        let mut out = Vec::new();
        let mut flat = 0;
        for row in &self.rows {
            let picked: Vec<ScriptureResult> = row
                .cards
                .iter()
                .enumerate()
                .filter(|(i, _)| self.picks.contains(&(flat + i)))
                .map(|(_, c)| c.result.clone())
                .collect();
            flat += row.cards.len();
            if let Some(combined) = combine_scripture_results(&picked)
                && !combined.verses.is_empty()
            {
                out.push(combined);
            }
        }
        out
    }

    pub fn add_picked_to_playlist(&mut self, plan_id: &str, cx: &mut Context<Self>) {
        let results = self.picked_results();
        let Some(target) = self.plan(plan_id) else {
            return;
        };
        if results.is_empty() {
            return;
        }
        let now = now_ms();
        let plan = append_scripture_results_to_plan(
            target,
            results.clone(),
            &format!("manual-{}", now as u64),
            now,
        );
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, plan).await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => {
                        let n = results.len();
                        let text = format!(
                            "Added {n} passage{} to {}",
                            if n == 1 { "" } else { "s" },
                            saved.title
                        );
                        this.replace_plan(saved);
                        this.picks.clear();
                        cx.emit(ScriptureEvent::Notify(text));
                    }
                    Err(error) => {
                        this.error = Some(plain_error(
                            error,
                            "Could not add those verses to the playlist.",
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Selection bar ▸ Save to library.
    pub fn save_picked_to_library(&mut self, cx: &mut Context<Self>) {
        let results = self.picked_results();
        if results.is_empty() {
            return;
        }
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut last = None;
            let mut failed = None;
            for result in &results {
                match ask(
                    &backend,
                    ScriptureCall::Passages(PassagesCommand::Save {
                        result: result.clone(),
                        library_id: Some(DEFAULT_LIBRARY_ID.into()),
                    }),
                )
                .await
                {
                    Ok(ScriptureAnswer::Passages(p)) => last = Some(p),
                    Ok(_) => {}
                    Err(error) => {
                        failed = Some(error);
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = last {
                    this.passages = p;
                }
                if let Some(error) = failed {
                    this.error = Some(plain_error(error, "Could not save those passages."));
                } else {
                    let n = results.len();
                    this.picks.clear();
                    cx.emit(ScriptureEvent::Notify(format!(
                        "Saved {n} passage{} to your library",
                        if n == 1 { "" } else { "s" }
                    )));
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Save ▸ a playlist: every loaded passage, then open it.
    pub fn add_all_to_playlist(&mut self, plan_id: &str, cx: &mut Context<Self>) {
        let results = self.loaded_results();
        let Some(target) = self.plan(plan_id) else {
            return;
        };
        if results.is_empty() {
            return;
        }
        let now = now_ms();
        let plan = append_scripture_results_to_plan(
            target,
            results,
            &format!("manual-{}", now as u64),
            now,
        );
        let backend = self.backend.clone();
        let translation = self.translation.clone();
        let translations = self.translations.clone();
        cx.spawn(async move |this, cx| {
            let outcome = async {
                let saved = save_plan(&backend, plan).await?;
                resolve_and_open(&backend, saved, &translation, &translations).await
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(plan) => {
                        let title = plan.title.clone();
                        if this.source == Some(Source::Search) {
                            // Stay on the lookup; the playlist shows under it.
                            this.selected_plan = Some(plan.id.clone());
                            this.replace_plan(plan);
                        } else {
                            this.show_plan(plan);
                        }
                        this.added_all = Some(title);
                    }
                    Err(error) => {
                        this.error = Some(plain_error(
                            error,
                            "Could not add the loaded verses to that playlist.",
                        ))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Save ▸ a library: the selected verse, with its text.
    pub fn save_to_library(&mut self, library_id: String, cx: &mut Context<Self>) {
        let Some(card) = rows::card_at(&self.rows, self.active) else {
            return;
        };
        self.passages_call(
            PassagesCommand::Save {
                result: card.result.clone(),
                library_id: Some(library_id),
            },
            cx,
        );
    }

    fn passages_call(&mut self, command: PassagesCommand, cx: &mut Context<Self>) {
        let call = ask(&self.backend, ScriptureCall::Passages(command));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(ScriptureAnswer::Passages(p)) => this.passages = p,
                    Ok(_) => {}
                    Err(error) => tracing::error!(%error, "[Scripture] passages command failed"),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Removes several items from the open playlist.
    pub fn remove_plan_items(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let Some(mut plan) = self.selected_plan().cloned() else {
            return;
        };
        if ids.is_empty() {
            return;
        }
        plan.items.retain(|i| !ids.contains(&i.id));
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, plan).await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => {
                        this.replace_plan(saved);
                        this.rows
                            .retain(|r| r.plan_item_id.as_ref().is_none_or(|id| !ids.contains(id)));
                        this.active = 0;
                        this.item_picks.clear();
                    }
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not remove those items."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reorders the open playlist; rows follow without a new lookup.
    pub fn reorder_plan_item(
        &mut self,
        dragged: &str,
        target: &str,
        position: DropPosition,
        cx: &mut Context<Self>,
    ) {
        if dragged == target || self.source != Some(Source::Plan) {
            return;
        }
        let Some(open) = self.selected_plan().cloned() else {
            return;
        };
        let optimistic =
            reorder_sermon_plan_item(open.clone(), dragged, target, position, now_ms());
        if optimistic
            .items
            .iter()
            .map(|i| &i.id)
            .eq(open.items.iter().map(|i| &i.id))
        {
            return;
        }
        self.replace_plan(optimistic.clone());
        self.order_rows(&optimistic);
        cx.notify();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let saved = save_plan(&backend, optimistic).await;
            let _ = this.update(cx, |this, cx| {
                match saved {
                    Ok(saved) => this.replace_plan(saved),
                    Err(error) => {
                        this.replace_plan(open.clone());
                        this.order_rows(&open);
                        this.error =
                            Some(plain_error(error, "Could not save the new passage order."));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn order_rows(&mut self, plan: &SermonPlan) {
        let next: Vec<ResultRow> = plan
            .items
            .iter()
            .filter_map(|item| {
                self.rows
                    .iter()
                    .find(|r| r.plan_item_id.as_deref().unwrap_or(&r.id) == item.id)
                    .cloned()
            })
            .collect();
        if next.len() == self.rows.len() {
            self.rows = next;
        }
    }

    // ─── Sermon notes and transfer ──────────────────────────────────────────

    /// Upload sermon notes: the dialog and scan, then 079's review.
    pub fn import_notes(&mut self, cx: &mut Context<Self>) {
        if self.importing {
            return;
        }
        self.importing = true;
        self.error = None;
        cx.notify();
        let call = ask(&self.backend, ScriptureCall::ImportSermonNotes);
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                match answer {
                    Ok(ScriptureAnswer::Draft(Some(draft))) => {
                        cx.emit(ScriptureEvent::ReviewSermonNotes(draft))
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not import sermon notes."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The review's Create playlist (`confirmImport`), for 079.
    pub fn confirm_import(
        &mut self,
        draft: &SermonPlanDraft,
        title: String,
        text: String,
        items: Vec<SermonPlanDraftItem>,
        cx: &mut Context<Self>,
    ) {
        self.importing = true;
        self.error = None;
        cx.notify();
        let backend = self.backend.clone();
        let translation = self.translation.clone();
        let translations = self.translations.clone();
        let source_file_name = draft.source_file_name.clone();
        cx.spawn(async move |this, cx| {
            let outcome = async {
                let pending: Vec<SermonScriptureItem> = items
                    .into_iter()
                    .map(|i| SermonScriptureItem {
                        id: i.id,
                        reference: i.reference,
                        translation: i.translation,
                        verses: Vec::new(),
                        available: false,
                        error: None,
                        extra: Default::default(),
                    })
                    .collect();
                let b = backend.clone();
                let resolved = resolve_sermon_plan_items(
                    &pending,
                    &translation,
                    |q, t| search(&b, q, t),
                    &translations,
                )
                .await;
                let now = now_ms();
                save_plan(
                    &backend,
                    SermonPlan {
                        source_text: Some(text),
                        id: format!("sermon-{}", now as u64),
                        title,
                        source_file_name,
                        items: resolved,
                        created_at: now,
                        updated_at: now,
                        extra: Default::default(),
                    },
                )
                .await
            }
            .await;
            let _ = this.update(cx, |this, cx| {
                this.importing = false;
                match outcome {
                    Ok(plan) => {
                        this.plans.retain(|p| p.id != plan.id);
                        this.plans.insert(0, plan.clone());
                        this.show_plan(plan);
                        cx.emit(ScriptureEvent::ImportFinished(true));
                    }
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not import sermon notes."));
                        cx.emit(ScriptureEvent::ImportFinished(false));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn export_playlist(&mut self, plan_id: String, cx: &mut Context<Self>) {
        self.fire(ScriptureCall::ExportPlaylist(plan_id), cx);
    }

    pub fn import_kairo(&mut self, cx: &mut Context<Self>) {
        self.fire(ScriptureCall::ImportKairo, cx);
    }

    fn fire(&mut self, call: ScriptureCall, cx: &mut Context<Self>) {
        let call = ask(&self.backend, call);
        cx.spawn(async move |this, cx| {
            if let Err(error) = call.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    // ─── Options and library ────────────────────────────────────────────────

    /// The Options panel saves as it changes.
    pub fn change_option(&mut self, change: OptionChange, cx: &mut Context<Self>) {
        match &change {
            OptionChange::VerseNumbers(on) => self.options.show_verse_numbers = *on,
            OptionChange::ShowTranslation(on) => self.options.show_translation = *on,
            OptionChange::MaxVerses(n) => self.options.max_verses = *n,
            OptionChange::DefaultTranslation(t) => self.options.default_translation = t.clone(),
        }
        cx.notify();
        let call = ask(&self.backend, ScriptureCall::SaveOption(change));
        cx.spawn(async move |_, _| {
            if let Err(error) = call.await {
                tracing::error!(%error, "[Scripture] option save failed");
            }
        })
        .detach();
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        cx.emit(ScriptureEvent::OpenSettings);
    }

    pub fn select_library(&mut self, id: String, cx: &mut Context<Self>) {
        self.active_library = id;
        self.passage_picks.clear();
        cx.notify();
    }

    pub fn pick_passage(&mut self, id: String, gesture: PickGesture, cx: &mut Context<Self>) {
        let order: Vec<String> = self
            .visible_passages()
            .iter()
            .map(|p| p.id.clone())
            .collect();
        self.passage_picks.pick(id, gesture, &order);
        cx.notify();
    }

    pub fn select_all_passages(&mut self, cx: &mut Context<Self>) {
        let order: Vec<String> = self
            .visible_passages()
            .iter()
            .map(|p| p.id.clone())
            .collect();
        self.passage_picks.select_all(&order);
        cx.notify();
    }

    pub fn clear_passage_picks(&mut self, cx: &mut Context<Self>) {
        self.passage_picks.clear();
        cx.notify();
    }

    pub fn remove_passages(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut last = None;
            for id in ids {
                match ask(
                    &backend,
                    ScriptureCall::Passages(PassagesCommand::Remove { passage_id: id }),
                )
                .await
                {
                    Ok(ScriptureAnswer::Passages(p)) => last = Some(p),
                    Ok(_) => {}
                    Err(error) => {
                        tracing::error!(%error, "[Scripture] passage removal failed");
                        break;
                    }
                }
            }
            let _ = this.update(cx, |this, cx| {
                if let Some(p) = last {
                    this.passages = p;
                }
                this.passage_picks.clear();
                cx.notify();
            });
        })
        .detach();
    }

    /// The library section's commands; `on_created` gets the new library.
    pub fn library_command(
        &mut self,
        command: LibrariesCommand,
        cx: &mut Context<Self>,
        on_created: impl FnOnce(&mut Self, Option<String>, &mut Context<Self>) + 'static,
    ) {
        let before: Vec<String> = self
            .libraries
            .libraries
            .iter()
            .map(|l| l.id.clone())
            .collect();
        let call = ask(&self.backend, ScriptureCall::Libraries(command));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                match answer {
                    Ok(ScriptureAnswer::Libraries(state)) => {
                        let created = state
                            .libraries
                            .iter()
                            .find(|l| !before.contains(&l.id))
                            .map(|l| l.id.clone());
                        this.libraries = state;
                        on_created(this, created, cx);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        this.error = Some(plain_error(error, "Could not change that library."))
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// A library was just made: its name field should open.
    pub fn take_library_to_rename(&mut self) -> Option<String> {
        self.library_to_rename.take()
    }

    pub(crate) fn set_library_to_rename(&mut self, id: Option<String>) {
        self.library_to_rename = id;
    }

    pub fn scripture_kind() -> LibraryKind {
        LibraryKind::Scripture
    }
}

fn has_chapter_verse(q: &str) -> bool {
    let bytes = q.as_bytes();
    bytes
        .windows(3)
        .any(|w| w[1] == b':' && w[0].is_ascii_digit() && w[2].is_ascii_digit())
}

/// `openPlan`'s first half: fills items without text, saving when it changed.
async fn resolve_and_open(
    backend: &Arc<dyn ScriptureBackend>,
    plan: SermonPlan,
    translation: &str,
    translations: &[ScriptureTranslationOption],
) -> Result<SermonPlan, String> {
    let b = backend.clone();
    let items = resolve_sermon_plan_items(
        &plan.items,
        translation,
        |q, t| search(&b, q, t),
        translations,
    )
    .await;
    let changed = items.iter().enumerate().any(|(i, item)| {
        let previous = plan.items.get(i);
        previous.is_none_or(|p| {
            item.available != p.available
                || item.translation != p.translation
                || item.verses.len() != p.verses.len()
                || item.error != p.error
        })
    });
    let mut next = plan;
    next.items = items;
    if changed {
        save_plan(backend, next).await
    } else {
        Ok(next)
    }
}

/// A playlist's items as grid rows.
fn plan_rows(plan: &SermonPlan) -> Vec<ResultRow> {
    plan.items
        .iter()
        .map(|item| {
            create_result_row(
                &ScriptureResult {
                    reference: item.reference.clone(),
                    translation: item.translation.clone(),
                    verses: item.verses.clone(),
                },
                RowOptions {
                    id: Some(item.id.clone()),
                    plan_item_id: Some(item.id.clone()),
                    note: item.error.clone(),
                    ..Default::default()
                },
            )
        })
        .collect()
}

/// A reference the chosen Bible does not include, and the first Bible
/// (the default one first) that does.
#[derive(Clone, Debug, PartialEq)]
pub struct MissingPassage {
    pub reference: String,
    pub translation: String,
    pub fallback: String,
    pub results: Vec<ScriptureResult>,
}
