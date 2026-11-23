//! Live smoke test against the real sources. Non-blocking: ignored by
//! default, needs the network. `cargo test -p kairo-lyrics --test online_live -- --ignored`

use kairo_lyrics::online::http::{BoxFuture, GatewayError, Http, ManagedGateway, ReqwestHttp};
use kairo_lyrics::online::{
    OnlineLyrics, OnlineSource, genius::Genius, lrclib::Lrclib,
    web_lyrics::{scrape_lyrics_for_review, scrape_lyrics_page},
};
use kairo_domain::cloud::ManagedRequest;
use serde_json::Value;
use std::sync::Arc;

struct NoGateway;
impl ManagedGateway for NoGateway {
    fn request(&self, _r: ManagedRequest) -> BoxFuture<'_, Result<Value, GatewayError>> {
        Box::pin(async {
            Err(GatewayError {
                status: None,
                message: "no managed gateway in unit test".into(),
            })
        })
    }
}

#[tokio::test]
#[ignore = "needs network"]
async fn live_sources_answer() {
    let http: Arc<dyn Http> = Arc::new(ReqwestHttp::default());
    let lrclib = Lrclib::new(http.clone());
    let hits = lrclib.search("amazing grace").await.expect("lrclib search");
    assert!(!hits.is_empty(), "lrclib returned nothing");
    let lyrics = lrclib
        .fetch_lyrics(&hits[0].url)
        .await
        .expect("lrclib lyrics");
    assert!(!lyrics.is_empty());

    let page = scrape_lyrics_page(
        &http,
        "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html",
    )
    .await
    .expect("hymn page");
    assert!(page.lyrics.contains("[Verse 1]"), "{}", page.lyrics);

    // Genius answers this machine with 403 at times; report rather than fail.
    match Genius::new(http).search("amazing grace").await {
        Ok(results) => eprintln!("genius: {} results", results.len()),
        Err(error) => eprintln!("genius unavailable: {error}"),
    }
}

#[tokio::test]
#[ignore = "needs network"]
async fn live_multilingual_queries() {
    let http: Arc<dyn Http> = Arc::new(ReqwestHttp::default());
    let online = OnlineLyrics::new(http, Arc::new(NoGateway));
    let queries = [
        ("Hausa", "Nagode Solomon Lange"),
        ("Yoruba-typo", "Ami oluwa sunmisola"),
        ("Igbo-phonetic", "Nara Ekele Tim Godfrey"),
        ("Igbo-snippet", "Narekele mo"),
        ("Igbo-typo", "Ekweme Prospa"),
        ("Spanish-typo", "Cuan grnde es dios"),
        ("Spanish", "Al Que Esta Sentado En El Trono Marcos Brunet"),
    ];
    for (lang, query) in queries {
        eprintln!("\n=== [{lang}] {query} ===");
        match online.search_all(query).await {
            Ok(results) => {
                eprintln!("  hits: {}", results.len());
                for (i, r) in results.iter().take(3).enumerate() {
                    eprintln!(
                        "  [{i}] {:?} | {} — {} | avail={:?} | {}",
                        r.provider, r.title, r.artist, r.lyrics_available, r.url
                    );
                }
                for (idx, hit) in results
                    .iter()
                    .filter(|r| r.lyrics_available != Some(false))
                    .take(2)
                    .enumerate()
                {
                    if idx > 0 && hit.provider != kairo_domain::lyrics::LyricsProvider::Web {
                        continue;
                    }
                    let src = OnlineSource {
                        provider: hit.provider,
                        url: hit.url.clone(),
                        title: hit.title.clone(),
                        artist: hit.artist.clone(),
                    };
                    match online.prepare(&src, "").await {
                        Ok(prepared) => {
                            let lines: Vec<&str> = prepared.text.lines().collect();
                            eprintln!(
                                "  prepared[{idx}] ({:?}): review={:?}, lines={}, chars={}",
                                hit.provider,
                                prepared.review,
                                lines.len(),
                                prepared.text.len()
                            );
                            for line in lines.iter().take(10) {
                                eprintln!("    | {line}");
                            }
                            if lines.len() > 14 {
                                eprintln!("    | ... ({} middle lines)", lines.len() - 14);
                                for line in lines.iter().skip(lines.len().saturating_sub(4)) {
                                    eprintln!("    | {line}");
                                }
                            }
                        }
                        Err(err) => eprintln!("  prepare[{idx}] error: {err}"),
                    }
                }
            }
            Err(err) => eprintln!("  search error: {err}"),
        }
    }
}

#[tokio::test]
#[ignore = "needs network"]
async fn live_web_scrape_audit() {
    let http: Arc<dyn Http> = Arc::new(ReqwestHttp::default());
    let urls = [
        ("Hausa-blogspot", "https://moseskogis.blogspot.com/2012/04/nagode-lyrics-solomon-lange.html"),
        ("Hausa-gospelsongs", "https://gospelsongs.com.ng/na-gode-lyrics-solomon-lange/"),
        ("Yoruba-hymn", "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html"),
    ];
    for (label, url) in urls {
        eprintln!("\n=== [{label}] {url} ===");
        match scrape_lyrics_for_review(&http, url).await {
            Ok(page) => {
                let lines: Vec<&str> = page.lyrics.lines().collect();
                eprintln!(
                    "  title={:?} artist={:?} lines={} chars={}",
                    page.title,
                    page.artist,
                    lines.len(),
                    page.lyrics.len()
                );
                for (i, line) in lines.iter().enumerate().take(25) {
                    eprintln!("  {i:3}: {line}");
                }
                if lines.len() > 35 {
                    eprintln!("  ... ({} middle lines)", lines.len() - 35);
                    for (i, line) in lines.iter().enumerate().skip(lines.len() - 10) {
                        eprintln!("  {i:3}: {line}");
                    }
                }
            }
            Err(err) => eprintln!("  error: {err}"),
        }
    }
}
