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
import { useEffect, useRef, useState } from "react";
import { ChatRow } from "@/components/chat/chat-row";
import { FavoritesSection, hydrateFavoritesCollapsed } from "@/components/layout/favorites-section";
import { PinsStrip } from "@/components/layout/pins-strip";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { InboxBatch, InboxRoomVm, NetworksSnapshot, SpacesSnapshot } from "@/lib/ipc/client";
import {
  archiveRoom,
  chatNotifyModeSet,
  favoriteRoom,
  getFavoritesCollapsed,
  listDrafts,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  setNetworkFilter,
  setSpaceFilter,
  subscribeDraftMirror,
  subscribeInbox,
  unarchiveRoom,
  unfavoriteRoom,
  unpinRoom,
  unsubscribeDraftMirror,
  unsubscribeInbox,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore, useArchiveRoomsStore } from "@/lib/stores/archive-rooms";
import { useChatListFocusNonce } from "@/lib/stores/chat-list-focus";
import { composerStore } from "@/lib/stores/composer";
import { draftsStore } from "@/lib/stores/drafts";
import { favoritesRoomsStore, useFavoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { networksStore, useNetworksStore } from "@/lib/stores/networks";
import { pinsRoomsStore, usePinsRoomsStore } from "@/lib/stores/pins-rooms";
import { usePrimaryView } from "@/lib/stores/primary-view";
import { effectiveIsUnread, roomsStore, useRoomsStore } from "@/lib/stores/rooms";
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
  // Phone tier (Story 13.1): opening a Chat on the phone must not auto-focus the
  // composer (UX-DR22) — the row-open Enter handler gates its focus request on this.
  const { phone } = useShellLayout();
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
  // Roving keyboard focus over the main list (Story 9.2): the stable
  // `${accountId}:${roomId}` key of the row that carries the visible focus ring +
  // `tabIndex={0}`, or `null` when no row is keyboard-focused (the ring is cleared,
  // e.g. after Esc). Keyed by identity, NOT position, so that when the Rust stream
  // re-orders `visibleRooms` (a routine recency bump) or the focused row leaves the
  // window (its own `e`-archive), the cursor still points at the same room — or at
  // nothing if that room is gone — never at whatever row now sits at a stale index.
  // Pure UI cursor over the Rust-ordered list — never a source of truth, never
  // re-orders.
  const [focusedKey, setFocusedKey] = useState<string | null>(null);
  // Live refs to each rendered row button so the container handler can imperatively
  // move `.focus()` as the roving index changes. Rebuilt each render from the
  // current `visibleRooms` length.
  const rowRefs = useRef<(HTMLButtonElement | null)[]>([]);
  // The chat-list container element, so a focus request can fall back to focusing it
  // when the Inbox list is empty (no row to focus). See the focus-request effect below.
  const containerRef = useRef<HTMLDivElement | null>(null);
  // A focus request that landed on an empty/not-yet-loaded Inbox and is still pending:
  // the container was focused as a fallback and the request should complete (jump to the
  // first row) once the Inbox rows arrive — the cold-start raise path, where the hotkey
  // fires before the first inbox batch has streamed in (Story 9.4).
  const pendingFocusRef = useRef(false);
  // The global summon hotkey's focus-request nonce (Story 9.4): each bump asks this
  // pane to move keyboard focus to the first visible Inbox row (or the container when
  // empty), reusing the Story 9.2 roving-focus state without lifting it out.
  const focusNonce = useChatListFocusNonce();

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

  // Global summon-hotkey focus request (Story 9.4): when the hotkey raises the window
  // it bumps `focusNonce`; move the roving cursor to the first visible Inbox row and
  // focus it, or — when the Inbox list is empty (or another view is active) — fall back
  // to focusing the list container so keyboard focus still lands here (matrix: empty
  // inbox). The initial mount (nonce 0) is skipped so the list is not auto-focused on
  // load. Reuses the Story 9.2 roving state (`focusedKey`/`rowRefs`) — never re-orders.
  // biome-ignore lint/correctness/useExhaustiveDependencies: intentionally keyed on `focusNonce` only — each bump is one focus request; re-running on every `visibleRooms`/`view` change would steal focus spuriously.
  useEffect(() => {
    if (focusNonce === 0) {
      return;
    }
    if (view === "inbox" && visibleRooms.length > 0) {
      const first = visibleRooms[0];
      setFocusedKey(`${first.accountId}:${first.roomId}`);
      rowRefs.current[0]?.focus();
      pendingFocusRef.current = false;
      return;
    }
    // Empty/not-yet-loaded Inbox (or a non-inbox view): focus the container so keyboard
    // focus still lands here. When the target is the Inbox, remember the request so it
    // completes once rows stream in (cold-start raise); the completion effect below
    // only acts while focus is still parked on the container.
    pendingFocusRef.current = view === "inbox";
    containerRef.current?.focus();
  }, [focusNonce]);

  // Complete a pending cold-start focus request (Story 9.4): once the Inbox rows arrive
  // after the container fallback, move focus to the first row — but only while focus is
  // still on the container, so a request is never allowed to steal focus the user has
  // since moved elsewhere. Runs on Inbox-list changes; the pending flag makes it a
  // cheap no-op in the common case.
  useEffect(() => {
    if (!pendingFocusRef.current) {
      return;
    }
    if (view !== "inbox" || visibleRooms.length === 0) {
      return;
    }
    if (document.activeElement !== containerRef.current) {
      // Focus already moved on (user or another surface) — abandon the request.
      pendingFocusRef.current = false;
      return;
    }
    const first = visibleRooms[0];
    setFocusedKey(`${first.accountId}:${first.roomId}`);
    rowRefs.current[0]?.focus();
    pendingFocusRef.current = false;
  }, [visibleRooms, view]);

  // ── Chat-list keyboard navigation (Story 9.2) ──────────────────────────────
  // Bare-key list verbs on the focused row, mirroring the `ChatRow` context menu:
  // the command direction is chosen from the row's current flag, and `u` mirrors
  // the optimistic-unread pattern (`setOptimisticUnread` then round-trip; revert on
  // a hard reject). These reuse the shipped commands — nothing new is wired.
  const runVerb = (room: InboxRoomVm, verb: "e" | "u" | "p" | "f" | "m") => {
    if (verb === "e") {
      const fn = room.isArchived ? unarchiveRoom : archiveRoom;
      void fn(room.accountId, room.roomId).catch(() => {});
      return;
    }
    if (verb === "p") {
      const fn = room.isPinned ? unpinRoom : pinRoom;
      void fn(room.accountId, room.roomId).catch(() => {});
      return;
    }
    if (verb === "f") {
      const fn = room.isFavourite ? unfavoriteRoom : favoriteRoom;
      void fn(room.accountId, room.roomId).catch(() => {});
      return;
    }
    if (verb === "m") {
      // Cycle the per-Chat notification mode: All → Mentions only → Mute → All.
      // Rust owns the synced rule + the row glyph (no optimistic overlay).
      const next =
        room.muteState === "none"
          ? "mention_only"
          : room.muteState === "mention_only"
            ? "mute"
            : "all";
      void chatNotifyModeSet(room.accountId, room.roomId, next).catch(() => {});
      return;
    }
    // `u`: toggle read/unread with the optimistic overlay, reverting on hard reject.
    const store = roomsStore.getState();
    const intendedRead = effectiveIsUnread(room, store.optimisticUnread);
    store.setOptimisticUnread(room.accountId, room.roomId, !intendedRead);
    const mark = intendedRead ? markRoomRead : markRoomUnread;
    void mark(room.accountId, room.roomId).catch(() =>
      roomsStore.getState().clearOptimisticUnread(room.accountId, room.roomId),
    );
  };

  // Resolve the roving cursor's key to a position in the CURRENT `visibleRooms`
  // each render: `-1` when nothing is keyboard-focused or the focused room has left
  // the window (re-ordered away / archived). The tab stop falls back to the first
  // row in that case, so the list never loses its single keyboard entry point.
  const resolvedFocusIdx =
    focusedKey === null
      ? -1
      : visibleRooms.findIndex((r) => `${r.accountId}:${r.roomId}` === focusedKey);

  // The chat-list container's keyboard handler. Fires the list-focused keys only
  // when focus is within the main conversations list (not the Pins strip, Favorites
  // section, or filter chips — all focusable siblings in this container) AND no
  // ⌘/⌥/⌃ modifier is held — so those chords fall through to the global hooks and
  // typing/other surfaces are never hijacked — driving the roving focus ring over
  // `visibleRooms` (Rust order; never re-sorted). Extends the existing Esc
  // filter-clearing handler with a second Esc clearing the focused-row ring.
  const onListKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    // Esc: clear any active filter first, else clear the focused-row ring. Handled
    // container-wide (independent of the main-list scope check below) so Esc from a
    // filter chip still clears the filter.
    if (e.key === "Escape") {
      if (anyFilterActive) {
        e.preventDefault();
        clearFilters();
      } else if (resolvedFocusIdx >= 0) {
        e.preventDefault();
        // Blur the still-focused row so its `focus-visible` ring actually clears
        // (dropping the cursor alone leaves DOM focus — and its ring — on the row).
        rowRefs.current[resolvedFocusIdx]?.blur();
        setFocusedKey(null);
      }
      return;
    }
    // Only the main conversations list owns the movement/verb keys: ignore keydowns
    // bubbling up from a Pins/Favorites/chip button so they keep their native
    // activation (the spec scopes those surfaces out of keyboard nav).
    const target = e.target as HTMLElement | null;
    if (target === null || target.closest('ul[aria-label="Conversations"]') === null) {
      return;
    }
    // Let ⌘/⌥/⌃ chords pass through to the global window hooks; only bare keys are
    // list-owned. (Shift alone is fine — it is not a chord modifier here.)
    if (e.metaKey || e.altKey || e.ctrlKey) {
      return;
    }
    if (visibleRooms.length === 0) {
      return;
    }
    const moveTo = (index: number) => {
      e.preventDefault();
      const row = visibleRooms[index];
      setFocusedKey(`${row.accountId}:${row.roomId}`);
      rowRefs.current[index]?.focus();
    };
    // ↑/↓ and j/k move the ring, clamping at the ends deterministically.
    if (e.key === "ArrowDown" || e.key === "j") {
      moveTo(Math.min(resolvedFocusIdx + 1, visibleRooms.length - 1));
      return;
    }
    if (e.key === "ArrowUp" || e.key === "k") {
      const base = resolvedFocusIdx < 0 ? visibleRooms.length : resolvedFocusIdx;
      moveTo(Math.max(base - 1, 0));
      return;
    }
    // The remaining keys act on the focused row (resolved by identity); no-op when
    // nothing is focused or the focused room has left the window.
    if (resolvedFocusIdx < 0) {
      return;
    }
    const room = visibleRooms[resolvedFocusIdx];
    if (room === undefined) {
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      selectRoom({ accountId: room.accountId, roomId: room.roomId });
      // Desktop keeps focus-on-open; the phone stack never steals composer focus
      // when a Chat opens (UX-DR22, Story 13.1).
      if (!phone) {
        composerStore.getState().requestFocus();
      }
      return;
    }
    if (e.key === "e" || e.key === "u" || e.key === "p" || e.key === "f" || e.key === "m") {
      e.preventDefault();
      runVerb(room, e.key);
    }
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
      ref={containerRef}
      // `tabIndex={-1}` makes the container programmatically focusable so the global
      // summon-hotkey fallback (empty inbox) can land focus here (Story 9.4). It is not
      // a tab stop for normal keyboard nav — the roving rows own that.
      tabIndex={-1}
      className="flex h-full w-[320px] shrink-0 flex-col border-border border-r bg-background outline-none"
      onKeyDown={onListKeyDown}
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
            {visibleRooms.map((room, index) => (
              <li key={`${room.accountId}:${room.roomId}`}>
                <ChatRow
                  ref={(el) => {
                    rowRefs.current[index] = el;
                  }}
                  room={room}
                  onSelect={selectRoom}
                  selected={
                    selected?.roomId === room.roomId && selected?.accountId === room.accountId
                  }
                  // Roving tabindex (Story 9.2): the keyboard-focused row is `0` so a
                  // single Tab lands on it; every other row is `-1`. Before any row is
                  // keyboard-focused — or when the focused row has left the window — the
                  // first row is the tab stop, so the list always has exactly one.
                  tabIndex={(resolvedFocusIdx >= 0 ? resolvedFocusIdx : 0) === index ? 0 : -1}
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
