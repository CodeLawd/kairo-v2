//! Video playback for the engine (prompt 055).
//!
//! The compositor reports each output's video nodes ([`MediaUse`]); this
//! crate keeps one player per source id. A background's id is a hash of
//! `VIDEO:file:cue`, so outputs that show the same file at the same cue
//! share one decoder and show the same frame, while a new cue is a new id
//! and starts from the beginning (`mediaCue`). A node is an `<video>`
//! element: a new node for an id the output already shows is a new element
//! and restarts the video, while a node kept with new settings (text
//! changed, a filter slider moved) keeps playing and only takes the loop.

pub mod decode;
pub mod player;
pub mod transport;

pub use decode::Backend;
pub use kairo_compositor::MediaUse;
pub use player::{Control, Feed, Player, Request};
pub use transport::{Status, Timing};

use player::{OnError, Owners};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

struct Entry {
    player: Player,
    looped: bool,
}

/// Every playing video, by source id.
pub struct Videos {
    feed: Arc<dyn Feed>,
    on_error: OnError,
    owners: Arc<Owners>,
    players: HashMap<u64, Entry>,
    /// Each output's video nodes as last reported.
    outputs: HashMap<String, Vec<MediaUse>>,
    next_generation: u64,
    hardware: bool,
}

impl Videos {
    /// `on_error(id, path, message)` runs on the player's thread, once per
    /// player that fails.
    pub fn new(
        feed: Arc<dyn Feed>,
        on_error: impl Fn(u64, &str, &str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            feed,
            on_error: Arc::new(on_error),
            owners: Arc::new(Owners::default()),
            players: HashMap::new(),
            outputs: HashMap::new(),
            next_generation: 0,
            hardware: std::env::var_os("KAIRO_VIDEO_SOFTWARE").is_none(),
        }
    }

    /// Whether players try hardware decode first (on unless
    /// `KAIRO_VIDEO_SOFTWARE` is set).
    pub fn set_hardware(&mut self, hardware: bool) {
        self.hardware = hardware;
    }

    fn start(&mut self, u: &MediaUse) {
        self.next_generation += 1;
        let generation = self.next_generation;
        self.owners.claim(u.id, generation);
        let request = Request {
            path: PathBuf::from(&*u.params.path),
            timing: Timing {
                start: u.params.start_seconds.filter(|s| s.is_finite() && *s > 0.0),
                end: u.params.end_seconds.filter(|e| e.is_finite() && *e > 0.0),
                rate: if u.params.rate.is_finite() && u.params.rate > 0.0 {
                    u.params.rate
                } else {
                    1.0
                },
            },
            looped: u.params.looped,
            hardware: self.hardware,
        };
        let player = Player::start(
            u.id,
            generation,
            request,
            self.feed.clone(),
            self.owners.clone(),
            self.on_error.clone(),
        );
        // A replaced player stops; its texture now belongs to this one.
        if let Some(old) = self.players.insert(
            u.id,
            Entry {
                player,
                looped: u.params.looped,
            },
        ) {
            old.player.stop();
        }
    }

    /// The compositor's report of `output`'s video nodes.
    pub fn sync(&mut self, output: &str, uses: Vec<MediaUse>) {
        let previous = self.outputs.remove(output).unwrap_or_default();
        let mut restarted = HashSet::new();
        for u in &uses {
            let known = previous.iter().any(|p| p.node == u.node);
            if !known {
                // A new element. On an output that already showed this id
                // through another node, it starts again; elsewhere it joins
                // the one playing.
                let again = previous.iter().any(|p| p.id == u.id && p.node != u.node);
                if (again || !self.players.contains_key(&u.id)) && restarted.insert(u.id) {
                    self.start(u);
                    continue;
                }
            }
            if let Some(entry) = self.players.get_mut(&u.id)
                && entry.looped != u.params.looped
                && !restarted.contains(&u.id)
            {
                entry.looped = u.params.looped;
                entry.player.control(Control::Loop(u.params.looped));
            }
        }
        if !uses.is_empty() {
            self.outputs.insert(output.to_owned(), uses);
        }
        let live: HashSet<u64> = self.outputs.values().flatten().map(|u| u.id).collect();
        let gone: Vec<u64> = self
            .players
            .keys()
            .copied()
            .filter(|id| !live.contains(id))
            .collect();
        for id in gone {
            if let Some(entry) = self.players.remove(&id) {
                entry.player.stop();
            }
        }
    }

    /// `__setVideoPaused` / `__seekVideo` on source `id`, the background of
    /// the output it was sent to.
    pub fn control(&self, id: u64, control: Control) {
        if let Some(entry) = self.players.get(&id) {
            entry.player.control(control);
        }
    }

    /// The output's current background video, when it has one.
    pub fn background(&self, output: &str) -> Option<u64> {
        self.outputs
            .get(output)?
            .iter()
            .find(|u| u.transport)
            .map(|u| u.id)
    }

    /// `__videoEnded`: true with no background video, and when it ended,
    /// failed or is paused at its end.
    pub fn ended(&self, output: &str) -> bool {
        match self.background(output).and_then(|id| self.players.get(&id)) {
            None => true,
            Some(entry) => entry.player.status().video_ended(),
        }
    }

    pub fn status(&self, id: u64) -> Option<Status> {
        self.players.get(&id).map(|e| e.player.status())
    }

    pub fn player(&self, id: u64) -> Option<&Player> {
        self.players.get(&id).map(|e| &e.player)
    }

    /// How many streams are playing.
    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// Stops every player and waits for each thread.
    pub fn shutdown(&mut self) {
        self.outputs.clear();
        for (_, entry) in self.players.drain() {
            entry.player.join();
        }
    }
}

impl Drop for Videos {
    fn drop(&mut self) {
        for (_, entry) in self.players.drain() {
            entry.player.stop();
        }
    }
}
