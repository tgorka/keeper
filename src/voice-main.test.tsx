/**
 * The voice pill window's one job (Story 64.4): subscribe to the shell's
 * snapshot event, draw what arrives, and let go on unmount.
 *
 * The document is mocked to its prop boundary — `voice-pill.test.tsx` owns
 * what it draws. What is asserted here is the seam this file owns: that the
 * window listens rather than invokes, that a payload reaches the document,
 * and that an unmount racing the subscription still unlistens.
 */
import { act, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { VoiceStateVm } from "@/lib/ipc/client";

const drawn = vi.fn<(state: VoiceStateVm | null) => void>();
vi.mock("@/components/voice/voice-pill", () => ({
  VoicePill: ({ state }: { state: VoiceStateVm | null }) => {
    drawn(state);
    return <div data-testid="pill" />;
  },
}));

const unlisten = vi.fn();
let deliver: ((state: VoiceStateVm) => void) | undefined;
let resolveListen: (() => void) | undefined;
const listenVoiceState = vi.fn((onState: (state: VoiceStateVm) => void) => {
  deliver = onState;
  return new Promise<() => void>((resolve) => {
    resolveListen = () => resolve(unlisten);
  });
});
vi.mock("@/lib/ipc/client", () => ({
  listenVoiceState: (onState: (state: VoiceStateVm) => void) => listenVoiceState(onState),
}));

import { VoiceWindow } from "@/voice-main";

beforeEach(() => {
  vi.clearAllMocks();
  deliver = undefined;
  resolveListen = undefined;
});

describe("VoiceWindow", () => {
  it("draws nothing until a snapshot arrives, then draws every one", async () => {
    render(<VoiceWindow />);
    expect(drawn).toHaveBeenLastCalledWith(null);
    expect(listenVoiceState).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveListen?.();
    });
    await act(async () => {
      deliver?.({ kind: "listening", heard: "hello", level: 0.3 });
    });
    expect(drawn).toHaveBeenLastCalledWith({ kind: "listening", heard: "hello", level: 0.3 });
    await act(async () => {
      deliver?.({ kind: "speaking" });
    });
    expect(drawn).toHaveBeenLastCalledWith({ kind: "speaking" });
  });

  it("unlistens on unmount, even when the subscription resolves after it", async () => {
    const view = render(<VoiceWindow />);
    view.unmount();
    // The listener resolved late: the window is gone, so the stop function
    // is called at once rather than kept for a cleanup that already ran.
    await act(async () => {
      resolveListen?.();
    });
    expect(unlisten).toHaveBeenCalledTimes(1);
  });
});
