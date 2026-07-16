import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NavState } from "@/lib/ipc/gen/NavState";

vi.mock("@/lib/ipc/client", () => ({
  navStateSet: vi.fn(() => Promise.resolve()),
  navStateClear: vi.fn(() => Promise.resolve()),
  navStateGet: vi.fn(() => Promise.resolve(null)),
}));

import { useNavStatePersistence } from "@/hooks/use-nav-state-persistence";
import { navStateClear, navStateGet, navStateSet } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { detailStore } from "@/lib/stores/detail-ui";
import { roomsStore } from "@/lib/stores/rooms";

const mockNavStateSet = vi.mocked(navStateSet);
const mockNavStateClear = vi.mocked(navStateClear);
const mockNavStateGet = vi.mocked(navStateGet);

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

const storedNav: NavState = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  roomId: "!restored:example.org",
  detailOpen: false,
};

/** Flush the restore read's microtask chain, then the detail-reopen macrotask. */
async function flushRestore(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  mockNavStateSet.mockClear();
  mockNavStateSet.mockResolvedValue(undefined);
  mockNavStateClear.mockClear();
  mockNavStateClear.mockResolvedValue(undefined);
  mockNavStateGet.mockReset();
  mockNavStateGet.mockResolvedValue(null);
  roomsStore.getState().selectRoom(null);
  detailStore.setState({ open: false });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

afterEach(() => {
  roomsStore.getState().selectRoom(null);
  detailStore.setState({ open: false });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

/** Mark the mirror hydrated on the reduced (iOS) tier: all-false capabilities. */
function hydrateReducedTier(): void {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
}

describe("useNavStatePersistence", () => {
  it("restores a stored Room level on mount (no transient write back to Rust)", async () => {
    hydrateReducedTier();
    mockNavStateGet.mockResolvedValue(storedNav);

    renderHook(() => useNavStatePersistence());
    await flushRestore();

    expect(roomsStore.getState().selected).toEqual({
      accountId: storedNav.accountId,
      roomId: storedNav.roomId,
    });
    expect(detailStore.getState().open).toBe(false);
    // A clean restore reports nothing — the baseline is seeded from the restored
    // state, so no redundant (or destructive) write goes back to Rust.
    expect(mockNavStateSet).not.toHaveBeenCalled();
    expect(mockNavStateClear).not.toHaveBeenCalled();
  });

  it("restores a stored Detail level (deferred one macrotask) without ever writing detailOpen: false", async () => {
    hydrateReducedTier();
    mockNavStateGet.mockResolvedValue({ ...storedNav, detailOpen: true });

    renderHook(() => useNavStatePersistence());
    await flushRestore();

    expect(roomsStore.getState().selected).toEqual({
      accountId: storedNav.accountId,
      roomId: storedNav.roomId,
    });
    expect(detailStore.getState().open).toBe(true);
    // The Review R1 guard: the selected-but-detail-not-yet-reopened window must
    // never write `detailOpen: false` (or anything) back over the stored level.
    expect(mockNavStateSet).not.toHaveBeenCalled();
    expect(mockNavStateClear).not.toHaveBeenCalled();
  });

  it("cold launch: no stored nav stays on the Inbox", async () => {
    hydrateReducedTier();
    mockNavStateGet.mockResolvedValue(null);

    renderHook(() => useNavStatePersistence());
    await flushRestore();

    expect(roomsStore.getState().selected).toBeNull();
    expect(detailStore.getState().open).toBe(false);
    expect(mockNavStateSet).not.toHaveBeenCalled();
  });

  it("treats a rejected read as a cold launch (start at the Inbox)", async () => {
    hydrateReducedTier();
    mockNavStateGet.mockRejectedValue({ code: "internal", message: "boom", retriable: false });

    renderHook(() => useNavStatePersistence());
    await flushRestore();

    expect(roomsStore.getState().selected).toBeNull();
    expect(mockNavStateSet).not.toHaveBeenCalled();
  });

  it("user navigation made before the read resolves wins over the restore", async () => {
    hydrateReducedTier();
    let resolveRead: (nav: NavState | null) => void = () => {};
    mockNavStateGet.mockReturnValue(
      new Promise<NavState | null>((resolve) => {
        resolveRead = resolve;
      }),
    );

    renderHook(() => useNavStatePersistence());
    // The user opens a different Chat while the read is still in flight.
    act(() => {
      roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!user:example.org" });
    });
    act(() => {
      resolveRead({ ...storedNav, detailOpen: true });
    });
    await flushRestore();

    // The user's navigation stands; the stored Detail level is never re-applied.
    expect(roomsStore.getState().selected).toEqual({
      accountId: "a1",
      roomId: "!user:example.org",
    });
    expect(detailStore.getState().open).toBe(false);
    // Once the restore settles, the user's state is reported (it diverges from
    // the stored one).
    expect(mockNavStateSet).toHaveBeenLastCalledWith(
      { accountId: "a1", roomId: "!user:example.org" },
      false,
    );
  });

  it("reports each nav change after the restore settles: room, detail, back to Inbox", async () => {
    hydrateReducedTier();
    renderHook(() => useNavStatePersistence());
    await flushRestore();
    mockNavStateSet.mockClear();
    mockNavStateClear.mockClear();

    const selection = { accountId: "a1", roomId: "!r1:example.org" };
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    expect(mockNavStateSet).toHaveBeenLastCalledWith(selection, false);

    act(() => {
      detailStore.getState().openDetail();
    });
    expect(mockNavStateSet).toHaveBeenLastCalledWith(selection, true);

    act(() => {
      detailStore.getState().closeDetail();
    });
    expect(mockNavStateSet).toHaveBeenLastCalledWith(selection, false);

    act(() => {
      roomsStore.getState().selectRoom(null);
    });
    expect(mockNavStateClear).toHaveBeenCalledTimes(1);
  });

  it("dedupes by value: a same-room re-selection with a fresh literal does not re-report", async () => {
    hydrateReducedTier();
    renderHook(() => useNavStatePersistence());
    await flushRestore();

    const selection = { accountId: "a1", roomId: "!r1:example.org" };
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    mockNavStateSet.mockClear();

    // Fresh object literal, identical value — no redundant IPC round-trip.
    act(() => {
      roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r1:example.org" });
    });
    expect(mockNavStateSet).not.toHaveBeenCalled();
  });

  it("desktop tier: no restore read, no reports, no store writes", async () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    mockNavStateGet.mockResolvedValue(storedNav);

    renderHook(() => useNavStatePersistence());
    await flushRestore();

    expect(mockNavStateGet).not.toHaveBeenCalled();
    expect(roomsStore.getState().selected).toBeNull();

    act(() => {
      roomsStore.getState().selectRoom({ accountId: "a1", roomId: "!r1:example.org" });
    });
    expect(mockNavStateSet).not.toHaveBeenCalled();
    expect(mockNavStateClear).not.toHaveBeenCalled();
  });

  it("does not clear the Rust-held state on unmount (it must outlive this JS context)", async () => {
    hydrateReducedTier();
    mockNavStateGet.mockResolvedValue(storedNav);
    const { unmount } = renderHook(() => useNavStatePersistence());
    await flushRestore();

    unmount();
    expect(mockNavStateClear).not.toHaveBeenCalled();
  });
});
