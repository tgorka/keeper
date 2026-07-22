/**
 * Source-picker mirror store (Story 19.1, FR — application/window picker).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-settings.ts` / `incognito.ts` precedent). Two kinds of state live
 * here with deliberately different ownership:
 *
 * - `sources` mirrors the Rust-resolved {@link RecordingSourcesVm} — it is NOT
 *   the source of truth. The Rust `recording_list_sources` command enumerates
 *   the live displays + applications; this store only holds the last snapshot
 *   the picker polled (~3s while the idle setup surface is visible + on focus).
 * - `selected` is ephemeral UI selection ({@link RecordingTargetVm}) — the one
 *   capture target the header Start passes to `recording_start`. It defaults to
 *   the main display (preserving today's full-main-display behavior) and is
 *   never persisted.
 *
 * The list is live: {@link startRecordingSourcePolling} runs a fixed-interval
 * re-enumeration (with a `refreshing` affordance during an in-flight fetch) and
 * {@link stopRecordingSourcePolling} halts it while a session is recording or the
 * surface unmounts. A vanished selection (the polled list no longer contains the
 * selected app) is *marked*, not silently swapped — Start against it yields the
 * sidecar's clean `Failed` (the pid is re-resolved live at Start).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  listRecordingSources,
  type RecordingSourcesVm,
  type RecordingTargetVm,
} from "@/lib/ipc/client";

/** How often the picker re-enumerates while the idle setup surface is visible. */
export const RECORDING_SOURCE_POLL_MS = 3000;

/** The default capture target: the main display (`displayId: null`). Preserves
 * the pre-19.1 full-main-display behavior when the user changes nothing. */
export const DEFAULT_RECORDING_TARGET: RecordingTargetVm = Object.freeze({
  kind: "display",
  displayId: null,
});

/** Whether two targets address the same source. App targets match on both `pid`
 * AND `bundleId` — the OS recycles a pid to a different app within seconds, so
 * `pid` alone could mark a recycled pid as the same app (and record the wrong
 * one). */
export function isSameTarget(a: RecordingTargetVm, b: RecordingTargetVm): boolean {
  if (a.kind === "display" && b.kind === "display") {
    return a.displayId === b.displayId;
  }
  if (a.kind === "application" && b.kind === "application") {
    return a.pid === b.pid && a.bundleId === b.bundleId;
  }
  // Story 21.3: the audio-only target has no parameters — kind equality is it.
  return a.kind === "audioOnly" && b.kind === "audioOnly";
}

/**
 * Whether `selected` still exists in `sources` (Story 19.1). A display target is
 * present when its id (or the main-display default) is enumerated; an app target
 * is present when its pid is still enumerated. `null` sources (never polled) is
 * treated as "not yet known" → available (never a spurious unavailable flag
 * before the first enumeration lands).
 */
export function isSelectionAvailable(
  selected: RecordingTargetVm,
  sources: RecordingSourcesVm | null,
): boolean {
  if (sources === null) {
    return true;
  }
  // Story 21.3: audio-only never depends on the enumerated video sources.
  if (selected.kind === "audioOnly") {
    return true;
  }
  if (selected.kind === "display") {
    // The main-display default (`null`) is available whenever any display is.
    if (selected.displayId === null) {
      return sources.displays.length > 0;
    }
    return sources.displays.some((display) => display.id === selected.displayId);
  }
  // Match on both pid and bundleId: a recycled pid belonging to a different app
  // must NOT read back as the same (still-available) selection.
  return sources.applications.some(
    (app) => app.pid === selected.pid && app.bundleId === selected.bundleId,
  );
}

export interface RecordingSourceState {
  /** The last-polled source list, or `null` before the first enumeration. */
  sources: RecordingSourcesVm | null;
  /** The selected capture target (defaults to the main display). */
  selected: RecordingTargetVm;
  /** Whether an enumeration is currently in flight (the "refreshing…" affordance). */
  refreshing: boolean;
  /** Replace the mirrored source list (from a poll). */
  setSources: (sources: RecordingSourcesVm) => void;
  /** Set the in-flight enumeration flag. */
  setRefreshing: (refreshing: boolean) => void;
  /** Select a capture target (radio semantics — exactly one). */
  select: (target: RecordingTargetVm) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const recordingSourceStore = createStore<RecordingSourceState>()((set) => ({
  sources: null,
  selected: DEFAULT_RECORDING_TARGET,
  refreshing: false,
  setSources: (sources) => set({ sources }),
  setRefreshing: (refreshing) => set({ refreshing }),
  select: (target) => set({ selected: target }),
}));

/** In-flight enumeration, deduped so a focus refresh never races a poll tick. */
let inFlight: Promise<void> | null = null;

/**
 * Run one enumeration: flip `refreshing`, read `recording_list_sources`, replace
 * the mirror. Best-effort — a failure leaves the prior list rendered (a transient
 * enumeration failure never blanks the picker) and clears `refreshing`. Deduped:
 * a call while one is in flight joins it rather than issuing a second read.
 */
export async function refreshRecordingSources(): Promise<void> {
  if (inFlight !== null) {
    return inFlight;
  }
  recordingSourceStore.getState().setRefreshing(true);
  inFlight = listRecordingSources()
    .then((vm) => {
      recordingSourceStore.getState().setSources(vm);
    })
    .catch(() => {
      // Keep the prior snapshot; the next poll/focus retries.
    })
    .finally(() => {
      recordingSourceStore.getState().setRefreshing(false);
      inFlight = null;
    });
  return inFlight;
}

/** The active poll timer, or `null` when polling is stopped. */
let pollTimer: ReturnType<typeof setInterval> | null = null;

/**
 * Start the live source poll (Story 19.1): an immediate enumeration, then a
 * fixed-interval re-enumeration while the idle setup surface is visible. Idempotent
 * — a second start does not stack timers. Callers also re-enumerate on window
 * focus (a focus refresh is just a {@link refreshRecordingSources} call).
 */
export function startRecordingSourcePolling(): void {
  void refreshRecordingSources();
  if (pollTimer !== null) {
    return;
  }
  pollTimer = setInterval(() => {
    void refreshRecordingSources();
  }, RECORDING_SOURCE_POLL_MS);
}

/**
 * Stop the live source poll (Story 19.1): halts re-enumeration while a session is
 * recording or the surface is unmounted. Idempotent.
 */
export function stopRecordingSourcePolling(): void {
  if (pollTimer !== null) {
    clearInterval(pollTimer);
    pollTimer = null;
  }
}

/** React selector hook: the mirrored source list (or `null` before the first poll). */
export function useRecordingSources(): RecordingSourcesVm | null {
  return useStore(recordingSourceStore, (state) => state.sources);
}

/** React selector hook: the selected capture target. */
export function useSelectedRecordingTarget(): RecordingTargetVm {
  return useStore(recordingSourceStore, (state) => state.selected);
}

/** React selector hook: whether an enumeration is in flight (refreshing affordance). */
export function useRecordingSourcesRefreshing(): boolean {
  return useStore(recordingSourceStore, (state) => state.refreshing);
}

/** Read the current selected target imperatively (for the header Start click). */
export function selectedRecordingTarget(): RecordingTargetVm {
  return recordingSourceStore.getState().selected;
}

/** Select a capture target (radio semantics). */
export function selectRecordingTarget(target: RecordingTargetVm): void {
  recordingSourceStore.getState().select(target);
}

/** Test-only reset: clear the mirror, restore the default selection, stop polling. */
export function resetRecordingSourceForTest(): void {
  stopRecordingSourcePolling();
  inFlight = null;
  recordingSourceStore.setState({
    sources: null,
    selected: DEFAULT_RECORDING_TARGET,
    refreshing: false,
  });
}
