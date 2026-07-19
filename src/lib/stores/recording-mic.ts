/**
 * Microphone selection store (Story 19.3, FR-69, AD-36).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-audio.ts` precedent) holding the ephemeral mic selection for the
 * next Recording Session: whether the mic source is enabled (default **off** —
 * off-by-default is what makes the lazy-permission contract true: microphone
 * permission is requested only when the user enables the source, never
 * preemptively) and which input device to use (`null` = the system default
 * input, the picker's default). Never persisted to `keeper.db` and never
 * mirrored into Settings → Recording — DB persistence + Settings mirroring are
 * reserved for segmentation (17.5) and folder/fps (19.5). The header Start
 * click reads both values imperatively and threads them through
 * `recording_start` as the new `microphone_enabled`/`microphone_device_id`
 * params.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RecordingSourcesVm } from "@/lib/ipc/client";

export interface RecordingMicState {
  /** Whether the next session captures the microphone (default off). */
  micEnabled: boolean;
  /** The selected input device's id, or `null` for the system default input. */
  micDeviceId: string | null;
  /** Set the mic toggle. */
  setMicEnabled: (enabled: boolean) => void;
  /** Set the mic device selection (`null` = system default input). */
  setMicDeviceId: (deviceId: string | null) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const recordingMicStore = createStore<RecordingMicState>()((set) => ({
  micEnabled: false,
  micDeviceId: null,
  setMicEnabled: (enabled) => set({ micEnabled: enabled }),
  setMicDeviceId: (deviceId) => set({ micDeviceId: deviceId }),
}));

/** React selector hook: whether the mic source is currently enabled. */
export function useMicEnabled(): boolean {
  return useStore(recordingMicStore, (state) => state.micEnabled);
}

/** React selector hook: the selected device id (`null` = system default input). */
export function useMicDeviceId(): string | null {
  return useStore(recordingMicStore, (state) => state.micDeviceId);
}

/** Read the current mic toggle imperatively (for the header Start click). */
export function micEnabled(): boolean {
  return recordingMicStore.getState().micEnabled;
}

/** Read the current device selection imperatively (for the header Start click). */
export function micDeviceId(): string | null {
  return recordingMicStore.getState().micDeviceId;
}

/** Set the mic toggle (bound to the Audio card's mic `Switch`). */
export function setMicEnabled(enabled: boolean): void {
  recordingMicStore.getState().setMicEnabled(enabled);
}

/** Set the device selection (bound to the Audio card's device `Select`). */
export function setMicDeviceId(deviceId: string | null): void {
  recordingMicStore.getState().setMicDeviceId(deviceId);
}

/**
 * Whether the mic device selection still exists in the live enumeration (Story
 * 19.4) — mirrors `recording-source.ts::isSelectionAvailable`. `null` sources
 * (never polled) is "not yet known" → available (never a spurious reset before
 * the first enumeration lands); `null` deviceId (System default input) is
 * always available; a real id is available only while it is still enumerated
 * in `sources.microphones`.
 */
export function isMicSelectionAvailable(
  deviceId: string | null,
  sources: RecordingSourcesVm | null,
): boolean {
  if (sources === null || deviceId === null) {
    return true;
  }
  return sources.microphones.some((mic) => mic.id === deviceId);
}

/** Test-only reset: restore the default-off toggle + default input. */
export function resetRecordingMicForTest(): void {
  recordingMicStore.setState({ micEnabled: false, micDeviceId: null });
}
