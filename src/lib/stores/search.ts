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

export interface SearchState {
  /** Whether the search overlay is open. */
  isOpen: boolean;
  /** The scope the surface was opened with (meaningful only while open). */
  scope: SearchScope;
  /** Open the surface with the given scope (global or chat-locked). */
  open: (scope: SearchScope) => void;
  /** Close the surface; the overlay discards its results on unmount. */
  close: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const searchStore = createStore<SearchState>()((set) => ({
  isOpen: false,
  scope: "global",
  open: (scope) => set({ isOpen: true, scope }),
  close: () => set({ isOpen: false }),
}));

/**
 * React selector hook over {@link searchStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useSearchStore<T>(selector: (state: SearchState) => T): T {
  return useStore(searchStore, selector);
}
