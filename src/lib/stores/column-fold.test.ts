import { afterEach, describe, expect, it } from "vitest";
import { SURFACE_COLUMN_IDS } from "@/lib/column-widths";
import {
  COLUMN_FOLD_COOKIE,
  columnFoldCookie,
  columnFoldStore,
  columnsUnfolded,
  hydrateColumnFold,
  readColumnFold,
  resetColumnFoldForTest,
} from "@/lib/stores/column-fold";

/**
 * The encoding and the store (Story 48.1).
 *
 * What this file deliberately cannot prove is that anything MOUNTS the restore.
 * `hydrateColumnFold` is called by `AppShell`, and a store-level test passes
 * unchanged on a build where that call was deleted (DW-172). That assertion
 * lives in `app-shell.test.tsx`, against the real shell.
 */

afterEach(() => {
  resetColumnFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing cookie state is this test's subject
  document.cookie = `${COLUMN_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("column fold cookie", () => {
  it("round-trips every column through its own cookie", () => {
    const fold = {
      "notes-rail": true,
      "notes-list": false,
      "files-tree": true,
      "chat-list": false,
      "tasks-list": true,
      "bots-list": false,
    };
    expect(readColumnFold(columnFoldCookie(fold))).toEqual(fold);
  });

  it("writes every column, not only the folded ones", () => {
    // A cookie write replaces the name's whole value, so an omitted column
    // would be indistinguishable from one an older build never knew about.
    const value = columnFoldCookie(columnsUnfolded());
    for (const id of SURFACE_COLUMN_IDS) {
      expect(decodeURIComponent(value)).toContain(`${id}:0`);
    }
  });

  it("keeps a column open when the jar says something it cannot read", () => {
    // Three shapes of damage in one value: a key this build has no column for,
    // a value that is not 0/1, and an entry with no colon at all. None of them
    // may cost the user a surface.
    const damaged = `${COLUMN_FOLD_COOKIE}=${encodeURIComponent("nope:1|notes-rail:yes|chat-list")}`;
    expect(readColumnFold(damaged)).toEqual(columnsUnfolded());
  });

  it("reads its own cookie and not another fold's", () => {
    // `keeper_sidebar_fold` and `keeper_notes_rail_fold` are in the same jar and
    // one of them has a `spaces` key. A column fold and a section fold are
    // different facts and must not be able to read each other.
    const foreign = "keeper_notes_rail_fold=spaces%3A1%7Ctags%3A1%7Cfiles%3A1";
    expect(readColumnFold(foreign)).toEqual(columnsUnfolded());
  });

  it("starts with every column showing", () => {
    // The rail's Files section defaults FOLDED (a cold directory scan per
    // expansion). No column has an equivalent cost, and one that started away
    // would read as a surface that failed to render.
    expect(readColumnFold("")).toEqual({
      "notes-rail": false,
      "notes-list": false,
      "files-tree": false,
      "chat-list": false,
      "tasks-list": false,
      "bots-list": false,
    });
  });
});

describe("column fold store", () => {
  it("toggles one column and writes the whole set out", () => {
    columnFoldStore.getState().toggleColumn("notes-list");

    expect(columnFoldStore.getState().columns["notes-list"]).toBe(true);
    expect(columnFoldStore.getState().columns["notes-rail"]).toBe(false);
    // Through the same parser a restart would use, not through the store.
    expect(readColumnFold(document.cookie)["notes-list"]).toBe(true);
  });

  it("toggles back", () => {
    columnFoldStore.getState().toggleColumn("files-tree");
    columnFoldStore.getState().toggleColumn("files-tree");
    expect(readColumnFold(document.cookie)["files-tree"]).toBe(false);
  });

  it("restores once, so a second hydrate cannot overwrite a live fold", () => {
    hydrateColumnFold(columnFoldCookie({ ...columnsUnfolded(), "chat-list": true }));
    expect(columnFoldStore.getState().columns["chat-list"]).toBe(true);

    columnFoldStore.getState().toggleColumn("chat-list");
    // React's double-invoked development effect, or a second surface mounting.
    hydrateColumnFold(columnFoldCookie({ ...columnsUnfolded(), "chat-list": true }));

    expect(columnFoldStore.getState().columns["chat-list"]).toBe(false);
  });
});
