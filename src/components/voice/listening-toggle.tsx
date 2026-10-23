/**
 * Listening, as one action every menu calls (Epic 68, Story 68.4, AD-218).
 *
 * This module is the one verb the menus call — the ⌘K palette's
 * `voice-toggle-listening`, Settings › Voice, the account view — and the
 * tray calls the same Rust command from Rust. One command,
 * `voice_wake_toggle`, so every surface is one setting.
 *
 * # Nothing here decides
 *
 * Which way the switch flips, and whether switching on asks for the
 * microphone first, is Rust's (`voice_ipc::voice_wake_toggle` carries the
 * rule the pane's switch applied in `VoiceWake.save`). The store is
 * written from what Rust answered, never from a local "did I click" flag,
 * and availability is read again after the flip — as the locale picker
 * does — so a refusal the port answered on switching on is shown beside the
 * switch, whichever surface flipped it.
 *
 * # Where the control exists at all
 *
 * {@link VoiceWake}'s conditions, and every failing one is an AD-27 absence:
 * `voice_availability` answered, and the answer not `unsupported` (AD-179:
 * voice exists on availability alone); plus the wake facts read
 * (`wake !== null`), because the label *is* the state and a chip with no
 * state would be a chip lying.
 */

import { sayFailure } from "@/lib/failure";
import type { VoiceWakeVm } from "@/lib/ipc/client";
import { voiceAvailability, voiceWakeToggle } from "@/lib/ipc/client";
import { useVoiceStore, voiceStore } from "@/lib/stores/voice";
import { cn } from "@/lib/utils";

/** The label while the switch is off — the folded band's own words. */
export const LISTENING_OFF_LABEL = "Listening off";

/** The label while the switch is on: the state, then the phrase it listens for. */
export function listeningLabel(wake: VoiceWakeVm): string {
  return wake.enabled ? `Listening on · ${wake.phrase}` : LISTENING_OFF_LABEL;
}

/** What the control is, for assistive technology; the visible text is the state. */
export const LISTENING_TOGGLE_LABEL = "Listening for the wake phrase";

/**
 * Flip the switch through Rust and mirror what it stored.
 *
 * Exported because the command palette's entry is the other caller (UX-DR42's
 * one-verb rule). Reads nothing from the store first: the palette can be
 * opened with no voice control mounted, so Rust reads the stored switch
 * itself and flips it. Availability is re-read after
 * the write for the reason the module doc gives.
 */
export async function toggleVoiceListening(): Promise<void> {
  const next = await voiceWakeToggle();
  voiceStore.getState().applyWake(next);
  const unavailable = await voiceAvailability();
  voiceStore.getState().applyAvailability(unavailable);
}

/**
 * The control: an `aria-pressed` button whose text is the state, the
 * metadata chip's idiom. `phone` draws it at the 44 pt height the phone's
 * bars use; the desktop draws the chip. Absent under the module doc's rule.
 */
export function ListeningToggle({ phone = false }: { phone?: boolean } = {}) {
  const unavailable = useVoiceStore((s) => s.unavailable);
  const wake = useVoiceStore((s) => s.wake);

  if (unavailable === undefined || unavailable?.kind === "unsupported" || wake === null) {
    return null;
  }

  return (
    <button
      type="button"
      aria-label={LISTENING_TOGGLE_LABEL}
      aria-pressed={wake.enabled}
      onClick={() =>
        void toggleVoiceListening().catch(
          sayFailure("voice.toggle", "keeper couldn't switch listening for the wake phrase."),
        )
      }
      className={cn(
        "shrink-0 rounded-md border border-border",
        phone ? "h-11 px-3 text-sm" : "px-2 py-1 text-xs",
        "hover:bg-accent hover:text-accent-foreground",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
        wake.enabled && "bg-accent text-accent-foreground",
      )}
    >
      {listeningLabel(wake)}
    </button>
  );
}
