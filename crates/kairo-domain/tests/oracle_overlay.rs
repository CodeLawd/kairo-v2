//! Every other prompt 013 export against the TypeScript: constants, box
//! maths, the shape catalog, output routing, content formatting, theme
//! library edits, displays, dispatch health, the test pattern and prop
//! geometry. Typed inputs come from the normalisers, so they are valid.

mod common;

use common::*;
use kairo_domain::displays::*;
use kairo_domain::js::number_to_string;
use kairo_domain::json::to_js_value;
use kairo_domain::output_health::dispatch_health;
use kairo_domain::output_test_pattern::output_test_pattern_html;
use kairo_domain::overlay::*;
use kairo_domain::overlay_boxes::*;
use kairo_domain::overlay_content::*;
use kairo_domain::overlay_defaults::*;
use kairo_domain::overlay_elements::*;
use kairo_domain::overlay_outputs::*;
use kairo_domain::overlay_shapes::*;
use kairo_domain::prop_geometry::*;
use kairo_domain::scripture::ScriptureVerse;
use kairo_domain::serde_util::Extra;
use kairo_domain::theme_library::*;
use kairo_domain::video_devices::VideoTestPattern;
use kairo_oracle::{
    assert_all_match, assert_matches_oracle, compare, number, oracle_value, undefined,
};
use proptest::prelude::*;
use serde::Serialize;
use serde_json::{Value, json};

fn js<T: Serialize>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

fn opt<T: Serialize>(value: Option<T>) -> Value {
    value.map_or(Value::Null, |v| js(&v))
}

// ─── Typed generators ──────────────────────────────────────────────────────────

fn typed_theme() -> BoxedStrategy<OverlayTheme> {
    theme()
        .prop_map(|raw| normalize_overlay_theme(Some(&raw)))
        .boxed()
}

fn legacy_fields() -> LegacyOverlayFields {
    LegacyOverlayFields {
        mode: None,
        theme: DEFAULT_OVERLAY_THEME.clone(),
        template: DEFAULT_TEMPLATE.into(),
    }
}

fn typed_outputs() -> BoxedStrategy<Vec<OverlayOutput>> {
    outputs()
        .prop_map(|raw| normalize_overlay_outputs(Some(&raw), &legacy_fields()))
        .boxed()
}

fn typed_output() -> BoxedStrategy<OverlayOutput> {
    output()
        .prop_map(|raw| normalize_overlay_outputs(Some(&json!([raw])), &legacy_fields()).remove(0))
        .boxed()
}

fn typed_library() -> BoxedStrategy<Vec<CustomOverlayTheme>> {
    theme_library()
        .prop_map(|raw| normalize_theme_library(Some(&raw), &DEFAULT_OVERLAY_THEME, 1.0, "legacy"))
        .boxed()
}

fn content_kind() -> BoxedStrategy<OverlayContentKind> {
    prop::sample::select(OverlayContentKind::ALL.to_vec()).boxed()
}

/// Floats for geometry: ordinary values and the ones JavaScript treats specially.
fn float() -> BoxedStrategy<f64> {
    prop_oneof![
        6 => -150.0..150.0f64,
        1 => prop::sample::select(vec![0.0, -0.0, 100.0, 0.5, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300]),
    ]
    .boxed()
}

fn raw_box() -> BoxedStrategy<OverlayBox> {
    (float(), float(), float(), float())
        .prop_map(|(x, y, w, h)| kairo_domain::overlay_boxes::overlay_box(x, y, w, h))
        .boxed()
}

fn box_json(b: &OverlayBox) -> Value {
    json!({"xPct": number(b.x_pct), "yPct": number(b.y_pct), "widthPct": number(b.width_pct), "heightPct": number(b.height_pct)})
}

fn min_json(m: BoxMinimum) -> Value {
    json!({"widthPct": m.width_pct, "heightPct": m.height_pct})
}

fn handle() -> BoxedStrategy<ResizeHandle> {
    prop::sample::select(ResizeHandle::ALL.to_vec()).boxed()
}

fn minimum() -> BoxedStrategy<BoxMinimum> {
    prop::sample::select(vec![TEXT_BOX_MIN, ELEMENT_BOX_MIN]).boxed()
}

// ─── Constants ─────────────────────────────────────────────────────────────────

#[test]
fn constants_match() {
    let constant = |name: &str, value: Value| assert_matches_oracle(name, json!([]), value);
    constant(
        "overlay-defaults.DEFAULT_TEXT_SHADOW",
        js(&default_text_shadow()),
    );
    constant(
        "overlay-defaults.DEFAULT_TEXT_OUTLINE",
        js(&default_text_outline()),
    );
    constant(
        "overlay-defaults.DEFAULT_OVERLAY_THEME",
        js(&*DEFAULT_OVERLAY_THEME),
    );
    constant(
        "overlay-defaults.DEFAULT_OVERLAY_SETTINGS",
        js(&*DEFAULT_OVERLAY_SETTINGS),
    );
    constant(
        "overlay-defaults.PROJECTOR_OUTPUT_ID",
        json!(PROJECTOR_OUTPUT_ID),
    );
    constant(
        "overlay-elements.OVERLAY_ELEMENT_KINDS",
        json!(
            OVERLAY_ELEMENT_KINDS
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
        ),
    );
    constant(
        "overlay-elements.MAX_OVERLAY_ELEMENTS",
        json!(MAX_OVERLAY_ELEMENTS),
    );
    for (name, value) in [
        ("MIN_BOX_WIDTH_PCT", MIN_BOX_WIDTH_PCT),
        ("MIN_BOX_HEIGHT_PCT", MIN_BOX_HEIGHT_PCT),
        ("VERSE_HEIGHT_PCT", VERSE_HEIGHT_PCT),
        ("REFERENCE_HEIGHT_PCT", REFERENCE_HEIGHT_PCT),
        ("STACK_GAP_PCT", STACK_GAP_PCT),
        ("FRAME_MARGIN_PCT", FRAME_MARGIN_PCT),
    ] {
        constant(&format!("overlay-boxes.{name}"), json!(value));
    }
    constant("overlay-boxes.TEXT_BOX_MIN", min_json(TEXT_BOX_MIN));
    constant("overlay-boxes.ELEMENT_BOX_MIN", min_json(ELEMENT_BOX_MIN));
    constant(
        "overlay-shapes.OVERLAY_SHAPE_GROUPS",
        js(&*OVERLAY_SHAPE_GROUPS),
    );
    constant(
        "overlay-shapes.OVERLAY_SHAPE_IDS",
        json!(*OVERLAY_SHAPE_IDS),
    );
    constant(
        "overlay-outputs.OVERLAY_OUTPUT_KINDS",
        json!(
            OVERLAY_OUTPUT_KINDS
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
        ),
    );
    constant("overlay-outputs.MAX_NDI_OUTPUTS", json!(MAX_NDI_OUTPUTS));
    constant(
        "overlay-outputs.MAX_SYPHON_OUTPUTS",
        json!(MAX_SYPHON_OUTPUTS),
    );
    constant(
        "overlay-outputs.OVERLAY_CONTENT_KINDS",
        json!(
            OVERLAY_CONTENT_KINDS
                .iter()
                .map(|k| k.as_str())
                .collect::<Vec<_>>()
        ),
    );
    constant(
        "theme-library.BUILT_IN_THEMES",
        Value::Array(
            BUILT_IN_THEMES
                .iter()
                .map(|t| json!({"id": t.id, "name": t.name, "kind": t.kind.as_str(), "theme": js(&t.theme)}))
                .collect(),
        ),
    );
    constant(
        "displays.OUTPUT_ASPECTS",
        Value::Array(
            OUTPUT_ASPECTS
                .iter()
                .map(|o| json!({"value": o.value.as_str(), "label": o.label, "ratio": o.ratio.map(|(w, h)| json!([w, h]))}))
                .collect(),
        ),
    );
    constant(
        "displays.OUTPUT_ASPECT_VALUES",
        json!(
            OUTPUT_ASPECT_VALUES
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
        ),
    );
    constant(
        "prop-geometry.MIN_PROP_OBJECT_SIZE",
        json!(MIN_PROP_OBJECT_SIZE),
    );
}

// ─── Box maths ─────────────────────────────────────────────────────────────────

#[test]
fn box_maths_match() {
    let strategy = (raw_box(), raw_box(), float(), float(), handle(), minimum());
    chunked(15_100, cases(2_000), strategy, |inputs| {
        let mut clamp = Vec::new();
        let mut moved = Vec::new();
        let mut resized = Vec::new();
        let mut aspect = Vec::new();
        let mut placed = Vec::new();
        let mut style = Vec::new();
        for (a, b, dx, dy, h, min) in &inputs {
            let (ja, jb, jm) = (box_json(a), box_json(b), min_json(*min));
            let (ndx, ndy) = (number(*dx), number(*dy));
            clamp.push((json!([ja, jm]), box_json(&clamp_overlay_box(a, *min))));
            moved.push((
                json!([ja, ndx, ndy, jm]),
                box_json(&move_overlay_box(a, *dx, *dy, *min)),
            ));
            resized.push((
                json!([ja, h.as_str(), ndx, ndy, jm]),
                box_json(&resize_overlay_box(a, *h, *dx, *dy, *min)),
            ));
            aspect.push((
                json!([ja, h.as_str(), ndx, ndy, jm]),
                box_json(&resize_overlay_box_keeping_aspect(a, *h, *dx, *dy, *min)),
            ));
            for position in ReferencePosition::ALL {
                placed.push((
                    json!([ja, jb, position.as_str()]),
                    box_json(&place_reference_against_verse(a, b, *position)),
                ));
            }
            style.push((json!([ja]), json!(overlay_box_style(a))));
        }
        assert_all_match("overlay-boxes.clampOverlayBox", &clamp);
        assert_all_match("overlay-boxes.moveOverlayBox", &moved);
        assert_all_match("overlay-boxes.resizeOverlayBox", &resized);
        assert_all_match("overlay-boxes.resizeOverlayBoxKeepingAspect", &aspect);
        assert_all_match("overlay-boxes.placeReferenceAgainstVerse", &placed);
        assert_all_match("overlay-boxes.overlayBoxStyle", &style);
    });
}

#[test]
fn layout_presets_match() {
    let position = prop::sample::select(OverlayLayoutPosition::ALL.to_vec());
    let reference = prop::sample::select(ReferencePosition::ALL.to_vec());
    let width = prop_oneof![Just(None), float().prop_map(Some)];
    let strategy = (
        typed_theme(),
        position,
        reference,
        float(),
        any::<bool>(),
        width,
    );
    chunked(15_200, cases(500), strategy, |inputs| {
        let mut boxes = Vec::new();
        let mut applied = Vec::new();
        for (theme, position, reference, max, show, width) in &inputs {
            let b = boxes_for_layout_preset(*position, *reference, *max, *show);
            boxes.push((
                json!([position.as_str(), reference.as_str(), number(*max), show]),
                json!({"verse": box_json(&b.verse), "reference": box_json(&b.reference)}),
            ));
            // Finite widths only: a NaN width would be written into the theme.
            if width.is_none_or(f64::is_finite) {
                let out = apply_layout_preset(theme, *position, *width);
                let w = width.map_or_else(undefined, number);
                applied.push((json!([js(theme), position.as_str(), w]), js(&out)));
            }
        }
        assert_all_match("overlay-boxes.boxesForLayoutPreset", &boxes);
        assert_all_match("overlay-boxes.applyLayoutPreset", &applied);
    });
}

// ─── Shapes and elements ──────────────────────────────────────────────────────

#[test]
fn shape_lookup_matches() {
    let mut ids: Vec<Value> = OVERLAY_SHAPE_IDS.iter().map(|id| json!(id)).collect();
    ids.extend([json!(""), json!("nope"), json!("HEART"), undefined()]);
    let cases: Vec<_> = ids
        .iter()
        .map(|id| {
            let found = overlay_shape(id.as_str());
            (json!([id]), found.map_or(Value::Null, js))
        })
        .collect();
    assert_all_match("overlay-shapes.overlayShape", &cases);
}

#[test]
fn element_helpers_match() {
    let kind = prop::sample::select(OverlayElementKind::ALL.to_vec());
    let shape = prop_oneof![
        Just(None),
        shape_id().prop_map(|v| v.as_str().map(str::to_owned))
    ];
    let path = prop_oneof![Just(None), Just(Some("/media/x.png".to_owned()))];
    chunked(15_300, cases(1_000), (kind, shape, path), |inputs| {
        let mut media = Vec::new();
        let mut line = Vec::new();
        let mut label = Vec::new();
        let mut created = Vec::new();
        for (kind, shape, path) in &inputs {
            let mut pick = json!({"kind": kind.as_str()});
            if let Some(shape) = shape {
                pick["shape"] = json!(shape);
            }
            media.push((json!([pick]), json!(is_media_element(*kind))));
            line.push((
                json!([pick]),
                json!(is_line_element(*kind, shape.as_deref())),
            ));
            label.push((
                json!([pick]),
                json!(overlay_element_label(*kind, shape.as_deref())),
            ));
            let element = create_overlay_element(*kind, path.as_deref(), "el-1", shape.as_deref());
            let path_arg = path.as_ref().map_or_else(undefined, |p| json!(p));
            let shape_arg = shape.as_ref().map_or_else(undefined, |s| json!(s));
            created.push((
                json!([kind.as_str(), path_arg, "el-1", shape_arg]),
                js(&element),
            ));
        }
        assert_all_match("overlay-elements.isMediaElement", &media);
        assert_all_match("overlay-elements.isLineElement", &line);
        assert_all_match("overlay-elements.overlayElementLabel", &label);
        assert_all_match("overlay-elements.createOverlayElement", &created);
    });
}

#[test]
fn element_list_edits_match() {
    let list = elements().prop_map(|raw| normalize_overlay_elements(Some(&raw)));
    let alignment = prop::sample::select(vec![
        ("left", ElementAlignment::Left),
        ("center", ElementAlignment::Center),
        ("right", ElementAlignment::Right),
        ("top", ElementAlignment::Top),
        ("middle", ElementAlignment::Middle),
        ("bottom", ElementAlignment::Bottom),
    ]);
    let to = prop::sample::select(vec![
        ("forward", ReorderTarget::Forward),
        ("backward", ReorderTarget::Backward),
        ("front", ReorderTarget::Front),
        ("back", ReorderTarget::Back),
    ]);
    let target = prop::sample::select(vec!["a", "b", "c", "x y", "e3", "missing"]);
    chunked(
        15_400,
        cases(1_000),
        (list, alignment, to, target, raw_box()),
        |inputs| {
            let mut aligned = Vec::new();
            let mut removed = Vec::new();
            let mut duplicated = Vec::new();
            let mut reordered = Vec::new();
            let mut updated = Vec::new();
            for (list, (align_name, align), (to_name, to), target, frame) in &inputs {
                let el = js(list);
                aligned.push((
                    json!([{"box": box_json(frame)}, align_name]),
                    box_json(&align_overlay_element(frame, *align)),
                ));
                removed.push((
                    json!([el, target]),
                    js(&remove_overlay_element(list, target)),
                ));
                let (next, id) = duplicate_overlay_element(list, target, "copy");
                duplicated.push((
                    json!([el, target, "copy"]),
                    json!({"elements": js(&next), "id": id}),
                ));
                reordered.push((
                    json!([el, target, to_name]),
                    js(&reorder_overlay_element(list, target, *to)),
                ));
                let patched = update_overlay_element(list, target, |e| {
                    e.opacity = 0.25;
                    e.fill = "#123456".into();
                });
                updated.push((
                    json!([el, target, {"opacity": 0.25, "fill": "#123456"}]),
                    js(&patched),
                ));
            }
            assert_all_match("overlay-elements.alignOverlayElement", &aligned);
            assert_all_match("overlay-elements.removeOverlayElement", &removed);
            assert_all_match("overlay-elements.duplicateOverlayElement", &duplicated);
            assert_all_match("overlay-elements.reorderOverlayElement", &reordered);
            assert_all_match("overlay-elements.updateOverlayElement", &updated);
        },
    );
}

// ─── Output routing ───────────────────────────────────────────────────────────

#[test]
fn output_routing_matches() {
    chunked(15_500, cases(500), typed_outputs(), |inputs| {
        let mut rows: Vec<(&str, Vec<(Value, Value)>)> = [
            "surfaceOutputs",
            "previewOutputs",
            "primaryRenderedOutput",
            "renderedOutputs",
            "fallbackOutputs",
            "primaryNdiOutputId",
            "findNdiOutput",
            "getDispatchPlan",
        ]
        .into_iter()
        .map(|n| (n, Vec::new()))
        .collect();
        for outputs in &inputs {
            let arg = json!([js(outputs)]);
            let refs = |list: Vec<&OverlayOutput>| Value::Array(list.into_iter().map(js).collect());
            let plan = get_dispatch_plan(outputs, |_| true);
            let values = [
                refs(surface_outputs(outputs)),
                refs(preview_outputs(outputs)),
                opt(primary_rendered_output(outputs)),
                refs(rendered_outputs(outputs, |_| true)),
                refs(fallback_outputs(outputs)),
                json!(primary_ndi_output_id(outputs)),
                opt(find_ndi_output(outputs)),
                json!({"rendered": refs(plan.rendered), "fallbacks": refs(plan.fallbacks)}),
            ];
            for ((_, cases), value) in rows.iter_mut().zip(values) {
                cases.push((arg.clone(), value));
            }
        }
        for (name, cases) in &rows {
            assert_all_match(&format!("overlay-outputs.{name}"), cases);
        }
    });
}

#[test]
fn output_kind_helpers_match() {
    let mut label = Vec::new();
    let mut rendered = Vec::new();
    let mut local = Vec::new();
    let mut follows = Vec::new();
    for kind in OverlayOutputKind::ALL {
        // Deliberate deviation (DECISIONS 113): a display output is
        // "External display" here, "Kairo screen" in Electron.
        let native = output_destination_label(*kind);
        let electron = if *kind == OverlayOutputKind::Screen {
            assert_eq!(native, "External display");
            "Kairo screen"
        } else {
            native
        };
        label.push((json!([kind.as_str()]), json!(electron)));
        rendered.push((json!([kind.as_str()]), json!(is_rendered_kind(*kind))));
        local.push((json!([kind.as_str()]), json!(is_local_kind(*kind))));
        for source in OutputSource::ALL {
            follows.push((
                json!([{"kind": kind.as_str(), "source": source.as_str()}]),
                json!(follows_program(*kind, *source)),
            ));
        }
    }
    assert_all_match("overlay-outputs.outputDestinationLabel", &label);
    assert_all_match("overlay-outputs.isRenderedKind", &rendered);
    assert_all_match("overlay-outputs.isLocalKind", &local);
    assert_all_match("overlay-outputs.followsProgram", &follows);
    let mut kind_label = Vec::new();
    let mut layer_label = Vec::new();
    for kind in OverlayContentKind::ALL {
        kind_label.push((json!([kind.as_str()]), json!(content_kind_label(*kind))));
        for (name, layer) in [
            ("verse", OverlayLayer::Verse),
            ("reference", OverlayLayer::Reference),
        ] {
            layer_label.push((
                json!([kind.as_str(), name]),
                json!(overlay_layer_label(*kind, layer)),
            ));
        }
    }
    assert_all_match("overlay-outputs.contentKindLabel", &kind_label);
    assert_all_match("overlay-outputs.overlayLayerLabel", &layer_label);
}

#[test]
fn content_overrides_match() {
    let patch = (
        prop::option::of(prop::option::of(prop::sample::select(vec!["t1", "t2"]))),
        prop::option::of(typed_theme()),
        prop::option::of(prop::sample::select(vec!["{Text}", "{Reference}"])),
    );
    let strategy = (typed_output(), content_kind(), any::<bool>(), patch);
    chunked(15_600, cases(500), strategy, |inputs| {
        let mut rows: Vec<(&str, Vec<(Value, Value)>)> = [
            "inheritLyricsTheme",
            "outputVariant",
            "hasContentOverride",
            "outputThemeFor",
            "outputThemeIdFor",
            "outputTemplateFor",
            "withContentPatch",
            "setContentOverride",
        ]
        .into_iter()
        .map(|n| (n, Vec::new()))
        .collect();
        for (output, kind, on, (theme_id, theme, template)) in &inputs {
            let o = js(output);
            let k = kind.as_str();
            let mut patch_json = json!({});
            if let Some(id) = theme_id {
                patch_json["themeId"] = json!(id);
            }
            if let Some(theme) = theme {
                patch_json["theme"] = js(theme);
            }
            if let Some(template) = template {
                patch_json["template"] = json!(template);
            }
            let patch = VariantPatch {
                theme_id: theme_id.map(|id| id.map(str::to_owned)),
                theme: theme.clone(),
                template: template.map(str::to_owned),
            };
            let values = [
                (
                    json!([js(&output.theme)]),
                    js(&inherit_lyrics_theme(&output.theme)),
                ),
                (json!([o, k]), opt(output_variant(output, *kind))),
                (json!([o, k]), json!(has_content_override(output, *kind))),
                (json!([o, k]), js(&output_theme_for(output, *kind))),
                (json!([o, k]), json!(output_theme_id_for(output, *kind))),
                (json!([o, k]), json!(output_template_for(output, *kind))),
                (
                    json!([o, k, patch_json]),
                    js(&with_content_patch(output, *kind, patch)),
                ),
                (
                    json!([o, k, on]),
                    js(&set_content_override(output, *kind, *on)),
                ),
            ];
            for ((_, cases), value) in rows.iter_mut().zip(values) {
                cases.push(value);
            }
        }
        for (name, cases) in &rows {
            assert_all_match(&format!("overlay-outputs.{name}"), cases);
        }
    });
}

#[test]
fn live_theme_and_media_paths_match() {
    let strategy = (
        overlay_settings(),
        theme_library(),
        content_kind(),
        retire_input(),
    );
    chunked(15_700, cases(300), strategy, |inputs| {
        let mut live = Vec::new();
        let mut media = Vec::new();
        for (raw, library, kind, other) in &inputs {
            let overlay = normalize_overlay_settings(Some(raw));
            live.push((
                json!([js(&overlay), kind.as_str()]),
                js(&live_overlay_theme(&overlay, *kind)),
            ));
            media.push((
                json!([raw, library]),
                json!(configured_media_paths(Some(raw), Some(library))),
            ));
            media.push((
                json!([js(&overlay), other]),
                json!(configured_media_paths(Some(&js(&overlay)), Some(other))),
            ));
        }
        media.push((
            json!([undefined(), undefined()]),
            json!(configured_media_paths(None, None)),
        ));
        assert_all_match("overlay-outputs.liveOverlayTheme", &live);
        assert_all_match("overlay-outputs.configuredMediaPaths", &media);
    });
}

// ─── Overlay defaults builders ────────────────────────────────────────────────

#[test]
fn output_builders_match() {
    let mut made = Vec::new();
    for kind in OverlayOutputKind::ALL {
        for id in ["main", "", " x "] {
            made.push((
                json!([id, kind.as_str()]),
                js(&make_overlay_output(id, *kind)),
            ));
        }
    }
    assert_all_match("overlay-defaults.makeOverlayOutput", &made);
    let projector: Vec<_> = [0.0, 3.0, -1.5]
        .iter()
        .map(|order| (json!([order]), js(&make_projector_output(*order))))
        .collect();
    assert_all_match("overlay-defaults.makeProjectorOutput", &projector);
}

// ─── Content formatting ───────────────────────────────────────────────────────

fn verse() -> BoxedStrategy<ScriptureVerse> {
    (1.0..200.0f64, tricky_string())
        .prop_map(|(n, text)| ScriptureVerse {
            book: "Psalms".into(),
            chapter: 1.0,
            verse: n.floor(),
            text: text.as_str().unwrap_or_default().to_owned(),
            extra: Extra::new(),
        })
        .boxed()
}

#[test]
fn content_formatting_matches() {
    let max = prop_oneof![
        Just(None),
        prop::sample::select(vec![0.0, 1.0, 2.0, 2.5, -1.0, 10.0]).prop_map(Some)
    ];
    let text = || tricky_string().prop_map(|v| v.as_str().unwrap_or_default().to_owned());
    let template = prop::sample::select(vec![
        "{Reference}\n{Text}",
        " {Text} ",
        "{Reference}{Reference}",
        "plain",
        "",
    ]);
    let strategy = (
        prop::collection::vec(verse(), 0..5),
        any::<bool>(),
        max,
        text(),
        text(),
        template,
    );
    chunked(15_800, cases(1_000), strategy, |inputs| {
        let mut verses_out = Vec::new();
        let mut references = Vec::new();
        let mut cards = Vec::new();
        let mut templates = Vec::new();
        for (verses, numbers, max, a, b, template) in &inputs {
            let mut options = json!({"showVerseNumbers": numbers});
            if let Some(max) = max {
                options["maxVerses"] = json!(max);
            }
            verses_out.push((
                json!([js(verses), options]),
                json!(format_overlay_verse_text(verses, *numbers, *max)),
            ));
            references.push((
                json!([a, b, numbers]),
                json!(format_overlay_reference(a, b, *numbers)),
            ));
            cards.push((json!([a]), json!(format_card_reference(a))));
            templates.push((
                json!([template, {"reference": a, "text": b}]),
                json!(render_overlay_template(template, a, b)),
            ));
        }
        assert_all_match("overlay-content.formatOverlayVerseText", &verses_out);
        assert_all_match("overlay-content.formatOverlayReference", &references);
        assert_all_match("overlay-content.formatCardReference", &cards);
        assert_all_match("overlay-content.renderOverlayTemplate", &templates);
    });
}

// ─── Theme library ────────────────────────────────────────────────────────────

#[test]
fn theme_library_edits_match() {
    let name = prop::sample::select(vec![
        "  Sunday  ",
        "",
        "Untitled theme",
        "Untitled theme 2",
        " ",
    ]);
    let target = prop::sample::select(vec!["a", "b", "c", "missing"]);
    let strategy = (
        typed_library(),
        typed_theme(),
        content_kind(),
        name,
        target,
        0.0..1e12f64,
    );
    chunked(15_900, cases(300), strategy, |inputs| {
        let mut rows: Vec<(&str, Vec<(Value, Value)>)> = [
            "createCustomTheme",
            "nextUntitledThemeName",
            "themesForKind",
            "updateLibraryTheme",
            "setThemeBaseline",
            "themeBaseline",
            "isUntouchedThemeLibrary",
            "builtInThemesForKind",
        ]
        .into_iter()
        .map(|n| (n, Vec::new()))
        .collect();
        for (library, theme, kind, name, target, now) in &inputs {
            let lib = js(library);
            let t = js(theme);
            let k = kind.as_str();
            let refs =
                |list: Vec<&CustomOverlayTheme>| Value::Array(list.into_iter().map(js).collect());
            let mut values = vec![
                (
                    json!([name, t, k, now, "new-id"]),
                    js(&create_custom_theme(name, theme, *kind, *now, "new-id")),
                ),
                (
                    json!([lib, k]),
                    json!(next_untitled_theme_name(library, *kind)),
                ),
                (json!([lib, k]), refs(themes_for_kind(library, *kind))),
                (
                    json!([lib, target, {"name": name, "theme": t}, now]),
                    js(&update_library_theme(library, target, name, theme, *now)),
                ),
                (
                    json!([lib, target, t]),
                    js(&set_theme_baseline(library, target, theme)),
                ),
            ];
            values.extend(
                library
                    .first()
                    .map(|item| (json!([js(item)]), js(theme_baseline(item)))),
            );
            let mut untouched = vec![(json!([lib]), json!(is_untouched_theme_library(library)))];
            // The single first-launch theme, as built, renamed and edited.
            let placeholder =
                normalize_theme_library(None, &DEFAULT_OVERLAY_SETTINGS.theme, 1.0, "p");
            untouched.push((
                json!([js(&placeholder)]),
                json!(is_untouched_theme_library(&placeholder)),
            ));
            let mut legacy = placeholder.clone();
            legacy[0].theme.layout.backdrop_box = true;
            untouched.push((
                json!([js(&legacy)]),
                json!(is_untouched_theme_library(&legacy)),
            ));
            let mut edited = placeholder.clone();
            edited[0].theme = theme.clone();
            untouched.push((
                json!([js(&edited)]),
                json!(is_untouched_theme_library(&edited)),
            ));
            let built: Value = Value::Array(
                built_in_themes_for_kind(*kind)
                    .into_iter()
                    .map(|t| json!({"id": t.id, "name": t.name, "kind": t.kind.as_str(), "theme": js(&t.theme)}))
                    .collect(),
            );
            for ((_, cases), value) in rows.iter_mut().zip(values) {
                cases.push(value);
            }
            rows[6].1.extend(untouched);
            rows[7].1.push((json!([k]), built));
        }
        for (name, cases) in &rows {
            assert_all_match(&format!("theme-library.{name}"), cases);
        }
    });
}

#[test]
fn themes_on_outputs_match() {
    let defaults = (
        prop::option::of(prop::sample::select(vec!["a", "b", "missing"])),
        prop::option::of(prop::sample::select(vec!["a", "b", "c"])),
        any::<bool>(),
    );
    let strategy = (typed_outputs(), typed_library(), defaults);
    chunked(16_000, cases(300), strategy, |inputs| {
        let mut rows: Vec<(&str, Vec<(Value, Value)>)> = [
            "outputUsesTheme",
            "assignThemeToOutput",
            "unassignThemeFromOutput",
            "syncThemeToOutputs",
            "detachThemeFromOutputs",
            "withDefaultThemes",
        ]
        .into_iter()
        .map(|n| (n, Vec::new()))
        .collect();
        for (outputs, library, (scripture, lyrics, has_defaults)) in &inputs {
            let outs = js(outputs);
            let lib = js(library);
            let defaults = ThemeDefaults {
                scripture: scripture.map(str::to_owned),
                lyrics: lyrics.map(str::to_owned),
            };
            let mut defaults_json = json!({});
            if let Some(s) = scripture {
                defaults_json["scripture"] = json!(s);
            }
            if let Some(l) = lyrics {
                defaults_json["lyrics"] = json!(l);
            }
            for theme in library {
                let pick = json!({"id": theme.id, "kind": theme.kind.as_str()});
                let t = js(theme);
                rows[3]
                    .1
                    .push((json!([outs, t]), js(&sync_theme_to_outputs(outputs, theme))));
                rows[4].1.push((
                    json!([outs, pick]),
                    js(&detach_theme_from_outputs(outputs, &theme.id, theme.kind)),
                ));
                for output in outputs {
                    let o = js(output);
                    rows[0].1.push((
                        json!([o, pick]),
                        json!(output_uses_theme(output, &theme.id, theme.kind)),
                    ));
                    rows[1]
                        .1
                        .push((json!([o, t]), js(&assign_theme_to_output(output, theme))));
                    let assigned = assign_theme_to_output(output, theme);
                    rows[2].1.push((
                        json!([js(&assigned), pick]),
                        js(&unassign_theme_from_output(
                            &assigned, &theme.id, theme.kind,
                        )),
                    ));
                }
            }
            for output in outputs {
                let d = if *has_defaults {
                    json!(defaults_json)
                } else {
                    undefined()
                };
                let value = with_default_themes(output, library, has_defaults.then_some(&defaults));
                rows[5].1.push((json!([js(output), lib, d]), js(&value)));
            }
        }
        for (name, cases) in &rows {
            assert_all_match(&format!("theme-library.{name}"), cases);
        }
    });
}

/// `seedBuiltInThemes` draws random ids for all but the first theme, so the
/// ids after the first are compared as "some id" rather than exactly.
#[test]
fn seeding_built_in_themes_matches() {
    for library in [
        normalize_theme_library(None, &DEFAULT_OVERLAY_SETTINGS.theme, 1.0, "placeholder"),
        Vec::new(),
    ] {
        let mut n = 0;
        let seeded = seed_built_in_themes(&library, 1000.0, || {
            n += 1;
            format!("generated-{n}")
        });
        let mut expected = oracle_value(
            "theme-library.seedBuiltInThemes",
            json!([js(&library), 1000]),
        );
        let mut actual = js(&seeded);
        let skip = usize::from(!library.is_empty());
        for list in [&mut expected, &mut actual] {
            for item in list.as_array_mut().unwrap().iter_mut().skip(skip) {
                assert!(item["id"].as_str().is_some_and(|id| !id.is_empty()));
                item["id"] = json!("<random>");
            }
        }
        assert_eq!(compare(&expected, &actual), Vec::<String>::new());
    }
}

// ─── Displays, health, test pattern, props ────────────────────────────────────

fn display_info() -> BoxedStrategy<DisplayInfo> {
    (
        1u8..6,
        prop::sample::select(vec!["EPSON PJ", "Built-in", " EPSON PJ ", ""]),
        prop::sample::select(vec![(1920.0, 1080.0), (1280.0, 800.0), (1512.0, 982.0)]),
        prop::sample::select(vec![1.0, 2.0, 1.5]),
    )
        .prop_map(|(id, label, (w, h), scale)| DisplayInfo {
            id: f64::from(id),
            label: label.into(),
            bounds: DisplayBounds {
                x: 0.0,
                y: 0.0,
                width: w,
                height: h,
            },
            size: Size {
                width: w,
                height: h,
                extra: Extra::new(),
            },
            scale_factor: scale,
            primary: false,
            hosts_main_window: false,
        })
        .boxed()
}

fn binding_json(b: &DisplayBinding) -> Value {
    json!({"displayId": b.display_id, "displayLabel": b.display_label, "displaySize": opt(b.display_size.clone())})
}

#[test]
fn displays_match() {
    let binding = (
        prop::option::of(1u8..7),
        prop::sample::select(vec!["EPSON PJ", " EPSON PJ ", "", "Other"]),
        prop::option::of(prop::sample::select(vec![
            (1920.0, 1080.0),
            (1280.0, 800.0),
        ])),
    )
        .prop_map(|(id, label, size)| DisplayBinding {
            display_id: id.map(f64::from),
            display_label: label.into(),
            display_size: size.map(|(width, height)| Size {
                width,
                height,
                extra: Extra::new(),
            }),
        });
    let claimed = prop::collection::vec(1u8..6, 0..3);
    let strategy = (
        binding,
        prop::collection::vec(display_info(), 0..4),
        claimed,
    );
    chunked(16_100, cases(1_000), strategy, |inputs| {
        let mut resolved = Vec::new();
        let mut bound = Vec::new();
        let mut described = Vec::new();
        for (binding, displays, claimed) in &inputs {
            let claimed: Vec<f64> = claimed.iter().map(|c| f64::from(*c)).collect();
            let set = json!({"$oracle": "set", "value": claimed});
            resolved.push((
                json!([binding_json(binding), js(displays), set]),
                opt(resolve_display(binding, displays, &claimed)),
            ));
            for d in displays {
                bound.push((json!([js(d)]), binding_json(&binding_for_display(d))));
                described.push((
                    json!([js(d)]),
                    json!(describe_display(&d.label, &d.size, d.scale_factor)),
                ));
            }
        }
        assert_all_match("displays.resolveDisplay", &resolved);
        assert_all_match("displays.bindingForDisplay", &bound);
        assert_all_match("displays.describeDisplay", &described);
    });
}

fn dims_json(d: Option<Dimensions>) -> Value {
    d.map_or(
        Value::Null,
        |d| json!({"width": number(d.width), "height": number(d.height)}),
    )
}

#[test]
fn frame_shapes_match() {
    let dims = || {
        prop::option::of(
            (float(), float()).prop_map(|(width, height)| Dimensions { width, height }),
        )
    };
    let aspect = prop::sample::select(OutputAspect::ALL.to_vec());
    let bounds =
        (float(), float(), float(), float()).prop_map(|(x, y, width, height)| DisplayBounds {
            x,
            y,
            width,
            height,
        });
    let strategy = (aspect, dims(), dims(), bounds, any::<bool>(), float());
    chunked(16_200, cases(2_000), strategy, |inputs| {
        let mut ratio = Vec::new();
        let mut height = Vec::new();
        let mut layout = Vec::new();
        for (aspect, custom, display, bounds, rehearsal, frame) in &inputs {
            let a = aspect.as_str();
            ratio.push((
                json!([a, dims_json(*custom), dims_json(*display)]),
                aspect_ratio_of(*aspect, *custom, *display).map_or(Value::Null, number),
            ));
            if let Some(size) = display {
                height.push((
                    json!([dims_json(Some(*size)), a, dims_json(*custom)]),
                    number(frame_height_for(*size, *aspect, *custom)),
                ));
            }
            let out = test_pattern_layout(bounds, *rehearsal, *frame);
            let b = |v: &DisplayBounds| json!({"x": number(v.x), "y": number(v.y), "width": number(v.width), "height": number(v.height)});
            layout.push((
                json!([b(bounds), rehearsal, number(*frame)]),
                json!({"bounds": b(&out.bounds), "frame": dims_json(Some(out.frame))}),
            ));
        }
        assert_all_match("displays.aspectRatioOf", &ratio);
        assert_all_match("displays.frameHeightFor", &height);
        assert_all_match("displays.testPatternLayout", &layout);
    });
}

#[test]
fn dispatch_health_matches() {
    let result = (
        prop::sample::select(OverlayOutputKind::ALL.to_vec()),
        any::<bool>(),
        prop::option::of(prop::sample::select(vec!["Device disconnected", "", "x"])),
    )
        .prop_map(|(kind, ok, reason)| OverlayDispatchResult {
            output_id: kind.as_str().into(),
            name: kind.as_str().into(),
            kind,
            ok,
            reason: reason.map(str::to_owned),
        });
    chunked(
        16_300,
        cases(1_000),
        prop::collection::vec(result, 0..4),
        |inputs| {
            let cases: Vec<_> = inputs
                .iter()
                .map(|r| (json!([js(r)]), js(&dispatch_health(r))))
                .collect();
            assert_all_match("output-health.dispatchHealth", &cases);
        },
    );
}

#[test]
fn test_pattern_html_matches() {
    let name = tricky_string().prop_map(|v| v.as_str().unwrap_or_default().to_owned());
    chunked(16_400, cases(300), name, |inputs| {
        let mut cases = Vec::new();
        for name in &inputs {
            for pattern in VideoTestPattern::ALL {
                cases.push((
                    json!([pattern.as_str(), name]),
                    json!(output_test_pattern_html(*pattern, name)),
                ));
            }
        }
        cases.push((
            json!(["bars", "<img src=x onerror=alert(1)>"]),
            json!(output_test_pattern_html(
                VideoTestPattern::Bars,
                "<img src=x onerror=alert(1)>"
            )),
        ));
        assert_all_match("output-test-pattern.outputTestPatternHTML", &cases);
    });
}

#[test]
fn prop_gestures_match() {
    let unit = || prop_oneof![5 => 0.0..1.0f64, 1 => float()];
    let corner = prop::sample::select(vec![
        ("nw", PropCorner::Nw),
        ("ne", PropCorner::Ne),
        ("sw", PropCorner::Sw),
        ("se", PropCorner::Se),
    ]);
    let strategy = (
        (unit(), unit(), unit(), unit()),
        prop::option::of(corner),
        unit(),
        unit(),
    );
    chunked(16_500, cases(2_000), strategy, |inputs| {
        let cases: Vec<_> = inputs
            .iter()
            .map(|((x, y, w, h), corner, dx, dy)| {
                let origin = PropBox { x: *x, y: *y, w: *w, h: *h };
                let o = json!({"x": number(*x), "y": number(*y), "w": number(*w), "h": number(*h)});
                let (gesture, g) = match corner {
                    None => (
                        PropGesture::Move { object_id: "o".into(), start_x: 0.0, start_y: 0.0, origin },
                        json!({"kind": "move", "objectId": "o", "startX": 0, "startY": 0, "origin": o}),
                    ),
                    Some((name, c)) => (
                        PropGesture::Resize { object_id: "o".into(), corner: *c, start_x: 0.0, start_y: 0.0, origin },
                        json!({"kind": "resize", "objectId": "o", "corner": name, "startX": 0, "startY": 0, "origin": o}),
                    ),
                };
                let out = apply_prop_gesture(&gesture, *dx, *dy);
                (
                    json!([g, number(*dx), number(*dy)]),
                    json!({"x": number(out.x), "y": number(out.y), "w": number(out.w), "h": number(out.h)}),
                )
            })
            .collect();
        assert_all_match("prop-geometry.applyPropGesture", &cases);
    });
}

/// `number_to_string` is what makes the shape paths and box styles match;
/// a direct spot check keeps a failure here easy to read.
#[test]
fn shape_paths_use_javascript_number_text() {
    assert_eq!(
        number_to_string(50.0 + 50.0 * (-std::f64::consts::FRAC_PI_2).cos()),
        "50"
    );
}
