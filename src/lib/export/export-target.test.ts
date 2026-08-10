/**
 * Story 45.21 — what `exportTarget` sends, and what it does not.
 *
 * Every test here asserts the CALL and not only the outcome. A mocked command
 * answers whatever it was told to answer regardless of its arguments, so a test
 * that picks a folder, exports, and checks the receipt has checked the mock:
 * sending an empty destination, the wrong vault id, or the note's id where the
 * path belongs would all pass it. The wave-2 stories lost three real defects to
 * exactly that shape.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

const notesExport = vi.fn();
const syncExportEntry = vi.fn();
const notesSave = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  notesExport: (vaultId: unknown, noteId: unknown, destination: unknown) =>
    notesExport(vaultId, noteId, destination),
  syncExportEntry: (id: unknown, subpath: unknown, destination: unknown) =>
    syncExportEntry(id, subpath, destination),
  notesSave: (subscriptionId: unknown, text: unknown, rev: unknown) =>
    notesSave(subscriptionId, text, rev),
}));

import {
  EXPORT_FAILED_SENTENCE,
  EXPORT_PICKER_TITLE,
  EXPORT_UNSAVED_SENTENCE,
  EXPORT_UNSUPPORTED_SENTENCE,
  exportTarget,
} from "@/lib/export/export-target";
import {
  adoptBodySubscription,
  applyBodyBatch,
  editBuffer,
  openNoteDocument,
  readNoteDocument,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";

/** A receipt shaped like the one Rust composes. Two written entries, always:
 *  a caller that reported only the first would pass a one-entry fixture. */
const RECEIPT = {
  path: "/Users/alice/Desktop/Meeting",
  written: ["Meeting/Meeting.md", "Meeting/attachments/photo.png"],
  missing: [],
  notes: [],
  summary: "Exported Meeting.md and 1 attachment to /Users/alice/Desktop/Meeting.",
};

const FILE_RECEIPT = {
  path: "/Users/alice/Desktop/report.pdf",
  written: ["report.pdf"],
  missing: [],
  notes: [],
  summary: "Exported report.pdf to /Users/alice/Desktop.",
};

/**
 * The store as it is for a note nobody has edited since it opened.
 *
 * The same three calls `useNotesBody` makes on mount, rather than a hand-built
 * document: a document written straight into the store would have no reference
 * count and no generation, and every reducer treats that as "nobody has this
 * open" and ignores it.
 */
function openClean(vaultId = "v1", noteId = "n1"): void {
  openNoteDocument(vaultId, noteId);
  adoptBodySubscription(vaultId, noteId, readNoteDocument(vaultId, noteId).generation, "sub-1");
  applyBodyBatch(vaultId, noteId, {
    kind: "reset",
    rev: "r1",
    path: "notes/Meeting.md",
    frontmatter: "",
    text: "body",
    cursor: null,
  });
}

beforeEach(() => {
  resetNotesEditorStoreForTest();
  openFolder.mockReset().mockResolvedValue("/Users/alice/Desktop");
  notesExport.mockReset().mockResolvedValue(RECEIPT);
  syncExportEntry.mockReset().mockResolvedValue(FILE_RECEIPT);
  notesSave.mockReset().mockResolvedValue({
    rev: "r2",
    frontmatter: "",
    path: "notes/Meeting.md",
    conflictCopy: null,
  });
});

describe("exporting a note", () => {
  it("sends the vault, the note and the folder that was picked", async () => {
    openClean("vault-7", "note-9");
    const outcome = await exportTarget({ kind: "note", vaultId: "vault-7", noteId: "note-9" });

    expect(notesExport).toHaveBeenCalledWith("vault-7", "note-9", "/Users/alice/Desktop");
    expect(syncExportEntry).not.toHaveBeenCalled();
    expect(outcome).toEqual({ status: "exported", receipt: RECEIPT });
  });

  it("asks for a folder, one of them, under a title that names the act", async () => {
    openClean();
    await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    expect(openFolder).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: EXPORT_PICKER_TITLE,
    });
  });

  it("writes the buffer before Rust reads the file, and sends what the save left", async () => {
    const order: string[] = [];
    notesSave.mockImplementation(async () => {
      order.push("save");
      return { rev: "r2", frontmatter: "", path: "notes/Meeting.md", conflictCopy: null };
    });
    notesExport.mockImplementation(async () => {
      order.push("export");
      return RECEIPT;
    });
    openClean();
    editBuffer("v1", "n1", "body and a new paragraph");

    const outcome = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });

    // The flush is the same write the autosave performs, with the buffer and
    // the revision it belongs to — not the last-acknowledged text.
    expect(notesSave).toHaveBeenCalledWith("sub-1", "body and a new paragraph", "r1");
    expect(order).toEqual(["save", "export"]);
    expect(outcome.status).toBe("exported");
  });

  it("refuses rather than exporting a copy missing the edits it could not save", async () => {
    notesSave.mockImplementation(async () => {
      throw { code: "internal", message: "the disk is full" };
    });
    openClean();
    editBuffer("v1", "n1", "unsaved words");

    const outcome = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });

    expect(outcome).toEqual({ status: "refused", reason: EXPORT_UNSAVED_SENTENCE });
    // And nothing was asked for: no folder picked, no bytes copied.
    expect(openFolder).not.toHaveBeenCalled();
    expect(notesExport).not.toHaveBeenCalled();
  });

  it("does not flush, or refuse, for a note that is not the one in the editor", async () => {
    // Another note is open and dirty. Exporting a closed note must not save
    // that one's buffer, and must not be refused because of it.
    openClean("v1", "someone-else");
    editBuffer("v1", "someone-else", "their unsaved words");

    const outcome = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });

    expect(notesSave).not.toHaveBeenCalled();
    expect(notesExport).toHaveBeenCalledWith("v1", "n1", "/Users/alice/Desktop");
    expect(outcome.status).toBe("exported");
  });

  it("does not flush a clean buffer", async () => {
    openClean();
    await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    expect(notesSave).not.toHaveBeenCalled();
    expect(notesExport).toHaveBeenCalledTimes(1);
  });
});

describe("exporting a file", () => {
  it("sends the profile, the listing's own path and the folder that was picked", async () => {
    const outcome = await exportTarget({
      kind: "file",
      profileId: "p2",
      relativePath: "docs/reports/report.pdf",
    });

    expect(syncExportEntry).toHaveBeenCalledWith(
      "p2",
      "docs/reports/report.pdf",
      "/Users/alice/Desktop",
    );
    expect(notesExport).not.toHaveBeenCalled();
    expect(outcome).toEqual({ status: "exported", receipt: FILE_RECEIPT });
  });

  it("sends the whole subpath, not the file's own name", async () => {
    await exportTarget({ kind: "file", profileId: "p2", relativePath: "a/b/c/deep.png" });
    const [, subpath] = syncExportEntry.mock.calls[0] ?? [];
    expect(subpath).toBe("a/b/c/deep.png");
  });
});

describe("when nothing should happen", () => {
  it("calls no command at all when the dialog is cancelled", async () => {
    openFolder.mockResolvedValue(null);
    openClean();

    const note = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    const file = await exportTarget({ kind: "file", profileId: "p1", relativePath: "a.pdf" });

    expect(note).toEqual({ status: "cancelled" });
    expect(file).toEqual({ status: "cancelled" });
    expect(notesExport).not.toHaveBeenCalled();
    expect(syncExportEntry).not.toHaveBeenCalled();
  });

  /** `multiple: false` should make this impossible. If the plugin ever answers
   *  with an array anyway, exporting to `[0]` would put somebody's file in a
   *  folder they did not pick — so an unexpected answer writes nothing. */
  it("treats an unexpected array answer as a cancel rather than unwrapping it", async () => {
    openFolder.mockResolvedValue(["/Users/alice/Desktop", "/tmp"]);
    const outcome = await exportTarget({ kind: "file", profileId: "p1", relativePath: "a.pdf" });
    expect(outcome).toEqual({ status: "cancelled" });
    expect(syncExportEntry).not.toHaveBeenCalled();
  });

  it("has no export path for a recording, and says so instead of throwing", async () => {
    const outcome = await exportTarget({ kind: "recording", sessionId: "s1" });
    expect(outcome).toEqual({ status: "refused", reason: EXPORT_UNSUPPORTED_SENTENCE });
    expect(openFolder).not.toHaveBeenCalled();
  });
});

describe("when Rust refuses", () => {
  it("shows Rust's sentence verbatim and adds no words", async () => {
    const refusal =
      '"Meeting" is already in that folder. keeper will not write over something it did not put there — pick another folder, or move that one out of the way.';
    notesExport.mockImplementation(async () => {
      throw { code: "internal", message: refusal };
    });
    openClean();

    const outcome = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });

    expect(outcome).toEqual({ status: "refused", reason: refusal });
  });

  it("says something finished when the rejection carries no sentence", async () => {
    syncExportEntry.mockImplementation(async () => {
      throw new Error("");
    });
    const outcome = await exportTarget({ kind: "file", profileId: "p1", relativePath: "a.pdf" });
    expect(outcome).toEqual({ status: "refused", reason: EXPORT_FAILED_SENTENCE });
  });

  it("surfaces a destination that cannot be written, in the OS's own words", async () => {
    const refusal =
      'keeper could not make a folder called "Meeting" there: Permission denied (os error 13).';
    notesExport.mockImplementation(async () => {
      throw { code: "internal", message: refusal };
    });
    openClean();

    const outcome = await exportTarget({ kind: "note", vaultId: "v1", noteId: "n1" });

    expect(outcome).toEqual({ status: "refused", reason: refusal });
  });
});
