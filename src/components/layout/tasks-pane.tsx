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
 */
import { useCallback, useEffect, useRef, useState } from "react";
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
import type { TaskListingVm, TaskRunVm, TaskVm } from "@/lib/ipc/client";
import { syncTaskForget, syncTaskRunNow, syncTasks } from "@/lib/ipc/client";

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
 * Three of the five are deliberately not failures — `busy` and `deferred` are a
 * run that did not happen, and `abandoned` is a lease the next host reclaimed —
 * so none of them is worded as one.
 */
const OUTCOME_LABELS: Record<string, string> = {
  ok: "Succeeded",
  busy: "Target was already in use",
  deferred: "Waited for a condition",
  failed: "Failed",
  abandoned: "Abandoned by the host that started it",
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
    return run.unknownOutcome;
  }
  if (run.outcome === null) {
    return TASK_IN_FLIGHT_TEXT;
  }
  return OUTCOME_LABELS[run.outcome] ?? run.outcome;
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
      <dd
        className={
          wide
            ? "whitespace-pre-wrap text-foreground text-sm [overflow-wrap:anywhere]"
            : "text-foreground text-sm"
        }
      >
        {children}
      </dd>
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
  onRunNow,
  onEditToggle,
  onSaved,
  onSavingChange,
  onForget,
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
  onRunNow: (id: string) => void;
  onEditToggle: (id: string) => void;
  onSaved: () => void;
  onSavingChange: (saving: boolean) => void;
  onForget: (id: string) => void;
}) {
  const unhosted = task.host.kind === "unhosted";
  /**
   * The run's own words, or `null` when there are none to draw.
   *
   * Blank counts as none, and that is the guard rather than a nicety: `detail`
   * is `TEXT NULL` with no non-empty constraint and `finish_task_run` binds
   * whatever it is handed, so a writer this build never met — the NFR-43 case
   * this file exists to tolerate — can store `""` or `" "`. On `!== null` alone
   * that renders a LAST REPORT heading over nothing, which is the one shape a
   * reader really would read as a failed read. Trimmed to decide, untrimmed to
   * draw: what is stored is what is shown.
   */
  const report =
    task.lastRun !== null && task.lastRun.detail !== null && task.lastRun.detail.trim() !== ""
      ? task.lastRun.detail
      : null;
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

export function TasksPane() {
  const [listing, setListing] = useState<TaskListingVm | null>(null);
  const [error, setError] = useState<string | null>(null);
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

  const refresh = useCallback(async (keepRefusals = false) => {
    readToken.current += 1;
    const mine = readToken.current;
    try {
      const next = await syncTasks();
      if (mine !== readToken.current) {
        return;
      }
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
    } catch (cause) {
      if (mine !== readToken.current) {
        return;
      }
      setError(messageOf(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const clock = setInterval(() => setNow(Date.now()), TASKS_CLOCK_TICK_MS);
    return () => clearInterval(clock);
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

  const runNow = useCallback(
    async (id: string) => {
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
      }
    },
    [refresh],
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
                  onRunNow={(id) => void runNow(id)}
                  onEditToggle={(id) => setEditingId((open) => (open === id ? null : id))}
                  onSaved={() => {
                    setEditingId(null);
                    void refresh();
                  }}
                  onSavingChange={setFormSaving}
                  onForget={(id) => {
                    setForgetSubject(id);
                    setForgetAsking(true);
                  }}
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
