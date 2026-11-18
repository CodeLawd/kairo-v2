//! Plays looping videos through the real compositor on offscreen outputs
//! and samples this process's CPU and resident memory, for the prompt 055
//! performance figures.
//!
//! ```text
//! cargo run --release -p kairo-video --example soak -- FILE [--streams N]
//!     [--secs S] [--warmup S] [--every S] [--software] [--decode-only]
//! ```
//!
//! Each stream is its own source (a different cue) on its own 1080p
//! offscreen output. After `--warmup` seconds (default 15) it prints one
//! line every `--every` seconds (default 1): elapsed time, CPU percent of
//! one core over the interval, RSS, and frames shown and dropped so far.

#[cfg(unix)]
#[path = "soak/unix.rs"]
mod unix;

#[cfg(unix)]
fn main() {
    unix::run();
}

#[cfg(not(unix))]
fn main() {
    eprintln!("The video soak example requires Unix process metrics.");
    std::process::exit(1);
}
