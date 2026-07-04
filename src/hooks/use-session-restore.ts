/**
 * Boot session-restore hook (FR-8, Story 1.8).
 *
 * On mount — once, before any shell renders — it calls the Rust-authoritative
 * `session_restore` command, which returns *every* restorable account (Story
 * 2.1). It hydrates the accounts store with all of them (`hydrateAll`), so `App`
 * boots straight into the shell behind a no-flash splash; the lazy merged-inbox
 * subscribe then restores each SDK session and renders cached chats before the
 * network settles. It **always** marks the store hydrated — on success, on an
 * empty array (cold install / missing sessions), and in the error path — so a
 * failed or empty restore falls through to the login screen rather than hanging
 * on the splash forever.
 */
import { useEffect } from "react";
import { sessionRestore } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";

export function useSessionRestore(): void {
  useEffect(() => {
    let cancelled = false;
    sessionRestore()
      .then((accounts) => {
        if (cancelled) {
          return;
        }
        if (accounts.length > 0) {
          accountsStore.getState().hydrateAll(accounts);
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
