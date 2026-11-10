//! Finding installed office apps, from `office-apps.ts`. The probes are a
//! trait so tests never depend on what is installed.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

/// System access the discovery needs.
pub trait OfficeProbes: Sync {
    /// `darwin`, `win32` or `linux`, as Node names them.
    fn platform(&self) -> &str;
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<String>>;
    fn read_file(&self, path: &Path) -> std::io::Result<String>;
    fn exists(&self, path: &Path) -> bool;
    fn home(&self) -> PathBuf;
    fn env(&self, key: &str) -> Option<String>;
    /// `mdfind kMDItemCFBundleIdentifier == "<id>"` output.
    fn mdfind(&self, bundle_id: &str) -> Option<String>;
    /// `which` (`where` on Windows): the first line, or the bare name when
    /// the lookup printed nothing. `None` when the lookup failed.
    fn command_on_path(&self, name: &str) -> Option<String>;
}

/// The real machine.
pub struct SystemProbes;

pub fn node_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(windows) {
        "win32"
    } else {
        "linux"
    }
}

impl OfficeProbes for SystemProbes {
    fn platform(&self) -> &str {
        node_platform()
    }
    fn read_dir(&self, path: &Path) -> std::io::Result<Vec<String>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect()
    }
    fn read_file(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn home(&self) -> PathBuf {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
            .map(PathBuf::from)
            .unwrap_or_default()
    }
    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
    fn mdfind(&self, bundle_id: &str) -> Option<String> {
        let query = format!("kMDItemCFBundleIdentifier == \"{bundle_id}\"");
        let output = crate::process::run(
            std::process::Command::new("mdfind").arg(query),
            Duration::from_secs(5),
        )
        .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
    fn command_on_path(&self, name: &str) -> Option<String> {
        let tool = if cfg!(windows) { "where" } else { "which" };
        let output = crate::process::run_ok(
            std::process::Command::new(tool).arg(name),
            Duration::from_secs(5),
        )
        .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let found = stdout.lines().map(str::trim).find(|l| !l.is_empty());
        Some(found.unwrap_or(name).to_owned())
    }
}

static POWERPOINT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)powerpoint\.app$").unwrap());
static WPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^wpsoffice.*\.app$|^wps office.*\.app$").unwrap());
static KEYNOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^keynote\.app$").unwrap());
static LIBREOFFICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^libreoffice\.app$").unwrap());

pub fn is_power_point_app_name(name: &str) -> bool {
    POWERPOINT.is_match(name)
}
pub fn is_wps_app_name(name: &str) -> bool {
    WPS.is_match(name)
}
pub fn is_keynote_app_name(name: &str) -> bool {
    KEYNOTE.is_match(name)
}
pub fn is_libre_office_app_name(name: &str) -> bool {
    LIBREOFFICE.is_match(name)
}

/// macOS `.app` scan of `/Applications`, `~/Applications` and
/// `/System/Applications`. `None` elsewhere.
pub fn find_app_by_name(
    match_name: fn(&str) -> bool,
    probes: &dyn OfficeProbes,
) -> Option<PathBuf> {
    if probes.platform() != "darwin" {
        return None;
    }
    let roots = [
        PathBuf::from("/Applications"),
        probes.home().join("Applications"),
        PathBuf::from("/System/Applications"),
    ];
    for root in roots {
        // An inaccessible root: try the next one.
        let Ok(names) = probes.read_dir(&root) else {
            continue;
        };
        if let Some(found) = names.iter().find(|n| match_name(n)) {
            return Some(root.join(found));
        }
    }
    None
}

pub fn find_app_by_bundle_id(bundle_id: &str, probes: &dyn OfficeProbes) -> Option<PathBuf> {
    if probes.platform() != "darwin" {
        return None;
    }
    probes
        .mdfind(bundle_id)?
        .lines()
        .map(str::trim)
        .find(|line| line.ends_with(".app"))
        .map(PathBuf::from)
}

fn linux_desktop_dirs(home: &Path, probes: &dyn OfficeProbes) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let data_home = probes
        .env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    dirs.push(data_home.join("applications"));
    let data_dirs = probes
        .env("XDG_DATA_DIRS")
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for dir in data_dirs.split(':').filter(|d| !d.is_empty()) {
        dirs.push(Path::new(dir).join("applications"));
    }
    // Non-PATH installs (manual /opt unpacks) that still register a launcher.
    dirs.push(PathBuf::from("/opt/libreoffice/program"));
    dirs
}

static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^"((?:\\.|[^"\\])*)""#).unwrap());
static BARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^((?:\\.|[^\s\\])+)").unwrap());
static ESCAPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\(.)").unwrap());
static NAME_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Name=(.*)$").unwrap());
static EXEC_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Exec=(.*)$").unwrap());
static SOFFICE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[\s/])soffice(\.bin)?\b").unwrap());

/// First token of an `Exec=` line (`soffice --impress %U` is `soffice`).
pub fn parse_desktop_exec(exec_line: &str) -> Option<String> {
    let input = kairo_domain::js::trim(exec_line);
    if input.is_empty() {
        return None;
    }
    let token = QUOTED
        .captures(input)
        .or_else(|| BARE.captures(input))
        .map(|c| c[1].to_owned())?;
    let token = ESCAPE.replace_all(&token, "$1").into_owned();
    (!token.is_empty()).then_some(token)
}

/// JS `/^X=(.*)$/m`: `.` stops at `\n` and `\r`, and so does `$`.
fn line_value<'a>(re: &Regex, content: &'a str) -> Option<&'a str> {
    re.captures(content)
        .map(|c| c.get(1).map_or("", |m| m.as_str()))
        .map(|v| v.split('\r').next().unwrap_or(v))
}

fn matches_libre_office_impress(content: &str) -> bool {
    let name = line_value(&NAME_LINE, content).unwrap_or("").to_lowercase();
    let exec = line_value(&EXEC_LINE, content).unwrap_or("").to_lowercase();
    (name.contains("libreoffice") && (name.contains("impress") || name.contains("presentation")))
        || SOFFICE.is_match(&exec)
}

/// Linux `.desktop` scan for LibreOffice Impress: the `soffice` binary, or a
/// bare command for the caller to resolve on `PATH`.
pub fn find_libre_office_desktop_binary(probes: &dyn OfficeProbes) -> Option<String> {
    if probes.platform() != "linux" {
        return None;
    }
    let home = probes.home();
    for dir in linux_desktop_dirs(&home, probes) {
        let Ok(names) = probes.read_dir(&dir) else {
            continue;
        };
        for name in names.iter().filter(|n| n.ends_with(".desktop")) {
            let Ok(content) = probes.read_file(&dir.join(name)) else {
                continue;
            };
            if !matches_libre_office_impress(&content) {
                continue;
            }
            // A matching entry with no Exec= is malformed.
            let Some(exec_line) = line_value(&EXEC_LINE, &content) else {
                continue;
            };
            let Some(binary) = parse_desktop_exec(exec_line) else {
                continue;
            };
            if binary.starts_with('/') {
                if probes.exists(Path::new(&binary)) {
                    return Some(binary);
                }
                continue;
            }
            return Some(binary);
        }
    }
    // Manual /opt installs with no registered launcher.
    let fallback = "/opt/libreoffice/program/soffice";
    probes
        .exists(Path::new(fallback))
        .then(|| fallback.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct Fake {
        platform: &'static str,
        dirs: HashMap<PathBuf, Vec<String>>,
        files: HashMap<PathBuf, String>,
        existing: Vec<PathBuf>,
        env: HashMap<&'static str, String>,
        mdfind: Option<String>,
    }

    impl OfficeProbes for Fake {
        fn platform(&self) -> &str {
            self.platform
        }
        fn read_dir(&self, path: &Path) -> std::io::Result<Vec<String>> {
            self.dirs
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::ErrorKind::NotFound.into())
        }
        fn read_file(&self, path: &Path) -> std::io::Result<String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| std::io::ErrorKind::NotFound.into())
        }
        fn exists(&self, path: &Path) -> bool {
            self.existing.iter().any(|p| p == path)
        }
        fn home(&self) -> PathBuf {
            PathBuf::from("/home/u")
        }
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn mdfind(&self, _: &str) -> Option<String> {
            self.mdfind.clone()
        }
        fn command_on_path(&self, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn app_names() {
        assert!(is_power_point_app_name("Microsoft PowerPoint.app"));
        assert!(!is_power_point_app_name("PowerPoint.app.zip"));
        assert!(is_wps_app_name("wpsoffice.app") && is_wps_app_name("WPS Office for Mac.app"));
        assert!(!is_wps_app_name("My WPS Office.app"));
        assert!(is_keynote_app_name("Keynote.app") && !is_keynote_app_name("Keynote 2.app"));
        assert!(is_libre_office_app_name("LibreOffice.app"));
    }

    #[test]
    fn darwin_scan_skips_unreadable_roots_and_mdfind_takes_the_first_app() {
        let mut fake = Fake {
            platform: "darwin",
            ..Fake::default()
        };
        fake.dirs
            .insert("/home/u/Applications".into(), vec!["Keynote.app".into()]);
        assert_eq!(
            find_app_by_name(is_keynote_app_name, &fake),
            Some(PathBuf::from("/home/u/Applications/Keynote.app"))
        );
        fake.mdfind = Some("\n  /x/Notes.txt\n /Applications/Microsoft PowerPoint.app \n".into());
        assert_eq!(
            find_app_by_bundle_id("com.microsoft.Powerpoint", &fake),
            Some(PathBuf::from("/Applications/Microsoft PowerPoint.app"))
        );
        fake.platform = "linux";
        assert_eq!(find_app_by_name(is_keynote_app_name, &fake), None);
        assert_eq!(find_app_by_bundle_id("x", &fake), None);
    }

    #[test]
    fn desktop_exec_tokens() {
        assert_eq!(
            parse_desktop_exec("soffice --impress %U").as_deref(),
            Some("soffice")
        );
        assert_eq!(
            parse_desktop_exec(r#""/opt/Libre Office/soffice" --impress"#).as_deref(),
            Some("/opt/Libre Office/soffice")
        );
        assert_eq!(
            parse_desktop_exec(r"/opt/a\ b/soffice -x").as_deref(),
            Some("/opt/a b/soffice")
        );
        assert_eq!(parse_desktop_exec("   "), None);
    }

    #[test]
    fn linux_desktop_entries() {
        let mut fake = Fake {
            platform: "linux",
            ..Fake::default()
        };
        fake.env.insert("XDG_DATA_DIRS", "/usr/share".into());
        fake.dirs.insert(
            "/home/u/.local/share/applications".into(),
            vec![
                "broken.desktop".into(),
                "missing.desktop".into(),
                "notes.txt".into(),
            ],
        );
        fake.files.insert(
            "/home/u/.local/share/applications/broken.desktop".into(),
            "Name=LibreOffice Impress\n".into(),
        );
        fake.files.insert(
            "/home/u/.local/share/applications/missing.desktop".into(),
            "Name=x\nExec=/gone/soffice --impress\n".into(),
        );
        fake.dirs.insert(
            "/usr/share/applications".into(),
            vec!["impress.desktop".into()],
        );
        fake.files.insert(
            "/usr/share/applications/impress.desktop".into(),
            "[Desktop Entry]\r\nName=LibreOffice Impress\r\nExec=libreoffice --impress %U\r\n"
                .into(),
        );
        assert_eq!(
            find_libre_office_desktop_binary(&fake).as_deref(),
            Some("libreoffice")
        );
        fake.dirs.clear();
        assert_eq!(find_libre_office_desktop_binary(&fake), None);
        fake.existing
            .push("/opt/libreoffice/program/soffice".into());
        assert_eq!(
            find_libre_office_desktop_binary(&fake).as_deref(),
            Some("/opt/libreoffice/program/soffice")
        );
    }
}
