import { describe, expect, it } from "vitest";
import { BRIDGE_STATUS_LABEL, COMPANION_STACK_DOCS_URL } from "@/lib/bridges";
import type { BridgeStatus } from "@/lib/ipc/client";

describe("bridges helpers", () => {
  it("points the companion-stack docs link at the real repo docs (never a fabricated host)", () => {
    expect(COMPANION_STACK_DOCS_URL).toBe("https://github.com/tgorka/keeper/tree/main/docs");
    expect(COMPANION_STACK_DOCS_URL.startsWith("https://github.com/tgorka/keeper")).toBe(true);
  });

  it("maps every discovery status to an honest label", () => {
    const statuses: BridgeStatus[] = ["loggedIn", "notLoggedIn", "configured"];
    for (const status of statuses) {
      expect(BRIDGE_STATUS_LABEL[status]).toBeTruthy();
    }
    expect(BRIDGE_STATUS_LABEL.loggedIn).toBe("Connected");
    expect(BRIDGE_STATUS_LABEL.notLoggedIn).toBe("Action needed");
    expect(BRIDGE_STATUS_LABEL.configured).toBe("Not set up");
  });
});
