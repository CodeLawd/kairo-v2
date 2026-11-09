# Kairo native migration — start here

This directory is a playbook for rebuilding Kairo from Electron into a native Rust
application using GPUI Kit. The new app lives in this repository (`kairo-gpui-kit`). The
Electron app stays in `../kairo-presenter`, as both the reference and the shipping product.
Keep both folders side by side.

**Status at creation:** planning only. No migration code exists yet, no spike has run, and
the performance figures are targets, not measurements.

## What is here

| File | Purpose |
|---|---|
| `01-codebase-audit.md` | How Kairo works today, including what it silently relies on Chromium for |
| `02-gpui-kit-feasibility.md` | What GPUI Kit can and cannot do, checked against its source |
| `03-native-architecture.md` | The target design |
| `04-feature-parity-matrix.md` | Every feature, where it goes and how it is proven |
| `05-roadmap.md` | Phases and ordering rationale |
| `06-testing-performance-strategy.md` | Test layers and budgets |
| `07-risks.md` | Risks, fallbacks and stop conditions |
| `08-production-readiness-checklist.md` | Final release gate |
| `prompts/` | 95 numbered prompts, to run in order |
| `PROGRESS.md` | Status of each prompt, and the session log |
| `DECISIONS.md` | Choices made along the way. Overrides everything else |
| `SYNC.md` | The last Electron commit carried over, and the queue of changes still to port |

Files created as the work proceeds: `baseline.md`, `api-inventory.md`, `qualification.md`,
`spikes/`, `notes/`, `evidence/`.

## The key idea

GPUI Kit is excellent for the operator's controls, but production GPUI cannot render
offscreen, so it cannot feed NDI, DeckLink, Syphon or recording. It also has no video
element and cannot draw Kairo's slide effects. The plan therefore uses **GPUI Kit for the
booth, and a separate `wgpu` output engine for everything the congregation sees**. The
engine runs in its own process, so a UI problem cannot blank the wall.

## Prerequisites

**Tools:** Rust at the version GPUI Kit requires (1.92 or later at time of writing), Bun,
Node, Xcode Command Line Tools on macOS, and Visual Studio C++ tools on Windows. For Linux,
see GPUI Kit's installation guide.

**Required for some prompts:** the NDI SDK, DeckLink SDK (`DECKLINK_SDK_PATH`), an LGPL
FFmpeg build, and PDFium. Never commit proprietary SDK files.

**For real qualification:** two or more displays, an NDI receiver, and DeckLink hardware
if you support it. Prompts say plainly when hardware was not available.

## How to run the prompts

1. Start a fresh agent session in `kairo-gpui-kit`, with read access to `../kairo-presenter`.
2. Give it one prompt:

   ```text
   Read native-migration/prompts/000-workspace-bootstrap.md and carry it out exactly.
   ```

3. Review its results and PROGRESS.md.
4. Commit on a branch of your choosing, then move to the next prompt.

Use one session per prompt. Each prompt is written to work with no earlier context. If you
want to know the next prompt, ask the agent to read PROGRESS.md and take the first `todo`
row whose prerequisites are all `done`.

## Working on Electron during the migration

Carry on as normal in `../kairo-presenter`. Then:

- Run `tools/electron-sync` (built by prompt 009) before each phase, and before 100, 109 and 112.
- Bug fixes and new features are queued for porting.
- Changes to stored data, `.kairo` files or the slide look carry the most risk, and are flagged.
- Agree a short feature freeze before cutover.

## Human checkpoints

An agent must stop for you at these prompts:

- **008** Go or no-go, plus the decisions that shape everything after it.
- **105** Moving existing users from Electron to native.
- **110** Physical hardware tests.
- **111** Pilot services.
- **112** General release and removing Electron.

Other prompts can also stop as `blocked`.

## Decisions you will be asked for at prompt 008

Thinking about these early will save time:

- Lowest macOS version to support. GPUI's real floor is unknown until spike 007.
- Whether Intel Macs and Linux stay supported, and at what level.
- FFmpeg and H.264/HEVC licensing. This may deserve legal advice.
- Settled: web content uses the OS web views through `gpui-webview`, so nothing is shipped
  or downloaded for it. Open: whether web on NDI and capture outputs is required (prompt 059).

## Agent protocol

Each session must follow this exactly.

**Starting**

1. Read `PROGRESS.md`, then `DECISIONS.md`.
2. Confirm every prerequisite is `done`. If not, stop and report it.
3. Check drift: `git -C ../kairo-presenter log --oneline <last-synced commit in SYNC.md>..HEAD --
   <the prompt's source files>`, and look at uncommitted changes to them.
4. Treat `../kairo-presenter` as read-only unless the prompt says otherwise. The owner works
   there: never commit, stash, reset, pull or switch branches in it.
5. Mark the prompt `in-progress`, and add a line to the session log.

**Working**

- Read the listed source before writing code. The code is the specification.
- For GPUI, the pinned source in `~/.cargo/registry/src` is the authority, followed by the
  installed GPUI Kit skills. Do not rely on memory.
- Record anything that deviates from the prompt in `DECISIONS.md` before doing it.
- Do not widen scope. If you find a separate problem, record it in PROGRESS.md notes.

**Finishing**

1. Run every gate in section 7 of the prompt, and fix what fails.
2. Update PROGRESS.md (status, date, branch, measured numbers, anything unverified) and
   `api-inventory.md`.
3. End your report with what was verified, what was not, and the next prompt.

**Resuming after an interruption**

Read the session log. If a row is `in-progress` with no owner, inspect its branch and the
partial work, compare against the prompt's section 4, and continue. Do not restart from
scratch or assume earlier work is complete. Re-run the gates before marking it done.

**If something fails**

Set `blocked`, record what you tried and what would unblock it, and stop. Do not disable
tests, lower golden thresholds or fake results. Stop conditions for the whole migration
are listed in `07-risks.md`.

## Execution sequence

### 0. Foundations and gating spikes

- **000** Create the native workspace and agent tooling *(needs: —)*
- **001** Measure the Electron baseline *(needs: 000)*
- **002** Spike: separate engine process with output windows and preview *(needs: 000)*
- **003** Spike: match Chromium slide text *(needs: 000)*
- **004** Spike: video decode into wgpu *(needs: 000)*
- **005** Spike: NDI, DeckLink ring and macOS native code from Rust *(needs: 000)*
- **006** Spike: CEF web host for all browser needs *(needs: 000)*. Superseded by `gpui-webview`; see DECISIONS.md
- **007** Spike: operator UI on GPUI Kit *(needs: 000)*
- **008** Human checkpoint: go or no-go *(needs: 001, 002, 003, 004, 005, 006, 007)*
- **009** Electron-to-native sync process *(needs: 000)*

### 1. Domain model

- **010** Freeze the application API inventory *(needs: 008)*
- **011** Differential test harness against the TypeScript originals *(needs: 010)*
- **012** Port the data types with on-disk compatibility *(needs: 011)*
- **013** Port settings and theme normalizers *(needs: 012)*
- **014** Port lyrics logic *(needs: 012)*
- **015** Port scripture logic *(needs: 012)*
- **016** Port program, media, transfer and remaining shared logic *(needs: 012, 013)*

### 2. Persistence and runtime

- **020** Paths, logging, crash handling and single instance *(needs: 016)*
- **021** Settings store and migrations *(needs: 013, 020)*
- **022** Secure storage and Electron secret migration *(needs: 020)*
- **023** SQLite access layer *(needs: 020)*
- **024** Workspace folders and song files *(needs: 021, 023)*
- **025** AppCore, actors and event topics *(needs: 010, 021)*

### 3. Services

- **030** Cloud API client and session *(needs: 022, 025)*
- **031** Licence and onboarding *(needs: 030)*
- **032** Usage, crash reports, feedback and sermon records *(needs: 030)*
- **033** Bible lookup and search *(needs: 015, 023)*
- **034** API.Bible, offline policy and download manager *(needs: 022, 033)*
- **035** Scripture detector, deterministic path *(needs: 011, 015, 033)*
- **036** Scripture detector, LLM path and fallback *(needs: 035)*
- **037** Global quote search worker *(needs: 034, 035)*
- **038** Audio capture and Deepgram transcription *(needs: 025)*
- **039** Orchestrator, voice navigation and recovery *(needs: 036, 037, 038)*
- **040** Lyrics library and search *(needs: 014, 023, 024)*
- **041** Online lyric search and translation *(needs: 040)*
- **042** Sermon notes import *(needs: 033, 039)*
- **043** Media and tracks libraries *(needs: 024, 016)*
- **044** Setlist, libraries, passages, usage and transfer *(needs: 040, 033)*
- **045** Document import pipeline *(needs: 025, 006)*
- **046** Audience rooms and Engage *(needs: 030)*
- **047** Slido source *(needs: 006, 046)*
- **048** Auto-update *(needs: 025, 020)*

### 4. Output engine

- **050** Engine process, protocol and supervision *(needs: 008, 025)*
- **051** Compositor core and layer stack *(needs: 050)*
- **052** Scene model and slide text *(needs: 013, 051)*
- **053** Backgrounds, elements, shapes and filters *(needs: 052)*
- **054** Messages, props, timers, polls, questions, info, logo *(needs: 052, 053)*
- **055** Video playback layer *(needs: 051, 004)*
- **056** Program audio, routing and tracks *(needs: 055)*
- **057** Live camera layer *(needs: 051, 056)*
- **058** Document pages and embedded video *(needs: 055, 045)*
- **059** Live web layer *(needs: 006, 051, 056)*
- **060** Display outputs, stage windows and test patterns *(needs: 051, 054)*
- **061** NDI output *(needs: 005, 051, 056)*
- **062** DeckLink output *(needs: 005, 051)*
- **063** Syphon output *(needs: 005, 051)*
- **064** Capture to file *(needs: 005, 051, 056)*
- **065** Reconcile, routing, lobby playlists and program service *(needs: 039, 058, 060, 061, 062)*
- **066** Operator preview transport *(needs: 050, 065)*
- **067** Application wiring formerly in the IPC layer and main entry *(needs: 032, 039, 043, 044, 045, 046, 048, 065)*

### 5. Operator interface

- **070** Design system *(needs: 007, 025)*
- **071** Application shell, menus and quit flow *(needs: 070, 065, 067)*
- **072** Launch screen *(needs: 071)*
- **073** Account, onboarding, coach tour *(needs: 072, 031)*
- **074** Settings *(needs: 073)*
- **075** Screens configuration *(needs: 074, 065)*
- **076** Operator workspace, live rail and monitor *(needs: 066, 071)*
- **077** Operator queue, suggestions and program panels *(needs: 076, 054, 057)*
- **078** Scripture workspace *(needs: 077, 033)*
- **079** Sermon notes review *(needs: 078, 042)*
- **080** Lyrics library and quick open *(needs: 078, 040)*
- **081** Lyrics slide grid *(needs: 080, 066)*
- **082** Lyrics editing and import *(needs: 081, 041)*
- **083** Media dock and inspector *(needs: 082, 043, 055)*
- **084** Tracks *(needs: 083, 056)*
- **085** Documents *(needs: 084, 058)*
- **086** Theme editor canvas *(needs: 085, 053)*
- **087** Theme editor panels *(needs: 086)*
- **088** Props editor *(needs: 087, 054)*
- **089** Web workspace *(needs: 088, 059)*
- **090** Audience workspace *(needs: 089, 046, 047)*
- **091** Transcription, transfer, capture popover and leftovers *(needs: 090, 044, 063, 064)*

### 6. Hardening and cutover

- **100** First-run migration and rollback *(needs: 091)*
- **101** macOS packaging *(needs: 100)*
- **102** Windows packaging *(needs: 100)*
- **103** Linux packaging *(needs: 100)*
- **104** Release pipeline *(needs: 101, 102, 103)*
- **105** Bridge from Electron to native *(needs: 048, 104)*
- **106** Accessibility and keyboard pass *(needs: 091)*
- **107** Performance validation and tuning *(needs: 091)*
- **108** Soak and failure testing *(needs: 107)*
- **109** Full parity audit *(needs: 106, 108)*
- **110** Hardware qualification *(needs: 109)*
- **111** Pilot with real services *(needs: 110)*
- **112** General release and Electron retirement *(needs: 105, 111)*
