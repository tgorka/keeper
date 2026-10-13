/**
 * The tasks key surface (Epic 57, FR-351, FR-352): ⌘8 switches to the Tasks
 * view — Ctrl+8 on non-mac, matching ⌘1–⌘7.
 *
 * The `use-sessions-shortcut` shape, self-gated on the sessions capability read
 * from the Rust-authored mirror rather than from a platform sniff, typing
 * targets skipped, and IME composition ignored.
 *
 * **The gate is `sessions` on a desktop.** AD-137 gates tasks on
 * `sync && desktop`, and on a desktop `CapabilitiesVm.sessions` is computed
 * from that same condition. Since 98.2 `sessions` is also true on a phone with
 * a sessions zone, and the phone has no Tasks surface (DW-238) — so the chord
 * does nothing on the reduced tier, where a view the phone shell cannot show
 * would leave the view store wrong until the next tap.
 *
 * ⌘8 is the first free number: ⌘1–⌘6 are the views and ⌘7 is the sessions
 * board. The registry entry that puts `⌘8` on the ⌘K row, the ⌘? cheat sheet
 * and the native menu bar is `keeper-core/src/palette.rs`'s `tasks-view`; this
 * hook owns the binding, as every chip in that registry is display-only.
 */
import { useEffect } from "react";
import { capabilitiesStore, isReducedCapabilityPlatform } from "@/lib/stores/capabilities";
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
      const state = capabilitiesStore.getState();
      if (!state.capabilities.sessions || isReducedCapabilityPlatform(state)) {
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
