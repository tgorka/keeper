import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RecordingStatusVm } from "@/lib/ipc/client";

const recordingStart = vi.fn();
const recordingStop = vi.fn();
const recordingStatus = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  recordingStart: (...args: unknown[]) => recordingStart(...args),
  recordingStop: () => recordingStop(),
  recordingStatus: () => recordingStatus(),
}));

import { IDLE_RECORDING_STATUS } from "@/hooks/use-recording-session";
import {
  startRecordingWithCurrentSelections,
  stopRecording,
  toggleRecording,
} from "@/lib/recording-control";
import { setSystemAudioEnabled } from "@/lib/stores/recording-audio";
import { setMicDeviceId, setMicEnabled } from "@/lib/stores/recording-mic";
import {
  DEFAULT_RECORDING_TARGET,
  resetRecordingSourceForTest,
  selectRecordingTarget,
} from "@/lib/stores/recording-source";
import { setCameraDeviceId, setWebcamEnabled } from "@/lib/stores/recording-webcam";

/** A live snapshot for the toggle's stop path. */
const LIVE_STATUS: RecordingStatusVm = {
  ...IDLE_RECORDING_STATUS,
  state: "recording",
  startedAtEpochMs: 1_720_000_000_000,
};

beforeEach(() => {
  recordingStart.mockReset().mockResolvedValue(IDLE_RECORDING_STATUS);
  recordingStop.mockReset().mockResolvedValue(undefined);
  recordingStatus.mockReset().mockResolvedValue(IDLE_RECORDING_STATUS);
  // Reset the module-level capture stores back to their shipped defaults.
  resetRecordingSourceForTest();
  setSystemAudioEnabled(true);
  setMicEnabled(false);
  setMicDeviceId(null);
  setWebcamEnabled(false);
  setCameraDeviceId(null);
});

describe("startRecordingWithCurrentSelections", () => {
  it("reads the current capture selections from the module-level stores", async () => {
    // Non-default selections across all six stores — the exact singletons the
    // Start button and banner Restart read (Story 20.4).
    selectRecordingTarget({ kind: "application", pid: 501, bundleId: "com.apple.Safari" });
    setSystemAudioEnabled(false);
    setMicEnabled(true);
    setMicDeviceId("mic-1");
    setWebcamEnabled(true);
    setCameraDeviceId("cam-1");

    await startRecordingWithCurrentSelections();

    expect(recordingStart).toHaveBeenCalledWith(
      { kind: "application", pid: 501, bundleId: "com.apple.Safari" },
      false,
      true,
      "mic-1",
      true,
      "cam-1",
    );
  });

  it("passes the shipped defaults when nothing was changed", async () => {
    await startRecordingWithCurrentSelections();
    expect(recordingStart).toHaveBeenCalledWith(
      DEFAULT_RECORDING_TARGET,
      true,
      false,
      null,
      false,
      null,
    );
  });

  it("swallows a rejected start (the 18.4 pipeline surfaces it)", async () => {
    recordingStart.mockRejectedValue(new Error("permission blocked"));
    await expect(startRecordingWithCurrentSelections()).resolves.toBeUndefined();
  });
});

describe("stopRecording", () => {
  it("routes through recordingStop", async () => {
    await stopRecording();
    expect(recordingStop).toHaveBeenCalledTimes(1);
  });

  it("swallows a rejected stop", async () => {
    recordingStop.mockRejectedValue(new Error("no host"));
    await expect(stopRecording()).resolves.toBeUndefined();
  });
});

describe("toggleRecording", () => {
  it("stops when the authoritative snapshot is live", async () => {
    recordingStatus.mockResolvedValue(LIVE_STATUS);
    await toggleRecording();
    expect(recordingStop).toHaveBeenCalledTimes(1);
    expect(recordingStart).not.toHaveBeenCalled();
  });

  it("starts with the current selections when idle", async () => {
    recordingStatus.mockResolvedValue(IDLE_RECORDING_STATUS);
    await toggleRecording();
    expect(recordingStart).toHaveBeenCalledTimes(1);
    expect(recordingStop).not.toHaveBeenCalled();
  });

  it("treats a terminal (finalized) snapshot as not live and starts", async () => {
    recordingStatus.mockResolvedValue({ ...IDLE_RECORDING_STATUS, state: "finalized" });
    await toggleRecording();
    expect(recordingStart).toHaveBeenCalledTimes(1);
  });

  it("is a safe no-op when the status read fails (outside Tauri)", async () => {
    recordingStatus.mockRejectedValue(new Error("no tauri host"));
    await toggleRecording();
    expect(recordingStart).not.toHaveBeenCalled();
    expect(recordingStop).not.toHaveBeenCalled();
  });
});
