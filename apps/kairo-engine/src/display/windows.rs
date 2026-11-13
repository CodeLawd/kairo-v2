//! Chromium 130 display identity: adapter LUID + target id, PersistentHash.
use super::persistent_hash;
use windows::Win32::{Devices::Display::*, Foundation::*, Graphics::Gdi::*};
use winit::{monitor::MonitorHandle, platform::windows::MonitorHandleExtWindows};

fn text(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
}

pub(super) fn metadata(monitor: &MonitorHandle) -> Option<(u32, u64, String)> {
    let handle = monitor.hmonitor();
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: initialized buffers and an OS monitor owned by winit.
    unsafe {
        if !GetMonitorInfoW(HMONITOR(handle as _), &mut info.monitorInfo).as_bool() {
            return None;
        }
        let device = text(&info.szDevice);
        let mut identity = device.clone();
        let mut label = String::new();
        // A hotplug may resize both arrays between calls. Bound retries and
        // allocation; Chromium falls back to the GDI device name on failure.
        for _ in 0..4 {
            let (mut paths, mut modes) = (0, 0);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes)
                != ERROR_SUCCESS
                || paths > 4096
                || modes > 16384
            {
                break;
            }
            let mut p = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
            let mut m = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
            let result = QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut paths,
                p.as_mut_ptr(),
                &mut modes,
                m.as_mut_ptr(),
                None,
            );
            if result == ERROR_INSUFFICIENT_BUFFER {
                continue;
            }
            if result != ERROR_SUCCESS {
                break;
            }
            for path in p.iter().take(paths as usize) {
                let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                source.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: std::mem::size_of_val(&source) as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                };
                if DisplayConfigGetDeviceInfo(&mut source.header) != 0
                    || text(&source.viewGdiDeviceName) != device
                {
                    continue;
                }
                let target = path.targetInfo;
                identity = format!(
                    "{}/{}/{}",
                    target.adapterId.LowPart, target.adapterId.HighPart, target.id
                );
                let mut name = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
                name.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                    size: std::mem::size_of_val(&name) as u32,
                    adapterId: target.adapterId,
                    id: target.id,
                };
                if DisplayConfigGetDeviceInfo(&mut name.header) == 0 {
                    label = text(&name.monitorFriendlyDeviceName);
                }
                break;
            }
            break;
        }
        Some((persistent_hash(identity.as_bytes()), handle as u64, label))
    }
}
