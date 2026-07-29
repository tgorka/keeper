import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncGitStatus: vi.fn(),
  syncGitPathSet: vi.fn(),
  // Setting a path changes `capabilities.sync`, so the row re-reads it.
  capabilities: vi.fn(),
}));

import {
  SYNC_GIT_APPLIED_SENTENCE,
  SYNC_GIT_CLEAR_LABEL,
  SYNC_GIT_CLEARED_SENTENCE,
  SYNC_GIT_OK_NOTE,
  SYNC_GIT_PATH_LABEL,
  SYNC_GIT_PROBLEM_NOTE,
  SYNC_GIT_SAVE_LABEL,
  SYNC_GIT_TITLE,
  SyncGitRow,
} from "@/components/settings/sync-git-row";
import { capabilities, syncGitPathSet, syncGitStatus } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockStatus = vi.mocked(syncGitStatus);
const mockPathSet = vi.mocked(syncGitPathSet);
const mockCapabilities = vi.mocked(capabilities);

/** A resolution that found a usable binary. */
function okVm(over: Record<string, unknown> = {}) {
  return {
    state: "ok" as const,
    summary: "git 2.52 at /opt/homebrew/bin/git (clears the 2.42 floor)",
    problem: null,
    configuredPath: null,
    ...over,
  };
}

/** A resolution that found something and refused it. */
function tooOldVm(over: Record<string, unknown> = {}) {
  return {
    state: "tooOld" as const,
    summary: null,
    problem: "/usr/local/bin/git is 2.23, below the 2.42 floor\nskipped /usr/bin/git: 2.39",
    configuredPath: null,
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
  mockStatus.mockResolvedValue(okVm());
  mockCapabilities.mockResolvedValue(DEFAULT_CAPABILITIES);
});

describe("SyncGitRow", () => {
  it("renders the Rust-composed summary verbatim when a git was chosen", async () => {
    render(<SyncGitRow open />);

    expect(await screen.findByText(SYNC_GIT_TITLE)).toBeInTheDocument();
    // Verbatim: the same sentence `keeper-syncd doctor` prints, so the two
    // surfaces cannot word one machine's state two different ways.
    expect(
      screen.getByText("git 2.52 at /opt/homebrew/bin/git (clears the 2.42 floor)"),
    ).toBeInTheDocument();
    expect(screen.getByText(SYNC_GIT_OK_NOTE)).toBeInTheDocument();
    expect(screen.queryByText(SYNC_GIT_PROBLEM_NOTE)).not.toBeInTheDocument();
  });

  it("renders the refusal, naming every candidate, when nothing cleared the floor", async () => {
    mockStatus.mockResolvedValue(tooOldVm());
    render(<SyncGitRow open />);

    // The whole point of the story: "using /opt/homebrew/bin/git" is not an
    // answer on a box with three of them, and neither is silence.
    expect(
      await screen.findByText(/\/usr\/local\/bin\/git is 2\.23, below the 2\.42 floor/),
    ).toBeInTheDocument();
    expect(screen.getByText(SYNC_GIT_PROBLEM_NOTE)).toBeInTheDocument();
  });

  /**
   * This is the reason the row is not inside `SyncSection`: that section is
   * gated on `capabilities.sync`, which IS "a usable git was found", so it does
   * not render on the machines this report exists for.
   */
  it("renders on a machine with no usable git, where the Sync section cannot", async () => {
    mockStatus.mockResolvedValue(tooOldVm());
    render(<SyncGitRow open />);

    await screen.findByText(SYNC_GIT_TITLE);
    expect(capabilitiesStore.getState().capabilities.sync).toBe(false);
    expect(screen.getByLabelText(SYNC_GIT_PATH_LABEL)).toBeInTheDocument();
  });

  it("renders nothing at all on a build without folder sync", async () => {
    mockStatus.mockResolvedValue({
      state: "unsupported",
      summary: null,
      problem: null,
      configuredPath: null,
    });
    render(<SyncGitRow open />);

    // Telling a phone user about a git version floor would be noise.
    await waitFor(() => expect(mockStatus).toHaveBeenCalled());
    expect(screen.queryByText(SYNC_GIT_TITLE)).not.toBeInTheDocument();
  });

  it("claims nothing before the read lands", () => {
    mockStatus.mockReturnValue(Promise.race([]));
    render(<SyncGitRow open />);

    expect(screen.queryByText(SYNC_GIT_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByText(SYNC_GIT_OK_NOTE)).not.toBeInTheDocument();
  });

  it("sends the typed path and adopts the report that comes back", async () => {
    mockStatus.mockResolvedValue(tooOldVm());
    mockPathSet.mockResolvedValue(okVm({ configuredPath: "/opt/homebrew/bin/git" }));
    render(<SyncGitRow open />);

    const field = await screen.findByLabelText(SYNC_GIT_PATH_LABEL);
    fireEvent.change(field, { target: { value: "  /opt/homebrew/bin/git  " } });
    fireEvent.click(screen.getByRole("button", { name: SYNC_GIT_SAVE_LABEL }));

    // Trimmed: a pasted path with a trailing space is not a different path.
    await waitFor(() => expect(mockPathSet).toHaveBeenCalledWith("/opt/homebrew/bin/git"));
    expect(await screen.findByText(SYNC_GIT_APPLIED_SENTENCE)).toBeInTheDocument();
    // The refusal is gone because the report replaced it, not because the row
    // guessed the change had worked.
    expect(screen.queryByText(SYNC_GIT_PROBLEM_NOTE)).not.toBeInTheDocument();
    expect(screen.getByText(SYNC_GIT_OK_NOTE)).toBeInTheDocument();
  });

  it("re-reads capabilities so a repaired git reveals the Sync section", async () => {
    mockStatus.mockResolvedValue(tooOldVm());
    mockPathSet.mockResolvedValue(okVm({ configuredPath: "/opt/homebrew/bin/git" }));
    mockCapabilities.mockResolvedValue({ ...DEFAULT_CAPABILITIES, sync: true });
    render(<SyncGitRow open />);

    fireEvent.change(await screen.findByLabelText(SYNC_GIT_PATH_LABEL), {
      target: { value: "/opt/homebrew/bin/git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_GIT_SAVE_LABEL }));

    // Without this the section stays hidden until the dialog is reopened, which
    // reads as the fix not having worked.
    await waitFor(() => expect(capabilitiesStore.getState().capabilities.sync).toBe(true));
  });

  it("keeps a rejected path rather than silently falling back to automatic", async () => {
    mockStatus.mockResolvedValue(okVm());
    // The engine refused it, and says so, while still reporting it as the
    // setting in force. A field that cleared itself here would be a silent
    // fallback to automatic — the defect the story exists to end.
    mockPathSet.mockResolvedValue(tooOldVm({ configuredPath: "/usr/local/bin/git" }));
    render(<SyncGitRow open />);

    fireEvent.change(await screen.findByLabelText(SYNC_GIT_PATH_LABEL), {
      target: { value: "/usr/local/bin/git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_GIT_SAVE_LABEL }));

    await waitFor(() => expect(screen.getByText(SYNC_GIT_PROBLEM_NOTE)).toBeInTheDocument());
    expect(screen.getByLabelText(SYNC_GIT_PATH_LABEL)).toHaveValue("/usr/local/bin/git");
  });

  it("clears the setting with an empty string and says which thing happened", async () => {
    mockStatus.mockResolvedValue(okVm({ configuredPath: "/opt/homebrew/bin/git" }));
    mockPathSet.mockResolvedValue(okVm({ configuredPath: null }));
    render(<SyncGitRow open />);

    fireEvent.click(await screen.findByRole("button", { name: SYNC_GIT_CLEAR_LABEL }));

    await waitFor(() => expect(mockPathSet).toHaveBeenCalledWith(""));
    // Not "Saved": the opposite action deserves the opposite sentence.
    expect(await screen.findByText(SYNC_GIT_CLEARED_SENTENCE)).toBeInTheDocument();
    expect(screen.queryByText(SYNC_GIT_APPLIED_SENTENCE)).not.toBeInTheDocument();
  });

  it("offers no clear when there is no stored setting, and no save with an empty field", async () => {
    render(<SyncGitRow open />);

    await screen.findByText(SYNC_GIT_TITLE);
    // Automatic already: clearing would do nothing.
    expect(screen.getByRole("button", { name: SYNC_GIT_CLEAR_LABEL })).toBeDisabled();
    expect(screen.getByRole("button", { name: SYNC_GIT_SAVE_LABEL })).toBeDisabled();
  });

  it("shows the Rust-authored reason when the write is refused", async () => {
    // An IPC rejection is a `{ code, message }` object, not an `Error`;
    // stringifying one prints "[object Object]" where this belongs.
    mockPathSet.mockRejectedValue({ code: "internal", message: "settings store is read-only" });
    render(<SyncGitRow open />);

    fireEvent.change(await screen.findByLabelText(SYNC_GIT_PATH_LABEL), {
      target: { value: "/opt/homebrew/bin/git" },
    });
    fireEvent.click(screen.getByRole("button", { name: SYNC_GIT_SAVE_LABEL }));

    expect(await screen.findByText("settings store is read-only")).toBeInTheDocument();
  });

  it("does not read until the dialog is open", () => {
    render(<SyncGitRow open={false} />);
    expect(mockStatus).not.toHaveBeenCalled();
  });
});
