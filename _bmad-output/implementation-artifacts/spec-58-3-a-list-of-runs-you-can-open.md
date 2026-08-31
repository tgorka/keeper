---
title: 'Story 58.3: a list of runs you can open'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: 'a558ee5'
final_revision: 'b120b6f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** The whole run-history path is built and tested and nothing opens it. `db::task_runs` reads all seven columns newest-first under `TASK_RUNS_CAP` (`db.rs:3513-3517`, cap 50 trimmed at `:3340-3348`); `Engine::task_history` exposes it (`engine.rs:7882-7884`); `sync_task_history` clamps a limit to `1..=TASK_HISTORY_LIMIT_MAX` with `TASK_HISTORY_LIMIT_DEFAULT = 20` and returns `Vec<TaskRunVm>` (`sync_ipc.rs:2123-2137`, constants `:1745-1748`); `syncTaskHistory` exists (`client.ts:6348-6350`); `dev/mock-shell.ts:1818-1824` answers it with the clamp mirrored. The **only** reference to the wrapper under `src/` is a `vi.fn()` in a test (`tasks-pane.test.tsx:17`). A person can see that a task ended and never what it has been doing.

**Approach:** A per-row disclosure in the Tasks pane that reads `syncTaskHistory(task.id)` **on open**, rendering the runs on `SyncActivityList`'s idiom — quiet `label-caps` control, loading-versus-empty kept apart, `useFold`/`FoldToggle` truncation, an unknown-value fallback — with the CLI's settled columns. **Frontend only: no Rust file changes, no schema, no new IPC, no `src/lib/ipc/gen/**` changes.**

## Boundaries & Constraints

**Always:**
- Read **on open**, never on render and never on a clock. One press, one call, with that task's id.
- One section open at a time, `editingId`'s rule and reason (`tasks-pane.tsx:532-540`): a twenty-run list is taller than the eight-control form that argument was written for.
- Closing drops the runs it held, so re-opening re-reads. That is the only manual re-read, and it is what makes "one open, one call" a property rather than a cache policy.
- A listing refresh does **not** re-read history: `refresh()` fires from the mount, the Refresh button and every Run now settle, and a history read per open row per refresh is the poll AD-62's sentence is about. The open section keeps its rows and stays open. The one exception is a Run now **on the open row**, which changed that task's history and is a press by the same person, not a clock.
- A section whose row leaves the listing is closed, `refresh`'s `editingId` pruning verbatim (`tasks-pane.tsx:641-643`).
- `null` means unread and `[]` means empty, and they never render the same words. A failed read shows the refusal and keeps whatever rows were on screen — *a failed read is a fault to report, not a fact to invent* (`sync_ipc.rs:2072-2077`, Story 57.5's review).
- The refusal is rendered with `messageOf` and corrected in no way: `client.ts` normalises a Tauri rejection into an `IpcError` **value**, so `instanceof Error` renders `"[object Object]"` (`tasks-pane.tsx:322-328`).
- Columns are the CLI's, in its order: outcome word, relative time, host, detail (`commands.rs:3305-3317`). The outcome word comes from the pane's existing `taskOutcomeText`, so in-flight, a known outcome and a spelling a newer keeper wrote stay three distinct facts (NFR-43).
- Relative times come from the pane's existing display clock (`now`), never a second one.
- Ask for the command's default page — no `limit` argument — so the bound stays `TASK_HISTORY_LIMIT_DEFAULT`, in Rust, where it already is.
- History is **not** offered on an `unknown` row: it is not a `TaskVm`, its id may be `""`, and `tasks-pane.test.tsx:735` asserts those rows carry no buttons at all.

**Block If:** the change appears to require editing any file under `src-tauri/` or `src/lib/ipc/gen/**`, or a second list idiom because the existing fold cannot be shared.

**Never:** poll history; hold one fetch per row in flight at once; register a timer; ask for an unbounded list or invent a frontend limit; render `null` as "no runs recorded"; put the trigger in the row's header action cluster (see Design Notes); invent a third list idiom (`tasks-pane.tsx:47-49` promises the Sync pane's); touch `src/components/sync/task-form.tsx`, `dev/mock-shell.ts` or `src-tauri/**` (owned by `Story584`); touch `_bmad-output/planning-artifacts/**` (owned by `Epic58Plan`).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Render | pane loads two rows | `syncTaskHistory` **not called** | No error expected |
| Open | the disclosure on `01SCHED` pressed | `syncTaskHistory("01SCHED")` called **once**; the runs appear newest first with outcome, time, host, detail | No error expected |
| Re-render while open | the display clock ticks, or Refresh re-reads the listing | still exactly one call; the section stays open with its rows | No error expected |
| Open a second row | the disclosure on `01OTHER` pressed | the first section closes; one call for `01OTHER` | A slow first read cannot land in the second section |
| Close and re-open | the same disclosure pressed twice more | a second call for that id | No error expected |
| Empty history | the command resolves `[]` | *no runs recorded*, and never the loading line | No error expected |
| Refused read | the command rejects `{ code: "internal", message: "database is locked" }` | that sentence verbatim in the section, and no "no runs recorded" | The refusal is the render |
| Run now on the open row | Run now pressed while the section is open | the history is re-read once for that id | A refused re-read keeps the rows and shows the sentence |
| Row disappears | a refresh no longer lists the open task | the section closes | No error expected |
| Unknown row | `listing.unknown` has a row | no disclosure, no buttons at all | No error expected |

</intent-contract>

## Code Map

- `src/components/layout/list-fold.tsx` — NEW. `useFold`, `FoldToggle`, `LIST_FOLD_MORE_LABEL`, `LIST_FOLD_LESS_LABEL`, moved verbatim out of `sync-pane.tsx`. One copy of the fold, shared, rather than a second implementation of it.
- `src/components/layout/sync-pane.tsx` — imports the two moved components and deletes its own copies. No behaviour change: the classes, the label strings and `syncListSizes()` are unchanged, and its 85 tests are untouched.
- `src/components/layout/tasks-pane.tsx` — the new `TaskRunList`; `TaskRow` gains the disclosure and its `useId` region; `TasksPane` gains the one open-section slot, the read, the pruning and the row-count hydration. `taskOutcomeText`, `formatTaskAgo` and `messageOf` are reused; `taskReportText` is Story 58.2's row-local rule lifted to a function so the row and the history cannot disagree.
- `src/components/layout/tasks-pane.test.tsx` — the matrix above; the `vi.mock` factory already listed `syncTaskHistory`, so no harness change was needed beyond the store import the fold assertions read.
- `src/lib/ipc/client.ts:6348-6350` — `syncTaskHistory(id, limit?)`. Read only.
- `src/lib/stores/sync-detail.ts:54-95` — `syncListSizes()`/`hydrateSyncListSizes()`, the app's one list-length preference. Read only, and now hydrated by this pane as well as by the Sync pane.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `task_run_lines`, the column set and the empty-state phrase. Read only, cited by symbol.

## Tasks & Acceptance

**Execution:**
- [x] `src/components/layout/list-fold.tsx` — NEW: move `useFold`/`FoldToggle` and their two labels here, with a doc comment stating that the sizes are one global preference read from Rust.
- [x] `src/components/layout/sync-pane.tsx` — import them; delete the moved definitions and the two `SYNC_FOLD_*` constants. Mechanical, no behaviour change.
- [x] `src/components/layout/tasks-pane.tsx` — add the copy constants, `TaskRunList`, the row disclosure, and the pane's read-on-open with its race token and its pruning.
- [x] `src/components/layout/tasks-pane.tsx` — extend the module doc with what 58.3 added, why the read is on open, and what a refresh does to an open section.
- [x] `src/components/layout/tasks-pane.test.tsx` — assert every matrix row, the not-called-on-render property first.

**Acceptance Criteria:**
- Given a task with recorded runs, when its disclosure is pressed, then the runs are on screen with their own detail strings and `syncTaskHistory` has been called exactly once, with that task's id.
- Given the pane has merely rendered, when nothing is pressed, then `syncTaskHistory` has not been called at all.
- Given an open section, when the listing is re-read, then no further history read is issued and the rows stay.
- Given a refused history read, when it settles, then the engine's sentence is on screen and nothing claims the task has no runs.
- Given a mutation that removes the read-once guard, when the suite runs, then a test fails.
- No file under `src-tauri/` or `src/lib/ipc/gen/` is modified.

## Design Notes

**Why the trigger is not a fourth button in the header cluster.** The row header already carries Run now, Edit and Forget, all `size="sm"` `Button`s in a `shrink-0` cluster; at a narrow window the left block truncates to pay for each one, and jsdom performs no layout so no component test here could catch a control leaving the screen. `FoldToggle` states the rule that settles it: a control that *"changes how much of a list is on screen … is not an action on the folder and must not carry the same visual weight as Retry or Sync now"*. So the disclosure is a link-weight `<button>` on its own line below the fields, wearing `FoldToggle`'s own treatment — and deliberately **not** `label-caps text-faint`, because `DESIGN.md` reserves that tone for *"`aria-hidden` glyphs and section labels … and never carries a fact"*, and this control is the only route to a task's history. The section prints no heading of its own; the trigger names it, and `aria-controls` ties the pair together.

**Why closing forgets.** A cache keyed by id would make "one open, one call" depend on which row was opened before, and would hold a run list that `task_runs` may have trimmed underneath it (cap 50). Dropping on close costs one IPC read per deliberate press and gives the person a re-read with no extra control.

**Why the fold is shared and not copied.** `tasks-pane.tsx:47-49` already promises it reuses the Sync pane's list idioms *"rather than inventing a third"*. Re-implementing `useFold` beside it would be that third idiom with the same name. `SYNC_FOLD_MORE_LABEL`/`SYNC_FOLD_LESS_LABEL` are referenced nowhere outside `sync-pane.tsx`, so the move is contained; the strings do not change, so the Sync pane's tests do not either.

## Verification

**Commands:**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx src/components/layout/sync-pane.test.tsx` — expected: all pass, the Sync pane's unchanged.
- `bun run typecheck` — expected: clean.
- `bun run lint` — expected: 4 warnings + 1 info (the pre-existing baseline).
- `bun run test` — expected: at or above 301 files / 5002 tests, plus this story's additions.
- `git diff --stat -- src-tauri src/lib/ipc/gen` — expected: empty.

**Manual checks (if no CLI):**
- Mutate the read-on-open guard away (read on render) and confirm the not-called-on-render test fails; restore, and verify the restore by reading `git diff` rather than from memory.

## Spec Change Log

_No spec amendment was needed: every review finding was a patch, a defer or a reject. The intent contract stood as written._

## Review Triage Log

### 2026-08-31 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 19: (high 0, medium 10, low 9)
- defer: 2: (high 0, medium 2, low 0)
- reject: 4: (high 0, medium 1, low 3)
- addressed_findings:
  - `[medium]` `[patch]` an empty history and a refusal rendered together, so a task that read `[]` and was then run said "no runs recorded" beside "database is locked" at the instant `claim_task` had written a row — the empty sentence now yields to a refusal exactly as the loading line does, with a test that drives that sequence.
  - `[medium]` `[patch]` a refused first read was a dead end: nothing re-reads it and the one obvious press looks like a dismissal — the refusal now says how to ask again, and a test proves the close-and-open retry actually re-reads.
  - `[medium]` `[patch]` the disclosure carried `aria-expanded` and named no region, against a rule this repo documents in four components and tests in two — `useId` + `aria-controls`, present only while the section is, and asserted.
  - `[medium]` `[patch]` the trigger wore `text-faint`, which `DESIGN.md` reserves for glyphs and section labels and forbids from carrying a fact — it now wears `FoldToggle`'s own `text-muted-foreground text-xs`, the treatment whose rule it cites.
  - `[medium]` `[patch]` the runs section and an edit form could stand open on one row at once, which is the wall of height the borrowed one-at-a-time argument exists to forbid — each now closes the other, and the disclosure is refused while a save is in flight so it can never unmount a form Rust's answer is coming to.
  - `[medium]` `[patch]` the disclosure had no `deleting` guard, so it could ask about a row whose Forget was in flight and answer "no runs recorded" about a record that was leaving — guarded like Forget, and asserted.
  - `[medium]` `[patch]` an unfolded list could still hold rows back — the unfolded preference floors at ten while a history page is twenty — and `FoldToggle` says only "Show fewer", so the reader believed they had seen everything: the remainder is now counted out loud.
  - `[medium]` `[patch]` `taskOutcomeText` rendered a blank stored spelling as nothing at all, which is the leading word of a run row; falling through would have called a closed run "running now". Now named as an outcome this build cannot read.
  - `[medium]` `[patch]` the open section lived in three separate slots compared against each other at render, which is correct only while every writer happens to batch them — collapsed into one `{ id, runs, error }` object, so a row can never be handed another row's runs.
  - `[medium]` `[patch]` `syncListSizes()` is module state that only the Sync pane's mount and the settings form fill in, and the shell renders the two views exclusively — so a person going straight to Tasks folded at the fallback rather than at their own setting, the exact drift the fold extraction was meant to end. The pane now hydrates it.
  - `[low]` `[patch]` a `Runs` press during a Run now on the same row issued a second read for one deliberate press — openness is now captured at the press and re-checked at the settle, with a test that a Run now on a closed row reads nothing.
  - `[low]` `[patch]` a blank `host` rendered as a gap in the row, and which host ran it is most of the point of this list — named, on `TASKS_UNKNOWN_NO_ID_TEXT`'s precedent.
  - `[low]` `[patch]` the host cell was `shrink-0` over a stored string this build did not choose, so a long id would push the report out of the row — it now breaks.
  - `[low]` `[patch]` the `<ul>`'s `aria-label` duplicated the trigger's accessible name, giving a screen reader two targets called the same thing — dropped, with `aria-controls` doing the naming (`tag-combobox.tsx`'s rule).
  - `[low]` `[patch]` the loading line and the empty sentence were inert while only the refusal announced — `role="status"` on both, so a press is never answered with silence.
  - `[low]` `[patch]` the comment claimed focus never leaves the trigger; the prune path destroys the section without a press — qualified rather than left as an invariant the code does not hold.
  - `[low]` `[patch]` the new unknown-row test re-asserted two assertions that already existed elsewhere — it now asserts the one 58.3-specific claim, the named control's absence.
  - `[low]` `[patch]` the fold test hardcoded `10` for a number that lives in mutable module state and has an exported name — read from `syncListSizes()`, with the sizes restored after each test.
  - `[low]` `[patch]` two count assertions fired the microtask after `waitFor` reached their number, so a double read could have slipped past — both now assert after a settle the pane itself evidences.
- Deferred: `formatTaskAgo` clamps a clock that runs ahead to "just now", so every run recorded by a peer whose clock leads renders as just now in a list whose whole point is other hosts; and a readable `tasks` row whose `id` is the empty string is unnamed everywhere on this pane, not only on this control. Both pre-date this story.
- Rejected: a composite React key for the run rows (this list is one `ORDER BY id DESC` over an INTEGER PRIMARY KEY, unlike the Activity list whose rows have no identity); collapsing an expanded fold when a re-read shrinks the list below the folded size (inherited `FoldToggle` behaviour, shared by five lists, and cosmetic); skipping the read when `task.lastRun === null` (the listing can be arbitrarily old because this pane does not poll, so it would assert an empty history from stale evidence — the same shape as the defect Story 57.5's review fixed); and a `max-h` scroll region for the report (rejected for 58.2's reason, and the row is narrower here, not wider).

## Auto Run Result

Status: done

**What was implemented.** A task's runs are openable from ⌘8. The whole path — DDL, index, cap, query, engine method, IPC verb with its clamp, wire type, TypeScript wrapper, mock-shell handler — was finished and tested for a wave, and the only reference to `syncTaskHistory` under `src/` was a `vi.fn()`. Each readable row now carries a quiet link-weight disclosure that reads that command **on open**: one press, one call, that row's id, no limit invented here. Newest first, in the CLI's own columns — outcome word, relative time, host, report — folded at the app's one list-length preference. `null`, `[]` and a refusal never borrow each other's words. **No Rust file, no schema, no new IPC, and nothing under `src/lib/ipc/gen/`.**

**Files changed**
- `src/components/layout/list-fold.tsx` — NEW. `useFold`/`FoldToggle` and their labels, moved verbatim out of `sync-pane.tsx` so the run history reuses the one fold rather than becoming a second copy of it.
- `src/components/layout/sync-pane.tsx` — imports them; its own copies and the two `SYNC_FOLD_*` constants deleted. 85 tests untouched and green.
- `src/components/layout/tasks-pane.tsx` — `TaskRunList`; the row disclosure with its `aria-controls` region and its write-in-flight guards; one `{ id, runs, error }` slot with a race token, pruning, the post-Run-now re-read, and the row-count hydration; `taskReportText` lifted out of Story 58.2's row.
- `src/components/layout/tasks-pane.test.tsx` — 24 tests for this story, and the `TaskVm` fixture given Story 58.4's two new required keys.

**Review findings:** 19 patches applied, 2 deferred, 4 rejected, 0 intent gaps, 0 spec loopbacks.

**Verification performed**
- `bun run vitest run src/components/layout/tasks-pane.test.tsx src/components/layout/sync-pane.test.tsx` — **148 passed**, the Sync pane's 85 unchanged.
- `bun run test` — **301 files / 5037 tests passed** (baseline before this epic's Wave 1: 301 / 5002; 58.2 added 11, this story 24).
- `bun run typecheck` — clean, project-wide.
- `bun run lint` — 4 warnings + 1 info, exactly the pre-existing baseline.
- `git diff --stat -- src/lib/ipc/gen` — empty. Every `src-tauri/` change on this branch belongs to Stories 58.4/58.5.
- Mutation proof: making `refresh` re-read the open section failed two tests — *"issues no second read when the clock ticks or the listing is re-read"* and *"re-reads the open row's history once after a Run now on that row"* (2 failed / 61 passed). Restored from a copy taken before the mutation, and the restore verified by grepping `git diff` for the mutation's own symbols — none — plus a `sha256sum` match against the pristine copy, rather than from memory. 63 pass again.
- No visual verification: the `keeper` shell crate does not link on this Linux host. `dev/mock-shell.ts` already answers `sync_task_history` with the clamp mirrored and carries four run fixtures — a clean summary, a release tally, a failure with its reason, an in-flight run and a newer keeper's spelling — plus tasks with no runs at all, so the dev shell exercises the loading, empty, populated and unknown-spelling states with no fixture work.

**Residual risks**
- The fold's `unfolded` size and the command's page size are set independently, so the "N more recorded and not shown" line is load-bearing rather than decorative. A future page-size change should be checked against it.
- `historyRef` still mirrors `history` by hand, now through a single writer. A future writer that sets the state directly would break the pane's ability to see the open section from a stable callback.
- The disclosure's layout claim — a link-weight control on its own line rather than a fourth button in the header — cannot be tested here, because jsdom performs no layout.

**The bookkeeping went wrong once and has been repaired.** This story's four files were staged and,
in the same second, a sibling agent working the same worktree ran a plain `git commit` — so 58.3's
code first landed inside `feat(58.6): two hosts, one missed window, one run` rather than in a commit
of its own. The content was complete and green throughout; only the commit boundary was wrong, which
matters because this epic ships as one PR per story.

The coordinator split it before anything was pushed: `b120b6f feat(58.3): a list of runs you can
open` now holds this story's four files and its spec, `9de7aac feat(58.6)` holds the seven
`src-tauri` paths, `deferred-work.md` and its own spec, and the three commits that sat on top were
replayed onto the split. The proof the split lost nothing is that the resulting tree is byte-identical
to the pre-split one (`HEAD^{tree}` compared directly), not that the diffs looked right.

The avoidable half was staging early in a shared worktree: it leaves work in an index a sibling's
next `git commit` will sweep up, so the pathspec form (`git commit -- <paths>`) is the only safe one
there. Every later commit in this epic used it.
