//! `ProgramSurface` semantics and state replay.

use kairo_engine_proto::*;
use serde_json::json;

fn display_spec() -> SurfaceSpec {
    SurfaceSpec {
        output: "screen".into(),
        label: "Screen".into(),
        sink: SinkSpec::Display(DisplaySink {
            binding: DisplayBinding {
                display_id: Some(1),
                label: "Display".into(),
                size: Some((1920, 1080)),
            },
            rehearsal: false,
            frame_height: 1080,
        }),
    }
}

fn ndi_spec() -> SurfaceSpec {
    SurfaceSpec {
        output: "ndi".into(),
        label: "NDI".into(),
        sink: SinkSpec::Ndi {
            name: "Kairo".into(),
        },
    }
}

fn slide(text: &str) -> SlideContent {
    SlideContent {
        reference: "John 3:16".into(),
        text: text.into(),
        theme: Json::from_value(&json!({
            "background": {"type": "video", "mediaPath": "/a.mp4", "blurPx": 4},
            "fontSize": 60
        })),
        colored_lines: None,
    }
}

fn show(output: &str, text: &str) -> Command {
    Command::ShowSlide {
        output: output.into(),
        source: output.into(),
        slide: slide(text),
        instant: false,
    }
}

fn background(state: &SurfaceState) -> serde_json::Value {
    state.last_render.as_ref().unwrap().theme.value().unwrap()["background"].clone()
}

#[test]
fn pushes_need_an_available_unparked_surface() {
    let mut state = SurfaceState::new(display_spec());
    assert!(!state.apply(&show("screen", "a"), false));
    assert!(state.last_render.is_none());
    assert!(state.apply(&show("screen", "a"), true));
    assert!(state.apply(
        &Command::CloseSurface {
            output: "screen".into(),
            destroy: false
        },
        true
    ));
    assert!(state.parked);
    assert!(!state.apply(&show("screen", "b"), true));
    // `close` keeps the last render, so a replugged display repaints it.
    assert_eq!(state.last_render.as_ref().unwrap().text, "a");
    state.apply(
        &Command::RestoreSurface {
            output: "screen".into(),
        },
        true,
    );
    assert!(state.apply(&show("screen", "b"), true));
}

#[test]
fn ndi_surfaces_do_not_park_on_close() {
    let mut state = SurfaceState::new(ndi_spec());
    state.apply(
        &Command::CloseSurface {
            output: "ndi".into(),
            destroy: false,
        },
        true,
    );
    assert!(!state.parked);
}

#[test]
fn background_swaps_need_something_painted() {
    let mut state = SurfaceState::new(display_spec());
    let next = Json::from_value(&json!({"type": "color", "color": "#123"}));
    let set = Command::SetBackground {
        output: "screen".into(),
        background: next.clone(),
    };
    assert!(!state.apply(&set, true));
    state.apply(&show("screen", "a"), true);
    assert!(state.apply(&set, true));
    assert_eq!(background(&state), next.value().unwrap());
    // The rest of the theme is kept.
    assert_eq!(
        state.last_render.as_ref().unwrap().theme.value().unwrap()["fontSize"],
        60
    );
}

#[test]
fn patch_background_changes_loop_and_colour_only() {
    let mut state = SurfaceState::new(display_spec());
    let patch = Command::PatchBackground {
        output: "screen".into(),
        playback: Json::from_value(
            &json!({"loop": false, "hue": 10, "saturation": 1.5, "brightness": 1, "contrast": 1}),
        ),
        filter: "hue-rotate(10deg)".into(),
    };
    assert!(!state.apply(&patch, true));
    state.apply(&show("screen", "a"), true);
    assert!(state.apply(&patch, true));
    assert_eq!(
        background(&state),
        json!({"type": "video", "mediaPath": "/a.mp4", "blurPx": 4, "mediaLoop": false, "hue": 10, "saturation": 1.5, "brightness": 1, "contrast": 1})
    );
}

#[test]
fn clear_text_keeps_the_background_unless_given_a_new_one() {
    let mut state = SurfaceState::new(display_spec());
    let clear = Command::ClearText {
        output: "screen".into(),
        background: None,
    };
    assert!(!state.apply(&clear, true));
    state.apply(&show("screen", "a"), true);
    assert!(state.apply(&clear, true));
    let render = state.last_render.clone().unwrap();
    assert_eq!((render.reference.as_str(), render.text.as_str()), ("", ""));
    assert_eq!(background(&state)["mediaPath"], "/a.mp4");

    let colour = json!({"type": "color", "color": "#fff"});
    state.apply(
        &Command::ClearText {
            output: "screen".into(),
            background: Some(Json::from_value(&colour)),
        },
        true,
    );
    assert_eq!(background(&state), colour);

    state.apply(
        &Command::Clear {
            output: "screen".into(),
        },
        true,
    );
    assert!(state.last_render.is_none() && state.last_output_id.is_none());
}

#[test]
fn page_swaps_only_continue_the_same_outputs_deck() {
    let mut state = SurfaceState::new(display_spec());
    let doc = |page: f64| {
        Json::from_value(
            &json!({"id": "deck", "page": page, "path": "/p.png", "width": 1, "height": 1, "videos": [{"file": "/v.mp4"}]}),
        )
    };
    let swap = |source: &str| Command::SwapPage {
        output: "screen".into(),
        source: source.into(),
        media_path: "/page-2.png".into(),
        document: Some(doc(2.0)),
    };
    state.apply(&show("screen", "a"), true);
    assert!(!state.apply(&swap("screen"), true), "not in document mode");
    assert!(state.apply(
        &Command::ShowDocument {
            output: "screen".into(),
            source: "screen".into(),
            slide: slide(""),
            document: Some(doc(1.0)),
        },
        true
    ));
    assert!(state.document_mode);
    assert!(!state.apply(&swap("other"), true));
    assert!(state.apply(&swap("screen"), true));
    assert_eq!(background(&state)["mediaPath"], "/page-2.png");

    let control = |page: f64, video: u32| Command::ControlDocumentVideo {
        output: "screen".into(),
        id: "deck".into(),
        page,
        video,
        command: Json::from_value(&json!({"action": "play"})),
    };
    assert!(state.apply(&control(2.0, 0), true));
    assert!(!state.apply(&control(1.0, 0), true));
    assert!(!state.apply(&control(2.0, 1), true));

    // Any full paint leaves document mode.
    state.apply(&show("screen", "b"), true);
    assert!(!state.document_mode && state.document_slide.is_none());
    assert!(!state.apply(&control(2.0, 0), true));
}

#[test]
fn web_pointer_and_scroll_need_a_web_layer() {
    let mut state = SurfaceState::new(display_spec());
    let pointer = Command::WebPointer {
        output: "screen".into(),
        point: None,
    };
    assert!(!state.apply(&pointer, true));
    state.apply(
        &Command::SetWeb {
            output: "screen".into(),
            web: Some(WebLayer {
                url: "https://example.org".into(),
                reload: 0.0,
            }),
        },
        true,
    );
    assert!(state.apply(&pointer, true));
}

/// Every reachable state comes back exactly from its replay.
#[test]
fn replay_rebuilds_the_same_state() {
    let mut states = Vec::new();
    let mut full = SurfaceState::new(display_spec());
    full.apply(
        &Command::Configure {
            output: "screen".into(),
            config: SurfaceConfig {
                transition_ms: 500.0,
                camera_audible: true,
                ..SurfaceConfig::default()
            },
        },
        true,
    );
    for (index, kind) in LayerKind::ALL.into_iter().enumerate() {
        full.apply(
            &Command::SetLayer {
                output: "screen".into(),
                kind,
                markup: format!("<div>{index}</div>"),
            },
            true,
        );
    }
    full.apply(
        &Command::SetInfo {
            output: "screen".into(),
            info: Some(Json("{\"clock\":true}".into())),
        },
        true,
    );
    full.apply(
        &Command::SetCamera {
            output: "screen".into(),
            camera: Some(CameraLayer {
                label: "FaceTime".into(),
                audio_label: None,
            }),
        },
        true,
    );
    full.apply(
        &Command::SetTestPattern {
            output: "screen".into(),
            pattern: Some(TestPattern::Bars),
        },
        true,
    );
    states.push(full.clone());
    full.apply(&show("screen", "verse"), true);
    states.push(full.clone());
    full.apply(
        &Command::ShowDocument {
            output: "screen".into(),
            source: "screen".into(),
            slide: slide(""),
            document: Some(Json("{\"id\":\"d\",\"page\":1,\"videos\":[]}".into())),
        },
        true,
    );
    states.push(full.clone());
    full.apply(
        &Command::CloseSurface {
            output: "screen".into(),
            destroy: false,
        },
        true,
    );
    states.push(full.clone());
    states.push(SurfaceState::new(ndi_spec()));

    for state in states {
        let mut rebuilt: Option<SurfaceState> = None;
        for command in state.replay() {
            match (&mut rebuilt, &command) {
                (None, Command::OpenSurface(spec)) => {
                    rebuilt = Some(SurfaceState::new(spec.clone()))
                }
                (Some(target), command) => {
                    assert!(target.apply(command, true), "{command:?}");
                }
                (None, other) => panic!("replay must open first, got {other:?}"),
            }
        }
        assert_eq!(rebuilt.unwrap(), state);
    }
}
