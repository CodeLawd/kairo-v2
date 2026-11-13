//! Decoded video frames as the playback layer hands them over: YUV planes
//! that the GPU turns into the source texture a video node draws.

/// The matrix coefficients and range of a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct YuvColor {
    pub kr: f32,
    pub kb: f32,
    /// JPEG (full) range rather than limited (studio) range.
    pub full_range: bool,
}

impl YuvColor {
    pub const BT709: Self = Self {
        kr: 0.2126,
        kb: 0.0722,
        full_range: false,
    };
    pub const BT601: Self = Self {
        kr: 0.299,
        kb: 0.114,
        full_range: false,
    };
}

/// One frame: a luma plane, an interleaved chroma plane at half size
/// (NV12 when `bits` is 8, P010 little-endian when it is 10) and an
/// optional 8-bit alpha plane of full size.
#[derive(Clone, Debug)]
pub struct YuvFrame {
    pub width: u32,
    pub height: u32,
    pub bits: u32,
    pub color: YuvColor,
    pub luma: Vec<u8>,
    pub luma_stride: u32,
    pub chroma: Vec<u8>,
    pub chroma_stride: u32,
    /// Tightly packed, `width` bytes a row.
    pub alpha: Option<Vec<u8>>,
}

impl YuvFrame {
    fn sample(&self) -> u32 {
        if self.bits > 8 { 2 } else { 1 }
    }

    /// Whether the planes hold what the sizes say. A frame that does not is
    /// refused by the compositor rather than read out of bounds.
    pub fn is_valid(&self) -> bool {
        let s = self.sample();
        let (cw, ch) = (self.width.div_ceil(2), self.height.div_ceil(2));
        let rows = |len: usize, stride: u32, row: u32, n: u32| {
            stride >= row && n > 0 && len >= stride as usize * (n as usize - 1) + row as usize
        };
        self.width > 0
            && self.height > 0
            && (self.bits == 8 || self.bits == 10)
            && rows(
                self.luma.len(),
                self.luma_stride,
                self.width * s,
                self.height,
            )
            && rows(self.chroma.len(), self.chroma_stride, cw * 2 * s, ch)
            && self
                .alpha
                .as_ref()
                .is_none_or(|a| a.len() >= (self.width * self.height) as usize)
    }

    /// A flat frame of one colour, for tests and tools.
    pub fn solid(width: u32, height: u32, y: u8, u: u8, v: u8) -> Self {
        let (cw, ch) = (width.div_ceil(2), height.div_ceil(2));
        Self {
            width,
            height,
            bits: 8,
            color: YuvColor::BT709,
            luma: vec![y; (width * height) as usize],
            luma_stride: width,
            chroma: [u, v].repeat((cw * ch) as usize),
            chroma_stride: cw * 2,
            alpha: None,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ColorUniform {
    kr: f32,
    kb: f32,
    y_offset: f32,
    y_scale: f32,
    c_offset: f32,
    c_scale: f32,
    shift: u32,
    alpha: u32,
}

impl ColorUniform {
    pub(crate) fn of(frame: &YuvFrame) -> Self {
        let ten = frame.bits > 8;
        let steps = if ten { 4.0 } else { 1.0 };
        let max_code: f32 = if ten { 1023.0 } else { 255.0 };
        let full = frame.color.full_range;
        Self {
            kr: frame.color.kr,
            kb: frame.color.kb,
            y_offset: if full { 0.0 } else { 16.0 * steps },
            y_scale: if full {
                max_code.recip()
            } else {
                1.0 / (219.0 * steps)
            },
            c_offset: 128.0 * steps,
            c_scale: if full {
                max_code.recip()
            } else {
                1.0 / (224.0 * steps)
            },
            // P010 keeps its ten bits at the top of each 16-bit sample.
            shift: if ten { 6 } else { 0 },
            alpha: u32::from(frame.alpha.is_some()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solid_frame_is_valid_and_short_planes_are_not() {
        let mut f = YuvFrame::solid(5, 3, 16, 128, 128);
        assert!(f.is_valid());
        f.chroma.pop();
        assert!(!f.is_valid());
        let mut f = YuvFrame::solid(4, 4, 16, 128, 128);
        f.bits = 9;
        assert!(!f.is_valid());
        let mut f = YuvFrame::solid(4, 4, 16, 128, 128);
        f.alpha = Some(vec![0; 15]);
        assert!(!f.is_valid());
    }
}
