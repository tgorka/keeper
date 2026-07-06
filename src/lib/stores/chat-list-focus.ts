/**
 * Chat-list focus-request signal (Story 9.4, FR-50).
 *
 * The global summon hotkey needs to move keyboard focus into the Unified Inbox chat
 * list, but Story 9.2's roving focus (`focusedKey`/`rowRefs`) is local component state
 * inside `chat-list-pane.tsx` with no external entry point. Rather than lift that
 * roving state out of the component (and risk re-deriving ordering), this tiny vanilla
 * zustand store carries a monotonic nonce: `requestFocus()` bumps it, and the chat-list
 * pane subscribes to the nonce and, on each bump, moves its own cursor to the first
 * visible Inbox row (or focuses the list container when empty). Pure UI signalling —
 * never a source of truth, never re-orders the Rust list.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface ChatListFocusState {
  /** Monotonic focus-request nonce; each `requestFocus()` increments it. */
  focusNonce: number;
  /** Request that the chat list move keyboard focus to its first visible row. */
  requestFocus: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const chatListFocusStore = createStore<ChatListFocusState>()((set) => ({
  focusNonce: 0,
  requestFocus: () => set((s) => ({ focusNonce: s.focusNonce + 1 })),
}));

/** Subscribe to the focus-request nonce (bumps whenever focus is requested). */
export function useChatListFocusNonce(): number {
  return useStore(chatListFocusStore, (s) => s.focusNonce);
}
