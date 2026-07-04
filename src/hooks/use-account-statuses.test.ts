import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, ConnectionStatusBatch } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
// `subscribeConnectionStatus` captures each account's `onBatch` handler so the
// test can drive the streams.
const subscribeConnectionStatus = vi.fn();
const unsubscribeConnectionStatus = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeConnectionStatus: (accountId: string, onBatch: (b: ConnectionStatusBatch) => void) =>
    subscribeConnectionStatus(accountId, onBatch),
  unsubscribeConnectionStatus: (accountId: string, id: number) =>
    unsubscribeConnectionStatus(accountId, id),
}));

import { useAccountStatuses } from "@/hooks/use-account-statuses";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";

function account(id: string): AccountVm {
  return {
    accountId: id,
    userId: `@user-${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ");

beforeEach(() => {
  accountsStore.getState().clear();
  accountStatusStore.getState().reset();
  subscribeConnectionStatus.mockReset();
  unsubscribeConnectionStatus.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  accountStatusStore.getState().reset();
});

describe("useAccountStatuses", () => {
  it("does not subscribe when there are no accounts", () => {
    renderHook(() => useAccountStatuses());
    expect(subscribeConnectionStatus).not.toHaveBeenCalled();
  });

  it("subscribes every account and mirrors each stream into the per-account map", async () => {
    const captured = new Map<string, (b: ConnectionStatusBatch) => void>();
    subscribeConnectionStatus.mockImplementation(
      (accountId: string, onBatch: (b: ConnectionStatusBatch) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(captured.size);
      },
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    renderHook(() => useAccountStatuses());

    expect(subscribeConnectionStatus).toHaveBeenCalledWith(alice.accountId, expect.any(Function));
    expect(subscribeConnectionStatus).toHaveBeenCalledWith(bob.accountId, expect.any(Function));

    captured.get(alice.accountId)?.({ status: "offline" });
    captured.get(bob.accountId)?.({ status: "online" });

    await waitFor(() => {
      expect(accountStatusStore.getState().statuses).toEqual({
        [alice.accountId]: "offline",
        [bob.accountId]: "online",
      });
    });
  });

  it("unsubscribes and removes each account's entry on unmount", async () => {
    subscribeConnectionStatus.mockImplementation((accountId: string) =>
      Promise.resolve(accountId === alice.accountId ? 1 : 2),
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    const { unmount } = renderHook(() => useAccountStatuses());

    await waitFor(() => {
      expect(subscribeConnectionStatus).toHaveBeenCalledTimes(2);
    });
    accountStatusStore.getState().setStatus(alice.accountId, "offline");
    accountStatusStore.getState().setStatus(bob.accountId, "online");

    unmount();

    await waitFor(() => {
      expect(unsubscribeConnectionStatus).toHaveBeenCalledWith(alice.accountId, 1);
    });
    expect(unsubscribeConnectionStatus).toHaveBeenCalledWith(bob.accountId, 2);
    expect(accountStatusStore.getState().statuses).toEqual({});
  });

  it("ignores a batch delivered after cleanup", async () => {
    const captured = new Map<string, (b: ConnectionStatusBatch) => void>();
    subscribeConnectionStatus.mockImplementation(
      (accountId: string, onBatch: (b: ConnectionStatusBatch) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    const { unmount } = renderHook(() => useAccountStatuses());

    await waitFor(() => {
      expect(subscribeConnectionStatus).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.get(alice.accountId)?.({ status: "offline" });
    expect(accountStatusStore.getState().statuses).toEqual({});
  });
});
