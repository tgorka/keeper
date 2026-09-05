/**
 * Who a spoken turn talks to (Epic 67, Story 67.1, AD-206).
 *
 * A turn the phrase started finishes in Rust with the screen locked
 * (AD-205), so the bot it goes to cannot be read off the screen — there is
 * none — and is never guessed. It is chosen here: a select of the pinned
 * bots, whose unset state is "the pinned bot most recently talked to", the
 * rule `keeper_core::bots::voice_target::resolve` applies when nothing is
 * chosen. The choice is `bots.voice_target`, written through
 * `voice_target_set` and read back as `VoiceWakeVm.voiceTarget`, so the
 * desktop's Settings, the desktop pane's voice fold and the phone's Bots
 * sheet — every place {@link BotVoiceWake} renders — show one answer.
 *
 * The bots are read once on mount (`bots_bots_list`), the same read the
 * picker makes, because this control also lives in Settings, where the Bots
 * store may not have been filled. With no pinned bot there is nothing to
 * choose and the control is absent (AD-27): the turn's own refusal — "choose
 * a bot to talk to under Bots" — is the sentence that says what to do, and a
 * select with one dead option would say it worse.
 *
 * Each option carries the bot's speed (Epic 68, AD-216): the median time to
 * its first token over its last ten answers, `voice_target_speeds`, so
 * choosing a fast bot for voice is a choice made with numbers — "nixie ·
 * first word ~25 s". A bot with fewer than three measured answers shows its
 * name alone; the median is Rust's (`median_first_token`), never computed
 * here, and a read that fails leaves every option nameless of speed rather
 * than absent.
 */
import { useEffect, useId, useState } from "react";
import { Label } from "@/components/ui/label";
import type { BotVm, VoiceTargetSpeedVm } from "@/lib/ipc/client";
import { botsBotsList, voiceTargetSet, voiceTargetSpeeds } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { useVoiceStore, voiceStore } from "@/lib/stores/voice";

/** The control's label. */
export const VOICE_TARGET_LABEL = "Speak to";
/** The option for the setting unset: Rust's rule when nothing is chosen. */
export const VOICE_TARGET_RECENT_LABEL = "Most recently talked to";
/** What the choice is for. */
export const VOICE_TARGET_NOTE =
  "Where a spoken question goes, whatever is open on the screen. A bot never talked to gets a new conversation.";
/** When the choice could not be written. */
const VOICE_TARGET_WRITE_FAILED = "Could not save who to speak to.";

/**
 * The option's words: the name, then — when Rust has a median — the wait
 * for its first word in whole seconds, `~` because it is a median, not a
 * promise. Under a second reads "~1 s" rather than "~0 s": the number says
 * "fast", and zero would say "instant", which nothing measured.
 */
export function voiceTargetOptionLabel(name: string, firstTokenMedianMs: number | null): string {
  if (firstTokenMedianMs === null) {
    return name;
  }
  const seconds = Math.max(1, Math.round(firstTokenMedianMs / 1000));
  return `${name} · first word ~${seconds} s`;
}

/**
 * The picker. Renders nothing until the wake facts and the bots are read, and
 * nothing at all with no pinned bot.
 */
export function BotVoiceTarget({ className }: { className?: string } = {}) {
  const wake = useVoiceStore((s) => s.wake);
  const selectId = useId();
  const [bots, setBots] = useState<BotVm[] | null>(null);
  const [speeds, setSpeeds] = useState<VoiceTargetSpeedVm[]>([]);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void botsBotsList()
      .then((read) => {
        if (!cancelled) {
          setBots(read);
        }
      })
      .catch(() => {
        // The list could not be read: no picker, and the turn's own refusal
        // still says where to go. Settings shows the read's failure itself.
        if (!cancelled) {
          setBots([]);
        }
      });
    void voiceTargetSpeeds()
      .then((read) => {
        if (!cancelled) {
          setSpeeds(read);
        }
      })
      .catch(() => {
        // No numbers: the names still choose. Nothing to say beside them.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (wake === null || bots === null || bots.length === 0) {
    return null;
  }

  // A choice naming a bot that is no longer pinned reads as unset here, the
  // way Rust treats it at send time — so the select never shows a value its
  // options do not have.
  const chosen = bots.some((bot) => bot.id === wake.voiceTarget) ? wake.voiceTarget : null;

  const choose = (botId: string | null) => {
    setBusy(true);
    void voiceTargetSet(botId)
      .then((next) => {
        voiceStore.getState().applyWake(next);
        setRefusal(null);
      })
      .catch((raw: unknown) => {
        setRefusal(syncErrorMessage(raw, VOICE_TARGET_WRITE_FAILED));
      })
      .finally(() => setBusy(false));
  };

  return (
    <div className={className ?? "flex flex-col gap-1"}>
      <div className="flex items-center gap-2">
        <Label htmlFor={selectId} className="min-w-0 flex-1">
          {VOICE_TARGET_LABEL}
        </Label>
        <select
          id={selectId}
          // `""` is the setting unset: a `<select>`'s value is always a
          // string, and `null` would come back as the word "null".
          value={chosen ?? ""}
          disabled={busy}
          onChange={(event) => choose(event.target.value === "" ? null : event.target.value)}
          className="h-9 max-w-64 rounded-md border border-input bg-transparent px-2 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <option value="">{VOICE_TARGET_RECENT_LABEL}</option>
          {bots.map((bot) => (
            <option key={bot.id} value={bot.id}>
              {voiceTargetOptionLabel(
                bot.name,
                speeds.find((speed) => speed.botId === bot.id)?.firstTokenMedianMs ?? null,
              )}
            </option>
          ))}
        </select>
      </div>
      {refusal !== null && (
        <p role="alert" className="text-destructive text-xs">
          {refusal}
        </p>
      )}
      <p className="text-muted-foreground text-xs">{VOICE_TARGET_NOTE}</p>
    </div>
  );
}
