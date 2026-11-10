//! The transcription service. From `src/main/services/stt/index.ts`.
//!
//! Holds the provider, the saved transcript history and the analysis buffer.
//! Finals go into both; every event is republished to subscribers. When
//! `KAIRO_STT_REPLAY_PATH` is set, interims and finals are also recorded.

use crate::access::TranscriptionAccess;
use crate::buffer::{BufferConfig, BufferEvent, TranscriptionBuffer};
use crate::deepgram::{DeepgramStt, SttEvent};
use crate::recording::TranscriptRecorder;
use kairo_domain::status::SttProvider;
use kairo_domain::transcript::TranscriptResult;
use kairo_net::ApiError;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

#[derive(Clone, Debug, PartialEq)]
pub enum ServiceEvent {
    Stt(SttEvent),
    Buffer(BufferEvent),
}

struct State {
    provider: SttProvider,
    active: bool,
    history: Vec<TranscriptResult>,
    buffer: TranscriptionBuffer,
    recorder: Option<TranscriptRecorder>,
}

#[derive(Clone)]
pub struct SttService {
    deepgram: DeepgramStt,
    access: Arc<dyn TranscriptionAccess>,
    state: Arc<Mutex<State>>,
    events: broadcast::Sender<ServiceEvent>,
}

use kairo_domain::js::now_ms;

impl SttService {
    /// Spawns the event pump and the analyse timer on the current runtime.
    pub fn new(access: Arc<dyn TranscriptionAccess>, deepgram: DeepgramStt) -> Self {
        let (events, _) = broadcast::channel(512);
        let state = Arc::new(Mutex::new(State {
            provider: SttProvider::None,
            active: false,
            history: Vec::new(),
            buffer: TranscriptionBuffer::new(BufferConfig::default()),
            recorder: TranscriptRecorder::from_env(),
        }));
        let service = Self {
            deepgram,
            access,
            state,
            events,
        };
        service.spawn_pump();
        service
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn spawn_pump(&self) {
        let mut rx = self.deepgram.subscribe();
        let weak = Arc::downgrade(&self.state);
        let events = self.events.clone();
        tokio::spawn(async move {
            loop {
                let event = match rx.recv().await {
                    Ok(event) => event,
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(n, "stt events lagged");
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let Some(state) = weak.upgrade() else { return };
                let mut buffered = Vec::new();
                {
                    let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
                    match &event {
                        SttEvent::Final(result) => {
                            state.history.push(result.clone());
                            buffered = state.buffer.push(result, now_ms());
                            if let Some(r) = state.recorder.as_mut() {
                                r.record_final(result);
                            }
                        }
                        SttEvent::Interim(result) => {
                            if let Some(r) = state.recorder.as_mut() {
                                r.record_interim(result);
                            }
                        }
                        SttEvent::Error { message, .. } => {
                            tracing::error!(message, "deepgram error")
                        }
                        SttEvent::Reconnecting { attempt, delay_ms } => {
                            tracing::warn!(attempt, delay_ms, "deepgram reconnecting")
                        }
                        _ => {}
                    }
                }
                let _ = events.send(ServiceEvent::Stt(event));
                for b in buffered {
                    let _ = events.send(ServiceEvent::Buffer(b));
                }
            }
        });
        let weak = Arc::downgrade(&self.state);
        let events = self.events.clone();
        tokio::spawn(async move {
            loop {
                let interval = {
                    let Some(state) = weak.upgrade() else { return };
                    let seconds = state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .buffer
                        .config()
                        .analyze_interval_seconds;
                    std::time::Duration::from_secs_f64(seconds.max(0.01))
                };
                tokio::time::sleep(interval).await;
                let Some(state) = weak.upgrade() else { return };
                let ready = state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .buffer
                    .tick(now_ms());
                if let Some(event) = ready {
                    let _ = events.send(ServiceEvent::Buffer(event));
                }
            }
        });
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    pub fn deepgram(&self) -> &DeepgramStt {
        &self.deepgram
    }

    pub fn configure(&self, provider: SttProvider, language: &str, keyterms: Vec<String>) {
        self.lock().provider = provider;
        tracing::info!(?provider, language, "stt configured");
        if provider == SttProvider::Deepgram {
            self.deepgram.configure(language, keyterms);
        }
    }

    pub fn configure_buffer(&self, config: BufferConfig) {
        self.lock().buffer.configure(config);
    }

    /// Refuses without the transcription feature. With no provider, does nothing.
    pub async fn start(&self) -> Result<(), ApiError> {
        self.access.assert_transcription().await?;
        let provider = self.lock().provider;
        if provider == SttProvider::None {
            tracing::warn!("stt start called with no provider configured");
            return Ok(());
        }
        self.lock().active = true;
        tracing::info!(?provider, "stt starting");
        if provider == SttProvider::Deepgram {
            self.deepgram.connect().await;
        }
        Ok(())
    }

    /// Keeps the saved transcript, but never analyses a previous window again.
    pub fn stop(&self) {
        let provider = {
            let mut state = self.lock();
            state.active = false;
            state.buffer.clear();
            state.provider
        };
        tracing::info!(?provider, "stt stopped");
        if provider == SttProvider::Deepgram {
            self.deepgram.disconnect();
        }
    }

    pub fn history(&self) -> Vec<TranscriptResult> {
        self.lock().history.clone()
    }

    pub fn set_history(&self, history: Vec<TranscriptResult>) {
        let mut state = self.lock();
        state.buffer.clear_all();
        let now = now_ms();
        for result in &history {
            // Restoring is not new speech: buffer events are not republished.
            let _ = state.buffer.push(result, now);
        }
        tracing::info!(count = history.len(), "transcript history restored");
        state.history = history;
    }

    pub fn clear_history(&self) {
        let mut state = self.lock();
        state.history.clear();
        state.buffer.clear_all();
    }

    pub fn is_active(&self) -> bool {
        self.lock().active
    }

    pub fn with_buffer<R>(&self, f: impl FnOnce(&TranscriptionBuffer) -> R) -> R {
        f(&self.lock().buffer)
    }
}
