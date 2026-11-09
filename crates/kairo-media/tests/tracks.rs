//! Behaviours of `src/main/services/tracks/index.ts` and `tests/tracks.test.ts`.

use kairo_domain::media::{is_track_extension, sibling_audio_folder, track_file_name};
use kairo_media::settings::SettingsPort;
use kairo_media::tracks::TracksService;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Default, Clone)]
struct Settings(Arc<Mutex<HashMap<String, Value>>>);

impl SettingsPort for Settings {
    fn get(&self, key: &str) -> Option<Value> {
        self.0.lock().unwrap().get(key).cloned()
    }
    fn set(&mut self, key: &str, value: Value) {
        self.0.lock().unwrap().insert(key.into(), value);
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    settings: Settings,
    trashed: Arc<Mutex<Vec<PathBuf>>>,
    service: TracksService<Settings>,
}

/// Media at `<tmp>/Media`; audio adopts `<tmp>/Audio`. Trash records instead of
/// touching the real Trash.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = Settings::default();
    settings.set(
        "media",
        json!({ "folder": dir.path().join("Media").to_string_lossy() }),
    );
    let trashed = Arc::new(Mutex::new(Vec::new()));
    let log = trashed.clone();
    let service = TracksService::with_trash(
        settings.clone(),
        Box::new(move |path: &Path| {
            if !path.exists() {
                return Err(std::io::ErrorKind::NotFound.into());
            }
            log.lock().unwrap().push(path.to_path_buf());
            std::fs::remove_file(path)
        }),
        Duration::ZERO,
    );
    Fixture {
        dir,
        settings,
        trashed,
        service,
    }
}

fn audio(f: &Fixture) -> PathBuf {
    f.dir.path().join("Audio")
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"abc").unwrap();
}

#[test]
fn audio_lives_next_to_the_backgrounds_folder() {
    assert_eq!(
        sibling_audio_folder("/Users/codelawd/Documents/Kairo Presenter/Media"),
        "/Users/codelawd/Documents/Kairo Presenter/Audio"
    );
    assert!(is_track_extension("mp3") && is_track_extension(".wav"));
    assert!(!is_track_extension("mp4"));
    assert_eq!(track_file_name("My Song.MP3", "mp3"), "My Song");
}

#[test]
fn scan_adopts_the_sibling_folder_and_lists_audio() {
    let mut f = fixture();
    touch(&audio(&f).join("b.MP3"));
    touch(&audio(&f).join("Hymns/a.flac"));
    touch(&audio(&f).join("Hymns/Deep/c.mp3"));
    touch(&audio(&f).join(".hidden.mp3"));
    touch(&audio(&f).join("clip.mp4"));
    let library = f.service.scan();
    assert_eq!(
        f.settings.get("tracks").unwrap()["folder"],
        json!(audio(&f).to_string_lossy())
    );
    let items: Vec<_> = library
        .items
        .iter()
        .map(|i| (i.id.as_str(), i.ext.as_str(), i.size))
        .collect();
    assert_eq!(
        items,
        [("Hymns/a.flac", "flac", 3.0), ("b.MP3", "mp3", 3.0)]
    );
    assert_eq!(library.error, None);
}

#[test]
fn no_media_folder_means_an_empty_library_and_import_refuses() {
    let mut f = fixture();
    f.settings.set("media", json!({}));
    assert!(f.service.scan().items.is_empty());
    assert_eq!(
        f.service.import_files(&[]).unwrap_err().0,
        "Choose a backgrounds folder first — audio is kept next to it."
    );
}

#[test]
fn import_copies_audio_and_skips_the_rest() {
    let mut f = fixture();
    let downloads = f.dir.path().join("Downloads");
    touch(&downloads.join("song.mp3"));
    touch(&downloads.join("clip.mp4"));
    f.service
        .import_files(&[downloads.join("song.mp3")])
        .unwrap();
    let library = f
        .service
        .import_files(&[
            downloads.join("song.mp3"),
            downloads.join("clip.mp4"),
            audio(&f).join("song.mp3"),
        ])
        .unwrap();
    let ids: Vec<_> = library.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["song", "song 1"].map(|s| format!("{s}.mp3")));
    assert_eq!(
        f.service
            .import_files(&[downloads.join("clip.mp4")])
            .unwrap_err()
            .0,
        "None of those files are audio we can play."
    );
}

#[test]
fn remove_trashes_the_file_and_stops_it_when_live() {
    let mut f = fixture();
    touch(&audio(&f).join("a.mp3"));
    f.service.scan();
    f.service.play("a.mp3").unwrap();
    let library = f.service.remove("a.mp3").unwrap();
    assert_eq!(*f.trashed.lock().unwrap(), [audio(&f).join("a.mp3")]);
    assert!(library.items.is_empty());
    assert_eq!(library.live_id, None);
    assert_eq!(
        f.service.remove("a.mp3").unwrap_err().0,
        "That track is no longer in the folder."
    );

    // Deleted in the file manager first: still succeeds and the row goes.
    touch(&audio(&f).join("b.mp3"));
    f.service.scan();
    std::fs::remove_file(audio(&f).join("b.mp3")).unwrap();
    assert!(f.service.remove("b.mp3").unwrap().items.is_empty());
}

#[test]
fn remove_and_rename_refuse_a_track_outside_the_folder() {
    let mut f = fixture();
    touch(&audio(&f).join("a.mp3"));
    f.service.scan();
    let other = f.dir.path().join("Other");
    std::fs::create_dir_all(&other).unwrap();
    // Point the setting elsewhere without rescanning.
    f.settings
        .set("tracks", json!({ "folder": other.to_string_lossy() }));
    assert_eq!(
        f.service.remove("a.mp3").unwrap_err().0,
        "That track is outside the audio folder."
    );
    assert_eq!(
        f.service.rename("a.mp3", "x").unwrap_err().0,
        "That track is outside the audio folder."
    );
}

#[test]
fn rename_keeps_extension_case_and_live_state() {
    let mut f = fixture();
    touch(&audio(&f).join("Hymns/old.MP3"));
    touch(&audio(&f).join("Hymns/taken.MP3"));
    f.service.scan();
    f.service.play("Hymns/old.MP3").unwrap();
    f.service.set_paused(true);
    let library = f
        .service
        .rename("Hymns/old.MP3", "  New: name.mp3 ")
        .unwrap();
    assert!(
        audio(&f).join("Hymns/New  name.MP3").exists()
            || audio(&f).join("Hymns/New name.MP3").exists()
    );
    assert_eq!(library.live_id.as_deref(), Some("Hymns/New name.MP3"));
    assert!(library.live_paused);

    assert_eq!(
        f.service.rename("Hymns/New name.MP3", "...").unwrap_err().0,
        "Give the track a name."
    );
    assert_eq!(
        f.service
            .rename("Hymns/New name.MP3", "taken")
            .unwrap_err()
            .0,
        "A file called “taken.MP3” is already in that folder."
    );
    // Case-only rename is not a clash.
    let library = f.service.rename("Hymns/New name.MP3", "new name").unwrap();
    assert_eq!(
        library
            .items
            .iter()
            .filter(|i| i.name.eq_ignore_ascii_case("new name"))
            .count(),
        1
    );
}

#[test]
fn play_pause_and_stop() {
    let mut f = fixture();
    touch(&audio(&f).join("a.mp3"));
    f.service.scan();
    assert!(
        !f.service.set_paused(true).live_paused,
        "nothing live to pause"
    );
    assert_eq!(
        f.service.play("nope").unwrap_err().0,
        "That track is no longer in the folder."
    );
    f.service.play("a.mp3").unwrap();
    assert!(f.service.set_paused(true).live_paused);
    let library = f.service.stop();
    assert_eq!((library.live_id, library.live_paused), (None, false));
    assert_eq!(f.service.reveal("a.mp3").unwrap(), audio(&f).join("a.mp3"));
    assert_eq!(f.service.allowed_root(), Some(audio(&f)));
}

#[test]
fn set_folder_resets_live_and_unreadable_folder_reports() {
    let mut f = fixture();
    let file = f.dir.path().join("file.txt");
    touch(&file);
    let library = f.service.set_folder(&file.to_string_lossy());
    assert!(library.error.is_some());
    assert!(library.items.is_empty());
}
