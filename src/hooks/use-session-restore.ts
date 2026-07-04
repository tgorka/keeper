/**
 * Boot session-restore hook (FR-8, Story 1.8).
 *
 * On mount — once, before any shell renders — it calls the Rust-authoritative
 * `session_restore` command. If a persisted account is returned it hydrates the
 * accounts store (`setCurrentAccount`), so `App` boots straight into the shell
 * behind a no-flash splash; the existing lazy room-list subscribe then restores
 * the SDK session and renders cached chats before the network settles. It
 * **always** marks the store hydrated — on success, on a `null` (cold install /
 * missing session), and in the error path — so a failed or empty restore falls
 * through to the login screen rather than hanging on the splash forever.
 */
import { useEffect } from "react";
import { sessionRestore } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

export function useSessionRestore(): void {
  useEffect(() => {
    let cancelled = false;
    sessionRestore()
      .then((account) => {
        if (cancelled) {
          return;
        }
        if (account) {
          accountsStore.getState().setCurrentAccount(account);
        }
      })
      .catch(() => {
        // A failed restore is fail-safe: fall through to the login screen.
      })
      .finally(() => {
        if (!cancelled) {
          // Always open the gate — success, null, or error — so the splash is
          // never held indefinitely.
          accountsStore.getState().markHydrated();
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);
}
