/**
 * Command-palette keyboard shortcut (Story 9.1, epic 9 spine).
 *
 * Wires ⌘K (Ctrl+K for non-mac parity) to toggle the single always-mounted command
 * palette, following the app's ad-hoc `window.addEventListener("keydown", …)`
 * shortcut pattern (there is no central registry — mirrors
 * {@link import("./use-new-chat-shortcut").useNewChatShortcut}). `preventDefault` so
 * the webview never runs a native ⌘K binding.
 *
 * Unlike the other shortcut hooks this one deliberately does NOT guard against
 * text-edit fields: ⌘K is unambiguous and the palette is a global finder that must
 * open even from the composer. Toggling means a second ⌘K closes it (the palette
 * itself owns Esc). Opening it never stacks on another dialog — the palette is a
 * modal overlay that closes anything below it (modal depth ≤ 1).
 */
import { useEffect } from "react";
import { commandPaletteStore } from "@/lib/stores/command-palette";

export function useCommandPaletteShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Ignore keydowns mid-IME/dead-key composition so composing users never
      // mis-toggle the palette (the composition's committed key is what counts).
      if (event.isComposing) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key.toLowerCase() !== "k") {
        return;
      }
      event.preventDefault();
      commandPaletteStore.getState().toggle();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
