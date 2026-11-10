// One colour pipeline: premultiplied RGBA, blended as stored (sRGB-encoded
// values, as Chromium's compositor blends them), `One, OneMinusSrcAlpha`.

struct Globals {
    target_size: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var fill_texture: texture_2d<f32>;
@group(1) @binding(1) var fill_sampler: sampler;

struct Instance {
    // x, y, width, height in target pixels.
    @location(0) rect: vec4<f32>,
    // Premultiplied tint.
    @location(1) color: vec4<f32>,
    // radius, opacity, mode (0 fill, 1 shadow), shadow sigma.
    @location(2) params: vec4<f32>,
};

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) pixel: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) params: vec4<f32>,
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
    if (instance.params.z > 0.5) {
        grow = 3.0 * instance.params.w;
    }
    let origin = instance.rect.xy - vec2<f32>(grow);
    let size = instance.rect.zw + vec2<f32>(2.0 * grow);
    let pixel = origin + corner * size;
    let ndc = vec2<f32>(
        pixel.x / globals.target_size.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.target_size.y * 2.0,
    );
    var out: Varyings;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.pixel = pixel;
    out.uv = (pixel - instance.rect.xy) / instance.rect.zw;
    out.rect = instance.rect;
    out.color = instance.color;
    out.params = instance.params;
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

@fragment
fn fs(v: Varyings) -> @location(0) vec4<f32> {
    // Sampled before any branch: WGSL requires uniform control flow here.
    let texel = textureSample(fill_texture, fill_sampler, clamp(v.uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    let radius = v.params.x;
    let opacity = v.params.y;
    if (v.params.z > 0.5) {
        let sigma = max(v.params.w, 0.001);
        return v.color * rounded_box_shadow(v.rect, v.pixel, sigma, min(radius, min(v.rect.z, v.rect.w) * 0.5)) * opacity;
    }
    let half_size = v.rect.zw * 0.5;
    let d = rounded_box_distance(v.pixel - (v.rect.xy + half_size), half_size, radius);
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    return texel * v.color * (coverage * opacity);
}
