import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
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
  it("defaults the mic to off with the system default input", () => {
    // Off-by-default is the lazy-permission hinge (FR-69, AD-36): no
    // permission is ever requested until the user enables the source.
    expect(micEnabled()).toBe(false);
    expect(micDeviceId()).toBeNull();
    expect(recordingMicStore.getState().micEnabled).toBe(false);
  });

  it("setMicEnabled flips the toggle, read back imperatively", () => {
    setMicEnabled(true);
    expect(micEnabled()).toBe(true);
    setMicEnabled(false);
    expect(micEnabled()).toBe(false);
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
    expect(result.current.enabled).toBe(false);
    expect(result.current.deviceId).toBeNull();
    act(() => {
      setMicEnabled(true);
      setMicDeviceId("X");
    });
    expect(result.current.enabled).toBe(true);
    expect(result.current.deviceId).toBe("X");
  });

  it("reset restores the default-off toggle and default input", () => {
    setMicEnabled(true);
    setMicDeviceId("X");
    resetRecordingMicForTest();
    expect(micEnabled()).toBe(false);
    expect(micDeviceId()).toBeNull();
  });
});
