// Premultiplied RGBA frame to NDI UYVA (or, with `bgra`, premultiplied BGRA
// for the recorder): a UYVY plane (Rec.709 limited
// range) followed by a one-byte-per-pixel alpha plane.
//
// The maths is `uyvy()` in Electron's native/decklink/uyvy.h, in integers,
// so the output matches the CPU reference byte for byte. Premultiplied RGB
// is the colour composited on black, as the CPU path sends it. Scaling is
// the helper's nearest-neighbour integer mapping.
//
// One invocation writes four output pixels: two UYVY words and one word of
// the alpha plane (storage buffers are written a word at a time).

struct Params {
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    alpha: u32,
    // 1: plain BGRA words (the recorder's tap) instead of UYVA.
    // 2: BGRA averaged over each output pixel's source box, with straight
    //    alpha (the operator preview, drawn by GPUI).
    bgra: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> p: Params;

fn clamp8(x: i32) -> u32 {
    return u32(clamp(x, 0, 255));
}

fn luma(c: vec4<i32>) -> u32 {
    return clamp8(((47 * c.r + 157 * c.g + 16 * c.b + 128) >> 8u) + 16);
}

fn bytes_at(x: u32, sy: u32) -> vec4<i32> {
    let sx = x * p.src_w / p.dst_w;
    return vec4<i32>(round(textureLoad(src, vec2<u32>(sx, sy), 0) * 255.0));
}

// The mean of the source pixels under output pixel (x, y), sampling at most
// 8 × 8 of them evenly, un-premultiplied: GPUI draws images with straight
// alpha. Integer maths, so the CPU reference matches it exactly.
fn box_at(x: u32, y: u32) -> vec4<u32> {
    let x0 = x * p.src_w / p.dst_w;
    let x1 = max((x + 1u) * p.src_w / p.dst_w, x0 + 1u);
    let y0 = y * p.src_h / p.dst_h;
    let y1 = max((y + 1u) * p.src_h / p.dst_h, y0 + 1u);
    let nx = min(x1 - x0, 8u);
    let ny = min(y1 - y0, 8u);
    var sum = vec4<u32>(0u);
    for (var j = 0u; j < ny; j = j + 1u) {
        let sy = y0 + j * (y1 - y0) / ny;
        for (var i = 0u; i < nx; i = i + 1u) {
            let sx = x0 + i * (x1 - x0) / nx;
            sum = sum + vec4<u32>(round(textureLoad(src, vec2<u32>(sx, sy), 0) * 255.0));
        }
    }
    let n = nx * ny;
    let c = (sum + vec4<u32>(n / 2u)) / n;
    if (c.a == 0u) {
        return vec4<u32>(0u);
    }
    if (c.a == 255u) {
        return c;
    }
    let rgb = min((c.rgb * 255u + vec3<u32>(c.a / 2u)) / c.a, vec3<u32>(255u));
    return vec4<u32>(rgb, c.a);
}

// One UYVY group (two pixels) as a little-endian word: U, Y0, V, Y1.
fn group(a: vec4<i32>, c: vec4<i32>) -> u32 {
    let r = (a.r + c.r) / 2;
    let g = (a.g + c.g) / 2;
    let b = (a.b + c.b) / 2;
    let u = clamp8(((-26 * r - 87 * g + 112 * b + 128) >> 8u) + 128);
    let v = clamp8(((112 * r - 102 * g - 10 * b + 128) >> 8u) + 128);
    return u | (luma(a) << 8u) | (v << 16u) | (luma(c) << 24u);
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x * 4u;
    let y = gid.y;
    if (x >= p.dst_w || y >= p.dst_h) {
        return;
    }
    if (p.bgra == 2u) {
        for (var k = 0u; k < 4u; k = k + 1u) {
            let c = box_at(x + k, y);
            dst[y * p.dst_w + x + k] = u32(c.b) | (u32(c.g) << 8u) | (u32(c.r) << 16u) | (u32(c.a) << 24u);
        }
        return;
    }
    let sy = y * p.src_h / p.dst_h;
    if (p.bgra != 0u) {
        for (var k = 0u; k < 4u; k = k + 1u) {
            let c = bytes_at(x + k, sy);
            dst[y * p.dst_w + x + k] = u32(c.b) | (u32(c.g) << 8u) | (u32(c.r) << 16u) | (u32(c.a) << 24u);
        }
        return;
    }
    let px0 = bytes_at(x, sy);
    let px1 = bytes_at(x + 1u, sy);
    let px2 = bytes_at(x + 2u, sy);
    let px3 = bytes_at(x + 3u, sy);
    let word = (y * p.dst_w + x) / 2u;
    dst[word] = group(px0, px1);
    dst[word + 1u] = group(px2, px3);
    if (p.alpha != 0u) {
        let plane = p.dst_w * p.dst_h / 2u;
        dst[plane + (y * p.dst_w + x) / 4u] = u32(px0.a) | (u32(px1.a) << 8u) | (u32(px2.a) << 16u) | (u32(px3.a) << 24u);
    }
}
