//! 16 kHz mono float audio to 100 ms PCM16 chunks and a 10 Hz level meter.
//! From `src/renderer/public/audio/pcm-capture.worklet.js`.

/// 1600 samples at 16 kHz = 100 ms per chunk.
pub const CHUNK_SAMPLES: usize = 1600;
pub const SAMPLE_RATE: u32 = 16_000;
/// The meter runs on its own audio clock so a chunk never gates a level update.
pub const METER_INTERVAL_SAMPLES: u64 = 1600;
/// Web Audio's render quantum. Blocks are processed in quanta like the
/// worklet, so the meter cadence does not depend on the caller's block size.
pub const RENDER_QUANTUM: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    pub rms: f64,
    pub peak: f64,
}

#[derive(Default)]
pub struct PcmChunker {
    samples: Vec<i16>,
    meter_sum: f64,
    meter_count: u64,
    meter_peak: f64,
    /// Samples processed so far: the worklet's `currentTime`.
    clock: u64,
    next_meter_at: u64,
}

/// `[-1, 1]` with non-finite as silence, then to signed 16-bit.
pub fn to_pcm16(sample: f32) -> (f32, i16) {
    let s = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let v = if s < 0.0 { s * 32768.0 } else { s * 32767.0 };
    (s, v as i16)
}

impl PcmChunker {
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(CHUNK_SAMPLES),
            ..Self::default()
        }
    }

    /// Process samples. Each full chunk goes to `on_chunk` as little-endian
    /// bytes; a meter update goes to `on_meter` when its interval has passed
    /// at a quantum's start, as the worklet checks once per render quantum.
    pub fn process(
        &mut self,
        samples: &[f32],
        mut on_chunk: impl FnMut(Vec<u8>),
        mut on_meter: impl FnMut(Meter),
    ) {
        for quantum in samples.chunks(RENDER_QUANTUM) {
            self.process_quantum(quantum, &mut on_chunk, &mut on_meter);
        }
    }

    fn process_quantum(
        &mut self,
        block: &[f32],
        on_chunk: &mut impl FnMut(Vec<u8>),
        on_meter: &mut impl FnMut(Meter),
    ) {
        if block.is_empty() {
            return;
        }
        let block_start = self.clock;
        for &raw in block {
            let (s, v) = to_pcm16(raw);
            self.samples.push(v);
            let s = f64::from(s);
            self.meter_sum += s * s;
            self.meter_peak = self.meter_peak.max(s.abs());
            if self.samples.len() == CHUNK_SAMPLES {
                let mut bytes = Vec::with_capacity(CHUNK_SAMPLES * 2);
                for v in &self.samples {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
                on_chunk(bytes);
                self.samples.clear();
            }
        }
        self.meter_count += block.len() as u64;
        self.clock += block.len() as u64;
        if block_start >= self.next_meter_at {
            self.next_meter_at = block_start + METER_INTERVAL_SAMPLES;
            on_meter(Meter {
                rms: if self.meter_count > 0 {
                    (self.meter_sum / self.meter_count as f64).sqrt()
                } else {
                    0.0
                },
                peak: self.meter_peak,
            });
            self.meter_sum = 0.0;
            self.meter_count = 0;
            self.meter_peak = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Run {
        chunks: Vec<Vec<i16>>,
        meters: Vec<Meter>,
    }

    /// Feeds `total` samples of `value` in 128-sample render quanta.
    fn run(chunker: &mut PcmChunker, value: f32, total: usize, block: usize) -> Run {
        let mut out = Run {
            chunks: Vec::new(),
            meters: Vec::new(),
        };
        let data = vec![value; block];
        let mut left = total;
        while left > 0 {
            let n = left.min(block);
            chunker.process(
                &data[..n],
                |c| {
                    out.chunks.push(
                        c.chunks(2)
                            .map(|b| i16::from_le_bytes([b[0], b[1]]))
                            .collect(),
                    )
                },
                |m| out.meters.push(m),
            );
            left -= n;
        }
        out
    }

    #[test]
    fn emits_1600_sample_chunks_with_full_scale_mapping() {
        let r = run(&mut PcmChunker::new(), 1.0, 3200, 128);
        assert_eq!(r.chunks.len(), 2);
        assert!(
            r.chunks
                .iter()
                .all(|c| c.len() == 1600 && c.iter().all(|&s| s == 32767))
        );
        let r = run(&mut PcmChunker::new(), -1.0, 1600, 128);
        assert!(r.chunks[0].iter().all(|&s| s == -32768));
        let r = run(&mut PcmChunker::new(), 0.0, 1600, 128);
        assert!(r.chunks[0].iter().all(|&s| s == 0));
    }

    #[test]
    fn a_chunk_is_emitted_only_once_full() {
        let mut chunker = PcmChunker::new();
        assert!(run(&mut chunker, 0.1, 1599, 1599).chunks.is_empty());
        assert_eq!(run(&mut chunker, 0.1, 1, 1).chunks.len(), 1);
    }

    #[test]
    fn clamps_out_of_range_and_non_finite_samples() {
        assert!(
            run(&mut PcmChunker::new(), 4.0, 1600, 128).chunks[0]
                .iter()
                .all(|&s| s == 32767)
        );
        assert!(
            run(&mut PcmChunker::new(), f32::NAN, 1600, 128).chunks[0]
                .iter()
                .all(|&s| s == 0)
        );
        assert!(
            run(&mut PcmChunker::new(), f32::NEG_INFINITY, 1600, 128).chunks[0]
                .iter()
                .all(|&s| s == 0)
        );
    }

    #[test]
    fn the_meter_runs_at_10_hz_independent_of_chunks() {
        let r = run(&mut PcmChunker::new(), 0.5, 16_000, 128);
        assert!((9..=11).contains(&r.meters.len()), "{}", r.meters.len());
        for m in &r.meters {
            assert!((m.rms - 0.5).abs() < 0.01 && (m.peak - 0.5).abs() < 0.01);
        }
    }

    #[test]
    fn levels_arrive_before_the_first_chunk() {
        let r = run(&mut PcmChunker::new(), 0.3, 128, 128);
        assert!(r.chunks.is_empty());
        assert_eq!(r.meters.len(), 1);
    }

    #[test]
    fn peaks_do_not_latch_across_intervals() {
        let mut chunker = PcmChunker::new();
        let loud = run(&mut chunker, 1.0, 1664, 128);
        assert!(loud.meters.iter().any(|m| m.peak == 1.0));
        let quiet = run(&mut chunker, 0.0, 3200, 128);
        let last = quiet.meters.last().unwrap();
        assert_eq!((last.peak, last.rms), (0.0, 0.0));
    }

    #[test]
    fn batches_across_arbitrary_block_sizes_without_losing_samples() {
        let mut chunker = PcmChunker::new();
        assert!(run(&mut chunker, 0.2, 900, 900).chunks.is_empty());
        assert_eq!(run(&mut chunker, 0.2, 2300, 2300).chunks.len(), 2);
        let mut fresh = PcmChunker::new();
        assert_eq!(run(&mut fresh, 0.2, 5000, 5000).chunks.len(), 3);
    }

    #[test]
    fn an_empty_block_posts_nothing() {
        let r = run(&mut PcmChunker::new(), 0.2, 0, 128);
        assert_eq!(r.chunks.len() + r.meters.len(), 0);
    }
}
