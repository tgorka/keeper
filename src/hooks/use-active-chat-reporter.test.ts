import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  activeChatSet: vi.fn(() => Promise.resolve()),
}));

import { useActiveChatReporter } from "@/hooks/use-active-chat-reporter";
import { activeChatSet } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { roomsStore } from "@/lib/stores/rooms";

const mockActiveChatSet = vi.mocked(activeChatSet);

/** All capabilities present = the desktop tier. */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  recording: false,
};

describe("useActiveChatReporter", () => {
  beforeEach(() => {
    mockActiveChatSet.mockClear();
    mockActiveChatSet.mockResolvedValue(undefined);
    // Reset both stores to a clean baseline.
    roomsStore.getState().selectRoom(null);
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  });

  afterEach(() => {
    roomsStore.getState().selectRoom(null);
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  });

  it("reduced tier: reports the current selection at mount", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    roomsStore.getState().selectRoom({ accountId: "acct", roomId: "!room:example.org" });

    renderHook(() => useActiveChatReporter());

    expect(mockActiveChatSet).toHaveBeenCalledWith({
      accountId: "acct",
      roomId: "!room:example.org",
    });
  });

  it("reduced tier: reports each selection change and clears on null", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    // Mount with no open Chat (reports null once at attach).
    renderHook(() => useActiveChatReporter());
    mockActiveChatSet.mockClear();

    roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r1:example.org" });
    expect(mockActiveChatSet).toHaveBeenLastCalledWith({
      accountId: "a1",
      roomId: "!r1:example.org",
    });

    roomsStore.getState().selectRoom(null);
    expect(mockActiveChatSet).toHaveBeenLastCalledWith(null);
  });

  it("reduced tier: does not re-report a value-equal re-selection", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r1:example.org" });
    renderHook(() => useActiveChatReporter());
    mockActiveChatSet.mockClear();

    // Re-select the SAME room via a fresh object literal (new identity, same value).
    roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r1:example.org" });
    expect(mockActiveChatSet).not.toHaveBeenCalled();

    // A genuinely different room still reports.
    roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r2:example.org" });
    expect(mockActiveChatSet).toHaveBeenCalledTimes(1);
    expect(mockActiveChatSet).toHaveBeenLastCalledWith({
      accountId: "a1",
      roomId: "!r2:example.org",
    });
  });

  it("reduced tier: clears the active Chat on unmount", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    roomsStore.getState().selectRoom({ accountId: "acct", roomId: "!room:example.org" });
    const { unmount } = renderHook(() => useActiveChatReporter());
    mockActiveChatSet.mockClear();

    unmount();
    expect(mockActiveChatSet).toHaveBeenCalledWith(null);
  });

  it("desktop tier: never reports (no active-chat signal on desktop)", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    roomsStore.getState().selectRoom({ accountId: "acct", roomId: "!room:example.org" });

    const { unmount } = renderHook(() => useActiveChatReporter());
    // No report at mount…
    expect(mockActiveChatSet).not.toHaveBeenCalled();

    // …and a selection change on desktop is ignored (no subscription attached).
    roomsStore.getState().selectRoom({ accountId: "a2", roomId: "!r2:example.org" });
    expect(mockActiveChatSet).not.toHaveBeenCalled();

    unmount();
    expect(mockActiveChatSet).not.toHaveBeenCalled();
  });

  it("pre-hydration: reports nothing until the tier resolves", () => {
    // Not hydrated → predicate false → no subscription.
    roomsStore.getState().selectRoom({ accountId: "acct", roomId: "!room:example.org" });
    renderHook(() => useActiveChatReporter());
    expect(mockActiveChatSet).not.toHaveBeenCalled();
  });
});
