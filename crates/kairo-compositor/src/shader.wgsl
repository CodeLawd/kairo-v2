// One colour pipeline: premultiplied RGBA, blended as stored (sRGB-encoded
// values, as Chromium's compositor blends them), `One, OneMinusSrcAlpha`.
//
// A quad is drawn in "local" pixels and may be turned about a centre; the
// part of it that shows is a (rounded) clip box, so a picture that overflows
// its box (`object-fit: cover`, the background's blur cover) is cut to it.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var fill_texture: texture_2d<f32>;
@group(1) @binding(1) var fill_sampler: sampler;

struct Instance {
    // x, y, width, height of the quad in local pixels.
    @location(0) rect: vec4<f32>,
    // Premultiplied tint (gradient: the start colour).
    @location(1) color: vec4<f32>,
    // radius, opacity, mode (0 fill, 1 shadow, 2 gradient), shadow sigma.
    @location(2) params: vec4<f32>,
    // Visible box in local pixels: x, y, width, height.
    @location(3) clip: vec4<f32>,
    // Turn about (cx, cy): cos, sin.
    @location(4) turn: vec4<f32>,
    // Colour filter: hue (radians), saturate, brightness, contrast
    // (gradient: the end colour, premultiplied).
    @location(5) filt: vec4<f32>,
    // Bit flags (1 filter, 2 flip x, 4 flip y), gradient angle (radians).
    @location(6) extra: vec4<f32>,
};

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec4<f32>,
    @location(5) @interpolate(flat) clip: vec4<f32>,
    @location(6) @interpolate(flat) filt: vec4<f32>,
    @location(7) @interpolate(flat) extra: vec4<f32>,
};

@vertex
fn vs(@builtin(vertex_index) index: u32, instance: Instance) -> Varyings {
    // Two triangles: (0,0) (1,0) (0,1) / (0,1) (1,0) (1,1).
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let corner = corners[index];
    // A shadow paints past its box by three standard deviations.
    var grow = 0.0;
    if (instance.params.z > 0.5 && instance.params.z < 1.5) {
        grow = 3.0 * instance.params.w;
    }
    let origin = instance.rect.xy - vec2<f32>(grow);
    let size = instance.rect.zw + vec2<f32>(2.0 * grow);
    let local = origin + corner * size;
    // Turn about the centre: x' = x cos - y sin, y' = x sin + y cos (y down).
    let centre = instance.turn.xy;
    let angle = instance.turn.zw;
    let d = local - centre;
    let world = centre + vec2<f32>(d.x * angle.x - d.y * angle.y, d.x * angle.y + d.y * angle.x);
    let ndc = vec2<f32>(
        world.x / globals.target_size.x * 2.0 - 1.0,
        1.0 - world.y / globals.target_size.y * 2.0,
    );
    var uv = (local - instance.rect.xy) / instance.rect.zw;
    let flags = u32(instance.extra.x + 0.5);
    if ((flags & 2u) != 0u) { uv.x = 1.0 - uv.x; }
    if ((flags & 4u) != 0u) { uv.y = 1.0 - uv.y; }
    var out: Varyings;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.pixel = local;
    out.uv = uv;
    out.rect = instance.rect;
    out.color = instance.color;
    out.params = instance.params;
    out.clip = instance.clip;
    out.filt = instance.filt;
    out.extra = instance.extra;
    return out;
}

fn rounded_box_distance(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - half_size + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    return exp(-(x * x) / (2.0 * sigma * sigma)) / (2.5066282746 * sigma);
}

fn erf2(x: vec2<f32>) -> vec2<f32> {
    let s = sign(x);
    let a = abs(x);
    var r = vec2<f32>(1.0) + (vec2<f32>(0.278393) + (vec2<f32>(0.230389) + 0.078108 * (a * a)) * a) * a;
    r = r * r;
    return s - s / (r * r);
}

fn shadow_x(x: f32, y: f32, sigma: f32, corner: f32, half_size: vec2<f32>) -> f32 {
    let delta = min(half_size.y - corner - abs(y), 0.0);
    let curved = half_size.x - corner + sqrt(max(0.0, corner * corner - delta * delta));
    let integral = vec2<f32>(0.5) + 0.5 * erf2((vec2<f32>(x) + vec2<f32>(-curved, curved)) * (0.70710678 / sigma));
    return integral.y - integral.x;
}

// A blurred rounded box: the Gaussian integral along x in closed form,
// sampled along y.
fn rounded_box_shadow(rect: vec4<f32>, point: vec2<f32>, sigma: f32, corner: f32) -> f32 {
    let half_size = rect.zw * 0.5;
    let p = point - (rect.xy + half_size);
    let low = p.y - half_size.y;
    let high = p.y + half_size.y;
    let start = clamp(-3.0 * sigma, low, high);
    let end = clamp(3.0 * sigma, low, high);
    let step = (end - start) / 4.0;
    var y = start + step * 0.5;
    var value = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        value = value + shadow_x(p.x, p.y - y, sigma, corner, half_size) * gaussian(y, sigma) * step;
        y = y + step;
    }
    return value;
}

// The CSS filter functions of `mediaFilterCss`, in its order, on straight
// (unpremultiplied) colour with each stage clamped, as Chromium applies its
// colour matrices: hue-rotate, saturate, brightness, contrast.
fn color_filter(c: vec4<f32>, f: vec4<f32>) -> vec4<f32> {
    if (c.a <= 0.0) {
        return c;
    }
    var rgb = c.rgb / c.a;
    let hue = f.x;
    if (hue != 0.0) {
        let co = cos(hue);
        let si = sin(hue);
        let m = mat3x3<f32>(
            vec3<f32>(0.213 + co * 0.787 - si * 0.213, 0.213 - co * 0.213 + si * 0.143, 0.213 - co * 0.213 - si * 0.787),
            vec3<f32>(0.715 - co * 0.715 - si * 0.715, 0.715 + co * 0.285 + si * 0.140, 0.715 - co * 0.715 + si * 0.715),
            vec3<f32>(0.072 - co * 0.072 + si * 0.928, 0.072 - co * 0.072 - si * 0.283, 0.072 + co * 0.928 + si * 0.072),
        );
        rgb = clamp(m * rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    let s = f.y;
    if (s != 1.0) {
        let m = mat3x3<f32>(
            vec3<f32>(0.213 + 0.787 * s, 0.213 - 0.213 * s, 0.213 - 0.213 * s),
            vec3<f32>(0.715 - 0.715 * s, 0.715 + 0.285 * s, 0.715 - 0.715 * s),
            vec3<f32>(0.072 - 0.072 * s, 0.072 - 0.072 * s, 0.072 + 0.928 * s),
        );
        rgb = clamp(m * rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (f.z != 1.0) {
        rgb = clamp(rgb * f.z, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if (f.w != 1.0) {
        rgb = clamp((rgb - vec3<f32>(0.5)) * f.w + vec3<f32>(0.5), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(rgb * c.a, c.a);
}

@fragment
fn fs(v: Varyings) -> @location(0) vec4<f32> {
    // Sampled before any branch: WGSL requires uniform control flow here.
    let texel = textureSample(fill_texture, fill_sampler, clamp(v.uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let radius = v.params.x;
    let opacity = v.params.y;
    let mode = v.params.z;
    if (mode > 0.5 && mode < 1.5) {
        let sigma = max(v.params.w, 0.001);
        return v.color * rounded_box_shadow(v.rect, v.pixel, sigma, min(radius, min(v.rect.z, v.rect.w) * 0.5)) * opacity;
    }
    // The visible box (rounded), and the quad itself so a picture inside
    // its box (`contain`) has an anti-aliased edge of its own.
    let clip_half = v.clip.zw * 0.5;
    let d = rounded_box_distance(v.pixel - (v.clip.xy + clip_half), clip_half, radius);
    let quad_half = v.rect.zw * 0.5;
    let q = rounded_box_distance(v.pixel - (v.rect.xy + quad_half), quad_half, 0.0);
    let coverage = clamp(0.5 - max(d, q), 0.0, 1.0);
    if (mode > 1.5) {
        // CSS linear-gradient(angle): 0deg points up, clockwise. The gradient
        // line is as long as |w sin a| + |h cos a| and passes through the centre.
        let angle = v.extra.y;
        let dir = vec2<f32>(sin(angle), -cos(angle));
        let length_px = abs(v.rect.z * dir.x) + abs(v.rect.w * dir.y);
        let t = clamp(dot(v.pixel - (v.rect.xy + quad_half), dir) / max(length_px, 0.001) + 0.5, 0.0, 1.0);
        return mix(v.color, v.filt, t) * (coverage * opacity);
    }
    var c = texel * v.color;
    if ((u32(v.extra.x + 0.5) & 1u) != 0u) {
        c = color_filter(c, v.filt);
    }
    return c * (coverage * opacity);
}

// ─── Gaussian blur ────────────────────────────────────────────────────────────
// Separable, one direction per pass, over a texture whose border is already
// transparent (the caller pads it by three standard deviations), so an edge
// fades the way CSS `blur()` does. Pairs of taps are folded into one linear
// sample.

struct BlurParams {
    // Step of one texel along the pass direction, in uv units.
    step: vec2<f32>,
    sigma: f32,
    radius: f32,
};

@group(1) @binding(2) var<uniform> blur: BlurParams;

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn blur_vs(@builtin(vertex_index) index: u32) -> FullscreenOut {
    var corners = array<vec2<f32>, 3>(vec2<f32>(0.0, 0.0), vec2<f32>(2.0, 0.0), vec2<f32>(0.0, 2.0));
    let corner = corners[index];
    var out: FullscreenOut;
    out.position = vec4<f32>(corner.x * 2.0 - 1.0, 1.0 - corner.y * 2.0, 0.0, 1.0);
    out.uv = corner;
    return out;
}

@fragment
fn blur_fs(v: FullscreenOut) -> @location(0) vec4<f32> {
    let sigma = max(blur.sigma, 0.001);
    let radius = i32(blur.radius);
    let w0 = gaussian(0.0, sigma);
    var sum = textureSampleLevel(fill_texture, fill_sampler, v.uv, 0.0) * w0;
    var total = w0;
    for (var i = 1; i <= radius; i = i + 2) {
        let wa = gaussian(f32(i), sigma);
        var wb = 0.0;
        if (i + 1 <= radius) {
            wb = gaussian(f32(i + 1), sigma);
        }
        let w = wa + wb;
        let offset = (f32(i) * wa + f32(i + 1) * wb) / w;
        let at = blur.step * offset;
        sum = sum + (textureSampleLevel(fill_texture, fill_sampler, v.uv + at, 0.0)
            + textureSampleLevel(fill_texture, fill_sampler, v.uv - at, 0.0)) * w;
        total = total + 2.0 * w;
    }
    return sum / total;
}
