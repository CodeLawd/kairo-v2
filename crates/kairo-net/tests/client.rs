use std::{
    future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use kairo_net::{CloudApiClient, Method, RequestOptions};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    task::JoinHandle,
    time::sleep,
};

type Reply = (Duration, &'static str, &'static str);

fn immediate(status: &'static str, body: &'static str) -> Reply {
    (Duration::ZERO, status, body)
}

struct Server {
    url: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let task = tokio::spawn({
            let requests = requests.clone();
            let reply = Arc::new(reply);
            async move {
                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let requests = requests.clone();
                    let reply = reply.clone();
                    tokio::spawn(async move {
                        let mut bytes = Vec::new();
                        let mut chunk = [0; 1024];
                        while !bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                            let count = socket.read(&mut chunk).await.unwrap();
                            if count == 0 {
                                return;
                            }
                            bytes.extend_from_slice(&chunk[..count]);
                        }
                        let request = String::from_utf8_lossy(&bytes);
                        let (delay, status, body) = reply(&request);
                        requests.lock().unwrap().push(request.into_owned());
                        sleep(delay).await;
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    });
                }
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| {
                request
                    .lines()
                    .next()
                    .unwrap()
                    .trim_end_matches(" HTTP/1.1")
                    .to_owned()
            })
            .collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn anonymous() -> RequestOptions {
    RequestOptions {
        authenticated: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn selects_backup_before_write_and_rewrites_primary_absolute_path() {
    let primary = Server::start(|_| immediate("503 Service Unavailable", "{}")).await;
    let backup = Server::start(|_| immediate("200 OK", r#"{"ok":true}"#)).await;
    let api = CloudApiClient::new(&primary.url, Some(&backup.url)).unwrap();

    let reply: Value = api
        .request(
            Method::POST,
            &format!("{}/v1/engage", primary.url),
            Some(json!({})),
            anonymous(),
        )
        .await
        .unwrap();
    assert_eq!(reply, json!({"ok": true}));
    api.request::<Value>(Method::GET, "/v1/licensing", None, anonymous())
        .await
        .unwrap();
    assert_eq!(primary.paths(), ["GET /v1/health"]);
    assert_eq!(
        backup.paths(),
        ["GET /v1/health", "POST /v1/engage", "GET /v1/licensing"]
    );
    assert!(
        backup
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.contains("x-pa-client: desktop"))
    );
}

#[tokio::test]
async fn timed_out_write_never_reaches_backup() {
    let primary = Server::start(|request| {
        if request.starts_with("POST") {
            (Duration::from_secs(2), "200 OK", "{}")
        } else {
            immediate("200 OK", "{}")
        }
    })
    .await;
    let backup = Server::start(|_| immediate("200 OK", "{}")).await;
    let api = CloudApiClient::new(&primary.url, Some(&backup.url)).unwrap();
    let options = RequestOptions {
        timeout: Some(Duration::from_millis(100)),
        ..anonymous()
    };
    let error = api
        .request::<Value>(Method::POST, "/v1/engage", None, options)
        .await
        .unwrap_err();
    assert_eq!(error.status, None);
    assert_eq!(primary.paths(), ["GET /v1/health", "POST /v1/engage"]);
    assert!(backup.paths().is_empty());
}

#[tokio::test]
async fn refresh_retry_preserves_first_server_validation_message() {
    let server = Server::start(|request| {
        if request.contains("authorization: Bearer fresh") {
            immediate(
                "403 Forbidden",
                r#"{"message":["Activation required","other"]}"#,
            )
        } else {
            immediate("401 Unauthorized", "{}")
        }
    })
    .await;
    let api = CloudApiClient::new(&server.url, None).unwrap();
    api.set_access_token(Some("stale".into()));
    let error = api
        .request_with_refresh::<Value, _, _>(
            Method::POST,
            "/v1/managed",
            None,
            RequestOptions::default(),
            || future::ready(Some("fresh".into())),
        )
        .await
        .unwrap_err();
    assert_eq!(error.status, Some(403));
    assert_eq!(error.message, "Activation required");
}

#[tokio::test]
async fn simultaneous_401s_refresh_once_even_when_one_response_is_late() {
    let server = Server::start(|request| {
        if request.contains("authorization: Bearer fresh") {
            immediate("200 OK", "{}")
        } else if request.starts_with("GET /late ") {
            (Duration::from_millis(250), "401 Unauthorized", "{}")
        } else {
            immediate("401 Unauthorized", "{}")
        }
    })
    .await;
    let api = CloudApiClient::new(&server.url, None).unwrap();
    api.set_access_token(Some("stale".into()));
    let refreshes = Arc::new(AtomicUsize::new(0));
    let request = |path| {
        let api = api.clone();
        let refreshes = refreshes.clone();
        async move {
            api.request_with_refresh::<Value, _, _>(
                Method::GET,
                path,
                None,
                RequestOptions::default(),
                move || async move {
                    refreshes.fetch_add(1, Ordering::Relaxed);
                    sleep(Duration::from_millis(70)).await;
                    Some("fresh".into())
                },
            )
            .await
            .unwrap()
        }
    };
    tokio::join!(request("/first"), request("/second"), request("/late"));
    assert_eq!(refreshes.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn transport_failure_reselects_backup_for_a_read() {
    let unhealthy = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let primary = Server::start({
        let unhealthy = unhealthy.clone();
        move |request| {
            if request.starts_with("GET /v1/health ") {
                if unhealthy.load(Ordering::Relaxed) {
                    immediate("503 Service Unavailable", "{}")
                } else {
                    immediate("200 OK", "{}")
                }
            } else {
                unhealthy.store(true, Ordering::Relaxed);
                (Duration::from_secs(2), "200 OK", "{}")
            }
        }
    })
    .await;
    let backup = Server::start(|_| immediate("200 OK", r#"{"backup":true}"#)).await;
    let api = CloudApiClient::new(&primary.url, Some(&backup.url)).unwrap();
    let reply: Value = api
        .request(
            Method::GET,
            "/v1/read",
            None,
            RequestOptions {
                timeout: Some(Duration::from_millis(200)),
                ..anonymous()
            },
        )
        .await
        .unwrap();
    assert_eq!(reply, json!({"backup":true}));
    assert_eq!(
        primary.paths(),
        ["GET /v1/health", "GET /v1/read", "GET /v1/health"]
    );
    assert_eq!(backup.paths(), ["GET /v1/health", "GET /v1/read"]);
}

#[tokio::test]
async fn retries_do_not_cross_an_account_change() {
    let server = Server::start(|request| {
        if request.contains("authorization: Bearer replaced") {
            immediate("200 OK", "{}")
        } else {
            (Duration::from_millis(60), "401 Unauthorized", "{}")
        }
    })
    .await;
    let api = CloudApiClient::new(&server.url, None).unwrap();
    api.set_access_token(Some("old".into()));
    let send = api.request_with_refresh::<Value, _, _>(
        Method::POST,
        "/v1/managed/request",
        None,
        RequestOptions::default(),
        || async { panic!("old account must not refresh") },
    );
    let change = async {
        while server.paths().is_empty() {
            sleep(Duration::from_millis(1)).await;
        }
        api.set_access_token(Some("replaced".into()));
    };
    let (result, _) = tokio::join!(send, change);
    assert_eq!(result.unwrap_err().status, Some(401));
    assert_eq!(server.paths().len(), 1);
}
