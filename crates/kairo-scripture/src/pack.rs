//! Downloadable Bible packs, from `local-bible-pack.ts`: fetch the
//! allowlisted registry asset, bound its size, check its pinned SHA-256 and
//! decompress it. File validation is `kairo_store::db::validate_bible_pack`.

use crate::api::BoxFuture;
use crate::versification::{has_custom_pack_versification, pack_totals_for};
use flate2::read::MultiGzDecoder;
use kairo_domain::scripture::translations::{DEFAULT_PACK_TRANSLATION_ID, get_downloadable_pack};
use kairo_domain::scripture::{LocalBibleInstallPhase, LocalBiblePackStatus};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

pub const MAX_COMPRESSED_PACK_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_UNCOMPRESSED_PACK_BYTES: u64 = 24 * 1024 * 1024;
/// The whole request, body included, as `AbortSignal.timeout(120_000)`.
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a pack download failed. Messages match Electron's, which the
/// installer turns into recovery text.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum DownloadError {
    #[error("No downloadable local pack is available for {0}.")]
    NotDownloadable(String),
    #[error("{0} download failed (HTTP {1}).")]
    Http(String, u16),
    #[error("The downloaded {0} pack has an invalid size.")]
    InvalidSize(String),
    #[error("The downloaded {0} pack failed its security check.")]
    SecurityCheck(String),
    #[error("The downloaded {0} pack could not be decompressed.")]
    Decompress(String),
    /// The request outlived [`DOWNLOAD_TIMEOUT`].
    #[error("The download timed out: {0}")]
    Timeout(String),
    /// No connection, DNS failure, reset, TLS failure.
    #[error("The network request failed: {0}")]
    Network(String),
}

/// Download progress, as the `phase`, `receivedBytes` and `totalBytes` of
/// `LocalBibleInstallProgress`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransferProgress {
    pub phase: LocalBibleInstallPhase,
    pub received_bytes: u64,
    pub total_bytes: Option<u64>,
}

/// An HTTP response body read chunk by chunk.
pub trait PackBody: Send {
    fn status(&self) -> u16;
    /// The `content-length` header, when it parses.
    fn content_length(&self) -> Option<u64>;
    /// The next chunk, or `None` at the end.
    fn chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, DownloadError>>;
}

/// `fetch` for pack assets, following redirects. Injectable for tests.
pub trait PackFetcher: Send + Sync {
    fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Box<dyn PackBody>, DownloadError>>;
}

/// The production fetcher: rustls with the system roots and proxy, a 120 s
/// limit on the whole request. Must be polled inside a tokio runtime.
#[derive(Clone)]
pub struct HttpPackFetcher {
    client: reqwest::Client,
}

impl HttpPackFetcher {
    pub fn new() -> Result<Self, DownloadError> {
        let client = kairo_net::transport::client_builder()
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|error| DownloadError::Network(error.to_string()))?;
        Ok(Self { client })
    }
}

fn transport_error(error: reqwest::Error) -> DownloadError {
    if kairo_net::transport::TransportError::from(&error).is_timeout() {
        DownloadError::Timeout(error.to_string())
    } else {
        DownloadError::Network(error.to_string())
    }
}

struct HttpBody(reqwest::Response);

impl PackBody for HttpBody {
    fn status(&self) -> u16 {
        self.0.status().as_u16()
    }

    fn content_length(&self) -> Option<u64> {
        self.0
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)?
            .to_str()
            .ok()?
            .trim()
            .parse()
            .ok()
    }

    fn chunk(&mut self) -> BoxFuture<'_, Result<Option<Vec<u8>>, DownloadError>> {
        Box::pin(async move {
            Ok(self
                .0
                .chunk()
                .await
                .map_err(transport_error)?
                .map(|bytes| bytes.to_vec()))
        })
    }
}

impl PackFetcher for HttpPackFetcher {
    fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Box<dyn PackBody>, DownloadError>> {
        Box::pin(async move {
            let response = self.client.get(url).send().await.map_err(transport_error)?;
            Ok(Box::new(HttpBody(response)) as Box<dyn PackBody>)
        })
    }
}

/// `downloadBiblePack`: the registry asset for `translation_id`, as SQLite
/// bytes. `expected_hash` replaces the pinned hash (tests only).
pub async fn download_bible_pack(
    translation_id: &str,
    fetcher: &dyn PackFetcher,
    expected_hash: Option<&str>,
    on_progress: &mut (dyn FnMut(TransferProgress) + Send),
) -> Result<Vec<u8>, DownloadError> {
    let normalized = translation_id.to_uppercase();
    let Some(pack) = get_downloadable_pack(&normalized) else {
        return Err(DownloadError::NotDownloadable(normalized));
    };
    let invalid_size = || DownloadError::InvalidSize(normalized.clone());
    let mut response = fetcher.get(pack.url).await?;
    let status = response.status();
    if !(200..300).contains(&status) {
        return Err(DownloadError::Http(normalized, status));
    }
    // JavaScript's `Number.isSafeInteger(n) && n > 0`.
    let total_bytes = response
        .content_length()
        .filter(|&n| n > 0 && n < (1 << 53));
    if total_bytes.is_some_and(|n| n > MAX_COMPRESSED_PACK_BYTES) {
        return Err(invalid_size());
    }
    let mut received_bytes = 0;
    let mut progress = |phase, received_bytes| {
        on_progress(TransferProgress {
            phase,
            received_bytes,
            total_bytes,
        })
    };
    progress(LocalBibleInstallPhase::Downloading, received_bytes);
    let mut compressed = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        received_bytes += chunk.len() as u64;
        // Dropping the response cancels the transfer.
        if received_bytes > MAX_COMPRESSED_PACK_BYTES {
            return Err(invalid_size());
        }
        compressed.extend_from_slice(&chunk);
        progress(LocalBibleInstallPhase::Downloading, received_bytes);
    }
    drop(response);
    if compressed.is_empty() {
        return Err(invalid_size());
    }
    progress(LocalBibleInstallPhase::Verifying, received_bytes);
    let digest = hex(&Sha256::digest(&compressed));
    if digest != expected_hash.unwrap_or(pack.sha256) {
        return Err(DownloadError::SecurityCheck(normalized));
    }
    gunzip(&compressed, MAX_UNCOMPRESSED_PACK_BYTES)
        .ok_or_else(|| DownloadError::Decompress(normalized.clone()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// `gunzipSync` with `maxOutputLength`: every member, refusing larger output.
fn gunzip(compressed: &[u8], limit: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    MultiGzDecoder::new(compressed)
        .take(limit + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() as u64 <= limit).then_some(out)
}

/// A unique `kairo-<slug>-<uuid>.db` path in the system temporary folder.
pub fn temporary_pack_path(translation_id: Option<&str>) -> PathBuf {
    let slug: String = translation_id
        .unwrap_or(DEFAULT_PACK_TRANSLATION_ID)
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect();
    let slug = if slug.is_empty() { "bible" } else { &slug };
    std::env::temp_dir().join(format!("kairo-{slug}-{}.db", uuid::Uuid::new_v4()))
}

/// `packStatusFor`: registry packs and translations with their own
/// numbering count as installed only when complete; any other pack once it
/// has a verse.
pub fn pack_status_for(
    translation: &str,
    verse_count: u64,
    chapter_count: u64,
    has_translation: bool,
) -> LocalBiblePackStatus {
    let normalized = translation.to_uppercase();
    let totals = pack_totals_for(&normalized);
    let installed = has_translation
        && if get_downloadable_pack(&normalized).is_some()
            || has_custom_pack_versification(&normalized)
        {
            verse_count == totals.verses && chapter_count == totals.chapters
        } else {
            verse_count > 0
        };
    LocalBiblePackStatus {
        translation: translation.to_owned(),
        verse_count: verse_count as f64,
        chapter_count: chapter_count as f64,
        installed,
        installation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_requires_canonical_counts_for_registry_packs() {
        assert!(pack_status_for("NKJV", 31_102, 1_189, true).installed);
        assert!(!pack_status_for("NKJV", 31_102, 1_188, true).installed);
        assert!(!pack_status_for("NKJV", 31_102, 1_189, false).installed);
        // ampc-pack.test.ts
        assert!(pack_status_for("AMPC", 31_101, 1_189, true).installed);
        assert!(!pack_status_for("AMPC", 31_102, 1_189, true).installed);
        assert!(!pack_status_for("AMPC", 31_101, 1_188, true).installed);
        // A file-imported pack the registry does not know.
        assert!(pack_status_for("ZZT", 1, 1, true).installed);
        assert!(!pack_status_for("ZZT", 0, 0, true).installed);
        assert_eq!(pack_status_for("nkjv", 0, 0, false).translation, "nkjv");
    }

    #[test]
    fn temporary_paths_are_unique_slugged_and_in_the_temp_dir() {
        let first = temporary_pack_path(None);
        let second = temporary_pack_path(None);
        assert_ne!(first, second);
        for path in [&first, &second] {
            assert!(path.starts_with(std::env::temp_dir()));
            let name = path.file_name().unwrap().to_str().unwrap();
            assert!(name.starts_with("kairo-nkjv-") && name.ends_with(".db"));
            assert!(!path.to_str().unwrap().contains(".."));
        }
        let esv = temporary_pack_path(Some("ESV"));
        assert!(
            esv.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("kairo-esv-")
        );
        let odd = temporary_pack_path(Some("../É"));
        assert!(
            odd.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("kairo-bible-")
        );
    }

    #[test]
    fn gunzip_limits_output_and_reads_every_member() {
        use flate2::{Compression, write::GzEncoder};
        use std::io::Write;
        let gz = |data: &[u8]| {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data).unwrap();
            encoder.finish().unwrap()
        };
        let mut two = gz(b"first ");
        two.extend(gz(b"second"));
        assert_eq!(gunzip(&two, 100).unwrap(), b"first second");
        assert_eq!(gunzip(&gz(&[0; 11]), 10), None);
        assert_eq!(gunzip(&gz(&[0; 10]), 10).unwrap().len(), 10);
        assert_eq!(gunzip(b"not gzip", 10), None);
    }
}
