/**
 * Every `<FileControlled settingKey="…" />` names a settings key a file can
 * actually set (Story 46.7, AD-98).
 *
 * **The failure this stops.** `FileControlled` looks its key up in the layer
 * stack and renders `null` when it finds nothing — which is the correct
 * behaviour for a key no file happens to set, and is *indistinguishable* from
 * the behaviour for a key that does not exist. A typo (`notify.preview_enabled`
 * for `notify.previews_enabled`), a rename in `keys.rs`, or a marker put on a
 * control whose key no file may ever set, all produce the same thing: a marker
 * that never appears, forever, with a green tree and a user whose switch keeps
 * flipping back with nothing on screen to explain it. Story 45.19's shape
 * again — *does this thing name something, and does anything check the thing it
 * names exists?*
 *
 * `docs/settings-keys.md` is the right side of the comparison because it is
 * generated from `keeper_core::config::keys::KEYS` and pinned by a test that
 * fails if the two drift (`config::keys::tests::docs`). So this reads the
 * registry, one remove, without needing to build the Rust that owns it — which
 * matters, because half the markers live in a shell crate that does not compile
 * on Linux (AD-55/AD-56).
 *
 * The comparison is deliberately against the two *settable* sections only. A
 * marker on a key from "Keys no file may set" would be a promise keeper cannot
 * keep: those are latches and session state that keeper rewrites itself, so no
 * file will ever set one and the badge could not appear even in principle.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = resolve(import.meta.dirname, "..");
const DOC = readFileSync(resolve(SRC, "../docs/settings-keys.md"), "utf8");

/** Every `.ts`/`.tsx` file under `src/`, so no marker can hide in a corner. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      return sources(path);
    }
    return path.endsWith(".ts") || path.endsWith(".tsx") ? [path] : [];
  });
}

/**
 * The keys the settables sections list, which is `KEYS` minus the never-settable
 * ones. Sliced at the heading rather than by scanning the whole document: the
 * third section is a table of the same shape, and including it would let a
 * marker on a latch pass.
 */
const settable = (() => {
  const start = DOC.indexOf("## Keys a file may set");
  const end = DOC.indexOf("## Keys no file may set");
  return new Set(
    [...DOC.slice(start, end).matchAll(/^\| `([a-z_][a-z0-9_.]*)` \|/gm)].map(([, key]) => key),
  );
})();

/**
 * Every key a marker in `src/` claims a file can decide.
 *
 * This file is skipped, and only this file: the regex above and the doc comment
 * at the top both contain the literal `settingKey="…"`, so the scanner would
 * otherwise report its own source as two broken markers. The capture stays
 * permissive (`[^"]+` rather than a key-shaped pattern) on purpose — a marker
 * with a capitalised or space-bearing typo is exactly what this exists to
 * catch, and a tighter pattern would quietly skip it instead.
 */
const marked = [
  ...new Set(
    sources(SRC)
      .filter((path) => path !== import.meta.filename)
      .flatMap((path) =>
        [...readFileSync(path, "utf8").matchAll(/settingKey="([^"]+)"/g)].map(([, key]) => key),
      ),
  ),
].sort();

describe("the settings keys the UI marks as file-controlled", () => {
  it("are read against a key list that was actually found", () => {
    // The guard on the guard: a failed slice or a changed table shape yields an
    // empty set, and an empty set makes every assertion below vacuous in the
    // one direction that matters.
    expect(settable.size).toBeGreaterThan(15);
    expect(settable.has("notify.previews_enabled")).toBe(true);
    // ...and the slice stopped before the never-settable table.
    expect(settable.has("sdk_encryption")).toBe(false);
  });

  it("are found at all, so this file is not asserting about an empty list", () => {
    expect(marked.length).toBeGreaterThan(4);
  });

  it("each name a key a settings file may actually set", () => {
    // Named rather than counted: a count says something is wrong, a list says
    // which control would have gone on lying.
    expect(marked.filter((key) => !settable.has(key))).toEqual([]);
  });
});
