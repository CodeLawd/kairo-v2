use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use kairo_cloud::{
    CloudConfig, CloudSession,
    telemetry::{CrashContext, CrashReason, NativeCrash, Telemetry},
};
use kairo_core::Publisher;
use kairo_domain::{cloud::SignInInput, usage::UsageFeature};
use kairo_store::{
    crash::{CrashEntry, record_crash},
    paths::CRASH_LEDGER_FILE_NAME,
    secrets::{KeyProtector, SecretError},
    settings::{LaunchContext, SettingsStore, now_ms, random_uuid},
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Keyring(Mutex<Option<[u8; 32]>>);
impl KeyProtector for Keyring {
    fn load(&self, _: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Ok(*self.0.lock().unwrap())
    }

    fn save(&self, _: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        *self.0.lock().unwrap() = Some(*key);
        Ok(())
    }
}

#[tokio::test]
async fn opt_out_discards_pending_data_and_uploads_no_reports() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
        let server = tokio::spawn({
            let received = received.clone();
            async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let mut chunk = [0; 4096];
                    let (end, length, path) = loop {
                        let n = socket.read(&mut chunk).await.unwrap();
                        assert_ne!(n, 0);
                        bytes.extend_from_slice(&chunk[..n]);
                        if let Some(end) = bytes.windows(4).position(|slice| slice == b"\r\n\r\n") {
                            let header = std::str::from_utf8(&bytes[..end]).unwrap();
                            let path = header.split_whitespace().nth(1).unwrap().to_owned();
                            let length = header.lines().find_map(|line| {
                                line.strip_prefix("content-length: ")?.parse::<usize>().ok()
                            }).unwrap_or(0);
                            break (end + 4, length, path);
                        }
                    };
                    while bytes.len() < end + length {
                        let n = socket.read(&mut chunk).await.unwrap();
                        assert_ne!(n, 0);
                        bytes.extend_from_slice(&chunk[..n]);
                    }
                    let payload: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap_or_default();
                    let response = match path.as_str() {
                        "/v1/auth/login" => json!({
                            "accessToken":"fixture","refreshToken":"fixture","expiresIn":"15m",
                            "user":{"id":"u","email":"fixture@example.invalid","name":"Fixture","emailVerified":true},
                            "org":{"id":"o","name":"Fixture","role":"owner"}
                        }),
                        "/v1/usage/report" => json!({"accepted": payload["days"].as_array().unwrap().iter().map(|day| day["day"].clone()).collect::<Vec<_>>()}),
                        "/v1/crashes" => json!({"accepted": payload["reports"].as_array().unwrap().iter().map(|report| report["id"].clone()).collect::<Vec<_>>()}),
                        _ => panic!("unexpected fixture request"),
                    }.to_string();
                    received.lock().unwrap().push((path, payload));
                    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{response}", response.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let settings = Arc::new(Mutex::new(SettingsStore::open(dir.path(), &mut LaunchContext {
            now_ms: now_ms(), new_id: &mut random_uuid,
        })));
        let cloud = CloudSession::open(
            CloudConfig::new(&base, &base, Some(&base)).unwrap(), dir.path().to_owned(),
            Arc::new(Keyring::default()), Some(settings.clone()), Publisher::default(),
        ).await.unwrap();
        assert!(cloud.sign_in(SignInInput { email: "fixture@example.invalid".into(), password: "fixture".into() }).await.ok);
        let reporting = Telemetry::open(cloud.clone(), dir.path().to_owned(), settings.clone()).await.unwrap();
        reporting.track(UsageFeature::LyricsSlide, 17.0).await;
        reporting.note_error(CrashContext::Network).await;
        reporting.record_crash(NativeCrash::Engine, CrashReason::AbnormalExit, 1).await;

        settings.lock().unwrap().set_raw("usage", json!({"shareStats":false,"sendCrashReports":false}));
        reporting.track(UsageFeature::LyricsSlide, 10.0).await;
        reporting.record_crash(NativeCrash::WebHost, CrashReason::AbnormalExit, 2).await;
        record_crash(&dir.path().join(CRASH_LEDGER_FILE_NAME), CrashEntry {
            at: chrono::Utc::now().to_rfc3339(),
            thread: "fixture".into(), message: "synthetic local-only payload".into(),
            location: None, app_version: "0.1.0".into(),
        }).unwrap();
        reporting.flush_usage().await;
        reporting.flush_crashes().await;
        settings.lock().unwrap().set_raw("usage", json!({"shareStats":true,"sendCrashReports":true}));
        drop(reporting);
        let reporting = Telemetry::open(cloud, dir.path().to_owned(), settings.clone()).await.unwrap();
        reporting.flush_usage().await;
        reporting.flush_crashes().await;
        assert_eq!(received.lock().unwrap().len(), 1, "opt-out must drop the backlog");

        reporting.track(UsageFeature::LyricsSlide, 1.0).await;
        reporting.note_error(CrashContext::Graphics).await;
        reporting.record_crash(NativeCrash::Engine, CrashReason::AbnormalExit, 3).await;
        reporting.flush_usage().await;
        reporting.flush_crashes().await;
        reporting.flush_crashes().await;
        let received = received.lock().unwrap();
        assert_eq!(received.len(), 3);
        assert_eq!(received[1].1["days"][0]["counts"]["lyrics_slide"], 1.0);
        assert_eq!(received[2].1["reports"][0]["process"], "engine");
        assert_eq!(received[2].1["reports"][0]["errors"], json!(["graphics error"]));
        assert_eq!(received[2].1["reports"][0]["log"], "");
        server.abort();
    }).await.expect("fixture finished");
}
