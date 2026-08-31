---
title: 'Story 59.9: a task that runs a verb keeper already owns'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '925bdf4'
final_revision: 'b710e6c'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** The owner asked for a task kind that runs a script. That request lands on a
recorded **Deferred** decision with a real, itemised bill —
`architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md:364-369`: *"a task
kind is one of keeper's own verbs, never a shell string … Revisit only with a stated threat
model."* The bill is specific rather than vague: every configured remote is a **disclosed** egress
destination, disclosed in `docs/egress.md`, which the release workflow diffs against the previous
tag — a user script can reach any host on the internet and that diff would show nothing whatsoever;
there is **no task timeout** anywhere (only the one-hour `TASK_LEASE_MS`); and there is **no stdout
capture**, so a command's output would be a line nobody wrote. Resolved with the owner on
2026-08-31 as *both, in that order*: this story ships the narrow half, and Epic 60 — whose first
story is the threat model, the egress answer and the timeout — ships the general one.

The narrow half is worth shipping on its own merits, not as a consolation. `TaskKind` had exactly
two variants and both **move bytes** (`tasks.rs:155-178` at baseline). Nothing keeper does
unattended could be *checked* on a schedule, even though the check exists, is read-only, and is
already the thing whose non-execution is hardest to notice: `keeper-syncd verify`'s own `virtual`
count exists because *"a row that is suppressed and reported nowhere is indistinguishable from a
check that stopped running"* (`commands.rs:1619-1624`). A check with no schedule and no memory has
exactly that defect at the level above.

**Approach:** One new variant, `TaskKind::Verify`, spelled `"verify"`, delegating to
`Engine::verify` — the verb `keeper-syncd verify` already runs — through a new
`Engine::perform_verify_task` whose target selection is `perform_release_task`'s line for line. No
command column, no args column, no shell-out, no timeout: the general exec kind is Epic 60 and
nothing here moves toward it. The vocabulary stays closed, and the price of that — somebody has to
write the arm — is stated in `TaskKind`'s own doc as the reason rather than apologised for.

## Why `verify` and not something else

The story required a verb that (a) needs no new machinery, (b) is safe unattended on a lease, and
(c) produces a `detail` line worth reading. `verify` is the only candidate that meets all three,
and it meets them by construction rather than by argument:

- **No new machinery.** `Engine::verify` (`engine.rs:11060` at baseline) is complete, public, and
  already driven by a CLI verb. It takes no reservation, asks no network, and opens the repository
  through `git::repo::open_read_only` *precisely* so it is safe beside a keeper commit — its own
  doc says *"a check that repairs what it is checking is not a check"*.
- **Safe unattended.** It writes no worktree file and adds no object to the store.
- **A detail worth reading.** `VerifyReport` carries `checked`, `bad` and `virtual_paths`, which is
  three numbers `sync` and `release` cannot be asked for.

Candidates examined and rejected: `doctor` (its checks are `keeper-syncd`-local — `/proc/sys/fs/…`,
`df`, `git --version` — and it lives in the daemon crate, not the engine, so a task kind over it
would be new machinery by definition); `ls-files` (a listing, not a check: it reports missing remote
objects and *deliberately* does not fail on them); `verify --remote` and
`republish_missing_objects` (one batch round trip per object against NFR-41's ten-thousand-path
fixture, and `--remote` would need an argument column this story does not have).

## Boundaries & Constraints

**Always:**
- The match in `Engine::perform_task` stays **exhaustive with no `_` arm** (`engine.rs:2505-2511`),
  which is the mechanism that made this story a decision rather than an inheritance.
- `update` stays refused in **all three** places, untouched and unweakened: `docs/sync.md`'s
  *"**`update` is not a task kind and never will be.**"* paragraph, `TaskKind::from_stored`
  returning `None` for it, and `perform_task`'s doc naming it as the kind with nowhere to be added.
- The arbitrary-command deferral is left **standing**. `TaskKind`'s doc now states its price
  explicitly — egress disclosure, no timeout, no stdout capture — so a later reader finds the
  reason next to the enum instead of only in an architecture document.
- The kind takes **no reservation**, deliberately. Consequence, stated rather than left implicit:
  `TaskOutcome::Busy` is unreachable for this kind. `tasks run --help`'s exit 4 remains true of the
  two kinds that can collide.
- `volume_ready` is asked **before** the walk. For this verb that is stronger than AD-48's usual
  argument: `verify` records an unreadable directory as a `bad` path, so a detached drive walked
  without the gate would fail *claiming the folder's content is damaged*.
- A `bad` path **fails** the run and the detail names the first one. `keeper-syncd verify` exits
  non-zero on the same finding; the record and the exit code may not disagree about one walk.
- Target selection is `perform_release_task`'s exactly — gone folder is `Failed`, paused is
  `Deferred`, `profile_id: None` is every enabled folder, no folders configured is `Ok`, all paused
  is `Deferred`.
- The `From<TaskKindArg> for TaskKind` bridge stays exhaustive, so an engine kind that never
  reaches the CLI is a compile error.

**Block If:** no existing keeper verb qualified — in which case the instruction was to report and
stop rather than invent work for the variant to do. `verify` qualified; nothing was invented.

**Never:** add a command, args, shell-out or timeout; add a `--remote` knob to the kind; take a
reservation for a read-only pass; propagate one folder's verify failure past the folders beside it;
touch `src/components/**` (this story needs nothing there — see the hand-off note below); touch
`_bmad-output/planning-artifacts/**`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Round trip | `TaskKind::Verify` | `from_stored(as_str())` is `Some(Verify)` | No error expected |
| Closed vocabulary | stored `"exec"`, `"run"`, `"Verify"`, `"verifies"`, `"update"` | `from_stored` is `None` | listed-not-run (NFR-43) |
| Unknown row on the tick | rows naming `update`, `teleport`, `exec` written past the typed door | all three in `listing.unknown`; none in `listing.tasks`; no run history for any | Skipped, not fatal |
| Host-wide run, clean folder | one enabled folder, two files, one subdirectory | `ok`, `"2 paths checked, 0 bad, 0 virtual in 1 folders, 0 could not be checked, 0 unavailable"` | No error expected |
| Damage found | a committed-shaped pointer whose object is not in the store, nothing authorizing it away | `failed`; detail carries the counters, the path and the oid | the run's failure notification fires, as for any failed task |
| Removable volume out | `removable`, `volume_id` naming an absent volume; both host-wide and folder-scoped | `deferred`, `"0 paths checked, 0 bad, … 1 unavailable"` — and **`0 bad`**, never one bad path per unreadable directory | absence, never failure (AD-48) |
| Named folder gone | `profile_id` naming nothing | `failed`, `"no such folder: {id}"` | No error expected |
| Named folder paused | `enabled = false` | `deferred`, `"{name} is paused, so nothing was checked"` | No error expected |
| No folders configured | empty profile list | `ok`, `"no folders are configured, so there is nothing to check"` | No error expected |
| Every folder paused | all `enabled = false`, host-wide | `deferred`, `"every folder is paused, so nothing was checked"` | No error expected |
| Verify could not run | a `.keepervirtual` that will not compile (FR-329) | counted in `could not be checked`, not in `bad`; the run fails; the folders beside it are still checked | contained, never propagated |
| CLI | `tasks set nightly-check --kind verify` | accepted; `tasks list` prints `verify`; `--kind --help` describes it | clap refuses any spelling outside the three |
| Documentation | §14's kind table | carries a `verify` row; the heading counts nothing | asserted mechanically off `TaskKindArg::value_variants` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` — `TaskKind::Verify` with its doc (why the verb
  needed no new machinery, why `Busy` is unreachable) and the paragraph on **why the vocabulary
  stays closed**, which is where the deferral's price now lives; the `as_str` and `from_stored`
  rows; the two existing kind tests extended (`an_unrecognised_stored_kind_including_update_…`,
  `every_stored_spelling_round_trips`).
- `src-tauri/crates/keeper-sync/src/engine.rs` — the `Verify` arm in `perform_task`'s exhaustive
  match; `perform_verify_task`, new, beside `perform_release_task`; the NFR-43 tick test extended
  with an `exec` row; three new tests
  (`a_verify_task_walks_the_folder_and_records_what_the_verb_found`,
  `a_verify_task_that_finds_a_bad_path_fails_and_names_it`,
  `a_verify_task_on_an_unplugged_drive_waits_rather_than_calling_it_damage`) and a `verify_task`
  fixture spelled as an override of `task`.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `TaskKindArg::Verify`; per-value `--help` lines
  for all three kinds (a bare `[possible values: …]` list stopped being self-explanatory at three);
  the `From` bridge arm; `every_kind_the_cli_offers_is_a_documented_row_and_the_heading_counts_nothing`,
  a docs guard driven off `TaskKindArg::value_variants`.
- `docs/sync.md` §14 — the kind table grows a `verify` row; the heading loses its count (*"The two
  kinds"* → *"The kinds"*); three paragraphs on the reservation, the record and the closed
  vocabulary; the `update` refusal paragraph left exactly as strong as it was.

**No generated bindings changed, and that was checked rather than assumed.** `TaskKind` is not
`#[ts(export)]`; `TaskVm.kind` and `TaskSaveReq.kind` are `String` precisely so *"a row a newer
keeper wrote must cross the wire"* (`TaskVm.ts:8-10`). No doc comment on a wire type was touched,
so `src/lib/ipc/gen/` is untouched by this story.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync/src/tasks.rs` — the variant, its two spelling rows, the closed-vocabulary
  paragraph, and both kind tests extended rather than duplicated.
- [x] `keeper-sync/src/engine.rs` — the arm, `perform_verify_task`, and the four tests.
- [x] `keeper-syncd/src/commands.rs` — the `--kind` value, its help, the bridge arm, the docs guard.
- [x] `docs/sync.md` — the row, the heading, and the paragraphs that say what the kind refuses.

**Acceptance Criteria:**
- Given a `verify` task and a folder with content, when the task comes due on the tick, then the
  engine performs `Engine::verify` and the run row carries that verb's own counters. **Met** —
  asserted in-process, and driven end-to-end through the shipped `keeper-syncd` binary.
- Given a stored kind this build cannot read, including `exec`, when the tick runs, then the row is
  listed and never run. **Met.**
- Given `update`, when it is looked for in any of its three refusals, then all three still refuse
  it. **Met**, and the docs half is now mechanically guarded.
- Given the ARCHITECTURE deferral on arbitrary commands, when this story is read against it, then
  it is unweakened. **Met** — no command, args, shell-out or timeout exists, and the deferral's
  price is now restated at the enum.

## Design Notes

### The volume gate, and why the release kind does not need what this kind cannot do without

**The mechanism.** `Engine::verify`'s walk is a `read_dir` over a stack of directories, and its
error arm does not abort the walk — it records the directory as **damage**:

```rust
let entries = match std::fs::read_dir(&dir) {
    Ok(entries) => entries,
    Err(err) => {
        report.bad.push((dir.display().to_string(), format!("unreadable: {err}")));
        continue;
    }
};
```

That is right for the verb: a directory inside a mounted folder that cannot be read *is* something
wrong, and a check that silently skipped it would be a check that stopped looking. It is wrong for
a folder that is not here at all. A removable volume that is out makes the folder's own root
unreadable, so the report comes back with the root as a `bad` path — and this story's own rule (a
bad path fails the run) would then turn a drive somebody unplugged into
`failed: /media/… unreadable`.

**Why `release` does not have this problem in the same way.** `perform_release_task` asks
`volume_ready` first too, but its argument is the ordinary AD-48 one: *"an unplugged volume is
absence, never failure"*, and the cost of getting it wrong is a **nuisance** — a nightly failure
notification every night the drive is out. `Engine::release_expired` does not manufacture a damage
report from an absent tree; it deletes nothing and reports nothing swept. For `verify` the cost of
getting it wrong is a **lie**, and a specific one: the run does not say *"I could not look"*, it
says *"your content is not what it should be"*.

**Why that matters more than the outcome code.** This is the defect class epic 58's review caught
twice — a surface stating a fact about the folder that the code could not support (a paced row
claiming a cadence nothing kept; a row saying nothing was paced while a vault was being pushed).
What makes this instance worse is what the person does next. `0 bad` on an absent drive means
*wait, plug it in*. `1 bad` means *reach for the backup*. A check is the one surface where a false
positive costs more than a false negative, because acting on it is destructive.

**The mutation.** Deleting the `volume_ready` match from `perform_verify_task` and letting the loop
call `self.verify` directly:

```
thread 'engine::tests::a_verify_task_on_an_unplugged_drive_waits_rather_than_calling_it_damage'
panicked at crates/keeper-sync/src/engine.rs:15977:13:
assertion `left == right` failed: absence is not failure, so nothing here is worth a
notification (None)
  left: Some(Failed)
 right: Some(Deferred)
```

`Some(Failed)` is the lie, observed rather than argued. The test asserts the whole detail string and
not only the outcome, precisely so that a future fix which returns `Deferred` while still counting
the root as a bad path does not go green.

**What the record says at that moment, in one line.** The outcome is `TaskOutcome::Deferred` — so
`next_task_window` retries within `TASK_RETRY_MS` instead of consuming the window, and the check
happens in the minute after the drive returns rather than tomorrow night — and the detail is
`"0 paths checked, 0 bad, 0 virtual in 0 folders, 0 could not be checked, 1 unavailable"`, which
**counts the drive rather than naming it**, and names no path at all. The counting is deliberate:
it is `perform_release_task`'s wording verbatim in that position (`{n} unavailable`), and target
selection is copied line for line from that method so that a reader comparing the two kinds finds
no difference to explain. For a folder-scoped task the *paused* case does name the folder
(`"{name} is paused, so nothing was checked"`), because that branch returns before the loop and has
exactly one folder to name; the unavailable case shares the loop with the host-wide shape, where the
honest answer is a count. Naming the volume there would be a genuine improvement to **both** kinds
and is therefore not this story's to make unilaterally.

### Two facts the detail line keeps apart on purpose

`0 could not be checked` and `0 unavailable` are different columns and never collapse. *Could not
be checked* is a folder whose `verify` returned `Err` — a `.keepervirtual` that will not compile is
the ordinary way (FR-329) — which fails the run, because a check that could not run is not a check
that passed. *Unavailable* is a folder that is not here, which defers. An operator who reads one
number where there were two cannot tell "something in that folder is misconfigured" from "that
drive is in a drawer", and those have opposite remedies. `cmd_verify` already keeps the same two
apart in its `--json` document, where an `Err` entry carries `error` and **no** `bad` array for
exactly this reason.

### The failure notification is inherited, not added

A failed `verify` run raises exactly one notification per onset, and this story adds no
notification path of its own to be wrong about it. `Engine::claim_and_run` calls
`note_task_outcome` for **every** kind, one statement before `finish_task_run`; the onset edge is an
`insert` into `Engine::task_faults` keyed by **task id**, so it is kind-agnostic by construction,
and `Deferred` neither raises nor clears — which is why a nightly `verify` on a drive that stays
unplugged notifies nothing, ever, rather than once a night. The existing guard is
`a_failing_task_notifies_once_per_onset_and_re_arms_only_on_a_success`, which drives 3 600 failures
through `note_task_outcome` and asserts one toast; it needed no extension for this kind, and
extending it would have asserted a property of the fault set twice.

The one place this reads differently is a **one-shot CLI invocation**: `keeper-syncd tasks run` is a
fresh process, so its fault set starts empty and a repeated hand-run notifies each time. That is a
property of the process boundary rather than of the kind, and it is why the rule lives in the
long-lived host where a schedule actually runs.

## Verification

Run in an isolated worktree carrying **only this story's hunks** (`git worktree add --detach` at
`408fc6e`, then a filtered patch), because three sibling stories were mid-flight in the same files
and `keeper-sync` did not compile in the shared tree.

- `cargo test -p keeper-sync --lib` — the six tests this story owns or extended: green.
- `cargo test -p keeper-syncd --bin keeper-syncd` — 133 passed, 0 failed.
- `cargo clippy -p keeper-sync -p keeper-syncd --all-targets` — no warnings.
- `cargo fmt -p keeper-sync -p keeper-syncd -- --check` — clean.
- The shipped binary, on a real fixture: `tasks set nightly-check --kind verify --mode manual` then
  `tasks run nightly-check` recorded `outcome="ok"` `detail="2 paths checked, 0 bad, 0 virtual in 1
  folders, 0 could not be checked, 0 unavailable"` and exited 0; adding a pointer with no object
  behind it made the next run `failed`, exit 1, with the path and the oid in the detail and the
  failure notification raised.

**Mutation proof.** Four inversions, each failing exactly one guard:

| Mutation | Test that failed | Message |
|---|---|---|
| `"verify" => Some(Self::Verify)` removed from `from_stored` | `every_stored_spelling_round_trips` | `left: None, right: Some(Verify)` |
| the bad-path `failure.get_or_insert_with` removed | `a_verify_task_that_finds_a_bad_path_fails_and_names_it` | `left: Some(Ok), right: Some(Failed)` — "damage found is a failed run, or the schedule reports that everything is fine about the one folder where it is not" |
| the `volume_ready` gate removed | `a_verify_task_on_an_unplugged_drive_waits_rather_than_calling_it_damage` | `left: Some(Failed), right: Some(Deferred)` — confirming the failure mode the gate exists for: without it a drive that is out reads as damage |
| heading restored to *"The two kinds"* / the `verify` table row deleted | `every_kind_the_cli_offers_is_a_documented_row_and_the_heading_counts_nothing` | "§14 must not count the kinds in a heading" / "§14's kind table must carry a row for `verify`" |

**This story's work ships in two commits, and the split is not this story's choice.** Four agents
wrote into one worktree, and three files ended up holding hunks from three different stories each:
`keeper-sync/src/engine.rs`, `keeper-syncd/src/commands.rs` and `dev/mock-shell.ts`. The
coordinator's ruling is that those three are shared by construction and get exactly one commit,
made by him, last, attributed to every story in it. So this story's commit carries
`keeper-sync/src/tasks.rs`, `docs/sync.md` and this spec; `engine.rs` (the `Verify` arm,
`perform_verify_task`, the four tests) and `commands.rs` (the `--kind` value, its help, the `From`
arm, the docs guard) ship in his.

**Consequence, stated because a later reader will need it and no test can tell them.** This
story's own commit **cannot build alone**, and the reason is precisely the mechanism the story is
built on: `Engine::perform_task`'s match is exhaustive with no `_` arm, so a `TaskKind` variant
without its arm is a compile error by design. The buildable unit is this commit **plus** the shared
one. Anybody splitting epic 59 into one-PR-per-story later must put `tasks.rs`'s variant and
`engine.rs`'s arm in the same PR; there is no ordering that avoids it, which is the same conclusion
the coordinator reached for the worktree.

**What was verified, and where.** Everything above was run against the complete change — both
files together — in the scratch worktree, because that is the only tree in which it compiles while
the siblings are mid-flight. Nothing in the list was run against the partial commit, and nothing in
it is claimed of one.

## Notes for the coordinator

**No `tasks-pane.tsx` hand-off is needed — but there is one frontend line, and it is not in a
component.** The pane *reads* `TaskVm.kind` as a widened `String`, so a `verify` row already
lists, renders and runs from the app with nothing changed. What the app cannot yet do is
**create** one: `TASK_KINDS` in `src/lib/stores/sync.ts:66` is the list of *"the spellings **this**
build can write"*, it reads `["sync", "release"]`, and the task form's kind picker is drawn from
it. Adding `"verify"` to that array is the whole of the wiring.

It was deliberately **not** done here. It is outside this story's file list, `task-form.tsx` was
being rewritten by a sibling while this ran, and its test may count the picker's options — so a
one-word edit landing mid-flight is a red suite for somebody else rather than a feature. Story
59.9's acceptance is *spellable from the CLI*, which is met and proven against the shipped binary.

For whoever wires it: `src/lib/stores/sync.ts:66`, `["sync", "release"]` becomes
`["sync", "release", "verify"]`, and nothing else — the constant's own doc explains why no derived
union type accompanies it and why an editable row's spelling is narrow by construction. Note that
`TASK_KINDS` is a hand-maintained mirror of `TaskKind::from_stored` with **no mechanical guard**
tying the two together; the guard this story added is on `docs/sync.md`, not on the TypeScript. A
frontend-side equivalent is a fair follow-up and is not in this story.

**Nothing found that contradicts the epic's triage.** The epic proposed `TaskKind::Verify` and the
verb it named is the one that qualified. Two details worth recording because the epic could not
have known them from a scout pass:

1. `verify` takes **no reservation**, which the epic did not mention and which makes `Busy`
   unreachable for the kind. That is a property, not a gap, and it is documented as one in three
   places (the variant, the method, §14).
2. `verify` reports an unreadable directory as a **bad path**, which makes the `volume_ready` gate
   load-bearing in a way the release kind's is not: without it a detached drive is reported as
   damaged content rather than as absence. The mutation table above records the observed failure.

## Review Triage Log
