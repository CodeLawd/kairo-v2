//! Audio input devices. From `src/renderer/src/audio/devices.ts`.
//!
//! Ids are cpal device ids (`host:id`), stable across runs where the OS
//! allows. `audio.deviceId` saved by Electron holds a Chromium id, an opaque
//! per-origin hash that cannot be mapped, or, from before renderer-side
//! enumeration, a CoreAudio label. A saved value that matches a device label
//! is migrated to that device's id once; anything else falls back to the
//! default, as Electron does, and is left in settings untouched.

use cpal::traits::{DeviceTrait, HostTrait};
use kairo_domain::status::{AudioDevice, AudioDeviceKind};

pub const DEFAULT_DEVICE_ID: &str = "default";

pub fn default_device() -> AudioDevice {
    AudioDevice {
        id: DEFAULT_DEVICE_ID.into(),
        label: "System Default".into(),
        kind: AudioDeviceKind::AudioInput,
        is_default: true,
    }
}

/// The system default first, then every input with a usable id. Never empty.
pub fn list_audio_input_devices() -> Vec<AudioDevice> {
    let mut devices = vec![default_device()];
    let host = cpal::default_host();
    let Ok(inputs) = host.input_devices() else {
        return devices;
    };
    for (index, device) in inputs.enumerate() {
        let Ok(id) = device.id() else { continue };
        let label = device
            .description()
            .map(|d| d.name().to_owned())
            .ok()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("Microphone {}", index + 1));
        devices.push(AudioDevice {
            id: id.to_string(),
            label,
            kind: AudioDeviceKind::AudioInput,
            is_default: false,
        });
    }
    devices
}

/// `resolveCaptureDeviceId`: the saved id while present, else the default.
pub fn resolve_capture_device_id(devices: &[AudioDevice], saved_id: &str) -> String {
    if !saved_id.is_empty() && devices.iter().any(|d| d.id == saved_id) {
        return saved_id.into();
    }
    devices
        .iter()
        .find(|d| d.is_default)
        .or(devices.first())
        .map_or_else(|| DEFAULT_DEVICE_ID.into(), |d| d.id.clone())
}

/// The id to store in place of a legacy `audio.deviceId`, when the saved
/// value names a present device by label. `None` leaves settings alone.
pub fn migrate_saved_device_id(devices: &[AudioDevice], saved_id: &str) -> Option<String> {
    let saved = kairo_domain::js::trim(saved_id);
    if saved.is_empty() || saved == DEFAULT_DEVICE_ID || devices.iter().any(|d| d.id == saved) {
        return None;
    }
    let matches: Vec<_> = devices
        .iter()
        .filter(|d| !d.is_default && d.label.trim().eq_ignore_ascii_case(saved))
        .collect();
    // Two inputs with the same name: do not guess.
    match matches.as_slice() {
        [only] => Some(only.id.clone()),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    BuiltIn,
    External,
    Continuity,
    Virtual,
}

impl InputKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::BuiltIn => "Built-in",
            Self::External => "External",
            Self::Continuity => "Continuity",
            Self::Virtual => "Virtual",
        }
    }
}

/// What kind of input a device is, from its name.
pub fn input_kind(device: &AudioDevice) -> InputKind {
    let label = device.label.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| label.contains(w));
    if any(&["iphone", "ipad", "airpods", "continuity"]) {
        InputKind::Continuity
    } else if any(&[
        "virtual",
        "teams",
        "zoom",
        "blackhole",
        "loopback",
        "aggregate",
        "cable",
    ]) {
        InputKind::Virtual
    } else if any(&["built-in", "macbook", "imac", "internal"]) {
        InputKind::BuiltIn
    } else {
        InputKind::External
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, label: &str) -> AudioDevice {
        AudioDevice {
            id: id.into(),
            label: label.into(),
            kind: AudioDeviceKind::AudioInput,
            is_default: false,
        }
    }

    fn list() -> Vec<AudioDevice> {
        vec![
            default_device(),
            device("coreaudio:BuiltInMic", "MacBook Pro Microphone"),
            device("coreaudio:Scarlett", "Scarlett 2i2 USB"),
        ]
    }

    #[test]
    fn a_present_saved_id_wins_else_the_default() {
        assert_eq!(
            resolve_capture_device_id(&list(), "coreaudio:Scarlett"),
            "coreaudio:Scarlett"
        );
        assert_eq!(resolve_capture_device_id(&list(), "gone"), "default");
        assert_eq!(resolve_capture_device_id(&list(), ""), "default");
        assert_eq!(resolve_capture_device_id(&[], "x"), "default");
    }

    #[test]
    fn a_legacy_label_migrates_once_and_chromium_hashes_do_not() {
        assert_eq!(
            migrate_saved_device_id(&list(), " scarlett 2i2 usb "),
            Some("coreaudio:Scarlett".into())
        );
        let chromium = "6f1c2c1d0b0a9e8f7d6c5b4a39281706f5e4d3c2b1a09f8e7d6c5b4a39281706";
        assert_eq!(migrate_saved_device_id(&list(), chromium), None);
        assert_eq!(migrate_saved_device_id(&list(), "coreaudio:Scarlett"), None);
        assert_eq!(migrate_saved_device_id(&list(), "default"), None);
        assert_eq!(migrate_saved_device_id(&list(), "System Default"), None);
        let mut twins = list();
        twins.push(device("coreaudio:Scarlett2", "Scarlett 2i2 USB"));
        assert_eq!(migrate_saved_device_id(&twins, "Scarlett 2i2 USB"), None);
    }

    #[test]
    fn input_kinds_follow_the_name() {
        let kind = |l: &str| input_kind(&device("x", l)).label();
        assert_eq!(kind("Josh's iPhone Microphone"), "Continuity");
        assert_eq!(kind("BlackHole 2ch"), "Virtual");
        assert_eq!(kind("MacBook Pro Microphone"), "Built-in");
        assert_eq!(kind("Scarlett 2i2"), "External");
    }
}
