---
title: 'Archive Survives Sign-Out — and Deletes Only on Command'
type: 'feature'
created: '2026-07-05'
status: 'done'
review_loop_iteration: 1
baseline_revision: '805b1091a59e3a8817bf740b328a055a23a9c409'
final_revision: '6f52b003150cc55523d304c25802ceec216fc3e1'
followup_review_recommended: false
context: []
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** Sign-out must never silently destroy history. Today the default sign-out already keeps `archive.db` (only the SDK dir, Keychain, and registry row are deleted), but three things are missing to complete FR-6/FR-37: (1) the deliberate destructive "delete this Account's archive" path deferred from Story 2.5's dialog does not exist; (2) a per-account purge would corrupt the shared `events_fts` index because the external-content FTS table has no delete-maintenance path (bundled deferred-work); (3) the default dialog lacks the honest "unsynced content is not recoverable" caveat, and survival of FTS/Export past sign-out is unverified.

**Approach:** Keep the default keep-archive sign-out (unchanged `sign_out_cleanup`) and add its caveat copy. Add a deliberate, serialized per-account archive purge routed through the single archive writer that removes only the target account's `events` rows and its `events_fts` entries (via the FTS5 external-content `'delete'` command), exposed as a new `delete_account_archive` IPC command. Wire the destructive "…and delete this Account's archive" option into the sign-out dialog behind a type-the-account-identity confirmation. Log both paths ids-only. Verify FTS + Export survive a default sign-out.

## Boundaries & Constraints

**Always:**
- The archive purge routes through the single serialized archive writer (`ArchiveMsg` channel) — never a competing/second connection. Preserves the one-writer-owns-`archive.db` invariant.
- A purge touches ONLY the target `account_id`'s rows in `events` and `events_fts`; every other account's rows and FTS entries stay intact.
- FTS stays consistent: for each purged **indexed** row issue the external-content `'delete'` command (mirroring `index_body`'s skip of empty bodies) in the same transaction as the base-row `DELETE` — never orphan trigrams. This is the deferred FTS-maintenance path landing here.
- Default sign-out keeps `archive.db` untouched; FTS (`search`) and Export (`run_export`) keep working for the signed-out account with no active session (both are read-only `archive.db` reads keyed by `account_id`).
- Destructive delete is gated by an exact-match typed account identity (the identity string the dialog shows, trimmed). Both paths logged ids-only via `tracing`.
- `keeper-core` stays tauri-free; no `.unwrap()`/bare `.expect()` in production paths; new command args/VM cross IPC via ts-rs (`bindings:check` reflects it).

**Block If:**
- A per-account purge cannot be scoped to one account without deleting or reindexing another account's `events`/`events_fts` data. (It can — the external-content `'delete'` command scopes to the target rowids; this is a safety tripwire, not an expected outcome.)

**Never:**
- Never touch other accounts' data; never do server-side logout (AD-10 stays local-only).
- Never delete any archive rows on the DEFAULT (keep) sign-out path.
- Never perform archive deletion outside the serialized writer.
- Never `'rebuild'` the whole `events_fts` index for a single-account purge — it re-indexes all accounts (slow at 100k+, and touches data out of scope). Use the scoped `'delete'` command.
- Never add `VACUUM`/`PRAGMA auto_vacuum` to `archive.db` — `events` has no `INTEGER PRIMARY KEY`, so a vacuum would renumber rowids and desync the FTS shadow tables (deferred-work trap).
- Never log message content.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Purge account with data | acctA rows incl. empty-body rows + edit version chains; acctB present | acctA `events` + `events_fts` entries removed; acctB rows + FTS untouched; `events_fts` `'integrity-check'` passes | No error expected |
| Purge account with no rows | account_id absent from `events` | Success no-op; other accounts untouched | No error expected |
| Empty-body rows | rows with `body = ''` (never FTS-indexed) | Base rows deleted; no `'delete'` issued for them (mirrors `index_body` skip) | No error expected |
| SQL failure mid-purge | e.g. transient lock/error during transaction | Transaction rolls back (no partial purge); error returned to caller via writer completion channel | Writer task keeps running; logged ids-only |
| Survival after default sign-out | acctA archived, then `sign_out_cleanup(acctA)` | `archive.db` acctA rows still present; `search` + `run_export` return them with no session | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/db.rs` -- schema (`events` PK `(account_id, event_id)`); add `delete_account_archive(conn, account_id)` (scoped FTS `'delete'` + `DELETE FROM events`).
- `src-tauri/crates/keeper-core/src/archive/fts.rs` -- `events_fts` external-content (`body`, trigram). NOTE two population paths with DIFFERENT empty-body behavior: `ensure_fts` does a one-time `'rebuild'` that indexes **every** row incl. empty/NULL bodies; the incremental `index_body` **skips** empty bodies. The purge must therefore gate the `'delete'` on actual index membership (`events_fts_docsize`), NOT on `body <> ''` (see Design Notes).
- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- `ArchiveMsg` enum + `ArchiveHandle` (`tx: UnboundedSender<ArchiveMsg>`); add `DeleteAccount` variant + `delete_account` method.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- serialized writer `run` loop (owns the `Connection`); add `DeleteAccount` handler.
- `src-tauri/crates/keeper-core/src/account.rs` -- `AccountManager` holds `archive: Option<ArchiveHandle>` (line ~211); `sign_out`/`shutdown` (~2294–2364). Add `delete_account_archive(&self, account_id)`.
- `src-tauri/crates/keeper-core/src/auth.rs` -- `sign_out_cleanup` (~720–761): deletes SDK dir + Keychain + registry row, NOT `archive.db` (unchanged; asserted by survival test).
- `src-tauri/crates/keeper/src/ipc.rs` -- `AppState { accounts }` (~40); add `#[tauri::command] delete_account_archive`.
- `src-tauri/crates/keeper/src/lib.rs` -- `generate_handler!` (~59–120); register `ipc::delete_account_archive`.
- `src/lib/ipc/client.ts` -- typed `invoke` wrapper; add `deleteAccountArchive(accountId)`.
- `src/components/layout/account-footer.tsx` -- `SignOutDialog` (~147–201): add caveat copy + destructive delete path with typed-identity confirm `Input`.
- `src/hooks/use-sign-out.ts` -- sign-out hook + `accountsStore.removeAccount`; extend for the delete-archive path.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/db.rs` -- add `delete_account_archive(conn, account_id) -> Result<(), ArchiveError>`: in ONE transaction opened with `BEGIN IMMEDIATE` (take the write lock up front, matching `ensure_fts`), issue the FTS5 external-content `'delete'` for **exactly the account's rows that are actually in the index**, then delete the base rows: `INSERT INTO events_fts(events_fts, rowid, body) SELECT 'delete', e.rowid, COALESCE(e.body, '') FROM events e WHERE e.account_id = ?1 AND e.rowid IN (SELECT id FROM events_fts_docsize)`, then `DELETE FROM events WHERE account_id = ?1`. **Do NOT gate on `body <> ''`** — that misses `'rebuild'`-indexed empty/NULL rows and orphans them (see Design Notes + Spec Change Log). Roll back on any error.
- [x] `src-tauri/crates/keeper-core/src/archive/mod.rs` -- add `ArchiveMsg::DeleteAccount { account_id: String, done: oneshot::Sender<Result<(), ArchiveError>> }` and `ArchiveHandle::delete_account(&self, account_id) -> Result<(), ArchiveError>` (send msg, await the oneshot). A closed send channel maps to a definite "writer stopped, not purged" error; a dropped completion sender (`RecvError`) maps to an **indeterminate** error ("could not confirm the archive was deleted"), since the writer may have committed before the task ended — do not assert the archive survives.
- [x] `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- handle `DeleteAccount` in `run`: call `db::delete_account_archive`, send the `Result` on `done`; never panic (writer keeps running on error).
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- add `AccountManager::delete_account_archive(&self, account_id) -> Result<(), CoreError>`: clone `self.archive` and await the purge; `archive: None` (archive disabled) returns `Ok(())` (nothing to purge). Log ids-only.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- add `#[tauri::command] async fn delete_account_archive(state, account_id: String) -> Result<(), IpcError>` delegating to `state.accounts`; `tracing` ids-only; map via `to_ipc_error`.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- register `ipc::delete_account_archive` in `generate_handler!`.
- [x] `src/lib/ipc/client.ts` -- add `deleteAccountArchive(accountId: string): Promise<void>` calling `invoke("delete_account_archive", { accountId })`.
- [x] `src/components/layout/account-footer.tsx` -- `SignOutDialog`: add the default caveat sentence (content never synced-and-decrypted before sign-out is not recoverable). Add a destructive option "…and delete this Account's archive"; its **arming control is a secondary/non-destructive button** (destructive styling is reserved for the actual confirm), and arming is **reversible** (a control returns to the keep-archive choice without closing the dialog). Once armed, the dialog **title and description switch to a destructive framing** (must NOT still say "keep local archive" / "stays on this Mac"). The confirm stays disabled until an `Input` matches the account identity exactly (trimmed). On confirm: call the hook's delete-archive path. A purge failure must surface a **persistent error on a surface that survives the shell unmount** (see hook task), never only dialog-local state — because deleting the last account unmounts the dialog.
- [x] `src/hooks/use-sign-out.ts` -- support the delete-archive path, keeping the default keep-archive path unchanged. On the delete path: sign out, remove the account, then purge (removal before purge so a purge failure never rolls back the completed sign-out). A purge failure must NOT be swallowed when it was the **last** account (removing it unmounts the shell + dialog): deliver the failure via a surface that outlives the unmount — a global toast/persistent error store (check for an existing toast system, e.g. `sonner`/`useToast`; if none, a small module-level error surface). Word it as the archive-deletion failing while the sign-out completed (retriable), not as a rollback.
- [x] Tests -- `archive/db.rs` unit tests: (1) purge scoping across two accounts incl. version-chain rows AND **empty/NULL-body rows indexed via the `'rebuild'` path** (open the DB so `ensure_fts` rebuilds, or otherwise populate the index the rebuild way — NOT via `index_body`, which would hide the orphan), asserting after purge that `events_fts_docsize` has **no orphaned id** for the purged account, `'integrity-check'` passes, the purged account returns no search hits, and a surviving account still does; (2) absent-account no-op; (3) an incremental empty-body row (skipped by `index_body`, so NOT in the index) is NOT `'delete'`-issued and the purge still succeeds. `ingest.rs` writer test (`DeleteAccount` resolves the oneshot `Ok(())`, rows gone). `keeper-core` integration test `tests/archive_survives_sign_out.rs` (survival: after `sign_out_cleanup`, SDK dir + keychain gone but `archive.db` rows persist and `search` returns them, no session). `account-footer.test.tsx` + `use-sign-out.test.ts` (destructive button gated by typed identity; armed title/description are destructive-framed; arming reversible; confirm runs the delete path in order; last-account purge failure surfaces a non-dialog-local error; default path never purges).

**Acceptance Criteria:**
- Given the sign-out dialog, when the user signs out with the default option, then the SDK store and Keychain entries are deleted, `archive.db` is untouched, and FTS + Export over that account's history still work with no active session (FR-37, FR-6).
- Given the default dialog copy, then it states that content never synced-and-decrypted before sign-out is not recoverable.
- Given the destructive option, when the user selects it, then the confirm button stays disabled until the Account identity is typed exactly, and confirming deletes only that Account's `events` rows and `events_fts` entries — other Accounts' data untouched (FR-6, UX-DR20, AD-10).
- Given either path, then the action is logged (ids only) and the account switcher updates immediately (Zustand `removeAccount` fires subscribers synchronously).
- Given the shared `events_fts` after a per-account purge, then `INSERT INTO events_fts(events_fts) VALUES('integrity-check')` succeeds and searches for surviving accounts still return their hits (no index drift).

## Spec Change Log

### 2026-07-05 — bad_spec loopback (review pass 1)
- **Triggering finding:** per-account purge used `... WHERE account_id = ?1 AND body <> ''` for the FTS `'delete'`, per the original Tasks/Design-Notes instruction and its "mirrors `index_body`'s empty-body skip" rationale. That rationale was wrong: `ensure_fts` populates the index on fresh creation via `'rebuild'`, which indexes empty/NULL bodies too, so `body <> ''` left orphaned `events_fts_docsize` entries for the purged account (confirmed with a SQLite trigram probe; integrity-check still passed, so the original test was blind to it). Reviewers also found the last-account purge-failure error was swallowed (shell unmounts before the await resolves) and the armed dialog copy still said "keep local archive."
- **Amended:** Tasks + Design Notes + Code Map (all outside `<intent-contract>`) — the purge now gates the `'delete'` on actual index membership (`rowid IN (SELECT id FROM events_fts_docsize)`, `COALESCE(body,'')`), uses `BEGIN IMMEDIATE`, maps a dropped ack to an indeterminate error, requires the armed dialog to switch to destructive framing with a secondary/reversible arming control, and requires last-account purge failures to surface on a non-dialog surface. Tests must exercise the `'rebuild'`-indexed empty/NULL case and assert no orphaned docsize rows.
- **Known-bad state avoided:** a silent, integrity-check-passing FTS index drift (orphaned zero-term docsize rows for a purged account) that risks corruption on later rowid reuse; a last-account delete failure that leaves the archive on disk while telling the user nothing.
- **The `<intent-contract>` is unchanged:** its binding invariant ("only the target account's rows removed; FTS index stays consistent; never orphan trigrams") was already correct — only the non-contract mechanism ("how") was wrong. The I/O matrix's "Empty-body rows" parenthetical ("never FTS-indexed" / "mirrors `index_body` skip") is a non-binding descriptive hint superseded by the corrected Design Notes; the row's binding behavior (base rows deleted, no orphans) still holds.
- **KEEP (must survive re-derivation):** the single-serialized-writer routing via a new `ArchiveMsg::DeleteAccount` carrying a `oneshot` completion channel; `ArchiveHandle::delete_account` awaiting it; `AccountManager::delete_account_archive` with the `archive: None` no-op; the `#[tauri::command] delete_account_archive` + handler registration; the `deleteAccountArchive` client wrapper; the dialog's exact-trim identity gate, `handleOpenChange` state reset, and per-scenario multi-account scoping; the survival integration test asserting `sign_out_cleanup` leaves `archive.db` intact and searchable; sign-out-before-purge ordering with removal before purge. All of these traced clean in review — re-derive them as-is and change only what the amendments above require.

## Review Triage Log

### 2026-07-05 — Review pass 1 (bad_spec loopback)
- intent_gap: 0
- bad_spec: 7: (high 0, medium 3, low 4)
- patch: 0
- defer: 1
- reject: 5
- addressed_findings:
  - `[medium]` `[bad_spec]` **FTS orphan: `body <> ''` misses `'rebuild'`-indexed empty/NULL rows.** Confirmed empirically; amended to gate the `'delete'` on `events_fts_docsize` membership. (See Spec Change Log.)
  - `[medium]` `[bad_spec]` **Last-account delete-archive failure silently swallowed** (shell unmounts before the purge await resolves; `setError` on an unmounted dialog). Amended: deliver the failure on a surface that survives the unmount.
  - `[medium]` `[bad_spec]` **Armed dialog copy dishonest** ("Sign out, keep local archive" / "stays on this Mac" while armed to delete). Amended: switch title/description to destructive framing when armed.
  - `[low]` `[bad_spec]` **Arming control styled destructive but only reveals the confirm field.** Amended: secondary/non-destructive arming control.
  - `[low]` `[bad_spec]` **Deferred `BEGIN` vs sibling `ensure_fts`'s `BEGIN IMMEDIATE`.** Amended: `BEGIN IMMEDIATE`.
  - `[low]` `[bad_spec]` **Writer-dropped ack reported as definite failure** though the purge may have committed. Amended: indeterminate ("could not confirm") wording.
  - `[low]` `[bad_spec]` **No way to disarm the destructive path** except Cancel+reopen. Amended: arming is reversible.
- deferred (1): no UI entry point to purge a *leftover* archive for an already-signed-out account (a failed/interrupted purge leaves rows on disk with no later retry surface) — belongs to a future Archive-management surface.
- rejected (5): typed-identity gate defeatable by copy-paste (the story requires only that the account name be typed; met); keep-archive sign-out-failure closes the dialog silently (pre-existing, out of scope); dropped `Clone/PartialEq/Eq` derives on `ArchiveMsg` (required for the move-only oneshot; workspace compiles + clippy/tests green); discarded `ROLLBACK` result (standard idiom; a failing ROLLBACK is extreme); cross-account/rapid-click concurrency (safety rests on the single-writer FIFO, which holds).

### 2026-07-05 — Review pass 2 (post-loopback, no further loopback)
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 0, low 4)
- defer: 0
- reject: 12
- addressed_findings:
  - `[low]` `[patch]` **Empty-`userId` could enable the destructive confirm with no typed confirmation.** `identityMatches` was `typedIdentity.trim() === userId`; a degenerate empty `userId` would match an empty field. Guarded with `userId.length > 0 && …`.
  - `[low]` `[patch]` **"You can retry later" toast was not actionable** — after a purge failure the account row is gone and there is no retry surface. Replaced with a toast carrying an actual **Retry** action that re-invokes the purge (keyed by `accountId`, session-free) and re-offers itself on repeat failure, so the promise is fulfillable.
  - `[low]` `[patch]` **Double failure logging.** The writer (`ingest.rs`) and `AccountManager` both `warn!`-logged the same purge failure. Downgraded the writer-context line to `debug!` (kept for the dropped-receiver case); `AccountManager`'s `warn!` is now the single ids-only audit line.
  - `[low]` `[patch]` **Misleading dialog-`error` comment** claimed it covered non-last-account purge failures; it only covers sign-out failures (purge failures are toast-surfaced and the row unmounts first). Comment corrected.
- rejected (12): FTS `'delete'` `COALESCE(body,'')` content-mismatch (NOT reachable — `backfill_missing_bodies` runs before `ensure_fts`'s rebuild and `body` is never mutated after indexing; edits append new rows, redaction only sets `redacted_ts`; independently confirmed handled); post-sign-out `Insert` re-creating purged rows (NOT reachable — `shutdown` removes the archive handler + stops sync before `sign_out` returns, and the writer FIFO purges any earlier-enqueued `Insert`); re-add-during-purge "data loss" (NOT reachable — accountIds are per-add ULIDs, so a re-added account has a new id and the purge targets only the old one); swallowed `ROLLBACK` error (standard idiom; ROLLBACK failing is pathological/IOERR); `BEGIN IMMEDIATE` `SQLITE_BUSY` under checkpoint (5 s busy-timeout mitigates); `userId`-vs-`accountId` confirm on duplicate userIds (very narrow); dropped enum derives (compiles green); `archive: None` silent-Ok honesty (degenerate: `archive.db` unopenable at startup); "entire local archive" copy vs media (events + `events_fts` IS the whole per-account `archive.db` footprint; SDK media cache goes with the sdk dir on sign-out); no Rust test for the error-forwarding path (happy/no-op + frontend rejection covered); `App.tsx` `renderContent` hoisting (style); disarm not resetting `signingOut` (guarded by `disabled`, latent only).

## Design Notes

**FTS external-content delete (golden pattern) — gate on index membership, NOT on `body`.** `events_fts` is `content='events'`, so a plain `DELETE FROM events` orphans trigrams and can trip `SQLITE_CORRUPT_VTAB` on a later `'rebuild'`/`'integrity-check'`. To purge one account we must issue the FTS5 external-content `'delete'` for **exactly the account's rows that are in the index** — no more (deleting a never-indexed rowid corrupts the index), no fewer (skipping an indexed rowid orphans it). The two index-population paths disagree on empty bodies: `ensure_fts`'s one-time `'rebuild'` indexes **every** row incl. empty/NULL bodies (each gets an `events_fts_docsize` entry), while the incremental `index_body` **skips** empty bodies. So `body <> ''` is WRONG — it orphans `'rebuild'`-indexed empty/NULL rows. Gate on the actual index instead (one transaction, before the base delete):

```sql
BEGIN IMMEDIATE;
INSERT INTO events_fts(events_fts, rowid, body)
  SELECT 'delete', e.rowid, COALESCE(e.body, '')
  FROM events e
  WHERE e.account_id = ?1
    AND e.rowid IN (SELECT id FROM events_fts_docsize);
DELETE FROM events WHERE account_id = ?1;
COMMIT;
```

`events_fts_docsize` holds one row per indexed document (the codebase already reads it in `db.rs`), so `rowid IN (SELECT id FROM events_fts_docsize)` selects exactly what the index holds regardless of how it got there; `COALESCE(e.body,'')` supplies the indexed content (`body` is never mutated in place — edits append new rows, `mark_redacted` only sets `redacted_ts`). This purges exactly one account and never `'rebuild'`s (which would re-index every account). Empirically verified: this leaves zero orphaned docsize rows and passes `'integrity-check'`, while `body <> ''` left orphans for rebuilt empty/NULL rows.

**Completion signal.** The existing `Insert`/`Redact` messages are fire-and-forget, but a destructive purge must report its outcome so the IPC command, switcher update, and audit log happen honestly. The `DeleteAccount` message carries a `oneshot::Sender`; `delete_account` awaits it through the same serialized writer (no second connection). A dropped completion sender (`RecvError`, e.g. the writer task ended on shutdown after committing) is **indeterminate**, not a definite "not deleted" — surface it as "could not confirm," never as an assertion the archive survives.

**Dialog honesty when armed.** The default framing ("Sign out, keep local archive" / "your local archive stays on this Mac") is a lie once the destructive path is armed. When armed, switch the title + description to a destructive framing; make the arming control a secondary (non-destructive) button and reversible; reserve destructive styling for the actual confirm.

**Ordering + last-account error visibility.** Sign out first, then remove the account, then purge — so a purge failure never rolls back the completed sign-out. But removing the **last** account empties the accounts store and unmounts the whole shell (`App.tsx` gates on `accounts.length`), taking `SignOutDialog` with it: a subsequent purge rejection cannot be shown via dialog-local `setError` (the component is gone). Deliver a delete-failure via a surface that outlives the unmount (a global toast / persistent error store), worded as "signed out; the archive could not be deleted (retriable)."

**Bundled deferred-work:** this story is where the `events_fts` delete-maintenance path lands (previously reserved for 5.7). The VACUUM/rowid-stability trap stays out of scope and is honored by the "never add VACUUM" boundary.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` + `clippy --all-targets -- -D warnings` clean; `keeper-core` stays tauri-free; no `.unwrap()`.
- `bun run test:rust` -- expected: cargo-nextest green incl. new purge-scoping, writer `DeleteAccount`, and sign-out-survival tests.
- `bun run check` -- expected: biome + tsc + vitest green incl. the `SignOutDialog` test.
- `bun run check:all` -- expected: full gate green incl. `tauri build --no-bundle`; `bindings:check` reflects the new `delete_account_archive` command args (no other VM/schema drift).

**Manual checks (real accounts):**
- Sign out with the default option → archive stays; open global search / start an export for that signed-out account → both still work offline. Choose "…and delete this Account's archive" → button enables only after typing the account identity; confirm → that account's search hits and export vanish while another account's history is unaffected; switcher updates immediately.

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.7 — sign-out keeps the local archive by default and deletes it only on a deliberate, per-account command (FR-37, FR-6 completion; UX-DR20; AD-10). The default sign-out is unchanged (`sign_out_cleanup` deletes only the SDK dir + Keychain + registry row, never `archive.db`), so FTS search and Export keep working for a signed-out account with no session — asserted by a new survival integration test. The destructive path is a new serialized per-account purge routed through the single archive writer (`ArchiveMsg::DeleteAccount` + `oneshot` completion), removing only that account's `events` rows and `events_fts` entries and leaving every other account intact. Exposed as a new `delete_account_archive` IPC command and wired into the sign-out dialog behind a type-the-account-identity confirmation. This story is where the previously-deferred `events_fts` delete-maintenance path lands.

**Files changed:**
- `src-tauri/crates/keeper-core/src/archive/db.rs` — `delete_account_archive(conn, account_id)`: one `BEGIN IMMEDIATE` transaction issuing the FTS5 external-content `'delete'` gated on `events_fts_docsize` membership (`COALESCE(body,'')`) then `DELETE FROM events`, rolling back on error; +3 tests (rebuild-path empty/NULL scoping with a no-orphan-docsize + integrity-check assertion, absent-account no-op, incremental-empty handling).
- `src-tauri/crates/keeper-core/src/archive/mod.rs` — `ArchiveMsg::DeleteAccount { account_id, done }` (derives reduced to `Debug` for the move-only oneshot); `ArchiveHandle::delete_account` (closed send → definite "not purged"; `RecvError` → indeterminate "could not confirm"); `log_dropped` arm.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` — writer `DeleteAccount` arm (delegates to db, forwards the `Result` on `done`, never panics; writer-context failure at `debug!`); +writer test.
- `src-tauri/crates/keeper-core/src/account.rs` — `AccountManager::delete_account_archive` (clone+await; `archive: None` → ids-only log + `Ok(())`; single audit `warn!` on failure). `sign_out`/`sign_out_cleanup` untouched.
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `#[tauri::command] delete_account_archive` + handler registration; ids-only tracing.
- `src-tauri/crates/keeper-core/tests/archive_survives_sign_out.rs` (new) — survival: after `sign_out_cleanup` the SDK dir + keychain + registry row are gone but `archive.db` rows persist and `archive::search` returns them with no session.
- `src/lib/ipc/client.ts` — `deleteAccountArchive(accountId)` wrapper.
- `src/App.tsx` — mounted `<Toaster/>` above the `hasAccount` gate so a purge-failure toast survives the last-account shell→login unmount.
- `src/hooks/use-sign-out.ts` — delete-archive path (sign out → remove account → purge); purge failure surfaced via a toast with an actionable **Retry**; default path unchanged.
- `src/components/layout/account-footer.tsx` — default caveat copy; secondary/reversible arming control; destructive title+description when armed; `variant="destructive"` confirm gated on trimmed-equals `userId` (with an empty-`userId` guard).
- Tests: `src/hooks/use-sign-out.test.ts`, `src/components/layout/account-footer.test.tsx`.

**Review:** 2 passes. Pass 1 → 1 bad_spec loopback (7 findings; 3 medium, 4 low): the FTS `'delete'` gated on `body <> ''` orphaned `'rebuild'`-indexed empty/NULL rows (silent index drift; empirically confirmed) — corrected to gate on `events_fts_docsize` membership; plus last-account failure visibility (App-root toast), armed-dialog honesty, secondary/reversible arming, `BEGIN IMMEDIATE`, indeterminate-ack wording. 1 deferred (no persistent retry surface for a leftover archive), 5 rejected. Pass 2 (post-loopback, re-derived code) → no loopback: 4 low patches (empty-`userId` guard, actionable Retry toast, deduped failure log, corrected comment), 0 defer, 12 rejected (incl. two hypothesized FTS/writer correctness issues verified NOT reachable). `followup_review_recommended: false` — the final pass made only localized low-consequence fixes.

**Verification (independently re-run, all green):** `bun run check:rust` — PASS (fmt + clippy `--workspace --all-targets -D warnings`; keeper-core tauri-free; no `.unwrap()`). `bun run test:rust` — PASS (415/415, incl. the 3 db purge tests, the writer `DeleteAccount` test, and the survival integration test). `bun run check` — PASS (biome + tsc + vitest 599/599; core-tauri-free guard). `bun run check:all` — PASS (incl. `bindings:check` — no ts-rs drift; the command args are a plain `{ accountId }`/void — and `tauri build --no-bundle`).

**Residual risks:** No persistent UI surface to delete a *leftover* archive for an already-signed-out account if the purge fails and the toast is dismissed (deferred; the toast Retry mitigates during its lifetime). The FR-37 user-visible flow (default sign-out then offline search/export; destructive delete then confirm scoping) is covered by automated tests at the unit/integration layer; real-homeserver manual confirmation remains OQ-1 / Epic 11 territory.
