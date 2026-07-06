/**
 * Run-your-own-bridge sheet open state (Story 6.7, FR-29).
 *
 * A tiny vanilla zustand store created at module load *outside* React so the
 * always-mounted run Sheet can be opened from the {@link import("@/components/bridges/bbctl-panel").BbctlPanel}
 * without prop-drilling (mirrors {@link import("./new-chat").newChatStore}). Pure UI
 * state: whether the Sheet is open, plus the `(accountId, networkId)` it is opened
 * for. No token, `bbctl` output, or session material is ever held here — the Sheet
 * owns its own transient run state and discards it on close; Rust is the source of
 * truth.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface BbctlState {
  /** Whether the run Sheet is open. */
  isOpen: boolean;
  /** The account id the run is keyed to (or `null` when closed). */
  accountId: string | null;
  /** The network id selected to run (or `null` when closed). */
  selectedNetworkId: string | null;
  /** Open the run Sheet for `(accountId, networkId)`. */
  open: (accountId: string, networkId: string) => void;
  /** Close the run Sheet; the Sheet discards its transient state on unmount. */
  close: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const bbctlStore = createStore<BbctlState>()((set) => ({
  isOpen: false,
  accountId: null,
  selectedNetworkId: null,
  open: (accountId, networkId) => set({ isOpen: true, accountId, selectedNetworkId: networkId }),
  close: () => set({ isOpen: false, accountId: null, selectedNetworkId: null }),
}));

/**
 * React selector hook over {@link bbctlStore}. Pass a selector to subscribe to just
 * the slice a component needs.
 */
export function useBbctlStore<T>(selector: (state: BbctlState) => T): T {
  return useStore(bbctlStore, selector);
}
