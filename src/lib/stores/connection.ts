/**
 * Connection-status mirror store (AD-8, AD-9).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the Rust-authoritative {@link ConnectionStatus} streamed over the
 * connection-status channel — never a source of truth. The default is `"online"`
 * so no false-offline pill flashes before the first snapshot arrives. `applyBatch`
 * simply records the streamed scalar status (inherently idempotent); `reset()`
 * returns to the default on unsubscribe / account clear. The offline pill and the
 * "Queued" send caption are pure projections of this single slice.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { ConnectionStatus, ConnectionStatusBatch } from "@/lib/ipc/client";

export interface ConnectionState {
  /** The current connectivity, exactly as Rust streamed it. */
  status: ConnectionStatus;
  /** Apply one streamed batch, recording its current status. */
  applyBatch: (batch: ConnectionStatusBatch) => void;
  /** Reset to the `"online"` default (on unsubscribe / account change). */
  reset: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for connectivity stays in Rust.
 */
export const connectionStore = createStore<ConnectionState>()((set) => ({
  status: "online",
  applyBatch: (batch) => set({ status: batch.status }),
  reset: () => set({ status: "online" }),
}));

/**
 * React selector hook over {@link connectionStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useConnectionStore<T>(selector: (state: ConnectionState) => T): T {
  return useStore(connectionStore, selector);
}
