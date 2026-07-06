/**
 * Global summon-hotkey listener (Story 9.4, FR-50).
 *
 * The OS-global hotkey is registered in Rust (`tauri-plugin-global-shortcut`); on a
 * raise it emits `keeper://global-hotkey-activated`. This hook listens for that event
 * and, exactly as the Acceptance Criteria require, switches the shell to the Unified
 * Inbox view and requests keyboard focus in the chat list — reusing Story 9.2's roving
 * focus via the {@link chatListFocusStore} nonce (never re-deriving list ordering).
 *
 * Registering the listener is best-effort and graceful outside a Tauri webview (jsdom
 * in tests, or a future non-desktop port), mirroring {@link useMenuActions}: a failure
 * just means the hotkey bridge is inert — it never crashes the shell.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { chatListFocusStore } from "@/lib/stores/chat-list-focus";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** The event the Rust hotkey handler emits when it raises the main window. */
export const GLOBAL_HOTKEY_EVENT = "keeper://global-hotkey-activated";

export function useGlobalHotkey(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listen(GLOBAL_HOTKEY_EVENT, () => {
        // Switch to the Unified Inbox, then ask the chat list to move keyboard focus
        // to its first visible row (Story 9.2 roving focus via the nonce store).
        primaryViewStore.getState().setView("inbox");
        chatListFocusStore.getState().requestFocus();
      })
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — the hotkey bridge is inert in this environment.
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
