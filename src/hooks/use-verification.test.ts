import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, VerificationFlowVm } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
// `subscribeVerification` captures each account's `onBatch` handler so the test
// can drive the streams; `verificationCancel` is stubbed for the store's close().
const subscribeVerification = vi.fn();
const unsubscribeVerification = vi.fn();
const verificationAccept = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeVerification: (accountId: string, onBatch: (b: VerificationFlowVm) => void) =>
    subscribeVerification(accountId, onBatch),
  unsubscribeVerification: (accountId: string, id: number) =>
    unsubscribeVerification(accountId, id),
  verificationAccept: (accountId: string, flowId: string) => verificationAccept(accountId, flowId),
  verificationCancel: () => Promise.resolve(),
}));

import { useVerification } from "@/hooks/use-verification";
import { accountsStore } from "@/lib/stores/accounts";
import { verificationStore } from "@/lib/stores/verification";

function account(id: string): AccountVm {
  return {
    accountId: id,
    userId: `@user-${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

function flow(overrides: Partial<VerificationFlowVm> = {}): VerificationFlowVm {
  return {
    flowId: "$flow1",
    phase: "requested",
    emojis: null,
    qrCodeSvg: null,
    reason: null,
    ...overrides,
  };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV");
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ");

beforeEach(() => {
  accountsStore.getState().clear();
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
  subscribeVerification.mockReset();
  unsubscribeVerification.mockReset();
  verificationAccept.mockReset();
  verificationAccept.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
});

describe("useVerification", () => {
  it("does not subscribe when there are no accounts", () => {
    renderHook(() => useVerification());
    expect(subscribeVerification).not.toHaveBeenCalled();
  });

  it("subscribes every account", async () => {
    subscribeVerification.mockImplementation((accountId: string) =>
      Promise.resolve(accountId === alice.accountId ? 1 : 2),
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    renderHook(() => useVerification());

    await waitFor(() => {
      expect(subscribeVerification).toHaveBeenCalledWith(alice.accountId, expect.any(Function));
    });
    expect(subscribeVerification).toHaveBeenCalledWith(bob.accountId, expect.any(Function));
  });

  it("auto-opens the modal on an incoming (requested) batch and mirrors the flow", async () => {
    const captured = new Map<string, (b: VerificationFlowVm) => void>();
    subscribeVerification.mockImplementation(
      (accountId: string, onBatch: (b: VerificationFlowVm) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(captured.size);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    renderHook(() => useVerification());

    await waitFor(() => {
      expect(captured.has(alice.accountId)).toBe(true);
    });

    captured.get(alice.accountId)?.(flow({ phase: "requested", flowId: "$incoming" }));

    await waitFor(() => {
      expect(verificationStore.getState().modalOpen).toBe(true);
    });
    expect(verificationStore.getState().activeAccountId).toBe(alice.accountId);
    expect(verificationStore.getState().flow?.flowId).toBe("$incoming");
    // The peer started this request, so keeper must accept it to advance it to
    // Ready — otherwise the incoming direction stalls in Requested forever.
    await waitFor(() => {
      expect(verificationAccept).toHaveBeenCalledWith(alice.accountId, "$incoming");
    });
  });

  it("does NOT auto-open when a modal is already open (keeper-started flow)", async () => {
    const captured = new Map<string, (b: VerificationFlowVm) => void>();
    subscribeVerification.mockImplementation(
      (accountId: string, onBatch: (b: VerificationFlowVm) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    // Simulate the Settings Verify button having already opened the modal.
    verificationStore.getState().openFor(alice.accountId);
    renderHook(() => useVerification());

    await waitFor(() => {
      expect(captured.has(alice.accountId)).toBe(true);
    });

    captured.get(alice.accountId)?.(flow({ phase: "requested", flowId: "$started" }));

    // The modal stays open for the same account and mirrors the flow.
    await waitFor(() => {
      expect(verificationStore.getState().flow?.flowId).toBe("$started");
    });
    expect(verificationStore.getState().activeAccountId).toBe(alice.accountId);
    // keeper-started requests are accepted by the *other* session, never by
    // keeper — so we must not auto-accept our own outgoing request.
    expect(verificationAccept).not.toHaveBeenCalled();
  });

  it("ignores a batch delivered after cleanup", async () => {
    const captured = new Map<string, (b: VerificationFlowVm) => void>();
    subscribeVerification.mockImplementation(
      (accountId: string, onBatch: (b: VerificationFlowVm) => void) => {
        captured.set(accountId, onBatch);
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().hydrateAll([alice]);
    const { unmount } = renderHook(() => useVerification());

    await waitFor(() => {
      expect(captured.has(alice.accountId)).toBe(true);
    });

    unmount();
    captured.get(alice.accountId)?.(flow({ phase: "requested" }));
    expect(verificationStore.getState().modalOpen).toBe(false);
  });

  it("unsubscribes each account on unmount", async () => {
    subscribeVerification.mockImplementation((accountId: string) =>
      Promise.resolve(accountId === alice.accountId ? 1 : 2),
    );
    accountsStore.getState().hydrateAll([alice, bob]);
    const { unmount } = renderHook(() => useVerification());

    await waitFor(() => {
      expect(subscribeVerification).toHaveBeenCalledTimes(2);
    });

    unmount();

    await waitFor(() => {
      expect(unsubscribeVerification).toHaveBeenCalledWith(alice.accountId, 1);
    });
    expect(unsubscribeVerification).toHaveBeenCalledWith(bob.accountId, 2);
  });

  it("swallows a subscribe failure", async () => {
    subscribeVerification.mockRejectedValue(new Error("stream failed"));
    accountsStore.getState().hydrateAll([alice]);
    renderHook(() => useVerification());

    await waitFor(() => {
      expect(subscribeVerification).toHaveBeenCalled();
    });
    expect(verificationStore.getState().modalOpen).toBe(false);
  });
});
