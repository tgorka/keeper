/**
 * Bridge re-link request signal (Story 10.4).
 *
 * A tiny vanilla zustand store created at module load *outside* React so a coarse
 * notification-click landing (from the Rust `notify://navigate` event) can record which
 * Bridge session `(accountId, networkId)` the user should be routed to re-link — without
 * prop-drilling into the Bridges view.
 *
 * This is a UI signal only, never a source of truth for bridge health (that is computed
 * in Rust and mirrored by the health store). Under the Option B MVP scope the coarse
 * landing routes to the Bridges view via {@link primaryViewStore}; the persistent Story
 * 6.5 surfaces route the user into the exact re-login. Exact re-login deep-landing (auto-
 * opening the login sheet for the specific network) is deferred to Epic 11 — this store
 * carries the target so that later work can consume it without a new event contract.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** The Bridge session a re-link was requested for. */
export interface BridgeRelinkTarget {
  /** The opaque keeper account id owning the bridge session. */
  accountId: string;
  /** The stable machine `network_id` of the bridge to re-link. */
  networkId: string;
}

export interface BridgeRelinkState {
  /** The pending re-link target, or `null` when none is requested. */
  target: BridgeRelinkTarget | null;
  /** Record a pending re-link target (from a coarse bridge-notification landing). */
  requestRelink: (target: BridgeRelinkTarget) => void;
  /** Clear the pending target once the Bridges view has consumed it. */
  clearRelink: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const bridgeRelinkStore = createStore<BridgeRelinkState>()((set) => ({
  target: null,
  requestRelink: (target) => set({ target }),
  clearRelink: () => set({ target: null }),
}));

/** Subscribe to the pending bridge re-link target. */
export function useBridgeRelinkTarget(): BridgeRelinkTarget | null {
  return useStore(bridgeRelinkStore, (s) => s.target);
}
