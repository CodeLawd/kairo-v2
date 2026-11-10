//! Fixture differential against Electron's online lyric code
//! (`oracle:lyrics-online`, prompt 041). Recorded pages live in
//! `tests/fixtures/online`; the TypeScript runs over the same bytes.

use kairo_domain::lyrics::LyricsProvider;
use kairo_lyrics::online::http::{
    BoxFuture, GatewayError, Http, HttpError, HttpRequest, HttpResponse, ManagedGateway,
};
use kairo_lyrics::online::rank::{
    LyricMatch, core_title, find_lyric_match, needs_web_resolution, rank_results,
};
use kairo_lyrics::online::resolver::{
    clean_page_title, discover_song_pages, looks_like_challenge, parse_brave_html, parse_duck_html,
    parse_mojeek_html,
};
use kairo_lyrics::online::web_lyrics::{
    clean_scraped_lyrics, is_allowed_lyrics_url, parse_page_heading, structure_bilingual_hymn,
};
use kairo_lyrics::online::web_search::{WebSearch, WebSearchResult};
use kairo_lyrics::online::{ProviderResult, genius, glosses, lrclib, normalize};
use kairo_oracle::assert_all_match;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/online/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// Serves recorded bodies by URL (query ignored); 404 for anything else.
struct Pages(HashMap<String, String>);

impl Http for Pages {
    fn get(
        &self,
        request: HttpRequest,
        _accept: fn(u16) -> bool,
    ) -> BoxFuture<'_, Result<HttpResponse, HttpError>> {
        let body = self.0.get(&request.url).cloned();
        Box::pin(async move {
            body.map(|body| HttpResponse { status: 200, body })
                .ok_or(HttpError::Status {
                    status: 404,
                    body: String::new(),
                })
        })
    }
}

struct NoGateway;

impl ManagedGateway for NoGateway {
    fn request(
        &self,
        _r: kairo_domain::cloud::ManagedRequest,
    ) -> BoxFuture<'_, Result<Value, GatewayError>> {
        Box::pin(async {
            Err(GatewayError {
                status: None,
                message: "offline".into(),
            })
        })
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
}

/// Lines and blocks from every fixture plus hostile strings.
fn corpus() -> Vec<String> {
    let mut out: Vec<String> = [
        "Èsé! èsé, ÈSÉ o",
        "[Chorus: Some Artist]",
        "[Verse 2: A & B]",
        "[Refrão]",
        "[Instrumental]",
        "[Spoken]",
        "[Pre Chorus 3]",
        "[Interlude Musical]",
        "Instruments playing",
        "Speaking in tongues.",
        "  \u{a0} ",
        "Ese (Thank You) | NATHANIEL BASSEY feat. AIDEE IME - #nathanielbassey",
        "Way Maker (Official Video) #sinach #worship",
        "DOWNLOAD SONG: Nathaniel Bassey - Ese (Mp3 & Lyrics) | CeeNaija",
        "Odudu (Spirit) Lyrics by Theophilus Sunday | African Gospel Lyrics",
        "Amioluwa (The Mark of God) Sunmisola Agbebi and Yinka Okeleye",
        "Dunsin Oyekan - Baruch Hashem Adonai",
        "Odudu dabu Jesus no (There is no other Name like Jesus) x2\nChorus: Amioluwa  Amioluwa\nRefrain:\nVerse  one  two",
        "😀 emoji [Verse 1]line\r\nnext\rlast",
        "Ese - Live",
        "ESE feat. AIDEE IME",
        "Amazing Grace (Remastered 2009) [Radio Edit]",
    ]
    .map(String::from)
    .to_vec();
    let flat = (1..=11)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    out.push(flat);
    out.push("[Chorus]\nEse o\nThank you\n\n[Verse 1]\nOne\n\n[Chorus]\nEse o\nThank you\nAmen\n\n[Instrumental]\n\n[Bridge]\n".into());
    out.push("a\n\nb\n\n\n\nc".into());
    out
}

#[test]
fn normalization_matches_electron() {
    let mut canonical = Vec::new();
    let mut structure = Vec::new();
    let mut header = Vec::new();
    let mut dedupe = Vec::new();
    let mut lines = Vec::new();
    let mut track = Vec::new();
    let mut page_title = Vec::new();
    let mut heading = Vec::new();
    let mut cleaned = Vec::new();
    let mut hymn = Vec::new();
    let mut core = Vec::new();
    let mut genius_text = Vec::new();
    for s in corpus() {
        let a = json!([s]);
        canonical.push((a.clone(), json!(normalize::canonical_text(&s))));
        structure.push((a.clone(), json!(normalize::structure_lyrics(&s))));
        header.push((a.clone(), json!(normalize::normalize_section_header(&s))));
        dedupe.push((a.clone(), json!(normalize::dedupe_sections(&s))));
        lines.push((a.clone(), json!(normalize::extract_lyric_lines(&s))));
        track.push((a.clone(), json!(lrclib::clean_track_title(&s))));
        page_title.push((a.clone(), json!(clean_page_title(&s))));
        let (t, ar) = parse_page_heading(&s);
        heading.push((a.clone(), json!({ "title": t, "artist": ar })));
        cleaned.push((a.clone(), json!(clean_scraped_lyrics(&s))));
        hymn.push((a.clone(), json!(structure_bilingual_hymn(&s))));
        core.push((a.clone(), json!(core_title(&s))));
        genius_text.push((a, json!(genius::normalize_genius_lyrics(&s))));
    }
    assert_all_match("oracle:lyrics-online.canonicalText", &canonical);
    assert_all_match("oracle:lyrics-online.structureLyrics", &structure);
    assert_all_match("oracle:lyrics-online.normalizeSectionHeader", &header);
    assert_all_match("oracle:lyrics-online.dedupeSections", &dedupe);
    assert_all_match("oracle:lyrics-online.extractLyricLines", &lines);
    assert_all_match("oracle:lyrics-online.cleanTrackTitle", &track);
    assert_all_match("oracle:lyrics-online.cleanPageTitle", &page_title);
    assert_all_match("oracle:lyrics-online.parsePageHeading", &heading);
    assert_all_match("oracle:lyrics-online.cleanScrapedLyrics", &cleaned);
    assert_all_match("oracle:lyrics-online.structureBilingualHymn", &hymn);
    assert_all_match("oracle:lyrics-online.coreTitle", &core);
    assert_all_match("oracle:lyrics-online.normalizeGeniusLyrics", &genius_text);
}

fn search_json(results: &[WebSearchResult]) -> Value {
    json!(
        results
            .iter()
            .map(|r| {
                let mut o = json!({ "url": r.url });
                if let Some(t) = &r.title {
                    o["title"] = json!(t);
                }
                o
            })
            .collect::<Vec<_>>()
    )
}

fn provider_json(r: &ProviderResult, with_lyrics: bool) -> Value {
    let mut o = json!({ "id": r.id, "provider": r.provider.as_str(), "title": r.title, "artist": r.artist, "url": r.url });
    for (k, v) in [
        ("album", &r.album),
        ("releaseYear", &r.release_year),
        ("snippet", &r.snippet),
    ] {
        if let Some(v) = v {
            o[k] = json!(v);
        }
    }
    if let Some(v) = r.lyrics_available {
        o["lyricsAvailable"] = json!(v);
    }
    if with_lyrics && let Some(l) = &r.lyrics {
        o["lyrics"] = json!(l);
    }
    o
}

#[test]
fn search_engine_pages_parse_like_electron() {
    let challenge = "<html><head><title>Just a moment...</title></head><body>verify you are human</body></html>";
    let pages = [
        fixture("duckduckgo-amazing-grace.html"),
        fixture("mojeek-amazing-grace.html"),
        fixture("brave-amazing-grace.html"),
        challenge.to_owned(),
    ];
    let mut duck = Vec::new();
    let mut brave = Vec::new();
    let mut mojeek = Vec::new();
    let mut challenges = Vec::new();
    for html in &pages {
        let a = json!([html]);
        duck.push((a.clone(), search_json(&parse_duck_html(html))));
        brave.push((a.clone(), search_json(&parse_brave_html(html))));
        mojeek.push((a.clone(), search_json(&parse_mojeek_html(html))));
        challenges.push((a, json!(looks_like_challenge(html))));
    }
    assert_all_match("oracle:lyrics-online.parseDuckHtml", &duck);
    assert_all_match("oracle:lyrics-online.parseBraveHtml", &brave);
    assert_all_match("oracle:lyrics-online.parseMojeekHtml", &mojeek);
    assert_all_match("oracle:lyrics-online.looksLikeChallenge", &challenges);

    // Pages a search would surface, judged against two queries.
    let mut discovered = Vec::new();
    let results: Vec<WebSearchResult> = pages[..3]
        .iter()
        .flat_map(|h| {
            parse_duck_html(h)
                .into_iter()
                .chain(parse_brave_html(h))
                .chain(parse_mojeek_html(h))
        })
        .chain([
            WebSearchResult {
                title: Some("Amazing Grace Lyrics".into()),
                url: "https://genius.com/John-newton-amazing-grace-lyrics".into(),
                description: None,
            },
            WebSearchResult {
                title: Some("Ija Dopin Lyrics".into()),
                url: "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html".into(),
                description: None,
            },
            WebSearchResult {
                title: Some("Amazing grace".into()),
                url: "http://insecure.genius.com/x".into(),
                description: None,
            },
        ])
        .collect();
    for query in ["amazing grace", "ija dopin lyrics"] {
        let found: Vec<Value> = discover_song_pages(&results, query)
            .iter()
            .map(|r| provider_json(r, false))
            .collect();
        discovered.push((json!([search_json(&results), query]), json!(found)));
    }
    assert_all_match("oracle:lyrics-online.discoverSongPages", &discovered);

    let mut allowed = Vec::new();
    for r in &results {
        allowed.push((json!([r.url]), json!(is_allowed_lyrics_url(&r.url))));
    }
    assert_all_match("oracle:lyrics-online.isAllowedLyricsUrl", &allowed);
}

#[test]
fn genius_and_lyric_pages_extract_like_electron() {
    let rt = runtime();
    let medley = fixture("genius-medley.html");
    let url = "https://genius.com/Worship-amazing-grace-live-medley-lyrics";
    let mut genius_cases = Vec::new();
    for title in [None, Some("Amazing Grace"), Some("When I Survey")] {
        let http: Arc<dyn Http> =
            Arc::new(Pages(HashMap::from([(url.to_owned(), medley.clone())])));
        let service = genius::Genius::new(http);
        let found = rt.block_on(service.fetch_lyrics(url, title));
        genius_cases.push((
            json!([url, title, medley]),
            match found {
                Ok(text) => json!(text),
                Err(error) => json!({ "error": error }),
            },
        ));
    }
    // Not a Genius URL, and a page with no lyric containers.
    let http: Arc<dyn Http> = Arc::new(Pages(HashMap::new()));
    let empty = genius::Genius::new(http);
    for (u, html) in [
        ("https://example.com/x-lyrics", "<p>x</p>"),
        (url, "<html><body><p>no lyrics</p></body></html>"),
    ] {
        let pages: Arc<dyn Http> =
            Arc::new(Pages(HashMap::from([(u.to_owned(), html.to_owned())])));
        let g = genius::Genius::new(pages);
        let found = rt.block_on(g.fetch_lyrics(u, None));
        genius_cases.push((
            json!([u, Value::Null, html]),
            match found {
                Ok(t) => json!(t),
                Err(e) => json!({ "error": e }),
            },
        ));
    }
    drop(empty);
    assert_all_match("oracle:lyrics-online.geniusFromHtml", &genius_cases);

    let hymn_url = "https://yorubahymns.blogspot.com/2015/04/ija-dopin-ogun-si-tan.html";
    let hymn = fixture("yorubahymns-ija-dopin.html");
    let mut scraped = Vec::new();
    for (u, html) in [
        (hymn_url, hymn.as_str()),
        ("https://example.com/x", "<p>x</p>"),
        ("https://www.ceenaija.com/x/", "<html><body></body></html>"),
    ] {
        let http: Arc<dyn Http> = Arc::new(Pages(HashMap::from([(u.to_owned(), html.to_owned())])));
        let page = rt.block_on(kairo_lyrics::online::web_lyrics::scrape_lyrics_page(
            &http, u,
        ));
        scraped.push((
            json!([u, html]),
            match page {
                Ok(p) => json!({ "title": p.title, "artist": p.artist, "lyrics": p.lyrics, "url": p.url }),
                Err(e) => json!({ "error": e }),
            },
        ));
    }
    assert_all_match("oracle:lyrics-online.scrapeFromHtml", &scraped);

    // Glosses from the hymn's own seed page, with managed search unavailable.
    let http: Arc<dyn Http> = Arc::new(Pages(HashMap::from([(hymn_url.to_owned(), hymn.clone())])));
    let finder =
        glosses::GlossFinder::new(http, Arc::new(NoGateway), Arc::new(WebSearch::default()));
    let pairs = rt.block_on(finder.fetch("Ija D'opin", None, None));
    assert_all_match(
        "oracle:lyrics-online.glossMap",
        &[(
            json!(["Ija D'opin", Value::Null, { hymn_url: hymn }]),
            json!(pairs.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()),
        )],
    );
}

#[test]
fn lrclib_search_and_ranking_match_electron() {
    let rt = runtime();
    let body = fixture("lrclib-search-amazing-grace.json");
    let http: Arc<dyn Http> = Arc::new(Pages(HashMap::from([(
        "https://lrclib.net/api/search".to_owned(),
        body.clone(),
    )])));
    let service = lrclib::Lrclib::new(http);
    let results = rt.block_on(service.search("amazing grace")).unwrap();
    assert_all_match(
        "oracle:lyrics-online.lrclibSearch",
        &[(
            json!(["amazing grace", body]),
            json!(
                results
                    .iter()
                    .map(|r| provider_json(r, true))
                    .collect::<Vec<_>>()
            ),
        )],
    );

    let mut pool = results.clone();
    pool.push(ProviderResult {
        id: "genius:1".into(),
        provider: LyricsProvider::Genius,
        title: "Amazing Grace (My Chains Are Gone)".into(),
        artist: "Chris Tomlin".into(),
        url: "https://genius.com/1".into(),
        popularity: Some(250_000.0),
        ..ProviderResult::default()
    });
    pool.push(ProviderResult {
        id: "genius:2".into(),
        provider: LyricsProvider::Genius,
        title: "Amazing Grace - Live".into(),
        artist: "Aretha Franklin".into(),
        url: "https://genius.com/2".into(),
        release_year: Some("1972".into()),
        popularity: Some(40_000.0),
        ..ProviderResult::default()
    });
    pool.push(ProviderResult {
        id: "web:x".into(),
        provider: LyricsProvider::Web,
        title: "Ija Dopin".into(),
        artist: String::new(),
        url: "https://yorubahymns.blogspot.com/x".into(),
        lyrics: Some("Ija d'opin ogun si tan".into()),
        ..ProviderResult::default()
    });
    let pool_json: Vec<Value> = pool
        .iter()
        .map(|r| {
            let mut o = provider_json(r, true);
            if let Some(p) = r.popularity {
                o["popularity"] = json!(p);
            }
            o
        })
        .collect();
    let mut ranked = Vec::new();
    let mut needs = Vec::new();
    for q in [
        "amazing grace",
        "amazing grace by aretha franklin",
        "how sweet the sound",
        "i once was lost but now am found",
        "ija dopin",
        "chains are gone - chris tomlin",
        "zzz",
    ] {
        let out: Vec<Value> = rank_results(q, &pool)
            .iter()
            .map(|r| provider_json(r, false))
            .collect();
        ranked.push((json!([q, pool_json]), json!(out)));
        needs.push((json!([q, pool_json]), json!(needs_web_resolution(q, &pool))));
        needs.push((json!([q, []]), json!(needs_web_resolution(q, &[]))));
    }
    assert_all_match("oracle:lyrics-online.rankResults", &ranked);
    assert_all_match("oracle:lyrics-online.needsWebResolution", &needs);

    let lyrics = "Amazing grace! How sweet\nthe sound that saved\nA wretch like me\nAmioluwa o";
    let mut matches = Vec::new();
    for q in [
        "sweet the sound",
        "how sweet the sound that",
        "ami oluwa",
        "wretch grace amazing",
        "nothing here",
        "",
    ] {
        let cq = normalize::canonical_text(q);
        let words: Vec<String> = cq
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_owned)
            .collect();
        let out = match find_lyric_match(lyrics, &cq, &words) {
            LyricMatch::Phrase(line) => json!({ "kind": "phrase", "line": line }),
            LyricMatch::Words(line) => json!({ "kind": "words", "line": line }),
            LyricMatch::None => json!({ "kind": "none" }),
        };
        matches.push((json!([lyrics, cq, words]), out));
    }
    assert_all_match("oracle:lyrics-online.findLyricMatch", &matches);
}
