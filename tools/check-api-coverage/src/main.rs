//! Fails if any call in `api-inventory.md` has no valid owning prompt or status.

use std::{collections::BTreeSet, fs, path::Path, process::Command, process::ExitCode};

const COLUMNS: usize = 11;

/// Splits a table row, keeping the escaped pipes inside TypeScript unions.
fn cells(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut previous = ' ';
    for c in line.trim().trim_matches('|').chars() {
        if c == '|' && previous != '\\' {
            out.push(String::new());
        } else {
            out.last_mut().expect("never empty").push(c);
        }
        previous = c;
    }
    out.iter().map(|cell| cell.trim().to_owned()).collect()
}

fn check(inventory: &str, prompts: &BTreeSet<String>) -> Result<usize, Vec<String>> {
    let Some(start) = inventory.find("\n## Calls") else {
        return Err(vec!["no Calls section".into()]);
    };
    let mut errors = Vec::new();
    let mut count = 0;
    let rows = inventory[start..]
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .skip(2)
        .take_while(|line| line.starts_with('|'));
    for row in rows {
        count += 1;
        let cell = cells(row);
        if cell.len() != COLUMNS {
            errors.push(format!("{} columns: {row}", cell.len()));
            continue;
        }
        let name = format!("{}.{} {}", cell[0], cell[1], cell[2]);
        if !prompts.contains(&cell[9]) {
            errors.push(format!("{name}: owner '{}' is not a prompt", cell[9]));
        }
        if !matches!(cell[10].as_str(), "todo" | "done") {
            errors.push(format!("{name}: status '{}' is invalid", cell[10]));
        }
    }
    if count == 0 {
        errors.push("inventory has no calls".into());
    }
    if errors.is_empty() {
        Ok(count)
    } else {
        Err(errors)
    }
}

fn prompt_ids(root: &Path) -> Result<BTreeSet<String>, String> {
    let entries = fs::read_dir(root.join("native-migration/prompts")).map_err(|e| e.to_string())?;
    Ok(entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|n| n.get(..3))
                .map(str::to_owned)
        })
        .collect())
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if std::env::args().any(|arg| arg == "--regenerate") {
        let status = Command::new("node")
            .arg(root.join("tools/ts-oracle/api-inventory.cjs"))
            .status();
        if !status.is_ok_and(|status| status.success()) {
            eprintln!("could not regenerate the inventory");
            return ExitCode::FAILURE;
        }
    }
    let inventory = match fs::read_to_string(root.join("native-migration/api-inventory.md")) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read api-inventory.md: {error}");
            return ExitCode::FAILURE;
        }
    };
    let prompts = match prompt_ids(&root) {
        Ok(prompts) => prompts,
        Err(error) => {
            eprintln!("cannot read prompts: {error}");
            return ExitCode::FAILURE;
        }
    };
    match check(&inventory, &prompts) {
        Ok(count) => {
            println!("{count} calls, all assigned");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            errors.iter().for_each(|error| eprintln!("{error}"));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "x\n## Calls\n\n| a |\n|---|\n";

    fn row(owner: &str, status: &str) -> String {
        format!("| n | m | `c` | invoke | a: 'x' \\| 'y' | r | h | p | c | {owner} | {status} |\n")
    }

    fn prompts() -> BTreeSet<String> {
        ["038".to_string()].into()
    }

    #[test]
    fn accepts_assigned_rows_with_escaped_pipes() {
        assert_eq!(
            check(&format!("{HEAD}{}", row("038", "todo")), &prompts()),
            Ok(1)
        );
    }

    #[test]
    fn rejects_unassigned_unknown_and_bad_status() {
        assert!(check(&format!("{HEAD}{}", row("unassigned", "todo")), &prompts()).is_err());
        assert!(check(&format!("{HEAD}{}", row("999", "todo")), &prompts()).is_err());
        assert!(check(&format!("{HEAD}{}", row("038", "maybe")), &prompts()).is_err());
    }

    #[test]
    fn rejects_empty_inventory() {
        assert!(check(HEAD, &prompts()).is_err());
        assert!(check("nothing", &prompts()).is_err());
    }
}
