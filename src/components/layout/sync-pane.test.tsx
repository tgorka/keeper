import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The shared profile/status mirror.
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  syncProfileSave: vi.fn(),
  syncProfileRemove: vi.fn(),
  syncProfileSetEnabled: vi.fn(),
  syncFolderNow: vi.fn(),
  syncVerify: vi.fn(),
  // The three detail reads plus the parked-unit retry (Story 32.4).
  syncActivity: vi.fn(),
  syncPending: vi.fn(),
  syncProblems: vi.fn(),
  syncRetryParked: vi.fn(),
  // The progress stream, the only source of in-flight counters.
  syncSubscribeProgress: vi.fn(),
  syncUnsubscribeProgress: vi.fn(),
  // The one-time verified copy (Story 33.3): start, poll, stop.
  copyStart: vi.fn(),
  copyStatus: vi.fn(),
  copyCancel: vi.fn(),
  // The shared add-folder form's keychain calls (Story 32.7, Story 34.4).
  syncSetCredential: vi.fn(),
  syncGetCredential: vi.fn(),
  syncClearCredential: vi.fn(),
}));

// The shared add-folder form opens the native directory picker; mock it so
// mounting never reaches Tauri.
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve(null)),
}));

import { open as openFolder } from "@tauri-apps/plugin-dialog";
import {
  COPY_CURRENT_LABEL,
  COPY_DESTINATION_TESTID,
  COPY_NOTHING_SENTENCE,
  COPY_PICK_DESTINATION_LABEL,
  COPY_PICK_SOURCE_FOLDER_LABEL,
  COPY_PROGRESS_LABEL,
  COPY_REPLACE_LABEL,
  COPY_REPLACE_NOTE,
  COPY_REPORT_TESTID,
  COPY_RESULT_TITLE,
  COPY_SOURCE_TESTID,
  COPY_STOP_LABEL,
  COPY_STOPPED_SENTENCE,
  COPY_SUBMIT_LABEL,
  copyProgressSentence,
  copySummarySentence,
  formatCopyBytes,
  formatSyncWaited,
  SYNC_ACTIVITY_EMPTY_SENTENCE,
  SYNC_ACTIVITY_TITLE,
  SYNC_CONFLICT_SENTENCE,
  SYNC_CONFLICT_TITLE,
  SYNC_PANE_EMPTY_SENTENCE,
  SYNC_PARKED_NO_ERROR_SENTENCE,
  SYNC_PARKED_TITLE,
  SYNC_PENDING_EMPTY_SENTENCE,
  SYNC_PENDING_TITLE,
  SYNC_PROBLEMS_TITLE,
  SYNC_RETRY_LABEL,
  SYNC_SETTLING_NOTE,
  SYNC_SETTLING_SENTENCE,
  SyncPane,
  syncParkedSummary,
  syncPendingReason,
} from "@/components/layout/sync-pane";
import {
  SYNC_NOW_LABEL,
  SYNC_PAUSE_LABEL,
  SYNC_PROGRESS_LABEL,
  SYNC_REMOVE_CANCEL_LABEL,
  SYNC_REMOVE_CONFIRM_LABEL,
  SYNC_REMOVE_CONFIRM_SENTENCE,
  SYNC_REMOVE_LABEL,
  SYNC_RESUME_LABEL,
} from "@/components/settings/sync-section";
import {
  SYNC_ADD_SUBMIT_LABEL,
  SYNC_ADD_TITLE,
  SYNC_ADVANCED_TOGGLE_TESTID,
  SYNC_CHOOSE_FOLDER_LABEL,
  SYNC_EDIT_SUBMIT_LABEL,
  SYNC_EDIT_TITLE,
  SYNC_FORM_CANCEL_LABEL,
  SYNC_FORM_PATH_TESTID,
  SYNC_NAME_LABEL,
  SYNC_NO_FOLDER_CHOSEN_LABEL,
  SYNC_REMOTE_URL_LABEL,
  SYNC_TOKEN_FAILED_PREFIX,
  SYNC_TOKEN_LABEL,
} from "@/components/sync/add-folder-form";
import type {
  CopyJobVm,
  SyncActivityVm,
  SyncOutcomeVm,
  SyncPendingVm,
  SyncProblemsVm,
  SyncProfileVm,
  SyncProgressVm,
  SyncStatusVm,
} from "@/lib/ipc/client";
import {
  copyCancel,
  copyStart,
  copyStatus,
  syncActivity,
  syncFolderNow,
  syncPending,
  syncProblems,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncRetryParked,
  syncSetCredential,
  syncStatuses,
  syncSubscribeProgress,
  syncUnsubscribeProgress,
} from "@/lib/ipc/client";
import {
  COPY_POLL_MS,
  copyEntryGroups,
  copyJobStore,
  resetCopyJobStoreForTest,
} from "@/lib/stores/copy-job";
import { resetSyncStoreForTest, syncStore } from "@/lib/stores/sync";
import {
  refreshSyncDetail,
  resetSyncDetailStoreForTest,
  syncLiveFraction,
  syncLiveRate,
} from "@/lib/stores/sync-detail";

const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockActivity = vi.mocked(syncActivity);
const mockPending = vi.mocked(syncPending);
const mockProblems = vi.mocked(syncProblems);
const mockRetryParked = vi.mocked(syncRetryParked);
const mockFolderNow = vi.mocked(syncFolderNow);
const mockSetEnabled = vi.mocked(syncProfileSetEnabled);
const mockSubscribe = vi.mocked(syncSubscribeProgress);
const mockUnsubscribe = vi.mocked(syncUnsubscribeProgress);
const mockSave = vi.mocked(syncProfileSave);
const mockRemove = vi.mocked(syncProfileRemove);
const mockPicker = vi.mocked(openFolder);
const mockSetCredential = vi.mocked(syncSetCredential);
const mockCopyStart = vi.mocked(copyStart);
const mockCopyStatus = vi.mocked(copyStatus);
const mockCopyCancel = vi.mocked(copyCancel);

/** The exact line Rust composes — the pane must render it character for character. */
const RUST_LINE = "tgdrive — 3 waiting to sync";
const RUST_TRANSFER_LINE = "Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB";

/**
 * The reference "now" every timestamp fixture is measured back from. Taken
 * once at load rather than faked: the relative figures are minute-granular, so
 * the few milliseconds until a test renders cannot move one.
 */
const NOW = Date.now();

/** The subscribed progress sink, captured from the mocked subscribe call. */
let emitProgress: ((event: SyncProgressVm) => void) | null = null;

function profileVm(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
  return {
    id: "p1",
    name: "tgdrive",
    localPath: "/Users/alice/Documents/tgdrive",
    remoteUrl: "git@github.com:alice/tgdrive.git",
    branch: "main",
    direction: "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 4 * 1024 * 1024,
    settleMs: null,
    effectiveSettleMs: 5_000,
    pollIntervalMs: null,
    effectivePollIntervalMs: 15_000,
    tags: [],
    commitSubjectTemplate: "",
    authorOverride: null,
    enabled: true,
    ...over,
  };
}

function statusVm(over: Partial<SyncStatusVm> = {}): SyncStatusVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    state: "watching",
    phase: "idle",
    line: RUST_LINE,
    filesDone: 0,
    filesTotal: null,
    bytesDone: 0,
    bytesTotal: null,
    pending: 3,
    settling: 0,
    warning: null,
    error: null,
    lastSyncMs: null,
    needsAttention: false,
    ...over,
  };
}

function progressVm(over: Partial<SyncProgressVm> = {}): SyncProgressVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    phase: "pushing",
    filesDone: 42,
    filesTotal: 310,
    bytesDone: 1_200_000_000,
    bytesTotal: 4_700_000_000,
    current: null,
    fraction: null,
    bytesPerSecond: null,
    ...over,
  };
}

function problemsVm(over: Partial<SyncProblemsVm> = {}): SyncProblemsVm {
  return { warning: null, error: null, parked: [], conflicts: [], ...over };
}

/** The two paths every copy test picks, and a job carrying them. */
const COPY_SOURCE = "/Users/alice/Pictures";
const COPY_DESTINATION = "/Volumes/backup";

function copyJobVm(over: Partial<CopyJobVm> = {}): CopyJobVm {
  return {
    id: "job-1",
    source: COPY_SOURCE,
    destination: COPY_DESTINATION,
    state: "copying",
    filesDone: 42,
    filesTotal: 310,
    bytesDone: 1_200_000_000,
    bytesTotal: 4_700_000_000,
    current: "2019/summer.jpg",
    // Empty until the job is terminal, exactly as Rust reports it.
    entries: [],
    error: null,
    ...over,
  };
}

/** Put a job in the mirror as though this session had started it. */
function seedCopyJob(job: CopyJobVm) {
  copyJobStore.setState({ id: job.id, job, starting: false, error: null });
}

/** Choose both paths through the native picker, as the user would. */
async function chooseCopyPaths() {
  mockPicker.mockResolvedValueOnce(COPY_SOURCE);
  fireEvent.click(screen.getByRole("button", { name: COPY_PICK_SOURCE_FOLDER_LABEL }));
  await waitFor(() =>
    expect(screen.getByTestId(COPY_SOURCE_TESTID)).toHaveTextContent(COPY_SOURCE),
  );
  mockPicker.mockResolvedValueOnce(COPY_DESTINATION);
  fireEvent.click(screen.getByRole("button", { name: COPY_PICK_DESTINATION_LABEL }));
  await waitFor(() =>
    expect(screen.getByTestId(COPY_DESTINATION_TESTID)).toHaveTextContent(COPY_DESTINATION),
  );
}

/** Mount the pane and wait for the first status snapshot to land. */
async function renderPane() {
  const view = render(<SyncPane />);
  await screen.findByText(RUST_LINE);
  return view;
}

beforeEach(() => {
  resetSyncStoreForTest();
  resetSyncDetailStoreForTest();
  resetCopyJobStoreForTest();
  emitProgress = null;
  mockProfiles.mockResolvedValue([profileVm()]);
  mockStatuses.mockResolvedValue([statusVm()]);
  mockActivity.mockResolvedValue([]);
  mockPending.mockResolvedValue([]);
  mockProblems.mockResolvedValue(problemsVm());
  mockRemove.mockResolvedValue(undefined);
  mockSubscribe.mockImplementation((onProgress: (event: SyncProgressVm) => void) => {
    emitProgress = onProgress;
    return Promise.resolve(7);
  });
  mockUnsubscribe.mockResolvedValue(undefined);
  mockCopyStart.mockResolvedValue("job-1");
  mockCopyStatus.mockResolvedValue(copyJobVm());
  mockCopyCancel.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SyncPane profile header", () => {
  it("renders the Rust-composed line verbatim beside a state word, path and host", async () => {
    await renderPane();

    // Verbatim: the tray renders this same sentence, so the pane may not reword it.
    expect(screen.getByText(RUST_LINE)).toBeInTheDocument();
    expect(screen.getByText("tgdrive")).toBeInTheDocument();
    expect(screen.getByText("Watching")).toBeInTheDocument();
    expect(screen.getByText("/Users/alice/Documents/tgdrive")).toBeInTheDocument();
    expect(screen.getByText("github.com")).toBeInTheDocument();
  });

  /** A `sync_folder_now` reply, with the fields a case does not care about. */
  function outcomeVm(over: Partial<SyncOutcomeVm> = {}): SyncOutcomeVm {
    return {
      committed: false,
      pushed: true,
      pulled: true,
      filesChanged: 0,
      conflicts: [],
      bytes: 0,
      line: "Nothing to sync — this folder already matches the remote.",
      ...over,
    };
  }

  it("offers Sync now and re-reads the lists after the action", async () => {
    mockFolderNow.mockResolvedValue(outcomeVm({ committed: true, filesChanged: 2 }));
    await renderPane();
    await waitFor(() => expect(mockActivity).toHaveBeenCalled());
    const readsBefore = mockActivity.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    await waitFor(() => expect(mockFolderNow).toHaveBeenCalledWith("p1"));
    // An action is exactly when the three lists are most likely to have moved,
    // and the poll is deliberately too slow to notice.
    await waitFor(() => expect(mockActivity.mock.calls.length).toBeGreaterThan(readsBefore));
  });

  /**
   * AD-34-12. The command already returned the whole outcome and the card threw
   * it away, so a successful click rendered nothing whatsoever — and a pass
   * that stages nothing finishes far inside the 2 s status poll, so there was
   * nothing else on screen to move either.
   */
  it("states what Sync now did, including when it did nothing", async () => {
    mockFolderNow.mockResolvedValue(
      outcomeVm({
        committed: true,
        filesChanged: 3,
        bytes: 2_048,
        line: "Committed and pushed 3 files, moved 2 KB.",
      }),
    );
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    const report = await screen.findByText("Committed and pushed 3 files, moved 2 KB.");
    // Announced, not just painted: the result of a button press has to reach a
    // screen reader without stealing focus.
    expect(report).toHaveAttribute("role", "status");
    expect(report).not.toHaveClass("text-destructive");
  });

  it("says so when there was nothing to sync, rather than nothing", async () => {
    mockFolderNow.mockResolvedValue(outcomeVm());
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    expect(
      await screen.findByText("Nothing to sync — this folder already matches the remote."),
    ).toBeInTheDocument();
  });

  it("does not dress a conflict up as a success", async () => {
    mockFolderNow.mockResolvedValue(
      outcomeVm({
        conflicts: ["notes.sync-conflict-20250725-120000-host.md"],
        line: "Kept your version of 1 file that changed in both places, alongside the remote's.",
      }),
    );
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    const report = await screen.findByText(
      "Kept your version of 1 file that changed in both places, alongside the remote's.",
    );
    expect(report).toHaveClass("text-destructive");
  });

  it("reports a failed pass as a failure and claims nothing else", async () => {
    mockFolderNow.mockRejectedValue({ code: "serverUnreachable", message: "remote unreachable" });
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    expect(await screen.findByText("remote unreachable")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("offers Resume instead of Pause for a paused folder", async () => {
    mockProfiles.mockResolvedValue([profileVm({ enabled: false })]);
    mockStatuses.mockResolvedValue([statusVm({ state: "paused" })]);
    mockSetEnabled.mockResolvedValue(statusVm());
    await renderPane();

    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SYNC_PAUSE_LABEL })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SYNC_RESUME_LABEL }));

    await waitFor(() => expect(mockSetEnabled).toHaveBeenCalledWith("p1", true));
  });

  it("draws no bar without a denominator, and the streamed one once there is", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", line: RUST_TRANSFER_LINE }),
    ]);
    render(<SyncPane />);
    await screen.findByText(RUST_TRANSFER_LINE);

    // No total known anywhere: the Rust line still says what is happening, and
    // a meter that invents a position would be worse than none.
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();

    act(() => {
      emitProgress?.(progressVm({ fraction: 0.42, current: "notes/today.md" }));
    });

    const meter = await screen.findByRole("progressbar", {
      name: `${SYNC_PROGRESS_LABEL}: tgdrive`,
    });
    expect(meter).toHaveAttribute("aria-valuenow", "42");
    // The human description stays the one Rust composed.
    expect(meter).toHaveAttribute("aria-valuetext", RUST_TRANSFER_LINE);
    // The path in flight exists only on the stream.
    expect(screen.getByText("notes/today.md")).toBeInTheDocument();
  });

  it("shows how fast and how far while a folder is working", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", line: RUST_TRANSFER_LINE }),
    ]);
    render(<SyncPane />);
    await screen.findByText(RUST_TRANSFER_LINE);

    act(() => {
      emitProgress?.(
        progressVm({
          fraction: 0.42,
          current: "clips/holiday.mov",
          filesDone: 3,
          filesTotal: 12,
          bytesPerSecond: 4_100_000,
        }),
      );
    });

    // Worded the way the Rust status line words its counter, so the fast copy
    // and the polled sentence above read as one quantity.
    expect(await screen.findByText("3/12 files · 4.1 MB/s")).toBeInTheDocument();
    expect(screen.getByText("clips/holiday.mov")).toBeInTheDocument();
  });

  it("never prints a rate of nothing", async () => {
    // Committing: the leg where files move one at a time with no wire to measure,
    // so the counter is the whole of what the engine honestly knows.
    const committing = "Committing tgdrive — 6/12 files";
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "committing", line: committing }),
    ]);
    render(<SyncPane />);
    await screen.findByText(committing);

    act(() => {
      emitProgress?.(
        progressVm({ fraction: 0.5, filesDone: 6, filesTotal: 12, bytesPerSecond: null }),
      );
    });

    // The counter still lands; a rate the engine could not honestly measure is
    // absent rather than zero, and no separator is orphaned where it would be.
    expect(await screen.findByText("6/12 files")).toBeInTheDocument();
    expect(screen.queryByText(/B\/s/)).not.toBeInTheDocument();
  });

  it("drops a stale streamed fraction once the poll says the folder is settled", async () => {
    await renderPane();
    act(() => {
      emitProgress?.(progressVm({ fraction: 0.42 }));
    });
    // `watching` with pending work is active, so the bar is honest here…
    await screen.findByRole("progressbar");

    act(() => {
      syncStore.getState().mergeStatuses([statusVm({ state: "idle", pending: 0, phase: "idle" })]);
    });

    // …but the last event the engine sent must not leave a filled bar behind.
    await waitFor(() => expect(screen.queryByRole("progressbar")).not.toBeInTheDocument());
  });

  it("unsubscribes from the progress stream on unmount", async () => {
    const view = await renderPane();
    await waitFor(() => expect(mockSubscribe).toHaveBeenCalled());
    // Let the subscription id land before tearing down.
    await act(async () => {});

    view.unmount();

    await waitFor(() => expect(mockUnsubscribe).toHaveBeenCalledWith(7));
  });
});

describe("SyncPane profile list", () => {
  it("claims nothing before the first read, then says the list is empty", async () => {
    // An empty race never settles: the read stays in flight for the whole test.
    mockProfiles.mockReturnValue(Promise.race([]));
    const view = render(<SyncPane />);
    expect(screen.queryByText(SYNC_PANE_EMPTY_SENTENCE)).not.toBeInTheDocument();
    view.unmount();

    resetSyncStoreForTest();
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    render(<SyncPane />);

    expect(await screen.findByText(SYNC_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });
});

describe("SyncPane add a folder", () => {
  /** Fill the three fields the submit button waits on. */
  async function fillRequired() {
    // Once, so the module-level "cancelled" default is what every other test
    // still mounts against.
    mockPicker.mockResolvedValueOnce("/Users/alice/notes");
    fireEvent.change(screen.getByLabelText(SYNC_NAME_LABEL), { target: { value: "notes" } });
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/notes.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));
    await waitFor(() =>
      expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Users/alice/notes"),
    );
  }

  /** The mirror re-read `saveSyncProfile` runs, now carrying the new folder. */
  function mirrorAfterAdd() {
    const added = profileVm({ id: "p2", name: "notes" });
    mockSave.mockResolvedValue(added);
    mockProfiles.mockResolvedValue([added]);
    mockStatuses.mockResolvedValue([statusVm({ profileId: "p2", profileName: "notes" })]);
  }

  it("offers the form itself when nothing is configured, instead of naming Settings", async () => {
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    render(<SyncPane />);

    expect(await screen.findByText(SYNC_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
    // The first thing a new user can do here is the thing they came for.
    expect(screen.getByRole("form", { name: SYNC_ADD_TITLE })).toBeInTheDocument();
    // No folders, so nothing to reveal: the form is the empty state. Which is
    // also why it offers no discard — there is nothing behind it to go back to,
    // and the action that would reopen it is not on screen.
    expect(screen.queryByRole("button", { name: SYNC_ADD_TITLE })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SYNC_FORM_CANCEL_LABEL })).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("Settings");
  });

  it("shows the added folder without waiting for a poll", async () => {
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    render(<SyncPane />);
    await screen.findByRole("form", { name: SYNC_ADD_TITLE });

    await fillRequired();
    mirrorAfterAdd();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    // The save re-reads the mirror, so the card is here now — not up to a poll
    // interval later — and its lists are read for the same reason.
    expect(await screen.findByText("notes")).toBeInTheDocument();
    await waitFor(() => expect(mockActivity).toHaveBeenCalledWith("p2", expect.any(Number)));
  });

  it("keeps the form behind the header action once a folder exists, and closes it after an add", async () => {
    await renderPane();

    // A permanently expanded form above a populated list would be noise.
    expect(screen.queryByRole("form", { name: SYNC_ADD_TITLE })).not.toBeInTheDocument();
    const action = screen.getByRole("button", { name: SYNC_ADD_TITLE });
    expect(action).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(action);
    expect(screen.getByRole("form", { name: SYNC_ADD_TITLE })).toBeInTheDocument();
    expect(action).toHaveAttribute("aria-expanded", "true");

    await fillRequired();
    mirrorAfterAdd();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(screen.queryByRole("form", { name: SYNC_ADD_TITLE })).not.toBeInTheDocument(),
    );
    expect(await screen.findByText("notes")).toBeInTheDocument();
  });

  it("keeps a rejected add on screen with every typed value", async () => {
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_TITLE }));
    await fillRequired();
    mockSave.mockRejectedValue({ code: "internal", message: "remote is not reachable" });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    expect(await screen.findByText("remote is not reachable")).toBeInTheDocument();
    // Closing the disclosure here would destroy the only place the message is
    // shown, along with the remote URL that has to be corrected.
    expect(screen.getByRole("form", { name: SYNC_ADD_TITLE })).toBeInTheDocument();
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue(
      "git@github.com:alice/notes.git",
    );
  });

  it("keeps a failed token write on screen across the flip from empty to populated", async () => {
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    mockSetCredential.mockRejectedValue({ code: "internal", message: "keychain refused" });
    render(<SyncPane />);
    await screen.findByRole("form", { name: SYNC_ADD_TITLE });

    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });
    mirrorAfterAdd();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // The add turns this surface from empty to populated, which is exactly the
    // moment the empty state's form would vanish — taking with it the only
    // report that the folder exists but its token does not.
    expect(
      await screen.findByText(`${SYNC_TOKEN_FAILED_PREFIX}keychain refused`),
    ).toBeInTheDocument();
    expect(await screen.findByText("notes")).toBeInTheDocument();
  });

  it("discards a half-typed add, and reopens on an empty form", async () => {
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_TITLE }));
    await fillRequired();

    fireEvent.click(screen.getByRole("button", { name: SYNC_FORM_CANCEL_LABEL }));

    await waitFor(() =>
      expect(screen.queryByRole("form", { name: SYNC_ADD_TITLE })).not.toBeInTheDocument(),
    );
    // Discarding is not a save with the fields blanked: nothing was created.
    expect(mockSave).not.toHaveBeenCalled();

    // A draft that survived a discard is a draft the next add would submit by
    // accident, so the reopened form starts from nothing — including the folder
    // that was picked through the native dialog.
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_TITLE }));
    const reopened = screen.getByRole("form", { name: SYNC_ADD_TITLE });
    expect(within(reopened).getByLabelText(SYNC_NAME_LABEL)).toHaveValue("");
    expect(within(reopened).getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue("");
    expect(within(reopened).getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent(
      SYNC_NO_FOLDER_CHOSEN_LABEL,
    );
    expect(within(reopened).getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL })).toBeDisabled();
  });
});

describe("SyncPane edit a folder", () => {
  /** The accessible name of the only card's form: one per folder on screen. */
  const EDIT_FORM = `${SYNC_EDIT_TITLE}: tgdrive`;

  /** Mount the pane and open the card's edit form. */
  async function openEdit() {
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_TITLE }));
    return await screen.findByRole("form", { name: EDIT_FORM });
  }

  it("fixes a mistyped remote from the card itself, without waiting for a poll", async () => {
    const form = await openEdit();
    // The form sits inside the card it belongs to, which goes on reporting.
    expect(screen.getByText(RUST_LINE)).toBeInTheDocument();

    const fixed = "git@gitlab.example.org:alice/tgdrive.git";
    fireEvent.change(within(form).getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: fixed },
    });
    const edited = profileVm({ remoteUrl: fixed });
    mockSave.mockResolvedValue(edited);
    mockProfiles.mockResolvedValue([edited]);
    fireEvent.click(within(form).getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // The id is the whole difference between this and adding a second folder,
    // and the path it was bound to goes back unchanged.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "p1",
          remoteUrl: fixed,
          localPath: "/Users/alice/Documents/tgdrive",
        }),
      ),
    );
    // Saving closes the form, and the save re-read the mirror, so the card
    // already points at the corrected remote rather than a poll interval later.
    expect(await screen.findByText("gitlab.example.org")).toBeInTheDocument();
    expect(screen.queryByText("github.com")).not.toBeInTheDocument();
    await waitFor(() =>
      expect(screen.queryByRole("form", { name: EDIT_FORM })).not.toBeInTheDocument(),
    );
  });

  it("leaves a paused folder paused", async () => {
    mockProfiles.mockResolvedValue([profileVm({ enabled: false })]);
    mockStatuses.mockResolvedValue([statusVm({ state: "paused" })]);
    const form = await openEdit();

    fireEvent.change(within(form).getByLabelText(SYNC_NAME_LABEL), {
      target: { value: "tgdrive archive" },
    });
    const renamed = profileVm({ enabled: false, name: "tgdrive archive" });
    mockSave.mockResolvedValue(renamed);
    mockProfiles.mockResolvedValue([renamed]);
    mockStatuses.mockResolvedValue([statusVm({ state: "paused", profileName: "tgdrive archive" })]);
    fireEvent.click(within(form).getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    expect(await screen.findByText("tgdrive archive")).toBeInTheDocument();
    // An edit is one write to one command. Routing it through anything that
    // also toggles pause would quietly resume a folder that was stopped on
    // purpose…
    expect(mockSetEnabled).not.toHaveBeenCalled();
    // …and the request carries no pause state for the merge in Rust to
    // contradict, which is what keeps the two ends from disagreeing.
    expect(mockSave.mock.calls[0]?.[0]).not.toHaveProperty("enabled");
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: SYNC_RESUME_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SYNC_PAUSE_LABEL })).not.toBeInTheDocument();
  });

  it("cancels back to the card with nothing saved, and reopens from the profile", async () => {
    const form = await openEdit();
    fireEvent.change(within(form).getByLabelText(SYNC_NAME_LABEL), {
      target: { value: "half-typed rename" },
    });

    fireEvent.click(within(form).getByRole("button", { name: SYNC_FORM_CANCEL_LABEL }));

    await waitFor(() =>
      expect(screen.queryByRole("form", { name: EDIT_FORM })).not.toBeInTheDocument(),
    );
    expect(mockSave).not.toHaveBeenCalled();
    expect(screen.getByText("tgdrive")).toBeInTheDocument();
    // Reopening starts from the stored profile, not from the abandoned edit.
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_TITLE }));
    const reopened = await screen.findByRole("form", { name: EDIT_FORM });
    expect(within(reopened).getByLabelText(SYNC_NAME_LABEL)).toHaveValue("tgdrive");
  });
});

describe("SyncPane remove a folder", () => {
  it("asks first, in the Settings wording, and then forgets the folder", async () => {
    await renderPane();

    fireEvent.click(screen.getByRole("button", { name: SYNC_REMOVE_LABEL }));

    // Nothing is forgotten until the confirmation is accepted.
    expect(mockRemove).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("alertdialog");
    // The same sentence Settings shows, not a second wording of the same
    // promise: what removal keeps is the whole question being answered here.
    expect(within(dialog).getByText(SYNC_REMOVE_CONFIRM_SENTENCE)).toBeInTheDocument();
    expect(SYNC_REMOVE_CONFIRM_SENTENCE).toMatch(/git repository are left on disk/);
    expect(SYNC_REMOVE_CONFIRM_SENTENCE).toMatch(/deletes the access token/);

    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    fireEvent.click(within(dialog).getByRole("button", { name: SYNC_REMOVE_CONFIRM_LABEL }));

    await waitFor(() => expect(mockRemove).toHaveBeenCalledWith("p1"));
    expect(await screen.findByText(SYNC_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });

  it("keeps the folder when the confirmation is declined", async () => {
    await renderPane();
    fireEvent.click(screen.getByRole("button", { name: SYNC_REMOVE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");

    fireEvent.click(within(dialog).getByRole("button", { name: SYNC_REMOVE_CANCEL_LABEL }));

    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(mockRemove).not.toHaveBeenCalled();
    expect(screen.getByText(RUST_LINE)).toBeInTheDocument();
  });

  it("does not re-read the lists of the folder it just removed", async () => {
    await renderPane();
    await waitFor(() => expect(mockActivity).toHaveBeenCalledWith("p1", expect.any(Number)));
    const reads = mockActivity.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: SYNC_REMOVE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    fireEvent.click(within(dialog).getByRole("button", { name: SYNC_REMOVE_CONFIRM_LABEL }));

    await waitFor(() => expect(mockRemove).toHaveBeenCalledWith("p1"));
    // Every other card action ends in a re-read, because the lists are most
    // likely to have moved. This one has no folder left to read, and asking
    // would only record three rejections against an id nothing will show again.
    expect(mockActivity.mock.calls.length).toBe(reads);
  });
});

describe("SyncPane activity", () => {
  const activity: SyncActivityVm[] = [
    { tsMs: NOW - 120_000, kind: "modified", path: "notes/today.md", sizeBytes: 2_500_000 },
    { tsMs: NOW - 3_600_000, kind: "added", path: "notes/new.md", sizeBytes: 12 },
    { tsMs: NOW - 7_200_000, kind: "deleted", path: "notes/old.md", sizeBytes: 4_000 },
    {
      tsMs: NOW - 10_800_000,
      kind: "conflict",
      path: "notes/shared.sync-conflict-01.md",
      sizeBytes: null,
    },
  ];

  it("lists what sync did, newest first, with the kind spoken and the time relative", async () => {
    mockActivity.mockResolvedValue(activity);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    // Rendered in the order Rust returned them, which is newest first.
    expect(rows).toHaveLength(4);
    expect(rows[0]).toHaveTextContent("notes/today.md");
    expect(rows[3]).toHaveTextContent("notes/shared.sync-conflict-01.md");
    // The kind rides an icon on screen and a word to a screen reader.
    expect(rows[0]).toHaveTextContent("Changed");
    expect(rows[1]).toHaveTextContent("Added");
    expect(rows[2]).toHaveTextContent("Deleted");
    expect(rows[3]).toHaveTextContent("Conflict copy");
    // Relative, in whatever the runtime locale calls two minutes.
    expect(rows[0].textContent ?? "").toMatch(/2\s*min/);
    // Four kinds, four glyphs: this list is scanned rather than read, and
    // three variations on one page outline made every row look like the last.
    const glyphs = rows.map((row) => row.querySelector("svg")?.getAttribute("class") ?? "");
    expect(new Set(glyphs).size).toBe(4);
    // The size sits beside the time, and a row nobody measured shows none at
    // all — never "0 B", which would claim the file was empty.
    expect(rows[0]).toHaveTextContent("2.5 MB");
    expect(rows[1]).toHaveTextContent("12 bytes");
    expect(rows[2]).toHaveTextContent("4.0 kB");
    expect(rows[3].textContent ?? "").not.toMatch(/\d\s(bytes?|kB|MB|GB|TB)/);
  });

  it("asks for a bounded page rather than the whole history", async () => {
    await renderPane();
    await waitFor(() => expect(mockActivity).toHaveBeenCalled());

    expect(mockActivity).toHaveBeenCalledWith("p1", expect.any(Number));
  });

  it("says nothing has synced yet rather than reporting no data", async () => {
    await renderPane();

    expect(await screen.findByText(SYNC_ACTIVITY_EMPTY_SENTENCE)).toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` }),
    ).not.toBeInTheDocument();
  });

  it("keeps the previous list when a read fails, instead of claiming it is empty", async () => {
    mockActivity.mockResolvedValue(activity);
    await renderPane();
    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });

    // An unknown profile rejects rather than resolving empty, so a rejection
    // must never be rendered as calm.
    mockActivity.mockRejectedValue({ code: "internal", message: "no such profile" });
    await act(async () => {
      await refreshSyncDetail("p1");
    });

    expect(await screen.findByText("no such profile")).toBeInTheDocument();
    expect(screen.getByText("notes/today.md")).toBeInTheDocument();
    expect(screen.queryByText(SYNC_ACTIVITY_EMPTY_SENTENCE)).not.toBeInTheDocument();
  });
});

describe("SyncPane pending", () => {
  const pending: SyncPendingVm[] = [
    { path: "notes/draft.md", reason: "settling", sinceMs: NOW - 300_000 },
    { path: "notes/scratch.md", reason: "untracked", sinceMs: null },
  ];

  it("lists what is waiting and why", async () => {
    mockPending.mockResolvedValue(pending);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("notes/draft.md");
    expect(rows[1]).toHaveTextContent("notes/scratch.md");
    expect(rows[1]).toHaveTextContent("New file, not synced yet");
  });

  it("explains a settling file as a wait so far, never as a finish time", async () => {
    mockPending.mockResolvedValue(pending);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const settling = within(list).getAllByRole("listitem")[0];
    expect(settling).toHaveTextContent(SYNC_SETTLING_SENTENCE);
    // How long it has been waiting — elapsed, not remaining.
    expect(settling).toHaveTextContent("5 min so far");
    // And the reason there is no estimate at all, said once under the list.
    expect(screen.getByText(SYNC_SETTLING_NOTE)).toBeInTheDocument();
    // Nothing in the list promises when it will finish.
    expect(list.textContent ?? "").not.toMatch(/remaining|left|eta|finishe?s|in \d/i);
  });

  it("drops the settling explanation when nothing is settling", async () => {
    mockPending.mockResolvedValue([pending[1]]);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    expect(screen.queryByText(SYNC_SETTLING_NOTE)).not.toBeInTheDocument();
  });

  it("says nothing is waiting when the list is genuinely empty", async () => {
    await renderPane();

    expect(await screen.findByText(SYNC_PENDING_EMPTY_SENTENCE)).toBeInTheDocument();
  });
});

describe("SyncPane problems", () => {
  it("renders no Problems section at all when nothing is wrong", async () => {
    await renderPane();

    await screen.findByText(SYNC_PENDING_EMPTY_SENTENCE);
    expect(screen.queryByText(SYNC_PROBLEMS_TITLE)).not.toBeInTheDocument();
  });

  it("names a parked unit's error and retries exactly that unit", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({
        parked: [
          { id: 41, kind: "push", attempts: 5, lastError: "remote hung up" },
          { id: 42, kind: "lfsUpload", attempts: 2, lastError: null },
        ],
      }),
    );
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PARKED_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Push · stopped after 5 attempts");
    expect(rows[0]).toHaveTextContent("remote hung up");
    // A unit that failed without a recorded cause says so rather than showing a gap.
    expect(rows[1]).toHaveTextContent("Large file upload · stopped after 2 attempts");
    expect(rows[1]).toHaveTextContent(SYNC_PARKED_NO_ERROR_SENTENCE);

    // Each Retry is named for the unit it retries, so several are tellable apart.
    fireEvent.click(
      screen.getByRole("button", {
        name: `${SYNC_RETRY_LABEL}: Large file upload · stopped after 2 attempts`,
      }),
    );

    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledWith("p1", 42));
    expect(mockRetryParked).toHaveBeenCalledTimes(1);
  });

  it("lists conflict copies and says which version is which", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({ conflicts: ["notes/shared.sync-conflict-20260727-air.md"] }),
    );
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_CONFLICT_TITLE}: tgdrive` });
    expect(
      within(list).getByText("notes/shared.sync-conflict-20260727-air.md"),
    ).toBeInTheDocument();
    expect(screen.getByText(SYNC_CONFLICT_SENTENCE)).toBeInTheDocument();
  });

  it("shows the live warning and error the engine reported", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({ warning: "Large files are missing.", error: "Authentication failed." }),
    );
    await renderPane();

    expect(await screen.findByText("Authentication failed.")).toBeInTheDocument();
    expect(screen.getByText("Large files are missing.")).toBeInTheDocument();
  });
});

describe("SyncPane copy files once", () => {
  it("keeps Copy disabled until both a source and a destination are chosen", async () => {
    await renderPane();

    const copy = screen.getByRole("button", { name: COPY_SUBMIT_LABEL });
    expect(copy).toBeDisabled();

    mockPicker.mockResolvedValueOnce(COPY_SOURCE);
    fireEvent.click(screen.getByRole("button", { name: COPY_PICK_SOURCE_FOLDER_LABEL }));
    await waitFor(() =>
      expect(screen.getByTestId(COPY_SOURCE_TESTID)).toHaveTextContent(COPY_SOURCE),
    );
    // Half a job is not a job: a copy with nowhere to land cannot be started.
    expect(copy).toBeDisabled();

    mockPicker.mockResolvedValueOnce(COPY_DESTINATION);
    fireEvent.click(screen.getByRole("button", { name: COPY_PICK_DESTINATION_LABEL }));
    await waitFor(() => expect(copy).toBeEnabled());

    fireEvent.click(copy);

    // Replace defaults off (AD-C4), and the choice is sent rather than assumed.
    await waitFor(() =>
      expect(mockCopyStart).toHaveBeenCalledWith(COPY_SOURCE, COPY_DESTINATION, false),
    );
  });

  it("carries the replace choice, reached by the label that explains it", async () => {
    await renderPane();
    await chooseCopyPaths();

    // Named by its own label, so it is reachable and announced without sight of
    // the box, and described by the note that says what leaving it off means.
    const replace = screen.getByRole("checkbox", { name: COPY_REPLACE_LABEL });
    expect(replace).not.toBeChecked();
    expect(replace).toHaveAccessibleDescription(COPY_REPLACE_NOTE);

    fireEvent.click(replace);
    expect(replace).toBeChecked();
    fireEvent.click(screen.getByRole("button", { name: COPY_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockCopyStart).toHaveBeenCalledWith(COPY_SOURCE, COPY_DESTINATION, true),
    );
  });

  it("shows the state, the bar, the file in flight and Stop, then stops polling once settled", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      await renderPane();
      await chooseCopyPaths();
      fireEvent.click(screen.getByRole("button", { name: COPY_SUBMIT_LABEL }));

      const meter = await screen.findByRole("progressbar", { name: COPY_PROGRESS_LABEL });
      // 1.2 GB of 4.7 GB, from the job's own counters.
      expect(meter).toHaveAttribute("aria-valuenow", "26");
      expect(meter).toHaveAttribute(
        "aria-valuetext",
        "Copying — 42 of 310 files · 1.2 GB of 4.7 GB",
      );
      // The path in flight, spoken with what it is.
      expect(screen.getByText("2019/summer.jpg")).toBeInTheDocument();
      expect(screen.getByText(COPY_CURRENT_LABEL)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: COPY_STOP_LABEL })).toBeInTheDocument();
      await waitFor(() => expect(mockCopyStatus).toHaveBeenCalledWith("job-1"));

      // The next poll finds it settled…
      mockCopyStatus.mockResolvedValue(
        copyJobVm({
          state: "done",
          current: null,
          entries: [{ path: "2019/summer.jpg", bytes: 4_200_000, outcome: "copied", reason: null }],
        }),
      );
      await vi.advanceTimersByTimeAsync(COPY_POLL_MS);
      await waitFor(() =>
        expect(screen.queryByRole("button", { name: COPY_STOP_LABEL })).not.toBeInTheDocument(),
      );

      // …and nothing asks again: a terminal job cannot change, and the report
      // arrived with the state that ended it.
      const settledReads = mockCopyStatus.mock.calls.length;
      await vi.advanceTimersByTimeAsync(COPY_POLL_MS * 4);
      expect(mockCopyStatus.mock.calls.length).toBe(settledReads);
    } finally {
      vi.useRealTimers();
    }
  });

  it("asks the job to stop and shows what had already finished", async () => {
    seedCopyJob(copyJobVm());
    mockCopyStatus.mockResolvedValue(
      copyJobVm({
        state: "cancelled",
        current: null,
        entries: [{ path: "2019/summer.jpg", bytes: 4_200_000, outcome: "copied", reason: null }],
      }),
    );
    await renderPane();

    fireEvent.click(screen.getByRole("button", { name: COPY_STOP_LABEL }));

    await waitFor(() => expect(mockCopyCancel).toHaveBeenCalledWith("job-1"));
    expect(await screen.findByText("Stopped")).toBeInTheDocument();
    // Nothing was left half-written, so the partial report is worth showing.
    const report = screen.getByTestId(COPY_REPORT_TESTID);
    expect(report).toHaveTextContent("2019/summer.jpg");
    expect(report).toHaveTextContent(COPY_STOPPED_SENTENCE);
  });

  it("says a job that found nothing found nothing, rather than showing an empty list", async () => {
    seedCopyJob(copyJobVm({ state: "done", current: null, filesTotal: 0, bytesTotal: 0 }));
    await renderPane();

    const report = within(screen.getByTestId(COPY_REPORT_TESTID));
    expect(report.getByText(COPY_NOTHING_SENTENCE)).toBeInTheDocument();
    expect(report.queryAllByRole("list")).toHaveLength(0);
  });

  it("groups a settled report worst first and gives a failure its reason", async () => {
    seedCopyJob(
      copyJobVm({
        state: "done",
        current: null,
        // Deliberately arriving best first, so the order below is the pane's.
        entries: [
          { path: "2019/notes.txt", bytes: 1_024, outcome: "identical", reason: null },
          { path: "2019/summer.jpg", bytes: 4_200_000, outcome: "copied", reason: null },
          { path: "2019/receipt.pdf", bytes: 90_000, outcome: "collision", reason: null },
          {
            path: "2019/inbox",
            bytes: 0,
            outcome: "failed",
            reason: "symbolic links are not followed",
          },
        ],
      }),
    );
    await renderPane();

    const report = within(screen.getByTestId(COPY_REPORT_TESTID));
    expect(report.getAllByRole("list").map((list) => list.getAttribute("aria-label"))).toEqual([
      "Could not be copied",
      "Already there, and different",
      "Copied and verified",
      "Already identical",
    ]);
    // The summary is counted off that same grouping, so the two cannot disagree.
    expect(
      report.getByText("1 failed · 1 left untouched · 1 copied and verified · 1 already identical"),
    ).toBeInTheDocument();
    // A failure says why, in Rust's own words.
    expect(report.getByText("symbolic links are not followed")).toBeInTheDocument();
    // …and carries no byte figure, because none of it reached the destination.
    expect(
      within(report.getByRole("list", { name: "Could not be copied" })).getByRole("listitem"),
    ).not.toHaveTextContent("bytes");
  });

  it("says a collision left the existing file untouched", async () => {
    seedCopyJob(
      copyJobVm({
        state: "done",
        current: null,
        entries: [{ path: "2019/receipt.pdf", bytes: 90_000, outcome: "collision", reason: null }],
      }),
    );
    await renderPane();

    const report = within(screen.getByTestId(COPY_REPORT_TESTID));
    const note = report.getByText(/left it exactly as it was/);
    // Nothing was overwritten, and the lever that would change that is named.
    expect(note).toHaveTextContent(COPY_REPLACE_LABEL);
  });

  it("renders a job-level error as a failed job, and a file that failed as neither", async () => {
    seedCopyJob(
      copyJobVm({ state: "failed", current: null, error: "/Volumes/backup is read-only" }),
    );
    const view = await renderPane();

    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("/Volumes/backup is read-only");
    view.unmount();

    resetSyncStoreForTest();
    resetCopyJobStoreForTest();
    seedCopyJob(
      copyJobVm({
        state: "done",
        current: null,
        entries: [
          {
            path: "2019/inbox",
            bytes: 0,
            outcome: "failed",
            reason: "symbolic links are not followed",
          },
        ],
      }),
    );
    render(<SyncPane />);
    await screen.findByTestId(COPY_REPORT_TESTID);

    // One file that could not be copied is an entry, never a failed job: the
    // job finished, and the report says what happened to that file.
    expect(screen.getByText("Finished")).toBeInTheDocument();
    expect(screen.queryByText("Failed")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByText("symbolic links are not followed")).toBeInTheDocument();
  });

  it("renders no report while the job's entries are still empty", async () => {
    seedCopyJob(copyJobVm());
    await renderPane();

    // `entries` is empty until a job is terminal; drawing it now would present
    // a copy that has touched nothing as one that finished with nothing to say.
    expect(screen.queryByTestId(COPY_REPORT_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByText(COPY_RESULT_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByText(COPY_NOTHING_SENTENCE)).not.toBeInTheDocument();
    // It is still running, and still stoppable.
    expect(screen.getByRole("button", { name: COPY_STOP_LABEL })).toBeInTheDocument();
  });

  it("reports the start Rust refused, and starts no job", async () => {
    mockCopyStart.mockRejectedValue({
      code: "internal",
      message: "the destination is inside the source, which would copy the tree into itself",
    });
    await renderPane();
    await chooseCopyPaths();

    fireEvent.click(screen.getByRole("button", { name: COPY_SUBMIT_LABEL }));

    expect(
      await screen.findByText(
        "the destination is inside the source, which would copy the tree into itself",
      ),
    ).toBeInTheDocument();
    // No job exists, so nothing is polled and the action is offered again.
    expect(mockCopyStatus).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: COPY_SUBMIT_LABEL })).toBeEnabled();
  });
});

describe("sync pane projections", () => {
  it("reports a wait as elapsed time, coarsely, and never below zero", () => {
    expect(formatSyncWaited(NOW - 30_000, NOW)).toBe("under a minute");
    expect(formatSyncWaited(NOW - 5 * 60_000, NOW)).toBe("5 min");
    expect(formatSyncWaited(NOW - 3 * 3_600_000, NOW)).toBe("3 hr");
    expect(formatSyncWaited(NOW - 86_400_000, NOW)).toBe("1 day");
    expect(formatSyncWaited(NOW - 3 * 86_400_000, NOW)).toBe("3 days");
    // A clock-skewed start must not read as a wait that has not begun.
    expect(formatSyncWaited(NOW + 60_000, NOW)).toBe("under a minute");
  });

  it("words each pending reason, and shows an unknown one as itself", () => {
    expect(syncPendingReason({ path: "a", reason: "modified", sinceMs: null })).toBe(
      "Changed, not synced yet",
    );
    // A settling row with no recorded start still says what it is waiting for.
    expect(syncPendingReason({ path: "a", reason: "settling", sinceMs: null })).toBe(
      SYNC_SETTLING_SENTENCE,
    );
    // A reason Rust grows later is shown, not swallowed.
    expect(syncPendingReason({ path: "a", reason: "quarantined", sinceMs: null })).toBe(
      "quarantined",
    );
  });

  it("counts a single attempt in the singular and an unknown kind as itself", () => {
    expect(syncParkedSummary({ id: 1, kind: "pull", attempts: 1, lastError: null })).toBe(
      "Pull · stopped after 1 attempt",
    );
    expect(syncParkedSummary({ id: 2, kind: "rebase", attempts: 3, lastError: null })).toBe(
      "rebase · stopped after 3 attempts",
    );
  });

  it("lets the poll decide whether a folder is working and the stream how far", () => {
    const idle = statusVm({ state: "idle", pending: 0 });
    const busy = statusVm({ state: "syncing", phase: "pushing", bytesDone: 250, bytesTotal: 1000 });
    // No status at all, or a settled one: nothing to draw, whatever the stream said.
    expect(syncLiveFraction(undefined, progressVm({ fraction: 0.9 }))).toBeNull();
    expect(syncLiveFraction(idle, progressVm({ fraction: 0.9 }))).toBeNull();
    // The stream refines the polled snapshot…
    expect(syncLiveFraction(busy, progressVm({ fraction: 0.9 }))).toBeCloseTo(0.9);
    // …and is clamped, because a byte total grows as more objects are announced.
    expect(syncLiveFraction(busy, progressVm({ fraction: 1.4 }))).toBe(1);
    // With no event yet, the polled counters still answer.
    expect(syncLiveFraction(busy, undefined)).toBeCloseTo(0.25);
  });

  it("keeps a streamed rate behind the poll's verdict on whether work is happening", () => {
    const idle = statusVm({ state: "idle", pending: 0 });
    const busy = statusVm({ state: "syncing", phase: "pushing" });
    // A rate arriving between two polls must not be what makes a card look busy.
    expect(syncLiveRate(undefined, progressVm({ bytesPerSecond: 9_000_000 }))).toBeNull();
    expect(syncLiveRate(idle, progressVm({ bytesPerSecond: 9_000_000 }))).toBeNull();
    expect(syncLiveRate(busy, progressVm({ bytesPerSecond: 9_000_000 }))).toBe(9_000_000);
    // No event yet, and a zero the poll cannot vouch for, are the same answer:
    // nothing to show. The poll carries no rate of its own to fall back on.
    expect(syncLiveRate(busy, undefined)).toBeNull();
    expect(syncLiveRate(busy, progressVm({ bytesPerSecond: 0 }))).toBeNull();
  });
});

describe("copy pane projections", () => {
  it("leaves out a total the engine has not worked out yet", () => {
    // A zero total means unknown, and a card must never claim a total it does
    // not have — not even as "0 of 0".
    expect(copyProgressSentence(copyJobVm({ filesTotal: 0, bytesTotal: 0 }))).toBe("Copying");
    expect(copyProgressSentence(copyJobVm({ filesTotal: 0 }))).toBe("Copying — 1.2 GB of 4.7 GB");
    expect(copyProgressSentence(copyJobVm({ state: "verifying" }))).toBe(
      "Verifying — 42 of 310 files · 1.2 GB of 4.7 GB",
    );
  });

  it("counts small copies in bytes rather than rounding them away", () => {
    expect(formatCopyBytes(0)).toBe("0 bytes");
    expect(formatCopyBytes(1)).toBe("1 byte");
    expect(formatCopyBytes(999)).toBe("999 bytes");
    expect(formatCopyBytes(1_500)).toBe("1.5 kB");
    // Truncates: a figure here must never overstate what reached the disk.
    expect(formatCopyBytes(1_299_000_000)).toBe("1.2 GB");
  });

  it("counts the summary off the grouping the list renders, worst first", () => {
    const groups = copyEntryGroups([
      { path: "a", bytes: 1, outcome: "copied", reason: null },
      { path: "b", bytes: 1, outcome: "identical", reason: null },
      { path: "c", bytes: 1, outcome: "copied", reason: null },
      { path: "d", bytes: 0, outcome: "failed", reason: "gone" },
      { path: "e", bytes: 1, outcome: "collision", reason: null },
    ]);
    expect(groups.map((group) => group.outcome)).toEqual([
      "failed",
      "collision",
      "copied",
      "identical",
    ]);
    expect(copySummarySentence(groups)).toBe(
      "1 failed · 1 left untouched · 2 copied and verified · 1 already identical",
    );
  });

  it("shows an outcome Rust grows later rather than dropping its files", () => {
    const groups = copyEntryGroups([
      { path: "a", bytes: 1, outcome: "quarantined", reason: null },
      { path: "b", bytes: 1, outcome: "copied", reason: null },
    ]);
    // Ranked after everything known, but never swallowed: a report that lost a
    // row would under-report the copy.
    expect(groups.map((group) => group.outcome)).toEqual(["copied", "quarantined"]);
    expect(copySummarySentence(groups)).toBe("1 copied and verified · 1 quarantined");
  });
});
