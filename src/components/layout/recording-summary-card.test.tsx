import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  revealPath: vi.fn(() => Promise.resolve()),
}));

import {
  RECOVERY_DISMISS_LABEL,
  REVEAL_IN_FINDER_LABEL,
  RecordingSummaryCard,
} from "@/components/layout/recording-summary-card";
import { revealPath } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const mockReveal = vi.mocked(revealPath);

const FOLDER = "/Users/alice/Movies/keeper/keeper-rec 2026-07-19 14.23.45";

beforeEach(() => {
  mockReveal.mockReset();
  mockReveal.mockResolvedValue(undefined);
  // Reveal is capability-gated: default it ON for the base cases.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

describe("RecordingSummaryCard", () => {
  it("renders the completion variant: count, size, folder, and Reveal", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={3}
        totalBytes={412_000_000}
      />,
    );

    expect(screen.getByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    const reveal = screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL });
    fireEvent.click(reveal);
    expect(mockReveal).toHaveBeenCalledWith(FOLDER);
    // No dismiss on the completion variant (a finalized session is never dismissed).
    expect(screen.queryByRole("button", { name: RECOVERY_DISMISS_LABEL })).not.toBeInTheDocument();
  });

  it("says '1 segment' (singular) for a single-segment session", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.getByText(/Saved 1 segment · 1 MB/)).toBeInTheDocument();
  });

  it("renders the recovered variant: interruption copy, warning edge, and Dismiss", () => {
    const onDismiss = vi.fn();
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={2}
        totalBytes={200_000_000}
        onDismiss={onDismiss}
      />,
    );

    expect(
      screen.getByText(/A recording was interrupted; 2 segments were saved/),
    ).toBeInTheDocument();
    // The bridge-degraded warning edge (the bridge-card recipe).
    const card = screen.getByRole("status");
    expect(card.className).toContain("border-bridge-degraded/50");
    expect(card.className).toContain("text-bridge-degraded");

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("degrades to a figureless headline (never '0 segments · 0 MB') when the summary is unavailable", () => {
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
      />,
    );
    // The honest degraded shape: outcome + folder + Reveal, no fabricated zero.
    expect(screen.getByText("Recording saved")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
    expect(screen.queryByText(/0 MB/)).not.toBeInTheDocument();
    expect(screen.getByText(FOLDER)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
  });

  it("degrades the recovered variant to the interruption headline without fabricated figures", () => {
    render(
      <RecordingSummaryCard
        variant="recovered"
        sessionFolder={FOLDER}
        screenSegmentCount={null}
        totalBytes={null}
        onDismiss={vi.fn()}
      />,
    );
    expect(screen.getByText("A recording was interrupted")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
  });

  it("hides the Reveal button when revealInFileManager is false", () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });
    render(
      <RecordingSummaryCard
        variant="completion"
        sessionFolder={FOLDER}
        screenSegmentCount={1}
        totalBytes={1_000_000}
      />,
    );
    expect(screen.queryByRole("button", { name: REVEAL_IN_FINDER_LABEL })).not.toBeInTheDocument();
  });
});
