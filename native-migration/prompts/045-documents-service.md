# Prompt 045 — Document import pipeline

**Phase:** 3. Services  
**Tracker ID:** `045-documents-service`

> **Agent start protocol.** This prompt assumes you have no earlier context.
> 1. Read `native-migration/START_HERE.md` (Agent protocol section) and `native-migration/PROGRESS.md`.
> 2. Read `native-migration/DECISIONS.md`. It overrides this prompt wherever they conflict.
> 3. Read `native-migration/03-native-architecture.md` for the sections this work touches.
> 4. If a prerequisite is not marked `done` in PROGRESS.md, **stop** and report it. Do not build on unfinished work.
> 5. Paths: run from this repository's root (`kairo-gpui-kit`). The Electron app is at `../kairo-presenter`. Unprefixed mentions of `src/`, `native/` (when describing existing code), `tests/`, `scripts/`, `server/`, `website/`, `docs/`, `resources/`, `build/`, `package.json`, `DESIGN.md` and `CLAUDE.md` mean that repo. New code goes here.
> 6. Check for drift: run `git -C ../kairo-presenter log --oneline <last-synced commit from native-migration/SYNC.md>..HEAD -- <the Electron files in section 3>`, and inspect uncommitted changes to them. The source as it is now is the specification.
> 7. Set this prompt to `in-progress` in PROGRESS.md before you change anything.

## 1. Objective and context

Port PDF and PowerPoint import, which turns documents into page images.

## 2. Prerequisites

- Prompt [025](./025-core-runtime.md) is marked `done`.
- Prompt 006 (CEF spike) is superseded by the `gpui-webview` decision in DECISIONS.md and is not a prerequisite.

## 3. Relevant existing source files

Read these before writing code. They are the specification.

- `../kairo-presenter/src/main/services/documents/index.ts` — service
- `../kairo-presenter/src/main/services/documents/converters.ts` — external apps
- `../kairo-presenter/src/main/services/documents/office-apps.ts` — discovery
- `../kairo-presenter/src/main/services/documents/slide-converter.ts` — LibreOffice install
- `../kairo-presenter/src/main/services/documents/pptx-builtin.ts` — HTML then PDF
- `../kairo-presenter/src/main/services/documents/pptx-slides.ts` — slide to HTML
- `../kairo-presenter/src/main/services/documents/pptx-videos.ts` — embedded video
- `../kairo-presenter/src/main/services/documents/__tests__/` — behaviours
- `../kairo-presenter/src/lib/documents.ts` — types
- `../kairo-presenter/docs/embedded-powerpoint-video.md` — design notes
- `../kairo-presenter/tests/document-converters.test.ts` — expectations
- `../kairo-presenter/tests/documents.integration.cjs` — expected flows
- `../kairo-presenter/tests/pptx-rendering.integration.cjs` — expected output
- `../kairo-presenter/tests/document-video-playback.integration.cjs` — expected output
- `../kairo-presenter/tests/document-page-swap.test.ts` — page swap; currently an untracked file in the owner's working tree, read but do not alter it

## 4. Implementation requirements

1. Replace `pdfjs-dist` rasterisation in `Documents.tsx` with `pdfium-render` at the same scale rule (fit in 3840×2160), the 500 page cap, cancellation and progress.
2. Port PPTX parsing with an XML reader replacing `cheerio`, emitting identical HTML. Convert HTML to PDF in a hidden Wry web view, using the platform's PDF call through Wry's raw handle (`WKWebView` `createPDF`/`printOperation`, WebView2 `PrintToPdf`, WebKitGTK `WebKitPrintOperation`).
3. The OS web view is always present on macOS. On Windows, check that the WebView2 runtime is installed and report clearly if it is missing.
4. Port detection and invocation of PowerPoint, WPS, Keynote and LibreOffice per platform, with AppleScript text unchanged.
5. Keep `prepare` options `resume` and `simplified`, the `preparing` push, atomic `document.json`, `KAIRO_PPT_SOURCE_B64` and `KAIRO_PPT_DEST_B64` for PowerPoint scripting, and page size lookup. Replace `jszip` with the `zip` crate and `nativeImage` with `image`.

Edge cases: list every error, empty and failure path you find in the source above, and handle each one. Add any that the source handles and this prompt does not name.

## 5. Technical constraints

- `../kairo-presenter` is a read-only reference unless this prompt explicitly permits an edit. The owner is actively working there: never commit, stash, reset, pull, switch branches or overwrite anything in it.
- `gpui-kit` stays pinned at the version in the root `Cargo.toml`. Do not bump it here.
- Do not invent APIs. If an API named here differs in the pinned version, read the crate source under `~/.cargo/registry/src`, use what exists, and record the difference in `native-migration/DECISIONS.md`.
- Do not leave `todo!()`, `unimplemented!()`, stubs or fake data in shipped paths. Test fakes belong under test configuration only.
- No secrets, personal data or proprietary SDK files in commits.

## 6. Expected deliverables

- `kairo-documents`, tests.
- Updated `native-migration/PROGRESS.md`, and `native-migration/api-inventory.md` for any API items this prompt finishes.

## 7. Testing and verification

Specific to this prompt:

- Compare page PNGs against pdf.js results using SSIM over a document corpus, including fonts, gradients and transparency. Compare generated PPTX HTML byte for byte through the oracle.

Required gates, all of which must pass before this prompt is finished:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- You made no edits in `../kairo-presenter`, unless this prompt permits them. Its status may still change, because the owner works there.

If a gate fails, debug and fix the cause. Do not disable a test, weaken an assertion or mark the work done. If you are stuck after a real attempt, set the status to `blocked` in PROGRESS.md with what you tried.

## 8. Performance considerations

- Pages rendered in parallel on a bounded pool. Compare with baseline.md.

Record measured numbers in PROGRESS.md notes. Do not state a number you did not measure.

## 9. Definition of done

- [ ] Inventory `documents` items done.
- [ ] Every requirement in section 4 is implemented and every deliverable exists.
- [ ] Every gate in section 7 passed, and the commands' results are summarised in PROGRESS.md.
- [ ] Anything not verified is written down as not verified.
- [ ] PROGRESS.md status is `done`, with date, branch and notes.

## 10. Dependencies for subsequent tasks

- 058, 085.

## Amendment from prompt 008

PPTX printing must wait for images before printing, as spike 006 did not. Output must be one 1440×810 pt page per slide (superseded CEF note; see DECISIONS.md, "Replace CEF with `gpui-webview`").
