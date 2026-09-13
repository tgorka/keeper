/**
 * Webcam selection store (Story 20.1, FR-70, AD-36; spec *Recording remembers
 * which sources are on*).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-mic.ts` precedent) holding the webcam selection for the next
 * Recording Session: whether the webcam source is enabled and which camera to
 * use (`null` = the system default camera, the picker's default).
 *
 * Since 2026-09-13 the ENABLED half mirrors a persisted answer
 * (`recording.camera`, default **on**), seeded once per launch by
 * `recording-settings.ts` and written through on every toggle; the CAMERA half
 * stays per-session, for the same reason the mic's device does. Nothing is
 * requested from render: a camera that is on without its grant blocks Start and
 * names itself (Story 20.2). The header Start click reads both values
 * imperatively and threads them through `recording_start` — the sidecar then
 * records `camera-####.mp4` as its own separate file, synced to the screen.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RecordingSourcesVm } from "@/lib/ipc/client";

export interface RecordingWebcamState {
  /** Whether the next session records the webcam (persisted, default on). */
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
  webcamEnabled: true,
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

/** Test-only reset: restore the default-on toggle + default camera. */
export function resetRecordingWebcamForTest(): void {
  recordingWebcamStore.setState({ webcamEnabled: true, cameraDeviceId: null });
}
