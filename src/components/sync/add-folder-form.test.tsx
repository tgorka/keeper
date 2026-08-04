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
  SYNC_REMOTE_URL_LABEL,
  SYNC_REMOVABLE_LABEL,
  SYNC_SETTLE_LABEL,
  SYNC_SUBJECT_LABEL,
  SYNC_SUBPATHS_LABEL,
  SYNC_TAGS_LABEL,
  SYNC_TOKEN_EDIT_NOTE,
  SYNC_TOKEN_HIDE_LABEL,
  SYNC_TOKEN_LABEL,
  SYNC_TOKEN_NONE_STORED_NOTE,
  SYNC_TOKEN_NOTE,
  SYNC_TOKEN_READ_FAILED_PREFIX,
  SYNC_TOKEN_SHOW_LABEL,
  SYNC_TOKEN_UNREADABLE_NOTE,
  syncInForceNote,
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
    settleMs: null,
    effectiveSettleMs: 5_000,
    pollIntervalMs: null,
    effectivePollIntervalMs: 15_000,
    tags: [],
    commitSubjectTemplate: "",
    notes: false,
    notesSubfolder: null,
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
        settleMs: 12_000,
        pollIntervalMs: 45_000,
        tags: ["field"],
        authorOverride: "Ada <ada@example.org>",
        commitSubjectTemplate: "",
        notes: false,
        notesSubfolder: null,
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
