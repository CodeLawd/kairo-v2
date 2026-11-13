//! Operator preview frames (prompt 066): a memory-mapped file holding three
//! seqlocked frames, written by the engine and read by the UI process.
//!
//! The UI process creates the file (owner-only on Unix), names it in
//! `Command::SetPreview`, and removes it when the last view lets go. The
//! engine opens it and publishes downscaled frames into it. Only the newest
//! frame matters: the writer overwrites the oldest slot, and a reader that
//! falls behind reads the newest complete one, never a backlog.
//!
//! Pixels are straight-alpha BGRA, rows of `width × 4` bytes with no
//! padding, which is what GPUI's `RenderImage` draws. A frame's size may be
//! anything up to the capacity the file was made with, so a view that
//! resizes, or an output whose shape changes, needs no new file.
//!
//! Layout (little-endian):
//!
//! | Offset | Field |
//! |---|---|
//! | 0 | magic `KPRV` |
//! | 4 | layout version |
//! | 8, 12 | capacity width, height |
//! | 16 | slot holding the newest frame |
//! | 24 | frames published (u64) |
//! | 32 + 16 i | slot i: sequence (odd while writing), width, height |
//! | 128 + i × capacity | slot i's pixels |

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

pub const MAGIC: u32 = u32::from_le_bytes(*b"KPRV");
pub const LAYOUT_VERSION: u32 = 1;
pub const SLOTS: usize = 3;
/// The largest frame a preview map holds. Previews are thumbnails of the
/// program; nothing in the operator window is wider than this.
pub const MAX_WIDTH: u32 = 1920;
pub const MAX_HEIGHT: u32 = 1080;

const HEADER: usize = 128;
const LATEST: usize = 16;
const FRAMES: usize = 24;
const SLOT_META: usize = 32;
const SLOT_META_LEN: usize = 16;

/// One frame read from the map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewFrame {
    /// Frames published when this one was read; grows by one per frame.
    pub number: u64,
    pub width: u32,
    pub height: u32,
    /// Straight-alpha BGRA, `width × height × 4` bytes.
    pub pixels: Vec<u8>,
}

/// The shared mapping, from either side.
pub struct PreviewMap {
    map: memmap2::MmapMut,
    capacity: (u32, u32),
    stride: usize,
    /// Set on the side that created the file, which removes it.
    owned: Option<PathBuf>,
}

fn invalid(why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("invalid preview map: {why}"),
    )
}

impl PreviewMap {
    /// Makes a new map at `path` for frames up to `width × height`.
    pub fn create(path: &Path, width: u32, height: u32) -> io::Result<Self> {
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(invalid(&format!("capacity {width}×{height}")));
        }
        let stride = width as usize * height as usize * 4;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.set_len((HEADER + stride * SLOTS) as u64)?;
        let mut map = map(&file)?;
        map[8..12].copy_from_slice(&width.to_le_bytes());
        map[12..16].copy_from_slice(&height.to_le_bytes());
        map[4..8].copy_from_slice(&LAYOUT_VERSION.to_le_bytes());
        // The magic last: a map is valid only once its header is.
        map[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        map.flush()?;
        Ok(Self {
            map,
            capacity: (width, height),
            stride,
            owned: Some(path.to_owned()),
        })
    }

    /// Opens a map another process made, checking its header and length.
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let map = map(&file)?;
        if map.len() < HEADER {
            return Err(invalid("too short"));
        }
        let word = |at: usize| u32::from_le_bytes(map[at..at + 4].try_into().expect("4 bytes"));
        if word(0) != MAGIC {
            return Err(invalid("bad magic"));
        }
        if word(4) != LAYOUT_VERSION {
            return Err(invalid(&format!("layout version {}", word(4))));
        }
        let (width, height) = (word(8), word(12));
        if width == 0 || height == 0 || width > MAX_WIDTH || height > MAX_HEIGHT {
            return Err(invalid(&format!("capacity {width}×{height}")));
        }
        let stride = width as usize * height as usize * 4;
        if map.len() != HEADER + stride * SLOTS {
            return Err(invalid("length does not match its capacity"));
        }
        Ok(Self {
            map,
            capacity: (width, height),
            stride,
            owned: None,
        })
    }

    pub fn capacity(&self) -> (u32, u32) {
        self.capacity
    }

    fn word(&self, offset: usize) -> &AtomicU32 {
        debug_assert!(offset % 4 == 0 && offset + 4 <= HEADER);
        // SAFETY: an aligned header word inside the mapping; the mapping is
        // page-aligned and outlives the reference.
        unsafe { &*(self.map.as_ptr().add(offset) as *const AtomicU32) }
    }

    fn frames(&self) -> &AtomicU64 {
        // SAFETY: FRAMES is 8-aligned in a page-aligned mapping.
        unsafe { &*(self.map.as_ptr().add(FRAMES) as *const AtomicU64) }
    }

    fn slot(&self, index: usize) -> *mut u8 {
        debug_assert!(index < SLOTS);
        // SAFETY: index < SLOTS, so the offset stays inside the mapping.
        unsafe { (self.map.as_ptr() as *mut u8).add(HEADER + index * self.stride) }
    }

    /// Publishes one frame. Fails, publishing nothing, when the frame does
    /// not fit the map or `pixels` is not `width × height × 4` bytes.
    pub fn publish(&self, width: u32, height: u32, pixels: &[u8]) -> io::Result<()> {
        let len = width as usize * height as usize * 4;
        if width == 0
            || height == 0
            || width > self.capacity.0
            || height > self.capacity.1
            || pixels.len() != len
        {
            return Err(invalid(&format!(
                "frame {width}×{height} ({} bytes) in a {}×{} map",
                pixels.len(),
                self.capacity.0,
                self.capacity.1
            )));
        }
        let latest = self.word(LATEST).load(Ordering::Acquire) as usize % SLOTS;
        let index = (latest + 1) % SLOTS;
        let meta = SLOT_META + index * SLOT_META_LEN;
        let sequence = self.word(meta);
        // A writer killed mid-frame leaves an odd value: make it odd regardless.
        let odd = (sequence.load(Ordering::Relaxed) | 1).wrapping_add(2);
        sequence.store(odd, Ordering::Relaxed);
        fence(Ordering::Release);
        self.word(meta + 4).store(width, Ordering::Relaxed);
        self.word(meta + 8).store(height, Ordering::Relaxed);
        // SAFETY: `len` fits the slot (checked above); a reader detects an
        // overlapping copy by the sequence and discards it.
        unsafe { std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.slot(index), len) };
        sequence.store(odd.wrapping_add(1), Ordering::Release);
        self.word(LATEST).store(index as u32, Ordering::Release);
        self.frames().fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    /// Frames published so far.
    pub fn published(&self) -> u64 {
        self.frames().load(Ordering::Acquire)
    }

    /// The newest complete frame, if one was published after frame
    /// number `seen`. `None` also when the newest slot is being written:
    /// the next look finds it done.
    pub fn read_new(&self, seen: u64) -> Option<PreviewFrame> {
        let mut buffer = Vec::new();
        self.read_into(seen, &mut buffer)
            .map(|(number, width, height)| PreviewFrame {
                number,
                width,
                height,
                pixels: buffer,
            })
    }

    /// As [`PreviewMap::read_new`], into a caller's buffer (resized to the frame).
    pub fn read_into(&self, seen: u64, buffer: &mut Vec<u8>) -> Option<(u64, u32, u32)> {
        let number = self.frames().load(Ordering::Acquire);
        if number == seen || number == 0 {
            return None;
        }
        let index = self.word(LATEST).load(Ordering::Acquire) as usize % SLOTS;
        let meta = SLOT_META + index * SLOT_META_LEN;
        let before = self.word(meta).load(Ordering::Acquire);
        if before & 1 == 1 {
            return None;
        }
        let width = self.word(meta + 4).load(Ordering::Relaxed);
        let height = self.word(meta + 8).load(Ordering::Relaxed);
        if width == 0 || height == 0 || width > self.capacity.0 || height > self.capacity.1 {
            return None;
        }
        let len = width as usize * height as usize * 4;
        buffer.resize(len, 0);
        // SAFETY: `len` fits the slot; the copy may race a writer, which the
        // sequence check below detects.
        unsafe { std::ptr::copy_nonoverlapping(self.slot(index), buffer.as_mut_ptr(), len) };
        fence(Ordering::Acquire);
        if self.word(meta).load(Ordering::Relaxed) != before {
            return None;
        }
        Some((number, width, height))
    }
}

impl Drop for PreviewMap {
    fn drop(&mut self) {
        if let Some(path) = self.owned.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn map(file: &File) -> io::Result<memmap2::MmapMut> {
    // SAFETY: the file is private to the UI process and its engine; both
    // sides only touch it through the seqlock above.
    unsafe { memmap2::MmapMut::map_mut(file) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kairo-preview-test-{name}-{}", std::process::id()))
    }

    #[test]
    fn frames_go_through_and_only_the_newest_is_read() {
        let p = path("newest");
        let _ = std::fs::remove_file(&p);
        let reader = PreviewMap::create(&p, 4, 2).unwrap();
        assert!(reader.read_new(0).is_none(), "nothing published yet");
        let writer = PreviewMap::open(&p).unwrap();
        writer.publish(4, 2, &[1; 32]).unwrap();
        let first = reader.read_new(0).unwrap();
        assert_eq!((first.number, first.width, first.height), (1, 4, 2));
        assert_eq!(first.pixels, vec![1; 32]);
        assert!(reader.read_new(1).is_none(), "nothing new");
        // Five frames while the reader looked away: it gets the last only.
        for n in 2..=6u8 {
            writer.publish(2, 1, &[n; 8]).unwrap();
        }
        let newest = reader.read_new(1).unwrap();
        assert_eq!((newest.number, newest.width, newest.height), (6, 2, 1));
        assert_eq!(newest.pixels, vec![6; 8]);
        drop(writer);
        drop(reader);
        assert!(!p.exists(), "the creator removes the file");
    }

    #[test]
    fn a_frame_being_written_or_torn_is_not_read() {
        let p = path("torn");
        let _ = std::fs::remove_file(&p);
        let reader = PreviewMap::create(&p, 2, 1).unwrap();
        let writer = PreviewMap::open(&p).unwrap();
        writer.publish(2, 1, &[1; 8]).unwrap();
        // The writer dies while writing the newest slot.
        let latest = writer.word(LATEST).load(Ordering::Acquire) as usize;
        writer
            .word(SLOT_META + latest * SLOT_META_LEN)
            .fetch_or(1, Ordering::SeqCst);
        writer.frames().fetch_add(1, Ordering::SeqCst);
        assert!(reader.read_new(0).is_none());
        // A relaunched engine reopens the map and goes on.
        let relaunched = PreviewMap::open(&p).unwrap();
        relaunched.publish(2, 1, &[2; 8]).unwrap();
        assert_eq!(reader.read_new(2).unwrap().pixels, vec![2; 8]);
    }

    #[test]
    fn bad_maps_and_frames_are_refused() {
        let p = path("bad");
        let _ = std::fs::remove_file(&p);
        std::fs::write(&p, [0u8; 8]).unwrap();
        assert!(PreviewMap::open(&p).is_err(), "too short");
        std::fs::write(&p, [0u8; 256]).unwrap();
        assert!(PreviewMap::open(&p).is_err(), "no magic");
        assert!(
            PreviewMap::create(&p, 4, 4).is_err(),
            "never over an existing file"
        );
        std::fs::remove_file(&p).unwrap();
        assert!(PreviewMap::open(&p).is_err(), "missing");
        assert!(PreviewMap::create(&p, 0, 4).is_err());
        assert!(PreviewMap::create(&p, MAX_WIDTH + 1, 4).is_err());

        let map = PreviewMap::create(&p, 4, 2).unwrap();
        assert!(map.publish(8, 2, &[0; 64]).is_err(), "wider than the map");
        assert!(map.publish(4, 2, &[0; 31]).is_err(), "short pixels");
        assert!(map.publish(0, 0, &[]).is_err());
        assert_eq!(map.published(), 0);
        drop(map);
        // A map whose length disagrees with its header is refused.
        let q = path("bad-len");
        let _ = std::fs::remove_file(&q);
        let made = PreviewMap::create(&q, 4, 2).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&q)
            .unwrap()
            .set_len(HEADER as u64 + 10)
            .unwrap();
        assert!(PreviewMap::open(&q).is_err());
        drop(made);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let p = path("mode");
        let _ = std::fs::remove_file(&p);
        let _map = PreviewMap::create(&p, 2, 2).unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn a_reader_racing_a_writer_never_sees_a_mixed_frame() {
        let p = path("race");
        let _ = std::fs::remove_file(&p);
        let reader = PreviewMap::create(&p, 64, 64).unwrap();
        let writer = PreviewMap::open(&p).unwrap();
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = done.clone();
        let thread = std::thread::spawn(move || {
            let mut n = 0u8;
            while !stop.load(Ordering::Relaxed) {
                n = n.wrapping_add(1);
                writer.publish(64, 64, &[n; 64 * 64 * 4]).unwrap();
            }
        });
        let mut seen = 0;
        let mut read = 0;
        let started = std::time::Instant::now();
        while started.elapsed() < std::time::Duration::from_millis(300) {
            if let Some(frame) = reader.read_new(seen) {
                let first = frame.pixels[0];
                assert!(frame.pixels.iter().all(|&b| b == first), "mixed frame");
                seen = frame.number;
                read += 1;
            }
        }
        done.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        assert!(read > 0);
    }
}
