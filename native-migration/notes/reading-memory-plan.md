# Reading memory and core follow-along — plan

Goal: Kairo follows a sermon's readings on its own. It remembers every
passage read during the service, where each reading stopped, and resumes the
right one when the preacher comes back to it — with no added latency and no
operator window required.

Status: stages 1–4 implemented (2026-11-22), including quote suppression from the memory. See DECISIONS.md, "Reading memory and core follow-along".

## Verified findings

Checked against the code and by running the live detector on the scenarios
(scratch test, 2026-11-22).

| #  | Finding | Location |
| -- | ------- | -------- |
| F1 | A bare verse ("verse 62") resolves against one remembered book and chapter. A newer citation replaces it; the verse context expires after 12 s without a citation. | `crates/kairo-scripture/src/live_detector.rs` (`analyze`, `fragments`, `with_remembered_book`) |
| F2 | After a detour, a bare verse goes to the detour's book: Luke 1:57 → Romans 8:28 → "Let's continue from verse 20" emits **Romans 8:20**. Since explicit references now go live at once, this reaches the screen. | same |
| F3 | "Back in Luke, verse 20" (book and verse, no chapter) emits nothing. | same |
| F4 | "Let's continue our reading" (no verse) emits nothing. Voice "continue" is a plain next-verse step from whatever was shown last, so after an aside it steps the aside (Romans 8:29). | `crates/kairo-orchestrator/src/voice.rs` (`NEXT_VERSE`, `pick_command_context`) |
| F5 | "Go back to verse 62" inside the current reading works, even a minute later, through `passage` (book only) — by luck of the remembered book, not by design. | `live_detector.rs` |
| F6 | Follow-along runs in the operator window, not the core. With no booth open nothing follows; the core learns progress only through `Approve` + `FollowStepped` calls. | `crates/ui-operator/src/booth/mod.rs` (`advance_follow`, `start_follow`, `arm_auto_clear`) |
| F7 | Follow-along only moves through cards of the detected passage. Reading on past the detected range is not followed; the passage auto-clears instead. | `crates/kairo-domain/src/scripture/live_progress.rs` (`update_passage_follow_strict`, `passage_read_through`) |
| F8 | The core keeps one "reading passage" (last presented) only to stop quote detection firing on text being read aloud. | `orchestrator.rs` (`set_reading_passage`), `live_detector.rs` (`reading`) |

## Design

### Reading memory

A small, pure, synchronous structure in `kairo-scripture`, owned by
`LiveCitationDetector` so resolution stays on the transcript path with no
await and no lock beyond the one already held.

```rust
pub struct Reading {
    pub book: &'static str,
    pub chapter: f64,
    pub start: f64,            // first verse of the reading
    pub end: f64,              // furthest verse known to belong to it (grows)
    pub last_read: Option<f64>,// last verse follow-along heard to the end
    pub last_shown: f64,       // last verse put on screen
    pub kind: ReadingKind,     // Main or Aside
    pub touched_at: u64,
}
pub struct ReadingMemory { readings: Vec<Reading> } // newest first, cap 8
```

- **Lifetime:** the whole service. Cleared on service stop or a new service,
  never by the 12 s timer or an STT reconnect. Older entries drop off only
  when the cap is reached.
- **Main vs Aside:** a reading is *Main* when a range was cited ("57 to 80"),
  follow-along has read two or more verses of it, or it was introduced as a
  reading ("turn to", "let's read", "our text"). A single verse cited while a
  Main reading is open is an *Aside*; it is promoted to Main once two of its
  verses are read.
- **Fed by:** the detector (citations), and the orchestrator after each
  presentation and each follow-along step (`last_shown`, `last_read`). This
  replaces `set_reading_passage` (F8), which becomes a query on the memory.

### Resolution rules

Applied in `analyze` before today's fragment logic. Resume wording means
"continue", "go back to", "back to", "return to", "pick up", "where we
stopped / left off", "our reading / our text". The whole-phrase matching used
by voice navigation keeps narration ("let's continue in prayer") out.

| Speech | Resolves to |
| ------ | ----------- |
| Full citation ("Romans 8:28") | Unchanged. Added to memory. |
| Bare verse, within 12 s of a citation ("John 3 … verse 16") | Unchanged: the citation just heard. |
| Bare verse with resume wording ("continue from verse 65", "go back to verse 62") | Newest **Main** reading whose chapter has that verse; prefer one where the verse is within its read span or just past it. |
| Bare verse, no resume wording, later than 12 s | The current (newest) reading if the verse exists there, else the newest Main reading that has it. |
| Book + verse, no chapter ("back in Luke, verse 20") | The newest reading of that book. |
| Book + resume wording, no verse ("back to Luke") | That reading's resume verse. |
| Resume wording only ("let's continue our reading") | Newest Main reading's resume verse. |

- **Resume verse:** `last_read + 1` when the last verse was read to its end,
  else `last_shown` (a verse left half-read is shown again).
- **Confidence:** a rule that picks one reading emits an Explicit reference
  with a new resolver, `ScriptureResolver::Reading`, which goes live at once
  like a spoken reference. When two readings fit equally (both chapters have
  the verse and nothing prefers one), it emits with `ambiguous = true`, so it
  waits for the safety delay as a card instead.
- **Voice navigation:** split `NEXT_VERSE`. "Next verse", "following verse"
  stay a step from the verse on screen. "Continue", "carry on", "keep going",
  "go on" become *resume the reading*: the same step while a Main reading is
  on screen, a jump back to the paused Main reading after an Aside.

### Follow-along in the orchestrator

The core already receives every interim and final (`TranscriptEvent` loop in
`orchestrator.rs`). Follow-along moves there:

- The orchestrator owns `PassageFollowState`, runs
  `update_passage_follow_strict` on its pending suggestions, and presents the
  next verse through the existing `present_scripture` path. `FollowStepped`
  becomes internal. Auto-clear moves with it (it is decided by follow state).
- The automation options (`follow_along`, `auto_clear_delay_sec`) already
  reach the running service through `SetAutomation`; the orchestrator uses
  them directly.
- New broadcast `FollowProgress { passage_id, index, read_to, heard }` for
  the booth, which becomes a view: it shows progress and the operator can
  still override (pick a card, dismiss, present). `advance_follow`,
  `start_follow` and `arm_auto_clear` leave `booth/mod.rs`.
- **Reading past the detected range (F7):** when a reading starts, the core
  prefetches its chapter (also removes the ~620 ms first-verse lookup seen in
  the logs). At the last card, if the next verse's opening words are heard,
  that verse is added to the passage and presented, up to the chapter's end.
- Every step updates the reading memory, so "last read" is always known,
  with or without an operator window.

## Stages

Each stage ships on its own and keeps all existing tests green.

1. **Reading memory and resolution rules** (`kairo-scripture`). Memory type,
   Main/Aside rules, the resolution table, `ScriptureResolver::Reading`.
   Fixes F1–F5. Orchestrator feeds `last_shown` from presentations; `last_read`
   is approximated by `last_shown` until stage 2.
2. **Follow-along in the orchestrator** (`kairo-orchestrator`, `ui-operator`).
   Move follow and auto-clear, add `FollowProgress`, booth becomes a view,
   feed `last_read`. Fixes F6.
3. **Read past the range and voice split.** Chapter prefetch, extending a
   passage by heard opening words (F7), "continue" as resume.
4. **Optional AI fallback.** Only when resume wording was heard and no rule
   resolved it ("let's pick it up where we left off in the Gospel"): one
   non-thinking model call with the memory as context; result is a card
   behind the safety delay, never instant.

## Tests

- `kairo-scripture` replay tests, one per row of the resolution table, plus
  the F2 detour case (must give Luke 1:20, not Romans 8:20), two Main readings
  where only one chapter has the verse, and the ambiguous case (flagged).
- Memory survives 30 minutes of unrelated speech and an STT reconnect;
  cleared on stop.
- Orchestrator integration: with no booth, a read passage advances verse by
  verse; Luke 1:57–80 read to 65 → aside Romans 8:28 → "let's continue" shows
  Luke 1:66; reading on past the detected range is followed.
- Booth tests: follow progress shown from `FollowProgress`; operator pick and
  dismiss still win.
- Latency: transcript-to-detection p99 stays at today's ~1 ms (memory is at
  most 8 entries).

## Risks

- **Narration that sounds like resume** ("let's continue to pray"): whole-
  phrase match, and resume needs a reading in memory.
- **A stale reading from early in the service:** plain "let's continue" only
  targets the newest Main reading; older readings need their book named.
- **Changing who drives the screen:** stage 2 moves presentation decisions
  from the booth to the core. The booth must not also advance, or verses
  would be presented twice; the booth's follow code is removed, not kept
  alongside.
- **Electron parity:** this goes beyond the Electron behaviour. Record it in
  `DECISIONS.md` when stage 1 lands.

## Open questions

1. A single verse cited during a reading is treated as an Aside. Is there a
   common case where a preacher cites one verse and then reads on from it as
   the new main text, without saying "turn to"? (Follow-along promoting it
   after two verses covers most of this.)
2. Ambiguous resumes wait as a card behind the safety delay. Acceptable, or
   should Kairo prefer the paused Main reading outright?
