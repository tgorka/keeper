import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRootVm } from "@/lib/ipc/client";

/**
 * The section's whole IPC surface: which folders are flagged as zones, and the
 * one setting it moves. A module factory replaces the WHOLE module, so an
 * omitted name is `undefined` and the failure would be a rejected promise
 * nothing renders rather than an error anyone sees — hence all three.
 */
vi.mock("@/lib/ipc/client", () => ({
  sessionsRoots: vi.fn(),
  sessionsSpacesFoldedGet: vi.fn(),
  sessionsSpacesFoldedSet: vi.fn(),
}));

import {
  SESSIONS_SECTION_TITLE,
  SESSIONS_SPACES_FOLDED_LABEL,
  SESSIONS_SPACES_FOLDED_NOTE,
  SessionsSettingsSection,
} from "@/components/sessions/sessions-settings";
import { sessionsRoots, sessionsSpacesFoldedGet, sessionsSpacesFoldedSet } from "@/lib/ipc/client";
import {
  resetSessionSpacesFoldForTest,
  sessionSpacesFoldStore,
  setSpacesFoldedDefault,
} from "@/lib/stores/session-spaces-fold";
import { resetSessionsRootsStoreForTest } from "@/lib/stores/sessions-roots";

const mockRoots = vi.mocked(sessionsRoots);
const mockGet = vi.mocked(sessionsSpacesFoldedGet);
const mockSet = vi.mocked(sessionsSpacesFoldedSet);

function root(): SessionRootVm {
  return {
    id: "tgdrive",
    name: "tgdrive",
    subfolder: "60-sessions",
    root: "/Users/tgorka/tgdrive/60-sessions",
    indexed: true,
    activeCount: 3,
    unreadCount: 0,
  };
}

/** The switch, once the section has decided it has anything to show. */
function switchControl() {
  return screen.findByRole("switch", { name: SESSIONS_SPACES_FOLDED_LABEL });
}

beforeEach(() => {
  mockRoots.mockReset();
  mockRoots.mockResolvedValue([root()]);
  mockGet.mockReset();
  mockGet.mockResolvedValue(false);
  mockSet.mockReset();
  mockSet.mockResolvedValue(undefined);
  resetSessionsRootsStoreForTest();
  resetSessionSpacesFoldForTest();
});

afterEach(() => {
  vi.clearAllMocks();
  resetSessionsRootsStoreForTest();
  resetSessionSpacesFoldForTest();
});

describe("the Sessions settings section", () => {
  /**
   * Matrix row 12. Absent, not empty and not disabled: a fold default is a fact
   * about spaces, spaces live in a zone, and with no folder flagged as one
   * there is nothing here to set. `CaptureSettingsSection`'s rule for a vault.
   */
  it("row 12: renders nothing when no synced folder is a sessions root", async () => {
    mockRoots.mockResolvedValue([]);

    render(<SessionsSettingsSection open={true} />);

    await waitFor(() => expect(mockRoots).toHaveBeenCalled());
    expect(screen.queryByText(SESSIONS_SECTION_TITLE)).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
    // Not the read either: nothing to configure, nothing to ask Rust about.
    expect(mockGet).not.toHaveBeenCalled();
  });

  it("says nothing at all while it is still finding out", () => {
    render(<SessionsSettingsSection open={true} />);

    expect(screen.queryByText(SESSIONS_SECTION_TITLE)).toBeNull();
  });

  it("does not read anything until the dialog is open", () => {
    render(<SessionsSettingsSection open={false} />);

    expect(mockRoots).not.toHaveBeenCalled();
  });

  /**
   * Matrix row 10, first third: `undefined` while the read is out, so the
   * switch is disabled rather than claiming a state keeper has not looked up.
   * An off-looking switch that is actually unknown is how somebody flips a
   * setting to the value it already had and believes they changed it.
   */
  it("row 10: is disabled until the stored value has arrived", async () => {
    let answer: (value: boolean) => void = () => {};
    mockGet.mockReturnValue(
      new Promise<boolean>((resolve) => {
        answer = resolve;
      }),
    );

    render(<SessionsSettingsSection open={true} />);

    const control = await switchControl();
    expect(control).toBeDisabled();
    answer(true);
    await waitFor(() => expect(control).toBeEnabled());
    expect(control).toBeChecked();
  });

  it("row 10: shows the stored value and explains the exception to it", async () => {
    mockGet.mockResolvedValue(true);

    render(<SessionsSettingsSection open={true} />);

    const control = await switchControl();
    await waitFor(() => expect(control).toBeChecked());
    expect(screen.getByText(SESSIONS_SPACES_FOLDED_NOTE)).toBeInTheDocument();
  });

  /**
   * Matrix row 10, second third, and the half of row 11 that lives here: the
   * write goes to Rust AND the store's fallback moves, or the detail behind the
   * dialog would keep the old answer until the next restart.
   */
  it("row 10: sets optimistically and moves the fold's default with it", async () => {
    render(<SessionsSettingsSection open={true} />);
    const control = await switchControl();
    await waitFor(() => expect(control).toBeEnabled());

    fireEvent.click(control);

    expect(control).toBeChecked();
    expect(sessionSpacesFoldStore.getState().defaultFolded).toBe(true);
    await waitFor(() => expect(mockSet).toHaveBeenCalledWith(true));
  });

  /**
   * Matrix row 10, last third. A switch left showing a value that was never
   * saved is the specific dishonesty the `menu_bar_presence` idiom exists to
   * prevent — and the store's fallback has to come back with it, or the spaces
   * on screen would obey a setting Rust refused.
   */
  it("row 10: reverts the switch and the default when the write is refused", async () => {
    mockSet.mockRejectedValue(new Error("no registry"));
    render(<SessionsSettingsSection open={true} />);
    const control = await switchControl();
    await waitFor(() => expect(control).toBeEnabled());

    fireEvent.click(control);

    await waitFor(() => expect(control).not.toBeChecked());
    expect(sessionSpacesFoldStore.getState().defaultFolded).toBe(false);
  });

  /**
   * The ordering the `writeId` guard does NOT provide, and nothing tested
   * before: that guard drops a stale REVERT, but two `sessionsSpacesFoldedSet`
   * calls in flight are unordered, so on-then-off could commit `0` then `1` and
   * leave `sessions.spaces_folded` holding the value the person turned off
   * while the switch and the store both show off. Nothing re-reads, so the
   * disagreement would survive until the next document.
   */
  it("row 10: serialises two fast flips, so the last one is the last write", async () => {
    const gates: Array<() => void> = [];
    mockSet.mockImplementation(() => new Promise<void>((resolve) => gates.push(resolve)));
    render(<SessionsSettingsSection open={true} />);
    const control = await switchControl();
    await waitFor(() => expect(control).toBeEnabled());

    fireEvent.click(control);
    fireEvent.click(control);

    // One at a time: the second write is not even issued while the first is out.
    await waitFor(() => expect(mockSet).toHaveBeenCalledTimes(1));
    expect(mockSet).toHaveBeenNthCalledWith(1, true);

    gates[0]?.();

    await waitFor(() => expect(mockSet).toHaveBeenCalledTimes(2));
    expect(mockSet).toHaveBeenNthCalledWith(2, false);
    expect(control).not.toBeChecked();
    expect(sessionSpacesFoldStore.getState().defaultFolded).toBe(false);
  });

  /** A read that fails is not evidence that spaces should arrive shut: the
   *  store starts unfolded, and that is what the row reports. */
  it("falls back to open when the stored value cannot be read", async () => {
    mockGet.mockRejectedValue(new Error("no registry"));

    render(<SessionsSettingsSection open={true} />);

    const control = await switchControl();
    await waitFor(() => expect(control).toBeEnabled());
    expect(control).not.toBeChecked();
  });

  /**
   * ...but "open" is the store's answer, not a hard-coded one. A detail that
   * mounted earlier read the setting successfully, and every untouched space on
   * screen is folding to it; a later transient failure of the same command must
   * not render an ENABLED switch claiming the opposite of them, where flipping
   * it on is a no-op the person reads as the setting being broken.
   */
  it("reports the fold default the spaces are already obeying when the read fails", async () => {
    setSpacesFoldedDefault(true);
    mockGet.mockRejectedValue(new Error("no registry"));

    render(<SessionsSettingsSection open={true} />);

    const control = await switchControl();
    await waitFor(() => expect(control).toBeChecked());
    expect(control).toBeEnabled();
    // Reported, never written: reporting a fold must not fold anything.
    expect(sessionSpacesFoldStore.getState().defaultFolded).toBe(true);
  });
});
