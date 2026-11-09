use kairo_domain::{
    kairo_bundle::{
        KairoBundleKind, bundle_file_name, merge_kairo_bundles, parse_kairo_bundle, setlist_bundle,
        songs_bundle,
    },
    keyboard_shortcuts::{ShortcutKeyEvent, command_for_shortcut, shortcut_from_event},
    settings::{ShortcutBindings, ShortcutCommand},
};
use serde_json::json;

#[test]
fn combine_song_exports_once_but_keep_ordered_playlists_separate() {
    let song = |id: &str| {
        parse_kairo_bundle(
            &json!({"kairoSong":1,"id":id,"title":id,"sections":[]}).to_string(),
            42.0,
        )
        .unwrap()
        .songs
        .remove(0)
    };
    let a = song("a");
    let b = song("b");
    let list = setlist_bundle("Service".into(), vec![b.clone(), a.clone()], None, 42.0);
    assert_eq!(list.setlist.as_ref().unwrap().song_ids, ["b", "a"]);
    let first = songs_bundle("First".into(), vec![a.clone()], None, 1.0);
    let second = songs_bundle("Second".into(), vec![a, b], None, 2.0);
    assert!(merge_kairo_bundles(vec![first.clone(), list]).is_err());
    let merged = merge_kairo_bundles(vec![first, second]).unwrap();
    assert_eq!(merged.kind, KairoBundleKind::Songs);
    assert_eq!(merged.name, "2 songs");
    assert_eq!(
        merged
            .songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(merge_kairo_bundles(Vec::new()).is_err());
    assert_eq!(bundle_file_name("Ọ̀pẹ́ / Sunday"), "Ope-Sunday.kairo");
}

#[test]
fn clicker_defaults_can_be_overridden_and_modifiers_are_exact() {
    let mut bindings = ShortcutBindings::default();
    assert_eq!(
        command_for_shortcut("PageDown", &bindings),
        Some(ShortcutCommand::Next)
    );
    assert_eq!(
        command_for_shortcut("PageUp", &bindings),
        Some(ShortcutCommand::Previous)
    );
    bindings.next = Some(vec!["ArrowRight".into()]);
    assert_eq!(command_for_shortcut("PageDown", &bindings), None);
    bindings.clear = Some(Vec::new());
    assert_eq!(command_for_shortcut("Delete", &bindings), None);
    let mut event = ShortcutKeyEvent {
        key: "P",
        meta_key: true,
        ctrl_key: false,
        alt_key: false,
        shift_key: true,
    };
    assert_eq!(shortcut_from_event(&event).as_deref(), Some("Mod+Shift+p"));
    event.meta_key = false;
    event.ctrl_key = true;
    assert_eq!(shortcut_from_event(&event).as_deref(), Some("Mod+Shift+p"));
    event.key = "Shift";
    assert_eq!(shortcut_from_event(&event), None);
}
