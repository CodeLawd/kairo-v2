mod decode;
mod gpu;

use anyhow::{Context, Result, ensure};
use clap::Parser;
use decode::{Decoder, Frame};
use gpu::{Gpu, Planes};
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(about = "Throwaway FFmpeg decode and offscreen wgpu playback measurement")]
struct Options {
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    #[arg(long)]
    software: bool,
    #[arg(long)]
    looping: bool,
    #[arg(long, default_value_t = 1.0)]
    rate: f64,
    #[arg(long, default_value_t = 0.0)]
    start: f64,
    #[arg(long)]
    end: Option<f64>,
    #[arg(long, default_value_t = 8.0)]
    seconds: f64,
    #[arg(long, requires = "seek_to")]
    seek_at: Option<f64>,
    #[arg(long, requires = "seek_at")]
    seek_to: Option<f64>,
    #[arg(long)]
    pause_at: Option<f64>,
    #[arg(long, default_value_t = 0.5)]
    pause_for: f64,
    #[arg(long)]
    expect_alpha: bool,
    #[arg(long)]
    csv: Option<PathBuf>,
}

struct Stream {
    decoder: Decoder,
    planes: Option<Planes>,
    frame: Option<Frame>,
    clip_start: f64,
    epoch: f64,
    wraps: usize,
    last_pts: Option<f64>,
    last_submit: Option<f64>,
    intervals: Timing,
    loop_intervals: Timing,
    gpu_times: Timing,
    advance_times: Timing,
    transfer_times: Timing,
    frames: usize,
    saw_p010: bool,
    saw_alpha: bool,
    ended: bool,
}

impl Stream {
    fn open(path: &std::path::Path, options: &Options) -> Result<Self> {
        let mut stream = Self {
            decoder: Decoder::open(path, !options.software)?,
            planes: None,
            frame: None,
            clip_start: options.start,
            epoch: 0.0,
            wraps: 0,
            last_pts: None,
            last_submit: None,
            intervals: Timing::default(),
            loop_intervals: Timing::default(),
            gpu_times: Timing::default(),
            advance_times: Timing::default(),
            transfer_times: Timing::default(),
            frames: 0,
            saw_p010: false,
            saw_alpha: false,
            ended: false,
        };
        stream.frame = stream.position(options.start, options.end)?;
        ensure!(
            stream.frame.is_some(),
            "no frame in requested range in {}",
            path.display()
        );
        Ok(stream)
    }

    fn position(&mut self, target: f64, end: Option<f64>) -> Result<Option<Frame>> {
        if target > 0.0 || self.last_pts.is_some() {
            self.decoder.seek(target)?;
        }
        while let Some(frame) = self.decoder.next()? {
            if end.is_some_and(|end| frame.pts >= end) {
                return Ok(None);
            }
            if frame.pts + 1e-6 >= target {
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }

    fn advance(&mut self, options: &Options) -> Result<()> {
        let frame = self.decoder.next()?;
        if let Some(frame) = frame.filter(|f| options.end.is_none_or(|end| f.pts < end)) {
            self.frame = Some(frame);
            return Ok(());
        }
        if !options.looping {
            self.ended = true;
            return Ok(());
        }
        let observed_end = self.last_pts.context("empty playback segment")? + self.decoder.interval;
        let boundary = options
            .end
            .map_or(observed_end, |end| end.min(observed_end));
        self.epoch += (boundary - self.clip_start).max(self.decoder.interval) / options.rate;
        self.clip_start = options.start;
        self.frame = self.position(options.start, options.end)?;
        ensure!(self.frame.is_some(), "loop range no longer contains frames");
        self.wraps += 1;
        Ok(())
    }

    fn deadline(&self, rate: f64) -> Option<f64> {
        self.frame
            .as_ref()
            .map(|frame| self.epoch + (frame.pts - self.clip_start) / rate)
    }
}

fn main() -> Result<()> {
    let options = Options::parse();
    ensure!(
        (0.25..=2.0).contains(&options.rate),
        "rate must be in 0.25..=2"
    );
    ensure!(
        options.start.is_finite() && options.start >= 0.0,
        "invalid start"
    );
    ensure!(
        options
            .end
            .is_none_or(|end| end.is_finite() && end > options.start),
        "end must be after start"
    );
    ensure!(
        options.seconds.is_finite() && options.seconds > 0.0,
        "invalid run duration"
    );
    ensure!(
        options.pause_for.is_finite() && options.pause_for > 0.0,
        "invalid pause duration"
    );
    for value in [options.seek_at, options.pause_at].into_iter().flatten() {
        ensure!(
            (0.0..options.seconds).contains(&value),
            "transport event outside run"
        );
    }
    if let Some(target) = options.seek_to {
        ensure!(
            target.is_finite()
                && target >= options.start
                && options.end.is_none_or(|end| target < end),
            "seek target outside clip"
        );
    }

    ffmpeg_next::init()?;
    let gpu = pollster::block_on(Gpu::open())?;
    let mut streams = options
        .inputs
        .iter()
        .map(|path| Stream::open(path, &options))
        .collect::<Result<Vec<_>>>()?;
    let mut csv = options
        .csv
        .as_ref()
        .map(File::create)
        .transpose()?
        .map(BufWriter::new);
    if let Some(csv) = &mut csv {
        writeln!(
            csv,
            "wall_ms,stream,pts_ms,interval_ms,gpu_ms,decode_ms,hw_transfer_ms,wraps,rss_kib"
        )?;
    }

    let mut seek_at = options.seek_at;
    let mut pause_at = options.pause_at;
    let mut previous_rss = None;
    let mut first_rss = None;
    let mut last_rss = None;
    let mut next_rss_sample = 0.0;
    let cpu_start = process_cpu()?;
    let start = Instant::now();
    while start.elapsed().as_secs_f64() < options.seconds {
        let now = start.elapsed().as_secs_f64();
        if now >= next_rss_sample {
            previous_rss = rss_kib();
            first_rss = first_rss.or(previous_rss);
            last_rss = previous_rss;
            next_rss_sample += 5.0;
        }
        if pause_at.is_some_and(|at| now >= at) {
            let paused = Instant::now();
            thread::sleep(Duration::from_secs_f64(options.pause_for));
            let duration = paused.elapsed().as_secs_f64();
            for stream in &mut streams {
                stream.epoch += duration;
                stream.last_submit = None;
            }
            println!("pause held textures for {duration:.3}s");
            pause_at = None;
            continue;
        }
        if seek_at.is_some_and(|at| now >= at) {
            let target = options.seek_to.context("seek target missing")?;
            for stream in &mut streams {
                stream.frame = stream.position(target, options.end)?;
                let frame = stream.frame.as_ref().context("seek found no frame")?;
                println!(
                    "seek target={target:.3}s first_frame={:.3}s error_ms={:.2}",
                    frame.pts,
                    (frame.pts - target) * 1000.0
                );
                stream.clip_start = target;
                stream.epoch = start.elapsed().as_secs_f64();
                stream.last_submit = None;
                stream.ended = false;
            }
            seek_at = None;
        }

        let mut next = options.seconds;
        for (index, stream) in streams.iter_mut().enumerate() {
            let Some(deadline) = stream.deadline(options.rate) else {
                continue;
            };
            if deadline > start.elapsed().as_secs_f64() {
                next = next.min(deadline);
                continue;
            }

            let frame = stream.frame.take().context("scheduled frame missing")?;
            let begin = Instant::now();
            gpu.render(&frame, &mut stream.planes)?;
            let gpu_ms = begin.elapsed().as_secs_f64() * 1000.0;
            let now = start.elapsed().as_secs_f64();
            let interval = stream.last_submit.map_or(0.0, |prev| (now - prev) * 1000.0);
            if stream.last_submit.is_some() {
                stream.intervals.record(interval);
                if stream.last_pts.is_some_and(|previous| frame.pts < previous) {
                    stream.loop_intervals.record(interval);
                }
            }
            stream.saw_p010 |= frame.bits == 10;
            stream.saw_alpha |= frame.alpha.is_some();
            if stream.frames == 0 {
                let width = frame.pixels.width();
                let height = frame.pixels.height();
                let planes = stream.planes.as_ref().context("missing rendered texture")?;
                let pixel = gpu.read_pixel(planes, width / 2, height / 2)?;
                if options.expect_alpha {
                    let expected = frame
                        .alpha
                        .as_ref()
                        .context("decoder did not preserve alpha")?
                        [(height / 2 * width + width / 2) as usize];
                    ensure!(
                        pixel[3].abs_diff(expected) <= 1 && expected < 255,
                        "alpha readback did not match non-opaque fixture"
                    );
                }
                println!(
                    "stream={index} decoder={} size={width}x{height} bits={} alpha={} RGBA={pixel:?}",
                    stream.decoder.backend,
                    frame.bits,
                    frame.alpha.is_some()
                );
            }
            stream.frames += 1;
            stream.last_submit = Some(now);
            stream.last_pts = Some(frame.pts);
            stream.gpu_times.record(gpu_ms);
            let transfer_ms = frame.transfer_ms;
            if let Some(ms) = transfer_ms {
                stream.transfer_times.record(ms);
            }
            drop(frame);
            let advance_start = Instant::now();
            stream.advance(&options)?;
            let decode_ms = advance_start.elapsed().as_secs_f64() * 1000.0;
            stream.advance_times.record(decode_ms);
            if let Some(csv) = &mut csv {
                writeln!(
                    csv,
                    "{:.3},{index},{:.3},{interval:.3},{gpu_ms:.3},{decode_ms:.3},{},{},{}",
                    now * 1000.0,
                    stream.last_pts.unwrap_or_default() * 1000.0,
                    transfer_ms.map_or_else(String::new, |ms| format!("{ms:.3}")),
                    stream.wraps,
                    previous_rss.map_or_else(String::new, |value| value.to_string())
                )?;
            }
            if let Some(deadline) = stream.deadline(options.rate) {
                next = next.min(deadline);
            }
        }
        if streams.iter().all(|stream| stream.frame.is_none()) && seek_at.is_none() {
            break;
        }
        if let Some(at) = seek_at {
            next = next.min(at);
        }
        if let Some(at) = pause_at {
            next = next.min(at);
        }
        let remaining = next - start.elapsed().as_secs_f64();
        if remaining > 0.0 {
            thread::sleep(Duration::from_secs_f64(remaining));
        }
    }

    if let Some(csv) = &mut csv {
        csv.flush()?;
    }
    let wall = start.elapsed().as_secs_f64();
    println!(
        "wall_s={wall:.3} cpu_percent={:.2} rss_first_kib={first_rss:?} rss_last_kib={last_rss:?}",
        (process_cpu()? - cpu_start) * 100.0 / wall
    );
    for (index, stream) in streams.iter().enumerate() {
        println!(
            "stream={index} frames={} wraps={} ended={} p010={} alpha={} interval_p99_ms={:?} loop_interval_max_ms={:?} gpu_p99_ms={:?} advance_p99_ms={:?} hw_transfer_p99_ms={:?}",
            stream.frames,
            stream.wraps,
            stream.ended,
            stream.saw_p010,
            stream.saw_alpha,
            stream.intervals.p99(),
            stream.loop_intervals.max,
            stream.gpu_times.p99(),
            stream.advance_times.p99(),
            stream.transfer_times.p99()
        );
    }
    Ok(())
}

// Fixed storage keeps long-run memory measurements independent of frame count.
// The final bucket contains all samples at or above one second.
struct Timing {
    bins: Vec<u64>,
    count: u64,
    max: Option<f64>,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            bins: vec![0; 10_001],
            count: 0,
            max: None,
        }
    }
}

impl Timing {
    fn record(&mut self, ms: f64) {
        let bucket = (ms * 10.0).ceil() as usize;
        let index = bucket.min(self.bins.len() - 1);
        self.bins[index] += 1;
        self.count += 1;
        self.max = Some(self.max.map_or(ms, |previous| previous.max(ms)));
    }

    fn p99(&self) -> Option<f64> {
        if self.count == 0 {
            return None;
        }
        let rank = (self.count * 99).div_ceil(100);
        let mut seen = 0;
        for (index, count) in self.bins.iter().enumerate() {
            seen += count;
            if seen >= rank {
                return if index == self.bins.len() - 1 {
                    self.max
                } else {
                    Some(index as f64 / 10.0)
                };
            }
        }
        None
    }
}

#[cfg(unix)]
fn rss_kib() -> Option<u64> {
    let result = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    if !result.status.success() {
        return None;
    }
    std::str::from_utf8(&result.stdout)
        .ok()?
        .trim()
        .parse()
        .ok()
}

#[cfg(unix)]
fn process_cpu() -> Result<f64> {
    // SAFETY: getrusage initializes the provided struct when it succeeds.
    unsafe {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
        if libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) != 0 {
            return Err(std::io::Error::last_os_error()).context("sample CPU time");
        }
        let usage = usage.assume_init();
        let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1_000_000.0;
        Ok(seconds(usage.ru_utime) + seconds(usage.ru_stime))
    }
}

#[cfg(windows)]
fn rss_kib() -> Option<u64> {
    use windows_sys::Win32::System::{
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    // SAFETY: the current process handle is valid and counters has the requested size.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some(counters.WorkingSetSize as u64 / 1024)
}

#[cfg(windows)]
fn process_cpu() -> Result<f64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let [mut created, mut exited, mut kernel, mut user] = [FILETIME::default(); 4];
    // SAFETY: the process handle and all four output pointers are valid.
    let ok = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    if ok == 0 {
        return Err(std::io::Error::last_os_error()).context("sample CPU time");
    }
    let seconds = |time: FILETIME| {
        ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as f64 / 1e7
    };
    Ok(seconds(kernel) + seconds(user))
}
