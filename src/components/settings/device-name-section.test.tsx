import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEVICE_NAME_RESET,
  DEVICE_NAME_SAVE,
  DEVICE_NAME_SIGN_IN,
  DEVICE_NAME_TITLE,
  DeviceNameSection,
} from "@/components/settings/device-name-section";
import {
  accountSignIn,
  type DeviceNameVm,
  deviceNameGet,
  deviceNameReset,
  deviceNameSet,
  type HqStatusVm,
  hqStatusSubscribe,
} from "@/lib/ipc/client";

vi.mock("@/lib/ipc/client", () => ({
  accountSignIn: vi.fn(() => Promise.resolve()),
  deviceNameGet: vi.fn(),
  deviceNameSet: vi.fn(),
  deviceNameReset: vi.fn(),
  hqStatusSubscribe: vi.fn(),
  hqStatusUnsubscribe: vi.fn(() => Promise.resolve()),
}));

const SYSTEM: DeviceNameVm = {
  name: "iPhone",
  source: "system",
  system: "iPhone",
  sentence: "From the system.",
};
const KALYPSO: DeviceNameVm = {
  name: "kalypso",
  source: "you",
  system: "iPhone",
  sentence: "Set by you. The system calls this device iPhone.",
};

function hq(signIn: HqStatusVm["signIn"]): HqStatusVm {
  return {
    server: "https://agents.example.org",
    phase: "paired",
    sentence: "Paired with agents.example.org.",
    device: null,
    databases: [],
    waitingHere: 0,
    devices: [],
    offer: null,
    signIn,
  };
}
const ENROLLED = hq({
  state: "enrolled",
  sentence: "Signed in: agents.example.org knows this device by your account.",
});

let push: (status: HqStatusVm) => void = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(hqStatusSubscribe).mockImplementation((onStatus) => {
    push = onStatus;
    onStatus(ENROLLED);
    return Promise.resolve("sub-1");
  });
});

const field = () => screen.getByRole("textbox", { name: DEVICE_NAME_TITLE });

describe("DeviceNameSection (R-HQ-94)", () => {
  it("is prefilled with the effective name, says where it comes from, and saves what was typed", async () => {
    vi.mocked(deviceNameGet).mockResolvedValue(SYSTEM);
    vi.mocked(deviceNameSet).mockResolvedValue(KALYPSO);
    render(<DeviceNameSection open />);
    await waitFor(() => expect(field()).toHaveValue("iPhone"));
    expect(screen.getByText("From the system.")).toHaveAttribute("data-source", "system");
    expect(screen.getByRole("button", { name: DEVICE_NAME_RESET })).toBeDisabled();
    expect(screen.getByRole("button", { name: DEVICE_NAME_SAVE })).toBeDisabled();
    // Unchanged, the agents server's line is the Agents server section's business.
    expect(screen.queryByRole("status")).toBeNull();

    fireEvent.change(field(), { target: { value: " kalypso " } });
    fireEvent.click(screen.getByRole("button", { name: DEVICE_NAME_SAVE }));
    await waitFor(() => expect(deviceNameSet).toHaveBeenCalledWith(" kalypso "));
    expect(await screen.findByText(KALYPSO.sentence)).toHaveAttribute("data-source", "you");
    expect(field()).toHaveValue("kalypso");
    expect(screen.getByRole("button", { name: DEVICE_NAME_RESET })).toBeEnabled();
    // Changed: what the agents server made of it shows here.
    act(() =>
      push(
        hq({
          state: "enrolling",
          sentence: "Updating the name on agents.example.org…",
        }),
      ),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Updating the name on agents.example.org…",
    );
  });

  it("shows Rust's refusal and keeps the typed name", async () => {
    const refusal = "A device's name cannot be blank. Type one, or Reset to use the system's.";
    vi.mocked(deviceNameGet).mockResolvedValue(KALYPSO);
    vi.mocked(deviceNameSet).mockRejectedValue({ code: "internal", message: refusal });
    render(<DeviceNameSection open />);
    await waitFor(() => expect(field()).toHaveValue("kalypso"));
    fireEvent.change(field(), { target: { value: "   " } });
    fireEvent.click(screen.getByRole("button", { name: DEVICE_NAME_SAVE }));
    expect(await screen.findByRole("alert")).toHaveTextContent(refusal);
    expect(field()).toHaveValue("   ");
  });

  it("Reset goes back to the system's name", async () => {
    vi.mocked(deviceNameGet).mockResolvedValue(KALYPSO);
    vi.mocked(deviceNameReset).mockResolvedValue(SYSTEM);
    render(<DeviceNameSection open />);
    await waitFor(() => expect(field()).toHaveValue("kalypso"));
    fireEvent.click(screen.getByRole("button", { name: DEVICE_NAME_RESET }));
    await waitFor(() => expect(field()).toHaveValue("iPhone"));
    expect(deviceNameReset).toHaveBeenCalledTimes(1);
    expect(screen.getByText("From the system.")).toHaveAttribute("data-source", "system");
  });

  it("asks for a sign-in when the server still has the old name, and starts it", async () => {
    vi.mocked(deviceNameGet).mockResolvedValue(KALYPSO);
    render(<DeviceNameSection open />);
    await waitFor(() => expect(field()).toHaveValue("kalypso"));
    act(() =>
      push(
        hq({
          state: "renamed",
          sentence: "Sign in again to use the new name on agents.example.org.",
        }),
      ),
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      "Sign in again to use the new name on agents.example.org.",
    );
    fireEvent.click(screen.getByRole("button", { name: DEVICE_NAME_SIGN_IN }));
    expect(accountSignIn).toHaveBeenCalledTimes(1);
  });
});
