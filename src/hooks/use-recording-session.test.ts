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
  durability: { state: "local", detail: null },
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

describe("useRecordingSession (Story 70.1 in-flight guard)", () => {
  // Every tick used to fire `recordingStatus()` whether or not the previous
  // one had answered; behind it was a full-tree walk of the sync folder that
  // took up to a minute on the field machine, so the ticks piled up into
  // concurrent walks. One call in flight at a time is the whole contract.
  it("skips a tick while the previous recordingStatus() is still in flight", async () => {
    vi.useFakeTimers();
    try {
      // The mount read answers at once and puts the session live, which is
      // what starts the 1 s poll.
      mockStatus.mockResolvedValueOnce(RECORDING);
      const { result } = renderHook(() => useRecordingSession());
      await act(async () => {
        await Promise.resolve();
      });
      expect(result.current.status.state).toBe("recording");
      expect(mockStatus).toHaveBeenCalledTimes(1);

      // The first poll hangs.
      let answer: (vm: RecordingStatusVm) => void = () => {};
      mockStatus.mockImplementationOnce(
        () =>
          new Promise<RecordingStatusVm>((resolve) => {
            answer = resolve;
          }),
      );
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(mockStatus).toHaveBeenCalledTimes(2);

      // Three more ticks while it hangs: not one more call.
      await act(async () => {
        vi.advanceTimersByTime(3000);
      });
      expect(mockStatus).toHaveBeenCalledTimes(2);

      // Answered: the next tick asks again.
      mockStatus.mockResolvedValue(RECORDING);
      await act(async () => {
        answer(RECORDING);
        await Promise.resolve();
      });
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(mockStatus).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a poll that fails releases the guard for the next tick", async () => {
    vi.useFakeTimers();
    try {
      mockStatus.mockResolvedValueOnce(RECORDING);
      const { result } = renderHook(() => useRecordingSession());
      await act(async () => {
        await Promise.resolve();
      });
      expect(result.current.status.state).toBe("recording");

      mockStatus.mockRejectedValueOnce({ message: "ipc unavailable" });
      await act(async () => {
        vi.advanceTimersByTime(1000);
        await Promise.resolve();
      });
      expect(mockStatus).toHaveBeenCalledTimes(2);
      // The failure kept the snapshot and did not wedge the poll.
      expect(result.current.status.state).toBe("recording");
      mockStatus.mockResolvedValue(RECORDING);
      await act(async () => {
        vi.advanceTimersByTime(1000);
      });
      expect(mockStatus).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });
});
