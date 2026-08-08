import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Mock the typed IPC client so the surface never touches Tauri.
vi.mock("@/lib/ipc/client", () => ({
  recordingNoteStub: vi.fn(),
  recordingNoteStubSave: vi.fn(),
  recordingNoteStubDismiss: vi.fn(),
}));

import {
  NOTE_STUB_BODY_TESTID,
  NOTE_STUB_FAULT_TESTID,
  NOTE_STUB_HINT,
  NOTE_STUB_HINT_TESTID,
  NOTE_STUB_KEPT_TESTID,
  NOTE_STUB_SAVED,
  RecordingNoteStub,
} from "@/components/recording/recording-note-stub";
import type { RecordingNoteStubVm } from "@/lib/ipc/client";
import {
  recordingNoteStub,
  recordingNoteStubDismiss,
  recordingNoteStubSave,
} from "@/lib/ipc/client";

const mockResolve = vi.mocked(recordingNoteStub);
const mockSave = vi.mocked(recordingNoteStubSave);
const mockDismiss = vi.mocked(recordingNoteStubDismiss);

const FOLDER = "/Users/alice/Movies/keeper/2026/2026-08-08 1423 quarterly review";

/** Where a rename moves the session. The stub itself does not move. */
const MOVED_FOLDER = "/Users/alice/Movies/keeper/2026/2026-08-08 1423 retro";

/** The keeper-authored block, verbatim — the em dash is deliberate: it is one
 *  UTF-16 code unit and three UTF-8 bytes, so a split at `bodyOffset` only
 *  lands on the body if the offset really is in code units. */
const FRONT = "---\ntitle: Quarterly review — Q3\nsession: 01JDEVICE-01JSESSION\n---\n";

/** What keeper prefilled the body with. */
const BODY = "# Quarterly review — Q3\n\n";

function stub(p: Partial<RecordingNoteStubVm> = {}): RecordingNoteStubVm {
  const contents = p.contents ?? `${FRONT}${BODY}`;
  return {
    path: p.path ?? "/Users/alice/Movies/keeper/2026/2026-08-08-quarterly-review.md",
    filename: p.filename ?? "2026-08-08-quarterly-review.md",
    contents,
    bodyOffset: p.bodyOffset ?? FRONT.length,
    inVault: p.inVault ?? false,
    sessionId: p.sessionId ?? "01JDEVICE-01JSESSION",
    // Beside the recording the anchor is the session folder's PARENT, so the
    // relative path is the bare filename; in a vault it is anchored on the
    // synced folder (see the vault case below).
    relativePath: p.relativePath ?? "2026-08-08-quarterly-review.md",
  };
}

beforeEach(() => {
  mockResolve.mockReset();
  mockResolve.mockResolvedValue(stub());
  mockSave.mockReset();
  mockSave.mockResolvedValue(undefined);
  mockDismiss.mockReset();
  mockDismiss.mockResolvedValue(true);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("RecordingNoteStub", () => {
  it("presents the composed body prefilled, with the cursor in it and the frontmatter nowhere", async () => {
    render(<RecordingNoteStub folder={FOLDER} />);

    const field = await screen.findByTestId<HTMLTextAreaElement>(NOTE_STUB_BODY_TESTID);
    expect(mockResolve).toHaveBeenCalledWith(FOLDER);
    // Split at `bodyOffset` in UTF-16 code units: the body, exactly, and not
    // one character of keeper's block.
    expect(field.value).toBe(BODY);
    expect(screen.queryByText(/session:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^---$/)).not.toBeInTheDocument();

    // UX-DR51: the cursor is IN THE BODY, at the end of what keeper prefilled,
    // so the first keystroke continues the note instead of shoving keeper's own
    // first line down the page.
    expect(field).toHaveFocus();
    expect(field.selectionStart).toBe(BODY.length);
    expect(screen.getByTestId(NOTE_STUB_HINT_TESTID)).toHaveTextContent(NOTE_STUB_HINT);
  });

  it("deletes an untouched note on the dismiss key, calling dismiss exactly once and writing nothing", async () => {
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.keyDown(field, { key: "Escape" });

    await waitFor(() => expect(mockDismiss).toHaveBeenCalledWith(FOLDER));
    expect(mockDismiss).toHaveBeenCalledTimes(1);
    // An untouched stub is never written first — a save would make it
    // non-identical on disk and Rust would then refuse to delete it.
    expect(mockSave).not.toHaveBeenCalled();
    // Deleted: the surface goes with the file.
    await waitFor(() =>
      expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument(),
    );
    expect(screen.queryByTestId(NOTE_STUB_KEPT_TESTID)).not.toBeInTheDocument();
  });

  it("saves the typed words BEFORE dismissing, so the dismiss key cannot discard them", async () => {
    // Rust deletes only a file still byte-identical to what keeper composed.
    // Words that never reached disk would therefore be deleted with it.
    mockDismiss.mockResolvedValue(false);
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.change(field, { target: { value: `${BODY}Renewals slipped a quarter.` } });
    fireEvent.keyDown(field, { key: "Escape" });

    // The whole file, with keeper's block returned byte-identical: the user can
    // never damage the frontmatter AC1's round-trip depends on.
    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(FOLDER, `${FRONT}${BODY}Renewals slipped a quarter.`),
    );
    expect(mockSave).toHaveBeenCalledTimes(1);
    expect(mockDismiss).toHaveBeenCalledTimes(1);
    // Rust kept it, and the card says so rather than going quiet. The path is
    // the fixture's own `relativePath` — the beside-the-folder case, whose stub
    // is a bare filename; the vault case asserts its `recordings/` prefix below.
    await waitFor(() => expect(screen.getByTestId(NOTE_STUB_KEPT_TESTID)).toBeInTheDocument());
    expect(screen.getByTestId(NOTE_STUB_KEPT_TESTID)).toHaveTextContent(
      "Kept beside the recording as 2026-08-08-quarterly-review.md",
    );
  });

  it("refuses to dismiss behind a failed save, keeping the words on screen", async () => {
    mockSave.mockRejectedValue({
      code: "notesWriteFailed",
      message: "The volume holding your notes is read-only.",
    });
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.change(field, { target: { value: `${BODY}Renewals slipped.` } });
    fireEvent.keyDown(field, { key: "Escape" });

    await waitFor(() =>
      expect(screen.getByTestId(NOTE_STUB_FAULT_TESTID)).toHaveTextContent(
        "The volume holding your notes is read-only.",
      ),
    );
    // Dismissing now would delete the pristine file the words were meant to
    // replace — the one unrecoverable move on this surface.
    expect(mockDismiss).not.toHaveBeenCalled();
    expect(screen.getByTestId(NOTE_STUB_BODY_TESTID)).toHaveValue(`${BODY}Renewals slipped.`);
  });

  it("commits a dirty draft on blur, once, and says so", async () => {
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.change(field, { target: { value: `${BODY}Ship date moved.` } });
    fireEvent.blur(field);

    await waitFor(() =>
      expect(mockSave).toHaveBeenCalledWith(FOLDER, `${FRONT}${BODY}Ship date moved.`),
    );
    await waitFor(() =>
      expect(screen.getByTestId(NOTE_STUB_HINT_TESTID)).toHaveTextContent(NOTE_STUB_SAVED),
    );

    // A second blur on text Rust already has writes nothing.
    fireEvent.blur(field);
    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
  });

  it("writes nothing when the body was never touched", async () => {
    // Saving an untouched stub would make it non-identical on disk, and Rust
    // would then keep the empty note this story exists to delete.
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.blur(field);
    fireEvent.change(field, { target: { value: BODY } });
    fireEvent.blur(field);

    await waitFor(() => expect(mockResolve).toHaveBeenCalled());
    expect(mockSave).not.toHaveBeenCalled();
  });

  it("is simply absent when no stub could be written", async () => {
    mockResolve.mockResolvedValue(null);
    render(<RecordingNoteStub folder={FOLDER} />);

    await waitFor(() => expect(mockResolve).toHaveBeenCalledWith(FOLDER));
    expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByTestId(NOTE_STUB_HINT_TESTID)).not.toBeInTheDocument();
  });

  it("treats a failed resolution as absence, never as an error", async () => {
    // The recording finalized. A stub that cannot even be looked up is logged
    // in Rust and silent here — it is never allowed to read as a failure.
    mockResolve.mockRejectedValue({ code: "notesVaultUnknown", message: "No vault configured." });
    render(<RecordingNoteStub folder={FOLDER} />);

    await waitFor(() => expect(mockResolve).toHaveBeenCalledWith(FOLDER));
    expect(screen.queryByTestId(NOTE_STUB_BODY_TESTID)).not.toBeInTheDocument();
    expect(screen.queryByTestId(NOTE_STUB_FAULT_TESTID)).not.toBeInTheDocument();
  });

  it("does not steal the caret from a field the user is already typing in", async () => {
    // The stub arrives a round trip after the card mounts, and the rename field
    // is one click away.
    render(
      <>
        <input data-testid="elsewhere" />
        <RecordingNoteStub folder={FOLDER} />
      </>,
    );
    screen.getByTestId("elsewhere").focus();

    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);
    expect(screen.getByTestId("elsewhere")).toHaveFocus();
    expect(field).not.toHaveFocus();
  });

  it("keeps the user's words when a rename re-resolves the SAME session at its new folder", async () => {
    const { rerender } = render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);
    fireEvent.change(field, { target: { value: `${BODY}Half-typed sentence` } });

    // A rename moves the session folder (Story 40.4); the stub lives in the
    // parent and does not move, so the same note resolves from the new path.
    mockResolve.mockResolvedValue(stub({ contents: `${FRONT}${BODY}` }));
    rerender(<RecordingNoteStub folder={MOVED_FOLDER} />);

    await waitFor(() => expect(mockResolve).toHaveBeenCalledWith(MOVED_FOLDER));
    expect(screen.getByTestId(NOTE_STUB_BODY_TESTID)).toHaveValue(`${BODY}Half-typed sentence`);

    // And the commands follow the session to where it is now.
    fireEvent.keyDown(screen.getByTestId(NOTE_STUB_BODY_TESTID), { key: "Escape" });
    await waitFor(() => expect(mockDismiss).toHaveBeenCalledWith(MOVED_FOLDER));
  });

  it("re-seeds for a DIFFERENT session in the same card slot", async () => {
    const { rerender } = render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);
    fireEvent.change(field, { target: { value: `${BODY}Belongs to the first session` } });

    const NEXT_BODY = "# Standup\n\n";
    mockResolve.mockResolvedValue(
      stub({
        sessionId: "01JDEVICE-01JOTHER",
        contents: `${FRONT}${NEXT_BODY}`,
        filename: "2026-08-08-standup.md",
      }),
    );
    rerender(
      <RecordingNoteStub folder="/Users/alice/Movies/keeper/2026/2026-08-08 1600 standup" />,
    );

    // One session's draft must never be offered as another session's note.
    await waitFor(() => expect(screen.getByTestId(NOTE_STUB_BODY_TESTID)).toHaveValue(NEXT_BODY));
  });

  it("names where a kept note landed beside the recording", async () => {
    mockDismiss.mockResolvedValue(false);
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.change(field, { target: { value: `${BODY}Kept.` } });
    fireEvent.keyDown(field, { key: "Escape" });

    // A note the user wrote does not vanish silently: the card says where it
    // is, by its RELATIVE path (FR-145 — no absolute path anywhere in 42.4).
    await waitFor(() =>
      expect(screen.getByTestId(NOTE_STUB_KEPT_TESTID)).toHaveTextContent(
        "Kept beside the recording as 2026-08-08-quarterly-review.md",
      ),
    );
  });

  it("names the vault when that is where the note landed", async () => {
    mockDismiss.mockResolvedValue(false);
    // In a vault the note is anchored on the synced folder, in a sibling
    // subtree — never inside the recordings folder, which RecordingsConfig
    // refuses to overlap.
    mockResolve.mockResolvedValue(
      stub({ inVault: true, relativePath: "recordings/2026-08-08-quarterly-review.md" }),
    );
    render(<RecordingNoteStub folder={FOLDER} />);
    const field = await screen.findByTestId(NOTE_STUB_BODY_TESTID);

    fireEvent.change(field, { target: { value: `${BODY}Kept in the vault.` } });
    fireEvent.keyDown(field, { key: "Escape" });

    await waitFor(() =>
      expect(screen.getByTestId(NOTE_STUB_KEPT_TESTID)).toHaveTextContent(
        "Kept in your notes as recordings/2026-08-08-quarterly-review.md",
      ),
    );
  });
});
