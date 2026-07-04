import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Composer } from "@/components/chat/composer";

describe("Composer", () => {
  it("sends the trimmed body on Enter and clears the draft", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "  hello  " } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith("hello");
    });
    await waitFor(() => {
      expect(textarea.value).toBe("");
    });
  });

  it("inserts a newline on ⇧Enter and does not send", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "line one" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });

    expect(onSend).not.toHaveBeenCalled();
  });

  it("ignores a whitespace-only body", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "   \n\t " } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).not.toHaveBeenCalled();
  });

  it("sends on the send button click", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "click send" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith("click send");
    });
  });

  it("keeps the draft and surfaces an inline error when the send rejects", async () => {
    const onSend = vi.fn().mockRejectedValue(new Error("nope"));
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "keep me" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => {
      expect(onSend).toHaveBeenCalled();
    });
    // A failed enqueue keeps the user's text and shows an honest inline error.
    expect(textarea.value).toBe("keep me");
    expect(await screen.findByRole("alert")).toHaveTextContent(/couldn't send/i);
  });

  it("clears the inline send error when the draft is edited", async () => {
    const onSend = vi.fn().mockRejectedValue(new Error("nope"));
    render(<Composer onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "boom" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await screen.findByRole("alert");

    fireEvent.change(textarea, { target: { value: "boom edited" } });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is inert when disabled", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} disabled />);
    const textarea = screen.getByLabelText("Message");

    expect(textarea).toBeDisabled();
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("disables the send button for an empty draft", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer onSend={onSend} />);
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });
});
