//! Rasterising on worker threads, cached by content hash.
//!
//! A slide's text changes only when the slide does, so it is drawn once, off
//! the threads that present frames, and the pixels are reused until the
//! scene changes. The engine and the UI process (thumbnails) share this.

use crate::raster::{TextRaster, rasterize};
use crate::scene::SlideScene;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// Builds the scene on a worker: parsing and normalising a theme is not
/// work for the caller's thread. `None` means the scene could not be built.
pub type Job = Box<dyn FnOnce() -> Option<SlideScene> + Send>;

/// How a finished job went.
pub struct Done<T> {
    /// The converted raster, or `None` when the job's scene failed to build.
    pub value: Option<T>,
    pub hash: u64,
    pub cached: bool,
    /// Time spent building the scene and drawing it, in milliseconds.
    pub build_ms: f64,
    pub raster_ms: f64,
}

type Callback<T> = Box<dyn FnOnce(Done<T>) + Send>;

struct Task<T> {
    job: Job,
    done: Callback<T>,
}

struct Entry<T> {
    hash: u64,
    value: T,
    bytes: usize,
}

struct Shared<T> {
    queue: Mutex<VecDeque<Task<T>>>,
    wake: Condvar,
    stop: AtomicBool,
    cache: Mutex<VecDeque<Entry<T>>>,
    budget: usize,
    /// Turns drawn pixels (or nothing) into the caller's type.
    convert: Box<dyn Fn(Option<TextRaster>) -> T + Send + Sync>,
}

pub struct RasterPool<T> {
    shared: Arc<Shared<T>>,
    workers: Vec<JoinHandle<()>>,
}

impl<T: Clone + Send + 'static> RasterPool<T> {
    /// `workers` threads, a cache of at most `budget_bytes` of pixels, and
    /// `convert` to make the value cached and handed back.
    pub fn new(
        workers: usize,
        budget_bytes: usize,
        convert: impl Fn(Option<TextRaster>) -> T + Send + Sync + 'static,
    ) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            cache: Mutex::new(VecDeque::new()),
            budget: budget_bytes,
            convert: Box::new(convert),
        });
        let workers = (0..workers.max(1))
            .map(|i| {
                let shared = shared.clone();
                std::thread::Builder::new()
                    .name(format!("scene-raster-{i}"))
                    .spawn(move || worker(&shared))
                    .expect("spawn scene raster worker")
            })
            .collect();
        Self { shared, workers }
    }

    /// Queue a scene to be built and drawn; `done` runs on the worker.
    pub fn submit(&self, job: Job, done: impl FnOnce(Done<T>) + Send + 'static) {
        self.shared
            .queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(Task {
                job,
                done: Box::new(done),
            });
        self.shared.wake.notify_one();
    }

    /// Draw on the calling thread, through the same cache. For a thumbnail
    /// that is wanted now.
    pub fn render(&self, scene: &SlideScene) -> (T, Done<()>) {
        let (value, timing) = render(&self.shared, scene);
        (value, timing)
    }

    /// Entries and bytes held by the cache.
    pub fn cached(&self) -> (usize, usize) {
        let cache = self.shared.cache.lock().unwrap_or_else(|e| e.into_inner());
        (cache.len(), cache.iter().map(|e| e.bytes).sum())
    }
}

impl<T> Drop for RasterPool<T> {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker<T: Clone + Send + 'static>(shared: &Shared<T>) {
    loop {
        let task = {
            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if shared.stop.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(task) = queue.pop_front() {
                    break task;
                }
                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
            }
        };
        let started = Instant::now();
        // A panic in shaping must not take the worker, or every later slide
        // would wait for it.
        let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task.job))
            .unwrap_or_else(|_| {
                tracing::error!("building a slide scene panicked");
                None
            });
        let build_ms = ms(started);
        let done = match built {
            None => Done {
                value: None,
                hash: 0,
                cached: false,
                build_ms,
                raster_ms: 0.0,
            },
            Some(scene) => {
                let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render(shared, &scene)
                }));
                match drawn {
                    Ok((value, t)) => Done {
                        value: Some(value),
                        hash: t.hash,
                        cached: t.cached,
                        build_ms,
                        raster_ms: t.raster_ms,
                    },
                    Err(_) => {
                        tracing::error!("drawing a slide scene panicked");
                        Done {
                            value: None,
                            hash: 0,
                            cached: false,
                            build_ms,
                            raster_ms: 0.0,
                        }
                    }
                }
            }
        };
        (task.done)(done);
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn render<T: Clone>(shared: &Shared<T>, scene: &SlideScene) -> (T, Done<()>) {
    let hash = scene.content_hash();
    {
        let mut cache = shared.cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = cache.iter().position(|e| e.hash == hash) {
            let entry = cache.remove(i).expect("position is in range");
            let value = entry.value.clone();
            cache.push_back(entry);
            return (
                value,
                Done {
                    value: Some(()),
                    hash,
                    cached: true,
                    build_ms: 0.0,
                    raster_ms: 0.0,
                },
            );
        }
    }
    let started = Instant::now();
    let raster = rasterize(scene);
    let raster_ms = ms(started);
    let bytes = raster.as_ref().map_or(0, |r| r.pixels.len());
    let value = (shared.convert)(raster);
    let mut cache = shared.cache.lock().unwrap_or_else(|e| e.into_inner());
    cache.push_back(Entry {
        hash,
        value: value.clone(),
        bytes,
    });
    // Oldest out first, but the newest always stays.
    while cache.len() > 1 && cache.iter().map(|e| e.bytes).sum::<usize>() > shared.budget {
        cache.pop_front();
    }
    (
        value,
        Done {
            value: Some(()),
            hash,
            cached: false,
            build_ms: 0.0,
            raster_ms,
        },
    )
}
