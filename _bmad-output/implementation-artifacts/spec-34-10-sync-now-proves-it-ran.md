---
title: 'Sync now proves it ran'
type: 'feature'
created: '2026-07-28'
status: 'done'
baseline_revision: '5c40a22'
final_revision: '1ef0854eb2eda88a9036f7c5aa74e7216623decf'

---

<intent-contract>

## Intent

**Problem:** Three separate defects make a manual sync unprovable.

1. **The commit trailer lies.** `sync_once` takes a `SyncSource` and spends it on one
   `tracing::info!`; both `Provenance::new` sites hard-code `SyncSource::Watch`. So `git log` on a
   folder a person just synced by hand says `Keeper-Source: watch`, and a `keeper-syncd` cron says
   the same. `SyncSource::Bot` could never appear at all — no caller passes it.
2. **A successful pass often records nothing.** `last_sync_ms` was stamped at the tail of `do_push`
   and nowhere else, so a pull-only profile — and a push profile whose tree had nothing to send,
   which returns early before the stamp — never recorded that it had succeeded. The UI cannot then
   tell "never synced" from "synced, nothing to do".
3. **The click produces no statement.** `sync_folder_now` returns a full `SyncOutcomeVm` and both
   call sites discarded it, so a successful `Sync now` rendered nothing whatsoever. A pass that
   stages nothing finishes in milliseconds while the status poll runs at 2 s (10 s when idle) and
   the status does not move at all for "nothing to do" — so there was no second-hand evidence
   either. The user's report was "even after clicking Sync now I cannot see that sync works".

**Approach:** Thread `source: SyncSource` down the one call chain that reaches a commit —
`drain_journal` → `execute` → `do_pull` / `do_push` → `commit_local` → `commit` — with the
supervisor tick as the only caller that passes `Watch`, and derive `Bot` from the worktree lane.
Replace `do_push`'s stamp with `Engine::mark_synced`, called where a reconciliation leg genuinely
completed. Add `bytes` and a Rust-composed `line` to `SyncOutcomeVm`, and render that line inline at
both call sites in a `role="status"` region, with the destructive tone reserved for conflicts.

## Boundaries & Constraints

**Always:** The source is an explicit parameter, never ambient state read from a side map —
"what caused this commit" is exactly the question a hidden default silently gets wrong. The outcome
sentence is composed in Rust for the same reason `SyncStatusVm.line` is: the Sync view and the
Settings row both render it verbatim and cannot word one result two ways. The report appears from
the command's own return value, never from the status poll. Rust workspace lints (no `unwrap`);
TS at 2-space / 100 cols / double quotes / no `any`.

**Block If:** (none — the epic fixed every decision this story needed; the two it left open are
recorded under Design Notes and resolved from evidence in the code.)

**Never:** Do not report `pushed` / `pulled` as work done — they say which legs the profile's
direction allows, not that either leg carried anything, and "pushed" over a no-op push is the same
class of lie as `Keeper-Source: watch` on a manual sync. Do not stamp `last_sync_ms` on a bare
supervisor tick. Do not use a toast: no sync surface has ever used one. Do not touch
`provenance.rs`'s `change_subject` (34.5), `SyncProgressVm` (34.8), `SyncStatusVm` (34.9),
`collect_stable_changes`, `refresh_pending`, `Engine::pending` (34.9), or `formatCopyBytes` and the
copy card in `sync-pane.tsx`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Supervisor tick commits | `tick_profile` → `drain_journal(.., Watch)` | `Keeper-Source: watch` | — |
| `Sync now` commits | `sync_folder_now` → `sync_once(.., Manual)` | `Keeper-Source: manual` | — |
| `keeper-syncd sync` commits | `sync_once(.., Cli)` | `Keeper-Source: cli` | — |
| Merge commit, any caller | `do_pull`'s converge | the same source as the caller's, not `watch` | — |
| Unattended pass, worktree lane | `Watch` + `lane = Worktree` | `Keeper-Source: bot` (AD-50) | — |
| Manual pass, worktree lane | `Manual` + `lane = Worktree` | `manual` — a person really did ask | — |
| Journaled Pull unit succeeds | supervisor drains a `Pull` | `last_sync_ms` stamped | error → no stamp |
| Journaled Push unit succeeds | including "nothing to push" | `last_sync_ms` stamped | error → no stamp |
| Idle tick | nothing claimed, no scan due | `last_sync_ms` untouched | — |
| `sync_once` returns `Ok` | any direction, incl. pull-only | `last_sync_ms` stamped | `Err` → no stamp |
| Outcome: work happened | `files_changed > 0`, `bytes > 0` | `Committed and pushed 3 files, moved 2 KB.` | — |
| Outcome: pull only moved bytes | `files_changed = 0`, `bytes > 0` | `Moved 3 KB.` | — |
| Outcome: nothing to do | all zero, legs ran | `Nothing to sync — this folder already matches the remote.` | — |
| Outcome: conflicts | `conflicts` non-empty | names them, rendered `text-destructive` | — |
| Outcome: one file | `files_changed = 1` | `file`, not `files` | — |
| Pass failed | `sync_once` → `Err` | existing `actionError` path only; no `role="status"` at all | `syncErrorMessage` |
| Another row action | Pause / Remove / Edit | the previous report is cleared, never left stale | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` -- `tick_profile` (passes `Watch`), `drain_journal`,
  `execute`, `do_pull`, `do_push`, `commit_local`, `commit` (all gain a trailing
  `source: SyncSource`), the two `Provenance::new` sites, the new `Engine::commit_source` and
  `Engine::mark_synced`, and `sync_once`'s tail.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `SyncOutcomeVm` (`bytes`, `line`), the new `plural`,
  `NOTHING_TO_SYNC` and `outcome_line`, and `sync_folder_now`'s mapper.
- `src/lib/ipc/gen/SyncOutcomeVm.ts` -- the ts-rs binding, kept in step with the Rust type.
- `src/lib/stores/sync.ts` -- `syncProfileNow`'s contract (the outcome is the report).
- `src/components/layout/sync-pane.tsx` -- `SyncProfileCard`: the `outcome` state, its reset in
  `run`, and the `role="status"` paragraph in the card header.
- `src/components/settings/sync-section.tsx` -- `SyncProfileRow`: the same three.
- Tests: `engine.rs`'s test module, `sync_ipc.rs`'s test module, `src/lib/stores/sync.test.ts`,
  `src/components/layout/sync-pane.test.tsx`, `src/components/settings/sync-section.test.tsx`.

## Tasks & Acceptance

**Execution:**
- [x] `engine.rs` -- Add a trailing `source: SyncSource` to `drain_journal`, `execute`, `do_pull`,
  `do_push`, `commit_local` and `commit`, forwarding it at every internal call. `tick_profile`
  passes `SyncSource::Watch` with a comment saying it is the one genuinely watcher-initiated
  caller; `sync_once` forwards its own. -- The source reaches the commit instead of a log line.
- [x] `engine.rs` -- `Engine::commit_source(profile, requested)`: `Watch` on a `SyncLane::Worktree`
  profile becomes `Bot`; everything else passes through. Both `Provenance::new` sites call it. --
  All four arms of `SyncSource` can now arrive, and a manual sync on a lane still says `manual`.
- [x] `engine.rs` -- Delete the `last_sync_ms` stamp inside `do_push`; add `Engine::mark_synced`,
  called from `execute`'s `Pull` and `Push` arms on success and at the tail of `sync_once`. -- A
  pull-only profile, and a push with nothing to send, both record that they worked.
- [x] `sync_ipc.rs` -- `SyncOutcomeVm.bytes: u64` and `SyncOutcomeVm.line: String`; `outcome_line`
  composing the sentence from `files_changed`, `bytes` and `conflicts` only, with `NOTHING_TO_SYNC`
  for the empty case; `sync_folder_now` fills both. -- One wording, composed once.
- [x] `SyncOutcomeVm.ts` -- the matching ts-rs output, including the trailing space ts-rs emits
  before a field doc block.
- [x] `sync.ts` -- Document that the returned outcome is the report and why it may not wait for the
  poll. (`syncProfileNow` already returned it; nothing about the code changed.)
- [x] `sync-pane.tsx` + `sync-section.tsx` -- An `outcome` state per card/row, cleared at the top of
  `run` (any action makes the previous report stale), set from `syncProfileNow`, rendered as a
  `role="status"` paragraph beside the existing `actionError`, `text-destructive` when
  `conflicts.length > 0`. -- Every click says what happened, in both places, in the same words.
- [x] Tests -- `engine.rs`: a manual, a CLI and a supervisor commit read their trailer back out of
  the worktree's HEAD; the lane rule; `last_sync_ms` across publish / nothing-to-push / pull-only.
  `sync_ipc.rs`: all four sentence shapes and the "a leg that ran is not work" rule.
  `sync.test.ts`: the store hands the whole outcome back. Both component suites: the four cases,
  including `role="status"` and the conflict tone.

**Acceptance Criteria:**
- Given a folder synced from the app, when `git log` is read, then the commit says
  `Keeper-Source: manual`; the supervisor's own passes on the same folder still say `watch`.
- Given a `keeper-syncd sync`, when the commit lands, then it says `Keeper-Source: cli`.
- Given a pull-only profile, when a sync succeeds, then `last_sync_ms` is set.
- Given a supervisor tick that claimed no work and was not due a scan, then `last_sync_ms` is
  unchanged.
- Given `Sync now` on a folder with nothing to do, when it returns, then the row states
  "Nothing to sync — this folder already matches the remote." without waiting for a poll.
- Given `Sync now` that committed files or moved bytes, then the row names what happened.
- Given `Sync now` that produced conflict copies, then the statement is destructive-toned and says
  both revisions were kept.
- Given `Sync now` that failed, then only the error is shown and nothing claims a result.

## Design Notes

**Why a parameter and not ambient state.** Passes are already serialized per profile by
`Engine::reserve`, so the source could have ridden on the reservation and left every signature
alone. It does not, because provenance answers "what caused this commit to exist" and a value read
out of a side map has a default — and a default is precisely how this bug has already been shipped
once. Six private signatures grow one `Copy` argument and every call site states its answer out
loud. Cost at runtime: nothing (a one-byte enum by value).

**Why the worktree lane produces `Bot`, and why only when unattended.** The epic requires that all
four arms of `SyncSource` be reachable, and no caller passes `Bot`. `SyncSource::Bot`'s own doc says
"an autonomous agent writing into a worktree lane (AD-50)", and the lane is a profile property, so
the lane is the only place `Bot` can come from. It is applied to `Watch` alone: an unattended pass
on a lane is keeper couriering an agent's output, but a human who clicks `Sync now` on that same
lane really did cause the commit, and `Keeper-Source` records the cause.

**What counts as a successful pass.** `mark_synced` is called from exactly two places: a journaled
`Pull` or `Push` unit that returned `Ok`, and the tail of `sync_once`. Deliberately *not* from
`tick_profile`: a tick that claimed no work and was not due a scan checked nothing, and stamping it
would leave "Last synced" reading "just now" forever, which is a subtler version of the same lie
this story removes. A push that found nothing to publish *does* count — it ran the leg and it
succeeded, and "synced, nothing to do" is what the field then has to be able to express.

**Why `pushed` / `pulled` are absent from the sentence.** `sync_once` sets them from
`profile.direction`, not from whether either leg carried anything, so a folder with nothing to do
returns `pushed: true, pulled: true`. Rendering that as "pushed" would restate the trailer bug in
the UI. What is left — commits made, bytes moved, revisions kept aside — is the honest set, and when
all three are empty the answer is the "nothing to do" sentence, which is a result and not a failure.

**Why the sentence is composed in Rust.** Both surfaces already render `SyncStatusVm.line` verbatim
under a header comment saying why ("it is composed in Rust so the tray and this window can never
word the same state differently"). The outcome has the same two consumers and the same failure mode.
Composing it in Rust also sidesteps a real structural problem: the byte formatter the frontend would
need (`formatCopyBytes`) lives in `sync-pane.tsx`, which *imports from* `sync-section.tsx`, so
sharing it the other way would have meant either an import cycle, a duplicated formatter, or moving
a function out of a file another story in this batch is editing. `progress.rs::format_bytes` is
already public and already the family the pane mirrors.

**Why inline and not a toast.** Sonner exists in this app, but no sync surface uses it, and
`sidebar-pane.tsx` states the house rule for this family outright ("No toasts for connectivity,
ever."). The row already reports the result of its other explicit action — `Check files` — as an
inline sentence directly under the row, with a plain all-clear line for the empty case. `Sync now`
gets the same shape in the same place. `role="status"` announces it politely rather than
interrupting; conflicts take `text-destructive` but stay `status` rather than `alert`, because the
engine's own comment records that they are non-blocking by contract — both revisions survive.

## Verification

**Deliberately not run by this agent:** the build, `cargo test`, `cargo clippy`, `cargo fmt` and
`bun run check` were all left to the parent, per the batch constraint that five agents are editing
this worktree concurrently and the suite runs once at the end. Nothing here claims a green run.
(`cargo build` for the `keeper` crate does not work on this Linux box at all — tauri needs
GTK/glib-sys.)

**What was actually checked, by reading:**
- Every call site of the six re-signed functions was re-greped after the edits, and after the
  concurrent 34.5 / 34.8 / 34.9 edits landed in the same file: `drain_journal` (2), `execute` (1),
  `do_pull` (2), `do_push` (2), `commit_local` (3 production + 8 test), `commit` (1). None remains
  unmigrated, and `commit_local`'s test callers all pass `SyncSource::Watch`.
- `grep -E "SyncSource::(Watch|Manual|Cli|Bot)"` across `src-tauri/crates` now shows exactly two
  production literals outside `provenance.rs`: `tick_profile`'s `Watch` and `commit_source`'s lane
  rule. `keeper-syncd/src/commands.rs:818` still passes `Cli` and `sync_ipc.rs` still passes
  `Manual`; neither signature changed.
- `grep -n last_sync_ms` across the crate: exactly one assignment remains, inside `mark_synced`.
- Field-evaluation order in `sync_folder_now`'s struct literal is deliberate — `line:
  outcome_line(&outcome)` borrows and is written *before* `conflicts: outcome.conflicts` moves, so
  the borrow ends before the move.
- `keeper_sync::progress::format_bytes` is `pub` (`progress.rs:343`) and the module is already
  imported by `sync_ipc.rs`; `keeper_sync::engine::SyncOutcome` is `pub` with all-`pub` fields and
  `#[derive(Default)]`, which is what makes the `..SyncOutcome::default()` test fixtures legal.
- Byte figures in the assertions were chosen to be exact, not lucky: `format_bytes` uses binary
  units with `{:.0} KB` below 1 MiB, so 2 048 → `2 KB` and 3 072 → `3 KB` with no rounding to argue
  about.
- `SyncOutcomeVm.ts` was diffed by eye against `SyncActivityVm.ts` (the nearest neighbour with
  per-field doc comments) with `cat -A`, and matches its shape byte for byte including the trailing
  space ts-rs emits before a doc block — `bindings:check` compares the generated directory with
  `git status --porcelain`, so that whitespace is load-bearing.
- The JSX comment-inside-`&&` form used for the new paragraph is the same one
  `sync-section.tsx:284-288` already uses for its needs-attention `Alert`.
- `role="status"` appears nowhere else in `sync-pane.tsx` or `sync-section.tsx`, which is what makes
  the "a failed pass renders no `status` region" assertion meaningful rather than vacuous.

**Commands for the parent to run:**
- `bun run test:rust` -- expected: the three new `engine.rs` tests and the two new `sync_ipc.rs`
  tests pass, and ts-rs rewrites `SyncOutcomeVm.ts` to exactly the committed bytes.
- `bun run check` -- expected: biome + tsc + vitest pass, including the four new outcome cases in
  each component suite and the reworked store test.
- `bun run bindings:check` -- expected: clean, proving the hand-updated binding matches ts-rs.
