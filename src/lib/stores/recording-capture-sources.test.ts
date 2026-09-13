/**
 * Which capture sources the next Recording Session starts with is a persisted
 * answer (spec *Recording remembers which sources are on*), and this file covers
 * the seam that makes it one: the session stores the Start click reads are
 * seeded from Rust once per launch, and every toggle writes back.
 *
 * The report this comes from was "every restart of the app these settings are
 * coming back to different defaults", so the cases that matter are the ones
 * about a choice SURVIVING: a stored `false` reaching the switch, a late read
 * never overwriting a choice made by hand in the meantime, and a failed write
 * never re-persisting a value the switches no longer show.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

const recordingCaptureSourcesGet = vi.fn();
const recordingCaptureSourcesSet = vi.fn();

vi.mock("@/lib/ipc/client", () => ({
  recordingCaptureSourcesGet: () => recordingCaptureSourcesGet(),
  recordingCaptureSourcesSet: (sources: unknown) => recordingCaptureSourcesSet(sources),
  // Imported by the recording-source store module that the capture stores pull
  // their types from; never called here.
  listRecordingSources: vi.fn(),
}));

import type { RecordingCaptureSourcesPatchVm, RecordingCaptureSourcesVm } from "@/lib/ipc/client";
import {
  resetRecordingAudioForTest,
  setSystemAudioEnabled,
  systemAudioEnabled,
} from "@/lib/stores/recording-audio";
import {
  captureSourcesForStart,
  ensureCaptureSourcesHydrated,
  persistCaptureSources,
  resetCaptureSourcesForTest,
} from "@/lib/stores/recording-capture-sources";
import { micEnabled, resetRecordingMicForTest, setMicEnabled } from "@/lib/stores/recording-mic";
import {
  resetRecordingWebcamForTest,
  setWebcamEnabled,
  webcamEnabled,
} from "@/lib/stores/recording-webcam";

const ALL_ON: RecordingCaptureSourcesVm = { systemAudio: true, microphone: true, camera: true };

beforeEach(() => {
  recordingCaptureSourcesGet.mockReset();
  recordingCaptureSourcesSet
    .mockReset()
    .mockImplementation(async (patch: RecordingCaptureSourcesPatchVm) => ({
      systemAudio: patch.systemAudio ?? ALL_ON.systemAudio,
      microphone: patch.microphone ?? ALL_ON.microphone,
      camera: patch.camera ?? ALL_ON.camera,
    }));
  resetCaptureSourcesForTest();
  resetRecordingAudioForTest();
  resetRecordingMicForTest();
  resetRecordingWebcamForTest();
});

describe("capture sources are seeded from the stored answer", () => {
  it("brings a stored off across the launch that forgot it before", async () => {
    recordingCaptureSourcesGet.mockResolvedValue({ ...ALL_ON, microphone: false, camera: false });

    await ensureCaptureSourcesHydrated();

    expect(micEnabled()).toBe(false);
    expect(webcamEnabled()).toBe(false);
    // System audio was on in the stored answer and stays on.
    expect(systemAudioEnabled()).toBe(true);
  });

  it("leaves the shipped defaults standing when the answer cannot be read", async () => {
    recordingCaptureSourcesGet.mockRejectedValue(new Error("settings unreadable"));

    await ensureCaptureSourcesHydrated();

    expect(systemAudioEnabled()).toBe(true);
    expect(micEnabled()).toBe(true);
    expect(webcamEnabled()).toBe(true);
  });

  it("reads once per launch however many surfaces ask", async () => {
    recordingCaptureSourcesGet.mockResolvedValue(ALL_ON);

    await Promise.all([
      ensureCaptureSourcesHydrated(),
      ensureCaptureSourcesHydrated(),
      ensureCaptureSourcesHydrated(),
    ]);
    await ensureCaptureSourcesHydrated();

    expect(recordingCaptureSourcesGet).toHaveBeenCalledTimes(1);
  });

  it("never moves a switch somebody already set this launch", async () => {
    // The ordering that makes this real: the cards are live from first paint,
    // so the person can turn the mic off while the first read is still in
    // flight — and that read then answers `true`. Seeding it would undo their
    // click in front of them, and the write would persist the wrong answer.
    let answerRead!: (vm: RecordingCaptureSourcesVm) => void;
    recordingCaptureSourcesGet.mockReturnValue(
      new Promise<RecordingCaptureSourcesVm>((resolve) => {
        answerRead = resolve;
      }),
    );
    const hydrating = ensureCaptureSourcesHydrated();

    setMicEnabled(false);
    const persisting = persistCaptureSources({ microphone: false });
    answerRead(ALL_ON);
    await Promise.all([hydrating, persisting]);

    expect(micEnabled()).toBe(false);
    expect(recordingCaptureSourcesSet).toHaveBeenCalledWith({
      systemAudio: null,
      microphone: false,
      camera: null,
    });
  });

  it("still seeds the sources nobody answered when one was toggled early", async () => {
    // A launch-wide one-shot would throw the OTHER two stored answers away the
    // moment one switch is touched before the read lands.
    let answerRead!: (vm: RecordingCaptureSourcesVm) => void;
    recordingCaptureSourcesGet.mockReturnValue(
      new Promise<RecordingCaptureSourcesVm>((resolve) => {
        answerRead = resolve;
      }),
    );
    const hydrating = ensureCaptureSourcesHydrated();

    // The switch's real path: move the store, then write it back — which is
    // what a launch-wide one-shot would read as "everything is decided".
    setWebcamEnabled(false);
    const persisting = persistCaptureSources({ camera: false });
    answerRead({ systemAudio: false, microphone: false, camera: true });
    await Promise.all([hydrating, persisting]);

    expect(webcamEnabled()).toBe(false);
    expect(micEnabled()).toBe(false);
    expect(systemAudioEnabled()).toBe(false);
  });
});

describe("persistCaptureSources", () => {
  it("writes only the switch that moved", async () => {
    // The other two answers may not have been read yet this launch; sending
    // them would persist the shipped defaults over what this device chose.
    setWebcamEnabled(false);

    await persistCaptureSources({ camera: false });

    expect(recordingCaptureSourcesSet).toHaveBeenCalledWith({
      systemAudio: null,
      microphone: null,
      camera: false,
    });
  });

  it("never re-persists a source whose own write failed", async () => {
    // The failure that would reproduce the original report through the fix:
    // the mic write fails and the switch stays off, so no later write may
    // carry `microphone: true` back to disk.
    setMicEnabled(false);
    recordingCaptureSourcesSet.mockRejectedValueOnce(new Error("database is locked"));
    await persistCaptureSources({ microphone: false });

    setWebcamEnabled(false);
    await persistCaptureSources({ camera: false });

    expect(recordingCaptureSourcesSet).toHaveBeenLastCalledWith({
      systemAudio: null,
      microphone: null,
      camera: false,
    });
    expect(micEnabled()).toBe(false);
  });

  it("takes Rust's answer when a config layer refuses the change", async () => {
    // `recording.camera` pinned by a config file: the write lands in the
    // database, the effective read still says `true`, and the switch has to
    // show that rather than promise a session that will not happen.
    setWebcamEnabled(false);
    recordingCaptureSourcesSet.mockResolvedValueOnce(ALL_ON);

    await persistCaptureSources({ camera: false });

    expect(webcamEnabled()).toBe(true);
  });

  it("leaves the session value standing when the write cannot be made at all", async () => {
    setSystemAudioEnabled(false);
    recordingCaptureSourcesSet.mockRejectedValue(new Error("no host"));

    await expect(persistCaptureSources({ systemAudio: false })).resolves.toBeUndefined();
    expect(systemAudioEnabled()).toBe(false);
  });
});

describe("captureSourcesForStart", () => {
  it("leaves an unanswered source to Rust", () => {
    // A palette verb or the hotkey can be the first thing that touches
    // recording in a launch: sending the compile-time default would start a
    // session with sources this device did not choose.
    expect(captureSourcesForStart()).toEqual({
      systemAudio: undefined,
      microphone: undefined,
      camera: undefined,
    });
  });

  it("sends what the switches show once the answer is in", async () => {
    recordingCaptureSourcesGet.mockResolvedValue({ ...ALL_ON, camera: false });
    await ensureCaptureSourcesHydrated();

    expect(captureSourcesForStart()).toEqual({
      systemAudio: true,
      microphone: true,
      camera: false,
    });
  });
});
