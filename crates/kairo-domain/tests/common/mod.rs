//! Hostile JSON generators shared by the prompt 013 oracle tests.
//!
//! Every field of a generated object is, at random, absent, plausible (near
//! or outside its valid range, a valid or near-miss enum name or colour) or
//! hostile (wrong type, `null`, numeric strings such as `"NaN"`, huge numbers,
//! arrays, objects). Values are JSON, as a store file holds; JSON cannot hold
//! `NaN` or infinities, so neither can these.

#![allow(dead_code)]

use proptest::prelude::*;
use proptest::strategy::BoxedStrategy;
use serde_json::{Map, Value, json};

/// Cases per oracle chunk. Chunk `i` uses seed `base + i`, so the first
/// chunks of a long run are the cached ones.
pub const CHUNK: usize = 100;

/// Cases to run: `KAIRO_DIFF_CASES` when set (the 100,000-case run), else `default`.
pub fn cases(default: usize) -> usize {
    std::env::var("KAIRO_DIFF_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Generates `total` values in chunks of [`CHUNK`] and hands each chunk to `check`.
pub fn chunked<S: Strategy>(
    seed: u64,
    total: usize,
    strategy: S,
    mut check: impl FnMut(Vec<S::Value>),
) {
    let chunks = total.div_ceil(CHUNK);
    for i in 0..chunks {
        let count = CHUNK.min(total - i * CHUNK);
        check(kairo_oracle::generate(seed + i as u64, count, &strategy));
    }
}

pub fn s(text: &str) -> Value {
    Value::String(text.to_owned())
}

/// Strings that coerce, trim or match differently in JavaScript.
pub fn tricky_string() -> BoxedStrategy<Value> {
    prop_oneof![
        Just(s("")),
        Just(s(" ")),
        Just(s("NaN")),
        Just(s("Infinity")),
        Just(s("-1")),
        Just(s("12")),
        Just(s(" 12 ")),
        Just(s("0x10")),
        Just(s("1e3")),
        Just(s("true")),
        Just(s("null")),
        Just(s("undefined")),
        Just(s("\u{feff}x\u{a0}")),
        Just(s("\u{85}y\u{85}")),
        Just(s("a\u{2028}b\u{2029}")),
        Just(s("$&$$$`$'")),
        "[ -~]{0,10}".prop_map(Value::String),
        "\\PC{0,6}".prop_map(Value::String),
    ]
    .boxed()
}

/// Numbers at the edges of `f64`, and plain integers and fractions.
pub fn tricky_number() -> BoxedStrategy<Value> {
    prop_oneof![
        Just(json!(0)),
        Just(json!(-0.0)),
        Just(json!(1e308)),
        Just(json!(-1e308)),
        Just(json!(5e-324)),
        Just(json!(9_007_199_254_740_993_u64)),
        Just(json!(0.5)),
        Just(json!(-0.5)),
        Just(json!(2.5)),
        (-1000i64..1000).prop_map(|n| json!(n)),
        any::<f64>()
            .prop_filter("JSON numbers are finite", |f| f.is_finite())
            .prop_map(|f| json!(f)),
    ]
    .boxed()
}

pub fn scalar() -> BoxedStrategy<Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        tricky_number(),
        tricky_string(),
    ]
    .boxed()
}

/// Any JSON value: scalars, and small arrays and objects of them.
pub fn hostile() -> BoxedStrategy<Value> {
    prop_oneof![
        4 => scalar(),
        1 => prop::collection::vec(scalar(), 0..3).prop_map(Value::Array),
        1 => prop::collection::btree_map("[a-z]{1,3}", scalar(), 0..3)
            .prop_map(|m| Value::Object(m.into_iter().collect())),
    ]
    .boxed()
}

/// A field: absent, plausible or hostile.
pub fn field(plausible: BoxedStrategy<Value>) -> BoxedStrategy<Option<Value>> {
    prop_oneof![
        2 => Just(None),
        5 => plausible.prop_map(Some),
        3 => hostile().prop_map(Some),
    ]
    .boxed()
}

/// An object from `(key, plausible value)` pairs, each run through [`field`],
/// sometimes with an unknown key that must be dropped.
pub fn obj(fields: Vec<(&'static str, BoxedStrategy<Value>)>) -> BoxedStrategy<Value> {
    let keys: Vec<&'static str> = fields.iter().map(|(k, _)| *k).collect();
    let values: Vec<BoxedStrategy<Option<Value>>> =
        fields.into_iter().map(|(_, v)| field(v)).collect();
    (values, prop::option::weighted(0.2, hostile()))
        .prop_map(move |(values, junk)| {
            let mut map = Map::new();
            for (key, value) in keys.iter().zip(values) {
                if let Some(value) = value {
                    map.insert((*key).to_owned(), value);
                }
            }
            if let Some(junk) = junk {
                map.insert("futureKey".into(), junk);
            }
            Value::Object(map)
        })
        .boxed()
}

/// A number around `min..=max`: inside, at the bounds, and beyond them.
pub fn num(min: f64, max: f64) -> BoxedStrategy<Value> {
    let span = (max - min).max(1.0);
    prop_oneof![
        3 => (min..=max).prop_map(|n| json!(n)),
        1 => (min - span..=max + span).prop_map(|n| json!(n)),
        1 => prop::sample::select(vec![min, max, min - 1.0, max + 1.0]).prop_map(|n| json!(n)),
        1 => ((min as i64 - 2)..=(max as i64 + 2)).prop_map(|n| json!(n)),
    ]
    .boxed()
}

pub fn boolean() -> BoxedStrategy<Value> {
    any::<bool>().prop_map(Value::Bool).boxed()
}

/// Valid colours and near misses.
pub fn color() -> BoxedStrategy<Value> {
    prop::sample::select(vec![
        "#fff",
        "#ABCDEF",
        " #123456 ",
        "#12345678",
        "#ff",
        "#123456789",
        "#ggg",
        "rgb(1, 2, 3)",
        "rgba(3, 10, 20, 0.75)",
        " rgba(0,0,0,0) ",
        "rgb()",
        "rgba(1,2,3",
        "red",
        "url(javascript:alert(1))",
        "javascript:alert(1)",
        "\u{a0}#000\u{feff}",
    ])
    .prop_map(s)
    .boxed()
}

/// Every name in `names`, plus near misses.
pub fn enumeration(names: &[&'static str]) -> BoxedStrategy<Value> {
    let mut all: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    all.extend(names.iter().map(|n| n.to_uppercase()));
    all.extend(names.iter().map(|n| format!(" {n}")));
    all.push(String::new());
    all.push("bogus".into());
    prop::sample::select(all).prop_map(Value::String).boxed()
}

/// Ids from a small pool, so duplicates, blanks and padding are common.
pub fn id() -> BoxedStrategy<Value> {
    prop::sample::select(vec![
        "a",
        "b",
        "c",
        " a ",
        "",
        "  ",
        "main",
        "projector",
        "x y",
    ])
    .prop_map(s)
    .boxed()
}

pub fn text() -> BoxedStrategy<Value> {
    prop_oneof![
        prop::sample::select(vec![
            "Main",
            " Lobby ",
            "",
            "  ",
            "{Reference}\n{Text}",
            "{Text}"
        ])
        .prop_map(s),
        tricky_string(),
    ]
    .boxed()
}

pub fn media_path() -> BoxedStrategy<Value> {
    prop::sample::select(vec![
        "",
        "  ",
        "/media/a.png",
        " /media/b c.mp4 ",
        "relative.mov",
    ])
    .prop_map(s)
    .boxed()
}

pub fn overlay_box() -> BoxedStrategy<Value> {
    obj(vec![
        ("xPct", num(0.0, 100.0)),
        ("yPct", num(0.0, 100.0)),
        ("widthPct", num(0.0, 100.0)),
        ("heightPct", num(0.0, 100.0)),
    ])
}

pub fn shadow() -> BoxedStrategy<Value> {
    prop_oneof![
        1 => boolean(),
        3 => obj(vec![
            ("enabled", boolean()),
            ("xPx", num(-40.0, 40.0)),
            ("yPx", num(-40.0, 40.0)),
            ("blurPx", num(0.0, 80.0)),
            ("color", color()),
            ("opacity", num(0.0, 1.0)),
        ]),
    ]
    .boxed()
}

pub fn outline() -> BoxedStrategy<Value> {
    obj(vec![
        ("enabled", boolean()),
        ("position", enumeration(&["outside", "inside", "center"])),
        ("widthPx", num(0.0, 24.0)),
        ("style", enumeration(&["solid", "dashed", "dotted"])),
        ("color", color()),
        ("opacity", num(0.0, 1.0)),
    ])
}

fn text_style_fields() -> Vec<(&'static str, BoxedStrategy<Value>)> {
    vec![
        (
            "fontFamily",
            prop_oneof![
                prop::sample::select(vec![
                    "Georgia",
                    " Ari;al ",
                    "\"{}\\;",
                    "  ",
                    "'Helvetica Neue', Arial"
                ])
                .prop_map(s),
                tricky_string(),
            ]
            .boxed(),
        ),
        ("fontSizePx", num(12.0, 200.0)),
        ("fontWeight", num(100.0, 900.0)),
        ("color", color()),
        ("colorOpacity", num(0.0, 1.0)),
        ("lineHeight", num(0.9, 2.5)),
        ("letterSpacingPx", num(-4.0, 20.0)),
        (
            "align",
            enumeration(&["left", "center", "right", "justify"]),
        ),
        ("verticalAlign", enumeration(&["top", "middle", "bottom"])),
        (
            "textDecoration",
            enumeration(&["none", "underline", "line-through"]),
        ),
        (
            "textTransform",
            enumeration(&["none", "uppercase", "capitalize", "lowercase"]),
        ),
        ("uppercase", boolean()),
        ("shadow", shadow()),
        ("outline", outline()),
        ("box", overlay_box()),
    ]
}

pub fn text_style() -> BoxedStrategy<Value> {
    obj(text_style_fields())
}

pub fn reference_style() -> BoxedStrategy<Value> {
    let mut fields = text_style_fields();
    fields.push(("show", boolean()));
    fields.push(("position", enumeration(&["above", "below"])));
    obj(fields)
}

pub fn shape_id() -> BoxedStrategy<Value> {
    prop::sample::select(vec![
        "heart",
        "line-arrow",
        "star-5",
        "no-symbol",
        "sun",
        "not-a-shape",
        "",
    ])
    .prop_map(s)
    .boxed()
}

pub fn element() -> BoxedStrategy<Value> {
    obj(vec![
        ("id", id()),
        (
            "kind",
            enumeration(&["rectangle", "ellipse", "shape", "image", "video"]),
        ),
        ("shape", shape_id()),
        ("box", overlay_box()),
        ("aboveText", boolean()),
        ("opacity", num(0.0, 1.0)),
        ("rotationDeg", num(-180.0, 180.0)),
        ("flipX", boolean()),
        ("flipY", boolean()),
        ("fill", color()),
        ("fillOpacity", num(0.0, 1.0)),
        (
            "stroke",
            obj(vec![
                ("enabled", boolean()),
                ("color", color()),
                ("opacity", num(0.0, 1.0)),
                ("widthPx", num(0.0, 200.0)),
            ]),
        ),
        ("radiusPx", num(0.0, 1080.0)),
        ("mediaPath", media_path()),
        ("mediaFit", enumeration(&["cover", "contain", "fill"])),
        ("mediaLoop", boolean()),
        ("hue", num(-180.0, 180.0)),
        ("saturation", num(0.0, 2.0)),
        ("brightness", num(0.25, 1.75)),
        ("contrast", num(0.25, 1.75)),
        ("blurPx", num(0.0, 40.0)),
    ])
}

pub fn elements() -> BoxedStrategy<Value> {
    prop_oneof![
        20 => prop::collection::vec(element(), 0..5).prop_map(Value::Array),
        // Past the cap of 40, with distinct ids.
        1 => (40usize..46).prop_map(|n| Value::Array(
            (0..n).map(|i| json!({"id": format!("e{i}"), "kind": "rectangle"})).collect()
        )),
    ]
    .boxed()
}

pub fn background() -> BoxedStrategy<Value> {
    obj(vec![
        (
            "type",
            enumeration(&["color", "gradient", "transparent", "image", "video"]),
        ),
        ("color", color()),
        ("color2", color()),
        ("angleDeg", num(0.0, 360.0)),
        ("opacity", num(0.0, 1.0)),
        ("mediaPath", media_path()),
        ("mediaFit", enumeration(&["cover", "contain", "fill"])),
        ("mediaLoop", boolean()),
        ("hue", num(-180.0, 180.0)),
        ("saturation", num(0.0, 2.0)),
        ("brightness", num(0.25, 1.75)),
        ("contrast", num(0.25, 1.75)),
        ("blurPx", num(0.0, 40.0)),
    ])
}

pub fn layout() -> BoxedStrategy<Value> {
    obj(vec![
        (
            "position",
            enumeration(&["lower-third", "center", "top", "full"]),
        ),
        ("maxWidthPct", num(20.0, 100.0)),
        ("paddingPx", num(0.0, 200.0)),
        ("backdropBox", boolean()),
        ("backdropColor", color()),
        ("backdropRadiusPx", num(0.0, 40.0)),
        ("autoFitText", boolean()),
    ])
}

pub fn theme() -> BoxedStrategy<Value> {
    obj(vec![
        ("elements", elements()),
        ("background", background()),
        ("verse", text_style()),
        ("reference", reference_style()),
        ("layout", layout()),
    ])
}

fn size() -> BoxedStrategy<Value> {
    obj(vec![
        ("width", num(-10.0, 4000.0)),
        ("height", num(-10.0, 4000.0)),
    ])
}

pub fn display_id() -> BoxedStrategy<Value> {
    prop_oneof![
        3 => (1i64..4).prop_map(|n| json!(n)),
        1 => prop::sample::select(vec![json!(1.5), json!("7"), json!(9_007_199_254_740_993_u64), json!(-3)]),
    ]
    .boxed()
}

pub fn output() -> BoxedStrategy<Value> {
    obj(vec![
        ("id", id()),
        ("name", text()),
        (
            "kind",
            enumeration(&["ndi", "screen", "device", "syphon", "message", "library"]),
        ),
        ("enabled", boolean()),
        ("order", num(0.0, 999.0)),
        ("fallbackOnly", boolean()),
        ("themeId", prop_oneof![id(), Just(Value::Null)].boxed()),
        ("theme", theme()),
        ("template", text()),
        (
            "lyrics",
            prop_oneof![
                2 => Just(Value::Null),
                1 => Just(json!([])),
                3 => obj(vec![("themeId", id()), ("theme", theme()), ("template", text())]),
            ]
            .boxed(),
        ),
        ("displayId", display_id()),
        ("displayLabel", text()),
        ("devicePortId", text()),
        ("devicePortLabel", text()),
        ("deviceModeId", text()),
        ("deviceKeyFill", boolean()),
        ("displaySize", size()),
        (
            "aspect",
            enumeration(&[
                "letterbox",
                "16:10",
                "4:3",
                "21:9",
                "32:9",
                "9:16",
                "1:1",
                "fill",
                "custom",
            ]),
        ),
        (
            "customAspect",
            obj(vec![
                ("width", num(1.0, 100.0)),
                ("height", num(1.0, 100.0)),
            ]),
        ),
        (
            "show",
            obj(vec![
                ("scripture", boolean()),
                ("lyrics", boolean()),
                ("documents", boolean()),
                ("backgrounds", boolean()),
                ("poll", boolean()),
                ("question", boolean()),
                ("messages", boolean()),
                ("overlays", boolean()),
                ("countdown", boolean()),
                ("clock", boolean()),
                ("stageMessage", boolean()),
            ]),
        ),
        ("source", enumeration(&["program", "playlist"])),
        ("playlistId", text()),
        ("slideSec", num(3.0, 600.0)),
        (
            "confidence",
            obj(vec![
                ("position", enumeration(&["top", "bottom"])),
                ("size", enumeration(&["small", "medium", "large"])),
            ]),
        ),
        ("ppVideoInputUuid", text()),
        ("lookId", text()),
    ])
}

pub fn outputs() -> BoxedStrategy<Value> {
    prop_oneof![
        8 => prop::collection::vec(output(), 0..4).prop_map(Value::Array),
        // Many senders of one kind, to reach the NDI and Syphon caps.
        1 => (prop::sample::select(vec!["ndi", "syphon", "screen"]), 4usize..8).prop_map(|(kind, n)| {
            Value::Array((0..n).map(|i| json!({"id": format!("o{i}"), "kind": kind, "enabled": true, "displayId": 1})).collect())
        }),
    ]
    .boxed()
}

pub fn overlay_settings() -> BoxedStrategy<Value> {
    obj(vec![
        ("template", text()),
        ("showTranslation", boolean()),
        ("showVerseNumbers", boolean()),
        ("maxVerses", num(-5.0, 20.0)),
        ("mode", enumeration(&["auto", "ndi", "message"])),
        ("theme", theme()),
        ("outputs", outputs()),
        ("ppVideoInputUuid", text()),
        ("autoClearSec", num(0.0, 60.0)),
    ])
}

pub fn library_item() -> BoxedStrategy<Value> {
    obj(vec![
        ("id", id()),
        ("name", text()),
        ("kind", enumeration(&["scripture", "lyrics"])),
        ("createdAt", num(0.0, 2e12)),
        ("updatedAt", num(0.0, 2e12)),
        ("theme", theme()),
        (
            "baseline",
            prop_oneof![
                theme(),
                Just(json!(false)),
                Just(json!(0)),
                Just(json!("")),
                Just(json!(1))
            ]
            .boxed(),
        ),
    ])
}

pub fn theme_library() -> BoxedStrategy<Value> {
    prop::collection::vec(library_item(), 0..4)
        .prop_map(Value::Array)
        .boxed()
}

/// Raw settings files for `retireProPresenterSettings`: small themes, all
/// output kinds including retired ones and coerced kinds.
pub fn retire_input() -> BoxedStrategy<Value> {
    let output = obj(vec![
        ("id", id()),
        (
            "kind",
            prop_oneof![
                enumeration(&["ndi", "screen", "device", "syphon", "message", "library"]),
                Just(json!(["ndi"])),
                Just(json!(["screen", null])),
            ]
            .boxed(),
        ),
        ("enabled", boolean()),
        ("lookId", text()),
        ("ppVideoInputUuid", text()),
        ("displayId", display_id()),
    ]);
    let overlay = obj(vec![
        (
            "outputs",
            prop::collection::vec(output, 0..4)
                .prop_map(Value::Array)
                .boxed(),
        ),
        ("mode", enumeration(&["auto", "ndi", "message"])),
        ("ppVideoInputUuid", text()),
        ("theme", hostile()),
        ("template", text()),
    ]);
    obj(vec![
        ("propresenter", hostile()),
        ("retirementNotice", boolean()),
        ("propresenterResources", hostile()),
        ("songs", hostile()),
        ("overlay", overlay),
    ])
}
