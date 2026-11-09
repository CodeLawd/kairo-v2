//! Runs the tool against a scratch repository with one commit of each class.

use electron_sync::{Options, run};
use std::{fs, path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-C",
        ])
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn commit(repo: &Path, file: &str) {
    let path = repo.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, file).unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "-qm", file]);
}

const SYNC: &str = "**Last-synced commit:** `LAST`\n\n## Queue\n\n| Electron commit | File | Owning prompt | Class | Status | Native commit |\n|---|---|---|---|---|---|\n\n## Copied native sources\n\n| Path here | Electron path | Source commit | Notes |\n|---|---|---|---|\n| `copy.h` | `native/orig.h` | `LAST` | x |\n";

#[test]
fn queues_each_class_and_keeps_uncommitted_out() {
    let base = std::env::temp_dir().join(format!("electron-sync-test-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let electron = base.join("electron");
    let root = base.join("native");
    fs::create_dir_all(root.join("native-migration/prompts")).unwrap();
    fs::create_dir_all(&electron).unwrap();
    git(&electron, &["init", "-q"]);
    commit(&electron, "native/orig.h");
    commit(&electron, "src/lib/program.ts");
    let last = git(&electron, &["rev-parse", "--short=10", "HEAD"]);
    fs::write(root.join("copy.h"), "native/orig.h").unwrap();

    let migration = root.join("native-migration");
    let owns =
        |file: &str| format!("# P\n\n## 3. Files\n\n- `../kairo-presenter/{file}`\n\n## 4. Next\n");
    fs::write(migration.join("prompts/100-a.md"), owns("src/done.ts")).unwrap();
    fs::write(
        migration.join("prompts/101-b.md"),
        owns("src/lib/program.ts"),
    )
    .unwrap();
    fs::write(migration.join("PROGRESS.md"), "| 100 | [A](./prompts/100-a.md) | done | | | |\n| 101 | [B](./prompts/101-b.md) | todo | | | |\n").unwrap();
    fs::write(migration.join("SYNC.md"), SYNC.replace("LAST", &last)).unwrap();

    commit(&electron, "src/done.ts");
    commit(&electron, "website/page.tsx");
    fs::write(electron.join("src/lib/program.ts"), "changed").unwrap();
    git(&electron, &["add", "-A"]);
    git(&electron, &["commit", "-qm", "program"]);
    commit(&electron, "src/orphan.ts");
    fs::write(electron.join("native/orig.h"), "changed upstream").unwrap();
    fs::write(electron.join("src/draft.ts"), "wip").unwrap();
    let before = git(&electron, &["status", "--porcelain"]);

    let options = |write, advance| Options {
        root: root.clone(),
        electron: Some(electron.clone()),
        write,
        advance,
        apply_generated: true,
    };
    let report = run(&options(true, false)).unwrap();
    assert!(report.contains("src/done.ts — port needed"), "{report}");
    assert!(
        report.contains("website/page.tsx — no native impact: marketing site"),
        "{report}"
    );
    assert!(
        report.contains("src/lib/program.ts — not started — owners: 101 [HIGH RISK]"),
        "{report}"
    );
    assert!(
        report.contains("src/orphan.ts — no owner — owners: none [NEW FILE, NO OWNER]"),
        "{report}"
    );
    assert!(
        report.contains("DRIFT: Electron changed: copy.h"),
        "{report}"
    );
    assert!(report.contains("SKIPPED api-inventory.md"), "{report}");

    let sync = fs::read_to_string(migration.join("SYNC.md")).unwrap();
    let rows = sync.lines().filter(|line| line.starts_with("| `")).count();
    assert_eq!(rows - 1, 4, "{sync}");
    assert!(!sync.contains("draft.ts"));
    let (uncommitted, _) = report.split_once("## Copied").unwrap();
    assert!(
        uncommitted
            .split("## Uncommitted")
            .nth(1)
            .unwrap()
            .contains("src/draft.ts")
    );

    run(&options(true, false)).unwrap();
    assert_eq!(
        fs::read_to_string(migration.join("SYNC.md")).unwrap(),
        sync,
        "not idempotent"
    );

    // The first commit is `open`, so nothing may advance.
    run(&options(true, true)).unwrap();
    assert!(
        fs::read_to_string(migration.join("SYNC.md"))
            .unwrap()
            .contains(&format!("`{last}`\n"))
    );
    assert_eq!(git(&electron, &["status", "--porcelain"]), before);

    fs::write(migration.join("SYNC.md"), "no marker").unwrap();
    assert!(run(&options(false, false)).is_err());
    fs::write(migration.join("SYNC.md"), SYNC.replace("LAST", "deadbeef")).unwrap();
    assert!(
        run(&options(false, false))
            .unwrap_err()
            .contains("not in the Electron")
    );
    fs::remove_dir_all(&base).unwrap();
}
