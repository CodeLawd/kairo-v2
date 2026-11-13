//! A block of program sound for an NDI sender, and the checks
//! `handleNdiAudio` makes before one crosses a process boundary.

/// The tap's block: 1024 frames, as `KairoTap` posts them.
pub const BLOCK_FRAMES: usize = 1024;

/// Planar float PCM: `channels` runs of `samples` values each.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBlock {
    pub sample_rate: f64,
    pub channels: u32,
    pub samples: u32,
    pub data: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockError {
    Channels,
    Samples,
    SampleRate,
    Length,
}

impl std::fmt::Display for BlockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BlockError::Channels => "channels must be 1 to 8",
            BlockError::Samples => "samples must be 1 to 16384",
            BlockError::SampleRate => "sample rate must be 8 to 192 kHz",
            BlockError::Length => "data length is not channels × samples",
        })
    }
}

impl std::error::Error for BlockError {}

impl AudioBlock {
    /// `handleNdiAudio`'s checks, in its order.
    pub fn validate(&self) -> Result<(), BlockError> {
        if !(1..=8).contains(&self.channels) {
            return Err(BlockError::Channels);
        }
        if !(1..=16_384).contains(&self.samples) {
            return Err(BlockError::Samples);
        }
        if !(self.sample_rate.is_finite() && (8_000.0..=192_000.0).contains(&self.sample_rate)) {
            return Err(BlockError::SampleRate);
        }
        if self.data.len() != self.channels as usize * self.samples as usize {
            return Err(BlockError::Length);
        }
        Ok(())
    }

    /// `channelStrideBytes` for the sender: one channel's run, in bytes.
    pub fn channel_stride_bytes(&self) -> usize {
        self.samples as usize * 4
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(channels: u32, samples: u32, rate: f64, len: usize) -> AudioBlock {
        AudioBlock {
            sample_rate: rate,
            channels,
            samples,
            data: vec![0.0; len],
        }
    }

    #[test]
    fn the_limits_are_handle_ndi_audios() {
        assert_eq!(block(2, 1024, 48_000.0, 2048).validate(), Ok(()));
        assert_eq!(block(1, 1, 8_000.0, 1).validate(), Ok(()));
        assert_eq!(block(8, 16_384, 192_000.0, 8 * 16_384).validate(), Ok(()));
        assert_eq!(
            block(0, 1, 48_000.0, 0).validate(),
            Err(BlockError::Channels)
        );
        assert_eq!(
            block(9, 1, 48_000.0, 9).validate(),
            Err(BlockError::Channels)
        );
        assert_eq!(
            block(2, 0, 48_000.0, 0).validate(),
            Err(BlockError::Samples)
        );
        assert_eq!(
            block(2, 16_385, 48_000.0, 32_770).validate(),
            Err(BlockError::Samples)
        );
        assert_eq!(
            block(2, 8, 7_999.0, 16).validate(),
            Err(BlockError::SampleRate)
        );
        assert_eq!(
            block(2, 8, 192_001.0, 16).validate(),
            Err(BlockError::SampleRate)
        );
        assert_eq!(
            block(2, 8, f64::NAN, 16).validate(),
            Err(BlockError::SampleRate)
        );
        assert_eq!(
            block(2, 8, 48_000.0, 15).validate(),
            Err(BlockError::Length)
        );
        assert_eq!(block(2, 1024, 48_000.0, 2048).channel_stride_bytes(), 4096);
    }
}
