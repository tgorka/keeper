/**
 * The voice stream's lifecycle (Story 62.5, DW-4): one watch per mount, torn
 * down on unmount, and a snapshot after teardown that changes nothing.
 */
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";

const voiceWatch = vi.fn<(onState: (state: VoiceStateVm) => void) => Promise<number>>();
const voiceUnwatch = vi.fn<(id: number) => Promise<void>>();
const voiceAvailability = vi.fn<() => Promise<VoiceUnavailableVm | null>>();
const voiceWakeGet = vi.fn<() => Promise<VoiceWakeVm>>();
vi.mock("@/lib/ipc/client", () => ({
  voiceWatch: (onState: (state: VoiceStateVm) => void) => voiceWatch(onState),
  voiceUnwatch: (id: number) => voiceUnwatch(id),
  voiceAvailability: () => voiceAvailability(),
  voiceWakeGet: () => voiceWakeGet(),
}));

import { useVoiceStream } from "@/hooks/use-voice-stream";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

const WAKE: VoiceWakeVm = { enabled: false, phrase: "nixie", limits: "costs" };
const LISTENING: VoiceStateVm = { kind: "idle", wake: "nixie", listeningForWake: true };

beforeEach(() => {
  voiceWatch.mockReset();
  voiceUnwatch.mockReset();
  voiceAvailability.mockReset();
  voiceWakeGet.mockReset();
  voiceUnwatch.mockResolvedValue();
  voiceAvailability.mockResolvedValue(null);
  voiceWakeGet.mockResolvedValue(WAKE);
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, bots: true });
});

afterEach(() => {
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("useVoiceStream", () => {
  it("opens nothing where the surface does not exist", () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    renderHook(() => useVoiceStream());
    expect(voiceWatch).not.toHaveBeenCalled();
    expect(voiceAvailability).not.toHaveBeenCalled();
    expect(voiceWakeGet).not.toHaveBeenCalled();
  });

  it("watches once, and mirrors every snapshot plus the two one-shot reads", async () => {
    let sink: ((state: VoiceStateVm) => void) | null = null;
    voiceWatch.mockImplementation((onState) => {
      sink = onState;
      return Promise.resolve(7);
    });
    renderHook(() => useVoiceStream());
    await waitFor(() => expect(voiceWatch).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(voiceStore.getState().wake).toEqual(WAKE));
    expect(voiceStore.getState().unavailable).toBeNull();
    if (sink === null) {
      throw new Error("the watch never handed over its sink");
    }
    (sink as (state: VoiceStateVm) => void)(LISTENING);
    expect(voiceStore.getState().state).toEqual(LISTENING);
  });

  it("tears the watch down on unmount and forgets the snapshot", async () => {
    let sink: ((state: VoiceStateVm) => void) | null = null;
    voiceWatch.mockImplementation((onState) => {
      sink = onState;
      return Promise.resolve(7);
    });
    const { unmount } = renderHook(() => useVoiceStream());
    await waitFor(() => expect(voiceWatch).toHaveBeenCalledTimes(1));
    if (sink === null) {
      throw new Error("the watch never handed over its sink");
    }
    const deliver = sink as (state: VoiceStateVm) => void;
    deliver(LISTENING);
    expect(voiceStore.getState().state).toEqual(LISTENING);
    unmount();
    expect(voiceUnwatch).toHaveBeenCalledWith(7);
    expect(voiceStore.getState().state).toBeNull();
    // A late snapshot after teardown changes nothing: a chip on a pane that
    // is gone must not light.
    deliver(LISTENING);
    expect(voiceStore.getState().state).toBeNull();
  });

  it("unwatches at once when torn down before the id resolved", async () => {
    let resolve: ((id: number) => void) | null = null;
    voiceWatch.mockImplementation(
      () =>
        new Promise<number>((r) => {
          resolve = r;
        }),
    );
    const { unmount } = renderHook(() => useVoiceStream());
    await waitFor(() => expect(voiceWatch).toHaveBeenCalledTimes(1));
    unmount();
    expect(voiceUnwatch).not.toHaveBeenCalled();
    if (resolve === null) {
      throw new Error("the watch never exposed its resolver");
    }
    (resolve as (id: number) => void)(9);
    await waitFor(() => expect(voiceUnwatch).toHaveBeenCalledWith(9));
  });

  it("leaves availability unasked when the question fails, so absence is never decided from it", async () => {
    voiceAvailability.mockRejectedValue({ code: "internal", message: "no shell" });
    voiceWatch.mockResolvedValue(1);
    renderHook(() => useVoiceStream());
    await waitFor(() => expect(voiceWakeGet).toHaveBeenCalled());
    await waitFor(() => expect(voiceStore.getState().wake).toEqual(WAKE));
    expect(voiceStore.getState().unavailable).toBeUndefined();
  });
});
