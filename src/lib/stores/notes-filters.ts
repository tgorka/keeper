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
 * cycle rewrites it in place, and {@link noteQueryFor} ships it as a map keyed by
 * tag so the wire cannot carry the contradiction either.
 *
 * `folder` scope is the one that does not go through {@link NoteQueryReq}: the
 * physical lens has its own command (`notes_tree`, FR-106) because a
 * vault-relative directory is not one of the query's axes. {@link isFolderScope}
 * is how the pane picks its source.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteQueryReq, NoteTagTerm } from "@/lib/ipc/client";

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
      readonly id: string;
      readonly name: string;
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
  | { readonly kind: "folder"; readonly path: string };

/** The unscoped list — every note in the vault, in the vault's own order. */
export const ALL_NOTES_SCOPE: NoteScope = { kind: "all" };

/** The chip label for a scope, as the bar renders it. */
export function scopeLabel(scope: NoteScope): string {
  switch (scope.kind) {
    case "space":
      return scope.name;
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
export function isFolderScope(scope: NoteScope): scope is { kind: "folder"; path: string } {
  return scope.kind === "folder";
}

/** Whether two scopes name the same thing (so re-selecting one clears it). */
function sameScope(a: NoteScope, b: NoteScope): boolean {
  if (a.kind !== b.kind) {
    return false;
  }
  if (a.kind === "space" && b.kind === "space") {
    return a.id === b.id;
  }
  if (a.kind === "folder" && b.kind === "folder") {
    return a.path === b.path;
  }
  return true;
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

/** The order one press walks. */
const CYCLE: readonly TagChipState[] = ["off", "include", "exclude"];

/**
 * The state one press moves a chip to.
 *
 * Include before exclude, because including is what people do far more often
 * and the common case has to be one press. Exported because the cycle order is
 * the control's contract and two surfaces press it: the chip in the bar goes
 * through {@link NotesFiltersState.cycleTag}, and a plain press in the tag tree
 * has to read the next state *before* it clears the rest of the bar — a second
 * definition of the order is a tree and a bar that disagree about what a press
 * does.
 */
export function nextTagChipState(state: TagChipState): TagChipState {
  return CYCLE[(CYCLE.indexOf(state) + 1) % CYCLE.length] ?? "off";
}

/**
 * What `tag` is currently doing in a chip list, `off` when it is doing nothing.
 *
 * The one reader of {@link NotesFiltersState.tagTerms}' shape. The tag tree, the
 * cycle and the space editor all ask this rather than each searching the array,
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

export interface NotesFiltersState {
  /** The selected sidebar scope; `all` when none is. */
  scope: NoteScope;
  /**
   * The active tag chips, in the order they were first pressed, at most one
   * entry per tag. They INTERSECT: a note matches only when it carries every
   * `include` and none of the `exclude`.
   */
  tagTerms: readonly TagChip[];
  /** The search field's text — a content scan, not a name match (FR-118). */
  text: string;
  /** The "Changed by agent" chip. */
  agentOnly: boolean;
  /** The "Pinned only" chip, independent of the Pinned scope row. */
  pinnedOnly: boolean;
  /**
   * A monotonic nonce bumped by the palette's Open Note… / Search Notes actions.
   * The search field's DOM node belongs to the pane that renders it, so rather
   * than lift a ref out of the tree, the pane subscribes to this and takes focus
   * on each bump — the same shape `chat-list-focus.ts` uses for the summon
   * hotkey.
   */
  searchNonce: number;
  /** Select a scope. Selecting the active one again clears it back to `all`. */
  setScope: (scope: NoteScope) => void;
  /**
   * Advance one tag chip: off → include → exclude → off. A chip that reaches
   * `off` leaves the array, so the bar shows exactly the terms that are doing
   * something.
   */
  cycleTag: (tag: string) => void;
  /**
   * Put one tag chip in a named state, `off` removing it. The explicit form the
   * space editor (43.4) needs, and what {@link NotesFiltersState.cycleTag} is
   * written in terms of, so there is one place a chip changes state.
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

/** The vanilla store instance, created once at module load and shared app-wide. */
export const notesFiltersStore = createStore<NotesFiltersState>()((set) => ({
  scope: ALL_NOTES_SCOPE,
  tagTerms: [],
  text: "",
  agentOnly: false,
  pinnedOnly: false,
  searchNonce: 0,
  setScope: (scope) =>
    set((state) => ({
      scope: sameScope(state.scope, scope) ? ALL_NOTES_SCOPE : scope,
    })),
  cycleTag: (tag) =>
    set((state) => ({
      tagTerms: withTagTerm(
        state.tagTerms,
        tag,
        nextTagChipState(tagChipState(state.tagTerms, tag)),
      ),
    })),
  setTagTerm: (tag, term) => set((state) => ({ tagTerms: withTagTerm(state.tagTerms, tag, term) })),
  removeTag: (tag) => set((state) => ({ tagTerms: withTagTerm(state.tagTerms, tag, "off") })),
  setText: (text) => set({ text }),
  setAgentOnly: (agentOnly) => set({ agentOnly }),
  setPinnedOnly: (pinnedOnly) => set({ pinnedOnly }),
  dropLastChip: () =>
    set((state) => {
      if (state.pinnedOnly) {
        return { pinnedOnly: false };
      }
      if (state.agentOnly) {
        return { agentOnly: false };
      }
      if (state.tagTerms.length > 0) {
        return { tagTerms: state.tagTerms.slice(0, -1) };
      }
      if (state.scope.kind !== "all") {
        return { scope: ALL_NOTES_SCOPE };
      }
      return {};
    }),
  clearAll: () =>
    set({ scope: ALL_NOTES_SCOPE, tagTerms: [], text: "", agentOnly: false, pinnedOnly: false }),
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
    state.pinnedOnly
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
    state.tagTerms.length === 0 && state.text.trim() === "" && !state.agentOnly && !state.pinnedOnly
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
 * Rust from `spaceId`. The table that mapped four hard-coded rows onto
 * `untagged`/`journal`/`pinned`/`recording` is gone with the rows; those four
 * strings now live where every other query term lives, in the note.
 *
 * `pinnedOnly` is the one flag left, and it is a chip rather than a scope.
 */
export function noteQueryFor(
  state: NotesFiltersState,
  offset: number,
  limit: number,
): NoteQueryReq {
  const flags: string[] = [];
  if (state.pinnedOnly) {
    flags.push("pinned");
  }
  const text = state.text.trim();
  return {
    text: text === "" ? null : text,
    // Keyed by tag, so the request cannot say "include and exclude draft" — the
    // same thing the three-state chip guarantees at this end (FR-148).
    tags: Object.fromEntries(state.tagTerms.map((chip) => [chip.tag, chip.term])),
    spaceId: state.scope.kind === "space" ? state.scope.id : null,
    // The DSL's origin vocabulary: `agent` is a commit whose `Keeper-Source` is
    // `bot`. There is one chip because there is one question people ask of it.
    origin: state.agentOnly ? "agent" : null,
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
}
