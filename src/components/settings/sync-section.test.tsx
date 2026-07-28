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
  // The Advanced disclosure's access-token field (Story 32.7) writes straight
  // through to the keychain pair; there is no read side to mock, because no
  // command reports what a keychain holds.
  syncSetCredential: vi.fn(),
  syncClearCredential: vi.fn(),
  // A successful add re-reads the new folder's three detail lists so the Sync
  // view is not blank for a poll interval — the same add path runs from here.
  syncActivity: vi.fn(() => Promise.resolve([])),
  syncPending: vi.fn(() => Promise.resolve([])),
  syncProblems: vi.fn(() =>
    Promise.resolve({ warning: null, error: null, parked: [], conflicts: [] }),
  ),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

import { open as openFolder } from "@tauri-apps/plugin-dialog";
import {
  SYNC_ATTENTION_FALLBACK_SENTENCE,
  SYNC_NO_PROFILES_SENTENCE,
  SYNC_NOW_LABEL,
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
  SYNC_TOKEN_CLEAR_LABEL,
  SYNC_TOKEN_FAILED_PREFIX,
  SYNC_TOKEN_LABEL,
  SYNC_TOKEN_STORED_LABEL,
} from "@/components/sync/add-folder-form";
import type { SyncProfileVm, SyncStatusVm } from "@/lib/ipc/client";
import {
  syncClearCredential,
  syncFolderNow,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncSetCredential,
  syncStatuses,
  syncVerify,
} from "@/lib/ipc/client";
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
const mockClearCredential = vi.mocked(syncClearCredential);
const mockPicker = vi.mocked(openFolder);

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
    settleMs: 5000,
    tags: [],
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
});

afterEach(() => {
  vi.clearAllMocks();
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

  it("syncs one folder now", async () => {
    mockFolderNow.mockResolvedValue({
      committed: true,
      pushed: true,
      pulled: false,
      filesChanged: 3,
      conflicts: [],
    });
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

    await waitFor(() => expect(mockFolderNow).toHaveBeenCalledWith("p1"));
  });

  it("surfaces a rejected row action inline", async () => {
    mockFolderNow.mockRejectedValue({ code: "serverUnreachable", message: "remote unreachable" });
    render(<SyncSection open />);
    fireEvent.click(await screen.findByRole("button", { name: SYNC_NOW_LABEL }));

    expect(await screen.findByText("remote unreachable")).toBeInTheDocument();
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
        settleMs: null,
        tags: [],
        authorOverride: null,
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
    // The acknowledgement says a token is held; it never shows which.
    expect(await screen.findByText(new RegExp(SYNC_TOKEN_STORED_LABEL))).toBeInTheDocument();
    expect(screen.queryByDisplayValue("ghp_secret")).not.toBeInTheDocument();
    expect(document.body.textContent).not.toContain("ghp_secret");
  });

  it("clears a stored token through the keychain, not by blanking a field", async () => {
    mockSetCredential.mockResolvedValue(undefined);
    mockClearCredential.mockResolvedValue(undefined);
    await openAdvanced();

    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));
    const clear = await screen.findByRole("button", { name: SYNC_TOKEN_CLEAR_LABEL });

    fireEvent.click(clear);

    await waitFor(() => expect(mockClearCredential).toHaveBeenCalledWith("p2"));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: SYNC_TOKEN_CLEAR_LABEL }),
      ).not.toBeInTheDocument(),
    );
  });

  it("says the folder was added when only the keychain write failed", async () => {
    mockSetCredential.mockRejectedValue({ code: "internal", message: "keychain refused" });
    await openAdvanced();

    fireEvent.change(screen.getByLabelText(SYNC_TOKEN_LABEL), { target: { value: "ghp_secret" } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_ADD_SUBMIT_LABEL }));

    // Two writes, two outcomes: the profile exists, so "add failed" would be a
    // lie that sends the user back to a form that can only reject as a duplicate.
    expect(
      await screen.findByText(`${SYNC_TOKEN_FAILED_PREFIX}keychain refused`),
    ).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText(SYNC_NAME_LABEL)).toHaveValue(""));
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
