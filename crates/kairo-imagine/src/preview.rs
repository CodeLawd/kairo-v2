//! Preview art for the style templates, painted here rather than shipped
//! as files: each style gets a small abstract piece in its own manner
//! (soft glows for worship, leaded panes for stained glass, washes for
//! watercolor...). Deterministic, so a template always looks the same.

use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point,
    RadialGradient, Rect, Shader, SpreadMode, Stroke, Transform,
};

/// A tiny xorshift generator: the same art on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

type Rgb = (u8, u8, u8);

fn rgba(c: Rgb, a: f32) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, (a.clamp(0., 1.) * 255.).round() as u8)
}

fn paint(shader: Shader<'static>) -> Paint<'static> {
    Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    }
}

fn solid(c: Color) -> Paint<'static> {
    paint(Shader::SolidColor(c))
}

fn whole(pm: &Pixmap) -> Rect {
    Rect::from_xywh(0., 0., pm.width() as f32, pm.height() as f32).expect("a slide-sized rect")
}

/// A top-to-bottom wash.
fn vertical(pm: &mut Pixmap, top: Rgb, bottom: Rgb) {
    let h = pm.height() as f32;
    if let Some(shader) = LinearGradient::new(
        Point::from_xy(0., 0.),
        Point::from_xy(0., h),
        vec![
            GradientStop::new(0., rgba(top, 1.)),
            GradientStop::new(1., rgba(bottom, 1.)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        let r = whole(pm);
        pm.fill_rect(r, &paint(shader), Transform::identity(), None);
    }
}

/// A soft round glow fading to nothing at `r`.
fn glow(pm: &mut Pixmap, x: f32, y: f32, r: f32, c: Rgb, a: f32) {
    if let Some(shader) = RadialGradient::new(
        Point::from_xy(x, y),
        Point::from_xy(x, y),
        r,
        vec![
            GradientStop::new(0., rgba(c, a)),
            GradientStop::new(0.45, rgba(c, a * 0.45)),
            GradientStop::new(1., rgba(c, 0.)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) && let Some(rect) = Rect::from_xywh(x - r, y - r, r * 2., r * 2.)
    {
        pm.fill_rect(rect, &paint(shader), Transform::identity(), None);
    }
}

fn circle(pm: &mut Pixmap, x: f32, y: f32, r: f32, c: Color) {
    if let Some(path) = PathBuilder::from_circle(x, y, r) {
        pm.fill_path(
            &path,
            &solid(c),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn polygon(points: &[(f32, f32)]) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let (first, rest) = points.split_first()?;
    pb.move_to(first.0, first.1);
    for p in rest {
        pb.line_to(p.0, p.1);
    }
    pb.close();
    pb.finish()
}

/// Darkened edges, so the eye settles in the middle.
fn vignette(pm: &mut Pixmap, strength: f32) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    if let Some(shader) = RadialGradient::new(
        Point::from_xy(w / 2., h / 2.),
        Point::from_xy(w / 2., h / 2.),
        w.max(h) * 0.75,
        vec![
            GradientStop::new(0.55, Color::from_rgba8(0, 0, 0, 0)),
            GradientStop::new(1., rgba((0, 0, 0), strength)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        let r = whole(pm);
        pm.fill_rect(r, &paint(shader), Transform::identity(), None);
    }
}

/// Film grain, so flat washes read as a surface.
fn grain(pm: &mut Pixmap, amount: f32, seed: u64) {
    let mut rng = Rng(seed | 1);
    for px in pm.data_mut().as_chunks_mut::<4>().0 {
        let n = (rng.next() - 0.5) * 2. * amount;
        let alpha = px[3] as f32;
        for c in &mut px[..3] {
            *c = (*c as f32 + n).clamp(0., alpha) as u8;
        }
    }
}

fn worship(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (22, 14, 48), (38, 18, 70));
    glow(pm, w * 0.28, h * 0.32, w * 0.8, (124, 77, 255), 0.55);
    glow(pm, w * 0.8, h * 0.62, w * 0.65, (214, 66, 160), 0.42);
    glow(pm, w * 0.5, h * 0.98, w * 0.6, (255, 168, 92), 0.38);
    // A shaft of light from above.
    if let Some(path) = polygon(&[(w * 0.46, 0.), (w * 0.58, 0.), (w * 0.9, h), (w * 0.2, h)])
        && let Some(shader) = LinearGradient::new(
            Point::from_xy(0., 0.),
            Point::from_xy(0., h),
            vec![
                GradientStop::new(0., rgba((255, 240, 255), 0.09)),
                GradientStop::new(1., rgba((255, 240, 255), 0.)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        )
    {
        pm.fill_path(
            &path,
            &paint(shader),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    // Floating points of light.
    let mut rng = Rng(0xb0_4e);
    for _ in 0..14 {
        let (x, y) = (rng.range(0.05, 0.95) * w, rng.range(0.1, 0.9) * h);
        let r = rng.range(0.015, 0.05) * w;
        glow(pm, x, y, r, (255, 226, 250), rng.range(0.25, 0.6));
    }
    vignette(pm, 0.4);
    grain(pm, 6., 11);
}

fn sermon(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (14, 14, 20), (40, 22, 16));
    glow(pm, w * 0.5, h * 0.56, w * 0.75, (255, 112, 40), 0.55);
    if let Some(shader) = RadialGradient::new(
        Point::from_xy(w * 0.5, h * 0.5),
        Point::from_xy(w * 0.5, h * 0.5),
        w * 0.22,
        vec![
            GradientStop::new(0., rgba((255, 214, 150), 1.)),
            GradientStop::new(1., rgba((244, 110, 46), 1.)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    ) && let Some(path) = PathBuilder::from_circle(w * 0.5, h * 0.5, w * 0.22)
    {
        pm.fill_path(
            &path,
            &paint(shader),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    // Ridges in front of the sun.
    let ridge = |pm: &mut Pixmap, base: f32, peaks: &[(f32, f32)], c: Rgb| {
        let mut pts = vec![(0., h), (0., h * base)];
        pts.extend(peaks.iter().map(|(x, y)| (w * x, h * y)));
        pts.extend([(w, h * base), (w, h)]);
        if let Some(path) = polygon(&pts) {
            pm.fill_path(
                &path,
                &solid(rgba(c, 1.)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    };
    ridge(
        pm,
        0.66,
        &[(0.18, 0.6), (0.34, 0.64), (0.55, 0.57), (0.78, 0.63)],
        (58, 26, 20),
    );
    ridge(
        pm,
        0.74,
        &[(0.12, 0.7), (0.4, 0.76), (0.66, 0.69), (0.9, 0.73)],
        (18, 12, 14),
    );
    // Room for a title.
    let bar = |pm: &mut Pixmap, y: f32, wf: f32, a: f32| {
        if let Some(r) = Rect::from_xywh(w * 0.1, h * y, w * wf, h * 0.018) {
            pm.fill_rect(
                r,
                &solid(rgba((255, 244, 232), a)),
                Transform::identity(),
                None,
            );
        }
    };
    bar(pm, 0.84, 0.46, 0.9);
    bar(pm, 0.875, 0.28, 0.45);
    vignette(pm, 0.45);
    grain(pm, 7., 23);
}

fn scripture(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (190, 208, 226), (244, 236, 222));
    glow(pm, w * 0.76, h * 0.22, w * 0.55, (255, 240, 214), 0.75);
    let layers: [(f32, Rgb, f32); 4] = [
        (0.52, (172, 188, 206), 0.7),
        (0.62, (138, 158, 182), 0.8),
        (0.72, (104, 124, 152), 0.9),
        (0.82, (74, 92, 120), 1.),
    ];
    for (i, (y, c, a)) in layers.into_iter().enumerate() {
        let mut pb = PathBuilder::new();
        let shift = i as f32 * 0.17;
        pb.move_to(0., h);
        pb.line_to(0., h * y);
        pb.cubic_to(
            w * (0.25 + shift * 0.3),
            h * (y - 0.07),
            w * (0.5 - shift * 0.2),
            h * (y + 0.06),
            w,
            h * (y - 0.03),
        );
        pb.line_to(w, h);
        pb.close();
        if let Some(path) = pb.finish() {
            pm.fill_path(
                &path,
                &solid(rgba(c, a)),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        // Mist settles between the ridges.
        glow(pm, w * 0.5, h * (y + 0.02), w * 0.7, (248, 244, 236), 0.18);
    }
    grain(pm, 4., 37);
}

fn announcement(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (255, 112, 82), (247, 86, 72));
    circle(pm, w * 0.8, h * 0.24, w * 0.34, rgba((255, 206, 84), 1.));
    if let Some(r) = Rect::from_xywh(-w * 0.15, h * 0.58, w * 0.75, h * 0.52)
        && let Some(path) = rounded_rect(r, w * 0.08)
    {
        pm.fill_path(
            &path,
            &solid(rgba((34, 160, 150), 1.)),
            FillRule::Winding,
            Transform::from_rotate_at(-12., r.x() + r.width() / 2., r.y() + r.height() / 2.),
            None,
        );
    }
    if let Some(path) = PathBuilder::from_circle(w * 0.24, h * 0.27, w * 0.13) {
        let stroke = Stroke {
            width: w * 0.035,
            ..Stroke::default()
        };
        pm.stroke_path(
            &path,
            &solid(rgba((255, 250, 240), 1.)),
            &stroke,
            Transform::identity(),
            None,
        );
    }
    for i in 0..5 {
        for j in 0..5 {
            circle(
                pm,
                w * (0.68 + i as f32 * 0.06),
                h * (0.66 + j as f32 * 0.05),
                w * 0.011,
                rgba((30, 36, 72), 0.85),
            );
        }
    }
    grain(pm, 5., 41);
}

fn rounded_rect(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x(), r.y(), r.width(), r.height());
    let k = radius * 0.5523;
    let mut pb = PathBuilder::new();
    pb.move_to(x + radius, y);
    pb.line_to(x + w - radius, y);
    pb.cubic_to(
        x + w - radius + k,
        y,
        x + w,
        y + radius - k,
        x + w,
        y + radius,
    );
    pb.line_to(x + w, y + h - radius);
    pb.cubic_to(
        x + w,
        y + h - radius + k,
        x + w - radius + k,
        y + h,
        x + w - radius,
        y + h,
    );
    pb.line_to(x + radius, y + h);
    pb.cubic_to(
        x + radius - k,
        y + h,
        x,
        y + h - radius + k,
        x,
        y + h - radius,
    );
    pb.line_to(x, y + radius);
    pb.cubic_to(x, y + radius - k, x + radius - k, y, x + radius, y);
    pb.close();
    pb.finish()
}

fn watercolor(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (248, 244, 236), (242, 236, 226));
    let mut rng = Rng(0x5eed_cafe);
    let palette: [Rgb; 5] = [
        (70, 132, 200),
        (96, 176, 188),
        (226, 120, 150),
        (244, 176, 100),
        (132, 104, 190),
    ];
    for i in 0..8 {
        let c = palette[i % palette.len()];
        let (cx, cy) = (rng.range(0.05, 0.95) * w, rng.range(0.05, 0.95) * h);
        let r = rng.range(0.22, 0.42) * w;
        // A wash: pigment pools toward a drier, darker rim.
        for _ in 0..7 {
            let (dx, dy) = (rng.range(-0.4, 0.4) * r, rng.range(-0.4, 0.4) * r);
            let rr = r * rng.range(0.45, 0.8);
            let (x, y) = (cx + dx, cy + dy);
            if let Some(shader) = RadialGradient::new(
                Point::from_xy(x, y),
                Point::from_xy(x, y),
                rr,
                vec![
                    GradientStop::new(0., rgba(c, 0.07)),
                    GradientStop::new(0.82, rgba(c, 0.13)),
                    GradientStop::new(0.92, rgba(c, 0.05)),
                    GradientStop::new(1., rgba(c, 0.)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            ) && let Some(rect) = Rect::from_xywh(x - rr, y - rr, rr * 2., rr * 2.)
            {
                pm.fill_rect(rect, &paint(shader), Transform::identity(), None);
            }
        }
    }
    grain(pm, 10., 53);
}

fn stained_glass(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (20, 16, 20), (20, 16, 20));
    let (cols, rows) = (5usize, 6usize);
    let mut rng = Rng(0x061a_55e5);
    let mut grid = vec![vec![(0f32, 0f32); cols + 1]; rows + 1];
    for (j, row) in grid.iter_mut().enumerate() {
        for (i, p) in row.iter_mut().enumerate() {
            let jitter = |edge: bool, rng: &mut Rng| if edge { 0. } else { rng.range(-0.32, 0.32) };
            let dx = jitter(i == 0 || i == cols, &mut rng) * w / cols as f32;
            let dy = jitter(j == 0 || j == rows, &mut rng) * h / rows as f32;
            *p = (
                i as f32 * w / cols as f32 + dx,
                j as f32 * h / rows as f32 + dy,
            );
        }
    }
    let jewels: [Rgb; 6] = [
        (176, 32, 62),
        (32, 74, 176),
        (22, 132, 92),
        (232, 164, 44),
        (112, 52, 152),
        (24, 132, 152),
    ];
    let lead = Stroke {
        width: w * 0.009,
        line_join: tiny_skia::LineJoin::Round,
        ..Stroke::default()
    };
    for j in 0..rows {
        for i in 0..cols {
            let (a, b, c, d) = (
                grid[j][i],
                grid[j][i + 1],
                grid[j + 1][i + 1],
                grid[j + 1][i],
            );
            let tris = if rng.next() < 0.5 {
                [[a, b, c], [a, c, d]]
            } else {
                [[a, b, d], [b, c, d]]
            };
            for tri in tris {
                let color = jewels[(rng.next() * jewels.len() as f32) as usize % jewels.len()];
                if let Some(path) = polygon(&tri) {
                    let light = |c: u8, t: f32| (c as f32 + (255. - c as f32) * t) as u8;
                    let dark = |c: u8, t: f32| (c as f32 * t) as u8;
                    let lit = rng.range(0.1, 0.4);
                    let deep = rng.range(0.55, 0.85);
                    let glass = LinearGradient::new(
                        Point::from_xy(tri[0].0, tri[0].1),
                        Point::from_xy(tri[2].0, tri[2].1),
                        vec![
                            GradientStop::new(
                                0.,
                                rgba(
                                    (
                                        light(color.0, lit),
                                        light(color.1, lit),
                                        light(color.2, lit),
                                    ),
                                    1.,
                                ),
                            ),
                            GradientStop::new(
                                1.,
                                rgba(
                                    (
                                        dark(color.0, deep),
                                        dark(color.1, deep),
                                        dark(color.2, deep),
                                    ),
                                    1.,
                                ),
                            ),
                        ],
                        SpreadMode::Pad,
                        Transform::identity(),
                    );
                    if let Some(shader) = glass {
                        pm.fill_path(
                            &path,
                            &paint(shader),
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                    pm.stroke_path(
                        &path,
                        &solid(rgba((18, 14, 16), 1.)),
                        &lead,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
    }
    // Light pouring through the glass.
    glow(pm, w * 0.5, h * 0.12, w * 0.85, (255, 236, 190), 0.42);
    glow(pm, w * 0.5, h * 0.08, w * 0.3, (255, 250, 235), 0.35);
    vignette(pm, 0.5);
    grain(pm, 5., 61);
}

fn cinematic(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (8, 16, 26), (20, 36, 46));
    let (sx, sy) = (w * 0.12, -h * 0.08);
    let mut rng = Rng(0xc1_4e);
    for k in 0..5 {
        let spread = 0.12 + k as f32 * 0.2 + rng.range(-0.05, 0.05);
        let width = rng.range(0.08, 0.2);
        let (x1, x2) = (w * spread, w * (spread + width));
        if let Some(path) = polygon(&[(sx, sy), (x1 + w * 0.3, h), (x2 + w * 0.3, h)])
            && let Some(shader) = LinearGradient::new(
                Point::from_xy(sx, sy),
                Point::from_xy(x1 + w * 0.3, h),
                vec![
                    GradientStop::new(0., rgba((255, 230, 190), 0.17)),
                    GradientStop::new(1., rgba((255, 230, 190), 0.)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            )
        {
            pm.fill_path(
                &path,
                &paint(shader),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    glow(pm, sx, sy + h * 0.06, w * 0.6, (255, 206, 150), 0.55);
    for _ in 0..70 {
        let (x, y) = (rng.range(0.1, 0.9) * w, rng.range(0.1, 0.9) * h);
        circle(
            pm,
            x,
            y,
            rng.range(0.6, 1.8),
            rgba((255, 236, 210), rng.range(0.15, 0.5)),
        );
    }
    vignette(pm, 0.6);
    grain(pm, 7., 71);
}

fn gradient(pm: &mut Pixmap) {
    let (w, h) = (pm.width() as f32, pm.height() as f32);
    vertical(pm, (14, 36, 34), (10, 26, 30));
    glow(pm, w * 0.18, h * 0.2, w * 0.95, (58, 196, 140), 0.85);
    glow(pm, w * 0.88, h * 0.42, w * 0.7, (184, 228, 96), 0.6);
    glow(pm, w * 0.5, h * 1.02, w * 0.85, (24, 118, 164), 0.75);
    grain(pm, 8., 83);
}

/// Preview art for a template by its key, as a PNG of `width` x
/// `height`; `None` for an unknown key.
pub fn template_preview(key: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    let mut pm = Pixmap::new(width, height)?;
    match key {
        "worship" => worship(&mut pm),
        "sermon" => sermon(&mut pm),
        "scripture" => scripture(&mut pm),
        "announcement" => announcement(&mut pm),
        "watercolor" => watercolor(&mut pm),
        "stained-glass" => stained_glass(&mut pm),
        "cinematic" => cinematic(&mut pm),
        "gradient" => gradient(&mut pm),
        _ => return None,
    }
    pm.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_template_has_its_own_art() {
        let keys = [
            "worship",
            "sermon",
            "scripture",
            "announcement",
            "watercolor",
            "stained-glass",
            "cinematic",
            "gradient",
        ];
        let mut seen = std::collections::HashSet::new();
        for key in keys {
            let png = template_preview(key, 80, 100).expect(key);
            let img = image::load_from_memory(&png).unwrap();
            assert_eq!((img.width(), img.height()), (80, 100));
            assert!(seen.insert(png), "{key} differs from the others");
        }
        assert_eq!(
            template_preview("worship", 40, 50),
            template_preview("worship", 40, 50),
            "the same every time"
        );
        assert!(template_preview("nope", 10, 10).is_none());
    }
}
