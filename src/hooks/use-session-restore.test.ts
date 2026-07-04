import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
const sessionRestore = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionRestore: () => sessionRestore(),
}));

import { useSessionRestore } from "@/hooks/use-session-restore";
import { accountsStore } from "@/lib/stores/accounts";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ hydrated: false });
  sessionRestore.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ hydrated: false });
});

describe("useSessionRestore", () => {
  it("hydrates the account and marks hydrated when restore returns one", async () => {
    sessionRestore.mockResolvedValue(account);
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().currentAccount).toEqual(account);
  });

  it("marks hydrated only (no account) when restore returns null", async () => {
    sessionRestore.mockResolvedValue(null);
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().currentAccount).toBeNull();
  });

  it("marks hydrated only (no account) when restore rejects", async () => {
    sessionRestore.mockRejectedValue({ code: "internal", message: "boom", retriable: false });
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().currentAccount).toBeNull();
  });
});
