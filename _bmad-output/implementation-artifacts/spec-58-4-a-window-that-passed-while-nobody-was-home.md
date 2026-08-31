---
title: 'Story 58.4: a window that passed while nobody was home'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: 'f8fbb90'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** A window that fell due while no host was home is served, or not, by an
accident of history rather than by a choice. `run_now` is what an ordinary restart does
(nothing rewrites the row, so the stored past window fires on the next tick); `skip` is what
`upsert_task`'s three service edges do (`db.rs:3050-3066` clears `next_due_ms` precisely so a
stale window cannot fire). The owner gets one or the other depending on which door the row last
came through, and *"run it after a delay"* has no analogue at all. `tasks` has ten typed columns,
no JSON blob (`db.rs:191-201`), and `ensure_task_columns` **does not exist** — only the DDL comment
demanding it (`db.rs:184-189`). `Action` is `{None, Arm, Run}` (`tasks.rs:293-300`) and cannot
express *skip*: returning `None` leaves the past window standing, so the next tick decides again,
forever.

**Approach:** One additive column, `on_missed TEXT NOT NULL DEFAULT 'run_now'`, added by a newly
written `ensure_task_columns`; read into `TaskState`; decided in `decide` against one named
boundary, `TASK_MISSED_GRACE_MS`; a fourth `Action` variant so *skip* re-arms **forward** through
its own compare-and-set write rather than through `arm_task`. Reachable from **both** writers in
this same story — a `--on-missed` flag on `keeper-syncd tasks set` and a control in
`src/components/sync/task-form.tsx`. The same change closes Wave 1's lost update
(`deferred-work.md:5044-5066`) with an `updated_ms` compare-and-set on `upsert_task`, because that
is the same table, the same write path and the same story that opens this write door.

## Boundaries & Constraints

**Always:**
- **No setting may enumerate more than one missed window** (AD-138, NFR-44). The window stays one
  `i64`; nothing anywhere counts elapsed windows.
- `run_now` reproduces today's behaviour **byte for byte**: its arm of `decide` is the existing
  `Some(at) if now_ms >= at => Action::Run`. No existing install changes meaning on upgrade.
- The `DEFAULT` is mandatory, not tidy: `upsert_task`'s `INSERT` names its columns
  (`db.rs:3142-3146`), so an older binary writing against a newer schema fails without it. Asserted
  by a test that writes the older column list by hand.
- `ensure_task_columns` is written on the `ensure_journal_columns` shape (`db.rs:429-432`): one
  column at a time, and the `PRAGMA table_info` statement **dropped** before any `execute` on the
  same connection. Called in `migrate` beside its three siblings (`db.rs:234-236`).
- An unreadable `on_missed` spelling is **skipped and listed** by `decode_task`, exactly as kind,
  mode and schedule already are (NFR-43, `db.rs:3019`).
- `db::arm_task` is **not** reused for the skip write. It is `WHERE id = ?1 AND next_due_ms IS NULL`
  because *"first sight can only happen once, so the statement says so"* (`db.rs:3256-3260`).
- The delay is enforced in **`decide`**, never at the claim: `claim_task`'s
  `next_due_ms <= now` condition (`db.rs:3303`) passes throughout the delay, so a claim-side guard
  could not implement it and a `Requested` trigger bypasses that condition entirely
  (`engine.rs:2215`).
- `delay` adds **no column**. Lateness is `now_ms - next_due_ms`, already on the row.
- `run_due_tasks`'s match (`engine.rs:2133-2148`) is **extended**, never defaulted with a `_` arm.
- The policy ships with its CLI flag **and** its form control in this story. Hard criterion.

**Block If:** the exactly-once invariant cannot be preserved under some setting; or a fix appears to
require a second `tasks` column for the delay.

**Never:** enumerate missed windows; reuse `arm_task` for a skip; enforce the delay at the claim;
re-implement in TypeScript any rule Rust already owns (the policy vocabulary is a frontend constant
list in the `TASK_KINDS`/`TASK_MODES` pattern, and nothing else); touch
`src/components/layout/tasks-pane.tsx` or its test (owned by `Story582`); touch
`_bmad-output/planning-artifacts/**`; add a `TaskOutcome` (that is 58.5); change what a
`Requested` trigger may claim (that is 58.6).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default on upgrade | a row written before the column existed | reads back `on_missed = run_now` after `ensure_task_columns`; behaviour unchanged | No error expected |
| Older write | `INSERT` naming only the ten original columns | succeeds; the row reads `run_now` | No error expected |
| Two-hundred-window absence, `run_now` | `every 5m`, clock advanced 200 intervals with no tick | **exactly one** run | No error expected |
| Two-hundred-window absence, `skip` | same, `on_missed = skip` | **zero** runs; `next_due_ms` moves **forward** of the clock; the next decision is not the same past window | No error expected |
| Two-hundred-window absence, `delay` | same, `on_missed = delay` | one run, immediately: the window has been open far longer than the grace, and the grace is a floor on how soon, not a postponement | No error expected |
| Fresh window, `delay` | window open for less than `TASK_MISSED_GRACE_MS` | `Action::None`; **no** run, although `claim_task`'s window condition already passes | No error expected |
| Fresh window, `skip` | window open for less than the grace | `Action::Run` — a present host serves its own window; `skip` only abandons one nobody served | No error expected |
| Hand-run during a delay | `tasks run` while the delayed window is open | runs at once, recorded as a requested run; the window is then consumed, so there is one run for one window (NFR-44) | No error expected |
| Unreadable policy | `on_missed = 'teleport'` written by a newer keeper | the row is listed as unknown with the reason; every other row still runs | Skipped, not fatal |
| CLI | `tasks set nightly --on-missed skip` | the row reads back with `onMissed: "skip"` in both renderings | clap refuses a spelling outside the three |
| Form | the policy control on an edit form | sent verbatim on `TaskSaveReq.onMissed` | Rust's refusal rendered unchanged |
| Stale form save | edit form open while another host moved the row | refused, naming the baseline that moved; nothing is overwritten | the refusal is rendered in the form that asked |
| Create | `id: ""`, `baselineUpdatedMs: null` | inserted; no precondition to check | No error expected |
| Baseline names a row that is gone | edit form open while another host forgot the task | refused, not resurrected | the refusal is rendered |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TaskMissedPolicy` beside `TaskKind`/`TaskMode`
  (same `as_str`/`from_stored` shape, `:120-178`); `TASK_MISSED_GRACE_MS` beside
  `MIN_SCHEDULE_INTERVAL_MS` (`:31`); `TaskState.on_missed` (`:280-289`); `Action::Skip` (`:293-300`);
  the three arms in `decide` (`:735-739`).
- `src-tauri/crates/keeper-sync/src/db.rs` -- `ensure_task_columns` on `ensure_journal_columns`'
  shape (`:432-445`), called at `:234-236`; `TASK_COLUMNS` (`:2834`); `StoredTask` (`:2933`);
  `read_task` (`:2960`); `decode_task` (`:2981`); `TaskRow` + `TaskRow::state` (`:2844-2873`);
  `upsert_task`'s `INSERT` column list and the baseline precondition (`:3082-3167`);
  `skip_task_window`, new, beside `arm_task` (`:3256`).
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `run_due_tasks`' match (`:2134-2145`);
  `skip_task_window`, new, beside `arm_task_window` (`:2155-2183`) and carrying the same
  load-bearing `warn`; `save_task`'s new baseline argument (`:7867`).
- `src-tauri/crates/keeper-core/src/tasks.rs` -- `TaskVm` gains `on_missed` and `updated_ms`
  (`:277-307`); `TaskSaveReq` gains `on_missed` and `baseline_updated_ms` (`:345-358`). Both
  `#[ts(export)]`; bindings regenerate on this host.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `task_vm` (`:1793`); `sync_task_save`'s `TaskRow`
  (`:2209-2222`) and its refusal path. **Shell crate: list every symbol for the macOS gate.**
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `TaskMissedArg` beside `TaskModeArg` (`:773`);
  `TaskSetArgs.on_missed` (`:617-652`); `cmd_task_set`'s `TaskRow` (`:3793-3808`);
  `cmd_task_set_enabled`'s (`:3829`); `task_lines` (`:3237-3255`); `task_json` (`:3355-3367`).
- `src-tauri/crates/keeper-sync/tests/release_sweep.rs` -- one `TaskRow` literal (`:544`).
- `src/lib/stores/sync.ts` -- `TASK_MISSED_POLICIES`, in the `TASK_KINDS` pattern (`:66-74`).
- `src/components/sync/task-form.tsx` + `.test.tsx` -- the policy `<select>` and the baseline the
  form seeds and sends.
- `dev/mock-shell.ts` -- seven `TaskVm` literals (`:1678-1799`) and `sync_task_save` (`:1868`).

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-sync/src/tasks.rs` -- add `TASK_MISSED_GRACE_MS = 15 * 60_000` with the sentence that
  gives it meaning (*how long a window may sit open before keeper concludes nobody was home*),
  `TaskMissedPolicy {RunNow, Delay, Skip}` with `as_str`/`from_stored`, `TaskState.on_missed`,
  `Action::Skip`, and the three arms of `decide` -- one boundary, two complementary settings, and
  `run_now` textually unchanged.
- [ ] `keeper-sync/src/db.rs` -- write `ensure_task_columns` and call it; carry `on_missed` through
  `TASK_COLUMNS`, `StoredTask`, `read_task`, `decode_task`, `TaskRow`, `TaskRow::state` and
  `upsert_task`'s `INSERT`; add the `Option<i64>` baseline precondition; add `skip_task_window` as a
  compare-and-set on the **observed** window.
- [ ] `keeper-sync/src/engine.rs` -- extend `run_due_tasks`' match with the `Skip` arm; add
  `skip_task_window`; thread the baseline through `save_task`.
- [ ] `keeper-core/src/tasks.rs` -- the four new wire fields, doc'd as `String` for NFR-43's reason.
- [ ] `keeper/src/sync_ipc.rs` -- project the two new `TaskVm` fields; read the two new
  `TaskSaveReq` fields; refuse an unreadable policy spelling before the engine, as kind and mode
  already are.
- [ ] `keeper-syncd/src/commands.rs` -- `--on-missed`, its `ValueEnum`, the `cmd_task_set` rule
  (omitted keeps stored; `run_now` on create), and the policy in both renderings.
- [ ] `src/lib/stores/sync.ts`, `src/components/sync/task-form.tsx` -- the vocabulary constant and
  the control, with a note accurate to `decide` and no re-implementation of anything.
- [ ] `dev/mock-shell.ts` -- the two new keys on every fixture, a policy that is not the default on
  at least one row, and a `sync_task_save` that refuses a moved baseline so the form's refusal path
  is drivable in the dev shell.
- [ ] `keeper-sync/src/tasks.rs`, `db.rs`, `engine.rs`, `keeper-syncd/src/commands.rs`,
  `src/components/sync/task-form.test.tsx` -- one test per matrix row above.

**Acceptance Criteria:**
- Given a store written by a keeper with no `on_missed` column, when `migrate` runs, then every row
  reads `run_now` and no row's behaviour changes.
- Given any of the three settings, when a host is absent across two hundred windows and then ticks,
  then the number of runs is one, one, or zero — never two, and never one per window.
- Given `skip` abandoned a window, when the next decision is taken, then it is about a **later**
  instant than the one abandoned.
- Given a `--on-missed` spelling clap accepts, when `tasks set` writes it, then `tasks list`,
  `tasks list --json`, `tasks status` and the app's edit form all read it back.

## Design Notes

**One boundary, two complementary settings.** `TASK_MISSED_GRACE_MS` is the whole of the policy's
arithmetic and it is a single number with a single meaning: *how long a window may sit open before
keeper concludes nobody was home.* `run_now` does not consult it. `delay` waits for it. `skip` gives
up at it. At the boundary the two non-default settings are exact complements — `delay` runs the
window, `skip` drops it — which is why one constant is honest where two would invite drift.

```rust
match state.next_due_ms {
    None => Action::Arm,
    Some(at) if now_ms >= at => match state.on_missed {
        // Byte for byte today's behaviour, which is why it is the default.
        TaskMissedPolicy::RunNow => Action::Run,
        TaskMissedPolicy::Delay if now_ms >= at.saturating_add(TASK_MISSED_GRACE_MS) => Action::Run,
        TaskMissedPolicy::Delay => Action::None,
        TaskMissedPolicy::Skip if now_ms >= at.saturating_add(TASK_MISSED_GRACE_MS) => Action::Skip,
        TaskMissedPolicy::Skip => Action::Run,
    },
    Some(_) => Action::None,
}
```

**Why `skip` must not fire on a fresh window.** The tick is 1 Hz, so without the grace a `skip`
task's window would be abandoned within a second of opening, on every window, forever — a task that
reports itself enabled and scheduled and never runs, which is the one shape this feature exists to
close (`engine.rs:2160-2172`). The owner's sentence is *"czekac na nastepny schedule **w takiej
sytuacji**"* — in *that* situation, meaning the one where nobody was home. The grace is what makes
"nobody was home" a fact the pure layer can read off one integer.

**Why the delay is a floor and not a postponement.** AD-139 fixes the anchor: *"not before
`next_due_ms + delay`"*, with no second column. Any formula anchored on the stored window is
therefore already elapsed for a sufficiently old one, so a window open for two hours runs at once
under `delay`. That is stated rather than papered over: `delay` guarantees *no sooner than*, which
is what protects a boot or a lid-open from firing housekeeping in the same second, and an
observation-anchored postponement would need either a column AD-139 forbids or a forward rewrite
that would make `claim_task`'s condition stop passing — and the passing condition is precisely the
fact this story has to prove.

**Why a hand-run during the delay consumes the window.** It is served: `next_task_window`'s
`Requested` arm re-arms an already-open window because *"that window has just been served and
writing it back would run the task again on the very next tick"* (`engine.rs:2276-2279`). Running
the policy's delayed run afterwards would be **two runs for one missed window**, which NFR-44
forbids outright. What must not happen is the opposite — the request being blocked, deferred or
relabelled — and it cannot be, because the delay lives in `decide` and a request never passes
through `decide`.

**Why the skip write cannot be `arm_task`, and what it is instead.** `arm_task` is `WHERE
next_due_ms IS NULL`, which a skip can never satisfy. The skip needs the same protection for a
different precondition, so it compare-and-sets the **window it decided about**:
`WHERE id = ?1 AND next_due_ms = ?2 AND ?3 > ?2`. A decision computed from a listing read earlier in
this tick then cannot clobber a window the other host has since moved, and the `>` makes the write
forward-only by construction rather than by the caller's care.

**Why the `updated_ms` compare-and-set lands here.** `TaskSaveReq` carries all six fields and
`upsert_task` is an unconditional `INSERT … ON CONFLICT DO UPDATE` with no version column, so a form
seeded once — deliberately, since re-syncing from the prop would overwrite what has been typed —
reverts every field another host moved while it sat open. Unlike `SyncProfileReq`, a task save has no
merge to hide behind. The guard is the NFR-43 stored-row refusal
(`db.rs:3113-3128`) applied to a row that *moved* rather than one that is *unreadable*: same rule,
same place, same rendered sentence. `None` means "no baseline to check" and is what every engine-
internal and CLI write passes, because those read the row and write it back inside one call rather
than across a person's typing.

**58.5 follows immediately, and this story is why.** A `skip` here writes one `info` line and
nothing else, so a declined window is still invisible in history. That is the invisible-non-execution
shape, and it is closed by the next commit rather than left standing — which is what makes 58.4→58.5
a strict chain rather than two independent stories.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd`
  (with the git identity prefix) -- expected: at or above the measured baseline of 3765 passed /
  0 failed, 0 failed.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd
  --all-targets -- -D warnings` -- expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml` -- expected: applied, and it parses the shell
  crate, which is this host's only gate on it.
- `bun run typecheck`, `bun run lint`, `bun run test` -- expected: clean, lint at baseline
  (4 warnings + 1 info), tests at or above 302 files / 5023 tests.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core` -- expected: regenerates
  `src/lib/ipc/gen/TaskVm.ts` and `TaskSaveReq.ts`, which are committed from that run and never
  hand-edited.

**Manual checks (if no CLI):**
- The `keeper` shell crate cannot link on this host (`gobject-sys`). Every symbol touched there is
  listed for the macOS gate: `task_vm`, `sync_task_save`.

## Auto Run Result

Status: done

**Rust:** 3776 passed / 0 failed (`-p keeper-sync -p keeper-core -p keeper-syncd`), against a
measured pre-story baseline of 3765 / 0 at `f8fbb90`. `cargo clippy` over the three crates with
`-D warnings`: clean. `cargo fmt` applied.

**Frontend:** `src/components/sync/task-form.test.tsx` 17 passed / 17 (14 before). `bun run
typecheck` and `bun run lint` clear of everything this story owns; the remaining diagnostics are all
in `src/components/layout/tasks-pane.*`, which `Story582` was mid-implementation on and owns. Lint
otherwise at baseline: 4 warnings + 1 info, 0 errors in this story's files. The full `bun run test`
was held at `Story582`'s request until their 58.3 lands.

**Bindings:** `src/lib/ipc/gen/TaskVm.ts` and `TaskSaveReq.ts` regenerated on this host by
`cargo test -p keeper-core` and committed from that run.

**macOS gate — shell-crate symbols touched:** `keeper/src/sync_ipc.rs::task_vm`,
`keeper/src/sync_ipc.rs::sync_task_save`. `cargo fmt` is this host's only local gate on that crate
and it parses.

**Mutation proof.** Five guards mutated one at a time, each owning test confirmed to fail, and every
restore verified by `md5sum` against the pre-mutation copy plus a re-read of the five sites:

| mutation | owning test that failed |
|---|---|
| `Skip if grace_over => Action::Run` | `each_missed_window_policy_decides_its_own_side_of_the_grace_boundary`, `a_skipped_window_is_declined_forward_and_the_task_runs_on_the_next_one` |
| `Delay => Action::Run` (no wait) | `a_delayed_window_waits_although_the_claim_would_already_admit_it`, and the pure boundary case |
| `ADD COLUMN on_missed TEXT` (no `DEFAULT`) | `the_missed_window_column_is_additive_and_its_default_is_todays_behaviour` |
| `skip_task_window` without `AND ?3 > ?2` | `declining_a_window_moves_it_forward_and_only_from_the_window_it_saw` |
| baseline mismatch refusal disabled | `a_save_whose_baseline_has_moved_is_refused_rather_than_reverting_the_row` |
