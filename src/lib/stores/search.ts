/**
 * Search-surface open/scope state (Story 5.4, FR-34).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the
 * `⌘⇧F` (global) / `⌘F` (in-chat) shortcut hook can open the single search
 * overlay from anywhere without prop-drilling. Pure UI state: it records only
 * whether the surface is open and, if so, its scope. Search results themselves
 * are NEVER held here — they live only in the overlay component's own lifetime
 * and are discarded on close (the archive in Rust is the source of truth).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/**
 * How the search surface was opened: `"global"` searches every account/room;
 * `"chat"` locks the query to the currently-selected Chat.
 */
export type SearchScope = "global" | "chat";

/**
 * Which content the surface searches (FR-267).
 *
 * Three sources rather than one merged list, because they are three different
 * searches over three different stores and only the word being looked for is
 * shared: messages come from the archive, notes from a vault scan, sessions
 * from a zone scan — and a vault and a zone can never be the same folder, so
 * there is no single walk that could serve two of them. Merging their results
 * would also merge their ranking, and a message and a session file have no
 * common order to be sorted into.
 *
 * `"chat"` scope always means `"messages"`: an in-chat search is a search of
 * that chat.
 */
export type SearchSource = "messages" | "notes" | "sessions";

export interface SearchState {
  /** Whether the search overlay is open. */
  isOpen: boolean;
  /** The scope the surface was opened with (meaningful only while open). */
  scope: SearchScope;
  /**
   * Which content the surface is searching. Set when it opens, from the view
   * the chord was pressed in, and switchable from the surface itself.
   */
  source: SearchSource;
  /** Open the surface with the given scope and source. */
  open: (scope: SearchScope, source?: SearchSource) => void;
  /** Switch sources without closing — the query survives the switch. */
  setSource: (source: SearchSource) => void;
  /** Close the surface; the overlay discards its results on unmount. */
  close: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const searchStore = createStore<SearchState>()((set) => ({
  isOpen: false,
  scope: "global",
  source: "messages",
  open: (scope, source) =>
    // A chat-locked surface is a message search by construction; anything else
    // opens on what it was told, defaulting to messages as it always did.
    set({ isOpen: true, scope, source: scope === "chat" ? "messages" : (source ?? "messages") }),
  setSource: (source) => set({ source }),
  close: () => set({ isOpen: false }),
}));

/**
 * React selector hook over {@link searchStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useSearchStore<T>(selector: (state: SearchState) => T): T {
  return useStore(searchStore, selector);
}
