import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MessageActions } from "@/components/chat/message-actions";

/** Render the action bar with sensible defaults, overriding only what a test needs. */
function renderActions(props: Partial<React.ComponentProps<typeof MessageActions>> = {}) {
  return render(
    <MessageActions
      messageKey="k1"
      canEdit={false}
      canDelete={false}
      onReact={() => {}}
      onReply={() => {}}
      onEdit={() => {}}
      onDelete={() => {}}
      {...props}
    />,
  );
}

describe("MessageActions", () => {
  it("always offers Reply", () => {
    renderActions();
    expect(screen.getByRole("button", { name: "Reply" })).toBeInTheDocument();
  });

  it("always offers the Add reaction affordance", () => {
    renderActions();
    expect(screen.getByRole("button", { name: "Add reaction" })).toBeInTheDocument();
  });

  it("offers Edit only when canEdit", () => {
    const { rerender } = renderActions({ canEdit: false });
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    rerender(
      <MessageActions
        messageKey="k1"
        canEdit={true}
        canDelete={false}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });

  it("offers Delete only when canDelete (own message)", () => {
    const { rerender } = renderActions({ canDelete: false });
    expect(screen.queryByRole("button", { name: "Delete" })).not.toBeInTheDocument();

    rerender(
      <MessageActions
        messageKey="k1"
        canEdit={false}
        canDelete={true}
        onReact={() => {}}
        onReply={() => {}}
        onEdit={() => {}}
        onDelete={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Delete" })).toBeInTheDocument();
  });

  it("calls onReply with the message key", () => {
    const onReply = vi.fn();
    renderActions({ messageKey: "k7", onReply });
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    expect(onReply).toHaveBeenCalledWith("k7");
  });

  it("calls onEdit with the message key", () => {
    const onEdit = vi.fn();
    renderActions({ messageKey: "k7", canEdit: true, onEdit });
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledWith("k7");
  });

  it("calls onDelete with the message key", () => {
    const onDelete = vi.fn();
    renderActions({ messageKey: "k7", canDelete: true, onDelete });
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onDelete).toHaveBeenCalledWith("k7");
  });

  it("fires onReact with the message key and picked emoji", () => {
    const onReact = vi.fn();
    renderActions({ messageKey: "k9", onReact });
    // Open the curated Popover, then pick an emoji.
    fireEvent.click(screen.getByRole("button", { name: "Add reaction" }));
    fireEvent.click(screen.getByRole("button", { name: "React with 👍" }));
    expect(onReact).toHaveBeenCalledWith("k9", "👍");
  });
});
