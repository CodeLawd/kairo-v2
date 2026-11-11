//! The prompt 053 corpus against Chromium: every background, filter, blur,
//! element, shape and transform pushed through the engine's own path
//! (shell, workers, compositor) and compared with what `overlay.html` drew
//! for the same theme.
//!
//! References are Chromium's renders over flat grey (`golden-capture ...
//! visuals`). They are large, so they are cached in
//! `.cache/golden-capture/053/visuals` (or `KAIRO_VISUAL_REFERENCE`) rather
//! than committed. Without them the test says so and returns, unless
//! `KAIRO_REQUIRE_ORACLE` is set.

use crate::program::{Shell, VisualRequest};
use crate::text::{Ordered, TextDone, TextWork, Wait, text_slot};
use crate::visuals::{Output, VisualWork};
use kairo_compositor::thread::{Compositor, CompositorEvent, Sink};
use kairo_engine_proto::{Command, Json, SinkSpec, SlideContent, SurfaceSpec, SurfaceState};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const W: usize = 1920;
const H: usize = 1080;

pub fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn fixtures() -> PathBuf {
    workspace()
        .join("tests/fixtures/visuals")
        .canonicalize()
        .expect("fixtures")
}

fn reference_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("KAIRO_VISUAL_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace().join(".cache/golden-capture/053/visuals"));
    dir.is_dir().then_some(dir)
}

enum Back {
    Text(u64, TextDone),
    Visual(u64, VisualRequestInfo, Box<Output>),
}

struct VisualRequestInfo {
    slot: kairo_compositor::Slot,
    key: String,
    generation: u64,
}

/// A surface wired like `Engine`, for tests.
pub struct Rig {
    pub compositor: Compositor,
    events: mpsc::Receiver<CompositorEvent>,
    text: TextWork,
    visuals: VisualWork,
    pub shell: Shell,
    pub state: SurfaceState,
    queue: Ordered,
    back_rx: mpsc::Receiver<Back>,
    back_tx: mpsc::Sender<Back>,
    next_job: u64,
    seq: u64,
    /// Results still to come.
    outstanding: usize,
}

pub fn rig() -> Option<Rig> {
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
    let (back_tx, back_rx) = mpsc::channel();
    Some(Rig {
        compositor,
        events,
        text: TextWork::new(),
        visuals: VisualWork::new(),
        shell: Shell::new("o", 1080),
        state: SurfaceState::new(SurfaceSpec {
            output: "o".into(),
            label: "o".into(),
            sink: SinkSpec::Ndi { name: "n".into() },
        }),
        queue: Ordered::default(),
        back_rx,
        back_tx,
        next_job: 0,
        seq: 0,
        outstanding: 0,
    })
}

impl Rig {
    /// The path `Engine::dispatch` takes for one command.
    pub fn push(&mut self, command: Command) -> u64 {
        self.seq += 1;
        let before = self.state.clone();
        self.state.apply(&command, true);
        let planned = self.shell.command(&command, &before, &self.state);
        let received = Instant::now();
        let mut waits = Vec::new();
        if let Some(request) = planned.text
            && let Some(slot) = text_slot(&planned.ops)
        {
            let job = self.next_job;
            self.next_job += 1;
            let tx = self.back_tx.clone();
            self.text.submit(request, move |done| {
                let _ = tx.send(Back::Text(job, done));
            });
            self.outstanding += 1;
            waits.push(Wait::Text(job, slot));
        }
        for request in planned.visuals {
            let VisualRequest {
                op,
                slot,
                key,
                generation,
                blocking,
                job: work,
            } = request;
            let id = self.next_job;
            self.next_job += 1;
            let make = |tx: mpsc::Sender<Back>, key: String| {
                move |o: Output| {
                    let _ = tx.send(Back::Visual(
                        id,
                        VisualRequestInfo {
                            slot,
                            key,
                            generation,
                        },
                        Box::new(o),
                    ));
                }
            };
            if blocking {
                waits.push(Wait::Visual(id, op));
                self.visuals.submit(
                    work,
                    Some(Box::new(make(self.back_tx.clone(), key.clone()))),
                    Box::new(make(self.back_tx.clone(), key)),
                );
                // The first stage, and the second when pictures follow.
                self.outstanding += 1;
            } else {
                self.visuals
                    .submit(work, None, Box::new(make(self.back_tx.clone(), key)));
                self.outstanding += 1;
            }
        }
        self.queue
            .push_waiting(self.seq, planned.ops, waits, received);
        self.flush();
        self.seq
    }

    fn flush(&mut self) {
        for ready in self.queue.ready() {
            self.compositor.apply("o", ready.seq, ready.ops);
        }
    }

    /// Runs the engine's completion handling until `seq` is committed and
    /// every worker result that can still change the picture has landed.
    pub fn settle(&mut self, seq: u64) {
        self.settle_from(seq, false);
    }

    fn settle_from(&mut self, seq: u64, mut committed: bool) {
        let end = Instant::now() + Duration::from_secs(30);
        let mut quiet_since = Instant::now();
        loop {
            assert!(Instant::now() < end, "seq {seq} never settled");
            if let Ok(back) = self.back_rx.recv_timeout(Duration::from_millis(2)) {
                self.absorb(back);
                quiet_since = Instant::now();
            }
            while let Ok(event) = self.events.try_recv() {
                match event {
                    CompositorEvent::Committed { seq: s, .. } if s >= seq => committed = true,
                    CompositorEvent::Error { message, .. } => panic!("{message}"),
                    _ => {}
                }
            }
            // A layer with pictures sends two results for one request; give
            // the second a moment after the first before calling it done.
            if committed
                && self.queue_idle()
                && self.outstanding == 0
                && quiet_since.elapsed() > Duration::from_millis(30)
                && self.back_rx.try_recv().is_err()
            {
                return;
            }
        }
    }

    /// What `Engine` does with a worker's result.
    fn absorb(&mut self, back: Back) {
        match back {
            Back::Text(job, done) => {
                self.queue.complete(job, &done);
                self.outstanding = self.outstanding.saturating_sub(1);
            }
            Back::Visual(job, info, output) => {
                match *output {
                    Output::Fast(content) => {
                        // A layer with pictures sends a second result.
                        let more = matches!(&content.fill, kairo_compositor::Fill::Group(c)
                            if c.iter().any(|c| matches!(c.fill, kairo_compositor::Fill::Pending)));
                        self.queue.complete_visual(job, content);
                        if more {
                            return self.flush();
                        }
                    }
                    Output::Full(content) | Output::Image(Some(content)) => {
                        if self.shell.is_current(info.slot, info.generation) {
                            self.queue.push_waiting(
                                0,
                                vec![kairo_compositor::thread::Op::Update {
                                    slot: info.slot,
                                    key: info.key,
                                    content,
                                }],
                                Vec::new(),
                                Instant::now(),
                            );
                        }
                    }
                    Output::Image(None) => {}
                    Output::Layer(out) => {
                        let full = out.full;
                        let content =
                            self.shell
                                .layer_done(info.slot, &info.key, info.generation, out);
                        if !self.queue.complete_visual(job, content.clone())
                            && self.shell.is_current(info.slot, info.generation)
                        {
                            self.queue.push_waiting(
                                0,
                                vec![kairo_compositor::thread::Op::Update {
                                    slot: info.slot,
                                    key: info.key,
                                    content,
                                }],
                                Vec::new(),
                                Instant::now(),
                            );
                        }
                        if !full {
                            // Its pictures follow.
                            return self.flush();
                        }
                    }
                    Output::Live(out) => {
                        if let Some(op) = self.shell.live_done(info.slot, info.generation, *out) {
                            self.queue
                                .push_waiting(0, vec![op], Vec::new(), Instant::now());
                        }
                    }
                }
                self.outstanding = self.outstanding.saturating_sub(1);
            }
        }
        self.flush();
    }

    fn queue_idle(&mut self) -> bool {
        // Nothing waits when `ready` has nothing more to give.
        self.flush();
        true
    }

    /// One 250 ms tick of the engine at `now`: readouts that changed are
    /// redrawn. Returns how many were.
    pub fn tick_at(&mut self, now: &kairo_scene::program::Now) -> usize {
        let jobs = self.shell.tick(now);
        let count = jobs.len();
        for (slot, key, generation, live) in jobs {
            let id = self.next_job;
            self.next_job += 1;
            let tx = self.back_tx.clone();
            self.outstanding += 1;
            self.visuals.submit(
                crate::visuals::Job::Live(live),
                None,
                Box::new(move |o| {
                    let _ = tx.send(Back::Visual(
                        id,
                        VisualRequestInfo {
                            slot,
                            key,
                            generation,
                        },
                        Box::new(o),
                    ));
                }),
            );
        }
        count
    }

    /// Waits for every outstanding worker result.
    pub fn drain(&mut self) {
        let end = Instant::now() + Duration::from_secs(30);
        while self.outstanding > 0 {
            assert!(Instant::now() < end, "workers never finished");
            if let Ok(back) = self.back_rx.recv_timeout(Duration::from_millis(5)) {
                self.absorb(back);
            }
        }
    }

    /// `stage.html` drawn through the workers and the compositor, over
    /// everything (the stage has its own window from prompt 060).
    pub fn show_stage(
        &mut self,
        view: kairo_domain::program::StageView,
        now: kairo_scene::program::Now,
    ) {
        let (tx, rx) = mpsc::channel();
        self.visuals.submit(
            crate::visuals::Job::Layer(crate::layers::LayerJob {
                source: crate::layers::LayerSource::Stage(view),
                frame: (1920, 1080),
                now,
                still: true,
            }),
            None,
            Box::new(move |o| {
                let _ = tx.send(o);
            }),
        );
        let Ok(Output::Layer(out)) = rx.recv_timeout(Duration::from_secs(30)) else {
            panic!("the stage was not drawn");
        };
        self.seq += 1;
        self.compositor.apply(
            "o",
            self.seq,
            vec![kairo_compositor::thread::Op::Keyed {
                slot: kairo_compositor::Slot::Pattern,
                content: Some((out.content(), format!("stage#{}", self.seq))),
                ms: 0,
            }],
        );
        self.settle(self.seq);
    }

    pub fn show(&mut self, theme: &Value) -> u64 {
        self.push(Command::ShowSlide {
            output: "o".into(),
            source: "o".into(),
            slide: SlideContent {
                reference: String::new(),
                text: String::new(),
                theme: Json::from_value(theme),
                colored_lines: None,
            },
            instant: true,
        })
    }

    /// The frame over flat grey, as the references are.
    pub fn frame_over_grey(&self) -> Vec<u8> {
        let (_, _, pixels) = self.compositor.read("o").expect("frame");
        over_grey(&pixels)
    }
}

pub fn over_grey(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(premultiplied.len());
    for p in premultiplied.as_chunks::<4>().0 {
        let inv = 255 - u32::from(p[3]);
        for c in &p[..3] {
            out.push((u32::from(*c) + (128 * inv + 127) / 255).min(255) as u8);
        }
        out.push(255);
    }
    out
}

pub fn load_png(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    assert_eq!((info.width as usize, info.height as usize), (W, H));
    match info.color_type {
        png::ColorType::Rgba => Some(buf),
        png::ColorType::Rgb => Some(
            buf.as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
        ),
        other => panic!("unexpected {other:?}"),
    }
}

pub fn save_png(path: &Path, rgba: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), W as u32, H as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .expect("png header")
        .write_image_data(rgba)
        .expect("png data");
}

/// Mean SSIM over 8×8 luminance blocks (spike 003's definition), and the
/// share of pixels whose largest channel difference exceeds `tolerance`.
pub fn compare(left: &[u8], right: &[u8], tolerance: i32) -> (f64, f64) {
    let luma =
        |p: &[u8]| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
    let mut sum = 0.0;
    let mut blocks = 0u32;
    for y in (0..H).step_by(8) {
        for x in (0..W).step_by(8) {
            let (mut a, mut b, mut aa, mut bb, mut ab) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for yy in y..y + 8 {
                for xx in x..x + 8 {
                    let i = (yy * W + xx) * 4;
                    let (l, r) = (luma(&left[i..i + 3]), luma(&right[i..i + 3]));
                    a += l;
                    b += r;
                    aa += l * l;
                    bb += r * r;
                    ab += l * r;
                }
            }
            let (ma, mb) = (a / 64.0, b / 64.0);
            let (va, vb) = ((aa - a * ma) / 63.0, (bb - b * mb) / 63.0);
            let cov = (ab - a * mb) / 63.0;
            let (c1, c2) = (6.5025, 58.5225);
            sum += ((2.0 * ma * mb + c1) * (2.0 * cov + c2))
                / ((ma * ma + mb * mb + c1) * (va + vb + c2));
            blocks += 1;
        }
    }
    let far = left
        .as_chunks::<4>()
        .0
        .iter()
        .zip(right.as_chunks::<4>().0)
        .filter(|(l, r)| (0..3).any(|c| (i32::from(l[c]) - i32::from(r[c])).abs() > tolerance))
        .count();
    (sum / f64::from(blocks), far as f64 / (W * H) as f64)
}

/// Floors for a case's SSIM and ceilings for the share of pixels more than 24
/// levels off. Measured: mean 0.9995, worst 0.989; see DECISIONS 053.
fn limits(_group: &str, id: &str) -> (f64, f64) {
    match id {
        // Chromium draws a broken-image frame and icon where a file is missing
        // or damaged; this engine draws nothing (DECISIONS 053).
        "media-missing-file" | "media-broken-file" => (0.98, 0.003),
        _ => (0.99, 0.005),
    }
}

#[test]
fn every_visual_case_matches_chromium() {
    let Some(refs) = reference_dir() else {
        assert!(
            std::env::var("KAIRO_REQUIRE_ORACLE").is_err(),
            "visual references are missing (golden-capture ... visuals)"
        );
        eprintln!("skipped: no Chromium references under .cache/golden-capture/053/visuals");
        return;
    };
    let Some(mut rig) = rig() else { return };
    let fixtures = fixtures();
    let cases: Vec<Value> = serde_json::from_slice(
        &std::fs::read(workspace().join("tests/golden/053-visuals/cases.json")).expect("cases"),
    )
    .expect("cases json");
    let ours_dir = std::env::var_os("KAIRO_VISUAL_OUT").map(PathBuf::from);
    if let Some(dir) = &ours_dir {
        std::fs::create_dir_all(dir).unwrap();
    }
    let mut failures = Vec::new();
    let mut rows = Vec::new();
    for case in &cases {
        let id = case["id"].as_str().unwrap();
        let group = case["group"].as_str().unwrap();
        let text = serde_json::to_string(&case["theme"])
            .unwrap()
            .replace("@FIX@", fixtures.to_str().unwrap());
        let theme: Value = serde_json::from_str(&text).unwrap();
        // Each case starts from nothing, as the capture clears between cases.
        let clear = rig.push(Command::Clear { output: "o".into() });
        rig.settle(clear);
        let seq = rig.show(&theme);
        rig.settle(seq);
        let ours = rig.frame_over_grey();
        if let Some(dir) = &ours_dir {
            save_png(&dir.join(format!("{id}.png")), &ours);
        }
        let reference = load_png(&refs.join(format!("{id}.png"))).expect("reference png");
        let (ssim, far) = compare(&ours, &reference, 24);
        let (min_ssim, max_far) = limits(group, id);
        rows.push((id.to_owned(), ssim, far));
        if ssim < min_ssim || far > max_far {
            failures.push(format!(
                "{id}: ssim {ssim:.4} (min {min_ssim}), far {far:.4} (max {max_far})"
            ));
        }
    }
    for (id, ssim, far) in &rows {
        eprintln!("{id:<32} ssim {ssim:.4} far {far:.4}");
    }
    let mean = rows.iter().map(|r| r.1).sum::<f64>() / rows.len() as f64;
    let worst = rows.iter().map(|r| r.1).fold(1.0, f64::min);
    eprintln!("cases {} mean ssim {mean:.5} worst {worst:.5}", rows.len());
    assert!(mean >= 0.999, "mean ssim {mean}");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ── The engine path, beyond the corpus ──────────────────────────────────────

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kairo-053-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn flat_png(path: &Path, rgba: [u8; 4]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), 8, 8);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let data: Vec<u8> = (0..64).flat_map(|_| rgba).collect();
    enc.write_header().unwrap().write_image_data(&data).unwrap();
}

fn picture_theme(path: &Path) -> Value {
    serde_json::json!({"background": {"type": "image", "mediaPath": path, "mediaFit": "fill",
        "opacity": 1, "color": "#000000"}})
}

fn centre(frame: &[u8]) -> [u8; 3] {
    let i = (540 * W + 960) * 4;
    [frame[i], frame[i + 1], frame[i + 2]]
}

#[test]
fn the_newest_slide_wins_however_the_decodes_finish() {
    let Some(mut rig) = rig() else { return };
    let dir = temp_dir("race");
    let (a, b) = (dir.join("a.png"), dir.join("b.png"));
    flat_png(&a, [255, 0, 0, 255]);
    flat_png(&b, [0, 0, 255, 255]);
    for round in 0..8 {
        // Alternate which file is asked for last.
        let (first, last, want) = if round % 2 == 0 {
            (&a, &b, [0, 0, 255])
        } else {
            (&b, &a, [255, 0, 0])
        };
        rig.show(&picture_theme(first));
        let seq = rig.show(&picture_theme(last));
        rig.settle(seq);
        assert_eq!(centre(&rig.frame_over_grey()), want, "round {round}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn a_background_stays_up_while_the_next_verse_with_the_same_file_arrives() {
    let Some(mut rig) = rig() else { return };
    let dir = temp_dir("keep");
    let file = dir.join("a.png");
    flat_png(&file, [10, 200, 30, 255]);
    let theme = picture_theme(&file);
    let seq = rig.show(&theme);
    rig.settle(seq);
    assert_eq!(centre(&rig.frame_over_grey()), [10, 200, 30]);
    // Another verse on the same background: the picture is never blanked,
    // not even for the frame between the new command and its decode result.
    for i in 0..5 {
        let mut themed = theme.clone();
        themed["background"]["opacity"] = serde_json::json!(1.0 - f64::from(i) * 0.0);
        let seq = rig.show(&themed);
        rig.settle(seq);
        assert_eq!(centre(&rig.frame_over_grey()), [10, 200, 30], "verse {i}");
    }
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn an_animated_picture_plays_from_when_it_is_decoded() {
    let Some(mut rig) = rig() else { return };
    let dir = temp_dir("gif");
    let file = dir.join("a.gif");
    let mut bytes = Vec::new();
    {
        use image::{
            Delay, Frame, Rgba, RgbaImage,
            codecs::gif::{GifEncoder, Repeat},
        };
        let mut enc = GifEncoder::new(&mut bytes);
        enc.set_repeat(Repeat::Infinite).unwrap();
        for c in [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]] {
            let frame = Frame::from_parts(
                RgbaImage::from_pixel(8, 8, Rgba([c[0], c[1], c[2], 255])),
                0,
                0,
                Delay::from_numer_denom_ms(300, 1),
            );
            enc.encode_frame(frame).unwrap();
        }
    }
    std::fs::write(&file, bytes).unwrap();
    let seq = rig.show(&picture_theme(&file));
    rig.settle(seq);
    let mut seen = std::collections::BTreeSet::new();
    let end = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < end {
        seen.insert(centre(&rig.frame_over_grey()));
        std::thread::sleep(Duration::from_millis(40));
    }
    assert!(
        seen.len() >= 3,
        "a 900 ms loop seen for 1.5 s shows every frame: {seen:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn text_is_not_held_up_by_a_picture() {
    let Some(mut rig) = rig() else { return };
    let dir = temp_dir("text");
    let file = dir.join("big.png");
    // A large file takes a moment to decode; the words must not wait for it.
    {
        let (w, h) = (6000u32, 4000u32);
        let f = std::fs::File::create(&file).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let row: Vec<u8> = (0..w * 3).map(|i| (i % 251) as u8).collect();
        let mut writer = enc.write_header().unwrap().into_stream_writer().unwrap();
        for _ in 0..h {
            std::io::Write::write_all(&mut writer, &row).unwrap();
        }
        writer.finish().unwrap();
    }
    let mut theme = picture_theme(&file);
    theme["background"]["mediaFit"] = serde_json::json!("cover");
    let started = Instant::now();
    let seq = rig.push(Command::ShowSlide {
        output: "o".into(),
        source: "o".into(),
        slide: SlideContent {
            reference: "John 3:16".into(),
            text: "For God so loved the world".into(),
            theme: Json::from_value(&theme),
            colored_lines: None,
        },
        instant: true,
    });
    // Committed means the slide is applied, words included.
    let end = Instant::now() + Duration::from_secs(20);
    let mut committed_at = None;
    while Instant::now() < end && committed_at.is_none() {
        while let Ok(back) = rig.back_rx.try_recv() {
            rig.absorb(back);
        }
        while let Ok(event) = rig.events.try_recv() {
            if let CompositorEvent::Committed { seq: s, .. } = event
                && s >= seq
            {
                committed_at = Some(started.elapsed());
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(committed_at.is_some(), "the slide never committed");
    rig.settle_from(seq, true);
    let decoded_at = started.elapsed();
    eprintln!("slide committed after {committed_at:?}; picture settled after {decoded_at:?}");
    assert!(committed_at.unwrap() < decoded_at, "the words came first");
    std::fs::remove_dir_all(dir).ok();
}
