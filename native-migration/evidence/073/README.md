# 073 evidence

- `wizard-welcome.png`: `cargo run -p kairo` (debug) in a fresh profile. First-run setup opens over the booth on the first tour slide ("Present"), with Continue and Skip Tour.

Not captured: the account step, the sign-in wall, email confirmation and the coach tips in the running app (the debug window could not be driven from the background); those flows are covered by `crates/ui-account/tests/flow.rs` against a fake backend. Not run against the local server. No Electron side-by-side.
