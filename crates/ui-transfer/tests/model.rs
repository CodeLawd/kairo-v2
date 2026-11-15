use kairo_domain::kairo_bundle::{
    KairoBundleKind, PlaylistImportChoice, SongImportChoice, TransferCommitResult,
    TransferExportRequest, TransferPlaylistPreview, TransferPreview, TransferSetlistPreview,
    TransferSongPreview,
};
use kairo_domain::lyrics::{DuplicateMatch, DuplicateReason};
use ui_transfer::model::*;

fn song(id: &str, duplicate: bool) -> TransferSongPreview {
    TransferSongPreview {
        id: id.into(),
        title: format!("Song {id}"),
        artist: String::new(),
        section_count: 2,
        duplicate: duplicate.then(|| DuplicateMatch {
            song_id: "lib".into(),
            title: "Song".into(),
            artist: String::new(),
            reason: DuplicateReason::TitleArtist,
        }),
    }
}

fn preview(songs: Vec<TransferSongPreview>) -> TransferPreview {
    TransferPreview {
        token: "t".into(),
        file_name: "Songs.kairo".into(),
        kind: KairoBundleKind::Songs,
        name: "Songs".into(),
        exported_at: 0.,
        songs,
        setlist: None,
        playlist: None,
        skipped_files: None,
    }
}

#[test]
fn new_songs_ticked_duplicates_kept() {
    let r = ImportReview::new(preview(vec![song("a", false), song("b", true)]));
    assert_eq!(r.choice("a"), SongImportChoice::Add);
    assert_eq!(r.choice("b"), SongImportChoice::Skip);
    assert_eq!(r.counts(), (1, 0));
    assert_eq!(r.import_label(), "Import 1 song");
    assert_eq!(r.summary(), "2 songs · 1 new · 1 already in your library");
    assert!(!r.all_already_here());
}

#[test]
fn bulk_applies_to_duplicates_only() {
    let mut r = ImportReview::new(preview(vec![
        song("a", false),
        song("b", true),
        song("c", true),
    ]));
    r.set_choice("a", SongImportChoice::Skip);
    r.apply_bulk(DuplicateChoice::Replace);
    assert_eq!(r.choice("a"), SongImportChoice::Skip);
    assert_eq!(r.counts(), (0, 2));
    assert_eq!(r.import_label(), "Import 2 songs");
    r.apply_bulk(DuplicateChoice::Skip);
    assert!(r.nothing_to_do());
    assert_eq!(r.import_label(), "Nothing to import");
}

#[test]
fn all_duplicates_and_setlist() {
    let mut p = preview(vec![song("a", true)]);
    p.setlist = Some(TransferSetlistPreview {
        name: "Sunday".into(),
        song_count: 1,
    });
    let mut r = ImportReview::new(p);
    assert!(r.all_already_here());
    assert!(r.already_here_text().starts_with("This song is already"));
    assert!(r.create_setlist);
    assert!(!r.nothing_to_do());
    assert_eq!(r.import_label(), "Create playlist");
    r.create_setlist = false;
    assert!(r.nothing_to_do());
    let req = r.request();
    assert!(req.playlist.is_none());
    assert_eq!(req.songs["a"], SongImportChoice::Skip);
}

#[test]
fn playlist_defaults_and_skip() {
    let mut p = preview(Vec::new());
    p.kind = KairoBundleKind::ScripturePlaylist;
    p.playlist = Some(TransferPlaylistPreview {
        title: "Advent".into(),
        item_count: 3,
        exists: true,
    });
    let mut r = ImportReview::new(p.clone());
    assert_eq!(r.playlist, PlaylistImportChoice::KeepBoth);
    assert_eq!(r.import_label(), "Import playlist");
    r.playlist = PlaylistImportChoice::Skip;
    assert!(r.nothing_to_do());
    assert_eq!(r.request().playlist, Some(PlaylistImportChoice::Skip));
    if let Some(pl) = p.playlist.as_mut() {
        pl.exists = false;
    }
    assert_eq!(ImportReview::new(p).playlist, PlaylistImportChoice::Replace);
}

#[test]
fn describe_commit_lines() {
    let mut r = TransferCommitResult {
        added: 0,
        replaced: 0,
        skipped: 0,
        setlist_name: None,
        playlist_title: None,
    };
    assert_eq!(describe_commit(&r), "Nothing was imported.");
    r.added = 1;
    assert_eq!(describe_commit(&r), "1 song added.");
    r.added = 3;
    r.replaced = 2;
    r.setlist_name = Some("Sunday".into());
    assert_eq!(
        describe_commit(&r),
        "3 songs added, 2 replaced, playlist “Sunday” created."
    );
    r = TransferCommitResult {
        added: 0,
        replaced: 0,
        skipped: 0,
        setlist_name: None,
        playlist_title: Some("Advent".into()),
    };
    assert_eq!(describe_commit(&r), "Playlist “Advent” added.");
}

fn row(id: &str, title: &str, artist: &str) -> SongRow {
    SongRow {
        id: id.into(),
        title: title.into(),
        artist: artist.into(),
    }
}

#[test]
fn picker_selection_and_request() {
    let songs = vec![
        row("2", "beta", "X"),
        row("1", "Alpha", ""),
        row("3", "Gamma", "Y"),
    ];
    let mut p = ExportPicker::new(songs, &["3".into()]);
    assert_eq!(p.songs[0].title, "Alpha");
    assert_eq!(p.count(), 1);
    assert_eq!(p.name_placeholder(), "File name (optional)");
    assert_eq!(p.export_label(), "Export 1 song…");
    p.query = "x".into();
    assert_eq!(p.visible().len(), 1);
    assert_eq!(p.select_label(), "Select matches");
    p.toggle_all_visible();
    assert_eq!(p.select_label(), "Clear");
    p.query.clear();
    assert_eq!(p.select_label(), "Select all");
    p.toggle_all_visible();
    assert!(p.everything());
    assert_eq!(p.name_placeholder(), "All songs");
    assert_eq!(p.request("  "), TransferExportRequest::Library);
    assert_eq!(
        p.request(" Mine "),
        TransferExportRequest::Songs {
            song_ids: vec!["1".into(), "2".into(), "3".into()],
            name: Some("Mine".into())
        }
    );
    p.toggle("1");
    p.toggle("2");
    p.toggle("3");
    assert_eq!(p.count(), 0);
    assert_eq!(p.export_label(), "Export songs…");
    p.query = "zzz".into();
    assert_eq!(p.empty_text(), "No songs match.");
    assert_eq!(
        ExportPicker::new(Vec::new(), &[]).empty_text(),
        "Your song library is empty."
    );
}

#[test]
fn picker_keeps_ticks_when_library_changes() {
    let mut p = ExportPicker::new(vec![row("1", "A", "")], &["1".into(), "gone".into()]);
    assert_eq!(p.count(), 1);
    p.set_songs(vec![row("1", "A", ""), row("2", "B", "")]);
    assert_eq!(p.count(), 1);
    assert!(!p.everything());
}

#[test]
fn exported_label_without_date() {
    let p = preview(Vec::new());
    assert_eq!(exported_label(&p), "Songs.kairo");
    let mut dated = p;
    dated.exported_at = 1_700_000_000_000.;
    assert!(exported_label(&dated).starts_with("Songs.kairo · exported "));
    assert!(exported_label(&dated).ends_with("2023"));
}
