import { beforeEach, describe, expect, it, vi } from "vitest";

// Mock the IPC client so refreshIncognito can be driven deterministically.
const incognitoGet = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  incognitoGet: (accountId: string, roomId: string) => incognitoGet(accountId, roomId),
}));

import type { IncognitoVm } from "@/lib/ipc/gen/IncognitoVm";
import { incognitoStore, refreshIncognito } from "./incognito";

function vm(over: Partial<IncognitoVm> = {}): IncognitoVm {
  return {
    effective: false,
    source: "global",
    global: false,
    account: null,
    chat: null,
    ...over,
  };
}

describe("incognitoStore", () => {
  beforeEach(() => {
    incognitoStore.getState().clear();
    incognitoGet.mockReset();
  });

  it("mirrors a VM per (account, room) and derives global + per-account scopes", () => {
    incognitoStore
      .getState()
      .applyVm(
        "acctA",
        "!r1",
        vm({ effective: true, source: "chat", global: true, account: false, chat: true }),
      );
    const state = incognitoStore.getState();
    expect(state.byChat.get("acctA !r1")?.effective).toBe(true);
    expect(state.byChat.get("acctA !r1")?.source).toBe("chat");
    expect(state.global).toBe(true);
    expect(state.byAccount.get("acctA")).toBe(false);
  });

  it("keeps distinct chats independent", () => {
    incognitoStore.getState().applyVm("acctA", "!r1", vm({ effective: true }));
    incognitoStore.getState().applyVm("acctA", "!r2", vm({ effective: false }));
    expect(incognitoStore.getState().byChat.get("acctA !r1")?.effective).toBe(true);
    expect(incognitoStore.getState().byChat.get("acctA !r2")?.effective).toBe(false);
  });

  it("applyGlobal updates only the global bool without churning identity when unchanged", () => {
    incognitoStore.getState().applyGlobal(true);
    expect(incognitoStore.getState().global).toBe(true);
    const before = incognitoStore.getState();
    incognitoStore.getState().applyGlobal(true);
    expect(incognitoStore.getState()).toBe(before);
  });

  it("refreshIncognito reads the authoritative VM into the mirror", async () => {
    incognitoGet.mockResolvedValue(vm({ effective: true, source: "account", account: true }));
    await refreshIncognito("acctA", "!r1");
    expect(incognitoGet).toHaveBeenCalledWith("acctA", "!r1");
    expect(incognitoStore.getState().byChat.get("acctA !r1")?.effective).toBe(true);
    expect(incognitoStore.getState().byChat.get("acctA !r1")?.source).toBe("account");
  });

  it("refreshIncognito swallows a read failure and leaves the mirror untouched", async () => {
    incognitoStore.getState().applyVm("acctA", "!r1", vm({ effective: true }));
    incognitoGet.mockRejectedValue(new Error("boom"));
    await expect(refreshIncognito("acctA", "!r1")).resolves.toBeUndefined();
    // Last-observed VM stays in place rather than flashing a wrong state.
    expect(incognitoStore.getState().byChat.get("acctA !r1")?.effective).toBe(true);
  });

  it("bumpPolicyVersion increments monotonically to re-trigger open-chat reads", () => {
    const start = incognitoStore.getState().policyVersion;
    incognitoStore.getState().bumpPolicyVersion();
    incognitoStore.getState().bumpPolicyVersion();
    expect(incognitoStore.getState().policyVersion).toBe(start + 2);
  });

  it("refreshIncognito drops a read that resolved after it was cancelled", async () => {
    incognitoStore.getState().applyVm("acctA", "!r1", vm({ effective: true }));
    incognitoGet.mockResolvedValue(vm({ effective: false }));
    // Cancelled before the read applies (e.g. a fast room switch): the stale VM must
    // not clobber the newer selection's mirrored state.
    await refreshIncognito("acctA", "!r1", () => true);
    expect(incognitoStore.getState().byChat.get("acctA !r1")?.effective).toBe(true);
  });

  it("clear resets to the empty state", () => {
    incognitoStore.getState().applyVm("acctA", "!r1", vm({ effective: true, global: true }));
    incognitoStore.getState().clear();
    expect(incognitoStore.getState().byChat.size).toBe(0);
    expect(incognitoStore.getState().global).toBe(false);
    expect(incognitoStore.getState().byAccount.size).toBe(0);
  });
});
