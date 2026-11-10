//! Typed references, from `reference.ts` and `reference-search.ts`.

use kairo_domain::js::{parse_int, trim};
use kairo_domain::scripture::query::normalize_scripture_query;
use regex::Regex;
use std::future::Future;
use std::sync::LazyLock;

/// JavaScript's `\s`.
const SPACE: &str = r"[\t\n\x0B\x0C\r \u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
/// JavaScript's `.`, which stops at line terminators.
const ANY: &str = r"[^\n\r\u{2028}\u{2029}]";

static CHAPTER_ONLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^({ANY}+?){SPACE}+([0-9]+)$")).unwrap());
static VERSES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^({ANY}+?){SPACE}+([0-9]+):([0-9]+)(?:{SPACE}*[-–—]{SPACE}*([0-9]+))?$"
    ))
    .unwrap()
});

/// A parsed reference. Numbers are JavaScript numbers: `parseInt` of any
/// length of digits, so a huge chapter simply finds nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedScriptureReference {
    pub book: String,
    pub chapter: f64,
    pub verse_start: Option<f64>,
    pub verse_end: Option<f64>,
    /// True for chapter-only queries ("Psalms 23"): load the whole chapter.
    pub is_chapter: bool,
}

/// `parseScriptureReference`. Chapter-only queries count only when the book
/// is known, so a remembered phrase like "love 123" stays a phrase search.
pub fn parse_scripture_reference(input: &str) -> Option<ParsedScriptureReference> {
    if let Some(normalized) = normalize_scripture_query(input) {
        if let Some(caps) = CHAPTER_ONLY.captures(&normalized)
            && !caps[0].contains(':')
        {
            let chapter = parse_int(&caps[2], Some(10.0));
            if chapter.fract() != 0.0 || chapter < 1.0 {
                return None;
            }
            return Some(ParsedScriptureReference {
                book: trim(&caps[1]).to_owned(),
                chapter,
                verse_start: None,
                verse_end: None,
                is_chapter: true,
            });
        }
        return verse_reference(&normalized);
    }
    // Already-canonical verse references fall back to the raw input.
    verse_reference(trim(input))
}

fn verse_reference(text: &str) -> Option<ParsedScriptureReference> {
    let caps = VERSES.captures(text)?;
    let number = |i: usize| caps.get(i).map(|m| parse_int(m.as_str(), Some(10.0)));
    Some(ParsedScriptureReference {
        book: trim(&caps[1]).to_owned(),
        chapter: number(2)?,
        verse_start: number(3),
        verse_end: number(4),
        is_chapter: false,
    })
}

/// `resolveReferenceLookup`: resolve only the translation the operator
/// selected and never substitute wording. A local copy answers when one is
/// installed; otherwise API.Bible does, when it is available.
pub async fn resolve_reference_lookup<V, R, E, LF, AF, TF>(
    requested: &str,
    local_ids: &[String],
    lookup_local: impl FnOnce(String) -> LF,
    lookup_api: Option<impl FnOnce() -> AF>,
    to_local_result: impl FnOnce(Vec<V>, String) -> TF,
) -> Result<Vec<R>, E>
where
    LF: Future<Output = Result<Vec<V>, E>>,
    AF: Future<Output = Result<Vec<R>, E>>,
    TF: Future<Output = Result<R, E>>,
{
    let upper = requested.to_uppercase();
    if local_ids.iter().any(|id| id.to_uppercase() == upper) {
        let local = lookup_local(requested.to_owned()).await?;
        if local.is_empty() {
            return Ok(Vec::new());
        }
        return Ok(vec![to_local_result(local, requested.to_owned()).await?]);
    }
    match lookup_api {
        Some(lookup) => lookup().await,
        None => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use std::cell::RefCell;

    fn verses(book: &str, chapter: f64, start: f64, end: Option<f64>) -> ParsedScriptureReference {
        ParsedScriptureReference {
            book: book.into(),
            chapter,
            verse_start: Some(start),
            verse_end: end,
            is_chapter: false,
        }
    }

    // reference.test.ts
    #[test]
    fn parses_ranges_using_hyphen_en_dash_or_em_dash() {
        assert_eq!(
            parse_scripture_reference("Genesis 21:1–2"),
            Some(verses("Genesis", 21.0, 1.0, Some(2.0)))
        );
        assert_eq!(
            parse_scripture_reference("Proverbs 3:5-6")
                .unwrap()
                .verse_end,
            Some(6.0)
        );
        assert_eq!(
            parse_scripture_reference("Romans 8:28—30")
                .unwrap()
                .verse_end,
            Some(30.0)
        );
    }

    #[test]
    fn parses_relaxed_space_separated_shorthand() {
        assert_eq!(
            parse_scripture_reference("jos 1 5 9"),
            Some(verses("Joshua", 1.0, 5.0, Some(9.0)))
        );
    }

    #[test]
    fn parses_incomplete_book_names_and_exact_short_abbreviations() {
        assert_eq!(
            parse_scripture_reference("joshu 1:9"),
            Some(verses("Joshua", 1.0, 9.0, None))
        );
        assert_eq!(
            parse_scripture_reference("ez 1"),
            Some(ParsedScriptureReference {
                book: "Ezra".into(),
                chapter: 1.0,
                verse_start: None,
                verse_end: None,
                is_chapter: true,
            })
        );
    }

    #[test]
    fn unknown_books_fall_back_to_the_raw_verse_form_only() {
        assert_eq!(
            parse_scripture_reference("  Foo 3:16 "),
            Some(verses("Foo", 3.0, 16.0, None))
        );
        assert_eq!(parse_scripture_reference("love 123"), None);
        assert_eq!(parse_scripture_reference(""), None);
    }

    type Lookup = Result<Vec<&'static str>, String>;

    fn run(requested: &str, api: Option<Lookup>) -> (Lookup, Vec<String>) {
        let calls = RefCell::new(Vec::new());
        let local_ids = ["KJV".to_string(), "WEB".to_string()];
        let result = resolve_reference_lookup(
            requested,
            &local_ids,
            |translation: String| {
                calls.borrow_mut().push(format!("local:{translation}"));
                async move {
                    Ok::<_, String>(if translation == "KJV" {
                        vec!["KJV"]
                    } else {
                        vec![]
                    })
                }
            },
            api.map(|api| {
                || {
                    calls.borrow_mut().push("api".into());
                    async move { api }
                }
            }),
            |verses: Vec<&'static str>, _translation| async move { Ok(verses[0]) },
        )
        .now_or_never()
        .unwrap();
        (result, calls.into_inner())
    }

    // reference-search.test.ts
    #[test]
    fn an_api_backed_default_bypasses_local_bibles() {
        let (result, calls) = run("NIV", Some(Ok(vec!["NIV"])));
        assert_eq!(calls, ["api"]);
        assert_eq!(result.unwrap(), ["NIV"]);
    }

    #[test]
    fn uses_the_local_copy_when_the_requested_translation_is_seeded() {
        let (result, calls) = run("KJV", Some(Ok(vec!["NIV"])));
        assert_eq!(calls, ["local:KJV"]);
        assert_eq!(result.unwrap(), ["KJV"]);
    }

    #[test]
    fn does_not_substitute_another_local_bible_when_the_api_fails() {
        let error = "NIV is not authorized for this API.Bible key.".to_string();
        let (result, calls) = run("NIV", Some(Err(error.clone())));
        assert_eq!(calls, ["api"]);
        assert_eq!(result.unwrap_err(), error);
        // Without API.Bible there is nothing to ask.
        let (result, calls) = run("NIV", None);
        assert!(calls.is_empty());
        assert!(result.unwrap().is_empty());
    }
}
