/**
 * Open the agents session a tapped keeper push names (story 106.6; Epic 98, UX-DR144).
 *
 * The phone's shell keeps where a tapped notification opens and emits `push://open`;
 * where is Rust's (`keeper_core::agents::push::open_target`): an agents server session,
 * named by its database, or — for a notification of a Matrix agent room, until 108.2 —
 * that room. This hook takes it once on mount — a tap that launched keeper fires before
 * the webview listens — and again on every event, and opens it in the Agents window.
 * Off the phones nothing is ever waiting, so the take answers `null`.
 *
 * Registering is best-effort, as for `useNotifyNavigate`: outside a Tauri webview the
 * bridge is inert.
 */
import { useEffect } from "react";
import { logFailure, sayFailure } from "@/lib/failure";
import { listenPushOpen, pushOpenTake } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { primaryViewStore } from "@/lib/stores/primary-view";

async function openWaiting(): Promise<void> {
  const target = await pushOpenTake();
  if (target === null) {
    return;
  }
  primaryViewStore.getState().setView("agents");
  agentSessionsStore.getState().select({ database: target.database, sessionId: target.sessionId });
}

export function usePushOpen(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    const open = () => {
      // A tapped push that opens nothing is a tap the person has to hear about.
      openWaiting().catch(sayFailure("push.open", "keeper couldn't open the session."));
    };
    try {
      open();
      void listenPushOpen(open)
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        // No Tauri host — the bridge is inert in this environment.
        .catch(logFailure("push.listenOpen"));
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
