---
title: 'Story 59.1: a list of names, and one task at a time'
type: 'feature'
status: 'in-review'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** ⌘8 is one flat, unbounded column of ~250px detail cards in a single
scroller (`tasks-pane.tsx:1924-1966`), and `list_tasks` returns every row
`ORDER BY id` with no cap (`db.rs:3301-3302`). Level 1 (the names) and level 2
(one task's detail) are fused into one page, so reaching the eighth task's runs
means scrolling past seven full cards. Epic 58 grew the row from 57.6's five
cells to ten stacked blocks and the owner's report — *"the task list it would be
good to see the list of the saved names … -> detail"* — is what that costs.
**Every capability he asked for is already built and honest; none of it is
findable.**

**Approach:** Split the one column into a master list and a detail region.
The master is a `useSurfaceColumn("tasks-list", …)` column of one-line rows —
kind, mode, name, host, next due — and the detail is a **plain sibling region
inside `tasks-pane.tsx`** holding everything the card shows today, re-sited and
not reworded. No IPC changes, no Rust, no schema. Selection is state, never a
read.

## Boundaries & Constraints

**Always:**
- **Re-siting, not deletion.** Every fact epic 58 and stories 59.2/59.3/59.5 put
  on the row keeps its current wording: the host badge and its Rust-composed
  sentence, the unhosted reason, the refusal paragraph, the run report, the mode
  badge, the description, and the paced section. A constant may move file
  position; none may change text.
- **The two conventions are already chosen and this story does not get to
  choose** (`0a24b39`). The master column is `useSurfaceColumn`
  (`surface-column.tsx:246`, the app's master-column convention, already used by
  `files-pane.tsx:1775`). The detail is a plain sibling `<section>` inside this
  pane — **never `PanelStrip`**, whose targets are documents opened in an editor.
  Nothing here may enter `panelsStore`.
- **Selection must never become a read.** `spec-58-3:40`'s Never clause — *"poll
  history; hold one fetch per row in flight at once; register a timer; ask for
  an unbounded list or invent a frontend limit"* — is AD-62's anti-poll
  invariant and it has teeth. Selecting a task issues **no IPC at all**; the run
  history stays behind the deliberate `Runs` press that 59.2 shipped. A
  master/detail satisfies the clause *better* than the old disclosure because
  exactly one task is selected by construction, but only if selection stays
  inert.
- **Exactly one task is selected.** One `aria-current` mark and no checkbox
  column — 59.4's refusal is honoured in advance rather than contradicted and
  then walked back. `aria-current` and not `aria-selected`: that is this app's
  own idiom for a single-selection master list (`chat-row.tsx:266`, asserted at
  `chat-row.test.tsx:198-207`), and `aria-selected` belongs to the multi-select
  model 59.4 defers — borrowing its attribute now would announce a *set* to a
  reader when there can only ever be one.
- **The first readable task is selected once the listing lands.** A detail
  region that starts empty over a list that has rows is a second empty state
  nobody asked for. This costs no read, because the detail is drawn from the
  `TaskVm` the listing already holds.
- **Selection is pruned like every other per-row slot.** A `refresh()` that no
  longer holds the selected id moves selection rather than leaving a detail
  region describing a record that is gone — `editingId`'s rule at
  `tasks-pane.tsx:1574-1576` and `history`'s at `:1585-1589`.
- The unreadable rows (`TaskListing.unknown`) keep their own section, their own
  heading and their own explanation, and are **not** selectable: there is no
  `TaskVm` to draw a detail from, and a row that selects into an empty detail is
  a control that can only disappoint.
- `app-shell.tsx:342-343`'s comment is **amended, not overturned**: it refuses
  the panel strip, which stays refused, and must stop reading as a refusal of a
  detail region.

**Block If:** the detail region cannot be built without a new IPC read, or
without `panelsStore`.

**Never:** add an IPC verb, a Rust file or a schema column; poll; register a
timer; put a task into `panelsStore`; add a checkbox column or a second
selection idiom; re-word a Rust-composed sentence; make an id editable
(`task_runs.task_id` joins on it); sort or filter the list beyond `list_tasks`'
own order.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Listing lands with rows | 2 readable tasks | 2 one-line rows; the first is selected; the detail shows that task | No error expected |
| A row is chosen | click row 2 | row 2 is `aria-selected`, row 1 is not; detail shows task 2 | No error expected |
| Selection changes | row 1 → row 2 | any open edit form closes and any open Runs section closes | No error expected |
| Selection is inert | any selection change | **zero** additional `syncTasks` / `syncTaskHistory` / `syncPacedWork` calls | No error expected |
| Runs still deliberate | press `Runs` in the detail | exactly one `syncTaskHistory` for the selected id | Refusal rendered, rows kept |
| Selected task vanishes | refresh drops its id | selection moves to the first remaining task; no detail about a gone record | No error expected |
| Every task vanishes | refresh returns `[]` | selection clears; the empty state renders | No error expected |
| Only unknown rows | `tasks: []`, `unknown: [1]` | the unknown section renders; nothing is selected; no detail region | No error expected |
| Unknown row | any | not selectable, no buttons, no `aria-current`, keeps its own heading and reason | No error expected |
| Unread | before the first read | the loading line, never the empty sentence | No error expected |
| Refused listing | `sync_tasks` rejects | the refusal; no detail region invented | The refusal is the render |
| Column folded | fold `tasks-list` | the strip says how many names it hides and gives them back; **no second Refresh or Add**; the detail region stays readable | No error expected |
| Refusal for an unshown task | Run now on A, choose B, A refuses | the refusal is promoted to the pane's own alert, named by its task | The refusal is the render |
| Governed / paused projected row | a cadence arrives beside a non-`paced` standing | the cadence is **not** drawn; the standing decides, not the string | Contradiction refused |
| Paced section | any | still rendered, still last, still read in the same settled pass | Unchanged from 58.7 |

</intent-contract>

## Code Map

- `src/lib/column-widths.ts` — `SURFACE_COLUMN_IDS:107` and `SURFACE_COLUMNS:111`
  gain `tasks-list`. One registry, because `column-fold.ts` keys its cookie on
  exactly this id set.
- `src/lib/stores/column-fold.ts` — `columnsUnfolded():51` gains the key. The
  parser and the cookie writer already read `SURFACE_COLUMN_IDS`.
- `src/lib/stores/column-fold.test.ts` — two hardcoded four-key objects
  (`:30-34`, `:68-72`) become five-key.
- `src/components/layout/tasks-pane.tsx` — the story. `TasksPane` returns the
  master column + seam + detail region; `TaskRow` becomes the one-line master
  row; a new `TaskDetail` holds what `TaskRow` used to.
- `src/components/layout/tasks-pane.test.tsx` — the row-scoped assertions for
  facts that moved become detail-scoped; the new selection tests land here.
- `src/components/layout/app-shell.tsx:342-343` — the comment, amended.
- Read only, unchanged: `list-fold.tsx`, `surface-column.tsx`,
  `task-form.tsx` (its props are untouched — confirmed with `StorySched`, who
  owns it for 59.7), `src/test/task-host-tick.test.ts`.

## Tasks & Acceptance

**Execution:**
- [x] `src/lib/column-widths.ts` — register `tasks-list` with a label, a title, a
      default width and a floor stated per column.
- [x] `src/lib/stores/column-fold.ts` + its test — the fifth key.
- [x] `src/components/layout/tasks-pane.tsx` — `useSurfaceColumn("tasks-list")`
      with a rail; `selectedId` resolved against the newest listing; `TaskRow`
      reduced to one line with `aria-current` and a roving tab stop; a new
      `TaskDetail` holding the field grid, host block, refusal, Run now / Edit /
      Forget, Runs and the edit form.
- [x] `src/components/layout/tasks-pane.tsx` — the projection's cadence cell now
      keyed on `standing` rather than on the presence of `cadence` (the 58.7
      contract, see the change log).
- [x] `src/components/layout/app-shell.tsx` — amend the comment.
- [x] `src/components/layout/tasks-pane.test.tsx` — migrate the moved
      assertions; add the selection, inertness, keyboard, pruning and fold tests.
- [x] `dev/mock-shell.ts` + `src/test/mock-shell-schedule-preview.test.ts` — the
      59.7 preview arm and its dialect/arity guard (see the change log).
- [x] `src-tauri/crates/keeper/src/sync_ipc.rs`, `keeper/src/lib.rs`,
      `src/lib/ipc/client.ts` — `sync_task_schedule_preview`, written to
      `StorySched`'s contract for Story 59.7 because `sync_ipc.rs` is this
      story's file.

**Acceptance Criteria:**
- Given a listing of twenty tasks, when ⌘8 is opened, then twenty one-line rows
  are on screen and none of them carries a field grid, a host sentence or a
  button.
- Given the listing has landed, when nothing has been clicked, then the first
  readable task is selected and its detail is on screen.
- Given a task is selected, when another is chosen, then exactly one row carries
  `aria-current` and no IPC call is made by the selection.
- Given the selected task is dropped by a refresh, when the read lands, then the
  detail describes a task that still exists, or none at all.
- Given an unknown row, when it is inspected, then it is not selectable and
  keeps its own heading and reason.
- Given `src/test/task-host-tick.test.ts`, when it runs, then it still finds
  exactly one `tokio::time::interval` in `keeper/src`.

## Spec Change Log

- 2026-09-01 — created. Baseline `c7ae611`, measured on this tree: frontend 301
  files / 5074 tests green; Rust 3827 / 0.
- 2026-09-01 — `aria-selected` became `aria-current`. The spec inherited
  `aria-selected` from the epic's 59.4 paragraph, which describes what a future
  **multi**-select would need. This list is single by construction, and the app
  already has an idiom for that: `chat-row.tsx:266`. Using the multi-select
  attribute would announce a set to a reader when there can only ever be one.
- 2026-09-01 — **the rail was wrong on the first pass, and a test caught it.**
  The spec asked for `files-pane.tsx`'s two entries, Refresh and Add. But this
  pane's header sits above BOTH columns rather than inside the folding one, so
  the fold takes neither away, and the strip's Refresh was a second control with
  the same accessible name as the one still on screen — *Found multiple elements
  with the role "button" and name "Refresh"*, which is precisely what a person
  navigating by name would have hit. What the fold genuinely takes is the
  **names**, so the rail now says how many there are and gives them back, which
  is what the Files tree's strip does with a selection it cannot show.
- 2026-09-01 — **a hole this story opened, and closed.** `refusals` is keyed by
  task id and drawn by the region, which now draws exactly one task. A Run now
  answered *after* the person has chosen another task therefore had nowhere to
  land: a refused run would have looked exactly like a successful one. The
  orphan-refusal promotion, which previously covered only *the row is gone*, now
  covers *the region is not showing it* — one rule instead of two, and the
  wider one. Tested directly.
- 2026-09-01 — **carried Story 58.7's cadence contract**, which is in the same
  file and could not sensibly wait. See that spec's Review Triage Log pass 2:
  `paced_work` pairs `cadence` with `standing` under a `debug_assert!` that is
  compiled out of the build a person runs, so the pane now draws the cadence
  from the **standing** rather than from the presence of the string. Two tests
  feed it the contradiction and require it to be refused.

## Design Notes

**Why the first task is selected rather than nothing.** The epic's sentence is
*"Selecting one shows everything the card shows today"*, which reads as an empty
detail until a click. Two things argue against that and neither is taste: a pane
whose right half is a placeholder over a list that has rows is a second empty
state competing with the real one, and — the load-bearing half — an initial
selection costs **no read**, because the detail is drawn entirely from the
`TaskVm` the listing already carries. The anti-poll clause is about reads, and
defaulting the selection issues none.

**Why the mode badge and the description do not move together.** 59.3's
acceptance is *"the row states that it is scheduled"*, so the mode badge stays on
the master row — it is a scannable word and the epic's four-fact list is a
minimum, not a ceiling. 59.5's is *"shown under its name"*, and in a
master/detail the name it belongs under is the detail's heading, so the
description moves. Neither is re-worded.

**What 59.2 already assumed.** `tasks-pane.tsx:1185-1200` says the `Runs`
control was promoted *"since Story 59.1's review"* and that 58.3's `shrink-0`
argument is *"answered by 59.1 rather than argued with, because the detail
region this now sits in is not competing with three buttons for a narrow row's
width"* — written against a detail region that did not exist yet. This story is
what makes those two sentences true; they are not amended, they are honoured.

## Verification

**Commands:**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx`
- `bun run vitest run src/test/task-host-tick.test.ts`
- `bun run vitest run src/lib/column-widths.test.ts src/lib/stores/column-fold.test.ts src/components/layout/surface-column.test.tsx src/components/layout/fold-strip.test.tsx`
- `bun run lint`, `bun run typecheck`, `bun run test`
- Mutation proof, all three inverted one at a time and restored, with the
  restore verified by reading `git diff` rather than by recalling the edit:
  - selection issues a read (`void syncTaskHistory(id)` in `selectTask`) →
    *expected "vi.fn()" to be called 1 times, but got 2 times*, and *expected
    "vi.fn()" to not be called at all, but actually been called 3 times*.
  - every row marked current (`aria-current="true"` unconditionally) →
    *expected [ '01FIRST', '01SECOND' ] to deeply equal [ '01FIRST' ]*.
  - the cadence drawn from the string rather than the standing
    (`row.cadence ?? PACED_NO_CADENCE_TEXT`) → *expected `<dd …>` to be null*,
    twice, once for governed and once for paused.

**Measured, on this tree:**
- Frontend: **302 files / 5132 tests passed** (baseline 301 / 5074). `+1` file
  is the new mock-shell dialect guard; the extra tests are this story's and
  Story 59.7's.
- `bun run typecheck`: clean. `bun run lint`: 4 warnings + 1 info, zero errors —
  the baseline exactly. (An alarm about a `useTemplate` **error** at
  `markdown-preview.ts:424` was raised and retracted; three of us measured it
  independently and the CLI's own tally is 4 + 1 with exit 0.)
- Rust, three crates: **3832 passed / 0 failed** (baseline 3827 / 0; the `+5` is
  Story 58.8's one new test and Story 59.7's four). `cargo clippy … -D warnings`
  clean; `cargo fmt --all --check` clean.
- `src/test/task-host-tick.test.ts` green: still exactly one
  `tokio::time::interval` in `keeper/src`. This story registers no clock — the
  arrow keys move a selection, and the pane's one display clock is untouched.

**Owed to the macOS gate**, because the `keeper` shell crate does not link on
this Linux host (`gobject-sys`) and `cargo fmt --check` is its only local gate:
`keeper::sync_ipc::sync_task_schedule_preview`, `TASK_SCHEDULE_PREVIEW_COUNT`
and the `generate_handler!` registration in `keeper/src/lib.rs`. Written to
Story 59.7's contract and committed separately from this story, so that story's
PR compiles on its own.
