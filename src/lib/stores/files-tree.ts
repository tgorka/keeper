/**
 * Which folders the Files tree has open, and where that survives to (Story 46.3).
 *
 * **The defect this closes.** `FilesPane` held its expansion in `useState`, and
 * `AppShell` mounts the pane conditionally on the primary view — so looking at
 * Notes for two seconds unmounted the tree and threw the set away. The pane's
 * own doc comment promised the opposite ("Collapsing keeps what was loaded, so
 * re-opening a branch is free"), which was true within one mount and false the
 * moment you looked at anything else. Component state was never the right home
 * for it: the lifetime of a lens the user chose is not the lifetime of the
 * component that happens to render it.
 *
 * **The shape is {@link "@/lib/stores/sidebar-fold"}'s**, for the reason that
 * module's doc gives: one cookie per concern, a `keeper_` name, a year, a pure
 * reader, a pure builder, a best-effort write, and an idempotent hydrate called
 * once from `AppShell` rather than from the component — a restore mounted inside
 * a component that is unmounted for whole sessions is a restore that silently
 * does not happen, and no hook-level test can see it fail to run (DW-172). Not
 * `localStorage`, which this codebase refuses out loud (`column-widths.ts`), and
 * not an IPC command, because which folders someone has open is a lens and not a
 * fact Rust has any use for.
 *
 * **The encoding is {@link "@/lib/stores/panels"}'s, not the fold's.** The fold
 * packs its state into `key:value|key:value`, which works because its keys are a
 * closed set of identifiers. A node key is a profile id and a *file path*, and
 * `|` and `:` are both legal in a path — so this is versioned JSON behind
 * `encodeURIComponent`, with an unknown `v` discarded rather than guessed at.
 *
 * **Expansion only. Not the listings.** `FilesPane` caches what each directory
 * held; that cache stays component state and dies with the mount, deliberately.
 * A restored listing is a claim about a disk keeper has not looked at since the
 * app was last closed — it would render a file that has been deleted, from a
 * volume that may not even be attached, with no way for the viewer to tell. What
 * a person expanded is a fact about the person; what a folder contained is a
 * fact about the disk, and only one of those is still true tomorrow.
 *
 * **Restoring costs one `sync_browse` per open folder, at mount, and that is
 * accepted rather than worked around** — a tree that comes back open but empty
 * would be a worse lie than one that comes back shut. It is bounded twice, and
 * the bounds are the interesting part of this module:
 *
 * 1. {@link FILES_TREE_LIMIT} keys, shallowest first. Not a byte count, because
 *    the cost being bounded is IPC calls at startup, and bytes are a poor proxy
 *    for those.
 * 2. {@link FILES_TREE_COOKIE_BUDGET} encoded bytes, as a backstop for paths
 *    long enough that 32 of them do not fit. Browsers drop an oversized cookie
 *    *silently* (see `panels.ts`), so the overflow is handled here, where it can
 *    be reported, rather than discovered as a total loss at the next launch.
 *
 * **Both bounds drop the deepest keys first**, and this is the rule to keep if
 * anything else here changes. A deep node is only visible when its ancestors are
 * open, so dropping a shallow key while keeping its descendant would restore an
 * expansion that renders nothing and still pays for a browse. Dropping from the
 * bottom keeps the part of the tree nearest what a person actually sees. Ties at
 * equal depth break lexicographically, so the write is deterministic — a cookie
 * that reordered itself on every toggle would defeat any attempt to diff it.
 * Nothing is truncated silently: a partial write says how much it kept.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { writeCookie } from "@/components/ui/cookie-writer";

/** The cookie the whole expansion lives in. One cookie, not one per profile. */
export const FILES_TREE_COOKIE = "keeper_files_tree";

/** A year, matching the panel list, the fold and the column widths. A tree that
 *  shuts itself every Monday is a tree nobody bothers to open. */
export const FILES_TREE_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * How many open folders are remembered.
 *
 * The binding cost is not storage, it is that every restored key issues a
 * `sync_browse` when the pane mounts — against folders that may be on a pendrive
 * or a network mount. Thirty-two is well past what anyone reaches by clicking
 * chevrons and still a startup this surface can pay for in one breath.
 *
 * A session may hold more than this open; only the persisted set is bounded, so
 * exceeding it never collapses a folder under the person using it.
 */
export const FILES_TREE_LIMIT = 32;

/**
 * How many encoded bytes the expansion may occupy.
 *
 * Below the ~4096 a browser silently discards at, and well below what
 * {@link FILES_TREE_LIMIT} keys of ordinary length need, so the count is the
 * bound that normally binds and this only engages for pathologically long paths.
 * Deliberately smaller than `PANELS_COOKIE_BUDGET`: the panel arrangement is
 * what you were working on and this is which folders were open, so when the jar
 * is under pressure this is the one that should give way first.
 */
export const FILES_TREE_COOKIE_BUDGET = 2000;

/**
 * The key one directory is remembered under: a profile and a subpath.
 *
 * `\u0000` rather than `/` or `:` because both are legal in the strings being
 * joined — a folder called `a/b` cannot exist, but a profile id and a subpath
 * concatenated with `:` can collide with a different pair, and a cache that
 * confuses two directories would show one folder's contents under another
 * folder's name.
 *
 * Here rather than in `FilesPane`, where it used to live, because the format is
 * now written into a cookie and read back by a later build: the vocabulary and
 * the thing that persists it belong in one file, and the separator has exactly
 * one definition.
 */
export function nodeKey(profileId: string, subpath: string): string {
  return `${profileId}\u0000${subpath}`;
}

/** The profile a node key names. `""` for a key with no separator, which
 *  {@link readFilesTree} never produces and no profile ever matches. */
export function nodeKeyProfile(key: string): string {
  const separator = key.indexOf("\u0000");
  return separator === -1 ? "" : key.slice(0, separator);
}

/** The profile-relative subpath a node key names; `""` is the profile root. */
export function nodeKeySubpath(key: string): string {
  const separator = key.indexOf("\u0000");
  return separator === -1 ? "" : key.slice(separator + 1);
}

/** How deep a node key sits. A profile root is 0. */
function nodeDepth(key: string): number {
  const subpath = nodeKeySubpath(key);
  return subpath === "" ? 0 : subpath.split("/").length;
}

/**
 * Whether a subpath out of a cookie is one this app is willing to browse.
 *
 * A cookie is a string the user can edit, so this is the boundary where a
 * remembered path stops being trusted, exactly as `isRestorableTarget` is for
 * the panel list. AD-65 says no frontend joins a root and a subpath; the
 * corollary is that a path arriving from outside the app has to be a relative
 * one before it is handed to a call that will join it. `keeper_sync::browse`
 * refuses `..` lexically and refuses a symlink out of the tree after
 * canonicalisation, so this is the outer of two gates rather than the only one.
 */
function isRestorableSubpath(subpath: string): boolean {
  return (
    !subpath.startsWith("/") &&
    !subpath.startsWith("\\") &&
    !/^[a-zA-Z]:[\\/]/.test(subpath) &&
    !subpath.includes("\u0000") &&
    !subpath.split("/").includes("..")
  );
}

/**
 * The keys that will be bounded, in the order the bounds eat them: shallowest
 * first, then lexicographic. Shared by the reader and the writer so a cookie
 * cannot hold a set that the next write would order differently.
 */
function ordered(expanded: Iterable<string>): string[] {
  return [...expanded].sort((a, b) => nodeDepth(a) - nodeDepth(b) || (a < b ? -1 : a > b ? 1 : 0));
}

/**
 * The open folders that will actually be visible: those every one of whose
 * ancestors is also open.
 *
 * Collapsing a folder does not collapse what was open inside it — that is
 * deliberate, and it is why re-opening a branch comes back the way you left it.
 * The consequence is that the set can hold a node nothing can render, and
 * browsing for one at mount would be an IPC call for a row with nowhere to go.
 * So the restore loads this subset, and the rest waits in the set for the day
 * its parent opens again.
 */
export function reachableNodeKeys(expanded: ReadonlySet<string>): string[] {
  return [...expanded].filter((key) => {
    const profileId = nodeKeyProfile(key);
    const subpath = nodeKeySubpath(key);
    if (subpath === "") {
      return true;
    }
    const segments = subpath.split("/");
    // Every proper ancestor, up to and including the profile root.
    for (let i = segments.length - 1; i >= 0; i -= 1) {
      if (!expanded.has(nodeKey(profileId, segments.slice(0, i).join("/")))) {
        return false;
      }
    }
    return true;
  });
}

/** The persisted form: open subpaths, grouped under the profile they belong to.
 *
 * Grouped rather than a flat list of joined keys because a profile id is a ULID
 * repeated once per open folder underneath it, and because the shape then says
 * out loud what the stale-profile drop acts on.
 *
 * Versioned for the reason the panel list is: the vocabulary will change, and an
 * unrecognised version is discarded rather than guessed at — which costs the
 * reader their tree once and never opens a folder that meant something else. */
interface PersistedTree {
  readonly v: 1;
  readonly p: Readonly<Record<string, readonly string[]>>;
}

/** Structural guard over whatever the cookie actually held. */
function isPersisted(value: unknown): value is PersistedTree {
  return (
    typeof value === "object" &&
    value !== null &&
    "v" in value &&
    value.v === 1 &&
    "p" in value &&
    typeof value.p === "object" &&
    value.p !== null &&
    !Array.isArray(value.p)
  );
}

/**
 * The expansion remembered in a `document.cookie` string.
 *
 * Pure and total, and tolerant in one direction only: a malformed cookie, an
 * unknown version, an entry that is not a string or a path that is not relative
 * is dropped, and what is left is a tree with those folders shut. This runs
 * before anything is on screen, so a throw here is a white window, and "shut" is
 * the state every folder is reachable from.
 *
 * Bounded on the way in as well as on the way out: the string is editable, and
 * an editor who pastes five thousand keys into it should not get five thousand
 * browse calls at the next launch.
 */
export function readFilesTree(cookie: string): Set<string> {
  const empty = new Set<string>();
  for (const part of cookie.split(";")) {
    const trimmed = part.trim();
    if (!trimmed.startsWith(`${FILES_TREE_COOKIE}=`)) {
      continue;
    }
    let decoded: unknown;
    try {
      decoded = JSON.parse(decodeURIComponent(trimmed.slice(FILES_TREE_COOKIE.length + 1)));
    } catch {
      return empty;
    }
    if (!isPersisted(decoded)) {
      return empty;
    }
    const keys: string[] = [];
    for (const [profileId, subpaths] of Object.entries(decoded.p)) {
      if (profileId === "" || !Array.isArray(subpaths)) {
        continue;
      }
      for (const subpath of subpaths) {
        if (typeof subpath === "string" && isRestorableSubpath(subpath)) {
          keys.push(nodeKey(profileId, subpath));
        }
      }
    }
    return new Set(ordered(keys).slice(0, FILES_TREE_LIMIT));
  }
  return empty;
}

/**
 * The `document.cookie` assignment that records this expansion.
 *
 * Takes the set rather than the store so it is assertable without one, the shape
 * {@link "@/lib/column-widths"} established. Nothing open is *forgotten* rather
 * than written as an empty tree: a person who shut everything comes back to a
 * clean start instead of to a cookie that decodes to nothing, and the jar gets
 * its bytes back.
 */
export function filesTreeCookie(expanded: ReadonlySet<string>): string {
  const all = ordered(expanded);
  let kept = all.slice(0, FILES_TREE_LIMIT);
  let value = encode(kept);
  // Drop the deepest first: a node whose ancestors did not fit is a node that
  // restores into nothing, so the bottom of the tree is what can be spared.
  while (kept.length > 0 && value.length > FILES_TREE_COOKIE_BUDGET) {
    kept = kept.slice(0, -1);
    value = encode(kept);
  }
  if (kept.length < all.length) {
    console.info(
      `keeper: remembering ${kept.length} of ${all.length} open folders — the rest do not fit in a cookie.`,
    );
  }
  if (kept.length === 0) {
    return `${FILES_TREE_COOKIE}=; path=/; max-age=0; samesite=lax`;
  }
  return `${FILES_TREE_COOKIE}=${value}; path=/; max-age=${FILES_TREE_MAX_AGE}; samesite=lax`;
}

/** The encoded cookie value for exactly these keys. */
function encode(keys: readonly string[]): string {
  const grouped: Record<string, string[]> = {};
  for (const key of keys) {
    const profileId = nodeKeyProfile(key);
    const subpaths = grouped[profileId] ?? [];
    subpaths.push(nodeKeySubpath(key));
    grouped[profileId] = subpaths;
  }
  return encodeURIComponent(JSON.stringify({ v: 1, p: grouped }));
}

export interface FilesTreeState {
  /** Node keys of every folder currently open. */
  expanded: ReadonlySet<string>;
  /** Open or shut one folder. Idempotent, so a caller that already knows the
   *  state it wants does not have to check first. */
  setNodeOpen: (key: string, open: boolean) => void;
  /**
   * Forget every open folder belonging to a profile that is not in this list.
   *
   * The stale-key drop, and it happens here rather than in {@link readFilesTree}
   * because the reader is pure and the set of profiles that exist is a fact only
   * `sync_profiles` knows. Silent: a folder someone stopped syncing months ago
   * is not news, and a sentence about it would be a sentence about nothing they
   * can act on. Without it the cookie would accumulate keys nothing can ever
   * clear, and the restore would browse profiles that are gone.
   */
  retainProfiles: (profileIds: readonly string[]) => void;
}

/** Write the expansion out. Best effort: a document that refuses cookies costs
 *  the user the restore, and must not cost them the click. */
function persist(expanded: ReadonlySet<string>): void {
  if (typeof document === "undefined") {
    return;
  }
  try {
    writeCookie(filesTreeCookie(expanded));
  } catch {
    // Nothing to say and nothing to retry: the folder is open on screen either way.
  }
}

export const filesTreeStore = createStore<FilesTreeState>()((set, get) => ({
  expanded: new Set<string>(),
  setNodeOpen: (key, open) => {
    const current = get().expanded;
    if (current.has(key) === open) {
      return;
    }
    const expanded = new Set(current);
    if (open) {
      expanded.add(key);
    } else {
      expanded.delete(key);
    }
    persist(expanded);
    set({ expanded });
  },
  retainProfiles: (profileIds) => {
    const live = new Set(profileIds);
    const current = get().expanded;
    const expanded = new Set([...current].filter((key) => live.has(nodeKeyProfile(key))));
    if (expanded.size === current.size) {
      return;
    }
    persist(expanded);
    set({ expanded });
  },
}));

/** Whether {@link hydrateFilesTree} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered expansion.
 *
 * Idempotent, so React's double-invoked development effects restore once, and so
 * a second caller cannot overwrite folders the user has opened since the first.
 * Mounted at the shell rather than inside `FilesPane`: the pane is unmounted
 * whenever another primary view is up, which is the whole defect this module
 * exists for, and a restore living inside it would be a restore that runs after
 * the state it was meant to precede.
 */
export function hydrateFilesTree(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  const expanded = readFilesTree(cookie);
  if (expanded.size === 0) {
    return;
  }
  filesTreeStore.setState({ expanded });
}

/** React selector hook over {@link filesTreeStore}. */
export function useFilesTree<T>(selector: (state: FilesTreeState) => T): T {
  return useStore(filesTreeStore, selector);
}

/** Test-only reset: nothing open, unhydrated, no cookie written. */
export function resetFilesTreeForTest(): void {
  hydrated = false;
  filesTreeStore.setState({ expanded: new Set() });
}
