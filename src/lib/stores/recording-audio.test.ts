import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import {
  recordingAudioStore,
  resetRecordingAudioForTest,
  setSystemAudioEnabled,
  systemAudioEnabled,
  useSystemAudioEnabled,
} from "@/lib/stores/recording-audio";

afterEach(() => {
  resetRecordingAudioForTest();
});

describe("recording-audio store", () => {
  it("defaults system audio to enabled", () => {
    expect(systemAudioEnabled()).toBe(true);
    expect(recordingAudioStore.getState().systemAudioEnabled).toBe(true);
  });

  it("setSystemAudioEnabled flips the toggle, read back imperatively", () => {
    setSystemAudioEnabled(false);
    expect(systemAudioEnabled()).toBe(false);
    setSystemAudioEnabled(true);
    expect(systemAudioEnabled()).toBe(true);
  });

  it("the hook selector reflects store changes reactively", () => {
    const { result } = renderHook(() => useSystemAudioEnabled());
    expect(result.current).toBe(true);
    act(() => {
      setSystemAudioEnabled(false);
    });
    expect(result.current).toBe(false);
    act(() => {
      setSystemAudioEnabled(true);
    });
    expect(result.current).toBe(true);
  });

  it("reset restores the default-on toggle", () => {
    setSystemAudioEnabled(false);
    resetRecordingAudioForTest();
    expect(systemAudioEnabled()).toBe(true);
  });
});
