import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { MessageBubble, type MessageVm } from "@/components/chat/message-bubble";

function msg(overrides: Partial<MessageVm> = {}): MessageVm {
  return {
    kind: "message",
    key: "unique-1",
    sender: "@bob:example.org",
    senderDisplayName: "Bob Jones",
    body: "hello there",
    timestamp: new Date(2026, 6, 4, 9, 30, 0).getTime(),
    isOwn: false,
    ...overrides,
  };
}

describe("MessageBubble", () => {
  it("renders the body text", () => {
    render(<MessageBubble item={msg()} grouped={false} />);
    expect(screen.getByText("hello there")).toBeInTheDocument();
  });

  it("shows the sender name and avatar on an ungrouped incoming bubble", () => {
    render(<MessageBubble item={msg({ isOwn: false })} grouped={false} />);
    expect(screen.getByText("Bob Jones")).toBeInTheDocument();
    // Avatar fallback initials of "Bob Jones" → "BJ".
    expect(screen.getByText("BJ")).toBeInTheDocument();
  });

  it("hides the sender name and avatar on a grouped bubble", () => {
    render(<MessageBubble item={msg({ isOwn: false })} grouped={true} />);
    expect(screen.queryByText("Bob Jones")).not.toBeInTheDocument();
    expect(screen.queryByText("BJ")).not.toBeInTheDocument();
  });

  it("falls back to the sender id when there is no display name", () => {
    render(<MessageBubble item={msg({ senderDisplayName: null })} grouped={false} />);
    expect(screen.getByText("@bob:example.org")).toBeInTheDocument();
  });

  it("uses the primary surface for an outgoing bubble", () => {
    render(<MessageBubble item={msg({ isOwn: true })} grouped={false} />);
    const body = screen.getByText("hello there").closest("div");
    expect(body).not.toBeNull();
    expect(body).toHaveClass("bg-primary");
    expect(body).toHaveClass("rounded-[14px]");
  });

  it("uses the muted surface for an incoming bubble", () => {
    render(<MessageBubble item={msg({ isOwn: false })} grouped={false} />);
    const body = screen.getByText("hello there").closest("div");
    expect(body).not.toBeNull();
    expect(body).toHaveClass("bg-muted");
    expect(body).toHaveClass("rounded-[14px]");
  });

  it("does not render the sender name for an outgoing bubble", () => {
    render(<MessageBubble item={msg({ isOwn: true })} grouped={false} />);
    expect(screen.queryByText("Bob Jones")).not.toBeInTheDocument();
  });

  it("renders a clock timestamp", () => {
    render(<MessageBubble item={msg()} grouped={false} />);
    expect(screen.getByText(/\d{1,2}:\d{2}/)).toBeInTheDocument();
  });
});
