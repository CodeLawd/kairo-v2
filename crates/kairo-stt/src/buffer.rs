//! Rolling transcript window, sentence detection, trigger phrases and the
//! analyse cadence. From `src/main/services/stt/buffer.ts`.
//!
//! Electron's buffer owns a `setInterval`; here the owner calls [`tick`]
//! every [`BufferConfig::analyze_interval_seconds`] and receives the events
//! as return values.
//!
//! [`tick`]: TranscriptionBuffer::tick

use kairo_domain::transcript::TranscriptResult;

#[derive(Clone, Debug, PartialEq)]
pub struct BufferSegment {
    pub id: String,
    pub text: String,
    pub timestamp: f64,
    pub duration: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FullTranscriptEntry {
    pub timestamp: f64,
    pub text: String,
    pub duration: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BufferConfig {
    pub window_seconds: f64,
    pub analyze_interval_seconds: f64,
    pub trigger_phrases: Vec<String>,
}

impl Default for BufferConfig {
    fn default() -> Self {
        Self {
            window_seconds: 90.0,
            analyze_interval_seconds: 8.0,
            trigger_phrases: [
                "turn to",
                "turn your bibles to",
                "let's read",
                "the scripture says",
                "the bible says",
                "it is written",
                "as it says in",
                "chapter",
                "verse",
            ]
            .map(String::from)
            .to_vec(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BufferEvent {
    SentenceComplete {
        text: String,
        timestamp: f64,
    },
    TriggerPhrase {
        phrase: String,
        context: String,
        timestamp: f64,
    },
    AnalyzeReady {
        context: String,
        timestamp: f64,
    },
}

#[derive(Default)]
pub struct TranscriptionBuffer {
    config: BufferConfig,
    window: Vec<BufferSegment>,
    history: Vec<FullTranscriptEntry>,
    sentence_accumulator: String,
    dirty_after_last_analyze: bool,
}

/// `/[.?!](?:\s|$)/`: the index of the terminator.
fn sentence_end(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(c, '.' | '?' | '!')
            && chars
                .peek()
                .is_none_or(|(_, next)| kairo_domain::js::is_whitespace(*next))
        {
            return Some(i);
        }
    }
    None
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut remaining = kairo_domain::js::trim(text);
    while !remaining.is_empty() {
        let Some(i) = sentence_end(remaining) else {
            break;
        };
        results.push(kairo_domain::js::trim(&remaining[..=i]).to_owned());
        remaining = kairo_domain::js::trim(&remaining[i + 1..]);
    }
    if !remaining.is_empty() {
        results.push(remaining.to_owned());
    }
    results
}

impl TranscriptionBuffer {
    pub fn new(config: BufferConfig) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }

    pub fn configure(&mut self, config: BufferConfig) {
        self.config = config;
    }

    pub fn config(&self) -> &BufferConfig {
        &self.config
    }

    /// Feed a final segment. `now` is the wall clock in milliseconds.
    pub fn push(&mut self, result: &TranscriptResult, now: f64) -> Vec<BufferEvent> {
        let text = kairo_domain::js::trim(&result.text).to_owned();
        if text.is_empty() {
            return Vec::new();
        }
        let segment = BufferSegment {
            id: result.id.clone(),
            text: text.clone(),
            timestamp: result.timestamp,
            duration: result.duration,
        };
        self.history.push(FullTranscriptEntry {
            timestamp: segment.timestamp,
            text: text.clone(),
            duration: segment.duration,
        });
        self.window.push(segment);
        self.evict_expired(now);
        let mut events = self.process_sentences(&text, result.timestamp);
        self.dirty_after_last_analyze = true;
        events.extend(self.check_trigger_phrases(&text, result.timestamp, now));
        events
    }

    /// The analyse timer: context when new text arrived since the last analysis.
    pub fn tick(&mut self, now: f64) -> Option<BufferEvent> {
        if self.dirty_after_last_analyze {
            self.analyze_ready(now)
        } else {
            None
        }
    }

    pub fn context(&self) -> String {
        self.window
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn recent_context(&self, seconds: f64, now: f64) -> String {
        let cutoff = now - seconds * 1000.0;
        self.window
            .iter()
            .filter(|s| s.timestamp >= cutoff)
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn full_transcript(&self) -> &[FullTranscriptEntry] {
        &self.history
    }

    pub fn export_as_text(&self) -> String {
        self.history
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Text grouped under a `## HH:MM` heading per local minute.
    pub fn export_as_markdown(&self, local_minute: impl Fn(f64) -> (u32, u32)) -> String {
        if self.history.is_empty() {
            return String::new();
        }
        let mut lines = Vec::new();
        let mut current = None;
        for entry in &self.history {
            let (hours, minutes) = local_minute(entry.timestamp);
            if current != Some((hours, minutes)) {
                current = Some((hours, minutes));
                lines.push(format!("\n## {hours:02}:{minutes:02}\n"));
            }
            lines.push(entry.text.clone());
        }
        kairo_domain::js::trim(&lines.join(" ").replace(" \n", "\n")).to_owned()
    }

    pub fn window_size(&self) -> usize {
        self.window.len()
    }

    pub fn history_size(&self) -> usize {
        self.history.len()
    }

    /// Forget the window, keep the history.
    pub fn clear(&mut self) {
        self.window.clear();
        self.sentence_accumulator.clear();
        self.dirty_after_last_analyze = false;
    }

    pub fn clear_all(&mut self) {
        self.clear();
        self.history.clear();
    }

    fn evict_expired(&mut self, now: f64) {
        let cutoff = now - self.config.window_seconds * 1000.0;
        let expired = self
            .window
            .iter()
            .take_while(|s| s.timestamp < cutoff)
            .count();
        self.window.drain(..expired);
    }

    fn process_sentences(&mut self, text: &str, timestamp: f64) -> Vec<BufferEvent> {
        let joined = format!("{} {text}", self.sentence_accumulator);
        self.sentence_accumulator = kairo_domain::js::trim(&joined).to_owned();
        let sentences = split_sentences(&self.sentence_accumulator);
        let Some(last) = sentences.last() else {
            return Vec::new();
        };
        let last_complete = sentence_end(last).is_some();
        let complete = if last_complete {
            &sentences[..]
        } else {
            &sentences[..sentences.len() - 1]
        };
        let events = complete
            .iter()
            .map(|s| kairo_domain::js::trim(s))
            .filter(|s| !s.is_empty())
            .map(|s| BufferEvent::SentenceComplete {
                text: s.to_owned(),
                timestamp,
            })
            .collect();
        self.sentence_accumulator = if last_complete {
            String::new()
        } else {
            last.clone()
        };
        events
    }

    fn check_trigger_phrases(&mut self, text: &str, timestamp: f64, now: f64) -> Vec<BufferEvent> {
        let lower = text.to_lowercase();
        let Some(phrase) = self
            .config
            .trigger_phrases
            .iter()
            .find(|p| lower.contains(&p.to_lowercase()))
            .cloned()
        else {
            return Vec::new();
        };
        let (start, end) = (timestamp - 10_000.0, timestamp + 10_000.0);
        let context = self
            .window
            .iter()
            .filter(|s| s.timestamp >= start && s.timestamp <= end)
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut events = vec![BufferEvent::TriggerPhrase {
            phrase,
            context,
            timestamp,
        }];
        // Analysis runs now, bypassing the interval; one trigger per segment.
        events.extend(self.analyze_ready(now));
        events
    }

    fn analyze_ready(&mut self, now: f64) -> Option<BufferEvent> {
        let context = self.context();
        if kairo_domain::js::trim(&context).is_empty() {
            return None;
        }
        self.dirty_after_last_analyze = false;
        Some(BufferEvent::AnalyzeReady {
            context,
            timestamp: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn final_result(id: &str, text: &str, timestamp: f64) -> TranscriptResult {
        serde_json::from_value(serde_json::json!({
            "id": id, "text": text, "timestamp": timestamp, "duration": 2,
            "words": [], "isFinal": true,
        }))
        .unwrap()
    }

    fn analyses(events: &[BufferEvent]) -> usize {
        events
            .iter()
            .filter(|e| matches!(e, BufferEvent::AnalyzeReady { .. }))
            .count()
    }

    #[test]
    fn a_trigger_consumes_the_dirty_context_instead_of_scheduling_it_twice() {
        let mut buffer = TranscriptionBuffer::default();
        let events = buffer.push(
            &final_result("one", "turn to Psalm 115 verses two to thirteen", 1000.0),
            1000.0,
        );
        assert_eq!(analyses(&events), 1);
        assert_eq!(buffer.tick(2000.0), None);
    }

    #[test]
    fn sentences_complete_across_segments() {
        let mut buffer = TranscriptionBuffer::default();
        assert!(
            buffer
                .push(&final_result("a", "Grace and", 0.0), 0.0)
                .is_empty()
        );
        let events = buffer.push(&final_result("b", "peace. Amen? Still", 1.0), 1.0);
        let sentences: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                BufferEvent::SentenceComplete { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(sentences, ["Grace and peace.", "Amen?"]);
        // "3.5" is not a sentence end.
        let events = buffer.push(&final_result("c", "going 3.5 miles!", 2.0), 2.0);
        assert!(
            matches!(&events[0], BufferEvent::SentenceComplete { text, .. } if text == "Still going 3.5 miles!")
        );
    }

    #[test]
    fn the_window_evicts_and_the_timer_fires_only_when_dirty() {
        let mut buffer = TranscriptionBuffer::default();
        buffer.push(&final_result("a", "first", 0.0), 0.0);
        assert!(
            matches!(buffer.tick(1.0), Some(BufferEvent::AnalyzeReady { context, .. }) if context == "first")
        );
        assert_eq!(buffer.tick(2.0), None);
        buffer.push(&final_result("b", "second", 100_000.0), 100_000.0);
        assert_eq!(buffer.context(), "second");
        assert_eq!(buffer.history_size(), 2);
        assert_eq!(buffer.export_as_text(), "first second");
        assert_eq!(buffer.recent_context(1.0, 100_500.0), "second");
        buffer.clear();
        assert_eq!(buffer.window_size(), 0);
        assert_eq!(buffer.history_size(), 2);
        buffer.clear_all();
        assert_eq!(buffer.history_size(), 0);
        assert!(buffer.push(&final_result("c", "   ", 0.0), 0.0).is_empty());
    }

    #[test]
    fn markdown_groups_by_minute() {
        let mut buffer = TranscriptionBuffer::default();
        assert_eq!(buffer.export_as_markdown(|_| (0, 0)), "");
        for (i, t) in [0.0, 30_000.0, 60_000.0].into_iter().enumerate() {
            buffer.push(&final_result(&i.to_string(), &format!("line{i}"), t), t);
        }
        let md = buffer.export_as_markdown(|t| (9, (t / 60_000.0) as u32));
        assert_eq!(md, "## 09:00\n line0 line1\n## 09:01\n line2");
    }
}
