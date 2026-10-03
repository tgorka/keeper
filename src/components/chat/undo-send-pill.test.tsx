import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useRef } from "react";
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

/**
 * The pill inside its conversation (holding the composer's field), beside a
 * field outside it — the note editor in the notes view.
 */
function Conversation() {
  const root = useRef<HTMLDivElement>(null);
  return (
    <>
      <textarea aria-label="Note" />
      <div ref={root}>
        <UndoSendPill accountId="acctA" roomId="!r1" scope={root} />
        <textarea aria-label="Message" />
      </div>
    </>
  );
}

function pressRedo(on: HTMLElement): void {
  on.focus();
  fireEvent.keyDown(on, { key: "z", metaKey: true, shiftKey: true });
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
    render(<Conversation />);
    expect(screen.queryByTestId("undo-send-pill-stack")).toBeNull();
  });

  it("renders one pill per held send, stacked oldest-first", () => {
    act(() => {
      outboxStore
        .getState()
        .applySnapshot("acctA", "!r1", [held("id1", 10_000), held("id2", 20_000)]);
    });
    render(<Conversation />);
    const pills = screen.getAllByTestId("undo-send-pill");
    expect(pills).toHaveLength(2);
  });

  it("shows a countdown label and announces the countdown once (aria-live)", () => {
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<Conversation />);
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
    render(<Conversation />);

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
    render(<Conversation />);

    fireEvent.click(screen.getByTestId("undo-send-button"));
    await waitFor(() => expect(mockCancel).toHaveBeenCalled());
    expect(composerStore.getState().restoreBody).toBeNull();
  });

  it("⌘⇧Z in the conversation undoes the oldest pending hold", async () => {
    mockCancel.mockResolvedValue("oldest body");
    act(() => {
      outboxStore
        .getState()
        .applySnapshot("acctA", "!r1", [held("id1", 10_000), held("id2", 20_000)]);
    });
    render(<Conversation />);

    pressRedo(screen.getByLabelText("Message"));
    await waitFor(() => expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1"));
  });

  it("plain ⌘Z is ignored (left to composer text-undo)", () => {
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<Conversation />);

    const message = screen.getByLabelText("Message");
    message.focus();
    fireEvent.keyDown(message, { key: "z", metaKey: true, shiftKey: false });
    expect(mockCancel).not.toHaveBeenCalled();
  });

  /**
   * U1: the dock's held send survives redo in the note beside it. ⌘⇧Z outside the
   * conversation is not this pill's, nor is one a handler before it already took
   * (CodeMirror's redo `preventDefault`s and lets the event bubble).
   */
  it("leaves ⌘⇧Z outside the conversation, or already taken, alone", () => {
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", 10_000)]);
    });
    render(<Conversation />);

    pressRedo(screen.getByLabelText("Note"));
    expect(mockCancel).not.toHaveBeenCalled();

    const message = screen.getByLabelText("Message");
    message.focus();
    const taken = new KeyboardEvent("keydown", {
      key: "z",
      metaKey: true,
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    });
    taken.preventDefault();
    message.dispatchEvent(taken);
    expect(mockCancel).not.toHaveBeenCalled();
  });
});
