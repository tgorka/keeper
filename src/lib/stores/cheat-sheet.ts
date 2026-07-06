/**
 * Cheat-sheet overlay open state (Story 9.3, epic 9 spine).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the ⌘?
 * shortcut hook can toggle the single always-mounted cheat-sheet overlay from
 * anywhere. Pure UI state — the shortcut reference itself is authoritative in Rust
 * (`cheat_sheet_sections`, derived from `palette_actions()`); nothing here mirrors,
 * re-orders, or holds that list. The overlay fetches sections fresh on open, so this
 * store carries only the open/closed flag (mirrors {@link import("./command-palette")}).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface CheatSheetState {
  /** Whether the cheat-sheet overlay is open. */
  isOpen: boolean;
  /** Open the cheat sheet. */
  open: () => void;
  /** Close the cheat sheet. */
  close: () => void;
  /** Toggle the cheat sheet open/closed (the ⌘? behavior). */
  toggle: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const cheatSheetStore = createStore<CheatSheetState>()((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((state) => ({ isOpen: !state.isOpen })),
}));

/**
 * React selector hook over {@link cheatSheetStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useCheatSheetStore<T>(selector: (state: CheatSheetState) => T): T {
  return useStore(cheatSheetStore, selector);
}
