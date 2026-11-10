//! The slide converter Kairo downloads on demand, from `slide-converter.ts`:
//! LibreOffice, unpacked into Kairo's own folder and run headless. Never
//! installed system-wide, needs no admin password, and only fetched when a
//! PowerPoint has nothing better to convert it.

use crate::process::run_ok;
use kairo_domain::documents::{PowerpointConverterId, SlideConverterState, SlideConverterStatus};
use sha2::Sha256;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, broadcast};
use tokio_util::sync::CancellationToken;

/// Pinned build. Bump the version, URLs and checksums together (TDF
/// publishes a .sha256 next to each file).
pub const SLIDE_CONVERTER_VERSION: &str = "26.2.7";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildKind {
    Dmg,
    Msi,
}

impl BuildKind {
    fn extension(self) -> &'static str {
        match self {
            Self::Dmg => "dmg",
            Self::Msi => "msi",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Build {
    pub url: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub kind: BuildKind,
}

/// The download for `platform`/`arch`, as Node names them.
pub fn build_for(platform: &str, arch: &str) -> Option<Build> {
    let base = format!(
        "https://download.documentfoundation.org/libreoffice/stable/{SLIDE_CONVERTER_VERSION}"
    );
    let v = SLIDE_CONVERTER_VERSION;
    let (path, sha256, size_bytes, kind) = match (platform, arch) {
        ("darwin", "arm64") => (
            format!("mac/aarch64/LibreOffice_{v}_MacOS_aarch64.dmg"),
            "58601808ae2b919c82dc7c4d6a26389de0e167f344ca48e5b6c9189218eb95ae",
            297_813_809,
            BuildKind::Dmg,
        ),
        ("darwin", "x64") => (
            format!("mac/x86_64/LibreOffice_{v}_MacOS_x86-64.dmg"),
            "26a830205c617cc624b2a83783ef46434a4d6c337c3a873f79c43012171f83de",
            308_070_986,
            BuildKind::Dmg,
        ),
        ("win32", "x64") => (
            format!("win/x86_64/LibreOffice_{v}_Win_x86-64.msi"),
            "480fecf759fc6393bd614fbf43753eb95a8198fc1a065d41b727e3b664cd1e59",
            373_121_024,
            BuildKind::Msi,
        ),
        _ => return None,
    };
    Some(Build {
        url: format!("{base}/{path}"),
        sha256: sha256.into(),
        size_bytes,
        kind,
    })
}

/// This machine's build.
pub fn current_build() -> Option<Build> {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    build_for(crate::office_apps::node_platform(), arch)
}

/// Why an install failed, in words for the operator.
#[derive(Debug)]
enum InstallError {
    Checksum,
    Network(String),
    Other(String),
}

pub fn friendly_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if message == "checksum" {
        "The download was damaged. Try again."
    } else if message.contains("ENOSPC") || lower.contains("no space left") {
        "There isn’t enough free disk space. Free up about 1 GB and try again."
    } else if [
        "fetch failed",
        "enotfound",
        "econnreset",
        "etimedout",
        "network",
        "download failed",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        "The download stopped. Check the internet connection and try again."
    } else {
        "The converter couldn’t be set up. Try again."
    }
}

impl InstallError {
    fn friendly(&self) -> &'static str {
        match self {
            Self::Checksum => friendly_error("checksum"),
            Self::Network(m) => friendly_error(&format!("network: {m}")),
            Self::Other(m) => friendly_error(m),
        }
    }
}

impl From<std::io::Error> for InstallError {
    fn from(e: std::io::Error) -> Self {
        if e.raw_os_error() == Some(28) {
            Self::Other("ENOSPC".into())
        } else {
            Self::Other(e.to_string())
        }
    }
}

struct State {
    state: SlideConverterState,
    received: u64,
    error: Option<String>,
    cancel: Option<CancellationToken>,
}

type DetectOthers = Box<dyn Fn() -> Option<PowerpointConverterId> + Send + Sync>;

/// The downloaded converter under `<userData>/slide-converter`.
pub struct SlideConverter {
    root: PathBuf,
    build: Option<Build>,
    http: reqwest::Client,
    state: Mutex<State>,
    installing: AsyncMutex<()>,
    changed: broadcast::Sender<SlideConverterStatus>,
    detect_others: Mutex<DetectOthers>,
}

impl SlideConverter {
    pub fn new(user_data: &Path, http: reqwest::Client) -> Arc<Self> {
        Self::with_build(user_data, http, current_build())
    }

    /// For tests: a converter fetched from `build` instead of TDF.
    pub fn with_build(user_data: &Path, http: reqwest::Client, build: Option<Build>) -> Arc<Self> {
        Arc::new(Self {
            root: user_data.join("slide-converter"),
            build,
            http,
            state: Mutex::new(State {
                state: SlideConverterState::Missing,
                received: 0,
                error: None,
                cancel: None,
            }),
            installing: AsyncMutex::new(()),
            changed: broadcast::channel(16).0,
            detect_others: Mutex::new(Box::new(|| None)),
        })
    }

    /// Set by the documents service so status can say when another app
    /// already converts.
    pub fn set_detect_others(
        &self,
        detect: impl Fn() -> Option<PowerpointConverterId> + Send + Sync + 'static,
    ) {
        *self.detect_others.lock().unwrap_or_else(|e| e.into_inner()) = Box::new(detect);
    }

    fn install_dir(&self) -> PathBuf {
        self.root.join(SLIDE_CONVERTER_VERSION)
    }

    /// The `soffice` binary of the downloaded converter, when installed.
    pub fn binary(&self) -> Option<PathBuf> {
        let dir = self.install_dir();
        let candidates = if cfg!(target_os = "macos") {
            vec![dir.join("LibreOffice.app/Contents/MacOS/soffice")]
        } else {
            vec![
                dir.join("program/soffice.exe"),
                dir.join("LibreOffice/program/soffice.exe"),
            ]
        };
        candidates.into_iter().find(|p| p.exists())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SlideConverterStatus> {
        self.changed.subscribe()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn status(&self) -> SlideConverterStatus {
        let (mut state, received, error) = {
            let s = self.lock();
            (s.state, s.received, s.error.clone())
        };
        if self.build.is_none() {
            state = SlideConverterState::Unsupported;
        } else if !matches!(
            state,
            SlideConverterState::Downloading | SlideConverterState::Installing
        ) {
            state = if self.binary().is_some() {
                SlideConverterState::Ready
            } else if state == SlideConverterState::Error {
                SlideConverterState::Error
            } else {
                SlideConverterState::Missing
            };
        }
        let other = (self.detect_others.lock().unwrap_or_else(|e| e.into_inner()))();
        SlideConverterStatus {
            state,
            version: SLIDE_CONVERTER_VERSION.into(),
            size_bytes: self.build.as_ref().map_or(0.0, |b| b.size_bytes as f64),
            received_bytes: received as f64,
            error: (state == SlideConverterState::Error)
                .then_some(error)
                .flatten(),
            other_converter: other,
        }
    }

    fn emit(&self) {
        let _ = self.changed.send(self.status());
    }

    fn set(&self, state: SlideConverterState) {
        self.lock().state = state;
        self.emit();
    }

    pub fn cancel(&self) {
        if let Some(token) = &self.lock().cancel {
            token.cancel();
        }
    }

    pub async fn remove(&self) -> SlideConverterStatus {
        self.cancel();
        let _wait = self.installing.lock().await;
        let _ = tokio::fs::remove_dir_all(&self.root).await;
        {
            let mut s = self.lock();
            s.state = SlideConverterState::Missing;
            s.error = None;
            s.received = 0;
        }
        self.emit();
        self.status()
    }

    /// Download, verify and unpack. Concurrent calls share one download:
    /// later callers wait for it and see its result.
    pub async fn install(self: &Arc<Self>) -> SlideConverterStatus {
        let Ok(guard) = self.installing.try_lock() else {
            let _wait = self.installing.lock().await;
            return self.status();
        };
        let Some(build) = self.build.clone() else {
            return self.status();
        };
        if self.binary().is_some() {
            self.set(SlideConverterState::Ready);
            return self.status();
        }
        let token = CancellationToken::new();
        {
            let mut s = self.lock();
            s.cancel = Some(token.clone());
            s.state = SlideConverterState::Downloading;
            s.received = 0;
            s.error = None;
        }
        self.emit();
        let download = self
            .root
            .join(format!("download.{}", build.kind.extension()));
        let staging = self.root.join("staging");
        let result = self.run_install(&build, &download, &staging, &token).await;
        let state = match result {
            Ok(()) => {
                tracing::info!(
                    version = SLIDE_CONVERTER_VERSION,
                    "[SlideConverter] Installed"
                );
                SlideConverterState::Ready
            }
            Err(error) => {
                let cancelled = token.is_cancelled();
                if !cancelled {
                    tracing::warn!(?error, "[SlideConverter] Install failed");
                }
                self.lock().error = (!cancelled).then(|| error.friendly().to_owned());
                let _ = tokio::fs::remove_dir_all(&staging).await;
                if cancelled {
                    SlideConverterState::Missing
                } else {
                    SlideConverterState::Error
                }
            }
        };
        let _ = tokio::fs::remove_file(&download).await;
        {
            let mut s = self.lock();
            s.state = state;
            s.cancel = None;
            s.received = if state == SlideConverterState::Ready {
                build.size_bytes
            } else {
                0
            };
        }
        self.emit();
        drop(guard);
        self.status()
    }

    async fn run_install(
        self: &Arc<Self>,
        build: &Build,
        download: &Path,
        staging: &Path,
        token: &CancellationToken,
    ) -> Result<(), InstallError> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::select! {
            result = self.download(build, download) => result?,
            () = token.cancelled() => return Err(InstallError::Other("cancelled".into())),
        }
        self.set(SlideConverterState::Installing);
        let _ = tokio::fs::remove_dir_all(staging).await;
        tokio::fs::create_dir_all(staging).await?;
        let this = self.clone();
        let (file, into, kind) = (download.to_path_buf(), staging.to_path_buf(), build.kind);
        tokio::task::spawn_blocking(move || match kind {
            BuildKind::Dmg => this.unpack_dmg(&file, &into),
            BuildKind::Msi => unpack_msi(&file, &into),
        })
        .await
        .map_err(|e| InstallError::Other(e.to_string()))??;
        // Old versions go once the new one is in place.
        let mut entries = tokio::fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().into_owned();
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() == 3
                && parts
                    .iter()
                    .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            {
                let _ = tokio::fs::remove_dir_all(entry.path()).await;
            }
        }
        tokio::fs::rename(staging, self.install_dir()).await?;
        if self.binary().is_none() {
            return Err(InstallError::Other(
                "The converter was downloaded but could not be unpacked.".into(),
            ));
        }
        Ok(())
    }

    async fn download(&self, build: &Build, dest: &Path) -> Result<(), InstallError> {
        use kairo_net::transport::{DownloadError, TransportError, download_to_file};
        let mut last_emit = Instant::now();
        let done =
            download_to_file::<Sha256>(&self.http, &build.url, dest, None, None, |received, _| {
                self.lock().received = received;
                if last_emit.elapsed() > Duration::from_millis(250) {
                    last_emit = Instant::now();
                    self.emit();
                }
            })
            .await
            .map_err(|error| match error {
                DownloadError::Status(status) => {
                    InstallError::Network(format!("Download failed ({status})."))
                }
                DownloadError::Io(error) => InstallError::from(error),
                DownloadError::Transport(
                    TransportError::Timeout(m)
                    | TransportError::Offline(m)
                    | TransportError::Other(m),
                ) => InstallError::Network(m),
                other => InstallError::Network(other.to_string()),
            })?;
        let digest: String = done.digest.iter().map(|b| format!("{b:02x}")).collect();
        if digest != build.sha256 {
            return Err(InstallError::Checksum);
        }
        Ok(())
    }

    fn unpack_dmg(&self, dmg: &Path, staging: &Path) -> Result<(), InstallError> {
        let mount = self.root.join("mount");
        std::fs::create_dir_all(&mount)?;
        run_ok(
            Command::new("hdiutil")
                .args(["attach"])
                .arg(dmg)
                .args(["-nobrowse", "-readonly", "-noautoopen", "-mountpoint"])
                .arg(&mount),
            Duration::from_secs(120),
        )?;
        // ditto keeps the code signature and symlinks intact.
        let copied = run_ok(
            Command::new("ditto")
                .arg(mount.join("LibreOffice.app"))
                .arg(staging.join("LibreOffice.app")),
            Duration::from_secs(600),
        );
        let _ = run_ok(
            Command::new("hdiutil")
                .arg("detach")
                .arg(&mount)
                .arg("-force"),
            Duration::from_secs(60),
        );
        let _ = std::fs::remove_dir_all(&mount);
        copied?;
        Ok(())
    }
}

/// An administrative install only unpacks files: no admin rights, no
/// registry, no Start menu entry.
fn unpack_msi(msi: &Path, staging: &Path) -> Result<(), InstallError> {
    let mut target = std::ffi::OsString::from("TARGETDIR=");
    target.push(staging);
    run_ok(
        Command::new("msiexec")
            .arg("/a")
            .arg(msi)
            .arg("/qn")
            .arg(target),
        Duration::from_secs(900),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_per_platform() {
        let arm = build_for("darwin", "arm64").unwrap();
        assert_eq!(
            arm.url,
            "https://download.documentfoundation.org/libreoffice/stable/26.2.7/mac/aarch64/LibreOffice_26.2.7_MacOS_aarch64.dmg"
        );
        assert_eq!((arm.size_bytes, arm.kind), (297_813_809, BuildKind::Dmg));
        assert_eq!(build_for("win32", "x64").unwrap().kind, BuildKind::Msi);
        assert_eq!(build_for("linux", "x64"), None);
        assert_eq!(build_for("win32", "arm64"), None);
    }

    #[test]
    fn friendly_errors() {
        assert_eq!(
            friendly_error("checksum"),
            "The download was damaged. Try again."
        );
        assert_eq!(
            friendly_error("write ENOSPC"),
            "There isn’t enough free disk space. Free up about 1 GB and try again."
        );
        assert_eq!(
            friendly_error("Download failed (404)."),
            "The download stopped. Check the internet connection and try again."
        );
        assert_eq!(
            friendly_error("boom"),
            "The converter couldn’t be set up. Try again."
        );
    }
}
