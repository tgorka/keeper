import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingStart: vi.fn(),
  recordingStop: vi.fn(),
  recordingStatus: vi.fn(),
  recordingAcknowledge: vi.fn(),
}));

import { IDLE_RECORDING_STATUS, useRecordingSession } from "@/hooks/use-recording-session";
import type { RecordingStatusVm } from "@/lib/ipc/client";
import {
  recordingAcknowledge,
  recordingStart,
  recordingStatus,
  recordingStop,
} from "@/lib/ipc/client";

const mockStart = vi.mocked(recordingStart);
const mockStop = vi.mocked(recordingStop);
const mockStatus = vi.mocked(recordingStatus);
const mockAcknowledge = vi.mocked(recordingAcknowledge);

const RECORDING: RecordingStatusVm = {
  state: "recording",
  segmentsClosed: 0,
  startedAtEpochMs: 1_700_000_000_000,
  outputPath: "/Users/alice/Movies/keeper/session",
  error: null,
  warning: null,
  onDiskBytes: 0,
  currentSegmentBytes: 0,
  segmentCapMb: 500,
};

const FAILED: RecordingStatusVm = {
  ...RECORDING,
  state: "failed",
  error: "keeper-rec exited unexpectedly",
};

beforeEach(() => {
  mockStart.mockReset();
  mockStart.mockResolvedValue(RECORDING);
  mockStop.mockReset();
  mockStop.mockResolvedValue(undefined);
  mockStatus.mockReset();
  mockStatus.mockResolvedValue(IDLE_RECORDING_STATUS);
  mockAcknowledge.mockReset();
  mockAcknowledge.mockResolvedValue(IDLE_RECORDING_STATUS);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("useRecordingSession (Story 18.4 acknowledge)", () => {
  it("acknowledge() adopts the Rust-returned idle snapshot", async () => {
    mockStatus.mockResolvedValue(FAILED);
    const { result } = renderHook(() => useRecordingSession());
    // The mount adoption picks up the failed session living in Rust — the
    // terminal snapshot is retained for the banner (no poll resets it).
    await waitFor(() => expect(result.current.status.state).toBe("failed"));

    await act(async () => {
      await result.current.acknowledge();
    });
    expect(mockAcknowledge).toHaveBeenCalledTimes(1);
    expect(result.current.status).toEqual(IDLE_RECORDING_STATUS);
  });

  it("a failed acknowledge() keeps the honest failed snapshot", async () => {
    mockStatus.mockResolvedValue(FAILED);
    mockAcknowledge.mockRejectedValue({ message: "ipc unavailable" });
    const { result } = renderHook(() => useRecordingSession());
    await waitFor(() => expect(result.current.status.state).toBe("failed"));

    await act(async () => {
      await result.current.acknowledge();
    });
    // Never an invented reset: the snapshot stays failed until Rust clears it.
    expect(result.current.status.state).toBe("failed");
    expect(result.current.status.error).toBe("keeper-rec exited unexpectedly");
  });
});
