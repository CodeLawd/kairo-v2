//! A local page for the shared live web layer, so the engine's web view has
//! content processes to lose. Served over plain HTTP on the loopback.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>Kairo soak</title>
<style>body{margin:0;background:#0b1320;color:#e6edf3;font:64px system-ui;display:grid;place-items:center;height:100vh}
#t{font-variant-numeric:tabular-nums}</style></head>
<body><div>Soak web layer <span id="t">0</span></div>
<script>let n=0;setInterval(()=>{document.getElementById('t').textContent=++n},250)</script></body></html>"#;

/// Starts the server; returns its page address.
pub async fn serve() -> std::io::Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://localhost:{}/soak", listener.local_addr()?.port());
    tokio::spawn(async move {
        while let Ok((mut socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = [0u8; 2048];
                // One request per connection; the page is the same for any path.
                if socket.read(&mut request).await.unwrap_or(0) == 0 {
                    return;
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{PAGE}",
                    PAGE.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn serves_the_page_at_an_address_the_web_layer_accepts() {
        let url = serve().await.unwrap();
        assert!(
            kairo_domain::web::web_address(&url, false).is_some(),
            "{url}"
        );
        let port = url.split(':').nth(2).unwrap().split('/').next().unwrap();
        let mut socket = tokio::net::TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        socket
            .write_all(b"GET /soak HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .await
            .unwrap();
        let mut body = String::new();
        socket.read_to_string(&mut body).await.unwrap();
        assert!(body.starts_with("HTTP/1.1 200 OK"));
        assert!(body.contains("Soak web layer"));
    }
}
