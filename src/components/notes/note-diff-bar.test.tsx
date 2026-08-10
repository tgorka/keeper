import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import {
  applyBodyBatch,
  beginOpenNote,
  editBuffer,
  notesEditorStore,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { NoteDiffBar } from "./note-diff-bar";

const notesMarkRead = vi.fn<(vaultId: string, noteId: string, rev: string) => Promise<void>>();

vi.mock("@/lib/ipc/client", () => ({
  notesMarkRead: (vaultId: string, noteId: string, rev: string) =>
    notesMarkRead(vaultId, noteId, rev),
}));

const OPENED = "alpha\nbeta\n";
/** The block the note opened with. It travels beside the body, never in it. */
const BLOCK = "---\nid: 01AAA\nupdated: 2026-08-03T10:00:00+00:00\n---\n";

function openClean(): void {
  beginOpenNote("vault-1", "note-1");
  applyBodyBatch({
    kind: "reset",
    rev: "rev-1",
    path: "notes/opened.md",
    frontmatter: BLOCK,
    text: OPENED,
    cursor: null,
  });
}

/** Deliver a batch the way the channel would, after the surface is mounted. */
function deliver(batch: NoteBodyBatch): void {
  act(() => {
    applyBodyBatch(batch);
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
    render(<NoteDiffBar />);

    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}gamma\n` });

    expect(screen.queryByRole("status")).toBeNull();
    expect(notesEditorStore.getState().text).toBe(`${OPENED}gamma\n`);
    expect(notesEditorStore.getState().rev).toBe("rev-2");
  });

  it("appears when a dirty buffer meets an external revision, and names what arrived", () => {
    openClean();
    editBuffer(`${OPENED}mine\n`);
    render(<NoteDiffBar />);

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
    editBuffer(`${OPENED}mine\n`);
    render(<NoteDiffBar />);
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
    const state = notesEditorStore.getState();
    expect(state.text).toBe(`${OPENED}theirs\n`);
    expect(state.rev).toBe("rev-2");
    expect(state.frontmatter).toBe("---\nid: 01AAA\nupdated: 2026-08-04T09:00:00+00:00\n---\n");
    expect(state.dirty).toBe(false);
    // Accept is the only path that clears the unread mark (UX-DR39), and it
    // acknowledges the revision the body stream delivered — never a guess.
    expect(notesMarkRead).toHaveBeenCalledWith("vault-1", "note-1", "rev-2");
  });

  it("keeps the buffer and the stale base when the user keeps mine", () => {
    openClean();
    editBuffer(`${OPENED}mine\n`);
    render(<NoteDiffBar />);
    deliver({ kind: "external", rev: "rev-2", frontmatter: BLOCK, text: `${OPENED}theirs\n` });

    fireEvent.click(screen.getByRole("button", { name: "Keep mine" }));
    expect(notesMarkRead).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    const state = notesEditorStore.getState();
    expect(state.text).toBe(`${OPENED}mine\n`);
    // The base revision deliberately does NOT move: the next save carries the
    // stale rev, which is what makes Rust keep the other side (NFR-30).
    expect(state.rev).toBe("rev-1");
    expect(state.frontmatter).toBe(BLOCK);
    expect(state.dirty).toBe(true);
  });

  it("offers resolution only when the hunks overlapped", () => {
    openClean();
    editBuffer(`${OPENED}mine\n`);
    render(<NoteDiffBar onResolve={() => {}} />);

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
