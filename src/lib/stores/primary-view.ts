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
 * Bridges surface (Story 6.1), the Approval Pane (Story 7.3), the Recording
 * view (Story 16.3), the Recordings browser (Story 42.3), the Sync view (Story
 * 32.5), the Notes view (Story 37.1), or Settings. "inbox"/"archive" pick which
 * window the chat-list pane shows; "bridges", "approval", "recording",
 * "recordings", "sync", "notes" and "settings" each replace the chat-list +
 * conversation cluster entirely.
 *
 * Settings joined this list rather than staying a dialog because it is a place
 * you go and stay, not a question you answer and dismiss — and a modal covers the
 * app, which is wrong for a surface whose Sync section you read *while* watching a
 * folder work.
 *
 * "recordings" (the browser over every session ever recorded) is a sibling of
 * "recording" (the capture surface) rather than a tab inside it: the epic calls
 * it a browser, and a browser buried under the capture settings is a browser
 * nobody opens. Both are gated on the same `recording` capability — a browser
 * for recordings you cannot make is a puzzle, not a surface.
 */
export type PrimaryView =
  | "inbox"
  | "archive"
  | "bridges"
  | "approval"
  | "recording"
  | "recordings"
  | "sync"
  | "notes"
  | "settings";

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
