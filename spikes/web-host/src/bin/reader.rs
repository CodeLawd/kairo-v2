//! A separate process that reads frames from shared memory into a wgpu texture.
use std::{fs::OpenOptions, os::fd::AsRawFd, sync::atomic::*, time::Instant};

const HEADER: usize = 32;

unsafe extern "C" {
    fn clock_gettime_nsec_np(clock: u32) -> u64;
}

/// CLOCK_UPTIME_RAW, comparable across processes.
fn now_ns() -> u64 {
    unsafe { clock_gettime_nsec_np(8) }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let seconds: f64 = args[2].parse().expect("seconds");
    let (w, h) = (1920u32, 1080u32);
    let size = HEADER + (w * h * 4) as usize;
    let started = Instant::now();
    let file = loop {
        if let Ok(file) = OpenOptions::new().read(true).open(path)
            && file
                .metadata()
                .map(|m| m.len() as usize >= size)
                .unwrap_or(false)
        {
            break file;
        }
        assert!(
            started.elapsed().as_secs() < 60,
            "no shared memory appeared"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let map = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            file.as_raw_fd(),
            0,
        )
    } as *const u8;
    assert_ne!(map, libc::MAP_FAILED as *const u8);
    let seq = unsafe { &*(map as *const AtomicU64) };
    let stamp = unsafe { &*(map.add(16) as *const AtomicU64) };

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("adapter");
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("device");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8Unorm,
        usage: wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let mut last = 0;
    let (mut torn, mut frames, mut window_frames) = (0u64, 0u64, 0u64);
    let mut latencies = Vec::new();
    let mut gaps = Vec::new();
    let mut last_frame = Instant::now();
    let mut window = Instant::now();
    while started.elapsed().as_secs_f64() < seconds {
        let before = seq.load(Ordering::Acquire);
        if before == last || before % 2 == 1 {
            std::thread::sleep(std::time::Duration::from_micros(500));
            continue;
        }
        let painted = stamp.load(Ordering::Relaxed);
        unsafe {
            std::ptr::copy_nonoverlapping(map.add(HEADER), pixels.as_mut_ptr(), pixels.len())
        };
        if seq.load(Ordering::Acquire) != before {
            torn += 1;
            continue;
        }
        last = before;
        queue.write_texture(
            texture.as_image_copy(),
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            texture.size(),
        );
        queue.submit([]);
        latencies.push((now_ns() - painted) as f64 / 1e6);
        gaps.push(last_frame.elapsed().as_secs_f64() * 1000.0);
        last_frame = Instant::now();
        frames += 1;
        window_frames += 1;
        if window.elapsed().as_secs() >= 60 {
            println!(
                "t={:.0}s fps={:.1}",
                started.elapsed().as_secs_f64(),
                window_frames as f64 / window.elapsed().as_secs_f64()
            );
            window_frames = 0;
            window = Instant::now();
        }
    }
    let pct = |v: &mut Vec<f64>, q: f64| {
        v.sort_by(f64::total_cmp);
        v.get(((v.len().max(1) - 1) as f64 * q) as usize)
            .copied()
            .unwrap_or(f64::NAN)
    };
    println!(
        "reader frames={frames} fps={:.2} torn={torn} latency_ms p50={:.2} p99={:.2} gap_ms p50={:.2} p99={:.2} max={:.2}",
        frames as f64 / seconds,
        pct(&mut latencies, 0.5),
        pct(&mut latencies, 0.99),
        pct(&mut gaps, 0.5),
        pct(&mut gaps, 0.99),
        pct(&mut gaps, 1.0)
    );
}
