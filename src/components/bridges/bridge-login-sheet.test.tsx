import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeLoginSheet } from "@/components/bridges/bridge-login-sheet";
import type { BridgeLoginVm } from "@/lib/ipc/client";

// Drive the Sheet through the hook seam: the mock lets each test hand the Sheet a
// specific phase VM and capture the start/submit/cancel calls the Sheet makes.
const hookState = {
  vm: null as BridgeLoginVm | null,
  start: vi.fn(),
  submit: vi.fn(),
  cancel: vi.fn(),
};

vi.mock("@/hooks/use-bridge-login", () => ({
  useBridgeLogin: () => hookState,
}));

const ACCOUNT_ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function baseVm(overrides: Partial<BridgeLoginVm>): BridgeLoginVm {
  return {
    networkId: "whatsapp",
    phase: "waiting",
    instruction: null,
    qrSvg: null,
    qrRefreshed: false,
    fields: [],
    flows: [],
    error: null,
    ...overrides,
  };
}

function renderSheet(onOpenChange = vi.fn()) {
  return render(
    <BridgeLoginSheet
      accountId={ACCOUNT_ID}
      networkId="whatsapp"
      networkName="WhatsApp"
      open={true}
      onOpenChange={onOpenChange}
    />,
  );
}

beforeEach(() => {
  hookState.vm = null;
  hookState.start.mockClear();
  hookState.submit.mockClear();
  hookState.cancel.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("BridgeLoginSheet", () => {
  it("starts the login when opened", () => {
    renderSheet();
    expect(hookState.start).toHaveBeenCalled();
  });

  it("choosingMethod renders a RadioGroup of flows and submits the choice", () => {
    hookState.vm = baseVm({
      phase: "choosingMethod",
      instruction: "Choose how to sign in.",
      flows: [
        { id: "qr", name: "QR code", description: null },
        { id: "phone", name: "Phone number", description: "SMS code" },
      ],
    });
    renderSheet();
    expect(screen.getByText("QR code")).toBeInTheDocument();
    expect(screen.getByText("Phone number")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    expect(hookState.submit).toHaveBeenCalledWith({ kind: "chooseFlow", flowId: "qr" });
  });

  it("qr renders the SVG as a data-uri image on a white card with the instruction", () => {
    hookState.vm = baseVm({
      phase: "qr",
      instruction: "Scan this QR code with WhatsApp on your phone.",
      qrSvg: "<svg><rect/></svg>",
    });
    renderSheet();
    const img = screen.getByRole("img", { name: /Scan this QR code with WhatsApp/ });
    expect(img.getAttribute("src")).toContain("data:image/svg+xml,");
    expect(screen.getByText("Scan this QR code with WhatsApp on your phone.")).toBeInTheDocument();
    expect(screen.getByText("Scan QR")).toBeInTheDocument();
  });

  it("qr refresh shows the 'QR refreshed' note", () => {
    hookState.vm = baseVm({ phase: "qr", qrSvg: "<svg/>", qrRefreshed: true });
    renderSheet();
    expect(screen.getByText("QR refreshed")).toBeInTheDocument();
  });

  it("codeEntry renders labeled fields, pattern-gates submit, and submits values", () => {
    hookState.vm = baseVm({
      phase: "codeEntry",
      instruction: "Enter the code.",
      fields: [
        {
          id: "2fa_code",
          fieldType: "2fa_code",
          name: "Verification code",
          description: "The 6-digit code",
          pattern: "^[0-9]{6}$",
          defaultValue: null,
        },
      ],
    });
    renderSheet();
    expect(screen.getByText("Verification code")).toBeInTheDocument();

    const input = screen.getByLabelText(/Verification code/);
    const submit = screen.getByRole("button", { name: "Submit" });

    // An invalid (too-short) value keeps submit disabled (client-side pattern gate).
    fireEvent.change(input, { target: { value: "123" } });
    expect(submit).toBeDisabled();

    // A valid value enables it and submits the body keyed by the field id.
    fireEvent.change(input, { target: { value: "123456" } });
    expect(submit).not.toBeDisabled();
    fireEvent.click(submit);
    expect(hookState.submit).toHaveBeenCalledWith({
      kind: "fields",
      values: { "2fa_code": "123456" },
    });
  });

  it("codeEntry anchors the pattern: rejects a trailing char, accepts the exact match", () => {
    // A `[0-9]{6}` pattern must full-string match — an unanchored regex would
    // substring-match and wrongly accept "123456x".
    hookState.vm = baseVm({
      phase: "codeEntry",
      fields: [
        {
          id: "code",
          fieldType: "text",
          name: "Code",
          description: null,
          pattern: "[0-9]{6}",
          defaultValue: null,
        },
      ],
    });
    renderSheet();
    const input = screen.getByLabelText(/Code/);
    const submit = screen.getByRole("button", { name: "Submit" });

    // Previously accepted (substring match) — must now be rejected.
    fireEvent.change(input, { target: { value: "123456x" } });
    expect(submit).toBeDisabled();

    // The exact full-string match is accepted.
    fireEvent.change(input, { target: { value: "123456" } });
    expect(submit).not.toBeDisabled();
  });

  it("codeEntry resets field state when a second step carries different fields", () => {
    // First step: a single "phone" field the user fills in.
    hookState.vm = baseVm({
      phase: "codeEntry",
      fields: [
        {
          id: "phone",
          fieldType: "text",
          name: "Phone",
          description: null,
          pattern: null,
          defaultValue: null,
        },
      ],
    });
    const { rerender } = renderSheet();
    fireEvent.change(screen.getByLabelText(/Phone/), { target: { value: "555-0100" } });
    expect((screen.getByLabelText(/Phone/) as HTMLInputElement).value).toBe("555-0100");

    // Second step: a different field set with its own default — the panel must
    // remount (keyed by field ids), resetting to the new default, not the stale
    // "phone" value.
    hookState.vm = baseVm({
      phase: "codeEntry",
      fields: [
        {
          id: "code",
          fieldType: "text",
          name: "Code",
          description: null,
          pattern: null,
          defaultValue: "prefilled",
        },
      ],
    });
    rerender(
      <BridgeLoginSheet
        accountId={ACCOUNT_ID}
        networkId="whatsapp"
        networkName="WhatsApp"
        open={true}
        onOpenChange={vi.fn()}
      />,
    );
    // The old field is gone and the new one shows its default (not a stale value).
    expect(screen.queryByLabelText(/Phone/)).toBeNull();
    expect((screen.getByLabelText(/Code/) as HTMLInputElement).value).toBe("prefilled");
  });

  it("failure renders the bridge's error message verbatim with a Retry", () => {
    hookState.vm = baseVm({
      phase: "failure",
      error: "M_FORBIDDEN: this account is already linked",
    });
    renderSheet();
    expect(screen.getByText("M_FORBIDDEN: this account is already linked")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    // Retry re-starts the login (start is called on open + on retry).
    expect(hookState.start.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it("the unsupported-method failure names the Bridge Bot chat honestly (no webview)", () => {
    hookState.vm = baseVm({
      phase: "failure",
      error:
        "This network needs browser sign-in, which keeper can't do natively yet. You can still log in from the Bridge Bot chat.",
    });
    renderSheet();
    expect(screen.getByText(/Bridge Bot chat/)).toBeInTheDocument();
    // No embedded webview / iframe is ever rendered for an unsupported method.
    expect(document.querySelector("iframe")).toBeNull();
  });

  it("success shows 'Linked ✓' in green and auto-advances the Sheet closed", () => {
    vi.useFakeTimers();
    const onOpenChange = vi.fn();
    hookState.vm = baseVm({ phase: "success" });
    renderSheet(onOpenChange);
    // The success panel shows the green "Linked ✓" headline (the SheetDescription
    // state word carries the same phrase, so scope to the success panel).
    const panel = document.querySelector('[data-slot="bridge-login-success"]');
    expect(panel).not.toBeNull();
    expect(panel).toHaveTextContent("Linked ✓");

    vi.advanceTimersByTime(1500);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("closing the Sheet cancels the running login", async () => {
    hookState.vm = baseVm({ phase: "qr", qrSvg: "<svg/>" });
    renderSheet();
    // Esc closes the Sheet (Radix Dialog), triggering our cancel.
    fireEvent.keyDown(document.body, { key: "Escape", code: "Escape" });
    await waitFor(() => {
      expect(hookState.cancel).toHaveBeenCalled();
    });
  });
});
