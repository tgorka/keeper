import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, EncryptionStatusBatch } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
// `subscribeEncryptionStatus` captures each account's `onBatch` handler so the
// test can drive the streams.
const subscribeEncryptionStatus = vi.fn();
const unsubscribeEncryptionStatus = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeEncryptionStatus: (accountId: string, onBatch: (b: EncryptionStatusBatch) => void) =>
    subscribeEncryptionStatus(accountId, onBatch),
  unsubscribeEncryptionStatus: (accountId: string, id: number) =>
    unsubscribeEncryptionStatus(accountId, id),
}));

import { useEncryptionStatuses } from "@/hooks/use-encryption-statuses";
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";

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
  encryptionStatusStore.getState().reset();
  subscribeEncryptionStatus.mockReset();
  unsubscribeEncryptionStatus.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  encryptionStatusStore.getState().reset();
});

describe("useEncryptionStatuses", () => {
  it("does not subscribe when there are no accounts", () => {
    renderHook(() => useEncryptionStatuses());
    expect(subscribeEncryptionStatus).not.toHaveBeenCalled();
  });

  it("subscribes every account and mirrors each stream into the per-account map", async () => {
    const captured = new Map<string, (b: EncryptionStatusBatch) => void>();
    subscribeEncryptionStatus.mockImplementation(
      (accountId: string, onBatch: (b: EncryptionStatusBatch) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(captured.size);
      },
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    renderHook(() => useEncryptionStatuses());

    expect(subscribeEncryptionStatus).toHaveBeenCalledWith(alice.accountId, expect.any(Function));
    expect(subscribeEncryptionStatus).toHaveBeenCalledWith(bob.accountId, expect.any(Function));

    captured.get(alice.accountId)?.({ status: "unverified" });
    captured.get(bob.accountId)?.({ status: "verified" });

    await waitFor(() => {
      expect(encryptionStatusStore.getState().statuses).toEqual({
        [alice.accountId]: "unverified",
        [bob.accountId]: "verified",
      });
    });
  });

  it("unsubscribes and removes each account's entry on unmount", async () => {
    subscribeEncryptionStatus.mockImplementation((accountId: string) =>
      Promise.resolve(accountId === alice.accountId ? 1 : 2),
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    const { unmount } = renderHook(() => useEncryptionStatuses());

    await waitFor(() => {
      expect(subscribeEncryptionStatus).toHaveBeenCalledTimes(2);
    });
    encryptionStatusStore.getState().setStatus(alice.accountId, "unverified");
    encryptionStatusStore.getState().setStatus(bob.accountId, "verified");

    unmount();

    await waitFor(() => {
      expect(unsubscribeEncryptionStatus).toHaveBeenCalledWith(alice.accountId, 1);
    });
    expect(unsubscribeEncryptionStatus).toHaveBeenCalledWith(bob.accountId, 2);
    expect(encryptionStatusStore.getState().statuses).toEqual({});
  });

  it("ignores a batch delivered after cleanup", async () => {
    const captured = new Map<string, (b: EncryptionStatusBatch) => void>();
    subscribeEncryptionStatus.mockImplementation(
      (accountId: string, onBatch: (b: EncryptionStatusBatch) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    const { unmount } = renderHook(() => useEncryptionStatuses());

    await waitFor(() => {
      expect(subscribeEncryptionStatus).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.get(alice.accountId)?.({ status: "unverified" });
    expect(encryptionStatusStore.getState().statuses).toEqual({});
  });

  it("swallows a subscribe failure and leaves the account pending", async () => {
    subscribeEncryptionStatus.mockRejectedValue(new Error("stream failed"));
    accountsStore.getState().hydrateAll([alice]);
    renderHook(() => useEncryptionStatuses());

    await waitFor(() => {
      expect(subscribeEncryptionStatus).toHaveBeenCalled();
    });
    // No throw, no status recorded.
    expect(encryptionStatusStore.getState().statuses).toEqual({});
  });
});
