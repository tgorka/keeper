---
title: 'Story 58.3: a list of runs you can open'
type: 'feature'
created: '2026-08-31'
status: 'in-progress'
baseline_revision: 'f8fbb90'
review_loop_iteration: 0
followup_review_recommended: false
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

- `src/components/layout/list-fold.tsx` — NEW. `useFold`, `FoldToggle`, `LIST_FOLD_MORE_LABEL`, `LIST_FOLD_LESS_LABEL`, moved verbatim out of `sync-pane.tsx:243-253,363-428`. One copy of the fold, shared, rather than a second implementation of it.
- `src/components/layout/sync-pane.tsx` — imports the four moved names and deletes its own copies. No behaviour change: the classes, the labels and `syncListSizes()` are unchanged.
- `src/components/layout/tasks-pane.tsx` — the new `TaskRunList` and its copy; `TaskRow` gains the disclosure; `TasksPane` gains the open-id / runs / error slots, the read, and the pruning. `taskOutcomeText` (`:348-359`), `formatTaskAgo` (`:302-317`) and `messageOf` (`:330-338`) are reused unchanged.
- `src/components/layout/tasks-pane.test.tsx` — the matrix above; `:14-22` already mocks `syncTaskHistory`, so no harness change is needed.
- `src/lib/ipc/client.ts:6348-6350` — `syncTaskHistory(id, limit?)`. Read only.
- `src/lib/stores/sync-detail.ts:54-72` — `syncListSizes()`, the app's one list-length preference; the fold reads it and this story does not change it.
- `src-tauri/crates/keeper-syncd/src/commands.rs:3299-3320` — `task_run_lines`, the column set and the empty-state phrase. Read only.

## Tasks & Acceptance

**Execution:**
- [ ] `src/components/layout/list-fold.tsx` — NEW: move `useFold`/`FoldToggle` and their two labels here, with a doc comment stating that the sizes are one global preference read from Rust.
- [ ] `src/components/layout/sync-pane.tsx` — import them; delete the moved definitions and the two `SYNC_FOLD_*` constants. Mechanical, no behaviour change.
- [ ] `src/components/layout/tasks-pane.tsx` — add the copy constants, `TaskRunList`, the row disclosure, and the pane's read-on-open with its race token and its pruning.
- [ ] `src/components/layout/tasks-pane.tsx` — extend the module doc with what 58.3 added, why the read is on open, and what a refresh does to an open section.
- [ ] `src/components/layout/tasks-pane.test.tsx` — assert every matrix row, the not-called-on-render property first.

**Acceptance Criteria:**
- Given a task with recorded runs, when its disclosure is pressed, then the runs are on screen with their own detail strings and `syncTaskHistory` has been called exactly once, with that task's id.
- Given the pane has merely rendered, when nothing is pressed, then `syncTaskHistory` has not been called at all.
- Given an open section, when the listing is re-read, then no further history read is issued and the rows stay.
- Given a refused history read, when it settles, then the engine's sentence is on screen and nothing claims the task has no runs.
- Given a mutation that removes the read-once guard, when the suite runs, then a test fails.
- No file under `src-tauri/` or `src/lib/ipc/gen/` is modified.

## Design Notes

**Why the trigger is not a fourth button in the header cluster.** The row header already carries Run now, Edit and Forget, all `size="sm"` outline/destructive `Button`s in a `shrink-0` cluster; at a narrow window the left block truncates to pay for it, and this repo has already shipped a row whose last control left the screen. `FoldToggle` states the rule that settles it: a control that *"changes how much of a list is on screen … is not an action on the folder and must not carry the same visual weight as Retry or Sync now"* (`sync-pane.tsx:417-419`). So the disclosure is a link-weight `<button>` on its own line below the fields, carrying the `label-caps text-faint` treatment — which makes it the section's quiet heading as well as its control, rather than printing the word twice.

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
