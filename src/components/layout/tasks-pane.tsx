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
 */
import { useCallback, useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { TaskListingVm, TaskRunVm, TaskVm } from "@/lib/ipc/client";
import { syncTaskRunNow, syncTasks } from "@/lib/ipc/client";

/** The heading, and the promise the pane makes in one line. */
export const TASKS_PANE_TITLE = "Tasks";
export const TASKS_PANE_SUBTITLE =
  "Work keeper does on a schedule, and which host on this machine will actually run each one.";

/** Before the first read has landed the list is unknown, not empty. */
export const TASKS_PANE_LOADING_SENTENCE = "Reading the task record…";
export const TASKS_PANE_EMPTY_SENTENCE =
  "No tasks yet. `keeper-syncd task add` creates one; every host that shares this folder's record will see it.";

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

export const TASK_RUN_NOW_TEXT = "Run now";
export const TASK_REFRESH_TEXT = "Refresh";

/** Column labels, so the row is readable without a table header. */
export const TASK_SCHEDULE_LABEL = "Schedule";
export const TASK_HOST_LABEL = "Host";
export const TASK_NEXT_DUE_LABEL = "Next due";
export const TASK_LAST_RUN_LABEL = "Last run";
export const TASK_LAST_OUTCOME_LABEL = "Last outcome";

/** What a null in each of those columns honestly means. */
export const TASK_NO_SCHEDULE_TEXT = "none stored";
export const TASK_NEVER_DUE_TEXT = "nothing will make it due";
export const TASK_NEVER_RAN_TEXT = "never run";
export const TASK_DUE_NOW_TEXT = "due now";
export const TASK_IN_FLIGHT_TEXT = "running now";

/** Which folder a task is scoped to, or that it belongs to the machine. */
export const TASK_HOST_WIDE_TEXT = "the whole machine";

export const TASKS_ROW_TESTID = "task-row";
export const TASKS_UNKNOWN_ROW_TESTID = "task-unknown-row";
export const TASKS_HOST_TESTID = "task-host";
export const TASKS_REFUSAL_TESTID = "task-refusal";
export const TASKS_ERROR_TESTID = "tasks-error";

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
function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-muted-foreground text-xs uppercase tracking-wide">{label}</dt>
      <dd className="text-foreground text-sm">{children}</dd>
    </div>
  );
}

function TaskRow({
  task,
  now,
  refusal,
  running,
  onRunNow,
}: {
  task: TaskVm;
  now: number;
  refusal: string | null;
  running: boolean;
  onRunNow: (id: string) => void;
}) {
  const unhosted = task.host.kind === "unhosted";
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
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          disabled={running}
          onClick={() => onRunNow(task.id)}
        >
          {TASK_RUN_NOW_TEXT}
        </Button>
      </div>

      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Field label={TASK_SCHEDULE_LABEL}>{task.schedule ?? TASK_NO_SCHEDULE_TEXT}</Field>
        <Field label={TASK_NEXT_DUE_LABEL}>{formatTaskDue(task.nextDueMs, now)}</Field>
        <Field label={TASK_LAST_RUN_LABEL}>
          {task.lastRun === null ? TASK_NEVER_RAN_TEXT : formatTaskAgo(task.lastRun.startedMs, now)}
        </Field>
        <Field label={TASK_LAST_OUTCOME_LABEL}>{taskOutcomeText(task.lastRun)}</Field>
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

      {/* A refusal from Run now, quoted where it was asked. The row keeps every
          other value it had: nothing here may read as though the task ran. */}
      {refusal !== null && (
        <p role="alert" data-testid={TASKS_REFUSAL_TESTID} className="text-destructive text-sm">
          {refusal}
        </p>
      )}
    </li>
  );
}

export function TasksPane() {
  const [listing, setListing] = useState<TaskListingVm | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Per-task refusals from Run now, keyed by task id. */
  const [refusals, setRefusals] = useState<Record<string, string>>({});
  const [runningId, setRunningId] = useState<string | null>(null);
  /**
   * The instant every relative time on screen is measured from, captured per
   * read rather than per render: two rows re-rendered a tick apart must not
   * disagree about what "now" is, and re-reading the clock in each formatter
   * would make the pane's output depend on render order.
   */
  const [now, setNow] = useState(() => Date.now());

  const refresh = useCallback(async () => {
    try {
      const next = await syncTasks();
      setListing(next);
      setNow(Date.now());
      setError(null);
    } catch (cause) {
      setError(messageOf(cause));
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const runNow = useCallback(
    async (id: string) => {
      setRunningId(id);
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
        setRunningId(null);
        // Re-read either way: a refused run still changes nothing, and a run
        // that happened changed the history, the window and possibly the lease.
        await refresh();
      }
    },
    [refresh],
  );

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
        <Button
          type="button"
          variant="outline"
          size="sm"
          className="shrink-0"
          onClick={() => void refresh()}
        >
          {TASK_REFRESH_TEXT}
        </Button>
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
          {listing === null && error === null && (
            <p className="px-6 pt-4 text-muted-foreground text-sm">{TASKS_PANE_LOADING_SENTENCE}</p>
          )}
          {listing !== null && listing.tasks.length === 0 && listing.unknown.length === 0 && (
            <p className="px-6 pt-4 text-muted-foreground text-sm">{TASKS_PANE_EMPTY_SENTENCE}</p>
          )}
          {listing !== null && listing.tasks.length > 0 && (
            <ul className="flex flex-col">
              {listing.tasks.map((task) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  now={now}
                  refusal={refusals[task.id] ?? null}
                  running={runningId === task.id}
                  onRunNow={(id) => void runNow(id)}
                />
              ))}
            </ul>
          )}
          {listing !== null && listing.unknown.length > 0 && (
            <>
              <h2 className="border-border border-t px-6 pt-4 font-heading text-muted-foreground text-sm">
                {TASKS_UNKNOWN_HEADING}
              </h2>
              <ul className="flex flex-col">
                {listing.unknown.map((row) => (
                  <li
                    key={row.id}
                    data-testid={TASKS_UNKNOWN_ROW_TESTID}
                    data-task-id={row.id}
                    className="flex flex-col gap-1 px-6 py-3"
                  >
                    <span className="flex items-center gap-2">
                      <Badge variant="outline">{TASKS_UNKNOWN_BADGE}</Badge>
                      <span className="truncate font-medium text-foreground text-sm">{row.id}</span>
                    </span>
                    <span className="text-muted-foreground text-sm">{row.reason}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      </ScrollArea>
    </section>
  );
}
