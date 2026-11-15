# Native release

`.github/workflows/release-native.yml` builds macOS (arm64, x86_64), Windows x64
and Linux x64 on their own runners, runs the workspace checks, and publishes
only when every job succeeds. Until prompt 112 it makes a **draft
pre-release** on `CodeLawd/kairo-presenter`, never the latest release, so the
website and every installed app keep seeing Electron's releases.

## Release

```bash
cargo run -p kairo-release -- prepare patch
```

`prepare` takes `patch`, `minor`, `major` or an exact version (the first native
release is `2.0.0`), and `--skip-changelog` for an internal-only build (every
draft before 112). It checks the tree, the version order against the latest
Electron release, the What's New entry and the DeckLink helper, asks to
confirm, runs fmt, clippy and the tests, bumps the workspace version, commits,
tags `v<version>`, pushes, and starts the workflow on the tag.

By hand, on an existing tag:

```bash
gh workflow run release-native.yml --ref v2.0.0 -f release_tag=v2.0.0 -f skip_changelog=true
```

## Dry run

A test tag publishes a draft to this repository only:

```bash
gh workflow run release-native.yml --ref v2.0.0-test.1 -f release_tag=v2.0.0-test.1 -f target_repo=CodeLawd/kairo-v2 -f skip_changelog=true
```

Add `-f fail_job=linux` (or `macos-arm64`, `macos-x86_64`, `windows`) to fail
one platform on purpose; the publish job must then be skipped and no release
created.

## Secrets and variables

In the `release` environment:

- `KAIRO_PRESENTER_RELEASE_TOKEN`: a fine-grained token for
  `CodeLawd/kairo-presenter` only, with Contents read and write and nothing else.
- The platform secrets (`CSC_LINK`, `CSC_KEY_PASSWORD`, `APPLE_ID`,
  `APPLE_APP_SPECIFIC_PASSWORD`, `APPLE_TEAM_ID`, `WIN_CSC_LINK`,
  `WIN_CSC_KEY_PASSWORD`) and variables (`PROD_API_URL`, `PROD_WEB_URL`,
  optional `PROD_AUDIENCE_API_URL`, `LICENSE_PUBLIC_KEY`), unchanged from the
  Electron release.

Rules and reasons: `native-migration/DECISIONS.md`, 104.
