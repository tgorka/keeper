---
title: 'OIDC Login (MAS / MSC3861)'
type: 'feature'
created: '2026-07-04'
status: 'done'
baseline_revision: '457709057d2f1a3742f4f980d198c00a8577043e'
final_revision: '798045767afe296ba95473a5cf4ab11d1302d539'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper can only sign in with `m.login.password`. Modern Matrix homeservers behind a Matrix Authentication Service (MAS / MSC3861) delegate auth to OAuth 2.0 and offer no password flow, so keeper cannot add those accounts at all. Story 2.1 extracted an `AuthProvider` trait (password as the first impl) precisely so this story can add an `oidc` impl.

**Approach:** Add an `OidcAuthProvider` impl of the existing `AuthProvider` trait that drives matrix-sdk 0.18's `client.oauth()` MSC3861 flow — dynamic client registration → authorization URL → open the **system browser** → receive the `keeper://oauth/callback?code&state` custom-scheme deep link → `finish_login`. Reuse the shared `add_account` orchestration (store-less SSS gate → persistent store → `authenticate` → Keychain session → registry row + hue → rollback). Generalize session persistence/restore from a bare `MatrixSession` to a tagged `StoredSession` (password vs OAuth) so OAuth sessions round-trip through the Keychain, keeping legacy password sessions restorable. Add the `keeper://` scheme + deep-link plumbing, a browser-open platform port, an in-flight OAuth-callback registry, and a minimal OIDC affordance on the existing login screen with a "complete sign-in in your browser" pending state, Cancel, and named failure states.

## Boundaries & Constraints

**Always:**
- Reuse the Story 2.1 shared `add_account` orchestration and its rollback; the OIDC path differs only in the mechanism-specific `authenticate` step. Adding an OIDC account must behave identically to adding the Nth of any kind — no account-count assumptions.
- The store-less SSS (MSC4186) capability gate runs **before** any store dir / Keychain entry, identically to password login; a non-SSS server fails with the existing `SlidingSyncUnsupported` error naming SSS, before any OAuth work.
- Cancelling (explicit Cancel) or abandoning (closing the browser, timeout) an OIDC flow leaves **zero residue**: no partial Account, store dir, Keychain entry, or dangling registry callback. Rollback + registry cleanup are guaranteed on every non-success exit.
- Access/refresh tokens live **only** in the macOS Keychain (`dev.tgorka.keeper`), serialized as a tagged `StoredSession`; they never touch disk unencrypted and never cross IPC into JavaScript. The authorization `code`/`state` are transient and never persisted or logged.
- Persistent Clients (both at add time and on restore/activation) are built with `handle_refresh_tokens()`; refreshed OAuth tokens are re-persisted to the Keychain so restart-after-refresh restores cleanly (MAS refresh tokens are one-time-use).
- The `keeper://oauth/callback` redirect is registered as `ApplicationType::Native`; the callback is matched to its in-flight flow by the OAuth `state` parameter. Spurious/late/unmatched callbacks are ignored, never crash.
- `keeper-core` stays **tauri-free** (the crate-boundary test must keep passing): the browser-open is a new `Platform` port; the OAuth-callback registry is plain tokio/std in core; deep-link wiring lives only in the `keeper` (Tauri) crate.
- Rust: `unsafe_code` denied, no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`; `#[non_exhaustive]` matrix-sdk enums matched with a catch-all. TS `strict`, no `any`, `import type`. New deps pass cargo-deny.

**Block If:**
- matrix-sdk 0.18's OAuth API in the local toolchain does not actually expose the discovered surface (`client.oauth()`, `server_metadata()`, `ClientMetadata`/`ClientRegistrationData`, `login(...).build()`, `finish_login`, `full_session`/`restore_session`) — i.e. a compile-time API mismatch that cannot be resolved without changing the login design. HALT rather than inventing an API.
- Generalizing Keychain session persistence to `StoredSession` would require a change that cannot deserialize already-stored bare `MatrixSession` blobs (i.e. would silently drop existing signed-in password accounts). A backward-compatible read is required; if impossible, HALT.

**Never:**
- Do **not** add a `features = ["oauth"]` / `["experimental-oidc"]` to matrix-sdk — those features do not exist in 0.18 and will fail the build; the OAuth module is always compiled.
- Do not use the legacy `matrix_auth().login_sso()` / `sso-login` redirect flow for MSC3861 — that is a different mechanism; MSC3861 uses `client.oauth()`.
- Do not run the OAuth token exchange, homeserver HTTP, or any Matrix/crypto logic in TypeScript; the frontend only opens/awaits via IPC and renders view models.
- Do not build the designed account switcher / per-account sync glyph (Story 2.5) or auto-detect-and-route password-vs-OIDC beyond a minimal explicit OIDC affordance; keep the login UI a minimal, throwaway extension consistent with 2.1.
- Do not embed a confidential client secret or hardcode a client_id — the flow is a public native client via dynamic registration (matrix-sdk forces `token_endpoint_auth_method: none`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| OIDC happy path | User enters a MAS homeserver, chooses OIDC; completes consent in browser; `keeper://oauth/callback?code&state` returns | Callback matched by `state`; `finish_login` succeeds; session persisted as `StoredSession::Oauth`; registry row + hue; syncing `AccountVm` returned and merged into the inbox | No error |
| Non-OAuth homeserver | User chooses OIDC against a server without MSC3861 | `server_metadata()` reports not-supported → distinct `OAuthUnsupported` failure naming that OIDC isn't offered | Non-retriable; no store/Keychain residue |
| Non-SSS homeserver | Any homeserver missing MSC4186 | Existing `SlidingSyncUnsupported` error naming SSS, raised by the pre-store gate before any OAuth work | Non-retriable; zero residue |
| User cancels in app | Flow pending; user clicks Cancel | `cancel_oidc` aborts the pending flow; `authenticate` returns cancelled; `add_account` rolls back; UI returns to the form (no scary error) | Cancelled; zero residue |
| Browser abandoned / timeout | Callback never arrives within the timeout (~5 min) | `authenticate` times out → named `OAuthTimedOut` failure with Retry | Retriable; zero residue |
| Callback error param | Callback is `keeper://oauth/callback?error=access_denied&state=...` | Flow resolves as `OAuthFailed` naming the server error; Retry offered | Retriable; zero residue |
| Spurious/late callback | A `keeper://oauth/callback` whose `state` matches no in-flight flow | Ignored (logged at debug); no crash, no effect on any account | n/a |
| Restore mixed accounts | keeper.db lists a password account and an OIDC account with valid Keychain sessions | Both restore: password via `MatrixSession`, OIDC via `OAuthSession` (auto-refresh); both merge into the inbox | An account whose Keychain session is missing/undeserializable is skipped, not fatal |
| Legacy session blob | Keychain holds a pre-2.2 bare `MatrixSession` JSON (no tag) | Reads as `StoredSession::Password`; restores normally | Fallback deserialize; never drops the account |
| Token refresh across restart | OIDC account's access token expired; refresh rotates the refresh token | Refreshed session re-persisted to Keychain; next restart restores from the rotated token | On refresh-token failure, account is skipped/needs re-login (not a crash) |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/auth.rs` -- `AuthProvider` trait + `add_account` orchestration + `PasswordAuthProvider` + `rollback`. Extend the trait's `authenticate` to also receive `&dyn Platform` (for the browser-open side effect); add `OidcAuthProvider`; add a `StoredSession` tagged enum + `persist`/`restore` helpers replacing the bare `MatrixSession` serialize at `auth.rs:210-221`; add `login_oidc` core entry (`OidcAuthProvider` → `add_account`).
- `src-tauri/crates/keeper-core/src/oauth.rs` -- NEW: `OAuthFlowRegistry` (register a pending flow by `state` → `oneshot::Receiver<OAuthCallback>`; `resolve(url)` parses `state` and forwards the callback URL; `cancel_all`); `OAuthCallback` enum; client-registration metadata builder (`ApplicationType::Native`, redirect `keeper://oauth/callback`, `client_uri`, `client_name`). Tauri-free (tokio + url).
- `src-tauri/crates/keeper-core/src/platform.rs` -- add `fn open_url(&self, url: &str) -> Result<(), CoreError>` to the `Platform` trait (`platform.rs:18`); update any in-core mock `Platform` impls to record/no-op it.
- `src-tauri/crates/keeper-core/src/account.rs` -- account activation/restore path (`account.rs:798-819`): restore via `StoredSession` (password vs OAuth via `oauth().restore_session`), build the persistent Client with `handle_refresh_tokens()`, and re-persist on session-token change (subscribe to session changes) so rotated OAuth tokens survive restart. Update the in-core mock `Platform` impls used in tests.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `AuthError` variants `OAuthUnsupported`, `OAuthTimedOut`, `OAuthCancelled`, `OAuthFailed(String)` (`error.rs:31-56`).
- `src-tauri/crates/keeper-core/Cargo.toml` -- ensure matrix-sdk OAuth types are reachable with **no** new feature flag; no `oauth`/`experimental-oidc` feature.
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState` gains `oauth_flows: Arc<OAuthFlowRegistry>` (`ipc.rs:34-42`); add `login_oidc(homeserver) -> Result<AccountVm, IpcError>` and `cancel_oidc() -> Result<(), IpcError>` commands; map the new `AuthError` variants in `to_ipc_error`/`IpcErrorCode` (`ipc.rs:119-166`) with retriable flags (unsupported=false, timed-out/failed/cancelled=true).
- `src-tauri/crates/keeper/src/lib.rs` -- register `tauri_plugin_deep_link::init()`; in `setup`, wire `deep_link().on_open_url` to `state.oauth_flows.resolve(url)`; add `login_oidc`/`cancel_oidc` to `generate_handler!`; implement `Platform::open_url` for `DesktopPlatform` (system browser).
- `src-tauri/crates/keeper/Cargo.toml` + `src-tauri/Cargo.toml` -- add `tauri-plugin-deep-link` (workspace + keeper) and a system-browser opener for the Rust `open_url` impl (the `opener` crate, MIT — or route through the already-present `tauri-plugin-opener`); both must pass cargo-deny.
- `src-tauri/tauri.conf.json` -- register the `keeper` custom scheme for deep-link (plugin `deep-link` config) so `keeper://oauth/callback` reaches the app.
- `src-tauri/crates/keeper/capabilities/default.json` -- add the `deep-link` plugin permission alongside `opener:default`.
- `package.json` -- add `@tauri-apps/plugin-deep-link` (guest bindings).
- `src/lib/ipc/client.ts` -- add `loginOidc(homeserver): Promise<AccountVm>` and `cancelOidc(): Promise<void>` wrappers.
- `src/components/auth/login-screen.tsx` -- add a minimal OIDC affordance (e.g. a "Sign in with single sign-on" button using the entered homeserver) that calls `loginOidc`; render a "Complete sign-in in your browser…" pending state with a Cancel button (`cancelOidc`); on success `addAccount(account)` + `onDone`; on failure show the named error (unsupported / timed-out / failed / SSS) with Retry; cancellation returns to the form quietly.
- `src/lib/ipc/gen/*` -- regenerate ts-rs bindings (new `IpcErrorCode` variants; `AccountVm` unchanged).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/error.rs` -- Add `OAuthUnsupported`, `OAuthTimedOut`, `OAuthCancelled`, `OAuthFailed(String)` to `AuthError` with clear messages.
- [x] `keeper-core/src/platform.rs` -- Add `open_url(&self, url: &str) -> Result<(), CoreError>` to `Platform`; implement in in-core mock/test platforms (record the URL).
- [x] `keeper-core/src/oauth.rs` -- NEW: `OAuthFlowRegistry` (`register(state) -> Receiver<OAuthCallback>`, `resolve(url) -> bool` matching by `state`, `cancel_all`), `OAuthCallback`, and the `ClientRegistrationData` builder (native app, `keeper://oauth/callback`).
- [x] `keeper-core/src/auth.rs` -- Extend `AuthProvider::authenticate` to take `&dyn Platform`; add `StoredSession` tagged enum (`Password(MatrixSession)` / `Oauth { client_id, user: UserSession }`) with persist (detect via `client.oauth().full_session()` else `client.session()`) and a **legacy-tolerant** restore read; wire persistence into `add_account`; add `OidcAuthProvider` (metadata check via `server_metadata().is_not_supported()` → `OAuthUnsupported`; `oauth.login(...).build()`; register `state`; `platform.open_url(url)`; `select!` on callback / ~5-min timeout; `finish_login` on success); add `login_oidc(platform, homeserver, flows)`.
- [x] `keeper-core/src/account.rs` -- Restore both `StoredSession` kinds (OAuth via `oauth().restore_session`), build persistent Clients with `handle_refresh_tokens()`, and re-persist the Keychain blob when session tokens change; update in-core test platform impls.
- [x] `keeper/src/ipc.rs` -- Add `oauth_flows: Arc<OAuthFlowRegistry>` to `AppState`; add `login_oidc`/`cancel_oidc` commands; map the new errors in `to_ipc_error`/`IpcErrorCode` with correct retriable flags.
- [x] `keeper/src/lib.rs` + `keeper/Cargo.toml` + `src-tauri/Cargo.toml` -- Add & register `tauri-plugin-deep-link`; wire `on_open_url → oauth_flows.resolve`; implement `DesktopPlatform::open_url` (system browser); register `login_oidc`/`cancel_oidc`; add the opener dependency.
- [x] `src-tauri/tauri.conf.json` + `keeper/capabilities/default.json` + `package.json` -- Register the `keeper` scheme, grant the deep-link permission, add `@tauri-apps/plugin-deep-link`.
- [x] `src/lib/ipc/client.ts` -- Add `loginOidc`/`cancelOidc`; regenerate ts-rs bindings.
- [x] `src/components/auth/login-screen.tsx` -- Add the OIDC affordance, pending/Cancel state, named failure + Retry, and `addAccount` on success.
- [x] Tests -- Rust: `OAuthFlowRegistry` (resolve-by-state match / spurious no-match / cancel_all / receiver-drop cleanup), `StoredSession` serde round-trip for both kinds **and legacy bare-`MatrixSession` read**, `to_ipc_error` mapping for the four new variants, and the registration-metadata builder (native app_type + exact redirect URI). Frontend: login-screen OIDC button → `loginOidc`, pending state + Cancel → `cancelOidc`, success → `addAccount`+`onDone`, each named error renders with Retry; `client.ts` invokes the right commands.

**Acceptance Criteria:**
- Given a MAS/MSC3861 homeserver, when the user chooses OIDC and completes the browser consent, then keeper opens the system browser, receives the `keeper://oauth/callback` deep link, finishes the OAuth exchange, and the account is added and syncing in the merged inbox — password and OIDC both flowing through the same `AuthProvider`/`add_account` path.
- Given an in-progress OIDC flow, when it is cancelled, times out, errors, or targets a non-OAuth/non-SSS server, then a distinct named failure state is shown (never a hang/spinner/crash) and **no** partial Account, store dir, Keychain entry, or dangling callback remains.
- Given a Keychain holding both a password (`MatrixSession`) and an OIDC (`OAuthSession`) account — including a pre-2.2 legacy bare-`MatrixSession` blob — when the app restarts, then all restore correctly (OAuth auto-refreshing) and merge into the inbox, and no existing password account is dropped by the persistence change.
- Given `keeper-core`, when the workspace is built and the crate-boundary test runs, then core remains tauri-free (test passes): browser-open is a `Platform` port and the OAuth-callback registry is plain tokio/std; only the `keeper` crate touches the deep-link plugin.

## Design Notes

**Session shape is the central risk.** OAuth login produces an `OAuthSession` reachable via `client.oauth().full_session()`, *not* `client.matrix_auth().session()`. matrix-sdk's `AuthSession`/`OAuthSession` are **not** `Serialize`, but `MatrixSession` and `oauth::UserSession` are. So persist a keeper-owned tagged enum and reconstruct on restore:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
enum StoredSession {
    Password(MatrixSession),
    Oauth { client_id: String, user: oauth::UserSession },
}
// persist: if let Some(s) = client.oauth().full_session() { Oauth { client_id: s.client_id.into(), user: s.user } }
//          else if let Some(AuthSession::Matrix(m)) = client.session() { Password(m) }
// restore: Password(m) => client.restore_session(m).await?;                        // MatrixSession: Into<AuthSession>
//          Oauth{client_id,user} => client.oauth()
//              .restore_session(OAuthSession{ client_id: ClientId::new(client_id), user }, RoomLoadSettings::default()).await?;
// legacy read: try StoredSession first; on failure, parse a bare MatrixSession -> Password.
```

**OIDC `authenticate` runs the whole browser round-trip inside `add_account`** (matrix-sdk stashes PKCE/state in the in-memory `OAuth` handle, so `build()` and `finish_login()` must use the same live `Client`):

```rust
async fn authenticate(&self, client: &Client, platform: &dyn Platform) -> Result<(), CoreError> {
    let oauth = client.oauth();
    oauth.server_metadata().await.map_err(|e|
        if e.is_not_supported() { AuthError::OAuthUnsupported } else { AuthError::ServerUnreachable(e.to_string()) })?;
    let data = oauth.login(redirect_uri(), None, Some(registration_data()), None).build().await.map_err(map_oauth)?;
    let rx = self.flows.register(data.state.secret().clone());
    platform.open_url(data.url.as_str())?;
    match timeout(OAUTH_TIMEOUT, rx).await {              // never hangs
        Err(_) => Err(AuthError::OAuthTimedOut.into()),
        Ok(Err(_)) | Ok(Ok(OAuthCallback::Cancelled)) => Err(AuthError::OAuthCancelled.into()),
        Ok(Ok(OAuthCallback::Error(e))) => Err(AuthError::OAuthFailed(e).into()),
        Ok(Ok(OAuthCallback::Redirect(url))) => { oauth.finish_login(Url::parse(&url)?.into()).await.map_err(map_oauth)?; Ok(()) }
    }
}
```

The `keeper` crate's `on_open_url` handler calls `state.oauth_flows.resolve(url)`, which parses the `state` query param and forwards the full URL to the matching `oneshot` sender; unmatched callbacks return `false` and are ignored. `cancel_oidc` calls `cancel_all`. Registry entries are removed on resolve and on cancel; because `oneshot` gives the registry no receiver-drop signal, the OIDC provider also removes its own entry on every exit path (timeout, cancel, browser-open failure, error, success) via an RAII guard, so no dangling sender or leaked `state` secret accumulates. **macOS dev caveat:** custom-scheme deep links rely on the bundled app's `CFBundleURLTypes` (via the plugin config); they may not fire under `tauri dev` from an unbundled binary — verify against a built app. The Rust unit tests drive the registry with a synthetic callback and never touch a real browser/server; the end-to-end browser flow is a manual check.

**No matrix-sdk feature flag** — the `oauth` module is unconditionally compiled in 0.18; adding a feature will fail the build.

## Verification

**Commands:**
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` (no `.unwrap()`, no `unsafe`, `#[non_exhaustive]` catch-alls present).
- `bun run test:rust` -- expected: cargo-nextest green incl. new `OAuthFlowRegistry`, `StoredSession` (both kinds + legacy), error-mapping, and registration-metadata tests; the keeper-core tauri-free boundary test still passes.
- `bun run check` -- expected: biome + tsc strict + vitest green incl. new login-screen / client tests.
- ts-rs binding regeneration -- expected: `src/lib/ipc/gen/*` updated with new `IpcErrorCode` variants, no stale drift.
- `cargo deny check` (from `src-tauri/`) -- expected: pass with `tauri-plugin-deep-link` + the opener crate added (permissive licenses only).

**Manual checks (if no CLI):**
- Against a real MAS homeserver in a **built** app: choose OIDC, complete browser consent, confirm the account appears syncing in the merged inbox; quit and relaunch to confirm OIDC restore (token auto-refresh) alongside a password account; cancel a flow and confirm the form returns with no leftover store dir / Keychain entry.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 0
- defer: 1
- reject: 23
- addressed_findings:
  - none
- notes: Independent follow-up review (Blind Hunter + Edge Case Hunter) of the full baseline→HEAD diff. All 5 prior-pass patches re-verified against the code and confirmed correct. The two highest-signal re-raised findings were verified as non-issues: (a) the persister/sign-out "keychain resurrection race" — `shutdown` runs `session_persister.abort(); .await` *before* `sign_out_cleanup`, and a JoinHandle `.await` waits for full task termination (a task cannot be cancelled mid-synchronous `keychain_set`), so every persister write strictly precedes the delete — no resurrection; (b) `StoredSession::from_json` legacy detection is sound because a serialized `MatrixSession` carries no top-level `"kind"`. `cancel_all` / single-flight, `access_denied`→retriable `OAuthFailed`, and the SSS-before-OAuth gate are all explicitly by-design per the intent contract / Design Notes. One genuine, low-severity, pre-existing item deferred (below).

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 5: (high 0, medium 2, low 3)
- defer: 0
- reject: 19
- addressed_findings:
  - `[medium]` `[patch]` Registry residue leak: abandoned/timeout/`open_url`-failure/error OIDC flows never removed their `state`→sender entry (only a matched callback or `cancel_all` did), leaking memory + live `state` secrets and violating the "zero residue / no dangling callback" constraint; the module doc-comment falsely claimed receiver-drop cleanup that `oneshot` does not provide. Fixed: added `OAuthFlowRegistry::remove` + an RAII `FlowGuard` in `authenticate` that removes the entry on every exit path; corrected the doc-comments; added a `remove` residue test.
  - `[medium]` `[patch]` Sign-out keychain-resurrection race: the new session-persister task held its own `Client` clone, so it survived `AccountManager::shutdown`, kept the store's SQLite handles open past `sign_out_cleanup`'s store-dir deletion, and could re-write the just-deleted Keychain key on a late `TokensRefreshed` (resurrecting a signed-out secret). Fixed: store the persister `JoinHandle` on `AccountHandle` and abort+await it in `shutdown` before store teardown (and on partial-activation teardown) — mirrors the Story 2.1 inbox-producer fix.
  - `[low]` `[patch]` Persister `Lagged` branch could drop the final one-time-use OAuth refresh token (a broadcast-buffer overflow coalescing the last rotation before client teardown → next cold-start restore fails). Fixed: on `Lagged`, persist the current live session immediately instead of `continue` (extracted a `persist_current_session` helper).
  - `[low]` `[patch]` `StoredSession::from_json` fell back to a bare `MatrixSession` on ANY tagged-parse failure, masking real errors and risking mis-reading a future tagged variant as a password session. Fixed: only fall back for a genuinely untagged (no `"kind"`) blob; surface tagged-blob parse errors; added a test.
  - `[low]` `[patch]` Frontend: unmounting the login overlay mid-flow left an orphaned backend OIDC flow until timeout. Fixed: an unmount cleanup effect calls `cancelOidc` when a flow is pending.

## Auto Run Result

Status: **done**

**Summary.** Added OIDC (OAuth 2.0 / MSC3861, MAS) login as a second `AuthProvider` impl (`OidcAuthProvider`) behind the Story 2.1 shared `add_account` orchestration. The provider drives matrix-sdk 0.18's `client.oauth()` flow (dynamic native-client registration → authorization URL → open the system browser via a new `Platform::open_url` port → await the `keeper://oauth/callback?code&state` deep link, matched by OAuth `state` in a tauri-free `OAuthFlowRegistry` → `finish_login`) entirely inside one `authenticate` call, with a ~5-minute timeout and RAII registry cleanup so cancel/timeout/abandon leave zero residue. Session persistence was generalized from a bare `MatrixSession` to a tagged `StoredSession` (`Password` | `Oauth`) that round-trips OAuth sessions through the Keychain while staying backward-compatible with pre-2.2 untagged blobs; clients are built with `handle_refresh_tokens()` and a session-persister re-writes rotated (one-time-use) tokens. Deep-link plumbing (`tauri-plugin-deep-link`, `keeper://` scheme, capability, `on_open_url` routing) and a minimal login-screen OIDC affordance (pending / Cancel / named failures) complete the flow.

**Files changed (one-line each).**
- Backend core: `keeper-core/src/oauth.rs` (new `OAuthFlowRegistry` + `remove` + `OAuthCallback` + native registration metadata), `auth.rs` (`OidcAuthProvider`, `authenticate(+&dyn Platform)`, `FlowGuard`, `StoredSession` tagged enum + legacy-tolerant `from_json`, `login_oidc`), `account.rs` (restore via `StoredSession`, `handle_refresh_tokens()`, `session_persister` handle stored + aborted on shutdown, `Lagged` persists current session), `platform.rs` (`open_url` port), `error.rs` (4 new `AuthError` variants), `vm.rs`/`lib.rs`/`Cargo.toml` (`IpcErrorCode` variants, `oauth` module, `url` dep).
- Backend shell: `keeper/src/ipc.rs` (`AppState.oauth_flows`, `login_oidc`/`cancel_oidc`, `DesktopPlatform::open_url`, error mappings), `keeper/src/lib.rs` (deep-link plugin + `on_open_url` routing + command registration), `Cargo.toml`s, `tauri.conf.json` (`keeper` scheme), `capabilities/default.json` (`deep-link:default`).
- Frontend: `lib/ipc/client.ts` (`loginOidc`/`cancelOidc`), `lib/ipc/gen/IpcErrorCode.ts` (regenerated), `components/auth/login-screen.tsx` (OIDC affordance + pending/Cancel + named errors + unmount cleanup), `login-screen.test.tsx` (OIDC tests), `package.json`/`bun.lock` (`@tauri-apps/plugin-deep-link`).

**Review findings.** 2 reviewers (adversarial + edge-case). After dedup/severity/triage: intent_gap 0, bad_spec 0, patch 5 (medium 2, low 3), defer 0, reject 19.
- Patches applied: (1) registry residue leak — RAII `FlowGuard` + `remove` remove the `state` entry on every exit path (fixes the "no dangling callback" constraint the story is graded on); (2) sign-out/persister keychain-resurrection race — persister handle now aborted+awaited in `shutdown` before store teardown; (3) `Lagged` now persists the current session so the final rotated token isn't lost; (4) `from_json` surfaces tagged-blob parse errors instead of masking them as legacy; (5) frontend unmount cancels a pending flow.
- Notable rejects (with reason): `cancel_all` cancels all in-flight flows — by-design per Design Notes (single-flight UI; frontend has no `state`); password-path `restore_session` "dropped `RoomLoadSettings`" — verified identical (`Client::restore_session` internally uses `RoomLoadSettings::default()`); spurious callbacks lacking host/path validation — guarded by the secret `state`; late callback double-add — `addAccount` upserts idempotently; orphaned server-side MAS dynamic registrations — inherent to RFC 7591, not local residue.

**Verification.**
- `bun run check:rust` — PASS (rustfmt clean, clippy `-D warnings`), re-run after patches.
- `bun run test:rust` — PASS, 140 tests (registry resolve/cancel/spurious/dropped/**remove-residue**; `StoredSession` both kinds + legacy + **tagged-corrupt-errors**; 4 new error-code mappings; native registration metadata).
- `bun run check` — PASS (biome + tsc strict + 180 vitest + keeper-core tauri-free boundary).
- `cargo deny check` — licenses/bans/sources **PASS**; the `advisories` section's failures are the pre-existing gtk-rs/tauri-ecosystem unmaintained-crate warnings (same baseline Story 2.1 shipped on) — the added deps introduce no new advisories.
- ts-rs bindings regenerated (only `IpcErrorCode.ts` changed); `handle_refresh_tokens`/OAuth restore covered by unit tests.

**Residual risks.**
- The end-to-end browser + deep-link round-trip is only manually verifiable against a real MAS homeserver in a **built** app (macOS custom-scheme deep links need `CFBundleURLTypes`; may not fire under `tauri dev`). Unit tests cover the registry, session serde, error mapping, and metadata; the live flow is a manual check.
- `client_uri`/`client_name` for dynamic registration are `https://keeper.tgorka.dev/` / `keeper` (consistent with the `dev.tgorka.keeper` bundle id) — shown on the MAS consent screen; a canonical product URL is a later product decision, not a code defect.

---

**Follow-up review pass (2026-07-04).** An independent second review (Blind Hunter + Edge Case Hunter over the full baseline→HEAD diff) was run because the first pass had `followup_review_recommended: true`. Outcome: intent_gap 0, bad_spec 0, **patch 0**, defer 1, reject 23 — **no code changes**. All 5 first-pass patches were re-verified against the code and confirmed correct. The two highest-signal re-raised concerns were verified as non-issues:
- **Persister/sign-out "keychain resurrection race"** — `AccountManager::shutdown` runs `session_persister.abort(); let _ = session_persister.await;` *before* `sign_out_cleanup` deletes the Keychain key, and awaiting a `JoinHandle` waits for full task termination. A tokio task cannot be cancelled mid-synchronous section, so any in-flight `keychain_set` completes before the await returns and no write can occur afterward; every persister write strictly precedes the delete. The prior fix is sound.
- **`StoredSession::from_json` legacy detection** — a serialized `MatrixSession` has no top-level `"kind"` field, so the "tagged ⇔ has `kind`" discriminant never misclassifies a legacy blob; tagged-blob parse errors are surfaced, not masked.

`cancel_all`/single-flight, `access_denied`→retriable `OAuthFailed`, the SSS-before-OAuth gate, and the identical `RoomLoadSettings::default()` password-restore behavior are all explicitly by-design per the intent contract / Design Notes (and several were already adjudicated in the first pass). One genuine, low-severity, **pre-existing** item was deferred (not caused by this story): OIDC sign-out does local-only teardown and never revokes the delegated token at MAS, so the server-side session lingers until expiry (see `deferred-work.md`; AD-10 chose local-only logout, which is materially weaker for long-lived OAuth refresh tokens). Because this pass made no code changes, `followup_review_recommended` is now `false`.
