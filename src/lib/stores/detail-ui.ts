/**
 * Detail-panel open/close UI store (Story 13.1).
 *
 * A tiny vanilla zustand store created at module load *outside* React holding
 * whether the conversation Detail panel is open. Lifted out of `AppShell`'s
 * local state so both shell arrangements project the same signal (AD-31): the
 * desktop three-pane frame (pinned panel / floating Sheet) and the phone stack
 * (level 1 Room ↔ level 2 Detail) read and mutate one detail-open flag. Pure
 * UI state; never a source of truth for domain data.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface DetailUiState {
  /** Whether the conversation Detail panel is open. */
  open: boolean;
  /** Open the Detail panel. */
  openDetail: () => void;
  /** Close the Detail panel. */
  closeDetail: () => void;
  /** Toggle the Detail panel. */
  toggleDetail: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const detailStore = createStore<DetailUiState>()((set) => ({
  open: false,
  openDetail: () => set({ open: true }),
  closeDetail: () => set({ open: false }),
  toggleDetail: () => set((state) => ({ open: !state.open })),
}));

/**
 * React selector hook over {@link detailStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useDetailStore<T>(selector: (state: DetailUiState) => T): T {
  return useStore(detailStore, selector);
}
