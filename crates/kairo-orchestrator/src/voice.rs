//! Spoken scripture navigation. From
//! `src/main/services/scripture/voice-navigation.ts`.
//!
//! "Next verse", "previous verse" and "switch to the NIV" step from the verse
//! on screen or the passage just detected. Each finalized command is consumed
//! once, keyed by provider connection and audio offset. Commands run one at a
//! time on a worker task; a reset, a cleared program or a program revision the
//! command did not cause drops a command still waiting on its lookup.

use kairo_domain::js::number_to_string;
use kairo_domain::scripture::spoken_translations::find_spoken_translation_matches;
use kairo_domain::scripture::{ScriptureSuggestion, ScriptureVerse};
use kairo_domain::transcript::TranscriptResult;
use regex::Regex;
use std::collections::{HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock, Mutex};
use tokio::sync::{mpsc, oneshot};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, PartialEq)]
pub struct LiveScriptureContext {
    pub book: String,
    pub chapter: f64,
    pub verse_start: f64,
    pub verse_end: f64,
    pub translation: String,
    pub revision: i64,
    /// The detected row this verse belongs to, so spoken steps extend it.
    pub passage_id: Option<String>,
    pub passage_index: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum VoiceCommand {
    Step(i8),
    /// "Continue": the next verse of the main reading, which after an aside
    /// is back where that reading paused.
    Resume,
    Translation(String),
}

/// Words a preacher puts in front of a command that change nothing.
static LEAD_IN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:okay|ok|alright|all right|so|and|now|then|right|please|let's|let us) )+")
        .unwrap()
});
static NEXT_VERSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^(?:{})(?: please)?$",
        [
            "(?:(?:go|move|read) (?:on )?(?:to )?)?(?:the )?next(?: verse| one)?",
            "move on",
            "(?:the )?following verse",
        ]
        .join("|")
    ))
    .unwrap()
});
static RESUME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^(?:go on|carry on|keep going|continue)(?: please)?$").unwrap());
static PREVIOUS_VERSE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        "^(?:{})(?: please)?$",
        [
            "(?:(?:go back|back|read) (?:to )?)?(?:the )?previous(?: verse| one)?",
            "go back(?: (?:a|one)(?: verse)?)?",
            "back(?: (?:a|one)(?: verse)?)?",
            "(?:the )?verse before",
        ]
        .join("|")
    ))
    .unwrap()
});
static SWITCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:please )?(?:switch to|read (?:it|that) in|show (?:it|that) in) (.+?)(?: please)?$",
    )
    .unwrap()
});

/// The whole phrase must be the command, so narration stays narration.
pub fn parse_voice_command(text: &str) -> Option<VoiceCommand> {
    let stripped: String = text
        .to_lowercase()
        .chars()
        .filter(|c| !matches!(c, '.' | '!' | '?' | ','))
        .collect();
    let collapsed = stripped
        .split(kairo_domain::js::is_whitespace)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let phrase = LEAD_IN.replace(&collapsed, "");
    if NEXT_VERSE.is_match(&phrase) {
        return Some(VoiceCommand::Step(1));
    }
    if RESUME.is_match(&phrase) {
        return Some(VoiceCommand::Resume);
    }
    if PREVIOUS_VERSE.is_match(&phrase) {
        return Some(VoiceCommand::Step(-1));
    }
    let named = SWITCH.captures(&phrase)?.get(1)?.as_str();
    let length = named.encode_utf16().count();
    let whole: Vec<_> = find_spoken_translation_matches(named)
        .into_iter()
        .filter(|m| m.start == 0 && m.end == length)
        .collect();
    match whole.as_slice() {
        [only] => Some(VoiceCommand::Translation(only.id.clone())),
        _ => None,
    }
}

/// A detected passage as the place the next command steps from. Ambiguous
/// and provisional matches are not a place to step from.
pub fn follow_up_context(
    suggestion: &ScriptureSuggestion,
    revision: i64,
) -> Option<LiveScriptureContext> {
    let first = suggestion.verses.first()?;
    let last = suggestion.verses.last()?;
    if suggestion.ambiguous == Some(true) || suggestion.provisional == Some(true) {
        return None;
    }
    Some(LiveScriptureContext {
        book: first.book.clone(),
        chapter: first.chapter,
        verse_start: first.verse,
        verse_end: if last.chapter == first.chapter {
            last.verse
        } else {
            first.verse
        },
        translation: suggestion.translation.clone(),
        revision,
        passage_id: Some(
            suggestion
                .passage_id
                .clone()
                .unwrap_or_else(|| suggestion.id.clone()),
        ),
        passage_index: Some(suggestion.passage_index.unwrap_or(0.0)),
    })
}

/// Whichever came last: the verse put on screen or the passage just detected.
pub fn pick_command_context(
    live: Option<(LiveScriptureContext, f64)>,
    follow_up: Option<(LiveScriptureContext, f64)>,
) -> Option<LiveScriptureContext> {
    match (live, follow_up) {
        (Some((live, live_at)), Some((follow, follow_at))) => {
            Some(if follow_at > live_at { follow } else { live })
        }
        (Some((live, _)), None) => Some(live),
        (None, follow) => follow.map(|(c, _)| c),
    }
}

/// The translation being read, for a verse in the same book and the same or
/// a neighbouring chapter.
pub fn reading_translation(
    context: Option<&LiveScriptureContext>,
    book: &str,
    chapter: f64,
) -> Option<String> {
    let context = context?;
    (context.book == book && (context.chapter - chapter).abs() <= 1.0)
        .then(|| context.translation.clone())
}

/// The detected verse is exactly what is already on screen.
pub fn is_on_screen(
    live: Option<&LiveScriptureContext>,
    book: &str,
    chapter: f64,
    verse_start: f64,
    verse_end: Option<f64>,
    translation: &str,
) -> bool {
    live.is_some_and(|live| {
        live.book == book
            && live.chapter == chapter
            && live.translation == translation
            && live.verse_start == verse_start
            && live.verse_end == verse_end.unwrap_or(verse_start)
    })
}

/// What navigation needs from the orchestrator.
pub trait VoiceDeps: Send + Sync + 'static {
    fn enabled(&self) -> bool;
    fn auto_mode(&self) -> bool;
    fn current(&self) -> Option<LiveScriptureContext>;
    fn chapter(
        &self,
        book: String,
        chapter: f64,
        translation: String,
    ) -> BoxFuture<'_, Result<Vec<ScriptureVerse>, String>>;
    /// Where the main reading paused, while an aside is the newest reading:
    /// book, chapter and verse.
    fn paused_reading(&self) -> BoxFuture<'_, Option<(String, f64, f64)>>;
    fn present(&self, suggestion: ScriptureSuggestion) -> BoxFuture<'_, ()>;
    fn suggest(&self, suggestion: ScriptureSuggestion);
    /// Switch a whole detected row to another translation in place.
    /// `false` when the host has no rows to switch.
    fn retranslate_row(&self, passage_id: &str, translation: &str) -> bool;
    fn error(&self, error: String);
}

const MEMORY: usize = 1024;

#[derive(Default)]
struct State {
    consumed: HashSet<String>,
    consumed_order: VecDeque<String>,
    generation: u64,
    own_revisions: VecDeque<(i64, i64)>,
}

impl State {
    fn own(&self, root: i64) -> Option<i64> {
        self.own_revisions
            .iter()
            .find(|(r, _)| *r == root)
            .map(|(_, v)| *v)
    }

    fn set_own(&mut self, root: i64, revision: i64) {
        if let Some(entry) = self.own_revisions.iter_mut().find(|(r, _)| *r == root) {
            entry.1 = revision;
        } else {
            self.own_revisions.push_back((root, revision));
        }
    }
}

type Job = BoxFuture<'static, ()>;

#[derive(Clone)]
pub struct VoiceNavigation {
    deps: Arc<dyn VoiceDeps>,
    state: Arc<Mutex<State>>,
    queue: mpsc::UnboundedSender<Job>,
}

/// `Book C:S` or `Book C:S-E`, with JavaScript number text.
fn reference_text(book: &str, chapter: f64, start: f64, end: f64) -> String {
    let range = if end != start {
        format!("-{}", number_to_string(end))
    } else {
        String::new()
    };
    format!(
        "{book} {}:{}{range}",
        number_to_string(chapter),
        number_to_string(start)
    )
}

impl VoiceNavigation {
    /// Spawns the command worker on the current runtime.
    pub fn new(deps: Arc<dyn VoiceDeps>) -> Self {
        let (queue, mut jobs) = mpsc::unbounded_channel::<Job>();
        tokio::spawn(async move {
            while let Some(job) = jobs.recv().await {
                job.await;
            }
        });
        Self {
            deps,
            state: Arc::new(Mutex::new(State::default())),
            queue,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn reset(&self) {
        let mut state = self.lock();
        state.generation += 1;
        state.consumed.clear();
        state.consumed_order.clear();
        state.own_revisions.clear();
    }

    /// `true` when `result` is a navigation command, consumed or queued.
    pub fn claim(&self, result: &TranscriptResult) -> bool {
        // The cheap parse runs first: most finals are not commands.
        let Some(command) = parse_voice_command(&result.text) else {
            return false;
        };
        if !self.deps.enabled() {
            return false;
        }
        let Some(expected) = self.deps.current() else {
            return false;
        };
        let key = kairo_stt::recording::transcript_evidence_key(
            result.audio_epoch,
            result.audio_start,
            &result.id,
        );
        let generation = {
            let mut state = self.lock();
            if state.consumed.contains(&key) {
                return true;
            }
            state.consumed.insert(key.clone());
            state.consumed_order.push_back(key.clone());
            if state.consumed_order.len() > MEMORY
                && let Some(oldest) = state.consumed_order.pop_front()
            {
                state.consumed.remove(&oldest);
            }
            state.generation
        };
        let this = self.clone();
        let text = result.text.clone();
        let job: Job = Box::pin(async move {
            this.run(command, expected, generation, key, text).await;
        });
        let _ = self.queue.send(job);
        true
    }

    /// Resolves once every command queued so far has finished.
    pub async fn settled(&self) {
        let (tx, rx) = oneshot::channel();
        let job: Job = Box::pin(async move {
            let _ = tx.send(());
        });
        if self.queue.send(job).is_ok() {
            let _ = rx.await;
        }
    }

    fn generation(&self) -> u64 {
        self.lock().generation
    }

    async fn run(
        &self,
        command: VoiceCommand,
        expected: LiveScriptureContext,
        generation: u64,
        key: String,
        text: String,
    ) {
        if generation != self.generation() || !self.deps.enabled() {
            return;
        }
        let Some(current) = self.deps.current() else {
            return;
        };
        if current.revision != expected.revision
            && self.lock().own(expected.revision) != Some(current.revision)
        {
            return;
        }
        let translation = match &command {
            VoiceCommand::Translation(t) => t.clone(),
            VoiceCommand::Step(_) | VoiceCommand::Resume => current.translation.clone(),
        };
        let command = match command {
            VoiceCommand::Resume => match self.deps.paused_reading().await {
                Some(paused) => {
                    return self
                        .resume(paused, &current, generation, &key, text, translation)
                        .await;
                }
                None => VoiceCommand::Step(1),
            },
            command => command,
        };
        // A translation switch changes its row; only in Auto mode does the
        // verse also go back up in the new translation.
        if let (VoiceCommand::Translation(_), Some(passage)) = (&command, &current.passage_id)
            && self.deps.retranslate_row(passage, &translation)
            && !self.deps.auto_mode()
        {
            return;
        }
        let mut chapter = current.chapter;
        let lookup = |chapter: f64| {
            self.deps
                .chapter(current.book.clone(), chapter, translation.clone())
        };
        let mut verses = match lookup(chapter).await {
            Ok(v) => v,
            Err(e) => return self.deps.error(e),
        };
        let selected: Vec<ScriptureVerse> = match &command {
            VoiceCommand::Translation(_) => verses
                .into_iter()
                .filter(|v| v.verse >= current.verse_start && v.verse <= current.verse_end)
                .collect(),
            VoiceCommand::Resume => unreachable!("resolved to a step above"),
            VoiceCommand::Step(direction) => {
                let forward = *direction == 1;
                let mut picked: Vec<_> = verses
                    .drain(..)
                    .filter(|v| {
                        if forward {
                            v.verse > current.verse_end
                        } else {
                            v.verse < current.verse_start
                        }
                    })
                    .collect();
                if picked.is_empty() {
                    chapter += f64::from(*direction);
                    if chapter < 1.0 {
                        return;
                    }
                    picked = match lookup(chapter).await {
                        Ok(v) => v,
                        Err(e) => return self.deps.error(e),
                    };
                }
                let one = if forward {
                    picked.first()
                } else {
                    picked.last()
                };
                one.cloned().into_iter().collect()
            }
        };
        if selected.is_empty()
            || generation != self.generation()
            || !self.deps.enabled()
            || self.deps.current().map(|c| c.revision) != Some(current.revision)
        {
            return;
        }
        let start = selected[0].verse;
        let end = selected[selected.len() - 1].verse;
        let passage_index = current.passage_id.as_ref().map(|_| {
            current.passage_index.unwrap_or(0.0)
                + match &command {
                    VoiceCommand::Translation(_) | VoiceCommand::Resume => 0.0,
                    VoiceCommand::Step(d) => f64::from(*d),
                }
        });
        let suggestion = crate::suggestion::manual(
            format!("voice-{generation}-{key}"),
            reference_text(&current.book, chapter, start, end),
            selected,
            translation.clone(),
            text,
            current.passage_id.clone(),
            passage_index,
        );
        self.deliver(
            suggestion,
            &current,
            chapter,
            start,
            translation,
            generation,
        )
        .await;
    }

    /// Back to the main reading after an aside, at the verse it paused on.
    async fn resume(
        &self,
        (book, chapter, verse): (String, f64, f64),
        current: &LiveScriptureContext,
        generation: u64,
        key: &str,
        text: String,
        translation: String,
    ) {
        let verses = match self
            .deps
            .chapter(book.clone(), chapter, translation.clone())
            .await
        {
            Ok(v) => v,
            Err(e) => return self.deps.error(e),
        };
        let selected: Vec<ScriptureVerse> =
            verses.into_iter().filter(|v| v.verse == verse).collect();
        if selected.is_empty()
            || generation != self.generation()
            || !self.deps.enabled()
            || self.deps.current().map(|c| c.revision) != Some(current.revision)
        {
            return;
        }
        let suggestion = crate::suggestion::manual(
            format!("voice-{generation}-{key}"),
            reference_text(&book, chapter, verse, verse),
            selected,
            translation.clone(),
            text,
            None,
            None,
        );
        let target = LiveScriptureContext {
            book,
            ..current.clone()
        };
        self.deliver(suggestion, &target, chapter, verse, translation, generation)
            .await;
    }

    /// Presents in Auto mode, else leaves a card; a step presented here is
    /// remembered as this command's own revision.
    async fn deliver(
        &self,
        suggestion: ScriptureSuggestion,
        current: &LiveScriptureContext,
        chapter: f64,
        start: f64,
        translation: String,
        generation: u64,
    ) {
        if self.deps.auto_mode() {
            self.deps.present(suggestion).await;
            let after = self.deps.current();
            if generation == self.generation()
                && let Some(after) = after
                && after.book == current.book
                && after.chapter == chapter
                && after.verse_start == start
                && after.translation == translation
            {
                let mut state = self.lock();
                for entry in state.own_revisions.iter_mut() {
                    if entry.1 == current.revision {
                        entry.1 = after.revision;
                    }
                }
                state.set_own(current.revision, after.revision);
                if state.own_revisions.len() > MEMORY {
                    state.own_revisions.pop_front();
                }
            }
        } else {
            self.deps.suggest(suggestion);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn final_result(text: &str, id: &str) -> TranscriptResult {
        serde_json::from_value(serde_json::json!({
            "text": text, "id": id, "words": [], "timestamp": 0, "duration": 1, "isFinal": true
        }))
        .unwrap()
    }

    fn verse(book: &str, chapter: f64, n: f64) -> ScriptureVerse {
        serde_json::from_value(serde_json::json!({ "book": book, "chapter": chapter, "verse": n, "text": format!("text {n}") })).unwrap()
    }

    type Lookup = Box<
        dyn Fn(&str, f64) -> BoxFuture<'static, Result<Vec<ScriptureVerse>, String>> + Send + Sync,
    >;

    struct Harness {
        auto: bool,
        current: Mutex<Option<LiveScriptureContext>>,
        presented: Mutex<Vec<ScriptureSuggestion>>,
        suggested: Mutex<Vec<ScriptureSuggestion>>,
        lookup: Lookup,
        enabled: AtomicBool,
        paused: Mutex<Option<(String, f64, f64)>>,
    }

    impl VoiceDeps for Harness {
        fn enabled(&self) -> bool {
            self.enabled.load(Ordering::SeqCst)
        }
        fn auto_mode(&self) -> bool {
            self.auto
        }
        fn current(&self) -> Option<LiveScriptureContext> {
            self.current.lock().unwrap().clone()
        }
        fn chapter(
            &self,
            book: String,
            chapter: f64,
            _t: String,
        ) -> BoxFuture<'_, Result<Vec<ScriptureVerse>, String>> {
            (self.lookup)(&book, chapter)
        }
        fn paused_reading(&self) -> BoxFuture<'_, Option<(String, f64, f64)>> {
            let paused = self.paused.lock().unwrap().clone();
            Box::pin(async move { paused })
        }
        fn present(&self, s: ScriptureSuggestion) -> BoxFuture<'_, ()> {
            Box::pin(async move {
                let mut current = self.current.lock().unwrap();
                if let Some(c) = current.as_mut() {
                    c.verse_start = s.verses[0].verse;
                    c.verse_end = s.verses[0].verse;
                    c.revision += 1;
                }
                drop(current);
                self.presented.lock().unwrap().push(s);
            })
        }
        fn suggest(&self, s: ScriptureSuggestion) {
            self.suggested.lock().unwrap().push(s);
        }
        fn retranslate_row(&self, _p: &str, _t: &str) -> bool {
            false
        }
        fn error(&self, _e: String) {}
    }

    fn harness(auto: bool, lookup: Option<Lookup>) -> (Arc<Harness>, VoiceNavigation) {
        let h = Arc::new(Harness {
            auto,
            current: Mutex::new(Some(LiveScriptureContext {
                book: "John".into(),
                chapter: 3.0,
                verse_start: 16.0,
                verse_end: 16.0,
                translation: "KJV".into(),
                revision: 1,
                passage_id: None,
                passage_index: None,
            })),
            presented: Mutex::default(),
            suggested: Mutex::default(),
            lookup: lookup.unwrap_or_else(|| {
                Box::new(|book: &str, chapter| {
                    let v: Vec<_> = [16.0, 17.0, 18.0]
                        .into_iter()
                        .map(|n| verse(book, chapter, n))
                        .collect();
                    Box::pin(async move { Ok(v) })
                })
            }),
            enabled: AtomicBool::new(true),
            paused: Mutex::default(),
        });
        let nav = VoiceNavigation::new(h.clone());
        (h, nav)
    }

    fn refs(list: &Mutex<Vec<ScriptureSuggestion>>) -> Vec<String> {
        list.lock()
            .unwrap()
            .iter()
            .map(|s| s.reference.clone())
            .collect()
    }

    #[test]
    fn narration_and_corrections_are_not_commands() {
        for text in [
            "do not go to the next verse",
            "next verse is what he said",
            "next verse no stay here",
            "switch to KJV but not yet",
            "Next week we will look at Romans.",
            "Go on and tell your neighbour.",
            "Continue to pray for the family.",
            "Back in the day.",
        ] {
            assert_eq!(parse_voice_command(text), None, "{text}");
        }
    }

    #[test]
    fn whole_commands_and_spelled_translations_parse() {
        assert_eq!(
            parse_voice_command("Next verse."),
            Some(VoiceCommand::Step(1))
        );
        assert_eq!(
            parse_voice_command("Go back to the previous verse."),
            Some(VoiceCommand::Step(-1))
        );
        assert_eq!(
            parse_voice_command("Switch to K J V."),
            Some(VoiceCommand::Translation("KJV".into()))
        );
        for (text, d) in [
            ("Okay, next verse.", 1),
            ("Let's go to the next verse.", 1),
            ("Read the next verse, please.", 1),
            ("And next verse.", 1),
            ("Alright, previous verse.", -1),
            ("Go back a verse.", -1),
            ("Let's go back to the previous verse.", -1),
            ("Back to the previous verse.", -1),
            ("Next.", 1),
            ("Next one.", 1),
            ("Move on.", 1),
            ("Following verse.", 1),
            ("And the next one.", 1),
            ("Previous.", -1),
            ("Previous one.", -1),
            ("Back.", -1),
            ("Back one.", -1),
            ("The verse before.", -1),
            ("Go back one.", -1),
        ] {
            assert_eq!(
                parse_voice_command(text),
                Some(VoiceCommand::Step(d)),
                "{text}"
            );
        }
    }

    #[test]
    fn continue_is_a_resume() {
        for text in [
            "Go on.",
            "Continue.",
            "Keep going.",
            "Carry on.",
            "Okay, go on.",
        ] {
            assert_eq!(
                parse_voice_command(text),
                Some(VoiceCommand::Resume),
                "{text}"
            );
        }
    }

    #[tokio::test]
    async fn continue_steps_on_in_the_main_reading_and_returns_to_it_after_an_aside() {
        let (h, nav) = harness(true, None);
        nav.claim(&final_result("Continue.", "one"));
        nav.settled().await;
        assert_eq!(refs(&h.presented), ["John 3:17"]);
        *h.paused.lock().unwrap() = Some(("Luke".into(), 1.0, 18.0));
        nav.claim(&final_result("Continue.", "two"));
        nav.settled().await;
        assert_eq!(refs(&h.presented), ["John 3:17", "Luke 1:18"]);
    }

    #[tokio::test]
    async fn manual_commands_suggest_once_without_presenting() {
        let (h, nav) = harness(false, None);
        nav.claim(&final_result("Next verse", "same"));
        nav.claim(&final_result("Next verse", "same"));
        nav.settled().await;
        assert_eq!(refs(&h.suggested), ["John 3:17"]);
        assert!(h.presented.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_reset_or_cleared_program_drops_queued_commands() {
        let (h, nav) = harness(true, None);
        nav.claim(&final_result("Next verse", "a"));
        nav.reset();
        nav.settled().await;
        assert!(h.presented.lock().unwrap().is_empty());
        let (h, nav) = harness(true, None);
        nav.claim(&final_result("Next verse", "a"));
        *h.current.lock().unwrap() = None;
        nav.settled().await;
        assert!(h.presented.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn two_queued_commands_advance_twice() {
        let (h, nav) = harness(true, None);
        nav.claim(&final_result("Next verse", "one"));
        nav.claim(&final_result("Next verse", "two"));
        nav.settled().await;
        assert_eq!(refs(&h.presented), ["John 3:17", "John 3:18"]);
    }

    #[tokio::test]
    async fn next_crosses_chapters_on_actual_verse_numbers() {
        let lookup: Lookup = Box::new(|book: &str, chapter| {
            let nums: &[f64] = if chapter == 3.0 { &[16.0] } else { &[1.0, 3.0] };
            let v: Vec<_> = nums.iter().map(|n| verse(book, chapter, *n)).collect();
            Box::pin(async move { Ok(v) })
        });
        let (h, nav) = harness(false, Some(lookup));
        nav.claim(&final_result("Next verse", "x"));
        nav.settled().await;
        let suggested = h.suggested.lock().unwrap();
        assert_eq!(
            (
                suggested[0].reference.as_str(),
                suggested[0].translation.as_str()
            ),
            ("John 4:1", "KJV")
        );
    }

    #[tokio::test]
    async fn an_unavailable_adjacent_chapter_changes_nothing() {
        let lookup: Lookup = Box::new(|book: &str, chapter| {
            let v = if chapter == 3.0 {
                vec![verse(book, chapter, 16.0)]
            } else {
                Vec::new()
            };
            Box::pin(async move { Ok(v) })
        });
        let (h, nav) = harness(true, Some(lookup));
        nav.claim(&final_result("Next verse", "x"));
        nav.settled().await;
        assert!(h.presented.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_program_cleared_during_lookup_is_not_overwritten() {
        let gate = Arc::new(tokio::sync::Notify::new());
        let g = gate.clone();
        let lookup: Lookup = Box::new(move |book: &str, chapter| {
            let g = g.clone();
            let v = vec![verse(book, chapter, 17.0)];
            Box::pin(async move {
                g.notified().await;
                Ok(v)
            })
        });
        let (h, nav) = harness(true, Some(lookup));
        nav.claim(&final_result("Next verse", "x"));
        tokio::task::yield_now().await;
        *h.current.lock().unwrap() = None;
        gate.notify_one();
        nav.settled().await;
        assert!(h.presented.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn the_same_offset_on_a_new_connection_is_fresh() {
        let (h, nav) = harness(true, None);
        let mut one = final_result("Next verse", "one");
        one.audio_start = Some(0.0);
        one.audio_epoch = Some(1.0);
        nav.claim(&one);
        nav.settled().await;
        let mut two = final_result("Next verse", "two");
        two.audio_start = Some(0.0);
        two.audio_epoch = Some(2.0);
        nav.claim(&two);
        nav.settled().await;
        assert_eq!(refs(&h.presented), ["John 3:17", "John 3:18"]);
    }

    fn suggestion(verses: Vec<ScriptureVerse>) -> ScriptureSuggestion {
        crate::suggestion::manual(
            "s".into(),
            "Matthew 1:8-9".into(),
            verses,
            "NKJV".into(),
            String::new(),
            None,
            None,
        )
    }

    #[test]
    fn a_detected_passage_is_a_starting_point() {
        let s = suggestion(vec![verse("Matthew", 1.0, 8.0), verse("Matthew", 1.0, 9.0)]);
        let ctx = follow_up_context(&s, -3).unwrap();
        assert_eq!(
            (ctx.verse_start, ctx.verse_end, ctx.revision),
            (8.0, 9.0, -3)
        );
        assert_eq!(
            (ctx.passage_id.as_deref(), ctx.passage_index),
            (Some("s"), Some(0.0))
        );
        let mut ambiguous = s.clone();
        ambiguous.ambiguous = Some(true);
        assert_eq!(follow_up_context(&ambiguous, -4), None);
        assert_eq!(follow_up_context(&suggestion(Vec::new()), -5), None);
    }

    #[test]
    fn reading_translation_and_on_screen() {
        let ampc = LiveScriptureContext {
            book: "John".into(),
            chapter: 5.0,
            verse_start: 30.0,
            verse_end: 30.0,
            translation: "AMPC".into(),
            revision: 1,
            passage_id: None,
            passage_index: None,
        };
        assert_eq!(
            reading_translation(Some(&ampc), "John", 5.0).as_deref(),
            Some("AMPC")
        );
        assert_eq!(
            reading_translation(Some(&ampc), "John", 6.0).as_deref(),
            Some("AMPC")
        );
        assert_eq!(reading_translation(Some(&ampc), "Romans", 5.0), None);
        assert_eq!(reading_translation(Some(&ampc), "John", 14.0), None);
        assert_eq!(reading_translation(None, "John", 5.0), None);
        assert!(is_on_screen(Some(&ampc), "John", 5.0, 30.0, None, "AMPC"));
        assert!(!is_on_screen(Some(&ampc), "John", 5.0, 30.0, None, "NKJV"));
        assert!(!is_on_screen(Some(&ampc), "John", 5.0, 31.0, None, "AMPC"));
        assert!(!is_on_screen(None, "John", 5.0, 30.0, None, "AMPC"));
    }

    #[test]
    fn pick_prefers_the_latest() {
        let ctx = |r| LiveScriptureContext {
            book: "John".into(),
            chapter: 1.0,
            verse_start: 1.0,
            verse_end: 1.0,
            translation: "KJV".into(),
            revision: r,
            passage_id: None,
            passage_index: None,
        };
        assert_eq!(
            pick_command_context(Some((ctx(1), 10.0)), Some((ctx(-1), 20.0)))
                .unwrap()
                .revision,
            -1
        );
        assert_eq!(
            pick_command_context(Some((ctx(1), 30.0)), Some((ctx(-1), 20.0)))
                .unwrap()
                .revision,
            1
        );
        assert_eq!(
            pick_command_context(None, Some((ctx(-1), 20.0)))
                .unwrap()
                .revision,
            -1
        );
        assert_eq!(pick_command_context(None, None), None);
    }
}
