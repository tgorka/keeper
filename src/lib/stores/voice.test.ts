/**
 * The voice mirror (Story 62.5): what the chip reads, and what a teardown
 * forgets versus keeps.
 */
import { afterEach, describe, expect, it } from "vitest";
import type { VoiceStateVm } from "@/lib/ipc/client";
import {
  isListening,
  VOICE_ANSWERING_WORD,
  voiceLastWaitLine,
  voiceLevel,
  voiceStore,
  voiceWaitWord,
} from "@/lib/stores/voice";

const WAKE = {
  enabled: false,
  phrase: "nixie",
  limits: "what listening costs",
  locale: "en-US",
  localeChosen: null,
  onDeviceLocales: ["en-US"],
  stopPhrase: "stop",
  voiceTarget: null,
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
    expect(isListening({ kind: "listening", heard: "", level: null })).toBe(true);
    expect(isListening({ kind: "listening", heard: "what time", level: 0.4 })).toBe(true);
  });

  it("is off in every turn state where the microphone is released", () => {
    const released: VoiceStateVm[] = [
      { kind: "heard", text: "what time is it", level: 0.2 },
      { kind: "sending", answering: false },
      { kind: "sending", answering: true },
      { kind: "speaking" },
      { kind: "failed", reason: "the port refused" },
    ];
    for (const state of released) {
      expect(isListening(state), state.kind).toBe(false);
    }
  });
});

describe("voiceLevel", () => {
  it("reads the level from a snapshot with the microphone open for a turn", () => {
    expect(voiceLevel({ kind: "listening", heard: "", level: 0.35 })).toBe(0.35);
    expect(voiceLevel({ kind: "heard", text: "what time", level: 0.1 })).toBe(0.1);
    expect(voiceLevel({ kind: "listening", heard: "", level: 0 })).toBe(0);
  });

  it("is null before any snapshot, where the port has not measured, and with the microphone released", () => {
    expect(voiceLevel(null)).toBeNull();
    expect(voiceLevel({ kind: "listening", heard: "", level: null })).toBeNull();
    expect(voiceLevel({ kind: "heard", text: "x", level: null })).toBeNull();
    expect(voiceLevel({ kind: "idle", wake: "nixie", listeningForWake: true })).toBeNull();
    expect(voiceLevel({ kind: "sending", answering: false })).toBeNull();
    expect(voiceLevel({ kind: "speaking" })).toBeNull();
    expect(voiceLevel({ kind: "failed", reason: "x" })).toBeNull();
  });
});

describe("the counted wait (AD-215)", () => {
  const sentAtMs = 1_700_000_000_000;

  it("counts whole seconds from Rust's sentAtMs and names the bot", () => {
    const waiting: VoiceStateVm = { kind: "sending", answering: false, bot: "nixie", sentAtMs };
    expect(voiceWaitWord(waiting, sentAtMs)).toBe("Waiting for nixie · 0 s");
    expect(voiceWaitWord(waiting, sentAtMs + 12_400)).toBe("Waiting for nixie · 12 s");
    // A bot Rust did not name is not invented.
    expect(voiceWaitWord({ kind: "sending", answering: false, sentAtMs }, sentAtMs + 3_000)).toBe(
      "Waiting · 3 s",
    );
    // A wall clock behind Rust's stamp is not a negative wait.
    expect(voiceWaitWord(waiting, sentAtMs - 5_000)).toBe("Waiting for nixie · 0 s");
  });

  it("says Answering once the first token is in, and nothing when Rust stamped nothing", () => {
    expect(
      voiceWaitWord(
        {
          kind: "sending",
          answering: true,
          bot: "nixie",
          sentAtMs,
          firstTokenMs: sentAtMs + 28_550,
        },
        sentAtMs + 40_000,
      ),
    ).toBe(VOICE_ANSWERING_WORD);
    expect(voiceWaitWord({ kind: "sending", answering: false }, sentAtMs)).toBeNull();
    expect(voiceWaitWord({ kind: "speaking" }, sentAtMs)).toBeNull();
    expect(voiceWaitWord(null, sentAtMs)).toBeNull();
  });

  it("says how long the last first word took only once a turn has answered", () => {
    expect(
      voiceLastWaitLine({
        kind: "idle",
        wake: "nixie",
        listeningForWake: true,
        lastWaitMs: 28_550,
      }),
    ).toBe("first word after 28 s");
    expect(voiceLastWaitLine({ kind: "idle", wake: null, listeningForWake: false })).toBeNull();
    expect(
      voiceLastWaitLine({ kind: "idle", wake: null, listeningForWake: false, lastWaitMs: null }),
    ).toBeNull();
    expect(voiceLastWaitLine({ kind: "speaking" })).toBeNull();
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
