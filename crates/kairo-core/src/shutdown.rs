//! One teardown shared by window close, app quit and updater requests.

use std::{
    fmt,
    future::Future,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{
    sync::watch,
    time::{Instant, timeout_at},
};

pub const OUTPUT_RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

/// Application services provide these operations once their actors exist.
/// Every hook is awaited on the core runtime and must yield during I/O.
pub trait ShutdownServices: Send + Sync + 'static {
    /// Finish the movie while its source is still painting. Return immediately
    /// when capture is idle.
    fn finish_capture(&self) -> impl Future<Output = Result<(), String>> + Send;
    fn shutdown_audience(&self) -> impl Future<Output = Result<(), String>> + Send;
    fn stop_quote_worker(&self) -> impl Future<Output = Result<(), String>> + Send;
    fn stop_ndi(&self) -> impl Future<Output = Result<(), String>> + Send;
    fn release_outputs(&self) -> impl Future<Output = Result<(), String>> + Send;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownStep {
    Capture,
    Audience,
    QuoteWorker,
    Ndi,
    Outputs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShutdownFailure {
    step: ShutdownStep,
    message: String,
}

impl ShutdownFailure {
    pub fn step(&self) -> ShutdownStep {
        self.step
    }
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct ShutdownReport {
    failures: Vec<ShutdownFailure>,
    outputs_timed_out: bool,
}

impl ShutdownReport {
    pub fn failures(&self) -> &[ShutdownFailure] {
        &self.failures
    }
    pub fn outputs_timed_out(&self) -> bool {
        self.outputs_timed_out
    }

    fn record(&mut self, step: ShutdownStep, result: Result<(), String>) {
        if let Err(message) = result {
            tracing::warn!(?step, %message, "Shutdown step failed");
            self.failures.push(ShutdownFailure { step, message });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShutdownInterrupted;

impl fmt::Display for ShutdownInterrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("The application runtime stopped during shutdown.")
    }
}

impl std::error::Error for ShutdownInterrupted {}

/// Store one coordinator for the app lifetime. Cancelling a waiter leaves
/// teardown running, and subsequent requests wait on the original teardown.
#[derive(Default)]
pub struct Shutdown {
    completion: OnceLock<watch::Receiver<Option<Arc<ShutdownReport>>>>,
}

impl Shutdown {
    /// Run on the core runtime after quit has been confirmed. The first caller
    /// supplies the hooks. Other callers receive the same result.
    pub async fn run(
        &self,
        services: Arc<impl ShutdownServices>,
    ) -> Result<Arc<ShutdownReport>, ShutdownInterrupted> {
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| ShutdownInterrupted)?;
        let mut completion = self
            .completion
            .get_or_init(|| {
                let (publisher, receiver) = watch::channel(None);
                runtime.spawn(async move {
                    publisher.send_replace(Some(Arc::new(execute(services).await)));
                });
                receiver
            })
            .clone();
        let result = completion
            .wait_for(Option::is_some)
            .await
            .map_err(|_| ShutdownInterrupted)?;
        result.clone().ok_or(ShutdownInterrupted)
    }
}

async fn execute(services: Arc<impl ShutdownServices>) -> ShutdownReport {
    let mut report = ShutdownReport::default();
    report.record(ShutdownStep::Capture, services.finish_capture().await);
    report.record(ShutdownStep::Audience, services.shutdown_audience().await);
    report.record(
        ShutdownStep::QuoteWorker,
        services.stop_quote_worker().await,
    );

    // Poll NDI stop first, as Electron initiates it before hardware teardown.
    // A stalled NDI service must not prevent releasing the other outputs.
    let ndi_services = services.clone();
    let mut ndi = Box::pin(async move { ndi_services.stop_ndi().await });
    let deadline = Instant::now() + OUTPUT_RELEASE_TIMEOUT;
    let release = timeout_at(deadline, async { services.release_outputs().await });
    tokio::pin!(release);
    let released = tokio::select! {
        biased;
        result = &mut ndi => {
            report.record(ShutdownStep::Ndi, result);
            release.await
        }
        result = &mut release => {
            tokio::spawn(async move {
                if let Ok(Err(message)) = timeout_at(deadline, ndi).await {
                    tracing::warn!(%message, "NDI stop failed during shutdown");
                }
            });
            result
        }
    };
    match released {
        Ok(result) => report.record(ShutdownStep::Outputs, result),
        Err(_) => {
            tracing::warn!("Output hardware release timed out");
            report.outputs_timed_out = true;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Services {
        calls: Mutex<Vec<ShutdownStep>>,
        started: Notify,
        capture: Notify,
        release: Notify,
    }

    impl ShutdownServices for Services {
        async fn finish_capture(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push(ShutdownStep::Capture);
            self.started.notify_one();
            self.capture.notified().await;
            Err("capture failed".into())
        }

        async fn shutdown_audience(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push(ShutdownStep::Audience);
            Ok(())
        }

        async fn stop_quote_worker(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push(ShutdownStep::QuoteWorker);
            Ok(())
        }

        async fn stop_ndi(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push(ShutdownStep::Ndi);
            std::future::pending().await
        }

        async fn release_outputs(&self) -> Result<(), String> {
            self.calls.lock().unwrap().push(ShutdownStep::Outputs);
            self.release.notified().await;
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn quit_survives_waiter_cancellation_and_stalled_hardware() {
        let shutdown = Arc::new(Shutdown::default());
        let services = Arc::new(Services::default());
        let waiter = tokio::spawn({
            let shutdown = shutdown.clone();
            let services = services.clone();
            async move { shutdown.run(services).await }
        });
        services.started.notified().await;
        waiter.abort();
        services.capture.notify_one();

        let start = Instant::now();
        let report = shutdown.run(services.clone()).await.unwrap();
        assert_eq!(start.elapsed(), Duration::from_secs(2));
        assert_eq!(
            *services.calls.lock().unwrap(),
            [
                ShutdownStep::Capture,
                ShutdownStep::Audience,
                ShutdownStep::QuoteWorker,
                ShutdownStep::Ndi,
                ShutdownStep::Outputs,
            ]
        );
        assert_eq!(report.failures()[0].step(), ShutdownStep::Capture);
        assert_eq!(report.failures()[0].message(), "capture failed");
        assert!(report.outputs_timed_out());
        assert!(Arc::ptr_eq(&report, &shutdown.run(services).await.unwrap()));
    }

    #[tokio::test(start_paused = true)]
    async fn released_outputs_do_not_wait_for_ndi_to_finish() {
        let services = Arc::new(Services::default());
        services.capture.notify_one();
        services.release.notify_one();
        let start = Instant::now();
        let report = Shutdown::default().run(services.clone()).await.unwrap();
        assert_eq!(start.elapsed(), Duration::ZERO);
        assert!(!report.outputs_timed_out());
        assert!(
            services
                .calls
                .lock()
                .unwrap()
                .ends_with(&[ShutdownStep::Ndi, ShutdownStep::Outputs])
        );
    }
}
