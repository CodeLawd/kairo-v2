# ts-oracle

Runs the Electron app's TypeScript `src/lib` so Rust ports can be compared with it. Development and CI only; nothing here ships.

- `runner.mjs` reads JSON lines `{"fn":"module.export","args":[...]}` on stdin and writes `{"ok":true,"value":...}` or `{"ok":false,"error":{"kind":"throw"|"oracle",...}}`. It is started by `kairo-oracle` with Node and the Electron checkout's `tsx` loader.
- A non-function export is read by calling it with no arguments, so defaults compare like functions: `{"fn":"program.DEFAULT_SHOW_FILTER","args":[]}`.
- `oracle:<name>.<export>` loads `modules/<name>.ts` instead of `src/lib`. `modules/settings-defaults.ts` gives the settings defaults from `src/main/db/index.ts` and a fresh-install settings file.
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
