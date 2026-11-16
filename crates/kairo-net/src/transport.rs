//! The one HTTP client setup and transport error every crate shares.
//!
//! Every client honours the OS proxy settings (as `electron.net` does) and
//! fails a connect or a stalled read instead of hanging. Callers add a
//! whole-request timeout where an operation has a deadline of its own.

use std::time::Duration;

/// Longest a TCP and TLS connect may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest a response may go without sending a byte.
pub const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// A builder with the shared connect and read timeouts. Add a user agent,
/// redirect policy or whole-request timeout as the caller needs.
///
/// Every client shares one TLS configuration. Left to itself, `reqwest`
/// reads the operating system's root certificates again for each client it
/// builds, which took about 115 ms per client on macOS and five clients
/// are built during launch (prompt 107). TLS options set on the builder
/// after this call are ignored; none are used.
pub fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT);
    match shared_tls() {
        Some(tls) => builder.use_preconfigured_tls(tls.clone()),
        None => builder,
    }
}

/// Loads the shared TLS configuration now, so the first client built on a
/// startup path does not wait for the root store. Call it on a spare thread.
pub fn warm_tls() {
    let _ = shared_tls();
}

/// The TLS configuration `reqwest` would build for a default client:
/// native roots (unparseable ones skipped, as `reqwest` does), the ring
/// provider, TLS 1.2 and 1.3, SNI, HTTP/1.1 ALPN. `None` when the native
/// store has no usable root, so `reqwest` reports that error itself.
fn shared_tls() -> Option<&'static rustls::ClientConfig> {
    static TLS: std::sync::OnceLock<Option<rustls::ClientConfig>> = std::sync::OnceLock::new();
    TLS.get_or_init(|| {
        let loaded = rustls_native_certs::load_native_certs();
        for error in &loaded.errors {
            tracing::debug!(%error, "native root certificate store error");
        }
        let mut roots = rustls::RootCertStore::empty();
        let (valid, invalid) = roots.add_parsable_certificates(loaded.certs);
        tracing::debug!(valid, invalid, "native root certificates loaded");
        if valid == 0 {
            return None;
        }
        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .unwrap_or_else(|| std::sync::Arc::new(rustls::crypto::ring::default_provider()));
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Some(config)
    })
    .as_ref()
}

/// The shared client with default settings.
pub fn client() -> reqwest::Client {
    client_builder().build().unwrap_or_default()
}

/// Why a request failed, classified once from the `reqwest` error rather
/// than by each caller parsing its text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    /// The request or a read ran out of time.
    #[error("timed out: {0}")]
    Timeout(String),
    /// The host could not be resolved or reached: the machine looks offline.
    #[error("network error: {0}")]
    Offline(String),
    /// The server answered with a failure status.
    #[error("HTTP {status}")]
    Status { status: u16 },
    /// Anything else (TLS, a broken body, a bad URL).
    #[error("network error: {0}")]
    Other(String),
}

impl TransportError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }
}

impl From<reqwest::Error> for TransportError {
    fn from(error: reqwest::Error) -> Self {
        Self::from(&error)
    }
}

impl From<&reqwest::Error> for TransportError {
    fn from(error: &reqwest::Error) -> Self {
        let text = error.to_string();
        if error.is_timeout() {
            return Self::Timeout(text);
        }
        if let Some(status) = error.status() {
            return Self::Status {
                status: status.as_u16(),
            };
        }
        if error.is_connect() && unreachable(error) {
            return Self::Offline(text);
        }
        Self::Other(text)
    }
}

/// A connect failure caused by name resolution or an unreachable network.
fn unreachable(error: &reqwest::Error) -> bool {
    use std::error::Error as _;
    use std::io::ErrorKind;
    let mut source = error.source();
    while let Some(cause) = source {
        if let Some(io) = cause.downcast_ref::<std::io::Error>()
            && matches!(
                io.kind(),
                ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable | ErrorKind::NotFound
            )
        {
            return true;
        }
        // hyper-util reports a failed lookup only in its message.
        let text = cause.to_string().to_lowercase();
        if text.contains("dns error")
            || text.contains("failed to lookup")
            || text.contains("nodename")
        {
            return true;
        }
        source = cause.source();
    }
    false
}

/// Why a file download failed.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The server answered with a failure status.
    #[error("HTTP {0}")]
    Status(u16),
    /// The body ended early without an error: a broken connection.
    #[error("network error: received {received} of {expected} bytes")]
    Truncated { received: u64, expected: u64 },
    /// The body grew past the caller's limit; the transfer was cancelled.
    #[error("download is larger than {0} bytes")]
    TooLarge(u64),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A finished download: its size and the digest of its bytes.
pub struct FileDownload {
    pub received: u64,
    pub digest: Vec<u8>,
}

/// Streams `url` into `out`, hashing with `H` as it goes, then flushes and
/// fsyncs the file so a verified download survives a power cut.
///
/// `expected_size` (or the server's length) is the total passed to
/// `progress(received, total)` after each chunk; a body shorter than
/// `expected_size` fails as [`DownloadError::Truncated`]. `max_bytes` caps
/// the body. The caller compares the digest with what it trusts.
pub async fn download_to_file<H: sha2::Digest>(
    client: &reqwest::Client,
    url: &str,
    out: &std::path::Path,
    expected_size: Option<u64>,
    max_bytes: Option<u64>,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<FileDownload, DownloadError> {
    use tokio::io::AsyncWriteExt;
    let mut response = client.get(url).send().await.map_err(TransportError::from)?;
    let status = response.status();
    if !status.is_success() {
        return Err(DownloadError::Status(status.as_u16()));
    }
    let total = response
        .content_length()
        .filter(|n| *n > 0)
        .or(expected_size);
    if let (Some(max), Some(total)) = (max_bytes, total)
        && total > max
    {
        return Err(DownloadError::TooLarge(max));
    }
    // Buffered: each unbuffered tokio write is a blocking-pool round trip.
    let mut file =
        tokio::io::BufWriter::with_capacity(1 << 20, tokio::fs::File::create(out).await?);
    let mut hash = H::new();
    let mut received: u64 = 0;
    while let Some(chunk) = response.chunk().await.map_err(TransportError::from)? {
        received += chunk.len() as u64;
        if max_bytes.is_some_and(|max| received > max) {
            return Err(DownloadError::TooLarge(max_bytes.unwrap_or_default()));
        }
        hash.update(&chunk);
        file.write_all(&chunk).await?;
        progress(received, total);
    }
    file.flush().await?;
    file.get_ref().sync_all().await?;
    drop(file);
    if let Some(expected) = expected_size
        && received < expected
    {
        return Err(DownloadError::Truncated { received, expected });
    }
    Ok(FileDownload {
        received,
        digest: hash.finalize().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest as _;

    #[tokio::test]
    #[ignore = "needs the network"]
    async fn the_shared_tls_configuration_verifies_a_real_server() {
        let status = client()
            .get("https://example.com/")
            .send()
            .await
            .unwrap()
            .status();
        assert!(status.is_success(), "{status}");
        let bad = client().get("https://self-signed.badssl.com/").send().await;
        assert!(bad.is_err(), "a self-signed certificate was accepted");
    }

    #[test]
    fn clients_share_one_tls_configuration() {
        // Building clients after the first must not reload the root store.
        let _ = client();
        let start = std::time::Instant::now();
        for _ in 0..5 {
            client_builder().build().unwrap();
        }
        let five = start.elapsed();
        assert!(
            five < std::time::Duration::from_millis(100),
            "five clients took {five:?}"
        );
    }

    /// Serves one HTTP response with `body` on a local port.
    async fn serve(body: &'static [u8], length: usize) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 1024];
            let _ = socket.read(&mut request).await;
            let head =
                format!("HTTP/1.1 200 OK\r\ncontent-length: {length}\r\nconnection: close\r\n\r\n");
            let _ = socket.write_all(head.as_bytes()).await;
            let _ = socket.write_all(body).await;
        });
        format!("http://{address}/file")
    }

    #[tokio::test]
    async fn a_download_is_written_hashed_and_reported() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("file");
        let url = serve(b"hello world", 11).await;
        let mut seen = Vec::new();
        let done =
            download_to_file::<sha2::Sha256>(&client(), &url, &out, Some(11), None, |r, t| {
                seen.push((r, t))
            })
            .await
            .unwrap();
        assert_eq!(done.received, 11);
        assert_eq!(done.digest, sha2::Sha256::digest(b"hello world").to_vec());
        assert_eq!(std::fs::read(&out).unwrap(), b"hello world");
        assert_eq!(seen.last(), Some(&(11, Some(11))));
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let url = serve(b"hello world", 11).await;
        let result = download_to_file::<sha2::Sha256>(
            &client(),
            &url,
            &dir.path().join("file"),
            None,
            Some(4),
            |_, _| {},
        )
        .await;
        assert!(matches!(result, Err(DownloadError::TooLarge(4))));
    }

    #[tokio::test]
    async fn an_unresolvable_host_is_offline() {
        let error = client()
            .get("http://kairo-transport-test.invalid/")
            .send()
            .await
            .unwrap_err();
        assert!(matches!(
            TransportError::from(error),
            TransportError::Offline(_)
        ));
    }

    #[tokio::test]
    async fn a_refused_connection_is_not_offline() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let error = client()
            .get(format!("http://127.0.0.1:{port}/"))
            .send()
            .await
            .unwrap_err();
        assert!(matches!(
            TransportError::from(error),
            TransportError::Other(_)
        ));
    }
}
