//! Fetching the feed and the update file through a client that honours the
//! OS proxy settings, as `electron.net` does (`reqwest` with `system-proxy`).

use base64::Engine;
use sha2::Sha512;
use std::path::Path;

/// A failure, phrased so the operator message can tell a network problem
/// from anything else (see [`crate::update_error_message`]).
pub(crate) fn describe(error: &reqwest::Error) -> String {
    match kairo_net::transport::TransportError::from(error) {
        kairo_net::transport::TransportError::Status { .. } => error.to_string(),
        other => other.to_string(),
    }
}

pub fn client() -> reqwest::Client {
    // The shared timeouts: a stalled transfer fails instead of hanging the
    // download forever.
    kairo_net::transport::client_builder()
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
    use kairo_net::transport::{DownloadError, download_to_file};
    let mut shown: Option<u8> = None;
    let done =
        download_to_file::<Sha512>(client, url, out, expected_size, None, |received, total| {
            if let Some(total) = total.filter(|t| *t > 0) {
                let percent = ((received as f64 / total as f64) * 100.0).floor().min(99.0) as u8;
                if shown != Some(percent) {
                    shown = Some(percent);
                    progress(percent);
                }
            }
        })
        .await
        .map_err(|error| match error {
            DownloadError::Status(status) => format!("Download failed (HTTP {status})"),
            other => other.to_string(),
        })?;
    let digest = base64::engine::general_purpose::STANDARD.encode(&done.digest);
    if digest != sha512 {
        return Err(
            "The update did not match its published checksum, so it was not installed.".into(),
        );
    }
    Ok(())
}
