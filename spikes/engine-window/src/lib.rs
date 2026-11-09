//! Shared protocol and logic for the engine-process spike. Throwaway code.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering, fence};

pub const PREVIEW_WIDTH: u32 = 640;
pub const PREVIEW_HEIGHT: u32 = 360;
const MAX_MESSAGE: u32 = 1 << 20;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DisplayInfo {
    pub id: u32,
    pub label: String,
    pub bounds: Rect,
    pub scale_factor: f64,
}

/// What an output remembers about the display it was pointed at.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Binding {
    pub display_id: Option<u32>,
    pub label: String,
    pub size: Option<(u32, u32)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct OutputSpec {
    pub name: String,
    pub binding: Binding,
    pub rehearsal: bool,
    pub frame_height: u32,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Command {
    Ping(u64),
    Configure {
        outputs: Vec<OutputSpec>,
        preview_fps: u32,
    },
    Shutdown,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Event {
    Pong(u64),
    Ready,
    Displays(Vec<DisplayInfo>),
    Parked(String),
    Restored(String),
    Report(String),
}

pub fn write_message<T: Serialize>(writer: &mut impl Write, message: &T) -> io::Result<()> {
    let body = bincode::serialize(message).map_err(io::Error::other)?;
    let mut framed = Vec::with_capacity(body.len() + 4);
    framed.extend_from_slice(&(body.len() as u32).to_le_bytes());
    framed.extend_from_slice(&body);
    writer.write_all(&framed)?;
    writer.flush()
}

pub fn read_message<T: for<'de> Deserialize<'de>>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    if length > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut body = vec![0; length as usize];
    reader.read_exact(&mut body)?;
    bincode::deserialize(&body).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Port of `resolveDisplay`: id first, then a unique label and size match, and
/// never any other screen.
pub fn resolve_display<'a>(
    binding: &Binding,
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

/// Port of the rehearsal branch of `createDisplayWindow`: half width, centred.
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

/// Port of `ManagedWindow.allowFailure`: three failures per rolling minute.
#[derive(Default)]
pub struct FailureBudget {
    failures: Vec<u64>,
}

impl FailureBudget {
    pub const MAX_FAILURES: usize = 3;
    pub const WINDOW_MS: u64 = 60_000;

    pub fn allow(&mut self, now_ms: u64) -> bool {
        self.failures
            .retain(|at| now_ms.saturating_sub(*at) < Self::WINDOW_MS);
        if self.failures.len() >= Self::MAX_FAILURES {
            return false;
        }
        self.failures.push(now_ms);
        true
    }
}

/// Nearest-rank percentile of sorted samples.
pub fn percentile(sorted: &[f64], fraction: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    Some(sorted[rank.clamp(1, sorted.len()) - 1])
}

const MAGIC: u32 = 0x4b50_5256;
const SLOTS: usize = 3;
const HEADER: usize = 64;
const LATEST: usize = 12;
const FRAME: usize = 16;
const SEQUENCES: usize = 24;

/// Three seqlocked BGRA frames. No reader state lives in the writer, so a
/// relaunched engine can resume on the same mapping after a SIGKILL.
pub struct PreviewShm {
    map: memmap2::MmapMut,
    stride: usize,
}

impl PreviewShm {
    pub fn create(path: &Path, width: u32, height: u32) -> io::Result<Self> {
        let stride = (width * height * 4) as usize;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len((HEADER + stride * SLOTS) as u64)?;
        // SAFETY: the file is private to this controller and its engine.
        let mut map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        map[4..8].copy_from_slice(&width.to_le_bytes());
        map[8..12].copy_from_slice(&height.to_le_bytes());
        map[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        Ok(Self { map, stride })
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        // SAFETY: as in `create`; the length is validated below.
        let map = unsafe { memmap2::MmapMut::map_mut(&file)? };
        let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid preview mapping");
        if map.len() < HEADER {
            return Err(invalid());
        }
        let word = |at: usize| u32::from_le_bytes(map[at..at + 4].try_into().unwrap());
        let stride = (word(4) as usize) * (word(8) as usize) * 4;
        if word(0) != MAGIC || map.len() != HEADER + stride * SLOTS {
            return Err(invalid());
        }
        Ok(Self { map, stride })
    }

    fn atomic32(&self, offset: usize) -> &AtomicU32 {
        // SAFETY: offsets are aligned header words inside the mapping.
        unsafe { &*(self.map.as_ptr().add(offset) as *const AtomicU32) }
    }

    fn frame_counter(&self) -> &AtomicU64 {
        // SAFETY: FRAME is 8-aligned in a page-aligned mapping.
        unsafe { &*(self.map.as_ptr().add(FRAME) as *const AtomicU64) }
    }

    fn slot(&self, index: usize) -> *mut u8 {
        // SAFETY: index < SLOTS, so the offset stays inside the mapping.
        unsafe { (self.map.as_ptr() as *mut u8).add(HEADER + index * self.stride) }
    }

    pub fn publish(&self, pixels: &[u8]) {
        assert_eq!(pixels.len(), self.stride);
        let index = (self.atomic32(LATEST).load(Ordering::Acquire) as usize + 1) % SLOTS;
        let sequence = self.atomic32(SEQUENCES + index * 4);
        // An engine killed mid-write leaves an odd value; make it odd regardless.
        let odd = (sequence.load(Ordering::Relaxed) | 1).wrapping_add(2);
        sequence.store(odd, Ordering::Relaxed);
        fence(Ordering::Release);
        // SAFETY: readers detect overlap with the sequence and discard.
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.slot(index), self.stride);
        }
        sequence.store(odd.wrapping_add(1), Ordering::Release);
        self.atomic32(LATEST).store(index as u32, Ordering::Release);
        self.frame_counter().fetch_add(1, Ordering::AcqRel);
    }

    /// Copies the newest frame when it is newer than `last_frame`.
    pub fn read_new(&self, last_frame: u64) -> Option<(u64, Vec<u8>)> {
        let frame = self.frame_counter().load(Ordering::Acquire);
        if frame == 0 || frame == last_frame {
            return None;
        }
        for _ in 0..4 {
            let index = self.atomic32(LATEST).load(Ordering::Acquire) as usize;
            let sequence = self.atomic32(SEQUENCES + index * 4);
            let before = sequence.load(Ordering::Acquire);
            if before & 1 == 1 {
                continue;
            }
            let mut out = vec![0; self.stride];
            // SAFETY: the sequence check below rejects a torn copy.
            unsafe {
                std::ptr::copy_nonoverlapping(self.slot(index), out.as_mut_ptr(), self.stride);
            }
            fence(Ordering::Acquire);
            if sequence.load(Ordering::Relaxed) == before {
                return Some((frame, out));
            }
        }
        None
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

    fn binding(id: Option<u32>, label: &str) -> Binding {
        Binding {
            display_id: id,
            label: label.into(),
            size: Some((1920, 1080)),
        }
    }

    #[test]
    fn framing_round_trips_and_rejects_oversize() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Command::Ping(7)).unwrap();
        let decoded: Command = read_message(&mut bytes.as_slice()).unwrap();
        assert_eq!(decoded, Command::Ping(7));
        let oversize = (MAX_MESSAGE + 1).to_le_bytes();
        assert!(read_message::<Command>(&mut oversize.as_slice()).is_err());
    }

    #[test]
    fn truncated_message_is_an_error() {
        let mut bytes = Vec::new();
        write_message(&mut bytes, &Command::Ping(7)).unwrap();
        bytes.pop();
        assert!(read_message::<Command>(&mut bytes.as_slice()).is_err());
    }

    #[test]
    fn resolve_never_guesses() {
        let none = HashSet::new();
        let displays = [display(1, "A"), display(2, "B")];
        assert_eq!(
            resolve_display(&binding(Some(2), "x"), &displays, &none)
                .unwrap()
                .id,
            2
        );
        assert!(resolve_display(&binding(None, "A"), &displays, &none).is_none());
        assert_eq!(
            resolve_display(&binding(Some(9), "A"), &displays, &none)
                .unwrap()
                .id,
            1
        );
        assert!(resolve_display(&binding(Some(9), "  "), &displays, &none).is_none());
        let twins = [display(1, "A"), display(2, "A")];
        assert!(resolve_display(&binding(Some(9), "A"), &twins, &none).is_none());
        let claimed = HashSet::from([1]);
        assert!(resolve_display(&binding(Some(1), "A"), &displays, &claimed).is_none());
    }

    #[test]
    fn rehearsal_is_half_width_and_centred() {
        let bounds = rehearsal_bounds(
            &Rect {
                x: 100,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1080,
        );
        assert_eq!(
            bounds,
            Rect {
                x: 580,
                y: 270,
                width: 960,
                height: 540
            }
        );
    }

    #[test]
    fn budget_allows_three_per_rolling_minute() {
        let mut budget = FailureBudget::default();
        assert!(budget.allow(0) && budget.allow(1) && budget.allow(2));
        assert!(!budget.allow(59_999));
        assert!(budget.allow(60_000));
    }

    #[test]
    fn percentile_uses_nearest_rank() {
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        assert_eq!(percentile(&samples, 0.99), Some(99.0));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn shm_survives_a_writer_killed_mid_frame() {
        let path = std::env::temp_dir().join(format!("spike-shm-{}", std::process::id()));
        let reader = PreviewShm::create(&path, 2, 1).unwrap();
        assert!(reader.read_new(0).is_none());
        let writer = PreviewShm::open(&path).unwrap();
        writer.publish(&[1; 8]);
        let (frame, pixels) = reader.read_new(0).unwrap();
        assert_eq!((frame, pixels), (1, vec![1; 8]));
        assert!(reader.read_new(1).is_none());

        // Simulate death during a write to the next slot.
        let next = (writer.atomic32(LATEST).load(Ordering::Acquire) as usize + 1) % SLOTS;
        writer
            .atomic32(SEQUENCES + next * 4)
            .fetch_or(1, Ordering::SeqCst);
        let relaunched = PreviewShm::open(&path).unwrap();
        relaunched.publish(&[2; 8]);
        assert_eq!(reader.read_new(1).unwrap().1, vec![2; 8]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn shm_rejects_a_bad_mapping() {
        let path = std::env::temp_dir().join(format!("spike-bad-{}", std::process::id()));
        std::fs::write(&path, [0; 8]).unwrap();
        assert!(PreviewShm::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
