/**
 * Phone Search surface open/scope/lock state (Story 13.4, FR-34/FR-48/FR-58).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the
 * phone Inbox header magnifier, the level-0 pull-down gesture, and the Room ⋯
 * "Search in chat" overflow item share one source of the merged full-screen
 * Search surface's open/scope/lock state without prop-drilling through
 * `PhoneShell`. Mirrors the always-mounted-overlay idiom of
 * {@link import("./leading-drawer").leadingDrawerStore}. Pure UI state — the
 * surface reuses the desktop engines (`paletteQuery` / `searchArchive`) and holds
 * no search results here (Rust stays authoritative).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RoomSelection } from "@/lib/stores/rooms";

/**
 * Which segmented scope the surface opens in: `"chats"` (contacts + chats via
 * `paletteQuery` default mode), `"messages"` (message FTS via `searchArchive`),
 * or `"actions"` (palette action mode).
 */
export type SearchSurfaceScope = "chats" | "messages" | "actions";

/** Options for {@link SearchSurfaceState.open}; both fields optional. */
export interface OpenSearchSurfaceOptions {
  /** The scope to open in; defaults to `"chats"`. */
  scope?: SearchSurfaceScope;
  /**
   * An in-chat scope lock (the Room ⋯ "Search in chat" entry): forces Messages
   * scope locked to this Chat. `null`/absent for an unlocked (global) surface.
   */
  chatLock?: RoomSelection | null;
}

export interface SearchSurfaceState {
  /** Whether the full-screen Search surface is open. */
  isOpen: boolean;
  /** The active segmented scope (meaningful only while open). */
  scope: SearchSurfaceScope;
  /**
   * The in-chat scope lock, or `null` for an unlocked surface. When set, the
   * surface is locked to this Chat in Messages scope (same semantics as the
   * desktop `search` store's `"chat"` scope).
   */
  chatLock: RoomSelection | null;
  /** Open the surface with an optional scope + chat lock (defaults: Chats, no lock). */
  open: (options?: OpenSearchSurfaceOptions) => void;
  /** Close the surface and clear the chat lock; results live only in the surface component. */
  close: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const searchSurfaceStore = createStore<SearchSurfaceState>()((set) => ({
  isOpen: false,
  scope: "chats",
  chatLock: null,
  open: (options) =>
    set({
      isOpen: true,
      scope: options?.scope ?? "chats",
      chatLock: options?.chatLock ?? null,
    }),
  close: () => set({ isOpen: false, chatLock: null }),
}));

/**
 * React selector hook over {@link searchSurfaceStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useSearchSurfaceStore<T>(selector: (state: SearchSurfaceState) => T): T {
  return useStore(searchSurfaceStore, selector);
}
