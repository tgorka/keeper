/**
 * Chat-list pane: the merged unified inbox (FR-18, AD-8/9/20).
 *
 * Subscribes to the Rust-computed merged inbox channel on mount, mirrors the
 * streamed ops into the recency-ordered {@link roomsStore} (never sorting), and
 * renders 64 px rows inside a `ScrollArea`. Each row carries its owning account
 * and hue, so selecting one records `{ accountId, roomId }`. On effect cleanup —
 * including React 19 StrictMode's double-mount — it unsubscribes the backend
 * task and clears the store, so streams never leak or duplicate. A stream-start
 * failure surfaces an honest inline error rather than a silent spinner (AD-21).
 *
 * The inbox re-subscribes whenever the set of signed-in accounts changes (an
 * account added or signed out): the effect keys on the account-id set so the
 * merged window always covers exactly the live accounts.
 */
import { useEffect, useState } from "react";
import { ChatRow } from "@/components/chat/chat-row";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import type { InboxBatch } from "@/lib/ipc/client";
import { subscribeInbox, unsubscribeInbox } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore, useArchiveRoomsStore } from "@/lib/stores/archive-rooms";
import { usePrimaryView } from "@/lib/stores/primary-view";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";

export function ChatListPane() {
  // Key the subscription on the set of account ids: an add/sign-out re-subscribes
  // the merged inbox so it covers exactly the live accounts.
  const accountKey = useAccountsStore((s) =>
    s.accounts
      .map((a) => a.accountId)
      .sort()
      .join(","),
  );
  const view = usePrimaryView();
  const inboxRooms = useRoomsStore((s) => s.rooms);
  const archiveRooms = useArchiveRoomsStore((s) => s.rooms);
  const selected = useRoomsStore((s) => s.selected);
  const selectRoom = useRoomsStore((s) => s.selectRoom);
  // Account switcher filter (Story 2.5): a pure display filter over the already-
  // merged, Rust-ordered rooms — it hides non-matching rows without touching the
  // merged subscription or the sort. `null` shows every account.
  const filterAccountId = useAccountsStore((s) => s.filterAccountId);
  const [errored, setErrored] = useState(false);
  // Track skeleton-dismissal per window: the Inbox and Archive stream on
  // independent channels, so gating one view's skeleton on the *other* view's
  // arrival would flash a premature empty-state (e.g. "Nothing archived." before
  // the archive window has actually loaded). Each flag flips only when its own
  // window delivers a batch.
  const [loadedInbox, setLoadedInbox] = useState(false);
  const [loadedArchive, setLoadedArchive] = useState(false);

  useEffect(() => {
    if (accountKey.length === 0) {
      return;
    }

    setErrored(false);
    setLoadedInbox(false);
    setLoadedArchive(false);
    // Establish clean state at mount so the newest mount always wins; clearing
    // in cleanup instead would race the next mount.
    roomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate both sinks so they no-op after cleanup (post-unmount / StrictMode late
    // batches never mutate the stores). Each window marks itself loaded when its
    // own channel first delivers.
    const onInbox = (b: InboxBatch) => {
      if (!cancelled) {
        roomsStore.getState().applyBatch(b);
        setLoadedInbox(true);
      }
    };
    const onArchive = (b: InboxBatch) => {
      if (!cancelled) {
        archiveRoomsStore.getState().applyBatch(b);
        setLoadedArchive(true);
      }
    };
    subscribeInbox(onInbox, onArchive)
      .then((id) => {
        if (cancelled) {
          // Unmounted before the id resolved — tear down immediately.
          void unsubscribeInbox(id);
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
        void unsubscribeInbox(subscriptionId);
      }
    };
  }, [accountKey]);

  // Pick the active window's rows, then apply the account switcher filter as a
  // pure display filter (no re-sort, no mutation): when a filter is active, hide
  // rows not owned by that account.
  const activeRooms = view === "archive" ? archiveRooms : inboxRooms;
  const activeLoaded = view === "archive" ? loadedArchive : loadedInbox;
  const visibleRooms =
    filterAccountId === null
      ? activeRooms
      : activeRooms.filter((room) => room.accountId === filterAccountId);
  // Per-view empty state (UX-DR13): the Archive uses sentence case with a code-font
  // `E` and no exclamation; the Inbox keeps its existing copy.
  const emptyState =
    view === "archive" ? (
      <>
        Nothing archived. <code className="font-mono text-xs">E</code> archives a chat and keeps it
        searchable.
      </>
    ) : (
      "No conversations yet."
    );

  return (
    <div className="flex h-full w-[320px] shrink-0 flex-col border-border border-r bg-background">
      {errored ? (
        <div className="flex flex-1 items-center justify-center p-4">
          <p className="text-center text-muted-foreground text-sm">
            Couldn't start syncing. Check your connection and try again.
          </p>
        </div>
      ) : visibleRooms.length > 0 ? (
        <ScrollArea className="flex-1">
          <ul aria-label="Conversations" className="flex flex-col">
            {visibleRooms.map((room) => (
              <li key={`${room.accountId}:${room.roomId}`}>
                <ChatRow
                  room={room}
                  onSelect={selectRoom}
                  selected={
                    selected?.roomId === room.roomId && selected?.accountId === room.accountId
                  }
                />
              </li>
            ))}
          </ul>
        </ScrollArea>
      ) : !activeLoaded ? (
        <div role="status" aria-label="Loading conversations" className="flex flex-col gap-1 p-3">
          {[0, 1, 2, 3, 4].map((i) => (
            <div key={i} className="flex h-16 items-center gap-3">
              <Skeleton className="size-10 shrink-0 rounded-full" />
              <div className="flex min-w-0 flex-1 flex-col gap-2">
                <Skeleton className="h-4 w-3/4" />
                <Skeleton className="h-3 w-1/2" />
              </div>
            </div>
          ))}
        </div>
      ) : (
        <ul aria-label="Conversations" className="flex flex-1 items-center justify-center p-4">
          <li className="text-center text-muted-foreground text-sm">{emptyState}</li>
        </ul>
      )}
    </div>
  );
}
