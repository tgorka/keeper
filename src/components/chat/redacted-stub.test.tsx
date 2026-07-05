import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { REDACTED_STUB_TEXT, RedactedStub } from "@/components/chat/redacted-stub";

describe("RedactedStub", () => {
  it("renders the honest 'Message deleted' copy (never blank)", () => {
    render(<RedactedStub />);
    expect(screen.getByText(REDACTED_STUB_TEXT)).toBeInTheDocument();
    // Sentence case, no exclamation (UX-DR10).
    expect(REDACTED_STUB_TEXT).toBe("Message deleted");
  });

  it("exposes the stub as a status region (honest, non-blank)", () => {
    render(<RedactedStub />);
    expect(screen.getByRole("status")).toHaveTextContent("Message deleted");
  });
});
