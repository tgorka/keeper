/**
 * Coarse notification-navigate listener (Story 10.4, Option B).
 *
 * The kept `tauri-plugin-notification` desktop backend has NO per-notification click
 * callback, so exact per-notification routing is impossible on this backend (deferred to
 * Epic 11). Instead, on app activation following a notification the Rust shell
 * summons+focuses the window and emits `notify://navigate` carrying the `NotifyTarget`
 * recorded at dispatch. This hook subscribes once and routes the target's KIND to a
 * **coarse** view:
 *
 * - `Message` → the Unified Inbox (`primaryViewStore.setView("inbox")`).
 * - `Bridge`  → the Bridges view (`primaryViewStore.setView("bridges")`), and records the
 *   `(accountId, networkId)` in the {@link bridgeRelinkStore} so the Bridges surface can
 *   route the user toward re-link (the persistent Story 6.5 surfaces do the exact routing).
 * - `None`    → no view switch (a plain summon+focus).
 *
 * This is deliberately coarse — it NEVER lands on the exact message. Exact Chat/Account/
 * message and exact re-login deep landing are deferred to Epic 11, so this hook must NOT
 * call `roomsStore.requestFocus`.
 *
 * Registering the listener is best-effort and graceful outside a Tauri webview (jsdom in
 * tests, or a future non-desktop port), mirroring {@link useGlobalHotkey}: a failure just
 * means the bridge is inert — it never crashes the shell.
 */
import { useEffect } from "react";
import { listenNotifyNavigate } from "@/lib/ipc/client";
import { bridgeRelinkStore } from "@/lib/stores/bridge-relink";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useNotifyNavigate(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listenNotifyNavigate((target) => {
        switch (target.kind) {
          case "message":
            // Coarse landing: switch to the Unified Inbox. Exact Chat/Account/message
            // landing is deferred to Epic 11 — no requestFocus here.
            primaryViewStore.getState().setView("inbox");
            break;
          case "bridge":
            // Coarse landing: switch to the Bridges view and record the target so the
            // Bridges surface can route the user toward re-link (Story 6.5 does the exact
            // routing). Exact re-login deep-landing is deferred to Epic 11.
            primaryViewStore.getState().setView("bridges");
            bridgeRelinkStore.getState().requestRelink({
              accountId: target.accountId,
              networkId: target.networkId,
            });
            break;
          case "none":
            // Nothing to land on — the window is already summoned+focused by the shell.
            break;
        }
      })
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — the navigate bridge is inert in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
