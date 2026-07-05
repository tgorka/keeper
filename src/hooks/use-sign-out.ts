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
import { toast } from "sonner";
import { deleteAccountArchive, signOut } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

/** Options for the sign-out handler. */
export interface SignOutOptions {
  /**
   * Also permanently delete this account's local archive after signing out
   * (Story 5.7, FR-6). Defaults to `false` — the keep-archive path. The purge
   * runs *after* the account has been removed (so a purge failure never rolls
   * back the completed sign-out); a purge rejection is surfaced via a toast that
   * outlives the unmounting shell/dialog (the last-account case unmounts both).
   */
  deleteArchive?: boolean;
}

export function useSignOut(): (accountId: string, options?: SignOutOptions) => Promise<void> {
  return useCallback(async (accountId: string, options?: SignOutOptions) => {
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

    // Delete-archive path: purge AFTER removal so a purge failure never rolls back
    // the completed sign-out. Removing the last account unmounts the shell + the
    // sign-out dialog, so a dialog-local error would be invisible — surface a
    // purge failure via a toast that outlives the unmount, worded as retriable
    // (the sign-out itself succeeded).
    if (options?.deleteArchive) {
      // Purge, and on failure surface a toast with an ACTIONABLE Retry: the purge
      // is keyed by `accountId` and needs no live session, so it can be retried
      // even after the account row is gone. Retrying re-offers itself on repeat
      // failure, so the "retry" promise is always fulfillable.
      const purge = async (): Promise<void> => {
        try {
          await deleteAccountArchive(accountId);
        } catch {
          toast.error("Signed out, but this account's archive could not be deleted.", {
            action: {
              label: "Retry",
              onClick: () => {
                void purge();
              },
            },
          });
        }
      };
      await purge();
    }
  }, []);
}
