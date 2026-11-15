use kairo_domain::overlay::*;
use kairo_domain::overlay_defaults::make_overlay_output;
use kairo_domain::settings::AppSettings;
use serde_json::json;
use ui_screens::model::{Edit, apply, initial_selection, look_applies};

#[test]
fn edits_use_current_outputs_and_delete_the_context_target() {
    let mut s = AppSettings::defaults(kairo_domain::overlay_defaults::normalize_overlay_settings(
        None,
    ));
    s.overlay.outputs = vec![
        make_overlay_output("a", OverlayOutputKind::Screen),
        make_overlay_output("b", OverlayOutputKind::Screen),
    ];
    apply(
        &mut s,
        &Edit::AddOutput {
            id: "card".into(),
            kind: OverlayOutputKind::Device,
        },
    )
    .unwrap();
    apply(
        &mut s,
        &Edit::Output {
            id: "card".into(),
            patch: json!({"devicePortId":"decklink:123:3","deviceModeId":"decklink:25"}),
        },
    )
    .unwrap();
    // A deferred theme edit only patches its target against this current list.
    apply(
        &mut s,
        &Edit::Output {
            id: "a".into(),
            patch: json!({"name":"Sunday edited"}),
        },
    )
    .unwrap();
    let saved: AppSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert_eq!(saved.overlay.outputs[2].device_mode_id, "decklink:25");
    apply(&mut s, &Edit::Remove("b".into())).unwrap();
    assert!(s.overlay.outputs.iter().any(|o| o.id == "a"));
    assert!(!s.overlay.outputs.iter().any(|o| o.id == "b"));
}

#[test]
fn last_audience_and_missing_targets_are_rejected() {
    let mut s = AppSettings::defaults(kairo_domain::overlay_defaults::normalize_overlay_settings(
        None,
    ));
    s.overlay.outputs = vec![make_overlay_output("a", OverlayOutputKind::Screen)];
    assert!(
        apply(&mut s, &Edit::Remove("a".into()))
            .unwrap_err()
            .contains("at least one")
    );
    assert!(
        apply(
            &mut s,
            &Edit::Output {
                id: "gone".into(),
                patch: json!({"name":"lost"})
            }
        )
        .is_err()
    );
}

#[test]
fn remembered_show_panels_and_lobby_layers() {
    let mut s = AppSettings::defaults(kairo_domain::overlay_defaults::normalize_overlay_settings(
        None,
    ));
    s.overlay.outputs.clear();
    for key in ["show:setup", "show:looks", "show:transition", "show:check"] {
        assert_eq!(initial_selection(&s, None, Some(key)).as_deref(), Some(key));
    }
    let mut output = make_overlay_output("lobby", OverlayOutputKind::Screen);
    output.source = OutputSource::Playlist;
    assert!(!look_applies(&output, "scripture"));
    assert!(look_applies(&output, "clock"));
}

#[test]
fn card_conversion_preserves_content_looks_and_themes() {
    let mut s =
        AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
    let mut output = make_overlay_output("a", OverlayOutputKind::Screen);
    output.name = "Lobby".into();
    output.source = OutputSource::Playlist;
    output.playlist_id = "welcome".into();
    output.show.clock = true;
    s.overlay.outputs = vec![output.clone()];
    let port = kairo_domain::video_devices::VideoPort {
        id: "card:1".into(),
        provider: "decklink".into(),
        device_name: "Duo".into(),
        port_name: "Duo (1)".into(),
        modes: vec![],
        supports_key_fill: false,
        unavailable_reason: None,
    };
    apply(
        &mut s,
        &Edit::MoveToCard {
            id: "a".into(),
            next_id: "card".into(),
            port,
        },
    )
    .unwrap();
    let card = &s.overlay.outputs[0];
    assert_eq!(card.name, output.name);
    assert_eq!(card.theme, output.theme);
    assert_eq!(card.show, output.show);
    assert_eq!(card.playlist_id, "welcome");
    assert_eq!(card.kind, OverlayOutputKind::Device);
    assert_eq!(card.id, "card");
    assert_eq!(card.display_id, None);
}

#[test]
fn independent_nested_edits_do_not_replace_other_fields() {
    let mut s =
        AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
    s.overlay.outputs = vec![make_overlay_output("a", OverlayOutputKind::Screen)];
    apply(&mut s,&Edit::Output{id:"a".into(),patch:json!({"show":{"clock":true},"customAspect":{"width":102,"height":0},"slideSec":900})}).unwrap();
    apply(
        &mut s,
        &Edit::Output {
            id: "a".into(),
            patch: json!({"show":{"countdown":true}}),
        },
    )
    .unwrap();
    let o = &s.overlay.outputs[0];
    assert!(o.show.clock && o.show.countdown && o.show.scripture);
    assert_eq!(o.slide_sec, 600.);
    assert_eq!(o.custom_aspect.width, 100.);
    assert_eq!(o.custom_aspect.height, 1.);
    let old_audio = s.presentation.audio.clone();
    apply(&mut s, &Edit::Presentation(json!({"audio":{"ndi":true}}))).unwrap();
    assert!(s.presentation.audio.ndi);
    assert_eq!(s.presentation.audio.output_label, old_audio.output_label);
}

#[test]
fn feed_limits_and_explicit_empty_outputs() {
    let mut s =
        AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
    s.overlay.outputs.clear();
    for ix in 0..kairo_domain::overlay_outputs::MAX_NDI_OUTPUTS {
        apply(
            &mut s,
            &Edit::AddOutput {
                id: format!("ndi-{ix}"),
                kind: OverlayOutputKind::Ndi,
            },
        )
        .unwrap();
    }
    assert!(
        apply(
            &mut s,
            &Edit::AddOutput {
                id: "extra".into(),
                kind: OverlayOutputKind::Ndi
            }
        )
        .is_err()
    );
    s.overlay.outputs.clear();
    let roundtrip: AppSettings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
    assert!(roundtrip.overlay.outputs.is_empty());
}

#[test]
fn binding_rejects_an_occupied_monitor_and_keeps_disconnected_bindings() {
    let mut s =
        AppSettings::defaults(kairo_domain::overlay_defaults::DEFAULT_OVERLAY_SETTINGS.clone());
    s.overlay.outputs = vec![
        make_overlay_output("a", OverlayOutputKind::Screen),
        make_overlay_output("b", OverlayOutputKind::Screen),
    ];
    s.overlay.outputs[0].enabled = true;
    s.overlay.outputs[1].enabled = true;
    let d = DisplayInfo {
        id: 7.,
        label: "Projector".into(),
        bounds: DisplayBounds {
            x: -1920.,
            y: 0.,
            width: 1920.,
            height: 1080.,
        },
        size: Size {
            width: 1920.,
            height: 1080.,
            extra: Default::default(),
        },
        scale_factor: 1.,
        primary: false,
        hosts_main_window: false,
    };
    apply(
        &mut s,
        &Edit::Bind {
            key: "a".into(),
            display: Some(d.clone()),
        },
    )
    .unwrap();
    assert!(
        apply(
            &mut s,
            &Edit::Bind {
                key: "b".into(),
                display: Some(d)
            }
        )
        .unwrap_err()
        .contains("already on that display")
    );
    assert_eq!(s.overlay.outputs[1].display_id, None);
    let b = ui_screens::model::binding(
        s.overlay.outputs[0].display_id,
        &s.overlay.outputs[0].display_label,
        &s.overlay.outputs[0].display_size,
    );
    assert!(kairo_domain::displays::resolve_display(&b, &[], &[]).is_none());
    assert_eq!(b.display_id, Some(7.));
}
