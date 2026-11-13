// Decoded video planes to premultiplied RGBA (prompt 055, from spike 004).
// Luma and interleaved chroma arrive as integer textures: NV12 (8-bit) or
// P010 (the top ten bits of 16). An optional 8-bit alpha plane rides
// alongside. The output is the compositor's premultiplied RGBA.
struct Color {
    kr: f32,
    kb: f32,
    y_offset: f32,
    y_scale: f32,
    c_offset: f32,
    c_scale: f32,
    shift: u32,
    alpha: u32,
}

@group(0) @binding(0) var luma: texture_2d<u32>;
@group(0) @binding(1) var chroma: texture_2d<u32>;
@group(0) @binding(2) var opacity: texture_2d<u32>;
@group(0) @binding(3) var rgba: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(4) var<uniform> color: Color;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(rgba)) {
        return;
    }
    let pixel = vec2<i32>(id.xy);
    let y_code = textureLoad(luma, pixel, 0).r >> color.shift;
    let uv_code = textureLoad(chroma, pixel / 2, 0).rg >> vec2<u32>(color.shift);
    let y = (f32(y_code) - color.y_offset) * color.y_scale;
    let uv = (vec2<f32>(uv_code) - vec2<f32>(color.c_offset)) * color.c_scale;
    let kg = 1.0 - color.kr - color.kb;
    let r = y + 2.0 * (1.0 - color.kr) * uv.y;
    let b = y + 2.0 * (1.0 - color.kb) * uv.x;
    let g = (y - color.kr * r - color.kb * b) / kg;
    var alpha = 1.0;
    if color.alpha != 0u {
        alpha = f32(textureLoad(opacity, pixel, 0).r) / 255.0;
    }
    let straight = clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(rgba, pixel, vec4<f32>(straight * alpha, alpha));
}
