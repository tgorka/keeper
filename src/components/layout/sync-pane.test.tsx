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
  // The path control (Story 32.4): resolves and opens the folder in Rust.
  syncOpenPath: vi.fn(),
  // The three detail reads plus the parked-unit retry (Story 32.4).
  syncActivity: vi.fn(),
  syncPending: vi.fn(),
  syncProblems: vi.fn(),
  syncRetryParked: vi.fn(),
  syncRescan: vi.fn(),
  // The persisted list sizes (folded / unfolded).
  syncListSettingsGet: vi.fn(),
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
  SYNC_DELIVERY_DETAIL_LABEL,
  SYNC_DELIVERY_RETRYING_SENTENCE,
  SYNC_DELIVERY_STATES,
  SYNC_PANE_EMPTY_SENTENCE,
  SYNC_PARKED_NO_ERROR_SENTENCE,
  SYNC_PARKED_TITLE,
  SYNC_PENDING_CURRENT_WORD,
  SYNC_PENDING_EMPTY_SENTENCE,
  SYNC_PENDING_INBOUND_WORD,
  SYNC_PENDING_OUTBOUND_WORD,
  SYNC_PENDING_TITLE,
  SYNC_PROBLEMS_TITLE,
  SYNC_RESCAN_LABEL,
  SYNC_RESCAN_NOTE,
  SYNC_RETRY_ALL_LABEL,
  SYNC_RETRY_LABEL,
  SYNC_SETTLING_NOTE,
  SYNC_SETTLING_SENTENCE,
  SYNC_UNSPELLABLE_SENTENCE,
  SYNC_UNSPELLABLE_TITLE,
  SyncPane,
  syncParkedSummary,
  syncPendingReason,
} from "@/components/layout/sync-pane";
import {
  SYNC_NOW_LABEL,
  SYNC_OPEN_PATH_LABEL,
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
  syncGetCredential,
  syncListSettingsGet,
  syncOpenPath,
  syncPending,
  syncProblems,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncRescan,
  syncRetryParked,
  syncSetCredential,
  syncStatuses,
  syncSubscribeProgress,
  syncUnsubscribeProgress,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
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
const mockListSettings = vi.mocked(syncListSettingsGet);
const mockRescan = vi.mocked(syncRescan);
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
const mockGetCredential = vi.mocked(syncGetCredential);
const mockOpenPath = vi.mocked(syncOpenPath);
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

function statusVm(over: Partial<SyncStatusVm> = {}): SyncStatusVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    state: "watching",
    phase: "idle",
    line: RUST_LINE,
    queuedFiles: 0,
    queuedBytes: 0,
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

/**
 * One streamed frame, shaped like one the engine can actually emit.
 *
 * `uploadingLfs`, not `pushing`: byte counters exist on the fetch leg and the
 * LFS legs and nowhere else, and `RUST_TRANSFER_LINE` above is `Transferring`,
 * which is the label of exactly these two phases — the old `pushing` default
 * disagreed with the very line it was paired with (Story 34.8).
 */
function progressVm(over: Partial<SyncProgressVm> = {}): SyncProgressVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    phase: "uploadingLfs",
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
  return { warning: null, error: null, parked: [], conflicts: [], unspellable: [], ...over };
}

/**
 * One activity row, with the fields a case does not care about. `success` and
 * no failure is the shape nearly every real row has, so a delivery case says
 * only how it differs from that.
 */
function activityVm(over: Partial<SyncActivityVm> = {}): SyncActivityVm {
  return {
    tsMs: NOW - 120_000,
    kind: "modified",
    path: "notes/today.md",
    sizeBytes: 2_500_000,
    delivery: "success",
    failure: null,
    unitId: null,
    ...over,
  };
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
  mockListSettings.mockResolvedValue({ folded: 10, unfolded: 100 });
  mockRescan.mockResolvedValue(undefined);
  mockPending.mockResolvedValue([]);
  mockProblems.mockResolvedValue(problemsVm());
  // Every edit form reads the keychain as it opens (Story 34.12); this folder
  // has nothing stored.
  mockGetCredential.mockResolvedValue(null);
  mockRemove.mockResolvedValue(undefined);
  mockSubscribe.mockImplementation((onProgress: (event: SyncProgressVm) => void) => {
    emitProgress = onProgress;
    return Promise.resolve(7);
  });
  mockUnsubscribe.mockResolvedValue(undefined);
  mockCopyStart.mockResolvedValue("job-1");
  mockCopyStatus.mockResolvedValue(copyJobVm());
  mockCopyCancel.mockResolvedValue(undefined);
  mockOpenPath.mockResolvedValue(undefined);
  // The path control is gated on a real file manager existing, so these cards
  // render as a desktop would show them.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
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
      stale: [],
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

  /**
   * Story 32.4, specified and never shipped: the card showed the path as text
   * and there was no way to open the folder from the app at all. The path itself
   * is the control, and it is a real button so it is keyboard-reachable.
   */
  it("opens the folder from the path itself, asking for it by profile id", async () => {
    await renderPane();

    const control = screen.getByRole("button", {
      name: `${SYNC_OPEN_PATH_LABEL}: /Users/alice/Documents/tgdrive`,
    });
    fireEvent.click(control);

    // The id, never a path: the frontend cannot name a folder here, so it cannot
    // ask for one keeper does not already sync.
    await waitFor(() => expect(mockOpenPath).toHaveBeenCalledWith("p1"));
    expect(mockOpenPath).toHaveBeenCalledTimes(1);
    // Opening a folder changes nothing about it, so it must not clear the card's
    // last sync report or re-read the three detail lists.
    expect(mockFolderNow).not.toHaveBeenCalled();
  });

  it("shows what Rust said when the folder is gone or its volume is out", async () => {
    const refusal =
      "/Volumes/stick/field is not there. This folder lives on removable media — reattach the volume, then open it again.";
    mockOpenPath.mockRejectedValue({ code: "internal", message: refusal, retriable: false });
    await renderPane();

    fireEvent.click(
      screen.getByRole("button", {
        name: `${SYNC_OPEN_PATH_LABEL}: /Users/alice/Documents/tgdrive`,
      }),
    );

    // Verbatim: the sentence Rust composed is the one that names the next step.
    expect(await screen.findByText(refusal)).toBeInTheDocument();
  });

  it("leaves the path as plain text where there is no file manager to open it in", async () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    await renderPane();

    // Still readable — just not a control that would fail on activation.
    expect(screen.getByText("/Users/alice/Documents/tgdrive")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: new RegExp(SYNC_OPEN_PATH_LABEL) }),
    ).not.toBeInTheDocument();
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

  it("shows how fast and how far while a large file is on the wire", async () => {
    // `uploadingLfs`, not `pushing`. Only three phases can carry a rate at all —
    // `SyncPhase::carries_rate` — because only the fetch fold and the LFS
    // transfer tally measure bytes; a plain `git push` is a captured subprocess
    // whose byte counters nothing reads. This case used to set `phase:
    // "pushing"` beside `bytesPerSecond: 4_100_000`, a pair the engine cannot
    // produce, so it proved the renderer against an event that never arrives and
    // would have passed on a gutted producer (Story 34.8).
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "uploadingLfs", line: RUST_TRANSFER_LINE }),
    ]);
    render(<SyncPane />);
    await screen.findByText(RUST_TRANSFER_LINE);

    act(() => {
      emitProgress?.(
        progressVm({
          phase: "uploadingLfs",
          fraction: 0.42,
          current: "clips/holiday.mov",
          filesDone: 3,
          filesTotal: 12,
          bytesPerSecond: 4_100_000,
        }),
      );
    });

    // Worded the way the Rust status line words its counter, so the fast copy
    // and the polled sentence above read as one quantity. Two elements, not
    // one string: the rate needs a box of its own to stop it dragging the row
    // about, and the file count cannot have one.
    expect(await screen.findByText("3/12 files")).toBeInTheDocument();
    const rate = screen.getByText("4.1 MB/s");
    expect(screen.getByText("clips/holiday.mov")).toBeInTheDocument();

    // The reservation itself, asserted as the class because jsdom has no
    // layout to measure: `2 kB/s` and `294.8 kB/s` differ by four characters,
    // and without a fixed box everything after the rate would move every tick.
    expect(rate.className).toContain("min-w-[11ch]");
    expect(rate.className).toContain("text-right");

    // And in that order. The figures are the fixed-width half and the half that
    // changes every tick; a reader watching a rate should not have to find it
    // at the end of a path whose length depends on how deep the file sits.
    // Stated as the property rather than as literal text: `textContent` glues
    // the spans together without the flex gap, so a string match here pins the
    // layout's whitespace instead of the thing that matters.
    const detail = rate.closest("p")?.textContent ?? "";
    expect(detail.indexOf("4.1 MB/s")).toBeLessThan(detail.indexOf("clips/holiday.mov"));
  });

  it("claims nothing is in flight for a folder that has stopped", async () => {
    // Stories 34.8 and 34.10. A push refused with a 401 parks the profile in
    // `needsAttention`, and the engine now retires the phase with it — but this
    // arranges the pre-fix snapshot on purpose, phase and all, because the card
    // must not depend on the engine having remembered. A bar and a frozen rate
    // sitting directly above "git.invalid rejected the access token" is the
    // exact contradiction of 34-10's "a failed Sync shows only the error".
    const refusal = "git.invalid rejected the access token — replace it with a current one";
    mockStatuses.mockResolvedValue([
      statusVm({
        state: "needsAttention",
        phase: "pushing",
        pending: 0,
        error: refusal,
        needsAttention: true,
        line: "tgdrive — needs attention",
      }),
    ]);
    mockProblems.mockResolvedValue(problemsVm({ error: refusal }));
    render(<SyncPane />);
    await screen.findByText("tgdrive — needs attention");

    // The last frame the engine sent before the refusal, still in the store.
    // Asserted rather than assumed: a `null` sink would make every negative
    // below pass for the wrong reason.
    expect(emitProgress).not.toBeNull();
    act(() => {
      emitProgress?.(
        progressVm({
          fraction: 0.42,
          current: "notes/today.md",
          filesDone: 3,
          filesTotal: 12,
          bytesPerSecond: 4_100_000,
        }),
      );
    });

    // The error is on screen, and it is the only thing on screen claiming
    // anything about this folder.
    expect(await screen.findByText(refusal)).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.queryByText(/B\/s/)).not.toBeInTheDocument();
    expect(screen.queryByText(/3\/12 files/)).not.toBeInTheDocument();
    expect(screen.queryByText("notes/today.md")).not.toBeInTheDocument();
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
    activityVm(),
    activityVm({ tsMs: NOW - 3_600_000, kind: "added", path: "notes/new.md", sizeBytes: 12 }),
    activityVm({ tsMs: NOW - 7_200_000, kind: "deleted", path: "notes/old.md", sizeBytes: 4_000 }),
    // A conflict copy is written by the pull itself, so no unit of work is
    // accountable for carrying it — which is what `unknown` delivery is for.
    activityVm({
      tsMs: NOW - 10_800_000,
      kind: "conflict",
      path: "notes/shared.sync-conflict-01.md",
      sizeBytes: null,
      delivery: "unknown",
    }),
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

describe("SyncPane activity delivery", () => {
  /**
   * One row per delivery value, so the case that renders nothing is asserted
   * beside the four that render rather than in isolation.
   */
  const delivered: SyncActivityVm[] = [
    activityVm({ path: "notes/arrived.md", delivery: "success" }),
    activityVm({ path: "notes/moving.md", delivery: "inProgress" }),
    activityVm({
      path: "notes/failing.md",
      delivery: "failed",
      failure: "fatal: could not read from remote repository",
      unitId: 88,
    }),
    activityVm({
      path: "notes/given-up.md",
      delivery: "abandoned",
      failure: "413 Payload Too Large",
      unitId: 77,
    }),
    activityVm({ path: "notes/copy.md", kind: "conflict", delivery: "unknown" }),
  ];

  it("says how far each file got, and says nothing where nothing is accountable", async () => {
    mockActivity.mockResolvedValue(delivered);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    // The state rides a glyph on screen and a word to a screen reader, the way
    // the kind at the other end of the row does.
    expect(rows[0]).toHaveTextContent(SYNC_DELIVERY_STATES.success.word);
    expect(rows[1]).toHaveTextContent(SYNC_DELIVERY_STATES.inProgress.word);
    expect(rows[2]).toHaveTextContent(SYNC_DELIVERY_STATES.failed.word);
    expect(rows[3]).toHaveTextContent(SYNC_DELIVERY_STATES.abandoned.word);
    // Two glyphs on a row with a delivery fact — the kind and the delivery —
    // and the kind alone on the row that has none.
    expect(rows[0].querySelectorAll("svg")).toHaveLength(2);
    expect(rows[4].querySelectorAll("svg")).toHaveLength(1);
    // Not a word either, and not a placeholder held open: a row nothing is
    // accountable for reports no delivery at all rather than a guess.
    for (const state of Object.values(SYNC_DELIVERY_STATES)) {
      expect(rows[4]).not.toHaveTextContent(state.word);
    }
    // Only a row with something recorded against it is a control at all — a
    // trigger that opened an empty popover would claim there was a reason to read.
    expect(
      screen.queryByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: notes/arrived.md` }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: notes/failing.md` }),
    ).toBeInTheDocument();
  });

  it("names the file, the state and the engine's own message when asked why", async () => {
    mockActivity.mockResolvedValue(delivered);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    fireEvent.click(
      screen.getByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: notes/given-up.md` }),
    );

    // The reason sits with the file, which the Problems section below can never
    // do: it reports the unit of work and never the path.
    const why = await screen.findByRole("dialog");
    expect(within(why).getByText("notes/given-up.md")).toBeInTheDocument();
    expect(within(why).getByText(SYNC_DELIVERY_STATES.abandoned.word)).toBeInTheDocument();
    // Verbatim, as the engine recorded it.
    expect(within(why).getByText("413 Payload Too Large")).toBeInTheDocument();
  });

  it("retries exactly the unit the abandoned row was waiting on", async () => {
    mockActivity.mockResolvedValue(delivered);
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    fireEvent.click(
      screen.getByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: notes/given-up.md` }),
    );
    // Named for the file it is about: a card holds one of these per row.
    fireEvent.click(
      await screen.findByRole("button", { name: `${SYNC_RETRY_LABEL}: notes/given-up.md` }),
    );

    // That row's unit, not the failing row's — the two are different work.
    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledWith("p1", 77));
    expect(mockRetryParked).toHaveBeenCalledTimes(1);
  });

  it("offers no retry on a row keeper has not given up on", async () => {
    mockActivity.mockResolvedValue(delivered);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    fireEvent.click(
      screen.getByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: notes/failing.md` }),
    );

    // keeper is still retrying this one, so the popover says so instead of
    // offering a button whose whole meaning would be that it had stopped.
    const why = await screen.findByRole("dialog");
    expect(within(why).getByText(SYNC_DELIVERY_RETRYING_SENTENCE)).toBeInTheDocument();
    expect(within(why).queryByRole("button")).not.toBeInTheDocument();
    expect(mockRetryParked).not.toHaveBeenCalled();
  });

  it("shows a held file's reason for waiting without dressing it as a failure", async () => {
    // The shape this whole feature exists to make legible: a large file's
    // pointer is committed, its upload has not landed, and the push that would
    // publish it is deferred carrying the reason. It is `inProgress`, not
    // `failed` — reporting it as broken would accuse keeper of failing while it
    // is doing the one careful thing that keeps a peer from cloning a pointer to
    // content nobody has.
    const held =
      "publishing is on hold until this folder's large files reach the remote (1 outstanding)";
    mockActivity.mockResolvedValue([
      activityVm({
        path: "footage/clip.mp4",
        delivery: "inProgress",
        failure: held,
        unitId: 91,
      }),
    ]);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    fireEvent.click(
      screen.getByRole("button", { name: `${SYNC_DELIVERY_DETAIL_LABEL}: footage/clip.mp4` }),
    );

    const why = await screen.findByRole("dialog");
    const reason = within(why).getByText(held);
    expect(reason).toBeInTheDocument();
    // Toned as the state is, so a wait does not read as a breakage.
    expect(reason.className).toContain(SYNC_DELIVERY_STATES.inProgress.tone);
    expect(reason.className).not.toContain(SYNC_DELIVERY_STATES.failed.tone);
    // And no Retry: nothing has stopped, and there is nothing for a human to
    // restart.
    expect(within(why).queryByRole("button")).not.toBeInTheDocument();
  });
});

describe("SyncPane pending", () => {
  const pending: SyncPendingVm[] = [
    { path: "notes/draft.md", reason: "settling", sinceMs: NOW - 300_000, sizeBytes: null },
    { path: "notes/scratch.md", reason: "untracked", sinceMs: null, sizeBytes: null },
  ];

  /**
   * The lists are paths — repository-relative, four folders deep, beside a size
   * and a date. Capped at 720px they truncate into ellipses while the window
   * sits half empty, and the tail of a path is the half that identifies it.
   *
   * The forms are the opposite case and keep their measure, which is why this
   * asserts both halves rather than "no max-width anywhere".
   */
  it("gives the lists the whole window and the forms a measure", async () => {
    await renderPane();
    // Any rendered row will do; this one is the folder card's own line.
    await screen.findByText(RUST_LINE);

    const body = document.querySelector('[data-slot="sync-body"]');
    expect(body).not.toBeNull();
    expect(body?.className).not.toMatch(/max-w-/);
    expect(body?.className).not.toMatch(/mx-auto/);
  });

  it("lists what is waiting and why", async () => {
    mockPending.mockResolvedValue(pending);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("notes/draft.md");
    expect(rows[1]).toHaveTextContent("notes/scratch.md");
    // The reason is the row's accessible description now, not visible prose:
    // the glyph carries it for a reader who can see it.
    expect(rows[1]).toHaveTextContent("New file");
    expect(rows[1]).toHaveTextContent(SYNC_PENDING_OUTBOUND_WORD);
  });

  /** The list carries both directions now, so each row has to say which one it
   * is without the reader parsing the sentence at the far end. */
  it("marks which way each pending row is travelling", async () => {
    mockPending.mockResolvedValue([
      { path: "notes/scratch.md", reason: "untracked", sinceMs: null, sizeBytes: null },
      {
        path: "70-comms/camera-0001.mov",
        reason: "incoming",
        sinceMs: null,
        sizeBytes: 405_800_000,
      },
    ]);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent(SYNC_PENDING_OUTBOUND_WORD);
    expect(rows[1]).toHaveTextContent(SYNC_PENDING_INBOUND_WORD);
    // Both halves of the mark: which way, and whether the far end already has
    // something. A bare arrow is new content; a circled one is a second version.
    expect(rows[0]).toHaveTextContent("New file");
    expect(rows[1]).toHaveTextContent("New file");
    // The size is a column of its own, and both directions have one — the
    // uploads used to show none, which is what made the list read as two.
    expect(rows[1]).toHaveTextContent("405.8 MB");
    // And no prose: the sentence lives in the accessible name, not the row.
    expect(rows[1]).not.toHaveTextContent("Waiting to download ·");
  });

  /** Which of the queued rows is the one actually moving right now. */
  it("marks the row the transfer is on, and only that one", async () => {
    mockPending.mockResolvedValue([
      { path: "70-comms/camera-0000.mov", reason: "incoming", sinceMs: null, sizeBytes: 9_500_000 },
      {
        path: "70-comms/camera-0001.mov",
        reason: "incoming",
        sinceMs: null,
        sizeBytes: 405_800_000,
      },
    ]);
    await renderPane();

    act(() => {
      emitProgress?.(
        progressVm({
          phase: "downloadingLfs",
          fraction: 0.2,
          current: "70-comms/camera-0001.mov",
          bytesPerSecond: 294_800,
        }),
      );
    });

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows[0]).not.toHaveAttribute("aria-current");
    expect(rows[1]).toHaveAttribute("aria-current", "true");
    // Named, not merely shaded: a background colour is nothing to a screen
    // reader and nothing to anyone who cannot separate these two greys.
    expect(rows[1]).toHaveTextContent(SYNC_PENDING_CURRENT_WORD);
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

describe("SyncPane list folding", () => {
  /** `n` activity rows, each with a distinct path so they are tellable apart. */
  const rows = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      activityVm({ tsMs: NOW - i * 60_000, path: `notes/file-${i}.md` }),
    );

  it("shows only the folded count and unfolds to the rest on request", async () => {
    mockActivity.mockResolvedValue(rows(25));
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(10));
    // The newest is kept and the oldest dropped: a recent-history surface that
    // folded away the recent half would be useless.
    expect(within(list).getByTitle("notes/file-0.md")).toBeInTheDocument();
    expect(within(list).queryByTitle("notes/file-10.md")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Show all 25/ }));

    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(25));
    expect(within(list).getByTitle("notes/file-24.md")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Show fewer/ }));
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(10));
  });

  it("offers no fold when the whole list already fits", async () => {
    mockActivity.mockResolvedValue(rows(4));
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    // Four rows below a fold of ten: the control would do nothing in either
    // direction, so it must not be drawn at all.
    expect(screen.queryByRole("button", { name: /^Show all/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Show fewer/ })).not.toBeInTheDocument();
  });

  it("never offers to reveal more rows than were read", async () => {
    // 40 rows with an unfolded size of 12: the query asked Rust for 12, so
    // "Show all 40" would be a promise nothing can keep.
    mockListSettings.mockResolvedValue({ folded: 3, unfolded: 12 });
    mockActivity.mockResolvedValue(rows(12));
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(3));
    expect(screen.getByRole("button", { name: /^Show all 12/ })).toBeInTheDocument();
  });

  it("reads history up to the unfolded size, so the setting bounds the query", async () => {
    mockListSettings.mockResolvedValue({ folded: 5, unfolded: 250 });
    await renderPane();

    // The saved unfolded count IS the `LIMIT`: without this the fold could only
    // ever reveal rows the default read happened to include.
    await waitFor(() => expect(mockActivity).toHaveBeenCalledWith("p1", 250));
  });

  it("folds the parked list too, without narrowing what Retry all covers", async () => {
    mockListSettings.mockResolvedValue({ folded: 2, unfolded: 100 });
    mockProblems.mockResolvedValue(
      problemsVm({
        parked: Array.from({ length: 5 }, (_, i) => ({
          id: 40 + i,
          kind: "lfsUpload",
          attempts: 3,
          lastError: `rejected ${i}`,
        })),
      }),
    );
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PARKED_TITLE}: tgdrive` });
    await waitFor(() => expect(within(list).getAllByRole("listitem")).toHaveLength(2));

    // The fold is about reading, not about scope: the bulk retry still requeues
    // every parked unit, including the three currently hidden.
    fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${SYNC_RETRY_ALL_LABEL}`) }));
    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledTimes(5));
    expect(mockRetryParked.mock.calls.map(([, id]) => id)).toEqual([40, 41, 42, 43, 44]);
  });
});

describe("SyncPane recheck", () => {
  it("forgets the remembered tree for the folder it was pressed on", async () => {
    await renderPane();

    fireEvent.click(await screen.findByRole("button", { name: SYNC_RESCAN_LABEL }));

    // Named for the profile, not for "all folders": the button lives on one
    // card and must not quietly re-walk the others.
    await waitFor(() => expect(mockRescan).toHaveBeenCalledWith("p1"));
    expect(mockRescan).toHaveBeenCalledTimes(1);
  });

  it("explains, on hover, the one symptom it is the answer to", async () => {
    await renderPane();

    const button = await screen.findByRole("button", { name: SYNC_RESCAN_LABEL });
    // The button is an exception, not a habit — the note is what stops it being
    // pressed as a ritual after every copy.
    expect(button).toHaveAttribute("title", SYNC_RESCAN_NOTE);
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

  it("retries every parked unit from one press", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({
        parked: [
          { id: 41, kind: "push", attempts: 5, lastError: "remote hung up" },
          { id: 42, kind: "lfsUpload", attempts: 2, lastError: null },
          { id: 43, kind: "lfsUpload", attempts: 10, lastError: "rejected" },
        ],
      }),
    );
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    fireEvent.click(
      await screen.findByRole("button", {
        name: `${SYNC_RETRY_ALL_LABEL}: 3 ${SYNC_PARKED_TITLE.toLowerCase()}, tgdrive`,
      }),
    );

    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledTimes(3));
    expect(mockRetryParked.mock.calls.map(([, unitId]) => unitId)).toEqual([41, 42, 43]);
  });

  it("retries the units behind one that will not requeue, and still reports it", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({
        parked: [
          { id: 41, kind: "push", attempts: 5, lastError: "remote hung up" },
          { id: 42, kind: "lfsUpload", attempts: 2, lastError: null },
        ],
      }),
    );
    // The first unit is the one that fails: a bulk action that gave up on the
    // rest would be worse than not offering the button at all.
    mockRetryParked.mockRejectedValueOnce({ code: "journal", message: "unit is gone" });
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    fireEvent.click(
      await screen.findByRole("button", {
        name: `${SYNC_RETRY_ALL_LABEL}: 2 ${SYNC_PARKED_TITLE.toLowerCase()}, tgdrive`,
      }),
    );

    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledTimes(2));
    expect(mockRetryParked.mock.calls.map(([, unitId]) => unitId)).toEqual([41, 42]);
    // The rejection is surfaced rather than swallowed into a silent success.
    expect(await screen.findByText("unit is gone")).toBeInTheDocument();
  });

  it("offers no bulk retry for a single parked unit", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({ parked: [{ id: 41, kind: "push", attempts: 5, lastError: "remote hung up" }] }),
    );
    await renderPane();

    // The row's own Retry is already the whole action; a second button beside it
    // would do exactly the same thing.
    await screen.findByRole("list", { name: `${SYNC_PARKED_TITLE}: tgdrive` });
    expect(
      screen.queryByRole("button", { name: new RegExp(`^${SYNC_RETRY_ALL_LABEL}`) }),
    ).not.toBeInTheDocument();
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

  it("reports a name that is not text, in both the readable and the byte-exact form", async () => {
    // DW-200. The engine finds these; before this they reached no surface, so
    // keeper knew about a file it never mentioned. Two entries whose LOSSY
    // renderings are identical — which is the whole hazard: `a\xff.txt` and
    // `a\xfe.txt` read the same to a person and are two different files.
    // React only *warns* about duplicate keys, so the row count below cannot
    // catch a pane keyed on the lossy `display`. Captured and restored around
    // the render, and asserted after, so a failure here never leaves the rest
    // of the file with a muted console.
    const complaints: string[] = [];
    const spy = vi
      .spyOn(console, "error")
      .mockImplementation((...args: unknown[]) => complaints.push(args.map(String).join(" ")));
    mockProblems.mockResolvedValue(
      problemsVm({
        unspellable: [
          { display: "a\uFFFD.txt", escaped: "a\\xff.txt" },
          { display: "a\uFFFD.txt", escaped: "a\\xfe.txt" },
        ],
      }),
    );
    let list: HTMLElement;
    try {
      await renderPane();
      list = await screen.findByRole("list", { name: `${SYNC_UNSPELLABLE_TITLE}: tgdrive` });
    } finally {
      spy.mockRestore();
    }
    expect(within(list).getAllByRole("listitem")).toHaveLength(2);
    // The byte-exact form is what makes the row actionable — a person can paste
    // it into a shell. Without it the two rows above are indistinguishable.
    expect(within(list).getByText("a\\xff.txt")).toBeInTheDocument();
    expect(within(list).getByText("a\\xfe.txt")).toBeInTheDocument();
    // …and the readable form is still shown, because `a\uFFFD.txt` is what the
    // person will recognise in their file manager.
    expect(within(list).getAllByText("a\uFFFD.txt")).toHaveLength(2);
    expect(screen.getByText(SYNC_UNSPELLABLE_SENTENCE)).toBeInTheDocument();
    // The rows must be KEYED on the byte-exact name, and this is the assertion
    // that says so. A mutation that keyed on `display` SURVIVED until this line
    // existed, because React renders duplicate-keyed siblings and only warns.
    // It is not pedantry: this list is the one place in the app where two
    // entries routinely share a `display`, so a duplicate key here is a real
    // reconciliation hazard the moment the list changes.
    expect(complaints.filter((line) => line.includes("same key"))).toEqual([]);
  });

  it("says nothing about names when every name in the folder is text", async () => {
    // The section must not appear on the overwhelmingly common folder, and
    // "Problems" with an empty body is a worry with no cause (AD-S5).
    //
    // The folder has a DIFFERENT problem, deliberately: with nothing at all
    // wrong the whole Problems section is absent and this test would pass
    // against a name list rendered unconditionally. Asked this way it pins the
    // one conditional it is about.
    mockProblems.mockResolvedValue(problemsVm({ warning: "Large files are missing." }));
    await renderPane();
    await screen.findByText("Large files are missing.");

    expect(screen.queryByText(SYNC_UNSPELLABLE_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByText(SYNC_UNSPELLABLE_SENTENCE)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: `${SYNC_UNSPELLABLE_TITLE}: tgdrive` }),
    ).not.toBeInTheDocument();
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
    expect(
      syncPendingReason({ path: "a", reason: "modified", sinceMs: null, sizeBytes: null }),
    ).toBe("Changed, not synced yet");
    // A settling row with no recorded start still says what it is waiting for.
    expect(
      syncPendingReason({ path: "a", reason: "settling", sinceMs: null, sizeBytes: null }),
    ).toBe(SYNC_SETTLING_SENTENCE);
    // A reason Rust grows later is shown, not swallowed.
    expect(
      syncPendingReason({ path: "a", reason: "quarantined", sinceMs: null, sizeBytes: null }),
    ).toBe("quarantined");
    // The size is NOT in here any more: it is a column of its own, on every
    // row, so this composes the accessible description alone.
    expect(
      syncPendingReason({
        path: "70-comms/camera-0001.mov",
        reason: "incoming",
        sinceMs: null,
        sizeBytes: 405_800_000,
      }),
    ).toBe("Waiting to download");
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
    // A phase that carries bytes at all: `pushing` publishes a file count and
    // never a byte one, so a byte-counted snapshot under it is a shape the
    // engine cannot produce (Story 34.8).
    const busy = statusVm({
      state: "syncing",
      phase: "uploadingLfs",
      bytesDone: 250,
      bytesTotal: 1000,
    });
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
    const busy = statusVm({ state: "syncing", phase: "uploadingLfs" });
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
