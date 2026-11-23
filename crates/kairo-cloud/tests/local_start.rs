//! Local account restoration must not make first paint depend on the network.
use kairo_cloud::{CloudConfig, CloudSession, licensing::LicensingService};
use kairo_core::Publisher;
use kairo_domain::cloud::CloudSessionState;
use kairo_store::secrets::{KeyProtector, SecretError, SecureStore};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
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
async fn restored_account_and_licence_stay_local_until_explicit_start() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let keyring = Arc::new(Keyring::default());
    assert!(
        SecureStore::new(dir.path(), keyring.as_ref(), "Kairo").write("refresh-token", "fixture")
    );
    let topics = Publisher::default();
    let session = CloudSession::open_local(
        CloudConfig::new(&base, &base, Some(&base)).unwrap(),
        dir.path().to_owned(),
        keyring.clone(),
        None,
        topics.clone(),
    )
    .await
    .unwrap();
    let licence =
        LicensingService::open_local(session.clone(), dir.path().to_owned(), keyring, topics).await;
    assert_eq!(session.session().await.state, CloudSessionState::Stale);
    assert!(!licence.get_status().await.activated);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err(),
        "local construction contacted the server"
    );
    licence.start();
    licence.start();
    session.start().await;
    session.start().await;
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut request = vec![0; 4096];
    let count = socket.read(&mut request).await.unwrap();
    assert!(
        std::str::from_utf8(&request[..count])
            .unwrap()
            .starts_with("POST /v1/auth/refresh ")
    );
    socket.write_all(b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 2\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{}").await.unwrap();
    drop(socket);
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err(),
        "starting twice installed duplicate refresh loops"
    );
    session.stop().await;
}

#[tokio::test]
async fn stop_waits_for_inflight_refresh_to_persist_rotated_token() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let dir = tempfile::tempdir().unwrap();
    let keyring = Arc::new(Keyring::default());
    assert!(
        SecureStore::new(dir.path(), keyring.as_ref(), "Kairo").write("refresh-token", "initial")
    );
    let session = CloudSession::open_local(
        CloudConfig::new(&base, &base, Some(&base)).unwrap(),
        dir.path().to_owned(),
        keyring.clone(),
        None,
        Publisher::default(),
    )
    .await
    .unwrap();
    session.start().await;

    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut request = vec![0; 4096];
    let _ = socket.read(&mut request).await.unwrap();

    // Quit while /v1/auth/refresh is in flight; stop() must wait for the
    // rotated token to be written before returning.
    let stopping = tokio::spawn({
        let session = session.clone();
        async move { session.stop().await }
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(!stopping.is_finished());

    let body = r#"{"accessToken":"next-access","refreshToken":"rotated-token","expiresIn":"15m"}"#;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await.unwrap();
    drop(socket);

    tokio::time::timeout(Duration::from_secs(2), stopping)
        .await
        .expect("stop should finish once refresh completes")
        .unwrap();
    assert_eq!(
        SecureStore::new(dir.path(), keyring.as_ref(), "Kairo")
            .read("refresh-token")
            .as_deref(),
        Some("rotated-token")
    );
}
