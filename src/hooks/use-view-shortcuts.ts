/**
 * Inbox / Archive view chords (Story 9.2).
 *
 * Wires `⌘1` → Inbox and `⌘2` → Archive (Ctrl+1/2 for non-mac parity) to switch
 * the shell's primary view, completing the ⌘1–4 set alongside the existing
 * ⌘3 (Approval) and ⌘4 (Bridges) hooks. Follows the app's ad-hoc
 * `window.addEventListener("keydown", …)` shortcut pattern (mirrors
 * {@link useApprovalShortcut}/{@link useBridgesShortcut}; there is no central
 * registry). Guarded off while typing in a text field so the chord never yanks the
 * user between views mid-edit, IME-guarded, and `preventDefault`s to keep the
 * webview from acting on the chord.
 */
import { useEffect } from "react";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useViewShortcuts(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      // Ignore IME composition keystrokes (mirrors the other shortcut hooks).
      if (event.isComposing) {
        return;
      }
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        return;
      }
      if (event.key !== "1" && event.key !== "2") {
        return;
      }
      // Don't hijack the chord while the user is typing in a field — swallowing it
      // would yank them to another view mid-edit.
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
      primaryViewStore.getState().setView(event.key === "1" ? "inbox" : "archive");
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
