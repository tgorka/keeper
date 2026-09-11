---
title: 'The archive follows every recordings root'
type: 'bugfix'
created: '2026-09-10'
status: 'done'
review_loop_iteration: 1
baseline_commit: '482669d82886f24554d5d1c2397180cd2f0cacd0'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** the recordings index (`archive.db`) only ever indexes the *current* recording destination, and never reconciles what it already holds. On hesperia 2026-09-09: neuradrive declares `[folder.recordings] subfolder = "70-comms/meetings"` and has zero rows — its 23 meeting folders are unknown to search and to the durability banner; the GSD meeting, moved by hand from tgdrive into neuradrive where it is untracked, still reads `profile = tgdrive, relative_path = "2026/2026-09-08 10.01 gsd-20260908", durability = committed`; 49 rows of deleted test recordings stand as orphans. `recover_orphaned_recordings` rebuilds one root, `rebuild_from_disk_within` never removes a row whose folder is gone, and `floored_durability` keeps the old word through a move.

**Approach:** every recordings root is rebuilt — the folder destination and each enabled profile's `recordings_root()` — at startup and whenever the profile set or the destination changes. A rebuild of a root reconciles it: a session folder found elsewhere is re-homed (path, root kind, profile), one found nowhere is removed with its segments and search rows, and the durability floor never survives a move. For a session under a profile's root, durability is derived from that repository — the shell hands the archive a probe over `Engine::path_durability` — never trusted from the old row. A root whose directory is absent (a drive that is out) is left exactly as it is.

## Boundaries & Constraints

**Always:** keeper-core stays free of keeper-sync — durability reaches the rebuild as a probe (`Box<dyn Fn(&Path) -> Option<RecordingDurabilityState> + Send + Sync>`) supplied by the shell, `None` for the folder destination; a rebuild that could not read its root reconciles nothing; reconciliation is scoped by `(root_kind, profile_id)` so one root's pass never touches another's rows; `PathDurability` maps `verified > pushed > committed > local`; removals keep FTS consistent the way existing deletions do; the recovery scan still runs off the boot thread and never fails boot; no `.unwrap()` outside tests.

**Ask First:** if a removed orphan row would lose a transcript or note that exists only in the archive (not on disk), ask before deleting rather than marking.

**Never:** change the `recordings` schema beyond what reconciliation needs; move files on disk; touch the sync engine's own durability writers (`SetRecordingDurability` from the push path); index a paused profile's root or a removable profile whose volume is absent.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Second profile declares a recordings root | tgdrive is the destination, neuradrive has `[folder.recordings]` | both roots rebuilt at startup; neuradrive's folders indexed with `profile_id = neuradrive` | unreadable root logged, others still rebuilt |
| Session moved between roots by hand | row under A, folder now under B | after both rebuilds: one row, under B, durability from B's git (`local` if untracked) | — |
| Session deleted | row under A, folder gone, found nowhere | row, segments and FTS rows removed; count logged | — |
| Drive out | removable profile, volume absent | its root skipped, rows untouched | — |
| Durability re-derived | folder committed and pushed in B | `pushed`; a later `SetRecordingDurability` advance still floors upward | probe error ⇒ `local`, logged |
| Destination or profile set changes | `sync_profile_*`, recording destination saved | the full rebuild runs again | — |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` -- `recover_orphaned_recordings` (:7990–8030) — one root today; `effective_recording_destination` (:7540) and `destination_profile_table` (:7742) already enumerate profiles with `recordings: Some(RecordingsPlace { root, .. })`; `scan_destination_volume` (:7815) answers volume presence; the shell's `path_durability` port (:5583/:6024) is the probe's source; recording destination save near :6511.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `sync_profile_remove` (:1372) and the profile upsert verb beside it — where the rebuild is re-run.
- `src-tauri/crates/keeper/src/lib.rs` -- boot hook (:679).
- `src-tauri/crates/keeper-core/src/archive/mod.rs` -- `rebuild_recordings` (:363) → `ArchiveMsg::RebuildRecordings`; `ingest.rs` (:58, :145) runs it; `move_recording` (:123).
- `src-tauri/crates/keeper-core/src/archive/recordings.rs` -- `rebuild_from_disk_within` (:954), `write_rebuilt_session` (:1069), `upsert_recording` (:401, `INSERT OR REPLACE`, floored), `floored_durability` (:324), `move_session` (:593), schema (:206), `relative_session_path` (:673); test `rebuild_from_disk_reproduces_fifty_sessions…` (:2165).
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `PathDurability` (:242: `committed`, `pushed`, `verified`), `path_durability` (:6614); `SyncProfile::recordings_root` (`profile/mod.rs:1154`).
- `docs/recording.md` -- "Recording into a synced folder" (:143), "When the synced folder is on a drive you unplug" (:195).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` -- `rebuild_from_disk_within` returns a `RebuildOutcome { written, removed, found: Vec<session_id> }` and takes an optional durability probe; a session written under a root different from its stored `(root_kind, profile_id, relative_path)` is a move and is written unfloored; after the walk, when the root was readable, rows under `(root_kind, profile_id)` not in `found` are deleted with their segments and FTS rows -- the reconciliation.
- [x] `src-tauri/crates/keeper-core/src/archive/mod.rs` + `ingest.rs` -- `rebuild_recordings` carries the probe (`Option<DurabilityProbe>`), logs `written`/`removed` -- the API.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `recover_orphaned_recordings` rebuilds every root: the destination plus each enabled profile's `recordings_root()` whose volume is present, each profile root with a probe over the engine's `path_durability` mapped to a state; one pure `recordings_roots(destination, table) -> Vec<RecordingsRootPlan>` for tests -- the enumeration.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` / `ipc.rs` -- re-run the full rebuild after a profile upsert or removal and after the destination is saved -- the triggers.
- [x] tests -- keeper-core: move between roots re-homes and re-derives (probe says `local`), delete removes rows/segments/FTS, absent root reconciles nothing, floor still applies to a later advance; shell: `recordings_roots` covers the matrix (second profile, paused, volume out) -- the rows.
- [x] `docs/recording.md` -- under "Recording into a synced folder": every synced folder that declares a recordings subfolder is indexed; a recording moved by hand is re-homed on the next rebuild and its durability is what *that* folder's repository says -- the record.

Review patches (step 04, loop 1 — amendments to the diff, the diff is kept). Ask-First status: the owner was asked whether an orphan row (title/note cached from a deleted manifest, no transcripts) is deleted or soft-marked; until answered, deletion stands as the frozen tasks require.
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` + `src-tauri/crates/keeper/src/ipc.rs` -- a triggered rebuild can run while a session is recording: the recorder sends the segment row before the manifest lists it, so `stale_segment_keys` prunes the newest segment row (its `closed_ts` exists nowhere else). Fix: the rebuild takes the set of reserved session folders (`reserved_recording_folders`) as `skip`; a skipped folder is neither rewritten, pruned nor reconciled (its id counts as found); a trigger while a session is live is logged and deferred — `recording_finalize` (or whatever ends a session) fires the rebuild once the folder is released. Update the `recover_orphaned_recordings` comment that says startup is the only moment nothing is recording. Test: a reserved folder's fresh segment row survives a rebuild.
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` -- a rename inside one root is not a move between roots. Fix: a move is a change of `(root_kind, profile_id)` only; within the same root a relocated folder is written `Exact` when the probe answered `Some` and `Floored` when it answered `None` (no repository answer never downgrades); between roots `Exact` always (`local` without a probe). Tests: profile → plain folder ends `local`; within-root rename with a `Committed` probe over a `verified` row ends `committed`; within-root rename with a `None` probe keeps `verified`.
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` -- an empty-but-present root (a stale mountpoint, a subfolder not yet synced) would wipe its rows. Fix: reconcile only when the walk was complete AND (it wrote at least one session OR the root held no rows before); log the refusal. Also `relative_session_path == None` (non-UTF-8 name, symlink out of root) marks the walk incomplete instead of letting the row be forgotten. Tests for both.
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` -- a session id stored under another root whose folder still exists on disk is not re-homed (log "one session id under two roots; keeping the first") so a copied folder does not flap between roots on every rebuild. Test.
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs` + `keeper-core` -- a removed profile's rows are purged: `sync_profile_remove` reconciles `(profile, id)` against an empty found set (rows, segments, FTS), because keeper no longer knows that folder; a paused profile's rows stay and the doc says so. Test through the sink below.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `recordings_roots`: a root nested inside another plan's root (either direction) is skipped with a warn naming both; when the plain-folder destination equals an enabled profile's recordings root, the profile plan wins (its repository can answer durability). Tests for both.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `spawn_recordings_index_rebuild`: one in-flight guard plus a dirty flag, so a burst of saves runs one rebuild now and one after (never N); panics in the detached thread are caught and logged. Test the coalescing with a fake sink.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- orphan recovery (`recover_orphaned_sessions`) runs over every planned root, not only the destination, so a crash-orphaned session in a second synced folder is marked recovered too.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- testability: the trigger path emits `RecordingsRootPlan`s through a `&dyn Fn(RecordingsRootPlan, Option<DurabilityProbe>)` sink so a test can assert one message per root per trigger (save, remove, pause/resume, destination save); the probe is built over the existing `RecordingSyncPort::path_durability` double so a test can assert it receives the absolute session folder and folds `verified > pushed > committed > local`.
- [x] `src-tauri/crates/keeper-core/src/archive/ingest.rs` -- if the archive writer runs on the async runtime, run the rebuild under `tokio::task::block_in_place` (the probe opens a repository per session folder); log `found` count or drop the field; merge the duplicate `stored_place`/`stored_durability` reads.
- [x] `src-tauri/crates/keeper-core/src/archive/recordings.rs` + `recordings_fts.rs` docs -- fix the `index_text` "never recycled" wording (recycled only after the table empties, harmlessly), the stale note near `fallback_session_id` (:733) that says a missing folder never deletes a row, and the "root absent" incomplete-walk line goes to debug.
- [x] `docs/recording.md` -- say plainly: a `[folder.recordings]` block that arrives by pull or hand edit is picked up at the next save or start; a drive plugged in after start is indexed at the next save or start; a removed folder's recordings leave the index, a paused folder's stay; on a machine without git every synced-folder session reads `local`; a rebuild waits for a recording in progress.

**Acceptance Criteria:**
- Given two profiles with recordings roots, when keeper starts, then both roots' sessions are searchable and carry their own `profile_id`.
- Given a session moved by hand into a folder that has never committed it, when the roots are rebuilt, then it has one row, under the new root, reading `local`.
- Given a session whose folder was deleted, when its root is rebuilt, then no row remains for it.
- Given a removable profile whose drive is out, when the rebuild runs, then its rows are untouched.

## Verification

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core archive` -- expected: rebuild/reconcile tests pass.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync` -- expected: all green.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync --all-targets -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper -- --check` -- expected: clean.
- The `keeper` shell crate compiles only on the Mac: `bun run check:rust:macos` (or CI's macOS job) -- expected: clean; say plainly if it was not run.

## Suggested Review Order

**The reconciliation — what a rebuild of one root may forget**

- Entry point: one request carries the root, its probe, the reserved folders to skip and the other roots it walks beside.
  [`recordings.rs:964`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L964)
- The walk, its completeness flag, and the guard that refuses to reconcile an empty-but-present root or an incomplete walk.
  [`recordings.rs:1155`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L1155)
- A move is a change of root, not of path; within a root the repository's answer wins only when it answered; a copied session keeps its first root.
  [`recordings.rs:1371`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L1371)
- Rows under this root not found by a complete walk are removed with segments and search entries.
  [`recordings.rs:1593`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L1593) · [`recordings_fts.rs:1`](../../src-tauri/crates/keeper-core/src/archive/recordings_fts.rs#L1)
- A removed profile's rows are forgotten wholesale.
  [`recordings.rs:1572`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L1572)
- The probe: keeper-core asks, the shell answers from the repository.
  [`recordings.rs:918`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L918)

**The enumeration — which roots, from where, with what durability**

- Every root: the destination plus each enabled profile's recordings root; nested roots skipped, a shared root goes to the profile.
  [`ipc.rs:8105`](../../src-tauri/crates/keeper/src/ipc.rs#L8105)
- The steps a trigger produces — forgets first, then one rebuild per root — through a sink a test can watch.
  [`ipc.rs:8254`](../../src-tauri/crates/keeper/src/ipc.rs#L8254)
- The probe over the sync port: `verified > pushed > committed > local`, `Err ⇒ local`.
  [`ipc.rs:8579`](../../src-tauri/crates/keeper/src/ipc.rs#L8579)

**The triggers — when, and never during a recording**

- Boot: orphan recovery over every root, then the gated rebuild.
  [`ipc.rs:8004`](../../src-tauri/crates/keeper/src/ipc.rs#L8004)
- The gate: in-flight, one folded rerun, deferred while a session is live, released when the folder is dropped, panic-safe.
  [`ipc.rs:8383`](../../src-tauri/crates/keeper/src/ipc.rs#L8383) · [`ipc.rs:8497`](../../src-tauri/crates/keeper/src/ipc.rs#L8497) · [`ipc.rs:8516`](../../src-tauri/crates/keeper/src/ipc.rs#L8516)
- Profile saved / removed (with its forget) / paused-resumed.
  [`sync_ipc.rs:1315`](../../src-tauri/crates/keeper/src/sync_ipc.rs#L1315) · [`sync_ipc.rs:1390`](../../src-tauri/crates/keeper/src/sync_ipc.rs#L1390) · [`sync_ipc.rs:1411`](../../src-tauri/crates/keeper/src/sync_ipc.rs#L1411)
- The writer runs the rebuild off the async runtime where there is one.
  [`ingest.rs:54`](../../src-tauri/crates/keeper-core/src/archive/ingest.rs#L54)

**The record**

- What is indexed, when a hand move is re-homed, what a paused or removed folder keeps, and that a rebuild waits for a recording.
  [`recording.md:188`](../../docs/recording.md#L188)

**Tests**

- A reserved folder's fresh segment row survives a rebuild.
  [`recordings.rs:2929`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L2929)
- Moved between roots, in both rebuild orders: one row, re-derived.
  [`recordings.rs:3031`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L3031)
- Renamed within a root: the repository's answer, or the floor when it did not answer.
  [`recordings.rs:3186`](../../src-tauri/crates/keeper-core/src/archive/recordings.rs#L3186)
- Nested roots skipped either way; the other planner rows.
  [`ipc.rs:13238`](../../src-tauri/crates/keeper/src/ipc.rs#L13238)
