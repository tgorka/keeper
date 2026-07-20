import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  ACCESS_LABEL,
  CAMERA_PERMISSION_NAME,
  CAMERA_ROW_NOTE,
  DEV_BUILD_NOTE,
  MICROPHONE_PERMISSION_NAME,
  MICROPHONE_ROW_NOTE,
  MONTHLY_RECONFIRM_NOTE,
  OPEN_SETTINGS_LABEL,
  RELAUNCH_NOTE,
  REQUEST_PERMISSION_LABEL,
  RecordingPermissionRow,
  SCREEN_RECORDING_NOTES,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";

function renderRow(
  access: "granted" | "notYetRequested" | "denied",
  {
    name = SCREEN_RECORDING_PERMISSION_NAME,
    notes = SCREEN_RECORDING_NOTES,
  }: { name?: string; notes?: readonly string[] } = {},
) {
  const onRequest = vi.fn();
  const onOpenSettings = vi.fn();
  render(
    <RecordingPermissionRow
      name={name}
      access={access}
      notes={notes}
      onRequest={onRequest}
      onOpenSettings={onOpenSettings}
    />,
  );
  return { onRequest, onOpenSettings };
}

describe("RecordingPermissionRow", () => {
  it("granted: shows the green pill and no action", () => {
    renderRow("granted");

    expect(screen.getByText(SCREEN_RECORDING_PERMISSION_NAME)).toBeInTheDocument();
    expect(screen.getByText(ACCESS_LABEL.granted)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: REQUEST_PERMISSION_LABEL }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: OPEN_SETTINGS_LABEL })).not.toBeInTheDocument();
  });

  it("not yet requested: offers the OS request and dispatches it", () => {
    const { onRequest, onOpenSettings } = renderRow("notYetRequested");

    expect(screen.getByText(ACCESS_LABEL.notYetRequested)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL }));
    expect(onRequest).toHaveBeenCalledTimes(1);
    expect(onOpenSettings).not.toHaveBeenCalled();
  });

  it("denied: offers the System Settings deep link (no re-prompt) and dispatches it", () => {
    const { onRequest, onOpenSettings } = renderRow("denied");

    expect(screen.getByText(ACCESS_LABEL.denied)).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: REQUEST_PERMISSION_LABEL }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: OPEN_SETTINGS_LABEL }));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(onRequest).not.toHaveBeenCalled();
  });

  it("states the honest quirk note-lines in every state", () => {
    renderRow("granted");

    expect(screen.getByText(RELAUNCH_NOTE)).toBeInTheDocument();
    expect(screen.getByText(MONTHLY_RECONFIRM_NOTE)).toBeInTheDocument();
    expect(screen.getByText(DEV_BUILD_NOTE)).toBeInTheDocument();
  });

  // --- The generalized Microphone / Camera rows (Story 20.2) ---------------

  it("renders the Microphone row with its own honest note-line", () => {
    renderRow("granted", { name: MICROPHONE_PERMISSION_NAME, notes: [MICROPHONE_ROW_NOTE] });

    expect(screen.getByText(MICROPHONE_PERMISSION_NAME)).toBeInTheDocument();
    expect(screen.getByText(MICROPHONE_ROW_NOTE)).toBeInTheDocument();
    // The screen quirks belong to the screen row only.
    expect(screen.queryByText(RELAUNCH_NOTE)).not.toBeInTheDocument();
    expect(screen.getByText(ACCESS_LABEL.granted)).toBeInTheDocument();
  });

  it("a not-yet-requested Microphone row dispatches its request action", () => {
    const { onRequest } = renderRow("notYetRequested", {
      name: MICROPHONE_PERMISSION_NAME,
      notes: [MICROPHONE_ROW_NOTE],
    });

    fireEvent.click(screen.getByRole("button", { name: REQUEST_PERMISSION_LABEL }));
    expect(onRequest).toHaveBeenCalledTimes(1);
  });

  it("a denied Camera row offers the deep link and states its note", () => {
    const { onOpenSettings } = renderRow("denied", {
      name: CAMERA_PERMISSION_NAME,
      notes: [CAMERA_ROW_NOTE],
    });

    expect(screen.getByText(CAMERA_PERMISSION_NAME)).toBeInTheDocument();
    expect(screen.getByText(CAMERA_ROW_NOTE)).toBeInTheDocument();
    expect(screen.getByText(ACCESS_LABEL.denied)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: OPEN_SETTINGS_LABEL }));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
  });

  it("never uses the recording-red token (reserved for the 16.6 live dot)", () => {
    const { container } = render(
      <RecordingPermissionRow
        name={CAMERA_PERMISSION_NAME}
        access="denied"
        notes={[CAMERA_ROW_NOTE]}
        onRequest={vi.fn()}
        onOpenSettings={vi.fn()}
      />,
    );
    expect(container.innerHTML).not.toContain("recording-red");
  });
});
