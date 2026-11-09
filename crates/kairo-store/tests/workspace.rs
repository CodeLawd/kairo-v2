//! The workspace service and song files against the Electron app: songs
//! written here are byte-identical to `serializeSongFile` and read back by
//! `parseSongFile`; files Electron writes read here as Electron reads them.

use kairo_domain::json::to_js_value;
use kairo_domain::lyrics::LyricsSong;
use kairo_oracle::{generate, oracle_values, strip_undefined};
use kairo_store::settings::{LaunchContext, SettingsStore};
use kairo_store::workspace::{
    Workspace, WorkspaceError, delete_song_file, find_song_file, read_song_files, write_song_file,
};
use proptest::prelude::*;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Instant;

fn settings(dir: &Path) -> SettingsStore {
    let mut ids = kairo_oracle::pinned_uuids();
    SettingsStore::open(
        &dir.join("userData"),
        &mut LaunchContext {
            now_ms: 1.0,
            new_id: &mut ids,
        },
    )
}

fn song(id: &str, title: &str) -> LyricsSong {
    serde_json::from_value(json!({
        "id": id, "title": title, "artist": "", "copyright": "", "ccliNumber": "", "isFavorite": false,
        "source": "text", "createdAt": 1, "updatedAt": 2,
        "sections": [{ "type": "verse", "label": "Verse 1", "lines": ["Amazing grace", "How sweet the sound"] }]
    }))
    .unwrap()
}

#[test]
fn ensure_creates_the_folders_and_reports_failure_without_failing() {
    let dir = tempfile::tempdir().unwrap();
    let store = settings(dir.path());
    let mut ws = Workspace::new(dir.path().join("Documents/Kairo Presenter"));
    let info = ws.ensure(&store);
    assert!(info.ready && info.is_default && info.error.is_none());
    assert!(Path::new(&info.songs_dir).is_dir() && Path::new(&info.media_dir).is_dir());
    // Deleted between services: back on the next ensure.
    std::fs::remove_dir_all(&info.songs_dir).unwrap();
    assert!(ws.ensure(&store).ready && Path::new(&info.songs_dir).is_dir());

    // A root that cannot be created (a file is in the way) degrades to ready: false.
    std::fs::write(dir.path().join("blocked"), "").unwrap();
    let mut ws = Workspace::new(dir.path().join("blocked/Kairo Presenter"));
    let info = ws.ensure(&store);
    assert!(!info.ready);
    assert!(info.error.is_some());
}

#[test]
fn the_configured_folder_wins_and_is_trimmed_and_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settings(dir.path());
    let ws = Workspace::new(dir.path().join("default"));
    let custom = dir.path().join("custom");
    store.set_raw(
        "workspace",
        json!({ "folder": format!("  {}/./x/..  ", custom.display()) }),
    );
    assert_eq!(ws.root(&store), custom);
    store.set_raw("workspace", json!({ "folder": "   " }));
    assert_eq!(ws.root(&store), dir.path().join("default"));
    store.set_raw("workspace", json!({ "folder": 5 }));
    assert_eq!(ws.root(&store), dir.path().join("default"));
}

#[test]
fn set_root_moves_contents_and_media_follows() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settings(dir.path());
    let old = dir.path().join("old");
    let mut ws = Workspace::new(old.clone());
    ws.ensure(&store);
    std::fs::write(old.join("Songs/a__1.song.json"), "{}").unwrap();
    std::fs::write(old.join("Media/loop.mp4"), "m").unwrap();
    store.set_raw(
        "media",
        json!({ "folder": old.join("Media").to_string_lossy(), "playlists": [] }),
    );

    assert!(matches!(
        ws.set_root(&mut store, &old.join("inside"), true),
        Err(WorkspaceError::InsideCurrent)
    ));
    let same = ws.set_root(&mut store, &old, true).unwrap();
    assert_eq!(same.root, old.to_string_lossy());

    let new = dir.path().join("external");
    std::fs::create_dir_all(new.join("Media")).unwrap();
    std::fs::write(new.join("Media/loop.mp4"), "theirs").unwrap();
    let info = ws.set_root(&mut store, &new, true).unwrap();
    assert!(info.ready && !info.is_default);
    assert_eq!(
        std::fs::read_to_string(new.join("Songs/a__1.song.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(new.join("Media/loop.mp4")).unwrap(),
        "theirs",
        "an existing name is never overwritten"
    );
    assert!(
        old.join("Media/loop.mp4").exists(),
        "the clashing source stays"
    );
    assert_eq!(
        store.data()["workspace"]["folder"],
        json!(new.to_string_lossy())
    );
    assert_eq!(
        store.data()["media"]["folder"],
        json!(new.join("Media").to_string_lossy())
    );
    assert_eq!(
        store.data()["media"]["playlists"],
        json!([]),
        "other media fields kept"
    );
}

#[test]
fn set_root_without_move_leaves_files_and_a_foreign_media_folder() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settings(dir.path());
    let mut ws = Workspace::new(dir.path().join("old"));
    ws.ensure(&store);
    std::fs::write(dir.path().join("old/Songs/a__1.song.json"), "{}").unwrap();
    store.set_raw("media", json!({ "folder": "/Volumes/Backgrounds" }));
    ws.set_root(&mut store, &dir.path().join("new"), false)
        .unwrap();
    assert!(dir.path().join("old/Songs/a__1.song.json").exists());
    assert!(!dir.path().join("new/Songs/a__1.song.json").exists());
    assert_eq!(
        store.data()["media"]["folder"],
        json!("/Volumes/Backgrounds")
    );
}

#[test]
fn media_folder_adoption() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settings(dir.path());
    let mut ws = Workspace::new(dir.path().join("ws"));
    // Unset: nothing to offer.
    let m = ws.media_migration(&store);
    assert!(!m.needed && m.current_folder.is_empty() && m.file_count == 0.0);

    let backgrounds = dir.path().join("Backgrounds");
    std::fs::create_dir_all(backgrounds.join("Sub")).unwrap();
    for name in ["a.MP4", "b.jpg", "c.txt", ".hidden.png"] {
        std::fs::write(backgrounds.join(name), "x").unwrap();
    }
    store.set_raw(
        "media",
        json!({ "folder": backgrounds.to_string_lossy(), "playlists": [1] }),
    );
    let m = ws.media_migration(&store);
    assert!(m.needed);
    assert_eq!(
        m.file_count, 3.0,
        "media files only, any case, hidden ones count as Electron counts them"
    );
    assert_eq!(
        m.target_folder,
        dir.path().join("ws/Media").to_string_lossy()
    );

    store.set_raw(
        "media",
        json!({ "folder": dir.path().join("gone").to_string_lossy() }),
    );
    assert_eq!(
        ws.media_migration(&store).file_count,
        -1.0,
        "an unreadable folder"
    );

    store.set_raw(
        "media",
        json!({ "folder": backgrounds.to_string_lossy(), "playlists": [1] }),
    );
    let target = ws.adopt_media_folder(&mut store, true);
    assert_eq!(target, dir.path().join("ws/Media").to_string_lossy());
    assert!(dir.path().join("ws/Media/a.MP4").exists() && dir.path().join("ws/Media/Sub").is_dir());
    assert_eq!(
        store.data()["media"],
        json!({ "folder": target, "playlists": [1] })
    );
    assert!(
        !ws.media_migration(&store).needed,
        "inside the workspace now"
    );
}

#[test]
fn the_default_media_folder_never_replaces_a_chosen_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = settings(dir.path());
    let mut ws = Workspace::new(dir.path().join("ws"));
    store.set_raw("media", json!({ "folder": "/chosen" }));
    ws.apply_default_media_folder(&mut store);
    assert_eq!(store.data()["media"]["folder"], json!("/chosen"));
    store.set_raw("media", json!({ "folder": "" }));
    ws.apply_default_media_folder(&mut store);
    assert_eq!(
        store.data()["media"]["folder"],
        json!(dir.path().join("ws/Media").to_string_lossy())
    );
    // A workspace that cannot be created leaves media unset.
    std::fs::write(dir.path().join("file"), "").unwrap();
    let mut blocked = Workspace::new(dir.path().join("file/ws"));
    store.set_raw("media", json!({ "folder": "" }));
    blocked.apply_default_media_folder(&mut store);
    assert_eq!(store.data()["media"]["folder"], json!(""));
}

#[test]
fn reveal_targets_exist() {
    let dir = tempfile::tempdir().unwrap();
    let store = settings(dir.path());
    let mut ws = Workspace::new(dir.path().join("ws"));
    assert!(ws.reveal_target(&store).is_dir());
    assert!(ws.reveal_songs_target(&store).ends_with("Songs"));
}

#[test]
fn song_files_are_named_renamed_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let songs = dir.path().join("Songs");
    let file = write_song_file(&songs, &song("song-1", "Amazing Grace (My Chains)")).unwrap();
    assert_eq!(
        file.file_name().unwrap(),
        "Amazing-Grace-My-Chains__song-1.song.json"
    );
    let renamed = write_song_file(&songs, &song("song-1", "Grace")).unwrap();
    assert_eq!(renamed.file_name().unwrap(), "Grace__song-1.song.json");
    assert!(
        !file.exists(),
        "the old name is removed when the title changes"
    );
    assert_eq!(find_song_file(&songs, "song-1"), Some(renamed.clone()));
    assert_eq!(find_song_file(&songs, "nope"), None);
    assert_eq!(
        std::fs::read_dir(&songs).unwrap().count(),
        1,
        "no temporary files remain"
    );
    assert!(delete_song_file(&songs, "song-1"));
    assert!(
        delete_song_file(&songs, "song-1"),
        "already gone counts as success"
    );
    assert_eq!(find_song_file(&dir.path().join("missing"), "x"), None);
}

#[test]
fn reading_skips_what_electron_skips() {
    let dir = tempfile::tempdir().unwrap();
    let songs = dir.path().join("Songs");
    write_song_file(&songs, &song("song-1", "One")).unwrap();
    std::fs::write(songs.join("broken__song-2.song.json"), "{not json").unwrap();
    std::fs::write(songs.join(".hidden__song-3.song.json"), "{}").unwrap();
    std::fs::write(songs.join("notes.txt"), "x").unwrap();
    std::fs::write(
        songs.join("Untitled__song-4.song.json"),
        r#"{"title":"No id in file","sections":[]}"#,
    )
    .unwrap();
    // Unrelated JSON dropped in the folder is not a song.
    std::fs::write(
        songs.join("stray__song-5.song.json"),
        r#"{"title":"No sections"}"#,
    )
    .unwrap();
    std::fs::write(songs.join("array__song-6.song.json"), "[]").unwrap();
    let mut entries = read_song_files(&songs, 5.0);
    entries.sort_by(|a, b| a.song.id.cmp(&b.song.id));
    let ids: Vec<_> = entries.iter().map(|e| e.song.id.as_str()).collect();
    assert_eq!(
        ids,
        ["song-1", "song-4"],
        "the file name supplies a missing id"
    );
    assert!(read_song_files(&dir.path().join("missing"), 0.0).is_empty());
    // A failed write reports None rather than throwing.
    std::fs::write(dir.path().join("file"), "").unwrap();
    assert_eq!(
        write_song_file(&dir.path().join("file/Songs"), &song("x", "X")),
        None
    );
}

/// A generated song, as the oracle tests in `kairo-domain` build them.
fn generated_song() -> impl Strategy<Value = Value> {
    let text = || prop_oneof!["[A-Za-z ,'!?-]{0,24}", "\\PC{0,12}", Just(String::new())];
    let section = (
        prop::sample::select(vec!["verse", "chorus", "bridge", "tag", "other"]),
        text(),
        prop::collection::vec(text(), 0..5),
    )
        .prop_map(|(kind, label, lines)| json!({ "type": kind, "label": label, "lines": lines }));
    ("[a-z0-9-]{1,12}", text(), text(), prop::collection::vec(section, 0..4), 1u64..2_000_000_000_000).prop_map(
        |(id, title, artist, sections, at)| {
            json!({ "id": id, "title": title, "artist": artist, "copyright": "", "ccliNumber": "", "isFavorite": false,
                "source": "text", "sections": sections, "createdAt": at, "updatedAt": at + 1 })
        },
    )
}

#[test]
fn songs_written_here_are_electrons_files_and_load_in_electron() {
    let dir = tempfile::tempdir().unwrap();
    let raw = generate(2401, 300, generated_song());
    // Electron's normalisation first, so both sides start from a song it would hold.
    let parsed_inputs: Vec<_> = raw
        .iter()
        .map(|s| json!([serde_json::to_string(s).unwrap(), "fallback"]))
        .collect();
    let songs: Vec<LyricsSong> = oracle_values("workspace.parseSongFile", &parsed_inputs)
        .into_iter()
        .map(|v| serde_json::from_value(strip_undefined(v)).unwrap())
        .collect();

    let expected_texts = oracle_values(
        "workspace.serializeSongFile",
        &songs
            .iter()
            .map(|s| json!([to_js_value(s).unwrap()]))
            .collect::<Vec<_>>(),
    );
    let mut texts = Vec::new();
    for (song, expected) in songs.iter().zip(&expected_texts) {
        let file = write_song_file(&dir.path().join("Songs"), song).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(&json!(text), expected, "{}", song.id);
        texts.push(json!([text, "fallback"]));
    }
    // Electron reads each file back to the same song.
    let read_back = oracle_values("workspace.parseSongFile", &texts);
    for (song, back) in songs.iter().zip(read_back) {
        assert_eq!(
            to_js_value(song).unwrap(),
            strip_undefined(back),
            "{}",
            song.id
        );
    }
}

#[test]
fn files_electron_writes_read_here_as_electron_reads_them() {
    let dir = tempfile::tempdir().unwrap();
    let songs_dir = dir.path().join("Songs");
    std::fs::create_dir_all(&songs_dir).unwrap();
    let raw = generate(2402, 300, generated_song());
    let texts = oracle_values(
        "workspace.serializeSongFile",
        &raw.iter().map(|s| json!([s])).collect::<Vec<_>>(),
    );
    let mut expected = Vec::new();
    for (index, text) in texts.iter().enumerate() {
        let name = format!(
            "song-{index}__{}.song.json",
            raw[index]["id"].as_str().unwrap()
        );
        std::fs::write(songs_dir.join(&name), text.as_str().unwrap()).unwrap();
        expected.push((name, text.clone()));
    }
    let electron = oracle_values(
        "workspace.parseSongFile",
        &expected
            .iter()
            .map(|(name, text)| {
                json!([
                    text,
                    kairo_domain::workspace::song_id_from_file_name(name).unwrap()
                ])
            })
            .collect::<Vec<_>>(),
    );
    let mut ours = read_song_files(&songs_dir, 0.0);
    ours.sort_by_key(|e| e.file.clone());
    let mut wanted: Vec<_> = expected
        .iter()
        .map(|(name, _)| songs_dir.join(name))
        .zip(electron)
        .collect();
    wanted.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(ours.len(), wanted.len());
    for (entry, (file, song)) in ours.iter().zip(wanted) {
        assert_eq!(entry.file, file);
        assert_eq!(
            to_js_value(&entry.song).unwrap(),
            strip_undefined(song),
            "{}",
            file.display()
        );
    }
}

/// The real Songs folder on this machine against the live TypeScript parser.
/// Personal data: nothing is cached or committed. Run with
/// `KAIRO_ORACLE_MODE=live cargo test -p kairo-store --test workspace -- --ignored real_songs`.
#[test]
#[ignore = "reads the local Songs folder; live oracle only"]
fn real_songs_folder_reads_as_electron_reads_it() {
    assert_eq!(
        std::env::var("KAIRO_ORACLE_MODE").as_deref(),
        Ok("live"),
        "live only, so personal data is never cached"
    );
    let paths = kairo_store::paths::detect().unwrap();
    let mut ws = Workspace::new(paths.default_workspace_root());
    let songs_dir = std::path::PathBuf::from(&paths.default_workspace_root()).join("Songs");
    let _ = &mut ws;
    let mut ours = read_song_files(&songs_dir, 0.0);
    ours.sort_by_key(|e| e.file.clone());
    let inputs: Vec<_> = ours
        .iter()
        .map(|e| {
            let name = e.file.file_name().unwrap().to_string_lossy().into_owned();
            json!([
                std::fs::read_to_string(&e.file).unwrap(),
                kairo_domain::workspace::song_id_from_file_name(&name).unwrap()
            ])
        })
        .collect();
    let electron = oracle_values("workspace.parseSongFile", &inputs);
    for (entry, song) in ours.iter().zip(electron) {
        assert_eq!(
            to_js_value(&entry.song).unwrap(),
            strip_undefined(song),
            "{}",
            entry.file.display()
        );
    }
    println!("{} real song files read identically", ours.len());
}

#[test]
fn rescanning_five_thousand_songs_off_the_ui_thread() {
    let dir = tempfile::tempdir().unwrap();
    let songs_dir = dir.path().join("Songs");
    std::fs::create_dir_all(&songs_dir).unwrap();
    // Written directly: `write_song_file` looks for an older file of the same
    // song each time, which makes building 5,000 of them slow.
    for i in 0..5_000 {
        let mut s = song(&format!("song-{i}"), &format!("Song number {i}"));
        s.sections = std::iter::repeat_n(s.sections[0].clone(), 6).collect();
        let text = kairo_domain::workspace::serialize_song_file(&s).unwrap();
        std::fs::write(
            songs_dir.join(kairo_domain::workspace::song_file_name(&s)),
            text,
        )
        .unwrap();
    }
    let mut samples = Vec::new();
    for _ in 0..3 {
        let dir = songs_dir.clone();
        let start = Instant::now();
        let count = std::thread::spawn(move || read_song_files(&dir, 0.0).len())
            .join()
            .unwrap();
        samples.push(start.elapsed());
        assert_eq!(count, 5_000);
    }
    println!("rescan of 5,000 song files: {samples:?}");
    assert!(samples.iter().all(|s| s.as_secs_f64() < 2.0), "{samples:?}");
}
