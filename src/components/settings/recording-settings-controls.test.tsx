import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  recordingSettingsGet: vi.fn(),
  recordingSettingsSet: vi.fn(),
}));

import {
  DURATION_CAP_LABEL,
  RecordingSettingsControls,
  SEGMENT_SIZE_LABEL,
} from "@/components/settings/recording-settings-controls";
import type { RecordingSettingsVm } from "@/lib/ipc/client";
import { recordingSettingsGet, recordingSettingsSet } from "@/lib/ipc/client";
import { resetRecordingSettingsForTest } from "@/lib/stores/recording-settings";

const mockGet = vi.mocked(recordingSettingsGet);
const mockSet = vi.mocked(recordingSettingsSet);

const DEFAULTS: RecordingSettingsVm = {
  segmentMb: 500,
  durationCapMinutes: 30,
  // Story 19.5: the co-settings ride the same VM; a segment/duration edit must
  // carry them along unchanged.
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
  // Default echo: Rust returns the effective VM, which equals the sent one
  // when it is already in bounds.
  mockSet.mockImplementation((vm) => Promise.resolve(vm));
});

afterEach(() => {
  vi.clearAllMocks();
});

/** The segment-size input of the `index`-th mounted instance. */
function segmentInput(index = 0): HTMLInputElement {
  return screen.getAllByLabelText(SEGMENT_SIZE_LABEL)[index] as HTMLInputElement;
}

/** The duration-cap input of the `index`-th mounted instance. */
function durationInput(index = 0): HTMLInputElement {
  return screen.getAllByLabelText(DURATION_CAP_LABEL)[index] as HTMLInputElement;
}

describe("RecordingSettingsControls", () => {
  it("hydrates lazily and shows the persisted values with the next-session note", async () => {
    mockGet.mockResolvedValue({ ...DEFAULTS, segmentMb: 800, durationCapMinutes: 45 });
    render(<RecordingSettingsControls />);

    await waitFor(() => expect(segmentInput()).toHaveValue(800));
    expect(durationInput()).toHaveValue(45);
    expect(mockGet).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Applies to the next Recording Session.")).toBeInTheDocument();
  });

  it("clamps an out-of-range entry on blur and persists the clamped value", async () => {
    render(<RecordingSettingsControls />);
    await waitFor(() => expect(segmentInput()).toHaveValue(500));

    // Below the floor: 10 → 100.
    fireEvent.change(segmentInput(), { target: { value: "10" } });
    fireEvent.blur(segmentInput());
    await waitFor(() => expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, segmentMb: 100 }));
    await waitFor(() => expect(segmentInput()).toHaveValue(100));

    // Above the ceiling: 5000 caps the duration field at 600.
    fireEvent.change(durationInput(), { target: { value: "5000" } });
    fireEvent.blur(durationInput());
    await waitFor(() =>
      expect(mockSet).toHaveBeenCalledWith({
        ...DEFAULTS,
        segmentMb: 100,
        durationCapMinutes: 600,
      }),
    );
    await waitFor(() => expect(durationInput()).toHaveValue(600));
  });

  it("calls the setter on an in-range edit and displays the effective VM it returns", async () => {
    // Rust may clamp differently than the local bounds (e.g. after an authored
    // bounds change) — the field must show the *effective* persisted value.
    mockSet.mockResolvedValue({ ...DEFAULTS, segmentMb: 1000 });
    render(<RecordingSettingsControls />);
    await waitFor(() => expect(segmentInput()).toHaveValue(500));

    fireEvent.change(segmentInput(), { target: { value: "1024" } });
    fireEvent.blur(segmentInput());

    await waitFor(() => expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, segmentMb: 1024 }));
    await waitFor(() => expect(segmentInput()).toHaveValue(1000));
  });

  it("keeps two mounted instances in sync through the shared store", async () => {
    // The Settings → Recording section and the "Segmenting" setup card mount
    // the same control; the shared store mirrors an edit in either into both.
    render(
      <>
        <RecordingSettingsControls />
        <RecordingSettingsControls />
      </>,
    );
    await waitFor(() => expect(segmentInput(0)).toHaveValue(500));
    await waitFor(() => expect(segmentInput(1)).toHaveValue(500));
    // One shared hydration, not one per surface.
    expect(mockGet).toHaveBeenCalledTimes(1);

    fireEvent.change(durationInput(0), { target: { value: "45" } });
    fireEvent.blur(durationInput(0));

    await waitFor(() => expect(durationInput(0)).toHaveValue(45));
    await waitFor(() => expect(durationInput(1)).toHaveValue(45));
    expect(mockSet).toHaveBeenCalledTimes(1);
  });

  it("reverts to the prior value when the persist fails", async () => {
    mockSet.mockRejectedValue({
      code: "internal",
      message: "registry write failed",
      accountId: null,
      retriable: false,
    });
    render(<RecordingSettingsControls />);
    await waitFor(() => expect(segmentInput()).toHaveValue(500));

    fireEvent.change(segmentInput(), { target: { value: "800" } });
    fireEvent.blur(segmentInput());

    // Optimistic first, then the honest revert once the write rejects.
    await waitFor(() => expect(segmentInput()).toHaveValue(500));
    expect(mockSet).toHaveBeenCalledWith({ ...DEFAULTS, segmentMb: 800 });
  });

  it("discards a non-numeric entry on blur without persisting", async () => {
    render(<RecordingSettingsControls />);
    await waitFor(() => expect(segmentInput()).toHaveValue(500));

    fireEvent.change(segmentInput(), { target: { value: "" } });
    fireEvent.blur(segmentInput());

    await waitFor(() => expect(segmentInput()).toHaveValue(500));
    expect(mockSet).not.toHaveBeenCalled();
  });
});
