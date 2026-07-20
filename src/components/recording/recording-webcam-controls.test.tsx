import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The webcam Switch's lazy permission request (Story 20.1).
  requestCameraPermission: vi.fn(),
  // Imported by the recording-source store module (not called here — the
  // Webcam card never polls; the Source card owns the poll).
  listRecordingSources: vi.fn(),
}));

import {
  CAMERA_DEFAULT_DEVICE_LABEL,
  CAMERA_DEVICE_SELECT_TESTID,
  CAMERA_PERMISSION_DENIED_NOTE,
  CAMERA_PERMISSION_GRANTED_NOTE,
  RecordingWebcamControls,
  WEBCAM_CAPTION,
  WEBCAM_DISCLOSURE,
  WEBCAM_OFF_NOTE,
  WEBCAM_SWITCH_TESTID,
} from "@/components/recording/recording-webcam-controls";
import { requestCameraPermission } from "@/lib/ipc/client";
import { recordingSourceStore, resetRecordingSourceForTest } from "@/lib/stores/recording-source";
import {
  cameraDeviceId,
  resetRecordingWebcamForTest,
  setCameraDeviceId,
  webcamEnabled,
} from "@/lib/stores/recording-webcam";

const mockRequestCamera = vi.mocked(requestCameraPermission);

beforeEach(() => {
  mockRequestCamera.mockReset();
  mockRequestCamera.mockResolvedValue("granted");
});

afterEach(() => {
  resetRecordingWebcamForTest();
  resetRecordingSourceForTest();
  vi.clearAllMocks();
});

describe("RecordingWebcamControls", () => {
  it("renders the webcam switch OFF by default and requests no permission on render", () => {
    render(<RecordingWebcamControls />);

    const toggle = screen.getByTestId(WEBCAM_SWITCH_TESTID);
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(webcamEnabled()).toBe(false);
    // The lazy-permission contract (FR-70, AD-36): nothing fires on render.
    expect(mockRequestCamera).not.toHaveBeenCalled();
    // The separate-file framing is always visible (FR-70).
    expect(screen.getByText(WEBCAM_CAPTION)).toBeInTheDocument();
  });

  it("greys the camera picker with the helper caption while the webcam is off", () => {
    render(<RecordingWebcamControls />);

    const picker = screen.getByTestId(CAMERA_DEVICE_SELECT_TESTID);
    expect(picker).toBeDisabled();
    // "System default camera" is the default selection.
    expect(picker).toHaveTextContent(CAMERA_DEFAULT_DEVICE_LABEL);
    expect(screen.getByText(WEBCAM_OFF_NOTE)).toBeInTheDocument();
    expect(cameraDeviceId()).toBeNull();
    // No no-burn-in claim while off — the disclosure belongs to the on state.
    expect(screen.queryByText(WEBCAM_DISCLOSURE)).not.toBeInTheDocument();
  });

  it("enabling the webcam requests permission exactly once and shows the granted caption", async () => {
    render(<RecordingWebcamControls />);

    fireEvent.click(screen.getByTestId(WEBCAM_SWITCH_TESTID));

    expect(webcamEnabled()).toBe(true);
    expect(mockRequestCamera).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(CAMERA_PERMISSION_GRANTED_NOTE)).toBeInTheDocument();
    // The picker is live now, the off-note gone, the presenter-overlay /
    // no-PiP disclosure shown (UX-DR34).
    expect(screen.getByTestId(CAMERA_DEVICE_SELECT_TESTID)).toBeEnabled();
    expect(screen.queryByText(WEBCAM_OFF_NOTE)).not.toBeInTheDocument();
    expect(screen.getByText(WEBCAM_DISCLOSURE)).toBeInTheDocument();
  });

  it("a denied permission surfaces the honest denied caption without blocking anything", async () => {
    mockRequestCamera.mockResolvedValue("denied");
    render(<RecordingWebcamControls />);

    fireEvent.click(screen.getByTestId(WEBCAM_SWITCH_TESTID));

    expect(await screen.findByRole("alert")).toHaveTextContent(CAMERA_PERMISSION_DENIED_NOTE);
    // The toggle stays on — the session records honestly (no camera file);
    // Start is never gated on the camera grant (the mic precedent).
    expect(webcamEnabled()).toBe(true);
  });

  it("a late resolution from a superseded enable never overwrites the current outcome", async () => {
    // Rapid on→off→on fires overlapping permission requests; the first one
    // must not win when it resolves after a newer toggle (stale caption).
    let resolveA!: (status: "granted" | "denied") => void;
    let resolveB!: (status: "granted" | "denied") => void;
    mockRequestCamera
      .mockImplementationOnce(() => new Promise((resolve) => (resolveA = resolve)))
      .mockImplementationOnce(() => new Promise((resolve) => (resolveB = resolve)));
    render(<RecordingWebcamControls />);

    const toggle = screen.getByTestId(WEBCAM_SWITCH_TESTID);
    fireEvent.click(toggle); // enable → request A in flight
    await waitFor(() => expect(mockRequestCamera).toHaveBeenCalledTimes(1));
    fireEvent.click(toggle); // disable
    fireEvent.click(toggle); // enable → request B in flight
    await waitFor(() => expect(mockRequestCamera).toHaveBeenCalledTimes(2));

    resolveB("denied");
    expect(await screen.findByRole("alert")).toHaveTextContent(CAMERA_PERMISSION_DENIED_NOTE);

    resolveA("granted"); // stale resolution from the superseded first enable
    await waitFor(() =>
      expect(screen.queryByText(CAMERA_PERMISSION_GRANTED_NOTE)).not.toBeInTheDocument(),
    );
    expect(screen.getByText(CAMERA_PERMISSION_DENIED_NOTE)).toBeInTheDocument();
  });

  it("a failed permission round-trip makes no claim either way", async () => {
    mockRequestCamera.mockRejectedValue({ message: "keeper-rec did not answer" });
    render(<RecordingWebcamControls />);

    fireEvent.click(screen.getByTestId(WEBCAM_SWITCH_TESTID));

    await waitFor(() => expect(mockRequestCamera).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(CAMERA_PERMISSION_GRANTED_NOTE)).not.toBeInTheDocument();
    expect(screen.queryByText(CAMERA_PERMISSION_DENIED_NOTE)).not.toBeInTheDocument();
  });

  it("disabling the webcam restores the off note and never re-requests", async () => {
    render(<RecordingWebcamControls />);

    const toggle = screen.getByTestId(WEBCAM_SWITCH_TESTID);
    fireEvent.click(toggle);
    await waitFor(() => expect(mockRequestCamera).toHaveBeenCalledTimes(1));
    fireEvent.click(toggle);

    expect(webcamEnabled()).toBe(false);
    expect(screen.getByText(WEBCAM_OFF_NOTE)).toBeInTheDocument();
    expect(screen.getByTestId(CAMERA_DEVICE_SELECT_TESTID)).toBeDisabled();
    // Turning OFF is never a permission trigger.
    expect(mockRequestCamera).toHaveBeenCalledTimes(1);
  });

  it("renders enumerated cameras in the flat picker under the default option", async () => {
    // The mirrored source list carries the sidecar-enumerated cameras — a
    // flat name list, no device-class grouping (the intent contract).
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [],
      cameras: [{ id: "X", name: "FaceTime HD Camera" }],
    });
    render(<RecordingWebcamControls />);

    fireEvent.click(screen.getByTestId(WEBCAM_SWITCH_TESTID));
    const picker = screen.getByTestId(CAMERA_DEVICE_SELECT_TESTID);
    expect(picker).toBeEnabled();
    // The default remains selected until the user picks a device; the
    // enumerated device is offered as an option (Radix renders options into
    // the trigger's listbox on open — asserting the closed trigger still
    // shows the default keeps this jsdom-safe).
    expect(picker).toHaveTextContent(CAMERA_DEFAULT_DEVICE_LABEL);
    expect(cameraDeviceId()).toBeNull();
  });

  // --- Pre-Start camera reconciliation (the 19.4 mic pattern) -------------

  it("reconciles a vanished selected camera back to the system default", async () => {
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [],
      cameras: [{ id: "X", name: "FaceTime HD Camera" }],
    });
    setCameraDeviceId("X");
    render(<RecordingWebcamControls />);
    // While the device is still enumerated the selection stays.
    expect(cameraDeviceId()).toBe("X");

    // The device disappears from the next poll (a Continuity Camera walking
    // away pre-Start) → the picker reconciles to "System default camera"
    // (`null`), so Start ships no dead id.
    act(() => {
      recordingSourceStore.getState().setSources({
        displays: [],
        applications: [],
        microphones: [],
        cameras: [],
      });
    });

    await waitFor(() => expect(cameraDeviceId()).toBeNull());
    expect(screen.getByTestId(CAMERA_DEVICE_SELECT_TESTID)).toHaveTextContent(
      CAMERA_DEFAULT_DEVICE_LABEL,
    );
    // Reconciliation is never a permission trigger (the webcam was never
    // enabled here).
    expect(mockRequestCamera).not.toHaveBeenCalled();
  });

  it("never resets a real selection before the first enumeration lands", () => {
    // `sources: null` (never polled) must not read as "the device vanished".
    setCameraDeviceId("X");
    render(<RecordingWebcamControls />);
    expect(cameraDeviceId()).toBe("X");
  });

  it("freezes reconciliation while a session is live (active=false)", async () => {
    // The pause-while-live contract: the card stays mounted during a live
    // session, and a live-session poll must never silently reset the
    // selection mid-recording (the running sidecar owns the camera by then).
    recordingSourceStore.getState().setSources({
      displays: [],
      applications: [],
      microphones: [],
      cameras: [],
    });
    setCameraDeviceId("X");
    render(<RecordingWebcamControls active={false} />);

    // The id is absent from the enumeration, but the inactive card must not
    // touch the selection.
    await waitFor(() => expect(cameraDeviceId()).toBe("X"));
  });
});
