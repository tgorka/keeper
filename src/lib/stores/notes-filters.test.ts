import { beforeEach, describe, expect, it } from "vitest";
import {
  isFiltered,
  noteQueryFor,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";

beforeEach(() => {
  resetNotesFiltersStoreForTest();
});

describe("noteQueryFor", () => {
  it("sends every active tag, so Rust intersects rather than unions them", () => {
    const state = notesFiltersStore.getState();
    state.toggleTag("work");
    state.toggleTag("urgent");

    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual(["work", "urgent"]);
  });

  it("drops a tag from the request when its chip is cleared", () => {
    const state = notesFiltersStore.getState();
    state.toggleTag("work");
    state.toggleTag("urgent");
    state.removeTag("urgent");

    // Widening is a shorter tag list, never a switch to a different predicate.
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual(["work"]);
  });

  it("does not send the same flag twice when the scope and the chip agree", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "pinned" });
    state.setPinnedOnly(true);

    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).flags).toEqual(["pinned"]);
  });

  it("sends a space id rather than a flag for a space scope", () => {
    notesFiltersStore.getState().setScope({ kind: "space", id: "space-1", name: "Active work" });

    const query = noteQueryFor(notesFiltersStore.getState(), 0, 200);
    expect(query.spaceId).toBe("space-1");
    expect(query.flags).toEqual([]);
  });

  it("treats whitespace-only search text as no search at all", () => {
    notesFiltersStore.getState().setText("   ");
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).text).toBeNull();
  });
});

describe("scope selection", () => {
  it("clears the scope when the active row is selected again", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "inbox" });
    state.setScope({ kind: "inbox" });

    expect(notesFiltersStore.getState().scope.kind).toBe("all");
  });

  it("distinguishes two spaces, so picking a different one switches rather than clears", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "a", name: "A" });
    state.setScope({ kind: "space", id: "b", name: "B" });

    expect(notesFiltersStore.getState().scope).toEqual({ kind: "space", id: "b", name: "B" });
  });
});

describe("dropLastChip", () => {
  it("walks the bar down from its end, one press at a time", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "inbox" });
    state.toggleTag("work");
    state.toggleTag("urgent");
    state.setAgentOnly(true);
    state.setPinnedOnly(true);

    const drop = () => notesFiltersStore.getState().dropLastChip();

    drop();
    expect(notesFiltersStore.getState().pinnedOnly).toBe(false);
    drop();
    expect(notesFiltersStore.getState().agentOnly).toBe(false);
    drop();
    expect(notesFiltersStore.getState().tags).toEqual(["work"]);
    drop();
    expect(notesFiltersStore.getState().tags).toEqual([]);
    drop();
    expect(notesFiltersStore.getState().scope.kind).toBe("all");
    // An empty bar absorbs further presses rather than throwing or wrapping.
    drop();
    expect(isFiltered(notesFiltersStore.getState())).toBe(false);
  });
});

describe("isFiltered", () => {
  it("separates an unfiltered list from one narrowed by a lone chip", () => {
    expect(isFiltered(notesFiltersStore.getState())).toBe(false);
    notesFiltersStore.getState().toggleTag("work");
    // This boolean is what picks between "this vault is empty" and "no notes
    // match these filters", so a false negative would word an over-filtered
    // list as an empty vault.
    expect(isFiltered(notesFiltersStore.getState())).toBe(true);
  });
});
