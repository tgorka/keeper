---
title: 'Story 1.8 — Session Restore and Sign-Out'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '0891dc28454eb17103d86915e16c6560e7f01653'
final_revision: 'fcc60a470dfc79794dc791f35e53a6ae68494cc9'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Epic 1's account lifecycle is only half-wired. Login (1.3) persists the Matrix session (Keychain + `keeper.db` row + SDK SQLite store), and `activate()` already restores it — but only *lazily* on first subscribe, and the frontend always boots to the login screen because `currentAccount` starts `null`. There is no eager restore-on-launch, and `AccountManager::shutdown` (built in 1.4) is dead code: no sign-out path calls it, and it only tears down in-memory tasks — it never deletes the Keychain entry, registry row, or SDK store dir. So a relaunch re-shows login even though the session is intact, and there is no way to sign out.

**Approach:** Close the lifecycle with two thin seams over existing machinery. (a) **Restore:** a Rust-authoritative `session_restore` command reports the persisted account's identity (registry row + a present Keychain session); the frontend hydrates `currentAccount` from it at boot behind a splash gate (no login-flash), then the *existing* lazy room-list subscribe drives `activate()`→`restore_session` and renders cached chats before the network settles. (b) **Sign-out:** a `sign_out` command orchestrates `AccountManager::shutdown` (stop sync, abort tasks, drop live state) then deletes exactly this account's SDK dir + Keychain entry + registry row — mirroring the existing `auth::rollback` cleanup — and the frontend resets its stores and returns to login. No new session/crypto logic; both stories wire and prove seams that already exist.

## Boundaries & Constraints

**Always:**
- All lifecycle logic stays in `keeper-core`; the `keeper` shell stays IPC/platform glue only, no `tauri` dep leaks into core (AD-6). Restore/sign-out orchestration and the "what to delete" decision live in core.
- **Reuse existing restore machinery.** `activate()` already builds the client with `.sqlite_store(accounts/<id>/sdk)` and calls `restore_session`; do **not** add a second restore path. `session_restore` returns identity only (no eager activation) — the already-shipped lazy room-list subscribe path (Story 1.4) restores the session and emits cached rooms first, then network diffs. Cold-start cached-first rendering is thereby inherited, not reimplemented.
- **`session_restore(account) -> Result<Option<AccountVm>, IpcError>`** (Rust-authoritative). Core `auth::find_restorable_account(platform)` lists persisted accounts (`registry::list_accounts`) and returns the first whose Keychain session (`session_keychain_key(id)`) is present, built as `AccountVm { accountId, userId, homeserverUrl }` from its `AccountRow`. A registry row without a Keychain session is **not** restorable (returns `None`/skipped) — never lands the user on a broken shell. Single-account slice: at most one account.
- **`sign_out(account_id) -> Result<(), IpcError>`** (Rust-authoritative, local-only). Core `AccountManager::sign_out(platform, account_id)` = `self.shutdown(account_id).await` (stop `SyncService`, abort reconnect supervisor + all producer tasks, drop timelines, remove from registry map — already implemented) **then** `auth::sign_out_cleanup(platform, account_id)`: `remove_dir_all(data_dir/accounts/<id>/sdk)`, `keychain_delete(session_keychain_key(id))`, `registry::delete_account(data_dir, id)` — each idempotent/best-effort, tolerating already-absent state, mirroring `auth::rollback`. Shutdown-first so SQLite handles release before the dir is removed. Idempotent whether or not the account was ever activated (shutdown is a no-op when absent).
- **AD-10 exact-deletion invariant.** Sign-out touches only *this* account's persisted state: its `accounts/<id>/sdk` dir, its `session/<id>` Keychain entry, and its own `keeper.db` registry row — nothing belonging to another account, no unrelated file. Deleting this account's own registry row is part of removing "this account" (required for "no residual session"), consistent with AD-10's "nothing else" (which scopes out *other* accounts / unrelated data).
- **WAL is already satisfied — keep it.** `keeper.db` opens `journal_mode=WAL` (`registry.rs`, asserted by `db_uses_wal_journal_mode`); matrix-sdk-sqlite manages its own store WAL. No change; force-quit resilience (NFR-8) is inherited. Do not regress it.
- **Frontend restore gate (no login-flash).** Add `hydrated: boolean` (default `false`) + `markHydrated()` to the accounts store. A boot hook `useSessionRestore` calls `sessionRestore()` once, `setCurrentAccount` if it returns an account, and `markHydrated()` **always** (incl. error → fail safe to login). `App.tsx` renders a minimal accessible splash (`role="status"`) while `!hydrated`, then `currentAccount ? <AppShell/> : <LoginScreen/>`.
- **Frontend sign-out teardown.** A `useSignOut` hook returns a handler that awaits `signOut(accountId)` then resets stores: `rooms.selectRoom(null)` + `rooms.clear()` (not auto-triggered by account-null), `timeline.clear()`, `connection.reset()`, and `accounts.clear()` last (unmounts the shell → pane cleanups unsubscribe). Confirmation is required: a sign-out control in the sidebar footer opens a shadcn `Dialog` (installed; no `alert-dialog`) whose confirm invokes the handler; cancel is a no-op.
- TS: no `any`, `import type`, `@/` alias, 2-space/100-col/double-quote Biome, `cn()` for classes, reuse installed `src/components/ui/` shadcn primitives — never hand-write there. Vanilla zustand stores created outside React (AD-9). Rust: no `.unwrap()`/bare `.expect()` in production paths, `?` + `thiserror`, clippy `-D warnings` clean, `tracing` (account id / subscription id only — never token, session JSON, user id in message bodies, or plaintext).

**Block If:**
- `matrix-sdk` 0.18 `restore_session` / `AuthSession` serialization, `AccountManager::shutdown`, or the `Platform` keychain/data-dir ports prove absent or differently shaped than the vendored source (all verified present during planning; only block if implementation disproves).

**Never:**
- No **server-side** `matrix_auth().logout()` call — Epic 1 sign-out is local per AD-10 ("nothing else") and must work offline. No token/device invalidation on the homeserver, no OIDC end-session (Epic 2).
- No eager `activate()` in `session_restore`; no second restore/reconnect path; no new `SyncService`/send-queue wiring (Story 1.7 owns that). No multi-account UI, account switcher, or dropdown menu (Story 2.5). No Settings screen. No destructive "keep vs delete local archive" choice (Story 5.7) — Epic 1 sign-out deletes the SDK store. No new `SendState`/timeline behavior. No `matrix-js-sdk`; no token/crypto in TS; no token in any JS-reachable storage.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Restore with valid session | launch; registry row + Keychain `session/<id>` present | `session_restore` → `AccountVm`; frontend hydrates `currentAccount`, renders `AppShell`; room-list subscribe → `activate()`→`restore_session` renders cached chats first, then SSS diffs | none |
| Cold launch, no account | launch; empty registry | `session_restore` → `null`; splash → `LoginScreen`; no flash | none |
| Registry row, Keychain session gone | launch; row present, `keychain_get(session/<id>)` = `None` | treated as not restorable → `null` → `LoginScreen` (no broken shell) | none (defensive skip) |
| Sign out confirmed (online) | signed in, user confirms Dialog | `sign_out`: shutdown → delete `accounts/<id>/sdk` + `session/<id>` Keychain + registry row; frontend resets stores + `accounts.clear()` → `LoginScreen`; relaunch → `null` → login | cleanup error → `IpcError` surfaced, still non-secret |
| Sign out while offline | signed in, disconnected, confirm | local-only sign-out succeeds (no server call); returns to login | none |
| Sign out, account not yet activated | restored but never subscribed (absent from manager map) | `shutdown` no-ops; persistent cleanup still runs; returns to login | none (idempotent) |
| Force-quit while signed in | mid-session kill, relaunch | WAL stores intact (zero loss); `session_restore` restores without re-login; sync resumes via SSS | none (SDK + WAL) |
| Cancel sign-out Dialog | Dialog open, user cancels | no IPC call; stays signed in; no store change | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- add `pub fn list_accounts(data_dir: &Path) -> Result<Vec<AccountRow>, CoreError>` (select all rows). `insert/delete/get_account` unchanged. WAL setup already present.
- `src-tauri/crates/keeper-core/src/auth.rs` -- add `pub fn find_restorable_account(platform: &dyn Platform) -> Result<Option<AccountVm>, CoreError>` (list rows, return first with a present Keychain session as `AccountVm`) and `pub fn sign_out_cleanup(platform: &dyn Platform, account_id: &str) -> Result<(), CoreError>` (remove `accounts/<id>/sdk` dir + `keychain_delete` + `registry::delete_account`; idempotent, mirroring the private `rollback`). Reuse `session_keychain_key` and the `data_dir.join("accounts").join(id).join("sdk")` path convention.
- `src-tauri/crates/keeper-core/src/account.rs` -- add `pub async fn sign_out(&self, platform: &dyn Platform, account_id: &str) -> Result<(), CoreError>` = `self.shutdown(account_id).await` then `auth::sign_out_cleanup(platform, account_id)?`. `shutdown` (in-memory teardown) already exists; no change to `activate`.
- `src-tauri/crates/keeper/src/ipc.rs` -- `#[tauri::command] async fn session_restore(state) -> Result<Option<AccountVm>, IpcError>` (→ `auth::find_restorable_account(state.platform.as_ref())`) and `async fn sign_out(state, account_id: String) -> Result<(), IpcError>` (→ `state.accounts.sign_out(state.platform.as_ref(), &account_id).await`); both funnel through existing `to_ipc_error` (no new `IpcErrorCode`).
- `src-tauri/crates/keeper/src/lib.rs` -- register `session_restore` + `sign_out` in `generate_handler!`.
- `src/lib/ipc/gen/` -- **no new bindings** (`AccountVm` already exported; `Option<AccountVm>`→`AccountVm | null`, `()`→`void`). Confirm regeneration is byte-identical.
- `src/lib/ipc/client.ts` -- add `sessionRestore(): Promise<AccountVm | null>` (`"session_restore"`) and `signOut(accountId: string): Promise<void>` (`"sign_out"`) via `invoke`.
- `src/lib/stores/accounts.ts` -- add `hydrated: boolean` (default `false`) + `markHydrated()`; keep `currentAccount`, `setCurrentAccount`, `clear`.
- `src/hooks/use-session-restore.ts` -- NEW boot hook: on mount call `sessionRestore()`, `setCurrentAccount` if present, `markHydrated()` always (incl. catch). Run once.
- `src/hooks/use-sign-out.ts` -- NEW: returns `signOut()` handler that awaits `signOut(accountId)` then resets stores (`rooms.selectRoom(null)`, `rooms.clear()`, `timeline.clear()`, `connection.reset()`, `accounts.clear()` last).
- `src/App.tsx` -- mount `useSessionRestore()`; render splash while `!hydrated`; then `currentAccount ? <AppShell/> : <LoginScreen/>`.
- `src/components/layout/account-footer.tsx` -- NEW: sidebar-footer account row (signed-in `userId`, collapsed-rail icon variant) + Sign out control opening a `Dialog` confirm; confirm → `useSignOut`. Focus rings, accessible labels (a11y baseline).
- `src/components/layout/sidebar-pane.tsx` -- mount `<AccountFooter collapsed={...} />` as the persistent bottom footer element; keep the offline pill directly above it (both in the `mt-auto` footer region).
- Tests: Rust (`registry.rs` `list_accounts`; `auth.rs` `find_restorable_account` present/missing-session/empty + `sign_out_cleanup` deletes exactly the three targets and is idempotent when absent; `account.rs` `sign_out` idempotent when account not active). Frontend (`accounts.test.ts` `hydrated`/`markHydrated`; `use-session-restore.test.ts` restores→setaccount+hydrate, null→hydrate-only, error→hydrate-only; `use-sign-out.test.ts` calls `signOut` then all resets in order; `account-footer.test.tsx` Dialog confirm invokes handler / cancel no-ops / collapsed variant; `sidebar-pane.test.tsx` footer row present when signed in; `App.test.tsx` splash while `!hydrated`, then restore→shell / no-account→login — update existing routing tests to set `hydrated`).

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/registry.rs` -- add `list_accounts` (+ test).
- [x] `keeper-core/src/auth.rs` -- add `find_restorable_account` + `sign_out_cleanup` (+ tests for present/missing/empty and exact-deletion/idempotent).
- [x] `keeper-core/src/account.rs` -- add `AccountManager::sign_out` (shutdown + cleanup) (+ idempotency test).
- [x] `keeper/src/ipc.rs` -- `session_restore` + `sign_out` commands via `to_ipc_error`.
- [x] `keeper/src/lib.rs` -- register both commands.
- [ ] regenerate ts-rs bindings; confirm **no diff** (no new VM types).
- [x] `src/lib/ipc/client.ts` -- `sessionRestore` + `signOut` wrappers.
- [x] `src/lib/stores/accounts.ts` (+ test) -- `hydrated` + `markHydrated`.
- [x] `src/hooks/use-session-restore.ts` (+ test) -- boot restore/hydrate.
- [x] `src/hooks/use-sign-out.ts` (+ test) -- sign-out + store resets in order.
- [x] `src/App.tsx` (+ update `App.test.tsx`) -- mount restore hook; splash-until-hydrated gate.
- [x] `src/components/layout/account-footer.tsx` (+ test) -- account row + Dialog confirm sign-out (expanded + collapsed).
- [x] `src/components/layout/sidebar-pane.tsx` (+ test) -- mount `AccountFooter` in footer; offline pill above it.

**Acceptance Criteria:**
- Given a signed-in account and a force-quit, when keeper relaunches, then the session restores from the SDK store + Keychain without re-login (`session_restore` → `AccountVm`, frontend boots straight to `AppShell` behind a no-flash splash), previously synced chats render from local cache before network round-trips complete, and sync resumes via SSS (FR-8, NFR-1 path); and all SQLite stores run in WAL mode so the force-quit loses zero previously persisted state (NFR-8).
- Given the sidebar-footer account row, when the user chooses Sign out and confirms, then keeper deletes exactly `accounts/<ulid>/sdk/` and that account's Keychain entry (and its own registry row) — nothing else — stops the account's supervision tasks (`shutdown`), and returns to the login screen (AD-10); and relaunching after sign-out lands on login with no residual session (`session_restore` → `null`).
- Given AD-6/AD-10, then all restore/sign-out orchestration and the exact-deletion decision live in `keeper-core`, the shell adds only IPC glue, no token or session JSON crosses IPC or reaches `tracing`, and the frontend holds only the non-secret `AccountVm`.
- Given the FR-41 gate and Story 1.7 seams, then this story adds no new send-dispatch call site, no new `SyncService`/send-queue path, and no server-side logout.
- Given the quality gates, when `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check` (from `src-tauri/`) run, then all pass.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 0
- reject: 11
- addressed_findings:
  - `[medium]` `[patch]` `sign_out_cleanup` removed the SDK store dir *first* and propagated its error before deleting the Keychain session and registry row, so a transient `remove_dir_all` failure (file lock / permission on macOS) returned `Err` with the registry row + Keychain session still present — `find_restorable_account` would then report the account as restorable on next launch, `activate()` would rebuild against the gone/partial store, and the user would land on a broken shell that can't sync and can't be cleanly signed out of. Reordered to delete the registry row + Keychain session **first** (both propagating), with best-effort store-dir removal **last** (logged, not propagated, mirroring `rollback`), so a dir-removal failure can never resurrect a "restorable" ghost — the account is already non-restorable once either key is gone. `keeper-core/src/auth.rs`. Existing tests unchanged and green (`sign_out_cleanup_deletes_exactly_the_three_targets`, `sign_out_cleanup_is_idempotent_when_absent`).
  - Rejected (11, verified out-of-scope / non-issue / mitigated): frontend teardown ordering + stale post-await unsubscribe (`shutdown` aborts the backend producer tasks before the dir is removed; the frontend's later unsubscribe against an already-removed account is a harmless no-op and the `use-sign-out` doc-comment is accurate); dead-shell-on-cleanup-failure (mitigated by the patch — the realistic `remove_dir_all` failure no longer errors); SDK-dir existence check in `find_restorable_account` (the patch closes the only internally-created path; the residual "keychain present but dir externally deleted" is out-of-scope tampering, and the realistic "row present, session gone" case is already handled); stale `accountId` closure in `use-sign-out` and non-deterministic `list_accounts` tie-break (both latent for multi-account, no Epic-1 trigger — Epic 2's account switcher/manager owns restore selection); no separate outbox/send store to reset (send state rides the timeline store, which is cleared); missing live-account deletion test (the live `shutdown`→`remove_dir_all` path needs a real Synapse — a documented live-only residual consistent with Stories 1.5–1.7); silent sign-out error UX (the failure path is near-unreachable after the patch and Epic-1 keeps the footer minimal); empty-string `userId` render guard (`userId` is a guaranteed non-empty Matrix user id from a successful login); multiple restorable rows orphaning (single-account slice); footer local-state reset on the success path (correct as-is — `accountsStore.clear()` unmounts the component; resetting state after unmount would warn).

## Design Notes

**Restore is identity-only; cached-first rendering is inherited.** `activate()` (built in 1.4/1.7) already does `.sqlite_store(accounts/<id>/sdk)` → `restore_session` → `SyncService::with_offline_mode` → `set_enabled(true)`. It fires lazily on the first `room_list_subscribe`, which the `ChatListPane` issues as soon as `AppShell` mounts. So `session_restore` only needs to get the frontend into the shell with the right `accountId`; the existing subscribe path then restores the session and the SDK's `RoomListService` emits the cached room window first (local, no round-trip), then network diffs — satisfying "cached chats render before network." Adding an eager `activate()` in restore would duplicate that path and risk a double-activation race; explicitly avoided.

**Why a `hydrated` gate.** `App.tsx` currently renders `LoginScreen` whenever `currentAccount` is `null`, which is the initial state — so without a gate a restorable user sees a login flash before the async restore resolves. `hydrated` (false until the one-shot restore attempt completes) holds a splash instead, and `markHydrated()` runs on both success and failure so a failed/empty restore falls through to login rather than hanging.

**Sign-out cleanup mirrors `rollback`, plus the registry row.** `auth::rollback` (private, used on login failure) already removes the SDK dir + Keychain entry; it does *not* delete a registry row only because login rollback runs before the row is inserted. Sign-out runs after the row exists, so `sign_out_cleanup` adds `registry::delete_account`. Deleting this account's own row is what makes "no residual session on relaunch" true and is squarely "this account's" state — not the "something else" AD-10 forbids. Order is shutdown → dir → keychain → registry, each tolerant of already-absent state (idempotent), so a partial prior sign-out or an inactive account both converge cleanly.

**Sign-out is local by design.** AD-10 scopes logout to deleting local state ("nothing else") and Epic 1 targets offline-capable operation, so no homeserver `logout` is issued — the persisted session is destroyed locally and cannot be restored. Server-side token/device revocation belongs to the OIDC/multi-account work (Epic 2), not here.

**No new IPC error code / VM type.** `session_restore` reuses `AccountVm`; `Option`/`()` need no ts-rs export. Cleanup/registry/keychain failures funnel through the existing `to_ipc_error` (`Platform`/`Internal` arms) — consistent with Story 1.7 adding no new `IpcErrorCode`.

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest green (new `accounts.hydrated`, `use-session-restore`, `use-sign-out`, `account-footer`, updated `App`/`sidebar-pane` tests).
- `bun run check:rust` -- expected: rustfmt + clippy `-D warnings` clean; core stays tauri-free; no `.unwrap()`.
- `bun run test:rust` -- expected: cargo-nextest green (`list_accounts`, `find_restorable_account`, `sign_out_cleanup`, `AccountManager::sign_out`); ts-rs bindings regenerate byte-identical (no new files).
- `cd src-tauri && cargo deny check` -- expected: license firewall passes (no new crates).

**Manual checks (require a real Synapse ≥ 1.114 — automated tests can't exercise live restore/session):**
- `op run --env-file=.env.1p -- bun run tauri dev`: sign in → force-quit the app → relaunch → lands directly in the chat UI with cached chats visible before network settles, no re-login.
- In the signed-in app, open the sidebar-footer account row → Sign out → confirm → returns to the login screen; relaunch → still the login screen (no residual session). Inspect that only `accounts/<id>/sdk` and the `session/<id>` Keychain entry are gone.

## Auto Run Result

Status: **done**

### Summary
Closed Epic 1's account-lifecycle loop over the machinery Stories 1.3–1.7 already built. **Restore:** a new Rust-authoritative `session_restore` command (`auth::find_restorable_account` → `registry::list_accounts` + a present Keychain session, returning the non-secret `AccountVm`) reports the persisted account's identity at boot; the frontend hydrates `currentAccount` behind a `hydrated` splash gate (no login-flash, fail-safe to login), then the *existing* lazy room-list subscribe drives `activate()`→`restore_session` and renders cached chats before the network settles. Restore reports identity only — no eager activation, no second restore path. **Sign-out:** a new `sign_out(account_id)` command (`AccountManager::sign_out` = existing `shutdown` in-memory teardown, then `auth::sign_out_cleanup`) deletes exactly this account's registry row + `session/<id>` Keychain entry + `accounts/<id>/sdk` store dir — local-only per AD-10, works offline, no server-side logout. The frontend `useSignOut` resets the mirror stores (`rooms`/`timeline`/`connection`) and clears `accounts` last (unmounting the shell → login). WAL (NFR-8) was already in place (`keeper.db` + matrix-sdk-sqlite) and is unchanged. No new IPC error code or ts-rs binding (`AccountVm` reused; `Option`/`()` need none).

### Files changed
- `crates/keeper-core/src/registry.rs` — `list_accounts()` (all rows, ordered by `created_ts`) + test.
- `crates/keeper-core/src/auth.rs` — `find_restorable_account()` (first row with a present Keychain session → `AccountVm`; rows without a session skipped) and `sign_out_cleanup()` (delete registry row + Keychain session first, best-effort store-dir removal last — review-hardened ordering) + tests (present/missing-session/empty; exact-three-target deletion with untouched sibling; idempotent-when-absent).
- `crates/keeper-core/src/account.rs` — `AccountManager::sign_out` (`shutdown` then `sign_out_cleanup`) + idempotent-when-inactive test.
- `crates/keeper/src/ipc.rs` — `session_restore` + `sign_out` commands through the existing `to_ipc_error` (no new `IpcErrorCode`).
- `crates/keeper/src/lib.rs` — registered both commands.
- `src/lib/stores/accounts.ts` (+ test) — `hydrated` (default `false`) + `markHydrated()`; `clear()` preserves `hydrated`.
- `src/lib/ipc/client.ts` — `sessionRestore()` + `signOut(accountId)` wrappers.
- `src/hooks/use-session-restore.ts` (+ test) — mount-once restore/hydrate (StrictMode-safe cancel flag).
- `src/hooks/use-sign-out.ts` (+ test) — `signOut` then ordered store resets, `accounts.clear()` last.
- `src/components/layout/account-footer.tsx` (+ test) — sidebar-footer account row (truncated `userId`), `Dialog`-confirmed sign-out, collapsed-rail variant, a11y labels/focus rings.
- `src/components/layout/sidebar-pane.tsx` (+ test) — mounts `AccountFooter` as the persistent footer element with the offline pill directly above it.
- `src/App.tsx` (+ `App.test.tsx`) — mounts `useSessionRestore()`, `role="status"` splash while `!hydrated`, then the account ternary.

### Review findings
- Two fresh-context reviewers (adversarial-general Blind Hunter + edge-case-hunter). Triage: **0 intent_gap, 0 bad_spec, 1 patch (medium), 0 defer, 11 reject**. See Review Triage Log.
- **Patch (applied):** reordered `sign_out_cleanup` to delete the registry row + Keychain session before a best-effort SDK-store-dir removal, so a transient `remove_dir_all` failure can no longer strand a "restorable" ghost that would land the user on a broken shell on relaunch.
- **Rejected (11):** frontend teardown ordering (backend tasks aborted before dir removal — no-op unsubscribe); dead-shell-on-failure (mitigated by the patch); dir-existence restore check + multi-account tie-break/stale closure (out-of-scope tampering / latent for Epic 2); no separate outbox store; live-account deletion test (live-only residual); silent error UX / empty-`userId` / footer reset (near-unreachable or correct-as-is).

### Verification
- `bun run check` ✅ — biome clean, tsc strict clean, vitest **167 passed (21 files)**, core-tauri-free guard passes.
- `bun run check:rust` ✅ — rustfmt `--check` + clippy `--all-targets -D warnings` clean (re-run after the patch).
- `bun run test:rust` ✅ — cargo-nextest **108 passed, 0 skipped**; ts-rs bindings regenerate byte-identical (no new/changed files under `src/lib/ipc/gen/`).
- `cd src-tauri && cargo deny check licenses bans sources` ✅ (`bans ok, licenses ok, sources ok`). No new crate — `Cargo.lock`/`Cargo.toml` unchanged. The pre-existing OpenSSL unmatched-allowance warning is unchanged (identical to Stories 1.1–1.7); the transitive Tauri/GTK3 `unmaintained` RUSTSEC advisories under `cargo deny check advisories` are the same baseline item flagged since 1.1 and out of this story's scope (no dependency change).
- Not run: live restore/sign-out against a real Synapse ≥ 1.114 (the epic exit gate) — reasoned-about and unit-tested only at the pure seams. See Manual checks.

### Residual risks
- The live path — actual force-quit → restore-without-re-login with cached-first render, and confirmed sign-out deleting exactly `accounts/<id>/sdk` + the Keychain entry then landing on login with no residual — runs only against a real homeserver, consistent with Stories 1.5–1.7's live-only residuals.
- Multi-account restore selection (`find_restorable_account` "first with session", `list_accounts` ordering) is defined for the single-account slice only; Epic 2's account manager/switcher owns deterministic multi-account selection.
- On the near-impossible failure of the registry-row or Keychain delete (rusqlite/keyring error) *after* `shutdown`, sign-out surfaces an `IpcError` and the user stays on a now-dead shell until relaunch (which then lands on login); the realistic `remove_dir_all` failure is fully absorbed by the patch.
- `followup_review_recommended: false` — the single review-driven change is a localized, test-covered backend reordering, not broad or complex enough to warrant an independent follow-up.
