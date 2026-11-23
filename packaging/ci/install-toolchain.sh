#!/usr/bin/env bash
# Installs the Rust toolchain pinned in rust-toolchain.toml, so CI builds with
# the same compiler and clippy as everyone's machine. Extra arguments go to
# `rustup toolchain install` (for example --profile minimal --component clippy).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)"
test -n "$channel" || { echo "::error::no channel in rust-toolchain.toml"; exit 1; }
rustup toolchain install "$channel" "$@"
