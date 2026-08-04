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
 *   - **Tags intersect.** Two chips mean "both", never "either" — the Apple
 *     Notes contract, and the one people already expect from a chip bar.
 *   - **A filter change is a filter.** Nothing here touches the selection or the
 *     open note (UX-DR41). The note under the cursor survives every chip, and
 *     the pane keeps it open even when the new filter would exclude its row.
 *
 * `folder` scope is the one that does not go through {@link NoteQueryReq}: the
 * physical lens has its own command (`notes_tree`, FR-106) because a
 * vault-relative directory is not one of the query's axes. {@link isFolderScope}
 * is how the pane picks its source.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteQueryReq } from "@/lib/ipc/client";

/**
 * What the list is scoped to — the sidebar row that is selected, or `all` when
 * none is. Every one of these is a filter and not a route (UX-DR41).
 *
 * "Today" is deliberately absent: the sidebar's Today row opens or creates
 * today's journal entry (FR-99), which is an action on one note rather than a
 * way of narrowing the list.
 */
export type NoteScope =
  | { readonly kind: "all" }
  | { readonly kind: "inbox" }
  | { readonly kind: "journal" }
  | { readonly kind: "pinned" }
  | { readonly kind: "space"; readonly id: string; readonly name: string }
  | { readonly kind: "folder"; readonly path: string };

/** The unscoped list — every note in the vault, in the vault's own order. */
export const ALL_NOTES_SCOPE: NoteScope = { kind: "all" };

/**
 * The `is:` flag each scope narrows by, or `null` where the scope is not a flag
 * at all. The strings are the closed enum `keeper_core::notes::query` parses, so
 * this table is the one place the two vocabularies meet.
 *
 * `inbox` maps to `untagged` — the honest home of the unfiled is the note no tag
 * has claimed, and `untagged` is what the index computes.
 */
const SCOPE_FLAG: Record<NoteScope["kind"], string | null> = {
  all: null,
  inbox: "untagged",
  journal: "journal",
  pinned: "pinned",
  space: null,
  folder: null,
};

/** The chip label for a scope, as the bar renders it. */
export function scopeLabel(scope: NoteScope): string {
  switch (scope.kind) {
    case "inbox":
      return "Inbox";
    case "journal":
      return "Journal";
    case "pinned":
      return "Pinned";
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

export interface NotesFiltersState {
  /** The selected sidebar scope; `all` when none is. */
  scope: NoteScope;
  /**
   * The active tag chips, in the order they were added. They INTERSECT: a note
   * matches only when it carries every one of them.
   */
  tags: string[];
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
  /** Add a tag chip, or remove it when it is already in the intersection. */
  toggleTag: (tag: string) => void;
  /** Remove one tag chip, widening the intersection. */
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
   * newest tag, then the scope — so repeated presses empty the bar from its end
   * and land on an unfiltered list rather than a random one.
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
  tags: [],
  text: "",
  agentOnly: false,
  pinnedOnly: false,
  searchNonce: 0,
  setScope: (scope) =>
    set((state) => ({
      scope: sameScope(state.scope, scope) ? ALL_NOTES_SCOPE : scope,
    })),
  toggleTag: (tag) =>
    set((state) => ({
      tags: state.tags.includes(tag) ? state.tags.filter((t) => t !== tag) : [...state.tags, tag],
    })),
  removeTag: (tag) => set((state) => ({ tags: state.tags.filter((t) => t !== tag) })),
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
      if (state.tags.length > 0) {
        return { tags: state.tags.slice(0, -1) };
      }
      if (state.scope.kind !== "all") {
        return { scope: ALL_NOTES_SCOPE };
      }
      return {};
    }),
  clearAll: () =>
    set({ scope: ALL_NOTES_SCOPE, tags: [], text: "", agentOnly: false, pinnedOnly: false }),
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
    state.tags.length > 0 ||
    state.text.trim() !== "" ||
    state.agentOnly ||
    state.pinnedOnly
  );
}

/**
 * Compose the chip set into the request Rust evaluates.
 *
 * Every axis is expressed even when it is empty, because `NoteQueryReq` is a
 * complete description of the window and not a patch — an omitted axis would
 * mean "unchanged" to a reader and "unfiltered" to Rust.
 *
 * Flags accumulate and intersect, exactly like tags: the Pinned scope and the
 * pinned-only chip resolve to the same flag, which is why this de-duplicates
 * rather than sending it twice.
 */
export function noteQueryFor(
  state: NotesFiltersState,
  offset: number,
  limit: number,
): NoteQueryReq {
  const flags: string[] = [];
  const scopeFlag = SCOPE_FLAG[state.scope.kind];
  if (scopeFlag !== null) {
    flags.push(scopeFlag);
  }
  if (state.pinnedOnly && !flags.includes("pinned")) {
    flags.push("pinned");
  }
  const text = state.text.trim();
  return {
    text: text === "" ? null : text,
    tags: [...state.tags],
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
