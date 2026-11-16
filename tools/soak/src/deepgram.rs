//! A local stand-in for Deepgram and the licence server, reached through a
//! TCP proxy that can be cut to emulate losing the network. The real
//! `kairo_stt::deepgram::DeepgramStt` client talks to it with its default
//! (Electron) timings.

use futures_util::{SinkExt, StreamExt};
use kairo_domain::cloud::TranscriptionGrant;
use kairo_net::ApiError;
use kairo_stt::access::{BoxFuture, TranscriptionAccess};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

/// The fake transcription server: counts the audio it receives.
pub struct FakeDeepgram {
    pub addr: std::net::SocketAddr,
    received: Arc<AtomicU64>,
    connections: Arc<AtomicU64>,
    /// Bumped to close every open socket.
    close: watch::Sender<u64>,
}

impl FakeDeepgram {
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let received = Arc::new(AtomicU64::new(0));
        let connections = Arc::new(AtomicU64::new(0));
        let (close, _) = watch::channel(0u64);
        let (bytes, count, closer) = (received.clone(), connections.clone(), close.clone());
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let (bytes, count) = (bytes.clone(), count.clone());
                let mut close = closer.subscribe();
                close.borrow_and_update();
                tokio::spawn(async move {
                    let Ok(mut ws) = tokio_tungstenite::accept_async(tcp).await else {
                        return;
                    };
                    count.fetch_add(1, Ordering::Relaxed);
                    // Deepgram sends results and metadata; the client treats
                    // a long silence from the server as a half-open socket.
                    let mut tick = tokio::time::interval(Duration::from_secs(2));
                    loop {
                        tokio::select! {
                            _ = close.changed() => {
                                let _ = ws.close(None).await;
                                return;
                            }
                            _ = tick.tick() => {
                                let metadata = r#"{"type":"Metadata","request_id":"soak"}"#;
                                if ws.send(Message::Text(metadata.into())).await.is_err() {
                                    return;
                                }
                            }
                            frame = ws.next() => match frame {
                                Some(Ok(Message::Binary(data))) => {
                                    bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
                                }
                                Some(Ok(_)) => {}
                                _ => return,
                            },
                        }
                    }
                });
            }
        });
        Ok(Self {
            addr,
            received,
            connections,
            close,
        })
    }

    /// Audio bytes received over every connection.
    pub fn received(&self) -> u64 {
        self.received.load(Ordering::Relaxed)
    }

    /// Accepted WebSocket connections so far.
    pub fn connections(&self) -> u64 {
        self.connections.load(Ordering::Relaxed)
    }

    /// The server closes every open socket (a Deepgram-side disconnect).
    pub fn disconnect_all(&self) {
        self.close.send_modify(|n| *n += 1);
    }
}

/// A TCP forwarder that can be cut: open connections are reset and new
/// ones are closed at once, as when the network goes away.
pub struct Proxy {
    pub addr: std::net::SocketAddr,
    cut: Arc<AtomicBool>,
    links: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl Proxy {
    pub async fn start(target: std::net::SocketAddr) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let cut = Arc::new(AtomicBool::new(false));
        let links: Arc<Mutex<Vec<JoinHandle<()>>>> = Arc::default();
        let (is_cut, registry) = (cut.clone(), links.clone());
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                if is_cut.load(Ordering::SeqCst) {
                    drop(inbound);
                    continue;
                }
                let link = tokio::spawn(async move {
                    let Ok(mut outbound) = TcpStream::connect(target).await else {
                        return;
                    };
                    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                });
                let mut links = registry.lock().unwrap_or_else(|p| p.into_inner());
                links.retain(|l| !l.is_finished());
                links.push(link);
            }
        });
        Ok(Self { addr, cut, links })
    }

    pub fn is_cut(&self) -> bool {
        self.cut.load(Ordering::SeqCst)
    }

    pub fn cut_flag(&self) -> Arc<AtomicBool> {
        self.cut.clone()
    }

    pub fn cut(&self) {
        self.cut.store(true, Ordering::SeqCst);
        for link in self
            .links
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
        {
            link.abort();
        }
    }

    pub fn restore(&self) {
        self.cut.store(false, Ordering::SeqCst);
    }
}

/// The licence server: grants every request unless the network is cut.
pub struct Access {
    pub network_cut: Arc<AtomicBool>,
    issued: AtomicU64,
}

impl Access {
    pub fn new(network_cut: Arc<AtomicBool>) -> Self {
        Self {
            network_cut,
            issued: AtomicU64::new(0),
        }
    }

    fn unreachable(&self) -> Option<ApiError> {
        self.network_cut.load(Ordering::SeqCst).then(|| ApiError {
            status: None,
            message: "The network is unreachable".into(),
        })
    }
}

impl TranscriptionAccess for Access {
    fn transcription(&self) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(async move {
            if let Some(error) = self.unreachable() {
                return Err(error);
            }
            let n = self.issued.fetch_add(1, Ordering::Relaxed) + 1;
            Ok(TranscriptionGrant {
                access_token: format!("soak-{n}"),
                expires_in: 600.0,
                session_id: format!("soak-session-{n}"),
                renew_after_seconds: 300.0,
            })
        })
    }

    fn renew_transcription(
        &self,
        id: String,
    ) -> BoxFuture<'_, Result<TranscriptionGrant, ApiError>> {
        Box::pin(async move {
            if let Some(error) = self.unreachable() {
                return Err(error);
            }
            Ok(TranscriptionGrant {
                access_token: format!("soak-renewed-{id}"),
                expires_in: 600.0,
                session_id: id,
                renew_after_seconds: 300.0,
            })
        })
    }

    fn release_transcription(&self, _id: String) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async { Ok(()) })
    }

    fn assert_transcription(&self) -> BoxFuture<'_, Result<(), ApiError>> {
        Box::pin(async move { self.unreachable().map_or(Ok(()), Err) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn echo() -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buffer = [0u8; 64];
                    while let Ok(n) = socket.read(&mut buffer).await {
                        if n == 0 || socket.write_all(&buffer[..n]).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        addr
    }

    async fn round_trip(addr: std::net::SocketAddr) -> bool {
        let Ok(mut socket) = TcpStream::connect(addr).await else {
            return false;
        };
        if socket.write_all(b"ping").await.is_err() {
            return false;
        }
        let mut buffer = [0u8; 4];
        matches!(
            tokio::time::timeout(Duration::from_secs(1), socket.read_exact(&mut buffer)).await,
            Ok(Ok(_))
        ) && &buffer == b"ping"
    }

    #[tokio::test]
    async fn a_cut_proxy_resets_open_links_and_refuses_new_ones_until_restored() {
        let proxy = Proxy::start(echo().await).await.unwrap();
        assert!(round_trip(proxy.addr).await);
        let mut open = TcpStream::connect(proxy.addr).await.unwrap();
        open.write_all(b"x").await.unwrap();
        let mut one = [0u8; 1];
        open.read_exact(&mut one).await.unwrap();
        proxy.cut();
        assert!(proxy.is_cut());
        // The open link ends: a read sees EOF or a reset.
        let read = tokio::time::timeout(Duration::from_secs(2), open.read(&mut one)).await;
        assert!(matches!(read, Ok(Ok(0)) | Ok(Err(_))), "{read:?}");
        assert!(!round_trip(proxy.addr).await);
        proxy.restore();
        assert!(round_trip(proxy.addr).await);
    }

    #[tokio::test]
    async fn the_licence_server_refuses_while_the_network_is_cut() {
        let cut = Arc::new(AtomicBool::new(false));
        let access = Access::new(cut.clone());
        let first = access.transcription().await.unwrap();
        assert_ne!(
            first.access_token,
            access.transcription().await.unwrap().access_token
        );
        cut.store(true, Ordering::SeqCst);
        let error = access.transcription().await.unwrap_err();
        assert_eq!(error.status, None);
        assert!(access.assert_transcription().await.is_err());
        assert!(access.renew_transcription("a".into()).await.is_err());
        cut.store(false, Ordering::SeqCst);
        assert_eq!(
            access
                .renew_transcription("a".into())
                .await
                .unwrap()
                .session_id,
            "a"
        );
        assert!(access.release_transcription("a".into()).await.is_ok());
    }
}
