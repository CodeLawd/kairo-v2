//! Live smoke test against the real sources. Non-blocking: ignored by
//! default, needs the network. `cargo test -p kairo-lyrics --test online_live -- --ignored`

use kairo_lyrics::online::http::{Http, ReqwestHttp};
use kairo_lyrics::online::{genius::Genius, lrclib::Lrclib, web_lyrics::scrape_lyrics_page};
use std::sync::Arc;

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
