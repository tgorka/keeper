import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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
    sendState: null,
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

  it("renders no send-state caption for a remote message (null sendState)", () => {
    render(<MessageBubble item={msg({ isOwn: true, sendState: null })} grouped={false} />);
    expect(screen.queryByText("Sending…")).not.toBeInTheDocument();
    expect(screen.queryByText("Sent")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
  });

  it("shows the Sending… caption on the group tail", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sending" })}
        grouped={false}
        groupTail={true}
      />,
    );
    expect(screen.getByText("Sending…")).toBeInTheDocument();
  });

  it("shows the Sent caption on the group tail", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sent" })}
        grouped={false}
        groupTail={true}
      />,
    );
    expect(screen.getByText("Sent")).toBeInTheDocument();
  });

  it("hides the transient caption when not the group tail", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sending" })}
        grouped={false}
        groupTail={false}
      />,
    );
    expect(screen.queryByText("Sending…")).not.toBeInTheDocument();
  });

  it("always shows the persistent Failed — Retry caption, even when not the tail", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "failed" })}
        grouped={false}
        groupTail={false}
      />,
    );
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("calls onRetry with the message key when Retry is activated", () => {
    const onRetry = vi.fn();
    render(
      <MessageBubble
        item={msg({ key: "outgoing-9", isOwn: true, sendState: "failed" })}
        grouped={false}
        onRetry={onRetry}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetry).toHaveBeenCalledWith("outgoing-9");
  });
});
