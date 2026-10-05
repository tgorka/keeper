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
 *
 * # The wake VM moves on its own (Epic 97, UX-DR142)
 *
 * The turn models' line in the wake VM changes with no voice command — a
 * models fetch starts or ends, the account goes — and the shell then sends
 * the whole VM on `keeper://voice-wake`. {@link useVoiceWakeMirror} holds
 * ONE listener for every surface showing the line, however many are open,
 * and reads the VM again when one opens and when the document comes back
 * into view. Nothing here polls. Every read is ticketed
 * ({@link askWakeTicket}): an answer that arrives after a newer VM landed —
 * the event, a save's reply, a later read — is dropped, so a slow read never
 * puts back a line the shell has moved past.
 */
import { useEffect } from "react";
import { listenVoiceWake, voiceAvailability, voiceWakeGet } from "@/lib/ipc/client";
import { askWakeTicket, voiceStore } from "@/lib/stores/voice";

/**
 * Read the wake VM once and mirror it, unless a newer one landed meanwhile
 * or `cancelled()` says the asker has gone.
 */
export function readVoiceWake(cancelled: () => boolean = () => false): void {
  const ticket = askWakeTicket();
  void voiceWakeGet()
    .then((wake) => {
      if (!cancelled()) {
        voiceStore.getState().applyWakeRead(ticket, wake);
      }
    })
    .catch(() => {
      // No settings read means no switch to draw.
    });
}

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

  readVoiceWake(cancelled);
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

/** How many open surfaces hold the mirror, and how to let it go. */
let mirrorHolders = 0;
let releaseMirror: (() => void) | null = null;

/** Start the one listener and the visibility re-read for the first holder. */
function startMirror(): () => void {
  let stopped = false;
  let unlisten: (() => void) | null = null;
  void listenVoiceWake((wake) => {
    if (!stopped) {
      voiceStore.getState().applyWake(wake);
    }
  })
    .then((stop) => {
      if (stopped) {
        stop();
      } else {
        unlisten = stop;
      }
    })
    .catch(() => {
      // No event means the line moves on the next open or visibility return.
    });
  const onVisible = () => {
    if (document.visibilityState === "visible") {
      readVoiceWake();
    }
  };
  document.addEventListener("visibilitychange", onVisible);
  return () => {
    stopped = true;
    unlisten?.();
    document.removeEventListener("visibilitychange", onVisible);
  };
}

/**
 * Keep the wake VM current while `open` — a surface showing the turn models'
 * line is in view (the Bots sheet, the unfolded voice fold, a Settings
 * section): read it now, and share the one event listener and visibility
 * re-read every open surface holds. The last surface to close stops both.
 */
export function useVoiceWakeMirror(open: boolean): void {
  useEffect(() => {
    if (!open) {
      return;
    }
    mirrorHolders += 1;
    if (mirrorHolders === 1) {
      releaseMirror = startMirror();
    }
    readVoiceWake();
    return () => {
      mirrorHolders -= 1;
      if (mirrorHolders === 0) {
        releaseMirror?.();
        releaseMirror = null;
      }
    };
  }, [open]);
}
