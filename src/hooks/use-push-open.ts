/**
 * Open the session room a tapped keeper push names (Epic 98, story 98.1, UX-DR144).
 *
 * The iPhone's shell keeps the room a tapped notification opens and emits
 * `push://open`; which room is Rust's (`keeper_core::agents::push::tap_target`). This
 * hook takes it once on mount — a tap that launched keeper fires before the webview
 * listens — and again on every event, and opens it in the Agents window, where a session
 * room lives. Off the iPhone nothing is ever waiting, so the take answers `null`.
 *
 * Registering is best-effort, as for `useNotifyNavigate`: outside a Tauri webview the
 * bridge is inert.
 */
import { useEffect } from "react";
import { logFailure, sayFailure } from "@/lib/failure";
import { listenPushOpen, pushOpenTake } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

async function openWaiting(): Promise<void> {
  const target = await pushOpenTake();
  if (target === null) {
    return;
  }
  primaryViewStore.getState().setView("agents");
  roomsStore.getState().selectRoom({ accountId: target.accountId, roomId: target.roomId });
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
