//! Readings remembered through a service: a bare verse or a resume goes back
//! to the right one, not to whatever was cited last.

use kairo_scripture::citation::ExplicitCitation;
use kairo_scripture::live_detector::{
    LiveCitationDetector, ReadingKind, ScriptureResolver, TranscriptEvent, TranscriptKind,
};

struct Service(LiveCitationDetector);

impl Service {
    fn new() -> Self {
        Self(LiveCitationDetector::default())
    }

    /// Every reference the detector emits for a final at `at_s` seconds.
    fn say(&mut self, at_s: u64, text: &str) -> Vec<(String, bool, ScriptureResolver)> {
        self.say_on(at_s, text, Some(1))
    }

    fn say_on(
        &mut self,
        at_s: u64,
        text: &str,
        epoch: Option<u64>,
    ) -> Vec<(String, bool, ScriptureResolver)> {
        self.0
            .process(TranscriptEvent {
                at_ms: at_s * 1_000,
                kind: TranscriptKind::Final,
                text,
                audio_epoch: epoch,
            })
            .into_iter()
            .map(|e| {
                let r = e.reference;
                let end = r
                    .verse_end
                    .map_or_else(String::new, |end| format!("-{end}"));
                (
                    format!("{} {}:{}{end}", r.book, r.chapter, r.verse_start),
                    e.ambiguous,
                    e.resolver,
                )
            })
            .collect()
    }

    fn interim(&mut self, at_s: u64, text: &str) -> usize {
        self.0
            .process(TranscriptEvent {
                at_ms: at_s * 1_000,
                kind: TranscriptKind::Interim,
                text,
                audio_epoch: Some(1),
            })
            .len()
    }

    /// The Luke reading with a Romans aside, as in the verified scenario.
    fn luke_then_romans() -> Self {
        let mut service = Self::new();
        assert_eq!(
            refs(service.say(0, "Turn with me to Luke chapter 1 verse 57.")),
            ["Luke 1:57"]
        );
        assert_eq!(
            refs(service.say(90, "This reminds me of Romans 8:28.")),
            ["Romans 8:28"]
        );
        service
    }
}

fn refs(found: Vec<(String, bool, ScriptureResolver)>) -> Vec<String> {
    found.into_iter().map(|(r, _, _)| r).collect()
}

fn one(found: Vec<(String, bool, ScriptureResolver)>) -> (String, bool, ScriptureResolver) {
    assert_eq!(found.len(), 1, "{found:?}");
    found.into_iter().next().unwrap()
}

#[test]
fn go_back_to_a_verse_of_the_current_reading() {
    let mut service = Service::new();
    service.say(0, "Turn with me to Luke chapter 1 verse 57.");
    assert_eq!(
        one(service.say(61, "Go back to verse 62.")),
        ("Luke 1:62".into(), false, ScriptureResolver::Reading)
    );
}

#[test]
fn a_bare_verse_after_an_aside_resumes_the_main_reading() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        one(service.say(120, "Let's continue from verse 20.")),
        ("Luke 1:20".into(), false, ScriptureResolver::Reading)
    );
    let memory = service.0.memory();
    assert_eq!(
        memory.newest().unwrap().book,
        "Luke",
        "resumed reading moves up"
    );
    assert_eq!(memory.readings()[1].kind, ReadingKind::Aside);
}

#[test]
fn a_resume_moments_after_an_aside_still_means_the_main_reading() {
    let mut service = Service::new();
    service.say(0, "Turn with me to Luke chapter 1 verse 57.");
    service.say(90, "This reminds me of Romans 8:28.");
    assert_eq!(
        one(service.say(95, "Let's continue from verse 20.")),
        ("Luke 1:20".into(), false, ScriptureResolver::Reading)
    );
}

#[test]
fn a_verse_mentioned_again_without_resume_wording_is_not_repeated() {
    let mut service = Service::new();
    service.say(0, "Turn with me to Luke chapter 1 verse 57.");
    assert_eq!(refs(service.say(61, "Look at verse 62.")), ["Luke 1:62"]);
    assert!(
        service.say(75, "As verse 62 says.").is_empty(),
        "a second mention does not pull the screen back"
    );
    assert_eq!(
        refs(service.say(90, "Go back to verse 62.")),
        ["Luke 1:62"],
        "asking for it again does"
    );
}

#[test]
fn a_resumed_verse_only_the_main_chapter_has() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        refs(service.say(120, "Let's continue from verse 65.")),
        ["Luke 1:65"]
    );
}

#[test]
fn a_bare_verse_without_resume_wording_stays_in_the_aside_when_it_has_it() {
    let mut service = Service::luke_then_romans();
    assert_eq!(refs(service.say(120, "Look at verse 31.")), ["Romans 8:31"]);
    // Romans 8 stops at 39: verse 62 can only be the Luke reading.
    assert_eq!(refs(service.say(150, "Look at verse 62.")), ["Luke 1:62"]);
}

#[test]
fn a_bare_verse_just_after_a_citation_keeps_that_citation() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        refs(service.say(95, "Continue to verse 29.")),
        ["Romans 8:29"]
    );
}

#[test]
fn book_and_verse_without_a_chapter() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        one(service.say(120, "Back in Luke, verse 20.")),
        ("Luke 1:20".into(), false, ScriptureResolver::Reading)
    );
}

#[test]
fn book_named_alone_then_a_verse() {
    let mut service = Service::luke_then_romans();
    assert!(service.say(120, "Luke.").is_empty());
    assert_eq!(refs(service.say(122, "Verse 20.")), ["Luke 1:20"]);
}

#[test]
fn back_to_a_book_resumes_its_reading() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        refs(service.say(120, "Let's go back to Luke.")),
        ["Luke 1:57"]
    );
}

#[test]
fn resume_wording_alone_resumes_the_newest_main_reading() {
    let mut service = Service::luke_then_romans();
    assert_eq!(
        one(service.say(120, "Let's continue our reading.")),
        ("Luke 1:57".into(), false, ScriptureResolver::Reading)
    );
}

#[test]
fn resume_continues_after_the_last_verse_heard_read() {
    let mut service = Service::luke_then_romans();
    service.0.note_shown(&citation("Luke", 1.0, 65.0), 100_000);
    service.0.note_read("Luke", 1.0, 65.0, 101_000);
    service
        .0
        .note_shown(&citation("Romans", 8.0, 28.0), 110_000);
    assert_eq!(
        refs(service.say(130, "Let's pick up where we left off.")),
        ["Luke 1:66"]
    );
}

#[test]
fn narration_is_not_a_resume() {
    let mut service = Service::luke_then_romans();
    for text in [
        "Let's continue in prayer.",
        "We continue to trust him.",
        "Back in Luke's day, people walked everywhere.",
    ] {
        assert!(service.say(150, text).is_empty(), "{text}");
    }
}

#[test]
fn a_resume_with_nothing_read_stays_silent() {
    let mut service = Service::new();
    assert!(service.say(0, "Let's continue our reading.").is_empty());
    assert!(service.say(5, "Back in Luke, verse 20.").is_empty());
}

#[test]
fn two_main_readings_and_only_one_chapter_has_the_verse() {
    let mut service = Service::new();
    service.say(0, "Turn to John chapter 3 verses 1 to 8.");
    service.say(60, "Now turn to Luke chapter 1 verse 57.");
    // John 3 has 36 verses.
    assert_eq!(
        refs(service.say(120, "Let's go back to verse 70.")),
        ["Luke 1:70"]
    );
}

#[test]
fn a_resume_two_readings_fit_equally_is_flagged() {
    let mut service = Service::new();
    service.say(0, "Turn to John chapter 3 verses 1 to 8.");
    service.say(60, "Now turn to Luke chapter 1 verses 57 to 60.");
    let (reference, ambiguous, _) = one(service.say(120, "Let's go back to verse 20."));
    assert_eq!(reference, "Luke 1:20");
    assert!(ambiguous);
    // Inside one reading's span: that one, and sure.
    assert_eq!(
        one(service.say(180, "Let's go back to verse 5.")),
        ("John 3:5".into(), false, ScriptureResolver::Reading)
    );
}

#[test]
fn interim_resume_waits_for_the_final() {
    let mut service = Service::luke_then_romans();
    assert_eq!(service.interim(119, "Let's continue from verse 6"), 0);
    assert_eq!(
        refs(service.say(120, "Let's continue from verse 65.")),
        ["Luke 1:65"]
    );
}

#[test]
fn memory_outlives_unrelated_speech_and_a_reconnect() {
    let mut service = Service::luke_then_romans();
    for minute in 2..32 {
        service.say_on(
            minute * 60,
            "And the people of the church gathered to pray.",
            Some(minute),
        );
    }
    assert_eq!(
        refs(service.say_on(32 * 60, "Let's continue our reading.", Some(99))),
        ["Luke 1:57"]
    );
}

fn citation(book: &'static str, chapter: f64, verse: f64) -> ExplicitCitation {
    ExplicitCitation {
        book,
        chapter,
        verse_start: verse,
        verse_end: None,
        source_text: String::new(),
    }
}

#[test]
fn a_resume_no_rule_places_goes_to_the_model_with_the_readings() {
    use kairo_scripture::model_detector::{DetectionType, ModelDetection, ModelWork};
    let mut service = Service::luke_then_romans();
    assert!(
        service
            .say(120, "Let's pick it up where we left off in the Gospel.")
            .is_empty()
    );
    let transcript = service.0.take_unresolved().expect("asks the model");
    assert!(transcript.resume);
    assert!(
        transcript.text.contains("Luke 1:57-57 (main reading"),
        "{}",
        transcript.text
    );
    let work = ModelWork {
        transcript,
        generation: service.0.generation(),
    };
    let detection = |book, chapter, verse| ModelDetection {
        reference: citation(book, chapter, verse),
        detection_type: DetectionType::Partial,
        confidence: 0.8,
    };
    let found = service.0.publish_model_result(
        &work,
        vec![detection("John", 3.0, 16.0), detection("Luke", 1.0, 57.0)],
        &[],
        121_000,
    );
    assert_eq!(found.len(), 1, "only a passage this service read");
    assert_eq!(
        (found[0].reference.book, found[0].reference.verse_start),
        ("Luke", 57.0)
    );
    assert_eq!(found[0].resolver, ScriptureResolver::Ai, "never instant");
}

#[test]
fn narration_with_resume_wording_never_asks_the_model() {
    let mut service = Service::luke_then_romans();
    service.say(120, "We continue to trust him.");
    assert!(service.0.take_unresolved().is_none());
}
