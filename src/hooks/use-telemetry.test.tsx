import { act, cleanup, renderHook } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useTelemetryOpening, useTelemetryReadiness } from "./use-telemetry";

const capture = vi.hoisted(() => vi.fn(async () => {}));
vi.mock("@/lib/ipc/client", () => ({ telemetryCapture: capture }));
let frames: Map<number, FrameRequestCallback>;
let id: number;
beforeEach(() => {
  capture.mockClear();
  frames = new Map();
  id = 0;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
    frames.set(++id, callback);
    return id;
  });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation((frame) => {
    frames.delete(frame);
  });
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});
function paint() {
  act(() => {
    const batch = [...frames.values()];
    frames.clear();
    for (const callback of batch) callback(performance.now());
  });
}

it("reports one readiness only after a non-splash screen paints, including StrictMode", () => {
  const view = renderHook(({ ready }) => useTelemetryReadiness(ready), {
    initialProps: { ready: false },
    wrapper: StrictMode,
  });
  paint();
  paint();
  expect(capture).not.toHaveBeenCalled();
  view.rerender({ ready: true });
  paint();
  paint();
  expect(capture).toHaveBeenCalledTimes(1);
  expect(capture).toHaveBeenCalledWith({ kind: "appReady", durationMs: expect.any(Number) });
  view.rerender({ ready: false });
  view.rerender({ ready: true });
  paint();
  paint();
  expect(capture).toHaveBeenCalledTimes(1);
});

it("does not report a surface cancelled before paint and reports reopening separately", () => {
  const view = renderHook(({ open }) => useTelemetryOpening("settingsOpened", open), {
    initialProps: { open: true },
  });
  paint();
  view.rerender({ open: false });
  paint();
  expect(capture).not.toHaveBeenCalled();
  view.rerender({ open: true });
  paint();
  paint();
  expect(capture).toHaveBeenCalledWith({ kind: "settingsOpened", durationMs: null });
  expect(capture).toHaveBeenCalledWith({ kind: "interaction", durationMs: expect.any(Number) });
  view.unmount();
  paint();
  expect(capture).toHaveBeenCalledTimes(2);
});
