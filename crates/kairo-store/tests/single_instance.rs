//! Two real processes: the first takes the single-instance lock, the second
//! forwards its `.kairo` file to it and exits.
//!
//! Both are this test executable, started again with `KAIRO_INSTANCE_PROBE`
//! set, running only `probe` (which does nothing in a normal run).

use kairo_store::instance::{Forwarded, Instance, acquire};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

const PROBE_ENV: &str = "KAIRO_INSTANCE_PROBE";

/// In a child process: acquire the lock for the folder in `PROBE_ENV`, with
/// the arguments after `--` as the launch, and report what happened.
#[test]
fn probe() {
    let Ok(user_data) = std::env::var(PROBE_ENV) else {
        return;
    };
    let argv: Vec<String> = std::env::var("KAIRO_PROBE_ARGS")
        .unwrap_or_default()
        .split('\n')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    let launch = Forwarded {
        argv,
        cwd: std::env::current_dir().ok(),
    };
    match acquire(Path::new(&user_data), &launch).expect("acquire") {
        Instance::Primary(primary) => {
            println!("\n@@PRIMARY");
            let got = primary
                .forwarded()
                .recv_timeout(Duration::from_secs(20))
                .expect("a forwarded launch");
            let paths: Vec<String> = got
                .kairo_paths()
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            println!("\n@@RECEIVED {}", serde_json::to_string(&paths).unwrap());
        }
        Instance::Secondary => println!("\n@@SECONDARY"),
    }
}

/// The probe's next report (`PRIMARY`, `SECONDARY` or `RECEIVED …`), without its marker.
fn next_report(lines: &mut impl Iterator<Item = std::io::Result<String>>) -> String {
    lines
        .map_while(Result::ok)
        .find_map(|l| l.split_once("@@").map(|(_, report)| report.to_string()))
        .expect("the probe reported")
}

fn spawn(user_data: &Path, args: &[&str]) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "probe", "--nocapture", "--test-threads", "1"])
        .env(PROBE_ENV, user_data)
        .env("KAIRO_PROBE_ARGS", args.join("\n"))
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

#[test]
fn a_second_launch_forwards_its_kairo_file_to_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let user_data = dir.path().join("Kairo");
    let song = dir.path().join("Amazing Grace.kairo");
    std::fs::write(&song, "{}").unwrap();

    let mut first = spawn(&user_data, &[]);
    let mut first_out = BufReader::new(first.stdout.take().unwrap()).lines();
    assert_eq!(next_report(&mut first_out), "PRIMARY");

    let song_arg = song.to_string_lossy().into_owned();
    let second = spawn(&user_data, &["--some-flag", &song_arg])
        .wait_with_output()
        .unwrap();
    assert!(second.status.success());
    assert!(String::from_utf8_lossy(&second.stdout).contains("@@SECONDARY"));

    let received = next_report(&mut first_out);
    let paths: Vec<String> =
        serde_json::from_str(received.trim_start_matches("RECEIVED ")).unwrap();
    assert_eq!(paths, vec![song_arg]);
    assert!(first.wait().unwrap().success());

    // The first process has exited, so the lock is free again.
    let mut third = spawn(&user_data, &[]);
    let mut third_out = BufReader::new(third.stdout.take().unwrap()).lines();
    assert_eq!(next_report(&mut third_out), "PRIMARY");
    let _ = third.kill();
    let _ = third.wait();
}

#[test]
fn a_killed_first_instance_does_not_block_the_next() {
    let dir = tempfile::tempdir().unwrap();
    let user_data = dir.path().join("Kairo");
    let mut first = spawn(&user_data, &[]);
    let mut out = BufReader::new(first.stdout.take().unwrap()).lines();
    assert_eq!(next_report(&mut out), "PRIMARY");
    first.kill().unwrap();
    first.wait().unwrap();

    let mut next = spawn(&user_data, &[]);
    let mut out = BufReader::new(next.stdout.take().unwrap()).lines();
    assert_eq!(
        next_report(&mut out),
        "PRIMARY",
        "a crash must not leave Kairo unable to start"
    );
    let _ = next.kill();
    let _ = next.wait();
}
