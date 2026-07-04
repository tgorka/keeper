import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MessageActions } from "@/components/chat/message-actions";

describe("MessageActions", () => {
  it("always offers Reply", () => {
    render(
      <MessageActions
        messageKey="k1"
        canEdit={false}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Reply" })).toBeInTheDocument();
  });

  it("always offers the Add reaction affordance", () => {
    render(
      <MessageActions
        messageKey="k1"
        canEdit={false}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Add reaction" })).toBeInTheDocument();
  });

  it("offers Edit only when canEdit", () => {
    const { rerender } = render(
      <MessageActions
        messageKey="k1"
        canEdit={false}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    rerender(
      <MessageActions
        messageKey="k1"
        canEdit={true}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });

  it("calls onReply with the message key", () => {
    const onReply = vi.fn();
    render(
      <MessageActions
        messageKey="k7"
        canEdit={true}
        onReact={() => {}}
        onReply={onReply}
        onEdit={() => {}}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    expect(onReply).toHaveBeenCalledWith("k7");
  });

  it("calls onEdit with the message key", () => {
    const onEdit = vi.fn();
    render(
      <MessageActions
        messageKey="k7"
        canEdit={true}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={onEdit}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledWith("k7");
  });

  it("fires onReact with the message key and picked emoji", () => {
    const onReact = vi.fn();
    render(
      <MessageActions
        messageKey="k9"
        canEdit={false}
        onReact={onReact}
        onReply={() => {}}
        onEdit={() => {}}
      />,
    );
    // Open the curated Popover, then pick an emoji.
    fireEvent.click(screen.getByRole("button", { name: "Add reaction" }));
    fireEvent.click(screen.getByRole("button", { name: "React with 👍" }));
    expect(onReact).toHaveBeenCalledWith("k9", "👍");
  });
});
