/**
 * Approval Pane keyboard shortcut (Story 7.3).
 *
 * Wires `⌘3` (Ctrl+3 for non-mac parity) to switch the shell to the Approval Pane
 * primary view, following the app's ad-hoc `window.addEventListener("keydown", …)`
 * shortcut pattern (mirrors {@link useBridgesShortcut}; there is no central
 * registry). `preventDefault` keeps the webview from acting on the chord.
 */
import { useEffect } from "react";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useApprovalShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key !== "3") {
        return;
      }
      // Don't hijack the chord while the user is typing in a field — swallowing it
      // would yank them to the Approval Pane mid-edit.
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
      primaryViewStore.getState().setView("approval");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
