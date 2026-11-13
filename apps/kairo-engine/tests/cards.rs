//! Video card surfaces in the real engine: the engine owns the card service,
//! spawns the helper and reports readiness and its reasons. A shell script
//! stands in for `decklink-helper` (no card is needed); with Blackmagic
//! Desktop Video installed, the real helper is also asked for a port that
//! does not exist.
#![cfg(unix)]

use kairo_core::engine::*;
use kairo_engine_proto::*;
use std::path::Path;
use std::time::Duration;

const ENGINE: &str = env!("CARGO_BIN_EXE_kairo-engine");

fn card(output: &str, port: &str, mode: &str) -> Command {
    Command::OpenSurface(SurfaceSpec {
        output: output.into(),
        label: "Card".into(),
        sink: SinkSpec::Device {
            port_id: port.into(),
            mode_id: mode.into(),
            key_fill: false,
        },
    })
}

fn script(dir: &Path, log: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("helper");
    std::fs::write(
        &path,
        format!(
            r#"#!/bin/sh
case "$1" in
--version) echo '{{"version":"16.4"}}';;
--list) echo '{{"version":"16.4","ports":[{{"id":"decklink:t:0","provider":"decklink","deviceName":"Test","portName":"SDI 1","supportsKeyFill":false,"modes":[{{"id":"decklink:1","label":"1080p25","width":1920,"height":1080,"frameRateN":25,"frameRateD":1,"interlaced":false}}],"unavailableReason":null}}],"compatibilityMode":false}}';;
--play-shm)
  if [ "$2" != "decklink:t:0" ]; then echo 'Device disconnected' >&2; exit 1; fi
  echo "open $2 $3 $4" >> '{log}'
  echo "$$" > '{log}.pid'
  echo '{{"event":"ring","ok":true}}'
  echo '{{"event":"started","port":"'"$2"'"}}'
  echo '{{"event":"stats","completed":25,"late":0,"dropped":0,"repeated":0,"received":25,"outputFps":25}}'
  cat > /dev/null
  echo "released $2" >> '{log}';;
esac
"#,
            log = log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[tokio::test(flavor = "multi_thread")]
async fn card_surfaces_acquire_report_release_and_say_why() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    // SAFETY: this test binary holds this one test, so no other thread
    // reads the environment while it is set.
    unsafe { std::env::set_var("KAIRO_DECKLINK_HELPER", script(dir.path(), &log)) };
    let mut options = EngineOptions::new(ENGINE);
    options.grace = Duration::from_secs(2);
    let engine = EngineHandle::spawn(options);
    let mut events = engine.subscribe();
    let mut status_of = async |output: &str| -> SurfaceStatus {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let event = tokio::time::timeout_at(deadline, events.recv())
                .await
                .unwrap_or_else(|_| panic!("no status for {output}"))
                .expect("events");
            if let EngineEvent::Engine(Event::SurfaceStatus { output: o, status }) = event
                && o == output
                && status
                    != (SurfaceStatus::Unavailable {
                        reason: "Output unavailable".into(),
                    })
            {
                return status;
            }
        }
    };

    if !Path::new("/Library/Frameworks/DeckLinkAPI.framework").exists() {
        // Without Desktop Video the provider says to install it.
        engine.command(card("a", "decklink:t:0", "decklink:1"));
        let SurfaceStatus::Unavailable { reason } = status_of("a").await else {
            panic!("a card without Desktop Video cannot be ready");
        };
        assert_eq!(
            reason,
            "Install Blackmagic Desktop Video to use DeckLink cards"
        );
        engine.shutdown();
        return;
    }

    // An unknown mode and an unknown port say why.
    engine.command(card("b", "decklink:t:0", "decklink:99"));
    assert_eq!(
        status_of("b").await,
        SurfaceStatus::Unavailable {
            reason: "Choose a supported video mode".into()
        }
    );
    engine.command(card("b", "decklink:gone:0", "decklink:1"));
    assert_eq!(
        status_of("b").await,
        SurfaceStatus::Unavailable {
            reason: "Device disconnected".into()
        }
    );

    engine.command(card("a", "decklink:t:0", "decklink:1"));
    assert_eq!(status_of("a").await, SurfaceStatus::Ready);
    let opened = std::fs::read_to_string(&log).unwrap();
    assert!(
        opened.starts_with("open decklink:t:0 decklink:1 /kairo-dl-"),
        "{opened}"
    );

    // A second output on the same port is refused, with Electron's reason
    // (ownership is checked before the mode).
    engine.command(card("b", "decklink:t:0", "decklink:1"));
    assert_eq!(
        status_of("b").await,
        SurfaceStatus::Unavailable {
            reason: "Port assigned to another output".into()
        }
    );

    // The snapshot names the driver, the port and the hardware counters.
    // The helper's first stats line has arrived by now or soon after.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let snapshot = tokio::time::timeout(Duration::from_secs(5), engine.query(Query::VideoDevices))
        .await
        .expect("in time")
        .expect("reply");
    let Reply::VideoDevices(json) = snapshot else {
        panic!("{snapshot:?}");
    };
    let snapshot: serde_json::Value = json.decode().unwrap();
    assert_eq!(snapshot["ports"][0]["id"], "decklink:t:0");
    assert_eq!(snapshot["providers"][0]["driver"]["state"], "current");
    assert_eq!(snapshot["outputs"]["a"]["ready"], true);
    assert_eq!(
        snapshot["outputs"]["a"]["diagnostics"]["hardware"]["completed"],
        25.0
    );

    engine.command(Command::CloseSurface {
        output: "b".into(),
        destroy: true,
    });
    // The helper dying faults the output with its reason; a refresh
    // (the operator's "refresh") acquires the port again and the surface
    // shows its state once more.
    let pid: i32 = std::fs::read_to_string(dir.path().join("log.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    // SAFETY: our own helper process.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    let SurfaceStatus::Unavailable { reason } = status_of("a").await else {
        panic!("a dead helper is not ready");
    };
    assert!(reason.contains("DeckLink helper exited"), "{reason}");
    engine.command(Command::RefreshVideoDevices);
    assert_eq!(status_of("a").await, SurfaceStatus::Ready);

    // Closing the surface releases the port (the helper reads EOF).
    engine.command(Command::CloseSurface {
        output: "a".into(),
        destroy: true,
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !std::fs::read_to_string(&log)
        .unwrap()
        .contains("released decklink:t:0")
    {
        assert!(std::time::Instant::now() < deadline, "port not released");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    engine.command(Command::RefreshVideoDevices);
    engine.shutdown();
}
