/**
 * Which document a capture window renders (Story 45.15, FR-191).
 *
 * **This file is a door.** The capture window's chrome has its own suite and
 * every test in it enters through a window that already exists; the note item
 * has its own and enters through a menu. Nothing else exercises the decision
 * made here — the one that turns a URL into either the prewarmed page or
 * somebody's existing note — and that decision is the whole of "any note
 * openable as a capture window" on the receiving side.
 *
 * The document components are mocked, deliberately: mounting the real
 * `NoteEditor` would test Story 45.14 with this story's fixtures. What is
 * asserted instead is the **prop boundary** — that each branch hands its own
 * note, and its own capture key, to the thing below it.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const draftProps = vi.fn<(props: { captureKey: string }) => void>();
const documentProps = vi.fn<(props: { vaultId: string; noteId: string }) => void>();
const dismiss = vi.fn();

vi.mock("@/components/capture/capture-document", () => ({
  CaptureDraftDocument: (props: {
    captureKey: string;
    chrome?: (dismiss: () => void) => ReactNode;
  }) => {
    draftProps({ captureKey: props.captureKey });
    return (
      <div data-testid={`draft-${props.captureKey}`}>
        {/* The slot is invoked with the document's own dismissal, which is the
            entire reason the prop exists — a chrome that never receives it
            would render a close button wired to nothing. */}
        {props.chrome?.(dismiss)}
      </div>
    );
  },
  CaptureDocument: (props: { vaultId: string; noteId: string }) => {
    documentProps(props);
    return <div data-testid={`document-${props.vaultId}-${props.noteId}`} />;
  },
}));

const notesCaptureWindows = vi.fn<() => Promise<unknown[]>>();
vi.mock("@/lib/ipc/client", () => ({
  notesCaptureWindows: () => notesCaptureWindows(),
  notesCaptureOpen: vi.fn(),
  notesCaptureClose: vi.fn(),
  notesCaptureSetLocked: vi.fn(),
  listenNotesCaptureWindows: () => Promise.resolve(() => {}),
}));

vi.mock("@/hooks/use-notes-body", () => ({
  saveOpenNote: () => Promise.resolve(),
}));

import { CapturePanel } from "@/capture-main";
import { CAPTURE_CLOSE_LABEL } from "@/components/capture/capture-window";
import { DRAFT_CAPTURE_KEY } from "@/lib/capture-target";
import { resetCaptureWindowsStoreForTest } from "@/lib/stores/capture-windows";

beforeEach(() => {
  vi.clearAllMocks();
  resetCaptureWindowsStoreForTest();
  notesCaptureWindows.mockResolvedValue([]);
});

describe("CapturePanel", () => {
  it("renders the prewarmed page when the window names no note", () => {
    render(<CapturePanel search="" />);
    expect(screen.getByTestId(`draft-${DRAFT_CAPTURE_KEY}`)).toBeInTheDocument();
    expect(draftProps).toHaveBeenCalledWith({ captureKey: DRAFT_CAPTURE_KEY });
    expect(documentProps).not.toHaveBeenCalled();
  });

  it("gives the prewarmed page a close button wired to its own dismissal", () => {
    // The story's first sentence: a close button, not only Escape — and the
    // SAME act. Asserted by pressing it and checking the document's dismissal
    // ran, not by finding a button.
    render(<CapturePanel search="" />);
    fireEvent.click(screen.getByRole("button", { name: CAPTURE_CLOSE_LABEL }));
    expect(dismiss).toHaveBeenCalledTimes(1);
  });

  it("renders the note the window was opened on", () => {
    render(<CapturePanel search="?vault=vault-a&note=note-1" />);
    expect(documentProps).toHaveBeenCalledWith({ vaultId: "vault-a", noteId: "note-1" });
    expect(draftProps).not.toHaveBeenCalled();
  });

  it("carries a name with a space and a slash through the URL intact", () => {
    // The composer is Rust's and the parser is here, so this is the half of
    // that seam this document owns. A vault called `my notes` losing its space
    // resolves to nothing and renders "not found" on a note the person just
    // asked keeper to open.
    render(<CapturePanel search="?vault=my%20notes&note=sub%2Fdir%2Fn3.md" />);
    expect(documentProps).toHaveBeenCalledWith({
      vaultId: "my notes",
      noteId: "sub/dir/n3.md",
    });
  });

  it("falls back to the prewarmed page rather than guessing half a target", () => {
    // A note id is unique only inside its vault. Opening SOME note under this
    // note's name is the one outcome worse than opening the draft.
    for (const search of ["?note=note-1", "?vault=vault-a", "?vault=&note=note-1"]) {
      draftProps.mockClear();
      documentProps.mockClear();
      const view = render(<CapturePanel search={search} />);
      expect(draftProps).toHaveBeenCalledWith({ captureKey: DRAFT_CAPTURE_KEY });
      expect(documentProps).not.toHaveBeenCalled();
      view.unmount();
    }
  });
});
