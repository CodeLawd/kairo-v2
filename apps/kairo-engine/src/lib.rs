//! Main-thread browser source used by the engine and its native verification rig.
#[cfg(target_os = "macos")]
#[allow(dead_code)] // Only the source-window placement helper is needed here.
mod mac;
pub mod web;

/// Stable FNV-1a ids, namespaced by the caller (video, elements, web).
pub fn source_id(key: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}
