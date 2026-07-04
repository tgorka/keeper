import { describe, expect, it } from "vitest";
import { BEEPER_HOMESERVER_HOST, isBeeperAccount } from "@/lib/beeper";
import type { AccountVm } from "@/lib/ipc/client";

function account(homeserverUrl: string): AccountVm {
  return {
    accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    userId: "@alice:beeper.com",
    homeserverUrl,
    hueIndex: 0,
  };
}

describe("isBeeperAccount", () => {
  it("is true for the Beeper homeserver", () => {
    expect(isBeeperAccount(account("https://matrix.beeper.com"))).toBe(true);
  });

  it("is true for the Beeper homeserver with a trailing slash", () => {
    expect(isBeeperAccount(account("https://matrix.beeper.com/"))).toBe(true);
  });

  it("is true for the Beeper homeserver with an explicit port", () => {
    expect(isBeeperAccount(account("https://matrix.beeper.com:443/"))).toBe(true);
  });

  it("is false for a different homeserver", () => {
    expect(isBeeperAccount(account("https://matrix.example.org/"))).toBe(false);
  });

  it("is false for a malformed URL and never throws", () => {
    expect(isBeeperAccount(account("not a url"))).toBe(false);
  });

  it("is false for an empty URL and never throws", () => {
    expect(isBeeperAccount(account(""))).toBe(false);
  });

  it("is false for a lookalike host (exact host match, not substring)", () => {
    expect(isBeeperAccount(account("https://matrix.beeper.com.evil.example"))).toBe(false);
  });

  it("exposes the Beeper homeserver host constant", () => {
    expect(BEEPER_HOMESERVER_HOST).toBe("matrix.beeper.com");
  });
});
