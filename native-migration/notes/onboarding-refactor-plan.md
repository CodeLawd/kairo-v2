# Onboarding follow-up plan — 2026-11-16

## Contract

Preserve the implemented full-window stage, six-slide tour before sign-up,
Welcome → Account → Output domain step order, persisted writes, pointer cursors,
public test IDs, and existing form values. Keep the caller-owned velocity-aware
spring. No glow bitmap cache without GPU/paint evidence. This supersedes the
pasted review's stale phase 4 and its suggestion to delete fluid.rs.

## Work and acceptance

1. Establish a reproducible release CPU render/allocation baseline for tour,
   output and gate. Headless results are not GPU frame times. Record limitations;
   defer glow baking until native GPU evidence justifies it.
2. Reproduce password-reset duplication and pending-write/account-readiness races.
   Serialize explicit user commands by rejecting duplicates while busy. Coalesce
   automatic account reconciliation into one retry after the active write, rather
   than enqueue stale Back/Continue commands. Cancel stale results after session
   or onboarding generation changes; leave errors retryable.
3. Inject the executor timestamp into springs/pager constructors and every motion
   timestamp. Tests advance fake time and deliver frames; no real sleeps. Error
   shake schedules its own short frames and snaps under reduced motion. Story
   entry uses frames only while active; thereafter caret/countdown use cancellable
   deadline timers (500 ms / 1 s). Offscreen stories schedule nothing.
4. Extract pure Page/action derivation and display/church settings transformations.
   Domain functions receive time/IDs as input; no windows or system clock.
5. Share AccountPage between gate and setup. It owns auth/verify/activate and one
   sign-out operation, creates children on transitions, and rejects stale results.
   Setup owns progression and writes; gate owns its blocking/focus policy.
6. Extract TourPage with its own pager, story scheduling, drag/scroll listeners,
   and dots. Parent observes only semantic target changes; spring/story frames
   must not rebuild the setup top bar, account form or footer. Derive page identity
   once; only identity changes reset entry and welcome-letter clocks.
7. Create display pickers on account/settings transitions and retain output targets.
   Convert remaining command divs to Kit Buttons, including shared text_button
   callers; retain pointer preference. Use rem units for interface layout while
   keeping pointer geometry and illustrative artwork in logical pixels.
8. Verify unit and UI tests for delayed/failed writes, duplicate commands, readiness
   during writes, session replacement during sign-out, keyboard focus, disabled
   reset, pager interruption, reduced motion, timer cancellation, minimum window,
   and zoom. Re-run release benchmark and document decisions and limitations.

## Execution record

- Initial tree includes the prior onboarding implementation and hosted build URL
  fix, all uncommitted. Preserve that work; no reset or checkout elsewhere.
- Profiling numbers must be labelled CPU/headless; no claim of native GPU timing.

### Implemented

- Release headless CPU/allocation harness added. GPU costs are not measured, so
  glow caching remains conditional and is not implemented.
- Password reset is single flight. Sequential write runner checks identity/reset
  generations before and after effects; ordinary sign-in can complete a pending
  tour transition. User commands are rejected while busy; automatic account
  reconciliation is coalesced. Failed automatic progression has an explicit retry.
- Display saves also reject stale session/reset/settings snapshots. Obsolete
  failures do not show errors on a new setup journey.
- All production motion callers use the executor clock and timestamp-aware spring
  constructors. Compatibility constructors remain available for callers outside
  this path. Tests deliver fake frames and use no sleeps.
- Pure SetupState/Page/actions and domain settings transformations extracted.
- AccountPage shared by gate/setup; hidden signup drafts are retained without
  creating/focusing children. TourPage owns pager/stories and cancellable timers.
- Pickers are created from state events; targets are cached and invalidated by
  settings events. No entities/subscriptions are created by these render methods.
- Remaining account command helpers and welcome quick starts use Kit Buttons.
  Pointer cursors remain. Interface dimensions use rems; physical drag coordinates
  and illustrative mock-ups retain logical pixel geometry.
- Regression coverage includes delayed success/failure, duplicate activation,
  readiness during writes, progress reset, stale sign-out/display save, draft
  retention, late error frames/retry, idle tour scheduling, minimum size and zoom.

### Decisions and limits

- Keep the six-slide tour before sign-up; no persisted step semantic changes.
- Do not queue Back/Continue commands captured against an old page. Coalesce only
  automatic reconciliation against current state after the running effect.
- Keep the prior hosted API configuration and unrelated concurrent brand edits.
- Headless tests do not establish native GPU costs, VoiceOver behavior or real
  trackpad phase correctness. Those require a native hands-on pass.

### Final checks

Implemented and verified. Workspace run: 2,142 passed / 0 failed / 46 ignored;
current targeted account suite: 25 unit + 23 UI flow tests pass. Clippy, formatting,
native build and three explicit release benchmark runs pass. See
[verification evidence](../evidence/onboarding-followup.md) for methodology,
measurements, review findings and remaining native verification limits.
