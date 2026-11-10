//! Real-time replay of a raw PCM file into the relay. From
//! `src/main/tools/replay-audio.ts`, the developer-only diagnostic that sends
//! recorded audio through live transcription.

use crate::relay::AudioRelay;
use std::path::Path;
use std::time::Duration;

/// 20 ms of 16 kHz mono PCM16, the tool's read size.
pub const FRAME_BYTES: usize = 640;
/// Trailing silence frames that let the provider endpoint the last segment.
pub const SILENCE_FRAMES: usize = 150;
/// 30 minutes.
pub const MAX_BYTES: u64 = 32_000 * 60 * 30;

/// Nonempty, whole 16-bit samples, at most 30 minutes.
pub fn validate_pcm_file(path: &Path) -> Result<u64, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() == 0 || meta.len() % 2 != 0 || meta.len() > MAX_BYTES {
        return Err("Expected nonempty 16-bit PCM file, at most 30 minutes".into());
    }
    Ok(meta.len())
}

/// Feed `path` (s16le, 16 kHz, mono) into `relay` paced at real time, then
/// three seconds of silence. Returns the input duration in milliseconds.
pub async fn replay_pcm(path: &Path, relay: &AudioRelay) -> Result<f64, String> {
    let len = validate_pcm_file(path)?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let started = tokio::time::Instant::now();
    let mut sent = 0usize;
    let frames = bytes
        .chunks(FRAME_BYTES)
        .map(<[u8]>::to_vec)
        .chain(std::iter::repeat_n(vec![0; FRAME_BYTES], SILENCE_FRAMES));
    for frame in frames {
        // 32 bytes per millisecond.
        tokio::time::sleep_until(started + Duration::from_micros(sent as u64 * 1000 / 32)).await;
        sent += frame.len();
        relay.feed(frame);
    }
    Ok(len as f64 / 32.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_odd_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let empty = dir.path().join("e.pcm");
        std::fs::write(&empty, b"").unwrap();
        let odd = dir.path().join("o.pcm");
        std::fs::write(&odd, b"abc").unwrap();
        assert!(validate_pcm_file(&empty).is_err());
        assert!(validate_pcm_file(&odd).is_err());
        assert!(validate_pcm_file(&dir.path().join("missing")).is_err());
        assert!(validate_pcm_file(dir.path()).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn paces_at_real_time_and_adds_silence() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.pcm");
        std::fs::write(&file, vec![1u8; 32_000]).unwrap();
        let relay = AudioRelay::default();
        let stream = relay.create_stream();
        let started = tokio::time::Instant::now();
        let ms = replay_pcm(&file, &relay).await.unwrap();
        assert_eq!(ms, 1000.0);
        // 1 s of audio plus 3 s of silence, minus the last frame's 20 ms.
        assert_eq!(started.elapsed(), Duration::from_millis(3980));
        let mut total = 0;
        while let Some(chunk) = stream.try_recv() {
            total += chunk.len();
        }
        // The relay keeps 5 s, so all 4 s arrive.
        assert_eq!(total, 32_000 + FRAME_BYTES * SILENCE_FRAMES);
    }
}
