//! Authorize paths received from a web view or another external caller before
//! forwarding them to the engine. Engine IPC itself remains private to Kairo.
use kairo_domain::overlay_outputs::configured_media_paths;
use kairo_domain::program::presentation_media_paths;
use kairo_store::workspace::{is_under_allowed_folder, resolve};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn media_path_allowed(
    requested: &Path,
    settings: &Value,
    is_picked: &dyn Fn(&Path) -> bool,
    library_roots: &[PathBuf],
) -> bool {
    let requested = resolve(requested);
    let mut configured =
        configured_media_paths(settings.get("overlay"), settings.get("themeLibrary"));
    configured.extend(presentation_media_paths(
        settings.get("presentation").unwrap_or(&Value::Null),
    ));
    configured
        .iter()
        .any(|p| resolve(Path::new(p)) == requested)
        || is_picked(&requested)
        || is_under_allowed_folder(&requested, library_roots)
}
