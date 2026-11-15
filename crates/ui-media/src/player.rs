//! Private preview playback (choice for prompt 083, see DECISIONS 083):
//! the file is decoded in this process through
//! [`MediaBackend::open_preview`] on GPUI's background executor, scaled to a
//! thumbnail width, never through
//! the engine or a screen. Brightness, contrast and saturation are applied
//! to the decoded pixels, so a slider moves without restarting the video;
//! the worker keeps the last frame and re-grades it while paused.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, RenderImage, Task, Window};
use image::{Frame as ImageFrame, RgbaImage};

use crate::backend::{Frame, MediaBackend};

/// Colour adjustments (1.0 is unchanged).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grade {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
}

impl Default for Grade {
    fn default() -> Self {
        Self {
            brightness: 1.,
            contrast: 1.,
            saturation: 1.,
        }
    }
}

/// What the preview plays between.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub start: f64,
    /// `None`: the file's end.
    pub end: Option<f64>,
    pub looped: bool,
    pub rate: f64,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            start: 0.,
            end: None,
            looped: true,
            rate: 1.,
        }
    }
}

enum Command {
    Play,
    Pause,
    Seek(f64),
    Range(Range),
    Grade(Grade),
    Stop,
}

enum Update {
    Opened { duration: Option<f64> },
    Frame { frame: Frame },
    Playing(bool),
    Failed(String),
}

/// CSS filter order: brightness, contrast, saturate.
pub fn grade_pixels(pixels: &mut [u8], g: Grade) {
    if g == Grade::default() {
        return;
    }
    let (b, c, s) = (g.brightness as f32, g.contrast as f32, g.saturation as f32);
    for px in pixels.as_chunks_mut::<4>().0 {
        let mut rgb = [
            px[0] as f32 / 255.,
            px[1] as f32 / 255.,
            px[2] as f32 / 255.,
        ];
        for v in &mut rgb {
            *v = ((*v * b - 0.5) * c + 0.5).clamp(0., 1.);
        }
        let l = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
        for (i, v) in rgb.iter().enumerate() {
            px[i] = ((l + (v - l) * s).clamp(0., 1.) * 255.).round() as u8;
        }
    }
}

async fn worker(
    executor: gpui_kit::BackgroundExecutor,
    backend: Arc<dyn MediaBackend>,
    path: String,
    width: u32,
    mut commands: futures_channel::mpsc::UnboundedReceiver<Command>,
    updates: futures_channel::mpsc::UnboundedSender<Update>,
    autoplay: bool,
) {
    use futures_util::StreamExt;
    use futures_util::future::{Either, select};
    let send = |u: Update| updates.unbounded_send(u).is_ok();
    let mut source = match backend.open_preview(&path, width) {
        Ok(s) => s,
        Err(e) => {
            send(Update::Failed(e));
            return;
        }
    };
    if !send(Update::Opened {
        duration: source.duration(),
    }) {
        return;
    }
    let mut range = Range::default();
    let mut grade = Grade::default();
    let mut playing = autoplay;
    let mut last: Option<Frame> = None;
    let show = |raw: &Frame, grade: Grade| {
        let mut f = raw.clone();
        grade_pixels(&mut f.pixels, grade);
        send(Update::Frame { frame: f })
    };
    // The first frame, so a paused preview shows the picture.
    match source.next_frame() {
        Ok(Some(f)) => {
            show(&f, grade);
            last = Some(f);
        }
        Ok(None) => {}
        Err(e) => {
            send(Update::Failed(e));
            return;
        }
    }
    send(Update::Playing(playing));
    loop {
        let command = if playing {
            let wait = Duration::from_secs_f64(source.interval() / range.rate.clamp(0.5, 2.));
            match select(commands.next(), executor.timer(wait)).await {
                Either::Left((Some(c), _)) => Some(c),
                Either::Left((None, _)) => return,
                Either::Right(_) => None,
            }
        } else {
            match commands.next().await {
                Some(c) => Some(c),
                None => return,
            }
        };
        match command {
            Some(Command::Stop) => return,
            Some(Command::Play) => {
                playing = true;
                // Ended or outside the range: from the start.
                let at = last.as_ref().map_or(0., |f| f.seconds);
                let end = range.end.or(source.duration()).unwrap_or(f64::INFINITY);
                if at < range.start || at >= end - 1e-3 {
                    let _ = source.seek(range.start);
                }
                send(Update::Playing(true));
                continue;
            }
            Some(Command::Pause) => {
                playing = false;
                send(Update::Playing(false));
                continue;
            }
            Some(Command::Range(r)) => {
                range = r;
                continue;
            }
            Some(Command::Grade(g)) => {
                grade = g;
                if let Some(f) = &last {
                    show(f, grade);
                }
                continue;
            }
            Some(Command::Seek(t)) => {
                if source.seek(t).is_err() {
                    continue;
                }
                // Decode up to the target and show it.
                while let Ok(Some(f)) = source.next_frame() {
                    let done = f.seconds + 1e-3 >= t;
                    last = Some(f);
                    if done {
                        break;
                    }
                }
                if let Some(f) = &last {
                    show(f, grade);
                }
                continue;
            }
            None => {}
        }
        // A frame is due.
        let frame = match source.next_frame() {
            Ok(f) => f,
            Err(e) => {
                send(Update::Failed(e));
                return;
            }
        };
        let end = range.end.unwrap_or(f64::INFINITY);
        match frame {
            Some(f) if f.seconds < end => {
                show(&f, grade);
                last = Some(f);
            }
            _ if range.looped => {
                let _ = source.seek(range.start);
            }
            _ => {
                playing = false;
                send(Update::Playing(false));
            }
        }
    }
}

/// Converts RGBA to the BGRA bytes GPUI's images hold.
fn to_image(frame: Frame) -> Option<Arc<RenderImage>> {
    let mut pixels = frame.pixels;
    for px in pixels.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    RgbaImage::from_raw(frame.width, frame.height, pixels)
        .map(|b| Arc::new(RenderImage::new([ImageFrame::new(b)])))
}

/// One preview, owned by a card or the inspector.
pub struct PreviewPlayer {
    commands: futures_channel::mpsc::UnboundedSender<Command>,
    _worker: Task<()>,
    image: Option<Arc<RenderImage>>,
    retired: Vec<Arc<RenderImage>>,
    position: f64,
    duration: Option<f64>,
    playing: bool,
    error: Option<String>,
    frames: u64,
    _updates: Task<()>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    Failed(String),
}

impl EventEmitter<PlayerEvent> for PreviewPlayer {}

impl PreviewPlayer {
    pub fn new(
        backend: Arc<dyn MediaBackend>,
        path: String,
        width: u32,
        autoplay: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let (tx, rx) = futures_channel::mpsc::unbounded();
        let (utx, mut urx) = futures_channel::mpsc::unbounded();
        // Decoding runs on GPUI's background executor, off the UI thread.
        let executor = cx.background_executor().clone();
        let worker = cx
            .background_executor()
            .spawn(worker(executor, backend, path, width, rx, utx, autoplay));
        let updates = cx.spawn(async move |this, cx| {
            use futures_util::StreamExt;
            while let Some(update) = urx.next().await {
                if this.update(cx, |this, cx| this.apply(update, cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            commands: tx,
            _worker: worker,
            image: None,
            retired: Vec::new(),
            position: 0.,
            duration: None,
            playing: false,
            error: None,
            frames: 0,
            _updates: updates,
        }
    }

    fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            Update::Opened { duration } => self.duration = duration,
            Update::Frame { frame } => {
                self.position = frame.seconds;
                if let Some(image) = to_image(frame) {
                    if let Some(old) = self.image.replace(image) {
                        self.retired.push(old);
                    }
                    self.frames += 1;
                }
            }
            Update::Playing(p) => self.playing = p,
            Update::Failed(e) => {
                self.error = Some(e.clone());
                self.playing = false;
                cx.emit(PlayerEvent::Failed(e));
            }
        }
        cx.notify();
    }

    /// Evicts replaced frames from the window's sprite atlas.
    pub fn release_retired(&mut self, window: &mut Window) {
        for old in self.retired.drain(..) {
            let _ = window.drop_image(old);
        }
    }

    pub fn image(&self) -> Option<&Arc<RenderImage>> {
        self.image.as_ref()
    }
    pub fn position(&self) -> f64 {
        self.position
    }
    pub fn duration(&self) -> Option<f64> {
        self.duration
    }
    pub fn playing(&self) -> bool {
        self.playing
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn play(&self) {
        let _ = self.commands.unbounded_send(Command::Play);
    }
    pub fn pause(&self) {
        let _ = self.commands.unbounded_send(Command::Pause);
    }
    pub fn seek(&mut self, seconds: f64) {
        self.position = seconds;
        let _ = self.commands.unbounded_send(Command::Seek(seconds));
    }
    pub fn set_range(&self, range: Range) {
        let _ = self.commands.unbounded_send(Command::Range(range));
    }
    pub fn set_grade(&self, grade: Grade) {
        let _ = self.commands.unbounded_send(Command::Grade(grade));
    }
}

impl Drop for PreviewPlayer {
    fn drop(&mut self) {
        let _ = self.commands.unbounded_send(Command::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grading_matches_css_filters() {
        let mut p = vec![200, 50, 10, 255];
        grade_pixels(&mut p, Grade::default());
        assert_eq!(p, [200, 50, 10, 255]);
        grade_pixels(
            &mut p,
            Grade {
                saturation: 0.,
                ..Default::default()
            },
        );
        assert!(p[0] == p[1] && p[1] == p[2]);
        let mut d = vec![100, 100, 100, 255];
        grade_pixels(
            &mut d,
            Grade {
                brightness: 0.5,
                ..Default::default()
            },
        );
        assert_eq!(d[0], 50);
    }
}
