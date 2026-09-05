/**
 * The two one-shot voice facts (Epic 62, Story 62.5; Epic 63, Story 63.5,
 * AD-179): why voice is unavailable if it is, and the persisted wake switch
 * with its phrase.
 *
 * Every voice surface decides whether to exist from the first of these —
 * `voice_availability`, the one runtime answer, never a capability flag —
 * and the wake control draws itself from the second. {@link useVoiceStream}
 * reads both beside the stream for the Bots pane; the Settings sections read
 * them here without a stream, because Settings is a dialog over whatever
 * pane is open and a second `voice_watch` would replace the pane's watcher.
 *
 * A read that fails leaves the store as it was: `unavailable` stays
 * `undefined` (a question not answered, from which absence is never
 * decided) and `wake` stays `null` (no switch to draw).
 */
import { useEffect } from "react";
import { voiceAvailability, voiceWakeGet } from "@/lib/ipc/client";
import { voiceStore } from "@/lib/stores/voice";

/**
 * Ask both facts once and mirror the answers, unless `cancelled()` says the
 * asker has gone. Shared by the hooks so the two never ask differently.
 */
export function readVoiceFacts(cancelled: () => boolean): void {
  void voiceAvailability()
    .then((unavailable) => {
      if (!cancelled()) {
        voiceStore.getState().applyAvailability(unavailable);
      }
    })
    .catch(() => {
      // Unanswered stays `undefined`: the affordance neither shows nor
      // claims absence on a question that failed.
    });

  void voiceWakeGet()
    .then((wake) => {
      if (!cancelled()) {
        voiceStore.getState().applyWake(wake);
      }
    })
    .catch(() => {
      // No settings read means no switch to draw.
    });
}

/** Read the facts whenever `when` flips true (a Settings section's `open`). */
export function useVoiceFacts(when: boolean): void {
  useEffect(() => {
    if (!when) {
      return;
    }
    let cancelled = false;
    readVoiceFacts(() => cancelled);
    return () => {
      cancelled = true;
    };
  }, [when]);
}
