/**
 * A thread a person started from this conversation with an `@mention` (109.24,
 * Q109-21 a), drawn inline where it was started: the agent it brought in, the
 * person's line, the agent's latest turn as Rust folds it (109.13's `items`),
 * and "Open thread". A line the server refused shows Rust's sentence instead,
 * and opens nothing — that thread never existed. An archived thread is one
 * line with "Open thread". The line and the answer are Rust's, cut to a bound
 * (`truncated`), and the answer's box is height-capped: whenever either cut
 * or the cap hides something, the block says the rest is in the thread.
 * Threads are told apart by `threadId` (their first line), never by the
 * session id a refused child shares with the next one.
 *
 * Its own component, hosted by 109.14's session view for each `child` item of
 * the reading column. The inline fold is its summary line alone (R10924K-13,
 * -14): it never opens, in any view mode, so the answer stays first in the
 * capped box; the steps are the thread's, behind "Open thread". Nothing here
 * is a live region (R10924K-15): the thread announces its work in its own
 * view; here each line names whose it is.
 */
import { useLayoutEffect, useRef, useState } from "react";
import { AgentAnswer } from "@/components/agents/agent-answer";
import { MINE_ROW, PersonLine } from "@/components/agents/person-line";
import { Button } from "@/components/ui/button";
import { formatMessageTime } from "@/lib/format-time";
import type {
  AgentChildSessionVm,
  AgentMessageVm,
  AgentSessionRef,
  AgentTimelineItemVm,
} from "@/lib/ipc/client";

/** What the block says when its line or answer is not shown whole. */
export const CHILD_CONTINUES = "This continues in the thread.";

export function SessionChildThread({
  database,
  child,
  onOpen,
}: {
  database: string;
  child: AgentChildSessionVm;
  onOpen: (target: AgentSessionRef) => void;
}) {
  const { prompt, refusal } = child;
  const byId = new Map(child.messages.map((message) => [message.id, message]));
  const folds = new Map(child.folds.map((fold) => [fold.turnId, fold]));
  const live = new Map(child.live.map((turn) => [turn.turnId, turn]));
  // R10924K-06: the height cap hides the end of an answer Rust did not cut;
  // measured after layout, and again whenever the box resizes.
  const latest = useRef<HTMLOListElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: a new child is new content to measure
  useLayoutEffect(() => {
    const node = latest.current;
    if (node === null) {
      setOverflowing(false);
      return;
    }
    const measure = () => setOverflowing(node.scrollHeight > node.clientHeight + 1);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    return () => observer.disconnect();
  }, [child]);
  return (
    <section
      aria-label={`Thread with ${child.agentName}`}
      data-slot="agent-child-thread"
      className="min-w-0 rounded-md border border-border bg-muted/40 px-3 py-2"
    >
      <header className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs">
        <span
          aria-hidden="true"
          className="flex size-6 shrink-0 items-center justify-center rounded-full bg-primary/15 font-medium text-primary"
        >
          {child.agentMark}
        </span>
        <span className="min-w-0 break-words font-medium">{child.agentName}</span>
        <span className="text-muted-foreground">{formatMessageTime(child.createdAtMs)}</span>
        {child.archived && <span className="text-muted-foreground">Thread archived</span>}
        {refusal == null && (
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto min-h-11 sm:min-h-8"
            aria-label={`Open thread with ${child.agentName}`}
            onClick={() => onOpen({ database, sessionId: child.sessionId })}
          >
            Open thread
          </Button>
        )}
      </header>
      {prompt && (
        <div className={prompt.mine ? `mt-1 min-w-0 ${MINE_ROW}` : "mt-1 min-w-0"}>
          <PersonLine body={prompt.body} mine={prompt.mine} />
          {prompt.state && refusal == null && (
            <span className="text-muted-foreground text-xs">{prompt.state}</span>
          )}
        </div>
      )}
      {refusal != null && (
        <p role="alert" className="mt-1 text-destructive text-xs">
          {refusal}
        </p>
      )}
      {child.items.length > 0 && (
        <ol
          ref={latest}
          aria-label={`Latest from ${child.agentName}`}
          className="mt-2 flex max-h-80 min-w-0 flex-col gap-2 overflow-hidden border-border border-l-2 pl-3"
        >
          {child.items.map((item) => (
            <ChildItem
              key={itemKey(item)}
              item={item}
              agentName={child.agentName}
              byId={byId}
              folds={folds}
              live={live}
            />
          ))}
        </ol>
      )}
      {refusal == null && (child.truncated || overflowing) && (
        <p className="mt-1 text-muted-foreground text-xs">{CHILD_CONTINUES}</p>
      )}
    </section>
  );
}

function itemKey(item: AgentTimelineItemVm): string {
  switch (item.kind) {
    case "message":
      return `m:${item.messageId}`;
    case "fold":
      return `f:${item.turnId}`;
    case "liveAnswer":
      return `l:${item.turnId}`;
    case "child":
      return `c:${item.threadId}`;
  }
}

function ChildItem({
  item,
  agentName,
  byId,
  folds,
  live,
}: {
  item: AgentTimelineItemVm;
  agentName: string;
  byId: Map<string, AgentMessageVm>;
  folds: Map<string, AgentChildSessionVm["folds"][number]>;
  live: Map<string, AgentChildSessionVm["live"][number]>;
}) {
  switch (item.kind) {
    case "fold": {
      const fold = folds.get(item.turnId);
      // Rust's summary sentence, and nothing to open: the steps are the thread's.
      return fold ? (
        <li
          data-slot="inline-fold"
          data-status={fold.status}
          className="min-w-0 text-muted-foreground text-xs [overflow-wrap:anywhere]"
        >
          <span className="sr-only">{agentName}: </span>
          {fold.summary}
        </li>
      ) : null;
    }
    case "message": {
      const message = byId.get(item.messageId);
      if (!message) return null;
      return (
        <li className={message.mine ? `min-w-0 ${MINE_ROW}` : "min-w-0"}>
          <div className="mb-1 flex flex-wrap items-baseline gap-x-2 text-xs">
            <span className="font-medium">{message.authorName}</span>
            <span className="text-muted-foreground">{formatMessageTime(message.createdAtMs)}</span>
          </div>
          {message.role === "agent" ? (
            <AgentAnswer body={message.body} />
          ) : (
            <PersonLine body={message.body} mine={message.mine} />
          )}
        </li>
      );
    }
    case "liveAnswer": {
      const turn = live.get(item.turnId);
      // Drawn, not announced: the thread's own view is the live region.
      return turn ? (
        <li className="min-w-0">
          <div className="mb-1 flex flex-wrap items-baseline gap-x-2 text-xs">
            <span className="font-medium">{agentName}</span>
            <span className="text-muted-foreground">
              {turn.stopped ? "The answer stopped." : "Answering…"}
            </span>
          </div>
          <AgentAnswer body={turn.text} />
        </li>
      ) : null;
    }
    case "child":
      return null;
  }
}
