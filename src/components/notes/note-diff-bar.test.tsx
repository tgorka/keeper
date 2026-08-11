import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import {
  applyBodyBatch,
  editBuffer,
  openNoteDocument,
  readNoteDocument,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { NoteDiffBar } from "./note-diff-bar";

const notesMarkRead = vi.fn<(vaultId: string, noteId: string, rev: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesMarkRead: (vaultId: string, noteId: string, rev: string) =>
    notesMarkRead(vaultId, noteId, rev),
}));

const VAULT = "vault-1";
const NOTE = "note-1";
const OPENED = "alpha\nbeta\n";
/** The block the note opened with. It travels beside the body, never in it. */
const BLOCK = "---\nid: 01AAA\nupdated: 2026-08-03T10:00:00+00:00\n---\n";

function openClean(noteId: string = NOTE): void {
  openNoteDocument(VAULT, noteId);
  applyBodyBatch(VAULT, noteId, {
    kind: "reset",
    rev: "rev-1",
    path: "notes/opened.md",
    frontmatter: BLOCK,
    text: OPENED,
    cursor: null,
  });
}

/** Deliver a batch the way the channel would, after the surface is mounted. */
function deliver(batch: NoteBodyBatch, noteId: string = NOTE): void {
  act(() => {
    applyBodyBatch(VAULT, noteId, batch);
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  notesMarkRead.mockResolvedValue(undefined);
  resetNotesEditorStoreForTest();
});

describe("NoteDiffBar", () => {
  it("stays away while the buffer is clean, and the write applies live", () => {
    openClean();
    render(<NoteDiffBar vaultId={VAULT} noteId={NOTE} />);

    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}gamma\n` });

    expect(screen.queryByRole("status")).toBeNull();
    expect(readNoteDocument(VAULT, NOTE).text).toBe(`${OPENED}gamma\n`);
    expect(readNoteDocument(VAULT, NOTE).rev).toBe("rev-2");
  });

  it("appears when a dirty buffer meets an external revision, and names what arrived", () => {
    openClean();
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    render(<NoteDiffBar vaultId={VAULT} noteId={NOTE} />);

    deliver({
      kind: "external",
      rev: "rev-2",
      frontmatter: BLOCK,
      text: `${OPENED}theirs\nagain\n`,
    });

    const bar = screen.getByRole("status");
    expect(bar).toHaveTextContent("Changed on disk");
    expect(bar).toHaveTextContent("2 additions");
  });

  it("clears the bar and adopts the revision when it is accepted", () => {
    openClean();
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    render(<NoteDiffBar vaultId={VAULT} noteId={NOTE} />);
    // The arriving revision changed a property as well as the body, so accepting
    // it has to adopt both halves.
    deliver({
      kind: "external",
      rev: "rev-2",
      frontmatter: "---\nid: 01AAA\nupdated: 2026-08-04T09:00:00+00:00\n---\n",
      text: `${OPENED}theirs\n`,
    });

    fireEvent.click(screen.getByRole("button", { name: "Accept" }));

    expect(screen.queryByRole("status")).toBeNull();
    const document = readNoteDocument(VAULT, NOTE);
    expect(document.text).toBe(`${OPENED}theirs\n`);
    expect(document.rev).toBe("rev-2");
    expect(document.frontmatter).toBe("---\nid: 01AAA\nupdated: 2026-08-04T09:00:00+00:00\n---\n");
    expect(document.dirty).toBe(false);
    // Accept is the only path that clears the unread mark (UX-DR39), and it
    // acknowledges the revision the body stream delivered — never a guess.
    expect(notesMarkRead).toHaveBeenCalledWith("vault-1", "note-1", "rev-2");
  });

  it("keeps the buffer and the stale base when the user keeps mine", () => {
    openClean();
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    render(<NoteDiffBar vaultId={VAULT} noteId={NOTE} />);
    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}theirs\n` });

    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    expect(notesMarkRead).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    const document = readNoteDocument(VAULT, NOTE);
    expect(document.text).toBe(`${OPENED}mine\n`);
    // The base revision deliberately does NOT move: the next save carries the
    // stale rev, which is what makes Rust keep the other side (NFR-30).
    expect(document.rev).toBe("rev-1");
    expect(document.frontmatter).toBe(BLOCK);
    expect(document.dirty).toBe(true);
  });

  it("offers resolution only when the hunks overlapped", () => {
    openClean();
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    render(<NoteDiffBar vaultId={VAULT} noteId={NOTE} onResolve={() => {}} />);

    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}theirs\n` });
    expect(screen.queryByRole("button", { name: "Resolve" })).toBeNull();

    deliver({
      kind: "diverged",
      rev: "rev-3",
      frontmatter: BLOCK,
      theirs: `${OPENED}other\n`,
    });
    expect(screen.getByRole("button", { name: "Resolve" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Take theirs" })).toBeInTheDocument();
  });
});

/**
 * Story 46.12. Two bars can be on screen at once, one per panel, and each of
 * them is about its own note.
 *
 * Worth its own block because the bar is the one surface in the editor that
 * used to read the store with no note in hand at all: it took `pending` off the
 * singleton and its Accept button acknowledged `vaultId`/`noteId` out of the
 * same place. Under two panels that is not a cosmetic mix-up — Accept ADOPTS a
 * revision, so an Accept aimed at the wrong document replaces a buffer somebody
 * is typing into with a body that arrived for a different file.
 */
describe("two notes, two bars", () => {
  const OTHER = "note-2";

  it("raises a bar only over the note whose revision arrived", () => {
    openClean();
    openClean(OTHER);
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    editBuffer(VAULT, OTHER, `${OPENED}also mine\n`);
    render(
      <>
        <NoteDiffBar vaultId={VAULT} noteId={NOTE} />
        <NoteDiffBar vaultId={VAULT} noteId={OTHER} />
      </>,
    );

    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}theirs\n` });

    expect(screen.getAllByRole("status")).toHaveLength(1);
    expect(readNoteDocument(VAULT, OTHER).pending).toBeNull();
  });

  it("accepts into the note the bar belongs to and leaves the other buffer alone", () => {
    openClean();
    openClean(OTHER);
    editBuffer(VAULT, NOTE, `${OPENED}mine\n`);
    editBuffer(VAULT, OTHER, `${OPENED}also mine\n`);
    render(
      <>
        <NoteDiffBar vaultId={VAULT} noteId={NOTE} />
        <NoteDiffBar vaultId={VAULT} noteId={OTHER} />
      </>,
    );
    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}theirs\n` });

    fireEvent.click(screen.getByRole("button", { name: "Accept" }));

    expect(readNoteDocument(VAULT, NOTE).text).toBe(`${OPENED}theirs\n`);
    expect(readNoteDocument(VAULT, OTHER).text).toBe(`${OPENED}also mine\n`);
    expect(readNoteDocument(VAULT, OTHER).dirty).toBe(true);
    // And the acknowledgement names the note that was accepted, not the one the
    // store happened to have touched last.
    expect(notesMarkRead).toHaveBeenCalledExactlyOnceWith(VAULT, NOTE, "rev-2");
  });
});
