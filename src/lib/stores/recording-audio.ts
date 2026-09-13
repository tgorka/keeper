/**
 * System-audio toggle store (Story 19.2, FR-69; spec *Recording remembers which
 * sources are on*).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-source.ts` precedent) holding whether the next Recording Session
 * captures system audio. Default **on**, as it always was — what changed on
 * 2026-09-13 is that the answer is now persisted (`recording.system_audio`):
 * `recording-settings.ts` seeds this store from Rust once per launch and every
 * toggle writes through, so turning system audio off survives a relaunch
 * instead of being forgotten by it. The compile-time default here equals the
 * Rust default. The header Start click reads the current value imperatively and
 * threads it through `recording_start` as the `system_audio` param.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

export interface RecordingAudioState {
  /** Whether the next session captures system audio (persisted, default on). */
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
