---
title: 'What the end-to-end run found'
type: 'bugfix'
created: '2026-08-28'
status: 'done' # draft -> ready-for-dev -> in-progress -> in-review -> done
baseline_revision: '74b0278ab0cdf3b1283bee7f0f034a17c19156d0'
review_loop_iteration: 0
followup_review_recommended: true
context: []
warnings: []
---

<intent-contract>

## Intent

**Problem:** An end-to-end run of the shipped `keeper-syncd` binary against a real bare
remote (`/tmp/vf-e2e.sh`) failed three of its ten checks. `keeper-syncd materialize` queues a
download and returns exit `0` with the word `queued` — for a one-shot process with no
supervisor behind it that is a no-op wearing a success message, and it is the only reason the
harness could never reach its open-descriptor check. Its `ls-files` check failed for a
different reason entirely: the row's honest byte count is carried as `sizeBytes` and the
harness asked for `size`.

**Approach:** Give the one-shot `materialize` verb the drain `sync_once` already performs —
bounded by a strictly-decreasing outstanding count — so it either leaves the real bytes on
disk or exits non-zero naming the unit that did not deliver. Prove `ls-files` was already
honest on every surface rather than changing a documented wire contract to match a wrong key.

## Boundaries & Constraints

**Always:** `Engine::materialize_entry`'s signature and behaviour stay exactly as they are —
the app door has a supervisor and wants the queue-and-return semantics. One reservation is
held across the request and its drain, so no second `Busy` window is opened. The drain loop's
bound is *strict decrease*, never merely non-zero, so a transfer that genuinely cannot land
terminates instead of spinning. `--json` emits exactly one document per invocation.

**Block If:** a fix would require renaming or duplicating a documented `--json` key
(`docs/sync.md` §13 calls the field names the contract), or adding a `SyncError` variant —
the exhaustive funnel in `keeper/src/sync_ipc.rs` lives in a crate that cannot compile on this
host.

**Never:** no new whole-tree sweep on the request path (`materialize_landed`'s doc records the
40 hours that cost). No cross-process lock — that is a change to every one-shot verb and is
already recorded in `deferred-work.md`. No fix for the unborn-HEAD defect here; log it.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Object already in the local store | one-shot `materialize` | publishes inline, `outcome: materialized`, no `unitId`, exit 0 | No error expected |
| Object only on the remote | one-shot `materialize` | queues, drains it, publishes, `outcome: materialized`, `unitId` present, exit 0 | No error expected |
| Worktree already holds the content | one-shot `materialize` | `outcome: alreadyMaterialized`, exit 0 | No error expected |
| Transfer cannot land (remote has no object) | one-shot `materialize` | exit 1, one error document naming path, unit and the journal's recorded reason | `CliError::Operational` |
| Another run holds the reservation in-process | `materialize` while `watch` ticks | `SyncError::Busy`, nothing queued, exit 1 | existing `Busy` prose |
| `ls-files --json` on a virtual path | 4 MiB pointer, ~130 bytes on disk | `sizeBytes: 4194304`, `state: "virtual"` | No error expected |
| `ls-files --json` on an absent path | pointer indexed, no file | `sizeBytes` is the pointer's number, `state: "absent"` | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — `materialize_entry` (7383) queues and returns;
  split so a draining sibling can share the reservation. `sync_once` (6486) holds the
  drain-to-quiescence loop being reused. `materialize_landed` (5939) is what publishes a
  requested arrival past the virtual policy and re-stats the index.
- `src-tauri/crates/keeper-sync/src/db.rs` — `WorkKind::tag` (1213) spells `lfsDownload` as a
  literal while its siblings have consts; `outstanding_count` (1666) is the loop's counter.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `cmd_materialize` (1587) is the one-shot
  door; `materialize_lines` (1528) prints the sentence that promises a supervisor.
- `src-tauri/crates/keeper-sync/src/lfs/listing.rs` — `LfsFile.size_bytes` (117) is already
  `pointer.size` for every state. **No change.**
- `src-tauri/crates/keeper-sync/src/browse.rs:955` — the Files pane's own honest size. **No change.**
- `src-tauri/crates/keeper-sync/tests/materialize_entry.rs` — the fixture the drain test extends.
- `docs/sync.md:1380` — the paragraph that documents "does not wait".

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/tests/materialize_entry.rs` -- add the failing test first: object in a filesystem remote's store only, one-shot request must leave real bytes and a clean tree -- D2 has no coverage because every existing test asserts what the *request* left behind, never what a one-shot *delivered*.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- add `WorkKind::LFS_DOWNLOAD` and use it in `tag()`; add `unit_failure` -- the drain needs to count download units by kind and to quote why one did not land.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- split `materialize_entry` into `materialize_request` / `materialize_held`, add `materialize_entry_now` and `lfs_downloads_outstanding`/`unit_failure` -- one reservation must cover the request and its drain.
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` -- `cmd_materialize` becomes async and calls `materialize_entry_now`; `materialize_lines` stops promising a supervisor; a still-queued outcome exits non-zero -- the verb must not claim success unless the bytes landed.
- [x] `docs/sync.md` -- rewrite the `materialize` paragraph in §13 and note that `ls-files`' size key is `sizeBytes` -- the "has no event loop and does not wait" contract is the one being changed.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- log the unborn-HEAD defect with its reproduction, and the `size` / `sizeBytes` key inconsistency inside one document.

**Acceptance Criteria:**
- Given a profile whose object lives only on a filesystem remote, when `keeper-syncd materialize <profile> <path>` runs with no daemon anywhere, then the worktree holds the real content, `git status` is clean, `--json` says `outcome: "materialized"`, and the exit code is `0`.
- Given the same request against a remote that cannot serve the object, when the verb returns, then the exit code is non-zero and stdout carries exactly one JSON document naming the path, the unit and the recorded reason.
- Given `Engine::materialize_entry` (the app door), when it is called, then it still queues and returns without draining — its existing tests are unchanged.
- Given `/tmp/vf-e2e.sh` re-run against the rebuilt binary, when it finishes, then all ten checks pass, including step 6's open-descriptor refusal and the release after the descriptor closes.

## Spec Change Log

### 2026-08-28 — the drain was scoped by the wrong thing, and the second call did too much

**Triggering findings.** Two review layers independently reported the same two root causes, each
with a verified trace:

1. The drain's bound. The Design Notes below argued for a **profile-wide** outstanding-download
   count on the grounds that "a row this call did not queue can be the one making progress". That
   is wrong, and `enqueue_unique` is why: it deduplicates on the serialized payload, so the row
   that will deliver *this* object is exactly the row whose id came back. Meanwhile
   `materialize_pending` queues one `LfsDownload` per unfetched pointer after every pull, so on a
   fresh media clone the wide count is the folder's entire backlog and `CLAIM_LIMIT` is 16 — the
   requested row completes on pass one and the loop keeps going for ~N/16 more passes.
   `keeper-syncd materialize photos one-small.jpg` downloads the folder, holding the reservation
   throughout.
2. The second `materialize_held` call. Re-running the *whole request* to re-observe the worktree
   has side effects the observation never wanted, and each one defeats something this story added:
   a `parked` row is not cover for `enqueue_unique`, so the second call **inserts a duplicate
   journal row** and `unit_failure` then reads the fresh row's `NULL` instead of the permanent
   reason — the exact sentence `unit_failure` exists to recover — while every failed invocation
   leaks a row; on a *transient* failure `promote_unit` **erases the backoff** the same pass just
   applied, so a cron `materialize` hammers a dead remote at the cron cadence with `Requested`
   urgency.

Three further findings share those roots or fall out of them: `drain_journal` claims **every**
kind, so this verb could commit, push and open a pull request (and a drained `Pull` can move the
committed pointer under the second observation); a concurrent `keeper-syncd watch` holding the row
`running` makes both passes no-ops, so the verb exits non-zero on a transfer that is actively
succeeding; and `volume_ready` moved into `materialize_request` and now runs once while the
`.git` test it exists to precede runs twice, so a drive unplugged mid-transfer is reported as
`NotTracked` instead of `MediaAbsent`.

**Amended.** The Design Notes' "The count is every outstanding download for this profile" clause
is withdrawn and replaced by: the drain's **primary exit is the requested unit's own fate** —
delivered (its row is gone), given up on (`parked`), or held by another process (`running`) — with
the strictly-decreasing profile-wide count kept **only** as the anti-spin guard for the one case
that needs it, a requested row bumped out of a full batch by `claim_ready`'s background-slot swap.
The drain is narrowed to `lfsDownload` rows. The second pass **observes and never queues**, so it
neither duplicates a parked row nor cancels a backoff, and the failure report carries the unit
this invocation queued. `volume_ready` is re-asked before it. A delivered outcome carries
`unit_id: None`, honouring `Materialization::unit_id`'s own documented contract ("for `Queued` and
for nothing else") rather than amending two published contracts to fit a nicety.

**Known-bad state avoided.** A one-file request that downloads a whole folder; a verb sold as a
read that pushes; a permanent failure whose reason is thrown away by the code added to quote it;
an unbounded journal for a cron entry against dead credentials; a `unitId` on a delivered document
that a polling consumer would wait on forever.

**KEEP.** These survived review and must survive re-derivation: the `materialize_request` /
`materialize_held` split and the single reservation across request-and-drain (both layers called
it clean); `Engine::materialize_entry`'s unchanged signature, behaviour and check ordering for the
app door; reusing `materialize_held` for the second pass rather than writing a second publish
path; the strictly-decreasing bound as the anti-spin guard; `undelivered` as a pure, tested
function that decides whether the run kept its promise; adding no `SyncError` and no
`ContentRefusal` variant, so the un-compilable shell crate needs nothing; and the D1 finding
exactly as recorded — it is verified, mutation-guarded, and in scope.

**Rejected, with the argument.** (a) "A delivered run can still be reported as a refusal by the
second pass": `lfs::hydrate::plan` answers `AlreadyHeld` precisely when the content is here, so a
`Modified` refusal from the second pass is the true statement that the bytes are now neither the
pointer nor the content — an honest refusal about the present, not a stale re-adjudication.
(b) "Drop the `ls-files` `sizeBytes` paragraph from `docs/sync.md` as unrelated": it is this
story's D1 and the invocation asked for it in §13; the claim is pinned by
`the_state_and_the_size_come_from_the_worktree_and_the_pointer` and by the mutation recorded under
Verification. The reviewers had the diff without the story brief.

## Review Triage Log

### 2026-08-28 — Review pass
- intent_gap: 0
- bad_spec: 6: (high 4, medium 2, low 0)
- patch: 4: (high 0, medium 3, low 1)
- defer: 0
- reject: 2: (high 0, medium 0, low 2)
- addressed_findings:
  - `[high]` `[bad_spec]` The drain's only exit was the profile-wide outstanding-download count, so one small request drained the folder's whole backlog. Spec amended; primary exit is now `db::UnitStanding` on the requested row, with the strictly-decreasing count kept only as the anti-spin guard. Pinned by `one_request_does_not_drain_the_folders_whole_download_backlog`, which fails when the per-unit exit is removed.
  - `[high]` `[bad_spec]` `drain_journal` claims every kind, so this verb could commit, push, merge and open a pull request. Added `db::claim_ready_of_kind` and `Engine::drain_kind`; the request now drains `lfsDownload` only. Pinned by `the_request_drains_transfers_and_leaves_every_other_kind_alone`, which fails when the narrowing is removed.
  - `[high]` `[bad_spec]` The second `materialize_held` re-entered `enqueue_unique`, so a `parked` row got a duplicate and `unit_failure` read the fresh row's `NULL` — losing the very reason it was added to quote, and leaking one row per failed invocation. Added `WhenAbsent::Report`: the second pass observes and writes nothing, and the report carries the unit this run queued.
  - `[high]` `[bad_spec]` A concurrent `keeper-syncd watch` holding the row `running` made both passes no-ops, so the verb exited non-zero on a transfer that was actively succeeding — an alert on a healthy folder. `UnitStanding::InFlight` is now its own sentence and its own loop exit.
  - `[medium]` `[bad_spec]` The same re-asking second pass cancelled the backoff `reschedule_after` had just applied (`promote_unit` pulls `not_before_ms` back to now), so a cron `materialize` would hammer a dead remote at the cron cadence. Fixed by the same `WhenAbsent::Report`; pinned by `a_transfer_that_cannot_land_stops_without_cancelling_its_own_backoff`, which fails when the mode is reverted.
  - `[medium]` `[bad_spec]` `unit_id` on a `Materialized` outcome contradicted `Materialization::unit_id`'s own doc and `materialize_json`'s stated key set, so a consumer polling on the field's presence would wait forever for a row `db::complete` had deleted. A delivered outcome now carries `None`; the contract is honoured rather than amended.
  - `[medium]` `[patch]` `volume_ready` ran once while the `.git` test it precedes ran twice, so a removable drive unplugged mid-transfer read as `NotTracked`. Re-asked before the observing pass.
  - `[medium]` `[patch]` The queued line unconditionally advised starting a `keeper-syncd watch`, which cannot claim a parked row. `materialize_lines` is now one line and every remaining sentence comes from `undelivered`, which branches on the standing.
  - `[medium]` `[patch]` A `last_error` may predate this run (`claim_ready` does not clear it, and the background-slot swap can skip the row). Now quoted as the "last recorded failure" rather than as this run's.
  - `[low]` `[patch]` `drain_journal`'s error was `?`-propagated, so a `SQLITE_BUSY` after `materialize_landed` had already published would report a failed run over content on disk. The drain error now breaks the loop and lets the observation decide.
  - `[low]` `[patch]` The no-progress test's failure mode was an unbounded hang. Wrapped in `tokio::time::timeout`.
  - `[low]` `[reject]` "A delivered run can be reported as a refusal by the second pass": `hydrate::plan` answers `AlreadyHeld` exactly when the content is here, so a `Modified` refusal is the true statement that the bytes are now neither the pointer nor the content.
  - `[low]` `[reject]` "Drop the `ls-files` `sizeBytes` paragraph as unrelated": it is this story's D1, verified and mutation-guarded. The reviewers had the diff without the story brief.

## Design Notes

**D1 was not a product defect, and the evidence is the raw document.** `LfsFile.size_bytes` is
`pointer.size` unconditionally (`listing.rs:199`) for `virtual`, `materialized` and `absent`
alike — the pointer is the source for every state, so there is no nullability question to
settle and the field is `u64`, never `Option`. The human form prints the same number through
`format_bytes(file.size_bytes)`. The harness's own baseline output proves both: its step-5 line
read `media: queued  4.0 MB  scans/big.bin` while its step-4 assertion read `virtual null`. The
document the same run wrote:

```json
{"path":"scans/big.bin","state":"virtual","sizeBytes":4194304,"oid":"3762e862…"}
```

`docs/sync.md:1374` calls the field names the contract. Emitting a second `size` key beside
`sizeBytes` to satisfy a jq filter would install a second convention over a documented one, so
the wrong key is corrected in the harness and the finding is recorded rather than "fixed".
`story 56.2`'s `indexed_size` is genuinely unused by the listing — because
`stage::indexed_pointers` already hands `collect` the whole pointer, size included, and
re-deriving it per row would be a second index read. The one real inconsistency found is that
`MissingObject` (inside `remote.missing` of the same `--json` document) spells its byte count
`size`, which is exactly the trap the harness fell into — deferred, because renaming a
published key is not an unattended decision.

**Why the drain belongs in the engine and not in the CLI.** `drain_journal` is private and its
doc requires the caller to own the profile reservation. Reserving in `cmd_materialize` is
impossible (`materialize_entry` reserves for itself and `reserve` is not re-entrant), so the
request and the drain have to sit inside one function that holds one reservation:

```rust
let _reservation = self.reserve(&profile.id).ok_or_else(|| Busy(name))?;
let queued = self.materialize_held(&profile, &rela, path.clone(), WhenAbsent::Queue)?;
// … only when `queued.outcome == Queued`, with `unit` its row:
let mut previous = u32::MAX;
loop {
    // A drain that could not finish its bookkeeping must not discard a
    // delivery that already happened.
    if self.drain_kind(&profile, WorkKind::LFS_DOWNLOAD, source).await.is_err() { break; }
    // The primary exit: THIS row's fate, not the folder's queue depth.
    if !self.with_db(|c| db::unit_standing(c, unit))?.worth_waiting_for() { break; }
    // Anti-spin only: a requested row bumped out of a full batch deserves
    // one more pass; a queue making no progress does not.
    let outstanding = self.lfs_downloads_outstanding(&profile)?;
    if outstanding == 0 || outstanding >= previous { break; }
    previous = outstanding;
}
let settled = self.materialize_held(&profile, &rela, path, WhenAbsent::Report)?;
```

The second `materialize_held` is the whole reason the split is worth it: it is the *same*
publish path, so nothing is reimplemented — and it runs in **observing** mode, so it writes
nothing to the journal. During the drain `materialize_landed` normally publishes the labelled
path already (it is the arm that bypasses the virtual policy for a requested unit and re-stats
the index), in which case the second call reports `AlreadyMaterialized`; if the object landed in
the store but was not published, the second call publishes it. Either way this invocation
delivered the content, so the reported outcome is `Materialized` — `AlreadyMaterialized` means
"the worktree already had it before you asked", which is not what happened — with `unit_id` of
`None`, because after a successful drain there is no row left that will deliver anything.

**The reservation question, answered deliberately.** In-process — the app, and any future
supervisor sharing this engine — a `watch` holding the reservation makes
`materialize_entry_now` answer `SyncError::Busy` *before anything is queued*, which is the
existing sentence and the existing exit code. Cross-process it cannot: `Engine::reserve` is an
in-process map, so `keeper-syncd materialize` never sees `keeper-syncd watch`'s reservation
(already recorded in `deferred-work.md`, and `materialize_entry`'s doc says so out loud).
Draining anyway is safe rather than merely tolerated: `claim_ready` flips a row to `running`, so
a doubly-claimed `LfsDownload` re-fetches immutable content-addressed bytes into a store that
publishes by rename (`LfsStore::insert_verified`), and `stage::materialize` publishes the
worktree file the same way. The cost of the race is bandwidth, and it cannot corrupt.

**`dehydrate` and `pin` cannot queue, so the same question has no purchase on them.**
`dehydrate_entry` either releases the path inside its own reservation or returns a typed
`ContentRefusal`; `pin_entry` writes one ledger flag. Neither calls `enqueue_unique` — the only
`WorkKind::LfsDownload` enqueue in the crate is `materialize_entry`'s, and the only
`LfsUpload` enqueue is `commit_local`'s. Verified by reading every `SyncError` and `enqueue`
site in both functions.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` (with the `GIT_CONFIG_GLOBAL=/dev/null` identity prefix) -- **3553 passed, 0 failed**, against a 3548 baseline (+5: four engine tests, one CLI test).
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- clean.
- `cargo fmt --all` (from `src-tauri/`) -- applied, no further diff.
- `bash /tmp/vf-e2e.sh "$(pwd)/src-tauri/target/debug/keeper-syncd"` -- **10 passed, 0 failed**, up from 7 passed / 3 failed.
- Frontend: `bun run typecheck` clean, `bun run lint` 4 warnings + 1 info (baseline), `bun run test` at baseline. No frontend file is touched.

### D1 — the defect that was not one

There is no product change, so there is no first failure to record: the code was already
correct. What is recorded instead is the *evidence*, and a mutation proving the correctness is
already defended.

The harness's own baseline run printed the honest number and the `null` in the same output:

```text
== 4. ls-files reports the honest size and the state
    {"path":"scans/big.bin","state":"virtual","size":null}
    {"path":"small/tiny.bin","state":"materialized","size":null}
  FAIL big.bin reported as 'virtual null'

== 5. materialize lands real bytes on request, and the path stays clean
    media: queued  4.0 MB  scans/big.bin
```

`4.0 MB` on the human surface is `format_bytes(size_bytes)` over the same row. The `--json`
document that same run wrote (`jq` over `ls1.json`):

```json
{"path":"scans/big.bin","state":"virtual","sizeBytes":4194304,"oid":"3762e862…"}
{"path":"small/tiny.bin","state":"materialized","sizeBytes":65536,"oid":"1739b533…"}
```

The assertion was reading `.size`; the documented key is `sizeBytes`. Corrected in the harness
(`/tmp/vf-e2e.sh` step 4 only, original kept at `/tmp/vf-e2e.orig.sh`), and the cross-document
key inconsistency that made the mistake easy is in `deferred-work.md`.

Mutation, to show the honest size is guarded rather than accidental — `lfs/listing.rs`
`size_bytes: pointer.size` replaced with the worktree stat:

```text
test lfs::listing::tests::the_state_and_the_size_come_from_the_worktree_and_the_pointer ... FAILED
assertion `left == right` failed: the pointer's size is reported for all three, and the state is
what the worktree holds
  left:  [("media/away.mp4", Virtual, 132), ("media/gone.mp4", Absent, 0),  ("media/held.mp4", Materialized, 2048)]
  right: [("media/away.mp4", Virtual, 4194304), ("media/gone.mp4", Absent, 99), ("media/held.mp4", Materialized, 2048)]
```

`Absent` is settled by that same test and needs no new decision: its size is the pointer's,
because the row is about the object the path names and nothing on disk can answer for it.
Restored; `git diff` for `lfs/listing.rs` is empty.

### D2 — the failing test, before the fix

`a_one_shot_request_drains_what_it_queued_and_lands_the_bytes` run against the unmodified verb
(`materialize_entry`, no drain):

```text
running 6 tests
test a_one_shot_request_drains_what_it_queued_and_lands_the_bytes ... FAILED

---- a_one_shot_request_drains_what_it_queued_and_lands_the_bytes stdout ----
thread '…' panicked at crates/keeper-sync/tests/materialize_entry.rs:453:5:
assertion `left == right` failed: a one-shot invocation must report what it DID, and it did deliver
  left: Queued
 right: Materialized

test result: FAILED. 5 passed; 1 failed
```

### D2 — mutation sweep, after the fix and after the review amendment

Five mutations, each reverting exactly one decision, one test run per mutation, restored and
verified by reading `git diff` for the restored line plus SHA-256 against the pre-mutation copy.

**1. The drain loop deleted from `materialize_entry_now`:**

```text
test a_one_shot_request_drains_what_it_queued_and_lands_the_bytes ... FAILED
assertion `left == right` failed: a one-shot invocation must report what it DID, and it did deliver
  left: Queued
 right: Materialized
test result: FAILED. 5 passed; 1 failed
```

**2. The per-unit primary exit deleted, leaving only the profile-wide count** (the shape review
rejected):

```text
test one_request_does_not_drain_the_folders_whole_download_backlog ... FAILED
panicked at crates/keeper-sync/tests/materialize_entry.rs:663:5:
the request was satisfied on the first pass, so the loop had no reason to claim a second batch;
32 background rows were queued and none survived, which is the whole-backlog drain
test result: FAILED. 7 passed; 1 failed
```

**3. `drain_kind` swapped back to `drain_journal`** (no kind narrowing):

```text
test the_request_drains_transfers_and_leaves_every_other_kind_alone ... FAILED
panicked at crates/keeper-sync/tests/materialize_entry.rs:464:6
```

The panic is `unit_schedule`'s lookup finding **no row** — `db::complete` had deleted the `Push`,
which is to say the verb performed it. Sharper than the assertion that was written; the helper now
returns `Option` so the message says so out loud.

**4. The observing second pass swapped back to `WhenAbsent::Queue`:**

```text
test a_transfer_that_cannot_land_stops_without_cancelling_its_own_backoff ... FAILED
the backoff `reschedule_after` earned must survive the reporting pass; `promote_unit` would have
pulled it back to now (state=pending, not_before=1700000000000)
test result: FAILED. 7 passed; 1 failed
```

**5. The CLI's half** — `undelivered`'s `outcome != Queued` guard replaced with an unconditional
`return None`. Run before review renamed the test, so the name below is the pre-rename one; the
assertions it failed on are the ones the current
`only_an_undelivered_run_is_a_failure_and_each_standing_says_its_own_thing` still makes about a
`Queued` outcome:

```text
test commands::tests::only_an_undelivered_run_is_a_failure_and_it_says_why ... FAILED
panicked at crates/keeper-syncd/src/commands.rs:3404:10
test result: FAILED. 0 passed; 1 failed
```

### The exit-code path, on the shipped binary

Review's remaining gap was that nothing exercised `cmd_materialize`'s own wiring — `undelivered`'s
`Some` becoming a non-zero exit, and one JSON document on that path. Driven against the real
binary in the harness's kept tree, with the object removed from both the remote's and the clone's
object stores:

```text
$ keeper-syncd --json materialize media scans/big.bin ; echo rc=$?
{
  "code": "operational",
  "error": "media: scans/big.bin was not delivered — unit 8 is still owed (last recorded failure:
            integrity check failed for lfs object 84fb15b9…: expected 4194304 bytes at
            …/remote.git/lfs/objects/84/fb/84fb15b9…, got absent)",
  "exit": 1,
  "ok": false
}
rc=1
```

`json.loads` over the whole of stdout succeeds, which is the one-document claim: no
materialization document was printed beside the envelope. The journal's own reason is quoted, and
`stderr` carries the matching `WARN … the requested transfer did not land in this run unit=8`.

**Manual checks:**
- `keeper/src/sync_ipc.rs` is untouched, so the macOS gate has no new symbol to compile: no `SyncError` variant was added and no `ContentRefusal` variant was added, and `Engine::materialize_entry`'s signature, behaviour and check ordering are unchanged. The new public surface is `Engine::materialize_entry_now`, `Engine::unit_standing`, `db::unit_standing`, `db::UnitStanding` and `db::claim_ready_of_kind`; nothing in the shell crate calls any of them, and `db::claim_ready`'s and `Engine::drain_journal`'s existing signatures are unchanged, so their ~25 existing call sites are untouched.

## Auto Run Result

Status: done

**Implemented.** `keeper-syncd materialize <profile> <path>` now performs the transfer it queues
and reports honestly when it could not. `Engine::materialize_entry` was split into
`materialize_request` (the pre-reservation checks) and `materialize_held` (everything under the
reservation, in `WhenAbsent::Queue` or `WhenAbsent::Report` mode); the new
`Engine::materialize_entry_now` holds one reservation across the request, a download-only drain
bounded by the requested unit's own `db::UnitStanding`, and an observing second pass. The app's
door is byte-for-byte unchanged. D1 turned out not to be a product defect — every keeper surface
already reports the pointer's size for `virtual`, `materialized` and `absent`; the harness was
reading `.size` where the documented contract is `sizeBytes` — so it is recorded, mutation-proved
and the cross-document key inconsistency deferred. D3 is in `deferred-work.md` with its
reproduction.

**Files changed.**
- `src-tauri/crates/keeper-sync/src/engine.rs` — the split, `materialize_entry_now`, `drain_kind`/`drain`, `lfs_downloads_outstanding`, `unit_standing`, the `WhenAbsent` mode.
- `src-tauri/crates/keeper-sync/src/db.rs` — `WorkKind::LFS_DOWNLOAD`, `claim_ready_of_kind`, `unit_standing` + `UnitStanding`, a `kind` bind through `ready_rows`.
- `src-tauri/crates/keeper-sync/tests/materialize_entry.rs` — a filesystem-remote fixture and four tests: the drain lands the bytes; a dead transfer stops without cancelling its backoff; one request does not drain the backlog; the drain touches no other kind.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `cmd_materialize` awaits the draining verb, `materialize_lines` stops promising a supervisor, `undelivered` decides `$?` from the unit's standing.
- `docs/sync.md` — §13's `materialize` contract rewritten; `ls-files`' `sizeBytes` documented per state.
- `_bmad-output/implementation-artifacts/deferred-work.md` — the unborn-HEAD defect and the `size`/`sizeBytes` inconsistency.

**Review.** 12 findings after dedup across two layers: 6 bad_spec (one spec amendment and
re-derivation, converged in one pass), 4 patch, 2 reject, 0 intent_gap, 0 defer.

**Verified.** Rust 3553 passed / 0 failed (baseline 3548). `cargo clippy` on the three crates with
`-D warnings` clean. `cargo fmt` applied. Frontend at baseline: typecheck clean, lint 4 warnings +
1 info, 297 files / 4925 tests, and no frontend file is touched. `/tmp/vf-e2e.sh` **10 passed, 0
failed** — steps 4, 5 and 6 all pass, including the open-descriptor refusal that was unreachable
before. Five mutations, each caught by its owning test, each restored and verified by `git diff`
and SHA-256. The undelivered exit path proven on the shipped binary: exit 1, one JSON document,
the journal's reason quoted.

**Residual risks.** `Engine::reserve` remains in-process, so a `keeper-syncd materialize` and a
`keeper-syncd watch` can still drain the same journal concurrently; `claim_ready`'s SELECT-then-
UPDATE is not transactional, so a doubly-claimed download re-fetches immutable content-addressed
bytes into a store that publishes by rename — bandwidth, never corruption — and the `InFlight`
standing now reports the other half of that race honestly instead of calling it a failure. The
cross-process lock is pre-existing deferred work. The macOS gate has not been run from here: no
`SyncError` or `ContentRefusal` variant was added and no existing signature changed, so the shell
crate needs no edit, but only a build on the Mac can confirm it.
