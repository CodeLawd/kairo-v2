//! Displays: listing them and choosing the one an output is bound to.

use kairo_engine_proto::{DisplayBinding, DisplayInfo, Rect};
use std::collections::HashSet;
use winit::event_loop::ActiveEventLoop;

#[cfg(target_os = "windows")]
mod windows;

pub fn scan(event_loop: &ActiveEventLoop) -> Vec<DisplayInfo> {
    event_loop
        .available_monitors()
        .filter_map(|monitor| {
            let (id, native_id, label) = metadata(&monitor)?;
            let position = monitor.position();
            let size = monitor.size();
            Some(DisplayInfo {
                id,
                native_id,
                label,
                bounds: Rect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
                scale_factor: monitor.scale_factor(),
                primary: event_loop.primary_monitor().as_ref() == Some(&monitor),
                hosts_main_window: false,
                refresh_millihertz: monitor.refresh_rate_millihertz(),
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn metadata(monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u64, String)> {
    use objc2::{MainThreadMarker, msg_send};
    use objc2_app_kit::NSScreen;
    use objc2_foundation::ns_string;
    use winit::platform::macos::MonitorHandleExtMacOS;
    let id = monitor.native_id();
    let screens = NSScreen::screens(MainThreadMarker::new()?);
    let label = screens
        .iter()
        .find_map(|screen| {
            let number = screen
                .deviceDescription()
                .objectForKey(ns_string!("NSScreenNumber"))?;
            // SAFETY: NSScreenNumber is an NSNumber supplied by AppKit.
            let candidate: u32 = unsafe { msg_send![&*number, unsignedIntValue] };
            (candidate == id).then(|| screen.localizedName().to_string())
        })
        .unwrap_or_default();
    Some((id, id as u64, label))
}
#[cfg(target_os = "windows")]
fn metadata(monitor: &winit::monitor::MonitorHandle) -> Option<(u32, u64, String)> {
    windows::metadata(monitor)
}
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn metadata(_: &winit::monitor::MonitorHandle) -> Option<(u32, u64, String)> {
    // Linux/Wayland's monitor ids are not Chromium's EDID display ids. Never
    // claim a random hash is compatible with an existing stored assignment.
    None
}

/// Chromium's PersistentHash is Paul Hsieh's SuperFastHash (32-bit wrapping).
/// Derived from Chromium base/third_party/superfasthash/superfasthash.c;
/// Paul Hsieh copyright/license in display/SuperFastHash.LICENSE.
#[cfg(any(target_os = "windows", test))]
fn persistent_hash(data: &[u8]) -> u32 {
    if data.is_empty() {
        return 0;
    }
    let mut hash = data.len() as u32;
    let word = |i| u16::from_le_bytes([data[i], data[i + 1]]) as u32;
    let mut i = 0;
    while i + 4 <= data.len() {
        hash = hash.wrapping_add(word(i));
        let tmp = (word(i + 2) << 11) ^ hash;
        hash = (hash << 16) ^ tmp;
        hash = hash.wrapping_add(hash >> 11);
        i += 4;
    }
    match data.len() - i {
        3 => {
            hash = hash.wrapping_add(word(i));
            hash ^= hash << 16;
            hash ^= (data[i + 2] as i8 as i32 as u32) << 18;
            hash = hash.wrapping_add(hash >> 11);
        }
        2 => {
            hash = hash.wrapping_add(word(i));
            hash ^= hash << 11;
            hash = hash.wrapping_add(hash >> 17);
        }
        1 => {
            hash = hash.wrapping_add(data[i] as i8 as i32 as u32);
            hash ^= hash << 10;
            hash = hash.wrapping_add(hash >> 1);
        }
        _ => {}
    }
    hash ^= hash << 3;
    hash = hash.wrapping_add(hash >> 5);
    hash ^= hash << 4;
    hash = hash.wrapping_add(hash >> 17);
    hash ^= hash << 25;
    hash.wrapping_add(hash >> 6)
}

/// `resolveDisplay`: the bound id first, then a unique label and size
/// match, and never any other screen. A display already claimed by another
/// output is not used twice.
pub fn resolve<'a>(
    binding: &DisplayBinding,
    displays: &'a [DisplayInfo],
    claimed: &HashSet<u32>,
) -> Option<&'a DisplayInfo> {
    let id = binding.display_id?;
    if let Some(display) = displays.iter().find(|d| d.id == id) {
        return (!claimed.contains(&display.id)).then_some(display);
    }
    let label = binding.label.trim();
    let size = binding.size?;
    if label.is_empty() {
        return None;
    }
    let mut matches = displays
        .iter()
        .filter(|d| d.label == label && d.logical_size() == size);
    let first = matches.next()?;
    (matches.next().is_none() && !claimed.contains(&first.id)).then_some(first)
}

/// The rehearsal branch of `createDisplayWindow`: half width, centred, at
/// the frame's shape.
pub fn rehearsal_bounds(display: &Rect, frame_height: u32) -> Rect {
    let width = (f64::from(display.width) / 2.0).round() as u32;
    let height = (f64::from(width) * f64::from(frame_height) / 1920.0).round() as u32;
    Rect {
        x: display.x + ((f64::from(display.width) - f64::from(width)) / 2.0).round() as i32,
        y: display.y + ((f64::from(display.height) - f64::from(height)) / 2.0).round() as i32,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u32, label: &str) -> DisplayInfo {
        DisplayInfo {
            id,
            label: label.into(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale_factor: 1.0,
            native_id: id as u64,
            primary: id == 1,
            hosts_main_window: false,
            refresh_millihertz: Some(60_000),
        }
    }

    fn binding(id: Option<u32>, label: &str) -> DisplayBinding {
        DisplayBinding {
            display_id: id,
            label: label.into(),
            size: Some((1920, 1080)),
        }
    }

    #[test]
    fn windows_display_identity_matches_chromium_c_vectors() {
        // Evaluated with the unmodified Chromium 130 C implementation.
        for (text, expected) in [
            ("", 0),
            ("a", 291415938),
            ("ab", 1366002500),
            ("abc", 3535673738),
            ("abcd", 3671636187),
            (r"\\.\DISPLAY1", 2528732444),
            ("1234/-42/7", 878747013),
            ("Aé", 226779832),
        ] {
            assert_eq!(persistent_hash(text.as_bytes()), expected, "{text:?}");
        }
    }

    #[test]
    fn resolve_never_guesses() {
        let none = HashSet::new();
        let displays = [display(1, "A"), display(2, "B")];
        assert_eq!(
            resolve(&binding(Some(2), "x"), &displays, &none)
                .unwrap()
                .id,
            2
        );
        assert!(resolve(&binding(None, "A"), &displays, &none).is_none());
        assert_eq!(
            resolve(&binding(Some(9), "A"), &displays, &none)
                .unwrap()
                .id,
            1
        );
        assert!(resolve(&binding(Some(9), "  "), &displays, &none).is_none());
        let twins = [display(1, "A"), display(2, "A")];
        assert!(resolve(&binding(Some(9), "A"), &twins, &none).is_none());
        let claimed = HashSet::from([1]);
        assert!(resolve(&binding(Some(1), "A"), &displays, &claimed).is_none());
        let unsized_binding = DisplayBinding {
            size: None,
            ..binding(Some(9), "A")
        };
        assert!(resolve(&unsized_binding, &displays, &none).is_none());
    }

    #[test]
    fn rematch_uses_electron_logical_size_on_scaled_displays() {
        let mut retina = display(7, "A");
        retina.bounds.width = 3840;
        retina.bounds.height = 2160;
        retina.scale_factor = 2.;
        assert_eq!(
            resolve(&binding(Some(9), "A"), &[retina], &HashSet::new()).map(|d| d.id),
            Some(7)
        );
    }

    #[test]
    fn rehearsal_is_half_width_and_centred() {
        let display = Rect {
            x: 100,
            y: 0,
            width: 1920,
            height: 1080,
        };
        assert_eq!(
            rehearsal_bounds(&display, 1080),
            Rect {
                x: 580,
                y: 270,
                width: 960,
                height: 540
            }
        );
    }
}

/// The 250 ms quiet period is measured since the last distinct topology.
/// Removal safety uses the raw scan immediately; only reopening is debounced.
#[derive(Default)]
pub struct Debounce {
    pending: Option<(Vec<DisplayInfo>, std::time::Instant)>,
}
impl Debounce {
    pub fn observe(
        &mut self,
        now: std::time::Instant,
        current: &[DisplayInfo],
        observed: Vec<DisplayInfo>,
    ) {
        if self
            .pending
            .as_ref()
            .is_some_and(|(last, _)| last == &observed)
        {
            return;
        }
        if current == observed {
            self.pending = None;
            return;
        }
        self.pending = Some((observed, now + std::time::Duration::from_millis(250)));
    }
    pub fn take_ready(&mut self, now: std::time::Instant) -> Option<Vec<DisplayInfo>> {
        if self
            .pending
            .as_ref()
            .is_some_and(|(_, deadline)| now >= *deadline)
        {
            self.pending.take().map(|(displays, _)| displays)
        } else {
            None
        }
    }
    pub fn deadline(&self) -> Option<std::time::Instant> {
        self.pending.as_ref().map(|(_, at)| *at)
    }
}

#[cfg(test)]
mod debounce_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn a_burst_waits_for_250_ms_of_quiet_and_a_reverted_change_is_cancelled() {
        let now = Instant::now();
        let mut debounce = Debounce::default();
        let empty = Vec::new();
        // The old/current list does not need a real monitor to test deadlines.
        let display = DisplayInfo {
            id: 1,
            native_id: 1,
            primary: true,
            hosts_main_window: false,
            label: "A".into(),
            bounds: Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            scale_factor: 1.,
            refresh_millihertz: Some(60_000),
        };
        debounce.observe(now, &empty, vec![display.clone()]);
        debounce.observe(
            now + Duration::from_millis(200),
            &empty,
            vec![display.clone()],
        );
        assert!(
            debounce
                .take_ready(now + Duration::from_millis(249))
                .is_none()
        );
        assert!(
            debounce
                .take_ready(now + Duration::from_millis(250))
                .is_some(),
            "identical polls do not extend the quiet period"
        );
        let current = vec![display];
        debounce.observe(now, &current, vec![]);
        debounce.observe(now + Duration::from_millis(200), &current, current.clone());
        assert!(debounce.take_ready(now + Duration::from_secs(1)).is_none());
    }
}
