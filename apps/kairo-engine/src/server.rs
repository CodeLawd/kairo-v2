//! The engine's end of the local socket: handshake, framing, and handing
//! messages to the winit loop. One client is current at a time; a newer
//! authenticated connection replaces it.

use interprocess::local_socket::{
    GenericNamespaced, ListenerOptions, Stream, ToNsName, prelude::*,
};
use kairo_engine_proto::*;
use std::io::BufReader;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

/// A client that connects must say hello within this long.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

pub enum Message {
    DisplaysChanged,
    Connected(u64),
    /// From the compositor thread.
    Compositor(kairo_compositor::thread::CompositorEvent),
    /// A video could not be opened or stopped decoding (from its thread).
    VideoFailed {
        path: String,
        message: String,
    },
    /// A camera went live, failed, was lost or came back (its threads).
    Camera(kairo_camera::CameraEvent),
    /// A slide's text was drawn on a worker.
    Text {
        output: String,
        job: u64,
        done: crate::text::TextDone,
    },
    /// A background picture or elements layer was made on a worker.
    Visual {
        output: String,
        job: u64,
        slot: kairo_compositor::Slot,
        key: String,
        generation: u64,
        output_kind: crate::visuals::Output,
    },
    Disconnected(u64),
    Command {
        seq: u64,
        command: Command,
    },
    Query {
        id: u64,
        query: Query,
    },
    Ping {
        nonce: u64,
    },
}

/// The connected client: its id and stream.
type Client = Option<(u64, Arc<Stream>)>;

/// Where engine messages go: the current client, if any.
#[derive(Clone, Default)]
pub struct Outbox(Arc<Mutex<Client>>);

impl Outbox {
    pub fn send(&self, message: &FromEngine) {
        let guard = self.0.lock().expect("outbox lock");
        if let Some((_, stream)) = guard.as_ref() {
            // A failed write means the client is going; its reader thread
            // sees the same error and reports the disconnect.
            if let Err(error) = write_frame(&mut &**stream, message) {
                tracing::debug!("write to client failed: {error}");
            }
        }
    }

    pub fn connected(&self) -> bool {
        self.0.lock().expect("outbox lock").is_some()
    }

    fn replace(&self, id: u64, stream: Arc<Stream>) {
        *self.0.lock().expect("outbox lock") = Some((id, stream));
    }

    fn is_current(&self, id: u64) -> bool {
        self.0
            .lock()
            .expect("outbox lock")
            .as_ref()
            .is_some_and(|(current, _)| *current == id)
    }

    /// Clears the client only if it is still `id`.
    fn release(&self, id: u64) -> bool {
        let mut guard = self.0.lock().expect("outbox lock");
        if guard.as_ref().is_some_and(|(current, _)| *current == id) {
            *guard = None;
            true
        } else {
            false
        }
    }
}

/// Constant-time comparison, so the token cannot be guessed byte by byte.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

pub fn listen(
    socket: &str,
    token: String,
    proxy: EventLoopProxy<Message>,
    outbox: Outbox,
) -> std::io::Result<()> {
    let name = socket.to_ns_name::<GenericNamespaced>()?;
    // A name left by a crashed engine is replaced; the name is random per
    // launch, so this never takes over another live engine's socket.
    let listener = ListenerOptions::new()
        .name(name)
        .try_overwrite(true)
        .create_sync()?;
    let token = Arc::new(token);
    let next_id = Arc::new(AtomicU64::new(1));
    std::thread::Builder::new()
        .name("engine-accept".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let stream = match stream {
                    Ok(stream) => stream,
                    Err(error) => {
                        tracing::warn!("accept failed: {error}");
                        continue;
                    }
                };
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let (token, proxy, outbox) = (token.clone(), proxy.clone(), outbox.clone());
                let spawned = std::thread::Builder::new()
                    .name(format!("engine-client-{id}"))
                    .spawn(move || serve(id, stream, &token, proxy, outbox));
                if let Err(error) = spawned {
                    tracing::error!("could not start a client thread: {error}");
                }
            }
        })?;
    Ok(())
}

fn serve(id: u64, stream: Stream, token: &str, proxy: EventLoopProxy<Message>, outbox: Outbox) {
    let stream = Arc::new(stream);
    if let Err(error) = stream.set_recv_timeout(Some(HANDSHAKE_TIMEOUT)) {
        tracing::warn!("client {id}: cannot set handshake timeout: {error}");
    }
    let mut reader = BufReader::new(&*stream);
    let reject = |reason: String| {
        tracing::error!("client {id} rejected: {reason}");
        let _ = write_frame(&mut &*stream, &FromEngine::Rejected { reason });
    };
    let hello = match read_frame::<ToEngine>(&mut reader) {
        Ok(ToEngine::Hello(hello)) => hello,
        Ok(other) => {
            return reject(format!("expected a handshake, got {}", kind(&other)));
        }
        Err(error) => return reject(format!("no handshake: {error}")),
    };
    if hello.protocol != PROTOCOL_VERSION {
        return reject(format!(
            "protocol version mismatch: client {}, engine {}",
            hello.protocol, PROTOCOL_VERSION
        ));
    }
    if !same_token(&hello.token, token) {
        return reject("bad handshake token".into());
    }
    if let Err(error) = stream.set_recv_timeout(None) {
        tracing::warn!("client {id}: cannot clear timeout: {error}");
    }
    let welcome = FromEngine::Welcome(Welcome {
        protocol: PROTOCOL_VERSION,
        engine_version: env!("CARGO_PKG_VERSION").into(),
        pid: std::process::id(),
    });
    if let Err(error) = write_frame(&mut &*stream, &welcome) {
        tracing::warn!("client {id}: welcome failed: {error}");
        return;
    }
    outbox.replace(id, stream.clone());
    // However this thread ends, a panic included, the loop hears that the
    // client is gone: otherwise the grace period never starts and the
    // engine outlives its UI with its windows up.
    let _gone = Gone {
        id,
        outbox: outbox.clone(),
        proxy: proxy.clone(),
    };
    tracing::info!("client {id} connected");
    if proxy.send_event(Message::Connected(id)).is_err() {
        return;
    }
    loop {
        let message = match read_frame::<ToEngine>(&mut reader) {
            Ok(message) => message,
            Err(error) => {
                if error.kind() == std::io::ErrorKind::UnexpectedEof {
                    tracing::info!("client {id} closed the connection");
                } else {
                    tracing::error!("client {id}: dropping connection: {error}");
                }
                break;
            }
        };
        if !outbox.is_current(id) {
            tracing::info!("client {id} was replaced by a newer connection");
            return;
        }
        let forwarded = match message {
            ToEngine::Hello(_) => {
                tracing::warn!("client {id}: ignoring a second handshake");
                continue;
            }
            ToEngine::Command { seq, command } => Message::Command { seq, command },
            ToEngine::Query { id, query } => Message::Query { id, query },
            ToEngine::Ping { nonce } => Message::Ping { nonce },
        };
        if proxy.send_event(forwarded).is_err() {
            return;
        }
    }
}

struct Gone {
    id: u64,
    outbox: Outbox,
    proxy: EventLoopProxy<Message>,
}

impl Drop for Gone {
    fn drop(&mut self) {
        // A connection replaced by a newer one is not a disconnect.
        if self.outbox.release(self.id) {
            let _ = self.proxy.send_event(Message::Disconnected(self.id));
        }
    }
}

fn kind(message: &ToEngine) -> &'static str {
    match message {
        ToEngine::Hello(_) => "Hello",
        ToEngine::Command { .. } => "Command",
        ToEngine::Query { .. } => "Query",
        ToEngine::Ping { .. } => "Ping",
    }
}

#[cfg(test)]
mod tests {
    use super::same_token;

    #[test]
    fn tokens_compare_exactly() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("abc", "abcd"));
        assert!(!same_token("", "a"));
    }
}
