//! Putting a verified update in place.
//!
//! - macOS: unpack the zip with `ditto`, check the bundle, and swap it in
//!   after quit with a detached script (`mac-self-update.ts`). Developer ID
//!   builds also require the new bundle to satisfy the running app's
//!   designated requirement, the check Squirrel.Mac makes.
//! - Windows: run the NSIS installer as electron-updater does.
//! - Linux AppImage: replace the file after quit with a detached script.

use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

/// `bundleFromExecutable`: the `.app` that holds `<bundle>/Contents/MacOS/<name>`.
pub fn bundle_from_executable(exe: &Path) -> Option<PathBuf> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern =
        PATTERN.get_or_init(|| Regex::new(r"^(.*?\.app)/Contents/MacOS/[^/]+$").expect("valid"));
    let text = exe.to_str()?;
    pattern.captures(text).map(|c| PathBuf::from(&c[1]))
}

/// `isAdhocSignature`: `codesign -dv` describes an ad-hoc signature.
pub fn is_adhoc_signature(codesign_output: &str) -> bool {
    let lower = codesign_output.to_ascii_lowercase();
    lower.contains("signature=adhoc") || lower.contains("teamidentifier=not set")
}

/// The `designated => …` line of `codesign -d -r-`.
pub fn designated_requirement(codesign_output: &str) -> Option<String> {
    codesign_output
        .lines()
        .find_map(|line| line.trim().strip_prefix("designated => "))
        .map(|r| r.trim().to_owned())
        .filter(|r| !r.is_empty())
}

/// How the running Mac app is signed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacSigning {
    /// Ad-hoc or unsigned: only bundle id and an intact signature are checked.
    Adhoc,
    /// Developer ID: the update must also satisfy this requirement.
    Designated(String),
}

/// Longest a child process may run before it is killed.
const PROCESS_TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) async fn run(
    program: &str,
    args: &[&std::ffi::OsStr],
) -> Result<(String, String), String> {
    run_with_timeout(program, args, PROCESS_TIMEOUT).await
}

/// Runs `program`, killing it if it outlives `timeout` (a stuck
/// `hdiutil attach` must not leave the update installing forever).
async fn run_with_timeout(
    program: &str,
    args: &[&std::ffi::OsStr],
    timeout: Duration,
) -> Result<(String, String), String> {
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| format!("{program} timed out"))?
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok((stdout, stderr))
    } else {
        Err(format!("{program} failed: {}", stderr.trim()))
    }
}

/// Reads the signature of the installed bundle. No signature at all is the
/// ad-hoc case, as in Electron.
pub async fn mac_signing(bundle: &Path) -> MacSigning {
    let shown = run(
        "codesign",
        &["-dv".as_ref(), "--verbose=2".as_ref(), bundle.as_os_str()],
    )
    .await;
    match shown {
        Ok((out, err)) if !is_adhoc_signature(&format!("{out}\n{err}")) => {
            match run(
                "codesign",
                &["-d".as_ref(), "-r-".as_ref(), bundle.as_os_str()],
            )
            .await
            {
                Ok((out, err)) => designated_requirement(&format!("{out}\n{err}"))
                    .map_or(MacSigning::Adhoc, MacSigning::Designated),
                Err(_) => MacSigning::Adhoc,
            }
        }
        _ => MacSigning::Adhoc,
    }
}

/// `CFBundleIdentifier` from the bundle's `Info.plist`.
pub async fn bundle_id(bundle: &Path) -> Result<String, String> {
    let plist = bundle.join("Contents").join("Info.plist");
    run(
        "/usr/libexec/PlistBuddy",
        &[
            "-c".as_ref(),
            "Print :CFBundleIdentifier".as_ref(),
            plist.as_os_str(),
        ],
    )
    .await
    .map(|(out, _)| out.trim().to_owned())
}

/// Unpacks `zip` and checks the bundle inside is this app with an intact
/// signature before it may be staged. Returns the bundle path.
pub async fn verify_mac_update(
    zip: &Path,
    unpack_to: &Path,
    current: &Path,
    signing: &MacSigning,
) -> Result<PathBuf, String> {
    run(
        "ditto",
        &[
            "-x".as_ref(),
            "-k".as_ref(),
            zip.as_os_str(),
            unpack_to.as_os_str(),
        ],
    )
    .await?;
    let _ = tokio::fs::remove_file(zip).await;
    let mut entries = tokio::fs::read_dir(unpack_to)
        .await
        .map_err(|e| e.to_string())?;
    let mut bundle = None;
    while let Some(entry) = entries.next_entry().await.map_err(|e| e.to_string())? {
        if entry.file_name().to_string_lossy().ends_with(".app") {
            bundle = Some(entry.path());
            break;
        }
    }
    let bundle = bundle.ok_or("The update did not contain an app.")?;
    // Same app (bundle id) and an intact signature; never swap in anything else.
    let (mine, theirs) = tokio::join!(bundle_id(current), bundle_id(&bundle));
    let (mine, theirs) = (mine?, theirs.unwrap_or_default());
    if mine != theirs {
        return Err(format!("The update is not Kairo ({theirs})."));
    }
    run(
        "codesign",
        &[
            "--verify".as_ref(),
            "--deep".as_ref(),
            "--strict".as_ref(),
            bundle.as_os_str(),
        ],
    )
    .await?;
    if let MacSigning::Designated(requirement) = signing {
        let requirement = format!("-R={requirement}");
        run(
            "codesign",
            &[
                "--verify".as_ref(),
                "--deep".as_ref(),
                "--strict".as_ref(),
                requirement.as_ref(),
                bundle.as_os_str(),
            ],
        )
        .await
        .map_err(|_| "The update is not signed by the same developer as this Kairo.".to_owned())?;
    }
    let _ = run(
        "xattr",
        &[
            "-dr".as_ref(),
            "com.apple.quarantine".as_ref(),
            bundle.as_os_str(),
        ],
    )
    .await;
    Ok(bundle)
}

/// `SWAP_SCRIPT`, byte for byte. Arguments: the old app's PID, the installed
/// bundle, the new bundle, and `1` to reopen afterwards. It waits up to a
/// minute for the PID to exit, moves the old bundle aside, moves the new one
/// in and deletes the old one; when the move fails the old bundle goes back.
pub const MAC_SWAP_SCRIPT: &str = r#"#!/bin/bash
PID="$1"; OLD="$2"; NEW="$3"; RELAUNCH="$4"
for _ in $(seq 1 300); do kill -0 "$PID" 2>/dev/null || break; sleep 0.2; done
BACKUP="$OLD.kairo-old-$$"
if mv "$OLD" "$BACKUP"; then
  if mv "$NEW" "$OLD"; then
    rm -rf "$BACKUP"
  else
    mv "$BACKUP" "$OLD"
  fi
fi
rm -rf "$(dirname "$NEW")"
if [ "$RELAUNCH" = "1" ]; then open "$OLD"; fi
"#;

/// The AppImage counterpart. The staged file may be on another file system,
/// so it is copied next to the installed one first and then renamed over it;
/// a failed copy leaves the installed AppImage untouched.
pub const LINUX_SWAP_SCRIPT: &str = r#"#!/bin/sh
PID="$1"; OLD="$2"; NEW="$3"; RELAUNCH="$4"
for _ in $(seq 1 300); do kill -0 "$PID" 2>/dev/null || break; sleep 0.2; done
NEXT="$OLD.kairo-new-$$"
if cp "$NEW" "$NEXT" && chmod 755 "$NEXT"; then
  mv -f "$NEXT" "$OLD" || rm -f "$NEXT"
else
  rm -f "$NEXT"
fi
rm -rf "$(dirname "$NEW")"
if [ "$RELAUNCH" = "1" ]; then nohup "$OLD" >/dev/null 2>&1 & fi
"#;

/// electron-updater's NSIS arguments: an explicit install shows the
/// installer and runs Kairo after; the install on an ordinary quit is silent.
pub fn windows_installer_args(explicit: bool) -> Vec<&'static str> {
    if explicit {
        vec!["--updated", "--force-run"]
    } else {
        vec!["--updated", "/S"]
    }
}

/// True when the folder and the item can both be replaced.
pub fn can_replace(installed: &Path) -> bool {
    fn writable(path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
                return false;
            };
            // SAFETY: `c` is a valid NUL-terminated path for the call's duration.
            unsafe { libc_access(c.as_ptr()) }
        }
        #[cfg(not(unix))]
        {
            std::fs::metadata(path).is_ok_and(|m| !m.permissions().readonly())
        }
    }
    installed.parent().is_some_and(writable) && writable(installed)
}

#[cfg(unix)]
unsafe fn libc_access(path: *const std::ffi::c_char) -> bool {
    unsafe extern "C" {
        fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
    }
    const W_OK: i32 = 2;
    // SAFETY: the caller passes a valid C string.
    unsafe { access(path, W_OK) == 0 }
}

/// Starts `script` detached; it outlives this process.
pub fn spawn_detached(
    interpreter: &str,
    script: &Path,
    args: &[&std::ffi::OsStr],
) -> std::io::Result<()> {
    let mut command = std::process::Command::new(interpreter);
    command
        .arg(script)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so the app's exit does not take it down.
        command.process_group(0);
    }
    command.spawn().map(drop)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn kills_a_child_that_outlives_the_timeout() {
        let started = std::time::Instant::now();
        let result = run_with_timeout("sleep", &["5".as_ref()], Duration::from_millis(100)).await;
        assert!(result.unwrap_err().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
