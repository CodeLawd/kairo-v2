//! Where Kairo reads and writes: every `app.getPath` the Electron app uses,
//! computed so the native app opens the same folders.
//!
//! Sources: `src/main/index.ts` (`app.setName("Kairo")`), `src/main/dev-profile.ts`
//! (a dev build's separate profile), `src/main/db/legacy-store.ts` (renamed
//! store files) and `src/main/services/workspace/index.ts` (Documents falling
//! back to home).
//!
//! [`compute`] is pure: it takes the operating system's answers as
//! [`PathInputs`] and builds every path with that system's separator, so the
//! layout for each OS is tested on any machine. [`detect`] fills the inputs
//! from the running system. Nothing here creates or writes a file.

use kairo_domain::brand::PRODUCT_NAME;
use kairo_domain::workspace::WORKSPACE_FOLDER_NAME;
use std::fmt;
use std::path::{Path, PathBuf};

/// `DEV_PROFILE_NAME`: a dev build's own data folder.
pub const DEV_PROFILE_NAME: &str = "Kairo Dev";

/// The environment variable that puts a dev build back on the installed
/// app's profile (`KAIRO_SHARED_PROFILE=1`).
pub const SHARED_PROFILE_ENV: &str = "KAIRO_SHARED_PROFILE";

/// The native log file, beside electron-log's `main.log` in the same folder.
/// A separate name keeps the two apps from interleaving lines in one file.
pub const LOG_FILE_NAME: &str = "kairo.log";

/// electron-log's own file, for reading Electron's log (crash reports, support).
pub const ELECTRON_LOG_FILE_NAME: &str = "main.log";

/// `RECOVERY_FILE` in `services/resilience.ts`.
pub const RECOVERY_FILE_NAME: &str = "session-recovery.json";

/// Local record of native crashes (panics). Uploads are prompt 032.
pub const CRASH_LEDGER_FILE_NAME: &str = "kairo-native-crashes.json";

/// The operating systems Kairo builds for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Windows,
    Linux,
}

impl Os {
    /// The system this binary was built for.
    pub const fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }

    fn separator(self) -> char {
        match self {
            Os::Windows => '\\',
            Os::MacOs | Os::Linux => '/',
        }
    }
}

/// What the operating system reports, as [`compute`] needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct PathInputs {
    pub os: Os,
    /// The user's home folder (`os.homedir()`).
    pub home: String,
    /// Windows: the Roaming AppData known folder. Ignored elsewhere.
    pub roaming_app_data: Option<String>,
    /// Linux: `$XDG_CONFIG_HOME`. Ignored elsewhere.
    pub xdg_config_home: Option<String>,
    /// The Documents known folder, if the system reports one.
    pub documents: Option<String>,
    /// The Videos (Movies on macOS) known folder, if the system reports one.
    pub videos: Option<String>,
    /// The temporary folder.
    pub temp: String,
    /// This executable.
    pub exe: Option<String>,
    /// A packaged (release) build rather than a dev build.
    pub packaged: bool,
    /// `KAIRO_SHARED_PROFILE=1` was set.
    pub shared_profile: bool,
}

/// Every folder and file location Kairo uses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// The system the paths are laid out for.
    pub os: Os,
    /// `appData`: the per-user application data root.
    pub app_data: PathBuf,
    /// `userData`: `appData/Kairo`, or `appData/Kairo Dev` for a dev build.
    pub user_data: PathBuf,
    /// `logs`: electron-log's folder.
    pub logs: PathBuf,
    /// The native log file in [`AppPaths::logs`].
    pub log_file: PathBuf,
    /// electron-log's `main.log`.
    pub electron_log_file: PathBuf,
    /// `crashDumps`: `userData/Crashpad`.
    pub crash_dumps: PathBuf,
    /// `documents`, `None` when the system has none (Electron throws).
    pub documents: Option<PathBuf>,
    /// `home`.
    pub home: PathBuf,
    /// `videos`, `None` when the system has none (Electron throws).
    pub videos: Option<PathBuf>,
    /// `temp`.
    pub temp: PathBuf,
    /// `exe`, `None` when the system cannot say.
    pub exe: Option<PathBuf>,
    /// Running on the separate dev profile.
    pub dev_profile: bool,
    /// A packaged build.
    pub packaged: bool,
}

impl AppPaths {
    /// `session-recovery.json` in userData.
    pub fn recovery_file(&self) -> PathBuf {
        self.user_data.join(RECOVERY_FILE_NAME)
    }

    /// The local crash ledger in userData.
    pub fn crash_ledger_file(&self) -> PathBuf {
        self.user_data.join(CRASH_LEDGER_FILE_NAME)
    }

    /// A store file in userData: `<userData>/<name>.json`.
    pub fn store_file(&self, name: &str) -> PathBuf {
        self.user_data.join(format!("{name}.json"))
    }

    /// `WorkspaceService.defaultRoot`: `Documents/Kairo Presenter`, with home
    /// standing in when the system reports no Documents folder.
    pub fn default_workspace_root(&self) -> PathBuf {
        let base = self.documents.as_deref().unwrap_or(&self.home);
        join(self.os, &base.to_string_lossy(), &[WORKSPACE_FOLDER_NAME]).into()
    }
}

fn join(os: Os, base: &str, parts: &[&str]) -> String {
    let sep = os.separator();
    let mut out = base.trim_end_matches(sep).to_string();
    for part in parts {
        out.push(sep);
        out.push_str(part);
    }
    out
}

/// The paths Electron uses for `app.setName("Kairo")` on `inputs.os`, with
/// the dev-profile rule from `dev-profile.ts` applied.
pub fn compute(inputs: &PathInputs) -> AppPaths {
    let os = inputs.os;
    let home = inputs.home.as_str();
    let non_empty = |v: &Option<String>| v.as_deref().filter(|s| !s.is_empty()).map(str::to_string);

    // Electron: NSApplicationSupportDirectory, FOLDERID_RoamingAppData, and
    // Chromium's XDG rule ($XDG_CONFIG_HOME when set and non-empty, else ~/.config).
    let app_data = match os {
        Os::MacOs => join(os, home, &["Library", "Application Support"]),
        Os::Windows => non_empty(&inputs.roaming_app_data)
            .unwrap_or_else(|| join(os, home, &["AppData", "Roaming"])),
        Os::Linux => {
            non_empty(&inputs.xdg_config_home).unwrap_or_else(|| join(os, home, &[".config"]))
        }
    };

    let dev_profile = !inputs.packaged && !inputs.shared_profile;
    let profile = if dev_profile {
        DEV_PROFILE_NAME
    } else {
        PRODUCT_NAME
    };
    let user_data = join(os, &app_data, &[profile]);

    // Electron's default `logs`: ~/Library/Logs/<name> on macOS, userData/logs
    // elsewhere. dev-profile.ts moves it to userData/logs on every system.
    let logs = if os == Os::MacOs && !dev_profile {
        join(os, home, &["Library", "Logs", PRODUCT_NAME])
    } else {
        join(os, &user_data, &["logs"])
    };

    // Chromium's xdg-user-dirs fallback on Linux is ~/Documents and ~/Videos;
    // on macOS and Windows a missing known folder is an error in Electron.
    let documents = non_empty(&inputs.documents)
        .or_else(|| (os == Os::Linux).then(|| join(os, home, &["Documents"])));
    let videos = non_empty(&inputs.videos)
        .or_else(|| (os == Os::Linux).then(|| join(os, home, &["Videos"])));

    AppPaths {
        os,
        app_data: app_data.clone().into(),
        log_file: join(os, &logs, &[LOG_FILE_NAME]).into(),
        electron_log_file: join(os, &logs, &[ELECTRON_LOG_FILE_NAME]).into(),
        logs: logs.into(),
        crash_dumps: join(os, &user_data, &["Crashpad"]).into(),
        user_data: user_data.into(),
        documents: documents.map(PathBuf::from),
        home: home.into(),
        videos: videos.map(PathBuf::from),
        temp: inputs.temp.clone().into(),
        exe: inputs.exe.clone().map(PathBuf::from),
        dev_profile,
        packaged: inputs.packaged,
    }
}

/// Why the paths could not be found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathsError {
    /// The system reports no home folder.
    NoHome,
    /// A system folder is not valid Unicode, so it cannot match Electron's.
    NotUnicode(PathBuf),
}

impl fmt::Display for PathsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PathsError::NoHome => write!(f, "the system reports no home folder"),
            PathsError::NotUnicode(p) => write!(f, "{} is not valid Unicode", p.display()),
        }
    }
}

impl std::error::Error for PathsError {}

fn text(path: PathBuf) -> Result<String, PathsError> {
    path.into_os_string()
        .into_string()
        .map_err(|p| PathsError::NotUnicode(p.into()))
}

/// Whether this is a packaged build: release builds are, debug builds are not.
pub const fn is_packaged_build() -> bool {
    !cfg!(debug_assertions)
}

/// The inputs for this machine and build.
pub fn detect_inputs() -> Result<PathInputs, PathsError> {
    let home = text(dirs::home_dir().ok_or(PathsError::NoHome)?)?;
    let optional = |p: Option<PathBuf>| p.and_then(|p| p.into_os_string().into_string().ok());
    Ok(PathInputs {
        os: Os::current(),
        home,
        // dirs::config_dir is FOLDERID_RoamingAppData on Windows.
        roaming_app_data: if cfg!(windows) {
            optional(dirs::config_dir())
        } else {
            None
        },
        xdg_config_home: std::env::var("XDG_CONFIG_HOME").ok(),
        documents: optional(dirs::document_dir()),
        videos: optional(dirs::video_dir()),
        temp: text(std::env::temp_dir())?,
        exe: optional(std::env::current_exe().ok()),
        packaged: is_packaged_build(),
        shared_profile: std::env::var(SHARED_PROFILE_ENV).as_deref() == Ok("1"),
    })
}

/// The paths for this machine and build.
pub fn detect() -> Result<AppPaths, PathsError> {
    detect_inputs().map(|inputs| compute(&inputs))
}

/// `renamedStore`: before a renamed store opens, copy `<legacy>.json` to
/// `<next>.json` when only the legacy file exists. The legacy file stays, so
/// an older build on the same profile still finds it. Never fails: on any
/// error the store starts from its defaults, as on a fresh install. Returns `next`.
pub fn renamed_store<'a>(user_data: &Path, legacy: &str, next: &'a str) -> &'a str {
    let from = user_data.join(format!("{legacy}.json"));
    let to = user_data.join(format!("{next}.json"));
    if !to.exists() && from.exists() {
        match std::fs::copy(&from, &to) {
            Ok(_) => tracing::info!(legacy, next, "[Store] Adopted legacy store file"),
            Err(error) => {
                tracing::warn!(legacy, next, %error, "[Store] Could not adopt legacy store file")
            }
        }
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(os: Os, home: &str, packaged: bool) -> PathInputs {
        PathInputs {
            os,
            home: home.into(),
            roaming_app_data: None,
            xdg_config_home: None,
            documents: None,
            videos: None,
            temp: "/tmp".into(),
            exe: None,
            packaged,
            shared_profile: false,
        }
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn macos_packaged_matches_electron() {
        let mut i = inputs(Os::MacOs, "/Users/ada", true);
        i.documents = Some("/Users/ada/Documents".into());
        i.videos = Some("/Users/ada/Movies".into());
        let p = compute(&i);
        assert_eq!(s(&p.app_data), "/Users/ada/Library/Application Support");
        assert_eq!(
            s(&p.user_data),
            "/Users/ada/Library/Application Support/Kairo"
        );
        assert_eq!(s(&p.logs), "/Users/ada/Library/Logs/Kairo");
        assert_eq!(
            s(&p.electron_log_file),
            "/Users/ada/Library/Logs/Kairo/main.log"
        );
        assert_eq!(s(&p.log_file), "/Users/ada/Library/Logs/Kairo/kairo.log");
        assert_eq!(
            s(&p.crash_dumps),
            "/Users/ada/Library/Application Support/Kairo/Crashpad"
        );
        assert_eq!(
            s(&p.default_workspace_root()),
            "/Users/ada/Documents/Kairo Presenter"
        );
        assert_eq!(s(p.videos.as_ref().unwrap()), "/Users/ada/Movies");
        assert_eq!(
            s(&p.recovery_file()),
            "/Users/ada/Library/Application Support/Kairo/session-recovery.json"
        );
        assert!(!p.dev_profile);
    }

    #[test]
    fn windows_packaged_matches_electron() {
        let mut i = inputs(Os::Windows, r"C:\Users\Ada", true);
        i.roaming_app_data = Some(r"C:\Users\Ada\AppData\Roaming".into());
        i.documents = Some(r"C:\Users\Ada\Documents".into());
        let p = compute(&i);
        assert_eq!(s(&p.user_data), r"C:\Users\Ada\AppData\Roaming\Kairo");
        assert_eq!(s(&p.logs), r"C:\Users\Ada\AppData\Roaming\Kairo\logs");
        assert_eq!(
            s(&p.electron_log_file),
            r"C:\Users\Ada\AppData\Roaming\Kairo\logs\main.log"
        );
        assert_eq!(
            s(&p.crash_dumps),
            r"C:\Users\Ada\AppData\Roaming\Kairo\Crashpad"
        );
        // A redirected Roaming folder is used as the system reports it.
        i.roaming_app_data = Some(r"\\server\profiles\ada\Roaming\".into());
        assert_eq!(
            s(&compute(&i).user_data),
            r"\\server\profiles\ada\Roaming\Kairo"
        );
        // No known folder: the conventional location under home.
        i.roaming_app_data = None;
        assert_eq!(
            s(&compute(&i).user_data),
            r"C:\Users\Ada\AppData\Roaming\Kairo"
        );
    }

    #[test]
    fn linux_packaged_matches_electron() {
        let p = compute(&inputs(Os::Linux, "/home/ada", true));
        assert_eq!(s(&p.user_data), "/home/ada/.config/Kairo");
        assert_eq!(s(&p.logs), "/home/ada/.config/Kairo/logs");
        assert_eq!(
            s(&p.electron_log_file),
            "/home/ada/.config/Kairo/logs/main.log"
        );
        assert_eq!(s(p.documents.as_ref().unwrap()), "/home/ada/Documents");
        assert_eq!(s(p.videos.as_ref().unwrap()), "/home/ada/Videos");
        let mut i = inputs(Os::Linux, "/home/ada", true);
        i.xdg_config_home = Some("/data/config".into());
        assert_eq!(s(&compute(&i).user_data), "/data/config/Kairo");
        // An empty XDG_CONFIG_HOME is ignored, as Chromium does.
        i.xdg_config_home = Some(String::new());
        assert_eq!(s(&compute(&i).user_data), "/home/ada/.config/Kairo");
    }

    #[test]
    fn dev_builds_use_their_own_profile_on_every_os() {
        let mac = compute(&inputs(Os::MacOs, "/Users/ada", false));
        assert_eq!(
            s(&mac.user_data),
            "/Users/ada/Library/Application Support/Kairo Dev"
        );
        assert_eq!(
            s(&mac.logs),
            "/Users/ada/Library/Application Support/Kairo Dev/logs"
        );
        assert_eq!(
            s(&mac.electron_log_file),
            "/Users/ada/Library/Application Support/Kairo Dev/logs/main.log"
        );
        assert_eq!(
            s(&mac.crash_dumps),
            "/Users/ada/Library/Application Support/Kairo Dev/Crashpad"
        );
        assert!(mac.dev_profile);
        let win = compute(&inputs(Os::Windows, r"C:\Users\Ada", false));
        assert_eq!(s(&win.user_data), r"C:\Users\Ada\AppData\Roaming\Kairo Dev");
        let linux = compute(&inputs(Os::Linux, "/home/ada", false));
        assert_eq!(s(&linux.logs), "/home/ada/.config/Kairo Dev/logs");
    }

    #[test]
    fn the_shared_profile_flag_returns_a_dev_build_to_the_installed_profile() {
        let mut i = inputs(Os::MacOs, "/Users/ada", false);
        i.shared_profile = true;
        let p = compute(&i);
        assert_eq!(
            s(&p.user_data),
            "/Users/ada/Library/Application Support/Kairo"
        );
        assert_eq!(s(&p.logs), "/Users/ada/Library/Logs/Kairo");
        assert!(!p.dev_profile);
    }

    #[test]
    fn documents_fall_back_to_home_where_electron_throws() {
        let p = compute(&inputs(Os::MacOs, "/Users/ada", true));
        assert_eq!(p.documents, None);
        assert_eq!(p.videos, None);
        assert_eq!(s(&p.default_workspace_root()), "/Users/ada/Kairo Presenter");
        let mut i = inputs(Os::Windows, r"C:\Users\Ada", true);
        i.documents = Some(String::new());
        assert_eq!(
            s(&compute(&i).default_workspace_root()),
            r"C:\Users\Ada\Kairo Presenter"
        );
    }

    #[test]
    fn a_trailing_separator_on_home_is_not_doubled() {
        let p = compute(&inputs(Os::Linux, "/home/ada/", true));
        assert_eq!(s(&p.user_data), "/home/ada/.config/Kairo");
    }

    #[test]
    fn renamed_store_adopts_only_when_the_new_file_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("proautomate-settings.json");
        let next = dir.path().join("kairo-settings.json");
        std::fs::write(&legacy, "{\"a\":1}").unwrap();
        assert_eq!(
            renamed_store(dir.path(), "proautomate-settings", "kairo-settings"),
            "kairo-settings"
        );
        assert_eq!(std::fs::read_to_string(&next).unwrap(), "{\"a\":1}");
        assert!(legacy.exists(), "the legacy file stays for older builds");
        std::fs::write(&legacy, "{\"a\":2}").unwrap();
        renamed_store(dir.path(), "proautomate-settings", "kairo-settings");
        assert_eq!(
            std::fs::read_to_string(&next).unwrap(),
            "{\"a\":1}",
            "an existing file is never replaced"
        );
        // Neither file: nothing is created.
        renamed_store(dir.path(), "missing", "also-missing");
        assert!(!dir.path().join("also-missing.json").exists());
        // An unreadable folder does not fail.
        assert_eq!(
            renamed_store(Path::new("/nonexistent/\0bad"), "a", "b"),
            "b"
        );
    }

    #[test]
    fn detect_on_this_machine_is_consistent() {
        let p = detect().unwrap();
        assert!(p.user_data.starts_with(&p.app_data));
        assert!(p.log_file.starts_with(&p.logs));
        assert_eq!(p.packaged, is_packaged_build());
    }
}
