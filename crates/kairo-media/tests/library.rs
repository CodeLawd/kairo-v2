//! Behaviours of `src/main/services/media/index.ts`, `tests/media-import.test.ts`
//! and `tests/media-playlists.test.ts`.

use kairo_domain::media::{MediaKind, MediaPlaylist};
use kairo_media::library::{
    MediaImportPlan, MediaService, legacy_item_id, list_media_folders, plan_media_import,
    plan_media_paste, repair_playlist_item_ids,
};
use kairo_media::settings::SettingsPort;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

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
    _dir: tempfile::TempDir,
    root: PathBuf,
    settings: Settings,
    clipboard: Arc<Mutex<Vec<PathBuf>>>,
    service: MediaService<Settings>,
}

fn fixture(files: &[&str]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Media");
    std::fs::create_dir_all(&root).unwrap();
    for file in files {
        touch(&root.join(file));
    }
    let mut settings = Settings::default();
    settings.set("media", json!({ "folder": root.to_string_lossy() }));
    let clipboard = Arc::new(Mutex::new(Vec::new()));
    let reader = clipboard.clone();
    let service = MediaService::with_clipboard(
        settings.clone(),
        Box::new(move || reader.lock().unwrap().clone()),
    );
    Fixture {
        _dir: dir,
        root,
        settings,
        clipboard,
        service,
    }
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

fn ids(f: &Fixture) -> Vec<String> {
    f.service
        .library()
        .items
        .into_iter()
        .map(|i| i.id)
        .collect()
}

fn manifest(f: &Fixture) -> Value {
    serde_json::from_str(&std::fs::read_to_string(f.root.join(".kairo/playlists.json")).unwrap())
        .unwrap()
}

fn playlist(item_ids: &[&str]) -> MediaPlaylist {
    MediaPlaylist {
        id: "pl-1".into(),
        name: "Sunday set".into(),
        item_ids: item_ids.iter().map(|s| s.to_string()).collect(),
        created_at: 1.0,
        extra: Default::default(),
    }
}

/// An absolute path on this platform for a Unix-style one: `C:\\…` on
/// Windows, where `/media` has no drive and resolves against the current one.
fn abs(unix: &str) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(format!("C:{}", unix.replace('/', "\\")))
    } else {
        PathBuf::from(unix)
    }
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ─── media-import.test.ts ───────────────────────────────────────────────────

#[test]
fn files_already_in_the_media_folder_are_reused_not_copied() {
    let root = abs("/media/backgrounds");
    let inside = abs("/media/backgrounds/Motion/loop.mp4");
    assert_eq!(
        plan_media_import(&inside, &root, &HashSet::new()),
        MediaImportPlan::Reuse(inside)
    );
}

#[test]
fn files_outside_the_media_folder_are_copied_in() {
    let taken = HashSet::from(["sunset.mp4".to_string()]);
    let plan = plan_media_import(&abs("/Downloads/sunset.mp4"), &abs("/media/bg"), &taken);
    assert_eq!(
        plan,
        MediaImportPlan::Copy {
            from: abs("/Downloads/sunset.mp4"),
            dest_name: "sunset 1.mp4".into()
        }
    );
}

#[test]
fn unsupported_types_are_skipped() {
    let plan = plan_media_import(
        Path::new("/Downloads/notes.pdf"),
        Path::new("/m"),
        &HashSet::new(),
    );
    assert_eq!(plan, MediaImportPlan::Skip);
    // Vector art belongs to logos and props, not backgrounds.
    let plan = plan_media_import(
        Path::new("/Downloads/logo.svg"),
        Path::new("/m"),
        &HashSet::new(),
    );
    assert_eq!(plan, MediaImportPlan::Skip);
}

#[test]
fn paste_duplicates_a_file_already_in_the_library() {
    let taken = HashSet::from(["loop.mp4".to_string()]);
    let plan = plan_media_paste(&abs("/m/bg/loop.mp4"), &taken);
    assert_eq!(
        plan,
        MediaImportPlan::Copy {
            from: abs("/m/bg/loop.mp4"),
            dest_name: "loop 1.mp4".into()
        }
    );
}

// ─── media-playlists.test.ts ────────────────────────────────────────────────

#[test]
fn entries_that_still_resolve_are_left_alone() {
    let known = strings(&["Motion/loop.mp4", "Stills/warm.png"]);
    let (p, healed) =
        repair_playlist_item_ids(&[playlist(&["Motion/loop.mp4", "Stills/warm.png"])], &known);
    assert_eq!(healed, 0);
    assert_eq!(p[0].item_ids, known);
}

#[test]
fn a_file_moved_to_another_folder_is_re_pointed() {
    let (p, healed) = repair_playlist_item_ids(
        &[playlist(&["Motion/loop.mp4"])],
        &strings(&["Stills/loop.mp4"]),
    );
    assert_eq!(healed, 1);
    assert_eq!(p[0].item_ids, ["Stills/loop.mp4"]);
}

#[test]
fn an_ambiguous_basename_is_never_guessed() {
    let (p, healed) = repair_playlist_item_ids(
        &[playlist(&["Motion/loop.mp4"])],
        &strings(&["Stills/loop.mp4", "Christmas/loop.mp4"]),
    );
    assert_eq!(healed, 0);
    assert_eq!(p[0].item_ids, ["Motion/loop.mp4"]);
}

#[test]
fn missing_and_renamed_entries_are_kept() {
    let (p, healed) = repair_playlist_item_ids(
        &[playlist(&[
            "Motion/gone.mp4",
            "Stills/warm.png",
            "Motion/loop.mp4",
        ])],
        &strings(&["Stills/warm.png", "Motion/loop-v2.mp4"]),
    );
    assert_eq!(healed, 0);
    assert_eq!(
        p[0].item_ids,
        ["Motion/gone.mp4", "Stills/warm.png", "Motion/loop.mp4"]
    );
}

#[test]
fn repair_is_idempotent_and_per_playlist() {
    let known = strings(&["Stills/loop.mp4"]);
    let mut b = playlist(&["Motion/loop.mp4", "gone.png"]);
    b.id = "b".into();
    let (first, healed) = repair_playlist_item_ids(&[playlist(&["Motion/loop.mp4"]), b], &known);
    assert_eq!(healed, 2);
    assert_eq!(first[1].item_ids, ["Stills/loop.mp4", "gone.png"]);
    assert_eq!(repair_playlist_item_ids(&first, &known).1, 0);
}

// ─── Service ────────────────────────────────────────────────────────────────

#[test]
fn scan_lists_backgrounds_one_level_deep_with_empty_shelves() {
    let mut f = fixture(&[
        "b.mp4",
        "A.PNG",
        "notes.txt",
        ".hidden.mp4",
        "Motion/loop.webm",
        "Motion/Deep/too-deep.mp4",
        ".kairo/x.mp4",
    ]);
    std::fs::create_dir(f.root.join("Empty")).unwrap();
    let library = f.service.scan();
    assert_eq!(library.error, None);
    assert_eq!(ids(&f), ["A.PNG", "b.mp4", "Motion/loop.webm"]);
    let a = &library.items[0];
    assert_eq!(
        (a.name.as_str(), a.ext.as_str(), a.kind, a.folder.as_str()),
        ("A", "png", MediaKind::Image, "")
    );
    assert_eq!(a.size_bytes, 1.0);
    let folders: Vec<_> = library
        .folders
        .iter()
        .map(|f| (f.id.as_str(), f.name.as_str(), f.count))
        .collect();
    assert_eq!(
        folders,
        [
            ("", "Media", 2.0),
            ("Empty", "Empty", 0.0),
            ("Motion", "Motion", 1.0)
        ]
    );
    assert_eq!(library.live_cue, Some(0.0));
}

#[test]
fn list_media_folders_puts_the_root_first() {
    let folders = list_media_folders(
        "Root",
        &strings(&["b", "A"]),
        &[("b".into(), 2), ("".into(), 1)],
    );
    let ids: Vec<_> = folders.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["", "A", "b"]);
}

#[test]
fn no_folder_clears_everything_and_unreadable_folder_reports() {
    let mut f = fixture(&["a.mp4"]);
    f.service.scan();
    f.service.set_folder("");
    assert!(f.service.library().items.is_empty());
    assert_eq!(f.service.library().error, None);
    assert_eq!(
        f.service.create_folder("x").unwrap_err().0,
        "Choose a backgrounds folder first."
    );

    let missing = f.root.join("nope");
    let library = f.service.set_folder(&missing.to_string_lossy());
    assert!(
        library
            .error
            .unwrap()
            .starts_with("Could not read the backgrounds folder — ")
    );
}

#[test]
fn a_stale_scan_for_another_folder_is_ignored() {
    let mut f = fixture(&["a.mp4"]);
    let stale = kairo_media::library::scan_media_folder("/elsewhere");
    f.service.apply_scan(stale);
    assert!(f.service.library().items.is_empty());
    let fresh = kairo_media::library::scan_media_folder(&f.service.scan_root());
    assert_eq!(f.service.apply_scan(fresh).items.len(), 1);
}

#[test]
fn playlists_order_and_grades_persist_in_the_manifest() {
    let mut f = fixture(&["a.mp4", "b.mp4", "c.png"]);
    f.service.scan();
    let library = f.service.create_playlist("  ");
    let id = library.playlists[0].id.clone();
    assert_eq!(library.playlists[0].name, "New playlist");
    assert!(id.starts_with("pl-"));
    f.service.rename_playlist(&id, " Sunday ");
    f.service.rename_playlist(&id, "   ");
    f.service
        .set_playlist_items(&id, strings(&["b.mp4", "a.mp4"]));
    f.service.set_item_order(strings(&["c.png", "", "a.mp4"]));
    let mut patch = Map::new();
    patch.insert("loop".into(), json!(true));
    patch.insert("hue".into(), json!(500));
    f.service.set_playback("a.mp4", &patch, false).unwrap();

    let text = std::fs::read_to_string(f.root.join(".kairo/playlists.json")).unwrap();
    assert!(text.starts_with("{\n  \"version\": 1,\n  \"playlists\": [\n    {"));
    assert!(text.ends_with("}\n"));
    let m = manifest(&f);
    assert_eq!(m["itemOrder"], json!(["c.png", "a.mp4"]));
    assert_eq!(m["playlists"][0]["name"], "Sunday");
    assert_eq!(m["playback"]["a.mp4"]["hue"], json!(180));

    // A fresh service reads it all back; new files append after the saved order.
    touch(&f.root.join("0-new.mp4"));
    let mut again = MediaService::new(f.settings.clone());
    let library = again.scan();
    let order: Vec<_> = library.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(order, ["c.png", "a.mp4", "0-new.mp4", "b.mp4"]);
    assert_eq!(library.playlists[0].item_ids, ["b.mp4", "a.mp4"]);
    assert!(again.playback("a.mp4").looped);

    // A default grade is removed again.
    let mut reset = Map::new();
    reset.insert("loop".into(), json!(false));
    reset.insert("hue".into(), json!(0));
    again.set_playback("a.mp4", &reset, false).unwrap();
    assert_eq!(manifest(&f)["playback"], json!({}));

    let library = again.delete_playlist(&id);
    assert!(library.playlists.is_empty());
}

#[test]
fn the_legacy_manifest_folder_is_read_and_moved_entries_heal() {
    let f0 = fixture(&["Stills/loop.mp4"]);
    std::fs::create_dir_all(f0.root.join(".proautomate")).unwrap();
    std::fs::write(
        f0.root.join(".proautomate/playlists.json"),
        r#"{"playlists":[{"id":"p","name":"N","itemIds":["Motion/loop.mp4",3]},{"bad":1}]}"#,
    )
    .unwrap();
    let mut f = f0;
    let library = f.service.scan();
    assert_eq!(library.playlists.len(), 1);
    assert_eq!(library.playlists[0].item_ids, ["Stills/loop.mp4"]);
    // The repair is written to the new location.
    assert_eq!(
        manifest(&f)["playlists"][0]["itemIds"],
        json!(["Stills/loop.mp4"])
    );
}

#[test]
fn a_corrupt_manifest_empties_playlists_without_overwriting() {
    let mut f = fixture(&["a.mp4"]);
    std::fs::create_dir_all(f.root.join(".kairo")).unwrap();
    std::fs::write(f.root.join(".kairo/playlists.json"), "{not json").unwrap();
    let library = f.service.scan();
    assert!(library.playlists.is_empty());
    assert_eq!(library.error, None);
    assert_eq!(
        std::fs::read_to_string(f.root.join(".kairo/playlists.json")).unwrap(),
        "{not json"
    );
}

#[test]
fn settings_playlists_migrate_once_into_the_manifest() {
    let mut f = fixture(&["a.mp4", "Motion/b.mp4"]);
    let a = legacy_item_id(&f.root.join("a.mp4"));
    let b = legacy_item_id(&f.root.join("Motion/b.mp4"));
    f.settings.set(
        "media",
        json!({
            "folder": f.root.to_string_lossy(),
            "playlists": [{ "id": "p", "name": "Old", "itemIds": [b, "unknown", a], "createdAt": 5 }],
            "stray": true,
        }),
    );
    let library = f.service.scan();
    assert_eq!(library.playlists[0].item_ids, ["Motion/b.mp4", "a.mp4"]);
    assert_eq!(manifest(&f)["playlists"][0]["createdAt"], json!(5));
    assert_eq!(
        f.settings.get("media").unwrap(),
        json!({ "folder": f.root.to_string_lossy(), "playlists": [] })
    );
}

#[test]
fn create_folder_validates_names() {
    let mut f = fixture(&[]);
    f.service.scan();
    // The root shelf is listed only once it holds files.
    assert_eq!(
        f.service.create_folder(" Christmas ").unwrap().folders[0].id,
        "Christmas"
    );
    assert_eq!(
        f.service.create_folder("Christmas").unwrap_err().0,
        "That folder already exists."
    );
    for bad in ["", " .hidden", "..", "/:*"] {
        assert_eq!(
            f.service.create_folder(bad).unwrap_err().0,
            "That folder name is not allowed.",
            "{bad:?}"
        );
    }
    // Separators are stripped, so a climbing name becomes `..x`: a dot name.
    assert_eq!(
        f.service.create_folder("../x").unwrap_err().0,
        "That folder name is not allowed."
    );
    f.service.create_folder("a/b").unwrap();
    assert!(f.root.join("ab").is_dir());
}

#[test]
fn delete_unlinks_and_drops_every_reference() {
    let mut f = fixture(&["a.mp4", "Motion/b.mp4"]);
    f.service.scan();
    let pl = f.service.create_playlist("P").playlists[0].id.clone();
    f.service
        .set_playlist_items(&pl, strings(&["Motion/b.mp4", "a.mp4"]));
    f.service
        .set_item_order(strings(&["Motion/b.mp4", "a.mp4"]));
    f.service.set_live_item(Some("Motion/b.mp4"));
    let library = f.service.delete_item("Motion/b.mp4").unwrap();
    assert!(!f.root.join("Motion/b.mp4").exists());
    assert_eq!(library.live_item_id, None);
    assert_eq!(library.playlists[0].item_ids, ["a.mp4"]);
    assert_eq!(
        library
            .folders
            .iter()
            .find(|x| x.id == "Motion")
            .unwrap()
            .count,
        0.0
    );
    assert_eq!(manifest(&f)["itemOrder"], json!(["a.mp4"]));
    assert_eq!(
        f.service.delete_item("Motion/b.mp4").unwrap_err().0,
        "That background is no longer in the folder."
    );

    // Already gone from disk: references still clear.
    std::fs::remove_file(f.root.join("a.mp4")).unwrap();
    assert!(f.service.delete_item("a.mp4").unwrap().items.is_empty());
}

#[test]
fn rename_keeps_the_extension_and_moves_references() {
    let mut f = fixture(&["Motion/loop.mp4", "Motion/taken.mp4"]);
    f.service.scan();
    let pl = f.service.create_playlist("P").playlists[0].id.clone();
    f.service
        .set_playlist_items(&pl, strings(&["Motion/loop.mp4"]));
    let mut patch = Map::new();
    patch.insert("loop".into(), json!(true));
    f.service
        .set_playback("Motion/loop.mp4", &patch, false)
        .unwrap();
    f.service.set_live_item(Some("Motion/loop.mp4"));

    let library = f
        .service
        .rename_item("Motion/loop.mp4", "Sunrise.MP4")
        .unwrap();
    assert!(f.root.join("Motion/Sunrise.mp4").exists());
    assert_eq!(library.live_item_id.as_deref(), Some("Motion/Sunrise.mp4"));
    assert_eq!(library.playlists[0].item_ids, ["Motion/Sunrise.mp4"]);
    assert!(library.playback.contains_key("Motion/Sunrise.mp4"));
    let item = f.service.item("Motion/Sunrise.mp4").unwrap();
    assert_eq!(
        (item.name.as_str(), item.folder.as_str()),
        ("Sunrise", "Motion")
    );

    assert_eq!(
        f.service
            .rename_item("Motion/Sunrise.mp4", "taken")
            .unwrap_err()
            .0,
        "“taken” already exists in this folder."
    );
    for bad in ["", ".x", ".mp4"] {
        assert_eq!(
            f.service
                .rename_item("Motion/Sunrise.mp4", bad)
                .unwrap_err()
                .0,
            "That name is not allowed."
        );
    }
    // Same name is a no-op.
    f.service
        .rename_item("Motion/Sunrise.mp4", "Sunrise")
        .unwrap();
    assert_eq!(
        f.service.reveal_item("Motion/Sunrise.mp4").unwrap(),
        f.root.join("Motion/Sunrise.mp4")
    );
}

#[test]
fn copy_then_paste_duplicates_after_the_anchor_and_repeats() {
    let mut f = fixture(&["a.mp4", "b.mp4"]);
    f.service.scan();
    assert_eq!(
        f.service.copy_items(&strings(&["gone"])).unwrap_err().0,
        "Nothing to copy."
    );
    f.service.copy_items(&strings(&["a.mp4"])).unwrap();
    assert!(f.service.clipboard_has_files());
    f.service.set_item_order(strings(&["a.mp4", "b.mp4"]));
    f.service.paste_items(None).unwrap();
    assert_eq!(ids(&f), ["a.mp4", "a 1.mp4", "b.mp4"]);
    f.service.paste_items(None).unwrap();
    assert_eq!(ids(&f), ["a.mp4", "a 2.mp4", "a 1.mp4", "b.mp4"]);
}

#[test]
fn cut_from_a_subfolder_moves_to_the_root_and_between_playlists() {
    let mut f = fixture(&["Motion/a.mp4", "a.mp4", "b.mp4"]);
    f.service.scan();
    let one = f.service.create_playlist("One").playlists[0].id.clone();
    let two = f.service.create_playlist("Two").playlists[1].id.clone();
    f.service
        .set_playlist_items(&one, strings(&["Motion/a.mp4", "b.mp4"]));
    f.service.set_playlist_items(&two, strings(&["b.mp4"]));

    f.service
        .cut_items(&strings(&["Motion/a.mp4", "b.mp4"]), Some(&one))
        .unwrap();
    let library = f.service.paste_items(Some(&two)).unwrap();
    assert!(!f.root.join("Motion/a.mp4").exists());
    assert!(f.root.join("a 1.mp4").exists());
    let p = |id: &str| {
        library
            .playlists
            .iter()
            .find(|p| p.id == id)
            .unwrap()
            .item_ids
            .clone()
    };
    assert!(p(&one).is_empty());
    assert_eq!(p(&two), ["a 1.mp4", "b.mp4"]);
    // Cut is consumed; the system clipboard is empty.
    assert_eq!(
        f.service.paste_items(None).unwrap_err().0,
        "Nothing to paste."
    );
    assert_eq!(
        f.service.cut_items(&[], None).unwrap_err().0,
        "Nothing to cut."
    );
}

#[test]
fn paste_from_the_system_clipboard_copies_supported_files() {
    let mut f = fixture(&["sunset.mp4"]);
    f.service.scan();
    let outside = f.root.parent().unwrap().join("Downloads");
    touch(&outside.join("sunset.mp4"));
    touch(&outside.join("notes.pdf"));
    *f.clipboard.lock().unwrap() = vec![outside.join("sunset.mp4"), outside.join("notes.pdf")];
    assert!(f.service.clipboard_has_files());
    let library = f.service.paste_items(None).unwrap();
    // Pasted ids join the saved order, which then leads the scan order.
    let order: Vec<_> = library.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(order, ["sunset 1.mp4", "sunset.mp4"]);

    *f.clipboard.lock().unwrap() = vec![outside.join("notes.pdf")];
    assert_eq!(
        f.service.paste_items(None).unwrap_err().0,
        "Nothing to paste."
    );
}

#[test]
fn import_reuses_inside_files_and_copies_outside_ones() {
    let mut f = fixture(&["Motion/loop.mp4"]);
    f.service.scan();
    let pl = f.service.create_playlist("P").playlists[0].id.clone();
    let outside = f.root.parent().unwrap().join("Downloads/still.png");
    touch(&outside);
    let library = f
        .service
        .import_into_playlist(
            Some(&pl),
            &[
                f.root.join("Motion/loop.mp4"),
                outside.clone(),
                outside.clone(),
            ],
        )
        .unwrap();
    assert_eq!(
        library.playlists[0].item_ids,
        ["Motion/loop.mp4", "still.png", "still 1.png"]
    );
    assert_eq!(
        f.service
            .import_into_playlist(Some("nope"), std::slice::from_ref(&outside))
            .unwrap_err()
            .0,
        "That playlist is gone."
    );
    assert_eq!(
        f.service
            .import_into_playlist(None, &[PathBuf::from("/x/notes.pdf")])
            .unwrap_err()
            .0,
        "None of those files are a background we can play."
    );
}

#[test]
fn live_item_cue_pause_and_disappearance() {
    let mut f = fixture(&["a.mp4", "b.png"]);
    f.service.scan();
    let sub = f.service.subscribe();
    f.service.set_live_item(Some("b.png"));
    assert!(
        !f.service.set_live_paused(true).live_paused,
        "a still cannot pause"
    );
    f.service.set_live_item(Some("a.mp4"));
    assert!(f.service.set_live_paused(true).live_paused);
    assert_eq!(f.service.live_cue(), 2.0);

    // Live grade follows edits unless they are for the next cue.
    let mut patch = Map::new();
    patch.insert("rate".into(), json!(2));
    f.service.set_playback("a.mp4", &patch, true).unwrap();
    assert_eq!(f.service.live_playback().unwrap().rate, None);
    f.service.set_playback("a.mp4", &patch, false).unwrap();
    assert_eq!(f.service.live_playback().unwrap().rate, Some(2.0));

    std::fs::remove_file(f.root.join("a.mp4")).unwrap();
    let library = f.service.scan();
    assert_eq!(library.live_item_id, None);
    assert!(!library.live_paused);
    // The burst above coalesced to the newest snapshot.
    assert_eq!(sub.take().unwrap(), library);
    assert_eq!(sub.take(), None);
    assert_eq!(f.service.allowed_root(), Some(f.root.clone()));
}

#[cfg(unix)]
#[test]
fn a_read_only_folder_keeps_the_edit_and_reports() {
    use std::os::unix::fs::PermissionsExt;
    let mut f = fixture(&["a.mp4"]);
    f.service.scan();
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o555)).unwrap();
    let library = f.service.create_playlist("Kept");
    let mut patch = Map::new();
    patch.insert("loop".into(), json!(true));
    let next_cue = f.service.set_playback("a.mp4", &patch, true);
    std::fs::set_permissions(&f.root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(library.playlists.len(), 1);
    assert!(
        library
            .error
            .unwrap()
            .starts_with("Playlists could not be saved to the media folder — ")
    );
    assert!(next_cue.is_err());
}

#[test]
fn a_2000_file_library_scans_and_reorders() {
    let names: Vec<String> = (0..2000)
        .map(|n| match n % 4 {
            0 => format!("Motion/clip {n}.mp4"),
            1 => format!("Stills/still {n}.jpg"),
            2 => format!("Loop {n}.webm"),
            _ => format!("Christmas/x {n}.mov"),
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut f = fixture(&refs);
    let mut samples = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        let library = f.service.scan();
        samples.push(started.elapsed());
        assert_eq!(library.items.len(), 2000);
    }
    samples.sort();
    eprintln!(
        "2,000-file scan: median {:?}, max {:?}",
        samples[2], samples[4]
    );
    let library = f.service.library();
    assert_eq!(library.folders.len(), 4);
    assert!(library.folders.iter().all(|x| x.count == 500.0));

    let mut reversed: Vec<String> = library.items.iter().map(|i| i.id.clone()).collect();
    reversed.reverse();
    let started = Instant::now();
    f.service.set_item_order(reversed.clone());
    eprintln!(
        "2,000-item reorder and manifest save: {:?}",
        started.elapsed()
    );
    assert_eq!(ids(&f), reversed);
    assert_eq!(
        MediaService::new(f.settings.clone()).scan().items.len(),
        2000
    );
}
