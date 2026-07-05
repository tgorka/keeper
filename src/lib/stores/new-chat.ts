/**
 * New-chat surface open/last-used state (Story 6.6, FR-32).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the ⌘N
 * shortcut hook can open the single always-mounted new-chat dialog from anywhere
 * without prop-drilling (mirrors {@link import("./search").searchStore}). Pure UI
 * state: whether the dialog is open, plus the last-used Account + Network so
 * re-opening defaults the pickers to the user's previous choice. No identifier,
 * resolve result, or session material is ever held here — the dialog owns its own
 * transient input/resolve state and discards it on close; Rust is the source of truth.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface NewChatState {
  /** Whether the new-chat dialog is open. */
  isOpen: boolean;
  /** The account id last used in the dialog, to default the picker (or `null`). */
  lastAccountId: string | null;
  /** The network id last used in the dialog, to default the picker (or `null`). */
  lastNetworkId: string | null;
  /** Open the dialog. */
  open: () => void;
  /** Close the dialog; the dialog discards its transient input on unmount. */
  close: () => void;
  /** Record the last-used Account + Network so the next open defaults to them. */
  rememberSelection: (accountId: string, networkId: string) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const newChatStore = createStore<NewChatState>()((set) => ({
  isOpen: false,
  lastAccountId: null,
  lastNetworkId: null,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  rememberSelection: (accountId, networkId) =>
    set({ lastAccountId: accountId, lastNetworkId: networkId }),
}));

/**
 * React selector hook over {@link newChatStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useNewChatStore<T>(selector: (state: NewChatState) => T): T {
  return useStore(newChatStore, selector);
}
