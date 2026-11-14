use kairo_core::media_access::media_path_allowed;
use serde_json::json;
use std::path::Path;
#[test]
fn configured_and_picked_paths_are_exact_and_library_roots_are_realpaths() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("media");
    let sibling = dir.path().join("media-private");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();
    let file = root.join("new.mp4");
    let private = sibling.join("private.mp4");
    std::fs::write(&file, "a").unwrap();
    std::fs::write(&private, "a").unwrap();
    let settings = json!({"overlay":{"theme":{"background":{"mediaPath":"/picked/logo.png"}}}});
    let picked = |p: &Path| p == Path::new("/chosen/other.png");
    assert!(media_path_allowed(
        &file,
        &settings,
        &picked,
        std::slice::from_ref(&root)
    ));
    assert!(!media_path_allowed(
        &private,
        &settings,
        &picked,
        std::slice::from_ref(&root)
    ));
    assert!(!media_path_allowed(
        &root.join("missing.mp4"),
        &settings,
        &picked,
        std::slice::from_ref(&root)
    ));
    assert!(media_path_allowed(
        Path::new("/picked/logo.png"),
        &settings,
        &picked,
        &[]
    ));
    assert!(media_path_allowed(
        Path::new("/chosen/other.png"),
        &settings,
        &picked,
        &[]
    ));
    assert!(!media_path_allowed(
        Path::new("/chosen/no.png"),
        &settings,
        &picked,
        &[]
    ));
    #[cfg(unix)]
    {
        let link = root.join("escape.mp4");
        std::os::unix::fs::symlink(&private, &link).unwrap();
        assert!(!media_path_allowed(&link, &settings, &picked, &[root]));
    }
}
