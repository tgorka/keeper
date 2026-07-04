import { describe, expect, it } from "vitest";
import { isBeeperAccount } from "@/lib/beeper";
import type { AccountVm, Provider } from "@/lib/ipc/client";

function account(provider: Provider): AccountVm {
  return {
    accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    userId: "@alice:beeper.com",
    homeserverUrl: "https://matrix.beeper.com/",
    hueIndex: 0,
    provider,
  };
}

describe("isBeeperAccount", () => {
  it("is true for a Beeper-provider account", () => {
    expect(isBeeperAccount(account("beeper"))).toBe(true);
  });

  it("is false for a password-provider account", () => {
    expect(isBeeperAccount(account("password"))).toBe(false);
  });

  it("is false for an oidc-provider account", () => {
    expect(isBeeperAccount(account("oidc"))).toBe(false);
  });

  it("keys off the provider tag, not the homeserver host", () => {
    // A non-Beeper provider resolved onto the Beeper host is still not Beeper.
    expect(
      isBeeperAccount({
        accountId: "x",
        userId: "@x:beeper.com",
        homeserverUrl: "https://matrix.beeper.com/",
        hueIndex: 0,
        provider: "password",
      }),
    ).toBe(false);
    // A Beeper provider on any host is Beeper.
    expect(
      isBeeperAccount({
        accountId: "y",
        userId: "@y:example.org",
        homeserverUrl: "https://matrix.example.org/",
        hueIndex: 0,
        provider: "beeper",
      }),
    ).toBe(true);
  });
});
