//! External PowerPoint converters, from `converters.ts`: detection per
//! platform and conversion to PDF. The AppleScript and PowerShell text is
//! Electron's, unchanged.

use crate::DocumentError;
use crate::office_apps::{
    OfficeProbes, find_app_by_bundle_id, find_app_by_name, find_libre_office_desktop_binary,
    is_keynote_app_name, is_libre_office_app_name, is_power_point_app_name, is_wps_app_name,
};
use base64::Engine;
use kairo_domain::documents::PowerpointConverterId;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// `CONVERT_MS`.
pub const CONVERT_TIMEOUT: Duration = Duration::from_secs(120);

pub const POWERPOINT_CONVERTER_ORDER: [PowerpointConverterId; 4] = [
    PowerpointConverterId::PowerPoint,
    PowerpointConverterId::Wps,
    PowerpointConverterId::Keynote,
    PowerpointConverterId::LibreOffice,
];

pub const NEEDS_AN_APP: &str = "PowerPoint import needs an app that can convert the deck: Microsoft PowerPoint, WPS Office, Keynote, or LibreOffice. Or export the deck to PDF and import that instead.";

pub fn escape_apple_script_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Encodes an untrusted path as inert data for a PowerShell environment variable.
pub fn encode_power_shell_transport(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
}

const POWERSHELL_DECODE_SOURCE: &str =
    "[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:KAIRO_PPT_SOURCE_B64))";
const POWERSHELL_DECODE_DEST: &str =
    "[Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($env:KAIRO_PPT_DEST_B64))";

/// What converts on this machine: the probes plus Kairo's downloaded
/// LibreOffice, when it is installed.
pub struct Detector<'a> {
    pub probes: &'a dyn OfficeProbes,
    pub downloaded_libre_office: Option<PathBuf>,
}

impl Detector<'_> {
    fn first_existing(&self, paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
        paths.into_iter().find(|p| self.probes.exists(p))
    }

    fn first_available_command(&self, names: &[&str]) -> Option<PathBuf> {
        names
            .iter()
            .find_map(|n| self.probes.command_on_path(n))
            .map(PathBuf::from)
    }

    fn program_files(&self) -> Vec<PathBuf> {
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .iter()
            .filter_map(|k| self.probes.env(k).filter(|v| !v.is_empty()))
            .map(PathBuf::from)
            .collect()
    }

    pub fn power_point(&self) -> Option<PathBuf> {
        match self.probes.platform() {
            "darwin" => find_app_by_name(is_power_point_app_name, self.probes)
                .or_else(|| find_app_by_bundle_id("com.microsoft.Powerpoint", self.probes))
                .or_else(|| {
                    self.first_existing(
                        [
                            "/Applications/Microsoft PowerPoint.app",
                            "/Applications/Microsoft Office 2016/Microsoft PowerPoint.app",
                            "/Applications/Microsoft Office 2019/Microsoft PowerPoint.app",
                        ]
                        .map(PathBuf::from),
                    )
                }),
            "win32" => {
                let named = self.program_files().into_iter().flat_map(|root| {
                    [
                        root.join("Microsoft Office/root/Office16/POWERPNT.EXE"),
                        root.join("Microsoft Office/root/Office15/POWERPNT.EXE"),
                        root.join("Microsoft Office/Office16/POWERPNT.EXE"),
                        root.join("Microsoft Office/Office15/POWERPNT.EXE"),
                    ]
                });
                self.first_existing(named.collect::<Vec<_>>())
                    .or_else(|| self.first_available_command(&["POWERPNT.EXE"]))
            }
            _ => None,
        }
    }

    pub fn wps(&self) -> Option<PathBuf> {
        match self.probes.platform() {
            "darwin" => find_app_by_name(is_wps_app_name, self.probes)
                .or_else(|| find_app_by_bundle_id("com.kingsoft.wpsoffice.mac.global", self.probes))
                .or_else(|| find_app_by_bundle_id("com.kingsoft.wpsoffice.mac", self.probes))
                .or_else(|| {
                    self.first_existing(
                        [
                            "/Applications/wpsoffice.app",
                            "/Applications/WPS Office.app",
                            "/Applications/WPS Office for Mac.app",
                        ]
                        .map(PathBuf::from),
                    )
                }),
            "win32" => {
                let mut named: Vec<PathBuf> = self
                    .program_files()
                    .into_iter()
                    .flat_map(|root| {
                        [
                            root.join("Kingsoft/WPS Office/office6/wpp.exe"),
                            root.join("WPS Office/office6/wpp.exe"),
                        ]
                    })
                    .collect();
                if let Some(local) = self.probes.env("LOCALAPPDATA").filter(|v| !v.is_empty()) {
                    named.push(Path::new(&local).join("Kingsoft/WPS Office/ksolaunch.exe"));
                }
                self.first_existing(named)
                    .or_else(|| self.first_available_command(&["wpp"]))
            }
            _ => self.first_available_command(&["wpp", "wps"]),
        }
    }

    pub fn keynote(&self) -> Option<PathBuf> {
        if self.probes.platform() != "darwin" {
            return None;
        }
        find_app_by_name(is_keynote_app_name, self.probes)
            .or_else(|| find_app_by_bundle_id("com.apple.iWork.Keynote", self.probes))
            .or_else(|| {
                self.first_existing(
                    [
                        "/Applications/Keynote.app",
                        "/System/Applications/Keynote.app",
                    ]
                    .map(PathBuf::from),
                )
            })
    }

    /// A LibreOffice installed on the system, not the one Kairo downloads.
    pub fn system_libre_office(&self) -> Option<PathBuf> {
        match self.probes.platform() {
            "darwin" => {
                let soffice = find_app_by_name(is_libre_office_app_name, self.probes)
                    .map(|app| app.join("Contents/MacOS/soffice"))
                    .unwrap_or_else(|| {
                        PathBuf::from("/Applications/LibreOffice.app/Contents/MacOS/soffice")
                    });
                if self.probes.exists(&soffice) {
                    return Some(soffice);
                }
            }
            "win32" => {
                let named = self
                    .program_files()
                    .into_iter()
                    .map(|root| root.join("LibreOffice/program/soffice.exe"));
                if let Some(found) = self.first_existing(named.collect::<Vec<_>>()) {
                    return Some(found);
                }
            }
            "linux" => {
                // .desktop launchers catch non-PATH installs.
                if let Some(binary) = find_libre_office_desktop_binary(self.probes) {
                    if binary.starts_with('/') {
                        return Some(binary.into());
                    }
                    if let Some(via_path) = self.probes.command_on_path(&binary) {
                        return Some(via_path.into());
                    }
                }
            }
            _ => {}
        }
        self.first_available_command(&["soffice", "libreoffice"])
    }

    /// Kairo's downloaded converter first: its version is pinned and known to work.
    pub fn libre_office(&self) -> Option<PathBuf> {
        self.downloaded_libre_office
            .clone()
            .filter(|p| self.probes.exists(p))
            .or_else(|| self.system_libre_office())
    }

    pub fn detect(&self, id: PowerpointConverterId) -> Option<PathBuf> {
        match id {
            PowerpointConverterId::PowerPoint => self.power_point(),
            PowerpointConverterId::Wps => self.wps(),
            PowerpointConverterId::Keynote => self.keynote(),
            PowerpointConverterId::LibreOffice => self.libre_office(),
            PowerpointConverterId::Builtin => None,
        }
    }

    pub fn converters(&self) -> Vec<PowerpointConverterId> {
        POWERPOINT_CONVERTER_ORDER
            .into_iter()
            .filter(|id| self.detect(*id).is_some())
            .collect()
    }

    /// Accurate converters other than Kairo's own download. WPS is left out:
    /// on macOS it can't convert in the background.
    pub fn other_converter(&self) -> Option<PowerpointConverterId> {
        if self.power_point().is_some() {
            Some(PowerpointConverterId::PowerPoint)
        } else if self.keynote().is_some() {
            Some(PowerpointConverterId::Keynote)
        } else if self.system_libre_office().is_some() {
            Some(PowerpointConverterId::LibreOffice)
        } else {
            None
        }
    }
}

fn message(text: impl Into<String>) -> DocumentError {
    DocumentError::Message(text.into())
}

fn assert_pdf(path: &Path) -> Result<(), DocumentError> {
    let mut header = [0u8; 5];
    let mut file = std::fs::File::open(path)?;
    let read = file.read(&mut header)?;
    if read < 5 || &header != b"%PDF-" {
        return Err(message("Converter did not produce a PDF."));
    }
    Ok(())
}

fn osascript(source: &str) -> Result<(), DocumentError> {
    crate::process::run_ok(
        Command::new("osascript").arg("-e").arg(source),
        CONVERT_TIMEOUT,
    )?;
    Ok(())
}

fn powershell(script: &str, source: &Path, dest_pdf: &Path) -> Result<(), DocumentError> {
    crate::process::run_ok(
        Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env(
                "KAIRO_PPT_SOURCE_B64",
                encode_power_shell_transport(&source.to_string_lossy()),
            )
            .env(
                "KAIRO_PPT_DEST_B64",
                encode_power_shell_transport(&dest_pdf.to_string_lossy()),
            ),
        CONVERT_TIMEOUT,
    )?;
    Ok(())
}

/// The AppleScripts PowerPoint is tried with, in order.
pub fn power_point_apple_scripts(source: &Path, dest_pdf: &Path) -> [String; 2] {
    let src = escape_apple_script_string(&source.to_string_lossy());
    let dest = escape_apple_script_string(&dest_pdf.to_string_lossy());
    [
        format!(
            "tell application \"Microsoft PowerPoint\"\nset thePres to open POSIX file \"{src}\"\nsave thePres in POSIX file \"{dest}\" as save as PDF\nclose thePres saving no\nend tell"
        ),
        format!(
            "tell application \"Microsoft PowerPoint\"\nopen POSIX file \"{src}\"\nsave active presentation in POSIX file \"{dest}\" as save as PDF\nclose active presentation saving no\nend tell"
        ),
    ]
}

pub fn keynote_apple_script(source: &Path, dest_pdf: &Path) -> String {
    let src = escape_apple_script_string(&source.to_string_lossy());
    let dest = escape_apple_script_string(&dest_pdf.to_string_lossy());
    format!(
        "tell application \"Keynote\"\nset theDoc to open POSIX file \"{src}\"\nexport theDoc to POSIX file \"{dest}\" as PDF\nclose theDoc saving no\nend tell"
    )
}

pub fn power_point_power_shell() -> String {
    [
        "$ErrorActionPreference = \"Stop\"",
        &format!("$src = {POWERSHELL_DECODE_SOURCE}"),
        &format!("$dest = {POWERSHELL_DECODE_DEST}"),
        "$ppt = New-Object -ComObject PowerPoint.Application",
        "try {",
        "  $ppt.DisplayAlerts = 2",
        "  $pres = $ppt.Presentations.Open($src, $true, $false, $false)",
        "  $pres.SaveAs($dest, 32)",
        "  $pres.Close()",
        "} finally {",
        "  if ($ppt.Presentations.Count -eq 0) { $ppt.Quit() }",
        "}",
    ]
    .join("; ")
}

pub fn wps_power_shell() -> String {
    [
        "$ErrorActionPreference = \"Stop\"",
        &format!("$src = {POWERSHELL_DECODE_SOURCE}"),
        &format!("$dest = {POWERSHELL_DECODE_DEST}"),
        "foreach ($prog in @(\"Kwpp.Application\",\"wpp.Application\",\"WPP.Application\")) {",
        "  try {",
        "    $app = New-Object -ComObject $prog",
        "    $pres = $app.Presentations.Open($src)",
        "    $pres.SaveAs($dest, 32)",
        "    $pres.Close()",
        "    if ($app.Presentations.Count -eq 0) { $app.Quit() }",
        "    exit 0",
        "  } catch {}",
        "}",
        "throw \"WPS Office could not convert this file.\"",
    ]
    .join("; ")
}

fn convert_with_power_point(
    platform: &str,
    source: &Path,
    dest_pdf: &Path,
) -> Result<(), DocumentError> {
    match platform {
        "darwin" => {
            let mut last = None;
            for script in power_point_apple_scripts(source, dest_pdf) {
                match osascript(&script).and_then(|()| assert_pdf(dest_pdf)) {
                    Ok(()) => return Ok(()),
                    Err(e) => last = Some(e),
                }
            }
            Err(last.unwrap_or_else(|| message("PowerPoint could not export this file as PDF.")))
        }
        "win32" => {
            powershell(&power_point_power_shell(), source, dest_pdf)?;
            assert_pdf(dest_pdf)
        }
        _ => Err(message(
            "Microsoft PowerPoint conversion is not available on this system.",
        )),
    }
}

/// `None` is Electron's `'skip'`: this converter does not apply here.
fn convert_with_wps(
    detector: &Detector<'_>,
    source: &Path,
    dest_pdf: &Path,
) -> Option<Result<(), DocumentError>> {
    match detector.probes.platform() {
        // The Mac app launches a GUI for any argv, including --help / --headless.
        "darwin" => None,
        "win32" => Some(
            powershell(&wps_power_shell(), source, dest_pdf).and_then(|()| assert_pdf(dest_pdf)),
        ),
        _ => Some((|| {
            let cli = detector
                .first_available_command(&["wpp", "wps"])
                .ok_or_else(|| message("WPS Office is not available."))?;
            let outdir = dest_pdf.parent().unwrap_or(Path::new("."));
            crate::process::run_ok(
                Command::new(cli)
                    .args(["--headless", "--convert-to", "pdf", "--outdir"])
                    .arg(outdir)
                    .arg(source),
                CONVERT_TIMEOUT,
            )?;
            assert_pdf(dest_pdf)
        })()),
    }
}

/// LibreOffice's arguments, with a private profile in `work_dir`.
pub fn libre_office_args(source: &Path, work_dir: &Path) -> Vec<std::ffi::OsString> {
    let profile = url::Url::from_file_path(work_dir.join("profile"))
        .map_or_else(|()| String::new(), |u| u.to_string());
    let mut args: Vec<std::ffi::OsString> = [
        format!("-env:UserInstallation={profile}"),
        "--headless".into(),
        "--nologo".into(),
        "--nolockcheck".into(),
        "--convert-to".into(),
        "pdf:impress_pdf_Export".into(),
        "--outdir".into(),
    ]
    .into_iter()
    .map(Into::into)
    .collect();
    args.push(work_dir.into());
    args.push(source.into());
    args
}

fn convert_with_libre_office(
    source: &Path,
    dest_pdf: &Path,
    work_dir: &Path,
    soffice: &Path,
) -> Result<(), DocumentError> {
    crate::process::run_ok(
        Command::new(soffice).args(libre_office_args(source, work_dir)),
        CONVERT_TIMEOUT,
    )?;
    if !dest_pdf.exists() {
        return Err(message("LibreOffice did not write a PDF."));
    }
    assert_pdf(dest_pdf)?;
    let _ = std::fs::remove_dir_all(work_dir.join("profile"));
    Ok(())
}

/// Tries each installed converter in order. `dest_pdf` must be
/// `work_dir/source.pdf` for LibreOffice, which names its output itself.
pub fn convert_power_point_to_pdf(
    detector: &Detector<'_>,
    source: &Path,
    dest_pdf: &Path,
    work_dir: &Path,
) -> Result<PowerpointConverterId, DocumentError> {
    let platform = detector.probes.platform();
    let mut errors: Vec<PowerpointConverterId> = Vec::new();
    for id in POWERPOINT_CONVERTER_ORDER {
        let attempt = match id {
            PowerpointConverterId::PowerPoint => detector
                .power_point()
                .map(|_| convert_with_power_point(platform, source, dest_pdf)),
            PowerpointConverterId::Wps => detector
                .wps()
                .and_then(|_| convert_with_wps(detector, source, dest_pdf)),
            PowerpointConverterId::Keynote => detector.keynote().map(|_| {
                osascript(&keynote_apple_script(source, dest_pdf))
                    .and_then(|()| assert_pdf(dest_pdf))
            }),
            PowerpointConverterId::LibreOffice => detector
                .libre_office()
                .map(|soffice| convert_with_libre_office(source, dest_pdf, work_dir, &soffice)),
            PowerpointConverterId::Builtin => None,
        };
        match attempt {
            None => {}
            Some(Ok(())) => return Ok(id),
            Some(Err(error)) => {
                tracing::warn!(converter = id.as_str(), %error, "[Documents] Converter failed");
                errors.push(id);
            }
        }
    }
    if errors.is_empty() {
        return Err(message(NEEDS_AN_APP));
    }
    let tried: Vec<&str> = errors.iter().map(|id| id.as_str()).collect();
    Err(message(format!(
        "Could not convert this PowerPoint file. Tried {}. The file may be protected or damaged — export it to PDF and import that instead.",
        tried.join(", ")
    )))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Linux with nothing on `PATH`; files are the real disk.
    struct Bare(&'static [&'static str]);

    impl OfficeProbes for Bare {
        fn platform(&self) -> &str {
            "linux"
        }
        fn read_dir(&self, _: &Path) -> std::io::Result<Vec<String>> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn read_file(&self, _: &Path) -> std::io::Result<String> {
            Err(std::io::ErrorKind::NotFound.into())
        }
        fn exists(&self, path: &Path) -> bool {
            path.is_file()
        }
        fn home(&self) -> PathBuf {
            PathBuf::from("/nonexistent")
        }
        fn env(&self, _: &str) -> Option<String> {
            None
        }
        fn mdfind(&self, _: &str) -> Option<String> {
            None
        }
        fn command_on_path(&self, name: &str) -> Option<String> {
            self.0.contains(&name).then(|| format!("/usr/bin/{name}"))
        }
    }

    #[test]
    fn order_escaping_and_transport() {
        assert_eq!(
            POWERPOINT_CONVERTER_ORDER.map(PowerpointConverterId::as_str),
            ["powerpoint", "wps", "keynote", "libreoffice"]
        );
        assert_eq!(
            escape_apple_script_string("/tmp/Sunday \"slides\".pptx"),
            "/tmp/Sunday \\\"slides\\\".pptx"
        );
        assert_eq!(
            escape_apple_script_string("C:\\decks\\a.pptx"),
            "C:\\\\decks\\\\a.pptx"
        );
        let path = "C:\\Users\\O'Brien\\$(calc).pptx";
        let encoded = encode_power_shell_transport(path);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), path);
        for script in [power_point_power_shell(), wps_power_shell()] {
            assert!(
                script.contains("$env:KAIRO_PPT_SOURCE_B64")
                    && script.contains("$env:KAIRO_PPT_DEST_B64")
            );
        }
        let [first, _] = power_point_apple_scripts(Path::new("/a \"b\".pptx"), Path::new("/c.pdf"));
        assert!(first.starts_with("tell application \"Microsoft PowerPoint\"\nset thePres to open POSIX file \"/a \\\"b\\\".pptx\""));
    }

    #[test]
    fn later_command_names_are_tried() {
        let detector = Detector {
            probes: &Bare(&["libreoffice"]),
            downloaded_libre_office: None,
        };
        assert_eq!(
            detector.system_libre_office(),
            Some(PathBuf::from("/usr/bin/libreoffice"))
        );
        assert_eq!(detector.converters(), [PowerpointConverterId::LibreOffice]);
        let detector = Detector {
            probes: &Bare(&["wpp", "wps"]),
            downloaded_libre_office: None,
        };
        assert_eq!(detector.wps(), Some(PathBuf::from("/usr/bin/wpp")));
    }

    #[test]
    fn nothing_installed_asks_for_an_app() {
        let dir = tempfile::tempdir().unwrap();
        let detector = Detector {
            probes: &Bare(&[]),
            downloaded_libre_office: None,
        };
        assert_eq!(detector.converters(), []);
        let error = convert_power_point_to_pdf(
            &detector,
            &dir.path().join("a.pptx"),
            &dir.path().join("source.pdf"),
            dir.path(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), NEEDS_AN_APP);
    }

    fn fake_soffice(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("soffice");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn libre_office_writes_into_the_work_folder() {
        let tools = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        let source = work.path().join("source.pptx");
        std::fs::write(&source, b"deck").unwrap();
        let dest = work.path().join("source.pdf");
        // Arguments: -env:… --headless --nologo --nolockcheck --convert-to <f> --outdir <dir> <src>
        let soffice = fake_soffice(
            tools.path(),
            "mkdir -p \"$8/profile\"; printf '%%PDF-1.4' > \"$8/source.pdf\"",
        );
        let detector = Detector {
            probes: &Bare(&[]),
            downloaded_libre_office: Some(soffice.clone()),
        };
        let used = convert_power_point_to_pdf(&detector, &source, &dest, work.path()).unwrap();
        assert_eq!(used, PowerpointConverterId::LibreOffice);
        assert!(dest.is_file() && !work.path().join("profile").exists());
        assert!(
            libre_office_args(&source, work.path())[0]
                .to_string_lossy()
                .starts_with("-env:UserInstallation=file:///")
        );

        std::fs::remove_file(&dest).unwrap();
        let broken = fake_soffice(tools.path(), "printf 'nope' > \"$8/source.pdf\"");
        let detector = Detector {
            probes: &Bare(&[]),
            downloaded_libre_office: Some(broken),
        };
        let error = convert_power_point_to_pdf(&detector, &source, &dest, work.path()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Could not convert this PowerPoint file. Tried libreoffice. The file may be protected or damaged — export it to PDF and import that instead."
        );
    }
}
