import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
import { accountStatusStore, useAccountStatus, useShellOffline } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";

function account(accountId: string): AccountVm {
  return {
    accountId,
    userId: `@${accountId}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

beforeEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
});

afterEach(() => {
  accountStatusStore.getState().reset();
  accountsStore.getState().clear();
});

describe("accountStatusStore", () => {
  it("starts empty", () => {
    expect(accountStatusStore.getState().statuses).toEqual({});
  });

  it("records a per-account status", () => {
    accountStatusStore.getState().setStatus("a", "offline");
    expect(accountStatusStore.getState().statuses).toEqual({ a: "offline" });
  });

  it("overwrites an existing status idempotently", () => {
    accountStatusStore.getState().setStatus("a", "offline");
    accountStatusStore.getState().setStatus("a", "online");
    expect(accountStatusStore.getState().statuses).toEqual({ a: "online" });
  });

  it("removes one account's entry, keeping the others", () => {
    accountStatusStore.getState().setStatus("a", "online");
    accountStatusStore.getState().setStatus("b", "offline");
    accountStatusStore.getState().removeAccount("a");
    expect(accountStatusStore.getState().statuses).toEqual({ b: "offline" });
  });

  it("removing an absent account is a no-op", () => {
    accountStatusStore.getState().setStatus("a", "online");
    const before = accountStatusStore.getState().statuses;
    accountStatusStore.getState().removeAccount("missing");
    expect(accountStatusStore.getState().statuses).toBe(before);
  });

  it("reset clears every tracked account", () => {
    accountStatusStore.getState().setStatus("a", "online");
    accountStatusStore.getState().setStatus("b", "offline");
    accountStatusStore.getState().reset();
    expect(accountStatusStore.getState().statuses).toEqual({});
  });
});

describe("useAccountStatus", () => {
  it("returns undefined for an untracked account (pending)", () => {
    const { result } = renderHook(() => useAccountStatus("a"));
    expect(result.current).toBeUndefined();
  });

  it("returns the tracked status and updates on change", () => {
    const { result } = renderHook(() => useAccountStatus("a"));
    act(() => {
      accountStatusStore.getState().setStatus("a", "offline");
    });
    expect(result.current).toBe("offline");
    act(() => {
      accountStatusStore.getState().setStatus("a", "online");
    });
    expect(result.current).toBe("online");
  });
});

describe("useShellOffline", () => {
  it("is false when no accounts are signed in", () => {
    const { result } = renderHook(() => useShellOffline());
    expect(result.current).toBe(false);
  });

  it("is false while any signed-in account is pending (no batch yet)", () => {
    const { result } = renderHook(() => useShellOffline());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      accountStatusStore.getState().setStatus("a", "offline");
      // 'b' is signed in but never delivered a batch → pending, so the pill
      // stays hidden even though the only reported status is offline.
    });
    expect(result.current).toBe(false);
  });

  it("is true only when every signed-in account is offline", () => {
    const { result } = renderHook(() => useShellOffline());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      accountStatusStore.getState().setStatus("a", "offline");
      accountStatusStore.getState().setStatus("b", "offline");
    });
    expect(result.current).toBe(true);
  });

  it("is false when one account is offline and another is online", () => {
    const { result } = renderHook(() => useShellOffline());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      accountStatusStore.getState().setStatus("a", "offline");
      accountStatusStore.getState().setStatus("b", "online");
    });
    expect(result.current).toBe(false);
  });

  it("ignores a stale status for a signed-out account", () => {
    const { result } = renderHook(() => useShellOffline());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      accountStatusStore.getState().setStatus("a", "offline");
      // A leftover status for an account that is not signed in must not gate
      // the pill on its own.
      accountStatusStore.getState().setStatus("ghost", "online");
    });
    expect(result.current).toBe(true);
  });
});
