//! Rust ports of the Electron test files listed in prompt 013, one module per
//! file. Assertions on modules this prompt does not port are left to their
//! owners and named here:
//!
//! - `overlay-boxes.test.ts`, `overlay-elements.test.ts`: `renderOverlayHTML`,
//!   `estimateAutoFitVerseFontPx`, `sanitizeOverlayColor`,
//!   `formatOverlayColoredText` (`overlay-template.ts`) and the
//!   `overlay-fit.ts` video and fit helpers: prompt 051.
//! - `overlay-outputs.test.ts`: `withoutBackground`, `themeForPush`
//!   (`media-playback.ts`): prompt 016.
//! - `screen-settings.test.ts`: the file writes and async rejections belong
//!   to the settings store (prompt 021); the revision logic they exercise is
//!   ported here against `OutputSettingsWriter`.

use kairo_domain::json::to_js_value;
use kairo_domain::overlay::*;
use kairo_domain::overlay_defaults::*;
use kairo_domain::serde_util::Extra;
use serde_json::{Value, json};

fn js<T: serde::Serialize>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

fn red_theme() -> OverlayTheme {
    let mut theme = DEFAULT_OVERLAY_THEME.clone();
    theme.verse.color = "#ff0000".into();
    theme
}

fn output(id: &str, kind: OverlayOutputKind) -> OverlayOutput {
    make_overlay_output(id, kind)
}

fn legacy() -> LegacyOverlayFields {
    LegacyOverlayFields {
        mode: Some(LegacyOverlayMode::Auto),
        theme: DEFAULT_OVERLAY_THEME.clone(),
        template: "{Reference}\n{Text}".into(),
    }
}

fn outputs_of(raw: Value) -> Vec<OverlayOutput> {
    normalize_overlay_outputs(Some(&raw), &legacy())
}

fn settings_of(raw: Value) -> kairo_domain::settings::OverlaySettings {
    normalize_overlay_settings(Some(&raw))
}

fn ids(outputs: &[OverlayOutput]) -> Vec<&str> {
    outputs.iter().map(|o| o.id.as_str()).collect()
}

mod overlay_boxes {
    use super::*;
    use kairo_domain::overlay_boxes::*;

    #[test]
    fn clamps_overlay_boxes_onto_the_canvas() {
        let boxed = clamp_overlay_box(&overlay_box(-10.0, 90.0, 40.0, 40.0), TEXT_BOX_MIN);
        assert_eq!(boxed.x_pct, 0.0);
        assert!(boxed.y_pct + boxed.height_pct <= 100.0);
        assert!(boxed.width_pct >= 8.0);
    }

    #[test]
    fn moves_and_resizes_without_leaving_the_frame() {
        let start = overlay_box(10.0, 10.0, 40.0, 20.0);
        let moved = move_overlay_box(&start, 80.0, 80.0, TEXT_BOX_MIN);
        assert_eq!(moved.x_pct + moved.width_pct, 100.0);
        assert_eq!(moved.y_pct + moved.height_pct, 100.0);
        let resized = resize_overlay_box(&start, ResizeHandle::Se, 100.0, 100.0, TEXT_BOX_MIN);
        assert_eq!(resized.x_pct, 10.0);
        assert_eq!(resized.width_pct, 90.0);
        assert_eq!(resized.height_pct, 90.0);
    }

    #[test]
    fn layout_presets_stack_verse_and_reference_inside_the_frame() {
        let lower = boxes_for_layout_preset(
            OverlayLayoutPosition::LowerThird,
            ReferencePosition::Below,
            82.0,
            true,
        );
        assert!(lower.verse.y_pct > 50.0);
        assert!(lower.reference.y_pct > lower.verse.y_pct);
        assert_eq!(lower.verse.width_pct, 82.0);
        let full = boxes_for_layout_preset(
            OverlayLayoutPosition::Full,
            ReferencePosition::Below,
            82.0,
            true,
        );
        assert_eq!(full.verse.x_pct, 0.0);
        assert_eq!(full.verse.width_pct, 100.0);
    }

    #[test]
    fn snaps_the_reference_box_above_or_below_the_verse() {
        let verse = overlay_box(10.0, 40.0, 80.0, 20.0);
        let reference = overlay_box(0.0, 0.0, 50.0, 10.0);
        let below = place_reference_against_verse(&verse, &reference, ReferencePosition::Below);
        assert_eq!(below.x_pct, 10.0);
        assert_eq!(below.width_pct, 80.0);
        assert!(below.y_pct >= verse.y_pct + verse.height_pct);
    }

    #[test]
    fn migrates_themes_that_predate_canvas_boxes() {
        let mut raw = js(&*DEFAULT_OVERLAY_THEME);
        raw["verse"].as_object_mut().unwrap().remove("box");
        raw["reference"].as_object_mut().unwrap().remove("box");
        raw["layout"]["position"] = json!("center");
        let normalized = normalize_overlay_theme(Some(&raw));
        assert!(normalized.verse.frame.width_pct > 0.0);
        assert!(normalized.reference.text.frame.height_pct > 0.0);
        assert_eq!(normalized.layout.position, OverlayLayoutPosition::Center);
    }

    #[test]
    fn migrates_legacy_boolean_shadow_and_uppercase_reference() {
        let mut raw = js(&*DEFAULT_OVERLAY_THEME);
        let reference = raw["reference"].as_object_mut().unwrap();
        reference.remove("textTransform");
        reference.insert("uppercase".into(), json!(true));
        reference.insert("shadow".into(), json!(false));
        raw["verse"]["shadow"] = json!(true);
        let normalized = normalize_overlay_theme(Some(&raw));
        assert!(normalized.verse.shadow.enabled);
        assert!(!normalized.reference.text.shadow.enabled);
        assert_eq!(
            normalized.reference.text.text_transform,
            OverlayTextTransform::Uppercase
        );
        assert_eq!(
            normalized.verse.vertical_align,
            OverlayVerticalAlign::Middle
        );
    }
}

mod overlay_elements {
    use super::*;
    use kairo_domain::overlay_boxes::*;
    use kairo_domain::overlay_elements::*;
    use kairo_domain::overlay_outputs::configured_media_paths;
    use kairo_domain::overlay_shapes::OVERLAY_SHAPE_GROUPS;

    fn rect(id: &str) -> OverlayElement {
        create_overlay_element(OverlayElementKind::Rectangle, None, id, None)
    }

    #[test]
    fn a_theme_saved_before_elements_existed_normalizes_to_none() {
        let mut raw = js(&*DEFAULT_OVERLAY_THEME);
        raw.as_object_mut().unwrap().remove("elements");
        assert!(normalize_overlay_theme(Some(&raw)).elements.is_empty());
    }

    #[test]
    fn normalizing_elements_drops_junk_duplicates_and_out_of_range_values() {
        let elements = normalize_overlay_elements(Some(&json!([
            {"id": "a", "kind": "ellipse", "opacity": 4, "box": {"xPct": 90, "yPct": 0, "widthPct": 50, "heightPct": 10}},
            {"id": "a", "kind": "rectangle"},
            {"kind": "rectangle"},
            "nope",
            {"id": "b", "kind": "rectangle", "mediaPath": "/tmp/x.png", "fill": "url(javascript:1)"},
        ])));
        assert_eq!(
            elements.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(elements[0].opacity, 1.0);
        assert_eq!(elements[0].frame.x_pct + elements[0].frame.width_pct, 100.0);
        assert_eq!(elements[1].media_path, None);
        assert_eq!(elements[1].fill, "#ffffff");
    }

    #[test]
    fn a_theme_holds_at_most_max_overlay_elements() {
        let many: Vec<_> = (0..MAX_OVERLAY_ELEMENTS + 5)
            .map(|i| rect(&format!("r{i}")))
            .collect();
        assert_eq!(
            normalize_overlay_elements(Some(&js(&many))).len(),
            MAX_OVERLAY_ELEMENTS
        );
        assert_eq!(
            duplicate_overlay_element(&many[..MAX_OVERLAY_ELEMENTS], "r0", "copy").1,
            None
        );
    }

    #[test]
    fn elements_can_shrink_far_below_the_text_box_minimum() {
        let thin = resize_overlay_box(
            &overlay_box(10.0, 10.0, 50.0, 20.0),
            ResizeHandle::S,
            0.0,
            -19.5,
            ELEMENT_BOX_MIN,
        );
        assert_eq!(thin.height_pct, 0.5);
    }

    #[test]
    fn duplicate_lands_just_above_the_original_nudged() {
        let (elements, id) = duplicate_overlay_element(&[rect("a"), rect("b")], "a", "copy");
        assert_eq!(id.as_deref(), Some("copy"));
        assert_eq!(
            elements.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["a", "copy", "b"]
        );
        assert_eq!(elements[1].frame.x_pct, elements[0].frame.x_pct + 2.0);
    }

    #[test]
    fn reordering_stays_within_the_under_text_or_over_text_group() {
        let mut over = rect("over");
        over.above_text = true;
        let list = vec![rect("a"), rect("b"), over, rect("c")];
        let order = |els: Vec<OverlayElement>, above: bool| -> Vec<String> {
            els.into_iter()
                .filter(|e| e.above_text == above)
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(
            order(
                reorder_overlay_element(&list, "a", ReorderTarget::Front),
                false
            ),
            ["b", "c", "a"]
        );
        assert_eq!(
            order(
                reorder_overlay_element(&list, "c", ReorderTarget::Back),
                false
            ),
            ["c", "a", "b"]
        );
        assert_eq!(
            order(
                reorder_overlay_element(&list, "a", ReorderTarget::Forward),
                false
            ),
            ["b", "a", "c"]
        );
        assert_eq!(
            order(
                reorder_overlay_element(&list, "a", ReorderTarget::Backward),
                false
            ),
            ["a", "b", "c"]
        );
        assert_eq!(
            order(
                reorder_overlay_element(&list, "a", ReorderTarget::Front),
                true
            ),
            ["over"]
        );
    }

    #[test]
    fn element_media_and_a_theme_default_are_servable() {
        let mut with_element = DEFAULT_OVERLAY_THEME.clone();
        with_element.elements = vec![create_overlay_element(
            OverlayElementKind::Image,
            Some("/media/logo.png"),
            "i",
            None,
        )];
        let mut baseline = DEFAULT_OVERLAY_THEME.clone();
        baseline.elements = vec![create_overlay_element(
            OverlayElementKind::Video,
            Some("/media/old.mp4"),
            "v",
            None,
        )];
        let paths = configured_media_paths(
            Some(&json!({"theme": js(&*DEFAULT_OVERLAY_THEME), "outputs": []})),
            Some(&json!([{"id": "t", "theme": js(&with_element), "baseline": js(&baseline)}])),
        );
        assert!(paths.contains(&"/media/logo.png".to_owned()));
        assert!(paths.contains(&"/media/old.mp4".to_owned()));
    }

    const BOX: (f64, f64, f64, f64) = (20.0, 20.0, 20.0, 10.0);
    fn shape(b: &OverlayBox) -> f64 {
        b.height_pct / b.width_pct
    }

    #[test]
    fn shift_on_a_corner_scales_from_the_opposite_corner() {
        let start = overlay_box(BOX.0, BOX.1, BOX.2, BOX.3);
        let next =
            resize_overlay_box_keeping_aspect(&start, ResizeHandle::Se, 10.0, 1.0, ELEMENT_BOX_MIN);
        assert_eq!(shape(&next), shape(&start));
        assert_eq!(next.width_pct, 30.0);
        assert_eq!((next.x_pct, next.y_pct), (20.0, 20.0));
        let nw = resize_overlay_box_keeping_aspect(
            &start,
            ResizeHandle::Nw,
            -10.0,
            0.0,
            ELEMENT_BOX_MIN,
        );
        assert_eq!(nw.x_pct + nw.width_pct, 40.0);
        assert_eq!(nw.y_pct + nw.height_pct, 30.0);
    }

    #[test]
    fn shift_on_an_edge_keeps_the_shape_about_the_centre_line() {
        let start = overlay_box(BOX.0, BOX.1, BOX.2, BOX.3);
        let next =
            resize_overlay_box_keeping_aspect(&start, ResizeHandle::E, 20.0, 0.0, ELEMENT_BOX_MIN);
        assert_eq!(next.width_pct, 40.0);
        assert_eq!(next.height_pct, 20.0);
        assert_eq!(next.y_pct + next.height_pct / 2.0, 25.0);
    }

    #[test]
    fn shift_never_grows_a_box_past_the_frame() {
        let start = overlay_box(BOX.0, BOX.1, BOX.2, BOX.3);
        let next = resize_overlay_box_keeping_aspect(
            &start,
            ResizeHandle::Se,
            500.0,
            0.0,
            ELEMENT_BOX_MIN,
        );
        assert!(next.width_pct <= 100.0 && next.height_pct <= 100.0);
        assert_eq!(shape(&next), shape(&start));
    }

    #[test]
    fn rotation_is_clamped() {
        let element = normalize_overlay_elements(Some(&json!([{"id": "x", "rotationDeg": 400}])));
        assert_eq!(element[0].rotation_deg, 180.0);
    }

    #[test]
    fn every_catalog_shape_has_a_unique_id_and_a_drawable_path() {
        let shapes: Vec<_> = OVERLAY_SHAPE_GROUPS
            .iter()
            .flat_map(|g| &g.shapes)
            .collect();
        assert!(shapes.len() >= 80);
        let unique: std::collections::HashSet<_> = shapes.iter().map(|s| s.id).collect();
        assert_eq!(unique.len(), shapes.len());
        for shape in shapes {
            // /^M[\d.\s\-MLHVCQAZ]+$/
            assert!(
                shape.path.starts_with('M') && shape.path.len() > 1,
                "{}",
                shape.id
            );
            assert!(
                shape
                    .path
                    .chars()
                    .all(|c| c.is_ascii_digit() || " .-MLHVCQAZ".contains(c)),
                "{}",
                shape.id
            );
        }
    }

    #[test]
    fn a_new_line_starts_visible_and_flat() {
        let line = create_overlay_element(OverlayElementKind::Shape, None, "l", Some("line-arrow"));
        assert!(line.stroke.enabled);
        assert!(line.frame.width_pct > line.frame.height_pct);
    }

    #[test]
    fn a_shape_the_catalog_does_not_know_is_dropped_on_load() {
        let elements = normalize_overlay_elements(Some(&json!([
            {"id": "gone", "kind": "shape", "shape": "not-a-shape"},
            {"id": "kept", "kind": "shape", "shape": "heart"},
        ])));
        let got: Vec<_> = elements
            .iter()
            .map(|e| (e.id.as_str(), e.shape.as_deref()))
            .collect();
        assert_eq!(got, [("kept", Some("heart"))]);
    }

    #[test]
    fn flips_persist_through_theme_reload() {
        let mut arrow = create_overlay_element(
            OverlayElementKind::Shape,
            None,
            "arrow",
            Some("arrow-right"),
        );
        arrow.flip_x = true;
        arrow.flip_y = true;
        arrow.rotation_deg = 30.0;
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.elements = vec![arrow];
        let theme = normalize_overlay_theme(Some(&js(&theme)));
        assert!(theme.elements[0].flip_x && theme.elements[0].flip_y);
        let legacy =
            normalize_overlay_elements(Some(&json!([{"id": "legacy", "kind": "rectangle"}])));
        assert!(!legacy[0].flip_x && !legacy[0].flip_y);
    }

    #[test]
    fn alignment_moves_to_slide_edges_and_centres_without_resizing() {
        let frame = overlay_box(17.0, 23.0, 30.0, 20.0);
        for (alignment, x, y) in [
            (ElementAlignment::Left, 0.0, 23.0),
            (ElementAlignment::Center, 35.0, 23.0),
            (ElementAlignment::Right, 70.0, 23.0),
            (ElementAlignment::Top, 17.0, 0.0),
            (ElementAlignment::Middle, 17.0, 40.0),
            (ElementAlignment::Bottom, 17.0, 80.0),
        ] {
            assert_eq!(
                align_overlay_element(&frame, alignment),
                overlay_box(x, y, 30.0, 20.0)
            );
        }
    }
}

mod overlay_content {
    use kairo_domain::overlay_content::*;
    use kairo_domain::scripture::ScriptureVerse;
    use kairo_domain::serde_util::Extra;

    fn verse(n: f64, text: &str) -> ScriptureVerse {
        ScriptureVerse {
            book: "Psalms".into(),
            chapter: 1.0,
            verse: n,
            text: text.into(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn formats_overlay_reference_with_optional_translation_suffix() {
        assert_eq!(
            format_overlay_reference("John 3:16", "KJV", true),
            "John 3:16 (KJV)"
        );
        assert_eq!(
            format_overlay_reference("John 3:16", "KJV", false),
            "John 3:16"
        );
    }

    #[test]
    fn formats_a_card_reference_without_repeating_its_translation_badge() {
        assert_eq!(format_card_reference("Romans 8:10"), "Romans 8:10");
    }

    #[test]
    fn formats_overlay_verse_text_with_numbers_and_max_verses_truncation() {
        let verses = [verse(2.0, "Two"), verse(3.0, "Three"), verse(4.0, "Four")];
        assert_eq!(
            format_overlay_verse_text(&verses, true, None).as_deref(),
            Some("2 Two\n3 Three\n4 Four")
        );
        assert_eq!(
            format_overlay_verse_text(&verses, false, Some(2.0)).as_deref(),
            Some("Two\nThree…")
        );
        assert_eq!(format_overlay_verse_text(&[], true, None), None);
    }

    #[test]
    fn template_replacement_follows_replace_all() {
        assert_eq!(
            render_overlay_template(" {Reference}\n{Text} ", "John 3:16", "For God"),
            "John 3:16\nFor God"
        );
        // `$&` in a replacement is the matched token, as in JavaScript.
        assert_eq!(
            render_overlay_template("{Text}", "r", "cost $&"),
            "cost {Text}"
        );
    }
}

mod overlay_outputs {
    use super::*;
    use kairo_domain::overlay_elements::create_overlay_element;
    use kairo_domain::overlay_outputs::*;

    #[test]
    fn legacy_modes_migrate_only_independent_destinations() {
        for mode in LegacyOverlayMode::ALL {
            let fields = LegacyOverlayFields {
                mode: Some(*mode),
                ..legacy()
            };
            let outputs = normalize_overlay_outputs(None, &fields);
            assert_eq!(
                outputs.iter().map(|o| o.kind).collect::<Vec<_>>(),
                [OverlayOutputKind::Ndi, OverlayOutputKind::Screen]
            );
            assert_eq!(outputs[0].enabled, *mode != LegacyOverlayMode::Message);
            assert!(!outputs[1].enabled);
        }
    }

    #[test]
    fn ndi_outputs_beyond_the_sender_limit_are_disabled_not_deleted() {
        let mut list: Vec<OverlayOutput> = (0..=MAX_NDI_OUTPUTS)
            .map(|i| OverlayOutput {
                enabled: true,
                ..output(&format!("ndi-{i}"), OverlayOutputKind::Ndi)
            })
            .collect();
        list.push(OverlayOutput {
            enabled: true,
            ..output("card", OverlayOutputKind::Device)
        });
        let outputs = outputs_of(js(&list));
        assert_eq!(ids(&outputs), ids(&list));
        let enabled: Vec<bool> = outputs
            .iter()
            .filter(|o| o.kind == OverlayOutputKind::Ndi)
            .map(|o| o.enabled)
            .collect();
        let mut expected = vec![true; MAX_NDI_OUTPUTS];
        expected.push(false);
        assert_eq!(enabled, expected);
    }

    #[test]
    fn duplicate_blank_and_unknown_kind_entries_are_healed() {
        let outputs = outputs_of(json!([
            {"id": "a", "kind": "screen", "enabled": true},
            {"id": "a", "kind": "screen", "enabled": true},
            {"id": "", "kind": "screen"},
            {"id": "b", "kind": "teleport", "enabled": true},
            "not an object",
        ]));
        assert_eq!(ids(&outputs), ["a"]);
    }

    #[test]
    fn an_empty_saved_list_gets_the_main_screen_back() {
        let outputs = outputs_of(json!([]));
        assert_eq!(
            outputs
                .iter()
                .map(|o| (o.kind, o.name.as_str()))
                .collect::<Vec<_>>(),
            [(OverlayOutputKind::Screen, "Main Screen")]
        );
        assert!(!normalize_overlay_outputs(Some(&json!("nope")), &legacy()).is_empty());
        assert!(!normalize_overlay_outputs(Some(&Value::Null), &legacy()).is_empty());
    }

    #[test]
    fn each_outputs_theme_is_clamped_through_the_theme_normalizer() {
        let outputs = outputs_of(
            json!([{"id": "main", "kind": "ndi", "theme": {"verse": {"fontSizePx": 9999, "color": "javascript:alert(1)"}}}]),
        );
        assert_eq!(outputs[0].theme.verse.font_size_px, 200.0);
        assert_eq!(
            outputs[0].theme.verse.color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
    }

    #[test]
    fn normalization_strips_retired_settings_and_output_fields() {
        let settings = settings_of(
            json!({"mode": "ndi", "ppVideoInputUuid": "uuid-9", "outputs": [
                {"id": "feed", "kind": "ndi", "enabled": true, "ppVideoInputUuid": "uuid-9", "lookId": "look-1"},
                {"id": "old", "kind": "message", "enabled": true},
            ]}),
        );
        assert_eq!(ids(&settings.outputs), ["feed"]);
        let value = js(&settings);
        for key in ["mode", "ppVideoInputUuid"] {
            assert!(value.get(key).is_none());
        }
        for key in ["ppVideoInputUuid", "lookId"] {
            assert!(value["outputs"][0].get(key).is_none());
        }
    }

    fn overlay_with(outputs: Vec<OverlayOutput>) -> kairo_domain::settings::OverlaySettings {
        kairo_domain::settings::OverlaySettings {
            outputs,
            ..DEFAULT_OVERLAY_SETTINGS.clone()
        }
    }

    #[test]
    fn live_overlay_theme_prefers_the_enabled_ndi_output_over_a_disabled_one() {
        let overlay = overlay_with(vec![
            output("off", OverlayOutputKind::Ndi),
            OverlayOutput {
                enabled: true,
                theme: red_theme(),
                ..output("on", OverlayOutputKind::Ndi)
            },
        ]);
        assert_eq!(
            live_overlay_theme(&overlay, OverlayContentKind::Scripture)
                .verse
                .color,
            "#ff0000"
        );
    }

    #[test]
    fn an_output_with_no_override_resolves_lyrics_to_its_scripture_theme_and_template() {
        let o = OverlayOutput {
            theme: red_theme(),
            template: "{Text}".into(),
            ..output("main", OverlayOutputKind::Ndi)
        };
        assert!(!has_content_override(&o, OverlayContentKind::Lyrics));
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Lyrics).verse.color,
            "#ff0000"
        );
        assert_eq!(
            output_template_for(&o, OverlayContentKind::Lyrics),
            "{Text}"
        );
    }

    #[test]
    fn lyrics_inheriting_scripture_leave_fit_backdrop_and_elements_behind() {
        let mut fitted = red_theme();
        fitted.layout.auto_fit_text = true;
        fitted.layout.backdrop_box = true;
        fitted.elements = vec![create_overlay_element(
            OverlayElementKind::Rectangle,
            None,
            "logo",
            None,
        )];
        let o = OverlayOutput {
            theme: fitted.clone(),
            ..output("main", OverlayOutputKind::Ndi)
        };
        let scripture = output_theme_for(&o, OverlayContentKind::Scripture);
        let lyrics = output_theme_for(&o, OverlayContentKind::Lyrics);
        assert!(scripture.layout.auto_fit_text && scripture.layout.backdrop_box);
        assert_eq!(scripture.elements.len(), 1);
        assert!(!lyrics.layout.auto_fit_text && !lyrics.layout.backdrop_box);
        assert!(lyrics.elements.is_empty());
        assert_eq!(lyrics.verse.color, "#ff0000");
        assert!(!inherit_lyrics_theme(&fitted).layout.auto_fit_text);
        let with_override = set_content_override(&o, OverlayContentKind::Lyrics, true);
        let seeded = output_theme_for(&with_override, OverlayContentKind::Lyrics);
        assert!(!seeded.layout.backdrop_box && !seeded.layout.auto_fit_text);
        assert!(seeded.elements.is_empty());
    }

    #[test]
    fn a_new_theme_has_no_backdrop_box() {
        assert!(!DEFAULT_OVERLAY_THEME.layout.backdrop_box);
    }

    #[test]
    fn an_applied_lyrics_theme_keeps_its_own_fit_to_box() {
        let mut lyrics_theme = DEFAULT_OVERLAY_THEME.clone();
        lyrics_theme.layout.auto_fit_text = true;
        let base = set_content_override(
            &output("main", OverlayOutputKind::Ndi),
            OverlayContentKind::Lyrics,
            true,
        );
        let o = with_content_patch(
            &base,
            OverlayContentKind::Lyrics,
            VariantPatch {
                theme_id: Some(Some("lyrics-stage".into())),
                theme: Some(lyrics_theme),
                template: None,
            },
        );
        assert!(
            output_theme_for(&o, OverlayContentKind::Lyrics)
                .layout
                .auto_fit_text
        );
        assert!(
            !output_theme_for(&o, OverlayContentKind::Scripture)
                .layout
                .auto_fit_text
        );
    }

    #[test]
    fn enabling_the_lyrics_override_seeds_styling_but_drops_the_citation() {
        let base = OverlayOutput {
            theme: red_theme(),
            theme_id: Some("theme-1".into()),
            ..output("main", OverlayOutputKind::Ndi)
        };
        let o = set_content_override(&base, OverlayContentKind::Lyrics, true);
        assert!(has_content_override(&o, OverlayContentKind::Lyrics));
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Lyrics).verse.color,
            "#ff0000"
        );
        assert_eq!(
            output_theme_id_for(&o, OverlayContentKind::Lyrics),
            Some("theme-1")
        );
        assert!(
            !output_theme_for(&o, OverlayContentKind::Lyrics)
                .reference
                .show
        );
        assert_eq!(
            output_template_for(&o, OverlayContentKind::Lyrics),
            "{Text}"
        );
        assert!(
            output_theme_for(&o, OverlayContentKind::Scripture)
                .reference
                .show
        );
        assert_eq!(
            output_template_for(&o, OverlayContentKind::Scripture),
            "{Reference}\n{Text}"
        );
    }

    #[test]
    fn editing_one_slot_leaves_the_other_untouched() {
        let base = set_content_override(
            &OverlayOutput {
                template: "{Reference}".into(),
                ..output("main", OverlayOutputKind::Ndi)
            },
            OverlayContentKind::Lyrics,
            true,
        );
        let o = with_content_patch(
            &base,
            OverlayContentKind::Lyrics,
            VariantPatch {
                theme: Some(red_theme()),
                template: Some("{Text}".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Lyrics).verse.color,
            "#ff0000"
        );
        assert_eq!(
            output_template_for(&o, OverlayContentKind::Lyrics),
            "{Text}"
        );
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Scripture)
                .verse
                .color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
        assert_eq!(
            output_template_for(&o, OverlayContentKind::Scripture),
            "{Reference}"
        );
        // A scripture patch writes the base fields even with an override.
        let s = with_content_patch(
            &base,
            OverlayContentKind::Scripture,
            VariantPatch {
                template: Some("{Text}".into()),
                ..Default::default()
            },
        );
        assert_eq!(
            output_template_for(&s, OverlayContentKind::Scripture),
            "{Text}"
        );
        assert!(has_content_override(&s, OverlayContentKind::Lyrics));
    }

    #[test]
    fn turning_the_override_off_returns_lyrics_to_the_scripture_theme() {
        let with = with_content_patch(
            &set_content_override(
                &output("main", OverlayOutputKind::Ndi),
                OverlayContentKind::Lyrics,
                true,
            ),
            OverlayContentKind::Lyrics,
            VariantPatch {
                theme: Some(red_theme()),
                ..Default::default()
            },
        );
        let o = set_content_override(&with, OverlayContentKind::Lyrics, false);
        assert_eq!(o.lyrics, None);
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Lyrics).verse.color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
    }

    #[test]
    fn live_overlay_theme_answers_per_content_kind() {
        let on = OverlayOutput {
            enabled: true,
            ..output("on", OverlayOutputKind::Ndi)
        };
        let o = with_content_patch(
            &set_content_override(&on, OverlayContentKind::Lyrics, true),
            OverlayContentKind::Lyrics,
            VariantPatch {
                theme: Some(red_theme()),
                ..Default::default()
            },
        );
        let overlay = overlay_with(vec![o]);
        assert_eq!(
            live_overlay_theme(&overlay, OverlayContentKind::Scripture)
                .verse
                .color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
        assert_eq!(
            live_overlay_theme(&overlay, OverlayContentKind::Lyrics)
                .verse
                .color,
            "#ff0000"
        );
    }

    #[test]
    fn a_stored_lyrics_override_survives_normalization_and_keeps_its_background() {
        let mut bg = red_theme();
        bg.background.kind = OverlayBackgroundType::Video;
        bg.background.media_path = Some("/tmp/loop.mp4".into());
        let outputs = outputs_of(json!([
            {"id": "a", "kind": "ndi", "lyrics": {"themeId": "t", "theme": js(&bg), "template": "{Text}"}},
            {"id": "b", "kind": "screen"},
        ]));
        let lyrics = outputs[0].lyrics.as_ref().unwrap();
        assert_eq!(lyrics.template, "{Text}");
        assert_eq!(lyrics.theme.verse.color, "#ff0000");
        assert_eq!(lyrics.theme.background.kind, OverlayBackgroundType::Video);
        assert_eq!(outputs[1].lyrics, None);
    }

    #[test]
    fn seeding_a_lyrics_override_strips_an_inherited_background() {
        let mut theme = DEFAULT_OVERLAY_THEME.clone();
        theme.background.kind = OverlayBackgroundType::Gradient;
        let o = set_content_override(
            &OverlayOutput {
                theme,
                ..output("main", OverlayOutputKind::Ndi)
            },
            OverlayContentKind::Lyrics,
            true,
        );
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Lyrics)
                .background
                .kind,
            OverlayBackgroundType::Transparent
        );
        assert_eq!(
            output_theme_for(&o, OverlayContentKind::Scripture)
                .background
                .kind,
            OverlayBackgroundType::Gradient
        );
    }

    #[test]
    fn screen_outputs_keep_display_fields_and_others_clear_them() {
        let outputs = outputs_of(json!([
            {"id": "proj", "kind": "screen", "enabled": true, "displayId": 7, "displayLabel": " DELL ", "displaySize": {"width": 1920, "height": 1080}},
            {"id": "feed", "kind": "ndi", "displayId": 7, "displayLabel": "junk"},
        ]));
        assert_eq!(outputs[0].display_id, Some(7.0));
        assert_eq!(outputs[0].display_label, "DELL");
        assert_eq!(
            js(&outputs[0].display_size),
            json!({"width": 1920, "height": 1080})
        );
        assert_eq!(outputs[1].display_id, None);
        assert_eq!(outputs[1].display_label, "");
        assert_eq!(outputs[1].display_size, None);
    }

    #[test]
    fn an_invalid_display_id_or_size_heals_to_null() {
        // NaN and Infinity cannot appear in JSON; the rest of the TypeScript list can.
        for display_id in [json!("7"), json!(1.5), Value::Null] {
            let outputs =
                outputs_of(json!([{"id": "p", "kind": "screen", "displayId": display_id}]));
            assert_eq!(outputs[0].display_id, None);
        }
        let outputs = outputs_of(json!([{"id": "p", "kind": "screen"}]));
        assert_eq!(outputs[0].display_id, None);
        let outputs = outputs_of(
            json!([{"id": "p", "kind": "screen", "displaySize": {"width": -1, "height": 1080}}]),
        );
        assert_eq!(outputs[0].display_size, None);
    }

    #[test]
    fn a_second_enabled_screen_on_the_same_display_is_disabled() {
        let screen = |id: &str, display: f64| OverlayOutput {
            enabled: true,
            display_id: Some(display),
            ..output(id, OverlayOutputKind::Screen)
        };
        let outputs = outputs_of(js(&vec![
            screen("a", 3.0),
            screen("b", 3.0),
            screen("c", 4.0),
        ]));
        assert_eq!(
            outputs
                .iter()
                .map(|o| (o.id.as_str(), o.enabled))
                .collect::<Vec<_>>(),
            [("a", true), ("b", false), ("c", true)]
        );
    }

    #[test]
    fn explicit_lists_do_not_recreate_a_removed_projector() {
        let once = outputs_of(js(&vec![OverlayOutput {
            enabled: true,
            ..output("main", OverlayOutputKind::Ndi)
        }]));
        assert!(!once.iter().any(|o| o.kind == OverlayOutputKind::Screen));
        let twice = outputs_of(js(&once));
        assert!(!twice.iter().any(|o| o.kind == OverlayOutputKind::Screen));
        let migrated = normalize_overlay_outputs(None, &legacy());
        assert_eq!(
            migrated
                .iter()
                .filter(|o| o.kind == OverlayOutputKind::Screen && !o.enabled)
                .count(),
            1
        );
    }

    #[test]
    fn live_overlay_theme_prefers_an_enabled_screen_then_ndi() {
        let mut screen_theme = DEFAULT_OVERLAY_THEME.clone();
        screen_theme.verse.color = "#00ff00".into();
        let mut overlay = overlay_with(vec![
            OverlayOutput {
                enabled: true,
                theme: red_theme(),
                ..output("main", OverlayOutputKind::Ndi)
            },
            OverlayOutput {
                enabled: true,
                theme: screen_theme,
                ..output("projector", OverlayOutputKind::Screen)
            },
        ]);
        assert_eq!(
            live_overlay_theme(&overlay, OverlayContentKind::Scripture)
                .verse
                .color,
            "#00ff00"
        );
        overlay.outputs[1].enabled = false;
        assert_eq!(
            live_overlay_theme(&overlay, OverlayContentKind::Scripture)
                .verse
                .color,
            "#ff0000"
        );
    }

    #[test]
    fn playlist_fields_are_clamped_and_ndi_always_follows_the_program() {
        let mut lobby = js(&output("lobby", OverlayOutputKind::Screen));
        lobby["source"] = json!("playlist");
        lobby["playlistId"] = json!(" p1 ");
        lobby["slideSec"] = json!(1);
        let mut feed = js(&output("feed", OverlayOutputKind::Ndi));
        feed["source"] = json!("playlist");
        feed["playlistId"] = json!("p1");
        feed["slideSec"] = json!(9999);
        let outputs = outputs_of(json!([lobby, feed]));
        assert_eq!(outputs[0].source, OutputSource::Playlist);
        assert_eq!(outputs[0].playlist_id, "p1");
        assert_eq!(outputs[0].slide_sec, 3.0);
        assert_eq!(outputs[1].source, OutputSource::Program);
        assert_eq!(outputs[1].playlist_id, "");
        let mut bogus = js(&output("s", OverlayOutputKind::Screen));
        bogus["source"] = json!("bogus");
        assert_eq!(outputs_of(json!([bogus]))[0].source, OutputSource::Program);
    }

    #[test]
    fn retired_auto_clear_values_are_discarded() {
        let mut raw = js(&*DEFAULT_OVERLAY_SETTINGS);
        raw["autoClearSec"] = json!(15);
        assert!(js(&settings_of(raw)).get("autoClearSec").is_none());
        assert!(js(&*DEFAULT_OVERLAY_SETTINGS).get("autoClearSec").is_none());
    }

    #[test]
    fn syphon_outputs_are_kept_with_at_most_four_switched_on() {
        let outputs: Vec<Value> = (0..6).map(|i| json!({"id": format!("syphon-{i}"), "kind": "syphon", "name": format!("Syphon {i}"), "enabled": true})).collect();
        let normalized: Vec<OverlayOutput> = settings_of(json!({"outputs": outputs}))
            .outputs
            .into_iter()
            .filter(|o| o.kind == OverlayOutputKind::Syphon)
            .collect();
        assert_eq!(normalized.len(), 6);
        assert_eq!(normalized.iter().filter(|o| o.enabled).count(), 4);
        assert!(is_rendered_kind(OverlayOutputKind::Syphon));
    }

    #[test]
    fn preview_picker_includes_configured_outputs_in_order() {
        let main = OverlayOutput {
            name: "Main Screen".into(),
            order: 0.0,
            ..output("main", OverlayOutputKind::Screen)
        };
        let lower = OverlayOutput {
            name: "Lower Third".into(),
            enabled: true,
            order: 1.0,
            ..output("lower", OverlayOutputKind::Device)
        };
        let list = [lower, main];
        let picked: Vec<_> = preview_outputs(&list)
            .into_iter()
            .map(|o| (o.name.as_str(), o.enabled))
            .collect();
        assert_eq!(picked, [("Main Screen", false), ("Lower Third", true)]);
    }
}

mod theme_library {
    use super::*;
    use kairo_domain::theme_library::*;

    fn custom(name: &str, kind: OverlayContentKind, id: &str) -> CustomOverlayTheme {
        create_custom_theme(name, &DEFAULT_OVERLAY_THEME, kind, 1.0, id)
    }

    #[test]
    fn creates_a_named_custom_theme_with_an_independent_snapshot() {
        let mut source = DEFAULT_OVERLAY_THEME.clone();
        let saved = create_custom_theme(
            "  Sunday Warmth  ",
            &source,
            OverlayContentKind::Scripture,
            1234.0,
            "theme-1",
        );
        assert_eq!(saved.id, "theme-1");
        assert_eq!(saved.name, "Sunday Warmth");
        assert_eq!(saved.kind, OverlayContentKind::Scripture);
        assert_eq!(saved.created_at, 1234.0);
        assert_eq!(saved.theme, source);
        source.verse.color = "#000000".into();
        assert_ne!(saved.theme.verse.color, source.verse.color);
    }

    #[test]
    fn migrates_the_active_legacy_theme_into_my_current_theme() {
        let mut active = DEFAULT_OVERLAY_THEME.clone();
        active.background.color = "#123456".into();
        let library = normalize_theme_library(None, &active, 2000.0, "legacy-theme");
        assert_eq!(library.len(), 1);
        assert_eq!(library[0].name, "My current theme");
        assert_eq!(library[0].id, "legacy-theme");
        assert_eq!(library[0].theme.background.color, "#123456");
    }

    #[test]
    fn normalizes_saved_themes_removes_duplicates_and_defaults_kind() {
        let t = js(&*DEFAULT_OVERLAY_THEME);
        let library = normalize_theme_library(
            Some(&json!([
                {"id": "one", "name": " First ", "createdAt": 1, "updatedAt": 2, "theme": t},
                {"id": "one", "name": "Duplicate", "createdAt": 3, "updatedAt": 3, "theme": t},
                {"id": "lyric", "name": "New", "kind": "lyrics", "createdAt": 1, "updatedAt": 1, "theme": t},
                {"id": "bogus", "name": "Bad", "kind": "nonsense", "createdAt": 1, "updatedAt": 1, "theme": t},
            ])),
            &DEFAULT_OVERLAY_THEME,
            0.0,
            "x",
        );
        assert_eq!(
            library.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["one", "lyric", "bogus"]
        );
        assert_eq!(library[0].name, "First");
        assert_eq!(library[0].kind, OverlayContentKind::Scripture);
        assert_eq!(library[1].kind, OverlayContentKind::Lyrics);
        assert_eq!(library[2].kind, OverlayContentKind::Scripture);
    }

    #[test]
    fn themes_for_kind_and_untitled_names() {
        let library = [
            custom("Untitled theme", OverlayContentKind::Scripture, "a"),
            custom("Untitled theme 2", OverlayContentKind::Scripture, "b"),
            custom("Untitled theme", OverlayContentKind::Lyrics, "c"),
        ];
        assert_eq!(
            next_untitled_theme_name(&[], OverlayContentKind::Scripture),
            "Untitled theme"
        );
        assert_eq!(
            next_untitled_theme_name(&library, OverlayContentKind::Scripture),
            "Untitled theme 3"
        );
        assert_eq!(
            next_untitled_theme_name(&library, OverlayContentKind::Lyrics),
            "Untitled theme 2"
        );
        assert_eq!(
            themes_for_kind(&library, OverlayContentKind::Lyrics)
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            ["c"]
        );
    }

    #[test]
    fn saving_one_library_theme_leaves_every_other_untouched() {
        let warm = custom("Warm", OverlayContentKind::Scripture, "warm");
        let cool = custom("Cool", OverlayContentKind::Scripture, "cool");
        let mut draft = warm.theme.clone();
        draft.verse.color = "#ff0000".into();
        draft.verse.font_size_px = 72.0;
        draft.layout.auto_fit_text = true;
        let next = update_library_theme(&[warm, cool.clone()], "warm", "Warm", &draft, 99.0);
        assert_eq!(next[1], cool);
        assert_eq!(next[0].theme.verse.color, "#ff0000");
        assert_eq!(next[0].updated_at, 99.0);
    }

    #[test]
    fn assigning_and_unassigning_themes_on_outputs() {
        let warm = custom("Warm", OverlayContentKind::Scripture, "warm");
        let o = assign_theme_to_output(&output("main", OverlayOutputKind::Screen), &warm);
        assert_eq!(o.theme_id.as_deref(), Some("warm"));
        assert!(output_uses_theme(&o, "warm", OverlayContentKind::Scripture));
        assert_eq!(o.lyrics, None);

        let stage = custom("Stage", OverlayContentKind::Lyrics, "stage");
        let on = assign_theme_to_output(&output("main", OverlayOutputKind::Screen), &stage);
        let lyrics = on.lyrics.as_ref().unwrap();
        assert_eq!(lyrics.theme_id.as_deref(), Some("stage"));
        assert_eq!(
            lyrics.theme.background.kind,
            OverlayBackgroundType::Transparent
        );
        assert_eq!(on.theme_id, None);
        assert_eq!(
            unassign_theme_from_output(&on, "stage", OverlayContentKind::Lyrics).lyrics,
            None
        );
    }

    #[test]
    fn syncing_and_detaching() {
        let theme = custom("Warm", OverlayContentKind::Scripture, "warm");
        let using = assign_theme_to_output(&output("a", OverlayOutputKind::Screen), &theme);
        let other = output("b", OverlayOutputKind::Screen);
        let mut edited = theme.clone();
        edited.theme.verse.color = "#ff0000".into();
        let synced = sync_theme_to_outputs(&[using.clone(), other.clone()], &edited);
        assert_eq!(synced[0].theme.verse.color, "#ff0000");
        assert_eq!(synced[1], other);
        let detached = detach_theme_from_outputs(
            std::slice::from_ref(&using),
            "warm",
            OverlayContentKind::Scripture,
        );
        assert_eq!(detached[0].theme_id, None);
        assert_eq!(detached[0].theme, using.theme);
    }

    #[test]
    fn a_new_install_is_seeded_with_every_built_in() {
        let placeholder =
            normalize_theme_library(None, &DEFAULT_OVERLAY_SETTINGS.theme, 1.0, "placeholder");
        assert!(is_untouched_theme_library(&[]));
        assert!(is_untouched_theme_library(&placeholder));
        let mut n = 0;
        let seeded = seed_built_in_themes(&placeholder, 1000.0, || {
            n += 1;
            format!("id-{n}")
        });
        assert_eq!(
            seeded.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            BUILT_IN_THEMES.iter().map(|t| t.name).collect::<Vec<_>>()
        );
        assert_eq!(seeded[0].id, placeholder[0].id);
        assert_eq!(seeded[0].name, "Broadcast");
        let lyrics: Vec<_> = seeded
            .iter()
            .filter(|t| t.kind == OverlayContentKind::Lyrics)
            .map(|t| t.name.as_str())
            .collect();
        assert_eq!(lyrics, ["Stage lyrics", "Lyrics lower third"]);
        let mut renamed = placeholder.clone();
        renamed[0].name = "Sunday".into();
        assert!(!is_untouched_theme_library(&renamed));
        assert!(!is_untouched_theme_library(&seeded));
    }

    #[test]
    fn edits_leave_the_default_alone_until_set_as_default() {
        let red = red_theme();
        let library = [custom("Warm", OverlayContentKind::Scripture, "warm")];
        let edited = update_library_theme(&library, "warm", "Warm", &red, 2.0);
        assert_eq!(edited[0].theme.verse.color, "#ff0000");
        assert_eq!(
            theme_baseline(&edited[0]).verse.color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
        let committed = set_theme_baseline(&edited, "warm", &red);
        assert_eq!(theme_baseline(&committed[0]).verse.color, "#ff0000");
    }

    #[test]
    fn a_theme_saved_before_defaults_existed_takes_its_pre_edit_look() {
        let legacy = normalize_theme_library(
            Some(
                &json!([{"id": "old", "name": "Old", "kind": "scripture", "createdAt": 1, "updatedAt": 1, "theme": js(&*DEFAULT_OVERLAY_THEME)}]),
            ),
            &DEFAULT_OVERLAY_THEME,
            0.0,
            "x",
        );
        assert_eq!(legacy[0].baseline, None);
        let edited = update_library_theme(&legacy, "old", "Old", &red_theme(), 2.0);
        assert_eq!(
            theme_baseline(&edited[0]).verse.color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
        let stored = normalize_theme_library(Some(&js(&edited)), &DEFAULT_OVERLAY_THEME, 0.0, "x");
        assert_eq!(
            theme_baseline(&stored[0]).verse.color,
            DEFAULT_OVERLAY_THEME.verse.color
        );
    }

    #[test]
    fn a_lyrics_theme_keeps_its_background_when_assigned() {
        let mut with_image = DEFAULT_OVERLAY_THEME.clone();
        with_image.background.kind = OverlayBackgroundType::Image;
        with_image.background.media_path = Some("/tmp/a.png".into());
        let theme = create_custom_theme(
            "Stage",
            &with_image,
            OverlayContentKind::Lyrics,
            1.0,
            "stage",
        );
        let o = assign_theme_to_output(&output("main", OverlayOutputKind::Screen), &theme);
        assert_eq!(
            o.lyrics.unwrap().theme.background.kind,
            OverlayBackgroundType::Image
        );
    }
}

mod pp_retirement {
    use super::*;
    use kairo_domain::overlay_outputs::OVERLAY_OUTPUT_KINDS;
    use kairo_domain::pp_retirement::retire_pro_presenter_settings;

    fn retire(v: Value) -> Value {
        Value::Object(retire_pro_presenter_settings(Some(&v)))
    }

    #[test]
    fn mixed_upgrade_removes_pp_destinations_and_bindings() {
        let mut screen = js(&OverlayOutput {
            enabled: true,
            display_id: Some(24.0),
            ..output("room", OverlayOutputKind::Screen)
        });
        screen["lookId"] = json!("look");
        let mut ndi = js(&OverlayOutput {
            enabled: true,
            ..output("stream", OverlayOutputKind::Ndi)
        });
        ndi["ppVideoInputUuid"] = json!("input");
        let settings = json!({
            "propresenter": {"enabled": true, "password": "secret"},
            "propresenterResources": {"lookId": "look"},
            "overlay": {"outputs": [screen.clone(), {"id": "old", "kind": "message", "enabled": true}, ndi], "ppVideoInputUuid": "old", "mode": "auto"},
        });
        let result = retire(settings.clone());
        assert!(result.get("propresenter").is_none());
        assert!(result.get("propresenterResources").is_none());
        let outs = result["overlay"]["outputs"].as_array().unwrap();
        assert_eq!(
            outs.iter()
                .map(|o| o["id"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["room", "stream"]
        );
        assert_eq!(outs[0]["displayId"], json!(24));
        assert!(outs[0].get("lookId").is_none());
        assert!(outs[1].get("ppVideoInputUuid").is_none());
        assert!(result["overlay"].get("mode").is_none());
        assert_eq!(
            settings["overlay"]["outputs"][0], screen,
            "the input is not changed"
        );
    }

    #[test]
    fn pp_only_upgrade_never_enables_an_arbitrary_output() {
        let result = retire(
            json!({"overlay": {"outputs": [{"id": "old", "kind": "library", "enabled": true}]}}),
        );
        let outputs = normalize_overlay_settings(result.get("overlay")).outputs;
        assert!(!outputs.iter().any(|o| o.id == "old"));
        assert_eq!(
            outputs
                .iter()
                .map(|o| (o.kind, o.name.as_str()))
                .collect::<Vec<_>>(),
            [(OverlayOutputKind::Screen, "Main Screen")]
        );
    }

    #[test]
    fn repeated_migration_removes_notice_state_and_keeps_content() {
        let result = retire(
            json!({"retirementNotice": false, "songs": [{"source": "propresenter", "text": "Saved song"}], "overlay": {"outputs": []}}),
        );
        assert!(result.get("retirementNotice").is_none());
        assert_eq!(retire(result.clone()), result);
        assert_eq!(
            result["songs"],
            json!([{"source": "propresenter", "text": "Saved song"}])
        );
    }

    #[test]
    fn normalization_drops_retired_and_unknown_output_kinds() {
        let result = settings_of(json!({"outputs": [
            {"id": "one", "kind": "message", "enabled": true},
            {"id": "two", "kind": "unknown", "enabled": true},
            js(&output("ndi", OverlayOutputKind::Ndi)),
        ]}));
        assert_eq!(
            result.outputs.iter().map(|o| o.kind).collect::<Vec<_>>(),
            [OverlayOutputKind::Ndi]
        );
    }

    #[test]
    fn legacy_message_only_stores_do_not_become_an_enabled_ndi_feed() {
        assert!(
            !settings_of(json!({"mode": "message"}))
                .outputs
                .iter()
                .any(|o| o.enabled)
        );
    }

    #[test]
    fn every_kind_of_screen_survives_the_launch_time_sanitize() {
        let outputs: Vec<OverlayOutput> = OVERLAY_OUTPUT_KINDS
            .iter()
            .map(|kind| OverlayOutput {
                name: format!("{} screen", kind.as_str()),
                enabled: true,
                ..output(&format!("{}-screen", kind.as_str()), *kind)
            })
            .collect();
        let stored = settings_of(json!({"outputs": js(&outputs)}));
        let once = retire(json!({"overlay": js(&stored)}));
        let twice = retire(once.clone());
        for sanitized in [once, twice] {
            assert_eq!(
                ids(&normalize_overlay_settings(sanitized.get("overlay")).outputs),
                ids(&outputs)
            );
        }
    }

    #[test]
    fn a_saved_syphon_screen_is_not_deleted_on_launch() {
        let syphon = OverlayOutput {
            name: "Lower Third".into(),
            enabled: true,
            ..output("lower-third", OverlayOutputKind::Syphon)
        };
        let screen = OverlayOutput {
            name: "Main Screen".into(),
            enabled: true,
            ..output("projector", OverlayOutputKind::Screen)
        };
        let result = retire(
            json!({"overlay": js(&settings_of(json!({"outputs": [js(&screen), js(&syphon)]})))}),
        );
        let got: Vec<String> = normalize_overlay_settings(result.get("overlay"))
            .outputs
            .iter()
            .map(|o| format!("{}:{}", o.id, o.kind.as_str()))
            .collect();
        assert_eq!(got, ["projector:screen", "lower-third:syphon"]);
    }
}

mod displays {
    use super::*;
    use kairo_domain::displays::*;

    fn display(id: f64, label: &str, width: f64, height: f64) -> DisplayInfo {
        DisplayInfo {
            id,
            label: label.into(),
            bounds: DisplayBounds {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            size: Size {
                width,
                height,
                extra: Extra::new(),
            },
            scale_factor: 1.0,
            primary: false,
            hosts_main_window: false,
        }
    }

    fn laptop() -> DisplayInfo {
        DisplayInfo {
            primary: true,
            hosts_main_window: true,
            ..display(1.0, "Built-in Retina Display", 1512.0, 982.0)
        }
    }

    fn projector() -> DisplayInfo {
        display(2.0, "EPSON PJ", 1920.0, 1080.0)
    }

    #[test]
    fn a_bound_display_is_found_by_id() {
        let all = [laptop(), projector()];
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &all, &[]),
            Some(&all[1])
        );
    }

    #[test]
    fn a_replugged_display_with_a_new_id_is_re_matched() {
        let all = [laptop(), display(99.0, "EPSON PJ", 1920.0, 1080.0)];
        let found = resolve_display(&binding_for_display(&projector()), &all, &[]);
        assert_eq!(found, Some(&all[1]));
        assert_ne!(found.unwrap().id, projector().id);
    }

    #[test]
    fn two_identical_displays_are_ambiguous() {
        let all = [
            laptop(),
            display(10.0, "EPSON PJ", 1920.0, 1080.0),
            display(11.0, "EPSON PJ", 1920.0, 1080.0),
        ];
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &all, &[]),
            None
        );
    }

    #[test]
    fn a_label_match_at_a_different_size_is_a_different_display() {
        let all = [laptop(), display(5.0, "EPSON PJ", 1280.0, 800.0)];
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &all, &[]),
            None
        );
    }

    #[test]
    fn an_unplugged_display_resolves_to_nothing() {
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &[laptop()], &[]),
            None
        );
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &[], &[]),
            None
        );
    }

    #[test]
    fn an_output_with_no_display_chosen_resolves_to_nothing() {
        let binding = DisplayBinding {
            display_id: None,
            display_label: String::new(),
            display_size: None,
        };
        assert_eq!(
            resolve_display(&binding, &[laptop(), projector()], &[]),
            None
        );
    }

    #[test]
    fn a_replugged_output_cannot_claim_a_display_already_assigned() {
        let all = [laptop(), display(99.0, "EPSON PJ", 1920.0, 1080.0)];
        assert_eq!(
            resolve_display(&binding_for_display(&projector()), &all, &[99.0]),
            None
        );
    }

    #[test]
    fn a_display_is_described_by_its_name_and_physical_resolution() {
        let l = laptop();
        assert_eq!(
            describe_display(&l.label, &l.size, 2.0),
            "Built-in Retina Display · 3024×1964"
        );
        let p = projector();
        assert_eq!(
            describe_display("  ", &p.size, p.scale_factor),
            "Display · 1920×1080"
        );
    }
}

mod output_health {
    use super::*;
    use kairo_domain::output_health::*;
    use kairo_domain::status::HealthStatus;

    fn result(kind: OverlayOutputKind, ok: bool, reason: Option<&str>) -> OverlayDispatchResult {
        OverlayDispatchResult {
            output_id: kind.as_str().into(),
            name: kind.as_str().into(),
            kind,
            ok,
            reason: reason.map(str::to_owned),
        }
    }

    #[test]
    fn successful_destinations_report_healthy_output() {
        let health = dispatch_health(&[
            result(OverlayOutputKind::Screen, true, None),
            result(OverlayOutputKind::Ndi, true, None),
            result(OverlayOutputKind::Device, true, None),
        ]);
        assert_eq!(js(&health), json!({"output": {"status": "ok"}}));
    }

    #[test]
    fn partial_failure_is_degraded_and_names_the_destination() {
        let health = dispatch_health(&[
            result(OverlayOutputKind::Screen, true, None),
            result(
                OverlayOutputKind::Device,
                false,
                Some("Device disconnected"),
            ),
        ]);
        assert_eq!(health.output.status, HealthStatus::Degraded);
        assert!(
            health
                .output
                .last_error
                .unwrap()
                .contains("device (Device disconnected)")
        );
    }

    #[test]
    fn all_destinations_failing_is_an_error() {
        let health = dispatch_health(&[
            result(
                OverlayOutputKind::Screen,
                false,
                Some("Display not connected"),
            ),
            result(OverlayOutputKind::Ndi, false, None),
        ]);
        assert_eq!(health.output.status, HealthStatus::Error);
        assert!(
            health
                .output
                .last_error
                .unwrap()
                .contains("screen (Display not connected); ndi (unknown)")
        );
    }

    #[test]
    fn an_empty_dispatch_reports_no_enabled_outputs() {
        assert_eq!(
            js(&dispatch_health(&[])),
            json!({"output": {"status": "error", "lastError": "No outputs are enabled"}})
        );
    }
}

mod output_test_pattern {
    use kairo_domain::output_test_pattern::output_test_pattern_html;
    use kairo_domain::video_devices::VideoTestPattern;

    #[test]
    fn escapes_output_names_and_keeps_one_removable_layer() {
        for pattern in VideoTestPattern::ALL {
            let html = output_test_pattern_html(*pattern, "<img src=x onerror=alert(1)>");
            assert!(html.contains(r#"id="kairo-diagnostic-pattern""#));
            assert!(html.contains("&lt;img"));
            assert!(
                !html.contains("<img")
                    && !html.contains("<script")
                    && !html.contains("http:")
                    && !html.contains("https:")
            );
        }
    }

    #[test]
    fn patterns_carry_their_content() {
        assert!(
            output_test_pattern_html(VideoTestPattern::Motion, "Main")
                .contains("3s linear infinite alternate")
        );
        assert!(output_test_pattern_html(VideoTestPattern::Text, "Main").contains("0123456789"));
        assert!(output_test_pattern_html(VideoTestPattern::Bars, "Main").contains("#ff0000"));
    }
}

mod prop_geometry {
    use kairo_domain::prop_geometry::*;

    const ORIGIN: PropBox = PropBox {
        x: 0.4,
        y: 0.4,
        w: 0.2,
        h: 0.2,
    };

    fn moving() -> PropGesture {
        PropGesture::Move {
            object_id: "o".into(),
            start_x: 0.0,
            start_y: 0.0,
            origin: ORIGIN,
        }
    }
    fn resize(corner: PropCorner) -> PropGesture {
        PropGesture::Resize {
            object_id: "o".into(),
            corner,
            start_x: 0.0,
            start_y: 0.0,
            origin: ORIGIN,
        }
    }
    fn near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-9, "{actual} ≈ {expected}");
    }

    #[test]
    fn moving_keeps_the_size_and_follows_the_pointer() {
        let b = apply_prop_gesture(&moving(), 0.1, -0.05);
        near(b.x, 0.5);
        near(b.y, 0.35);
        assert_eq!((b.w, b.h), (0.2, 0.2));
    }

    #[test]
    fn moving_stops_at_every_edge() {
        assert_eq!(
            apply_prop_gesture(&moving(), -5.0, -5.0),
            PropBox {
                x: 0.0,
                y: 0.0,
                ..ORIGIN
            }
        );
        let far = apply_prop_gesture(&moving(), 5.0, 5.0);
        near(far.x, 0.8);
        near(far.y, 0.8);
    }

    #[test]
    fn dragging_the_bottom_right_corner_grows_from_the_top_left() {
        let b = apply_prop_gesture(&resize(PropCorner::Se), 0.1, 0.05);
        assert_eq!((b.x, b.y), (0.4, 0.4));
        near(b.w, 0.3);
        near(b.h, 0.25);
    }

    #[test]
    fn dragging_the_top_left_corner_keeps_the_bottom_right_still() {
        let b = apply_prop_gesture(&resize(PropCorner::Nw), -0.1, -0.1);
        near(b.x, 0.3);
        near(b.y, 0.3);
        near(b.x + b.w, 0.6);
        near(b.y + b.h, 0.6);
    }

    #[test]
    fn a_resize_never_turns_the_box_inside_out() {
        let shrunk = apply_prop_gesture(&resize(PropCorner::Se), -1.0, -1.0);
        near(shrunk.w, MIN_PROP_OBJECT_SIZE);
        near(shrunk.h, MIN_PROP_OBJECT_SIZE);
        let pushed = apply_prop_gesture(&resize(PropCorner::Nw), 1.0, 1.0);
        near(pushed.w, MIN_PROP_OBJECT_SIZE);
        near(pushed.x + pushed.w, 0.6);
    }

    #[test]
    fn a_resize_stops_at_the_frame_edge() {
        let b = apply_prop_gesture(&resize(PropCorner::Se), 5.0, 5.0);
        near(b.x + b.w, 1.0);
        near(b.y + b.h, 1.0);
        let other = apply_prop_gesture(&resize(PropCorner::Nw), -5.0, -5.0);
        assert_eq!((other.x, other.y), (0.0, 0.0));
    }
}

mod screen_settings {
    use super::*;
    use kairo_domain::output_settings::*;
    use kairo_domain::theme_library::*;

    /// The store file the TypeScript test writes: overlay `outputs` and
    /// `theme` replaced by each persisted patch.
    fn persist(file: &mut Value, write: &PendingWrite) {
        let (outputs, theme) = write.patch();
        file["outputs"] = js(&outputs);
        file["theme"] = js(theme);
    }

    #[test]
    fn a_later_theme_save_keeps_a_new_video_output_across_reload() {
        let theme = create_custom_theme(
            "Sunday",
            &DEFAULT_OVERLAY_SETTINGS.theme,
            OverlayContentKind::Scripture,
            1.0,
            "sunday",
        );
        let mut raw = js(&*DEFAULT_OVERLAY_SETTINGS);
        raw["outputs"] = json!([js(&assign_theme_to_output(
            &output("screen", OverlayOutputKind::Screen),
            &theme
        ))]);
        let mut overlay = settings_of(raw);
        let mut file = js(&overlay);
        let mut writer = OutputSettingsWriter::new();
        let mut write =
            |overlay: &mut kairo_domain::settings::OverlaySettings,
             file: &mut Value,
             edit: &dyn Fn(Vec<OverlayOutput>) -> Vec<OverlayOutput>| {
                let pending = writer.begin(overlay.clone(), edit);
                *overlay = settings_of(js(&pending.next));
                persist(file, &pending);
                assert_eq!(writer.settle(&pending, true), None);
            };
        let card = OverlayOutput {
            name: "Video hub".into(),
            enabled: true,
            device_port_id: "decklink:123:3".into(),
            device_mode_id: "decklink:1080".into(),
            device_port_label: "Duo 2 (4)".into(),
            ..output("card", OverlayOutputKind::Device)
        };
        write(&mut overlay, &mut file, &|mut outputs| {
            outputs.push(card.clone());
            outputs
        });
        let mut edited = theme.clone();
        edited.theme.verse.color = "#123456".into();
        write(&mut overlay, &mut file, &|outputs| {
            sync_theme_to_outputs(&outputs, &edited)
        });
        let restored = settings_of(file.clone());
        assert_eq!(restored.outputs.len(), 2);
        let card_out = restored.outputs.iter().find(|o| o.id == "card").unwrap();
        assert_eq!(card_out.device_port_id, "decklink:123:3");
        assert_eq!(card_out.device_mode_id, "decklink:1080");
        assert_eq!(card_out.name, "Video hub");
        assert_eq!(
            restored
                .outputs
                .iter()
                .find(|o| o.id == "screen")
                .unwrap()
                .theme
                .verse
                .color,
            "#123456"
        );
        write(&mut overlay, &mut file, &|outputs| {
            outputs.into_iter().filter(|o| o.id != "card").collect()
        });
        write(&mut overlay, &mut file, &|outputs| {
            sync_theme_to_outputs(&outputs, &theme)
        });
        assert!(!settings_of(file).outputs.iter().any(|o| o.id == "card"));
    }

    #[test]
    fn an_emptied_output_list_comes_back_as_the_main_screen() {
        let mut saved = js(&*DEFAULT_OVERLAY_SETTINGS);
        saved["outputs"] = json!([]);
        let restored = settings_of(saved.clone()).outputs;
        assert_eq!(
            restored.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(),
            ["Main Screen"]
        );
        saved["outputs"] = js(&restored);
        assert_eq!(ids(&settings_of(saved.clone()).outputs), ids(&restored));
        saved["outputs"] = json!([js(&output("feed", OverlayOutputKind::Ndi))]);
        assert_eq!(ids(&settings_of(saved).outputs), ["feed"]);
    }

    #[test]
    fn a_failed_screen_save_rolls_back_its_optimistic_list() {
        let mut overlay = normalize_overlay_settings(Some(&js(&*DEFAULT_OVERLAY_SETTINGS)));
        let before = overlay.outputs.clone();
        let mut writer = OutputSettingsWriter::new();
        let pending = writer.begin(overlay.clone(), |mut outputs| {
            outputs.push(output("card", OverlayOutputKind::Device));
            outputs
        });
        overlay = pending.next.clone();
        let rollback = writer
            .settle(&pending, false)
            .expect("a failed newest write rolls back");
        overlay.outputs = rollback.outputs;
        overlay.theme = rollback.theme;
        assert_eq!(overlay.outputs, before);
    }

    #[test]
    fn two_overlapping_failed_saves_restore_the_last_confirmed_list() {
        let mut raw = js(&*DEFAULT_OVERLAY_SETTINGS);
        raw["outputs"] = json!([js(&output("a", OverlayOutputKind::Screen))]);
        let mut overlay = settings_of(raw);
        let mut writer = OutputSettingsWriter::new();
        let first = writer.begin(overlay.clone(), |mut outputs| {
            outputs.push(output("b", OverlayOutputKind::Device));
            outputs
        });
        overlay = first.next.clone();
        let second = writer.begin(overlay.clone(), |mut outputs| {
            outputs.push(output("c", OverlayOutputKind::Device));
            outputs
        });
        overlay = second.next.clone();
        assert_eq!(
            writer.settle(&first, false),
            None,
            "the batch has not settled yet"
        );
        let rollback = writer
            .settle(&second, false)
            .expect("rolls back once all settle");
        overlay.outputs = rollback.outputs;
        assert_eq!(ids(&overlay.outputs), ["a"]);
    }
}
