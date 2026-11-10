//! The shared-memory frame ring of `native/gpu-frames/ring.h`, byte for byte.
//!
//! One 16 KiB header page, then three slots, each a whole number of 16 KiB
//! pages. A slot's sequence number is a seqlock: odd while it is written,
//! even once complete. The DeckLink helper reads this ring unchanged.

use std::ffi::CString;
use std::io;
use std::mem::offset_of;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering, fence};

pub const MAGIC: u32 = 0x3146_474B; // "KGF1"
pub const VERSION: u32 = 1;
pub const SLOTS: u32 = 3;
/// Page size both Apple Silicon (16 KiB) and Intel (4 KiB) Macs divide.
pub const PAGE: usize = 16384;
pub const HEADER_BYTES: usize = PAGE;

#[repr(C)]
pub struct Slot {
    pub seq: AtomicU64,
    reserved: [u64; 7],
}

#[repr(C)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub slot_count: u32,
    pub slot_bytes: u32,
    /// Bumped once per completed frame.
    pub published: AtomicU64,
    /// Index of the newest complete slot.
    pub latest: AtomicU32,
    reserved: u32,
    pub slots: [Slot; SLOTS as usize],
}

// The C++ layout, checked at compile time.
const _: () = {
    assert!(size_of::<Slot>() == 64);
    assert!(offset_of!(Header, magic) == 0);
    assert!(offset_of!(Header, version) == 4);
    assert!(offset_of!(Header, width) == 8);
    assert!(offset_of!(Header, height) == 12);
    assert!(offset_of!(Header, slot_count) == 16);
    assert!(offset_of!(Header, slot_bytes) == 20);
    assert!(offset_of!(Header, published) == 24);
    assert!(offset_of!(Header, latest) == 32);
    assert!(offset_of!(Header, slots) == 40);
    assert!(size_of::<Header>() == 232);
    assert!(size_of::<Header>() <= HEADER_BYTES);
};

/// Bytes in one UYVY frame (two bytes per pixel).
pub fn frame_bytes(width: u32, height: u32) -> usize {
    width as usize * height as usize * 2
}

pub fn slot_bytes_for(width: u32, height: u32) -> usize {
    frame_bytes(width, height).div_ceil(PAGE) * PAGE
}

pub fn total_bytes(slot_bytes: usize) -> usize {
    HEADER_BYTES + SLOTS as usize * slot_bytes
}

/// `validRaster` in `gpu_frames.mm`.
pub fn valid_raster(width: u32, height: u32) -> bool {
    width >= 2 && height >= 1 && width.is_multiple_of(2) && width <= 4096 && height <= 2160
}

/// `compatible`: a ring this build can read at the given raster.
pub fn compatible(header: &Header, width: u32, height: u32) -> bool {
    header.magic == MAGIC
        && header.version == VERSION
        && header.slot_count == SLOTS
        && header.width == width
        && header.height == height
        && header.slot_bytes as usize == slot_bytes_for(width, height)
}

fn shm_name(name: &str) -> io::Result<CString> {
    if name.is_empty() || name.len() > 30 || !name.starts_with('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Ring name must start with / and be at most 30 characters",
        ));
    }
    CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "ring name has a NUL"))
}

struct Mapping {
    base: *mut u8,
    bytes: usize,
}

// SAFETY: the mapping is plain shared memory; access goes through atomics
// for the header and the seqlock protocol for slot data.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    fn header(&self) -> &Header {
        // SAFETY: every mapping holds at least the header page, page aligned.
        unsafe { &*(self.base as *const Header) }
    }

    fn slot(&self, index: u32, slot_bytes: usize) -> *mut u8 {
        // SAFETY: index < SLOTS, so the slot lies inside the mapping.
        unsafe { self.base.add(HEADER_BYTES + index as usize * slot_bytes) }
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: mapped once in `map`, unmapped once here.
        unsafe { libc::munmap(self.base.cast(), self.bytes) };
    }
}

fn map(fd: libc::c_int, bytes: usize) -> io::Result<Mapping> {
    // SAFETY: a plain shared mapping of a descriptor we own.
    let base = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            bytes,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_SHARED,
            fd,
            0,
        )
    };
    if base == libc::MAP_FAILED {
        return Err(io::Error::other(format!(
            "mmap failed: {}",
            io::Error::last_os_error()
        )));
    }
    Ok(Mapping {
        base: base.cast(),
        bytes,
    })
}

/// The writer side. Owns the name: dropping it unlinks the ring.
pub struct RingWriter {
    name: CString,
    map: Mapping,
    width: u32,
    height: u32,
    slot_bytes: usize,
    /// Slots with a write in flight (bit per slot). Process-local.
    busy: AtomicU8,
}

impl RingWriter {
    /// `createRing`. A stale ring from a crashed run is never reused.
    pub fn create(name: &str, width: u32, height: u32) -> io::Result<Self> {
        let c_name = shm_name(name)?;
        if !valid_raster(width, height) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid ring raster",
            ));
        }
        let slot_bytes = slot_bytes_for(width, height);
        let bytes = total_bytes(slot_bytes);
        // SAFETY: POSIX calls on a validated name; every failure unlinks.
        let fd = unsafe {
            libc::shm_unlink(c_name.as_ptr());
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            return Err(io::Error::other(format!(
                "shm_open failed: {}",
                io::Error::last_os_error()
            )));
        }
        // SAFETY: fd is ours until closed below.
        let sized = unsafe { libc::ftruncate(fd, bytes as libc::off_t) };
        let mapped = if sized == 0 {
            map(fd, bytes)
        } else {
            Err(io::Error::other(format!(
                "ftruncate failed: {}",
                io::Error::last_os_error()
            )))
        };
        // SAFETY: closing our descriptor; the mapping keeps the memory.
        unsafe { libc::close(fd) };
        let map = match mapped {
            Ok(map) => map,
            Err(error) => {
                // SAFETY: undo the name we created.
                unsafe { libc::shm_unlink(c_name.as_ptr()) };
                return Err(error);
            }
        };
        // `initHeader`. The memory is fresh and nobody else has it mapped
        // as a ring until the name is handed out.
        let header = map.base as *mut Header;
        // SAFETY: header page is inside the new mapping.
        unsafe {
            (*header).magic = MAGIC;
            (*header).version = VERSION;
            (*header).width = width;
            (*header).height = height;
            (*header).slot_count = SLOTS;
            (*header).slot_bytes = slot_bytes as u32;
        }
        let header = map.header();
        header.published.store(0, Ordering::Relaxed);
        header.latest.store(0, Ordering::Relaxed);
        for slot in &header.slots {
            slot.seq.store(0, Ordering::Relaxed);
        }
        fence(Ordering::Release);
        Ok(Self {
            name: c_name,
            map,
            width,
            height,
            slot_bytes,
            busy: AtomicU8::new(0),
        })
    }

    pub fn header(&self) -> &Header {
        self.map.header()
    }

    pub fn raster(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// `takeSlot` plus `beginWrite`: a free slot that is never the newest
    /// complete one, which readers may be copying. `None` when every
    /// candidate is busy: the frame is dropped, never queued.
    pub fn begin_write(&self) -> Option<SlotWrite<'_>> {
        let header = self.header();
        let avoid = header.latest.load(Ordering::Acquire);
        let any_published = header.published.load(Ordering::Acquire) > 0;
        for index in 0..SLOTS {
            if any_published && index == avoid {
                continue;
            }
            let bit = 1u8 << index;
            if self.busy.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
                continue;
            }
            header.slots[index as usize]
                .seq
                .fetch_add(1, Ordering::AcqRel);
            return Some(SlotWrite {
                ring: self,
                index,
                finished: false,
            });
        }
        None
    }

    /// Writes one frame synchronously. False when it was dropped.
    pub fn write(&self, fill: impl FnOnce(&mut [u8])) -> bool {
        let Some(mut slot) = self.begin_write() else {
            return false;
        };
        fill(slot.data());
        slot.publish();
        true
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        // ENOENT is fine: the helper may have unlinked it.
        // SAFETY: the name we created.
        unsafe { libc::shm_unlink(self.name.as_ptr()) };
    }
}

/// One slot being written. Dropping it without `publish` returns the
/// sequence to even without publishing, as a failed GPU write does.
pub struct SlotWrite<'a> {
    ring: &'a RingWriter,
    index: u32,
    finished: bool,
}

impl SlotWrite<'_> {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn data(&mut self) -> &mut [u8] {
        let length = frame_bytes(self.ring.width, self.ring.height);
        // SAFETY: the slot is inside the mapping, our busy bit excludes other
        // writers, and its odd sequence keeps readers from trusting it.
        unsafe {
            std::slice::from_raw_parts_mut(
                self.ring.map.slot(self.index, self.ring.slot_bytes),
                length,
            )
        }
    }

    /// `endWrite`: the newest complete frame.
    pub fn publish(mut self) {
        let header = self.ring.header();
        header.slots[self.index as usize]
            .seq
            .fetch_add(1, Ordering::Release);
        header.latest.store(self.index, Ordering::Release);
        header.published.fetch_add(1, Ordering::Release);
        self.finished = true;
    }
}

impl Drop for SlotWrite<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.ring.header().slots[self.index as usize]
                .seq
                .fetch_add(1, Ordering::Release);
        }
        self.ring
            .busy
            .fetch_and(!(1u8 << self.index), Ordering::AcqRel);
    }
}

/// The reader side (the helper's role), for tests and diagnostics.
pub struct RingReader {
    map: Mapping,
    width: u32,
    height: u32,
    slot_bytes: usize,
}

impl RingReader {
    pub fn open(name: &str, width: u32, height: u32) -> io::Result<Self> {
        let c_name = shm_name(name)?;
        // SAFETY: opening an existing name read-write, as the helper does.
        let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat on our descriptor into owned storage.
        let size = if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } == 0 {
            // SAFETY: fstat succeeded.
            unsafe { stat.assume_init() }.st_size as usize
        } else {
            0
        };
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "incompatible frame ring");
        let slot_bytes = slot_bytes_for(width, height);
        let mapped = if size == total_bytes(slot_bytes) {
            map(fd, size)
        } else {
            Err(invalid())
        };
        // SAFETY: our descriptor.
        unsafe { libc::close(fd) };
        let map = mapped?;
        fence(Ordering::Acquire);
        if !compatible(map.header(), width, height) {
            return Err(invalid());
        }
        Ok(Self {
            map,
            width,
            height,
            slot_bytes,
        })
    }

    pub fn header(&self) -> &Header {
        self.map.header()
    }

    /// `readNewest`: copies the newest complete frame into `out` when one
    /// arrived since `last_published`. False when nothing is new or the slot
    /// was rewritten during the copy; the caller keeps what it had.
    pub fn read_newest(&self, last_published: &mut u64, out: &mut [u8]) -> bool {
        self.read_newest_with(last_published, out, || {})
    }

    /// As `read_newest`; `during_copy` runs between the copy and the check.
    pub fn read_newest_with(
        &self,
        last_published: &mut u64,
        out: &mut [u8],
        during_copy: impl FnOnce(),
    ) -> bool {
        let header = self.header();
        let published = header.published.load(Ordering::Acquire);
        if published == *last_published {
            return false;
        }
        let index = header.latest.load(Ordering::Acquire);
        if index >= SLOTS {
            return false;
        }
        let sequence = &header.slots[index as usize].seq;
        let before = sequence.load(Ordering::Acquire);
        if before & 1 == 1 {
            return false;
        }
        let length = frame_bytes(self.width, self.height).min(out.len());
        // SAFETY: reading inside the mapping; a torn copy is rejected below.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.map.slot(index, self.slot_bytes),
                out.as_mut_ptr(),
                length,
            );
        }
        during_copy();
        fence(Ordering::Acquire);
        if sequence.load(Ordering::Relaxed) != before {
            return false;
        }
        *last_published = published;
        true
    }
}
