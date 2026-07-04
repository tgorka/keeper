/**
 * Connection-status subscription lifecycle (FR-8/FR-9, UX-DR18, AD-8/AD-9).
 *
 * Reads the current account id from the accounts store and, on mount / account
 * change, subscribes to the account's connection-status channel, mirroring each
 * streamed batch into the {@link connectionStore}. On cleanup — StrictMode
 * double-mount, account change, unmount, or account clear — it unsubscribes the
 * backend task and `reset()`s the store, so streams never leak and no stale
 * offline status lingers across accounts. Mounted once by the always-mounted
 * signed-in root (`AppShell`). A subscribe failure is swallowed: the pill is a
 * non-critical projection, so a failed connection stream simply leaves the store
 * at its `"online"` default rather than surfacing an error.
 */
import { useEffect } from "react";
import type { ConnectionStatusBatch } from "@/lib/ipc/client";
import { subscribeConnectionStatus, unsubscribeConnectionStatus } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { connectionStore } from "@/lib/stores/connection";

export function useConnectionStatus(): void {
  const accountId = useAccountsStore((s) => s.currentAccount?.accountId ?? null);

  useEffect(() => {
    if (accountId === null) {
      // No account: keep the store at its default so the pill never shows.
      connectionStore.getState().reset();
      return;
    }

    // Establish clean state at mount so the newest mount always wins; resetting
    // in cleanup instead would race the next account's mount.
    connectionStore.getState().reset();
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
    // batches never mutate the store).
    const onBatch = (b: ConnectionStatusBatch) => {
      if (!cancelled) {
        connectionStore.getState().applyBatch(b);
      }
    };
    subscribeConnectionStatus(accountId, onBatch)
      .then((id) => {
        if (cancelled) {
          // Unmounted / account changed before the id resolved — tear down now.
          void unsubscribeConnectionStatus(accountId, id);
          return;
        }
        subscriptionId = id;
      })
      .catch(() => {
        // A failed connection stream is non-fatal: leave the store at "online".
      });

    return () => {
      cancelled = true;
      if (subscriptionId !== null) {
        void unsubscribeConnectionStatus(accountId, subscriptionId);
      }
      connectionStore.getState().reset();
    };
  }, [accountId]);
}
