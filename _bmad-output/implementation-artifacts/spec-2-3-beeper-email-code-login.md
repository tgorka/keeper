---
title: 'Beeper Email-Code Login'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '8711e72266ea1c3034a84cd0e71d81f58a0b4c21'
final_revision: '881846e247b6b60b9337e206b90c810177a5581c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper can sign in with password (Story 1.3) and OIDC (Story 2.2), but not Beeper. Beeper users have no Matrix password; adding their account requires Beeper's unofficial email-code flow (`api.beeper.com` → JWT → `org.matrix.login.jwt` against `matrix.beeper.com`). FR-3 requires this as a third login path funnelled through the same account pipeline.

**Approach:** Add a third `AuthProvider` impl behind the shared `add_account` orchestration (Story 2.1). A new tauri-free `auth::beeper` module owns **all** `api.beeper.com` HTTP: a two-step flow (submit email → request emailed code, then submit code → obtain JWT) whose intermediate login-request id is held server-side in a `BeeperFlowRegistry` (never crossing IPC), followed by a `BeeperAuthProvider` that completes login via matrix-sdk's `login_custom("org.matrix.login.jwt", …)`. Every Beeper failure collapses into one typed `BeeperUnavailable` state so private-API breakage cannot leak into core Matrix login and is invisible to other accounts. A new Beeper tab on the login screen (permanently labelled "Unofficial API — may break without notice") drives the two steps.

## Boundaries & Constraints

**Always:**
- All `api.beeper.com` HTTP lives ONLY in `keeper-core/src/auth/beeper.rs` (AD-17 containment). No Beeper HTTP in the `keeper` shell crate or in TypeScript.
- Beeper login funnels through the existing `add_account` pipeline (SSS gate → store → Keychain → registry → hue → `AccountVm`). Homeserver is fixed to `https://matrix.beeper.com`; the Beeper tab must NOT ask for a homeserver.
- The Beeper session persists exactly like a password session: `login_custom` yields a `MatrixSession`, stored as `StoredSession::Password` — no changes to persistence, restore, or sign-out.
- Every Beeper failure (non-2xx, network error, request timeout, missing/renamed JSON fields, JWT/Matrix-login rejection, or an abandoned flow whose request id is gone) surfaces as `AuthError::BeeperUnavailable(String)` — the message must contain no secrets (no email code, no JWT, no bearer). All Beeper HTTP calls carry an explicit request timeout so nothing hangs.
- Cancelling/abandoning the Beeper flow leaves zero residue: no pending registry entry, no store dir, no Keychain entry (rely on `add_account` rollback + registry cleanup).
- keeper-core stays tauri-free (the crate-boundary test must keep passing): the Beeper module is plain reqwest/tokio/serde.
- Rust: no `.unwrap()`/`unsafe` in production paths; `#[non_exhaustive]` catch-alls preserved; `reqwest` added with `default-features = false, features = ["json", "rustls-tls"]` (matches matrix-sdk's rustls backend) and must pass `cargo deny`.

**Block If:**
- The `api.beeper.com` login flow shape has materially changed from `/user/login` → `/user/login/email` → `/user/login/response` such that no JWT can be obtained (i.e. the documented endpoints/fields no longer exist). Do not reverse-engineer a new private protocol unattended — HALT `blocked` with the observed shape.

**Never:**
- No Beeper coverage-gap disclosure card (Story 2.4), no account switcher / per-account glyph (Story 2.5), no at-rest passphrase choice (Story 2.6).
- Never store or log the emailed code, the JWT, or the bearer token; never return them across IPC.
- Do not special-case Beeper in the inbox/room-list/timeline — Beeper rooms (Matrix-native, cloud-Bridge, bbctl-Bridge) flow through the existing sync/inbox unchanged.
- Do not add a second TLS stack (no `native-tls`/OpenSSL); no new Matrix JS lib.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Send code — happy | valid Beeper email | `/user/login` + `/user/login/email` succeed; `request` id stored in registry for that email; UI advances to code step | none |
| Verify — happy | correct emailed code, request id present | `/user/login/response` returns JWT; `add_account("https://matrix.beeper.com", BeeperAuthProvider{jwt})` yields syncing `AccountVm`; registry entry removed | none |
| API rejects / wrong code | non-2xx from any step | `AuthError::BeeperUnavailable` → `beeperUnavailable` (retriable); no partial account | Named "Beeper login unavailable" UI, Retry returns to email step |
| API timeout / offline | request exceeds timeout or transport error | `AuthError::BeeperUnavailable`; never hangs/spinners | Same named state + Retry |
| Shape change | 2xx but `request`/`token` field missing/renamed | parse fails → `AuthError::BeeperUnavailable` | Same named state + Retry |
| Non-SSS homeserver | Beeper server lacks MSC4186 | `add_account` Phase-A fails `SlidingSyncUnsupported` before any store/Keychain write | SSS-named error (JWT already fetched is discarded; zero keeper residue) |
| Verify without send | `login_beeper` called with no registry entry (abandoned/expired) | `AuthError::BeeperUnavailable` | Named state; user restarts at email step |
| Cancel / unmount mid-flow | user closes overlay | `cancel_beeper` clears registry; nothing persisted | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/auth.rs` -- declare `mod beeper;` and re-export its public entry points; `add_account`, `StoredSession`, `map_login_error`, rollback stay unchanged and are reused. `BeeperAuthProvider` produces a `StoredSession::Password` via the existing `from_client` path.
- `src-tauri/crates/keeper-core/src/auth/beeper.rs` -- NEW. Owns ALL `api.beeper.com` HTTP. Contains: `BEEPER_HOMESERVER = "https://matrix.beeper.com"`, bearer constant `BEEPER-PRIVATE-API-PLEASE-DONT-USE`, request timeout; `BeeperFlowRegistry { http: reqwest::Client, pending: Mutex<HashMap<String,String>> }` with `request_code(email)` (POST `/user/login` → parse `request`; POST `/user/login/email` {request,email}; store request keyed by email), `login(platform, email, code)` (take request; POST `/user/login/response` {request, response:code} → parse `token` JWT; `super::add_account(platform, BEEPER_HOMESERVER, &BeeperAuthProvider{jwt})`; remove entry), `cancel_all()`; pure helpers `parse_login_request(&str)`/`parse_jwt(&str)`/body builders; `BeeperAuthProvider { jwt }` impl of `AuthProvider` calling `client.matrix_auth().login_custom("org.matrix.login.jwt", data)?.initial_device_display_name("keeper").send()`. Every non-2xx/transport/timeout/parse error → `AuthError::BeeperUnavailable`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `AuthError::BeeperUnavailable(String)` (retriable; message carries no secrets), near the OAuth variants.
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `IpcErrorCode::BeeperUnavailable` (serialises `beeperUnavailable`), `#[ts(export)]`.
- `src-tauri/crates/keeper-core/Cargo.toml` + `src-tauri/Cargo.toml` -- add `reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls"] }` (workspace + keeper-core).
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState` gains `beeper_flows: Arc<BeeperFlowRegistry>`; add commands `beeper_request_code(email) -> Result<(), IpcError>`, `login_beeper(email, code) -> Result<AccountVm, IpcError>`, `cancel_beeper() -> Result<(), IpcError>`; map `AuthError::BeeperUnavailable` → `IpcErrorCode::BeeperUnavailable` (retriable=true) in `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- register the three commands in `generate_handler!`; construct `beeper_flows` in `AppState::new`.
- `src/lib/ipc/client.ts` -- add `beeperRequestCode(email): Promise<void>`, `loginBeeper(email, code): Promise<AccountVm>`, `cancelBeeper(): Promise<void>`.
- `src/components/auth/login-screen.tsx` -- introduce shadcn `Tabs` ("Password" wrapping the existing password+SSO form; "Beeper" new). Beeper tab: permanent subtitle "Unofficial API — may break without notice"; email step (email input + "Send code") → code step (code input + "Verify"); on success `addAccount`+`onDone`; `BeeperUnavailable` renders the named failure "Beeper login unavailable — this is an unofficial API and may have changed." with Retry (→ email step) and a status link; unmount-while-pending calls `cancelBeeper` (mirror the OIDC cleanup ref).
- `src/lib/ipc/gen/*` -- regenerate ts-rs bindings (`IpcErrorCode` gains `beeperUnavailable`; `AccountVm` unchanged).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/error.rs` -- Add `AuthError::BeeperUnavailable(String)` with a clear, secret-free message.
- [x] `keeper-core/src/vm.rs` -- Add `IpcErrorCode::BeeperUnavailable` (`#[ts(export)]`, camelCase).
- [x] `keeper-core/Cargo.toml` + `src-tauri/Cargo.toml` -- Add `reqwest` (`default-features=false`, `rustls`, `json`); `cargo deny` licenses/bans/sources pass. (reqwest 0.13's feature is `rustls`, not `rustls-tls`.)
- [x] `keeper-core/src/auth/beeper.rs` -- NEW module: `BeeperFlowRegistry` (`request_code`/`login`/`cancel_all`), pure request/response helpers (parse `request`, parse `token`, build bodies), `BeeperAuthProvider` (JWT `login_custom`), timeout + full failure→`BeeperUnavailable` mapping. All `api.beeper.com` HTTP confined here.
- [x] `keeper-core/src/auth.rs` -- `pub mod beeper;` + re-exports; reuse `add_account`/`StoredSession`/rollback unchanged.
- [x] `keeper/src/ipc.rs` -- `AppState.beeper_flows`; `beeper_request_code`/`login_beeper`/`cancel_beeper` commands; map the new error in `to_ipc_error` (retriable=true).
- [x] `keeper/src/lib.rs` -- Construct `beeper_flows`; register the three commands.
- [x] `src/lib/ipc/client.ts` -- Add `beeperRequestCode`/`loginBeeper`/`cancelBeeper`; regenerate ts-rs bindings.
- [x] `src/components/auth/login-screen.tsx` -- Add the Beeper tab (permanent unofficial-API subtitle, email→code steps, named failure + Retry + status link, `addAccount` on success, unmount cancel).
- [x] Tests -- Rust: `BeeperFlowRegistry` (store/take/take-missing→`BeeperUnavailable`, `cancel_all` clears), pure parsers (valid `request`/`token`, and missing/renamed field → error i.e. shape-change), `to_ipc_error` maps `BeeperUnavailable`→`beeperUnavailable` retriable, keeper-core tauri-free boundary test still green. Frontend: Beeper tab renders permanent subtitle; email step → `beeperRequestCode` advances to code step; code step → `loginBeeper` → `addAccount`+`onDone`; `beeperUnavailable` rejection renders the named failure with Retry (→ email step) + status link; blank email/blank code guarded; unmount-mid-flow calls `cancelBeeper`; `client.ts` invokes the right commands.

**Acceptance Criteria:**
- Given the Add Account surface, when the Beeper tab renders, then it is permanently subtitled "Unofficial API — may break without notice" as part of the form (not a dismissible hint) and asks only for email then code — no homeserver field.
- Given a valid Beeper email and correct emailed code, when the user completes the flow, then keeper runs `/user/login` → `/user/login/email` → `/user/login/response` → JWT → `org.matrix.login.jwt` against `matrix.beeper.com`, produces a syncing Beeper `AccountVm` merged into the inbox alongside password/OIDC accounts, and all `api.beeper.com` HTTP lived only in `auth::beeper`.
- Given the Beeper API rejects, times out, or changes shape, when login fails, then the UI shows the distinct "Beeper login unavailable — this is an unofficial API and may have changed." state with Retry and a status link — never a hang, spinner, or crash — no partial account/store/Keychain residue, and the failure is unobservable from non-Beeper accounts (typed `BeeperUnavailable`, contained in the beeper module).
- Given keeper-core, when the crate-boundary test runs, then core stays tauri-free and the Beeper session restores across restart exactly like a password session (stored as `StoredSession::Password`).

## Design Notes

**Two-step flow, server-held request id.** The emailed code round-trip needs the `request` id from step 1 at step 2. Hold it server-side in `BeeperFlowRegistry` keyed by email (mirrors `OAuthFlowRegistry`), so it never crosses IPC. `request_code` performs steps 1–2; `login_beeper` performs step 3 then `add_account`. Retry from the failure state restarts at the email step (a fresh `request_code`), so a stale/expired request id is simply replaced.

Beeper HTTP shape (unofficial — treat any deviation as `BeeperUnavailable`):
```
POST https://api.beeper.com/user/login            Authorization: Bearer BEEPER-PRIVATE-API-PLEASE-DONT-USE   {}            -> { "request": "<id>" }
POST https://api.beeper.com/user/login/email      (same auth)   { "request": "<id>", "email": "<addr>" }              -> 200
POST https://api.beeper.com/user/login/response   (same auth)   { "request": "<id>", "response": "<code>" }           -> { "token": "<JWT>" }
```
The bearer string `BEEPER-PRIVATE-API-PLEASE-DONT-USE` is a public constant every Beeper client sends — not a secret, and not matched by the pre-commit secret scanner. Give the reqwest client an explicit timeout so no step hangs.

**JWT → Matrix login** (matrix-sdk 0.18 has `login_custom`, whose own docs use this exact JWT example):
```rust
let mut data = matrix_sdk::ruma::serde::JsonObject::new();
data.insert("token".to_owned(), serde_json::Value::String(self.jwt.clone()));
client.matrix_auth()
    .login_custom("org.matrix.login.jwt", data)
    .map_err(|e| AuthError::BeeperUnavailable(e.to_string()))?
    .initial_device_display_name("keeper")
    .send().await
    .map_err(|e| AuthError::BeeperUnavailable(e.to_string()))?;
```
`add_account` then extracts the resulting `MatrixSession` via the existing `StoredSession::from_client` (→ `Password` variant) — so persistence, restore, and sign-out need no changes.

**Containment is the whole point (AD-17).** Only `auth::beeper` imports `reqwest` / touches `api.beeper.com`; the shell and TS never do. A single `BeeperUnavailable` variant means a private-API change can only ever produce that one named, retriable state — the "honest degradation" requirement — and cannot corrupt password/OIDC login or be observed from other accounts.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `.unwrap()`/`unsafe`, `#[non_exhaustive]` catch-alls present).
- `bun run test:rust` -- expected: cargo-nextest green incl. new `BeeperFlowRegistry`, parser (happy + shape-change), and `to_ipc_error` tests; keeper-core tauri-free boundary test still passes.
- `bun run check` -- expected: biome + tsc strict + vitest green incl. new login-screen Beeper-tab / `client.ts` tests.
- `cargo deny check` (from `src-tauri/`) -- expected: pass with `reqwest` (rustls) added; no GPL/AGPL, no second TLS stack.
- ts-rs regeneration -- expected: `src/lib/ipc/gen/IpcErrorCode.ts` gains `beeperUnavailable`, no stale drift.

**Manual checks (if no CLI):**
- Against a real Beeper account: enter email, receive the code, complete login; confirm the account appears syncing in the merged inbox showing Matrix-native + cloud-Bridge + bbctl-Bridge chats; quit/relaunch to confirm restore alongside a password/OIDC account; abandon a flow and confirm no leftover store dir / Keychain entry.
- OQ-3 exit check (real Beeper account, hungryserv surface): verify `thirdparty/protocols`, custom account data, `m.read.private`, and push rules behave; record any gaps as per-feature degradation notes appended to `deferred-work.md` for later epics (bridges/incognito/notifications). This is a manual, real-account check — not automatable here.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 1
- reject: 9
- addressed_findings:
  - `[medium]` `[patch]` Secret-free contract breach: `BeeperAuthProvider::authenticate` mapped `login_custom` failures with raw `AuthError::BeeperUnavailable(e.to_string())`, interpolating an unbounded matrix-sdk error string into the one error path that holds the JWT — contradicting the module's/`error.rs`'s asserted "message carries no secrets" guarantee and risking leakage into `tracing` on add failure. Fixed: both arms now emit fixed secret-free strings ("could not start the Beeper JWT login" / "the Beeper JWT login was rejected") and log the raw error separately at `debug`.
  - `[low]` `[patch]` Registry residue on abandon: the `BeeperTab` unmount cleanup only cancelled the backend flow when `pending` was true, but the code step sits idle with `pending === false` while a request id is stored in `BeeperFlowRegistry` — so dismissing the overlay / switching tabs at the code step leaked a registry entry until process exit, violating the "abandoning leaves zero residue: no pending registry entry" invariant. Fixed: track a `flowStartedRef` set on `beeper_request_code` success and cleared on completed login; unmount cancels when `pending || flowStarted`. Added a code-step-idle unmount test.
  - `[low]` `[patch]` Empty-field shape-change gap: `parse_login_request`/`parse_jwt` accepted a present-but-empty `request`/`token` (`{"token":""}`) as success, storing a blank request id or handing an empty JWT to `login_custom` for a doomed round-trip. Fixed: both parsers `.filter(|s| !s.is_empty())` so an empty field takes the `BeeperUnavailable` shape-change path. Added two parser tests.
  - `[low]` `[patch]` Timeout invariant could be silently dropped: `BeeperFlowRegistry::new` used `.build().unwrap_or_default()`, and `reqwest::Client::default()` has no timeout — so a client-build failure would forfeit the mandatory per-request timeout. Fixed: `.expect(...)` at startup (idiomatic here per `lib.rs:59`; only `unwrap_used` is linted), surfacing a broken-TLS config loudly instead of shipping a timeout-less client.
- notes: Blind Hunter + Edge Case Hunter reviewed the full baseline→working-tree diff. Deferred 1 (below). Rejected 9: the "non-SSS/unreachable Beeper server shows the wrong error copy" finding is a mis-trace — the Beeper tab's `catch` is a catch-all that collapses every error (incl. `serverUnreachable`/`slidingSyncUnsupported`) to the named unavailable state, and the fetch-JWT-then-SSS-probe ordering is by-design per the I/O matrix; the unnormalized-email-key and whitespace-to-backend findings have no trigger (the frontend trims and disables the email input across steps, guaranteeing an identical key); the "use a different email" affordance and tab-switch-loses-progress are UX enhancements beyond the spec with the coupling enforced by the disabled input; the missing HTTP-orchestration tests contradict the spec's conscious scoping (pure-fn tests + manual network check); the per-request-vs-operation timeout meets the spec's literal per-call requirement and wrapping `add_account` in an external timeout would defeat its rollback-on-error (cancellation skips it); the bearer-allowlist and doc-drift findings are subsumed by the secret-free patch.

## Auto Run Result

Status: **done**

**Summary.** Added Beeper email-code login as a third `AuthProvider` impl (`BeeperAuthProvider`) behind the Story 2.1 shared `add_account` orchestration (FR-3, AD-17). A new tauri-free `auth::beeper` module (`src/auth/beeper.rs`) owns **all** `api.beeper.com` HTTP: a two-step flow (`beeper_request_code` runs `POST /user/login` → `POST /user/login/email`; `login_beeper` runs `POST /user/login/response` → JWT) whose intermediate login-request id is held server-side in a `BeeperFlowRegistry` keyed by email so it never crosses IPC. `BeeperAuthProvider` completes login via matrix-sdk's `login_custom("org.matrix.login.jwt", …)` against the fixed `matrix.beeper.com`; the resulting `MatrixSession` persists/restores as `StoredSession::Password` with no changes to persistence, restore, or sign-out. Every Beeper failure (non-2xx, transport, timeout, shape change, JWT rejection, abandoned flow) collapses to one typed, secret-free `AuthError::BeeperUnavailable` → `IpcErrorCode::BeeperUnavailable` (retriable), so private-API breakage cannot leak into core Matrix login and is unobservable from other accounts. The login screen was refactored into shadcn `Tabs` ("Password & SSO" wrapping the unchanged password/OIDC form; a new "Beeper" tab: permanent "Unofficial API — may break without notice" subtitle, email→code steps, the named unavailable state with Retry + a Beeper status link, and unmount-cancel).

**Files changed (one-line each).**
- `src-tauri/crates/keeper-core/src/auth/beeper.rs` — NEW: `BeeperFlowRegistry` (request_code/login/cancel_all + store/take), pure parsers/body-builders, `BeeperAuthProvider` (JWT `login_custom`), 30s per-request timeout, full failure→`BeeperUnavailable` mapping, 17 colocated tests.
- `src-tauri/crates/keeper-core/src/auth.rs` — `pub mod beeper;` + re-exports; `add_account`/`StoredSession`/rollback reused unchanged.
- `src-tauri/crates/keeper-core/src/error.rs` — `AuthError::BeeperUnavailable(String)` (secret-free).
- `src-tauri/crates/keeper-core/src/vm.rs` — `IpcErrorCode::BeeperUnavailable` (`beeperUnavailable`) + serialization test.
- `src-tauri/crates/keeper/src/ipc.rs` — `AppState.beeper_flows`; `beeper_request_code`/`login_beeper`/`cancel_beeper` commands; `to_ipc_error` mapping (retriable) + test.
- `src-tauri/crates/keeper/src/lib.rs` — construct `beeper_flows`; register the three commands.
- `src-tauri/Cargo.toml` + `keeper-core/Cargo.toml` (+ `Cargo.lock`) — `reqwest` (`default-features=false`, `rustls`, `json`; no second TLS stack).
- `src/lib/ipc/client.ts` — `beeperRequestCode`/`loginBeeper`/`cancelBeeper` wrappers.
- `src/lib/ipc/gen/IpcErrorCode.ts` — regenerated (gained `beeperUnavailable`).
- `src/components/auth/login-screen.tsx` — Tabs refactor + Beeper tab.
- `src/components/auth/login-screen.test.tsx` — Beeper-tab tests (incl. code-step-idle unmount cancel).

**Review findings breakdown.** intent_gap 0, bad_spec 0, patch 4 (1 medium, 3 low), defer 1, reject 9. Patches applied: (medium) fixed secret-free JWT-login error messages replacing raw `e.to_string()`; (low) cancel the backend flow on unmount whenever a request id may exist (not only while `pending`) — closes a registry-residue leak at the idle code step; (low) reject empty-but-present `request`/`token` fields as a shape change; (low) `expect` the Beeper HTTP client build at startup so the timeout invariant can't be silently dropped. Deferred 1: `cancel_all` clears all flows (becomes relevant with Story 2.5 concurrent adds — per-email cancel there). Rejected 9 (mis-traced "wrong error copy", by-design SSS ordering, no-trigger email-key/whitespace findings, UX enhancements, spec-scoped test coverage, per-call-vs-operation timeout, and docs subsumed by the secret-free patch).

**Verification.** `bun run check:rust` PASS (rustfmt + clippy `-D warnings`; no `.unwrap()`/`unsafe`). `bun run test:rust` PASS (156/156; ts-rs bindings regenerated). `bun run check` PASS (biome + tsc strict + vitest 190/190 + keeper-core tauri-free boundary check). `cargo deny check licenses bans sources` PASS (reqwest on rustls; no GPL/AGPL, no OpenSSL/native-tls) — the `advisories` subcommand fails only on pre-existing unmaintained-crate advisories from Tauri's tree (baseline-identical; this change adds no new crates to the graph).

**Residual risks.** The manual real-Beeper-account checks and the OQ-3 hungryserv exit check (`thirdparty/protocols`, custom account data, `m.read.private`, push rules) are inherently non-automatable here and were not performed — they require a live Beeper account and produce degradation notes for later epics. The Beeper flow depends on an unofficial API (`api.beeper.com`) whose shape can change without notice; the module is built to degrade honestly to `BeeperUnavailable` if it does, but a shape change will require a code update to restore functionality.
