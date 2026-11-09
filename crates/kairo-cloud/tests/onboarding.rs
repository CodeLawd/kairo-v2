use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use kairo_cloud::onboarding::{GetOnboarding, Onboarding, OnboardingCommand};
use kairo_core::AppCore;
use kairo_domain::settings::{OnboardingSource, OnboardingState, OnboardingStepId};
use kairo_store::settings::{LaunchContext, MigrationFlag, SettingsStore};
use serde_json::json;

fn open_settings(path: &Path) -> Arc<Mutex<SettingsStore>> {
    Arc::new(Mutex::new(SettingsStore::open(
        path,
        &mut LaunchContext {
            now_ms: 1.0,
            new_id: &mut || "fixture-id".into(),
        },
    )))
}

#[tokio::test]
async fn seeds_once_and_preserves_wizard_choices_on_reopen() {
    use OnboardingStepId::{Account, Output, Welcome};

    let profile = tempfile::tempdir().unwrap();
    let settings = open_settings(profile.path());
    settings.lock().unwrap().set_raw(
        "overlay",
        json!({"outputs": [{"id":"fixture-output","kind":"screen","displayId":12,"enabled":true}]}),
    );
    let core = AppCore::new().unwrap();
    Onboarding::register(&core, profile.path().into(), settings.clone())
        .await
        .unwrap();
    let seeded = core.call(GetOnboarding).await.unwrap();
    assert_eq!(seeded.source, OnboardingSource::Legacy);
    assert_eq!(seeded.completed_steps, vec![Output]);
    assert_eq!(seeded.skipped_steps, vec![Welcome, Account]);
    assert!(seeded.completed_at.is_some());
    assert!(
        settings
            .lock()
            .unwrap()
            .migration_done(MigrationFlag::CloudOnboarding)
    );

    let mut updates = core.watch::<OnboardingState>();
    assert_eq!(updates.next().await.as_deref(), Some(&seeded));
    let reset = core.call(OnboardingCommand::Reset).await.unwrap().unwrap();
    assert_eq!(reset.source, OnboardingSource::Legacy);
    assert_eq!(reset.completed_at, None);
    let positioned = core
        .call(OnboardingCommand::SetCurrent(Account))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(positioned.current_step, Account);
    core.call(OnboardingCommand::Skip(Account))
        .await
        .unwrap()
        .unwrap();
    let completed = core
        .call(OnboardingCommand::Complete(Account))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.completed_steps, vec![Account]);
    assert!(completed.skipped_steps.is_empty());
    assert_eq!(completed.current_step, Account);
    let finished = core.call(OnboardingCommand::Finish).await.unwrap().unwrap();
    assert_eq!(updates.next().await.as_deref(), Some(&finished));
    assert_eq!(
        core.call(OnboardingCommand::Finish).await.unwrap().unwrap(),
        finished
    );
    core.stop_actors().await;
    drop(settings);

    let settings = open_settings(profile.path());
    let core = AppCore::new().unwrap();
    Onboarding::register(&core, profile.path().into(), settings)
        .await
        .unwrap();
    assert_eq!(core.call(GetOnboarding).await.unwrap(), finished);
}

#[tokio::test]
async fn adopts_legacy_store_and_heals_retired_steps_without_reopening_wizard() {
    let profile = tempfile::tempdir().unwrap();
    let settings = open_settings(profile.path());
    {
        let mut settings = settings.lock().unwrap();
        settings.mark_migration_done(MigrationFlag::CloudOnboarding);
        settings.flush().unwrap();
    }
    std::fs::write(
        profile.path().join("proautomate-onboarding.json"),
        r#"{"onboarding":{"currentStep":"apiKeys","completedSteps":["output","apiKeys","output"],"skippedSteps":"invalid","completedAt":0,"source":"legacy"}}"#,
    )
    .unwrap();
    let core = AppCore::new().unwrap();
    Onboarding::register(&core, profile.path().into(), settings)
        .await
        .unwrap();
    let state = core.call(GetOnboarding).await.unwrap();
    assert_eq!(state.source, OnboardingSource::Legacy);
    assert_eq!(state.current_step, OnboardingStepId::Welcome);
    assert_eq!(state.completed_steps, vec![OnboardingStepId::Output]);
    assert!(state.skipped_steps.is_empty());
    assert_eq!(state.completed_at, Some(0.0));
    assert!(profile.path().join("kairo-onboarding.json").is_file());
}

#[tokio::test]
async fn migration_is_retried_after_an_unreadable_destination() {
    let profile = tempfile::tempdir().unwrap();
    let settings = open_settings(profile.path());
    let path = profile.path().join("kairo-onboarding.json");
    std::fs::create_dir(&path).unwrap();

    let core = AppCore::new().unwrap();
    assert!(
        Onboarding::register(&core, profile.path().into(), settings.clone())
            .await
            .is_err()
    );
    assert!(path.is_dir());
    assert!(
        !settings
            .lock()
            .unwrap()
            .migration_done(MigrationFlag::CloudOnboarding)
    );

    std::fs::remove_dir(path).unwrap();
    Onboarding::register(&core, profile.path().into(), settings.clone())
        .await
        .unwrap();
    let fresh = core.call(GetOnboarding).await.unwrap();
    assert_eq!(fresh, OnboardingState::default());
    assert!(
        settings
            .lock()
            .unwrap()
            .migration_done(MigrationFlag::CloudOnboarding)
    );
}

#[tokio::test]
async fn legacy_provider_presence_skips_the_tour_without_storing_the_key() {
    let profile = tempfile::tempdir().unwrap();
    let settings = open_settings(profile.path());
    settings.lock().unwrap().set_raw(
        "stt",
        json!({ "apiKey": "synthetic-retired-provider-value" }),
    );
    let core = AppCore::new().unwrap();
    Onboarding::register(&core, profile.path().into(), settings)
        .await
        .unwrap();
    let state = core.call(GetOnboarding).await.unwrap();
    assert_eq!(state.source, OnboardingSource::Legacy);
    assert_eq!(state.current_step, OnboardingStepId::Output);
    assert_eq!(
        state.skipped_steps,
        vec![OnboardingStepId::Welcome, OnboardingStepId::Account]
    );
    assert_eq!(state.completed_at, None);
    let file = std::fs::read_to_string(profile.path().join("kairo-onboarding.json")).unwrap();
    assert!(!file.contains("synthetic-retired-provider-value"));
}
