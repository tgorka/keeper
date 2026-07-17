import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  ACCESS_LABEL,
  DEV_BUILD_NOTE,
  MONTHLY_RECONFIRM_NOTE,
  OPEN_SETTINGS_LABEL,
  RELAUNCH_NOTE,
  REQUEST_PERMISSION_LABEL,
  RecordingPermissionRow,
  SCREEN_RECORDING_PERMISSION_NAME,
} from "@/components/recording/recording-permission-row";

function renderRow(access: "granted" | "notYetRequested" | "denied") {
  const onRequest = vi.fn();
  const onOpenSettings = vi.fn();
  render(
    <RecordingPermissionRow
      access={access}
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

  it("never uses the recording-red token (reserved for the 16.6 live dot)", () => {
    const { container } = render(
      <RecordingPermissionRow access="denied" onRequest={vi.fn()} onOpenSettings={vi.fn()} />,
    );
    expect(container.innerHTML).not.toContain("recording-red");
  });
});
