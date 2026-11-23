# Kairo native workspace

This repository contains the Rust and GPUI Kit bootstrap for the native Kairo app.
The shipping Electron app lives next door in `../kairo-presenter` and serves as the
migration reference. Building, testing, and running CI for this workspace requires
only the native checkout.

## Build and run

Install Rust through [rustup](https://rustup.rs/) and follow the
[GPUI Kit platform requirements](https://gpui-kit.com/docs/installation/).
The Rust version is pinned in `rust-toolchain.toml` (rustup installs it on first
build), along with rustfmt and Clippy. CI builds with the same version, so a new
Rust release can't change what passes. Upgrade it on purpose: edit the file, run
the quality gates, commit.

```sh
cargo build --workspace
cargo run -p kairo
```

The app uses the hosted Kairo API and web app by default, configured in
[.cargo/config.toml](.cargo/config.toml). These public URLs are compiled into the
binary; changing them requires rebuilding. To use local services explicitly:

```sh
KAIRO_API_URL=http://localhost:3000 KAIRO_WEB_URL=http://localhost:3001 cargo run -p kairo
```

## Quality gates

CI runs these on macOS, Linux and Windows for every push:

```sh
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Install the repository's Git hooks once per clone, so the quick ones run before
anything reaches GitHub:

```sh
tools/install-hooks.sh
```

The pre-push hook runs the formatting check, workspace Clippy, and the tests of
every crate the pushed commits change. Skip it once with `git push --no-verify`,
or skip only its tests with `KAIRO_PUSH_TESTS=0 git push`.

See [the migration playbook](native-migration/START_HERE.md) for the prompt
sequence and [the tracker](native-migration/PROGRESS.md) for verification status.
