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
 */
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { FoldToggle, useFold } from "@/components/layout/list-fold";
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
import type { PacedWorkVm, TaskListingVm, TaskRunVm, TaskVm } from "@/lib/ipc/client";
import {
  syncPacedWork,
  syncTaskForget,
  syncTaskHistory,
  syncTaskRunNow,
  syncTasks,
} from "@/lib/ipc/client";
import { hydrateSyncListSizes } from "@/lib/stores/sync-detail";

/** The heading, and the promise the pane makes in one line. */
export const TASKS_PANE_TITLE = "Tasks";
export const TASKS_PANE_SUBTITLE =
  "Work keeper does on a schedule, and which host on this machine will actually run each one.";

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
export const TASKS_UNKNOWN_ROW_TESTID = "task-unknown-row";
export const TASKS_HOST_TESTID = "task-host";
export const TASKS_REFUSAL_TESTID = "task-refusal";
/** Where a refusal whose row the listing no longer holds is drawn instead. */
export const TASKS_ORPHAN_REFUSAL_TESTID = "tasks-orphan-refusal";
export const TASKS_ERROR_TESTID = "tasks-error";
export const TASK_FORGET_TESTID = "task-forget-confirm";

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
    </div>
  );
}

function TaskRow({
  task,
  now,
  refusal,
  running,
  deleting,
  editing,
  writing,
  historyOpen,
  historyRuns,
  historyError,
  onRunNow,
  onEditToggle,
  onSaved,
  onSavingChange,
  onForget,
  onHistoryToggle,
}: {
  task: TaskVm;
  now: number;
  refusal: string | null;
  running: boolean;
  /** This row's own Forget is in flight, so a second confirm cannot re-issue it. */
  deleting: boolean;
  editing: boolean;
  /** A save is in flight somewhere in the pane — see `formSaving`. */
  writing: boolean;
  /** Whether this row's runs are the pane's one open section. */
  historyOpen: boolean;
  /**
   * The runs read for this row: `null` while unread, `[]` for a task with none.
   *
   * The data rather than a rendered node, so this component stays a pure
   * function of its task and no caller can hand a row the runs of another one.
   */
  historyRuns: TaskRunVm[] | null;
  historyError: string | null;
  onRunNow: (id: string) => void;
  onEditToggle: (id: string) => void;
  onSaved: () => void;
  onSavingChange: (saving: boolean) => void;
  onForget: (id: string) => void;
  onHistoryToggle: (id: string) => void;
}) {
  const unhosted = task.host.kind === "unhosted";
  const report = taskReportText(task.lastRun);
  // Names the region the disclosure genuinely opens, which this project treats
  // as a requirement rather than a nicety (`sidebar-pane.tsx`, `note-editor.tsx`
  // and two guard tests): `aria-expanded` alone announces "collapsed" and gives
  // a screen-reader user nothing to jump to. Passed only while the section
  // exists, `note-editor.tsx`'s form, so there is never a dangling IDREF.
  const historyRegionId = useId();
  // The header disclosure's rule, per row: the form is closed from inside
  // itself, so without this focus lands on `<body>`.
  const editTriggerRef = useRef<HTMLButtonElement>(null);
  const wasEditing = useRef(false);
  useEffect(() => {
    if (!editing && wasEditing.current) {
      editTriggerRef.current?.focus();
    }
    wasEditing.current = editing;
  }, [editing]);
  return (
    <li
      data-testid={TASKS_ROW_TESTID}
      data-task-id={task.id}
      className="flex flex-col gap-3 border-border border-b px-6 py-4 last:border-b-0"
    >
      <div className="flex items-start justify-between gap-4">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            {/* The kind as stored, so a kind a newer keeper wrote is shown
                rather than hidden (NFR-43). */}
            <Badge variant="secondary">{task.kind}</Badge>
            <span className="truncate font-medium text-foreground text-sm">{task.id}</span>
          </div>
          <p className="truncate text-muted-foreground text-xs">
            {task.profile ?? (task.profileId === null ? TASK_HOST_WIDE_TEXT : task.profileId)}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={running}
            onClick={() => onRunNow(task.id)}
          >
            {TASK_RUN_NOW_TEXT}
          </Button>
          {/* A disclosure, not a dialog: the same component the header reveals,
              in the row it is about (AD-C7). Disabled while a save is in flight
              for the reason the header's twin is — pressing it unmounts the form
              Rust's answer has to land in. */}
          <Button
            ref={editTriggerRef}
            type="button"
            variant="outline"
            size="sm"
            aria-expanded={editing}
            disabled={writing}
            onClick={() => onEditToggle(task.id)}
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
            disabled={writing || deleting}
            onClick={() => onForget(task.id)}
          >
            {TASK_FORGET_TEXT}
          </Button>
        </div>
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

      {/* The host claim, and the one place on screen it comes from. The label
          is the scannable word; the sentence beside it is Rust's, verbatim. */}
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
          Forget the engine would not do. The row keeps every other value it
          had: nothing here may read as though the task ran or went away. */}
      {refusal !== null && (
        <p role="alert" data-testid={TASKS_REFUSAL_TESTID} className="text-destructive text-sm">
          {refusal}
        </p>
      )}

      {/* A link-weight control on its own line, and NOT a fourth `Button` in
          the header cluster. `FoldToggle` states the rule that settles it: a
          control that changes "how much of a list is on screen … is not an
          action on the folder and must not carry the same visual weight as
          Retry or Sync now" (`list-fold.tsx`). The second reason is the cluster
          itself, which already holds Run now, Edit and Forget in a `shrink-0`
          block — so at a narrow window this row's id is what truncates to pay
          for each one, and jsdom performs no layout, so no component test in
          this file could ever catch a control that had left the screen.

          It wears `FoldToggle`'s own treatment for the same reason it borrows
          its rule, and deliberately NOT `text-faint`: that tone is "reserved for
          `aria-hidden` glyphs and section labels … and never carries a fact"
          (`DESIGN.md`), and this control is the only route to a task's history,
          which makes it the most load-bearing thing on the row. The section
          below still prints no heading of its own, because the trigger names it.

          Refused while a write is on its way, the rule Edit and Forget already
          follow: opening this closes an edit form, so pressing it mid-save would
          unmount the form Rust's answer has to land in — and a row whose Forget
          is in flight is about to go, so answering "no runs recorded" about it
          would be a claim about a record that is leaving.

          No focus-return effect, unlike Edit: on the self-close path focus never
          leaves the trigger, which is also why the section needs no close
          control of its own. The section can also be destroyed without a press —
          `refresh` prunes it when the row leaves the listing — but that takes
          the whole row with it, which is the case the pane already accepts for
          Edit and Forget. */}
      <button
        type="button"
        aria-expanded={historyOpen}
        aria-controls={historyOpen ? historyRegionId : undefined}
        // Named for its task, `FoldToggle`'s reason: ten rows would otherwise
        // offer ten controls a screen reader calls "Runs".
        aria-label={`${TASK_HISTORY_TITLE}: ${task.id}`}
        disabled={writing || deleting}
        onClick={() => onHistoryToggle(task.id)}
        className="self-start text-muted-foreground text-xs underline decoration-dotted hover:text-foreground"
      >
        {TASK_HISTORY_TITLE}
      </button>
      {historyOpen && (
        <TaskRunList
          taskId={task.id}
          regionId={historyRegionId}
          runs={historyRuns}
          error={historyError}
          now={now}
        />
      )}

      {/* Capped where the row is not, the Sync pane's reason: a form is read
          line by line, and a label-and-field pair stretched across a wide
          window is worse than one that sits still. */}
      {editing && (
        <Card size="sm" className="w-full max-w-[720px]">
          <CardContent>
            <TaskForm
              task={task}
              onSaved={onSaved}
              onCancel={() => onEditToggle(task.id)}
              onSavingChange={onSavingChange}
            />
          </CardContent>
        </Card>
      )}
    </li>
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
                {/* Null only for a paused or a governed row, and the sentence
                    below says which — so this cell never guesses a reason. */}
                <Field label={PACED_CADENCE_LABEL}>{row.cadence ?? PACED_NO_CADENCE_TEXT}</Field>
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
   * The task a Forget is asking about, and whether the question is on screen.
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
   */
  const [forgetSubject, setForgetSubject] = useState<string | null>(null);
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

  /**
   * Refusals whose row the listing no longer holds.
   *
   * `refusals` is keyed by task id and drawn by {@link TaskRow}, so a refusal
   * for a task that is not in the listing had nowhere to be drawn at all — and
   * the likeliest reason `sync_task_forget` refuses is that another writer on
   * this shared record removed the row first, at which point the re-read in
   * `forget`'s own `finally` takes away the row that would have carried the
   * sentence. A failed delete then looked exactly like a successful one, which
   * is the invisible-failure shape this whole epic exists to close, so an
   * orphaned refusal is promoted to the pane's own alert instead of dropped.
   */
  const orphanRefusals =
    listing === null
      ? []
      : Object.entries(refusals).filter(([id]) => !listing.tasks.some((task) => task.id === id));

  return (
    <section
      aria-label={TASKS_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading text-title">{TASKS_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{TASKS_PANE_SUBTITLE}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
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

      <ScrollArea fitWidth className="min-h-0 flex-1">
        <div data-slot="tasks-body" className="flex flex-col">
          {error !== null && (
            <p
              role="alert"
              data-testid={TASKS_ERROR_TESTID}
              className="px-6 pt-4 text-destructive text-sm"
            >
              {error}
            </p>
          )}
          {/* A refusal whose row the listing no longer has — see
              `orphanRefusals`. Named by its task, because the row that would
              have said which one is gone. */}
          {orphanRefusals.map(([id, refusal]) => (
            <p
              key={id}
              role="alert"
              data-testid={TASKS_ORPHAN_REFUSAL_TESTID}
              className="px-6 pt-4 text-destructive text-sm"
            >
              {id}: {refusal}
            </p>
          ))}
          {listing === null && error === null && (
            <p className="px-6 pt-4 text-muted-foreground text-sm">{TASKS_PANE_LOADING_SENTENCE}</p>
          )}
          {/* The add form, revealed by the header and mounted at the top of the
              body — inline, never a dialog (AD-C7): the two configuration
              surfaces are the same component, so they cannot word or validate a
              task differently. Closing unmounts it, so the next open starts from
              a fresh form rather than an abandoned draft. */}
          {adding && (
            <Card size="sm" className="m-6 w-full max-w-[720px]">
              <CardContent>
                <TaskForm
                  onSaved={() => {
                    setAdding(false);
                    void refresh();
                  }}
                  onCancel={() => setAdding(false)}
                  onSavingChange={setFormSaving}
                />
              </CardContent>
            </Card>
          )}
          {listing !== null && listing.tasks.length === 0 && listing.unknown.length === 0 && (
            <div className="flex flex-col gap-2 px-6 pt-4">
              <p className="text-muted-foreground text-sm">{TASKS_PANE_EMPTY_SENTENCE}</p>
              <code className="w-fit max-w-full overflow-x-auto rounded bg-muted px-2 py-1 font-mono text-foreground text-xs">
                {TASKS_PANE_EMPTY_COMMAND}
              </code>
              <p className="text-muted-foreground text-sm">{TASKS_PANE_EMPTY_AFTER}</p>
            </div>
          )}
          {listing !== null && listing.tasks.length > 0 && (
            <ul className="flex flex-col">
              {listing.tasks.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  now={now}
                  refusal={refusals[task.id] ?? null}
                  running={running[task.id] === true}
                  deleting={deleting[task.id] === true}
                  editing={editingId === task.id}
                  writing={formSaving}
                  // All three read the ONE slot, so a row can never be handed
                  // another row's runs: the id and the runs it belongs to move
                  // together or not at all.
                  historyOpen={history?.id === task.id}
                  historyRuns={history?.id === task.id ? history.runs : null}
                  historyError={history?.id === task.id ? history.error : null}
                  onRunNow={(id) => void runNow(id)}
                  // Opening an edit form closes this row's runs, the mirror of
                  // `toggleHistory` closing the form: one of the two, never both,
                  // on one row.
                  onEditToggle={(id) => {
                    setEditingId((open) => (open === id ? null : id));
                    if (historyRef.current?.id === id) {
                      historyToken.current += 1;
                      openHistory(null);
                    }
                  }}
                  onSaved={() => {
                    setEditingId(null);
                    void refresh();
                  }}
                  onSavingChange={setFormSaving}
                  onForget={(id) => {
                    setForgetSubject(id);
                    setForgetAsking(true);
                  }}
                  onHistoryToggle={toggleHistory}
                />
              ))}
            </ul>
          )}
          {/* These rows carry no Edit and no Forget, now that the readable ones
              do. They are not `TaskVm`s — `db::list_tasks` could not decode
              them — so there is nothing to seed a form from, and an upsert built
              out of a reason string is one `sync_task_save` would refuse. A
              control that can only fail is worse than no control. */}
          {listing !== null && listing.unknown.length > 0 && (
            <>
              <h2 className="border-border border-t px-6 pt-4 font-heading text-muted-foreground text-sm">
                {TASKS_UNKNOWN_HEADING}
              </h2>
              <ul className="flex flex-col">
                {listing.unknown.map((row, index) => (
                  <li
                    // The index, because the ID is the thing that is not unique
                    // here (finding 10): `db::list_tasks` emits
                    // `UnknownTask { id: String::new(), … }` for a row whose `id`
                    // column will not read, and two of those gave React two
                    // siblings keyed `""` — a duplicate-key warning, and
                    // reconciliation free to reuse one row's DOM for the other so
                    // the two distinct reasons swap or fail to update. This is
                    // the one list that exists to tolerate malformed rows, and it
                    // is `ORDER BY id` from the store rather than reorderable by
                    // the user, so the index is stable across reads.
                    // biome-ignore lint/suspicious/noArrayIndexKey: see above — the id is not unique
                    key={`${index}:${row.id}`}
                    data-testid={TASKS_UNKNOWN_ROW_TESTID}
                    data-task-id={row.id}
                    className="flex flex-col gap-1 px-6 py-3"
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
          {/* Last in the body and inside the same `ScrollArea`, because it is a
              second class in ONE view and not a second view: the question it
              answers — "is that everything keeper does on a clock?" — is only
              asked once the task list above has been read. Rendered
              unconditionally, so its own loading line and its own empty sentence
              are reachable rather than hidden behind the tasks' states. */}
          <PacedWorkList rows={paced} error={pacedError} />
        </div>
      </ScrollArea>

      {/* Asked before anything is deleted, and the question says what the answer
          costs. Every word of it is the backend's own framing: this deletes a
          record, never content. */}
      <AlertDialog open={forgetAsking} onOpenChange={(open) => !open && setForgetAsking(false)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            {/* Named from `forgetSubject` and never from the slot that drives
                `open`, so the question still names its task through the close. */}
            <AlertDialogTitle>
              {forgetSubject !== null && taskForgetConfirmTitle(forgetSubject)}
            </AlertDialogTitle>
            <AlertDialogDescription data-testid={TASK_FORGET_TESTID}>
              {TASK_FORGET_CONFIRM_BODY}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{TASK_FORGET_CANCEL_TEXT}</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                if (forgetSubject !== null) {
                  void forget(forgetSubject);
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
