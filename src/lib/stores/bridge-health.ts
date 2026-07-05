/**
 * Bridge-session health mirror store (Story 6.5, FR-28, AD-8).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only the
 * Rust-authoritative {@link BridgeSessionHealthVm} for every monitored (logged-in)
 * session, keyed `${accountId}:${networkId}` — never a source of truth. The whole set
 * is replaced wholesale on each streamed {@link BridgeHealthSnapshot} (no diff protocol
 * — sessions are few, and Rust already diffs before emitting). A session absent from
 * the map has no live health (not monitored, or Healthy-but-dropped is impossible since
 * the snapshot always carries every monitored session).
 *
 * The card dot + state word, the sidebar Bridges worst-state roll-up, the affected
 * chat-row dot, and the non-dismissible in-conversation re-link banner are all pure
 * projections of this single slice — never re-derived on the frontend.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { BridgeHealth, BridgeHealthSnapshot, BridgeSessionHealthVm } from "@/lib/ipc/client";

/**
 * The keyed-map key for a session, flattening the Rust `(accountId, networkId)` tuple.
 * Both parts are percent-encoded so a `:` in an account id can't alias two distinct
 * sessions (e.g. `("a","b:c")` vs `("a:b","c")`). Build and lookup both route through
 * here, so the encoding stays internally consistent.
 */
export function healthKey(accountId: string, networkId: string): string {
  return `${encodeURIComponent(accountId)}:${encodeURIComponent(networkId)}`;
}

/** Worst-state precedence: disconnected beats degraded beats healthy. */
const HEALTH_RANK: Record<BridgeHealth, number> = {
  healthy: 0,
  degraded: 1,
  disconnected: 2,
};

/**
 * Roll a set of session healths up to the single worst state, or `null` when the set
 * is empty (no dot). Shared by the sidebar roll-up and any per-account roll-up.
 */
export function worstOf(healths: readonly BridgeHealth[]): BridgeHealth | null {
  if (healths.length === 0) {
    return null;
  }
  return healths.reduce((worst, h) => (HEALTH_RANK[h] > HEALTH_RANK[worst] ? h : worst));
}

export interface BridgeHealthState {
  /** Every monitored session's live health, keyed `${accountId}:${networkId}`,
   * exactly as Rust streamed it. Replaced wholesale on each snapshot. */
  sessions: Record<string, BridgeSessionHealthVm>;
  /** Replace the whole keyed map from a streamed snapshot. */
  applySnapshot: (snapshot: BridgeHealthSnapshot) => void;
  /** Clear every tracked session (subscription teardown). */
  reset: () => void;
}

/** The vanilla store instance, created once at module load. Source of truth stays in
 * Rust; this only mirrors the streamed snapshot. */
export const bridgeHealthStore = createStore<BridgeHealthState>()((set) => ({
  sessions: {},
  applySnapshot: (snapshot) =>
    set(() => {
      const sessions: Record<string, BridgeSessionHealthVm> = {};
      for (const session of snapshot.sessions) {
        sessions[healthKey(session.accountId, session.networkId)] = session;
      }
      return { sessions };
    }),
  reset: () => set({ sessions: {} }),
}));

/**
 * The live session health for one `(accountId, networkId)`, or `undefined` when the
 * session is not monitored (no snapshot entry). A subscription hook over the store.
 */
export function useBridgeHealth(
  accountId: string,
  networkId: string,
): BridgeSessionHealthVm | undefined {
  return useStore(bridgeHealthStore, (s) => s.sessions[healthKey(accountId, networkId)]);
}

/**
 * The single worst health across every monitored session (the sidebar Bridges roll-up
 * dot), or `null` when nothing is monitored. Ranges over the whole session set.
 */
export function useWorstBridgeHealth(): BridgeHealth | null {
  const sessions = useStore(bridgeHealthStore, (s) => s.sessions);
  return worstOf(Object.values(sessions).map((session) => session.health));
}
