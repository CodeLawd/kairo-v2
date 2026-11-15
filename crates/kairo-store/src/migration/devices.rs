//! Electron's saved microphone (`audio.deviceId`) to a device label.
//!
//! Chromium gave the renderer `deviceId = hex(HMAC-SHA256(key = origin,
//! message = rawUniqueId + salt))` (`GetHMACForRawMediaDeviceID`), with
//! `default` and `communications` passed through. Electron keeps the salt in
//! `Preferences` at `electron.media.device_id_salt`. The raw id of a CoreAudio
//! input is its device UID, the part of a cpal id after `coreaudio:`. So a
//! saved id is matched by hashing every present input; an HMAC match cannot
//! be a false one. Verified against a real Electron 33 profile (2026-11-15).

use std::path::Path;

use hmac::{Hmac, Mac};
use sha2::Sha256;

/// `electron.media.device_id_salt` from `<userData>/Preferences`.
pub fn device_id_salt(user_data: &Path) -> Option<String> {
    let text = std::fs::read_to_string(user_data.join("Preferences")).ok()?;
    let prefs: serde_json::Value = serde_json::from_str(&text).ok()?;
    prefs
        .get("electron")?
        .get("media")?
        .get("device_id_salt")?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Chromium's hashed id for `raw_unique_id` seen from `origin`.
pub fn chromium_device_id(salt: &str, origin: &str, raw_unique_id: &str) -> String {
    if raw_unique_id == "default" || raw_unique_id == "communications" {
        return raw_unique_id.to_owned();
    }
    let mut mac =
        <Hmac<Sha256>>::new_from_slice(origin.as_bytes()).expect("HMAC accepts any key length");
    mac.update(raw_unique_id.as_bytes());
    mac.update(salt.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// An input the native app can see: its cpal id (`host:raw`) and label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    pub id: String,
    pub label: String,
}

/// What became of the saved microphone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MicrophoneMatch {
    /// Nothing to do: empty, `default`, or already a native id or label.
    Unchanged,
    /// The saved hash names this present input.
    Found(InputDevice),
    /// A Chromium id no present input hashes to (unplugged, or no salt).
    NotConnected,
}

/// Matches Electron's saved `audio.deviceId` against present inputs.
pub fn match_microphone(
    saved: &str,
    salt: Option<&str>,
    origin: &str,
    devices: &[InputDevice],
) -> MicrophoneMatch {
    let saved = saved.trim();
    if saved.is_empty()
        || saved == "default"
        || saved == "communications"
        || devices.iter().any(|d| d.id == saved || d.label == saved)
    {
        return MicrophoneMatch::Unchanged;
    }
    // Chromium ids are 64 lowercase hex characters; anything else is an
    // older label the audio service migrates itself (074).
    let hashed = saved.len() == 64 && saved.bytes().all(|b| b.is_ascii_hexdigit());
    if !hashed {
        return MicrophoneMatch::Unchanged;
    }
    let Some(salt) = salt else {
        return MicrophoneMatch::NotConnected;
    };
    devices
        .iter()
        .find(|d| {
            let raw = d.id.split_once(':').map_or(d.id.as_str(), |(_, raw)| raw);
            chromium_device_id(salt, origin, raw) == saved
        })
        .cloned()
        .map_or(MicrophoneMatch::NotConnected, MicrophoneMatch::Found)
}
