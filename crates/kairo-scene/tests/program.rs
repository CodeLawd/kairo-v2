//! Program layers as plans: the rules of `program.ts`, `overlay.html` and
//! `stage.html` that a picture comparison would not pin down.

use kairo_domain::program::{ProgramLayer, StageView};
use kairo_scene::color::Rgba;
use kairo_scene::program::{
    Live, LiveKind, LiveState, Now, Pattern, Piece, TimerRun, plan_info, plan_layer, plan_pattern,
    plan_stage,
};
use kairo_scene::with_typesetter;
use serde_json::json;

const FRAME: (u32, u32) = (1920, 1080);

fn now(ms: f64) -> Now {
    Now {
        ms,
        clock: "9:05 PM".into(),
    }
}

fn layer(value: serde_json::Value) -> ProgramLayer {
    serde_json::from_value(value).expect("layer")
}

fn lives(pieces: &[Piece]) -> Vec<&Live> {
    pieces
        .iter()
        .filter_map(|p| match p {
            Piece::Live(l) => Some(l),
            _ => None,
        })
        .collect()
}

fn timer_live(run: serde_json::Value) -> Live {
    let l = layer(json!({
        "kind": "props",
        "props": [{"id": "p", "name": "p", "background": null, "objects": [
            {"id": "t", "kind": "timer", "x": 0.3, "y": 0.05, "w": 0.4, "h": 0.12, "opacity": 0.8,
             "fontSizePx": 96, "color": "#5eead4", "overrunColor": "#f87171", "bold": true, "align": "center"}
        ]}],
        "timer": run
    }));
    let plan = with_typesetter(|ts| plan_layer(ts, &l, FRAME));
    lives(&plan.pieces)[0].clone()
}

const TEAL: Rgba = Rgba::new(0x5e, 0xea, 0xd4, 255);
const RED: Rgba = Rgba::new(0xf8, 0x71, 0x71, 255);

#[test]
fn a_running_countdown_ticks_from_ends_at_and_rolls_over() {
    let t0 = 1_000_000.0;
    let live = timer_live(
        json!({"durationSec": 10, "endsAt": t0 + 10_000.0, "remainingSec": 10, "rollover": true, "now": t0}),
    );
    let at = |ms: f64| live.state(&now(t0 + ms));
    assert_eq!(
        at(0.0),
        LiveState {
            text: "0:10".into(),
            color: TEAL,
            opacity: 1.0
        }
    );
    // `Math.ceil`: 9.75 s left still reads 0:10.
    assert_eq!(at(250.0).text, "0:10");
    assert_eq!(at(1_000.0).text, "0:09");
    // Exactly 0 while running is already time up.
    assert_eq!(
        at(10_000.0),
        LiveState {
            text: "0:00".into(),
            color: RED,
            opacity: 1.0
        }
    );
    // Roll over into overtime.
    assert_eq!(
        at(12_001.0),
        LiveState {
            text: "-0:02".into(),
            color: RED,
            opacity: 1.0
        }
    );
    assert_eq!(at(3_610_000.0).text, "-1:00:00");
}

#[test]
fn without_roll_over_the_countdown_holds_at_zero_in_the_overrun_colour() {
    let t0 = 0.0;
    let live = timer_live(
        json!({"durationSec": 10, "endsAt": t0 + 10_000.0, "remainingSec": 10, "rollover": false, "now": t0}),
    );
    let state = live.state(&now(t0 + 30_000.0));
    assert_eq!(state.text, "0:00");
    assert_eq!(state.color, RED);
}

#[test]
fn a_paused_countdown_dims_only_part_way() {
    let idle = timer_live(
        json!({"durationSec": 300, "endsAt": null, "remainingSec": 300, "rollover": true, "now": 0}),
    );
    assert_eq!(
        idle.state(&now(0.0)),
        LiveState {
            text: "5:00".into(),
            color: TEAL,
            opacity: 1.0
        }
    );
    let paused = timer_live(
        json!({"durationSec": 300, "endsAt": null, "remainingSec": 125, "rollover": true, "now": 0}),
    );
    assert_eq!(
        paused.state(&now(99e9)),
        LiveState {
            text: "2:05".into(),
            color: TEAL,
            opacity: 0.55
        }
    );
    // Paused in overtime: overrun colour and dimmed.
    let over = timer_live(
        json!({"durationSec": 60, "endsAt": null, "remainingSec": -12, "rollover": true, "now": 0}),
    );
    assert_eq!(
        over.state(&now(0.0)),
        LiveState {
            text: "-0:12".into(),
            color: RED,
            opacity: 0.55
        }
    );
    // A zero-length timer is never "up".
    let zero = timer_live(
        json!({"durationSec": 0, "endsAt": null, "remainingSec": 0, "rollover": true, "now": 0}),
    );
    assert_eq!(zero.state(&now(0.0)).color, TEAL);
    // The object's own opacity is kept for the pixels.
    assert!((zero.opacity - 0.8).abs() < 1e-6);
}

#[test]
fn a_readout_is_a_small_tile_inside_its_box() {
    let live = timer_live(
        json!({"durationSec": 300, "endsAt": null, "remainingSec": 300, "rollover": true, "now": 0}),
    );
    let tile = with_typesetter(|ts| live.draw(ts, &live.state(&now(0.0)))).expect("pixels");
    assert!(
        tile.x >= 576 && tile.x + tile.width as i32 <= 1344,
        "{} {}",
        tile.x,
        tile.width
    );
    assert!(tile.y >= 54 && tile.y + tile.height as i32 <= 184);
}

fn message(text: &str, scroll: bool) -> ProgramLayer {
    layer(json!({"kind": "message", "text": text, "style": {
        "position": "bottom", "fontSizePx": 44, "color": "#ffffff",
        "background": "rgba(0,0,0,0.72)", "scroll": scroll}}))
}

#[test]
fn a_ticker_starts_past_the_right_edge_and_runs_until_it_has_left() {
    let text = "Cars in the red zone\nplease move";
    let plan = with_typesetter(|ts| plan_layer(ts, &message(text, true), FRAME));
    let moving = plan
        .pieces
        .iter()
        .find_map(|p| match p {
            Piece::Moving {
                tile,
                clip,
                distance,
                period_ms,
                alternate,
            } => Some((tile.clone(), *clip, *distance, *period_ms, *alternate)),
            _ => None,
        })
        .expect("a ticker");
    let (tile, clip, distance, period, alternate) = moving;
    // Padding 0.8 em each side: the words start one content width in.
    let inner = 1920.0 - 2.0 * 35.0;
    assert!(tile.x as f32 >= 35.0 + inner - 40.0, "{}", tile.x);
    // `max(8, round(length * 0.22))` seconds; the length is UTF-16 units.
    assert_eq!(period, 8_000);
    assert!(!alternate);
    // It moves by the padding plus the words, so it ends past the left edge.
    let words = distance.abs() - inner;
    assert!(words > 300.0 && (tile.x as f32 + tile.width as f32 + distance) < 35.0 + 60.0);
    // Cut to the bar at the bottom.
    assert_eq!(clip.y + clip.height, 1080.0);
    assert_eq!(clip.width, 1920.0);
    let long = "x".repeat(100);
    let plan = with_typesetter(|ts| plan_layer(ts, &message(&long, true), FRAME));
    assert!(plan.pieces.iter().any(|p| matches!(
        p,
        Piece::Moving {
            period_ms: 22_000,
            ..
        }
    )));
}

#[test]
fn a_blank_message_draws_nothing() {
    let plan = with_typesetter(|ts| plan_layer(ts, &message("  \n ", false), FRAME));
    assert!(plan.pieces.is_empty());
}

#[test]
fn only_a_png_data_url_becomes_a_qr_code() {
    let join = |url: &str| {
        layer(
            json!({"kind": "question", "question": {"kind": "join", "code": "ABC", "url": "x", "qrDataUrl": url},
            "style": {"position": "center", "align": "left", "fontFamily": "sans", "fontSizePx": 64,
              "color": "#ffffff", "background": "#18212d", "backgroundOpacity": 95, "borderRadiusPx": 24,
              "widthPct": 85, "showAuthor": true, "joinHeadline": "Ask", "qrSizePx": 280, "showJoinUrl": true}}),
        )
    };
    let scaled = |l: &ProgramLayer| {
        with_typesetter(|ts| plan_layer(ts, l, FRAME))
            .pieces
            .iter()
            .filter(|p| matches!(p, Piece::Scaled { .. }))
            .count()
    };
    // A 1 × 1 PNG.
    let png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    assert_eq!(scaled(&join(png)), 1);
    assert_eq!(scaled(&join("https://example.com/qr.png")), 0);
    assert_eq!(scaled(&join("data:image/svg+xml;base64,PHN2Zz4=")), 0);
    assert_eq!(scaled(&join("data:image/png;base64,<script>")), 0);
}

#[test]
fn props_skip_images_with_no_file_and_colours_that_cannot_be_read() {
    let l = layer(
        json!({"kind": "props", "props": [{"id": "p", "name": "p", "background": "not a colour", "objects": [
        {"id": "i", "kind": "image", "x": 0, "y": 0, "w": 0.5, "h": 0.5, "opacity": 1, "mediaPath": "", "fit": "contain"},
        {"id": "s", "kind": "shape", "x": 0, "y": 0, "w": 0.5, "h": 0.5, "opacity": 1, "shape": "rectangle", "fill": "nope", "radiusPx": 0},
        {"id": "c", "kind": "image", "x": 0.5, "y": 0.5, "w": 0.5, "h": 0.5, "opacity": 0.5, "mediaPath": "/x.png", "fit": "cover"}
    ]}], "timer": {"durationSec": 0, "endsAt": null, "remainingSec": 0, "rollover": true, "now": 0}}),
    );
    let plan = with_typesetter(|ts| plan_layer(ts, &l, FRAME));
    assert_eq!(plan.pieces.len(), 1);
    assert!(
        matches!(&plan.pieces[0], Piece::Picture { path, opacity, .. } if path == "/x.png" && *opacity == 0.5)
    );
}

fn info(
    clock: bool,
    message: Option<&str>,
    position: &str,
    backdrop: bool,
) -> kairo_domain::program::ConfidenceInfo {
    serde_json::from_value(json!({
        "clock": clock, "stageMessage": message,
        "layout": {"position": position, "size": "large"},
        "sizes": {"barPx": 104, "messagePx": 84},
        "style": {"rollover": true, "color": "#5eead4", "overrunColor": "#f87171", "backdrop": backdrop}
    }))
    .expect("info")
}

#[test]
fn the_info_clock_is_live_and_the_message_needs_text() {
    let plan = with_typesetter(|ts| plan_info(ts, &info(true, Some(""), "top", false), FRAME));
    let live = lives(&plan.pieces);
    assert_eq!(live.len(), 1);
    assert!(matches!(live[0].kind, LiveKind::Clock { .. }));
    assert_eq!(live[0].state(&now(0.0)).text, "9:05 PM");
    // The empty string hides the message box; only the clock is drawn.
    assert_eq!(plan.pieces.len(), 1);
    // A backdrop is drawn behind the clock, at the bottom when asked.
    let plan = with_typesetter(|ts| plan_info(ts, &info(true, None, "bottom", true), FRAME));
    assert_eq!(plan.pieces.len(), 2);
    let Piece::Pixels(bar) = &plan.pieces[0] else {
        panic!("backdrop")
    };
    assert_eq!(bar.y as u32 + bar.height, 1080);
    // Whitespace is not empty: the box shows.
    let plan = with_typesetter(|ts| plan_info(ts, &info(false, Some("  "), "top", false), FRAME));
    assert!(!plan.pieces.is_empty());
}

fn stage(patch: serde_json::Value) -> StageView {
    let mut view = json!({
        "current": {"reference": "John 3:16", "text": "For God so loved the world"},
        "next": {"reference": "John 3:17", "text": "For God sent not his Son"},
        "stageMessage": null,
        "timer": {"durationSec": 300, "endsAt": null, "remainingSec": 300},
        "showNext": true, "showClock": true, "showTimer": true,
        "timerStyle": {"rollover": true, "color": "#5eead4", "overrunColor": "#f87171", "backdrop": false}
    });
    for (k, v) in patch.as_object().expect("patch") {
        view[k] = v.clone();
    }
    serde_json::from_value(view).expect("stage view")
}

#[test]
fn stage_hides_clock_timer_and_next_as_asked() {
    let all = with_typesetter(|ts| plan_stage(ts, &stage(json!({}))));
    assert_eq!(lives(&all.pieces).len(), 2);
    let none = with_typesetter(|ts| {
        plan_stage(ts, &stage(json!({"showClock": false, "showTimer": false})))
    });
    assert!(lives(&none.pieces).is_empty());
    // Alone, the countdown takes the clock's place on the left.
    let timer_only = with_typesetter(|ts| plan_stage(ts, &stage(json!({"showClock": false}))));
    let live = lives(&timer_only.pieces);
    assert_eq!(live.len(), 1);
    let tile = with_typesetter(|ts| live[0].draw(ts, &live[0].state(&now(0.0)))).unwrap();
    assert!(tile.x < 200, "{}", tile.x);
    // Without NEXT, the slide takes the space down to 40 px from the bottom.
    let full = with_typesetter(|ts| plan_stage(ts, &stage(json!({"showNext": false}))));
    assert!(full.pieces.len() < all.pieces.len());
}

#[test]
fn stage_next_falls_back_to_the_reference_then_an_em_dash() {
    // Text drawn for NEXT differs between these three, so their pixels do.
    let draw = |patch| {
        let plan = with_typesetter(|ts| plan_stage(ts, &stage(patch)));
        plan.pieces
            .iter()
            .filter_map(|p| match p {
                Piece::Pixels(t) if t.y > 830 => Some(t.width),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    };
    let with_text = draw(json!({}));
    let reference = draw(json!({"next": {"reference": "Psalm 23", "text": ""}}));
    let dash = draw(json!({"next": null}));
    assert!(
        with_text > reference && reference > dash && dash > 0,
        "{with_text} {reference} {dash}"
    );
}

#[test]
fn stage_timer_uses_the_style_and_roll_over() {
    let view = stage(
        json!({"timer": {"durationSec": 60, "endsAt": null, "remainingSec": -5},
        "timerStyle": {"rollover": false, "color": "#ffffff", "overrunColor": "#ff0000", "backdrop": false}}),
    );
    let plan = with_typesetter(|ts| plan_stage(ts, &view));
    let timer = lives(&plan.pieces)
        .into_iter()
        .find(|l| matches!(l.kind, LiveKind::Timer { .. }))
        .unwrap();
    assert_eq!(
        timer.state(&now(0.0)),
        LiveState {
            text: "0:00".into(),
            color: Rgba::new(255, 0, 0, 255),
            opacity: 0.55
        }
    );
    let LiveKind::Timer { run, .. } = &timer.kind else {
        unreachable!()
    };
    assert_eq!(
        *run,
        TimerRun {
            duration_sec: 60.0,
            ends_at: None,
            remaining_sec: -5.0,
            rollover: false
        }
    );
}

#[test]
fn test_patterns_draw_their_parts_and_the_name() {
    for pattern in [Pattern::Bars, Pattern::Text, Pattern::Motion] {
        let plan = with_typesetter(|ts| plan_pattern(ts, pattern, "Main", FRAME));
        assert!(plan.pieces.len() >= 3, "{pattern:?}");
        let moving = plan.pieces.iter().any(|p| matches!(p, Piece::Moving { alternate: true, distance, period_ms: 3000, .. } if *distance == 1800.0));
        assert_eq!(moving, pattern == Pattern::Motion);
    }
}
