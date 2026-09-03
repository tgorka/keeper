/**
 * The wake phrase — a switch that is off until chosen, the phrase a person
 * types, a chip that shows whenever it listens, and the sentence about what
 * listening costs (Epic 62, Story 62.5, FR-404–FR-406, AD-168/AD-169).
 *
 * # Where the affordance exists at all
 *
 * Three conditions, and every failing one is an AD-27 *absence*, never a
 * disabled control:
 *
 * 1. **`capabilities.bots` must be on.** No pane, no phrase.
 * 2. **`voice_availability` must have answered.** `undefined` is a question
 *    not yet asked, and absence must not be decided from it.
 * 3. **The answer must not be `unsupported`.** That is every build without a
 *    voice port — the desktop today — and the reason a person would look for
 *    is the platform disclosure in Settings (62.3), not a band in the pane
 *    saying a feature is missing on a machine that never offered it.
 *
 * The other three unavailability answers are not absence. `notAuthorized`
 * is a prompt: the switch stays and the sentence beside it says what to
 * allow (asking by name is 62.6's, FR-408). `noOnDeviceModel` names the
 * language to download and says why keeper will not use a server instead;
 * `noMicrophone` says so. Each sentence is Rust's, rendered from the payload.
 *
 * # Nothing here decides
 *
 * The phrase is validated by `keeper_core::voice::WakePhrase::parse` and a
 * refusal is rendered letter for letter from the rejection — never
 * re-validated here, so a phrase the box accepted is a phrase Rust accepted.
 * The limits sentence is `keeper_core::voice::LISTENING_LIMITS`, carried in
 * `VoiceWakeVm.limits`. Whether the microphone is open is the turn's, read
 * from the streamed snapshot; the chip is a projection of it and never of a
 * local "did I turn it on" flag, so a device that refused to open shows no
 * chip.
 *
 * # The chip is small, persistent, and not a hover
 *
 * FR-405: visible whenever it listens. A `role="status"` badge beside the
 * switch, present exactly while the snapshot says the microphone is open —
 * for the phrase or for a turn — and announced when it appears. Someone
 * driving glances at it; nobody hovers.
 */
import { Mic } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { voiceAuthorize, voiceWakeSet } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { isListening, useVoiceStore, voiceStore } from "@/lib/stores/voice";

/** The switch's label. */
export const WAKE_SWITCH_LABEL = "Listen for a phrase";
/** The phrase box's label. */
export const WAKE_PHRASE_LABEL = "Wake phrase";
/** The save control beside the box. */
export const WAKE_SAVE_LABEL = "Save phrase";
/** The chip while the microphone is open for the phrase. */
export function wakeListeningLabel(phrase: string | null): string {
  return phrase === null ? "Listening" : `Listening for "${phrase}"`;
}
/** When a write failed for a reason that was not the phrase. */
const WAKE_WRITE_FAILED = "Could not save the wake phrase.";

/** The wake phrase band. Absent, or the switch with its chip and sentence. */
export function BotVoiceWake() {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const wake = useVoiceStore((s) => s.wake);
  const state = useVoiceStore((s) => s.state);
  const switchId = useId();
  const phraseId = useId();
  /** The box's contents: what the person is typing, seeded from what Rust
   *  holds. Reseeded whenever Rust's answer changes, so a save that came
   *  back normalised or a read that arrived late lands in the box. */
  const [draft, setDraft] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const held = wake?.phrase ?? null;
  useEffect(() => {
    if (held !== null) {
      setDraft(held);
    }
  }, [held]);

  if (!bots || unavailable === undefined || unavailable?.kind === "unsupported" || wake === null) {
    return null;
  }

  const save = (enabled: boolean) => {
    setBusy(true);
    // Switching the phrase on is a deliberate voice act, so it is where the
    // microphone and recogniser are asked for by name (FR-408, 62.6's
    // `voice_authorize`). A refusal is mirrored — the sentence beside the
    // switch says what to allow — and the switch is written **off**: a switch
    // persisted on with nothing able to listen would be the AD-27 lie. The
    // phrase is kept, so the choice survives a trip to Settings and back.
    const gate = enabled ? voiceAuthorize() : Promise.resolve(null);
    void gate
      .then((unavailable) => {
        if (unavailable !== null) {
          voiceStore.getState().applyAvailability(unavailable);
        }
        return voiceWakeSet(enabled && unavailable === null, draft);
      })
      .then((next) => {
        voiceStore.getState().applyWake(next);
        setRefusal(null);
      })
      .catch((raw: unknown) => {
        // A refusal is the phrase's, in Rust's words. The switch stays where
        // Rust left it, because nothing was persisted.
        setRefusal(syncErrorMessage(raw, WAKE_WRITE_FAILED));
      })
      .finally(() => setBusy(false));
  };

  const listening = isListening(state);
  const listeningFor = state?.kind === "idle" ? state.wake : null;

  return (
    <section
      aria-label={WAKE_PHRASE_LABEL}
      className="flex shrink-0 flex-col gap-2 border-border border-b px-6 py-2"
    >
      <div className="flex items-center gap-2">
        <Label htmlFor={switchId} className="min-w-0 flex-1">
          {WAKE_SWITCH_LABEL}
        </Label>
        {listening && (
          // `status` + `aria-live`: the chip is announced when it appears, and
          // it is drawn, not hovered — the person may be looking at another
          // app's map when it lights.
          <Badge role="status" aria-live="polite" variant="default">
            <Mic aria-hidden="true" />
            {wakeListeningLabel(listeningFor)}
          </Badge>
        )}
        <Switch
          id={switchId}
          checked={wake.enabled}
          disabled={busy}
          onCheckedChange={(checked) => save(checked)}
        />
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          save(wake.enabled);
        }}
      >
        <Label htmlFor={phraseId} className="sr-only">
          {WAKE_PHRASE_LABEL}
        </Label>
        <Input
          id={phraseId}
          value={draft}
          disabled={busy}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          onChange={(event) => setDraft(event.target.value)}
        />
        <Button type="submit" variant="outline" size="sm" disabled={busy || draft === held}>
          {WAKE_SAVE_LABEL}
        </Button>
      </form>
      {refusal !== null && (
        <p role="alert" className="text-destructive text-xs">
          {refusal}
        </p>
      )}
      {unavailable !== null && (
        // `status`, not `alert`: a permission not yet given or a model not
        // yet downloaded is a state of the phone worth reading, and the
        // sentence says what to do about it.
        <p role="status" className="text-xs">
          {unavailable.message}
        </p>
      )}
      <p className="text-muted-foreground text-xs">{wake.limits}</p>
    </section>
  );
}
