import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The save plus the mirror re-read `saveSyncProfile` does after it.
  syncProfileSave: vi.fn(),
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  // The keychain pair, written straight through: there is no read side,
  // because no command reports what a keychain holds.
  syncSetCredential: vi.fn(),
  syncClearCredential: vi.fn(),
  // The Sync view's three per-folder lists, re-read for the folder just added.
  syncActivity: vi.fn(),
  syncPending: vi.fn(),
  syncProblems: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

import { open as openFolder } from "@tauri-apps/plugin-dialog";
import {
  AddFolderForm,
  SYNC_ADD_SUBMIT_LABEL,
  SYNC_ADVANCED_TOGGLE_TESTID,
  SYNC_CHOOSE_FOLDER_LABEL,
  SYNC_FORM_PATH_TESTID,
  SYNC_NAME_LABEL,
  SYNC_REMOTE_URL_LABEL,
  SYNC_TOKEN_CLEAR_LABEL,
  SYNC_TOKEN_LABEL,
} from "@/components/sync/add-folder-form";
import type { SyncProfileVm } from "@/lib/ipc/client";
import {
  syncActivity,
  syncPending,
  syncProblems,
  syncProfileSave,
  syncProfiles,
  syncSetCredential,
  syncStatuses,
} from "@/lib/ipc/client";
import { resetSyncStoreForTest } from "@/lib/stores/sync";
import { resetSyncDetailStoreForTest, syncDetailStore } from "@/lib/stores/sync-detail";

const mockSave = vi.mocked(syncProfileSave);
const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockActivity = vi.mocked(syncActivity);
const mockPending = vi.mocked(syncPending);
const mockProblems = vi.mocked(syncProblems);
const mockSetCredential = vi.mocked(syncSetCredential);
const mockPicker = vi.mocked(openFolder);

function profileVm(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
  return {
    id: "p2",
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
    settleMs: 5000,
    tags: [],
    authorOverride: null,
    enabled: true,
    ...over,
  };
}

/** Fill the three fields the submit button waits on. */
async function fillRequired() {
  fireEvent.change(screen.getByLabelText(SYNC_NAME_LABEL), { target: { value: "notes" } });
  fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
    target: { value: "git@github.com:alice/notes.git" },
  });
  fireEvent.click(screen.getByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }));
  await waitFor(() =>
    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Users/alice/notes"),
  );
}

beforeEach(() => {
  resetSyncStoreForTest();
  resetSyncDetailStoreForTest();
  mockProfiles.mockResolvedValue([]);
  mockStatuses.mockResolvedValue([]);
  mockActivity.mockResolvedValue([]);
  mockPending.mockResolvedValue([]);
  mockProblems.mockResolvedValue({ warning: null, error: null, parked: [], conflicts: [] });
  mockPicker.mockResolvedValue("/Users/alice/notes");
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("AddFolderForm", () => {
  it("names itself for a screen reader even where no heading is drawn beside it", () => {
    // Every surface titles the form in its own chrome — a section heading, a
    // card title, or the disclosure button that revealed it — so the accessible
    // name has to live on the form itself or the disclosure has none at all.
    render(<AddFolderForm />);

    expect(screen.getByRole("form", { name: "Add a folder" })).toBeInTheDocument();
  });

  it("reports a settled add to its caller only after clearing the form", async () => {
    mockSave.mockResolvedValue(profileVm());
    const onAdded = vi.fn();
    render(<AddFolderForm onAdded={onAdded} />);
    await fillRequired();

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() => expect(onAdded).toHaveBeenCalledWith(profileVm(), true));
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("");
  });

  it("re-reads the new folder's lists instead of leaving the Sync view to its poll", async () => {
    mockSave.mockResolvedValue(profileVm());
    mockActivity.mockResolvedValue([{ tsMs: 1, kind: "added", path: "notes/today.md" }]);
    render(<AddFolderForm />);
    await fillRequired();

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // The detail mirror is a second mirror on a deliberately slower poll; a
    // card that just appeared would otherwise sit blank for a poll interval.
    await waitFor(() => expect(mockActivity).toHaveBeenCalledWith("p2", expect.any(Number)));
    await waitFor(() =>
      expect(syncDetailStore.getState().detail.p2?.activity).toEqual([
        { tsMs: 1, kind: "added", path: "notes/today.md" },
      ]),
    );
  });

  it("shows a rejected save inline, keeps every typed value, and reports no add", async () => {
    mockSave.mockRejectedValue({
      code: "internal",
      message: "local path must be absolute, got relative/path",
    });
    const onAdded = vi.fn();
    render(<AddFolderForm onAdded={onAdded} />);
    await fillRequired();
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/half-typed" },
    });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    expect(
      await screen.findByText("local path must be absolute, got relative/path"),
    ).toBeInTheDocument();
    // Nothing typed is lost to a validation error, and no surface hides the
    // form out from under the message that says what to fix.
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("notes");
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue(
      "git@github.com:alice/half-typed",
    );
    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Users/alice/notes");
    expect(onAdded).not.toHaveBeenCalled();
  });

  it("tells the caller a stored token leaves the form something only it can show", async () => {
    mockSave.mockResolvedValue(profileVm());
    mockSetCredential.mockResolvedValue(undefined);
    const onAdded = vi.fn();
    render(<AddFolderForm onAdded={onAdded} />);
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // Clear is the only undo there is — nothing can read a stored token back to
    // offer it later — so the caller is told the folder exists and told, in the
    // same call, that hiding the form now would destroy the only way to use it.
    expect(await screen.findByRole("button", { name: SYNC_TOKEN_CLEAR_LABEL })).toBeInTheDocument();
    expect(onAdded).toHaveBeenCalledWith(profileVm(), false);
  });
});
