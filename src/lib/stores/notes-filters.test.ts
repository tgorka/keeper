import { beforeEach, describe, expect, it } from "vitest";
import {
  emptyFilterReason,
  isFiltered,
  noteQueryFor,
  notesFiltersStore,
  resetNotesFiltersStoreForTest,
  tagChipState,
} from "@/lib/stores/notes-filters";

beforeEach(() => {
  resetNotesFiltersStoreForTest();
});

/** The current chip states, which is what every assertion here is about. */
const terms = () => notesFiltersStore.getState().tagTerms;

describe("noteQueryFor", () => {
  it("sends every active tag, so Rust intersects rather than unions them", () => {
    const state = notesFiltersStore.getState();
    state.cycleTag("work");
    state.cycleTag("urgent");

    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual({
      work: "include",
      urgent: "include",
    });
  });

  it("sends an excluded chip as an exclude term rather than dropping it", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("client/acme", "include");
    state.setTagTerm("draft", "exclude");

    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual({
      "client/acme": "include",
      draft: "exclude",
    });
  });

  it("drops a tag from the request when its chip is cleared", () => {
    const state = notesFiltersStore.getState();
    state.cycleTag("work");
    state.cycleTag("urgent");
    state.removeTag("urgent");

    // Widening is a shorter term set, never a switch to a different predicate.
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual({ work: "include" });
  });

  it("sends the pinned chip's flag once, and it is the only flag a scope can no longer add", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "pinned-space", name: "Pinned", defaultKey: "pinned" });
    state.setPinnedOnly(true);

    // The seeded Pinned space says `is:pinned` in its own frontmatter, which
    // Rust evaluates from `spaceId`. If the store still carried a scope→flag
    // table this would be `["pinned", "pinned"]` or would double-filter.
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).flags).toEqual(["pinned"]);
  });

  it("asks for a seeded default's notes through its space id, never through a flag of its own", () => {
    // Recordings, Inbox and Journal were `is:recording` / `is:untagged` /
    // `is:journal` in a table here. Those strings live in the vault now, so the
    // request for any of them carries no flag at all — a flag surviving here
    // would mean the store was still filtering a space a second time.
    for (const key of ["recordings", "inbox", "journal"]) {
      notesFiltersStore.getState().clearAll();
      notesFiltersStore
        .getState()
        .setScope({ kind: "space", id: `s-${key}`, name: key, defaultKey: key });

      const query = noteQueryFor(notesFiltersStore.getState(), 0, 200);
      expect(query.flags).toEqual([]);
      expect(query.spaceId).toBe(`s-${key}`);
    }
  });

  it("sends a space id rather than a flag for a space scope", () => {
    notesFiltersStore
      .getState()
      .setScope({ kind: "space", id: "space-1", name: "Active work", defaultKey: null });

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
    const inbox = { kind: "space", id: "s-inbox", name: "Inbox", defaultKey: "inbox" } as const;
    state.setScope(inbox);
    state.setScope(inbox);

    expect(notesFiltersStore.getState().scope.kind).toBe("all");
  });

  it("distinguishes two spaces, so picking a different one switches rather than clears", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "a", name: "A", defaultKey: null });
    state.setScope({ kind: "space", id: "b", name: "B", defaultKey: null });

    expect(notesFiltersStore.getState().scope).toEqual({
      kind: "space",
      id: "b",
      name: "B",
      defaultKey: null,
    });
  });

  /**
   * A seeded default and a space of the user's own are the same kind of thing.
   * `defaultKey` rides along for the sentence an empty Recordings shows; it must
   * not be a second axis of identity, or re-pressing a renamed default would
   * fail to clear it.
   */
  it("tells two spaces apart by id alone, not by whether one is a seeded default", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "s1", name: "Recordings", defaultKey: "recordings" });
    state.setScope({ kind: "space", id: "s1", name: "Sessions", defaultKey: "recordings" });

    expect(notesFiltersStore.getState().scope.kind).toBe("all");
  });
});

describe("dropLastChip", () => {
  it("walks the bar down from its end, one press at a time", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "s-inbox", name: "Inbox", defaultKey: "inbox" });
    state.cycleTag("work");
    state.cycleTag("urgent");
    state.setAgentOnly(true);
    state.setPinnedOnly(true);

    const drop = () => notesFiltersStore.getState().dropLastChip();

    drop();
    expect(notesFiltersStore.getState().pinnedOnly).toBe(false);
    drop();
    expect(notesFiltersStore.getState().agentOnly).toBe(false);
    drop();
    expect(terms().map((chip) => chip.tag)).toEqual(["work"]);
    drop();
    expect(terms()).toEqual([]);
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
    notesFiltersStore.getState().cycleTag("work");
    // This boolean is what picks between "this vault is empty" and "no notes
    // match these filters", so a false negative would word an over-filtered
    // list as an empty vault.
    expect(isFiltered(notesFiltersStore.getState())).toBe(true);
  });
});

describe("the three-state tag chip", () => {
  it("cycles off, include, exclude, off", () => {
    const cycle = () => notesFiltersStore.getState().cycleTag("draft");

    expect(tagChipState(terms(), "draft")).toBe("off");
    cycle();
    expect(tagChipState(terms(), "draft")).toBe("include");
    cycle();
    expect(tagChipState(terms(), "draft")).toBe("exclude");
    cycle();
    // Off is the absence of a term, not a third kind of term.
    expect(tagChipState(terms(), "draft")).toBe("off");
    expect(terms()).toEqual([]);
    // And the cycle keeps going rather than sticking at either end.
    cycle();
    expect(tagChipState(terms(), "draft")).toBe("include");
  });

  it("cannot hold one tag as both included and excluded", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("draft", "include");
    state.setTagTerm("draft", "exclude");

    // One entry, not two: there is no state in which Rust would have to pick a
    // winner, which is the difference between unrepresentable and resolved.
    expect(terms()).toEqual([{ tag: "draft", term: "exclude" }]);
    expect(Object.entries(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags)).toEqual([
      ["draft", "exclude"],
    ]);
  });

  it("keeps a chip where it is in the bar when its state changes", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("work", "include");
    state.setTagTerm("draft", "include");
    state.setTagTerm("work", "exclude");

    // The target must not move under the cursor mid-cycle: a chip that jumped
    // to the end of the bar on every press would be unclickable twice.
    expect(terms()).toEqual([
      { tag: "work", term: "exclude" },
      { tag: "draft", term: "include" },
    ]);
  });
});

describe("emptyFilterReason", () => {
  /**
   * The tag that is honest and empty at the same time.
   *
   * The rail's counts include tags carried by RECORDINGS; this list shows notes.
   * A vault with a recording tagged `epic22` and no note carrying it shows
   * `epic22 1` in the rail and nothing in the list — which reads as a broken
   * filter until somebody says where the tag lives. Diagnosed on a real vault.
   */
  it("says where else a tag can live, when a tag is the only thing narrowing", () => {
    notesFiltersStore.getState().setTagTerm("epic22", "include");
    const reason = emptyFilterReason(notesFiltersStore.getState()) ?? "";

    expect(reason).toContain("Narrowed by epic22.");
    expect(reason).toContain("carried by a recording");
  });

  /**
   * And not otherwise: with a search term in the sentence there are other
   * explanations, and offering this one would be guessing at which term emptied
   * the list — which this function's own doc refuses to do.
   */
  it("keeps quiet about recordings when something else is narrowing too", () => {
    notesFiltersStore.getState().setTagTerm("epic22", "include");
    notesFiltersStore.getState().setText("quarterly");

    expect(emptyFilterReason(notesFiltersStore.getState()) ?? "").not.toContain("recording");
  });

  it("names the excluded term that emptied the list, in words rather than a sign", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("client/acme", "include");
    state.setTagTerm("draft", "exclude");

    // The `−` on the chip does not survive being read aloud, and an exclusion is
    // the term whose effect a user cannot see — so the sentence has to say it.
    expect(emptyFilterReason(notesFiltersStore.getState())).toBe(
      "Narrowed by client/acme and not draft.",
    );
  });

  it("names a lone term without inventing a list", () => {
    notesFiltersStore.getState().setTagTerm("draft", "exclude");
    expect(emptyFilterReason(notesFiltersStore.getState())).toBe("Narrowed by not draft.");
  });

  it("names every axis of the bar, so no term can go unmentioned", () => {
    const state = notesFiltersStore.getState();
    state.setScope({ kind: "space", id: "s-inbox", name: "Inbox", defaultKey: "inbox" });
    state.setTagTerm("work", "include");
    state.setAgentOnly(true);
    state.setPinnedOnly(true);
    state.setText("  pricing  ");

    expect(emptyFilterReason(notesFiltersStore.getState())).toBe(
      'Narrowed by Inbox, work, changed by agent, pinned only and "pricing".',
    );
  });

  it("says nothing when nothing is narrowing", () => {
    expect(emptyFilterReason(notesFiltersStore.getState())).toBeNull();
  });
});
