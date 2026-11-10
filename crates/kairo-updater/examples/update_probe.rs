//! A stand-in app for the macOS N to N+1 test (`tests/mac_bundle.rs`). Put
//! inside a `.app`, it reads its version from its own `Info.plist`, then:
//!
//! - with `<releases url> <log file>`: runs the real updater (packaged, on
//!   this bundle), downloads when offered, installs with relaunch, and
//!   exits as the app's quit would;
//! - with no arguments (the relaunch): writes `relaunched-<version>` next to
//!   its bundle and exits.

use kairo_updater::feed::ReleaseSource;
use kairo_updater::install::bundle_from_executable;
use kairo_updater::{InstallOutcome, UpdaterConfig, UpdaterService};
use std::io::Write;

fn version(bundle: &std::path::Path) -> String {
    let out = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleShortVersionString"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .expect("PlistBuddy");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

#[tokio::main]
async fn main() {
    let exe = std::env::current_exe().expect("exe");
    let bundle = bundle_from_executable(&exe).expect("run from inside a .app");
    let version = version(&bundle);
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        let marker = bundle
            .parent()
            .unwrap()
            .join(format!("relaunched-{version}"));
        std::fs::write(marker, "").unwrap();
        return;
    }
    let (releases, log) = (args[0].clone(), args[1].clone());
    let mut config =
        UpdaterConfig::for_this_process(version, true, Some("https://kairopresenter.com".into()));
    config.source = ReleaseSource { releases };
    let service = UpdaterService::new(config);
    let log_file = std::sync::Arc::new(std::sync::Mutex::new(std::fs::File::create(&log).unwrap()));
    let l = log_file.clone();
    service.on_change(move |status| {
        let _ = writeln!(l.lock().unwrap(), "{}", serde_json_line(status));
    });
    service.set_before_install(|| async {});
    service.set_quit(|| {
        // The app's quit: the swap script waits for this process to exit.
        std::process::exit(0);
    });
    service.init().await;
    service.download().await;
    let outcome = service.install(true).await;
    let _ = writeln!(log_file.lock().unwrap(), "install {outcome:?}");
    assert_ne!(
        outcome,
        InstallOutcome::Started,
        "Started exits before this line"
    );
}

fn serde_json_line(status: &kairo_domain::status::UpdateStatus) -> String {
    format!("{status:?}")
}
