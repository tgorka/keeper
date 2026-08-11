/**
 * A capture window's chrome (Story 45.15, FR-191, FR-192, UX-DR77).
 *
 * Two things this file is careful about, both from wave 2's audit:
 *
 * - **Assert the call, not only the render.** Every control here ends in an IPC
 *   call carrying a key, and a mock resolves the same value whatever key it is
 *   handed. A test that presses a button and checks the button is checking the
 *   button.
 * - **Two windows in the fixture, always.** The story's headline is *several*
 *   capture windows, and a mutation that hands every control the first
 *   window's key passes every single-window test while making the second
 *   window's close button close somebody else's window.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CaptureWindowVm } from "@/lib/ipc/client";

const notesCaptureWindows = vi.fn<() => Promise<CaptureWindowVm[]>>();
const notesCaptureOpen = vi.fn<(target: unknown) => Promise<void>>();
const notesCaptureClose = vi.fn<(key: string) => Promise<void>>();
const notesCaptureSetLocked = vi.fn<(key: string, locked: boolean) => Promise<void>>();
const listenNotesCaptureWindows = vi.fn<(onChanged: () => void) => Promise<() => void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesCaptureWindows: () => notesCaptureWindows(),
  notesCaptureOpen: (target: unknown) => notesCaptureOpen(target),
  notesCaptureClose: (key: string) => notesCaptureClose(key),
  notesCaptureSetLocked: (key: string, locked: boolean) => notesCaptureSetLocked(key, locked),
  listenNotesCaptureWindows: (onChanged: () => void) => listenNotesCaptureWindows(onChanged),
}));

/**
 * Story 46.12: the save is addressed to a note. The spy takes the pair, so this
 * file can assert that the window closing is the one whose note was written —
 * which is the prop boundary it exists to guard, one level down.
 */
const saveNote = vi.fn<(vaultId: string, noteId: string) => Promise<boolean>>();
vi.mock("@/hooks/use-notes-body", () => ({
  saveNote: (vaultId: string, noteId: string) => saveNote(vaultId, noteId),
}));

/**
 * The document is somebody else's component (Story 45.14) and mounting the real
 * `NoteEditor` here would test their story with this story's fixtures. What
 * this file owes is the PROP BOUNDARY: that each window hands its own note
 * down, which is the half a rendered editor would hide rather than reveal.
 */
const documentProps = vi.fn<(props: { vaultId: string; noteId: string }) => void>();
vi.mock("@/components/capture/capture-document", () => ({
  CaptureDocument: (props: { vaultId: string; noteId: string }) => {
    documentProps(props);
    return <div data-testid={`document-${props.vaultId}-${props.noteId}`} />;
  },
}));

import {
  CAPTURE_CLOSE_LABEL,
  CAPTURE_LOCK_LABEL,
  CAPTURE_UNLOCK_LABEL,
  CaptureNoteWindow,
  CaptureWindowChrome,
  useCaptureDismissKeys,
} from "@/components/capture/capture-window";
import { resetCaptureWindowsStoreForTest } from "@/lib/stores/capture-windows";

const FIRST: CaptureWindowVm = {
  key: "note:v1/n1",
  target: { kind: "note", vaultId: "v1", noteId: "n1" },
  locked: true,
  visible: true,
  // Locked, so tao hit-tests no resize edge and there is no border to dodge.
  chromeInset: 0,
};

const SECOND: CaptureWindowVm = {
  key: "note:v1/n2",
  target: { kind: "note", vaultId: "v1", noteId: "n2" },
  locked: false,
  visible: true,
  // Unlocked on a 2x GTK display: `scale_factor() * 5`.
  chromeInset: 10,
};

beforeEach(() => {
  vi.clearAllMocks();
  resetCaptureWindowsStoreForTest();
  notesCaptureWindows.mockResolvedValue([FIRST, SECOND]);
  notesCaptureClose.mockResolvedValue(undefined);
  notesCaptureSetLocked.mockResolvedValue(undefined);
  saveNote.mockResolvedValue(true);
  listenNotesCaptureWindows.mockResolvedValue(() => {});
});

describe("CaptureWindowChrome", () => {
  it("dismisses through the act it was handed, not through one of its own", async () => {
    // The close button and Escape must be the same act, and the only way to
    // guarantee that is for the strip not to own one. Rendered with a real
    // callback and pressed, rather than checked for existence.
    const onClose = vi.fn();
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={onClose} />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    expect(onClose).toHaveBeenCalledTimes(1);
    // And it decides nothing about what dismissal means.
    expect(notesCaptureClose).not.toHaveBeenCalled();
  });

  it("locks and unlocks the window it belongs to, with the value it is toggling to", async () => {
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    // FIRST is locked, so the control offers to unlock — the label is the state
    // it moves to, which is what a person reads before pressing.
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_UNLOCK_LABEL }));
    // Both arguments asserted: passing the current value instead of the next
    // one is a lock button that presses and changes nothing.
    expect(notesCaptureSetLocked).toHaveBeenCalledWith("note:v1/n1", false);
  });

  it("reads its own row out of a list holding several windows", async () => {
    // Two windows in two states. A chrome that read the first row regardless of
    // its key would show this window as locked and offer the wrong verb.
    render(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_LOCK_LABEL }));
    expect(notesCaptureSetLocked).toHaveBeenCalledWith("note:v1/n2", true);
  });

  it("makes the strip a drag region only while the window is unlocked", async () => {
    // The drag region IS the unlocked window's mechanism — an undecorated
    // window has no title bar — so its presence is the feature and its absence
    // is the lock. A locked strip that dragged would move when the user aimed
    // at the close button.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    expect(screen.getByTestId("capture-window-chrome")).not.toHaveAttribute(
      "data-tauri-drag-region",
    );
    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    expect(screen.getByTestId("capture-window-chrome")).toHaveAttribute("data-tauri-drag-region");
  });

  it("keeps the buttons out of the window's own resize border, by the number Rust measured", async () => {
    // DW-199. On GTK an unlocked undecorated window's resize edges are
    // hit-tested inside the surface, and the close button is flush into the
    // corner where two of those strips overlap — so aiming at close starts a
    // resize. The strip is inset by exactly what the shell measured, and by
    // nothing when there is no border there.
    const { rerender } = render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_UNLOCK_LABEL });
    // Locked: no inset at all, not "an inset of zero pixels" — a gutter on a
    // window with no resize border is a control moved for nothing.
    expect(screen.getByTestId("capture-window-chrome")).not.toHaveAttribute("style");

    rerender(<CaptureWindowChrome captureKey="note:v1/n2" onClose={() => {}} />);
    await screen.findByRole("button", { name: CAPTURE_LOCK_LABEL });
    const strip = screen.getByTestId("capture-window-chrome");
    // 10, not 5: the number is `scale_factor() * 5` and this fixture is a 2x
    // display. A component that hard-coded the constant would pass a 1x test
    // and leave half the border over the close button on the owner's hardware.
    expect(strip).toHaveStyle({ paddingTop: "10px" });
    // Added to the strip's existing `px-1` rather than replacing it, so the
    // buttons clear the border AND keep the padding they always had. Matched
    // on the terms rather than on the string: the CSSOM reorders `calc`
    // operands, and which side of the plus each lands on is not the contract.
    expect(strip.style.paddingRight).toMatch(/^calc\(/);
    expect(strip.style.paddingRight).toContain("0.25rem");
    expect(strip.style.paddingRight).toContain("10px");
  });

  it("behaves as locked before Rust has answered", async () => {
    // Unknown must not render a live drag region for a frame: a click aimed at
    // the close button would move the window instead.
    // A read that never resolves, so the store stays at `null` for the whole
    // test rather than for one tick. The executor form rather than
    // `Promise.withResolvers`: this tsconfig's `lib` predates it, and the
    // resolvers would go unused anyway — the point is that nothing settles.
    notesCaptureWindows.mockReturnValue(new Promise<CaptureWindowVm[]>(() => {}));
    render(<CaptureWindowChrome captureKey="note:v1/n1" onClose={() => {}} />);
    expect(screen.getByRole("button", { name: CAPTURE_UNLOCK_LABEL })).toBeInTheDocument();
    expect(screen.getByTestId("capture-window-chrome")).not.toHaveAttribute(
      "data-tauri-drag-region",
    );
  });
});

describe("useCaptureDismissKeys", () => {
  function Harness({ onDismiss }: { onDismiss: () => void }) {
    useCaptureDismissKeys(onDismiss);
    return <input aria-label="field" />;
  }

  it("dismisses on Escape and on Ctrl/Cmd+W", () => {
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "Escape" });
    // `ctrlKey`, not `metaKey`: jsdom matches nothing on the latter, and this
    // app reads the platform nowhere.
    fireEvent.keyDown(window, { key: "w", ctrlKey: true });
    expect(onDismiss).toHaveBeenCalledTimes(2);
  });

  it("leaves an Escape somebody else has handled alone", () => {
    // CodeMirror marks the event handled when Escape closes the `/` menu, the
    // tag chooser or the emoji chooser. Without this guard, dismissing a
    // completion popup destroys the window the person is working in.
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    const handled = new KeyboardEvent("keydown", { key: "Escape", cancelable: true });
    handled.preventDefault();
    window.dispatchEvent(handled);
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("ignores every other key", () => {
    const onDismiss = vi.fn();
    render(<Harness onDismiss={onDismiss} />);
    fireEvent.keyDown(window, { key: "w" });
    fireEvent.keyDown(window, { key: "q", ctrlKey: true });
    fireEvent.keyDown(window, { key: "Enter" });
    expect(onDismiss).not.toHaveBeenCalled();
  });
});

describe("CaptureNoteWindow", () => {
  it("hands its own note to the document, for each of two windows at once", async () => {
    // The acceptance criterion in the form one realm can express: two capture
    // windows, two different notes, neither reading the other's. Asserted on
    // both, because a component that always renders the first would satisfy a
    // single-window test.
    render(
      <>
        <CaptureNoteWindow vaultId="v1" noteId="n1" />
        <CaptureNoteWindow vaultId="v1" noteId="n2" />
      </>,
    );
    expect(screen.getByTestId("document-v1-n1")).toBeInTheDocument();
    expect(screen.getByTestId("document-v1-n2")).toBeInTheDocument();
    expect(documentProps).toHaveBeenCalledWith({ vaultId: "v1", noteId: "n1" });
    expect(documentProps).toHaveBeenCalledWith({ vaultId: "v1", noteId: "n2" });
  });

  it("waits for the save to LAND before it closes", async () => {
    // Invocation order is not the contract and asserting it was a hole: `void
    // saveNote()` starts the save first and still lets the close fire while
    // the write is in flight, which passed an order assertion and survived a
    // mutation. This window is DESTROYED rather than hidden, so a write still
    // travelling when the webview goes away is the last 1.5 s of typing lost
    // (AD-62). What has to be true is that the save has RESOLVED.
    // Held in a one-slot box rather than a `let`: TypeScript narrows a `let`
    // initialised to `null` to `null` at the call site below, because the
    // assignment happens inside a callback it cannot order.
    const landed: { resolve: ((ok: boolean) => void) | null } = { resolve: null };
    saveNote.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          landed.resolve = resolve;
        }),
    );
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      // Story 46.12: the write that gates this close is THIS window's note.
      expect(saveNote).toHaveBeenCalledExactlyOnceWith("v1", "n2");
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
    landed.resolve?.(true);
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/n2");
    });
  });

  it("does NOT close when the write was refused", async () => {
    // W3NoteFile's shape, and this window is where it bites hardest: the
    // prewarmed window merely hides on a refused write, so the words survive in
    // a buffer on a page that is handed back. This one is DESTROYED. Closing it
    // over a write Rust refused takes the webview, the buffer and the unsaved
    // text with it, and says nothing — because the only surface that could have
    // said anything is the one that just vanished.
    saveNote.mockResolvedValue(false);
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      expect(saveNote).toHaveBeenCalledTimes(1);
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
    // Still on screen, with the words still in it. The reason is already
    // rendered by the editor, which reads the same store `markSaveFailed` wrote.
    expect(screen.getByTestId("document-v1-n2")).toBeInTheDocument();
    // And Escape does not get a second bite at throwing them away either.
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(saveNote).toHaveBeenCalledTimes(2);
    });
    expect(notesCaptureClose).not.toHaveBeenCalled();
  });

  it("closes the window Escape was pressed in, not the other one", async () => {
    render(<CaptureNoteWindow vaultId="v1" noteId="n2" />);
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/n2");
    });
    expect(notesCaptureClose).toHaveBeenCalledTimes(1);
  });

  it("keys itself the way Rust does, including an id with a slash in it", async () => {
    // A note id is derived from a path, so a slash in one is ordinary — and an
    // unescaped one would make this window ask about a different note's
    // placement.
    render(<CaptureNoteWindow vaultId="v1" noteId="sub/dir/n3" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    await waitFor(() => {
      expect(notesCaptureClose).toHaveBeenCalledWith("note:v1/sub%2Fdir%2Fn3");
    });
  });
});
