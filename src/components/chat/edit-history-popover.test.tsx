import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { EditHistoryPopover } from "@/components/chat/edit-history-popover";
import type { EditVersionVm } from "@/lib/ipc/client";

const getEditHistory = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  getEditHistory: (...args: unknown[]) => getEditHistory(...args),
}));

function version(overrides: Partial<EditVersionVm> = {}): EditVersionVm {
  return { body: "a version", timestamp: 1_720_000_000_000, isCurrent: false, ...overrides };
}

describe("EditHistoryPopover", () => {
  beforeEach(() => {
    getEditHistory.mockReset();
  });

  it("fetches on open and lists prior versions (newest→oldest), omitting the current", async () => {
    getEditHistory.mockResolvedValue([
      version({ body: "v1", timestamp: 100, isCurrent: false }),
      version({ body: "v2", timestamp: 200, isCurrent: false }),
      version({ body: "v3", timestamp: 300, isCurrent: true }),
    ]);
    render(
      <EditHistoryPopover accountId="acctA" roomId="!r:e.org" messageKey="unique-1">
        <button type="button">Edited</button>
      </EditHistoryPopover>,
    );
    fireEvent.click(screen.getByText("Edited"));

    await waitFor(() => expect(screen.getByText("v1")).toBeInTheDocument());
    expect(screen.getByText("v2")).toBeInTheDocument();
    // The current version is not listed as a prior version.
    expect(screen.queryByText("v3")).not.toBeInTheDocument();
    expect(getEditHistory).toHaveBeenCalledWith("acctA", "!r:e.org", "unique-1");
  });

  it("shows the empty state when there are no prior versions", async () => {
    // Only the current version exists → no prior versions to show.
    getEditHistory.mockResolvedValue([version({ body: "only", isCurrent: true })]);
    render(
      <EditHistoryPopover accountId="acctA" roomId="!r:e.org" messageKey="unique-1">
        <button type="button">Edited</button>
      </EditHistoryPopover>,
    );
    fireEvent.click(screen.getByText("Edited"));
    await waitFor(() => expect(screen.getByText("No local history.")).toBeInTheDocument());
  });

  it("shows a distinct error state on a fetch error", async () => {
    getEditHistory.mockRejectedValue(new Error("boom"));
    render(
      <EditHistoryPopover accountId="acctA" roomId="!r:e.org" messageKey="unique-1">
        <button type="button">Edited</button>
      </EditHistoryPopover>,
    );
    fireEvent.click(screen.getByText("Edited"));
    // A real read failure is reported distinctly from "no prior versions".
    await waitFor(() => expect(screen.getByText("Couldn't load history.")).toBeInTheDocument());
    expect(screen.queryByText("No local history.")).not.toBeInTheDocument();
  });
});
