---
title: 'Archive Ingestion Pipeline'
type: 'feature'
created: '2026-07-05'
baseline_revision: '24d3894adeb100b154e031cc471cc763d7bb3012'
final_revision: '68f4d6cf332041a7ebc5d8b38a3a1722544cd5ab'
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

**Problem:** keeper never persists synced history to the user's own disk, so message history depends entirely on each platform's retention. Epic 5's trust pillar starts here: every event keeper syncs, for every Account, must land in a local `archive.db`.

**Approach:** Add a tauri-free `keeper-core::archive` module owning one `archive.db` (all Accounts, keyed by `account_id`, WAL mode). A single serialized writer task owns the connection and receives normalized events over a channel. Each per-account matrix client registers a post-decryption event handler that maps message-like events into normalized rows and hands them to the shared writer. Append-only, idempotent by `(account_id, event_id)`. No UI, no FTS, no version chains — this is the ingestion foundation only.

## Boundaries & Constraints

**Always:**
- Exactly ONE `archive.db` for all Accounts at `<data_dir>/archive.db`; WAL mode (`journal_mode=WAL`), created idempotently (`CREATE TABLE IF NOT EXISTS`) mirroring `registry.rs::open()`.
- Exactly ONE serialized writer task owns the archive `Connection` app-wide; all writes funnel through it via a channel. No other code opens the DB for writing.
- Ingestion is append-only and idempotent: `INSERT OR IGNORE` keyed on `(account_id, event_id)`. Re-syncing the same event never duplicates or mutates a row.
- The archive path must NEVER block the messaging/sync path: use a non-blocking send (unbounded channel); a write failure is logged (`tracing`, ids only) and the task continues.
- Normalized row captures: `account_id`, `event_id`, `room_id`, `sender`, `origin_ts` (i64 ms epoch), `event_type`, `content_json`, optional media metadata JSON, `inserted_ts`.
- Message text/metadata retention is independent of any media cache: store media *metadata* (mxc, mimetype, size, dims, filename), never media bytes.
- Per-account event handler is registered in `activate()` and removed/aborted in `shutdown()` alongside existing task teardown.
- Rust owns all Matrix logic; keeper-core stays tauri-free; no `.unwrap()`/bare `.expect()` in production paths; `?` + `thiserror`; logs carry ids not content.

**Block If:**
- Capturing account-wide post-decryption events is not achievable through `matrix-sdk` without copying AGPL/GPL code or violating the "Rust owns Matrix, media never crosses IPC as bytes" invariants.

**Never:**
- No FTS/FTS5 index (Story 5.3), no edit version chains / redaction-honoring / "honor remote deletions" toggle (Story 5.2), no export (Story 5.5), no archive-deletion or sign-out-survival path (Story 5.7).
- No media bytes copied into `archive.db`.
- No frontend/UI changes, no IPC commands, no `Vm` view models, no ts-rs exports (this story is backend-only).
- No passphrase encryption of `archive.db` (FileVault-only posture per epic).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| New message event | Post-decryption message-like event for account A | One normalized row inserted with all fields; `content_json` = event content, `media_json` NULL | No error expected |
| Duplicate event | Same `(account_id, event_id)` ingested again (re-sync) | `INSERT OR IGNORE`; exactly one row remains, unchanged | No error expected |
| Media message | Image/video/audio/file event | Row inserted; `media_json` holds mxc/mimetype/size/dims/filename; no bytes stored | No error expected |
| Write failure | DB busy / IO error on insert | Event dropped for this attempt, logged via `tracing` with ids only; writer task keeps running; sync/messaging unaffected | Swallowed at archiver boundary |
| Restart, network off | Rows previously committed; app reopened, no network | `open_archive_db` reopens same file; all previously ingested events present and queryable | No error expected |
| Multi-account | Events from accounts A and B | Rows land in the one `archive.db` distinguished by `account_id`; writer serializes both | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- NEW module root: `ArchiveHandle`, `ArchiveEvent`, `ArchiveMedia`, `ArchiveWriter::spawn(data_dir)`; re-exports.
- `src-tauri/crates/keeper-core/src/archive/db.rs` -- NEW: `open_archive_db(data_dir) -> Connection` (WAL + `CREATE TABLE IF NOT EXISTS events`), read helpers `event_count`/`get_event` for tests + downstream.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- NEW: writer task loop consuming `ArchiveEvent`, `INSERT OR IGNORE`, media-metadata JSON serialization.
- `src-tauri/crates/keeper-core/src/error.rs` -- add `ArchiveError` (thiserror) + `#[from]` into `CoreError`.
- `src-tauri/crates/keeper-core/src/lib.rs` -- declare `mod archive;`.
- `src-tauri/crates/keeper-core/src/account.rs` -- create the single `ArchiveHandle` in `AccountManager::new`; in `activate()` register a `client.add_event_handler` mapping message-like events → `ArchiveEvent` → `handle.ingest`; store the `EventHandlerHandle`/task on `AccountHandle` and remove/abort in `shutdown()`.
- `src-tauri/crates/keeper-core/src/registry.rs` -- reference pattern for `db_path`, WAL, idempotent schema, and temp-dir test style (do not modify).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/db.rs` -- add `open_archive_db(data_dir)` (creates dir, opens `<data_dir>/archive.db`, sets `journal_mode=WAL`, `CREATE TABLE IF NOT EXISTS events(account_id TEXT, event_id TEXT, room_id TEXT, sender TEXT, origin_ts INTEGER, event_type TEXT, content_json TEXT, media_json TEXT, inserted_ts INTEGER, PRIMARY KEY(account_id, event_id))`) plus `event_count(account_id)` / `get_event(account_id, event_id)` read helpers -- schema + persistence foundation.
- [x] `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- writer task: owns the `Connection`, awaits only on channel recv, performs synchronous `INSERT OR IGNORE`, serializes `ArchiveMedia` to `media_json`, logs failures with ids -- serialized single-writer + idempotent append.
- [x] `src-tauri/crates/keeper-core/src/archive/mod.rs` -- define `ArchiveEvent { account_id, event_id, room_id, sender, origin_ts, event_type, content_json, media: Option<ArchiveMedia> }`, `ArchiveMedia { mxc, mimetype, size, width, height, filename, thumbnail_mxc }`, `ArchiveHandle { tx }` with non-blocking `ingest(&self, ArchiveEvent)`, and `ArchiveWriter::spawn(data_dir) -> Result<ArchiveHandle, ArchiveError>` (opens DB, spawns writer task over an unbounded channel) -- public API + wiring seam.
- [x] `src-tauri/crates/keeper-core/src/error.rs` -- add `ArchiveError` variants (Sqlite/Serialization) and `CoreError::Archive(#[from] ArchiveError)` -- error rollup.
- [x] `src-tauri/crates/keeper-core/src/lib.rs` -- add `mod archive;` -- register module.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- construct one `ArchiveHandle` in `AccountManager::new` (from `platform` data_dir); in `activate()` register `client.add_event_handler` for message-like room events that builds an `ArchiveEvent` (extracting `origin_server_ts` ms, sender, room id, event type, content JSON, and media metadata from message content) and calls `handle.ingest`; store the handler handle on `AccountHandle` and remove it during `shutdown()` -- live account-wide ingestion + clean teardown.
- [x] `archive/*` unit tests + `src-tauri/crates/keeper-core/tests/` integration test -- cover every I/O & Edge-Case Matrix row: insert, dedupe idempotency, media-metadata round-trip, reopen-after-close persistence, multi-account keying, write-failure resilience -- verifies the matrix.

**Acceptance Criteria:**
- Given connected Accounts with events flowing through sync, when an event is delivered post-decryption, then a per-account handler appends a normalized row to the single `archive.db` via the serialized writer in WAL mode.
- Given the writer has committed rows and the app is restarted with the network disabled, when `archive.db` is reopened and queried, then every previously ingested event is present and queryable.
- Given a media message event, when it is ingested, then the row stores media metadata only (no bytes) and text/metadata persistence does not depend on the media cache being present.
- Given the same event is ingested twice, when the second insert runs, then exactly one row exists and it is unchanged (idempotent).
- Given an archive write fails, when the error occurs, then it is logged with ids only and neither the sync loop nor message sending is blocked or aborted.

## Spec Change Log

_No bad_spec loopback occurred; this story's code matched the spec's scope. Empty._

## Review Triage Log

### 2026-07-05 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 4: (high 0, medium 1, low 3)
- defer: 4: (high 0, medium 1, low 3)
- reject: 10
- addressed_findings:
  - `[medium]` `[patch]` Archive event handler was registered in `activate()` *after* `sync.start()`, so events in the first sync batch after every activation could be missed — moved `register_archive_handler` to before the SyncService is built/started (registration only needs the `Client`).
  - `[low]` `[patch]` No `busy_timeout` on archive connections while a long-lived writer connection runs concurrently with short-lived reader connections — added a 5 s `busy_timeout` in `open_archive_db` so a briefly-held WAL/checkpoint lock makes readers wait rather than return `SQLITE_BUSY`.
  - `[low]` `[patch]` Doc-comments overclaimed coverage ("Every event keeper syncs … must land on the user's own disk" / "all synced history lands") — corrected `archive/mod.rs` and the `activate()` comment to state the real scope (`m.room.message` via the live sync flow) and name the non-goals (paginated history, non-message types, UTD re-decryption).
  - `[low]` `[patch]` The `archive.db` path literal was duplicated in `db::db_path` (private) and `archive_db_path` (public) and could drift — made `db::db_path` the single canonical helper and had `archive_db_path` delegate to it.



```rust
// mod.rs — non-blocking producer side
pub fn ingest(&self, ev: ArchiveEvent) {
    // unbounded => never awaits, never blocks sync/send path
    let _ = self.tx.send(ev); // drop+log on closed channel
}

// ingest.rs — the ONE writer task
async fn run(mut rx: UnboundedReceiver<ArchiveEvent>, conn: Connection) {
    while let Some(ev) = rx.recv().await {
        let media = ev.media.as_ref().map(|m| serde_json::to_string(m));
        if let Err(e) = conn.execute(
            "INSERT OR IGNORE INTO events \
             (account_id,event_id,room_id,sender,origin_ts,event_type,content_json,media_json,inserted_ts) \
             VALUES (?,?,?,?,?,?,?,?,?)", params![..]) {
            tracing::warn!(account_id = %ev.account_id, event_id = %ev.event_id, error = %e, "archive write failed");
        }
    }
}
```

- `Connection` is `Send`; the writer task holds it across `rx.recv().await` — fine for a single owning task. Do sync rusqlite calls only between recvs (mirrors `registry.rs`; never share the connection).
- The `ArchiveHandle` is created ONCE (in `AccountManager::new`) and cloned into each account's event handler, guaranteeing a single writer for `archive.db`.
- `ArchiveEvent` is a plain keeper-core struct (not a `Vm`, no IPC) so the archive module is unit-testable without a live matrix client; `account.rs` performs the matrix-event → `ArchiveEvent` mapping.
- Known 5.1 limitation (defer, do not implement here): events that are UTD at sync time and later re-decrypt are not re-archived by the one-shot event handler; re-decryption durability belongs to Story 5.2.

## Verification

**Commands:**
- `bun run check:rust` -- expected: `cargo fmt --check` clean + `clippy --all-targets -- -D warnings` passes (no `.unwrap()`, no warnings).
- `bun run test:rust` -- expected: cargo-nextest green, including new archive unit + integration tests covering every I/O matrix row.
- `bun run check:all` -- expected: full gate (frontend unchanged) passes; confirms no unintended frontend/IPC surface was added.

## Auto Run Result

Status: done

**Summary:** Added the local archive ingestion pipeline (Story 5.1, epic 5 trust pillar). A new tauri-free `keeper-core::archive` module owns one `archive.db` for all Accounts (WAL, keyed by `account_id`) with a single serialized writer task fed over an unbounded channel; each per-account matrix client registers a post-decryption `m.room.message` event handler that normalizes events into append-only, idempotent rows (`INSERT OR IGNORE` on `(account_id, event_id)`), storing media *metadata* only (never bytes). Backend-only — no UI, IPC, or ts-rs surface.

**Files changed:**
- `src-tauri/crates/keeper-core/src/archive/mod.rs` (new) — `ArchiveEvent`/`ArchiveMedia`, cloneable `ArchiveHandle` (non-blocking `ingest`), `ArchiveWriter::spawn` (runtime-agnostic writer spawn), canonical-delegating `archive_db_path`.
- `src-tauri/crates/keeper-core/src/archive/db.rs` (new) — `open_archive_db` (WAL + `busy_timeout` + idempotent `events` schema), `event_count`/`get_event` read helpers, `StoredEvent`, canonical `db_path`.
- `src-tauri/crates/keeper-core/src/archive/ingest.rs` (new) — the single serialized writer task (`INSERT OR IGNORE`, id-only failure logging, never panics).
- `src-tauri/crates/keeper-core/src/account.rs` — one app-wide `ArchiveHandle` in `AccountManager::new`; handler registered in `activate()` **before** sync starts and removed in `shutdown()`; pure `build_archive_event`/`archive_media`/`plain_mxc` mappers + tests.
- `src-tauri/crates/keeper-core/src/error.rs` — `ArchiveError` + `CoreError::Archive(#[from])`.
- `src-tauri/crates/keeper-core/src/lib.rs` — `pub mod archive;`.
- `src-tauri/crates/keeper/src/ipc.rs` — `AppState::new` resolves the data dir for the writer; exhaustive `CoreError::Archive` IPC arm.
- `src-tauri/crates/keeper-core/tests/archive_ingestion.rs` (new) — end-to-end ingestion/dedupe/media/multi-account/persistence integration tests.

**Review findings:** 2 reviewers (adversarial + edge-case). Triage: 0 intent_gap, 0 bad_spec, 4 patch, 4 defer, 10 reject.
- Patches applied (see Review Triage Log): handler registered before `sync.start()` (first-batch race), `busy_timeout` on archive connections, doc-comment scope accuracy, unified `archive.db` path helper.
- Deferred to `deferred-work.md`: graceful writer drain on app quit; writer health/restart supervision; broader event-type coverage (reactions/state/paginated history/UTD re-decryption — owned by Stories 5.2/5.6); `data_dir()`-failure temp-dir fallback.
- Rejected (noise / by-design): hardcoded `event_type` (correct for the ingested type), redaction & edit-chain durability (Story 5.2), insert-vs-chrono ordering (rows carry `origin_ts`), WAL checkpoint (SQLite auto-checkpoints, matches `registry.rs`), unbounded channel (deliberate non-blocking trade-off), `filename()` usage (matches `timeline.rs` convention), encrypted-media key / plaintext at rest (the epic's disclosed FileVault-only posture; content JSON must stay lossless for export in Story 5.5), `as`-cast overflow (harmless).

**Verification:** `bun run check:rust` → fmt clean + clippy `-D warnings` clean. `bun run test:rust` → 342/342 pass (incl. new archive unit + integration tests covering every I/O & Edge-Case Matrix row). Re-run green after patches.

**Residual risks:** (1) Events queued in the writer channel at abrupt app exit are not flushed (steady-state queue is near-empty; deferred). (2) Archive coverage is `m.room.message` from the live sync flow only — not a total server capture (deferred; later epic-5 stories own the rest). (3) `archive.db` stores decrypted plaintext + media keys unencrypted, protected only by FileVault — the epic's explicit, to-be-disclosed at-rest posture.
