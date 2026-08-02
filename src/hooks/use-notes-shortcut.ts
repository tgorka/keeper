/**
 * Notes keyboard shortcuts (Epic 37, FR-117, UX-DR42).
 *
 * `⌘6` for the view, and the `⌘⌥` cluster for the verbs. The collision analysis
 * is already settled in EXPERIENCE-NOTES and this hook implements exactly what
 * it decided — nothing here invents a binding:
 *
 *   - **`⌘6`** because numeric accelerators bind to registry actions and not to
 *     sidebar position, so `⌘5` already survives Recording being absent and `⌘6`
 *     is simply the first free number.
 *   - **`⌘⌥N` / `⌘⌥K` / `⌘⌥J` / `⌘⌥V`** because `⌘N` belongs to New Chat and is
 *     not being taken. `⌘⌥` is a modifier pair this app has never used, so every
 *     notes verb is new key space and the cluster is learnable as one thing.
 *
 * Every chord self-gates on the `notes` capability read from the Rust-authored
 * mirror (AD-27) — never a build flag, never a user-agent sniff — so there is no
 * dead `⌘6` on a platform where a vault cannot exist. And every chord is a
 * no-op while the user is typing, because swallowing `⌘⌥J` mid-sentence to open
 * a journal entry is worse than not having the key.
 *
 * The single-key list verbs (`j`, `k`, `e`, `u`, `p`) are NOT here: they are
 * list-scoped and belong to the focused list's own handler, exactly as the chat
 * list's are, so they can never fire while another surface has focus.
 */
import { useEffect } from "react";
import { createNote, openJournalToday, showCapture } from "@/hooks/use-notes-actions";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** Whether the event landed in a field, where a chord must not be hijacked. */
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

export function useNotesShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        return;
      }
      // No dead notes chord where no vault can exist — read the capability
      // mirror, which is the only platform truth the frontend is allowed.
      if (!capabilitiesStore.getState().capabilities.notes) {
        return;
      }
      if (isTypingTarget(event.target)) {
        return;
      }
      if (!event.altKey) {
        // ⌘6 — the view. Ctrl+6 is the non-mac twin, matching ⌘1–⌘5.
        if (event.key === "6") {
          event.preventDefault();
          primaryViewStore.getState().setView("notes");
        }
        return;
      }
      // The ⌘⌥ verb cluster. `event.key` under Alt is layout-dependent on some
      // platforms (macOS gives `˜` for ⌥N), so the chord is matched on `code`,
      // which is the physical key and stable across layouts.
      switch (event.code) {
        case "KeyN": {
          event.preventDefault();
          // New Note lands you in the editor, so the view has to come first —
          // otherwise the note is created behind whatever is on screen.
          primaryViewStore.getState().setView("notes");
          void createNote();
          break;
        }
        case "KeyK": {
          event.preventDefault();
          // The in-app twin of the global capture hotkey, and the reason the
          // global one is never a single point of failure: on a compositor that
          // hands out no global shortcuts, this still works.
          void showCapture();
          break;
        }
        case "KeyJ": {
          event.preventDefault();
          primaryViewStore.getState().setView("notes");
          void openJournalToday();
          break;
        }
        case "KeyV": {
          event.preventDefault();
          primaryViewStore.getState().setView("notes");
          notesVaultsStore.getState().requestSwitcherOpen();
          break;
        }
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
