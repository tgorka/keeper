/**
 * The tasks key surface (Epic 57, FR-351, FR-352): ⌘8 switches to the Tasks
 * view — Ctrl+8 on non-mac, matching ⌘1–⌘7.
 *
 * The `use-sessions-shortcut` shape, self-gated on the sessions capability read
 * from the Rust-authored mirror rather than from a platform sniff, typing
 * targets skipped, and IME composition ignored.
 *
 * **The capability is `sessions` and that is not a copy-paste slip.** AD-137
 * gates tasks on `sync && desktop`, which is the identical condition
 * `CapabilitiesVm.sessions` and `CapabilitiesVm.notes` are both computed from,
 * and `keeper_core::palette::registry_sections` takes one boolean for all three
 * categories for exactly that reason. Minting a twelfth flag would be a second
 * name for one fact until the day they diverge — and that day amends the Rust
 * signature, its two call sites and this line together.
 *
 * ⌘8 is the first free number: ⌘1–⌘6 are the views and ⌘7 is the sessions
 * board. The registry entry that puts `⌘8` on the ⌘K row, the ⌘? cheat sheet
 * and the native menu bar is `keeper-core/src/palette.rs`'s `tasks-view`; this
 * hook owns the binding, as every chip in that registry is display-only.
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

export function useTasksShortcut(): void {
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
      // ⌘8 must not answer for ⌘⌥8.
      if (event.altKey) {
        return;
      }
      if (event.key !== "8") {
        return;
      }
      // No dead chord where no task surface can exist (AD-27's no-dead-buttons
      // rule): with the capability off the event is left alone entirely, so the
      // webview's own handling of ⌘8 is not swallowed either.
      if (!capabilitiesStore.getState().capabilities.sessions) {
        return;
      }
      if (isTypingTarget(event.target)) {
        return;
      }
      event.preventDefault();
      primaryViewStore.getState().setView("tasks");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
