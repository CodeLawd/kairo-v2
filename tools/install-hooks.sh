#!/usr/bin/env bash
# Uses the repository's hooks in tools/hooks (pre-push runs CI's quick checks).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
chmod +x tools/hooks/*
git config core.hooksPath tools/hooks
echo "Git hooks installed from tools/hooks."
