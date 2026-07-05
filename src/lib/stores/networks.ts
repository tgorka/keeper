/**
 * Networks mirror store (AD-9, AD-20, Story 4.6).
 *
 * A vanilla zustand store created at module load *outside* React. It holds the
 * distinct-Networks {@link NetworkVm} list streamed from the Rust
 * `keeper-core::inbox` merge's sixth (networks) channel, plus the single active
 * Network selection. The list is a pure mirror of the Rust-authoritative
 * {@link NetworksSnapshot} (a whole snapshot replaces it wholesale — no diff
 * protocol, since Networks are few and derived pre-filter).
 *
 * The `activeNetwork` selection is **ephemeral view state** (no persistence): it
 * identifies the selected Network by its `name` (cross-account), and the actual
 * inbox filtering happens in Rust (poked via {@link setNetworkFilter}). This store
 * only mirrors the selection so the sidebar can render the active row and the chat
 * list its dismissible chip; it never derives, sorts, or filters inbox membership.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NetworksSnapshot, NetworkVm } from "@/lib/ipc/client";

export interface NetworksState {
  /** The distinct-Networks list, exactly as Rust streamed it (name-sorted). */
  networks: NetworkVm[];
  /** The active Network filter (its name), or `null` when the inbox is unfiltered. */
  activeNetwork: string | null;
  /** Replace the Network list from a streamed whole snapshot. */
  applySnapshot: (snapshot: NetworksSnapshot) => void;
  /** Set (or clear) the active Network selection. Does NOT poke the Rust filter —
   *  the caller pairs this with {@link setNetworkFilter}. */
  setActiveNetwork: (name: string | null) => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app;
 * the source of truth for the Network list stays in Rust.
 */
export const networksStore = createStore<NetworksState>()((set) => ({
  networks: [],
  activeNetwork: null,
  applySnapshot: (snapshot) => set({ networks: snapshot.networks }),
  setActiveNetwork: (name) => set({ activeNetwork: name }),
  clear: () => set({ networks: [], activeNetwork: null }),
}));

/**
 * React selector hook over {@link networksStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useNetworksStore<T>(selector: (state: NetworksState) => T): T {
  return useStore(networksStore, selector);
}
