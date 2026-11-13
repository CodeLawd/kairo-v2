//! Device labels as Chromium showed them, and how a saved label finds its
//! device again.
//!
//! Electron saved the `MediaDeviceInfo.label` the operator picked. On macOS
//! Chromium builds a camera's label from AVFoundation's `localizedName`
//! plus, when the device's unique id ends in a vendor and product id, that
//! model id in brackets (`VideoCaptureDeviceDescriptor::GetNameAndModel`):
//! `"Logitech BRIO (046d:085e)"`. Audio inputs carry Core Audio's name, with
//! a `"Default - "` entry for the system default.
//!
//! A saved label is matched exactly first. If none matches, the label is
//! compared with the bare device name after removing a `"Default - "`
//! prefix and then each trailing bracketed group in turn, so labels whose
//! suffix this port derives differently (built-in or Bluetooth markers,
//! another model id format) still find their device.

/// A device as the operator sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// The backend's own handle (AVFoundation unique id, cpal name).
    pub id: String,
    /// The name the system gives the device.
    pub name: String,
    /// The label Chromium would have shown.
    pub label: String,
}

/// `GetNameAndModel`: `name (vvvv:pppp)` when `unique_id` ends in eight hex
/// digits (vendor then product), unless the name already shows the model.
pub fn chromium_label(name: &str, unique_id: &str) -> String {
    match model_id(unique_id) {
        Some(model) if !name.contains(&model) => format!("{name} ({model})"),
        _ => name.to_owned(),
    }
}

/// The vendor and product id at the end of an AVFoundation unique id
/// (`0x14100000046d085e` becomes `046d:085e`).
pub fn model_id(unique_id: &str) -> Option<String> {
    let bytes = unique_id.as_bytes();
    if bytes.len() < 8 {
        return None;
    }
    let tail = &unique_id[unique_id.len() - 8..];
    if !tail.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    // A unique id that is all digits is a serial number, not a USB id.
    if unique_id.len() > 8 && !unique_id.starts_with("0x") {
        return None;
    }
    Some(format!("{}:{}", &tail[..4], &tail[4..]))
}

/// Whether `saved` names the system default (`"Default - …"`).
pub fn is_default(saved: &str) -> bool {
    saved.starts_with("Default - ")
}

/// The forms of `saved` to compare with a bare device name, most specific
/// first: without a `"Default - "` or `"Communications - "` prefix, then with
/// each trailing ` (…)` group removed in turn.
pub fn name_forms(saved: &str) -> Vec<&str> {
    let mut s = saved
        .strip_prefix("Default - ")
        .or_else(|| saved.strip_prefix("Communications - "))
        .unwrap_or(saved)
        .trim();
    let mut out = vec![s];
    while s.ends_with(')') {
        let Some(open) = s.rfind(" (") else {
            break;
        };
        s = s[..open].trim_end();
        if s.is_empty() {
            break;
        }
        out.push(s);
    }
    out
}

/// The device `saved` refers to: an exact label first, then a name form.
pub fn find<'a>(devices: &'a [DeviceInfo], saved: &str) -> Option<&'a DeviceInfo> {
    if saved.is_empty() {
        return None;
    }
    if let Some(d) = devices.iter().find(|d| d.label == saved) {
        return Some(d);
    }
    for form in name_forms(saved) {
        if let Some(d) = devices.iter().find(|d| d.name == form || d.label == form) {
            return Some(d);
        }
    }
    None
}

/// Labels for a device list as `enumerateDevices` deduplicated them.
pub fn labels(devices: &[DeviceInfo]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in devices {
        if !d.label.is_empty() && !out.contains(&d.label) {
            out.push(d.label.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, unique: &str) -> DeviceInfo {
        DeviceInfo {
            id: unique.to_owned(),
            name: name.to_owned(),
            label: chromium_label(name, unique),
        }
    }

    #[test]
    fn usb_cameras_carry_their_model_id_like_chromium() {
        assert_eq!(
            chromium_label("Logitech BRIO", "0x14100000046d085e"),
            "Logitech BRIO (046d:085e)"
        );
        assert_eq!(model_id("0x1a11000005ac8510").as_deref(), Some("05ac:8510"));
        // Already in the name, too short, not hex, or a serial number.
        assert_eq!(
            chromium_label("Cam (046d:085e)", "0x1000046d085e"),
            "Cam (046d:085e)"
        );
        assert_eq!(chromium_label("Cam", "abc"), "Cam");
        assert_eq!(
            chromium_label("Cam", "47B4B64B-7067-4B9C-AD2B-AE273A71F4B5"),
            "Cam"
        );
        assert_eq!(chromium_label("Cam", "1234567890"), "Cam");
    }

    #[test]
    fn saved_labels_find_their_device_exactly_then_by_name() {
        let devices = vec![
            dev("FaceTime HD Camera", "47B4B64B-7067-4B9C-AD2B-AE273A71F4B5"),
            dev("Logitech BRIO", "0x14100000046d085e"),
            dev("Cam Link 4K", "0x11000000fd90066"),
        ];
        assert_eq!(
            find(&devices, "Logitech BRIO (046d:085e)").unwrap().name,
            "Logitech BRIO"
        );
        // A suffix Chromium added that this port does not derive.
        assert_eq!(
            find(&devices, "FaceTime HD Camera (Built-in)")
                .unwrap()
                .name,
            "FaceTime HD Camera"
        );
        assert_eq!(
            find(&devices, "FaceTime HD Camera (3A71:F4B5)")
                .unwrap()
                .name,
            "FaceTime HD Camera"
        );
        assert_eq!(
            find(&devices, "Logitech BRIO").unwrap().name,
            "Logitech BRIO"
        );
        assert!(find(&devices, "Logitech C920 (046d:082d)").is_none());
        assert!(find(&devices, "").is_none());
        assert!(find(&[], "Logitech BRIO").is_none());
    }

    #[test]
    fn default_and_bracket_forms() {
        assert!(is_default("Default - MacBook Pro Microphone (Built-in)"));
        assert!(!is_default("MacBook Pro Microphone"));
        assert_eq!(
            name_forms("Default - MacBook Pro Microphone (Built-in)"),
            vec![
                "MacBook Pro Microphone (Built-in)",
                "MacBook Pro Microphone"
            ]
        );
        assert_eq!(name_forms("(odd)"), vec!["(odd)"]);
        assert_eq!(name_forms("A (x) (y)"), vec!["A (x) (y)", "A (x)", "A"]);
    }

    #[test]
    fn labels_are_deduplicated_and_never_empty() {
        let mut devices = vec![dev("A", "1"), dev("A", "2"), dev("B", "3")];
        devices.push(DeviceInfo {
            id: "4".into(),
            name: String::new(),
            label: String::new(),
        });
        assert_eq!(labels(&devices), vec!["A", "B"]);
    }
}
