/**
 * Command-palette open state (Story 9.1, epic 9 spine).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the ⌘K
 * shortcut hook can toggle the single always-mounted palette overlay from anywhere
 * and so any surface can open it programmatically. Pure UI state — the results
 * themselves are authoritative in Rust (`palette_query`); nothing here mirrors or
 * re-orders them. The default/action mode is derived by the palette component from
 * the input's leading `>`, so the store carries only the open/closed flag.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface CommandPaletteState {
  /** Whether the palette overlay is open. */
  isOpen: boolean;
  /** Open the palette. */
  open: () => void;
  /** Close the palette. */
  close: () => void;
  /** Toggle the palette open/closed (the ⌘K behavior). */
  toggle: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const commandPaletteStore = createStore<CommandPaletteState>()((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((state) => ({ isOpen: !state.isOpen })),
}));

/**
 * React selector hook over {@link commandPaletteStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useCommandPaletteStore<T>(selector: (state: CommandPaletteState) => T): T {
  return useStore(commandPaletteStore, selector);
}
