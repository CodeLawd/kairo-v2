use crate::overlay::OverlayOutputKind;
use crate::settings::{
    AppSettings, OnboardingReadiness, OnboardingSource, OnboardingState, OnboardingStepId,
    OnboardingSummaryLine,
};
use OnboardingStepId::{Account, ApiKeys, Church, Output, Welcome};
use serde_json::Value;

pub const ONBOARDING_STEPS: &[OnboardingStepId] = &[Welcome, Account, Output];

pub fn onboarding_steps() -> &'static [OnboardingStepId] {
    ONBOARDING_STEPS
}

pub fn normalize_onboarding_state(value: &Value) -> OnboardingState {
    let steps = |key| {
        value[key].as_array().map_or_else(Vec::new, |list| {
            ONBOARDING_STEPS
                .iter()
                .copied()
                .filter(|step| list.iter().any(|v| v.as_str() == Some(step.as_str())))
                .collect()
        })
    };
    let current = value["currentStep"]
        .as_str()
        .and_then(OnboardingStepId::from_name)
        .filter(|step| ONBOARDING_STEPS.contains(step))
        .unwrap_or(Welcome);
    OnboardingState {
        completed_steps: steps("completedSteps"),
        skipped_steps: steps("skippedSteps"),
        current_step: current,
        completed_at: value["completedAt"].as_f64(),
        source: crate::normalize::safe_enum(value.get("source"), OnboardingSource::Fresh),
        extra: Default::default(),
    }
}

pub fn onboarding_step_label(step: OnboardingStepId) -> &'static str {
    match step {
        Welcome => "Welcome",
        Account => "Account",
        Output => "Output",
        ApiKeys => "API keys",
        Church => "Church profile",
    }
}

fn api_keys(settings: &AppSettings) -> (bool, bool) {
    if let Some(configured) = settings
        .extra
        .get("secretsConfigured")
        .filter(|v| crate::js::truthy(Some(v)))
    {
        return (
            crate::js::truthy(configured.get("bible")),
            crate::js::truthy(configured.get("deepgram")),
        );
    }
    (
        !crate::js::trim(&settings.stt.bible_api_key).is_empty(),
        !crate::js::trim(&settings.stt.api_key).is_empty(),
    )
}

pub fn is_step_complete(step: OnboardingStepId, settings: &AppSettings) -> bool {
    match step {
        Output => settings.overlay.outputs.iter().any(|output| {
            output.enabled
                && output.kind == OverlayOutputKind::Screen
                && output.display_id.is_some()
        }),
        ApiKeys => {
            let (bible, deepgram) = api_keys(settings);
            bible || deepgram
        }
        Church => !crate::js::trim(&settings.church.name).is_empty(),
        Welcome | Account => false,
    }
}

pub fn derive_completed_steps(settings: &AppSettings) -> Vec<OnboardingStepId> {
    ONBOARDING_STEPS
        .iter()
        .copied()
        .filter(|step| is_step_complete(*step, settings))
        .collect()
}

pub fn remaining_steps(state: &OnboardingState) -> impl Iterator<Item = OnboardingStepId> + '_ {
    ONBOARDING_STEPS
        .iter()
        .copied()
        .filter(|step| !state.completed_steps.contains(step) && !state.skipped_steps.contains(step))
}

pub fn next_incomplete_step(state: &OnboardingState) -> Option<OnboardingStepId> {
    remaining_steps(state).next()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnboardingProgress {
    pub resolved: usize,
    pub total: usize,
    pub ratio: f64,
}

pub fn onboarding_progress(state: &OnboardingState) -> OnboardingProgress {
    let total = ONBOARDING_STEPS.len();
    let resolved = total - remaining_steps(state).count();
    OnboardingProgress {
        resolved,
        total,
        ratio: resolved as f64 / total as f64,
    }
}

pub fn is_onboarding_finished(state: &OnboardingState) -> bool {
    state.completed_at.is_some()
}

pub fn complete_step(mut state: OnboardingState, step: OnboardingStepId) -> OnboardingState {
    if !state.completed_steps.contains(&step) {
        state.completed_steps.push(step);
    }
    state.skipped_steps.retain(|id| *id != step);
    state
}

pub fn skip_step(mut state: OnboardingState, step: OnboardingStepId) -> OnboardingState {
    if !state.completed_steps.contains(&step) && !state.skipped_steps.contains(&step) {
        state.skipped_steps.push(step);
    }
    state
}

pub fn set_current_step(mut state: OnboardingState, step: OnboardingStepId) -> OnboardingState {
    state.current_step = step;
    state
}

pub fn step_index(step: OnboardingStepId) -> usize {
    ONBOARDING_STEPS
        .iter()
        .position(|candidate| *candidate == step)
        .unwrap_or_default()
}

pub fn next_step(state: &OnboardingState) -> Option<OnboardingStepId> {
    ONBOARDING_STEPS
        .get(step_index(state.current_step) + 1)
        .copied()
}

pub fn previous_step(state: &OnboardingState) -> Option<OnboardingStepId> {
    step_index(state.current_step)
        .checked_sub(1)
        .map(|index| ONBOARDING_STEPS[index])
}

pub fn finish_onboarding(mut state: OnboardingState, now_ms: f64) -> OnboardingState {
    state.completed_at.get_or_insert(now_ms);
    state
}

pub fn reset_onboarding(state: &OnboardingState) -> OnboardingState {
    OnboardingState {
        source: state.source,
        ..OnboardingState::default()
    }
}

pub fn seed_onboarding_from_settings(settings: &AppSettings, now_ms: f64) -> OnboardingState {
    let completed_steps = derive_completed_steps(settings);
    if completed_steps.is_empty()
        && !is_step_complete(ApiKeys, settings)
        && !is_step_complete(Church, settings)
    {
        return OnboardingState::default();
    }
    let state = OnboardingState {
        source: OnboardingSource::Legacy,
        completed_steps,
        skipped_steps: vec![Welcome, Account],
        ..OnboardingState::default()
    };
    match next_incomplete_step(&state) {
        Some(step) => set_current_step(state, step),
        None => finish_onboarding(state, now_ms),
    }
}

pub fn should_offer_onboarding(state: &OnboardingState, dismissed: bool) -> bool {
    !dismissed && !is_onboarding_finished(state)
}

pub fn onboarding_summary(settings: &AppSettings) -> Vec<OnboardingSummaryLine> {
    let outputs = settings
        .overlay
        .outputs
        .iter()
        .filter(|output| output.enabled)
        .count();
    let (bible, deepgram) = api_keys(settings);
    let keys = match (bible, deepgram) {
        (true, true) => "API.Bible, Deepgram",
        (true, false) => "API.Bible",
        (false, true) => "Deepgram",
        (false, false) => "None — offline Bibles only",
    };
    let church = crate::js::trim(&settings.church.name);
    vec![
        OnboardingSummaryLine {
            step: Output,
            label: "Outputs".into(),
            done: outputs > 0,
            detail: if outputs > 0 {
                format!("{outputs} turned on")
            } else {
                "None turned on".into()
            },
        },
        OnboardingSummaryLine {
            step: ApiKeys,
            label: "API keys".into(),
            done: bible || deepgram,
            detail: keys.into(),
        },
        OnboardingSummaryLine {
            step: Church,
            label: "Church".into(),
            done: !church.is_empty(),
            detail: if church.is_empty() {
                "Not named".into()
            } else {
                church.into()
            },
        },
    ]
}

pub fn onboarding_readiness(settings: &AppSettings) -> OnboardingReadiness {
    let lines = onboarding_summary(settings);
    let outstanding: Vec<_> = lines.iter().filter(|line| !line.done).cloned().collect();
    let ready = outstanding
        .iter()
        .all(|line| matches!(line.step, ApiKeys | Church));
    OnboardingReadiness {
        lines,
        outstanding,
        ready,
    }
}
