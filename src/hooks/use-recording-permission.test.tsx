import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingPermission: vi.fn(),
  requestScreenRecordingPermission: vi.fn(),
  openScreenRecordingSettings: vi.fn(),
}));

import {
  DEFAULT_RECORDING_PERMISSION,
  useRecordingPermission,
} from "@/hooks/use-recording-permission";
import type { RecordingPermissionVm } from "@/lib/ipc/client";
import {
  openScreenRecordingSettings,
  recordingPermission,
  requestScreenRecordingPermission,
} from "@/lib/ipc/client";

const mockFetch = vi.mocked(recordingPermission);
const mockRequest = vi.mocked(requestScreenRecordingPermission);
const mockOpenSettings = vi.mocked(openScreenRecordingSettings);

const GRANTED: RecordingPermissionVm = { screenRecording: "granted", canStart: true };
const DENIED: RecordingPermissionVm = { screenRecording: "denied", canStart: false };

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
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("useRecordingPermission", () => {
  it("starts at the safe default and live-detects on mount", async () => {
    const { result } = renderHook(() => useRecordingPermission());

    // Before the probe lands: the safe default (Start disabled).
    expect(DEFAULT_RECORDING_PERMISSION.canStart).toBe(false);

    await waitFor(() => expect(result.current.permission).toEqual(GRANTED));
    expect(mockFetch).toHaveBeenCalledTimes(1);
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

  it("swallows a failed probe to the safe default (no crash, no spinner)", async () => {
    mockFetch.mockImplementation(ipcRejection);
    const { result } = renderHook(() => useRecordingPermission());

    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
    expect(result.current.permission).toEqual(DEFAULT_RECORDING_PERMISSION);
  });

  it("request adopts the re-resolved outcome", async () => {
    mockFetch.mockResolvedValue({ screenRecording: "notYetRequested", canStart: false });
    const { result } = renderHook(() => useRecordingPermission());
    await waitFor(() => expect(result.current.permission.screenRecording).toBe("notYetRequested"));

    mockRequest.mockResolvedValue(GRANTED);
    await act(async () => {
      await result.current.request();
    });

    expect(result.current.permission).toEqual(GRANTED);
    expect(mockRequest).toHaveBeenCalledTimes(1);
  });

  it("request that re-resolves denied adopts the denied-with-fix-path", async () => {
    mockFetch.mockResolvedValue({ screenRecording: "notYetRequested", canStart: false });
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
