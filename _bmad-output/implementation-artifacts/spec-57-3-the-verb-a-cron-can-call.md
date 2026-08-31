---
title: 'The verb a cron can call, and release becomes the first task that answers to it'
type: 'feature'
created: '2026-08-29'
status: 'done'
baseline_revision: '935ed181d6465c9c4488f362adec0819f5bb910c'
final_revision: 'c9f7d99f78355db31709160501f3fc7c04b2c96b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/planning-artifacts/epic-57-a-task-that-runs-when-it-should.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md'
warnings: ['multiple-goals', 'oversized']
---

<intent-contract>

## Intent

**Problem:** Story 57.1/57.2 built the record, the dialect, the lease and the public door
(`save_task`, `tasks`, `task_history`, `forget_task`, `run_task_now`) — and nothing outside the
crate can reach any of it. A headless box has no way to name, schedule, inspect or run a task, and
the owner's ask (*"usuwanie nie musi być automatyczne, może to być skrypt puszczany w odpowiednim
czasie"*) is still unmet because the one piece of housekeeping that deletes — Epic 56's release
sweep — is welded to the success edge of a sync with no way to move it, drive it or switch it off
(FR-349, FR-350, AD-136).

**Approach:** A `keeper-syncd tasks` subcommand — `list`, `status`, `run`, `set`, `enable`,
`disable`, `forget` — over that door, each with `--json` and an exit code exhaustive over
`TaskOutcome`; and `TaskKind::Release`, a second built-in kind that runs the very
`Engine::release_expired` the success edge runs, with the stored task's `mode` now governing the
success edge too.

## Boundaries & Constraints

**Always:**
- **No second implementation.** Every verb delegates to the engine door; the CLI selects, renders
  and maps an exit code, exactly as AD-52 requires of every other verb in this crate.
- **A task is not a privileged caller.** `tasks run release` reaches `release_expired`, so all five
  Epic 56 refusals, both AD-131 clocks, the pin, `RELEASE_BUDGET_OBJECTS`/`_BYTES` and 56.17's
  per-file `release_at_ms` apply identically. The *only* thing a task trigger bypasses is the
  hourly **look** gate (`release_is_due`) — because the task's schedule is what replaces it.
- **Default is today's behaviour, bit for bit.** No release task row ⇒ the success-edge sweep runs
  exactly as Epic 56 ships it. Nothing creates a row on migration, on open or on first tick.
- **Refusal, never coercion, with the input quoted** — the shape `select()` and
  `validate_quiet_time` already have. A malformed task id and an unknown task id are two different
  sentences, both `SyncError::Config` (exit 2).
- **One rule, one implementation.** The "what spellings can a task id have" rule moves into
  `tasks::validate_id` and is called by both `db::upsert_task` and the CLI selector.
- **`--json` is camelCase and is the contract**, matching `ls-files`, `verify`, `materialize` and
  `dehydrate` (`sizeBytes`, `profileId`); absent, never `null`, when a key means "nobody asked".

**Block If:** nothing. Every decision is derivable from AD-136, Epic 56 and this tree.

**Never:**
- Nothing in the `keeper` shell crate (wave 3 owns the Tasks view and the IPC).
- No `docs/sync.md` §13 (57.7 owns it), no systemd unit (57.7), no desktop host (57.5).
- No new dependency, thread, interval or timer; no `TaskKind::Update`, ever.
- No auto-created rows, no schedule defaults, no catch-up.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| list with an unreadable row | `kind = 'teleport'` written by hand | listed under `unknown` with the reason; exit 0 | never fatal (NFR-43) |
| run a task that works | `tasks run nightly` | one `task_runs` row identical in shape to a scheduled one; exit 0 | none |
| run a task that defers | drive unplugged / folder paused | `outcome: deferred`; **exit 4** | not a failure |
| run a task that is busy | reservation held, or lease held elsewhere | `outcome: busy` / `SyncError::Busy`; **exit 4** | not a failure |
| run a task that fails | remote unreachable | `outcome: failed`, `detail` quoted; exit 1 | `EXIT_FAILURE` |
| malformed id | `tasks run " "`, `tasks run "x "` | refused: "not a task id a keeper could ever have stored"; exit 2 | `Config` |
| unknown id | `tasks run nope` | refused: "no task matches `nope`; known tasks are: …"; exit 2 | `Config` |
| id of an unreadable row | `tasks run teleport-1` | refused naming the stored kind; exit 2 | `Config` |
| set a bad schedule | `--schedule "0 3 * *"` | refused at the write door, expression quoted; exit 2 | `Config` |
| set with no `--kind`, no row | `tasks set nightly` | refused: a new task must name its kind; exit 2 | `Config` |
| release, no task row | un-migrated `sync.db` | success edge sweeps exactly as before | none |
| release task `off` | mode `off` (or `enabled = 0`) | success edge releases nothing; `decide` never runs it; `run` refused | `Config` on request |
| release task `manual` | mode `manual` | success edge releases nothing; only `tasks run` releases | none |
| release task `scheduled` | mode `scheduled` | success edge releases **and** the schedule drives it | none |
| release task on absent media | removable volume detached | `deferred`, nothing deleted | AD-48 |
| locally authored, unconfirmed | `local_origin = 1`, `synced_at_ms IS NULL` | not a candidate under `tasks run release`, at any age | FR-341 |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TaskKind::Release`; new pure `validate_id`.
- `src-tauri/crates/keeper-sync/src/db.rs` -- `upsert_task` calls `tasks::validate_id`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `ReleaseTrigger`; `release_expired` takes it and
  returns a `ReleaseSweep` count; `release_governs`/`release_permits`; `perform_release_task`;
  the `Release` arm of `perform_task`; `mark_synced` passes `SuccessEdge`.
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` -- the three modes, the un-migrated
  default, and FR-341 under `tasks run release`, on the real repository fixture.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `EXIT_DEFERRED`; `Command::Tasks` +
  `TaskCommand`; `task_exit_code`; the pure renderers `task_json`, `task_run_json`, `task_lines`,
  `task_run_lines`; `select_task`; `run()` now yields the exit code.
- `src-tauri/crates/keeper-syncd/src/main.rs` -- takes the code `run()` returns.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync/src/tasks.rs` -- add `TaskKind::Release` (`"release"`) and `pub fn validate_id`
      carrying the empty/whitespace rule verbatim -- one vocabulary, one id rule, two callers.
- [x] `keeper-sync/src/db.rs` -- `upsert_task` delegates its two id checks to `tasks::validate_id`.
- [x] `keeper-sync/src/engine.rs` -- `ReleaseTrigger { SuccessEdge, Task }`; `release_expired`
      takes it, gates the success edge on the governing mode, bypasses `release_is_due` for a task,
      and returns `ReleaseSweep { released, reclaimed_bytes }`; `perform_release_task` (volume gate
      → `Deferred`; paused → `Deferred`; missing folder → `Failed`; first error → `Failed`);
      `Release` arm in `perform_task` -- the sweep gains a driver without gaining a second body.
- [x] `keeper-sync/src/engine.rs` -- unit-test governance: the mode table above, `enabled = 0`
      reading as off, per-profile beating host-wide, the least-permissive fold, an unknown row not
      governing, and a release task on absent media recording `deferred`.
- [x] `keeper-sync/tests/release_sweep.rs` -- the five 57.4 proofs on the real fixture.
- [x] `keeper-syncd/src/commands.rs` -- `EXIT_DEFERRED = 4`; the `tasks` tree with accurate help;
      `task_exit_code` as an exhaustive match over `TaskOutcome`; the pure renderers; `select_task`;
      `run()` returns `u8`.
- [x] `keeper-syncd/src/commands.rs` -- test: every verb parses, `--json` key sets field by field,
      the exit map, both refusals, the help text naming the exit codes, and an end-to-end
      `cmd_task_run` over a `TestPlatform` engine.
- [x] `keeper-syncd/src/main.rs` -- exit with the code `run()` returns.

**Acceptance Criteria:**
- Given a `sync.db` with no `tasks` rows, when a folder syncs successfully and its window is open,
  then exactly the content Epic 56 would have released is released.
- Given a release task in mode `off`, when the same sync succeeds **and** when its window comes due
  on the tick, then nothing is released by either path and no `task_runs` row is opened.
- Given mode `manual`, when the sync succeeds nothing is released, and when `run_task_now` is
  called the very same content is released.
- Given a path with `local_origin = 1` and `synced_at_ms IS NULL`, when `tasks run release` runs,
  then the file still holds its content and its ledger row is untouched.
- Given each `TaskOutcome` in turn, when `task_exit_code` maps it, then `Ok → 0`, `Busy → 4`,
  `Deferred → 4`, `Failed → 1`, `Abandoned → 1`, and adding a variant fails to compile.
- Given `tasks run <id>` on a healthy task, when it returns, then `task_runs` holds a row whose
  columns are indistinguishable from a scheduled run's except `host`'s pid and the untouched
  `next_due_ms`.

## Spec Change Log

## Review Triage Log

### 2026-08-29 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 14: (high 3, medium 7, low 4)
- defer: 1: (medium 1)
- reject: 1: (low 1)
- addressed_findings:
  - `[high]` `[patch]` `perform_release_task` drove a **deleting** sweep with no reservation, so a
    cron `tasks run release` could hold one folder's git index against `keeper-syncd watch`'s own
    pass. It now takes `Engine::reserve` per target for the whole sweep — the gate `tick_profile`
    and `sync_once` take — which also makes `TaskOutcome::Busy` reachable for this kind and
    `tasks run --help`'s exit-4 promise true.
  - `[high]` `[patch]` `release_governance` failed **open** on a `list_tasks` error, so a stored
    `mode off` stopped governing whenever the shared `sync.db` read failed and content was deleted.
    It now returns a `Result` and `release_permits` declines on `Err`.
  - `[high]` `[patch]` `tasks enable`, and `--mode scheduled` after a spell as `manual`, preserved
    a `next_due_ms` that had fallen into the past, so a month-disabled `@daily` release task fired
    a deletion on the next 1 Hz tick — catch-up, which this epic forbids. `db::upsert_task` now
    clears the window on both edges, beside the schedule-text edge it already had.
  - `[medium]` `[patch]` `release_permits`' placement stopped a vetoed folder *arming* a window but
    never cleared one already armed, so switching a task back to `scheduled` deleted on the very
    next sync — the failure the placement's own doc claimed to prevent. It now `remove`s the entry.
  - `[medium]` `[patch]` A sweep that released paths and then hit a hard error dropped its counters,
    so `task_runs.detail` said `released 0 paths` about a run that deleted content. The error path
    now carries them in a `SweepFailure`.
  - `[medium]` `[patch]` Every folder that declined before looking was counted as swept, so ten
    vetoed folders read exactly like ten swept ones; and a host-wide run whose folders are all
    paused reported `Ok`. `ReleaseSweep::looked` now separates them, the detail counts `declined`
    and `already syncing`, and a run where nothing looked is `Deferred`.
  - `[medium]` `[patch]` `tasks set --kind` repurposed a stored task, carrying its armed window (so
    a `sync` task became a `release` task that deleted at an instant armed for a sync) and its run
    history. A kind change is now refused, pointing at `tasks forget`.
  - `[medium]` `[patch]` `cmd_task_run` rendered its finished run against the clock sampled before
    the run, so a five-minute sweep printed as having started `now`. It renders against
    `now_ms.max(run.finished_ms)`.
  - `[medium]` `[patch]` The absent-media test's rationale — "only the host-wide branch reaches
    `volume_ready`" — was false, leaving the folder-scoped row the matrix names untested. Both
    shapes are now asserted.
  - `[medium]` `[patch]` `a_release_task_that_is_off_never_comes_due` passed with every line of this
    story deleted: it exercises 57.1's `decide`. Its doc now says so, and
    `a_second_row_switches_one_folder_off_under_a_host_wide_release_run` reaches `release_permits`
    on the tick with a control arm proving the folder is swept when nothing vetoes it.
  - `[low]` `[patch]` A task pass never recorded that a look happened, so the first sync inside the
    hour repeated the pass the task had just made. `note_release_look` records it.
  - `[low]` `[patch]` An unreadable row whose `id` column will not read rendered as a line starting
    with a bare colon. It now says what is unknowable about it.
  - `[low]` `[patch]` The baseline test's reclaimed-space arithmetic was forced by its neighbours
    and could not fail independently; it now asserts the absolute bytes left in the worktree.
  - `[low]` `[patch]` `release_expired`'s doc claimed a task trigger "skips the due gate and nothing
    else" while it skips both of that gate's rules. It now states the first-sight grace explicitly
    and why a one-shot process cannot honour it: `next_release_ms` is per-process, so a task that
    respected first sight would arm, decline, exit, and release never.

## Design Notes

**Why deferral needs a fourth exit code.** `sync_exit_code` has three answers and a cron wrapper
needs four: *worked* (0), *did not run, do not alert* (4), *failed, alert* (1), *fix your config*
(2). Folding deferral into 0 would make an external drive that has been unplugged for a month
indistinguishable from a nightly sweep that is working; folding it into 1 would page somebody every
night for AD-48's "absence, never failure". `EXIT_DEFERRED = 4` is additive — no existing verb can
return it — and 57.7's `RestartPreventExitStatus` will list it beside 2 and 3.

**`Busy` is a deferral here, though `SyncError::Busy` is 0 elsewhere.** `next_task_window` already
groups `Busy` with `Deferred` as "the run did not happen, do not consume the window"; the exit code
says the same thing. A lease held by the other host arrives as `Err(SyncError::Busy)` from
`run_task_now`, and `cmd_task_run` maps that one error to `EXIT_DEFERRED` too, so *every* "did not
run" answer this verb can give is one number.

**Governance: which row decides whether the success edge sweeps.**

```rust
// Per-profile beats host-wide; within a tier the least permissive wins, because
// the safe reading of two rows disagreeing about a deletion is the one that
// deletes less. `enabled = 0` reads as `Off`: a row that is not live must not
// leave a knob that does nothing.
match (governing_mode, trigger) {
    (None,            _)                     => true,  // default: Epic 56, unchanged
    (Some(Off),       _)                     => false, // off is off, both ways
    (Some(Manual),    SuccessEdge)           => false,
    (Some(Manual),    Task)                  => true,
    (Some(Scheduled), _)                     => true,
}
```

**An unreadable release *row* does not govern; an unreadable *table* stops the pass.** NFR-43 skips
the row, so the folder falls back to the default — today's behaviour, not a new one — because the
alternative reading would let one bad row silently stop housekeeping on a host. A `list_tasks`
**error** is the opposite case and the opposite answer: this pass does not know whether an operator
switched the sweep off, and the honest answer about a deletion is to decline. The next successful
sync sweeps, so it costs nothing.

**The task trigger skips the whole due gate, both of its rules.** `release_is_due` is a *look*
interval plus a first-sight arm-and-decline grace. The interval is skipped because a schedule
replaces it: honouring both would make the first scheduled run a silent no-op. The grace is skipped
because it **cannot** be honoured — `next_release_ms` is an in-process map and `keeper-syncd tasks
run release` is a fresh process every time cron starts one, so a task respecting first sight would
arm, decline, exit, and release never. A grace keyed to process lifetime cannot gate a scheduled
deletion without making the schedule a lie. A task pass records the look afterwards, so the next
sync inside the hour does not repeat it.

**A refusal clears the folder's look window.** Sitting above the due gate stops a vetoed folder from
arming; it does not clear an entry already there, and an armed window does not expire. Without the
`remove`, switching a task back to `scheduled` hours later meets an open window and deletes on the
very next sync — the failure the placement exists to prevent, and the one the zero-TTL arm already
`remove`s an entry to avoid.

**A task takes the reservation; the success edge inherits one.** `release_expired` deletes files and
rewrites the index, and its original caller runs inside the sync pass's own reservation, so NFR-42
was structurally true and nobody arranged it. A task is a second driver running outside any pass, so
`perform_release_task` takes `Engine::reserve` per target — which is also what makes
`TaskOutcome::Busy` reachable for this kind, and therefore what makes the exit-4 promise in
`tasks run --help` true rather than aspirational.

**A task coming back into service arms afresh.** `enabled` false→true and mode→`scheduled` both clear
`next_due_ms` at the write door, beside the schedule-text edge that was already there. Otherwise a
month-disabled `@daily` release task fires a deletion on the next 1 Hz tick — catch-up, which this
epic rules out by name.

**The record separates swept from declined.** Seven refusals inside `release_expired` answer `Ok`
without having looked at anything, so counting them as sweeps made "0 released from 10 folders" mean
either "nothing was due" or "everything refused". `ReleaseSweep::looked` carries the difference, the
detail counts `declined` and `already syncing` beside `unavailable`, and a run where nothing looked
is `Deferred` rather than a cheerful `Ok`.

**Rejected, with the reasoning kept:** moving the governance read behind the hourly look gate to
save a `SELECT`. `release_is_due` arms as a side effect of being asked, so asking it first would let
a vetoed folder's window advance while its sweep is switched off. The read sits behind four pure
checks, on the tail of a git sync pass that has just done network and disk I/O, over a table bounded
by how many tasks a person created.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, total at or above the 3667 baseline.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-syncd -p keeper-core` -- expected: no diff afterwards. The bare form (no `-p`) fails on this host with "Failed to find targets", which the shell crate's inability to build here explains; that is not something this story introduced.
- `bun run lint && bun run typecheck && bun run test` -- expected: unchanged from baseline; this wave touches no frontend file.

**Manual checks (if no CLI):**
- Each guard is mutated away in turn and the owning test must fail; the restore is verified by
  reading `git diff`, not from memory.

## Auto Run Result

Status: done

**Implemented.** Stories 57.3 and 57.4 together: `keeper-syncd tasks` as the verb a `cron` entry or
a systemd timer calls, and Epic 56's release sweep registered as the second built-in task kind with
modes off / manual / scheduled governing the success edge as well as the schedule. Nothing in the
`keeper` shell crate, nothing in the frontend, no new dependency, thread, interval or timer, and no
`update` task kind.

### Files changed

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TaskKind::Release`; `pub fn validate_id`, the one
  implementation of the task-id rule, shared by the write door and the CLI selector.
- `src-tauri/crates/keeper-sync/src/db.rs` -- `upsert_task` delegates the id rule, and clears
  `next_due_ms` on the two edges that bring a task back into service, so nothing catches up.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `ReleaseTrigger`, `ReleaseSweep`, `SweepFailure`,
  `release_governance`, `release_permits`, `forget_release_window`, `note_release_look`,
  `perform_release_task` (reserved per target), the `Release` arm of `perform_task`, and
  `release_expired` taking the trigger and answering with counts.
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` -- the un-migrated default, the three modes,
  and FR-341 under `tasks run release`, on the real repository fixture.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `EXIT_DEFERRED`, the `tasks` subcommand tree,
  `task_exit_code`, the pure renderers and envelopes, `select_task`, and `run()` answering with an
  exit code.
- `src-tauri/crates/keeper-syncd/src/main.rs` -- exits with the code the verb earned.

### Review findings

14 patched (3 high, 7 medium, 4 low), 1 deferred, 1 rejected — see the Review Triage Log. The three
high ones were all about a deletion happening when it should not: a sweep with no reservation, a
`mode off` that stopped governing when a database read failed, and a re-enabled task catching up.

### Verification

- Rust: **3703 passed / 0 failed** across `keeper-sync`, `keeper-core` and `keeper-syncd` (baseline
  3667, +36).
- `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`: clean
  (the only line is the pre-existing `proc-macro-error2` future-incompat note).
- `cargo fmt … --check`: clean. Frontend at baseline — 297 files / 4938 tests, typecheck clean, lint
  4 warnings + 1 info — and `git status` shows no file under `src/` touched.
- Every guard mutated away in turn; each owning test failed; each restore verified by reading
  `git diff` and by `cmp` against a pre-mutation snapshot.
- Smoke-tested end to end on the built `keeper-syncd` against a throwaway XDG root: `tasks set`
  derives its mode and refuses a bad schedule (exit 2) and a kind change (exit 2); `tasks run` on a
  paused folder exits **4** with `outcome: deferred`; a malformed id, an unknown id and an `off`
  task each exit 2 with distinct sentences; `tasks enable` leaves `nextDueMs` null.

### Residual risks

- `release_permits` reads the `tasks` table on every successful sync that gets past the four pure
  checks. Judged negligible against the git pass it follows, and the cheaper ordering was rejected
  for a stated reason — but it is the one cost this change adds to a hot path, and it is unmeasured.
- A task-triggered sweep does not honour the first-sight grace, by necessity (see Design Notes). A
  folder added minutes before a scheduled release task fires is swept without that hour of grace;
  every refusal that decides whether a byte may go still applies.
- `TaskOutcome::Abandoned` is unreachable from `tasks run`'s own run and is mapped by construction
  rather than by test-observed behaviour.
