---
title: 'Story 1.3 — Password Login with Sliding-Sync Verification'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '617dd765217681081433f2b4b1051055fd0e0682'
final_revision: 'c0a7d949eed9d3340861afc4a47ed27a55527704'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper renders its three-pane shell unconditionally with static placeholders and has no way to add a Matrix account. Epic 1's whole vertical slice (room list, timeline, send) is blocked until a user can sign in — and signing in must refuse a homeserver that cannot do Simplified Sliding Sync (MSC4186) *before* any account state exists, so no later story ever inherits a half-configured account.

**Approach:** Add a password login flow spanning both layers. In `keeper-core`, an auth module builds a `matrix_sdk::Client` with well-known discovery, probes Simplified Sliding Sync support with a store-less client *before* creating any persistent state, then (on success) generates a ULID account id, opens a persistent SQLite store at `accounts/<ulid>/sdk/`, logs in via `matrix_auth().login_username`, stores the resulting session tokens only in the macOS Keychain, and writes one account row into `keeper.db` (WAL). Errors resolve to a stable, named taxonomy (bad credentials / unreachable / unsupported login type / non-SSS). In the frontend, a full-screen login view collects homeserver + username + password, calls the typed `login_password` command, renders the named error inline, and — on success — records the returned non-secret `AccountVm` in a zustand store that gates the shell.

## Boundaries & Constraints

**Always:**
- All Matrix, crypto, and persistence logic lives in `keeper-core`; the `keeper` shell is IPC/platform glue only. `keeper-core` gains no `tauri` dependency. OS keychain access goes through the existing `Platform` port (AD-6/AD-24).
- **SSS gate ordering (FR-5):** verify Simplified Sliding Sync support with a **store-less** client (default in-memory store, no `sqlite_store`) *before* generating a ULID, creating any directory, writing the Keychain, or inserting a `keeper.db` row. A non-SSS (or unreachable) server must leave **zero** persistent state behind. Probe via `Client::available_sliding_sync_versions().await` — SSS is supported iff the returned `Vec` contains the native version.
- **Well-known discovery (FR-1):** accept a bare domain (`example.org`) or a full URL; resolve via `ClientBuilder::server_name_or_homeserver_url(..)` which does `/.well-known/matrix/client` discovery and falls back to treating the input as a URL. Reuse the resolved `client.homeserver()` URL for the persistent build so discovery runs once.
- **Secret containment (NFR-9, AD-3, AD-10):** the session (access + refresh tokens, serialized `MatrixSession` JSON) is written **only** to the macOS Keychain via the `keyring` crate (service `dev.tgorka.keeper`). `keeper.db` stores **no** token column — only non-secret registry fields. Tokens, refresh tokens, crypto/device keys, and any `MatrixSession` material **never** cross IPC back to TypeScript and never touch any TS-reachable storage (zustand/localStorage/sessionStorage/IndexedDB). The `AccountVm` returned to TS carries only `{ accountId, userId, homeserverUrl }`.
- **Password in transit:** the password crosses IPC exactly once, TS→Rust, as a transient `login_password` argument used only to drive the SDK login. It is never returned to TS, never stored in any store, never logged, and the login form clears its password field after submit. (This is the only sanctioned interpretation of "no password crosses IPC" — the webview is the only UI, so password collection is inherently in TS; the invariant is *no persistence and no round-trip*, matching NFR-9's intent.)
- **Never half-configured (AC "never a half-configured one"):** any failure *after* the persistent store dir is created (login failure, keychain failure, db failure) rolls back — remove `accounts/<ulid>/sdk/`, delete any Keychain entry written, and leave no `keeper.db` row — then returns the error.
- **Observability (AD-21):** log the SSS probe result and login outcome per account via `tracing`, keyed by `account_id`. **Never** log passwords, tokens, or `MatrixSession` contents.
- **Error taxonomy:** extend `IpcErrorCode` with `slidingSyncUnsupported`, `invalidCredentials`, `serverUnreachable`, `unsupportedLoginType`; map each `keeper-core` `AuthError` variant through the single `to_ipc_error` funnel. `serverUnreachable` is `retriable: true`; the others are `retriable: false`.
- **All SQLite in WAL (NFR-8):** open `keeper.db` with `PRAGMA journal_mode=WAL`.
- TS: no `any`, `import type` for types, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed shadcn primitives (`Card`, `Input`, `Label`, `Button`, `Alert`) — do not hand-write in `src/components/ui/`. Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` not `println!`.
- Regenerate the ts-rs bindings in `src/lib/ipc/gen/` from the new/changed VMs and commit them (the generated `AccountVm.ts` and updated `IpcErrorCode.ts` must match `cargo` output).

**Block If:**
- The pinned matrix-sdk 0.18 does not expose a way to detect Simplified Sliding Sync support without creating persistent account state (i.e. no `available_sliding_sync_versions` / store-less probe path) — this would signal a stack-anchor conflict with the FR-5 ordering guarantee.
- Adding `keyring` fails the `cargo deny check` license firewall on macOS (an unexpected transitive dep outside the allow-list) that cannot be resolved by a `[[licenses.clarify]]` for a permissive license.

**Never:**
- No live `SyncService`/`RoomListService`, room-list stream, or timeline — this story ends at a logged-in, persisted account; sync attaches in Story 1.4. No `AccountManager`/multi-account machinery, no `AuthProvider` trait extraction (Epic 2); a single account slice is sufficient.
- No at-rest DB encryption / passphrase choice (Epic 2 first-run story) — `sqlite_store(dir, None)`.
- No `matrix-js-sdk` or any Matrix JS lib; no crypto/token/message logic in TS.
- No token, refresh token, or `MatrixSession` field on any VM or IPC response; no token column in `keeper.db`.
- No manual light/dark toggle or Settings UI; reuse the Story 1.2 brand tokens.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Happy path | reachable SSS-capable Synapse ≥1.114, valid homeserver+user+password | logged-in `Client`, store at `accounts/<ulid>/sdk/`, session in Keychain, one `keeper.db` row; `login_password` resolves to `AccountVm`; store set → shell renders | none |
| Bare domain | `example.org` (no scheme), well-known present | homeserver resolved via `/.well-known/matrix/client`; login proceeds against resolved URL | discovery miss → treated as URL, then unreachable path if invalid |
| Bad credentials | valid SSS server, wrong password | login rejected; **no** persistent state left (store dir rolled back) | `AuthError::InvalidCredentials` → `invalidCredentials`, inline "Wrong username or password", `retriable:false` |
| Unreachable server | bad domain / DNS fail / connection refused | fails at build/probe before any state | `AuthError::ServerUnreachable` → `serverUnreachable`, inline "Couldn't reach that homeserver", `retriable:true` |
| Unsupported login type | server without `m.login.password` | login refused | `AuthError::UnsupportedLoginType` → `unsupportedLoginType`, inline naming password login not supported, `retriable:false` |
| Non-SSS server | reachable server, no MSC4186 | refused **before** ULID/dir/Keychain/db creation; probe result logged | `AuthError::SlidingSyncUnsupported` → `slidingSyncUnsupported`, inline names Simplified Sliding Sync + doc link |
| Mid-flow failure | login OK but Keychain or db write fails | store dir + any Keychain entry rolled back, no db row | mapped `IpcError`; no half-account |

</intent-contract>

## Code Map

- `src-tauri/Cargo.toml` -- add to `[workspace.dependencies]`: `keyring = "3"`, `ulid = "1"`, `rusqlite = { version = "0.37", features = ["bundled"] }` (rusqlite 0.37 + ulid are already in the lock via matrix-sdk — no new license surface; `keyring` is the only genuinely new crate).
- `src-tauri/crates/keeper-core/Cargo.toml` -- depend on `ulid`, `rusqlite` (workspace).
- `src-tauri/crates/keeper/Cargo.toml` -- depend on `keyring` (workspace).
- `src-tauri/crates/keeper-core/src/vm.rs` -- add `AccountVm { account_id, user_id, homeserver_url }` (serde+TS, `#[ts(export)]`, camelCase); extend `IpcErrorCode` with `SlidingSyncUnsupported`, `InvalidCredentials`, `ServerUnreachable`, `UnsupportedLoginType`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `AuthError` enum (`ServerUnreachable(String)`, `InvalidCredentials`, `UnsupportedLoginType(String)`, `SlidingSyncUnsupported`); add `CoreError::Auth(#[from] AuthError)`; add `PlatformError::Keychain(String)`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- NEW: `keeper.db` access via rusqlite — open at `<data_dir>/keeper.db`, `PRAGMA journal_mode=WAL`, `CREATE TABLE IF NOT EXISTS accounts(account_id TEXT PRIMARY KEY, user_id TEXT NOT NULL, homeserver_url TEXT NOT NULL, device_id TEXT NOT NULL, created_ts INTEGER NOT NULL)`, `insert_account(..)`, `delete_account(account_id)` (rollback helper).
- `src-tauri/crates/keeper-core/src/auth.rs` -- NEW: `pub async fn login_password(platform: &dyn Platform, homeserver: &str, username: &str, password: &str) -> Result<AccountVm, CoreError>` — the full ordered flow (probe → create → login → persist → rollback-on-failure). Also `session_keychain_key(account_id) -> String`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- `pub mod auth; pub mod registry;`.
- `src-tauri/crates/keeper/src/ipc.rs` -- implement `DesktopPlatform::keychain_{set,get,delete}` via `keyring::Entry::new("dev.tgorka.keeper", key)`; add `#[tauri::command] pub async fn login_password(state, homeserver, username, password) -> Result<AccountVm, IpcError>`; extend `to_ipc_error` for `CoreError::Auth(..)` and `PlatformError::Keychain`.
- `src-tauri/crates/keeper/src/lib.rs` -- register `ipc::login_password` in `generate_handler!`.
- `src/lib/ipc/gen/` -- regenerated: NEW `AccountVm.ts`, updated `IpcErrorCode.ts`.
- `src/lib/ipc/client.ts` -- add `loginPassword(homeserver, username, password): Promise<AccountVm>` wrapper; re-export `AccountVm`.
- `src/lib/stores/accounts.ts` -- NEW zustand **vanilla** store (`createStore` from `zustand/vanilla` + a `useAccountsStore` selector hook via `useStore`): `{ currentAccount: AccountVm | null, setCurrentAccount, clear }`, created outside React (AD-9).
- `src/components/auth/login-screen.tsx` -- NEW full-screen centered `Card` form (homeserver/username/password `Input`+`Label`, submit `Button`, inline `Alert` mapping `IpcErrorCode`→friendly copy incl. the SSS doc link); clears password on submit; on success calls `setCurrentAccount`.
- `src/App.tsx` -- gate: render `<LoginScreen/>` when `currentAccount` is null, else `<AppShell/>`.
- `package.json` -- add `zustand` (`bun add zustand`).
- Tests: `keeper-core` unit tests (`error.rs` mapping, `registry.rs` WAL+insert+delete round-trip in a temp dir, `vm.rs` `AccountVm` serde round-trip); `keeper/src/ipc.rs` unit test asserting each `AuthError`/`PlatformError::Keychain` maps to the right `IpcErrorCode`+`retriable`; frontend `src/components/auth/login-screen.test.tsx`, `src/lib/stores/accounts.test.ts`, updated `src/App.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/Cargo.toml`, `crates/keeper-core/Cargo.toml`, `crates/keeper/Cargo.toml` -- add the `keyring`/`ulid`/`rusqlite` workspace deps and wire them into the two crates (ulid+rusqlite → core, keyring → shell).
- [x] `keeper-core/src/error.rs` -- add `AuthError`, `CoreError::Auth`, `PlatformError::Keychain`; keep messages secret-free.
- [x] `keeper-core/src/vm.rs` -- add `AccountVm`; extend `IpcErrorCode` with the four new variants; add a serde round-trip test for `AccountVm`.
- [x] `keeper-core/src/registry.rs` -- implement the WAL `keeper.db` accounts registry (open/ensure-schema/insert/delete) with a temp-dir unit test covering insert→read-back and delete.
- [x] `keeper-core/src/auth.rs` -- implement `login_password` with the strict ordering (store-less SSS probe first; ULID + persistent `sqlite_store` + `login_username(..).initial_device_display_name("keeper").send()`; extract `matrix_auth().session()`, serialize, `platform.keychain_set`; `registry::insert_account`); on any post-create failure, roll back store dir + Keychain + db; map matrix-sdk errors to `AuthError` variants; `tracing` the probe/login outcome by `account_id` with no secrets.
- [x] `keeper-core/src/lib.rs` -- expose the new modules.
- [x] `keeper/src/ipc.rs` -- implement the three `DesktopPlatform` keychain methods via `keyring` (map errors to `PlatformError::Keychain`); add the async `login_password` command delegating to `keeper-core`; extend `to_ipc_error` for the new error variants with correct codes+`retriable`; add the mapping unit test.
- [x] `keeper/src/lib.rs` -- register `login_password` in `generate_handler!`.
- [x] regenerate ts-rs bindings (`bun run test:rust` / cargo build) and commit `src/lib/ipc/gen/AccountVm.ts` + updated `IpcErrorCode.ts`.
- [x] `package.json` -- `bun add zustand`.
- [x] `src/lib/stores/accounts.ts` -- vanilla zustand accounts store + `accounts.test.ts`.
- [x] `src/lib/ipc/client.ts` -- `loginPassword` typed wrapper + `AccountVm` re-export.
- [x] `src/components/auth/login-screen.tsx` -- login form UI + inline error mapping (incl. SSS doc link) + password clear-on-submit; `login-screen.test.tsx` covering render, invoke-on-submit, each error code → its inline message, and success → `setCurrentAccount`.
- [x] `src/App.tsx` + `src/App.test.tsx` -- auth gate (login screen when unauthenticated, shell when a `currentAccount` is set).

**Acceptance Criteria:**
- Given a reachable SSS-capable homeserver with password login, when valid credentials are submitted, then a logged-in `matrix_sdk::Client` exists with its store at `accounts/<ulid>/sdk/`, the session is stored only in the macOS Keychain (service `dev.tgorka.keeper`), a single `accounts` row is written to `keeper.db` (WAL), and the frontend transitions from the login screen to the shell (FR-1).
- Given a non-SSS homeserver, when login is attempted, then it fails with an error naming Simplified Sliding Sync (and a doc link in the UI) **before** any ULID, store directory, Keychain entry, or `keeper.db` row is created, and the probe result is logged via `tracing` with no credentials (FR-5).
- Given invalid input or an incapable/unreachable server, when login fails, then the inline error names the specific cause — bad credentials vs. unreachable vs. unsupported login type — via a distinct `IpcErrorCode`, and any partially-created state is rolled back so no half-configured account remains.
- Given code review, then no token, password, or `MatrixSession` material appears on any VM/IPC response, in `keeper.db`, or in any TS-reachable storage (NFR-9).
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 1, medium 0, low 3)
- defer: 1
- reject: 21
- addressed_findings:
  - `[high]` `[patch]` SSS probe misclassified an unreachable/flaky server as permanently non-SSS. `available_sliding_sync_versions()` swallows a transport error into an empty `Vec` (verified in the vendored source: "No error will be reported"), so a dropped `/versions` request — or a URL-form dead homeserver that slipped past lazy `build()` — returned `slidingSyncUnsupported` (permanent, non-retriable) instead of the `serverUnreachable` the I/O matrix requires. Replaced the probe with a direct `supported_versions()` call: transport `Err` → `ServerUnreachable` (retriable); reachable-but-no-`FeatureFlag::Msc4186` → `SlidingSyncUnsupported`. Aligned the Design Notes probe hint to match.
  - `[low]` `[patch]` `map_login_error` mapped every non-auth server errcode (rate-limit, deactivated account) to retriable `ServerUnreachable` ("check your connection"), inviting a harmful retry. Changed the catch-all to a non-retriable `CoreError::Internal`; the function now returns `CoreError` directly.
  - `[low]` `[patch]` The login form is `noValidate`, so blank/whitespace-only homeserver/username/password reached the backend as an opaque error. Added a client-side trim + missing-fields guard (own inline message) before the IPC call, and trims the values actually sent. Two new component tests cover it.
  - `[low]` `[patch]` `platform.rs` keychain trait docs still said "Not wired in Story 1.1 — returns `CoreError::Unsupported`"; corrected to note they are wired in Story 1.3 via the shell's `DesktopPlatform`.
  - `[medium]` `[coverage]` Added Rust unit tests for the `rollback` path (fake recording `Platform`): asserts the store dir is removed and exactly the account's keychain entry is deleted, and that rollback of a never-created dir is silent. (Live login and `map_login_error` over a real `matrix_sdk::Error` remain exercisable only against a real homeserver — the epic exit gate.)

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1: (high 0, medium 1, low 0)
- reject: 22: (high 0, medium 2, low 20)
- addressed_findings:
  - none
- notes: Independent follow-up pass (Blind Hunter + Edge Case Hunter) on the already-`done` story. 23 unique findings after dedup; no intolerable, unacknowledged defect. **1 deferred** (new ledger entry): the `unsupportedLoginType` classification is unreliable because a password-login-disabled homeserver returns `M_FORBIDDEN` (same errcode as bad credentials), so it surfaces as `invalidCredentials`; a robust fix needs a pre-login `login_types()` flow check the spec did not mandate — low user impact, deferred rather than re-derived. **22 rejected**, notably: (a) Blind Hunter's proposal to wire `registry::delete_account` into `rollback` is **harmful** — `registry::open()` runs `CREATE TABLE`/`create_dir_all` on every call, so it would create `keeper.db` on a failed login and violate the "leave zero persistent state" guarantee; the current omission is correct. (b) Same-identity re-login creating a second ULID account, live-login path untested, matrix-sdk store caching the token, and the SSS doc-link open behavior are all **already documented** as intended/deferred in Residual notes / the ledger. (c) Password-cleared-on-submit and remove-only-`accounts/<ulid>/sdk/` on rollback are **spec-mandated** by `<intent-contract>`. (d) `now_ms` fallback, per-call schema DDL, missing zeroize, no request timeout, direct-IPC empty-input guard, and rate-limit→non-retriable (a conscious pass-1 decision) are low-consequence hardening beyond this story's scope. No code changed this pass.

## Design Notes

**Grounded matrix-sdk 0.18 API (verified against the vendored source):**
```rust
// Phase A — store-less SSS probe (NOTHING persisted; default in-memory store):
let probe = Client::builder()
    .server_name_or_homeserver_url(homeserver)   // does /.well-known discovery, URL fallback
    .build().await                               // build/connect errors → AuthError::ServerUnreachable
    .map_err(|e| AuthError::ServerUnreachable(e.to_string()))?;
// NOTE: use `supported_versions()`, NOT `available_sliding_sync_versions()` —
// the latter swallows transport errors into an empty Vec ("No error will be
// reported"), which would mislabel an unreachable server as non-SSS.
let supported = probe.supported_versions().await          // HttpResult<SupportedVersions>
    .map_err(|e| AuthError::ServerUnreachable(e.to_string()))?;   // transport fail → retriable
if !supported.features.contains(&FeatureFlag::Msc4186) {  // reachable but no MSC4186
    return Err(AuthError::SlidingSyncUnsupported.into());  // no state created yet
}
let resolved = probe.homeserver();               // reuse discovered URL; drop `probe`

// Phase B — persistent account:
let account_id = Ulid::new().to_string();
let sdk_dir = platform.data_dir()?.join("accounts").join(&account_id).join("sdk");
let client = Client::builder()
    .homeserver_url(resolved)                    // already discovered → no second round-trip
    .sqlite_store(&sdk_dir, None)                // None passphrase (at-rest encryption is Epic 2)
    .build().await?;
let resp = client.matrix_auth()
    .login_username(username, password)
    .initial_device_display_name("keeper")
    .send().await;                               // Err → map + rollback (remove sdk_dir)
let session = client.matrix_auth().session().ok_or_else(|| CoreError::Internal(..))?;
platform.keychain_set(&session_keychain_key(&account_id), &serde_json::to_string(&session)?)?;
registry::insert_account(&platform.data_dir()?, &account_id, &session.meta.user_id, resolved, &session.meta.device_id, now_ms)?;
```
`FeatureFlag`/`SupportedVersions` live under `matrix_sdk::ruma::api`; `supported_versions()` returns `HttpResult<SupportedVersions>` whose `.features` set contains `FeatureFlag::Msc4186` iff the homeserver advertises Simplified Sliding Sync. The behavioral contract is fixed: a transport failure during the probe is `serverUnreachable` (retriable), a reachable server lacking MSC4186 is `slidingSyncUnsupported`. `MatrixSession` (`{ meta: SessionMeta{user_id,device_id}, tokens: SessionTokens{access_token, refresh_token: Option} }`) derives `Serialize`/`Deserialize`, so it round-trips through the Keychain string.

**Error mapping.** Map matrix-sdk login errors by inspecting the client-API error kind: an authentication rejection (`M_FORBIDDEN`/`M_UNAUTHORIZED`) → `InvalidCredentials`; a password-login-not-offered / unknown login type → `UnsupportedLoginType`; a transport/connection/DNS failure at build or send → `ServerUnreachable`. Confirm the exact `ErrorKind` accessor on matrix-sdk `Error` at implementation time; keep the mapping in `keeper-core` so the shell only funnels `CoreError`→`IpcError`.

**Rollback.** Treat "persistent state created" as starting when `sdk_dir` is first built. Wrap Phase B so any `Err` after that point runs a best-effort cleanup: `std::fs::remove_dir_all(&sdk_dir)`, `platform.keychain_delete(&key)` (ignore not-found), and never call `insert_account`. This is what makes "never a half-configured account" true.

**Frontend error copy** is keyed off `IpcErrorCode` in `login-screen.tsx` (sentence case, no error codes shown, per UX microcopy): `slidingSyncUnsupported` → "This homeserver doesn't support Simplified Sliding Sync, which keeper requires." + a doc link; `invalidCredentials` → "Wrong username or password."; `serverUnreachable` → "Couldn't reach that homeserver. Check the address and your connection."; `unsupportedLoginType` → "This homeserver doesn't support password login." Use `bg-destructive` `Alert` styling.

**Vanilla zustand (AD-9).** `export const accountsStore = createStore<AccountsState>()(...)` created at module load outside React; components read via `useStore(accountsStore, selector)`. The store holds only the non-secret `AccountVm` + ephemeral flags — never tokens.

**Residual (documented, not a gap):** matrix-sdk's own unencrypted sqlite store may internally cache the access token inside `accounts/<ulid>/sdk/`. keeper treats that dir as opaque SDK-owned state and keeps its *own* token handling Keychain-only (no token in `keeper.db`, none to TS); encrypting that store at rest is the Epic 2 first-run-encryption story. The live `Client` is dropped after persisting — Story 1.4 restores it via `restore_session` and attaches `SyncService`.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest (new login-screen/accounts tests, updated App test) green.
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean (new auth/registry/keychain code, no `.unwrap()`).
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs bindings regenerated to match committed `src/lib/ipc/gen/`.
- `cd src-tauri && cargo deny check` -- expected: license firewall passes with `keyring` added (ulid/rusqlite already present).

**Manual checks (require a real Synapse ≥1.114 — the automated tests can't exercise live login):**
- `op run --env-file=.env.1p -- bun run tauri dev`: submit valid creds → shell appears; confirm `accounts/<ulid>/sdk/` + `keeper.db` exist under the data dir and the Keychain has one `dev.tgorka.keeper` entry, with no token in `keeper.db` (inspect with `sqlite3`).
- Submit against a non-SSS server → SSS-named error and **no** new `accounts/<ulid>` dir / Keychain entry / db row. Submit wrong password / bad domain / password-disabled server → the respective named inline error.

## Auto Run Result

Status: **done**

### Summary
Implemented password login with Simplified Sliding Sync verification across both layers. `keeper-core` gained an `auth` module whose `login_password` runs a strict ordered flow: a **store-less** SSS probe first (via `supported_versions()` — nothing persisted), and only on MSC4186 support does it generate a ULID, open a persistent SQLite store at `accounts/<ulid>/sdk/`, log in via `matrix_auth().login_username`, write the serialized `MatrixSession` **only** to the macOS Keychain (`keyring`, service `dev.tgorka.keeper`), and insert one non-secret row into a WAL `keeper.db`. Any post-store-creation failure rolls back (remove dir + delete keychain entry + no db row). Errors resolve to a named taxonomy (`invalidCredentials` / `serverUnreachable` / `unsupportedLoginType` / `slidingSyncUnsupported`) mapped once in the shell's `to_ipc_error`. The `keeper` shell wires the keychain ports via `keyring` and exposes the async `login_password` command. The frontend adds a vanilla-zustand accounts store, a full-screen `LoginScreen` (shadcn `Card`/`Input`/`Label`/`Button`/`Alert`, controlled inputs, client-side blank-input guard, password cleared on submit, inline named errors incl. an SSS doc link), and an auth gate in `App.tsx`. `AccountVm` carries only `{ accountId, userId, homeserverUrl }`; no token/password/`MatrixSession` crosses IPC or lands in TS storage.

### Files changed
- `src-tauri/Cargo.toml` + `crates/{keeper-core,keeper}/Cargo.toml` — added `keyring` (apple-native) to the shell, `ulid`+`rusqlite` (bundled) to core (both already in the lock).
- `crates/keeper-core/src/error.rs` — `AuthError`, `CoreError::Auth`, `PlatformError::Keychain`.
- `crates/keeper-core/src/vm.rs` — `AccountVm`; four new `IpcErrorCode` variants; serde tests.
- `crates/keeper-core/src/registry.rs` (NEW) — WAL `keeper.db` accounts registry (no token column) + tests.
- `crates/keeper-core/src/auth.rs` (NEW) — ordered login flow, `supported_versions()` SSS probe, error mapping, rollback + tests.
- `crates/keeper-core/src/{lib,platform}.rs` — module exposure; corrected keychain trait docs.
- `crates/keeper/src/ipc.rs` — keychain ports via `keyring`; async `login_password` command; `to_ipc_error` mapping + tests.
- `crates/keeper/src/lib.rs` — command registration.
- `src/lib/ipc/gen/{AccountVm.ts(NEW),IpcErrorCode.ts}` — regenerated bindings.
- `src/lib/ipc/client.ts` — `loginPassword` wrapper + `AccountVm` re-export.
- `src/lib/stores/accounts.ts` (NEW) + test — vanilla zustand store.
- `src/components/auth/login-screen.tsx` (NEW) + test — login UI.
- `src/App.tsx` + `src/App.test.tsx` — auth gate.
- `package.json` / `bun.lock` — added `zustand`.

### Review findings
- Two reviewers (adversarial-general Blind Hunter + edge-case-hunter). Triage: 0 intent_gap, 0 bad_spec, 4 patch (1 high, 3 low), 1 defer, 21 reject. See Review Triage Log.
- **Patches:** (high) replaced the SSS probe — `available_sliding_sync_versions()` silently swallowed transport errors and misclassified an unreachable server as permanent `slidingSyncUnsupported`; now `supported_versions()` distinguishes unreachable (retriable) from genuine non-SSS; (low) `map_login_error` catch-all → non-retriable `Internal` instead of misleading retriable "unreachable"; (low) client-side blank/whitespace input guard on the `noValidate` form; (low) corrected stale keychain trait docs; plus new `rollback` unit tests.
- **Deferred:** the non-SSS "Learn more" link's open-in-browser behavior in the Tauri webview is unverified (see `deferred-work.md`).

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **27 passed (6 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean.
- `bun run test:rust` ✅ — cargo-nextest **30 passed, 0 skipped**; ts-rs bindings regenerate idempotently (only `AccountVm.ts` new + `IpcErrorCode.ts` changed).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). `keyring` introduces **no** new advisory; the `advisories` subsection remains red only on the pre-existing gtk-rs/tauri transitive residual (RUSTSEC-2024-0413 et al.) documented in stories 1.1/1.2 — the license firewall passes.
- Not run: live login against a real Synapse ≥1.114 (blocking, needs a real SSS homeserver) — the happy path, well-known discovery, the four error classifications, and on-disk store/Keychain/db creation + rollback are the epic exit gate. See Manual checks.

### Residual risks
- The whole live-login path (matrix-sdk `build`/`login`/`session`, well-known discovery, real error-kind classification, on-disk persistence + rollback) is reasoned-about and unit-tested only at its pure seams; it is not exercised without a real homeserver.
- matrix-sdk's own unencrypted SDK store under `accounts/<ulid>/sdk/` may internally cache the access token; keeper keeps its own token handling Keychain-only (no token in `keeper.db`, none to TS). At-rest encryption of that store is the Epic 2 first-run story.
- Persisted accounts are not restored on next launch (the zustand store starts empty) — session restore + sync attach in Stories 1.8 / 1.4; until then a relaunch returns to the login screen. Same-identity re-login creating a second ULID account is expected for the single-account slice; dedup/merge is Epic 2.
- The non-SSS doc link's external-open behavior is deferred (see above).
- Pre-existing `cargo deny` advisories (gtk/unic via Tauri) remain out of scope.

### Follow-up review pass (2026-07-04)
An independent second review pass (Blind Hunter + Edge Case Hunter) ran against the full baseline→HEAD diff. After dedup, 23 unique findings: **0 intent_gap, 0 bad_spec, 0 patch, 1 defer, 22 reject**. No intolerable, unacknowledged defect was found, so **no code changed** and the prior quality gates (pass-1 `bun run check` / `check:rust` / `test:rust` / `cargo deny check` all green) remain valid.
- **Deferred (new ledger entry):** `unsupportedLoginType` is unreliable — a password-login-disabled homeserver returns `M_FORBIDDEN`, which maps to `invalidCredentials`; a robust fix needs a pre-login `login_types()` flow check the spec did not mandate. Low user impact.
- **Rejected highlight:** the reviewer proposal to call `registry::delete_account` in `rollback` was rejected as *harmful* — `registry::open()` runs `CREATE TABLE`/`create_dir_all` on every call, so it would create `keeper.db` on a failed login, breaking the "zero persistent state on failure" guarantee; the current omission is correct.
- Most other findings (re-login duplicate account, untested live-login path, SDK store token caching, SSS doc-link open behavior) are already documented as intended/deferred; password-clear-on-submit and remove-only-`accounts/<ulid>/sdk/` are spec-mandated by `<intent-contract>`.
- `followup_review_recommended` set to **false**: this pass made no review-driven changes.
