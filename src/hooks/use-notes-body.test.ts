/**
 * `saveNote`: one write per note at a time. Blur, the idle autosave and a
 * block's forced save can all ask within one round trip; a second write that
 * carried the revision the first had already moved past would leave a
 * conflict copy of the note's own words.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { NoteWriteVm } from "@/lib/ipc/client";
import {
  adoptBodySubscription,
  applyBodyBatch,
  editBuffer,
  openNoteDocument,
  readNoteDocument,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { saveNote } from "./use-notes-body";

const notesSave = vi.fn<(id: string, text: string, rev: string) => Promise<NoteWriteVm>>();
vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  notesSave: (id: string, text: string, rev: string) => notesSave(id, text, rev),
}));

const VAULT = "v1";
const NOTE = "n1";

function written(rev: string): NoteWriteVm {
  return { rev, frontmatter: "", path: "n1.md", conflictCopy: null };
}

beforeEach(() => {
  resetNotesEditorStoreForTest();
  notesSave.mockReset();
  openNoteDocument(VAULT, NOTE);
  applyBodyBatch(VAULT, NOTE, {
    kind: "reset",
    rev: "rev-1",
    path: "n1.md",
    frontmatter: "",
    text: "",
    cursor: null,
  });
  adoptBodySubscription(VAULT, NOTE, readNoteDocument(VAULT, NOTE).generation, "sub-1");
});

describe("saveNote", () => {
  it("waits for the write in flight, then writes the rest against the revision it landed", async () => {
    let land: (write: NoteWriteVm) => void = () => {};
    notesSave.mockImplementationOnce(
      () =>
        new Promise<NoteWriteVm>((resolve) => {
          land = resolve;
        }),
    );
    notesSave.mockResolvedValueOnce(written("rev-3"));
    editBuffer(VAULT, NOTE, "a");
    const first = saveNote(VAULT, NOTE);
    editBuffer(VAULT, NOTE, "ab");
    const second = saveNote(VAULT, NOTE);
    await Promise.resolve();

    expect(notesSave).toHaveBeenCalledTimes(1);
    land(written("rev-2"));
    expect(await first).toBe(true);
    expect(await second).toBe(true);

    expect(notesSave.mock.calls).toEqual([
      ["sub-1", "a", "rev-1"],
      ["sub-1", "ab", "rev-2"],
    ]);
  });

  it("writes nothing more when the write in flight already carried every word", async () => {
    notesSave.mockResolvedValue(written("rev-2"));
    editBuffer(VAULT, NOTE, "a");

    const both = await Promise.all([saveNote(VAULT, NOTE), saveNote(VAULT, NOTE)]);

    expect(both).toEqual([true, true]);
    expect(notesSave).toHaveBeenCalledTimes(1);
  });
});
