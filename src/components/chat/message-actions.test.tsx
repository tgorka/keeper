import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { MessageActions } from "@/components/chat/message-actions";

describe("MessageActions", () => {
  it("always offers Reply", () => {
    render(<MessageActions messageKey="k1" canEdit={false} onReply={() => {}} onEdit={() => {}} />);
    expect(screen.getByRole("button", { name: "Reply" })).toBeInTheDocument();
  });

  it("offers Edit only when canEdit", () => {
    const { rerender } = render(
      <MessageActions messageKey="k1" canEdit={false} onReply={() => {}} onEdit={() => {}} />,
    );
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();

    rerender(
      <MessageActions messageKey="k1" canEdit={true} onReply={() => {}} onEdit={() => {}} />,
    );
    expect(screen.getByRole("button", { name: "Edit" })).toBeInTheDocument();
  });

  it("calls onReply with the message key", () => {
    const onReply = vi.fn();
    render(<MessageActions messageKey="k7" canEdit={true} onReply={onReply} onEdit={() => {}} />);
    fireEvent.click(screen.getByRole("button", { name: "Reply" }));
    expect(onReply).toHaveBeenCalledWith("k7");
  });

  it("calls onEdit with the message key", () => {
    const onEdit = vi.fn();
    render(<MessageActions messageKey="k7" canEdit={true} onReply={() => {}} onEdit={onEdit} />);
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));
    expect(onEdit).toHaveBeenCalledWith("k7");
  });
});
