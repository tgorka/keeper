import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BbctlRunSheet } from "@/components/bridges/bbctl-run-sheet";
import type { BbctlProgressVm } from "@/lib/ipc/client";

// Drive the Sheet through the hook seam: the mock lets each test hand the Sheet a
// specific phase VM and capture the start/cancel calls the Sheet makes.
const hookState = {
  vm: null as BbctlProgressVm | null,
  start: vi.fn(),
  cancel: vi.fn(),
};

vi.mock("@/hooks/use-bbctl-run", () => ({
  useBbctlRun: () => hookState,
}));

const ACCOUNT_ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function baseVm(overrides: Partial<BbctlProgressVm>): BbctlProgressVm {
  return {
    networkId: "signal",
    phase: "checking",
    message: null,
    error: null,
    ...overrides,
  };
}

function renderSheet(onOpenChange = vi.fn(), onSuccess = vi.fn()) {
  render(
    <BbctlRunSheet
      accountId={ACCOUNT_ID}
      networkId="signal"
      networkName="Signal"
      open={true}
      onOpenChange={onOpenChange}
      onSuccess={onSuccess}
    />,
  );
  return { onOpenChange, onSuccess };
}

beforeEach(() => {
  hookState.vm = null;
  hookState.start.mockClear();
  hookState.cancel.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("BbctlRunSheet", () => {
  it("starts the run when opened", () => {
    renderSheet();
    expect(hookState.start).toHaveBeenCalled();
  });

  it("renders the phase step word for an in-flight phase", () => {
    hookState.vm = baseVm({ phase: "registering" });
    renderSheet();
    // The step word shows in the Sheet description slot (the progress panel falls
    // back to the same label, so target the state-word slot specifically).
    const stateWord = document.querySelector('[data-slot="bbctl-run-state-word"]');
    expect(stateWord).toHaveTextContent("Registering bridge");
  });

  it("shows only the phase label (log-free) for a non-terminal phase", () => {
    // Even if a raw bbctl line slipped into `message`, the log-free stepper must
    // render the phase LABEL only — no raw bbctl output ever reaches the UI.
    hookState.vm = baseVm({ phase: "starting", message: "Starting bridge sh-signal" });
    renderSheet();
    const progress = document.querySelector('[data-slot="bbctl-run-progress"]');
    expect(progress).toHaveTextContent("Starting bridge");
    expect(screen.queryByText("Starting bridge sh-signal")).not.toBeInTheDocument();
  });

  it("fires onSuccess exactly once and auto-closes on success", async () => {
    vi.useFakeTimers();
    hookState.vm = baseVm({ phase: "success" });
    const { onOpenChange, onSuccess } = renderSheet();
    // Fired once immediately on the success phase.
    expect(onSuccess).toHaveBeenCalledTimes(1);
    // Auto-close after the advance delay.
    vi.advanceTimersByTime(1500);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("does not strand the sheet when the success refresh throws", () => {
    vi.useFakeTimers();
    hookState.vm = baseVm({ phase: "success" });
    const onSuccess = vi.fn(() => {
      throw new Error("refresh boom");
    });
    const onOpenChange = vi.fn();
    render(
      <BbctlRunSheet
        accountId={ACCOUNT_ID}
        networkId="signal"
        networkName="Signal"
        open={true}
        onOpenChange={onOpenChange}
        onSuccess={onSuccess}
      />,
    );
    // Even though the refresh threw, the auto-close still fires.
    vi.advanceTimersByTime(1500);
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("renders the verbatim error and a Retry on failure", () => {
    hookState.vm = baseVm({
      phase: "failure",
      error: "bbctl: could not reach the appservice",
    });
    renderSheet();
    expect(screen.getByText("bbctl: could not reach the appservice")).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: "Retry" });
    retry.click();
    expect(hookState.start).toHaveBeenCalled();
  });

  it("cancels the run when the sheet is closed", () => {
    const onOpenChange = vi.fn();
    renderSheet(onOpenChange);
    expect(hookState.cancel).not.toHaveBeenCalled();
    // Closing via the Sheet's close affordance routes through handleOpenChange(false)
    // → the hook's cancel (tearing down keeper's streaming session).
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(hookState.cancel).toHaveBeenCalled();
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
