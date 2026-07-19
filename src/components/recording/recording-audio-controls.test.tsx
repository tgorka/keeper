import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { RecordingAudioControls } from "@/components/recording/recording-audio-controls";
import { resetRecordingAudioForTest, systemAudioEnabled } from "@/lib/stores/recording-audio";

afterEach(() => {
  resetRecordingAudioForTest();
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
});
