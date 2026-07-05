import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import type { InboxRoomVm } from "@/lib/ipc/client";

function room(overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "acctA",
    hueIndex: 0,
    roomId: "!abc:example.org",
    displayName: "Alice Smith",
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    ...overrides,
  };
}

describe("RoomAvatar Network badge (Story 4.6)", () => {
  it("renders a Network badge with the label initial when the room is bridged", () => {
    render(<RoomAvatar room={room({ network: "Telegram" })} size="lg" />);
    const badge = screen.getByLabelText("Telegram network");
    expect(badge).toBeInTheDocument();
    // The badge shows the Network label's first grapheme, uppercased.
    expect(badge).toHaveTextContent("T");
    // Full Network name is exposed as the title too.
    expect(badge).toHaveAttribute("title", "Telegram");
    // Uniform 16 px badge (AC): the `size-4!` important class must survive so the
    // component's own `group-data-[size=lg]/avatar:size-3` variant cannot shrink it.
    expect(badge).toHaveClass("size-4!");
  });

  it("derives the initial from the first grapheme, uppercased", () => {
    render(<RoomAvatar room={room({ network: "whatsapp" })} size="lg" />);
    const badge = screen.getByLabelText("whatsapp network");
    expect(badge).toHaveTextContent("W");
  });

  it("renders no badge for a native Matrix room (network null)", () => {
    render(<RoomAvatar room={room({ network: null })} size="lg" />);
    expect(screen.queryByLabelText(/network$/)).not.toBeInTheDocument();
  });
});
