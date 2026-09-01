---
title: 'Story 59.2/59.3: a run you can open, and a row that says enough to act on'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '394d8c4'
final_revision: '0928db9'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
warnings: ['two-stories-one-spec', 'navigation-half-not-built']
---

<intent-contract>

## Intent

**Problem:** The owner, running v0.8.24 with tasks in it for the first time, reported
that he could not see a run's details and could not tell that a scheduled task was
runnable by hand. Six scouts found that **both capabilities already shipped and
worked** — `run_task_now` drops the window predicate entirely for a person
(`engine.rs:2381-2407`), and `task_runs` is read newest-first in SQL over a
50-run cap (`db.rs:3748-3752`). What did not exist was any way to tell:

- the row never rendered `mode`, so a `scheduled` row's only visible schedule
  facts were a cron string and a next-due time beside a button whose relationship
  to them was unstated;
- the sentence describing what **Run now** does existed twice in `docs/sync.md`
  and nowhere in the app;
- the `Runs` control was a dotted-underline link, **last** in the row — after the
  field grid, the host block and any refusal — with no chevron, no count and no
  affordance of a button;
- and three bounds were invisible at once: the read asks for twenty, the store
  keeps fifty per task, the fold shows ten first. A reader who pressed *Show all*
  had reached the end of neither.

**Approach:** Say the things the pane already knew. No Rust, no IPC, no new read.

## Boundaries & Constraints

**Always:**
- **A closed section prints no number.** `task_runs` is read on open and never on
  render (58.3's teeth-bearing `Never: poll history`), so a count on a shut row
  could only be guessed — and a guessed total that looks real is what
  `count-label.ts` exists to prevent. `lastRun === null` is the single fact the
  listing already carries, so it is the only one a shut control may state.
- **Stored spellings on both badges.** `decode_task` diverts an unreadable mode
  into the unknown list, so the mode badge can only ever render one of
  `TASK_MODES`; two words for one stored value is the drift AD-C7 forbids.
- **Nothing may make Run now consult the window.** 58.6 asserts the opposite in a
  pair of tests precisely so a later change cannot narrow *run it now* into *run
  it if due*.

**Never:** re-read history on render, on a clock or on a listing refresh; invent a
frontend limit; print a count the pane has not read.

## I/O & Edge-Case Matrix

| Scenario | Expected |
|---|---|
| `mode = scheduled` | the row shows `scheduled`, in the stored spelling |
| zero tasks | the Run-now sentence is absent — a sentence about an invisible button is noise |
| shut section, `lastRun = null` | `Runs — none yet`, and no digit anywhere in the label |
| shut section, task has run | `Runs`, with no count |
| open section holding 3 | `Runs · 3 runs` |
| open section holding ≥ 20 | the trimming notice, once, under the list |
| open section holding 19 | **no** notice — nothing has been trimmed |
| `description = "the photos, nightly"` | rendered under the id |
| `description = "   "` or `null` | **no element at all**, not an empty paragraph |

</intent-contract>

## Tasks & Acceptance

**Execution:**
- [x] `tasks-pane.tsx` — the mode badge beside the kind badge.
- [x] `tasks-pane.tsx` — `TASKS_RUN_NOW_SENTENCE`, rendered only when a row exists.
- [x] `tasks-pane.tsx` — the `Runs` trigger becomes a ghost `Button` with a rotating
      chevron and `taskHistoryTriggerText`'s three shapes.
- [x] `tasks-pane.tsx` — `TASK_HISTORY_BOUND_TEXT` under a full section, gated on
      `TASK_HISTORY_BOUND_NOTICE_AT`.
- [x] `tasks-pane.tsx` — 59.5's description under the id, via `taskDescriptionText`.
- [x] `count-label.ts` — the `RUNS` noun.
- [x] `docs/sync.md` §14 — the pane paragraph rewritten for all of it.

## Spec Change Log

- 2026-08-31 — **two stories in one spec, and the navigation half is not here.**
  59.2's own acceptance ("a run you can open" as a *surface*) and the whole of
  59.1/59.4 need the master/detail restructure, which is not built. What shipped is
  everything that did not depend on it. The frontmatter carries
  `navigation-half-not-built` so nobody reads this file as 59.1 being done.

## Review Triage Log

## Design Notes

**What 58.3's decision was, and why overturning half of it is honest.** The dotted
underline was deliberate: `FoldToggle`'s rule that a control changing *how much of
a list is on screen* "must not carry the same visual weight as Retry or Sync now",
plus the `shrink-0` cluster already holding three buttons. The first reason does
not apply — **this is not a fold, it is the only route to a task's history**, which
the original comment itself conceded made it "the most load-bearing thing on the
row". The second is a layout constraint that 59.1 answers by moving the detail out
of the row entirely; until then the control is a `ghost` Button, which is quieter
than the three `outline` ones beside it.

**The guard that could not fail, and why it is written down.** The description test
first asserted on *text*: `queryByText(/\S/, …)`. Mutating `taskDescriptionText` to
return its argument unchanged left it **green**, because a text query cannot tell a
paragraph that was never rendered from one rendered around three spaces — which is
exactly the bug the helper prevents. It asserts on `TASKS_DESCRIPTION_TESTID` now.
A testid exists here for a reason a text query cannot serve.

## Verification

- `bun run vitest run src/components/layout/tasks-pane.test.tsx` — 80 green (74 before).
- `bun run test` 301 files / 5074 tests; `typecheck` clean; `lint` at baseline.
- Mutation proof, each restored and re-verified: the mode badge rendering `kind`
  instead fails two tests; flattening `taskHistoryTriggerText` to the bare title
  fails the count test; `taskDescriptionText` returning its argument unchanged
  fails the description test (**after** it was rewritten to assert on the element —
  the first version did not fail, and that is recorded above).
