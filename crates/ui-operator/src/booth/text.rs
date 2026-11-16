//! Pure helpers the booth views share: transcript highlighting, card states,
//! verse labels, timer text, nuggets and key names. Ported from
//! `Operator.tsx`, `ProgramPanels.tsx` and `OperatorToolbar.tsx`.

use std::collections::HashSet;

use kairo_domain::program::format_clock;
use kairo_domain::scripture::{PlanMatch, ScriptureSuggestion, ScriptureVerse, SuggestionSource};
use serde_json::Value;

use crate::booth::backend::LegacyNugget;

/// `TRIGGER_PHRASES`: words that usually lead into a reading.
pub const TRIGGER_PHRASES: [&str; 9] = [
    "turn your bibles to",
    "turn to",
    "let's read",
    "the scripture says",
    "the bible says",
    "it is written",
    "as it says in",
    "chapter",
    "verse",
];

/// `NUGGET_STORAGE_KEY`.
pub const NUGGET_STORAGE_KEY: &str = "kairo-message-nuggets";

/// The transcript keeps its last 50 segments.
pub const MAX_SEGMENTS: usize = 50;

#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub id: String,
    pub trigger_text: String,
    pub reference: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextRun {
    Plain(String),
    Trigger(String),
    Scripture { text: String, reference: String },
}

#[derive(Clone, PartialEq)]
enum Style {
    Trigger,
    Scripture(String),
}

/// `buildRuns`: trigger phrases in bold teal, matched quotes tinted with
/// their reference. Works on characters, as the source did on UTF-16 units.
pub fn build_runs(text: &str, highlights: &[Highlight]) -> Vec<TextRun> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let lower: Vec<char> = text.to_lowercase().chars().collect();
    // Lower-casing can change the length (rare letters); fall back to plain.
    if lower.len() != chars.len() {
        return vec![TextRun::Plain(text.into())];
    }
    let mut styles: Vec<Option<Style>> = vec![None; chars.len()];
    let find = |needle: &[char], from: usize| -> Option<usize> {
        if needle.is_empty() || needle.len() > lower.len() {
            return None;
        }
        (from..=lower.len() - needle.len()).find(|&i| lower[i..i + needle.len()] == *needle)
    };
    for phrase in TRIGGER_PHRASES {
        let needle: Vec<char> = phrase.chars().collect();
        let mut pos = 0;
        while let Some(at) = find(&needle, pos) {
            for style in styles.iter_mut().skip(at).take(needle.len()) {
                if style.is_none() {
                    *style = Some(Style::Trigger);
                }
            }
            pos = at + needle.len();
        }
    }
    for highlight in highlights {
        let needle: Vec<char> = highlight.trigger_text.to_lowercase().chars().collect();
        if let Some(at) = find(&needle, 0) {
            for style in styles.iter_mut().skip(at).take(needle.len()) {
                *style = Some(Style::Scripture(highlight.reference.clone()));
            }
        }
    }
    let mut runs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let style = styles[i].clone();
        let mut j = i + 1;
        while j < chars.len() && styles[j] == style {
            j += 1;
        }
        let piece: String = chars[i..j].iter().collect();
        runs.push(match style {
            None => TextRun::Plain(piece),
            Some(Style::Trigger) => TextRun::Trigger(piece),
            Some(Style::Scripture(reference)) => TextRun::Scripture {
                text: piece,
                reference,
            },
        });
        i = j;
    }
    runs
}

/// `DetectionState`: the one state a detected verse shows, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetectionState {
    Live,
    Reading,
    Next,
}

pub fn detection_state(live: bool, reading: bool, up_next: bool) -> Option<DetectionState> {
    if live {
        Some(DetectionState::Live)
    } else if reading {
        Some(DetectionState::Reading)
    } else if up_next {
        Some(DetectionState::Next)
    } else {
        None
    }
}

/// `detectionSource`: where the verse came from.
pub fn detection_source(suggestion: &ScriptureSuggestion) -> Option<&'static str> {
    match suggestion.plan_match {
        Some(PlanMatch::Quote) => Some("Sermon · matched"),
        Some(_) => Some("From sermon"),
        None if suggestion.preloaded_next == Some(true) => Some("Preloaded"),
        None => None,
    }
}

/// `${book} ${chapter}:${verse}`.
pub fn verse_reference(verse: &ScriptureVerse) -> String {
    format!(
        "{} {}:{}",
        verse.book,
        number(verse.chapter),
        number(verse.verse)
    )
}

/// A whole number without `.0`, as JavaScript prints it.
pub fn number(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// A verse chip's label in a queued passage: a step across a chapter keeps
/// its chapter, so "2:1" never reads as "1".
pub fn queue_chip_label(verse: &ScriptureVerse, first: Option<&ScriptureVerse>) -> String {
    if first.is_some_and(|first| first.chapter != verse.chapter) {
        format!("{}:{}", number(verse.chapter), number(verse.verse))
    } else {
        number(verse.verse)
    }
}

/// The same for a detected passage, read from the card's reference.
pub fn detected_chip_label(reference: &str, first_reference: Option<&str>) -> String {
    let tail = |r: &str| r.rsplit(' ').next().unwrap_or("").to_owned();
    let own = tail(reference);
    let mut parts = own.splitn(2, ':');
    let chapter = parts.next().unwrap_or("");
    let verse = parts.next().unwrap_or("");
    let first_chapter = first_reference
        .map(tail)
        .and_then(|t| t.split(':').next().map(str::to_owned));
    if !chapter.is_empty() && first_chapter.as_deref() != Some(chapter) {
        format!("{chapter}:{verse}")
    } else {
        verse.to_owned()
    }
}

/// `projectedIds`: on screen means this verse in this translation. The
/// wording decides; when no wording matches, the reference alone does.
pub fn projected_ids(
    projection: Option<&(String, String)>,
    suggestions: &[ScriptureSuggestion],
) -> HashSet<String> {
    let Some((reference, text)) = projection else {
        return HashSet::new();
    };
    // The program reports `John 3:16 (NKJV)` when the translation shows on
    // screen; the row holds `John 3:16`.
    let key = |r: &str| -> String {
        let r = r.trim();
        match r.rfind(" (") {
            Some(i) if r.ends_with(')') => r[..i].trim().to_owned(),
            _ => r.to_owned(),
        }
    };
    let live = key(reference);
    let same_reference: Vec<_> = suggestions
        .iter()
        .filter(|item| key(&item.reference) == live)
        .collect();
    let same_wording: Vec<_> = same_reference
        .iter()
        .filter(|item| {
            item.verses
                .first()
                .is_some_and(|verse| !verse.text.is_empty() && text.contains(&verse.text))
        })
        .copied()
        .collect();
    let chosen = if same_wording.is_empty() {
        same_reference
    } else {
        same_wording
    };
    chosen.into_iter().map(|item| item.id.clone()).collect()
}

/// `currentSlide` split into reference and text, or "Projected".
pub fn split_slide(slide: &str) -> (String, String) {
    // `/^(.*?\d+:\d+)\s*[\r\n]+([\s\S]+)$/`
    if let Some(line_break) = slide.find(['\r', '\n']) {
        let head = slide[..line_break].trim_end();
        let rest = slide[line_break..].trim_start_matches(['\r', '\n']);
        let ends_with_ref = head.rsplit_once(':').is_some_and(|(left, right)| {
            !right.is_empty()
                && right.bytes().all(|b| b.is_ascii_digit())
                && left.bytes().last().is_some_and(|b| b.is_ascii_digit())
        });
        if ends_with_ref && !rest.is_empty() {
            return (head.to_owned(), rest.to_owned());
        }
    }
    ("Projected".into(), slide.to_owned())
}

/// `storedNuggets`: the legacy list, keeping only well-formed entries.
pub fn parse_legacy_nuggets(value: Option<&Value>) -> Vec<LegacyNugget> {
    let parsed = match value {
        // `localStorage` held a JSON string.
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok(),
        Some(other) => Some(other.clone()),
        None => None,
    };
    let Some(Value::Array(items)) = parsed else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            Some(LegacyNugget {
                id: item.get("id")?.as_str()?.to_owned(),
                text: item.get("text")?.as_str()?.to_owned(),
                captured_at: item.get("capturedAt")?.as_f64()?,
            })
        })
        .collect()
}

/// The legacy list as `localStorage` held it (a JSON string).
pub fn legacy_nuggets_value(nuggets: &[LegacyNugget]) -> Value {
    let items: Vec<Value> = nuggets
        .iter()
        .map(|n| serde_json::json!({ "id": n.id, "text": n.text, "capturedAt": n.captured_at }))
        .collect();
    Value::String(Value::Array(items).to_string())
}

/// `exportNuggets`'s body: numbered quotes with when each was captured.
pub fn nuggets_export(nuggets: &[(String, f64)], format_time: impl Fn(f64) -> String) -> String {
    nuggets
        .iter()
        .enumerate()
        .map(|(index, (text, at))| {
            format!("{}. {}\nCaptured {}", index + 1, text, format_time(*at))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `pad2`.
pub fn pad2(value: i64) -> String {
    format!("{value:02}")
}

/// `formatTimerPadded`: `300` → `00:05:00`.
pub fn format_timer_padded(seconds: f64) -> String {
    let sign = if seconds < 0.0 { "-" } else { "" };
    let total = seconds.trunc().abs() as i64;
    format!(
        "{sign}{}:{}:{}",
        pad2(total / 3600),
        pad2((total % 3600) / 60),
        pad2(total % 60)
    )
}

/// `parseDuration`: `5:00`, `1:02:03` → seconds; `None` when not a time.
pub fn parse_duration(text: &str) -> Option<f64> {
    let parts: Vec<&str> = text.trim().split(':').map(str::trim).collect();
    if parts.is_empty()
        || parts.len() > 3
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some(parts.iter().fold(0.0, |total, part| {
        total * 60.0 + part.parse::<f64>().unwrap_or(0.0)
    }))
}

/// `formatTimeOfDay`: `17:00` → `5:00 PM`.
pub fn format_time_of_day(target: &str) -> String {
    let mut parts = target.split(':').map(|p| p.parse::<u32>().unwrap_or(0));
    let hours = parts.next().unwrap_or(0).min(23);
    let minutes = parts.next().unwrap_or(0).min(59);
    let time = chrono::NaiveTime::from_hms_opt(hours, minutes, 0).unwrap_or_default();
    format_clock(time)
}

/// `TimeOfDayField`'s write: a 12-hour reading stored back as `HH:MM`.
pub fn time_of_day(hour12: u32, minute: u32, pm: bool) -> String {
    let h24 = (hour12 % 12) + if pm { 12 } else { 0 };
    format!("{h24:02}:{minute:02}")
}

/// `TimeOfDayField`'s read: (hour 1–12, minute, pm).
pub fn read_time_of_day(value: &str) -> (u32, u32, bool) {
    let mut parts = value.split(':').map(|p| p.parse::<u32>().unwrap_or(0));
    let hours = parts.next().unwrap_or(0);
    let minutes = parts.next().unwrap_or(0);
    let hour12 = if hours % 12 == 0 { 12 } else { hours % 12 };
    (hour12, minutes, hours >= 12)
}

/// `displayPlanTitle`: `sunday_morning-notes` → `sunday morning notes`.
pub fn display_plan_title(title: &str) -> String {
    title
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `hexOr`.
pub fn hex_or<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    let ok = value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|b| b.is_ascii_hexdigit());
    if ok { value } else { fallback }
}

/// GPUI's key name as the DOM `KeyboardEvent.key` the shortcut table uses.
pub fn dom_key(key: &str) -> String {
    match key {
        "left" => "ArrowLeft".into(),
        "right" => "ArrowRight".into(),
        "up" => "ArrowUp".into(),
        "down" => "ArrowDown".into(),
        "pageup" => "PageUp".into(),
        "pagedown" => "PageDown".into(),
        "space" => " ".into(),
        "enter" => "Enter".into(),
        "escape" => "Escape".into(),
        "backspace" => "Backspace".into(),
        "delete" => "Delete".into(),
        "tab" => "Tab".into(),
        "home" => "Home".into(),
        "end" => "End".into(),
        other => other.into(),
    }
}

/// A suggestion the operator made (queue, preload, sermon list, search).
pub fn manual_suggestion(
    id: String,
    reference: String,
    verses: Vec<ScriptureVerse>,
    translation: String,
) -> ScriptureSuggestion {
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
        trigger_text: reference.clone(),
        reference,
        verses,
        translation,
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

/// Electron prefixes main-process errors; the operator only needs the message.
pub fn strip_ipc_prefix(message: &str) -> String {
    let prefix = "Error invoking remote method '";
    if let Some(rest) = message.strip_prefix(prefix)
        && let Some((_, tail)) = rest.split_once("': ")
    {
        return tail.strip_prefix("Error: ").unwrap_or(tail).to_owned();
    }
    message.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verse(chapter: f64, n: f64) -> ScriptureVerse {
        ScriptureVerse {
            book: "John".into(),
            chapter,
            verse: n,
            text: format!("text {n}"),
            extra: Default::default(),
        }
    }

    #[test]
    fn runs_mark_triggers_and_quotes() {
        let highlight = Highlight {
            id: "s".into(),
            trigger_text: "God so loved".into(),
            reference: "John 3:16".into(),
            confidence: 0.9,
        };
        let runs = build_runs("Turn to John, for God so loved the world", &[highlight]);
        assert_eq!(runs[0], TextRun::Trigger("Turn to".into()));
        assert_eq!(runs[1], TextRun::Plain(" John, for ".into()));
        assert_eq!(
            runs[2],
            TextRun::Scripture {
                text: "God so loved".into(),
                reference: "John 3:16".into()
            }
        );
        assert_eq!(runs[3], TextRun::Plain(" the world".into()));
        assert!(build_runs("", &[]).is_empty());
    }

    #[test]
    fn states_and_sources() {
        assert_eq!(
            detection_state(true, true, true),
            Some(DetectionState::Live)
        );
        assert_eq!(
            detection_state(false, true, true),
            Some(DetectionState::Reading)
        );
        assert_eq!(
            detection_state(false, false, true),
            Some(DetectionState::Next)
        );
        assert_eq!(detection_state(false, false, false), None);
        let mut s = manual_suggestion("a".into(), "John 1:1".into(), vec![], "KJV".into());
        assert_eq!(detection_source(&s), None);
        s.preloaded_next = Some(true);
        assert_eq!(detection_source(&s), Some("Preloaded"));
        s.plan_match = Some(PlanMatch::Reference);
        assert_eq!(detection_source(&s), Some("From sermon"));
        s.plan_match = Some(PlanMatch::Quote);
        assert_eq!(detection_source(&s), Some("Sermon · matched"));
    }

    #[test]
    fn chip_labels_keep_a_changed_chapter() {
        assert_eq!(
            queue_chip_label(&verse(1.0, 2.0), Some(&verse(1.0, 1.0))),
            "2"
        );
        assert_eq!(
            queue_chip_label(&verse(2.0, 1.0), Some(&verse(1.0, 51.0))),
            "2:1"
        );
        assert_eq!(detected_chip_label("John 1:5", Some("John 1:4")), "5");
        assert_eq!(detected_chip_label("John 2:1", Some("John 1:51")), "2:1");
        assert_eq!(detected_chip_label("1 John 2:1", Some("1 John 2:1")), "1");
    }

    #[test]
    fn projection_prefers_wording() {
        let mut kjv = manual_suggestion(
            "k".into(),
            "John 1:1".into(),
            vec![verse(1.0, 1.0)],
            "KJV".into(),
        );
        kjv.verses[0].text = "In the beginning was the Word".into();
        let mut niv = kjv.clone();
        niv.id = "n".into();
        niv.verses[0].text = "In the beginning was the Word, NIV".into();
        let projection = (
            "John 1:1".to_owned(),
            "In the beginning was the Word".to_owned(),
        );
        let ids = projected_ids(Some(&projection), &[kjv.clone(), niv.clone()]);
        assert_eq!(ids, HashSet::from(["k".to_owned()]));
        // Reformatted text: the reference decides.
        let projection = ("John 1:1".to_owned(), "IN THE BEGINNING".to_owned());
        assert_eq!(
            projected_ids(Some(&projection), &[kjv.clone(), niv]).len(),
            2
        );
        assert!(projected_ids(None, &[]).is_empty());
        // The screen shows the translation after the reference.
        let projection = (
            "John 1:1 (KJV)".to_owned(),
            "In the beginning was the Word".to_owned(),
        );
        assert_eq!(
            projected_ids(Some(&projection), &[kjv]),
            HashSet::from(["k".to_owned()])
        );
    }

    #[test]
    fn slides_split_on_a_reference_line() {
        assert_eq!(
            split_slide("John 3:16\nFor God so loved"),
            ("John 3:16".into(), "For God so loved".into())
        );
        assert_eq!(
            split_slide("Amazing grace\nhow sweet"),
            ("Projected".into(), "Amazing grace\nhow sweet".into())
        );
    }

    #[test]
    fn legacy_nuggets_round_trip_and_drop_bad_rows() {
        let stored = Value::String(
            r#"[{"id":"a","text":"Grace","capturedAt":5},{"id":1,"text":"x","capturedAt":1},"junk"]"#.into(),
        );
        let nuggets = parse_legacy_nuggets(Some(&stored));
        assert_eq!(nuggets.len(), 1);
        assert_eq!(
            parse_legacy_nuggets(Some(&legacy_nuggets_value(&nuggets))),
            nuggets
        );
        assert!(parse_legacy_nuggets(Some(&Value::String("not json".into()))).is_empty());
        assert!(parse_legacy_nuggets(None).is_empty());
        let body = nuggets_export(&[("Grace".into(), 5.0), ("Hope".into(), 6.0)], |_| {
            "then".into()
        });
        assert_eq!(body, "1. Grace\nCaptured then\n\n2. Hope\nCaptured then");
    }

    #[test]
    fn timer_text() {
        assert_eq!(format_timer_padded(300.0), "00:05:00");
        assert_eq!(format_timer_padded(-61.5), "-00:01:01");
        assert_eq!(format_timer_padded(3723.0), "01:02:03");
        assert_eq!(parse_duration("5:00"), Some(300.0));
        assert_eq!(parse_duration("1:02:03"), Some(3723.0));
        assert_eq!(parse_duration(" 90 "), Some(90.0));
        assert_eq!(parse_duration("1:2:3:4"), None);
        assert_eq!(parse_duration("a:00"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("1234"), None);
        assert_eq!(format_time_of_day("17:00"), "5:00 PM");
        assert_eq!(time_of_day(12, 5, false), "00:05");
        assert_eq!(time_of_day(12, 0, true), "12:00");
        assert_eq!(time_of_day(5, 30, true), "17:30");
        assert_eq!(read_time_of_day("00:05"), (12, 5, false));
        assert_eq!(read_time_of_day("17:30"), (5, 30, true));
    }

    #[test]
    fn small_text_helpers() {
        assert_eq!(
            display_plan_title("sunday_morning--notes "),
            "sunday morning notes"
        );
        assert_eq!(hex_or("#a0B1c2", "#fff"), "#a0B1c2");
        assert_eq!(hex_or("red", "#ffffff"), "#ffffff");
        assert_eq!(dom_key("pagedown"), "PageDown");
        assert_eq!(dom_key("a"), "a");
        assert_eq!(
            strip_ipc_prefix(
                "Error invoking remote method 'scripture:retranslate': Error: NIV isn't available"
            ),
            "NIV isn't available"
        );
        assert_eq!(strip_ipc_prefix("plain"), "plain");
        assert_eq!(number(3.0), "3");
    }
}
