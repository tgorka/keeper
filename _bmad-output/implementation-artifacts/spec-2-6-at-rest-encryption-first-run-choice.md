---
title: 'At-Rest Encryption First-Run Choice'
type: 'feature'
created: '2026-07-04'
baseline_revision: '664f7f171a2af0cf3e9530c5b3873e25bfc68185'
final_revision: 'c8e791218f50121c62bf8c52c4bb56ba2788ff07'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-2-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper's per-account matrix-sdk-sqlite stores under `accounts/<id>/sdk/` are created unencrypted (`.sqlite_store(&sdk_dir, None)` in both `add_account` and `activate`), so session/crypto state at rest is protected only by FileVault. NFR-10/AD-22 require an opt-in, first-run passphrase choice that encrypts SDK stores with matrix-sdk-sqlite's native passphrase (generated, kept only in the Keychain), plus honest settings copy that `archive.db`/`keeper.db` are not passphrase-encrypted in this version.

**Approach:** Persist an app-wide encryption **posture** (`on`/`off`, default off) in a new `settings` key/value table in `keeper.db`. On a fresh install (zero accounts, posture unchosen), present a first-run choice before the first login; the chosen posture is read inside the single `add_account` command, which — when on — generates a random per-account passphrase, stores it only in the Keychain at `store_passphrase/<id>`, and builds the SDK store with `Some(passphrase)`. `activate` re-reads that per-account Keychain entry (self-describing: present ⇒ encrypted). Sign-out/rollback also delete it. A read-only Settings → Archive & Storage surface states the honest storage posture.

## Boundaries & Constraints

**Always:**
- The passphrase is generated in Rust (`rand`, alphanumeric ≥ 32 chars), written **only** to the macOS Keychain (`keychain_set`), and never returned over IPC, logged, or written to `keeper.db`/disk (AD-10, NFR-9).
- The posture is chosen **before** the first account's store is built; matrix-sdk-sqlite's passphrase can only be set at store creation and cannot be retrofitted (see Design Notes) — so the first-run choice gates first-account onboarding on a fresh install (`accounts.length === 0 && encryptionPosture() === null`).
- Default is **off** (FileVault posture): a `Continue` without opting in persists `off`.
- Subsequent account adds read the persisted posture and are **never** re-prompted; `add_account` applies it uniformly to the Nth store.
- Encrypted-store identity is per-account and self-describing: `activate` passes `Some(passphrase)` iff `store_passphrase/<id>` exists in the Keychain, else `None`.
- Rollback (`add_account` failure) and `sign_out_cleanup` delete the `store_passphrase/<id>` Keychain entry too (best-effort; `keychain_delete` already tolerates `NoEntry`).
- Settings copy follows voice rules (UX-DR10): sentence case, no exclamation marks, no softening; names the consequence plainly; glossary nouns capitalized. Exported copy constants pinned by tests.
- Reuse existing shadcn primitives (`Dialog`, `Card`, `Alert`, `Switch`, `Button`, `Label`); no new frontend deps.

**Block If:**
- matrix-sdk 0.18's `.sqlite_store(path, Option<String>)` passphrase parameter is not the current signature (Agent-verified present at `auth.rs:455` / `account.rs:847`) — none anticipated.

**Never:**
- Do not pass any session token, passphrase, or store material across IPC into JavaScript; the passphrase never leaves Rust except into the Keychain.
- Do not passphrase-encrypt `keeper.db` or `archive.db` (AD-22: FTS cannot index ciphertext; SQLCipher conflicts with matrix-sdk-sqlite's bundled SQLite) — settings copy must state this honestly.
- Do not add a post-first-run posture toggle, a store re-key/migration path, or per-account re-prompting — first-run choice only; changing posture later is out of scope.
- Do not add a full Settings routing system — wire the existing (inert) sidebar Settings button to a single `Dialog`; leave Chats/Bridges buttons unchanged.
- No secrets in code/tests/fixtures; no softening copy; no exclamation marks.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Fresh install, posture unchosen | 0 accounts, `encryptionPosture()===null` | First-run choice shown before login form | none |
| Opt out (default) | user clicks Continue, switch off | `set_encryption_posture(false)`; login form then shown | none |
| Opt in | switch on, Continue | `set_encryption_posture(true)` persisted; login form shown | none |
| `add_account`, posture on | setting `sdk_encryption="on"` | passphrase generated, stored at `store_passphrase/<id>`, `.sqlite_store(&sdk_dir, Some(pw))` | any Phase-B failure → rollback deletes passphrase + session + dir |
| `add_account`, posture off/absent | setting off or missing | no passphrase entry; `.sqlite_store(&sdk_dir, None)` (FileVault) | rollback as today |
| Restore encrypted account | Keychain has `store_passphrase/<id>` | `activate` builds store with `Some(passphrase)` | wrong/missing passphrase → `RestoreFailed` |
| Restore unencrypted account | no passphrase entry | `activate` builds store with `None` | none |
| Sign out encrypted account | posture-on account | deletes registry row + `session/<id>` + `store_passphrase/<id>` + sdk dir | best-effort; missing entries tolerated |
| Subsequent add (2nd..N) | `accounts.length > 0` | no choice shown; `add_account` applies stored posture | none |
| Settings → Archive & Storage | dialog open | honest copy: `archive.db`/`keeper.db` not passphrase-encrypted, rely on FileVault; SDK-store status reflects posture | none |
| `get_setting`/`set_setting` | key/value roundtrip in `keeper.db` | value persisted and re-read; overwrite replaces | DB error → `CoreError::Internal` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/registry.rs` -- Add a `settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)` table in `open()` (idempotent `CREATE TABLE IF NOT EXISTS`, alongside `accounts`). Add `pub fn get_setting(data_dir, key) -> Result<Option<String>, CoreError>` and `pub fn set_setting(data_dir, key, value) -> Result<(), CoreError>` (`INSERT ... ON CONFLICT(key) DO UPDATE`).
- `src-tauri/crates/keeper-core/src/auth.rs` -- Posture + passphrase seam. Add `const SDK_ENCRYPTION_SETTING: &str = "sdk_encryption";`, `pub fn store_passphrase_keychain_key(account_id) -> String` (`format!("store_passphrase/{account_id}")`), `pub fn get_encryption_posture(platform) -> Result<Option<bool>, CoreError>` (`"on"→Some(true)`, `"off"→Some(false)`, absent→`None`), `pub fn set_encryption_posture(platform, enabled)`, and `fn generate_store_passphrase() -> String` (rand alphanumeric, 32). In `add_account` Phase B (inside the rollback-wrapped block, **before** `Client::builder()`): compute `let passphrase = if get_encryption_posture(platform)?.unwrap_or(false) { let pw = generate_store_passphrase(); platform.keychain_set(&store_passphrase_keychain_key(&account_id), &pw)?; Some(pw) } else { None };` then `.sqlite_store(&sdk_dir, passphrase.clone())`. Extend `rollback(...)` to also delete `store_passphrase/<id>` (best-effort). Extend `sign_out_cleanup` to delete `store_passphrase/<id>` (best-effort, after the session delete).
- `src-tauri/crates/keeper-core/src/account.rs` -- In `activate` (~L845): `let passphrase = platform.keychain_get(&auth::store_passphrase_keychain_key(account_id))?;` then `.sqlite_store(&sdk_dir, passphrase)`.
- `src-tauri/crates/keeper-core/Cargo.toml` -- Add `rand = "0.8"` (matrix-sdk 0.18 already resolves it; permissive license — must pass `cargo deny`).
- `src-tauri/crates/keeper/src/ipc.rs` -- Two sync commands (mirror `cancel_oidc` shape, funnel errors via `to_ipc_error`): `set_encryption_posture(state, enabled: bool) -> Result<(), IpcError>` and `encryption_posture(state) -> Result<Option<bool>, IpcError>`.
- `src-tauri/crates/keeper/src/lib.rs` -- Register `ipc::set_encryption_posture` and `ipc::encryption_posture` in `generate_handler!`.
- `src/lib/ipc/client.ts` -- Add `setEncryptionPosture(enabled: boolean): Promise<void>` and `encryptionPosture(): Promise<boolean | null>` wrappers (no new ts-rs type; `Option<bool>` ⇒ `boolean | null`).
- `src/components/settings/at-rest-encryption-choice.tsx` -- NEW. `AtRestEncryptionChoice({ onResolved })`: full-screen `Card` with title/explanation copy, a default-off `Switch` ("Encrypt Matrix stores with a passphrase"), a `Continue` button (idempotency-guarded) that calls `setEncryptionPosture(switchOn)` then `onResolved()`. Also renders the honest note that `archive.db`/`keeper.db` stay on FileVault. Export copy constants (choice + shared storage honesty sentences) for reuse/tests.
- `src/components/settings/settings-dialog.tsx` -- NEW. `SettingsDialog({ open, onOpenChange })`: `Dialog` with an "Archive & Storage" section reusing the storage honesty copy constants; loads `encryptionPosture()` on open to show SDK-store status ("passphrase-encrypted" / "not encrypted — FileVault only"). Read-only (no toggle).
- `src/App.tsx` -- When `!hasAccount`: load `encryptionPosture()` once; while loading hold the existing splash; if `null` render `<AtRestEncryptionChoice onResolved={…}/>`, else render `<LoginScreen/>`. The `addMode` overlay path (subsequent adds, `hasAccount === true`) is unchanged — never gated.
- `src/components/layout/sidebar-pane.tsx` -- Give the Settings view button an `onClick` opening `SettingsDialog` (controlled state); Chats/Bridges unchanged.
- Tests (colocated): `registry` settings roundtrip + `auth` posture/keychain-cleanup tests (Rust `#[cfg(test)]`); `src/components/settings/at-rest-encryption-choice.test.tsx`, `src/components/settings/settings-dialog.test.tsx` (NEW); extend `src/App.test.tsx` and add/extend `src/components/layout/sidebar-pane.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/registry.rs` -- Add `settings` table + `get_setting`/`set_setting`.
- [x] `src-tauri/crates/keeper-core/Cargo.toml` -- Add `rand = "0.8"`.
- [x] `src-tauri/crates/keeper-core/src/auth.rs` -- Posture helpers, `store_passphrase_keychain_key`, `generate_store_passphrase`; wire passphrase into `add_account` Phase B; extend `rollback` + `sign_out_cleanup` to delete the passphrase entry.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `activate` fetches the per-account passphrase and passes it to `.sqlite_store`.
- [x] `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` -- `set_encryption_posture` / `encryption_posture` commands, registered.
- [x] `src/lib/ipc/client.ts` -- `setEncryptionPosture` / `encryptionPosture` wrappers.
- [x] `src/components/settings/at-rest-encryption-choice.tsx` -- First-run choice component + exported copy constants.
- [x] `src/components/settings/settings-dialog.tsx` -- Settings dialog with Archive & Storage section (status + honest copy).
- [x] `src/App.tsx` -- Gate first-account onboarding behind the choice when posture is unchosen.
- [x] `src/components/layout/sidebar-pane.tsx` -- Open `SettingsDialog` from the Settings button.
- [x] Tests -- Rust: `settings` roundtrip; posture on/off roundtrip; `generate_store_passphrase` (len ≥ 32, alphanumeric, distinct across calls); `sign_out_cleanup` and `rollback` delete `store_passphrase/<id>`; `activate`/`add_account` pass `Some`/`None` per the passphrase-entry presence (via a fake `Platform`). Frontend: choice renders exact copy with no exclamation mark, default-off switch, Continue-off ⇒ `setEncryptionPosture(false)`+`onResolved`, toggle-on+Continue ⇒ `setEncryptionPosture(true)`; settings dialog shows the archive.db/keeper.db+FileVault copy and reflects posture status; App shows the choice when posture `null` and `LoginScreen` when chosen; Settings button opens the dialog.

**Acceptance Criteria:**
- Given a fresh install with no accounts, when the app is opened, then a first-run choice offers passphrase-based at-rest encryption for SDK stores (default off), and opting in causes the first account's `accounts/<id>/sdk/` store to be created with matrix-sdk-sqlite's native passphrase, generated and stored only in the Keychain (`store_passphrase/<id>`), never crossing IPC (NFR-10, AD-22, NFR-9).
- Given the encryption posture is on, when a subsequent account is added, then its SDK store is created encrypted with a fresh per-account passphrase without re-prompting; and when any encrypted account is restored on relaunch, then `activate` unlocks it with the per-account Keychain passphrase.
- Given an account is signed out, when cleanup runs, then its `store_passphrase/<id>` Keychain entry is deleted along with its session, registry row, and SDK dir, and nothing belonging to another account is touched.
- Given the user opens Settings → Archive & Storage, then the copy states honestly that `archive.db`/`keeper.db` are not passphrase-encrypted in this version and rely on FileVault, follows the voice rules, and reflects whether SDK stores are passphrase-encrypted (AD-22, UX-DR17).
- Given `bun run check`, `bun run check:rust`, `bun run test:rust`, and `cargo deny check`, then biome + tsc strict + vitest + fmt + clippy (`-D warnings`) + nextest + the license firewall all pass, including the new/updated tests.

## Design Notes

**Why the choice precedes the first login (AC-1 ordering reconciliation).** matrix-sdk-sqlite derives its store-cipher key from the passphrase at store **creation/open** time; 0.18 exposes no re-key API, so a store created with `None` cannot later become encrypted without a full migration (out of scope). The first account's store is created inside the single `add_account` Rust command (probe → build persistent `sqlite_store` → authenticate → persist), which cannot pause mid-flight for a UI round-trip, and splitting it to return a session to JS for a "choose-then-finalize" step would violate the "tokens never cross into JavaScript" invariant (NFR-9). Therefore the only architecture-preserving placement is to resolve the posture **before** the first `login_*` call, reading it back inside `add_account`. This honestly satisfies "on the first Account add on a fresh install, a first-run choice offers … and choosing it creates the store with the passphrase."

**Per-account passphrase, app-wide posture.** The posture (`sdk_encryption` in the `settings` table) decides whether *new* adds encrypt; the passphrase is generated per account and stored at `store_passphrase/<id>`. This makes `activate` self-describing (entry present ⇒ pass `Some`) with no registry schema change and no need to consult the posture at restore, and lets a mixed set (some accounts added before opt-in, some after) each restore correctly. Sign-out naturally scopes deletion to one account.

**Keychain cleanup safety.** `DesktopPlatform::keychain_delete` already maps `keyring::Error::NoEntry → Ok(())` (`ipc.rs:112-116`), so unconditionally deleting `store_passphrase/<id>` in `rollback`/`sign_out_cleanup` is safe for unencrypted (posture-off) accounts that never had the entry.

**Copy shape (voice-rules compliant, final wording in constants).** Example:
```
Archive & Storage
Your Matrix session and crypto state can be encrypted at rest with a passphrase.
keeper.db and archive.db are not passphrase-encrypted in this version and rely on
your Mac's FileVault. Turning this on generates a passphrase kept only in your
Keychain; it applies to every account you add.
```

## Verification

**Commands:**
- `bun run check` -- expected: biome + tsc strict + vitest green, including the new settings/choice/dialog/App/sidebar tests.
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean (no `.unwrap()`/`unsafe` in new code).
- `bun run test:rust` -- expected: cargo-nextest green, including registry/auth settings & passphrase-cleanup tests.
- `cd src-tauri && cargo deny check` -- expected: the new `rand` dependency passes the license firewall.

**Manual checks (if no CLI):**
- Fresh install (remove `~/Library/Application Support/dev.tgorka.keeper/`): confirm the first-run choice appears before login; opt in, sign in, and confirm a `store_passphrase/<id>` Keychain entry exists and the SDK store opens on relaunch. Sign out and confirm the entry is gone.
- Open Settings → Archive & Storage and confirm the honest archive.db/keeper.db + FileVault copy and the SDK-store status.

## Spec Change Log

_No `bad_spec` loopback occurred; the spec was implemented as written._

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 1
- reject: 14
- addressed_findings:
  - `[low]` `[patch]` The Settings → Archive & Storage surface rendered "Matrix stores are not encrypted — FileVault only." while the `encryptionPosture()` read was in flight (initial `null` state), so an actually-encrypted install briefly showed a wrong security claim on an honesty-focused surface (and a stale value flashed on reopen). Fixed: `settings-dialog.tsx` now distinguishes a loading state (`undefined`) from the resolved posture, shows a neutral `SDK_STORE_STATUS_LOADING` line while loading, and resets to loading on every (re)open.
  - `[low]` `[patch]` `sign_out_cleanup` deleted the `store_passphrase/<id>` secret with `let _ = …`, swallowing a genuine (non-`NoEntry`) Keychain failure with no log — asymmetric with `rollback` and the store-dir removal, which both `warn!`. Fixed: `auth.rs` now logs a `tracing::warn!` on a real delete failure (still best-effort, non-propagating) so a stranded secret leaves a forensic trail.
  - `[low]` `[patch]` The first-run choice's Continue button silently no-oped if `setEncryptionPosture` rejected (guard reset + button re-enabled, but no feedback), inconsistent with the app's error-surfacing convention. Fixed: `at-rest-encryption-choice.tsx` surfaces an inline `role="alert"` `CHOICE_SAVE_ERROR` and allows retry; added a failure-path test.
- notes: Blind Hunter + Edge Case Hunter reviewed the full baseline→working-tree diff. Deferred 1: an encrypted account whose Keychain passphrase entry is lost restores with a generic decrypt failure and no honest "encryption key missing" state (and boots as a normal-but-never-syncing account) — inherent to the spec's deliberate self-describing, no-registry-schema design; an honest surface needs a durable per-account encrypted-flag + a new restore-failure state (logged to `deferred-work.md`, likely alongside Epic 3's UTD/verification honest states). Rejected 14, all either consequences of consciously-chosen, spec-documented design or pre-existing crate-wide patterns: passphrase held as a plain (un-zeroized) `String` (the whole crate, incl. session tokens, does this — Keychain is the boundary); orphaned-secret-on-process-kill mid-add (pre-existing best-effort-rollback exposure the `session/<id>` entry already shares); `onResolved` setting `postureChosen=false` after opt-in (functionally correct — the DB holds the true posture; the state only routes to the login screen); a redundant `encryptionPosture()` read on established-install boot (fires pre-hydration when `hasAccount` is still false regardless; a cheap local read); the read-failure fail-open to login (defensible, extremely rare, benign re-prompt); the three `rand` versions (all already resolved in `Cargo.lock` before this change — the `rand = "0.8"` edge added no crate); the `32`-char literal / `>= 32` test looseness (cosmetic); the "missing full `add_account`/`activate` round-trip test" (infeasible without a live homeserver — the Phase-A network probe precedes the passphrase decision — and the spec explicitly scoped coverage to the helper + Keychain-entry level, which exists); "Account" capitalization (correct per the established UX-DR10 glossary; the spec's example copy was explicitly non-canonical); corrupted-posture-value → off (fail-safe toward FileVault, needs DB corruption); a benign setState-after-unmount switch race; a resolved-`null` posture showing FileVault (honest — unchosen means not encrypted); concurrent `set_encryption_posture` vs. `add_account` (no concurrent posture-change UI in this story); and an astronomically-unlikely ULID collision.

## Auto Run Result

Status: **done**

**Summary.** Implemented the opt-in, first-run at-rest encryption choice for the per-account matrix-sdk-sqlite stores (NFR-10, AD-22), default off (FileVault posture). An app-wide posture (`sdk_encryption` = `on`/`off`) is persisted in a new `settings` key/value table in `keeper.db`. On a fresh install (zero accounts, posture unchosen) `App` shows a first-run choice **before** the first login; the posture is read back inside the single `add_account` command, which — when on — generates a random per-account passphrase (`rand`, 32 alphanumeric), stores it **only** in the Keychain at `store_passphrase/<id>`, and builds the SDK store with it. `activate` re-opens each store with `Some(passphrase)` iff that Keychain entry exists (self-describing; no registry schema change), and `rollback`/`sign_out_cleanup` delete it. A read-only Settings → Archive & Storage dialog states honestly that `keeper.db`/`archive.db` are not passphrase-encrypted and rely on FileVault, and reflects the SDK-store status. The passphrase never crosses IPC, is never logged, and is never written to disk outside the Keychain (NFR-9).

**Files changed (one-line each).**
- `src-tauri/crates/keeper-core/src/registry.rs` — added an idempotent `settings(key,value)` table + `get_setting`/`set_setting`.
- `src-tauri/crates/keeper-core/src/auth.rs` — `store_passphrase_keychain_key`, `SDK_ENCRYPTION_SETTING`, `get/set_encryption_posture`, `generate_store_passphrase`; wired the passphrase into `add_account` Phase B; extended `rollback` + `sign_out_cleanup` to delete (and now log on failure) the passphrase entry.
- `src-tauri/crates/keeper-core/src/account.rs` — `activate` fetches the per-account passphrase and threads it into `.sqlite_store`.
- `src-tauri/crates/keeper-core/Cargo.toml` — added `rand = "0.8"`.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `set_encryption_posture` / `encryption_posture` commands, registered.
- `src/lib/ipc/client.ts` — `setEncryptionPosture` / `encryptionPosture` wrappers.
- `src/components/settings/at-rest-encryption-choice.tsx` — first-run choice (default-off Switch, idempotency-guarded Continue with an error surface) + exported copy constants.
- `src/components/settings/settings-dialog.tsx` — read-only Archive & Storage dialog with a loading-safe SDK-store status.
- `src/App.tsx` — gates first-account onboarding behind the choice (loading / unchosen / chosen); addMode overlay unchanged.
- `src/components/layout/sidebar-pane.tsx` — the Settings button opens the dialog.
- Tests: new `at-rest-encryption-choice.test.tsx` (incl. failure-path), `settings-dialog.test.tsx`; extended `App.test.tsx`, `sidebar-pane.test.tsx`; Rust settings/posture/passphrase-cleanup tests.
- `_bmad-output/implementation-artifacts/deferred-work.md` — one deferred entry (honest "encryption key missing" restore state).

**Review findings breakdown.** intent_gap 0, bad_spec 0, patch 3 (all low: settings loading-state honesty; sign-out passphrase-delete warn log; Continue error surface + test), defer 1 (durable encrypted-flag + honest missing-key restore state), reject 14 (design-by-choice or pre-existing).

**Verification.** `bun run check` PASS (biome clean, tsc strict clean, vitest 215/215, keeper-core tauri-free guard PASS); `bun run check:rust` PASS (fmt + clippy `-D warnings`); `bun run test:rust` PASS (nextest 162/162); `cargo deny check` licenses/bans/sources OK (`rand` passes the license firewall — it was already resolved in `Cargo.lock`; the advisory failures are pre-existing GTK/webkit RUSTSEC from Tauri transitive deps, outside the license firewall).

**Residual risks.** The end-to-end "store is actually created encrypted and re-opens on relaunch" path needs a live SSS homeserver, so it is covered by the helper + Keychain-entry assertions plus the documented manual check, not an automated test. Losing the Keychain passphrase makes an encrypted account unrecoverable with a generic error and no honest state (deferred). A process kill in the narrow window between the passphrase `keychain_set` and the registry-row insert can orphan a `store_passphrase/<id>` entry (same best-effort exposure as the existing `session/<id>` entry; deferred as a crash-recovery-sweep idea).
