/**
 * The phone's quick-capture sheet open state (Epic 66, Story 66.4, AD-200).
 *
 * On the desktop quick capture is a window `notes_window` prewarms and the
 * hotkey shows; a phone has no second window, so the same page opens as a
 * bottom sheet in the phone stack. This is the sheet's open flag, a vanilla
 * store outside React on the `leadingDrawerStore` idiom, so the Inbox header's
 * button, the Notes level's button, the ⌘⌥K chord and the palette's
 * `notes-capture` all open one sheet without threading a prop through
 * `PhoneShell`. What the sheet holds — which note, whether it was written on —
 * is Rust's (`notes_capture_draft`), never state here.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface CaptureSheetState {
  /** Whether the capture sheet is open. */
  isOpen: boolean;
  /** Open the sheet; Rust resolves the page when its content mounts. */
  open: () => void;
  /** Close the sheet. The sheet itself saves before it calls this. */
  close: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const captureSheetStore = createStore<CaptureSheetState>()((set) => ({
  isOpen: false,
  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
}));

/** React selector hook over {@link captureSheetStore}. */
export function useCaptureSheetStore<T>(selector: (state: CaptureSheetState) => T): T {
  return useStore(captureSheetStore, selector);
}
