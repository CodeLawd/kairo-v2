//! Fetching the feed and the update file through a client that honours the
//! OS proxy settings, as `electron.net` does (`reqwest` with `system-proxy`).

use base64::Engine;
use futures_util::StreamExt;
use sha2::{Digest, Sha512};
use std::path::Path;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// A failure, phrased so the operator message can tell a network problem
/// from anything else (see [`crate::update_error_message`]).
pub(crate) fn describe(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        format!("timed out: {error}")
    } else if error.is_status() {
        error.to_string()
    } else {
        format!("network error: {error}")
    }
}

pub fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        // A stalled transfer fails instead of hanging the download forever.
        .read_timeout(Duration::from_secs(60))
        .user_agent(concat!("Kairo-Updater/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
}

pub(crate) async fn get_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client.get(url).send().await.map_err(|e| describe(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("HTTP {} for {url}", status.as_u16()));
    }
    response.text().await.map_err(|e| describe(&e))
}

/// Downloads `url` to `out`, hashing as it goes. `progress` receives whole
/// percentages, each once, capped at 99 until the checksum is confirmed.
/// Fails when the SHA-512 (base64) does not match `sha512`.
pub async fn download_verified(
    client: &reqwest::Client,
    url: &str,
    out: &Path,
    sha512: &str,
    expected_size: Option<u64>,
    mut progress: impl FnMut(u8),
) -> Result<(), String> {
    let response = client.get(url).send().await.map_err(|e| describe(&e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Download failed (HTTP {})", status.as_u16()));
    }
    let total = response
        .content_length()
        .filter(|n| *n > 0)
        .or(expected_size)
        .unwrap_or(0);
    // Buffered: each unbuffered tokio write is a blocking-pool round trip.
    let mut file = tokio::io::BufWriter::with_capacity(
        1 << 20,
        tokio::fs::File::create(out)
            .await
            .map_err(|e| e.to_string())?,
    );
    let mut hash = Sha512::new();
    let mut received: u64 = 0;
    let mut shown: Option<u8> = None;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| describe(&e))?;
        hash.update(&chunk);
        received += chunk.len() as u64;
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        if total > 0 {
            let percent = ((received as f64 / total as f64) * 100.0).floor().min(99.0) as u8;
            if shown != Some(percent) {
                shown = Some(percent);
                progress(percent);
            }
        }
    }
    file.flush().await.map_err(|e| e.to_string())?;
    file.get_ref().sync_all().await.map_err(|e| e.to_string())?;
    drop(file);
    // A transfer that ended early without an error is a broken connection.
    if let Some(size) = expected_size
        && received < size
    {
        return Err(format!(
            "network error: received {received} of {size} bytes"
        ));
    }
    let digest = base64::engine::general_purpose::STANDARD.encode(hash.finalize());
    if digest != sha512 {
        return Err(
            "The update did not match its published checksum, so it was not installed.".into(),
        );
    }
    Ok(())
}
