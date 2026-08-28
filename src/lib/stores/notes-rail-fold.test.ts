import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  hydrateNotesRailFold,
  NOTES_RAIL_FOLD_COOKIE,
  NOTES_RAIL_GROUPS,
  notesRailFoldCookie,
  notesRailFoldStore,
  notesRailUnfolded,
  readNotesRailFold,
  resetNotesRailFoldForTest,
} from "@/lib/stores/notes-rail-fold";
import { readSidebarFold, SIDEBAR_FOLD_COOKIE } from "@/lib/stores/sidebar-fold";

/**
 * A jar with other people's cookies in it, and the rail's own somewhere inside.
 *
 * One of the foreign entries is the CHAT SIDEBAR's fold, with its own `spaces`
 * key set. That is the collision this story exists to make impossible, and a jar
 * without it could not tell "found the right name" from "found the only name".
 */
function jar(value: string): string {
  return [
    "theme=dark",
    `${SIDEBAR_FOLD_COOKIE}=${encodeURIComponent("menu:1|spaces:1|networks:1")}`,
    `${NOTES_RAIL_FOLD_COOKIE}=${encodeURIComponent(value)}`,
  ].join("; ");
}

beforeEach(resetNotesRailFoldForTest);

afterEach(() => {
  resetNotesRailFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${NOTES_RAIL_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("readNotesRailFold", () => {
  /**
   * The default is not "everything open", and the difference is a cold scan.
   *
   * Files loads one `notes_tree` per expanded directory and has arrived
   * collapsed since Story 37.9; an all-open default would make every mount of
   * the notes surface walk the vault root. Spaces and Tags are already in hand
   * by the time they render, so open is their honest default.
   */
  it("starts Spaces and Tags open and Files shut when there is no cookie", () => {
    expect(readNotesRailFold("theme=dark")).toEqual({ spaces: false, tags: false, files: true });
  });

  it("reads every section out of a jar full of other cookies", () => {
    expect(readNotesRailFold(jar("spaces:1|tags:0|files:0"))).toEqual({
      spaces: true,
      tags: false,
      files: false,
    });
  });

  it("drops a malformed entry rather than refusing to render the rail", () => {
    // No colon, a value that is not 0/1, and an empty entry. Each leaves its
    // section at its default; none of them takes the rail down.
    expect(readNotesRailFold(jar("spaces|tags:yes||files:1"))).toEqual({
      spaces: false,
      tags: false,
      files: true,
    });
  });

  it("drops a key this build does not know and keeps the ones it does", () => {
    expect(readNotesRailFold(jar("spaces:1|recordings:1|files:0"))).toEqual({
      spaces: true,
      tags: false,
      files: false,
    });
  });

  it("round trips every fold state through its own writer", () => {
    for (const spaces of [false, true]) {
      for (const tags of [false, true]) {
        for (const files of [false, true]) {
          const fold = { spaces, tags, files };
          expect(readNotesRailFold(notesRailFoldCookie(fold))).toEqual(fold);
        }
      }
    }
  });

  it("writes a year-long path-wide cookie under the keeper-prefixed name", () => {
    expect(notesRailFoldCookie({ spaces: true, tags: false, files: true })).toBe(
      `${NOTES_RAIL_FOLD_COOKIE}=${encodeURIComponent("spaces:1|tags:0|files:1")}; path=/; max-age=${60 * 60 * 24 * 365}`,
    );
  });

  it("records every section, so open is distinguishable from unwritten", () => {
    // Files is the section where the two already differ: unwritten means shut,
    // and a writer that omitted open sections would say nothing about a Files
    // the user has deliberately opened.
    expect(notesRailFoldCookie({ spaces: false, tags: false, files: false })).toContain(
      encodeURIComponent("spaces:0|tags:0|files:0"),
    );
  });
});

/**
 * The test for the branch this story did NOT take.
 *
 * The cheap shape was one cookie and one widened union, chat and notes sharing a
 * namespace. It was rejected because `SIDEBAR_GROUPS` already contains `spaces`
 * and so does the notes rail, and they are different sections on different
 * surfaces — a shared namespace would make folding one silently fold the other.
 * These two assertions are what make that rejection real rather than stated: if
 * anyone later merges the two key sets, both fail.
 */
describe("the two surfaces do not read each other", () => {
  it("leaves the notes rail at its defaults when only the chat sidebar has a cookie", () => {
    const chatOnly = `theme=dark; ${SIDEBAR_FOLD_COOKIE}=${encodeURIComponent("menu:1|spaces:1|networks:1")}`;

    expect(readNotesRailFold(chatOnly)).toEqual({ spaces: false, tags: false, files: true });
  });

  it("leaves the chat sidebar at its defaults when only the notes rail has a cookie", () => {
    const railOnly = `theme=dark; ${NOTES_RAIL_FOLD_COOKIE}=${encodeURIComponent("spaces:1|tags:1|files:0")}`;

    expect(readSidebarFold(railOnly)).toEqual({
      menu: false,
      groups: { spaces: false, networks: false },
    });
  });

  it("names two different cookies, which is the whole reason the above holds", () => {
    expect(NOTES_RAIL_FOLD_COOKIE).not.toBe(SIDEBAR_FOLD_COOKIE);
  });
});

describe("the notes rail fold store", () => {
  it("folds a section, writes the whole rail out, and folds it back", () => {
    notesRailFoldStore.getState().toggleGroup("tags");

    expect(notesRailFoldStore.getState().groups.tags).toBe(true);
    // The write carries the sections nobody touched too, at their real values.
    expect(readNotesRailFold(document.cookie)).toEqual({
      spaces: false,
      tags: true,
      files: true,
    });

    notesRailFoldStore.getState().toggleGroup("tags");
    expect(readNotesRailFold(document.cookie).tags).toBe(false);
  });

  it("toggles one section without disturbing the others", () => {
    for (const group of NOTES_RAIL_GROUPS) {
      resetNotesRailFoldForTest();
      notesRailFoldStore.getState().toggleGroup(group);
      const expected = notesRailUnfolded();
      expected[group] = !expected[group];

      expect(notesRailFoldStore.getState().groups).toEqual(expected);
    }
  });

  it("restores a remembered fold, once", () => {
    hydrateNotesRailFold(jar("spaces:1|tags:0|files:0"));
    expect(notesRailFoldStore.getState().groups).toEqual({
      spaces: true,
      tags: false,
      files: false,
    });

    // A second hydrate must not undo a fold the user has changed since the
    // first — the shell can mount twice and React double-invokes effects.
    notesRailFoldStore.getState().toggleGroup("spaces");
    hydrateNotesRailFold(jar("spaces:1|tags:0|files:0"));
    expect(notesRailFoldStore.getState().groups.spaces).toBe(false);
  });
});
