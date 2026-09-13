import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { RecordingSourcesVm } from "@/lib/ipc/client";
import {
  isMicSelectionAvailable,
  micDeviceId,
  micEnabled,
  recordingMicStore,
  resetRecordingMicForTest,
  setMicDeviceId,
  setMicEnabled,
  useMicDeviceId,
  useMicEnabled,
} from "@/lib/stores/recording-mic";

afterEach(() => {
  resetRecordingMicForTest();
});

describe("recording-mic store", () => {
  it("defaults the mic to on with the system default input", () => {
    // On by default since 2026-09-13 (spec *Recording remembers which sources
    // are on*), and this compile-time value is what a start that happens before
    // the persisted answer is read will use — so it has to equal the Rust
    // default, not merely be truthy. The half of AD-36 that still holds is
    // asserted where it lives: the card requests nothing on render.
    expect(micEnabled()).toBe(true);
    expect(micDeviceId()).toBeNull();
    expect(recordingMicStore.getState().micEnabled).toBe(true);
  });

  it("setMicEnabled flips the toggle, read back imperatively", () => {
    setMicEnabled(false);
    expect(micEnabled()).toBe(false);
    setMicEnabled(true);
    expect(micEnabled()).toBe(true);
  });

  it("setMicDeviceId selects a device and null restores the system default", () => {
    setMicDeviceId("BuiltInMicrophoneDevice");
    expect(micDeviceId()).toBe("BuiltInMicrophoneDevice");
    setMicDeviceId(null);
    expect(micDeviceId()).toBeNull();
  });

  it("the hook selectors reflect store changes reactively", () => {
    const { result } = renderHook(() => ({
      enabled: useMicEnabled(),
      deviceId: useMicDeviceId(),
    }));
    expect(result.current.enabled).toBe(true);
    expect(result.current.deviceId).toBeNull();
    act(() => {
      setMicEnabled(false);
      setMicDeviceId("X");
    });
    expect(result.current.enabled).toBe(false);
    expect(result.current.deviceId).toBe("X");
  });

  it("reset restores the default-on toggle and default input", () => {
    setMicEnabled(false);
    setMicDeviceId("X");
    resetRecordingMicForTest();
    expect(micEnabled()).toBe(true);
    expect(micDeviceId()).toBeNull();
  });
});

describe("isMicSelectionAvailable", () => {
  const sources = (microphones: RecordingSourcesVm["microphones"]): RecordingSourcesVm => ({
    displays: [],
    applications: [],
    microphones,
    cameras: [],
  });

  it("treats never-polled sources as available (no spurious reset before the first poll)", () => {
    expect(isMicSelectionAvailable(null, null)).toBe(true);
    expect(isMicSelectionAvailable("X", null)).toBe(true);
  });

  it("the system default input (null) is always available, even with no devices", () => {
    expect(isMicSelectionAvailable(null, sources([]))).toBe(true);
    expect(isMicSelectionAvailable(null, sources([{ id: "X", name: "USB Microphone" }]))).toBe(
      true,
    );
  });

  it("a real id is available only while it is still enumerated", () => {
    const list = sources([{ id: "X", name: "USB Microphone" }]);
    expect(isMicSelectionAvailable("X", list)).toBe(true);
    expect(isMicSelectionAvailable("Y", list)).toBe(false);
    expect(isMicSelectionAvailable("X", sources([]))).toBe(false);
  });
});
