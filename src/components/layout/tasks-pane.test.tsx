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
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncTasks: vi.fn(),
  syncTaskRunNow: vi.fn(),
  syncTaskHistory: vi.fn(),
  syncTaskSave: vi.fn(),
  syncTaskForget: vi.fn(),
  // The batched pair Story 59.4's multi-selection drives. Answered in
  // `beforeEach` with an empty receipt, so a test that presses a bulk control
  // without saying what came back gets a receipt rather than `undefined`.
  syncTasksSetEnabled: vi.fn(),
  syncTasksForget: vi.fn(),
  // Story 58.7's read. Mocked here and answered in `beforeEach`, because the
  // pane now reads it in the SAME settled pass as `syncTasks` — an unanswered
  // mock resolves `undefined` and the section would render rows out of it.
  syncPacedWork: vi.fn(),
  // The mounted form's own read (Story 58.1): the folder picker.
  syncProfiles: vi.fn(),
}));

import { LIST_FOLD_MORE_LABEL } from "@/components/layout/list-fold";
import { COLUMN_COLLAPSE_PREFIX } from "@/components/layout/surface-column";
import {
  formatTaskAgo,
  formatTaskDue,
  PACED_BADGE,
  PACED_CADENCE_LABEL,
  PACED_EMPTY_TEXT,
  PACED_FOLDER_LABEL,
  PACED_HEADING,
  PACED_KIND_LABELS,
  PACED_LOADING_TEXT,
  PACED_NO_CADENCE_TEXT,
  PACED_REFUSAL_TESTID,
  PACED_ROW_TESTID,
  PACED_STANDING_LABELS,
  PACED_SUBTITLE,
  TASK_DUE_NOW_TEXT,
  TASK_EDIT_TEXT,
  TASK_FORGET_CANCEL_TEXT,
  TASK_FORGET_CONFIRM_BODY,
  TASK_FORGET_TESTID,
  TASK_FORGET_TEXT,
  TASK_HISTORY_BOUND_NOTICE_AT,
  TASK_HISTORY_BOUND_TEXT,
  TASK_HISTORY_EMPTY_TEXT,
  TASK_HISTORY_LOADING_TEXT,
  TASK_HISTORY_NO_HOST_TEXT,
  TASK_HISTORY_RETRY_NOTE,
  TASK_HISTORY_TITLE,
  TASK_HOST_LABEL,
  TASK_IN_FLIGHT_TEXT,
  TASK_LAST_OUTCOME_LABEL,
  TASK_LAST_REPORT_LABEL,
  TASK_LAST_RUN_LABEL,
  TASK_NEVER_DUE_TEXT,
  TASK_NEVER_RAN_TEXT,
  TASK_NEXT_DUE_LABEL,
  TASK_NO_SCHEDULE_TEXT,
  TASK_REFRESH_TEXT,
  TASK_RUN_NOW_TEXT,
  TASK_SCHEDULE_LABEL,
  TASK_UNREADABLE_OUTCOME_TEXT,
  TASKS,
  TASKS_BULK_DISABLE_TEXT,
  TASKS_BULK_ENABLE_TEXT,
  TASKS_BULK_ERROR_TESTID,
  TASKS_BULK_FORGET_TEXT,
  TASKS_BULK_MISSING_TEXT,
  TASKS_BULK_NO_REASON_TEXT,
  TASKS_CLOCK_TICK_MS,
  TASKS_DESCRIPTION_TESTID,
  TASKS_DETAIL_LABEL,
  TASKS_DETAIL_TESTID,
  TASKS_ERROR_TESTID,
  TASKS_HISTORY_REFUSAL_TESTID,
  TASKS_HISTORY_ROW_TESTID,
  TASKS_HISTORY_TESTID,
  TASKS_LIST_LABEL,
  TASKS_ORPHAN_REFUSAL_TESTID,
  TASKS_PANE_EMPTY_AFTER,
  TASKS_PANE_EMPTY_COMMAND,
  TASKS_PANE_EMPTY_SENTENCE,
  TASKS_PANE_TITLE,
  TASKS_RAIL_LIST_LABEL,
  TASKS_REFUSAL_TESTID,
  TASKS_ROW_TESTID,
  TASKS_RUN_NOW_SENTENCE,
  TASKS_SELECTED_TESTID,
  TASKS_SELECTION_TESTID,
  TASKS_UNKNOWN_BADGE,
  TASKS_UNKNOWN_HEADING,
  TASKS_UNKNOWN_NO_ID_TEXT,
  TASKS_UNKNOWN_ROW_TESTID,
  TasksPane,
  taskForgetConfirmTitle,
  taskHistoryUnshownText,
  taskOutcomeText,
  tasksForgetConfirmTitle,
  tasksSelectionSentence,
} from "@/components/layout/tasks-pane";
import {
  TASK_FORM_ADD_SUBMIT_LABEL,
  TASK_FORM_ADD_TITLE,
  TASK_FORM_EDIT_SUBMIT_LABEL,
  TASK_FORM_EDIT_TITLE,
  TASK_FORM_ID_LABEL,
  TASK_FORM_SCHEDULE_LABEL,
  TASK_HOST_WIDE_TEXT,
} from "@/components/sync/task-form";
import { SURFACE_COLUMNS } from "@/lib/column-widths";
import { countLabel } from "@/lib/count-label";
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
  syncProfiles,
  syncTaskForget,
  syncTaskHistory,
  syncTaskRunNow,
  syncTaskSave,
  syncTasks,
  syncTasksForget,
  syncTasksSetEnabled,
} from "@/lib/ipc/client";
import { resetColumnFoldForTest } from "@/lib/stores/column-fold";
import { activePanel, panelsStore, resetPanelsStoreForTest, sameTarget } from "@/lib/stores/panels";
import {
  SYNC_LIST_FOLDED_FALLBACK,
  SYNC_LIST_UNFOLDED_FALLBACK,
  setSyncListSizes,
  syncListSizes,
} from "@/lib/stores/sync-detail";

const NOW = 1_760_000_000_000;

/** The sentences `keeper_core::tasks` composes, verbatim. */
const SENTENCE_APP = "keeper runs this — only while keeper is running";
const SENTENCE_DAEMON = "the keeper-syncd unit on this machine runs this, logged in or not";
const SENTENCE_UNHOSTED = "nothing will run this";
const REASON_FOLDER_GONE = "it names a folder keeper does not sync, so no host here can run it";

/**
 * The `PACED_SENTENCE_*` constants `keeper_core::tasks` composes, verbatim.
 *
 * Spelled out here rather than imported, for the reason the four above are: the
 * pane renders Rust's words and must not be allowed to agree with itself. A
 * paraphrase in Rust would leave these tests green while the screen changed.
 */
const PACED_SENTENCE_SCAN =
  "keeper looks for changes on this cadence while it is running. The cadence is a backstop and not the only trigger: a file the watcher sees settle, or a write that closes, brings the next look forward.";
const PACED_SENTENCE_GOVERNED =
  "a scheduled sync task decides when this folder is looked at, so the paced backstop has stood down. A file the watcher sees settle still brings a look forward.";
const PACED_SENTENCE_SWEEP =
  "keeper deletes transfer scratch this folder will never use again, on this cadence, while it is running.";
const PACED_SENTENCE_PAUSED =
  "this folder is paused, so nothing here is paced and no cadence is in force.";
const PACED_SENTENCE_UNREGISTERED =
  "keeper has no vault registered for this folder, so nothing paces it: the vault folder could not be found when the registry was last built. The registry is rebuilt at launch, and when a vault is flagged or unflagged — not when a drive comes back.";

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
    // Required on `TaskVm` and not rendered by this pane, which is why they sit
    // together with one reason rather than four: the fixture carries them
    // because the type does. `onMissed` and `updatedMs` arrived with Story 58.4
    // (the missed-window policy, and the reading a save is checked against);
    // `description` and `missedDelayMs` with Stories 59.5 and 59.6. Both of the
    // new pair are `null` here on purpose — `null` is *absent*, and 59.5's whole
    // column argument is that an absent description and a blank one are two
    // different facts. A test that wants either must say so at its own call
    // site, so this default can never quietly assert one of them.
    onMissed: "run_now",
    updatedMs: NOW - 60_000,
    description: null,
    missedDelayMs: null,
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

/** A paced scan row — the projection's canonical shape (Story 58.7). */
function pacedRow(over: Partial<PacedWorkVm> = {}): PacedWorkVm {
  return {
    id: "scan:p1",
    kind: "scan",
    profileId: "p1",
    profile: "keeper",
    standing: "paced",
    cadence: "about every 15 seconds",
    sentence: PACED_SENTENCE_SCAN,
    ...over,
  };
}

function answerPaced(rows: PacedWorkVm[]): void {
  vi.mocked(syncPacedWork).mockResolvedValue(rows);
}

/**
 * The row, which IS the listbox's own `option` (Story 59.1, re-roled by 59.4).
 *
 * `role="option"` and no longer a `<button>` inside an `<li>`: `aria-selected`
 * is not a supported state on `role="button"`, and a listbox has to own its
 * options directly — so once a selection could hold several rows, one row became
 * one element carrying the role, the state and the testid together. What has not
 * changed is that the row holds no controls of its own, which the block below
 * asserts as an absence.
 */
function rowOption(id: string): HTMLElement {
  const row = screen
    .getAllByTestId(TASKS_ROW_TESTID)
    .find((candidate) => candidate.dataset.taskId === id);
  expect(row, `row ${id}`).toBeDefined();
  return row as HTMLElement;
}

/** Choose a task with a plain click, which REPLACES the selection. */
function selectRow(id: string): void {
  fireEvent.click(rowOption(id));
}

/**
 * A modifier click, in `files-pane.test.tsx:2630`'s idiom.
 *
 * Inside one `act` with a flush, because the gesture settles a state update the
 * assertion after it reads — and one modifier per call, because three modifier
 * clicks in one `act` cannot tell you which modifier the handler honoured.
 */
async function clickRowWith(id: string, modifiers: MouseEventInit): Promise<void> {
  const option = rowOption(id);
  await act(async () => {
    fireEvent.click(option, modifiers);
    await Promise.resolve();
  });
}

/** Which rows read as selected, by id and in the list's own order. */
function selectedRows(): string[] {
  return screen
    .getAllByTestId(TASKS_ROW_TESTID)
    .filter((row) => row.getAttribute("aria-selected") === "true")
    .map((row) => row.dataset.taskId ?? "");
}

/**
 * Which rows carry the roving tab stop, by id and in the list's own order.
 *
 * There must always be exactly one while the listing is non-empty: a listbox
 * whose every option is `tabIndex -1` is unreachable by Tab.
 */
function tabStops(): string[] {
  return screen
    .getAllByTestId(TASKS_ROW_TESTID)
    .filter((row) => row.getAttribute("tabindex") === "0")
    .map((row) => row.dataset.taskId ?? "");
}

/** A control in the detail region, which draws exactly one task. */
function detailButton(name: string): HTMLElement {
  return within(screen.getByTestId(TASKS_DETAIL_TESTID)).getByRole("button", { name });
}

beforeEach(() => {
  // Every form this pane reveals reads the folder list as it mounts, so a test
  // that opens one needs an answer here or the read never resolves.
  vi.mocked(syncProfiles).mockResolvedValue([]);
  // The projection is read in the same pass as the listing, so every test needs
  // an answer for it. `[]` and not a row: the 63 tests that predate Story 58.7
  // are about the task list, and a projected row in each of them would put a
  // second `Cadence` cell inside reach of their queries.
  answerPaced([]);
  // An empty receipt rather than `undefined`: a bulk press in a test that does
  // not care what came back must not fall over inside the pane's own receipt
  // handling and report itself as a rendering failure.
  vi.mocked(syncTasksSetEnabled).mockResolvedValue({ entries: [] });
  vi.mocked(syncTasksForget).mockResolvedValue({ entries: [] });
  // The fold is module state and one test below folds the names away, so
  // without this every test declared after it renders a 48px strip and can find
  // no rows at all. It leaked harmlessly only while that test happened to be
  // last in the file.
  resetColumnFoldForTest();
  // The panel list is a module singleton and this pane now writes to it, so one
  // test's preview would otherwise be the next test's starting arrangement.
  resetPanelsStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("the Tasks pane", () => {
  it("states, per row, the kind, schedule, host, next due, last run, outcome and report", async () => {
    answer(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).getByText("sync")).toBeInTheDocument();
    expect(within(row).getByText("01SCHED")).toBeInTheDocument();
    // Host-wide: the row says so rather than leaving the folder column blank.
    expect(within(row).getByText(TASK_HOST_WIDE_TEXT)).toBeInTheDocument();

    for (const label of [
      TASK_SCHEDULE_LABEL,
      TASK_NEXT_DUE_LABEL,
      TASK_LAST_RUN_LABEL,
      TASK_LAST_OUTCOME_LABEL,
      // Added by Story 58.2, and added HERE and not only in that story's own
      // block: this is the test a reader opens to learn what a row says, so a
      // row that says six things and a test that enumerates five is the pane's
      // contract understating itself — and deleting the cell would leave this
      // test green.
      TASK_LAST_REPORT_LABEL,
      TASK_HOST_LABEL,
    ]) {
      expect(within(row).getByText(label)).toBeInTheDocument();
    }
    expect(within(row).getByText("@daily")).toBeInTheDocument();
    expect(within(row).getByText("Succeeded")).toBeInTheDocument();
    // The default fixture's own run reports this, so the canonical row carries
    // the run's words too.
    expect(within(row).getByText("no folders to sync")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeInTheDocument();
  });

  it("says on the macOS shape that the task runs only while keeper is running", async () => {
    // The sentence is Rust's and is rendered verbatim: on a Mac there is no
    // keeper-syncd unit anywhere in the repository, so the app is the only host
    // and the row must not imply a background service (AD-137).
    answer(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
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
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
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
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
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
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
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
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

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
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    const refusal = await screen.findByTestId(TASKS_REFUSAL_TESTID);
    expect(refusal).toHaveTextContent("nothing runs it — not even a request");
    // The row keeps the state it had: both the last-run and the last-outcome
    // cell still say never run, so nothing on screen reads as though the
    // refusal produced a run.
    const after = await screen.findByTestId(TASKS_DETAIL_TESTID);
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
    // Rust ships instants, and the pane measures them against a `now` it
    // re-reads on its own coarse tick. This block asserts only the arithmetic —
    // that the pane's `now` actually advances is a property of the component and
    // is asserted where it lives, in "a pane left open keeps its relative times
    // moving" below. Saying it here was the mistake finding 6 named: the comment
    // claimed a property no test had, and the property was in fact false.
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
    // Stories 58.4/58.5's two, and the property is that they are DIFFERENT
    // sentences: a declined window will never be served, a postponed one will be
    // served later. Rendering them as shades of one idea would tell somebody a
    // sweep had been dropped when it had only been held back.
    const declined = taskOutcomeText(run({ outcome: "declined" }));
    const postponed = taskOutcomeText(run({ outcome: "postponed" }));
    expect(declined).not.toBe(postponed);
    expect(declined).toMatch(/next window/);
    expect(postponed).toMatch(/later/);
    // Neither is worded as a failure, and neither reads as still running: both
    // rows are closed and zero-duration.
    for (const text of [declined, postponed]) {
      expect(text).not.toMatch(/fail|abandon/i);
      expect(text).not.toBe(TASK_IN_FLIGHT_TEXT);
    }
  });

  it("says a schedule is absent rather than rendering an empty cell", async () => {
    answer(listing({ tasks: [task({ schedule: null, nextDueMs: null })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
    expect(within(row).getByText(TASK_NO_SCHEDULE_TEXT)).toBeInTheDocument();
    expect(within(row).getByText(TASK_NEVER_DUE_TEXT)).toBeInTheDocument();
  });
});

/**
 * The defects Story 57.5's adversarial review found in this pane, each asserted
 * as the property it broke.
 */
describe("the Tasks pane's empty state names something that exists", () => {
  /**
   * The daemon's real task group and verbs, from `keeper-syncd`'s clap tree
   * (`keeper-syncd/src/commands.rs`: `Command::Tasks { TaskCommand }` with
   * `List`, `Status`, `Run`, `Set`, `Enable`, `Disable`, `Forget`).
   *
   * Hard-coded here on purpose: this file cannot import a Rust enum, and the
   * point of the check is that the copy and the CLI cannot drift apart silently.
   * If the CLI grows a verb, this list is the one place to add it.
   */
  const CLI_GROUP = "tasks";
  const CLI_VERBS = ["list", "status", "run", "set", "enable", "disable", "forget"];

  const COPY = [TASKS_PANE_EMPTY_SENTENCE, TASKS_PANE_EMPTY_COMMAND, TASKS_PANE_EMPTY_AFTER].join(
    "\n",
  );

  it("never names a keeper-syncd group or verb the CLI does not have", () => {
    // The defect verbatim: the sentence said "`keeper-syncd task add` creates
    // one". There is no `task` group and no `add` verb, so a user following it
    // got `ErrorKind::InvalidSubcommand` — and this is the ONLY text every
    // existing install sees when it first opens ⌘8, because nothing in this epic
    // creates a task row on migration or on open.
    expect(COPY).not.toMatch(/keeper-syncd\s+task\s/);
    // Narrowed from a blanket `/\badd\b/` by Story 58.1: the pane now
    // legitimately says "Add a task", because the app can create one. What the
    // blanket ban was actually protecting is the CLI phrase, so that is what is
    // banned — and the mechanical phrase loop below checks every
    // `keeper-syncd <group> <verb>` in the copy against the real clap tree,
    // which covers `add` and every other verb the binary does not have.
    expect(COPY).not.toMatch(/keeper-syncd\s+\S+\s+add\b/);

    // Mechanical rather than a spot check: every `keeper-syncd <word> <word>`
    // phrase in the copy is measured against the real tree, so a future rename
    // cannot quietly re-break this.
    const phrases = [...COPY.matchAll(/keeper-syncd\s+([a-z-]+)(?:\s+([a-z-]+))?/g)];
    expect(phrases.length).toBeGreaterThan(0);
    for (const [phrase, group, verb] of phrases) {
      expect(group, phrase).toBe(CLI_GROUP);
      // A bare "keeper-syncd tasks" with no verb is fine; a named verb must be
      // one the binary dispatches.
      if (verb !== undefined) {
        expect(CLI_VERBS, phrase).toContain(verb);
      }
    }
  });

  it("says a task can be made here, and offers the control that makes one", async () => {
    answer(listing({ tasks: [], unknown: [] }));
    render(<TasksPane />);

    // On the real strings, so a reworded constant still has to say these things.
    expect(await screen.findByText(TASKS_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
    // The inverse of what this asserted until Story 58.1. The old copy said the
    // view "cannot create one yet" and sent the reader to a terminal; it now can,
    // and a sentence that still said otherwise would have the app deny a button
    // sitting in the header above it.
    expect(TASKS_PANE_EMPTY_SENTENCE).not.toMatch(/cannot create/);
    expect(screen.getByRole("button", { name: TASK_FORM_ADD_TITLE })).toBeInTheDocument();
    // The command stays named as the other way in, and stays real.
    expect(screen.getByText(TASKS_PANE_EMPTY_COMMAND)).toBeInTheDocument();
    expect(TASKS_PANE_EMPTY_COMMAND).toContain("keeper-syncd tasks set");
    expect(screen.getByText(TASKS_PANE_EMPTY_AFTER)).toBeInTheDocument();
  });

  it("promises no background service the platform it is read on may not have", () => {
    // `keeper-syncd` ships for macOS as well as Linux, so the command itself is
    // true to read on either — but no launchd plist exists anywhere in the tree,
    // so nothing starts it in the background on a Mac. The empty state therefore
    // promises only what is true on both: keeper hosts a due task while keeper is
    // running, and the ROW states the real host. Anything stronger here would be
    // the over-claim AD-137 forbids.
    expect(TASKS_PANE_EMPTY_AFTER).toContain("while keeper is running");
    expect(COPY).not.toMatch(/logged in or not/);
    expect(COPY).not.toMatch(/\bsystemctl\b/);
    expect(COPY).not.toMatch(/\blaunchd\b/);
    // And no platform sniff produced any of it: the copy is one set of constants.
    expect(COPY).not.toMatch(/darwin|macOS|Windows|Linux/);
  });
});

describe("the Tasks pane's live state", () => {
  it("keeps a pane left open from freezing its relative times", async () => {
    // Finding 6. `now` was written only in a read's success branch, so a row
    // that said "in 5 min" said "in 5 min" an hour later and never reached
    // "due now". The clock is driven here rather than asserted in prose.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const due = Date.now() + 90_000;
      answer(listing({ tasks: [task({ nextDueMs: due })] }));
      render(<TasksPane />);
      const row = await screen.findByTestId(TASKS_ROW_TESTID);
      expect(within(row).getByText("in 1 min")).toBeInTheDocument();

      // Past the instant, with no read in between: only the pane's own tick can
      // move this, and `syncTasks` is asserted not to have been called again.
      await vi.advanceTimersByTimeAsync(TASKS_CLOCK_TICK_MS * 4);
      await waitFor(() => {
        expect(
          within(screen.getByTestId(TASKS_ROW_TESTID)).getByText(TASK_DUE_NOW_TEXT),
        ).toBeInTheDocument();
      });
      expect(syncTasks).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("re-enables only the row whose run settled", async () => {
    // Finding 7. One shared slot re-enabled task A the moment B was started, and
    // A's own settle then re-enabled B while B was still in flight — so a further
    // click issued a second run for a task that already held a lease and the pane
    // painted "somebody else is doing this" on a task the same user had started
    // from this same pane.
    answer(
      listing({
        tasks: [task({ id: "A" }), task({ id: "B" })],
      }),
    );
    // The executor form, not `Promise.withResolvers`: this project compiles
    // against `lib: ES2020`, which predates it — the reason seven other test
    // files in this tree give for the same choice.
    let settleA = (): void => {};
    let settleB = (): void => {};
    vi.mocked(syncTaskRunNow).mockImplementation((id: string) =>
      id === "A"
        ? new Promise<TaskRunVm>((resolve) => {
            settleA = () => resolve(run({ id: 2 }));
          })
        : new Promise<TaskRunVm>((resolve) => {
            settleB = () => resolve(run({ id: 3 }));
          }),
    );
    render(<TasksPane />);
    await waitFor(() => {
      expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2);
    });
    // Story 59.1 moved Run now into the detail region, so exactly one of these
    // is ever mounted. That does not weaken the property — it sharpens it. The
    // defect was a single `string | null` slot that cleared unconditionally, and
    // a slot is invisible while both buttons are on screen at once but obvious
    // the moment you come BACK to a row: what this now asserts is that the pane
    // still remembers A is running after it has drawn B and returned.
    const runNowFor = (id: string): HTMLElement => {
      selectRow(id);
      return detailButton(TASK_RUN_NOW_TEXT);
    };

    fireEvent.click(runNowFor("A"));
    await waitFor(() => {
      expect(detailButton(TASK_RUN_NOW_TEXT)).toBeDisabled();
    });
    fireEvent.click(runNowFor("B"));
    await waitFor(() => {
      expect(detailButton(TASK_RUN_NOW_TEXT)).toBeDisabled();
    });
    // THE PROPERTY: starting B must not re-offer A, which is still running.
    expect(runNowFor("A")).toBeDisabled();

    settleA();
    await waitFor(() => {
      expect(runNowFor("A")).toBeEnabled();
    });
    // ...and A settling must not re-offer B either.
    expect(runNowFor("B")).toBeDisabled();
    settleB();
    await waitFor(() => {
      expect(runNowFor("B")).toBeEnabled();
    });
  });

  it("never lets a slow listing read overwrite a newer one", async () => {
    // Finding 8. Three independent triggers, no ordering: press Refresh, then Run
    // now before it resolves, and the PRE-run listing could land last — the row
    // then showed "never run" immediately after a run that happened, the exact
    // failure the post-run re-read exists to prevent.
    const stale = listing({ tasks: [task({ lastRun: null })] });
    const fresh = listing({ tasks: [task({ lastRun: run({ startedMs: NOW - 60_000 }) })] });
    let releaseStale = (): void => {};
    vi.mocked(syncTasks)
      .mockResolvedValueOnce(fresh)
      .mockImplementationOnce(
        () =>
          new Promise<TaskListingVm>((resolve) => {
            releaseStale = () => resolve(stale);
          }),
      )
      .mockResolvedValue(fresh);

    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    // Read 2 is issued and parked; read 3 is issued and lands.
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
    await waitFor(() => {
      expect(syncTasks).toHaveBeenCalledTimes(3);
    });

    // Now the parked older read resolves, last.
    releaseStale();
    await waitFor(() => {
      expect(syncTasks).toHaveBeenCalledTimes(3);
    });
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
    expect(within(row).queryByText(TASK_NEVER_RAN_TEXT)).not.toBeInTheDocument();
  });

  it("clears a Run now refusal on a later read, but never on its own attempt's", async () => {
    // Finding 9. `refusals` was cleared at exactly one point — the top of
    // `runNow`, for the one id being run — so a "the other host is doing this"
    // alert kept asserting a task was busy elsewhere while the row above it
    // showed the completed run and no holder, clearable only by running it again.
    //
    // The half the finding did not state, and the reason the fix is not "clear on
    // every read": `runNow`'s settle issues a read of its own, so clearing there
    // erases the refusal in the tick it appeared — which is the pane's entire
    // answer to a refused Run now and an acceptance criterion of this story.
    answer(listing());
    vi.mocked(syncTaskRunNow).mockRejectedValue({
      code: "busy",
      message: "task 01SCHED is being run by another host on this machine",
      accountId: null,
      retriable: true,
    });
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    const refusal = await screen.findByTestId(TASKS_REFUSAL_TESTID);
    expect(refusal).toHaveTextContent("another host on this machine");
    // The attempt's own re-read has landed, and the refusal survived it.
    await waitFor(() => {
      expect(syncTasks).toHaveBeenCalledTimes(2);
    });
    expect(screen.getByTestId(TASKS_REFUSAL_TESTID)).toBeInTheDocument();

    // THE PROPERTY: a later read is newer evidence and clears it. The reachable
    // sequence is the holder's run finishing and the user pressing Refresh — the
    // alert must not outlive the listing that disproves it.
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
    await waitFor(() => {
      expect(screen.queryByTestId(TASKS_REFUSAL_TESTID)).not.toBeInTheDocument();
    });
  });

  it("renders two unknown rows with no readable id as two distinct rows", async () => {
    // Finding 10. `db::list_tasks` emits `UnknownTask { id: String::new(), … }`
    // for a row whose `id` column will not read, and `key={row.id}` gave React
    // two siblings keyed "" — a duplicate-key warning, and reconciliation free to
    // reuse one row's DOM for the other so the two distinct reasons swap or fail
    // to update. This is the one list that exists to tolerate malformed rows.
    answer(
      listing({
        tasks: [],
        unknown: [
          { id: "", reason: "unreadable task row: invalid kind 'teleport'" },
          { id: "", reason: "unreadable task row: invalid mode 'sublimated'" },
        ],
      }),
    );
    const warned = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(<TasksPane />);
      const rows = await screen.findAllByTestId(TASKS_UNKNOWN_ROW_TESTID);
      expect(rows).toHaveLength(2);
      expect(rows[0]).toHaveTextContent("invalid kind 'teleport'");
      expect(rows[1]).toHaveTextContent("invalid mode 'sublimated'");
      // And neither renders a blank where the id would be.
      for (const row of rows) {
        expect(within(row).getByText(TASKS_UNKNOWN_NO_ID_TEXT)).toBeInTheDocument();
      }
      expect(
        warned.mock.calls.flat().join(" "),
        "a duplicate React key is the defect, not a cosmetic warning",
      ).not.toMatch(/same key|duplicate key/i);
    } finally {
      warned.mockRestore();
    }
  });
});

/**
 * Story 58.1: the two commands nothing ever called. `sync_task_save` and
 * `sync_task_forget` were registered, typed, wrapped and mocked for a whole
 * wave, and every one of these assertions is a control that now reaches one.
 */
describe("the Tasks pane creates, changes and forgets a task", () => {
  it("reveals the add form inline in the pane rather than in a dialog", async () => {
    answer(listing({ tasks: [], unknown: [] }));
    render(<TasksPane />);
    await screen.findByText(TASKS_PANE_EMPTY_SENTENCE);

    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_TITLE }));

    expect(await screen.findByRole("form", { name: TASK_FORM_ADD_TITLE })).toBeInTheDocument();
    // AD-C7's idiom is a disclosure, and the reason is not taste: the same
    // component is revealed in two places, and a modal over a list of tasks
    // hides the rows whose settings the person is comparing this one against.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("re-reads the listing after a save, so the window and the host verdict move", async () => {
    answer(listing({ tasks: [], unknown: [] }));
    vi.mocked(syncTaskSave).mockResolvedValue(task());
    render(<TasksPane />);
    await screen.findByText(TASKS_PANE_EMPTY_SENTENCE);
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_TITLE }));
    await screen.findByRole("form", { name: TASK_FORM_ADD_TITLE });

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@daily" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(syncTaskSave).toHaveBeenCalledWith(
        expect.objectContaining({ id: "", schedule: "@daily" }),
      ),
    );
    // `nextDueMs`, both lease columns and the host verdict are the store's and
    // the engine's, never the request's, so the row can only be right after a
    // read. And the disclosure closes behind a save that actually happened.
    await waitFor(() => expect(syncTasks).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByRole("form", { name: TASK_FORM_ADD_TITLE })).not.toBeInTheDocument(),
    );
  });

  it("reveals a row's edit form seeded from the row already on screen", async () => {
    answer(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));

    // Named for the task it belongs to, because several rows can have one open.
    const form = await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });
    expect(within(form).getByLabelText(TASK_FORM_ID_LABEL)).toHaveValue("01SCHED");
    // Seeded from the listing that is already on screen: no second read of the
    // task record to open a form over a row it was just rendered from.
    expect(syncTasks).toHaveBeenCalledTimes(1);
  });

  it("offers neither Edit nor Forget on a row this build cannot read", async () => {
    // They are not `TaskVm`s — `db::list_tasks` could not decode them — so there
    // is nothing to seed a form from, and an upsert assembled out of a reason
    // string is one `sync_task_save` would refuse. A control that can only fail
    // is worse than no control.
    answer(
      listing({
        tasks: [],
        unknown: [{ id: "01FUTURE", reason: "unreadable task row: invalid kind 'teleport'" }],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);

    expect(within(row).queryByRole("button", { name: TASK_EDIT_TEXT })).not.toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: TASK_FORGET_TEXT })).not.toBeInTheDocument();
    expect(within(row).queryAllByRole("button")).toHaveLength(0);
  });

  it("asks before forgetting, and says the answer deletes a record and not content", async () => {
    answer(listing());
    vi.mocked(syncTaskForget).mockResolvedValue(undefined);
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_FORGET_TEXT }));

    const dialog = await screen.findByRole("alertdialog");
    // Which task, by the id the row shows: a list of ten of these all confirm
    // with the same words otherwise.
    expect(within(dialog).getByText(taskForgetConfirmTitle("01SCHED"))).toBeInTheDocument();
    // The one thing a person deciding this needs to know, in the backend's own
    // framing (`sync_ipc.rs`: "Deletes a record, never content").
    expect(within(dialog).getByTestId(TASK_FORGET_TESTID)).toHaveTextContent(
      "deletes a record, never content",
    );
    expect(TASK_FORGET_CONFIRM_BODY).toMatch(/never content/);
    // Asking is the point: nothing has happened yet.
    expect(syncTaskForget).not.toHaveBeenCalled();

    fireEvent.click(within(dialog).getByRole("button", { name: TASK_FORGET_CANCEL_TEXT }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(syncTaskForget).not.toHaveBeenCalled();
  });

  it("forgets the task on the confirm, and the row leaves the pane", async () => {
    // The one user-visible outcome of the whole path, and the earlier version of
    // this test answered the SAME listing to both reads — so it passed over an
    // implementation that deleted the wrong id, or whose re-read never reached
    // the rendered list.
    vi.mocked(syncTasks).mockResolvedValueOnce(listing());
    vi.mocked(syncTasks).mockResolvedValue(listing({ tasks: [], unknown: [] }));
    vi.mocked(syncTaskForget).mockResolvedValue(undefined);
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_FORGET_TEXT }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: TASK_FORGET_TEXT }));

    await waitFor(() => expect(syncTaskForget).toHaveBeenCalledWith("01SCHED"));
    await waitFor(() => expect(syncTasks).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByTestId(TASKS_ROW_TESTID)).not.toBeInTheDocument());
    expect(screen.getByText(TASKS_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });

  it("shows a refused Forget on the task it is about, as a refused Run now is", async () => {
    // An `internal` store error, which is what this path can actually emit:
    // `sync_task_forget` runs two unconditional DELETEs and has no
    // does-this-exist branch, so a "no such task" refusal was an invented
    // failure. Two tasks, so *which one* is the assertion and not decoration.
    answer(listing({ tasks: [task(), task({ id: "01OTHER" })] }));
    vi.mocked(syncTaskForget).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));
    // The SECOND task, deliberately: the first is the one selection defaults to,
    // so a refusal drawn on whatever happens to be open would pass anyway.
    selectRow("01OTHER");

    fireEvent.click(detailButton(TASK_FORGET_TEXT));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: TASK_FORGET_TEXT,
      }),
    );

    // In the region that is about it, and naming it.
    const region = await screen.findByTestId(TASKS_DETAIL_TESTID);
    expect(region).toHaveAttribute("data-task-id", "01OTHER");
    await waitFor(() =>
      expect(
        within(screen.getByTestId(TASKS_DETAIL_TESTID)).getByTestId(TASKS_REFUSAL_TESTID),
      ).toHaveTextContent("database is locked"),
    );
    // And the task is still there, because it was not deleted.
    expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2);

    // On no other. Choosing the untouched task must not carry the refusal
    // across — it is keyed by id, and the region is keyed by the task it draws.
    selectRow("01SCHED");
    expect(
      within(screen.getByTestId(TASKS_DETAIL_TESTID)).queryByTestId(TASKS_REFUSAL_TESTID),
    ).not.toBeInTheDocument();
  });

  it("promotes a refusal to the pane when its task is no longer the one on screen", async () => {
    // Story 59.1's own hole, and it did not exist before it: `refusals` is keyed
    // by task id and drawn by the ONE task the detail region holds, so a Run now
    // answered after the person has moved to another task had nowhere to be
    // drawn at all. A refused run would then look exactly like a successful one
    // — the invisible-failure shape this epic exists to close — so anything the
    // region is not showing is promoted to the pane's own alert.
    answer(listing({ tasks: [task(), task({ id: "01OTHER" })] }));
    let refuse = (): void => {};
    vi.mocked(syncTaskRunNow).mockImplementation(
      () =>
        new Promise<TaskRunVm>((_resolve, reject) => {
          refuse = () =>
            reject({
              code: "busy",
              message: "01OTHER is already running on dev#2",
              accountId: null,
              retriable: false,
            });
        }),
    );
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    selectRow("01OTHER");
    fireEvent.click(detailButton(TASK_RUN_NOW_TEXT));
    // Move away while the run is still in flight, which is the whole point.
    selectRow("01SCHED");
    refuse();

    const orphan = await screen.findByTestId(TASKS_ORPHAN_REFUSAL_TESTID);
    expect(orphan).toHaveTextContent("01OTHER");
    expect(orphan).toHaveTextContent("already running on dev#2");
    // And not silently duplicated onto the task that IS on screen.
    expect(
      within(screen.getByTestId(TASKS_DETAIL_TESTID)).queryByTestId(TASKS_REFUSAL_TESTID),
    ).not.toBeInTheDocument();
  });

  it("still reports a refused Forget when the row it belonged to has gone", async () => {
    // `refusals` is keyed by task id and drawn by the row, so a refusal for a
    // task the re-read no longer lists had nowhere to be drawn — and the
    // likeliest reason a Forget is refused is that another writer removed the row
    // first. A failed delete then looked exactly like a successful one, which is
    // the invisible-failure shape this epic exists to close.
    vi.mocked(syncTasks).mockResolvedValueOnce(listing());
    vi.mocked(syncTasks).mockResolvedValue(listing({ tasks: [], unknown: [] }));
    vi.mocked(syncTaskForget).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_FORGET_TEXT }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: TASK_FORGET_TEXT,
      }),
    );

    const orphan = await screen.findByTestId(TASKS_ORPHAN_REFUSAL_TESTID);
    expect(orphan).toHaveTextContent("database is locked");
    // Named, because the row that would have said which task is gone.
    expect(orphan).toHaveTextContent("01SCHED");
  });

  it("refuses to unmount a form, or delete its row, while its save is in flight", async () => {
    // Two defects with one flag. Pressing the disclosure mid-save unmounted the
    // form, so Rust's refusal had nowhere to land and a collapsed disclosure with
    // no message read as a save that happened. And a Forget confirmed mid-save
    // deletes a row the settling save re-inserts — `upsert_task` inserts when the
    // id is absent — so a confirmed deletion silently undoes itself.
    answer(listing());
    // A held promise, `files-pane.test.tsx`'s shape: the executor form and not
    // `Promise.withResolvers`, which this project's `lib` target does not have.
    let land: ((saved: TaskVm) => void) | null = null;
    vi.mocked(syncTaskSave).mockImplementation(
      () =>
        new Promise<TaskVm>((resolve) => {
          land = resolve;
        }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));
    const form = await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });

    fireEvent.click(within(form).getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    const live = screen.getByTestId(TASKS_DETAIL_TESTID);
    await waitFor(() =>
      expect(within(live).getByRole("button", { name: TASK_EDIT_TEXT })).toBeDisabled(),
    );
    expect(within(live).getByRole("button", { name: TASK_FORGET_TEXT })).toBeDisabled();
    expect(screen.getByRole("button", { name: TASK_FORM_ADD_TITLE })).toBeDisabled();
    expect(syncTaskForget).not.toHaveBeenCalled();

    // The save settles and every control comes back.
    await act(async () => {
      land?.(task());
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: TASK_FORM_ADD_TITLE })).toBeEnabled(),
    );
  });

  it("drops an edit disclosure whose row the record no longer has", async () => {
    // The id is user-supplied on the Add form, so a stale `editingId` is
    // re-creatable: forget `01SCHED`, add a new task called `01SCHED`, and the
    // new row rendered with its form already expanded and `aria-expanded` set on
    // a disclosure nobody had opened.
    vi.mocked(syncTasks).mockResolvedValueOnce(listing());
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));
    await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });

    // The record loses the task, then gains one with the same id again.
    vi.mocked(syncTasks).mockResolvedValueOnce(listing({ tasks: [], unknown: [] }));
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
    await waitFor(() =>
      expect(
        screen.queryByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` }),
      ).not.toBeInTheDocument(),
    );

    vi.mocked(syncTasks).mockResolvedValue(listing());
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
    const back = await screen.findByTestId(TASKS_DETAIL_TESTID);
    expect(within(back).getByRole("button", { name: TASK_EDIT_TEXT })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(
      screen.queryByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` }),
    ).not.toBeInTheDocument();
  });
});

/**
 * The row says what the run said (Story 58.2).
 *
 * One property, asserted ten ways: the sentence Rust composed for a run is on
 * the row in its own words, and where a run recorded nothing the row is silent
 * rather than blank or invented. Every assertion here is against the real string
 * a reader sees, never against the presence of an element — the data was already
 * typed, served and mocked for a whole wave, and it was the *reading* that was
 * missing, so an assertion that only proves a cell exists would prove nothing.
 *
 * Cross-crate facts are named by symbol rather than by line: `src-tauri/` is
 * being rewritten by the same wave that reads this file, so a line number here
 * is wrong within the day.
 */
describe("the row says what the last run reported", () => {
  it("shows the summary the run recorded, in the engine's own words", async () => {
    // The whole story: `perform_sync_task` has composed this sentence on every
    // completed run since wave 2, `TaskRunVm.detail` carried it to the frontend,
    // and no control in this pane read it.
    answer(
      listing({
        tasks: [
          task({ lastRun: run({ detail: "3 synced, 0 already syncing, 0 waiting, 0 failed" }) }),
        ],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).getByText(TASK_LAST_REPORT_LABEL)).toBeInTheDocument();
    expect(
      within(row).getByText("3 synced, 0 already syncing, 0 waiting, 0 failed"),
    ).toBeInTheDocument();
    // Beside keeper's verdict on the run, not instead of it.
    expect(within(row).getByText("Succeeded")).toBeInTheDocument();
  });

  it("keeps a failure's reason whole, reason included", async () => {
    // The reason is the actionable half — `perform_sync_task` wraps its counts
    // as `"{detail}: {reason}"` on failure — so truncating this cell would be
    // the one clipping on the row that actually costs the reader something.
    const detail =
      "0 synced, 0 already syncing, 0 waiting, 1 failed: could not resolve host git.tgorka.dev";
    answer(listing({ tasks: [task({ lastRun: run({ outcome: "failed", detail }) })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).getByText(detail)).toBeInTheDocument();
    expect(
      within(row).getByText("could not resolve host git.tgorka.dev", { exact: false }),
    ).toBeInTheDocument();
    expect(within(row).getByText("Failed")).toBeInTheDocument();
  });

  it("says nothing for a run still in flight, and lets the outcome cell explain", async () => {
    // `claim_task` opens the run row with `detail` unset, so an unfinished run
    // has genuinely reported nothing yet — and the cell beside this one already
    // accounts for the silence.
    answer(
      listing({
        tasks: [
          task({
            lastRun: run({ finishedMs: null, outcome: null, unknownOutcome: null, detail: null }),
          }),
        ],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).queryByText(TASK_LAST_REPORT_LABEL)).not.toBeInTheDocument();
    expect(within(row).getByText(TASK_IN_FLIGHT_TEXT)).toBeInTheDocument();
  });

  it("says nothing for a reclaimed lease, which is a real state and not a failed read", async () => {
    // Both `claim_task` and `release_host_leases` write `abandoned` without
    // touching `detail`, so an absent report here means the run was taken away
    // rather than that this pane failed to read one — and the outcome cell names
    // it.
    answer(listing({ tasks: [task({ lastRun: run({ outcome: "abandoned", detail: null }) })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).queryByText(TASK_LAST_REPORT_LABEL)).not.toBeInTheDocument();
    expect(within(row).getByText("Abandoned by the host that started it")).toBeInTheDocument();
  });

  it("adds no third copy of never run to a row that has not run", async () => {
    // The count itself belongs to the refusal test — *"shows a refused Run now
    // on the row, and no row claims the task ran"* — which asserts that a
    // never-ran row says the words exactly twice. What is asserted here is the
    // report cell's own half of that: a "nothing recorded" sentence in this cell
    // would have been the third copy.
    answer(listing({ tasks: [task({ lastRun: null })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).queryByText(TASK_LAST_REPORT_LABEL)).not.toBeInTheDocument();
    expect(within(row).queryByText(/nothing recorded|no report/i)).not.toBeInTheDocument();
  });

  it("keeps both a newer keeper's spelling and its report", async () => {
    // NFR-43 on both halves at once: an outcome this build cannot read renders
    // as itself, and the detail beside it is still the run's own words.
    answer(
      listing({
        tasks: [
          task({
            lastRun: run({
              outcome: null,
              unknownOutcome: "sublimated",
              detail: "recorded by keeper 0.9.0",
            }),
          }),
        ],
      }),
    );
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    expect(within(row).getByText("sublimated")).toBeInTheDocument();
    expect(within(row).getByText("recorded by keeper 0.9.0")).toBeInTheDocument();
    expect(within(row).queryByText(TASK_IN_FLIGHT_TEXT)).not.toBeInTheDocument();
  });

  it("draws no cell at all rather than an empty one when there is no report", async () => {
    // Counted by the label rather than by a total: the host block renders a
    // `<dt>` of its own outside the `<dl>`, so a row-wide `<dt>` total is one
    // higher than the grid's, and a magic total would also have to move every
    // time another story adds a cell to this row. Counting the cells whose
    // heading IS the report label says what the test is about and needs no
    // defensive note telling a later reader not to touch the numbers.
    const reportCells = (row: HTMLElement): number =>
      within(row).queryAllByText(TASK_LAST_REPORT_LABEL).length;

    answer(listing({ tasks: [task({ lastRun: run({ outcome: "abandoned", detail: null }) })] }));
    const silent = render(<TasksPane />);
    expect(reportCells(await screen.findByTestId(TASKS_DETAIL_TESTID))).toBe(0);
    silent.unmount();

    answer(listing({ tasks: [task()] }));
    render(<TasksPane />);
    expect(reportCells(await screen.findByTestId(TASKS_DETAIL_TESTID))).toBe(1);
  });

  it("stays silent for a stored report that is blank rather than absent", async () => {
    // `detail` is `TEXT NULL` with no non-empty constraint and `finish_task_run`
    // binds whatever it is handed, so a writer this build never met — the NFR-43
    // case this pane exists to tolerate — can store `""` or `"   "`. On a
    // `!== null` guard that renders a LAST REPORT heading over nothing, which is
    // the one shape a reader really would read as a failed read. Both spellings,
    // because `getByText` normalises whitespace and would not tell them apart.
    for (const detail of ["", "   "]) {
      answer(listing({ tasks: [task({ lastRun: run({ detail }) })] }));
      const view = render(<TasksPane />);
      const row = await screen.findByTestId(TASKS_DETAIL_TESTID);
      expect(within(row).queryByText(TASK_LAST_REPORT_LABEL), detail).not.toBeInTheDocument();
      // And the row still says everything else it said.
      expect(within(row).getByText("Succeeded")).toBeInTheDocument();
      view.unmount();
    }
  });

  it("gives the report the whole row rather than a quarter of it", async () => {
    // The only assertion in this file that reads a class, and deliberately: the
    // claim the `wide` prop makes is about layout, jsdom performs no layout, and
    // dropping `wide` at the callsite is a silent omission rather than a type
    // error — the prop defaults to `false`. Without this, half the change has no
    // test and a git error goes back to wrapping five lines in a quarter column.
    answer(listing({ tasks: [task({ lastRun: run({ detail: "3 synced" }) })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    const cell = within(row).getByText(TASK_LAST_REPORT_LABEL).parentElement;
    expect(cell).toHaveClass("col-span-2", "sm:col-span-4");
    // And the engine's own line breaks survive, which is what "verbatim" means
    // once HTML is involved.
    expect(within(row).getByText("3 synced")).toHaveClass("whitespace-pre-wrap");
  });

  it("draws the report on the row it belongs to and on no other", async () => {
    // Two rows, one loud and one silent, because every other test here renders a
    // single task: a cell drawn from the wrong row's run, or drawn on every row,
    // would pass all of them.
    answer(
      listing({
        tasks: [
          task({
            id: "A",
            lastRun: run({ detail: "3 synced, 0 already syncing, 0 waiting, 0 failed" }),
          }),
          task({ id: "B", lastRun: run({ outcome: "abandoned", detail: null }) }),
        ],
      }),
    );
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));
    const reportRegion = (id: string): HTMLElement => {
      selectRow(id);
      return screen.getByTestId(TASKS_DETAIL_TESTID);
    };

    expect(
      within(reportRegion("A")).getByText("3 synced, 0 already syncing, 0 waiting, 0 failed"),
    ).toBeInTheDocument();
    const silent = reportRegion("B");
    expect(within(silent).queryByText(TASK_LAST_REPORT_LABEL)).not.toBeInTheDocument();
    expect(
      within(silent).queryByText("3 synced, 0 already syncing, 0 waiting, 0 failed"),
    ).not.toBeInTheDocument();
  });

  it("keeps the report a row already had when a Run now is refused", async () => {
    // The row's stated invariant is that a refusal changes nothing else on it,
    // and the pane's own refusal test pins `lastRun: null` — so the fifth cell
    // was the one value that invariant was never asserted for.
    answer(listing({ tasks: [task({ lastRun: run({ detail: "3 synced, 0 waiting" }) })] }));
    vi.mocked(syncTaskRunNow).mockRejectedValue({
      code: "busy",
      message: "task 01SCHED is being run by another host on this machine",
      accountId: null,
      retriable: true,
    });
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    await screen.findByTestId(TASKS_REFUSAL_TESTID);

    const after = screen.getByTestId(TASKS_DETAIL_TESTID);
    expect(within(after).getByText("3 synced, 0 waiting")).toBeInTheDocument();
  });
});

/**
 * A list of runs you can open (Story 58.3).
 *
 * `db::task_runs`, `Engine::task_history` and `sync_task_history` were finished,
 * clamped, typed, wrapped and mocked for a whole wave while the only reference
 * to the wrapper under `src/` was the `vi.fn()` at the top of this file. So the
 * property under test is not that the data arrives — it always did — but that a
 * control reaches it, exactly once per deliberate press, and that the three
 * states of a read never borrow each other's words.
 *
 * The first assertion here is the one that keeps the section from becoming a
 * poll, and every other assertion is against a string a reader sees or an
 * argument the command was called with. Cross-crate facts are named by symbol
 * rather than by line: `src-tauri/` is edited by the same wave that reads this
 * file.
 */
describe("a task's runs open on the row, and are read only when asked for", () => {
  /**
   * The answer `sync_task_history` gives, newest first as `db::task_runs` orders
   * it.
   *
   * Timed from the real clock rather than from {@link NOW}, because the pane
   * measures every relative time from `Date.now()` at the instant the listing
   * landed — so a fixture pinned to a constant would render as years ago.
   */
  function runs(count: number, base: number = Date.now()): TaskRunVm[] {
    return Array.from({ length: count }, (_, index) =>
      run({
        id: count - index,
        startedMs: base - (index + 1) * 60_000,
        detail: `run ${count - index} of ${count}`,
      }),
    );
  }

  /**
   * The disclosure on one row. Its accessible name is its `aria-label` — the
   * word alone would name ten controls the same thing on a list of ten tasks.
   */
  function disclosure(id: string): HTMLElement {
    return screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: ${id}` });
  }
  /**
   * The fold's row counts are mutable module state, and one test below changes
   * them to reach a state the defaults cannot. Restored here so nothing after it
   * inherits a two-row fold.
   */
  afterEach(() => {
    setSyncListSizes({
      folded: SYNC_LIST_FOLDED_FALLBACK,
      unfolded: SYNC_LIST_UNFOLDED_FALLBACK,
    });
  });

  it("reads no history at all until somebody opens a section", async () => {
    // The property that keeps this section from becoming a poll: with the read
    // on render, or on the listing, every row of every refresh would cost an
    // IPC call and AD-62's sentence would be about this pane.
    answer(listing({ tasks: [task({ id: "01SCHED" }), task({ id: "01OTHER" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    expect(syncTaskHistory).not.toHaveBeenCalled();
    expect(screen.queryByTestId(TASKS_HISTORY_TESTID)).not.toBeInTheDocument();
  });

  it("reads that row's runs once when opened, and shows what each of them said", async () => {
    const base = Date.now();
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue([
      run({
        id: 7,
        host: "dev#1",
        startedMs: base - 5 * 60_000,
        detail: "3 synced, 0 already syncing, 0 waiting, 0 failed",
      }),
      run({
        id: 6,
        host: "laptop#2",
        outcome: "failed",
        startedMs: base - 65 * 60_000,
        detail: "0 synced, 1 failed: could not resolve host git.tgorka.dev",
      }),
    ]);
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(2));

    // One press, one call, with that row's id and no invented limit: the bound
    // is `TASK_HISTORY_LIMIT_DEFAULT`, in Rust, where it already is.
    expect(syncTaskHistory).toHaveBeenCalledTimes(1);
    expect(syncTaskHistory).toHaveBeenCalledWith("01SCHED");

    // The CLI's four columns, in the CLI's order, each asserted as the real
    // string rather than as an element that exists.
    const [newest, older] = screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID);
    expect(within(newest).getByText("Succeeded")).toBeInTheDocument();
    expect(within(newest).getByText("5 min ago")).toBeInTheDocument();
    expect(within(newest).getByText("dev#1")).toBeInTheDocument();
    expect(
      within(newest).getByText("3 synced, 0 already syncing, 0 waiting, 0 failed"),
    ).toBeInTheDocument();
    // Two runs with different reports, so a section that drew one run's words on
    // every row could not pass.
    expect(within(older).getByText("Failed")).toBeInTheDocument();
    expect(within(older).getByText("1 hr ago")).toBeInTheDocument();
    expect(within(older).getByText("laptop#2")).toBeInTheDocument();
    expect(
      within(older).getByText("0 synced, 1 failed: could not resolve host git.tgorka.dev"),
    ).toBeInTheDocument();
  });

  it("issues no second read when the clock ticks or the listing is re-read", async () => {
    // The anti-poll property, driven rather than asserted in prose: the display
    // clock and `refresh` are the two things that re-render an open section
    // without anybody pressing anything, and a history read on either of them
    // would be a poll per open row.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      answer(listing({ tasks: [task({ id: "01SCHED" })] }));
      vi.mocked(syncTaskHistory).mockResolvedValue(runs(1));
      render(<TasksPane />);
      await screen.findByTestId(TASKS_ROW_TESTID);

      fireEvent.click(disclosure("01SCHED"));
      await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(1));

      await vi.advanceTimersByTimeAsync(TASKS_CLOCK_TICK_MS * 4);
      fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
      await waitFor(() => expect(syncTasks).toHaveBeenCalledTimes(2));
      // `waitFor` returns when the listing call is MADE, not when it has settled
      // and re-rendered — and a regression that chained a history read onto that
      // continuation would still be a microtask away. So the count is read after
      // the settle has visibly landed and the timers have been flushed again.
      await waitFor(() => expect(screen.getByText("run 1 of 1")).toBeInTheDocument());
      await vi.advanceTimersByTimeAsync(TASKS_CLOCK_TICK_MS);

      expect(syncTaskHistory).toHaveBeenCalledTimes(1);
      // And the section is still open, still holding the rows it read.
      expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("closes the first section when another task is chosen, and reads once for it", async () => {
    answer(listing({ tasks: [task({ id: "01SCHED" }), task({ id: "01OTHER" })] }));
    vi.mocked(syncTaskHistory).mockImplementation(async (id) => [
      run({ id: 1, detail: `${id}'s own run` }),
    ]);
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText("01SCHED's own run")).toBeInTheDocument();

    // Story 59.1 turned "open another row's section" into "choose another
    // task", and the rule it is testing survived the move intact — in fact the
    // structure now enforces half of it, since only one task is drawn at all.
    // What is still worth asserting is that choosing does NOT read: the section
    // closes, and the second read happens only when Runs is pressed again.
    selectRow("01OTHER");
    expect(screen.queryByTestId(TASKS_HISTORY_TESTID)).not.toBeInTheDocument();
    expect(syncTaskHistory).toHaveBeenCalledTimes(1);

    fireEvent.click(disclosure("01OTHER"));
    expect(await screen.findByText("01OTHER's own run")).toBeInTheDocument();

    // One section open at a time, `editingId`'s rule: the first is gone rather
    // than scrolled apart from the task it belongs to.
    expect(screen.queryByText("01SCHED's own run")).not.toBeInTheDocument();
    expect(screen.getAllByTestId(TASKS_HISTORY_TESTID)).toHaveLength(1);
    expect(syncTaskHistory).toHaveBeenCalledTimes(2);
    expect(syncTaskHistory).toHaveBeenLastCalledWith("01OTHER");
  });

  it("cannot land a slow read in a section that has since moved to another row", async () => {
    // `historyToken`'s whole reason. Without it the first row's read resolves
    // into the one slot the second row is now drawing from, and a person reading
    // `01OTHER`'s history is shown `01SCHED`'s.
    let landFirst: (value: TaskRunVm[]) => void = () => {};
    answer(listing({ tasks: [task({ id: "01SCHED" }), task({ id: "01OTHER" })] }));
    vi.mocked(syncTaskHistory).mockImplementation((id) =>
      id === "01SCHED"
        ? new Promise<TaskRunVm[]>((resolve) => {
            landFirst = resolve;
          })
        : Promise.resolve([run({ id: 2, detail: "01OTHER's own run" })]),
    );
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText(TASK_HISTORY_LOADING_TEXT)).toBeInTheDocument();

    selectRow("01OTHER");
    fireEvent.click(disclosure("01OTHER"));
    expect(await screen.findByText("01OTHER's own run")).toBeInTheDocument();

    await act(async () => {
      landFirst([run({ id: 1, detail: "01SCHED's own run" })]);
    });

    expect(screen.getByText("01OTHER's own run")).toBeInTheDocument();
    expect(screen.queryByText("01SCHED's own run")).not.toBeInTheDocument();
  });

  it("re-reads when a section is closed and opened again", async () => {
    // Closing forgets, so re-opening is a re-read rather than a cache hit — the
    // list `task_runs` may have trimmed underneath it is not what gets shown.
    let landSecond: (value: TaskRunVm[]) => void = () => {};
    let calls = 0;
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockImplementation(() => {
      calls += 1;
      return calls === 1
        ? Promise.resolve([run({ id: 1, detail: "the first read" })])
        : new Promise<TaskRunVm[]>((resolve) => {
            landSecond = resolve;
          });
    });
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText("the first read")).toBeInTheDocument();

    fireEvent.click(disclosure("01SCHED"));
    expect(screen.queryByTestId(TASKS_HISTORY_TESTID)).not.toBeInTheDocument();

    fireEvent.click(disclosure("01SCHED"));
    // The loading line again, and not the list it had: an unread section says so.
    expect(await screen.findByText(TASK_HISTORY_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.queryByText("the first read")).not.toBeInTheDocument();
    expect(syncTaskHistory).toHaveBeenCalledTimes(2);

    await act(async () => {
      landSecond([run({ id: 2, detail: "the second read" })]);
    });
    expect(screen.getByText("the second read")).toBeInTheDocument();
  });

  it("says no runs recorded for an empty history, and never the loading line", async () => {
    // `[]` is a fact and `null` is not: the CLI answers this case with
    // `"{task_id}: no runs recorded"` and so does this section.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue([]);
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText(TASK_HISTORY_EMPTY_TEXT)).toBeInTheDocument();
    expect(screen.queryByText(TASK_HISTORY_LOADING_TEXT)).not.toBeInTheDocument();
    expect(screen.queryAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(0);
  });

  it("quotes a refused read and never claims the task has no runs", async () => {
    // A failed read is a fault to report, not a fact to invent — and the
    // rejection is an `IpcError` *value*, so `messageOf` rather than
    // `instanceof Error` is what keeps this from rendering "[object Object]".
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    const refusal = await screen.findByTestId(TASKS_HISTORY_REFUSAL_TESTID);

    expect(refusal).toHaveTextContent("database is locked");
    expect(screen.queryByText(TASK_HISTORY_EMPTY_TEXT)).not.toBeInTheDocument();
    expect(screen.queryByText(TASK_HISTORY_LOADING_TEXT)).not.toBeInTheDocument();
  });

  it("keeps the runs it already had when a re-read is refused", async () => {
    // `keepRows`: the rows on screen were read successfully and are still the
    // best thing known about this task, so a refusal is added to them rather
    // than substituted for them.
    let calls = 0;
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskRunNow).mockResolvedValue(run());
    vi.mocked(syncTaskHistory).mockImplementation(() => {
      calls += 1;
      return calls === 1
        ? Promise.resolve([run({ id: 1, detail: "the read that worked" })])
        : Promise.reject({
            code: "internal",
            message: "database is locked",
            accountId: null,
            retriable: false,
          });
    });
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText("the read that worked")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    const refusal = await screen.findByTestId(TASKS_HISTORY_REFUSAL_TESTID);

    expect(refusal).toHaveTextContent("database is locked");
    expect(screen.getByText("the read that worked")).toBeInTheDocument();
    expect(screen.queryByText(TASK_HISTORY_EMPTY_TEXT)).not.toBeInTheDocument();
  });

  it("re-reads the open row's history once after a Run now on that row", async () => {
    // The one re-read a refresh does not do, and the reason it is not a poll:
    // this run is what changed that task's history, and the person pressed the
    // button themselves.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskRunNow).mockResolvedValue(run());
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(1));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() => expect(syncTaskHistory).toHaveBeenCalledTimes(1));

    fireEvent.click(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    // Two settles, not one: `waitFor` resolves the moment the count REACHES two,
    // so a regression that read twice per Run now would slip past. The Run now
    // button coming back enabled is the pane's own evidence that the whole
    // settle — the run, the listing re-read and the history re-read — has run to
    // completion.
    await waitFor(() => expect(syncTaskHistory).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeEnabled(),
    );

    expect(vi.mocked(syncTaskHistory).mock.calls.map(([id]) => id)).toEqual(["01SCHED", "01SCHED"]);
    expect(screen.getByText("run 1 of 1")).toBeInTheDocument();

    // And a Run now on a row whose section is NOT open reads no history at all.
    fireEvent.click(disclosure("01SCHED"));
    fireEvent.click(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeEnabled(),
    );
    expect(syncTaskHistory).toHaveBeenCalledTimes(2);
  });

  it("closes a section whose row the listing no longer holds", async () => {
    // `refresh`'s `editingId` pruning, verbatim: a section cannot belong to a
    // row the record does not have, and the id is re-creatable by hand.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(1));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await screen.findByTestId(TASKS_HISTORY_TESTID);

    answer(listing({ tasks: [task({ id: "01OTHER" })] }));
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));

    await waitFor(() => expect(screen.queryByTestId(TASKS_HISTORY_TESTID)).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: 01OTHER` })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("offers no runs disclosure on a row this build cannot read", async () => {
    // Not a `TaskVm`, and its id may be `""` — so `sync_task_history` has
    // nothing to be asked about. The 58.3-specific claim is the named control's
    // absence; the blanket no-buttons assertion is the pane's own, one block up.
    answer(
      listing({
        tasks: [],
        unknown: [{ id: "01FUTURE", reason: "unreadable task row: invalid kind 'teleport'" }],
      }),
    );
    render(<TasksPane />);
    await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);

    expect(
      screen.queryByRole("button", { name: `${TASK_HISTORY_TITLE}: 01FUTURE` }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: new RegExp(`^${TASK_HISTORY_TITLE}:`) }),
    ).toBeNull();
  });

  it("folds a long history to the folded size and unfolds it on one press", async () => {
    // The shared fold rather than a third list idiom, and the control is named
    // for its own list so ten rows do not offer ten controls a screen reader
    // calls the same thing. The expected count is READ from the preference
    // rather than written as 10: `syncListSizes()` is mutable module state, so a
    // literal here would fail one day with a row count that looks unrelated to
    // the fold.
    const { folded } = syncListSizes();
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(20));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() =>
      expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(folded),
    );
    expect(screen.getByText("run 20 of 20")).toBeInTheDocument();
    expect(screen.queryByText("run 1 of 20")).not.toBeInTheDocument();

    const unfold = screen.getByRole("button", {
      name: `${LIST_FOLD_MORE_LABEL(20)}: ${TASK_HISTORY_TITLE}: 01SCHED`,
    });
    fireEvent.click(unfold);

    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(20));
    expect(screen.getByText("run 1 of 20")).toBeInTheDocument();
    // Everything read is now on screen, so nothing is being held back.
    expect(screen.queryByText(/more recorded and not shown/)).not.toBeInTheDocument();
  });

  it("counts the runs an unfolded list still holds back", async () => {
    // The fold's unfolded size is a global preference with a floor of ten while
    // a history page is twenty runs, so *Show all* can leave half the list
    // hidden with `FoldToggle` saying only "Show fewer" — a reader who pressed
    // it would believe they had seen everything.
    setSyncListSizes({ folded: 2, unfolded: 3 });
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(5));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(2));

    fireEvent.click(
      screen.getByRole("button", {
        name: `${LIST_FOLD_MORE_LABEL(3)}: ${TASK_HISTORY_TITLE}: 01SCHED`,
      }),
    );
    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(3));

    expect(screen.getByText(taskHistoryUnshownText(2))).toBeInTheDocument();
  });

  it("drops the empty sentence once a refusal explains why it cannot know", async () => {
    // The sharpest of the three-states rules. A task with no runs reads `[]`,
    // then a Run now writes one — and if that re-read is refused, a section
    // still saying "no runs recorded" beside "database is locked" states as a
    // fact the thing the refusal has just said it cannot know.
    let calls = 0;
    answer(listing({ tasks: [task({ id: "01SCHED", lastRun: null })] }));
    vi.mocked(syncTaskRunNow).mockResolvedValue(run());
    vi.mocked(syncTaskHistory).mockImplementation(() => {
      calls += 1;
      return calls === 1
        ? Promise.resolve([])
        : Promise.reject({
            code: "internal",
            message: "database is locked",
            accountId: null,
            retriable: false,
          });
    });
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText(TASK_HISTORY_EMPTY_TEXT)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    await screen.findByTestId(TASKS_HISTORY_REFUSAL_TESTID);

    expect(screen.queryByText(TASK_HISTORY_EMPTY_TEXT)).not.toBeInTheDocument();
  });

  it("says how to ask again, because nothing re-reads a refusal on its own", async () => {
    // Without this the refusal is a dead end: a listing refresh deliberately
    // leaves the section alone, so the only retry is closing the disclosure and
    // opening it — which the one obvious press looks like a dismissal of.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    const refusal = await screen.findByTestId(TASKS_HISTORY_REFUSAL_TESTID);
    expect(refusal).toHaveTextContent(TASK_HISTORY_RETRY_NOTE);

    // And that retry genuinely works: close, open, and the command is asked
    // again rather than the section restoring what it had.
    fireEvent.click(disclosure("01SCHED"));
    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() => expect(syncTaskHistory).toHaveBeenCalledTimes(2));
  });

  it("names a run whose host or report the record left blank", async () => {
    // `host` is `TEXT NOT NULL` with no non-empty constraint and `detail` is
    // nullable, so the same foreign-writer class `taskReportText` exists for can
    // leave either blank. Which host ran it is most of the point of this list, so
    // a blank there is named; a blank report is silence.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    // Timed from the real clock, because the pane measures every relative time
    // against `Date.now()` at the instant the listing landed.
    const started = Date.now() - 5 * 60_000;
    vi.mocked(syncTaskHistory).mockResolvedValue([
      run({ id: 2, startedMs: started, host: "   ", detail: "   " }),
      run({ id: 1, startedMs: started, host: "dev#1", detail: "3 synced" }),
    ]);
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(2));
    const [blank, named] = screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID);

    expect(within(blank).getByText(TASK_HISTORY_NO_HOST_TEXT)).toBeInTheDocument();
    // The blank report draws no cell at all: the row ends at its host, so its
    // whole text is the outcome, the time and the stand-in for the host.
    expect(blank.textContent).toBe(`Succeeded5 min ago${TASK_HISTORY_NO_HOST_TEXT}`);
    // And the row beside it is unaffected.
    expect(within(named).getByText("dev#1")).toBeInTheDocument();
    expect(within(named).getByText("3 synced")).toBeInTheDocument();
  });

  it("names an outcome whose stored spelling is blank rather than rendering nothing", async () => {
    // A spelling this build cannot read is rendered verbatim (NFR-43) — but `""`
    // renders as nothing, and this is the leading word of the row. Falling
    // through would be worse still: the next branch would call a closed run
    // "running now".
    expect(taskOutcomeText(run({ outcome: null, unknownOutcome: "  " }))).toBe(
      TASK_UNREADABLE_OUTCOME_TEXT,
    );
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue([
      run({ id: 1, outcome: null, unknownOutcome: "" }),
    ]);
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    const entry = await screen.findByTestId(TASKS_HISTORY_ROW_TESTID);
    expect(within(entry).getByText(TASK_UNREADABLE_OUTCOME_TEXT)).toBeInTheDocument();
    expect(within(entry).queryByText(TASK_IN_FLIGHT_TEXT)).not.toBeInTheDocument();
  });

  it("names the region it opens, so the control is not a dead end for a screen reader", async () => {
    // `aria-expanded` alone announces "collapsed" and offers nothing to jump to.
    // The IDREF exists only while the section does (`note-editor.tsx`'s form), so
    // it can never dangle.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(1));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    expect(disclosure("01SCHED")).not.toHaveAttribute("aria-controls");
    fireEvent.click(disclosure("01SCHED"));
    const section = await screen.findByTestId(TASKS_HISTORY_TESTID);

    expect(disclosure("01SCHED")).toHaveAttribute("aria-controls", section.id);
    expect(section.id).not.toBe("");
  });

  it("keeps one of the runs and the edit form open on a row, never both", async () => {
    // The one-at-a-time argument this section borrows from `editingId` is about
    // height: a twenty-run list plus an eight-control form is exactly the wall
    // that argument exists to forbid.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runs(1));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(disclosure("01SCHED"));
    await screen.findByTestId(TASKS_HISTORY_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));
    await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });
    expect(screen.queryByTestId(TASKS_HISTORY_TESTID)).not.toBeInTheDocument();

    fireEvent.click(disclosure("01SCHED"));
    await screen.findByTestId(TASKS_HISTORY_TESTID);
    expect(
      screen.queryByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` }),
    ).not.toBeInTheDocument();
  });

  it("refuses to open the runs while a save is on its way to that record", async () => {
    // The guard Edit and Forget already carry, for this control's own reason:
    // opening the runs closes the edit form, so pressing this mid-save would
    // unmount the form Rust's refusal has to land in.
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskSave).mockImplementation(() => new Promise<TaskVm>(() => {}));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_DETAIL_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));
    const form = await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });
    fireEvent.click(within(form).getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(disclosure("01SCHED")).toBeDisabled());
    fireEvent.click(disclosure("01SCHED"));
    expect(syncTaskHistory).not.toHaveBeenCalled();
    expect(
      screen.getByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` }),
    ).toBeInTheDocument();
  });
});

describe("the pane also lists what this host paces, and says it is not a task", () => {
  it("states, per projected row, its kind, its folder, its cadence and Rust's sentence", async () => {
    answer(listing());
    answerPaced([pacedRow()]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(row).toHaveAttribute("data-paced-id", "scan:p1");
    expect(within(row).getByText(PACED_BADGE)).toBeInTheDocument();
    expect(within(row).getByText(PACED_KIND_LABELS.scan)).toBeInTheDocument();
    expect(within(row).getByText(PACED_FOLDER_LABEL)).toBeInTheDocument();
    expect(within(row).getByText("keeper")).toBeInTheDocument();
    expect(within(row).getByText(PACED_CADENCE_LABEL)).toBeInTheDocument();
    expect(within(row).getByText("about every 15 seconds")).toBeInTheDocument();
    // Rust's, verbatim — the two filesystem triggers included, which is the one
    // thing the interval alone would be read as denying.
    expect(within(row).getByText(PACED_SENTENCE_SCAN)).toBeInTheDocument();
  });

  it("renders a kind this build does not know as its own spelling", async () => {
    // `HOST_KIND_LABELS`'s rule, applied to the class that grows next:
    // `PacedWorkKind` can gain a variant in Rust, and a blank label would hide
    // a row this build was still handed.
    answer(listing());
    answerPaced([
      pacedRow({
        id: "teleport:p1",
        // A spelling only a newer keeper writes. Cast because the union in the
        // generated binding is this build's, which is exactly the drift the
        // fallback exists for.
        kind: "teleportSweep" as PacedWorkVm["kind"],
      }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);
    expect(within(row).getByText("teleportSweep")).toBeInTheDocument();
  });

  it("offers no control of any kind on a projected row, and says so in words first", async () => {
    // The absence, asserted in BOTH shapes. By name, because those four are the
    // controls a reader of the list above will look for; and by role over the
    // whole row, because a fifth control added later would pass the by-name
    // assertions while putting a button on a row nothing can run.
    //
    // Not a disabled button either: a disabled control says *not now* and the
    // truth is *not ever*.
    answer(listing());
    answerPaced([pacedRow()]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).queryAllByRole("button")).toEqual([]);
    for (const name of [TASK_RUN_NOW_TEXT, TASK_EDIT_TEXT, TASK_FORGET_TEXT, TASK_HISTORY_TITLE]) {
      expect(within(row).queryByRole("button", { name })).toBeNull();
    }
    // And the absence is EXPLAINED rather than merely present: a reader who
    // knows a Sync task can be run on demand would otherwise read a row with no
    // Run now as a row whose Run now failed to render.
    expect(screen.getByText(PACED_SUBTITLE)).toBeInTheDocument();
  });

  it("does not re-read the projection when the pane's display clock ticks", async () => {
    // AD-142: the projection registers no clock and is not polled. It rides the
    // pane's existing read, so the tick that moves every relative time on screen
    // must not cost a second call.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      answer(listing());
      answerPaced([pacedRow()]);
      render(<TasksPane />);
      await screen.findByTestId(PACED_ROW_TESTID);

      for (let tick = 0; tick < 3; tick += 1) {
        await vi.advanceTimersByTimeAsync(TASKS_CLOCK_TICK_MS);
      }

      expect(syncPacedWork).toHaveBeenCalledTimes(1);
      // The listing is not re-read either: one pass read both.
      expect(syncTasks).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId(PACED_ROW_TESTID)).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("says a paused folder is paused, and shows no cadence beside it", async () => {
    // The invariant is enforced in Rust — `cadence` is `Some` only for `paced` —
    // so what this asserts is that the view does not invent one to fill the cell.
    answer(listing());
    answerPaced([
      pacedRow({
        id: "sweep:p3",
        kind: "scratchSweep",
        profile: "archive",
        profileId: "p3",
        standing: "paused",
        cadence: null,
        sentence: PACED_SENTENCE_PAUSED,
      }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).getByText(PACED_SENTENCE_PAUSED)).toBeInTheDocument();
    expect(within(row).getByText(PACED_NO_CADENCE_TEXT)).toBeInTheDocument();
    expect(within(row).queryByText(/about every/)).toBeNull();
    // The badge carries the standing, so it cannot contradict the sentence on
    // its own row. Found by rendering the section, not by a test: *Paced* sat
    // one line above "nothing here is paced" and each half was correct alone.
    expect(within(row).getByText(PACED_STANDING_LABELS.paused)).toBeInTheDocument();
    expect(within(row).queryByText(PACED_BADGE)).toBeNull();
  });

  // NOT a contract test on Story 58.8, though it was labelled one. This test
  // hands `syncPacedWork` a hardcoded governed row at the IPC boundary, so it
  // exercises no Rust: reverting 58.8's stand-down leaves it green. What it does
  // assert is this pane's half of the contract — that a governed row renders its
  // sentence and invents no cadence to fill the empty column.
  //
  // The guard that would actually fail on such a revert lives where the decision
  // does: `sync_poll_permits`' `Some(Scheduled) => false` arm and its tests in
  // `keeper-sync`, and the projection's governed-row tests in `keeper-core`. A
  // comment claiming coverage that a mock cannot provide is worse than no
  // comment, because it is read as a reason not to write the real one.
  it("says a governed scan has stood down, and advertises no cadence for it", async () => {
    answer(listing());
    answerPaced([
      pacedRow({ standing: "governed", cadence: null, sentence: PACED_SENTENCE_GOVERNED }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).getByText(PACED_SENTENCE_GOVERNED)).toBeInTheDocument();
    expect(within(row).getByText(PACED_NO_CADENCE_TEXT)).toBeInTheDocument();
    // The event triggers survive governance: what stood down is the interval.
    expect(within(row).getByText(/watcher sees settle still brings a look forward/)).toBeVisible();
  });

  // The half of the 58.8 contract this pane CAN hold on its own, and the reason
  // it is worth holding twice.
  //
  // `keeper_core::tasks::paced_work` pairs `cadence` with `standing` and asserts
  // it — with a `debug_assert!`, which is compiled out of the build a person
  // runs. So the invariant is proven in `cargo test` and unenforced in the app.
  // This feeds the pane the contradiction that assert would have caught and
  // requires the pane to refuse it: a row that has stood its backstop down must
  // not go on advertising the interval that no longer fires, whatever arrives on
  // the wire.
  //
  // Reachable in exactly the way that matters. #303 added a SECOND gate over
  // walking a folder — `poll_may_walk` / `POLL_WALK_MIN_INTERVAL` — after both
  // 58.7 and 58.8 shipped, so this projection is now describing a world with two
  // gates while it was written against one. `sync_poll_permits` stands the paced
  // scan down and deliberately leaves the Pending poll's walk alone; the day
  // somebody widens or reverts that decision, the adapter can start sending a
  // cadence with a governed standing, and this is where the screen refuses to
  // print it.
  it("prints no cadence for a stood-down row even when one arrives on the wire", async () => {
    answer(listing());
    answerPaced([
      pacedRow({
        standing: "governed",
        // The contradiction: a cadence beside a standing that says nothing is
        // pacing this folder. Rust will not build one; the wire could.
        cadence: "about every 15 seconds",
        sentence: PACED_SENTENCE_GOVERNED,
      }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).queryByText("about every 15 seconds")).toBeNull();
    expect(within(row).getByText(PACED_NO_CADENCE_TEXT)).toBeInTheDocument();
    // And it still says WHY, in Rust's words, so the empty cell is explained
    // rather than merely blank.
    expect(within(row).getByText(PACED_SENTENCE_GOVERNED)).toBeInTheDocument();
  });

  it("prints no cadence for a paused folder even when one arrives on the wire", async () => {
    // The same refusal for the standing a person meets far more often. *Paused,
    // about every 15 seconds* is the exact phrase Story 58.7's review found on
    // screen once already — it is what made the badge carry the standing — and
    // it must be unrenderable rather than merely unsent.
    answer(listing());
    answerPaced([
      pacedRow({
        standing: "paused",
        cadence: "about every 15 seconds",
        sentence: PACED_SENTENCE_PAUSED,
      }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).queryByText("about every 15 seconds")).toBeNull();
    expect(within(row).getByText(PACED_NO_CADENCE_TEXT)).toBeInTheDocument();
  });

  // The over-claim this row's registration fact exists to prevent: a folder can
  // hold a vault keeper has nothing registered to pace, and the cadence cell
  // must not recite an interval nobody is keeping.
  it("says an unregistered vault is not paced, and names the registry rather than a cadence", async () => {
    answer(listing());
    answerPaced([
      pacedRow({
        id: "notes:p1",
        kind: "notesCadence",
        standing: "unregistered",
        cadence: null,
        sentence: PACED_SENTENCE_UNREGISTERED,
      }),
    ]);
    render(<TasksPane />);
    const row = await screen.findByTestId(PACED_ROW_TESTID);

    expect(within(row).getByText(PACED_SENTENCE_UNREGISTERED)).toBeInTheDocument();
    expect(within(row).getByText(PACED_NO_CADENCE_TEXT)).toBeInTheDocument();
    expect(within(row).queryByText(/committed after/)).toBeNull();
    // And it is still not a task: no control appeared with the new standing.
    expect(within(row).queryAllByRole("button")).toHaveLength(0);
  });

  // The section's claim is completeness, so the fold may hide rows only while it
  // is collapsed. Every other folding list in the app is capped by the query
  // behind it; this one has no query, so a cap on the expanded view would drop
  // rows with no control left to reveal them.
  it("unfolds to every projected row rather than to the global unfolded size", async () => {
    setSyncListSizes({ folded: 2, unfolded: 3 });
    answer(listing());
    const rows = Array.from({ length: 7 }, (_unused, index) =>
      pacedRow({ id: `scan:p${index}`, profileId: `p${index}`, profile: `folder-${index}` }),
    );
    answerPaced(rows);
    render(<TasksPane />);
    await screen.findAllByTestId(PACED_ROW_TESTID);

    expect(screen.getAllByTestId(PACED_ROW_TESTID)).toHaveLength(2);
    // The control promises the list's own length, not the setting's.
    fireEvent.click(
      screen.getByRole("button", {
        name: `${LIST_FOLD_MORE_LABEL(7)}: ${PACED_HEADING}`,
      }),
    );
    await waitFor(() => expect(screen.getAllByTestId(PACED_ROW_TESTID)).toHaveLength(7));

    expect(screen.getByText("folder-6")).toBeVisible();
  });

  it("quotes a refused projection and leaves the task rows above it standing", async () => {
    // `Promise.allSettled` is what makes this true: `all` would reject on the
    // first failure and throw the listing away with it. A failed read is a fault
    // to report, not a fact to invent — and never a reason to blank a list that
    // read successfully.
    answer(listing());
    vi.mocked(syncPacedWork).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);

    const refusal = await screen.findByTestId(PACED_REFUSAL_TESTID);
    expect(refusal).toHaveTextContent("database is locked");
    expect(refusal).toHaveAttribute("role", "alert");
    // The section's own sentences yield to it: neither may claim anything about
    // a machine whose projection did not read.
    expect(screen.queryByText(PACED_EMPTY_TEXT)).toBeNull();
    expect(screen.queryByText(PACED_LOADING_TEXT)).toBeNull();
    // And the task list is untouched.
    const row = screen.getByTestId(TASKS_DETAIL_TESTID);
    expect(within(row).getByText("01SCHED")).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeInTheDocument();
    expect(screen.queryByTestId(TASKS_ERROR_TESTID)).toBeNull();
  });

  it("says unread and says empty in different words, and never both", async () => {
    // `null` is unread and `[]` is "keeper paces nothing here". A single
    // falsy check would have said one of those two things about the other.
    answer(listing());
    // Never resolves: the section stays at `null` for the whole of this half.
    vi.mocked(syncPacedWork).mockImplementation(() => new Promise<PacedWorkVm[]>(() => {}));
    const unread = render(<TasksPane />);
    expect(await screen.findByText(PACED_LOADING_TEXT)).toBeInTheDocument();
    expect(screen.queryByText(PACED_EMPTY_TEXT)).toBeNull();
    unread.unmount();

    answerPaced([]);
    render(<TasksPane />);
    expect(await screen.findByText(PACED_EMPTY_TEXT)).toBeInTheDocument();
    expect(screen.queryByText(PACED_LOADING_TEXT)).toBeNull();
    expect(screen.queryAllByTestId(PACED_ROW_TESTID)).toEqual([]);
  });

  it("lists every projected row for a folder, and the sweep reads the engine's hour", async () => {
    answer(listing());
    answerPaced([
      pacedRow(),
      pacedRow({
        id: "sweep:p1",
        kind: "scratchSweep",
        cadence: "every 1 hour",
        sentence: PACED_SENTENCE_SWEEP,
      }),
    ]);
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(PACED_ROW_TESTID)).toHaveLength(2));

    const sweep = screen
      .getAllByTestId(PACED_ROW_TESTID)
      .find((candidate) => candidate.getAttribute("data-paced-id") === "sweep:p1");
    expect(sweep).toBeDefined();
    expect(within(sweep as HTMLElement).getByText(PACED_KIND_LABELS.scratchSweep)).toBeVisible();
    expect(within(sweep as HTMLElement).getByText("every 1 hour")).toBeVisible();
    expect(within(sweep as HTMLElement).getByText(PACED_SENTENCE_SWEEP)).toBeVisible();
  });
});

// ---------------------------------------------------------------------------
// Epic 59 — the facts the pane was silent about
// ---------------------------------------------------------------------------

/**
 * A run list of a given length, newest first.
 *
 * A second, top-level copy of the one nested inside the history describe above,
 * because these tests live outside that block and a helper reached across a
 * describe boundary is a helper two suites can silently disagree about. Ids
 * descend so the order matches what `task_runs … ORDER BY id DESC` hands over.
 */
function runList(count: number): TaskRunVm[] {
  return Array.from({ length: count }, (_unused, index) =>
    run({ id: count - index, startedMs: NOW - (index + 1) * 60_000 }),
  );
}

describe("the row says enough to act on", () => {
  it("states the mode, so a scheduled row says it is scheduled", async () => {
    // The owner asked for a Run now on scheduled tasks that has always been
    // there. Half of why he could not tell: the row never rendered `mode` at
    // all, so a scheduled row's only visible schedule facts were a cron string
    // and a next-due time.
    answer(listing({ tasks: [task({ id: "01SCHED", mode: "scheduled" })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    expect(within(row).getByText("scheduled")).toBeInTheDocument();
    // The stored spelling, not a prettified one: `tasks list --json` prints
    // this vocabulary and two words for one stored value is the drift AD-C7
    // forbids.
    expect(within(row).queryByText("Scheduled")).toBeNull();
  });

  it("says what Run now does, once, and only when there is a row to do it to", async () => {
    answer(listing({ tasks: [] }));
    render(<TasksPane />);
    await screen.findByText(TASKS_PANE_EMPTY_SENTENCE);
    // A sentence about a button nobody can see yet is noise.
    expect(screen.queryByText(TASKS_RUN_NOW_SENTENCE)).toBeNull();

    answer(listing());
    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));

    expect(await screen.findByText(TASKS_RUN_NOW_SENTENCE)).toBeVisible();
    // The two halves that are worth stating, and both are in it: the window is
    // not consulted, and the schedule does not move.
    expect(TASKS_RUN_NOW_SENTENCE).toMatch(/whether or not a window is open/);
    expect(TASKS_RUN_NOW_SENTENCE).toMatch(/does not move the schedule/);
  });

  it("shows a task's own words when it has any, and nothing at all when it does not", async () => {
    // `taskDescriptionText`'s rule, which is `taskReportText`'s: blank and
    // absent are one rendered state — nothing — while the store keeps them
    // apart. A heading over an empty string reads as a failed read.
    answer(
      listing({
        tasks: [
          task({ id: "01NAMED", description: "the photos, nightly" }),
          task({ id: "01BLANK", description: "   " }),
          task({ id: "01NONE", description: null }),
        ],
      }),
    );
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(3));

    // Story 59.1 re-sited the description under the name in the detail region —
    // it is a sentence about the task rather than a scannable cell, and a 320px
    // line cannot hold one. Each task is therefore chosen in turn, which is also
    // what makes this three assertions about three tasks rather than three reads
    // of whatever happened to be open.
    const descriptionOf = (id: string): HTMLElement | null => {
      selectRow(id);
      return within(screen.getByTestId(TASKS_DETAIL_TESTID)).queryByTestId(
        TASKS_DESCRIPTION_TESTID,
      );
    };

    expect(descriptionOf("01NAMED")).toHaveTextContent("the photos, nightly");
    // Asserted on the ELEMENT, not on its text. A text query cannot tell a
    // paragraph that was never rendered from one rendered around three spaces,
    // and that is the whole distinction here: mutating `taskDescriptionText` to
    // return its argument unchanged left a text-based version of this test
    // green, which made it a test that could not fail.
    expect(descriptionOf("01BLANK")).toBeNull();
    expect(descriptionOf("01NONE")).toBeNull();
  });
});

describe("the runs control reads as a control", () => {
  it("says a task has never run before anything is opened, and counts only what it holds", async () => {
    // The count is the affordance that costs nothing — but ONLY once the
    // section is open. A closed section has read nothing, so a number on it
    // could only be guessed, and a guessed total that looks real is what
    // `count-label.ts` exists to prevent.
    // Stated rather than inherited: the fold is a module-level preference, so a
    // sibling test that lowered it to two leaks into this one and the section
    // renders two of the three runs it holds. A test whose subject is a COUNT
    // must own every number that can change it.
    setSyncListSizes({ folded: 10, unfolded: 100 });
    answer(listing({ tasks: [task({ id: "01SCHED", lastRun: null })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runList(3));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    const trigger = screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: 01SCHED` });
    expect(trigger).toHaveTextContent(`${TASK_HISTORY_TITLE} — none yet`);
    expect(trigger).not.toHaveTextContent(/\d/);

    fireEvent.click(trigger);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_HISTORY_ROW_TESTID)).toHaveLength(3));

    // Now it may count, because now it has something to count.
    expect(
      screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: 01SCHED` }),
    ).toHaveTextContent("3 runs");
  });

  it("says the history is a page of a longer record, and only when it is full", async () => {
    // Three numbers were invisible at once: the read asks for twenty, the store
    // keeps fifty, and the fold shows ten first. A reader who pressed Show all
    // had reached the end of neither.
    setSyncListSizes({ folded: 5, unfolded: 25 });
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runList(TASK_HISTORY_BOUND_NOTICE_AT));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: 01SCHED` }));

    expect(await screen.findByText(TASK_HISTORY_BOUND_TEXT)).toBeVisible();
  });

  it("does not warn about trimming a history that has not been trimmed", async () => {
    answer(listing({ tasks: [task({ id: "01SCHED" })] }));
    vi.mocked(syncTaskHistory).mockResolvedValue(runList(TASK_HISTORY_BOUND_NOTICE_AT - 1));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(screen.getByRole("button", { name: `${TASK_HISTORY_TITLE}: 01SCHED` }));
    await screen.findByTestId(TASKS_HISTORY_TESTID);

    expect(screen.queryByText(TASK_HISTORY_BOUND_TEXT)).toBeNull();
  });
});

describe("a list of names, and one task at a time", () => {
  it("gives each name one line and no controls at all", async () => {
    // The owner's complaint, made mechanical. Before Story 59.1 the row WAS the
    // detail — ten stacked blocks and three buttons each — so reaching the
    // eighth task's runs meant scrolling past seven cards. A control that
    // reappears here is the regression, and it is asserted as an absence
    // because that is the only shape that fails when somebody adds one back.
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" }), task({ id: "C" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(3));

    // One option per row and not one more: the row itself. Asserted over the
    // whole list because the row IS the option now, so a nested one would be a
    // second option this count would catch.
    expect(screen.getAllByRole("option")).toHaveLength(3);
    for (const row of screen.getAllByTestId(TASKS_ROW_TESTID)) {
      // ZERO buttons, which Story 59.4 made the stronger claim rather than a
      // weaker one: the row lost its `<button>` because `aria-selected` is not a
      // state `role="button"` supports, so the old "exactly one button" would
      // now be satisfied by a row that had grown a control and lost its option.
      // Not Run now, not Edit, not Forget, not Runs.
      expect(row).toHaveAttribute("role", "option");
      expect(within(row).queryAllByRole("button")).toHaveLength(0);
      for (const name of [
        TASK_RUN_NOW_TEXT,
        TASK_EDIT_TEXT,
        TASK_FORGET_TEXT,
        `${TASK_HISTORY_TITLE}: ${row.dataset.taskId}`,
      ]) {
        expect(within(row).queryByRole("button", { name })).toBeNull();
      }
      // And none of the detail's own cells, which is what made the row tall.
      for (const label of [TASK_SCHEDULE_LABEL, TASK_LAST_RUN_LABEL, TASK_HOST_LABEL]) {
        expect(within(row).queryByText(label)).toBeNull();
      }
      expect(within(row).queryByText(SENTENCE_APP)).toBeNull();
    }
  });

  it("still says the four facts a name has to carry", async () => {
    // Re-siting, not deletion: the epic's list is kind, name, host and next due,
    // and Story 59.3's acceptance adds the mode — a fact that quietly moved into
    // the detail would un-ship that story.
    // Against the real clock and not `NOW`: this pane measures every relative
    // time from `Date.now()`, and the fixture epoch is nearly a year behind it —
    // so a `NOW`-relative instant renders as "due now" and would assert nothing
    // about the next-due cell at all.
    answer(listing({ tasks: [task({ id: "01SCHED", nextDueMs: Date.now() + 90_000 })] }));
    render(<TasksPane />);
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    expect(within(row).getByText("sync")).toBeInTheDocument();
    expect(within(row).getByText("scheduled")).toBeInTheDocument();
    expect(within(row).getByText("01SCHED")).toBeInTheDocument();
    expect(within(row).getByText("This app")).toBeInTheDocument();
    expect(within(row).getByText("in 1 min")).toBeInTheDocument();
  });

  it("opens on the first task rather than on an empty region", async () => {
    // A detail region that started blank over a list with rows would be a second
    // empty state competing with the real one — and defaulting costs no read,
    // because every field it draws is already on the `TaskVm` the listing
    // carries. That last clause is the one that matters, and the next test is
    // the one that holds it.
    answer(listing({ tasks: [task({ id: "01FIRST" }), task({ id: "01SECOND" })] }));
    render(<TasksPane />);

    const region = await screen.findByTestId(TASKS_DETAIL_TESTID);
    expect(region).toHaveAttribute("data-task-id", "01FIRST");
    expect(within(region).getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeInTheDocument();
    // Three regions sit inside this surface and a reader jumping between
    // landmarks has to be able to tell them apart: the pane, the column of
    // names, and this. All three are named, and named differently.
    expect(screen.getByRole("region", { name: TASKS_DETAIL_LABEL })).toContainElement(region);
    expect(
      screen.getByRole("region", { name: SURFACE_COLUMNS["tasks-list"].title }),
    ).toBeInTheDocument();
    expect(screen.getByRole("region", { name: TASKS_PANE_TITLE })).toBeInTheDocument();
  });

  it("selects nothing on mount, and a click moves the mark rather than adding one", async () => {
    // Story 59.1's test, rewritten rather than worked around. It asserted one
    // `aria-current` row on the ground that `aria-selected` announces a SET to a
    // reader when only one thing can be chosen — and Story 59.4 removed that
    // ground by building the bulk consumer, so the attribute and the refusal
    // flip together.
    //
    // What has NOT changed is the claim: the selection's **contents**, not its
    // size. "Exactly one" and "the right one" are different claims, and a test
    // that counted would pass while the mark sat on the wrong row.
    //
    // **Nothing is selected on mount** (Story 59.4 review, P6). The detail
    // region's `tasks[0]` fallback decides which task is *drawn* with nobody
    // chosen; it is not a selection, and announcing "01FIRST, selected" inside
    // an `aria-multiselectable` listbox while no bulk verb is offered told a
    // screen-reader user about a choice nobody made. `files-pane.tsx` has no
    // such fallback and marks nothing on its own mount.
    answer(listing({ tasks: [task({ id: "01FIRST" }), task({ id: "01SECOND" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    expect(selectedRows()).toEqual([]);
    // On EVERY row, `"false"` included: a list that marked only the selected
    // rows would leave a screen reader unable to say "not selected" about the
    // others (`files-pane.tsx:2528-2532`).
    expect(rowOption("01FIRST")).toHaveAttribute("aria-selected", "false");
    expect(rowOption("01SECOND")).toHaveAttribute("aria-selected", "false");
    // The detail region still draws the first task — the fallback is unchanged,
    // and it is the thing `aria-selected` was conflated with.
    expect(
      within(screen.getByRole("region", { name: TASKS_DETAIL_LABEL })).getByText("01FIRST"),
    ).toBeInTheDocument();
    // ...and the list still has exactly one tab stop with nothing selected,
    // which is the cursor's job rather than the selection's (see `cursorId`).
    expect(tabStops()).toEqual(["01FIRST"]);
    // And the container says a set is possible at all, which is the half a
    // reader learns before touching anything.
    expect(screen.getByRole("listbox", { name: TASKS_LIST_LABEL })).toHaveAttribute(
      "aria-multiselectable",
      "true",
    );

    selectRow("01FIRST");
    expect(selectedRows()).toEqual(["01FIRST"]);

    selectRow("01SECOND");
    // One, and the other one — not two: a plain click REPLACES.
    expect(selectedRows()).toEqual(["01SECOND"]);
    expect(rowOption("01FIRST")).toHaveAttribute("aria-selected", "false");
  });

  it("reads nothing at all when a task is chosen", async () => {
    // `spec-58-3:40`'s Never clause is AD-62's anti-poll invariant and it has
    // teeth. A master/detail satisfies it BETTER than the old per-row disclosure
    // — exactly one task is drawn by construction — but only while choosing
    // stays inert. The moment selection triggers a read, twenty tasks is twenty
    // reads for a person arrowing through the list.
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" }), task({ id: "C" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(3));
    expect(syncTasks).toHaveBeenCalledTimes(1);
    expect(syncPacedWork).toHaveBeenCalledTimes(1);

    selectRow("B");
    selectRow("C");
    selectRow("A");
    // The modifier gestures too (Story 59.4). Assembling a five-row selection
    // is the shape most able to break this: it is five gestures, and a read on
    // any of them would be five reads for one deliberate bulk action.
    await clickRowWith("C", { metaKey: true });
    await clickRowWith("B", { shiftKey: true });

    expect(syncTasks).toHaveBeenCalledTimes(1);
    expect(syncPacedWork).toHaveBeenCalledTimes(1);
    expect(syncTaskHistory).not.toHaveBeenCalled();
    // And no WRITE either: a selection is not an action.
    expect(syncTasksSetEnabled).not.toHaveBeenCalled();
    expect(syncTasksForget).not.toHaveBeenCalled();
    // Story 59.12 put a panel target behind the plain clicks above, and it does
    // not weaken this claim: `setActiveTarget` is a store write, so the panel
    // moved three times and the pane still issued exactly one read.
    expect(activePanel(panelsStore.getState()).target).toEqual({ kind: "task", taskId: "A" });
  });

  it("moves the selection with the arrow keys, and stops at both ends", async () => {
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));
    const list = screen.getByRole("listbox", { name: TASKS_LIST_LABEL });
    const shown = (): string | undefined => screen.getByTestId(TASKS_DETAIL_TESTID).dataset.taskId;

    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(shown()).toBe("B");
    // Clamped rather than wrapping: a list whose Down at the bottom silently
    // returns to the top is a list that has lost the reader's place.
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(shown()).toBe("B");
    fireEvent.keyDown(list, { key: "ArrowUp" });
    expect(shown()).toBe("A");
    fireEvent.keyDown(list, { key: "ArrowUp" });
    expect(shown()).toBe("A");
    fireEvent.keyDown(list, { key: "End" });
    expect(shown()).toBe("B");
    fireEvent.keyDown(list, { key: "Home" });
    expect(shown()).toBe("A");
  });

  it("leaves a chord to the global shortcuts", async () => {
    // A pane that swallows ⌘↓ breaks a shortcut it knows nothing about.
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    fireEvent.keyDown(screen.getByRole("listbox", { name: TASKS_LIST_LABEL }), {
      key: "ArrowDown",
      metaKey: true,
    });
    expect(screen.getByTestId(TASKS_DETAIL_TESTID).dataset.taskId).toBe("A");
  });

  it("moves off a task the record no longer holds", async () => {
    // The same pruning rule the edit form and the open runs section already
    // follow: a region cannot describe a row the listing has dropped, and the
    // likeliest way that happens is the other host on this shared record.
    vi.mocked(syncTasks)
      .mockResolvedValueOnce(listing({ tasks: [task({ id: "A" }), task({ id: "B" })] }))
      .mockResolvedValue(listing({ tasks: [task({ id: "A" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));
    selectRow("B");
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "B");

    fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));

    await waitFor(() =>
      expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "A"),
    );
  });

  it("draws no detail region at all when nothing readable is listed", async () => {
    // Two shapes at once, and they are different facts: an empty record, and a
    // record whose only rows this build cannot decode. Neither may leave a
    // region describing a task, and the unknown rows keep their own section.
    answer(listing({ tasks: [], unknown: [{ id: "01TELEPORT", reason: "invalid kind" }] }));
    render(<TasksPane />);
    await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);

    expect(screen.queryByTestId(TASKS_DETAIL_TESTID)).toBeNull();
    expect(screen.getByText(TASKS_UNKNOWN_HEADING)).toBeInTheDocument();
  });

  it("offers no way to choose a row this build cannot read", async () => {
    // There is no `TaskVm` behind it, so there is nothing to draw a detail from.
    // A row that selects into an empty region is the same defect as a control
    // that can only fail.
    answer(
      listing({
        tasks: [task({ id: "A" })],
        unknown: [{ id: "01TELEPORT", reason: "invalid kind 'teleport'" }],
      }),
    );
    render(<TasksPane />);
    const unknown = await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);

    expect(within(unknown).queryAllByRole("button")).toHaveLength(0);
    expect(within(unknown).queryByRole("option")).toBeNull();
    // No selection state of any kind, in either spelling: Story 59.1 marked the
    // chosen row with `aria-current` and Story 59.4 moved to `aria-selected`, and
    // an unreadable row has never carried either. It is not a `TaskVm`, so there
    // is nothing to draw a detail from and nothing a batch could act on.
    expect(unknown.querySelector("[aria-current]")).toBeNull();
    expect(unknown.querySelector("[aria-selected]")).toBeNull();
  });

  it("says how many names it is hiding, and offers no second Refresh", async () => {
    // The correction this rail needed. The Files tree's strip carries Refresh
    // because folding that column takes its header with it; this pane's header
    // sits above BOTH columns, so a Refresh on the strip would have been a
    // second control with the same accessible name as the one still on screen —
    // undistinguishable to anyone navigating by name. What the fold really
    // takes is the names, so that is what the strip answers for.
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["tasks-list"].label}`,
      }),
    );

    expect(screen.queryByTestId(TASKS_ROW_TESTID)).toBeNull();
    // Exactly one of each, still: the header's, not a copy on the strip.
    expect(screen.getAllByRole("button", { name: TASK_REFRESH_TEXT })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: TASK_FORM_ADD_TITLE })).toHaveLength(1);
    // And the strip says what is behind it, from the listing rather than from
    // the rows it just unmounted — `count-label.ts`'s enforcement.
    // On the control's accessible NAME, which is where `SurfaceRailControl`
    // puts `detail` — a badge alone reaches a screen reader not at all, and the
    // digits in the corner are `aria-hidden` for exactly that reason.
    expect(
      screen.getByRole("button", {
        name: `${TASKS_RAIL_LIST_LABEL}, ${countLabel(2, TASKS)}`,
      }),
    ).toBeInTheDocument();
    // The detail region does NOT fold with the list: a person who put the names
    // away to read one task must still be reading that task.
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "A");
  });
});

/**
 * Several tasks at once (Story 59.4).
 *
 * The semantics asserted here are **not this pane's own**: every one of them is
 * the semantics `files-pane.test.tsx:2609/2630/2657/2912` already asserts about
 * the one selection idiom this app has, read against
 * `files-pane.tsx:1656-1668`'s doc for what each modifier is *supposed* to mean.
 * A gesture that behaved differently here would be the second selection model
 * `spec-45-17…:200` forbids by name, and these tests are what would catch it.
 *
 * Three idioms are copied verbatim from that file: a modifier click is a
 * `fireEvent.click(el, { metaKey: true })` inside one `act`; selection is read
 * through `aria-selected` in both spellings; and the count is asserted by ROLE
 * and NAME plus its text content, because the badge draws the figure and
 * announces the sentence.
 */
describe("several tasks at once", () => {
  /** Three rows, each with its own `updatedMs`, ready to select in. */
  async function threeTasks(): Promise<void> {
    answer(
      listing({
        tasks: [
          task({ id: "A", updatedMs: NOW - 3_000 }),
          task({ id: "B", updatedMs: NOW - 2_000 }),
          task({ id: "C", updatedMs: NOW - 1_000 }),
        ],
      }),
    );
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(3));
  }

  async function pressBulk(name: string): Promise<void> {
    const control = screen.getByRole("button", { name });
    await act(async () => {
      fireEvent.click(control);
      await Promise.resolve();
    });
  }

  it("selects one row on a plain click and replaces the selection on the next", async () => {
    await threeTasks();

    selectRow("A");
    expect(selectedRows()).toEqual(["A"]);

    selectRow("B");
    // Replaced, not accumulated: a plain click is not a multiselect gesture,
    // and a list that accumulated them would disable the task you looked at
    // five minutes ago.
    expect(selectedRows()).toEqual(["B"]);
  });

  it("extends the selection with Cmd-click and takes the run with Shift-click", async () => {
    await threeTasks();

    selectRow("A");
    // Singular at exactly one, and through `countLabel` rather than a
    // hand-rolled plural (`count-label.ts:29-31`).
    expect(tasksSelectionSentence(1)).toBe("1 task selected");
    expect(screen.getByRole("status", { name: "1 task selected" })).toHaveTextContent("1");

    await clickRowWith("C", { metaKey: true });
    expect(selectedRows()).toEqual(["A", "C"]);
    // The middle row was NOT taken: Cmd adds one, it does not fill the gap.
    expect(rowOption("B")).toHaveAttribute("aria-selected", "false");
    expect(screen.getByRole("status", { name: "2 tasks selected" })).toHaveTextContent("2");

    selectRow("A");
    await clickRowWith("C", { shiftKey: true });
    // Shift fills it, because a run is what a person sees between two rows —
    // and it is inclusive at both ends.
    expect(selectedRows()).toEqual(["A", "B", "C"]);
    expect(screen.getByRole("status", { name: "3 tasks selected" })).toHaveTextContent("3");
  });

  it("treats Ctrl-click as Cmd-click, because one of them is the wrong platform", async () => {
    // Asserted on its own rather than folded into the test above, for
    // `files-pane.test.tsx:2657`'s reason: three modifier clicks inside one
    // `act` cannot tell you which modifier the handler honoured, and jsdom
    // reports a non-Mac platform.
    await threeTasks();

    selectRow("A");
    await clickRowWith("B", { ctrlKey: true });

    expect(selectedRows()).toEqual(["A", "B"]);
    expect(screen.getByRole("status", { name: "2 tasks selected" })).toHaveTextContent("2");
  });

  it("clears the selection on Escape", async () => {
    await threeTasks();

    selectRow("A");
    await clickRowWith("B", { metaKey: true });
    expect(screen.getByTestId(TASKS_SELECTED_TESTID)).toBeInTheDocument();

    await act(async () => {
      fireEvent.keyDown(rowOption("B"), { key: "Escape" });
      await Promise.resolve();
    });

    // Nothing is held, so the count and the three verbs are gone. The detail
    // region falls back to the first row, which is Story 59.1's rule and not a
    // selection anybody made.
    expect(screen.queryByTestId(TASKS_SELECTED_TESTID)).toBeNull();
    expect(screen.queryByRole("button", { name: TASKS_BULK_ENABLE_TEXT })).toBeNull();
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "A");
  });

  it("offers no bulk action until something is selected", async () => {
    // Absent, not disabled: a disabled control says *not now*, and with nothing
    // selected the truth is *there is nothing to do this to*. Asserted before
    // any click, which is also the state every install opens ⌘8 in.
    await threeTasks();

    expect(screen.queryByTestId(TASKS_SELECTED_TESTID)).toBeNull();
    for (const name of [TASKS_BULK_ENABLE_TEXT, TASKS_BULK_DISABLE_TEXT, TASKS_BULK_FORGET_TEXT]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
  });

  it("sends each row's own reading as the baseline the write is checked against", async () => {
    // The decision this argues for is in `TaskBatchIdReq`'s doc: the app's edit
    // form is the one caller that passes a baseline, because it seeded its
    // values once — and a bulk action from a rendered list is that case, not the
    // read-and-write-in-one-call case the CLI is. Ids alone would make the bulk
    // path silently weaker than the single-id path it stands in for.
    await threeTasks();

    selectRow("A");
    await clickRowWith("C", { shiftKey: true });
    await pressBulk(TASKS_BULK_ENABLE_TEXT);

    expect(syncTasksSetEnabled).toHaveBeenCalledWith(
      [
        { id: "A", baselineUpdatedMs: NOW - 3_000 },
        { id: "B", baselineUpdatedMs: NOW - 2_000 },
        { id: "C", baselineUpdatedMs: NOW - 1_000 },
      ],
      true,
    );
  });

  it("names what it could not enable rather than shrinking the selection in silence", async () => {
    // The receipt is the reason this story exists. Three of five refuse for
    // three DIFFERENT reasons — a moved baseline, a row this build cannot read,
    // a spelling keeper could never have stored — and each sentence has to reach
    // the screen attributed to its own id. A surface that rendered the first
    // refusal, or a count of refusals, would leave two ids looking enabled.
    const moved =
      "task 'B' was changed elsewhere since this was opened: refusing to write stale values over it — re-read it and try again";
    const unreadable =
      "task 'C' is stored, but this keeper cannot read it: invalid kind 'teleport'";
    const spelling = "task id 'D' is not a spelling this keeper could ever have stored";
    answer(
      listing({
        tasks: [
          task({ id: "A" }),
          task({ id: "B" }),
          task({ id: "C" }),
          task({ id: "D" }),
          task({ id: "E" }),
        ],
      }),
    );
    vi.mocked(syncTasksSetEnabled).mockResolvedValue({
      entries: [
        { id: "A", outcome: "saved", effect: "updated", reason: null },
        { id: "B", outcome: "refused", effect: null, reason: moved },
        { id: "C", outcome: "refused", effect: null, reason: unreadable },
        { id: "D", outcome: "refused", effect: null, reason: spelling },
        { id: "E", outcome: "saved", effect: "rearmed", reason: null },
      ],
    });
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(5));

    selectRow("A");
    await clickRowWith("E", { shiftKey: true });
    await pressBulk(TASKS_BULK_DISABLE_TEXT);

    await waitFor(() => expect(screen.getAllByTestId(TASKS_ORPHAN_REFUSAL_TESTID)).toHaveLength(3));
    // Verbatim, and attributed: keeper's own sentence under the id it is about.
    // Nothing here is a sentence this pane composed.
    const drawn = screen
      .getAllByTestId(TASKS_ORPHAN_REFUSAL_TESTID)
      .map((node) => node.textContent ?? "");
    expect(drawn).toEqual([`B: ${moved}`, `C: ${unreadable}`, `D: ${spelling}`]);
    // And the selection is NOT quietly shrunk to the two that went: the person
    // still has the five rows they chose, and can act on them again.
    expect(screen.getByRole("status", { name: "5 tasks selected" })).toHaveTextContent("5");
    expect(selectedRows()).toEqual(["A", "B", "C", "D", "E"]);
  });

  it("says so when an id it was asked about is not there at all", async () => {
    // `missing` is a fourth outcome and not a refusal — a well-formed id whose
    // row another host forgot is usually benign — so the wire carries no
    // sentence for it and the pane owns one. Something must still be said: a
    // bulk action that silently skipped an id looks exactly like one that
    // worked.
    await threeTasks();
    vi.mocked(syncTasksSetEnabled).mockResolvedValue({
      entries: [
        { id: "A", outcome: "saved", effect: "updated", reason: null },
        { id: "B", outcome: "missing", effect: null, reason: null },
      ],
    });

    selectRow("A");
    await clickRowWith("B", { metaKey: true });
    await pressBulk(TASKS_BULK_ENABLE_TEXT);

    await waitFor(() =>
      expect(screen.getByTestId(TASKS_ORPHAN_REFUSAL_TESTID)).toHaveTextContent(
        `B: ${TASKS_BULK_MISSING_TEXT}`,
      ),
    );
  });

  it("asks before forgetting several, and empties the selection once they are gone", async () => {
    // A destructive verb over a set with no confirmation would be worse than the
    // single-id Forget it stands in for, not better — and the question names the
    // COUNT, because a set has no one name and the number is what is being
    // decided about.
    await threeTasks();

    selectRow("A");
    await clickRowWith("C", { shiftKey: true });
    await pressBulk(TASKS_BULK_FORGET_TEXT);

    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText(tasksForgetConfirmTitle(3))).toBeInTheDocument();
    expect(within(dialog).getByText("Forget 3 tasks?")).toBeInTheDocument();
    // Nothing has gone yet: there is no gesture in this pane that forgets a task
    // without keeper's framing being read first.
    expect(syncTasksForget).not.toHaveBeenCalled();

    await act(async () => {
      fireEvent.click(within(dialog).getByRole("button", { name: TASK_FORGET_TEXT }));
      await Promise.resolve();
    });

    // Exactly the ids that were held, in the list's own order.
    expect(syncTasksForget).toHaveBeenCalledWith(["A", "B", "C"]);
    // And nothing stays selected: the rows are gone, unlike an Enable where they
    // are still there and still the same set.
    await waitFor(() => expect(screen.queryByTestId(TASKS_SELECTED_TESTID)).toBeNull());
  });

  it("keeps one task at a time working exactly as it did", async () => {
    // Story 59.1's whole surface, re-asserted under a set: single-select has to
    // be byte-for-byte what it was, which is what the resolved-not-stored rule
    // buys. The empty-selection fallback, a plain click and the arrows all
    // resolve to exactly one task and the region draws it.
    await threeTasks();

    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "A");

    selectRow("B");
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "B");

    const list = screen.getByRole("listbox", { name: TASKS_LIST_LABEL });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "C");
    fireEvent.keyDown(list, { key: "ArrowUp" });
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "B");

    // Two selected: no task's detail is drawn, and what stands in its place says
    // what there is rather than pretending to be one task.
    await clickRowWith("C", { metaKey: true });
    expect(screen.queryByTestId(TASKS_DETAIL_TESTID)).toBeNull();
    expect(screen.getByTestId(TASKS_SELECTION_TESTID)).toHaveTextContent("2 tasks selected");
    expect(screen.getByRole("region", { name: TASKS_DETAIL_LABEL })).toBeInTheDocument();

    // Collapsed back to one, and the region is a task's again.
    await clickRowWith("C", { metaKey: true });
    expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", "B");
  });

  it("offers no selection and no bulk action on a row it cannot read", async () => {
    // Neither an `unknown` row nor a projected paced row is a `TaskVm`, so
    // neither can be in a selection: a control that can only fail is worse than
    // no control, and half a selection no command can act on is worse than one
    // that visibly reset.
    answer(
      listing({
        tasks: [task({ id: "A" })],
        unknown: [{ id: "01TELEPORT", reason: "invalid kind 'teleport'" }],
      }),
    );
    answerPaced([pacedRow()]);
    render(<TasksPane />);
    const unknown = await screen.findByTestId(TASKS_UNKNOWN_ROW_TESTID);
    const paced = screen.getByTestId(PACED_ROW_TESTID);

    // Exactly one listbox on this surface: the readable names. Neither of the
    // other two lists was widened into one.
    expect(screen.getAllByRole("listbox")).toHaveLength(1);
    expect(screen.getByRole("list", { name: PACED_HEADING })).toBeInTheDocument();
    for (const row of [unknown, paced]) {
      expect(within(row).queryByRole("option")).toBeNull();
      expect(row.querySelector("[aria-selected]")).toBeNull();
    }

    // Clicking either one selects nothing, so no bulk control appears.
    await act(async () => {
      fireEvent.click(unknown);
      fireEvent.click(paced);
      await Promise.resolve();
    });
    expect(screen.queryByTestId(TASKS_SELECTED_TESTID)).toBeNull();
    expect(screen.queryByRole("button", { name: TASKS_BULK_FORGET_TEXT })).toBeNull();
  });

  it("names a whole batch that would not run at all, and keeps the selection", async () => {
    // The one whole-batch failure the design keeps distinct from a per-id
    // refusal: the batched verbs reject only when the task record would not read
    // at all, and every per-id outcome comes back inside the receipt instead. So
    // this alert must carry keeper's sentence and NOT be a place a single id's
    // reason can land — and the selection must survive, because the rows are
    // still there and still the same set. Clearing it would tell the person the
    // action was taken.
    const refused = { message: "the task record could not be read" };
    await threeTasks();
    vi.mocked(syncTasksSetEnabled).mockRejectedValue(refused);

    selectRow("A");
    await clickRowWith("C", { shiftKey: true });
    await pressBulk(TASKS_BULK_DISABLE_TEXT);

    const alert = await waitFor(() => screen.getByTestId(TASKS_BULK_ERROR_TESTID));
    expect(alert).toHaveAttribute("role", "alert");
    expect(alert).toHaveTextContent(refused.message);
    expect(selectedRows()).toEqual(["A", "B", "C"]);
    expect(screen.getByRole("status", { name: "3 tasks selected" })).toHaveTextContent("3");

    // The same for Forget, whose success path is the one that DOES empty the
    // selection — so a rejection there is the case where the two diverge, and
    // the rows are still on screen to be tried again.
    vi.mocked(syncTasksForget).mockRejectedValue({ message: "the record is held elsewhere" });
    await pressBulk(TASKS_BULK_FORGET_TEXT);
    const dialog = await screen.findByRole("alertdialog");
    await act(async () => {
      fireEvent.click(within(dialog).getByRole("button", { name: TASK_FORGET_TEXT }));
      await Promise.resolve();
    });

    await waitFor(() =>
      expect(screen.getByTestId(TASKS_BULK_ERROR_TESTID)).toHaveTextContent(
        "the record is held elsewhere",
      ),
    );
    expect(selectedRows()).toEqual(["A", "B", "C"]);
  });

  it("says a refused id was refused even when keeper gave no reason", async () => {
    // `reason` is non-null exactly when `outcome` is `refused`, by
    // `TaskBatchEntryVm`'s documented invariant — but the wire type is nullable,
    // and a `refused` entry that rendered nothing would read exactly like a
    // success. The pane owns a sentence for that, and this is the guard.
    await threeTasks();
    vi.mocked(syncTasksSetEnabled).mockResolvedValue({
      entries: [
        { id: "A", outcome: "saved", effect: "updated", reason: null },
        { id: "B", outcome: "refused", effect: null, reason: null },
      ],
    });

    selectRow("A");
    await clickRowWith("B", { metaKey: true });
    await pressBulk(TASKS_BULK_ENABLE_TEXT);

    await waitFor(() =>
      expect(screen.getByTestId(TASKS_ORPHAN_REFUSAL_TESTID)).toHaveTextContent(
        `B: ${TASKS_BULK_NO_REASON_TEXT}`,
      ),
    );
  });

  it("issues no second call while a bulk write is still in flight", async () => {
    // Both calls would carry the SAME pre-bump `baselineUpdatedMs` values, so
    // the second is refused `changed elsewhere` for every id — by the caller's
    // own first write. A person double-clicking Disable saw five spurious
    // "changed elsewhere" refusals, which is worse than no feedback at all.
    await threeTasks();
    // The executor form, not `Promise.withResolvers`: this project compiles
    // against `lib: ES2020`, which predates it — the reason this file already
    // gives twice above.
    let settle!: (receipt: TaskBatchReceiptVm) => void;
    vi.mocked(syncTasksSetEnabled).mockReturnValue(
      new Promise<TaskBatchReceiptVm>((resolve) => {
        settle = resolve;
      }),
    );

    selectRow("A");
    await clickRowWith("C", { shiftKey: true });
    await pressBulk(TASKS_BULK_DISABLE_TEXT);
    expect(syncTasksSetEnabled).toHaveBeenCalledTimes(1);

    // The control is disabled rather than merely inert, so the person can see
    // that the first press is still going.
    expect(screen.getByRole("button", { name: TASKS_BULK_DISABLE_TEXT })).toBeDisabled();
    await pressBulk(TASKS_BULK_DISABLE_TEXT);
    expect(syncTasksSetEnabled).toHaveBeenCalledTimes(1);

    await act(async () => {
      settle({ entries: [] });
      await Promise.resolve();
    });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: TASKS_BULK_DISABLE_TEXT })).toBeEnabled(),
    );
  });

  it("keeps exactly one tab stop when the anchor row leaves the listing", async () => {
    // With two or more rows selected the resolved task is `null`, so a cursor
    // that fell back no further left NO row with `tabIndex 0` the moment another
    // host forgot the anchor row — and a listbox whose every option is
    // `tabIndex -1` is unreachable by Tab.
    await threeTasks();

    // Three held, so the resolved task is `null`, and the anchor is the row the
    // last Cmd-click landed on.
    selectRow("A");
    await clickRowWith("B", { metaKey: true });
    await clickRowWith("C", { metaKey: true });
    expect(selectedRows()).toEqual(["A", "B", "C"]);
    expect(tabStops()).toEqual(["C"]);

    // Another host forgot the anchor row. Two rows are still held, so
    // `resolvedId` stays `null` and the cursor has to fall back to the
    // selection's own first row.
    answer(
      listing({
        tasks: [
          task({ id: "A", updatedMs: NOW - 3_000 }),
          task({ id: "B", updatedMs: NOW - 2_000 }),
        ],
      }),
    );
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: TASK_REFRESH_TEXT }));
      await Promise.resolve();
    });
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    expect(selectedRows()).toEqual(["A", "B"]);
    expect(tabStops()).toEqual(["A"]);
  });
});

/**
 * A task you can open in a tab (Story 59.12).
 *
 * The owner's report was that he *"could not click an element in the task list
 * and see the details in a new tab"*. Clicking a row already worked and was
 * already tested — it selects, and the pane's own sibling region draws the task
 * — so what these assertions are about is the OTHER half: the panel list beside
 * the pane, and the two gestures every browsing surface in this app already
 * answers to.
 *
 * Asserted against `panelsStore` and nothing invented for the occasion. That is
 * the point of the story: a task is an ordinary `PanelTargetVm`, so the preview
 * and the open-beside are the same two store verbs a file row calls, and a test
 * that measured them any other way would be testing a second idiom rather than
 * proving there is not one.
 */
describe("a task you can open beside the list", () => {
  /** What every panel is showing, left to right. */
  function panelTargets(): (PanelTargetVm | null)[] {
    return panelsStore.getState().panels.map((panel) => panel.target);
  }

  async function twoTasks(): Promise<void> {
    answer(listing({ tasks: [task({ id: "A" }), task({ id: "B" })] }));
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));
  }

  it("previews a task into the active panel on a plain click, without growing the list", async () => {
    await twoTasks();

    selectRow("A");

    expect(activePanel(panelsStore.getState()).target).toEqual({ kind: "task", taskId: "A" });
    expect(panelsStore.getState().panels).toHaveLength(1);

    // And a second plain click REPLACES rather than appends, which is what
    // makes stepping down a list of twenty leave one panel and not twenty.
    selectRow("B");

    expect(panelTargets()).toEqual([{ kind: "task", taskId: "B" }]);
  });

  it("opens a task beside what was already open on a double click, and puts back what the click displaced", async () => {
    await twoTasks();

    // Pinned, so there is something under the next preview for the pin to put
    // back. This is the arrangement a person is in after opening one task.
    await act(async () => {
      fireEvent.doubleClick(rowOption("A"));
      await Promise.resolve();
    });
    expect(panelTargets()).toEqual([{ kind: "task", taskId: "A" }]);

    // The gesture as the DOM delivers it: a real double click fires `click`
    // first, so without `Panel.replaced` this would replace A with B and then
    // open B beside itself — two panels of B, and A gone.
    const rowB = rowOption("B");
    await act(async () => {
      fireEvent.click(rowB);
      fireEvent.doubleClick(rowB);
      await Promise.resolve();
    });

    expect(panelTargets()).toEqual([
      { kind: "task", taskId: "A" },
      { kind: "task", taskId: "B" },
    ]);
    expect(activePanel(panelsStore.getState()).target).toEqual({ kind: "task", taskId: "B" });
  });

  it("leaves every panel alone for a modified click, which is a selection gesture only", async () => {
    await twoTasks();

    // A selected and its panel pinned, so a modifier click that leaked into the
    // panel would be visible as a changed target rather than only as a count —
    // and so the Cmd-click below grows a set rather than starting one.
    selectRow("A");
    await act(async () => {
      fireEvent.doubleClick(rowOption("A"));
      await Promise.resolve();
    });
    const before = panelTargets();

    await clickRowWith("B", { metaKey: true });
    expect(selectedRows()).toEqual(["A", "B"]);
    expect(panelTargets()).toEqual(before);

    // Back to A, so the Shift below measures a run that ENDS on B — the case
    // where a leak into the panel would be visible, because the range's last
    // row is not the row the panel is holding.
    selectRow("A");
    await clickRowWith("B", { shiftKey: true });
    expect(selectedRows()).toEqual(["A", "B"]);
    expect(panelTargets()).toEqual(before);

    // Somebody assembling a selection to Forget did not ask for a panel, and
    // the last Shift-click of a range is not the task they were looking at.
    expect(activePanel(panelsStore.getState()).target).toEqual({ kind: "task", taskId: "A" });
    expect(sameTarget(panelTargets()[0] ?? null, { kind: "task", taskId: "A" })).toBe(true);
  });

  /**
   * The invariant that makes two hosts over one task record safe.
   *
   * The pane's own detail region and a task panel are two hosts of one
   * component, so they cannot word a fact differently — but they CAN be aimed
   * at different tasks, and the story's claim is that only a gesture which
   * asked for exactly that will do it. A single click keeps them in lockstep;
   * the double click is the one that says *keep this one while I look at
   * another*, which is the whole reason it exists.
   *
   * The panel side is read off the store rather than rendered, because
   * `PanelStrip` is mounted beside this pane by `AppShell` and not by it: what
   * this file owns is which target the pane PUTS there.
   */
  it("keeps the pane's detail region and the panel on one task until a gesture asks otherwise", async () => {
    await twoTasks();

    // A pinned panel to start from, and it is load-bearing rather than
    // scene-setting: previewing into the panel a fresh keeper starts with
    // records `was: null`, and the store deliberately PINS in place rather than
    // appending when the thing a preview displaced was nothing. So a run that
    // only ever previewed could never grow a second panel, and a test that
    // started there would prove the opposite of what it claimed.
    await act(async () => {
      fireEvent.doubleClick(rowOption("A"));
      await Promise.resolve();
    });

    // Lockstep. A plain click moves both, and does so for every row it lands
    // on, so a reader stepping down the list never sees two different tasks.
    for (const id of ["B", "A", "B"]) {
      selectRow(id);
      expect(screen.getByTestId(TASKS_DETAIL_TESTID)).toHaveAttribute("data-task-id", id);
      expect(activePanel(panelsStore.getState()).target).toEqual({ kind: "task", taskId: id });
    }
    expect(panelTargets()).toHaveLength(1);

    // The one gesture that asks for a difference, delivered as the DOM delivers
    // it: B is previewing over A, so pinning it puts A back and opens B beside.
    const rowB = rowOption("B");
    await act(async () => {
      fireEvent.click(rowB);
      fireEvent.doubleClick(rowB);
      await Promise.resolve();
    });

    // The region followed the selection to B and still draws everything Story
    // 59.1 put in it; the panel that was holding A is still holding A. Two
    // subjects, and the reader asked for both of them.
    const region = screen.getByTestId(TASKS_DETAIL_TESTID);
    expect(region).toHaveAttribute("data-task-id", "B");
    expect(within(region).getByRole("button", { name: TASK_RUN_NOW_TEXT })).toBeInTheDocument();
    expect(panelTargets()).toEqual([
      { kind: "task", taskId: "A" },
      { kind: "task", taskId: "B" },
    ]);
  });
});
