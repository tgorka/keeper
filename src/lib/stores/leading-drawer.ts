/**
 * Leading-drawer open state (Story 13.3, FR-58 rail leg).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the
 * phone Inbox header's avatar button and the level-0 leading-edge swipe zone
 * share one source of drawer open-state without prop-drilling through
 * `PhoneShell`. Mirrors the always-mounted-overlay idiom of
 * {@link import("./command-palette").commandPaletteStore} /
 * {@link import("./new-chat").newChatStore}. Pure UI state — the drawer's
 * content (the reused `SidebarPane`) reads its own authoritative stores.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface LeadingDrawerState {
  /** Whether the leading navigation drawer is open. */
  isOpen: boolean;
  /** Open the drawer. */
  open: () => void;
  /** Close the drawer. */
  close: () => void;
  /** Toggle the drawer open/closed. */
  toggle: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const leadingDrawerStore = createStore<LeadingDrawerState>()((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  toggle: () => set((state) => ({ isOpen: !state.isOpen })),
}));

/**
 * React selector hook over {@link leadingDrawerStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useLeadingDrawerStore<T>(selector: (state: LeadingDrawerState) => T): T {
  return useStore(leadingDrawerStore, selector);
}
