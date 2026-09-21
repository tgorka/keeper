import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteSpaceVm } from "@/lib/ipc/client";
import {
  notesHideServiceFilesGet,
  notesHideServiceFilesSet,
  notesIncludePrivateGet,
  notesIncludePrivateSet,
} from "@/lib/ipc/client";
import {
  ALL_NOTES_SCOPE,
  emptyFilterReason,
  flipTagTerm,
  hydrateHideServiceFiles,
  hydrateIncludePrivate,
  isFiltered,
  isScopeOnly,
  noteQueryFor,
  notesFiltersStore,
  persistHideServiceFiles,
  persistIncludePrivate,
  resetNotesFiltersStoreForTest,
} from "@/lib/stores/notes-filters";

vi.mock("@/lib/ipc/client", () => ({
  notesHideServiceFilesGet: vi.fn(async () => true),
  notesHideServiceFilesSet: vi.fn(async () => {}),
  notesIncludePrivateGet: vi.fn(async () => false),
  notesIncludePrivateSet: vi.fn(async () => {}),
}));

beforeEach(() => {
  resetNotesFiltersStoreForTest();
  vi.mocked(notesHideServiceFilesGet).mockReset().mockResolvedValue(true);
  vi.mocked(notesHideServiceFilesSet).mockReset().mockResolvedValue();
  vi.mocked(notesIncludePrivateGet).mockReset().mockResolvedValue(false);
  vi.mocked(notesIncludePrivateSet).mockReset().mockResolvedValue();
});

/** The current chip states, which is what every assertion here is about. */
const terms = () => notesFiltersStore.getState().tagTerms;

function space(id: string, restore: Partial<NoteSpaceVm["restore"]> = {}): NoteSpaceVm {
  return {
    id,
    name: id,
    vaultId: "vault-1",
    vaultName: "Personal",
    defaultKey: null,
    restore: {
      tagTerms: {},
      origin: null,
      flags: [],
      text: null,
      sort: null,
      opaque: false,
      ...restore,
    },
  } as NoteSpaceVm;
}

describe("noteQueryFor", () => {
  it("sends every active tag, so Rust intersects rather than unions them", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("work", "include");
    state.setTagTerm("urgent", "include");

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
    state.setTagTerm("work", "include");
    state.setTagTerm("urgent", "include");
    state.removeTag("urgent");

    // Widening is a shorter term set, never a switch to a different predicate.
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).tags).toEqual({ work: "include" });
  });

  it("sends the pinned chip's flag once, and it is the only flag a scope can no longer add", () => {
    const state = notesFiltersStore.getState();
    state.enterSpace(space("pinned-space"));
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
      notesFiltersStore.getState().enterSpace(space(`s-${key}`));
      const query = noteQueryFor(notesFiltersStore.getState(), 0, 200);
      expect(query.flags).toEqual([]);
      expect(query.spaceId).toBe(`s-${key}`);
    }
  });

  it("sends a space id rather than a flag for a space scope", () => {
    notesFiltersStore.getState().enterSpace(space("space-1"));

    const query = noteQueryFor(notesFiltersStore.getState(), 0, 200);
    expect(query.spaceId).toBe("space-1");
    expect(query.flags).toEqual([]);
  });

  it("treats whitespace-only search text as no search at all", () => {
    notesFiltersStore.getState().setText("   ");
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).text).toBeNull();
  });
});

describe("space entry and explicit merging", () => {
  it("replaces query terms but preserves the person's drives and session privacy", () => {
    const state = notesFiltersStore.getState();
    state.setText("old prompt");
    state.setTagTerm("old", "include");
    state.setAgentOnly(true);
    state.setPinnedOnly(true);
    state.setVaultIds(["other"]);
    state.setIncludePrivate(true);
    state.enterSpace(
      space("B", { tagTerms: { draft: "exclude" }, flags: ["archived"], sort: "name asc" }),
    );
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      text: null,
      tags: { draft: "exclude" },
      origin: null,
      flags: ["archived"],
      sort: "name asc",
      vaultIds: ["other"],
      includePrivate: true,
      spaceId: "B",
      spaceTerms: false,
    });
    state.removeTag("draft");
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      tags: {},
      spaceTerms: false,
    });
    state.enterSpace(space("C", { text: "saved prompt" }));
    expect(notesFiltersStore.getState().text).toBe("saved prompt");
    state.enterSpace(space("keeper:all"));
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      spaceId: null,
      text: null,
      vaultIds: ["other"],
      includePrivate: true,
    });
  });

  it("ANDs a named merge without replacing the current prompt or drive selection", () => {
    const state = notesFiltersStore.getState();
    state.enterSpace(space("A", { tagTerms: { work: "include" }, text: "pricing" }));
    state.setVaultIds(["v2"]);
    state.mergeSpace(space("B", { tagTerms: { draft: "exclude" }, flags: ["archived"] }));
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      text: "pricing",
      tags: { work: "include", draft: "exclude" },
      flags: ["archived"],
      spaceId: "A",
      vaultIds: ["v2"],
    });
  });

  it("refuses opaque or contradictory merges without silently widening a query", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("draft", "include");
    expect(() => state.mergeSpace(space("opaque", { opaque: true }))).toThrow(/open it instead/i);
    expect(() => state.mergeSpace(space("opposite", { tagTerms: { draft: "exclude" } }))).toThrow(
      /opposite filters/,
    );
    expect(terms()).toEqual([{ tag: "draft", term: "include" }]);
    state.enterSpace(space("opaque", { opaque: true }));
    expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
      spaceId: "opaque",
      spaceTerms: true,
      tags: {},
      text: null,
    });
  });
});

it.each(["scope", "escape"] as const)("forgets an explicit space sort on leaving by %s", (exit) => {
  const state = notesFiltersStore.getState();
  state.enterSpace(space("work", { sort: "modified desc", text: "budget" }));
  expect(noteQueryFor(notesFiltersStore.getState(), 0, 20).sort).toBe("modified desc");
  if (exit === "scope") state.setScope(ALL_NOTES_SCOPE);
  else state.dropLastChip();
  expect(noteQueryFor(notesFiltersStore.getState(), 0, 20)).toMatchObject({
    spaceId: null,
    sort: null,
    text: "budget",
  });
  expect(notesFiltersStore.getState().enteredSpace).toBeNull();
});

describe("dropLastChip", () => {
  it("walks the bar down from its end, one press at a time", () => {
    const state = notesFiltersStore.getState();
    state.enterSpace(space("s-inbox"));
    state.setTagTerm("work", "include");
    state.setTagTerm("urgent", "include");
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
    notesFiltersStore.getState().setTagTerm("work", "include");
    // This boolean is what picks between "this vault is empty" and "no notes
    // match these filters", so a false negative would word an over-filtered
    // list as an empty vault.
    expect(isFiltered(notesFiltersStore.getState())).toBe(true);
  });
});

describe("tag chip signs", () => {
  it("flips each chip state into its next sign", () => {
    expect(flipTagTerm("off")).toBe("include");
    expect(flipTagTerm("include")).toBe("exclude");
    expect(flipTagTerm("exclude")).toBe("include");
  });

  it("ten alternating flips never empty tagTerms", () => {
    const state = notesFiltersStore.getState();
    state.setTagTerm("draft", "include");
    for (let flip = 0; flip < 10; flip += 1) {
      state.toggleTagSign("draft");
      expect(terms()).toEqual([{ tag: "draft", term: flip % 2 === 0 ? "exclude" : "include" }]);
    }
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
    state.enterSpace({ ...space("s-inbox"), name: "Inbox" });
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

describe("service visibility is a preference, not a chip", () => {
  it("sends the eye state without treating it as a savable filter", () => {
    for (const hidden of [true, false]) {
      notesFiltersStore.getState().setHideServiceFiles(hidden);
      expect(noteQueryFor(notesFiltersStore.getState(), 0, 200).hideServiceFiles).toBe(hidden);
      expect(isFiltered(notesFiltersStore.getState())).toBe(false);
      expect(isScopeOnly(notesFiltersStore.getState())).toBe(true);
      notesFiltersStore.getState().dropLastChip();
      notesFiltersStore.getState().clearAll();
      expect(notesFiltersStore.getState().hideServiceFiles).toBe(hidden);
    }
  });

  it("ignores a hydrate started before the user's choice", async () => {
    let resolve: (hidden: boolean) => void = () => {};
    vi.mocked(notesHideServiceFilesGet).mockReturnValue(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const hydrate = hydrateHideServiceFiles();
    await persistHideServiceFiles(false);
    resolve(true);
    await hydrate;
    expect(notesFiltersStore.getState().hideServiceFiles).toBe(false);
  });

  it("restores the acknowledged choice when the latest write fails", async () => {
    vi.mocked(notesHideServiceFilesGet).mockResolvedValue(false);
    await hydrateHideServiceFiles();
    vi.mocked(notesHideServiceFilesSet).mockRejectedValue(new Error("disk full"));
    await expect(persistHideServiceFiles(true)).rejects.toThrow("disk full");
    expect(notesFiltersStore.getState().hideServiceFiles).toBe(false);
  });

  it("serializes fast presses so the final acknowledged choice wins", async () => {
    let release: () => void = () => {};
    vi.mocked(notesHideServiceFilesSet).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const first = persistHideServiceFiles(false);
    const second = persistHideServiceFiles(true);
    await Promise.resolve();
    expect(notesHideServiceFilesSet).toHaveBeenCalledTimes(1);
    release();
    await Promise.all([first, second]);
    expect(vi.mocked(notesHideServiceFilesSet).mock.calls).toEqual([[false], [true]]);
    expect(notesFiltersStore.getState().hideServiceFiles).toBe(true);
  });
});

it("does not let space entry rewrite a session privacy read already in flight", async () => {
  let release: (value: boolean) => void = () => {};
  vi.mocked(notesIncludePrivateGet).mockReturnValue(
    new Promise((resolve) => {
      release = resolve;
    }),
  );
  const hydration = hydrateIncludePrivate();
  await Promise.resolve();
  notesFiltersStore.getState().enterSpace(space("work"));
  release(true);
  await hydration;
  expect(noteQueryFor(notesFiltersStore.getState(), 0, 20).includePrivate).toBe(true);
});

it("rolls private visibility back only when its current write fails", async () => {
  await hydrateIncludePrivate();
  vi.mocked(notesIncludePrivateSet).mockRejectedValueOnce(new Error("disk full"));
  await expect(persistIncludePrivate(true)).rejects.toThrow("disk full");
  expect(notesFiltersStore.getState().includePrivate).toBe(false);
});
