import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ReactionPopover } from "@/components/chat/reaction-popover";

describe("ReactionPopover", () => {
  it("renders the Add reaction trigger", () => {
    render(<ReactionPopover onPick={() => {}} />);
    expect(screen.getByRole("button", { name: "Add reaction" })).toBeInTheDocument();
  });

  it("opens the curated emoji set on click", () => {
    render(<ReactionPopover onPick={() => {}} />);
    expect(screen.queryByRole("button", { name: "React with 👍" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add reaction" }));
    expect(screen.getByRole("button", { name: "React with 👍" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "React with ❤️" })).toBeInTheDocument();
  });

  it("fires onPick with the chosen emoji and closes", () => {
    const onPick = vi.fn();
    render(<ReactionPopover onPick={onPick} />);
    fireEvent.click(screen.getByRole("button", { name: "Add reaction" }));
    fireEvent.click(screen.getByRole("button", { name: "React with 🎉" }));
    expect(onPick).toHaveBeenCalledWith("🎉");
    // The popover closes after a pick.
    expect(screen.queryByRole("button", { name: "React with 🎉" })).not.toBeInTheDocument();
  });
});
