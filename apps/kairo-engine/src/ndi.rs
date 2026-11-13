//! NDI feeds of the engine (prompt 061): Electron's `ndiService` and its
//! channels, owned by the engine instead of the main process.
//!
//! The runtime is loaded once at launch from the app folder; if it is
//! missing or broken only NDI is disabled. The primary feed is named
//! `NDI_SENDER_NAME` ("Kairo Scripture") so existing receivers still find
//! it, and lives as long as the engine: its surface closing clears it to
//! transparent instead of removing the source. Every other NDI output gets
//! its own sender under the name the client gives it (`Kairo <name>`),
//! removed when the surface closes and restarted when the name changes.

use kairo_audio::AudioBlock;
use kairo_domain::brand::NDI_SENDER_NAME;
use kairo_sinks::ndi::Runtime;
use kairo_sinks::{Make, NdiOutput};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// The reason shown when NDI cannot run on this machine.
pub const UNAVAILABLE: &str = "NDI sender unavailable";

/// A surface's feed.
pub struct Feed {
    pub name: String,
    pub output: Arc<NdiOutput>,
    pub primary: bool,
}

pub struct Ndi {
    make: Option<Arc<dyn Make>>,
    /// Why the runtime did not load.
    pub error: Option<String>,
    primary: Option<Arc<NdiOutput>>,
    /// Feeds by output, for program sound.
    routes: Arc<Mutex<HashMap<String, Arc<NdiOutput>>>>,
}

impl Ndi {
    /// Loads the runtime and starts the primary sender.
    pub fn launch() -> Self {
        match Runtime::find() {
            Ok(runtime) => {
                tracing::info!(
                    version = runtime.version(),
                    path = %runtime.path.display(),
                    "NDI runtime loaded"
                );
                Self::with(Arc::new(runtime))
            }
            Err(error) => {
                tracing::warn!(%error, "NDI unavailable; NDI features disabled");
                Self {
                    make: None,
                    error: Some(error),
                    primary: None,
                    routes: Arc::default(),
                }
            }
        }
    }

    pub fn with(make: Arc<dyn Make>) -> Self {
        let primary = Arc::new(NdiOutput::start(make.clone(), NDI_SENDER_NAME));
        Self {
            make: Some(make),
            error: None,
            primary: Some(primary),
            routes: Arc::default(),
        }
    }

    pub fn available(&self) -> bool {
        self.make.is_some()
    }

    /// The feed for `output` under `name`.
    pub fn open(&self, output: &str, name: &str) -> Result<Feed, String> {
        let Some(make) = self.make.as_ref().filter(|_| self.available()) else {
            tracing::warn!(output, error = ?self.error, "{UNAVAILABLE}");
            return Err(UNAVAILABLE.into());
        };
        let primary = name == NDI_SENDER_NAME;
        let feed = match (&self.primary, primary) {
            (Some(p), true) => p.clone(),
            _ => Arc::new(NdiOutput::start(make.clone(), name)),
        };
        if !primary {
            tracing::info!(output, name, "extra NDI sender added");
        }
        self.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(output.to_owned(), feed.clone());
        Ok(Feed {
            name: name.to_owned(),
            output: feed,
            primary,
        })
    }

    /// `output` no longer sends: the primary goes transparent, others stop.
    pub fn close(&self, output: &str, feed: Feed) {
        self.routes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(output);
        if feed.primary {
            feed.output.clear();
        }
        // An extra feed's sender goes when the last reference does.
    }

    /// Program sound for `output`'s feed (from the NDI tap).
    pub fn audio_route(&self) -> impl Fn(&str, &AudioBlock) + Send + Sync + 'static {
        let routes = self.routes.clone();
        move |output, block| {
            let feed = routes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(output)
                .cloned();
            if let Some(feed) = feed {
                feed.audio(block);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_sinks::Transmit;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Names(Mutex<Vec<String>>);
    struct Silent(Arc<AtomicU32>);
    impl Transmit for Silent {
        fn video(&self, _: u32, _: u32, _: u32, _: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn audio(&self, _: &AudioBlock) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }
    struct Maker(Arc<Names>, Arc<AtomicU32>);
    impl Make for Maker {
        fn make(&self, name: &str) -> Result<Arc<dyn Transmit>, String> {
            self.0.0.lock().unwrap().push(name.to_owned());
            Ok(Arc::new(Silent(self.1.clone())))
        }
    }

    fn wait(f: impl Fn() -> bool) {
        let t = std::time::Instant::now();
        while !f() {
            assert!(t.elapsed() < std::time::Duration::from_secs(5));
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn the_primary_starts_at_launch_and_outlives_its_surface() {
        let names = Arc::new(Names(Mutex::new(Vec::new())));
        let audio = Arc::new(AtomicU32::new(0));
        let ndi = Ndi::with(Arc::new(Maker(names.clone(), audio.clone())));
        wait(|| names.0.lock().unwrap().len() == 1);
        assert_eq!(names.0.lock().unwrap()[0], "Kairo Scripture");
        let primary = ndi.open("ndi-1", "Kairo Scripture").unwrap();
        assert!(primary.primary);
        let extra = ndi.open("ndi-2", "Kairo Stage").unwrap();
        wait(|| names.0.lock().unwrap().len() == 2);
        assert_eq!(names.0.lock().unwrap()[1], "Kairo Stage");
        // Sound reaches each output's own feed.
        let route = ndi.audio_route();
        wait(|| extra.output.status().sending);
        route(
            "ndi-2",
            &AudioBlock {
                sample_rate: 48_000.0,
                channels: 2,
                samples: 1024,
                data: vec![0.0; 2048],
            },
        );
        route(
            "nobody",
            &AudioBlock {
                sample_rate: 48_000.0,
                channels: 2,
                samples: 1024,
                data: vec![0.0; 2048],
            },
        );
        assert_eq!(audio.load(Ordering::Relaxed), 1);
        let primary_feed = primary.output.clone();
        ndi.close("ndi-1", primary);
        assert!(
            primary_feed.status().sending,
            "the primary stays on the network"
        );
        let extra_feed = Arc::downgrade(&extra.output);
        ndi.close("ndi-2", extra);
        assert!(extra_feed.upgrade().is_none(), "an extra sender is removed");
        // A rename is a new sender under the new name.
        let renamed = ndi.open("ndi-2", "Kairo Lobby").unwrap();
        wait(|| names.0.lock().unwrap().len() == 3);
        assert_eq!(names.0.lock().unwrap()[2], "Kairo Lobby");
        drop(renamed);
    }

    #[test]
    fn no_runtime_means_unavailable_with_the_existing_reason() {
        let ndi = Ndi {
            make: None,
            error: Some("no libndi.dylib in the app folder".into()),
            primary: None,
            routes: Arc::default(),
        };
        assert!(!ndi.available());
        assert_eq!(
            ndi.open("ndi-1", "Kairo Scripture").err().as_deref(),
            Some(UNAVAILABLE)
        );
    }
}
