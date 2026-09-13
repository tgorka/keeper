/**
 * Microphone selection store (Story 19.3, FR-69, AD-36; spec *Recording
 * remembers which sources are on*).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-audio.ts` precedent) holding the mic selection for the next
 * Recording Session: whether the mic source is enabled and which input device
 * to use (`null` = the system default input, the picker's default).
 *
 * Since 2026-09-13 the ENABLED half is a mirror of a persisted answer
 * (`recording.microphone`, default **on**): `recording-settings.ts` seeds it
 * from Rust once per launch and every toggle writes through, because a choice
 * thrown away by the next relaunch is the report this changed. The compile-time
 * default here equals the Rust default, so a start that happens before the
 * first read lands still captures what the shipped default promises.
 *
 * The DEVICE half stays per-session and is never persisted: a remembered id for
 * hardware that is not plugged in today is reconciled away at the next
 * enumeration anyway (see {@link isMicSelectionAvailable}).
 *
 * Off no longer guards the permission request — the half of AD-36 that still
 * holds is that nothing is ever requested from render: the request is bound to
 * an explicit enable, and a mic that is on without its grant blocks Start and
 * names itself (Story 20.2). The header Start click reads both values
 * imperatively and threads them through `recording_start`.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RecordingSourcesVm } from "@/lib/ipc/client";

export interface RecordingMicState {
  /** Whether the next session captures the microphone (persisted, default on). */
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
  micEnabled: true,
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

/** Test-only reset: restore the default-on toggle + default input. */
export function resetRecordingMicForTest(): void {
  recordingMicStore.setState({ micEnabled: true, micDeviceId: null });
}
