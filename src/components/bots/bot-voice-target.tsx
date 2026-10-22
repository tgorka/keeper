/**
 * Who a spoken turn talks to (Epic 67, Story 67.1, AD-206; AD-384).
 *
 * A turn the phrase started finishes in Rust with the screen locked
 * (AD-205), so where it goes cannot be read off the screen — there is
 * none — and is never guessed. It is chosen here: a select of the pinned
 * bots, then the person's agents — every agent of every agents' database
 * this device holds (108.2) — whose unset state is "the pinned bot most
 * recently talked to", the rule `keeper_core::bots::voice_target::resolve`
 * applies when nothing is chosen. The choice is `bots.voice_target` — a bot
 * id, or an agent's `agent:<agent>` — written through `voice_target_set` and
 * read back as
 * `VoiceWakeVm.voiceTarget`, so the desktop's Settings, the desktop pane's
 * voice fold and the phone's Bots sheet — every place {@link BotVoiceWake}
 * renders — show one answer.
 *
 * The bots are read once on mount (`bots_bots_list`), the same read the
 * picker makes, because this control also lives in Settings, where the Bots
 * store may not have been filled. The agents (`voice_agent_targets`) are
 * read on mount and on focus, and again after a growing pause while none is
 * listed or the chosen one is not: keeper may not have opened the agents'
 * databases yet. A chosen agent not (yet) listed still shows as chosen, and
 * is never rewritten here — if it really is gone, Rust refuses the turn with
 * a sentence naming this control. A choice that named one of the agents' old
 * Matrix rooms was cleared at launch, and Rust's one-time notice says so
 * here until it is dismissed (UX-DR-E108-1). With nothing to
 * choose the control is absent (AD-27): the turn's own refusal — "choose a
 * bot to talk to under Bots" — is the sentence that says what to do, and a
 * select with one dead option would say it worse.
 *
 * Each bot option carries the bot's speed (Epic 68, AD-216): the median time
 * to its first token over its last ten answers, `voice_target_speeds`, so
 * choosing a fast bot for voice is a choice made with numbers — "nixie ·
 * first word ~25 s". A bot with fewer than three measured answers shows its
 * name alone; the median is Rust's (`median_first_token`), never computed
 * here, and a read that fails leaves every option nameless of speed rather
 * than absent.
 */
import { useCallback, useEffect, useId, useRef, useState } from "react";
import { Label } from "@/components/ui/label";
import { logFailure } from "@/lib/failure";
import type { BotVm, VoiceAgentTargetVm, VoiceTargetSpeedVm } from "@/lib/ipc/client";
import {
  botsBotsList,
  voiceAgentTargets,
  voiceOldTargetNoticeDismiss,
  voiceOldTargetNoticeGet,
  voiceTargetSet,
  voiceTargetSpeeds,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";
import { useVoiceStore, voiceStore } from "@/lib/stores/voice";

/** The control's label. */
export const VOICE_TARGET_LABEL = "Speak to";
/** The option for the setting unset: Rust's rule when nothing is chosen. */
export const VOICE_TARGET_RECENT_LABEL = "Most recently talked to";
/** What the choice is for. */
export const VOICE_TARGET_NOTE =
  "Where a spoken question goes, whatever is open on the screen. A bot never talked to gets a new conversation.";
/** The group of the person's agents. */
export const VOICE_TARGET_AGENT_GROUP = "Your agents";
/** A chosen agent keeper has not listed (yet). */
export const VOICE_TARGET_UNLISTED_LABEL = "An agent (not listed yet)";
/** Puts Rust's one-time notice about a cleared old choice away. */
export const VOICE_TARGET_NOTICE_DISMISS = "Dismiss";
/** When the choice could not be written. */
const VOICE_TARGET_WRITE_FAILED = "Could not save who to speak to.";
/** What a stored choice of one of the person's agents starts with (Rust's `AGENT_TARGET_PREFIX`). */
const AGENT_TARGET_PREFIX = "agent:";
/** The first pause before looking for the agents again, doubled up to the most. */
const RELIST_FIRST_MS = 5_000;
const RELIST_MOST_MS = 60_000;

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
 * nothing at all with no pinned bot, no conversation of the assistant's and
 * none of them chosen.
 */
export function BotVoiceTarget({ className }: { className?: string } = {}) {
  const wake = useVoiceStore((s) => s.wake);
  const selectId = useId();
  const [bots, setBots] = useState<BotVm[] | null>(null);
  const [agents, setAgents] = useState<VoiceAgentTargetVm[]>([]);
  const [speeds, setSpeeds] = useState<VoiceTargetSpeedVm[]>([]);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);
  const generation = useRef(0);

  // The newest listing wins: a slow one that a later look overtook is dropped.
  const relist = useCallback(() => {
    const mine = ++generation.current;
    void voiceAgentTargets()
      .then((read) => {
        if (alive.current && mine === generation.current) {
          setAgents(read);
        }
      })
      // Rust never refuses this read; with no shell there is nothing to list.
      .catch(logFailure("voice.agentTargets"));
  }, []);

  useEffect(() => {
    alive.current = true;
    relist();
    void botsBotsList()
      .then((read) => {
        if (alive.current) {
          setBots(read);
        }
      })
      .catch((raw: unknown) => {
        logFailure("voice.botsList")(raw);
        // The list could not be read: no bots to offer, and the turn's own
        // refusal still says where to go. Settings shows the read's failure itself.
        if (alive.current) {
          setBots([]);
        }
      });
    void voiceOldTargetNoticeGet()
      .then((read) => {
        if (alive.current) {
          setNotice(read);
        }
      })
      // No notice to show: the choice reads as unset all the same.
      .catch(logFailure("voice.oldTargetNotice"));
    void voiceTargetSpeeds()
      .then((read) => {
        if (alive.current) {
          setSpeeds(read);
        }
      })
      // No numbers: the names still choose. Nothing to say beside them.
      .catch(logFailure("voice.targetSpeeds"));
    return () => {
      alive.current = false;
    };
  }, [relist]);

  const stored = wake?.voiceTarget ?? null;
  const unlisted =
    stored?.startsWith(AGENT_TARGET_PREFIX) === true &&
    !agents.some((agent) => agent.target === stored);
  const waiting = agents.length === 0 || unlisted;
  useEffect(() => {
    if (!waiting) return;
    let delay = RELIST_FIRST_MS;
    let timer = 0;
    const look = () => {
      timer = window.setTimeout(() => {
        relist();
        delay = Math.min(delay * 2, RELIST_MOST_MS);
        look();
      }, delay);
    };
    look();
    return () => clearTimeout(timer);
  }, [waiting, relist]);

  if (
    wake === null ||
    bots === null ||
    (bots.length === 0 && agents.length === 0 && !unlisted && notice === null)
  ) {
    return null;
  }

  // A choice naming a bot that is no longer pinned reads as unset here, the
  // way Rust treats it at send time — so the select never shows a value its
  // options do not have. One of the person's agents shows as chosen listed
  // or not: Rust either finds it or says so.
  const chosen =
    stored !== null &&
    (stored.startsWith(AGENT_TARGET_PREFIX) || bots.some((bot) => bot.id === stored))
      ? stored
      : null;

  const dismissNotice = () => {
    setNotice(null);
    void voiceOldTargetNoticeDismiss().catch(logFailure("voice.oldTargetNoticeDismiss"));
  };

  const choose = (target: string | null) => {
    setBusy(true);
    void voiceTargetSet(target)
      .then((next) => {
        voiceStore.getState().applyWake(next);
        setRefusal(null);
      })
      .catch((raw: unknown) => {
        logFailure("voice.targetSet")(raw);
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
          // An agent added since the list was read is offered here.
          onFocus={relist}
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
          {agents.length > 0 && (
            <optgroup label={VOICE_TARGET_AGENT_GROUP}>
              {agents.map((agent) => (
                <option key={agent.target} value={agent.target}>
                  {agent.name}
                </option>
              ))}
            </optgroup>
          )}
          {unlisted && stored !== null && (
            <optgroup label={VOICE_TARGET_AGENT_GROUP}>
              <option value={stored}>{VOICE_TARGET_UNLISTED_LABEL}</option>
            </optgroup>
          )}
        </select>
      </div>
      {refusal !== null && (
        <p role="alert" className="text-destructive text-xs">
          {refusal}
        </p>
      )}
      {notice !== null && (
        <p role="status" className="flex items-start gap-2 text-xs">
          <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{notice}</span>
          <button
            type="button"
            onClick={dismissNotice}
            className="shrink-0 text-muted-foreground underline underline-offset-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
          >
            {VOICE_TARGET_NOTICE_DISMISS}
          </button>
        </p>
      )}
      <p className="text-muted-foreground text-xs">{VOICE_TARGET_NOTE}</p>
    </div>
  );
}
