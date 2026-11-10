# Plan 001: Verify licence leases over the claims exactly as received

> **Executor instructions**: Follow this plan step by step. Run every
> verification command and confirm the expected result before moving to the
> next step. If anything in the "STOP conditions" section occurs, stop and
> report — do not improvise. When done, update the status row for this plan
> in `plans/README.md` — unless a reviewer dispatched you and told you they
> maintain the index.
>
> **Drift check (run first)**: `git diff --stat a2628bd..HEAD -- crates/kairo-cloud/src/licensing.rs crates/kairo-domain/src/cloud.rs crates/kairo-cloud/tests/licensing.rs`
> If any in-scope file changed since this plan was written, compare the
> "Current state" excerpts against the live code before proceeding; on a
> mismatch, treat it as a STOP condition.

## Status

- **Priority**: P1
- **Effort**: M
- **Risk**: MED
- **Depends on**: none
- **Category**: bug (forward compatibility of a security check)
- **Planned at**: commit `a2628bd`, 2026-11-10

## Why this matters

The Electron app verifies the Ed25519 lease signature over `JSON.stringify(c)` of the
claims object **as received from the server** — every field, in the server's order.
The Rust port re-serializes its typed `LicenseLeaseClaims` struct instead, which drops
unknown fields and uses struct field order, and the closed `LicensedFeature` enum
fails deserialization on any feature string it does not know. The day the server adds
one claim field (e.g. `plan`) or one new feature (e.g. `"lyrics"`), every native
client rejects every lease and loses transcription, scripture and all other licensed
features, while Electron keeps working. Fix: verify over the raw claims bytes and
parse features leniently.

## Current state

- `crates/kairo-cloud/src/licensing.rs` — `valid_license_lease` (around lines 63–108).
  The signature payload is rebuilt from the typed struct:

  ```rust
  // crates/kairo-cloud/src/licensing.rs:101
  let (Ok(payload), Ok(signature)) = (to_js_string(claims), STANDARD.decode(&lease.signature))
  else {
      return false;
  };
  UnparsedPublicKey::new(&ED25519, key)
      .verify(payload.as_bytes(), &signature)
      .is_ok()
  ```

- `crates/kairo-domain/src/cloud.rs` (around lines 556–650) — the types:

  ```rust
  string_enum!(
      pub enum LicensedFeature {
          Presentation = "presentation", Transcription = "transcription",
          Scripture = "scripture", Summaries = "summaries",
          Translation = "translation", WebSearch = "webSearch",
      }
  );

  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct LicenseLeaseClaims {
      pub org_id: String,
      pub device_id: String,
      pub features: Vec<LicensedFeature>,
      pub issued_at: f64,
      pub refresh_after: f64,
      pub expires_at: f64,
      #[serde(deserialize_with = "required")]
      pub license_expires_at: Option<f64>,
  }

  pub struct SignedLicenseLease {
      pub claims: LicenseLeaseClaims,
      pub signature: String,
      pub public_key: String,
  }
  ```

- Electron reference (`../kairo-presenter/src/main/services/cloud/license-verifier.ts`):
  `verify(null, Buffer.from(JSON.stringify(c)), createPublicKey(publicKey), Buffer.from(lease.signature, 'base64'))`
  where `c = lease.claims` is the parsed object.
- `SignedLicenseLease` is persisted inside `LicenseSnapshot` on disk and is read back
  through serde; the round trip must keep the raw claims.
- `kairo_domain::json::to_js_string` (`crates/kairo-domain/src/json.rs:24`) serializes
  with whole floats as integers, matching `JSON.stringify` for typed fields.
- Existing test: `crates/kairo-cloud/tests/licensing.rs` (`#[ignore]`, needs Node and the
  Electron checkout). Unit tests may also live in `licensing.rs` under `#[cfg(test)]`.
- Workspace `serde_json` features: kairo-domain enables `raw_value` and `float_roundtrip`
  (see `crates/kairo-domain/Cargo.toml`). Some crates enable `preserve_order`; do not rely
  on it — use `RawValue`.

## Commands you will need

| Purpose | Command | Expected on success |
|---|---|---|
| Format check | `cargo fmt --all -- --check` | exit 0 |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 |
| Tests (crate) | `cargo test -p kairo-cloud -p kairo-domain` | all pass |
| Tests (all) | `cargo test --workspace` | 0 failed |

## Scope

**In scope**:
- `crates/kairo-domain/src/cloud.rs` (lease types only)
- `crates/kairo-cloud/src/licensing.rs`
- `crates/kairo-cloud/tests/licensing.rs` (add tests)

**Out of scope**:
- The validity rules themselves (org, device, time windows, 14-day ceiling) — unchanged.
- Any other `string_enum!` type — do not make the macro lenient globally.
- Server code in `../kairo-presenter` — never edit it.

## Git workflow

- Branch: `advisor/001-lease-raw-claims`
- Commit message style matches the repo, e.g. `Verify licence leases over the received claims`.
- Do NOT push or open a PR unless instructed.

## Steps

### Step 1: Keep the raw claims text on `SignedLicenseLease`

Change `SignedLicenseLease` so it deserializes `claims` once as `Box<serde_json::value::RawValue>`
and also exposes the typed view. Recommended shape: a custom `Deserialize` impl (or
`#[serde(from = "...")]` helper struct) that reads `{ claims: Box<RawValue>, signature, publicKey }`,
then parses `LicenseLeaseClaims` from `claims.get()`. Store both:

```rust
pub struct SignedLicenseLease {
    pub claims: LicenseLeaseClaims,
    /// The claims JSON exactly as received; the signature covers this.
    #[serde(skip)] // or handled by the custom impl
    pub raw_claims: Option<Box<RawValue>>,
    pub signature: String,
    pub public_key: String,
}
```

`Serialize` must write the raw claims back verbatim when present (so the persisted
snapshot round-trips byte-identically), else the typed claims.

**Verify**: `cargo check -p kairo-domain -p kairo-cloud` → exit 0.

### Step 2: Parse features leniently

`LicenseLeaseClaims.features` must not fail on unknown strings. Deserialize it as
`Vec<String>` then filter with `LicensedFeature::from_name`, or a `deserialize_with`
helper that drops unknown entries. Unknown fields at the top level of claims are
already ignored by serde (no `deny_unknown_fields`) — confirm that.

**Verify**: `cargo test -p kairo-domain` → all pass.

### Step 3: Verify over the raw text

In `valid_license_lease`, compute the payload as JavaScript does. `JSON.stringify` of a
parsed object re-emits it compactly in key order with JS number formatting. The raw
server text is normally already compact `JSON.stringify` output, but do not assume
that: parse the raw claims into `serde_json::Value` **with key order preserved**
(enable `preserve_order` on kairo-cloud's serde_json, or walk the `RawValue` with a
small order-preserving parser) and serialize with `kairo_domain::json::to_js_string`.
Fall back to the typed struct only when `raw_claims` is `None` (old snapshots).

**Verify**: `cargo test -p kairo-cloud` → all pass.

## Test plan

Add to `crates/kairo-cloud/tests/licensing.rs` (non-ignored tests), generating an
Ed25519 key pair with `ring::signature::Ed25519KeyPair::generate_pkcs8` and signing a
claims string yourself:
1. Claims with an extra field `"plan":"pro"` placed between known fields → valid.
2. Claims with `"features":["scripture","lyrics"]` → valid for `Scripture`, and
   `LicensedFeature` list contains only `Scripture`.
3. Tampered raw claims (change `deviceId` after signing) → invalid.
4. Serialize then deserialize a `SignedLicenseLease` → still valid (raw kept).
Model key handling on how `parse_public_key` expects SPKI PEM in `licensing.rs:50-60`.

## Done criteria

- [ ] `cargo fmt --all -- --check` exits 0
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` exits 0
- [ ] `cargo test --workspace` reports 0 failed, with the 4 new tests passing
- [ ] `grep -n "to_js_string(claims)" crates/kairo-cloud/src/licensing.rs` no longer matches the only payload path (fallback allowed)
- [ ] Only in-scope files modified (`git status`)
- [ ] `plans/README.md` row updated

## STOP conditions

- The server-signed payload is NOT `JSON.stringify` of the claims (check
  `../kairo-presenter/server` signing code if unsure) — report, don't guess.
- Making `preserve_order` a kairo-cloud feature changes outputs of other kairo-cloud
  tests (oracle parity) — stop and report.
- The ignored Nest test (`cargo test -p kairo-cloud -- --ignored verifies_a_lease`) can be
  run locally and fails after your change.

## Maintenance notes

- Any future claims field must still be checked in `valid_license_lease` only if it
  affects validity; the signature now covers it automatically.
- Reviewer: confirm the persisted `LicenseSnapshot` file is unchanged for an existing lease.
