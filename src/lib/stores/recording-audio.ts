/**
 * System-audio toggle store (Story 19.2, FR-69).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-source.ts` precedent) holding one piece of ephemeral UI state:
 * whether the next Recording Session captures system audio. It defaults to
 * `true` on load ("default on" — the epic's wording; never "remembered") and
 * is never persisted to `keeper.db` and never mirrored into Settings →
 * Recording — DB persistence + Settings mirroring are reserved for
 * segmentation (17.5) and folder/fps (19.5). The header Start click reads the
 * current value imperatively and threads it through `recording_start` as the
 * new `system_audio` param.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface RecordingAudioState {
  /** Whether the next session captures system audio (default on). */
  systemAudioEnabled: boolean;
  /** Set the system-audio toggle. */
  setSystemAudioEnabled: (enabled: boolean) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const recordingAudioStore = createStore<RecordingAudioState>()((set) => ({
  systemAudioEnabled: true,
  setSystemAudioEnabled: (enabled) => set({ systemAudioEnabled: enabled }),
}));

/** React selector hook: whether system audio is currently enabled. */
export function useSystemAudioEnabled(): boolean {
  return useStore(recordingAudioStore, (state) => state.systemAudioEnabled);
}

/** Read the current toggle imperatively (for the header Start click). */
export function systemAudioEnabled(): boolean {
  return recordingAudioStore.getState().systemAudioEnabled;
}

/** Set the system-audio toggle (bound to the Audio card's `Switch`). */
export function setSystemAudioEnabled(enabled: boolean): void {
  recordingAudioStore.getState().setSystemAudioEnabled(enabled);
}

/** Test-only reset: restore the default-on toggle. */
export function resetRecordingAudioForTest(): void {
  recordingAudioStore.setState({ systemAudioEnabled: true });
}
