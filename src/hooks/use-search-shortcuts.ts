/**
 * Search keyboard shortcuts (Story 5.4, FR-34, UX-DR13).
 *
 * Wires the two entry points to the single search surface, following the app's
 * ad-hoc `window.addEventListener("keydown", …)` shortcut pattern (there is no
 * central registry, and this deliberately builds no ⌘K palette or ⌘? cheat sheet):
 * - `⌘⇧F` opens the surface **global** (all accounts, no room lock).
 * - `⌘F` opens it **in-chat** (scoped to the currently-selected Chat) — and is a
 *   no-op when no Chat is open. Both `preventDefault` (⌘F is the webview's native
 *   find). Ctrl is accepted alongside ⌘ for non-mac parity.
 */
import { useEffect } from "react";
import { roomsStore } from "@/lib/stores/rooms";
import { searchStore } from "@/lib/stores/search";

export function useSearchShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key.toLowerCase() !== "f") {
        return;
      }
      if (event.shiftKey) {
        // ⌘⇧F — global search across every account.
        event.preventDefault();
        searchStore.getState().open("global");
        return;
      }
      // ⌘F — in-chat search, only when a Chat is open. Always preventDefault so the
      // webview's native find never triggers, even in the no-op case.
      event.preventDefault();
      if (roomsStore.getState().selected !== null) {
        searchStore.getState().open("chat");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
