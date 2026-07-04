/**
 * Sign-out hook (AD-10, Story 1.8 / 2.1).
 *
 * Returns an async handler that signs out a *specific* account by id. It awaits
 * the Rust-authoritative `sign_out` command (local-only: tears down that
 * account's supervision and deletes its SDK store dir + Keychain session +
 * registry row; the merged inbox drops its rooms while other accounts keep
 * syncing), then removes the account from the accounts store.
 *
 * When the signed-out account is the one whose conversation is open, the open
 * selection and the mirrored timeline are cleared too. The account's per-account
 * connection-status entry is dropped so the shell offline pill and switcher glyph
 * never reflect a gone account. Signing out the *last* account additionally
 * resets the room mirror store and returns the user to the login screen (the
 * shell unmounts once `accounts` is empty).
 */
import { useCallback } from "react";
import { signOut } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

export function useSignOut(): (accountId: string) => Promise<void> {
  return useCallback(async (accountId: string) => {
    await signOut(accountId);

    // If the open conversation belonged to this account, close it and drop its
    // mirrored timeline so a signed-out account's messages never linger.
    const selected = roomsStore.getState().selected;
    if (selected?.accountId === accountId) {
      roomsStore.getState().selectRoom(null);
      timelineStore.getState().clear();
    }

    // Drop this account's per-account connection-status entry so the shell
    // offline pill / switcher glyph never reflect a signed-out account. (The
    // status subscriber's teardown also removes it, but do it here so it is gone
    // immediately regardless of subscriber timing.)
    accountStatusStore.getState().removeAccount(accountId);

    const remaining = accountsStore.getState().accounts.filter((a) => a.accountId !== accountId);
    if (remaining.length === 0) {
      // Last account signed out: reset the mirror stores, then clear accounts
      // last so the shell unmounts only after the mirrors are clean.
      roomsStore.getState().selectRoom(null);
      roomsStore.getState().clear();
      timelineStore.getState().clear();
    }
    accountsStore.getState().removeAccount(accountId);
  }, []);
}
