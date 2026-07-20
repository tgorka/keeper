import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  ActiveRecordingBanner,
  BANNER_DISMISS_LABEL,
  BANNER_RESTART_LABEL,
  BANNER_STOP_LABEL,
  BANNER_STOPPING_LABEL,
} from "@/components/recording/active-recording-banner";
import type { RecordingStatusVm } from "@/lib/ipc/client";

/**
 * Mock `matchMedia` so `(prefers-reduced-motion: reduce)` reports `reduced`
 * (default: not reduced, so the dot pulses).
 */
const originalMatchMedia = window.matchMedia;
function mockReducedMotion(reduced: boolean) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes("prefers-reduced-motion") ? reduced : false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

const LIVE: RecordingStatusVm = {
  state: "recording",
  segmentsClosed: 2,
  startedAtEpochMs: 1_700_000_000_000,
  outputPath: "/Users/alice/Movies/keeper/session",
  error: null,
  warning: null,
  onDiskBytes: 412_000_000,
  currentSegmentBytes: 250_000_000,
  segmentCapMb: 500,
};

/** The sticky mic-loss warning message the sidecar emits (Story 19.4). */
const MIC_WARNING = "microphone disconnected — using system default input";

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  vi.restoreAllMocks();
});

function renderBanner(overrides: Partial<RecordingStatusVm> = {}, elapsed = "12:34") {
  const onStop = vi.fn();
  const onRestart = vi.fn();
  const onDismiss = vi.fn();
  const status: RecordingStatusVm = { ...LIVE, ...overrides };
  const view = render(
    <ActiveRecordingBanner
      status={status}
      elapsed={elapsed}
      onStop={onStop}
      onRestart={onRestart}
      onDismiss={onDismiss}
    />,
  );
  return { onStop, onRestart, onDismiss, ...view };
}

describe("ActiveRecordingBanner", () => {
  it("renders the live line: elapsed · segment · size (segment = closed + 1)", () => {
    mockReducedMotion(false);
    renderBanner({ segmentsClosed: 2, onDiskBytes: 412_000_000 }, "12:34");
    // 2 closed ⇒ segment 3; 412_000_000 bytes ⇒ "412 MB".
    expect(screen.getByText(/12:34 · segment 3 · 412 MB/)).toBeInTheDocument();
    expect(screen.getByText("Recording")).toBeInTheDocument();
  });

  it("renders nothing on any terminal/idle state", () => {
    mockReducedMotion(false);
    for (const state of ["idle", "finalized", "recovered", "failed"] as const) {
      const { container } = render(
        <ActiveRecordingBanner
          status={{ ...LIVE, state }}
          elapsed="1:00"
          onStop={vi.fn()}
          onRestart={vi.fn()}
          onDismiss={vi.fn()}
        />,
      );
      expect(container).toBeEmptyDOMElement();
    }
  });

  it("shows for every live state", () => {
    mockReducedMotion(false);
    for (const state of ["preflight", "recording", "rotating", "stopping"] as const) {
      const { unmount } = render(
        <ActiveRecordingBanner
          status={{ ...LIVE, state }}
          elapsed="1:00"
          onStop={vi.fn()}
          onRestart={vi.fn()}
          onDismiss={vi.fn()}
        />,
      );
      expect(screen.getByText("Recording")).toBeInTheDocument();
      unmount();
    }
  });

  it("fills the meter proportionally with a used / cap MB caption", () => {
    mockReducedMotion(false);
    renderBanner({ segmentsClosed: 2, currentSegmentBytes: 250_000_000, segmentCapMb: 500 });
    const meter = screen.getByRole("progressbar", { name: "Segment size" });
    expect(meter).toHaveAttribute("aria-valuenow", "250");
    expect(meter).toHaveAttribute("aria-valuemax", "500");
    expect(screen.getByText("segment 3 · 250 / 500 MB")).toBeInTheDocument();
    // 250 / 500 ⇒ 50% fill.
    const fill = meter.firstElementChild as HTMLElement;
    expect(fill.style.width).toBe("50%");
  });

  it("renders a valid empty meter at a fresh rotation (0 bytes, cap > 0)", () => {
    // The moment a new segment file opens, current bytes fall to ~0 — the bar
    // must render a clean 0% (never NaN) and the caption `0 / cap`.
    mockReducedMotion(false);
    renderBanner({ segmentsClosed: 2, currentSegmentBytes: 0, segmentCapMb: 500 });
    const meter = screen.getByRole("progressbar", { name: "Segment size" });
    const fill = meter.firstElementChild as HTMLElement;
    expect(fill.style.width).toBe("0%");
    expect(meter).toHaveAttribute("aria-valuenow", "0");
    expect(screen.getByText("segment 3 · 0 / 500 MB")).toBeInTheDocument();
  });

  it("clamps the meter to 100% when the current segment exceeds the cap", () => {
    mockReducedMotion(false);
    renderBanner({ currentSegmentBytes: 520_000_000, segmentCapMb: 500 });
    const meter = screen.getByRole("progressbar", { name: "Segment size" });
    const fill = meter.firstElementChild as HTMLElement;
    expect(fill.style.width).toBe("100%");
    // The caption still shows the honest over-cap figure.
    expect(screen.getByText(/520 \/ 500 MB/)).toBeInTheDocument();
  });

  it("hides the meter when the session cap is 0 (defensive, no NaN fraction)", () => {
    mockReducedMotion(false);
    renderBanner({ segmentCapMb: 0 });
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("keeps the record dot steady (no animate-pulse) under reduced motion", () => {
    mockReducedMotion(true);
    renderBanner();
    const dot = screen.getByTestId("recording-dot");
    expect(dot).not.toHaveClass("animate-pulse");
  });

  it("pulses the record dot when reduced motion is not preferred", () => {
    mockReducedMotion(false);
    renderBanner();
    const dot = screen.getByTestId("recording-dot");
    expect(dot).toHaveClass("animate-pulse");
  });

  it("Stop calls onStop while not stopping", () => {
    mockReducedMotion(false);
    const { onStop } = renderBanner();
    fireEvent.click(screen.getByRole("button", { name: BANNER_STOP_LABEL }));
    expect(onStop).toHaveBeenCalledTimes(1);
  });

  it("Stop is disabled and labelled Stopping… while stopping", () => {
    mockReducedMotion(false);
    renderBanner({ state: "stopping" });
    const stop = screen.getByRole("button", { name: BANNER_STOPPING_LABEL });
    expect(stop).toBeDisabled();
  });

  // --- The sticky warning variant (Story 19.4) -----------------------------

  it("renders the amber warning variant while status.warning is set", () => {
    mockReducedMotion(false);
    const { container } = render(
      <ActiveRecordingBanner
        status={{ ...LIVE, warning: MIC_WARNING }}
        elapsed="12:34"
        onStop={vi.fn()}
        onRestart={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    const banner = container.querySelector('[data-slot="active-recording-banner"]');
    expect(banner).toHaveClass("border-held");
    expect(banner).not.toHaveClass("border-recording-red");
    // The warning line is an alert carrying the honest sidecar message…
    expect(screen.getByRole("alert")).toHaveTextContent(MIC_WARNING);
    // …while the session stays visibly live (recording line + Stop intact).
    expect(screen.getByText("Recording")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: BANNER_STOP_LABEL })).toBeEnabled();
  });

  it("stays on the recording-red edge with no warning", () => {
    mockReducedMotion(false);
    const { container } = render(
      <ActiveRecordingBanner
        status={LIVE}
        elapsed="12:34"
        onStop={vi.fn()}
        onRestart={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    const banner = container.querySelector('[data-slot="active-recording-banner"]');
    expect(banner).toHaveClass("border-recording-red");
    expect(banner).not.toHaveClass("border-held");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("the warning persists across later live states and is non-dismissible", () => {
    // The slot is sticky in the Rust snapshot; the banner must keep rendering
    // it for every later live state (rotation, stopping) and offer NO close
    // affordance — the only button is Stop.
    mockReducedMotion(false);
    const { rerender } = render(
      <ActiveRecordingBanner
        status={{ ...LIVE, warning: MIC_WARNING }}
        elapsed="12:34"
        onStop={vi.fn()}
        onRestart={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    for (const state of ["rotating", "recording", "stopping"] as const) {
      rerender(
        <ActiveRecordingBanner
          status={{ ...LIVE, state, warning: MIC_WARNING }}
          elapsed="13:00"
          onStop={vi.fn()}
          onRestart={vi.fn()}
          onDismiss={vi.fn()}
        />,
      );
      expect(screen.getByRole("alert")).toHaveTextContent(MIC_WARNING);
    }
    // Non-dismissible: no close/dismiss button exists — only Stop.
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  // --- The error variant (Story 18.4) --------------------------------------

  /** A failed snapshot carrying the honest sidecar reason. */
  const FAILED_REASON = "keeper-rec exited unexpectedly";

  function renderError(overrides: Partial<RecordingStatusVm> = {}) {
    return renderBanner({ state: "failed", error: FAILED_REASON, ...overrides });
  }

  it("renders the filled recording-red error variant on failed + error", () => {
    mockReducedMotion(false);
    const { container } = renderError();
    const banner = container.querySelector('[data-slot="active-recording-banner"]');
    expect(banner).toHaveAttribute("data-variant", "error");
    // Filled recording-red (fill + edge), never the amber warning chrome.
    expect(banner).toHaveClass("bg-recording-red/15");
    expect(banner).toHaveClass("border-recording-red");
    expect(banner).not.toHaveClass("border-held");
    // The reason is announced assertively as a loss-risk event.
    expect(screen.getByRole("alert")).toHaveTextContent(`Recording failed — ${FAILED_REASON}`);
    // No live chrome: no Stop, no meter, no ticking line.
    expect(screen.queryByRole("button", { name: BANNER_STOP_LABEL })).not.toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
  });

  it("renders the error variant for each induced-fault leg's reason", () => {
    // The recorder-kill / writer-stall / device-loss legs all surface through
    // the same snapshot contract — the banner renders whatever honest reason
    // the sidecar reported.
    mockReducedMotion(false);
    for (const reason of [
      "keeper-rec exited unexpectedly", // recorder-kill
      "writer stalled — no samples appended", // writer-stall
      "capture device lost", // device-loss
    ]) {
      const { unmount } = renderBanner({ state: "failed", error: reason });
      expect(screen.getByRole("alert")).toHaveTextContent(reason);
      unmount();
    }
  });

  it("offers Restart and Dismiss, wired to their callbacks", () => {
    mockReducedMotion(false);
    const { onRestart, onDismiss } = renderError();
    fireEvent.click(screen.getByRole("button", { name: BANNER_RESTART_LABEL }));
    expect(onRestart).toHaveBeenCalledTimes(1);
    expect(onDismiss).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: BANNER_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(onRestart).toHaveBeenCalledTimes(1);
  });

  it("never puts recording-red on the Restart/Dismiss buttons", () => {
    // Recording-red is reserved for the dot, edge, and fill — the buttons keep
    // the destructive-outline / neutral variants (distinct destructive red).
    mockReducedMotion(false);
    renderError();
    for (const name of [BANNER_RESTART_LABEL, BANNER_DISMISS_LABEL]) {
      const button = screen.getByRole("button", { name });
      expect(button.className).not.toMatch(/recording-red/);
    }
    expect(screen.getByRole("button", { name: BANNER_RESTART_LABEL })).toHaveAttribute(
      "data-variant",
      "destructive",
    );
    expect(screen.getByRole("button", { name: BANNER_DISMISS_LABEL })).toHaveAttribute(
      "data-variant",
      "outline",
    );
  });

  it("keeps the error dot steady regardless of the motion preference", () => {
    for (const reduced of [true, false]) {
      mockReducedMotion(reduced);
      const { unmount } = renderError();
      expect(screen.getByTestId("recording-error-dot")).not.toHaveClass("animate-pulse");
      unmount();
    }
  });

  it("renders nothing on failed WITHOUT an error (and on other terminals with one)", () => {
    mockReducedMotion(false);
    // failed + error==null: no reason to surface — hidden (the LIVE fixture's
    // error is already null; covered again explicitly here).
    const { container } = renderBanner({ state: "failed", error: null });
    expect(container).toBeEmptyDOMElement();
    // A (stale) error on a non-failed terminal never shows the error variant.
    for (const state of ["idle", "finalized", "recovered"] as const) {
      const { container: c, unmount } = renderBanner({ state, error: FAILED_REASON });
      expect(c).toBeEmptyDOMElement();
      unmount();
    }
  });

  it("keeps the live variant while live even if a (stale) error rides the snapshot", () => {
    // The error variant is gated on state === "failed", not on error alone.
    mockReducedMotion(false);
    renderBanner({ state: "recording", error: FAILED_REASON });
    expect(screen.getByText("Recording")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: BANNER_STOP_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: BANNER_RESTART_LABEL })).not.toBeInTheDocument();
  });

  it("announces state and segment assertively (not the per-second elapsed)", () => {
    mockReducedMotion(false);
    const { container } = render(
      <ActiveRecordingBanner
        status={{ ...LIVE, segmentsClosed: 2 }}
        elapsed="12:34"
        onStop={vi.fn()}
        onRestart={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    const live = container.querySelector('[aria-live="assertive"]');
    expect(live).not.toBeNull();
    expect(live).toHaveTextContent("Recording, segment 3");
    // The ticking elapsed must never sit inside the live region.
    expect(live).not.toHaveTextContent("12:34");
  });
});
