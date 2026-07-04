/**
 * Conversation pane: the read-only per-room timeline (FR-8/FR-9, AD-4/AD-8/AD-19).
 *
 * On `selectedRoomId` change it clears the timeline store (newest-mount-wins),
 * subscribes to the room's timeline channel, and mirrors the streamed ops into
 * the ordered {@link timelineStore} (never sorting). Rendered `Message` items
 * become grouped {@link MessageBubble}s inside a bottom-anchored scroll region
 * with a 720 px-max centered column; `Other` items are skipped (they exist only
 * to keep diff indices aligned). Cleanup — StrictMode double-mount, room change,
 * unmount — unsubscribes the backend task and clears the store, so timelines
 * never leak or stack. A failed subscribe surfaces an honest inline error
 * instead of a silent spinner (AD-21). A bottom {@link Composer} footer (720 px-
 * centered, `border-t`) sends via the single Rust dispatch gate — disabled until
 * a room's timeline is loaded — and outgoing bubbles carry a Rust-authoritative
 * send-state caption with a persistent `Failed — Retry` (FR-9, AD-13).
 */
import { PanelRight } from "lucide-react";
import { type Ref, useCallback, useEffect, useRef, useState } from "react";
import { Composer } from "@/components/chat/composer";
import { MessageBubble, type MessageVm } from "@/components/chat/message-bubble";
import { UtdStub } from "@/components/chat/utd-stub";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { TimelineBatch, TimelineItemVm } from "@/lib/ipc/client";
import { retrySend, sendText, subscribeTimeline, unsubscribeTimeline } from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { useRoomsStore } from "@/lib/stores/rooms";
import { timelineStore, useTimelineStore } from "@/lib/stores/timeline";

interface ConversationPaneProps {
  detailOpen: boolean;
  onToggleDetail: () => void;
  toggleRef?: Ref<HTMLButtonElement>;
}

/** The `utd`-variant of {@link TimelineItemVm} (rendered as an honest stub). */
type UtdVm = Extract<TimelineItemVm, { kind: "utd" }>;

/**
 * A renderable timeline row. A `message` row is a text bubble paired with whether
 * it continues a same-sender run (`grouped`) and whether it ends one (`groupTail`
 * — the transient send-state caption renders only on the tail). A `utd` row is an
 * undecryptable-event stub (never grouped); it breaks same-sender runs but is
 * emitted (not skipped like `other`), so it renders inline and never blank.
 */
type RenderedRow =
  | { kind: "message"; item: MessageVm; grouped: boolean; groupTail: boolean }
  | { kind: "utd"; item: UtdVm };

/**
 * Project the streamed timeline into the renderable row sequence, computing
 * grouping in a single pass: a `Message` is `grouped` when the immediately
 * preceding **rendered** message has the same sender, and is the run's
 * `groupTail` when the immediately following **rendered** message has a different
 * sender (or there is none). A `utd` item is emitted as its own row and breaks a
 * same-sender run (like `other`, but visible). `Other` items are skipped but also
 * break a run (an interleaved non-text item ungroups the next message and ends
 * the current run).
 */
function toRenderedRows(items: TimelineItemVm[]): RenderedRow[] {
  const rendered: RenderedRow[] = [];
  let prevSender: string | null = null;

  /** Mark the last rendered message (if any) as a group tail — a boundary. */
  const closeRun = () => {
    const last = rendered[rendered.length - 1];
    if (last?.kind === "message") {
      last.groupTail = true;
    }
    prevSender = null;
  };

  for (const item of items) {
    if (item.kind === "utd") {
      // A UTD stub breaks the run but is itself rendered (never blank).
      closeRun();
      rendered.push({ kind: "utd", item });
      continue;
    }
    if (item.kind !== "message") {
      // A non-rendered item breaks the same-sender run.
      closeRun();
      continue;
    }
    const last = rendered[rendered.length - 1];
    if (last?.kind === "message" && prevSender === item.sender) {
      // This message continues the run, so the previous one is not the tail.
      last.groupTail = false;
    }
    rendered.push({
      kind: "message",
      item,
      grouped: prevSender === item.sender,
      groupTail: true,
    });
    prevSender = item.sender;
  }
  return rendered;
}

export function ConversationPane({ detailOpen, onToggleDetail, toggleRef }: ConversationPaneProps) {
  const selected = useRoomsStore((s) => s.selected);
  const accountId = selected?.accountId ?? null;
  const selectedRoomId = selected?.roomId ?? null;
  const items = useTimelineStore((s) => s.items);
  // The open conversation's account status drives the "Queued" caption. An empty
  // key (no room open) reads as `undefined` → not offline.
  const offline = useAccountStatus(accountId ?? "") === "offline";
  const [errored, setErrored] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (accountId === null || selectedRoomId === null) {
      // No conversation open, or the account went away (e.g. sign-out): drop any
      // rendered timeline so a previous room's / account's messages never
      // linger, and reset the load/error state.
      timelineStore.getState().clear();
      setErrored(false);
      setLoaded(false);
      return;
    }

    setErrored(false);
    setLoaded(false);
    // Establish clean state at mount so the newest mount always wins; clearing
    // in cleanup instead would race the next room's mount.
    timelineStore.getState().clear();
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
    // batches never mutate the store).
    const onBatch = (b: TimelineBatch) => {
      if (!cancelled) {
        timelineStore.getState().applyBatch(b);
        setLoaded(true);
      }
    };
    subscribeTimeline(accountId, selectedRoomId, onBatch)
      .then((id) => {
        if (cancelled) {
          // Unmounted / room changed before the id resolved — tear down now.
          void unsubscribeTimeline(accountId, id);
          return;
        }
        subscriptionId = id;
      })
      .catch(() => {
        if (!cancelled) {
          setErrored(true);
        }
      });

    return () => {
      cancelled = true;
      if (subscriptionId !== null) {
        void unsubscribeTimeline(accountId, subscriptionId);
      }
      timelineStore.getState().clear();
    };
  }, [accountId, selectedRoomId]);

  // Bottom-anchor the scroll region: keep the newest message in view whenever
  // the streamed timeline changes (a `Reset` snapshot or a live diff). This is a
  // plain always-scroll-to-bottom — no auto-follow tuning / jump-to-bottom
  // (Epic 3 polish). Short lists rest at the bottom via the `mt-auto` content.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && items.length > 0) {
      el.scrollTop = el.scrollHeight;
    }
  }, [items]);

  const rows = toRenderedRows(items);
  const roomLoaded = accountId !== null && selectedRoomId !== null && loaded && !errored;

  const onSend = useCallback(
    async (body: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      await sendText(accountId, selectedRoomId, body);
    },
    [accountId, selectedRoomId],
  );

  const onRetry = useCallback(
    (key: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      // A failed retry (e.g. the echo reconciled away → `EchoNotFound`) leaves
      // the persistent `Failed — Retry` caption in place, inviting another
      // attempt; swallow the rejection so it is never an unhandled promise.
      retrySend(accountId, selectedRoomId, key).catch(() => {});
    },
    [accountId, selectedRoomId],
  );

  return (
    <main className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-end border-border border-b p-2">
        <Button
          ref={toggleRef}
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Toggle detail panel"
          aria-pressed={detailOpen}
          onClick={onToggleDetail}
          className="focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <PanelRight aria-hidden="true" />
        </Button>
      </div>
      {selectedRoomId === null ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <p className="max-w-sm text-center text-muted-foreground text-sm">
            Select a conversation to start reading.
          </p>
        </div>
      ) : errored ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <p className="max-w-sm text-center text-muted-foreground text-sm">
            Couldn't open this conversation. Check your connection and try again.
          </p>
        </div>
      ) : !loaded ? (
        <div
          role="status"
          aria-label="Loading messages"
          className="mx-auto flex w-full max-w-[720px] flex-1 flex-col justify-end gap-3 p-4"
        >
          {[0, 1, 2].map((i) => (
            <Skeleton key={i} className="h-10 w-1/2 rounded-[14px]" />
          ))}
        </div>
      ) : rows.length === 0 ? (
        <div className="flex flex-1 items-center justify-center p-8">
          <p className="max-w-sm text-center text-muted-foreground text-sm">No messages yet.</p>
        </div>
      ) : (
        <div ref={scrollRef} className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <ol
            aria-label="Messages"
            className="mx-auto mt-auto flex w-full max-w-[720px] flex-col px-4 py-4"
          >
            {rows.map((row) =>
              row.kind === "utd" ? (
                <li key={row.item.key}>
                  <UtdStub />
                </li>
              ) : (
                <li key={row.item.key}>
                  <MessageBubble
                    item={row.item}
                    grouped={row.grouped}
                    groupTail={row.groupTail}
                    onRetry={onRetry}
                    offline={offline}
                  />
                </li>
              ),
            )}
          </ol>
        </div>
      )}
      {selectedRoomId !== null && (
        <div className="shrink-0 border-border border-t">
          <div className="mx-auto w-full max-w-[720px] px-4 py-3">
            <Composer key={selectedRoomId} onSend={onSend} disabled={!roomLoaded} />
          </div>
        </div>
      )}
    </main>
  );
}
