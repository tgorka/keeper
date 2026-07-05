import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { HistoryBoundary } from "@/components/chat/history-boundary";

describe("HistoryBoundary", () => {
  it("renders nothing in the idle state", () => {
    const { container } = render(<HistoryBoundary state="idle" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a busy spinner while paginating", () => {
    render(<HistoryBoundary state="paginating" />);
    const row = screen.getByRole("status");
    expect(row).toHaveAttribute("aria-busy", "true");
    expect(screen.getByText("Older history loads from your homeserver")).toBeInTheDocument();
  });

  it("states offline and does NOT spin when offline", () => {
    render(<HistoryBoundary state="offline" />);
    expect(
      screen.getByText("You're offline — older messages will load when you reconnect"),
    ).toBeInTheDocument();
    // No busy/spinner region in the offline state (it stops, never spins forever).
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("states the conversation start at the homeserver boundary", () => {
    render(<HistoryBoundary state="atStart" />);
    expect(screen.getByText("This is the start of the conversation")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("offers a Retry in the error state", () => {
    const onRetry = vi.fn();
    render(<HistoryBoundary state="error" onRetry={onRetry} />);
    expect(screen.getByText("Couldn't load older messages.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledTimes(1);
  });
});
