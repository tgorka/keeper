/**
 * One row of a conversation (Epic 61, Story 61.4).
 *
 * It composes and decides nothing else: {@link BotAnswer} for the body (Story
 * 61.5 replaces it), {@link BotMessageMeta} for the caption (61.8), and
 * {@link BotToolCall} for the tool line (61.11's UI). Splitting them this way
 * is what lets three later stories each own one file instead of three stories
 * editing this one.
 *
 * **The partial caption is this story's, and it is the honest half of the
 * streaming contract.** A row marked `partial` is a row Rust never closed
 * cleanly — a stopped answer, a dead socket, a process that died mid-write —
 * and it renders with what arrived plus a sentence saying so. It is never
 * hidden and never silently retried: a truncated answer somebody can read is a
 * record, and a discarded one is a surprise.
 *
 * A row that is *currently arriving* is also `partial`, and the two must not
 * read alike — so `streaming` is a separate prop rather than derived from the
 * flag. The store holds the subscription id for exactly this reason.
 */
import { BotAnswer } from "@/components/bots/bot-answer";
import { BotReplyPaths } from "@/components/bots/bot-attachment";
import { BotContextNote } from "@/components/bots/bot-context-note";
import { BotMessageMeta } from "@/components/bots/bot-message-meta";
import { BotToolCall } from "@/components/bots/bot-tool-call";
import { Button } from "@/components/ui/button";
import type { BotContextBundleVm, BotMessageVm, BotToolCallVm } from "@/lib/ipc/client";
import { useBotsStore } from "@/lib/stores/bots";
import { cn } from "@/lib/utils";

/** One shared empty list, so a row with no calls does not re-render on
 *  identity every time its parent does. */
const EMPTY_CALLS: BotToolCallVm[] = [];

/**
 * What a stopped or broken answer says under itself.
 *
 * States what happened and what keeper did about it, in that order, and does
 * not apologise — the `"Queued — sends when you're back online"` shape.
 */
export const BOT_PARTIAL_CAPTION = "This answer stopped before it finished. What arrived is kept.";

/** What a row still arriving says, so the two states cannot read alike. */
export const BOT_STREAMING_CAPTION = "Answering…";

/** The Retry verb, on the last answer only. */
export const BOT_RETRY_LABEL = "Retry";

/** How an empty answer that spent its turn on tools reads, when even the tool
 *  count is zero — a model that genuinely said nothing. */
export const BOT_EMPTY_ANSWER_CAPTION = "The model returned no text.";

export function BotMessage({
  message,
  streaming,
  onRetry,
  toolCalls,
  context,
}: {
  message: BotMessageVm;
  /** Whether this row is the answer currently arriving. */
  streaming: boolean;
  /** Retry this answer, or `null` where the verb does not belong on it — a
   *  question, or an answer that is not the last one. */
  onRetry: (() => void) | null;
  /** The calls this turn ran. Absent, the row reads what the live turn
   *  reported into the store under this message's id; a replayed conversation
   *  has the stored `toolCallCount` instead (Story 61.11). */
  toolCalls?: BotToolCallVm[];
  /** What the model was told about the drive. Absent, the row reads the
   *  store; `null` there too means keeper does not know — which is not the
   *  same as none (Story 61.11, AD-27). */
  context?: BotContextBundleVm | null;
}) {
  const liveCalls = useBotsStore((s) => s.toolRows[message.id]);
  const liveContext = useBotsStore((s) => s.contexts[message.id]);
  const calls = toolCalls ?? liveCalls ?? EMPTY_CALLS;
  const bundle = context === undefined ? (liveContext ?? null) : context;
  const own = message.role === "user";
  // A row that never closed AND is not arriving now: the honest failure state.
  const stalled = message.partial && !streaming;
  const emptyAnswer =
    !own && message.content.length === 0 && message.toolCallCount === 0 && !streaming;
  return (
    <li
      className={cn(
        "flex min-w-0 flex-col gap-1 rounded-md px-3 py-2",
        own ? "bg-muted" : "bg-card",
      )}
      data-role={message.role}
      data-partial={message.partial ? "true" : "false"}
    >
      <BotAnswer body={message.content} />
      <BotToolCall count={message.toolCallCount} calls={calls} />
      <BotContextNote bundle={bundle} />
      {!own && (
        <BotReplyPaths sessionId={message.sessionId} body={message.content} streaming={streaming} />
      )}
      {emptyAnswer && <p className="text-muted-foreground text-xs">{BOT_EMPTY_ANSWER_CAPTION}</p>}
      {streaming && (
        <p role="status" className="text-muted-foreground text-xs">
          {BOT_STREAMING_CAPTION}
        </p>
      )}
      {/* `role="status"`, not `alert`: a truncated answer is a fact about a
          finished attempt, not a failure demanding an action. The failure that
          DOES demand one — a send that never reached the endpoint — lands in the
          pane's own alert region. */}
      {stalled && (
        <p role="status" className="text-muted-foreground text-xs">
          {BOT_PARTIAL_CAPTION}
        </p>
      )}
      <BotMessageMeta message={message} />
      {onRetry !== null && (
        <div className="flex">
          <Button type="button" variant="outline" size="sm" onClick={onRetry}>
            {BOT_RETRY_LABEL}
          </Button>
        </div>
      )}
    </li>
  );
}
