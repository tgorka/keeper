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
 * `noOnDeviceRecognition` says the language has no on-device asset here and
 * names the ones that do; `noMicrophone` says so. Each sentence is Rust's,
 * rendered from the payload.
 *
 * # The language (Epic 63)
 *
 * Recognition runs on this device and nowhere else, so the language control
 * offers exactly `VoiceWakeVm.onDeviceLocales` — what this device can run
 * itself, probed by the port — plus "Choose for me", which is the setting
 * unset and the system language in force when it can run here. A phone set
 * to Polish whose only on-device asset is English is the case this exists
 * for: the control must not offer Polish, and the refusal Rust sends for the
 * system language sits beside the control that fixes it. An empty list is an
 * AD-27 absence — no control — and the availability sentence explains.
 *
 * # The stop word (Epic 67, Story 67.3, AD-208)
 *
 * One line under the phrase: the word that ends an answer when it is said
 * while keeper is speaking — `VoiceWakeVm.stopPhrase`, "stop" on a fresh
 * install. Saved through the same `voiceWakeSet` as the phrase, validated by
 * `keeper_core::voice::WakePhrase::parse_stop`, and matched by Rust on the
 * barge-in transcript; nothing here listens. Any other speech mid-answer
 * still asks a question (FR-403).
 *
 * # Nothing here decides
 *
 * The phrase is validated by `keeper_core::voice::WakePhrase::parse` and a
 * refusal is rendered letter for letter from the rejection — never
 * re-validated here, so a phrase the box accepted is a phrase Rust accepted.
 * The limits sentence is the port's own `VoicePlatform::limits`, carried in
 * `VoiceWakeVm.limits`. Whether the microphone is open is the turn's, read
 * from the streamed snapshot; the chip is a projection of it and never of a
 * local "did I turn it on" flag, so a device that refused to open shows no
 * chip.
 *
 * # The switch is intent; the port's answer is shown beside it (Epic 65, AD-190)
 *
 * `voiceWakeSet(enabled, …)` writes what the person chose, whatever the
 * port said at arming time. Until Epic 65 a refusal — a Polish system
 * language with no on-device asset, a microphone not yet granted — wrote
 * the switch OFF and said nothing, so the phrase on the owner's phone was
 * never armed. Now the refusal is rendered beside the switch as a refusal
 * (the `unavailable` sentence, or the turn's `failed` reason when the
 * probe did not predict it) with the switch ON, and `keeper_core::voice::
 * should_rearm` has Rust arm the phrase again when the refusal clears: a
 * grant, a language change, keeper back in front, the port's own resume.
 * Nothing here re-arms; this file only stops saving a "no" nobody said.
 *
 * # The chip is small, persistent, and not a hover
 *
 * FR-405: visible whenever it listens. A `role="status"` badge beside the
 * switch, present exactly while the snapshot says the microphone is open —
 * for the phrase or for a turn — and announced when it appears. Someone
 * driving glances at it; nobody hovers.
 *
 * # Folded to one line in the desktop pane (Epic 64, Story 64.1, AD-184)
 *
 * Epic 63 gave the Mac a voice, and this block came with it: 210–260 px of
 * switch, box, picker, sentence, note and limits above the transcript the
 * pane exists for. In the Bots pane the block renders through
 * {@link FoldSection} when the host hands in a `fold`, and folded it is ONE
 * line — {@link voiceFoldedLine}: whether listening is armed, the phrase and
 * the language in force, with the refusal's first clause when the port
 * refuses, so "not allowed" is read without unfolding. One click unfolds to
 * the whole block; the host owns where that is remembered. Without a `fold`
 * (Settings → Bots, the phone's sheet) the block is exactly what it was, so
 * folding removes no path to any control.
 */
import { ChevronDown, ChevronRight, Mic } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { BotVoiceTarget } from "@/components/bots/bot-voice-target";
import { FoldSection } from "@/components/layout/sidebar-group";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import type { VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";
import { voiceAuthorize, voiceAvailability, voiceLocaleSet, voiceWakeSet } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { syncErrorMessage } from "@/lib/stores/sync";
import { isListening, useVoiceStore, voiceStore } from "@/lib/stores/voice";
import { cn } from "@/lib/utils";

/** The switch's label. */
export const WAKE_SWITCH_LABEL = "Listen for a phrase";
/** The phrase box's label. */
export const WAKE_PHRASE_LABEL = "Wake phrase";
/** The save control beside the box. */
export const WAKE_SAVE_LABEL = "Save phrase";
/** The stop word box's label (Epic 67, AD-208). */
export const STOP_PHRASE_LABEL = "Stop word";
/** The save control beside the stop word. */
export const STOP_SAVE_LABEL = "Save stop word";
/** What the stop word does, in one sentence. */
export const STOP_PHRASE_NOTE = "Said while keeper is answering, it ends the answer.";
/** The chip while the microphone is open for the phrase. */
export function wakeListeningLabel(phrase: string | null): string {
  return phrase === null ? "Listening" : `Listening for "${phrase}"`;
}
/** When a write failed for a reason that was not the phrase. */
const WAKE_WRITE_FAILED = "Could not save the wake phrase.";
/** The language control's label. */
export const VOICE_LOCALE_LABEL = "Language";
/** The option for the setting unset: the system language when it runs here. */
export const VOICE_LOCALE_AUTO_LABEL = "Choose for me";
/** What the list is, and is not. */
export const VOICE_LOCALE_NOTE =
  "Recognition runs on this device only, so these are the languages it can run itself — not every language the model understands.";
/** When the language could not be written. */
const VOICE_LOCALE_WRITE_FAILED = "Could not save the language.";

/** English names for the identifiers the port reports; the identifier is
 *  kept beside the name because it is what the OS's own download list shows. */
const LOCALE_NAMES = new Intl.DisplayNames(["en"], { type: "language", fallback: "none" });

/** `en-US` → `American English (en-US)`; an identifier no name is known for
 *  is shown as it is. `_` is the OS's own spelling of the system locale. */
export function voiceLocaleName(locale: string): string {
  const tag = locale.replace("_", "-");
  let name: string | undefined;
  try {
    name = LOCALE_NAMES.of(tag);
  } catch {
    name = undefined;
  }
  return name === undefined || name === tag ? locale : `${name} (${locale})`;
}

/** The sentence naming the language in force. */
export function voiceListeningIn(locale: string): string {
  return `Listens in ${voiceLocaleName(locale)}.`;
}

/** The folded line while the switch is off. */
export const VOICE_FOLDED_OFF = "Listening off";

/**
 * A refusal's first clause: Rust's sentences all open with the fact and
 * follow it, after a comma, a dash or a semicolon, with the remedy — and the
 * remedy is what unfolding (or, on the phone, opening the sheet) is for. A
 * sentence with no such break is carried whole.
 */
export function voiceRefusalClause(message: string): string {
  const clause = /^(.*?)(?:,|;| —)\s/.exec(message);
  return clause?.[1] ?? message;
}

/**
 * The one line the folded block says (AD-184).
 *
 * `Listening for "nixie" · en-US` while the switch is on, `Listening off ·
 * en-US` while it is not — the SETTING, from `VoiceWakeVm`, not the turn's
 * live state, which the status line above the composer already shows. The
 * identifier rather than {@link voiceLocaleName}'s long form, because this is
 * a glance and the unfolded picker spells the name. A refusal contributes
 * {@link voiceRefusalClause}.
 */
export function voiceFoldedLine(wake: VoiceWakeVm, unavailable: VoiceUnavailableVm | null): string {
  const armed = wake.enabled ? wakeListeningLabel(wake.phrase) : VOICE_FOLDED_OFF;
  const parts = [armed, wake.locale];
  if (unavailable !== null) {
    parts.push(voiceRefusalClause(unavailable.message));
  }
  return parts.join(" · ");
}

/** The id of the region the pane's fold discloses. Unique in the document. */
const FOLD_REGION_ID = "bots-voice-wake";

/**
 * The wake phrase band. Absent, or the switch with its chip and sentence.
 * `className` is the host's frame: the phone's sheet draws the pane band by
 * default, and Settings → Bots (Story 63.5) hands in its own row spacing,
 * because the control is one and the frames are two. `fold` is the desktop
 * pane's: given, the block folds to {@link voiceFoldedLine} and the host says
 * whether it is folded and where a toggle is remembered.
 */
export function BotVoiceWake({
  className,
  fold,
}: {
  className?: string;
  fold?: { folded: boolean; onToggle: () => void };
} = {}) {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const wake = useVoiceStore((s) => s.wake);
  const state = useVoiceStore((s) => s.state);
  const switchId = useId();
  const phraseId = useId();
  const stopId = useId();
  const localeId = useId();
  /** The boxes' contents: what the person is typing, seeded from what Rust
   *  holds. Reseeded whenever Rust's answer changes, so a save that came
   *  back normalised or a read that arrived late lands in the box. */
  const [draft, setDraft] = useState("");
  const [stopDraft, setStopDraft] = useState("");
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const held = wake?.phrase ?? null;
  const heldStop = wake?.stopPhrase ?? null;
  useEffect(() => {
    if (held !== null) {
      setDraft(held);
    }
  }, [held]);
  useEffect(() => {
    if (heldStop !== null) {
      setStopDraft(heldStop);
    }
  }, [heldStop]);

  if (!bots || unavailable === undefined || unavailable?.kind === "unsupported" || wake === null) {
    return null;
  }

  const save = (enabled: boolean) => {
    setBusy(true);
    // Switching the phrase on is a deliberate voice act, so it is where the
    // microphone and recogniser are asked for by name (FR-408, 62.6's
    // `voice_authorize`). What is written is the person's choice, whatever
    // the port answered (Epic 65, AD-190): a refusal is mirrored — the
    // sentence beside the switch says what to allow — and the switch stays
    // ON, because "no" was the port's word, not theirs, and keeper arms the
    // phrase itself once the refusal clears. A grant lifts an earlier
    // refusal and nothing else (the mic button's rule): a missing on-device
    // model, read at mount, is still missing.
    const gate = enabled ? voiceAuthorize() : Promise.resolve(null);
    void gate
      .then((unavailable) => {
        if (unavailable !== null) {
          voiceStore.getState().applyAvailability(unavailable);
        } else if (enabled && voiceStore.getState().unavailable?.kind === "notAuthorized") {
          voiceStore.getState().applyAvailability(null);
        }
        return voiceWakeSet(enabled, draft, stopDraft);
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

  const chooseLocale = (locale: string | null) => {
    setBusy(true);
    // The locale in force is Rust's answer, and so is whether it can run
    // here: availability is asked again after the write, because "choose
    // for me" on a phone whose system language has no on-device asset is
    // exactly the refusal this control exists to show beside itself.
    void voiceLocaleSet(locale)
      .then((next) => {
        voiceStore.getState().applyWake(next);
        setRefusal(null);
        return voiceAvailability();
      })
      .then((unavailable) => voiceStore.getState().applyAvailability(unavailable))
      .catch((raw: unknown) => {
        setRefusal(syncErrorMessage(raw, VOICE_LOCALE_WRITE_FAILED));
      })
      .finally(() => setBusy(false));
  };

  const localeRefused = unavailable !== null && "locale" in unavailable;

  const listening = isListening(state);
  const listeningFor = state?.kind === "idle" ? state.wake : null;

  const rows = (
    <>
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
      <form
        className="flex items-center gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          save(wake.enabled);
        }}
      >
        <Label htmlFor={stopId} className="shrink-0">
          {STOP_PHRASE_LABEL}
        </Label>
        <Input
          id={stopId}
          value={stopDraft}
          disabled={busy}
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          onChange={(event) => setStopDraft(event.target.value)}
        />
        <Button type="submit" variant="outline" size="sm" disabled={busy || stopDraft === heldStop}>
          {STOP_SAVE_LABEL}
        </Button>
      </form>
      <p className="text-muted-foreground text-xs">{STOP_PHRASE_NOTE}</p>
      {wake.onDeviceLocales.length > 0 && (
        <div className="flex flex-col gap-1">
          <div className="flex items-center gap-2">
            <Label htmlFor={localeId} className="min-w-0 flex-1">
              {VOICE_LOCALE_LABEL}
            </Label>
            <select
              id={localeId}
              // `""` is the setting unset: a `<select>`'s value is always a
              // string, and `null` would come back as the word "null".
              value={wake.localeChosen ?? ""}
              disabled={busy}
              onChange={(event) =>
                chooseLocale(event.target.value === "" ? null : event.target.value)
              }
              className="h-9 max-w-64 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            >
              <option value="">{VOICE_LOCALE_AUTO_LABEL}</option>
              {wake.onDeviceLocales.map((locale) => (
                <option key={locale} value={locale}>
                  {voiceLocaleName(locale)}
                </option>
              ))}
            </select>
          </div>
          {/* Not while Rust refuses the language in force: "listens in
              Polish" beside "Polish has no on-device asset" is the lie. */}
          {!localeRefused && <p className="text-xs">{voiceListeningIn(wake.locale)}</p>}
          <p className="text-muted-foreground text-xs">{VOICE_LOCALE_NOTE}</p>
        </div>
      )}
      <BotVoiceTarget />
      {refusal !== null && (
        <p role="alert" className="text-destructive text-xs">
          {refusal}
        </p>
      )}
      {unavailable !== null ? (
        // `status`, not `alert`: a permission not yet given or a model not
        // yet downloaded is a state of the phone worth reading, and the
        // sentence says what to do about it.
        <p role="status" className="text-xs">
          {unavailable.message}
        </p>
      ) : (
        // The port refused when the phrase was armed, for a reason the
        // availability probe did not carry: the turn's own reason, Rust's
        // sentence with its remedy, beside the switch it belongs to
        // (AD-190). The switch stays on; keeper re-arms when it clears.
        state?.kind === "failed" && (
          <p role="status" className="text-xs">
            {state.reason}
          </p>
        )
      )}
      <p className="text-muted-foreground text-xs">{wake.limits}</p>
    </>
  );

  if (fold !== undefined) {
    // The pane's band: `shrink-0` whether folded or not, because the
    // transcript beside it is the one flexible box (Story 61.14's contract),
    // and `hidden` on the body takes the block out of the column entirely.
    return (
      <FoldSection
        label={voiceFoldedLine(wake, unavailable)}
        icon={fold.folded ? ChevronRight : ChevronDown}
        folded={fold.folded}
        onToggle={fold.onToggle}
        id={FOLD_REGION_ID}
        labelClassName="text-sm"
        className={cn("shrink-0 border-border border-b px-4 py-1", className)}
        bodyClassName="flex flex-col gap-2 px-2 pt-1 pb-2"
      >
        {rows}
      </FoldSection>
    );
  }

  return (
    <section
      aria-label={WAKE_PHRASE_LABEL}
      className={cn(
        "flex shrink-0 flex-col gap-2",
        className ?? "border-border border-b px-6 py-2",
      )}
    >
      {rows}
    </section>
  );
}
