import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteRevisionVm } from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  notesHistory: vi.fn(),
  notesDiff: vi.fn(),
  notesRestoreRevision: vi.fn(),
}));

import {
  NoteHistoryPanel,
  RESTORE_REVISION,
  RESTORE_REVISION_CONFIRM,
  RESTORE_REVISION_FAILED,
} from "@/components/notes/note-history-panel";
import { notesDiff, notesHistory, notesRestoreRevision } from "@/lib/ipc/client";

const mockHistory = vi.mocked(notesHistory);
const mockDiff = vi.mocked(notesDiff);
const mockRestore = vi.mocked(notesRestoreRevision);

function revision(rev: string, subject: string): NoteRevisionVm {
  return {
    rev,
    whenMs: 1_754_700_000_000,
    device: "studio",
    origin: "local",
    source: "app",
    subject,
  };
}

beforeEach(() => {
  mockHistory.mockReset();
  mockDiff.mockReset();
  mockRestore.mockReset();
  mockRestore.mockResolvedValue(undefined);
  mockDiff.mockResolvedValue({ hunks: [], fromRev: "r2", toRev: null });
  mockHistory.mockResolvedValue([
    revision("r2", "notes: 1 modified"),
    revision("r1", "notes: 1 added"),
  ]);
});

describe("NoteHistoryPanel — restoring a version", () => {
  /**
   * The whole reason 44.8 could promise "accepting is undoable through the
   * existing history": before this the panel could show a revision and could
   * not act on one.
   */
  it("writes the selected revision back, but only on the second press", async () => {
    render(<NoteHistoryPanel vaultId="v1" noteId="n1" onBack={() => {}} />);
    await screen.findByRole("button", { name: /1 modified/ });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION }));
    // Armed, not written: one stray click must not rewrite a note.
    expect(mockRestore).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION_CONFIRM }));
    await waitFor(() => expect(mockRestore).toHaveBeenCalledWith("v1", "n1", "r2"));
    expect(await screen.findByRole("status")).toHaveTextContent("undoable too");
  });

  it("restores the revision the reader selected, not the newest one", async () => {
    render(<NoteHistoryPanel vaultId="v1" noteId="n1" onBack={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: /1 added/ }));

    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION }));
    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION_CONFIRM }));

    await waitFor(() => expect(mockRestore).toHaveBeenCalledWith("v1", "n1", "r1"));
  });

  it("says so when the write did not happen, and stays armed for nothing", async () => {
    mockRestore.mockRejectedValue("nope");
    render(<NoteHistoryPanel vaultId="v1" noteId="n1" onBack={() => {}} />);
    await screen.findByRole("button", { name: /1 modified/ });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION }));
    fireEvent.click(screen.getByRole("button", { name: RESTORE_REVISION_CONFIRM }));

    expect(await screen.findByText(RESTORE_REVISION_FAILED)).toBeInTheDocument();
    // Disarmed: the next press has to be deliberate again.
    expect(screen.getByRole("button", { name: RESTORE_REVISION })).toBeInTheDocument();
  });

  it("offers nothing to restore on a note with no history", async () => {
    mockHistory.mockResolvedValue([]);
    render(<NoteHistoryPanel vaultId="v1" noteId="n1" onBack={() => {}} />);

    expect(await screen.findByText(/No versions yet/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: RESTORE_REVISION })).toBeNull();
  });
});
