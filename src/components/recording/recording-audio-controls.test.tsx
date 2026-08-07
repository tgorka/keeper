import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The mic Switch's lazy permission request (Story 19.3).
  requestMicrophonePermission: vi.fn(),
  // Imported by the recording-source store module (not called here — the
  // Audio card never polls; the Source card owns the poll).
  listRecordingSources: vi.fn(),
  // Story 22.7: the persisted echo-cancellation setting the card hydrates
  // from and commits through (the shared recording-settings mirror).
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

import {
  ECHO_CANCELLATION_COST_NOTE,
  ECHO_CANCELLATION_LABEL,
  ECHO_CANCELLATION_SWITCH_TESTID,
  MIC_DEFAULT_DEVICE_LABEL,
  MIC_DEVICE_SELECT_TESTID,
  MIC_OFF_NOTE,
  MIC_PERMISSION_DENIED_NOTE,
  MIC_PERMISSION_GRANTED_NOTE,
  MIC_SWITCH_TESTID,
  RecordingAudioControls,
} from "@/components/recording/recording-audio-controls";
import {
  type RecordingSettingsVm,
  recordingSettingsGet,
  recordingSettingsSet,
  requestMicrophonePermission,
} from "@/lib/ipc/client";
import { resetRecordingAudioForTest, systemAudioEnabled } from "@/lib/stores/recording-audio";
import {
  micDeviceId,
  micEnabled,
  resetRecordingMicForTest,
  setMicDeviceId,
} from "@/lib/stores/recording-mic";
import { resetRecordingSettingsForTest } from "@/lib/stores/recording-settings";
import { recordingSourceStore, resetRecordingSourceForTest } from "@/lib/stores/recording-source";

const mockRequestMic = vi.mocked(requestMicrophonePermission);
const mockSettingsGet = vi.mocked(recordingSettingsGet);
const mockSettingsSet = vi.mocked(recordingSettingsSet);

/** The effective VM a fresh install reads — echo cancellation ON (Story 22.7). */
const DEFAULT_SETTINGS: RecordingSettingsVm = {
  segmentMb: 500,
  durationCapMinutes: 30,
  destinationDir: "/Users/dev/Movies/keeper",
  destinationKind: "folder",
  destinationProfileId: null,
  destinationProfileName: null,
  fps: 30,
  codec: "h264",
  scalePercent: 100,
  echoCancellation: false,
  pathTemplate: "{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}",
};

beforeEach(() => {
  mockRequestMic.mockReset();
  mockRequestMic.mockResolvedValue("granted");
  mockSettingsGet.mockReset();
  mockSettingsGet.mockResolvedValue(DEFAULT_SETTINGS);
  mockSettingsSet.mockReset();
  mockSettingsSet.mockImplementation(async (settings) => settings);
});

afterEach(() => {
  resetRecordingAudioForTest();
  resetRecordingMicForTest();
  resetRecordingSourceForTest();
  resetRecordingSettingsForTest();
  vi.clearAllMocks();
});

describe("RecordingAudioControls", () => {
  it("renders the System-audio switch checked by default with the content-audio caption", () => {
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId("system-audio-switch");
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("System audio")).toBeInTheDocument();
    expect(screen.getByText("The audio the recorded content plays.")).toBeInTheDocument();
  });

  it("discloses separate tracks and keeper's excluded sounds while on", () => {
    render(<RecordingAudioControls />);

    expect(screen.getByText(/separate tracks, never mixed/)).toBeInTheDocument();
    expect(screen.getByText(/keeper's own notification sounds are excluded/)).toBeInTheDocument();
    expect(screen.queryByText(/no content audio/)).not.toBeInTheDocument();
  });

  it("turning the switch off updates the store and shows the honest off-state line", () => {
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId("system-audio-switch");
    fireEvent.click(toggle);

    expect(systemAudioEnabled()).toBe(false);
    expect(
      screen.getByText("System audio is off. The recording will have no content audio."),
    ).toBeInTheDocument();
    // The "on" disclosure is gone — no claim of a recorded track while off.
    expect(screen.queryByText(/separate tracks, never mixed/)).not.toBeInTheDocument();
  });

  it("turning the switch back on restores the on-state disclosure", () => {
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId("system-audio-switch");
    fireEvent.click(toggle);
    fireEvent.click(toggle);

    expect(systemAudioEnabled()).toBe(true);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByText(/no content audio/)).not.toBeInTheDocument();
  });

  // --- The microphone row (Story 19.3) ------------------------------------

  it("renders the mic switch OFF by default and requests no permission on render", () => {
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId(MIC_SWITCH_TESTID);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(micEnabled()).toBe(false);
    // The lazy-permission contract (FR-69, AD-36): nothing fires on render.
    expect(mockRequestMic).not.toHaveBeenCalled();
  });

  it("greys the device picker with the helper caption while the mic is off", () => {
    render(<RecordingAudioControls />);

    const picker = screen.getByTestId(MIC_DEVICE_SELECT_TESTID);
    expect(picker).toBeDisabled();
    // "System default input" is the default selection.
    expect(picker).toHaveTextContent(MIC_DEFAULT_DEVICE_LABEL);
    expect(screen.getByText(MIC_OFF_NOTE)).toBeInTheDocument();
    expect(micDeviceId()).toBeNull();
  });

  it("enabling the mic requests permission exactly once and shows the granted caption", async () => {
    render(<RecordingAudioControls />);

    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));

    expect(micEnabled()).toBe(true);
    expect(mockRequestMic).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(MIC_PERMISSION_GRANTED_NOTE)).toBeInTheDocument();
    // The picker is live now, the off-note gone.
    expect(screen.getByTestId(MIC_DEVICE_SELECT_TESTID)).toBeEnabled();
    expect(screen.queryByText(MIC_OFF_NOTE)).not.toBeInTheDocument();
  });

  it("a denied permission surfaces the honest denied caption (Start blocked, fix path named)", async () => {
    mockRequestMic.mockResolvedValue("denied");
    render(<RecordingAudioControls />);

    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));

    expect(await screen.findByRole("alert")).toHaveTextContent(MIC_PERMISSION_DENIED_NOTE);
    // The toggle stays on — Start is blocked upstream by the pre-flight
    // (Story 20.2) until the grant lands or the mic is turned back off.
    expect(micEnabled()).toBe(true);
  });

  it("a late resolution from a superseded enable never overwrites the current outcome", async () => {
    // Rapid on→off→on fires overlapping permission requests; the first one
    // must not win when it resolves after a newer toggle (stale caption).
    let resolveA!: (status: "granted" | "denied") => void;
    let resolveB!: (status: "granted" | "denied") => void;
    mockRequestMic
      .mockImplementationOnce(() => new Promise((resolve) => (resolveA = resolve)))
      .mockImplementationOnce(() => new Promise((resolve) => (resolveB = resolve)));
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId(MIC_SWITCH_TESTID);
    fireEvent.click(toggle); // enable → request A in flight
    await waitFor(() => expect(mockRequestMic).toHaveBeenCalledTimes(1));
    fireEvent.click(toggle); // disable
    fireEvent.click(toggle); // enable → request B in flight
    await waitFor(() => expect(mockRequestMic).toHaveBeenCalledTimes(2));

    resolveB("denied");
    expect(await screen.findByRole("alert")).toHaveTextContent(MIC_PERMISSION_DENIED_NOTE);

    resolveA("granted"); // stale resolution from the superseded first enable
    await waitFor(() =>
      expect(screen.queryByText(MIC_PERMISSION_GRANTED_NOTE)).not.toBeInTheDocument(),
    );
    expect(screen.getByText(MIC_PERMISSION_DENIED_NOTE)).toBeInTheDocument();
  });

  it("a failed permission round-trip makes no claim either way", async () => {
    mockRequestMic.mockRejectedValue({ message: "keeper-rec did not answer" });
    render(<RecordingAudioControls />);

    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));

    await waitFor(() => expect(mockRequestMic).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(MIC_PERMISSION_GRANTED_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByText(MIC_PERMISSION_DENIED_NOTE)).not.toBeInTheDocument();
  });

  it("disabling the mic restores the off note and never re-requests", async () => {
    render(<RecordingAudioControls />);

    const toggle = screen.getByTestId(MIC_SWITCH_TESTID);
    fireEvent.click(toggle);
    await waitFor(() => expect(mockRequestMic).toHaveBeenCalledTimes(1));
    fireEvent.click(toggle);

    expect(micEnabled()).toBe(false);
    expect(screen.getByText(MIC_OFF_NOTE)).toBeInTheDocument();
    expect(screen.getByTestId(MIC_DEVICE_SELECT_TESTID)).toBeDisabled();
    // Turning OFF is never a permission trigger.
    expect(mockRequestMic).toHaveBeenCalledTimes(1);
  });

  it("renders enumerated devices in the picker under the default option", async () => {
    // The mirrored source list carries the sidecar-enumerated microphones.
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [{ id: "X", name: "USB Microphone" }],
      cameras: [],
    });
    render(<RecordingAudioControls />);

    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));
    const picker = screen.getByTestId(MIC_DEVICE_SELECT_TESTID);
    expect(picker).toBeEnabled();
    // The default remains selected until the user picks a device; the
    // enumerated device is offered as an option (Radix renders options into
    // the trigger's listbox on open — asserting the closed trigger still shows
    // the default keeps this jsdom-safe).
    expect(picker).toHaveTextContent(MIC_DEFAULT_DEVICE_LABEL);
    expect(micDeviceId()).toBeNull();
  });

  // --- Pre-Start mic reconciliation (Story 19.4) ---------------------------

  it("reconciles a vanished selected mic back to the system default input", async () => {
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [{ id: "X", name: "USB Microphone" }],
      cameras: [],
    });
    setMicDeviceId("X");
    render(<RecordingAudioControls />);
    // While the device is still enumerated the selection stays.
    expect(micDeviceId()).toBe("X");

    // The device disappears from the next poll → the picker reconciles to
    // "System default input" (`null`), so Start ships no dead id.
    act(() => {
      recordingSourceStore.getState().setSources({
        displays: [],
        applications: [],
        microphones: [],
        cameras: [],
      });
    });

    await waitFor(() => expect(micDeviceId()).toBeNull());
    expect(screen.getByTestId(MIC_DEVICE_SELECT_TESTID)).toHaveTextContent(
      MIC_DEFAULT_DEVICE_LABEL,
    );
    // Reconciliation is never a permission trigger (the mic was never enabled).
    expect(mockRequestMic).not.toHaveBeenCalled();
  });

  it("never resets a real selection before the first enumeration lands", () => {
    // `sources: null` (never polled) must not be read as "the device vanished".
    setMicDeviceId("X");
    render(<RecordingAudioControls />);
    expect(micDeviceId()).toBe("X");
  });

  // --- Echo cancellation (Story 22.7) -------------------------------------

  /** Render with the mic enabled and the persisted settings hydrated — the
   * only state in which the echo-cancellation switch is interactive. */
  async function renderWithLiveMic(active = true) {
    render(<RecordingAudioControls active={active} />);
    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));
    await waitFor(() => expect(mockSettingsGet).toHaveBeenCalled());
    const toggle = screen.getByTestId(ECHO_CANCELLATION_SWITCH_TESTID);
    return toggle;
  }

  it("renders the echo-cancellation switch OFF by default with the honest cost note", async () => {
    const toggle = await renderWithLiveMic();

    await waitFor(() => expect(toggle).toBeEnabled());
    // Opt-in (owner decision 2026-08-05): the cancellation works, but it costs a
    // mono track and non-defeatable voice-band noise suppression, so a fresh
    // install records the microphone exactly as it always did.
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(ECHO_CANCELLATION_LABEL)).toBeInTheDocument();
    // The costs are never hidden: mono + non-defeatable noise suppression.
    expect(screen.getByText(new RegExp(ECHO_CANCELLATION_COST_NOTE))).toBeInTheDocument();
  });

  it("is disabled until the settings hydration lands (never a fake value)", () => {
    // The first render happens before `recordingSettingsGet` resolves.
    render(<RecordingAudioControls />);
    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));
    expect(screen.getByTestId(ECHO_CANCELLATION_SWITCH_TESTID)).toBeDisabled();
  });

  it("turning it on persists through recordingSettingsSet and reads back on", async () => {
    mockSettingsSet.mockResolvedValue({ ...DEFAULT_SETTINGS, echoCancellation: true });
    const toggle = await renderWithLiveMic();
    await waitFor(() => expect(toggle).toBeEnabled());

    fireEvent.click(toggle);

    await waitFor(() => expect(mockSettingsSet).toHaveBeenCalledTimes(1));
    expect(mockSettingsSet).toHaveBeenCalledWith({
      ...DEFAULT_SETTINGS,
      echoCancellation: true,
    });
    // The mirror now shows the EFFECTIVE (Rust-confirmed) value.
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "true"));
  });

  it("a rejected write reverts the switch to the last confirmed value", async () => {
    // The live-session guard: `recording_settings_set` refuses a CHANGED echo
    // cancellation while a recording runs, and writes nothing.
    mockSettingsSet.mockRejectedValue({
      code: "internal",
      message: "echo cancellation cannot be changed while a recording is running",
    });
    const toggle = await renderWithLiveMic();
    await waitFor(() => expect(toggle).toBeEnabled());

    fireEvent.click(toggle);

    await waitFor(() => expect(mockSettingsSet).toHaveBeenCalledTimes(1));
    // Optimism is rolled back — the UI never claims an unsaved value.
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
  });

  it("is disabled while the card is not active (a live session owns the mic)", async () => {
    const toggle = await renderWithLiveMic(false);

    // Hydration landed and the mic is on, so `active: false` is the only
    // reason left for the control to be inert.
    await waitFor(() => expect(mockSettingsGet).toHaveBeenCalled());
    expect(toggle).toBeDisabled();
  });

  it("is disabled while the microphone is off (there is no feed to process)", async () => {
    const toggle = await renderWithLiveMic();
    await waitFor(() => expect(toggle).toBeEnabled());

    // Turning the mic back off greys the echo switch with it.
    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));

    expect(micEnabled()).toBe(false);
    expect(toggle).toBeDisabled();
  });

  it("never touches the default (null) selection, even with no devices enumerated", () => {
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [],
      cameras: [],
    });
    render(<RecordingAudioControls />);
    expect(micDeviceId()).toBeNull();
    expect(screen.getByTestId(MIC_DEVICE_SELECT_TESTID)).toHaveTextContent(
      MIC_DEFAULT_DEVICE_LABEL,
    );
  });
});
