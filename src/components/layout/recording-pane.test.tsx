import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingPermission: vi.fn(),
  requestScreenRecordingPermission: vi.fn(),
  openScreenRecordingSettings: vi.fn(),
  // The mic/camera pre-flight rows' deep links (Story 20.2).
  openMicrophoneSettings: vi.fn(() => Promise.resolve()),
  openCameraSettings: vi.fn(() => Promise.resolve()),
  recordingStart: vi.fn(),
  recordingStop: vi.fn(),
  recordingStatus: vi.fn(),
  // The banner's Dismiss acknowledges a terminal outcome (Story 18.4).
  recordingAcknowledge: vi.fn(),
  // The Audio card's mic Switch fires this lazily on enable (Story 19.3).
  requestMicrophonePermission: vi.fn(() => Promise.resolve("granted")),
  // The Webcam card's Switch fires this lazily on enable (Story 20.1).
  requestCameraPermission: vi.fn(() => Promise.resolve("granted")),
  // The "Source" card mounts the live picker (Story 19.1), which polls this.
  // A real main display keeps the default (main-display) selection available.
  listRecordingSources: vi.fn(() =>
    Promise.resolve({
      displays: [{ id: 1, width: 3456, height: 2234, isMain: true }],
      applications: [],
      microphones: [],
      cameras: [],
    }),
  ),
  // The "Segmenting" / "Destination" / "Advanced" cards mount the shared
  // settings controls (Story 17.5 + 19.5), which lazily hydrate from this read.
  recordingSettingsGet: vi.fn(() =>
    Promise.resolve({
      segmentMb: 500,
      durationCapMinutes: 30,
      destinationDir: "/Users/alice/Movies/keeper",
      fps: 30,
    }),
  ),
  recordingSettingsSet: vi.fn((vm: unknown) => Promise.resolve(vm)),
  // The completion / recovery cards (Story 20.3): the summary fetch, the
  // cross-restart recovery scan, the acknowledgement latch, and Reveal.
  recordingSessionSummary: vi.fn(() =>
    Promise.resolve({
      sessionFolder: "/Users/alice/Movies/keeper/keeper-rec test.mp4",
      screenSegmentCount: 3,
      totalBytes: 412_000_000,
    }),
  ),
  recoveredSessionsList: vi.fn(() => Promise.resolve([])),
  recoveredSessionAcknowledge: vi.fn(() => Promise.resolve()),
  revealPath: vi.fn(() => Promise.resolve()),
}));

// The Destination card's folder chooser (Story 19.5) opens the OS-native
// directory picker; mock it so the pane renders without a Tauri runtime.
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve(null)),
}));

import {
  RecordingPane,
  START_RECORDING_LABEL,
  STOP_RECORDING_LABEL,
  startBlockedNote,
} from "@/components/layout/recording-pane";
import {
  RECOVERY_DISMISS_LABEL,
  REVEAL_IN_FINDER_LABEL,
} from "@/components/layout/recording-summary-card";
import {
  BANNER_DISMISS_LABEL,
  BANNER_RESTART_LABEL,
} from "@/components/recording/active-recording-banner";
import {
  ADVANCED_TOGGLE_TESTID,
  FPS_SELECT_TESTID,
} from "@/components/recording/recording-advanced-controls";
import {
  MIC_SWITCH_TESTID,
  SYSTEM_AUDIO_LABEL,
  SYSTEM_AUDIO_SWITCH_TESTID,
} from "@/components/recording/recording-audio-controls";
import { DESTINATION_PATH_TESTID } from "@/components/recording/recording-destination-controls";
import {
  CAMERA_PERMISSION_NAME,
  CAMERA_ROW_NOTE,
  MICROPHONE_PERMISSION_NAME,
  MICROPHONE_ROW_NOTE,
  OPEN_SETTINGS_LABEL,
  permissionRowTestId,
  REQUEST_PERMISSION_LABEL,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";
import { WEBCAM_SWITCH_TESTID } from "@/components/recording/recording-webcam-controls";
import {
  DURATION_CAP_LABEL,
  SEGMENT_SIZE_LABEL,
} from "@/components/settings/recording-settings-controls";
import type { RecordingPermissionVm, RecordingStatusVm } from "@/lib/ipc/client";
import {
  openCameraSettings,
  openMicrophoneSettings,
  openScreenRecordingSettings,
  recordingAcknowledge,
  recordingPermission,
  recordingSessionSummary,
  recordingStart,
  recordingStatus,
  recordingStop,
  recoveredSessionAcknowledge,
  recoveredSessionsList,
  requestCameraPermission,
  requestMicrophonePermission,
  requestScreenRecordingPermission,
  revealPath,
} from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { resetRecordingAudioForTest } from "@/lib/stores/recording-audio";
import { resetRecordingMicForTest, setMicEnabled } from "@/lib/stores/recording-mic";
import { resetRecordingSourceForTest, selectRecordingTarget } from "@/lib/stores/recording-source";
import { resetRecordingWebcamForTest, setWebcamEnabled } from "@/lib/stores/recording-webcam";

const mockFetch = vi.mocked(recordingPermission);
const mockRequest = vi.mocked(requestScreenRecordingPermission);
const mockOpenSettings = vi.mocked(openScreenRecordingSettings);
const mockOpenMicSettings = vi.mocked(openMicrophoneSettings);
const mockOpenCameraSettings = vi.mocked(openCameraSettings);
const mockRequestMic = vi.mocked(requestMicrophonePermission);
const mockRequestCamera = vi.mocked(requestCameraPermission);
const mockStart = vi.mocked(recordingStart);
const mockStop = vi.mocked(recordingStop);
const mockStatus = vi.mocked(recordingStatus);
const mockSummary = vi.mocked(recordingSessionSummary);
const mockAcknowledge = vi.mocked(recordingAcknowledge);
const mockRecoveredList = vi.mocked(recoveredSessionsList);
const mockRecoveredAck = vi.mocked(recoveredSessionAcknowledge);
const mockReveal = vi.mocked(revealPath);

const IDLE_STATUS: RecordingStatusVm = {
  state: "idle",
  segmentsClosed: 0,
  startedAtEpochMs: null,
  outputPath: null,
  error: null,
  warning: null,
  onDiskBytes: 0,
  currentSegmentBytes: 0,
  segmentCapMb: 0,
};

const RECORDING_STATUS: RecordingStatusVm = {
  state: "recording",
  segmentsClosed: 0,
  startedAtEpochMs: 1_700_000_000_000,
  outputPath: "/Users/alice/Movies/keeper/keeper-rec test.mp4",
  error: null,
  warning: null,
  onDiskBytes: 412_000_000,
  currentSegmentBytes: 100_000_000,
  segmentCapMb: 500,
};

const GRANTED: RecordingPermissionVm = {
  screenRecording: "granted",
  microphone: null,
  camera: null,
  canStart: true,
};
const NOT_YET: RecordingPermissionVm = {
  screenRecording: "notYetRequested",
  microphone: null,
  camera: null,
  canStart: false,
};
const DENIED: RecordingPermissionVm = {
  screenRecording: "denied",
  microphone: null,
  camera: null,
  canStart: false,
};

beforeEach(() => {
  mockFetch.mockReset();
  mockFetch.mockResolvedValue(NOT_YET);
  mockRequest.mockReset();
  mockRequest.mockResolvedValue(GRANTED);
  mockOpenSettings.mockReset();
  mockOpenSettings.mockResolvedValue(undefined);
  mockStart.mockReset();
  mockStart.mockResolvedValue(RECORDING_STATUS);
  mockStop.mockReset();
  mockStop.mockResolvedValue(undefined);
  mockStatus.mockReset();
  mockStatus.mockResolvedValue(IDLE_STATUS);
  mockAcknowledge.mockReset();
  mockAcknowledge.mockResolvedValue(IDLE_STATUS);
  mockSummary.mockReset();
  mockSummary.mockResolvedValue({
    sessionFolder: RECORDING_STATUS.outputPath ?? "",
    screenSegmentCount: 3,
    totalBytes: 412_000_000,
  });
  mockRecoveredList.mockReset();
  mockRecoveredList.mockResolvedValue([]);
  mockRecoveredAck.mockReset();
  mockRecoveredAck.mockResolvedValue(undefined);
  mockReveal.mockReset();
  mockReveal.mockResolvedValue(undefined);
  // Reveal is capability-gated (Story 20.3): default ON for the base cases.
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true, revealInFileManager: true });
});

afterEach(() => {
  // Stop the picker's poll timer + restore the default selection between tests
  // (Story 19.1) — a leaked interval would keep firing into the next test.
  resetRecordingSourceForTest();
  // Restore the default-on system-audio toggle between tests (Story 19.2).
  resetRecordingAudioForTest();
  // Restore the default-off mic selection between tests (Story 19.3).
  resetRecordingMicForTest();
  // Restore the default-off webcam selection between tests (Story 20.1).
  resetRecordingWebcamForTest();
  // Restore the safe-default capabilities between tests (Story 20.3).
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  vi.clearAllMocks();
});

describe("RecordingPane", () => {
  it("renders the shell chrome with the honest local-only subtitle", async () => {
    render(<RecordingPane />);

    expect(screen.getByRole("region", { name: "Recording" })).toBeInTheDocument();
    expect(screen.getByText("Recorded locally. Nothing uploads.")).toBeInTheDocument();
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
  });

  it("hosts the Permissions section above the setup cards", async () => {
    render(<RecordingPane />);

    expect(screen.getByText("Permissions")).toBeInTheDocument();
    expect(screen.getByText("Screen Recording")).toBeInTheDocument();
    // The 16.3 setup placeholders are still reserved below it.
    for (const title of ["Source", "Audio", "Webcam", "Destination", "Segmenting", "Advanced"]) {
      expect(screen.getByText(title)).toBeInTheDocument();
    }
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
  });

  it("mounts the live segmentation control inside the Segmenting card (Story 17.5)", async () => {
    render(<RecordingPane />);

    // The Segmenting card is not a placeholder — it hosts the shared
    // segment-size + duration-cap control, hydrated from the store.
    expect(await screen.findByLabelText(SEGMENT_SIZE_LABEL)).toHaveValue(500);
    expect(screen.getByLabelText(DURATION_CAP_LABEL)).toHaveValue(30);
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
  });

  it("mounts the destination chooser and the collapsed Advanced group (Story 19.5)", async () => {
    render(<RecordingPane />);

    // The Destination card shows the Rust-resolved effective folder.
    expect(await screen.findByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
    // The Advanced card renders collapsed — fps hidden behind the disclosure.
    expect(screen.getByTestId(ADVANCED_TOGGLE_TESTID)).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId(FPS_SELECT_TESTID)).not.toBeInTheDocument();
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
  });

  it("mounts the live system-audio control inside the Audio card, checked by default (Story 19.2)", async () => {
    render(<RecordingPane />);

    // The Audio card is not a placeholder — it hosts the system-audio Switch.
    expect(await screen.findByText(SYSTEM_AUDIO_LABEL)).toBeInTheDocument();
    expect(screen.getByTestId(SYSTEM_AUDIO_SWITCH_TESTID)).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
  });

  it("disables Start and names the blocking permission until the grant is green", async () => {
    render(<RecordingPane />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL })).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
    expect(
      screen.getByText(startBlockedNote(SCREEN_RECORDING_PERMISSION_NAME)),
    ).toBeInTheDocument();
  });

  it("enables Start (and drops the blocking note) once granted", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
    expect(
      screen.queryByText(startBlockedNote(SCREEN_RECORDING_PERMISSION_NAME)),
    ).not.toBeInTheDocument();
    // Granted needs no action affordance.
    expect(
      screen.queryByRole("button", { name: REQUEST_PERMISSION_LABEL }),
    ).not.toBeInTheDocument();
  });

  it("request → granted flips the row and unlocks Start", async () => {
    render(<RecordingPane />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL })).toBeInTheDocument(),
    );

    fireEvent.click(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL }));

    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
    expect(mockRequest).toHaveBeenCalledTimes(1);
  });

  it("denied offers the System Settings deep link and keeps Start gated", async () => {
    mockFetch.mockResolvedValue(DENIED);
    render(<RecordingPane />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: OPEN_SETTINGS_LABEL })).toBeInTheDocument(),
    );
    fireEvent.click(screen.getByRole("button", { name: OPEN_SETTINGS_LABEL }));
    expect(mockOpenSettings).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
  });

  it("a failed pre-flight falls back to the safe default (Start disabled, request offered)", async () => {
    mockFetch.mockRejectedValue({
      code: "internal",
      message: "keeper-rec did not answer",
      accountId: null,
      retriable: false,
    });
    render(<RecordingPane />);

    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
    expect(
      await screen.findByRole("button", { name: REQUEST_PERMISSION_LABEL }),
    ).toBeInTheDocument();
  });

  // --- Microphone / Camera pre-flight rows (Story 20.2) --------------------

  it("renders no Microphone or Camera permission row while both sources are off", async () => {
    render(<RecordingPane />);

    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
    expect(screen.getByTestId(permissionRowTestId(SCREEN_RECORDING_PERMISSION_NAME))).toBeVisible();
    expect(
      screen.queryByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME)),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId(permissionRowTestId(CAMERA_PERMISSION_NAME)),
    ).not.toBeInTheDocument();
    // Both sources off → the probe reports both legs disabled.
    expect(mockFetch).toHaveBeenLastCalledWith(false, false);
  });

  it("an enabled+denied mic renders its row, blocks Start, and names Microphone", async () => {
    setMicEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "granted",
      microphone: "denied",
      camera: null,
      canStart: false,
    });
    render(<RecordingPane />);

    const micRow = await screen.findByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME));
    expect(within(micRow).getByText(MICROPHONE_ROW_NOTE)).toBeInTheDocument();
    expect(mockFetch).toHaveBeenLastCalledWith(true, false);
    // Denied → the deep link goes to the Microphone pane.
    fireEvent.click(within(micRow).getByRole("button", { name: OPEN_SETTINGS_LABEL }));
    await waitFor(() => expect(mockOpenMicSettings).toHaveBeenCalledTimes(1));
    // Start is disabled and the note names the mic (screen is green).
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
    expect(screen.getByText(startBlockedNote(MICROPHONE_PERMISSION_NAME))).toBeInTheDocument();
    // No Camera row — the webcam stayed off.
    expect(
      screen.queryByTestId(permissionRowTestId(CAMERA_PERMISSION_NAME)),
    ).not.toBeInTheDocument();
  });

  it("an enabled+denied camera deep-links to the Camera pane (Story 20.2)", async () => {
    setWebcamEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "granted",
      microphone: null,
      camera: "denied",
      canStart: false,
    });
    render(<RecordingPane />);

    const cameraRow = await screen.findByTestId(permissionRowTestId(CAMERA_PERMISSION_NAME));
    fireEvent.click(within(cameraRow).getByRole("button", { name: OPEN_SETTINGS_LABEL }));
    await waitFor(() => expect(mockOpenCameraSettings).toHaveBeenCalledTimes(1));
  });

  it("an enabled+not-requested camera renders its row with the request action and names Camera", async () => {
    setWebcamEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "granted",
      microphone: null,
      camera: "notYetRequested",
      canStart: false,
    });
    render(<RecordingPane />);

    const cameraRow = await screen.findByTestId(permissionRowTestId(CAMERA_PERMISSION_NAME));
    expect(within(cameraRow).getByText(CAMERA_ROW_NOTE)).toBeInTheDocument();
    expect(mockFetch).toHaveBeenLastCalledWith(false, true);
    // The camera is the last (and only) blocker: Start names Camera.
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
    expect(screen.getByText(startBlockedNote(CAMERA_PERMISSION_NAME))).toBeInTheDocument();

    // The row's explicit request action fires the existing camera request
    // command, then re-probes live (mocked granted here).
    mockFetch.mockResolvedValue({ ...GRANTED, camera: "granted" });
    fireEvent.click(within(cameraRow).getByRole("button", { name: REQUEST_PERMISSION_LABEL }));
    await waitFor(() => expect(mockRequestCamera).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
  });

  it("the Microphone row's request action fires the mic request then re-probes (Story 20.2)", async () => {
    setMicEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "granted",
      microphone: "notYetRequested",
      camera: null,
      canStart: false,
    });
    render(<RecordingPane />);

    const micRow = await screen.findByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME));
    mockFetch.mockResolvedValue({ ...GRANTED, microphone: "granted" });
    fireEvent.click(within(micRow).getByRole("button", { name: REQUEST_PERMISSION_LABEL }));

    await waitFor(() => expect(mockRequestMic).toHaveBeenCalledTimes(1));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
  });

  it("Screen Recording outranks source legs in the blocked-Start note", async () => {
    // Screen denied while the enabled mic is granted: the note must name the
    // highest-priority blocker — Screen Recording, never the mic.
    setMicEnabled(true);
    mockFetch.mockResolvedValue({
      screenRecording: "denied",
      microphone: "granted",
      camera: null,
      canStart: false,
    });
    render(<RecordingPane />);

    await screen.findByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME));
    expect(
      screen.getByText(startBlockedNote(SCREEN_RECORDING_PERMISSION_NAME)),
    ).toBeInTheDocument();
    expect(
      screen.queryByText(startBlockedNote(MICROPHONE_PERMISSION_NAME)),
    ).not.toBeInTheDocument();
  });

  it("toggling the mic on re-probes and mounts the Microphone row live (Story 20.2)", async () => {
    render(<RecordingPane />);
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());
    expect(
      screen.queryByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME)),
    ).not.toBeInTheDocument();

    // Enabling the mic on the Audio card fires the lazy request (mocked
    // granted) AND flips the store — the permission hook re-probes with the
    // mic flag and the row appears without any focus/return round-trip.
    mockFetch.mockResolvedValue({ ...GRANTED, microphone: "granted" });
    fireEvent.click(await screen.findByTestId(MIC_SWITCH_TESTID));

    expect(
      await screen.findByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME)),
    ).toBeVisible();
    expect(mockFetch).toHaveBeenLastCalledWith(true, false);
  });

  it("re-probes after the OS prompt resolves so Start reflects the grant with no focus event (Story 20.2)", async () => {
    render(<RecordingPane />);
    await waitFor(() => expect(mockFetch).toHaveBeenCalled());

    // Model the real ordering the enable triggers: the enable-time probe (fired
    // the instant the store flips) reads the mic as not-yet-requested because the
    // OS prompt is still on screen; only once the prompt resolves does the
    // post-prompt re-sync probe read the grant. No focus/visibility event fires
    // in this test — the row + Start must still end up granted, proving the
    // re-sync is wired to the prompt's resolution, not an incidental refocus.
    mockFetch.mockReset();
    mockFetch
      .mockResolvedValueOnce({
        screenRecording: "granted",
        microphone: "notYetRequested",
        camera: null,
        canStart: false,
      })
      .mockResolvedValue({ ...GRANTED, microphone: "granted" });
    mockRequestMic.mockResolvedValue("granted");

    fireEvent.click(await screen.findByTestId(MIC_SWITCH_TESTID));

    await screen.findByTestId(permissionRowTestId(MICROPHONE_PERMISSION_NAME));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
    // Both post-enable probes carried the mic flag on — never a stale mic-off arg
    // from a captured closure (the regression this re-sync path guards against).
    expect(mockFetch.mock.calls.length).toBeGreaterThanOrEqual(2);
    for (const call of mockFetch.mock.calls) {
      expect(call).toEqual([true, false]);
    }
  });

  // --- Live session (Story 16.6) ------------------------------------------

  it("Start mounts the pinned active-recording banner (Recording + Stop)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    // The live dot/elapsed/Stop cluster now lives in the banner (Story 18.3),
    // not the header — the Start affordance is gone while live.
    expect(await screen.findByText("Recording")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: STOP_RECORDING_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Segment size" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: START_RECORDING_LABEL })).not.toBeInTheDocument();
    expect(mockStart).toHaveBeenCalledTimes(1);
  });

  it("Start threads the system-audio toggle's current value (Story 19.2)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    // Default on: the first arg is the default target, the second `true`; the
    // mic defaults off (Story 19.3): `false` + `null` (system default input);
    // the webcam defaults off too (Story 20.1): `false` + `null`.
    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart).toHaveBeenLastCalledWith(expect.anything(), true, false, null, false, null);
  });

  it("Start carries an off system-audio toggle through to recording_start", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    const toggle = await screen.findByTestId(SYSTEM_AUDIO_SWITCH_TESTID);
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "false");

    const startButton = screen.getByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart).toHaveBeenLastCalledWith(expect.anything(), false, false, null, false, null);
  });

  it("Start threads an enabled mic through to recording_start (Story 19.3)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    // Enable the mic on the Audio card (this also fires the lazy permission
    // request — mocked granted); the device stays the system default (null).
    const micToggle = await screen.findByTestId(MIC_SWITCH_TESTID);
    fireEvent.click(micToggle);
    expect(micToggle).toHaveAttribute("aria-checked", "true");

    const startButton = screen.getByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart).toHaveBeenLastCalledWith(expect.anything(), true, true, null, false, null);
  });

  it("Start threads an enabled webcam through to recording_start (Story 20.1)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    // Enable the webcam on the Webcam card (this also fires the lazy camera
    // permission request — mocked granted); the device stays the system
    // default camera (null).
    const webcamToggle = await screen.findByTestId(WEBCAM_SWITCH_TESTID);
    fireEvent.click(webcamToggle);
    expect(webcamToggle).toHaveAttribute("aria-checked", "true");

    const startButton = screen.getByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart).toHaveBeenLastCalledWith(expect.anything(), true, false, null, true, null);
  });

  it("Stop requests the graceful stop", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue(RECORDING_STATUS);
    render(<RecordingPane />);

    fireEvent.click(await screen.findByRole("button", { name: STOP_RECORDING_LABEL }));
    await waitFor(() => expect(mockStop).toHaveBeenCalledTimes(1));
  });

  it("a finalized session renders the completion card with count, size, path, and Reveal", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "finalized",
    });
    render(<RecordingPane />);

    // The completion card renders the manifest-authoritative count · size.
    expect(await screen.findByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.getByText(RECORDING_STATUS.outputPath ?? "")).toBeInTheDocument();
    // Reveal in Finder opens the folder (capability on).
    fireEvent.click(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL }));
    await waitFor(() => expect(mockReveal).toHaveBeenCalledWith(RECORDING_STATUS.outputPath));
    // Back to the Start affordance (a terminal state is not live).
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeInTheDocument();
  });

  it("degrades the completion card to folder + Reveal (no '0 segments · 0 MB') when the summary fetch fails", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "finalized",
    });
    mockSummary.mockRejectedValue(new Error("manifest load failed"));
    render(<RecordingPane />);

    // Honest degrade: the outcome headline + folder + Reveal, never a fabricated
    // "Saved 0 segments · 0 MB" for a session that really did save.
    expect(await screen.findByText("Recording saved")).toBeInTheDocument();
    expect(screen.queryByText(/0 segments/)).not.toBeInTheDocument();
    expect(screen.queryByText(/0 MB/)).not.toBeInTheDocument();
    expect(screen.getByText(RECORDING_STATUS.outputPath ?? "")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: REVEAL_IN_FINDER_LABEL })).toBeInTheDocument();
  });

  it("hides Reveal in Finder on the completion card when the capability is off", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true, revealInFileManager: false });
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "finalized",
    });
    render(<RecordingPane />);

    expect(await screen.findByText(/Saved 3 segments · 412 MB/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: REVEAL_IN_FINDER_LABEL })).not.toBeInTheDocument();
  });

  it("surfaces an unacknowledged recovered session as a warning card and dismisses it", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue(IDLE_STATUS);
    mockRecoveredList.mockResolvedValue([
      {
        sessionFolder: "/Users/alice/Movies/keeper/keeper-rec recovered",
        screenSegmentCount: 2,
        totalBytes: 200_000_000,
      },
    ]);
    render(<RecordingPane />);

    const card = await screen.findByText(/A recording was interrupted; 2 segments were saved/);
    // The bridge-degraded warning edge (the bridge-card recipe).
    const cardRoot = card.closest("[data-slot='card']");
    expect(cardRoot?.className).toContain("border-bridge-degraded/50");

    fireEvent.click(screen.getByRole("button", { name: RECOVERY_DISMISS_LABEL }));
    // Latched in the Rust seen-set + removed from the surface.
    await waitFor(() =>
      expect(mockRecoveredAck).toHaveBeenCalledWith(
        "/Users/alice/Movies/keeper/keeper-rec recovered",
      ),
    );
    await waitFor(() =>
      expect(screen.queryByText(/A recording was interrupted/)).not.toBeInTheDocument(),
    );
  });

  it("a failed start surfaces the banner error variant (not a header note)", async () => {
    // Story 18.4: the failed note moved from the pane header into the banner's
    // error variant — one surface, with Restart + Dismiss beside the reason.
    mockFetch.mockResolvedValue(GRANTED);
    mockStart.mockRejectedValue({ message: "keeper-rec could not spawn" });
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Recording failed — keeper-rec could not spawn",
    );
    expect(screen.getByRole("button", { name: BANNER_RESTART_LABEL })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: BANNER_DISMISS_LABEL })).toBeInTheDocument();
  });

  it("a failed session's banner shows the Rust-reported reason (poll path)", async () => {
    // A mid-session fault lands on the polled snapshot: failed + error.
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "failed",
      error: "keeper-rec exited unexpectedly",
    });
    render(<RecordingPane />);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Recording failed — keeper-rec exited unexpectedly",
    );
  });

  it("Restart re-invokes Start with the current capture selections (Story 18.4)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStart.mockRejectedValue({ message: "keeper-rec could not spawn" });
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    const restart = await screen.findByRole("button", { name: BANNER_RESTART_LABEL });
    mockStart.mockResolvedValue(RECORDING_STATUS);
    // Keep the 1 s poll on the live snapshot too, so the fresh session stays
    // rendered while the assertions below run.
    mockStatus.mockResolvedValue(RECORDING_STATUS);
    fireEvent.click(restart);

    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(2));
    // Restart reads the same capture stores the Start button does, so within
    // one mount it carries the identical target/audio/mic tuple the first Start
    // sent (default target, system audio on, mic off) — and, reading the stores
    // rather than a per-mount ref, it survives a view remount too.
    expect(mockStart.mock.calls[1]).toEqual(mockStart.mock.calls[0]);
    // The error surfaces clear: the banner is back to the live variant (Stop
    // present, Restart gone).
    expect(await screen.findByRole("button", { name: STOP_RECORDING_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: BANNER_RESTART_LABEL })).not.toBeInTheDocument();
  });

  it("Restart uses the current store selection even across a view remount (Story 18.4)", async () => {
    // The regression this guards: a fault, then the user leaves and reopens the
    // Recording view (the hook remounts, its local state resets) and clicks
    // Restart. Restart must record the user's chosen source + mic + webcam —
    // read live from the remount-surviving stores — never silently revert to
    // the main-display / mic-off / webcam-off defaults.
    mockFetch.mockResolvedValue(GRANTED);
    // A non-default selection sitting in the stores (they outlive a remount).
    const chosenTarget = { kind: "display", displayId: 2 } as const;
    selectRecordingTarget(chosenTarget);
    setMicEnabled(true);
    setWebcamEnabled(true);
    // The remounted view adopts the ALREADY-failed session from Rust — no Start
    // ran this mount, so a per-mount arg ref would have nothing to replay.
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "failed",
      error: "keeper-rec exited unexpectedly",
    });
    render(<RecordingPane />);

    fireEvent.click(await screen.findByRole("button", { name: BANNER_RESTART_LABEL }));

    // The chosen display + mic-on + webcam-on reach recording_start — not
    // the defaults.
    await waitFor(() => expect(mockStart).toHaveBeenCalledTimes(1));
    expect(mockStart).toHaveBeenLastCalledWith(chosenTarget, true, true, null, true, null);
  });

  it("Dismiss acknowledges the failed session back to idle (Story 18.4)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStart.mockRejectedValue({ message: "keeper-rec could not spawn" });
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    fireEvent.click(await screen.findByRole("button", { name: BANNER_DISMISS_LABEL }));
    await waitFor(() => expect(mockAcknowledge).toHaveBeenCalledTimes(1));

    // The returned idle snapshot is adopted: the banner clears and the idle
    // Start affordance is back.
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeInTheDocument();
  });
});
