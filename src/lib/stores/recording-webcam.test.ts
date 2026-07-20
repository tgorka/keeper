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
  it("defaults the webcam to off with the system default camera", () => {
    // Off-by-default is the lazy-permission hinge (FR-70, AD-36): no camera
    // permission is ever requested — and no camera-#### file is ever written
    // — until the user enables the source.
    expect(webcamEnabled()).toBe(false);
    expect(cameraDeviceId()).toBeNull();
    expect(recordingWebcamStore.getState().webcamEnabled).toBe(false);
  });

  it("setWebcamEnabled flips the toggle, read back imperatively", () => {
    setWebcamEnabled(true);
    expect(webcamEnabled()).toBe(true);
    setWebcamEnabled(false);
    expect(webcamEnabled()).toBe(false);
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
    expect(result.current.enabled).toBe(false);
    expect(result.current.deviceId).toBeNull();
    act(() => {
      setWebcamEnabled(true);
      setCameraDeviceId("X");
    });
    expect(result.current.enabled).toBe(true);
    expect(result.current.deviceId).toBe("X");
  });

  it("reset restores the default-off toggle and default camera", () => {
    setWebcamEnabled(true);
    setCameraDeviceId("X");
    resetRecordingWebcamForTest();
    expect(webcamEnabled()).toBe(false);
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
