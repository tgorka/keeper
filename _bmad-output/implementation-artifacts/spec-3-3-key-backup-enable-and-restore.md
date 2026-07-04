---
title: 'Key Backup — Enable and Restore'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: '789015d344fda5e6ae2f2303b3493cbd4a5f9896'
final_revision: '8fef7b84846dbea1e44f53b75ca1bc44817d8b70'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 3.1 renders UTD stubs ("verify this device or restore key backup") and 3.2 added device verification, but there is still no way to **restore encrypted history on a fresh login** nor to **enable server-side key backup**. An account whose other sessions have a backup lands `Incomplete` with locked history and no recovery path; an account with no backup can never protect its keys. The Settings Encryption section shows verification state only — backup is invisible.

**Approach:** Add server-side key backup enable + restore, confined to `keeper-core`, on matrix-sdk 0.18's high-level `client.encryption().recovery()` API (no low-level `backups()`/`secret_storage()` mixing, no new crate deps). A new `keeper-core::backup` module owns every recovery SDK call: a per-account status stream mapping `RecoveryState` → a typed `BackupStatus`, and one-shot actions `enable` (returns the base58 recovery key once) and `restore` (`recover(key)`, mapping invalid-key errors to *named* codes). The React layer adds a per-account backup row to Settings and an Element-X-style modal with two modes: **enable** (show the recovery key exactly once in `mono`, with an explicit "save this" step and optional Keychain save) and **restore** (paste key → named inline error on a bad key → history re-renders via 3.1's existing streams on success). On restore, room keys download automatically and 3.1's encryption/timeline streams re-render UTD rows — no new re-render code.

## Boundaries & Constraints

**Always:**
- All recovery/backup SDK calls, `RecoveryState`, secret-storage/backup key material, and error inspection live in `keeper-core::backup`; the webview receives only a typed `BackupStatus` and named error codes (AD-1 / NFR-9).
- **The one sanctioned key-material exception:** the base58 **recovery key** string returned by `enable()` crosses IPC *because it is meant for the human to save* (FR-14 / epic UX: shown once in `mono`). It is displayed once, never written to `tracing`, never persisted in a JS store beyond the open modal's lifecycle (cleared on close), and only re-crosses IPC when the user opts to save it to the OS Keychain or when prefilling restore from the Keychain. No other key, secret, or plaintext crosses the boundary.
- Use the SDK's native recovery flow and Element-X-style patterns; do not invent novel crypto UX. Recovery keys and codes render in `mono`.
- Invalid recovery keys produce **named** inline errors, not a generic failure: distinguish a malformed key (decode failure) from a well-formed-but-wrong key (MAC failure), each mapped to its own `IpcErrorCode`.
- Enable that races an existing server backup must surface a named "backup already exists — restore instead" state, never a generic error (guard/catch `RecoveryError::BackupExistsOnServer`).
- Backup status is sourced from the Rust core (`recovery().state_stream()`), mirroring 3.1's `subscribe_encryption_status` subscription lifecycle (lazy activate, supervised task in the shared `subscriptions` map, self-reap, abort on unsubscribe).
- Reuse 3.1's encryption-status + timeline streams to re-render decrypted history after restore — do not add parallel re-render logic.

**Block If:**
- matrix-sdk 0.18's `Recovery::enable()` provides no way to surface the recovery key string exactly once (contradicting the researched API) — the "show the key once" acceptance path is then unbuildable.
- Enabling backup/secret-storage requires an interactive UIA (user-interactive auth) re-authentication that the current keeper session cannot satisfy unattended and matrix-sdk exposes no non-interactive path — the enable leg is then unbuildable as specified.

**Never:**
- No low-level `client.encryption().backups()` / `secret_storage()` orchestration or hand-rolled secret-storage layering — the module docs forbid mixing layers; go through `recovery()` only.
- No new crate dependencies (recovery is already under the enabled `e2e-encryption` feature) — do not touch `Cargo.toml` features or `cargo deny`.
- No recovery-key logging, no recovery key held in a persistent JS store, no key material in TypeScript beyond the display/save exception above; no `matrix-js-sdk`.
- Do not rewire the UTD stub or verify banner away from opening Settings — the backup entry point lives in the Settings Encryption section for this story (mirrors 3.2).
- No cross-signing reset / identity reset / backup deletion / `recover_and_reset` — only enable-fresh and restore-with-key.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Status stream | Account live; `recovery().state_stream()` emits | `map_recovery_state`: `Enabled`→`Enabled`, `Disabled`→`Disabled`, `Incomplete`→`Incomplete`, `Unknown`→`Unknown`; Settings row renders "Backup on" / "Not set up" / "Needs your recovery key" / "Checking…" | Stream ends → producer self-reaps; account not live → subscribe activates it |
| Enable (no backup) | Status `Disabled`; user clicks "Set up backup" | `backup_enable` → `recovery().enable().await` returns base58 key; modal shows it once in `mono` + explicit "save this" acknowledgment; status stream → `Enabled` | Enable fails → `BackupError::RestoreFailed`/`Action` surfaced in modal, honest failed state |
| Enable races server backup | `enable()` returns `BackupExistsOnServer` | Named `BackupError::AlreadyExistsOnServer` → modal states "a backup already exists — restore instead" and offers the restore mode | — |
| Save to Keychain | User opts in after seeing the key | `backup_save_recovery_key` → `platform.keychain_set("recovery_key/<id>", key)` | Keychain write fails → surface inline; key stays visible so the user can copy it manually |
| Restore (Incomplete) | Fresh login, status `Incomplete`; user pastes a valid key | `backup_restore` → `recovery().recover(key).await` Ok; room keys download automatically; 3.1 streams flip status `Enabled` + re-render UTD rows; modal shows restored | — |
| Restore prefill | Restore modal opens; a saved key exists in Keychain | `backup_saved_recovery_key` → `Some(key)` prefills the textarea | `None` → empty textarea |
| Malformed recovery key | User pastes a non-decodable / wrong-length string | `recover` → `SecretStorageError::SecretStorageKey(_)` → `BackupError::MalformedRecoveryKey` → `IpcErrorCode::BackupMalformedKey` | Named inline: "That doesn't look like a recovery key." |
| Wrong recovery key | Well-formed key, MAC check fails | `recover` → `SecretStorageError::Decryption(Mac)` → `BackupError::IncorrectRecoveryKey` → `IpcErrorCode::BackupIncorrectKey` | Named inline: "Recovery key didn't match this account." |
| Other restore failure | Network / other `RecoveryError` | `BackupError::RestoreFailed(reason)` → `IpcErrorCode::BackupFailed` | Generic-but-honest failed state with reason |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- add `BackupStatus { Unknown, Disabled, Enabled, Incomplete }` (serde camelCase + `#[ts(export)]`, mirroring `EncryptionStatus` ~L157) and new `IpcErrorCode` variants `BackupMalformedKey`/`BackupIncorrectKey`/`BackupExists`/`BackupFailed`.
- `src-tauri/crates/keeper-core/src/backup.rs` -- **NEW** module: pure `map_recovery_state(&RecoveryState) -> BackupStatus` and `map_recover_error(&RecoveryError) -> BackupError`; `run_status_producer(client, sink, account_id)` over `recovery().state_stream()`; one-shot `enable(&Client) -> Result<String, CoreError>` (returns the base58 key; catch `BackupExistsOnServer`) and `restore(&Client, &str) -> Result<(), CoreError>`.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `BackupError { Unavailable(String), AlreadyExistsOnServer, MalformedRecoveryKey, IncorrectRecoveryKey, RestoreFailed(String), Action(String) }`; add `CoreError::Backup(#[from] BackupError)` (mirror `Verification` ~L233).
- `src-tauri/crates/keeper/src/ipc.rs` -- `to_ipc_error` arms mapping each `BackupError` variant to its `IpcErrorCode`; new commands (below).
- `src-tauri/crates/keeper-core/src/lib.rs` -- declare `pub mod backup;`.
- `src-tauri/crates/keeper-core/src/account.rs` -- add `BackupSink`, `subscribe_backup_status`/`unsubscribe_backup_status` (copy the `subscribe_encryption_status` lifecycle ~L674), and one-shot methods `backup_enable`/`backup_restore` (resolve via `client_for` ~L870, delegate to `backup`) + keychain methods `backup_save_recovery_key`/`backup_saved_recovery_key` (take `platform`, call `keychain_set`/`keychain_get` with key `recovery_key/<account_id>`).
- `src-tauri/crates/keeper/src/ipc.rs` -- commands `backup_status_subscribe(state, account_id, channel: Channel<BackupStatus>)`, `backup_status_unsubscribe`, `backup_enable(state, account_id) -> String`, `backup_restore(state, account_id, recovery_key)`, `backup_save_recovery_key(state, account_id, recovery_key)`, `backup_saved_recovery_key(state, account_id) -> Option<String>`; all via `to_ipc_error`.
- `src-tauri/crates/keeper/src/lib.rs` -- register all six commands in `invoke_handler!` (~L36).
- `src/lib/ipc/client.ts` -- `subscribeBackupStatus`/`unsubscribeBackupStatus`, `backupEnable(accountId): Promise<string>`, `backupRestore(accountId, recoveryKey)`, `backupSaveRecoveryKey(accountId, recoveryKey)`, `backupSavedRecoveryKey(accountId): Promise<string | null>`; re-export `BackupStatus` + regenerated `IpcErrorCode`.
- `src/lib/stores/key-backup.ts` -- **NEW** zustand store (mirror `stores/verification.ts` + `stores/encryption-status.ts`): `statuses: Record<string, BackupStatus>`, modal `{ open, mode: 'enable' | 'restore', accountId, recoveryKey: string | null, phase, error: IpcErrorCode | null }`, actions `setStatus`/`removeAccount`/`openEnable`/`openRestore`/`close` (clears `recoveryKey`).
- `src/hooks/use-key-backup-statuses.ts` -- **NEW** all-account subscriber mirroring `use-encryption-statuses.ts`: subscribe per account, forward batches to `setStatus`, gate late batches, teardown + `removeAccount` on account-set change/unmount.
- `src/components/settings/key-backup-dialog.tsx` -- **NEW** `Dialog` modal; **enable** mode: call `backupEnable`, show returned key once in `mono` (Copy + "Save to Keychain" + explicit "I've saved it" acknowledgment gating Done; warn it won't be shown again), map `BackupExists` to the restore offer; **restore** mode: `InputGroupTextarea` (`font-mono`) prefilled from `backupSavedRecoveryKey`, "Restore" → `backupRestore`, named inline error per `IpcErrorCode`, `restoring`/`restored`/`failed` states. Keyboard-operable, `Esc` closes.
- `src/components/settings/settings-dialog.tsx` -- in `EncryptionSection` (~L106) add a per-account backup line sourced from `useKeyBackupStatus(accountId)`: `Disabled`→"Set up backup" button (`openEnable`), `Incomplete`→"Restore" button (`openRestore`), `Enabled`→"Backup on", `Unknown`→"Checking…".
- `src/components/layout/app-shell.tsx` -- invoke `useKeyBackupStatuses()` and mount `<KeyBackupDialog/>` alongside the 3.1/3.2 wiring.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `BackupStatus { Unknown, Disabled, Enabled, Incomplete }` (camelCase + `#[ts(export)]`) and `IpcErrorCode` variants `BackupMalformedKey`, `BackupIncorrectKey`, `BackupExists`, `BackupFailed`. -- Typed status + named error codes the webview renders.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- Add `BackupError` enum and `CoreError::Backup(#[from] BackupError)`. -- Module error type.
- [x] `src-tauri/crates/keeper-core/src/backup.rs` -- New module. Pure `map_recovery_state(&RecoveryState) -> BackupStatus` (all four variants) and `map_recover_error(&RecoveryError) -> BackupError` (`SecretStorage(SecretStorageError::SecretStorageKey(_))`→`MalformedRecoveryKey`; `SecretStorage(SecretStorageError::Decryption(DecryptionError::Mac(_)))`→`IncorrectRecoveryKey`; `BackupExistsOnServer`→`AlreadyExistsOnServer`; other→`RestoreFailed(reason)`). `run_status_producer(client, sink, account_id)` emits `map_recovery_state` over `recovery().state_stream()` and self-terminates when the sink closes. `enable(&Client) -> Result<String, CoreError>` = `recovery().enable().await` returning the base58 key (map `BackupExistsOnServer` before generic). `restore(&Client, &str) -> Result<(), CoreError>` = `recovery().recover(key).await`, errors via `map_recover_error`. Never expose SDK objects across the boundary. -- The backup engine.
- [x] `src-tauri/crates/keeper-core/src/lib.rs` -- Add `pub mod backup;`. -- Wire module.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- Add `BackupSink`, `subscribe_backup_status`/`unsubscribe_backup_status` (copy the encryption-status lifecycle: lazy activate, supervised task in `subscriptions`, self-reap, abort on unsubscribe), one-shot `backup_enable`/`backup_restore` (resolve `client_for`, delegate to `backup`), and `backup_save_recovery_key(platform, account_id, key)`/`backup_saved_recovery_key(platform, account_id)` using `keychain_set`/`keychain_get` at `recovery_key/<account_id>`. -- Per-account lifecycle + action dispatch + keychain.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- Add `to_ipc_error` arms (`MalformedRecoveryKey`→`BackupMalformedKey`, `IncorrectRecoveryKey`→`BackupIncorrectKey`, `AlreadyExistsOnServer`→`BackupExists`, `Unavailable`/`RestoreFailed`/`Action`→`BackupFailed`) and the six commands (`backup_status_subscribe`/`unsubscribe`, `backup_enable`→`String`, `backup_restore`, `backup_save_recovery_key`, `backup_saved_recovery_key`→`Option<String>`), passing `state.platform` to the keychain ones. -- IPC surface + error mapping.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register all six commands in `invoke_handler!`. -- Wire commands.
- [x] `src/lib/ipc/client.ts` -- Add `subscribeBackupStatus`/`unsubscribeBackupStatus`, `backupEnable`, `backupRestore`, `backupSaveRecoveryKey`, `backupSavedRecoveryKey`; re-export `BackupStatus` + `IpcErrorCode`. -- Typed frontend IPC.
- [x] `src/lib/stores/key-backup.ts` -- New store: `statuses` map (+ `setStatus`/`removeAccount`) and modal state (`open`, `mode`, `accountId`, `recoveryKey`, `phase`, `error`) with `openEnable`/`openRestore`/`close` (clears `recoveryKey` on close). -- Drives the row + modal.
- [x] `src/hooks/use-key-backup-statuses.ts` -- All-account subscriber mirroring `use-encryption-statuses`; forward batches to `setStatus`, gate late batches, teardown + `removeAccount`. -- Single backup-status subscriber.
- [x] `src/components/settings/key-backup-dialog.tsx` -- New two-mode `Dialog`. Enable: `backupEnable` → show key once in `mono` (Copy, "Save to Keychain" → `backupSaveRecoveryKey`, explicit acknowledgment gating Done with a "shown once" warning), `BackupExists` → switch to restore. Restore: `font-mono` textarea prefilled from `backupSavedRecoveryKey`, Restore → `backupRestore`, named inline error keyed on `IpcErrorCode` (`BackupMalformedKey`/`BackupIncorrectKey` distinct from generic `BackupFailed`), distinct `restoring`/`restored`/`failed`. Keyboard-operable, labeled controls, `Esc` closes. -- The backup UI.
- [x] `src/components/settings/settings-dialog.tsx` -- In `EncryptionSection`, add a per-account backup line + action button chosen by `BackupStatus` (`Disabled`→Set up backup, `Incomplete`→Restore, `Enabled`→Backup on, `Unknown`→Checking…). -- Backup status + entry point (AC3).
- [x] `src/components/layout/app-shell.tsx` -- Invoke `useKeyBackupStatuses()` and mount `<KeyBackupDialog/>`. -- Wire subscriber + modal.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,backup.rs}` (tests) -- Unit tests: serde round-trip for `BackupStatus`; `map_recovery_state` across all four `RecoveryState` variants. For `map_recover_error`, test every variant whose SDK error is constructible in a unit test; where a variant isn't constructible (like 3.2's `CancelInfo`), assert what is and document the malformed-vs-wrong-key split as a manual second-session check. -- Verify pure mappings.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (tests) -- Assert `to_ipc_error` maps each `BackupError` variant to the expected `IpcErrorCode`. -- Lock the named-error contract.
- [x] `src/**` (tests) -- Colocated vitest/RTL (mock IPC like `use-encryption-statuses.test.ts`): store transitions (`openEnable`/`openRestore`/`close` clears `recoveryKey`); hook subscribe/forward/teardown; dialog enable renders the key once + save-to-keychain + acknowledgment, restore shows the *named* inline error distinctly for malformed vs wrong vs generic and prefills from a saved key; settings row shows the correct action per `BackupStatus`. -- Cover the I/O matrix edge cases.

**Acceptance Criteria:**
- Given an account without key backup, when the user enables it from Settings, then keeper creates the server-side backup and displays the recovery key exactly once in `mono` with an explicit "save this" step, and the key is storable in the OS Keychain at the user's choice (FR-14).
- Given a fresh keeper login on an account with an existing backup (status `Incomplete`), when the user restores with a valid recovery key, then historical encrypted messages decrypt after restore and previously-undecryptable rows re-render via 3.1's streams; an invalid key produces a *named* inline error (malformed vs wrong distinguished), never a generic failure (FR-14).
- Given backup state, then Settings shows the current per-account backup status (enabled / not set up / needs recovery key / checking) sourced from the Rust core, with no key material or plaintext crossing into JS except the human-facing recovery key on the enable/save/prefill paths.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass and the ts-rs bindings (`BackupStatus`, updated `IpcErrorCode`) regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 1: (high 0, medium 0, low 1)
- reject: 23: (high 0, medium 0, low 23)
- addressed_findings:
  - `[low]` `[patch]` The restore textarea did not clear the `failed` phase on edit, so the *named* inline error and its `aria-invalid` lingered while the user retyped a corrected key (a screen reader keeps announcing invalid). `key-backup-dialog.tsx` `onChange` now resets `phase` to `idle` (clearing the error) when it was `failed`, returning the field to neutral as the user corrects it.
  - `[low]` `[patch]` The restore Keychain prefill (`backupSavedRecoveryKey`) called `setValue(saved)` unconditionally, so a slow Keychain read could clobber a key the user had already pasted/typed. Now guarded with a functional update (`current.length === 0 ? saved : current`) so a late prefill never overwrites user input.
  - `[low]` `[patch]` `backup_enable`/`backup_restore` resolved the live `Client` via the verification-oriented `client_for`, which returns `VerificationError::Unavailable` when the account is not live — mapping to a *verification* IPC code, contradicting this story's named-backup-error taxonomy. Added `client_for_backup`, which surfaces the already-defined `BackupError::Unavailable` (→ `backupFailed`) instead. (In practice unreachable — backup status is subscribed at app-shell mount and lazily activates the account before any enable/restore button is reachable — but the failure code is now honest regardless.)

## Design Notes

**Recovery, not backups/secret_storage.** matrix-sdk 0.18 exposes three layers; the module docs explicitly forbid mixing them. Use `client.encryption().recovery()` exclusively: `enable()` internally creates the backup **and** the secret store and returns the base58 secret-storage key (`Ok(String)`); `recover(key)` opens the secret store, imports secrets, and the backup transitions `Enabling → Downloading → Enabled`, downloading room keys automatically. No explicit `download_room_keys` call is needed for the restore leg.

**`RecoveryState` maps cleanly to honest UI.** `Enabled` (on), `Disabled` (not set up), `Incomplete` (backup exists on server but this device isn't connected — the fresh-login restore case), `Unknown` (crypto not synced yet — "Checking…"). keeper does not override `EncryptionSettings`, so `auto_enable_backups` is off: a fresh login with a server backup surfaces `Incomplete`, and enable is only offered when `Disabled`. The AC's "error" status is carried by the failed-command named error path, not a persistent status variant.

**Invalid-key errors are two distinct SDK variants.** `recover()` returns `RecoveryError`; invalid keys land in `RecoveryError::SecretStorage(SecretStorageError::…)`. A malformed/undecodable key → `SecretStorageKey(DecodeError)` (→ `MalformedRecoveryKey`); a well-formed but wrong key fails the MAC → `Decryption(DecryptionError::Mac(_))` (→ `IncorrectRecoveryKey`). Match these two explicitly with a `_ => RestoreFailed` catch-all (`SecretStorageError` is `#[non_exhaustive]`). This is what makes the error "named, not generic" per FR-14.

**The recovery key is the deliberate boundary exception.** Every other story keeps key material in Rust; here the recovery key *must* reach the user to be saved (that is its entire purpose). Constrain it tightly: returned only by `enable`, displayed once, never logged, cleared from the store on modal close, and re-crossing IPC only for the explicit Keychain save or restore prefill. Call this out so a reviewer doesn't read it as an NFR-9 violation.

**Re-render is free (as in 3.2).** After `recover()`, the SDK downloads room keys and flips `verification_state()`/`RecoveryState`; 3.1's encryption-status producer and timeline stream already re-render UTD rows and clear banners. Do not duplicate that — just let restore succeed.

## Verification

**Commands:**
- `bun run check` -- expected: Biome + tsc + vitest all green.
- `bun run check:rust` -- expected: rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- expected: cargo-nextest green; ts-rs regenerates `BackupStatus` + updated `IpcErrorCode` with no git drift.
- `cargo deny check` (from `src-tauri/`) -- expected: still green; this story adds no crate deps (recovery is under the already-enabled `e2e-encryption` feature).

**Manual checks (real second session, test credentials in 1Password):**
- On an account with no backup, enable from Settings; confirm the recovery key shows exactly once in `mono`, "Save to Keychain" persists it, and status flips to "Backup on"; confirm the same key works to restore in Element.
- On a fresh keeper login for an account with an existing backup (status "Needs your recovery key"), restore with the valid key; confirm historical encrypted messages decrypt and prior UTD stubs re-render.
- Confirm a malformed key and a well-formed-but-wrong key each show their own distinct named inline error; confirm the whole flow is operable with keyboard only.

## Auto Run Result

Status: done

**Summary:** Added server-side Matrix key backup — enable + restore — confined to `keeper-core`, on matrix-sdk 0.18's high-level `client.encryption().recovery()` API (no low-level `backups()`/`secret_storage()` mixing, no new crate deps). A new `keeper-core::backup` module owns every recovery SDK call: a per-account status producer mapping `RecoveryState` → a typed `BackupStatus` over `recovery().state_stream()`, and one-shot `enable` (returns the base58 recovery key once; catches `BackupExistsOnServer` → a named "restore instead" state) and `restore` (`recover(key)`, mapping invalid-key errors to *named* codes — malformed vs well-formed-but-wrong distinguished). Six Tauri commands (status subscribe/unsubscribe, enable, restore, keychain save/read) surface it. The React layer adds a per-account backup row to the Settings Encryption section and a two-mode Element-X-style modal: **enable** shows the recovery key once in `mono` with Copy + optional Keychain save + an explicit "I've saved it" acknowledgment gating Done; **restore** offers a `font-mono` textarea (prefilled from the Keychain), named inline errors per code, and distinct working/restored/failed states. On restore, the SDK downloads room keys automatically and 3.1's encryption-status + timeline streams re-render UTD rows — no new re-render code. The recovery key is the single sanctioned boundary exception (shown once, never logged, cleared from the store on close, re-crossing IPC only for keychain save / restore prefill).

**Files changed:**
- `src-tauri/crates/keeper-core/src/backup.rs` — NEW: `map_recovery_state`, `map_recover_error` (named malformed/incorrect/exists split), `run_status_producer`, one-shot `enable`/`restore`; unit tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `BackupStatus` enum + four `IpcErrorCode` variants (`BackupMalformedKey`/`BackupIncorrectKey`/`BackupExists`/`BackupFailed`); serde round-trip tests.
- `src-tauri/crates/keeper-core/src/error.rs` — `BackupError` enum + `CoreError::Backup`.
- `src-tauri/crates/keeper-core/src/{lib.rs,account.rs}` — module wiring; `subscribe/unsubscribe_backup_status` (encryption-status lifecycle), `backup_enable`/`backup_restore`, keychain `backup_save_recovery_key`/`backup_saved_recovery_key` at `recovery_key/<id>`; (review) `client_for_backup` for a named not-live error.
- `src-tauri/crates/keeper/src/{ipc.rs,lib.rs}` — 6 commands + `to_ipc_error` arms + error-mapping tests.
- `src/lib/ipc/client.ts` (+ `gen/BackupStatus.ts`, `gen/IpcErrorCode.ts`) — typed IPC wrappers + regenerated bindings.
- `src/lib/stores/key-backup.ts` — NEW modal + status store (clears `recoveryKey` on close); test.
- `src/hooks/use-key-backup-statuses.ts` — NEW all-account subscriber; test.
- `src/components/settings/key-backup-dialog.tsx` — NEW two-mode modal; (review) restore clears the stale error on edit and never clobbers user input on late prefill.
- `src/components/settings/settings-dialog.tsx` — per-account backup row + action button per status; tests.
- `src/components/layout/app-shell.tsx` — mounts the subscriber hook + modal.

**Review findings:** 3 patches applied (all low: restore stale-error/aria-invalid cleared on textarea edit; Keychain prefill no longer clobbers user-typed input; `client_for_backup` surfaces a named `BackupError::Unavailable` instead of a verification code). 1 deferred (the pre-existing spawn→register subscription-lifecycle race, shared with 2.1/2.5/3.1/3.2 — see deferred-work.md). 23 rejected — notably: the `other.to_string()` "secret-leak" concern (the SDK error Display impls carry no key/plaintext and the message is never shown to the user, which renders fixed copy per code); the malformed-vs-incorrect split being untested (the variant mapping is grounded in the actual 0.18 source and its payloads are unit-inconstructible — a spec-sanctioned manual check, mirroring 3.2's `CancelInfo`); reveal-gate on the recovery key (contradicts the AC's "shown once in `mono`"); optimistic "restored" copy (by-spec; honestly hedged, re-render via 3.1 streams); the status-flicker/swallowed-subscribe-failure hook behaviors (the deliberate mirror of the proven 3.1 pattern). No intent_gap, no bad_spec — the spec held up.

**Verification (all re-run green after the review patches):** `bun run check` (Biome + tsc + vitest: 319 passed), `bun run check:rust` (rustfmt + clippy `-D warnings`: clean), `bun run test:rust` (cargo-nextest: 205 passed; ts-rs regenerates `BackupStatus` + `IpcErrorCode` with no unexpected drift), `cargo deny check licenses` (ok — no new crate deps; only the benign unmatched-OpenSSL allowance warning). The pre-existing `cargo deny check advisories` finding (unmaintained gtk-rs GTK3 binding via Tauri) is unchanged and unrelated; the license firewall the spec gates on is green.

**Residual risks:** The headline malformed-vs-wrong-key named-error split and the full enable→show-once→restore→history-re-render round-trip are real-second-session manual checks, intentionally not run unattended — the two `SecretStorageError` payload variants aren't unit-constructible (their `DecodeError`/`MacError` have no public constructor, same as 3.2's `CancelInfo`), so the mapping is proven only at the source-grounded/`to_ipc_error` level. Enable may in some deployments trigger interactive UIA the current session can't satisfy unattended (spec Block-If) — not exercised here. The shared subscription spawn→register race is deferred. `followup_review_recommended: false` — the final pass made only three localized, low-consequence fixes (two UI/a11y, one practically-unreachable error-code mapping) with no security-semantics change; the meaningful remaining gate is human manual verification against a real second session (Element).
