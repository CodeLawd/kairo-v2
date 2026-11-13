//! A lobby playlist (`lobby-player.ts`).
//!
//! Drives a `screen` or `device` output whose source is `playlist`: a foyer
//! or lobby display that cycles a media-dock playlist on its own and never
//! takes service pushes. Images hold for `slideSec`; videos play once, to
//! their end. The output's transition (cut or fade) applies between items,
//! as on the program.

use crate::ports::LobbyMedia;
use kairo_core::engine::OutputEngine;
use kairo_domain::media::{MediaKind, theme_with_live_media};
use kairo_domain::overlay::OverlayOutput;
use kairo_engine_proto::{Command, Json, Query, Reply, SlideContent, VideoTransport};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// How often a playing video is checked for its end.
pub const VIDEO_POLL: Duration = Duration::from_millis(500);
/// A new video element gets this long to load before the first check.
pub const VIDEO_GRACE: Duration = Duration::from_secs(1);
/// A video that never reports an end (a broken file, a live stream) still moves on.
pub const MAX_VIDEO: Duration = Duration::from_secs(30 * 60);
/// Retry interval while the playlist is missing, empty or not shown.
pub const IDLE_RETRY: Duration = Duration::from_secs(5);

enum Control {
    Start,
    Stop,
    Update(Box<OverlayOutput>),
}

/// One output's playlist. Dropping it stops the player.
pub struct LobbyPlayer {
    control: mpsc::UnboundedSender<Control>,
    task: tokio::task::JoinHandle<()>,
}

impl LobbyPlayer {
    /// A stopped player for `output`. Needs a Tokio runtime.
    pub fn new(
        engine: Arc<dyn OutputEngine>,
        media: Arc<dyn LobbyMedia>,
        output: OverlayOutput,
    ) -> Self {
        let (control, receiver) = mpsc::unbounded_channel();
        let task = tokio::spawn(
            Loop {
                engine,
                media,
                output,
                running: false,
                index: 0,
                current_item: None,
            }
            .run(receiver),
        );
        Self { control, task }
    }

    /// Starts from the current item; does nothing while already running.
    pub fn start(&self) {
        let _ = self.control.send(Control::Start);
    }

    pub fn stop(&self) {
        let _ = self.control.send(Control::Stop);
    }

    /// Picks up a new playlist or timing; restarts from the top when the
    /// playlist changed.
    pub fn update(&self, output: OverlayOutput) {
        let _ = self.control.send(Control::Update(Box::new(output)));
    }
}

impl Drop for LobbyPlayer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone, Copy)]
enum Next {
    Step,
    /// Ask whether the video that went up at `started` has ended.
    Poll {
        started: Instant,
    },
}

struct Loop {
    engine: Arc<dyn OutputEngine>,
    media: Arc<dyn LobbyMedia>,
    output: OverlayOutput,
    running: bool,
    index: usize,
    /// Item on screen now: a repeat of the same video rewinds instead of repainting.
    current_item: Option<String>,
}

impl Loop {
    async fn run(mut self, mut control: mpsc::UnboundedReceiver<Control>) {
        let mut next: Option<(Instant, Next)> = None;
        loop {
            let deadline = next.map(|(at, _)| at);
            tokio::select! {
                message = control.recv() => match message {
                    None => return,
                    Some(Control::Start) => {
                        if !self.running {
                            self.running = true;
                            next = Some((Instant::now(), Next::Step));
                        }
                    }
                    Some(Control::Stop) => {
                        self.running = false;
                        next = None;
                    }
                    Some(Control::Update(output)) => {
                        let changed = output.playlist_id != self.output.playlist_id;
                        self.output = *output;
                        if changed {
                            self.index = 0;
                            if self.running {
                                next = Some((Instant::now(), Next::Step));
                            }
                        }
                    }
                },
                _ = sleep_until(deadline), if deadline.is_some() => {
                    let (_, what) = next.take().expect("deadline set");
                    next = Some(match what {
                        Next::Step => self.step().await,
                        Next::Poll { started } => self.poll(started).await,
                    });
                }
            }
        }
    }

    /// Shows the next playable item and says when to look again.
    async fn step(&mut self) -> (Instant, Next) {
        let items = self.media.playlist_items(&self.output.playlist_id);
        if items.is_empty() {
            return after(IDLE_RETRY, Next::Step);
        }
        if self.index >= items.len() {
            self.index = 0;
        }
        let item = &items[self.index];
        self.index = (self.index + 1) % items.len();

        // A text-free frame with the item as its background. A video must
        // not loop here, or the playlist would never advance past it.
        let video = item.kind == MediaKind::Video;
        let mut theme = theme_with_live_media(
            self.output.theme.clone(),
            Some((&item.path, item.kind)),
            &self.media.playback(&item.id),
            None,
        );
        if video {
            theme.background.media_loop = Some(false);
        }
        // The program keeps a background whose file did not change, so a
        // one-video playlist would otherwise sit on its finished last frame.
        let repeat_video = video && self.current_item.as_deref() == Some(item.id.as_str());
        let output = self.output.id.clone();
        let shown = if repeat_video {
            for transport in [VideoTransport::Seek(0.), VideoTransport::Pause(false)] {
                self.engine.command(Command::VideoTransport {
                    output: output.clone(),
                    transport,
                });
            }
            true
        } else {
            self.engine
                .command(Command::ShowSlide {
                    output: output.clone(),
                    source: output.clone(),
                    slide: SlideContent {
                        reference: String::new(),
                        text: String::new(),
                        theme: Json::encode(&theme),
                        colored_lines: None,
                    },
                    instant: false,
                })
                .accepted
        };
        self.current_item = Some(item.id.clone());
        if !shown {
            return after(IDLE_RETRY, Next::Step);
        }
        if !video {
            let hold = Duration::try_from_secs_f64(self.output.slide_sec.max(0.))
                .unwrap_or(Duration::ZERO);
            return after(hold, Next::Step);
        }
        after(
            VIDEO_GRACE,
            Next::Poll {
                started: Instant::now(),
            },
        )
    }

    async fn poll(&mut self, started: Instant) -> (Instant, Next) {
        let ended = match self
            .engine
            .query(Query::VideoEnded {
                output: self.output.id.clone(),
            })
            .await
        {
            Some(Reply::VideoEnded(ended)) => ended,
            // No surface, or no answer: `videoEnded` resolved true.
            _ => true,
        };
        if ended || started.elapsed() > MAX_VIDEO {
            return self.step().await;
        }
        after(VIDEO_POLL, Next::Poll { started })
    }
}

fn after(wait: Duration, next: Next) -> (Instant, Next) {
    (Instant::now() + wait, next)
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}
