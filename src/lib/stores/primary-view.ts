/**
 * Primary-view switch (Story 4.2).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the
 * sidebar can switch which window the chat-list pane renders — the Unified Inbox
 * or the Archive — without prop-drilling. Pure UI state; nothing here is a source
 * of truth for domain state (the inbox/archive split is computed in Rust).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/**
 * Which primary window the shell renders: the Unified Inbox, the Archive, the
 * Bridges surface (Story 6.1), the Approval Pane (Story 7.3), or the Recording
 * view (Story 16.3). "inbox"/"archive" pick which window the chat-list pane shows;
 * "bridges", "approval", and "recording" each replace the chat-list + conversation
 * cluster entirely.
 */
export type PrimaryView = "inbox" | "archive" | "bridges" | "approval" | "recording";

export interface PrimaryViewState {
  /** The active primary view; defaults to the Unified Inbox. */
  view: PrimaryView;
  /** Switch the active primary view. */
  setView: (view: PrimaryView) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const primaryViewStore = createStore<PrimaryViewState>()((set) => ({
  view: "inbox",
  setView: (view) => set({ view }),
}));

/** Subscribe to the active primary view. */
export function usePrimaryView(): PrimaryView {
  return useStore(primaryViewStore, (s) => s.view);
}
