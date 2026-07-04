/**
 * All-account encryption-status subscription lifecycle (Story 3.1, AD-8/AD-9).
 *
 * The SINGLE encryption-status subscriber: it reads the set of signed-in account
 * ids and, on that set changing, subscribes every account's encryption-status
 * channel, mirroring each streamed batch into the per-account
 * {@link encryptionStatusStore}. The verify banner, the Settings badge, and the
 * Settings Encryption section are pure projections of that map.
 *
 * The effect keys on the sorted account-id set so an add / sign-out re-runs it,
 * subscribing exactly the live accounts. On cleanup — StrictMode double-mount,
 * account-set change, or unmount — every open subscription is torn down and its
 * account's entry removed from the store, so streams never leak and no stale
 * status lingers. Each per-account sink is gated so a late batch after cleanup
 * never mutates the store, and a subscribe failure is swallowed (the banner /
 * badge are non-critical projections — a failed stream simply leaves that
 * account pending, so no banner shows).
 */
import { useEffect } from "react";
import type { EncryptionStatusBatch } from "@/lib/ipc/client";
import { subscribeEncryptionStatus, unsubscribeEncryptionStatus } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";

export function useEncryptionStatuses(): void {
  // Key on the sorted account-id set: an add / sign-out re-subscribes exactly
  // the live accounts.
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
    // One resolved subscription id per account (absent until it resolves).
    const subscriptionIds = new Map<string, number>();

    for (const accountId of accountIds) {
      // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
      // batches never mutate the store).
      const onBatch = (b: EncryptionStatusBatch) => {
        if (!cancelled) {
          encryptionStatusStore.getState().setStatus(accountId, b.status);
        }
      };
      subscribeEncryptionStatus(accountId, onBatch)
        .then((id) => {
          if (cancelled) {
            // Torn down before the id resolved — unsubscribe immediately.
            void unsubscribeEncryptionStatus(accountId, id);
            return;
          }
          subscriptionIds.set(accountId, id);
        })
        .catch(() => {
          // A failed encryption stream is non-fatal: leave the account pending.
        });
    }

    return () => {
      cancelled = true;
      for (const accountId of accountIds) {
        const id = subscriptionIds.get(accountId);
        if (id !== undefined) {
          void unsubscribeEncryptionStatus(accountId, id);
        }
        encryptionStatusStore.getState().removeAccount(accountId);
      }
    };
  }, [accountKey]);
}
