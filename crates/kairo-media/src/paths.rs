//! Path helpers shared by the media and tracks libraries, matching Node's
//! `path` functions as `src/main/services/media/index.ts` uses them.

use std::path::{Component, Path, PathBuf};

/// Node's `path.resolve(p)`: absolute against the working directory, with `.`
/// and `..` removed lexically. Symlinks are not followed.
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
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `isInsideRoot`: true when `file_path` is `root` or sits below it.
/// Component-wise, so `/media/backgrounds-secret` is not inside `/media/backgrounds`.
pub fn is_inside_root(file_path: &Path, root: &Path) -> bool {
    resolve(file_path).starts_with(resolve(root))
}

/// `itemIdFor`: the path relative to the root, in posix form.
pub fn item_id_for(root: &Path, abs_path: &Path) -> String {
    let relative = abs_path
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .or_else(|_| {
            resolve(abs_path)
                .strip_prefix(resolve(root))
                .map(Path::to_path_buf)
        })
        .unwrap_or_else(|_| abs_path.to_path_buf());
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Node's `path.extname`, including the dot. A leading dot is not an extension.
pub fn extname(file_name: &str) -> &str {
    let base = basename(file_name);
    match base.rfind('.') {
        Some(0) | None => "",
        Some(i) => &base[i..],
    }
}

/// The final segment of a `/` or `\` separated path.
pub fn basename(path: &str) -> &str {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed)
}

/// File name without its extension, as `basename(name, extname(name))`.
pub fn stem(file_name: &str) -> &str {
    let base = basename(file_name);
    &base[..base.len() - extname(base).len()]
}

/// Lowercased extension without the dot.
pub fn ext_lower(file_name: &str) -> String {
    extname(file_name).trim_start_matches('.').to_lowercase()
}

/// `uniqueFileName`: `name.ext`, else `name 1.ext`, `name 2.ext`… The check
/// is against lowercased names already in the folder.
pub fn unique_file_name(
    file_name: &str,
    taken_lower: &std::collections::HashSet<String>,
) -> String {
    if !taken_lower.contains(&file_name.to_lowercase()) {
        return file_name.into();
    }
    let ext = extname(file_name);
    let stem = &file_name[..file_name.len() - ext.len()];
    let mut n = 1;
    loop {
        let candidate = format!("{stem} {n}{ext}");
        if !taken_lower.contains(&candidate.to_lowercase()) {
            return candidate;
        }
        n += 1;
    }
}

/// The name cleaning `createFolder` and `renameItem` share: drop `/\:*?"<>|`, trim.
pub fn clean_name(name: &str) -> String {
    let removed: String = kairo_domain::js::trim(name)
        .chars()
        .filter(|c| !"/\\:*?\"<>|".contains(*c))
        .collect();
    kairo_domain::js::trim(&removed).into()
}

/// Lowercased names of every entry in `dir`, for collision checks.
pub fn taken_names(dir: &Path) -> std::io::Result<std::collections::HashSet<String>> {
    std::fs::read_dir(dir)?
        .map(|entry| entry.map(|e| e.file_name().to_string_lossy().to_lowercase()))
        .collect()
}

/// `String.prototype.localeCompare` with the default (root) collation.
pub fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    thread_local! {
        static COLLATOR: std::cell::RefCell<feruca::Collator> = std::cell::RefCell::new(
            feruca::Collator::new(feruca::Tailoring::Cldr(feruca::Locale::Root), false, false),
        );
    }
    COLLATOR.with(|c| c.borrow_mut().collate(a, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn set(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_free_name_is_left_alone() {
        assert_eq!(
            unique_file_name("loop.mp4", &set(&["still.png"])),
            "loop.mp4"
        );
    }

    #[test]
    fn a_collision_gets_a_numeric_suffix() {
        assert_eq!(
            unique_file_name("loop.mp4", &set(&["loop.mp4"])),
            "loop 1.mp4"
        );
        assert_eq!(
            unique_file_name("loop.mp4", &set(&["loop.mp4", "loop 1.mp4"])),
            "loop 2.mp4"
        );
    }

    #[test]
    fn collision_check_is_case_insensitive() {
        assert_eq!(
            unique_file_name("Loop.mp4", &set(&["loop.mp4"])),
            "Loop 1.mp4"
        );
    }

    #[test]
    fn a_sibling_with_the_root_as_prefix_is_outside() {
        assert!(is_inside_root(Path::new("/m/bg/a.mp4"), Path::new("/m/bg")));
        assert!(is_inside_root(Path::new("/m/bg"), Path::new("/m/bg/")));
        assert!(!is_inside_root(
            Path::new("/m/bg-secret/a.mp4"),
            Path::new("/m/bg")
        ));
        assert!(!is_inside_root(
            Path::new("/m/bg/../x.mp4"),
            Path::new("/m/bg")
        ));
    }

    #[test]
    fn extname_matches_node() {
        assert_eq!(extname("a.mp4"), ".mp4");
        assert_eq!(extname(".bashrc"), "");
        assert_eq!(extname("a."), ".");
        assert_eq!(extname("a.b.c"), ".c");
        assert_eq!(stem("Motion/a.b.MP4"), "a.b");
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("  a/b:c*  "), "abc");
        assert_eq!(clean_name(" / "), "");
    }

    #[test]
    fn locale_compare_folds_case_before_tiebreaking() {
        // Expected order from Node 22: `names.sort((a, b) => a.localeCompare(b))`.
        let mut names = vec![
            "beta", "Alpha", "alpha", "Été", "ete", "10", "9", "loop 2", "loop 10", "Loop", "_x",
            "-y", "a b", "ab",
        ];
        names.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(
            names,
            [
                "_x", "-y", "10", "9", "a b", "ab", "alpha", "Alpha", "beta", "ete", "Été", "Loop",
                "loop 10", "loop 2"
            ]
        );
    }
}
