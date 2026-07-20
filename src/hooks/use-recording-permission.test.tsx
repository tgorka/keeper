import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingPermission: vi.fn(),
  requestScreenRecordingPermission: vi.fn(),
  openScreenRecordingSettings: vi.fn(),
  // The mic/camera legs (Story 20.2): row request + deep-link commands.
  requestMicrophonePermission: vi.fn(),
  requestCameraPermission: vi.fn(),
  openMicrophoneSettings: vi.fn(),
  openCameraSettings: vi.fn(),
}));

import {
  DEFAULT_RECORDING_PERMISSION,
  useRecordingPermission,
} from "@/hooks/use-recording-permission";
import type { RecordingPermissionVm } from "@/lib/ipc/client";
import {
  openCameraSettings,
  openMicrophoneSettings,
  openScreenRecordingSettings,
  recordingPermission,
  requestCameraPermission,
  requestMicrophonePermission,
  requestScreenRecordingPermission,
} from "@/lib/ipc/client";
import { resetRecordingMicForTest, setMicEnabled } from "@/lib/stores/recording-mic";
import { resetRecordingWebcamForTest, setWebcamEnabled } from "@/lib/stores/recording-webcam";

const mockFetch = vi.mocked(recordingPermission);
const mockRequest = vi.mocked(requestScreenRecordingPermission);
const mockOpenSettings = vi.mocked(openScreenRecordingSettings);
const mockRequestMic = vi.mocked(requestMicrophonePermission);
const mockRequestCamera = vi.mocked(requestCameraPermission);
const mockOpenMicSettings = vi.mocked(openMicrophoneSettings);
const mockOpenCameraSettings = vi.mocked(openCameraSettings);

const GRANTED: RecordingPermissionVm = {
  screenRecording: "granted",
  microphone: null,
  camera: null,
  canStart: true,
};
const DENIED: RecordingPermissionVm = {
  screenRecording: "denied",
  microphone: null,
  camera: null,
  canStart: false,
};
/** Mic enabled but denied: the leg is present and blocks Start (Story 20.2). */
const MIC_BLOCKING: RecordingPermissionVm = {
  screenRecording: "granted",
  microphone: "denied",
  camera: null,
  canStart: false,
};

/** A rejected-with-IpcError promise, matching the client contract. */
function ipcRejection() {
  return Promise.reject({
    code: "internal",
    message: "keeper-rec did not answer",
    accountId: null,
    retriable: false,
  });
}

beforeEach(() => {
  mockFetch.mockReset();
  mockFetch.mockResolvedValue(GRANTED);
  mockRequest.mockReset();
  mockRequest.mockResolvedValue(GRANTED);
  mockOpenSettings.mockReset();
  mockOpenSettings.mockResolvedValue(undefined);
  mockRequestMic.mockReset();
  mockRequestMic.mockResolvedValue("granted");
  mockRequestCamera.mockReset();
  mockRequestCamera.mockResolvedValue("granted");
  mockOpenMicSettings.mockReset();
  mockOpenMicSettings.mockResolvedValue(undefined);
  mockOpenCameraSettings.mockReset();
  mockOpenCameraSettings.mockResolvedValue(undefined);
});

afterEach(() => {
  // Restore the default-off mic/webcam toggles between tests (Story 20.2 —
  // the hook subscribes to both stores).
  resetRecordingMicForTest();
  resetRecordingWebcamForTest();
  vi.clearAllMocks();
});

describe("useRecordingPermission", () => {
  it("starts at the safe default and live-detects on mount", async () => {
    const { result } = renderHook(() => useRecordingPermission());

    // Before the probe lands: the safe default (Start disabled, no leg claimed).
    expect(DEFAULT_RECORDING_PERMISSION.canStart).toBe(false);
    expect(DEFAULT_RECORDING_PERMISSION.microphone).toBeNull();
    expect(DEFAULT_RECORDING_PERMISSION.camera).toBeNull();

    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));
    expect(mockFetch).toHaveBeenCalledTimes(1);
    // Both sources default off: the probe reports both legs disabled.
    expect(mockFetch).toHaveBeenCalledWith(false, false);
  });

  it("re-detects when the document becomes visible again", async () => {
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));

    mockFetch.mockResolvedValue(DENIED);
    act(() => {
      document.dispatchEvent(new Event("visibilitychange"));
    });

    // jsdom reports visibilityState "visible", so the re-detect fires.
    await waitFor(() => expect(result.current.permission).toEqual(DENIED));
    expect(mockFetch).toHaveBeenCalledTimes(2);
  });

  it("re-detects on window focus (the System Settings round-trip)", async () => {
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));

    mockFetch.mockResolvedValue(DENIED);
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });

    await waitFor(() => expect(result.current.permission).toEqual(DENIED));
  });

  it("re-fetches with the mic flag when the mic source is toggled on (Story 20.2)", async () => {
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));
    expect(mockFetch).toHaveBeenLastCalledWith(false, false);

    mockFetch.mockResolvedValue(MIC_BLOCKING);
    act(() => {
      setMicEnabled(true);
    });

    // The enabled-change re-fetch carries the flag, and the resolved leg (with
    // its Start gate) is adopted.
    await waitFor(() => expect(result.current.permission).toEqual(MIC_BLOCKING));
    expect(mockFetch).toHaveBeenLastCalledWith(true, false);
    expect(result.current.permission.canStart).toBe(false);
  });

  it("re-fetches with the camera flag when the webcam is toggled on (Story 20.2)", async () => {
    const CAMERA_LEG: RecordingPermissionVm = {
      screenRecording: "granted",
      microphone: null,
      camera: "notYetRequested",
      canStart: false,
    };
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));

    mockFetch.mockResolvedValue(CAMERA_LEG);
    act(() => {
      setWebcamEnabled(true);
    });

    await waitFor(() => expect(result.current.permission).toEqual(CAMERA_LEG));
    expect(mockFetch).toHaveBeenLastCalledWith(false, true);
  });

  it("swallows a failed probe to the safe default (no crash, no spinner)", async () => {
    mockFetch.mockImplementation(ipcRejection);
    const { result } = renderHook(() => useRecordingPermission());

    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
    expect(result.current.permission).toEqual(DEFAULT_RECORDING_PERMISSION);
  });

  it("request adopts the re-resolved outcome and threads the enabled flags", async () => {
    setMicEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "notYetRequested",
      microphone: "granted",
      camera: null,
      canStart: false,
    });
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission.screenRecording).toBe("notYetRequested"));

    mockRequest.mockResolvedValue({ ...GRANTED, microphone: "granted" });
    await act(async () => {
      await result.current.request();
    });

    expect(result.current.permission).toEqual({ ...GRANTED, microphone: "granted" });
    expect(mockRequest).toHaveBeenCalledTimes(1);
    // The request carries the same enabled flags the probe does, so the
    // adopted VM never blanks an enabled source's leg.
    expect(mockRequest).toHaveBeenCalledWith(true, false);
  });

  it("request that re-resolves denied adopts the denied-with-fix-path", async () => {
    mockFetch.mockResolvedValue({
      screenRecording: "notYetRequested",
      microphone: null,
      camera: null,
      canStart: false,
    });
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission.screenRecording).toBe("notYetRequested"));

    // The OS answer comes back not-granted (a prior denial shows no prompt): the
    // command re-resolves to denied. Start stays disabled and the row must offer
    // the System Settings fix path.
    mockRequest.mockResolvedValue(DENIED);
    await act(async () => {
      await result.current.request();
    });

    expect(result.current.permission).toEqual(DENIED);
    expect(result.current.permission.canStart).toBe(false);
  });

  it("requestMicrophone runs the OS request then re-probes live (Story 20.2)", async () => {
    setMicEnabled(true);
    mockFetch.mockResolvedValue(MIC_BLOCKING);
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(MIC_BLOCKING));
    const fetchesBefore = mockFetch.mock.calls.length;

    const MIC_GRANTED: RecordingPermissionVm = { ...GRANTED, microphone: "granted" };
    mockFetch.mockResolvedValue(MIC_GRANTED);
    await act(async () => {
      await result.current.requestMicrophone();
    });

    // The request itself makes no state claim — the follow-up live probe does.
    expect(mockRequestMic).toHaveBeenCalledTimes(1);
    expect(mockFetch.mock.calls.length).toBe(fetchesBefore + 1);
    expect(result.current.permission).toEqual(MIC_GRANTED);
  });

  it("a failed requestMicrophone still re-probes (no claim either way)", async () => {
    setMicEnabled(true);
    mockFetch.mockResolvedValue(MIC_BLOCKING);
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(MIC_BLOCKING));

    mockRequestMic.mockImplementation(ipcRejection);
    await act(async () => {
      await result.current.requestMicrophone();
    });

    // The re-probe still resolved the honest current state — never a crash.
    expect(result.current.permission).toEqual(MIC_BLOCKING);
  });

  it("requestCamera runs the OS request then re-probes live (Story 20.2)", async () => {
    setWebcamEnabled(true);
    const CAMERA_BLOCKING: RecordingPermissionVm = {
      screenRecording: "granted",
      microphone: null,
      camera: "notYetRequested",
      canStart: false,
    };
    mockFetch.mockResolvedValue(CAMERA_BLOCKING);
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(CAMERA_BLOCKING));

    const CAMERA_GRANTED: RecordingPermissionVm = { ...GRANTED, camera: "granted" };
    mockFetch.mockResolvedValue(CAMERA_GRANTED);
    await act(async () => {
      await result.current.requestCamera();
    });

    expect(mockRequestCamera).toHaveBeenCalledTimes(1);
    expect(result.current.permission).toEqual(CAMERA_GRANTED);
  });

  it("a stale in-flight probe never clobbers a newer result", async () => {
    // First probe resolves slowly to GRANTED; a second, newer probe resolves
    // quickly to DENIED. Last-initiated wins: the late GRANTED must be dropped.
    let releaseSlow!: (vm: RecordingPermissionVm) => void;
    const slow = new Promise<RecordingPermissionVm>((resolve) => {
      releaseSlow = resolve;
    });
    mockFetch.mockReturnValueOnce(slow).mockResolvedValue(DENIED);

    const { result } = renderHook(() => useRecordingPermission());
    // Kick a newer probe while the mount probe is still pending.
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    await waitFor(() => expect(result.current.permission).toEqual(DENIED));

    // The earlier (mount) probe now resolves GRANTED — it must not win.
    await act(async () => {
      releaseSlow(GRANTED);
      await slow;
    });
    expect(result.current.permission).toEqual(DENIED);
  });

  it("a failed request degrades to a fresh live probe", async () => {
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));

    mockRequest.mockImplementation(ipcRejection);
    mockFetch.mockResolvedValue(DENIED);
    await act(async () => {
      await result.current.request();
    });

    expect(result.current.permission).toEqual(DENIED);
  });

  it("openSettings fires the deep link and swallows rejection", async () => {
    mockOpenSettings.mockImplementation(ipcRejection);
    const { result } = renderHook(() => useRecordingPermission());

    act(() => {
      result.current.openSettings();
    });

    await waitFor(() => expect(mockOpenSettings).toHaveBeenCalledTimes(1));
  });

  it("openMicrophoneSettings / openCameraSettings fire their deep links and swallow rejection", async () => {
    mockOpenMicSettings.mockImplementation(ipcRejection);
    mockOpenCameraSettings.mockImplementation(ipcRejection);
    const { result } = renderHook(() => useRecordingPermission());

    act(() => {
      result.current.openMicrophoneSettings();
      result.current.openCameraSettings();
    });

    await waitFor(() => expect(mockOpenMicSettings).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(mockOpenCameraSettings).toHaveBeenCalledTimes(1));
  });

  it("removes its listeners on unmount (no re-detect after teardown)", async () => {
    const { result, unmount } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));
    const calls = mockFetch.mock.calls.length;

    unmount();
    document.dispatchEvent(new Event("visibilitychange"));
    window.dispatchEvent(new Event("focus"));

    expect(mockFetch).toHaveBeenCalledTimes(calls);
  });
});
