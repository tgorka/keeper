/**
 * Webcam selection store (Story 20.1, FR-70, AD-36).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-mic.ts` precedent) holding the ephemeral webcam selection for the
 * next Recording Session: whether the webcam source is enabled (default
 * **off** — off-by-default is what makes the lazy-permission contract true:
 * camera permission is requested only when the user enables the source, never
 * preemptively) and which camera to use (`null` = the system default camera,
 * the picker's default). Never persisted to `keeper.db` and never mirrored
 * into Settings → Recording (ephemeral per-session, like the mic). The header
 * Start click reads both values imperatively and threads them through
 * `recording_start` as the new `camera_enabled`/`camera_device_id` params —
 * the sidecar then records `camera-####.mp4` as its own separate file, synced
 * to the screen.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RecordingSourcesVm } from "@/lib/ipc/client";

export interface RecordingWebcamState {
  /** Whether the next session records the webcam (default off). */
  webcamEnabled: boolean;
  /** The selected camera's id, or `null` for the system default camera. */
  cameraDeviceId: string | null;
  /** Set the webcam toggle. */
  setWebcamEnabled: (enabled: boolean) => void;
  /** Set the camera selection (`null` = system default camera). */
  setCameraDeviceId: (deviceId: string | null) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const recordingWebcamStore = createStore<RecordingWebcamState>()((set) => ({
  webcamEnabled: false,
  cameraDeviceId: null,
  setWebcamEnabled: (enabled) => set({ webcamEnabled: enabled }),
  setCameraDeviceId: (deviceId) => set({ cameraDeviceId: deviceId }),
}));

/** React selector hook: whether the webcam source is currently enabled. */
export function useWebcamEnabled(): boolean {
  return useStore(recordingWebcamStore, (state) => state.webcamEnabled);
}

/** React selector hook: the selected camera id (`null` = system default camera). */
export function useCameraDeviceId(): string | null {
  return useStore(recordingWebcamStore, (state) => state.cameraDeviceId);
}

/** Read the current webcam toggle imperatively (for the header Start click). */
export function webcamEnabled(): boolean {
  return recordingWebcamStore.getState().webcamEnabled;
}

/** Read the current camera selection imperatively (for the header Start click). */
export function cameraDeviceId(): string | null {
  return recordingWebcamStore.getState().cameraDeviceId;
}

/** Set the webcam toggle (bound to the Webcam card's `Switch`). */
export function setWebcamEnabled(enabled: boolean): void {
  recordingWebcamStore.getState().setWebcamEnabled(enabled);
}

/** Set the camera selection (bound to the Webcam card's device `Select`). */
export function setCameraDeviceId(deviceId: string | null): void {
  recordingWebcamStore.getState().setCameraDeviceId(deviceId);
}

/**
 * Whether the camera selection still exists in the live enumeration (Story
 * 20.1) — mirrors `recording-mic.ts::isMicSelectionAvailable`. `null` sources
 * (never polled) is "not yet known" → available (never a spurious reset before
 * the first enumeration lands); `null` deviceId (System default camera) is
 * always available; a real id is available only while it is still enumerated
 * in `sources.cameras`.
 */
export function isCameraSelectionAvailable(
  deviceId: string | null,
  sources: RecordingSourcesVm | null,
): boolean {
  if (sources === null || deviceId === null) {
    return true;
  }
  return sources.cameras.some((camera) => camera.id === deviceId);
}

/** Test-only reset: restore the default-off toggle + default camera. */
export function resetRecordingWebcamForTest(): void {
  recordingWebcamStore.setState({ webcamEnabled: false, cameraDeviceId: null });
}
