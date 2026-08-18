/**
 * A closed set of boolean flags remembered in one cookie (Story 45.20, Story 47.3).
 *
 * **Why this is its own module.** Story 45.20 gave the chat sidebar a fold and
 * put the parser beside it. Story 47.3 gave the notes rail folds of its own, and
 * a second surface meant either a second copy of "find the name in a jar full of
 * other people's cookies, split on `|`, drop what this build does not know" or
 * one shared copy. Two copies of an ENCODING agree until the day one of them is
 * edited, and the symptom — a build that writes a cookie an older build silently
 * drops — is invisible to a test that only ever exercises one surface. So the
 * encoding is shared and the surfaces are not.
 *
 * **Two cookies, not one namespace.** Each surface names its own cookie and its
 * own closed key set, and nothing here lets one read the other's. The collision
 * that rule prevents is not hypothetical: the chat sidebar already has a group
 * keyed `spaces`, and the first section of the notes rail is also called Spaces.
 * They are different surfaces showing different things, and a shared namespace
 * would make folding one silently fold the other.
 *
 * **A closed key set, not an open map.** An id is written into somebody's cookie
 * and read back by a later build, so a typo must be droppable and a section that
 * no longer exists must not leave a key nothing can clear.
 *
 * Everything here is pure and takes the cookie string, so the round trip is
 * assertable without a document. That is deliberately NOT a test of the restore:
 * a restore is a `hydrate…` call at a mount point, and a store-level test can
 * never see that the mount point does not call it (DW-172).
 *
 * Not `localStorage`, which this codebase refuses out loud
 * (`iosSyncDisclosureShownGet` says so), and not an IPC command, because which
 * sections a person has folded is a lens they chose and not a fact Rust has any
 * use for.
 */

/** A year, matching the panel list and the column widths. A menu that unfolds
 *  itself every Monday is a menu nobody bothers to fold. */
export const FOLD_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * The flags `name` remembers in `cookie`, defaulted per key.
 *
 * Total, and tolerant in one direction only: a malformed entry, an unknown key,
 * a value that is not `0`/`1`, and a value that is not valid percent-encoding
 * at all is dropped and leaves that flag at its default. Refusing to render a
 * menu because a jar holds a stale entry would be a worse outcome than a menu
 * that starts at its default.
 *
 * `defaults` rather than "everything starts open", because one section's honest
 * default is folded: the notes rail's Files tree loads a directory per expansion
 * and has been collapsed on arrival since Story 37.9, so defaulting it open
 * would turn every mount into a cold scan nobody asked for.
 */
export function readFoldFlags<K extends string>(
  cookie: string,
  name: string,
  keys: readonly K[],
  defaults: Readonly<Record<K, boolean>>,
): Record<K, boolean> {
  const flags: Record<K, boolean> = { ...defaults };
  for (const pair of cookie.split(";")) {
    const separator = pair.indexOf("=");
    if (separator === -1 || pair.slice(0, separator).trim() !== name) {
      continue;
    }
    // `decodeURIComponent` THROWS a `URIError` on a lone `%` or a truncated
    // escape, and a jar is other people's bytes: an extension, a proxy that
    // rewrote a value, a build interrupted mid-write. Every reader of this runs
    // inside a mount effect — two in `AppShell`, one in `NotesPane`, one in
    // `TextFileFrame` — so an unguarded throw does not degrade a fold, it takes
    // the surface down.
    let decoded: string;
    try {
      decoded = decodeURIComponent(pair.slice(separator + 1).trim());
    } catch {
      // Not valid percent-encoding, and the value this module writes has its
      // `|` and its `:` encoded — so these bytes carry no entry this parser
      // could read. Every flag stays at its default, which is what the rest of
      // this loop does with junk it can read.
      continue;
    }
    for (const entry of decoded.split("|")) {
      const colon = entry.indexOf(":");
      if (colon === -1) {
        continue;
      }
      const key = entry.slice(0, colon);
      const value = entry.slice(colon + 1);
      if (value !== "0" && value !== "1") {
        continue;
      }
      if ((keys as readonly string[]).includes(key)) {
        flags[key as K] = value === "1";
      }
    }
  }
  return flags;
}

/**
 * The `document.cookie` assignment that records these flags under `name`.
 *
 * Writes every key rather than only the folded ones, because a cookie write
 * replaces the name's whole value: omitting an unfolded section would make
 * "unfolded" indistinguishable from "written by an older build", and the two
 * differ the moment a section's default stops being open — which, for Files,
 * it already has.
 */
export function foldFlagsCookie<K extends string>(
  name: string,
  keys: readonly K[],
  flags: Readonly<Record<K, boolean>>,
): string {
  const value = keys.map((key) => `${key}:${flags[key] ? 1 : 0}`).join("|");
  return `${name}=${encodeURIComponent(value)}; path=/; max-age=${FOLD_MAX_AGE}`;
}

/** Write an assignment out. Best effort: a document that refuses cookies costs
 *  the user the restore, and must not cost them the click. */
export function persistFold(assignment: string): void {
  if (typeof document === "undefined") {
    return;
  }
  try {
    // biome-ignore lint/suspicious/noDocumentCookie: chrome state read before React mounts, following `panels.ts` and `column-widths.ts`; CookieStore is async and unavailable there
    document.cookie = assignment;
  } catch {
    // A jar that will not take a write is not a reason to refuse the fold.
  }
}
