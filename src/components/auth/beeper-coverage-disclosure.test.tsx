import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  BeeperCoverageDisclosure,
  DISCLOSURE_EXPLANATION,
  DISCLOSURE_PARITY_SENTENCE,
  DISCLOSURE_TITLE,
  DISCLOSURE_WHATSAPP_SENTENCE,
} from "@/components/auth/beeper-coverage-disclosure";

describe("BeeperCoverageDisclosure", () => {
  it("renders the title and explanation naming the consequence (voice rules)", () => {
    render(<BeeperCoverageDisclosure />);
    expect(screen.getByText(DISCLOSURE_TITLE)).toBeInTheDocument();
    expect(screen.getByText(DISCLOSURE_EXPLANATION)).toBeInTheDocument();
    expect(DISCLOSURE_TITLE).toBe("On-Device Chats won't appear in keeper");
  });

  it("renders the exact literal WhatsApp sentence", () => {
    render(<BeeperCoverageDisclosure />);
    expect(
      screen.getByText("WhatsApp connected in the official Beeper app will not appear here."),
    ).toBeInTheDocument();
    expect(DISCLOSURE_WHATSAPP_SENTENCE).toBe(
      "WhatsApp connected in the official Beeper app will not appear here.",
    );
  });

  it("renders the self-hosted Bridge parity-path sentence", () => {
    render(<BeeperCoverageDisclosure />);
    expect(screen.getByText("Running your own Bridge is the path to parity.")).toBeInTheDocument();
    expect(DISCLOSURE_PARITY_SENTENCE).toBe("Running your own Bridge is the path to parity.");
  });

  it("contains no exclamation mark (voice rules)", () => {
    const { container } = render(<BeeperCoverageDisclosure />);
    expect(container.textContent).not.toContain("!");
  });
});
