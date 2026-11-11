//! The on-disk workspace: one folder holding `Songs/` (one
//! `<slug>__<id>.song.json` per song, the library's source of truth) and
//! `Media/` (backgrounds the media dock indexes). From
//! `src/main/services/workspace/index.ts`, `src/main/services/lyrics/song-files.ts`
//! and `isInsideRoot` in `src/main/services/media/index.ts`.
//!
//! Every other service asks this module where things live, so moving the
//! workspace in Settings moves the whole app. Folders are created lazily and
//! come back if an operator deletes them.

use crate::fs_atomic::write_atomic;
use crate::settings::SettingsStore;
use kairo_domain::lyrics::LyricsSong;
use kairo_domain::workspace::{
    MEDIA_DIR_NAME, MediaFolderMigration, SONGS_DIR_NAME, WorkspaceInfo, is_inside_folder,
    is_song_file_name, parse_song_file, serialize_song_file, song_file_name,
    song_id_from_file_name,
};
use serde_json::{Value, json};
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// Media types counted when offering to adopt an existing media folder.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "avif", "heic", "mp4", "mov", "m4v", "webm", "mkv",
    "avi",
];

/// Suffix of a copy in progress during a cross-volume move.
const MOVING_SUFFIX: &str = ".kairo-moving";

/// `path.resolve(p)`: absolute against the current folder, with `.` and `..`
/// removed lexically. Symlinks are not followed.
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // `..` at the root stays at the root, as in Node.
                if out.parent().is_some() {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `isInsideRoot`: whether `file` is `root` or inside it, after resolving
/// both lexically. A sibling sharing the prefix (`<root>-private`) is outside.
pub fn is_inside_root(file: &Path, root: &Path) -> bool {
    let (target, base) = (resolve(file), resolve(root));
    // Component-wise, so `/media/backgroundsX` is not inside `/media/backgrounds`.
    target == base || target.starts_with(&base)
}

/// `isUnderAllowedFolder` in `src/main/index.ts`: both sides resolved
/// through the filesystem first, so a symlink inside an allowed folder that
/// points elsewhere is refused. A path that cannot be resolved (missing, or a
/// broken symlink) is refused.
pub fn is_under_allowed_folder(requested: &Path, roots: &[PathBuf]) -> bool {
    let Ok(target) = std::fs::canonicalize(requested) else {
        return false;
    };
    roots
        .iter()
        .any(|root| std::fs::canonicalize(root).is_ok_and(|base| is_inside_root(&target, &base)))
}

/// Why a workspace operation was refused.
#[derive(Debug)]
pub enum WorkspaceError {
    /// The new root is the current workspace or inside it.
    InsideCurrent,
    /// The folders could not be created.
    Io(std::io::Error),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::InsideCurrent => {
                write!(f, "Choose a folder outside the current workspace.")
            }
            WorkspaceError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<std::io::Error> for WorkspaceError {
    fn from(error: std::io::Error) -> Self {
        WorkspaceError::Io(error)
    }
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The workspace service. Holds the last `ensure` result so repeat reads skip
/// the filesystem.
pub struct Workspace {
    default_root: PathBuf,
    cached: Option<WorkspaceInfo>,
}

impl Workspace {
    /// `default_root` is [`crate::paths::AppPaths::default_workspace_root`].
    pub fn new(default_root: PathBuf) -> Self {
        Self {
            default_root,
            cached: None,
        }
    }

    /// `defaultRoot`.
    pub fn default_root(&self) -> &Path {
        &self.default_root
    }

    /// `root`: the configured folder (`workspace.folder`, trimmed and
    /// resolved), or the default when it is empty or not text.
    pub fn root(&self, settings: &SettingsStore) -> PathBuf {
        match settings
            .data()
            .get("workspace")
            .and_then(|w| w.get("folder"))
            .and_then(Value::as_str)
        {
            Some(folder) if !kairo_domain::js::trim(folder).is_empty() => {
                resolve(Path::new(kairo_domain::js::trim(folder)))
            }
            _ => self.default_root.clone(),
        }
    }

    pub fn songs_dir(&self, settings: &SettingsStore) -> PathBuf {
        self.root(settings).join(SONGS_DIR_NAME)
    }

    pub fn media_dir(&self, settings: &SettingsStore) -> PathBuf {
        self.root(settings).join(MEDIA_DIR_NAME)
    }

    /// `ensure` (`workspace:get`): creates the folders if missing. Never
    /// fails: a workspace on an unplugged drive reports `ready: false` and the error.
    pub fn ensure(&mut self, settings: &SettingsStore) -> WorkspaceInfo {
        let root = self.root(settings);
        let mut info = WorkspaceInfo {
            root: text(&root),
            songs_dir: text(&root.join(SONGS_DIR_NAME)),
            media_dir: text(&root.join(MEDIA_DIR_NAME)),
            is_default: root == self.default_root,
            ready: false,
            error: None,
        };
        let created = std::fs::create_dir_all(root.join(SONGS_DIR_NAME))
            .and_then(|()| std::fs::create_dir_all(root.join(MEDIA_DIR_NAME)));
        match created {
            Ok(()) => info.ready = true,
            Err(error) => {
                tracing::error!(root = %info.root, %error, "[Workspace] Could not create workspace folders");
                info.error = Some(error.to_string());
            }
        }
        self.cached = Some(info.clone());
        info
    }

    /// `info`: the cached result while the root is unchanged.
    pub fn info(&mut self, settings: &SettingsStore) -> WorkspaceInfo {
        let root = text(&self.root(settings));
        match &self.cached {
            Some(info) if info.root == root => info.clone(),
            _ => self.ensure(settings),
        }
    }

    /// `setRoot`: points the workspace at `next`, optionally moving `Songs`
    /// and `Media` contents across. A media folder inside the old workspace follows.
    pub fn set_root(
        &mut self,
        settings: &mut SettingsStore,
        next: &Path,
        move_contents: bool,
    ) -> Result<WorkspaceInfo, WorkspaceError> {
        let target = resolve(next);
        let previous = self.root(settings);
        if target == previous {
            return Ok(self.ensure(settings));
        }
        if is_inside_folder(&text(&target), &text(&previous)) {
            return Err(WorkspaceError::InsideCurrent);
        }
        std::fs::create_dir_all(target.join(SONGS_DIR_NAME))?;
        std::fs::create_dir_all(target.join(MEDIA_DIR_NAME))?;
        if move_contents {
            for dir in [SONGS_DIR_NAME, MEDIA_DIR_NAME] {
                move_folder_contents(&previous.join(dir), &target.join(dir));
            }
        }
        settings.merge_fields("workspace", [("folder", json!(text(&target)))]);
        self.cached = None;
        tracing::info!(from = %previous.display(), to = %target.display(), moved = move_contents, "[Workspace] Root changed");

        let media = Self::media_folder(settings);
        if !media.is_empty() && is_inside_folder(&media, &text(&previous)) {
            settings.merge_fields(
                "media",
                [("folder", json!(text(&target.join(MEDIA_DIR_NAME))))],
            );
        }
        Ok(self.ensure(settings))
    }

    /// `media.folder` resolved, or empty when unset.
    fn media_folder(settings: &SettingsStore) -> String {
        match settings
            .data()
            .get("media")
            .and_then(|m| m.get("folder"))
            .and_then(Value::as_str)
        {
            Some(folder) if !folder.is_empty() => text(&resolve(Path::new(folder))),
            _ => String::new(),
        }
    }

    /// `mediaMigration`: whether a media folder outside the workspace could
    /// be brought in, and how many media files it holds (`-1` if unreadable).
    pub fn media_migration(&self, settings: &SettingsStore) -> MediaFolderMigration {
        let resolved = Self::media_folder(settings);
        let needed =
            !resolved.is_empty() && !is_inside_folder(&resolved, &text(&self.root(settings)));
        let mut file_count = 0.0;
        if needed {
            file_count = match std::fs::read_dir(&resolved) {
                Ok(entries) => entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                    .filter(|e| {
                        Path::new(&e.file_name())
                            .extension()
                            .and_then(|x| x.to_str())
                            .is_some_and(|x| MEDIA_EXTENSIONS.contains(&x.to_lowercase().as_str()))
                    })
                    .count() as f64,
                Err(_) => -1.0,
            };
        }
        MediaFolderMigration {
            current_folder: resolved,
            target_folder: text(&self.media_dir(settings)),
            needed,
            file_count,
        }
    }

    /// `adoptMediaFolder`: points `media.folder` at the workspace `Media`,
    /// optionally moving the old folder's contents. The media library rescan
    /// that follows (`mediaService.setFolder`) is prompt 043's.
    pub fn adopt_media_folder(
        &mut self,
        settings: &mut SettingsStore,
        move_contents: bool,
    ) -> String {
        let info = self.ensure(settings);
        let previous = Self::media_folder(settings);
        if move_contents && !previous.is_empty() && previous != info.media_dir {
            move_folder_contents(Path::new(&previous), Path::new(&info.media_dir));
        }
        settings.merge_fields("media", [("folder", json!(info.media_dir))]);
        tracing::info!(from = if previous.is_empty() { "(unset)" } else { &previous }, to = %info.media_dir, moved = move_contents, "[Workspace] Media folder adopted");
        info.media_dir
    }

    /// `applyDefaultMediaFolder`: a fresh install's media dock uses the
    /// workspace `Media`, never replacing a folder the operator chose.
    pub fn apply_default_media_folder(&mut self, settings: &mut SettingsStore) {
        if !Self::media_folder(settings).is_empty() {
            return;
        }
        let info = self.ensure(settings);
        if !info.ready {
            return;
        }
        settings.merge_fields("media", [("folder", json!(info.media_dir))]);
        tracing::info!(folder = %info.media_dir, "[Workspace] Media folder defaulted to the workspace");
    }

    /// The folder `workspace:reveal` opens, after making sure it exists. The
    /// UI opens it in Finder or Explorer.
    pub fn reveal_target(&mut self, settings: &SettingsStore) -> PathBuf {
        PathBuf::from(self.ensure(settings).root)
    }

    /// The folder `workspace:revealSongs` opens.
    pub fn reveal_songs_target(&mut self, settings: &SettingsStore) -> PathBuf {
        PathBuf::from(self.ensure(settings).songs_dir)
    }
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        // `fs.cp` copies a link as a link.
        let target = std::fs::read_link(from)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, to);
        #[cfg(windows)]
        return if std::fs::metadata(from).is_ok_and(|m| m.is_dir()) {
            std::os::windows::fs::symlink_dir(target, to)
        } else {
            std::os::windows::fs::symlink_file(target, to)
        };
    }
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

fn remove_any(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// `moveFolderContents`: moves each entry of `from` into `to`. A name already
/// taken in `to` is skipped (the existing entry wins, the source stays).
///
/// Across volumes, where a rename fails, an entry is copied to a hidden
/// `.<name>.kairo-moving` beside its destination, renamed into place, and
/// only then removed from `from`. Interrupted at any point, nothing is lost:
/// before the rename the source is whole and the partial copy is hidden (and
/// cleared by the next move); after it, both copies exist.
pub fn move_folder_contents(from: &Path, to: &Path) {
    move_folder_contents_with(from, to, &|a, b| std::fs::rename(a, b));
}

/// [`move_folder_contents`] with the first, same-volume rename supplied, so
/// tests can make it fail as it does across volumes.
fn move_folder_contents_with(
    from: &Path,
    to: &Path,
    rename: &dyn Fn(&Path, &Path) -> std::io::Result<()>,
) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    if let Err(error) = std::fs::create_dir_all(to) {
        tracing::warn!(dest = %to.display(), %error, "[Workspace] Could not create the destination");
        return;
    }
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_text = name.to_string_lossy();
        if name_text.ends_with(MOVING_SUFFIX) {
            continue;
        }
        let src = entry.path();
        let dest = to.join(&name);
        if std::fs::symlink_metadata(&dest).is_ok() {
            tracing::warn!(dest = %dest.display(), "[Workspace] Skipped move — name already exists at destination");
            continue;
        }
        if rename(&src, &dest).is_ok() {
            continue;
        }
        let staging = to.join(format!(".{name_text}{MOVING_SUFFIX}"));
        let result = remove_any(&staging)
            .and_then(|()| copy_recursive(&src, &staging))
            .and_then(|()| std::fs::rename(&staging, &dest))
            .and_then(|()| remove_any(&src));
        if let Err(error) = result {
            let _ = remove_any(&staging);
            tracing::warn!(src = %src.display(), %error, "[Workspace] Could not move entry");
        }
    }
}

// ─── Song files ─────────────────────────────────────────────────────────────────

/// A song read from the Songs folder.
#[derive(Clone, Debug, PartialEq)]
pub struct SongFileEntry {
    pub song: LyricsSong,
    /// The file it was read from.
    pub file: PathBuf,
}

/// `readSongFiles`: every readable song in `songs_dir`. Unreadable files are
/// logged and skipped; a missing folder is empty. `now_ms` stands in for
/// timestamps a file lacks.
pub fn read_song_files(songs_dir: &Path, now_ms: f64) -> Vec<SongFileEntry> {
    let Ok(names) = std::fs::read_dir(songs_dir) else {
        return Vec::new();
    };
    let mut entries = Vec::new();
    for entry in names.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_song_file_name(&name) {
            continue;
        }
        let file = songs_dir.join(&name);
        match std::fs::read_to_string(&file) {
            Ok(raw) => {
                match parse_song_file(&raw, song_id_from_file_name(&name).unwrap_or(""), now_ms) {
                    Some(song) => {
                        remember_embedded_id(&file, Some(song.id.clone()));
                        entries.push(SongFileEntry { song, file })
                    }
                    None => {
                        tracing::warn!(file = %file.display(), "[SongFiles] Skipped unreadable song file")
                    }
                }
            }
            Err(error) => {
                tracing::warn!(file = %file.display(), %error, "[SongFiles] Could not read song file")
            }
        }
    }
    entries
}

/// What a song file's metadata looked like when its embedded id was read.
#[derive(Clone, PartialEq)]
struct FileStamp {
    modified: Option<std::time::SystemTime>,
    len: u64,
    /// Inode and change time on Unix: an atomic replace gets a new inode and
    /// any write moves the change time, even at the same size and mtime.
    #[cfg(unix)]
    identity: (u64, i64, i64),
}

impl FileStamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            modified: metadata.modified().ok(),
            len: metadata.len(),
            #[cfg(unix)]
            identity: (metadata.ino(), metadata.ctime(), metadata.ctime_nsec()),
        }
    }
}

/// Embedded song ids by file, so finding a song's files only parses files
/// that are new or changed since they were last read. `None` means the file
/// did not parse and is matched by its file-name id.
type EmbeddedIds = std::collections::HashMap<PathBuf, (FileStamp, Option<String>)>;
static EMBEDDED_IDS: std::sync::Mutex<Option<EmbeddedIds>> = std::sync::Mutex::new(None);

fn remember_embedded_id(file: &Path, id: Option<String>) {
    let Ok(metadata) = std::fs::metadata(file) else {
        return;
    };
    let mut cache = EMBEDDED_IDS.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(Default::default)
        .insert(file.to_owned(), (FileStamp::of(&metadata), id));
}

fn forget_embedded_id(file: &Path) {
    let mut cache = EMBEDDED_IDS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cache) = cache.as_mut() {
        cache.remove(file);
    }
}

#[cfg(test)]
thread_local! {
    static SONG_FILE_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn embedded_id(file: &Path, metadata: &std::fs::Metadata, filename_id: &str) -> Option<String> {
    let stamp = FileStamp::of(metadata);
    {
        let cache = EMBEDDED_IDS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((cached, id)) = cache.as_ref().and_then(|c| c.get(file))
            && *cached == stamp
        {
            return id.clone();
        }
    }
    #[cfg(test)]
    SONG_FILE_PARSES.with(|n| n.set(n.get() + 1));
    let id = std::fs::read_to_string(file)
        .ok()
        .and_then(|raw| parse_song_file(&raw, filename_id, 0.0))
        .map(|song| song.id);
    let mut cache = EMBEDDED_IDS.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .get_or_insert_with(Default::default)
        .insert(file.to_owned(), (stamp, id.clone()));
    id
}

fn matching_song_files(songs_dir: &Path, song_id: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(songs_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !is_song_file_name(&name) {
                return None;
            }
            let metadata = entry.metadata().ok()?;
            if metadata.is_dir() {
                return None;
            }
            let file = entry.path();
            let filename_id = song_id_from_file_name(&name).unwrap_or("");
            let embedded_id = embedded_id(&file, &metadata, filename_id);
            (embedded_id.as_deref().unwrap_or(filename_id) == song_id).then_some(file)
        })
        .collect()
}

/// The first file holding a song id, including an externally renamed file.
pub fn find_song_file(songs_dir: &Path, song_id: &str) -> Option<PathBuf> {
    matching_song_files(songs_dir, song_id).into_iter().next()
}

/// `writeSongFile`: writes `song` as `<slug>__<id>.song.json`, atomically,
/// and removes its old file when the title (so the name) changed. `None` on failure.
pub fn write_song_file(songs_dir: &Path, song: &LyricsSong) -> Option<PathBuf> {
    let existing = matching_song_files(songs_dir, &song.id);
    write_song_file_replacing(songs_dir, song, existing)
}

fn write_song_file_replacing(
    songs_dir: &Path,
    song: &LyricsSong,
    existing: Vec<PathBuf>,
) -> Option<PathBuf> {
    let name = song_file_name(song);
    let target = songs_dir.join(&name);
    let result = (|| -> std::io::Result<()> {
        // The id comes from imported files: a separator or `..` in it must not
        // place the file outside the songs folder.
        if name.contains(['/', '\\']) || target.parent() != Some(songs_dir) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "song id is not a valid file name",
            ));
        }
        std::fs::create_dir_all(songs_dir)?;
        let text = serialize_song_file(song).map_err(std::io::Error::other)?;
        write_atomic(&target, text.as_bytes())?;
        remember_embedded_id(&target, Some(song.id.clone()));
        for existing in existing.into_iter().filter(|e| *e != target) {
            remove_any(&existing)?;
            forget_embedded_id(&existing);
        }
        Ok(())
    })();
    match result {
        Ok(()) => Some(target),
        Err(error) => {
            tracing::error!(id = %song.id, title = %song.title, %error, "[SongFiles] Could not write song file");
            None
        }
    }
}

/// `deleteSongFile`: removes the song's file. A missing file counts as success.
pub fn delete_song_file(songs_dir: &Path, song_id: &str) -> bool {
    let mut removed = true;
    for file in matching_song_files(songs_dir, song_id) {
        if let Err(error) = remove_any(&file) {
            tracing::error!(file = %file.display(), %error, "[SongFiles] Could not delete song file");
            removed = false;
        } else {
            forget_embedded_id(&file);
        }
    }
    removed
}

#[cfg(test)]
mod song_file_lookup_tests {
    use super::*;

    fn song(id: &str, title: &str) -> LyricsSong {
        serde_json::from_value(json!({
            "id": id, "title": title, "artist": "", "copyright": "", "ccliNumber": "",
            "isFavorite": false, "source": "text", "createdAt": 1, "updatedAt": 2,
            "sections": [{ "type": "verse", "label": "Verse 1", "lines": ["Line"] }]
        }))
        .unwrap()
    }

    fn parses() -> usize {
        SONG_FILE_PARSES.with(|n| n.get())
    }

    fn song_files(dir: &Path) -> usize {
        std::fs::read_dir(dir).unwrap().count()
    }

    #[test]
    fn an_externally_renamed_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let songs = dir.path().join("Songs");
        let file = write_song_file(&songs, &song("song-1", "Grace")).unwrap();
        let renamed = songs.join("Renamed__other.song.json");
        std::fs::rename(&file, &renamed).unwrap();
        let written = write_song_file(&songs, &song("song-1", "Grace")).unwrap();
        assert!(!renamed.exists());
        assert!(written.exists());
        assert_eq!(song_files(&songs), 1);
    }

    #[test]
    fn files_added_or_edited_by_another_process_are_found() {
        let dir = tempfile::tempdir().unwrap();
        let songs = dir.path().join("Songs");
        write_song_file(&songs, &song("song-1", "One")).unwrap();
        assert_eq!(find_song_file(&songs, "song-2"), None);

        // Added after the cache was built.
        let added = songs.join("Added__x.song.json");
        std::fs::write(&added, serialize_song_file(&song("song-2", "Two")).unwrap()).unwrap();
        assert_eq!(find_song_file(&songs, "song-2"), Some(added.clone()));

        // Edited in place to hold another id.
        std::fs::write(
            &added,
            serialize_song_file(&song("song-three", "Three")).unwrap(),
        )
        .unwrap();
        assert_eq!(find_song_file(&songs, "song-2"), None);
        assert_eq!(find_song_file(&songs, "song-three"), Some(added));
    }

    #[test]
    fn updating_one_song_does_not_parse_the_whole_folder() {
        let dir = tempfile::tempdir().unwrap();
        let songs = dir.path().join("Songs");
        for n in 0..200 {
            write_song_file(&songs, &song(&format!("song-{n}"), &format!("Song {n}"))).unwrap();
        }
        let before = parses();
        write_song_file(&songs, &song("song-7", "Renamed seven")).unwrap();
        assert!(delete_song_file(&songs, "song-8"));
        assert_eq!(parses() - before, 0, "every file was already known");
        assert_eq!(song_files(&songs), 199);
    }
}

#[cfg(test)]
mod tests {
    //! `tests/media-root.test.ts`, plus the realpath cases from `isUnderAllowedFolder`.
    use super::*;

    #[cfg(unix)]
    fn inside(file: &str, root: &str) -> bool {
        is_inside_root(Path::new(file), Path::new(root))
    }

    #[cfg(unix)]
    #[test]
    fn media_root_rules() {
        assert!(inside("/media/backgrounds/loop.mp4", "/media/backgrounds"));
        assert!(inside(
            "/media/backgrounds/motion/loop.mp4",
            "/media/backgrounds"
        ));
        assert!(inside("/media/backgrounds", "/media/backgrounds"));
        assert!(!inside(
            "/media/backgrounds-private/secrets.mp4",
            "/media/backgrounds"
        ));
        assert!(!inside("/media/backgroundsX", "/media/backgrounds"));
        assert!(!inside(
            "/media/backgrounds/../../etc/passwd",
            "/media/backgrounds"
        ));
        assert!(!inside("/etc/passwd", "/media/backgrounds"));
        assert!(!inside("/media", "/media/backgrounds"));
        assert!(inside(
            "/media/backgrounds/./motion/loop.mp4",
            "/media/backgrounds"
        ));
        assert!(inside(
            "/media/backgrounds/motion/../loop.mp4",
            "/media/backgrounds"
        ));
        assert!(inside("/media/backgrounds/", "/media/backgrounds"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_an_allowed_folder_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Media");
        let outside = dir.path().join("private");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("loop.mp4"), "").unwrap();
        std::fs::write(outside.join("secret.mp4"), "").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.mp4"), root.join("link.mp4")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("gone.mp4"), root.join("broken.mp4")).unwrap();
        let sibling = dir.path().join("Media-private");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("x.mp4"), "").unwrap();
        let roots = vec![root.clone()];
        assert!(is_under_allowed_folder(&root.join("loop.mp4"), &roots));
        assert!(
            !is_under_allowed_folder(&root.join("link.mp4"), &roots),
            "a link pointing out is refused"
        );
        assert!(
            !is_under_allowed_folder(&root.join("broken.mp4"), &roots),
            "a broken link is refused"
        );
        assert!(!is_under_allowed_folder(&root.join("missing.mp4"), &roots));
        assert!(!is_under_allowed_folder(&sibling.join("x.mp4"), &roots));
        // A root reached through a symlink still works: both sides are resolved.
        let linked_root = dir.path().join("linked");
        std::os::unix::fs::symlink(&root, &linked_root).unwrap();
        assert!(is_under_allowed_folder(
            &root.join("loop.mp4"),
            &[linked_root]
        ));
        assert!(!is_under_allowed_folder(
            &root.join("loop.mp4"),
            &[dir.path().join("no-such-root")]
        ));
    }

    fn tree(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if path.is_dir() {
                    out.push(format!("{rel}/"));
                    walk(base, &path, out);
                } else {
                    out.push(format!(
                        "{rel}={}",
                        std::fs::read_to_string(&path).unwrap_or_default()
                    ));
                }
            }
        }
        walk(root, root, &mut out);
        out.sort();
        out
    }

    fn cross_volume(_: &Path, _: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::CrossesDevices,
            "EXDEV",
        ))
    }

    #[test]
    fn a_cross_volume_move_copies_then_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("old"), dir.path().join("new"));
        std::fs::create_dir_all(from.join("Motion/deep")).unwrap();
        std::fs::write(from.join("a.mp4"), "a").unwrap();
        std::fs::write(from.join("Motion/deep/b.mov"), "b").unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(to.join("a.mp4"), "kept").unwrap();
        move_folder_contents_with(&from, &to, &cross_volume);
        assert_eq!(
            tree(&to),
            [
                "Motion/",
                "Motion/deep/",
                "Motion/deep/b.mov=b",
                "a.mp4=kept"
            ]
        );
        // The clash stays at the source; the moved folder is gone from it.
        assert_eq!(tree(&from), ["a.mp4=a"]);
    }

    #[test]
    fn an_interrupted_copy_is_hidden_and_finished_next_time() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("old"), dir.path().join("new"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("song.json"), "whole").unwrap();
        // A copy killed half way: a partial staging file, the source untouched.
        std::fs::write(to.join(".song.json.kairo-moving"), "wh").unwrap();
        assert_eq!(
            std::fs::read_dir(&to)
                .unwrap()
                .filter(|e| e.as_ref().unwrap().file_name() == "song.json")
                .count(),
            0
        );
        move_folder_contents_with(&from, &to, &cross_volume);
        assert_eq!(tree(&to), ["song.json=whole"]);
        assert!(tree(&from).is_empty());
    }

    #[test]
    fn a_failed_copy_leaves_the_source_and_no_staging() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("old"), dir.path().join("new"));
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("x.mp4"), "x").unwrap();
        // The destination is a file, so nothing can be created in it.
        std::fs::write(&to, "").unwrap();
        move_folder_contents_with(&from, &to, &cross_volume);
        assert_eq!(tree(&from), ["x.mp4=x"]);
        // A missing source is nothing to move.
        move_folder_contents_with(
            &dir.path().join("missing"),
            &dir.path().join("t"),
            &cross_volume,
        );
    }

    #[test]
    fn resolve_matches_node() {
        assert_eq!(resolve(Path::new("/a/b/../c/./d")), PathBuf::from("/a/c/d"));
        assert_eq!(resolve(Path::new("/../..")), PathBuf::from("/"));
        assert!(resolve(Path::new("rel")).is_absolute());
    }
}
