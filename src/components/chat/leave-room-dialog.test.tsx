/**
 * `/leave`'s confirmation (Story 100.5): only Leave leaves, Cancel leaves
 * nothing, and a refusal keeps the dialog open with its sentence and a log line.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  LEAVE_ROOM_CONFIRM,
  LEAVE_ROOM_FAILED,
  LeaveRoomDialog,
} from "@/components/chat/leave-room-dialog";

vi.mock("@/lib/crash-report", () => ({ reportFrontendError: vi.fn() }));

import { reportFrontendError } from "@/lib/crash-report";

function mount(leave: (accountId: string, roomId: string) => Promise<void>) {
  const onClose = vi.fn();
  render(
    <LeaveRoomDialog
      accountId="acct"
      roomId="!r:example.org"
      open
      onClose={onClose}
      leave={leave}
    />,
  );
  return { onClose };
}

describe("LeaveRoomDialog", () => {
  it("leaves the room only on Leave, then closes", async () => {
    const leave = vi.fn(async () => {});
    const { onClose } = mount(leave);
    expect(leave).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: LEAVE_ROOM_CONFIRM }));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(leave).toHaveBeenCalledWith("acct", "!r:example.org");
  });

  it("leaves nothing on Cancel", () => {
    const leave = vi.fn(async () => {});
    const { onClose } = mount(leave);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalled();
    expect(leave).not.toHaveBeenCalled();
  });

  it("stays open and says so when the server refuses", async () => {
    const { onClose } = mount(vi.fn(async () => Promise.reject(new Error("M_FORBIDDEN"))));
    fireEvent.click(screen.getByRole("button", { name: LEAVE_ROOM_CONFIRM }));
    expect(await screen.findByRole("alert")).toHaveTextContent(LEAVE_ROOM_FAILED);
    expect(onClose).not.toHaveBeenCalled();
    expect(reportFrontendError).toHaveBeenCalled();
  });
});
