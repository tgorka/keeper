/**
 * The bots key surface (Epic 61, FR-378): ⌘9 switches to the Bots view —
 * Ctrl+9 on non-mac, matching ⌘1–⌘8.
 *
 * The `use-tasks-shortcut` shape, self-gated on a capability read from the
 * Rust-authored mirror rather than from a platform sniff, typing targets
 * skipped, and IME composition ignored.
 *
 * **The capability is `bots` and that is not a twelfth name for one fact.**
 * `use-tasks-shortcut` gates on `sessions` because AD-137 puts tasks on
 * `sync && desktop`, which is exactly what `CapabilitiesVm.sessions` and
 * `.notes` are both computed from — three surfaces over one `sync.db`. This one
 * is genuinely a different condition: a chat needs no `git` binary and no
 * `sync.db` at all, so gating it on `sessions` would hide a working surface on
 * every desktop whose `git` is older than the sync engine's floor. The half
 * that does need `sync` is the drive-tool grant (Stories 61.10, 61.11), and
 * that affordance reads `capabilities.sync` where it is offered.
 *
 * ⌘9 is the last free digit: ⌘1–⌘6 are the views, ⌘7 the sessions board and ⌘8
 * the Tasks view. `Sync`, `Files`, `Recordings` and `Settings` deliberately
 * bind no digit. The registry entry that would put `⌘9` on the ⌘K row, the ⌘?
 * cheat sheet and the native menu bar is `keeper-core/src/palette.rs`'s
 * business; this hook owns the binding, as every chip in that registry is
 * display-only.
 */
import { useEffect } from "react";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";

function isTypingTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  if (element === null) {
    return false;
  }
  return (
    element.isContentEditable ||
    element.tagName === "INPUT" ||
    element.tagName === "TEXTAREA" ||
    element.tagName === "SELECT"
  );
}

export function useBotsShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // A composing IME delivers the chord as part of a candidate selection;
      // acting on it would yank the user out of the text they are writing and
      // lose the composition (every other shortcut hook guards this first).
      if (event.isComposing) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        return;
      }
      // The Alt-bearing chords in this range belong to other verbs, and a bare
      // ⌘9 must not answer for ⌘⌥9.
      if (event.altKey) {
        return;
      }
      if (event.key !== "9") {
        return;
      }
      // No dead chord where no bots surface can exist (AD-27's no-dead-buttons
      // rule): with the capability off the event is left alone entirely, so the
      // webview's own handling of ⌘9 is not swallowed either.
      if (!capabilitiesStore.getState().capabilities.bots) {
        return;
      }
      if (isTypingTarget(event.target)) {
        return;
      }
      event.preventDefault();
      primaryViewStore.getState().setView("bots");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
