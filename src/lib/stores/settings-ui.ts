/**
 * Shared Settings dialog open-state (Story 3.1), and where Settings should
 * land when something sends a person there (Story 102.8).
 *
 * A tiny vanilla zustand store created at module load *outside* React so any
 * affordance — the account-row menu, the "verify this device" banner, and the
 * UTD stub's inline action — can open the global {@link SettingsDialog} without
 * prop-drilling. Pure UI state; nothing here is a source of truth for domain
 * state (that stays in Rust).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { SettingsSectionId } from "@/lib/settings-categories";

/**
 * A section Settings is asked to show: its category opens, the section scrolls
 * into view, and the element `focusId` names — when it renders — takes focus.
 */
export interface SettingsReveal {
  section: SettingsSectionId;
  focusId: string | null;
}

export interface SettingsUiState {
  /** Whether the global Settings dialog is open. */
  settingsOpen: boolean;
  /** Open or close the Settings dialog. */
  setSettingsOpen: (open: boolean) => void;
  /**
   * The section the next Settings surface to read it should land on. Settings
   * is a list of categories, so "open Settings" alone no longer shows the
   * section a caller meant; the caller opens Settings the way it always has
   * and names the section here. The surface that lands it clears it.
   */
  reveal: SettingsReveal | null;
  revealSection: (section: SettingsSectionId, focusId?: string) => void;
  clearReveal: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const settingsUiStore = createStore<SettingsUiState>()((set) => ({
  settingsOpen: false,
  setSettingsOpen: (open) => set({ settingsOpen: open }),
  reveal: null,
  revealSection: (section, focusId) => set({ reveal: { section, focusId: focusId ?? null } }),
  clearReveal: () => set({ reveal: null }),
}));

/** Subscribe to whether the Settings dialog is open. */
export function useSettingsOpen(): boolean {
  return useStore(settingsUiStore, (s) => s.settingsOpen);
}
