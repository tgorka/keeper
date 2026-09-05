/**
 * Talk mode you can see (Epic 62, Story 62.6, FR-407, FR-408, NFR-51,
 * AD-170/AD-171): a microphone control with three legible states, the
 * transcript as it forms, and a stop that abandons.
 *
 * # Two pieces, one store
 *
 * {@link BotVoiceMic} is the button, mounted in the composer's row, and
 * {@link BotVoiceStatus} is the line above it — the interim transcript, the
 * spoken-answer notice, a failure, or a permission's remedy. They share the
 * voice store that `useVoiceStream` fills from the one `voice_watch` channel
 * (62.5) and open nothing of their own; each is a projection of the snapshot
 * Rust streamed, never of a local "did I press it" flag, so a device that
 * refused to open shows idle.
 *
 * # The three states are told apart by words, not colour
 *
 * The button's accessible name and visible label change with the state —
 * Talk, Cancel this question, Stop this answer — as does its glyph and its
 * `data-state`; `aria-pressed` is true while the microphone is open. A
 * driver glancing over, a screen reader, and a test read the same fact.
 *
 * # The turn is Rust's from the phrase to the last word (Epic 67, AD-205)
 *
 * This button starts a turn and can stop one; it sends nothing and speaks
 * nothing. What the turn heard is sent by the shell (`voice_ipc::transition`
 * → `bots_ipc::send_spoken`) to the bot chosen under Bots (AD-206), and the
 * answer is read aloud by the shell when the stream closes — so a turn
 * finishes with the screen locked, which is the point. A turn the button
 * started takes the same path as one the phrase started: one turn, not two.
 * The pane observes it — the snapshot here, the stream's events through
 * `listenSpokenStream` — and drives nothing. (Until Epic 67 a button turn
 * landed in the composer to be checked; that hand-off was the webview's,
 * and a webview is not there when the phone is in a pocket.)
 *
 * # Stop abandons
 *
 * While listening, the button sends `voice_stop`, which is `Abandoned` in
 * `keeper_core::voice`: the microphone is released and nothing heard is sent
 * (NFR-51). While the answer is spoken, it sends `voice_stop_speaking`,
 * which ends the turn as if the utterance had finished. Either way a wake
 * phrase that is switched on is re-armed by `keeper_core::voice::Turn`:
 * Stop ends this turn, and only the switch ends listening.
 *
 * # Asking, once, by name (FR-408)
 *
 * The first press is the first deliberate voice act, so it asks — through
 * `voice_authorize`, which decides in `keeper_core::voice::authorization`
 * whether anything is still undetermined and shows the recogniser's dialog,
 * then the microphone's, with the sentences `gen/apple/project.yml`
 * declares. Never at launch. A refusal is a state: the store's
 * `unavailable` becomes `notAuthorized` and the status line renders Rust's
 * sentence with an Open Settings control, because the OS will not ask
 * again and the person needs the way to Settings > keeper.
 */
import { Mic, Square, Volume2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { VoiceStateVm } from "@/lib/ipc/client";
import {
  iosOpenAppSettings,
  voiceAuthorize,
  voiceStart,
  voiceStop,
  voiceStopSpeaking,
} from "@/lib/ipc/client";
import { botsStore } from "@/lib/stores/bots";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { useVoiceStore, voiceStore } from "@/lib/stores/voice";

/** The button while idle: pressing it starts a turn. */
export const VOICE_TALK_LABEL = "Talk";
/** The button while the microphone is open for a turn. It ends this question, not listening: a switched-on phrase is re-armed by the turn. */
export const VOICE_STOP_LISTENING_LABEL = "Cancel this question";
/** The button while the answer is read aloud. It ends this answer, not listening. */
export const VOICE_STOP_SPEAKING_LABEL = "Stop this answer";
/** The status line while listening, before anything is heard. */
export const VOICE_LISTENING_STATUS = "Listening";
/** The status line while the answer is read aloud. */
export const VOICE_SPEAKING_STATUS = "Speaking the answer";
/** The status line once the question is heard and on its way to the bot. */
export const VOICE_HEARD_STATUS = "Heard — sending it";
/** The status line while the answer is on its way. */
export const VOICE_SENDING_STATUS = "Sending what you said";
/** The control beside a permission that was refused. */
export const VOICE_OPEN_SETTINGS_LABEL = "Open Settings";
/** When the shell rejected a start for a reason it did not name. */
const VOICE_START_FAILED = "Could not start listening.";

/** The button's state, from the snapshot. */
export type VoiceMicState = "idle" | "listening" | "speaking";

/** Which of the three faces the button wears for a snapshot. */
export function micState(state: VoiceStateVm | null): VoiceMicState {
  if (state?.kind === "listening") {
    return "listening";
  }
  if (state?.kind === "speaking") {
    return "speaking";
  }
  return "idle";
}

/**
 * The microphone button. Absent where voice is unsupported or its
 * availability is not yet known (AD-27: absence, never a dead control).
 */
export function BotVoiceMic() {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const state = useVoiceStore((s) => s.state);

  if (!bots || unavailable === undefined || unavailable?.kind === "unsupported") {
    return null;
  }

  const face = micState(state);

  const start = () => {
    void voiceAuthorize()
      .then((refusal) => {
        if (refusal !== null) {
          voiceStore.getState().applyAvailability(refusal);
          return;
        }
        // A grant lifts an earlier refusal and nothing else: a missing
        // on-device model, read at mount, is still missing.
        if (voiceStore.getState().unavailable?.kind === "notAuthorized") {
          voiceStore.getState().applyAvailability(null);
        }
        return voiceStart();
      })
      .catch((raw: unknown) => {
        botsStore.getState().setError(syncErrorMessage(raw, VOICE_START_FAILED));
      });
  };

  const press = () => {
    switch (face) {
      case "idle":
        start();
        return;
      case "listening":
        void voiceStop().catch(() => {});
        return;
      case "speaking":
        void voiceStopSpeaking().catch(() => {});
        return;
    }
  };

  const label =
    face === "listening"
      ? VOICE_STOP_LISTENING_LABEL
      : face === "speaking"
        ? VOICE_STOP_SPEAKING_LABEL
        : VOICE_TALK_LABEL;

  return (
    <Button
      type="button"
      variant={face === "idle" ? "outline" : "default"}
      size="sm"
      aria-label={label}
      aria-pressed={face === "listening"}
      data-state={face}
      onClick={press}
    >
      {face === "listening" ? (
        <Square aria-hidden="true" />
      ) : face === "speaking" ? (
        <Volume2 aria-hidden="true" />
      ) : (
        <Mic aria-hidden="true" />
      )}
      {label}
    </Button>
  );
}

/**
 * The line above the composer: what the turn is doing, in words, or why it
 * cannot. Absent while there is nothing to say (the 61.14 height contract:
 * a band exists only while it earns its height).
 */
export function BotVoiceStatus() {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const state = useVoiceStore((s) => s.state);

  if (!bots || unavailable === undefined || unavailable?.kind === "unsupported") {
    return null;
  }

  if (unavailable !== null) {
    // `status`, not `alert`: a permission refused or a model not downloaded
    // is a state of the phone, and the sentence — Rust's — says what to do.
    // Only a refusal gets the Settings control; a missing model names the
    // language and where to download it, which Settings > keeper cannot.
    return (
      <div className="flex shrink-0 items-center gap-2 border-border border-t px-6 py-2">
        <p role="status" className="min-w-0 flex-1 text-xs">
          {unavailable.message}
        </p>
        {unavailable.kind === "notAuthorized" && (
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              // Best-effort deep link through the Rust opener; never re-prompts.
              void iosOpenAppSettings().catch(() => {});
            }}
          >
            {VOICE_OPEN_SETTINGS_LABEL}
          </Button>
        )}
      </div>
    );
  }

  if (state === null) {
    return null;
  }

  switch (state.kind) {
    case "listening":
      return (
        <p
          role="status"
          aria-live="polite"
          data-voice="listening"
          className="shrink-0 border-border border-t px-6 py-2 text-xs"
        >
          <span className="text-muted-foreground">{VOICE_LISTENING_STATUS}</span>
          {state.heard.length > 0 && <span className="ml-2">{state.heard}</span>}
        </p>
      );
    case "heard":
      return (
        <p
          role="status"
          data-voice="heard"
          className="shrink-0 border-border border-t px-6 py-2 text-muted-foreground text-xs"
        >
          {VOICE_HEARD_STATUS}
        </p>
      );
    case "sending":
      return (
        <p
          role="status"
          data-voice="sending"
          className="shrink-0 border-border border-t px-6 py-2 text-muted-foreground text-xs"
        >
          {VOICE_SENDING_STATUS}
        </p>
      );
    case "speaking":
      return (
        <p
          role="status"
          aria-live="polite"
          data-voice="speaking"
          className="shrink-0 border-border border-t px-6 py-2 text-muted-foreground text-xs"
        >
          {VOICE_SPEAKING_STATUS}
        </p>
      );
    case "failed":
      return (
        <p
          role="alert"
          data-voice="failed"
          className="shrink-0 border-border border-t px-6 py-2 text-destructive text-xs"
        >
          {state.reason}
        </p>
      );
    case "idle":
      return null;
  }
}
