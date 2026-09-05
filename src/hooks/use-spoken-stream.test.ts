/**
 * The spoken turn's stream, observed (Epic 67, AD-205): one subscription
 * while the surface exists, every forwarded event handed to the latest
 * handler, nothing after unmount, and nothing at all without the bots
 * capability.
 */
import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BotStreamEvent } from "@/lib/ipc/client";

const listenSpokenStream =
  vi.fn<(onEvent: (event: BotStreamEvent) => void) => Promise<() => void>>();
vi.mock("@/lib/ipc/client", () => ({
  listenSpokenStream: (onEvent: (event: BotStreamEvent) => void) => listenSpokenStream(onEvent),
}));

import { useSpokenStream } from "@/hooks/use-spoken-stream";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

const DELTA: BotStreamEvent = { kind: "delta", text: "Three" };

beforeEach(() => {
  listenSpokenStream.mockReset();
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
});

describe("useSpokenStream", () => {
  it("subscribes nothing where the surface does not exist", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    renderHook(() => useSpokenStream(() => {}));
    expect(listenSpokenStream).not.toHaveBeenCalled();
  });

  it("hands every forwarded event to the latest handler, and none after unmount", async () => {
    const captured: { sink: ((event: BotStreamEvent) => void) | null } = { sink: null };
    const unlisten = vi.fn();
    listenSpokenStream.mockImplementation((onEvent) => {
      captured.sink = onEvent;
      return Promise.resolve(unlisten);
    });
    const first = vi.fn();
    const second = vi.fn();
    const { rerender, unmount } = renderHook(({ handler }) => useSpokenStream(handler), {
      initialProps: { handler: first },
    });
    await vi.waitFor(() => expect(listenSpokenStream).toHaveBeenCalledTimes(1));
    const sink = captured.sink;
    if (sink === null) {
      throw new Error("the listener never handed over its sink");
    }
    sink(DELTA);
    expect(first).toHaveBeenCalledWith(DELTA);

    // A re-render with a new handler re-subscribes nothing and routes to it.
    rerender({ handler: second });
    sink(DELTA);
    expect(listenSpokenStream).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledWith(DELTA);
    expect(first).toHaveBeenCalledTimes(1);

    unmount();
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
    sink(DELTA);
    expect(second).toHaveBeenCalledTimes(1);
  });

  it("tears down a subscription that resolved after unmount", async () => {
    const captured: { resolve: ((fn: () => void) => void) | null } = { resolve: null };
    listenSpokenStream.mockImplementation(
      () =>
        new Promise<() => void>((r) => {
          captured.resolve = r;
        }),
    );
    const { unmount } = renderHook(() => useSpokenStream(() => {}));
    await vi.waitFor(() => expect(listenSpokenStream).toHaveBeenCalledTimes(1));
    unmount();
    const resolve = captured.resolve;
    if (resolve === null) {
      throw new Error("the listener never started");
    }
    const unlisten = vi.fn();
    resolve(unlisten);
    await vi.waitFor(() => expect(unlisten).toHaveBeenCalledTimes(1));
  });
});
