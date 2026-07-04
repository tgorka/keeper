import { beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
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

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV", 0);
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ", 1);

describe("accountsStore", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    // `clear()` deliberately preserves `hydrated`; reset it here so each test
    // starts from the un-hydrated boot state regardless of run order.
    accountsStore.setState({ hydrated: false });
  });

  it("starts with no accounts", () => {
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("adds an account on addAccount", () => {
    accountsStore.getState().addAccount(alice);
    expect(accountsStore.getState().accounts).toEqual([alice]);
  });

  it("adds a second account without dropping the first (no cap)", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount(bob);
    expect(accountsStore.getState().accounts.map((a) => a.accountId)).toEqual([
      alice.accountId,
      bob.accountId,
    ]);
  });

  it("upserts by accountId (re-login never duplicates a row)", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount({ ...alice, hueIndex: 5 });
    const accounts = accountsStore.getState().accounts;
    expect(accounts).toHaveLength(1);
    expect(accounts[0].hueIndex).toBe(5);
  });

  it("removes one account, keeping the others", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount(bob);
    accountsStore.getState().removeAccount(alice.accountId);
    expect(accountsStore.getState().accounts.map((a) => a.accountId)).toEqual([bob.accountId]);
  });

  it("hydrateAll replaces the account set", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().hydrateAll([bob]);
    expect(accountsStore.getState().accounts).toEqual([bob]);
  });

  it("clears all accounts on clear", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount(bob);
    accountsStore.getState().clear();
    expect(accountsStore.getState().accounts).toEqual([]);
  });

  it("holds no token/session fields on stored accounts", () => {
    accountsStore.getState().addAccount(alice);
    expect(JSON.stringify(accountsStore.getState().accounts)).not.toContain("token");
  });

  it("carries the per-account hue index", () => {
    accountsStore.getState().addAccount(account("x", 4));
    expect(accountsStore.getState().accounts[0].hueIndex).toBe(4);
  });

  it("starts un-hydrated (splash gate closed)", () => {
    expect(accountsStore.getState().hydrated).toBe(false);
  });

  it("marks hydrated on markHydrated", () => {
    accountsStore.getState().markHydrated();
    expect(accountsStore.getState().hydrated).toBe(true);
  });

  it("keeps hydrated across a clear (sign-out does not reopen the splash)", () => {
    accountsStore.getState().markHydrated();
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().clear();
    expect(accountsStore.getState().hydrated).toBe(true);
  });
});

describe("accountsStore inbox filter", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ filterAccountId: null });
  });

  it("starts with no filter", () => {
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  it("toggleFilter sets the filter to the clicked account", () => {
    accountsStore.getState().toggleFilter(alice.accountId);
    expect(accountsStore.getState().filterAccountId).toBe(alice.accountId);
  });

  it("toggleFilter on the active account clears the filter", () => {
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().toggleFilter(alice.accountId);
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  it("toggleFilter on a different account switches the filter", () => {
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().toggleFilter(bob.accountId);
    expect(accountsStore.getState().filterAccountId).toBe(bob.accountId);
  });

  it("removeAccount clears the filter when the filtered account is removed", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount(bob);
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().removeAccount(alice.accountId);
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  it("removeAccount keeps the filter when a different account is removed", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().addAccount(bob);
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().removeAccount(bob.accountId);
    expect(accountsStore.getState().filterAccountId).toBe(alice.accountId);
  });

  it("hydrateAll clears any stale filter", () => {
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().hydrateAll([bob]);
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });

  it("addAccount clears an active filter so the new account is not hidden", () => {
    accountsStore.getState().addAccount(alice);
    accountsStore.getState().toggleFilter(alice.accountId);
    accountsStore.getState().addAccount(bob);
    expect(accountsStore.getState().filterAccountId).toBeNull();
  });
});
