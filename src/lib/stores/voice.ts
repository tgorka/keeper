/**
 * The voice mirror (Epic 62, Story 62.5, FR-404–FR-406).
 *
 * A vanilla zustand store created at module load *outside* React, holding what
 * Rust served and nothing it decided: the turn's snapshot as
 * `voice_ipc` streams it, why voice is unavailable if it is, and the wake
 * switch with its phrase and the sentence about what listening costs. Every
 * write goes through an IPC command and comes back as a fresh read — the
 * phrase is validated by `keeper_core::voice::WakePhrase::parse`, never here,
 * and the limits sentence is `keeper_core::voice::LISTENING_LIMITS`, never
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
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";

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
