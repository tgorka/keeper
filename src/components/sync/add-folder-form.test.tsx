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
  SYNC_AUTHOR_LABEL,
  SYNC_BRANCH_LABEL,
  SYNC_CHOOSE_FOLDER_LABEL,
  SYNC_EDIT_SUBMIT_LABEL,
  SYNC_EDIT_TITLE,
  SYNC_EXCLUDES_LABEL,
  SYNC_FOLDER_LABEL,
  SYNC_FORM_PATH_TESTID,
  SYNC_LFS_THRESHOLD_LABEL,
  SYNC_NAME_LABEL,
  SYNC_PATH_FIXED_NOTE,
  SYNC_REMOTE_URL_LABEL,
  SYNC_SETTLE_LABEL,
  SYNC_SUBPATHS_LABEL,
  SYNC_TAGS_LABEL,
  SYNC_TOKEN_CLEAR_LABEL,
  SYNC_TOKEN_CLEARED_LABEL,
  SYNC_TOKEN_EDIT_NOTE,
  SYNC_TOKEN_LABEL,
} from "@/components/sync/add-folder-form";
import type { SyncProfileVm } from "@/lib/ipc/client";
import {
  syncActivity,
  syncClearCredential,
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
const mockClearCredential = vi.mocked(syncClearCredential);
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
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
    await fillRequired();

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(profileVm(), true));
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
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
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
    expect(onSaved).not.toHaveBeenCalled();
  });

  it("tells the caller a stored token leaves the form something only it can show", async () => {
    mockSave.mockResolvedValue(profileVm());
    mockSetCredential.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // Clear is the only undo there is — nothing can read a stored token back to
    // offer it later — so the caller is told the folder exists and told, in the
    // same call, that hiding the form now would destroy the only way to use it.
    expect(await screen.findByRole("button", { name: SYNC_TOKEN_CLEAR_LABEL })).toBeInTheDocument();
    expect(onSaved).toHaveBeenCalledWith(profileVm(), false);
  });
});

describe("AddFolderForm editing an existing folder", () => {
  /** A stored profile with something in every field the form can carry. */
  function stored(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
    return profileVm({
      id: "p9",
      name: "field notes",
      localPath: "/Volumes/stick/field",
      remoteUrl: "git@github.com:alice/field.git",
      branch: "trunk",
      direction: "pushOnly",
      subpaths: ["today", "archive"],
      excludes: ["*.tmp"],
      removable: true,
      lfsMode: "pointerOnly",
      lfsThresholdBytes: 8 * 1024 * 1024,
      settleMs: 12_000,
      tags: ["field"],
      authorOverride: "Ada <ada@example.org>",
      ...over,
    });
  }

  it("starts from the stored profile and saves it back under its own id", async () => {
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);

    // Named for the folder it belongs to: several of these can be open at once.
    expect(
      screen.getByRole("form", { name: `${SYNC_EDIT_TITLE}: field notes` }),
    ).toBeInTheDocument();
    // Every field arrives filled in. An edit form that opened empty would be an
    // add form pointed at an existing folder, and saving it would erase the
    // settings the user came to change one of.
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("field notes");
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue(
      "git@github.com:alice/field.git",
    );
    expect(screen.getByLabelText(SYNC_BRANCH_LABEL)).toHaveValue("trunk");
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    // Seconds and MB here, milliseconds and bytes on the wire.
    expect(screen.getByLabelText(SYNC_SETTLE_LABEL)).toHaveValue(12);
    expect(screen.getByLabelText(SYNC_LFS_THRESHOLD_LABEL)).toHaveValue(8);
    expect(screen.getByLabelText(SYNC_EXCLUDES_LABEL)).toHaveValue("*.tmp");
    expect(screen.getByLabelText(SYNC_SUBPATHS_LABEL)).toHaveValue("today, archive");
    expect(screen.getByLabelText(SYNC_TAGS_LABEL)).toHaveValue("field");
    expect(screen.getByLabelText(SYNC_AUTHOR_LABEL)).toHaveValue("Ada <ada@example.org>");

    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/field-notes.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // The id is what Rust reads as "update that one", and the request is exact:
    // it carries no `enabled`, so the merge on the other side cannot be handed
    // a pause state to contradict.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith({
        id: "p9",
        name: "field notes",
        localPath: "/Volumes/stick/field",
        remoteUrl: "git@github.com:alice/field-notes.git",
        branch: "trunk",
        direction: "pushOnly",
        lane: "main",
        subpaths: ["today", "archive"],
        excludes: ["*.tmp"],
        removable: true,
        lfsMode: "pointerOnly",
        lfsThresholdBytes: 8 * 1024 * 1024,
        settleMs: 12_000,
        tags: ["field"],
        authorOverride: "Ada <ada@example.org>",
      }),
    );
  });

  it("shows the folder it is bound to without offering to repoint it", async () => {
    render(<AddFolderForm profile={stored()} />);

    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Volumes/stick/field");
    // No picker and no field: the engine binds a profile to this folder, and on
    // removable media to a marker written inside it.
    expect(
      screen.queryByRole("button", { name: SYNC_CHOOSE_FOLDER_LABEL }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: SYNC_FOLDER_LABEL })).not.toBeInTheDocument();
    // Explained rather than quietly dropped — a field that vanished would read
    // as one the form forgot.
    expect(screen.getByText(SYNC_PATH_FIXED_NOTE)).toBeInTheDocument();
  });

  it("starts the token field empty, and an untouched one keeps what is stored", async () => {
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    // Write-only: no command reads a stored token back, so anything in this
    // field — even a placeholder of dots — would be a claim the form cannot
    // make. The note is what says so.
    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    expect(token).toHaveValue("");
    expect(token).not.toHaveAttribute("placeholder");
    expect(screen.getByText(SYNC_TOKEN_EDIT_NOTE)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    // Empty means "leave it alone", so neither keychain write may fire.
    expect(mockSetCredential).not.toHaveBeenCalled();
    expect(mockClearCredential).not.toHaveBeenCalled();
  });

  it("removes a stored token through the keychain, and reports it", async () => {
    mockClearCredential.mockResolvedValue(undefined);
    render(<AddFolderForm profile={stored()} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    fireEvent.click(screen.getByRole("button", { name: SYNC_TOKEN_CLEAR_LABEL }));

    await waitFor(() => expect(mockClearCredential).toHaveBeenCalledWith("p9"));
    // A keychain has no read side, so this line is the only report there is.
    expect(await screen.findByText(SYNC_TOKEN_CLEARED_LABEL)).toBeInTheDocument();
    // Clearing a credential is not a profile change.
    expect(mockSave).not.toHaveBeenCalled();
  });

  it("says an emptied author override out loud, since an omission would keep it", async () => {
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    fireEvent.change(screen.getByLabelText(SYNC_AUTHOR_LABEL), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // Rust reads an absent override as "leave whatever is stored", so clearing
    // the field has to send the empty string or it does nothing at all.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ authorOverride: "" })),
    );
  });

  it("keeps every entered value on screen when the save is rejected", async () => {
    mockSave.mockRejectedValue({ code: "internal", message: "remote is not reachable" });
    const onSaved = vi.fn();
    render(<AddFolderForm profile={stored()} onSaved={onSaved} />);
    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/half-typed" },
    });

    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    expect(await screen.findByText("remote is not reachable")).toBeInTheDocument();
    // A correction that has to be retyped from memory is worse than no edit.
    expect(screen.getByLabelText(SYNC_REMOTE_URL_LABEL)).toHaveValue(
      "git@github.com:alice/half-typed",
    );
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("field notes");
    expect(screen.getByTestId(SYNC_FORM_PATH_TESTID)).toHaveTextContent("/Volumes/stick/field");
    // Nothing was saved, so no surface may close over the message.
    expect(onSaved).not.toHaveBeenCalled();
  });
});
