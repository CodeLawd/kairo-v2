use std::{future, num::NonZeroUsize, sync::Arc, time::Instant};

use futures_lite::future::{block_on, poll_once};
use kairo_core::{AppCore, CoreError, Handle, Message, StreamError, shutdown::ShutdownServices};
use tokio::sync::oneshot;

struct Counter(u64);
struct Add(u64);
struct Get;
struct Stall(oneshot::Sender<()>);

impl Message for Add {
    type Actor = Counter;
}
impl Message for Get {
    type Actor = Counter;
}
impl Message for Stall {
    type Actor = Counter;
}

impl Handle<Add> for Counter {
    type Reply = ();

    async fn handle(&mut self, message: Add) {
        self.0 += message.0;
    }
}

impl Handle<Get> for Counter {
    type Reply = u64;

    async fn handle(&mut self, _: Get) -> u64 {
        self.0
    }
}

impl Handle<Stall> for Counter {
    type Reply = ();

    async fn handle(&mut self, message: Stall) {
        let _ = message.0.send(());
        future::pending().await
    }
}

fn core() -> AppCore {
    let core = AppCore::new().unwrap();
    core.register(Counter(0), NonZeroUsize::MIN).unwrap();
    core
}

#[test]
fn send_and_call_are_serialized_by_actor() {
    let core = core();
    core.send(Add(2)).unwrap();
    assert_eq!(block_on(core.call(Get)), Ok(2));
    assert_eq!(
        core.register(Counter(1), NonZeroUsize::MIN),
        Err(CoreError::AlreadyRegistered)
    );
}

struct Teardown;

impl ShutdownServices for Teardown {
    async fn finish_capture(&self) -> Result<(), String> {
        Ok(())
    }
    async fn shutdown_audience(&self) -> Result<(), String> {
        Ok(())
    }
    async fn stop_quote_worker(&self) -> Result<(), String> {
        Ok(())
    }
    async fn stop_ndi(&self) -> Result<(), String> {
        Ok(())
    }
    async fn release_outputs(&self) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn stalled_actor_rejects_overflow_and_shutdown_releases_callers() {
    block_on(async {
        let core = core();
        let (started, entered) = oneshot::channel();
        let mut running = Box::pin(core.call(Stall(started)));
        assert_eq!(poll_once(&mut running).await, None);
        entered.await.unwrap();

        let mut queued = Box::pin(core.call(Get));
        assert_eq!(poll_once(&mut queued).await, None);
        assert_eq!(core.send(Add(1)), Err(CoreError::Busy));

        let mut waiting = Box::pin(core.call(Get));
        assert_eq!(poll_once(&mut waiting).await, None);

        let mut background = Box::pin(core.run(future::pending::<()>()));
        assert_eq!(poll_once(&mut background).await, None);
        assert!(
            core.shutdown(Arc::new(Teardown))
                .await
                .unwrap()
                .failures()
                .is_empty()
        );
        assert_eq!(background.await, Err(CoreError::Stopped));
        assert_eq!(running.await, Err(CoreError::Stopped));
        assert_eq!(queued.await, Err(CoreError::Stopped));
        assert_eq!(waiting.await, Err(CoreError::Stopped));
        assert_eq!(core.send(Add(1)), Err(CoreError::Stopped));
    });
}

struct Gate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
impl Message for Gate {
    type Actor = Counter;
}
impl Handle<Gate> for Counter {
    type Reply = ();
    async fn handle(&mut self, gate: Gate) {
        let _ = gate.entered.send(());
        let _ = gate.release.await;
    }
}

#[test]
fn dropping_a_queued_call_prevents_its_side_effect() {
    block_on(async {
        let core = core();
        let (entered, started) = oneshot::channel();
        let (release, released) = oneshot::channel();
        core.send(Gate {
            entered,
            release: released,
        })
        .unwrap();
        started.await.unwrap();
        let mut cancelled = Box::pin(core.call(Add(5)));
        assert_eq!(poll_once(&mut cancelled).await, None);
        drop(cancelled);
        release.send(()).unwrap();
        assert_eq!(core.call(Get).await, Ok(0));
    });
}

#[test]
fn watch_replays_latest_and_stream_reports_ordered_gap() {
    block_on(async {
        let core = core();
        let pub_ = core.publisher();
        pub_.publish(0_u64);
        let mut watch = core.watch::<u64>();
        let mut stream = core.stream::<u64>();
        let mut second = core.stream::<u64>();
        assert_eq!(watch.next().await.as_deref(), Some(&0));
        for value in 1_u64..=67 {
            pub_.publish(value);
            pub_.emit(value);
            assert_eq!(second.next().await, Some(Ok(value.into())));
        }
        assert_eq!(watch.next().await.as_deref(), Some(&67));
        assert_eq!(stream.next().await, Some(Err(StreamError::Lagged(3))));
        for value in 4..=67 {
            assert_eq!(stream.next().await, Some(Ok(value.into())));
        }
        drop(pub_);
        drop(core);
        assert_eq!(watch.next().await, None);
        assert_eq!(stream.next().await, None);
    });
}

#[test]
#[ignore = "round-trip measurement"]
fn call_round_trip() {
    block_on(async {
        let core = core();
        for _ in 0..1_000 {
            assert_eq!(core.call(Get).await, Ok(0));
        }
        let mut samples = (0..20_000)
            .map(|_| std::time::Duration::ZERO)
            .collect::<Vec<_>>();
        for sample in &mut samples {
            let started = Instant::now();
            assert_eq!(core.call(Get).await, Ok(0));
            *sample = started.elapsed();
        }
        samples.sort_unstable();
        eprintln!(
            "call RTT p50 {:?}, p99 {:?}",
            samples[10_000], samples[19_800]
        );
    });
}

#[test]
#[ignore = "idle CPU measurement"]
fn idle_core() {
    let _core = core();
    std::thread::sleep(std::time::Duration::from_secs(5));
}
