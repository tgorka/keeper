---
title: 'Story 58.9: the chapter grows a policy'
type: 'docs'
created: '2026-08-31'
status: 'done'
baseline_revision: 'c6f04fa'
final_revision: '4c6be3f'
review_loop_iteration: 2
followup_review_recommended: false
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

## Revision 2 — 2026-09-01: what PR #303 and Epic 59 made the chapter owe

### Why the chapter needed another pass

`7fbffff` wrote §14 against the tree as it stood on 2026-08-31. Two things landed
after it and neither is cosmetic:

1. **PR #303** (`c0fd6fe`, merged `427aea8`) floored the Pending poll's full
   status walk at one a minute. That is a second, independent gate over walking a
   folder, and it changes both what a person sees in the Pending list and what
   the word *governed* can honestly claim. Untouched, §14's *Paced* section
   promised an explanation of *governed* — *"see below"* — that the chapter did
   **not** contain.
2. **Story 59.6** gave each task its **own** missed-window delay. The `delay`
   table row still read *"serves it 30 minutes after a host noticed it
   (`TASK_MISSED_DELAY_MS`)"* as though the constant were the only answer, which
   after 59.6 is false for any task that carries one.

A third item was owed rather than falsified: the exactly-once rule was described
in the chapter only as `run_now`'s behaviour and as a systemd analogy, never as
the **safety** property it is, and never with the reason it is one.

### What changed in `docs/sync.md`

| where | change |
| --- | --- |
| §12, *The Pending list runs in both directions* | new: the list refreshes every five seconds but the walk behind it is floored at one a minute, measured between walks; what the walk contributes (untracked rows and a fresh verdict for paths the watcher never saw) versus what does not need it; the visible cost, the measured reason, the paused / unfinished-first-copy exclusions, and the closing sentence that **nothing about syncing is paced by that floor** |
| §14, *Paced* standings table | the **governed** row said *"has taken this folder's paced walk over"* — corrected to *"paced sync poll"*, and its dangling *"see below"* now names a section that exists |
| §14, new `#### What a scheduled sync task does to a folder's polling` | 58.8's decision in a person's words: the schedule **replaces** the backstop poll; `off` and `manual` take nothing away; the two things deliberately not stood down with it — the watcher/settle window, and the Sync pane's own status walk — and the honest small print that *governed* means the paced **sync** has stood down, not that nothing ever looks at the folder |
| §14, `on_missed` table | the `delay` row now reads *"a delay after a host noticed it — thirty minutes unless the task carries its own"* |
| §14, after the `run_now` paragraph | three new paragraphs: the per-task delay and its two flags; the fifteen-minute floor with its reason and the one-year ceiling; and exactly-once as a safety property, with the mechanism and the `release`-deletes-content argument |

**Nothing was renumbered.** The only structural addition is one `####` inside
§14, so every section number in the file is unchanged. Re-resolved anyway, line
by line: `docs/sync.md`'s own in-text references are `§4`, `§9`, `§12` and `§13`
(→ *Only complete files*, *Virtual files*, *Progress and warnings*, *`keeper-syncd`*),
all correct, and the `§12` this revision adds points at the section that now
carries the walk-floor paragraph. `docs/decisions.md` carries exactly three
references into this file — `docs/sync.md §4` (`decisions.md:73`),
`docs/sync.md §13` (`:98`) and `docs/sync.md §14` (`:109`) — and all three still
resolve to `## 4.`, `## 13.` and `## 14.` respectively.

### The cross-check: every verb, flag, field, outcome word, constant and exit code

Checked against the code, not against the epic — Story 56.13 shipped a `--help`
describing replaced behaviour and Epic 58 shipped a *"fifteen minutes"* string
against a thirty-minute constant, and both survived review because prose sits far
from code.

| the chapter says | verified at | verdict |
| --- | --- | --- |
| kinds `sync`, `release`, `verify` | `keeper-sync/src/tasks.rs:228-230` (`as_str`), `:238-240` (`from_stored`) | correct, and the set is closed — no `update` value exists to write |
| modes `off`, `manual`, `scheduled` | `tasks.rs:267-269`, `:277-279` | correct |
| policy spellings `run_now`, `delay`, `skip` stored; `run-now` on the command line | `tasks.rs:344-346`, `:356`; `commands.rs:695-696` (`#[arg(long, value_enum)] pub on_missed`) | correct, and the kebab/underscore divergence is exactly where the chapter says it is |
| outcome words `ok`, `busy`, `deferred`, `failed`, `abandoned`, `declined`, `postponed` | `tasks.rs:482-488` | all seven correct; `declined` and `postponed` are distinct variants, not shades |
| `--missed-delay <MINUTES>` and `--no-missed-delay` | `commands.rs:713-714`, `:722-723` (`conflicts_with = "missed_delay"`) | correct, including that the argument is **minutes** |
| `--description <TEXT>` and `--no-description` | `commands.rs:733-734`, `:740-741` | correct |
| `--timer` on `tasks run`, and the shipped unit passing it | `commands.rs:578` (`timer: bool`); `packaging/keeper-syncd-tasks@.service:118` — `ExecStart=%h/.local/bin/keeper-syncd tasks run --timer %i` | correct |
| `[on missed: delay 45m]` in the row bracket, minutes when they divide and raw ms otherwise | `commands.rs:3445-3453` | correct, including that `run_now` renders **nothing** |
| `name: <text>` printed under the row, `--json` key `description` | `commands.rs:3470-3471`, `:3595` | correct |
| `--json` key `missedDelayMs` | `commands.rs:3600` | correct |
| grace **15 minutes**, `TASK_MISSED_GRACE_MS` | `tasks.rs:66` — `15 * 60_000` | correct. This is the number Epic 58 once got wrong in prose; it is right now, and re-checked rather than assumed |
| default delay **30 minutes**, `TASK_MISSED_DELAY_MS` | `tasks.rs:102` — `30 * 60_000` | correct |
| delay floor = the grace period, ceiling = one year, both refused rather than clamped | `tasks.rs:1103-1130` (`validate_missed_delay_ms`), ceiling shares `MAX_SCHEDULE_INTERVAL_MS` `tasks.rs:42` — `366 * 24 * 60 * 60 * 1_000` | correct; "one year" is 366 days in the code, which the chapter's wording does not contradict |
| lease **one hour** | `engine.rs:532` — `TASK_LEASE_MS: i64 = 3_600_000` | correct |
| history capped at **50 per task**, the view's read asks for **20** | `db.rs:2891` — `TASK_RUNS_CAP: usize = 50`; `keeper/src/sync_ipc.rs:1745` — `TASK_HISTORY_LIMIT_DEFAULT: u32 = 20` (max 200 at `:1748`) | correct |
| exit **2** for *no such task* / configuration, exit **4** for *did not run, nothing wrong* | `commands.rs:63` — `EXIT_CONFIG: u8 = 2`; `:87` — `EXIT_DEFERRED: u8 = 4` | correct |
| the timer ships `OnCalendar=daily`, `Persistent=true`, `RandomizedDelaySec=3600`; the service ships `Restart=on-failure` / `RestartSec=60` (the systemd-244 requirement) | `keeper-syncd-tasks@.timer:72`, `:78`, `:84`; `keeper-syncd-tasks@.service:139-140` | all correct |
| schedule floor 60 s, refused not clamped | `tasks.rs:31` — `MIN_SCHEDULE_INTERVAL_MS: i64 = 60_000` | correct |
| the Pending walk floor is **one minute**, between walks | `engine.rs:428` — `POLL_WALK_MIN_INTERVAL = Duration::from_secs(60)`; `poll_walk_finished` at `:11676` called at `:11940` **after** the walk returns | correct |
| the Pending poll's own cadence is **five seconds**, over **every** mirrored folder | `src/lib/stores/sync-detail.ts:102` — `SYNC_DETAIL_POLL_MS = 5_000`; `:273` `startSyncDetailPolling` → `refreshSyncDetailAll` (`:255-260`, every profile in the store); started at `src/components/layout/sync-pane.tsx:770` | correct — and this is the claim the first draft of the sentence got wrong, saying *the folder you are looking at* |
| the walk is skipped for a paused folder and for one whose first copy never finished | `engine.rs:11660-11663` (`poll_may_walk`) | correct |
| the measured motivation: 155 625 entries, a walk every ten to thirty seconds | `engine.rs:410-427` (`POLL_WALK_MIN_INTERVAL`'s own doc) | correct |

**Two findings, both already corrected above rather than filed:** the `delay`
table row (falsified by 59.6) and the *governed* row's promise of an explanation
that did not exist (owed since `c6f04fa`, and made materially misleading by
#303).

**Checked and found already true, so left alone:** nothing in the file claims the
Tasks view cannot create a task (`grep -n "cannot create"` → no matches; the
⌘8 section at `docs/sync.md:2264-2299` describes create, edit, forget, Run now
and Runs); nothing claims a missed window has no policy; and nothing claimed a
poll walks on every tick — §12 simply had no sentence about the walk's cadence at
all, which is why this revision adds one rather than repairing one.

### Verification, this revision

- The cross-check table above **is** the verification for a documentation change:
  every row was read out of the named file at the named line during this pass.
- `docs/decisions.md`'s three `docs/sync.md §N` references re-resolved by hand
  against `grep -n "^## [0-9]" docs/sync.md`; all three land on the intended
  headings, and no heading moved.
- The code half of this revision lives in
  `spec-58-8-a-sync-task-that-governs-instead-of-duplicating.md`, *Revision 2*,
  with its own mutation proof in both directions.
