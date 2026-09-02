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
 */
import { useCallback, useEffect } from "react";
import { BotMessage } from "@/components/bots/bot-message";
import { useWindowedRows } from "@/components/ui/window-list";
import type { BotMessageVm } from "@/lib/ipc/client";

/** The accessible name of the list. */
export const BOT_CONVERSATION_LABEL = "Conversation";

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

  // Follow the tail while an answer is arriving, and only then: a reader who
  // has scrolled up to re-read an earlier answer is not yanked back once the
  // stream closes. In an effect rather than during render, because `reveal`
  // forces an index into the next render.
  //
  // `reveal` is a `useCallback` with no dependencies, so naming it here is a
  // real dependency rather than a suppressed one — the whole `list` object is
  // rebuilt every render and depending on that would scroll on every keystroke
  // elsewhere in the pane.
  const { reveal } = list;
  const count = messages.length;
  useEffect(() => {
    if (streamingMessageId !== null && count > 0) {
      reveal(count - 1);
    }
  }, [streamingMessageId, count, reveal]);

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
    </div>
  );
}
