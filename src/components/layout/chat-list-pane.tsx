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
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { ChatRow } from "@/components/chat/chat-row";
import { FavoritesSection, hydrateFavoritesCollapsed } from "@/components/layout/favorites-section";
import { PinsStrip } from "@/components/layout/pins-strip";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import type { InboxBatch, NetworksSnapshot, SpacesSnapshot } from "@/lib/ipc/client";
import {
  getFavoritesCollapsed,
  listDrafts,
  setNetworkFilter,
  setSpaceFilter,
  subscribeDraftMirror,
  subscribeInbox,
  unsubscribeDraftMirror,
  unsubscribeInbox,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore, useArchiveRoomsStore } from "@/lib/stores/archive-rooms";
import { draftsStore } from "@/lib/stores/drafts";
import { favoritesRoomsStore, useFavoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { networksStore, useNetworksStore } from "@/lib/stores/networks";
import { pinsRoomsStore, usePinsRoomsStore } from "@/lib/stores/pins-rooms";
import { usePrimaryView } from "@/lib/stores/primary-view";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { spacesStore, useSpacesStore } from "@/lib/stores/spaces";

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
  const pinsRooms = usePinsRoomsStore((s) => s.rooms);
  const favoritesRooms = useFavoritesRoomsStore((s) => s.rooms);
  const selected = useRoomsStore((s) => s.selected);
  const selectRoom = useRoomsStore((s) => s.selectRoom);
  // Account switcher filter (Story 2.5): a pure display filter over the already-
  // merged, Rust-ordered rooms — it hides non-matching rows without touching the
  // merged subscription or the sort. `null` shows every account.
  const filterAccountId = useAccountsStore((s) => s.filterAccountId);
  // The active Space filter (Story 4.5): a Rust-side inbox filter (unlike the
  // account switcher, which is a pure TS display filter). The selection is mirrored
  // here only to render the dismissible chip + empty state and to re-apply the
  // filter after an account-set re-subscribe. `null` means unfiltered.
  const activeSpace = useSpacesStore((s) => s.activeSpace);
  const activeSpaceName = useSpacesStore((s) =>
    s.activeSpace === null
      ? null
      : (s.spaces.find(
          (sp) =>
            sp.accountId === s.activeSpace?.accountId && sp.spaceId === s.activeSpace?.spaceId,
        )?.name ?? null),
  );
  // The active Network filter (Story 4.6): another Rust-side inbox filter, keyed by
  // Network name (cross-account), composing AND with the Space filter. Mirrored here
  // only to render the dismissible chip + empty state and to re-apply the filter
  // after an account-set re-subscribe. `null` means unfiltered.
  const activeNetwork = useNetworksStore((s) => s.activeNetwork);
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
      // No signed-in accounts: clear the Space and Network lists *and* their
      // selections (a full sign-out has nothing to filter to).
      spacesStore.getState().clear();
      networksStore.getState().clear();
      return;
    }

    setErrored(false);
    setLoadedInbox(false);
    setLoadedArchive(false);
    // Establish clean state at mount so the newest mount always wins; clearing
    // in cleanup instead would race the next mount.
    roomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
    pinsRoomsStore.getState().clear();
    favoritesRoomsStore.getState().clear();
    // The Space list is replaced wholesale by each snapshot, so reset only the
    // list here; the active Space *selection* is preserved across an account-set
    // re-subscribe so the Rust filter can be re-applied below (survive resubscribe).
    spacesStore.getState().applySnapshot({ spaces: [] });
    // Same for the Network list (Story 4.6): reset only the list; the active Network
    // *selection* is preserved across an account-set re-subscribe so the Rust filter
    // can be re-applied below.
    networksStore.getState().applySnapshot({ networks: [] });
    // Capture the carried-over selections (from before this run) so we can re-apply
    // the Rust-side filters once the new subscription is live.
    const carriedSpace = spacesStore.getState().activeSpace;
    const carriedNetwork = networksStore.getState().activeNetwork;
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate all sinks so they no-op after cleanup (post-unmount / StrictMode late
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
    const onPins = (b: InboxBatch) => {
      if (!cancelled) {
        pinsRoomsStore.getState().applyBatch(b);
      }
    };
    const onFavourites = (b: InboxBatch) => {
      if (!cancelled) {
        favoritesRoomsStore.getState().applyBatch(b);
      }
    };
    const onSpaces = (snapshot: SpacesSnapshot) => {
      if (!cancelled) {
        spacesStore.getState().applySnapshot(snapshot);
        // Reconcile a stale selection: if the active Space is no longer in the
        // streamed list (its owner account signed out, or the user left the
        // Space), clear the selection and the Rust filter — otherwise the merger
        // stays filtered on a `(account, space)` with no members and empties every
        // window indefinitely while the chip shows a now-nameless Space.
        const sel = spacesStore.getState().activeSpace;
        if (
          sel !== null &&
          !snapshot.spaces.some((s) => s.accountId === sel.accountId && s.spaceId === sel.spaceId)
        ) {
          spacesStore.getState().setActiveSpace(null);
          void setSpaceFilter(null, null).catch(() => {});
        }
      }
    };
    const onNetworks = (snapshot: NetworksSnapshot) => {
      if (!cancelled) {
        networksStore.getState().applySnapshot(snapshot);
        // Reconcile a stale selection (Story 4.6): if the active Network is no longer
        // in the streamed list (its last bridged room left, or an owner account
        // signed out), clear the selection and the Rust filter — otherwise the merger
        // stays filtered on a Network with no rooms and empties every window
        // indefinitely while the chip shows a now-absent Network.
        const sel = networksStore.getState().activeNetwork;
        if (sel !== null && !snapshot.networks.some((n) => n.name === sel)) {
          networksStore.getState().setActiveNetwork(null);
          void setNetworkFilter(null).catch(() => {});
        }
      }
    };
    subscribeInbox(onInbox, onArchive, onPins, onFavourites, onSpaces, onNetworks)
      .then((id) => {
        if (cancelled) {
          // Unmounted before the id resolved — tear down immediately.
          void unsubscribeInbox(id);
          return;
        }
        subscriptionId = id;
        // Re-apply the ephemeral Space filter after a (re)subscribe so it survives
        // an account-set re-subscribe (Story 4.5). The Rust merger starts each new
        // subscription unfiltered, so a carried-over selection must be re-poked.
        if (carriedSpace !== null) {
          void setSpaceFilter(carriedSpace.accountId, carriedSpace.spaceId).catch(() => {});
        }
        // Re-apply the ephemeral Network filter too (Story 4.6): the Rust merger
        // starts each new subscription unfiltered, so a carried-over selection must
        // be re-poked (survive resubscribe, compose AND with the Space filter).
        if (carriedNetwork !== null) {
          void setNetworkFilter(carriedNetwork).catch(() => {});
        }
      })
      .catch(() => {
        if (!cancelled) {
          setErrored(true);
        }
      });

    return () => {
      cancelled = true;
      // Clear the mirrored Space + Network lists on unsubscribe; the selections are
      // kept so a re-subscribe can re-apply the filters (cleared fully only on full
      // sign-out via the stores' own `clear`, when there are no accounts).
      spacesStore.getState().applySnapshot({ spaces: [] });
      networksStore.getState().applySnapshot({ networks: [] });
      if (subscriptionId !== null) {
        void unsubscribeInbox(subscriptionId);
      }
    };
  }, [accountKey]);

  // Hydrate the Favorites section's collapse chrome from the persisted registry
  // setting once on mount (Story 4.4). Idempotent; a read failure keeps the
  // expanded default.
  useEffect(() => {
    void hydrateFavoritesCollapsed(getFavoritesCollapsed);
  }, []);

  // Seed the inbox draft markers from `keeper.db` (Story 7.1, AD-15) so a chat with a
  // pending draft shows its amber pencil after relaunch, cross-account. Re-seeded
  // whenever the signed-in account set changes (an add/sign-out) so the markers cover
  // exactly the live accounts. A read failure is swallowed — no markers, no crash.
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-seed keyed on the account-id set so an add/sign-out refreshes markers; `accountKey` is intentionally the trigger, not read in the body.
  useEffect(() => {
    let cancelled = false;
    void listDrafts()
      .then((keys) => {
        if (!cancelled) {
          draftsStore.getState().applyKeys(keys);
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [accountKey]);

  // Start the single app-lifetime remote draft-mirror subscription (Story 7.2, AD-15):
  // pump each observed `dev.keeper.draft` edit into the drafts store's `remote` map so
  // an open composer can offer local-wins adoption. Mounted once (not keyed on the
  // account set — the backend relay spans every account); torn down on unmount. A
  // subscribe failure is swallowed — no live remote stream, no crash; the next chat
  // open still reconciles via `loadRemoteDraft`.
  useEffect(() => {
    let cancelled = false;
    let subId: number | null = null;
    void subscribeDraftMirror((batch) => {
      draftsStore
        .getState()
        .applyRemote(batch.accountId, batch.roomId, batch.body, batch.updatedTs);
    })
      .then((id) => {
        if (cancelled) {
          void unsubscribeDraftMirror(id).catch(() => {});
        } else {
          subId = id;
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      if (subId !== null) {
        void unsubscribeDraftMirror(subId).catch(() => {});
      }
    };
  }, []);

  // Pick the active window's rows, then apply the account switcher filter as a
  // pure display filter (no re-sort, no mutation): when a filter is active, hide
  // rows not owned by that account.
  const activeRooms = view === "archive" ? archiveRooms : inboxRooms;
  const activeLoaded = view === "archive" ? loadedArchive : loadedInbox;
  const visibleRooms =
    filterAccountId === null
      ? activeRooms
      : activeRooms.filter((room) => room.accountId === filterAccountId);
  // The Pins strip renders only atop the Inbox view; the account switcher filter
  // applies to it too (same pure display filter, no re-sort). Its order is
  // Rust-authoritative.
  const visiblePins =
    filterAccountId === null
      ? pinsRooms
      : pinsRooms.filter((room) => room.accountId === filterAccountId);
  const showPins = view === "inbox" && visiblePins.length > 0;
  // The Favorites section renders only atop the Inbox view; the account switcher
  // filter applies to it too (same pure display filter, no re-sort). Its recency
  // order is Rust-authoritative.
  const visibleFavorites =
    filterAccountId === null
      ? favoritesRooms
      : favoritesRooms.filter((room) => room.accountId === filterAccountId);
  const showFavorites = view === "inbox" && visibleFavorites.length > 0;
  // Clear ONLY the Space filter (the Space chip's ✕): drop the Space selection and
  // its Rust filter, leaving any active Network filter intact (Story 4.5 + 4.6).
  const clearSpaceFilter = () => {
    spacesStore.getState().setActiveSpace(null);
    void setSpaceFilter(null, null).catch(() => {});
  };
  // Clear ONLY the Network filter (the Network chip's ✕): drop the Network selection
  // and its Rust filter, leaving any active Space filter intact (Story 4.5 + 4.6).
  const clearNetworkFilter = () => {
    networksStore.getState().setActiveNetwork(null);
    void setNetworkFilter(null).catch(() => {});
  };
  // Clear ALL active filters (Esc / empty-state "Clear filter" button): drop both the
  // Space and Network selections and clear both Rust filters so the full inbox is
  // restored (Story 4.5 + 4.6). Each chip's ✕ clears only its own dimension
  // (`clearSpaceFilter`/`clearNetworkFilter`); Esc and the empty-state button clear all.
  const clearFilters = () => {
    clearSpaceFilter();
    clearNetworkFilter();
  };

  // Per-view empty state (UX-DR13): the Archive uses sentence case with a code-font
  // `E` and no exclamation; the Inbox keeps its existing copy. When any filter is
  // active and the view is empty, show "No chats in {filter names}." (Space · Network
  // joined by " · " under AND composition) with a Clear action instead (UX-DR13,
  // Story 4.5 + 4.6).
  const spaceFilterActive = activeSpace !== null;
  const networkFilterActive = activeNetwork !== null;
  const anyFilterActive = spaceFilterActive || networkFilterActive;
  const activeFilterLabels: string[] = [];
  if (spaceFilterActive) {
    activeFilterLabels.push(activeSpaceName ?? "Space");
  }
  if (networkFilterActive && activeNetwork !== null) {
    activeFilterLabels.push(activeNetwork);
  }
  const filterEmptyLabel = activeFilterLabels.join(" · ");
  const emptyState = anyFilterActive ? (
    <>
      No chats in {filterEmptyLabel}.{" "}
      <button
        type="button"
        onClick={clearFilters}
        className="text-foreground underline underline-offset-2 outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        Clear filter
      </button>
    </>
  ) : view === "archive" ? (
    <>
      Nothing archived. <code className="font-mono text-xs">E</code> archives a chat and keeps it
      searchable.
    </>
  ) : (
    "No conversations yet."
  );

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: container-level Esc handler clears all active filters before focus moves (UX-DR); rows stay independently keyboard-operable, so this is additive.
    <div
      className="flex h-full w-[320px] shrink-0 flex-col border-border border-r bg-background"
      onKeyDown={(e) => {
        // Esc from the list clears ALL active filters (Space + Network) before
        // moving focus (Story 4.5 + 4.6).
        if (e.key === "Escape" && anyFilterActive) {
          e.preventDefault();
          clearFilters();
        }
      }}
    >
      {/* Dismissible filter chips (Story 4.5 + 4.6): shown above the list when a
          Space and/or Network filter is active (AND composition — both chips
          render). Each chip's ✕ clears ONLY its own dimension (the other filter
          stays active); Esc clears ALL active filters and restores the inbox. */}
      {anyFilterActive && (
        <div className="flex shrink-0 flex-wrap gap-1 border-border border-b px-3 py-2">
          {spaceFilterActive && (
            <span className="inline-flex items-center gap-1 rounded-full bg-accent px-2 py-0.5 text-accent-foreground text-xs">
              {activeSpaceName ?? "Space"}
              <button
                type="button"
                onClick={clearSpaceFilter}
                aria-label={`Clear ${activeSpaceName ?? "Space"} filter`}
                className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X aria-hidden="true" className="size-3" />
              </button>
            </span>
          )}
          {networkFilterActive && activeNetwork !== null && (
            <span className="inline-flex items-center gap-1 rounded-full bg-accent px-2 py-0.5 text-accent-foreground text-xs">
              {activeNetwork}
              <button
                type="button"
                onClick={clearNetworkFilter}
                aria-label={`Clear ${activeNetwork} filter`}
                className="rounded-full outline-none hover:bg-background/40 focus-visible:ring-2 focus-visible:ring-ring"
              >
                <X aria-hidden="true" className="size-3" />
              </button>
            </span>
          )}
        </div>
      )}
      {showPins && (
        <PinsStrip
          pins={visiblePins}
          onSelect={selectRoom}
          selected={selected}
          reorderable={filterAccountId === null}
        />
      )}
      {showFavorites && (
        <FavoritesSection favorites={visibleFavorites} onSelect={selectRoom} selected={selected} />
      )}
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
