import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import {
  encryptionStatusStore,
  useAnyUnverified,
  useEncryptionStatus,
  useShowVerifyBadge,
  useShowVerifyBadgeForAccount,
  useShowVerifyBanner,
} from "@/lib/stores/encryption-status";

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
  encryptionStatusStore.getState().reset();
  accountsStore.getState().clear();
});

afterEach(() => {
  encryptionStatusStore.getState().reset();
  accountsStore.getState().clear();
});

describe("encryptionStatusStore", () => {
  it("starts empty and not dismissed", () => {
    expect(encryptionStatusStore.getState().statuses).toEqual({});
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(false);
  });

  it("records and overwrites a per-account status idempotently", () => {
    encryptionStatusStore.getState().setStatus("a", "unverified");
    expect(encryptionStatusStore.getState().statuses).toEqual({ a: "unverified" });
    encryptionStatusStore.getState().setStatus("a", "verified");
    expect(encryptionStatusStore.getState().statuses).toEqual({ a: "verified" });
  });

  it("removes one account's entry, keeping the others", () => {
    encryptionStatusStore.getState().setStatus("a", "verified");
    encryptionStatusStore.getState().setStatus("b", "unverified");
    encryptionStatusStore.getState().removeAccount("a");
    expect(encryptionStatusStore.getState().statuses).toEqual({ b: "unverified" });
  });

  it("removing an absent account is a no-op (same reference)", () => {
    encryptionStatusStore.getState().setStatus("a", "verified");
    const before = encryptionStatusStore.getState().statuses;
    encryptionStatusStore.getState().removeAccount("missing");
    expect(encryptionStatusStore.getState().statuses).toBe(before);
  });

  it("dismissBanner flips the session flag; reset clears everything", () => {
    encryptionStatusStore.getState().setStatus("a", "unverified");
    encryptionStatusStore.getState().dismissBanner();
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(true);
    encryptionStatusStore.getState().reset();
    expect(encryptionStatusStore.getState().statuses).toEqual({});
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(false);
  });
});

describe("useEncryptionStatus", () => {
  it("returns undefined for an untracked account, then updates", () => {
    const { result } = renderHook(() => useEncryptionStatus("a"));
    expect(result.current).toBeUndefined();
    act(() => {
      encryptionStatusStore.getState().setStatus("a", "unverified");
    });
    expect(result.current).toBe("unverified");
  });
});

describe("useAnyUnverified", () => {
  it("is false with no signed-in accounts", () => {
    const { result } = renderHook(() => useAnyUnverified());
    expect(result.current).toBe(false);
  });

  it("is true only when a signed-in account is explicitly unverified", () => {
    const { result } = renderHook(() => useAnyUnverified());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      encryptionStatusStore.getState().setStatus("a", "verified");
      encryptionStatusStore.getState().setStatus("b", "unverified");
    });
    expect(result.current).toBe(true);
  });

  it("is false on unknown or pending (never nags before crypto syncs)", () => {
    const { result } = renderHook(() => useAnyUnverified());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      encryptionStatusStore.getState().setStatus("a", "unknown");
      // 'b' pending (no batch).
    });
    expect(result.current).toBe(false);
  });

  it("ignores a stale status for a signed-out account", () => {
    const { result } = renderHook(() => useAnyUnverified());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "verified");
      encryptionStatusStore.getState().setStatus("ghost", "unverified");
    });
    expect(result.current).toBe(false);
  });
});

describe("useShowVerifyBanner / useShowVerifyBadge", () => {
  it("shows the banner (not the badge) when unverified and not dismissed", () => {
    const banner = renderHook(() => useShowVerifyBanner());
    const badge = renderHook(() => useShowVerifyBadge());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
    });
    expect(banner.result.current).toBe(true);
    expect(badge.result.current).toBe(false);
  });

  it("collapses to the badge (not the banner) once dismissed", () => {
    const banner = renderHook(() => useShowVerifyBanner());
    const badge = renderHook(() => useShowVerifyBadge());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
      encryptionStatusStore.getState().dismissBanner();
    });
    expect(banner.result.current).toBe(false);
    expect(badge.result.current).toBe(true);
  });

  it("clears both banner and badge once verified", () => {
    const banner = renderHook(() => useShowVerifyBanner());
    const badge = renderHook(() => useShowVerifyBadge());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
      encryptionStatusStore.getState().dismissBanner();
      encryptionStatusStore.getState().setStatus("a", "verified");
    });
    expect(banner.result.current).toBe(false);
    expect(badge.result.current).toBe(false);
  });

  it("re-surfaces the banner when a NEW device becomes unverified after a dismiss", () => {
    const banner = renderHook(() => useShowVerifyBanner());
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
      encryptionStatusStore.getState().dismissBanner();
    });
    // Dismissed → collapsed to badge, banner hidden.
    expect(banner.result.current).toBe(false);
    act(() => {
      // Account 'b' newly reports unverified — a genuinely new verification need.
      encryptionStatusStore.getState().setStatus("b", "unverified");
    });
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(false);
    expect(banner.result.current).toBe(true);
  });

  it("does not re-surface the banner on a redundant same-status batch", () => {
    const banner = renderHook(() => useShowVerifyBanner());
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
      encryptionStatusStore.getState().dismissBanner();
      // A duplicate 'unverified' batch for the same account must not un-dismiss.
      encryptionStatusStore.getState().setStatus("a", "unverified");
    });
    expect(encryptionStatusStore.getState().bannerDismissed).toBe(true);
    expect(banner.result.current).toBe(false);
  });
});

describe("useShowVerifyBadgeForAccount", () => {
  it("is scoped per account: only the unverified account's row shows the badge", () => {
    const a = renderHook(() => useShowVerifyBadgeForAccount("a"));
    const b = renderHook(() => useShowVerifyBadgeForAccount("b"));
    act(() => {
      accountsStore.getState().hydrateAll([account("a"), account("b")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
      encryptionStatusStore.getState().setStatus("b", "verified");
      encryptionStatusStore.getState().dismissBanner();
    });
    // 'a' is unverified → badge; 'b' is verified → no badge (never another
    // account's badge).
    expect(a.result.current).toBe(true);
    expect(b.result.current).toBe(false);
  });

  it("shows no badge until the banner is dismissed", () => {
    const a = renderHook(() => useShowVerifyBadgeForAccount("a"));
    act(() => {
      accountsStore.getState().hydrateAll([account("a")]);
      encryptionStatusStore.getState().setStatus("a", "unverified");
    });
    expect(a.result.current).toBe(false);
  });
});
