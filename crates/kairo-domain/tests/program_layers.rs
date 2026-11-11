//! `programLayersFor` as data, against `tests/engage-program.test.ts` and
//! the emptiness rules of each `render*HTML`.

use kairo_domain::program::{
    OutputShowFilter, PresentationSettings, ProgramLayer, ProgramState, normalize_show_filter,
    program_layers_for,
};
use serde_json::json;

fn never(_: &str, now: f64) -> f64 {
    now
}

fn show(raw: serde_json::Value) -> OutputShowFilter {
    normalize_show_filter(Some(&raw))
}

fn poll_state() -> ProgramState {
    ProgramState {
        poll: Some(
            serde_json::from_value(json!({
                "id": "p", "title": "<script>", "options": [{"id": "a", "text": "Yes & no"}, {"id": "b", "text": "B"}],
                "multiple": false, "state": "closed", "showResults": true,
                "totals": {"a": 3, "b": 1}, "voters": 4, "voted": []
            }))
            .unwrap(),
        ),
        ..ProgramState::default()
    }
}

#[test]
fn the_poll_follows_its_own_look_toggle() {
    let presentation = PresentationSettings::default();
    let state = poll_state();
    let visible = program_layers_for(
        &show(json!({"poll": true, "question": false})),
        &state,
        &presentation,
        0.0,
        never,
    );
    assert!(matches!(visible.poll, Some(ProgramLayer::Poll { .. })));
    assert!(visible.question.is_none());
    let hidden = program_layers_for(
        &show(json!({"poll": false})),
        &state,
        &presentation,
        0.0,
        never,
    );
    assert!(hidden.poll.is_none());
}

#[test]
fn a_blank_message_clears_the_layer_and_a_message_is_trimmed() {
    let presentation = PresentationSettings::default();
    let mut state = ProgramState {
        message: Some("  \n ".into()),
        ..ProgramState::default()
    };
    let all = show(json!({}));
    assert!(
        program_layers_for(&all, &state, &presentation, 0.0, never)
            .message
            .is_none()
    );
    state.message = Some("  Cars in the red zone \n".into());
    match program_layers_for(&all, &state, &presentation, 0.0, never).message {
        Some(ProgramLayer::Message { text, .. }) => assert_eq!(text, "Cars in the red zone"),
        other => panic!("{other:?}"),
    }
    let no_messages = show(json!({"messages": false}));
    assert!(
        program_layers_for(&no_messages, &state, &presentation, 0.0, never)
            .message
            .is_none()
    );
}

#[test]
fn props_need_to_be_active_and_draw_something() {
    let presentation = PresentationSettings {
        props: serde_json::from_value(json!([
            {"id": "empty", "name": "Empty", "background": null, "objects": []},
            {"id": "card", "name": "Card", "background": "#000000", "objects": []},
            {"id": "off", "name": "Off", "background": "#ff0000", "objects": []}
        ]))
        .unwrap(),
        ..PresentationSettings::default()
    };
    let mut state = ProgramState {
        active_prop_ids: vec!["empty".into(), "card".into()],
        ..ProgramState::default()
    };
    let all = show(json!({}));
    match program_layers_for(&all, &state, &presentation, 0.0, never).props {
        Some(ProgramLayer::Props { props, .. }) => {
            assert_eq!(
                props.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
                ["card"]
            );
        }
        other => panic!("{other:?}"),
    }
    state.active_prop_ids = vec!["empty".into()];
    assert!(
        program_layers_for(&all, &state, &presentation, 0.0, never)
            .props
            .is_none()
    );
    state.active_prop_ids = vec!["card".into()];
    let no_overlays = show(json!({"overlays": false}));
    let layers = program_layers_for(&no_overlays, &state, &presentation, 0.0, never);
    assert!(layers.props.is_none() && layers.logo.is_none());
}

#[test]
fn the_timer_needs_the_operator_and_the_look_and_uses_the_screen_timer() {
    let presentation = PresentationSettings::default();
    let mut state = ProgramState::default();
    let countdown = show(json!({"countdown": true}));
    assert!(
        program_layers_for(&countdown, &state, &presentation, 0.0, never)
            .timer
            .is_none()
    );
    state.countdown_on_screens = true;
    assert!(
        program_layers_for(&show(json!({})), &state, &presentation, 0.0, never)
            .timer
            .is_none()
    );
    match program_layers_for(&countdown, &state, &presentation, 5.0, never).timer {
        Some(ProgramLayer::Timer { prop, timer }) => {
            assert_eq!(prop.id, "timer-timer-main");
            assert_eq!(prop.objects.len(), 1);
            assert_eq!(timer.now, 5.0);
            assert_eq!(timer.duration_sec, 300.0);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_logo_shows_only_while_on_and_round_trips_as_json() {
    let presentation = PresentationSettings::default();
    let mut state = ProgramState::default();
    let all = show(json!({}));
    assert!(
        program_layers_for(&all, &state, &presentation, 0.0, never)
            .logo
            .is_none()
    );
    state.logo = true;
    let logo = program_layers_for(&all, &state, &presentation, 0.0, never)
        .logo
        .unwrap();
    let text = serde_json::to_string(&logo).unwrap();
    assert!(text.contains("\"kind\":\"logo\""));
    let back: ProgramLayer = serde_json::from_str(&text).unwrap();
    assert_eq!(back, logo);
}
