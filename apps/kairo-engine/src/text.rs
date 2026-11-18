//! Slide text on its way to the compositor.
//!
//! A slide's words are drawn by `kairo-scene` on a worker pool, never on the
//! winit thread (which must keep answering the supervisor's heartbeat) or on
//! the compositor thread (which must keep presenting frames). Everything
//! else about the command, its fades and its other layers, is decided at
//! once; only the pixels arrive later. Commands for one surface still take
//! effect in the order they were sent: a command that comes after a slide
//! still being drawn waits behind it, so a clear can never overtake the
//! slide it clears.

use kairo_compositor::thread::Op;
use kairo_compositor::{Content, Fill, Raster, Rect, Slot};
use kairo_domain::overlay::OverlayTheme;
use kairo_engine_proto::SlideContent;
use kairo_scene::{RasterPool, SlideScene, colored_lines_from_json};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

/// The pixels of one slide's text and where they sit in the frame.
#[derive(Clone)]
pub struct TextTile {
    pub raster: Arc<Raster>,
    pub x: u32,
    pub y: u32,
}

impl TextTile {
    pub fn content(&self) -> Content {
        Content {
            rect: Some(Rect::new(
                self.x as f32,
                self.y as f32,
                self.raster.width as f32,
                self.raster.height as f32,
            )),
            ..Content::new(Fill::Image(self.raster.clone()))
        }
    }
}

/// What the scene builder needs; owned so it can cross to a worker.
pub struct TextRequest {
    pub slide: SlideContent,
    pub frame: (u32, u32),
}

/// How a request went.
pub struct TextDone {
    /// The tile, or `None` when the slide draws no text.
    pub tile: Option<TextTile>,
    /// The scene could not be built (a theme or coloured lines the template
    /// would have thrown on).
    pub failed: bool,
    pub cached: bool,
    pub build_ms: f64,
    pub raster_ms: f64,
}

pub struct TextWork {
    pool: RasterPool<Option<TextTile>>,
}

impl TextWork {
    /// Two workers: a slide changes at human speed, and a second lets a
    /// thumbnail-sized backlog drain while a long passage is drawn.
    pub fn new() -> Self {
        Self {
            pool: RasterPool::new(2, 96 << 20, |raster| {
                raster.map(|r| TextTile {
                    raster: Raster::new(r.width, r.height, r.pixels),
                    x: r.x,
                    y: r.y,
                })
            }),
        }
    }

    pub fn submit(&self, request: TextRequest, done: impl FnOnce(TextDone) + Send + 'static) {
        self.pool
            .submit(Box::new(move || build_scene(&request)), move |d| {
                done(TextDone {
                    failed: d.value.is_none(),
                    tile: d.value.flatten(),
                    cached: d.cached,
                    build_ms: d.build_ms,
                    raster_ms: d.raster_ms,
                })
            });
    }
}

impl Default for TextWork {
    fn default() -> Self {
        Self::new()
    }
}

/// `renderOverlayHTML`'s inputs, from the wire. A theme that is not already
/// complete is normalised, as the settings loader would have.
fn build_scene(request: &TextRequest) -> Option<SlideScene> {
    let slide = &request.slide;
    let theme_value = match slide.theme.value() {
        Ok(value) => value,
        Err(error) => {
            tracing::error!("slide theme is not JSON: {error}");
            return None;
        }
    };
    let theme: OverlayTheme = match serde_json::from_value(theme_value.clone()) {
        Ok(theme) => theme,
        Err(_) => kairo_domain::overlay_defaults::normalize_overlay_theme(Some(&theme_value)),
    };
    let colored = match &slide.colored_lines {
        None => Vec::new(),
        Some(json) => match json.value() {
            Ok(serde_json::Value::Null) => Vec::new(),
            Ok(value) => match colored_lines_from_json(&value) {
                Some(lines) => lines,
                None => {
                    tracing::error!("slide coloured lines are not text and colour entries");
                    return None;
                }
            },
            Err(error) => {
                tracing::error!("slide coloured lines are not JSON: {error}");
                return None;
            }
        },
    };
    Some(SlideScene::build(
        &theme,
        &slide.reference,
        &slide.text,
        request.frame,
        &colored,
    ))
}

/// Whether a slide can draw anything: nothing is asked of a worker for a
/// blank one, as for a document page.
pub fn slide_is_blank(slide: &SlideContent) -> bool {
    kairo_domain::js::trim(&slide.reference).is_empty()
        && kairo_domain::js::trim(&slide.text).is_empty()
}

/// Where a text raster lands in an operation list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSlot {
    pub index: usize,
}

/// What an entry still waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// A slide's text tile, and the op it fills.
    Text(u64, TextSlot),
    /// The first stage of an elements layer, and the keyed op it fills.
    Visual(u64, usize),
}

struct Entry {
    seq: u64,
    ops: Vec<Op>,
    waiting: Vec<Wait>,
    received: Instant,
    done: Option<Timing>,
}

/// What the trace line of a finished slide reports.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub cached: bool,
    pub failed: bool,
    pub build_ms: f64,
    pub raster_ms: f64,
}

/// One surface's commands, applied in the order they arrived.
#[derive(Default)]
pub struct Ordered {
    entries: VecDeque<Entry>,
}

/// An entry that may be applied now.
pub struct Ready {
    pub seq: u64,
    pub ops: Vec<Op>,
    pub received: Instant,
    pub text: Option<Timing>,
}

impl Ordered {
    #[cfg(test)]
    pub fn push(
        &mut self,
        seq: u64,
        ops: Vec<Op>,
        waiting: Option<(u64, TextSlot)>,
        received: Instant,
    ) {
        let waits = waiting
            .map(|(job, slot)| Wait::Text(job, slot))
            .into_iter()
            .collect();
        self.push_waiting(seq, ops, waits, received);
    }

    pub fn push_waiting(&mut self, seq: u64, ops: Vec<Op>, waiting: Vec<Wait>, received: Instant) {
        self.entries.push_back(Entry {
            seq,
            ops,
            waiting,
            received,
            done: None,
        });
    }

    /// A job finished: fill its op. `false` when no entry wants it (the
    /// surface was closed or rebuilt in the meantime).
    pub fn complete(&mut self, job: u64, done: &TextDone) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| {
            e.waiting
                .iter()
                .any(|w| matches!(w, Wait::Text(j, _) if *j == job))
        }) else {
            return false;
        };
        let at = entry
            .waiting
            .iter()
            .position(|w| matches!(w, Wait::Text(j, _) if *j == job))
            .expect("matched above");
        let Wait::Text(_, slot) = entry.waiting.remove(at) else {
            return false;
        };
        if let Some(tile) = &done.tile
            && let Some(Op::Swap { content, .. }) = entry.ops.get_mut(slot.index)
        {
            *content = Some(tile.content());
        }
        entry.done = Some(Timing {
            cached: done.cached,
            failed: done.failed,
            build_ms: done.build_ms,
            raster_ms: done.raster_ms,
        });
        true
    }

    /// An elements layer's first stage is ready: put it in its keyed op.
    pub fn complete_visual(&mut self, job: u64, content: kairo_compositor::Content) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| {
            e.waiting
                .iter()
                .any(|w| matches!(w, Wait::Visual(j, _) if *j == job))
        }) else {
            return false;
        };
        let at = entry
            .waiting
            .iter()
            .position(|w| matches!(w, Wait::Visual(j, _) if *j == job))
            .expect("matched above");
        if let Wait::Visual(_, index) = entry.waiting.remove(at) {
            match entry.ops.get_mut(index) {
                Some(Op::Keyed {
                    content: Some((slot_content, _)),
                    ..
                })
                | Some(Op::Update {
                    content: slot_content,
                    ..
                }) => *slot_content = content,
                _ => {}
            }
        }
        true
    }

    /// A page turn's result is not to be shown: every operation of its
    /// entry is dropped (the frame still follows) and it stops waiting.
    pub fn cancel_visual(&mut self, job: u64) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|e| {
            e.waiting
                .iter()
                .any(|w| matches!(w, Wait::Visual(j, _) if *j == job))
        }) else {
            return false;
        };
        let at = entry
            .waiting
            .iter()
            .position(|w| matches!(w, Wait::Visual(j, _) if *j == job))
            .expect("matched above");
        entry.waiting.remove(at);
        // A refused page turn changes nothing: neither page nor videos.
        entry.ops.clear();
        true
    }

    /// Entries from the front that no longer wait for pixels.
    pub fn ready(&mut self) -> Vec<Ready> {
        let mut out = Vec::new();
        while self.entries.front().is_some_and(|e| e.waiting.is_empty()) {
            let e = self.entries.pop_front().expect("checked");
            out.push(Ready {
                seq: e.seq,
                ops: e.ops,
                received: e.received,
                text: e.done,
            });
        }
        out
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// The op list's text swap, if it has one that wants pixels.
pub fn text_slot(ops: &[Op]) -> Option<TextSlot> {
    ops.iter()
        .position(|op| {
            matches!(
                op,
                Op::Swap {
                    slot: Slot::Text,
                    content: Some(_),
                    ..
                }
            )
        })
        .map(|index| TextSlot { index })
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_engine_proto::Json;
    use serde_json::json;
    use std::sync::mpsc;
    use std::time::Duration;

    fn slide(text: &str, theme: serde_json::Value) -> SlideContent {
        SlideContent {
            reference: "John 3:16".into(),
            text: text.into(),
            theme: Json::from_value(&theme),
            colored_lines: None,
        }
    }

    fn default_theme() -> serde_json::Value {
        serde_json::to_value(&*kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME).unwrap()
    }

    fn run(work: &TextWork, slide: SlideContent) -> TextDone {
        let (tx, rx) = mpsc::channel();
        work.submit(
            TextRequest {
                slide,
                frame: (1920, 1080),
            },
            move |d| {
                tx.send(d).unwrap();
            },
        );
        rx.recv_timeout(Duration::from_secs(30)).unwrap()
    }

    #[test]
    fn a_default_theme_slide_becomes_a_tile_and_a_second_push_is_cached() {
        let work = TextWork::new();
        let first = run(&work, slide("For God so loved the world", default_theme()));
        let tile = first.tile.expect("text drawn");
        assert!(!first.failed && !first.cached);
        assert!(tile.raster.width > 0 && tile.x + tile.raster.width <= 1920);
        let second = run(&work, slide("For God so loved the world", default_theme()));
        assert!(second.cached);
        assert_eq!(
            second.tile.unwrap().raster.id(),
            tile.raster.id(),
            "the same texture is reused"
        );
        let other = run(&work, slide("Jesus wept.", default_theme()));
        assert!(!other.cached);
    }

    #[test]
    fn sparse_and_broken_input_is_handled() {
        let work = TextWork::new();
        // A theme missing fields is normalised, not rejected.
        let sparse = run(&work, slide("hello", json!({"verse": {"fontSizePx": 40}})));
        assert!(!sparse.failed && sparse.tile.is_some());
        // Not JSON at all, and coloured lines that are not entries, fail the slide.
        let mut bad = slide("hello", default_theme());
        bad.theme = Json("not json".into());
        let d = run(&work, bad);
        assert!(d.failed && d.tile.is_none());
        let mut bad = slide("hello", default_theme());
        bad.colored_lines = Some(Json(r##"[{"color":"#fff"}]"##.into()));
        assert!(run(&work, bad).failed);
        // `null` and `[]` mean no coloured lines.
        for none in ["null", "[]"] {
            let mut ok = slide("hello", default_theme());
            ok.colored_lines = Some(Json(none.into()));
            assert!(!run(&work, ok).failed, "{none}");
        }
        // A blank slide draws no tile and is not a failure.
        let blank = SlideContent {
            reference: "".into(),
            text: " ".into(),
            ..slide("", default_theme())
        };
        assert!(slide_is_blank(&blank));
        let d = run(&work, blank);
        assert!(!d.failed && d.tile.is_none());
    }

    fn swap(content: Option<Content>) -> Op {
        Op::Swap {
            slot: Slot::Text,
            content,
            ms: 0,
        }
    }

    fn done_with(tile: Option<TextTile>) -> TextDone {
        TextDone {
            tile,
            failed: false,
            cached: false,
            build_ms: 0.0,
            raster_ms: 1.0,
        }
    }

    #[test]
    fn commands_apply_in_order_and_a_clear_cannot_overtake_a_slide() {
        let mut queue = Ordered::default();
        let now = Instant::now();
        let raster = Raster::new(2, 2, vec![0; 16]);
        let tile = TextTile { raster, x: 5, y: 6 };
        // slide (waiting), then clear text, then another slide (waiting).
        queue.push(
            1,
            vec![swap(Some(Content::empty()))],
            Some((10, TextSlot { index: 0 })),
            now,
        );
        queue.push(2, vec![swap(None)], None, now);
        queue.push(
            3,
            vec![swap(Some(Content::empty()))],
            Some((11, TextSlot { index: 0 })),
            now,
        );
        assert!(
            queue.ready().is_empty(),
            "nothing is applied while the first slide waits"
        );
        // The later slide finishing first changes nothing yet.
        assert!(queue.complete(11, &done_with(Some(tile.clone()))));
        assert!(queue.ready().is_empty());
        assert!(queue.complete(10, &done_with(Some(tile.clone()))));
        let ready = queue.ready();
        assert_eq!(ready.iter().map(|r| r.seq).collect::<Vec<_>>(), [1, 2, 3]);
        match &ready[0].ops[0] {
            Op::Swap {
                content: Some(c), ..
            } => {
                assert_eq!(c.rect, Some(Rect::new(5.0, 6.0, 2.0, 2.0)));
                assert!(matches!(c.fill, Fill::Image(_)));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(ready[1].ops[0], Op::Swap { content: None, .. }));
        assert!(queue.ready().is_empty());
        // A result nobody waits for (surface rebuilt) is ignored.
        assert!(!queue.complete(99, &done_with(None)));
        // A job with no tile leaves the placeholder node.
        queue.push(
            4,
            vec![swap(Some(Content::empty()))],
            Some((12, TextSlot { index: 0 })),
            now,
        );
        queue.complete(12, &done_with(None));
        match &queue.ready()[0].ops[0] {
            Op::Swap {
                content: Some(c), ..
            } => assert!(matches!(c.fill, Fill::Empty)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_text_slot_is_found_only_when_a_swap_wants_pixels() {
        assert_eq!(
            text_slot(&[Op::ProgramBackground(None), swap(Some(Content::empty()))]),
            Some(TextSlot { index: 1 })
        );
        assert_eq!(text_slot(&[swap(None)]), None);
        assert_eq!(
            text_slot(&[Op::Swap {
                slot: Slot::Logo,
                content: Some(Content::empty()),
                ms: 0
            }]),
            None
        );
    }

    // ── The whole path, against a real (software or hardware) GPU ──────────

    use crate::program::Shell;
    use kairo_compositor::thread::{Compositor, CompositorEvent, Sink};
    use kairo_engine_proto::{Command, SinkSpec, SurfaceSpec, SurfaceState};

    struct Rig {
        compositor: Compositor,
        events: mpsc::Receiver<CompositorEvent>,
        work: TextWork,
        shell: Shell,
        state: SurfaceState,
        queue: Ordered,
        jobs: mpsc::Receiver<(u64, TextDone)>,
        job_tx: mpsc::Sender<(u64, TextDone)>,
        next_job: u64,
        seq: u64,
    }

    fn rig() -> Option<Rig> {
        let (tx, events) = mpsc::channel();
        let compositor = match Compositor::start(move |e| {
            let _ = tx.send(e);
        }) {
            Ok(c) => c,
            Err(error) => {
                assert!(
                    !std::env::var("KAIRO_REQUIRE_GPU").is_ok_and(|v| !v.is_empty()),
                    "{error}"
                );
                eprintln!("skipped: {error}");
                return None;
            }
        };
        compositor.open("o", 1080, Sink::Offscreen);
        let (job_tx, jobs) = mpsc::channel();
        Some(Rig {
            compositor,
            events,
            work: TextWork::new(),
            shell: Shell::new("o", 1080),
            state: SurfaceState::new(SurfaceSpec {
                output: "o".into(),
                label: "o".into(),
                sink: SinkSpec::Ndi { name: "n".into() },
            }),
            queue: Ordered::default(),
            jobs,
            job_tx,
            next_job: 0,
            seq: 0,
        })
    }

    impl Rig {
        /// The path `Engine::dispatch` and `flush` take for one command.
        fn push(&mut self, command: Command) -> u64 {
            self.seq += 1;
            let before = self.state.clone();
            self.state.apply(&command, true);
            let planned = self.shell.command(&command, &before, &self.state);
            let received = Instant::now();
            let waiting = planned.text.and_then(|request| {
                let slot = text_slot(&planned.ops)?;
                let job = self.next_job;
                self.next_job += 1;
                let tx = self.job_tx.clone();
                self.work.submit(request, move |done| {
                    let _ = tx.send((job, done));
                });
                Some((job, slot))
            });
            self.queue.push(self.seq, planned.ops, waiting, received);
            self.flush();
            self.seq
        }

        fn flush(&mut self) {
            for ready in self.queue.ready() {
                self.compositor.apply("o", ready.seq, ready.ops);
            }
        }

        /// Run the loop the winit thread runs until `seq` is committed;
        /// commits must arrive in command order.
        fn until_committed(&mut self, seq: u64) -> Vec<u64> {
            let mut seen = Vec::new();
            let end = Instant::now() + Duration::from_secs(20);
            loop {
                assert!(
                    Instant::now() < end,
                    "seq {seq} never committed; saw {seen:?}"
                );
                if let Ok((job, done)) = self.jobs.try_recv() {
                    self.queue.complete(job, &done);
                    self.flush();
                }
                match self.events.recv_timeout(Duration::from_millis(2)) {
                    Ok(CompositorEvent::Committed { seq: s, .. }) => {
                        seen.push(s);
                        if s >= seq {
                            return seen;
                        }
                    }
                    Ok(CompositorEvent::Error { message, .. }) => panic!("{message}"),
                    _ => {}
                }
            }
        }

        fn show(&mut self, text: &str) -> u64 {
            let slide = SlideContent {
                reference: "John 3:16".into(),
                text: text.into(),
                theme: Json::from_value(&default_theme()),
                colored_lines: None,
            };
            self.push(Command::ShowSlide {
                output: "o".into(),
                source: "o".into(),
                slide,
                instant: true,
            })
        }

        /// With `KAIRO_ENGINE_FRAME_OUT` set, save the frame as a PNG there.
        fn save(&self, name: &str) {
            let Some(dir) = std::env::var_os("KAIRO_ENGINE_FRAME_OUT") else {
                return;
            };
            let (w, h, pixels) = self.compositor.read("o").expect("frame");
            std::fs::create_dir_all(&dir).unwrap();
            let file = std::fs::File::create(std::path::Path::new(&dir).join(name)).unwrap();
            let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }

        fn ink(&self) -> usize {
            let (_, _, pixels) = self.compositor.read("o").expect("frame");
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0)
                .count()
        }
    }

    #[test]
    fn slide_text_reaches_the_frame_and_a_clear_cannot_overtake_it() {
        let Some(mut rig) = rig() else { return };
        let seq = rig.show("For God so loved the world, that he gave his only begotten Son");
        rig.until_committed(seq);
        let drawn = rig.ink();
        rig.save("slide.png");
        assert!(drawn > 2000, "text pixels in the frame: {drawn}");
        // A slide immediately followed by a clear: the clear waits for the
        // slide's words, then removes them. Commits stay in order.
        let slide = rig.show("Jesus wept.");
        let clear = rig.push(Command::ClearText {
            output: "o".into(),
            background: None,
        });
        let seen = rig.until_committed(clear);
        assert!(seen.windows(2).all(|w| w[0] < w[1]), "{seen:?}");
        assert!(seen.contains(&clear) && slide < clear);
        assert_eq!(rig.ink(), 0, "the clear removed the slide's text");
    }

    #[test]
    fn push_to_commit_is_measured_for_cold_and_cached_slides() {
        let Some(mut rig) = rig() else { return };
        let verses = [
            "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.",
            "The LORD is my shepherd; I shall not want. He maketh me to lie down in green pastures: he leadeth me beside the still waters.",
            "Jesus wept.",
            "Trust in the LORD with all thine heart; and lean not unto thine own understanding. In all thy ways acknowledge him, and he shall direct thy paths.",
            "Ìjà dópin, ogun sì tán\nThe strife is over, the battle done\nOlúwa wa ti ṣẹ́gun\nOur Lord has won the victory",
        ];
        // Warm the fonts and the GPU once, as a running engine has.
        let warm = rig.show("warm up");
        rig.until_committed(warm);
        let mut cold = Vec::new();
        for round in 0..8 {
            for (i, verse) in verses.iter().enumerate() {
                let text = format!("{verse} {round}{i}");
                let started = Instant::now();
                let seq = rig.show(&text);
                rig.until_committed(seq);
                cold.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
        let mut cached = Vec::new();
        for i in 0..20 {
            // The same slide as the one before: the raster is a cache hit.
            let started = Instant::now();
            let seq = rig.show(verses[i % verses.len()]);
            rig.until_committed(seq);
            let _ = started;
        }
        for _ in 0..3 {
            let seq = rig.show(verses[0]);
            rig.until_committed(seq);
        }
        for i in 0..20 {
            let started = Instant::now();
            let seq = rig.show(verses[i % verses.len()]);
            rig.until_committed(seq);
            cached.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        let pct = |v: &mut Vec<f64>, p: usize| {
            v.sort_by(f64::total_cmp);
            v[(v.len() * p).div_ceil(100) - 1]
        };
        eprintln!(
            "push to commit, new slides (n={}): p50 {:.2} ms p95 {:.2} ms max {:.2} ms; repeated slides (n={}): p50 {:.2} ms p95 {:.2} ms",
            cold.len(),
            pct(&mut cold.clone(), 50),
            pct(&mut cold.clone(), 95),
            cold.iter().cloned().fold(0.0, f64::max),
            cached.len(),
            pct(&mut cached.clone(), 50),
            pct(&mut cached, 95),
        );
        // A loose bound that holds on a loaded debug run; the tracked figures
        // are the ones printed above, from a release build.
        let budget = if cfg!(windows) && std::env::var("KAIRO_PERF_HOSTED").as_deref() == Ok("1") {
            // The hosted runner draws through WARP's CPU implementation.
            1000.0
        } else {
            250.0
        };
        assert!(pct(&mut cold, 95) < budget);
    }

    #[test]
    fn every_built_in_theme_draws_through_the_engine_path() {
        let Some(mut rig) = rig() else { return };
        let verse = "For God so loved the world, that he gave his only begotten Son, that whosoever believeth in him should not perish, but have everlasting life.";
        for built in kairo_domain::theme_library::BUILT_IN_THEMES.iter() {
            let mut theme = built.theme.clone();
            // Only colour backgrounds draw until prompt 053.
            theme.background.kind = kairo_domain::overlay::OverlayBackgroundType::Color;
            theme.background.color = "#2a3f5f".into();
            theme.background.opacity = 1.0;
            let slide = SlideContent {
                reference: "John 3:16 (KJV)".into(),
                text: verse.into(),
                theme: Json::encode(&theme),
                colored_lines: None,
            };
            let seq = rig.push(Command::ShowSlide {
                output: "o".into(),
                source: "o".into(),
                slide,
                instant: true,
            });
            rig.until_committed(seq);
            rig.save(&format!("theme-{}.png", built.id));
            assert!(
                rig.ink() > 100_000,
                "{} draws its background and text",
                built.id
            );
        }
        // Coloured gloss lines with Yoruba tone marks, fitted to the box.
        let mut theme = kairo_domain::theme_library::BUILT_IN_THEMES[0]
            .theme
            .clone();
        theme.background.kind = kairo_domain::overlay::OverlayBackgroundType::Color;
        theme.background.color = "#1d2b3a".into();
        let lines = [
            "Ìjà dópin, ogun sì tán",
            "The strife is over, the battle done",
            "Olúwa wa ti ṣẹ́gun",
            "Our Lord has won the victory",
        ];
        let slide = SlideContent {
            reference: "Yorùbá / English".into(),
            text: lines.join("\n"),
            theme: Json::encode(&theme),
            colored_lines: Some(Json::from_value(&json!(
                lines.iter().enumerate().map(|(i, l)| json!({"text": l, "color": if i % 2 == 1 { json!("#5eead4") } else { json!(null) }})).collect::<Vec<_>>()
            ))),
        };
        let seq = rig.push(Command::ShowSlide {
            output: "o".into(),
            source: "o".into(),
            slide,
            instant: true,
        });
        rig.until_committed(seq);
        rig.save("yoruba-coloured.png");
    }
}
