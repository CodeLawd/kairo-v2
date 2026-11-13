//! The live preview view and the thumbnail service in a GPUI test window.

use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions,
    prelude::*, px, size,
};
use kairo_core::engine::{EngineEvent, EngineFuture, OutputEngine, Sent};
use kairo_core::preview::PreviewHub;
use kairo_core_gpui::preview::LivePreview;
use kairo_core_gpui::thumbnails::{Thumbnail, ThumbnailRequest, Thumbnails};
use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;
use kairo_engine_proto::preview::PreviewMap;
use kairo_engine_proto::*;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

struct Fake {
    commands: Mutex<Vec<Command>>,
    events: broadcast::Sender<EngineEvent>,
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            commands: Mutex::default(),
            events: broadcast::channel(4).0,
        })
    }
    fn last_preview(&self) -> Option<Option<PreviewTarget>> {
        self.commands
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find_map(|c| match c {
                Command::SetPreview { preview, .. } => Some(preview.clone()),
                _ => None,
            })
    }
}

impl OutputEngine for Fake {
    fn command(&self, command: Command) -> Sent {
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
    fn surface_state(&self, _: &str) -> Option<SurfaceState> {
        None
    }
    fn surface_status(&self, _: &str) -> Option<SurfaceStatus> {
        None
    }
    fn surfaces(&self) -> Vec<String> {
        Vec::new()
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

fn open(cx: &mut TestAppContext, hub: PreviewHub) -> (AnyWindowHandle, Entity<LivePreview>) {
    cx.update(gpui_kit::init);
    let (handle, view) = cx.update(|cx| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                Default::default(),
                size(px(640.), px(360.)),
            ))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| cx.new(|cx| LivePreview::new(hub, cx)))
            .expect("window")
    });
    (handle, view)
}

fn frame(cx: &mut TestAppContext, handle: AnyWindowHandle) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
fn the_preview_shows_new_frames_evicts_old_ones_and_lets_go_when_hidden(cx: &mut TestAppContext) {
    let fake = Fake::new();
    let hub = PreviewHub::new(fake.clone()).unwrap();
    let (handle, view) = open(cx, hub.clone());
    frame(cx, handle);
    assert!(fake.last_preview().is_none(), "no output, no preview");

    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.set_output(Some("main".into()), window, cx))
    })
    .unwrap();
    frame(cx, handle);
    let target = fake.last_preview().unwrap().expect("subscribed when drawn");
    assert_eq!(target.fps, 30);
    let engine = PreviewMap::open(std::path::Path::new(&target.path)).unwrap();

    // Ten frames from the engine, each drawn: every replaced image is
    // evicted one frame later, so at most two are ever held.
    for n in 0..10u8 {
        engine.publish(8, 4, &[n; 8 * 4 * 4]).unwrap();
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        frame(cx, handle);
    }
    let stats = cx.update(|cx| view.read(cx).stats());
    assert_eq!(stats.frames_shown, 10);
    assert_eq!(stats.images_dropped, 8);
    assert_eq!(hub.active(), ["main"]);

    // Not painted for over a second (another page is showing): it lets go,
    // the engine is told to stop and the held images are evicted.
    std::thread::sleep(Duration::from_millis(1100));
    cx.executor().advance_clock(Duration::from_millis(50));
    cx.run_until_parked();
    assert_eq!(fake.last_preview(), Some(None));
    assert!(hub.active().is_empty());
    let stats = cx.update(|cx| view.read(cx).stats());
    assert_eq!(stats.images_dropped, 10);
    assert_eq!((stats.subscribes, stats.unsubscribes), (1, 1));
    drop(engine);

    // Shown again: it takes the preview up again.
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |_, cx| cx.notify());
        window.refresh();
    })
    .unwrap();
    frame(cx, handle);
    assert!(matches!(fake.last_preview(), Some(Some(_))));
    assert_eq!(hub.active(), ["main"]);
}

#[gpui_kit::test]
fn thumbnails_are_drawn_off_thread_shared_and_evicted(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let thumbs = cx.update(|cx| cx.new(|cx| Thumbnails::new((160, 90), 2, 2, cx)));
    let theme = Arc::new(DEFAULT_OVERLAY_THEME.clone());
    let request = |text: &str| ThumbnailRequest::new(theme.clone(), "John 3:16", text, Vec::new());
    let a = request("For God so loved the world");
    let wait = |cx: &mut TestAppContext, r: &ThumbnailRequest| {
        let started = Instant::now();
        loop {
            cx.run_until_parked();
            if let Thumbnail::Ready(image) = thumbs.update(cx, |t, _| t.get(r)) {
                return image;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "never drawn");
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    // Pending at first: the card shows its placeholder.
    assert!(matches!(
        thumbs.update(cx, |t, _| t.get(&a)),
        Thumbnail::Pending
    ));
    let image = wait(cx, &a);
    let bytes = image.as_bytes(0).unwrap();
    assert_eq!(bytes.len(), 160 * 90 * 4);
    assert!(bytes.chunks(4).any(|p| p[3] > 0), "words were drawn");
    assert!(bytes.chunks(4).any(|p| p[3] == 0), "and the rest is clear");

    // The same slide asked for by another card: the same image.
    let again = ThumbnailRequest::new(
        theme.clone(),
        "John 3:16",
        "For God so loved the world",
        Vec::new(),
    );
    assert_eq!(again.key(), a.key());
    assert_eq!(wait(cx, &again).id, image.id);

    // Two more slides push the first out (capacity 2); asked again, it is
    // drawn anew.
    let b = request("that he gave his only Son");
    let c = request("that whoever believes in him");
    wait(cx, &b);
    wait(cx, &c);
    let stats = cx.update(|cx| thumbs.read(cx).stats());
    assert_eq!((stats.drawn, stats.evicted), (3, 1));
    assert!(matches!(
        thumbs.update(cx, |t, _| t.get(&a)),
        Thumbnail::Pending
    ));
    let redrawn = wait(cx, &a);
    assert_ne!(redrawn.id, image.id);
    assert_eq!(cx.update(|cx| thumbs.read(cx).len()), 2);

    // An empty slide is a clear thumbnail, not a failure.
    let blank = wait(cx, &request(""));
    assert!(blank.as_bytes(0).unwrap().chunks(4).all(|p| p[3] == 0));
}
