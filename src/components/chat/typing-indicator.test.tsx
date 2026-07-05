import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { TypingIndicator } from "@/components/chat/typing-indicator";
import type { TypistVm } from "@/lib/ipc/client";

function typist(userId: string, displayName: string | null = null): TypistVm {
  return { userId, displayName };
}

describe("TypingIndicator", () => {
  it("renders an empty live region when nobody is typing", () => {
    render(<TypingIndicator typists={[]} />);
    const region = screen.getByTestId("typing-indicator");
    expect(region).toHaveTextContent("");
    // The live region stays mounted so a later change is announced.
    expect(region).toHaveAttribute("aria-live", "polite");
  });

  it("renders '<name> is typing…' for a single typist", () => {
    render(<TypingIndicator typists={[typist("@bob:example.org", "Bob")]} />);
    expect(screen.getByText("Bob is typing…")).toBeInTheDocument();
  });

  it("falls back to the user id when a display name is missing", () => {
    render(<TypingIndicator typists={[typist("@bob:example.org")]} />);
    expect(screen.getByText("@bob:example.org is typing…")).toBeInTheDocument();
  });

  it("renders '<a> and <b> are typing…' for two typists", () => {
    render(
      <TypingIndicator
        typists={[typist("@a:example.org", "Ann"), typist("@b:example.org", "Bo")]}
      />,
    );
    expect(screen.getByText("Ann and Bo are typing…")).toBeInTheDocument();
  });

  it("renders 'Several people are typing…' for three or more typists", () => {
    render(
      <TypingIndicator
        typists={[
          typist("@a:example.org", "Ann"),
          typist("@b:example.org", "Bo"),
          typist("@c:example.org", "Cy"),
        ]}
      />,
    );
    expect(screen.getByText("Several people are typing…")).toBeInTheDocument();
  });
});
