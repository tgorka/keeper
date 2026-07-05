import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IpcError } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the dialog never touches Tauri.
const deleteMessage = vi.fn();
const roomNetworkLabel = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  deleteMessage: (accountId: string, roomId: string, itemKey: string) =>
    deleteMessage(accountId, roomId, itemKey),
  roomNetworkLabel: (accountId: string, roomId: string) => roomNetworkLabel(accountId, roomId),
}));

import { DeleteMessageDialog } from "@/components/chat/delete-message-dialog";

const ACCOUNT = "acct-1";
const ROOM = "!room:example.org";
const ITEM = "item-7";

function ipcError(retriable: boolean): IpcError {
  return { code: "sendFailed", message: "Network dropped mid-delete.", accountId: null, retriable };
}

beforeEach(() => {
  deleteMessage.mockReset();
  deleteMessage.mockResolvedValue(undefined);
  roomNetworkLabel.mockReset();
  roomNetworkLabel.mockResolvedValue(null);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("DeleteMessageDialog", () => {
  it("shows honest framing with a conditional best-effort caveat and no fabricated Network when there is no label", async () => {
    roomNetworkLabel.mockResolvedValue(null);
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={ITEM} onClose={() => {}} />,
    );

    await waitFor(() => expect(roomNetworkLabel).toHaveBeenCalledWith(ACCOUNT, ROOM));
    expect(screen.getByText(/removes it for everyone in this Chat/)).toBeInTheDocument();
    // An undetected bridge must never be promised a guaranteed remote delete: the
    // null case still carries a conditional best-effort caveat, but names no Network.
    expect(
      screen.getByText(/bridged to another network, removal there is best-effort/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Removal on/)).not.toBeInTheDocument();
  });

  it("names the Network and states removal is best-effort for a bridged Chat", async () => {
    roomNetworkLabel.mockResolvedValue("Telegram");
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={ITEM} onClose={() => {}} />,
    );

    await waitFor(() => expect(screen.getByText(/Removal on/)).toBeInTheDocument());
    // The Network is named and the removal there is honestly best-effort.
    expect(screen.getByText(/Telegram/)).toBeInTheDocument();
    expect(screen.getByText(/best-effort/)).toBeInTheDocument();
  });

  it("dispatches the redaction and closes on a successful confirm", async () => {
    const onClose = vi.fn();
    deleteMessage.mockResolvedValue(undefined);
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={ITEM} onClose={onClose} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete for everyone" }));

    await waitFor(() => expect(deleteMessage).toHaveBeenCalledWith(ACCOUNT, ROOM, ITEM));
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("keeps the dialog open, shows an honest error, and re-enables for retry on dispatch failure", async () => {
    const onClose = vi.fn();
    // First attempt fails (retriable), second succeeds.
    deleteMessage.mockRejectedValueOnce(ipcError(true)).mockResolvedValueOnce(undefined);
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={ITEM} onClose={onClose} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete for everyone" }));

    // The dialog stays open, surfaces the honest error, and did not close.
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Network dropped"));
    expect(onClose).not.toHaveBeenCalled();
    // The action is re-enabled for a retry.
    const retryButton = await screen.findByRole("button", { name: "Delete for everyone" });
    expect(retryButton).not.toBeDisabled();

    // Retry succeeds and closes.
    fireEvent.click(retryButton);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(deleteMessage).toHaveBeenCalledTimes(2);
  });

  it("surfaces an honest terminal error and withdraws retry on a non-retriable failure", async () => {
    const onClose = vi.fn();
    // A vanished target maps to a non-retriable SendFailed (TargetNotFound).
    deleteMessage.mockRejectedValue(ipcError(false));
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={ITEM} onClose={onClose} />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Delete for everyone" }));

    // Honest "no longer available" copy, dialog stays for the user to dismiss…
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(/no longer available/i),
    );
    expect(onClose).not.toHaveBeenCalled();
    // …and the destructive action is withdrawn (disabled) — no futile retry loop.
    expect(screen.getByRole("button", { name: "Delete for everyone" })).toBeDisabled();
    expect(deleteMessage).toHaveBeenCalledTimes(1);
  });

  it("is closed (no dialog) when itemKey is null", () => {
    render(
      <DeleteMessageDialog accountId={ACCOUNT} roomId={ROOM} itemKey={null} onClose={() => {}} />,
    );
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();
    expect(roomNetworkLabel).not.toHaveBeenCalled();
  });
});
