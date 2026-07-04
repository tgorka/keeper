import { beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

function account(id: string, hue = 0): AccountVm {
  return {
    accountId: id,
    userId: `@user-${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
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
