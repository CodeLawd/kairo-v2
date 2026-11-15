//! A retained popover must invalidate its own recording clock, even when
//! capture status and the surrounding shell have not changed.
use gpui_kit::{AppContext, TestAppContext};
use kairo_domain::capture::{CaptureAudioInput, CaptureSettings, CaptureStatus};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use ui_capture::*;

struct Backend;
impl CaptureBackend for Backend {
    fn snapshot(&self) -> CaptureSnapshot {
        CaptureSnapshot {
            status: CaptureStatus {
                recording: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }
    fn listen(&self, _: FeedSink) -> Listening {
        Box::new(())
    }
    fn start(&self, _: Reply<CaptureStatus>) {
        panic!("not part of this clock test")
    }
    fn stop(&self, _: Reply<CaptureStatus>) {
        panic!("not part of this clock test")
    }
    fn save(&self, _: CaptureSettings) {
        panic!("not part of this clock test")
    }
    fn choose_folder(&self, _: Reply<Option<String>>) {
        panic!("not part of this clock test")
    }
    fn reveal(&self) {
        panic!("not part of this clock test")
    }
    fn audio_inputs(&self, _: Reply<Vec<CaptureAudioInput>>) {
        panic!("not part of this clock test")
    }
    fn set_output_enabled(&self, _: String, _: bool) {
        panic!("not part of this clock test")
    }
}

#[gpui_kit::test]
fn recording_clock_notifies_its_view_and_stops_when_recording_stops(cx: &mut TestAppContext) {
    let capture = cx.new(|cx| Capture::new(Arc::new(Backend), cx));
    let menu = cx.new(|cx| LiveMenu::new(capture.clone(), true, cx));
    let notifications = Arc::new(AtomicUsize::new(0));
    let seen = notifications.clone();
    let _observer = cx.update(|cx| {
        cx.observe(&menu, move |_, _| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert!(
        notifications.load(Ordering::SeqCst) > 0,
        "the elapsed clock must invalidate the retained menu"
    );
    capture.update(cx, |c, cx| {
        c.apply(CaptureFeed::Status(CaptureStatus::default()), cx)
    });
    cx.run_until_parked();
    notifications.store(0, Ordering::SeqCst);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    assert_eq!(
        notifications.load(Ordering::SeqCst),
        0,
        "a stopped recording must not keep redrawing"
    );
}
