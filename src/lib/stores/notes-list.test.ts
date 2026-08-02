import { beforeEach, describe, expect, it } from "vitest";
import type { NoteChangeBatch, NoteRowVm } from "@/lib/ipc/client";
import { notesListStore, resetNotesListStoreForTest } from "@/lib/stores/notes-list";

function row(id: string, title = id): NoteRowVm {
  return {
    id,
    path: `${id}.md`,
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
  };
}

function batch(...ops: NoteChangeBatch["ops"]): NoteChangeBatch {
  return { vaultId: "vault-a", ops };
}

beforeEach(() => {
  resetNotesListStoreForTest();
});

describe("notesListStore.applyBatch", () => {
  it("takes Rust's order verbatim on a reset", () => {
    notesListStore
      .getState()
      .applyBatch(batch({ op: "reset", rows: [row("b"), row("a")], total: 2 }));

    // Conflicts above pins above the active sort is `notes_list`'s decision; a
    // mirror that sorted would be a second, quieter opinion about order.
    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["b", "a"]);
    expect(notesListStore.getState().total).toBe(2);
  });

  it("moves an existing row to its new index rather than duplicating it", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ op: "reset", rows: [row("a"), row("b"), row("c")], total: 3 }));
    state.applyBatch(batch({ op: "upsert", index: 0, row: row("c", "C, bumped") }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["c", "a", "b"]);
    expect(notesListStore.getState().rows[0].title).toBe("C, bumped");
    // A move is not a new note, so the count is unchanged.
    expect(notesListStore.getState().total).toBe(3);
  });

  it("counts a genuinely new row into the total", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ op: "reset", rows: [row("a")], total: 1 }));
    state.applyBatch(batch({ op: "upsert", index: 0, row: row("new") }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["new", "a"]);
    expect(notesListStore.getState().total).toBe(2);
  });

  it("drops an out-of-range upsert instead of punching a hole in the window", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ op: "reset", rows: [row("a")], total: 1 }));
    state.applyBatch(batch({ op: "upsert", index: 9, row: row("late") }));

    // A batch that raced a window change is dropped, never applied at a stale
    // index — the same guard every other diff reducer in this app carries.
    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["a"]);
    expect(notesListStore.getState().total).toBe(1);
  });

  it("removes by id and never lets the total go negative", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ op: "reset", rows: [row("a"), row("b")], total: 2 }));
    state.applyBatch(batch({ op: "remove", id: "a" }, { op: "remove", id: "a" }));

    expect(notesListStore.getState().rows.map((r) => r.id)).toEqual(["b"]);
    expect(notesListStore.getState().total).toBe(1);
  });

  it("keeps the cursor when the row it names leaves the window", () => {
    const state = notesListStore.getState();
    state.applyBatch(batch({ op: "reset", rows: [row("a"), row("b")], total: 2 }));
    state.select("vault-a", "a");
    state.applyBatch(batch({ op: "remove", id: "a" }));

    // The note stays open in the editor and the row is simply no longer listed
    // (UX-DR41). A cursor that cleared itself would move the user's place on
    // every agent write.
    expect(notesListStore.getState().selected).toEqual({ vaultId: "vault-a", noteId: "a" });
  });

  it("remembers which vault the open note belongs to", () => {
    const state = notesListStore.getState();
    state.select("vault-a", "a");
    state.clear();

    // `clear` empties the window on a vault switch; the open note is not part of
    // the window, so it survives and comes back when its vault does.
    expect(notesListStore.getState().rows).toEqual([]);
    expect(notesListStore.getState().selected).toEqual({ vaultId: "vault-a", noteId: "a" });
  });
});
