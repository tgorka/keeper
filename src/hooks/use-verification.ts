/**
 * All-account device-verification subscription lifecycle (Story 3.2, FR-14,
 * AD-8).
 *
 * The SINGLE verification subscriber: it reads the set of signed-in account ids
 * and, on that set changing, subscribes every account's verification channel,
 * mirroring each streamed {@link VerificationFlowVm} snapshot into the
 * {@link verificationStore}. The device-verification modal is a pure projection of
 * that store.
 *
 * Crucially this is also how an *incoming* verification request (the other session
 * started it) reaches the UI: a `requested` snapshot arriving while no modal is
 * open auto-opens the modal for that account. A keeper-started flow already opened
 * the modal (via the Settings Verify button), so we never re-open on top of it.
 *
 * The effect keys on the sorted account-id set so an add / sign-out re-runs it,
 * subscribing exactly the live accounts. On cleanup — StrictMode double-mount,
 * account-set change, or unmount — every open subscription is torn down. Each
 * per-account sink is gated so a late batch after cleanup never mutates the store,
 * and a subscribe failure is swallowed (verification is a user-initiated flow — a
 * failed stream simply means no auto-open for that account).
 */
import { useEffect } from "react";
import type { VerificationFlowVm } from "@/lib/ipc/client";
import {
  subscribeVerification,
  unsubscribeVerification,
  verificationAccept,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { verificationStore } from "@/lib/stores/verification";

export function useVerification(): void {
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
      const onBatch = (flow: VerificationFlowVm) => {
        if (cancelled) {
          return;
        }
        const store = verificationStore.getState();
        // An incoming request (peer started it) auto-opens the modal when nothing
        // is open. A keeper-started flow already opened the modal, so we don't
        // re-open on top of it and reset its snapshot.
        if (flow.phase === "requested" && !store.modalOpen) {
          store.openFor(accountId);
          // The peer started this request, so keeper must accept it to move it
          // from `Requested` to `Ready` (a keeper-started request is accepted by
          // the *other* session instead). Without this the incoming direction
          // stalls in `Requested` forever — SAS can only start once ready.
          void verificationAccept(accountId, flow.flowId).catch(() => {
            // A failed accept surfaces as a terminal state via the stream; the
            // modal never traps the user on a dead "waiting" screen.
          });
        }
        // Only mirror the snapshot into the store for the account whose modal is
        // active, so a stray flow on a background account never drives the modal.
        if (verificationStore.getState().activeAccountId === accountId) {
          verificationStore.getState().setFlow(flow);
        }
      };
      subscribeVerification(accountId, onBatch)
        .then((id) => {
          if (cancelled) {
            void unsubscribeVerification(accountId, id);
            return;
          }
          subscriptionIds.set(accountId, id);
        })
        .catch(() => {
          // A failed verification stream is non-fatal: no auto-open for it.
        });
    }

    return () => {
      cancelled = true;
      for (const accountId of accountIds) {
        const id = subscriptionIds.get(accountId);
        if (id !== undefined) {
          void unsubscribeVerification(accountId, id);
        }
      }
    };
  }, [accountKey]);
}
