import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

import {
  ADVANCED_DISCLOSURE_LABEL,
  ADVANCED_TOGGLE_TESTID,
  FPS_NEXT_SESSION_NOTE,
  FPS_SELECT_TESTID,
  RecordingAdvancedControls,
} from "@/components/recording/recording-advanced-controls";
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
  codec: "h264",
  scalePercent: 100,
};

beforeEach(() => {
  resetRecordingSettingsForTest();
  mockGet.mockReset();
  mockGet.mockResolvedValue(DEFAULTS);
  mockSet.mockReset();
  mockSet.mockImplementation((vm) => Promise.resolve(vm));
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("RecordingAdvancedControls", () => {
  it("renders collapsed by default with the fps control hidden", async () => {
    render(<RecordingAdvancedControls />);

    const toggle = screen.getByTestId(ADVANCED_TOGGLE_TESTID);
    expect(toggle).toHaveTextContent(ADVANCED_DISCLOSURE_LABEL);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByTestId(FPS_SELECT_TESTID)).not.toBeInTheDocument();
    // The store still hydrates lazily behind the collapsed group.
    await waitFor(() => expect(mockGet).toHaveBeenCalledTimes(1));
  });

  it("expands to reveal the fps select defaulting to 30, and collapses again", async () => {
    render(<RecordingAdvancedControls />);

    fireEvent.click(screen.getByTestId(ADVANCED_TOGGLE_TESTID));
    expect(screen.getByTestId(ADVANCED_TOGGLE_TESTID)).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => expect(screen.getByTestId(FPS_SELECT_TESTID)).toHaveTextContent("30"));
    expect(screen.getByText(FPS_NEXT_SESSION_NOTE)).toBeInTheDocument();

    fireEvent.click(screen.getByTestId(ADVANCED_TOGGLE_TESTID));
    expect(screen.queryByTestId(FPS_SELECT_TESTID)).not.toBeInTheDocument();
  });

  it("offers exactly 30 and 60 and persists a picked 60", async () => {
    render(<RecordingAdvancedControls />);
    fireEvent.click(screen.getByTestId(ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(screen.getByTestId(FPS_SELECT_TESTID)).toBeEnabled());

    // Open the Radix select via keyboard (jsdom has no real pointer stack).
    fireEvent.keyDown(screen.getByTestId(FPS_SELECT_TESTID), { key: "Enter" });
    const options = await screen.findAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual(["30", "60"]);

    const sixty = await screen.findByRole("option", { name: "60" });
    // Radix SelectItem commits on Enter/Space keydown (jsdom has no real
    // pointer stack, so the keyboard path is the reliable one).
    fireEvent.keyDown(sixty, { key: "Enter" });

    await waitFor(() => expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, fps: 60 }));
    await waitFor(() => expect(screen.getByTestId(FPS_SELECT_TESTID)).toHaveTextContent("60"));
  });

  it("mirrors a persisted 60 from the shared store", async () => {
    mockGet.mockResolvedValue({ ...DEFAULTS, fps: 60 });
    render(<RecordingAdvancedControls />);

    fireEvent.click(screen.getByTestId(ADVANCED_TOGGLE_TESTID));
    await waitFor(() => expect(screen.getByTestId(FPS_SELECT_TESTID)).toHaveTextContent("60"));
  });

  it("disables the fps select until the shared store hydrates", () => {
    mockGet.mockImplementation(() => new Promise(() => {}));
    render(<RecordingAdvancedControls />);

    fireEvent.click(screen.getByTestId(ADVANCED_TOGGLE_TESTID));
    expect(screen.getByTestId(FPS_SELECT_TESTID)).toBeDisabled();
  });
});
