//! The video card service against a fake provider, ported from Electron's
//! `video-devices/__tests__/service.test.ts` and `provider-loader.test.ts`.
#![cfg(unix)]

use kairo_domain::video_devices::{
    DriverState, VideoDriverStatus, VideoMode, VideoPlaybackDiagnostics, VideoPort,
    VideoProviderStatus,
};
use kairo_engine_proto::ring::RingWriter;
use kairo_sinks::video_devices::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[derive(Default)]
struct Fake {
    availability: Option<VideoProviderStatus>,
    ports: Vec<VideoPort>,
    opens: usize,
    closes: usize,
    scans: usize,
    frames: Vec<Vec<u8>>,
    open_error: Option<String>,
    enumeration_error: Option<String>,
    /// Ports held open (for swap tests).
    occupied: HashSet<String>,
    exclusive: bool,
    fail_frames: Option<String>,
    broken_subscription: bool,
    errors: Vec<Arc<Mutex<Option<ErrorListener>>>>,
    changed: bool,
    /// On `list_ports`, becomes the availability (driver discovered by a scan).
    discovered: Option<VideoProviderStatus>,
    ring: bool,
    gate: Option<Arc<(Mutex<bool>, std::sync::Condvar)>>,
}

#[derive(Clone)]
struct FakeProvider(Arc<Mutex<Fake>>);

struct FakeSink {
    fake: Arc<Mutex<Fake>>,
    port: String,
    closed: Mutex<bool>,
    errors: Arc<Mutex<Option<ErrorListener>>>,
    ring: Option<Arc<RingWriter>>,
}

impl OutputSink for FakeSink {
    fn diagnostics(&self) -> Option<VideoPlaybackDiagnostics> {
        None
    }
    fn frame_ring(&self) -> Option<Arc<RingWriter>> {
        self.ring.clone()
    }
    fn set_frame(&self, bgra: &[u8], _: u32, _: u32) -> Result<(), String> {
        let mut fake = self.fake.lock().unwrap();
        if let Some(error) = &fake.fail_frames {
            return Err(error.clone());
        }
        if !*self.closed.lock().unwrap() {
            fake.frames.push(bgra.to_vec());
        }
        Ok(())
    }
    fn on_error(&self, listener: ErrorListener) {
        if self.fake.lock().unwrap().broken_subscription {
            panic!("Broken subscription");
        }
        *self.errors.lock().unwrap() = Some(listener);
    }
    fn close(&self) {
        let mut closed = self.closed.lock().unwrap();
        if !*closed {
            *closed = true;
            let mut fake = self.fake.lock().unwrap();
            fake.closes += 1;
            fake.occupied.remove(&self.port);
            *self.errors.lock().unwrap() = None;
        }
    }
}

impl Provider for FakeProvider {
    fn name(&self) -> &str {
        "fake"
    }
    fn status(&self) -> VideoProviderStatus {
        self.0
            .lock()
            .unwrap()
            .availability
            .clone()
            .unwrap_or(VideoProviderStatus {
                available: true,
                version: Some("test".into()),
                reason: None,
                driver: None,
            })
    }
    fn list_ports(&mut self) -> Result<Vec<VideoPort>, String> {
        let mut fake = self.0.lock().unwrap();
        fake.scans += 1;
        if let Some(found) = fake.discovered.clone() {
            fake.availability = Some(found);
        }
        if let Some(error) = &fake.enumeration_error {
            return Err(error.clone());
        }
        Ok(fake.ports.clone())
    }
    fn ports_changed(&mut self) -> bool {
        std::mem::take(&mut self.0.lock().unwrap().changed)
    }
    fn open_output(&mut self, port: &str, _: &str, _: bool) -> Result<Box<dyn OutputSink>, String> {
        let gate = {
            let mut fake = self.0.lock().unwrap();
            fake.opens += 1;
            fake.gate.clone()
        };
        if let Some(gate) = gate {
            let (lock, cv) = &*gate;
            let _held = cv.wait_while(lock.lock().unwrap(), |open| !*open).unwrap();
        }
        let mut fake = self.0.lock().unwrap();
        if let Some(error) = &fake.open_error {
            return Err(error.clone());
        }
        if fake.exclusive && !fake.occupied.insert(port.to_owned()) {
            return Err("Port busy".into());
        }
        let errors = Arc::new(Mutex::new(None));
        fake.errors.push(errors.clone());
        let ring = fake.ring.then(|| {
            Arc::new(
                RingWriter::create(&format!("/kairo-vt-{}", std::process::id()), 1920, 1080)
                    .unwrap(),
            )
        });
        Ok(Box::new(FakeSink {
            fake: self.0.clone(),
            port: port.to_owned(),
            closed: Mutex::new(false),
            errors,
            ring,
        }))
    }
}

fn mode(id: &str, n: f64, d: f64) -> VideoMode {
    VideoMode {
        id: id.into(),
        label: "1080p29.97".into(),
        width: 1920.0,
        height: 1080.0,
        frame_rate_n: n,
        frame_rate_d: d,
        interlaced: false,
    }
}

fn port(id: &str) -> VideoPort {
    VideoPort {
        id: id.into(),
        provider: "fake".into(),
        device_name: "Test card".into(),
        port_name: "SDI 1".into(),
        modes: vec![mode("fake:1080p", 30000.0, 1001.0)],
        supports_key_fill: false,
        unavailable_reason: None,
    }
}

fn output() -> Binding {
    Binding {
        id: "card".into(),
        enabled: true,
        port_id: "fake:1:0".into(),
        mode_id: "fake:1080p".into(),
        key_fill: false,
    }
}

fn with(id: &str, f: impl FnOnce(&mut Binding)) -> Binding {
    let mut b = output();
    b.id = id.into();
    f(&mut b);
    b
}

fn setup() -> (Arc<Mutex<Fake>>, VideoDevices) {
    let fake = Arc::new(Mutex::new(Fake {
        ports: vec![port("fake:1:0")],
        ..Fake::default()
    }));
    let service = VideoDevices::start(
        vec![Box::new(FakeProvider(fake.clone()))],
        Duration::ZERO,
        || {},
    );
    (fake, service)
}

fn reconcile(service: &VideoDevices, bindings: Vec<Binding>) {
    service.reconcile(bindings);
    service.settle();
}

fn refresh(service: &VideoDevices) {
    service.refresh();
    service.settle();
}

fn close(service: &VideoDevices) {
    assert!(service.close(Duration::from_secs(5)));
}

#[test]
fn publishes_driver_diagnostics_discovered_by_the_first_scan() {
    let (fake, service) = setup();
    fake.lock().unwrap().discovered = Some(VideoProviderStatus {
        available: true,
        version: Some("15.3.1".into()),
        reason: None,
        driver: Some(VideoDriverStatus {
            state: DriverState::Compatibility,
            message: "Compatibility mode".into(),
        }),
    });
    refresh(&service);
    let snapshot = service.snapshot();
    assert_eq!(
        snapshot.providers[0].status.driver.as_ref().unwrap().state,
        DriverState::Compatibility
    );
    assert_eq!(
        snapshot.providers[0].status.version.as_deref(),
        Some("15.3.1")
    );
    close(&service);
}

#[test]
fn preserves_version_and_update_advice_when_driver_enumeration_fails() {
    let (fake, service) = setup();
    {
        let mut fake = fake.lock().unwrap();
        fake.availability = Some(VideoProviderStatus {
            available: true,
            version: Some("14.4".into()),
            reason: None,
            driver: Some(VideoDriverStatus {
                state: DriverState::Unsupported,
                message: "Update Desktop Video".into(),
            }),
        });
        fake.enumeration_error = Some("Update Desktop Video".into());
    }
    refresh(&service);
    let provider = &service.snapshot().providers[0];
    assert!(!provider.status.available);
    assert_eq!(
        provider.status.driver.as_ref().unwrap().state,
        DriverState::Unsupported
    );
    assert_eq!(provider.status.version.as_deref(), Some("14.4"));
    close(&service);
}

#[test]
fn exposes_the_provider_frame_ring_through_the_managed_sink() {
    let (fake, service) = setup();
    fake.lock().unwrap().ring = true;
    reconcile(&service, vec![output()]);
    let ring = service
        .sink_for("card")
        .unwrap()
        .frame_ring()
        .expect("ring");
    assert_eq!(ring.raster(), (1920, 1080));
    close(&service);
}

#[test]
fn counts_valid_captured_frames_and_keeps_snapshot_diagnostics_isolated() {
    let (_, service) = setup();
    reconcile(&service, vec![output()]);
    let sink = service.sink_for("card").unwrap();
    sink.set_frame(&[0; 4], 1, 1).unwrap();
    sink.set_frame(&[0; 4], 1, 1).unwrap();
    assert!(sink.set_frame(&[0; 3], 1, 1).is_err());
    let mut snapshot = service.snapshot();
    let diagnostics = snapshot
        .outputs
        .get_mut("card")
        .unwrap()
        .diagnostics
        .as_mut()
        .unwrap();
    assert_eq!(diagnostics.captured_frames, 2.0);
    diagnostics.captured_frames = 100.0;
    assert_eq!(
        service.snapshot().outputs["card"]
            .diagnostics
            .as_ref()
            .unwrap()
            .captured_frames,
        2.0
    );
    close(&service);
}

#[test]
fn opens_once_forwards_valid_frames_and_rejects_malformed_frames() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    let sink = service.sink_for("card").unwrap();
    assert!(service.output_status("card").ready);
    sink.set_frame(&[1, 2, 3, 255], 1, 1).unwrap();
    assert_eq!(fake.lock().unwrap().frames[0], vec![1, 2, 3, 255]);
    assert!(
        sink.set_frame(&[0; 3], 1, 1)
            .unwrap_err()
            .to_lowercase()
            .contains("frame")
    );
    assert!(
        sink.set_frame(&[0; 4], 0, 1)
            .unwrap_err()
            .to_lowercase()
            .contains("frame")
    );
    reconcile(&service, vec![output()]);
    assert!(Arc::ptr_eq(&service.sink_for("card").unwrap(), &sink));
    assert_eq!(fake.lock().unwrap().opens, 1);
    close(&service);
    // A second close is safe and releases nothing more.
    service.close(Duration::from_secs(1));
    assert_eq!(fake.lock().unwrap().closes, 1);
}

#[test]
fn disconnect_closes_the_sink_and_reconnect_opens_it_again() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    let stale = service.sink_for("card").unwrap();
    fake.lock().unwrap().ports.clear();
    refresh(&service);
    assert_eq!(service.output_status("card").reason, "Device disconnected");
    assert_eq!(fake.lock().unwrap().closes, 1);
    stale.set_frame(&[0; 4], 1, 1).unwrap();
    assert!(fake.lock().unwrap().frames.is_empty());
    fake.lock().unwrap().ports = vec![port("fake:1:0")];
    refresh(&service);
    assert!(service.output_status("card").ready);
    assert_eq!(fake.lock().unwrap().opens, 2);
    close(&service);
}

#[test]
fn rejects_busy_ports_unsupported_modes_and_unsupported_key_fill() {
    let (fake, service) = setup();
    fake.lock().unwrap().ports[0].unavailable_reason = Some("In use by another app".into());
    reconcile(&service, vec![output()]);
    assert_eq!(
        service.output_status("card").reason,
        "In use by another app"
    );
    fake.lock().unwrap().ports = vec![port("fake:1:0")];
    reconcile(
        &service,
        vec![with("card", |b| b.mode_id = "missing".into())],
    );
    assert_eq!(
        service.output_status("card").reason,
        "Choose a supported video mode"
    );
    reconcile(&service, vec![with("card", |b| b.key_fill = true)]);
    assert_eq!(
        service.output_status("card").reason,
        "Key/fill unavailable for this port"
    );
    assert_eq!(fake.lock().unwrap().opens, 0);
    close(&service);
}

#[test]
fn prevents_duplicate_acquisitions_and_releases_removed_outputs() {
    let (fake, service) = setup();
    reconcile(&service, vec![output(), with("other", |_| {})]);
    assert_eq!(
        service.output_status("other").reason,
        "Port assigned to another output"
    );
    assert_eq!(fake.lock().unwrap().opens, 1);
    reconcile(&service, vec![]);
    assert_eq!(fake.lock().unwrap().closes, 1);
    assert!(service.sink_for("card").is_none());
    close(&service);
}

#[test]
fn contains_provider_errors_and_retries_acquisition() {
    let (fake, service) = setup();
    fake.lock().unwrap().open_error = Some("In use by another app".into());
    reconcile(&service, vec![output()]);
    assert_eq!(
        service.output_status("card").reason,
        "In use by another app"
    );
    fake.lock().unwrap().open_error = None;
    refresh(&service);
    assert!(service.output_status("card").ready);
    fake.lock().unwrap().enumeration_error = Some("Driver lost".into());
    refresh(&service);
    assert_eq!(service.output_status("card").reason, "Driver lost");
    assert_eq!(fake.lock().unwrap().closes, 1);
    close(&service);
}

#[test]
fn shutdown_closes_an_acquisition_still_in_flight_and_prevents_further_opens() {
    let (fake, service) = setup();
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    fake.lock().unwrap().gate = Some(gate.clone());
    service.reconcile(vec![output()]);
    while fake.lock().unwrap().opens == 0 {
        std::thread::sleep(Duration::from_millis(1));
    }
    let service = Arc::new(service);
    let closing = {
        let service = service.clone();
        std::thread::spawn(move || service.close(Duration::from_secs(5)))
    };
    std::thread::sleep(Duration::from_millis(20));
    *gate.0.lock().unwrap() = true;
    gate.1.notify_all();
    assert!(closing.join().unwrap());
    assert!(service.sink_for("card").is_none());
    assert_eq!(fake.lock().unwrap().closes, 1);
    service.reconcile(vec![output()]);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(fake.lock().unwrap().opens, 1);
}

#[test]
fn hotplug_is_debounced_and_emits_a_recovered_snapshot() {
    let fake = Arc::new(Mutex::new(Fake {
        ports: vec![port("fake:1:0")],
        ..Fake::default()
    }));
    let (tx, rx) = mpsc::channel();
    let service = VideoDevices::start(
        vec![Box::new(FakeProvider(fake.clone()))],
        Duration::from_millis(250),
        move || {
            let _ = tx.send(());
        },
    );
    reconcile(&service, vec![output()]);
    while rx.try_recv().is_ok() {}
    {
        let mut fake = fake.lock().unwrap();
        fake.ports.clear();
        fake.changed = true;
    }
    // Polled every 2 s, then the 250 ms debounce.
    rx.recv_timeout(Duration::from_secs(5)).expect("changed");
    service.settle();
    assert!(!service.output_status("card").ready);
    assert_eq!(fake.lock().unwrap().closes, 1);
    close(&service);
}

struct Broken;
impl Provider for Broken {
    fn name(&self) -> &str {
        "broken"
    }
    fn status(&self) -> VideoProviderStatus {
        panic!("Driver failed")
    }
    fn list_ports(&mut self) -> Result<Vec<VideoPort>, String> {
        Ok(Vec::new())
    }
    fn open_output(&mut self, _: &str, _: &str, _: bool) -> Result<Box<dyn OutputSink>, String> {
        Err("Unavailable".into())
    }
}

#[test]
fn one_failing_provider_does_not_disable_another_provider() {
    let fake = Arc::new(Mutex::new(Fake {
        ports: vec![port("fake:1:0")],
        ..Fake::default()
    }));
    let service = VideoDevices::start(
        vec![Box::new(Broken), Box::new(FakeProvider(fake))],
        Duration::ZERO,
        || {},
    );
    reconcile(&service, vec![output()]);
    assert!(service.output_status("card").ready);
    let providers = service.snapshot().providers;
    assert!(!providers[0].status.available);
    assert_eq!(providers[0].status.reason.as_deref(), Some("Driver failed"));
    close(&service);
}

#[test]
fn mode_changes_close_the_old_acquisition_before_reopening() {
    let (fake, service) = setup();
    fake.lock().unwrap().ports[0]
        .modes
        .push(mode("fake:other", 30000.0, 1001.0));
    reconcile(&service, vec![output()]);
    let stale = service.sink_for("card").unwrap();
    reconcile(
        &service,
        vec![with("card", |b| b.mode_id = "fake:other".into())],
    );
    assert_eq!(fake.lock().unwrap().closes, 1);
    assert_eq!(fake.lock().unwrap().opens, 2);
    stale.set_frame(&[0; 4], 1, 1).unwrap();
    assert!(fake.lock().unwrap().frames.is_empty());
    close(&service);
}

#[test]
fn duplicate_sink_close_releases_hardware_once_and_removes_readiness() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    let sink = service.sink_for("card").unwrap();
    sink.close();
    service.settle();
    assert!(!service.output_status("card").ready);
    assert_eq!(service.output_status("card").reason, "Output closed");
    assert!(service.sink_for("card").is_none());
    sink.close();
    close(&service);
    assert_eq!(fake.lock().unwrap().closes, 1);
}

#[test]
fn releases_all_changed_bindings_before_opening_swapped_ports() {
    let (fake, service) = setup();
    {
        let mut fake = fake.lock().unwrap();
        fake.ports.push(port("fake:1:1"));
        fake.exclusive = true;
    }
    reconcile(
        &service,
        vec![output(), with("other", |b| b.port_id = "fake:1:1".into())],
    );
    reconcile(
        &service,
        vec![
            with("card", |b| b.port_id = "fake:1:1".into()),
            with("other", |_| {}),
        ],
    );
    assert!(service.output_status("card").ready);
    assert!(service.output_status("other").ready);
    close(&service);
}

#[test]
fn frame_delivery_failure_closes_the_output_and_retries_only_on_refresh() {
    let (fake, service) = setup();
    fake.lock().unwrap().fail_frames = Some("Playback failed".into());
    reconcile(&service, vec![output()]);
    service
        .sink_for("card")
        .unwrap()
        .set_frame(&[0; 4], 1, 1)
        .unwrap();
    reconcile(&service, vec![output()]);
    assert!(!service.output_status("card").ready);
    assert_eq!(service.output_status("card").reason, "Playback failed");
    assert_eq!(fake.lock().unwrap().closes, 1);
    assert_eq!(fake.lock().unwrap().opens, 1);
    fake.lock().unwrap().fail_frames = None;
    refresh(&service);
    assert_eq!(fake.lock().unwrap().opens, 2);
    assert!(service.output_status("card").ready);
    close(&service);
}

#[test]
fn an_output_live_on_a_port_keeps_it_when_another_is_re_pointed_at_it() {
    let (fake, service) = setup();
    {
        let mut fake = fake.lock().unwrap();
        fake.ports.push(port("fake:1:1"));
        fake.exclusive = true;
    }
    let other = with("other", |b| b.port_id = "fake:1:1".into());
    // `other` is listed first, so list order alone would hand it the port.
    reconcile(&service, vec![other.clone(), output()]);
    let live = service.sink_for("card").unwrap();
    reconcile(
        &service,
        vec![with("other", |b| b.port_id = "fake:1:0".into()), output()],
    );
    assert!(Arc::ptr_eq(&service.sink_for("card").unwrap(), &live));
    assert!(service.output_status("card").ready);
    assert_eq!(
        service.output_status("other").reason,
        "Port assigned to another output"
    );
    close(&service);
}

#[test]
fn a_port_reported_busy_because_kairo_holds_it_stays_live() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    fake.lock().unwrap().ports[0].unavailable_reason = Some("In use".into());
    refresh(&service);
    assert!(service.output_status("card").ready);
    assert_eq!(fake.lock().unwrap().closes, 0);
    reconcile(&service, vec![output(), with("other", |_| {})]);
    assert_eq!(
        service.output_status("other").reason,
        "Port assigned to another output"
    );
    assert_eq!(fake.lock().unwrap().closes, 0);
    close(&service);
}

#[test]
fn steady_state_reconciles_reuse_the_cached_ports() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    reconcile(&service, vec![output()]);
    reconcile(&service, vec![output()]);
    assert_eq!(fake.lock().unwrap().scans, 1);
    refresh(&service);
    assert_eq!(fake.lock().unwrap().scans, 2);
    close(&service);
}

#[test]
fn reports_whether_hardware_is_held() {
    let (_, service) = setup();
    assert!(!service.holds_hardware());
    reconcile(&service, vec![output()]);
    assert!(service.holds_hardware());
    close(&service);
    assert!(!service.holds_hardware());
}

#[test]
fn asynchronous_native_failures_revoke_readiness_and_close_the_output() {
    let (fake, service) = setup();
    reconcile(&service, vec![output()]);
    let listener = fake.lock().unwrap().errors[0].clone();
    (listener.lock().unwrap().as_ref().unwrap())("DeckLink helper exited (SIGSEGV)".into());
    reconcile(&service, vec![output()]);
    assert!(!service.output_status("card").ready);
    assert!(service.output_status("card").reason.contains("SIGSEGV"));
    assert_eq!(fake.lock().unwrap().closes, 1);
    assert!(listener.lock().unwrap().is_none(), "unsubscribed on close");
    close(&service);
}

#[test]
fn cleans_up_a_sink_whose_error_subscription_panics() {
    let (fake, service) = setup();
    fake.lock().unwrap().broken_subscription = true;
    reconcile(&service, vec![output()]);
    assert!(!service.output_status("card").ready);
    assert_eq!(service.output_status("card").reason, "Broken subscription");
    assert!(service.sink_for("card").is_none());
    assert_eq!(fake.lock().unwrap().closes, 1);
    close(&service);
}

#[test]
fn exposes_the_selected_hardware_frame_rate_and_refreshes_it_when_modes_change() {
    let (fake, service) = setup();
    fake.lock().unwrap().ports[0].modes.push(VideoMode {
        interlaced: true,
        ..mode("fake:1080i50", 25000.0, 1000.0)
    });
    reconcile(&service, vec![output()]);
    assert_eq!(
        service.sink_for("card").unwrap().frame_rate,
        Some(30000.0 / 1001.0)
    );
    reconcile(
        &service,
        vec![with("card", |b| b.mode_id = "fake:1080i50".into())],
    );
    assert_eq!(service.sink_for("card").unwrap().frame_rate, Some(25.0));
    close(&service);
}

#[test]
fn says_no_cards_were_detected_not_choose_a_port_when_the_driver_lists_none() {
    let (fake, service) = setup();
    fake.lock().unwrap().ports.clear();
    reconcile(
        &service,
        vec![with("card", |b| {
            b.port_id.clear();
            b.mode_id.clear();
        })],
    );
    assert!(
        service
            .output_status("card")
            .reason
            .starts_with("No video cards detected")
    );
    close(&service);
}

#[test]
fn a_disabled_output_says_so_and_holds_nothing() {
    let (fake, service) = setup();
    reconcile(&service, vec![with("card", |b| b.enabled = false)]);
    assert_eq!(service.output_status("card").reason, "Output disabled");
    assert_eq!(
        service.output_status("unknown").reason,
        "Output unavailable"
    );
    assert_eq!(fake.lock().unwrap().opens, 0);
    close(&service);
}

#[test]
fn a_service_with_no_providers_says_so() {
    let service = VideoDevices::start(Vec::new(), Duration::ZERO, || {});
    reconcile(&service, vec![output()]);
    assert_eq!(
        service.output_status("card").reason,
        "No video device provider available"
    );
    close(&service);
}

// ─── provider-loader.test.ts ───────────────────────────────────────────────

fn registration(
    load: impl FnOnce() -> Result<Box<dyn Provider>, String> + 'static,
) -> Registration {
    Registration {
        name: "fake",
        tuples: &["darwin:arm64", "darwin:x64"],
        load: Box::new(load),
    }
}

#[test]
fn loads_an_available_provider_only_on_a_supported_tuple() {
    let fake = Arc::new(Mutex::new(Fake::default()));
    let f = fake.clone();
    assert!(
        load_provider(
            registration(move || Ok(Box::new(FakeProvider(f)))),
            "darwin:arm64"
        )
        .is_ok()
    );
    let (_, code, reason) = load_provider(
        registration(move || Ok(Box::new(FakeProvider(fake)))),
        "linux:x64",
    )
    .err()
    .unwrap();
    assert_eq!(code, LoadCode::UnsupportedTuple);
    assert_eq!(reason, "Unsupported platform/architecture: linux/x64");
}

#[test]
fn returns_a_driver_reason_without_exposing_an_unusable_provider() {
    let fake = Arc::new(Mutex::new(Fake {
        availability: Some(VideoProviderStatus {
            available: false,
            version: None,
            reason: Some("Install drivers".into()),
            driver: None,
        }),
        ..Fake::default()
    }));
    let (_, code, reason) = load_provider(
        registration(move || Ok(Box::new(FakeProvider(fake)))),
        "darwin:arm64",
    )
    .err()
    .unwrap();
    assert_eq!(code, LoadCode::RuntimeUnavailable);
    assert_eq!(reason, "Install drivers");
}

#[test]
fn contains_module_load_and_status_panics() {
    let (_, code, _) = load_provider(
        registration(|| panic!("native module missing")),
        "darwin:arm64",
    )
    .err()
    .unwrap();
    assert_eq!(code, LoadCode::LoadFailed);
    let (_, code, _) = load_provider(registration(|| Ok(Box::new(Broken))), "darwin:arm64")
        .err()
        .unwrap();
    assert_eq!(code, LoadCode::LoadFailed);
}
