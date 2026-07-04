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
    isEdited: false,
    reply: null,
    reactions: [],
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

  it("shows the amber Queued caption when offline and sending (in place of Sending…)", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sending" })}
        grouped={false}
        groupTail={true}
        offline={true}
      />,
    );
    const caption = screen.getByText("Queued — sends when you're back online");
    expect(caption).toBeInTheDocument();
    expect(caption).toHaveClass("text-held");
    expect(screen.queryByText("Sending…")).not.toBeInTheDocument();
  });

  it("shows Sending… when online and sending (offline defaults to false)", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sending" })}
        grouped={false}
        groupTail={true}
      />,
    );
    expect(screen.getByText("Sending…")).toBeInTheDocument();
    expect(screen.queryByText("Queued — sends when you're back online")).not.toBeInTheDocument();
  });

  it("does not show the Queued caption for a sent message even when offline", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sent" })}
        grouped={false}
        groupTail={true}
        offline={true}
      />,
    );
    expect(screen.getByText("Sent")).toBeInTheDocument();
    expect(screen.queryByText("Queued — sends when you're back online")).not.toBeInTheDocument();
  });

  it("keeps the Failed — Retry caption when offline (Queued never overrides failed)", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "failed" })}
        grouped={false}
        groupTail={true}
        offline={true}
      />,
    );
    expect(screen.getByText("Failed")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(screen.queryByText("Queued — sends when you're back online")).not.toBeInTheDocument();
  });

  it("hides the Queued caption when offline+sending but not the group tail", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: true, sendState: "sending" })}
        grouped={false}
        groupTail={false}
        offline={true}
      />,
    );
    expect(screen.queryByText("Queued — sends when you're back online")).not.toBeInTheDocument();
  });

  it("never shows the Queued caption for a non-own sending message (isOwn guard)", () => {
    render(
      <MessageBubble
        item={msg({ isOwn: false, sendState: "sending" })}
        grouped={false}
        groupTail={true}
        offline={true}
      />,
    );
    expect(screen.queryByText("Queued — sends when you're back online")).not.toBeInTheDocument();
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

  it("renders the Edited caption when isEdited", () => {
    render(<MessageBubble item={msg({ isEdited: true })} grouped={false} />);
    expect(screen.getByText("Edited")).toBeInTheDocument();
  });

  it("does not render the Edited caption for an unedited message", () => {
    render(<MessageBubble item={msg({ isEdited: false })} grouped={false} />);
    expect(screen.queryByText("Edited")).not.toBeInTheDocument();
  });

  it("renders the reply quote (sender + body) above the body", () => {
    render(
      <MessageBubble
        item={msg({
          body: "my reply",
          reply: {
            inReplyToKey: "orig-1",
            sender: "@carol:example.org",
            senderDisplayName: "Carol",
            body: "the original message",
          },
        })}
        grouped={false}
      />,
    );
    expect(screen.getByText("Carol")).toBeInTheDocument();
    expect(screen.getByText("the original message")).toBeInTheDocument();
    expect(screen.getByText("my reply")).toBeInTheDocument();
  });

  it("clicking a resolved reply quote jumps to the original by key", () => {
    const onJumpTo = vi.fn();
    render(
      <MessageBubble
        item={msg({
          reply: {
            inReplyToKey: "orig-1",
            sender: "@carol:example.org",
            senderDisplayName: "Carol",
            body: "the original",
          },
        })}
        grouped={false}
        onJumpTo={onJumpTo}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Jump to replied message" }));
    expect(onJumpTo).toHaveBeenCalledWith("orig-1");
  });

  it("renders an unresolved reply quote as a non-clickable block (null key)", () => {
    const onJumpTo = vi.fn();
    render(
      <MessageBubble
        item={msg({
          reply: {
            inReplyToKey: null,
            sender: "@carol:example.org",
            senderDisplayName: "Carol",
            body: "unloaded original",
          },
        })}
        grouped={false}
        onJumpTo={onJumpTo}
      />,
    );
    expect(screen.getByText("unloaded original")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Jump to replied message" }),
    ).not.toBeInTheDocument();
  });

  it("offers Reply always but Edit only on an own message in the action bar", () => {
    const { rerender } = render(
      <MessageBubble
        item={msg({ isOwn: false })}
        grouped={false}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Reply" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    rerender(
      <MessageBubble
        item={msg({ isOwn: true })}
        grouped={false}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });

  it("applies a selection ring when selected", () => {
    render(<MessageBubble item={msg()} grouped={false} selected={true} />);
    const body = screen.getByText("hello there").closest("div");
    expect(body).toHaveClass("ring-2");
  });

  it("renders no reaction pill row when there are no reactions", () => {
    render(
      <MessageBubble item={msg({ reactions: [] })} grouped={false} onToggleReaction={() => {}} />,
    );
    // Pills are the only toggle buttons (`aria-pressed`); asserting none exist in
    // either pressed state catches a stray pill of ANY emoji, not a fixed subset.
    expect(screen.queryByRole("button", { pressed: true })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { pressed: false })).not.toBeInTheDocument();
  });

  it("renders one pill per emoji group with its count", () => {
    render(
      <MessageBubble
        item={msg({
          reactions: [
            { emoji: "👍", count: 3, isOwn: false },
            { emoji: "❤️", count: 1, isOwn: true },
          ],
        })}
        grouped={false}
      />,
    );
    expect(screen.getByRole("button", { name: "👍 3" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "❤️ 1, you reacted" })).toBeInTheDocument();
  });

  it("marks an own-reaction pill as pressed and a non-own pill as not pressed", () => {
    render(
      <MessageBubble
        item={msg({
          reactions: [
            { emoji: "👍", count: 3, isOwn: false },
            { emoji: "❤️", count: 1, isOwn: true },
          ],
        })}
        grouped={false}
        onToggleReaction={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "👍 3" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("button", { name: "❤️ 1, you reacted" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("toggles a reaction with the message key and emoji when a pill is clicked", () => {
    const onToggleReaction = vi.fn();
    render(
      <MessageBubble
        item={msg({ key: "k42", reactions: [{ emoji: "🔥", count: 2, isOwn: false }] })}
        grouped={false}
        onToggleReaction={onToggleReaction}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "🔥 2" }));
    expect(onToggleReaction).toHaveBeenCalledWith("k42", "🔥");
  });

  it("toggles a reaction from the action-bar Popover pick", () => {
    const onToggleReaction = vi.fn();
    render(
      <MessageBubble
        item={msg({ key: "k7" })}
        grouped={false}
        onToggleReaction={onToggleReaction}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Add reaction" }));
    fireEvent.click(screen.getByRole("button", { name: "React with 😂" }));
    expect(onToggleReaction).toHaveBeenCalledWith("k7", "😂");
  });
});
