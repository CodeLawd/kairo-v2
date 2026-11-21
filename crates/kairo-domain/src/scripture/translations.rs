//! The Bible translation registry, from `src/lib/bible-translations.ts`.
//!
//! The single source of truth for every translation Kairo knows. Everything
//! else (catalogue, API.Bible id mapping, sermon reference detection, the
//! fallback order, the pack list) derives from [`BIBLE_TRANSLATIONS`].

use super::TranslationAccess;
use super::js;
use regex::Regex;
use std::collections::BTreeMap;

/// A one-click offline pack for Settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownloadableBiblePack {
    /// HTTPS URL of the `*.db.gz` asset.
    pub url: &'static str,
    /// Hex SHA-256 of the compressed asset, verified before install.
    pub sha256: &'static str,
    /// Human label for Settings, such as `about 5 MB`.
    pub approx_label: &'static str,
}

/// One registry entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BibleTranslationDefinition {
    /// Normalised id such as `NKJV`. Compared case-insensitively everywhere.
    pub id: &'static str,
    pub name: &'static str,
    pub access: TranslationAccess,
    /// Ships in `resources/bible.db`; its pack can never be removed.
    pub bundled: bool,
    /// Alternate abbreviations that resolve to this id.
    pub aliases: &'static [&'static str],
    /// Regex sources (case-insensitive) matched against API.Bible names.
    pub name_patterns: &'static [&'static str],
    /// Official English text id, for editions with unhelpful abbreviations.
    pub preferred_bible_id: Option<&'static str>,
    pub downloadable_pack: Option<DownloadableBiblePack>,
}

const fn entry(
    id: &'static str,
    name: &'static str,
    access: TranslationAccess,
) -> BibleTranslationDefinition {
    BibleTranslationDefinition {
        id,
        name,
        access,
        bundled: false,
        aliases: &[],
        name_patterns: &[],
        preferred_bible_id: None,
        downloadable_pack: None,
    }
}

const fn local_bundled(
    id: &'static str,
    name: &'static str,
    pattern: &'static [&'static str],
) -> BibleTranslationDefinition {
    BibleTranslationDefinition {
        bundled: true,
        name_patterns: pattern,
        ..entry(id, name, TranslationAccess::Local)
    }
}

const fn api(
    id: &'static str,
    name: &'static str,
    pattern: &'static [&'static str],
) -> BibleTranslationDefinition {
    BibleTranslationDefinition {
        name_patterns: pattern,
        ..entry(id, name, TranslationAccess::Api)
    }
}

/// Add a new translation here: one entry, no other code changes.
pub const BIBLE_TRANSLATIONS: &[BibleTranslationDefinition] = &[
    BibleTranslationDefinition {
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/nkjv-pack.db.gz",
            sha256: "12efa45e3be35978ffc72ee2305f4f085ca7076ef5974b7046dfcb8ac450b041",
            approx_label: "about 5 MB",
        }),
        ..api("NKJV", "New King James Version", &["new king james"])
    },
    local_bundled("KJV", "King James Version", &["king james"]),
    local_bundled("BBE", "Bible in Basic English", &["basic english"]),
    local_bundled("BSB", "Berean Standard Bible", &["berean standard"]),
    api("WEB", "World English Bible", &["world english"]),
    api("ASV", "American Standard Version", &["american standard"]),
    api("OEB", "Open English Bible", &["open english"]),
    BibleTranslationDefinition {
        aliases: &["NIVUK", "NIV11", "NIV2011", "NIV84"],
        preferred_bible_id: Some("78a9f6124f344018-01"),
        ..api("NIV", "New International Version", &["new international"])
    },
    BibleTranslationDefinition {
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/nlt-pack.db.gz",
            sha256: "ea57d70425083de77ad1aca16193bb2f8bd6629ec640200c88937aefd6f45528",
            approx_label: "about 5 MB",
        }),
        ..api("NLT", "New Living Translation", &["new living"])
    },
    BibleTranslationDefinition {
        aliases: &["NASB95", "NASB1995", "NASB20", "NASB2020"],
        preferred_bible_id: Some("a761ca71e0b3ddcf-01"),
        ..api(
            "NASB",
            "New American Standard Bible",
            &["new american standard"],
        )
    },
    BibleTranslationDefinition {
        aliases: &["MESSAGE", "THEMESSAGE"],
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/msg-pack.db.gz",
            sha256: "0a3ddc7dd55d04b347b07de41540cb51fdeeb85c415d8ca6cf45b0760960ac8f",
            approx_label: "about 6 MB",
        }),
        ..api("MSG", "The Message", &[r"\bthe message\b"])
    },
    BibleTranslationDefinition {
        aliases: &["AMP", "AMPLIFIED", "AMPLIFIEDCLASSIC", "AMPLIFIED CLASSIC"],
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/ampc-pack.db.gz",
            sha256: "d4a6d9a3faef85daea7f3b26c8f99dacb6e8ebdc4f859c10791d87e65c242f51",
            approx_label: "about 6 MB",
        }),
        ..api(
            "AMPC",
            "Amplified Bible, Classic Edition",
            &[r"amplified bible,?\s*classic", r"\bamplified\b"],
        )
    },
    BibleTranslationDefinition {
        aliases: &[
            "PASSION",
            "THEPASSIONTRANSLATION",
            "THE PASSION TRANSLATION",
        ],
        // Partial: 15 Old Testament books are not yet translated.
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/tpt-pack.db.gz",
            sha256: "13822ccd801dce33584cfec25d9234eaed5e777ed820665a60a8c81242e8c614",
            approx_label: "about 4 MB",
        }),
        ..api("TPT", "The Passion Translation", &["passion translation"])
    },
    api("ESV", "English Standard Version", &["english standard"]),
    BibleTranslationDefinition {
        preferred_bible_id: Some("a556c5305ee15c3f-01"),
        ..api("CSB", "Christian Standard Bible", &["christian standard"])
    },
    BibleTranslationDefinition {
        aliases: &["GNB", "TEV"],
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/gnt-pack.db.gz",
            sha256: "5df80c33b57ac552bb2ff69d2f1cc94e387fc8d0fc008c5fb0b67d92f344a909",
            approx_label: "about 5 MB",
        }),
        ..api("GNT", "Good News Translation", &["good news"])
    },
    BibleTranslationDefinition {
        downloadable_pack: Some(DownloadableBiblePack {
            url: "https://github.com/CodeLawd/kairo-bible-packs/releases/download/bible-packs-v1/easy-pack.db.gz",
            sha256: "6c3c11097104035a64003ed26d36dcec42c28f61b9ae036234684fd0b3da6184",
            approx_label: "about 6 MB",
        }),
        ..api("EASY", "EasyEnglish Bible", &[r"easy\s*english"])
    },
];

/// KJV ships with Kairo, so a new install can show scripture with no download or key.
pub use crate::settings::DEFAULT_TRANSLATION_ID;

/// What pack tooling targets when none is named. NKJV was the original pack.
pub const DEFAULT_PACK_TRANSLATION_ID: &str = "NKJV";

/// Ids shipped inside `resources/bible.db`, in registry order.
pub fn bundled_translation_ids() -> Vec<&'static str> {
    BIBLE_TRANSLATIONS
        .iter()
        .filter(|entry| entry.bundled)
        .map(|entry| entry.id)
        .collect()
}

/// Case-insensitive lookup (JavaScript `toUpperCase`).
pub fn get_translation_definition(id: &str) -> Option<&'static BibleTranslationDefinition> {
    let upper = id.to_uppercase();
    BIBLE_TRANSLATIONS
        .iter()
        .find(|entry| entry.id.to_uppercase() == upper)
}

pub fn is_bundled_translation(id: &str) -> bool {
    get_translation_definition(id).is_some_and(|entry| entry.bundled)
}

pub fn get_downloadable_pack(id: &str) -> Option<DownloadableBiblePack> {
    get_translation_definition(id).and_then(|entry| entry.downloadable_pack)
}

/// Ids with a one-click offline pack (the Settings pack list).
pub fn get_downloadable_translation_ids() -> Vec<&'static str> {
    BIBLE_TRANSLATIONS
        .iter()
        .filter(|entry| entry.downloadable_pack.is_some())
        .map(|entry| entry.id)
        .collect()
}

/// Catalogue rows in registry order: `(id, name, access)`.
pub fn translation_catalog_entries() -> Vec<(&'static str, &'static str, TranslationAccess)> {
    BIBLE_TRANSLATIONS
        .iter()
        .map(|entry| (entry.id, entry.name, entry.access))
        .collect()
}

fn normalize_abbreviation(value: &str) -> String {
    value
        .to_uppercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Normalised alias to translation id, for API.Bible abbreviation matching.
pub fn build_abbreviation_aliases() -> BTreeMap<String, &'static str> {
    let mut aliases = BTreeMap::new();
    for entry in BIBLE_TRANSLATIONS {
        for alias in entry.aliases {
            aliases.insert(normalize_abbreviation(alias), entry.id);
        }
    }
    aliases
}

/// API.Bible bible id to translation id.
pub fn build_preferred_bible_ids() -> BTreeMap<&'static str, &'static str> {
    BIBLE_TRANSLATIONS
        .iter()
        .filter_map(|entry| entry.preferred_bible_id.map(|bible| (bible, entry.id)))
        .collect()
}

/// A compiled API.Bible name pattern.
#[derive(Clone, Debug)]
pub struct NamePattern {
    /// The JavaScript regex source as written in the registry.
    pub source: &'static str,
    pub id: &'static str,
    regex: Regex,
}

impl NamePattern {
    /// Case-insensitive test, as `new RegExp(source, 'i').test(name)`.
    pub fn is_match(&self, name: &str) -> bool {
        // JavaScript's non-Unicode `i` flag folds ASCII letters only; every
        // registry source is lower case, so lower-casing ASCII in the
        // haystack is equivalent and keeps offsets.
        self.regex.is_match(&name.to_ascii_lowercase())
    }
}

/// Translates the JavaScript regex features the registry and spoken patterns
/// use (`\s`, `\b`) into `regex` crate syntax with identical meaning.
pub(crate) fn js_regex(source: &str) -> Regex {
    let translated = source
        .replace(r"\s", &format!("[{}]", js::SPACE_CLASS))
        .replace(r"\b", r"(?-u:\b)");
    Regex::new(&translated).unwrap_or_else(|e| panic!("bad built-in pattern {source}: {e}"))
}

/// `[pattern, id]` pairs for API.Bible name matching, longest source first
/// (so "New American Standard Bible" is NASB, not ASV); registry order breaks ties.
pub fn build_name_patterns() -> Vec<NamePattern> {
    let mut patterns: Vec<NamePattern> = BIBLE_TRANSLATIONS
        .iter()
        .flat_map(|entry| {
            entry.name_patterns.iter().map(|source| NamePattern {
                source,
                id: entry.id,
                regex: js_regex(source),
            })
        })
        .collect();
    // Stable, like Array.prototype.sort; lengths are UTF-16 (ASCII here).
    patterns.sort_by_key(|pattern| std::cmp::Reverse(js::utf16_len(pattern.source)));
    patterns
}

/// Alias to translation id for sermon reference detection, including each id.
/// Keys are upper-cased with interior whitespace collapsed to one space.
pub fn build_translation_alias_map() -> BTreeMap<String, &'static str> {
    let mut map = BTreeMap::new();
    for entry in BIBLE_TRANSLATIONS {
        map.insert(entry.id.to_uppercase(), entry.id);
        for alias in entry.aliases {
            map.insert(js::collapse_spaces(&alias.to_uppercase()), entry.id);
        }
    }
    map
}

/// Local-first fallback order: bundled translations in registry order.
pub fn bundled_translation_ids_in_order() -> Vec<&'static str> {
    bundled_translation_ids()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longer_name_patterns_win() {
        let patterns = build_name_patterns();
        let first = |name: &str| patterns.iter().find(|p| p.is_match(name)).map(|p| p.id);
        assert_eq!(first("New American Standard Bible"), Some("NASB"));
        assert_eq!(first("New King James Version"), Some("NKJV"));
        assert_eq!(first("Amplified Bible, Classic Edition"), Some("AMPC"));
        assert_eq!(first("THE MESSAGE"), Some("MSG"));
        assert_eq!(first("Something else"), None);
    }

    #[test]
    fn bundled_ids_and_defaults() {
        assert_eq!(bundled_translation_ids(), ["KJV", "BBE", "BSB"]);
        assert!(is_bundled_translation("kjv"));
        assert!(!is_bundled_translation("NKJV"));
        assert_eq!(
            get_downloadable_translation_ids(),
            ["NKJV", "NLT", "MSG", "AMPC", "TPT", "GNT", "EASY"]
        );
        assert!(get_translation_definition("nope").is_none());
    }
}
