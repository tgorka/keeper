/**
 * Bridges keyboard shortcut (Story 6.1).
 *
 * Wires `⌘4` (Ctrl+4 for non-mac parity) to switch the shell to the Bridges
 * primary view, following the app's ad-hoc `window.addEventListener("keydown", …)`
 * shortcut pattern (mirrors {@link useSearchShortcuts}; there is no central
 * registry). `preventDefault` keeps the webview from acting on the chord.
 */
import { useEffect } from "react";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useBridgesShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key !== "4") {
        return;
      }
      // Don't hijack the chord while the user is typing in a field — swallowing it
      // would yank them to Bridges mid-edit.
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
      primaryViewStore.getState().setView("bridges");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
