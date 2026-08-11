/**
 * The set of open capture windows (Story 45.15, FR-191, FR-192).
 *
 * **Rust owns the windows; this mirrors them.** A capture window is a real
 * Tauri window with its own webview, so "how many are open and what does each
 * hold" is a fact about the process and not a fact any one document can know.
 * This store is the hydrate-and-write-back shape `notes-vaults.ts` established:
 * read the list, act through a command, re-read the answer. It never decides
 * anything — a store that kept its own idea of which windows exist would
 * disagree with the compositor the first time one was closed from the OS.
 *
 * **One store, and today exactly one kind of window reads it.** A capture
 * window finds *its own* row in the list, by its own key, and reads its lock
 * state out of it. A second "what am I?" command would be a second answer to
 * one question, and the two would disagree the moment a window closed while
 * another was reading.
 *
 * The **main window does not mirror this list at all**, and this doc claimed
 * for three epics that it did — "the main window renders the list, so 'open
 * this note as a capture window' can raise the one that is already there
 * instead of asking for a second". It does not: `hydrateCaptureWindows` and
 * `listenNotesCaptureWindows` are called from one place in the repository,
 * `capture-window.tsx`'s chrome effect, which only ever runs inside a capture
 * window's own document. So in the main window `windows` is `null` and
 * {@link captureWindowFor} answers `null` for every key.
 *
 * Story 48.3 left it that way on purpose rather than adding a main-window
 * subscription. Nothing needs it: raising rather than duplicating is
 * `notes_window::open`'s own property, decided from the window label, where it
 * cannot go stale — a mirror could only be used to change a LABEL, and a mirror
 * that misses a window closed from the OS would change it wrongly. The claim is
 * corrected here rather than deleted because a doc that described a consumer
 * nobody had written is most of why Story 45.15 read as finished.
 *
 * **Keys are Rust's.** Every row carries the key `keeper_core::capture` built;
 * a caller compares keys and never constructs one. `captureKey` exists in
 * `src/lib/capture-target.ts` for the one caller that has a target and no row
 * yet, and it is pinned to Rust's by a shared vector table.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type CaptureTargetVm,
  type CaptureWindowVm,
  notesCaptureClose,
  notesCaptureOpen,
  notesCaptureSetAlwaysOnTop,
  notesCaptureSetLocked,
  notesCaptureWindows,
} from "@/lib/ipc/client";

/**
 * Emitted by Rust whenever the set of capture windows changes. Ids only, per
 * the `keeper://kebab-case` convention — the listener asks for the list rather
 * than trusting a payload that was true when it was sent.
 */
export const CAPTURE_WINDOWS_EVENT = "keeper://notes-capture-windows";

export interface CaptureWindowsState {
  /**
   * The mirrored windows, or `null` before the first successful read.
   *
   * `null` is "keeper has not looked", never "none". A surface that offered
   * "open a capture window" while the answer was unknown would offer a second
   * window for a note that already has one, and the difference matters because
   * the remedy for "already open" is to raise it, not to make another.
   */
  windows: CaptureWindowVm[] | null;
}

export const captureWindowsStore = createStore<CaptureWindowsState>()(() => ({
  windows: null,
}));

/**
 * Re-read the list from Rust.
 *
 * Never throws: this runs on an event and on mount, and a surface that threw
 * because a window list could not be read would take down the document beside
 * it. A failed read leaves the last known list in place rather than blanking
 * it — a stale list is wrong about a window that closed, an empty one is wrong
 * about every window that is open.
 */
export async function hydrateCaptureWindows(): Promise<void> {
  try {
    captureWindowsStore.setState({ windows: await notesCaptureWindows() });
  } catch {
    // Left as it was, deliberately. See above.
  }
}

/**
 * The row for `key`, or null when no window holds it.
 *
 * Null while the list is unread as well as when the window is genuinely absent,
 * and the caller must not tell those apart from here: both mean "do not claim
 * this window is open".
 */
export function captureWindowFor(state: CaptureWindowsState, key: string): CaptureWindowVm | null {
  return state.windows?.find((window) => window.key === key) ?? null;
}

/**
 * Open — or raise — the capture window holding `target`, then re-read.
 *
 * The re-read is not belt and braces: Rust emits
 * {@link CAPTURE_WINDOWS_EVENT} to every window, but the window that asked is
 * the one whose UI is about to change, and waiting for its own event to come
 * back around would show a stale list for one frame.
 */
export async function openCaptureWindow(target: CaptureTargetVm): Promise<void> {
  await notesCaptureOpen(target);
  await hydrateCaptureWindows();
}

/** Close the capture window `key`, then re-read. */
export async function closeCaptureWindow(key: string): Promise<void> {
  await notesCaptureClose(key);
  await hydrateCaptureWindows();
}

/** Lock or unlock the capture window `key`, then re-read. */
export async function setCaptureWindowLocked(key: string, locked: boolean): Promise<void> {
  await notesCaptureSetLocked(key, locked);
  await hydrateCaptureWindows();
}

/**
 * Pin or un-pin the capture window `key`, then re-read (Story 48.4).
 *
 * The re-read is not optional bookkeeping: the chrome's pressed state comes
 * from the row, and the row reports what the WINDOW MANAGER did rather than
 * what was asked for. A compositor that refuses the request leaves the button
 * where it was, which is the truth.
 */
export async function setCaptureWindowAlwaysOnTop(
  key: string,
  alwaysOnTop: boolean,
): Promise<void> {
  await notesCaptureSetAlwaysOnTop(key, alwaysOnTop);
  await hydrateCaptureWindows();
}

/** React selector hook over {@link captureWindowsStore}. */
export function useCaptureWindowsStore<T>(selector: (state: CaptureWindowsState) => T): T {
  return useStore(captureWindowsStore, selector);
}

/** Test-only reset: forget every mirrored window. */
export function resetCaptureWindowsStoreForTest(): void {
  captureWindowsStore.setState({ windows: null });
}
