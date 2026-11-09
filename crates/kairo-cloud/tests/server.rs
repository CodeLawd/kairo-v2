//! Opt-in test for a disposable Nest development server and fresh local MongoDB.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use kairo_cloud::{CloudConfig, CloudSession};
use kairo_core::Publisher;
use kairo_domain::cloud::{CloudSessionState, SignInInput, SignUpInput};
use kairo_net::{CloudApiClient, Method, RequestOptions};
use kairo_store::secrets::{KeyProtector, SecretError, SecureStore};
use serde_json::{Value, json};
use url::Url;

#[derive(Default)]
struct TestKeyring(Mutex<HashMap<String, [u8; 32]>>);

impl KeyProtector for TestKeyring {
    fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Ok(self.0.lock().unwrap().get(slot).copied())
    }

    fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(slot.to_owned(), *key);
        Ok(())
    }
}

#[tokio::test]
#[ignore = "requires disposable local Kairo Nest server and MongoDB"]
async fn native_refresh_rotates_and_replay_revokes_the_family() {
    let base = std::env::var("KAIRO_TEST_CLOUD_URL").expect("set KAIRO_TEST_CLOUD_URL");
    let url = Url::parse(&base).unwrap();
    assert!(matches!(url.host_str(), Some("127.0.0.1" | "[::1]")));
    let profile = tempfile::tempdir().unwrap();
    let keyring = Arc::new(TestKeyring::default());
    let vault = SecureStore::new(profile.path(), keyring.as_ref(), "Kairo");
    let session = CloudSession::open(
        CloudConfig::new(&base, &base, Some(&base)).unwrap(),
        profile.path().to_owned(),
        keyring.clone(),
        None,
        Publisher::default(),
    )
    .await
    .unwrap();

    let email = format!("fixture-{}@example.invalid", uuid::Uuid::new_v4());
    let password = "Fixture-password-23!".to_owned();
    let sign_up = session
        .sign_up(SignUpInput {
            email: email.clone(),
            password: password.clone(),
            name: "Fixture Operator".into(),
            org_name: "Fixture Church".into(),
        })
        .await;
    assert!(sign_up.ok, "sign-up failed: {:?}", sign_up.error);
    let invalid = session
        .sign_in(SignInInput {
            email,
            password: "incorrect".into(),
        })
        .await;
    assert!(!invalid.ok && invalid.error.is_some());
    assert_eq!(session.session().await.state, CloudSessionState::Active);

    let original = vault
        .read("refresh-token")
        .expect("sign-up persisted a refresh token");
    let (one, two) = tokio::join!(session.refresh(), session.refresh());
    assert!(one && two);
    let next = vault.read("refresh-token").unwrap();
    assert!(original != next, "refresh did not rotate");
    assert!(session.refresh().await);
    let latest = vault.read("refresh-token").unwrap();
    assert!(next != latest, "successor was not consumed");

    // The server deliberately allows the old token inside its race window.
    tokio::time::sleep(Duration::from_secs(32)).await;
    let api = CloudApiClient::new(&base, None).unwrap();
    let reused = api
        .request::<Value>(
            Method::POST,
            "/v1/auth/refresh",
            Some(json!({ "refreshToken": original })),
            RequestOptions {
                authenticated: false,
                allow_refresh: false,
                timeout: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(reused.status, Some(401));
    assert!(
        !session.refresh().await,
        "replay did not revoke the successor family"
    );
    assert_eq!(session.session().await.state, CloudSessionState::SignedOut);
    assert!(vault.read("refresh-token").is_none());
    session.stop().await;
}

#[tokio::test]
async fn offline_resume_managed_request_and_pairing_cancellation() {
    use kairo_cloud::{OpenBrowser, OrgProfilePatch};
    use kairo_domain::cloud::{DevicePairingState, DevicePairingStatus};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let refreshes = Arc::new(AtomicUsize::new(0));
    let polls = Arc::new(AtomicUsize::new(0));
    let user =
        json!({"id":"1","email":"fixture@example.invalid","name":"Fixture","emailVerified":true});
    let org = json!({"id":"2","name":"Fixture","role":"owner"});
    let worker = tokio::spawn({
        let refreshes = refreshes.clone();
        let polls = polls.clone();
        let base = base.clone();
        async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 2048];
                while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                    let read = socket.read(&mut buffer).await.unwrap();
                    if read == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buffer[..read]);
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or_default();
                let (status, body) = match path {
                    "/v1/auth/login" => (
                        "200 OK",
                        json!({
                            "accessToken":"initial","refreshToken":"first","expiresIn":"15m","user":user,"org":org,
                        }),
                    ),
                    "/v1/auth/refresh" if refreshes.fetch_add(1, Ordering::Relaxed) == 0 => {
                        ("503 Service Unavailable", json!({"message":"Starting"}))
                    }
                    "/v1/auth/refresh" => (
                        "200 OK",
                        json!({
                            "accessToken":"rotated","refreshToken":"second","expiresIn":"15m",
                        }),
                    ),
                    "/v1/auth/session" => ("200 OK", json!({"user":user,"orgId":"2","orgs":[org]})),
                    "/v1/managed/request" if request.contains("authorization: Bearer rotated") => {
                        ("200 OK", json!({"answer":42}))
                    }
                    "/v1/auth/device/start" => (
                        "200 OK",
                        json!({
                            "deviceCode":"secret","userCode":"PUBLIC",
                            "verificationUri":format!("{base}/device"),"interval":0.25,"expiresIn":1,
                        }),
                    ),
                    "/v1/auth/device/token" => {
                        polls.fetch_add(1, Ordering::Relaxed);
                        (
                            "200 OK",
                            json!({
                                "state":"approved", "accessToken":"rotated", "refreshToken":"paired",
                                "expiresIn":"15m", "user":user, "org":org,
                            }),
                        )
                    }
                    "/v1/orgs/2" if request.contains("authorization: Bearer rotated") => {
                        ("200 OK", json!({"id":"2","name":"Renamed"}))
                    }
                    "/v1/auth/logout" if request.contains("authorization: Bearer rotated") => {
                        ("200 OK", Value::Null)
                    }
                    _ => ("403 Forbidden", json!({"message":"Unexpected request"})),
                };
                let body = body.to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        }
    });
    let profile = tempfile::tempdir().unwrap();
    let topics = Publisher::default();
    let mut browser = topics.stream::<OpenBrowser>();
    let mut pairing = topics.watch::<DevicePairingState>();
    let session = CloudSession::open(
        CloudConfig::new(&base, &base, Some(&base)).unwrap(),
        profile.path().to_owned(),
        Arc::new(TestKeyring::default()),
        None,
        topics.clone(),
    )
    .await
    .unwrap();
    let result = session
        .sign_in(SignInInput {
            email: "fixture@example.invalid".into(),
            password: "fixture".into(),
        })
        .await;
    assert!(result.ok);
    assert!(!session.refresh().await);
    assert_eq!(session.session().await.state, CloudSessionState::Stale);
    assert!(!session.refresh().await);
    assert_eq!(refreshes.load(Ordering::Relaxed), 1);
    session.reconnect().await;
    assert!(session.refresh().await);

    let reply: Value = session
        .managed_request(Method::POST, "/v1/managed/request", Some(json!({})))
        .await
        .unwrap();
    assert_eq!(reply, json!({"answer":42}));
    assert_eq!(
        session.start_pairing().await.status,
        DevicePairingStatus::Waiting
    );
    assert_eq!(browser.next().await.unwrap().unwrap().0.path(), "/device");
    assert_eq!(
        session.cancel_pairing().await.status,
        DevicePairingStatus::Idle
    );
    tokio::time::sleep(Duration::from_millis(325)).await;
    assert_eq!(polls.load(Ordering::Relaxed), 0);
    assert_eq!(
        session.start_pairing().await.status,
        DevicePairingStatus::Waiting
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        while pairing.next().await.unwrap().status != DevicePairingStatus::Approved {}
    })
    .await
    .expect("pairing should adopt the approved session");
    assert_eq!(polls.load(Ordering::Relaxed), 1);
    session
        .sync_org_profile(OrgProfilePatch {
            name: Some("  Renamed  ".into()),
            ..Default::default()
        })
        .await;
    assert_eq!(session.session().await.org.unwrap().name, "Renamed");
    assert_eq!(session.sign_out().await.state, CloudSessionState::SignedOut);
    session.stop().await;
    worker.abort();
}
