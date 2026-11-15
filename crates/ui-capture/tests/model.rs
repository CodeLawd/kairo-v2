use kairo_domain::capture::{
    CaptureCodec, CaptureFrameRate, CaptureResolution, CaptureSettings, CaptureStatus,
};
use kairo_domain::overlay::OverlayOutputKind;
use kairo_domain::overlay_defaults::make_overlay_output;
use ui_capture::model::*;

fn output(
    id: &str,
    name: &str,
    kind: OverlayOutputKind,
    enabled: bool,
) -> kairo_domain::overlay::OverlayOutput {
    let mut o = make_overlay_output(id, kind);
    o.name = name.into();
    o.enabled = enabled;
    o
}

#[test]
fn plan_names_source_format_and_sound() {
    let mut s = CaptureSettings::default();
    assert_eq!(
        capture_plan(&s, &[]),
        "No screen · H.264 1080p30 · No sound"
    );
    let outputs = vec![output("a", "Main", OverlayOutputKind::Screen, true)];
    s.codec = CaptureCodec::ProRes4444;
    s.resolution = CaptureResolution::P720;
    s.frame_rate = CaptureFrameRate::Fps60;
    s.audio_device_id = "x".into();
    assert_eq!(
        capture_plan(&s, &outputs),
        "Main · ProRes 4444 720p60 · Sound"
    );
    s.audio_device_name = "Desk".into();
    assert!(capture_plan(&s, &outputs).ends_with("· Desk"));
}

#[test]
fn start_item_states() {
    let mut status = CaptureStatus::default();
    assert_eq!(capture_label(&status, false), "Start Capture");
    assert_eq!(capture_label(&status, true), "Starting…");
    assert!(capture_disabled(&status, false, true), "unavailable");
    status.available = true;
    assert!(!capture_disabled(&status, false, true));
    assert!(capture_disabled(&status, false, false), "no source");
    status.reason = Some("No recorder".into());
    assert_eq!(capture_title(&status, "plan").as_deref(), Some("plan"));
    status.available = false;
    assert_eq!(
        capture_title(&status, "plan").as_deref(),
        Some("No recorder")
    );
    status.recording = true;
    status.source_name = Some("Main".into());
    assert!(!capture_disabled(&status, false, false));
    assert_eq!(capture_label(&status, false), "Stop Capture");
    assert_eq!(capture_label(&status, true), "Stopping…");
    assert_eq!(capture_title(&status, "plan").as_deref(), Some("Main"));
}

#[test]
fn folder_size_and_audio_hints() {
    assert_eq!(short_folder(""), "Movies › Kairo");
    assert_eq!(short_folder("/Users/me/Movies/Kairo"), "Movies › Kairo");
    assert_eq!(short_folder("C:\\Rec"), "C: › Rec");
    let mut s = CaptureSettings::default();
    assert_eq!(size_hint(&s), "About 5.4 GB per hour");
    s.codec = CaptureCodec::ProRes422;
    assert!(!compressed(&s));
    assert_eq!(size_hint(&s), "About 66 GB per hour");
    s.audio_device_id = "gone".into();
    let inputs = vec![("mic".to_owned(), "Mic".to_owned())];
    assert!(audio_missing(&s, &inputs));
    assert!(!audio_missing(&s, &[]));
    assert_eq!(audio_hint(&s, true), "The chosen input isn’t connected");
    s.audio_device_name = "Desk".into();
    assert_eq!(audio_hint(&s, true), "Desk isn’t connected");
    assert!(audio_hint(&s, false).starts_with("Plug the sound desk"));
}

#[test]
fn source_hint_and_syphon() {
    let s = CaptureSettings::default();
    let outputs = vec![output("a", "Main", OverlayOutputKind::Screen, false)];
    assert_eq!(
        source_hint(&s, &outputs).as_deref(),
        Some("Main is off — turn it on in Screens")
    );
    assert_eq!(kind_label(OverlayOutputKind::Device), "Video card");
    assert_eq!(syphon_title(false, Some(true), None), "Off");
    assert_eq!(syphon_title(true, Some(true), None), "Sending");
    assert_eq!(
        syphon_title(true, Some(false), Some("No server")),
        "No server"
    );
    assert_eq!(syphon_title(true, None, Some("")), "Starting");
    assert_eq!(syphon_action(0), ("Set up Syphon…", "add:syphon"));
    assert_eq!(syphon_action(2), ("Syphon Settings…", "kind:syphon"));
}
