import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Composer } from "@/components/chat/composer";
import { attachmentsStore } from "@/lib/stores/attachments";
import { composerStore } from "@/lib/stores/composer";
import { draftsStore } from "@/lib/stores/drafts";

// Mock the native file picker; each test sets its return value.
const open = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => open(...args),
}));

// The composer persists drafts through the typed IPC client (Story 7.1); mock those
// so tests run without a live Tauri backend. `loadDraft` resolves `null` (no stored
// draft) by default; individual tests override it.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    loadDraft: vi.fn(async () => null),
    saveDraft: vi.fn(async () => {}),
    clearDraft: vi.fn(async () => {}),
  };
});

import { clearDraft, loadDraft, saveDraft } from "@/lib/ipc/client";

/** Reset the shared pending-attachment tray between tests. */
beforeEach(() => {
  attachmentsStore.getState().clear();
  composerStore.setState({ focusNonce: 0 });
  draftsStore.getState().clear();
  open.mockReset();
  vi.mocked(loadDraft).mockResolvedValue(null);
  vi.mocked(saveDraft).mockClear();
  vi.mocked(clearDraft).mockClear();
});
afterEach(() => {
  attachmentsStore.getState().clear();
  draftsStore.getState().clear();
});

describe("Composer", () => {
  it("sends the trimmed body on Enter and clears the draft", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
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
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "line one" } });
    fireEvent.keyDown(textarea, { key: "Enter", shiftKey: true });

    expect(onSend).not.toHaveBeenCalled();
  });

  it("ignores a whitespace-only body", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "   \n\t " } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    expect(onSend).not.toHaveBeenCalled();
  });

  it("sends on the send button click", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText("Message");

    fireEvent.change(textarea, { target: { value: "click send" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith("click send");
    });
  });

  it("keeps the draft and surfaces an inline error when the send rejects", async () => {
    const onSend = vi.fn().mockRejectedValue(new Error("nope"));
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
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
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "boom" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await screen.findByRole("alert");

    fireEvent.change(textarea, { target: { value: "boom edited" } });
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("is inert when disabled", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        disabled
      />,
    );
    const textarea = screen.getByLabelText("Message");

    expect(textarea).toBeDisabled();
    fireEvent.keyDown(textarea, { key: "Enter" });
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("disables the send button for an empty draft", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    expect(screen.getByRole("button", { name: "Send message" })).toBeDisabled();
  });

  it("renders a reply banner with the quoted sender/preview", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
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
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
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
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
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
    const { rerender } = render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        onCancelPending={onCancelPending}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    // The user has a half-typed draft, then enters edit mode (the parent sets
    // pending + supplies the target body to prefill).
    fireEvent.change(textarea, { target: { value: "half-typed" } });
    rerender(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
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
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        onEmptyArrowUp={onEmptyArrowUp}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(onEmptyArrowUp).toHaveBeenCalled();
  });

  it("↑ does not fire onEmptyArrowUp when the draft is non-empty", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onEmptyArrowUp = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        onEmptyArrowUp={onEmptyArrowUp}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "typed" } });
    fireEvent.keyDown(textarea, { key: "ArrowUp" });
    expect(onEmptyArrowUp).not.toHaveBeenCalled();
  });

  it("adds a chip when a file is picked via the attach button", async () => {
    open.mockResolvedValue("/home/alice/photo.png");
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await waitFor(() => {
      expect(screen.getByText("photo.png")).toBeInTheDocument();
    });
    expect(open).toHaveBeenCalledWith({ multiple: true });
  });

  it("is a no-op when the attach dialog is cancelled", async () => {
    open.mockResolvedValue(null);
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await waitFor(() => {
      expect(open).toHaveBeenCalled();
    });
    expect(attachmentsStore.getState().pending).toHaveLength(0);
  });

  it("adds a chip when an image is pasted", async () => {
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={vi.fn()}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    const file = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
    fireEvent.paste(textarea, {
      clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
    });
    await waitFor(() => {
      expect(screen.getByText("shot.png")).toBeInTheDocument();
    });
    const pending = attachmentsStore.getState().pending;
    expect(pending).toHaveLength(1);
    expect(pending[0].kind).toBe("bytes");
  });

  it("lets a non-image paste fall through to text (no chip)", () => {
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={vi.fn()}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.paste(textarea, {
      clipboardData: { items: [{ type: "text/plain", getAsFile: () => null }] },
    });
    expect(attachmentsStore.getState().pending).toHaveLength(0);
  });

  it("removes a chip when its × is clicked (pre-upload cancel)", async () => {
    open.mockResolvedValue("/home/alice/doc.pdf");
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await waitFor(() => {
      expect(screen.getByText("doc.pdf")).toBeInTheDocument();
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove doc.pdf" }));
    expect(screen.queryByText("doc.pdf")).not.toBeInTheDocument();
    expect(attachmentsStore.getState().pending).toHaveLength(0);
  });

  it("Send dispatches a single path attachment with the typed text as caption", async () => {
    open.mockResolvedValue("/home/alice/photo.png");
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onSendAttachments = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        onSendAttachments={onSendAttachments}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await waitFor(() => expect(screen.getByText("photo.png")).toBeInTheDocument());
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "look at this" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(onSendAttachments).toHaveBeenCalledWith(
        [expect.objectContaining({ kind: "path", path: "/home/alice/photo.png" })],
        "look at this",
      );
    });
    // The text rode as the caption, so no separate text send.
    expect(onSend).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(attachmentsStore.getState().pending).toHaveLength(0);
    });
    expect(textarea.value).toBe("");
  });

  it("Send dispatches a pasted-bytes attachment via onSendAttachments", async () => {
    const onSendAttachments = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={onSendAttachments}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    const file = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
    fireEvent.paste(textarea, {
      clipboardData: { items: [{ type: "image/png", getAsFile: () => file }] },
    });
    await waitFor(() => expect(screen.getByText("shot.png")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(onSendAttachments).toHaveBeenCalledWith(
        [expect.objectContaining({ kind: "bytes", mime: "image/png" })],
        undefined,
      );
    });
  });

  it("keeps the tray when an attachment send rejects", async () => {
    open.mockResolvedValue("/home/alice/photo.png");
    const onSendAttachments = vi.fn().mockRejectedValue(new Error("nope"));
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onSendAttachments={onSendAttachments}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Attach file" }));
    await waitFor(() => expect(screen.getByText("photo.png")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => {
      expect(screen.getByRole("alert")).toBeInTheDocument();
    });
    // The tray is kept for retry.
    expect(attachmentsStore.getState().pending).toHaveLength(1);
  });
});

describe("Composer typing notices (Story 3.9)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.runOnlyPendingTimers();
    vi.useRealTimers();
  });

  it("emits setTyping(true) on non-empty input, throttled to at most once per 3s", () => {
    const onTyping = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onTyping={onTyping}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "h" } });
    fireEvent.change(textarea, { target: { value: "he" } });
    fireEvent.change(textarea, { target: { value: "hel" } });
    // Throttled: only one `true` within the 3s window despite three keystrokes.
    expect(onTyping.mock.calls.filter(([t]) => t === true)).toHaveLength(1);
  });

  it("stops typing (setTyping(false)) after ~5s idle", () => {
    const onTyping = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onTyping={onTyping}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "typing" } });
    expect(onTyping).toHaveBeenLastCalledWith(true);
    vi.advanceTimersByTime(5000);
    expect(onTyping).toHaveBeenLastCalledWith(false);
  });

  it("stops typing immediately when the draft is cleared to empty", () => {
    const onTyping = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onTyping={onTyping}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "hi" } });
    onTyping.mockClear();
    fireEvent.change(textarea, { target: { value: "" } });
    expect(onTyping).toHaveBeenCalledWith(false);
  });

  it("stops typing on blur", () => {
    const onTyping = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={vi.fn()}
        onTyping={onTyping}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "hi" } });
    onTyping.mockClear();
    fireEvent.blur(textarea);
    expect(onTyping).toHaveBeenCalledWith(false);
  });

  it("stops typing when the message is sent", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    const onTyping = vi.fn();
    render(
      <Composer
        accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV"
        roomId="!r1:example.org"
        onSend={onSend}
        onTyping={onTyping}
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "hello" } });
    onTyping.mockClear();
    fireEvent.keyDown(textarea, { key: "Enter" });
    // The stop fires synchronously at the start of send, before the async dispatch.
    expect(onTyping).toHaveBeenCalledWith(false);
  });

  it("focuses the textarea when the composer store's focus nonce is bumped", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    // The textarea is not focused until a focus is requested (Story 6.6).
    expect(document.activeElement).not.toBe(textarea);
    await act(async () => {
      composerStore.getState().requestFocus();
    });
    expect(document.activeElement).toBe(textarea);
  });

  it("does not steal focus when it mounts onto an already-bumped focus nonce", () => {
    // Simulate a prior new-chat: the persisted nonce is already > 0 when a fresh
    // Composer mounts (every room switch remounts the pane). It must NOT self-focus.
    composerStore.setState({ focusNonce: 5 });
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(
      <Composer accountId="01ARZ3NDEKTSV4RRFFQ69G5FAV" roomId="!r1:example.org" onSend={onSend} />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    expect(document.activeElement).not.toBe(textarea);
  });
});

describe("Composer persistent drafts (Story 7.1)", () => {
  const ACCT = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
  const ROOM = "!r1:example.org";

  it("restores a stored draft into the textarea on mount", async () => {
    vi.mocked(loadDraft).mockResolvedValue("half a message");
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => {
      expect(textarea.value).toBe("half a message");
    });
    expect(loadDraft).toHaveBeenCalledWith(ACCT, ROOM);
  });

  it("does not restore a draft when entering edit mode (prefill wins)", async () => {
    vi.mocked(loadDraft).mockResolvedValue("stored draft");
    render(
      <Composer
        accountId={ACCT}
        roomId={ROOM}
        onSend={vi.fn()}
        pending={{ mode: "edit", targetKey: "k2" }}
        editPrefill="the edit body"
      />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    // Give the async load a chance to resolve; the edit prefill must still win.
    await waitFor(() => expect(loadDraft).toHaveBeenCalled());
    expect(textarea.value).toBe("the edit body");
  });

  it("debounces a keystroke persist and marks the draft present at once", () => {
    vi.useFakeTimers();
    try {
      render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
      const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
      fireEvent.change(textarea, { target: { value: "draft body" } });

      // The DB write is debounced — nothing persisted yet.
      expect(saveDraft).not.toHaveBeenCalled();
      // The inbox marker flips immediately (no debounce on presence).
      expect(draftsStore.getState().keys.has(`${ACCT} ${ROOM}`)).toBe(true);

      vi.advanceTimersByTime(200);
      expect(saveDraft).toHaveBeenCalledWith(ACCT, ROOM, "draft body");
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });

  it("clears the draft (row + marker) when the body trims to empty", () => {
    vi.useFakeTimers();
    try {
      render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
      const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
      fireEvent.change(textarea, { target: { value: "typed" } });
      fireEvent.change(textarea, { target: { value: "   " } });
      expect(draftsStore.getState().keys.has(`${ACCT} ${ROOM}`)).toBe(false);

      vi.advanceTimersByTime(200);
      expect(clearDraft).toHaveBeenCalledWith(ACCT, ROOM);
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });

  it("flushes the pending debounced save on unmount (room switch)", () => {
    vi.useFakeTimers();
    try {
      const { unmount } = render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
      const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
      fireEvent.change(textarea, { target: { value: "unsaved on switch" } });
      // Unmount before the debounce fires — the flush must persist the latest body.
      unmount();
      expect(saveDraft).toHaveBeenCalledWith(ACCT, ROOM, "unsaved on switch");
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });

  it("clears the draft row + marker on a successful send", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "send me" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("send me"));
    await waitFor(() => expect(clearDraft).toHaveBeenCalledWith(ACCT, ROOM));
    expect(draftsStore.getState().keys.has(`${ACCT} ${ROOM}`)).toBe(false);
  });

  it("editing an existing message never overwrites or clears the room's persistent draft", async () => {
    // A stored draft loads into the composer on open.
    vi.mocked(loadDraft).mockResolvedValue("real draft");
    const onSend = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <Composer accountId={ACCT} roomId={ROOM} onSend={onSend} pending={null} />,
    );
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(textarea.value).toBe("real draft"));
    // The startup seed lit this room's marker (modelled here directly).
    draftsStore.getState().mark(ACCT, ROOM, true);
    vi.mocked(saveDraft).mockClear();
    vi.mocked(clearDraft).mockClear();

    // Enter edit mode: the parent supplies the edit prefill; the pre-edit draft is
    // stashed and the edit body replaces the textarea.
    rerender(
      <Composer
        accountId={ACCT}
        roomId={ROOM}
        onSend={onSend}
        pending={{ mode: "edit", targetKey: "k9" }}
        editPrefill="old message body"
      />,
    );
    expect(textarea.value).toBe("old message body");

    // Type into the edit and save it.
    fireEvent.change(textarea, { target: { value: "old message body v2" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalledWith("old message body v2"));

    // The persistent draft is pre-send state: an edit must not persist the edit body
    // nor delete the stored draft row.
    expect(saveDraft).not.toHaveBeenCalled();
    expect(clearDraft).not.toHaveBeenCalled();
    // After the edit sends, the pre-edit real draft returns to the composer.
    await waitFor(() => expect(textarea.value).toBe("real draft"));
    expect(draftsStore.getState().keys.has(`${ACCT} ${ROOM}`)).toBe(true);
  });
});
