/**
 * The Tasks pane states the truth about every row (Epic 57, Story 57.6,
 * FR-351, FR-352, AD-137).
 *
 * The assertions here are the epic's promises, one apiece: a row names the host
 * that will actually run it; the macOS case says *only while keeper is running*;
 * a task no present host can run reads **Unhosted** with its reason; a row this
 * build cannot read is shown rather than dropped; and a Run now the engine
 * refuses shows the refusal without any row claiming the task ran.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncTasks: vi.fn(),
  syncTaskRunNow: vi.fn(),
  syncTaskHistory: vi.fn(),
  syncTaskSave: vi.fn(),
  syncTaskForget: vi.fn(),
}));

import {
  formatTaskAgo,
  formatTaskDue,
  TASK_DUE_NOW_TEXT,
  TASK_HOST_LABEL,
  TASK_HOST_WIDE_TEXT,
  TASK_IN_FLIGHT_TEXT,
  TASK_LAST_OUTCOME_LABEL,
  TASK_LAST_RUN_LABEL,
  TASK_NEVER_DUE_TEXT,
  TASK_NEVER_RAN_TEXT,
  TASK_NEXT_DUE_LABEL,
  TASK_NO_SCHEDULE_TEXT,
  TASK_RUN_NOW_TEXT,
  TASK_SCHEDULE_LABEL,
  TASKS_PANE_EMPTY_SENTENCE,
  TASKS_PANE_TITLE,
  TASKS_REFUSAL_TESTID,
  TASKS_ROW_TESTID,
  TASKS_UNKNOWN_BADGE,
  TASKS_UNKNOWN_HEADING,
  TASKS_UNKNOWN_ROW_TESTID,
  TasksPane,
  taskOutcomeText,
} from "@/components/layout/tasks-pane";
import type { TaskListingVm, TaskRunVm, TaskVm } from "@/lib/ipc/client";
import { syncTaskRunNow, syncTasks } from "@/lib/ipc/client";

const NOW = 1_760_000_000_000;

/** The sentences `keeper_core::tasks` composes, verbatim. */
const SENTENCE_APP = "keeper runs this — only while keeper is running";
const SENTENCE_DAEMON = "the keeper-syncd unit on this machine runs this, logged in or not";
const SENTENCE_UNHOSTED = "nothing will run this";
const REASON_FOLDER_GONE = "it names a folder keeper does not sync, so no host here can run it";

function run(over: Partial<TaskRunVm> = {}): TaskRunVm {
  return {
    id: 1,
    taskId: "01SCHED",
    startedMs: NOW - 5 * 60_000,
    finishedMs: NOW - 4 * 60_000,
    outcome: "ok",
    unknownOutcome: null,
    detail: "no folders to sync",
    host: "dev#1",
    ...over,
  };
}

function task(over: Partial<TaskVm> = {}): TaskVm {
  return {
    id: "01SCHED",
    kind: "sync",
    mode: "scheduled",
    enabled: true,
    profileId: null,
    profile: null,
    schedule: "@daily",
    nextDueMs: NOW + 3_600_000,
    runningHost: null,
    leaseUntilMs: null,
    lastRun: run(),
    host: { kind: "app", sentence: SENTENCE_APP, reason: null },
    ...over,
  };
}

function listing(over: Partial<TaskListingVm> = {}): TaskListingVm {
  return { tasks: [task()], unknown: [], ...over };
}

function answer(value: TaskListingVm): void {
  vi.mocked(syncTasks).mockResolvedValue(value);
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("the Tasks pane", () => {
  it("states, per row, the kind, schedule, host, next due, last run and last outcome", async () => {
    answer(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    expect(within(row).getByText("sync")).toBeInTheDocument();
    expect(within(row).getByText("01SCHED")).toBeInTheDocument();
    // Host-wide: the row says so rather than leaving the folder column blank.
    expect(within(row).getByText(TASK_HOST_WIDE_TEXT)).toBeInTheDocument();

    for (const label of [
      TASK_SCHEDULE_LABEL,
      TASK_NEXT_DUE_LABEL,
      TASK_LAST_RUN_LABEL,
      TASK_LAST_OUTCOME_LABEL,
      TASK_HOST_LABEL,
    ]) {
      expect(within(row).getByText(label)).toBeInTheDocument();
    }
    expect(within(row).getByText("@daily")).toBeInTheDocument();
    expect(within(row).getByText("Succeeded")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeInTheDocument();
  });

  it("says on the macOS shape that the task runs only while keeper is running", async () => {
    // The sentence is Rust's and is rendered verbatim: on a Mac there is no
    // keeper-syncd unit anywhere in the repository, so the app is the only host
    // and the row must not imply a background service (AD-137).
    answer(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(row).getByText(SENTENCE_APP)).toBeInTheDocument();
    expect(within(row).getByText("This app")).toBeInTheDocument();
    expect(within(row).queryByText(SENTENCE_DAEMON)).not.toBeInTheDocument();
  });

  it("credits the daemon when the daemon is the host", async () => {
    answer(
      listing({
        tasks: [task({ host: { kind: "daemon", sentence: SENTENCE_DAEMON, reason: null } })],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(row).getByText(SENTENCE_DAEMON)).toBeInTheDocument();
    expect(within(row).getByText("Daemon")).toBeInTheDocument();
  });

  it("reads Unhosted, with its reason, for a task no present host can run", async () => {
    // The honest negative, and the whole reason AD-137 is an architecture
    // decision and not UI copy: this row looks enabled and will never fire, and
    // nobody notices the absence of housekeeping.
    answer(
      listing({
        tasks: [
          task({
            id: "01GONE",
            profileId: "01NOSUCHPROFILE",
            profile: null,
            host: {
              kind: "unhosted",
              sentence: SENTENCE_UNHOSTED,
              reason: REASON_FOLDER_GONE,
            },
          }),
        ],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(row).getByText("Unhosted")).toBeInTheDocument();
    expect(within(row).getByText(SENTENCE_UNHOSTED)).toBeInTheDocument();
    expect(within(row).getByText(REASON_FOLDER_GONE)).toBeInTheDocument();
    // Not silently promoted to a host: an unhosted row must never also carry a
    // sentence that claims something will run it.
    expect(within(row).queryByText(SENTENCE_APP)).not.toBeInTheDocument();
  });

  it("keeps off and unhosted apart", async () => {
    // A switched-off task is off on purpose. Wording it as unhosted would raise
    // an alarm about a row the user deliberately silenced.
    answer(
      listing({
        tasks: [
          task({
            mode: "off",
            host: {
              kind: "off",
              sentence: "switched off — nothing runs this, not even a request",
              reason: null,
            },
          }),
        ],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(row).getByText("Off")).toBeInTheDocument();
    expect(within(row).queryByText("Unhosted")).not.toBeInTheDocument();
  });

  it("shows a row this build cannot read rather than dropping it", async () => {
    // NFR-43. A list that silently omitted the row would tell the user they have
    // no such task, while the other host runs it every night.
    answer(
      listing({
        tasks: [],
        unknown: [{ id: "01TELEPORT", reason: "unknown task kind 'teleport'" }],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);
    expect(screen.getByText(TASKS_UNKNOWN_HEADING)).toBeInTheDocument();
    expect(within(row).getByText("01TELEPORT")).toBeInTheDocument();
    expect(within(row).getByText("unknown task kind 'teleport'")).toBeInTheDocument();
    expect(within(row).getByText(TASKS_UNKNOWN_BADGE)).toBeInTheDocument();
    // And it is not counted as "no tasks at all".
    expect(screen.queryByText(TASKS_PANE_EMPTY_SENTENCE)).not.toBeInTheDocument();
  });

  it("calls the command on Run now and re-reads afterwards", async () => {
    answer(listing());
    vi.mocked(syncTaskRunNow).mockResolvedValue(run({ id: 2, startedMs: NOW }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    await waitFor(() => {
      expect(syncTaskRunNow).toHaveBeenCalledWith("01SCHED");
    });
    // Twice: the mount read, and the read after the run. A run changes the
    // history, the window and possibly the lease, so a pane that did not re-read
    // would show a task as never-run immediately after running it.
    await waitFor(() => {
      expect(syncTasks).toHaveBeenCalledTimes(2);
    });
    expect(screen.queryByTestId(TASKS_REFUSAL_TESTID)).not.toBeInTheDocument();
  });

  it("shows a refused Run now on the row, and no row claims the task ran", async () => {
    // The engine refuses an off task by name and a busy one by lease. Both
    // reject rather than resolving with a sad outcome, so a pane that only read
    // the resolution would report a run that never happened.
    answer(listing({ tasks: [task({ lastRun: null })] }));
    // An `IpcError` and deliberately not an `Error`: Tauri maps a Rust `Err` to
    // a *value*, so `client.ts` normalises every rejection into this plain
    // object. A pane that read the sentence with `instanceof Error` renders
    // "[object Object]" here — the refusal's one actionable half, gone.
    vi.mocked(syncTaskRunNow).mockRejectedValue({
      code: "internal",
      message: "task 01SCHED is off, so nothing runs it — not even a request",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    const refusal = await screen.findByTestId(TASKS_REFUSAL_TESTID);
    expect(refusal).toHaveTextContent("nothing runs it — not even a request");
    // The row keeps the state it had: both the last-run and the last-outcome
    // cell still say never run, so nothing on screen reads as though the
    // refusal produced a run.
    const after = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(after).getAllByText(TASK_NEVER_RAN_TEXT)).toHaveLength(2);
    expect(within(after).queryByText(TASK_IN_FLIGHT_TEXT)).not.toBeInTheDocument();
    expect(within(after).queryByText("Succeeded")).not.toBeInTheDocument();
  });

  it("has a heading and does not claim an empty list before the first read lands", async () => {
    answer(listing({ tasks: [], unknown: [] }));
    render(<TasksPane />);
    expect(screen.getByRole("heading", { name: TASKS_PANE_TITLE })).toBeInTheDocument();
    expect(await screen.findByText(TASKS_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });
});

describe("the pane's own formatters", () => {
  it("renders next due from the instant Rust ships, and says due now for an open window", () => {
    // Relative times are composed client-side (`formatSyncWaited`'s precedent):
    // Rust ships instants, so a pane left open does not show a figure that
    // froze when the read landed.
    expect(formatTaskDue(NOW + 30_000, NOW)).toBe("in under a minute");
    expect(formatTaskDue(NOW + 5 * 60_000, NOW)).toBe("in 5 min");
    expect(formatTaskDue(NOW + 3 * 3_600_000, NOW)).toBe("in 3 hr");
    expect(formatTaskDue(NOW + 86_400_000, NOW)).toBe("in 1 day");
    expect(formatTaskDue(NOW + 3 * 86_400_000, NOW)).toBe("in 3 days");
    // An already-open window is a fact, not a countdown that overran.
    expect(formatTaskDue(NOW - 60_000, NOW)).toBe(TASK_DUE_NOW_TEXT);
    // And a task nothing will ever make due says so rather than showing a date.
    expect(formatTaskDue(null, NOW)).toBe(TASK_NEVER_DUE_TEXT);
  });

  it("renders how long ago a run started", () => {
    expect(formatTaskAgo(NOW - 10_000, NOW)).toBe("just now");
    expect(formatTaskAgo(NOW - 5 * 60_000, NOW)).toBe("5 min ago");
    expect(formatTaskAgo(NOW - 2 * 3_600_000, NOW)).toBe("2 hr ago");
    expect(formatTaskAgo(NOW - 86_400_000, NOW)).toBe("1 day ago");
    expect(formatTaskAgo(NOW - 4 * 86_400_000, NOW)).toBe("4 days ago");
    // A clock that went backwards is not a negative duration.
    expect(formatTaskAgo(NOW + 60_000, NOW)).toBe("just now");
  });

  it("separates in-flight, a known outcome and a spelling a newer keeper wrote", () => {
    // Three distinct facts, and the pair of keys is what keeps them apart: both
    // null means the run has not finished, a string in `unknownOutcome` means a
    // newer keeper recorded a spelling this build cannot read — rendered
    // verbatim, never as "unknown" — and a known `outcome` gets its label.
    expect(taskOutcomeText(null)).toBe(TASK_NEVER_RAN_TEXT);
    expect(taskOutcomeText(run({ outcome: null, unknownOutcome: null, finishedMs: null }))).toBe(
      TASK_IN_FLIGHT_TEXT,
    );
    expect(taskOutcomeText(run({ outcome: null, unknownOutcome: "teleported" }))).toBe(
      "teleported",
    );
    expect(taskOutcomeText(run({ outcome: "failed" }))).toBe("Failed");
    // Neither busy nor deferred is worded as a failure: the work did not run.
    expect(taskOutcomeText(run({ outcome: "busy" }))).toBe("Target was already in use");
    expect(taskOutcomeText(run({ outcome: "deferred" }))).toBe("Waited for a condition");
    // A spelling this build knows nothing about renders as itself.
    expect(taskOutcomeText(run({ outcome: "sublimated" }))).toBe("sublimated");
  });

  it("says a schedule is absent rather than rendering an empty cell", async () => {
    answer(listing({ tasks: [task({ schedule: null, nextDueMs: null })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    expect(within(row).getByText(TASK_NO_SCHEDULE_TEXT)).toBeInTheDocument();
    expect(within(row).getByText(TASK_NEVER_DUE_TEXT)).toBeInTheDocument();
  });
});
