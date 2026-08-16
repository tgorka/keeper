/**
 * Which of a session's spaces are folded, and what an untouched one does
 * (Story 49.3, FR-275, FR-276).
 *
 * **Two decisions, and they are not the same decision.** Folding one space is
 * chrome the person arranged, so it lives where every other piece of arranged
 * chrome lives — a cookie, TS-only, never IPC (`fold-cookie.ts:29-33`). What a
 * space they have *never touched* does on arrival is a preference a `keeper.toml`
 * may set, so it lives in the settings table and arrives here as
 * {@link SessionSpacesFoldState.defaultFolded}. The composition is the story:
 *
 * > a space with no recorded fold follows the setting; a space the person folded
 * > or unfolded by hand keeps their answer.
 *
 * That is why the recorded state is a `boolean` per space rather than a set of
 * folded ids. A set can say "folded" and "not folded", and this needs a third
 * answer — "nothing recorded, ask the setting" — or flipping the setting would
 * either be ignored by every space or overwrite every hand-made choice.
 *
 * **A fourth cookie namespace, not a key in an existing one.** `fold-cookie.ts`
 * names this exact collision: the chat sidebar has a group called `spaces`, the
 * notes rail's first section is called Spaces, and a session's spaces are a
 * third thing with the same name. Widening {@link "@/lib/stores/notes-rail-fold"}'s
 * closed key set would make folding one surface silently fold another, and no
 * test that renders one surface can see it.
 *
 * **An OPEN key set, unlike the other three folds** — which is what makes this
 * module more than two lines. The rail's sections are a closed union written by
 * this build; a session's spaces are markdown files a person creates, so the
 * key space is unbounded and a cookie can grow past what a browser will store.
 * An oversized cookie is dropped **whole and silently**, so the bounds are
 * {@link "@/lib/stores/files-tree"}'s, for its reasons: an entry limit
 * ({@link SESSION_SPACES_FOLD_LIMIT}), a byte budget
 * ({@link SESSION_SPACES_FOLD_BUDGET}) as the backstop for pathologically long
 * ids, and eviction rather than refusal — losing the fold of a space nobody has
 * touched in months beats losing every fold at once.
 *
 * **Eviction drops the OLDEST record first**, where "oldest" is least recently
 * written: {@link setSpaceFolded} moves the key it writes to the end. Files-tree
 * drops its deepest key because a deep node restores into nothing without its
 * ancestors; there is no such structure here, and every record is equally
 * restorable, so recency is the only honest ranking of which one a person will
 * miss. Nothing is dropped silently: a partial write says how much it kept.
 *
 * The reader and the builder are pure and take the cookie string, so the round
 * trip is assertable without a document. That is deliberately NOT a test of the
 * restore: the restore is {@link hydrateSessionSpacesFold} at the detail's mount
 * point, and it is exercised there — a store-level test can never see that the
 * mount point does not call it (DW-172, and 48.1's mutation M3 measured exactly
 * that).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { writeCookie } from "@/components/ui/cookie-writer";

/** The cookie a session's spaces fold lives in. Not the rail's, not the sidebar's. */
export const SESSION_SPACES_FOLD_COOKIE = "keeper_session_spaces_fold";

/** A year, matching every other remembered fold. A section that unfolds itself
 *  every Monday is a section nobody bothers to fold. */
export const SESSION_SPACES_FOLD_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * How many spaces are remembered by hand.
 *
 * Thirty-two, the files tree's number, though the cost bounded is different:
 * nothing is re-read at startup for a fold, so what binds here is the cookie
 * jar. A zone ships five default spaces and a person who writes thirty more has
 * a rail they scroll — past that, the folds being forgotten are for spaces they
 * touched once and have not opened since.
 *
 * Only the PERSISTED set is bounded. A session may hold more folded than this
 * in memory, so exceeding it never unfolds a space under the person using it.
 */
export const SESSION_SPACES_FOLD_LIMIT = 32;

/**
 * How many encoded bytes the record may occupy.
 *
 * Below the ~4096 a browser silently discards at, and well past what
 * {@link SESSION_SPACES_FOLD_LIMIT} ordinary `_spaces/name.md` ids need — so
 * the count is the bound that normally binds and this engages only for ids long
 * enough that thirty-two of them do not fit. The same 2000 the files tree took,
 * and for the same reason it took a small one: when the jar is under pressure,
 * which sections were shut should give way before what someone was working on.
 */
export const SESSION_SPACES_FOLD_BUDGET = 2000;

/**
 * The key one space is remembered under: the root it belongs to, and its id.
 *
 * `\u0000` rather than `:` or `/` because a space id is a zone-relative *path*
 * (`_spaces/tasks.md`) and both are legal in one — a separator that can occur
 * inside either half would let two different pairs produce one key, and a fold
 * that confused two spaces would shut a section the person never touched.
 *
 * Scoped by root because the definitions belong to the zone: the same space id
 * in two synced folders is two different saved queries, and they fold apart.
 */
export function spaceFoldKey(rootId: string, spaceId: string): string {
  return `${rootId}\u0000${spaceId}`;
}

/** The persisted form: per root, the spaces with a recorded fold, in the order
 *  they were last written — oldest first, which is the order eviction eats.
 *
 *  Grouped by root because the root is a ULID that would otherwise repeat once
 *  per space, and versioned for the reason the panel list is: the vocabulary
 *  will change, and an unrecognised version is discarded rather than guessed at.
 *
 *  Grouping costs the interleaving of two roots' recency, and that is accepted:
 *  it changes only which record is evicted first at the thirty-third, never
 *  which fold a space is restored to. */
interface PersistedFold {
  readonly v: 1;
  readonly r: Readonly<Record<string, readonly unknown[]>>;
}

/** Structural guard over whatever the cookie actually held. */
function isPersisted(value: unknown): value is PersistedFold {
  return (
    typeof value === "object" &&
    value !== null &&
    "v" in value &&
    value.v === 1 &&
    "r" in value &&
    typeof value.r === "object" &&
    value.r !== null &&
    !Array.isArray(value.r)
  );
}

/**
 * The folds remembered in a `document.cookie` string.
 *
 * Pure and total, and tolerant in one direction only: a malformed cookie, an
 * unknown version, an entry that is not a `[spaceId, 0 | 1]` pair is dropped,
 * and what is left is a record every remaining space falls out of — which means
 * those spaces follow the setting, the state every space is reachable from.
 * This runs before anything is on screen, so a throw here is a white window.
 *
 * Bounded on the way in as well as on the way out: the string is editable, and
 * an editor who pastes five thousand entries into it gets thirty-two.
 */
export function readSessionSpacesFold(cookie: string): Map<string, boolean> {
  const recorded = new Map<string, boolean>();
  for (const part of cookie.split(";")) {
    const trimmed = part.trim();
    if (!trimmed.startsWith(`${SESSION_SPACES_FOLD_COOKIE}=`)) {
      continue;
    }
    let decoded: unknown;
    try {
      decoded = JSON.parse(
        decodeURIComponent(trimmed.slice(SESSION_SPACES_FOLD_COOKIE.length + 1)),
      );
    } catch {
      return recorded;
    }
    if (!isPersisted(decoded)) {
      return recorded;
    }
    for (const [rootId, entries] of Object.entries(decoded.r)) {
      if (rootId === "" || !Array.isArray(entries)) {
        continue;
      }
      for (const entry of entries) {
        if (!Array.isArray(entry) || entry.length !== 2) {
          continue;
        }
        const [spaceId, flag] = entry as readonly unknown[];
        if (typeof spaceId !== "string" || spaceId === "" || (flag !== 0 && flag !== 1)) {
          continue;
        }
        recorded.set(spaceFoldKey(rootId, spaceId), flag === 1);
      }
    }
    // Bounded on the way in too: the most recent records, oldest dropped.
    const all = [...recorded];
    return new Map(all.slice(Math.max(0, all.length - SESSION_SPACES_FOLD_LIMIT)));
  }
  return recorded;
}

/**
 * The `document.cookie` assignment that records these folds.
 *
 * Takes the map rather than the store so it is assertable without one, the
 * shape `column-widths.ts` established. An empty record is *forgotten* rather
 * than written as an empty object: a person who unfolded everything back to the
 * setting comes back to a clean start instead of to a cookie that decodes to
 * nothing, and the jar gets its bytes back.
 */
export function sessionSpacesFoldCookie(recorded: ReadonlyMap<string, boolean>): string {
  const all = [...recorded];
  let kept = all.slice(Math.max(0, all.length - SESSION_SPACES_FOLD_LIMIT));
  let value = encode(kept);
  // Drop the least recently written first: the folds a person is still using
  // are the ones they would notice going missing.
  while (kept.length > 0 && value.length > SESSION_SPACES_FOLD_BUDGET) {
    kept = kept.slice(1);
    value = encode(kept);
  }
  // Only when the BUDGET evicted. Hitting the entry limit is the ordinary,
  // documented path and `persist` runs on every single toggle, so reporting it
  // would print a line per press for the rest of the session in any zone with
  // more than 32 recorded folds — and would blame the byte budget for a drop
  // the count caused. Comparing against the limit reports only the case the
  // operator cannot predict from the number of spaces they have folded.
  if (kept.length < Math.min(all.length, SESSION_SPACES_FOLD_LIMIT)) {
    console.info(
      `keeper: remembering the fold of ${kept.length} of ${all.length} spaces — the rest do not fit in a cookie.`,
    );
  }
  if (kept.length === 0) {
    return `${SESSION_SPACES_FOLD_COOKIE}=; path=/; max-age=0; samesite=lax`;
  }
  return `${SESSION_SPACES_FOLD_COOKIE}=${value}; path=/; max-age=${SESSION_SPACES_FOLD_MAX_AGE}; samesite=lax`;
}

/** The encoded cookie value for exactly these records, in this order. */
function encode(entries: readonly (readonly [string, boolean])[]): string {
  const grouped: Record<string, [string, 0 | 1][]> = {};
  for (const [key, folded] of entries) {
    // The two halves of a {@link spaceFoldKey}. A key with no separator did not
    // come from that function; it groups under `""`, which the reader drops —
    // self-healing rather than an exception on a cookie write.
    const separator = key.indexOf("\u0000");
    const rootId = separator === -1 ? "" : key.slice(0, separator);
    const pairs = grouped[rootId] ?? [];
    pairs.push([separator === -1 ? key : key.slice(separator + 1), folded ? 1 : 0]);
    grouped[rootId] = pairs;
  }
  return encodeURIComponent(JSON.stringify({ v: 1, r: grouped }));
}

export interface SessionSpacesFoldState {
  /**
   * What each space was last folded or unfolded TO, by hand.
   *
   * Absent means untouched, which is not the same as unfolded — see the module
   * doc. Insertion order is write order, oldest first, and eviction depends on
   * it.
   */
  recorded: ReadonlyMap<string, boolean>;
  /** What a space with nothing recorded does: `sessions.spaces_folded`. */
  defaultFolded: boolean;
}

/**
 * Whether this space's rows are folded away right now.
 *
 * The composition rule, in one line and in one place: the recorded answer when
 * there is one, the setting otherwise.
 */
export function isSpaceFolded(state: SessionSpacesFoldState, key: string): boolean {
  return state.recorded.get(key) ?? state.defaultFolded;
}

export const sessionSpacesFoldStore = createStore<SessionSpacesFoldState>()(() => ({
  recorded: new Map<string, boolean>(),
  defaultFolded: false,
}));

/** Write the record out. Best effort: a document that refuses cookies costs the
 *  user the restore, and must not cost them the fold. */
function persist(recorded: ReadonlyMap<string, boolean>): void {
  if (typeof document === "undefined") {
    return;
  }
  try {
    writeCookie(sessionSpacesFoldCookie(recorded));
  } catch {
    // Nothing to say and nothing to retry: the section is folded on screen either way.
  }
}

/**
 * Record that this space is folded, or that it is not.
 *
 * Both directions are recorded, deliberately: with the setting on, "I opened
 * this one" is exactly the answer that must survive, and a store that only
 * remembered folds would reopen the setting's answer over it at every mount.
 *
 * The key is re-inserted rather than updated in place, so writing a fold makes
 * it the most recent record and the last one eviction reaches.
 */
export function setSpaceFolded(key: string, folded: boolean): void {
  const current = sessionSpacesFoldStore.getState().recorded;
  const recorded = new Map(current);
  recorded.delete(key);
  recorded.set(key, folded);
  persist(recorded);
  sessionSpacesFoldStore.setState({ recorded });
}

/**
 * What a space with nothing recorded does from now on.
 *
 * It touches `defaultFolded` and nothing else: the whole point of the split is
 * that changing the preference moves the spaces nobody has decided about, and
 * leaves the ones they have exactly where they put them. Not persisted here —
 * the setting is Rust's, written through `sessions_spaces_folded_set`.
 */
export function setSpacesFoldedDefault(folded: boolean): void {
  if (sessionSpacesFoldStore.getState().defaultFolded === folded) {
    return;
  }
  sessionSpacesFoldStore.setState({ defaultFolded: folded });
}

/** Whether {@link hydrateSessionSpacesFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered folds, and seed the setting they fall back to.
 *
 * Idempotent, so React's double-invoked development effects restore once, and
 * so a second detail mount cannot overwrite a fold the person has changed since
 * the first — including overwriting it with a `defaultFolded` read that was in
 * flight while they pressed something.
 *
 * Called from `session-detail.tsx`, the surface these sections render on, and
 * NOT from the store's own tests: a `hydrate…` that no mount point calls is a
 * restore that silently does not happen, and only a test at the mount point can
 * see that (DW-172).
 */
export function hydrateSessionSpacesFold(cookie: string, defaultFolded: boolean): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  sessionSpacesFoldStore.setState({ recorded: readSessionSpacesFold(cookie), defaultFolded });
}

/** React selector hook over {@link sessionSpacesFoldStore}. */
export function useSessionSpacesFold<T>(selector: (state: SessionSpacesFoldState) => T): T {
  return useStore(sessionSpacesFoldStore, selector);
}

/** Test-only reset: nothing recorded, the setting off, unhydrated, no cookie written. */
export function resetSessionSpacesFoldForTest(): void {
  hydrated = false;
  sessionSpacesFoldStore.setState({ recorded: new Map(), defaultFolded: false });
}
