//! The `.kairo` review and export rules (`ImportReviewModal.tsx`,
//! `ExportSongsModal.tsx`, `useTransfer.ts`), free of the UI.

use std::collections::{BTreeMap, BTreeSet};

use kairo_domain::kairo_bundle::{
    KairoBundleKind, PlaylistImportChoice, SongImportChoice, TransferCommitRequest,
    TransferCommitResult, TransferExportRequest, TransferPreview,
};

/// The result line under the window (`TransferNotice`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// `true` for `ok`, `false` for `error`.
    pub ok: bool,
    pub text: String,
}

impl Notice {
    pub fn ok(text: impl Into<String>) -> Self {
        Self {
            ok: true,
            text: text.into(),
        }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            ok: false,
            text: text.into(),
        }
    }
}

/// `describeCommit`.
pub fn describe_commit(result: &TransferCommitResult) -> String {
    let mut parts = Vec::new();
    if result.added > 0 {
        parts.push(format!(
            "{} song{} added",
            result.added,
            if result.added == 1 { "" } else { "s" }
        ));
    }
    if result.replaced > 0 {
        parts.push(format!("{} replaced", result.replaced));
    }
    if let Some(name) = &result.setlist_name {
        parts.push(format!("playlist “{name}” created"));
    }
    if let Some(title) = &result.playlist_title {
        parts.push(format!("playlist “{title}” added"));
    }
    if parts.is_empty() {
        return "Nothing was imported.".into();
    }
    let text = parts.join(", ");
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), chars.as_str()),
        None => text,
    }
}

/// The notice after an export (`exportKairo`).
pub fn export_notice(file_name: &str) -> Notice {
    Notice::ok(format!("Exported {file_name}"))
}

/// The notice for a file that could not be opened (`onOpened` error).
pub fn open_error_notice(file_name: &str, error: &str) -> Notice {
    Notice::error(format!("{file_name}: {error}"))
}

/// A choice for a song the library already holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateChoice {
    Skip,
    Replace,
    KeepBoth,
}

impl DuplicateChoice {
    /// `DUPLICATE_CHOICES`: key, label, hint.
    pub const ALL: [(DuplicateChoice, &'static str, &'static str, &'static str); 3] = [
        (
            DuplicateChoice::Skip,
            "skip",
            "Keep mine",
            "Leave your copy as it is",
        ),
        (
            DuplicateChoice::Replace,
            "replace",
            "Replace",
            "Overwrite your copy with the imported one",
        ),
        (
            DuplicateChoice::KeepBoth,
            "keep-both",
            "Keep both",
            "Add the imported one as a separate song",
        ),
    ];

    pub fn key(self) -> &'static str {
        match self {
            DuplicateChoice::Skip => "skip",
            DuplicateChoice::Replace => "replace",
            DuplicateChoice::KeepBoth => "keep-both",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().find(|c| c.1 == key).map(|c| c.0)
    }

    pub fn choice(self) -> SongImportChoice {
        match self {
            DuplicateChoice::Skip => SongImportChoice::Skip,
            DuplicateChoice::Replace => SongImportChoice::Replace,
            DuplicateChoice::KeepBoth => SongImportChoice::KeepBoth,
        }
    }

    pub fn of(choice: SongImportChoice) -> Self {
        match choice {
            SongImportChoice::Replace => DuplicateChoice::Replace,
            SongImportChoice::KeepBoth => DuplicateChoice::KeepBoth,
            _ => DuplicateChoice::Skip,
        }
    }
}

/// `PlaylistImportChoice` options for an existing playlist.
pub const PLAYLIST_CHOICES: [(PlaylistImportChoice, &str, &str); 3] = [
    (PlaylistImportChoice::KeepBoth, "keep-both", "Keep both"),
    (PlaylistImportChoice::Replace, "replace", "Replace mine"),
    (PlaylistImportChoice::Skip, "skip", "Don’t import"),
];

/// The review step's state.
#[derive(Clone, Debug, PartialEq)]
pub struct ImportReview {
    pub preview: TransferPreview,
    pub bulk: DuplicateChoice,
    pub choices: BTreeMap<String, SongImportChoice>,
    pub create_setlist: bool,
    pub playlist: PlaylistImportChoice,
    pub busy: bool,
    pub error: Option<String>,
}

impl ImportReview {
    /// New songs ticked; duplicates "Keep mine", so importing twice is harmless.
    pub fn new(preview: TransferPreview) -> Self {
        let choices = preview
            .songs
            .iter()
            .map(|s| {
                (
                    s.id.clone(),
                    if s.duplicate.is_some() {
                        SongImportChoice::Skip
                    } else {
                        SongImportChoice::Add
                    },
                )
            })
            .collect();
        let playlist = if preview.playlist.as_ref().is_some_and(|p| p.exists) {
            PlaylistImportChoice::KeepBoth
        } else {
            PlaylistImportChoice::Replace
        };
        Self {
            create_setlist: preview.setlist.is_some(),
            preview,
            bulk: DuplicateChoice::Skip,
            choices,
            playlist,
            busy: false,
            error: None,
        }
    }

    pub fn is_playlist(&self) -> bool {
        self.preview.kind == KairoBundleKind::ScripturePlaylist
    }

    pub fn duplicates(&self) -> usize {
        self.preview
            .songs
            .iter()
            .filter(|s| s.duplicate.is_some())
            .count()
    }

    pub fn new_songs(&self) -> usize {
        self.preview.songs.len() - self.duplicates()
    }

    pub fn apply_bulk(&mut self, choice: DuplicateChoice) {
        self.bulk = choice;
        for song in self.preview.songs.iter().filter(|s| s.duplicate.is_some()) {
            self.choices.insert(song.id.clone(), choice.choice());
        }
    }

    pub fn set_choice(&mut self, id: &str, choice: SongImportChoice) {
        self.choices.insert(id.to_owned(), choice);
    }

    pub fn choice(&self, id: &str) -> SongImportChoice {
        self.choices
            .get(id)
            .copied()
            .unwrap_or(SongImportChoice::Skip)
    }

    /// Songs added (new or kept both) and replaced.
    pub fn counts(&self) -> (usize, usize) {
        let mut add = 0;
        let mut replace = 0;
        for song in &self.preview.songs {
            match self.choice(&song.id) {
                SongImportChoice::Add | SongImportChoice::KeepBoth => add += 1,
                SongImportChoice::Replace => replace += 1,
                SongImportChoice::Skip => {}
            }
        }
        (add, replace)
    }

    pub fn nothing_to_do(&self) -> bool {
        if self.is_playlist() {
            return self.playlist == PlaylistImportChoice::Skip;
        }
        let (add, replace) = self.counts();
        add + replace == 0 && !(self.create_setlist && self.preview.setlist.is_some())
    }

    pub fn import_label(&self) -> String {
        if self.busy {
            return "Importing…".into();
        }
        if self.is_playlist() {
            return "Import playlist".into();
        }
        let (add, replace) = self.counts();
        let songs = add + replace;
        if songs > 0 {
            format!("Import {songs} song{}", if songs == 1 { "" } else { "s" })
        } else if self.create_setlist && self.preview.setlist.is_some() {
            "Create playlist".into()
        } else {
            "Nothing to import".into()
        }
    }

    /// Every song is already in the library.
    pub fn all_already_here(&self) -> bool {
        !self.is_playlist() && self.new_songs() == 0 && self.duplicates() > 0
    }

    /// `N songs · M new · K already in your library`.
    pub fn summary(&self) -> String {
        let total = self.preview.songs.len();
        let mut line = format!(
            "{total} song{} · {} new",
            if total == 1 { "" } else { "s" },
            self.new_songs()
        );
        let duplicates = self.duplicates();
        if duplicates > 0 {
            line.push_str(&format!(" · {duplicates} already in your library"));
        }
        line
    }

    pub fn already_here_text(&self) -> String {
        let duplicates = self.duplicates();
        let lead = if duplicates == 1 {
            "This song is".to_owned()
        } else {
            format!("All {duplicates} songs are")
        };
        format!(
            "{lead} already in your library, so there’s nothing new to add. Choose Replace to overwrite your copies with these, or Keep both to add them again."
        )
    }

    pub fn request(&self) -> TransferCommitRequest {
        TransferCommitRequest {
            token: self.preview.token.clone(),
            songs: self.choices.clone(),
            create_setlist: self.create_setlist,
            playlist: self.preview.playlist.as_ref().map(|_| self.playlist),
        }
    }
}

/// `exportedLabel`: the file name and the export date, as `15 Nov 2026`.
pub fn exported_label(preview: &TransferPreview) -> String {
    if preview.exported_at <= 0.0 || !preview.exported_at.is_finite() {
        return preview.file_name.clone();
    }
    let Some(date) = chrono::DateTime::from_timestamp_millis(preview.exported_at as i64) else {
        return preview.file_name.clone();
    };
    let local = date.with_timezone(&chrono::Local);
    format!(
        "{} · exported {}",
        preview.file_name,
        local.format("%-d %b %Y")
    )
}

/// A song in the export picker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongRow {
    pub id: String,
    pub title: String,
    pub artist: String,
}

/// The song export picker's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportPicker {
    /// Sorted by title.
    pub songs: Vec<SongRow>,
    pub selected: BTreeSet<String>,
    pub query: String,
    pub busy: bool,
}

impl ExportPicker {
    pub fn new(mut songs: Vec<SongRow>, preselect: &[String]) -> Self {
        songs.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then_with(|| a.title.cmp(&b.title))
        });
        Self {
            songs,
            selected: preselect.iter().cloned().collect(),
            query: String::new(),
            busy: false,
        }
    }

    /// Replaces the library, keeping ticks on songs still in it.
    pub fn set_songs(&mut self, songs: Vec<SongRow>) {
        let selected = std::mem::take(&mut self.selected);
        let ids: Vec<String> = selected.into_iter().collect();
        let mut next = Self::new(songs, &ids);
        next.query = std::mem::take(&mut self.query);
        next.busy = self.busy;
        *self = next;
    }

    pub fn visible(&self) -> Vec<&SongRow> {
        let q = self.query.trim().to_lowercase();
        self.songs
            .iter()
            .filter(|s| {
                q.is_empty()
                    || format!("{} {}", s.title, s.artist)
                        .to_lowercase()
                        .contains(&q)
            })
            .collect()
    }

    pub fn all_visible_selected(&self) -> bool {
        let visible = self.visible();
        !visible.is_empty() && visible.iter().all(|s| self.selected.contains(&s.id))
    }

    pub fn toggle_all_visible(&mut self) {
        let all = self.all_visible_selected();
        let ids: Vec<String> = self.visible().iter().map(|s| s.id.clone()).collect();
        for id in ids {
            if all {
                self.selected.remove(&id);
            } else {
                self.selected.insert(id);
            }
        }
    }

    pub fn toggle(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }

    /// Selected songs still in the library.
    pub fn count(&self) -> usize {
        self.songs
            .iter()
            .filter(|s| self.selected.contains(&s.id))
            .count()
    }

    pub fn everything(&self) -> bool {
        !self.songs.is_empty() && self.count() == self.songs.len()
    }

    pub fn select_label(&self) -> &'static str {
        if self.all_visible_selected() {
            "Clear"
        } else if !self.query.is_empty() {
            "Select matches"
        } else {
            "Select all"
        }
    }

    pub fn empty_text(&self) -> &'static str {
        if self.songs.is_empty() {
            "Your song library is empty."
        } else {
            "No songs match."
        }
    }

    pub fn name_placeholder(&self) -> String {
        let count = self.count();
        if self.everything() {
            "All songs".into()
        } else if count == 1 {
            "File name (optional)".into()
        } else {
            format!("{count} songs")
        }
    }

    pub fn export_label(&self) -> String {
        let count = self.count();
        format!(
            "Export {}song{}…",
            if count > 0 {
                format!("{count} ")
            } else {
                String::new()
            },
            if count == 1 { "" } else { "s" }
        )
    }

    /// The whole library when everything is ticked and no name is given.
    pub fn request(&self, name: &str) -> TransferExportRequest {
        let name = name.trim();
        if self.everything() && name.is_empty() {
            return TransferExportRequest::Library;
        }
        TransferExportRequest::Songs {
            song_ids: self
                .songs
                .iter()
                .filter(|s| self.selected.contains(&s.id))
                .map(|s| s.id.clone())
                .collect(),
            name: (!name.is_empty()).then(|| name.to_owned()),
        }
    }
}

/// The export name's longest length (`maxLength={80}`).
pub const EXPORT_NAME_MAX: usize = 80;
