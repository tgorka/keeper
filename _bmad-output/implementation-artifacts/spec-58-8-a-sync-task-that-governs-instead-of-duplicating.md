---
title: 'Story 58.8: a sync task that governs instead of duplicating'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: 'ce9fc87'
final_revision: '1fb6db7'
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** `TaskKind::Sync` is a **second body with a second driver**, not a knob over the
existing one. `perform_sync_task` (`engine.rs:2542`) → `sync_once` (`engine.rs:8154`) runs a whole
pass, while `tick_profile` (`engine.rs:2939`) → `scan_due` (`engine.rs:3288`) → `drain_journal`
independently paces the same folder every `effective_poll_interval_ms`, and **nothing suppresses
`scan_is_due` when a Sync task exists**. So somebody who creates an hourly sync task for a folder
gets the hourly task *plus* the ordinary 15 s polling: duplicated work, and a run row reporting
*"1 synced"* for a folder the supervisor would have synced anyway. AD-141 names this exactly —
*"adding a driver without surrendering the gate ships a folder that syncs twice"* — and requires the
twin of `release_governance` that **surrenders the existing gate in the same change**.

**Approach:** Lift `release_governance`'s fold into one `task_governance(profile_id, kind)` shared by
both kinds, add `sync_governance` over it, and gate the **paced backstop only** on it: a
`scheduled` Sync task stands `scan_is_due` down and becomes the folder's clock, while the two
filesystem-event triggers (`watch_wake_pending`, `settle_window_elapsed`) keep working untouched.
Expose the verdict as `pub fn sync_governance_mode` so 58.7's projected scan row does not advertise
a cadence that has been surrendered.

## Boundaries & Constraints

**Always:**
- **`scan_is_due` is evaluated first and unconditionally**, exactly as its own doc requires: it is
  what advances the paced window, and short-circuiting it would let a governed folder starve its own
  backstop bookkeeping. Governance is asked *after* it fires, so the tasks table is read at most
  once per poll interval per folder rather than once per 1 Hz tick.
- **Only the paced walk stands down.** `drain_journal` still runs every tick with `scan_when_idle =
  false`, so queued units keep draining — standing that down would strand an in-flight upload.
- **A table this pass could not read permits the poll.** The deliberate inverse of
  `release_permits`, which declines on `Err` because the question there is *may I delete content*.
  Here the question is *may I walk a tree*, and the honest answer when `sync.db` is briefly locked is
  yes: the same rule `run_due_tasks` already encodes at `engine.rs:2126-2130` — *housekeeping that
  could stop a folder syncing would be a far worse bargain than housekeeping that occasionally does
  not happen* — read from the other end. Logged at `debug`, never raised.
- **A row this build cannot read governs nothing**, so the folder keeps today's pacing
  (`list_tasks` never surfaces it; the fold sees `None`). NFR-43, and the same direction
  `release_governance` argues: one hand-written or newer-keeper row must not be able to stop a
  folder syncing.
- Both routes keep taking `Engine::reserve`, so one git index still cannot be held twice in-process
  (`engine.rs:8161-8163` vs `:2950-2954`). Nothing here touches either reservation.
- No `tokio::time::interval`, no thread, no second due-gate: this **removes** a driver.

**Block If:** nothing. Both candidate answers to the poll question are decided below and argued in
Design Notes.

**Never:**
- Never let `off` or `manual` stop a folder's ordinary pacing. A sync task is a *schedule*; the
  folder's pause is `profile.enabled` and has its own control. See the matrix.
- Never stand down `watch_wake_pending` or `settle_window_elapsed` — a schedule cannot own a
  filesystem event (AD-141), and a folder that felt dead after saving a file would be a regression.
- Never `remove` the armed scan window on a decline. Unlike `forget_release_window`, there is
  nothing to protect: a scan deletes nothing, and leaving the window armed is what makes a declined
  folder re-ask governance once per poll interval instead of once per tick.
- No new column, no migration, no IPC verb of my own, no keeper-core type. Not `sweep_is_due`, not
  the notes cadence (deferred, AD-142).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| no sync task | no `Sync` row for this folder | `sync_governance` → `None`; paced backstop runs at `effective_poll_interval_ms`. **Today's behaviour, and the arm an un-migrated `sync.db` takes** | none |
| scheduled, this folder | `Sync` row, `mode = scheduled`, `enabled` | paced backstop **stands down**; the task's schedule is the folder's clock; watcher wake and settle window still open a walk | none |
| scheduled, host-wide | `Sync` row, `profile_id = NULL`, `scheduled` | the same, for every folder — the host tier, as in `release_governance` | none |
| manual | `Sync` row, `mode = manual` | poll **unchanged**: a manual task adds a button, not a clock | none |
| off, or `enabled = 0` | `Sync` row, `off` (or not live, read as `off`) | poll **unchanged**: an `off` schedule is not a pause on the folder | none |
| two rows, one tier | `scheduled` + `manual`, both this folder | least permissive wins → `manual` → poll unchanged | none |
| another folder's row | `scheduled` row naming a different `profile_id` | `None` for this folder | none |
| a `Release` row | `Release`, `scheduled`, this folder | `None`: a release task says nothing about the sync poll | none |
| unreadable row | `kind`/`mode` a newer keeper wrote | `None` → poll unchanged | listed as `unknown`, `debug` |
| table unreadable | `list_tasks` returns `Err` | poll **permitted**; `sync_governance_mode` → `None` | `debug`, never raised |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — the only file with production changes.
  `SWEEP_EVERY_MS:443` (widen to `pub` for 58.7); `tick_profile:2939`; `scan_is_due:3106`;
  `scan_due:3288`; `release_governance:8373` (fold to lift); `release_permits:8459` (the pattern,
  and the `Err` arm this one inverts); `sync_once:8154`; `perform_sync_task:2542`.
- `src-tauri/crates/keeper-sync/src/tasks.rs` — `TaskKind`, `TaskMode` (derives no `Ord`; the rank
  stays spelled where the claim is made). Unchanged.
- `_bmad-output/.../ARCHITECTURE-SCHEDULED-TASKS.md:303-339` — AD-141, the surrender requirement.

## Tasks & Acceptance

**Execution:**
- [x] `engine.rs` — `SWEEP_EVERY_MS` → `pub const` — 58.7's projected sweep row needs the real
      cadence from the shell crate and must not re-spell the literal.
- [x] `engine.rs` — extract `release_governance`'s body into
      `fn task_governance(&self, profile_id: &str, kind: tasks::TaskKind) -> Result<Option<TaskMode>>`;
      `release_governance` becomes a one-line call — one fold, one rank, two kinds, so the tier and
      least-permissive rules cannot drift between them.
- [x] `engine.rs` — add `fn sync_governance` over it, and
      `pub fn sync_governance_mode(&self, profile_id: &str) -> Option<tasks::TaskMode>` for 58.7,
      documented with the read-error caveat: a read error and a real `None` are the same value here,
      so nothing downstream may render them as different facts.
- [x] `engine.rs` — add `fn sync_poll_permits(&self, profile: &SyncProfile) -> bool` carrying the
      whole exhaustive matrix in its doc, and thread it into `scan_due` as
      `self.scan_is_due(profile) && self.sync_poll_permits(profile)`.
- [x] `engine.rs` tests — the four below (`sync_governance` fold, `sync_poll_permits` matrix, the
      one-driver-per-window count, the surviving filesystem triggers).

**Acceptance Criteria:**
- Given a folder with a `scheduled` Sync task, when an hour of 1 Hz ticks runs, then the paced
  backstop opens **zero** walks and the task's schedule fires its windows — one driver per window,
  not two; and with the task absent the same loop opens the full 240.
- Given the same governed folder, when the watcher delivers an event or a held file's settle window
  elapses, then `scan_due` is still true — the live half survives the stand-down.
- Given a `scheduled` Sync task and the `&& self.sync_poll_permits(...)` conjunct mutated away, then
  the duplication test fails.

## Review Triage Log

**Pass 1 — 2026-08-31, salvage.** This story's review never ran either: the
session ended with `review_loop_iteration: 0` and this section absent while
`1fb6db7` was already committed. Audited against the epic's own **58.8**
paragraph, claim by claim, read-only.

| epic claim | verdict | evidence |
| --- | --- | --- |
| every `Sync` row folds into one least-permissive mode over an **explicit** rank, spelled where the claim is made, with `TaskMode` deriving no `Ord` | satisfied | the `rank` closure is local to `task_governance` (`engine.rs:8442-8446`) and the fold is a min over it (`:8467-8470`); `TaskMode` still derives no `Ord` |
| the narrower statement wins over host-wide, and another folder's row is not folded in | satisfied | `engine.rs:8460-8473`, `mine.or(host_wide)`; `Some(_) => continue` for a third folder |
| the fold **modulates** `scan_is_due` rather than adding a driver beside it | satisfied | `scan_due` is `self.scan_is_due(profile) && self.sync_poll_permits(profile)` (`:3310`) — one conjunct on the existing gate, no second caller |
| a `scheduled` row must not leave the 15 s pacing running: **surrendered, not raced** | satisfied | `Some(Scheduled) => false` (`:8582`), and the hour-long tick test asserts `(0, 2)` where the pre-story shape was `(240, 2)` (`:15347`) |
| an `off` row must not silently stop a folder nobody asked to stop | satisfied | `Some(Off) => true` and `Some(Manual) => true` (`:8580-8581`), asserted per mode (`:15215`); a *disabled* row ranks as `Off` (`:8455-8459`), so forgetting to delete one cannot stop a folder either |
| a test asserts the negative directly | satisfied for drivers, **argued** for the run record | the test counts drivers over 3 600 ticks and deliberately elides `perform_task`. The run record's *"1 synced"* half is a consequence rather than an assertion: with zero paced walks there is no folder for the task to claim that the supervisor would have synced anyway. Asserting the bytes would need a real git fixture and would measure `perform_sync_task`, not this story's gate. Accepted as scoped, and named here so nobody reads the elision as coverage |
| 58.7's dependency: `sync_governance_mode(profile_id) -> Option<TaskMode>` with `None` on a read error | satisfied | `:8491-8493`, `sync_governance(..).ok().flatten()`; `sync_poll_permits` permits on `Err` (`:8566-8576`), so an unreadable table leaves the poll running and 58.7's row printing its interval is true |

**Failure edges audited beyond the claims, no findings.**

- *A mode changed mid-tick.* Governance is re-read on each fired window rather
  than cached, so staleness is bounded by one poll interval and the next window
  sees the new mode.
- *Governance stops applying.* Nothing is `remove`d: `scan_is_due` is asked first
  and arms unconditionally, so a forgotten or demoted task leaves an armed window
  at or behind `now` and the very next tick walks (`:8554-8564`).
- *Neither driver.* Unreachable by construction: the only arm that stands the
  poll down is `Some(Scheduled)`, which is a task with a schedule — and the
  filesystem triggers are untouched in every arm (`:3311`).
- *A host-wide `scheduled` sync row.* It governs every folder, so every folder's
  paced poll stands down in favour of one schedule. That is the tier rule working
  as written and matches `release_governance`'s shape; the watcher and settle
  triggers still answer a file somebody saved.

**Fixed elsewhere.** The audit of 58.7 (see that spec's triage log) found two
false claims in the *view* over this story's data, not in this story's gate.
Neither changes anything here.

## Design Notes

### The question, and why the answer is *narrow*, not *stand down* and not *continue*

`scan_due` is three independent triggers (`engine.rs:3288-3291`):

```rust
let paced = self.scan_is_due(profile);
paced || self.watch_wake_pending(&profile.id) || self.settle_window_elapsed(profile)
```

Only the **first** is a clock. The other two are filesystem events, and AD-141 says a schedule
cannot own them: *"`scan_due` is `paced || watch_wake_pending || settle_window_elapsed`, so two of
its three triggers are filesystem events a schedule cannot own."* So the folder's *poll* — the thing
a task schedule could honestly replace — is exactly `scan_is_due`, and nothing else.

Both wrong answers are real regressions, and naming them is the argument:

- **Stand the whole of `scan_due` down.** A folder with an hourly sync task would feel *dead* to
  somebody who just saved a file: the low-latency path AD-34-11 was written for is gone, and a
  settling file would wait out an hour instead of its 5 s window. The person asked for a schedule,
  not for keeper to stop noticing their disk.
- **Leave the paced poll running.** Then the schedule **governs nothing** — the folder is synced
  every 15 s regardless, the task's run row lies about having done it, and the story has shipped
  AD-141's forbidden shape: a driver added without surrendering the gate.

So: `scheduled` surrenders `scan_is_due` and keeps the two event triggers. That is
`release_permits`' own sentence transposed rather than contradicted — *"the schedule drives it **and**
the success edge keeps working"* becomes **the schedule drives it and the filesystem events keep
working**. What differs is which thing keeps working, and it differs because the two kinds'
pre-existing drivers differ: release has one idempotent body an extra driver cannot harm, while a
sync walk *is* the folder's whole function and running it twice per window is the defect.

### Why `off` and `manual` leave the poll alone, where release's `off` stops the sweep

`release_permits` reads `off` as *the sweep does not happen*, because there the task governs the
**only** driver of optional housekeeping. Here the pre-existing driver is the folder's basic
function, switched on by adding the folder — not by adding a task. Reading `off` as a pause would
let one forgotten `off` row silently stop a folder syncing, which the epic's own acceptance forbids
(*"an `off` row must not silently stop a folder the owner never asked to stop"*), and it would
duplicate a control that already exists and is visible: `profile.enabled`. `manual` follows for the
same reason — it adds a button, and a button takes nothing away.

### The placement, and why nothing is `remove`d

`scan_is_due` runs first and arms unconditionally; `&&` then asks governance only on the tick the
window actually fired. Two consequences, both wanted: the `list_tasks` read costs one query per poll
interval per folder instead of one per 1 Hz tick, and a declined folder re-asks its governance at
exactly that cadence. When governance stops applying — the task forgotten, switched to `manual` — the
armed window is already at or behind `now`, so the very next tick walks. That is `scan_is_due`'s own
first-sight promise (*"adding a folder does not appear to do nothing for a quarter of a minute"*)
without a `remove`, and it is why `forget_release_window`'s idiom is deliberately **not** copied:
that `remove` exists to stop a re-enabled sweep deleting immediately, and a walk deletes nothing.

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` — expected: ≥ 3783 + the new tests, 0 failed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd --all-targets -- -D warnings` — expected: clean.
- `cargo fmt` from inside `src-tauri/` — expected: no diff after.
- Mutation: delete `&& self.sync_poll_permits(profile)` from `scan_due`, re-run the duplication
  test, confirm it fails, restore, and confirm `git diff` shows the conjunct back.

## Revision 2 — 2026-09-01: the second gate PR #303 added

### What changed underneath this story after it shipped

`1fb6db7` decided *"does a scheduled sync task stand the folder's paced walking down"* in a world
with **one** gate over walking a folder. PR **#303** (`c0fd6fe`, *"floor the Pending poll's walk at
one a minute"*, merged as `427aea8`, this branch rebased onto it) added a second, independent one:

- `POLL_WALK_MIN_INTERVAL: Duration = Duration::from_secs(60)` (`engine.rs:428`)
- `poll_walked: Mutex<HashMap<String, Instant>>` (`engine.rs:1195`, built at `:1389`) — per profile,
  **in memory**, absent meaning *never walked by a poll in this process*
- `fn poll_may_walk(&self, profile)` (`engine.rs:11660`) — `false` when `!profile.enabled` or
  `first_checkout_is_unfinished(profile)`, otherwise `true` only when the last walk is at least
  `POLL_WALK_MIN_INTERVAL` old
- `fn poll_walk_finished(&self, profile_id)` (`engine.rs:11676`), called at `:11940` **after** the
  walk returns and on both outcomes, so the floor is a gap between *walks*, not between *polls*

From that merge until this revision, `sync_poll_permits` accounted for one of two gates. AD-137's
rule is that a claim on screen must be true, and AD-141's is that a row which claims to govern must
actually govern — so a governance reasoned against half the gates is the exact defect class both
exist to forbid, whichever way the second gate is decided.

### The decision, per gate, and it is deliberately asymmetric

| gate | what it stands in front of | verdict under `Some(Scheduled)` |
| --- | --- | --- |
| `scan_is_due` / `scan_due` (`engine.rs:3335`, `:3533`) | a **driver**: the walk that discovers work and hands it to the journal | **surrendered** — unchanged from `1fb6db7` |
| `poll_may_walk` (`engine.rs:11660`) | a **read**: `Engine::pending`'s status walk, for the Sync pane's Pending list | **left standing** — narrowed by nothing, and never consulted about governance |

**The distinguishing fact, verified rather than assumed.** `poll_may_walk` has exactly two
references in the whole tree: its definition (`engine.rs:11660`) and one production call site
(`engine.rs:11848`, inside `Engine::pending`). `Engine::pending` is `pub async fn` at
`engine.rs:11680` and is reached from exactly one place — `sync_pending`
(`keeper/src/sync_ipc.rs:1471`, `engine.pending(&id)` at `:1476`), a `#[tauri::command]`. No
`keeper-syncd` caller exists. That path takes no `Engine::reserve`, enqueues no journal unit, opens
no `task_runs` row and commits nothing: it builds a `Vec<PendingFile>` and returns it. `c0fd6fe`'s
own message says the same thing from the other side — *"Nothing about syncing changes: the commit
leg has its own cadence and is untouched."*

So the two gates are not two of a kind. AD-141's rule is about **drivers** —
*"adding a driver without surrendering the gate ships a folder that syncs twice"* — and the Pending
poll's walk cannot make a folder sync twice because it cannot make a folder sync at all.

### Both failure modes, named beside the decision

Required by the brief, and each is the other's cure:

1. **Standing only `scan_is_due` down** — the shape a reviewer would call *"a row that claims to
   govern and does not"*. It is real, and it is a **cost**, not a lie: somebody who put a large
   folder on an hourly schedule still watches it walked, up to once a minute per folder, for as long
   as the Sync pane is open. `startSyncDetailPolling` (`src/lib/stores/sync-detail.ts:273`,
   `SYNC_DETAIL_POLL_MS = 5_000` at `:102`) calls `refreshSyncDetailAll`, which reads **every**
   mirrored profile, and `sync-pane.tsx:770` starts it when the pane mounts. What makes it
   acceptable: it is floored at a minute by #303 itself, it happens only while a person is looking,
   and — the part that is this revision's obligation — `docs/sync.md` now says so in those exact
   terms rather than letting a *governed* row imply the folder is never touched.
2. **Standing both down** — the pane goes dead. The Pending list is where the work accruing between
   two scheduled windows is visible, and its **untracked** rows come from this walk and from nowhere
   else (`engine.rs:11943-11948`; the settling rows and the repair backlog are assembled before the
   walk is asked for). So the folder whose sync is hourly would be the folder whose pane says
   nothing is waiting. That is the same *feels dead to somebody who just saved a file* failure
   `a_governed_folders_filesystem_triggers_are_not_stood_down_with_its_poll` already forbids one
   layer down, moved from the engine to the pane.

### `Engine::reserve` is untouched

Both pre-existing paths still take it — `sync_once` at `engine.rs:8456`ff and `tick_profile` at
`:3168`ff — and this revision adds no path that takes or skips it. The Pending walk never took one
and still does not; what serialises *it* is `claim_walk` (`engine.rs:1415`), the same one-walk-per-
folder claim the commit leg takes.

### Tasks

- [x] `engine.rs` — `sync_poll_permits`'s doc grows the section *"The Pending pane's walk is a
      second gate, and it is deliberately left"*: the driver-versus-read argument, both failure
      modes, and a reference to the test that guards it. A declined option argued only in prose is
      what a later reader deletes as stale, so the argument and its guard are named together.
- [x] `engine.rs` — `poll_may_walk`'s doc records that governance is deliberately **not** a third
      shape that answers no, pointing at `sync_poll_permits` for the argument. The decision is
      spelled in the code as an *absence* of a call, and an absence is what a later "make the
      stand-down thorough" change removes without noticing.
- [x] `engine.rs` tests — `a_governed_folder_still_answers_the_pending_polls_walk`: ungoverned both
      gates open; governed the driver is surrendered **and** the read is not; the minute floor still
      bites after `poll_walk_finished`; a paused folder is walked by nothing. Both halves are
      required — without the first a governance that stood everything down would pass, without the
      second one that stood nothing down would.
- [x] No production behaviour change. The verdict for both gates is the verdict the code already
      gave; what this revision adds is that it is now a **decision** with a guard, rather than an
      unexamined consequence of #303 landing after the story.

### Acceptance Criteria

- Given a folder with a `scheduled` Sync task, when an hour of 1 Hz ticks runs through the real
  pacing, then the paced backstop opens **zero** walks — unchanged, and re-verified on this tree.
- Given the same governed folder, when the Pending poll asks, then `poll_may_walk` is **true**;
  and when a walk has just finished, then it is **false** until the minute is up.
- Given `poll_may_walk` mutated to consult governance, then
  `a_governed_folder_still_answers_the_pending_polls_walk` fails.

### Verification, this revision

**Scoped run, on this tree, with the change in:**

- `cargo test … -p keeper-sync` — **1116 lib tests passed, 0 failed**, plus every integration
  target green (28, 7, 4, 3, 2, 2, 1, 1, 1, 1; 0 failed anywhere).
- `cargo clippy … -p keeper-sync --all-targets -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean, exit 0.

**Mutation, both directions, because a one-sided mutation cannot test an asymmetric decision:**

| mutant | edit | result |
| --- | --- | --- |
| **A** — governance removed | `sync_poll_permits`: `Some(TaskMode::Scheduled) => true` | `a_scheduled_sync_task_is_the_folders_only_paced_driver` **FAILED**, `left: (240, 2) right: (0, 2)` — literally the pre-58.8 duplicated-driver world; `a_governed_folder_still_answers_the_pending_polls_walk` also FAILED on its first assert |
| **B** — governance over-applied | `poll_may_walk`: `\|\| !self.sync_poll_permits(profile)` added to the early-return condition | `a_governed_folder_still_answers_the_pending_polls_walk` **FAILED** on *"governed: and the READ is not"* |

**Restore verified by reading the diff, not from memory.** After restoring:
`grep -n MUTANT engine.rs` → no matches, and `git diff --numstat -- …/engine.rs` →
`125  0  src-tauri/crates/keeper-sync/src/engine.rs` — 125 insertions, **0 deletions**, i.e. purely
additive with no residue anywhere in the file.

### Sentences this revision falsified, and where they were corrected

None in `keeper-sync` or `keeper-core`: the governed row's sentence
(`PACED_SENTENCE_SCAN_GOVERNED`, `keeper-core/src/tasks.rs:832`) says the *paced backstop* has stood
down and that the watcher still brings a look forward, which stays true under this decision. The
corrections are in `docs/sync.md` and are carried by 58.9's revision — see
`spec-58-9-the-chapter-grows-a-policy.md`, *Revision 2*.
