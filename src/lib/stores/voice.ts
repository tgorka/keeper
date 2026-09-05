/**
 * The voice mirror (Epic 62, Story 62.5, FR-404–FR-406).
 *
 * A vanilla zustand store created at module load *outside* React, holding what
 * Rust served and nothing it decided: the turn's snapshot as
 * `voice_ipc` streams it, why voice is unavailable if it is, and the wake
 * switch with its phrase and the sentence about what listening costs. Every
 * write goes through an IPC command and comes back as a fresh read — the
 * phrase is validated by `keeper_core::voice::WakePhrase::parse`, never here,
 * and the limits sentence is the port's own `VoicePlatform::limits`, never
 * retyped here.
 *
 * **One store, one stream.** The wake control (this story) and the talk-mode
 * control (62.6) both read the same `state`, fed by the single
 * `voice_watch` channel that {@link useVoiceStream} opens. Two subscribers
 * opening two channels would not double the snapshots — Rust keeps one
 * watcher — but they would race each other's teardown, and the second unmount
 * would silence the first.
 *
 * `state === null` means no snapshot has arrived yet, which the surface
 * treats as idle-and-not-listening: a chip that lit up before Rust said so
 * would be the indicator lying in the direction that matters.
 *
 * Since Epic 64 (Story 64.3, AD-186) a `listening` or `heard` snapshot may
 * carry `level`, the input level Rust measured, smoothed and rate-limited —
 * at most ~25 snapshots a second while it moves, none while it does not.
 * {@link voiceLevel} reads it; a component that does not draw the level
 * should select `state.kind` rather than `state`, so it is not re-rendered
 * for a number it does not show.
 *
 * Since Epic 68 (Story 68.3, AD-215) a `sending` snapshot carries the wait
 * — whom the question went to and when the request left, as Rust's clock
 * stamped it — and an `idle` one how long the last answer's first token
 * took. The words every surface says about it are {@link voiceWaitWord}
 * and {@link voiceLastWaitLine}; the count is {@link useVoiceClock}'s, a
 * 1 s interval that reads the wall clock against Rust's `sentAtMs` — never
 * a counter of its own, so a pane that mounts mid-wait shows the true
 * seconds, and two surfaces never disagree.
 */
import { useEffect, useState } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";

/** The state word once the first piece of the answer has arrived (AD-186). */
export const VOICE_ANSWERING_WORD = "Answering";

/**
 * The line while the model has said nothing yet (AD-215): whose seconds
 * they are, and how many so far — `Waiting for nixie · 12 s`, or
 * `Waiting · 12 s` when Rust has not named the bot. Whole seconds, never
 * negative: a clock that ran backwards is not a negative wait.
 */
export function voiceWaitingLine(bot: string | null | undefined, elapsedMs: number): string {
  const seconds = Math.max(0, Math.floor(elapsedMs / 1000));
  return bot ? `Waiting for ${bot} · ${seconds} s` : `Waiting · ${seconds} s`;
}

/** After the turn (AD-215): how long the first word took — `first word after 28 s`. */
export function voiceFirstWordLine(waitMs: number): string {
  return `first word after ${Math.max(0, Math.floor(waitMs / 1000))} s`;
}

/**
 * The word a `sending` snapshot earns at `nowMs`: {@link VOICE_ANSWERING_WORD}
 * once the first token is in, the counting {@link voiceWaitingLine} while
 * Rust has stamped when the request left, or `null` when it has not — the
 * caller's own "Sending" word then, as before the wait was counted.
 */
export function voiceWaitWord(state: VoiceStateVm | null, nowMs: number): string | null {
  if (state?.kind !== "sending") {
    return null;
  }
  if (state.answering) {
    return VOICE_ANSWERING_WORD;
  }
  if (state.sentAtMs == null) {
    return null;
  }
  return voiceWaitingLine(state.bot, nowMs - state.sentAtMs);
}

/** The after-the-turn line an `idle` snapshot carries, or `null` before a turn has answered. */
export function voiceLastWaitLine(state: VoiceStateVm | null): string | null {
  if (state?.kind !== "idle" || state.lastWaitMs == null) {
    return null;
  }
  return voiceFirstWordLine(state.lastWaitMs);
}

/**
 * The wall clock, re-read once a second while `state` is a wait being
 * counted (`sending`, no first token yet, `sentAtMs` stamped) and left
 * alone otherwise — so the interval exists only while a line is counting.
 */
export function useVoiceClock(state: VoiceStateVm | null): number {
  const counting = state?.kind === "sending" && !state.answering && state.sentAtMs != null;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!counting) {
      return;
    }
    setNow(Date.now());
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [counting]);
  return now;
}

export interface VoiceState {
  /** The turn's latest snapshot, exactly as streamed; `null` before the first. */
  state: VoiceStateVm | null;
  /** Why voice cannot work right now, or `null` when it can. `undefined`
   *  until `voice_availability` has answered, so absence is never decided
   *  from a question not yet asked. */
  unavailable: VoiceUnavailableVm | null | undefined;
  /** The wake switch, phrase and limits sentence; `null` until read. */
  wake: VoiceWakeVm | null;
  applyState: (state: VoiceStateVm) => void;
  applyAvailability: (unavailable: VoiceUnavailableVm | null) => void;
  applyWake: (wake: VoiceWakeVm) => void;
  /** Forget the stream's last snapshot (subscription teardown). The wake
   *  settings and availability survive: they are facts read once, not the
   *  stream's. */
  reset: () => void;
}

/** The vanilla store instance, created once at module load. */
export const voiceStore = createStore<VoiceState>()((set) => ({
  state: null,
  unavailable: undefined,
  wake: null,
  applyState: (state) => set({ state }),
  applyAvailability: (unavailable) => set({ unavailable }),
  applyWake: (wake) => set({ wake }),
  reset: () => set({ state: null }),
}));

/** React binding over {@link voiceStore}. */
export function useVoiceStore<T>(selector: (state: VoiceState) => T): T {
  return useStore(voiceStore, selector);
}

/**
 * Whether the microphone is open right now, per the last snapshot — for the
 * phrase (`idle` with `listeningForWake`) or for a turn (`listening`). This is
 * the fact the chip shows (FR-405: visible whenever it listens); `speaking`,
 * `sending` and `heard` are turn states with the microphone released.
 */
export function isListening(state: VoiceStateVm | null): boolean {
  if (state === null) {
    return false;
  }
  return state.kind === "listening" || (state.kind === "idle" && state.listeningForWake);
}

/**
 * The input level to draw, `0..1`, or `null` where there is none: before
 * any snapshot, in every state with the microphone released, and on a port
 * that has no meter (iOS this epic). `null` is absence — the meter is not
 * drawn — never zero, which would be a measured silence (AD-27).
 */
export function voiceLevel(state: VoiceStateVm | null): number | null {
  if (state?.kind === "listening" || state?.kind === "heard") {
    return state.level;
  }
  return null;
}
