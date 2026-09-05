/**
 * The voice pill's document (Epic 64, Story 64.4, FR-436–FR-439, AD-185).
 *
 * What a person sees over Maps or a browser while keeper listens behind it:
 * one lamp, one state word, the words as they are recognised, and a level
 * bar. Nothing here decides anything — the state is Rust's snapshot, the
 * level is Rust's measurement, and whether the window is on screen at all is
 * `voice_window.rs`'s. This document only draws what it is handed; when it
 * is handed nothing, or an idle-unarmed snapshot, it draws an empty pill
 * that the shell has already hidden.
 *
 * # The three things it draws
 *
 * - **The state**, as the lamp's shape and a word: Listening, Heard,
 *   Thinking (`sending` before the first token), Answering (after it),
 *   Speaking, or the failure sentence. `aria-live="polite"` on the word, so
 *   a reader hears the turn move without being interrupted by it.
 * - **The words**, in one line, ellipsised at the *start*: the newest words
 *   are the ones that say "it is still hearing me", so they stay visible and
 *   the sentence's beginning is what gives way. Done with `dir="rtl"` on the
 *   line and a `<bdi>` around the text, which is the way to put the ellipsis
 *   at the line's start without a script that measures widths — the
 *   isolate keeps the words themselves left-to-right, punctuation included.
 * - **The level**, as a horizontal fill in `primary` on a `muted` track.
 *   Absent (not zero) when Rust has not measured one — before the first
 *   buffer, in every state with the microphone released, and on a port with
 *   no meter (AD-27). Under reduced motion the fill is set, never eased: a
 *   width that jumps to the reading is a static fill at each reading; a
 *   short eased transition is what smooths it otherwise.
 *
 * DESIGN.md tokens only, no new colours: the pill wears `background` with a
 * `border` hairline, the lamp carries the state's colour as its second
 * channel, and the failure sentence is `destructive`.
 */
import { Lamp, type LampState } from "@/components/ui/lamp";
import { useReducedMotion } from "@/hooks/use-reduced-motion";
import type { VoiceStateVm } from "@/lib/ipc/client";
import { voiceLevel } from "@/lib/stores/voice";
import { cn } from "@/lib/utils";

/** The state word while the microphone is open for a turn. */
export const PILL_LISTENING = "Listening";
/** The state word once the transcript is final. */
export const PILL_HEARD = "Heard";
/** The state word while the model has said nothing yet. */
export const PILL_THINKING = "Thinking";
/** The state word once the first piece of the answer has arrived. */
export const PILL_ANSWERING = "Answering";
/** The state word while the answer is read aloud. */
export const PILL_SPEAKING = "Speaking";

/** The armed glance's line: what keeper is listening for. */
export function pillArmedLine(wake: string | null): string {
  return wake === null ? "Listening for the phrase" : `Listening for \u201C${wake}\u201D`;
}

/** The lamp's shape for a snapshot: the one vocabulary every indicator speaks. */
export function pillLamp(state: VoiceStateVm | null): LampState {
  switch (state?.kind) {
    case "listening":
    case "speaking":
      return "live";
    case "heard":
    case "sending":
      return "working";
    case "failed":
      return "fault";
    default:
      return "idle";
  }
}

/** The state word for a snapshot, or `null` while there is nothing to say. */
export function pillWord(state: VoiceStateVm | null): string | null {
  switch (state?.kind) {
    case "listening":
      return PILL_LISTENING;
    case "heard":
      return PILL_HEARD;
    case "sending":
      return state.answering ? PILL_ANSWERING : PILL_THINKING;
    case "speaking":
      return PILL_SPEAKING;
    case "failed":
      return state.reason;
    case "idle":
      return state.listeningForWake ? pillArmedLine(state.wake) : null;
    default:
      return null;
  }
}

/** The words as they arrive, or `""` in a state that has none to show. */
export function pillWords(state: VoiceStateVm | null): string {
  switch (state?.kind) {
    case "listening":
      return state.heard;
    case "heard":
      return state.text;
    default:
      return "";
  }
}

export function VoicePill({ state }: { state: VoiceStateVm | null }) {
  const reduced = useReducedMotion();
  const word = pillWord(state);
  const words = pillWords(state);
  const level = voiceLevel(state);
  const failed = state?.kind === "failed";
  return (
    <div
      data-voice-pill={state?.kind ?? "none"}
      className="flex h-screen w-screen select-none flex-col justify-center gap-1 overflow-hidden border border-border bg-background px-3 text-foreground"
    >
      <div className="flex min-w-0 items-center gap-2 text-xs leading-4">
        {/* The word beside it is the state; the lamp is its shape. */}
        <Lamp state={pillLamp(state)} label={null} />
        <span
          role="status"
          aria-live="polite"
          className={cn(
            "min-w-0 truncate font-medium",
            // A failure sentence gets the whole line; a state word shares it.
            failed ? "shrink text-destructive" : "shrink-0",
          )}
        >
          {word}
        </span>
        {words.length > 0 && (
          <p
            dir="rtl"
            data-slot="voice-pill-words"
            className="min-w-0 flex-1 truncate text-left text-muted-foreground"
          >
            <bdi>{words}</bdi>
          </p>
        )}
      </div>
      {level !== null && (
        <div
          data-slot="voice-pill-level"
          className="h-0.5 w-full shrink-0 overflow-hidden rounded-full bg-muted"
        >
          <div
            data-slot="voice-pill-fill"
            data-motion={reduced ? "static" : "eased"}
            className={cn(
              "h-full bg-primary",
              !reduced && "transition-[width] duration-100 ease-out",
            )}
            style={{ width: `${Math.round(Math.min(Math.max(level, 0), 1) * 100)}%` }}
          />
        </div>
      )}
    </div>
  );
}
