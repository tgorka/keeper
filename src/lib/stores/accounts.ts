/**
 * Accounts store (AD-9, AD-20).
 *
 * A vanilla zustand store created at module load *outside* React, holding only
 * the non-secret {@link AccountVm}s for every signed-in account plus ephemeral
 * boot state. It never holds tokens or any `MatrixSession` material — those live
 * only in the macOS Keychain, in Rust. Components read it via the
 * {@link useAccountsStore} selector hook.
 *
 * Multi-account (Story 2.1): `accounts` is an array; adding the Nth account is
 * identical to the 2nd (no count cap). `addAccount` upserts by `accountId` so a
 * re-login of an existing account never duplicates a row.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { AccountVm } from "@/lib/ipc/client";

export interface AccountsState {
  /** Every signed-in account, in restore/add order. Empty when signed out. */
  accounts: AccountVm[];
  /**
   * Whether the one-shot boot session-restore attempt has completed (Story 1.8).
   * `false` until then so `App` holds a splash instead of flashing the login
   * screen for a restorable user; flips `true` on both restore success and
   * failure (fail-safe to login).
   */
  hydrated: boolean;
  /** Replace the account set with the boot-restored accounts (hydrate all). */
  hydrateAll: (accounts: AccountVm[]) => void;
  /** Add (or upsert by `accountId`) one signed-in account. */
  addAccount: (account: AccountVm) => void;
  /** Remove one account by id (sign out). */
  removeAccount: (accountId: string) => void;
  /** Mark the boot restore attempt as complete (success or failure). */
  markHydrated: () => void;
  /** Clear all accounts (full sign-out / reset). */
  clear: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for auth-gating is this single slice.
 */
export const accountsStore = createStore<AccountsState>()((set) => ({
  accounts: [],
  hydrated: false,
  hydrateAll: (accounts) => set({ accounts }),
  addAccount: (account) =>
    set((state) => {
      const rest = state.accounts.filter((a) => a.accountId !== account.accountId);
      return { accounts: [...rest, account] };
    }),
  removeAccount: (accountId) =>
    set((state) => ({ accounts: state.accounts.filter((a) => a.accountId !== accountId) })),
  markHydrated: () => set({ hydrated: true }),
  clear: () => set({ accounts: [] }),
}));

/**
 * React selector hook over {@link accountsStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useAccountsStore<T>(selector: (state: AccountsState) => T): T {
  return useStore(accountsStore, selector);
}
