//! Finds Electron changes since the last-synced commit and queues them for porting.
//!
//! Every git command here only reads. `--no-optional-locks` stops `git status`
//! from refreshing the index of a checkout the owner is working in.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

pub struct Options {
    pub root: PathBuf,
    pub electron: Option<PathBuf>,
    pub write: bool,
    pub advance: bool,
    pub apply_generated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    NotStarted,
    PortNeeded,
    NoImpact(&'static str),
    Unowned,
}

impl Class {
    fn label(self) -> String {
        match self {
            Self::NotStarted => "not started".into(),
            Self::PortNeeded => "port needed".into(),
            Self::NoImpact(reason) => format!("no native impact: {reason}"),
            Self::Unowned => "no owner".into(),
        }
    }

    /// The status a new queue row starts with.
    fn status(self) -> &'static str {
        match self {
            Self::NotStarted => "not-started",
            Self::NoImpact(_) => "no-impact",
            Self::PortNeeded | Self::Unowned => "open",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub is_new: bool,
    pub deleted: bool,
}

/// Maps each Electron path or directory to the prompts whose section 3 names it.
#[derive(Default)]
pub struct Owners(BTreeMap<String, BTreeSet<String>>);

impl Owners {
    pub fn add_prompt(&mut self, id: &str, markdown: &str) {
        let Some(start) = markdown.find("\n## 3.") else {
            return;
        };
        let section = &markdown[start + 1..];
        let section = &section[..section[3..].find("\n## ").map_or(section.len(), |e| e + 3)];
        for (index, part) in section.split('`').enumerate() {
            if index % 2 == 1
                && let Some(path) = part.strip_prefix("../kairo-presenter/")
            {
                let path = path.split('*').next().unwrap_or(path).trim_end_matches('/');
                self.0
                    .entry(path.to_owned())
                    .or_default()
                    .insert(id.to_owned());
            }
        }
    }

    pub fn of(&self, file: &str) -> BTreeSet<String> {
        self.0
            .iter()
            .filter(|(path, _)| {
                file == path.as_str()
                    || path.is_empty()
                    || file
                        .strip_prefix(path.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                    || (path.ends_with('-') && file.starts_with(path.as_str()))
            })
            .flat_map(|(_, ids)| ids.iter().cloned())
            .collect()
    }
}

fn no_impact_reason(file: &str) -> Option<&'static str> {
    const RULES: [(&str, &str); 6] = [
        ("server/", "backend"),
        ("website/", "marketing site"),
        ("docs/", "documentation"),
        (".github/", "Electron CI"),
        (".claude/", "agent tooling"),
        ("release/", "build output"),
    ];
    RULES
        .iter()
        .find(|(prefix, _)| file.starts_with(prefix))
        .map(|(_, reason)| *reason)
}

pub fn classify(owners: &BTreeSet<String>, done: &BTreeSet<String>, file: &str) -> Class {
    if owners.iter().any(|id| done.contains(id)) {
        Class::PortNeeded
    } else if !owners.is_empty() {
        Class::NotStarted
    } else if let Some(reason) = no_impact_reason(file) {
        Class::NoImpact(reason)
    } else {
        Class::Unowned
    }
}

/// High-risk paths, and a path heuristic for stored formats. A format change
/// inside an ordinary file is not detected; the report says so.
pub fn high_risk(file: &str) -> bool {
    const EXACT: [&str; 4] = [
        "src/lib/ipc.ts",
        "src/lib/program.ts",
        "src/lib/workspace.ts",
        "src/lib/kairo-bundle.ts",
    ];
    const NAMES: [&str; 3] = ["overlay-template.ts", "overlay.html", "detector.ts"];
    let lower = file.to_ascii_lowercase();
    EXACT.contains(&file)
        || file.starts_with("src/main/db/")
        || NAMES.iter().any(|name| file.ends_with(name))
        || lower.contains("migration")
        || lower.contains("schema")
        || lower.ends_with(".sql")
}

fn git(repo: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("--no-optional-locks")
        .args(["-c", "core.quotepath=off", "-C"])
        .arg(repo)
        .args(args)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("git {} was not UTF-8", args.join(" ")))
}

pub fn last_synced(sync: &str) -> Result<String, String> {
    let line = sync
        .lines()
        .find(|line| line.starts_with("**Last-synced commit:**"))
        .ok_or("SYNC.md has no **Last-synced commit:** line")?;
    let commit = line.split('`').nth(1).unwrap_or("").trim();
    if commit.is_empty() || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("SYNC.md has an invalid last-synced commit: {line}"));
    }
    Ok(commit.to_owned())
}

/// Parses `git log --format=%x01%h --name-status -M`.
pub fn parse_log(log: &str) -> Vec<(String, Vec<Change>)> {
    let mut commits: Vec<(String, Vec<Change>)> = Vec::new();
    for line in log.lines().filter(|line| !line.is_empty()) {
        if let Some(hash) = line.strip_prefix('\u{1}') {
            commits.push((hash.to_owned(), Vec::new()));
        } else if let Some((_, changes)) = commits.last_mut() {
            let fields: Vec<_> = line.split('\t').collect();
            let status = fields[0].chars().next().unwrap_or('M');
            let Some(path) = fields.last() else { continue };
            changes.push(Change {
                path: (*path).to_owned(),
                is_new: matches!(status, 'A' | 'R' | 'C'),
                deleted: status == 'D',
            });
        }
    }
    commits
}

/// Parses `git status --porcelain=v1 -uall -z`, where renames take two fields.
pub fn parse_status(status: &str) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut fields = status.split('\0').filter(|field| !field.is_empty());
    while let Some(entry) = fields.next() {
        let Some((code, path)) = entry.split_at_checked(3) else {
            continue;
        };
        let renamed = code.contains('R') || code.contains('C');
        if renamed {
            fields.next();
        }
        changes.push(Change {
            path: path.to_owned(),
            is_new: renamed || code.starts_with("??") || code.contains('A'),
            deleted: code.contains('D'),
        });
    }
    changes
}

fn cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().trim_matches('`').to_owned())
        .collect()
}

/// Rows of the markdown table that follows `heading`, and its last line index.
fn table(sync: &str, heading: &str) -> Result<(Vec<Vec<String>>, usize), String> {
    let lines: Vec<_> = sync.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim() == heading)
        .ok_or(format!("SYNC.md has no {heading} section"))?;
    let first = (start + 1..lines.len())
        .find(|&i| lines[i].starts_with('|'))
        .ok_or(format!("SYNC.md has no table under {heading}"))?;
    let mut end = first;
    while end + 1 < lines.len() && lines[end + 1].starts_with('|') {
        end += 1;
    }
    let rows = if first + 2 <= end {
        lines[first + 2..=end].iter().map(|l| cells(l)).collect()
    } else {
        Vec::new()
    };
    Ok((rows, end))
}

/// Status of each prompt in PROGRESS.md.
pub fn done_prompts(progress: &str) -> BTreeSet<String> {
    progress
        .lines()
        .filter(|line| line.starts_with("| ") && line.contains("](./prompts/"))
        .filter_map(|line| {
            let row = cells(line);
            (row.get(2).map(String::as_str) == Some("done")).then(|| row[0].clone())
        })
        .collect()
}

/// The newest commit whose items, and all earlier ones, are resolved.
pub fn advance_target(commits: &[(String, Vec<Change>)], queue: &[Vec<String>]) -> Option<String> {
    let mut target = None;
    for (hash, changes) in commits {
        let resolved = changes.iter().all(|change| {
            queue.iter().any(|row| {
                row[0] == *hash
                    && row[1] == change.path
                    && matches!(row[4].as_str(), "done" | "no-impact" | "not-started")
            })
        });
        if !resolved {
            break;
        }
        target = Some(hash.clone());
    }
    target
}

fn check_copies(sync: &str, electron: &Path, root: &Path, out: &mut String) -> Result<(), String> {
    let (rows, _) = table(sync, "## Copied native sources")?;
    writeln!(out, "\n## Copied native sources\n").ok();
    if rows.is_empty() {
        writeln!(out, "None recorded.").ok();
    }
    for row in &rows {
        let [here, there, commit, ..] = row.as_slice() else {
            return Err(format!("malformed copied-source row: {row:?}"));
        };
        let Ok(local) = fs::read_to_string(root.join(here)) else {
            writeln!(out, "- MISSING HERE: {here}").ok();
            continue;
        };
        let Ok(original) = fs::read_to_string(electron.join(there)) else {
            writeln!(out, "- MISSING IN ELECTRON: {there} (for {here})").ok();
            continue;
        };
        let state = if copied(&local, &original) {
            "in sync".to_owned()
        } else {
            match git(electron, &["show", &format!("{commit}:{there}")]) {
                Ok(source) if copied(&local, &source) => "DRIFT: Electron changed".to_owned(),
                Ok(_) => "DRIFT: this copy was edited".to_owned(),
                Err(_) => format!("DRIFT: source commit {commit} is unreadable"),
            }
        };
        writeln!(out, "- {state}: {here}").ok();
    }
    Ok(())
}

/// Partial copies count, as some rows hold one section of a larger file.
fn copied(local: &str, original: &str) -> bool {
    let tidy = |text: &str| text.replace("\r\n", "\n").trim().to_owned();
    tidy(original).contains(&tidy(local))
}

fn is_implemented(root: &Path, package: &str) -> bool {
    fs::read_to_string(root.join("tools").join(package).join("src/main.rs"))
        .map(|source| !source.contains("fn main() {}"))
        .unwrap_or(false)
}

fn apply_generated(
    root: &Path,
    electron: &Path,
    changed: &BTreeSet<String>,
    out: &mut String,
) -> Result<(), String> {
    const GENERATORS: [(&str, &str, &str); 4] = [
        ("api-inventory.md", "check-api-coverage", "010"),
        ("oracle cache", "ts-oracle", "011"),
        ("lib coverage", "check-lib-coverage", "016"),
        ("embedded changelog", "embed-changelog", "071"),
    ];
    writeln!(out, "\n## Generated files\n").ok();
    for (name, package, prompt) in GENERATORS {
        if !is_implemented(root, package) {
            writeln!(
                out,
                "- SKIPPED {name}: tools/{package} is not built yet (prompt {prompt})"
            )
            .ok();
            continue;
        }
        let status = Command::new("cargo")
            .args(["run", "--quiet", "--package", package, "--", "--regenerate"])
            .env("KAIRO_ELECTRON_DIR", electron)
            .current_dir(root)
            .status()
            .map_err(|error| format!("could not run {package}: {error}"))?;
        if !status.success() {
            return Err(format!("{package} --regenerate failed: {status}"));
        }
        writeln!(out, "- regenerated {name}").ok();
    }
    /// Sources the text golden is rendered from.
    const TEXT_GOLDEN_SOURCES: [&str; 3] =
        ["overlay-template.ts", "overlay.html", "theme-library.ts"];
    writeln!(out, "\n## Goldens to recapture\n").ok();
    if changed
        .iter()
        .any(|file| TEXT_GOLDEN_SOURCES.iter().any(|name| file.ends_with(name)))
    {
        writeln!(out, "- tests/golden/003-text").ok();
    } else {
        writeln!(out, "None.").ok();
    }
    Ok(())
}

/// Owners as a queue cell: comma separated, or `none`.
fn owner_list(owners: &BTreeSet<String>) -> String {
    if owners.is_empty() {
        "none".into()
    } else {
        owners.iter().cloned().collect::<Vec<_>>().join(", ")
    }
}

/// Each prompt's id (the first three characters of its file name) and path.
pub fn prompt_files(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let entries = fs::read_dir(root.join("native-migration/prompts"))
        .map_err(|e| format!("cannot read prompts: {e}"))?;
    let mut prompts = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.ends_with(".md") && name.len() > 3 {
            prompts.push((name[..3].to_owned(), path.clone()));
        }
    }
    if prompts.is_empty() {
        return Err("no prompts found".into());
    }
    Ok(prompts)
}

fn describe(change: &Change, owners: &BTreeSet<String>, class: Class) -> String {
    let mut flags = Vec::new();
    if high_risk(&change.path) {
        flags.push("HIGH RISK");
    }
    if change.is_new {
        flags.push("NEW FILE");
    }
    if change.deleted {
        flags.push("DELETED");
    }
    if owners.is_empty() && !matches!(class, Class::NoImpact(_)) {
        flags.push("NO OWNER");
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!(" [{}]", flags.join(", "))
    };
    format!(
        "{} — {} — owners: {}{flags}",
        change.path,
        class.label(),
        owner_list(owners)
    )
}

pub fn run(options: &Options) -> Result<String, String> {
    let root = &options.root;
    let migration = root.join("native-migration");
    let electron = options
        .electron
        .clone()
        .or_else(|| std::env::var_os("KAIRO_ELECTRON_DIR").map(PathBuf::from))
        .unwrap_or_else(|| root.join("../kairo-presenter"));
    let sync_path = migration.join("SYNC.md");
    let sync = fs::read_to_string(&sync_path)
        .map_err(|e| format!("cannot read {}: {e}", sync_path.display()))?;
    let progress = fs::read_to_string(migration.join("PROGRESS.md"))
        .map_err(|e| format!("cannot read PROGRESS.md: {e}"))?;
    if !electron.is_dir() {
        return Err(format!(
            "Electron checkout not found at {}",
            electron.display()
        ));
    }
    let last = last_synced(&sync)?;
    git(
        &electron,
        &["rev-parse", "--verify", &format!("{last}^{{commit}}")],
    )
    .map_err(|_| format!("last-synced commit {last} is not in the Electron repository"))?;

    let mut owners = Owners::default();
    for (id, path) in prompt_files(root)? {
        owners.add_prompt(&id, &fs::read_to_string(&path).map_err(|e| e.to_string())?);
    }
    let done = done_prompts(&progress);

    let head = git(&electron, &["rev-parse", "--short=10", "HEAD"])?;
    let log = git(
        &electron,
        &[
            "log",
            "--reverse",
            "-m",
            "--first-parent",
            "-M",
            "--format=%x01%h",
            "--abbrev=10",
            "--name-status",
            &format!("{last}..HEAD"),
        ],
    )?;
    let commits = parse_log(&log);
    let status = git(&electron, &["status", "--porcelain=v1", "-uall", "-z"])?;
    let uncommitted = parse_status(&status);

    let mut out = String::new();
    writeln!(out, "# Electron sync\n\nLast synced: {last}. Electron HEAD: {}.\nStored-format detection is by path only: review diffs for format changes in ordinary files.", head.trim()).ok();
    writeln!(out, "\n## Committed since last sync\n").ok();
    if commits.is_empty() {
        writeln!(out, "None.").ok();
    }
    let (queue, end) = table(&sync, "## Queue")?;
    let mut additions = Vec::new();
    for (hash, changes) in &commits {
        writeln!(
            out,
            "- {hash}{}",
            if changes.is_empty() {
                " (no file changes)"
            } else {
                ""
            }
        )
        .ok();
        for change in changes {
            let who = owners.of(&change.path);
            let class = classify(&who, &done, &change.path);
            writeln!(out, "  - {}", describe(change, &who, class)).ok();
            if !queue
                .iter()
                .any(|row| row[0] == *hash && row[1] == change.path)
            {
                additions.push(vec![
                    hash.clone(),
                    change.path.clone(),
                    owner_list(&who),
                    class.label(),
                    class.status().to_owned(),
                    String::new(),
                ]);
            }
        }
    }
    writeln!(out, "\n## Uncommitted (never recorded as synced)\n").ok();
    if uncommitted.is_empty() {
        writeln!(out, "None.").ok();
    }
    for change in &uncommitted {
        let who = owners.of(&change.path);
        writeln!(
            out,
            "- {}",
            describe(change, &who, classify(&who, &done, &change.path))
        )
        .ok();
    }
    check_copies(&sync, &electron, root, &mut out)?;
    if options.apply_generated {
        let changed: BTreeSet<_> = commits
            .iter()
            .flat_map(|(_, c)| c)
            .chain(&uncommitted)
            .map(|c| c.path.clone())
            .collect();
        apply_generated(root, &electron, &changed, &mut out)?;
    }

    let mut text = sync.clone();
    if options.write && !additions.is_empty() {
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let rows = additions.iter().map(|row| {
            format!(
                "| `{}` | `{}` | {} | {} | {} | {} |",
                row[0], row[1], row[2], row[3], row[4], row[5]
            )
        });
        lines.splice(end + 1..end + 1, rows);
        text = lines.join("\n") + "\n";
        writeln!(out, "\nAdded {} queue rows.", additions.len()).ok();
    }
    if options.advance {
        let combined: Vec<Vec<String>> = queue
            .iter()
            .cloned()
            .chain(additions.iter().filter(|_| options.write).cloned())
            .collect();
        match advance_target(&commits, &combined) {
            Some(target) => {
                text = text.replace(
                    &format!("**Last-synced commit:** `{last}`"),
                    &format!("**Last-synced commit:** `{target}`"),
                );
                writeln!(out, "\nAdvanced last-synced to {target}.").ok();
            }
            None => {
                writeln!(out, "\nLast-synced not advanced: nothing to advance to, or the next commit has unresolved items.").ok();
            }
        }
    }
    if text != sync {
        fs::write(&sync_path, text).map_err(|e| format!("cannot write SYNC.md: {e}"))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn section_three_only_and_directories_match_on_boundaries() {
        let mut owners = Owners::default();
        owners.add_prompt("030", "`../kairo-presenter/a.ts`\n\n## 3. Files\n- `../kairo-presenter/server/src/auth` x\n\n## 4. Next\n`../kairo-presenter/b.ts`");
        assert_eq!(owners.of("server/src/auth/a.ts"), set(&["030"]));
        assert!(owners.of("server/src/authx.ts").is_empty());
        assert!(owners.of("a.ts").is_empty());
        assert!(owners.of("b.ts").is_empty());
    }

    #[test]
    fn classes() {
        let done = set(&["003"]);
        assert_eq!(
            classify(&set(&["003", "052"]), &done, "x"),
            Class::PortNeeded
        );
        assert_eq!(classify(&set(&["052"]), &done, "x"), Class::NotStarted);
        assert_eq!(
            classify(&set(&[]), &done, "website/a"),
            Class::NoImpact("marketing site")
        );
        assert_eq!(classify(&set(&[]), &done, "src/new.ts"), Class::Unowned);
    }

    #[test]
    fn high_risk_paths() {
        for path in [
            "src/lib/ipc.ts",
            "src/main/db/a.ts",
            "src/lib/overlay-template.ts",
            "src/main/services/output/overlay.html",
            "a/detector.ts",
            "x/migrations/1.sql",
        ] {
            assert!(high_risk(path), "{path}");
        }
        assert!(!high_risk("src/lib/web.ts"));
    }

    #[test]
    fn parses_renames_and_paths_with_spaces() {
        let changes = parse_status("R  new name.ts\0old.ts\0?? a b.ts\0 M c.ts\0 D d.ts\0");
        assert_eq!(changes.len(), 4);
        assert!(changes[0].is_new && changes[0].path == "new name.ts");
        assert!(changes[1].is_new && changes[1].path == "a b.ts");
        assert!(!changes[2].is_new);
        assert!(changes[3].deleted);
        let log = parse_log("\u{1}abc\n\nM\ta.ts\nR100\told.ts\tnew.ts\n\u{1}def\n");
        assert_eq!(log[0].1[1].path, "new.ts");
        assert!(log[1].1.is_empty());
    }

    #[test]
    fn rejects_bad_sync_files() {
        assert!(last_synced("nothing").is_err());
        assert!(last_synced("**Last-synced commit:** ``").is_err());
        assert_eq!(
            last_synced("**Last-synced commit:** `abc123`").unwrap(),
            "abc123"
        );
    }

    #[test]
    fn advance_stops_at_first_unresolved_commit() {
        let change = |p: &str| Change {
            path: p.into(),
            is_new: false,
            deleted: false,
        };
        let commits = vec![
            ("a".to_string(), vec![change("x")]),
            ("b".to_string(), vec![change("y")]),
            ("c".to_string(), vec![change("z")]),
        ];
        let row = |c: &str, f: &str, s: &str| {
            vec![
                c.to_string(),
                f.to_string(),
                String::new(),
                String::new(),
                s.to_string(),
                String::new(),
            ]
        };
        let queue = vec![
            row("a", "x", "done"),
            row("b", "y", "open"),
            row("c", "z", "done"),
        ];
        assert_eq!(advance_target(&commits, &queue).as_deref(), Some("a"));
        assert_eq!(advance_target(&commits, &[]), None);
    }

    #[test]
    fn partial_copies_count_as_in_sync() {
        assert!(copied("b\n", "a\nb\nc\n"));
        assert!(!copied("x", "a\nb"));
    }
}
