import type * as TauriWindowApi from "@tauri-apps/api/window";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Study } from "./study";

const ports = vi.hoisted(() => ({
  preview: vi.fn(),
  stop: vi.fn(),
  start: vi.fn(),
  close: vi.fn(),
  destroy: vi.fn(),
}));
vi.mock("./commands", () => ({
  telemetryStudyPreview: ports.preview,
  telemetryStudyStop: ports.stop,
}));
vi.mock("./session", () => ({ startStudyCapture: ports.start }));
vi.mock("@tauri-apps/api/window", async (importOriginal) => {
  const actual = await importOriginal<typeof TauriWindowApi>();
  return {
    getCurrentWindow: () => {
      const window = actual.getCurrentWindow();
      vi.spyOn(window, "listen").mockImplementation((_event, callback) => ports.close(callback));
      vi.spyOn(window, "destroy").mockImplementation(async () => {
        ports.destroy();
      });
      return window;
    },
  };
});
let closeWindow: () => Promise<void>;
beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal("__TAURI_INTERNALS__", { metadata: { currentWindow: { label: "main" } } });
  ports.preview.mockResolvedValue({
    configured: true,
    host: "https://us.i.posthog.com",
  });
  ports.close.mockImplementation(async (callback) => {
    closeWindow = () =>
      Promise.resolve(callback({ event: "tauri://close-requested", id: 1, payload: null }));
    return vi.fn();
  });
  ports.stop.mockResolvedValue(undefined);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

it("does not start capture while browsing synthetic tasks, then stops on unmount", async () => {
  let signal: AbortSignal | undefined;
  const stop = vi.fn(async () => {});
  ports.start.mockImplementation(async (next: AbortSignal) => {
    signal = next;
    return { stop };
  });
  const view = render(<Study />);
  const start = screen.getByRole("button", { name: "Start replay and heatmaps" });
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Move to Done" }));
  fireEvent.click(screen.getByRole("button", { name: "Done" }));
  expect(screen.getByRole("button", { name: "Move to To do" })).toBeInTheDocument();
  expect(ports.start).not.toHaveBeenCalled();
  fireEvent.click(start);
  await screen.findByText("Replay and heatmaps active");
  view.unmount();
  expect(signal?.aborted).toBe(true);
});

it("native close cancels startup without destroying the hidden main window", async () => {
  let signal: AbortSignal | undefined;
  ports.start.mockImplementation(async (next: AbortSignal) => {
    signal = next;
    return { stop: vi.fn(async () => {}) };
  });
  render(<Study />);
  const start = screen.getByRole("button", { name: "Start replay and heatmaps" });
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(start);
  expect(signal?.aborted).toBe(false);
  await act(async () => {
    await closeWindow();
  });
  expect(signal?.aborted).toBe(true);
  expect(ports.destroy).not.toHaveBeenCalled();
  expect(screen.getByRole("heading", { name: "Synthetic usability study" })).toBeInTheDocument();
  expect(screen.getByText("Replay and heatmaps off")).toBeInTheDocument();
});

it("refuses capture if the native close guard cannot be installed", async () => {
  ports.close.mockRejectedValue(new Error("native event unavailable"));
  render(<Study />);
  await screen.findByRole("alert");
  expect(screen.getByRole("button", { name: "Start replay and heatmaps" })).toBeDisabled();
  expect(ports.start).not.toHaveBeenCalled();
});

it("keeps capture unavailable when the pure destination preview is unconfigured", async () => {
  ports.preview.mockResolvedValue({ configured: false, host: null });
  render(<Study />);
  await waitFor(() => expect(ports.close).toHaveBeenCalled());
  expect(screen.getByRole("button", { name: "Start replay and heatmaps" })).toBeDisabled();
  expect(ports.start).not.toHaveBeenCalled();
});

it("removes the active claim when ingestion or a safety budget stops the session", async () => {
  let failure: () => void = () => {
    throw new Error("not started");
  };
  ports.start.mockImplementation(async (_signal: AbortSignal, onFailure: () => void) => {
    failure = onFailure;
    return { stop: vi.fn() };
  });
  render(<Study />);
  const start = screen.getByRole("button", { name: "Start replay and heatmaps" });
  await waitFor(() => expect(start).toBeEnabled());
  fireEvent.click(start);
  await screen.findByText("Replay and heatmaps active");
  act(() => failure());
  expect(screen.queryByText("Replay and heatmaps active")).not.toBeInTheDocument();
  expect(screen.getByRole("alert")).toHaveTextContent("may already have been transmitted");
  expect(screen.getByRole("button", { name: "Stop capture" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "Prepare a new session" })).toBeEnabled();
});

it("does not resurrect a failed capture when startup resolves in the same turn", async () => {
  ports.start.mockImplementation((_signal: AbortSignal, onFailure: () => void) => {
    const ready = Promise.resolve({ stop: vi.fn(async () => {}) });
    queueMicrotask(onFailure);
    return ready;
  });
  render(<Study />);
  const start = screen.getByRole("button", { name: "Start replay and heatmaps" });
  await waitFor(() => expect(start).toBeEnabled());
  await act(async () => {
    fireEvent.click(start);
  });
  expect(screen.getByRole("alert")).toHaveTextContent("may already have been transmitted");
  expect(screen.queryByText("Replay and heatmaps active")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Stop capture" })).toBeDisabled();
});
