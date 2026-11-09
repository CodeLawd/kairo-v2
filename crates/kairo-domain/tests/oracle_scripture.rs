//! Prompt 015: the scripture ports against the TypeScript originals through
//! `kairo-oracle`. Every book name and abbreviation, their prefixes, malformed
//! references and generated inputs go through each parser. Runs from
//! `tests/oracle-cache` unless `KAIRO_ORACLE_MODE` says otherwise; refresh with
//! `cargo run -p ts-oracle -- --regenerate`.

use kairo_domain::json::to_js_value;
use kairo_domain::scripture::live_progress::*;
use kairo_domain::scripture::notes_review::*;
use kairo_domain::scripture::offline_state::*;
use kairo_domain::scripture::plan::*;
use kairo_domain::scripture::query::*;
use kairo_domain::scripture::small::*;
use kairo_domain::scripture::spoken_translations::*;
use kairo_domain::scripture::trace::*;
use kairo_domain::scripture::translations::*;
use kairo_domain::scripture::versification::*;
use kairo_domain::scripture::{
    ApiBibleDownloadProgress, ApiBibleOfflineTranslation, ScriptureResult, ScriptureSuggestion,
    ScriptureTranslationOption, SermonPlan, SermonReferenceMatch, SermonScriptureItem,
};
use kairo_oracle::{assert_all_match, generate, number};
use proptest::prelude::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

fn js<T: Serialize + ?Sized>(value: &T) -> Value {
    to_js_value(value).unwrap()
}

fn or_null<T: Serialize>(value: Option<T>) -> Value {
    value.map_or(Value::Null, |v| js(&v))
}

fn parse<T: DeserializeOwned>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{e}: {value}"))
}

/// Polls a future that never waits (every test search is ready at once).
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// Compares one-argument string functions over many inputs.
fn each_string(name: &str, inputs: &[String], f: impl Fn(&str) -> Value) {
    let cases: Vec<_> = inputs.iter().map(|s| (json!([s]), f(s))).collect();
    assert_all_match(name, &cases);
}

// ─── Reference corpus ─────────────────────────────────────────────────────────

const SUFFIXES: &[&str] = &[
    "",
    " ",
    " 3",
    " 23",
    " 3:16",
    " 3 16",
    " 3:16-18",
    " 3 16 18",
    " 3:16–18",
    " 3 : 16 — 18",
    " 0",
    " 150",
    " 151",
    " 007:010",
    "  23  ",
    " 3:",
    " :16",
    " 3::16",
    " 3:16-",
    " 3-16",
    " 3:16:17",
    " 1 2 3 4",
    "3:16",
    " 3a",
    " 99999999999999999999999",
];

fn title(text: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in text.chars() {
        out.extend(if start {
            c.to_uppercase().collect::<Vec<_>>()
        } else {
            vec![c]
        });
        start = c == ' ';
    }
    out
}

/// Every book name and abbreviation in several cases with every suffix, every
/// prefix of each name, malformed references and generated strings.
fn reference_inputs() -> Vec<String> {
    let mut inputs = Vec::new();
    for (name, aliases) in BOOKS {
        let mut forms = vec![name.to_string(), name.to_uppercase(), name.to_lowercase()];
        for alias in *aliases {
            forms.push(alias.to_string());
            forms.push(title(alias));
        }
        for form in &forms {
            for suffix in SUFFIXES {
                inputs.push(format!("{form}{suffix}"));
            }
        }
        let lower = name.to_lowercase();
        for end in 1..=lower.len() {
            for suffix in ["", " 3", " 3:16"] {
                inputs.push(format!("{}{suffix}", &lower[..end]));
            }
        }
    }
    for odd in [
        "",
        " ",
        "  \t ",
        "12 4 8",
        "for God so loved",
        "love is patient",
        "love 123",
        "Jo",
        "jo 1 2",
        "j",
        "1",
        "1 ",
        "1 c",
        "song of",
        "song of songs 2:1",
        "Song  of\u{A0}Solomon 2 1",
        "\u{FEFF}John 3:16\u{3000}",
        "John\u{2028}3:16",
        "john\u{85}3:16",
        "Jöhn 3:16",
        "\u{212A}ings 1",
        "rom 8 28 ",
        "Ezek 1 2 3",
        "psa 23",
        "josh 1 9",
        "for God so loved the world",
        "it's what's written",
        "a b c defgh",
        "1 cor 13 4 7",
        "2tim 1 7",
        "3 John 1:15",
        "Revelation 22:21–22:21",
        "Matthew 5:3—12",
        "John 3:16 NIV",
        "😀 3:16",
        "john 😀",
        "phil 4:6-7-8",
    ] {
        inputs.push(odd.to_string());
    }
    let alphabet = prop::sample::select(vec![
        "j", "o", "h", "n", "1", "2", "3", " ", ":", "-", "–", "ps", "a", "rom", "\u{A0}", "'",
        "Ez", "e", "k", "9", "0",
    ]);
    let random = prop::collection::vec(alphabet, 0..10).prop_map(|parts| parts.concat());
    inputs.extend(generate(15, 1500, random));
    inputs
}

#[test]
fn reference_parsing_matches_over_every_book_and_abbreviation() {
    let inputs = reference_inputs();
    each_string("scripture-query.normalizeScriptureQuery", &inputs, |s| {
        or_null(normalize_scripture_query(s))
    });
    each_string(
        "scripture-query.resolveSubmittedScriptureQuery",
        &inputs,
        |s| json!(resolve_submitted_scripture_query(s)),
    );
    each_string("scripture-query.isLikelyPhraseQuery", &inputs, |s| {
        json!(is_likely_phrase_query(s))
    });
    each_string(
        "scripture-query.shouldLiveSuggestScriptureQuery",
        &inputs,
        |s| json!(should_live_suggest_scripture_query(s)),
    );
}

#[test]
fn book_completions_match() {
    // Completions are long lists, so a lighter corpus: every name, alias and
    // name prefix with and without numbers, plus the malformed set.
    let mut inputs: Vec<String> = reference_inputs()
        .into_iter()
        .filter(|s| !s.contains(' ') || s.ends_with(" 3") || s.ends_with(" 3:16") || s.len() < 8)
        .collect();
    inputs.sort();
    inputs.dedup();
    each_string("scripture-query.getBookCompletions", &inputs, |s| {
        js(&get_book_completions(s))
    });
    each_string("scripture-query.getBookCompletion", &inputs, |s| {
        or_null(get_book_completion(s))
    });
}

#[test]
fn book_names_constant_matches() {
    let names: Vec<_> = bible_book_names().collect();
    assert_all_match(
        "scripture-query.BIBLE_BOOK_NAMES",
        &[(json!([]), json!(names))],
    );
}

fn result(book: &str, chapter: f64, verses: &[f64], translation: &str) -> ScriptureResult {
    parse(&json!({
        "reference": format!("{book} {chapter}"),
        "translation": translation,
        "verses": verses.iter().map(|v| json!({"book": book, "chapter": chapter, "verse": v, "text": format!("Verse {v}")})).collect::<Vec<_>>(),
    }))
}

#[test]
fn passage_results_and_adjacent_queries_match() {
    let mut loaded_sets: Vec<Vec<ScriptureResult>> = vec![vec![]];
    for (book, chapter, verse, translation) in [
        ("Joshua", 1.0, 9.0, "KJV"),
        ("Joshua", 2.0, 1.0, "KJV"),
        ("Joshua", 1.0, 1.0, "KJV"),
        ("Mark", 9.0, 43.0, "AMPC"),
        ("Mark", 9.0, 45.0, "AMPC"),
        ("Mark", 9.0, 47.0, "ampc"),
        ("Mark", 9.0, 45.0, "NKJV"),
        ("3 John", 1.0, 14.0, "AMPC"),
        ("Unknown", 4.0, 2.0, "KJV"),
        ("mk", 9.0, 43.0, "AMPC"),
        ("Psalms", 119.0, 176.0, "KJV"),
        ("John", 0.0, 0.0, "KJV"),
        ("John", 2.5, 1.5, "KJV"),
        ("John", -1.0, -3.0, "KJV"),
    ] {
        loaded_sets.push(vec![result(book, chapter, &[verse], translation)]);
        loaded_sets.push(vec![
            result(book, chapter, &[verse, verse + 1.0], translation),
            result(book, chapter + 1.0, &[1.0], translation),
        ]);
    }
    loaded_sets.push(vec![ScriptureResult {
        reference: "Empty".into(),
        translation: "KJV".into(),
        verses: vec![],
    }]);
    loaded_sets.push(vec![
        result("John", 3.0, &[16.0], "KJV"),
        result("Romans", 8.0, &[28.0], "KJV"),
    ]);
    loaded_sets.push(vec![result("John", 3.0, &[16.0, 16.0], "KJV")]);

    let mut adjacent = Vec::new();
    let mut combined = Vec::new();
    let mut expanded = Vec::new();
    for loaded in &loaded_sets {
        for (direction, label) in [(Direction::Next, "next"), (Direction::Previous, "previous")] {
            adjacent.push((
                json!([loaded, label]),
                json!(get_adjacent_verse_queries(loaded, direction)),
            ));
        }
        combined.push((json!([loaded]), or_null(combine_scripture_results(loaded))));
        for one in loaded {
            expanded.push((json!([one]), js(&expand_scripture_result(one))));
        }
    }
    assert_all_match("scripture-query.getAdjacentVerseQueries", &adjacent);
    assert_all_match("scripture-query.combineScriptureResults", &combined);
    assert_all_match("scripture-query.expandScriptureResult", &expanded);
}

type Script = BTreeMap<String, Value>;

fn scripted(
    script: &Script,
) -> impl FnMut(String, String) -> std::future::Ready<Result<Vec<ScriptureResult>, String>> + '_ {
    move |query, translation| {
        let hit = script.get(&format!("{query}|{translation}")).cloned();
        std::future::ready(match hit {
            Some(Value::Object(o)) if o.contains_key("throw") => {
                Err(o["throw"].as_str().unwrap_or_default().to_string())
            }
            Some(value) => Ok(parse(&value)),
            None => Ok(Vec::new()),
        })
    }
}

#[test]
fn reloading_passages_in_another_translation_matches() {
    let passages = vec![
        result("John", 3.0, &[16.0], "KJV"),
        result("Romans", 8.0, &[28.0], "KJV"),
        result("John", 3.0, &[16.0, 17.0], "KJV"),
    ];
    let hit = |r: &ScriptureResult| json!([{"reference": r.reference, "translation": "NIV", "verses": [{"book": "X", "chapter": 1, "verse": 1, "text": "NIV"}]}]);
    let scripts: Vec<Script> = vec![
        Script::new(),
        [(format!("{}|NIV", passages[0].reference), hit(&passages[0]))].into(),
        [
            (format!("{}|NIV", passages[0].reference), json!({"throw": "offline"})),
            (format!("{}|NIV", passages[1].reference), json!([{"reference": "e", "translation": "NIV", "verses": []}, {"reference": "Romans 8:28", "translation": "NIV", "verses": [{"book": "Romans", "chapter": 8, "verse": 28, "text": "t"}]}])),
            (format!("{}|NIV", passages[2].reference), hit(&passages[2])),
        ]
        .into(),
    ];
    let cases: Vec<_> = scripts
        .iter()
        .map(|script| {
            let out = block_on(reload_passages_in_translation(
                &passages,
                &"NIV".to_string(),
                scripted(script),
            ));
            (
                json!([passages, "NIV", script]),
                json!({"results": out.results, "unavailable": out.unavailable}),
            )
        })
        .collect();
    assert_all_match("oracle:scripture.reload", &cases);
}

// ─── Translations and versification ──────────────────────────────────────────

#[test]
fn translation_registry_matches() {
    let definitions: Vec<Value> = BIBLE_TRANSLATIONS
        .iter()
        .map(|d| {
            let mut v = json!({"id": d.id, "name": d.name, "access": d.access});
            if d.bundled {
                v["bundled"] = json!(true);
            }
            // The source writes `aliases: []` for NKJV only.
            if !d.aliases.is_empty() || d.id == "NKJV" {
                v["aliases"] = json!(d.aliases);
            }
            if !d.name_patterns.is_empty() {
                v["namePatterns"] = json!(d.name_patterns);
            }
            if let Some(id) = d.preferred_bible_id {
                v["preferredBibleId"] = json!(id);
            }
            if let Some(p) = d.downloadable_pack {
                v["downloadablePack"] =
                    json!({"url": p.url, "sha256": p.sha256, "approxLabel": p.approx_label});
            }
            v
        })
        .collect();
    assert_all_match(
        "bible-translations.BIBLE_TRANSLATIONS",
        &[(json!([]), json!(definitions))],
    );
    assert_all_match(
        "bible-translations.DEFAULT_TRANSLATION_ID",
        &[(json!([]), json!(DEFAULT_TRANSLATION_ID))],
    );
    assert_all_match(
        "bible-translations.DEFAULT_PACK_TRANSLATION_ID",
        &[(json!([]), json!(DEFAULT_PACK_TRANSLATION_ID))],
    );
    assert_all_match(
        "bible-translations.BUNDLED_TRANSLATION_IDS",
        &[(json!([]), json!(bundled_translation_ids()))],
    );
    let none = json!([]);
    for (name, value) in [
        (
            "bible-translations.getDownloadableTranslationIds",
            json!(get_downloadable_translation_ids()),
        ),
        (
            "bible-translations.translationCatalogEntries",
            json!(translation_catalog_entries()),
        ),
        (
            "bible-translations.buildAbbreviationAliases",
            json!(build_abbreviation_aliases()),
        ),
        (
            "bible-translations.buildPreferredBibleIds",
            json!(build_preferred_bible_ids()),
        ),
        (
            "bible-translations.buildTranslationAliasMap",
            json!(build_translation_alias_map()),
        ),
        (
            "bible-translations.bundledTranslationIdsInOrder",
            json!(bundled_translation_ids_in_order()),
        ),
        (
            "oracle:scripture.namePatterns",
            json!(
                build_name_patterns()
                    .iter()
                    .map(|p| (p.source, p.id))
                    .collect::<Vec<_>>()
            ),
        ),
    ] {
        assert_all_match(name, &[(none.clone(), value)]);
    }

    let ids: Vec<String> = BIBLE_TRANSLATIONS
        .iter()
        .flat_map(|d| [d.id.to_string(), d.id.to_lowercase(), format!(" {}", d.id)])
        .chain(["", "nope", "AMPCX", "ﬀ", "kjv\u{0}"].map(String::from))
        .collect();
    each_string("bible-translations.getTranslationDefinition", &ids, |s| {
        let found = get_translation_definition(s).map(|d| d.id);
        // Compared by identity: the oracle returns the registry entry.
        found.map_or(kairo_oracle::undefined(), |id| {
            definitions[BIBLE_TRANSLATIONS.iter().position(|d| d.id == id).unwrap()].clone()
        })
    });
    each_string("bible-translations.isBundledTranslation", &ids, |s| {
        json!(is_bundled_translation(s))
    });
    each_string("bible-translations.getDownloadablePack", &ids, |s| {
        get_downloadable_pack(s).map_or(
            kairo_oracle::undefined(),
            |p| json!({"url": p.url, "sha256": p.sha256, "approxLabel": p.approx_label}),
        )
    });

    let names: Vec<String> = BIBLE_TRANSLATIONS
        .iter()
        .flat_map(|d| {
            [
                d.name.to_string(),
                d.name.to_uppercase(),
                format!("The {} 2011", d.name),
            ]
        })
        .chain(
            [
                "Amplified Bible Classic",
                "amplified bible,classic edition",
                "Amplified\u{A0}Bible, \u{2003}Classic",
                "THE MESSAGE",
                "themessage",
                "New King James",
                "\u{212A}ing James",
                "",
            ]
            .map(String::from),
        )
        .collect();
    let patterns = build_name_patterns();
    assert_all_match(
        "oracle:scripture.namePatternMatches",
        &[(
            json!([names]),
            json!(
                names
                    .iter()
                    .map(|n| patterns.iter().map(|p| p.is_match(n)).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            ),
        )],
    );
}

#[test]
fn versification_matches() {
    let mut has = Vec::new();
    let mut overrides = Vec::new();
    for translation in ["AMPC", "ampc", "KJV", "", "Ampc "] {
        has.push((
            json!([translation]),
            json!(has_custom_pack_versification(translation)),
        ));
        for (book, chapter) in [
            (41.0, 9.0),
            (64.0, 1.0),
            (41.0, 8.0),
            (0.0, 9.0),
            (41.0, -0.0),
            (41.0, 9.5),
        ] {
            let value = get_verse_numbering_override(translation, book, chapter).map_or(
                kairo_oracle::undefined(),
                |o| {
                    let mut v = json!({"lastVerse": o.last_verse});
                    if !o.omitted.is_empty() {
                        v["omitted"] = json!(o.omitted);
                    }
                    v
                },
            );
            overrides.push((json!([translation, book, chapter]), value));
        }
    }
    assert_all_match("bible-versification.hasCustomPackVersification", &has);
    assert_all_match("bible-versification.getVerseNumberingOverride", &overrides);
}

// ─── Spoken translations and verse text ──────────────────────────────────────

#[test]
fn spoken_translations_match() {
    let mut lines: Vec<String> = [
        "John 3:16 in the NLT",
        "read it in the n l t please",
        "the N.I.V. puts it this way",
        "in the N-K-J-V",
        "the New Living Translation says",
        "from the New International Version",
        "in the King James it says",
        "the English Standard Version renders it",
        "in the New King James Version",
        "the New American Standard Bible",
        "let us read it in the NLT and the NIV",
        "NIV, NLT and then the NIV again",
        "the message today is about grace",
        "his voice was amplified across the hall",
        "we stream on the web every Sunday",
        "the WEB says",
        "the passion of Christ",
        "in the Message Bible it reads",
        "the Amplified says",
        "the Passion Translation puts it",
        "john 3:16 in the niv",
        "the nkjv says",
        "read the esv and the kjv",
        "The NLT, in John 3:16, says",
        "John 3:16 (NIV)",
        "John 3:16 — New Living Translation.",
        "the King James and the New King James",
        "in the KJV and the NKJV",
        "nltd and kjvx are not translations",
        "we serve the new living God",
        "a new international partnership",
        "",
        "For God so loved the world",
        "😀 in the NIV 😀 and the New\u{A0}Living",
        "the new living\u{2028}translation",
        "the world english bible",
        "World English",
        "in the open english version",
        "BSB and bbe and the Berean Standard",
        "the christian standard puts",
        "N. L. T.",
        "n-l-t.",
        "the amplified classic version",
        "the AMPLIFIED BIBLE",
        "the message version",
        "basic english instead",
        "american standard,",
        "american standard?",
        "new living_",
        "New Living Translation and New Living",
        "\u{212A}JV",
    ]
    .map(String::from)
    .into();
    let words = prop::sample::select(vec![
        "the",
        "NIV",
        "niv",
        "n i v",
        "N.I.V.",
        "New",
        "Living",
        "living",
        "King",
        "James",
        "new",
        "king",
        "james",
        "translation",
        "version",
        "says",
        "and",
        "WEB",
        "web",
        ",",
        ".",
        "—",
        "message",
        "Message",
        "bible",
        "amplified",
        "the amplified",
        "Passion",
        "passion",
        "american standard",
        "New American Standard",
        "😀",
        "\u{A0}",
        "ESV",
        "e-s-v",
        "kjvx",
        "AMPC",
        "msg",
    ]);
    let sentence = prop::collection::vec(words, 0..9).prop_map(|w| w.join(" "));
    lines.extend(generate(16, 1500, sentence));
    each_string(
        "spoken-translations.findSpokenTranslationMatches",
        &lines,
        |s| js(&find_spoken_translation_matches(s)),
    );
    each_string("spoken-translations.findSpokenTranslations", &lines, |s| {
        json!(find_spoken_translations(s))
    });
}

#[test]
fn spoken_verse_text_matches() {
    let mut texts: Vec<String> = [
        "Come unto me, all {ye} that labour",
        "the {firmament: Heb. expansion} of heaven",
        "{a}{b:c}{",
        "no braces",
        "open { never closed",
        "}{x}",
        "{}",
        "{{nested}}",
        "{multi\nline: note}",
        "😀{😀}",
    ]
    .map(String::from)
    .into();
    let parts = prop::sample::select(vec!["{", "}", ":", "a", " ", "ye", "😀", "\n"]);
    texts.extend(generate(
        17,
        500,
        prop::collection::vec(parts, 0..12).prop_map(|p| p.concat()),
    ));
    each_string("verse-text.spokenVerseText", &texts, |s| {
        json!(spoken_verse_text(s))
    });
}

// ─── Live progress ───────────────────────────────────────────────────────────

const GENESIS: &[&str] = &[
    "And God spake unto Noah, saying,",
    "Go forth of the ark, thou, and thy wife, and thy sons, and thy sons' wives with thee.",
    "Bring forth with thee every living thing that is with thee, of all flesh.",
    "Every beast, every creeping thing, and every fowl, and whatsoever creepeth upon the earth.",
    "And Noah builded an altar unto the Lord.",
    "And the Lord smelled a sweet savour.",
    "And the Lord said in his heart, I will not again curse the ground any more for man's sake.",
    "While the earth remaineth, seedtime and harvest, and cold and heat shall not cease.",
];

fn genesis() -> Vec<Value> {
    GENESIS
        .iter()
        .enumerate()
        .map(|(i, text)| {
            json!({
                "id": format!("genesis-8-{}", i + 15),
                "reference": format!("Genesis 8:{}", i + 15),
                "verses": [{"book": "Genesis", "chapter": 8, "verse": i + 15, "text": text}],
                "translation": "KJV",
                "confidence": 0.97,
                "source": "auto",
                "triggerText": "Genesis chapter eight",
                "passageId": "genesis-8-15-22",
                "passageReference": "Genesis 8:15-22",
                "passageIndex": i,
                "passageLength": 8,
            })
        })
        .collect()
}

fn with(base: &Value, changes: Value) -> Value {
    let mut out = base.clone();
    for (k, v) in changes.as_object().unwrap() {
        if v.is_null() {
            out.as_object_mut().unwrap().remove(k);
        } else {
            out[k] = v.clone();
        }
    }
    out
}

fn passage(from: usize, to: usize, id: &str, translation: &str) -> Vec<Value> {
    let base = &genesis()[0];
    (from..=to)
        .enumerate()
        .map(|(i, verse)| {
            with(
                base,
                json!({
                    "id": format!("{id}-{verse}"),
                    "reference": format!("John 1:{verse}"),
                    "verses": [{"book": "John", "chapter": 1, "verse": verse, "text": format!("verse {verse}")}],
                    "translation": translation,
                    "passageId": id,
                    "passageReference": format!("John 1:{from}-{to}"),
                    "passageIndex": i,
                    "passageLength": to - from + 1,
                }),
            )
        })
        .collect()
}

fn suggestions(values: &[Value]) -> Vec<ScriptureSuggestion> {
    values.iter().map(parse).collect()
}

#[test]
fn grouping_partition_and_navigation_match() {
    let g = genesis();
    let mut lists: Vec<Vec<Value>> = vec![
        vec![],
        g.clone(),
        vec![
            with(
                &g[0],
                json!({"id": "old", "passageId": "old", "passageReference": "Genesis 8:15"}),
            ),
            with(
                &g[1],
                json!({"id": "active", "passageId": "active", "passageReference": "Genesis 8:16"}),
            ),
            with(
                &g[2],
                json!({"id": "newer", "passageId": "newer", "passageReference": "Genesis 8:17"}),
            ),
            with(
                &g[3],
                json!({"id": "newest", "passageId": "newest", "passageReference": "Genesis 8:18"}),
            ),
            with(
                &g[4],
                json!({"id": "solo", "passageId": null, "passageReference": null, "passageIndex": null}),
            ),
        ],
        vec![
            with(&g[1], json!({"passageId": "row", "passageIndex": 1})),
            with(
                &g[0],
                json!({"passageId": "row", "passageIndex": 0, "id": "x"}),
            ),
            with(
                &g[0],
                json!({"passageId": "row", "passageIndex": 0, "id": "y"}),
            ),
            with(
                &g[0],
                json!({"passageId": "row", "passageIndex": 0, "id": "z", "translation": "NIV"}),
            ),
            with(&g[2], json!({"passageId": "row", "passageIndex": null})),
        ],
    ];
    lists.push(g.iter().rev().cloned().collect());
    let mut groups_cases = Vec::new();
    let mut partition_cases = Vec::new();
    for list in &lists {
        let groups = group_scripture_suggestions(&suggestions(list));
        groups_cases.push((json!([list]), js(&groups)));
        for count in [0usize, 1, 3, 10] {
            partition_cases.push((
                json!([groups, "active", count]),
                js(&partition_operator_suggestion_groups(
                    &groups,
                    Some("active"),
                    count,
                )),
            ));
        }
        partition_cases.push((
            json!([groups, null]),
            js(&partition_operator_suggestion_groups(
                &groups,
                None,
                DEFAULT_VISIBLE_GROUP_COUNT,
            )),
        ));
    }
    assert_all_match(
        "scripture-live-progress.groupScriptureSuggestions",
        &groups_cases,
    );
    assert_all_match(
        "scripture-live-progress.partitionOperatorSuggestionGroups",
        &partition_cases,
    );

    let ids: Vec<String> = ["verse-1", "verse-2", "verse-3"].map(String::from).into();
    let mut nav = Vec::new();
    for visible in [vec![], ids.clone(), ids[..1].to_vec()] {
        for selected in [
            None,
            Some("verse-1"),
            Some("verse-2"),
            Some("verse-3"),
            Some("missing"),
        ] {
            for delta in [-1i8, 1] {
                nav.push((
                    json!([visible, selected, delta]),
                    or_null(navigate_operator_suggestion_id(&visible, selected, delta)),
                ));
            }
        }
    }
    assert_all_match("scripture-live-progress.navigateOperatorSuggestionId", &nav);
}

/// Transcripts built from verse words: whole verses, partial, shuffled,
/// paraphrased with spoken aliases, and noise.
fn transcripts(seed: u64, count: usize) -> Vec<String> {
    let mut words: Vec<String> = GENESIS
        .iter()
        .flat_map(|v| v.split(' ').map(String::from))
        .collect();
    words.extend(["you", "out", "spoke", "to", "um", "church", "amen", "Then"].map(String::from));
    let strategy =
        prop::collection::vec(prop::sample::select(words), 0..30).prop_map(|w| w.join(" "));
    let mut out: Vec<String> = GENESIS.iter().map(|s| s.to_string()).collect();
    out.push("Then God spoke to Noah saying go out of the ark, you and your wife, and your sons and your sons wives with you".into());
    out.push(String::new());
    out.push("'s ' ''".into());
    out.extend(generate(seed, count, strategy));
    out
}

#[test]
fn reading_progress_and_tokenize_match() {
    let list = suggestions(&genesis());
    let texts = transcripts(18, 800);
    each_string("scripture-live-progress.tokenize", &texts, |s| {
        json!(tokenize(s))
    });
    let cases: Vec<_> = texts
        .iter()
        .map(|t| {
            (
                json!([t, genesis()]),
                or_null(find_reading_progress(t, &list)),
            )
        })
        .collect();
    assert_all_match("scripture-live-progress.findReadingProgress", &cases);
}

#[test]
fn merging_detections_matches() {
    let g = genesis();
    let guessed = with(&g[6], json!({"passageId": null, "passageReference": null}));
    let fresh = with(
        &g[1],
        json!({"id": "fresh", "passageId": "fresh-passage", "passageLength": 1, "passageReference": "Genesis 8:16", "passageIndex": 0}),
    );
    let nlt: Vec<Value> = g.iter().map(|i| with(i, json!({"id": format!("{}-nlt", i["id"].as_str().unwrap()), "translation": "NLT", "passageId": "genesis-8-15-22-nlt"}))).collect();
    let sequences: Vec<(Vec<Value>, Vec<Value>)> = vec![
        (vec![guessed.clone()], vec![g[0].clone()]),
        (g.clone(), vec![fresh.clone()]),
        (
            g.clone(),
            vec![with(&fresh, json!({"translation": "NKJV"}))],
        ),
        (g.clone(), nlt),
        (
            vec![],
            passage(1, 10, "said", "KJV")
                .into_iter()
                .chain(passage(1, 4, "ai", "KJV"))
                .collect(),
        ),
        (
            vec![],
            passage(1, 4, "short", "KJV")
                .into_iter()
                .chain(passage(1, 10, "long", "KJV"))
                .collect(),
        ),
        (
            vec![],
            passage(3, 5, "a", "KJV")
                .into_iter()
                .chain(passage(1, 10, "b", "NIV"))
                .chain(passage(4, 6, "c", "KJV"))
                .collect(),
        ),
        (g.clone(), vec![g[2].clone()]),
        (
            g.clone(),
            vec![with(
                &g[2],
                json!({"id": "odd", "passageReference": "Genesis 8:15\n-22", "passageId": "odd"}),
            )],
        ),
        (
            g.clone(),
            vec![with(
                &g[2],
                json!({"id": "dash", "passageReference": "genesis  8:15—16", "passageId": "dash"}),
            )],
        ),
        (
            g.clone(),
            vec![with(
                &g[2],
                json!({"id": "bad", "passageReference": "Genesis 8", "passageId": "bad"}),
            )],
        ),
        (
            g.clone(),
            vec![with(
                &g[2],
                json!({"id": "nolen", "passageLength": null, "passageId": "nolen", "reference": " GENESIS 8:17 "}),
            )],
        ),
        (
            vec![with(&g[0], json!({"verses": []}))],
            vec![with(
                &g[1],
                json!({"id": "w", "passageId": "w", "passageReference": "Genesis 8:15-16"}),
            )],
        ),
    ];
    let cases: Vec<_> = sequences
        .iter()
        .map(|(initial, incoming)| {
            let mut list = suggestions(initial);
            for item in suggestions(incoming) {
                list = merge_scripture_suggestion(&list, &item);
            }
            (json!([initial, incoming]), js(&list))
        })
        .collect();
    assert_all_match("oracle:scripture.mergeSequence", &cases);
}

#[test]
fn sending_and_auto_follow_match() {
    let g = genesis();
    let list = suggestions(&g);
    let mut sent_cases = Vec::new();
    let mut follow_cases = Vec::new();
    let sets: Vec<Vec<&str>> = vec![
        vec![],
        vec!["genesis-8-15", "genesis-8-16"],
        vec!["genesis-8-18"],
        vec!["genesis-8-17"],
    ];
    for sent in &sets {
        let set: HashSet<String> = sent.iter().map(|s| s.to_string()).collect();
        let (out, ids) = apply_operator_suggestion_sent(list.clone(), &set, "genesis-8-15");
        let mut ids: Vec<_> = ids.into_iter().collect();
        ids.sort();
        sent_cases.push((
            json!([g, sent, "genesis-8-15"]),
            json!({"suggestions": out, "sent": ids}),
        ));
        for active in ["genesis-8-15", "genesis-8-16", "genesis-8-17", "missing"] {
            for armed in ["genesis-8-15-22", "another"] {
                follow_cases.push((
                    json!([g, active, armed, sent]),
                    or_null(next_auto_follow_suggestion(&list, active, armed, &set)),
                ));
            }
        }
    }
    assert_all_match("oracle:scripture.suggestionSent", &sent_cases);
    assert_all_match("oracle:scripture.nextAutoFollow", &follow_cases);
}

#[test]
fn passage_follow_matches() {
    let g = genesis();
    let john: Vec<Value> = [
        "In the beginning was the Word, and the Word was with God, and the Word was God.",
        "The same was in the beginning with God.",
        "All things were made by him; and without him was not any thing made that was made.",
    ]
    .iter()
    .enumerate()
    .map(|(i, text)| with(&g[0], json!({"id": format!("john-1-{}", i + 1), "verses": [{"book": "John", "chapter": 1, "verse": i + 1, "text": text}], "passageId": "john-1-1-3", "passageIndex": i, "passageLength": 3})))
    .collect();
    type FollowScenario = (Value, Vec<(String, bool)>, Vec<Value>);
    let mut scenarios: Vec<FollowScenario> = vec![
        (
            json!({"passageId": "genesis-8-15-22", "currentIndex": 1, "matchedTokenIndexes": []}),
            vec![
                ("Go out of the ark, you and your wife".into(), true),
                (
                    "The ark reminds us that obedience can be difficult in every generation".into(),
                    true,
                ),
                ("and your sons and your sons wives with you".into(), true),
            ],
            g.clone(),
        ),
        (
            json!({"passageId": "john-1-1-3", "currentIndex": 0, "matchedTokenIndexes": []}),
            [
                "In the beginning",
                "In the beginning was the Word",
                "In the beginning was the Word and the Word was with God",
                "In the beginning was the Word and the Word was with God and the Word was God",
            ]
            .iter()
            .flat_map(|t| std::iter::repeat_n((t.to_string(), false), 3))
            .collect(),
            john.clone(),
        ),
        (
            json!({"passageId": "john-1-1-3", "currentIndex": 0, "matchedTokenIndexes": [], "readTo": 1.5, "heardCount": 2}),
            vec![(
                "was the Word and the Word was with God and the Word was God".into(),
                true,
            )],
            john.clone(),
        ),
        (
            json!({"passageId": "nope", "currentIndex": 0, "matchedTokenIndexes": [1]}),
            vec![("anything".into(), true)],
            g.clone(),
        ),
    ];
    // Generated reading sessions through the Genesis passage.
    let steps = prop::collection::vec(
        (prop::sample::select(transcripts(19, 60)), any::<bool>()),
        1..6,
    );
    for (i, session) in generate(20, 300, steps).into_iter().enumerate() {
        scenarios.push((
            json!({"passageId": "genesis-8-15-22", "currentIndex": i % 9, "matchedTokenIndexes": []}),
            session,
            g.clone(),
        ));
    }
    let cases: Vec<_> = scenarios
        .iter()
        .map(|(state, steps, list)| {
            let list = suggestions(list);
            let mut current: PassageFollowState = parse(state);
            let updates: Vec<_> = steps
                .iter()
                .map(|(transcript, final_text)| {
                    let update = update_passage_follow(&current, transcript, &list, *final_text);
                    current = update.state.clone();
                    update
                })
                .collect();
            (json!([state, steps, list]), js(&updates))
        })
        .collect();
    assert_all_match("oracle:scripture.followSequence", &cases);
}

#[test]
fn single_verse_presentation_and_tab_push_match() {
    let base = json!({"id": "range", "reference": "Acts 4:8–9", "verses": ([8, 9].map(|v| json!({"book": "Acts", "chapter": 4, "verse": v, "text": format!("Verse {v}")}))), "translation": "NKJV", "confidence": 1, "source": "manual", "triggerText": "Acts 4", "planId": "sermon", "planItemId": "item"});
    let inputs = [
        base.clone(),
        with(&base, json!({"verses": []})),
        with(&base, json!({"passageId": "p", "custom": {"kept": true}})),
        with(
            &base,
            json!({"verses": [{"book": "Acts", "chapter": 4.5, "verse": 1e21, "text": ""}]}),
        ),
    ];
    let mut single = Vec::new();
    let mut split = Vec::new();
    let mut push = Vec::new();
    for input in &inputs {
        let s: ScriptureSuggestion = parse(input);
        single.push((json!([input]), js(&single_verse_suggestion(&s))));
        split.push((json!([input]), js(&split_verse_suggestions(&s))));
        push.push((json!([input]), json!([["presentDirectly", s]])));
        // The Rust trait gets the same single call.
        let api = RecordingApi::default();
        block_on(push_scripture_from_tab(&api, &s)).unwrap();
        assert_eq!(api.calls.borrow().as_slice(), ["presentDirectly"]);
    }
    assert_all_match("single-verse-presentation.singleVerseSuggestion", &single);
    assert_all_match("single-verse-presentation.splitVerseSuggestions", &split);
    assert_all_match("oracle:scripture.tabPush", &push);
}

#[derive(Default)]
struct RecordingApi {
    calls: std::cell::RefCell<Vec<&'static str>>,
}

impl ScriptureTabPresentationApi for RecordingApi {
    type Error = ();
    async fn present_directly(&self, _: &ScriptureSuggestion) -> Result<(), ()> {
        self.calls.borrow_mut().push("presentDirectly");
        Ok(())
    }
    async fn register(&self, _: &ScriptureSuggestion) -> Result<(), ()> {
        self.calls.borrow_mut().push("register");
        Ok(())
    }
    async fn approve(&self, _: &str) -> Result<(), ()> {
        self.calls.borrow_mut().push("approve");
        Ok(())
    }
}

#[test]
fn detector_recovery_matches() {
    let mut cases = Vec::new();
    for errors in [0u32, 1, 2, 5] {
        for fallback in [false, true] {
            for probing in [false, true] {
                let state = DetectorRecoveryState {
                    consecutive_errors: errors,
                    fallback_active: fallback,
                    probing,
                };
                for event in DetectorRecoveryEvent::ALL {
                    cases.push((
                        json!([state, event]),
                        js(&transition_detector_recovery(state, *event)),
                    ));
                }
            }
        }
    }
    assert_all_match("detector-recovery.transitionDetectorRecovery", &cases);
}

// ─── Sermon plans ────────────────────────────────────────────────────────────

const ROMANS_8_28: &str = "And we know that all things work together for good to those who love God, to those who are the called according to His purpose.";
const ROMANS_8_29: &str = "For whom He foreknew, He also predestined to be conformed to the image of His Son, that He might be the firstborn among many brethren.";

fn item(
    id: &str,
    reference: &str,
    translation: &str,
    verses: &[(&str, f64, f64, &str)],
    available: bool,
) -> Value {
    json!({
        "id": id, "reference": reference, "translation": translation, "available": available,
        "verses": verses.iter().map(|(b, c, v, t)| json!({"book": b, "chapter": c, "verse": v, "text": t})).collect::<Vec<_>>(),
    })
}

fn plan(items: Vec<Value>) -> Value {
    json!({"id": "plan-1", "title": "Sunday", "sourceFileName": "notes.docx", "items": items, "createdAt": 0, "updatedAt": 0})
}

fn plans() -> Vec<Value> {
    let single = item(
        "item-single",
        "Romans 8:28",
        "NKJV",
        &[("Romans", 8.0, 28.0, ROMANS_8_28)],
        true,
    );
    let range = item(
        "item-range",
        "Romans 8:28–29",
        "NKJV",
        &[
            ("Romans", 8.0, 28.0, ROMANS_8_28),
            ("Romans", 8.0, 29.0, ROMANS_8_29),
        ],
        true,
    );
    let next = item(
        "next",
        "John 3:16",
        "NKJV",
        &[(
            "John",
            3.0,
            16.0,
            "For God so loved the world that he gave his only begotten son",
        )],
        true,
    );
    let wording = "the Lord has been mindful of us and he will bless us";
    vec![
        plan(vec![]),
        plan(vec![range.clone()]),
        plan(vec![range.clone(), single.clone()]),
        plan(vec![single.clone(), range.clone(), next.clone()]),
        plan(vec![
            item("u", "John 3:16", "NKJV", &[("John", 3.0, 16.0, "x")], false),
            item("e", "John 3:17", "NKJV", &[], true),
        ]),
        plan(vec![item(
            "short",
            "John 11:35",
            "NKJV",
            &[("John", 11.0, 35.0, "Jesus wept.")],
            true,
        )]),
        plan(vec![
            item(
                "earlier",
                "Psalms 115:12",
                "NKJV",
                &[("Psalms", 115.0, 12.0, wording)],
                true,
            ),
            item(
                "expected",
                "Psalms 116:12",
                "NKJV",
                &[("Psalms", 116.0, 12.0, wording)],
                true,
            ),
        ]),
        plan(vec![
            item(
                "nkjv",
                "Romans 8:28",
                "NKJV",
                &[("Romans", 8.0, 28.0, ROMANS_8_28)],
                true,
            ),
            item(
                "nlt",
                "Romans 8:28",
                "nlt",
                &[(
                    "  ROMANS ",
                    8.0,
                    28.0,
                    "And we know that God causes everything to work together for the good of those who love God.",
                )],
                true,
            ),
        ]),
        plan(vec![range.clone(), next.clone(), range.clone(), single]),
    ]
}

#[test]
fn sermon_plan_index_matches() {
    let cases: Vec<_> = plans()
        .iter()
        .map(|p| {
            let index = build_sermon_plan_index(&parse::<SermonPlan>(p));
            let quotable: Vec<_> = index
                .quotable
                .iter()
                .map(|q| json!({"entry": q.entry, "tokens": q.tokens}))
                .collect();
            (
                json!([p]),
                json!({
                    "planId": index.plan_id,
                    "ordered": index.ordered,
                    "expectedIndex": index.expected_index,
                    "byVerseKey": index.by_verse_key.iter().collect::<Vec<_>>(),
                    "byVerseTranslation": index.by_verse_translation.iter().collect::<Vec<_>>(),
                    "quotable": quotable,
                }),
            )
        })
        .collect();
    assert_all_match("oracle:scripture.planIndex", &cases);
}

#[test]
fn sermon_plan_lookups_match() {
    let refs = [
        json!({"book": "Romans", "chapter": 8, "verseStart": 28}),
        json!({"book": "romans", "chapter": 8, "verseStart": 28, "verseEnd": 29}),
        json!({"book": " Romans ", "chapter": 8, "verseStart": 28, "verseEnd": 30}),
        json!({"book": "Romans", "chapter": 8, "verseStart": 29, "verseEnd": 28}),
        json!({"book": "Romans", "chapter": 9, "verseStart": 28}),
        json!({"book": "John", "chapter": 3, "verseStart": 16}),
        json!({"book": "Psalms", "chapter": 116, "verseStart": 12}),
    ];
    let mut reference_cases = Vec::new();
    let mut quote_cases = Vec::new();
    let mut advance_cases = Vec::new();
    let transcripts = [
        "and we know that all things work together for good to those who love God",
        "God is good, and the things He called us to do work out for our purpose when we love Him, together as one people who know that",
        "Jesus wept, and He wept again",
        "   ",
        "the Lord has been mindful of us and he will bless us",
        "good morning church",
        ROMANS_8_29,
        "for god so loved the world that he gave his only begotten son",
    ];
    let all_plans: Vec<Option<Value>> = std::iter::once(None)
        .chain(plans().into_iter().map(Some))
        .collect();
    for p in &all_plans {
        let index = p
            .as_ref()
            .map(|p| build_sermon_plan_index(&parse::<SermonPlan>(p)));
        for r in &refs {
            for translation in [None, Some("NLT"), Some("nkjv"), Some("NIV"), Some("")] {
                let found =
                    match_plan_reference(index.as_ref(), &parse::<PlanReference>(r), translation);
                reference_cases.push((json!([p, r, translation]), js(&found)));
            }
        }
        for expected in [0usize, 1, 5] {
            for t in transcripts {
                for min in [None, Some(0.9)] {
                    let mut index = index.clone();
                    if let Some(index) = index.as_mut() {
                        index.expected_index = expected;
                    }
                    let found =
                        match_plan_quote(index.as_ref(), t, min.unwrap_or(DEFAULT_MIN_COVERAGE));
                    quote_cases.push((
                        json!([p, expected, t, min]),
                        or_null(found.map(|m| json!({"entry": m.entry, "coverage": m.coverage}))),
                    ));
                }
            }
        }
        if let (Some(p), Some(index)) = (p, index.as_ref()) {
            let steps: Vec<(&str, Option<&str>)> = vec![
                ("Romans 8:28", None),
                ("Romans 8:29", None),
                ("Romans 8:28", None),
                ("Genesis 1:1", None),
                ("John 3:16", None),
                ("Romans 8:28", Some("item-single")),
                ("Romans 8:28", Some("")),
                ("Romans 8:29", Some("item-range")),
                ("Psalms 116:12", None),
                ("Romans 8:28", Some("missing")),
            ];
            let mut index = index.clone();
            let results: Vec<_> = steps
                .iter()
                .map(|(reference, item_id)| {
                    let moved = advance_plan_expectation(&mut index, reference, *item_id);
                    json!([moved, index.expected_index])
                })
                .collect();
            advance_cases.push((json!([p, steps]), json!(results)));
        }
    }
    assert_all_match("oracle:scripture.planReference", &reference_cases);
    assert_all_match("oracle:scripture.planQuote", &quote_cases);
    assert_all_match("oracle:scripture.planAdvance", &advance_cases);

    let keys: Vec<_> = [
        ("Romans", 8.0, 28.0),
        ("  Song \t of  Solomon ", 2.0, 1.5),
        ("JOHN", -0.0, 1e21),
        ("", f64::NAN, 3.0),
    ]
    .into_iter()
    .map(|(b, c, v)| (json!([b, number(c), v]), json!(verse_key(b, c, v))))
    .collect();
    assert_all_match("sermon-plan-match.verseKey", &keys);
}

fn option(id: &str, access: &str, available: bool) -> Value {
    json!({"id": id, "name": id, "access": access, "available": available, "requiresApiKey": access == "api"})
}

#[test]
fn sermon_plan_resolving_matches() {
    let options = json!([
        option("NLT", "api", false),
        option("KJV", "local", true),
        option("BSB", "local", true),
        option("ESV", "api", true)
    ]);
    let parsed_options: Vec<ScriptureTranslationOption> = parse(&options);
    let mut order = Vec::new();
    for (requested, preferred) in [("NLT", "KJV"), ("KJV", "KJV"), ("ESV", "NLT"), ("", "bbe")] {
        order.push((
            json!([requested, preferred, options]),
            json!(translation_fallback_order(
                requested,
                preferred,
                &parsed_options
            )),
        ));
        order.push((
            json!([requested, preferred]),
            json!(translation_fallback_order(requested, preferred, &[])),
        ));
    }
    assert_all_match("sermon-plan-resolve.translationFallbackOrder", &order);

    let phil = json!({"reference": "Philippians 4:6–7", "translation": "KJV", "verses": [{"book": "Philippians", "chapter": 4, "verse": 6, "text": "Be careful for nothing"}, {"book": "Philippians", "chapter": 4, "verse": 7, "text": "And the peace of God"}]});
    let items = json!([
        {"id": "phil", "reference": "Philippians 4:6–7", "translation": "NLT", "verses": [], "available": false, "error": "NLT text is unavailable"},
        {"id": "ready", "reference": "John 3:16", "translation": "KJV", "verses": [{"book": "John", "chapter": 3, "verse": 16, "text": "t"}], "available": true},
        {"id": "nothing", "reference": "Nowhere 1:1", "translation": "NIV", "verses": [], "available": false, "extra": 1},
        {"id": "throws", "reference": "Throw 1:1", "translation": "ESV", "verses": [], "available": true},
        {"id": "same", "reference": "Same 1:1", "translation": "ESV", "verses": [], "available": false},
    ]);
    let script: Script = [
        ("Philippians 4:6–7|KJV".to_string(), json!([phil])),
        ("Philippians 4:6–7|NLT".to_string(), json!([])),
        ("Throw 1:1|ESV".to_string(), json!({"throw": "API.Bible is offline"})),
        ("Throw 1:1|KJV".to_string(), json!({"throw": "bundled database missing"})),
        ("Same 1:1|ESV".to_string(), json!([{"reference": "Same 1:1", "translation": "ESV", "verses": [{"book": "Same", "chapter": 1, "verse": 1, "text": "s"}]}])),
    ]
    .into();
    let parsed: Vec<SermonScriptureItem> = parse(&items);
    let mut cases = Vec::new();
    for (preferred, opts) in [("KJV", options.clone()), ("NIV", json!([]))] {
        let parsed_opts: Vec<ScriptureTranslationOption> = parse(&opts);
        let resolved = block_on(resolve_sermon_plan_items(
            &parsed,
            preferred,
            scripted(&script),
            &parsed_opts,
        ));
        cases.push((json!([items, preferred, script, opts]), js(&resolved)));
    }
    assert_all_match("oracle:scripture.resolveItems", &cases);
}

// ─── Sermon notes ────────────────────────────────────────────────────────────

#[test]
fn notes_labels_and_html_match() {
    let matches = [
        ("ps 2 3", "Psalms 2:3"),
        ("Mark 4:2 NLT", "Mark 4:2"),
        ("", ""),
        ("x", ""),
        ("1 Cor 13", "1 Corinthians 13"),
        ("İsaiah 1", "Isaiah 1"),
        ("\u{212A}ings 1", "Kings 1"),
    ];
    let label_cases: Vec<_> = matches
        .iter()
        .map(|(text, reference)| {
            let m = SermonReferenceMatch {
                start: 0.0,
                end: 1.0,
                text: text.to_string(),
                reference: reference.to_string(),
                translations: vec!["NKJV".into()],
            };
            (json!([m]), or_null(normalized_reference_label(&m)))
        })
        .collect();
    assert_all_match("sermon-notes-review.normalizedReferenceLabel", &label_cases);

    let mut texts: Vec<String> = [
        "ABEL  Genesis 4:3-5\nNOAH",
        "",
        " ",
        "  ",
        "a\r\nb\rc",
        "\t<x> & y ",
        "   lead",
        "trail   ",
        " a  b   c ",
    ]
    .map(String::from)
    .into();
    let parts = prop::sample::select(vec![
        " ", "  ", "\t", "\n", "\r\n", "\r", "a", "<", ">", "&", "😀",
    ]);
    texts.extend(generate(
        21,
        400,
        prop::collection::vec(parts, 0..10).prop_map(|p| p.concat()),
    ));
    each_string("sermon-notes-review.escapeHtml", &texts, |s| {
        json!(escape_html(s))
    });
    each_string("sermon-notes-review.plainTextToEditorHtml", &texts, |s| {
        json!(plain_text_to_editor_html(s))
    });
    assert_all_match(
        "sermon-notes-review.SERMON_NOTES_BLOCK_SEPARATOR",
        &[(json!([]), json!(SERMON_NOTES_BLOCK_SEPARATOR))],
    );
}

/// A StarterKit ProseMirror JSON node as this crate's notes model.
fn notes_node(value: &Value) -> SermonNotesNode {
    let kind = value["type"].as_str().unwrap();
    let content = value["content"]
        .as_array()
        .map(|c| c.iter().map(notes_node).collect())
        .unwrap_or_default();
    match kind {
        "text" => SermonNotesNode::text(value["text"].as_str().unwrap()),
        "hardBreak" => SermonNotesNode::hard_break(),
        "horizontalRule" => SermonNotesNode::Element {
            type_name: kind.into(),
            block: true,
            leaf: true,
            content: vec![],
        },
        _ => SermonNotesNode::block(kind, content),
    }
}

fn text(t: &str) -> Value {
    json!({"type": "text", "text": t})
}

fn para(inline: Vec<Value>) -> Value {
    json!({"type": "paragraph", "content": inline})
}

/// Valid StarterKit documents: paragraphs with hard breaks, headings, lists,
/// quotes, rules and code blocks, with astral characters for UTF-16 offsets.
fn notes_doc_strategy() -> impl Strategy<Value = Value> {
    let words = prop::sample::select(vec![
        "Genesis 1:1",
        "John 3:16",
        "a",
        "😀",
        "Romans 8:28 ",
        " ",
        "é",
    ]);
    let inline = prop::collection::vec(
        prop_oneof![words.prop_map(text), Just(json!({"type": "hardBreak"}))],
        0..4,
    )
    // ProseMirror joins adjacent text nodes; the JSON never holds two in a row.
    .prop_map(|nodes| {
        let mut out: Vec<Value> = Vec::new();
        for node in nodes {
            match (out.last_mut(), node["type"].as_str()) {
                (Some(last), Some("text")) if last["type"] == "text" => {
                    last["text"] = json!(format!(
                        "{}{}",
                        last["text"].as_str().unwrap(),
                        node["text"].as_str().unwrap()
                    ));
                }
                _ => out.push(node),
            }
        }
        out
    });
    let block = prop_oneof![
        inline.clone().prop_map(para),
        inline.clone().prop_map(|i| json!({"type": "heading", "attrs": {"level": 2}, "content": i})),
        prop::collection::vec(inline.clone(), 1..3).prop_map(|items| json!({"type": "bulletList", "content": items.into_iter().map(|i| json!({"type": "listItem", "content": [para(i)]})).collect::<Vec<_>>()})),
        prop::collection::vec(inline.clone(), 1..3).prop_map(|items| json!({"type": "orderedList", "content": items.into_iter().map(|i| json!({"type": "listItem", "content": [para(i)]})).collect::<Vec<_>>()})),
        inline.clone().prop_map(|i| json!({"type": "blockquote", "content": [para(i)]})),
        Just(json!({"type": "horizontalRule"})),
        prop::sample::select(vec!["let x = 1", "😀 code"]).prop_map(|t| json!({"type": "codeBlock", "content": [text(t)]})),
    ];
    prop::collection::vec(block, 1..6).prop_map(|blocks| json!({"type": "doc", "content": blocks}))
}

#[test]
fn notes_offsets_match_tiptap() {
    let lines = [
        "STS BIBLICAL EXAMPLES",
        "",
        "INTRODUCTION",
        "#3 FATHER ABRAHAM Genesis 14:18-20 and Genesis 28:20-22",
        "#4 DAVID 1 Samuel 17:48-51",
        "E Esther 2:5-11; Esth 4:14",
        "Hebrews 7:1-7",
    ];
    let mut docs: Vec<Value> = vec![
        json!({"type": "doc", "content": lines.iter().map(|l| if l.is_empty() { json!({"type": "paragraph"}) } else { para(vec![text(l)]) }).collect::<Vec<_>>()}),
        json!({"type": "doc", "content": [
            para(vec![text("Intro Genesis 1:1")]),
            {"type": "orderedList", "content": [
                {"type": "listItem", "content": [para(vec![text("First John 3:16")])]},
                {"type": "listItem", "content": [para(vec![text("Second Romans 8:28")])]},
            ]},
        ]}),
        json!({"type": "doc", "content": [{"type": "paragraph"}]}),
    ];
    docs.extend(generate(22, 250, notes_doc_strategy()));
    for separator in ["\n", "\n\n", ""] {
        let cases: Vec<_> = docs
            .iter()
            .map(|doc| {
                let node = notes_node(doc);
                let plain = notes_plain_text(&node, separator);
                let length = plain.encode_utf16().count() as f64;
                let mut ranges: Vec<(f64, f64)> = vec![
                    (-1.0, 2.0),
                    (3.0, 2.0),
                    (0.0, 0.0),
                    (0.0, length),
                    (length, length + 1.0),
                    (0.5, 2.0),
                    (0.0, 1.0),
                ];
                for needle in [
                    "Genesis 1:1",
                    "John 3:16",
                    "Romans 8:28",
                    "Genesis 14:18-20",
                    "1 Samuel 17:48-51",
                    "Hebrews 7:1-7",
                    "😀",
                ] {
                    if let Some(at) = plain.find(needle) {
                        let start = plain[..at].encode_utf16().count() as f64;
                        ranges.push((start, start + needle.encode_utf16().count() as f64));
                    }
                }
                let mapped: Vec<_> = ranges
                    .iter()
                    .map(|(s, e)| or_null(map_text_range_to_pos(&node, *s, *e, separator)))
                    .collect();
                (
                    json!([doc, ranges, separator]),
                    json!({"text": plain, "ranges": mapped}),
                )
            })
            .collect();
        assert_all_match("oracle:scripture.mapRange", &cases);
    }
}

// ─── Offline state ───────────────────────────────────────────────────────────

#[test]
fn cache_notices_and_attribution_match() {
    use api_cache_messages::*;
    let messages = [
        NOT_AVAILABLE_OFFLINE,
        REFRESH_REQUIRED,
        ACCESS_REVOKED,
        OFFLINE_DOWNLOAD_NOT_PERMITTED,
        "",
        "Search failed",
        "No results. Try a reference like “John 3:16”.",
        "this passage is not available offline.",
    ];
    let mut cases = Vec::new();
    for message in messages {
        for kind in ["error", "string"] {
            cases.push((
                json!([kind, message]),
                or_null(get_scripture_cache_notice(message)),
            ));
        }
    }
    for kind in ["null", "number"] {
        cases.push((json!([kind, ""]), or_null(get_scripture_cache_notice(""))));
    }
    assert_all_match("oracle:scripture.cacheNotice", &cases);
    let shared = json!({"notAvailableOffline": NOT_AVAILABLE_OFFLINE, "refreshRequired": REFRESH_REQUIRED, "accessRevoked": ACCESS_REVOKED, "offlineDownloadNotPermitted": OFFLINE_DOWNLOAD_NOT_PERMITTED});
    assert_all_match(
        "oracle:scripture.apiCacheMessages",
        &[(
            json!([]),
            json!({"policy": shared, "shared": shared, "attribution": {"label": API_BIBLE_ATTRIBUTION_LABEL, "url": API_BIBLE_ATTRIBUTION_URL}}),
        )],
    );
    let attribution: Vec<_> = [
        Some(option("NKJV", "api", true)),
        Some(option("KJV", "local", true)),
        None,
    ]
    .into_iter()
    .map(|o| {
        let parsed: Option<ScriptureTranslationOption> = o.as_ref().map(parse);
        (
            json!([o.unwrap_or(kairo_oracle::undefined())]),
            json!(should_show_api_bible_attribution(parsed.as_ref())),
        )
    })
    .collect();
    assert_all_match(
        "scripture-offline-state.shouldShowApiBibleAttribution",
        &attribution,
    );
}

const NOW: f64 = 1_787_616_000_000.0;

fn offline_row(changes: Value) -> Value {
    with(
        &json!({"bibleId": "nkjv-01", "translation": "NKJV", "name": "New King James Version", "copyright": "© Thomas Nelson", "status": "downloaded", "cachedChapters": 1189, "totalChapters": 1189, "cachedVerses": 31102, "fetchedAt": NOW, "expiresAt": NOW + 2_592_000_000.0, "offlineDownloadEnabled": true}),
        changes,
    )
}

#[test]
fn offline_bible_rows_match() {
    let mut rows = Vec::new();
    for status in [
        "not-downloaded",
        "partial",
        "downloading",
        "paused",
        "downloaded",
        "stale",
        "unavailable",
        "failed",
    ] {
        for enabled in [true, false] {
            for (cached, total) in [
                (594.0, 1188.0),
                (5.0, 0.0),
                (2000.0, 1189.0),
                (1.0, 3.0),
                (-5.0, 10.0),
                (1.0, 200.0),
            ] {
                rows.push(offline_row(json!({"status": status, "offlineDownloadEnabled": enabled, "cachedChapters": cached, "totalChapters": total})));
            }
            rows.push(offline_row(
                json!({"status": status, "error": "network down"}),
            ));
            rows.push(offline_row(json!({"status": status, "error": ""})));
        }
    }
    let row_cases: Vec<_> = rows
        .iter()
        .map(|r| {
            let row: ApiBibleOfflineTranslation = parse(r);
            let actions = get_offline_bible_actions(&row);
            (
                json!([r]),
                json!({
                    "actions": actions,
                    "labels": actions.iter().map(|a| a.label()).collect::<Vec<_>>(),
                    "status": get_offline_bible_status_label(&row),
                    "percent": get_offline_bible_progress_percent(&row),
                }),
            )
        })
        .collect();
    assert_all_match("oracle:scripture.offlineRow", &row_cases);

    let expiry: Vec<_> = [
        json!(null),
        json!(NOW - 1.0),
        json!(NOW),
        json!(NOW + 86_400_000.0),
    ]
    .into_iter()
    .map(|at| {
        let r = offline_row(json!({}))
            .as_object()
            .cloned()
            .map(|mut o| {
                o.insert("expiresAt".into(), at);
                Value::Object(o)
            })
            .unwrap();
        let row: ApiBibleOfflineTranslation = parse(&r);
        let text = format_offline_bible_expiry(&row, NOW, |_| String::new());
        (json!([r, NOW]), json!(text))
    })
    .collect();
    assert_all_match("oracle:scripture.offlineExpiry", &expiry);

    let table = vec![
        offline_row(json!({})),
        offline_row(
            json!({"bibleId": "nlt-01", "translation": "NLT", "status": "failed", "error": "old"}),
        ),
    ];
    let progresses = [
        json!({"bibleId": "nlt-01", "status": "downloading", "completedChapters": 10, "totalChapters": 1189, "cachedVerses": 200}),
        json!({"bibleId": "nkjv-01", "status": "not-downloaded", "completedChapters": 0, "totalChapters": 0, "cachedVerses": 0}),
        json!({"bibleId": "nlt-01", "status": "failed", "completedChapters": 3, "totalChapters": 9, "cachedVerses": 7, "error": "boom"}),
        json!({"bibleId": "nlt-01", "status": "failed", "completedChapters": 3, "totalChapters": 9, "cachedVerses": 7, "error": ""}),
        json!({"bibleId": "missing", "status": "paused", "completedChapters": 3, "totalChapters": 9, "cachedVerses": 7}),
    ];
    let parsed_table: Vec<ApiBibleOfflineTranslation> = table.iter().map(parse).collect();
    let merge: Vec<_> = progresses
        .iter()
        .map(|p| {
            let progress: ApiBibleDownloadProgress = parse(p);
            (
                json!([table, p]),
                js(&merge_offline_download_progress(&parsed_table, &progress)),
            )
        })
        .collect();
    assert_all_match("oracle:scripture.offlineMerge", &merge);
}

// ─── Traces ──────────────────────────────────────────────────────────────────

const T0: f64 = 1_700_000_000_000.0;

fn trace_strategy() -> impl Strategy<Value = Value> {
    let stamp = prop::option::of(prop_oneof![
        (-50i64..2000).prop_map(|d| T0 + d as f64),
        Just(T0)
    ]);
    let stamps = prop::collection::vec(stamp, 12);
    (
        prop::sample::select(vec!["explicit", "quotation-local", "sermon-plan", "ai"]),
        prop::sample::select(vec!["interim", "final"]),
        prop::sample::select(vec!["active", "presented", "cancelled", "superseded", "failed"]),
        prop::option::of(prop::sample::select(vec!["KJV", "NIV"])),
        prop::option::of(prop::sample::select(vec![0.0, -0.0, 1000.0])),
        prop::option::of(prop::sample::select(vec!["connected", "degraded", "disconnected"])),
        stamps,
        0u32..1000,
    )
        .prop_map(|(resolver, source, status, translation, delay, mode, stamps, id)| {
            let mut trace = json!({"correlationId": format!("c{id}"), "sourceText": "psalm", "transcriptSource": source, "resolver": resolver, "status": status, "sttReceivedAt": T0});
            for (stage, at) in ScriptureTraceStage::ALL.iter().zip(stamps) {
                if let Some(at) = at {
                    trace[stage.as_str()] = json!(at);
                }
            }
            if let Some(t) = translation {
                trace["translation"] = json!(t);
            }
            if let Some(d) = delay {
                trace["autoPresentDelayMs"] = json!(d);
            }
            if let Some(m) = mode {
                trace["presenterConnectionMode"] = json!(m);
            }
            trace
        })
}

#[test]
fn trace_metrics_and_summaries_match() {
    let traces = generate(23, 600, trace_strategy());
    let parsed: Vec<ScriptureLatencyTrace> = traces.iter().map(parse).collect();
    let metrics: Vec<_> = traces
        .iter()
        .zip(&parsed)
        .map(|(v, t)| (json!([v]), js(&derive_metrics(t))))
        .collect();
    assert_all_match("scripture-trace.deriveMetrics", &metrics);
    let stages: Vec<_> = traces
        .iter()
        .zip(&parsed)
        .map(|(v, t)| {
            (
                json!([v]),
                json!(
                    recorded_stages(t)
                        .iter()
                        .map(|(s, at)| json!({"stage": s.as_str(), "at": at}))
                        .collect::<Vec<_>>()
                ),
            )
        })
        .collect();
    assert_all_match("scripture-trace.recordedStages", &stages);
    let terminal: Vec<_> = ["active", "presented", "cancelled", "superseded", "failed"]
        .iter()
        .map(|s| (json!([s]), json!(is_terminal(parse(&json!(s))))))
        .collect();
    assert_all_match("scripture-trace.isTerminal", &terminal);

    let batches: Vec<Vec<Value>> = traces
        .chunks(37)
        .map(<[Value]>::to_vec)
        .chain([vec![], traces.clone()])
        .collect();
    let summaries: Vec<_> = batches
        .iter()
        .map(|b| {
            let parsed: Vec<ScriptureLatencyTrace> = b.iter().map(parse).collect();
            (json!([b]), js(&summarize_by_resolver(&parsed)))
        })
        .collect();
    assert_all_match("scripture-trace.summarizeByResolver", &summaries);

    let samples = prop::collection::vec(0u32..500, 0..12).prop_map(|mut v| {
        v.sort();
        v.into_iter().map(f64::from).collect::<Vec<_>>()
    });
    let fractions = prop::sample::select(vec![
        0.0,
        0.5,
        0.9,
        0.95,
        0.99,
        1.0,
        -1.0,
        2.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.333,
    ]);
    let percentile_cases: Vec<_> = generate(24, 400, (samples, fractions))
        .into_iter()
        .map(|(sorted, fraction)| {
            (
                json!([sorted, number(fraction)]),
                percentile(&sorted, fraction).map_or(kairo_oracle::undefined(), number),
            )
        })
        .collect();
    assert_all_match("scripture-trace.percentile", &percentile_cases);
}

#[test]
fn trace_store_matches() {
    let start = |id: &str| json!({"op": "start", "input": {"correlationId": id, "sourceText": "psalm one nineteen nine", "transcriptSource": "interim", "resolver": "explicit", "sttReceivedAt": T0}});
    let scripted: Vec<(Option<usize>, Vec<Value>)> = vec![
        (
            None,
            vec![
                start("a"),
                json!({"op": "mark", "id": "a", "stage": "citationRecognizedAt", "at": T0 + 4.0}),
                json!({"op": "mark", "id": "a", "stage": "bibleLookupStartedAt", "at": T0 + 5.0}),
                json!({"op": "mark", "id": "a", "stage": "bibleLookupCompletedAt", "at": T0 + 19.0}),
                json!({"op": "metrics", "id": "a"}),
            ],
        ),
        (
            None,
            vec![
                start("m"),
                json!({"op": "annotate", "id": "m", "fields": {"translation": "KJV", "autoPresentDelayMs": 1000, "presenterConnectionMode": "connected", "presenterOutputMode": "library"}}),
                json!({"op": "get", "id": "m"}),
                json!({"op": "annotate", "id": "x", "fields": {"translation": "NIV"}}),
            ],
        ),
        (
            None,
            vec![
                start("a"),
                json!({"op": "mark", "id": "a", "stage": "suggestionRenderedAt", "at": T0 + 20.0}),
                json!({"op": "mark", "id": "a", "stage": "suggestionRenderedAt", "at": T0 + 45.0}),
            ],
        ),
        (
            None,
            vec![
                start("a"),
                json!({"op": "complete", "id": "a", "input": "cancelled"}),
                json!({"op": "mark", "id": "a", "stage": "presenterRequestCompletedAt", "at": T0 + 900.0}),
                json!({"op": "complete", "id": "a", "input": {"status": "presented"}}),
                json!({"op": "metrics", "id": "a"}),
            ],
        ),
        (
            None,
            vec![
                start("a"),
                json!({"op": "complete", "id": "a", "input": {"status": "presented", "confirmation": "request-accepted"}}),
                json!({"op": "mark", "id": "a", "stage": "suggestionRenderedAt", "at": T0 + 30.0}),
                json!({"op": "metrics", "id": "a"}),
            ],
        ),
        (
            None,
            vec![
                start("a"),
                json!({"op": "complete", "id": "a", "input": {"status": "failed", "failureReason": "presenter-disconnected", "error": "ECONNREFUSED"}}),
            ],
        ),
        (
            None,
            vec![
                start("ai"),
                json!({"op": "complete", "id": "ai", "input": {"status": "superseded", "supersededBy": "explicit-1", "error": ""}}),
            ],
        ),
        (
            None,
            vec![
                json!({"op": "mark", "id": "nope", "stage": "countdownStartedAt", "at": T0}),
                json!({"op": "complete", "id": "nope", "input": "presented"}),
                json!({"op": "get", "id": "nope"}),
                json!({"op": "metrics", "id": "nope"}),
            ],
        ),
        (
            Some(3),
            vec![
                start("a"),
                start("b"),
                start("c"),
                start("d"),
                json!({"op": "recent", "limit": 50}),
                json!({"op": "get", "id": "a"}),
            ],
        ),
        (
            None,
            vec![
                start("a"),
                start("b"),
                start("c"),
                json!({"op": "recent", "limit": 2}),
                json!({"op": "recent", "limit": 0}),
                json!({"op": "clear"}),
                json!({"op": "recent", "limit": 50}),
            ],
        ),
        // A restarted id is listed twice and evicted by its older entry, as in the source.
        (
            Some(2),
            vec![
                start("a"),
                start("b"),
                start("a"),
                json!({"op": "recent", "limit": 50}),
                start("c"),
                json!({"op": "recent", "limit": 50}),
            ],
        ),
        (
            Some(0),
            vec![start("a"), json!({"op": "recent", "limit": 5})],
        ),
    ];
    let cases: Vec<_> = scripted
        .iter()
        .map(|(limit, ops)| {
            let mut store = limit.map_or_else(ScriptureTraceStore::default, ScriptureTraceStore::new);
            let mut observed = Vec::new();
            for op in ops {
                let id = op["id"].as_str().unwrap_or_default();
                match op["op"].as_str().unwrap() {
                    "start" => {
                        store.start(parse(&op["input"]));
                    }
                    "mark" => store.mark(id, ScriptureTraceStage::parse(op["stage"].as_str().unwrap()).unwrap(), op["at"].as_f64().unwrap()),
                    "complete" => {
                        let input = &op["input"];
                        let complete = if let Some(status) = input.as_str() {
                            CompleteInput::from(parse::<TraceStatus>(&json!(status)))
                        } else {
                            CompleteInput {
                                status: parse(&input["status"]),
                                confirmation: input.get("confirmation").map(parse),
                                failure_reason: input.get("failureReason").map(parse),
                                error: input.get("error").map(parse),
                                superseded_by: input.get("supersededBy").map(parse),
                            }
                        };
                        store.complete(id, complete);
                    }
                    "annotate" => {
                        let f = &op["fields"];
                        store.annotate(id, TraceAnnotation {
                            translation: f.get("translation").map(parse),
                            auto_present_delay_ms: f.get("autoPresentDelayMs").map(parse),
                            presenter_connection_mode: f.get("presenterConnectionMode").map(parse),
                            presenter_output_mode: f.get("presenterOutputMode").map(parse),
                        });
                    }
                    "clear" => store.clear(),
                    "recent" => observed.push(json!(store.get_recent(op["limit"].as_u64().unwrap() as usize).iter().map(|t| &t.correlation_id).collect::<Vec<_>>())),
                    "get" => observed.push(or_null(store.get(id))),
                    "metrics" => observed.push(or_null(store.metrics(id))),
                    other => panic!("unknown op {other}"),
                }
            }
            let limit_arg = limit.map_or(kairo_oracle::undefined(), |l| json!(l));
            (
                json!([limit_arg, ops]),
                json!({"size": store.len(), "observed": observed, "recent": store.get_recent(usize::MAX)}),
            )
        })
        .collect();
    assert_all_match("oracle:scripture.traceStoreReplay", &cases);
}
