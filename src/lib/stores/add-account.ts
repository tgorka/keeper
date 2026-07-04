/**
 * Add-account overlay store (Story 2.1, minimal).
 *
 * A vanilla zustand store holding one ephemeral UI flag: whether the "add
 * account" login overlay is open. The sidebar footer's "Add Account" button
 * opens it; `App` renders the login screen in add mode while it is open and
 * closes it on a successful add or cancel. Intentionally throwaway — Story 2.5
 * replaces the minimal footer with the designed switcher.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface AddAccountState {
  /** Whether the add-account login overlay is open. */
  open: boolean;
  /** Open the add-account overlay. */
  openAddAccount: () => void;
  /** Close the add-account overlay. */
  closeAddAccount: () => void;
}

export const addAccountStore = createStore<AddAccountState>()((set) => ({
  open: false,
  openAddAccount: () => set({ open: true }),
  closeAddAccount: () => set({ open: false }),
}));

export function useAddAccountStore<T>(selector: (state: AddAccountState) => T): T {
  return useStore(addAccountStore, selector);
}
