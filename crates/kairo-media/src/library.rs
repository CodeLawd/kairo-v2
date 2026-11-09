//! Folder-backed background library. From `src/main/services/media/index.ts`.
//!
//! The folder is the library: a scan reads directory entries and sizes only.
//! Playlists, per-file playback grades and the operator's order live in
//! `<folder>/.kairo/playlists.json`, so they travel with the media.

use crate::paths::{
    self, basename, clean_name, ext_lower, is_inside_root, item_id_for, resolve, stem, taken_names,
    unique_file_name,
};
use crate::publish::{Publisher, Subscription};
use crate::settings::{SettingsPort, folder_of};
use kairo_domain::media::{
    MediaFolder, MediaItem, MediaKind, MediaLibrary, MediaPlayback, MediaPlaylist,
    is_default_media_playback, normalize_media_playback, normalize_playback_map,
};
use kairo_domain::media_order::apply_item_order;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

/// Containers the scan lists as video. `.mov` stays even though ProRes inside
/// it may not decode: hiding it would look like the scan missed files.
pub const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "webm", "m4v", "mov", "ogv"];
pub const IMAGE_EXTENSIONS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "gif", "avif", "bmp"];
/// Logos and props also accept vector art.
pub const LOGO_IMAGE_EXTENSIONS: [&str; 8] =
    ["png", "jpg", "jpeg", "webp", "gif", "avif", "bmp", "svg"];

/// File-picker extensions, in lockstep with what the scan accepts.
pub fn media_file_extensions() -> Vec<&'static str> {
    VIDEO_EXTENSIONS
        .iter()
        .chain(&IMAGE_EXTENSIONS)
        .copied()
        .collect()
}

/// Subfolders are one level deep: a media folder is a shelf, not a tree.
const MAX_DEPTH: usize = 1;
const MANIFEST_DIR: &str = ".kairo";
/// Where playlists lived before the rename; read once, never written again.
const LEGACY_MANIFEST_DIR: &str = ".proautomate";
const MANIFEST_FILE: &str = "playlists.json";
const MANIFEST_VERSION: u32 = 1;

pub fn kind_of(ext: &str) -> Option<MediaKind> {
    if VIDEO_EXTENSIONS.contains(&ext) {
        Some(MediaKind::Video)
    } else if IMAGE_EXTENSIONS.contains(&ext) {
        Some(MediaKind::Image)
    } else {
        None
    }
}

/// A message for the operator, worded as Electron words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaError(pub String);

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MediaError {}

impl From<io::Error> for MediaError {
    fn from(error: io::Error) -> Self {
        Self(error.to_string())
    }
}

fn err<T>(message: impl Into<String>) -> Result<T, MediaError> {
    Err(MediaError(message.into()))
}

const NO_FOLDER: &str = "Choose a backgrounds folder first.";
const GONE: &str = "That background is no longer in the folder.";

// ─── Pure helpers ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaImportPlan {
    Reuse(PathBuf),
    Copy { from: PathBuf, dest_name: String },
    Skip,
}

/// Files already under the root are reused. Anything else is copied in, never
/// moved, so ProPresenter's path references to the original stay intact.
pub fn plan_media_import(source: &Path, root: &Path, taken: &HashSet<String>) -> MediaImportPlan {
    if kind_of(&ext_lower(&source.to_string_lossy())).is_none() {
        return MediaImportPlan::Skip;
    }
    let abs = resolve(source);
    if is_inside_root(&abs, root) {
        return MediaImportPlan::Reuse(abs);
    }
    copy_plan(abs, taken)
}

/// Paste always writes a new file under the root, even for a library file
/// (Copy then Paste duplicates it as `name 1.ext`).
pub fn plan_media_paste(source: &Path, taken: &HashSet<String>) -> MediaImportPlan {
    if kind_of(&ext_lower(&source.to_string_lossy())).is_none() {
        return MediaImportPlan::Skip;
    }
    copy_plan(resolve(source), taken)
}

fn copy_plan(abs: PathBuf, taken: &HashSet<String>) -> MediaImportPlan {
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    MediaImportPlan::Copy {
        dest_name: unique_file_name(&name, taken),
        from: abs,
    }
}

/// One-level shelves under the root, empty ones included. Root first, then by name.
pub fn list_media_folders(
    root_name: &str,
    subdirectory_names: &[String],
    file_counts: &[(String, usize)],
) -> Vec<MediaFolder> {
    let mut counts: Vec<(String, usize)> = file_counts.to_vec();
    for id in subdirectory_names {
        if !id.is_empty() && !counts.iter().any(|(k, _)| k == id) {
            counts.push((id.clone(), 0));
        }
    }
    let mut folders: Vec<MediaFolder> = counts
        .into_iter()
        .map(|(id, count)| MediaFolder {
            name: if id.is_empty() {
                root_name.into()
            } else {
                id.clone()
            },
            id,
            count: count as f64,
        })
        .collect();
    folders.sort_by(|a, b| match (a.id.is_empty(), b.id.is_empty()) {
        (true, _) => std::cmp::Ordering::Less,
        (_, true) => std::cmp::Ordering::Greater,
        _ => paths::locale_compare(&a.name, &b.name),
    });
    folders
}

/// Re-points entries whose file moved between subfolders. An entry heals only
/// when exactly one known file has its basename; anything else is left in
/// place so the dock can report it missing.
pub fn repair_playlist_item_ids(
    playlists: &[MediaPlaylist],
    known_ids: &[String],
) -> (Vec<MediaPlaylist>, usize) {
    let known: HashSet<&str> = known_ids.iter().map(String::as_str).collect();
    let mut by_basename: HashMap<&str, Vec<&str>> = HashMap::new();
    for id in known_ids {
        by_basename.entry(basename(id)).or_default().push(id);
    }
    let mut healed = 0;
    let next = playlists
        .iter()
        .map(|playlist| {
            let mut playlist = playlist.clone();
            for id in &mut playlist.item_ids {
                if known.contains(id.as_str()) {
                    continue;
                }
                if let Some([only]) = by_basename.get(basename(id)).map(Vec::as_slice) {
                    *id = (*only).to_owned();
                    healed += 1;
                }
            }
            playlist
        })
        .collect();
    (next, healed)
}

/// `isPlaylist`, leniently: `id` and `name` strings and an `itemIds` array.
/// Non-string ids are dropped; a missing `createdAt` reads as 0.
fn parse_playlist(value: &Value) -> Option<MediaPlaylist> {
    let object = value.as_object()?;
    let id = object.get("id")?.as_str()?.to_owned();
    let name = object.get("name")?.as_str()?.to_owned();
    let item_ids = object
        .get("itemIds")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let created_at = object
        .get("createdAt")
        .and_then(Value::as_f64)
        .unwrap_or(0.0);
    let extra = object
        .iter()
        .filter(|(k, _)| !matches!(k.as_str(), "id" | "name" | "itemIds" | "createdAt"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Some(MediaPlaylist {
        id,
        name,
        item_ids,
        created_at,
        extra,
    })
}

fn parse_playlists(value: Option<&Value>) -> Vec<MediaPlaylist> {
    value
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(parse_playlist).collect())
        .unwrap_or_default()
}

/// The pre-manifest id: first 16 hex digits of SHA-1 over the resolved path.
pub fn legacy_item_id(abs_path: &Path) -> String {
    use sha1::{Digest, Sha1};
    let digest = Sha1::digest(resolve(abs_path).to_string_lossy().as_bytes());
    digest
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..16]
        .to_owned()
}

fn new_playlist_id() -> String {
    use std::hash::{BuildHasher, RandomState};
    let random = RandomState::new().hash_one(std::time::SystemTime::now());
    let suffix: String = to_base36(random).chars().take(5).collect();
    format!("pl-{}-{suffix}", to_base36(now_ms() as u64))
}

fn to_base36(mut n: u64) -> String {
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

// ─── Scanning ───────────────────────────────────────────────────────────────

/// What one walk of the folder found. Built off the service so a large
/// folder never blocks it; hand it to [`MediaService::apply_scan`].
#[derive(Debug)]
pub struct MediaScan {
    pub root: String,
    pub result: Result<ScannedFolder, String>,
}

#[derive(Debug, Default)]
pub struct ScannedFolder {
    pub items: Vec<MediaItem>,
    pub subdirectory_names: Vec<String>,
    pub counts: Vec<(String, usize)>,
}

/// Index `root`: entries and sizes only, never file contents. Dot entries
/// (the manifest folder among them), symlinks and unknown types are skipped.
pub fn scan_media_folder(root: &str) -> MediaScan {
    let mut folder = ScannedFolder::default();
    let result = walk(Path::new(root), root, "", 0, &mut folder).map(|()| {
        // Stable, so names that collate equal keep directory order.
        folder
            .items
            .sort_by(|a, b| paths::locale_compare(&a.name, &b.name));
        folder
    });
    MediaScan {
        root: root.into(),
        result: result.map_err(|e| e.to_string()),
    }
}

fn walk(
    dir: &Path,
    root: &str,
    relative_dir: &str,
    depth: usize,
    out: &mut ScannedFolder,
) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let full = dir.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if depth >= MAX_DEPTH {
                continue;
            }
            // Registered before walking: an empty folder is still a shelf.
            if depth == 0 {
                out.subdirectory_names.push(name.clone());
            }
            walk(&full, root, &name, depth + 1, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let ext = ext_lower(&name);
        let Some(kind) = kind_of(&ext) else { continue };
        // A file that vanished between listing and stat is skipped.
        let Ok(meta) = std::fs::metadata(&full) else {
            continue;
        };
        out.items.push(MediaItem {
            id: item_id_for(Path::new(root), &full),
            name: stem(&name).into(),
            path: full.to_string_lossy().into_owned(),
            kind,
            ext,
            folder: relative_dir.into(),
            size_bytes: meta.len() as f64,
        });
        match out.counts.iter_mut().find(|(k, _)| k == relative_dir) {
            Some((_, n)) => *n += 1,
            None => out.counts.push((relative_dir.into(), 1)),
        }
    }
    Ok(())
}

// ─── Service ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipboardMode {
    Copy,
    Cut,
}

/// Reads the system file clipboard. Swapped out in tests.
pub type ClipboardReader = Box<dyn Fn() -> Vec<PathBuf> + Send>;

pub struct MediaService<S: SettingsPort> {
    settings: S,
    items: Vec<MediaItem>,
    folders: Vec<MediaFolder>,
    playlists: Vec<MediaPlaylist>,
    playback: BTreeMap<String, MediaPlayback>,
    /// Operator order for "All backgrounds". Empty means scan order.
    item_order: Vec<String>,
    scan_error: Option<String>,
    live_item_id: Option<String>,
    live_paused: bool,
    live_playback: Option<MediaPlayback>,
    live_cue: f64,
    /// In-app clipboard; Paste prefers it over the system pasteboard.
    clipboard_ids: Option<Vec<String>>,
    clipboard_mode: Option<ClipboardMode>,
    /// Playlist the cut items came from; cleared on Paste and Copy.
    clipboard_cut_from: Option<String>,
    read_clipboard: ClipboardReader,
    publisher: Publisher<MediaLibrary>,
}

impl<S: SettingsPort> MediaService<S> {
    pub fn new(settings: S) -> Self {
        Self::with_clipboard(settings, Box::new(crate::clipboard::read_file_paths))
    }

    pub fn with_clipboard(settings: S, read_clipboard: ClipboardReader) -> Self {
        Self {
            settings,
            items: Vec::new(),
            folders: Vec::new(),
            playlists: Vec::new(),
            playback: BTreeMap::new(),
            item_order: Vec::new(),
            scan_error: None,
            live_item_id: None,
            live_paused: false,
            live_playback: None,
            live_cue: 0.0,
            clipboard_ids: None,
            clipboard_mode: None,
            clipboard_cut_from: None,
            read_clipboard,
            publisher: Publisher::default(),
        }
    }

    /// Snapshots after every library or live change, coalesced.
    pub fn subscribe(&mut self) -> Subscription<MediaLibrary> {
        self.publisher.subscribe()
    }

    fn emit(&mut self) {
        let library = self.library();
        self.publisher.publish(&library);
    }

    pub fn library(&self) -> MediaLibrary {
        MediaLibrary {
            folder: self.root(),
            folders: self.folders.clone(),
            items: self.items.clone(),
            playlists: self.playlists.clone(),
            playback: self.playback.clone(),
            live_item_id: self.live_item_id.clone(),
            live_playback: self.live_playback(),
            live_cue: Some(self.live_cue),
            live_paused: self.live_paused,
            error: self.scan_error.clone(),
        }
    }

    fn root(&self) -> String {
        folder_of(&self.settings, "media")
    }

    fn require_root(&self) -> Result<String, MediaError> {
        let root = self.root();
        if root.is_empty() {
            err(NO_FOLDER)
        } else {
            Ok(root)
        }
    }

    /// `readSettings()`: `media` with only `folder` and valid playlists.
    fn media_settings(&self) -> (String, Vec<Value>) {
        let raw = self.settings.get("media");
        let playlists = raw
            .as_ref()
            .and_then(|v| v.get("playlists"))
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter(|p| parse_playlist(p).is_some())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        (self.root(), playlists)
    }

    fn write_media_settings(&mut self, folder: &str, playlists: Vec<Value>) {
        self.settings
            .set("media", json!({ "folder": folder, "playlists": playlists }));
    }

    /// Scan on the calling thread. To keep a caller responsive, run
    /// [`scan_media_folder`] elsewhere and pass its result to [`Self::apply_scan`].
    pub fn scan(&mut self) -> MediaLibrary {
        let root = self.root();
        if root.is_empty() {
            return self.apply_scan(MediaScan {
                root,
                result: Ok(ScannedFolder::default()),
            });
        }
        self.apply_scan(scan_media_folder(&root))
    }

    /// The folder a scan should walk, or empty when none is set.
    pub fn scan_root(&self) -> String {
        self.root()
    }

    pub fn apply_scan(&mut self, scan: MediaScan) -> MediaLibrary {
        let root = self.root();
        if root.is_empty() {
            self.items.clear();
            self.folders.clear();
            self.playlists.clear();
            self.playback.clear();
            self.item_order.clear();
            self.scan_error = None;
            self.emit();
            return self.library();
        }
        if scan.root != root {
            // The folder changed while this scan ran; a newer one follows.
            return self.library();
        }
        let scanned = match scan.result {
            Ok(scanned) => scanned,
            Err(message) => {
                // A moved or unmounted folder must not wipe the dock silently.
                self.scan_error =
                    Some(format!("Could not read the backgrounds folder — {message}"));
                tracing::warn!(root, error = ?self.scan_error, "media scan failed");
                self.items.clear();
                self.folders.clear();
                self.emit();
                return self.library();
            }
        };
        self.scan_error = None;
        let root_name = basename(&root).to_owned();
        self.folders = list_media_folders(&root_name, &scanned.subdirectory_names, &scanned.counts);
        // The manifest holds the library order; load it before applying so a
        // rescan never shows the alphabetical list first.
        self.load_playlists(&root, &scanned.items);
        self.items = apply_item_order(&scanned.items, &self.item_order, |i| &i.id)
            .into_iter()
            .cloned()
            .collect();
        // A background that disappeared from disk is no longer on screen.
        if let Some(live) = &self.live_item_id
            && !self.items.iter().any(|i| &i.id == live)
        {
            self.live_item_id = None;
            self.live_paused = false;
        }
        tracing::info!(
            root,
            items = self.items.len(),
            folders = self.folders.len(),
            playlists = self.playlists.len(),
            "media scanned"
        );
        self.emit();
        self.library()
    }

    // ─── Playlist manifest ──────────────────────────────────────────────────

    fn load_playlists(&mut self, root: &str, scanned: &[MediaItem]) {
        let read = std::fs::read_to_string(manifest_path(root)).or_else(|_| {
            std::fs::read_to_string(
                Path::new(root)
                    .join(LEGACY_MANIFEST_DIR)
                    .join(MANIFEST_FILE),
            )
        });
        match read {
            Ok(text) => {
                let Ok(Value::Object(parsed)) = serde_json::from_str::<Value>(&text) else {
                    tracing::warn!("could not read the media playlists manifest");
                    self.reset_manifest_state();
                    return;
                };
                self.playlists = parse_playlists(parsed.get("playlists"));
                self.playback =
                    normalize_playback_map(parsed.get("playback").unwrap_or(&Value::Null));
                self.item_order = parsed
                    .get("itemOrder")
                    .and_then(Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(Value::as_str)
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                // Persist the repair, or the same entries re-match every launch.
                let known: Vec<String> = scanned.iter().map(|i| i.id.clone()).collect();
                let (playlists, healed) = repair_playlist_item_ids(&self.playlists, &known);
                self.playlists = playlists;
                if healed > 0 {
                    tracing::info!(healed, "re-pointed moved playlist entries");
                    let _ = self.save_playlists();
                }
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                tracing::warn!(%error, "could not read the media playlists manifest");
                self.reset_manifest_state();
            }
            Err(_) => self.migrate_settings_playlists(scanned),
        }
    }

    fn reset_manifest_state(&mut self) {
        self.playlists.clear();
        self.playback.clear();
        self.item_order.clear();
    }

    /// No manifest yet: carry settings-stored playlists over once, mapping
    /// their absolute-path hashes to relative ids, then clear them in settings.
    fn migrate_settings_playlists(&mut self, scanned: &[MediaItem]) {
        let (folder, legacy) = self.media_settings();
        self.reset_manifest_state();
        if legacy.is_empty() {
            return;
        }
        let by_legacy_id: HashMap<String, &str> = scanned
            .iter()
            .map(|i| (legacy_item_id(Path::new(&i.path)), i.id.as_str()))
            .collect();
        self.playlists = legacy
            .iter()
            .filter_map(parse_playlist)
            .map(|mut p| {
                p.item_ids = p
                    .item_ids
                    .iter()
                    .filter_map(|old| by_legacy_id.get(old).map(|id| (*id).to_owned()))
                    .collect();
                p
            })
            .collect();
        tracing::info!(
            count = self.playlists.len(),
            "migrated playlists into the media folder"
        );
        let _ = self.save_playlists();
        self.write_media_settings(&folder, Vec::new());
    }

    /// Write the manifest. On failure the in-memory edit is kept and the
    /// error is shown on the dock and returned.
    fn save_playlists(&mut self) -> Option<String> {
        let root = self.root();
        if root.is_empty() {
            return None;
        }
        let manifest = json!({
            "version": MANIFEST_VERSION,
            "playlists": self.playlists,
            "playback": self.playback,
            "itemOrder": self.item_order,
        });
        // JSON.stringify(…, null, 2): tabs never appear inside strings, which escape them.
        let text = format!(
            "{}\n",
            kairo_store::json_text::stringify_tabs(&manifest).replace('\t', "  ")
        );
        let result = std::fs::create_dir_all(Path::new(&root).join(MANIFEST_DIR)).and_then(|()| {
            kairo_store::fs_atomic::write_atomic(&manifest_path(&root), text.as_bytes())
        });
        match result {
            Ok(()) => None,
            Err(error) => {
                let message = format!("Playlists could not be saved to the media folder — {error}");
                tracing::warn!(%error, "could not write the media playlists manifest");
                self.scan_error = Some(message.clone());
                Some(message)
            }
        }
    }

    // ─── Folder and item operations ─────────────────────────────────────────

    pub fn set_folder(&mut self, folder: &str) -> MediaLibrary {
        let (_, playlists) = self.media_settings();
        self.write_media_settings(folder, playlists);
        self.live_item_id = None;
        self.live_paused = false;
        self.scan()
    }

    /// Creates an empty subfolder. The app never moves the operator's files
    /// into it: that would break ProPresenter's references to them.
    pub fn create_folder(&mut self, name: &str) -> Result<MediaLibrary, MediaError> {
        let root = self.require_root()?;
        let clean = clean_name(name);
        if clean.is_empty() || clean.starts_with('.') {
            return err("That folder name is not allowed.");
        }
        let target = resolve(&Path::new(&root).join(&clean));
        if !is_inside_root(&target, Path::new(&root))
            || target.parent() != Some(resolve(Path::new(&root)).as_path())
        {
            return err("That folder name is not allowed.");
        }
        std::fs::create_dir(&target).map_err(|e| {
            MediaError(if e.kind() == io::ErrorKind::AlreadyExists {
                "That folder already exists.".into()
            } else {
                e.to_string()
            })
        })?;
        tracing::info!(name = clean, "media folder created");
        Ok(self.scan())
    }

    pub fn item(&self, id: &str) -> Option<&MediaItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// Deletes a background from disk (unlinked, not trashed, as in Electron)
    /// and drops every reference to it. A missing file still clears references.
    pub fn delete_item(&mut self, id: &str) -> Result<MediaLibrary, MediaError> {
        let item = self.item(id).cloned().ok_or(MediaError(GONE.into()))?;
        let root = self.require_root()?;
        let target = resolve(Path::new(&item.path));
        if !is_inside_root(&target, Path::new(&root)) {
            return err("That file is outside the backgrounds folder.");
        }
        if let Err(error) = std::fs::remove_file(&target)
            && error.kind() != io::ErrorKind::NotFound
        {
            return err(format!("Could not delete “{}” — {error}", item.name));
        }
        self.items.retain(|i| i.id != id);
        self.item_order.retain(|i| i != id);
        for playlist in &mut self.playlists {
            playlist.item_ids.retain(|i| i != id);
        }
        self.playback.remove(id);
        if self.live_item_id.as_deref() == Some(id) {
            self.live_item_id = None;
            self.live_paused = false;
        }
        // Counts derive from the items; empty shelves stay listed at zero.
        for folder in &mut self.folders {
            folder.count = self.items.iter().filter(|i| i.folder == folder.id).count() as f64;
        }
        self.save_playlists();
        tracing::info!(id, name = item.name, "background deleted");
        self.emit();
        Ok(self.library())
    }

    /// Renames a file on disk, keeping its extension, and moves every
    /// reference (playlists, order, grade, live) to the new id.
    pub fn rename_item(&mut self, id: &str, next_name: &str) -> Result<MediaLibrary, MediaError> {
        let item = self.item(id).cloned().ok_or(MediaError(GONE.into()))?;
        let root = self.require_root()?;
        let clean = clean_name(next_name);
        if clean.is_empty() || clean.starts_with('.') {
            return err("That name is not allowed.");
        }
        let suffix = format!(".{}", item.ext);
        let stem = if clean.to_lowercase().ends_with(&suffix) {
            clean[..clean.len() - suffix.len()].to_owned()
        } else {
            clean
        };
        if stem.is_empty() {
            return err("That name is not allowed.");
        }
        let source = PathBuf::from(&item.path);
        let dest_dir = source.parent().unwrap_or(Path::new("")).to_path_buf();
        let dest = dest_dir.join(format!("{stem}.{}", item.ext));
        if resolve(&dest) == resolve(&source) {
            return Ok(self.library());
        }
        if !is_inside_root(&dest, Path::new(&root))
            || resolve(&dest).parent() != Some(resolve(&dest_dir).as_path())
        {
            return err("That name is not allowed.");
        }
        if dest.exists() {
            return err(format!("“{stem}” already exists in this folder."));
        }
        std::fs::rename(&source, &dest)?;
        let new_id = item_id_for(Path::new(&root), &dest);
        self.retarget(
            id,
            MediaItem {
                id: new_id.clone(),
                name: stem,
                path: dest.to_string_lossy().into_owned(),
                ..item
            },
        );
        self.save_playlists();
        tracing::info!(from = id, to = new_id, "background renamed");
        self.emit();
        Ok(self.library())
    }

    /// The file to show in Finder or Explorer. The UI opens it.
    pub fn reveal_item(&self, id: &str) -> Result<PathBuf, MediaError> {
        self.item(id)
            .map(|i| PathBuf::from(&i.path))
            .ok_or(MediaError(GONE.into()))
    }

    /// Remembers backgrounds for in-app Paste. The system pasteboard is not
    /// written, as in Electron; Finder-to-app paste reads it in `paste_items`.
    pub fn copy_items(&mut self, ids: &[String]) -> Result<(), MediaError> {
        if !ids.iter().any(|id| self.item(id).is_some()) {
            return err("Nothing to copy.");
        }
        self.clipboard_ids = Some(ids.to_vec());
        self.clipboard_mode = Some(ClipboardMode::Copy);
        self.clipboard_cut_from = None;
        Ok(())
    }

    /// Marks backgrounds to move on Paste. With a known `from_playlist_id`,
    /// Paste also takes them out of that playlist.
    pub fn cut_items(
        &mut self,
        ids: &[String],
        from_playlist_id: Option<&str>,
    ) -> Result<(), MediaError> {
        if !ids.iter().any(|id| self.item(id).is_some()) {
            return err("Nothing to cut.");
        }
        self.clipboard_ids = Some(ids.to_vec());
        self.clipboard_mode = Some(ClipboardMode::Cut);
        self.clipboard_cut_from = from_playlist_id
            .filter(|p| self.playlists.iter().any(|pl| pl.id == *p))
            .map(str::to_owned);
        Ok(())
    }

    pub fn clipboard_has_files(&self) -> bool {
        self.clipboard_ids
            .as_ref()
            .is_some_and(|ids| !ids.is_empty())
            || !(self.read_clipboard)().is_empty()
    }

    /// Paste into the root, and into `playlist_id` when given.
    /// Copy duplicates under a free name. Cut moves: subfolder files go up to
    /// the root, and the entries move to the end of the playlist or library order.
    pub fn paste_items(&mut self, playlist_id: Option<&str>) -> Result<MediaLibrary, MediaError> {
        let root = self.require_root()?;
        let mode = self.clipboard_mode;
        let ids = self.clipboard_ids.clone().filter(|ids| !ids.is_empty());

        if let (Some(ClipboardMode::Cut), Some(ids)) = (mode, &ids) {
            let cut_from = self.clipboard_cut_from.take();
            self.clipboard_ids = None;
            self.clipboard_mode = None;
            let root_path = resolve(Path::new(&root));
            let mut moved = Vec::new();
            for id in ids {
                let Some(item) = self.item(id).cloned() else {
                    continue;
                };
                let path = PathBuf::from(&item.path);
                if resolve(&path).parent() == Some(root_path.as_path())
                    || !is_inside_root(&path, &root_path)
                {
                    moved.push(id.clone());
                    continue;
                }
                let taken = taken_names(Path::new(&root))?;
                let file_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dest_name = unique_file_name(&file_name, &taken);
                let dest = Path::new(&root).join(&dest_name);
                std::fs::rename(&path, &dest)?;
                let new_id = item_id_for(Path::new(&root), &dest);
                self.retarget(
                    id,
                    MediaItem {
                        id: new_id.clone(),
                        name: stem(&dest_name).into(),
                        path: dest.to_string_lossy().into_owned(),
                        folder: String::new(),
                        ..item
                    },
                );
                moved.push(new_id);
            }
            if moved.is_empty() {
                return err("Nothing to paste.");
            }
            if let Some(from) = cut_from.as_deref()
                && Some(from) != playlist_id
            {
                self.remove_from_playlist(from, &moved);
            }
            if let Some(target) = playlist_id {
                self.remove_from_playlist(target, &moved);
                self.append_to_playlist(target, &moved);
            } else {
                self.item_order.retain(|id| !moved.contains(id));
                self.item_order.extend(moved);
            }
            self.save_playlists();
            return Ok(self.scan());
        }

        let mut sources = Vec::new();
        let copying = mode == Some(ClipboardMode::Copy) && ids.is_some();
        if copying {
            for id in ids.iter().flatten() {
                if let Some(item) = self.item(id) {
                    sources.push(PathBuf::from(&item.path));
                }
            }
            // Copy stays on the clipboard so Paste can repeat.
        } else {
            sources = (self.read_clipboard)();
            self.clipboard_ids = None;
            self.clipboard_mode = None;
            self.clipboard_cut_from = None;
        }
        if sources.is_empty() {
            return err("Nothing to paste.");
        }

        let mut taken = taken_names(Path::new(&root))?;
        let mut pasted = Vec::new();
        for source in &sources {
            let MediaImportPlan::Copy { from, dest_name } = plan_media_paste(source, &taken) else {
                continue;
            };
            let dest = Path::new(&root).join(&dest_name);
            std::fs::copy(&from, &dest)?;
            taken.insert(dest_name.to_lowercase());
            pasted.push(item_id_for(Path::new(&root), &dest));
        }
        if pasted.is_empty() {
            return err("Nothing to paste.");
        }

        // Saved before the scan, which reloads the manifest from disk.
        self.item_order.retain(|id| !pasted.contains(id));
        let anchor = ids
            .as_ref()
            .filter(|_| copying)
            .and_then(|ids| ids.first())
            .and_then(|anchor| self.item_order.iter().position(|id| id == anchor));
        match anchor {
            Some(at) => {
                let tail = self.item_order.split_off(at + 1);
                self.item_order.extend(pasted.iter().cloned());
                self.item_order.extend(tail);
            }
            None => self.item_order.extend(pasted.iter().cloned()),
        }
        if let Some(target) = playlist_id {
            self.append_to_playlist(target, &pasted);
        }
        self.save_playlists();
        Ok(self.scan())
    }

    fn append_to_playlist(&mut self, playlist_id: &str, item_ids: &[String]) {
        let Some(playlist) = self.playlists.iter_mut().find(|p| p.id == playlist_id) else {
            return;
        };
        for id in item_ids {
            if !playlist.item_ids.contains(id) {
                playlist.item_ids.push(id.clone());
            }
        }
    }

    fn remove_from_playlist(&mut self, playlist_id: &str, item_ids: &[String]) {
        for playlist in self.playlists.iter_mut().filter(|p| p.id == playlist_id) {
            playlist.item_ids.retain(|id| !item_ids.contains(id));
        }
    }

    /// Rewires every stored reference when a file's relative id changes.
    fn retarget(&mut self, old_id: &str, next: MediaItem) {
        let new_id = next.id.clone();
        for item in self.items.iter_mut().filter(|i| i.id == old_id) {
            *item = next.clone();
        }
        let swap = |id: &mut String| {
            if id == old_id {
                *id = new_id.clone();
            }
        };
        self.item_order.iter_mut().for_each(swap);
        for playlist in &mut self.playlists {
            playlist.item_ids.iter_mut().for_each(swap);
        }
        if let Some(grade) = self.playback.remove(old_id) {
            self.playback.insert(new_id.clone(), grade);
        }
        if let Some(live) = self.live_item_id.as_mut() {
            swap(live);
        }
    }

    // ─── Live item and playback ─────────────────────────────────────────────

    pub fn live_item(&self) -> Option<&MediaItem> {
        self.live_item_id.as_deref().and_then(|id| self.item(id))
    }

    pub fn playback(&self, id: &str) -> MediaPlayback {
        playback_value(self.playback.get(id))
            .map(|v| normalize_media_playback(&v))
            .unwrap_or_default()
    }

    pub fn live_playback(&self) -> Option<MediaPlayback> {
        self.live_item_id.as_ref().and(self.live_playback.clone())
    }

    pub fn live_cue(&self) -> f64 {
        self.live_cue
    }

    /// Merges `patch` (a `Partial<MediaPlayback>` object) into the file's
    /// grade. A default grade is removed from the manifest. With `next_cue`
    /// the live grade is left for the next push, and a save failure is an error.
    pub fn set_playback(
        &mut self,
        id: &str,
        patch: &Map<String, Value>,
        next_cue: bool,
    ) -> Result<MediaLibrary, MediaError> {
        let next =
            normalize_media_playback(&merge(playback_value(Some(&self.playback(id))), patch));
        if is_default_media_playback(&next) {
            self.playback.remove(id);
        } else {
            self.playback.insert(id.into(), next);
        }
        if self.live_item_id.as_deref() == Some(id) && !next_cue {
            self.live_playback = Some(normalize_media_playback(&merge(
                playback_value(self.live_playback.as_ref()),
                patch,
            )));
        }
        let save_error = self.save_playlists();
        self.emit();
        if let (true, Some(message)) = (next_cue, save_error) {
            return err(message);
        }
        Ok(self.library())
    }

    pub fn set_live_item(&mut self, id: Option<&str>) {
        self.live_item_id = id.map(str::to_owned);
        self.live_playback = id.map(|id| self.playback(id));
        self.live_cue += 1.0;
        self.live_paused = false;
        self.emit();
    }

    /// Pause or resume the clip on screen. Only a live video can pause; the
    /// state is not persisted, so the next push always plays.
    pub fn set_live_paused(&mut self, paused: bool) -> MediaLibrary {
        self.live_paused = paused && self.live_item().is_some_and(|i| i.kind == MediaKind::Video);
        self.emit();
        self.library()
    }

    /// The folder the media protocol may serve. A root, not a file list, so a
    /// file added after the last scan still plays.
    pub fn allowed_root(&self) -> Option<PathBuf> {
        let root = self.root();
        (!root.is_empty()).then(|| resolve(Path::new(&root)))
    }

    // ─── Playlists ──────────────────────────────────────────────────────────

    pub fn create_playlist(&mut self, name: &str) -> MediaLibrary {
        let name = kairo_domain::js::trim(name);
        self.playlists.push(MediaPlaylist {
            id: new_playlist_id(),
            name: if name.is_empty() {
                "New playlist".into()
            } else {
                name.into()
            },
            item_ids: Vec::new(),
            created_at: now_ms(),
            extra: Default::default(),
        });
        self.saved()
    }

    pub fn rename_playlist(&mut self, id: &str, name: &str) -> MediaLibrary {
        let name = kairo_domain::js::trim(name);
        for playlist in self.playlists.iter_mut().filter(|p| p.id == id) {
            if !name.is_empty() {
                playlist.name = name.into();
            }
        }
        self.saved()
    }

    pub fn delete_playlist(&mut self, id: &str) -> MediaLibrary {
        self.playlists.retain(|p| p.id != id);
        self.saved()
    }

    pub fn set_playlist_items(&mut self, id: &str, item_ids: Vec<String>) -> MediaLibrary {
        for playlist in self.playlists.iter_mut().filter(|p| p.id == id) {
            playlist.item_ids = item_ids.clone();
        }
        self.saved()
    }

    /// Saves the order for "All backgrounds". Files not in it still appear,
    /// after the saved ones, so rearranging never hides a drop-in.
    pub fn set_item_order(&mut self, item_ids: Vec<String>) -> MediaLibrary {
        self.item_order = item_ids.into_iter().filter(|id| !id.is_empty()).collect();
        self.items = apply_item_order(&self.items, &self.item_order, |i| &i.id)
            .into_iter()
            .cloned()
            .collect();
        self.saved()
    }

    fn saved(&mut self) -> MediaLibrary {
        self.save_playlists();
        self.emit();
        self.library()
    }

    /// Adds files to a playlist (or only the library when `None`). Files in
    /// the folder are referenced in place; others are copied into the root.
    pub fn import_into_playlist(
        &mut self,
        playlist_id: Option<&str>,
        source_paths: &[PathBuf],
    ) -> Result<MediaLibrary, MediaError> {
        const PLAYLIST_GONE: &str = "That playlist is gone.";
        let root = self.require_root()?;
        if let Some(id) = playlist_id
            && !self.playlists.iter().any(|p| p.id == id)
        {
            return err(PLAYLIST_GONE);
        }
        let mut taken = taken_names(Path::new(&root))?;
        let mut imported = Vec::new();
        for source in source_paths {
            let dest = match plan_media_import(source, Path::new(&root), &taken) {
                MediaImportPlan::Skip => continue,
                MediaImportPlan::Reuse(path) => path,
                MediaImportPlan::Copy { from, dest_name } => {
                    let dest = Path::new(&root).join(&dest_name);
                    std::fs::copy(&from, &dest)?;
                    taken.insert(dest_name.to_lowercase());
                    tracing::info!(from = %from.display(), dest = %dest.display(), "copied into media library");
                    dest
                }
            };
            imported.push(item_id_for(&resolve(Path::new(&root)), &dest));
        }
        if imported.is_empty() {
            return err("None of those files are a background we can play.");
        }
        self.scan();
        let Some(id) = playlist_id else {
            return Ok(self.library());
        };
        let Some(playlist) = self.playlists.iter().find(|p| p.id == id) else {
            return err(PLAYLIST_GONE);
        };
        let mut item_ids = playlist.item_ids.clone();
        for new_id in imported {
            if !item_ids.contains(&new_id) {
                item_ids.push(new_id);
            }
        }
        Ok(self.set_playlist_items(id, item_ids))
    }
}

fn manifest_path(root: &str) -> PathBuf {
    Path::new(root).join(MANIFEST_DIR).join(MANIFEST_FILE)
}

fn playback_value(playback: Option<&MediaPlayback>) -> Option<Value> {
    playback.and_then(|p| serde_json::to_value(p).ok())
}

/// `{ ...base, ...patch }`.
fn merge(base: Option<Value>, patch: &Map<String, Value>) -> Value {
    let mut object = match base {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    for (key, value) in patch {
        object.insert(key.clone(), value.clone());
    }
    Value::Object(object)
}
