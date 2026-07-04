/**
 * Accounts store (AD-9).
 *
 * A vanilla zustand store created at module load *outside* React, holding only
 * the non-secret {@link AccountVm} for the currently signed-in account plus
 * ephemeral UI state. It never holds tokens or any `MatrixSession` material —
 * those live only in the macOS Keychain, in Rust. Components read it via the
 * {@link useAccountsStore} selector hook.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { AccountVm } from "@/lib/ipc/client";

export interface AccountsState {
  /** The currently signed-in account, or `null` when signed out. */
  currentAccount: AccountVm | null;
  /**
   * Whether the one-shot boot session-restore attempt has completed (Story 1.8).
   * `false` until then so `App` holds a splash instead of flashing the login
   * screen for a restorable user; flips `true` on both restore success and
   * failure (fail-safe to login).
   */
  hydrated: boolean;
  /** Record a successful login. Gates the shell. */
  setCurrentAccount: (account: AccountVm) => void;
  /** Mark the boot restore attempt as complete (success or failure). */
  markHydrated: () => void;
  /** Clear the current account (sign out). */
  clear: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for auth-gating is this single slice.
 */
export const accountsStore = createStore<AccountsState>()((set) => ({
  currentAccount: null,
  hydrated: false,
  setCurrentAccount: (account) => set({ currentAccount: account }),
  markHydrated: () => set({ hydrated: true }),
  clear: () => set({ currentAccount: null }),
}));

/**
 * React selector hook over {@link accountsStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useAccountsStore<T>(selector: (state: AccountsState) => T): T {
  return useStore(accountsStore, selector);
}
