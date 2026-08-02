/**
 * The quick-capture buffer mirror (Epic 36, Story 36.4, FR-101, NFR-27, UX-DR35).
 *
 * Rust owns the buffer, not this store. `notes.capture_buffer` lives in the
 * registry's settings table, which is why the text survives a dismissal, a
 * `kill -9` and a reboot — a zustand store survives none of those. What lives
 * here is a mirror plus the debounce that keeps Rust current: every keystroke
 * arms a 300 ms timer, and the timer — never the keystroke — talks to IPC.
 *
 * Two rules shape the whole module and neither is negotiable:
 *
 * 1. **A keystroke never waits for IPC.** `setCaptureText` is synchronous; the
 *    save is fire-and-forget. Hydration is the same story in reverse: a
 *    character typed before the stored buffer arrives WINS, because the panel's
 *    entire promise is that the first keystroke lands (NFR-27).
 * 2. **Capture never swallows words.** A failed commit leaves the text exactly
 *    where it was and surfaces the reason; the panel stays open. The buffer is
 *    cleared by exactly one event — a write acknowledgement.
 *
 * The debounce timer is module-scoped rather than component-scoped on purpose:
 * the panel is hidden, not unmounted, and a pending save must not be lost by a
 * remount in the one case where React does tear it down.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type IpcError,
  type NoteRefVm,
  notesCaptureBuffer,
  notesCaptureBufferSave,
  notesCaptureHide,
} from "@/lib/ipc/client";

/** How long the panel waits after the last keystroke before telling Rust. */
export const CAPTURE_SAVE_DEBOUNCE_MS = 300;

/** Structural guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

export interface NotesCaptureState {
  /** The panel's text. Mirrors Rust's stored buffer, ahead of it by the debounce. */
  text: string;
  /** Whether Rust's stored buffer has been read into {@link text} yet. */
  hydrated: boolean;
  /**
   * The last commit failure — a vault folder that is gone, a read-only volume.
   * Non-null keeps the panel open with the text intact (UX-DR35's error branch).
   */
  error: string | null;
}

export const notesCaptureStore = createStore<NotesCaptureState>()(() => ({
  text: "",
  hydrated: false,
  error: null,
}));

/** Pending debounce timer; `undefined` once it has fired or been cleared. */
let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** Whether the mirrored text has changed since the last acknowledged save. */
let unsaved = false;

/** Push the current text at Rust. Never throws: a failed heartbeat is retried
 *  by the next keystroke, and the commit path flushes again before it hides. */
async function pushBuffer(): Promise<void> {
  const { text } = notesCaptureStore.getState();
  try {
    await notesCaptureBufferSave(text);
    unsaved = false;
  } catch {
    // Deliberately silent. This is a heartbeat, not the durability guarantee —
    // `commitCapture` flushes synchronously before it asks Rust to hide, and a
    // failure there is the one the user is shown.
  }
}

/**
 * Read Rust's stored buffer into the mirror. Called once per panel mount.
 *
 * Adopts the stored value only when nothing has been typed yet: a character
 * that beat the round trip is newer than anything on disk, and overwriting it
 * would be the exact failure NFR-27 exists to prevent.
 */
export async function hydrateCaptureBuffer(): Promise<void> {
  if (notesCaptureStore.getState().hydrated) {
    return;
  }
  try {
    const stored = await notesCaptureBuffer();
    notesCaptureStore.setState((state) => ({
      text: state.text === "" ? stored : state.text,
      hydrated: true,
    }));
  } catch {
    // A buffer we cannot read is an empty panel, never a blocked one.
    notesCaptureStore.setState({ hydrated: true });
  }
}

/** Adopt a keystroke and arm the debounced push. Synchronous by contract. */
export function setCaptureText(text: string): void {
  notesCaptureStore.setState({ text, error: null });
  unsaved = true;
  clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = undefined;
    void pushBuffer();
  }, CAPTURE_SAVE_DEBOUNCE_MS);
}

/** Cancel the debounce and push now. Awaited by the commit path. */
export async function flushCaptureBuffer(): Promise<void> {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  if (unsaved) {
    await pushBuffer();
  }
}

/**
 * Escape: commit the buffer into the vault and hide the panel.
 *
 * The flush comes first and is awaited, because `notes_capture_hide` writes
 * what Rust holds — the last 300 ms of typing would otherwise be the words the
 * user loses. Rust decides where the note lands and writes nothing at all for
 * an empty buffer; the panel asks no questions (UX-DR35).
 *
 * Resolves with the note Rust wrote, or null when there was nothing to write.
 * A rejection leaves the text untouched and the panel visible.
 */
export async function commitCapture(): Promise<NoteRefVm | null> {
  await flushCaptureBuffer();
  try {
    const written = await notesCaptureHide(true);
    // Cleared only after the ack — the confirmation is the file (NFR-30).
    notesCaptureStore.setState({ text: "", error: null });
    unsaved = false;
    return written;
  } catch (error) {
    notesCaptureStore.setState({
      error: isIpcError(error) ? error.message : String(error),
    });
    return null;
  }
}

/** React selector hook over {@link notesCaptureStore}. */
export function useNotesCaptureStore<T>(selector: (state: NotesCaptureState) => T): T {
  return useStore(notesCaptureStore, selector);
}

/** Test-only reset: forget the mirrored buffer and any armed save. */
export function resetNotesCaptureStoreForTest(): void {
  clearTimeout(saveTimer);
  saveTimer = undefined;
  unsaved = false;
  notesCaptureStore.setState({ text: "", hydrated: false, error: null });
}
