#!/usr/bin/env bash
# Cargo runner for macOS (see .cargo/config.toml): signs a debug `kairo`
# binary with a real identity before running it, then runs whatever cargo
# asked for.
#
# Why: the keychain remembers "Always Allow" per code signature. A plain
# `cargo build` gives every rebuild a new ad-hoc signature, so each `cargo run`
# asks for the "Kairo Native Storage" keychain item again. A certificate-based
# signature with a fixed identifier keeps the same designated requirement
# across rebuilds, so one "Always Allow" sticks.
#
# Identity: $KAIRO_SIGN_IDENTITY (the variable packaging uses), else the first
# valid "Developer ID Application" or "Apple Development" identity. With none,
# the binary runs unsigned as before. Test binaries are never signed.
set -euo pipefail

binary="$1"
if [[ "$(basename "$binary")" == kairo ]]; then
  identity="${KAIRO_SIGN_IDENTITY:-}"
  if [[ -z "$identity" ]]; then
    identity="$(security find-identity -v -p codesigning 2>/dev/null |
      sed -n -E 's/^ *[0-9]+\) ([0-9A-F]{40}) "(Developer ID Application|Apple Development):.*/\1/p' |
      head -n 1)"
  fi
  if [[ -n "$identity" ]]; then
    codesign --force --sign "$identity" --identifier com.kairo.app.dev \
      --timestamp=none "$binary" 2>/dev/null ||
      echo "dev-sign-run: codesign failed; running unsigned" >&2
  fi
fi
exec "$@"
