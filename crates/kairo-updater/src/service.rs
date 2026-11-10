//! `updater/index.ts`: check, download on request, install on quit.
//!
//! Nothing downloads until the operator asks. A downloaded update installs
//! when the operator chooses Install (after the shell has released hardware
//! and finished any recording) or on an ordinary quit.

use crate::download::{client, download_verified, get_text};
use crate::errors::{Operation, update_error_message};
use crate::feed::{
    Arch, Platform, ReleaseSource, UpdateFile, feed_file, parse_feed, pick_linux_appimage,
    pick_mac_zip, pick_windows_installer, release_notes_from_atom,
};
use crate::install::{
    LINUX_SWAP_SCRIPT, MAC_SWAP_SCRIPT, MacSigning, bundle_from_executable, can_replace,
    mac_signing, spawn_detached, verify_mac_update, windows_installer_args,
};
use kairo_domain::status::UpdateStatus;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// `KAIRO_UPDATE_SIM=1` in an unpackaged build: a fake feed that drives the
/// real status through every state. Never in a packaged build.
pub fn simulation_requested(packaged: bool) -> bool {
    !packaged && std::env::var("KAIRO_UPDATE_SIM").as_deref() == Ok("1")
}

#[derive(Clone, Debug)]
pub struct UpdaterConfig {
    pub current_version: String,
    /// Only a packaged build updates. An unpackaged run no-ops, as Electron
    /// does without `app-update.yml`.
    pub packaged: bool,
    pub simulate: bool,
    pub platform: Platform,
    pub arch: Arch,
    pub exe: PathBuf,
    pub pid: u32,
    pub temp_dir: PathBuf,
    /// `$APPIMAGE` on Linux; `None` means a package install (deb).
    pub appimage: Option<PathBuf>,
    /// The website named when Kairo cannot replace itself.
    pub web_url: Option<String>,
    pub source: ReleaseSource,
}

/// How long each simulated check and download step takes.
const SIM_STEP: Duration = Duration::from_millis(150);

impl UpdaterConfig {
    /// This process, publishing to [`crate::feed::RELEASE_REPO`].
    pub fn for_this_process(
        current_version: impl Into<String>,
        packaged: bool,
        web_url: Option<String>,
    ) -> Self {
        Self {
            current_version: current_version.into(),
            packaged,
            simulate: simulation_requested(packaged),
            platform: Platform::current(),
            arch: Arch::current(),
            exe: std::env::current_exe().unwrap_or_default(),
            pid: std::process::id(),
            temp_dir: std::env::temp_dir(),
            appimage: std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty()),
            web_url: web_url.filter(|u| !u.is_empty()),
            source: ReleaseSource::github(crate::feed::RELEASE_REPO),
        }
    }
}

/// What `install` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Hardware released, the installer or swap started, quit requested.
    Started,
    /// Not downloaded, not packaged, or it could not proceed (see the status).
    Ignored,
    /// Live output is running; ask the operator, then call again confirmed.
    NeedsConfirmation,
}

#[derive(Clone, Debug)]
enum Staged {
    MacBundle { bundle: PathBuf },
    WindowsInstaller { installer: PathBuf },
    AppImage { file: PathBuf },
}

struct Pending {
    version: String,
    files: Vec<UpdateFile>,
    notes: Option<String>,
}

type Hook = Arc<dyn Fn() -> BoxFuture<()> + Send + Sync>;

struct Inner {
    status: UpdateStatus,
    pending: Option<Pending>,
    staged: Option<(Staged, PathBuf)>,
    installing: bool,
    busy: bool,
    signing: MacSigning,
    before_install: Option<Hook>,
    live_output: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    quit: Option<Arc<dyn Fn() + Send + Sync>>,
}

#[derive(Clone)]
pub struct UpdaterService {
    config: Arc<UpdaterConfig>,
    http: reqwest::Client,
    inner: Arc<Mutex<Inner>>,
    listeners: kairo_core::Listeners<UpdateStatus>,
}

use kairo_domain::js::lock;

/// Simulator only: the running version with its patch number bumped, as
/// `Number(patch) + 1` gives it.
fn fake_next_version(current: &str) -> String {
    let mut parts = current.split('.');
    let major = parts.next().filter(|s| !s.is_empty()).unwrap_or("1");
    let minor = parts.next().unwrap_or("0");
    let patch = parts.next().unwrap_or("0");
    let next = patch
        .trim()
        .parse::<f64>()
        .map_or_else(|_| "NaN".to_owned(), |p| format!("{}", p + 1.0));
    format!("{major}.{minor}.{next}")
}

/// True when `feed` is newer than `current` (semver; a leading `v` allowed).
pub fn is_newer(feed: &str, current: &str) -> Result<bool, String> {
    let parse = |v: &str| {
        semver::Version::parse(v.trim().trim_start_matches('v'))
            .map_err(|e| format!("Invalid version {v}: {e}"))
    };
    Ok(parse(feed)? > parse(current)?)
}

impl UpdaterService {
    pub fn new(config: UpdaterConfig) -> Self {
        Self {
            config: Arc::new(config),
            http: client(),
            inner: Arc::new(Mutex::new(Inner {
                status: UpdateStatus::Idle {
                    current_version: None,
                },
                pending: None,
                staged: None,
                installing: false,
                busy: false,
                signing: MacSigning::Adhoc,
                before_install: None,
                live_output: None,
                quit: None,
            })),
            listeners: kairo_core::Listeners::default(),
        }
    }

    fn enabled(&self) -> bool {
        self.config.packaged
    }

    pub fn on_change(&self, listener: impl Fn(&UpdateStatus) + Send + Sync + 'static) {
        self.listeners.add(listener);
    }

    /// `setBeforeInstall`: release hardware and finish recording.
    pub fn set_before_install<F, Fut>(&self, hook: F)
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        lock(&self.inner).before_install = Some(Arc::new(move || Box::pin(hook())));
    }

    /// Reports whether live output is running, so install can ask first.
    pub fn set_live_output(&self, live: impl Fn() -> bool + Send + Sync + 'static) {
        lock(&self.inner).live_output = Some(Arc::new(live));
    }

    /// The app's quit, called once the install has been handed over.
    pub fn set_quit(&self, quit: impl Fn() + Send + Sync + 'static) {
        lock(&self.inner).quit = Some(Arc::new(quit));
    }

    fn set(&self, status: UpdateStatus) {
        lock(&self.inner).status = status.clone();
        // A listener that panics must not stop the updater.
        self.listeners.notify(&status);
    }

    /// The running version is shown only while nothing is pending.
    pub fn status(&self) -> UpdateStatus {
        match lock(&self.inner).status.clone() {
            UpdateStatus::Idle { .. } => UpdateStatus::Idle {
                current_version: Some(self.current_version()),
            },
            other => other,
        }
    }

    fn current_version(&self) -> String {
        self.config.current_version.clone()
    }

    /// Called once on start: cleans stale downloads, reads the Mac
    /// signature, then runs the first check. Never downloads.
    pub async fn init(&self) {
        if self.config.simulate {
            tracing::warn!(
                "[Updater] SIMULATED feed (KAIRO_UPDATE_SIM=1), no real update will be installed"
            );
            self.check().await;
            return;
        }
        if !self.enabled() {
            tracing::info!("[Updater] disabled (not a packaged build)");
            return;
        }
        self.remove_stale_downloads().await;
        if self.config.platform == Platform::Mac
            && let Some(bundle) = bundle_from_executable(&self.config.exe)
        {
            let signing = mac_signing(&bundle).await;
            tracing::info!(?signing, "[Updater] macOS signature read");
            lock(&self.inner).signing = signing;
        }
        self.check().await;
    }

    /// An interrupted download from an earlier run leaves its folder behind.
    async fn remove_stale_downloads(&self) {
        let Ok(mut entries) = tokio::fs::read_dir(&self.config.temp_dir).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("kairo-update-")
            {
                let _ = tokio::fs::remove_dir_all(entry.path()).await;
            }
        }
    }

    /// Manual "Check for Updates". Resolves with the status it settled on.
    /// While a download is running or staged, the status stays as it is.
    pub async fn check(&self) -> UpdateStatus {
        if self.config.simulate {
            self.set(UpdateStatus::Checking);
            tokio::time::sleep(SIM_STEP * 6).await;
            self.set(UpdateStatus::Available {
                version: fake_next_version(&self.config.current_version),
                notes: Some("Simulated release notes.".into()),
            });
            return self.status();
        }
        if !self.enabled() {
            return self.status();
        }
        {
            let inner = lock(&self.inner);
            if inner.busy
                || inner.installing
                || matches!(
                    inner.status,
                    UpdateStatus::Downloading { .. } | UpdateStatus::Downloaded { .. }
                )
            {
                return inner.status.clone();
            }
        }
        self.set(UpdateStatus::Checking);
        match self.fetch_feed().await {
            Ok(Some(pending)) => {
                tracing::info!(version = pending.version, "[Updater] update available");
                let status = UpdateStatus::Available {
                    version: pending.version.clone(),
                    notes: pending.notes.clone(),
                };
                lock(&self.inner).pending = Some(pending);
                self.set(status);
            }
            Ok(None) => self.set(UpdateStatus::Idle {
                current_version: Some(self.current_version()),
            }),
            Err(error) => {
                // A dead network or a repository with no releases is normal.
                tracing::warn!(error, "[Updater] check failed");
                self.set(UpdateStatus::Error {
                    message: update_error_message(&error, Operation::Check),
                });
            }
        }
        self.status()
    }

    async fn fetch_feed(&self) -> Result<Option<Pending>, String> {
        let url = self
            .config
            .source
            .feed_url(feed_file(self.config.platform, self.config.arch));
        let feed = parse_feed(&get_text(&self.http, &url).await?).map_err(|e| e.to_string())?;
        if !is_newer(&feed.version, &self.config.current_version)? {
            return Ok(None);
        }
        // Notes are a nicety: a missing or unreadable Atom feed is no error.
        let notes = match feed.release_notes.clone() {
            Some(notes) => Some(notes),
            None => get_text(&self.http, &self.config.source.atom_url())
                .await
                .ok()
                .and_then(|atom| release_notes_from_atom(&atom, &feed.version)),
        };
        Ok(Some(Pending {
            version: feed.version,
            files: feed.files,
            notes,
        }))
    }

    fn no_file_message(&self) -> &'static str {
        match self.config.platform {
            Platform::Mac => "This release has no update file for this Mac.",
            _ => "This release has no update file for this computer.",
        }
    }

    fn site(&self) -> String {
        self.config
            .web_url
            .clone()
            .unwrap_or_else(|| "the Kairo website".into())
    }

    /// Downloads the pending update; a no-op unless one is available.
    pub async fn download(&self) -> UpdateStatus {
        if self.config.simulate {
            let UpdateStatus::Available { version, notes } = self.status() else {
                return self.status();
            };
            for percent in (0..=100).step_by(5) {
                self.set(UpdateStatus::Downloading {
                    version: Some(version.clone()),
                    notes: notes.clone(),
                    percent: Some(f64::from(percent)),
                });
                tokio::time::sleep(SIM_STEP).await;
            }
            self.set(UpdateStatus::Downloaded { version, notes });
            return self.status();
        }
        let (version, notes, files) = {
            let mut inner = lock(&self.inner);
            let UpdateStatus::Available { version, notes } = inner.status.clone() else {
                return inner.status.clone();
            };
            if !self.enabled() || inner.busy {
                return inner.status.clone();
            }
            let files = inner
                .pending
                .as_ref()
                .map(|p| p.files.clone())
                .unwrap_or_default();
            inner.busy = true;
            (version, notes, files)
        };
        self.set(UpdateStatus::Downloading {
            version: Some(version.clone()),
            notes: notes.clone(),
            percent: Some(0.0),
        });
        let result = self.download_and_stage(&version, &notes, &files).await;
        lock(&self.inner).busy = false;
        match result {
            Ok(()) => {
                tracing::info!(version, "[Updater] update staged");
                self.set(UpdateStatus::Downloaded { version, notes });
            }
            Err(Failure::Shown(message)) => self.set(UpdateStatus::Error { message }),
            Err(Failure::Raw(error)) => {
                tracing::warn!(error, "[Updater] download failed");
                self.set(UpdateStatus::Error {
                    message: update_error_message(&error, Operation::Download),
                });
            }
        }
        self.status()
    }

    async fn download_and_stage(
        &self,
        version: &str,
        notes: &Option<String>,
        files: &[UpdateFile],
    ) -> Result<(), Failure> {
        let config = &self.config;
        if config.platform == Platform::Linux && config.appimage.is_none() {
            return Err(Failure::Shown(format!(
                "This Kairo was installed from a package. Download the new version from {} and install it.",
                self.site()
            )));
        }
        let file = match config.platform {
            Platform::Mac => pick_mac_zip(files, config.arch),
            Platform::Windows => pick_windows_installer(files, config.arch),
            Platform::Linux => pick_linux_appimage(files, config.arch),
        }
        .ok_or_else(|| Failure::Shown(self.no_file_message().into()))?
        .clone();
        let url = config
            .source
            .asset_url(version, &file.url)
            .map_err(Failure::Raw)?;
        let dir = config.temp_dir.join(format!(
            "kairo-update-{}-{}",
            config.pid,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        let staged = async {
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| e.to_string())?;
            let name = match config.platform {
                Platform::Mac => "update.zip".to_owned(),
                _ => Path::new(&file.url)
                    .file_name()
                    .map_or_else(|| "update".into(), |n| n.to_string_lossy().into_owned()),
            };
            let target = dir.join(name);
            let this = self.clone();
            let (version, notes) = (version.to_owned(), notes.clone());
            download_verified(
                &self.http,
                &url,
                &target,
                &file.sha512,
                file.size,
                move |percent| {
                    this.set(UpdateStatus::Downloading {
                        version: Some(version.clone()),
                        notes: notes.clone(),
                        percent: Some(f64::from(percent)),
                    });
                },
            )
            .await?;
            match config.platform {
                Platform::Mac => {
                    let current = bundle_from_executable(&config.exe)
                        .ok_or("Could not find the installed Kairo.")?;
                    let signing = lock(&self.inner).signing.clone();
                    let bundle =
                        verify_mac_update(&target, &dir.join("app"), &current, &signing).await?;
                    Ok(Staged::MacBundle { bundle })
                }
                Platform::Windows => Ok(Staged::WindowsInstaller { installer: target }),
                Platform::Linux => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        tokio::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
                            .await
                            .map_err(|e| e.to_string())?;
                    }
                    Ok(Staged::AppImage { file: target })
                }
            }
        }
        .await;
        match staged {
            Ok(staged) => {
                lock(&self.inner).staged = Some((staged, dir));
                Ok(())
            }
            Err(error) => {
                let _ = tokio::fs::remove_dir_all(&dir).await;
                Err(Failure::Raw(error))
            }
        }
    }

    /// Quits and installs. Only once downloaded: a stray call must never tear
    /// down a live service. With live output running, the operator confirms
    /// first (`confirmed`).
    pub async fn install(&self, confirmed: bool) -> InstallOutcome {
        if self.config.simulate {
            tracing::warn!("[Updater] SIMULATED install, would quit and install now");
            self.set(UpdateStatus::Idle {
                current_version: Some(fake_next_version(&self.config.current_version)),
            });
            return InstallOutcome::Ignored;
        }
        let (live, hook, quit) = {
            let inner = lock(&self.inner);
            if !self.enabled()
                || inner.installing
                || !matches!(inner.status, UpdateStatus::Downloaded { .. })
            {
                return InstallOutcome::Ignored;
            }
            (
                inner.live_output.clone(),
                inner.before_install.clone(),
                inner.quit.clone(),
            )
        };
        if !confirmed && live.is_some_and(|live| live()) {
            return InstallOutcome::NeedsConfirmation;
        }
        // Check the install can happen before any hardware is released.
        if !self.can_install() {
            return InstallOutcome::Ignored;
        }
        tracing::info!("[Updater] quitting to install");
        if let Some(hook) = hook {
            hook().await;
        }
        if !self.hand_over(true) {
            return InstallOutcome::Ignored;
        }
        if let Some(quit) = quit {
            quit();
        }
        InstallOutcome::Started
    }

    /// The app is quitting without Install: install a downloaded update now,
    /// silently and without reopening, as electron-updater's
    /// `autoInstallOnAppQuit` does. Synchronous, for the quit path.
    pub fn on_quit(&self) {
        let ready = {
            let inner = lock(&self.inner);
            inner.staged.is_some() && !inner.installing
        };
        if ready && self.enabled() && !self.config.simulate && self.can_install() {
            self.hand_over(false);
        }
    }

    fn installed_target(&self, staged: &Staged) -> Option<PathBuf> {
        match staged {
            Staged::MacBundle { .. } => bundle_from_executable(&self.config.exe),
            Staged::AppImage { .. } => self.config.appimage.clone(),
            Staged::WindowsInstaller { .. } => Some(self.config.exe.clone()),
        }
    }

    /// `false` with an error status when Kairo cannot replace itself where
    /// it is installed.
    fn can_install(&self) -> bool {
        let Some((staged, _)) = lock(&self.inner).staged.clone() else {
            return false;
        };
        if matches!(staged, Staged::WindowsInstaller { .. }) {
            // The installer handles permissions itself.
            return true;
        }
        let Some(current) = self.installed_target(&staged) else {
            return false;
        };
        if can_replace(&current) {
            return true;
        }
        let folder = current
            .parent()
            .unwrap_or(Path::new(""))
            .display()
            .to_string();
        let message = match staged {
            Staged::MacBundle { .. } => format!(
                "Kairo can't update itself in {folder}. Download the new version from {} and drag it into Applications.",
                self.site()
            ),
            _ => format!(
                "Kairo can't update itself in {folder}. Download the new version from {}.",
                self.site()
            ),
        };
        self.set(UpdateStatus::Error { message });
        false
    }

    /// Starts the detached swap or the installer; it waits for this process
    /// to exit.
    fn hand_over(&self, explicit: bool) -> bool {
        let Some((staged, dir)) = lock(&self.inner).staged.clone() else {
            return false;
        };
        let relaunch = if explicit { "1" } else { "0" };
        let pid = self.config.pid.to_string();
        let started = match &staged {
            Staged::MacBundle { bundle } => self.installed_target(&staged).map(|current| {
                let script = dir.join("swap.sh");
                std::fs::write(&script, MAC_SWAP_SCRIPT).and_then(|()| {
                    spawn_detached(
                        "/bin/bash",
                        &script,
                        &[
                            pid.as_ref(),
                            current.as_os_str(),
                            bundle.as_os_str(),
                            relaunch.as_ref(),
                        ],
                    )
                })
            }),
            Staged::AppImage { file } => self.installed_target(&staged).map(|current| {
                let script = dir.join("swap.sh");
                std::fs::write(&script, LINUX_SWAP_SCRIPT).and_then(|()| {
                    spawn_detached(
                        "/bin/sh",
                        &script,
                        &[
                            pid.as_ref(),
                            current.as_os_str(),
                            file.as_os_str(),
                            relaunch.as_ref(),
                        ],
                    )
                })
            }),
            Staged::WindowsInstaller { installer } => Some(spawn_installer(installer, explicit)),
        };
        match started {
            Some(Ok(())) => {
                lock(&self.inner).installing = true;
                tracing::info!(explicit, "[Updater] install handed over");
                true
            }
            Some(Err(error)) => {
                tracing::warn!(%error, "[Updater] could not start the install");
                self.set(UpdateStatus::Error {
                    message: update_error_message(&error.to_string(), Operation::Download),
                });
                false
            }
            None => false,
        }
    }
}

enum Failure {
    /// Already an operator message.
    Shown(String),
    /// Diagnostic text, logged; the operator sees `update_error_message`.
    Raw(String),
}

fn spawn_installer(installer: &Path, explicit: bool) -> std::io::Result<()> {
    let mut command = std::process::Command::new(installer);
    command
        .args(windows_installer_args(explicit))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_next_version_bumps_the_patch() {
        assert_eq!(fake_next_version("1.0.33"), "1.0.34");
        assert_eq!(fake_next_version("2"), "2.0.1");
        assert_eq!(fake_next_version("1.2.x"), "1.2.NaN");
    }

    #[test]
    fn newer_versions() {
        assert!(is_newer("1.0.34", "1.0.33").unwrap());
        assert!(!is_newer("1.0.33", "1.0.33").unwrap());
        assert!(!is_newer("1.0.32", "1.0.33").unwrap(), "never a downgrade");
        assert!(is_newer("v1.1.0", "1.0.99").unwrap());
        assert!(!is_newer("1.1.0-beta.1", "1.1.0").unwrap());
        assert!(is_newer("nonsense", "1.0.0").is_err());
    }
}
