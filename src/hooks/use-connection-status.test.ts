import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, ConnectionStatusBatch } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
// `subscribeConnectionStatus` captures the `onBatch` handler so the test can
// drive the stream.
const subscribeConnectionStatus = vi.fn();
const unsubscribeConnectionStatus = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeConnectionStatus: (accountId: string, onBatch: (b: ConnectionStatusBatch) => void) =>
    subscribeConnectionStatus(accountId, onBatch),
  unsubscribeConnectionStatus: (accountId: string, id: number) =>
    unsubscribeConnectionStatus(accountId, id),
}));

import { useConnectionStatus } from "@/hooks/use-connection-status";
import { accountsStore } from "@/lib/stores/accounts";
import { connectionStore } from "@/lib/stores/connection";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

beforeEach(() => {
  accountsStore.getState().clear();
  connectionStore.getState().reset();
  subscribeConnectionStatus.mockReset();
  unsubscribeConnectionStatus.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  connectionStore.getState().reset();
});

describe("useConnectionStatus", () => {
  it("does not subscribe when there is no account", () => {
    renderHook(() => useConnectionStatus());
    expect(subscribeConnectionStatus).not.toHaveBeenCalled();
  });

  it("subscribes with the current account id and mirrors streamed batches", async () => {
    const captured: { onBatch: ((b: ConnectionStatusBatch) => void) | null } = { onBatch: null };
    subscribeConnectionStatus.mockImplementation(
      (_accountId, onBatch: (b: ConnectionStatusBatch) => void) => {
        captured.onBatch = onBatch;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().setCurrentAccount(account);
    renderHook(() => useConnectionStatus());

    expect(subscribeConnectionStatus).toHaveBeenCalledWith(account.accountId, expect.any(Function));

    captured.onBatch?.({ status: "offline" });
    await waitFor(() => {
      expect(connectionStore.getState().status).toBe("offline");
    });

    captured.onBatch?.({ status: "online" });
    await waitFor(() => {
      expect(connectionStore.getState().status).toBe("online");
    });
  });

  it("unsubscribes and resets the store on unmount", async () => {
    subscribeConnectionStatus.mockResolvedValue(7);
    accountsStore.getState().setCurrentAccount(account);
    const { unmount } = renderHook(() => useConnectionStatus());

    await waitFor(() => {
      expect(subscribeConnectionStatus).toHaveBeenCalled();
    });

    connectionStore.getState().applyBatch({ status: "offline" });
    unmount();

    await waitFor(() => {
      expect(unsubscribeConnectionStatus).toHaveBeenCalledWith(account.accountId, 7);
    });
    expect(connectionStore.getState().status).toBe("online");
  });

  it("ignores a batch delivered after cleanup", async () => {
    const captured: { onBatch: ((b: ConnectionStatusBatch) => void) | null } = { onBatch: null };
    subscribeConnectionStatus.mockImplementation(
      (_accountId, onBatch: (b: ConnectionStatusBatch) => void) => {
        captured.onBatch = onBatch;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().setCurrentAccount(account);
    const { unmount } = renderHook(() => useConnectionStatus());

    await waitFor(() => {
      expect(subscribeConnectionStatus).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.onBatch?.({ status: "offline" });
    expect(connectionStore.getState().status).toBe("online");
  });
});
