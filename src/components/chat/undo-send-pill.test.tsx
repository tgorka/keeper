import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  cancelHeldSend: vi.fn(() => Promise.resolve("")),
}));

import { UndoSendPill } from "@/components/chat/undo-send-pill";
import type { HeldSendVm } from "@/lib/ipc/client";
import { cancelHeldSend } from "@/lib/ipc/client";
import { composerStore } from "@/lib/stores/composer";
import { outboxStore } from "@/lib/stores/outbox";

const mockCancel = vi.mocked(cancelHeldSend);

function held(id: string, dispatchInMs: number): HeldSendVm {
  const now = Date.now();
  return {
    id,
    accountId: "acctA",
    roomId: "!r1",
    body: `body-${id}`,
    heldAtMs: now,
    dispatchAtMs: now + dispatchInMs,
  };
}

describe("UndoSendPill", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    mockCancel.mockClear();
    mockCancel.mockResolvedValue("");
    outboxStore.getState().clear();
    composerStore.getState().clear();
    composerStore.setState({ restoreBody: null, restoreNonce: 0, focusNonce: 0 });
  });

  afterEach(() => {
    vi.useRealTimers();
    outboxStore.getState().clear();
  });

  it("renders nothing when there are no held sends", () => {
    const { container } = render(<UndoSendPill accountId="acctA" roomId="!r1" />);
    expect(container.firstChild).toBeNull();
  });

  it("renders one pill per held send, stacked oldest-first", () => {
    act(() => {
      outboxStore
        .getState()
        .applySnapshot("acctA", "!r1", [held("id1", 10_000), held("id2", 20_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);
    const pills = screen.getAllByTestId("undo-send-pill");
    expect(pills).toHaveLength(2);
  });

  it("shows a countdown label and announces the countdown once (aria-live)", () => {
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);
    // The visible label reflects the remaining seconds.
    expect(screen.getByText(/Sending in \d+s/)).toBeInTheDocument();
    // The announce-once region carries the initial remaining seconds.
    expect(screen.getByText(/Sending in \d+ seconds/)).toBeInTheDocument();
  });

  it("clicking Undo cancels the held send and restores the returned body", async () => {
    mockCancel.mockResolvedValue("restored body");
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);

    fireEvent.click(screen.getByTestId("undo-send-button"));
    await waitFor(() => expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1"));
    await waitFor(() => expect(composerStore.getState().restoreBody).toBe("restored body"));
    // The restore is scoped to the originating chat so it can't land in another room's
    // composer if the user switched chats during the async cancel.
    expect(composerStore.getState().restoreTarget).toEqual({ accountId: "acctA", roomId: "!r1" });
  });

  it("an empty cancel result (already dispatched) does not restore the composer", async () => {
    mockCancel.mockResolvedValue("");
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);

    fireEvent.click(screen.getByTestId("undo-send-button"));
    await waitFor(() => expect(mockCancel).toHaveBeenCalled());
    expect(composerStore.getState().restoreBody).toBeNull();
  });

  it("⌘⇧Z undoes the oldest pending hold", async () => {
    mockCancel.mockResolvedValue("oldest body");
    act(() => {
      outboxStore
        .getState()
        .applySnapshot("acctA", "!r1", [held("id1", 10_000), held("id2", 20_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);

    fireEvent.keyDown(window, { key: "z", metaKey: true, shiftKey: true });
    await waitFor(() => expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1"));
  });

  it("plain ⌘Z is ignored (left to composer text-undo)", () => {
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<UndoSendPill accountId="acctA" roomId="!r1" />);

    fireEvent.keyDown(window, { key: "z", metaKey: true, shiftKey: false });
    expect(mockCancel).not.toHaveBeenCalled();
  });
});
