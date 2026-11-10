# Plan 003: Write and delete song files without parsing the whole folder

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat a2628bd..HEAD -- crates/kairo-store/src/workspace.rs crates/kairo-lyrics/src/library.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P2
- **Effort**: S
- **Risk**: LOW
- **Depends on**: none
- **Category**: perf
- **Planned at**: commit `a2628bd`, 2026-11-10

## Why this matters

Every song save, favourite toggle or delete reads and JSON-parses **every** `.song.json`
in the songs folder to find files holding that id (externally renamed files carry the id
inside, not in the name). Single edits are O(library); a `.kairo` "All songs" import of
N songs into an M-song library is O(N×M) file reads on the lyrics writer — about four
million parses for 2,000 × 2,000, blocking lyrics writes for minutes. A bypass,
`export_song_file`, already exists for one caller to dodge this cost.

## Current state

- `crates/kairo-store/src/workspace.rs`
  - `matching_song_files` (~line 441): `read_dir`, then for each song file
    `read_to_string` + `parse_song_file` to get the embedded id; matches when
    `embedded_id.unwrap_or(filename_id) == song_id`.
  - `write_song_file(songs_dir, song)` → `matching_song_files` then `write_song_file_replacing`.
  - `export_song_file(songs_dir, song)` → `write_song_file_replacing(.., Vec::new())`
    ("absence already checked by a scan").
  - `delete_song_file(songs_dir, id)` → removes every `matching_song_files` hit.
  - `read_song_files(songs_dir, now_ms) -> Vec<SongFileEntry { song, file }>` — the full scan.
  - `song_id_from_file_name(name)` (in `kairo-domain/src/workspace.rs`) gives the id from
    `<slug>__<id>.song.json`.
- `crates/kairo-lyrics/src/library.rs` callers: `read_song_files` at ~249 (startup/sync),
  `export_song_file` ~291, `write_song_file` ~348, ~386, ~417, ~464, `delete_song_file` ~399.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Format check | `cargo fmt --all -- --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Tests | `cargo test -p kairo-store -p kairo-lyrics -p kairo-transfer` | all pass |

## Scope

**In scope**: `crates/kairo-store/src/workspace.rs`, `crates/kairo-lyrics/src/library.rs`,
and their tests (`crates/kairo-lyrics/tests/library.rs`).

**Out of scope**: the song file format, `song_file_name`, `read_song_files` semantics,
`kairo-transfer` (it calls into the library).

## Git workflow

- Branch: `advisor/003-song-file-index`; commit e.g. `Look up song files by name before parsing`.

## Steps

### Step 1: Cheap match first in `matching_song_files`

Two-pass match, same results:
1. A file whose **file-name id** equals `song_id` matches only if its embedded id is
   the same or absent — but it must still be parsed to confirm, so instead:
2. Keep a per-folder cache: `static INDEX: Mutex<HashMap<PathBuf, (SystemTime /*dir mtime*/, HashMap<String, Vec<PathBuf>>)>>`
   mapping embedded-or-filename id → files, rebuilt only when the folder's mtime
   changed since the last build. Writes made through `write_song_file_replacing` /
   deletes update the cached map in place (and refresh the stored mtime after the write).

If a mtime cache feels too clever, the simpler acceptable alternative: let callers pass
known paths. Add `write_song_file_at(songs_dir, song, existing: &[PathBuf])` and have
`kairo-lyrics` keep an `id -> Vec<PathBuf>` map built from `read_song_files` and updated
on each write/delete. Pick ONE approach and note it in the commit message.

**Verify**: `cargo test -p kairo-store` → all pass.

### Step 2: Remove the bypass

Once lookups are cheap, delete `export_song_file` and call `write_song_file` at the
lyrics call site (~291).

**Verify**: `grep -rn "export_song_file" crates/` → no matches; `cargo test -p kairo-lyrics` passes.

## Test plan

In `crates/kairo-store/src/workspace.rs` tests (or `crates/kairo-lyrics/tests/library.rs`):
1. Song renamed externally (file name id ≠ embedded id): `write_song_file` replaces it, leaves one file.
2. Title change: old file removed, new file written.
3. File added by another process after the cache was built: still found (mtime invalidation).
4. Counting test: write 200 songs then update one; assert (via a counter in a test-only
   hook or timing-free logic) that the update does not parse all 200 files.

## Done criteria

- [ ] fmt, clippy exit 0; `cargo test --workspace` 0 failed with new tests passing
- [ ] `grep -rn "export_song_file" crates/` → no matches
- [ ] Only in-scope files modified; `plans/README.md` row updated

## STOP conditions

- Directory mtime does not change on file add/remove on the target filesystem in tests (then use the caller-index approach).
- Any existing lyrics/transfer test changes output.

## Maintenance notes

- Anything else writing into the songs folder must go through these functions or invalidate the index.
