//! House audio library. From `src/main/services/tracks/index.ts`.
//!
//! The folder sits next to the backgrounds folder (`…/Audio`) unless the
//! operator picked another. Remove moves the file to the OS Trash.

use crate::library::MediaError;
use crate::paths::{
    self, ext_lower, is_inside_root, item_id_for, resolve, stem, taken_names, unique_file_name,
};
use crate::publish::{Publisher, Subscription};
use crate::settings::{SettingsPort, folder_of};
use kairo_domain::media::{
    TrackItem, TracksLibrary, is_track_extension, sibling_audio_folder, track_file_name,
};
use serde_json::json;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_DEPTH: usize = 1;
const GONE: &str = "That track is no longer in the folder.";
const OUTSIDE: &str = "That track is outside the audio folder.";
/// Time the player gets to close a file it holds open before it is moved.
const RELEASE_DELAY: Duration = Duration::from_millis(250);

/// Moves a file to the OS Trash. Swapped out in tests.
pub type Trasher = Box<dyn Fn(&Path) -> io::Result<()> + Send>;

fn os_trash(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::ErrorKind::NotFound.into());
    }
    trash::delete(path).map_err(io::Error::other)
}

fn err<T>(message: impl Into<String>) -> Result<T, MediaError> {
    Err(MediaError(message.into()))
}

pub struct TracksService<S: SettingsPort> {
    settings: S,
    items: Vec<TrackItem>,
    scan_error: Option<String>,
    live_id: Option<String>,
    live_paused: bool,
    trash: Trasher,
    release_delay: Duration,
    publisher: Publisher<TracksLibrary>,
}

impl<S: SettingsPort> TracksService<S> {
    pub fn new(settings: S) -> Self {
        Self::with_trash(settings, Box::new(os_trash), RELEASE_DELAY)
    }

    pub fn with_trash(settings: S, trash: Trasher, release_delay: Duration) -> Self {
        Self {
            settings,
            items: Vec::new(),
            scan_error: None,
            live_id: None,
            live_paused: false,
            trash,
            release_delay,
            publisher: Publisher::default(),
        }
    }

    pub fn subscribe(&mut self) -> Subscription<TracksLibrary> {
        self.publisher.subscribe()
    }

    fn emit(&mut self) {
        let library = self.library();
        self.publisher.publish(&library);
    }

    pub fn library(&self) -> TracksLibrary {
        TracksLibrary {
            folder: self.root(),
            items: self.items.clone(),
            live_id: self.live_id.clone(),
            live_paused: self.live_paused,
            error: self.scan_error.clone(),
        }
    }

    fn root(&self) -> String {
        folder_of(&self.settings, "tracks")
    }

    pub fn allowed_root(&self) -> Option<PathBuf> {
        let root = self.root();
        (!root.is_empty()).then(|| resolve(Path::new(&root)))
    }

    /// With no audio folder set, adopt `…/Audio` beside the backgrounds folder.
    fn adopt_sibling(&mut self) -> String {
        let mut folder = self.root();
        if folder.is_empty() {
            let media = folder_of(&self.settings, "media");
            if !media.is_empty() {
                folder = sibling_audio_folder(&media);
                self.settings.set("tracks", json!({ "folder": folder }));
            }
        }
        folder
    }

    pub fn ensure_folder(&mut self) -> Result<String, MediaError> {
        let folder = self.adopt_sibling();
        if folder.is_empty() {
            return err("Choose a backgrounds folder first — audio is kept next to it.");
        }
        std::fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    pub fn set_folder(&mut self, folder: &str) -> TracksLibrary {
        self.settings.set("tracks", json!({ "folder": folder }));
        self.live_id = None;
        self.live_paused = false;
        self.scan()
    }

    pub fn scan(&mut self) -> TracksLibrary {
        let root = self.adopt_sibling();
        if root.is_empty() {
            self.items.clear();
            self.scan_error = None;
            self.emit();
            return self.library();
        }
        match std::fs::create_dir_all(&root).and_then(|()| scan_tracks_folder(Path::new(&root))) {
            Ok(items) => {
                self.items = items;
                self.scan_error = None;
                if let Some(live) = &self.live_id
                    && !self.items.iter().any(|i| &i.id == live)
                {
                    self.live_id = None;
                    self.live_paused = false;
                }
            }
            Err(error) => {
                self.items.clear();
                self.scan_error = Some(error.to_string());
                tracing::warn!(%error, "tracks scan failed");
            }
        }
        self.emit();
        self.library()
    }

    /// Audio files are copied into the folder; ones already inside it count
    /// as imported without a copy.
    pub fn import_files(&mut self, source_paths: &[PathBuf]) -> Result<TracksLibrary, MediaError> {
        let root = self.ensure_folder()?;
        let mut taken = taken_names(Path::new(&root))?;
        let mut imported = 0;
        for source in source_paths {
            let name = source.to_string_lossy();
            if !is_track_extension(&ext_lower(&name)) {
                continue;
            }
            let abs = resolve(source);
            if is_inside_root(&abs, Path::new(&root)) {
                imported += 1;
                continue;
            }
            let dest_name = unique_file_name(paths::basename(&name), &taken);
            std::fs::copy(&abs, Path::new(&root).join(&dest_name))?;
            taken.insert(dest_name.to_lowercase());
            imported += 1;
            tracing::info!(from = %abs.display(), dest = dest_name, "copied into audio library");
        }
        if imported == 0 {
            return err("None of those files are audio we can play.");
        }
        Ok(self.scan())
    }

    fn checked(&self, id: &str) -> Result<(TrackItem, PathBuf), MediaError> {
        let item = self
            .items
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .ok_or(MediaError(GONE.into()))?;
        match self.allowed_root() {
            Some(root) if is_inside_root(Path::new(&item.path), &root) => Ok((item, root)),
            _ => err(OUTSIDE),
        }
    }

    /// Moves the file to the Trash; undo lives there. A live track is
    /// stopped first and the player given time to let go of the file.
    pub fn remove(&mut self, id: &str) -> Result<TracksLibrary, MediaError> {
        let (item, _) = self.checked(id)?;
        if self.live_id.as_deref() == Some(id) {
            self.live_id = None;
            self.live_paused = false;
            self.emit();
            std::thread::sleep(self.release_delay);
        }
        match (self.trash)(Path::new(&item.path)) {
            Ok(()) => tracing::info!(path = item.path, "track moved to trash"),
            // Already deleted in the file manager: rescan so the row goes.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(self.scan())
    }

    /// Renames the file, keeping its folder and its extension as written.
    pub fn rename(&mut self, id: &str, next_name: &str) -> Result<TracksLibrary, MediaError> {
        let (item, root) = self.checked(id)?;
        let path = PathBuf::from(&item.path);
        let file_ext = paths::extname(&item.path)
            .strip_prefix('.')
            .filter(|e| !e.is_empty())
            .unwrap_or(&item.ext)
            .to_owned();
        let name = track_file_name(next_name, &file_ext);
        if name.is_empty() {
            return err("Give the track a name.");
        }
        let target = path
            .parent()
            .unwrap_or(Path::new(""))
            .join(format!("{name}.{file_ext}"));
        if resolve(&target) == resolve(&path) {
            return Ok(self.library());
        }
        if !is_inside_root(&target, &root) {
            return err("That name cannot be used here.");
        }
        // A case-only rename on a case-insensitive disk finds the same file.
        let case_only = resolve(&target).to_string_lossy().to_lowercase()
            == resolve(&path).to_string_lossy().to_lowercase();
        if !case_only && target.exists() {
            return err(format!(
                "A file called “{name}.{file_ext}” is already in that folder."
            ));
        }
        let was_live = self.live_id.as_deref() == Some(id);
        let was_paused = self.live_paused;
        if was_live {
            // The player holds the file open; let it pause first.
            self.live_paused = true;
            self.emit();
            std::thread::sleep(self.release_delay);
        }
        if let Err(error) = std::fs::rename(&path, &target) {
            if was_live {
                self.live_paused = was_paused;
                self.emit();
            }
            return Err(error.into());
        }
        if was_live {
            self.live_id = Some(item_id_for(&root, &resolve(&target)));
            self.live_paused = was_paused;
        }
        tracing::info!(from = item.path, to = %target.display(), "track renamed");
        Ok(self.scan())
    }

    /// The file to show in Finder or Explorer. The UI opens it.
    pub fn reveal(&self, id: &str) -> Result<PathBuf, MediaError> {
        self.items
            .iter()
            .find(|i| i.id == id)
            .map(|i| PathBuf::from(&i.path))
            .ok_or(MediaError(GONE.into()))
    }

    pub fn play(&mut self, id: &str) -> Result<TracksLibrary, MediaError> {
        if !self.items.iter().any(|i| i.id == id) {
            return err(GONE);
        }
        self.live_id = Some(id.into());
        self.live_paused = false;
        self.emit();
        Ok(self.library())
    }

    pub fn set_paused(&mut self, paused: bool) -> TracksLibrary {
        self.live_paused = paused && self.live_id.is_some();
        self.emit();
        self.library()
    }

    pub fn stop(&mut self) -> TracksLibrary {
        self.live_id = None;
        self.live_paused = false;
        self.emit();
        self.library()
    }
}

/// Audio files in `root` and one level of subfolders, sorted by name with
/// case and accents ignored. A stat failure fails the scan, as in Electron.
pub fn scan_tracks_folder(root: &Path) -> io::Result<Vec<TrackItem>> {
    walk(root, root, 0)
}

fn walk(root: &Path, dir: &Path, depth: usize) -> io::Result<Vec<TrackItem>> {
    let mut items = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let abs = dir.join(&name);
        if entry.file_type()?.is_dir() {
            if depth < MAX_DEPTH {
                items.extend(walk(root, &abs, depth + 1)?);
            }
            continue;
        }
        let ext = ext_lower(&name);
        if !is_track_extension(&ext) {
            continue;
        }
        // `fs.stat` follows symlinks, so a linked audio file is listed.
        let size = std::fs::metadata(&abs)?.len() as f64;
        items.push(TrackItem {
            id: item_id_for(root, &abs),
            name: stem(&name).into(),
            path: abs.to_string_lossy().into_owned(),
            ext,
            size,
        });
    }
    items.sort_by(|a, b| base_compare(&a.name, &b.name));
    Ok(items)
}

/// `localeCompare(…, { sensitivity: 'base' })`: case and accents do not count.
fn base_compare(a: &str, b: &str) -> std::cmp::Ordering {
    let fold = |s: &str| -> String {
        s.chars()
            .flat_map(char::to_lowercase)
            .map(strip_accent)
            .collect()
    };
    paths::locale_compare(&fold(a), &fold(b))
}

/// Latin-1 and Latin Extended-A letters to their base letter.
fn strip_accent(c: char) -> char {
    const FROM: &str =
        "àáâãäåāăąçćĉċčďđèéêëēĕėęěĝğġģĥħìíîïĩīĭįıĵķĺļľŀłñńņňŉòóôõöøōŏőŕŗřśŝşšţťŧùúûüũūŭůűųŵýÿŷźżž";
    const TO: &str =
        "aaaaaaaaacccccddeeeeeeeeegggghhiiiiiiiiijklllllnnnnnooooooooorrrsssstttuuuuuuuuuuwyyyzzz";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|i| TO.chars().nth(i))
        .unwrap_or(c)
}
