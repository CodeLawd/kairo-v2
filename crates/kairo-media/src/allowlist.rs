//! Overlay media files the operator picked one by one, outside any library
//! folder. From `src/main/services/ndi/media-allowlist.ts`.

use crate::paths::resolve;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Default)]
pub struct PickedMediaAllowlist {
    paths: Mutex<HashSet<PathBuf>>,
}

impl PickedMediaAllowlist {
    pub fn allow(&self, file_path: &Path) {
        self.paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(resolve(file_path));
    }

    pub fn is_allowed(&self, file_path: &Path) -> bool {
        self.paths
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&resolve(file_path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picked_file_is_allowed_under_any_spelling() {
        let list = PickedMediaAllowlist::default();
        list.allow(Path::new("/a/b/logo.svg"));
        assert!(list.is_allowed(Path::new("/a/b/../b/./logo.svg")));
        assert!(!list.is_allowed(Path::new("/a/b/other.svg")));
    }
}
