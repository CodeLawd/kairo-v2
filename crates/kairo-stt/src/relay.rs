//! The capture-to-transcription relay. From `src/main/services/audio/index.ts`.
//!
//! Capture pushes 100 ms PCM chunks; the Deepgram connection pulls them.
//! While nothing pulls, at most [`MAX_QUEUED_BYTES`] (5 s) are held and the
//! oldest chunks give way, so a stalled consumer cannot grow memory and,
//! once it recovers, hears the preacher now rather than seconds ago. Deepgram
//! keeps its own replay buffer for reconnects.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::Notify;

/// 5 s of 16 kHz mono PCM16.
pub const MAX_QUEUED_BYTES: usize = 160_000;
const OVERFLOW_LOG_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Queue {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
    dropped_chunks: u64,
    dropped_bytes: u64,
    last_overflow_log: Option<Instant>,
}

struct Stream {
    queue: Mutex<Queue>,
    ready: Notify,
    closed: std::sync::atomic::AtomicBool,
}

/// Owns the current stream. Cloning shares it.
#[derive(Clone, Default)]
pub struct AudioRelay {
    current: Arc<Mutex<Option<Arc<Stream>>>>,
}

/// The pulling end of one stream. Returns `None` once the stream is replaced
/// or destroyed.
pub struct PcmStream {
    stream: Arc<Stream>,
}

impl AudioRelay {
    /// Replaces any previous stream, which ends for its reader.
    pub fn create_stream(&self) -> PcmStream {
        self.destroy_stream();
        let stream = Arc::new(Stream {
            queue: Mutex::new(Queue::default()),
            ready: Notify::new(),
            closed: false.into(),
        });
        *self.current.lock().unwrap_or_else(|e| e.into_inner()) = Some(stream.clone());
        tracing::info!("PCM relay stream created");
        PcmStream { stream }
    }

    /// Queue one chunk. A no-op without a stream; past the cap the oldest
    /// queued audio is dropped to make room.
    pub fn feed(&self, chunk: Vec<u8>) {
        let Some(stream) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        else {
            return;
        };
        {
            let mut queue = stream.queue.lock().unwrap_or_else(|e| e.into_inner());
            if chunk.len() > MAX_QUEUED_BYTES {
                return;
            }
            if queue.bytes + chunk.len() > MAX_QUEUED_BYTES {
                while queue.bytes + chunk.len() > MAX_QUEUED_BYTES {
                    let Some(oldest) = queue.chunks.pop_front() else {
                        break;
                    };
                    queue.bytes -= oldest.len();
                    queue.dropped_chunks += 1;
                    queue.dropped_bytes += oldest.len() as u64;
                }
                let now = Instant::now();
                if queue
                    .last_overflow_log
                    .is_none_or(|at| now - at >= OVERFLOW_LOG_INTERVAL)
                {
                    queue.last_overflow_log = Some(now);
                    tracing::warn!(
                        queued = queue.bytes,
                        cap = MAX_QUEUED_BYTES,
                        dropped_chunks = queue.dropped_chunks,
                        dropped_bytes = queue.dropped_bytes,
                        "PCM queue full; dropping oldest audio"
                    );
                }
            }
            queue.bytes += chunk.len();
            queue.chunks.push_back(chunk);
        }
        stream.ready.notify_one();
    }

    pub fn destroy_stream(&self) {
        let Some(stream) = self
            .current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        else {
            return;
        };
        stream
            .closed
            .store(true, std::sync::atomic::Ordering::Release);
        stream.ready.notify_one();
        let queue = stream.queue.lock().unwrap_or_else(|e| e.into_inner());
        tracing::info!(
            dropped_chunks = queue.dropped_chunks,
            dropped_bytes = queue.dropped_bytes,
            "PCM relay stream destroyed"
        );
    }

    /// Bytes waiting in the current stream.
    pub fn queued_bytes(&self) -> usize {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map_or(0, |s| {
                s.queue.lock().unwrap_or_else(|e| e.into_inner()).bytes
            })
    }

    pub fn has_stream(&self) -> bool {
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }
}

impl PcmStream {
    /// The next chunk, waiting for one. `None` when the stream has ended.
    pub async fn recv(&self) -> Option<Vec<u8>> {
        loop {
            let notified = self.stream.ready.notified();
            if let Some(chunk) = self.try_recv() {
                return Some(chunk);
            }
            if self
                .stream
                .closed
                .load(std::sync::atomic::Ordering::Acquire)
            {
                return None;
            }
            notified.await;
        }
    }

    pub fn try_recv(&self) -> Option<Vec<u8>> {
        let mut queue = self.stream.queue.lock().unwrap_or_else(|e| e.into_inner());
        let chunk = queue.chunks.pop_front()?;
        queue.bytes -= chunk.len();
        Some(chunk)
    }

    pub fn is_ended(&self) -> bool {
        self.stream
            .closed
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 100 ms of 16 kHz mono PCM16.
    const CHUNK: usize = 3200;

    fn chunk() -> Vec<u8> {
        vec![0; CHUNK]
    }

    #[test]
    fn feeding_without_a_stream_is_a_no_op() {
        let relay = AudioRelay::default();
        relay.feed(chunk());
        assert!(!relay.has_stream());
    }

    #[tokio::test]
    async fn a_flowing_consumer_receives_every_chunk() {
        let relay = AudioRelay::default();
        let stream = relay.create_stream();
        for _ in 0..100 {
            relay.feed(chunk());
            assert_eq!(stream.recv().await.unwrap().len(), CHUNK);
        }
        assert_eq!(relay.queued_bytes(), 0);
    }

    #[test]
    fn a_stalled_consumer_keeps_the_newest_five_seconds() {
        let relay = AudioRelay::default();
        let stream = relay.create_stream();
        for i in 0..600u32 {
            let mut c = chunk();
            c[..4].copy_from_slice(&i.to_le_bytes());
            relay.feed(c);
        }
        assert!(relay.queued_bytes() <= MAX_QUEUED_BYTES);
        // Drains in order from the oldest kept, which is the newest 5 s.
        let mut kept = Vec::new();
        while let Some(c) = stream.try_recv() {
            kept.push(u32::from_le_bytes(c[..4].try_into().unwrap()));
        }
        assert_eq!(kept, (550..600).collect::<Vec<_>>());
        relay.feed(chunk());
        assert_eq!(relay.queued_bytes(), CHUNK);
    }

    #[tokio::test]
    async fn replacing_the_stream_ends_the_old_one() {
        let relay = AudioRelay::default();
        let first = relay.create_stream();
        let second = relay.create_stream();
        assert_eq!(first.recv().await, None);
        relay.feed(chunk());
        assert_eq!(second.try_recv().unwrap().len(), CHUNK);
        relay.destroy_stream();
        assert!(!relay.has_stream());
        assert_eq!(second.recv().await, None);
    }
}
