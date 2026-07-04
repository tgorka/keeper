/**
 * Per-account connection-status mirror store (Story 2.5, AD-8, UX-DR18).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the Rust-authoritative {@link ConnectionStatus} for every tracked account,
 * keyed by opaque account id — never a source of truth. An account absent from
 * the map (`undefined`) means "no status batch yet" (the syncing/pending state);
 * `setStatus` records the streamed scalar (idempotent), `removeAccount` drops one
 * account's entry on sign-out / subscription teardown, and `reset()` clears all.
 *
 * The switcher's per-account sync glyph, the shell offline pill, and the
 * conversation "Queued" caption are all pure projections of this single slice.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { ConnectionStatus } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";

export interface AccountStatusState {
  /** The current connectivity per account id, exactly as Rust streamed it. An
   * account absent from the map has not delivered a status batch yet. */
  statuses: Record<string, ConnectionStatus>;
  /** Record one account's current status (from a streamed batch). */
  setStatus: (accountId: string, status: ConnectionStatus) => void;
  /** Drop one account's entry (sign-out / subscription teardown). */
  removeAccount: (accountId: string) => void;
  /** Clear every tracked account's status. */
  reset: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for connectivity stays in Rust.
 */
export const accountStatusStore = createStore<AccountStatusState>()((set) => ({
  statuses: {},
  setStatus: (accountId, status) =>
    set((state) => ({ statuses: { ...state.statuses, [accountId]: status } })),
  removeAccount: (accountId) =>
    set((state) => {
      if (!(accountId in state.statuses)) {
        return state;
      }
      const { [accountId]: _removed, ...rest } = state.statuses;
      return { statuses: rest };
    }),
  reset: () => set({ statuses: {} }),
}));

/**
 * The connection status for a single account, or `undefined` when no status
 * batch has arrived yet (the pending / syncing state). A subscription hook over
 * {@link accountStatusStore}.
 */
export function useAccountStatus(accountId: string): ConnectionStatus | undefined {
  return useStore(accountStatusStore, (s) => s.statuses[accountId]);
}

/**
 * Whether the shell offline pill should show: `true` iff there is at least one
 * signed-in account AND every signed-in account's status is exactly `"offline"`.
 *
 * The check ranges over the *signed-in account set* (not just the accounts that
 * have delivered a status batch): an account that is pending (no batch yet) has
 * `undefined` status, which is not `"offline"`, so it keeps the pill hidden.
 * That way a single account re-authing — or the transient pending state while a
 * newly added account's stream spins up — never flashes the shell pill; the
 * per-account switcher glyphs surface the offline one instead.
 */
export function useShellOffline(): boolean {
  const accounts = useAccountsStore((s) => s.accounts);
  const statuses = useStore(accountStatusStore, (s) => s.statuses);
  return accounts.length > 0 && accounts.every((a) => statuses[a.accountId] === "offline");
}
