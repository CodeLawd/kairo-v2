//! The lyrics service's online tier: `searchOnline`, `previewOnline` and the
//! online branch of `importSong`, from `src/main/services/lyrics/index.ts`.

use crate::import::parse_import;
use crate::library::{LyricsError, LyricsService, Result};
use crate::online::normalize::extract_lyric_lines;
use crate::online::{OnlineLyrics, OnlineSource};
use kairo_domain::lyrics::{
    LyricsImportSource, LyricsOnlinePreview, LyricsOnlineResult, LyricsProvider, LyricsSectionType,
    LyricsSong, LyricsSongSection, LyricsSource,
};
use regex::Regex;
use std::sync::{Arc, LazyLock};

static PARENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\(.*?\)").unwrap());
static NON_ALNUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]").unwrap());

/// `songMatchKey`: lowercase, parentheticals and non-alphanumerics removed.
pub fn song_match_key(title: &str, artist: &str) -> String {
    let norm = |v: &str| {
        NON_ALNUM
            .replace_all(&PARENS.replace_all(&v.to_lowercase(), ""), "")
            .into_owned()
    };
    format!("{}|{}", norm(title), norm(artist))
}

use kairo_domain::js::now_ms;

fn source_of(provider: LyricsProvider) -> LyricsSource {
    match provider {
        LyricsProvider::Genius => LyricsSource::Genius,
        LyricsProvider::Lrclib => LyricsSource::Lrclib,
        LyricsProvider::Web => LyricsSource::Web,
    }
}

/// Provider text to a song; a flat single verse when no sections parse.
fn parse_online_text(text: &str, source: &OnlineSource) -> Result<LyricsSong> {
    let mut song = parse_import(
        LyricsImportSource::Text {
            title: source.title.clone(),
            artist: source.artist.clone(),
            text: text.to_owned(),
            copyright: None,
        },
        now_ms(),
    )?;
    song.source = Some(source_of(source.provider));
    if !song.sections.is_empty() {
        return Ok(song);
    }
    let lines = extract_lyric_lines(text);
    if lines.is_empty() {
        return Err(LyricsError::Online(
            "No lyric text was found on this page.".into(),
        ));
    }
    tracing::warn!(
        title = source.title,
        lines = lines.len(),
        "no sections parsed; using a flat verse"
    );
    song.sections = vec![LyricsSongSection {
        kind: LyricsSectionType::Verse,
        label: "Verse 1".into(),
        lines,
        line_colors: None,
        hotkey: None,
        extra: Default::default(),
    }];
    Ok(song)
}

impl LyricsService {
    /// Remote catalogues, with results the library already holds flagged.
    /// The first two new results are prepared in the background.
    pub async fn search_online(
        &self,
        online: &Arc<OnlineLyrics>,
        query: &str,
        credential: &str,
    ) -> Result<Vec<LyricsOnlineResult>> {
        let results = online
            .search_all(query)
            .await
            .map_err(LyricsError::Online)?;
        if results.is_empty() {
            return Ok(results);
        }
        // Later songs overwrite earlier ones, as a Map insert would.
        let owned: std::collections::HashMap<String, String> = self
            .get_library()
            .await?
            .into_iter()
            .map(|s| (song_match_key(&s.title, &s.artist), s.id))
            .collect();
        let marked: Vec<LyricsOnlineResult> = results
            .into_iter()
            .map(|mut r| {
                r.existing_song_id = owned.get(&song_match_key(&r.title, &r.artist)).cloned();
                r
            })
            .collect();
        online.warm(&marked, credential);
        Ok(marked)
    }

    /// Fetched, reviewed and parsed lyrics for a hit, without saving.
    pub async fn preview_online(
        &self,
        online: &Arc<OnlineLyrics>,
        source: &OnlineSource,
        credential: &str,
    ) -> Result<LyricsOnlinePreview> {
        tracing::info!(
            provider = source.provider.as_str(),
            title = source.title,
            "preview online"
        );
        let reviewed = online
            .prepare(source, credential)
            .await
            .map_err(LyricsError::Online)?;
        let song = parse_online_text(&reviewed.text, source)?;
        Ok(LyricsOnlinePreview {
            review: Some(reviewed.review),
            title: song.title,
            artist: song.artist,
            provider: source.provider,
            url: source.url.clone(),
            sections: song
                .sections
                .into_iter()
                .map(|s| LyricsSongSection {
                    line_colors: None,
                    hotkey: None,
                    extra: Default::default(),
                    ..s
                })
                .collect(),
        })
    }

    /// The online branch of `importSong`: the library's copy when it already
    /// holds the song, else the same reviewed snapshot the preview showed.
    pub async fn import_online(
        &self,
        online: &Arc<OnlineLyrics>,
        source: &OnlineSource,
        credential: &str,
    ) -> Result<LyricsSong> {
        let key = song_match_key(&source.title, &source.artist);
        if let Some(existing) = self
            .get_library()
            .await?
            .into_iter()
            .find(|s| song_match_key(&s.title, &s.artist) == key)
        {
            tracing::info!(title = source.title, "online import already in library");
            return Ok(existing);
        }
        let reviewed = online
            .prepare(source, credential)
            .await
            .map_err(LyricsError::Online)?;
        let song = parse_online_text(&reviewed.text, source)?;
        self.save_import(song).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_keys_ignore_case_parentheses_and_punctuation() {
        assert_eq!(
            song_match_key("Ese (Thank You)", "Nathaniel Bassey"),
            "ese|nathanielbassey"
        );
        assert_eq!(song_match_key("Way-Maker!", "SINACH"), "waymaker|sinach");
    }
}
