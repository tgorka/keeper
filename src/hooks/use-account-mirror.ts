/**
 * Keep the account mirror live for the app's lifetime (Epic 82).
 *
 * Three things, all at the root because none of them belongs to a view:
 *
 * - **The subscription.** `account_subscribe` pushes a snapshot and then every
 *   change, so Settings, the status line and the credential choices all read
 *   one store instead of each asking Rust.
 * - **Setup links.** A `keeper://setup?…` link can arrive while any view — or
 *   the first-run wizard, or the login screen — is on screen, so the listener
 *   lives here and opens the one confirmation sheet `App` mounts.
 * - **Return-triggered sync.** Rust runs no timer (AD-62), so returning to
 *   keeper is the cadence: every window focus, and every time the page
 *   becomes visible again, asks for an unforced sync, which Rust throttles to
 *   one per 15 minutes. Both, because the phone tier's WKWebView reports an
 *   app coming back to the foreground as `visibilitychange` (every other
 *   return-to-app hook here listens to it) while a desktop window regaining
 *   focus without having been hidden reports only `focus`. Only while an
 *   account is signed in — an install without one sends nothing anywhere
 *   (the epic's first rule).
 */
import { useEffect } from "react";
import {
  accountSubscribe,
  accountSync,
  accountUnsubscribe,
  listenAccountSetup,
} from "@/lib/ipc/client";
import { accountStore } from "@/lib/stores/account";

export function useAccountMirror(): void {
  useEffect(() => {
    let cancelled = false;
    let subscriptionId: string | null = null;
    let unlisten: (() => void) | undefined;
    const { setVm, openSetup } = accountStore.getState();

    // The listener is registered BEFORE the subscription is opened, and the
    // order is load-bearing: the shell holds a setup link that started keeper
    // and emits it from inside `account_subscribe`, so a listener that is not
    // yet registered by then would drop the very link keeper was opened with.
    void (async () => {
      try {
        const fn = await listenAccountSetup(openSetup);
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
      } catch {
        // No Tauri host: no deep links either. The subscription below still runs.
      }
      try {
        const id = await accountSubscribe(setVm);
        if (cancelled) {
          void accountUnsubscribe(id).catch(() => {});
        } else {
          subscriptionId = id;
        }
      } catch {
        // No Tauri host, or no account commands in this build: the store
        // keeps its "no account" default, which is the honest answer.
      }
    })();

    const onReturn = () => {
      const { vm } = accountStore.getState();
      if (!vm.configured || vm.identity === null) {
        return;
      }
      void accountSync(false)
        .then(setVm)
        .catch(() => {
          // Rust's own sentence arrives over the subscription; a failed poke
          // is not a second thing to report.
        });
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") {
        onReturn();
      }
    };
    window.addEventListener("focus", onReturn);
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      cancelled = true;
      window.removeEventListener("focus", onReturn);
      document.removeEventListener("visibilitychange", onVisibility);
      unlisten?.();
      if (subscriptionId !== null) {
        void accountUnsubscribe(subscriptionId).catch(() => {});
      }
    };
  }, []);
}
