/**
 * Chat-list pane: the streaming sliding-sync room list (FR-8, AD-8/9/19/20).
 *
 * Subscribes to the account's room-list channel on mount, mirrors the streamed
 * ops into the ordered {@link roomsStore} (never sorting), and renders 64 px
 * rows inside a `ScrollArea`. On effect cleanup — including React 19 StrictMode's
 * double-mount and any account change — it unsubscribes the backend task and
 * clears the store, so streams never leak or duplicate. Activation failure
 * surfaces an honest inline error rather than a silent spinner (AD-21).
 */
import { useEffect, useState } from "react";
import { ChatRow } from "@/components/chat/chat-row";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import type { RoomListBatch } from "@/lib/ipc/client";
import { subscribeRoomList, unsubscribeRoomList } from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";

export function ChatListPane() {
  const accountId = useAccountsStore((s) => s.currentAccount?.accountId ?? null);
  const rooms = useRoomsStore((s) => s.rooms);
  const selectedRoomId = useRoomsStore((s) => s.selectedRoomId);
  const selectRoom = useRoomsStore((s) => s.selectRoom);
  const [errored, setErrored] = useState(false);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    if (accountId === null) {
      return;
    }

    setErrored(false);
    setLoaded(false);
    // Establish clean state at mount so the newest mount always wins; clearing
    // in cleanup instead would race the next mount (cross-account bleed).
    roomsStore.getState().clear();
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate the sink so it no-ops after cleanup (post-unmount repopulation /
    // StrictMode late batches never mutate the store).
    const onBatch = (b: RoomListBatch) => {
      if (!cancelled) {
        roomsStore.getState().applyBatch(b);
        setLoaded(true);
      }
    };
    subscribeRoomList(accountId, onBatch)
      .then((id) => {
        if (cancelled) {
          // Unmounted before the id resolved — tear down immediately.
          void unsubscribeRoomList(accountId, id);
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
        void unsubscribeRoomList(accountId, subscriptionId);
      }
    };
  }, [accountId]);

  return (
    <div className="flex h-full w-[320px] shrink-0 flex-col border-border border-r bg-background">
      {errored ? (
        <div className="flex flex-1 items-center justify-center p-4">
          <p className="text-center text-muted-foreground text-sm">
            Couldn't start syncing. Check your connection and try again.
          </p>
        </div>
      ) : rooms.length > 0 ? (
        <ScrollArea className="flex-1">
          <ul aria-label="Conversations" className="flex flex-col">
            {rooms.map((room) => (
              <li key={room.roomId}>
                <ChatRow
                  room={room}
                  onSelect={selectRoom}
                  selected={room.roomId === selectedRoomId}
                />
              </li>
            ))}
          </ul>
        </ScrollArea>
      ) : !loaded ? (
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
          <li className="text-center text-muted-foreground text-sm">No conversations yet.</li>
        </ul>
      )}
    </div>
  );
}
