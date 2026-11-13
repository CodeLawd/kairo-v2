//! The operator preview's client side (prompt 066).
//!
//! A view that shows an output's live picture holds a
//! [`PreviewSubscription`]. While at least one is held for an output, the
//! engine publishes that output's frames into a private memory-mapped file
//! ([`PreviewMap`]); when the last one is dropped the engine is told to
//! stop and the file is removed. Nothing is drawn, copied or sent for an
//! output nobody is looking at.
//!
//! Several views of one output share one map; the engine draws at the
//! largest size any of them asked for and the fastest rate. The request is
//! part of the surface's state, so the supervisor replays it into a
//! relaunched engine; a surface that was destroyed and opened again lost
//! it, which [`PreviewSubscription::ensure`] notices and repairs.

use crate::engine::OutputEngine;
use kairo_engine_proto::preview::{PreviewMap, SLOTS};
use kairo_engine_proto::{Command, PreviewTarget};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The largest preview a map holds. Operator monitors are well under this;
/// it bounds each map to `SLOTS` frames of 1280 × 720 × 4 bytes.
pub const CAPACITY: (u32, u32) = (1280, 720);
/// The rate the live monitor asks for (architecture section 6: "a capped rate").
pub const DEFAULT_FPS: u32 = 30;
/// How often a held subscription checks that the engine still has it.
const ENSURE_EVERY: Duration = Duration::from_secs(1);

/// Bytes of frame memory one map can hold, for budgeting.
pub const MAP_BYTES: usize = CAPACITY.0 as usize * CAPACITY.1 as usize * 4 * SLOTS;

struct Holder {
    size: (u32, u32),
    fps: u32,
}

struct Entry {
    map: Arc<PreviewMap>,
    path: String,
    holders: HashMap<u64, Holder>,
    /// What the engine was last asked for.
    sent: Option<PreviewTarget>,
}

impl Entry {
    fn wanted(&self) -> PreviewTarget {
        let width = self.holders.values().map(|h| h.size.0).max().unwrap_or(1);
        let height = self.holders.values().map(|h| h.size.1).max().unwrap_or(1);
        let fps = self.holders.values().map(|h| h.fps).max().unwrap_or(1);
        PreviewTarget {
            path: self.path.clone(),
            width: width.clamp(4, CAPACITY.0),
            height: height.clamp(1, CAPACITY.1),
            fps: fps.clamp(1, 60),
        }
    }
}

struct Shared {
    engine: Arc<dyn OutputEngine>,
    dir: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
    next: AtomicU64,
}

impl Shared {
    /// Tells the engine what `output` should get now, if that changed.
    fn send(&self, output: &str, entry: &mut Entry, force: bool) {
        let wanted = entry.wanted();
        if !force && entry.sent.as_ref() == Some(&wanted) {
            return;
        }
        self.engine.command(Command::SetPreview {
            output: output.to_owned(),
            preview: Some(wanted.clone()),
        });
        entry.sent = Some(wanted);
    }

    fn release(&self, output: &str, id: u64) {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = entries.get_mut(output) else {
            return;
        };
        entry.holders.remove(&id);
        if entry.holders.is_empty() {
            // The engine stops first, then the file goes with the map.
            self.engine.command(Command::SetPreview {
                output: output.to_owned(),
                preview: None,
            });
            entries.remove(output);
        } else {
            self.send(output, entry, false);
        }
    }
}

/// Hands out preview subscriptions. One per UI process.
#[derive(Clone)]
pub struct PreviewHub {
    shared: Arc<Shared>,
}

impl PreviewHub {
    /// Maps go in a new owner-only folder under the system temp folder.
    pub fn new(engine: Arc<dyn OutputEngine>) -> std::io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "kairo-preview-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir)?;
        Ok(Self {
            shared: Arc::new(Shared {
                engine,
                dir,
                entries: Mutex::new(HashMap::new()),
                next: AtomicU64::new(1),
            }),
        })
    }

    /// Starts (or joins) `output`'s preview at up to `size` pixels and `fps`.
    pub fn subscribe(
        &self,
        output: &str,
        size: (u32, u32),
        fps: u32,
    ) -> std::io::Result<PreviewSubscription> {
        let shared = &self.shared;
        let id = shared.next.fetch_add(1, Ordering::Relaxed);
        let mut entries = shared.entries.lock().unwrap_or_else(|e| e.into_inner());
        if !entries.contains_key(output) {
            let path = shared.dir.join(format!("{id}.map"));
            let map = PreviewMap::create(&path, CAPACITY.0, CAPACITY.1)?;
            entries.insert(
                output.to_owned(),
                Entry {
                    map: Arc::new(map),
                    path: path.to_string_lossy().into_owned(),
                    holders: HashMap::new(),
                    sent: None,
                },
            );
        }
        let entry = entries.get_mut(output).expect("inserted");
        entry.holders.insert(id, Holder { size, fps });
        let map = entry.map.clone();
        shared.send(output, entry, false);
        Ok(PreviewSubscription {
            shared: shared.clone(),
            output: output.to_owned(),
            id,
            map,
            checked: Instant::now(),
        })
    }

    /// Outputs with a preview running, for diagnostics and tests.
    pub fn active(&self) -> Vec<String> {
        let entries = self
            .shared
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut outputs: Vec<String> = entries.keys().cloned().collect();
        outputs.sort();
        outputs
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        let entries = std::mem::take(self.entries.get_mut().unwrap_or_else(|e| e.into_inner()));
        drop(entries);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// One view's hold on an output's preview. Dropping it lets go.
pub struct PreviewSubscription {
    shared: Arc<Shared>,
    output: String,
    id: u64,
    map: Arc<PreviewMap>,
    checked: Instant,
}

impl PreviewSubscription {
    pub fn output(&self) -> &str {
        &self.output
    }

    /// The newest frame published after frame number `seen`, into `buffer`
    /// (straight-alpha BGRA). Returns its number, width and height.
    pub fn read_into(&self, seen: u64, buffer: &mut Vec<u8>) -> Option<(u64, u32, u32)> {
        self.map.read_into(seen, buffer)
    }

    /// Frames the engine has published into this map.
    pub fn published(&self) -> u64 {
        self.map.published()
    }

    /// The view changed size or rate.
    pub fn update(&mut self, size: (u32, u32), fps: u32) {
        let mut entries = self
            .shared
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = entries.get_mut(&self.output) {
            entry.holders.insert(self.id, Holder { size, fps });
            self.shared.send(&self.output, entry, false);
        }
    }

    /// Called while the view is shown. About once a second, checks that
    /// the engine's surface still carries this preview (a destroyed and
    /// reopened surface does not) and asks again if not. A surface that
    /// does not exist yet is left alone: it gets the request when it opens.
    pub fn ensure(&mut self) {
        if self.checked.elapsed() < ENSURE_EVERY {
            return;
        }
        self.checked = Instant::now();
        let Some(state) = self.shared.engine.surface_state(&self.output) else {
            return;
        };
        let mut entries = self
            .shared
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = entries.get_mut(&self.output)
            && state.preview != Some(entry.wanted())
        {
            self.shared.send(&self.output, entry, true);
        }
    }
}

impl Drop for PreviewSubscription {
    fn drop(&mut self) {
        self.shared.release(&self.output, self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{EngineEvent, EngineFuture, Sent};
    use kairo_engine_proto::*;
    use tokio::sync::broadcast;

    /// Records commands and keeps surface states the way the client does.
    struct Fake {
        commands: Mutex<Vec<Command>>,
        states: Mutex<HashMap<String, SurfaceState>>,
        events: broadcast::Sender<EngineEvent>,
    }

    impl Fake {
        fn new(outputs: &[&str]) -> Arc<Self> {
            let states = outputs
                .iter()
                .map(|o| {
                    (
                        (*o).to_owned(),
                        SurfaceState::new(SurfaceSpec {
                            output: (*o).into(),
                            label: (*o).into(),
                            sink: SinkSpec::Ndi { name: (*o).into() },
                        }),
                    )
                })
                .collect();
            Arc::new(Self {
                commands: Mutex::default(),
                states: Mutex::new(states),
                events: broadcast::channel(8).0,
            })
        }

        fn previews(&self) -> Vec<(String, Option<PreviewTarget>)> {
            self.commands
                .lock()
                .unwrap()
                .iter()
                .filter_map(|c| match c {
                    Command::SetPreview { output, preview } => {
                        Some((output.clone(), preview.clone()))
                    }
                    _ => None,
                })
                .collect()
        }
    }

    impl OutputEngine for Fake {
        fn command(&self, command: Command) -> Sent {
            if let Some(output) = command.output()
                && let Some(state) = self.states.lock().unwrap().get_mut(output)
            {
                state.apply(&command, true);
            }
            self.commands.lock().unwrap().push(command);
            Sent {
                accepted: true,
                seq: 0,
            }
        }
        fn is_available(&self, _: &str) -> bool {
            true
        }
        fn unavailable_reason(&self, _: &str) -> Option<String> {
            None
        }
        fn surface_state(&self, output: &str) -> Option<SurfaceState> {
            self.states.lock().unwrap().get(output).cloned()
        }
        fn surface_status(&self, _: &str) -> Option<SurfaceStatus> {
            None
        }
        fn surfaces(&self) -> Vec<String> {
            self.states.lock().unwrap().keys().cloned().collect()
        }
        fn displays(&self) -> Vec<DisplayInfo> {
            Vec::new()
        }
        fn running(&self) -> bool {
            true
        }
        fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
            self.events.subscribe()
        }
        fn query(&self, _: Query) -> EngineFuture<'_, Option<Reply>> {
            Box::pin(async { None })
        }
        fn wait_frame<'a>(&'a self, _: &'a str, _: u64, _: Duration) -> EngineFuture<'a, bool> {
            Box::pin(async { false })
        }
        fn identify_displays(&self) -> bool {
            false
        }
        fn show_display_test_pattern(&self, _: &kairo_domain::overlay::TestPatternInput) -> bool {
            false
        }
    }

    #[test]
    fn a_preview_runs_only_while_a_view_holds_it() {
        let fake = Fake::new(&["main"]);
        let hub = PreviewHub::new(fake.clone()).unwrap();
        assert!(fake.previews().is_empty(), "nothing until a view asks");
        let sub = hub.subscribe("main", (640, 360), 30).unwrap();
        let sent = fake.previews();
        assert_eq!(sent.len(), 1);
        let target = sent[0].1.clone().unwrap();
        assert_eq!((target.width, target.height, target.fps), (640, 360, 30));
        let path = PathBuf::from(&target.path);
        assert!(path.exists());
        // The engine side can open what the hub made.
        let engine_side = PreviewMap::open(&path).unwrap();
        engine_side.publish(4, 1, &[9; 16]).unwrap();
        let mut buffer = Vec::new();
        assert_eq!(sub.read_into(0, &mut buffer), Some((1, 4, 1)));
        assert_eq!(buffer, vec![9; 16]);
        drop(engine_side);

        drop(sub);
        assert_eq!(fake.previews().last().unwrap(), &("main".into(), None));
        assert!(!path.exists(), "the map goes with the last view");
        assert!(hub.active().is_empty());
    }

    #[test]
    fn views_of_one_output_share_a_map_at_the_largest_size() {
        let fake = Fake::new(&["main", "ndi"]);
        let hub = PreviewHub::new(fake.clone()).unwrap();
        let small = hub.subscribe("main", (320, 180), 15).unwrap();
        let mut large = hub.subscribe("main", (640, 360), 30).unwrap();
        let other = hub.subscribe("ndi", (320, 180), 30).unwrap();
        let sent = fake.previews();
        assert_eq!(sent.len(), 3);
        assert_eq!(
            sent[0].1.as_ref().unwrap().path,
            sent[1].1.as_ref().unwrap().path
        );
        assert_ne!(
            sent[0].1.as_ref().unwrap().path,
            sent[2].1.as_ref().unwrap().path
        );
        assert_eq!(sent[1].1.as_ref().unwrap().width, 640);
        // An unchanged size sends nothing; a larger one is sent; the size
        // is capped at the map's capacity.
        large.update((640, 360), 30);
        assert_eq!(fake.previews().len(), 3);
        large.update((4000, 4000), 90);
        let t = fake.previews().last().unwrap().1.clone().unwrap();
        assert_eq!((t.width, t.height, t.fps), (CAPACITY.0, CAPACITY.1, 60));
        // The large view goes: back to the small one's size and rate.
        drop(large);
        let t = fake.previews().last().unwrap().1.clone().unwrap();
        assert_eq!((t.width, t.height, t.fps), (320, 180, 15));
        assert_eq!(hub.active(), ["main", "ndi"]);
        drop(small);
        drop(other);
        assert!(hub.active().is_empty());
    }

    #[test]
    fn a_reopened_surface_gets_its_preview_back() {
        let fake = Fake::new(&["main"]);
        let hub = PreviewHub::new(fake.clone()).unwrap();
        let mut sub = hub.subscribe("main", (640, 360), 30).unwrap();
        // Within a second, or with the state intact, nothing is resent.
        sub.ensure();
        sub.checked -= ENSURE_EVERY;
        sub.ensure();
        assert_eq!(fake.previews().len(), 1);
        // The surface is destroyed and opened again without the preview.
        fake.states.lock().unwrap().get_mut("main").unwrap().preview = None;
        sub.checked -= ENSURE_EVERY;
        sub.ensure();
        assert_eq!(fake.previews().len(), 2);
        assert!(fake.surface_state("main").unwrap().preview.is_some());
        // An output with no surface yet is left alone.
        let mut absent = hub.subscribe("later", (64, 36), 30).unwrap();
        absent.checked -= ENSURE_EVERY;
        absent.ensure();
        assert_eq!(fake.previews().len(), 3);
    }

    #[test]
    fn the_hub_folder_is_private_and_removed_with_the_hub() {
        let fake = Fake::new(&["main"]);
        let hub = PreviewHub::new(fake.clone()).unwrap();
        let dir = hub.shared.dir.clone();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }
        let sub = hub.subscribe("main", (64, 36), 30).unwrap();
        drop(hub);
        // A view still holding on keeps the folder until it lets go.
        assert!(dir.exists());
        drop(sub);
        assert!(!dir.exists());
    }
}
