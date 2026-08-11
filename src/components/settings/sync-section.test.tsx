import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  syncProfileSave: vi.fn(),
  syncProfileRemove: vi.fn(),
  syncProfileSetEnabled: vi.fn(),
  syncFolderNow: vi.fn(),
  syncVerify: vi.fn(),
  // The path control (Story 32.4): resolves and opens the folder in Rust.
  syncOpenPath: vi.fn(),
  // The device name (Story 34.5): read once when the section opens, written by
  // the Rename button.
  syncDevice: vi.fn(),
  syncDeviceSetLabel: vi.fn(),
  // The Advanced disclosure's access-token field (Story 32.7, Story 34.4): two
  // writes and a read the user has to ask for.
  syncSetCredential: vi.fn(),
  syncGetCredential: vi.fn(),
  syncClearCredential: vi.fn(),
  // A successful add re-reads the new folder's three detail lists so the Sync
  // view is not blank for a poll interval — the same add path runs from here.
  syncActivity: vi.fn(() => Promise.resolve([])),
  syncPending: vi.fn(() => Promise.resolve([])),
  syncProblems: vi.fn(() =>
    Promise.resolve({ warning: null, error: null, parked: [], conflicts: [], unspellable: [] }),
  ),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

import { open as openFolder } from "@tauri-apps/plugin-dialog";
import {
  DeviceSection,
  SYNC_ATTENTION_FALLBACK_SENTENCE,
  SYNC_DEVICE_ID_LABEL,
  SYNC_DEVICE_NAME_LABEL,
  SYNC_DEVICE_SAVE_LABEL,
  SYNC_DEVICE_SAVED_SENTENCE,
  SYNC_NO_PROFILES_SENTENCE,
  SYNC_NOW_LABEL,
  SYNC_OPEN_PATH_LABEL,
  SYNC_PAUSE_LABEL,
  SYNC_PROGRESS_LABEL,
  SYNC_REMOVE_CANCEL_LABEL,
  SYNC_REMOVE_CONFIRM_LABEL,
  SYNC_REMOVE_CONFIRM_SENTENCE,
  SYNC_REMOVE_LABEL,
  SYNC_RESUME_LABEL,
  SYNC_VERIFY_LABEL,
  SyncSection,
  syncRemoteHost,
} from "@/components/settings/sync-section";
import {
  SYNC_ADD_SUBMIT_LABEL,
  SYNC_ADD_TITLE,
  SYNC_ADVANCED_TOGGLE_TESTID,
  SYNC_AUTHOR_LABEL,
  SYNC_CHOOSE_FOLDER_LABEL,
  SYNC_EDIT_SUBMIT_LABEL,
  SYNC_EDIT_TITLE,
  SYNC_EXCLUDES_LABEL,
  SYNC_FORM_PATH_TESTID,
  SYNC_NAME_LABEL,
  SYNC_REMOTE_URL_LABEL,
  SYNC_SETTLE_LABEL,
  SYNC_SUBPATHS_LABEL,
  SYNC_TAGS_LABEL,
  SYNC_TOKEN_EDIT_NOTE,
  SYNC_TOKEN_FAILED_PREFIX,
  SYNC_TOKEN_LABEL,
  SYNC_TOKEN_SHOW_LABEL,
} from "@/components/sync/add-folder-form";
import type { SyncOutcomeVm, SyncProfileVm, SyncStatusVm } from "@/lib/ipc/client";
import {
  syncDevice,
  syncDeviceSetLabel,
  syncFolderNow,
  syncGetCredential,
  syncOpenPath,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncSetCredential,
  syncStatuses,
  syncVerify,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { resetSyncStoreForTest } from "@/lib/stores/sync";
import { resetSyncDetailStoreForTest } from "@/lib/stores/sync-detail";

const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockSave = vi.mocked(syncProfileSave);
const mockRemove = vi.mocked(syncProfileRemove);
const mockSetEnabled = vi.mocked(syncProfileSetEnabled);
const mockFolderNow = vi.mocked(syncFolderNow);
const mockVerify = vi.mocked(syncVerify);
const mockSetCredential = vi.mocked(syncSetCredential);
const mockGetCredential = vi.mocked(syncGetCredential);
const mockDevice = vi.mocked(syncDevice);
const mockSetDeviceLabel = vi.mocked(syncDeviceSetLabel);
const mockPicker = vi.mocked(openFolder);
const mockOpenPath = vi.mocked(syncOpenPath);

/** The exact line Rust composes — the UI must render it character for character. */
const RUST_LINE = "tgdrive — 3 waiting to sync";
const RUST_TRANSFER_LINE = "Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB";

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

beforeEach(() => {
  resetSyncStoreForTest();
  resetSyncDetailStoreForTest();
  mockProfiles.mockResolvedValue([profileVm()]);
  mockStatuses.mockResolvedValue([statusVm()]);
  mockPicker.mockResolvedValue(null);
  mockDevice.mockResolvedValue({ id: "01JDEVICE", label: "hesperia" });
  // Every edit form reads the keychain as it opens (Story 34.12); this is the
  // answer for the folders that have nothing stored.
  mockGetCredential.mockResolvedValue(null);
  mockOpenPath.mockResolvedValue(undefined);
  // The path control is gated on a real file manager existing, so these rows
  // render as a desktop would show them.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("SyncSection hydration", () => {
  it("disables the add-profile controls until the mirror hydrates, claiming nothing meanwhile", () => {
    // An empty race never settles: the read stays in flight for the whole test,
    // so the surface must hold its honestly-unknown state.
    mockProfiles.mockReturnValue(Promise.race([]));
    render(<SyncSection open />);

    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toBeDisabled();
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toBeDisabled();
    expect(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL })).toBeDisabled();
    // No "nothing configured" claim before a read has landed.
    expect(screen.queryByText(SYNC_NO_PROFILES_SENTENCE)).not.toBeInTheDocument();
  });

  it("enables the form and says so plainly once a read returns no profiles", async () => {
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    render(<SyncSection open />);

    expect(await screen.findByText(SYNC_NO_PROFILES_SENTENCE)).toBeInTheDocument();
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toBeEnabled();
  });

  it("surfaces a failed read without pretending the list is empty", async () => {
    mockProfiles.mockRejectedValue({ code: "internal", message: "engine unavailable" });
    render(<SyncSection open />);

    expect(await screen.findByText("engine unavailable")).toBeInTheDocument();
    expect(screen.queryByText(SYNC_NO_PROFILES_SENTENCE)).not.toBeInTheDocument();
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toBeDisabled();
  });
});

describe("SyncSection profile rows", () => {
  it("renders the Rust-composed status line verbatim, with the path and remote host", async () => {
    render(<SyncSection open />);

    expect(await screen.findByText(RUST_LINE)).toBeInTheDocument();
    const path = screen.getByText("/Users/alice/Documents/tgdrive");
    expect(path).toHaveAttribute("title", "/Users/alice/Documents/tgdrive");
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

  /**
   * AD-34-12. The click used to produce nothing at all: the command returned a
   * full outcome and the row threw it away, so "even after clicking Sync now I
   * cannot see that sync works" was literally true. Each honest case has to
   * land on screen, and the sentence comes from Rust so this row and the Sync
   * view cannot word one result two ways.
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
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

    await waitFor(() => expect(mockFolderNow).toHaveBeenCalledWith("p1"));
    const report = await screen.findByText("Committed and pushed 3 files, moved 2 KB.");
    // Announced, not just painted: the result of a button press has to reach a
    // screen reader without stealing focus.
    expect(report).toHaveAttribute("role", "status");
    expect(report).not.toHaveClass("text-destructive");
  });

  it("says so when there was nothing to sync, rather than nothing", async () => {
    mockFolderNow.mockResolvedValue(outcomeVm());
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

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
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

    const report = await screen.findByText(
      "Kept your version of 1 file that changed in both places, alongside the remote's.",
    );
    expect(report).toHaveClass("text-destructive");
  });

  it("surfaces a rejected row action inline", async () => {
    mockFolderNow.mockRejectedValue({ code: "serverUnreachable", message: "remote unreachable" });
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

    expect(await screen.findByText("remote unreachable")).toBeInTheDocument();
    // And nothing claims the pass did anything.
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("pauses and resumes, reflecting the status the backend returned", async () => {
    const paused = statusVm({ state: "paused", pending: 0, line: "tgdrive — paused" });
    mockSetEnabled.mockImplementation(async (_id, enabled) => {
      // Model the backend: the write moves both halves of the mirror.
      mockProfiles.mockResolvedValue([profileVm({ enabled })]);
      mockStatuses.mockResolvedValue([enabled ? statusVm() : paused]);
      return enabled ? statusVm() : paused;
    });
    render(<SyncSection open />);

    fireEvent.click(await screen.findByRole("button", { name: SYNC_PAUSE_LABEL }));

    expect(await screen.findByText("tgdrive — paused")).toBeInTheDocument();
    expect(mockSetEnabled).toHaveBeenLastCalledWith("p1", false);
    const resume = await screen.findByRole("button", { name: SYNC_RESUME_LABEL });

    fireEvent.click(resume);

    expect(await screen.findByText(RUST_LINE)).toBeInTheDocument();
    expect(mockSetEnabled).toHaveBeenLastCalledWith("p1", true);
    expect(screen.getByRole("button", { name: SYNC_PAUSE_LABEL })).toBeInTheDocument();
  });

  /**
   * Story 32.4. The path was on screen as plain text with no way to reach the
   * folder from the app at all, so the path itself is the control.
   */
  it("opens the folder from the path itself, asking for it by profile id", async () => {
    render(<SyncSection open />);

    fireEvent.click(
      await screen.findByRole("button", {
        name: `${SYNC_OPEN_PATH_LABEL}: /Users/alice/Documents/tgdrive`,
      }),
    );

    // The id, never a path: the frontend cannot name a folder here, so it cannot
    // ask for one keeper does not already sync.
    await waitFor(() => expect(mockOpenPath).toHaveBeenCalledWith("p1"));
    expect(mockOpenPath).toHaveBeenCalledTimes(1);
  });

  it("leaves the path as plain text where there is no file manager to open it in", async () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<SyncSection open />);

    // Still readable — just not a control that would fail on activation.
    expect(await screen.findByText("/Users/alice/Documents/tgdrive")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: new RegExp(SYNC_OPEN_PATH_LABEL) }),
    ).not.toBeInTheDocument();
  });
});

describe("SyncSection needs-attention notice", () => {
  it("renders a persistent alert with the error text and no dismiss control", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({
        state: "needsAttention",
        needsAttention: true,
        error: "authentication failed for github.com",
        warning: "an older warning",
      }),
    ]);
    render(<SyncSection open />);

    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText("authentication failed for github.com")).toBeInTheDocument();
    // Cleared by recovery, never by waving it away.
    const dismissish = within(alert)
      .queryAllByRole("button")
      .filter((button) => /dismiss|close|ok|got it|hide/i.test(button.textContent ?? ""));
    expect(dismissish).toHaveLength(0);
  });

  it("falls back to the warning, then to the fixed sentence", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ needsAttention: true, warning: "the volume is not mounted" }),
    ]);
    const { unmount } = render(<SyncSection open />);
    expect(await screen.findByText("the volume is not mounted")).toBeInTheDocument();
    unmount();

    resetSyncStoreForTest();
    mockStatuses.mockResolvedValue([statusVm({ needsAttention: true })]);
    render(<SyncSection open />);

    expect(await screen.findByText(SYNC_ATTENTION_FALLBACK_SENTENCE)).toBeInTheDocument();
  });

  it("checks the files from the alert and lists what it found", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ needsAttention: true, error: "content does not match" }),
    ]);
    mockVerify.mockResolvedValue(["notes/a.md: digest mismatch"]);
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_VERIFY_LABEL }));

    expect(await screen.findByText("notes/a.md: digest mismatch")).toBeInTheDocument();
    expect(mockVerify).toHaveBeenCalledWith("p1");
  });

  it("does not render an alert for a healthy profile", async () => {
    render(<SyncSection open />);

    await screen.findByText(RUST_LINE);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});

describe("SyncSection progress meter", () => {
  it("is indeterminate when no total is known", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", line: RUST_TRANSFER_LINE, bytesDone: 900 }),
    ]);
    render(<SyncSection open />);

    const meter = await screen.findByRole("progressbar", {
      name: `${SYNC_PROGRESS_LABEL}: tgdrive`,
    });
    expect(meter).not.toHaveAttribute("aria-valuenow");
    expect(meter).toHaveAttribute("aria-valuemin", "0");
    expect(meter).toHaveAttribute("aria-valuemax", "100");
    // The human description stays the one Rust composed.
    expect(meter).toHaveAttribute("aria-valuetext", RUST_TRANSFER_LINE);
  });

  it("reports the byte fraction when a byte total is known", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", bytesDone: 250, bytesTotal: 1000 }),
    ]);
    render(<SyncSection open />);

    const meter = await screen.findByRole("progressbar");
    expect(meter).toHaveAttribute("aria-valuenow", "25");
  });

  it("clamps rather than overflowing when more moved than the total claimed", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", bytesDone: 5000, bytesTotal: 1000 }),
    ]);
    render(<SyncSection open />);

    const meter = await screen.findByRole("progressbar");
    expect(meter).toHaveAttribute("aria-valuenow", "100");
  });

  it("shows no meter for a settled profile", async () => {
    mockStatuses.mockResolvedValue([statusVm({ state: "idle", pending: 0 })]);
    render(<SyncSection open />);

    await screen.findByText(RUST_LINE);
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });
});

describe("SyncSection remove", () => {
  it("confirms first, and says the folder and its contents stay on disk", async () => {
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_REMOVE_LABEL }));

    // Nothing is forgotten until the confirmation is accepted.
    expect(mockRemove).not.toHaveBeenCalled();
    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText(SYNC_REMOVE_CONFIRM_SENTENCE)).toBeInTheDocument();
    expect(SYNC_REMOVE_CONFIRM_SENTENCE).toMatch(/left on disk/);
    expect(SYNC_REMOVE_CONFIRM_SENTENCE).toMatch(/never deletes/);

    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    fireEvent.click(within(dialog).getByRole("button", { name: SYNC_REMOVE_CONFIRM_LABEL }));

    await waitFor(() => expect(mockRemove).toHaveBeenCalledWith("p1"));
    expect(await screen.findByText(SYNC_NO_PROFILES_SENTENCE)).toBeInTheDocument();
  });

  it("keeps the profile when the confirmation is cancelled", async () => {
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_REMOVE_LABEL }));
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: SYNC_REMOVE_CANCEL_LABEL }));

    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(mockRemove).not.toHaveBeenCalled();
    expect(screen.getByText(RUST_LINE)).toBeInTheDocument();
  });
});

describe("SyncSection edit a folder", () => {
  it("corrects the row's own profile in place, rather than by removing and re-adding it", async () => {
    render(<SyncSection open />);

    fireEvent.click(await screen.findByRole("button", { name: SYNC_EDIT_TITLE }));

    // This is the surface that removes a folder, so until now it was also the
    // surface where a mistyped remote meant removing it and starting over.
    const form = await screen.findByRole("form", { name: `${SYNC_EDIT_TITLE}: tgdrive` });
    const remote = within(form).getByLabelText(SYNC_REMOTE_URL_LABEL);
    expect(remote).toHaveValue("git@github.com:alice/tgdrive.git");

    const fixed = "git@github.com:alice/tgdrive-2.git";
    fireEvent.change(remote, { target: { value: fixed } });
    mockSave.mockResolvedValue(profileVm({ remoteUrl: fixed }));
    mockProfiles.mockResolvedValue([profileVm({ remoteUrl: fixed })]);
    fireEvent.click(within(form).getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ id: "p1", remoteUrl: fixed }),
      ),
    );
    expect(mockRemove).not.toHaveBeenCalled();
    // The add form below the list is a different form, and it stays put.
    await waitFor(() =>
      expect(
        screen.queryByRole("form", { name: `${SYNC_EDIT_TITLE}: tgdrive` }),
      ).not.toBeInTheDocument(),
    );
    expect(screen.getByRole("form", { name: SYNC_ADD_TITLE })).toBeInTheDocument();
  });
});

describe("SyncSection add-profile form", () => {
  it("saves a folder chosen from the picker and clears the form", async () => {
    mockPicker.mockResolvedValue("/Users/alice/notes");
    mockSave.mockResolvedValue(profileVm({ id: "p2", name: "notes" }));
    render(<SyncSection open />);
    await screen.findByText(RUST_LINE);

    fireEvent.change(screen.getByLabelText(SYNC_NAME_LABEL), { target: { value: "notes" } });
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/notes.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));
    await waitFor(() =>
      expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Users/alice/notes"),
    );

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // An untouched Advanced disclosure still sends the defaults it always did:
    // the new knobs (Story 32.7) are absent-by-default, not opinions the form
    // now imposes on every folder.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith({
        id: null,
        name: "notes",
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
        // An empty box means "keeper picks", and keeper's own number is how that
        // is spelled on the wire: `null` would be the omission Rust reads as
        // "leave whatever is stored" (AD-34-9), a different instruction.
        settleMs: 5_000,
        pollIntervalMs: 15_000,
        tags: [],
        authorOverride: null,
        commitSubjectTemplate: "",
        notes: false,
        notesSubfolder: null,
        // The recordings switch is off and untouched, so the save says "this
        // folder holds none" and names no subfolder at all (Story 41.7).
        recordings: false,
        recordingsSubfolder: null,
      }),
    );
    // Nothing was typed into the token field, so the keychain was left alone.
    expect(mockSetCredential).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue(""));
  });

  it("writes nothing when the folder picker is cancelled", async () => {
    mockPicker.mockResolvedValue(null);
    render(<SyncSection open />);
    await screen.findByText(RUST_LINE);

    fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(mockPicker).toHaveBeenCalled());
    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).not.toHaveTextContent("/Users");
    expect(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL })).toBeDisabled();
  });

  it("shows a rejected save inline and keeps every typed value", async () => {
    mockPicker.mockResolvedValue("relative/path");
    mockSave.mockRejectedValue({
      code: "internal",
      message: "local path must be absolute, got relative/path",
    });
    render(<SyncSection open />);
    await screen.findByText(RUST_LINE);

    fireEvent.change(screen.getByLabelText(SYNC_NAME_LABEL), { target: { value: "notes" } });
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/half-typed" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));
    await waitFor(() =>
      expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("relative/path"),
    );
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    expect(
      await screen.findByText("local path must be absolute, got relative/path"),
    ).toBeInTheDocument();
    // Nothing typed is lost to a validation error.
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("notes");
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue(
      "git@github.com:alice/half-typed",
    );
    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("relative/path");
  });
});

describe("SyncSection advanced options (Story 32.7)", () => {
  /** Fill the three required fields and open the Advanced disclosure. */
  async function openAdvanced() {
    mockPicker.mockResolvedValue("/Users/alice/notes");
    mockSave.mockResolvedValue(profileVm({ id: "p2", name: "notes" }));
    render(<SyncSection open />);
    await screen.findByText(RUST_LINE);

    fireEvent.change(screen.getByLabelText(SYNC_NAME_LABEL), { target: { value: "notes" } });
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/notes.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));
    await waitFor(() =>
      expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Users/alice/notes"),
    );
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
  }

  it("sends every advanced value as typed instead of hardcoding it away", async () => {
    await openAdvanced();

    fireEvent.change(screen.getByLabelText(SYNC_SETTLE_LABEL), { target: { value: "12" } });
    fireEvent.change(screen.getByLabelText(SYNC_EXCLUDES_LABEL), {
      target: { value: "*.tmp, .DS_Store" },
    });
    fireEvent.change(screen.getByLabelText(SYNC_SUBPATHS_LABEL), {
      // The trailing comma must not reach Rust as an empty subpath.
      target: { value: "notes, drafts," },
    });
    fireEvent.change(screen.getByLabelText(SYNC_TAGS_LABEL), { target: { value: "drive" } });
    fireEvent.change(screen.getByLabelText(SYNC_AUTHOR_LABEL), {
      target: { value: "Alice <alice@example.org>" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          // Seconds in the field, milliseconds on the wire.
          settleMs: 12_000,
          excludes: ["*.tmp", ".DS_Store"],
          subpaths: ["notes", "drafts"],
          tags: ["drive"],
          authorOverride: "Alice <alice@example.org>",
        }),
      ),
    );
  });

  it("stores a typed token against the saved profile and never renders it back", async () => {
    mockSetCredential.mockResolvedValue(undefined);
    await openAdvanced();

    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    // Write-only in the literal sense: the browser must not echo it either.
    expect(token).toHaveAttribute("type", "password");
    fireEvent.change(token, { target: { value: "ghp_secret" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // Keyed by the id the save minted, which is why it is a second write.
    await waitFor(() => expect(mockSetCredential).toHaveBeenCalledWith("p2", "ghp_secret"));
    // The add form goes back to a blank draft; the token it just stored belongs
    // to the folder now, and the folder's edit form is where it is seen again.
    await waitFor(() => expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue(""));
    expect(document.body.textContent).not.toContain("ghp_secret");
  });

  it("hands the edit form the stored token, masked, without a reveal step", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    render(<SyncSection open />);

    fireEvent.click(await screen.findByRole("button", { name: SYNC_EDIT_TITLE }));
    const form = await screen.findByRole("form", { name: `${SYNC_EDIT_TITLE}: tgdrive` });
    fireEvent.click(within(form).getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    // Story 34.12: the read happens because the form opened, and the only thing
    // standing between the secret and the screen is the eye.
    const token = within(form).getByLabelText(SYNC_TOKEN_LABEL);
    await waitFor(() => expect(token).toHaveValue("ghp_stored"));
    expect(token).toHaveAttribute("type", "password");
    expect(within(form).getByText(SYNC_TOKEN_EDIT_NOTE)).toBeInTheDocument();

    fireEvent.click(within(form).getByRole("button", { name: SYNC_TOKEN_SHOW_LABEL }));

    expect(token).toHaveAttribute("type", "text");
  });

  it("keeps the typed token on screen when only the keychain write failed", async () => {
    mockSetCredential.mockRejectedValue({ code: "internal", message: "keychain refused" });
    await openAdvanced();

    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // Two writes, two outcomes: the profile exists, so "add failed" would be a
    // lie that sends the user back to a form that can only add the folder twice.
    expect(
      await screen.findByText(`${SYNC_TOKEN_FAILED_PREFIX}keychain refused`),
    ).toBeInTheDocument();
    // And the draft is NOT blanked, which is the whole point of holding the
    // form open: the token in the box is the thing that failed to land, and a
    // reset would send the user back to their forge for a second PAT. This
    // asserted the opposite until the epic-34 review caught it (finding 2 of
    // the 34-12 audit) — the reset used to run before the keychain write.
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("ghp_secret");
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("notes");
  });
});

describe("syncRemoteHost", () => {
  it("names the host for both remote spellings, and never blanks an odd one", () => {
    expect(syncRemoteHost("git@github.com:alice/tgdrive.git")).toBe("github.com");
    expect(syncRemoteHost("https://user@git.example.org/alice/tgdrive.git")).toBe(
      "git.example.org",
    );
    expect(syncRemoteHost("ssh://git@git.example.org:2222/alice/x.git")).toBe(
      "git.example.org:2222",
    );
    expect(syncRemoteHost("/srv/mirrors/tgdrive.git")).toBe("/srv/mirrors/tgdrive.git");
  });
});

/**
 * The device name moved out of Sync into its own section — it is not a sync
 * setting, it names the machine. Same assertions, pointed at the component that
 * owns them now.
 */
describe("DeviceSection (Story 34.5)", () => {
  it("shows the name and the id keeper writes into every commit", async () => {
    render(<DeviceSection open />);

    const field = await screen.findByLabelText(SYNC_DEVICE_NAME_LABEL);
    expect(field).toHaveValue("hesperia");
    // The id is in every trailer, so someone reading `git log` can find it here.
    expect(screen.getByText(`${SYNC_DEVICE_ID_LABEL}: 01JDEVICE`)).toBeInTheDocument();
  });

  it("renames on request, keeps the id, and says the change is for later commits", async () => {
    mockSetDeviceLabel.mockResolvedValue({ id: "01JDEVICE", label: "Studio Mac" });
    render(<DeviceSection open />);

    const field = await screen.findByLabelText(SYNC_DEVICE_NAME_LABEL);
    const rename = screen.getByRole("button", { name: SYNC_DEVICE_SAVE_LABEL });
    // Nothing to do until the name actually differs.
    expect(rename).toBeDisabled();

    fireEvent.change(field, { target: { value: "  Studio Mac  " } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_DEVICE_SAVE_LABEL }));

    await waitFor(() => expect(mockSetDeviceLabel).toHaveBeenCalledWith("  Studio Mac  "));
    // Seeded from what Rust stored, not from what was typed: Rust trims.
    await waitFor(() =>
      expect(screen.getByLabelText(SYNC_DEVICE_NAME_LABEL)).toHaveValue("Studio Mac"),
    );
    expect(screen.getByText(SYNC_DEVICE_SAVED_SENTENCE)).toBeInTheDocument();
    expect(screen.getByText(`${SYNC_DEVICE_ID_LABEL}: 01JDEVICE`)).toBeInTheDocument();
  });

  it("reports a refused rename instead of showing a name nothing will use", async () => {
    mockSetDeviceLabel.mockRejectedValue({
      code: "internal",
      message: "device label must not be empty",
    });
    render(<DeviceSection open />);

    fireEvent.change(await screen.findByLabelText(SYNC_DEVICE_NAME_LABEL), {
      target: { value: "renamed" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_DEVICE_SAVE_LABEL }));

    expect(await screen.findByText("device label must not be empty")).toBeInTheDocument();
    expect(screen.queryByText(SYNC_DEVICE_SAVED_SENTENCE)).not.toBeInTheDocument();
  });
});
