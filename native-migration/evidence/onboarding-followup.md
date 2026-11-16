# Onboarding follow-up verification — 2026-11-16

## Measurement method

`cargo test -p ui-account --release --test flow onboarding_render_baseline -- --ignored --nocapture`

100 forced refresh/render traversals per case in 1280 × 800 headless windows.
Output has default settings and an activated session; gate is signed out.
Allocation counts are alloc/realloc calls on the test's thread, not bytes or
retained/peak memory. Timing includes CPU traversal and headless paint preparation;
it does not measure GPU submission, display latency or native compositor cost.
Do not compare runs taken during concurrent builds/render tests.

Before refactor:

| Case | Mean CPU ms/frame | Allocations/frame |
| --- | ---: | ---: |
| Tour | 0.525 | 1901 |
| Output | 0.408 | 1624 |
| Gate | 0.431 | 1793 |

No glow bitmap cache was added. GPU evidence is still required before trading
resolution/scale/theme invalidation and image memory for fewer painted paths.

## Behavioral checks

The account integration suite now delivers fake frames rather than sleeping.
Coverage exercises real button/input/drag/select handlers for reset duplication,
account readiness during a pending tour write, reset during finish, obsolete
errors, sign-out replacement, delayed display saves, account draft retention,
late error frames and retry, idle tour scheduling, keyboard navigation, minimum
window size and 125% interface sizing. Every normal test frame runs the existing
accessibility audit.

Independent review found lost signup drafts and stale display-save results;
both are addressed and have regression coverage. Minimum-size scrolling was
also corrected while converting commands and interface dimensions.

Native GPU profiling, real trackpad phases and VoiceOver remain unverified.

## Final release measurements

Median of three post-change runs after concurrent builds/tests completed:

| Case | Median CPU ms/frame | Allocations/frame |
| --- | ---: | ---: |
| Tour | 0.357 | 1858 |
| Output | 0.245 | 1607 |
| Gate | 0.273 | 1854 |

Forced-refresh allocation counts improved slightly for tour/output; gate gained
one retained view layer and its count increased. These are small samples and are
not GPU frame-time or end-to-end latency claims. The more substantial scheduling
change is verified behaviorally: settled stories no longer request continuous
frames, and hidden stories own no running timer.

## Final validation

- Full workspace: `cargo test --workspace --no-fail-fast` — 2,142 passed,
  0 failed, 46 ignored across 235 suites. This run began before the last
  account regression additions; those were validated in the targeted run below.
- Current account suite: 25 unit tests and 23 UI flow tests pass; explicit release
  benchmark is ignored in the normal suite. The stricter theme-driven zoom
  assertion also passes in a separate targeted run.
- Domain setup transformation tests pass (binding preservation, deterministic ID,
  empty stage disabling and idempotent church seeding).
- Account/domain Clippy with `--all-targets -- -D warnings` passes.
- `cargo fmt --all -- --check` and `git diff --check` pass.
- `cargo build -p kairo` passes. Linker reports the existing large `__eh_frame`
  compact-unwind warning; it is not a compile failure.
- No production credentials were submitted and no app was published.
