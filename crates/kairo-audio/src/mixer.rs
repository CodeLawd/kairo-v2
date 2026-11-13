//! The mixer: what `overlay.html`'s Web Audio graph did. Every source runs
//! through its own speaker gain into the output (`config.audio.enabled ?
//! volume : 0`), and through a shared bus, at full level, into each NDI tap
//! it belongs to (`source.connect(bus)` before the gain).
//!
//! [`Mixer::render`] runs on the audio thread and never locks or
//! allocates: sources arrive and leave through rings, their settings are
//! atomics, and a removed source goes back through a ring to be dropped
//! elsewhere. Sources push 48 kHz interleaved stereo.

use crate::block::BLOCK_FRAMES;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// The program sample rate (`new AudioContext({ sampleRate: 48000 })`).
pub const RATE: u32 = 48_000;
pub const MAX_SOURCES: usize = 32;
pub const MAX_TAPS: usize = 8;
/// The most frames one call renders.
pub const MAX_CHUNK: usize = 1024;
/// What a source can queue ahead: two seconds.
pub const SOURCE_FRAMES: usize = 2 * RATE as usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Video,
    DocumentVideo,
    Camera,
    Web,
    Tracks,
}

/// A source's settings, shared between the control side and the mixer.
#[derive(Debug)]
pub struct SourceControl {
    pub kind: SourceKind,
    gain: AtomicU32,
    taps: AtomicU8,
    playing: AtomicBool,
    held: AtomicBool,
    flush: AtomicBool,
    consumed: AtomicU64,
    underruns: AtomicU64,
}

impl SourceControl {
    fn new(kind: SourceKind) -> Self {
        Self {
            kind,
            gain: AtomicU32::new(0f32.to_bits()),
            taps: AtomicU8::new(0),
            playing: AtomicBool::new(false),
            held: AtomicBool::new(false),
            flush: AtomicBool::new(false),
            consumed: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
        }
    }

    /// The speaker gain, 0 to 1 (`speaker.gain.value`).
    pub fn set_gain(&self, gain: f32) {
        let g = if gain.is_finite() {
            gain.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.gain.store(g.to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain.load(Ordering::Relaxed))
    }

    /// Which taps hear this source, one bit each.
    pub fn set_taps(&self, mask: u8) {
        self.taps.store(mask, Ordering::Relaxed);
    }

    pub fn taps(&self) -> u8 {
        self.taps.load(Ordering::Relaxed)
    }

    /// While playing, a source that runs dry is an underrun (a glitch).
    pub fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Release);
    }

    /// Held: the mixer takes nothing from this source until released, so a
    /// player can queue enough to ride out a slow decoder before it is heard.
    pub fn set_held(&self, held: bool) {
        self.held.store(held, Ordering::Release);
    }

    /// Drop what is queued (a seek): the next samples pushed play next.
    pub fn flush(&self) {
        self.flush.store(true, Ordering::Release);
    }

    pub fn flush_pending(&self) -> bool {
        self.flush.load(Ordering::Acquire)
    }

    /// Frames the mixer has taken from this source.
    pub fn consumed(&self) -> u64 {
        self.consumed.load(Ordering::Acquire)
    }

    /// Frames of silence the mixer had to play while the source was playing.
    pub fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

/// The feeding end of a source: push interleaved stereo frames.
pub struct SourceInput {
    producer: rtrb::Producer<f32>,
    pub control: Arc<SourceControl>,
}

impl SourceInput {
    /// Frames there is room for now.
    pub fn room(&self) -> usize {
        self.producer.slots() / 2
    }

    /// Pushes as many whole frames of `interleaved` as fit; returns frames pushed.
    pub fn push(&mut self, interleaved: &[f32]) -> usize {
        let frames = (interleaved.len() / 2).min(self.room());
        if frames == 0 {
            return 0;
        }
        if let Ok(mut chunk) = self.producer.write_chunk(frames * 2) {
            let (a, b) = chunk.as_mut_slices();
            let (first, rest) = interleaved[..frames * 2].split_at(a.len());
            a.copy_from_slice(first);
            b.copy_from_slice(rest);
            chunk.commit_all();
            frames
        } else {
            0
        }
    }

    /// Frames waiting in the ring.
    pub fn queued(&self) -> usize {
        (self.producer.buffer().capacity() - self.producer.slots()) / 2
    }
}

struct Source {
    id: u64,
    consumer: rtrb::Consumer<f32>,
    control: Arc<SourceControl>,
    /// The gain at the end of the last render, for a click-free change.
    gain: f32,
}

enum Message {
    Add(Box<Source>),
    Remove(u64),
}

/// The control side of a mixer: makes sources and taps.
pub struct MixerControl {
    inbox: rtrb::Producer<Message>,
    trash: rtrb::Consumer<Box<Source>>,
    taps: Vec<Arc<TapSwitch>>,
    next_id: u64,
}

/// A tap's on switch and its counters.
#[derive(Debug, Default)]
pub struct TapSwitch {
    on: AtomicBool,
    dropped: AtomicU64,
    blocks: AtomicU64,
}

impl TapSwitch {
    /// `tap.port.postMessage(config.ndiAudio)`: off does no work at all.
    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Release);
    }

    pub fn is_on(&self) -> bool {
        self.on.load(Ordering::Acquire)
    }

    /// Blocks lost because the sender fell behind.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Blocks handed to the sender.
    pub fn blocks(&self) -> u64 {
        self.blocks.load(Ordering::Relaxed)
    }
}

/// What a tap hands its sender: planar stereo blocks of [`BLOCK_FRAMES`].
pub struct TapOutput {
    pub consumer: rtrb::Consumer<f32>,
    pub switch: Arc<TapSwitch>,
}

struct Tap {
    switch: Arc<TapSwitch>,
    left: Box<[f32; BLOCK_FRAMES]>,
    right: Box<[f32; BLOCK_FRAMES]>,
    filled: usize,
    out: rtrb::Producer<f32>,
    bus: Box<[f32]>,
}

impl Tap {
    /// `KairoTap.process`: collect frames, post a planar block every 1024.
    fn take(&mut self, frames: usize) {
        for i in 0..frames {
            self.left[self.filled] = self.bus[i * 2];
            self.right[self.filled] = self.bus[i * 2 + 1];
            self.filled += 1;
            if self.filled == BLOCK_FRAMES {
                self.filled = 0;
                match self.out.write_chunk(BLOCK_FRAMES * 2) {
                    Ok(mut chunk) => {
                        let (a, b) = chunk.as_mut_slices();
                        let mut planar = self.left.iter().chain(self.right.iter());
                        for v in a.iter_mut().chain(b.iter_mut()) {
                            *v = *planar.next().unwrap_or(&0.0);
                        }
                        chunk.commit_all();
                        self.switch.blocks.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(_) => {
                        self.switch.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

/// The audio thread's end.
pub struct Mixer {
    slots: Vec<Option<Box<Source>>>,
    inbox: rtrb::Consumer<Message>,
    trash: rtrb::Producer<Box<Source>>,
    taps: Vec<Tap>,
}

/// A mixer and its control side, with `taps` NDI taps (at most [`MAX_TAPS`]).
pub fn mixer(taps: usize) -> (Mixer, MixerControl, Vec<TapOutput>) {
    let (inbox_tx, inbox_rx) = rtrb::RingBuffer::new(MAX_SOURCES * 2);
    let (trash_tx, trash_rx) = rtrb::RingBuffer::new(MAX_SOURCES * 2);
    let mut tap_states = Vec::new();
    let mut outputs = Vec::new();
    let mut switches = Vec::new();
    for _ in 0..taps.min(MAX_TAPS) {
        let switch = Arc::new(TapSwitch::default());
        // A second of blocks queued for the sender.
        let (out, consumer) = rtrb::RingBuffer::new(BLOCK_FRAMES * 2 * 48);
        tap_states.push(Tap {
            switch: switch.clone(),
            left: Box::new([0.0; BLOCK_FRAMES]),
            right: Box::new([0.0; BLOCK_FRAMES]),
            filled: 0,
            out,
            bus: vec![0.0; MAX_CHUNK * 2].into_boxed_slice(),
        });
        outputs.push(TapOutput {
            consumer,
            switch: switch.clone(),
        });
        switches.push(switch);
    }
    let mut slots = Vec::with_capacity(MAX_SOURCES);
    slots.resize_with(MAX_SOURCES, || None);
    (
        Mixer {
            slots,
            inbox: inbox_rx,
            trash: trash_tx,
            taps: tap_states,
        },
        MixerControl {
            inbox: inbox_tx,
            trash: trash_rx,
            taps: switches,
            next_id: 0,
        },
        outputs,
    )
}

impl MixerControl {
    /// A new source; `None` when the mixer is full.
    pub fn add(&mut self, kind: SourceKind) -> Option<(u64, SourceInput)> {
        self.collect();
        let (producer, consumer) = rtrb::RingBuffer::new(SOURCE_FRAMES * 2);
        let control = Arc::new(SourceControl::new(kind));
        self.next_id += 1;
        let id = self.next_id;
        let source = Box::new(Source {
            id,
            consumer,
            control: control.clone(),
            gain: 0.0,
        });
        self.inbox.push(Message::Add(source)).ok()?;
        Some((id, SourceInput { producer, control }))
    }

    pub fn remove(&mut self, id: u64) {
        self.collect();
        let _ = self.inbox.push(Message::Remove(id));
    }

    pub fn tap(&self, index: usize) -> Option<&Arc<TapSwitch>> {
        self.taps.get(index)
    }

    /// Drops sources the mixer gave back, off the audio thread.
    pub fn collect(&mut self) {
        while self.trash.pop().is_ok() {}
    }
}

impl Mixer {
    fn receive(&mut self) {
        while let Ok(message) = self.inbox.pop() {
            match message {
                Message::Add(source) => {
                    if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
                        *slot = Some(source);
                    } else if let Err(rtrb::PushError::Full(source)) = self.trash.push(source) {
                        // Full both ways: never free on this thread.
                        std::mem::forget(source);
                    }
                }
                Message::Remove(id) => {
                    for slot in &mut self.slots {
                        if slot.as_ref().is_some_and(|s| s.id == id)
                            && let Some(source) = slot.take()
                            && let Err(rtrb::PushError::Full(source)) = self.trash.push(source)
                        {
                            std::mem::forget(source);
                        }
                    }
                }
            }
        }
    }

    /// Renders `speaker.len() / 2` frames (at most [`MAX_CHUNK`]) of
    /// interleaved stereo into `speaker`, and feeds the taps that are on.
    pub fn render(&mut self, speaker: &mut [f32]) {
        self.receive();
        let frames = (speaker.len() / 2).min(MAX_CHUNK);
        let speaker = &mut speaker[..frames * 2];
        speaker.fill(0.0);
        let mut tapping = 0u8;
        for (i, tap) in self.taps.iter_mut().enumerate() {
            if tap.switch.is_on() {
                tapping |= 1 << i;
                tap.bus[..frames * 2].fill(0.0);
            }
        }
        for source in self.slots.iter_mut().flatten() {
            let control = &source.control;
            if control.flush.swap(false, Ordering::AcqRel) {
                let queued = source.consumer.slots();
                if let Ok(chunk) = source.consumer.read_chunk(queued) {
                    chunk.commit_all();
                }
            }
            if control.held.load(Ordering::Acquire) {
                continue;
            }
            let available = source.consumer.slots() / 2;
            let take = available.min(frames);
            if take < frames && control.playing.load(Ordering::Acquire) {
                control
                    .underruns
                    .fetch_add((frames - take) as u64, Ordering::Relaxed);
            }
            if take == 0 {
                continue;
            }
            let target = control.gain();
            let from = source.gain;
            let mask = control.taps() & tapping;
            let Ok(chunk) = source.consumer.read_chunk(take * 2) else {
                continue;
            };
            let (a, b) = chunk.as_slices();
            let mut samples = a.iter().chain(b.iter());
            let step = (target - from) / take as f32;
            for f in 0..take {
                let l = *samples.next().unwrap_or(&0.0);
                let r = *samples.next().unwrap_or(&0.0);
                let g = from + step * (f + 1) as f32;
                speaker[f * 2] += l * g;
                speaker[f * 2 + 1] += r * g;
                if mask != 0 {
                    for (t, tap) in self.taps.iter_mut().enumerate() {
                        if mask & (1 << t) != 0 {
                            tap.bus[f * 2] += l;
                            tap.bus[f * 2 + 1] += r;
                        }
                    }
                }
            }
            chunk.commit_all();
            source.gain = target;
            control.consumed.fetch_add(take as u64, Ordering::AcqRel);
        }
        for tap in &mut self.taps {
            if tap.switch.is_on() {
                tap.take(frames);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, l: f32, r: f32) -> Vec<f32> {
        (0..frames).flat_map(|_| [l, r]).collect()
    }

    #[test]
    fn sources_sum_through_their_own_gain() {
        let (mut mixer, mut control, _) = mixer(0);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        let (_, mut b) = control.add(SourceKind::Tracks).unwrap();
        a.control.set_gain(0.5);
        b.control.set_gain(1.0);
        let mut out = vec![0.0; 2 * 256];
        // First render picks the sources up and ramps from silence.
        a.push(&tone(1024, 0.4, 0.2));
        b.push(&tone(1024, 0.1, 0.1));
        mixer.render(&mut out);
        mixer.render(&mut out);
        assert!((out[0] - (0.4 * 0.5 + 0.1)).abs() < 1e-6, "{}", out[0]);
        assert!((out[1] - (0.2 * 0.5 + 0.1)).abs() < 1e-6);
        assert_eq!(a.control.consumed(), 512);
    }

    #[test]
    fn a_gain_change_ramps_across_one_render() {
        let (mut mixer, mut control, _) = mixer(0);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        a.control.set_gain(1.0);
        a.push(&tone(2048, 1.0, 1.0));
        let mut out = vec![0.0; 2 * 100];
        mixer.render(&mut out);
        // From 0 up to 1 over the 100 frames.
        assert!((out[0] - 0.01).abs() < 1e-6 && (out[198] - 1.0).abs() < 1e-6);
        a.control.set_gain(0.0);
        mixer.render(&mut out);
        assert!(out[198].abs() < 1e-6 && out[0] > 0.98);
    }

    #[test]
    fn a_playing_source_that_runs_dry_counts_underruns_and_a_paused_one_does_not() {
        let (mut mixer, mut control, _) = mixer(0);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        a.push(&tone(10, 1.0, 1.0));
        let mut out = vec![0.0; 2 * 64];
        mixer.render(&mut out);
        assert_eq!(a.control.underruns(), 0);
        a.control.set_playing(true);
        mixer.render(&mut out);
        assert_eq!(a.control.underruns(), 64);
    }

    #[test]
    fn a_held_source_is_neither_heard_nor_drained_until_released() {
        let (mut mixer, mut control, _) = mixer(0);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        a.control.set_gain(1.0);
        a.control.set_held(true);
        a.push(&tone(64, 0.5, 0.5));
        let mut out = vec![0.0; 2 * 64];
        mixer.render(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(a.queued(), 64);
        assert_eq!(a.control.underruns(), 0);
        a.control.set_held(false);
        mixer.render(&mut out);
        assert_eq!(a.queued(), 0);
        assert!(out[127] > 0.0);
    }

    #[test]
    fn the_tap_hears_full_level_in_planar_blocks_and_only_when_on() {
        let (mut mixer, mut control, mut taps) = mixer(2);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        a.control.set_gain(0.0);
        a.control.set_taps(0b01);
        a.push(&tone(4096, 0.25, -0.5));
        let mut out = vec![0.0; 2 * 512];
        // Off: nothing is collected.
        for _ in 0..2 {
            mixer.render(&mut out);
        }
        assert_eq!(taps[0].consumer.slots(), 0);
        control.tap(0).unwrap().set_on(true);
        control.tap(1).unwrap().set_on(true);
        for _ in 0..4 {
            mixer.render(&mut out);
        }
        // 2048 frames: two blocks each. Tap 1 has no source, so its blocks
        // are silence: the bus is always connected, so the worklet posts.
        assert_eq!(taps[0].consumer.slots(), 2 * 2048);
        assert_eq!(taps[1].consumer.slots(), 2 * 2048);
        let silent = taps[1].consumer.read_chunk(2048).unwrap();
        assert!(silent.as_slices().0.iter().all(|v| *v == 0.0));
        silent.commit_all();
        let block = taps[0].consumer.read_chunk(2048).unwrap();
        let (data, _) = block.as_slices();
        // Speaker gain 0, the tap still at full level: left run, then right.
        assert_eq!(data[0], 0.25);
        assert_eq!(data[1023], 0.25);
        assert_eq!(data[1024], -0.5);
        assert!(out.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn removed_sources_go_back_to_be_dropped_and_a_full_mixer_refuses() {
        let (mut mixer, mut control, _) = mixer(0);
        let mut inputs = Vec::new();
        for _ in 0..MAX_SOURCES {
            inputs.push(control.add(SourceKind::Video).unwrap());
        }
        let mut out = vec![0.0; 2 * 8];
        mixer.render(&mut out);
        assert_eq!(mixer.slots.iter().flatten().count(), MAX_SOURCES);
        // One more than fits is handed straight back.
        let _extra = control.add(SourceKind::Video).unwrap();
        mixer.render(&mut out);
        control.remove(inputs[0].0);
        mixer.render(&mut out);
        assert_eq!(mixer.slots.iter().flatten().count(), MAX_SOURCES - 1);
        control.collect();
    }

    #[test]
    fn a_flush_drops_what_was_queued() {
        let (mut mixer, mut control, _) = mixer(0);
        let (_, mut a) = control.add(SourceKind::Video).unwrap();
        a.control.set_gain(1.0);
        a.push(&tone(1000, 1.0, 1.0));
        let mut out = vec![0.0; 2 * 8];
        mixer.render(&mut out);
        a.control.flush();
        mixer.render(&mut out);
        assert_eq!(a.queued(), 0);
    }
}
