/**
 * Note-list filter store (Epic 37, Stories 37.3–37.5, FR-103/FR-104, UX-DR37,
 * UX-DR41).
 *
 * The chip bar is simultaneously the control and the state, so this store is the
 * chip bar: a scope, a set of intersecting tags, a free-text query, and two
 * boolean chips. It holds *what the user asked for* and nothing about what came
 * back — the rows live in `notes-list.ts`, and the filtering itself is Rust's
 * (`notes_list` evaluates the composed {@link NoteQueryReq}). Forking the query
 * semantics into TypeScript is the thing AD-20 and AD-58 both rule out, so
 * nothing here ever inspects a row.
 *
 * Two rules this file exists to keep true:
 *
 *   - **Tag terms intersect.** Two chips mean "both", never "either" — the Apple
 *     Notes contract, and the one people already expect from a chip bar. Since
 *     Story 43.3 a chip is three-state (off, include, exclude) and an exclusion
 *     intersects the same way: `client/acme` AND not `draft`.
 *   - **A filter change is a filter.** Nothing here touches the selection or the
 *     open note (UX-DR41). The note under the cursor survives every chip, and
 *     the pane keeps it open even when the new filter would exclude its row.
 *
 * A tag appears in {@link NotesFiltersState.tagTerms} at most once, which is the
 * whole of how "include and exclude the same tag" is made impossible rather than
 * resolved by precedence (FR-148, UX-DR54): there is one entry per tag, the
 * sign action rewrites it in place, and {@link noteQueryFor} ships it as a map keyed by
 * tag so the wire cannot carry the contradiction either.
 *
 * `folder` scope is the one that does not go through {@link NoteQueryReq}: the
 * physical lens has its own command (`notes_tree`, FR-106) because a
 * vault-relative directory is not one of the query's axes. {@link isFolderScope}
 * is how the pane picks its source.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteQueryReq, NoteSpaceVm, NoteTagTerm } from "@/lib/ipc/client";
import {
  notesHideServiceFilesGet,
  notesHideServiceFilesSet,
  notesIncludePrivateGet,
  notesIncludePrivateSet,
} from "@/lib/ipc/client";
import { ALL_SPACE_ID } from "@/lib/notes/all-spaces";

/**
 * One space a scope holds (AD-306): enough to name it, search it in its own
 * drive, and speak about it — never the whole rail row.
 */
export interface NoteScopeSpace {
  readonly id: string;
  readonly name: string;
  readonly vaultId: string;
  /**
   * Which seeded default this space is, `null` for every other space.
   *
   * Carried on the scope so a surface can speak about *this* space without
   * re-reading the space list or, worse, matching on its name — a default
   * is renameable like any other, and a sentence that stopped appearing
   * because someone called Recordings "Sessions" would be a bug nobody
   * connects to the rename.
   */
  readonly defaultKey: string | null;
}

/**
 * What the list is scoped to — the sidebar row that is selected, or `all` when
 * none is. Every one of these is a filter and not a route (UX-DR41).
 *
 * **There are only two kinds of row, and one of them is a space** (Story 44.3,
 * AD-79). Inbox, Journal, Pinned and Recordings used to be four more variants
 * here, each with a hard-coded `is:` flag in a table below, and each therefore
 * unteachable: no icon, no rename, no reorder, no edit. They are seeded notes
 * under `spaces/` now, so their queries live in the vault where the user can
 * read and change them, and this type stopped needing to know their names.
 *
 * "Today" is not here and no longer anywhere: the row never filtered anything
 * (AD-80). Opening or creating today's journal entry is an action on one note,
 * and it still lives on `⌘⌥J`, the tray and the palette.
 */
export type NoteScope =
  | { readonly kind: "all" }
  | {
      readonly kind: "space";
      /**
       * The selected spaces, in the order they were selected (AD-306). Each
       * drive searched is narrowed by the union of the spaces that live on it
       * or, when none does, by the union of all of them. Never empty: a scope
       * with no spaces is `all`.
       */
      readonly spaces: readonly [NoteScopeSpace, ...NoteScopeSpace[]];
    }
  | { readonly kind: "folder"; readonly vaultId: string; readonly path: string };

/** The unscoped list — every note in the vault, in the vault's own order. */
export const ALL_NOTES_SCOPE = { kind: "all" } as const satisfies NoteScope;

/**
 * A space's identity across drives: one id can name rows on two drives
 * (Uncategorized always does), so the drive is part of the key.
 */
export function spaceKey(space: { readonly vaultId: string; readonly id: string }): string {
  return `${space.vaultId}:${space.id}`;
}

/** The spaces a scope holds, in scope order; none for `all` and a folder. */
export function scopeSpaces(scope: NoteScope): readonly NoteScopeSpace[] {
  return scope.kind === "space" ? scope.spaces : [];
}

/** Whether `space` (on its drive) is one of the scope's members. */
export function scopeHas(
  scope: NoteScope,
  space: { readonly vaultId: string; readonly id: string },
): boolean {
  const key = spaceKey(space);
  return scopeSpaces(scope).some((member) => spaceKey(member) === key);
}

/** The chip label for a scope, as the bar renders it. */
export function scopeLabel(scope: NoteScope): string {
  switch (scope.kind) {
    case "space":
      return scope.spaces.map((space) => space.name).join(" or ");
    case "folder":
      return scope.path === "" ? "All files" : scope.path;
    default:
      return "";
  }
}

/**
 * Whether this scope is served by the physical-tree command rather than by a
 * {@link NoteQueryReq}. A vault-relative directory is not one of the query's
 * axes, and `notes_tree` returns the folder's own rows (FR-106).
 */
export function isFolderScope(scope: NoteScope): scope is Extract<NoteScope, { kind: "folder" }> {
  return scope.kind === "folder";
}

/**
 * Whether two scopes name the same thing. Spaces compare by drive and id, in
 * order: the first member's ordering sorts a union, so a reordered selection
 * is a different list.
 */
export function sameScope(a: NoteScope, b: NoteScope): boolean {
  if (a.kind === "space" && b.kind === "space") {
    return (
      a.spaces.length === b.spaces.length &&
      a.spaces.every((space, index) => spaceKey(space) === spaceKey(b.spaces[index]))
    );
  }
  if (a.kind === "folder" && b.kind === "folder") {
    return a.vaultId === b.vaultId && a.path === b.path;
  }
  return a.kind === b.kind;
}

/** The four scope fields of a rail row — the scope never holds the whole row. */
function scopeSpaceOf(space: NoteScopeSpace): NoteScopeSpace {
  return { id: space.id, name: space.name, vaultId: space.vaultId, defaultKey: space.defaultKey };
}

/**
 * What one tag chip is doing, as the control shows it (FR-148, UX-DR54).
 *
 * `off` is the UI's word for "not in {@link NotesFiltersState.tagTerms}", and it
 * exists only on this side: a term that admits everything has no business on the
 * wire, so {@link NoteTagTerm} — the Rust vocabulary — has two values and this
 * has three.
 */
export type TagChipState = "off" | NoteTagTerm;

/** One active tag chip: the tag, and which way it is pointing. */
export interface TagChip {
  readonly tag: string;
  readonly term: NoteTagTerm;
}

/** A sign press can change polarity, never remove a term. */
export function flipTagTerm(state: TagChipState): NoteTagTerm {
  return state === "include" ? "exclude" : "include";
}

/**
 * What `tag` is currently doing in a chip list, `off` when it is doing nothing.
 *
 * The one reader of {@link NotesFiltersState.tagTerms}' shape. The tag tree, the
 * sign action and the space editor all ask this rather than each searching the array,
 * so a node in the tree and the same tag's chip in the bar cannot end up drawing
 * two different states.
 */
export function tagChipState(chips: readonly TagChip[], tag: string): TagChipState {
  return chips.find((chip) => chip.tag === tag)?.term ?? "off";
}

/**
 * The chip list with `tag` put into `term`, `off` removing it.
 *
 * The one mutation of the list, and the reason a tag cannot be included and
 * excluded at once: an existing entry is rewritten **in place**, keeping the
 * position the user first put it in, rather than appended beside itself. A
 * push-and-filter would work too, right up until it did not — and the bug it
 * would produce is a chip that jumps to the end of the bar every time you press
 * it, which is the target moving under the cursor mid-cycle.
 *
 * Exported for the space editor (Story 43.4), whose draft term list is not this
 * store's — editing a space must not re-filter the list behind the dialog — but
 * must behave identically under the same presses.
 */
export function withTagTerm(
  chips: readonly TagChip[],
  tag: string,
  term: TagChipState,
): readonly TagChip[] {
  if (term === "off") {
    return chips.filter((chip) => chip.tag !== tag);
  }
  if (chips.some((chip) => chip.tag === tag)) {
    return chips.map((chip) => (chip.tag === tag ? { tag, term } : chip));
  }
  return [...chips, { tag, term }];
}

export type NoteSortChoice = {
  key: "relevance" | "order" | "name" | "created" | "modified" | "recorded";
  dir: "asc" | "desc";
};

/** The producer supplies canonical sort strings, never arbitrary frontmatter. */
function restoredSort(value: string | null): NoteSortChoice | null {
  if (value === null) return null;
  const [key, dir] = value.split(" ");
  return { key: key as NoteSortChoice["key"], dir: dir as NoteSortChoice["dir"] };
}

/** Compare only the editable restore projection, never viewing preferences. */
export function spaceDrift(state: NotesFiltersState): boolean {
  const space = state.enteredSpace;
  if (!space) return false;
  const restore = space.restore;
  const terms = restore.opaque ? {} : restore.tagTerms;
  const flags = restore.opaque ? [] : restore.flags;
  const sort = restore.opaque ? null : restoredSort(restore.sort);
  return (
    state.scope.kind !== "space" ||
    state.scope.spaces.length !== 1 ||
    spaceKey(state.scope.spaces[0]) !== spaceKey(space) ||
    state.text !== (restore.opaque ? "" : (restore.text ?? "")) ||
    state.tagTerms.length !== Object.keys(terms).length ||
    state.tagTerms.some(({ tag, term }) => terms[tag] !== term) ||
    state.flags.length !== flags.length ||
    state.flags.some((flag) => !flags.includes(flag)) ||
    state.origin !== (restore.opaque ? null : restore.origin) ||
    state.sort?.key !== sort?.key ||
    state.sort?.dir !== sort?.dir
  );
}

/**
 * The bar with an entered space's restored terms taken back out, leaving only
 * what the person added on top (AD-306, AD-305). Those terms belong to the
 * space's own lens; left on the bar once the scope stops being that one space,
 * they would AND it onto whatever the scope became — X ∩ (X ∪ Y) is just X.
 */
function withoutRestored(state: NotesFiltersState, space: NoteSpaceVm): Partial<NotesFiltersState> {
  const restore = space.restore;
  if (restore.opaque) return {};
  const flags = state.flags.filter((flag) => !restore.flags.includes(flag));
  const origin = state.origin === restore.origin ? null : state.origin;
  return {
    tagTerms: state.tagTerms.filter(({ tag, term }) => restore.tagTerms[tag] !== term),
    text: state.text === (restore.text ?? "") ? "" : state.text,
    flags,
    pinnedOnly: flags.includes("pinned"),
    origin,
    agentOnly: origin === "agent",
  };
}

export interface NotesFiltersState {
  /** The selected sidebar scope; `all` when none is. */
  scope: NoteScope;
  /**
   * The active tag chips, in the order they were first pressed, at most one
   * entry per tag. They INTERSECT: a note matches only when it carries every
   * `include` and none of the `exclude`.
   */
  tagTerms: readonly TagChip[];
  /** The list's ranked full-text query. */
  text: string;
  /** The "Changed by agent" chip. */
  agentOnly: boolean;
  /** The "Pinned only" chip, independent of the Pinned scope row. */
  pinnedOnly: boolean;
  flags: readonly string[];
  origin: string | null;
  setFlags: (flags: readonly string[]) => void;
  setOrigin: (origin: string | null) => void;
  /** Global viewing preference, deliberately not a chip or space predicate. */
  hideServiceFiles: boolean;
  sort: NoteSortChoice | null;
  vaultIds: readonly string[];
  includePrivate: boolean;
  /**
   * The space whose saved search the bar was filled from, `null` when none
   * was. Non-null only while the scope is exactly that one space: only
   * {@link NotesFiltersState.enterSpace} sets it, and every other scope change
   * clears it, so drift and reset (79.2) speak about a single entered space.
   */
  readonly enteredSpace: NoteSpaceVm | null;
  /**
   * The rail's rows as last read (AD-305): what {@link NotesFiltersState.restoreScope}
   * checks a remembered space against, and where it finds that space's saved
   * search. `null` until the rail has been read for the active drive. Not a
   * filter — `clearAll` leaves it alone.
   */
  readonly railSpaces: readonly NoteSpaceVm[] | null;
  setRailSpaces: (rows: readonly NoteSpaceVm[] | null) => void;
  spacesNonce: number;
  setSort: (sort: NoteSortChoice | null) => void;
  setVaultIds: (ids: readonly string[]) => void;
  setIncludePrivate: (on: boolean) => void;
  enterSpace: (space: NoteSpaceVm) => void;
  mergeSpace: (space: NoteSpaceVm) => void;
  /**
   * Add `space` to the selection or take it out (AD-306): the ⌘-click, a
   * scope chip's ×, a deleted member. It never fills the bar from a space;
   * leaving a single entered space takes that space's restored terms back
   * out, so the person's own additions narrow the union and nothing else.
   * The last member out leaves the scope `all` with the bar as it was.
   */
  toggleSpace: (space: NoteScopeSpace) => void;
  /**
   * Follow history onto the scope a note was opened under (AD-305). Nothing
   * happens when the scope already is that one, or when a space it names is
   * no longer a healthy rail row. An untouched bar follows the space as a
   * rail click would; a search the person typed stays, minus the outgoing
   * space's own restored terms.
   */
  restoreScope: (scope: NoteScope) => void;
  requestSpacesReload: () => void;
  /**
   * A monotonic nonce bumped by the palette's Open Note… / Search Notes actions.
   * The search field's DOM node belongs to the pane that renders it, so rather
   * than lift a ref out of the tree, the pane subscribes to this and takes focus
   * on each bump — the same shape `chat-list-focus.ts` uses for the summon
   * hotkey.
   */
  searchNonce: number;
  /** Select a scope. Selecting the active one again clears it back to `all`. */
  setScope: (scope: Exclude<NoteScope, { kind: "space" }>) => void;
  /** Flip a tag's polarity without removing it; an absent tag becomes included. */
  toggleTagSign: (tag: string) => void;
  resetToEnteredSpace: () => void;
  /**
   * The trailing control's whole act: reset the search, not merely the text.
   *
   * Inside an entered space this restores that space's saved query — the
   * previous state a person expects back — and outside one it empties the
   * prompt and the tag chips together. Clearing the text while leaving three
   * tag chips filtering the list is the state the owner reported as "it did
   * not reset"; the two facts are one control's business.
   */
  resetSearch: () => void;
  /**
   * Put one tag chip in a named state, `off` removing it. The explicit form the
   * space editor needs, and the single place a chip changes state.
   */
  setTagTerm: (tag: string, term: TagChipState) => void;
  /**
   * Take one tag chip off the bar outright, whichever state it was in.
   *
   * Not `setTagTerm(tag, "off")` at every call site: the chip's own dismiss
   * affordance means "I am done with this tag", and spelling that as a state
   * transition invites the next reader to wonder whether `off` is a fourth
   * state a chip can sit in. It is not — it is the absence of one.
   */
  removeTag: (tag: string) => void;
  /** Replace the search text. */
  setText: (text: string) => void;
  /** Set the "Changed by agent" chip. */
  setAgentOnly: (on: boolean) => void;
  /** Set the "Pinned only" chip. */
  setPinnedOnly: (on: boolean) => void;
  setHideServiceFiles: (hidden: boolean) => void;
  /**
   * Drop the trailing chip, walking the bar down one press at a time (the Esc
   * contract). Resolves in reverse bar order — pinned, then origin, then the
   * newest tag term, then the scope — so repeated presses empty the bar from its
   * end and land on an unfiltered list rather than a random one. A tag term
   * leaves whole: Esc is an undo of the chip, not a step backwards through its
   * cycle.
   */
  dropLastChip: () => void;
  /** Clear every chip and the search text. */
  clearAll: () => void;
  /** Ask the pane to put the caret in the search field. */
  requestSearchFocus: () => void;
}

let visibilityRevision = 0;
let acknowledgedVisibility = true;
let visibilityWrites: Promise<void> = Promise.resolve();
let visibilityHydration: Promise<void> | null = null;

/** A read started before a click must not undo the person's newer choice. */
export function hydrateHideServiceFiles(): Promise<void> {
  visibilityHydration ??= (async () => {
    const revision = visibilityRevision;
    await visibilityWrites;
    const value = await notesHideServiceFilesGet();
    const hidden = typeof value === "boolean" ? value : true;
    if (revision === visibilityRevision) {
      acknowledgedVisibility = hidden;
      notesFiltersStore.getState().setHideServiceFiles(hidden);
    }
  })();
  return visibilityHydration;
}

/** Serialize dedicated preference writes; only the latest failure rolls back. */
export function persistHideServiceFiles(hidden: boolean): Promise<void> {
  notesFiltersStore.getState().setHideServiceFiles(hidden);
  const revision = visibilityRevision;
  const write = visibilityWrites.then(async () => {
    try {
      await notesHideServiceFilesSet(hidden);
      acknowledgedVisibility = hidden;
    } catch (error) {
      if (revision === visibilityRevision) {
        notesFiltersStore.getState().setHideServiceFiles(acknowledgedVisibility);
      }
      throw error;
    }
  });
  visibilityWrites = write.catch(() => {});
  return write;
}

let privateRevision = 0;
let acknowledgedPrivate = false;
let privateWrites: Promise<void> = Promise.resolve();
let privateHydration: Promise<void> | null = null;

export function hydrateIncludePrivate(): Promise<void> {
  privateHydration ??= (async () => {
    const revision = privateRevision;
    await privateWrites;
    const value = await notesIncludePrivateGet();
    if (revision === privateRevision) {
      acknowledgedPrivate = value;
      notesFiltersStore.getState().setIncludePrivate(value);
    }
  })();
  return privateHydration;
}

export function persistIncludePrivate(value: boolean): Promise<void> {
  notesFiltersStore.getState().setIncludePrivate(value);
  const revision = privateRevision;
  const write = privateWrites.then(async () => {
    try {
      await notesIncludePrivateSet(value);
      acknowledgedPrivate = value;
    } catch (error) {
      if (revision === privateRevision) {
        notesFiltersStore.getState().setIncludePrivate(acknowledgedPrivate);
      }
      throw error;
    }
  });
  privateWrites = write.catch(() => {});
  return write;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const notesFiltersStore = createStore<NotesFiltersState>()((set) => ({
  scope: ALL_NOTES_SCOPE,
  tagTerms: [],
  text: "",
  agentOnly: false,
  pinnedOnly: false,
  flags: [],
  origin: null,
  setFlags: (flags) => set({ flags, pinnedOnly: flags.includes("pinned") }),
  setOrigin: (origin) => set({ origin, agentOnly: origin === "agent" }),
  hideServiceFiles: true,
  sort: null,
  vaultIds: [],
  includePrivate: false,
  enteredSpace: null,
  railSpaces: null,
  setRailSpaces: (railSpaces) => set({ railSpaces }),
  spacesNonce: 0,
  setSort: (sort) => set({ sort }),
  setVaultIds: (vaultIds) => set({ vaultIds: [...new Set(vaultIds)] }),
  setIncludePrivate: (includePrivate) => {
    privateRevision += 1;
    set({ includePrivate });
  },
  enterSpace: (space) => {
    if (space.id === ALL_SPACE_ID) {
      set({
        enteredSpace: null,
        scope: ALL_NOTES_SCOPE,
        tagTerms: [],
        flags: [],
        origin: null,
        text: "",
        agentOnly: false,
        pinnedOnly: false,
        sort: null,
      });
      return;
    }
    const restore = space.restore;
    set({
      enteredSpace: space,
      scope: { kind: "space", spaces: [scopeSpaceOf(space)] },
      tagTerms: restore.opaque
        ? []
        : Object.entries(restore.tagTerms).map(([tag, term]) => ({ tag, term })),
      text: restore.opaque ? "" : (restore.text ?? ""),
      agentOnly: !restore.opaque && restore.origin === "agent",
      pinnedOnly: !restore.opaque && restore.flags.includes("pinned"),
      flags: restore.opaque ? [] : restore.flags,
      origin: restore.opaque ? null : restore.origin,
      sort: restore.opaque ? null : restoredSort(restore.sort),
    });
  },
  mergeSpace: (space) => {
    const restore = space.restore;
    if (restore.opaque) throw new Error("This space's search can't be combined — open it instead.");
    set((state) => {
      const incoming = Object.entries(restore.tagTerms);
      if (
        incoming.some(([tag, term]) =>
          state.tagTerms.some((chip) => chip.tag === tag && chip.term !== term),
        )
      ) {
        throw new Error(
          "These searches use opposite filters for the same tag — open the space instead.",
        );
      }
      if (state.origin && restore.origin && state.origin !== restore.origin) {
        throw new Error("These searches use different origins — open the space instead.");
      }
      return {
        tagTerms: incoming.reduce<readonly TagChip[]>(
          (chips, [tag, term]) => withTagTerm(chips, tag, term),
          state.tagTerms,
        ),
        agentOnly: state.agentOnly || restore.origin === "agent",
        pinnedOnly: state.pinnedOnly || restore.flags.includes("pinned"),
        flags: [...new Set([...state.flags, ...restore.flags])],
        origin: state.origin ?? restore.origin,
      };
    });
  },
  toggleSpace: (space) =>
    set((state) => {
      const key = spaceKey(space);
      const members = scopeSpaces(state.scope);
      const next = scopeHas(state.scope, space)
        ? members.filter((member) => spaceKey(member) !== key)
        : [...members, scopeSpaceOf(space)];
      const [first, ...rest] = next;
      // The last one out is today's scope ×: the bar stays, the scope's sort goes.
      if (first === undefined) return { scope: ALL_NOTES_SCOPE, enteredSpace: null, sort: null };
      return {
        ...(state.enteredSpace ? withoutRestored(state, state.enteredSpace) : {}),
        scope: { kind: "space", spaces: [first, ...rest] },
        enteredSpace: null,
      };
    }),
  restoreScope: (stamp) => {
    const state = notesFiltersStore.getState();
    if (sameScope(state.scope, stamp)) return;
    const rows: NoteSpaceVm[] = [];
    for (const member of scopeSpaces(stamp)) {
      const row = state.railSpaces?.find((each) => spaceKey(each) === spaceKey(member));
      // All or nothing: a partial union is a scope nobody chose.
      if (row === undefined || row.error !== null) return;
      rows.push(row);
    }
    // The members as the rail reads them now, not as the stamp remembered them:
    // a space renamed since the note was opened is shown under its new name.
    const [first, ...rest] = rows.map(scopeSpaceOf);
    const scope: NoteScope =
      stamp.kind === "space" && first ? { kind: "space", spaces: [first, ...rest] } : stamp;
    const untouched = state.enteredSpace
      ? !spaceDrift(state)
      : state.text.trim() === "" &&
        state.tagTerms.length === 0 &&
        state.flags.length === 0 &&
        state.origin === null &&
        !state.agentOnly &&
        !state.pinnedOnly;
    const [only] = rows;
    if (untouched && rows.length === 1 && only) {
      state.enterSpace(only);
      return;
    }
    if (untouched) {
      // The bar held nothing but the outgoing space's own search.
      set({
        scope,
        enteredSpace: null,
        tagTerms: [],
        text: "",
        flags: [],
        origin: null,
        agentOnly: false,
        pinnedOnly: false,
        sort: null,
      });
      return;
    }
    // The person typed a search of their own: it stays, and follows the scope.
    set({
      ...(state.enteredSpace ? withoutRestored(state, state.enteredSpace) : {}),
      scope,
      enteredSpace: null,
      sort: null,
    });
  },
  requestSpacesReload: () => set((state) => ({ spacesNonce: state.spacesNonce + 1 })),
  searchNonce: 0,
  setScope: (scope) =>
    set((state) => ({
      scope: sameScope(state.scope, scope) ? ALL_NOTES_SCOPE : scope,
      sort: null,
      enteredSpace: null,
    })),
  toggleTagSign: (tag) =>
    set((state) => ({
      tagTerms: withTagTerm(state.tagTerms, tag, flipTagTerm(tagChipState(state.tagTerms, tag))),
    })),
  resetToEnteredSpace: () => {
    const state = notesFiltersStore.getState();
    if (state.enteredSpace) state.enterSpace(state.enteredSpace);
  },
  resetSearch: () => {
    const state = notesFiltersStore.getState();
    if (state.enteredSpace) {
      state.enterSpace(state.enteredSpace);
      return;
    }
    // Everything the bar shows as a chip of the QUERY goes: the prompt, the
    // tags and the flags. `is:pinned` left filtering the list after a reset is
    // the same surprise as tags surviving a clear. The scope is deliberately
    // kept — it is the rail's selection rather than part of the query, and it
    // carries its own dismiss.
    set({ text: "", tagTerms: [], flags: [], origin: null, pinnedOnly: false, agentOnly: false });
  },
  setTagTerm: (tag, term) => set((state) => ({ tagTerms: withTagTerm(state.tagTerms, tag, term) })),
  removeTag: (tag) => set((state) => ({ tagTerms: withTagTerm(state.tagTerms, tag, "off") })),
  setText: (text) => set({ text }),
  setAgentOnly: (agentOnly) => set({ agentOnly, origin: agentOnly ? "agent" : null }),
  setPinnedOnly: (pinnedOnly) =>
    set((state) => ({
      pinnedOnly,
      flags: pinnedOnly
        ? [...new Set([...state.flags, "pinned"])]
        : state.flags.filter((flag) => flag !== "pinned"),
    })),
  setHideServiceFiles: (hideServiceFiles) => {
    visibilityRevision += 1;
    set({ hideServiceFiles });
  },
  dropLastChip: () =>
    set((state) => {
      if (state.pinnedOnly) {
        return { pinnedOnly: false, flags: state.flags.filter((flag) => flag !== "pinned") };
      }
      if (state.agentOnly) {
        return { agentOnly: false, origin: null };
      }
      if (state.origin) return { origin: null };
      if (state.flags.length) return { flags: state.flags.slice(0, -1) };
      if (state.tagTerms.length > 0) {
        return { tagTerms: state.tagTerms.slice(0, -1) };
      }
      if (state.scope.kind === "space" && state.scope.spaces.length > 1) {
        const [first, ...rest] = state.scope.spaces;
        return { scope: { kind: "space", spaces: [first, ...rest.slice(0, -1)] } };
      }
      if (state.scope.kind !== "all") {
        return { scope: ALL_NOTES_SCOPE, sort: null, enteredSpace: null };
      }
      return {};
    }),
  clearAll: () =>
    set({
      scope: ALL_NOTES_SCOPE,
      tagTerms: [],
      text: "",
      agentOnly: false,
      pinnedOnly: false,
      flags: [],
      origin: null,
      sort: null,
      vaultIds: [],
      enteredSpace: null,
    }),
  requestSearchFocus: () => set((state) => ({ searchNonce: state.searchNonce + 1 })),
}));

/**
 * Whether anything is narrowing the list. Drives the difference between the two
 * empty states that must never be confused: an empty vault is an invitation to
 * write the first note, an empty result is an invitation to widen the filter.
 */
export function isFiltered(state: NotesFiltersState): boolean {
  return (
    state.scope.kind !== "all" ||
    state.tagTerms.length > 0 ||
    state.text.trim() !== "" ||
    state.agentOnly ||
    state.pinnedOnly ||
    state.flags.length > 0 ||
    state.origin !== null
  );
}

/**
 * Whether the sidebar scope is the ONLY thing narrowing the list.
 *
 * A lens with nothing in it is entitled to say so in its own voice — but only
 * while the lens is all that is applied. Once a chip or a query sits on top,
 * "this vault has no recordings" is a lie about the vault rather than a fact
 * about the filter. Every axis is enumerated here, beside {@link isFiltered},
 * so a chip added later cannot leave that sentence quietly wrong.
 */
export function isScopeOnly(state: NotesFiltersState): boolean {
  return (
    state.tagTerms.length === 0 &&
    state.text.trim() === "" &&
    !state.agentOnly &&
    !state.pinnedOnly &&
    state.flags.length === 0 &&
    state.origin === null &&
    state.vaultIds.length === 0
  );
}

/**
 * Compose the chip set into the request Rust evaluates.
 *
 * Every axis is expressed even when it is empty, because `NoteQueryReq` is a
 * complete description of the window and not a patch — an omitted axis would
 * mean "unchanged" to a reader and "unfiltered" to Rust.
 *
 * **No scope contributes a flag any more** (Story 44.3). A scope is a space or
 * a folder, and a space's terms are its own DSL text in the vault, evaluated by
 * Rust from `spaces` — each named with the drive it is read from (AD-306). The
 * table that mapped four hard-coded rows onto `untagged`/`journal`/`pinned`/
 * `recording` is gone with the rows; those four strings now live where every
 * other query term lives, in the note.
 *
 * `spaceTerms` is false only for a single entered space whose saved terms are
 * already on the bar: Rust applying them again would stop a removed chip from
 * widening the space.
 *
 * `pinnedOnly` is the one flag left, and it is a chip rather than a scope.
 */
export function noteQueryFor(
  state: NotesFiltersState,
  offset: number,
  limit: number,
): NoteQueryReq {
  const flags = [...state.flags];
  if (state.pinnedOnly && !flags.includes("pinned")) {
    flags.push("pinned");
  }
  const text = state.text.trim();
  const spaces = scopeSpaces(state.scope);
  return {
    text: text === "" ? null : text,
    // Keyed by tag, so the request cannot say "include and exclude draft" — the
    // same thing the three-state chip guarantees at this end (FR-148).
    tags: Object.fromEntries(state.tagTerms.map((chip) => [chip.tag, chip.term])),
    spaces: spaces.map((space) => ({ vaultId: space.vaultId, spaceId: space.id })),
    spaceTerms:
      spaces.length !== 1 ||
      state.enteredSpace === null ||
      spaceKey(state.enteredSpace) !== spaceKey(spaces[0]) ||
      state.enteredSpace.restore.opaque,
    // The DSL's origin vocabulary: `agent` is a commit whose `Keeper-Source` is
    // `bot`. There is one chip because there is one question people ask of it.
    origin: state.origin ?? (state.agentOnly ? "agent" : null),
    hideServiceFiles: state.hideServiceFiles,
    sort: state.sort ? `${state.sort.key} ${state.sort.dir}` : null,
    vaultIds: [...state.vaultIds],
    includePrivate: state.includePrivate,
    flags,
    offset,
    limit,
  };
}

/**
 * The terms that are narrowing the list, in bar order, said in words — for the
 * sentence an empty result shows (FR-148, UX-DR54). `null` when nothing is.
 *
 * An exclusion is the term a person cannot see the effect of. An inclusion that
 * goes too far leaves a list that visibly does not contain what you wanted; an
 * exclusion leaves the same empty pane whether it removed one note or nine
 * hundred, and the chip that did it says only `−draft`. So the empty state names
 * the terms rather than repeating "no notes match these filters", and it says
 * "not draft" in words because a `−` glyph does not survive being read aloud.
 *
 * **This names every active term, not the one to blame.** Attributing an empty
 * result to a single chip would mean re-running the query once per term, and
 * that is a promise this surface cannot keep cheaply or honestly — two terms can
 * each be innocent alone and empty the list together. What it can promise is
 * that the term you have forgotten about is in the sentence.
 */
export function emptyFilterReason(state: NotesFiltersState): string | null {
  const terms = [
    state.scope.kind === "all" ? null : scopeLabel(state.scope),
    ...state.tagTerms.map((chip) => (chip.term === "exclude" ? `not ${chip.tag}` : chip.tag)),
    state.agentOnly ? "changed by agent" : null,
    state.pinnedOnly ? "pinned only" : null,
    ...state.flags.filter((flag) => flag !== "pinned").map((flag) => `is:${flag}`),
    state.origin && state.origin !== "agent" ? `origin:${state.origin}` : null,
    state.text.trim() === "" ? null : `"${state.text.trim()}"`,
  ].filter((term): term is string => term !== null);
  if (terms.length === 0) {
    return null;
  }
  const last = terms[terms.length - 1];
  const phrase = terms.length === 1 ? last : `${terms.slice(0, -1).join(", ")} and ${last}`;
  // A tag that is narrowing by itself gets one more sentence, because there is a
  // way for it to be honest AND empty: the rail's tag counts include the tags on
  // recordings, and this list shows notes. A vault with a recording tagged
  // `epic22` and no note carrying it shows `epic22 1` in the rail and nothing
  // here — which reads as a bug in the filter rather than as a fact about where
  // the tag lives.
  //
  // Said only for a tag-only narrowing: with a search term or a scope in the
  // sentence there are other explanations, and offering this one would be
  // guessing at which term emptied the list — the thing the doc above says this
  // function will not do.
  const tagOnly =
    state.tagTerms.length > 0 &&
    terms.length === state.tagTerms.length &&
    state.tagTerms.every((chip) => chip.term !== "exclude");
  const aside = tagOnly
    ? " A tag can also be carried by a recording, which this list does not show."
    : "";
  return `Narrowed by ${phrase}.${aside}`;
}

/**
 * React selector hook over {@link notesFiltersStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useNotesFiltersStore<T>(selector: (state: NotesFiltersState) => T): T {
  return useStore(notesFiltersStore, selector);
}

/** Test-only reset: clear every chip and the search text. */
export function resetNotesFiltersStoreForTest(): void {
  notesFiltersStore.getState().clearAll();
  notesFiltersStore.getState().setHideServiceFiles(true);
  notesFiltersStore.getState().setIncludePrivate(false);
  notesFiltersStore.getState().setRailSpaces(null);
  acknowledgedPrivate = false;
  privateWrites = Promise.resolve();
  privateHydration = null;
  acknowledgedVisibility = true;
  visibilityWrites = Promise.resolve();
  visibilityHydration = null;
}
