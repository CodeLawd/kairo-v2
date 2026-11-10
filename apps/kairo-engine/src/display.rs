//! Displays: listing them and choosing the one an output is bound to.

use kairo_engine_proto::{DisplayBinding, DisplayInfo, Rect};
use std::collections::HashSet;
use winit::event_loop::ActiveEventLoop;

pub fn scan(event_loop: &ActiveEventLoop) -> Vec<DisplayInfo> {
    event_loop
        .available_monitors()
        .map(|monitor| {
            #[cfg(target_os = "macos")]
            let id = winit::platform::macos::MonitorHandleExtMacOS::native_id(&monitor);
            #[cfg(not(target_os = "macos"))]
            let id = {
                // No stable native id through winit here; prompt 060 maps
                // display identity to stored Electron ids on each platform.
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                monitor.name().hash(&mut hasher);
                monitor.position().x.hash(&mut hasher);
                monitor.position().y.hash(&mut hasher);
                hasher.finish() as u32
            };
            let position = monitor.position();
            let size = monitor.size();
            DisplayInfo {
                id,
                label: monitor.name().unwrap_or_default(),
                bounds: Rect {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
                scale_factor: monitor.scale_factor(),
            }
        })
        .collect()
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
        .filter(|d| d.label == label && (d.bounds.width, d.bounds.height) == size);
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
