//! The frame ring against its own reader and against `ring.h` itself.
#![cfg(unix)]

use kairo_engine_proto::ring::*;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

fn name(tag: &str) -> String {
    format!("/kr-{tag}-{}", std::process::id())
}

#[test]
fn invalid_names_and_rasters_are_rejected() {
    assert!(RingWriter::create("no-slash", 2, 2).is_err());
    assert!(RingWriter::create("", 2, 2).is_err());
    assert!(RingWriter::create(&format!("/{}", "x".repeat(30)), 2, 2).is_err());
    assert!(RingWriter::create(&name("odd"), 3, 2).is_err());
    assert!(RingWriter::create(&name("wide"), 4098, 2).is_err());
    assert!(RingWriter::create(&name("tall"), 4, 2161).is_err());
    assert!(RingWriter::create(&name("flat"), 4, 0).is_err());
}

#[test]
fn slots_are_whole_pages() {
    assert_eq!(slot_bytes_for(1920, 1080) % PAGE, 0);
    assert!(slot_bytes_for(1920, 1080) >= frame_bytes(1920, 1080));
    assert_eq!(total_bytes(PAGE), PAGE * 4);
}

#[test]
fn reader_sees_each_new_frame_once_and_never_the_newest_slot_is_rewritten() {
    let name = name("rot");
    let writer = RingWriter::create(&name, 4, 2).unwrap();
    let reader = RingReader::open(&name, 4, 2).unwrap();
    let mut last = 0;
    let mut out = vec![0; frame_bytes(4, 2)];
    assert!(
        !reader.read_newest(&mut last, &mut out),
        "nothing published"
    );

    for value in 1..=10u8 {
        let before = writer.header().latest.load(Ordering::Acquire);
        let published = writer.header().published.load(Ordering::Acquire);
        let mut slot = writer.begin_write().unwrap();
        if published > 0 {
            assert_ne!(slot.index(), before, "the newest complete slot is reserved");
        }
        slot.data().fill(value);
        slot.publish();
        assert!(reader.read_newest(&mut last, &mut out));
        assert_eq!(out, vec![value; out.len()]);
        assert!(!reader.read_newest(&mut last, &mut out), "same frame twice");
    }
    assert_eq!(last, 10);
}

#[test]
fn a_slot_rewritten_during_the_copy_is_rejected() {
    let name = name("torn");
    let writer = RingWriter::create(&name, 4, 2).unwrap();
    let reader = RingReader::open(&name, 4, 2).unwrap();
    assert!(writer.write(|f| f.fill(1)));
    let mut last = 0;
    let mut out = vec![0; frame_bytes(4, 2)];
    let latest = writer.header().latest.load(Ordering::Acquire) as usize;
    let torn = reader.read_newest_with(&mut last, &mut out, || {
        writer.header().slots[latest]
            .seq
            .fetch_add(2, Ordering::SeqCst);
    });
    assert!(!torn);
    assert_eq!(last, 0, "a rejected copy is not consumed");
    assert!(reader.read_newest(&mut last, &mut out));
}

#[test]
fn an_abandoned_write_is_not_published_and_frees_its_slot() {
    let name = name("drop");
    let writer = RingWriter::create(&name, 4, 2).unwrap();
    {
        let mut slot = writer.begin_write().unwrap();
        slot.data().fill(9);
    }
    assert_eq!(writer.header().published.load(Ordering::Acquire), 0);
    for slot in &writer.header().slots {
        assert_eq!(slot.seq.load(Ordering::Acquire) % 2, 0);
    }
    // Two in flight at once take two different slots; a third waits for
    // neither and is dropped only when no slot is left.
    assert!(writer.write(|f| f.fill(1)));
    let first = writer.begin_write().unwrap();
    let second = writer.begin_write().unwrap();
    assert_ne!(first.index(), second.index());
    assert!(
        writer.begin_write().is_none(),
        "frames are dropped, not queued"
    );
    drop(first);
    assert!(writer.begin_write().is_some());
}

#[test]
fn a_stale_ring_is_replaced_and_a_wrong_raster_is_refused() {
    let name = name("stale");
    let first = RingWriter::create(&name, 4, 2).unwrap();
    first.write(|f| f.fill(5));
    std::mem::forget(first);
    let writer = RingWriter::create(&name, 4, 2).unwrap();
    assert_eq!(writer.header().published.load(Ordering::Acquire), 0);
    assert!(RingReader::open(&name, 6, 2).is_err());
    drop(writer);
    assert!(RingReader::open(&name, 4, 2).is_err(), "dropping unlinks");
}

/// Compiles a reader against Electron's `ring.h` and has it read our ring:
/// the layout proof. Skips when the Electron checkout or a C++ compiler is
/// missing, unless `KAIRO_REQUIRE_RING_H` is set.
#[test]
fn cpp_ring_h_reads_frames_this_writer_publishes() {
    let header = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../kairo-presenter/native/gpu-frames/ring.h");
    let required = std::env::var_os("KAIRO_REQUIRE_RING_H").is_some();
    if !header.exists() {
        assert!(!required, "ring.h not found at {}", header.display());
        eprintln!("skipped: {} not found", header.display());
        return;
    }
    let dir = std::env::temp_dir().join(format!("kairo-ring-h-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("reader.cpp");
    std::fs::write(
        &source,
        r#"
#include "ring.h"
#include <cstdio>
#include <fcntl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>
#include <vector>
int main(int argc, char** argv) {
  int fd = shm_open(argv[1], O_RDWR, 0);
  if (fd < 0) return 2;
  struct stat st; fstat(fd, &st);
  void* base = mmap(nullptr, st.st_size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
  close(fd);
  auto* h = static_cast<kairo_ring::Header*>(base);
  if (!kairo_ring::compatible(h, 4, 2)) return 3;
  if ((size_t)st.st_size != kairo_ring::totalBytes(h->slotBytes)) return 4;
  std::vector<uint8_t> out(kairo_ring::frameBytes(4, 2));
  uint64_t last = 0;
  if (!kairo_ring::readNewest(h, base, last, out.data(), out.size())) return 5;
  unsigned sum = 0; for (auto b : out) sum += b;
  std::printf("%llu %u %u\n", (unsigned long long)last, h->latest.load(), sum);
  return 0;
}
"#,
    )
    .unwrap();
    let binary = dir.join("reader");
    let compiled = std::process::Command::new("c++")
        .args(["-std=c++17", "-O1", "-I"])
        .arg(header.parent().unwrap())
        .arg(&source)
        .arg("-o")
        .arg(&binary)
        .status();
    match compiled {
        Ok(status) if status.success() => {}
        other => {
            assert!(!required, "c++ failed: {other:?}");
            eprintln!("skipped: c++ unavailable ({other:?})");
            return;
        }
    }
    let name = name("cpp");
    let writer = RingWriter::create(&name, 4, 2).unwrap();
    writer.write(|f| f.fill(1));
    writer.write(|f| {
        for (i, byte) in f.iter_mut().enumerate() {
            *byte = i as u8;
        }
    });
    let output = std::process::Command::new(&binary)
        .arg(&name)
        .output()
        .unwrap();
    assert!(output.status.success(), "reader exited {:?}", output.status);
    let latest = writer.header().latest.load(Ordering::Acquire);
    let expected_sum: u32 = (0..frame_bytes(4, 2) as u32).sum();
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("2 {latest} {expected_sum}")
    );
    std::fs::remove_dir_all(dir).ok();
}
