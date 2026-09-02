/**
 * The Bots mirror (Epic 61, Story 61.4).
 *
 * A vanilla zustand store created at module load *outside* React, holding what
 * Rust served: the providers, the pinned bots, the conversation list, and the
 * conversation currently open with its messages. Nothing here is a source of
 * truth — every fact is a projection of `keeper.db`, and every write goes
 * through an IPC command and comes back as a fresh read.
 *
 * **Why the open conversation lives here rather than in the pane.** The pane
 * unmounts on every surface switch — there is no lazy loading and no mounted
 * state preservation in this shell — so a conversation held in component state
 * is a conversation forgotten every time you glance at Files. That is the exact
 * defect `hydrateFilesTree` exists to fix, and the shell's own comment records
 * it: "the tree forgot itself whenever you looked at something else".
 *
 * **The streaming rules, which are the only interesting thing in this file.**
 *
 * 1. A delta appends to the row named by `streamingId` and to nothing else. An
 *    append that could not find its row is dropped rather than guessed at: two
 *    answers streaming into one conversation is a record nobody can read, and
 *    the shell already prevents it by naming the row on the `opened` event.
 * 2. `closed` **replaces** the row with what Rust stored. The store never
 *    decides that an answer is finished, and never computes its own metadata:
 *    the producer measured the time-to-first-token and read the usage, and a
 *    mirror that kept its own accumulated string would be a second answer that
 *    could differ from the one on disk.
 * 3. A `closed` carrying a `reason` leaves the row `partial`, because that is
 *    what Rust wrote. The caption the pane draws is a projection of the row,
 *    never of a local "did it fail" flag — so a reload shows the same sentence.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { BotApprovalAnswer, BotApprovalRequest } from "@/components/bots/bot-approval-dialog";
import type {
  BotContextBundleVm,
  BotConversationVm,
  BotMessageVm,
  BotProviderVm,
  BotSessionVm,
  BotStreamEvent,
  BotToolCallVm,
  BotVm,
} from "@/lib/ipc/client";

/**
 * One tool call waiting on a person, with the continuation that answers it
 * (Story 61.10, FR-387).
 *
 * The callback lives in the store rather than the answer being written to a
 * row, because the thing waiting is a blocking tool call inside one chat turn:
 * Rust's approval port is a function that returns consent
 * (`bots_tools.rs:74`), so what the UI holds is that function's other end. It
 * is a live continuation and never persisted; a reload finds no ask, which is
 * correct — the call it belonged to died with the process, and a grant is the
 * only thing that outlives a turn.
 */
export interface BotApprovalAsk {
  /** What is being asked. */
  request: BotApprovalRequest;
  /** Called exactly once with what the person answered. */
  answer: (answer: BotApprovalAnswer) => void;
}

export interface BotsState {
  /** Configured providers, in the order they were added. `null` until read. */
  providers: BotProviderVm[] | null;
  /** Pinned bots, in the hand-set order. `null` until read. */
  bots: BotVm[] | null;
  /** Conversations, newest activity first. `null` until read. */
  sessions: BotSessionVm[] | null;
  /** The bot the composer will ask, or `null` when none is chosen yet. */
  selectedBotId: string | null;
  /** The model the composer will send, or `null` when none is chosen yet. */
  selectedModel: string | null;
  /** The conversation on screen, or `null` for a fresh one. */
  conversation: BotConversationVm | null;
  /**
   * The subscription id of the answer currently arriving, or `null`.
   *
   * Held rather than derived from `messages.some(m => m.partial)`, because a
   * partial row is *also* what a dead stream leaves behind — the two states
   * look identical in the data and must not look identical on screen. This is
   * what Stop acts on.
   */
  streamingId: string | null;
  /** The message id the arriving answer is being written into, or `null`. */
  streamingMessageId: string | null;
  /** The last read or send failure, ready to print, or `null`. */
  error: string | null;
  /**
   * The tool call waiting on a person, or `null`.
   *
   * One at a time: the tool loop runs one call at a time, so a second ask
   * while one is open would be a queue nobody asked for.
   */
  pendingApproval: BotApprovalAsk | null;
  /**
   * The tool calls each answer ran, keyed by message id, as the turn reported
   * them (Story 61.11, FR-388).
   *
   * Live only: the durable record of a call is its audit row, and a replayed
   * conversation shows the stored `toolCallCount` instead. Keyed by message
   * rather than held on the row because `BotMessageVm` is what Rust stored,
   * and a row that carried rows Rust never persisted would claim a record
   * that is not there.
   */
  toolRows: Record<string, BotToolCallVm[]>;
  /**
   * What the model was told about the drive, per answer, keyed by message id
   * (Story 61.11, FR-391). Absent where keeper does not know, which is not
   * the same as none (AD-27).
   */
  contexts: Record<string, BotContextBundleVm>;

  applyProviders: (providers: BotProviderVm[]) => void;
  applyBots: (bots: BotVm[]) => void;
  applySessions: (sessions: BotSessionVm[]) => void;
  selectBot: (botId: string | null) => void;
  selectModel: (model: string | null) => void;
  openConversation: (conversation: BotConversationVm | null) => void;
  applyStreamEvent: (event: BotStreamEvent) => void;
  setError: (error: string | null) => void;

  /**
   * Whether an answer shows its metadata caption (Story 61.8, FR-384).
   *
   * A mirror of the persisted `bots.message_details` setting, not the setting
   * itself: the writer is `botsMessageDetailsSet` and this is what the pane and
   * the palette both read so the two surfaces cannot disagree while both are
   * alive. Deliberately **not** in `EMPTY` — `reset` clears a conversation, and
   * a preference the person set is not part of one.
   */
  metaShown: boolean;
  setMetaShown: (metaShown: boolean) => void;
  askApproval: (ask: BotApprovalAsk) => void;
  clearApproval: () => void;
  reset: () => void;
}

/** The pane's blank slate, so `reset` and the initial state cannot drift. */
const EMPTY = {
  providers: null,
  bots: null,
  sessions: null,
  selectedBotId: null,
  selectedModel: null,
  conversation: null,
  streamingId: null,
  streamingMessageId: null,
  error: null,
  pendingApproval: null,
  toolRows: {},
  contexts: {},
} as const;

/** The vanilla store instance, created once at module load and shared app-wide. */
export const botsStore = createStore<BotsState>()((set) => ({
  ...EMPTY,
  // Off until the persisted setting is read, so the first paint is the shipped
  // default rather than a caption that appears and then vanishes. Not spread
  // from `EMPTY`: `reset` must not un-set what somebody chose.
  metaShown: false,
  setMetaShown: (metaShown) => set({ metaShown }),

  applyProviders: (providers) => set({ providers }),
  applyBots: (bots) => set({ bots }),
  applySessions: (sessions) => set({ sessions }),
  selectBot: (selectedBotId) => set({ selectedBotId, selectedModel: null }),
  selectModel: (selectedModel) => set({ selectedModel }),
  openConversation: (conversation) =>
    set({ conversation, streamingId: null, streamingMessageId: null }),
  setError: (error) => set({ error }),
  askApproval: (pendingApproval) => set({ pendingApproval }),
  clearApproval: () => set({ pendingApproval: null }),
  reset: () => set({ ...EMPTY }),

  applyStreamEvent: (event) =>
    set((state) => {
      switch (event.kind) {
        case "opened": {
          // The conversation may be new, and both rows are already persisted.
          // Replacing the whole conversation rather than appending into the
          // one on screen is what makes a first send and a follow-up one code
          // path: the shell sent the session it wrote, so this cannot end up
          // showing a title the store minted itself.
          const existing = state.conversation;
          const carried =
            existing !== null && existing.session.id === event.session.id ? existing.messages : [];
          return {
            conversation: {
              session: event.session,
              messages: [...carried, event.user, event.assistant],
            },
            streamingId: event.subscriptionId,
            streamingMessageId: event.assistant.id,
            error: null,
          };
        }
        case "delta":
          return { conversation: appendTo(state, event.text) };
        // Reasoning is recorded by Rust and rendered by 61.8's metadata
        // caption; the tool-call *name* fragment is superseded by the row that
        // follows once the call has run. Ignoring them here is deliberate:
        // appending a model's private reasoning into the answer would put text
        // in the record the model did not answer with.
        case "reasoning":
        case "toolCall":
        case "firstToken":
          return {};
        // The approval round trip is the pane's: it holds the IPC call that
        // answers, and the store holds only the ask (`askApproval`).
        case "approvalAsked":
          return {};
        case "toolResult": {
          const target = state.streamingMessageId;
          if (target === null) {
            return {};
          }
          const held = state.toolRows[target] ?? [];
          return { toolRows: { ...state.toolRows, [target]: [...held, event.call] } };
        }
        case "context": {
          const target = state.streamingMessageId;
          if (target === null) {
            return {};
          }
          return { contexts: { ...state.contexts, [target]: event.bundle } };
        }
        case "closed": {
          const conversation = state.conversation;
          if (conversation === null) {
            return { streamingId: null, streamingMessageId: null };
          }
          return {
            conversation: {
              session: conversation.session,
              messages: conversation.messages.map((message) =>
                message.id === event.message.id ? event.message : message,
              ),
            },
            streamingId: null,
            streamingMessageId: null,
            error: event.reason,
          };
        }
      }
    }),
}));

/**
 * Append `text` to the row the current stream is writing into.
 *
 * Exported nowhere and named because the alternative is this arithmetic inlined
 * in a `switch` arm with the drop-on-miss rule as a comment. The rule is the
 * point: a delta whose row is not on screen is dropped, never appended to
 * whatever happens to be last.
 */
function appendTo(state: BotsState, text: string): BotConversationVm | null {
  const conversation = state.conversation;
  const target = state.streamingMessageId;
  if (conversation === null || target === null) {
    return conversation;
  }
  return {
    session: conversation.session,
    messages: conversation.messages.map((message) =>
      message.id === target ? { ...message, content: message.content + text } : message,
    ),
  };
}

/**
 * React selector hook over {@link botsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useBotsStore<T>(selector: (state: BotsState) => T): T {
  return useStore(botsStore, selector);
}

/**
 * The last assistant row of the open conversation, or `null`.
 *
 * What Retry acts on, and a pure function so the pane and its test agree about
 * which row that is. The **last** assistant row rather than the last row of
 * any kind: a conversation whose final row is the person's own question has
 * nothing to retry.
 */
export function lastAnswer(conversation: BotConversationVm | null): BotMessageVm | null {
  if (conversation === null) {
    return null;
  }
  for (let index = conversation.messages.length - 1; index >= 0; index -= 1) {
    const message = conversation.messages[index];
    if (message !== undefined && message.role === "assistant") {
      return message;
    }
  }
  return null;
}
