//! Time-based scrolling state. Positions are expressed in shaped lines.

use std::time::Instant;

pub struct Playback {
    revision: u64,
    sequence: u64,
    anchored_at: Instant,
    position: f64,
    speed: f64,
    playing: bool,
}

#[derive(Clone, Copy)]
pub enum Transport {
    Play,
    Pause,
    Seek(f64),
    Speed(f64),
}

impl Playback {
    pub fn new(revision: u64, position: f64, now: Instant) -> Self {
        Self {
            revision,
            sequence: 0,
            anchored_at: now,
            position,
            speed: 1.0,
            playing: false,
        }
    }

    pub fn apply(
        &mut self,
        revision: u64,
        sequence: u64,
        operation: Transport,
        end: f64,
        now: Instant,
    ) -> bool {
        if revision != self.revision || sequence <= self.sequence {
            return false;
        }
        if matches!(operation, Transport::Seek(value) | Transport::Speed(value) if !value.is_finite())
        {
            return false;
        }
        self.sample(end, now);
        self.sequence = sequence;
        match operation {
            Transport::Play => self.playing = self.position < end,
            Transport::Pause => self.playing = false,
            Transport::Seek(position) => {
                self.position = position.clamp(0.0, end);
                self.playing &= self.position < end;
            }
            Transport::Speed(speed) => self.speed = speed.clamp(0.1, 20.0),
        }
        true
    }

    /// Advance to the frame timestamp and commit the natural end transition.
    pub fn sample(&mut self, end: f64, now: Instant) -> (f64, bool) {
        let now = now.max(self.anchored_at);
        if self.playing {
            self.position += now
                .saturating_duration_since(self.anchored_at)
                .as_secs_f64()
                * self.speed;
        }
        self.anchored_at = now;
        self.position = self.position.clamp(0.0, end);
        self.playing &= self.position < end;
        (self.position, self.playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn samples_monotonic_time_and_reanchors_transport() {
        let now = Instant::now();
        let at = |seconds| now + Duration::from_secs(seconds);
        let mut playback = Playback::new(3, 2.0, now);
        assert_eq!(playback.sample(10.0, at(1)), (2.0, false));
        assert!(playback.apply(3, 1, Transport::Play, 10.0, at(1)));
        assert!(playback.apply(3, 2, Transport::Speed(2.0), 10.0, at(2)));
        assert!(playback.apply(3, 3, Transport::Pause, 10.0, at(3)));
        assert!(!playback.apply(2, 4, Transport::Play, 10.0, at(4)));
        assert!(!playback.apply(3, 2, Transport::Play, 10.0, at(4)));
        assert_eq!(playback.sample(10.0, at(5)), (5.0, false));
        assert!(playback.apply(3, 4, Transport::Seek(8.0), 10.0, at(5)));
        assert!(playback.apply(3, 5, Transport::Play, 10.0, at(5)));
        assert_eq!(playback.sample(10.0, at(9)), (10.0, false));
        assert!(playback.apply(3, 6, Transport::Play, 10.0, at(9)));
        assert_eq!(playback.sample(10.0, at(10)), (10.0, false));
    }
}

pub struct StagePlayback {
    pub key: String,
    /// Where it draws: the background of a stage display, or the prompter
    /// slot over an audience screen.
    pub slot: kairo_compositor::Slot,
    layout: kairo_scene::program::teleprompter::ScriptLayout,
    playback: Playback,
    view: kairo_domain::teleprompter::TeleprompterView,
    dirty: bool,
    next_frame: Instant,
    next_checkpoint: Instant,
    checkpoint_pending: bool,
}

impl StagePlayback {
    pub fn new(
        key: String,
        view: kairo_domain::teleprompter::TeleprompterView,
        frame: (u32, u32),
        now: Instant,
        previous: Option<Self>,
    ) -> Self {
        let layout = kairo_scene::with_typesetter(|ts| {
            kairo_scene::program::teleprompter::ScriptLayout::new(
                ts,
                &view.text,
                frame,
                view.font_px,
            )
        });
        let mut playback =
            Playback::new(view.revision, layout.position_for_anchor(view.anchor), now);
        // The view carries the operator's speed, so a reload, a reopened
        // window or a replay after an engine restart keeps it.
        playback.speed = kairo_domain::teleprompter::clamp_speed(view.speed);
        if let Some(mut previous) = previous {
            let (position, playing) = previous.playback.sample(previous.layout.end(), now);
            if !view.script_id.is_empty() && view.script_id == previous.view.script_id {
                let anchor = kairo_domain::teleprompter::remap_anchor(
                    &previous.view.text,
                    &view.text,
                    previous.layout.anchor_at(position),
                );
                playback.position = layout.position_for_anchor(anchor);
                playback.playing = playing && playback.position < layout.end();
                playback.sequence = previous.playback.sequence;
            }
        }
        Self {
            key,
            slot: kairo_compositor::Slot::Background,
            layout,
            playback,
            view,
            dirty: true,
            next_frame: now,
            next_checkpoint: now,
            checkpoint_pending: true,
        }
    }

    pub fn transport(
        &mut self,
        revision: u64,
        sequence: u64,
        operation: kairo_engine_proto::StageOperation,
        now: Instant,
    ) -> bool {
        use kairo_engine_proto::StageOperation;
        let end = self.layout.end();
        if revision != self.playback.revision || sequence <= self.playback.sequence {
            return false;
        }
        let operation = match operation {
            StageOperation::Play => Transport::Play,
            StageOperation::Pause => Transport::Pause,
            StageOperation::Seek { anchor } => {
                Transport::Seek(self.layout.position_for_anchor(anchor))
            }
            StageOperation::Scroll { lines } if lines.is_finite() => {
                let (position, _) = self.playback.sample(end, now);
                Transport::Seek(position + lines.clamp(-end, end))
            }
            StageOperation::Scroll { .. } => return false,
            StageOperation::Speed { lines_per_second } => Transport::Speed(lines_per_second),
        };
        if !self.playback.apply(revision, sequence, operation, end, now) {
            return false;
        }
        self.dirty = true;
        self.next_frame = now;
        self.checkpoint_pending = true;
        true
    }

    pub fn deadline(&self) -> Option<Instant> {
        (self.dirty || self.playback.playing).then_some(self.next_frame)
    }

    pub fn draw(&mut self, now: Instant) -> Option<kairo_compositor::Content> {
        if self.deadline().is_none_or(|deadline| deadline > now) {
            return None;
        }
        let was_playing = self.playback.playing;
        let (position, playing) = self.playback.sample(self.layout.end(), now);
        self.checkpoint_pending |= was_playing != playing;
        self.dirty = false;
        let period = std::time::Duration::from_nanos(16_666_667);
        let next = self.next_frame + period;
        self.next_frame = if next > now { next } else { now + period };
        let mirror = self.view.mirror;
        kairo_scene::with_typesetter(|ts| self.layout.draw(ts, position))
            .map(|tile| if mirror { mirrored(tile) } else { tile })
            .map(crate::layers::image)
    }
    pub fn checkpoint(
        &mut self,
        output: String,
        now: Instant,
    ) -> Option<kairo_engine_proto::StagePosition> {
        if !self.checkpoint_pending && (!self.playback.playing || self.next_checkpoint > now) {
            return None;
        }
        self.checkpoint_pending = false;
        // Often enough for the operator's preview to follow the screen.
        self.next_checkpoint = now + std::time::Duration::from_millis(250);
        Some(kairo_engine_proto::StagePosition {
            output,
            revision: self.playback.revision,
            sequence: self.playback.sequence,
            anchor: self.layout.anchor_at(self.playback.position),
            playing: self.playback.playing,
            line: self.playback.position,
            lines: self.layout.end(),
            speed: self.playback.speed,
        })
    }

    pub fn park(&mut self, now: Instant) {
        self.playback.sample(self.layout.end(), now);
        self.playback.playing = false;
        self.dirty = true;
        self.checkpoint_pending = true;
    }
    pub fn disconnect(&mut self, now: Instant) {
        self.park(now);
        self.playback.sequence = 0;
    }
}

/// Flip a drawn viewport left to right for a beam-splitter rig.
fn mirrored(mut tile: kairo_scene::vector::Tile) -> kairo_scene::vector::Tile {
    let row = tile.width as usize * 4;
    if row > 0 {
        for line in tile.pixels.chunks_exact_mut(row) {
            let pixels = line.len() / 4;
            for x in 0..pixels / 2 {
                let (left, right) = (x * 4, (pixels - 1 - x) * 4);
                for channel in 0..4 {
                    line.swap(left + channel, right + channel);
                }
            }
        }
    }
    tile
}

#[cfg(test)]
#[test]
fn mirroring_flips_each_row() {
    let tile = kairo_scene::vector::Tile {
        x: 0,
        y: 0,
        width: 3,
        height: 1,
        pixels: vec![1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3],
    };
    assert_eq!(
        mirrored(tile).pixels,
        vec![3, 3, 3, 3, 2, 2, 2, 2, 1, 1, 1, 1]
    );
}
