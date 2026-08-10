/**
 * Open the note the shell just created (Story 44.6, FR-160, FR-102).
 *
 * The tray's **New Note** and **Today's Journal** create a note in Rust, raise
 * the main window, and emit `keeper://notes-open-note` carrying the new
 * {@link NoteRefVm}. Until this hook existed **nothing in the webview listened
 * to that event**: `listenNotesOpenNote` was declared in the IPC client and
 * never called from anywhere. So the tray created the note, the window came to
 * the front, and the user was shown whatever had been on screen before — with
 * the note they had just asked for sitting in the vault, unopened and unnamed.
 *
 * Nothing failed, which is why it survived two epics. That is the shape worth
 * naming: a dead value is not always an armed hazard, it is sometimes a promise
 * the app has been making and cannot keep, and the second kind is harder to see
 * because there is no error to find.
 *
 * Mounted at the app root rather than inside the notes view, and that is the
 * whole point of the feature: the tray exists so the app window is optional for
 * a whole day (FR-102), so the event routinely arrives while some other view is
 * on screen. A listener living in `NotesPane` would only work once the user had
 * already navigated to the place the tray was supposed to take them.
 *
 * Three steps, in this order:
 *
 *   1. Switch to the notes view. A note created behind whatever is on screen is
 *      a note the user has to go looking for — the same reason the palette's
 *      `notes-new` sets the view before it creates.
 *   2. Make the note's vault active, if it is not. The ref carries its own
 *      `vaultId` because the tray acts on the active vault in Rust, which can
 *      differ from the one the webview last showed.
 *   3. Select the note. Selection is stored WITH its vault, so doing this before
 *      the vault switch resolves is safe: pane 3 shows the note the moment its
 *      vault is the active one, which is exactly what "a vault switch is a
 *      filter" already means.
 *
 * Registering is best-effort and graceful outside a Tauri webview (jsdom in
 * tests, or a future non-desktop port), mirroring {@link useNotifyNavigate}: a
 * failure means the bridge is inert, never that the shell crashes.
 */
import { useEffect } from "react";
import { listenNotesOpenNote } from "@/lib/ipc/client";
import { notesListStore } from "@/lib/stores/notes-list";
import { notesVaultsStore, setActiveVault } from "@/lib/stores/notes-vaults";
import { primaryViewStore } from "@/lib/stores/primary-view";

export function useNotesOpenNote(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listenNotesOpenNote((ref) => {
        primaryViewStore.getState().setView("notes");
        if (notesVaultsStore.getState().activeVaultId !== ref.vaultId) {
          // Tells Rust first and mirrors the answer, so the webview never holds
          // an active vault the shell disagrees with. A rejection leaves the
          // previous vault active and the selection below simply does not
          // render — the note is still on disk and still in the list.
          void setActiveVault(ref.vaultId).catch(() => {});
        }
        notesListStore.getState().select(ref.vaultId, ref.id);
      })
        .then((fn) => {
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — the open-note bridge is inert in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
