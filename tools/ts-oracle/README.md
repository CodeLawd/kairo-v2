# ts-oracle

Runs the Electron app's TypeScript `src/lib` so Rust ports can be compared with it. Development and CI only; nothing here ships.

- `runner.mjs` reads JSON lines `{"fn":"module.export","args":[...]}` on stdin and writes `{"ok":true,"value":...}` or `{"ok":false,"error":{"kind":"throw"|"oracle",...}}`. It is started by `kairo-oracle` with Node and the Electron checkout's `tsx` loader.
- A non-function export is read by calling it with no arguments, so defaults compare like functions: `{"fn":"program.DEFAULT_SHOW_FILTER","args":[]}`.
- `oracle:<name>.<export>` loads `modules/<name>.ts` instead of `src/lib`. `modules/settings-defaults.ts` gives the settings defaults from `src/main/db/index.ts` and a fresh-install settings file.
- `oracle:settings-launch.launch(files, readOnly)` runs the Electron app's real `src/main/db/index.ts` in a child Node process against a temporary userData folder holding `files`, with `electron` and `electron-log` stubbed by `modules/electron-launch/` and `Date.now` and `crypto.randomUUID` pinned. It returns every file the folder then holds, or the error Electron threw. Used by `crates/kairo-store/tests/settings_launch.rs` (prompt 021).
- `sanitize-fixture.mjs <in> <out>` turns a real store file into a fixture: every string that is not a TypeScript literal or a colour becomes a placeholder. Used for `crates/kairo-domain/tests/fixtures/owner`.
- `crates/kairo-oracle` holds `assert_matches_oracle`, `assert_all_match`, the seeded `generate` helper and the comparison rules (see `native-migration/DECISIONS.md`, prompt 011).

## Modes

`KAIRO_ORACLE_MODE` selects how tests get TypeScript results:

| Mode | Node needed | Effect |
|---|---|---|
| `cache` (default) | no | Read `tests/oracle-cache/`. A missing result fails the test. |
| `live` | yes | Call the TypeScript; write nothing. Also runs the runner's own tests. |
| `refresh` | yes | Call the TypeScript and merge results into the cache. |

`KAIRO_ELECTRON_DIR` points at the Electron checkout, default `../kairo-presenter`. It must have `node_modules` installed.

## Refreshing the cache

```bash
cargo run -p ts-oracle -- --regenerate
```

This deletes the cached `.jsonl` files and reruns `cargo test --workspace` in `refresh` mode, so stale entries disappear. Two runs on the same Electron commit give byte-identical files. Commit the result. `tools/electron-sync --apply-generated` runs the same command.

## Throughput

```bash
KAIRO_ORACLE_MODE=live cargo test -p kairo-oracle --release --test throughput -- --ignored --nocapture
```

## Bundled Bible FTS parity

Run the live 1,000-query comparison and point-lookup timing against the bundled
Bible with this command.

```bash
cargo test -p kairo-store --test sqlite bundled_fts_oracle_and_point_lookup_latency -- --ignored --nocapture
```

If the Electron checkout's better-sqlite3 was built for Electron's Node ABI,
run the oracle child with Electron as Node. From the workspace root:

```bash
export KAIRO_ORACLE_ELECTRON_BIN="$(node -p "require(require('node:path').resolve(process.env.KAIRO_ELECTRON_DIR || '../kairo-presenter', 'node_modules/electron'))")"
oracle_bin="$(mktemp -d)"
printf '%s\n' '#!/bin/sh' 'ELECTRON_RUN_AS_NODE=1 exec "$KAIRO_ORACLE_ELECTRON_BIN" "$@"' > "$oracle_bin/node"
chmod +x "$oracle_bin/node"
PATH="$oracle_bin:$PATH" cargo test -p kairo-store --test sqlite bundled_fts_oracle_and_point_lookup_latency -- --ignored --nocapture
rm "$oracle_bin/node"
rmdir "$oracle_bin"
```

The comparison opens temporary copies of the bundled database. The following
two tests exercise sustained WAL concurrency and validation of the real AMPC
pack asset from the Electron checkout.

```bash
cargo test -p kairo-store --test sqlite sustained_concurrent_readers_and_writer -- --ignored --nocapture
cargo test -p kairo-store --test sqlite validates_real_electron_bible_pack -- --ignored --nocapture
```
