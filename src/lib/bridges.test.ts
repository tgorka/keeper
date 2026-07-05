import { describe, expect, it } from "vitest";
import {
  BRIDGE_LOGIN_PHASE_LABEL,
  BRIDGE_STATUS_LABEL,
  COMPANION_STACK_DOCS_URL,
} from "@/lib/bridges";
import type { BridgeLoginPhase, BridgeStatus } from "@/lib/ipc/client";

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

  it("maps every login phase to a distinct live state word", () => {
    const phases: BridgeLoginPhase[] = [
      "choosingMethod",
      "waiting",
      "qr",
      "codeEntry",
      "success",
      "failure",
    ];
    for (const phase of phases) {
      expect(BRIDGE_LOGIN_PHASE_LABEL[phase]).toBeTruthy();
    }
    // Every phase's word is distinct so states never read the same.
    const words = phases.map((p) => BRIDGE_LOGIN_PHASE_LABEL[p]);
    expect(new Set(words).size).toBe(words.length);
    expect(BRIDGE_LOGIN_PHASE_LABEL.success).toBe("Linked ✓");
  });
});
