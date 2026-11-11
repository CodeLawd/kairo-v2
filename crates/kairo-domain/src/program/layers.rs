//! What each program layer shows, as data: `programLayersFor` without the
//! markup. The engine draws these (prompt 054); nothing here is HTML, so
//! nothing the operator types can become a style.

use super::{
    OutputShowFilter, PresentationSettings, ProgramLogo, ProgramMessageStyle, ProgramProp,
    ProgramQuestion, ProgramQuestionStyle, ProgramSlideInfo, ProgramState, ProgramTimerState,
    ProgramTimerStyle, PropTimerView, prop_timer_view_for, screen_timer_def,
};
use crate::audience::PollView;
use serde::{Deserialize, Serialize};

/// One program layer's content. `None` in [`ProgramLayers`] is the empty
/// string the HTML generators returned: the layer is cleared.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ProgramLayer {
    /// `renderMessageHTML`: the text is already trimmed and non-empty.
    Message {
        text: String,
        style: ProgramMessageStyle,
    },
    /// `renderPropsHTML`: the active props that draw anything, in list order.
    Props {
        props: Vec<ProgramProp>,
        timer: PropTimerView,
    },
    /// `renderTimerHTML`: the timer's design as one prop.
    Timer {
        prop: ProgramProp,
        timer: PropTimerView,
    },
    /// `renderPollHTML`.
    Poll {
        poll: PollView,
        style: ProgramQuestionStyle,
    },
    /// `renderQuestionHTML`.
    Question {
        question: ProgramQuestion,
        style: ProgramQuestionStyle,
    },
    /// `renderLogoHTML` while the logo is on.
    Logo { logo: ProgramLogo },
}

/// `programLayersFor`'s six layers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProgramLayers {
    pub poll: Option<ProgramLayer>,
    pub question: Option<ProgramLayer>,
    pub props: Option<ProgramLayer>,
    pub timer: Option<ProgramLayer>,
    pub message: Option<ProgramLayer>,
    pub logo: Option<ProgramLayer>,
}

/// `renderMessageHTML`'s emptiness rule: blank text clears the layer.
pub fn message_layer(text: Option<&str>, style: &ProgramMessageStyle) -> Option<ProgramLayer> {
    let body = crate::js::trim(text.unwrap_or(""));
    (!body.is_empty()).then(|| ProgramLayer::Message {
        text: body.to_owned(),
        style: style.clone(),
    })
}

/// `renderPropsHTML`: active props with a background or an object.
pub fn props_layer(
    props: &[ProgramProp],
    active_ids: &[String],
    timer: &PropTimerView,
) -> Option<ProgramLayer> {
    let active: Vec<ProgramProp> = props
        .iter()
        .filter(|p| active_ids.contains(&p.id) && (p.background.is_some() || !p.objects.is_empty()))
        .cloned()
        .collect();
    (!active.is_empty()).then(|| ProgramLayer::Props {
        props: active,
        timer: timer.clone(),
    })
}

/// `renderLogoHTML`: nothing while off.
pub fn logo_layer(logo: &ProgramLogo, on: bool) -> Option<ProgramLayer> {
    on.then(|| ProgramLayer::Logo { logo: logo.clone() })
}

/// `programLayersFor`: what one output shows, given its Look.
pub fn program_layers_for(
    show: &OutputShowFilter,
    state: &ProgramState,
    presentation: &PresentationSettings,
    now_ms: f64,
    next_time_of_day: impl FnOnce(&str, f64) -> f64,
) -> ProgramLayers {
    let timer = prop_timer_view_for(state, presentation, now_ms, next_time_of_day);
    let def = screen_timer_def(&presentation.timers, state.screen_timer_id.as_deref());
    ProgramLayers {
        poll: show
            .poll
            .then(|| state.poll.clone())
            .flatten()
            .map(|poll| ProgramLayer::Poll {
                poll,
                style: presentation.question.clone(),
            }),
        question: show
            .question
            .then(|| state.question.clone())
            .flatten()
            .map(|question| ProgramLayer::Question {
                question,
                style: presentation.question.clone(),
            }),
        props: if show.overlays {
            props_layer(&presentation.props, &state.active_prop_ids, &timer)
        } else {
            None
        },
        timer: (show.countdown && state.countdown_on_screens).then(|| ProgramLayer::Timer {
            prop: ProgramProp {
                id: format!("timer-{}", def.id),
                name: def.name.clone(),
                background: def.theme.background.clone(),
                objects: def.theme.objects.clone(),
                extra: Default::default(),
            },
            timer: timer.clone(),
        }),
        message: if show.messages {
            message_layer(state.message.as_deref(), &presentation.message)
        } else {
            None
        },
        logo: if show.overlays {
            logo_layer(&presentation.logo, state.logo)
        } else {
            None
        },
    }
}

/// `StageView`, what `stage.html`'s `__setStage` takes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StageView {
    #[serde(deserialize_with = "crate::serde_util::required")]
    pub current: Option<ProgramSlideInfo>,
    #[serde(deserialize_with = "crate::serde_util::required")]
    pub next: Option<ProgramSlideInfo>,
    #[serde(deserialize_with = "crate::serde_util::required")]
    pub stage_message: Option<String>,
    pub timer: ProgramTimerState,
    pub show_next: bool,
    pub show_clock: bool,
    pub show_timer: bool,
    pub timer_style: ProgramTimerStyle,
}
