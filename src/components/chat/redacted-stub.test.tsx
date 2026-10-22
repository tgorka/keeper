import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  REDACTED_STUB_TEXT,
  REDACTION_FAILED_TEXT,
  REDACTION_PENDING_TEXT,
  RedactedStub,
} from "@/components/chat/redacted-stub";

describe("RedactedStub", () => {
  it("is not an aria-live region, so a batch of historical redactions is not announced", () => {
    render(<RedactedStub />);
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("note")).toHaveTextContent(REDACTED_STUB_TEXT);
  });

  it("a deletion still queued is not shown as done", () => {
    render(<RedactedStub itemKey="r1" sendState="sending" />);
    expect(screen.getByRole("note")).toHaveTextContent(REDACTION_PENDING_TEXT);
    expect(screen.queryByText(REDACTED_STUB_TEXT)).toBeNull();
  });

  it("a deletion the server took is deleted", () => {
    render(<RedactedStub itemKey="r1" sendState="sent" />);
    expect(screen.getByRole("note")).toHaveTextContent(REDACTED_STUB_TEXT);
  });

  it("a refused deletion is said, never shown as deleted, and Retry and Cancel act on its key", () => {
    const onRetry = vi.fn();
    const onCancel = vi.fn();
    render(<RedactedStub itemKey="r1" sendState="failed" onRetry={onRetry} onCancel={onCancel} />);
    expect(screen.getByRole("status")).toHaveTextContent(REDACTION_FAILED_TEXT);
    expect(screen.queryByText(REDACTED_STUB_TEXT)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onRetry).toHaveBeenCalledWith("r1");
    expect(onCancel).toHaveBeenCalledWith("r1");
  });
});
