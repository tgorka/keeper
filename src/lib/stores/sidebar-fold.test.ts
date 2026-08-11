import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  hydrateSidebarFold,
  readSidebarFold,
  resetSidebarFoldForTest,
  SIDEBAR_FOLD_COOKIE,
  SIDEBAR_GROUPS,
  sidebarFoldCookie,
  sidebarFoldStore,
  unfolded,
} from "@/lib/stores/sidebar-fold";

/**
 * A jar with other people's cookies in it, and the fold's own somewhere inside.
 *
 * Two foreign entries rather than one, and one of them is `sidebar_state` — the
 * cookie a doc comment in this repo claimed keeper writes and which nothing ever
 * has. A single-entry jar cannot distinguish "found the right name" from "found
 * the only name".
 */
function jar(value: string): string {
  return `theme=dark; ${SIDEBAR_FOLD_COOKIE}=${encodeURIComponent(value)}; sidebar_state=true`;
}

beforeEach(() => {
  resetSidebarFoldForTest();
});

afterEach(() => {
  resetSidebarFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${SIDEBAR_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("readSidebarFold", () => {
  it("reads the menu and every group out of a jar full of other cookies", () => {
    expect(readSidebarFold(jar("menu:1|spaces:0|networks:1"))).toEqual({
      menu: true,
      groups: { spaces: false, networks: true },
    });
  });

  it("is nothing folded when the cookie is absent", () => {
    expect(readSidebarFold("")).toEqual(unfolded());
    expect(readSidebarFold("theme=dark; sidebar_state=true")).toEqual(unfolded());
  });

  it("drops a malformed entry rather than refusing to render the menu", () => {
    // A jar is shared with every other cookie on the origin and with older
    // builds of keeper. The only safe direction for a value keeper cannot read
    // is "open", because that is the state in which every control is reachable.
    for (const value of ["", "menu", "menu:", "menu:yes", "menu:2", ":1", "|||"]) {
      expect(readSidebarFold(jar(value)), value).toEqual(unfolded());
    }
  });

  it("drops a key this build does not know and keeps the ones it does", () => {
    expect(readSidebarFold(jar("menu:1|someday:1|networks:1"))).toEqual({
      menu: true,
      groups: { spaces: false, networks: true },
    });
  });

  it("round trips every fold state through its own writer", () => {
    // Both directions per field, not just the folded one: a writer that emitted
    // `1` unconditionally would pass a test that only ever wrote `true`.
    for (const menu of [false, true]) {
      for (const spaces of [false, true]) {
        for (const networks of [false, true]) {
          const fold = { menu, groups: { spaces, networks } };
          const written = sidebarFoldCookie(fold);
          expect(readSidebarFold(written), JSON.stringify(fold)).toEqual(fold);
        }
      }
    }
  });

  it("writes a year-long path-wide cookie under the keeper-prefixed name", () => {
    const written = sidebarFoldCookie(unfolded());
    expect(written.startsWith(`${SIDEBAR_FOLD_COOKIE}=`)).toBe(true);
    expect(written).toContain("path=/");
    expect(written).toContain(`max-age=${60 * 60 * 24 * 365}`);
  });

  it("records every group, so unfolded is distinguishable from unwritten", () => {
    const written = sidebarFoldCookie(unfolded());
    for (const group of SIDEBAR_GROUPS) {
      expect(decodeURIComponent(written)).toContain(`${group}:0`);
    }
  });
});

describe("the fold store", () => {
  it("toggles the menu and writes the whole state out", () => {
    sidebarFoldStore.getState().toggleGroup("networks");
    sidebarFoldStore.getState().toggleMenu();

    expect(sidebarFoldStore.getState().menu).toBe(true);
    expect(sidebarFoldStore.getState().groups).toEqual({ spaces: false, networks: true });
    // The document really carries it: a store that toggles and does not persist
    // is a fold that survives until the next launch and no further, which is
    // the whole of what this module exists for.
    expect(readSidebarFold(document.cookie)).toEqual({
      menu: true,
      groups: { spaces: false, networks: true },
    });
  });

  it("folds one group without folding its neighbour", () => {
    sidebarFoldStore.getState().toggleGroup("spaces");
    expect(sidebarFoldStore.getState().groups).toEqual({ spaces: true, networks: false });
  });

  it("restores a remembered fold, once", () => {
    hydrateSidebarFold(jar("menu:1|spaces:1|networks:0"));
    expect(sidebarFoldStore.getState().menu).toBe(true);
    expect(sidebarFoldStore.getState().groups.spaces).toBe(true);

    // Idempotent: React's double-invoked development effects must restore once,
    // and a second call must not undo a fold the user has already changed.
    sidebarFoldStore.getState().toggleMenu();
    hydrateSidebarFold(jar("menu:1|spaces:1|networks:0"));
    expect(sidebarFoldStore.getState().menu).toBe(false);
  });
});
