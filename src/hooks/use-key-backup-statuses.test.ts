import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, BackupStatus } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
const subscribeBackupStatus = vi.fn();
const unsubscribeBackupStatus = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeBackupStatus: (accountId: string, onStatus: (s: BackupStatus) => void) =>
    subscribeBackupStatus(accountId, onStatus),
  unsubscribeBackupStatus: (accountId: string, id: number) =>
    unsubscribeBackupStatus(accountId, id),
}));

import { useKeyBackupStatuses } from "@/hooks/use-key-backup-statuses";
import { accountsStore } from "@/lib/stores/accounts";
import { keyBackupStore } from "@/lib/stores/key-backup";

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
  keyBackupStore.getState().reset();
  subscribeBackupStatus.mockReset();
  unsubscribeBackupStatus.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  keyBackupStore.getState().reset();
});

describe("useKeyBackupStatuses", () => {
  it("does not subscribe when there are no accounts", () => {
    renderHook(() => useKeyBackupStatuses());
    expect(subscribeBackupStatus).not.toHaveBeenCalled();
  });

  it("subscribes every account and mirrors each stream into the per-account map", async () => {
    const captured = new Map<string, (s: BackupStatus) => void>();
    subscribeBackupStatus.mockImplementation(
      (accountId: string, onStatus: (s: BackupStatus) => void) => {
        captured.set(accountId, onStatus);
        return Promise.resolve(captured.size);
      },
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    renderHook(() => useKeyBackupStatuses());

    expect(subscribeBackupStatus).toHaveBeenCalledWith(alice.accountId, expect.any(Function));
    expect(subscribeBackupStatus).toHaveBeenCalledWith(bob.accountId, expect.any(Function));

    captured.get(alice.accountId)?.("incomplete");
    captured.get(bob.accountId)?.("enabled");

    await waitFor(() => {
      expect(keyBackupStore.getState().statuses).toEqual({
        [alice.accountId]: "incomplete",
        [bob.accountId]: "enabled",
      });
    });
  });

  it("unsubscribes and removes each account's entry on unmount", async () => {
    subscribeBackupStatus.mockImplementation((accountId: string) =>
      Promise.resolve(accountId === alice.accountId ? 1 : 2),
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    const { unmount } = renderHook(() => useKeyBackupStatuses());

    await waitFor(() => {
      expect(subscribeBackupStatus).toHaveBeenCalledTimes(2);
    });
    keyBackupStore.getState().setStatus(alice.accountId, "disabled");
    keyBackupStore.getState().setStatus(bob.accountId, "enabled");

    unmount();

    await waitFor(() => {
      expect(unsubscribeBackupStatus).toHaveBeenCalledWith(alice.accountId, 1);
    });
    expect(unsubscribeBackupStatus).toHaveBeenCalledWith(bob.accountId, 2);
    expect(keyBackupStore.getState().statuses).toEqual({});
  });

  it("ignores a status delivered after cleanup", async () => {
    const captured = new Map<string, (s: BackupStatus) => void>();
    subscribeBackupStatus.mockImplementation(
      (accountId: string, onStatus: (s: BackupStatus) => void) => {
        captured.set(accountId, onStatus);
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    const { unmount } = renderHook(() => useKeyBackupStatuses());

    await waitFor(() => {
      expect(subscribeBackupStatus).toHaveBeenCalled();
    });

    unmount();

    // A late status (arriving after cleanup) must not mutate the store.
    captured.get(alice.accountId)?.("enabled");
    expect(keyBackupStore.getState().statuses).toEqual({});
  });

  it("swallows a subscribe failure and leaves the account pending", async () => {
    subscribeBackupStatus.mockRejectedValue(new Error("stream failed"));
    accountsStore.getState().hydrateAll([alice]);
    renderHook(() => useKeyBackupStatuses());

    await waitFor(() => {
      expect(subscribeBackupStatus).toHaveBeenCalled();
    });
    expect(keyBackupStore.getState().statuses).toEqual({});
  });
});
