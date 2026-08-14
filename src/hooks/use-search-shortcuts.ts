/**
 * Search keyboard shortcuts (Story 5.4, FR-34, FR-267, UX-DR13).
 *
 * Wires the two entry points to the single search surface, following the app's
 * ad-hoc `window.addEventListener("keydown", …)` shortcut pattern (there is no
 * central registry, and this deliberately builds no ⌘K palette or ⌘? cheat sheet):
 * - `⌘⇧F` opens the surface **global** — messages, notes and sessions, chosen by
 *   which primary view you were in, switchable once open.
 * - `⌘F` opens it **in-chat** (scoped to the currently-selected Chat) — and is a
 *   no-op when no Chat is open. Ctrl is accepted alongside ⌘ for non-mac parity.
 *
 * Both branches used to `preventDefault` unconditionally, and `⌘F` still does
 * *when it acts*. What changed with FR-267 is that it can now decline: an open
 * editor binds `⌘F` to its own in-document find, and a listener on `window` is
 * always the last to see the event. So this checks `defaultPrevented` first and
 * stands down — the focused document wins the chord, and nothing here has to
 * know what a CodeMirror panel is. The webview's native find is still never
 * reached, because whoever did act called `preventDefault` before this ran.
 */
import { useEffect } from "react";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { type SearchSource, searchStore } from "@/lib/stores/search";

/**
 * Which content the global surface opens on, from where you pressed the chord.
 *
 * Searching starts from what you are looking at: `⌘⇧F` in Notes means "find it
 * in my notes" far more often than it means "find it in my messages". Every
 * source stays one click away, so the guess costs nothing when it is wrong.
 */
function sourceForView(): SearchSource {
  switch (primaryViewStore.getState().view) {
    case "notes":
      return "notes";
    case "sessions":
      return "sessions";
    default:
      return "messages";
  }
}

export function useSearchShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.key.toLowerCase() !== "f") {
        return;
      }
      // Already answered by something closer to the keystroke — the open
      // editor's own find. Not our chord to take.
      if (event.defaultPrevented) {
        return;
      }
      if (event.shiftKey) {
        // ⌘⇧F — global search, opened on the source you were already in.
        event.preventDefault();
        searchStore.getState().open("global", sourceForView());
        return;
      }
      // ⌘F — in-chat search, only when a Chat is open. Always preventDefault so the
      // webview's native find never triggers, even in the no-op case.
      event.preventDefault();
      if (roomsStore.getState().selected !== null) {
        searchStore.getState().open("chat", "messages");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
