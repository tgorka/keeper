import { beforeEach, describe, expect, it } from "vitest";
import type { NoteChangeBatch, NoteRowVm } from "@/lib/ipc/client";
import { notesListStore, resetNotesListStoreForTest } from "@/lib/stores/notes-list";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

function row(id: string, title = id): NoteRowVm {
  return {
    id,
    path: `${id}.md`,
    unresolvedTarget: "",
    predicates: [],
    title,
    snippet: "",
    tags: [],
    updatedMs: 1,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "",
    headRev: "",
    order: { value: 0, source: "default" },
  };
}

/**
 * A batch carrying explicit counts. Rust recomputes both for every batch it
 * sends (Story 44.11), so a fixture that wants a count states it rather than
 * expecting the store to derive one.
 */
function batch(
  counts: { total: number; matched?: number },
  ...ops: NoteChangeBatch["ops"]
): NoteChangeBatch {
  return {
    vaultId: "vault-a",
    ops,
    total: counts.total,
    matched: counts.matched ?? counts.total,
  };
}

beforeEach(() => {
  resetNotesListStoreForTest();
  resetPanelsStoreForTest();
});

describe("notesListStore.applyBatch", () => {
  it("takes Rust's order verbatim on a reset", () => {
    notesListStore
      .getState()
      .applyBatch(batch({ total: 2 }, { op: "reset", rows: [row("b"), row("a")] }));

    // Conflicts above pins above the active sort is `notes_list`'s decision; a
    // mirror that sorted would be a second, quieter opinion about order.
    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["b", "a"]);
    expect(notesListStore.getState().total).toBe(2);
  });

  it("moves an existing row to its new index rather than duplicating it", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 3 }, { op: "reset", rows: [row("a"), row("b"), row("c")] }));
    state.applyBatch(batch({ total: 3 }, { op: "upsert", index: 0, row: row("c", "C, bumped") }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["c", "a", "b"]);
    expect(notesListStore.getState().rows[0].title).toBe("C, bumped");
    // A move is not a new note, so the count is unchanged.
    expect(notesListStore.getState().total).toBe(3);
  });

  it("takes the count off the batch rather than counting the ops", () => {
    // Story 44.11. The store used to add one per upsert of an unseen id and
    // subtract one per remove, which is right only while every change to the
    // matched set also changes the window. Once the list is windowed (Story
    // 44.10) a note that starts matching three thousand rows below the page
    // produces no op at all — and there is no scroll that would correct a count
    // the receiver derived. Here the window gains one row and the set gains
    // eleven, and only Rust knows that.
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 1 }, { op: "reset", rows: [row("a")] }));
    state.applyBatch(batch({ total: 12 }, { op: "upsert", index: 0, row: row("new") }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["new", "a"]);
    expect(notesListStore.getState().total).toBe(12);
  });

  it("moves the count for a batch that moves no row at all", () => {
    // The case the arithmetic version could not express: nothing in the window
    // changed, and eleven notes joined the set below it.
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 1 }, { op: "reset", rows: [row("a")] }));
    state.applyBatch(batch({ total: 12 }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["a"]);
    expect(notesListStore.getState().total).toBe(12);
  });

  it("carries the matched count beside the selected one when a space caps", () => {
    // A space with `keeper.limit: 20` over a query that matches 347: the store
    // holds both, so the surface can say `20 of 347` instead of a `20` that
    // looks like the whole answer.
    notesListStore
      .getState()
      .applyBatch(batch({ total: 20, matched: 347 }, { op: "reset", rows: [row("a")] }));

    expect(notesListStore.getState().total).toBe(20);
    expect(notesListStore.getState().matched).toBe(347);
  });

  it("drops an out-of-range upsert instead of punching a hole in the window", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 1 }, { op: "reset", rows: [row("a")] }));
    state.applyBatch(batch({ total: 1 }, { op: "upsert", index: 9, row: row("late") }));

    // A batch that raced a window change is dropped, never applied at a stale
    // index — the same guard every other diff reducer in this app carries.
    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["a"]);
    expect(notesListStore.getState().total).toBe(1);
  });

  it("removes by id and applies a repeated remove exactly once", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 2 }, { op: "reset", rows: [row("a"), row("b")] }));
    state.applyBatch(batch({ total: 1 }, { op: "remove", id: "a" }, { op: "remove", id: "a" }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["b"]);
    expect(notesListStore.getState().total).toBe(1);
  });

  it("leaves the open note alone when the row it names leaves the window", () => {
    // The contract did not go away with the cursor (Story 45.1) — it moved to
    // the panel list, and it is asserted here because THIS store is the one
    // that could break it.
    const state = notesListStore.getState();
    state.applyBatch(batch({ total: 2 }, { op: "reset", rows: [row("a"), row("b")] }));
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "vault-a", noteId: "a" });
    state.applyBatch(batch({ total: 1 }, { op: "remove", id: "a" }));

    // The note stays open in the editor and the row is simply no longer listed
    // (UX-DR41). A list that closed the note would move the user's place on
    // every agent write.
    expect(activePanel(panelsStore.getState()).target).toEqual({
      kind: "note",
      vaultId: "vault-a",
      noteId: "a",
    });
  });

  it("keeps the open note across a vault switch, which only empties the window", () => {
    const state = notesListStore.getState();
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "vault-a", noteId: "a" });
    state.clear();

    // `clear` empties the window on a vault switch; the open note is a panel,
    // not part of the window, so it survives and comes back when its vault does.
    expect(notesListStore.getState().rows).toEqual([]);
    expect(activePanel(panelsStore.getState()).target).toEqual({
      kind: "note",
      vaultId: "vault-a",
      noteId: "a",
    });
  });
});
