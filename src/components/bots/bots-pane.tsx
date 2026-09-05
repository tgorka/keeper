/**
 * The Bots primary view — where you talk to a model in the app that already
 * holds your drive (Epic 61, Story 61.4, FR-378).
 *
 * The fifth keeper surface, built the way the other four were: one vocabulary
 * (provider → bot → session → message), every decision in `keeper-core`, and no
 * affordance that lies.
 *
 * # Capability gating is absence, and the flag is its own
 *
 * This pane renders only where `CapabilitiesVm.bots` is on, gated at the nav
 * entry and the shell's render chain like every gated surface. The flag is
 * **not** `sessions`: chat needs neither a `git` binary nor `sync.db`, so
 * gating on the sync capability would hide a working surface on a desktop whose
 * `git` is too old — and on a phone, where `bots` is true and the drive is not
 * linked at all (Epic 62). The half that does need the drive is the grant, and
 * {@link BotGrantBar} reads `capabilities.botTools` for exactly that: where it
 * is false the bar is absent, not disabled, and the conversation still works.
 *
 * # What it reads, and what it does on a refusal
 *
 * Three one-shot reads on mount — providers, bots, conversations — through
 * `Promise.allSettled`, the Tasks pane's rule: a refused read must not blank
 * the other two. Rows already read stay on a refusal, and the error explains
 * why nothing newer is known.
 *
 * # Streaming, and the one thing the store is not allowed to decide
 *
 * `botsChatSend` resolves with a subscription id after Rust has already
 * persisted the question and an empty, partial answer row. Deltas append to the
 * row Rust named; the terminal `closed` event **replaces** the row with what
 * Rust stored. The pane never decides an answer is finished and never computes
 * its own metadata, so what is on screen after a stream is byte-for-byte what a
 * reload would show.
 *
 * Stop fires the driver's cancel handle rather than dropping the subscription,
 * so what had arrived is written as a partial row. A partial row renders with
 * its honest caption and a Retry — never hidden, never silently re-sent.
 *
 * # No panel strip beside it
 *
 * Stated in the shell where the branch lives: this surface already has its own
 * document area — the conversation — and an empty strip is a claimant that
 * would take a third of the window to advertise a gesture for opening files
 * this pane does not list. Story 59.13 measured that cost on the Tasks pane.
 *
 * # The transcript gets the height (Story 61.14)
 *
 * The first cut stacked every band above the transcript: header, pins, a
 * wrapping picker, the grant bar and the whole conversation list, then the
 * composer under it. Measured on a real engine at 1440×1050 the chrome was
 * 670px of a 1022px pane — 65% — and the transcript was 353px, which on the
 * owner's machine with nine models and an account banner was one visible line.
 * Nothing in the pane scrolled: the transcript was simply what was left.
 *
 * The shape now is the Tasks pane's (Story 59.1): two levels in one row, the
 * conversation list a surface column that folds to a rail and scrolls inside
 * itself, and the transcript the `flex-1 min-h-0` region of the level beside
 * it. Above the transcript only the bands that are about THIS conversation —
 * the picker (one bounded row) and the grant bar — and below it the composer.
 * DESIGN.md's rule is that columns are the composition; a list beside the
 * thing it opens is a drawer in the cabinet, a list above it is a lid.
 *
 * jsdom lays nothing out, so the test for this is structural — the classes
 * that make the transcript the flexible region and the bands bounded — and the
 * pixels were measured on Chrome through `dev/mock-shell.ts`.
 *
 * # The voice block folds to a line (Epic 64, Story 64.1, AD-184)
 *
 * Epic 63 gave the Mac a voice and put the whole wake block — switch, phrase,
 * language, sentence, note, limits — above the transcript. Measured with
 * `dev/measure-bots.ts` on Chrome at 1440×900 over the dev shell, whose
 * fixture is the owner's own case (a Polish system language with English-only
 * on-device assets, so the refusal is on screen): before, the block was 223px
 * and the transcript **259px of the 872px pane — 29.7%**; folded, the block is
 * a 39px line and the transcript **444px — 50.9%**, the block's height less
 * the line that remains. Unfolded by the person, the block is 257px and the
 * transcript 226px (25.9%) — the disclosure row is what the unfold costs, and
 * unfolding is their choice. The fold is remembered in the pane's own cookie
 * (`bots-pane-fold.ts`) and hydrated here; Settings → Bots keeps the block
 * unfolded, so nothing is one fold further away than it was.
 */
import { MessagesSquare, Plus } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { BotApprovalHost } from "@/components/bots/bot-approval-dialog";
import { BotAttachmentStrip, useBotImagePaste } from "@/components/bots/bot-attachment";
import { BotComposer } from "@/components/bots/bot-composer";
import { BotConversation } from "@/components/bots/bot-conversation";
import { BotEmptyState, type BotsEmptyKind } from "@/components/bots/bot-empty-state";
import { BotGrantBar } from "@/components/bots/bot-grant-bar";
import { BotMetaToggle } from "@/components/bots/bot-message-meta";
import { BotPicker } from "@/components/bots/bot-picker";
import { BotPinsStrip } from "@/components/bots/bot-pins-strip";
import { BOT_SESSION_NEW_LABEL, BotSessionList } from "@/components/bots/bot-session-list";
import { botCommandContext, botCommandHost } from "@/components/bots/bot-slash-menu";
import { BotVoiceMic, BotVoiceStatus } from "@/components/bots/bot-voice-mic";
import { BotVoiceWake } from "@/components/bots/bot-voice-wake";
import { useSurfaceColumn } from "@/components/layout/surface-column";
import { useSpokenStream } from "@/hooks/use-spoken-stream";
import { useVoiceStream } from "@/hooks/use-voice-stream";
import { type CountNoun, countLabel } from "@/lib/count-label";
import type { BotModelVm, BotStreamEvent } from "@/lib/ipc/client";
import {
  botsApprovalAnswer,
  botsBotsList,
  botsChatSend,
  botsChatStop,
  botsMessageRetry,
  botsProvidersList,
  botsSessionOpen,
  botsSessionsList,
} from "@/lib/ipc/client";
import { botsStore, lastAnswer, useBotsStore } from "@/lib/stores/bots";
import {
  botsPaneFoldStore,
  hydrateBotsPaneFold,
  useBotsPaneFold,
} from "@/lib/stores/bots-pane-fold";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { columnFoldStore } from "@/lib/stores/column-fold";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The pane's heading, and the accessible name of the surface itself. */
export const BOTS_PANE_TITLE = "Bots";

/**
 * The one honest sentence under the heading.
 *
 * It says what the surface is over and what it will tell you, in one sentence,
 * lower case after the dash — the `SESSIONS_PANE_SUBTITLE` shape. And it states
 * the egress fact in four words at the end, the way "Recorded locally. Nothing
 * uploads." does, because "where does my question go" is the first thing anyone
 * asks of a surface like this.
 */
export const BOTS_PANE_SUBTITLE =
  "Models you have configured, and the conversations you have had with them — keeper talks to nothing you have not added.";

/** What a failed read says when Rust gave no sentence of its own. */
export const BOTS_READ_FAILED = "keeper couldn't read what models you have configured.";

/**
 * The folded column's counting noun. What the rail says it holds, and it counts
 * the pane's own mirror — the live list Rust returned — never the rows the
 * column happened to draw, which is `count-label.ts`'s enforcement.
 */
export const BOTS_CONVERSATIONS: CountNoun = { one: "conversation", many: "conversations" };

/** The rail control that gives the names back. */
export const BOTS_RAIL_LIST_LABEL = "Conversations";

/** Names the level that holds the transcript, so a test can find its bands. */
export const BOTS_TRANSCRIPT_LEVEL_SLOT = "bots-transcript-level";

/**
 * Where every stream event lands (Stories 61.4, 61.10, 61.11).
 *
 * One event needs more than the store: `approvalAsked` is a blocked tool call
 * in Rust waiting on `botsApprovalAnswer`, so the continuation the store holds
 * is that IPC call, made exactly once with whatever the sheet answered.
 * "always" has already saved its grant by the time it answers
 * (`BotApprovalDialog`), so it approves this call the same way "once" does.
 * A failed answer is logged and the turn is left to Stop or the pane going
 * away, both of which Rust reads as a refusal — never as consent.
 */
export function onStreamEvent(event: BotStreamEvent): void {
  if (event.kind === "approvalAsked") {
    const { requestId } = event.request;
    botsStore.getState().askApproval({
      request: event.request,
      answer: (answer) => {
        void botsApprovalAnswer(requestId, answer !== "deny").catch((raw: unknown) => {
          botsStore.getState().setError(syncErrorMessage(raw, BOTS_READ_FAILED));
        });
      },
    });
    return;
  }
  botsStore.getState().applyStreamEvent(event);
}

export function BotsPane() {
  const providers = useBotsStore((s) => s.providers);
  const bots = useBotsStore((s) => s.bots);
  const sessions = useBotsStore((s) => s.sessions);
  const selectedBotId = useBotsStore((s) => s.selectedBotId);
  const selectedModel = useBotsStore((s) => s.selectedModel);
  const conversation = useBotsStore((s) => s.conversation);
  const streamingId = useBotsStore((s) => s.streamingId);
  const streamingMessageId = useBotsStore((s) => s.streamingMessageId);
  const error = useBotsStore((s) => s.error);
  // The grant's own gate, and the only place this pane reads `botTools`.
  const botTools = useCapabilitiesStore((s) => s.capabilities.botTools);
  // The one voice watcher for the surface (Story 62.5): the wake chip and the
  // talk-mode control both read the store it fills.
  useVoiceStream();
  // Story 64.1: whether the voice block is folded to its line. Restored here
  // rather than in `AppShell`, the notes rail's argument (`bots-pane-fold.ts`).
  const voiceFolded = useBotsPaneFold((s) => s.bands.voice);
  useEffect(() => {
    hydrateBotsPaneFold(document.cookie);
  }, []);
  // The model row the picker last resolved, held here only so the grant bar can
  // read its tool capability. Not in the store: it is a fact about an endpoint
  // read a moment ago, not part of the conversation record.
  const [pickedModel, setPickedModel] = useState<BotModelVm | null>(null);
  // Story 61.12: the images this message will carry, and the tray that shows
  // them. The hook owns the bytes path, the caps and the object-URL lifetime.
  const imagePaste = useBotImagePaste(selectedBotId, selectedModel, pickedModel?.vision ?? null);
  // A stale-read token, the Tasks pane's idiom: a second refresh landing after
  // a first must not restore the older answer.
  const readToken = useRef(0);

  const refresh = useCallback(async () => {
    readToken.current += 1;
    const mine = readToken.current;
    const [providerRead, botRead, sessionRead] = await Promise.allSettled([
      botsProvidersList(),
      botsBotsList(),
      botsSessionsList(false),
    ]);
    if (mine !== readToken.current) {
      return;
    }
    const store = botsStore.getState();
    // `allSettled`, not `all`, and each answer applied on its own: a refused
    // conversation list must not blank the provider list somebody is about to
    // fix in Settings.
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
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Epic 67 (AD-205): a turn the voice heard is sent and spoken by Rust; the
  // pane observes its stream the way it observes its own, and re-reads the
  // list when it closes, because `updated_ms` moved.
  useSpokenStream((event) => {
    onStreamEvent(event);
    if (event.kind === "closed") {
      void refresh();
    }
  });

  const openConversation = (sessionId: string) => {
    void botsSessionOpen(sessionId)
      .then((read) => botsStore.getState().openConversation(read))
      .catch((raw: unknown) => {
        botsStore.getState().setError(syncErrorMessage(raw, BOTS_READ_FAILED));
      });
  };

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
        attachmentIds: imagePaste.take(),
      },
      onStreamEvent,
    )
      // The id is already on the `opened` event, which arrives before this
      // resolves; nothing here needs it. What this catch is for is the failure
      // BEFORE any event: an unknown bot, a refused credential, an endpoint
      // that never answered.
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

  const providerList = providers ?? [];
  const botList = bots ?? [];
  const selectedBot = botList.find((bot) => bot.id === selectedBotId) ?? null;
  const selectedProvider =
    selectedBot === null
      ? null
      : (providerList.find((row) => row.id === selectedBot.providerId) ?? null);
  // Retry belongs on the LAST answer only: an earlier one is a turn the
  // conversation has already built on, and re-sampling it would rewrite
  // everything after it.
  const answer = lastAnswer(conversation);
  const retryable =
    answer !== null && streamingId === null && answer.id !== streamingMessageId ? answer.id : null;

  const empty = emptyKind({
    providerCount: providerList.length,
    botCount: botList.length,
    secretMissing: selectedProvider !== null && selectedProvider.health === "secretMissing",
    hasConversation: conversation !== null,
  });
  /**
   * The conversation list is a surface column: it folds away and can be
   * dragged wider (Story 61.14, the Tasks pane's arrangement).
   *
   * Its rail says how many conversations it holds and gives them back, and it
   * keeps New reachable — a fold suspends a width, never a capability, and
   * starting a conversation is the one thing this column offers that does not
   * need the column open to do.
   */
  const list = useSurfaceColumn("bots-list", {
    rail: [
      {
        id: "conversations",
        icon: MessagesSquare,
        label: BOTS_RAIL_LIST_LABEL,
        detail: countLabel(sessions?.length ?? 0, BOTS_CONVERSATIONS),
        count: sessions?.length ?? 0,
        onSelect: () => columnFoldStore.getState().toggleColumn("bots-list"),
      },
      {
        id: "new",
        icon: Plus,
        label: BOT_SESSION_NEW_LABEL,
        onSelect: () => botsStore.getState().openConversation(null),
      },
    ],
  });

  return (
    <section
      aria-label={BOTS_PANE_TITLE}
      className="flex min-w-0 flex-1 flex-col border-border border-r bg-background last:border-r-0"
    >
      <header className="flex shrink-0 items-start justify-between gap-4 border-border border-b px-6 py-4">
        <div className="min-w-0">
          <h1 className="font-heading text-title">{BOTS_PANE_TITLE}</h1>
          <p className="text-muted-foreground text-sm">{BOTS_PANE_SUBTITLE}</p>
        </div>
        {/* Story 61.8's metadata toggle. It hydrates itself. */}
        <BotMetaToggle />
      </header>

      <BotPinsStrip
        bots={botList}
        selectedBotId={selectedBotId}
        onSelect={(botId) => botsStore.getState().selectBot(botId)}
      />

      {/* The pane's own alert, above both levels: a refused read is about the
          surface, and inside either level it would hide behind that level's
          fold or scroll. */}
      {error !== null && (
        <div
          role="alert"
          className="mx-6 mt-2 shrink-0 rounded-md bg-destructive/10 p-3 text-destructive text-sm"
        >
          {error}
        </div>
      )}

      <div className="flex min-h-0 min-w-0 flex-1">
        {/* Level 1 — the conversations. */}
        <section
          {...list.rootProps}
          className="flex min-w-0 flex-col border-border border-r bg-background last:border-r-0"
        >
          {list.chrome}
          {!list.folded && sessions !== null && (
            <BotSessionList
              sessions={sessions}
              openId={conversation?.session.id ?? null}
              onOpen={openConversation}
              onNew={() => botsStore.getState().openConversation(null)}
              onChanged={() => void refresh()}
              onClosed={() => botsStore.getState().openConversation(null)}
            />
          )}
        </section>
        {list.seam}

        {/* Level 2 — the conversation. The transcript is the one flexible box
            in this column; everything above and below it is `shrink-0` and
            bounded, so the transcript is what grows when the window does. */}
        <div
          data-slot={BOTS_TRANSCRIPT_LEVEL_SLOT}
          className="flex min-h-0 min-w-0 flex-1 flex-col bg-background"
        >
          {botList.length > 0 && (
            <BotPicker
              bots={botList}
              providers={providerList}
              selectedBotId={selectedBotId}
              selectedModel={selectedModel}
              onSelectBot={(botId) => {
                botsStore.getState().selectBot(botId);
                setPickedModel(null);
              }}
              onSelectModel={(model) => botsStore.getState().selectModel(model)}
            />
          )}

          <BotGrantBar
            botTools={botTools}
            provider={selectedProvider}
            botId={selectedBotId}
            model={pickedModel}
          />

          {/* Folded to one line by default (Story 64.1, AD-184); the whole
              block is one click away, and Settings → Bots always has it. */}
          <BotVoiceWake
            fold={{
              folded: voiceFolded,
              onToggle: () => botsPaneFoldStore.getState().toggleBand("voice"),
            }}
          />

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

          <BotAttachmentStrip
            images={imagePaste.images}
            notice={imagePaste.notice}
            onRemove={imagePaste.remove}
          />
          <BotVoiceStatus />
          <BotComposer
            onSend={send}
            // Image staging is the drive half of the shell (Epic 62): a phone
            // has no `bots_image_paste` to call, so a paste there keeps the
            // browser's own behaviour — the honest answer the composer already
            // gives for a `null` context — rather than a refusal.
            pasteContext={botTools ? imagePaste.context : null}
            onPaste={botTools ? imagePaste.handle : undefined}
            // Talk mode (Story 62.6; Epic 67, AD-205): the button starts a
            // turn, and what it hears is sent by Rust to the bot chosen under
            // Bots — the composer never receives it.
            accessory={<BotVoiceMic />}
            onStop={stop}
            streaming={streamingId !== null}
            disabled={selectedBotId === null || selectedModel === null}
            commandContext={botCommandContext({
              providerKind: selectedProvider?.kind ?? null,
              providerCount: providerList.length,
              botId: selectedBotId,
              hasSession: conversation !== null,
              modelTools: pickedModel?.tools ?? null,
            })}
            onCommand={botCommandHost({
              bots: botList,
              newConversation: () => botsStore.getState().openConversation(null),
              selectBot: (botId) => {
                botsStore.getState().selectBot(botId);
                setPickedModel(null);
              },
              selectModel: (model) => botsStore.getState().selectModel(model),
            })}
          />
        </div>
      </div>
      <BotApprovalHost />
    </section>
  );
}

/**
 * Which empty state the surface owes, or `null` when the conversation should
 * render instead.
 *
 * A pure function and exported, so the pane and its test agree about which of
 * the four sentences is owed — the four are deliberately different facts, and
 * the order below is the order of what a person must fix first.
 */
export function emptyKind({
  providerCount,
  botCount,
  secretMissing,
  hasConversation,
}: {
  providerCount: number;
  botCount: number;
  secretMissing: boolean;
  hasConversation: boolean;
}): BotsEmptyKind | null {
  if (providerCount === 0) {
    return "no-provider";
  }
  if (botCount === 0) {
    return "no-bot";
  }
  // Before the no-conversation state: a missing credential is worth saying
  // before somebody types a question that cannot be sent (FR-370).
  if (secretMissing) {
    return "secret-missing";
  }
  if (!hasConversation) {
    return "no-conversation";
  }
  return null;
}
