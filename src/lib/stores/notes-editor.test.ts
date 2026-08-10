/**
 * Story 46.12: the mirror is per note, and this is where that is load-bearing.
 *
 * The panel model was capped at one note panel for four stories because this
 * store was a module singleton — one buffer, one base, one subscription — so a
 * second mounted editor would have taken it and the first would have shown the
 * second's document under the first's title while its autosave wrote the
 * second's body into the first's file.
 *
 * Every test below is about that failure and not about the reducer's arithmetic:
 * the reducer's own rules (dirty is derived, a clean buffer takes an external
 * write live, a dirty one raises the bar) are proved through the surfaces that
 * show them in `note-diff-bar.test.tsx`. What is proved here is that two notes
 * cannot reach each other, that a closed note cannot be written to at all, and
 * that two VIEWS of one note are one document rather than two.
 */
import { beforeEach, describe, expect, it } from "vitest";
import type { NoteWriteVm } from "@/lib/ipc/client";
import {
  acceptPending,
  adoptBodySubscription,
  applyBodyBatch,
  documentKey,
  dropNoteDocument,
  EMPTY_NOTE_DOCUMENT,
  editBuffer,
  keepMine,
  markSaved,
  markSaveFailed,
  notesEditorStore,
  openNoteDocument,
  readNoteDocument,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";

const VAULT = "vault-1";
const ONE = "note-one";
const TWO = "note-two";

const WRITE: NoteWriteVm = {
  rev: "rev-2",
  frontmatter: "---\nid: 01AAA\n---\n",
  path: "notes/one.md",
  conflictCopy: null,
};

/** Open a note and give it a body, the way the hook and the channel do. */
function open(noteId: string, text: string, rev = "rev-1"): void {
  openNoteDocument(VAULT, noteId);
  applyBodyBatch(VAULT, noteId, {
    kind: "reset",
    rev,
    path: `notes/${noteId}.md`,
    frontmatter: "",
    text,
    cursor: null,
  });
}

beforeEach(() => {
  resetNotesEditorStoreForTest();
});

describe("two notes at once", () => {
  it("keeps a separate buffer and a separate dirty flag for each", () => {
    open(ONE, "one's body\n");
    open(TWO, "two's body\n");

    editBuffer(VAULT, ONE, "one's body\nand a new line\n");

    // The note that was typed in.
    expect(readNoteDocument(VAULT, ONE).text).toBe("one's body\nand a new line\n");
    expect(readNoteDocument(VAULT, ONE).dirty).toBe(true);
    // The one beside it, untouched. This is the whole story: before 46.12 the
    // second `open` above would have blanked the first document, and this edit
    // would have been the only buffer either editor could show.
    expect(readNoteDocument(VAULT, TWO).text).toBe("two's body\n");
    expect(readNoteDocument(VAULT, TWO).dirty).toBe(false);
    expect(readNoteDocument(VAULT, TWO).base).toBe("two's body\n");
  });

  it("acknowledges a write into the note that was written, and only that one", () => {
    open(ONE, "one's body\n");
    open(TWO, "two's body\n");
    editBuffer(VAULT, ONE, "one's saved body\n");
    editBuffer(VAULT, TWO, "two's unsaved body\n");

    markSaved(VAULT, ONE, "one's saved body\n", WRITE);

    const saved = readNoteDocument(VAULT, ONE);
    expect(saved.base).toBe("one's saved body\n");
    expect(saved.rev).toBe("rev-2");
    expect(saved.dirty).toBe(false);
    // The other note's unsaved words are still unsaved words, and still its own.
    const other = readNoteDocument(VAULT, TWO);
    expect(other.text).toBe("two's unsaved body\n");
    expect(other.base).toBe("two's body\n");
    expect(other.rev).toBe("rev-1");
    expect(other.dirty).toBe(true);
    // And the path a write stamps lands on the note that was written.
    expect(saved.path).toBe("notes/one.md");
    expect(other.path).toBe(`notes/${TWO}.md`);
  });

  it("routes a channel batch to the note the channel belongs to", () => {
    open(ONE, "one's body\n");
    open(TWO, "two's body\n");

    applyBodyBatch(VAULT, TWO, {
      kind: "external",
      rev: "rev-9",
      frontmatter: "",
      text: "two's body, changed on disk\n",
    });

    expect(readNoteDocument(VAULT, ONE).text).toBe("one's body\n");
    expect(readNoteDocument(VAULT, ONE).rev).toBe("rev-1");
    expect(readNoteDocument(VAULT, TWO).text).toBe("two's body, changed on disk\n");
  });

  it("raises and resolves the diff bar per note", () => {
    open(ONE, "body\n");
    open(TWO, "body\n");
    editBuffer(VAULT, ONE, "body, mine\n");
    editBuffer(VAULT, TWO, "body, also mine\n");

    applyBodyBatch(VAULT, ONE, {
      kind: "external",
      rev: "rev-9",
      frontmatter: "",
      text: "body, theirs\n",
    });

    expect(readNoteDocument(VAULT, ONE).pending).not.toBeNull();
    expect(readNoteDocument(VAULT, TWO).pending).toBeNull();

    acceptPending(VAULT, ONE);
    expect(readNoteDocument(VAULT, ONE).text).toBe("body, theirs\n");
    expect(readNoteDocument(VAULT, TWO).text).toBe("body, also mine\n");

    // And the other half of the pair, for the same reason.
    applyBodyBatch(VAULT, TWO, {
      kind: "external",
      rev: "rev-9",
      frontmatter: "",
      text: "body, theirs\n",
    });
    keepMine(VAULT, TWO);
    expect(readNoteDocument(VAULT, TWO).text).toBe("body, also mine\n");
    expect(readNoteDocument(VAULT, TWO).rev).toBe("rev-1");
  });

  it("puts a failed write's message on the note that failed", () => {
    open(ONE, "one\n");
    open(TWO, "two\n");

    markSaveFailed(VAULT, ONE, "the volume is read-only");

    expect(readNoteDocument(VAULT, ONE).error).toBe("the volume is read-only");
    expect(readNoteDocument(VAULT, TWO).error).toBeNull();
  });

  it("tells two notes apart even when one id is the other's prefix", () => {
    // A key is not a concatenation: `a` + `bc` and `ab` + `c` are different
    // documents, which a naive `${vaultId}${noteId}` would merge.
    expect(documentKey("a", "bc")).not.toBe(documentKey("ab", "c"));
    open("a", "first\n");
    openNoteDocument("a2", "b");
    expect(readNoteDocument("a", "a").text).toBe("");
  });
});

describe("a note nobody has open", () => {
  it("reads as the empty document rather than undefined", () => {
    expect(readNoteDocument(VAULT, ONE)).toBe(EMPTY_NOTE_DOCUMENT);
    // And so does a surface that has no note at all, so no caller branches on
    // "not open yet" separately from "open and empty".
    expect(readNoteDocument(null, null)).toBe(EMPTY_NOTE_DOCUMENT);
    expect(readNoteDocument(VAULT, null)).toBe(EMPTY_NOTE_DOCUMENT);
  });

  it("absorbs every reducer without coming back to life", () => {
    // A subscription outlives its surface by however long the round trip takes,
    // so a batch, a save acknowledgement or a timer can all land after the last
    // view has gone. None of them may resurrect a document nothing is mounted
    // over — which is also what stops a straggler writing into the note that
    // took its place.
    open(ONE, "body\n");
    dropNoteDocument(VAULT, ONE);

    editBuffer(VAULT, ONE, "typed after the close");
    applyBodyBatch(VAULT, ONE, {
      kind: "reset",
      rev: "rev-2",
      path: "notes/one.md",
      frontmatter: "",
      text: "delivered after the close",
      cursor: null,
    });
    markSaved(VAULT, ONE, "written after the close", WRITE);
    markSaveFailed(VAULT, ONE, "failed after the close");

    expect(notesEditorStore.getState().documents).toEqual({});
    expect(readNoteDocument(VAULT, ONE)).toBe(EMPTY_NOTE_DOCUMENT);
  });
});

describe("two views of one note", () => {
  it("share one document rather than opening a second buffer", () => {
    expect(openNoteDocument(VAULT, ONE)).toBe(true);
    applyBodyBatch(VAULT, ONE, {
      kind: "reset",
      rev: "rev-1",
      path: "notes/one.md",
      frontmatter: "",
      text: "body\n",
      cursor: null,
    });
    editBuffer(VAULT, ONE, "body, half typed\n");

    // The second panel arrives over a note that is already open.
    expect(openNoteDocument(VAULT, ONE)).toBe(false);

    // It must show what the first one is holding, unsaved keystrokes and all —
    // a reset here would throw away words the user can see.
    const document = readNoteDocument(VAULT, ONE);
    expect(document.text).toBe("body, half typed\n");
    expect(document.dirty).toBe(true);
    expect(document.views).toBe(2);
  });

  it("closes the channel only when the last one goes", () => {
    openNoteDocument(VAULT, ONE);
    adoptBodySubscription(VAULT, ONE, readNoteDocument(VAULT, ONE).generation, "sub-1");
    openNoteDocument(VAULT, ONE);

    // First view leaves: nothing to close, and the document stays.
    expect(dropNoteDocument(VAULT, ONE)).toBeNull();
    expect(readNoteDocument(VAULT, ONE).views).toBe(1);
    expect(readNoteDocument(VAULT, ONE).subscriptionId).toBe("sub-1");

    // Last view leaves: the caller is handed the document so it can flush the
    // buffer and close the channel from a value, not from a second read of a
    // store that no longer holds it.
    const closed = dropNoteDocument(VAULT, ONE);
    expect(closed?.subscriptionId).toBe("sub-1");
    expect(notesEditorStore.getState().documents).toEqual({});
    // And a third release is not a second close.
    expect(dropNoteDocument(VAULT, ONE)).toBeNull();
  });
});

describe("a subscription that resolves late", () => {
  it("is refused by the document that replaced the one which asked for it", () => {
    // Open, close and reopen inside one `notes_open` round trip — a panel
    // remounted, or React's double-invoked effects in development. The first
    // channel resolves last, and the document it belonged to is gone.
    openNoteDocument(VAULT, ONE);
    const stale = readNoteDocument(VAULT, ONE).generation;
    dropNoteDocument(VAULT, ONE);
    openNoteDocument(VAULT, ONE);
    const live = readNoteDocument(VAULT, ONE).generation;
    expect(live).not.toBe(stale);

    expect(adoptBodySubscription(VAULT, ONE, stale, "sub-orphan")).toBe(false);
    expect(readNoteDocument(VAULT, ONE).subscriptionId).toBeNull();

    // The live one is adopted, and it is the one a save would go through.
    expect(adoptBodySubscription(VAULT, ONE, live, "sub-live")).toBe(true);
    expect(readNoteDocument(VAULT, ONE).subscriptionId).toBe("sub-live");
  });

  it("is refused when nobody holds the note any more", () => {
    openNoteDocument(VAULT, ONE);
    const generation = readNoteDocument(VAULT, ONE).generation;
    dropNoteDocument(VAULT, ONE);

    expect(adoptBodySubscription(VAULT, ONE, generation, "sub-orphan")).toBe(false);
    expect(notesEditorStore.getState().documents).toEqual({});
  });
});
