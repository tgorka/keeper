import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { TelemetryStatusVm } from "@/lib/ipc/client";
import { TelemetrySection } from "./telemetry-section";

const ipc = vi.hoisted(() => ({
  status: vi.fn(),
  save: vi.fn(),
  remote: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("@/lib/ipc/client", () => ({
  telemetryStatus: ipc.status,
  telemetryConsentSet: ipc.save,
  telemetryRemoteConfig: ipc.remote,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: ipc.listen }));
const OFF: TelemetryStatusVm = {
  consent: { diagnostics: false, productAnalytics: false, remoteConfig: false },
  configured: true,
  host: "https://us.i.posthog.com",
  installationId: null,
};
let publish: (event: { payload: TelemetryStatusVm }) => void;
beforeEach(() => {
  vi.clearAllMocks();
  ipc.status.mockResolvedValue(OFF);
  ipc.remote.mockResolvedValue({ supportMessage: null });
  ipc.listen.mockImplementation((_name, callback) => {
    publish = callback;
    return Promise.resolve(vi.fn());
  });
});
afterEach(cleanup);

it("opening settings never opts in or fetches remote configuration; one category changes alone", async () => {
  ipc.save.mockImplementation(async (consent) => ({ ...OFF, consent }));
  render(<TelemetrySection open />);
  const diagnostic = await screen.findByRole("switch", { name: "Optional diagnostics" });
  await waitFor(() => expect(diagnostic).toBeEnabled());
  expect(ipc.save).not.toHaveBeenCalled();
  expect(ipc.remote).not.toHaveBeenCalled();
  fireEvent.click(diagnostic);
  await waitFor(() => expect(diagnostic).toBeChecked());
  expect(ipc.save).toHaveBeenCalledWith({
    diagnostics: true,
    productAnalytics: false,
    remoteConfig: false,
  });
  expect(screen.getByRole("switch", { name: "Optional product statistics" })).not.toBeChecked();
  expect(ipc.remote).not.toHaveBeenCalled();
});

it("cross-window revocation removes the message and rejects an older remote response", async () => {
  const enabled = { ...OFF, consent: { ...OFF.consent, remoteConfig: true } };
  ipc.status.mockResolvedValue(enabled);
  let finish: (value: { supportMessage: string }) => void = () => {
    throw new Error("request not started");
  };
  ipc.remote.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  render(<TelemetrySection open />);
  await waitFor(() => expect(ipc.remote).toHaveBeenCalledTimes(1));
  act(() => publish({ payload: OFF }));
  await act(async () => finish({ supportMessage: "Stale support message" }));
  expect(screen.queryByLabelText("Support message")).not.toBeInTheDocument();
  expect(screen.getByRole("switch", { name: "Optional remote configuration" })).not.toBeChecked();
});

it("failed persistence never claims a successful opt-in or renders arbitrary error text", async () => {
  ipc.save.mockRejectedValue(new Error("PRIVATE_ERROR_SENTINEL /Users/person/private.txt"));
  render(<TelemetrySection open />);
  const control = screen.getByRole("switch", { name: "Optional diagnostics" });
  await waitFor(() => expect(control).toBeEnabled());
  fireEvent.click(control);
  await screen.findByRole("alert");
  expect(control).not.toBeChecked();
  expect(control).toBeDisabled();
  expect(screen.queryByText(/PRIVATE_ERROR_SENTINEL/)).not.toBeInTheDocument();
  expect(ipc.remote).not.toHaveBeenCalled();
});

it("cannot leave the app for an unconfigured study", async () => {
  ipc.status.mockResolvedValue({ ...OFF, configured: false, host: null });
  render(<TelemetrySection open />);
  await screen.findByText(
    "PostHog is not configured in this build. No observability requests can be sent.",
  );
  expect(screen.getByRole("button", { name: "Open synthetic usability study" })).toBeDisabled();
});

it("requires acknowledging full-document replacement and allows keeping unsaved edits", async () => {
  render(<TelemetrySection open />);
  const open = screen.getByRole("button", { name: "Open synthetic usability study" });
  await waitFor(() => expect(open).toBeEnabled());
  fireEvent.click(open);
  expect(screen.getByRole("alertdialog")).toHaveTextContent("Unsaved edits may be lost");
  fireEvent.click(screen.getByRole("button", { name: "Keep editing" }));
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(open).toBeEnabled();
});
