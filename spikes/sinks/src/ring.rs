//! Writer for the POSIX frame ring in `native/ring.h`.

use std::ffi::CString;
use std::io;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

pub const MAGIC: u32 = 0x3146_474B;
const VERSION: u32 = 1;
pub const SLOTS: usize = 3;
const PAGE: usize = 16384;

#[repr(C)]
pub struct Slot {
    seq: AtomicU64,
    reserved: [u64; 7],
}

#[repr(C)]
pub struct Header {
    magic: u32,
    version: u32,
    width: u32,
    height: u32,
    slot_count: u32,
    slot_bytes: u32,
    pub published: AtomicU64,
    pub latest: AtomicU32,
    reserved: u32,
    pub slots: [Slot; SLOTS],
}

pub fn frame_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 2
}

pub fn slot_bytes(width: u32, height: u32) -> usize {
    frame_bytes(width, height).div_ceil(PAGE) * PAGE
}

fn valid_raster(width: u32, height: u32) -> bool {
    width >= 2 && height >= 1 && width.is_multiple_of(2) && width <= 4096 && height <= 2160
}

pub struct Ring {
    name: CString,
    base: *mut u8,
    total: usize,
    slot_bytes: usize,
    frame_bytes: usize,
}

impl Ring {
    pub fn create(name: &str, width: u32, height: u32) -> Result<Self, String> {
        if name.is_empty() || name.len() > 30 || !name.starts_with('/') {
            return Err("Ring name must start with / and be at most 30 characters".into());
        }
        if !valid_raster(width, height) {
            return Err("Invalid ring raster".into());
        }
        let c_name = CString::new(name).map_err(|e| e.to_string())?;
        let slot_bytes = slot_bytes(width, height);
        let total = PAGE + SLOTS * slot_bytes;
        // SAFETY: POSIX calls on a validated name; every failure path unlinks.
        unsafe {
            // A stale ring from a crashed run must not be reused.
            libc::shm_unlink(c_name.as_ptr());
            let fd = libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600 as libc::c_uint,
            );
            if fd < 0 {
                return Err(format!("shm_open failed: {}", io::Error::last_os_error()));
            }
            if libc::ftruncate(fd, total as libc::off_t) != 0 {
                let why = io::Error::last_os_error();
                libc::close(fd);
                libc::shm_unlink(c_name.as_ptr());
                return Err(format!("ftruncate failed: {why}"));
            }
            let base = libc::mmap(
                std::ptr::null_mut(),
                total,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            );
            libc::close(fd);
            if base == libc::MAP_FAILED {
                libc::shm_unlink(c_name.as_ptr());
                return Err("mmap failed".into());
            }
            let header = base as *mut Header;
            (*header).magic = MAGIC;
            (*header).version = VERSION;
            (*header).width = width;
            (*header).height = height;
            (*header).slot_count = SLOTS as u32;
            (*header).slot_bytes = slot_bytes as u32;
            fence(Ordering::Release);
            Ok(Self {
                name: c_name,
                base: base.cast(),
                total,
                slot_bytes,
                frame_bytes: frame_bytes(width, height),
            })
        }
    }

    fn header(&self) -> &Header {
        // SAFETY: `base` maps at least one header page for the ring's lifetime.
        unsafe { &*(self.base as *const Header) }
    }

    /// Writes one frame using the `beginWrite` and `endWrite` protocol.
    pub fn write(&self, fill: impl FnOnce(&mut [u8])) {
        let header = self.header();
        let index = (header.latest.load(Ordering::Acquire) as usize + 1) % SLOTS;
        header.slots[index].seq.fetch_add(1, Ordering::AcqRel);
        // SAFETY: the slot is inside the mapping, and odd `seq` keeps readers out.
        let data = unsafe {
            std::slice::from_raw_parts_mut(
                self.base.add(PAGE + index * self.slot_bytes),
                self.frame_bytes,
            )
        };
        fill(data);
        header.slots[index].seq.fetch_add(1, Ordering::Release);
        header.latest.store(index as u32, Ordering::Release);
        header.published.fetch_add(1, Ordering::Release);
    }
}

impl Drop for Ring {
    fn drop(&mut self) {
        // SAFETY: created in `create` and released exactly once. ENOENT is
        // fine, because the helper may have unlinked it.
        unsafe {
            libc::shm_unlink(self.name.as_ptr());
            libc::munmap(self.base.cast(), self.total);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_names_and_rasters_are_rejected() {
        assert!(Ring::create("no-slash", 2, 2).is_err());
        assert!(Ring::create("", 2, 2).is_err());
        assert!(Ring::create(&format!("/{}", "x".repeat(30)), 2, 2).is_err());
        assert!(Ring::create("/k-bad", 3, 2).is_err());
        assert!(Ring::create("/k-bad", 4098, 2).is_err());
    }

    #[test]
    fn slot_size_rounds_to_whole_pages() {
        assert_eq!(slot_bytes(1920, 1080) % PAGE, 0);
        assert!(slot_bytes(1920, 1080) >= frame_bytes(1920, 1080));
    }

    #[test]
    fn stale_ring_is_replaced_and_slots_rotate() {
        let name = format!("/kt-{}", std::process::id());
        drop(Ring::create(&name, 4, 2).unwrap());
        let first = Ring::create(&name, 4, 2).unwrap();
        std::mem::forget(first);
        let ring = Ring::create(&name, 4, 2).unwrap();
        ring.write(|f| f.fill(9));
        ring.write(|f| f.fill(8));
        assert_eq!(ring.header().published.load(Ordering::Acquire), 2);
        assert_eq!(ring.header().latest.load(Ordering::Acquire), 2);
        assert_eq!(ring.header().slots[1].seq.load(Ordering::Acquire), 2);
    }
}
