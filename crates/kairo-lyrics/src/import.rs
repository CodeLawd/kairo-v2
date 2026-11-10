use std::{collections::HashMap, sync::LazyLock};

use kairo_domain::{
    js::{is_whitespace, trim},
    lyrics::{
        LyricsImportSource, LyricsSectionType as Kind, LyricsSong, LyricsSongSection, LyricsSource,
        slides::{DEFAULT_IMPORT_SLIDE_LINES, apply_import_slide_breaks, preserve_slide_breaks},
    },
};
use regex::Regex;

use crate::library::{LyricsError, new_id};

static LABELS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:(?<v>v(?:erse)?)|(?<c>c(?:horus)?)|(?<b>b(?:ridge)?)|",
        r"(?<p>pc|p\.?c\.?|pre[- ]?c(?:ho(?:rus)?)?|prechorus)|(?<t>t(?:ag)?)|",
        r"(?<i>i(?:ntro(?:duction)?)?)|(?<o>o(?:utro)?)|(?<end>ending))\.? *(?<index>[0-9]*)$|^e(?<e>[0-9]*)$"
    )).expect("valid lyrics section labels")
});
static METADATA: LazyLock<Regex> = LazyLock::new(|| {
    const JS_SPACE: &str = r"[\x09-\x0D\x20\u{00A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}\u{FEFF}]";
    Regex::new(&format!(r"^{JS_SPACE}*([A-Za-z][A-Za-z #]*){JS_SPACE}*([=:]){JS_SPACE}*([^\r\n\u{{2028}}\u{{2029}}]*)$"))
        .expect("valid lyrics metadata")
});

pub fn parse_section_label(raw: &str) -> Option<(Kind, usize)> {
    let raw = raw.strip_prefix(['[', '(', '{']).unwrap_or(raw);
    let raw = raw.strip_suffix([']', ')', '}']).unwrap_or(raw);
    let line = trim(trim(raw).trim_end_matches(':'));
    let lower = line.to_lowercase();
    let normalized = lower
        .split(is_whitespace)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let found = LABELS.captures(&normalized)?;
    for (group, kind, numbered) in [
        ("v", Kind::Verse, true),
        ("c", Kind::Chorus, true),
        ("b", Kind::Bridge, true),
        ("p", Kind::PreChorus, false),
        ("t", Kind::Tag, false),
        ("i", Kind::Intro, false),
        ("o", Kind::Outro, false),
        ("end", Kind::Ending, false),
        ("e", Kind::Ending, true),
    ] {
        if let Some(matched) = found.name(group) {
            let index = if group == "e" {
                matched.as_str()
            } else {
                &found["index"]
            };
            return Some((
                kind,
                if numbered {
                    index.parse::<usize>().unwrap_or(1).max(1)
                } else {
                    1
                },
            ));
        }
    }
    None
}

fn label(kind: Kind, index: usize) -> String {
    let name = match kind {
        Kind::Verse => "Verse",
        Kind::Chorus => "Chorus",
        Kind::Bridge => "Bridge",
        Kind::PreChorus => "Pre-Chorus",
        Kind::Tag => "Tag",
        Kind::Intro => "Intro",
        Kind::Outro => "Outro",
        Kind::Ending => "Ending",
    };
    if kind == Kind::Verse
        || (index > 1 && !matches!(kind, Kind::Intro | Kind::Outro | Kind::Ending))
    {
        format!("{name} {index}")
    } else {
        name.to_owned()
    }
}

fn section(kind: Kind, index: usize, lines: Vec<String>) -> LyricsSongSection {
    LyricsSongSection {
        kind,
        label: label(kind, index),
        lines: apply_import_slide_breaks(&lines, Some(DEFAULT_IMPORT_SLIDE_LINES)),
        line_colors: None,
        hotkey: None,
        extra: Default::default(),
    }
}

fn marked_sections(lines: &[&str]) -> Option<Vec<LyricsSongSection>> {
    let mut output = Vec::new();
    let mut buffered = Vec::new();
    let mut current = None;
    let mut counts = HashMap::new();
    let mut has_markers = false;
    for line in lines {
        let trimmed = trim(line);
        if trimmed.encode_utf16().count() <= 30
            && let Some((kind, index)) = parse_section_label(trimmed)
        {
            has_markers = true;
            flush(&mut output, &mut buffered, current, &mut counts);
            let count = counts.entry(kind).or_insert(0);
            *count += 1;
            current = Some((kind, if index > 1 { index } else { *count }));
        } else {
            buffered.push(*line);
        }
    }
    if !has_markers {
        return None;
    }
    flush(&mut output, &mut buffered, current, &mut counts);
    Some(output)
}

fn flush(
    output: &mut Vec<LyricsSongSection>,
    buffered: &mut Vec<&str>,
    current: Option<(Kind, usize)>,
    counts: &mut HashMap<Kind, usize>,
) {
    let lines = preserve_slide_breaks(buffered);
    buffered.clear();
    if !lines.is_empty() {
        let (kind, index) = current.unwrap_or_else(|| {
            let count = counts.entry(Kind::Verse).or_insert(0);
            *count += 1;
            (Kind::Verse, *count)
        });
        output.push(section(kind, index, lines));
    }
}

fn canonical_block(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| {
            let kept: String = line
                .to_lowercase()
                .chars()
                .filter(|&ch| ch.is_ascii_alphanumeric() || ch == '_' || is_whitespace(ch))
                .collect();
            kept.split(is_whitespace)
                .filter(|word| !word.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_sections(lines: &[&str]) -> Vec<LyricsSongSection> {
    if let Some(sections) = marked_sections(lines) {
        return sections;
    }
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut blank = 0;
    for line in lines {
        if trim(line).is_empty() {
            blank += 1;
            if blank >= 2 && !current.is_empty() {
                blocks.push(std::mem::take(&mut current));
                blank = 0;
            }
        } else {
            blank = 0;
            current.push(line.trim_end_matches(is_whitespace).to_owned());
        }
    }
    if !current.is_empty() {
        blocks.push(current);
    }
    let canonicals: Vec<_> = blocks.iter().map(|block| canonical_block(block)).collect();
    let mut counts = HashMap::new();
    for key in &canonicals {
        *counts.entry(key).or_insert(0usize) += 1;
    }
    let mut repeated = None;
    let mut high = 1;
    for key in &canonicals {
        if !key.is_empty() && counts[key] > high {
            high = counts[key];
            repeated = Some(key);
        }
    }
    let (mut verses, mut choruses) = (0, 0);
    blocks
        .into_iter()
        .zip(&canonicals)
        .map(|(block, key)| {
            if repeated == Some(key) {
                choruses += 1;
                section(Kind::Chorus, choruses, block)
            } else {
                verses += 1;
                section(Kind::Verse, verses, block)
            }
        })
        .collect()
}

fn ccli(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

fn song(title: &str, artist: &str, copyright: &str, source: LyricsSource, now: f64) -> LyricsSong {
    LyricsSong {
        id: new_id(now),
        title: trim(title).to_owned(),
        artist: trim(artist).to_owned(),
        copyright: (!trim(copyright).is_empty()).then(|| trim(copyright).to_owned()),
        ccli_number: None,
        is_favorite: Some(false),
        source: Some(source),
        sections: Vec::new(),
        created_at: now,
        updated_at: now,
        extra: Default::default(),
    }
}

fn usr(content: &str, filename: Option<&str>, now: f64) -> LyricsSong {
    let content = content
        .strip_prefix('\u{feff}')
        .unwrap_or(content)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut lines = content.split('\n').peekable();
    let mut meta = HashMap::new();
    while let Some(line) = lines.peek() {
        if let Some(found) = METADATA.captures(line) {
            let key: String = found[1]
                .to_ascii_lowercase()
                .chars()
                .filter(|ch| !is_whitespace(*ch))
                .collect();
            if &found[2] == ":"
                && !matches!(
                    key.as_str(),
                    "title"
                        | "songtitle"
                        | "author"
                        | "artist"
                        | "writer"
                        | "writers"
                        | "songwriter"
                        | "authors"
                        | "copyright"
                        | "ccli#"
                        | "cclinumber"
                        | "cclisongnumber"
                        | "key"
                        | "tempo"
                        | "admin"
                        | "publisher"
                        | "theme"
                        | "themes"
                )
            {
                break;
            }
            meta.insert(key, trim(found.get(3).expect("metadata value").as_str()));
        } else if !trim(line).is_empty() {
            break;
        }
        lines.next();
    }
    let title_from_file = filename.map(|name| {
        let stem = name
            .rsplit_once('.')
            .filter(|(_, ext)| !ext.is_empty())
            .map_or(name, |(stem, _)| stem);
        static UNDERSCORES: LazyLock<Regex> =
            LazyLock::new(|| Regex::new("_+").expect("valid underscore pattern"));
        UNDERSCORES.replace_all(stem, " ").into_owned()
    });
    let first = |keys: &[&str]| keys.iter().find_map(|key| meta.get(*key).copied());
    let title = first(&["title", "songtitle"])
        .or_else(|| {
            title_from_file
                .as_deref()
                .map(trim)
                .filter(|title| !title.is_empty())
        })
        .unwrap_or("Untitled");
    let artist = first(&["author", "artist", "writer", "songwriter", "authors"]).unwrap_or("");
    let mut song = song(
        title,
        artist,
        first(&["copyright"]).unwrap_or(""),
        LyricsSource::Usr,
        now,
    );
    song.ccli_number = first(&["ccli#", "cclinumber", "cclisongnumber"]).and_then(ccli);
    song.sections = parse_sections(&lines.collect::<Vec<_>>());
    song
}

pub(crate) fn parse_import(
    source: LyricsImportSource,
    now: f64,
) -> Result<LyricsSong, LyricsError> {
    match source {
        LyricsImportSource::Usr { content, filename } => {
            Ok(usr(&content, filename.as_deref(), now))
        }
        LyricsImportSource::Text {
            title,
            artist,
            text,
            copyright,
        } => {
            let title = if trim(&title).is_empty() {
                "Untitled"
            } else {
                &title
            };
            let mut song = song(
                title,
                &artist,
                copyright.as_deref().unwrap_or(""),
                LyricsSource::Text,
                now,
            );
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            song.sections = parse_sections(&text.split('\n').collect::<Vec<_>>());
            Ok(song)
        }
        LyricsImportSource::Ccli { .. } => Err(LyricsError::LicensedImport),
        LyricsImportSource::Url { .. } => Err(LyricsError::UrlImport),
        LyricsImportSource::Online { .. } => Err(LyricsError::OnlineImport),
    }
}

pub fn parse_open_lyrics(xml: &str, now: f64) -> Result<LyricsSong, LyricsError> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|error| LyricsError::OpenLyrics(error.to_string()))?;
    let root = doc.root_element();
    if !root.has_tag_name("song") {
        return Err(LyricsError::OpenLyrics("expected a song element".into()));
    }
    let properties = root.children().find(|n| n.has_tag_name("properties"));
    let property =
        |name| properties.and_then(|node| node.children().find(|n| n.has_tag_name(name)));
    let title = property("titles")
        .and_then(|n| n.children().find(|n| n.has_tag_name("title")))
        .and_then(|n| n.text())
        .map(trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Untitled");
    let authors = property("authors")
        .into_iter()
        .flat_map(|n| n.children())
        .filter(|n| n.has_tag_name("author"))
        .filter_map(|n| n.text())
        .map(trim)
        .collect::<Vec<_>>()
        .join(", ");
    let mut song = song(
        title,
        &authors,
        property("copyright").and_then(|n| n.text()).unwrap_or(""),
        LyricsSource::Text,
        now,
    );
    song.ccli_number = property("ccliNo").and_then(|n| n.text()).and_then(ccli);
    let lyrics = root
        .children()
        .find(|n| n.has_tag_name("lyrics"))
        .ok_or_else(|| LyricsError::OpenLyrics("missing lyrics element".into()))?;
    let mut verses = Vec::new();
    for verse in lyrics.children().filter(|n| n.has_tag_name("verse")) {
        let name = verse.attribute("name").unwrap_or("");
        let (kind, index) = parse_section_label(name)
            .or_else(|| {
                let digits = name.get(1..)?.parse().ok()?;
                let kind = match name.as_bytes()[0] {
                    b'p' => Kind::PreChorus,
                    _ => return None,
                };
                Some((kind, digits))
            })
            .unwrap_or((Kind::Verse, verses.len() + 1));
        let mut lines = Vec::new();
        for group in verse.children().filter(|n| n.has_tag_name("lines")) {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            let mut line = String::new();
            for node in group.descendants().skip(1) {
                if node.has_tag_name("br") {
                    lines.push(trim(&line).to_owned());
                    line.clear();
                } else if node.is_text() {
                    line.push_str(node.text().unwrap_or(""));
                }
            }
            lines.push(trim(&line).to_owned());
        }
        let lines = preserve_slide_breaks(&lines);
        if !lines.is_empty() {
            verses.push((name, section(kind, index, lines)));
        }
    }
    if let Some(order) = property("verseOrder").and_then(|n| n.text()) {
        song.sections = order
            .split_whitespace()
            .filter_map(|name| {
                verses
                    .iter()
                    .find(|(key, _)| *key == name)
                    .map(|(_, verse)| verse.clone())
            })
            .collect();
    }
    if song.sections.is_empty() {
        song.sections = verses.into_iter().map(|(_, section)| section).collect();
    }
    Ok(song)
}
