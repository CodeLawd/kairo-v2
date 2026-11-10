#!/bin/sh
# Regenerates crates/kairo-audience/src/slido/selectors.js from Electron's
# slido-selectors.ts, unchanged. Needs ../kairo-presenter/node_modules.
set -eu
root="$(cd "$(dirname "$0")/../.." && pwd)"
src="$root/../kairo-presenter/src/main/services/audience/sources/slido-selectors.ts"
"$root/../kairo-presenter/node_modules/.bin/esbuild" "$src" --bundle --format=iife --global-name=qaSelectors --platform=browser \
  --outfile="$root/crates/kairo-audience/src/slido/selectors.js"
