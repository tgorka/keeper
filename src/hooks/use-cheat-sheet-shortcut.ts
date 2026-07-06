/**
 * Cheat-sheet keyboard shortcut (Story 9.3, epic 9 spine).
 *
 * Wires ⌘? (Ctrl+? for non-mac parity) to toggle the single always-mounted cheat
 * sheet, following the app's ad-hoc `window.addEventListener("keydown", …)` shortcut
 * pattern (mirrors {@link import("./use-command-palette-shortcut").useCommandPaletteShortcut}).
 * `preventDefault` so the webview never runs a native binding.
 *
 * Like the palette shortcut this is a chord, so it deliberately does NOT guard
 * against text-edit fields: `?` alone types a character, but `⌘?`/`Ctrl+?` is
 * unambiguous and must open the reference from anywhere (including the composer).
 * The `?` glyph is a shifted key on most layouts; matching `event.key === "?"`
 * (already the shifted result) keeps this layout-correct without inspecting Shift.
 * Toggling means a second ⌘? closes it (the overlay itself owns Esc); it never
 * stacks — the cheat sheet is a single modal overlay (depth ≤ 1).
 */
import { useEffect } from "react";
import { cheatSheetStore } from "@/lib/stores/cheat-sheet";

export function useCheatSheetShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Ignore keydowns mid-IME/dead-key composition so composing users never
      // mis-toggle the cheat sheet (the composition's committed key is what counts).
      if (event.isComposing) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key !== "?") {
        return;
      }
      event.preventDefault();
      cheatSheetStore.getState().toggle();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
