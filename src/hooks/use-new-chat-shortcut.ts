/**
 * New-chat keyboard shortcut (Story 6.6, FR-32).
 *
 * Wires ⌘N (Ctrl+N for non-mac parity) to open the single always-mounted new-chat
 * dialog, following the app's ad-hoc `window.addEventListener("keydown", …)`
 * shortcut pattern (there is no central registry — mirrors
 * {@link import("./use-search-shortcuts").useSearchShortcuts}). `preventDefault` so
 * the browser's native "new window" is never triggered.
 */
import { useEffect } from "react";
import { newChatStore } from "@/lib/stores/new-chat";

export function useNewChatShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key.toLowerCase() !== "n") {
        return;
      }
      // Don't hijack the chord while the user is typing in a field — swallowing it
      // would pop the dialog mid-edit and eat a native text-edit binding (e.g. the
      // emacs-style Ctrl+N caret move). Mirrors use-bridges-shortcut's guard.
      const target = event.target as HTMLElement | null;
      if (
        target !== null &&
        (target.isContentEditable ||
          target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT")
      ) {
        return;
      }
      event.preventDefault();
      newChatStore.getState().open();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
