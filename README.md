# Kairo native workspace

This repository contains the Rust and GPUI Kit bootstrap for the native Kairo app.
The shipping Electron app lives next door in `../kairo-presenter` and serves as the
migration reference. Building, testing, and running CI for this workspace requires
only the native checkout.

## Build and run

Install Rust through [rustup](https://rustup.rs/) and follow the
[GPUI Kit platform requirements](https://gpui-kit.com/docs/installation/).
Use stable Rust 1.92 or newer. The workspace toolchain file requests rustfmt and
Clippy.

```sh
cargo build --workspace
cargo run -p kairo
```

The bootstrap window displays **Hello, world!**.

## Quality gates

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [the migration playbook](native-migration/START_HERE.md) for the prompt
sequence and [the tracker](native-migration/PROGRESS.md) for verification status.
