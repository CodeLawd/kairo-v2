//! Pure API.Bible mapping and parsing, from `api-bible.ts` and the parsing
//! half of `api-bible-client.ts`.

use crate::api::ApiSearchHit;
use kairo_domain::js::{is_whitespace, trim, value_to_string};
use kairo_domain::scripture::translations::{
    BIBLE_TRANSLATIONS, build_abbreviation_aliases, build_name_patterns, build_preferred_bible_ids,
};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// One row of `GET /bibles`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApiBibleSummary {
    pub id: String,
    pub abbreviation: Option<String>,
    pub abbreviation_local: Option<String>,
    pub name: Option<String>,
    pub name_local: Option<String>,
    pub kind: Option<String>,
}

impl ApiBibleSummary {
    pub(crate) fn from_value(value: &Value) -> Option<Self> {
        let row = value.as_object()?;
        let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            id: row.get("id").map(value_to_string).unwrap_or_default(),
            abbreviation: text("abbreviation"),
            abbreviation_local: text("abbreviationLocal"),
            name: text("name"),
            name_local: text("nameLocal"),
            kind: text("type"),
        })
    }
}

/// A parsed verse from passage or chapter content.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedApiBibleVerse {
    pub book: String,
    pub chapter: f64,
    pub verse: f64,
    pub text: String,
}

/// Canonical book order to API.Bible USFM code (index = canonical book id − 1).
#[rustfmt::skip]
pub const USFM_BOOK_CODES: [&str; 66] = [
    "GEN","EXO","LEV","NUM","DEU","JOS","JDG","RUT","1SA","2SA","1KI","2KI","1CH","2CH","EZR","NEH","EST","JOB","PSA","PRO","ECC","SNG","ISA","JER","LAM","EZK","DAN","HOS","JOL","AMO","OBA","JON","MIC","NAM","HAB","ZEP","HAG","ZEC","MAL",
    "MAT","MRK","LUK","JHN","ACT","ROM","1CO","2CO","GAL","EPH","PHP","COL","1TH","2TH","1TI","2TI","TIT","PHM","HEB","JAS","1PE","2PE","1JN","2JN","3JN","JUD","REV",
];

/// Canonical book id (1-based) for a USFM book code.
pub fn canonical_book_id_from_usfm(code: &str) -> Option<u32> {
    let upper = code.to_uppercase();
    USFM_BOOK_CODES
        .iter()
        .position(|c| *c == upper)
        .map(|i| i as u32 + 1)
}

fn normalize_abbreviation(value: Option<&str>) -> String {
    value
        .unwrap_or_default()
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        .collect()
}

fn is_catalog_id(id: &str) -> bool {
    BIBLE_TRANSLATIONS
        .iter()
        .any(|entry| entry.id.to_uppercase() == id)
}

/// `^(ENG|EN)(?=[A-Z])`, with the regex engine's backtracking.
fn strip_language(value: &str) -> &str {
    let followed = |rest: &str| rest.starts_with(|c: char| c.is_ascii_uppercase());
    if let Some(rest) = value.strip_prefix("ENG")
        && followed(rest)
    {
        return rest;
    }
    if let Some(rest) = value.strip_prefix("EN")
        && followed(rest)
    {
        return rest;
    }
    value
}

fn catalog_id_from_abbreviation(value: Option<&str>) -> Option<(String, bool)> {
    let normalized = normalize_abbreviation(value);
    if normalized.is_empty() {
        return None;
    }
    if is_catalog_id(&normalized) {
        return Some((normalized, true));
    }
    let aliases = build_abbreviation_aliases();
    let without = strip_language(&normalized);
    if without != normalized {
        if is_catalog_id(without) {
            return Some((without.to_owned(), true));
        }
        if let Some(id) = aliases.get(without) {
            return Some(((*id).to_owned(), false));
        }
    }
    aliases.get(&normalized).map(|id| ((*id).to_owned(), false))
}

fn catalog_id_from_name(value: Option<&str>) -> Option<&'static str> {
    let value = value.filter(|v| !v.is_empty())?;
    build_name_patterns()
        .into_iter()
        .find(|pattern| pattern.is_match(value))
        .map(|pattern| pattern.id)
}

/// `buildApiBibleIdMap`: catalogue id to API.Bible id, in first-seen order.
pub fn build_api_bible_id_map(bibles: &[ApiBibleSummary]) -> Vec<(String, String)> {
    let preferred = build_preferred_bible_ids();
    let mut best: Vec<(String, String, u32)> = Vec::new();
    let mut consider = |catalog: String, bible: &str, score: u32| match best
        .iter_mut()
        .find(|(id, ..)| *id == catalog)
    {
        Some(current) if score > current.2 => {
            current.1 = bible.to_owned();
            current.2 = score;
        }
        Some(_) => {}
        None => best.push((catalog, bible.to_owned(), score)),
    };
    for bible in bibles {
        if bible
            .kind
            .as_deref()
            .is_some_and(|k| !k.is_empty() && k != "text")
        {
            continue;
        }
        if let Some(id) = preferred.get(bible.id.as_str()) {
            consider((*id).to_owned(), &bible.id, 85);
        }
        if let Some((id, exact)) = catalog_id_from_abbreviation(bible.abbreviation_local.as_deref())
        {
            consider(id, &bible.id, if exact { 100 } else { 80 });
        }
        if let Some((id, exact)) = catalog_id_from_abbreviation(bible.abbreviation.as_deref()) {
            consider(id, &bible.id, if exact { 90 } else { 70 });
        }
        let named = catalog_id_from_name(bible.name_local.as_deref())
            .or_else(|| catalog_id_from_name(bible.name.as_deref()));
        if let Some(id) = named {
            consider(id.to_owned(), &bible.id, 40);
        }
    }
    best.into_iter().map(|(id, bible, _)| (id, bible)).collect()
}

static VERSE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[1-3]?[A-Z]{2,3}\.([0-9]+)\.([0-9]+)$").unwrap());

fn collapse(value: &str, sep: &str) -> String {
    value
        .split(is_whitespace)
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(sep)
}

fn normalize_verse_id(value: Option<&Value>) -> Option<String> {
    let value = value?.as_str().filter(|v| !v.is_empty())?;
    // `trim().replace(/\s+/g, '.').replace(/:/g, '.')`
    let trimmed = trim(value);
    let mut out = String::new();
    let mut in_space = false;
    for c in trimmed.chars() {
        if is_whitespace(c) {
            if !in_space {
                out.push('.');
            }
            in_space = true;
        } else {
            in_space = false;
            out.push(if c == ':' { '.' } else { c });
        }
    }
    VERSE_ID.is_match(&out).then_some(out)
}

/// `parseApiBiblePassageContent`.
pub fn parse_api_bible_passage_content(
    content: &Value,
    book_name: &str,
) -> Vec<ParsedApiBibleVerse> {
    let Some(nodes) = content.as_array() else {
        return Vec::new();
    };
    let mut parts: Vec<(String, Vec<String>)> = Vec::new();
    let mut current: Option<String> = None;

    fn visit(
        node: &Value,
        inside: bool,
        current: &mut Option<String>,
        parts: &mut Vec<(String, Vec<String>)>,
    ) {
        let get = |k: &str| node.get(k);
        let is_marker = get("name").and_then(Value::as_str) == Some("verse");
        let attrs = get("attrs");
        let attr = |k: &str| attrs.and_then(|a| a.get(k)).filter(|v| !v.is_null());
        let raw = attr("verseId")
            .or_else(|| {
                attr("verseOrgIds")
                    .and_then(|ids| ids.get(0))
                    .filter(|v| !v.is_null())
            })
            .or_else(|| attr("sid"));
        if let Some(id) = normalize_verse_id(raw) {
            *current = Some(id);
        }
        let text = get("text")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty());
        if get("type").and_then(Value::as_str) == Some("text")
            && !inside
            && let (Some(text), Some(id)) = (text, current.as_ref())
        {
            match parts.iter_mut().find(|(k, _)| k == id) {
                Some((_, list)) => list.push(text.to_owned()),
                None => parts.push((id.clone(), vec![text.to_owned()])),
            }
        }
        if let Some(items) = get("items").and_then(Value::as_array) {
            for child in items {
                visit(child, inside || is_marker, current, parts);
            }
        }
    }

    for node in nodes {
        visit(node, false, &mut current, &mut parts);
    }
    parts
        .into_iter()
        .filter_map(|(id, list)| {
            let caps = VERSE_ID.captures(&id)?;
            let text = collapse(&list.join(" "), " ");
            if text.is_empty() {
                return None;
            }
            Some(ParsedApiBibleVerse {
                book: book_name.to_owned(),
                chapter: caps[1].parse().ok()?,
                verse: caps[2].parse().ok()?,
                text,
            })
        })
        .collect()
}

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static HIT_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^([1-3]?[A-Z]{2,3})\.([0-9]+)\.([0-9]+)$").unwrap());
static HIT_REF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(.+?)[\s\u{feff}]+([0-9]+):([0-9]+)(?:$|[^0-9A-Za-z_])").unwrap()
});

fn strip_html(value: &str) -> String {
    let mut out = TAG.replace_all(value, " ").into_owned();
    for (from, to) in [
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
    ] {
        out = replace_ascii_ci(&out, from, to);
    }
    collapse(&out, " ")
}

fn replace_ascii_ci(haystack: &str, needle: &str, to: &str) -> String {
    let lower = haystack.to_ascii_lowercase();
    let mut out = String::with_capacity(haystack.len());
    let mut last = 0;
    for (i, _) in lower.match_indices(needle) {
        out.push_str(&haystack[last..i]);
        out.push_str(to);
        last = i + needle.len();
    }
    out.push_str(&haystack[last..]);
    out
}

fn js_string(value: Option<&Value>) -> Option<String> {
    value.filter(|v| !v.is_null()).map(value_to_string)
}

/// `parseApiBibleSearchResponse`: accepts `{ data: { verses } }`,
/// `{ data: { passages } }` and a flat `data` array.
pub fn parse_api_bible_search_response(payload: &Value) -> Vec<ApiSearchHit> {
    let data = payload.get("data");
    let list = data
        .and_then(|d| d.get("verses"))
        .and_then(Value::as_array)
        .or_else(|| {
            data.and_then(|d| d.get("passages"))
                .and_then(Value::as_array)
        })
        .or_else(|| data.and_then(Value::as_array));
    let Some(list) = list else {
        return Vec::new();
    };
    let mut hits = Vec::new();
    for row in list {
        let id = js_string(row.get("id"))
            .or_else(|| js_string(row.get("orgId")))
            .unwrap_or_default();
        let reference = trim(&js_string(row.get("reference")).unwrap_or_default()).to_owned();
        let text = strip_html(
            &js_string(row.get("text"))
                .or_else(|| js_string(row.get("content")))
                .unwrap_or_default(),
        );
        if (reference.is_empty() && id.is_empty()) || text.is_empty() {
            continue;
        }
        let id_match = HIT_ID.captures(&id);
        let ref_match = HIT_REF.captures(&reference);
        let num = |s: &str| s.parse::<f64>().ok();
        let (chapter, verse) = match (&id_match, &ref_match) {
            (Some(c), _) => (num(&c[2]), num(&c[3])),
            (None, Some(c)) => (num(&c[2]), num(&c[3])),
            _ => (None, None),
        };
        hits.push(ApiSearchHit {
            reference: if !reference.is_empty() {
                reference
            } else if let Some(c) = &id_match {
                format!("{} {}:{}", &c[1], &c[2], &c[3])
            } else {
                id.clone()
            },
            text,
            book_id: id_match
                .as_ref()
                .map(|c| c[1].to_uppercase())
                .or_else(|| row.get("bookId").and_then(Value::as_str).map(str::to_owned)),
            chapter,
            verse,
        });
    }
    hits
}

pub use kairo_domain::js::encode_uri_component;
