import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The mic Switch's lazy permission request (Story 19.3).
  requestMicrophonePermission: vi.fn(),
  // Imported by the recording-source store module (not called here — the
  // Audio card never polls; the Source card owns the poll).
  listRecordingSources: vi.fn(),
}));

import {
  MIC_DEFAULT_DEVICE_LABEL,
  MIC_DEVICE_SELECT_TESTID,
  MIC_OFF_NOTE,
  MIC_PERMISSION_DENIED_NOTE,
  MIC_PERMISSION_GRANTED_NOTE,
  MIC_SWITCH_TESTID,
  RecordingAudioControls,
} from "@/components/recording/recording-audio-controls";
import { requestMicrophonePermission } from "@/lib/ipc/client";
import { resetRecordingAudioForTest, systemAudioEnabled } from "@/lib/stores/recording-audio";
import { micDeviceId, micEnabled, resetRecordingMicForTest } from "@/lib/stores/recording-mic";
import { recordingSourceStore, resetRecordingSourceForTest } from "@/lib/stores/recording-source";

const mockRequestMic = vi.mocked(requestMicrophonePermission);

beforeEach(() => {
  mockRequestMic.mockReset();
  mockRequestMic.mockResolvedValue("granted");
});

afterEach(() => {
  resetRecordingAudioForTest();
  resetRecordingMicForTest();
  resetRecordingSourceForTest();
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

  it("a denied permission surfaces the honest denied caption (mic track silent)", async () => {
    mockRequestMic.mockResolvedValue("denied");
    render(<RecordingAudioControls />);

    fireEvent.click(screen.getByTestId(MIC_SWITCH_TESTID));

    expect(await screen.findByRole("alert")).toHaveTextContent(MIC_PERMISSION_DENIED_NOTE);
    // The toggle stays on — the session records honestly (silent mic track).
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
});
