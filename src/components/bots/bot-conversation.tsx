/**
 * The message list (Epic 61, Story 61.4).
 *
 * **Windowed from the first line, and that is a correction rather than a
 * flourish.** The Matrix timeline never got virtualisation — it mounts every
 * row in a plain `<ol>` (`conversation-pane.tsx`) — while `useWindowedRows`
 * exists and is used by the note list, the Files tree, the recordings browser
 * and two viewers. A model conversation is exactly the shape that punishes the
 * omission: one answer can be 200 kB of prose, and a long-running conversation
 * is hundreds of them. Building it unwindowed would repeat a known cost in the
 * one surface where the rows are largest.
 *
 * Rows are **measured, not assumed**: an answer's height depends on its length,
 * so the constant below is only the estimate until a row has mounted once, and
 * measurements are keyed by message id so a re-read carries each row's geometry
 * with the row.
 *
 * jsdom lays nothing out, so a measurement of zero means "this environment did
 * not lay anything out", the estimate stands, and the list behaves as a
 * fixed-height window — which is what lets a component test count rows without
 * `withListGeometry`.
 *
 * # Following the other device (Epic 63, Story 63.7, AD-177)
 *
 * A conversation whose transcript is the gateway's may be growing on the
 * owner's other device. Hermes cannot stream that to a second reader — the
 * run's event queue is destroyed by one — so this device **follows**: it
 * re-reads the history at the interval Rust decides and sees each step as it
 * lands. The user's question at once, a tool call as it starts, the answer
 * when it completes. Never a token. {@link useBotFollow} owns the timer, and
 * lives here so the following lasts exactly as long as the transcript is on
 * screen: the pane unmounts on every surface switch, and a timer that
 * outlived it would be a phone's radio spent on a conversation nobody is
 * looking at.
 *
 * While a turn is open over there, one caption says what is happening — and
 * says it is not streaming. It is never shown under an answer arriving here.
 */
import { useCallback, useEffect } from "react";
import { BotMessage } from "@/components/bots/bot-message";
import { useWindowedRows } from "@/components/ui/window-list";
import { type BotMessageVm, botsSessionFollow } from "@/lib/ipc/client";
import { botsStore, useBotsStore } from "@/lib/stores/bots";

/** The accessible name of the list. */
export const BOT_CONVERSATION_LABEL = "Conversation";

/**
 * What the transcript says while a turn is open on the other device. One
 * sentence, and it names the difference from streaming: the step, not the
 * token, is the unit that arrives.
 */
export const BOT_FOLLOWING_CAPTION =
  "Answering on your other device — each step lands here when it completes, not as it is typed.";

/**
 * The height one row is assumed to be before it has been mounted.
 *
 * A short answer: two lines of body plus the row's own padding. Deliberately an
 * underestimate rather than an average — too small mounts a few rows more than
 * needed on first paint, too large leaves a gap under the last one.
 */
const BOT_ROW_HEIGHT = 72;

/** Space between rows, folded into each row's box — flex `gap` does not apply
 *  to absolutely positioned children. */
const BOT_ROW_GAP = 8;

/**
 * Read the open conversation's transcript at the cadence Rust decides, while
 * it is the gateway's and no answer is streaming here.
 *
 * One read at once — so a conversation opened mid-turn shows the caption
 * before its first interval, not after — then one per `nextPollMs` until Rust
 * answers `null`, the read fails, the conversation changes, an answer starts
 * streaming here, or the list unmounts. A failed read stops the following
 * rather than retrying: a gateway that stopped answering is not one to keep
 * asking every two seconds from a phone, and reopening reads afresh.
 */
export function useBotFollow(): void {
  const sessionId = useBotsStore((s) => s.conversation?.session.id ?? null);
  const remote = useBotsStore((s) => s.conversation?.transcript === "remote");
  const streaming = useBotsStore((s) => s.streamingId !== null);
  useEffect(() => {
    if (sessionId === null || !remote || streaming) {
      return;
    }
    let cancelled = false;
    let timer: number | null = null;
    const read = () => {
      void botsSessionFollow(sessionId)
        .then((vm) => {
          if (cancelled) {
            return;
          }
          const taken = botsStore.getState().applyFollow(vm);
          if (taken && vm.nextPollMs !== null) {
            timer = window.setTimeout(read, vm.nextPollMs);
          }
        })
        .catch(() => {
          if (!cancelled) {
            botsStore.getState().stopFollowing();
          }
        });
    };
    read();
    return () => {
      cancelled = true;
      window.clearTimeout(timer ?? undefined);
      botsStore.getState().stopFollowing();
    };
  }, [sessionId, remote, streaming]);
}

export function BotConversation({
  messages,
  streamingMessageId,
  retryableId,
  onRetry,
}: {
  messages: BotMessageVm[];
  /** The row an answer is currently arriving into, or `null`. */
  streamingMessageId: string | null;
  /** The one answer Retry belongs on, or `null`. */
  retryableId: string | null;
  onRetry: (messageId: string) => void;
}) {
  const getKey = useCallback((index: number) => messages[index]?.id ?? String(index), [messages]);
  const list = useWindowedRows({
    count: messages.length,
    getKey,
    rowHeight: BOT_ROW_HEIGHT,
    gap: BOT_ROW_GAP,
  });
  useBotFollow();
  // The caption is a projection of the last read and of the stream: a turn
  // open over there, and none arriving here.
  const followingLive = useBotsStore((s) => s.follow?.live === true);
  const following = followingLive && streamingMessageId === null;

  // Follow the tail while an answer is arriving — here, or on the other
  // device — and only then: a reader who has scrolled up to re-read an earlier
  // answer is not yanked back once the stream closes. In an effect rather
  // than during render, because `reveal` forces an index into the next render.
  //
  // `reveal` is a `useCallback` with no dependencies, so naming it here is a
  // real dependency rather than a suppressed one — the whole `list` object is
  // rebuilt every render and depending on that would scroll on every keystroke
  // elsewhere in the pane.
  const { reveal } = list;
  const count = messages.length;
  useEffect(() => {
    if ((streamingMessageId !== null || following) && count > 0) {
      reveal(count - 1);
    }
  }, [streamingMessageId, following, count, reveal]);

  return (
    <div {...list.viewportProps} className="min-h-0 flex-1 overflow-y-auto px-6 py-3">
      <ol
        aria-label={BOT_CONVERSATION_LABEL}
        className="relative mx-auto w-full max-w-[760px]"
        style={{ height: `${list.totalSize}px` }}
      >
        {list.rows.map((row) => {
          const message = messages[row.index];
          if (message === undefined) {
            return null;
          }
          return (
            <div key={row.key} {...list.rowProps(row)}>
              <BotMessage
                message={message}
                streaming={message.id === streamingMessageId}
                onRetry={message.id === retryableId ? () => onRetry(message.id) : null}
              />
            </div>
          );
        })}
      </ol>
      {/* `role="status"`, the streaming caption's own register: a fact about
          how the transcript is growing, not an alert. After the list, where
          the growth is and where a follower is looking. */}
      {following && (
        <p
          role="status"
          className="mx-auto w-full max-w-[760px] pt-2 text-muted-foreground text-xs"
        >
          {BOT_FOLLOWING_CAPTION}
        </p>
      )}
    </div>
  );
}
