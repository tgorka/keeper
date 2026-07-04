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

  it("renders a reply banner with the quoted sender/preview", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        onSend={onSend}
        pending={{ mode: "reply", targetKey: "k1", sender: "Bob", bodyPreview: "hi there" }}
      />,
    );
    expect(screen.getByText("Replying to Bob")).toBeInTheDocument();
    expect(screen.getByText("hi there")).toBeInTheDocument();
  });

  it("renders an edit banner and a Save button, prefilling the body", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        onSend={onSend}
        pending={{ mode: "edit", targetKey: "k2" }}
        editPrefill="original body"
      />,
    );
    expect(screen.getByText("Editing your message")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save edit" })).toBeInTheDocument();
    expect(screen.getByLabelText<HTMLTextAreaElement>("Message").value).toBe("original body");
  });

  it("Esc cancels the pending reply and keeps the typed draft (cancel returns null)", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onCancelPending = vi.fn().mockReturnValue(null);
    render(
      <Composer
        onSend={onSend}
        pending={{ mode: "reply", targetKey: "k1", sender: "Bob", bodyPreview: "hi" }}
        onCancelPending={onCancelPending}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "my reply text" } });
    fireEvent.keyDown(textarea, { key: "Escape" });
    expect(onCancelPending).toHaveBeenCalled();
    // Reply keeps the typed draft.
    expect(textarea.value).toBe("my reply text");
  });

  it("Esc cancels a pending edit and restores the pre-edit draft", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onCancelPending = vi.fn();
    const { rerender } = render(<Composer onSend={onSend} onCancelPending={onCancelPending} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    // The user has a half-typed draft, then enters edit mode (the parent sets
    // pending + supplies the target body to prefill).
    fireEvent.change(textarea, { target: { value: "half-typed" } });
    rerender(
      <Composer
        onSend={onSend}
        onCancelPending={onCancelPending}
        pending={{ mode: "edit", targetKey: "k2" }}
        editPrefill="the body"
      />,
    );
    expect(textarea.value).toBe("the body");
    fireEvent.keyDown(textarea, { key: "Escape" });
    expect(onCancelPending).toHaveBeenCalled();
    // The pre-edit draft is restored, not lost.
    expect(textarea.value).toBe("half-typed");
  });

  it("↑ in an empty composer with no pending calls onEmptyArrowUp", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onEmptyArrowUp = vi.fn();
    render(<Composer onSend={onSend} onEmptyArrowUp={onEmptyArrowUp} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(onEmptyArrowUp).toHaveBeenCalled();
  });

  it("↑ does not fire onEmptyArrowUp when the draft is non-empty", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onEmptyArrowUp = vi.fn();
    render(<Composer onSend={onSend} onEmptyArrowUp={onEmptyArrowUp} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "typed" } });
    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(onEmptyArrowUp).not.toHaveBeenCalled();
  });
});
