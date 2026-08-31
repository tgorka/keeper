---
title: 'Story 58.9: the chapter grows a policy'
type: 'docs'
created: '2026-08-31'
status: 'done'
baseline_revision: 'c6f04fa'
final_revision: '7fbffff'
review_loop_iteration: 1
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
warnings: ['written-from-implementation']
---

<intent-contract>

## Intent

**Problem:** `docs/sync.md` §14 was written when a task had no missed-window
policy, no projected class beside it and one honest driver per host. Epic 58
shipped all three. §14's own status stamp (§18) already claimed *"the three-way
missed-window policy with a recorded outcome for the two settings that decline a
window"* was real, while the chapter never said what the three settings are, what
the default preserves, or what a declined window leaves behind — a chapter
claiming coverage it did not have.

Two shipped strings had the same defect in a worse place. Story 58.4 wrote
*"`delay` serves it no sooner than fifteen minutes after it fell due"*; the
review moved the anchor to the instant a host **noticed** the window and gave it
a separate, longer constant. `TASK_MISSED_DELAY_MS` and `tasks::decide`'s doc
were updated. `tasks set --help` and the ⌘8 form's note were not, so a wrong
number survived a review pass and a full gate — prose sitting on a clap enum is
far from the code it describes and nothing compared them.

**Approach:** §14 grows one subsection for the policy, stated from the code that
decides it, plus the two edges where nothing is recorded. Both stale strings are
rewritten and then **derived** from their constants by a guard, so the next
change to either number fails a test instead of shipping.

## Boundaries & Constraints

**Always:**
- **Every number in prose is computed from the constant that decides it.**
  `TASK_MISSED_GRACE_MS` and `TASK_MISSED_DELAY_MS` are the authorities;
  `tasks set --help` is asserted against them in Rust, and the form note's two
  TypeScript mirrors are asserted against the Rust source text itself.
- **Assert a number in its role, never merely present.** Written as two bare
  `contains("N minutes")` checks the help guard passed on a sentence that said
  *fifteen* minutes, because the clause contrasting it with the wrong anchor also
  mentioned thirty — the guard was satisfied by the very phrase warning against
  the error. The number and the instant it is measured from are one claim, so
  they are one assertion.
- **Under-claim over hedging.** Where the code records nothing — the
  compare-and-set losing to the other host, `skip` on a schedule with no next
  instant — the chapter says so plainly rather than softening the general claim
  into vagueness.
- The two declined spellings stay two ideas: a `declined` window will never be
  served, a `postponed` one will be, later.

**Block If:** a claim cannot be traced to code in this tree.

**Never:** restate a cadence, an interval or a policy number as a literal beside
the constant that owns it; document a control the app does not have; edit
`src-tauri/crates/keeper-sync/src/tasks.rs` (the constants are read, never moved).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Either constant changes | `TASK_MISSED_DELAY_MS` 30 → 45 min | `tasks set --help` guard fails **and** the form-note guard fails | The test is the alarm |
| Grace and delay made equal | both 15 min | the help guard refuses to run rather than passing vacuously | An assertion, not a skip |
| Reader asks what `delay` waits from | — | the noticing, not the window, and why anchoring on the window would be `run_now` | No error expected |
| Reader asks what a skipped night left | — | outcome `declined`, zero duration, detail naming policy, window and replacement | No error expected |
| Two hosts, one window | the other host moved it first | documented: **no record is written** | Not a failure |
| `skip` with no next instant | schedule exhausted | documented: window unreplaced, logged, nothing recorded, task will not run | Not a failure |
| Unknown stored policy | a row from a newer keeper | documented: listed but not run, never guessed as `run_now` | NFR-43 |

</intent-contract>

## Code Map

- `docs/sync.md` §14 -- gains *A window that passed while nobody was home*; the
  ⌘8 paragraph, the *Paced* rows subsection and the `--timer` guidance land in the
  same chapter. §18's status stamp is brought in line with what is now documented.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `TaskSetArgs::on_missed`'s
  doc comment (which **is** `tasks set --help`), plus the guard that computes its
  numbers from `keeper_sync::tasks::TASK_MISSED_{GRACE,DELAY}_MS`.
- `src/components/sync/task-form.tsx` -- `TASK_MISSED_GRACE_MINUTES`,
  `TASK_MISSED_DELAY_MINUTES` and `TASK_FORM_ON_MISSED_NOTE` composed from them.
- `src/components/sync/task-form.test.tsx` -- the mirrors asserted against the
  Rust source, and the note asserted phrase-by-phrase.
- `src-tauri/crates/keeper-sync/src/tasks.rs` -- **read only**: the two
  constants, `TaskMissedPolicy`, `TaskOutcome::{Declined,Postponed}`, `decide`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- **read only**:
  `move_task_window`'s detail composition and its two silent edges.

## Tasks & Acceptance

**Execution:**
- [x] `docs/sync.md` -- the missed-window subsection: the three settings in the
  owner's three words, both CLI and stored spellings, the 15-minute grace inside
  which all three run, the 30-minute delay anchored on the noticing, why the
  anchor moved, that no backlog accrues, and that `run_now` is the default
  because it reproduces pre-58 behaviour and is `Persistent=true` in-process.
- [x] `docs/sync.md` -- what a declined window records, the two spellings kept
  distinct, and the two edges that record nothing.
- [x] `docs/sync.md` -- the unknown-policy refusal (listed, not run).
- [x] `src-tauri/crates/keeper-syncd/src/commands.rs` -- rewrite `--on-missed`'s
  help to the anchor the code uses, and guard both numbers in their roles.
- [x] `src/components/sync/task-form.tsx` + test -- the note composed from named
  mirrors, and the mirrors pinned to the Rust source.

**Acceptance Criteria:**
- Given `docs/sync.md` §14, when a reader looks for what happens to a window
  that passed while nobody was home, then all three settings, the grace, the
  delay and its anchor are stated, and nothing in the chapter names a number that
  differs from the constant that decides it.
- Given either constant is changed, when the suites run, then
  `tasks_set_help_names_the_missed_window_numbers_its_constants_actually_use`
  fails in Rust and the form-note mirror test fails in TypeScript.
- Given a reader asks whether a skipped night is recoverable, then the chapter
  distinguishes `declined` from `postponed` and names the two cases in which
  nothing is recorded at all.

## Spec Change Log

- 2026-08-31 -- written during a salvage pass, from the implementation, because
  the session that wrote the code did not write a spec before it ended. The
  frontmatter carries `written-from-implementation` so nobody reads this file as
  a design that was reviewed before the work.

## Review Triage Log

**Pass 1 — 2026-08-31, author-verified, and that is a weaker thing than the
other two stories got.** The independent review lanes were unavailable (their
provider was out of credits), so this chapter was not read by a second party.
What was done instead, claim by claim, is stated here so the difference is
visible rather than implied:

- Every number was taken from the constant that decides it, and both prose
  surfaces are now asserted against those constants by a test rather than by a
  reader (`TASK_MISSED_GRACE_MS`, `TASK_MISSED_DELAY_MS`).
- The three settings, the default's compatibility claim and the
  `Persistent=true` reading were checked against `TaskMissedPolicy`'s own doc
  and `tasks::decide` (`keeper-sync/src/tasks.rs:252-285`, `:1005-1042`).
- `declined` / `postponed` were checked against `TaskOutcome` (`:435-450`) and
  the detail composition in `Engine::move_task_window`
  (`engine.rs:2297-2323`) — which is why the chapter says the detail names the
  policy, the window and the replacing instant rather than just "the instant".
- The **two silent edges** were found by reading that same function rather than
  by trusting the general claim: the compare-and-set losing to the other host
  (`:2334-2343`) and `skip` on a schedule with no next instant (`:2277-2292`)
  both write no row. A first draft of the paragraph said *both settings that do
  not run a window close a zero-duration run for it*, which those two edges make
  false; the paragraph now states them.
- The unknown-policy refusal was checked against `db.rs:3048-3052`, which turns
  an unrecognised spelling into an `UnknownTask` — listed, not run.
- The macOS paragraph the epic asks for was already correct in the chapter
  (`docs/sync.md:2521-2526`) and was left alone.

**What this pass cannot claim.** Nobody adversarial read the prose for the
defect an author is worst at seeing: a sentence that is true of this build and
false of a configuration the author does not run. `followup_review_recommended`
is therefore set, and the next review loop should take this chapter first.


## Verification

**Commands:**
- `cargo test -p keeper-syncd tasks_set_help` -- expected: green.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- expected: 20 green.
- Mutation proof: `TASK_MISSED_DELAY_MS` 30 → 45 minutes; expected: the Rust help
  guard and the TypeScript mirror test both fail; restore and re-verify green.
