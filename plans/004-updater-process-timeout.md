# Plan 004: Give the updater's child processes a timeout

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat a2628bd..HEAD -- crates/kairo-updater/src/install.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: bug
- **Planned at**: commit `a2628bd`, 2026-11-10

## Why this matters

The updater shells out to `codesign`, `hdiutil`, `ditto` and similar during verify and
install. Its `run` helper awaits `Command::output()` with no timeout, so a hung `hdiutil
attach` (a known macOS failure mode with a stuck disk image) leaves the update stuck in
"installing" forever with no error for the operator. The documents crate already runs
children with a timeout (`crates/kairo-documents/src/process.rs::run`), so this is also
an inconsistency.

## Current state

`crates/kairo-updater/src/install.rs` (~lines 48–65):

```rust
pub(crate) async fn run(
    program: &str,
    args: &[&std::ffi::OsStr],
) -> Result<(String, String), String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| format!("{program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.success() {
        Ok((stdout, stderr))
    } else {
        Err(format!("{program} failed: {}", stderr.trim()))
    }
}
```

Callers in the same file at roughly lines 70, 77, 95, 115, 143, 155, 168. tokio features
`process` and `time` are already enabled in `crates/kairo-updater/Cargo.toml`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Format check | `cargo fmt --all -- --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Tests | `cargo test -p kairo-updater` | all pass |

## Scope

**In scope**: `crates/kairo-updater/src/install.rs`.
**Out of scope**: `kairo-documents/src/process.rs` (a shared runner is a separate, broader refactor); error strings other than the new timeout message.

## Git workflow

- Branch: `advisor/004-updater-timeout`; commit e.g. `Time out updater child processes`.

## Steps

### Step 1: Add a timeout and kill-on-drop

```rust
const PROCESS_TIMEOUT: Duration = Duration::from_secs(120);

let child = tokio::process::Command::new(program)
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .map_err(|e| format!("{program}: {e}"))?;
let output = tokio::time::timeout(PROCESS_TIMEOUT, child.wait_with_output())
    .await
    .map_err(|_| format!("{program} timed out"))?
    .map_err(|e| format!("{program}: {e}"))?;
```

Dropping the timed-out future drops the child, which kills it (`kill_on_drop`).

**Verify**: `cargo test -p kairo-updater` → all pass.

## Test plan

Add a `#[cfg(test)]` unit test in `install.rs` (unix only) that calls a variant of `run`
with a 100 ms timeout on `sleep 5` and asserts `Err` containing `timed out`, completing
in under 2 s. To make the timeout injectable, have `run` call `run_with_timeout(program, args, PROCESS_TIMEOUT)`.

## Done criteria

- [ ] fmt, clippy exit 0; `cargo test --workspace` 0 failed incl. the new test
- [ ] `grep -n "kill_on_drop(true)" crates/kairo-updater/src/install.rs` → 1 match
- [ ] Only `install.rs` modified; `plans/README.md` row updated

## STOP conditions

- Any install step legitimately takes longer than 120 s in the existing mac bundle test (`crates/kairo-updater/tests/mac_bundle.rs`) — report the measured time.

## Maintenance notes

- If a shared `kairo_core::process::run(cmd, timeout)` is introduced later, migrate this and `kairo-documents/src/process.rs` onto it.
