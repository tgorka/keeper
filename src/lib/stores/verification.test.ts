import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VerificationFlowVm } from "@/lib/ipc/client";

// Mock the IPC wrapper so `close()`'s best-effort cancel never touches Tauri.
const verificationCancel = vi.fn((_a: string, _f: string) => Promise.resolve());
vi.mock("@/lib/ipc/client", () => ({
  verificationCancel: (accountId: string, flowId: string) => verificationCancel(accountId, flowId),
}));

import { verificationStore } from "@/lib/stores/verification";

function flow(overrides: Partial<VerificationFlowVm> = {}): VerificationFlowVm {
  return {
    flowId: "$flow1",
    phase: "comparing",
    emojis: null,
    qrCodeSvg: null,
    reason: null,
    ...overrides,
  };
}

beforeEach(() => {
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
  verificationCancel.mockClear();
});

afterEach(() => {
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
});

describe("verificationStore", () => {
  it("openFor opens the modal for an account and clears any prior flow", () => {
    verificationStore.getState().setFlow(flow());
    verificationStore.getState().openFor("acc-1");
    const s = verificationStore.getState();
    expect(s.modalOpen).toBe(true);
    expect(s.activeAccountId).toBe("acc-1");
    expect(s.flow).toBeNull();
  });

  it("setFlow records the streamed snapshot", () => {
    verificationStore.getState().openFor("acc-1");
    verificationStore.getState().setFlow(flow({ phase: "ready" }));
    expect(verificationStore.getState().flow?.phase).toBe("ready");
  });

  it("close cancels a non-terminal flow and clears state", () => {
    verificationStore.getState().openFor("acc-1");
    verificationStore.getState().setFlow(flow({ phase: "comparing", flowId: "$abc" }));
    verificationStore.getState().close();
    expect(verificationCancel).toHaveBeenCalledWith("acc-1", "$abc");
    const s = verificationStore.getState();
    expect(s.modalOpen).toBe(false);
    expect(s.activeAccountId).toBeNull();
    expect(s.flow).toBeNull();
  });

  it("close does NOT cancel a terminal flow (done/cancelled/failed)", () => {
    for (const phase of ["done", "cancelled", "failed"] as const) {
      verificationStore.getState().openFor("acc-1");
      verificationStore.getState().setFlow(flow({ phase }));
      verificationStore.getState().close();
    }
    expect(verificationCancel).not.toHaveBeenCalled();
  });

  it("close does NOT cancel a confirmed flow (our SAS confirmation is already sent)", () => {
    verificationStore.getState().openFor("acc-1");
    verificationStore.getState().setFlow(flow({ phase: "confirmed", flowId: "$xyz" }));
    verificationStore.getState().close();
    expect(verificationCancel).not.toHaveBeenCalled();
    expect(verificationStore.getState().modalOpen).toBe(false);
  });

  it("close with no active flow does not attempt a cancel", () => {
    verificationStore.getState().openFor("acc-1");
    verificationStore.getState().close();
    expect(verificationCancel).not.toHaveBeenCalled();
    expect(verificationStore.getState().modalOpen).toBe(false);
  });
});
