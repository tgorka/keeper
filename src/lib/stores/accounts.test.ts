import { beforeEach, describe, expect, it } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

describe("accountsStore", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    // `clear()` deliberately preserves `hydrated`; reset it here so each test
    // starts from the un-hydrated boot state regardless of run order.
    accountsStore.setState({ hydrated: false });
  });

  it("starts with no current account", () => {
    expect(accountsStore.getState().currentAccount).toBeNull();
  });

  it("records the account on setCurrentAccount", () => {
    accountsStore.getState().setCurrentAccount(account);
    expect(accountsStore.getState().currentAccount).toEqual(account);
  });

  it("clears the account on clear", () => {
    accountsStore.getState().setCurrentAccount(account);
    accountsStore.getState().clear();
    expect(accountsStore.getState().currentAccount).toBeNull();
  });

  it("holds no token/session fields on the stored account", () => {
    accountsStore.getState().setCurrentAccount(account);
    const stored = accountsStore.getState().currentAccount;
    expect(stored).not.toBeNull();
    expect(JSON.stringify(stored)).not.toContain("token");
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
    accountsStore.getState().setCurrentAccount(account);
    accountsStore.getState().clear();
    expect(accountsStore.getState().hydrated).toBe(true);
  });
});
