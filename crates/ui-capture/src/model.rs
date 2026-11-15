//! Capture rules from `AppShell.tsx` (`CaptureSection`, `LiveMenu`) and
//! `CaptureSettings.tsx`, free of the UI.

use kairo_domain::capture::{
    CAPTURE_CODECS, CaptureCodec, CaptureSettings, CaptureStatus, capture_source_for,
    gigabytes_per_hour,
};
use kairo_domain::overlay::{OverlayOutput, OverlayOutputKind};

/// `KIND_LABEL`.
pub fn kind_label(kind: OverlayOutputKind) -> &'static str {
    match kind {
        OverlayOutputKind::Screen => "Screen",
        OverlayOutputKind::Ndi => "NDI",
        OverlayOutputKind::Syphon => "Syphon",
        OverlayOutputKind::Device => "Video card",
    }
}

/// The capture plan in the Start button's tooltip: source, format, sound.
pub fn capture_plan(settings: &CaptureSettings, outputs: &[OverlayOutput]) -> String {
    let source = capture_source_for(settings, outputs)
        .map(|o| o.name.clone())
        .unwrap_or_else(|| "No screen".into());
    let codec = CAPTURE_CODECS
        .iter()
        .find(|c| c.0 == settings.codec)
        .map(|c| c.1.strip_prefix("Apple ").unwrap_or(c.1))
        .unwrap_or("");
    let sound = if settings.audio_device_id.is_empty() {
        "No sound".to_owned()
    } else if settings.audio_device_name.is_empty() {
        "Sound".to_owned()
    } else {
        settings.audio_device_name.clone()
    };
    format!(
        "{source} · {codec} {}{} · {sound}",
        settings.resolution.as_str(),
        settings.frame_rate.value()
    )
}

/// The Start/Stop item's label.
pub fn capture_label(status: &CaptureStatus, busy: bool) -> &'static str {
    match (busy, status.recording) {
        (true, true) => "Stopping…",
        (true, false) => "Starting…",
        (false, true) => "Stop Capture",
        (false, false) => "Start Capture",
    }
}

/// Start is off while busy, and while idle with capture unavailable or no source.
pub fn capture_disabled(status: &CaptureStatus, busy: bool, has_source: bool) -> bool {
    busy || (!status.recording && (!status.available || !has_source))
}

/// The Start/Stop item's tooltip.
pub fn capture_title(status: &CaptureStatus, plan: &str) -> Option<String> {
    if status.recording {
        status.source_name.clone()
    } else if status.available {
        Some(plan.to_owned())
    } else {
        status.reason.clone()
    }
}

/// `shortFolder`: the last two folders, which is what tells them apart.
pub fn short_folder(folder: &str) -> String {
    if folder.is_empty() {
        return "Movies › Kairo".into();
    }
    let parts: Vec<&str> = folder
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    let tail = parts[parts.len().saturating_sub(2)..].join(" › ");
    if tail.is_empty() {
        folder.to_owned()
    } else {
        tail
    }
}

/// `About N GB per hour`, one decimal under 10.
pub fn size_hint(settings: &CaptureSettings) -> String {
    let gb = gigabytes_per_hour(settings);
    if gb < 10.0 {
        format!("About {gb:.1} GB per hour")
    } else {
        format!("About {gb:.0} GB per hour")
    }
}

/// H.264 and HEVC have a quality setting; ProRes does not.
pub fn compressed(settings: &CaptureSettings) -> bool {
    matches!(settings.codec, CaptureCodec::H264 | CaptureCodec::Hevc)
}

/// The chosen input is not among those connected (and inputs are known).
pub fn audio_missing(settings: &CaptureSettings, inputs: &[(String, String)]) -> bool {
    !settings.audio_device_id.is_empty()
        && !inputs.is_empty()
        && !inputs.iter().any(|(id, _)| *id == settings.audio_device_id)
}

/// The audio row's hint.
pub fn audio_hint(settings: &CaptureSettings, missing: bool) -> String {
    if missing {
        let name = if settings.audio_device_name.is_empty() {
            "The chosen input"
        } else {
            &settings.audio_device_name
        };
        format!("{name} isn’t connected")
    } else {
        "Plug the sound desk into this Mac to record the service sound.".into()
    }
}

/// The source row's hint when the chosen output is off.
pub fn source_hint(settings: &CaptureSettings, outputs: &[OverlayOutput]) -> Option<String> {
    capture_source_for(settings, outputs)
        .filter(|o| !o.enabled)
        .map(|o| format!("{} is off — turn it on in Screens", o.name))
}

/// A Syphon switch's tooltip.
pub fn syphon_title(enabled: bool, ready: Option<bool>, reason: Option<&str>) -> String {
    if !enabled {
        "Off".into()
    } else if ready == Some(true) {
        "Sending".into()
    } else {
        reason
            .filter(|r| !r.is_empty())
            .unwrap_or("Starting")
            .to_owned()
    }
}

/// The Syphon section's last item.
pub fn syphon_action(syphons: usize) -> (&'static str, &'static str) {
    if syphons == 0 {
        ("Set up Syphon…", "add:syphon")
    } else {
        ("Syphon Settings…", "kind:syphon")
    }
}
