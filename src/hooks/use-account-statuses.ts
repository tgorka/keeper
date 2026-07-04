/**
 * All-account connection-status subscription lifecycle (Story 2.5, UX-DR18,
 * AD-8/AD-9).
 *
 * The SINGLE connection-status subscriber: it reads the set of signed-in account
 * ids and, on that set changing, subscribes every account's connection-status
 * channel, mirroring each streamed batch into the per-account
 * {@link accountStatusStore}. The switcher's per-account sync glyph, the shell
 * offline pill, and the conversation "Queued" caption are pure projections of
 * that map.
 *
 * The effect keys on the sorted account-id set so an add / sign-out re-runs it,
 * subscribing exactly the live accounts. On cleanup — StrictMode double-mount,
 * account-set change, or unmount — every open subscription is torn down and its
 * account's entry removed from the store, so streams never leak and no stale
 * status lingers. Each per-account sink is gated so a late batch after cleanup
 * never mutates the store, and a subscribe failure is swallowed (the glyph/pill
 * are non-critical projections — a failed stream simply leaves that account
 * pending).
 */
import { useEffect } from "react";
import type { ConnectionStatusBatch } from "@/lib/ipc/client";
import { subscribeConnectionStatus, unsubscribeConnectionStatus } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { useAccountsStore } from "@/lib/stores/accounts";

export function useAccountStatuses(): void {
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
    // One resolved subscription id per account (null until it resolves).
    const subscriptionIds = new Map<string, number>();

    for (const accountId of accountIds) {
      // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
      // batches never mutate the store).
      const onBatch = (b: ConnectionStatusBatch) => {
        if (!cancelled) {
          accountStatusStore.getState().setStatus(accountId, b.status);
        }
      };
      subscribeConnectionStatus(accountId, onBatch)
        .then((id) => {
          if (cancelled) {
            // Torn down before the id resolved — unsubscribe immediately.
            void unsubscribeConnectionStatus(accountId, id);
            return;
          }
          subscriptionIds.set(accountId, id);
        })
        .catch(() => {
          // A failed connection stream is non-fatal: leave the account pending.
        });
    }

    return () => {
      cancelled = true;
      for (const accountId of accountIds) {
        const id = subscriptionIds.get(accountId);
        if (id !== undefined) {
          void unsubscribeConnectionStatus(accountId, id);
        }
        accountStatusStore.getState().removeAccount(accountId);
      }
    };
  }, [accountKey]);
}
