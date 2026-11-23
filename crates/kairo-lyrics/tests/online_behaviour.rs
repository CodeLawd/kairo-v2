//! Behaviours of `tests/lyrics-translate-client.test.ts`,
//! `tests/lyrics-web-search.test.ts`, `tests/lyrics-snippet-resolver.test.ts`
//! and `tests/lyrics-online-ranking.test.ts` that need fakes.

use kairo_domain::cloud::{ManagedProvider, ManagedRequest};
use kairo_domain::lyrics::{LyricsProvider, LyricsSectionType, LyricsSongSection};
use kairo_lyrics::online::http::{
    BoxFuture, GatewayError, Http, HttpError, HttpRequest, HttpResponse, ManagedGateway,
};
use kairo_lyrics::online::rank::rank_results;
use kairo_lyrics::online::resolver::{SnippetResolver, discover_song_pages};
use kairo_lyrics::online::translate::{TranslateOptions, translate_sections};
use kairo_lyrics::online::web_search::{SearchProvider, WebSearch, WebSearchResult};
use kairo_lyrics::online::{
    OnlineLyrics, ProviderResult, genius::Genius, glosses::GlossFinder, lrclib::Lrclib,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

type Reply = Box<dyn Fn(&ManagedRequest) -> Result<Value, GatewayError> + Send + Sync>;

struct Gateway {
    reply: Reply,
    calls: Mutex<Vec<ManagedRequest>>,
}

impl Gateway {
    fn new(
        reply: impl Fn(&ManagedRequest) -> Result<Value, GatewayError> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            reply: Box::new(reply),
            calls: Mutex::default(),
        })
    }
    fn providers(&self) -> Vec<ManagedProvider> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.provider)
            .collect()
    }
}

impl ManagedGateway for Gateway {
    fn request(&self, request: ManagedRequest) -> BoxFuture<'_, Result<Value, GatewayError>> {
        let out = (self.reply)(&request);
        self.calls.lock().unwrap().push(request);
        Box::pin(async move { out })
    }
}

fn refused(status: Option<u16>, message: &str) -> GatewayError {
    GatewayError {
        status,
        message: message.into(),
    }
}

type Page = Box<dyn Fn(&str) -> Result<String, HttpError> + Send + Sync>;

struct Web {
    page: Page,
    urls: Mutex<Vec<String>>,
}

impl Web {
    fn new(page: impl Fn(&str) -> Result<String, HttpError> + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            page: Box::new(page),
            urls: Mutex::default(),
        })
    }
}

impl Http for Web {
    fn get(
        &self,
        request: HttpRequest,
        _a: fn(u16) -> bool,
    ) -> BoxFuture<'_, Result<HttpResponse, HttpError>> {
        self.urls.lock().unwrap().push(request.url.clone());
        let out = (self.page)(&request.url).map(|body| HttpResponse { status: 200, body });
        Box::pin(async move { out })
    }
}

fn not_found() -> HttpError {
    HttpError::Status {
        status: 404,
        body: String::new(),
    }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .start_paused(true)
        .build()
        .unwrap()
}

fn section(lines: &[&str]) -> LyricsSongSection {
    LyricsSongSection {
        kind: LyricsSectionType::Chorus,
        label: "Chorus".into(),
        lines: lines.iter().map(|s| s.to_string()).collect(),
        line_colors: None,
        hotkey: None,
        extra: Default::default(),
    }
}

fn no_glosses(gateway: Arc<dyn ManagedGateway>) -> GlossFinder {
    GlossFinder::new(
        Web::new(|_| Err(not_found())),
        gateway,
        Arc::new(WebSearch::default()),
    )
}

// ─── Translation ────────────────────────────────────────────────────────────

#[test]
fn translation_reports_activation_when_managed_access_is_unavailable() {
    let gateway = Gateway::new(|_| {
        Err(refused(
            None,
            "Activate managed services before requesting a provider",
        ))
    });
    let error = rt()
        .block_on(translate_sections(
            gateway.as_ref(),
            &no_glosses(gateway.clone()),
            &[section(&["Hola"])],
            &TranslateOptions::default(),
        ))
        .unwrap_err();
    assert!(error.contains("Activate"));
}

#[test]
fn translation_inserts_glosses_and_skips_english() {
    let gateway = Gateway::new(|request| {
        assert_eq!(request.provider, ManagedProvider::GoogleTranslate);
        let lines = request.body.as_ref().unwrap()["q"]
            .as_array()
            .unwrap()
            .len();
        let rows = [
            json!({ "translatedText": "There is no other Name like Jesus", "detectedSourceLanguage": "yo" }),
            json!({ "translatedText": "He made the lame walk", "detectedSourceLanguage": "yo" }),
        ];
        Ok(json!({ "data": { "translations": rows[..lines] } }))
    });
    let finder = no_glosses(gateway.clone());
    let out = rt()
        .block_on(translate_sections(
            gateway.as_ref(),
            &finder,
            &[section(&["Odudu dabu Jesus no", "", "Ide na lolu le"])],
            &TranslateOptions::default(),
        ))
        .unwrap();
    assert_eq!(
        out[0].lines,
        [
            "Odudu dabu Jesus no",
            "(There is no other Name like Jesus)",
            "",
            "Ide na lolu le",
            "(He made the lame walk)"
        ]
    );
    let out = rt()
        .block_on(translate_sections(
            gateway.as_ref(),
            &finder,
            &[section(&["Power belongs to Jesus", "Odudu dabu Jesus no"])],
            &TranslateOptions::default(),
        ))
        .unwrap();
    assert_eq!(
        out[0].lines,
        [
            "Power belongs to Jesus",
            "Odudu dabu Jesus no",
            "(There is no other Name like Jesus)"
        ]
    );
}

#[test]
fn translation_forces_the_source_language_and_surfaces_errors() {
    let gateway = Gateway::new(|request| {
        assert_eq!(request.body.as_ref().unwrap()["source"], "yo");
        Ok(
            json!({ "data": { "translations": [{ "translatedText": "The mark of God is upon me" }] } }),
        )
    });
    let options = TranslateOptions {
        source_language: Some("yo".into()),
        ..TranslateOptions::default()
    };
    let out = rt()
        .block_on(translate_sections(
            gateway.as_ref(),
            &no_glosses(gateway.clone()),
            &[section(&["Amioluwa o mbe lori mi"])],
            &options,
        ))
        .unwrap();
    assert_eq!(
        out[0].lines,
        ["Amioluwa o mbe lori mi", "(The mark of God is upon me)"]
    );

    let denied = Gateway::new(|_| Err(refused(Some(403), "PERMISSION_DENIED")));
    let error = rt()
        .block_on(translate_sections(
            denied.as_ref(),
            &no_glosses(denied.clone()),
            &[section(&["Amioluwa"])],
            &TranslateOptions::default(),
        ))
        .unwrap_err();
    assert!(error.contains("denied"), "{error}");
}

// ─── Web search order ───────────────────────────────────────────────────────

const HIT: &str = "https://africangospellyrics.com/a-song/";

#[test]
fn deepseek_first_and_brave_only_when_needed() {
    let rt = rt();
    let search = WebSearch::default();
    let ok = Gateway::new(|r| {
        Ok(match r.provider {
            ManagedProvider::Deepseek => json!({ "results": [{ "title": "A song", "url": HIT }] }),
            _ => json!({ "web": { "results": [{ "title": "A song", "url": HIT }] } }),
        })
    });
    let first = rt
        .block_on(async {
            search
                .batches(ok.as_ref(), &["A song lyrics".into()])
                .next()
                .await
        })
        .unwrap();
    assert_eq!(first.provider, SearchProvider::Deepseek);
    assert_eq!(ok.providers(), [ManagedProvider::Deepseek]);

    for status in [401, 402, 429, 504] {
        let failing = Gateway::new(move |r| match r.provider {
            ManagedProvider::Deepseek => Err(refused(Some(status), "failed")),
            _ => Ok(json!({ "web": { "results": [{ "url": HIT }] } })),
        });
        let mut cursor = search.batches(failing.as_ref(), &["A song lyrics".into()]);
        let mut seen = Vec::new();
        while let Some(b) = rt.block_on(cursor.next()) {
            seen.push(b.provider);
        }
        assert_eq!(seen, [SearchProvider::Brave]);
    }

    let malformed = Gateway::new(|r| {
        Ok(match r.provider {
            ManagedProvider::Deepseek => {
                json!({ "results": [{ "url": "javascript:alert(1)" }, null] })
            }
            _ => json!({ "web": { "results": [{ "url": HIT }] } }),
        })
    });
    let mut cursor = search.batches(malformed.as_ref(), &["A song lyrics".into()]);
    assert_eq!(
        rt.block_on(cursor.next()).unwrap().provider,
        SearchProvider::Brave
    );

    let long = "a".repeat(129);
    let brave_only = Gateway::new(|_| Ok(json!({ "web": { "results": [{ "url": HIT }] } })));
    rt.block_on(async {
        search
            .batches(brave_only.as_ref(), std::slice::from_ref(&long))
            .next()
            .await
    });
    assert_eq!(brave_only.providers(), [ManagedProvider::Brave]);

    let emoji = "😀".repeat(128);
    let tracked = Gateway::new(|_| {
        Ok(json!({ "results": [{ "url": format!("{HIT}?share=twitter#respond") }] }))
    });
    let batch = rt
        .block_on(async { search.batches(tracked.as_ref(), &[emoji]).next().await })
        .unwrap();
    assert_eq!(batch.results[0].url, HIT);
}

// ─── Snippet resolver ───────────────────────────────────────────────────────

fn resolver(http: Arc<Web>, gateway: Arc<Gateway>) -> SnippetResolver {
    let genius = Arc::new(Genius::new(http.clone()));
    let lrclib = Arc::new(Lrclib::new(http.clone()));
    SnippetResolver::new(
        http,
        gateway,
        genius,
        lrclib,
        Arc::new(WebSearch::default()),
    )
}

#[test]
fn unverified_deepseek_hits_continue_to_brave_and_verified_results_cache() {
    let gateway = Gateway::new(|r| {
        Ok(match r.provider {
            ManagedProvider::Deepseek => {
                json!({ "results": [{ "title": "Other", "url": "https://africangospellyrics.com/other/" }] })
            }
            _ => json!({ "web": { "results": [{ "title": "A song", "url": HIT }] } }),
        })
    });
    let web = Web::new(|url| {
        if url == HIT {
            Ok("<html><head><title>A Song Lyrics by Choir</title></head><body><article><p>A song of praise tonight we sing</p><p>Glory glory hallelujah again</p></article></body></html>".into())
        } else if url.contains("/other/") {
            Ok("<html><body><article><p>Nothing to see in this stanza here</p><p>More unrelated words for padding text</p></article></body></html>".into())
        } else {
            Err(not_found())
        }
    });
    let r = resolver(web, gateway.clone());
    let rt = rt();
    let first = rt.block_on(r.resolve("A song"));
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].url, HIT);
    assert_eq!(rt.block_on(r.resolve("A song")), first);
    assert_eq!(
        gateway.providers(),
        [ManagedProvider::Deepseek, ManagedProvider::Brave]
    );
}

#[test]
fn failed_providers_fall_back_to_keyless_engines_and_keep_discovered_pages() {
    let page = "https://www.musixmatch.com/lyrics/Steady-Worship/Jesu-Oloruko-Aperire";
    let gateway = Gateway::new(move |r| {
        Ok(match r.provider {
            ManagedProvider::Deepseek => {
                json!({ "results": [{ "title": "Steady Worship - Jesu Oloruko Aperire lyrics", "url": page }] })
            }
            _ => json!({ "web": { "results": [] } }),
        })
    });
    let web = Web::new(|_| Ok("<html><title>Just a moment...</title></html>".into()));
    let r = resolver(web.clone(), gateway);
    let results = rt().block_on(r.resolve("oloruko aperire jesu"));
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].url, page);
    assert_eq!(results[0].lyrics_available, Some(false));
    assert!(
        web.urls
            .lock()
            .unwrap()
            .iter()
            .any(|u| u.contains("duckduckgo")),
        "keyless engines were tried"
    );
}

#[test]
fn web_discovery_keeps_a_relevant_unreadable_page() {
    let hits = discover_song_pages(
        &[
            WebSearchResult {
                title: Some("Steady Worship - Jesu Oloruko Aperire lyrics".into()),
                url: "https://www.musixmatch.com/lyrics/Steady-Worship/Jesu-Oloruko-Aperire".into(),
                description: None,
            },
            WebSearchResult {
                title: Some("Unrelated worship song".into()),
                url: "https://open.spotify.com/track/other".into(),
                description: None,
            },
            WebSearchResult {
                title: Some("Jesu Oloruko Aperire".into()),
                url: "javascript:alert(1)".into(),
                description: None,
            },
        ],
        "oloruko aperire jesu lyrics",
    );
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].lyrics_available, Some(false));
    assert!(hits[0].title.contains("Jesu Oloruko Aperire"));
    assert_eq!(hits[0].lyrics, None);
}

// ─── Glosses ────────────────────────────────────────────────────────────────

#[test]
fn incomplete_glosses_do_not_stop_the_brave_search() {
    let gateway = Gateway::new(|r| {
        Ok(match r.provider {
            ManagedProvider::Deepseek => {
                json!({ "results": [{ "title": "x", "url": "https://africangospellyrics.com/partial/" }] })
            }
            _ => {
                json!({ "web": { "results": [{ "title": "x", "url": "https://africangospellyrics.com/complete/" }] } })
            }
        })
    });
    let web = Web::new(|url| {
        Ok(if url.contains("/complete/") {
            "<article>Odudu (Power)<br>Abasi (God)</article>".into()
        } else if url.contains("/partial/") {
            "<article>Odudu (Power)<br>Jesu (Jesus)</article>".into()
        } else {
            return Err(not_found());
        })
    });
    let finder = GlossFinder::new(web, gateway.clone(), Arc::new(WebSearch::default()));
    let lines = vec!["Odudu".to_string(), "Abasi".to_string()];
    let pairs = rt().block_on(finder.fetch("Odudu", None, Some(&lines)));
    assert!(gateway.providers().contains(&ManagedProvider::Brave));
    assert!(pairs.iter().any(|(k, _)| k == "abasi"));
}

// ─── Ranking ────────────────────────────────────────────────────────────────

fn hit(
    provider: LyricsProvider,
    id: &str,
    title: &str,
    artist: &str,
    lyrics: Option<&str>,
    popularity: Option<f64>,
) -> ProviderResult {
    ProviderResult {
        id: id.into(),
        provider,
        title: title.into(),
        artist: artist.into(),
        url: format!("https://x/{id}"),
        lyrics: lyrics.map(str::to_owned),
        popularity,
        ..ProviderResult::default()
    }
}

#[test]
fn a_named_artist_and_exact_titles_win_over_popularity() {
    let pool = [
        hit(
            LyricsProvider::Genius,
            "cover",
            "Way Maker",
            "Leeland",
            None,
            Some(2_000_000.0),
        ),
        hit(
            LyricsProvider::Lrclib,
            "sinach",
            "Way Maker",
            "Sinach",
            Some("You are here, moving in our midst"),
            None,
        ),
        hit(
            LyricsProvider::Genius,
            "mistag",
            "Way Maker Sinach",
            "Random Uploader",
            None,
            None,
        ),
    ];
    assert_eq!(rank_results("way maker by sinach", &pool)[0].id, "sinach");
    let exact = [
        hit(
            LyricsProvider::Genius,
            "popular",
            "Ese Remix Party",
            "DJ",
            None,
            Some(9_000_000.0),
        ),
        hit(
            LyricsProvider::Genius,
            "exact",
            "Ese",
            "Nathaniel Bassey",
            None,
            Some(10.0),
        ),
    ];
    assert_eq!(rank_results("ese", &exact)[0].id, "exact");
    assert!(rank_results("es", &exact).len() <= exact.len());
}

#[test]
fn query_rescue_re_queries_clean_catalogues_and_skips_web_scraping() {
    let full_lyrics = format!(
        "You are here, moving in our midst\nI worship You\n{}",
        "Way maker, miracle worker, promise keeper, light in the darkness\n".repeat(12)
    );
    let gateway = Gateway::new(|req| {
        assert_eq!(req.path, "/chat/completions", "web search must be skipped");
        Ok(json!({
            "choices": [{
                "finish_reason": "stop",
                "message": {
                    "content": "{\"candidates\":[{\"title\":\"Way Maker\",\"artist\":\"Sinach\"}]}"
                }
            }]
        }))
    });
    let web = Web::new(move |url| {
        if url.contains("lrclib.net/api/search") && url.contains("track_name=Way") {
            Ok(json!([{
                "id": 7,
                "trackName": "Way Maker",
                "artistName": "Sinach",
                "plainLyrics": full_lyrics,
            }])
            .to_string())
        } else if url.contains("lrclib.net") {
            Ok("[]".into())
        } else if url.contains("genius.com") {
            Ok(json!({ "response": { "sections": [] } }).to_string())
        } else {
            Err(not_found())
        }
    });
    // Use full_url() in fake HTTP so structured query params are visible.
    struct QueryWeb(Arc<Web>);
    impl Http for QueryWeb {
        fn get(
            &self,
            request: HttpRequest,
            accept: fn(u16) -> bool,
        ) -> BoxFuture<'_, Result<HttpResponse, HttpError>> {
            let mut forwarded = request.clone();
            forwarded.url = request.full_url();
            self.0.get(forwarded, accept)
        }
    }
    let online = OnlineLyrics::new(Arc::new(QueryWeb(web.clone())), gateway.clone());
    let hits = rt()
        .block_on(online.search_all("you are here moving in our midst"))
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].provider, LyricsProvider::Lrclib);
    assert_eq!(hits[0].title, "Way Maker");
    assert_eq!(gateway.providers(), [ManagedProvider::Deepseek]);
    assert!(
        web.urls
            .lock()
            .unwrap()
            .iter()
            .all(|u| !u.contains("duckduckgo")),
        "web scraping was skipped after catalogue rescue"
    );
}
