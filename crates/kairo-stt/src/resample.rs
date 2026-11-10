//! Device rate to 16 kHz mono, off the audio callback.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

/// Accepts any block length; emits 16 kHz samples as whole resampler chunks
/// complete. At 16 kHz input it passes samples through untouched.
pub struct MonoResampler {
    inner: Option<Fft<f32>>,
    pending: Vec<f32>,
    out: Vec<f32>,
}

impl MonoResampler {
    pub fn new(input_rate: u32) -> Result<Self, String> {
        if input_rate == crate::pcm::SAMPLE_RATE {
            return Ok(Self {
                inner: None,
                pending: Vec::new(),
                out: Vec::new(),
            });
        }
        // 10 ms input chunks keep the added latency small.
        let chunk = (input_rate as usize / 100).max(1);
        let inner = Fft::<f32>::new(
            input_rate as usize,
            crate::pcm::SAMPLE_RATE as usize,
            chunk,
            1,
            FixedSync::Input,
        )
        .map_err(|e| e.to_string())?;
        let out = vec![0.0; inner.output_frames_max()];
        Ok(Self {
            inner: Some(inner),
            pending: Vec::with_capacity(chunk * 4),
            out,
        })
    }

    /// Frames of delay the resampler adds at the output rate.
    pub fn output_delay(&self) -> usize {
        self.inner.as_ref().map_or(0, Resampler::output_delay)
    }

    pub fn process(&mut self, input: &[f32], mut emit: impl FnMut(&[f32])) -> Result<(), String> {
        let Some(inner) = self.inner.as_mut() else {
            emit(input);
            return Ok(());
        };
        self.pending.extend_from_slice(input);
        let mut offset = 0;
        loop {
            let need = inner.input_frames_next();
            if self.pending.len() - offset < need {
                break;
            }
            let slice = &self.pending[offset..offset + need];
            let input = InterleavedSlice::new(slice, 1, need).map_err(|e| e.to_string())?;
            let capacity = self.out.len();
            let mut output =
                InterleavedSlice::new_mut(&mut self.out, 1, capacity).map_err(|e| e.to_string())?;
            let (read, written) = inner
                .process_into_buffer(&input, &mut output, None)
                .map_err(|e| e.to_string())?;
            offset += read;
            emit(&self.out[..written]);
        }
        self.pending.drain(..offset);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_delay_is_small() {
        for rate in [44_100, 48_000] {
            let delay_ms = MonoResampler::new(rate).unwrap().output_delay() as f64 / 16.0;
            eprintln!("{rate} Hz resampler delay {delay_ms} ms");
            assert!(delay_ms < 15.0, "{rate}: {delay_ms} ms");
        }
    }

    #[test]
    fn sixteen_khz_passes_through() {
        let mut r = MonoResampler::new(16_000).unwrap();
        let mut got = Vec::new();
        r.process(&[0.1, 0.2], |s| got.extend_from_slice(s))
            .unwrap();
        assert_eq!(got, [0.1, 0.2]);
    }

    #[test]
    fn rate_conversion_keeps_duration() {
        for rate in [44_100, 48_000, 96_000, 8_000] {
            let mut r = MonoResampler::new(rate).unwrap();
            let mut got = 0;
            // One second in odd-sized blocks.
            let block = vec![0.0f32; 333];
            let mut fed = 0;
            while fed < rate as usize {
                let n = (rate as usize - fed).min(block.len());
                r.process(&block[..n], |s| got += s.len()).unwrap();
                fed += n;
            }
            assert!((15_800..=16_000).contains(&got), "{rate}: {got}");
        }
    }
}
