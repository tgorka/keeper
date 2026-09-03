/**
 * The voice mirror (Story 62.5): what the chip reads, and what a teardown
 * forgets versus keeps.
 */
import { afterEach, describe, expect, it } from "vitest";
import type { VoiceStateVm } from "@/lib/ipc/client";
import { isListening, voiceStore } from "@/lib/stores/voice";

const WAKE = {
  enabled: false,
  phrase: "nixie",
  limits: "what listening costs",
};

afterEach(() => {
  voiceStore.setState({ state: null, unavailable: undefined, wake: null });
});

describe("isListening", () => {
  it("is off before any snapshot, and off while idle with the microphone released", () => {
    expect(isListening(null)).toBe(false);
    expect(isListening({ kind: "idle", wake: "nixie", listeningForWake: false })).toBe(false);
    expect(isListening({ kind: "idle", wake: null, listeningForWake: false })).toBe(false);
  });

  it("is on while idle with the microphone open for the phrase, and during a turn's listening", () => {
    expect(isListening({ kind: "idle", wake: "nixie", listeningForWake: true })).toBe(true);
    expect(isListening({ kind: "listening", heard: "" })).toBe(true);
    expect(isListening({ kind: "listening", heard: "what time" })).toBe(true);
  });

  it("is off in every turn state where the microphone is released", () => {
    const released: VoiceStateVm[] = [
      { kind: "heard", text: "what time is it" },
      { kind: "sending" },
      { kind: "speaking" },
      { kind: "failed", reason: "the port refused" },
    ];
    for (const state of released) {
      expect(isListening(state), state.kind).toBe(false);
    }
  });
});

describe("voiceStore", () => {
  it("starts with nothing decided: no snapshot, availability unasked, no settings", () => {
    const s = voiceStore.getState();
    expect(s.state).toBeNull();
    expect(s.unavailable).toBeUndefined();
    expect(s.wake).toBeNull();
  });

  it("reset forgets the stream's snapshot and keeps the facts read once", () => {
    const s = voiceStore.getState();
    s.applyState({ kind: "idle", wake: "nixie", listeningForWake: true });
    s.applyAvailability(null);
    s.applyWake(WAKE);
    s.reset();
    expect(voiceStore.getState().state).toBeNull();
    expect(voiceStore.getState().unavailable).toBeNull();
    expect(voiceStore.getState().wake).toEqual(WAKE);
  });
});
