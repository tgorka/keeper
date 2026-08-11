import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  FILES_TREE_COOKIE,
  FILES_TREE_COOKIE_BUDGET,
  FILES_TREE_LIMIT,
  FILES_TREE_MAX_AGE,
  filesTreeCookie,
  filesTreeStore,
  hydrateFilesTree,
  nodeKey,
  reachableNodeKeys,
  readFilesTree,
  resetFilesTreeForTest,
} from "@/lib/stores/files-tree";

/**
 * A jar with other people's cookies in it, and the tree's own somewhere inside.
 *
 * Two foreign entries, one of them `sidebar_state` — a name keeper has never
 * written and which `sidebar-fold.test.ts` uses for the same reason. A
 * single-entry jar cannot distinguish "found the right name" from "found the
 * only name".
 */
function jar(value: string): string {
  return `theme=dark; ${FILES_TREE_COOKIE}=${encodeURIComponent(value)}; sidebar_state=true`;
}

/** The cookie a tree with these folders open would be written as, as a jar. */
function written(keys: readonly string[]): string {
  const assignment = filesTreeCookie(new Set(keys));
  return `theme=dark; ${assignment.slice(0, assignment.indexOf(";"))}`;
}

/**
 * The node keys a cookie assignment literally carries, decoded without going
 * back through {@link readFilesTree}.
 *
 * The reader applies {@link FILES_TREE_LIMIT} too, to bound a hand-edited jar.
 * Round-tripping a write-side bound through it would therefore pass whether the
 * writer bounded anything or not — which is exactly what a mutation of the
 * writer's own `slice` proved. So the writer's rules are asserted on its bytes.
 */
function carried(assignment: string): string[] {
  const value = assignment.slice(FILES_TREE_COOKIE.length + 1, assignment.indexOf(";"));
  const decoded = JSON.parse(decodeURIComponent(value)) as {
    p: Record<string, string[]>;
  };
  return Object.entries(decoded.p).flatMap(([id, subpaths]) =>
    subpaths.map((subpath) => nodeKey(id, subpath)),
  );
}

const VAULT = "01VAULTVAULTVAULTVAULTVAUL";
const FIELD = "01FIELDFIELDFIELDFIELDFIEL";

beforeEach(() => {
  resetFilesTreeForTest();
});

afterEach(() => {
  resetFilesTreeForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${FILES_TREE_COOKIE}=; path=/; max-age=0`;
});

describe("readFilesTree", () => {
  it("reads every open folder out of a jar full of other cookies", () => {
    expect(readFilesTree(jar(`{"v":1,"p":{"${VAULT}":["","Notes","Notes/2026"]}}`))).toEqual(
      new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "Notes"), nodeKey(VAULT, "Notes/2026")]),
    );
  });

  it("is nothing open when the cookie is absent", () => {
    expect(readFilesTree("")).toEqual(new Set());
    expect(readFilesTree("theme=dark; sidebar_state=true")).toEqual(new Set());
  });

  it("keeps two profiles apart", () => {
    const keys = readFilesTree(jar(`{"v":1,"p":{"${VAULT}":["a"],"${FIELD}":["a"]}}`));
    expect(keys).toEqual(new Set([nodeKey(VAULT, "a"), nodeKey(FIELD, "a")]));
    // The separator is the whole reason this is not `profile:subpath`: two
    // different pairs must never encode to one string.
    expect(keys.size).toBe(2);
  });

  it("discards a tree it cannot read rather than throwing at boot", () => {
    // Nothing on screen yet when this runs, so a throw here is a white window.
    for (const value of ["", "{", "null", "[]", '{"v":1}', '{"p":{}}', '"a"', "7"]) {
      expect(readFilesTree(jar(value)), value).toEqual(new Set());
    }
  });

  it("discards a version this build does not know rather than guessing at it", () => {
    // The whole point of `v`: a later build's vocabulary must cost this one the
    // tree, never a folder that meant something else.
    expect(readFilesTree(jar(`{"v":2,"p":{"${VAULT}":["Notes"]}}`))).toEqual(new Set());
  });

  it("drops an entry that is not a relative path and keeps the ones that are", () => {
    // A cookie is a string the user can edit, and these go straight into
    // `sync_browse` as a subpath (AD-65). This is the outer of two gates.
    const hostile = [
      "/etc/passwd",
      "../../etc/passwd",
      "a/../../b",
      "\\\\server\\share",
      "C:/Windows",
      "c:\\Windows",
    ];
    const value = JSON.stringify({ v: 1, p: { [VAULT]: [...hostile, "Notes"] } });
    expect(readFilesTree(jar(value))).toEqual(new Set([nodeKey(VAULT, "Notes")]));
  });

  it("drops an entry that is not a string, and a profile that is not a list", () => {
    const value = JSON.stringify({ v: 1, p: { [VAULT]: ["ok", 7, null, {}], "": ["x"], z: "no" } });
    expect(readFilesTree(jar(value))).toEqual(new Set([nodeKey(VAULT, "ok")]));
  });

  it("bounds a hand-edited cookie, so a paste cannot buy five thousand browse calls", () => {
    const many = Array.from({ length: FILES_TREE_LIMIT * 4 }, (_, i) => `f${i}`);
    expect(readFilesTree(jar(JSON.stringify({ v: 1, p: { [VAULT]: many } }))).size).toBe(
      FILES_TREE_LIMIT,
    );
  });

  it("round trips an expansion through its own writer", () => {
    const keys = [
      nodeKey(VAULT, ""),
      nodeKey(VAULT, "10-notes"),
      nodeKey(VAULT, "10-notes/projects"),
      nodeKey(FIELD, ""),
      nodeKey(FIELD, "a b/c'd (e)/f,g"),
    ];
    expect(readFilesTree(written(keys))).toEqual(new Set(keys));
  });

  it("round trips a path holding the characters the fold's own format uses", () => {
    // `sidebar-fold.ts` packs `key:value|key:value`. Both are legal in a path,
    // which is why this module encodes JSON instead of borrowing that shape.
    const keys = [nodeKey(VAULT, "a|b"), nodeKey(VAULT, "c:d"), nodeKey(VAULT, "e;f")];
    expect(readFilesTree(written(keys))).toEqual(new Set(keys));
  });
});

describe("filesTreeCookie", () => {
  it("writes a year-long path-wide cookie under the keeper-prefixed name", () => {
    const assignment = filesTreeCookie(new Set([nodeKey(VAULT, "Notes")]));
    expect(assignment.startsWith(`${FILES_TREE_COOKIE}=`)).toBe(true);
    expect(assignment).toContain("path=/");
    expect(assignment).toContain(`max-age=${FILES_TREE_MAX_AGE}`);
    expect(assignment).toContain("samesite=lax");
  });

  it("forgets the cookie when nothing is open rather than storing an empty tree", () => {
    const assignment = filesTreeCookie(new Set());
    expect(assignment).toContain("max-age=0");
    expect(readFilesTree(assignment)).toEqual(new Set());
  });

  it("writes the same bytes for the same set whatever order it was built in", () => {
    const keys = [nodeKey(VAULT, "b"), nodeKey(VAULT, "a"), nodeKey(FIELD, "")];
    expect(filesTreeCookie(new Set(keys))).toBe(filesTreeCookie(new Set([...keys].reverse())));
  });

  it("keeps the limit and drops the deepest", () => {
    // Short names, so the count is the only rule in play here — the byte
    // backstop has its own test below, and a case where both bind at once
    // would not say which one did the dropping.
    const shallow = Array.from({ length: 24 }, (_, i) => `a${i}`);
    const deeper = Array.from({ length: 16 }, (_, i) => `a0/b${i}`);
    const keys = [nodeKey(VAULT, ""), ...[...shallow, ...deeper].map((p) => nodeKey(VAULT, p))];
    expect(keys.length).toBeGreaterThan(FILES_TREE_LIMIT);

    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const kept = new Set(carried(filesTreeCookie(new Set(keys))));
    info.mockRestore();

    expect(kept.size).toBe(FILES_TREE_LIMIT);
    // The shallow end survives whole — root and every depth-1 folder — and the
    // depth-2 tail is what went. A node whose ancestors did not fit would
    // restore into nothing, so the bottom is the part that can be spared.
    expect(kept.has(nodeKey(VAULT, ""))).toBe(true);
    for (const path of shallow) {
      expect(kept.has(nodeKey(VAULT, path)), path).toBe(true);
    }
    expect(kept.has(nodeKey(VAULT, "a0/b0"))).toBe(true);
    expect(kept.has(nodeKey(VAULT, "a0/b15"))).toBe(false);
  });

  it("says so rather than truncating silently", () => {
    const keys = Array.from({ length: FILES_TREE_LIMIT + 3 }, (_, i) => nodeKey(VAULT, `f${i}`));
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    filesTreeCookie(new Set(keys));
    expect(info).toHaveBeenCalledWith(
      `keeper: remembering ${FILES_TREE_LIMIT} of ${keys.length} open folders — the rest do not fit in a cookie.`,
    );
    info.mockRestore();
  });

  it("never writes past the byte budget, however long the paths are", () => {
    // The count is the bound that normally binds; this is the backstop for
    // paths long enough that even a few of them overrun. A browser drops an
    // oversized cookie silently, which would lose the whole tree instead of
    // its deep end.
    const segment = "a-folder-with-a-very-long-name-indeed";
    const keys: string[] = [];
    let path = "";
    for (let i = 0; i < FILES_TREE_LIMIT; i += 1) {
      path = path === "" ? `${segment}0` : `${path}/${segment}${i}`;
      keys.push(nodeKey(VAULT, path));
    }
    const info = vi.spyOn(console, "info").mockImplementation(() => undefined);
    const assignment = filesTreeCookie(new Set(keys));
    info.mockRestore();

    const value = assignment.slice(FILES_TREE_COOKIE.length + 1, assignment.indexOf(";"));
    expect(value.length).toBeLessThanOrEqual(FILES_TREE_COOKIE_BUDGET);
    // It gave up bytes, not the whole tree: the shallow end is still there.
    const kept = carried(assignment);
    expect(kept.length).toBeGreaterThan(0);
    expect(kept.length).toBeLessThan(FILES_TREE_LIMIT);
    expect(kept).toContain(nodeKey(VAULT, `${segment}0`));
  });

  it("fits an ordinary tree of the limit's size well inside the budget", () => {
    // If this ever fails the count has stopped being the binding rule and the
    // byte backstop has become the everyday one, which is the state the two
    // constants were chosen to avoid.
    const keys = Array.from({ length: FILES_TREE_LIMIT }, (_, i) =>
      nodeKey(i % 2 === 0 ? VAULT : FIELD, `10-notes/projects/2026/quarter-${i}`),
    );
    const assignment = filesTreeCookie(new Set(keys));
    const value = assignment.slice(FILES_TREE_COOKIE.length + 1, assignment.indexOf(";"));
    expect(value.length).toBeLessThanOrEqual(FILES_TREE_COOKIE_BUDGET);
    expect(readFilesTree(assignment).size).toBe(FILES_TREE_LIMIT);
  });
});

describe("reachableNodeKeys", () => {
  it("keeps a node whose ancestors are all open", () => {
    const expanded = new Set([
      nodeKey(VAULT, ""),
      nodeKey(VAULT, "a"),
      nodeKey(VAULT, "a/b"),
      nodeKey(VAULT, "a/b/c"),
    ]);
    expect(new Set(reachableNodeKeys(expanded))).toEqual(expanded);
  });

  it("drops a node hidden under a shut parent, without forgetting it", () => {
    // Collapsing a folder deliberately keeps what was open inside it, so
    // re-opening the branch comes back the way it was left. The consequence is
    // that the set holds nodes nothing renders, and browsing for one at mount
    // would be an IPC call for a row with nowhere to go.
    const expanded = new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "a/b"), nodeKey(VAULT, "a/b/c")]);
    expect(reachableNodeKeys(expanded)).toEqual([nodeKey(VAULT, "")]);
  });

  it("needs the profile root open before anything under it counts", () => {
    expect(reachableNodeKeys(new Set([nodeKey(VAULT, "a")]))).toEqual([]);
  });

  it("does not let one profile's open folder reach into another's", () => {
    const expanded = new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "a"), nodeKey(FIELD, "a")]);
    expect(new Set(reachableNodeKeys(expanded))).toEqual(
      new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "a")]),
    );
  });
});

describe("the files-tree store", () => {
  it("opens a folder and writes the whole expansion out", () => {
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, "Notes"), true);

    expect(filesTreeStore.getState().expanded).toEqual(
      new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "Notes")]),
    );
    // The document really carries it: a store that opens and does not persist
    // is a tree that survives until the next surface switch and no further,
    // which is the whole of what this module exists for.
    expect(readFilesTree(document.cookie)).toEqual(
      new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "Notes")]),
    );
  });

  it("shuts a folder and takes it back out of the cookie", () => {
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, "Notes"), true);
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, "Notes"), false);

    expect(readFilesTree(document.cookie)).toEqual(new Set([nodeKey(VAULT, "")]));
  });

  it("is idempotent, so a caller that already has the state it wants does nothing", () => {
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    const before = filesTreeStore.getState().expanded;
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    expect(filesTreeStore.getState().expanded).toBe(before);
  });

  it("forgets every folder under a profile that no longer exists", () => {
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    filesTreeStore.getState().setNodeOpen(nodeKey(FIELD, ""), true);
    filesTreeStore.getState().setNodeOpen(nodeKey(FIELD, "a"), true);

    filesTreeStore.getState().retainProfiles([VAULT]);

    expect(filesTreeStore.getState().expanded).toEqual(new Set([nodeKey(VAULT, "")]));
    // And out of the cookie too, or it would be a key nothing can ever clear.
    expect(readFilesTree(document.cookie)).toEqual(new Set([nodeKey(VAULT, "")]));
  });

  it("leaves the expansion alone when every profile is still there", () => {
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, ""), true);
    const before = filesTreeStore.getState().expanded;
    filesTreeStore.getState().retainProfiles([VAULT, FIELD]);
    expect(filesTreeStore.getState().expanded).toBe(before);
  });

  it("restores a remembered expansion, once", () => {
    hydrateFilesTree(jar(`{"v":1,"p":{"${VAULT}":["","Notes"]}}`));
    expect(filesTreeStore.getState().expanded).toEqual(
      new Set([nodeKey(VAULT, ""), nodeKey(VAULT, "Notes")]),
    );

    // Idempotent: React's double-invoked development effects must restore once,
    // and a second call must not re-open a folder the user has since shut.
    filesTreeStore.getState().setNodeOpen(nodeKey(VAULT, "Notes"), false);
    hydrateFilesTree(jar(`{"v":1,"p":{"${VAULT}":["","Notes"]}}`));
    expect(filesTreeStore.getState().expanded).toEqual(new Set([nodeKey(VAULT, "")]));
  });

  it("leaves the tree shut when there is nothing remembered", () => {
    hydrateFilesTree("theme=dark");
    expect(filesTreeStore.getState().expanded).toEqual(new Set());
  });
});
