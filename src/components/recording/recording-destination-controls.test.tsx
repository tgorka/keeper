import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

// The OS-native directory picker (the export-dialog mock pattern).
const openFolder = vi.fn();
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: (...args: unknown[]) => openFolder(...args),
}));

import {
  CHOOSE_FOLDER_LABEL,
  DESTINATION_LOCAL_ONLY_NOTE,
  DESTINATION_NEXT_SESSION_NOTE,
  DESTINATION_PATH_TESTID,
  RecordingDestinationControls,
} from "@/components/recording/recording-destination-controls";
import type { RecordingSettingsVm } from "@/lib/ipc/client";
import { recordingSettingsGet, recordingSettingsSet } from "@/lib/ipc/client";
import { resetRecordingSettingsForTest } from "@/lib/stores/recording-settings";

const mockGet = vi.mocked(recordingSettingsGet);
const mockSet = vi.mocked(recordingSettingsSet);

const DEFAULTS: RecordingSettingsVm = {
  segmentMb: 500,
  durationCapMinutes: 30,
  destinationDir: "/Users/alice/Movies/keeper",
  fps: 30,
};

beforeEach(() => {
  resetRecordingSettingsForTest();
  mockGet.mockReset();
  mockGet.mockResolvedValue(DEFAULTS);
  mockSet.mockReset();
  mockSet.mockImplementation((vm) => Promise.resolve(vm));
  openFolder.mockReset();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("RecordingDestinationControls", () => {
  it("shows the effective folder with the next-session note and local-only copy", async () => {
    render(<RecordingDestinationControls />);

    // The Rust-resolved EFFECTIVE default is always a concrete folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Movies/keeper",
      ),
    );
    expect(screen.getByText(DESTINATION_NEXT_SESSION_NOTE)).toBeInTheDocument();
    expect(screen.getByText(DESTINATION_LOCAL_ONLY_NOTE)).toBeInTheDocument();
    // Local-only: no share/cloud/network affordance anywhere in the card.
    expect(screen.queryByText(/share|cloud|network|http/i)).not.toBeInTheDocument();
  });

  it("opens the native directory picker and persists a confirmed selection", async () => {
    openFolder.mockResolvedValue("/Users/alice/Recordings");
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalledWith({ directory: true }));
    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...DEFAULTS,
        destinationDir: "/Users/alice/Recordings",
      }),
    );
    // The card reflects the effective persisted folder.
    await waitFor(() =>
      expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
        "/Users/alice/Recordings",
      ),
    );
  });

  it("keeps the current folder when the picker is cancelled", async () => {
    // A cancelled native picker resolves `null` — no write, no change.
    openFolder.mockResolvedValue(null);
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
    expect(screen.getByTestId(DESTINATION_PATH_TESTID)).toHaveTextContent(
      "/Users/alice/Movies/keeper",
    );
  });

  it("keeps the current folder when the picker throws", async () => {
    openFolder.mockRejectedValue(new Error("picker unavailable"));
    render(<RecordingDestinationControls />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeEnabled(),
    );

    fireEvent.click(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL }));

    await waitFor(() => expect(openFolder).toHaveBeenCalled());
    expect(mockSet).not.toHaveBeenCalled();
  });

  it("disables the chooser until the shared store hydrates", () => {
    // Never-resolving hydration: the affordance must not pretend to work.
    mockGet.mockImplementation(() => new Promise(() => {}));
    render(<RecordingDestinationControls />);

    expect(screen.getByRole("button", { name: CHOOSE_FOLDER_LABEL })).toBeDisabled();
  });
});
