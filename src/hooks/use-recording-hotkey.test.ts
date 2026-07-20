import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

// Capture the registered event listener so the test can fire the hotkey event
// without a live Tauri backend; a per-test `listenImpl` simulates "outside
// Tauri" (mirroring `use-global-hotkey.test.ts`).
type HotkeyHandler = () => void;
let registered: HotkeyHandler | undefined;
const unlisten = vi.fn();
let listenImpl: (event: string, handler: HotkeyHandler) => Promise<() => void>;

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: HotkeyHandler) => listenImpl(event, handler),
}));

const toggleRecording = vi.fn();
vi.mock("@/lib/recording-control", () => ({
  toggleRecording: () => toggleRecording(),
}));

import { RECORDING_HOTKEY_EVENT, useRecordingHotkey } from "@/hooks/use-recording-hotkey";

beforeEach(() => {
  registered = undefined;
  unlisten.mockClear();
  toggleRecording.mockReset().mockResolvedValue(undefined);
  listenImpl = (_event, handler) => {
    registered = handler;
    return Promise.resolve(unlisten);
  };
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: true });
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useRecordingHotkey", () => {
  it("subscribes to the recording-hotkey event and toggles capture on fire", async () => {
    let subscribedEvent: string | undefined;
    listenImpl = (event, handler) => {
      subscribedEvent = event;
      registered = handler;
      return Promise.resolve(unlisten);
    };
    renderHook(() => useRecordingHotkey());
    await waitFor(() => expect(registered).toBeTypeOf("function"));
    expect(subscribedEvent).toBe(RECORDING_HOTKEY_EVENT);

    registered?.();

    expect(toggleRecording).toHaveBeenCalledTimes(1);
  });

  it("never subscribes when the recording capability is off", async () => {
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, recording: false });
    renderHook(() => useRecordingHotkey());
    // Give any (wrong) subscription a microtask to land.
    await Promise.resolve();
    expect(registered).toBeUndefined();
    expect(toggleRecording).not.toHaveBeenCalled();
  });

  it("unlistens on unmount", async () => {
    const { unmount } = renderHook(() => useRecordingHotkey());
    await waitFor(() => expect(registered).toBeTypeOf("function"));
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("is a graceful no-op outside a Tauri host (listen rejects)", async () => {
    listenImpl = () => Promise.reject(new Error("no tauri host"));
    expect(() => renderHook(() => useRecordingHotkey())).not.toThrow();
    await Promise.resolve();
    expect(toggleRecording).not.toHaveBeenCalled();
  });

  it("does not throw when listen throws synchronously", () => {
    listenImpl = () => {
      throw new Error("ipc internals absent");
    };
    expect(() => renderHook(() => useRecordingHotkey())).not.toThrow();
  });
});
