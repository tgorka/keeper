import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingPermission: vi.fn(),
  requestScreenRecordingPermission: vi.fn(),
  openScreenRecordingSettings: vi.fn(),
  recordingStart: vi.fn(),
  recordingStop: vi.fn(),
  recordingStatus: vi.fn(),
}));

import {
  FINALIZED_NOTE_PREFIX,
  RecordingPane,
  START_BLOCKED_NOTE,
  START_RECORDING_LABEL,
  STOP_RECORDING_LABEL,
} from "@/components/layout/recording-pane";
import {
  OPEN_SETTINGS_LABEL,
  REQUEST_PERMISSION_LABEL,
} from "@/components/recording/recording-permission-row";
import type { RecordingPermissionVm, RecordingStatusVm } from "@/lib/ipc/client";
import {
  openScreenRecordingSettings,
  recordingPermission,
  recordingStart,
  recordingStatus,
  recordingStop,
  requestScreenRecordingPermission,
} from "@/lib/ipc/client";

const mockFetch = vi.mocked(recordingPermission);
const mockRequest = vi.mocked(requestScreenRecordingPermission);
const mockOpenSettings = vi.mocked(openScreenRecordingSettings);
const mockStart = vi.mocked(recordingStart);
const mockStop = vi.mocked(recordingStop);
const mockStatus = vi.mocked(recordingStatus);

const IDLE_STATUS: RecordingStatusVm = {
  state: "idle",
  segmentsClosed: 0,
  startedAtEpochMs: null,
  outputPath: null,
  error: null,
};

const RECORDING_STATUS: RecordingStatusVm = {
  state: "recording",
  segmentsClosed: 0,
  startedAtEpochMs: 1_700_000_000_000,
  outputPath: "/Users/alice/Movies/keeper/keeper-rec test.mp4",
  error: null,
};

const GRANTED: RecordingPermissionVm = { screenRecording: "granted", canStart: true };
const NOT_YET: RecordingPermissionVm = { screenRecording: "notYetRequested", canStart: false };
const DENIED: RecordingPermissionVm = { screenRecording: "denied", canStart: false };

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
});

afterEach(() => {
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

  it("disables Start and names the blocking permission until the grant is green", async () => {
    render(<RecordingPane />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL })).toBeInTheDocument(),
    );
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeDisabled();
    expect(screen.getByText(START_BLOCKED_NOTE)).toBeInTheDocument();
  });

  it("enables Start (and drops the blocking note) once granted", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    await waitFor(() =>
      expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeEnabled(),
    );
    expect(screen.queryByText(START_BLOCKED_NOTE)).not.toBeInTheDocument();
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

  // --- Live session (Story 16.6) ------------------------------------------

  it("Start flips the header into the live-session UI (red dot + Stop)", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    expect(await screen.findByRole("status", { name: "Recording active" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: STOP_RECORDING_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: START_RECORDING_LABEL })).not.toBeInTheDocument();
    expect(mockStart).toHaveBeenCalledTimes(1);
  });

  it("Stop requests the graceful stop", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue(RECORDING_STATUS);
    render(<RecordingPane />);

    fireEvent.click(await screen.findByRole("button", { name: STOP_RECORDING_LABEL }));
    await waitFor(() => expect(mockStop).toHaveBeenCalledTimes(1));
  });

  it("a finalized session renders the saved-file note with the path", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStatus.mockResolvedValue({
      ...RECORDING_STATUS,
      state: "finalized",
    });
    render(<RecordingPane />);

    expect(await screen.findByText(new RegExp(FINALIZED_NOTE_PREFIX))).toBeInTheDocument();
    expect(screen.getByText(RECORDING_STATUS.outputPath ?? "")).toBeInTheDocument();
    // Back to the Start affordance (a terminal state is not live).
    expect(screen.getByRole("button", { name: START_RECORDING_LABEL })).toBeInTheDocument();
  });

  it("a failed start surfaces the honest failure line", async () => {
    mockFetch.mockResolvedValue(GRANTED);
    mockStart.mockRejectedValue({ message: "keeper-rec could not spawn" });
    render(<RecordingPane />);

    const startButton = await screen.findByRole("button", { name: START_RECORDING_LABEL });
    await waitFor(() => expect(startButton).toBeEnabled());
    fireEvent.click(startButton);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Recording failed: keeper-rec could not spawn",
    );
  });
});
