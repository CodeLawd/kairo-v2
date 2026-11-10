//! The slide converter's download against a local server: checksum, HTTP
//! failure and status. Nothing reaches TDF.

use kairo_documents::slide_converter::{Build, BuildKind, SLIDE_CONVERTER_VERSION, SlideConverter};
use kairo_domain::documents::{PowerpointConverterId, SlideConverterState};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Serves `status` and `body` to every request; returns the base URL.
async fn serve(status: &'static str, body: Vec<u8>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            let body = body.clone();
            tokio::spawn(async move {
                let mut request = [0u8; 4096];
                let _ = socket.read(&mut request).await;
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
            });
        }
    });
    format!("http://{addr}")
}

fn build(url: String) -> Option<Build> {
    Some(Build {
        url,
        sha256: "00".repeat(32),
        size_bytes: 5,
        kind: BuildKind::Dmg,
    })
}

#[tokio::test]
async fn damaged_and_failed_downloads_report_plainly() {
    let user = tempfile::tempdir().unwrap();
    let http = reqwest::Client::new();
    let url = serve("200 OK", b"bytes".to_vec()).await;
    let converter =
        SlideConverter::with_build(user.path(), http.clone(), build(format!("{url}/lo.dmg")));
    converter.set_detect_others(|| Some(PowerpointConverterId::Keynote));
    let before = converter.status();
    assert_eq!(
        (before.state, before.version.as_str()),
        (SlideConverterState::Missing, SLIDE_CONVERTER_VERSION)
    );
    assert_eq!(before.other_converter, Some(PowerpointConverterId::Keynote));
    let mut changes = converter.subscribe();
    let after = converter.install().await;
    assert_eq!(after.state, SlideConverterState::Error);
    assert_eq!(
        after.error.as_deref(),
        Some("The download was damaged. Try again.")
    );
    assert_eq!(after.received_bytes, 0.0);
    assert_eq!(
        changes.recv().await.unwrap().state,
        SlideConverterState::Downloading
    );
    // The partial download is gone.
    assert!(!user.path().join("slide-converter/download.dmg").exists());
    let removed = converter.remove().await;
    assert_eq!(
        (removed.state, removed.error),
        (SlideConverterState::Missing, None)
    );
    assert!(!user.path().join("slide-converter").exists());

    let url = serve("404 Not Found", Vec::new()).await;
    let converter = SlideConverter::with_build(user.path(), http.clone(), build(url));
    assert_eq!(
        converter.install().await.error.as_deref(),
        Some("The download stopped. Check the internet connection and try again.")
    );
    let unsupported = SlideConverter::with_build(user.path(), http, None);
    assert_eq!(unsupported.status().state, SlideConverterState::Unsupported);
    assert_eq!(
        unsupported.install().await.state,
        SlideConverterState::Unsupported
    );
}
