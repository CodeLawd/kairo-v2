use kairo_core::application::{ApplicationLifecycle, BackgroundTask, LifecycleServices, LocalStep};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

type Call = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;
#[derive(Default)]
struct Services {
    calls: Mutex<Vec<String>>,
    fail: Option<LocalStep>,
}
impl LifecycleServices for Services {
    fn local_step(&self, step: LocalStep) -> Call {
        self.calls.lock().unwrap().push(format!("{step:?}"));
        let result = if self.fail == Some(step) {
            Err("disk failure".into())
        } else {
            Ok(())
        };
        Box::pin(async move { result })
    }
    fn start_background(&self, task: BackgroundTask) {
        self.calls.lock().unwrap().push(format!("{task:?}"));
    }
}
#[tokio::test]
async fn startup_preserves_source_order_without_waiting_for_network_or_scans() {
    let services = Arc::new(Services::default());
    ApplicationLifecycle::new(services.clone())
        .start()
        .await
        .unwrap();
    assert_eq!(
        *services.calls.lock().unwrap(),
        [
            "Settings",
            "Workspace",
            "DefaultMediaFolder",
            "OpenLyrics",
            "OpenScripture",
            "MigrateNkjvDefault",
            "DefaultTranslation",
            "OfflineBibles",
            "LivePlan",
            "OnboardingMigration",
            "Reporting",
            "License",
            "Session",
            "ScanMedia",
            "ScanTracks",
            "Ndi",
            "Updater",
            "Window",
            "SurfaceInit",
            "Reconcile"
        ]
    );
}
#[tokio::test]
async fn fatal_local_failure_does_not_start_later_services() {
    let services = Arc::new(Services {
        fail: Some(LocalStep::OpenLyrics),
        ..Default::default()
    });
    let error = ApplicationLifecycle::new(services.clone())
        .start()
        .await
        .unwrap_err();
    assert_eq!(error.step, LocalStep::OpenLyrics);
    assert_eq!(error.message, "disk failure");
    assert_eq!(services.calls.lock().unwrap().last().unwrap(), "OpenLyrics");
}
#[test]
fn resume_only_reconnects_and_focus_pulls_throttled_then_wakes_uploaders() {
    let services = Arc::new(Services::default());
    let lifecycle = ApplicationLifecycle::new(services.clone());
    lifecycle.resume();
    lifecycle.focus();
    assert_eq!(
        *services.calls.lock().unwrap(),
        [
            "ReconnectSession",
            "PullOrgSecretsThrottled",
            "WakeSermonUploader",
            "WakeUsage"
        ]
    );
}
