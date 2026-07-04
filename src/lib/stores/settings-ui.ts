/**
 * Shared Settings dialog open-state (Story 3.1).
 *
 * A tiny vanilla zustand store created at module load *outside* React so any
 * affordance — the account-row menu, the "verify this device" banner, and the
 * UTD stub's inline action — can open the global {@link SettingsDialog} without
 * prop-drilling. Pure UI state; nothing here is a source of truth for domain
 * state (that stays in Rust).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface SettingsUiState {
  /** Whether the global Settings dialog is open. */
  settingsOpen: boolean;
  /** Open or close the Settings dialog. */
  setSettingsOpen: (open: boolean) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const settingsUiStore = createStore<SettingsUiState>()((set) => ({
  settingsOpen: false,
  setSettingsOpen: (open) => set({ settingsOpen: open }),
}));

/** Subscribe to whether the Settings dialog is open. */
export function useSettingsOpen(): boolean {
  return useStore(settingsUiStore, (s) => s.settingsOpen);
}
