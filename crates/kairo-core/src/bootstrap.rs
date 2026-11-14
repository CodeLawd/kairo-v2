//! Concurrent, local-only startup resource loading.

use futures_util::{FutureExt as _, StreamExt as _, stream::FuturesUnordered};
use kairo_domain::{
    cloud::{CloudSessionState, SessionSnapshot},
    ipc_error::clean_ipc_error,
    lyrics::LyricsSong,
    scripture::{LivePlanState, ScriptureTranslationOption, SermonPlan},
    settings::{OnboardingSource, OnboardingState, OnboardingStepId, SettingsWithSecretsStatus},
    status::{
        AppBootstrapSnapshot, BootstrapProgress, BootstrapResource, BootstrapResourceError,
        OrchestratorStatus,
    },
    transcript::TranscriptResult,
};
use std::{future::Future, panic::AssertUnwindSafe, pin::Pin, time::Duration};

/// Maximum time startup waits for local resources.
pub const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(8);

/// An asynchronous resource read. Blocking reads belong on a dedicated worker.
pub type Loader<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send>>;

/// The nine local resources needed for the first paint.
/// Remote hydration is a separate operation.
pub struct BootstrapLoaders {
    pub settings: Loader<SettingsWithSecretsStatus>,
    pub orchestrator: Loader<OrchestratorStatus>,
    pub transcription: Loader<Vec<TranscriptResult>>,
    pub translations: Loader<Vec<ScriptureTranslationOption>>,
    pub sermon_plans: Loader<Vec<SermonPlan>>,
    pub live_plan: Loader<LivePlanState>,
    pub lyrics: Loader<Vec<LyricsSong>>,
    pub onboarding: Loader<OnboardingState>,
    pub account: Loader<SessionSnapshot>,
}

const STEPS: [(BootstrapResource, &str); 9] = [
    (BootstrapResource::Settings, "Getting everything ready…"),
    (BootstrapResource::Orchestrator, "Starting automation…"),
    (
        BootstrapResource::Transcription,
        "Loading transcript history…",
    ),
    (BootstrapResource::Translations, "Loading scriptures…"),
    (
        BootstrapResource::SermonPlans,
        "Importing scripture playlists…",
    ),
    (BootstrapResource::LivePlan, "Preparing live playlist…"),
    (BootstrapResource::Lyrics, "Getting lyrics ready…"),
    (BootstrapResource::Onboarding, "Checking setup…"),
    (BootstrapResource::Account, "Loading your account…"),
];

type SnapshotUpdate = Box<dyn FnOnce(&mut AppBootstrapSnapshot) + Send>;
type LoaderCompletion =
    Pin<Box<dyn Future<Output = (usize, Result<SnapshotUpdate, String>)> + Send>>;

/// `load` for `resource`, reporting its place in [`STEPS`].
fn completion<T: Send + 'static>(
    resource: BootstrapResource,
    load: Loader<T>,
    update: fn(&mut AppBootstrapSnapshot, T),
) -> LoaderCompletion {
    let index = STEPS
        .iter()
        .position(|(step, _)| *step == resource)
        .expect("every resource has a step");
    Box::pin(async move {
        let result = match AssertUnwindSafe(load).catch_unwind().await {
            Ok(Ok(value)) => {
                let update: SnapshotUpdate = Box::new(move |snapshot| update(snapshot, value));
                Ok(update)
            }
            Ok(Err(message)) => Err(clean_ipc_error(&message).to_owned()),
            Err(_) => Err("The startup resource stopped unexpectedly.".into()),
        };
        (index, result)
    })
}

/// Resolve local startup reads concurrently. Progress is scoped to the calling
/// view. Pending futures are dropped at the deadline before emitting Ready.
///
/// Run on a Tokio runtime with time enabled, for example through AppCore.
pub async fn run_bootstrap(
    loaders: BootstrapLoaders,
    on_progress: impl FnMut(BootstrapProgress),
) -> AppBootstrapSnapshot {
    load_until(loaders, BOOTSTRAP_TIMEOUT, on_progress).await
}

async fn load_until(
    loaders: BootstrapLoaders,
    timeout: Duration,
    mut on_progress: impl FnMut(BootstrapProgress),
) -> AppBootstrapSnapshot {
    let mut snapshot = neutral_snapshot();
    let mut pending = [true; STEPS.len()];
    let mut completed = 0;

    let mut emit = |completed: usize, step: &str| {
        on_progress(BootstrapProgress {
            completed: completed as f64,
            total: STEPS.len() as f64,
            step: step.into(),
        });
    };
    let timer = tokio::time::sleep(timeout);
    tokio::pin!(timer);
    emit(completed, STEPS[0].1);

    let mut tasks: FuturesUnordered<_> = [
        completion(BootstrapResource::Settings, loaders.settings, |s, value| {
            s.settings = Some(value)
        }),
        completion(
            BootstrapResource::Orchestrator,
            loaders.orchestrator,
            |s, value| s.orchestrator = Some(value),
        ),
        completion(
            BootstrapResource::Transcription,
            loaders.transcription,
            |s, value| s.transcription = value,
        ),
        completion(
            BootstrapResource::Translations,
            loaders.translations,
            |s, value| s.translations = value,
        ),
        completion(
            BootstrapResource::SermonPlans,
            loaders.sermon_plans,
            |s, value| s.sermon_plans = value,
        ),
        completion(
            BootstrapResource::LivePlan,
            loaders.live_plan,
            |s, value| s.live_plan = Some(value),
        ),
        completion(BootstrapResource::Lyrics, loaders.lyrics, |s, value| {
            s.lyrics = value
        }),
        completion(
            BootstrapResource::Onboarding,
            loaders.onboarding,
            |s, value| s.onboarding = value,
        ),
        completion(BootstrapResource::Account, loaders.account, |s, value| {
            s.account = value
        }),
    ]
    .into_iter()
    .collect();

    while completed < STEPS.len() {
        let (index, result) = tokio::select! {
            biased;
            _ = &mut timer => break,
            Some(result) = tasks.next() => result,
        };
        pending[index] = false;
        completed += 1;
        match result {
            Ok(update) => update(&mut snapshot),
            Err(message) => snapshot.errors.push(BootstrapResourceError {
                resource: STEPS[index].0,
                message,
            }),
        }
        if let Some(next) = pending.iter().position(|pending| *pending) {
            emit(completed, STEPS[next].1);
        }
    }
    drop(tasks);

    for (index, (resource, label)) in STEPS.iter().enumerate() {
        if pending[index] {
            snapshot.errors.push(BootstrapResourceError {
                resource: *resource,
                message: format!(
                    "{} timed out after {}ms.",
                    label.trim_end_matches('…'),
                    timeout.as_millis()
                ),
            });
        }
    }
    emit(STEPS.len(), "Ready");
    snapshot.completed_at = kairo_store::settings::now_ms();
    snapshot
}

/// The snapshot with every resource at its neutral value and no errors:
/// what a failed resource contributes.
pub fn neutral_snapshot() -> AppBootstrapSnapshot {
    AppBootstrapSnapshot {
        settings: None,
        orchestrator: None,
        transcription: Vec::new(),
        translations: Vec::new(),
        sermon_plans: Vec::new(),
        live_plan: None,
        lyrics: Vec::new(),
        onboarding: OnboardingState {
            current_step: OnboardingStepId::Account,
            completed_at: Some(0.0),
            source: OnboardingSource::Legacy,
            ..OnboardingState::default()
        },
        account: signed_out(),
        errors: Vec::new(),
        completed_at: 0.0,
    }
}

fn signed_out() -> SessionSnapshot {
    SessionSnapshot {
        state: CloudSessionState::SignedOut,
        user: None,
        org: None,
        orgs: Vec::new(),
        last_synced_at: None,
        credentials_persisted: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::{pending, ready};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    fn loaders() -> BootstrapLoaders {
        BootstrapLoaders {
            settings: Box::pin(ready(Err("store unreadable".into()))),
            orchestrator: Box::pin(ready(Err("not started".into()))),
            transcription: Box::pin(ready(Ok(Vec::new()))),
            translations: Box::pin(ready(Ok(Vec::new()))),
            sermon_plans: Box::pin(ready(Ok(Vec::new()))),
            live_plan: Box::pin(ready(Err("no plan".into()))),
            lyrics: Box::pin(ready(Ok(Vec::new()))),
            onboarding: Box::pin(ready(Ok(OnboardingState {
                completed_at: Some(1.0),
                ..OnboardingState::default()
            }))),
            account: Box::pin(ready(Ok(signed_out()))),
        }
    }

    #[tokio::test]
    async fn failed_resources_use_neutral_values_and_keep_successes() {
        let mut progress = Vec::new();
        let result = run_bootstrap(loaders(), |event| progress.push(event)).await;
        assert_eq!(result.settings, None);
        assert_eq!(result.orchestrator, None);
        assert_eq!(result.live_plan, None);
        assert_eq!(result.onboarding.completed_at, Some(1.0));
        assert_eq!(result.errors.len(), 3);
        assert!(result.errors.contains(&BootstrapResourceError {
            resource: BootstrapResource::Settings,
            message: "store unreadable".into(),
        }));
        assert!(result.completed_at > 0.0);
        assert_eq!(progress.len(), 10);
        assert_eq!(progress[0].step, "Getting everything ready…");
        assert_eq!(progress[9].step, "Ready");
        for (index, event) in progress.iter().enumerate() {
            assert_eq!(event.completed, index as f64);
            assert_eq!(event.total, 9.0);
        }
    }

    #[tokio::test]
    async fn one_loader_panic_does_not_discard_other_resources() {
        let mut input = loaders();
        let plan = LivePlanState {
            next_reference: None,
            next_plan_item_id: None,
            plan_id: Some("plan-1".into()),
            title: Some("Sunday".into()),
            item_count: 3.0,
            unavailable_count: 0.0,
        };
        input.live_plan = Box::pin(ready(Ok(plan.clone())));
        input.lyrics = Box::pin(async { panic!("loader fault") });
        let result = run_bootstrap(input, |_| {}).await;
        assert_eq!(result.live_plan, Some(plan));
        assert!(result.lyrics.is_empty());
        assert!(result.errors.contains(&BootstrapResourceError {
            resource: BootstrapResource::Lyrics,
            message: "The startup resource stopped unexpectedly.".into(),
        }));
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_returns_partial_data_and_drops_pending_loaders() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }

        let dropped = Arc::new(AtomicBool::new(false));
        let sentinel = Dropped(dropped.clone());
        let mut input = loaders();
        input.lyrics = Box::pin(async move {
            let _sentinel = sentinel;
            pending().await
        });
        input.account = Box::pin(pending());
        input.onboarding = Box::pin(pending());
        let mut progress = Vec::new();
        let result = load_until(input, Duration::from_millis(20), |event| {
            progress.push(event);
        })
        .await;

        assert!(dropped.load(Ordering::Relaxed));
        assert!(result.lyrics.is_empty());
        assert_eq!(result.account.state, CloudSessionState::SignedOut);
        assert_eq!(result.onboarding.completed_at, Some(0.0));
        assert_eq!(result.onboarding.source, OnboardingSource::Legacy);
        assert_eq!(
            result.errors[3],
            BootstrapResourceError {
                resource: BootstrapResource::Lyrics,
                message: "Getting lyrics ready timed out after 20ms.".into(),
            }
        );
        assert_eq!(progress.last().unwrap().completed, 9.0);
        assert_eq!(progress.last().unwrap().step, "Ready");
        assert_eq!(
            progress
                .iter()
                .filter(|event| event.step == "Ready")
                .count(),
            1
        );
    }

    #[test]
    fn startup_ceiling_is_eight_seconds() {
        assert_eq!(BOOTSTRAP_TIMEOUT, Duration::from_secs(8));
    }
}
