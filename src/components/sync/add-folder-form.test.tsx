import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The save plus the mirror re-read `saveSyncProfile` does after it.
  syncProfileSave: vi.fn(),
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  // The three keychain calls, written and read straight through: nothing the
  // mirror store holds changes when one of them runs.
  syncSetCredential: vi.fn(),
  syncGetCredential: vi.fn(),
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
  SYNC_POLL_LABEL,
  SYNC_RECORDINGS_LABEL,
  SYNC_RECORDINGS_SUBFOLDER_NOTE,
  SYNC_RELEASE_NEVER_NOTE,
  SYNC_RELEASE_TTL_LABEL,
  SYNC_REMOTE_URL_LABEL,
  SYNC_REMOVABLE_LABEL,
  SYNC_SETTLE_LABEL,
  SYNC_SUBJECT_LABEL,
  SYNC_SUBPATHS_LABEL,
  SYNC_TAGS_LABEL,
  SYNC_TOKEN_EDIT_NOTE,
  SYNC_TOKEN_FAILED_PREFIX,
  SYNC_TOKEN_HIDE_LABEL,
  SYNC_TOKEN_LABEL,
  SYNC_TOKEN_NONE_STORED_NOTE,
  SYNC_TOKEN_NOTE,
  SYNC_TOKEN_READ_FAILED_PREFIX,
  SYNC_TOKEN_SHOW_LABEL,
  SYNC_TOKEN_UNREADABLE_NOTE,
  SYNC_VIRTUAL_OVER_ALONE_NOTE,
  SYNC_VIRTUAL_OVER_LABEL,
  SYNC_VIRTUAL_OVER_MATCHED_NOTE,
  SYNC_VIRTUAL_OVER_NONE_NOTE,
  SYNC_VIRTUAL_PATTERNS_LABEL,
  syncFolderOwnedNote,
  syncInForceNote,
  syncReleaseInForceNote,
} from "@/components/sync/add-folder-form";
import type { SyncProfileVm } from "@/lib/ipc/client";
import {
  syncActivity,
  syncClearCredential,
  syncGetCredential,
  syncPending,
  syncProblems,
  syncProfileSave,
  syncProfiles,
  syncSetCredential,
  syncStatuses,
} from "@/lib/ipc/client";
import { resetSyncStoreForTest, SYNC_RECORDINGS_SUBFOLDER_LABEL } from "@/lib/stores/sync";
import { resetSyncDetailStoreForTest, syncDetailStore } from "@/lib/stores/sync-detail";

const mockSave = vi.mocked(syncProfileSave);
const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockActivity = vi.mocked(syncActivity);
const mockPending = vi.mocked(syncPending);
const mockProblems = vi.mocked(syncProblems);
const mockSetCredential = vi.mocked(syncSetCredential);
const mockClearCredential = vi.mocked(syncClearCredential);
const mockGetCredential = vi.mocked(syncGetCredential);
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
    // Rust resolves this even for a folder that holds no recordings: it is the
    // subfolder flagging it would use, and it is why the form keeps no copy of
    // keeper's default (Story 41.7).
    recordingsSubfolder: "recordings",
    sessions: false,
    sessionsSubfolder: "60-sessions",
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
  mockProblems.mockResolvedValue({
    warning: null,
    error: null,
    parked: [],
    conflicts: [],
    unspellable: [],
  });
  // The default keychain answer: a folder with nothing stored. Every edit form
  // reads this as it opens (Story 34.12), so every test that renders one needs
  // an answer here or the read resolves to nothing at all.
  mockGetCredential.mockResolvedValue(null);
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
    const carried = {
      tsMs: 1,
      kind: "added",
      path: "notes/today.md",
      sizeBytes: 128,
      delivery: "success",
      failure: null,
      unitId: null,
    };
    mockActivity.mockResolvedValue([carried]);
    render(<AddFolderForm />);
    await fillRequired();

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // The detail mirror is a second mirror on a deliberately slower poll; a
    // card that just appeared would otherwise sit blank for a poll interval.
    await waitFor(() => expect(mockActivity).toHaveBeenCalledWith("p2", expect.any(Number)));
    await waitFor(() => expect(syncDetailStore.getState().detail.p2?.activity).toEqual([carried]));
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

  it("reports a settled add once the keychain has taken the token", async () => {
    mockSave.mockResolvedValue(profileVm());
    mockSetCredential.mockResolvedValue(undefined);
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() => expect(mockSetCredential).toHaveBeenCalledWith("p2", "ghp_secret"));
    // Both writes went through, and the edit form is where the token is changed
    // or removed from now on — so this form has nothing left to hold open
    // (Story 34.12).
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(profileVm(), true));
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("");
    // The add form has no stored token to describe, so it keeps the note that
    // says where the one being typed will go.
    expect(screen.getByText(SYNC_TOKEN_NOTE)).toBeInTheDocument();
  });

  it("keeps the typed token in the field when the keychain write fails on an add", async () => {
    // The whole cost of getting this wrong is paid by the user: a token that is
    // gone from the box has to be fetched from the forge again, because a PAT
    // is shown once. The reset used to run before the keychain write, so every
    // failed add destroyed it — finding 2 of the epic-34 review of Story 34.12.
    mockSave.mockResolvedValue(profileVm());
    mockSetCredential.mockRejectedValue({ code: "internal", message: "keychain refused" });
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    expect(
      await screen.findByText(`${SYNC_TOKEN_FAILED_PREFIX}keychain refused`),
    ).toBeInTheDocument();
    // Unsettled, so every surface keeps the form mounted — and the form still
    // holds the value the failure was about, which is what makes that mounting
    // worth anything.
    expect(onSaved).toHaveBeenCalledWith(profileVm(), false);
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("ghp_secret");
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("notes");
  });

  it("finishes the folder it already created when a failed add is retried", async () => {
    // Keeping the draft is only half the fix: the folder exists now, so a
    // second Save that still sent `id: null` would add it twice — two profiles,
    // two watchers, one directory. The retry has to be an update.
    mockSave.mockResolvedValue(profileVm());
    mockSetCredential.mockRejectedValue({ code: "internal", message: "keychain refused" });
    const onSaved = vi.fn();
    render(<AddFolderForm onSaved={onSaved} />);
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));
    await screen.findByText(`${SYNC_TOKEN_FAILED_PREFIX}keychain refused`);
    expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ id: null }));

    mockSave.mockClear();
    mockSetCredential.mockReset();
    mockSetCredential.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() => expect(mockSetCredential).toHaveBeenCalledWith("p2", "ghp_secret"));
    expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ id: "p2" }));
    // Both stores hold it now, so the draft is finally spent and the next
    // folder starts from an empty form.
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith(profileVm(), true));
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("");
    expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue("");
  });

  it("shows exactly what was typed when the eye is pressed, and hides it again", () => {
    render(<AddFolderForm />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    fireEvent.change(token, { target: { value: "ghp_secret" } });

    // Masked until asked, and the button is named for the press rather than
    // for the state, because that is what is announced before it happens.
    expect(token).toHaveAttribute("type", "password");
    const eye = screen.getByRole("button", { name: SYNC_TOKEN_SHOW_LABEL });
    expect(eye).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(eye);

    expect(token).toHaveAttribute("type", "text");
    expect(token).toHaveValue("ghp_secret");
    const pressed = screen.getByRole("button", { name: SYNC_TOKEN_HIDE_LABEL });
    expect(pressed).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(pressed);

    expect(token).toHaveAttribute("type", "password");
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
      effectiveSettleMs: 12_000,
      pollIntervalMs: 45_000,
      effectivePollIntervalMs: 45_000,
      tags: ["field"],
      commitSubjectTemplate: "",
      notes: false,
      notesSubfolder: null,
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
        virtualPatterns: [],
        virtualOverBytes: 0,
        releaseTtlMs: 24 * 60 * 60 * 1000,
        settleMs: 12_000,
        pollIntervalMs: 45_000,
        tags: ["field"],
        authorOverride: "Ada <ada@example.org>",
        commitSubjectTemplate: "",
        notes: false,
        notesSubfolder: null,
        recordings: false,
        recordingsSubfolder: null,
        sessions: false,
        sessionsSubfolder: null,
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

  it("opens with the stored token in the field, as dots until the eye is pressed", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    render(<AddFolderForm profile={stored()} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    // Story 34.12 overrides AD-34-7: opening the form is the ask.
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p9"));
    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    await waitFor(() => expect(token).toHaveValue("ghp_stored"));
    expect(token).toHaveAttribute("type", "password");
    expect(screen.getByText(SYNC_TOKEN_EDIT_NOTE)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SYNC_TOKEN_SHOW_LABEL }));

    // The eye is the only way to see it, and it shows the stored value exactly.
    expect(token).toHaveAttribute("type", "text");
    expect(token).toHaveValue("ghp_stored");
  });

  it("starts masked again on the next open, so a reveal cannot outlive one", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    const profile = stored();
    const first = render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("ghp_stored"));
    fireEvent.click(screen.getByRole("button", { name: SYNC_TOKEN_SHOW_LABEL }));
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveAttribute("type", "text");

    // Every surface unmounts the form when it closes, which is what makes the
    // reveal mount-scoped rather than something that has to be reset.
    first.unmount();
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    const reopened = screen.getByLabelText(SYNC_TOKEN_LABEL);
    await waitFor(() => expect(reopened).toHaveValue("ghp_stored"));
    expect(reopened).toHaveAttribute("type", "password");
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

  it("writes nothing to the keychain when the field still holds what was read", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    const onSaved = vi.fn();
    render(<AddFolderForm profile={profile} onSaved={onSaved} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("ghp_stored"));

    fireEvent.change(screen.getByLabelText(SYNC_BRANCH_LABEL), { target: { value: "trunk-2" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // `onSaved` is the last thing the save does, so waiting on it is what makes
    // the two assertions below mean "never" rather than "not yet".
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    // Re-storing a byte-identical secret is a keychain write, and on some
    // platforms a prompt, for no change at all.
    expect(mockSetCredential).not.toHaveBeenCalled();
    expect(mockClearCredential).not.toHaveBeenCalled();
  });

  it("removes the stored token when the field it arrived in is emptied", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    mockClearCredential.mockResolvedValue(undefined);
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    await waitFor(() => expect(token).toHaveValue("ghp_stored"));

    fireEvent.change(token, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // The field arrived carrying the token, so emptying it is the removal —
    // there is no separate button left to mean it.
    await waitFor(() => expect(mockClearCredential).toHaveBeenCalledWith("p9"));
    expect(mockSetCredential).not.toHaveBeenCalled();
  });

  it("replaces the stored token when a different one is typed over it", async () => {
    mockGetCredential.mockResolvedValue("ghp_stored");
    mockSetCredential.mockResolvedValue(undefined);
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    const onSaved = vi.fn();
    render(<AddFolderForm profile={profile} onSaved={onSaved} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    const token = screen.getByLabelText(SYNC_TOKEN_LABEL);
    await waitFor(() => expect(token).toHaveValue("ghp_stored"));

    fireEvent.change(token, { target: { value: "ghp_rotated" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(mockSetCredential).toHaveBeenCalledWith("p9", "ghp_rotated"));
    expect(mockClearCredential).not.toHaveBeenCalled();

    // The baseline moved with the write, so a second Save rewrites nothing —
    // without that, every save of an open form would re-enter the keychain.
    mockSetCredential.mockClear();
    onSaved.mockClear();
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(mockSetCredential).not.toHaveBeenCalled();
  });

  it("says no token is stored rather than letting an empty field imply one", async () => {
    mockGetCredential.mockResolvedValue(null);
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    const onSaved = vi.fn();
    render(<AddFolderForm profile={profile} onSaved={onSaved} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    expect(await screen.findByText(SYNC_TOKEN_NONE_STORED_NOTE)).toBeInTheDocument();
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("");
    // The note that describes a stored token would be a claim there is one.
    expect(screen.queryByText(SYNC_TOKEN_EDIT_NOTE)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    // Nothing is stored, so the empty field has nothing to remove.
    expect(mockClearCredential).not.toHaveBeenCalled();
    expect(mockSetCredential).not.toHaveBeenCalled();
  });

  it("will not read an empty field as a removal when the read failed", async () => {
    mockGetCredential.mockRejectedValue({ code: "internal", message: "the keychain is locked" });
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    const onSaved = vi.fn();
    render(<AddFolderForm profile={profile} onSaved={onSaved} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));

    expect(
      await screen.findByText(`${SYNC_TOKEN_READ_FAILED_PREFIX}the keychain is locked`),
    ).toBeInTheDocument();
    // The field is empty and emptying it is how a token is removed, so the form
    // has to say out loud that this particular empty field is not that.
    expect(screen.getByText(SYNC_TOKEN_UNREADABLE_NOTE)).toBeInTheDocument();
    expect(screen.getByLabelText(SYNC_TOKEN_LABEL)).toHaveValue("");

    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    // A locked keychain must not be able to destroy a working credential by
    // looking exactly like a user who cleared the field.
    expect(mockClearCredential).not.toHaveBeenCalled();
    expect(mockSetCredential).not.toHaveBeenCalled();
  });

  it("still stores a token typed after a failed read, which is unambiguous", async () => {
    mockGetCredential.mockRejectedValue({ code: "internal", message: "the keychain is locked" });
    mockSetCredential.mockResolvedValue(undefined);
    const profile = stored();
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await screen.findByText(SYNC_TOKEN_UNREADABLE_NOTE);

    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_typed" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // Refusing this too would make an unreadable keychain unwritable as well.
    await waitFor(() => expect(mockSetCredential).toHaveBeenCalledWith("p9", "ghp_typed"));
  });
});

describe("AddFolderForm numeric knobs (Story 34.5, AD-34-8)", () => {
  /** Fill the required fields and open the Advanced disclosure. */
  async function openAdvanced() {
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
  }

  it("shows the wait keeper will use as the placeholder, and follows the removable box", async () => {
    // The measured bug: a removable folder rendered 5 while 10 s was in force.
    render(<AddFolderForm />);
    await openAdvanced();

    const settle = screen.getByLabelText(SYNC_SETTLE_LABEL);
    expect(settle).toHaveValue(null);
    expect(settle).toHaveAttribute("placeholder", "5");

    fireEvent.click(screen.getByLabelText(SYNC_REMOVABLE_LABEL));
    expect(screen.getByLabelText(SYNC_SETTLE_LABEL)).toHaveAttribute("placeholder", "10");
  });

  it("says which number is in force when the typed one is not it", async () => {
    render(<AddFolderForm />);
    await openAdvanced();

    // A cadence under the floor: keeper cannot walk the tree every second.
    fireEvent.change(screen.getByLabelText(SYNC_POLL_LABEL), { target: { value: "1" } });
    expect(screen.getByText(syncInForceNote(2))).toBeInTheDocument();

    // Honoured verbatim, so nothing is claimed.
    fireEvent.change(screen.getByLabelText(SYNC_POLL_LABEL), { target: { value: "30" } });
    expect(screen.queryByText(syncInForceNote(2))).not.toBeInTheDocument();

    // A wait of exactly keeper's own default on removable storage IS "keeper
    // picks", so Rust answers with the longer window and the form has to admit it.
    fireEvent.click(screen.getByLabelText(SYNC_REMOVABLE_LABEL));
    fireEvent.change(screen.getByLabelText(SYNC_SETTLE_LABEL), { target: { value: "5" } });
    expect(screen.getByText(syncInForceNote(10))).toBeInTheDocument();
  });

  it("sends keeper's own numbers for an empty box, never the omission Rust reads as leave-alone", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          settleMs: 5_000,
          pollIntervalMs: 15_000,
          lfsThresholdBytes: 4 * 1024 * 1024,
          commitSubjectTemplate: "",
          notes: false,
          notesSubfolder: null,
        }),
      ),
    );
  });

  it("carries the scan cadence and the commit subject as typed", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_POLL_LABEL), { target: { value: "45" } });
    fireEvent.change(screen.getByLabelText(SYNC_SUBJECT_LABEL), {
      target: { value: "  backup {profile}: {changed} files  " },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          pollIntervalMs: 45_000,
          commitSubjectTemplate: "backup {profile}: {changed} files",
          notes: false,
          notesSubfolder: null,
        }),
      ),
    );
  });

  it("seeds an edit form from what the profile pins, leaving unpinned knobs blank", async () => {
    // `null` means the profile pins nothing. Rendering 5 there would make the
    // next save store 5 s as a deliberate choice and take the removable
    // substitution away for good.
    render(
      <AddFolderForm
        profile={profileVm({
          removable: true,
          settleMs: null,
          effectiveSettleMs: 10_000,
          pollIntervalMs: 45_000,
          effectivePollIntervalMs: 45_000,
          commitSubjectTemplate: "backup {profile}",
          notes: false,
          notesSubfolder: null,
        })}
      />,
    );
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    // The edit form reads the stored token as it opens (Story 34.12), so settle
    // that before asserting — otherwise the read lands outside `act`.
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    const settle = screen.getByLabelText(SYNC_SETTLE_LABEL);
    expect(settle).toHaveValue(null);
    // And the placeholder agrees with what Rust computed for this profile.
    expect(settle).toHaveAttribute("placeholder", "10");
    expect(screen.getByLabelText(SYNC_POLL_LABEL)).toHaveValue(45);
    expect(screen.getByLabelText(SYNC_SUBJECT_LABEL)).toHaveValue("backup {profile}");
  });
});

describe("AddFolderForm fractional numbers (Story 52.9, FR-313)", () => {
  /** Fill the required fields and open the Advanced disclosure. */
  async function openAdvanced() {
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
  }

  it("lets the three numeric boxes hold a fraction the browser will accept", async () => {
    // The measured bug: no `step`, so HTML's implicit step is 1 and 1.5 is a
    // stepMismatch — which in a real form with a native submit and no
    // `noValidate` makes WKWebView refuse the save with no message at all.
    // Typing and submitting proves nothing here: jsdom runs no INTERACTIVE
    // validation, so a fireEvent submit succeeds against the bug too. The
    // attribute and the ValidityState jsdom does compute are the honest ones.
    render(<AddFolderForm />);
    await openAdvanced();

    for (const label of [SYNC_LFS_THRESHOLD_LABEL, SYNC_SETTLE_LABEL, SYNC_POLL_LABEL]) {
      const box = screen.getByLabelText(label) as HTMLInputElement;
      expect(box).toHaveAttribute("step", "any");
      expect(box).toHaveAttribute("inputmode", "decimal");
      fireEvent.change(box, { target: { value: "1.5" } });
      expect(box.validity.stepMismatch).toBe(false);
      expect(box.checkValidity()).toBe(true);
    }
  });

  it("saves 1.5 MB as exactly 1572864 bytes, rounded once", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_LFS_THRESHOLD_LABEL), {
      target: { value: "1.5" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // The exact byte count, not a whole MB: the one rounding on the way out
    // exists to keep Rust's `u64` integral, never to quantise the ask.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ lfsThresholdBytes: 1_572_864 }),
      ),
    );
  });

  it("takes a fractional wait and cadence as whole milliseconds", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_SETTLE_LABEL), { target: { value: "7.5" } });
    fireEvent.change(screen.getByLabelText(SYNC_POLL_LABEL), { target: { value: "12.5" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ settleMs: 7_500, pollIntervalMs: 12_500 }),
      ),
    );
  });

  it("opens a sub-MB profile at 0.25 and saves an unrelated change from it", async () => {
    // The worse face of the same bug, and the reason this is not a nicety: the
    // docs' own 256 KiB example (`docs/sync.md`) renders as 0.25, so before the
    // `step` the form refused EVERY save on such a profile — including one that
    // only came to fix the remote URL.
    const profile = profileVm({ lfsThresholdBytes: 262_144 });
    mockSave.mockResolvedValue(profile);
    render(<AddFolderForm profile={profile} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    const threshold = screen.getByLabelText(SYNC_LFS_THRESHOLD_LABEL) as HTMLInputElement;
    expect(threshold).toHaveValue(0.25);
    expect(threshold.checkValidity()).toBe(true);

    fireEvent.change(screen.getByLabelText(SYNC_REMOTE_URL_LABEL), {
      target: { value: "git@github.com:alice/notes-2.git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    // The threshold rides along untouched: a fraction survives the round trip,
    // so an edit form cannot silently round the user's stored setting up to 1 MB.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          id: "p2",
          remoteUrl: "git@github.com:alice/notes-2.git",
          lfsThresholdBytes: 262_144,
        }),
      ),
    );
  });
});

describe("AddFolderForm recordings switch (Story 41.7, AD-66)", () => {
  /** A folder that already holds recordings, as Rust reports it. */
  function flagged(): SyncProfileVm {
    return profileVm({
      id: "p9",
      name: "tgdrive",
      localPath: "/Volumes/merope/tgdrive",
      recordings: true,
      recordingsSubfolder: "sessions/raw",
    });
  }

  it("flags a new folder and leaves the subfolder to keeper", async () => {
    // The reported bug, at its narrowest: nothing in the app could write a
    // `recordings` block, so the Recording pane's destination picker — built in
    // Story 41.2, reading a flag Story 41.1 shipped — had nothing to offer.
    mockSave.mockResolvedValue(profileVm({ recordings: true }));
    render(<AddFolderForm />);
    await fillRequired();

    fireEvent.click(screen.getByLabelText(SYNC_RECORDINGS_LABEL));
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          recordings: true,
          // Not an empty string and not a guess at keeper's default: the field
          // is omitted, which is how Rust is told to use its own.
          recordingsSubfolder: null,
        }),
      ),
    );
  });

  it("sends a subfolder the owner chose exactly as typed", async () => {
    mockSave.mockResolvedValue(profileVm({ recordings: true }));
    render(<AddFolderForm />);
    await fillRequired();
    fireEvent.click(screen.getByLabelText(SYNC_RECORDINGS_LABEL));

    fireEvent.change(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL), {
      target: { value: "  media/screen-recordings  " },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          recordings: true,
          recordingsSubfolder: "media/screen-recordings",
        }),
      ),
    );
  });

  it("prefills the subfolder from what Rust resolved, never from a copy of the default", async () => {
    // The whole reason `SyncProfileVm.recordingsSubfolder` is never null: a
    // folder that holds no recordings still reports the subfolder flagging it
    // would use, so the form can show it without spelling `recordings` itself.
    render(<AddFolderForm profile={profileVm({ recordings: false })} />);
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    // Hidden until the switch is on, exactly as the vault subfolder is.
    expect(screen.queryByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).not.toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(SYNC_RECORDINGS_LABEL));
    expect(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).toHaveValue("recordings");

    // And the resolved root is stated as a fact about this folder.
    expect(screen.getByText("/Users/alice/notes/recordings")).toBeInTheDocument();
  });

  it("opens an already-flagged folder with its switch on and its own subfolder", async () => {
    render(<AddFolderForm profile={flagged()} />);
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p9"));

    expect(screen.getByLabelText(SYNC_RECORDINGS_LABEL)).toBeChecked();
    expect(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).toHaveValue("sessions/raw");
    expect(screen.getByText("/Volumes/merope/tgdrive/sessions/raw")).toBeInTheDocument();
    // Keeper's default is not asserted anywhere on a folder that has an answer.
    expect(screen.queryByText(SYNC_RECORDINGS_SUBFOLDER_NOTE)).not.toBeInTheDocument();
  });

  it("unflagging asks for the block to be removed, not emptied", async () => {
    const profile = flagged();
    mockSave.mockResolvedValue(profileVm({ id: "p9", recordings: false }));
    render(<AddFolderForm profile={profile} />);
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p9"));

    fireEvent.click(screen.getByLabelText(SYNC_RECORDINGS_LABEL));
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          // `false` is what makes Rust drop the block; a subfolder alongside it
          // would read as "still holds recordings, over here instead".
          recordings: false,
          recordingsSubfolder: null,
        }),
      ),
    );
    // The subfolder field goes with the switch, so nothing on screen still
    // claims this folder has a recordings root.
    expect(screen.queryByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).not.toBeInTheDocument();
  });

  it("sends an emptied subfolder on an edit so the shared validator can refuse it", async () => {
    // On an add form an empty box means "keeper picks". On an edit form it
    // arrived holding the value in force, so emptying it is deliberate — and the
    // refusal is the answer, not something to route around.
    mockSave.mockRejectedValue({
      code: "internal",
      message:
        "invalid sync configuration: recordings subfolder must not be empty: recordings live in a folder inside the profile, never at the profile root",
    });
    render(<AddFolderForm profile={flagged()} />);
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p9"));

    fireEvent.change(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ recordings: true, recordingsSubfolder: "" }),
      ),
    );
  });

  it("shows each refusal in the validator's own words and corrects nothing", async () => {
    // These sentences are `RecordingsConfig::validate`'s, verbatim. The form
    // must not re-implement the rules — a second copy would drift from the one
    // the engine and `keeper-syncd` enforce — and must not quietly pick a
    // different subfolder to make the save succeed, which is the failure that
    // would put someone's recordings in a folder they never named.
    for (const [typed, refusal] of [
      [
        "/tmp",
        "invalid sync configuration: recordings subfolder must be relative to the profile folder, got /tmp",
      ],
      [
        "../x",
        "invalid sync configuration: recordings subfolder must not escape the profile folder: ../x",
      ],
      [
        "10-notes/rec",
        "invalid sync configuration: recordings subfolder 10-notes/rec overlaps notes subfolder 10-notes: one folder cannot be both a vault and a recordings root",
      ],
    ] as const) {
      mockSave.mockRejectedValue({ code: "internal", message: refusal });
      const view = render(<AddFolderForm profile={flagged()} />);
      await waitFor(() => expect(mockGetCredential).toHaveBeenCalled());

      fireEvent.change(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL), {
        target: { value: typed },
      });
      fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));

      expect(await screen.findByText(refusal)).toBeInTheDocument();
      // What was typed is still what was sent and still what is on screen: the
      // refusal named a rule, and the field it is about is the one to fix.
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ recordings: true, recordingsSubfolder: typed }),
      );
      expect(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).toHaveValue(typed);
      view.unmount();
      // Only the call log: each iteration asserts about its own save.
      mockSave.mockClear();
    }
  });

  it("says keeper picks the subfolder only while there is no stored answer", async () => {
    render(<AddFolderForm />);
    await fillRequired();
    fireEvent.click(screen.getByLabelText(SYNC_RECORDINGS_LABEL));

    // An add form has no profile to have resolved the default, so the box starts
    // empty and the promise under it is what keeper will do with that.
    expect(screen.getByLabelText(SYNC_RECORDINGS_SUBFOLDER_LABEL)).toHaveValue("");
    expect(screen.getByText(SYNC_RECORDINGS_SUBFOLDER_NOTE)).toBeInTheDocument();
  });
});

describe("AddFolderForm virtual-file controls (Story 56.12)", () => {
  /** Fill the required fields and open the Advanced disclosure. */
  async function openAdvanced() {
    await fillRequired();
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
  }

  it("sends keeper's own answers when nothing is typed", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          // An untouched patterns box is an EXPRESSED empty list, not the
          // omission: `VirtualPolicy::compile` reads `[]` as silence, which
          // leaves the committed `.keepervirtual` deciding.
          virtualPatterns: [],
          virtualOverBytes: 0,
          releaseTtlMs: 24 * 60 * 60 * 1000,
        }),
      ),
    );
  });

  it("carries the patterns, the floor and the window as typed", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_VIRTUAL_PATTERNS_LABEL), {
      target: { value: " scans/** , *.psd ,, " },
    });
    fireEvent.change(screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL), { target: { value: "8" } });
    fireEvent.change(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL), { target: { value: "72" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          // Trimmed, and the trailing empty entry dropped — an empty pattern
          // reaching the engine would match everything.
          virtualPatterns: ["scans/**", "*.psd"],
          virtualOverBytes: 8 * 1024 * 1024,
          releaseTtlMs: 72 * 60 * 60 * 1000,
        }),
      ),
    );
  });

  it("reaches never-release with a zero, and says so in words", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL), { target: { value: "0" } });

    // The whole reason this box is not parsed by `pinnedValue`, which would
    // collapse the zero and make "never" unreachable from the form.
    expect(screen.getByText(SYNC_RELEASE_NEVER_NOTE)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ releaseTtlMs: 0 })),
    );
  });

  it("names the window in force when the box holds nothing usable", async () => {
    render(<AddFolderForm />);
    await openAdvanced();
    fireEvent.change(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL), { target: { value: "" } });

    expect(screen.getByText(syncReleaseInForceNote(24))).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL), { target: { value: "6" } });
    expect(screen.queryByText(syncReleaseInForceNote(24))).not.toBeInTheDocument();
  });

  /**
   * Story 56.14: the size-floor box says when its content means "no floor".
   *
   * Fails before `SYNC_VIRTUAL_OVER_NONE_NOTE` was rendered. `0` is not a neutral
   * fallback for this field — it is the documented instruction that nothing stays
   * away for being large — and anything `pinnedValue` cannot read as a positive
   * number silently became it with nothing on screen, while the release box one
   * line down explained both of its own coercions.
   *
   * Three inputs, because the failure is a CLASS and not the empty box: a blank
   * field, a typed zero, and a half-typed number that parses to nothing. Each
   * must show the note AND send `0`, so the sentence and the wire cannot drift.
   */
  it("says when the size floor is off, for every input that means off", async () => {
    mockSave.mockResolvedValue(profileVm({ id: "p9", name: "notes" }));
    render(<AddFolderForm />);
    await openAdvanced();
    const floor = screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL);

    // The seeded value is keeper's own `0`, so the note is there from the start:
    // a fresh form DOES mean no floor, and the previous silence made the widest
    // setting in the field the one nothing was said about.
    expect(screen.getByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).toBeInTheDocument();

    for (const value of ["", "0", "1e"]) {
      fireEvent.change(floor, { target: { value } });
      expect(screen.getByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).toBeInTheDocument();
    }

    // A real floor takes it away again — otherwise the note would be furniture.
    fireEvent.change(floor, { target: { value: "8" } });
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).not.toBeInTheDocument();
    // ...and it is replaced by the sentence for the state the form is now in:
    // with the patterns box empty, a real floor is the SELECTOR (story 56.16).
    // Asserting only the none-note's absence would pass over a form that said
    // nothing at all about a floor that decides everything, which is precisely
    // the silence the owner was left in.
    expect(screen.getByText(SYNC_VIRTUAL_OVER_ALONE_NOTE)).toBeInTheDocument();

    // And the note tells the truth about what is sent: the same `=== null` that
    // renders it is what sends `0`.
    fireEvent.change(floor, { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(expect.objectContaining({ virtualOverBytes: 0 })),
    );
  });

  /**
   * Story 56.16: the floor's note is true in each of the three states, and only
   * one of them is on screen at a time.
   *
   * The owner saved
   * `{"name":"tgdrive-light","virtualPatterns":[],"virtualOverBytes":1048576}` —
   * a 1 MiB floor and no patterns, which can only mean "don't fetch the big
   * files" — and the form answered with "A matched file smaller than this is
   * downloaded anyway", a sentence about a match that could never happen. It sat
   * beside `SYNC_VIRTUAL_OVER_NONE_NOTE`, so a blank box showed BOTH: two
   * sentences, one of them false, under one control.
   *
   * Each state therefore asserts the absence of the other two and not merely the
   * presence of its own. A note pair that can render two contradictory sentences
   * is the defect's own shape, and a test that only looked for the right one
   * would not have caught it before either.
   */
  it("the floor's note is true in each of the three states", async () => {
    render(<AddFolderForm />);
    await openAdvanced();
    const floor = screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL);
    const patterns = screen.getByLabelText(SYNC_VIRTUAL_PATTERNS_LABEL);

    // (1) Nothing named, no floor: nothing stays away at all.
    fireEvent.change(patterns, { target: { value: "" } });
    fireEvent.change(floor, { target: { value: "" } });
    expect(screen.getByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_ALONE_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_MATCHED_NOTE)).not.toBeInTheDocument();

    // (2) The owner's state: nothing named, a real floor — the floor decides.
    fireEvent.change(floor, { target: { value: "1" } });
    expect(screen.getByText(SYNC_VIRTUAL_OVER_ALONE_NOTE)).toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_MATCHED_NOTE)).not.toBeInTheDocument();

    // (3) Patterns named and a floor: the field's original job, and the only
    // state the old unconditional sentence was ever true in.
    fireEvent.change(patterns, { target: { value: "scans/**" } });
    expect(screen.getByText(SYNC_VIRTUAL_OVER_MATCHED_NOTE)).toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_NONE_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByText(SYNC_VIRTUAL_OVER_ALONE_NOTE)).not.toBeInTheDocument();
  });

  it("seeds an edit form from the policy in force and saves it back unchanged", async () => {
    const stored = profileVm({
      virtualPatterns: ["scans/**", "*.psd"],
      virtualOverBytes: 16 * 1024 * 1024,
      releaseTtlMs: 0,
    });
    mockSave.mockResolvedValue(stored);
    render(<AddFolderForm profile={stored} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    expect(screen.getByLabelText(SYNC_VIRTUAL_PATTERNS_LABEL)).toHaveValue("scans/**, *.psd");
    expect(screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL)).toHaveValue(16);
    expect(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL)).toHaveValue(0);

    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({
          virtualPatterns: ["scans/**", "*.psd"],
          virtualOverBytes: 16 * 1024 * 1024,
          releaseTtlMs: 0,
        }),
      ),
    );
  });

  it("cannot edit a key the folder's own config file owns, and says why", async () => {
    const stored = profileVm({
      virtualPatterns: ["raw/**"],
      virtualOverBytes: 32 * 1024 * 1024,
      releaseTtlMs: 7 * 24 * 60 * 60 * 1000,
      folderOwned: ["releaseTtlMs", "virtualOverBytes", "virtualPatterns"],
    });
    mockSave.mockResolvedValue(stored);
    render(<AddFolderForm profile={stored} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    const patterns = screen.getByLabelText(SYNC_VIRTUAL_PATTERNS_LABEL);
    expect(patterns).toBeDisabled();
    expect(screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL)).toBeDisabled();
    expect(screen.getByLabelText(SYNC_RELEASE_TTL_LABEL)).toBeDisabled();
    // The reason, on screen rather than only in a log line nobody reads.
    expect(screen.getByText(syncFolderOwnedNote("virtualPatterns"))).toBeInTheDocument();

    // And it cannot be smuggled in by driving the input anyway: `parse_req`
    // never sees the key, so `as_stored` has nothing to strip and nothing to
    // warn about.
    fireEvent.change(patterns, { target: { value: "everything/**" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSave).toHaveBeenCalledWith(
      expect.objectContaining({
        virtualPatterns: null,
        virtualOverBytes: null,
        releaseTtlMs: null,
      }),
    );
  });

  it("leaves a key no folder file owns editable and expressed", async () => {
    const stored = profileVm({ virtualPatterns: ["raw/**"], folderOwned: ["virtualOverBytes"] });
    mockSave.mockResolvedValue(stored);
    render(<AddFolderForm profile={stored} />);
    fireEvent.click(screen.getByTestId(SYNC_ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(mockGetCredential).toHaveBeenCalledWith("p2"));

    expect(screen.getByLabelText(SYNC_VIRTUAL_OVER_LABEL)).toBeDisabled();
    const patterns = screen.getByLabelText(SYNC_VIRTUAL_PATTERNS_LABEL);
    expect(patterns).not.toBeDisabled();
    expect(screen.queryByText(syncFolderOwnedNote("virtualPatterns"))).not.toBeInTheDocument();

    fireEvent.change(patterns, { target: { value: "media/**" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_EDIT_SUBMIT_LABEL }));
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(
        expect.objectContaining({ virtualPatterns: ["media/**"], virtualOverBytes: null }),
      ),
    );
  });
});
