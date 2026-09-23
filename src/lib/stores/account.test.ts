import { beforeEach, describe, expect, it } from "vitest";
import { accountStore, NO_ACCOUNT } from "@/lib/stores/account";
import { accountVm } from "@/test/account-fixture";

beforeEach(() => {
  accountStore.setState({ vm: NO_ACCOUNT, setupLink: null });
});

describe("accountStore.setVm", () => {
  it("keeps the newest snapshot when an older one arrives after it", () => {
    const { setVm } = accountStore.getState();
    setVm(accountVm({ state: "syncing", sentence: "Syncing your settings…", revision: 7 }));
    // A command's answer composed before that push, landing after it.
    setVm(
      accountVm({
        state: "signingIn",
        sentence: "Finish signing in to Acme in the browser.",
        revision: 6,
      }),
    );
    expect(accountStore.getState().vm.state).toBe("syncing");

    setVm(accountVm({ state: "ready", sentence: "Up to date.", revision: 8 }));
    expect(accountStore.getState().vm.state).toBe("ready");
  });

  it("takes Rust's first snapshot even when its counter starts where the default sits", () => {
    // The store boots holding NO_ACCOUNT at revision 0; a process whose first
    // snapshot is also revision 0 must still be mirrored.
    accountStore.getState().setVm(accountVm({ revision: 0 }));
    expect(accountStore.getState().vm.configured).toBe(true);
  });
});
