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
    // Cross-device mirror (Story 7.2): default to no remote draft; individual tests
    // override `loadRemoteDraft`. The write/clear mirror calls are best-effort no-ops.
    loadRemoteDraft: vi.fn(async () => null),
    mirrorDraft: vi.fn(async () => {}),
    clearDraftMirror: vi.fn(async () => {}),
  };
});

import {
  clearDraft,
  clearDraftMirror,
  loadDraft,
  loadRemoteDraft,
  mirrorDraft,
  saveDraft,
} from "@/lib/ipc/client";

/** Reset the shared pending-attachment tray between tests. */
beforeEach(() => {
  attachmentsStore.getState().clear();
  composerStore.setState({ focusNonce: 0 });
  draftsStore.getState().clear();
  open.mockReset();
  vi.mocked(loadDraft).mockResolvedValue(null);
  vi.mocked(saveDraft).mockClear();
  vi.mocked(clearDraft).mockClear();
  vi.mocked(loadRemoteDraft).mockResolvedValue(null);
  vi.mocked(mirrorDraft).mockClear();
  vi.mocked(clearDraftMirror).mockClear();
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

describe("Composer cross-device mirror (Story 7.2)", () => {
  const ACCT = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
  const ROOM = "!r1:example.org";

  it("adopts the remote draft into an empty, untouched composer on open", async () => {
    // No local draft; a remote draft is present → it follows the user.
    vi.mocked(loadDraft).mockResolvedValue(null);
    vi.mocked(loadRemoteDraft).mockResolvedValue({ body: "from device B", updatedTs: 100 });
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(textarea.value).toBe("from device B"));
    // No conflict chip when the composer was empty — adoption is silent.
    expect(screen.queryByText(/edited on another device/i)).toBeNull();
  });

  it("keeps local text and offers a conflict chip when the remote differs (local wins)", async () => {
    // A stored local draft loads; a differing remote draft is present.
    vi.mocked(loadDraft).mockResolvedValue("my local text");
    vi.mocked(loadRemoteDraft).mockResolvedValue({ body: "remote version", updatedTs: 200 });
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    // Local text stays put — never overwritten by the remote.
    await waitFor(() => expect(textarea.value).toBe("my local text"));
    const chip = await screen.findByText(/edited on another device/i);
    expect(chip).toBeInTheDocument();
    // Tapping "Use that version" adopts the remote body into the composer.
    fireEvent.click(screen.getByRole("button", { name: /use that version/i }));
    await waitFor(() => expect(textarea.value).toBe("remote version"));
    // The chip is dismissed after adoption.
    expect(screen.queryByText(/edited on another device/i)).toBeNull();
  });

  it("shows no chip when the remote equals the local draft", async () => {
    vi.mocked(loadDraft).mockResolvedValue("same text");
    vi.mocked(loadRemoteDraft).mockResolvedValue({ body: "same text", updatedTs: 300 });
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(textarea.value).toBe("same text"));
    // Equal bodies → nothing to reconcile, no chip.
    expect(screen.queryByText(/edited on another device/i)).toBeNull();
  });

  it("raises the conflict chip on a live remote edit while composing (local untouched)", async () => {
    vi.mocked(loadDraft).mockResolvedValue(null);
    vi.mocked(loadRemoteDraft).mockResolvedValue(null);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    // The user types local text (touches the composer).
    fireEvent.change(textarea, { target: { value: "typed locally" } });
    // A live remote edit arrives via the app-wide mirror subscription.
    act(() => {
      draftsStore.getState().applyRemote(ACCT, ROOM, "live remote edit", 400);
    });
    // Local text is untouched; the chip offers the remote for one-tap adoption.
    expect(textarea.value).toBe("typed locally");
    expect(await screen.findByText(/edited on another device/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /use that version/i }));
    await waitFor(() => expect(textarea.value).toBe("live remote edit"));
  });

  it("mirrors a draft cross-device on the looser debounce, off the keystroke path", () => {
    vi.useFakeTimers();
    try {
      render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
      const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
      fireEvent.change(textarea, { target: { value: "mirror me" } });
      // The local save fires first (200 ms) — the mirror has NOT yet fired.
      vi.advanceTimersByTime(200);
      expect(mirrorDraft).not.toHaveBeenCalled();
      // The looser mirror debounce (1000 ms total) then writes the mirror.
      vi.advanceTimersByTime(800);
      expect(mirrorDraft).toHaveBeenCalledWith(ACCT, ROOM, "mirror me");
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });

  it("tombstones the mirror on a successful send", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    fireEvent.change(textarea, { target: { value: "send and clear mirror" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() => expect(onSend).toHaveBeenCalled());
    // The cross-device mirror is tombstoned so other devices stop showing the draft.
    await waitFor(() => expect(clearDraftMirror).toHaveBeenCalledWith(ACCT, ROOM));
  });

  it("never raises the conflict chip while editing an existing message", async () => {
    // Edit mode owns the composer (the edit body); a remote draft must not offer a
    // chip over it, and adopting one would corrupt the edit + persistent draft.
    vi.mocked(loadDraft).mockResolvedValue(null);
    vi.mocked(loadRemoteDraft).mockResolvedValue(null);
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
    await waitFor(() => expect(textarea.value).toBe("the edit body"));
    // A live remote draft arrives mid-edit — it must NOT surface the chip.
    act(() => {
      draftsStore.getState().applyRemote(ACCT, ROOM, "remote draft body", 500);
    });
    expect(screen.queryByText(/edited on another device/i)).toBeNull();
    // The edit body is untouched.
    expect(textarea.value).toBe("the edit body");
  });

  it("cancels a queued mirror on send so it cannot resurrect the sent draft", async () => {
    vi.useFakeTimers();
    try {
      const onSend = vi.fn().mockResolvedValue(undefined);
      render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
      const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
      // Type, then send within the looser mirror debounce window (before it fires).
      fireEvent.change(textarea, { target: { value: "racy body" } });
      vi.advanceTimersByTime(200); // local save fired; the mirror is still queued
      expect(mirrorDraft).not.toHaveBeenCalled();
      fireEvent.keyDown(textarea, { key: "Enter" });
      // Flush the async send chain and any surviving timers.
      await vi.runAllTimersAsync();
      // The queued mirror was cancelled before the send — it never writes the body,
      // so it cannot land after and reorder past the post-send tombstone.
      expect(mirrorDraft).not.toHaveBeenCalled();
      // The mirror is tombstoned instead.
      expect(clearDraftMirror).toHaveBeenCalledWith(ACCT, ROOM);
    } finally {
      vi.runOnlyPendingTimers();
      vi.useRealTimers();
    }
  });
});

describe("Composer phone deltas (Story 13.5)", () => {
  const ACCT = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
  const ROOM = "!r1:example.org";

  /**
   * Mock matchMedia at a phone-tier width (mirrors phone-shell.test.tsx): any
   * `max-width: <bp>` query matches when the simulated viewport is below it, so
   * `useShellLayout().phone` reads true at 390px and false at 1024px.
   */
  const originalMatchMedia = window.matchMedia;
  function mockViewportWidth(width: number) {
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        matches: width <= maxWidth,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      };
    });
  }

  beforeEach(() => {
    mockViewportWidth(390);
  });
  afterEach(() => {
    window.matchMedia = originalMatchMedia;
  });

  it("phone: the on-screen return inserts a newline and never sends (button-only send)", () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    fireEvent.change(textarea, { target: { value: "line one" } });
    const enter = fireEvent.keyDown(textarea, { key: "Enter" });

    // The send branch is skipped: nothing dispatched, and the default (newline
    // insertion) is not prevented — fireEvent returns false when a handler
    // called preventDefault.
    expect(onSend).not.toHaveBeenCalled();
    expect(enter).toBe(true);
  });

  it("phone: the send button tap sends (FR-41 trigger) and carries the ≥44pt sizing", async () => {
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    const sendButton = screen.getByRole("button", { name: "Send message" });

    // ≥44pt hit target (h-11 = 44px), primary-tinted (the default variant).
    expect(sendButton.className).toContain("h-11");
    expect(sendButton.className).toContain("min-w-11");

    fireEvent.change(textarea, { target: { value: "tap to send" } });
    fireEvent.click(sendButton);
    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith("tap to send");
    });
  });

  it("phone: attach is a ≥44pt + presenting the native picker", async () => {
    open.mockResolvedValue("/home/alice/photo.png");
    render(
      <Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} onSendAttachments={vi.fn()} />,
    );
    const attach = screen.getByRole("button", { name: "Attach file" });
    // ≥44pt hit target rendering the `+` glyph (lucide Plus), not the paperclip.
    expect(attach.className).toContain("size-11");
    expect(attach.querySelector("svg.lucide-plus")).not.toBeNull();
    expect(attach.querySelector("svg.lucide-paperclip")).toBeNull();

    fireEvent.click(attach);
    await waitFor(() => {
      expect(screen.getByText("photo.png")).toBeInTheDocument();
    });
    expect(open).toHaveBeenCalledWith({ multiple: true });
  });

  it("phone: autogrow caps at 5 lines then scrolls", () => {
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={vi.fn()} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");
    expect(textarea.className).toContain("max-h-[calc(5*1.5rem+1rem)]");
    expect(textarea.className).not.toContain("max-h-[calc(8*1.5rem+1rem)]");
  });

  it("desktop (≥768px): Enter sends, default button size, 8-line cap — byte-for-byte", async () => {
    mockViewportWidth(1024);
    const onSend = vi.fn().mockResolvedValue(undefined);
    render(<Composer accountId={ACCT} roomId={ROOM} onSend={onSend} />);
    const textarea = screen.getByLabelText<HTMLTextAreaElement>("Message");

    // No phone sizing classes leak onto the desktop send button or textarea.
    const sendButton = screen.getByRole("button", { name: "Send message" });
    expect(sendButton.className).not.toContain("h-11");
    expect(textarea.className).toContain("max-h-[calc(8*1.5rem+1rem)]");
    expect(textarea.className).not.toContain("max-h-[calc(5*1.5rem+1rem)]");

    fireEvent.change(textarea, { target: { value: "desktop enter" } });
    fireEvent.keyDown(textarea, { key: "Enter" });
    await waitFor(() => {
      expect(onSend).toHaveBeenCalledWith("desktop enter");
    });
  });
});
