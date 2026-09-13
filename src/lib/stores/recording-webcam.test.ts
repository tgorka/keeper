import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import type { RecordingSourcesVm } from "@/lib/ipc/client";
import {
  cameraDeviceId,
  isCameraSelectionAvailable,
  recordingWebcamStore,
  resetRecordingWebcamForTest,
  setCameraDeviceId,
  setWebcamEnabled,
  useCameraDeviceId,
  useWebcamEnabled,
  webcamEnabled,
} from "@/lib/stores/recording-webcam";

afterEach(() => {
  resetRecordingWebcamForTest();
});

describe("recording-webcam store", () => {
  it("defaults the webcam to on with the system default camera", () => {
    // On by default since 2026-09-13 (spec *Recording remembers which sources
    // are on*); this compile-time value is what a start that happens before the
    // persisted answer is read uses, so it has to equal the Rust default. No
    // permission is requested on render — that half of AD-36 is asserted in the
    // card's own suite.
    expect(webcamEnabled()).toBe(true);
    expect(cameraDeviceId()).toBeNull();
    expect(recordingWebcamStore.getState().webcamEnabled).toBe(true);
  });

  it("setWebcamEnabled flips the toggle, read back imperatively", () => {
    setWebcamEnabled(false);
    expect(webcamEnabled()).toBe(false);
    setWebcamEnabled(true);
    expect(webcamEnabled()).toBe(true);
  });

  it("setCameraDeviceId selects a device and null restores the system default", () => {
    setCameraDeviceId("FaceTimeHDCamera");
    expect(cameraDeviceId()).toBe("FaceTimeHDCamera");
    setCameraDeviceId(null);
    expect(cameraDeviceId()).toBeNull();
  });

  it("the hook selectors reflect store changes reactively", () => {
    const { result } = renderHook(() => ({
      enabled: useWebcamEnabled(),
      deviceId: useCameraDeviceId(),
    }));
    expect(result.current.enabled).toBe(true);
    expect(result.current.deviceId).toBeNull();
    act(() => {
      setWebcamEnabled(false);
      setCameraDeviceId("X");
    });
    expect(result.current.enabled).toBe(false);
    expect(result.current.deviceId).toBe("X");
  });

  it("reset restores the default-on toggle and default camera", () => {
    setWebcamEnabled(false);
    setCameraDeviceId("X");
    resetRecordingWebcamForTest();
    expect(webcamEnabled()).toBe(true);
    expect(cameraDeviceId()).toBeNull();
  });
});

describe("isCameraSelectionAvailable", () => {
  const sources = (cameras: RecordingSourcesVm["cameras"]): RecordingSourcesVm => ({
    displays: [],
    applications: [],
    microphones: [],
    cameras,
  });

  it("treats never-polled sources as available (no spurious reset before the first poll)", () => {
    expect(isCameraSelectionAvailable(null, null)).toBe(true);
    expect(isCameraSelectionAvailable("X", null)).toBe(true);
  });

  it("the system default camera (null) is always available, even with no devices", () => {
    expect(isCameraSelectionAvailable(null, sources([]))).toBe(true);
    expect(isCameraSelectionAvailable(null, sources([{ id: "X", name: "FaceTime HD" }]))).toBe(
      true,
    );
  });

  it("a real id is available only while it is still enumerated", () => {
    const list = sources([{ id: "X", name: "FaceTime HD" }]);
    expect(isCameraSelectionAvailable("X", list)).toBe(true);
    expect(isCameraSelectionAvailable("Y", list)).toBe(false);
    expect(isCameraSelectionAvailable("X", sources([]))).toBe(false);
  });
});
