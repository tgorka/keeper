import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VerificationFlowVm } from "@/lib/ipc/client";

// Mock every IPC action the modal can call so nothing touches Tauri.
const verificationStart = vi.fn((_a: string) => Promise.resolve());
const verificationStartSas = vi.fn((_a: string, _f: string) => Promise.resolve());
const verificationConfirm = vi.fn((_a: string, _f: string) => Promise.resolve());
const verificationMismatch = vi.fn((_a: string, _f: string) => Promise.resolve());
const verificationCancel = vi.fn((_a: string, _f: string) => Promise.resolve());
vi.mock("@/lib/ipc/client", () => ({
  verificationStart: (a: string) => verificationStart(a),
  verificationStartSas: (a: string, f: string) => verificationStartSas(a, f),
  verificationConfirm: (a: string, f: string) => verificationConfirm(a, f),
  verificationMismatch: (a: string, f: string) => verificationMismatch(a, f),
  verificationCancel: (a: string, f: string) => verificationCancel(a, f),
}));

import { DeviceVerificationDialog } from "@/components/settings/device-verification-dialog";
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

function openWith(accountId: string, f: VerificationFlowVm | null) {
  verificationStore.setState({ modalOpen: true, activeAccountId: accountId, flow: f });
}

beforeEach(() => {
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
  verificationStart.mockClear();
  verificationStartSas.mockClear();
  verificationConfirm.mockClear();
  verificationMismatch.mockClear();
  verificationCancel.mockClear();
});

afterEach(() => {
  verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
});

describe("DeviceVerificationDialog", () => {
  it("renders nothing visible when closed", () => {
    render(<DeviceVerificationDialog />);
    expect(screen.queryByText("Verify this device")).not.toBeInTheDocument();
  });

  it("kicks off a keeper-started verification when opened with no flow", () => {
    openWith("acc-1", null);
    render(<DeviceVerificationDialog />);
    expect(verificationStart).toHaveBeenCalledWith("acc-1");
  });

  it("surfaces a failed state (not a hang) when the keeper-started request rejects", async () => {
    verificationStart.mockRejectedValueOnce(new Error("no other session"));
    openWith("acc-1", null);
    render(<DeviceVerificationDialog />);
    expect(await screen.findByText(/couldn't start verification/i)).toBeInTheDocument();
    expect(verificationStore.getState().flow?.phase).toBe("failed");
  });

  it("shows keeper's QR and a 'Verify with emoji' action in the ready phase", () => {
    openWith("acc-1", flow({ phase: "ready", qrCodeSvg: "<svg><rect/></svg>" }));
    render(<DeviceVerificationDialog />);
    expect(screen.getByAltText(/scan this qr code/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Verify with emoji" }));
    expect(verificationStartSas).toHaveBeenCalledWith("acc-1", "$flow1");
  });

  it("renders the 7 emoji and wires the match / no-match buttons", () => {
    const emojis = Array.from({ length: 7 }, (_, i) => ({
      symbol: "🐶",
      name: `Dog${i}`,
    }));
    openWith("acc-1", flow({ phase: "comparing", emojis, flowId: "$cmp" }));
    render(<DeviceVerificationDialog />);

    expect(screen.getByText("Dog0")).toBeInTheDocument();
    expect(screen.getByText("Dog6")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "They match" }));
    expect(verificationConfirm).toHaveBeenCalledWith("acc-1", "$cmp");

    fireEvent.click(screen.getByRole("button", { name: "They don't match" }));
    expect(verificationMismatch).toHaveBeenCalledWith("acc-1", "$cmp");
  });

  it("shows a success message in the done phase", () => {
    openWith("acc-1", flow({ phase: "done" }));
    render(<DeviceVerificationDialog />);
    expect(screen.getByText(/now verified/i)).toBeInTheDocument();
  });

  it("renders cancelled and failed distinctly", () => {
    openWith("acc-1", flow({ phase: "cancelled" }));
    const { unmount } = render(<DeviceVerificationDialog />);
    expect(screen.getByText(/verification cancelled/i)).toBeInTheDocument();
    expect(screen.queryByText(/verification failed/i)).not.toBeInTheDocument();
    unmount();

    openWith("acc-1", flow({ phase: "failed", reason: "The expected key did not match" }));
    render(<DeviceVerificationDialog />);
    expect(
      screen.getByText(/verification failed: the expected key did not match/i),
    ).toBeInTheDocument();
  });

  it("closing the modal (Esc) cancels the active flow", () => {
    openWith("acc-1", flow({ phase: "comparing", flowId: "$esc" }));
    render(<DeviceVerificationDialog />);
    // Radix Dialog closes on Escape; the store's close() fires the cancel.
    fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
    expect(verificationCancel).toHaveBeenCalledWith("acc-1", "$esc");
    expect(verificationStore.getState().modalOpen).toBe(false);
  });
});
