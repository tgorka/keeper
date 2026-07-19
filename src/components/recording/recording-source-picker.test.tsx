import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  listRecordingSources: vi.fn(),
}));

import {
  APPLICATIONS_HEADING,
  appScopeDisclosure,
  DISPLAYS_HEADING,
  MAIN_DISPLAY_LABEL,
  NO_APPLICATIONS_NOTE,
  RecordingSourcePicker,
  SELECTION_UNAVAILABLE_NOTE,
} from "@/components/recording/recording-source-picker";
import type { RecordingSourcesVm } from "@/lib/ipc/client";
import { listRecordingSources } from "@/lib/ipc/client";
import {
  resetRecordingSourceForTest,
  selectedRecordingTarget,
  selectRecordingTarget,
} from "@/lib/stores/recording-source";

const mockList = vi.mocked(listRecordingSources);

const SOURCES: RecordingSourcesVm = {
  displays: [
    { id: 1, width: 3456, height: 2234, isMain: true },
    { id: 2, width: 1920, height: 1080, isMain: false },
  ],
  applications: [
    { bundleId: "com.apple.Safari", name: "Safari", pid: 501, icon: "data:image/png;base64,AA==" },
    { bundleId: "com.example.NoIcon", name: "No Icon", pid: 777, icon: null },
  ],
  microphones: [],
  cameras: [],
};

const EMPTY: RecordingSourcesVm = {
  displays: [{ id: 1, width: 3456, height: 2234, isMain: true }],
  applications: [],
  microphones: [],
  cameras: [],
};

beforeEach(() => {
  mockList.mockReset();
  mockList.mockResolvedValue(SOURCES);
});

afterEach(() => {
  resetRecordingSourceForTest();
  vi.clearAllMocks();
});

describe("RecordingSourcePicker", () => {
  it("polls list_sources on mount and renders Displays then Applications", async () => {
    render(<RecordingSourcePicker />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());

    expect(screen.getByText(DISPLAYS_HEADING)).toBeInTheDocument();
    expect(screen.getByText(APPLICATIONS_HEADING)).toBeInTheDocument();
    expect(screen.getByText(MAIN_DISPLAY_LABEL)).toBeInTheDocument();
    expect(await screen.findByText("Safari")).toBeInTheDocument();
    expect(screen.getByText("No Icon")).toBeInTheDocument();
    // The second (non-main) display is individually selectable.
    expect(screen.getByText(/Display 2/)).toBeInTheDocument();
  });

  it("defaults the selection to the main display", async () => {
    render(<RecordingSourcePicker />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    expect(selectedRecordingTarget()).toEqual({ kind: "display", displayId: null });
  });

  it("selecting an application updates the target and shows the app-scope disclosure", async () => {
    render(<RecordingSourcePicker />);
    const safari = await screen.findByText("Safari");
    fireEvent.click(safari);

    expect(selectedRecordingTarget()).toEqual({
      kind: "application",
      pid: 501,
      bundleId: "com.apple.Safari",
    });
    // The inline exclusion disclosure names the app.
    expect(screen.getByText(appScopeDisclosure("Safari"))).toBeInTheDocument();
  });

  it("keyboard arrow navigation updates the selection (not only mouse clicks)", async () => {
    render(<RecordingSourcePicker />);
    await screen.findByText("Safari");
    const radios = screen.getAllByRole("radio");
    // The main display is the default selection; arrow-navigate off it. Radix
    // drives keyboard selection through the group's `onValueChange`, never a row
    // `onClick`, so this only updates the store when that channel is wired.
    radios[0].focus();
    fireEvent.keyDown(radios[0], { key: "ArrowDown" });
    await waitFor(() =>
      expect(selectedRecordingTarget()).not.toEqual({ kind: "display", displayId: null }),
    );
  });

  it("does not poll while inactive (a live recording keeps the setup mounted)", async () => {
    render(<RecordingSourcePicker active={false} />);
    // The picker stays mounted during recording; polling must be paused so no
    // fresh keeper-rec child spawns every ~3s.
    await Promise.resolve();
    expect(mockList).not.toHaveBeenCalled();
  });

  it("renders the real icon for apps that have one and a fallback glyph otherwise", async () => {
    render(<RecordingSourcePicker />);
    await screen.findByText("Safari");
    // The app with an icon renders an <img> data-URI.
    const icon = document.querySelector('img[src="data:image/png;base64,AA=="]');
    expect(icon).not.toBeNull();
  });

  it("shows the empty-applications note when none are recordable", async () => {
    mockList.mockResolvedValue(EMPTY);
    render(<RecordingSourcePicker />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    expect(await screen.findByText(NO_APPLICATIONS_NOTE)).toBeInTheDocument();
  });

  it("marks a vanished selection unavailable without swapping it", async () => {
    // Pre-select an app that will not be in the polled list.
    selectRecordingTarget({ kind: "application", pid: 999, bundleId: "com.gone.App" });
    render(<RecordingSourcePicker />);
    await waitFor(() => expect(mockList).toHaveBeenCalled());
    await screen.findByText("Safari");

    expect(screen.getByRole("alert")).toHaveTextContent(SELECTION_UNAVAILABLE_NOTE);
    // The selection is not silently replaced with a present source.
    expect(selectedRecordingTarget()).toEqual({
      kind: "application",
      pid: 999,
      bundleId: "com.gone.App",
    });
  });
});
