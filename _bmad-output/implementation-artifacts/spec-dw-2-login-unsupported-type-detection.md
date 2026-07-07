---
title: 'Pre-login supported-flows probe so password-login-disabled homeservers are named honestly'
type: 'bugfix'
created: '2026-07-06'
status: 'done'
baseline_revision: 'aa3762da93ebd11c9e8717556d4981ca366cbce7'
final_revision: 'f8f82bc11b8fd1ebedcec5bb0d21717b7e15518e'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** A homeserver with password login disabled returns `M_FORBIDDEN` on `matrix_auth().login_username()` — the *same* errcode as a genuinely wrong password — so `auth::map_login_error` maps it to `InvalidCredentials` and the user sees "Wrong username or password" instead of "This homeserver doesn't support password login." The error-kind mapping alone cannot tell the two cases apart (DW-2).

**Approach:** Add a pre-login supported-flows probe in `PasswordAuthProvider::authenticate`: call `client.matrix_auth().get_login_types()` (GET `/login`) *before* `login_username`, and if the advertised flows do not include `m.login.password`, return `AuthError::UnsupportedLoginType` up front. `map_login_error` is unchanged and remains the fallback for genuine credential rejections and for every case the probe cannot positively decide.

## Boundaries & Constraints

**Always:**
- Probe with `client.matrix_auth().get_login_types()` inside `PasswordAuthProvider::authenticate`, before `login_username(...).send()`.
- Detect password support via the flow's login-type discriminant (`m.login.password`), through a small pure helper that takes the flow list so the classification is unit-testable without a network.
- Only a **successful** probe whose flows omit `m.login.password` yields `UnsupportedLoginType`. A probe transport/HTTP failure is **non-fatal**: log at `info` and fall through to the `login_username` attempt, whose own error (via `map_login_error`) stays the source of truth. This guarantees the probe never turns a would-succeed login into a failure.
- Keep `map_login_error` and its existing `Forbidden`/`Unauthorized` → `InvalidCredentials` and `Unrecognized`/`InvalidParam`/`MissingParam` → `UnsupportedLoginType` branches intact as the fallback.
- Keep the probe secret-free: the `UnsupportedLoginType(String)` payload is a non-secret description; never log or wrap the username/password.

**Block If:** (none — self-contained keeper-core change with an established error taxonomy)

**Never:**
- Do not change the `AuthError` / `IpcErrorCode` taxonomy or the `to_ipc_error` funnel — `UnsupportedLoginType` already exists and maps to `IpcErrorCode::UnsupportedLoginType` (`retriable: false`).
- Do not add the probe to the OIDC or Beeper providers — DW-2 is password-login classification only.
- Do not move or duplicate the SSS capability probe in `add_account` (Phase A), and do not add any new persistent state — the probe runs on the already-built Phase-B client.
- Do not add a new network mock framework (no wiremock) — test the pure classifier, not the live HTTP call.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Password disabled | probe succeeds, flows = `[m.login.sso]` (no password) | `authenticate` returns `Err(AuthError::UnsupportedLoginType)` before any `login_username` call | non-retriable; `login_username` never attempted |
| Password offered, right creds | probe succeeds, flows include `m.login.password`; login accepted | proceeds to `login_username`, login succeeds | none |
| Password offered, wrong creds | probe succeeds, flows include `m.login.password`; `login_username` → `M_FORBIDDEN` | proceeds to `login_username`; `map_login_error` → `InvalidCredentials` | "Wrong username or password" (unchanged) |
| Probe request fails | `get_login_types()` returns `Err` (transport/HTTP) | probe result ignored (logged at info); falls through to `login_username`, whose error is authoritative | preserves pre-DW-2 behavior exactly |
| Custom/unknown flows only | probe succeeds, flows = `[m.login.token]`, `[some.custom.type]` | `UnsupportedLoginType` (no `m.login.password` present) | non-retriable |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/auth.rs` -- `PasswordAuthProvider::authenticate` (~line 96): insert the `get_login_types()` probe before `login_username`. Add a pure `flows_include_password(&[LoginType]) -> bool` helper. `map_login_error` (~line 387) is unchanged.
- `src-tauri/crates/keeper-core/src/error.rs` -- reference only: `AuthError::UnsupportedLoginType(String)` already exists (no edit).
- `src-tauri/crates/keeper/src/ipc.rs` -- reference only: `to_ipc_error` already maps `UnsupportedLoginType` → `IpcErrorCode::UnsupportedLoginType` (no edit).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/auth.rs` -- import `matrix_sdk::ruma::api::client::session::get_login_types::v3::LoginType`; add a pure `fn flows_include_password(flows: &[LoginType]) -> bool` that returns `true` iff some flow's `login_type()` equals `"m.login.password"`. In `PasswordAuthProvider::authenticate`, bind `let auth = client.matrix_auth();`, then `match auth.get_login_types().await`: on `Ok(types)` where `!flows_include_password(&types.flows)` return `Err(AuthError::UnsupportedLoginType(...).into())`; on `Err(e)` log `tracing::info!` and continue. Then run the existing `auth.login_username(...)` path (still mapped through `map_login_error`).
- [x] `src-tauri/crates/keeper-core/src/auth.rs` (tests) -- add unit tests for `flows_include_password`: password present among mixed flows → `true`; SSO-only → `false`; empty → `false`; custom/unknown-only (`m.login.token`, a custom type) → `false`. Construct `LoginType` values via `LoginType::new(type_str, JsonObject::default())`.

**Acceptance Criteria:**
- Given a homeserver whose `/login` flows omit `m.login.password`, when a password login is attempted, then `authenticate` returns `UnsupportedLoginType` (→ `IpcErrorCode::UnsupportedLoginType`, "This homeserver doesn't support password login") and `login_username` is never called.
- Given a homeserver that offers `m.login.password` but rejects the credentials with `M_FORBIDDEN`, when login is attempted, then the classification is still `InvalidCredentials` ("Wrong username or password") — the probe does not change genuine-credential-rejection behavior.
- Given the `get_login_types()` request itself fails, when login is attempted, then the login proceeds and its own error (via `map_login_error`) is surfaced — the probe never converts a would-succeed or transport-failed login into a spurious `UnsupportedLoginType`.
- Given `bun run test:rust` and `bun run check:rust` run, then keeper-core tests, `cargo fmt --check`, and `clippy -D warnings` all pass.

## Spec Change Log

_No bad_spec loopbacks — empty._

## Review Triage Log

### 2026-07-06 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 0
- reject: 13: (high 0, medium 0, low 13)
- addressed_findings:
  - none

Edge Case Hunter returned **0 findings** — every branch of the 5-part behavioral contract (probe-absent → `UnsupportedLoginType`; probe-present → login; genuine `M_FORBIDDEN` → `InvalidCredentials`; probe `Err` → non-fatal fall-through; custom/token-only → `UnsupportedLoginType`) is explicitly handled, and no meaningful code was deleted (the `login_username` chain is preserved verbatim, only re-bound through the `auth` local). Blind Hunter's 13 findings were all **rejected**: (1) the extra `GET /login` round-trip, (5) the hardcoded discriminant string vs a variant match, (7) `info` vs `warn`, and (11) empty-flows→unsupported are all the **spec-mandated approach** — reviewer conceded the string/variant choice has equal `#[non_exhaustive]` forward-compat; (2) the non-fatal probe leaving the compound "probe-fails + password-disabled + `M_FORBIDDEN`" case reachable is the spec's explicit I/O-matrix row and the DW-2 ledger's own "low user impact" acknowledgment (strictly additive over pre-DW-2); (3) Phase-B placement is spec-required and `rollback()` already guarantees zero residue on the error path; (8) no integration test of the `match` arms — the spec **forbade** adding a network mock framework and the pure classifier is tested (auth's live path is a pre-existing untested area per spec-1-3); (10) the `login_type()` binding **is** exercised (verified in the ruma source: `LoginType::new("m.login.password", …)` yields the real `Password` variant whose `login_type()` returns the canonical string); (9) the two `UnsupportedLoginType` strings are non-surfaced log descriptions (IPC maps by code); (4/13) UIA/per-user and SSO-steering are out of scope; (6/12) success-path logging and comment density are low-consequence nits.

## Design Notes

The probe is a *positive* discriminator, not a gate: it can only turn a login into `UnsupportedLoginType` when it definitively knows the homeserver does not advertise `m.login.password`. Every uncertain path (probe HTTP/transport failure) falls through to the real login attempt, so the change is strictly additive over today's behavior — the worst case degrades to exactly the pre-DW-2 `map_login_error` classification.

Detection keys off the SDK's `LoginType::login_type()` string rather than matching the `Password(_)` variant, so it stays correct across ruma's `#[non_exhaustive]` enum and needs no variant-shape assumptions. Extracting `flows_include_password` keeps the decision pure and unit-testable without introducing a homeserver mock.

Example shape:

```rust
use matrix_sdk::ruma::api::client::session::get_login_types::v3::LoginType;

/// Whether the homeserver's advertised login flows include `m.login.password`.
fn flows_include_password(flows: &[LoginType]) -> bool {
    flows.iter().any(|f| f.login_type() == "m.login.password")
}

// in authenticate(), before login_username:
let auth = client.matrix_auth();
match auth.get_login_types().await {
    Ok(types) if !flows_include_password(&types.flows) => {
        return Err(AuthError::UnsupportedLoginType(
            "homeserver does not offer m.login.password".to_owned(),
        )
        .into());
    }
    Ok(_) => {}
    Err(e) => tracing::info!(error = %e, "login-types probe failed; proceeding to login attempt"),
}
```

## Verification

**Commands:**
- `bun run test:rust` -- expected: keeper-core suite passes, including the new `flows_include_password` tests.
- `bun run check:rust` -- expected: `cargo fmt --check` clean and `clippy -D warnings` clean.
- `bun run check:core-tauri-free` -- expected: keeper-core stays tauri-free (no new tauri deps pulled in).

**Manual checks (if no CLI):**
- The live end-to-end confirmation (a real password-disabled homeserver returning `UnsupportedLoginType` rather than `InvalidCredentials`) needs a network homeserver and is out of scope for this unattended run; the pure-classifier tests plus the non-fatal-probe fall-through cover the decision logic.

## Auto Run Result

Status: done

**Summary:** DW-2 resolved. `PasswordAuthProvider::authenticate` now runs a pre-login supported-flows probe — `client.matrix_auth().get_login_types()` (GET `/login`) — *before* `login_username`. When the probe succeeds and the advertised flows omit `m.login.password`, login is classified up front as `AuthError::UnsupportedLoginType` (→ non-retriable `IpcErrorCode::UnsupportedLoginType`, "This homeserver doesn't support password login") instead of the misleading `InvalidCredentials` ("Wrong username or password") that a password-login-disabled homeserver's `M_FORBIDDEN` used to produce. The probe is a *positive* discriminator: a probe transport/HTTP failure is non-fatal (logged at `info`, falls through to the real login), so the change is strictly additive over pre-DW-2 behavior and never turns a would-succeed login into a spurious failure. `map_login_error` is unchanged and remains the fallback for genuine credential rejections.

**Files changed:**
- `src-tauri/crates/keeper-core/src/auth.rs` — imported `get_login_types::v3::LoginType`; added the pure, unit-tested `flows_include_password(&[LoginType]) -> bool` helper (keys off `LoginType::login_type()` discriminant string, `#[non_exhaustive]`-safe); inserted the `get_login_types()` probe (3-arm match: absent→`UnsupportedLoginType`, present→proceed, `Err`→log-and-fall-through) before `login_username`; added 4 `#[test]` fns for the classifier. `map_login_error`, `AuthError`, `to_ipc_error`, and the SSS probe untouched; no new dependencies.

**Review findings breakdown:**
- Patches applied: 0.
- Deferred: 0 (no confident new pre-existing issues surfaced).
- Rejected: 13 (all Blind Hunter; Edge Case Hunter found 0). Notable rejects: the extra `GET /login` round-trip, the discriminant-string vs variant-match choice, `info`-vs-`warn`, and empty-flows→unsupported are the **spec-mandated approach** (reviewer conceded string/variant have equal `#[non_exhaustive]` forward-compat); the non-fatal-probe residual (probe-fails + password-disabled + `M_FORBIDDEN` still → `InvalidCredentials`) is the spec's explicit I/O-matrix row and the DW-2 ledger's own "low user impact" acknowledgment; Phase-B placement is spec-required with `rollback()` guaranteeing zero residue; the "no integration test of the match arms" finding is bounded by the spec's explicit "no wiremock" boundary (the pure classifier is tested, the match is thin glue, and auth's live path is a pre-existing untested area); the "`login_type()` binding unverified" claim is false (verified against ruma source that `LoginType::new("m.login.password", …)` yields the real `Password` variant reporting the canonical string).

**Verification:** `cargo test -p keeper-core --lib auth` — `51 passed; 0 failed` (incl. the 4 new classifier tests); `cargo fmt --all --check` — clean; `cargo clippy -p keeper-core --all-targets -- -D warnings` — clean, exit 0; `bun run check:core-tauri-free` — passes (keeper-core stays tauri-free). All run independently by this session, not just reported by the subagent.

**Residual risks:** (1) The probe only *reliably* fixes the misclassification when the `GET /login` succeeds; if the probe request itself fails while a password-disabled homeserver's `login_username` still returns `M_FORBIDDEN`, the classification falls back to `InvalidCredentials` — a rare compound case the spec and the DW-2 ledger consciously accept as low-impact (both outcomes are non-retriable password-login failures). (2) The new `match` arms in `authenticate` are not covered by an integration test (no matrix-sdk mock framework in the project, per spec boundary); the extracted `flows_include_password` classifier — the only non-trivial decision — is fully unit-tested, and the live login path is a pre-existing untested area documented since spec-1-3. (3) A homeserver advertising `m.login.password` that still rejects the specific identifier/UIA stage continues to surface as `InvalidCredentials` — unchanged, out of DW-2 scope.
