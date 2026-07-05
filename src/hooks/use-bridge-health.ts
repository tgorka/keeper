/**
 * Bridge-session health subscription lifecycle (Story 6.5, FR-28, AD-8).
 *
 * The SINGLE bridge-health subscriber: one subscription across every active account
 * (the backend aggregates all accounts into one stream), mirroring each streamed
 * {@link BridgeHealthSnapshot} into the {@link bridgeHealthStore}. The card dot + state
 * word, the sidebar roll-up, the affected chat-row dot, and the in-conversation re-link
 * banner are pure projections of that store.
 *
 * The effect re-runs when the signed-in account set changes (an add / sign-out) so the
 * backend re-bootstraps the monitored sessions across exactly the live accounts. On
 * cleanup — StrictMode double-mount, account-set change, or unmount — the subscription
 * is torn down and the store cleared, so streams never leak and no stale health
 * lingers. The sink is gated so a late snapshot after cleanup never mutates the store,
 * and a subscribe failure is swallowed (health is a non-critical projection).
 */
import { useEffect } from "react";
import type { BridgeHealthSnapshot } from "@/lib/ipc/client";
import { subscribeBridgeHealth, unsubscribeBridgeHealth } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";

export function useBridgeHealthSubscription(): void {
  // Key on the sorted account-id set: an add / sign-out re-bootstraps the monitored
  // sessions across exactly the live accounts.
  const accountKey = useAccountsStore((s) =>
    s.accounts
      .map((a) => a.accountId)
      .sort()
      .join(","),
  );

  useEffect(() => {
    if (accountKey.length === 0) {
      // No signed-in accounts: clear any stale health and skip subscribing.
      bridgeHealthStore.getState().reset();
      return;
    }

    let cancelled = false;
    let subscriptionId: number | null = null;

    const onSnapshot = (snapshot: BridgeHealthSnapshot) => {
      if (!cancelled) {
        bridgeHealthStore.getState().applySnapshot(snapshot);
      }
    };

    subscribeBridgeHealth(onSnapshot)
      .then((id) => {
        if (cancelled) {
          void unsubscribeBridgeHealth(id);
          return;
        }
        subscriptionId = id;
      })
      .catch(() => {
        // A failed health stream is non-fatal: leave the store empty (no health).
      });

    return () => {
      cancelled = true;
      if (subscriptionId !== null) {
        void unsubscribeBridgeHealth(subscriptionId);
      }
      bridgeHealthStore.getState().reset();
    };
  }, [accountKey]);
}
