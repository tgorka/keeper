/**
 * The Tasks primary view (Epic 57, Story 57.6, FR-351, FR-352, AD-137).
 *
 * The owner's complaint that opened this epic was literally *"nie widzę w menu
 * croon like job schedules"* — I do not see it in the menu. Waves 1–2 gave
 * keeper a task record, a schedule, a lease and a CLI; nothing in the app could
 * see, name or drive one. This is that surface, and its whole discipline is one
 * sentence from AD-137: **every host claim on screen is true.**
 *
 * So each row states the host that will actually run it, and the negatives are
 * as loud as the positives:
 *
 * - On macOS there is no `keeper-syncd` unit anywhere in this repository, so the
 *   app is the only host a Mac has, and the row says the task runs *only while
 *   keeper is running*.
 * - On Linux the daemon is the host only when its unit is enabled **and** it
 *   reads the same `sync.db` — which by default it does not — and the row says
 *   which of those two it is.
 * - A task that looks enabled and that no present host can run reads
 *   **Unhosted**, with the reason, never as enabled-and-quiet. That is the
 *   invisible-failure shape this whole epic exists to close: nobody notices the
 *   absence of housekeeping.
 *
 * **Story 58.1 made it a surface that also creates, changes and deletes one.**
 * Until then `sync_task_save` and `sync_task_forget` were registered, typed,
 * wrapped and mocked, and no control anywhere in the app called either — so this
 * pane could only tell the owner to open a terminal. Now the header reveals
 * {@link TaskForm} inline, each readable row reveals the same component seeded
 * from itself, and Forget asks first. One component in two places (AD-C7),
 * because two forms would be two chances to word or validate the same task
 * differently. What that form deliberately does not validate is everything Rust
 * already refuses in its own words; the list and the reasons are in
 * `task-form.tsx`.
 *
 * **Not one word of that decision is made here.** The kind, the sentence and
 * the reason all arrive on {@link TaskHostVm}, composed by
 * `keeper_core::tasks::task_host` over facts the shell can establish, and are
 * rendered verbatim. A platform sniff in this file would be a guess — and
 * `src/test/no-user-agent-gating.test.ts` forbids one by name — and two copies
 * of a sentence is how a surface ends up claiming a host it no
 * longer has. What this file owns is the relative times — rendered client-side
 * from the instants Rust ships, `formatSyncWaited`'s precedent — and the words
 * for a *stored spelling*, which is a display concern and forward-compatible by
 * construction: an outcome or kind this build does not know renders as itself
 * (NFR-43).
 *
 * It reuses the Sync pane's outer chrome and its pending/parked list idiom
 * rather than inventing a third: `<section>`/`<header>`/`<ScrollArea>`, one
 * full-bleed column of rows separated by rules. The whole surface is
 * capability-gated at the app-shell and sidebar level, so a machine that cannot
 * keep a task record gets no Tasks entry at all rather than an empty one.
 *
 * **Story 58.2 put the run's own report on the row.** Every completed run
 * already recorded what it did — `perform_task` composes the sentence, either
 * the sync counts or a release sweep's tally, and `finish_task_run` persists it
 * with the outcome in one statement — and `TaskRunVm.detail` carried it all the
 * way to this file (`keeper-core/src/tasks.rs:262-263`) while nothing here read
 * it: the row said a run had ended and never what the run said. That string is
 * Rust's and is rendered verbatim — unclipped, unparsed and with its own line
 * breaks kept — in the column `keeper-syncd tasks status` has printed all
 * along. A report that is absent or blank is silence rather than a sentence
 * this file invented, because each of the states that reach it already has a
 * cell naming it: a task that never ran, an in-flight run whose row
 * `claim_task` opens with `detail` unset, and a lease the next host reclaimed,
 * written as `abandoned` without touching `detail` at all.
 *
 * **Story 58.3 opened the run history.** `db::task_runs`, `Engine::task_history`
 * and `sync_task_history` had been finished for a whole wave — clamped, typed,
 * wrapped in `client.ts` and answered by the mock shell — while the only
 * reference to the wrapper under `src/` was a `vi.fn()` in a test: a person
 * could see that a task had ended and never what it had been doing. Each
 * readable row now carries a quiet disclosure that reads that command **on
 * open**: one press, one call, with that row's id. Never on render, never on a
 * clock, and never on a listing refresh — `refresh()` fires from the mount,
 * from the Refresh button and from every Run now settle, so a history read per
 * open row per refresh is exactly the poll AD-62's sentence is about. A refresh
 * therefore leaves an open section as it is: still open, still holding the rows
 * it read. The one exception is a Run now **on the open row**, which is what
 * changed that task's history and was pressed by the same person. Closing
 * forgets the runs it held, so re-opening re-reads rather than showing a list
 * `task_runs` may have trimmed underneath it (cap `TASK_RUNS_CAP`).
 *
 * **Story 59.4 let a person hold several names at once.** Epic 59 refused a
 * Tasks multi-select on a checkable ground — every task write in the whole stack
 * was single-id, so a selection would have been state whose only action was a
 * loop of N writes with N partial-failure stories. `Engine::set_tasks_enabled`
 * and `Engine::forget_tasks` are that missing consumer, so the refusal is
 * overturned exactly the way `spec-43-8…:347-348`'s Files refusal was at 45.3:
 * *once a bulk consumer existed*. The selection model is therefore **copied**
 * from `files-pane.tsx` rather than designed — the same three modes, the same
 * precedence gate, the same one-pass toggle, the same inclusive Shift run over
 * the flat visible order, `aria-selected` on every row and the count through
 * `countLabel` — because `spec-45-17…:200` forbids a second selection idiom by
 * name. Anything that genuinely could not be reused says so where it is: see
 * {@link TaskRow} for the `aria-current` → `aria-selected` flip and `select`
 * below for the one Files branch that has no Tasks analogue.
 *
 * **Story 59.12 let a task be opened in a tab.** The owner could click a row
 * and read its detail beside the list, and could not keep one: *"nie mogle
 * kliknac na element z task list i zobaczyc szczegolow w nowym tabie."* So
 * `PanelTargetVm` grew a `task` variant and this pane answers the gesture pair
 * every other browsing surface already answers to — a plain click previews into
 * the active panel, a double click opens beside it — copied from
 * `files-pane.tsx` for the reason 59.4's selection model was. The panel draws
 * {@link TaskDetail}, this file's own component, with its write half switched
 * off; see that component's header for why the pane stays the only writer.
 */
import { ChevronRight, ListChecks } from "lucide-react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { FoldToggle, useFold } from "@/components/layout/list-fold";
import { useSurfaceColumn } from "@/components/layout/surface-column";
// `TASK_HOST_WIDE_TEXT` is the form's rather than this file's: the picker's
// first option and this pane's folder column are one fact, and the dependency
// has to run this way round because the pane mounts the form. The other
// arrangement is an import cycle between a pane and the form it reveals.
import { TASK_FORM_ADD_TITLE, TASK_HOST_WIDE_TEXT, TaskForm } from "@/components/sync/task-form";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type CountNoun, countLabel, RUNS } from "@/lib/count-label";
import type {
  PacedWorkVm,
  PanelTargetVm,
  TaskBatchReceiptVm,
  TaskListingVm,
  TaskRunVm,
  TaskVm,
} from "@/lib/ipc/client";
import {
  syncPacedWork,
  syncTaskForget,
  syncTaskHistory,
  syncTaskRunNow,
  syncTasks,
  syncTasksForget,
  syncTasksSetEnabled,
} from "@/lib/ipc/client";
import { columnFoldStore } from "@/lib/stores/column-fold";
import { panelsStore } from "@/lib/stores/panels";
import { hydrateSyncListSizes } from "@/lib/stores/sync-detail";
import { cn } from "@/lib/utils";

/** The heading, and the promise the pane makes in one line. */
export const TASKS_PANE_TITLE = "Tasks";
export const TASKS_PANE_SUBTITLE =
  "Work keeper does on a schedule, and which host on this machine will actually run each one.";

/**
 * What **Run now** means, said where the button is (Story 59.3).
 *
 * The sentence already existed — twice — in `docs/sync.md` (§14's exit-code
 * section and its `--timer` paragraph), and nowhere in the app. The owner asked
 * for a control that has always been there, is rendered on every readable row,
 * and is gated on nothing but *this row's run is in flight*; what he could not
 * tell is what pressing it would do to a `scheduled` task. Both halves of that
 * are worth stating and neither is obvious:
 *
 * - It performs the work **whether or not a window is open**. `run_task_now`
 *   passes `TaskTrigger::Requested`, which sets `due_at_most: None` and drops
 *   `claim_task`'s window predicate entirely. Story 58.6 asserts this against
 *   its opposite in one pair of tests, precisely so a later change cannot
 *   quietly narrow *run it now* into *run it if due*.
 * - It does **not move the schedule**. Somebody asking for a run now is not
 *   asking to skip tonight's.
 *
 * Sited on the pane rather than as a per-row tooltip: this is one fact about the
 * verb, not per-row state, and `PACED_SUBTITLE` set the precedent that a fact
 * worth knowing is said in words rather than left to be inferred from an
 * absence.
 */
export const TASKS_RUN_NOW_SENTENCE =
  "Run now performs the work immediately, whether or not a window is open — and it does not move the schedule.";

/** Before the first read has landed the list is unknown, not empty. */
export const TASKS_PANE_LOADING_SENTENCE = "Reading the task record…";

/**
 * The empty state, in three parts: what this view is, that a task can be made
 * right here, and how a host with no window gets one instead.
 *
 * **Load-bearing rather than cosmetic**, and it has now been wrong twice.
 * Nothing in either epic creates a task row on migration, on open or on first
 * tick, so *every existing install opens ⌘8 to this text and nothing else* — it
 * is the first thing the owner reads. Story 57.5's review found it naming a verb
 * the CLI does not have (`keeper-syncd task add`: there is no `task` group and
 * no `add` verb — the group is `tasks` and creation is `tasks set`), and Story
 * 58.1 found the remainder false: it said this view "cannot create one yet",
 * which it now can, and it instructed the reader to open a terminal while the
 * control that does it sits in the header above the sentence.
 *
 * Three things it must not do:
 *
 * - **Send the reader to a terminal for something the app now does.** Creation
 *   is {@link TASK_FORM_ADD_TITLE} in this pane's header, so that is the primary
 *   path and the sentence says so. The command is still named, because it is
 *   still true: a host with no window of its own — one reached only over a
 *   terminal — gets its tasks from the daemon's command line, and that is the
 *   *other* way rather than the only one.
 * - **Name a verb the CLI does not have.**
 *   {@link TASKS_PANE_EMPTY_COMMAND} is the real one, and
 *   `tasks-pane.test.tsx` checks every `keeper-syncd` phrase in this file's copy
 *   against the daemon's actual group and verb list, so a rename cannot quietly
 *   re-break it.
 * - **Promise a background service that is not there.** `keeper-syncd` builds
 *   and ships for both of keeper's desktop targets (`release.yml` publishes
 *   `keeper-syncd-x86_64-unknown-linux-gnu` and
 *   `keeper-syncd-aarch64-apple-darwin`), so the command is true to read on
 *   either — but no launchd plist exists anywhere in this repository, so nothing
 *   *starts* it in the background on a Mac. What is true on both is the thing
 *   worth saying: keeper itself hosts the task while keeper is open, and each
 *   row states which host it really has. No platform branch is needed for that,
 *   which is just as well: a sniff here would be a guess, and
 *   `src/test/no-user-agent-gating.test.ts` forbids one by name.
 */
export const TASKS_PANE_EMPTY_SENTENCE =
  "No tasks yet. This view lists, inspects and runs tasks, and you can create one right here. A host with no window of its own gets its tasks from the daemon's command line instead:";

/** The one real creation command, quoted exactly as `keeper-syncd` spells it. */
export const TASKS_PANE_EMPTY_COMMAND =
  'keeper-syncd tasks set nightly --kind sync --schedule "0 3 * * *"';

/**
 * What happens once a task exists — however it got here — and the only promise
 * made on this screen.
 *
 * Deliberately says *while keeper is running* and nothing stronger: whether a
 * daemon also runs it is a per-machine fact each row states for itself, and this
 * text has no way to establish it.
 */
export const TASKS_PANE_EMPTY_AFTER =
  "Either way the task appears here. Every host that shares this record sees it, and keeper runs a due task while keeper is running — each row says which host will actually run it.";

/**
 * The heading over the rows this build cannot read (NFR-43).
 *
 * They are shown rather than skipped, and the heading says why: a task written
 * by a newer keeper is still a task, and a list that silently omitted it would
 * tell the user they have none.
 */
export const TASKS_UNKNOWN_HEADING = "Written by a newer keeper";

/**
 * The badge on such a row. The word is *Unknown* and not *Unhosted*: those are
 * different facts. An unhosted task is one this build understands perfectly and
 * no host will run; this is a row this build cannot read at all, and it may well
 * be running on the other host right now.
 */
export const TASKS_UNKNOWN_BADGE = "Unknown";

/**
 * What stands in for the id of an unknown row that has none.
 *
 * `db::list_tasks` emits `UnknownTask { id: String::new(), … }` for a row whose
 * `id` column will not read at all, and an empty `<span>` there rendered as a
 * blank line above a reason with nothing to attach it to (Story 57.5's review,
 * finding 10).
 */
export const TASKS_UNKNOWN_NO_ID_TEXT = "a row with no readable id";

/**
 * How often the pane re-measures "now" (Story 57.5's review, finding 6).
 *
 * Half a minute, because every relative string this pane renders is
 * minute-grained: `formatTaskDue`/`formatTaskAgo` speak in minutes, hours and
 * days, so a finer tick would re-render for no visible change and a coarser one
 * would let "in 1 min" sit past its instant. It re-measures a clock and reads
 * nothing — the engine is polled only by an explicit Refresh or a Run now.
 */
export const TASKS_CLOCK_TICK_MS = 30_000;

export const TASK_RUN_NOW_TEXT = "Run now";
export const TASK_REFRESH_TEXT = "Refresh";

/** Reveals this row's own edit form, and hides it again. */
export const TASK_EDIT_TEXT = "Edit";

/** The destructive one, and the three sentences it is confirmed with. */
export const TASK_FORGET_TEXT = "Forget";
/**
 * Which task is being forgotten, by the id the row shows. A function and
 * therefore camelCase, `syncInForceNote`'s shape: a list of ten of these would
 * otherwise all confirm with the same words.
 */
export function taskForgetConfirmTitle(id: string): string {
  return `Forget task ${id}?`;
}
/**
 * What forgetting a task actually does, in the backend's own framing
 * (`sync_ipc.rs`: *"Deletes a record, never content"*).
 *
 * The distinction is the whole reason this confirmation exists. A `release`
 * task's forgotten schedule simply stops sweeping — nothing it has ever released
 * comes back, and nothing it would have swept goes away — and a `sync` task's
 * folder is untouched. Somebody who thinks Forget might delete files will not
 * press it, and somebody who thinks it will tidy their releases up will.
 */
export const TASK_FORGET_CONFIRM_BODY =
  "This deletes a record, never content. keeper drops the task and the runs it recorded; the folders it synced and everything it ever released are left exactly as they are — a release task's forgotten schedule just stops sweeping.";
export const TASK_FORGET_CANCEL_TEXT = "Keep it";

/** Column labels, so the row is readable without a table header. */
export const TASK_SCHEDULE_LABEL = "Schedule";
export const TASK_HOST_LABEL = "Host";
export const TASK_NEXT_DUE_LABEL = "Next due";
export const TASK_LAST_RUN_LABEL = "Last run";
export const TASK_LAST_OUTCOME_LABEL = "Last outcome";
/**
 * What the run itself said, as distinct from keeper's verdict on it.
 *
 * {@link TASK_LAST_OUTCOME_LABEL} is keeper's judgement of the run. This is the
 * run's own report, in the engine's words, and there is one per task kind:
 * `perform_sync_task` composes
 * `"{synced} synced, {busy} already syncing, {deferred} waiting, {failed} failed"`,
 * `perform_release_task` composes
 * `"released N paths (N bytes) from N folders, N declined, …"`, and both wrap it
 * as `"{detail}: {reason}"` when something failed — so a release row's sentence
 * is roughly twice a sync row's and neither has a bound. `finish_task_run`
 * persists it and `TaskRunVm.detail` carries it here
 * (`keeper-core/src/tasks.rs:262-263`).
 *
 * The names rather than line numbers into `src-tauri/` on purpose: those crates
 * are edited by the same wave that reads this file, and a line number is wrong
 * within the day. `keeper-syncd tasks status` already prints this column beside
 * the same outcome, host and time (`task_run_lines`), so the row borrows a
 * settled vocabulary rather than inventing a fifth word of its own.
 */
export const TASK_LAST_REPORT_LABEL = "Last report";

/** What a null in each of those columns honestly means. */
export const TASK_NO_SCHEDULE_TEXT = "none stored";
export const TASK_NEVER_DUE_TEXT = "nothing will make it due";
export const TASK_NEVER_RAN_TEXT = "never run";
export const TASK_DUE_NOW_TEXT = "due now";
export const TASK_IN_FLIGHT_TEXT = "running now";

export const TASKS_ROW_TESTID = "task-row";
/**
 * The row's description line (Story 59.5).
 *
 * A testid rather than a text query, because the fact under test is ABSENCE and
 * a text query cannot see the difference between a paragraph that was not
 * rendered and one rendered around whitespace — which is precisely the bug
 * `taskDescriptionText` exists to prevent. Proved: mutating that helper to
 * return its argument unchanged left a text-based assertion green.
 */
export const TASKS_DESCRIPTION_TESTID = "task-description";
export const TASKS_UNKNOWN_ROW_TESTID = "task-unknown-row";
export const TASKS_HOST_TESTID = "task-host";
export const TASKS_REFUSAL_TESTID = "task-refusal";
/** Where a refusal whose row the listing no longer holds is drawn instead. */
export const TASKS_ORPHAN_REFUSAL_TESTID = "tasks-orphan-refusal";
export const TASKS_ERROR_TESTID = "tasks-error";
export const TASK_FORGET_TESTID = "task-forget-confirm";

// ---------------------------------------------------------------------------
// The two levels (Story 59.1): a list of names, and one task at a time
// ---------------------------------------------------------------------------

/**
 * The accessible name of the region one selected task is drawn in.
 *
 * Distinct from the pane's own name and from the master column's — three
 * regions sit inside this surface and a reader jumping between landmarks has to
 * be able to tell "Tasks" (the surface), "Task list" (the column of names,
 * named by its own entry in `SURFACE_COLUMNS`) and this apart.
 */
export const TASKS_DETAIL_LABEL = "Task detail";

/**
 * What a task panel with nothing in it says (Story 59.12).
 *
 * `PanelStrip`'s own default names the gesture that fills a panel in the Files
 * surface, and "click a file to open it" is the wrong instruction beside a list
 * of task names. Sited here rather than in the strip for `notes-pane.tsx`'s
 * reason: the sentence belongs to the surface that knows what its rows are, and
 * the strip's prop exists exactly so that each host may say its own.
 */
export const TASKS_PANEL_EMPTY_SENTENCE =
  "Nothing is open here yet. Double-click a task to open it beside the list.";

/**
 * What the master list is called to a screen reader.
 *
 * The column region around it is already named *Task list* by its own fold-row
 * heading, so this names the `<ul>` for what its rows are rather than repeating
 * the column: "Tasks" is the set, "Task list" is the column that holds it.
 */
export const TASKS_LIST_LABEL = "Tasks";

export const TASKS_DETAIL_TESTID = "task-detail";

/**
 * The rail the master column still offers once its body is unmounted.
 *
 * `useSurfaceColumn` refuses an empty rail by construction, and getting this
 * right needed one correction worth recording. The first version put Refresh
 * and Add on the strip, `files-pane.tsx`'s two — but this pane's header sits
 * ABOVE both columns rather than inside the folding one, so the fold takes
 * neither of them away and the strip would have offered a second Refresh a
 * screen-reader user could not tell from the first. A test caught it as *Found
 * multiple elements with the role "button" and name "Refresh"*, which is
 * exactly what a person navigating by name would have hit.
 *
 * What the fold genuinely takes is the **names** — and the projected paced rows
 * under them. So the strip does what the Files tree's selection entry does with
 * a selection it cannot show: it says how many there are, and gives them back.
 * Without it a folded Tasks column is a 48px strip that answers nothing.
 */
export const TASKS_RAIL_LIST_LABEL = "Task list";
export const TASKS: CountNoun = { one: "task", many: "tasks" };

// ---------------------------------------------------------------------------
// Several tasks at once (Story 59.4): the selection model, copied from
// `files-pane.tsx` rather than invented
// ---------------------------------------------------------------------------

/**
 * How many names are held, said the same way wherever it is said.
 *
 * `filesSelectionSentence`'s shape and its reasoning (`files-pane.tsx:232-247`):
 * the sentence and not a numeral, because this is the **accessible name** of the
 * count badge and of the region the detail column draws in its place. The badge
 * draws the figure; a reader who cannot see it is told what the figure counts.
 *
 * Through {@link countLabel} and never a hand-rolled plural — one task is `1
 * task selected` and none is `0 tasks selected`, because zero is a number rather
 * than a silence (`count-label.ts:29-31`).
 */
export function tasksSelectionSentence(count: number): string {
  return `${countLabel(count, TASKS)} selected`;
}

/**
 * The three bulk verbs, in the header beside the count (Story 59.4).
 *
 * 45.3's rule for where they live: *a per-row Delete button cannot answer "and
 * the other four"*, so they sit beside the number that makes them safe to press
 * and never on a row — which is also what keeps Story 59.1's no-control-on-the-
 * row invariant true.
 *
 * **Suffixed `selected`, and that is not decoration.** {@link TASK_FORGET_TEXT}
 * is already the accessible name of the detail region's Forget and of the
 * confirmation's own action, and a third button called `Forget` would be
 * indistinguishable to anybody navigating by name — the failure a folded rail
 * with a second `Refresh` on it caused once already. Naming the subject also
 * answers *forget what?* for a control whose subject is a set.
 */
export const TASKS_BULK_ENABLE_TEXT = "Enable selected";
export const TASKS_BULK_DISABLE_TEXT = "Disable selected";
export const TASKS_BULK_FORGET_TEXT = "Forget selected";

/** The count badge beside them, and the region drawn where one task's detail would be. */
export const TASKS_SELECTED_TESTID = "tasks-selected";
export const TASKS_SELECTION_TESTID = "tasks-selection";
/** A whole batch that would not run at all — the store, not one id. */
export const TASKS_BULK_ERROR_TESTID = "tasks-bulk-error";

/**
 * How many tasks are about to be forgotten, asked before any of them are.
 *
 * {@link taskForgetConfirmTitle}'s sibling and its rule — a function, so ten of
 * these do not all confirm with the same words. The **count** and not the ids:
 * the number is what a person is deciding about, and forty ids in a dialog title
 * is a title nobody reads. Through {@link countLabel} for
 * {@link tasksSelectionSentence}'s reason.
 */
export function tasksForgetConfirmTitle(count: number): string {
  return `Forget ${countLabel(count, TASKS)}?`;
}

/**
 * What a `missing` entry says, in the pane's own words.
 *
 * The one sentence on this surface that is **not** Rust's, and it is that way by
 * a documented invariant rather than by oversight: `TaskBatchEntryVm.reason` is
 * non-null exactly for `refused`, so a `missing` id arrives with no sentence at
 * all. Something still has to be said — a bulk action that silently skipped two
 * of five ids is the invisible-failure shape this whole epic exists to close —
 * and it is said here, once, exported so a test can assert it by name
 * ({@link TASKS_UNKNOWN_NO_ID_TEXT}'s precedent).
 *
 * Worded as the benign thing it usually is rather than as a fault: `Missing` is
 * a fourth outcome precisely *because* it is not a refusal.
 */
export const TASKS_BULK_MISSING_TEXT =
  "keeper has no such task on this host — another writer on this shared record may have forgotten it already.";

/**
 * What a refusal that arrived without its sentence says.
 *
 * Unreachable while the wire keeps its invariant, and written anyway: the
 * alternative to a fallback is a refused id rendering as nothing, which reads
 * exactly like a success. A refusal must be seen even when it will not say why.
 */
export const TASKS_BULK_NO_REASON_TEXT = "keeper refused this one and did not say why.";

/**
 * The master column's own box.
 *
 * `shrink-0` and `min-w-0` together, `FILES_COLUMN_CLASS`'s pairing: the width
 * is a remembered number that arrives as an inline `flex-basis` from
 * `useSurfaceColumn`, so the column must not be squeezed below it by a long
 * task name — and must still be allowed to clip that name rather than push the
 * detail region off screen.
 */
const TASKS_COLUMN_CLASS = "flex min-w-0 shrink-0 flex-col border-border border-r bg-background";

/**
 * The disclosure over one task's recorded runs, and the section it reveals.
 *
 * *Runs* is the CLI's word rather than this file's: `keeper-syncd tasks status`
 * prints `"N run(s), newest first"` over this exact column set, so the app
 * borrows a settled vocabulary instead of inventing a sixth word of its own.
 * One constant for both the control and the section it opens, because the
 * trigger and the thing it reveals are worded identically — the rule the
 * header's Add control already follows (`add-folder-form.tsx`'s): a button
 * called something else would be a second name for one list.
 */
export const TASK_HISTORY_TITLE = "Runs";

/**
 * What the Runs control says (Story 59.2).
 *
 * Three shapes, and the distinction between them is the whole point:
 *
 * | state | reads |
 * | --- | --- |
 * | shut, and the listing says the task has never run | `Runs — none yet` |
 * | shut, otherwise | `Runs` |
 * | open | `Runs · 12 runs`, from what the section actually holds |
 *
 * **A closed section prints no number**, and that is a rule rather than an
 * omission. `task_runs` is read when the section opens and never on render, so
 * a count on a shut row could only be guessed — and a guessed total that looks
 * like a real one is exactly what `count-label.ts` was written to make
 * impossible. `lastRun === null` is the single fact the pane may state before
 * opening anything, because the listing carries it already.
 *
 * An OPEN section counts what it holds, not what exists: `task_runs` is capped
 * at fifty per task in the store and the read asks for twenty, so this is the
 * length of the answer in hand. {@link TASK_HISTORY_BOUND_TEXT} is where that
 * is said in words; a bare number here would quietly claim to be a total.
 */
export function taskHistoryTriggerText(
  runs: TaskRunVm[] | null,
  lastRun: TaskRunVm | null,
): string {
  if (runs !== null) {
    return `${TASK_HISTORY_TITLE} · ${countLabel(runs.length, RUNS)}`;
  }
  return lastRun === null ? `${TASK_HISTORY_TITLE} — none yet` : TASK_HISTORY_TITLE;
}

/**
 * That an open section is a page and not the whole history.
 *
 * The numbers were invisible before Story 59.2: the read asks for twenty
 * (`TASK_HISTORY_LIMIT_DEFAULT`), the store keeps fifty per task
 * (`TASK_RUNS_CAP`), and the fold showed ten of the twenty first. A reader who
 * pressed *Show all* therefore reached the end of a list that was not the end
 * of the history, with nothing on screen saying so. This says it once, under
 * the list, and only when the list is long enough for the question to arise.
 */
export const TASK_HISTORY_BOUND_TEXT =
  "Older runs are trimmed: keeper keeps the fifty most recent for each task.";

/**
 * How many runs a section must hold before {@link TASK_HISTORY_BOUND_TEXT} is
 * worth saying.
 *
 * The read's own limit (`TASK_HISTORY_LIMIT_DEFAULT`, twenty), because a section
 * holding fewer than that has not been trimmed by anything: the reader is
 * looking at every run the store has for this task, and warning them that older
 * ones are trimmed would describe something that has not happened.
 */
export const TASK_HISTORY_BOUND_NOTICE_AT = 20;

/**
 * An unread list is UNKNOWN, and not empty.
 *
 * That is the distinction this line exists to keep, and the one a failed read
 * must not collapse: `null` means nobody has answered yet, `[]` means the record
 * holds no runs, and a refusal means the question was refused. Rendering any two
 * of those with the same words would let this pane claim — on a read that never
 * landed — that a task has never run, which is the invisible-failure shape the
 * whole epic exists to close.
 */
export const TASK_HISTORY_LOADING_TEXT = "Reading the runs…";

/**
 * That the record holds no runs, and what would put one here.
 *
 * The first clause is the CLI's own empty state — `task_run_lines` prints
 * `"{task_id}: no runs recorded"` — so the two surfaces answer an empty history
 * with the same phrase. The second is `SYNC_ACTIVITY_EMPTY_SENTENCE`'s rule: an
 * empty list that only says it is empty leaves a reader unable to tell a
 * feature with nothing to show from one that does not work, so this one says
 * what would put a row here instead.
 */
export const TASK_HISTORY_EMPTY_TEXT =
  "No runs recorded. A row appears here each time a host starts this task.";

/**
 * How a refused history read is retried, said out loud.
 *
 * Nothing else re-reads it: a listing refresh deliberately leaves an open
 * section alone, so the only way to ask again is to close the disclosure and
 * open it — which is discoverable only if it is stated. Without this line the
 * refusal is a dead end whose single obvious press (the disclosure) looks like
 * it dismisses the message rather than retrying the read.
 */
export const TASK_HISTORY_RETRY_NOTE = "Close Runs and open it again to ask once more.";

/**
 * A run whose host column is blank.
 *
 * `host` is `TEXT NOT NULL` with no non-empty constraint, so the same writer
 * class {@link taskReportText} exists for can store `""` — and *which host ran
 * this* is the column the history is largely for, so a blank there must read as
 * unrecorded rather than as a gap in the row. `TASKS_UNKNOWN_NO_ID_TEXT` is the
 * precedent: this file already names an absence rather than rendering a blank.
 */
export const TASK_HISTORY_NO_HOST_TEXT = "no host recorded";

/**
 * How many recorded runs the unfolded list still does not show.
 *
 * The fold's unfolded size is a global preference with a floor of ten, while a
 * history page is `TASK_HISTORY_LIMIT_DEFAULT` runs — so a reader who has
 * pressed *Show all* can still be looking at half of what was read, with
 * `FoldToggle` saying only *Show fewer*. Counting the remainder out loud is the
 * cheapest honest fix; the alternative is inventing a page size here, which
 * would disagree with the one Rust already owns.
 */
export function taskHistoryUnshownText(count: number): string {
  return `${count} more recorded and not shown.`;
}

/**
 * The stand-in for an outcome whose stored spelling is blank.
 *
 * A newer keeper's spelling is rendered verbatim (NFR-43), but `""` renders as
 * nothing at all — and this is the leading word of a run row, so an empty one
 * reads as a broken renderer. Worded as the fact it is: something was recorded
 * and this build cannot read it.
 */
export const TASK_UNREADABLE_OUTCOME_TEXT = "an outcome this build cannot read";

export const TASKS_HISTORY_TESTID = "task-history";
export const TASKS_HISTORY_ROW_TESTID = "task-run";
export const TASKS_HISTORY_REFUSAL_TESTID = "task-history-refusal";

/**
 * The word for each host verdict.
 *
 * A short label beside the full sentence, and the two say the same thing at
 * two lengths — the label is what a reader scanning a list of ten tasks sees,
 * the sentence is what tells them what it means. `unhosted` is a word of its
 * own and never folded into `off`: a switched-off task is off on purpose, and an
 * unhosted one looks enabled and will never fire.
 *
 * A kind this build does not know renders as itself rather than as a blank,
 * because `TaskHostKind` can grow in Rust.
 */
const HOST_KIND_LABELS: Record<string, string> = {
  daemon: "Daemon",
  app: "This app",
  onRequest: "On request",
  unhosted: "Unhosted",
  off: "Off",
};

/**
 * The word for each recorded outcome.
 *
 * Only one of the seven is a failure. `busy` and `deferred` are a run that did
 * not happen, `abandoned` is a lease the next host reclaimed, and the two
 * Stories 58.4/58.5 added are a window a policy did not run — so none of them is
 * worded as one.
 *
 * **`declined` and `postponed` are not variants of one idea, and the difference
 * is the whole reason they are two spellings.** A declined window will never be
 * served: `skip` gave up on it and armed the next one. A postponed window WILL
 * be served, later: `delay` is holding it back from the instant a host noticed
 * it. Wording them as shades of the same thing would tell somebody their nightly
 * sweep had been dropped when it had only been held.
 *
 * Both are closed, zero-duration rows, so neither may read as in flight — and
 * neither carries the failure vocabulary, because nothing went wrong in either.
 * The `detail` beside them names the declined or postponed instant and the
 * policy that decided it, which the row's report cell renders unchanged.
 */
const OUTCOME_LABELS: Record<string, string> = {
  ok: "Succeeded",
  busy: "Target was already in use",
  deferred: "Waited for a condition",
  failed: "Failed",
  abandoned: "Abandoned by the host that started it",
  declined: "Not run — the next window was armed instead",
  postponed: "Held back — it will run later",
};

// ---------------------------------------------------------------------------
// The paced class (Story 58.7): what this host paces, which is not a task
// ---------------------------------------------------------------------------

/**
 * The heading over the projected rows.
 *
 * *Also* because the section sits under the task list and is the answer to the
 * question the list above raises — "is this everything keeper does on a clock?"
 * — and *paced by this host* because that is the honest verb: nothing here is
 * scheduled, and the pacing stops when the process does.
 */
export const PACED_HEADING = "Also paced by this host";

/**
 * What the section is, and — before anyone hunts for one — that the controls
 * every row above has do not exist *here*.
 *
 * Said in words rather than left to the absence of buttons, because an absence
 * is indistinguishable from a bug: a reader who knows a Sync task can be run on
 * demand will read a row with no Run now as a row whose Run now failed to
 * render.
 *
 * **"…from this section", not "…at all".** The first draft said nothing here
 * can be run on demand, which is false for two of the three kinds: the scan's
 * work is what the Sync pane's **Sync now** runs, and a vault is flushed on
 * demand every time the window hides or loses focus. A sentence written to stop
 * somebody hunting for a control must not hide the control they were looking
 * for, so it names where the scan can be asked for instead.
 */
export const PACED_SUBTITLE =
  "Work keeper paces on its own. These are not tasks: nothing here has a schedule you can set, and none of it can be started from this section — a folder's own Sync now is on the Sync pane.";

/**
 * The badge on such a row, `TASKS_UNKNOWN_BADGE`'s idiom: **the row's standing**,
 * not the class it belongs to.
 *
 * *Paced* and not *Automatic* or *Internal*: it is the one word that says the
 * clock in this process drives it, which is exactly what distinguishes the row
 * from every task above it.
 *
 * It used to be that one word on **every** row, because the badge named the
 * class the way the section heading does. Rendered, that put *Paced* beside
 * *"this folder is paused, so nothing here is paced and no cadence is in
 * force."* — the badge contradicting the sentence one line below it, on the same
 * row, in a section whose whole purpose is not over-claiming. No test could see
 * it: each half was correct on its own. The class is the heading's job, so the
 * badge now carries the standing, and a standing this build does not know
 * renders as its own spelling (`PACED_KIND_LABELS`' rule, because
 * `PacedWorkStanding` can grow in Rust).
 */
export const PACED_BADGE = "Paced";
export const PACED_STANDING_LABELS: Record<string, string> = {
  paced: PACED_BADGE,
  paused: "Paused",
  governed: "Scheduled",
  unregistered: "Not registered",
};

/** Before this section's read lands the projection is unknown, not empty. */
export const PACED_LOADING_TEXT = "Reading what this host paces…";

/**
 * That keeper paces nothing here, and what would put a row in this section.
 *
 * Every projected row is per-folder, so this sentence is about **what keeper
 * paces** and deliberately not about what the machine has. It used to say a
 * reader had no folders, which is a claim the projection cannot make: a profile
 * row this build cannot deserialize is skipped by `list_profiles` rather than
 * counted, so on a downgrade an empty projection and a machine full of folders
 * look identical from here. The unknown-row surfaces above this section are
 * where that fact belongs.
 */
export const PACED_EMPTY_TEXT =
  "keeper paces nothing here. A folder gets a cadence as soon as it is added and enabled.";

/** Column labels, so a projected row reads without a table header above it. */
export const PACED_CADENCE_LABEL = "Cadence";
export const PACED_FOLDER_LABEL = "Folder";

/**
 * What stands in for an absent cadence.
 *
 * `cadence` is null only for a standing that is not `paced` — paused, governed
 * or an unregistered vault — and in every case the row's sentence already says
 * which, so this cell says the one thing the *cadence* column can honestly say
 * and never guesses a reason.
 */
export const PACED_NO_CADENCE_TEXT = "nothing paces it";

export const PACED_ROW_TESTID = "paced-row";
export const PACED_REFUSAL_TESTID = "paced-refusal";

/**
 * The short label for each projected kind, `HOST_KIND_LABELS`'s idiom and its
 * fallback rule: a kind this build does not know renders as its own spelling
 * rather than as a blank, because `PacedWorkKind` can grow in Rust.
 *
 * Each label is a verb phrase and not a noun, because these are things keeper
 * *does* rather than records it keeps — and *Look for changes* rather than
 * *Scan* for the reason the row's sentence spells out at length: the interval is
 * a backstop behind the watcher, and "scan" reads like the only route in.
 */
export const PACED_KIND_LABELS: Record<string, string> = {
  scan: "Look for changes",
  scratchSweep: "Sweep transfer scratch",
  notesCadence: "Notes commit and push",
};

/**
 * How long until a task comes due, or that it is already due.
 *
 * Coarse for `formatSyncWaited`'s reason: this is re-rendered on every read, and
 * a second-by-second countdown to a housekeeping pass would read as a promise
 * about when it lands. A window that has already opened says so rather than
 * counting up, because "due now" is the fact and "3 min ago" would invite the
 * reader to conclude something went wrong.
 */
export function formatTaskDue(nextDueMs: number | null, now: number = Date.now()): string {
  if (nextDueMs === null) {
    return TASK_NEVER_DUE_TEXT;
  }
  const aheadMs = nextDueMs - now;
  if (aheadMs <= 0) {
    return TASK_DUE_NOW_TEXT;
  }
  const minutes = Math.floor(aheadMs / 60_000);
  if (minutes < 1) {
    return "in under a minute";
  }
  if (minutes < 60) {
    return `in ${minutes} min`;
  }
  const hours = Math.floor(aheadMs / 3_600_000);
  if (hours < 24) {
    return `in ${hours} hr`;
  }
  const days = Math.floor(aheadMs / 86_400_000);
  return days === 1 ? "in 1 day" : `in ${days} days`;
}

/**
 * How long ago something happened, at the same coarseness and from the same
 * instants.
 */
export function formatTaskAgo(atMs: number, now: number = Date.now()): string {
  const elapsedMs = Math.max(0, now - atMs);
  const minutes = Math.floor(elapsedMs / 60_000);
  if (minutes < 1) {
    return "just now";
  }
  if (minutes < 60) {
    return `${minutes} min ago`;
  }
  const hours = Math.floor(elapsedMs / 3_600_000);
  if (hours < 24) {
    return `${hours} hr ago`;
  }
  const days = Math.floor(elapsedMs / 86_400_000);
  return days === 1 ? "1 day ago" : `${days} days ago`;
}

/**
 * The sentence a rejection carries.
 *
 * **Not `instanceof Error`, and that distinction is the whole function.**
 * `client.ts` normalises every rejection into an {@link IpcError} — a plain
 * object with `code`, `message` and `retriable`, never an `Error` — because
 * Tauri's `invoke` maps a Rust `Err` to a *value*. An `instanceof Error` check
 * therefore misses the one case that actually happens and renders
 * `"[object Object]"` where the engine's own refusal should be, which is the
 * only actionable half of a refused Run now.
 */
function messageOf(cause: unknown): string {
  if (typeof cause === "object" && cause !== null && "message" in cause) {
    const { message } = cause;
    if (typeof message === "string") {
      return message;
    }
  }
  return String(cause);
}

/**
 * What the last run ended as.
 *
 * The three states of the pair are all distinct facts and each gets its own
 * words: both keys null means the run is still in flight, `unknownOutcome`
 * carries the stored spelling a newer keeper wrote — rendered verbatim, never as
 * "unknown" — and an `outcome` this build knows gets its label.
 */
export function taskOutcomeText(run: TaskRunVm | null): string {
  if (run === null) {
    return TASK_NEVER_RAN_TEXT;
  }
  if (run.unknownOutcome !== null) {
    // Blank counts as unreadable, {@link taskReportText}'s rule applied to the
    // one cell that must never be empty: a stored `outcome` of `""` does not
    // parse either, so it arrives here as `unknownOutcome: ""` — and rendering
    // it verbatim would leave a run row whose leading word is nothing at all,
    // which reads as a rendering fault rather than as a spelling this build
    // cannot read. Falling through instead would be worse: the next branch
    // would call a closed run "running now".
    return run.unknownOutcome.trim() === "" ? TASK_UNREADABLE_OUTCOME_TEXT : run.unknownOutcome;
  }
  if (run.outcome === null) {
    return TASK_IN_FLIGHT_TEXT;
  }
  return OUTCOME_LABELS[run.outcome] ?? run.outcome;
}

/**
 * The run's own words, or `null` when there are none to draw.
 *
 * Blank counts as absent, and that is the guard rather than a nicety: `detail`
 * is `TEXT NULL` with no non-empty constraint and `finish_task_run` binds
 * whatever it is handed, so a writer this build never met — the NFR-43 case this
 * file exists to tolerate — can store `""` or `" "`. On `!== null` alone that
 * renders a LAST REPORT heading over nothing, which is the one shape a reader
 * really would read as a failed read. Trimmed to decide, untrimmed to draw: what
 * is stored is what is shown.
 *
 * A function rather than the row-local const Story 58.2 wrote, because two
 * places now need this exact rule — the row's own cell and every row of the run
 * history — and a row and the history hanging under it must not disagree about
 * what *no report* means.
 */
export function taskReportText(run: TaskRunVm | null): string | null {
  if (run === null || run.detail === null || run.detail.trim() === "") {
    return null;
  }
  return run.detail;
}

/**
 * A task's own words, or `null` when it has none (Story 59.5).
 *
 * {@link taskReportText}'s rule, applied to the other free-text column and for
 * the same reason: `description` is `TEXT NULL` with no non-empty constraint,
 * the form deliberately sends what was typed **untrimmed**, and `tasks set
 * --description "   "` is a write nothing refuses. So blank and absent must
 * collapse to one rendered state — nothing — while what is stored stays exactly
 * what the person typed. Trimmed to decide, untrimmed to draw.
 *
 * The two states stay distinct everywhere they matter: the store keeps `NULL`
 * and `""` apart (that is 59.5's whole column argument, and the dev shell keeps
 * a `""` fixture to prove it), and `--no-description` restores the absent case
 * rather than the blank one. This function is about the SCREEN, where a heading
 * over an empty string reads as a failed read.
 */
export function taskDescriptionText(description: string | null): string | null {
  if (description === null || description.trim() === "") {
    return null;
  }
  return description;
}

/** One `label: value` cell, so a row reads without a table header above it. */
function Field({
  label,
  wide = false,
  children,
}: {
  label: string;
  /**
   * Hold an engine sentence rather than a short value: span the whole grid, and
   * wrap the way the rest of the app wraps engine text.
   *
   * The other four cells are short in every shape this build writes — a cron
   * expression, a coarse relative time, an outcome label. `detail` has no bound
   * at all, so a quarter-width column wraps a git error to five lines and
   * pushes the host claim off the fold. (The claim is only that they are short,
   * not that they cannot grow: `taskOutcomeText` renders an unknown spelling
   * verbatim and the schedule cell renders whatever is stored, so NFR-43 can
   * make either of them long.)
   *
   * Width alone is not enough, and both extra classes are the Sync pane's,
   * copied because this is the same input: `[overflow-wrap:anywhere]` because
   * `min-w-0` shrinks the track but nothing breaks an unbreakable token like
   * `fatal: unable to access 'https://…/long/path.git/'` (`sync-pane.tsx:1444`
   * renders a git failure that way for this exact reason), and
   * `whitespace-pre-wrap` because a git reason arrives with line breaks in it
   * and HTML would collapse them — which would make this file's promise to
   * render the string verbatim false (`sync-git-row.tsx:170`).
   */
  wide?: boolean;
  children: React.ReactNode;
}) {
  return (
    <div className={wide ? "col-span-2 min-w-0 sm:col-span-4" : "min-w-0"}>
      <dt className="text-muted-foreground text-xs uppercase tracking-wide">{label}</dt>
      {/* `[overflow-wrap:anywhere]` on BOTH branches, and only
          `whitespace-pre-wrap` is the wide cell's own. A narrow cell holds a
          short value in every shape this build writes — except one: the paced
          rows' Folder cell holds a **profile name**, which is user-typed and
          validated only as non-empty, so a pasted path with no spaces in it is
          an unbreakable token in a quarter-width track. `min-w-0` shrinks the
          track and nothing breaks the word, so the row pushed the pane sideways.
          Line breaks stay collapsed here, because a narrow value has no reason
          to carry them and an engine sentence belongs in a wide cell. */}
      <dd
        className={
          wide
            ? "whitespace-pre-wrap text-foreground text-sm [overflow-wrap:anywhere]"
            : "text-foreground text-sm [overflow-wrap:anywhere]"
        }
      >
        {children}
      </dd>
    </div>
  );
}

/**
 * One task's recorded runs, newest first as Rust ordered them (Story 58.3).
 *
 * `SyncActivityList`'s idiom (`sync-pane.tsx`, module-private there so this is a
 * citation and not a link), deliberately rather than by accident of
 * copying: the loading line, the empty sentence and the `useFold`/`FoldToggle`
 * truncation are all that list's, because this file's own header promises it
 * reuses the Sync pane's list idioms *"rather than inventing a third"*. What it
 * does not copy is that list's `aria-label`, and the reason is below.
 *
 * The columns are the CLI's, in the CLI's order — outcome word, relative time,
 * host, report — as `task_run_lines` prints them, so a reader who has used
 * `keeper-syncd tasks status` finds the same four facts in the same places.
 * Cross-crate facts here are named by symbol and never by line: those crates are
 * edited by the same wave that reads this file.
 */
function TaskRunList({
  taskId,
  regionId,
  runs,
  error,
  now,
}: {
  taskId: string;
  /** What the disclosure's `aria-controls` points at, so the pair is named once. */
  regionId: string;
  /** `null` until this section's read lands: unread, and not empty. */
  runs: TaskRunVm[] | null;
  error: string | null;
  now: number;
}) {
  const fold = useFold(runs);
  return (
    <div id={regionId} data-testid={TASKS_HISTORY_TESTID} className="flex flex-col gap-2">
      {/* Drawn INSTEAD of both sentences below and never instead of the rows: a
          re-read that is refused leaves every row already on screen exactly
          where it was and adds the engine's sentence about why it knows no more.
          A failed read is a fault to report, not a fact to invent. */}
      {error !== null && (
        <p
          role="alert"
          data-testid={TASKS_HISTORY_REFUSAL_TESTID}
          className="text-destructive text-xs"
        >
          {error}
          {/* The only way to ask again, because nothing re-reads this on its
              own. Inside the alert so it is announced with the refusal it is
              about. */}
          <span className="block text-muted-foreground">{TASK_HISTORY_RETRY_NOTE}</span>
        </p>
      )}
      {/* Three states and three sets of words, which is the whole point of this
          ternary's shape: `null` means unread, `[]` means the record holds no
          runs, and they never render the same sentence.

          BOTH sentences yield to a refusal, and the empty one for a sharper
          reason than the loading line: a refused re-read keeps the rows it had,
          so on a task that read `[]` and was then run, the section would say
          "no runs recorded" beside "database is locked" at the exact instant
          `claim_task` had written a run row. The refusal explains why the pane
          knows no more; the sentence beside it would be a claim it cannot
          support. `role="status"` on both, because a reader who presses Runs and
          is told nothing has to go hunting for the answer. */}
      {error === null &&
        (runs === null ? (
          <p role="status" className="text-muted-foreground text-xs">
            {TASK_HISTORY_LOADING_TEXT}
          </p>
        ) : (
          runs.length === 0 && (
            <p role="status" className="text-muted-foreground text-xs">
              {TASK_HISTORY_EMPTY_TEXT}
            </p>
          )
        ))}
      {runs !== null && runs.length > 0 && (
        // Deliberately unnamed: the disclosure owns the accessible name and
        // points here with `aria-controls`, and a list repeating that name would
        // give a screen reader two targets called the same thing
        // (`tag-combobox.tsx`'s rule for the same pairing). The Activity list's
        // `aria-label` is safe only because its name comes from an `<h2>`.
        <ul className="flex flex-col gap-1.5">
          {fold.visible.map((entry) => {
            const report = taskReportText(entry);
            return (
              // `task_runs.id` is an INTEGER PRIMARY KEY and `TaskRunVm` carries
              // it, and this list is one `ORDER BY id DESC` over that column —
              // so unlike the Activity list, whose rows have no identity of
              // their own and are therefore keyed by timestamp, kind and path,
              // this one needs no composite key.
              <li
                key={entry.id}
                data-testid={TASKS_HISTORY_ROW_TESTID}
                className="flex flex-wrap items-baseline gap-2 text-xs"
              >
                {/* The row above's own function, reused rather than re-worded:
                    an in-flight run, a known outcome and a spelling a newer
                    keeper wrote stay three distinct facts (NFR-43), and the
                    history cannot word an outcome differently from the row it
                    hangs under. */}
                <span className="text-foreground">{taskOutcomeText(entry)}</span>
                {/* The pane's existing display clock, threaded in — never a
                    second one, so two times on one screen cannot disagree about
                    what "now" is. */}
                <span className="figures shrink-0 text-muted-foreground">
                  {formatTaskAgo(entry.startedMs, now)}
                </span>
                {/* Which host ran it is most of the reason this list exists, so
                    a blank is named rather than left as a gap, and the string is
                    allowed to break: it is a stored id this build did not
                    choose, so `shrink-0` on it would push the report out of the
                    row at a narrow width. */}
                <span className="font-mono text-muted-foreground [overflow-wrap:anywhere]">
                  {entry.host.trim() === "" ? TASK_HISTORY_NO_HOST_TEXT : entry.host}
                </span>
                {/* Absent or blank is silence, `taskReportText`'s rule. The
                    wrapping is the pair Story 58.2 established for engine prose,
                    because this is the same unbounded string in a narrower
                    place. */}
                {report !== null && (
                  <span className="min-w-0 flex-1 whitespace-pre-wrap [overflow-wrap:anywhere]">
                    {report}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {runs !== null && runs.length > 0 && (
        <FoldToggle rows={runs} fold={fold} label={`${TASK_HISTORY_TITLE}: ${taskId}`} />
      )}
      {/* Unfolded and still holding rows back, which `FoldToggle` cannot say: it
          renders "Show fewer" and nothing else, so a reader who pressed *Show
          all* on a twenty-run history whose unfolded size is ten would believe
          they had seen all of it. */}
      {runs !== null && fold.expanded && fold.hidden > 0 && (
        <p className="text-muted-foreground text-xs">{taskHistoryUnshownText(fold.hidden)}</p>
      )}
      {/* And that the whole list is a page of a longer record, which nothing on
          screen said before Story 59.2. Three numbers were invisible at once:
          the read asks for twenty, the store keeps fifty per task, and the fold
          shows ten of the twenty first — so a reader who pressed *Show all* and
          then read the last row had reached the end of neither. Said only when
          the section is full enough for the question to arise, because on a
          three-run task it is a sentence about nothing. */}
      {runs !== null && runs.length >= TASK_HISTORY_BOUND_NOTICE_AT && (
        <p className="text-muted-foreground text-xs">{TASK_HISTORY_BOUND_TEXT}</p>
      )}
    </div>
  );
}

/**
 * One task as a single line in the master list (Story 59.1).
 *
 * Level 1 of the two the owner asked for: *"the task list it would be good to
 * see the list of the saved names … -> detail"*. Before this the row WAS the
 * detail — ten stacked blocks and three buttons, ~250px of card each — so
 * reaching the eighth task's runs meant scrolling past seven of them, and every
 * capability epic 58 shipped was on screen and unfindable.
 *
 * Five facts and no controls: the kind, the mode, the name, the host and when
 * it is next due. Four of those are the epic's own list; the mode badge is here
 * because Story 59.3's acceptance is that *the row* states a task is scheduled,
 * and a fact that moved into the detail would quietly un-ship it. Everything
 * else is re-sited into {@link TaskDetail} and **not one word of it is
 * reworded**.
 *
 * No button but the row itself. A control here would be back in the `shrink-0`
 * cluster Story 58.3 already had to move the `Runs` link out of, on a row that
 * is now a third of its old width.
 *
 * `aria-selected` on **every** row, `"true"` and `"false"` alike, and the
 * attribute is the story that changed. Story 59.1 chose `aria-current` on the
 * deliberate ground that `aria-selected` announces a *set* to a reader when only
 * one thing can be chosen (`chat-row.tsx:266` is the app's single-selection
 * idiom) — and that ground is exactly what Story 59.4 removes. The attribute and
 * the refusal flip **together**: while a bulk consumer did not exist a selection
 * would have been state no command could act on, so `aria-current` was right;
 * now that `sync_tasks_set_enabled` and `sync_tasks_forget` exist,
 * `aria-selected` is. Never omitted on the unselected rows, `files-pane.tsx:2528-2532`'s
 * stated reason: a list that marked only the selected rows would leave a screen
 * reader unable to say *not selected* about the others.
 *
 * **A `role="option"` box and not a `<button>`, which is a consequence rather
 * than a preference.** `aria-selected` is not a supported state on
 * `role="button"`, so the element carrying the selection has to be the option
 * itself. That is what `files-pane.tsx:2523` does for the same reason — a `<div
 * role="treeitem">` with its own `tabIndex`, `onClick` and `onKeyDown` rather
 * than a button — and the keyboard activation the `<button>` gave for free is
 * added back by hand.
 *
 * The `<li>` is gone with it, and that is the same consequence one level up: a
 * listbox has to own its options directly, `<li>` is only valid inside a
 * `<ul>`/`<ol>`, and `role="listbox"` on a `<ul>` is a non-interactive element
 * given an interactive role — which the project's own lint refuses by name
 * (`a11y/noNoninteractiveElementToInteractiveRole`, *"replace ul with a div"*).
 * So the container is a `<div role="listbox">` and each row is one
 * `<div role="option">` inside it, which is exactly the shape
 * `files-pane.tsx:3096-3113` already uses for its tree. The row's
 * `data-testid`/`data-task-id` move onto that element: one row is now one
 * element, so there is no wrapper for them to sit on and nothing that could
 * disagree about which row is which.
 *
 * Still no control on the row, which this story strengthens rather than relaxes:
 * each line now exposes exactly one `option` and **zero** buttons.
 */
function TaskRow({
  task,
  now,
  selected,
  tabIndex,
  optionRef,
  onRowClick,
  onRowDoubleClick,
  onActivate,
}: {
  task: TaskVm;
  now: number;
  selected: boolean;
  /**
   * The roving tab stop: `0` on the row the cursor is at and `-1` on every
   * other, so the list is one stop rather than twenty
   * (`chat-list-pane.tsx:756-760`). Under a single selection the cursor IS the
   * selection — there is no second highlight to keep in step with it; under a
   * set it is the anchor, which is the row a Shift-range would measure from.
   */
  tabIndex: number;
  optionRef: (element: HTMLDivElement | null) => void;
  /** The whole event, because the modifier is what decides the gesture. */
  onRowClick: (event: ReactMouseEvent<HTMLDivElement>, id: string) => void;
  /** The whole event too, for the same reason: the guards a double click has to
   *  pass are the ones a single click passes, and they are decoded in one place. */
  onRowDoubleClick: (event: ReactMouseEvent<HTMLDivElement>, id: string) => void;
  /** Enter, which a `<button>` used to answer for nothing. */
  onActivate: (id: string) => void;
}) {
  const unhosted = task.host.kind === "unhosted";
  return (
    <div
      ref={optionRef}
      role="option"
      data-testid={TASKS_ROW_TESTID}
      data-task-id={task.id}
      tabIndex={tabIndex}
      aria-selected={selected}
      onClick={(event) => onRowClick(event, task.id)}
      onDoubleClick={(event) => onRowDoubleClick(event, task.id)}
      onKeyDown={(event) => {
        // Space belongs to the list's own handler, where the modifier that
        // makes it a toggle is decoded beside the arrows — one place for the
        // keys this list owns. Enter is the plain activation the element lost
        // when it stopped being a button.
        if (event.key === "Enter") {
          event.preventDefault();
          onActivate(task.id);
        }
      }}
      className={cn(
        "flex w-full flex-col gap-1 border-border border-b px-3 py-2 text-left",
        "outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        selected ? "bg-accent" : "hover:bg-accent",
      )}
    >
      <span className="flex min-w-0 items-center gap-2">
        {/* Both stored spellings, the kind badge's rule: a kind or a mode a
            newer keeper wrote is shown rather than hidden (NFR-43). */}
        <Badge variant="secondary">{task.kind}</Badge>
        <Badge variant="outline">{task.mode}</Badge>
        <span className="truncate font-medium text-foreground text-sm">{task.id}</span>
      </span>
      <span className="flex min-w-0 items-center justify-between gap-2 text-xs">
        {/* The host WORD only. Its sentence and its reason are Rust's and are
            rendered whole in the detail — a line this narrow would clip them,
            and a clipped host claim is the one thing AD-137 cannot tolerate.
            What the word has to carry alone is the alarm, which is why an
            unhosted row is coloured here as well as named. */}
        <span className={unhosted ? "truncate text-destructive" : "truncate text-muted-foreground"}>
          {HOST_KIND_LABELS[task.host.kind] ?? task.host.kind}
        </span>
        <span className="shrink-0 text-muted-foreground">{formatTaskDue(task.nextDueMs, now)}</span>
      </span>
    </div>
  );
}

/**
 * The selected task, whole (Story 59.1).
 *
 * Level 2, and every line of it is what the flat card used to be: the field
 * grid, the host block and its Rust-composed sentence, the refusal, the three
 * controls, the `Runs` disclosure and the edit form. **This is a re-siting, not
 * a rewrite** — the epic's own instruction, because every fact epic 58 put on
 * the row is a fact somebody needs. Nothing here is new copy.
 *
 * What it gains from the move is room. Story 58.3 pulled the `Runs` control out
 * of a `shrink-0` strip that already held three buttons, and Story 59.2 then
 * promoted it to a count and a chevron on the argument that *"the detail region
 * this now sits in is not competing with three buttons for a narrow row's
 * width"* — a sentence written against a region that did not exist yet. This is
 * that region; the sentence is honoured rather than amended.
 *
 * # Two hosts, one rendering (Story 59.12)
 *
 * The paragraph that stood here said *"a region and not a panel: `PanelStrip`'s
 * targets are documents opened in an editor, and a task is not one"*. The owner
 * then asked for a task he could open in a tab, and 59.12 gave `PanelTargetVm`
 * a `task` variant — so this component now has **two hosts**: the pane's own
 * detail region, and a task panel in the strip beside it. Two hosts and ONE
 * rendering, deliberately: two components over one task could word the same
 * fact differently, which is the defect shape this codebase keeps closing.
 *
 * All of the difference between the two hosts is {@link TaskDetailVerbs}, and a
 * host that does not write passes `null` rather than nine inert props. A `null`
 * cannot drift; nine no-ops can, and the first one somebody wires up by mistake
 * is a write from the host that must not write.
 *
 * **Why the panel does not write, in the words of the flag that decides it.**
 * `writing` is `formSaving`, and `formSaving` is pane-wide *because* two write
 * surfaces over one task undo each other: `upsert_task` inserts when the id is
 * absent, so a Forget confirmed mid-save deletes a row the settling save then
 * re-inserts. That rule is enforceable only by a host that can see its own
 * in-flight writes, and a second host by definition cannot see the first's. So
 * the pane writes and the panel reads, and the read-only host is expressed as
 * the absence of the verbs rather than as a disabled copy of them.
 *
 * The `Runs` disclosure stays in both, which is that same rule rather than an
 * exception to it: `sync_task_history` is a **read**, it takes the id and
 * nothing else, and a detail that could not show a task's runs would be a
 * strictly poorer copy of the region it is a copy of.
 */
export interface TaskDetailVerbs {
  running: boolean;
  /** This task's own Forget is in flight, so a second confirm cannot re-issue it. */
  deleting: boolean;
  editing: boolean;
  /** A save is in flight somewhere in the pane — see `formSaving`. */
  writing: boolean;
  onRunNow: (id: string) => void;
  onEditToggle: (id: string) => void;
  onSaved: () => void;
  onSavingChange: (saving: boolean) => void;
  onForget: (id: string) => void;
}

export function TaskDetail({
  task,
  now,
  refusal,
  historyOpen,
  historyRuns,
  historyError,
  onHistoryToggle,
  heading = true,
  verbs,
}: {
  task: TaskVm;
  now: number;
  refusal: string | null;
  /** Whether this task's runs are the host's one open section. */
  historyOpen: boolean;
  /**
   * The runs read for this task: `null` while unread, `[]` for a task with none.
   *
   * The data rather than a rendered node, so this component stays a pure
   * function of its task and no caller can hand it another task's runs.
   */
  historyRuns: TaskRunVm[] | null;
  historyError: string | null;
  onHistoryToggle: (id: string) => void;
  /**
   * Whether the task's id is drawn as this region's heading.
   *
   * True in the pane, whose region is a section about exactly one task — the id
   * is its title rather than one cell among five. False in a panel, which is
   * already named twice over by the frame around it: the `aria-label` on its
   * `<section>` and the header row under it. `PanelFrame` refuses a heading for
   * a file for the reason that applies here word for word — *a second `h2`
   * naming the same document would put two entries in a screen reader's heading
   * list for one document* — and under the lockstep a plain click leaves, the
   * pane's region beside the panel is holding the very same task, so the two
   * entries would be the same word twice.
   *
   * Defaulted rather than required, because the heading is what this region has
   * always drawn: a host that says nothing gets what Story 59.1 shipped.
   */
  heading?: boolean;
  /** Everything that changes the task record, or `null` in a host that reads. */
  verbs: TaskDetailVerbs | null;
}) {
  const editing = verbs?.editing ?? false;
  // Both flags refuse the disclosure while a write to this task is on its way,
  // and both are false in a host with no verbs — nothing it can do could be the
  // write they are guarding against.
  const busy = verbs !== null && (verbs.writing || verbs.deleting);
  const unhosted = task.host.kind === "unhosted";
  const report = taskReportText(task.lastRun);
  // Names the region the disclosure genuinely opens, which this project treats
  // as a requirement rather than a nicety (`sidebar-pane.tsx`, `note-editor.tsx`
  // and two guard tests): `aria-expanded` alone announces "collapsed" and gives
  // a screen-reader user nothing to jump to. Passed only while the section
  // exists, `note-editor.tsx`'s form, so there is never a dangling IDREF.
  const historyRegionId = useId();
  // The header disclosure's rule: the form is closed from inside itself, so
  // without this focus lands on `<body>`.
  const editTriggerRef = useRef<HTMLButtonElement>(null);
  const wasEditing = useRef(false);
  useEffect(() => {
    if (!editing && wasEditing.current) {
      editTriggerRef.current?.focus();
    }
    wasEditing.current = editing;
  }, [editing]);
  return (
    <div
      data-testid={TASKS_DETAIL_TESTID}
      data-task-id={task.id}
      className="flex flex-col gap-3 px-6 py-4"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <Badge variant="secondary">{task.kind}</Badge>
            <Badge variant="outline">{task.mode}</Badge>
            {/* The name, and the reason it is a heading here and a span in the
                list: this region is about exactly one task, so the id is its
                title rather than one cell among five. A panel draws the same
                string with the same weight and no heading semantics — see
                {@link heading}; the styling is one class list either way, so
                the two hosts cannot come to look different. */}
            {heading ? (
              <h2 className="truncate font-medium text-foreground text-sm">{task.id}</h2>
            ) : (
              <span className="truncate font-medium text-foreground text-sm">{task.id}</span>
            )}
          </div>
          {/* The task's own words, when it has any (Story 59.5). Under the name
              because the name is what it describes, and absent when blank —
              `TASK_LAST_REPORT_LABEL`'s rule: a heading over an empty string
              reads as a failed read. */}
          {taskDescriptionText(task.description) !== null && (
            <p data-testid={TASKS_DESCRIPTION_TESTID} className="text-foreground text-xs">
              {taskDescriptionText(task.description)}
            </p>
          )}
          <p className="truncate text-muted-foreground text-xs">
            {task.profile ?? (task.profileId === null ? TASK_HOST_WIDE_TEXT : task.profileId)}
          </p>
        </div>
        {/* Absent, not disabled, in a read-only host: a disabled control says
            *not now*, and in a task panel the truth is *not here* — see this
            component's header for why the pane is the only writer. */}
        {verbs !== null && (
          <div className="flex shrink-0 items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={verbs.running}
              onClick={() => verbs.onRunNow(task.id)}
            >
              {TASK_RUN_NOW_TEXT}
            </Button>
            {/* A disclosure, not a dialog: the same component the header reveals,
                in the region it is about (AD-C7). Disabled while a save is in
                flight for the reason the header's twin is — pressing it unmounts
                the form Rust's answer has to land in. */}
            <Button
              ref={editTriggerRef}
              type="button"
              variant="outline"
              size="sm"
              aria-expanded={verbs.editing}
              disabled={verbs.writing}
              onClick={() => verbs.onEditToggle(task.id)}
            >
              {TASK_EDIT_TEXT}
            </Button>
            {/* Refused twice over while a write is on its way: `upsert_task`
                inserts when the id is absent, so a deletion confirmed mid-save is
                undone by the save settling behind it. `deleting` is the second
                confirm of the same delete. */}
            <Button
              type="button"
              variant="destructive"
              size="sm"
              disabled={verbs.writing || verbs.deleting}
              onClick={() => verbs.onForget(task.id)}
            >
              {TASK_FORGET_TEXT}
            </Button>
          </div>
        )}
      </div>

      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field label={TASK_SCHEDULE_LABEL}>{task.schedule ?? TASK_NO_SCHEDULE_TEXT}</Field>
        <Field label={TASK_NEXT_DUE_LABEL}>{formatTaskDue(task.nextDueMs, now)}</Field>
        <Field label={TASK_LAST_RUN_LABEL}>
          {task.lastRun === null ? TASK_NEVER_RAN_TEXT : formatTaskAgo(task.lastRun.startedMs, now)}
        </Field>
        <Field label={TASK_LAST_OUTCOME_LABEL}>{taskOutcomeText(task.lastRun)}</Field>
        {/* Absence, never an empty cell and never a sentence this file invented.
            The states that arrive with no report are all already named by a cell
            beside this one: `lastRun === null` — never ran, and a third copy of
            that one fact is exactly what the refusal test's *never run appears
            twice* count protects; an in-flight run, whose row `claim_task` opens
            with `detail` unset, so it has reported nothing yet; and a reclaimed
            lease, which both `claim_task` and `release_host_leases` write as
            `abandoned` without touching `detail` — so nothing here is a failed
            read. `SyncActivityList` settled the rule for this shape: "A size
            nobody measured shows nothing at all: `0 B` would claim the file was
            empty, and `unknown` is noise on a line already busy answering
            when." */}
        {report !== null && (
          <Field label={TASK_LAST_REPORT_LABEL} wide>
            {report}
          </Field>
        )}
      </dl>

      {/* The host claim, and the one place on screen it comes from whole. The
          list beside this carries the WORD so a person can scan for an unhosted
          task; the sentence and the reason are Rust's, verbatim, and live here
          because this is the only region wide enough to hold them unclipped. */}
      <div data-testid={TASKS_HOST_TESTID} data-host-kind={task.host.kind} className="min-w-0">
        <dt className="text-muted-foreground text-xs uppercase tracking-wide">{TASK_HOST_LABEL}</dt>
        <dd className="flex flex-wrap items-baseline gap-2 text-sm">
          <Badge variant={unhosted ? "destructive" : "outline"}>
            {HOST_KIND_LABELS[task.host.kind] ?? task.host.kind}
          </Badge>
          <span className={unhosted ? "text-destructive" : "text-foreground"}>
            {task.host.sentence}
          </span>
          {/* Non-null only for an unhosted task, so its presence IS the alarm. */}
          {task.host.reason !== null && (
            <span className="text-muted-foreground">{task.host.reason}</span>
          )}
        </dd>
      </div>

      {/* A refusal, quoted where it was asked — from a Run now, or from a
          Forget the engine would not do. The region keeps every other value it
          had: nothing here may read as though the task ran or went away. */}
      {refusal !== null && (
        <p role="alert" data-testid={TASKS_REFUSAL_TESTID} className="text-destructive text-sm">
          {refusal}
        </p>
      )}

      {/* A control that reads as one (Story 59.2). It was a bare
          dotted-underline link at the very bottom of the row, and the owner —
          running the build with tasks in it for the first time — reported that
          he could not see a task's runs at all. The link was there; it was last,
          after the field grid, the host block and any refusal, and it carried no
          affordance, no count and no chevron.

          What that overturns, and what it does not. `FoldToggle`'s rule stands
          for a FOLD — "how much of a list is on screen … is not an action on the
          folder" — but this is not a fold: it is the only route to a task's
          history, which the original comment itself conceded made it "the most
          load-bearing thing on the row". A route is not a fold, and the two
          deserve different weight. Story 58.3's other reason was the `shrink-0`
          cluster at the top; Story 59.1 answered it rather than arguing with it,
          because this region is not competing with three buttons for a narrow
          row's width.

          The count is the affordance that costs nothing: `historyRuns` is
          already in hand once opened, so an opened section can say how many it
          holds without a second read, and a closed one says nothing rather than
          guessing — a number the pane has not read is a number it must not
          print. `lastRun === null` is the one thing it may say before opening,
          because that fact is on the listing already.

          Refused while a write is on its way, the rule Edit and Forget already
          follow: opening this closes an edit form, so pressing it mid-save would
          unmount the form Rust's answer has to land in — and a task whose Forget
          is in flight is about to go, so answering "no runs recorded" about it
          would be a claim about a record that is leaving.

          No focus-return effect, unlike Edit: on the self-close path focus never
          leaves the trigger, which is also why the section needs no close
          control of its own. */}
      <Button
        type="button"
        variant="ghost"
        size="sm"
        aria-expanded={historyOpen}
        aria-controls={historyOpen ? historyRegionId : undefined}
        // Named for its task, `FoldToggle`'s reason — kept even though exactly
        // one of these is on screen now, because the name a reader hears should
        // not depend on how many happen to be mounted.
        aria-label={`${TASK_HISTORY_TITLE}: ${task.id}`}
        disabled={busy}
        onClick={() => onHistoryToggle(task.id)}
        className="self-start"
      >
        <ChevronRight
          aria-hidden="true"
          className={`size-3.5 transition-transform ${historyOpen ? "rotate-90" : ""}`}
        />
        {taskHistoryTriggerText(historyOpen ? historyRuns : null, task.lastRun)}
      </Button>
      {historyOpen && (
        <TaskRunList
          taskId={task.id}
          regionId={historyRegionId}
          runs={historyRuns}
          error={historyError}
          now={now}
        />
      )}

      {/* Capped where the region is not, the Sync pane's reason: a form is read
          line by line, and a label-and-field pair stretched across a wide
          window is worse than one that sits still. */}
      {/* `editing` rather than `verbs.editing`: it is the same value — see its
          declaration — and it keeps the guard a null check rather than a member
          access the linter would rewrite into an optional chain that no longer
          narrows `verbs` for the form below. */}
      {verbs !== null && editing && (
        <Card size="sm" className="w-full max-w-[720px]">
          <CardContent>
            <TaskForm
              task={task}
              onSaved={verbs.onSaved}
              onCancel={() => verbs.onEditToggle(task.id)}
              onSavingChange={verbs.onSavingChange}
            />
          </CardContent>
        </Card>
      )}
    </div>
  );
}

/**
 * What this host paces, as its own list (Story 58.7).
 *
 * **Its own component and its own list, never a widened {@link TaskRow}.** The
 * two classes share a pane and nothing else: a task has a mode, a schedule, a
 * window, a lease and a history, and a projected row has none of those and can
 * never grow them. One component serving both would have to branch on the class
 * at every cell, and the first branch anybody forgot would put a Run now on a
 * row nothing can run — which is the one failure this section's subtitle exists
 * to promise against.
 *
 * **No interactive element inside a row, of any kind.** Not a disabled button,
 * either: a disabled control says *not now*, and the truth is *not ever*. The
 * only control this section owns is `FoldToggle`, which belongs to the list
 * rather than to any row and changes nothing but how much is on screen.
 *
 * The loading/empty/refusal shape is {@link TaskRunList}'s, deliberately rather
 * than by accident of copying — this file's header promises it reuses the Sync
 * pane's list idioms rather than inventing a third.
 */
function PacedWorkList({
  rows,
  error,
}: {
  /** `null` until this section's read lands: unread, and not empty. */
  rows: PacedWorkVm[] | null;
  error: string | null;
}) {
  // Unfolds to every row, unlike the run history above it: Rust returned the
  // whole projection rather than a page of it, so a cap on the expanded view
  // would drop rows with no control left to reveal them — in the one section
  // whose claim is that it lists everything this host paces.
  const fold = useFold(rows, { unfoldToAll: true });
  return (
    <div className="flex flex-col gap-2 border-border border-t px-6 py-4">
      {/* The project's group-label treatment (`sync-pane.tsx`'s Activity
          heading), so this section reads as a second class in one view rather
          than as a second view. */}
      <div className="min-w-0">
        <h2 className="label-caps text-faint">{PACED_HEADING}</h2>
        <p className="text-muted-foreground text-xs">{PACED_SUBTITLE}</p>
      </div>
      {/* Drawn INSTEAD of both sentences below and never instead of rows already
          on screen: a refused re-read leaves every projected row exactly where
          it was. A failed read is a fault to report, not a fact to invent — and
          it must not blank the task list above, which is why the pane reads the
          two commands through `Promise.allSettled`. */}
      {error !== null && (
        <p role="alert" data-testid={PACED_REFUSAL_TESTID} className="text-destructive text-xs">
          {error}
        </p>
      )}
      {/* Two states and two sets of words, `TaskRunList`'s rule: `null` is
          unread and `[]` is "keeper paces nothing here", and they never render
          the same sentence. Both yield to a refusal, because a kept `[]` beside
          a refusal would claim this machine paces nothing on the strength of a
          read that failed. */}
      {error === null &&
        (rows === null ? (
          <p role="status" className="text-muted-foreground text-xs">
            {PACED_LOADING_TEXT}
          </p>
        ) : (
          rows.length === 0 && (
            <p role="status" className="text-muted-foreground text-xs">
              {PACED_EMPTY_TEXT}
            </p>
          )
        ))}
      {rows !== null && rows.length > 0 && (
        <ul aria-label={PACED_HEADING} className="flex flex-col gap-3">
          {fold.visible.map((row) => (
            // `id` is the projection's own composite key — `scan:<profileId>`
            // and its two siblings — so unlike the unknown-task list below, this
            // one needs no index in its key: one folder contributes at most one
            // row of each kind.
            <li
              key={row.id}
              data-testid={PACED_ROW_TESTID}
              data-paced-id={row.id}
              className="flex flex-col gap-1"
            >
              <span className="flex items-center gap-2">
                <Badge variant="outline">
                  {PACED_STANDING_LABELS[row.standing] ?? row.standing}
                </Badge>
                <span className="font-medium text-foreground text-sm">
                  {PACED_KIND_LABELS[row.kind] ?? row.kind}
                </span>
              </span>
              <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                <Field label={PACED_FOLDER_LABEL}>{row.profile}</Field>
                {/* The cadence is drawn ONLY for a row something is actually
                    pacing, and the standing is what decides — not the presence
                    of the string.

                    `keeper_core::tasks::paced_work` enforces the same pairing,
                    but it enforces it with a `debug_assert!`, which is compiled
                    out of the build a person runs. So this is the half that
                    survives release, and it is the half that matters after
                    Story 58.8: a folder whose paced backstop has stood down to a
                    scheduled sync task must not go on advertising the interval
                    that no longer fires. A row that says *Scheduled* over *about
                    every 15 seconds* is the exact over-claim AD-141 built this
                    whole class to prevent, and the sentence beside it already
                    says which of the three reasons applies. */}
                <Field label={PACED_CADENCE_LABEL}>
                  {row.standing === "paced" && row.cadence !== null
                    ? row.cadence
                    : PACED_NO_CADENCE_TEXT}
                </Field>
              </dl>
              {/* Rust's, verbatim. Each of these carries a fact the browser
                  cannot re-derive — that a saved file brings the next look
                  forward, that a governed folder's backstop has stood down, that
                  only the running app paces a vault — so nothing here composes,
                  trims or re-words it. */}
              <p className="whitespace-pre-wrap text-muted-foreground text-sm [overflow-wrap:anywhere]">
                {row.sentence}
              </p>
            </li>
          ))}
        </ul>
      )}
      {rows !== null && rows.length > 0 && (
        <FoldToggle rows={rows} fold={fold} label={PACED_HEADING} />
      )}
    </div>
  );
}

export function TasksPane() {
  const [listing, setListing] = useState<TaskListingVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  /**
   * The projected paced class, held beside the listing rather than inside it.
   *
   * Two slots because they are two reads that can disagree: one command may
   * refuse while the other lands, and the section that refused must keep its
   * last good rows without taking the other list down with it. `null` is unread
   * and `[]` is "keeper paces nothing here" — see {@link PacedWorkList}.
   */
  const [paced, setPaced] = useState<PacedWorkVm[] | null>(null);
  const [pacedError, setPacedError] = useState<string | null>(null);
  /** Per-task refusals from a Run now or a Forget, keyed by task id. */
  const [refusals, setRefusals] = useState<Record<string, string>>({});
  /** Whether the header's add form is revealed. */
  const [adding, setAdding] = useState(false);
  /**
   * Which row has its edit form open, or `null`.
   *
   * Held here rather than per-row so exactly one can be open: an edit form is
   * eight controls tall, and three of them open at once turns a list of tasks
   * into a wall of forms with the rows they belong to scrolled apart. It is
   * cleared by a save, by Cancel and by pressing Edit again.
   */
  const [editingId, setEditingId] = useState<string | null>(null);
  /**
   * The selection the rest of the pane reads (Story 59.1, widened by 59.4).
   *
   * **One slot, widened — never a second one beside 59.1's.** It was
   * `selectedId: string | null`, on the epic's stated ground that every task
   * write in the whole stack was single-id, so a set would have been state whose
   * only possible action was a loop of N writes. `Engine::set_tasks_enabled` and
   * `Engine::forget_tasks` are that missing consumer, so the refusal is
   * overturned the way `spec-43-8…:347-348`'s was at 45.3 — and
   * `spec-45-17…:200` forbids *inventing* a second idiom, which is why every
   * gesture below is `files-pane.tsx`'s.
   *
   * **Ids and not `TaskVm`s**, 59.1's rule verbatim: `refresh()` replaces the
   * whole listing on every mount, Refresh press and Run now settle, so a stored
   * view model would go stale the moment a run moved its `lastRun`. The id is
   * the only part of a task that does not move.
   *
   * Nothing prunes it, deliberately: {@link selection} resolves it against the
   * **newest** listing, so an id the record no longer holds contributes nothing
   * rather than leaving a stale row selected. One rule in one place instead of a
   * slot and an effect that can disagree.
   */
  const [selected, setSelected] = useState<ReadonlySet<string>>(() => new Set());
  /** Where a Shift-range starts. `null` once the anchor row is gone. */
  const [anchorKey, setAnchorKey] = useState<string | null>(null);
  /**
   * A whole batch that would not run at all, as distinct from one id's refusal.
   *
   * The batched verbs reserve rejection for the store failing outright — the
   * task record would not read — and answer every per-id outcome inside the
   * receipt. So this slot holds the first kind and {@link refusals} holds the
   * second, and they are never the same sentence in two places.
   */
  const [bulkError, setBulkError] = useState<string | null>(null);
  /**
   * Whether a batched verb is in flight, so the three bulk controls cannot
   * re-issue one (Story 59.4 review, P5).
   *
   * {@link deleting}'s reason at the batch's scale, and the reason it is one flag
   * rather than a set: every id in the batch carries the `updatedMs` the *last
   * listing* gave it, so two rapid clicks send two calls with the same
   * pre-bump baselines and the second is refused `changed elsewhere` for every
   * id — by the caller's own first write. A person double-clicking Disable saw
   * five spurious "changed elsewhere" refusals, which is worse than no feedback.
   * One flag because one call is outstanding at a time by construction, which is
   * the point.
   */
  const [bulkWriting, setBulkWriting] = useState(false);
  /**
   * Live refs to each rendered row, so the keyboard handler can move focus as
   * the selection moves (`chat-list-pane.tsx:142-145`'s idiom). Rebuilt each
   * render from the current row order, which is `list_tasks`' and is never
   * re-sorted here.
   */
  const rowRefs = useRef<(HTMLDivElement | null)[]>([]);
  /**
   * What a Forget is asking about, and whether the question is on screen.
   *
   * One dialog for the whole pane (`files-pane.tsx`'s shape) rather than one per
   * row: only one question can be being answered at a time. Two slots for it
   * rather than one, though, because `AlertDialogContent` stays mounted for its
   * own hundred-millisecond exit animation: driving the title from the same
   * state that drives `open` made the last frame the person sees read "Forget
   * task ?", on the one dialog in this pane whose entire job is naming the
   * record about to go. `files-pane.tsx` degrades to nothing there; naming the
   * task all the way through the close is better than naming nothing, so the
   * subject outlives the ask and is replaced only by the next one.
   *
   * **Tagged rather than a bare list** (Story 59.4). One dialog now asks two
   * questions — *forget this task?* and *forget these three?* — and they take
   * different verbs: the row's own Forget still goes through the single-id
   * `sync_task_forget` that Story 58.1 shipped, and only the header's bulk
   * control goes through the batch. A `readonly string[]` of length one could
   * not tell the two apart, and routing a deliberate single Forget through a
   * batched write on a count would be a difference nobody asked for.
   */
  const [forgetSubject, setForgetSubject] = useState<
    { kind: "one"; id: string } | { kind: "several"; ids: readonly string[] } | null
  >(null);
  const [forgetAsking, setForgetAsking] = useState(false);
  /**
   * Ids whose Forget is in flight, so a second confirmation cannot issue a
   * second delete for a task already going. {@link running}'s shape and
   * {@link running}'s reason — Story 57.5's finding 7 — applied to the
   * destructive path, which did not inherit it.
   */
  const [deleting, setDeleting] = useState<Record<string, true>>({});
  /**
   * Whether a revealed {@link TaskForm} has a save in flight.
   *
   * Deliberately pane-wide rather than per-form: what it really says is *a write
   * to the task record is on its way*, and two things must wait for that. A
   * disclosure toggle pressed mid-save unmounts the form, so Rust's refusal has
   * nowhere to land and a collapsed disclosure with no message reads as a save
   * that happened. And a Forget confirmed mid-save deletes a row the settling
   * save then re-inserts — `upsert_task` inserts when the id is absent — so a
   * confirmed deletion silently undoes itself.
   */
  const [formSaving, setFormSaving] = useState(false);
  /**
   * Ids whose Run now is in flight — a set of them, not one slot (Story 57.5's
   * review, finding 7).
   *
   * A single `string | null` disabled exactly one button and cleared
   * unconditionally, so clicking Run now on a slow task A and then on B
   * re-enabled A while A was still running, and A's own settle then re-enabled B
   * while B was still running. A further click issued a second run for a task
   * that already held a lease, the engine answered `Busy`, and the pane painted
   * "somebody else is doing this" on a task the same person had just started
   * from this same pane.
   *
   * `Record<string, true>` rather than a `Set`, matching {@link refusals} beside
   * it: the question asked of it is only *"is this id in flight"*, and two
   * differently-shaped keyed states in one component is a difference a reader
   * has to explain.
   */
  const [running, setRunning] = useState<Record<string, true>>({});
  /**
   * The instant every relative time on screen is measured from, captured once
   * per read *and* on a coarse tick: two rows re-rendered a tick apart must not
   * disagree about what "now" is, and re-reading the clock inside each formatter
   * would make the pane's output depend on render order.
   *
   * The tick is finding 6 of this story's review. Without it `now` moved only
   * when a read landed, so a pane left open froze: a row reading "in 5 min" said
   * "in 5 min" an hour later and never reached "due now". This is a display
   * clock in the frontend and not a second scheduler — AD-62's rule is about
   * `tokio::time::interval` in the `keeper` crate, and nothing here polls the
   * engine.
   */
  const [now, setNow] = useState(() => Date.now());
  /**
   * Which read is the newest, so a slow one cannot overwrite a fast one
   * (finding 8).
   *
   * `refresh` has three independent triggers — the mount effect, the Refresh
   * button and `runNow`'s settle — and no ordering between them. Press Refresh,
   * then Run now before it resolves, and the pre-run listing could land last:
   * the row then showed "never run" immediately after a run that happened, which
   * is the exact failure the post-run re-read exists to prevent. `setNow` was
   * overwritten with the stale read's clock too, shifting every relative time
   * backwards.
   */
  const readToken = useRef(0);
  /**
   * The one open run-history section: which row it belongs to, and what has been
   * read for it.
   *
   * One section at a time, {@link editingId}'s rule and its reason: a twenty-run
   * list is taller than the eight-control form that argument was written for. A
   * cache keyed by id would also make "one open, one call" depend on which row
   * had been opened before, and would hold a run list `task_runs` may have
   * trimmed underneath it.
   *
   * **One object rather than an id and two slots beside it**, so the id and the
   * runs cannot be separately stale. Rendering with three states compared
   * against each other is correct only while every writer happens to set them in
   * one synchronous batch; the first edit that awaits between two of them draws
   * row B's section with row A's runs, which is the one outcome
   * {@link historyToken} exists to prevent and the one it cannot see.
   */
  const [history, setHistory] = useState<{
    id: string;
    /** `null` until this section's read lands: unread, and not empty. */
    runs: TaskRunVm[] | null;
    error: string | null;
  } | null>(null);
  /**
   * The same value, readable without depending on it.
   *
   * `refresh` is a `useCallback` with an empty dependency list, because three
   * independent triggers rely on its identity being stable — so giving it
   * `history` would make the mount effect re-read the whole listing every time a
   * section opened or closed. {@link openHistory} is the only writer of both, so
   * the ref cannot drift from the state.
   */
  const historyRef = useRef<typeof history>(null);
  const openHistory = useCallback((next: typeof history) => {
    historyRef.current = next;
    setHistory(next);
  }, []);
  /**
   * Which history read is the newest — {@link readToken}'s idiom, for the same
   * reason: a slow read must not land in a section that has since closed or
   * moved to another task.
   */
  const historyToken = useRef(0);

  const refresh = useCallback(
    async (keepRefusals = false) => {
      readToken.current += 1;
      const mine = readToken.current;
      // BOTH commands in ONE settled pass (Story 58.7). One pass because the
      // projection must cost no read the pane did not already make — a section
      // with a `useEffect` of its own would re-read on every mount the pane's
      // own triggers cause, which is the poll AD-62's sentence is about.
      // `allSettled` and not `all` because the two answers are independent: a
      // refused projection must not blank the task list, and a refused listing
      // must not take down rows the projection read successfully. `all` would
      // reject on the first failure and throw both away.
      const [tasksRead, pacedRead] = await Promise.allSettled([syncTasks(), syncPacedWork()]);
      if (mine !== readToken.current) {
        return;
      }
      // Landed before the listing is even looked at, and unconditionally: this
      // is the half that must survive the other half's refusal.
      if (pacedRead.status === "fulfilled") {
        setPaced(pacedRead.value);
        setPacedError(null);
      } else {
        // The rows already read STAY. They were read successfully and are still
        // the best thing known about this machine; the refusal explains why the
        // pane knows no more.
        setPacedError(messageOf(pacedRead.reason));
      }
      if (tasksRead.status === "rejected") {
        setError(messageOf(tasksRead.reason));
        return;
      }
      const next = tasksRead.value;
      setListing(next);
      setNow(Date.now());
      setError(null);
      // A disclosure cannot belong to a row the record no longer has. Left
      // standing, the id is re-creatable — the Add form takes a typed id — so
      // forgetting `nightly` and adding a new `nightly` rendered the new row
      // with its edit form already expanded and `aria-expanded` set on a
      // disclosure nobody had opened. `forgetSubject` is deliberately NOT
      // pruned here: this read fires on every Run now settle too, and closing a
      // question under the person answering it is worse than asking about a row
      // that has gone — a confirm on a row that is gone is refused, and the
      // refusal is rendered either way (see `orphanRefusals`).
      setEditingId((open) =>
        open !== null && next.tasks.some((t) => t.id === open) ? open : null,
      );
      // The same pruning for an open history section, and the same reason: a
      // section cannot belong to a row the record no longer has.
      //
      // Nothing else here touches it. A refresh must leave an open section
      // exactly as it is — open, and holding the rows it read — because
      // `refresh` fires from the mount, from the Refresh button and from every
      // Run now settle, so a history read per open row per refresh is the poll
      // AD-62's sentence is about.
      const open = historyRef.current;
      if (open !== null && !next.tasks.some((t) => t.id === open.id)) {
        historyToken.current += 1;
        openHistory(null);
      }
      // A listing read *after* an attempt is newer evidence than the attempt
      // (finding 9). `refusals` used to be cleared at exactly one point — the
      // top of `runNow`, for the one id being run — so a "the other host is
      // doing this" alert kept asserting a task was busy elsewhere while the row
      // above it showed the completed run and no holder, clearable only by
      // running the task again.
      //
      // `keepRefusals` is the one exception, and it is not a hedge: the read
      // `runNow`'s own settle issues is *contemporaneous* with the attempt, not
      // later than it. Clearing there would erase the refusal in the same tick it
      // appeared, which is the pane's whole answer to a refused Run now and an
      // acceptance criterion of this story. Every other read — the mount, the
      // Refresh button — is genuinely newer and clears.
      if (!keepRefusals) {
        setRefusals({});
      }
      // `openHistory` is itself a `useCallback([])`, so naming it here keeps this
      // callback's identity stable and the mount effect a single read.
    },
    [openHistory],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), TASKS_CLOCK_TICK_MS);
    return () => clearInterval(clock);
  }, []);

  /**
   * Adopt the persisted row counts, so this pane's fold obeys the same
   * preference the Sync pane's lists do.
   *
   * `syncListSizes()` is module state that only the Sync pane's mount and the
   * settings form ever fill in, and the app shell renders these two views
   * exclusively — so a person who goes straight to Tasks folded their run
   * history at the built-in fallback rather than at the number they chose. That
   * is exactly the "second place for one preference to be honoured differently"
   * that `list-fold.tsx` was extracted to prevent.
   *
   * The rejection is ignored on purpose, `SyncPane`'s reason: the fallbacks equal
   * the Rust defaults, and a list that folds at ten is not a failure worth a
   * banner over.
   */
  useEffect(() => {
    void hydrateSyncListSizes().catch(() => {});
  }, []);

  /**
   * Return focus to the button that revealed the add form when it closes.
   *
   * The form is closed from a control inside itself — Cancel, or the submit that
   * unmounts it on success — so without this the focused element is destroyed
   * and focus falls to `<body>`: a keyboard user is thrown out of the pane and
   * has to tab from the top of the app to get back. `recording-summary-card.tsx`
   * is the near-exact analogue (an inline edit disclosure in a card) and this is
   * its shape; `app-shell.tsx`'s `closeDetail` states the rule.
   */
  const addTriggerRef = useRef<HTMLButtonElement>(null);
  const wasAdding = useRef(false);
  useEffect(() => {
    if (!adding && wasAdding.current) {
      addTriggerRef.current?.focus();
    }
    wasAdding.current = adding;
  }, [adding]);

  /**
   * Read one task's runs, and land them only if this section is still the one
   * asking.
   *
   * No `limit` argument, deliberately: the bound is `TASK_HISTORY_LIMIT_DEFAULT`
   * in `sync_ipc.rs`, clamped there against `TASK_HISTORY_LIMIT_MAX`, and the
   * store trims each task to `TASK_RUNS_CAP` regardless — so a number invented
   * here could only disagree with Rust about a page size Rust already owns.
   *
   * `keepRows` is the re-read case. On a first open there is nothing on screen
   * to keep and the loading line is the honest state; on a re-read the rows
   * already read stay while the new read is in flight, and survive its refusal —
   * a failed read is a fault to report, not a fact to invent.
   */
  const readHistory = useCallback(
    async (id: string, keepRows: boolean) => {
      historyToken.current += 1;
      const mine = historyToken.current;
      openHistory({
        id,
        runs: keepRows ? (historyRef.current?.runs ?? null) : null,
        error: null,
      });
      try {
        const runs = await syncTaskHistory(id);
        if (mine !== historyToken.current) {
          return;
        }
        openHistory({ id, runs, error: null });
      } catch (cause) {
        if (mine !== historyToken.current) {
          return;
        }
        // The rows already read stay: they were read successfully and are still
        // the best thing known about this task.
        openHistory({
          id,
          runs: historyRef.current?.runs ?? null,
          error: messageOf(cause),
        });
      }
    },
    [openHistory],
  );

  /** Open this row's runs and read them, or close the section already open. */
  const toggleHistory = useCallback(
    (id: string) => {
      if (historyRef.current?.id === id) {
        // Closing forgets: a read still in flight for this id must not land in a
        // closed section, and re-opening should re-read rather than show a list
        // `task_runs` may have trimmed underneath it. It is also the only retry
        // a refused read has, which is why the refusal says so.
        historyToken.current += 1;
        openHistory(null);
        return;
      }
      // Opening another row closes the first by replacing the one slot, and the
      // token `readHistory` bumps is what keeps the first row's slow read out of
      // the second row's section.
      //
      // It also closes an edit form on the row being opened, because the
      // one-at-a-time argument this section borrows from `editingId` is about
      // height, and a twenty-run list plus an eight-control form is exactly the
      // wall that argument forbids. Safe to do unconditionally here: the
      // disclosure is disabled while a save is in flight, so this can never
      // unmount a form Rust's answer is still coming to.
      setEditingId(null);
      void readHistory(id, false);
    },
    [openHistory, readHistory],
  );

  const runNow = useCallback(
    async (id: string) => {
      // Whether this row's runs were on screen when the button was pressed.
      // Checked at the press and again at the settle, because a section opened
      // DURING the run already has a read of its own in flight — re-reading on
      // its behalf would spend a second call for one deliberate press and throw
      // the person's own read away mid-flight.
      const openAtPress = historyRef.current?.id === id;
      setRunning((prior) => ({ ...prior, [id]: true }));
      // Cleared before the attempt, so a stale refusal cannot sit under a run
      // that has just succeeded.
      setRefusals((prior) => {
        const { [id]: _dropped, ...rest } = prior;
        return rest;
      });
      try {
        await syncTaskRunNow(id);
      } catch (cause) {
        // Quoted on the row and nowhere else. An engine that refuses — the task
        // is off, or the other host on this machine holds the lease — is not a
        // failure of this surface, and the sentence it gives is the actionable
        // half.
        setRefusals((prior) => ({ ...prior, [id]: messageOf(cause) }));
      } finally {
        // Only the id that settled, never the whole set: another row's run may
        // still be in flight.
        setRunning((prior) => {
          const { [id]: _settled, ...rest } = prior;
          return rest;
        });
        // Re-read either way: a refused run still changes nothing, and a run
        // that happened changed the history, the window and possibly the lease.
        // The refusal this attempt may just have recorded survives it — see
        // `refresh`.
        await refresh(true);
        // The one re-read a listing refresh does not do, and it is not a poll:
        // this run is precisely what changed that task's history, and the person
        // asked for it by pressing the button. Only the open section, and only
        // when it is this row's.
        if (openAtPress && historyRef.current?.id === id) {
          await readHistory(id, true);
        }
      }
    },
    [refresh, readHistory],
  );

  /**
   * Delete the task the confirmation named, and re-read.
   *
   * A refusal goes where a refused Run now's goes — the row's own alert, keyed
   * by id — because it is the same kind of answer: the engine would not do this,
   * and its sentence is the actionable half. The dialog closes either way: it has
   * asked its question and been answered, and leaving it open over a refusal
   * would hide the row the refusal is written on.
   */
  const forget = useCallback(
    async (id: string) => {
      setForgetAsking(false);
      setDeleting((prior) => ({ ...prior, [id]: true }));
      setRefusals((prior) => {
        const { [id]: _dropped, ...rest } = prior;
        return rest;
      });
      try {
        await syncTaskForget(id);
        // Only a row that is gone can have no form open on it.
        setEditingId((open) => (open === id ? null : open));
      } catch (cause) {
        setRefusals((prior) => ({ ...prior, [id]: messageOf(cause) }));
      } finally {
        setDeleting((prior) => {
          const { [id]: _settled, ...rest } = prior;
          return rest;
        });
        await refresh(true);
      }
    },
    [refresh],
  );

  const tasks = listing?.tasks ?? [];

  /**
   * The selected ids the **newest** listing still holds, in listing order.
   *
   * `files-pane.tsx:1708-1714`'s memo and its rule: a row that vanished on a
   * refresh is not silently still selected, and the order is the list's own
   * rather than the order the person clicked in — a bulk write should act in the
   * order the receipt will be read back in.
   */
  const selection = useMemo(() => tasks.filter((row) => selected.has(row.id)), [tasks, selected]);

  /**
   * Which task the detail region draws, resolved rather than stored (Story
   * 59.1, kept exactly by 59.4).
   *
   * Three states, one expression, and it is what keeps single-select
   * byte-for-byte what Story 59.1 shipped:
   *
   * - **Exactly one selected** — that task. Every plain click, every ↑/↓/Home/End
   *   and every Enter resolves here, because each of them is a `replace`.
   * - **Nothing selected** — the first row. A detail region that started empty
   *   over a list with rows would be a second empty state competing with the
   *   real one, and defaulting costs **no read**: every field it draws is
   *   already on the `TaskVm` the listing carries. The same fallback absorbs a
   *   chosen task the other host on this shared record forgot, and leaves
   *   `null` — no region at all — when the listing empties.
   * - **Two or more selected** — `null`. The region does not draw a task,
   *   because there is no one task to draw; it draws the selection's own count
   *   instead (see the render below). That is also what makes the bulk refusals
   *   render at pane level rather than one of five landing inside a region about
   *   somebody else.
   */
  const selectedTask =
    selection.length === 1 ? selection[0] : selection.length === 0 ? (tasks[0] ?? null) : null;
  const resolvedId = selectedTask?.id ?? null;

  /**
   * Which row holds the roving tab stop.
   *
   * The anchor while it is a row this listing has, because that is the row a
   * Shift-range measures from and the row the arrows step off. Under a single
   * selection the anchor and the resolved task are the same row — a plain click
   * and an arrow both set it — so this is a no-op for everything Story 59.1
   * shipped; under a set it is the one row that a keyboard user is meaningfully
   * "at". Falls back to the resolved task, which covers the mount, when nothing
   * has been clicked yet.
   *
   * ...and falls back **further**, because `resolvedId` is `null` under a set of
   * two or more: to the first selected row, then to the first row of the
   * listing. Neither is a nicety. With the anchor row forgotten by another host
   * mid-selection, `resolvedId` is already `null`, so stopping there left **no**
   * row with `tabIndex 0` and the listbox unreachable by Tab. And with nothing
   * selected — where nothing is `aria-selected` either — the cursor is the only
   * thing giving the list a tab stop at all. There is always exactly one while
   * {@link tasks} is non-empty.
   */
  const cursorId =
    anchorKey !== null && tasks.some((row) => row.id === anchorKey)
      ? anchorKey
      : (resolvedId ?? selection[0]?.id ?? tasks[0]?.id ?? null);

  /**
   * Replace, extend or toggle the selection from one row (Story 59.4).
   *
   * **Ported from `files-pane.tsx:1656-1706`**, branch for branch, because
   * `spec-45-17…:200` forbids a second selection idiom by name. The three
   * gestures a list has, and the modifier decides which: plain replaces,
   * Cmd/Ctrl toggles one, Shift takes the run from the anchor. The run is over
   * {@link tasks} — the flat visible order — so what a Shift takes is exactly
   * what a person sees between the two rows.
   *
   * Files' third precedence condition, `crossesProfile`, has **no Tasks
   * analogue**: a task is not scoped to a folder for these two verbs — one batch
   * may hold ids from any number of folders and from none — so there is no
   * cross-scope reset to copy. The rule that condition existed to keep, *half a
   * selection no command can act on is worse than one that visibly reset*, is
   * kept a stricter way here: `TaskListing.unknown` rows and 58.7's projected
   * paced rows are not selectable at all, so a selection cannot contain
   * something the batch could only refuse.
   */
  const select = useCallback(
    (id: string, mode: "replace" | "toggle" | "extend") => {
      setSelected((previous) => {
        if (mode === "replace" || previous.size === 0) {
          return new Set([id]);
        }
        if (mode === "toggle") {
          // One pass, and it adds exactly one: it never fills the gap between
          // this row and the anchor, which is Shift's job and only Shift's.
          const next = new Set(previous);
          if (!next.delete(id)) {
            next.add(id);
          }
          return next;
        }
        const from = tasks.findIndex((row) => row.id === anchorKey);
        const to = tasks.findIndex((row) => row.id === id);
        if (from < 0 || to < 0) {
          return new Set([id]);
        }
        // Inclusive at both ends, and it REPLACES rather than unions: a run is
        // what a person sees between two rows, and re-measuring from an unmoved
        // anchor is what makes consecutive Shift-clicks grow and shrink one run.
        const [low, high] = from <= to ? [from, to] : [to, from];
        return new Set(tasks.slice(low, high + 1).map((row) => row.id));
      });
      // Shift keeps the anchor it is measuring from; the other two move it.
      if (mode !== "extend") {
        setAnchorKey(id);
      }
      // A stale refusal from a previous attempt is not about this selection.
      setBulkError(null);
    },
    [anchorKey, tasks],
  );

  /**
   * Close what belonged to the last task, the moment the selection resolves to
   * one (Story 59.1's side effects, under 59.4's set).
   *
   * Story 59.1's `selectTask` closed the previous task's edit form and bumped
   * `historyToken` to drop its runs, on the rule that a form seeded from task A
   * must not survive into task B's region and a run list read for A must not sit
   * under B's name. Under a set that becomes a question with one honest answer:
   * those effects belong to **the region**, so they fire exactly when the region
   * changes which task it is drawing — every plain click, every arrow key, and
   * the moment a multi-selection collapses back to one.
   *
   * An additive Cmd-click that grows the set past one deliberately does NOT fire
   * them: `resolvedId` goes `null`, the region unmounts, and nothing is left
   * seeded from a task that is no longer on screen. Closing a form at that
   * moment would be work nobody can see; re-opening on the collapse is what the
   * person actually experiences, and that is the branch below.
   *
   * Keyed on the resolved **id** and not the `TaskVm`, so a refresh that hands
   * the region a fresher object for the same task is not a change of subject —
   * which is the whole reason Story 59.1 stored an id in the first place.
   */
  useEffect(() => {
    if (resolvedId === null) {
      return;
    }
    setEditingId(null);
    if (historyRef.current !== null) {
      historyToken.current += 1;
      openHistory(null);
    }
  }, [resolvedId, openHistory]);

  /**
   * Move the selection with the keyboard, and take focus with it.
   *
   * A **replace**, so an arrow always resolves the region to exactly one task —
   * `chat-list-pane.tsx` keeps a second `focusedKey` beside its selection
   * because opening a conversation is expensive and arrowing past one must not
   * open it; choosing a task costs nothing at all, so a second cursor would be
   * state with no consumer and two highlights a person has to tell apart.
   *
   * Clamped at both ends rather than wrapping: a list of twenty whose Down at
   * the bottom silently returns to the top is a list that has lost the reader's
   * place.
   */
  const moveSelection = useCallback(
    (to: number) => {
      const next = tasks[to];
      if (next === undefined) {
        return;
      }
      select(next.id, "replace");
      rowRefs.current[to]?.focus();
    },
    [tasks, select],
  );

  const onListKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    if (tasks.length === 0) {
      return;
    }
    // Space is the selection key on a multi-select list
    // (`files-pane.tsx:2061-2074`), and it is decoded BEFORE the chord guard
    // below because Cmd/Ctrl-Space is one of the two spellings of *toggle this
    // one* — a key this list owns with its modifier attached, unlike ⌘↓.
    if (event.key === " ") {
      // Which row, read off the event rather than taken from `cursorId`: after
      // a Shift-click the browser has focus on the clicked row while the anchor
      // — and so the cursor — is deliberately elsewhere. `data-task-id` is on
      // the `<li>` every option sits inside, the same closest-ancestor trick
      // `files-pane.tsx:2148` uses to tell a row's own click from a control's.
      const row = event.target instanceof Element ? event.target.closest("[data-task-id]") : null;
      const id = (row instanceof HTMLElement ? row.dataset.taskId : null) ?? cursorId;
      if (id === undefined || id === null) {
        return;
      }
      event.preventDefault();
      select(id, event.metaKey || event.ctrlKey ? "toggle" : "replace");
      return;
    }
    // Chords belong to the global shortcut hooks, `chat-list-pane.tsx`'s rule:
    // a pane that swallows ⌘↓ breaks a shortcut it knows nothing about.
    if (event.metaKey || event.altKey || event.ctrlKey) {
      return;
    }
    // Guarded on a non-empty selection, `files-pane.tsx:2088-2094`'s rule: an
    // Escape with nothing selected is not this list's to swallow — a dialog or
    // a global handler above it may well want it.
    if (event.key === "Escape") {
      if (selected.size > 0) {
        event.preventDefault();
        setSelected(new Set());
        setAnchorKey(null);
      }
      return;
    }
    const at = cursorId === null ? -1 : tasks.findIndex((row) => row.id === cursorId);
    const to =
      event.key === "ArrowDown"
        ? Math.min(at + 1, tasks.length - 1)
        : event.key === "ArrowUp"
          ? Math.max(at - 1, 0)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? tasks.length - 1
              : null;
    if (to === null) {
      return;
    }
    // Only once a key this list owns has been recognised, so Tab, Enter and
    // every shortcut above still reach whatever else wants them.
    event.preventDefault();
    moveSelection(to);
  };

  /**
   * What a row's click is allowed to do to the panel beside the list (Story
   * 59.12), `files-pane.tsx:2102-2131`'s rule, branch for branch.
   *
   * A modified click belongs to the selection model and never to the panel:
   * somebody assembling a five-task selection to Forget does not want five
   * panels, and the last Shift-click of a range is not the task they were
   * looking at. A click that landed on a control is that control's — no row
   * carries one today, and Story 59.1's invariant says none ever will, but this
   * is the line that keeps the invariant's next reader honest.
   *
   * The target is composed here and nowhere else, so the preview and the
   * open-beside below cannot come to disagree about which task a gesture named.
   */
  const clickTarget = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, id: string): PanelTargetVm | null => {
      if (event.metaKey || event.ctrlKey || event.shiftKey) {
        return null;
      }
      if (event.target instanceof Element && event.target.closest("button") !== null) {
        return null;
      }
      return { kind: "task", taskId: id };
    },
    [],
  );

  /**
   * What a row's own click means (Story 59.4), `files-pane.tsx:2146-2163`'s
   * decoding, with 59.12's panel half beside it.
   *
   * `metaKey || ctrlKey` is checked **before** `shiftKey`, and the two are one
   * branch rather than two: on any given machine one of them is the wrong
   * platform, and a browser that honoured only Cmd would leave every Linux user
   * without a toggle.
   *
   * Two things at once because they are one gesture, and the split between them
   * is {@link clickTarget}: the selection branch runs for every click including
   * the modified ones, and the panel branch runs only for a plain click that
   * did not land on a control. Previewing rather than opening, so a person
   * stepping down a list of twenty is left with one panel and not twenty.
   */
  const handleRowClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, id: string) => {
      if (!(event.target instanceof Element && event.target.closest("button") !== null)) {
        let mode: "replace" | "toggle" | "extend" = "replace";
        if (event.metaKey || event.ctrlKey) {
          mode = "toggle";
        } else if (event.shiftKey) {
          mode = "extend";
        }
        select(id, mode);
      }
      const target = clickTarget(event, id);
      if (target !== null) {
        panelsStore.getState().setActiveTarget(target);
      }
    },
    [clickTarget, select],
  );

  /** Double click: open this task BESIDE what is already open. The single click
   *  that necessarily preceded it is undone by the store, so the task that was
   *  showing comes back rather than being replaced by a second copy of this one. */
  const handleRowDoubleClick = useCallback(
    (event: ReactMouseEvent<HTMLDivElement>, id: string) => {
      const target = clickTarget(event, id);
      if (target !== null) {
        panelsStore.getState().openPanel(target);
      }
    },
    [clickTarget],
  );

  /**
   * Land a batch's receipt on the rows it is about (Story 59.4).
   *
   * **Per id, and the selection is never silently shrunk.** Every `refused`
   * entry goes into the existing {@link refusals} map under its own id, carrying
   * keeper's sentence verbatim, and every `missing` entry goes in too with the
   * pane's own {@link TASKS_BULK_MISSING_TEXT} — the wire's `reason` is non-null
   * only for `refused`, by `TaskBatchEntryVm`'s documented invariant, so a
   * `missing` id has no Rust sentence to render and something still has to be
   * said. A success clears whatever that id said last: a listing read after an
   * attempt is newer evidence than the attempt, which is `refresh`'s own rule.
   *
   * One map and not a second surface: because the region draws no task while
   * more than one row is selected, `orphanRefusals` below already renders every
   * one of these as `{id}: {reason}` at pane level.
   */
  const applyReceipt = useCallback((receipt: TaskBatchReceiptVm) => {
    setRefusals((prior) => {
      const next = { ...prior };
      for (const entry of receipt.entries) {
        switch (entry.outcome) {
          case "refused":
            next[entry.id] = entry.reason ?? TASKS_BULK_NO_REASON_TEXT;
            break;
          case "missing":
            next[entry.id] = TASKS_BULK_MISSING_TEXT;
            break;
          default:
            // `saved` and `forgotten`: the write happened, so anything this id
            // said about a previous attempt is stale.
            delete next[entry.id];
            break;
        }
      }
      return next;
    });
  }, []);

  /**
   * Take the whole selection in or out of service in one call.
   *
   * Each id carries **its own** `updatedMs` as the baseline, which is the
   * decision worth stating: `sync_task_save`'s baseline exists because the edit
   * form seeded its values once and every field it writes is as old as that
   * seeding — and a bulk action from a rendered list is that case, not the
   * read-and-write-in-one-call case the CLI is. Sending ids alone would make the
   * bulk path silently weaker than the single-id path it stands in for.
   *
   * The selection is **kept**: the rows are still there and still the same set,
   * so clearing it would be state loss with no reason. Contrast the Forget below.
   */
  const setSelectionEnabled = useCallback(
    async (enabled: boolean) => {
      const ids = selection.map((row) => ({ id: row.id, baselineUpdatedMs: row.updatedMs }));
      setBulkError(null);
      setBulkWriting(true);
      try {
        applyReceipt(await syncTasksSetEnabled(ids, enabled));
      } catch (cause) {
        // The one whole-batch failure: the task record would not read at all.
        setBulkError(messageOf(cause));
      } finally {
        // `true`, for `runNow`'s reason: this read is contemporaneous with the
        // attempt rather than newer than it, so clearing the receipt's refusals
        // here would erase them in the tick they appeared.
        await refresh(true);
        // Cleared only after the re-read, so the baselines the next click sends
        // are the ones this write bumped rather than the ones it consumed.
        setBulkWriting(false);
      }
    },
    [applyReceipt, refresh, selection],
  );

  /**
   * Forget every id the confirmation named, and empty the selection.
   *
   * **Cleared, unlike Enable/Disable, and the difference is deliberate**
   * (`files-pane.tsx:1876-1877`'s rule): those rows are gone, so a set still
   * holding them would be a selection of nothing that the header still counted.
   * The anchor goes with them — a Shift-range cannot measure from a row the
   * record no longer has.
   */
  const forgetSelection = useCallback(
    async (ids: readonly string[]) => {
      setForgetAsking(false);
      setBulkError(null);
      setBulkWriting(true);
      try {
        applyReceipt(await syncTasksForget([...ids]));
        setSelected(new Set());
        setAnchorKey(null);
      } catch (cause) {
        setBulkError(messageOf(cause));
      } finally {
        await refresh(true);
        setBulkWriting(false);
      }
    },
    [applyReceipt, refresh],
  );

  /**
   * Refusals with nowhere on screen to be drawn.
   *
   * `refusals` is keyed by task id and drawn by {@link TaskDetail}, which draws
   * exactly **one** task — so a refusal is homeless in two ways now, not one.
   * The row may be gone: the likeliest reason `sync_task_forget` refuses is that
   * another writer on this shared record removed it first, at which point the
   * re-read in `forget`'s own `finally` takes away the region that would have
   * carried the sentence. Or the row may simply no longer be selected, which
   * Story 59.1 made reachable — a Run now is answered asynchronously and the
   * person is free to choose another task while it is in flight.
   *
   * Either way a failed action would look exactly like a successful one, which
   * is the invisible-failure shape this whole epic exists to close. So the test
   * is *is this refusal being rendered somewhere* and not *does this row still
   * exist*: anything the detail region is not showing is promoted to the pane's
   * own alert instead of dropped.
   */
  const orphanRefusals = Object.entries(refusals).filter(([id]) => id !== selectedTask?.id);

  /**
   * The names are a surface column: it folds away and it can be dragged wider
   * (Story 59.1, the convention `0a24b39` settled rather than left to this
   * story).
   *
   * Its rail is one entry, and the reasoning is in {@link TASKS_RAIL_LIST_LABEL}:
   * this pane's header sits above BOTH columns, so folding takes away neither
   * Refresh nor Add — it takes away the names. The strip therefore says how many
   * there are and gives them back, which is what `files-pane.tsx` does with a
   * selection its strip cannot show.
   *
   * The count comes from the listing and not from the rendered rows, which is
   * `count-label.ts`'s whole enforcement — and `null` counts as none, because
   * before the first read there is nothing behind the strip yet.
   */
  const list = useSurfaceColumn("tasks-list", {
    rail: [
      {
        id: "tasks",
        icon: ListChecks,
        label: TASKS_RAIL_LIST_LABEL,
        detail: countLabel(tasks.length, TASKS),
        count: tasks.length,
        onSelect: () => columnFoldStore.getState().toggleColumn("tasks-list"),
      },
    ],
  });

  return (
    <section
      aria-label={TASKS_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading text-title">{TASKS_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{TASKS_PANE_SUBTITLE}</p>
          {/* Rendered only when there is a row it could apply to: a sentence
              about a button nobody can see yet is noise, and the empty state
              already carries its own three-part explanation. */}
          {listing !== null && listing.tasks.length > 0 && (
            <p className="text-muted-foreground text-xs">{TASKS_RUN_NOW_SENTENCE}</p>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {/* The bulk verbs, and they exist only when there is a selection to
              act on (Story 59.4). Absent rather than disabled, which is
              `files-pane.tsx:2989`'s gate: a disabled control says *not now*,
              and with nothing selected the truth is *there is nothing to do
              this to*.

              Sited in the header because the header sits above BOTH columns and
              does not fold — and because 45.3's rule is that *a per-row Delete
              button cannot answer "and the other four"*. That is also what keeps
              Story 59.1's no-control-on-the-row invariant intact. */}
          {selection.length > 0 && (
            <>
              {/* A COUNT, not a sentence beside buttons — the app's own chip,
                  `files-pane.tsx:3003`'s treatment: the figure is what is drawn
                  and the words are what is announced. `role="status"` earns its
                  live region here, because the number changes under the
                  reader's own clicks; it is also what makes the name reachable
                  at all, since an `aria-label` on a role-less `span` reaches a
                  screen reader not at all. */}
              <Badge
                variant="secondary"
                role="status"
                data-testid={TASKS_SELECTED_TESTID}
                aria-label={tasksSelectionSentence(selection.length)}
                title={tasksSelectionSentence(selection.length)}
                className="figures"
              >
                {selection.length}
              </Badge>
              {/* All three disabled while a batched call is outstanding — see
                  `bulkWriting`. Two rapid clicks would otherwise carry the same
                  pre-bump baselines and the second would be refused `changed
                  elsewhere` for every id, by the first's own write. */}
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={bulkWriting}
                onClick={() => void setSelectionEnabled(true)}
              >
                {TASKS_BULK_ENABLE_TEXT}
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={bulkWriting}
                onClick={() => void setSelectionEnabled(false)}
              >
                {TASKS_BULK_DISABLE_TEXT}
              </Button>
              {/* Asked before anything goes, exactly as the row's own Forget is:
                  a destructive verb over a set with no confirmation is worse
                  than the single-id one it stands in for, not better. The
                  question names the COUNT — see `tasksForgetConfirmTitle`. */}
              <Button
                type="button"
                variant="destructive"
                size="sm"
                disabled={bulkWriting}
                onClick={() => {
                  setForgetSubject({ kind: "several", ids: selection.map((row) => row.id) });
                  setForgetAsking(true);
                }}
              >
                {TASKS_BULK_FORGET_TEXT}
              </Button>
            </>
          )}
          {/* The action and the thing it reveals are worded identically
              (`add-folder-form.tsx`'s rule): a button called something else
              would be a second name for one form. Disabled while a save is in
              flight, because pressing it then unmounts the form Rust's answer
              has to land in — see `formSaving`. */}
          <Button
            ref={addTriggerRef}
            type="button"
            variant="outline"
            size="sm"
            aria-expanded={adding}
            disabled={formSaving}
            onClick={() => setAdding((open) => !open)}
          >
            {TASK_FORM_ADD_TITLE}
          </Button>
          <Button type="button" variant="outline" size="sm" onClick={() => void refresh()}>
            {TASK_REFRESH_TEXT}
          </Button>
        </div>
      </header>

      {/* Pane-level facts, above both levels and spanning them: a listing that
          would not read and a refusal with nowhere to be drawn are about the
          surface rather than about one task, and putting them inside either
          column would hide them behind that column's fold. */}
      {error !== null && (
        <p
          role="alert"
          data-testid={TASKS_ERROR_TESTID}
          className="shrink-0 px-6 pt-4 text-destructive text-sm"
        >
          {error}
        </p>
      )}
      {/* A whole batch the store would not run, which is a different fact from
          any one id's refusal and so has its own slot — see `bulkError`. Sited
          here rather than in the header, because a header that grows a sentence
          is a header that reflows the two columns under it. */}
      {bulkError !== null && (
        <p
          role="alert"
          data-testid={TASKS_BULK_ERROR_TESTID}
          className="shrink-0 px-6 pt-4 text-destructive text-sm"
        >
          {bulkError}
        </p>
      )}
      {/* A refusal the detail region is not showing — see `orphanRefusals`.
          Named by its task, because the region that would have said which one
          is either gone or showing somebody else. */}
      {orphanRefusals.map(([id, refusal]) => (
        <p
          key={id}
          role="alert"
          data-testid={TASKS_ORPHAN_REFUSAL_TESTID}
          className="shrink-0 px-6 pt-4 text-destructive text-sm"
        >
          {id}: {refusal}
        </p>
      ))}

      <div className="flex min-h-0 min-w-0 flex-1">
        {/* Level 1 — the names. */}
        <section {...list.rootProps} className={TASKS_COLUMN_CLASS}>
          {list.chrome}
          {!list.folded && (
            <ScrollArea fitWidth className="min-h-0 flex-1">
              <div data-slot="tasks-body" className="flex flex-col">
                {listing === null && error === null && (
                  <p className="px-3 pt-4 text-muted-foreground text-sm">
                    {TASKS_PANE_LOADING_SENTENCE}
                  </p>
                )}
                {tasks.length > 0 && (
                  // A listbox, and `aria-multiselectable` on it, because since
                  // Story 59.4 several rows really can be held at once — the
                  // container attribute and the row's `aria-selected` flip
                  // together, for the reason `TaskRow`'s doc gives.
                  //
                  // A `<div>` and not a `<ul>`: a listbox owns its options
                  // directly, and `role="listbox"` on a list element is a
                  // non-interactive element handed an interactive role, which
                  // this project's lint refuses by name. `files-pane.tsx:3096`
                  // is the same shape for the same reason.
                  //
                  // Keyboard navigation is over the whole list rather than a
                  // handler per row: the arrows, Space and Escape are facts
                  // about the list, and only Enter belongs to the row it
                  // activates.
                  <div
                    aria-label={TASKS_LIST_LABEL}
                    role="listbox"
                    aria-multiselectable="true"
                    className="flex flex-col"
                    onKeyDown={onListKeyDown}
                  >
                    {tasks.map((task, index) => (
                      <TaskRow
                        key={task.id}
                        task={task}
                        now={now}
                        // **Membership in the set, and nothing else.** The
                        // detail region's empty-selection fallback (`tasks[0]`,
                        // see `selectedTask`) is about which task is *drawn*
                        // when nobody has chosen yet — it is not a selection.
                        // Folding it in here announced "01FIRST, selected" on
                        // mount inside an `aria-multiselectable` listbox while
                        // `selection.length === 0` and no bulk verb was offered.
                        // `files-pane.tsx` has no such fallback and nothing is
                        // `aria-selected` on its mount either. The list still
                        // has a tab stop with nothing selected, because that is
                        // the cursor's job — see `cursorId`.
                        selected={selected.has(task.id)}
                        tabIndex={cursorId === task.id ? 0 : -1}
                        optionRef={(element) => {
                          rowRefs.current[index] = element;
                        }}
                        onRowClick={handleRowClick}
                        onRowDoubleClick={handleRowDoubleClick}
                        // Enter, and a plain replace: re-choosing the task
                        // already open costs nothing now, because the effect
                        // that closes a form keys off the resolved id rather
                        // than off the gesture.
                        onActivate={(id) => select(id, "replace")}
                      />
                    ))}
                  </div>
                )}
                {/* These rows carry no controls and are NOT selectable, now that
                    the readable ones open a detail region. They are not
                    `TaskVm`s — `db::list_tasks` could not decode them — so there
                    is nothing to draw a detail from and nothing to seed a form
                    with, and an upsert built out of a reason string is one
                    `sync_task_save` would refuse. A row that selects into an
                    empty region is the same defect as a control that can only
                    fail. */}
                {listing !== null && listing.unknown.length > 0 && (
                  <>
                    <h2 className="border-border border-t px-3 pt-4 font-heading text-muted-foreground text-sm">
                      {TASKS_UNKNOWN_HEADING}
                    </h2>
                    <ul className="flex flex-col">
                      {listing.unknown.map((row, index) => (
                        <li
                          // The index, because the ID is the thing that is not
                          // unique here (Story 57.5's finding 10):
                          // `db::list_tasks` emits
                          // `UnknownTask { id: String::new(), … }` for a row
                          // whose `id` column will not read, and two of those
                          // gave React two siblings keyed `""` — a duplicate-key
                          // warning, and reconciliation free to reuse one row's
                          // DOM for the other so the two distinct reasons swap
                          // or fail to update. This is the one list that exists
                          // to tolerate malformed rows, and it is `ORDER BY id`
                          // from the store rather than reorderable by the user,
                          // so the index is stable across reads.
                          // biome-ignore lint/suspicious/noArrayIndexKey: see above — the id is not unique
                          key={`${index}:${row.id}`}
                          data-testid={TASKS_UNKNOWN_ROW_TESTID}
                          data-task-id={row.id}
                          className="flex flex-col gap-1 px-3 py-3"
                        >
                          <span className="flex items-center gap-2">
                            <Badge variant="outline">{TASKS_UNKNOWN_BADGE}</Badge>
                            <span className="truncate font-medium text-foreground text-sm">
                              {row.id === "" ? TASKS_UNKNOWN_NO_ID_TEXT : row.id}
                            </span>
                          </span>
                          <span className="text-muted-foreground text-sm">{row.reason}</span>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
                {/* Last in the column and inside the same `ScrollArea`, because
                    it is a second class in ONE view and not a second view: the
                    question it answers — "is that everything keeper does on a
                    clock?" — is only asked once the task list above has been
                    read. This column is where the lists live, and a projected
                    row has no detail to open, so it belongs here rather than
                    beside a task. Rendered unconditionally, so its own loading
                    line and its own empty sentence are reachable rather than
                    hidden behind the tasks' states. */}
                <PacedWorkList rows={paced} error={pacedError} />
              </div>
            </ScrollArea>
          )}
        </section>
        {list.seam}

        {/* Level 2 — one task at a time, and still the pane's own region rather
            than the strip beside it (Story 59.12). The two are not rivals: this
            region follows the selection, a panel holds the task somebody asked
            to keep, and the only gesture that makes them differ is the double
            click that asked for exactly that. Deleting this region in favour of
            the strip would take Story 59.1's level 2 away from every reader who
            never double-clicks anything. */}
        <section
          aria-label={TASKS_DETAIL_LABEL}
          className="flex min-w-0 flex-1 flex-col bg-background"
        >
          <ScrollArea fitWidth className="min-h-0 flex-1">
            {/* The add form, revealed by the header or by the folded rail and
                drawn HERE — inline, never a dialog (AD-C7): the two
                configuration surfaces are the same component, so they cannot
                word or validate a task differently. It takes the region rather
                than sitting above the selected task, because a form and a task's
                detail are two different answers to "what am I looking at", and
                720px of form does not fit in a 320px column. Closing unmounts
                it, so the next open starts from a fresh form rather than an
                abandoned draft — and `task-form.tsx`'s own reads key off mount. */}
            {adding ? (
              <Card size="sm" className="m-6 w-full max-w-[720px]">
                <CardContent>
                  <TaskForm
                    onSaved={(saved) => {
                      setAdding(false);
                      // Show what was just made rather than leaving the region
                      // on whatever was selected before it existed. The id is
                      // Rust's — a create sends `id: ""` and gets a minted ULID
                      // back — so this is the only moment the pane learns it.
                      // A replace and not an addition: a new task is what the
                      // person is now looking at, not a fifth member of a set
                      // they assembled before it existed.
                      setSelected(new Set([saved.id]));
                      setAnchorKey(saved.id);
                      void refresh();
                    }}
                    onCancel={() => setAdding(false)}
                    onSavingChange={setFormSaving}
                  />
                </CardContent>
              </Card>
            ) : listing !== null && tasks.length === 0 && listing.unknown.length === 0 ? (
              // The empty state is drawn in the wide region rather than in the
              // 320px column: it is three paragraphs and a shell command, and a
              // command wrapped over four lines is a command nobody can copy.
              <div className="flex flex-col gap-2 px-6 pt-4">
                <p className="text-muted-foreground text-sm">{TASKS_PANE_EMPTY_SENTENCE}</p>
                <code className="w-fit max-w-full overflow-x-auto rounded bg-muted px-2 py-1 font-mono text-foreground text-xs">
                  {TASKS_PANE_EMPTY_COMMAND}
                </code>
                <p className="text-muted-foreground text-sm">{TASKS_PANE_EMPTY_AFTER}</p>
              </div>
            ) : selection.length > 1 ? (
              // More than one row is held, so there is no one task to draw and
              // this region says what there is instead — the count, and nothing
              // that pretends to be a task's detail. The verbs that act on it
              // are in the header beside the same number (45.3's rule), so this
              // is a statement rather than a second control surface.
              //
              // A plain paragraph and NOT a `role="status"`: the header's badge
              // already announces this exact sentence as its accessible name,
              // and a second live region with the same name is one a reader
              // cannot tell from the first.
              <p
                data-testid={TASKS_SELECTION_TESTID}
                className="px-6 pt-4 text-muted-foreground text-sm"
              >
                {tasksSelectionSentence(selection.length)}
              </p>
            ) : (
              selectedTask !== null && (
                <TaskDetail
                  // Keyed by id so choosing another task remounts the region
                  // rather than reconciling one task's DOM into another's: the
                  // edit-form focus-return effect and `useId` both belong to the
                  // task they were mounted for.
                  key={selectedTask.id}
                  task={selectedTask}
                  now={now}
                  refusal={refusals[selectedTask.id] ?? null}
                  // The pane is the writing host, so it passes all nine — see
                  // {@link TaskDetailVerbs} for why the panel beside it passes
                  // `null` instead of nine inert copies of them.
                  verbs={{
                    running: running[selectedTask.id] === true,
                    deleting: deleting[selectedTask.id] === true,
                    editing: editingId === selectedTask.id,
                    writing: formSaving,
                    onRunNow: (id) => void runNow(id),
                    // Opening an edit form closes this task's runs, the mirror of
                    // `toggleHistory` closing the form: one of the two, never both.
                    onEditToggle: (id) => {
                      setEditingId((open) => (open === id ? null : id));
                      if (historyRef.current?.id === id) {
                        historyToken.current += 1;
                        openHistory(null);
                      }
                    },
                    onSaved: () => {
                      setEditingId(null);
                      void refresh();
                    },
                    onSavingChange: setFormSaving,
                    onForget: (id) => {
                      setForgetSubject({ kind: "one", id });
                      setForgetAsking(true);
                    },
                  }}
                  // All three read the ONE slot, so the region can never be
                  // handed another task's runs: the id and the runs it belongs
                  // to move together or not at all.
                  historyOpen={history?.id === selectedTask.id}
                  historyRuns={history?.id === selectedTask.id ? history.runs : null}
                  historyError={history?.id === selectedTask.id ? history.error : null}
                  onHistoryToggle={toggleHistory}
                />
              )
            )}
          </ScrollArea>
        </section>
      </div>

      {/* Asked before anything is deleted, and the question says what the answer
          costs. Every word of it is the backend's own framing: this deletes a
          record, never content. */}
      <AlertDialog open={forgetAsking} onOpenChange={(open) => !open && setForgetAsking(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            {/* Named from `forgetSubject` and never from the slot that drives
                `open`, so the question still names its subject through the
                close. One task by its id, several by their COUNT — a set has no
                one name, and the number is what the person is deciding about. */}
            <AlertDialogTitle>
              {forgetSubject !== null &&
                (forgetSubject.kind === "one"
                  ? taskForgetConfirmTitle(forgetSubject.id)
                  : tasksForgetConfirmTitle(forgetSubject.ids.length))}
            </AlertDialogTitle>
            {/* One body for both, because the fact it states — this deletes a
                record, never content — is the same fact for one task and for
                five, and it is the whole reason the question is asked. */}
            <AlertDialogDescription data-testid={TASK_FORGET_TESTID}>
              {TASK_FORGET_CONFIRM_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{TASK_FORGET_CANCEL_TEXT}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                if (forgetSubject === null) {
                  return;
                }
                // The row's own Forget keeps going through the single-id verb it
                // has used since Story 58.1; only the header's bulk control
                // takes the batch. Same question, two doors, and the tag on the
                // subject is what keeps them apart.
                if (forgetSubject.kind === "one") {
                  void forget(forgetSubject.id);
                } else {
                  void forgetSelection(forgetSubject.ids);
                }
              }}
            >
              {TASK_FORGET_TEXT}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
