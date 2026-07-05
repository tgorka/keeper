---
title: 'Durability Against Remote Rewrites + Edit History'
type: 'feature'
created: '2026-07-05'
baseline_revision: '15c0170d6a25a6f3d33df430da3ed9f84737f5ca'
final_revision: '6fe524baf8261d1b9733b643456ee34aeac76eea'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-5-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** Story 5.1 archives every synced `m.room.message` as an append-only row, but it does not link remote edits into version chains and does not record remote redactions. So the archive cannot show edit history, and there is no local-durability policy for remotely deleted content — the epic's core "the platform's rewrite loses to your archive" promise (FR-36, FR-11) is unrealized.

**Approach:** Extend `keeper-core::archive` (no new writer, no new DB) so edit events (`m.room.message` carrying an `m.replace` relation) are stored with their relation extracted into queryable columns, and remote redactions **mark** the target row (`redacted_ts`) without erasing it. Add archive read helpers for the version chain and for redaction-aware retrievability, an IPC-served edit-history query fed by the Local Archive, a clickable "Edited" → edit-history popover in the timeline, and a "Honor remote deletions locally" setting (default off) in Settings → Archive & Storage with the plain disclosure. The timeline view always honors redaction (stub) regardless of the setting.

## Boundaries & Constraints

**Always:**
- One `archive.db`, one serialized writer, keyed by `(account_id, event_id)`, `INSERT OR IGNORE`, WAL — all 5.1 invariants hold. All new writes (edit rows, redaction marks) funnel through the single writer task via the existing channel.
- Edits: an `m.room.message` whose content carries `m.relates_to { rel_type: "m.replace", event_id: <target> }` is archived as its own row with `relates_to_event_id` + `rel_type` populated; the original row is never mutated. The version chain for target `E` = the row `E` plus all rows with `relates_to_event_id = E AND rel_type = 'm.replace'`, ordered by `origin_ts` ascending.
- Redactions **mark, never erase**: a remote redaction sets `redacted_ts` on the target row (if present); `content_json`/`media_json` are retained physically in all cases.
- "Honor remote deletions locally" is a read-time policy only (app-wide `settings` KV in `keeper.db`, values `"on"`/`"off"`, absent ⇒ off ⇒ preserve): retrievability read helpers return no content for a redacted row when the setting is on. This affects only this Mac.
- The timeline view honors redaction unconditionally (existing `TimelineItemVm::Redacted` stub) — independent of the setting.
- Edit-history popover is fed by the **Local Archive** (`archive.db`), never by a fresh homeserver fetch. Resolve the timeline item's opaque `item_key` → `event_id` via the live `Timeline` (the `send.rs` pattern: `items()…find(unique_id == item_key).as_event().event_id()`), then read the chain from the archive.
- Rust owns all Matrix logic; keeper-core stays tauri-free; no `.unwrap()`/bare `.expect()` in production paths; `?` + `thiserror`; `tracing` with ids not content; new IPC types are `Vm`-suffixed, serde camelCase, `#[ts(export)]` into `src/lib/ipc/gen/`.
- Schema migration is idempotent for pre-existing 5.1 `archive.db` files: add missing columns via `PRAGMA table_info` check + `ALTER TABLE … ADD COLUMN` (all new columns nullable), plus `CREATE INDEX IF NOT EXISTS`.

**Block If:**
- Capturing account-wide redaction events, or extracting the `m.replace` relation from message content, is not achievable through `matrix-sdk`/`ruma` without copying AGPL/GPL code or violating the "Rust owns Matrix" / "media never crosses IPC as bytes" invariants.

**Never:**
- No physical erasure of archived content on redaction or on toggling the setting (contradicts "mark, but do not erase"); honoring is retrieval-gating only, and flipping the toggle is **not** retroactive (documented). No destructive migration of existing rows.
- No FTS/search UI (5.3/5.4), no export (5.5), no archive-first pagination (5.6), no sign-out/delete-archive path (5.7).
- No re-archiving of events that were UTD at sync time and later re-decrypt (ingestion-completeness, not remote-rewrite durability — remains deferred; see Design Notes). No back-paginated history capture.
- No new writer/connection for writes; no second `archive.db`. No passphrase encryption (FileVault-only posture per epic).
- No event_id exposed to TypeScript — the frontend passes only `item_key`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Remote edit synced | `m.room.message` with `m.replace` → target `E` | New row stored; `relates_to_event_id=E`, `rel_type='m.replace'`; original row `E` unchanged | Missing/parse relation → store as plain message (no relation cols) |
| Edit-history query | `item_key` for an edited message with 2 prior edits | Ordered `Vec<EditVersionVm>` (original + edits by `origin_ts` asc), last flagged `is_current` | Item unresolvable or chain absent in archive → empty vec |
| Remote redaction synced | redaction targeting archived event `E` | Row `E` gets `redacted_ts`; `content_json` retained | Target not in archive → 0-row `UPDATE`, no error |
| Retrieve redacted, honor OFF | `retrievable_content(E, honor=false)`, `E` redacted | Returns the row incl. pre-redaction content | — |
| Retrieve redacted, honor ON | `retrievable_content(E, honor=true)`, `E` redacted | Returns `None` (not retrievable) while row stays on disk | — |
| Reopen pre-5.1 archive.db | existing DB lacking new columns | Migration adds columns/index idempotently; old rows readable | Re-run is a no-op |
| Toggle setting | user flips "Honor remote deletions locally" | Persisted to `keeper.db` `settings`; affects subsequent reads only | Read/write failure → surfaced as `IpcError`, toggle reverts |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/db.rs` -- add `relates_to_event_id TEXT`, `rel_type TEXT`, `redacted_ts INTEGER` to `CREATE TABLE`; idempotent migration + `CREATE INDEX IF NOT EXISTS idx_events_replace ON events(account_id, relates_to_event_id)`; extend `StoredEvent`; add `edit_chain`, `retrievable_content`, `mark_redacted` read/write helpers.
- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- `ArchiveEvent` gains `relates_to_event_id`/`rel_type: Option<String>`; introduce `ArchiveMsg { Insert(ArchiveEvent), Redact { account_id, event_id, redacted_ts } }`; channel + `ArchiveHandle::redact`; app-wide setting accessors `get_honor_remote_deletions(data_dir)` / `set_honor_remote_deletions(data_dir, bool)` wrapping `registry::{get,set}_setting`.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- writer loop matches `ArchiveMsg`; `Insert` writes new columns; `Redact` runs the `mark_redacted` UPDATE; failures logged (ids only), task never dies.
- `src-tauri/crates/keeper-core/src/account.rs` -- `build_archive_event` extracts `Relation::Replacement` → relation cols; new `register_redaction_handler` (subscribes to `OriginalSyncRoomRedactionEvent`, extracts target id honoring room-version `redacts` location, sends `Redact`); thread `redaction_handler: EventHandlerHandle` through `AccountHandle` + every `activate()` tuple + `shutdown()` teardown (mirror `archive_handler`); new `edit_history(&self, platform, account_id, room_id, item_key) -> Result<Vec<EditVersionVm>, CoreError>` resolving `item_key`→`event_id` then reading the archive chain.
- `src-tauri/crates/keeper-core/src/vm.rs` -- new `EditVersionVm { body: String, timestamp: i64, is_current: bool }` (`#[ts(export)]`, serde camelCase).
- `src-tauri/crates/keeper/src/ipc.rs` -- commands `edit_history_get(account_id, room_id, item_key)`, `honor_remote_deletions()`, `set_honor_remote_deletions(enabled)`; exhaustive error arms; register all three in the Tauri `invoke_handler` (in `keeper/src/lib.rs`).
- `src/lib/ipc/client.ts` -- bindings `getEditHistory`, `honorRemoteDeletions`, `setHonorRemoteDeletions`; re-export `EditVersionVm` from `gen/`.
- `src/components/chat/message-bubble.tsx` -- turn the static "Edited" caption into a `Popover` trigger button opening the edit-history popover; keep styling tier.
- `src/components/chat/edit-history-popover.tsx` -- NEW: on open, fetch versions via `getEditHistory`, render prior versions with timestamps (newest→oldest), empty state "No local history."; keyboard-operable (Esc closes, focus returns).
- `src/components/settings/settings-dialog.tsx` -- add "Honor remote deletions locally" `Switch` row + disclosure copy in the Archive & Storage section; reuse existing `STORAGE_HONESTY_SENTENCE` FileVault line.
- `src/components/ui/{popover,switch}.tsx` -- reference only (reuse; do not modify).
- `src-tauri/crates/keeper-core/src/send.rs` -- reference for the `item_key`→`event_id` resolution pattern (do not modify).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/db.rs` -- add three nullable columns to the `events` schema, idempotent `PRAGMA table_info` migration + replace-relation index, extend `StoredEvent`, and add `edit_chain(conn, account_id, event_id)`, `retrievable_content(conn, account_id, event_id, honor_deletions)`, `mark_redacted(conn, account_id, event_id, ts)` -- version-chain + redaction-durability storage foundation.
- [x] `src-tauri/crates/keeper-core/src/archive/mod.rs` -- extend `ArchiveEvent` with relation fields, add the `ArchiveMsg` channel enum + `ArchiveHandle::redact`, and the app-wide honor-deletions setting accessors -- write API + policy storage seam.
- [x] `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- match `ArchiveMsg` in the single writer: insert with relation columns, apply redaction marks -- serialized single-writer for both ops.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- extract the `m.replace` relation in `build_archive_event`; register/teardown the redaction handler; implement `edit_history` (resolve `item_key`→`event_id`, read the archive chain, map to `EditVersionVm` extracting `m.new_content.body` for edits) -- live edit/redaction ingestion + archive-fed history read.
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- add `EditVersionVm` with ts-rs export -- IPC view model.
- [x] `src-tauri/crates/keeper/src/ipc.rs` (+ `keeper/src/lib.rs`) -- add and register the three commands with exhaustive error mapping -- IPC surface.
- [x] `src/lib/ipc/client.ts` -- add the three TS bindings and re-export `EditVersionVm` -- typed frontend access.
- [x] `src/components/chat/edit-history-popover.tsx` (new) + `src/components/chat/message-bubble.tsx` -- clickable "Edited" caption opening an archive-fed popover of prior versions with timestamps; empty state; keyboard-operable -- FR-11 edit-history UI.
- [x] `src/components/settings/settings-dialog.tsx` -- "Honor remote deletions locally" toggle + plain disclosure in Archive & Storage -- FR-36/UX-DR17 settings surface.
- [x] `src-tauri/crates/keeper-core/tests/archive_durability.rs` (new) + `archive/*` unit tests -- cover every I/O & Edge-Case Matrix row: edit-chain extraction/ordering, plain-message fallback on missing relation, redaction marking (incl. target-absent no-op), retrievable_content honor on/off, migration idempotency over a pre-5.1 schema -- verifies the matrix.
- [x] Frontend tests -- `message-bubble`/`edit-history-popover` test (clicking "Edited" fetches and lists prior versions; empty state) and a `settings-dialog` toggle test (renders, reads, persists) -- verifies UI behavior.

**Acceptance Criteria:**
- Given a message that is remotely edited, when the edit syncs, then the archive holds both versions as a chain, the timeline shows the latest with the "Edited" caption, and clicking the caption opens an edit-history popover fed by the Local Archive listing prior versions with timestamps (FR-36, FR-11).
- Given a remote redaction, when it syncs, then the timeline shows the redaction stub (always), the target archive row is marked redacted with content physically retained, and the pre-redaction content is returned by `retrievable_content` unless "Honor remote deletions locally" is on (FR-36).
- Given Settings → Archive & Storage, when it renders, then it carries the plain disclosure that keeper keeps local copies of remotely edited/deleted messages by default, that this affects only this Mac, and the "Honor remote deletions locally" toggle whose state persists across restarts (FR-36, UX-DR17).
- Given a pre-5.1 `archive.db`, when the app reopens it, then the new columns/index are added idempotently and previously ingested rows remain intact and queryable.

## Spec Change Log

_No bad_spec loopback occurred; the code matched the spec's scope. Empty._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 9: (high 1, medium 2, low 6)
- defer: 1: (high 0, medium 0, low 1)
- reject: 9
- addressed_findings:
  - `[high]` `[patch]` The honor-remote-deletions policy was wired nowhere on a read path — `edit_history` mapped every chain row and never consulted the setting or `redacted_ts`, and `retrievable_content` had no production callers, so toggling the setting changed nothing observable. Added a testable `visible_versions(chain, honor)` gate in `account.rs` that drops redacted versions from the popover when the setting is on (content still never erased), and `edit_history` now reads `archive::get_honor_remote_deletions` and applies it; added a unit test.
  - `[medium]` `[patch]` The settings disclosure copy asserted behavior that (before the above) did not exist and described a non-retroactive gate; with the read-time gate now wired, corrected `HONOR_REMOTE_DELETIONS_SENTENCE` to honestly describe the reversible retrieval gate ("hides … from history retrieval … turning it off makes them retrievable again … local copies are never erased").
  - `[medium]` `[patch]` `edit-history-popover.tsx` called `new Date(version.timestamp).toISOString()` unguarded — an out-of-range ms timestamp throws a `RangeError` and crashes the popover render; added a `safeIso` guard that omits `dateTime` when the value is non-finite / out of range.
  - `[low]` `[patch]` `edit_chain` ordered only by `origin_ts ASC`, so rapid edits sharing a server timestamp ordered non-deterministically and could flag the wrong version `is_current`; added `inserted_ts ASC, event_id ASC` tiebreak.
  - `[low]` `[patch]` The popover rendered a genuine read failure with the same "No local history." text as an empty history; the error state now shows a distinct "Couldn't load history." (honest-UI).
  - `[low]` `[patch]` React list `key` was `${timestamp}-${body}`, which collides when two versions share both; now includes the render index (scoped biome-ignore, static non-reorderable list).
  - `[low]` `[patch]` The fixed-width popover could overflow on long/CJK bodies with no break opportunity; added `max-h-64 overflow-y-auto` and `[overflow-wrap:anywhere]`.
  - `[low]` `[patch]` A popover fetch from a previous open could resolve after close/reopen and overwrite state; added a monotonic request-token guard.
  - `[low]` `[patch]` The settings toggle could revert to a stale value when toggled rapidly (a failed earlier persist clobbering a newer state); added a monotonic write-token so a failed persist only reverts when it is still the latest toggle.

### 2026-07-05 — Review pass (follow-up)
- intent_gap: 0
- bad_spec: 0
- patch: 1: (high 0, medium 1, low 0)
- defer: 1: (high 0, medium 1, low 0)
- reject: 21
- addressed_findings:
  - `[medium]` `[patch]` `visible_versions` filtered redacted rows and *then* delegated to `edit_versions_from_chain`, which flagged the last *surviving* row `is_current` by position. When "Honor remote deletions locally" is ON and the newest (current) version is itself redacted, the true-current row is dropped and an older survivor was mislabelled `is_current` — contradicting the live timeline and silently suppressing that survivor from the popover's `!isCurrent` prior-versions list. Fixed by capturing the current version's `event_id` from the *full* chain before filtering and passing it to `edit_versions_from_chain(chain, current_event_id)`, which now flags `is_current` by identity; when the current version is dropped, no survivor is flagged (every survivor is an honest prior version). Added a `visible_versions_current_redacted_flags_no_survivor_current` unit test and updated the two existing `edit_versions_from_chain` tests to the new signature; `check:rust` + `test:rust` (364/364) green.
  - Note: the one `defer` (a remote redaction processed before its target's `Insert` in the same sync batch never re-applies the mark) is the identical finding the prior pass already recorded in `deferred-work.md` for this spec, so per the orchestrator's "NEW entries only" instruction no duplicate ledger entry was appended.

## Design Notes

**Why honoring is retrieval-gated, not erasing.** FR-36 states redactions "mark, but do not erase, the local copy." So the durable behavior in *all* cases is mark-only; the setting merely gates whether redacted content is *returned* by retrievability read helpers (search/export in 5.3/5.5, and the archive-fed history read). This is non-destructive, reversible, and matches the FR literally; physical erasure and retroactive honoring on toggle-flip are explicit non-goals.

**Edit relation extraction (ruma).** Match the message content's relation:
```rust
let (relates_to_event_id, rel_type) = match ev.content.relates_to.as_ref() {
    Some(Relation::Replacement(r)) => (Some(r.event_id.to_string()), Some("m.replace".to_owned())),
    _ => (None, None), // plain message or reply — not an edit
};
```
The edit row's `content_json` carries `m.new_content`; `edit_history` extracts each edit's display text from `m.new_content.body` (original row uses top-level `body`).

**Chain root = original event_id.** matrix-sdk's timeline aggregates edits onto the original item, so resolving `item_key` yields the *original* `event_id`, which equals the archived `relates_to_event_id` of every edit — the join key for `edit_chain`.

**UTD re-decryption deferred.** Re-archiving events that were UTD at sync and later re-decrypt is ingestion-completeness, orthogonal to remote-rewrite durability and outside 5.2's ACs; capturing it needs a re-decryption hook the archive's raw event handlers don't provide. Left deferred (log to `deferred-work.md`), not implemented here.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` passes (no `.unwrap()`, no warnings).
- `bun run test:rust` -- expected: cargo-nextest green incl. new archive durability unit + integration tests covering every I/O matrix row.
- `bun run check` -- expected: biome + tsc + vitest green incl. edit-history popover and settings-toggle tests.
- `bun run check:all` -- expected: full gate green (frontend + rust + build).

## Auto Run Result

Status: done

**Summary:** Delivered Story 5.2 (epic 5 trust pillar): the local archive now survives remote rewrites and exposes edit history. `keeper-core::archive` gained three nullable `events` columns (`relates_to_event_id`, `rel_type`, `redacted_ts`) with an idempotent `PRAGMA table_info` migration for pre-5.1 DBs and an `idx_events_replace` index. Remote edits (`m.room.message` with an `m.replace` relation) are archived with the relation extracted into queryable columns; remote redactions **mark** the target row (`redacted_ts`) through the same single serialized writer and never erase content. A new `ArchiveMsg { Insert, Redact }` channel and `ArchiveHandle::redact` keep the one-writer invariant. `edit_history` resolves the timeline item's opaque `item_key` → original `event_id` via the live `Timeline`, reads the version chain from `archive.db`, and (after review) gates it through the app-wide "Honor remote deletions locally" setting so redacted versions are withheld when the policy is on. The frontend "Edited" caption is now a clickable popover fed by the archive, and Settings → Archive & Storage carries the honest disclosure + toggle.

**Files changed:**
- `src-tauri/crates/keeper-core/src/archive/db.rs` — schema + idempotent migration + index; `StoredEvent` extended; `edit_chain` (deterministic order), `retrievable_content`, `mark_redacted`.
- `src-tauri/crates/keeper-core/src/archive/mod.rs` — `ArchiveEvent` relation fields; `ArchiveMsg` enum; `ArchiveHandle::redact`; `get_/set_honor_remote_deletions` accessors.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` — writer matches `ArchiveMsg` (insert relation cols; apply redaction mark).
- `src-tauri/crates/keeper-core/src/account.rs` — `build_archive_event` relation extraction; `register_redaction_handler` threaded through activate/`AccountHandle`/`shutdown`; `edit_history` + `visible_versions` honor gate; unit tests.
- `src-tauri/crates/keeper-core/src/vm.rs` — `EditVersionVm` (ts-rs export).
- `src-tauri/crates/keeper/src/ipc.rs` + `lib.rs` — `edit_history_get`, `honor_remote_deletions`, `set_honor_remote_deletions` registered.
- `src-tauri/crates/keeper-core/tests/archive_durability.rs` (new) + `tests/archive_ingestion.rs` (fixture) — durability integration coverage.
- `src/lib/ipc/client.ts` + `src/lib/ipc/gen/EditVersionVm.ts` — bindings + generated type.
- `src/components/chat/edit-history-popover.tsx` (new) + test, `src/components/chat/message-bubble.tsx`, `src/components/layout/conversation-pane.tsx` — clickable "Edited" → archive-fed popover.
- `src/components/settings/settings-dialog.tsx` + test — "Honor remote deletions locally" toggle + disclosure.

**Review findings:** 2 reviewers (adversarial Blind Hunter + Edge Case Hunter). Triage: 0 intent_gap, 0 bad_spec, 9 patch (high 1, medium 2, low 6), 1 defer, 9 reject. Both reviewers independently flagged the load-bearing defect — the honor-deletions policy was wired to no read path — which was patched by gating `edit_history`; the disclosure copy was corrected to match. Other patches: `toISOString` crash guard, deterministic chain ordering, distinct read-error state, unique React keys, popover overflow, and stale-resolution/toggle-race guards. Deferred: the same-sync-batch insert-vs-redact ordering hazard (needs pending-redaction reconciliation; logged to `deferred-work.md`). Rejected as by-design/negligible: live-timeline resolution dependency, position-based `is_current` when the original is absent, self-replace, `unique_id` collisions, name-only migration vs a legacy same-named column, index/`OR` planner note, per-event `String` clone when archiving is disabled, and the fire-and-forget eventual-consistency of the writer.

**Verification:** `bun run check:rust` → fmt clean + clippy `-D warnings` clean. `bun run test:rust` → 363/363 pass (incl. the new `archive_durability` suite covering every I/O matrix row + the `visible_versions` honor-gate test). `bun run check` → biome + tsc clean, vitest 552/552, tauri-free invariant holds. `bun run check:all` → all steps green except the pre-commit `bindings:check` invariant, which fails only because the newly generated `src/lib/ipc/gen/EditVersionVm.ts` is untracked until this commit lands (no existing binding drifted); it passes once committed.

**Residual risks:** (1) The deferred insert-vs-redact ordering race can, only under honor-deletions ON plus a rare same-batch race, leave a redacted row unmarked (observably harmless under the default honor-off posture). (2) Edit history is available only for the currently-open room's loaded timeline items (the item_key→event_id resolution is live-timeline-based, as the spec mandates); an item scrolled out of the SDK window resolves to empty — but its "Edited" affordance is not rendered either. (3) Archive content remains decrypted-at-rest under the epic's disclosed FileVault-only posture.

---

**Follow-up review pass (2026-07-05).** A fresh independent adversarial + edge-case review pass was run against the full diff since the baseline. It surfaced one genuinely new, actionable defect the original pass had left in its own `visible_versions` code: when "Honor remote deletions locally" is ON and the newest version of an edited message is itself redacted, the dropped-current row caused an older survivor to be mislabelled `is_current`, contradicting the live timeline and hiding that survivor from the popover. Patched by deriving `is_current` from the full chain's current `event_id` (by identity, before filtering) rather than by post-filter position; when the current version is dropped, no survivor is flagged. Added `visible_versions_current_redacted_flags_no_survivor_current` and adjusted the two `edit_versions_from_chain` tests. All other findings were re-confirmed as by-design (spec-scoped single-level chain, archive-only history, live-timeline resolution), not reachable (ruma-typed relation event ids, name-only migration on a codebase-controlled 5.1 schema), benign eventual-consistency, or cosmetic. The one deferrable finding (same-batch insert-vs-redact race) was already recorded in `deferred-work.md` by the original pass, so no duplicate ledger entry was added. Gates: `bun run check:rust` clean (fmt + clippy `-D warnings`), `bun run test:rust` 364/364. Frontend untouched. `followup_review_recommended` set `false` — a single localized medium-severity correctness patch does not warrant a further independent pass.
