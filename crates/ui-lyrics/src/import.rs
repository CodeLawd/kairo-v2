//! Import Song (`ImportModal` in `Lyrics.tsx`): search online with a
//! preview, upload song files (`.usr`, `.txt`, `.rtf`) into a queue that is
//! read, checked for duplicates and corrected before anything is saved, or
//! paste a song from the clipboard.

use gpui_kit::FocusHandle;
use gpui_kit::component::FocusTrapElement;
use kairo_ui_kit::ControlSize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::base::TestSupportExt;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::{Disableable, Icon, Sizable, h_flex, v_flex};
use gpui_kit::{
    AnyElement, AppContext, Context, Entity, EventEmitter, ExternalPaths, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, Role, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, div, prelude::FluentBuilder,
    px, uniform_list,
};
use kairo_domain::lyrics::clipboard::{
    has_song_metadata, lift_heading_title, looks_like_lyrics, parse_clipboard_song,
};
use kairo_domain::lyrics::duplicate::{DuplicateIndex, describe_duplicate};
use kairo_domain::lyrics::online_error::format_online_lyrics_error;
use kairo_domain::lyrics::propresenter::parse_propresenter;
use kairo_domain::lyrics::rtf::{looks_like_rtf, rtf_to_plain_text};
use kairo_domain::lyrics::section_edit::parse_marked_sections;
use kairo_domain::lyrics::{
    DuplicateMatch, LyricsImportSource, LyricsOnlinePreview, LyricsOnlineResult, LyricsSong,
    LyricsSongSection,
};
use kairo_ui_kit::tokens::{TextStep, radius, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

use crate::backend::{LyricsAnswer, LyricsBackend, LyricsCall, OnlineSource, SongFile};
use crate::model::{Lyrics, ask, clean_error};
use crate::quick_open::section_block;
use crate::rows;
use crate::view::review_notice_element;

/// Element ids, for tests.
pub mod ids {
    pub const DIALOG: &str = "lyrics-import";
    pub const TAB_ONLINE: &str = "lyrics-import-tab-online";
    pub const TAB_FILE: &str = "lyrics-import-tab-file";
    pub const TAB_PASTE: &str = "lyrics-import-tab-paste";
    pub const CLOSE: &str = "lyrics-import-close";
    pub const DROP: &str = "lyrics-import-drop";
    pub const CONFIRM: &str = "lyrics-import-confirm";
    pub const ERROR: &str = "lyrics-import-error";
    pub const CLIPBOARD: &str = "lyrics-import-clipboard";
    pub const NOTE: &str = "lyrics-import-note";
    pub const CLEAR_ALL: &str = "lyrics-import-clear";
    pub const ONLINE_EMPTY: &str = "lyrics-import-online-empty";
    pub fn queued(index: usize) -> String {
        format!("lyrics-import-file-{index}")
    }
    pub fn action(choice: &str) -> String {
        format!("lyrics-import-action-{choice}")
    }
    pub fn result(index: usize) -> String {
        format!("lyrics-import-result-{index}")
    }
}

/// The online tab's debounce.
pub const SEARCH_AFTER: Duration = Duration::from_millis(450);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Online,
    File,
    Paste,
}

/// What happens to a queued file on Add (`QueuedFileAction`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction {
    Skip,
    New,
    Replace,
}

/// One file waiting in the queue: parsed for reading, not yet saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub key: String,
    pub file_name: String,
    pub song: LyricsSong,
    pub source: LyricsImportSource,
    pub duplicate: Option<DuplicateMatch>,
    /// The duplicate is earlier in this batch, not in the library.
    pub in_batch: bool,
    pub action: FileAction,
    pub renamed: bool,
}

/// Song sheets arrive as SongSelect exports, plain text, RTF, or ProPresenter 7.
pub fn importable(name: &str) -> bool {
    let lower = name.to_lowercase();
    [".usr", ".txt", ".rtf", ".pro"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// "Way Maker.rtf" → "Way Maker", the last resort for an untitled document.
pub fn title_from_file_name(name: &str) -> String {
    let stem = match name.rfind('.') {
        Some(i) if i + 1 < name.len() => &name[..i],
        _ => name,
    };
    let spaced: String = stem
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let collapsed = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        "Untitled".into()
    } else {
        collapsed
    }
}

/// Binary-aware entry point: `.pro` is decoded as ProPresenter 7 protobuf;
/// text formats go through [`source_for_file`].
pub fn source_for_file_bytes(name: &str, raw: &[u8]) -> Result<LyricsImportSource, String> {
    if name.to_lowercase().ends_with(".pro") {
        let parsed = parse_propresenter(raw)?;
        let title = if parsed.title.trim().is_empty() {
            title_from_file_name(name)
        } else {
            parsed.title
        };
        return Ok(LyricsImportSource::ProPresenter {
            title,
            artist: parsed.artist,
            text: parsed.text,
            copyright: parsed.copyright,
            ccli_number: parsed.ccli_number,
        });
    }
    source_for_file(name, &String::from_utf8_lossy(raw))
}

/// `sourceForFile`: a header block is a SongSelect export; RTF is read to
/// plain text; otherwise the title is the clipboard parser's, the sheet's
/// own heading, or the file name.
pub fn source_for_file(name: &str, raw: &str) -> Result<LyricsImportSource, String> {
    let rtf = name.to_lowercase().ends_with(".rtf") || looks_like_rtf(raw);
    let text = if rtf {
        rtf_to_plain_text(raw)
    } else {
        raw.to_owned()
    };
    if rtf && text.trim().is_empty() {
        return Err("That RTF file has no readable text.".into());
    }
    if has_song_metadata(&text) {
        return Ok(LyricsImportSource::Usr {
            content: text,
            filename: Some(name.to_owned()),
        });
    }
    let draft = parse_clipboard_song(&text);
    let body = draft
        .as_ref()
        .map(|d| d.text.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| text.clone());
    let drafted_title = draft
        .as_ref()
        .map(|d| d.title.trim().to_owned())
        .unwrap_or_default();
    let lifted = if drafted_title.is_empty() {
        lift_heading_title(&body)
    } else {
        kairo_domain::lyrics::clipboard::LiftedHeading {
            title: String::new(),
            artist: String::new(),
            text: body.clone(),
        }
    };
    let title = [drafted_title, lifted.title.clone()]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| title_from_file_name(name));
    let artist = draft
        .as_ref()
        .map(|d| d.artist.trim().to_owned())
        .filter(|a| !a.is_empty())
        .unwrap_or(lifted.artist.clone());
    Ok(LyricsImportSource::Text {
        title,
        artist,
        text: if lifted.title.is_empty() {
            body
        } else {
            lifted.text
        },
        copyright: draft.map(|d| d.copyright).filter(|c| !c.is_empty()),
    })
}

/// `songToMarkedText`: the song as `[Label]`-marked text.
pub fn marked_text(song: &LyricsSong) -> String {
    song.sections
        .iter()
        .map(|s| format!("[{}]\n{}", s.label, s.lines.join("\n")))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `[Label]`-marked text as song sections (`parseMarkedSections`).
pub fn marked_text_sections(text: &str) -> Vec<LyricsSongSection> {
    parse_marked_sections(text)
        .into_iter()
        .map(|m| LyricsSongSection {
            kind: m.kind,
            label: m.label,
            lines: m.lines,
            line_colors: None,
            hotkey: None,
            extra: Default::default(),
        })
        .collect()
}

/// The note after reading the clipboard.
pub fn clipboard_note(lines: usize, title: &str, artist: &str) -> String {
    let found = [
        (!title.is_empty()).then(|| format!("title \"{title}\"")),
        (!artist.is_empty()).then(|| format!("artist \"{artist}\"")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(", ");
    format!(
        "Pasted {lines} lyric line{}{}",
        if lines == 1 { "" } else { "s" },
        if found.is_empty() {
            ". Add a title below.".to_owned()
        } else {
            format!(" — detected {found}.")
        }
    )
}

/// What the dialog tells the page.
#[derive(Clone, Debug, PartialEq)]
pub enum ImportEvent {
    /// Saved songs; the last opens.
    Imported(Vec<LyricsSong>),
    Close,
}

#[derive(Default)]
struct Online {
    results: Vec<LyricsOnlineResult>,
    searching: bool,
    searched: bool,
    importing: Option<String>,
    preview: Option<LyricsOnlineResult>,
    data: Option<LyricsOnlinePreview>,
    loading: bool,
    error: Option<String>,
    search_seq: u64,
    preview_seq: u64,
    cache: HashMap<String, LyricsOnlinePreview>,
    _search: Option<Task<()>>,
}

pub struct ImportDialog {
    /// The modal's focus scope; the workspace that opens the dialog sets it
    /// with [`Self::set_modal_focus`] and gives focus back on close.
    modal_focus: FocusHandle,
    _model: Entity<Lyrics>,
    backend: Arc<dyn LyricsBackend>,
    tab: Tab,
    busy: bool,
    error: Option<String>,
    queue: Vec<Queued>,
    file_errors: Vec<(String, String)>,
    show_error_details: bool,
    progress: Option<(usize, usize)>,
    selected: Option<String>,
    online: Online,
    query: Entity<InputState>,
    title: Entity<InputState>,
    artist: Entity<InputState>,
    copyright: Entity<InputState>,
    lyrics: Entity<TextareaState>,
    queued_title: Entity<InputState>,
    queued_artist: Entity<InputState>,
    queued_text: Entity<TextareaState>,
    queued_for: Option<String>,
    note: Option<String>,
    clipboard_read: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ImportEvent> for ImportDialog {}

impl ImportDialog {
    /// Uses the opener's modal focus scope (`ModalFocus::handle`).
    pub fn set_modal_focus(&mut self, handle: FocusHandle) {
        self.modal_focus = handle;
    }

    pub fn new(model: Entity<Lyrics>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let backend = model.read(cx).backend().clone();
        let input = |p: &str, window: &mut Window, cx: &mut Context<Self>| {
            let p = p.to_owned();
            cx.new(|cx| InputState::new(window, cx).placeholder(p))
        };
        let query = input("Song title, artist, or a line from the lyrics…", window, cx);
        let title = input("Song title", window, cx);
        let artist = input("Artist name", window, cx);
        let copyright = input("© Year Author", window, cx);
        let queued_title = input("Song title", window, cx);
        let queued_artist = input("Unknown artist", window, cx);
        let lyrics = cx.new(|cx| TextareaState::new(window, cx));
        let queued_text = cx.new(|cx| TextareaState::new(window, cx));
        let mut subs = Vec::new();
        subs.push(cx.subscribe(&query, |this, state, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let q = state.read(cx).value().to_string();
                this.set_query(q, cx);
            }
        }));
        for state in [&title, &artist, &copyright] {
            subs.push(cx.subscribe(state, |_, _, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    cx.notify();
                }
            }));
        }
        subs.push(cx.subscribe(&lyrics, |_, _, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                cx.notify();
            }
        }));
        subs.push(
            cx.subscribe(&queued_title, |this, s, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let v = s.read(cx).value().to_string();
                    this.rename_selected(Some(v), None, cx);
                }
            }),
        );
        subs.push(
            cx.subscribe(&queued_artist, |this, s, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let v = s.read(cx).value().to_string();
                    this.rename_selected(None, Some(v), cx);
                }
            }),
        );
        subs.push(
            cx.subscribe(&queued_text, |this, s, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let v = s.read(cx).value().to_string();
                    this.edit_selected_lyrics(v, cx);
                }
            }),
        );
        Self {
            modal_focus: cx.focus_handle(),
            _model: model,
            backend,
            tab: Tab::File,
            busy: false,
            error: None,
            queue: Vec::new(),
            file_errors: Vec::new(),
            show_error_details: false,
            progress: None,
            selected: None,
            online: Online::default(),
            query,
            title,
            artist,
            copyright,
            lyrics,
            queued_title,
            queued_artist,
            queued_text,
            queued_for: None,
            note: None,
            clipboard_read: false,
            _subscriptions: subs,
        }
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }
    pub fn queue(&self) -> &[Queued] {
        &self.queue
    }
    pub fn file_errors(&self) -> &[(String, String)] {
        &self.file_errors
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
    pub fn online_results(&self) -> &[LyricsOnlineResult] {
        &self.online.results
    }
    pub fn preview(&self) -> Option<&LyricsOnlinePreview> {
        self.online.data.as_ref()
    }
    pub fn preview_error(&self) -> Option<&str> {
        self.online.error.as_deref()
    }
    pub fn selected(&self) -> Option<&Queued> {
        let key = self.selected.as_deref()?;
        self.queue.iter().find(|q| q.key == key)
    }
    pub fn included(&self) -> usize {
        self.queue
            .iter()
            .filter(|q| q.action != FileAction::Skip)
            .count()
    }
    pub fn paste_form(&self, cx: &gpui_kit::App) -> (String, String, String, String) {
        (
            self.title.read(cx).value().to_string(),
            self.artist.read(cx).value().to_string(),
            self.copyright.read(cx).value().to_string(),
            self.lyrics.read(cx).value().to_string(),
        )
    }

    pub fn set_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        self.error = None;
        self.busy = false;
        // Opening Paste with an empty form pulls the song just copied.
        if tab == Tab::Paste && !self.clipboard_read {
            let (title, _, _, text) = self.paste_form(cx);
            if title.trim().is_empty() && text.trim().is_empty() {
                self.clipboard_read = true;
                self.read_clipboard(true, window, cx);
            }
        }
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(ImportEvent::Close);
    }

    // ─── Online ─────────────────────────────────────────────────────────────

    /// Debounced 450 ms; under three characters clears.
    pub fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        let q = query.trim().to_owned();
        self.online.preview = None;
        self.online.data = None;
        self.online.error = None;
        self.online.search_seq += 1;
        if q.chars().count() < rows::MIN_ONLINE_QUERY {
            self.online.results.clear();
            self.online.searched = false;
            self.online.searching = false;
            self.online._search = None;
            cx.notify();
            return;
        }
        let seq = self.online.search_seq;
        self.online.searching = true;
        let backend = self.backend.clone();
        self.online._search = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_AFTER).await;
            let answer = ask(&backend, LyricsCall::SearchOnline(q)).await;
            let _ = this.update(cx, |this, cx| {
                if this.online.search_seq != seq {
                    return;
                }
                this.online.searching = false;
                this.online.searched = true;
                match answer {
                    Ok(LyricsAnswer::Online(results)) => {
                        this.online.results = results;
                        this.error = None;
                    }
                    Ok(_) => this.online.results.clear(),
                    Err(e) => {
                        this.online.results.clear();
                        this.error = Some(clean_error(&e));
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    pub fn preview_result(&mut self, result: LyricsOnlineResult, cx: &mut Context<Self>) {
        self.online.preview_seq += 1;
        self.online.error = None;
        let unavailable = rows::lyrics_unavailable(&result);
        let cached = self.online.cache.get(&result.id).cloned();
        let source = OnlineSource::of(&result);
        let id = result.id.clone();
        self.online.preview = Some(result);
        self.online.data = cached.clone();
        self.online.loading = !unavailable && cached.is_none();
        if self.online.loading {
            let seq = self.online.preview_seq;
            let call = ask(&self.backend, LyricsCall::PreviewOnline(source));
            cx.spawn(async move |this, cx| {
                let answer = call.await;
                let _ = this.update(cx, |this, cx| {
                    if this.online.preview_seq != seq {
                        return;
                    }
                    this.online.loading = false;
                    match answer {
                        Ok(LyricsAnswer::Preview(p)) => {
                            this.online.cache.insert(id, p.clone());
                            this.online.data = Some(p);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            this.online.data = None;
                            this.online.error = Some(format_online_lyrics_error(&e));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    pub fn import_result(&mut self, result: LyricsOnlineResult, cx: &mut Context<Self>) {
        if rows::lyrics_unavailable(&result) {
            self.backend
                .call(LyricsCall::OpenUrl(result.url), Box::new(|_| {}));
            return;
        }
        if self.online.importing.is_some() {
            return;
        }
        self.online.importing = Some(result.id.clone());
        self.error = None;
        let call = ask(
            &self.backend,
            LyricsCall::Import(LyricsImportSource::Online {
                provider: result.provider,
                url: result.url,
                title: result.title,
                artist: result.artist,
            }),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.online.importing = None;
                match answer {
                    Ok(LyricsAnswer::Song(song)) => cx.emit(ImportEvent::Imported(vec![song])),
                    Ok(_) => {}
                    Err(e) => this.error = Some(format_online_lyrics_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn open_existing(&mut self, id: String, cx: &mut Context<Self>) {
        let call = ask(&self.backend, LyricsCall::GetSong(id));
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |_, cx| match answer {
                Ok(LyricsAnswer::Song(song)) => cx.emit(ImportEvent::Imported(vec![song])),
                _ => cx.emit(ImportEvent::Close),
            });
        })
        .detach();
    }

    // ─── Files ──────────────────────────────────────────────────────────────

    pub fn pick_files(&mut self, cx: &mut Context<Self>) {
        let call = ask(&self.backend, LyricsCall::PickSongFiles);
        cx.spawn(async move |this, cx| {
            if let Ok(LyricsAnswer::Files(files)) = call.await {
                let _ = this.update(cx, |this, cx| this.read_files(files, cx));
            }
        })
        .detach();
    }

    pub fn drop_paths(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        let call = ask(&self.backend, LyricsCall::ReadSongFiles(paths));
        cx.spawn(async move |this, cx| {
            if let Ok(LyricsAnswer::Files(files)) = call.await {
                let _ = this.update(cx, |this, cx| this.read_files(files, cx));
            }
        })
        .detach();
    }

    /// `readFiles`: files are parsed and duplicate-checked in chunks off the
    /// UI thread; nothing is saved. Bad files are recorded while the rest carry on.
    pub fn read_files(&mut self, files: Vec<SongFile>, cx: &mut Context<Self>) {
        const PREVIEW_CHUNK: usize = 128;

        let (accepted, rejected): (Vec<_>, Vec<_>) =
            files.into_iter().partition(|f| importable(&f.name));
        self.file_errors = rejected
            .iter()
            .map(|f| {
                (
                    f.name.clone(),
                    "Only .usr, .txt, .rtf and .pro files can be imported.".to_owned(),
                )
            })
            .collect();
        self.show_error_details = false;
        if accepted.is_empty() {
            if !rejected.is_empty() {
                self.error = Some("Only .usr, .txt, .rtf and .pro files are supported.".into());
            }
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        self.progress = Some((0, accepted.len()));
        let queued_so_far: Vec<LyricsSong> = self
            .queue
            .iter()
            .filter(|q| q.action != FileAction::Skip)
            .map(|q| q.song.clone())
            .collect();
        let mut batch_index = DuplicateIndex::new(&queued_so_far);
        let backend = self.backend.clone();
        let bg = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let mut read = Vec::with_capacity(accepted.len());
            let mut iter = accepted.into_iter();
            loop {
                let chunk: Vec<SongFile> = iter.by_ref().take(PREVIEW_CHUNK).collect();
                if chunk.is_empty() {
                    break;
                }
                let chunk_len = chunk.len();
                let parsed: Vec<(String, String, Result<LyricsImportSource, String>)> = bg
                    .spawn(async move {
                        let parse_one = |file: SongFile| {
                            let res = file
                                .content
                                .as_deref()
                                .map_err(Clone::clone)
                                .and_then(|raw| source_for_file_bytes(&file.name, raw));
                            (file.key, file.name, res)
                        };
                        if chunk.len() < 16 {
                            return chunk.into_iter().map(parse_one).collect();
                        }
                        let threads = std::thread::available_parallelism()
                            .map(usize::from)
                            .unwrap_or(1)
                            .min(8);
                        let part = chunk.len().div_ceil(threads).max(1);
                        let mut parts = Vec::with_capacity(threads);
                        let mut ci = chunk.into_iter();
                        loop {
                            let slice: Vec<_> = ci.by_ref().take(part).collect();
                            if slice.is_empty() {
                                break;
                            }
                            parts.push(slice);
                        }
                        std::thread::scope(|s| {
                            let handles: Vec<_> = parts
                                .into_iter()
                                .map(|p| {
                                    s.spawn(move || p.into_iter().map(parse_one).collect::<Vec<_>>())
                                })
                                .collect();
                            let mut out = Vec::with_capacity(chunk_len);
                            for h in handles {
                                if let Ok(v) = h.join() {
                                    out.extend(v);
                                }
                            }
                            out
                        })
                    })
                    .await;

                let mut chunk_errors: Vec<(String, String)> = Vec::new();
                let mut valid_meta: Vec<(String, String, LyricsImportSource)> = Vec::new();
                for (key, name, res) in parsed {
                    match res {
                        Ok(source) => valid_meta.push((key, name, source)),
                        Err(e) => chunk_errors.push((name, e)),
                    }
                }

                if !valid_meta.is_empty() {
                    let sources: Vec<LyricsImportSource> =
                        valid_meta.iter().map(|(_, _, s)| s.clone()).collect();
                    match ask(&backend, LyricsCall::PreviewFiles(sources)).await {
                        Ok(LyricsAnswer::ImportPreviews(previews)) => {
                            for ((key, name, source), preview_res) in
                                valid_meta.into_iter().zip(previews)
                            {
                                match preview_res {
                                    Ok(preview) => {
                                        let in_batch = preview
                                            .duplicate
                                            .is_none()
                                            .then(|| batch_index.find(&preview.song))
                                            .flatten();
                                        let is_batch = in_batch.is_some();
                                        let duplicate = preview.duplicate.clone().or(in_batch);
                                        if duplicate.is_none() {
                                            batch_index.insert(&preview.song);
                                        }
                                        read.push(Queued {
                                            key,
                                            file_name: name,
                                            song: preview.song,
                                            source,
                                            action: if duplicate.is_some() {
                                                FileAction::Skip
                                            } else {
                                                FileAction::New
                                            },
                                            duplicate,
                                            in_batch: is_batch,
                                            renamed: false,
                                        });
                                    }
                                    Err(e) => chunk_errors.push((name, clean_error(&e))),
                                }
                            }
                        }
                        Ok(_) => {
                            for (_, name, _) in valid_meta {
                                chunk_errors.push((name, "Could not read that file.".to_owned()));
                            }
                        }
                        Err(e) => {
                            let msg = clean_error(&e);
                            for (_, name, _) in valid_meta {
                                chunk_errors.push((name, msg.clone()));
                            }
                        }
                    }
                }

                let _ = this.update(cx, |this, cx| {
                    this.file_errors.extend(chunk_errors);
                    if let Some((done, _)) = this.progress.as_mut() {
                        *done += chunk_len;
                    }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                let first = read.first().map(|q| q.key.clone());
                let none = read.is_empty();
                // Re-dropping a file replaces its entry in O(1).
                let mut by_key: HashMap<String, usize> = this
                    .queue
                    .iter()
                    .enumerate()
                    .map(|(i, q)| (q.key.clone(), i))
                    .collect();
                for entry in read {
                    if let Some(&idx) = by_key.get(&entry.key) {
                        this.queue[idx] = entry;
                    } else {
                        by_key.insert(entry.key.clone(), this.queue.len());
                        this.queue.push(entry);
                    }
                }
                if this.selected.is_none() {
                    this.selected = first;
                }
                this.progress = None;
                this.busy = false;
                if none {
                    this.error = Some("None of those files could be read.".into());
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn select_queued(&mut self, key: String, cx: &mut Context<Self>) {
        self.selected = Some(key);
        cx.notify();
    }

    pub fn remove_queued(&mut self, key: &str, cx: &mut Context<Self>) {
        self.queue.retain(|q| q.key != key);
        if self.selected.as_deref() == Some(key) {
            self.selected = self.queue.first().map(|q| q.key.clone());
        }
        cx.notify();
    }

    pub fn clear_queue(&mut self, cx: &mut Context<Self>) {
        self.queue.clear();
        self.file_errors.clear();
        self.selected = None;
        cx.notify();
    }

    pub fn set_action(&mut self, action: FileAction, cx: &mut Context<Self>) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        if let Some(q) = self.queue.iter_mut().find(|q| q.key == key) {
            // Nothing exists to overwrite when the twin is only queued.
            if action == FileAction::Replace && q.in_batch {
                return;
            }
            q.action = action;
        }
        cx.notify();
    }

    fn rename_selected(
        &mut self,
        title: Option<String>,
        artist: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        if let Some(q) = self.queue.iter_mut().find(|q| q.key == key) {
            let mut changed = false;
            if let Some(t) = title.filter(|t| *t != q.song.title) {
                q.song.title = t;
                changed = true;
            }
            if let Some(a) = artist.filter(|a| *a != q.song.artist) {
                q.song.artist = a;
                changed = true;
            }
            if changed {
                q.renamed = true;
                cx.notify();
            }
        }
    }

    /// `editQueuedLyrics`: the text is parsed as marked sections and becomes
    /// a plain-text source.
    pub fn edit_selected_lyrics(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(key) = self.selected.clone() else {
            return;
        };
        if let Some(q) = self.queue.iter_mut().find(|q| q.key == key) {
            if marked_text(&q.song) == text {
                return;
            }
            q.song.sections = marked_text_sections(&text);
            q.source = LyricsImportSource::Text {
                title: q.song.title.clone(),
                artist: q.song.artist.clone(),
                text,
                copyright: q.song.copyright.clone(),
            };
            q.renamed = true;
            cx.notify();
        }
    }

    /// Add: the only path that saves. Plain new songs save in batched SQLite
    /// transactions; replacements and renamed entries save individually.
    pub fn confirm_files(&mut self, cx: &mut Context<Self>) {
        const SAVE_CHUNK: usize = 256;

        let chosen: Vec<Queued> = self
            .queue
            .iter()
            .filter(|q| q.action != FileAction::Skip)
            .cloned()
            .collect();
        if chosen.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        self.progress = Some((0, chosen.len()));
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let mut saved = Vec::with_capacity(chosen.len());
            let mut failures = Vec::new();
            let (batchable, individual): (Vec<Queued>, Vec<Queued>) = chosen
                .into_iter()
                .partition(|e| e.action != FileAction::Replace && !e.renamed);

            // Fast path: plain new imports in single-transaction chunks.
            let mut bi = batchable.into_iter();
            loop {
                let chunk: Vec<Queued> = bi.by_ref().take(SAVE_CHUNK).collect();
                if chunk.is_empty() {
                    break;
                }
                let chunk_len = chunk.len();
                let sources: Vec<LyricsImportSource> =
                    chunk.iter().map(|e| e.source.clone()).collect();
                match ask(&backend, LyricsCall::ImportBatch(sources)).await {
                    Ok(LyricsAnswer::ImportBatch(results)) => {
                        for (entry, res) in chunk.into_iter().zip(results) {
                            match res {
                                Ok(s) => saved.push(s),
                                Err(e) => failures.push((entry.file_name, clean_error(&e))),
                            }
                        }
                    }
                    Ok(_) => {
                        for entry in chunk {
                            failures.push((entry.file_name, "Could not add that song.".into()));
                        }
                    }
                    Err(e) => {
                        let msg = clean_error(&e);
                        for entry in chunk {
                            failures.push((entry.file_name, msg.clone()));
                        }
                    }
                }
                let _ = this.update(cx, |this, cx| {
                    if let Some((done, _)) = this.progress.as_mut() {
                        *done += chunk_len;
                    }
                    cx.notify();
                });
            }

            // Individual path: replacements and renamed/edited entries.
            for entry in individual {
                let result: Result<LyricsSong, String> = if entry.action == FileAction::Replace
                    && !entry.in_batch
                    && let Some(dup) = &entry.duplicate
                {
                    match ask(
                        &backend,
                        LyricsCall::Update(LyricsSong {
                            id: dup.song_id.clone(),
                            ..entry.song.clone()
                        }),
                    )
                    .await
                    {
                        Ok(LyricsAnswer::Song(s)) => Ok(s),
                        Ok(_) => Err("The song it replaces is no longer there.".into()),
                        Err(e) if e.is_empty() => {
                            Err("The song it replaces is no longer there.".into())
                        }
                        Err(e) => Err(clean_error(&e)),
                    }
                } else {
                    match ask(&backend, LyricsCall::Import(entry.source.clone())).await {
                        Ok(LyricsAnswer::Song(imported)) => {
                            let corrected = entry.renamed
                                && (imported.title != entry.song.title
                                    || imported.artist != entry.song.artist);
                            if corrected {
                                let title = entry.song.title.trim();
                                match ask(
                                    &backend,
                                    LyricsCall::Update(LyricsSong {
                                        title: if title.is_empty() {
                                            imported.title.clone()
                                        } else {
                                            title.to_owned()
                                        },
                                        artist: entry.song.artist.clone(),
                                        sections: entry.song.sections.clone(),
                                        ..imported.clone()
                                    }),
                                )
                                .await
                                {
                                    Ok(LyricsAnswer::Song(s)) => Ok(s),
                                    _ => Ok(imported),
                                }
                            } else {
                                Ok(imported)
                            }
                        }
                        Ok(_) => Err("Could not add that song.".into()),
                        Err(e) => Err(clean_error(&e)),
                    }
                };
                match result {
                    Ok(s) => saved.push(s),
                    Err(m) => failures.push((entry.file_name.clone(), m)),
                }
                let _ = this.update(cx, |this, cx| {
                    if let Some((done, _)) = this.progress.as_mut() {
                        *done += 1;
                    }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.progress = None;
                this.busy = false;
                if saved.is_empty() {
                    this.file_errors = failures;
                    this.error = Some("None of those songs could be added.".into());
                } else {
                    cx.emit(ImportEvent::Imported(saved));
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ─── Paste ──────────────────────────────────────────────────────────────

    /// Fills the form from song text on the clipboard. `silent`: the
    /// automatic read on opening the tab says nothing when empty.
    pub fn read_clipboard(&mut self, silent: bool, window: &mut Window, cx: &mut Context<Self>) {
        let raw = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .unwrap_or_default();
        self.apply_paste(&raw, silent, false, window, cx);
    }

    /// Applies song text: from the clipboard button, or text pasted into the
    /// lyrics box (`into_box`), which keeps fields already typed.
    pub fn apply_paste(
        &mut self,
        raw: &str,
        silent: bool,
        into_box: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if raw.trim().is_empty() || !looks_like_lyrics(raw) {
            if !silent {
                self.note =
                    Some("No song text on the clipboard — copy the lyrics, then try again.".into());
            }
            cx.notify();
            return false;
        }
        let Some(draft) = parse_clipboard_song(raw) else {
            if !silent {
                self.note = Some("Could not find lyrics in the copied text.".into());
            }
            cx.notify();
            return false;
        };
        let (title, artist, copyright, _) = self.paste_form(cx);
        let pick = |held: String, found: &str| -> String {
            if into_box {
                if held.is_empty() {
                    found.to_owned()
                } else {
                    held
                }
            } else if found.is_empty() {
                held
            } else {
                found.to_owned()
            }
        };
        let (t, a, c) = (
            pick(title, &draft.title),
            pick(artist, &draft.artist),
            pick(copyright, &draft.copyright),
        );
        self.title.update(cx, |s, cx| s.set_value(t, window, cx));
        self.artist.update(cx, |s, cx| s.set_value(a, window, cx));
        self.copyright
            .update(cx, |s, cx| s.set_value(c, window, cx));
        let body = draft.text.clone();
        self.lyrics
            .update(cx, |s, cx| s.set_value(body, window, cx));
        self.error = None;
        self.note = Some(if into_box {
            format!(
                "Cleaned {} lyric line{} from the pasted text.",
                draft.line_count,
                if draft.line_count == 1 { "" } else { "s" }
            )
        } else {
            clipboard_note(draft.line_count, &draft.title, &draft.artist)
        });
        cx.notify();
        true
    }

    /// The paste form's title (tests and the title field).
    pub fn set_paste_title(&mut self, title: &str, window: &mut Window, cx: &mut Context<Self>) {
        let title = title.to_owned();
        self.title
            .update(cx, |s, cx| s.set_value(title, window, cx));
        cx.notify();
    }

    pub fn confirm_paste(&mut self, cx: &mut Context<Self>) {
        let (title, artist, copyright, text) = self.paste_form(cx);
        if title.trim().is_empty() || text.trim().is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        let call = ask(
            &self.backend,
            LyricsCall::Import(LyricsImportSource::Text {
                title: title.trim().to_owned(),
                artist: artist.trim().to_owned(),
                text,
                copyright: Some(copyright.trim().to_owned()).filter(|c| !c.is_empty()),
            }),
        );
        cx.spawn(async move |this, cx| {
            let answer = call.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match answer {
                    Ok(LyricsAnswer::Song(song)) => cx.emit(ImportEvent::Imported(vec![song])),
                    Ok(_) => {}
                    Err(e) => this.error = Some(format_online_lyrics_error(&e)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    // ─── Render ─────────────────────────────────────────────────────────────

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected().cloned();
        let key = selected.as_ref().map(|q| q.key.clone());
        if key != self.queued_for {
            self.queued_for = key;
            if let Some(q) = selected {
                self.queued_title
                    .update(cx, |s, cx| s.set_value(q.song.title.clone(), window, cx));
                self.queued_artist
                    .update(cx, |s, cx| s.set_value(q.song.artist.clone(), window, cx));
                self.queued_text
                    .update(cx, |s, cx| s.set_value(marked_text(&q.song), window, cx));
            }
        }
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let tab = |id: &'static str, t: Tab, label: &'static str| {
            let e = entity.clone();
            let on = self.tab == t;
            div()
                .id(id)
                .test_support()
                .role(Role::Tab)
                .aria_label(label)
                .aria_selected(on)
                .px_3p5()
                .py_1p5()
                .rounded(radius::md())
                .text_size(text::SM.size)
                .font_weight(FontWeight::MEDIUM)
                .map(|d| {
                    if on {
                        d.bg(k.surface_tertiary)
                            .border_1()
                            .border_color(k.white.opacity(0.14))
                            .text_color(k.text_primary)
                    } else {
                        d.border_1()
                            .border_color(gpui_kit::transparent_black())
                            .text_color(k.text(TextStep::T400))
                            .hover(|s| s.text_color(k.text_primary))
                    }
                })
                .cursor_pointer()
                .on_click(move |_, window, cx| e.update(cx, |this, cx| this.set_tab(t, window, cx)))
                .child(label)
        };
        h_flex()
            .px_5()
            .pt_1()
            .child(
                h_flex()
                    .gap_0p5()
                    .rounded(radius::lg())
                    .bg(k.surface)
                    .border_1()
                    .border_color(k.panel_border)
                    .p_0p5()
                    .child(tab(ids::TAB_FILE, Tab::File, "Upload File"))
                    .child(tab(ids::TAB_PASTE, Tab::Paste, "Paste Lyrics"))
                    .child(tab(ids::TAB_ONLINE, Tab::Online, "Search Online")),
            )
            .into_any_element()
    }

    fn render_error(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let k = *cx.kairo();
        self.error.clone().map(|e| {
            div()
                .id(ids::ERROR)
                .test_support()
                .role(Role::Alert)
                .rounded(radius::lg())
                .bg(k.tints.red)
                .px_3()
                .py_2p5()
                .text_size(text::XS.size)
                .text_color(k.danger_text)
                .child(e)
                .into_any_element()
        })
    }

    fn render_online(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let q = self.query.read(cx).value().trim().chars().count();
        let rows_: Vec<AnyElement> = self
            .online
            .results
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let (pick, act) = (entity.clone(), entity.clone());
                let (r1, r2) = (r.clone(), r.clone());
                let selected = self.online.preview.as_ref().is_some_and(|p| p.id == r.id);
                let busy = self.online.importing.as_deref() == Some(r.id.as_str());
                let action = match r.existing_song_id.clone() {
                    Some(existing) => {
                        Button::new(SharedString::from(format!("{}-act", ids::result(i))))
                            .cursor_pointer()
                            .ghost()
                            .xsmall()
                            .label("In library")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                act.update(cx, |this, cx| this.open_existing(existing.clone(), cx))
                            })
                    }
                    None => Button::new(SharedString::from(format!("{}-act", ids::result(i))))
                        .cursor_pointer()
                        .ghost()
                        .xsmall()
                        .label(if busy {
                            "Importing…"
                        } else if rows::lyrics_unavailable(r) {
                            "Open source"
                        } else {
                            "Import"
                        })
                        .disabled(self.online.importing.is_some())
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            act.update(cx, |this, cx| this.import_result(r2.clone(), cx))
                        }),
                };
                h_flex()
                    .id(SharedString::from(ids::result(i)))
                    .test_support()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .rounded(radius::lg())
                    .map(|d| {
                        if selected {
                            d.bg(k.row_selected())
                        } else {
                            d.hover(|s| s.bg(k.surface_elevated))
                        }
                    })
                    .cursor_pointer()
                    .on_click(move |_, _, cx| {
                        pick.update(cx, |this, cx| this.preview_result(r1.clone(), cx))
                    })
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text(TextStep::T200))
                                    .child(r.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(rows::online_meta(r)),
                            ),
                    )
                    .child(action)
                    .into_any_element()
            })
            .collect();
        let empty = if !self.online.searching
            && self.online.searched
            && self.online.results.is_empty()
            && self.error.is_none()
        {
            Some("No matches found")
        } else if !self.online.searched && q < rows::MIN_ONLINE_QUERY {
            Some("Search by any part of the song")
        } else {
            None
        };
        let pane: AnyElement = match &self.online.preview {
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T300))
                .child("Click a result to preview lyrics before importing")
                .into_any_element(),
            Some(r) if rows::lyrics_unavailable(r) => v_flex()
                .flex_1()
                .gap_3()
                .p_4()
                .child(
                    div()
                        .text_size(text::SM.size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(r.title.clone()),
                )
                .child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T400))
                        .child(rows::UNAVAILABLE_PANE),
                )
                .into_any_element(),
            Some(r) => {
                let body: AnyElement = if self.online.loading {
                    div()
                        .py_16()
                        .text_center()
                        .text_size(text::SM.size)
                        .text_color(k.text(TextStep::T400))
                        .child("Loading and reviewing lyrics…")
                        .into_any_element()
                } else if let Some(e) = &self.online.error {
                    v_flex()
                        .gap_1p5()
                        .py_12()
                        .px_6()
                        .text_center()
                        .child(div().text_size(text::SM.size).text_color(k.text(TextStep::T200)).child(e.clone()))
                        .child(div().text_size(text::XS.size).text_color(k.text(TextStep::T400)).child("Try another search result, or paste lyrics under Import → Paste."))
                        .into_any_element()
                } else if let Some(p) = &self.online.data {
                    v_flex()
                        .gap_5()
                        .children(review_notice_element(p.review.as_ref(), cx))
                        .children(p.sections.iter().map(|s| section_block(k, s)))
                        .into_any_element()
                } else {
                    div().into_any_element()
                };
                let e = entity.clone();
                let result = r.clone();
                let action = match r.existing_song_id.clone() {
                    Some(id) => Button::new("lyrics-import-preview-open")
                        .cursor_pointer()
                        .primary()
                        .small()
                        .label("Open in library")
                        .on_click(move |_, _, cx| {
                            e.update(cx, |this, cx| this.open_existing(id.clone(), cx))
                        }),
                    None => Button::new("lyrics-import-preview-import")
                        .cursor_pointer()
                        .primary()
                        .small()
                        .label(if self.online.importing.is_some() {
                            "Importing…"
                        } else {
                            "Import song"
                        })
                        .disabled(
                            self.online.importing.is_some()
                                || self.online.loading
                                || self.online.error.is_some()
                                || self.online.data.is_none(),
                        )
                        .on_click(move |_, _, cx| {
                            e.update(cx, |this, cx| this.import_result(result.clone(), cx))
                        }),
                };
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        v_flex()
                            .px_4()
                            .py_3()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::SM.size)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(k.text_primary)
                                    .child(r.title.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(rows::online_meta(r)),
                            ),
                    )
                    .child(
                        div()
                            .id("lyrics-import-preview-body")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_4()
                            .py_3()
                            .child(body),
                    )
                    .child(h_flex().justify_end().px_4().py_3().child(action))
                    .into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .px_5()
            .py_4()
            .child(
                v_flex()
                    .child(
                        div()
                            .rounded(radius::md())
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.white.opacity(0.18))
                            .child(
                                Input::new(&self.query)
                                    .appearance(false)
                                    .kairo_size()
                                    .prefix(Icon::new(KairoIcon::ScanSearch).size_3p5()),
                            ),
                    )
                    .child(div().mt_1p5().text_size(text::LABEL.size).text_color(k.text(TextStep::T400)).child("Click a result to preview lyrics, then Import if it’s the right song.")),
            )
            .children(self.render_error(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .overflow_hidden()
                    .rounded(radius::xl())
                    .bg(k.surface)
                    .border_1()
                    .border_color(k.panel_border)
                    .child(
                        v_flex()
                            .id("lyrics-import-results")
                            .w(px(340.))
                            .flex_none()
                            .overflow_y_scroll()
                            .p_2()
                            .gap_1()
                            .border_r_1()
                            .border_color(k.panel_border)
                            .children(rows_)
                            .children(empty.map(|t| {
                                div().id(ids::ONLINE_EMPTY).test_support().py_10().text_center().text_size(text::SM.size).text_color(k.text(TextStep::T300)).child(t)
                            })),
                    )
                    .child(pane),
            )
            .into_any_element()
    }

    fn render_drop(&self, compact: bool, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let drop = entity.clone();
        let label = match self.progress {
            Some((done, total)) => format!("Reading {} of {total}…", (done + 1).min(total)),
            None if compact => "Drop more files".into(),
            None => "Drop .usr, .txt, .rtf or .pro files here".into(),
        };
        v_flex()
            .id(ids::DROP)
            .test_support()
            .role(Role::Button)
            .aria_label(label.clone())
            .when(!compact, |d| {
                d.aria_description("or click to browse — pick as many as you like")
            })
            .items_center()
            .gap_2()
            .rounded(radius::xl())
            .border_1()
            .border_dashed()
            .border_color(k.white.opacity(0.22))
            .bg(k.surface)
            .hover(|s| {
                s.bg(k.surface_header)
                    .border_color(k.accent(kairo_ui_kit::tokens::Step::S400).opacity(0.6))
            })
            .drag_over::<ExternalPaths>(move |s, _, _, cx| s.bg(cx.kairo().tints.teal))
            .on_drop(move |paths: &ExternalPaths, _, cx| {
                let list = paths.paths().to_vec();
                drop.update(cx, |this, cx| this.drop_paths(list, cx));
            })
            .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.pick_files(cx)))
            .map(|d| {
                if compact {
                    d.px_3().py_3()
                } else {
                    d.px_6().py_12()
                }
            })
            .child(
                Icon::new(if self.progress.is_some() {
                    KairoIcon::Loader
                } else {
                    KairoIcon::Upload
                })
                .size_5()
                .text_color(k.text(TextStep::T400)),
            )
            .child(
                div()
                    .text_size(if compact {
                        text::XS.size
                    } else {
                        text::SM.size
                    })
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(k.text(TextStep::T200))
                    .child(label),
            )
            .when(!compact, |d| {
                d.child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("or click to browse — pick as many as you like"),
                )
            })
            .into_any_element()
    }

    fn render_file_errors(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let k = *cx.kairo();
        if self.file_errors.len() <= 4 {
            return self
                .file_errors
                .iter()
                .map(|(name, m)| {
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.warning_text)
                        .child(format!("{name} — {m}"))
                        .into_any_element()
                })
                .collect();
        }
        // Group identical skip reasons so bulk imports don't flood the modal.
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for (name, reason) in &self.file_errors {
            if let Some((_, names)) = groups.iter_mut().find(|(r, _)| r == reason) {
                names.push(name.as_str());
            } else {
                groups.push((reason.clone(), vec![name.as_str()]));
            }
        }
        let expanded = self.show_error_details;
        let toggle = cx.entity();
        let total = self.file_errors.len();
        let mut out: Vec<AnyElement> = Vec::new();
        out.push(
            v_flex()
                .gap_1()
                .p_2p5()
                .rounded(radius::md())
                .bg(k.surface_header)
                .border_1()
                .border_color(k.panel_border)
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_size(text::LABEL.size)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(k.warning_text)
                                .child(format!("{total} files skipped")),
                        )
                        .child(
                            Button::new("lyrics-import-toggle-errors")
                                .cursor_pointer()
                                .ghost()
                                .xsmall()
                                .label(if expanded { "Hide details" } else { "Show details" })
                                .on_click(move |_, _, cx| {
                                    toggle.update(cx, |this, cx| {
                                        this.show_error_details = !this.show_error_details;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
                .children(groups.iter().map(|(reason, names)| {
                    div()
                        .text_size(text::LABEL.size)
                        .text_color(k.text(TextStep::T400))
                        .child(format!("{} — {reason}", rows::count_label(names.len(), "file")))
                }))
                .when(expanded, |el| {
                    let preview_limit = 50;
                    let extra = total.saturating_sub(preview_limit);
                    el.child(
                        v_flex()
                            .mt_1()
                            .pt_1p5()
                            .border_t_1()
                            .border_color(k.panel_border)
                            .gap_0p5()
                            .max_h(px(140.))
                            .id("lyrics-import-error-details")
                            .overflow_y_scroll()
                            .children(self.file_errors.iter().take(preview_limit).map(|(n, m)| {
                                div()
                                    .text_size(text::MICRO.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(format!("{n} — {m}"))
                            }))
                            .when(extra > 0, |list| {
                                list.child(
                                    div()
                                        .text_size(text::MICRO.size)
                                        .text_color(k.text(TextStep::T500))
                                        .child(format!("…and {extra} more")),
                                )
                            }),
                    )
                })
                .into_any_element(),
        );
        out
    }

    fn render_queued_row(&self, i: usize, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let q = &self.queue[i];
        let (pick, rm) = (cx.entity(), cx.entity());
        let (k1, k2) = (q.key.clone(), q.key.clone());
        let selected = self.selected.as_deref() == Some(q.key.as_str());
        let detail = match &q.duplicate {
            Some(_) if q.in_batch => "Already in this batch".to_owned(),
            Some(_) => "Already in your library".to_owned(),
            None => format!(
                "{} · {}",
                if q.song.artist.is_empty() {
                    "Unknown artist"
                } else {
                    &q.song.artist
                },
                rows::count_label(q.song.sections.len(), "section")
            ),
        };
        let badge = q.duplicate.as_ref().map(|_| match q.action {
            FileAction::Skip => "Skipped",
            FileAction::New => "New copy",
            FileAction::Replace => "Replaces",
        });
        h_flex()
            .id(SharedString::from(ids::queued(i)))
            .test_support()
            .h(px(48.))
            .gap_2()
            .px_2()
            .rounded(radius::lg())
            .when(q.action == FileAction::Skip, |d| d.opacity(0.6))
            .map(|d| {
                if selected {
                    d.bg(k.row_selected())
                } else {
                    d.hover(|s| s.bg(k.surface_tertiary))
                }
            })
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                pick.update(cx, |this, cx| this.select_queued(k1.clone(), cx))
            })
            .child(
                Icon::new(if q.duplicate.is_some() {
                    KairoIcon::AlertCircle
                } else {
                    KairoIcon::Music2
                })
                .size_3()
                .text_color(if q.duplicate.is_some() {
                    k.warning_text
                } else {
                    k.notice_text
                }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .truncate()
                            .text_size(text::SM.size)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(k.text(TextStep::T200))
                            .child(q.song.title.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(text::LABEL.size)
                            .text_color(if q.duplicate.is_some() {
                                k.warning_text
                            } else {
                                k.text(TextStep::T400)
                            })
                            .child(detail),
                    ),
            )
            .children(badge.map(|b| {
                div()
                    .flex_none()
                    .text_size(text::MICRO.size)
                    .text_color(k.text(TextStep::T400))
                    .child(b)
            }))
            .child(
                Button::new(SharedString::from(format!("{}-remove", ids::queued(i))))
                    .cursor_pointer()
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(KairoIcon::X))
                    .accessibility_label(SharedString::from(format!(
                        "Remove {} from this import",
                        q.song.title
                    )))
                    .on_click(move |_, _, cx| {
                        cx.stop_propagation();
                        rm.update(cx, |this, cx| this.remove_queued(&k2, cx))
                    }),
            )
            .into_any_element()
    }

    fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        if self.queue.is_empty() {
            return v_flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .px_5()
                .py_5()
                .child(self.render_drop(false, cx))
                .children(self.render_file_errors(cx))
                .children(self.render_error(cx))
                .into_any_element();
        }
        let entity = cx.entity();
        let duplicates = self.queue.iter().filter(|q| q.duplicate.is_some()).count();
        let skipped = self.queue.len() - self.included();
        let queue_len = self.queue.len();
        let preview: AnyElement = match self.selected() {
            None => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(text::SM.size)
                .text_color(k.text(TextStep::T300))
                .child("Pick a song to read it")
                .into_any_element(),
            Some(q) => {
                let dup = q.duplicate.as_ref().map(|d| {
                    let choices: Vec<AnyElement> = [
                        (FileAction::Skip, "skip", "Don't add"),
                        (FileAction::New, "new", "Save a new copy"),
                        (FileAction::Replace, "replace", "Replace the old one"),
                    ]
                    .into_iter()
                    .filter(|(a, _, _)| !(*a == FileAction::Replace && q.in_batch))
                    .map(|(a, id, label)| {
                        let e = entity.clone();
                        let on = q.action == a;
                        Button::new(SharedString::from(ids::action(id)))
                            .cursor_pointer()
                            .xsmall()
                            .label(label)
                            .map(|b| if on { b.primary() } else { b.ghost() })
                            .on_click(move |_, _, cx| {
                                e.update(cx, |this, cx| this.set_action(a, cx))
                            })
                            .into_any_element()
                    })
                    .collect();
                    v_flex()
                        .mb_2()
                        .rounded(radius::lg())
                        .bg(k.tints.amber)
                        .px_2p5()
                        .py_2()
                        .child(
                            div()
                                .text_size(text::LABEL.size)
                                .text_color(k.warning_text)
                                .child(format!(
                                    "{}{}.",
                                    describe_duplicate(d),
                                    if q.in_batch {
                                        " (earlier in this import)"
                                    } else {
                                        " (in your library)"
                                    }
                                )),
                        )
                        .child(h_flex().mt_2().gap_1p5().flex_wrap().children(choices))
                });
                let sections: Vec<AnyElement> = q
                    .song
                    .sections
                    .iter()
                    .map(|s| {
                        div()
                            .rounded(radius::sm())
                            .px_2()
                            .bg(kairo_ui_kit::tokens::section_color(rows::section_kind(
                                s.kind,
                            )))
                            .text_color(kairo_ui_kit::tokens::section_text())
                            .text_size(text::MICRO.size)
                            .font_weight(FontWeight::BOLD)
                            .child(s.kind.as_str().to_uppercase())
                            .into_any_element()
                    })
                    .collect();
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        v_flex()
                            .px_4()
                            .py_3()
                            .children(dup)
                            .child(Input::new(&self.queued_title).kairo_size().appearance(false))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(div().flex_1().child(Input::new(&self.queued_artist).kairo_size().small().appearance(false)))
                                    .child(div().flex_none().text_size(text::LABEL.size).text_color(k.text(TextStep::T500)).child(q.file_name.clone())),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_h_0()
                            .gap_2()
                            .px_4()
                            .py_3()
                            .child(div().flex_1().min_h_0().child(Textarea::new(&self.queued_text).h_full().aria_label("Lyrics")))
                            .when(!sections.is_empty(), |d| d.child(h_flex().gap_1().flex_wrap().child(div().mr_1().text_size(text::LABEL.size).text_color(k.text(TextStep::T400)).child("Sections")).children(sections)))
                            .child(div().text_size(text::LABEL.size).text_color(k.text(TextStep::T500)).child("[Verse 1], [Chorus] mark sections · a blank line breaks a slide")),
                    )
                    .into_any_element()
            }
        };
        let clear = entity.clone();
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .px_5()
            .py_4()
            .children(self.render_error(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .overflow_hidden()
                    .rounded(radius::xl())
                    .bg(k.surface_tertiary)
                    .child(
                        v_flex()
                            .w(px(340.))
                            .flex_none()
                            .border_r_1()
                            .border_color(k.surface_border)
                            .child(div().p_2().child(self.render_drop(true, cx)))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .px_3()
                                    .pb_1p5()
                                    .text_size(text::LABEL.size)
                                    .text_color(k.text(TextStep::T400))
                                    .child(format!(
                                        "{} ready{}{}",
                                        self.included(),
                                        if duplicates > 0 {
                                            format!(
                                                " · {}",
                                                rows::count_label(duplicates, "duplicate")
                                            )
                                        } else {
                                            String::new()
                                        },
                                        if skipped > 0 {
                                            format!(" · {skipped} skipped")
                                        } else {
                                            String::new()
                                        }
                                    ))
                                    .child(
                                        Button::new(ids::CLEAR_ALL)
                                            .cursor_pointer()
                                            .ghost()
                                            .xsmall()
                                            .label("Clear all")
                                            .on_click(move |_, _, cx| {
                                                clear.update(cx, |this, cx| this.clear_queue(cx))
                                            }),
                                    ),
                            )
                            .child(
                                uniform_list(
                                    "lyrics-import-queue",
                                    queue_len,
                                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                        range
                                            .map(|i| this.render_queued_row(i, cx))
                                            .collect()
                                    }),
                                )
                                .flex_1()
                                .min_h_0()
                                .px_2()
                                .pb_2(),
                            )
                            .when(!self.file_errors.is_empty(), |col| {
                                col.child(
                                    v_flex()
                                        .px_2()
                                        .pb_2()
                                        .gap_1()
                                        .children(self.render_file_errors(cx)),
                                )
                            }),
                    )
                    .child(preview),
            )
            .into_any_element()
    }

    fn render_paste(&self, cx: &mut Context<Self>) -> AnyElement {
        let k = *cx.kairo();
        let entity = cx.entity();
        let paste_entity = entity.downgrade();
        let field = |label: &'static str, input: &Entity<InputState>| {
            v_flex()
                .flex_1()
                .gap_1()
                .child(
                    div()
                        .text_size(text::LABEL.size)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(k.text(TextStep::T300))
                        .child(label),
                )
                .child(
                    div()
                        .rounded(radius::md())
                        .bg(k.surface)
                        .border_1()
                        .border_color(k.white.opacity(0.18))
                        .child(Input::new(input).kairo_size().appearance(false)),
                )
        };
        v_flex()
            .id("lyrics-import-paste")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3()
            .px_5()
            .py_5()
            .child(
                h_flex()
                    .justify_between()
                    .gap_3()
                    .p_3()
                    .rounded(radius::xl())
                    .bg(k.surface_header)
                    .border_1()
                    .border_color(k.panel_border)
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(div().text_size(text::SM.size).font_weight(FontWeight::MEDIUM).text_color(k.text_primary).child("Import from clipboard"))
                            .child(div().mt_0p5().text_size(text::LABEL.size).text_color(k.text(TextStep::T400)).child("Copy a song from any lyrics site, then paste it here — title, artist and site clutter are sorted out for you.")),
                    )
                    .child(
                        Button::new(ids::CLIPBOARD).cursor_pointer()
                            .small()
                            .label("Paste from clipboard")
                            .on_click(move |_, window, cx| entity.update(cx, |this, cx| this.read_clipboard(false, window, cx))),
                    ),
            )
            .children(self.note.clone().map(|n| div().id(ids::NOTE).test_support().text_size(text::LABEL.size).text_color(k.notice_text).child(n)))
            .child(h_flex().gap_3().child(field("Title *", &self.title)).child(field("Artist", &self.artist)))
            .child(field("Copyright", &self.copyright))
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_size(text::LABEL.size).font_weight(FontWeight::MEDIUM).text_color(k.text(TextStep::T300)).child("Lyrics *"))
                    .child(
                        div()
                            .rounded(radius::md())
                            .bg(k.surface)
                            .border_1()
                            .border_color(k.white.opacity(0.18))
                            .child(
                                Textarea::new(&self.lyrics)
                                    .appearance(false)
                                    .h(px(220.))
                                    .aria_label("Lyrics")
                                    .on_paste(move |item, window, cx| {
                                        let Some(raw) = item.text() else { return false };
                                        paste_entity
                                            .update(cx, |this, cx| this.apply_paste(&raw, true, true, window, cx))
                                            .unwrap_or(false)
                                    }),
                            ),
                    )
                    .child(div().text_size(text::LABEL.size).text_color(k.text(TextStep::T500)).child("Sections auto-detected from labels or double blank lines.")),
            )
            .children(self.render_error(cx))
            .into_any_element()
    }
}

impl Render for ImportDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync(window, cx);
        let k = *cx.kairo();
        let entity = cx.entity();
        let wide = self.tab == Tab::Online || (self.tab == Tab::File && !self.queue.is_empty());
        let body = match self.tab {
            Tab::Online => self.render_online(cx),
            Tab::File => self.render_files(cx),
            Tab::Paste => self.render_paste(cx),
        };
        let (title, _, _, text_) = self.paste_form(cx);
        let confirm: Option<AnyElement> = match self.tab {
            Tab::Online => None,
            Tab::File => {
                let e = entity.clone();
                let n = self.included();
                Some(
                    Button::new(ids::CONFIRM)
                        .cursor_pointer()
                        .primary()
                        .label(if n > 1 {
                            format!("Add {n} Songs")
                        } else {
                            "Add Song".into()
                        })
                        .disabled(n == 0 || self.busy)
                        .on_click(move |_, _, cx| e.update(cx, |this, cx| this.confirm_files(cx)))
                        .into_any_element(),
                )
            }
            Tab::Paste => {
                let e = entity.clone();
                Some(
                    Button::new(ids::CONFIRM)
                        .cursor_pointer()
                        .primary()
                        .label("Import Song")
                        .disabled(title.trim().is_empty() || text_.trim().is_empty() || self.busy)
                        .on_click(move |_, _, cx| e.update(cx, |this, cx| this.confirm_paste(cx)))
                        .into_any_element(),
                )
            }
        };
        let (close, cancel, backdrop) = (entity.clone(), entity.clone(), entity);
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .bg(k.scrim())
            .occlude()
            .on_mouse_down(gpui_kit::MouseButton::Left, move |_, _, cx| backdrop.update(cx, |this, cx| this.close(cx)))
            .child(
                kairo_ui_kit::motion::enter(v_flex()
                    .id(ids::DIALOG)
                    .test_support()
                    .role(Role::Dialog)
                    .aria_label("Import Song")
                    .w(px(if wide { 1024. } else { 576. }))
                    .when(wide, |d| d.h(px(720.)))
                    .max_h(px(860.))
                    .overflow_hidden()
                    .rounded(radius::xl2())
                    .bg(k.surface_secondary)
                    .border_1()
                    .border_color(k.white.opacity(0.14))
                    .shadow_2xl()
                    .on_mouse_down(gpui_kit::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        h_flex()
                            .justify_between()
                            .px_5()
                            .py_4()
                            .child(
                                v_flex()
                                    .child(div().text_size(text::BASE.size).font_weight(FontWeight::SEMIBOLD).text_color(k.text_primary).child("Import Song"))
                                    .child(div().mt_0p5().text_size(text::XS.size).text_color(k.text(TextStep::T400)).child("Upload song files, paste from your clipboard, or search online")),
                            )
                            .child(Button::new(ids::CLOSE).cursor_pointer().ghost().small().icon(Icon::new(KairoIcon::X)).accessibility_label("Close").on_click(move |_, _, cx| close.update(cx, |this, cx| this.close(cx)))),
                    )
                    .child(self.render_tabs(cx))
                    .child(body)
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2p5()
                            .px_5()
                            .py_3p5()
                            .bg(k.surface_header)
                            .border_t_1()
                            .border_color(k.panel_border)
                            .child(Button::new("lyrics-import-cancel").cursor_pointer().label(if self.tab == Tab::Online { "Close" } else { "Cancel" }).on_click(move |_, _, cx| cancel.update(cx, |this, cx| this.close(cx))))
                            .children(confirm),
                    )
.focus_trap(ids::DIALOG, &self.modal_focus), "lyrics-import-enter", cx),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_sources_follow_the_sheet() {
        assert!(importable("Way Maker.RTF"));
        assert!(importable("Alpha_Omega-1.pro"));
        assert!(!importable("song.docx"));
        assert_eq!(title_from_file_name("way_maker-live.txt"), "way maker live");
        assert_eq!(title_from_file_name(".txt"), "Untitled");
        match source_for_file(
            "x.usr",
            "Title=Amazing Grace\nAuthor=Newton\n\n{V}\nAmazing grace\n",
        )
        .unwrap()
        {
            LyricsImportSource::Usr { filename, .. } => {
                assert_eq!(filename.as_deref(), Some("x.usr"))
            }
            other => panic!("{other:?}"),
        }
        match source_for_file(
            "Blessed Be.txt",
            "Blessed be your name\nIn the land\n\nEvery blessing\n",
        )
        .unwrap()
        {
            LyricsImportSource::Text { title, .. } => assert!(!title.is_empty()),
            other => panic!("{other:?}"),
        }
        assert!(source_for_file("empty.rtf", r"{\rtf1\ansi }").is_err());
    }

    #[test]
    fn clipboard_note_copy() {
        assert_eq!(
            clipboard_note(1, "", ""),
            "Pasted 1 lyric line. Add a title below."
        );
        assert_eq!(
            clipboard_note(4, "Grace", "Newton"),
            "Pasted 4 lyric lines — detected title \"Grace\", artist \"Newton\"."
        );
    }
}
