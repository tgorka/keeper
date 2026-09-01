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
  // Story 58.7's read. Mocked here and answered in `beforeEach`, because the
  // pane now reads it in the SAME settled pass as `syncTasks` — an unanswered
  // mock resolves `undefined` and the section would render rows out of it.
  syncPacedWork: vi.fn(),
  // The mounted form's own read (Story 58.1): the folder picker.
  syncProfiles: vi.fn(),
}));

import { LIST_FOLD_MORE_LABEL } from "@/components/layout/list-fold";
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
  TASKS_CLOCK_TICK_MS,
  TASKS_DESCRIPTION_TESTID,
  TASKS_ERROR_TESTID,
  TASKS_HISTORY_REFUSAL_TESTID,
  TASKS_HISTORY_ROW_TESTID,
  TASKS_HISTORY_TESTID,
  TASKS_ORPHAN_REFUSAL_TESTID,
  TASKS_PANE_EMPTY_AFTER,
  TASKS_PANE_EMPTY_COMMAND,
  TASKS_PANE_EMPTY_SENTENCE,
  TASKS_PANE_TITLE,
  TASKS_REFUSAL_TESTID,
  TASKS_ROW_TESTID,
  TASKS_RUN_NOW_SENTENCE,
  TASKS_UNKNOWN_BADGE,
  TASKS_UNKNOWN_HEADING,
  TASKS_UNKNOWN_NO_ID_TEXT,
  TASKS_UNKNOWN_ROW_TESTID,
  TasksPane,
  taskForgetConfirmTitle,
  taskHistoryUnshownText,
  taskOutcomeText,
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
import type { PacedWorkVm, TaskListingVm, TaskRunVm, TaskVm } from "@/lib/ipc/client";
import {
  syncPacedWork,
  syncProfiles,
  syncTaskForget,
  syncTaskHistory,
  syncTaskRunNow,
  syncTaskSave,
  syncTasks,
} from "@/lib/ipc/client";
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

beforeEach(() => {
  // Every form this pane reveals reads the folder list as it mounts, so a test
  // that opens one needs an answer here or the read never resolves.
  vi.mocked(syncProfiles).mockResolvedValue([]);
  // The projection is read in the same pass as the listing, so every test needs
  // an answer for it. `[]` and not a row: the 63 tests that predate Story 58.7
  // are about the task list, and a projected row in each of them would put a
  // second `Cadence` cell inside reach of their queries.
  answerPaced([]);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("the Tasks pane", () => {
  it("states, per row, the kind, schedule, host, next due, last run, outcome and report", async () => {
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
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
    const buttonFor = (id: string): HTMLElement => {
      const row = screen
        .getAllByTestId(TASKS_ROW_TESTID)
        .find((candidate) => candidate.dataset.taskId === id);
      expect(row, `row ${id}`).toBeDefined();
      return within(row as HTMLElement).getByRole("button", { name: TASK_RUN_NOW_TEXT });
    };

    fireEvent.click(buttonFor("A"));
    await waitFor(() => {
      expect(buttonFor("A")).toBeDisabled();
    });
    fireEvent.click(buttonFor("B"));
    await waitFor(() => {
      expect(buttonFor("B")).toBeDisabled();
    });
    // THE PROPERTY: starting B must not re-offer A, which is still running.
    expect(buttonFor("A")).toBeDisabled();

    settleA();
    await waitFor(() => {
      expect(buttonFor("A")).toBeEnabled();
    });
    // ...and A settling must not re-offer B either.
    expect(buttonFor("B")).toBeDisabled();
    settleB();
    await waitFor(() => {
      expect(buttonFor("B")).toBeEnabled();
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_FORGET_TEXT }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: TASK_FORGET_TEXT }));

    await waitFor(() => expect(syncTaskForget).toHaveBeenCalledWith("01SCHED"));
    await waitFor(() => expect(syncTasks).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.queryByTestId(TASKS_ROW_TESTID)).not.toBeInTheDocument());
    expect(screen.getByText(TASKS_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });

  it("shows a refused Forget on the row it is about, as a refused Run now is", async () => {
    // An `internal` store error, which is what this path can actually emit:
    // `sync_task_forget` runs two unconditional DELETEs and has no
    // does-this-exist branch, so a "no such task" refusal was an invented
    // failure. Two rows, so `within` is the assertion and not decoration.
    answer(listing({ tasks: [task(), task({ id: "01OTHER" })] }));
    vi.mocked(syncTaskForget).mockRejectedValue({
      code: "internal",
      message: "database is locked",
      accountId: null,
      retriable: false,
    });
    render(<TasksPane />);
    const rows = await screen.findAllByTestId(TASKS_ROW_TESTID);
    const mine = rows[1];

    fireEvent.click(within(mine).getByRole("button", { name: TASK_FORGET_TEXT }));
    fireEvent.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: TASK_FORGET_TEXT,
      }),
    );

    // On its own row and on no other: the refusal names which task did not go.
    await waitFor(() =>
      expect(
        within(screen.getAllByTestId(TASKS_ROW_TESTID)[1]).getByTestId(TASKS_REFUSAL_TESTID),
      ).toHaveTextContent("database is locked"),
    );
    expect(
      within(screen.getAllByTestId(TASKS_ROW_TESTID)[0]).queryByTestId(TASKS_REFUSAL_TESTID),
    ).not.toBeInTheDocument();
    // And the task is still there, because it was not deleted.
    expect(
      within(screen.getAllByTestId(TASKS_ROW_TESTID)[1]).getByText("01OTHER"),
    ).toBeInTheDocument();
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
    fireEvent.click(within(row).getByRole("button", { name: TASK_EDIT_TEXT }));
    const form = await screen.findByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` });

    fireEvent.click(within(form).getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    const live = screen.getByTestId(TASKS_ROW_TESTID);
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);
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
    const back = await screen.findByTestId(TASKS_ROW_TESTID);
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    expect(reportCells(await screen.findByTestId(TASKS_ROW_TESTID))).toBe(0);
    silent.unmount();

    answer(listing({ tasks: [task()] }));
    render(<TasksPane />);
    expect(reportCells(await screen.findByTestId(TASKS_ROW_TESTID))).toBe(1);
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
      const row = await screen.findByTestId(TASKS_ROW_TESTID);
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const [loud, silent] = screen.getAllByTestId(TASKS_ROW_TESTID);

    expect(
      within(loud).getByText("3 synced, 0 already syncing, 0 waiting, 0 failed"),
    ).toBeInTheDocument();
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

    fireEvent.click(within(row).getByRole("button", { name: TASK_RUN_NOW_TEXT }));
    await screen.findByTestId(TASKS_REFUSAL_TESTID);

    const after = screen.getByTestId(TASKS_ROW_TESTID);
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

  it("closes the first section when a second row is opened, and reads once for it", async () => {
    answer(listing({ tasks: [task({ id: "01SCHED" }), task({ id: "01OTHER" })] }));
    vi.mocked(syncTaskHistory).mockImplementation(async (id) => [
      run({ id: 1, detail: `${id}'s own run` }),
    ]);
    render(<TasksPane />);
    await waitFor(() => expect(screen.getAllByTestId(TASKS_ROW_TESTID)).toHaveLength(2));

    fireEvent.click(disclosure("01SCHED"));
    expect(await screen.findByText("01SCHED's own run")).toBeInTheDocument();

    fireEvent.click(disclosure("01OTHER"));
    expect(await screen.findByText("01OTHER's own run")).toBeInTheDocument();

    // One section open at a time, `editingId`'s rule: the first is gone rather
    // than scrolled apart from the row it belongs to.
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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = await screen.findByTestId(TASKS_ROW_TESTID);

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
    const row = screen.getByTestId(TASKS_ROW_TESTID);
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
    await screen.findAllByTestId(TASKS_ROW_TESTID);

    const rows = screen.getAllByTestId(TASKS_ROW_TESTID);
    expect(within(rows[0]).getByTestId(TASKS_DESCRIPTION_TESTID)).toHaveTextContent(
      "the photos, nightly",
    );
    // Asserted on the ELEMENT, not on its text. A text query cannot tell a
    // paragraph that was never rendered from one rendered around three spaces,
    // and that is the whole distinction here: mutating `taskDescriptionText` to
    // return its argument unchanged left a text-based version of this test
    // green, which made it a test that could not fail.
    expect(within(rows[1]).queryByTestId(TASKS_DESCRIPTION_TESTID)).toBeNull();
    expect(within(rows[2]).queryByTestId(TASKS_DESCRIPTION_TESTID)).toBeNull();
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
