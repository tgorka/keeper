/**
 * The voice stream's lifecycle (Epic 62, Story 62.5, FR-405).
 *
 * The SINGLE `voice_watch` subscriber: one channel for the whole Bots
 * surface, mirroring every streamed {@link VoiceStateVm} into
 * {@link voiceStore}. The wake chip (62.5) and the talk-mode control (62.6)
 * are projections of that store and open no channel of their own.
 *
 * Mounted where `capabilities.bots` is true and nowhere else; where it is
 * false the effect does nothing at all, because a surface that does not exist
 * must not hold a microphone watcher open. On cleanup — StrictMode
 * double-mount or unmount — the watch is torn down and the last snapshot
 * forgotten (DW-4: no stream outlives its surface), so a chip cannot keep
 * saying "listening" on a pane that is gone. The sink is gated so a late
 * snapshot after cleanup never mutates the store, and a subscribe failure is
 * swallowed (the chip stays off, which is the honest default).
 *
 * The two one-shot reads — availability and the persisted wake settings —
 * ride the same effect through {@link readVoiceFacts}: they are what the
 * affordance needs before it can decide whether to exist (`unsupported` ⇒
 * absent) and what to show.
 * Re-arming a persisted switch is deliberately **not** done here: arming is
 * the person's act with keeper in front, and the shell holds the turn across
 * pane remounts, so a phrase armed once stays armed until they turn it off.
 *
 * Since Epic 64 (Story 64.3, AD-186) a `listening`/`heard` snapshot also
 * arrives whenever the input level moves — Rust bounds that to ~25 a second
 * and to changes only, so the sink here stays a plain mirror: no throttling,
 * no coalescing, every snapshot is the truth at the moment it was sent.
 */
import { useEffect } from "react";
import { readVoiceFacts } from "@/hooks/use-voice-facts";
import type { VoiceStateVm } from "@/lib/ipc/client";
import { voiceUnwatch, voiceWatch } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { voiceStore } from "@/lib/stores/voice";

export function useVoiceStream(): void {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);

  useEffect(() => {
    if (!bots) {
      return;
    }

    let cancelled = false;
    let watchId: number | null = null;

    const onState = (state: VoiceStateVm) => {
      if (!cancelled) {
        voiceStore.getState().applyState(state);
      }
    };

    readVoiceFacts(() => cancelled);

    voiceWatch(onState)
      .then((id) => {
        if (cancelled) {
          void voiceUnwatch(id);
          return;
        }
        watchId = id;
      })
      .catch(() => {
        // A failed watch is non-fatal: the store stays at `null` (not listening).
      });

    return () => {
      cancelled = true;
      if (watchId !== null) {
        void voiceUnwatch(watchId);
      }
      voiceStore.getState().reset();
    };
  }, [bots]);
}
