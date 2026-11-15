//! `kairo-release`: the native release flow (prompt 104), run from the
//! repository root.
//!
//!   kairo-release prepare <patch|minor|major|X.Y.Z> [--skip-changelog]
//!       The local release, as `scripts/release.mjs`: checks, bump, commit,
//!       tag, push, then starts `.github/workflows/release-native.yml`.
//!   kairo-release check-tag --tag vX.Y.Z [--allow-test] [--electron-latest vA.B.C] [--skip-changelog]
//!       Tag, workspace version, version sequence and changelog. Prints the version.
//!   kairo-release decklink
//!       The committed DeckLink helper matches its sources (and is universal, on macOS).
//!   kairo-release mac-feed --dir DIR --version X.Y.Z --release-date ISO
//!       Writes `latest-mac.yml` for both architectures.
//!   kairo-release verify --dir DIR --version X.Y.Z
//!       The staged release is complete and consistent.
//!   kairo-release sha256sums --dir DIR
//!       Writes `sha256sums.txt`.
//!   kairo-release notes --version X.Y.Z [--internal]
//!       Release notes from the changelog entry.

use kairo_release::*;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const WORKFLOW: &str = "release-native.yml";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        return fail(
            "usage: kairo-release <prepare|check-tag|decklink|mac-feed|verify|sha256sums|notes> …",
        );
    };
    let opts = Opts::parse(rest);
    let result = match command.as_str() {
        "prepare" => prepare(&opts),
        "check-tag" => check_tag(&opts),
        "decklink" => decklink(&opts.root()),
        "mac-feed" => mac_feed(&opts),
        "verify" => verify(&opts),
        "sha256sums" => sha256sums(&opts),
        "notes" => notes(&opts),
        other => Err(format!("unknown command \"{other}\"")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => fail(&message),
    }
}

fn fail(message: &str) -> ExitCode {
    eprintln!("\n✗ {message}\n");
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        // One annotation per line keeps the run summary readable.
        for line in message.lines().filter(|l| !l.trim().is_empty()) {
            eprintln!("::error::{line}");
        }
    }
    ExitCode::FAILURE
}

struct Opts {
    positional: Vec<String>,
    pairs: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Opts {
    fn parse(args: &[String]) -> Self {
        let valued = [
            "--tag",
            "--electron-latest",
            "--dir",
            "--version",
            "--release-date",
            "--root",
        ];
        let mut opts = Opts {
            positional: Vec::new(),
            pairs: Vec::new(),
            flags: Vec::new(),
        };
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if valued.contains(&arg.as_str()) {
                opts.pairs
                    .push((arg.clone(), iter.next().cloned().unwrap_or_default()));
            } else if arg.starts_with("--") {
                opts.flags.push(arg.clone());
            } else {
                opts.positional.push(arg.clone());
            }
        }
        opts
    }
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    fn need(&self, key: &str) -> Result<&str, String> {
        self.get(key)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("{key} is required"))
    }
    fn flag(&self, key: &str) -> bool {
        self.flags.iter().any(|f| f == key)
    }
    fn root(&self) -> PathBuf {
        PathBuf::from(self.get("--root").unwrap_or("."))
    }
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

fn today() -> Result<String, String> {
    capture("date", &["+%Y-%m-%d"])
}

fn changelog(root: &Path, version: &str, skip: bool) -> Result<(), String> {
    if skip {
        println!("  changelog: skipped (internal-only release)");
        return Ok(());
    }
    let json = read(&root.join(CHANGELOG_JSON))?;
    match changelog_entry_date(&json, version)? {
        None => Err(format!(
            "{CHANGELOG_JSON} has no entry for v{version}.\n\
             Add one to website/src/lib/changelog.ts in kairo-presenter (bun run changelog:draft {version} drafts it),\n\
             then run `cargo run -p electron-sync -- --apply-generated` (or `cargo run -p embed-changelog -- --regenerate`) and commit the JSON.\n\
             For a release with nothing a church would notice, use --skip-changelog."
        )),
        Some(date) => {
            if let Ok(today) = today()
                && date != today
            {
                eprintln!(
                    "  ! The changelog entry for v{version} is dated {date}, not today ({today})."
                );
            }
            println!("  changelog: v{version} dated {date}");
            Ok(())
        }
    }
}

fn check_tag(opts: &Opts) -> Result<(), String> {
    let root = opts.root();
    let (version, test) = version_from_tag(opts.need("--tag")?, opts.flag("--allow-test"))?;
    let manifest = workspace_version(&read(&root.join("Cargo.toml"))?)?;
    if manifest != version.to_string() {
        return Err(format!(
            "the tag names {version} but the workspace version is {manifest}; run `kairo-release prepare`, which bumps both"
        ));
    }
    match opts.get("--electron-latest") {
        Some(latest) if !latest.is_empty() => check_native_sequence(&version, latest)?,
        _ => {
            if !test {
                return Err("--electron-latest is required for a real release".into());
            }
            if version.major < NATIVE_MAJOR {
                return Err(format!(
                    "native versions start at {NATIVE_MAJOR}.0.0 (got {version})"
                ));
            }
        }
    }
    changelog(&root, &version.to_string(), opts.flag("--skip-changelog"))?;
    println!("{version}");
    Ok(())
}

fn decklink(root: &Path) -> Result<(), String> {
    check_decklink(root)?;
    if cfg!(target_os = "macos") {
        let archs = capture(
            "lipo",
            &["-archs", &root.join(DECKLINK_PREBUILT).to_string_lossy()],
        )?;
        check_universal(&archs)?;
    }
    println!("  DeckLink helper: {DECKLINK_PREBUILT} matches its sources");
    Ok(())
}

fn mac_feed(opts: &Opts) -> Result<(), String> {
    let version = parse_release_version(opts.need("--version")?)?;
    let yaml = write_mac_feed(
        Path::new(opts.need("--dir")?),
        &version,
        opts.need("--release-date")?,
    )?;
    print!("{yaml}");
    Ok(())
}

fn verify(opts: &Opts) -> Result<(), String> {
    let version = parse_release_version(opts.need("--version")?)?;
    let dir = Path::new(opts.need("--dir")?);
    verify_stage(dir, &version).map_err(|errors| {
        format!(
            "the release is incomplete or inconsistent:\n{}",
            errors
                .iter()
                .map(|e| format!("  - {e}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    })?;
    println!("  release v{version} complete: every asset, feed, hash and download name checks out");
    Ok(())
}

fn sha256sums(opts: &Opts) -> Result<(), String> {
    let dir = Path::new(opts.need("--dir")?);
    let manifest = sha256_manifest(dir)?;
    std::fs::write(dir.join("sha256sums.txt"), &manifest)
        .map_err(|e| format!("cannot write sha256sums.txt: {e}"))?;
    print!("{manifest}");
    Ok(())
}

fn notes(opts: &Opts) -> Result<(), String> {
    let version = parse_release_version(opts.need("--version")?)?;
    let json = read(&opts.root().join(CHANGELOG_JSON))?;
    let body = match changelog_notes(&json, &version.to_string())? {
        Some(text) if !opts.flag("--internal") => text,
        _ => format!(
            "Native build {version}, internal only. Not for general release (prompt 112).\n"
        ),
    };
    print!("{body}\nChecksums: sha256sums.txt\n");
    Ok(())
}

// ------------------------------------------------------------------ prepare

fn capture(cmd: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(cmd)
        .args(args)
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("cannot run {cmd}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{cmd} {} failed", args.join(" ")));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
    println!("  $ {cmd} {}", args.join(" "));
    let status = Command::new(cmd)
        .args(args)
        .status()
        .map_err(|e| format!("cannot run {cmd}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{cmd} {} failed", args.join(" ")))
    }
}

fn ask(question: &str) -> bool {
    print!("{question}");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    let _ = std::io::stdin().lock().read_line(&mut answer);
    matches!(answer.trim().to_lowercase().as_str(), "y" | "yes")
}

fn prepare(opts: &Opts) -> Result<(), String> {
    let root = opts.root();
    let bump = opts
        .positional
        .first()
        .map(String::as_str)
        .unwrap_or("patch");
    let skip_changelog = opts.flag("--skip-changelog");
    let git = |args: &[&str]| capture("git", args);

    // 1. Only release what is committed, so the build matches the tag.
    if !git(&["status", "--porcelain"])?.is_empty() {
        return Err(
            "You have uncommitted changes. Commit or stash them first, then run this again.".into(),
        );
    }
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        return Err("You are not on a branch (detached HEAD).".into());
    }
    // 2. This branch must have everything that is on GitHub.
    git(&["fetch", "origin", "--tags", "--quiet"])?;
    if let Ok(upstream) = git(&[
        "rev-parse",
        "--abbrev-ref",
        &format!("{branch}@{{upstream}}"),
    ]) && git(&["rev-list", "--count", &format!("HEAD..{upstream}")])? != "0"
    {
        return Err(format!(
            "{upstream} has commits you don't have. Run \"git pull\" first."
        ));
    }

    let cargo_toml = read(&root.join("Cargo.toml"))?;
    let current = workspace_version(&cargo_toml)?;
    let next = next_version(&current, bump)?;
    let tag = format!("v{next}");
    if !git(&["tag", "--list", &tag])?.is_empty() {
        return Err(format!("Tag {tag} already exists."));
    }
    let electron = capture("gh", &["api", &format!("repos/{RELEASE_REPO}/releases/latest"), "--jq", ".tag_name"])
        .map_err(|e| format!("cannot read the latest {RELEASE_REPO} release, so the version order cannot be checked: {e}"))?;
    check_native_sequence(&next, &electron)?;
    changelog(&root, &next.to_string(), skip_changelog)?;
    decklink(&root)?;

    // 3. Confirm.
    println!("\n  Native release Kairo v{current} → v{next} (latest Electron: {electron})");
    println!(
        "  from branch {branch}{}",
        if branch == "main" { "" } else { "  (not main)" }
    );
    println!("  It goes to {RELEASE_REPO} as a DRAFT pre-release only, until prompt 112.\n");
    if !ask("  Build this release? (y/N) ") {
        return Err("Cancelled. Nothing was changed.".into());
    }

    // 4. The same gates as CI, before anything leaves this machine.
    println!("\n→ Formatting, lints and tests");
    run("cargo", &["fmt", "--all", "--", "--check"])?;
    run(
        "cargo",
        &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(
        "cargo",
        &["test", "--locked", "--workspace", "--no-fail-fast"],
    )?;

    // 5. Bump the manifest and the lockfile, commit and tag.
    println!("\n→ Bumping to v{next}");
    std::fs::write(
        root.join("Cargo.toml"),
        set_workspace_version(&cargo_toml, &next.to_string())?,
    )
    .map_err(|e| format!("cannot write Cargo.toml: {e}"))?;
    let bumped = (|| {
        run("cargo", &["update", "--workspace", "--offline"])?;
        run("git", &["add", "Cargo.toml", "Cargo.lock"])?;
        run("git", &["commit", "-m", &format!("Release v{next}")])?;
        run(
            "git",
            &["tag", "-a", &tag, "-m", &format!("Release v{next}")],
        )
    })();
    if let Err(e) = bumped {
        let _ = run("git", &["checkout", "--", "Cargo.toml", "Cargo.lock"]);
        return Err(format!("{e}. The version bump was undone."));
    }

    // 6. Push the commit, then the tag, then start the release build.
    println!("\n→ Pushing");
    run("git", &["push", "-u", "origin", &branch])?;
    run("git", &["push", "origin", &tag])?;
    let mut workflow = vec!["workflow", "run", WORKFLOW, "--ref", tag.as_str(), "-f"];
    let tag_field = format!("release_tag={tag}");
    workflow.push(&tag_field);
    if skip_changelog {
        workflow.extend(["-f", "skip_changelog=true"]);
    }
    run("gh", &workflow)?;
    println!(
        "\n✓ v{next} is building. Progress: gh run list --workflow {WORKFLOW}\n  Draft: https://github.com/{RELEASE_REPO}/releases (drafts are visible to maintainers only)\n"
    );
    Ok(())
}
