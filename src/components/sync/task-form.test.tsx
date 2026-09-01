/**
 * The task form sends what was typed and shows what Rust said (Epic 58,
 * Story 58.1, FR-347, AD-C7).
 *
 * Every assertion here is one half of that sentence. The form's whole job is to
 * carry nine values across IPC without improving any of them and to render the
 * refusal verbatim when the store will not take them — so the tests check what
 * was *sent* as closely as what was shown, because a form that quietly trimmed
 * an id would pass a shallower version of all of this while storing a task under
 * a name nobody typed.
 *
 * Two of the eight arrived with Story 58.4: `onMissed`, the missed-window
 * policy, whose whole point is that it is writable from here and not only from
 * a terminal; and `baselineUpdatedMs`, which is not a value a person types at
 * all — it is the reading this form seeded from, sent back so the store can
 * refuse a save that would revert whatever another host moved meanwhile.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncTaskSave: vi.fn(),
  // The picker's read. It is the one defence against a `profileId` naming
  // nothing, which is the single refusal the backend does not make.
  syncProfiles: vi.fn(),
  // The schedule preview (Story 59.7). Mocked here rather than stubbed per test
  // because the form asks it on every change of the schedule box, so an edit
  // form seeded with a stored schedule asks it on mount — and a suite that left
  // it undefined would be exercising the effect's `catch` in every test that
  // touches the field.
  syncTaskSchedulePreview: vi.fn(),
}));

import {
  TASK_SCHEDULE_CEILING_DAYS,
  TASK_SCHEDULE_FLOOR_MINUTES,
  TASK_SCHEDULE_OFFERS,
  taskScheduleBoundsNote,
  taskSchedulePeriodPhrase,
} from "@/components/sync/schedule-offers";
import {
  TASK_FORM_ADD_SUBMIT_LABEL,
  TASK_FORM_ADD_TITLE,
  TASK_FORM_DESCRIPTION_LABEL,
  TASK_FORM_DESCRIPTION_NOTE,
  TASK_FORM_EDIT_SUBMIT_LABEL,
  TASK_FORM_EDIT_TITLE,
  TASK_FORM_ENABLED_LABEL,
  TASK_FORM_ERROR_TESTID,
  TASK_FORM_ID_ADD_NOTE,
  TASK_FORM_ID_EDIT_NOTE,
  TASK_FORM_ID_LABEL,
  TASK_FORM_KIND_LABEL,
  TASK_FORM_MISSED_DELAY_LABEL,
  TASK_FORM_MISSED_DELAY_NOT_A_NUMBER,
  TASK_FORM_MISSED_DELAY_NOTE,
  TASK_FORM_MODE_LABEL,
  TASK_FORM_ON_MISSED_LABEL,
  TASK_FORM_ON_MISSED_NOTE,
  TASK_FORM_PROFILE_LABEL,
  TASK_FORM_PROFILE_READ_FAILED_PREFIX,
  TASK_FORM_PROFILE_READING_NOTE,
  TASK_FORM_SCHEDULE_LABEL,
  TASK_FORM_SCHEDULE_OFFER_LABEL,
  TASK_FORM_SCHEDULE_OFFER_NOTE,
  TASK_FORM_SCHEDULE_OFFER_PLACEHOLDER,
  TASK_FORM_SCHEDULE_PREVIEW_TESTID,
  TASK_FORM_SCHEDULE_REFUSAL_PREFIX,
  TASK_HOST_WIDE_TEXT,
  TASK_MISSED_DELAY_MINUTES,
  TASK_MISSED_GRACE_MINUTES,
  TASK_SCHEDULE_BOUNDS_NOTE,
  TaskForm,
  taskFormMissedDelayMs,
  taskFormOnMissedNote,
  taskFormScheduleFiresNote,
  taskFormScheduleOfferText,
  taskFormUnlistedProfileText,
} from "@/components/sync/task-form";
import type { SyncProfileVm, TaskSchedulePreviewVm, TaskVm } from "@/lib/ipc/client";
import { syncProfiles, syncTaskSave, syncTaskSchedulePreview } from "@/lib/ipc/client";

const mockSave = vi.mocked(syncTaskSave);
const mockProfiles = vi.mocked(syncProfiles);
const mockPreview = vi.mocked(syncTaskSchedulePreview);

/** Rust's answer, as the verb shapes it: the echo, the refusal, the instants. */
function previewVm(over: Partial<TaskSchedulePreviewVm> = {}): TaskSchedulePreviewVm {
  return { expression: "", refusal: null, instants: [], ...over };
}

const NOW = 1_760_000_000_000;

function taskVm(over: Partial<TaskVm> = {}): TaskVm {
  return {
    id: "01SCHED",
    kind: "sync",
    mode: "scheduled",
    enabled: true,
    profileId: null,
    profile: null,
    schedule: "@daily",
    description: null,
    nextDueMs: NOW + 3_600_000,
    runningHost: null,
    leaseUntilMs: null,
    updatedMs: NOW - 60_000,
    onMissed: "run_now",
    missedDelayMs: null,
    lastRun: null,
    host: {
      kind: "app",
      sentence: "keeper runs this — only while keeper is running",
      reason: null,
    },
    ...over,
  };
}

/**
 * A folder as the picker receives it. Only the three keys the option uses are
 * interesting; the rest is what `SyncProfileVm` requires.
 */
function profileVm(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
  return {
    id: "01FOLDER",
    name: "field notes",
    localPath: "/Users/alice/notes",
    remoteUrl: "git@github.com:alice/notes.git",
    branch: "main",
    direction: "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 4 * 1024 * 1024,
    virtualPatterns: [],
    virtualOverBytes: 0,
    releaseTtlMs: 24 * 60 * 60 * 1000,
    folderOwned: [],
    settleMs: null,
    effectiveSettleMs: 5_000,
    pollIntervalMs: null,
    effectivePollIntervalMs: 15_000,
    tags: [],
    commitSubjectTemplate: "",
    notes: false,
    notesSubfolder: null,
    recordings: false,
    recordingsSubfolder: "recordings",
    sessions: false,
    sessionsSubfolder: "60-sessions",
    authorOverride: null,
    enabled: true,
    ...over,
  };
}

beforeEach(() => {
  mockProfiles.mockResolvedValue([]);
  // Echoing and empty by default: the echo keeps the staleness guard satisfied
  // for every test that is not about staleness, and no instants means no preview
  // paragraph — so a test that wants one asks for it explicitly and every other
  // test is unaffected by a control it does not care about.
  mockPreview.mockImplementation(async (expression: string) => previewVm({ expression }));
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("TaskForm, adding a task", () => {
  it("names itself for a screen reader where no heading is drawn beside it", async () => {
    render(<TaskForm />);

    expect(screen.getByRole("form", { name: TASK_FORM_ADD_TITLE })).toBeInTheDocument();
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());
  });

  it("sends an empty id so the backend mints the ULID", async () => {
    mockSave.mockResolvedValue(taskVm());
    const onSaved = vi.fn();
    render(<TaskForm onSaved={onSaved} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    // Chosen rather than defaulted into, so the assertion below is about what
    // the controls express and not about what the initial state happened to be.
    fireEvent.change(screen.getByLabelText(TASK_FORM_KIND_LABEL), { target: { value: "sync" } });
    fireEvent.change(screen.getByLabelText(TASK_FORM_MODE_LABEL), {
      target: { value: "scheduled" },
    });
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@daily" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    // Exactly these ten keys, and `id: ""` above all: an id invented here
    // would be a second minter, and `sync_ipc.rs` already has the only one.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith({
        id: "",
        kind: "sync",
        mode: "scheduled",
        enabled: true,
        profileId: null,
        schedule: "@daily",
        // Untouched, so absent — and `null` rather than `""`, which is the same
        // rule the schedule two lines up follows.
        description: null,
        onMissed: "run_now",
        // The control is not even on screen for `run_now`, so absence is the
        // only thing it could send — and absence means keeper's own delay, which
        // is what a task created before this box existed waited.
        missedDelayMs: null,
        // No reading to be stale: a create has no baseline, and passing one
        // would make the store refuse a row it is about to insert.
        baselineUpdatedMs: null,
      }),
    );
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(taskVm()));
  });

  it("sends no schedule as the absent value rather than as an empty string", async () => {
    // The one normalisation the form performs, and it is not tidying: the wire
    // type spells "store none" `null`, and `""` is a different thing. The
    // scheduled-with-no-schedule refusal is Rust's to make, so the combination
    // has to be expressible.
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        "invalid sync configuration: task '01SCHED' is scheduled with no schedule: it would report itself enabled and never run",
      accountId: null,
      retriable: false,
    });
    render(<TaskForm />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ schedule: null })),
    );
    expect(
      await screen.findByText(/is scheduled with no schedule: it would report itself enabled/),
    ).toBeInTheDocument();
  });

  it("sends a schedule of nothing but spaces verbatim, for Rust to refuse", async () => {
    // Finding 8 of this story's review. `.trim() === ""` treated a box holding
    // spaces as an absent schedule, so the task was stored with no schedule at
    // all where `TaskSchedule::parse` would have refused it and quoted what was
    // typed. That is the pre-validation this form disclaims, and it contradicted
    // the id, which is deliberately sent untrimmed for the very same reason.
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        'invalid sync configuration: task schedule must be a 5-field cron expression (minute hour day-of-month month day-of-week), one of @hourly, @daily or @weekly, or every <n><unit> with unit s/m/h/d, got ""',
      accountId: null,
      retriable: false,
    });
    render(<TaskForm />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), { target: { value: "  " } });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ schedule: "  " })),
    );
    expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
      "must be a 5-field cron expression",
    );
  });

  it("says the folder list is being read rather than showing one option", async () => {
    // The Tasks pane's rule one level down: before the first read has landed the
    // list is unknown, not empty. Without the note the picker held exactly
    // "the whole machine" for the length of the read and was indistinguishable
    // from a machine that syncs no folders, so somebody who opened it in that
    // window read "there is nothing to scope this to".
    mockProfiles.mockResolvedValue([profileVm()]);
    render(<TaskForm />);

    expect(screen.getByText(TASK_FORM_PROFILE_READING_NOTE)).toBeInTheDocument();

    await waitFor(() => expect(screen.getByText("field notes")).toBeInTheDocument());
    expect(screen.queryByText(TASK_FORM_PROFILE_READING_NOTE)).not.toBeInTheDocument();
  });

  it("admits in the id note that a taken id replaces rather than adds", async () => {
    // `upsert_task` has no create-only mode, so a memorable id typed twice
    // reconfigures the task that already has it. A control labelled "Add a task"
    // has to say so where the id is chosen; the alternative — checking the typed
    // id against the listing — would be this form deciding a rule Rust owns.
    render(<TaskForm />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.getByText(TASK_FORM_ID_ADD_NOTE)).toBeInTheDocument();
    expect(TASK_FORM_ID_ADD_NOTE).toMatch(/replaces that task/);
  });

  it("shows a rejected save inline, keeps every typed value, and reports no save", async () => {
    // A plain object, NOT an `Error`: Tauri maps a Rust `Err` to a *value*, and
    // `client.ts` normalises it into this envelope. A form that read it with
    // `instanceof Error` would render "[object Object]" where the refusal goes.
    mockSave.mockRejectedValue({
      code: "internal",
      message: "invalid sync configuration: this keeper does not know the task kind 'teleport'",
      accountId: null,
      retriable: false,
    });
    const onSaved = vi.fn();
    render(<TaskForm onSaved={onSaved} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_ID_LABEL), { target: { value: "nightly" } });
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "0 3 * * *" },
    });
    fireEvent.change(screen.getByLabelText(TASK_FORM_MODE_LABEL), { target: { value: "manual" } });
    fireEvent.click(screen.getByLabelText(TASK_FORM_ENABLED_LABEL));
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
      "this keeper does not know the task kind 'teleport'",
    );
    // Nothing typed is lost to a refusal, and no surface hides the form out from
    // under the sentence that says what to fix.
    expect(screen.getByLabelText(TASK_FORM_ID_LABEL)).toHaveValue("nightly");
    expect(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL)).toHaveValue("0 3 * * *");
    expect(screen.getByLabelText(TASK_FORM_MODE_LABEL)).toHaveValue("manual");
    expect(screen.getByLabelText(TASK_FORM_ENABLED_LABEL)).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("shows each refusal in the validator's own words and corrects nothing", async () => {
    // These sentences are `tasks::validate_id`'s and `TaskSchedule::parse`'s,
    // verbatim, as `SyncError::Config` renders them over the wire. The form must
    // not re-implement either rule — a second copy drifts toward accepting what
    // the store refuses — and must not tidy the input to make a save succeed,
    // which would store the task under something the person never typed.
    for (const [field, typed, refusal] of [
      [
        TASK_FORM_ID_LABEL,
        " nightly",
        'invalid sync configuration: task id must not begin or end with whitespace, got " nightly"',
      ],
      [
        TASK_FORM_SCHEDULE_LABEL,
        "every 30s",
        'invalid sync configuration: task schedule must not fire more often than once a minute (60000 ms), got "every 30s"',
      ],
    ] as const) {
      mockSave.mockRejectedValue({
        code: "internal",
        message: refusal,
        accountId: null,
        retriable: false,
      });
      const view = render(<TaskForm />);
      await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

      fireEvent.change(screen.getByLabelText(field), { target: { value: typed } });
      fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

      expect(await screen.findByText(refusal)).toBeInTheDocument();
      // What was typed is still what was sent, and still what is on screen: the
      // refusal named a rule, and the field it is about is the one to fix.
      const sent = mockSave.mock.calls[0]?.[0];
      expect(sent, refusal).toBeDefined();
      expect(field === TASK_FORM_ID_LABEL ? sent?.id : sent?.schedule).toBe(typed);
      expect(screen.getByLabelText(field)).toHaveValue(typed);
      view.unmount();
      // Only the call log: each iteration asserts about its own save.
      mockSave.mockClear();
    }
  });

  it("offers the whole machine, and the picked folder rather than a typed one", async () => {
    mockProfiles.mockResolvedValue([profileVm(), profileVm({ id: "01OTHER", name: "photos" })]);
    mockSave.mockResolvedValue(taskVm({ profileId: "01OTHER" }));
    render(<TaskForm />);
    const picker = screen.getByLabelText(TASK_FORM_PROFILE_LABEL);
    // Host-wide is the empty-string sentinel, which is why this is a native
    // `<select>` and not the Radix one.
    expect(picker).toHaveValue("");
    await waitFor(() => expect(screen.getByText("photos")).toBeInTheDocument());

    fireEvent.change(picker, { target: { value: "01OTHER" } });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ profileId: "01OTHER" })),
    );
  });

  it("stays usable when the folder read fails, and says what happened", async () => {
    mockProfiles.mockRejectedValue({
      code: "unsupported",
      message: "git is not usable on this machine",
      accountId: null,
      retriable: false,
    });
    mockSave.mockResolvedValue(taskVm());
    render(<TaskForm />);

    // The read's own sentence, beside the control it emptied: a picker that could
    // not be filled says so rather than silently offering one option.
    expect(
      await screen.findByText(
        `${TASK_FORM_PROFILE_READ_FAILED_PREFIX}git is not usable on this machine`,
      ),
    ).toBeInTheDocument();
    // Non-fatal: host-wide is still offered and the form still saves.
    expect(screen.getByText(TASK_HOST_WIDE_TEXT)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@daily" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ schedule: "@daily" })),
    );
  });
});

describe("TaskForm, editing a task", () => {
  it("starts from the stored task and saves back under its own id", async () => {
    const stored = taskVm({
      id: "01SCHED",
      kind: "release",
      mode: "manual",
      enabled: false,
      profileId: "01FOLDER",
      profile: "field notes",
      schedule: "0 3 * * *",
    });
    mockProfiles.mockResolvedValue([profileVm()]);
    mockSave.mockResolvedValue(stored);
    render(<TaskForm task={stored} />);

    // Named for the task it belongs to: several rows can have one open at once.
    expect(
      screen.getByRole("form", { name: `${TASK_FORM_EDIT_TITLE}: 01SCHED` }),
    ).toBeInTheDocument();
    // Every control arrives holding the stored value. One that opened on the add
    // form's defaults would silently rewrite the mode and the kind of a task
    // somebody came here to change the schedule of.
    expect(screen.getByLabelText(TASK_FORM_ID_LABEL)).toHaveValue("01SCHED");
    expect(screen.getByLabelText(TASK_FORM_ID_LABEL)).toHaveAttribute("readonly");
    expect(screen.getByLabelText(TASK_FORM_KIND_LABEL)).toHaveValue("release");
    expect(screen.getByLabelText(TASK_FORM_MODE_LABEL)).toHaveValue("manual");
    expect(screen.getByLabelText(TASK_FORM_ENABLED_LABEL)).toHaveAttribute(
      "data-state",
      "unchecked",
    );
    expect(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL)).toHaveValue("0 3 * * *");
    await waitFor(() =>
      expect(screen.getByLabelText(TASK_FORM_PROFILE_LABEL)).toHaveValue("01FOLDER"),
    );

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@weekly" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    // THE PROPERTY: the stored id goes back verbatim, so `upsert_task` updates
    // this row. A blank or a different id here would be a second task, and the
    // first one's run history would be orphaned rather than moved.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith({
        id: "01SCHED",
        kind: "release",
        mode: "manual",
        enabled: false,
        profileId: "01FOLDER",
        schedule: "@weekly",
        description: null,
        onMissed: "run_now",
        // Absent on the stored row, and this edit touched nothing about it.
        missedDelayMs: null,
        // The reading this form opened on, which is what makes the store's
        // refusal possible at all.
        baselineUpdatedMs: NOW - 60_000,
      }),
    );
  });

  it("never silently rescopes a task whose folder is gone", async () => {
    // A `<select>` whose value matches no option renders the FIRST one, and the
    // first one here is "the whole machine" — so a task scoped to a folder the
    // list does not contain would *report* itself as host-wide. A Save would not
    // even make that true: React's fallback selects the first option by mutating
    // the DOM and fires no `change`, so the stored id is still what goes over the
    // wire. Misinformed about the scope, and with no control able to express the
    // real one. This is the state the backend does NOT refuse: it stores the id,
    // the row comes back `unhosted`, and the run fails with "no such folder".
    const orphan = taskVm({ profileId: "01NOSUCH", profile: null });
    mockProfiles.mockResolvedValue([profileVm()]);
    mockSave.mockResolvedValue(orphan);
    render(<TaskForm task={orphan} />);
    await waitFor(() => expect(screen.getByText("field notes")).toBeInTheDocument());

    const picker = screen.getByLabelText(TASK_FORM_PROFILE_LABEL);
    expect(picker).toHaveValue("01NOSUCH");
    // And it says what it is, rather than showing an id that reads like a folder.
    expect(screen.getByText(taskFormUnlistedProfileText("01NOSUCH"))).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ profileId: "01NOSUCH" })),
    );
  });

  it("calls the stored folder by its name while the folder list is still unread", async () => {
    // The window between mount and the read landing is a frame the person sees,
    // and in it the list cannot yet say whether this folder exists. Dropping the
    // option for that window would be worse than saying the wrong thing: the
    // `<select>` would fall back to its first option and read as host-wide. So
    // the option is unconditional, and what it SAYS comes off the row —
    // `TaskVm.profile` is the name Rust resolved, `null` only when the id names
    // nothing — rather than off a read that has not happened. Accusing a folder
    // of being gone for as long as a read takes is a claim this form has no
    // basis for making.
    // Asserted before the first microtask checkpoint, which is exactly the frame
    // in question: `render` is synchronous, so the read's continuation has not
    // run yet however promptly it resolves.
    mockProfiles.mockResolvedValue([profileVm()]);
    const stored = taskVm({ profileId: "01FOLDER", profile: "field notes" });
    render(<TaskForm task={stored} />);

    const picker = screen.getByLabelText(TASK_FORM_PROFILE_LABEL);
    expect(picker).toHaveValue("01FOLDER");
    expect(screen.getByText("field notes")).toBeInTheDocument();
    expect(screen.queryByText(taskFormUnlistedProfileText("01FOLDER"))).not.toBeInTheDocument();

    // And once the list lands the folder is simply one of its options, with no
    // duplicate left behind.
    await waitFor(() => expect(screen.getAllByText("field notes")).toHaveLength(1));
    expect(picker).toHaveValue("01FOLDER");
  });

  it("keeps the gone folder offered after the picker has moved off it", async () => {
    // Finding 7 of this story's review. The option was keyed off the CURRENT
    // value, so selecting "the whole machine" to compare removed it in the same
    // render — and the gone id was then unrecoverable, because the whole design
    // is that a folder is picked and never typed. The only exit was Cancel, which
    // throws away every other edit in the form too.
    const orphan = taskVm({ profileId: "01NOSUCH", profile: null });
    mockProfiles.mockResolvedValue([profileVm()]);
    mockSave.mockResolvedValue(orphan);
    render(<TaskForm task={orphan} />);
    const picker = screen.getByLabelText(TASK_FORM_PROFILE_LABEL);
    await waitFor(() => expect(screen.getByText("field notes")).toBeInTheDocument());

    fireEvent.change(picker, { target: { value: "" } });
    expect(picker).toHaveValue("");
    expect(screen.getByText(taskFormUnlistedProfileText("01NOSUCH"))).toBeInTheDocument();

    // And going back to it is a selection, so the task keeps the scope it had.
    fireEvent.change(picker, { target: { value: "01NOSUCH" } });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ profileId: "01NOSUCH" })),
    );
  });
});

describe("TaskForm, the missed-window policy", () => {
  it("offers the three settings this build can write and sends the chosen spelling", async () => {
    // THE PROPERTY OF STORY 58.4, and it is a reachability property rather than
    // a behavioural one: a policy writable only from `keeper-syncd tasks set` is
    // born unreachable, which is the exact defect class this epic exists to
    // close. The vocabulary is the STORED spelling — `run_now`, not the
    // kebab-case `run-now` clap takes — because that is what crosses IPC.
    const stored = taskVm({ onMissed: "run_now" });
    mockSave.mockResolvedValue(taskVm({ onMissed: "skip" }));
    render(<TaskForm task={stored} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    const picker = screen.getByLabelText(TASK_FORM_ON_MISSED_LABEL);
    expect(picker).toHaveValue("run_now");
    expect(
      Array.from(picker.querySelectorAll("option")).map((option) => option.getAttribute("value")),
    ).toEqual(["run_now", "delay", "skip"]);

    fireEvent.change(picker, { target: { value: "skip" } });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ onMissed: "skip" })),
    );
  });

  it("arrives holding the stored policy rather than the add form's default", async () => {
    // The same trap the kind and the mode already guard: an edit form that
    // opened on the add form's defaults would silently rewrite the policy of a
    // task somebody came here to change the schedule of — and for the release
    // kind, rewriting `skip` to `run_now` is a deletion sweep at an instant
    // nobody chose.
    mockSave.mockResolvedValue(taskVm({ onMissed: "delay" }));
    render(<TaskForm task={taskVm({ onMissed: "delay" })} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.getByLabelText(TASK_FORM_ON_MISSED_LABEL)).toHaveValue("delay");

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@weekly" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ onMissed: "delay" })),
    );
  });

  it("renders the store's refusal when the row it seeded from has moved", async () => {
    // Closing `deferred-work.md:5044-5066`. The form seeds once — deliberately,
    // since re-syncing from the prop would overwrite what has been typed — so
    // every value it is about to send is as old as that seeding. Before the
    // store-side compare-and-set this save silently reverted whatever the other
    // host had changed and nothing on screen said so.
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        "invalid sync configuration: task '01SCHED' was changed elsewhere since this was opened (last written at 1760000090000, this edit started from 1759999940000): refusing to write stale values over it — re-read it and try again",
      accountId: null,
      retriable: false,
    });
    const onSaved = vi.fn();
    render(<TaskForm task={taskVm()} onSaved={onSaved} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@weekly" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    // Rust's sentence, corrected in no way, in the form that asked for it.
    expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
      /was changed elsewhere since this was opened/,
    );
    expect(onSaved).not.toHaveBeenCalled();
    // And every typed value survives, because the typed value is what a retry is
    // driven from.
    expect(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL)).toHaveValue("@weekly");
  });
});

describe("TaskForm, how long the delay is", () => {
  it("offers the delay box only for delay, and never hides a value it holds", async () => {
    // THE PROPERTY OF STORY 59.6's form half. `delay` is the one setting that
    // reads the number, so a box beside a `<select>` reading `skip` would invite
    // a value with no effect — and this form's whole history is about not telling
    // somebody something untrue.
    mockProfiles.mockResolvedValue([]);
    render(<TaskForm task={taskVm({ onMissed: "run_now" })} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.queryByLabelText(TASK_FORM_MISSED_DELAY_LABEL)).toBeNull();

    fireEvent.change(screen.getByLabelText(TASK_FORM_ON_MISSED_LABEL), {
      target: { value: "delay" },
    });
    const box = screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL);
    expect(box).toHaveValue("");
    expect(screen.getByText(TASK_FORM_MISSED_DELAY_NOTE)).toBeInTheDocument();

    // And the other half, which is not symmetry for its own sake: the store
    // keeps a stored delay across a policy change and the write door refuses an
    // incoherent one whatever the policy is, so a hidden non-empty box could
    // refuse a save with its cause off screen.
    fireEvent.change(box, { target: { value: "240" } });
    fireEvent.change(screen.getByLabelText(TASK_FORM_ON_MISSED_LABEL), {
      target: { value: "skip" },
    });
    expect(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL)).toHaveValue("240");
  });

  it("seeds from the stored value in minutes and sends it back in milliseconds", async () => {
    // The unit boundary, asserted in both directions in one test because it is
    // one claim: the box speaks minutes because every sentence about this setting
    // does, and the row speaks milliseconds because every instant on it does.
    const stored = taskVm({ onMissed: "delay", missedDelayMs: 4 * 3_600_000 });
    mockProfiles.mockResolvedValue([]);
    mockSave.mockResolvedValue(stored);
    render(<TaskForm task={stored} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL)).toHaveValue("240");

    fireEvent.change(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL), {
      target: { value: "90" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ missedDelayMs: 90 * 60_000 }),
      ),
    );
  });

  it("sends an empty box as absence, which is what means keeper's own delay", async () => {
    // `null`, never `TASK_MISSED_DELAY_MINUTES * 60_000`, and the difference is
    // not cosmetic: a row that CHOSE thirty minutes keeps thirty minutes if the
    // constant is ever retuned, and a row that chose nothing follows it. A form
    // that helpfully filled in the default would silently opt every task it
    // saved out of ever tracking the constant again.
    const stored = taskVm({ onMissed: "delay", missedDelayMs: 4 * 3_600_000 });
    mockProfiles.mockResolvedValue([]);
    mockSave.mockResolvedValue(taskVm({ onMissed: "delay" }));
    render(<TaskForm task={stored} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ missedDelayMs: null })),
    );
  });

  it("refuses a box that holds no number rather than sending absence", async () => {
    // The one refusal this form owns, and the reason it must: the wire type is
    // `number | null`, so there is no third state for "not a number". Reading it
    // as `null` would store *use keeper's default* while the person is looking at
    // what they typed — a control that reports one thing and does another, which
    // is the whole shape Story 59.6 exists to remove rather than add.
    mockProfiles.mockResolvedValue([]);
    render(<TaskForm task={taskVm({ onMissed: "delay" })} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    for (const nonsense of ["soon", "12abc", "1.5", ""]) {
      mockSave.mockClear();
      fireEvent.change(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL), {
        target: { value: nonsense },
      });
      fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));
      if (nonsense === "") {
        // The control case, in the same loop so the assertion above cannot be
        // passing because nothing ever saves: empty is absence and DOES save.
        await waitFor(() => expect(mockSave).toHaveBeenCalled());
        continue;
      }
      expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
        TASK_FORM_MISSED_DELAY_NOT_A_NUMBER,
      );
      expect(mockSave).not.toHaveBeenCalled();
      // And what was typed survives, because the typed value is what a
      // correction is driven from.
      expect(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL)).toHaveValue(nonsense);
    }
  });

  it("does not send a delay the bounds refuse, it renders Rust's refusal", async () => {
    // The bounds are Rust's, and this asserts that they STAY Rust's: nothing here
    // knows that fifteen minutes is the floor, and the sentence a person reads is
    // the one `validate_missed_delay_ms` wrote.
    mockProfiles.mockResolvedValue([]);
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        "invalid sync configuration: task missed-window delay must be at least the grace period (900000 ms), because the grace period is the interval that concludes nobody was home — a shorter delay would elapse before the window it holds back counted as missed, which is run_now wearing delay's name, got 300000 ms",
      accountId: null,
      retriable: false,
    });
    render(<TaskForm task={taskVm({ onMissed: "delay" })} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL), {
      target: { value: "5" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    // Sent, not pre-refused: five minutes is a number, and whether it is a
    // coherent delay is the write door's answer to give.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ missedDelayMs: 300_000 })),
    );
    expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
      /concludes nobody was home/,
    );
  });
});

describe("TaskForm, the task's description", () => {
  it("round-trips the stored description and sends what was typed, untrimmed", async () => {
    // THE PROPERTY OF STORY 59.5, and it has the same shape as the policy above:
    // a name writable only from `keeper-syncd tasks set --description` is born
    // unreachable. Two claims in one test because they are one claim from the
    // person's side — a box that does not arrive holding the stored value is a
    // box that silently un-names the task on the next save, and a box that
    // tidies what was typed stores something other than what is on screen.
    const stored = taskVm({ description: "nightly backup of the photos" });
    mockSave.mockResolvedValue(stored);
    render(<TaskForm task={stored} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    const box = screen.getByLabelText(TASK_FORM_DESCRIPTION_LABEL);
    expect(box).toHaveValue("nightly backup of the photos");

    // Padded on purpose. `id` is sent untrimmed so `validate_id`'s refusal can
    // quote it and `schedule` is sent untrimmed so `TaskSchedule::parse`'s can —
    // this field has no refusal behind it at all, so the only thing trimming
    // could do here is quietly edit a person's words. The note beside the box
    // promises exactly this, which is why it is asserted below.
    fireEvent.change(box, { target: { value: "  the photos, nightly  " } });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ description: "  the photos, nightly  " }),
      ),
    );
  });

  it("sends null rather than an empty string when the box is empty", async () => {
    // The wire type's absent value is `null`, and this is the one normalisation
    // the form performs — `schedule`'s rule, for its reason. It matters at the
    // far end: the store keeps `null` and `""` apart deliberately, so a form that
    // sent `""` for an untouched add would write every new task as *named
    // nothing* rather than as *unnamed*, and the column would then be unable to
    // tell a fresh task from one somebody had cleared.
    mockSave.mockResolvedValue(taskVm({ description: null }));
    render(<TaskForm />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.getByLabelText(TASK_FORM_DESCRIPTION_LABEL)).toHaveValue("");
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ description: null })),
    );
  });

  it("shows a refusal verbatim and keeps the description that was typed", async () => {
    // Nothing refuses a description — there is no vocabulary and no grammar in
    // it — so the refusal a person actually meets while naming a task comes from
    // somewhere else entirely: they opened this form, went to write a better
    // name for it, and another host moved the row meanwhile. The claim is that
    // the sentence arrives uncorrected AND that the words they had just typed
    // are still in the box, because retyping a name you already wrote is the
    // moment a person gives up on naming things.
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        "invalid sync configuration: task '01SCHED' was changed elsewhere since this was opened (last written at 1760000090000, this edit started from 1759999940000): refusing to write stale values over it — re-read it and try again",
      accountId: null,
      retriable: false,
    });
    const onSaved = vi.fn();
    render(<TaskForm task={taskVm()} onSaved={onSaved} />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    fireEvent.change(screen.getByLabelText(TASK_FORM_DESCRIPTION_LABEL), {
      target: { value: "the one that backs up the photos" },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_EDIT_SUBMIT_LABEL }));

    expect(await screen.findByTestId(TASK_FORM_ERROR_TESTID)).toHaveTextContent(
      "task '01SCHED' was changed elsewhere since this was opened (last written at 1760000090000, this edit started from 1759999940000): refusing to write stale values over it — re-read it and try again",
    );
    expect(onSaved).not.toHaveBeenCalled();
    expect(screen.getByLabelText(TASK_FORM_DESCRIPTION_LABEL)).toHaveValue(
      "the one that backs up the photos",
    );
  });

  it("tells the reader why this is the only name they can ever change", async () => {
    // The note carries a fact about the *id*, and it is the reason the field
    // exists: an add form sends `""` to have Rust mint a ULID, and an edit form
    // cannot change an id at all because `task_runs.task_id` joins on it. A
    // reader who does not know that keeps hunting for an editable name. Asserted
    // against the id notes rather than against a copy of the sentence, so the
    // two cannot come to disagree about which one is frozen.
    render(<TaskForm />);
    await waitFor(() => expect(mockProfiles).toHaveBeenCalled());

    expect(screen.getByText(TASK_FORM_DESCRIPTION_NOTE)).toBeInTheDocument();
    expect(TASK_FORM_DESCRIPTION_NOTE).toContain("the only name of this task you can ever change");
    expect(TASK_FORM_DESCRIPTION_NOTE).toContain("sent exactly as typed");
    // The two rules it is quoting, each stated where it is actually enforced.
    expect(TASK_FORM_ID_ADD_NOTE).toContain("Leave it blank and keeper mints one");
    expect(TASK_FORM_ID_EDIT_NOTE).toContain("The id cannot change");
  });
});

/**
 * Help for writing a schedule (Story 59.7, FR-368).
 *
 * Two claims, and the second one is the dangerous half. The first is that the
 * dialect is *offered*: a menu of expressions that get typed into the box, so
 * nobody has to hold five-field cron in their head. The second is that the form
 * shows when the typed expression will actually fire — and the only acceptable
 * source for that is Rust, because a preview computed in the browser would need
 * the dialect, the calendar, vixie's day rule and the zone, and the first of
 * those to drift would make this form promise an instant the engine had no
 * intention of keeping.
 *
 * So the assertions below are mostly about *provenance*: the rendered instants
 * are the ones the view model carried, the rendered refusal is the sentence Rust
 * sent, and neither is recomputed, reworded or second-guessed here. The
 * corresponding claim on the Rust side — that the offered expressions are ones
 * `TaskSchedule::parse` accepts, and that the instants are its own chained
 * cadence — is asserted in `keeper-sync/src/tasks.rs`, because only Rust can run
 * the parser.
 */
describe("TaskForm, help for writing a schedule", () => {
  it("offers every form the dialect accepts, expression first", async () => {
    render(<TaskForm />);
    const menu = await screen.findByLabelText(TASK_FORM_SCHEDULE_OFFER_LABEL);

    // The option list is the offered list, in order, and each option leads with
    // the expression — which is the whole mechanism by which somebody stops
    // needing the menu.
    expect([...menu.querySelectorAll("option")].map((option) => option.textContent)).toStrictEqual([
      TASK_FORM_SCHEDULE_OFFER_PLACEHOLDER,
      ...TASK_SCHEDULE_OFFERS.map((offer) => taskFormScheduleOfferText(offer)),
    ]);
    // Sized rather than merely non-empty: a list that shrank to one entry would
    // still pass a per-option assertion while having stopped being help.
    expect(TASK_SCHEDULE_OFFERS.length).toBeGreaterThanOrEqual(5);
    // Every one of the three shapes of the dialect is reachable from the menu,
    // because the point is to teach the grammar and not one corner of it. The
    // `@` aliases and `every <n><unit>` are the two nobody can guess.
    const offered = TASK_SCHEDULE_OFFERS.map((offer) => offer.expression);
    expect(offered.some((expression) => expression.startsWith("@"))).toBe(true);
    expect(offered.some((expression) => expression.startsWith("every "))).toBe(true);
    expect(offered.some((expression) => expression.split(" ").length === 5)).toBe(true);
    expect(screen.getByText(TASK_FORM_SCHEDULE_OFFER_NOTE)).toBeInTheDocument();
  });

  it("types the chosen form into the box and keeps claiming nothing itself", async () => {
    render(<TaskForm />);
    const menu = await screen.findByLabelText(TASK_FORM_SCHEDULE_OFFER_LABEL);
    const box = screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL);
    const chosen = TASK_SCHEDULE_OFFERS[2];

    fireEvent.change(menu, { target: { value: chosen.expression } });

    expect(box).toHaveValue(chosen.expression);
    // And the menu is back at its placeholder rather than showing what was
    // picked. It is an action, not a second view of the value: a `<select>` that
    // appeared to mirror the box would be claiming to know which option some
    // hand-written expression is, and for anything typed the answer is none of
    // them.
    expect(menu).toHaveValue("");
  });

  it("sends what the box holds after a choice, edits included", async () => {
    const onSaved = vi.fn();
    mockSave.mockResolvedValue(taskVm());
    render(<TaskForm onSaved={onSaved} />);
    const menu = await screen.findByLabelText(TASK_FORM_SCHEDULE_OFFER_LABEL);

    fireEvent.change(menu, { target: { value: "@daily" } });
    // Edited afterwards, which is the promise the note makes: the menu is a
    // starting point and the box is still the only place the schedule lives.
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "0 3 * * 1 " },
    });
    fireEvent.click(screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL }));

    // Verbatim, trailing space and all — nothing about offering the dialect
    // turned this form into something that tidies input.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ schedule: "0 3 * * 1 " })),
    );
  });

  it("shows the instants Rust computed, and computes none of its own", async () => {
    // Deliberately NOT the instants `0 3 * * *` really fires at: they are three
    // arbitrary numbers, and they still appear on screen. That is the assertion —
    // this form renders what the engine sent rather than what it would itself
    // have worked out, so a form that quietly recomputed the answer would fail
    // here even though its arithmetic was "right".
    const instants = [NOW + 61_000, NOW + 987_654, NOW + 3_600_000];
    mockPreview.mockResolvedValue(previewVm({ expression: "0 3 * * *", instants }));
    render(<TaskForm />);

    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "0 3 * * *" },
    });

    expect(await screen.findByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toHaveTextContent(
      taskFormScheduleFiresNote(instants),
    );
    expect(mockPreview).toHaveBeenCalledWith("0 3 * * *");
    // Each instant stamped as itself, so a sentence that dropped or reordered one
    // is caught. Composed with the same `Date` the component uses rather than
    // hard-coded, for the reason `recording-row.test.tsx` composes its own: a
    // literal here would assert the test machine's time zone.
    for (const instant of instants) {
      expect(screen.getByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toHaveTextContent(
        new Date(instant).toLocaleString(),
      );
    }
    // The count is never named, so a shorter list is stated as itself rather than
    // as two thirds of a promise.
    expect(taskFormScheduleFiresNote([instants[0]])).toBe(
      `Next: ${new Date(instants[0]).toLocaleString()}`,
    );
  });

  it("shows a refusal in Rust's own words, and refuses nothing itself", async () => {
    // The epic's own example, and the sentence is `TaskSchedule::parse`'s
    // verbatim — the same one `sync_ipc_error` puts in `message` when a save is
    // refused, because both come off the same `Display`.
    const refusal =
      'invalid sync configuration: task schedule matches no instant, got "0 0 30 2 *"';
    mockPreview.mockResolvedValue(previewVm({ expression: "0 0 30 2 *", refusal }));
    mockSave.mockResolvedValue(taskVm());
    render(<TaskForm />);
    const box = screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL);

    fireEvent.change(box, { target: { value: "0 0 30 2 *" } });

    const preview = await screen.findByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID);
    expect(preview).toHaveTextContent(`${TASK_FORM_SCHEDULE_REFUSAL_PREFIX}${refusal}`);
    // Not an alert, and not the form's error paragraph: this is help about text
    // somebody is still writing, and the error paragraph is for a save that
    // actually happened.
    expect(preview).not.toHaveAttribute("role", "alert");
    expect(screen.queryByTestId(TASK_FORM_ERROR_TESTID)).toBeNull();

    // What was typed is still on screen and unchanged: the refusal is beside the
    // box, not instead of it.
    expect(box).toHaveValue("0 0 30 2 *");
    // And nothing is prevented. Save is live and sends exactly what is there —
    // this form does not pre-empt a refusal, which is the rule its own header
    // states and the reason there is no client-side validator here.
    const submit = screen.getByRole("button", { name: TASK_FORM_ADD_SUBMIT_LABEL });
    expect(submit).toBeEnabled();
    fireEvent.click(submit);
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ schedule: "0 0 30 2 *" })),
    );
  });

  it("asks nothing about an empty box, because empty is a choice", async () => {
    render(<TaskForm />);
    await screen.findByLabelText(TASK_FORM_SCHEDULE_OFFER_LABEL);

    expect(mockPreview).not.toHaveBeenCalled();
    expect(screen.queryByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeNull();

    // Typed, then cleared: the preview goes with it rather than lingering over a
    // box that now means *store no schedule*.
    const box = screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL);
    mockPreview.mockResolvedValue(previewVm({ expression: "@daily", instants: [NOW + 1] }));
    fireEvent.change(box, { target: { value: "@daily" } });
    await screen.findByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID);
    fireEvent.change(box, { target: { value: "" } });
    await waitFor(() => expect(screen.queryByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeNull());

    // Whitespace is NOT empty, and is asked about — the same `=== ""` rule the
    // save uses for this field, so the preview and the save agree about which
    // strings are a schedule at all.
    fireEvent.change(box, { target: { value: " " } });
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith(" "));
  });

  it("never shows an answer about text the box no longer holds", async () => {
    // Two independent ways a preview can end up describing a string that is no
    // longer on screen, and each has its own guard. Both are asserted here,
    // because each guard is invisible while the other one holds.
    const settle: Array<() => void> = [];
    mockPreview.mockImplementation(
      (expression: string) =>
        new Promise<TaskSchedulePreviewVm>((resolve) => {
          settle.push(() => resolve(previewVm({ expression, instants: [NOW + 60_000] })));
        }),
    );
    render(<TaskForm />);
    const box = screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL);

    // (1) THE ALREADY-ANSWERED PREVIOUS EXPRESSION. `@daily`'s answer has landed
    // and is on screen. The moment the box changes, that answer is about text
    // nobody is looking at — and it is still sitting in state, because a
    // keystroke cannot un-answer a read that already completed. So the preview
    // must go blank until the new answer arrives, and the guard that blanks it is
    // the echo comparison rather than any cancellation: there is nothing left to
    // cancel.
    fireEvent.change(box, { target: { value: "@daily" } });
    settle[0]();
    expect(await screen.findByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toHaveTextContent(
      taskFormScheduleFiresNote([NOW + 60_000]),
    );

    fireEvent.change(box, { target: { value: "0 0 30 2 *" } });
    // Nothing, immediately — not `@daily`'s instants under an expression that
    // fires at no instant at all.
    expect(screen.queryByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeNull();
    settle[1]();
    await waitFor(() =>
      expect(screen.getByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeInTheDocument(),
    );

    // (2) THE OUT-OF-ORDER REPLY, which a happy-path mock can never show: the
    // answer about the half-typed `0 3 * * ` is slower than the answer about the
    // finished `0 3 * * *`, so it lands last. Rendering it would paint a refusal
    // over an expression that is perfectly good. The superseded read is abandoned
    // by the effect's cleanup, so it never reaches state at all.
    settle.length = 0;
    mockPreview.mockImplementation(
      (expression: string) =>
        new Promise<TaskSchedulePreviewVm>((resolve) => {
          settle.push(() =>
            resolve(
              expression === "0 3 * * *"
                ? previewVm({ expression, instants: [NOW + 120_000] })
                : previewVm({
                    expression,
                    refusal:
                      'invalid sync configuration: task schedule must be a 5-field cron expression, got "0 3 * * "',
                  }),
            ),
          );
        }),
    );
    fireEvent.change(box, { target: { value: "0 3 * * " } });
    fireEvent.change(box, { target: { value: "0 3 * * *" } });
    // Newest first, then the stale one.
    settle[1]();
    settle[0]();

    await waitFor(() =>
      expect(screen.getByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toHaveTextContent(
        taskFormScheduleFiresNote([NOW + 120_000]),
      ),
    );
    expect(screen.getByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).not.toHaveTextContent(
      "must be a 5-field cron",
    );
  });

  it("says nothing rather than something wrong when there is nothing to say", async () => {
    // A read that failed is silent. The preview is a hint; a second error
    // paragraph because a hint did not arrive would be worse than no hint.
    mockPreview.mockRejectedValue({
      code: "internal",
      message: "the engine is not running",
      accountId: null,
      retriable: false,
    });
    render(<TaskForm />);
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@daily" },
    });

    await waitFor(() => expect(mockPreview).toHaveBeenCalled());
    expect(screen.queryByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeNull();
    expect(screen.queryByTestId(TASK_FORM_ERROR_TESTID)).toBeNull();

    // And an expression that parsed but yielded no instant — possible in
    // principle, because Rust's search window is finite — renders nothing rather
    // than an empty "Next:" or a sentence about a search window that nobody can
    // ever check.
    mockPreview.mockResolvedValue(previewVm({ expression: "@weekly", instants: [] }));
    fireEvent.change(screen.getByLabelText(TASK_FORM_SCHEDULE_LABEL), {
      target: { value: "@weekly" },
    });
    await waitFor(() => expect(mockPreview).toHaveBeenCalledWith("@weekly"));
    expect(screen.queryByTestId(TASK_FORM_SCHEDULE_PREVIEW_TESTID)).toBeNull();
  });
});

/**
 * The note under *If a window is missed* states the numbers Rust actually uses
 * (Story 58.9).
 *
 * This exists because the note was **wrong when it shipped**, in Story 56.13's
 * shape: prose that sits far from the code it describes. Story 58.4 wrote
 * *"delay serves it no sooner than fifteen minutes after it fell due"*; the
 * review rejected that anchor and the fix moved it to the instant a host
 * noticed the window, with a separate and longer constant. `TASK_MISSED_DELAY_MS`
 * and `tasks::decide`'s doc were updated; this string was not, and a wrong
 * number survived a review pass and a full gate because nothing compared the
 * two.
 *
 * So the comparison is mechanical, and in this direction because it is the only
 * one available: the Rust constants cannot import a TypeScript literal, and no
 * ts-rs binding carries them. Reading Rust source from a frontend test is this
 * repo's existing idiom for an invariant about a Rust file — `task-host-tick.test.ts`,
 * `command-registration.test.ts`, `tray-notes-labels.test.ts`.
 *
 * **Re-pointed by Story 59.6, not deleted.** Once a task may carry its own delay
 * the sentence has to be composed from the *effective* value, and a guard that
 * only ever checked one fixed string would have stopped covering the sentence
 * most people read. So it now pins two things: the DEFAULT composition against
 * Rust's constants — still the only mechanical direction available — and the
 * composed one against the value it was given. The second half is what catches a
 * note that ignores its argument, which is the 59.6-shaped way to reintroduce
 * exactly the defect above.
 *
 * **What this does NOT prove:** that `decide` behaves as the sentence says.
 * `keeper-sync`'s own tests own that. This proves the sentence a person reads
 * carries the numbers that code is compiled with.
 */
describe("the missed-window note states the numbers Rust actually uses", () => {
  const TASKS_RS = resolve(
    import.meta.dirname,
    "../../../src-tauri/crates/keeper-sync/src/tasks.rs",
  );

  /** `pub const NAME: i64 = <n> * 60_000;` → `<n>`. */
  const rustMinutes = (name: string): number => {
    const source = readFileSync(TASKS_RS, "utf8");
    const match = new RegExp(`pub const ${name}: i64 = (\\d+) \\* 60_000;`).exec(source);
    if (match === null) {
      throw new Error(
        `${name} is not a whole number of minutes in tasks.rs any more, so this note's \
wording needs rewriting rather than this regex widening`,
      );
    }
    return Number(match[1]);
  };

  it("mirrors TASK_MISSED_GRACE_MS and TASK_MISSED_DELAY_MS", () => {
    expect(TASK_MISSED_GRACE_MINUTES).toBe(rustMinutes("TASK_MISSED_GRACE_MS"));
    expect(TASK_MISSED_DELAY_MINUTES).toBe(rustMinutes("TASK_MISSED_DELAY_MS"));
  });

  it("states each number in its role, and states the delay's anchor", () => {
    // In its role, not merely present: the first draft of the equivalent guard
    // on the CLI's `--help` asserted only that the digits appeared, and passed
    // on a sentence saying *fifteen* minutes — because the clause contrasting it
    // with the wrong anchor also mentioned thirty. The number and the instant it
    // is measured from are one claim.
    expect(TASK_FORM_ON_MISSED_NOTE).toContain(
      `concludes after ${TASK_MISSED_GRACE_MINUTES} minutes`,
    );
    expect(TASK_FORM_ON_MISSED_NOTE).toContain(
      `runs it ${TASK_MISSED_DELAY_MINUTES} minutes after a host noticed it`,
    );
    // The sentence the old one got wrong, asserted as itself: a reader who takes
    // the anchor to be the window reads `delay` as `run_now` for any absence
    // longer than the delay.
    expect(TASK_FORM_ON_MISSED_NOTE).toContain("the anchor is the noticing and not the window");
  });

  it("composes the delay's number from the value it is given, not from the constant", () => {
    // The 59.6 half, and each number still asserted IN ITS ROLE for the reason
    // above. A note that ignored its argument would pass a bare
    // `toContain("240")` — the grace's 15 and the anchor clause are in the same
    // sentence — so the assertion is the number together with the instant it is
    // measured from.
    const composed = taskFormOnMissedNote(240);
    expect(composed).toContain("runs it 240 minutes after a host noticed it");
    expect(composed).not.toContain(
      `runs it ${TASK_MISSED_DELAY_MINUTES} minutes after a host noticed it`,
    );
    // The grace is NOT a parameter: one boundary for the whole policy, which no
    // task may move, and a per-task grace would make "nobody was home" mean
    // different things on two rows in one store.
    expect(composed).toContain(`concludes after ${TASK_MISSED_GRACE_MINUTES} minutes`);
    expect(composed).toContain(`those ${TASK_MISSED_GRACE_MINUTES} minutes`);
    // And the default composition is exactly this function at the constant, so
    // the mirror above is a claim about the sentence a real form renders.
    expect(TASK_FORM_ON_MISSED_NOTE).toBe(taskFormOnMissedNote(TASK_MISSED_DELAY_MINUTES));
  });

  it("is the sentence the form actually renders, at the default and at a chosen value", () => {
    // The constants could be right and the note could be right while the form
    // rendered some other string. One assertion closes that — twice, because
    // since 59.6 there are two sentences a person can be shown and only the
    // second one is new.
    mockProfiles.mockResolvedValue([]);
    render(<TaskForm onSaved={vi.fn()} />);
    expect(screen.getByText(TASK_FORM_ON_MISSED_NOTE)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(TASK_FORM_ON_MISSED_LABEL), {
      target: { value: "delay" },
    });
    fireEvent.change(screen.getByLabelText(TASK_FORM_MISSED_DELAY_LABEL), {
      target: { value: "240" },
    });
    expect(screen.getByText(taskFormOnMissedNote(240))).toBeInTheDocument();
    expect(screen.queryByText(TASK_FORM_ON_MISSED_NOTE)).toBeNull();
  });

  it("converts minutes to the wire's milliseconds, and keeps absence apart from zero", () => {
    // The tri-state is the contract: `null` is *use the default*, `undefined` is
    // *that is not a number*, and reading either as the other is the failure this
    // whole story is about. Asserted directly because a test driving only the
    // rendered box cannot tell them apart.
    expect(taskFormMissedDelayMs("")).toBeNull();
    expect(taskFormMissedDelayMs("240")).toBe(240 * 60_000);
    // Zero and a box of spaces both convert to a number the write door refuses,
    // rather than to absence. `Number(" ")` being 0 is JavaScript's, and keeping
    // it is the schedule field's `=== ""`-not-`.trim()` rule applied here: a box
    // holding spaces is not an empty box, and coercing it to *use the default*
    // would store something other than what is on screen.
    expect(taskFormMissedDelayMs("0")).toBe(0);
    expect(taskFormMissedDelayMs(" ")).toBe(0);
    for (const nonsense of ["soon", "12abc", "1.5", "1e999", "-"]) {
      expect(taskFormMissedDelayMs(nonsense)).toBeUndefined();
    }
  });
});

/**
 * The schedule bounds note states the numbers Rust actually refuses at
 * (Story 59.7).
 *
 * The third member of a family, and the family exists because of one shipped
 * defect: Story 58.4 wrote a note claiming fifteen minutes against a
 * thirty-minute constant, and it survived a review and a full gate because
 * nothing compared the two (see the guard above). Story 59.7 adds a sentence
 * naming a floor and a ceiling, which is exactly the same exposure — so it
 * arrives with the same mechanical comparison, in the same direction and for the
 * same reason: Rust cannot import a TypeScript literal, and no ts-rs binding
 * carries either constant.
 *
 * The two halves are separate claims. That the mirrored numbers equal Rust's is
 * the first; that the *sentence* is composed from them rather than typed beside
 * them is the second, and only the second catches a note that goes on saying
 * "once a minute" after the floor has moved.
 *
 * **What this does NOT prove:** that the parser refuses at those bounds.
 * `keeper-sync`'s own tests own that, boundary by boundary. This proves the
 * sentence a person reads carries the numbers that code is compiled with.
 */
describe("the schedule bounds note states the numbers Rust refuses at", () => {
  const TASKS_RS = resolve(
    import.meta.dirname,
    "../../../src-tauri/crates/keeper-sync/src/tasks.rs",
  );

  /** `MIN_SCHEDULE_INTERVAL_MS: i64 = 60_000;` → `60000`. */
  const rustIntervalMs = (name: string): number => {
    const source = readFileSync(TASKS_RS, "utf8");
    // The floor is `pub`, the ceiling is not, and the ceiling is written as a
    // product of days. Both spellings are matched here rather than normalised in
    // Rust, because the Rust side is written for the reader who has to believe
    // the number and this side is written for a regex.
    const literal = new RegExp(`${name}: i64 = ([0-9_ *]+);`).exec(source);
    if (literal === null) {
      throw new Error(
        `${name} is not an integer expression in tasks.rs any more, so this note's \
wording needs rewriting rather than this regex widening`,
      );
    }
    return literal[1]
      .split("*")
      .map((factor) => Number(factor.trim().split("_").join("")))
      .reduce((product, factor) => product * factor, 1);
  };

  it("mirrors MIN_SCHEDULE_INTERVAL_MS and MAX_SCHEDULE_INTERVAL_MS", () => {
    expect(TASK_SCHEDULE_FLOOR_MINUTES).toBe(rustIntervalMs("MIN_SCHEDULE_INTERVAL_MS") / 60_000);
    expect(TASK_SCHEDULE_CEILING_DAYS).toBe(
      rustIntervalMs("MAX_SCHEDULE_INTERVAL_MS") / 86_400_000,
    );
    // Whole units, because the sentence says "minute" and "day" rather than
    // milliseconds. If either constant ever stops dividing exactly, the wording
    // needs rewriting and this is where that is noticed.
    expect(Number.isInteger(TASK_SCHEDULE_FLOOR_MINUTES)).toBe(true);
    expect(Number.isInteger(TASK_SCHEDULE_CEILING_DAYS)).toBe(true);
  });

  it("composes both bounds from the constants rather than naming them", () => {
    // Each in its role, which is the lesson the missed-window guard's own comment
    // records: a bare `toContain("366")` would pass on a sentence that had the
    // floor and the ceiling the wrong way round.
    expect(TASK_SCHEDULE_BOUNDS_NOTE).toContain(
      `more often than ${taskSchedulePeriodPhrase(TASK_SCHEDULE_FLOOR_MINUTES, "minute")}`,
    );
    expect(TASK_SCHEDULE_BOUNDS_NOTE).toContain(
      `less often than ${taskSchedulePeriodPhrase(TASK_SCHEDULE_CEILING_DAYS, "day")}`,
    );

    // The half that catches a literal: at other numbers the sentence is a
    // different sentence. A note typed with today's values would pass every
    // assertion above and fail these two.
    const retuned = taskScheduleBoundsNote(5, 30);
    expect(retuned).toContain("more often than once every 5 minutes");
    expect(retuned).toContain("less often than once every 30 days");
    expect(retuned).not.toContain(taskSchedulePeriodPhrase(TASK_SCHEDULE_FLOOR_MINUTES, "minute"));
    expect(retuned).not.toContain(taskSchedulePeriodPhrase(TASK_SCHEDULE_CEILING_DAYS, "day"));
    // And the rendered note is exactly this function at Rust's numbers, so the
    // mirror above is a claim about the sentence a real form shows.
    expect(TASK_SCHEDULE_BOUNDS_NOTE).toBe(
      taskScheduleBoundsNote(TASK_SCHEDULE_FLOOR_MINUTES, TASK_SCHEDULE_CEILING_DAYS),
    );
  });

  it("is the sentence the form actually renders", async () => {
    render(<TaskForm />);
    expect(await screen.findByText(TASK_SCHEDULE_BOUNDS_NOTE)).toBeInTheDocument();
  });

  it("names a singular period without a bare 1 in front of it", () => {
    // The pluraliser is the seam this guard leans on, so it is asserted rather
    // than assumed: "once a minute" at one, "once every N minutes" above one.
    expect(taskSchedulePeriodPhrase(1, "minute")).toBe("once a minute");
    expect(taskSchedulePeriodPhrase(366, "day")).toBe("once every 366 days");
  });
});
