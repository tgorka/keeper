/**
 * Favorites section collapse/expand UI store (Story 4.4).
 *
 * A tiny vanilla zustand store created at module load *outside* React holding the
 * Favorites section's collapse chevron state. This is pure UI chrome, not Matrix
 * state; it is **hydrated** on mount from the persisted `favorites_collapsed`
 * registry setting (via `getFavoritesCollapsed`) and each toggle is **persisted**
 * back (via `setFavoritesCollapsed`) so it survives restart and re-login. The
 * in-memory default before hydration is expanded (`isCollapsed: false`).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface FavoritesUiState {
  /** Whether the Favorites section is collapsed (list hidden, header shown). */
  isCollapsed: boolean;
  /** Set the collapse state (in-memory only; persistence is the caller's job). */
  setCollapsed: (value: boolean) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const favoritesUiStore = createStore<FavoritesUiState>()((set) => ({
  isCollapsed: false,
  setCollapsed: (value) => set({ isCollapsed: value }),
}));

/**
 * React selector hook over {@link favoritesUiStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useFavoritesUiStore<T>(selector: (state: FavoritesUiState) => T): T {
  return useStore(favoritesUiStore, selector);
}
