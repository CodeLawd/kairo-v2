//! NDI binding, DeckLink ring writer and Metal C ABI for spike 005. Throwaway.

#[cfg(target_os = "macos")]
pub mod converter;
pub mod ndi;
pub mod ring;

#[cfg(target_os = "macos")]
pub mod gpu {
    use std::ffi::{CStr, c_char, c_int};

    unsafe extern "C" {
        fn kgf_error() -> *const c_char;
        fn kgf_convert_pixels(
            bgra: *const u8,
            length: usize,
            sw: u32,
            sh: u32,
            dw: u32,
            dh: u32,
            alpha: bool,
            out: *mut u8,
            out_length: usize,
        ) -> c_int;
        fn kgf_cpu_convert(bgra: *const u8, sw: u32, sh: u32, out: *mut u8, dw: i64, dh: i64);
        pub fn kgf_last_gpu_ns() -> u64;
        pub fn kgf_ring_layout(what: c_int) -> usize;
    }

    pub fn error() -> Option<String> {
        // SAFETY: returns null or a string owned by the static singleton.
        let text = unsafe { kgf_error() };
        (!text.is_null()).then(|| {
            unsafe { CStr::from_ptr(text) }
                .to_string_lossy()
                .into_owned()
        })
    }

    pub fn convert(
        bgra: &[u8],
        sw: u32,
        sh: u32,
        dw: u32,
        dh: u32,
        alpha: bool,
        out: &mut [u8],
    ) -> i32 {
        // SAFETY: lengths are passed with the pointers and validated in C.
        unsafe {
            kgf_convert_pixels(
                bgra.as_ptr(),
                bgra.len(),
                sw,
                sh,
                dw,
                dh,
                alpha,
                out.as_mut_ptr(),
                out.len(),
            )
        }
    }

    pub fn reference(bgra: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
        assert_eq!(bgra.len(), (sw * sh * 4) as usize);
        let mut out = vec![0; (dw * dh * 2) as usize];
        // SAFETY: both buffers were sized for these rasters above.
        unsafe {
            kgf_cpu_convert(
                bgra.as_ptr(),
                sw,
                sh,
                out.as_mut_ptr(),
                dw.into(),
                dh.into(),
            )
        };
        out
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::{gpu, ring};
    use std::mem::{offset_of, size_of};

    fn solid(b: u8, g: u8, r: u8) -> Vec<u8> {
        [b, g, r, 255, b, g, r, 255].to_vec()
    }

    fn converted(pixels: &[u8]) -> Vec<u8> {
        let mut out = vec![0; 4];
        assert_eq!(
            gpu::convert(pixels, 2, 1, 2, 1, false, &mut out),
            0,
            "{:?}",
            gpu::error()
        );
        out
    }

    #[test]
    fn golden_vectors_match_uyvy_h_byte_for_byte() {
        for (pixels, expected) in [
            (solid(0, 0, 0), [128, 16, 128, 16]),
            // U is 127: `(-1 * 255 + 128) >> 8` rounds down in uyvy.h.
            (solid(255, 255, 255), [127, 235, 128, 235]),
            (solid(0, 0, 255), [102, 63, 240, 63]),
        ] {
            assert_eq!(gpu::reference(&pixels, 2, 1, 2, 1), expected);
            assert_eq!(converted(&pixels), expected);
        }
    }

    #[test]
    fn gradient_matches_reference_with_scaling() {
        let bgra: Vec<u8> = (0..64u32 * 36)
            .flat_map(|i| [(i % 251) as u8, (i % 241) as u8, (i % 239) as u8, 255])
            .collect();
        let mut out = vec![0; 32 * 18 * 2];
        assert_eq!(gpu::convert(&bgra, 64, 36, 32, 18, false, &mut out), 0);
        assert_eq!(out, gpu::reference(&bgra, 64, 36, 32, 18));
    }

    #[test]
    fn alpha_plane_follows_the_uyvy_plane() {
        let mut out = vec![0; 6];
        let pixels = [0, 0, 0, 10, 0, 0, 0, 200];
        assert_eq!(gpu::convert(&pixels, 2, 1, 2, 1, true, &mut out), 0);
        assert_eq!(&out[4..], [10, 200]);
    }

    #[test]
    fn bad_dimensions_are_rejected() {
        let mut out = vec![0; 4];
        assert_eq!(gpu::convert(&[0; 7], 2, 1, 2, 1, false, &mut out), 2);
        assert_eq!(gpu::convert(&[0; 8], 2, 1, 3, 1, false, &mut out), 2);
        let mut short = vec![0; 3];
        assert_eq!(gpu::convert(&[0; 8], 2, 1, 2, 1, false, &mut short), 2);
    }

    #[test]
    fn rust_ring_layout_matches_ring_h() {
        // SAFETY: a pure lookup.
        let fact = |what| unsafe { gpu::kgf_ring_layout(what) };
        assert_eq!(size_of::<ring::Header>(), fact(0));
        assert_eq!(offset_of!(ring::Header, published), fact(1));
        assert_eq!(offset_of!(ring::Header, latest), fact(2));
        assert_eq!(offset_of!(ring::Header, slots), fact(3));
        assert_eq!(size_of::<ring::Slot>(), fact(4));
        assert_eq!(ring::slot_bytes(1920, 1080), fact(5));
        assert_eq!(ring::MAGIC as usize, fact(6));
    }
}
