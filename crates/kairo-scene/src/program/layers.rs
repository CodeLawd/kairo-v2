//! `renderMessageHTML`, `renderPropsHTML`, `renderTimerHTML`,
//! `renderPollHTML`, `renderQuestionHTML` and `renderLogoHTML` as plans.

use super::*;
use crate::LINE_HEIGHT_NORMAL;
use crate::color::Rgba;
use crate::media::{BoxFit, decode_picture};
use crate::scene::{Align, Rect, Vertical};
use base64::Engine as _;
use kairo_domain::audience::PollView;
use kairo_domain::js;
use kairo_domain::program::{
    ProgramLayer, ProgramLogo, ProgramMessageStyle, ProgramProp, ProgramQuestion,
    ProgramQuestionFont, ProgramQuestionStyle, PropAlign, PropImageFit, PropObject, PropShape,
    PropTimerView, QuestionAlign, QuestionPosition, TopOrBottom,
};

const BLACK: Rgba = Rgba::new(0, 0, 0, 255);

/// Draws one program layer on a `frame` (1920 wide, the output's height).
pub fn plan_layer(ts: &mut Typesetter, layer: &ProgramLayer, frame: (u32, u32)) -> LayerPlan {
    let mut plan = LayerPlan::default();
    match layer {
        ProgramLayer::Message { text, style } => message(ts, &mut plan, text, style, frame),
        ProgramLayer::Props { props, timer } => {
            for prop in props {
                prop_plan(ts, &mut plan, prop, timer, frame);
            }
        }
        ProgramLayer::Timer { prop, timer } => prop_plan(ts, &mut plan, prop, timer, frame),
        ProgramLayer::Poll { poll, style } => poll_plan(ts, &mut plan, poll, style, frame),
        ProgramLayer::Question { question, style } => {
            question_plan(ts, &mut plan, question, style, frame)
        }
        ProgramLayer::Logo { logo } => logo_plan(&mut plan, logo, frame),
    }
    plan
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn full(frame: (u32, u32)) -> Rect {
    rect(0.0, 0.0, frame.0 as f32, frame.1 as f32)
}

// ─── Message ──────────────────────────────────────────────────────────────

/// `renderMessageHTML`: a bar at the top or bottom, centred words, or a
/// ticker that enters from the right edge and runs until it has left.
fn message(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    text: &str,
    style: &ProgramMessageStyle,
    frame: (u32, u32),
) {
    let body = js::trim(text);
    if body.is_empty() {
        return;
    }
    let fs = style.font_size_px as f32;
    let py = js::math_round(style.font_size_px * 0.45) as f32;
    let px = js::math_round(style.font_size_px * 0.8) as f32;
    let width = frame.0 as f32;
    let inner = width - 2.0 * px;
    let mut look = TextLook::new(SANS, fs, 600.0, 1.25, css(&style.color).unwrap_or(BLACK));
    let background = css(&style.background);
    let place = |h: f32| match style.position {
        TopOrBottom::Top => 0.0,
        TopOrBottom::Bottom => frame.1 as f32 - h,
    };
    if style.scroll {
        // `body.replace(/\s*\n\s*/g, '   ·   ')`, then `nowrap` collapses it.
        let line = body
            .split('\n')
            .map(js::trim)
            .collect::<Vec<_>>()
            .join("   ·   ");
        let seconds = (js::math_round(js::utf16_len(body) as f64 * 0.22)).max(8.0);
        let bar_h = 1.25 * fs + 2.0 * py;
        let y = place(bar_h);
        let bar = rect(0.0, y, width, bar_h);
        if let Some(c) = background {
            plan.push(box_tile(bar, Corners::Px(0.0), c, frame));
        }
        let (tw, _) = measure(ts, &look, &line, Space::NoWrap, inner);
        // `padding-left: 100%` puts the words one content width to the right.
        let start = px + inner;
        let lane = rect(start, y + py, tw, 1.25 * fs);
        // Drawn on a frame wide enough for where it starts.
        let wide = (frame.0 * 2 + tw.ceil() as u32 + 64, frame.1);
        if let Some(tile) = text_tile(
            ts,
            &look,
            &line,
            Space::NoWrap,
            lane,
            Vertical::Top,
            false,
            wide,
        ) {
            plan.pieces.push(Piece::Moving {
                tile,
                clip: bar,
                // `translateX(-100%)` of the inline block: padding plus words.
                distance: -(inner + tw),
                period_ms: (seconds * 1000.0) as u32,
                alternate: false,
            });
        }
        return;
    }
    look.align = Align::Center;
    let (_, h) = measure(ts, &look, body, Space::Breaks, inner);
    let bar_h = h + 2.0 * py;
    let y = place(bar_h);
    let bar = rect(0.0, y, width, bar_h);
    if let Some(c) = background {
        plan.push(box_tile(bar, Corners::Px(0.0), c, frame));
    }
    let tile = text_tile(
        ts,
        &look,
        body,
        Space::Breaks,
        rect(px, y + py, inner, h),
        Vertical::Top,
        false,
        frame,
    );
    plan.push(tile.and_then(|t| clip_tile(t, bar)));
}

// ─── Props and the timer design ───────────────────────────────────────────

/// `pct`: a share of the frame to two decimals of a percent.
fn pct(share: f64, of: u32) -> f32 {
    (js::math_round(share * 10_000.0) / 100.0 / 100.0 * f64::from(of)) as f32
}

fn align_of(align: PropAlign) -> Align {
    match align {
        PropAlign::Left => Align::Left,
        PropAlign::Center => Align::Center,
        PropAlign::Right => Align::Right,
    }
}

/// `renderPropHTML`: the background over the whole frame, then each object.
fn prop_plan(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    prop: &ProgramProp,
    timer: &PropTimerView,
    frame: (u32, u32),
) {
    if let Some(c) = prop.background.as_deref().and_then(css) {
        plan.push(box_tile(full(frame), Corners::Px(0.0), c, frame));
    }
    for object in &prop.objects {
        object_plan(ts, plan, object, timer, frame);
    }
}

fn object_box(f: &kairo_domain::program::PropFrame, frame: (u32, u32)) -> Rect {
    rect(
        pct(f.x, frame.0),
        pct(f.y, frame.1),
        pct(f.w, frame.0),
        pct(f.h, frame.1),
    )
}

fn opacity_of(v: f64) -> f32 {
    if v.is_nan() {
        1.0
    } else {
        v.clamp(0.0, 1.0) as f32
    }
}

/// `renderPropObjectHTML`.
fn object_plan(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    object: &PropObject,
    timer: &PropTimerView,
    frame: (u32, u32),
) {
    match object {
        PropObject::Text {
            frame: f,
            text,
            font_size_px,
            color,
            bold,
            align,
            ..
        } => {
            let b = object_box(f, frame);
            let mut look = TextLook::new(
                SANS,
                *font_size_px as f32,
                if *bold { 700.0 } else { 400.0 },
                1.2,
                css(color).unwrap_or(BLACK),
            );
            look.align = align_of(*align);
            // A flex box centres its one child; `overflow: hidden` cuts it.
            let tile = text_tile(
                ts,
                &look,
                text,
                Space::Pre,
                b,
                Vertical::Middle,
                true,
                frame,
            );
            plan.push(tile.map(|t| fade(t, opacity_of(f.opacity))));
        }
        PropObject::Timer {
            frame: f,
            font_size_px,
            color,
            overrun_color,
            bold,
            align,
            ..
        } => {
            let mut look = TextLook::new(
                SANS,
                *font_size_px as f32,
                if *bold { 700.0 } else { 400.0 },
                1.2,
                BLACK,
            );
            look.align = align_of(*align);
            look.tabular = true;
            plan.pieces.push(Piece::Live(Live {
                kind: LiveKind::Timer {
                    run: TimerRun {
                        duration_sec: timer.duration_sec,
                        ends_at: timer.ends_at,
                        remaining_sec: timer.remaining_sec,
                        rollover: timer.rollover,
                    },
                    // `data-color` and `data-overrun-color`, with the
                    // ticker's own fallbacks.
                    color: css(color).unwrap_or(Rgba::new(255, 255, 255, 255)),
                    overrun: css(overrun_color).unwrap_or(Rgba::new(0xf8, 0x71, 0x71, 255)),
                },
                text: look,
                rect: object_box(f, frame),
                vertical: Vertical::Middle,
                clipped: true,
                opacity: opacity_of(f.opacity),
                frame,
            }));
        }
        PropObject::Shape {
            frame: f,
            shape,
            fill,
            radius_px,
            ..
        } => {
            let corners = match shape {
                PropShape::Ellipse => Corners::Ellipse,
                PropShape::Rectangle => Corners::Px(*radius_px as f32),
            };
            if let Some(c) = css(fill) {
                let tile = box_tile(object_box(f, frame), corners, c, frame);
                plan.push(tile.map(|t| fade(t, opacity_of(f.opacity))));
            }
        }
        PropObject::Image {
            frame: f,
            media_path,
            fit,
            ..
        } => {
            if !media_path.is_empty() {
                plan.pieces.push(Piece::Picture {
                    path: media_path.clone(),
                    rect: object_box(f, frame),
                    fit: fit_of(*fit == PropImageFit::Cover),
                    opacity: opacity_of(f.opacity),
                });
            }
        }
    }
}

// ─── Poll and question cards ──────────────────────────────────────────────

fn font_of(font: ProgramQuestionFont) -> &'static str {
    match font {
        ProgramQuestionFont::Serif => SERIF,
        ProgramQuestionFont::Mono => MONO,
        ProgramQuestionFont::Sans => SANS,
    }
}

/// `Math.round(1760 * (widthPct / 100))`.
fn card_max_width(style: &ProgramQuestionStyle) -> f32 {
    js::math_round(1760.0 * (style.width_pct / 100.0)) as f32
}

/// Text drawn now, kept for a card's clip.
fn texts(plan: &mut LayerPlan, tiles: Vec<Option<Tile>>, clip: Option<Rect>) {
    for tile in tiles.into_iter().flatten() {
        match clip {
            Some(c) => plan.push(clip_tile(tile, c)),
            None => plan.push(Some(tile)),
        }
    }
}

/// `renderPollHTML`: a card in the middle of the frame inset by 8 %, the
/// title, one row and bar per option, and the voter count.
fn poll_plan(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    poll: &PollView,
    style: &ProgramQuestionStyle,
    frame: (u32, u32),
) {
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let area = rect(fw * 0.08, fh * 0.08, fw * 0.84, fh * 0.84);
    let card_w = area.width.min(card_max_width(style)).max(0.0);
    let inner = (card_w - 96.0).max(0.0);
    let family = font_of(style.font_family);
    let color = css(&style.color).unwrap_or(BLACK);
    let fs = style.font_size_px as f32;
    let base = js::math_round(style.font_size_px * 0.55) as f32;
    let title_look = TextLook::new(family, fs, 700.0, LINE_HEIGHT_NORMAL, color);
    let body_look = TextLook::new(family, base, 400.0, LINE_HEIGHT_NORMAL, color);
    let foot_look = TextLook::new(family, 24.0, 400.0, LINE_HEIGHT_NORMAL, color);

    // Lay out first, relative to the content box's top left.
    struct Row {
        left: String,
        right: String,
        y: f32,
        left_w: f32,
        right_w: f32,
        left_h: f32,
        right_h: f32,
        bar_y: f32,
        percent: f64,
    }
    let (_, title_h) = measure(ts, &title_look, &poll.poll.title, Space::Normal, inner);
    let mut y = title_h;
    let mut rows = Vec::new();
    for option in &poll.poll.options {
        let count = js::math_max(0.0, poll.totals.get(&option.id).copied().unwrap_or(0.0));
        let percent = if poll.voters != 0.0 && !poll.voters.is_nan() {
            js::math_min(100.0, js::math_round(100.0 * count / poll.voters))
        } else {
            0.0
        };
        let right = format!(
            "{}% · {}",
            js::number_to_string(percent),
            js::number_to_string(count)
        );
        // `display: flex; justify-content: space-between; gap: 24px`: both
        // spans start at their one-line width and shrink together.
        let (left_w, right_w) = flex_pair(ts, &body_look, &option.text, &right, inner - 24.0);
        let (_, left_h) = measure(ts, &body_look, &option.text, Space::Normal, left_w);
        let (_, right_h) = measure(ts, &body_look, &right, Space::Normal, right_w);
        y += 24.0;
        let row_y = y;
        y += left_h.max(right_h) + 10.0;
        let bar_y = y;
        y += 16.0;
        rows.push(Row {
            left: option.text.clone(),
            right,
            y: row_y,
            left_w,
            right_w,
            left_h,
            right_h,
            bar_y,
            percent,
        });
    }
    let foot = format!("{} voted", js::number_to_string(poll.voters));
    let (_, foot_h) = measure(ts, &foot_look, &foot, Space::NoWrap, inner);
    y += 24.0;
    let foot_y = y;
    y += foot_h;
    let card_h = y + 96.0;
    let cx = area.x + (area.width - card_w) / 2.0;
    let cy = area.y + (area.height - card_h) / 2.0;
    let (ix, iy) = (cx + 48.0, cy + 48.0);
    if let Some(bg) = color_with_opacity(&style.background, style.background_opacity) {
        plan.push(box_tile(
            rect(cx, cy, card_w, card_h),
            Corners::Px(style.border_radius_px as f32),
            bg,
            frame,
        ));
    }
    let mut tiles = vec![text_tile(
        ts,
        &title_look,
        &poll.poll.title,
        Space::Normal,
        rect(ix, iy, inner, title_h),
        Vertical::Top,
        false,
        frame,
    )];
    for row in &rows {
        tiles.push(text_tile(
            ts,
            &body_look,
            &row.left,
            Space::Normal,
            rect(ix, iy + row.y, row.left_w, row.left_h),
            Vertical::Top,
            false,
            frame,
        ));
        // The second span sits at the end; its own lines start at its left.
        let at = ix + inner - row.right_w;
        tiles.push(text_tile(
            ts,
            &body_look,
            &row.right,
            Space::Normal,
            rect(at, iy + row.y, row.right_w, row.right_h),
            Vertical::Top,
            false,
            frame,
        ));
    }
    texts(plan, tiles, None);
    for row in &rows {
        let track = rect(ix, iy + row.bar_y, inner, 16.0);
        plan.push(box_tile(
            track,
            Corners::Px(999.0),
            Rgba::new(255, 255, 255, 0x20),
            frame,
        ));
        let filled = rect(
            ix,
            iy + row.bar_y,
            inner * (row.percent as f32) / 100.0,
            16.0,
        );
        plan.push(box_tile(filled, Corners::Px(999.0), color, frame));
    }
    let foot_tile = text_tile(
        ts,
        &foot_look,
        &foot,
        Space::NoWrap,
        rect(ix, iy + foot_y, inner, foot_h),
        Vertical::Top,
        false,
        frame,
    );
    plan.push(foot_tile.map(|t| fade(t, 0.7)));
}

/// The widest word: a flex item's `min-width: auto`.
fn min_content(ts: &mut Typesetter, look: &TextLook, text: &str) -> f32 {
    text.split_whitespace()
        .map(|word| measure(ts, look, word, Space::NoWrap, 0.0).0)
        .fold(0.0, f32::max)
}

/// Two flex items in `avail`: each starts at its max-content width; when
/// they do not fit, both shrink in proportion to that width, never below
/// their widest word (CSS Flexbox 9.7, `flex-shrink: 1`).
fn flex_pair(ts: &mut Typesetter, look: &TextLook, a: &str, b: &str, avail: f32) -> (f32, f32) {
    let (wa, _) = measure(ts, look, a, Space::NoWrap, 0.0);
    let (wb, _) = measure(ts, look, b, Space::NoWrap, 0.0);
    let over = wa + wb - avail;
    if over <= 0.0 {
        return (wa, wb);
    }
    let (ma, mb) = (min_content(ts, look, a), min_content(ts, look, b));
    let mut sa = wa - over * wa / (wa + wb);
    let mut sb = wb - over * wb / (wa + wb);
    // A frozen item gives its share to the other.
    if sa < ma {
        sa = ma;
        sb = (avail - ma).max(mb);
    } else if sb < mb {
        sb = mb;
        sa = (avail - mb).max(ma);
    }
    (sa, sb)
}

/// `/^data:image\/png;base64,[A-Za-z0-9+/=]+$/`: only a QR made here.
fn qr_png(url: &str) -> Option<Vec<u8>> {
    let data = url.strip_prefix("data:image/png;base64,")?;
    if data.is_empty()
        || !data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=')
    {
        return None;
    }
    base64::engine::general_purpose::STANDARD.decode(data).ok()
}

/// `renderQuestionHTML`: the audience question, or the join card with its
/// QR code, headline, code and address.
fn question_plan(
    ts: &mut Typesetter,
    plan: &mut LayerPlan,
    question: &ProgramQuestion,
    style: &ProgramQuestionStyle,
    frame: (u32, u32),
) {
    let (fw, fh) = (frame.0 as f32, frame.1 as f32);
    let area = rect(72.0, 72.0, (fw - 144.0).max(0.0), (fh - 144.0).max(0.0));
    let card_w = area.width.min(card_max_width(style));
    let inner = (card_w - 128.0).max(0.0);
    let family = font_of(style.font_family);
    let color = css(&style.color).unwrap_or(BLACK);
    let fs = style.font_size_px as f32;
    let radius = style.border_radius_px as f32;
    // Each text block: look, text, white space, rect relative to the content box, opacity.
    let mut blocks: Vec<(TextLook, String, Space, Rect, f32)> = Vec::new();
    let mut qr: Option<(Vec<u8>, Rect)> = None;
    let content_h = match question {
        ProgramQuestion::Question { text, author } => {
            let align = match style.align {
                QuestionAlign::Left => Align::Left,
                QuestionAlign::Center => Align::Center,
            };
            let mut y = 0.0;
            if style.show_author {
                let size = js::math_max(22.0, js::math_round(style.font_size_px * 0.45)) as f32;
                let mut look = TextLook::new(family, size, 600.0, 1.25, color);
                look.align = align;
                // `question.author || 'Anonymous'`.
                let name = author
                    .as_deref()
                    .filter(|a| !a.is_empty())
                    .unwrap_or("Anonymous");
                let (_, h) = measure(ts, &look, name, Space::Normal, inner);
                blocks.push((
                    look,
                    name.to_owned(),
                    Space::Normal,
                    rect(0.0, 0.0, inner, h),
                    0.7,
                ));
                y = h + 20.0;
            }
            let mut look = TextLook::new(family, fs, 400.0, 1.25, color);
            look.align = align;
            let (_, h) = measure(ts, &look, text, Space::Pre, inner);
            blocks.push((look, text.clone(), Space::Pre, rect(0.0, y, inner, h), 1.0));
            y + h
        }
        ProgramQuestion::Join {
            code,
            url,
            qr_data_url,
            ..
        } => {
            let png = qr_png(qr_data_url);
            let q = style.qr_size_px as f32;
            let gap_left = if png.is_some() { q + 64.0 } else { 0.0 };
            let avail = (inner - gap_left).max(0.0);
            let headline = js::trim(&style.join_headline).to_owned();
            let head_look = TextLook::new(family, 32.0, 400.0, 1.25, color);
            let mut code_look = TextLook::new(family, 96.0, 700.0, 1.1, color);
            code_look.letter_spacing = 12.0;
            let url_look = TextLook::new(family, 32.0, 400.0, 1.25, color);
            // The column is as wide as its widest line, up to what is left.
            let mut col: Vec<(TextLook, String, Space, f32, f32, f32)> = Vec::new();
            let mut col_w: f32 = 0.0;
            let mut y = 0.0;
            if !headline.is_empty() {
                let (w, _) = measure(ts, &head_look, &headline, Space::NoWrap, avail);
                col_w = col_w.max(w);
                col.push((
                    head_look.clone(),
                    headline.clone(),
                    Space::Normal,
                    0.0,
                    0.85,
                    4.0,
                ));
            }
            let (w, _) = measure(ts, &code_look, code, Space::NoWrap, avail);
            col_w = col_w.max(w);
            col.push((
                code_look.clone(),
                code.clone(),
                Space::Normal,
                0.0,
                1.0,
                0.0,
            ));
            if style.show_join_url {
                let (w, _) = measure(ts, &url_look, url, Space::NoWrap, avail);
                col_w = col_w.max(w);
                col.push((url_look.clone(), url.clone(), Space::Normal, 8.0, 0.75, 0.0));
            }
            let col_w = col_w.min(avail);
            let mut placed = Vec::new();
            for (look, text, space, top, opacity, bottom) in col {
                y += top;
                let (_, h) = measure(ts, &look, &text, space, col_w);
                placed.push((look, text, space, y, h, opacity));
                y += h + bottom;
            }
            let col_h = y;
            let row_h = if png.is_some() { col_h.max(q) } else { col_h };
            let total = gap_left + col_w;
            let start = (inner - total) / 2.0;
            if let Some(png) = png {
                qr = Some((png, rect(start, (row_h - q) / 2.0, q, q)));
            }
            let col_x = start + gap_left;
            let col_y = (row_h - col_h) / 2.0;
            for (look, text, space, y, h, opacity) in placed {
                blocks.push((look, text, space, rect(col_x, col_y + y, col_w, h), opacity));
            }
            row_h
        }
    };
    let card_h = (content_h + 96.0).min(area.height);
    let cy = match style.position {
        QuestionPosition::Top => area.y,
        QuestionPosition::Center => area.y + (area.height - card_h) / 2.0,
        QuestionPosition::Bottom => area.y + area.height - card_h,
    };
    let cx = area.x + (area.width - card_w) / 2.0;
    let card = rect(cx, cy, card_w, card_h);
    let (ix, iy) = (cx + 64.0, cy + 48.0);
    if let Some(bg) = color_with_opacity(&style.background, style.background_opacity) {
        plan.push(box_tile(card, Corners::Px(radius), bg, frame));
    }
    if let Some((png, r)) = qr {
        let at = rect(ix + r.x, iy + r.y, r.width, r.height);
        if let Some(t) = box_tile(
            at,
            Corners::Px(radius.min(16.0)),
            Rgba::new(255, 255, 255, 255),
            frame,
        ) {
            plan.push(clip_tile(t, card));
        }
        // `padding: 16px` inside a border-box image: the code is 32 px smaller.
        let side = (r.width - 32.0).max(0.0);
        let target = BoxFit {
            width: side.round().max(1.0) as u32,
            height: side.round().max(1.0) as u32,
            fit: FitMode::Fill,
        };
        match decode_picture(&png, target) {
            Ok(picture) if !picture.frames.is_empty() => plan.pieces.push(Piece::Scaled {
                width: picture.width,
                height: picture.height,
                pixels: picture.frames[0].pixels.clone(),
                rect: rect(at.x + 16.0, at.y + 16.0, side, side),
                clip: Some(card),
            }),
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "the join QR code could not be read"),
        }
    }
    let mut tiles = Vec::new();
    for (look, text, space, r, opacity) in blocks {
        let tile = text_tile(
            ts,
            &look,
            &text,
            space,
            rect(ix + r.x, iy + r.y, r.width, r.height),
            Vertical::Top,
            false,
            frame,
        );
        tiles.push(tile.map(|t| fade(t, opacity)));
    }
    texts(plan, tiles, Some(card));
}

// ─── Logo ─────────────────────────────────────────────────────────────────

/// `renderLogoHTML`: the colour over everything, the picture contained in it.
fn logo_plan(plan: &mut LayerPlan, logo: &ProgramLogo, frame: (u32, u32)) {
    if let Some(c) = css(&logo.background) {
        plan.push(box_tile(full(frame), Corners::Px(0.0), c, frame));
    }
    if !logo.media_path.is_empty() {
        plan.pieces.push(Piece::Picture {
            path: logo.media_path.clone(),
            rect: full(frame),
            fit: FitMode::Contain,
            opacity: 1.0,
        });
    }
}
