/**
 * All-account key-backup-status subscription lifecycle (Story 3.3, AD-8/AD-9).
 *
 * The SINGLE backup-status subscriber: it reads the set of signed-in account ids
 * and, on that set changing, subscribes every account's backup-status channel,
 * mirroring each streamed {@link BackupStatus} into the per-account
 * {@link keyBackupStore}. The Settings backup row is a pure projection of that
 * map.
 *
 * Mirrors {@link useEncryptionStatuses}: the effect keys on the sorted account-id
 * set so an add / sign-out re-runs it, subscribing exactly the live accounts. On
 * cleanup — StrictMode double-mount, account-set change, or unmount — every open
 * subscription is torn down and its account's entry removed from the store, so
 * streams never leak and no stale status lingers. Each per-account sink is gated
 * so a late batch after cleanup never mutates the store, and a subscribe failure
 * is swallowed (the row is a non-critical projection — a failed stream simply
 * leaves that account pending / "Checking…").
 */
import { useEffect } from "react";
import type { BackupStatus } from "@/lib/ipc/client";
import { subscribeBackupStatus, unsubscribeBackupStatus } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { keyBackupStore } from "@/lib/stores/key-backup";

export function useKeyBackupStatuses(): void {
  const accountKey = useAccountsStore((s) =>
    s.accounts
      .map((a) => a.accountId)
      .sort()
      .join(","),
  );

  useEffect(() => {
    if (accountKey.length === 0) {
      return;
    }
    const accountIds = accountKey.split(",");

    let cancelled = false;
    const subscriptionIds = new Map<string, number>();

    for (const accountId of accountIds) {
      // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
      // batches never mutate the store).
      const onStatus = (status: BackupStatus) => {
        if (!cancelled) {
          keyBackupStore.getState().setStatus(accountId, status);
        }
      };
      subscribeBackupStatus(accountId, onStatus)
        .then((id) => {
          if (cancelled) {
            void unsubscribeBackupStatus(accountId, id);
            return;
          }
          subscriptionIds.set(accountId, id);
        })
        .catch(() => {
          // A failed backup stream is non-fatal: leave the account pending.
        });
    }

    return () => {
      cancelled = true;
      for (const accountId of accountIds) {
        const id = subscriptionIds.get(accountId);
        if (id !== undefined) {
          void unsubscribeBackupStatus(accountId, id);
        }
        keyBackupStore.getState().removeAccount(accountId);
      }
    };
  }, [accountKey]);
}
