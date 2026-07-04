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

function account(id: string, hue = 0): AccountVm {
  return {
    accountId: id,
    userId: `@user-${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
    provider: "password",
  };
}

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
  it("hydrates all accounts and marks hydrated when restore returns some", async () => {
    const accounts = [account("a", 0), account("b", 1)];
    sessionRestore.mockResolvedValue(accounts);
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().accounts).toEqual(accounts);
  });

  it("marks hydrated only (no accounts) when restore returns an empty array", async () => {
    sessionRestore.mockResolvedValue([]);
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("marks hydrated only (no accounts) when restore rejects", async () => {
    sessionRestore.mockRejectedValue({ code: "internal", message: "boom", retriable: false });
    renderHook(() => useSessionRestore());

    await waitFor(() => {
      expect(accountsStore.getState().hydrated).toBe(true);
    });
    expect(accountsStore.getState().accounts).toEqual([]);
  });
});
