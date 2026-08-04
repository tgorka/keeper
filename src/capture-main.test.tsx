import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { resetNotesCaptureStoreForTest } from "@/lib/stores/notes-capture";
import { CapturePanel } from "./capture-main";

const notesCaptureBuffer = vi.fn<() => Promise<string>>();
const notesCaptureBufferSave = vi.fn<(text: string) => Promise<void>>();
const notesCaptureHide = vi.fn<(commit: boolean) => Promise<null>>();

vi.mock("@/lib/ipc/client", () => ({
  notesCaptureBuffer: () => notesCaptureBuffer(),
  notesCaptureBufferSave: (text: string) => notesCaptureBufferSave(text),
  notesCaptureHide: (commit: boolean) => notesCaptureHide(commit),
}));

// The panel re-asserts focus on `keeper://notes-capture-shown`; the listener is
// irrelevant to these assertions, so it resolves to a no-op unsubscribe.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

beforeEach(() => {
  vi.clearAllMocks();
  resetNotesCaptureStoreForTest();
  notesCaptureBuffer.mockResolvedValue("");
  notesCaptureBufferSave.mockResolvedValue(undefined);
  notesCaptureHide.mockResolvedValue(null);
});

describe("CapturePanel", () => {
  it("focuses the textarea on mount so the first keystroke lands", () => {
    render(<CapturePanel />);
    expect(screen.getByLabelText("Quick capture")).toHaveFocus();
  });

  it("commits and hides on Escape, flushing the buffer first", async () => {
    render(<CapturePanel />);
    const field = screen.getByLabelText("Quick capture");
    fireEvent.change(field, { target: { value: "ring the dentist" } });
    fireEvent.keyDown(field, { key: "Escape" });

    await waitFor(() => {
      expect(notesCaptureHide).toHaveBeenCalledWith(true);
    });
    // Ordering is the guarantee: Rust writes what it holds, so the last
    // keystrokes must reach it before the hide that commits them.
    expect(notesCaptureBufferSave).toHaveBeenCalledWith("ring the dentist");
    expect(notesCaptureBufferSave.mock.invocationCallOrder[0]).toBeLessThan(
      notesCaptureHide.mock.invocationCallOrder[0],
    );
  });

  it("restores the persisted buffer on a fresh mount", async () => {
    const { unmount } = render(<CapturePanel />);
    fireEvent.change(screen.getByLabelText("Quick capture"), {
      target: { value: "half a thought" },
    });
    fireEvent.keyDown(screen.getByLabelText("Quick capture"), { key: "Escape" });
    await waitFor(() => {
      expect(notesCaptureBufferSave).toHaveBeenCalledWith("half a thought");
    });
    unmount();

    // A dismissal or a `kill -9` leaves the mirror empty and Rust holding the
    // words; the next summon must paint them, not an empty panel.
    resetNotesCaptureStoreForTest();
    notesCaptureBuffer.mockResolvedValue("half a thought");
    render(<CapturePanel />);

    expect(await screen.findByDisplayValue("half a thought")).toBeInTheDocument();
  });

  it("keeps the text and shows the reason when the write fails", async () => {
    notesCaptureHide.mockRejectedValue({
      code: "internal",
      message: "vault folder isn't there any more",
      accountId: null,
      retriable: false,
    });
    render(<CapturePanel />);
    const field = screen.getByLabelText("Quick capture");
    fireEvent.change(field, { target: { value: "do not lose me" } });
    fireEvent.keyDown(field, { key: "Escape" });

    expect(await screen.findByRole("alert")).toHaveTextContent("vault folder isn't there any more");
    expect(field).toHaveValue("do not lose me");
  });
});
