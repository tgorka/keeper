/**
 * The Bots surface on a phone (Epic 62, Story 62.2, FR-397, FR-398, AD-163).
 *
 * # One thing at a time
 *
 * A 390px viewport has room for one of the desktop pane's two levels, so the
 * conversation list and the conversation are two stack levels in `PhoneShell`
 * rather than two columns: {@link BotsPhoneList} is level 1 over the Inbox,
 * {@link BotsPhoneConversation} is level 2 over the list, and the shell's own
 * back button, edge-swipe and Escape pop them — this file adds no navigation
 * of its own. The bot/model choice, which the desktop draws as a bounded row
 * above the transcript, is a bottom sheet opened from the conversation's
 * header, because a row of two selects is a lid on a phone and the sheet is
 * the idiom the leading drawer already uses.
 *
 * # Reuse, and the one thing forked
 *
 * `BotSessionList`, `BotConversation` (hence `BotMessage` / `BotAnswer`),
 * `BotEmptyState`, `BotPicker`, `BotMetaToggle`, `BotComposer` and the voice
 * pieces are the desktop components unchanged, and every stream event lands
 * through the desktop pane's own `onStreamEvent`. What IS repeated here is the
 * desktop pane's glue — the three-read `refresh`, `send`, `retry`, `stop` —
 * because that glue is inline in `BotsPane`, a file several stories wire at
 * once, and lifting it into a hook would be a refactor of a hotspot rather
 * than a phone surface. The repetition is kept small and named, so the two
 * can be folded together when the pane is quiet.
 *
 * # The transcript gets the height (Story 61.14, held here too)
 *
 * The conversation level is one column: a 52px header, the transcript as the
 * single `min-h-0 flex-1 overflow-y-auto` region, the voice line when there
 * is one, and the composer. Every band but the transcript is `shrink-0`
 * and bounded, which at phone height means the composer plus at most one
 * caption above the keyboard — the wake-phrase band, a switch, a field and
 * two sentences on the desktop, lives in the sheet here for exactly that
 * reason. jsdom lays nothing out, so the test is structural.
 *
 * # What the ear is doing, on the phone's face (Epic 65, Story 65.2, AD-191)
 *
 * The desktop folds the wake band to one truthful line above the transcript
 * (`voiceFoldedLine`, AD-184); the phone had the band in the sheet and the
 * reason a phrase was not listening with it, so the owner's phone said
 * nothing about why. {@link BotsPhoneVoiceLine} is the phone's equivalent:
 * the one caption above the composer, {@link phoneVoiceLine} — the setting
 * (armed or off, the phrase, the language), the refusal's first clause when
 * the port refuses, and the turn's live state while one runs. It replaces
 * the desktop's `BotVoiceStatus` on this tier, so the band count stays at
 * one; tapping it opens the sheet, where the whole sentence and its remedy
 * sit beside the switch that fixes it. The Open Settings control a refused
 * permission needs (FR-408) rides the same line, because the OS will not
 * ask again and the sheet has no way to Settings > keeper.
 *
 * The pinned bots (Story 63.1, FR-412) are on the LIST level for the same
 * reason, not over the transcript where the desktop draws them: a 49px band
 * (`py-2` round a 32px cell, plus its rule) on the conversation level would
 * be 49px the transcript does not get on every phone, for a strip that is
 * only read when choosing whom to talk to. On the list it costs the rows —
 * which scroll — and tapping a pin starts a conversation with that bot, which
 * is what "reach a pinned bot" means on a surface with one level in view.
 *
 * # What is absent
 *
 * No grant bar, no tool rows, no deliverable paths, no image staging: every
 * one reads `capabilities.botTools`, which is `desktop && sync`, and this
 * composition simply never mounts the grant bar or the paste hook. A
 * slash-command that would point at the bar answers with a sentence instead.
 */
import { type Ref, useEffect, useState } from "react";
import { BotComposer } from "@/components/bots/bot-composer";
import { BotConversation } from "@/components/bots/bot-conversation";
import { BOTS_NO_DRIVE_HERE_SENTENCE, BotEmptyState } from "@/components/bots/bot-empty-state";
import { BotMetaToggle } from "@/components/bots/bot-message-meta";
import { BotPicker } from "@/components/bots/bot-picker";
import { BotPinsStrip } from "@/components/bots/bot-pins-strip";
import { BotSessionList } from "@/components/bots/bot-session-list";
import { botCommandContext, botCommandHost } from "@/components/bots/bot-slash-menu";
import { BotVoiceMic, VOICE_OPEN_SETTINGS_LABEL } from "@/components/bots/bot-voice-mic";
import {
  BotVoiceWake,
  voiceFoldedLine,
  voiceRefusalClause,
} from "@/components/bots/bot-voice-wake";
import {
  BOTS_PANE_TITLE,
  BOTS_READ_FAILED,
  emptyKind,
  onStreamEvent,
} from "@/components/bots/bots-pane";
import {
  PHONE_BACK_TO_INBOX,
  PHONE_INBOX_TITLE,
  PhoneBackBar,
} from "@/components/layout/phone-header";
import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useSpokenStream } from "@/hooks/use-spoken-stream";
import { useVoiceStream } from "@/hooks/use-voice-stream";
import type { VoiceStateVm, VoiceUnavailableVm, VoiceWakeVm } from "@/lib/ipc/client";
import {
  botsBotsList,
  botsChatSend,
  botsChatStop,
  botsMessageRetry,
  botsModelsList,
  botsProvidersList,
  botsSessionOpen,
  botsSessionsList,
  iosOpenAppSettings,
} from "@/lib/ipc/client";
import { botsStore, lastAnswer, useBotsStore } from "@/lib/stores/bots";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";
import { useVoiceStore } from "@/lib/stores/voice";
import { cn } from "@/lib/utils";

/** The list level's back control: the level beneath it is the Inbox. */
export const BOTS_PHONE_BACK_TO_INBOX = PHONE_BACK_TO_INBOX;

/** The conversation level's back control: the level beneath it is the list. */
export const BOTS_PHONE_BACK_TO_LIST = `Back to ${BOTS_PANE_TITLE}`;

/** The header control that opens the bot/model sheet, and the sheet's title. */
export const BOTS_PHONE_PICKER_LABEL = "Bot and model";

/** What the sheet says under its title. */
export const BOTS_PHONE_PICKER_DESCRIPTION =
  "Which bot this conversation asks, and what its endpoint should run.";

/**
 * Where this tier chooses a bot, as the composer's no-bot caption names it:
 * the sheet, opened from the header (Story 63.1, FR-411). The desktop says
 * "above", which on this column points at a back bar.
 */
export const BOTS_PHONE_PICKER_PLACE = `in the ${BOTS_PHONE_PICKER_LABEL} sheet`;

/** What the header says while no bot is chosen. */
export const BOTS_PHONE_NO_BOT = "Choose a bot";

/** Names the conversation level's column, so a test can find its bands. */
export const BOTS_PHONE_CONVERSATION_SLOT = "bots-phone-conversation";

/**
 * What `/grant`, `/history` and `/metadata` say here. The desktop's sentences
 * point at a bar, a list and a header control that this composition does not
 * draw, and a sentence that names a control the person cannot see is the
 * affordance AD-27 forbids.
 */
export const BOTS_PHONE_COMMAND_GRANT = BOTS_NO_DRIVE_HERE_SENTENCE;
export const BOTS_PHONE_COMMAND_HISTORY = "Your conversations are one level back.";
export const BOTS_PHONE_COMMAND_METADATA = `The per-answer details toggle is in the ${BOTS_PHONE_PICKER_LABEL} sheet.`;

/** The voice line's accessible name: it is a control, and it opens the sheet. */
export const BOTS_PHONE_VOICE_LINE_LABEL = "Listening state";

/** The live state word the voice line carries while a turn runs (AD-191). */
export const VOICE_PHONE_STATE_WORDS = {
  listening: "Listening",
  heard: "Heard",
  sending: "Sending",
  speaking: "Speaking",
} as const;

/**
 * The one line the phone says about its ear (AD-191): what the desktop's
 * folded line says — the setting and the refusal's first clause — plus the
 * turn's live state. While the turn listens and has heard something, the
 * words heard so far are the line, as they were on the status line this
 * replaces: a person mid-sentence needs to see what is being taken down,
 * not the phrase. A port that refused when the phrase was armed puts the
 * turn in `failed`; when the availability probe did not carry that reason,
 * the turn's own is appended, so the line says why either way. Nothing here
 * decides — every word is Rust's, or the setting's.
 */
export function phoneVoiceLine(
  wake: VoiceWakeVm,
  unavailable: VoiceUnavailableVm | null,
  state: VoiceStateVm | null,
): string {
  if (state?.kind === "listening" && state.heard.length > 0) {
    return `${VOICE_PHONE_STATE_WORDS.listening} · ${state.heard}`;
  }
  const line = voiceFoldedLine(wake, unavailable);
  switch (state?.kind) {
    case "listening":
    case "heard":
    case "sending":
    case "speaking":
      return `${line} · ${VOICE_PHONE_STATE_WORDS[state.kind]}`;
    case "failed":
      return unavailable === null ? `${line} · ${voiceRefusalClause(state.reason)}` : line;
    default:
      return line;
  }
}

/**
 * The band above the phone's composer. Absent exactly where the wake band is
 * absent (AD-27: `capabilities.bots` off, availability unanswered or
 * `unsupported`, the setting unread), so the resting column keeps its three
 * bands. A `status` live region, and a button: tapping opens the sheet.
 */
export function BotsPhoneVoiceLine({ onOpen }: { onOpen: () => void }) {
  const bots = useCapabilitiesStore((s) => s.capabilities.bots);
  const unavailable = useVoiceStore((s) => s.unavailable);
  const wake = useVoiceStore((s) => s.wake);
  const state = useVoiceStore((s) => s.state);

  if (!bots || unavailable === undefined || unavailable?.kind === "unsupported" || wake === null) {
    return null;
  }

  const line = phoneVoiceLine(wake, unavailable, state);
  const failed = unavailable === null && state?.kind === "failed";
  return (
    <div className="flex shrink-0 items-center gap-2 border-border border-t px-4">
      <button
        type="button"
        aria-label={BOTS_PHONE_VOICE_LINE_LABEL}
        onClick={onOpen}
        className="flex h-9 min-w-0 flex-1 items-center text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span
          role="status"
          aria-live="polite"
          data-voice={state?.kind ?? "unknown"}
          className={cn(
            "min-w-0 truncate text-xs",
            failed ? "text-destructive" : "text-muted-foreground",
          )}
        >
          {line}
        </span>
      </button>
      {unavailable?.kind === "notAuthorized" && (
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

/**
 * A stale-read token, the desktop pane's idiom: a second refresh landing after
 * a first must not restore the older answer. Module-level because the two
 * levels are siblings in the shell and both refresh; one token, one order.
 */
let readToken = 0;

/**
 * The three one-shot reads, each applied on its own (`allSettled`): a refused
 * conversation list must not blank the provider list. Mirrors `BotsPane`'s
 * `refresh` line for line — see the header for why it is not shared.
 */
async function refresh(): Promise<void> {
  readToken += 1;
  const mine = readToken;
  const [providerRead, botRead, sessionRead] = await Promise.allSettled([
    botsProvidersList(),
    botsBotsList(),
    botsSessionsList(false),
  ]);
  if (mine !== readToken) {
    return;
  }
  const store = botsStore.getState();
  if (providerRead.status === "fulfilled") {
    store.applyProviders(providerRead.value);
  }
  if (botRead.status === "fulfilled") {
    store.applyBots(botRead.value);
    // Choose the first bot only when nothing is chosen, so a refresh cannot
    // move somebody off the bot they are talking to.
    const first = botRead.value[0];
    if (first !== undefined && botsStore.getState().selectedBotId === null) {
      store.selectBot(first.id);
    }
  }
  if (sessionRead.status === "fulfilled") {
    store.applySessions(sessionRead.value);
  }
  const failure = [providerRead, botRead, sessionRead].find((read) => read.status === "rejected");
  store.setError(
    failure === undefined || failure.status !== "rejected"
      ? null
      : syncErrorMessage(failure.reason, BOTS_READ_FAILED),
  );
}

/**
 * Level 1: the conversations. Reads on mount, holds the surface's one voice
 * watcher (it stays mounted under the conversation, so the watcher outlives a
 * pop back to the list and never doubles), and hands a chosen conversation
 * to the shell through `onOpen` once the read has landed — a row whose read
 * failed stays here with the sentence, rather than pushing an empty level.
 */
export function BotsPhoneList({
  onBack,
  backRef,
  onOpen,
}: {
  /** Pop to the Inbox (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
  /** Push the conversation level; the store already holds what to show. */
  onOpen: () => void;
}) {
  const sessions = useBotsStore((s) => s.sessions);
  const bots = useBotsStore((s) => s.bots);
  const selectedBotId = useBotsStore((s) => s.selectedBotId);
  const conversation = useBotsStore((s) => s.conversation);
  const error = useBotsStore((s) => s.error);
  useVoiceStream();

  useEffect(() => {
    void refresh();
  }, []);

  // Epic 67 (AD-205): a turn the voice heard is sent and spoken by Rust. This
  // level stays mounted under the conversation, so it is where the spoken
  // stream is observed: the store applies each event as it applies the
  // conversation level's own, and the list is re-read when the answer closes.
  useSpokenStream((event) => {
    onStreamEvent(event);
    if (event.kind === "closed") {
      void refresh();
    }
  });

  const open = (sessionId: string) => {
    void botsSessionOpen(sessionId)
      .then((read) => {
        botsStore.getState().openConversation(read);
        onOpen();
      })
      .catch((raw: unknown) => {
        botsStore.getState().setError(syncErrorMessage(raw, BOTS_READ_FAILED));
      });
  };

  return (
    <section
      aria-label={BOTS_PANE_TITLE}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <PhoneBackBar
        backLabel={BOTS_PHONE_BACK_TO_INBOX}
        backTitle={PHONE_INBOX_TITLE}
        backRef={backRef}
        onBack={onBack}
      >
        <h1 className="min-w-0 flex-1 truncate font-heading text-title">{BOTS_PANE_TITLE}</h1>
      </PhoneBackBar>
      {error !== null && (
        <div
          role="alert"
          className="mx-4 mt-2 shrink-0 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
        >
          {error}
        </div>
      )}
      <BotPinsStrip
        bots={bots ?? []}
        selectedBotId={selectedBotId}
        onSelect={(botId) => {
          // A pin on this level is "talk to this bot": choose it and push a
          // fresh conversation, the same two steps New then the sheet would
          // take. The desktop only selects, because its picker and transcript
          // are both in view and the next message goes wherever it is aimed.
          const store = botsStore.getState();
          store.selectBot(botId);
          store.openConversation(null);
          onOpen();
        }}
      />
      {sessions !== null && (
        <BotSessionList
          sessions={sessions}
          openId={conversation?.session.id ?? null}
          onOpen={open}
          onNew={() => {
            botsStore.getState().openConversation(null);
            onOpen();
          }}
          onChanged={() => void refresh()}
          onClosed={() => botsStore.getState().openConversation(null)}
        />
      )}
    </section>
  );
}

/**
 * Level 2: the conversation. The transcript is the column's one flexible
 * region; the header, the caption and the composer are bounded.
 */
export function BotsPhoneConversation({
  onBack,
  backRef,
}: {
  /** Pop to the list (the shell's `onBack`). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
}) {
  const providers = useBotsStore((s) => s.providers);
  const bots = useBotsStore((s) => s.bots);
  const selectedBotId = useBotsStore((s) => s.selectedBotId);
  const selectedModel = useBotsStore((s) => s.selectedModel);
  const conversation = useBotsStore((s) => s.conversation);
  const streamingId = useBotsStore((s) => s.streamingId);
  const streamingMessageId = useBotsStore((s) => s.streamingMessageId);
  const error = useBotsStore((s) => s.error);
  const [pickerOpen, setPickerOpen] = useState(false);

  const providerList = providers ?? [];
  const botList = bots ?? [];
  const selectedBot = botList.find((bot) => bot.id === selectedBotId) ?? null;
  const selectedProvider =
    selectedBot === null
      ? null
      : (providerList.find((row) => row.id === selectedBot.providerId) ?? null);

  // On the desktop the first model is chosen as a consequence of the picker's
  // read, and the picker is always mounted. Here it is mounted only while the
  // sheet is open, so the same consequence is drawn here: a bot with no model
  // chosen gets its endpoint's first, once, and never over a choice somebody
  // made. The sheet's picker shows the read's failure when it is opened.
  const providerId = selectedBot?.providerId ?? null;
  const target = selectedBot?.target ?? null;
  useEffect(() => {
    if (providerId === null || target === null || selectedModel !== null) {
      return;
    }
    let cancelled = false;
    void botsModelsList(providerId, target)
      .then((models) => {
        const first = models[0];
        if (!cancelled && first !== undefined && botsStore.getState().selectedModel === null) {
          botsStore.getState().selectModel(first.id);
        }
      })
      .catch(() => {
        // The sheet's picker says why when it is opened; the composer stays
        // disabled, which is the honest state for a bot with nothing to run.
      });
    return () => {
      cancelled = true;
    };
  }, [providerId, target, selectedModel]);

  const send = (text: string) => {
    if (selectedBotId === null || selectedModel === null) {
      return;
    }
    botsStore.getState().setError(null);
    void botsChatSend(
      {
        sessionId: conversation?.session.id ?? null,
        botId: selectedBotId,
        model: selectedModel,
        text,
        // Image staging is the drive half of the shell; a phone has none.
        attachmentIds: [],
      },
      onStreamEvent,
    )
      .catch((raw: unknown) => {
        botsStore.getState().setError(syncErrorMessage(raw, BOTS_READ_FAILED));
      })
      // The list order is `updated_ms`, which the send just changed.
      .finally(() => void refresh());
  };

  const retry = (messageId: string) => {
    if (conversation === null || selectedModel === null) {
      return;
    }
    botsStore.getState().setError(null);
    void botsMessageRetry(
      { sessionId: conversation.session.id, messageId, model: selectedModel },
      onStreamEvent,
    ).catch((raw: unknown) => {
      botsStore.getState().setError(syncErrorMessage(raw, BOTS_READ_FAILED));
    });
  };

  const stop = () => {
    if (streamingId !== null) {
      void botsChatStop(streamingId);
    }
  };

  // Retry belongs on the LAST answer only (the desktop pane's rule).
  const answer = lastAnswer(conversation);
  const retryable =
    answer !== null && streamingId === null && answer.id !== streamingMessageId ? answer.id : null;

  const empty = emptyKind({
    providerCount: providerList.length,
    botCount: botList.length,
    secretMissing: selectedProvider !== null && selectedProvider.health === "secretMissing",
    hasConversation: conversation !== null,
  });

  const host = botCommandHost({
    bots: botList,
    newConversation: () => botsStore.getState().openConversation(null),
    selectBot: (botId) => botsStore.getState().selectBot(botId),
    selectModel: (model) => botsStore.getState().selectModel(model),
  });

  const pickedTitle =
    selectedBot === null
      ? BOTS_PHONE_NO_BOT
      : selectedModel === null
        ? selectedBot.name
        : `${selectedBot.name} · ${selectedModel}`;

  return (
    <div
      data-slot={BOTS_PHONE_CONVERSATION_SLOT}
      className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
    >
      <PhoneBackBar
        backLabel={BOTS_PHONE_BACK_TO_LIST}
        backTitle={BOTS_PANE_TITLE}
        backRef={backRef}
        onBack={onBack}
      >
        <button
          type="button"
          aria-label={BOTS_PHONE_PICKER_LABEL}
          onClick={() => setPickerOpen(true)}
          className="flex h-11 min-w-0 flex-1 items-center justify-start text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <span className="min-w-0 truncate text-sm">{pickedTitle}</span>
        </button>
      </PhoneBackBar>

      {error !== null && (
        <div
          role="alert"
          className="mx-4 mt-2 shrink-0 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
        >
          {error}
        </div>
      )}

      {empty === null ? (
        <BotConversation
          messages={conversation?.messages ?? []}
          streamingMessageId={streamingMessageId}
          retryableId={retryable}
          onRetry={retry}
        />
      ) : (
        <BotEmptyState
          kind={empty}
          onAction={() => {
            if (empty === "no-conversation") {
              botsStore.getState().openConversation(null);
              return;
            }
            primaryViewStore.getState().setView("settings");
          }}
        />
      )}

      <BotsPhoneVoiceLine onOpen={() => setPickerOpen(true)} />
      <BotComposer
        onSend={send}
        onStop={stop}
        // Talk mode (Story 62.6; Epic 67, AD-205): the button starts a turn,
        // and what it hears is sent by Rust to the bot chosen under Bots —
        // the composer never receives it.
        accessory={<BotVoiceMic />}
        streaming={streamingId !== null}
        disabled={selectedBotId === null || selectedModel === null}
        pickerPlace={BOTS_PHONE_PICKER_PLACE}
        commandContext={botCommandContext({
          providerKind: selectedProvider?.kind ?? null,
          providerCount: providerList.length,
          botId: selectedBotId,
          hasSession: conversation !== null,
          // Not read here: the drive half that would use it is absent, and a
          // `null` is "the endpoint did not say", which is true of this level.
          modelTools: null,
        })}
        onCommand={(command) => {
          switch (command.name) {
            case "grant":
              return BOTS_PHONE_COMMAND_GRANT;
            case "history":
              return BOTS_PHONE_COMMAND_HISTORY;
            case "metadata":
              return BOTS_PHONE_COMMAND_METADATA;
            default:
              return host(command);
          }
        }}
      />

      <Sheet open={pickerOpen} onOpenChange={setPickerOpen}>
        <SheetContent
          side="bottom"
          className="gap-0 pb-[var(--safe-bottom)] motion-reduce:animate-none motion-reduce:transition-none"
        >
          <SheetHeader>
            <SheetTitle>{BOTS_PHONE_PICKER_LABEL}</SheetTitle>
            <SheetDescription>{BOTS_PHONE_PICKER_DESCRIPTION}</SheetDescription>
          </SheetHeader>
          {botList.length > 0 && (
            <BotPicker
              bots={botList}
              providers={providerList}
              selectedBotId={selectedBotId}
              selectedModel={selectedModel}
              onSelectBot={(botId) => botsStore.getState().selectBot(botId)}
              onSelectModel={(model) => botsStore.getState().selectModel(model)}
            />
          )}
          <div className="flex items-center justify-between gap-2 px-4 py-2">
            <span className="text-muted-foreground text-xs">Answer details under each reply</span>
            <BotMetaToggle />
          </div>
          <BotVoiceWake />
        </SheetContent>
      </Sheet>
    </div>
  );
}
