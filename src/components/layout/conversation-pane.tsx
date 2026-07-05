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
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Download, PanelRight } from "lucide-react";
import {
  type KeyboardEvent,
  type Ref,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { BridgeLoginSheet } from "@/components/bridges/bridge-login-sheet";
import { Composer } from "@/components/chat/composer";
import { DeleteMessageDialog } from "@/components/chat/delete-message-dialog";
import { HistoryBoundary, type HistoryBoundaryState } from "@/components/chat/history-boundary";
import { MediaPreviewOverlay } from "@/components/chat/media-preview-overlay";
import { MessageBubble, type MessageVm } from "@/components/chat/message-bubble";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import { RedactedStub } from "@/components/chat/redacted-stub";
import { TypingIndicator } from "@/components/chat/typing-indicator";
import { UtdStub } from "@/components/chat/utd-stub";
import { Alert, AlertAction, AlertDescription } from "@/components/ui/alert";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { useSelectedRoomVm } from "@/hooks/use-selected-room-vm";
import { accountHueVar } from "@/lib/account-hue";
import { initials } from "@/lib/account-initials";
import type {
  PaginationStatusBatch,
  TimelineBatch,
  TimelineItemVm,
  TypingBatch,
  TypistVm,
} from "@/lib/ipc/client";
import {
  cancelSend,
  editMessage,
  markRoomRead,
  paginateBackwards,
  resolveTimelineEventKey,
  retrySend,
  sendAttachmentBytes,
  sendAttachmentPath,
  sendReply,
  sendText,
  setTyping,
  subscribePaginationStatus,
  subscribeTimeline,
  subscribeTyping,
  toggleReaction,
  unsubscribePaginationStatus,
  unsubscribeTimeline,
  unsubscribeTyping,
} from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { useAccountsStore } from "@/lib/stores/accounts";
import { attachmentId, attachmentsStore, type PendingAttachment } from "@/lib/stores/attachments";
import { useBridgeHealth } from "@/lib/stores/bridge-health";
import { composerStore, useComposerStore } from "@/lib/stores/composer";
import { exportStore } from "@/lib/stores/export";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { timelineStore, useTimelineStore } from "@/lib/stores/timeline";

/** Trim a body to a short single-line preview for the reply banner/quote. */
function previewOf(body: string): string {
  const collapsed = body.replace(/\s+/g, " ").trim();
  return collapsed.length > 120 ? `${collapsed.slice(0, 120)}…` : collapsed;
}

/** Distance from the bottom (px) still counted as "near the bottom" for auto-scroll. */
const NEAR_BOTTOM_PX = 80;
/** Distance from the top (px) that triggers a back-pagination fetch. */
const NEAR_TOP_PX = 200;
/** Number of older events to request per back-pagination. */
const PAGINATE_BATCH = 40;
/** Debounce before re-marking the room read after new content arrives while open. */
const MARK_READ_DEBOUNCE_MS = 1000;

/**
 * Classify a streamed timeline batch so the scroll layout effect can tell an
 * older-history prepend (preserve the visual position) from a bottom-append (a
 * new message — never yank the view) from a wholesale reset (anchor to bottom).
 * Older history arrives as `pushFront`/`insert`-at-index-0; a `reset` replaces
 * the contents. A single `scrollHeight` delta cannot distinguish these, so we
 * read the ops directly.
 */
function classifyBatch(batch: TimelineBatch): "reset" | "prepend" | "other" {
  let prepend = false;
  for (const op of batch.ops) {
    if (op.op === "reset") {
      return "reset";
    }
    if (op.op === "pushFront" || (op.op === "insert" && op.index === 0)) {
      prepend = true;
    }
  }
  return prepend ? "prepend" : "other";
}

interface ConversationPaneProps {
  detailOpen: boolean;
  onToggleDetail: () => void;
  toggleRef?: Ref<HTMLButtonElement>;
}

/** The `utd`-variant of {@link TimelineItemVm} (rendered as an honest stub). */
type UtdVm = Extract<TimelineItemVm, { kind: "utd" }>;

/** The `redacted`-variant of {@link TimelineItemVm} (rendered as an honest stub). */
type RedactedVm = Extract<TimelineItemVm, { kind: "redacted" }>;

/**
 * A renderable timeline row. A `message` row is a text bubble paired with whether
 * it continues a same-sender run (`grouped`) and whether it ends one (`groupTail`
 * — the transient send-state caption renders only on the tail). A `utd` row is an
 * undecryptable-event stub and a `redacted` row is a deleted-message stub (Story
 * 3.8); both are never grouped, break same-sender runs, and are emitted (not
 * skipped like `other`), so they render inline and never blank.
 */
type RenderedRow =
  | { kind: "message"; item: MessageVm; grouped: boolean; groupTail: boolean }
  | { kind: "utd"; item: UtdVm }
  | { kind: "redacted"; item: RedactedVm };

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
    if (item.kind === "redacted") {
      // A redacted (deleted-for-everyone) stub breaks the run but is itself
      // rendered (never blank, never silently removed) (Story 3.8, FR-15).
      closeRun();
      rendered.push({ kind: "redacted", item });
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

/**
 * The hue-tinted account-initial chip for the conversation header (Story 4.6),
 * reusing the account-footer's `AccountAvatar` pattern. Shows the selected room's
 * owning account so a two-account inbox is disambiguated in the header, matching
 * the row's hue edge bar.
 */
function AccountInitialChip({ userId, hueIndex }: { userId: string; hueIndex: number }) {
  return (
    <Avatar size="sm" data-testid="account-initial-chip">
      <AvatarFallback
        style={{ backgroundColor: accountHueVar(hueIndex) }}
        className="font-medium text-white"
      >
        {initials(userId)}
      </AvatarFallback>
    </Avatar>
  );
}

/**
 * The conversation header's identity block (Story 4.6, FR-24): the selected room's
 * {@link RoomAvatar} (the Network badge comes free) + display name + an
 * account-initial chip. When the room's VM is not in any streamed window, it
 * degrades to the account chip alone (looked up from {@link accountsStore} by the
 * selection's `accountId`) — never a crash. Renders nothing when no room is open or
 * the account is unknown.
 */
function ConversationHeaderIdentity({ accountId }: { accountId: string | null }) {
  const room = useSelectedRoomVm();
  const account = useAccountsStore((s) =>
    accountId === null ? null : (s.accounts.find((a) => a.accountId === accountId) ?? null),
  );

  if (room !== null) {
    return (
      <div className="flex min-w-0 items-center gap-2">
        <RoomAvatar room={room} size="lg" />
        <span className="truncate font-medium text-sm" title={room.displayName}>
          {room.displayName}
        </span>
        {account !== null && (
          <AccountInitialChip userId={account.userId} hueIndex={account.hueIndex} />
        )}
      </div>
    );
  }
  // No streamed VM for the selection: degrade to the account chip alone.
  if (account !== null) {
    return (
      <div className="flex min-w-0 items-center gap-2">
        <AccountInitialChip userId={account.userId} hueIndex={account.hueIndex} />
      </div>
    );
  }
  return null;
}

/**
 * The non-dismissible in-conversation re-link banner (Story 6.5, FR-28, UX-DR11).
 *
 * Shown iff the open room's `(accountId, networkId)` matches an *unhealthy* bridge
 * session (both keys must match — the machine `networkId`, never the display label).
 * The banner is persistent until the session recovers — never a dismissible toast.
 * Its single "Re-link" action opens the shipped {@link BridgeLoginSheet} for that exact
 * `(accountId, networkId)` (the same `start_bridge_login` entry the card uses — no new
 * login flow). Renders nothing for a native room, a healthy/unmonitored session, or no
 * open room. The health state is Rust-authoritative — this is a pure projection.
 */
export function ConversationHealthBanner({
  accountId,
  networkId,
}: {
  accountId: string | null;
  networkId: string | null;
}) {
  const [relinkOpen, setRelinkOpen] = useState(false);
  const health = useBridgeHealth(accountId ?? "", networkId ?? "");

  if (accountId === null || networkId === null || health === undefined) {
    return null;
  }
  if (health.health === "healthy") {
    return null;
  }

  return (
    <div className="shrink-0 px-3 pt-2">
      {/* role="alert" (not "status") — an unhealthy session is a persistent, actionable
          problem the user must see. No dismiss control: it clears only on recovery. */}
      <Alert role="alert" variant="destructive" className="pr-28">
        <AlertDescription>
          {health.networkName} disconnected — messages may not arrive.
        </AlertDescription>
        <AlertAction>
          <Button type="button" variant="outline" size="xs" onClick={() => setRelinkOpen(true)}>
            Re-link
          </Button>
        </AlertAction>
      </Alert>
      <BridgeLoginSheet
        accountId={accountId}
        networkId={networkId}
        networkName={health.networkName}
        open={relinkOpen}
        onOpenChange={setRelinkOpen}
      />
    </div>
  );
}

export function ConversationPane({ detailOpen, onToggleDetail, toggleRef }: ConversationPaneProps) {
  const selected = useRoomsStore((s) => s.selected);
  const accountId = selected?.accountId ?? null;
  const selectedRoomId = selected?.roomId ?? null;
  // The open room's stable machine `networkId` (Story 6.5) — the health join key.
  // `null` for a native room or when the room's VM isn't in any streamed window.
  const selectedRoom = useSelectedRoomVm();
  const selectedNetworkId = selectedRoom?.networkId ?? null;
  // A pending search deep-link focus target (Story 5.4): resolved to a timeline
  // render key, scrolled to, and tinted once the target room's timeline is loaded.
  const focusEvent = useRoomsStore((s) => s.focusEvent);
  const items = useTimelineStore((s) => s.items);
  const pending = useComposerStore((s) => s.pending);
  const selectedKey = useComposerStore((s) => s.selectedKey);
  // The open conversation's account status drives the "Queued" caption. An empty
  // key (no room open) reads as `undefined` → not offline.
  const offline = useAccountStatus(accountId ?? "") === "offline";
  const [errored, setErrored] = useState(false);
  const [loaded, setLoaded] = useState(false);
  // The opaque render key of the media message whose preview overlay is open, or
  // `null` when closed (Story 3.6).
  const [previewKey, setPreviewKey] = useState<string | null>(null);
  // The opaque render key of the own message pending a delete-for-everyone
  // confirmation, or `null` when the dialog is closed (Story 3.8).
  const [deleteKey, setDeleteKey] = useState<string | null>(null);
  // The members currently typing in the open room (Story 3.9), and the live
  // back-pagination status. Both are pure Rust-streamed mirrors reset on room change.
  const [typists, setTypists] = useState<TypistVm[]>([]);
  const [pagination, setPagination] = useState<PaginationStatusBatch>({
    state: "idle",
    hitStart: false,
  });
  // Whether a pagination request the frontend fired is in flight (drives the
  // spinner immediately, before the status stream reports `paginating`, and gates
  // the top-scroll trigger from firing again).
  const [paginationError, setPaginationError] = useState(false);
  // An honest, non-blocking note shown when a search deep-link target is further
  // back in history than the loaded window + bounded live paginate can reach
  // (archive-first seek-to-event is Story 5.6). `null` hides it.
  const [deepLinkNote, setDeepLinkNote] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  // The search deep-link focus target already handled (its `account|room|event`
  // key), so a re-render never spawns a second concurrent landing attempt (Story 5.4).
  const handledFocusRef = useRef<string | null>(null);
  // Scroll-preservation bookkeeping (Story 3.9): the scrollHeight captured *before*
  // the last applied batch, and whether the user was near the bottom then. On the
  // next layout after items change we either compensate scrollTop for a prepend
  // (older history) or auto-scroll to the bottom for near-bottom bottom-growth.
  const prevScrollHeight = useRef(0);
  const wasNearBottom = useRef(true);
  const prevItemCount = useRef(0);
  // The kind of the most recently applied batch (reset / prepend / other), so the
  // scroll layout effect compensates only a genuine older-history prepend and
  // never yanks the view on a bottom-append.
  const lastBatchKind = useRef<"reset" | "prepend" | "other">("reset");
  // Guard so we fire at most one back-pagination at a time from the scroll trigger.
  const paginatingRef = useRef(false);
  // The newest item key already marked read, so the read receipt re-advances at
  // most once per new-content settle (debounced) while the room stays open.
  const lastMarkedKey = useRef<string | null>(null);

  // The body to prefill the composer with when entering edit mode (the target
  // message's current body), or `null` outside edit.
  const editPrefill =
    pending?.mode === "edit"
      ? ((): string | null => {
          const target = items.find((it) => it.kind === "message" && it.key === pending.targetKey);
          return target?.kind === "message" ? target.body : null;
        })()
      : null;

  useEffect(() => {
    if (accountId === null || selectedRoomId === null) {
      // No conversation open, or the account went away (e.g. sign-out): drop any
      // rendered timeline so a previous room's / account's messages never
      // linger, and reset the load/error state.
      timelineStore.getState().clear();
      composerStore.getState().clear();
      composerStore.getState().clearSelection();
      attachmentsStore.getState().clear();
      setErrored(false);
      setLoaded(false);
      setPreviewKey(null);
      setDeleteKey(null);
      setTypists([]);
      setPagination({ state: "idle", hitStart: false });
      setPaginationError(false);
      return;
    }

    setErrored(false);
    setLoaded(false);
    setPreviewKey(null);
    setDeleteKey(null);
    setTypists([]);
    setPagination({ state: "idle", hitStart: false });
    setPaginationError(false);
    paginatingRef.current = false;
    prevScrollHeight.current = 0;
    wasNearBottom.current = true;
    prevItemCount.current = 0;
    lastBatchKind.current = "reset";
    lastMarkedKey.current = null;
    // Establish clean state at mount so the newest mount always wins; clearing
    // in cleanup instead would race the next room's mount.
    timelineStore.getState().clear();
    // A room switch drops any pending reply/edit context, selection, and the
    // attachment tray.
    composerStore.getState().clear();
    composerStore.getState().clearSelection();
    attachmentsStore.getState().clear();
    let subscriptionId: number | null = null;
    let cancelled = false;

    // Gate the sink so it no-ops after cleanup (post-unmount / StrictMode late
    // batches never mutate the store).
    const onBatch = (b: TimelineBatch) => {
      if (!cancelled) {
        // Capture pre-mutation scroll metrics so the layout effect can preserve the
        // user's visual position when older history prepends (Story 3.9): the
        // height before this batch and whether the user was near the bottom.
        const el = scrollRef.current;
        if (el) {
          prevScrollHeight.current = el.scrollHeight;
          wasNearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
        }
        lastBatchKind.current = classifyBatch(b);
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

  // Typing + back-pagination status subscriptions (Story 3.9). Both are opened on
  // room view and torn down on room change / unmount (mirroring the timeline
  // subscription lifecycle). The typing set and pagination status are pure
  // Rust-streamed mirrors — the frontend renders them, never derives them. Marking
  // the room read on view emits a public `m.read` receipt (best-effort).
  useEffect(() => {
    if (accountId === null || selectedRoomId === null) {
      return;
    }
    let cancelled = false;
    let typingSub: number | null = null;
    let paginationSub: number | null = null;

    subscribeTyping(accountId, selectedRoomId, (b: TypingBatch) => {
      if (!cancelled) {
        setTypists(b.typists);
      }
    })
      .then((id) => {
        if (cancelled) {
          void unsubscribeTyping(accountId, id);
          return;
        }
        typingSub = id;
      })
      .catch(() => {});

    subscribePaginationStatus(accountId, selectedRoomId, (b: PaginationStatusBatch) => {
      if (!cancelled) {
        // Mirror the SDK-streamed status verbatim. The in-flight guard and the
        // inline error are owned by the fetch promise (see `runPaginate`), not the
        // status stream — an idle/paginating batch must never silently clear a
        // genuine error boundary the user still needs to see and retry.
        setPagination(b);
      }
    })
      .then((id) => {
        if (cancelled) {
          void unsubscribePaginationStatus(accountId, id);
          return;
        }
        paginationSub = id;
      })
      .catch(() => {});

    // Mark the room read on view (best-effort — swallow any rejection).
    markRoomRead(accountId, selectedRoomId).catch(() => {});

    return () => {
      cancelled = true;
      if (typingSub !== null) {
        void unsubscribeTyping(accountId, typingSub);
      }
      if (paginationSub !== null) {
        void unsubscribePaginationStatus(accountId, paginationSub);
      }
    };
  }, [accountId, selectedRoomId]);

  // Re-mark the room read when new content settles while it stays open (Story 3.9),
  // so the user's public `m.read` advances past messages read in place — not only
  // at room-open. Debounced so a burst of incoming events emits a single receipt on
  // the newest item; best-effort (swallow rejections). The mark-on-view above still
  // handles the initial open promptly.
  useEffect(() => {
    if (accountId === null || selectedRoomId === null || items.length === 0) {
      return;
    }
    const newestKey = items[items.length - 1]?.key ?? null;
    if (newestKey === null || newestKey === lastMarkedKey.current) {
      return;
    }
    const timer = setTimeout(() => {
      lastMarkedKey.current = newestKey;
      markRoomRead(accountId, selectedRoomId).catch(() => {});
    }, MARK_READ_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [items, accountId, selectedRoomId]);

  // Native drag-drop ingestion (Story 3.7): while a room is open, a file dropped
  // anywhere on the window yields OS **paths** (Rust reads the files — no bytes
  // cross IPC), which are pushed into the composer's pending-attachment tray. The
  // listener is torn down on room close / unmount. `onDragDropEvent` resolves
  // asynchronously, so a late listener is unlistened immediately if we already
  // unmounted.
  useEffect(() => {
    if (accountId === null || selectedRoomId === null) {
      return;
    }
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") {
          return;
        }
        const paths = event.payload.paths;
        if (paths.length === 0) {
          return;
        }
        // A directory drop can't be read as a single attachment; the Rust file
        // read of a directory path fails and surfaces as a per-item send error, so
        // dropping paths verbatim is safe. Bytes never cross here — only paths.
        attachmentsStore.getState().addMany(
          paths.map((path): PendingAttachment => {
            const parts = path.split(/[/\\]/);
            return {
              id: attachmentId(),
              kind: "path",
              path,
              filename: parts[parts.length - 1] || path,
            };
          }),
        );
      })
      .then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [accountId, selectedRoomId]);

  // Scroll management on timeline change (Story 3.9). Preserves the user's visual
  // position when older history prepends (compensating scrollTop by the height
  // delta) so a ≥10k-event back-scroll never yanks the view, and only auto-scrolls
  // to the bottom on bottom-growth when the user was already near the bottom. A
  // `Reset` snapshot (first load / re-subscribe) always anchors to the bottom.
  // Runs in a layout effect so the scroll adjust happens before paint (no flicker).
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el || items.length === 0) {
      prevItemCount.current = items.length;
      return;
    }
    const kind = lastBatchKind.current;
    const heightDelta = el.scrollHeight - prevScrollHeight.current;

    if (kind === "reset" || prevItemCount.current === 0 || items.length < prevItemCount.current) {
      // A wholesale reset (first load / re-subscribe) or a shrink: anchor to the bottom.
      el.scrollTop = el.scrollHeight;
    } else if (kind === "prepend" && !wasNearBottom.current) {
      // Older history prepended while the user reads up-timeline: preserve the
      // visual position by compensating scrollTop for the added height (no yank).
      if (heightDelta > 0) {
        el.scrollTop += heightDelta;
      }
    } else if (wasNearBottom.current) {
      // Bottom growth (a new message) while the user was near the bottom: follow it.
      el.scrollTop = el.scrollHeight;
    }
    // A bottom-append while scrolled up (reading history): leave scrollTop untouched
    // so the newly arrived message below the viewport never jolts the view down.
    prevItemCount.current = items.length;
  }, [items]);

  const rows = toRenderedRows(items);
  const roomLoaded = accountId !== null && selectedRoomId !== null && loaded && !errored;

  // The honest history-boundary state (Story 3.9), in precedence order: the
  // homeserver start is a definitive truth (no more history), so it wins; offline
  // is next because when disconnected we genuinely cannot load more — it overrides
  // a transient in-flight spinner or a stale retriable error so the boundary stops
  // rather than spins forever (epic UX honesty rule); then a failed fetch shows a
  // retriable error; then the in-flight spinner; otherwise nothing (idle — more
  // history may exist, the near-top scroll trigger paginates).
  const boundaryState: HistoryBoundaryState = pagination.hitStart
    ? "atStart"
    : offline
      ? "offline"
      : paginationError
        ? "error"
        : pagination.state === "paginating"
          ? "paginating"
          : "idle";

  const onSend = useCallback(
    async (body: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      // Route the dispatch by the pending context: reply / edit / plain text
      // (all through the single Rust send gate).
      const current = composerStore.getState().pending;
      if (current?.mode === "reply") {
        await sendReply(accountId, selectedRoomId, current.targetKey, body);
      } else if (current?.mode === "edit") {
        await editMessage(accountId, selectedRoomId, current.targetKey, body);
      } else {
        await sendText(accountId, selectedRoomId, body);
      }
      // Clear only the context we just dispatched: if the user started a *new*
      // reply/edit during the in-flight enqueue, it must survive.
      const afterSend = composerStore.getState().pending;
      if (
        current !== null &&
        afterSend?.mode === current.mode &&
        afterSend?.targetKey === current.targetKey
      ) {
        composerStore.getState().clear();
      }
    },
    [accountId, selectedRoomId],
  );

  // Emit the account's typing notice (Story 3.9). Best-effort: swallow rejections so
  // a typing dispatch is never an unhandled promise or a UI error.
  const onTyping = useCallback(
    (typing: boolean) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      setTyping(accountId, selectedRoomId, typing).catch(() => {});
    },
    [accountId, selectedRoomId],
  );

  // Fire a back-pagination when the user scrolls near the top (Story 3.9), gated so
  // it never spins forever: skip while a request is in flight, when the homeserver
  // start is reached, or when offline (the boundary states offline instead). Older
  // events arrive over the timeline diff stream and prepend in place (the layout
  // effect preserves scroll). A failure surfaces a retriable inline boundary error.
  // Single-flight back-pagination fetch. `paginatingRef` is the sole in-flight
  // guard (cleared unconditionally when the promise settles); the resolved boolean
  // is authoritative for reaching the homeserver start, so pagination stops even if
  // the status stream is slow or silent, and a failure sets a sticky retriable error.
  const runPaginate = useCallback(() => {
    // `paginatingRef` is the sole in-flight guard: enforce it here so *every*
    // entry point (the near-top scroll trigger and the boundary Retry button) is
    // single-flight, not only the scroll path — a rapid Retry can no longer admit
    // a concurrent fetch.
    if (accountId === null || selectedRoomId === null || paginatingRef.current) {
      return;
    }
    paginatingRef.current = true;
    paginateBackwards(accountId, selectedRoomId, PAGINATE_BATCH)
      .then((hitStart) => {
        if (hitStart) {
          setPagination((p) => ({ ...p, state: "idle", hitStart: true }));
        }
      })
      .catch(() => {
        // A failed pagination surfaces a retriable inline boundary error (and stops
        // the spinner); it persists until the user retries — the status stream no
        // longer clears it.
        setPaginationError(true);
      })
      .finally(() => {
        paginatingRef.current = false;
      });
  }, [accountId, selectedRoomId]);

  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    if (el === null) {
      return;
    }
    if (
      el.scrollTop > NEAR_TOP_PX ||
      paginatingRef.current ||
      pagination.hitStart ||
      pagination.state === "paginating" ||
      offline ||
      paginationError
    ) {
      return;
    }
    runPaginate();
  }, [runPaginate, offline, pagination, paginationError]);

  // Retry a failed pagination from the boundary's Retry button (Story 3.9). Guarded
  // on offline so a retry that would immediately re-fail is not offered while
  // disconnected (the boundary shows the offline state instead).
  const onRetryPagination = useCallback(() => {
    if (offline) {
      return;
    }
    setPaginationError(false);
    runPaginate();
  }, [runPaginate, offline]);

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

  // Dispatch each pending attachment through the single Rust send gate (Story
  // 3.7): a path attachment via `sendAttachmentPath` (Rust reads the file), a
  // pasted-bytes attachment via `sendAttachmentBytes` (raw binary IPC body). The
  // caption (single-attachment only) rides on the first dispatch. Rejects if any
  // enqueue fails so the composer keeps the tray for retry.
  const onSendAttachments = useCallback(
    async (toSend: PendingAttachment[], caption?: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      for (const attachment of toSend) {
        if (attachment.kind === "path") {
          await sendAttachmentPath(accountId, selectedRoomId, attachment.path, caption);
        } else {
          await sendAttachmentBytes(
            accountId,
            selectedRoomId,
            attachment.bytes,
            attachment.filename,
            attachment.mime,
            caption,
          );
        }
        // Drop each attachment from the tray the moment it is enqueued so a later
        // failure in this loop (or a failed trailing text send) never re-dispatches
        // an already-enqueued item when the user retries — preventing duplicate
        // media sends. On full success the tray is already empty and the
        // composer's `clear()` is a harmless no-op.
        attachmentsStore.getState().remove(attachment.id);
      }
    },
    [accountId, selectedRoomId],
  );

  // Cancel an in-flight outgoing media echo by aborting its queued send (Story
  // 3.7). Best-effort: if it already dispatched, the abort is a no-op and the
  // message stays sent. A rejection (e.g. the echo reconciled away) is swallowed
  // so it is never an unhandled promise.
  const onCancelSend = useCallback(
    (key: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      cancelSend(accountId, selectedRoomId, key).catch(() => {});
    },
    [accountId, selectedRoomId],
  );

  const onReply = useCallback(
    (key: string) => {
      const target = items.find((it) => it.kind === "message" && it.key === key);
      if (target?.kind !== "message") {
        return;
      }
      composerStore.getState().startReply({
        targetKey: key,
        sender: target.senderDisplayName ?? target.sender,
        bodyPreview: previewOf(target.body),
      });
    },
    [items],
  );

  const onEdit = useCallback(
    (key: string) => {
      const target = items.find((it) => it.kind === "message" && it.key === key);
      // Only own text messages are editable (Rust also gates on `is_editable()`).
      if (target?.kind !== "message" || !target.isOwn) {
        return;
      }
      composerStore.getState().startEdit({ targetKey: key, body: target.body }, "");
    },
    [items],
  );

  // Open the delete-for-everyone confirmation for an own message (Story 3.8,
  // FR-15). Fired by the action-bar Delete button and the ⌫/Delete key. Only own
  // messages are deletable (Rust also gates redaction dispatch); a non-own or
  // missing target is a no-op. The actual redaction runs from the dialog's confirm.
  const onDelete = useCallback(
    (key: string) => {
      const target = items.find((it) => it.kind === "message" && it.key === key);
      // Delete-for-everyone is scoped to an own message that has actually been sent;
      // an unsent/failed echo (`sendState !== null`) has no remote event to redact.
      if (target?.kind !== "message" || !target.isOwn || target.sendState !== null) {
        return;
      }
      setDeleteKey(key);
    },
    [items],
  );

  // Toggle an emoji reaction on a message (Story 3.5, FR-12). Fired by both the
  // action-bar Popover pick and a click on an existing pill. Reactions are
  // stateless on the frontend: fire the IPC and let the diff stream re-render the
  // pills. A rejection (e.g. the target reconciled away → `TargetNotFound`) is
  // swallowed so it is never an unhandled promise.
  const onToggleReaction = useCallback(
    (key: string, emoji: string) => {
      if (accountId === null || selectedRoomId === null) {
        return;
      }
      toggleReaction(accountId, selectedRoomId, key, emoji).catch(() => {});
    },
    [accountId, selectedRoomId],
  );

  // Open the Quick-Look preview overlay for a media message (Story 3.6). The
  // resolved media VM is looked up from the live timeline by key at render time.
  const onOpenPreview = useCallback((key: string) => setPreviewKey(key), []);
  const onClosePreview = useCallback(() => setPreviewKey(null), []);

  // The media VM to preview, resolved from the current timeline by `previewKey`.
  // A `null` (item scrolled away / room changed / non-media target) closes the
  // overlay cleanly.
  const previewMedia =
    previewKey === null
      ? null
      : (items.find((it): it is MessageVm => it.kind === "message" && it.key === previewKey)
          ?.media ?? null);

  const onCancelPending = useCallback(() => composerStore.getState().cancel(), []);

  // Scroll a loaded message into view and flash a temporary highlight. `variant`
  // picks the highlight style: the default reply/jump `ring` (1200 ms), or the
  // search deep-link `search-highlight` BACKGROUND tint (2000 ms, Story 5.4).
  // Returns whether the target row was found in the loaded DOM (so the search
  // deep-link can decide whether to paginate + retry or degrade honestly).
  const jumpToKey = useCallback((key: string, variant: "ring" | "search" = "ring"): boolean => {
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-msg-key="${CSS.escape(key)}"]`);
    if (!el) {
      return false;
    }
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    const classes =
      variant === "search"
        ? ["bg-search-highlight", "text-search-highlight-foreground"]
        : ["ring-2", "ring-ring", "ring-offset-1", "ring-offset-background"];
    const duration = variant === "search" ? 2000 : 1200;
    el.classList.add(...classes);
    window.setTimeout(() => {
      el.classList.remove(...classes);
    }, duration);
    return true;
  }, []);

  const onJumpTo = useCallback((key: string) => jumpToKey(key, "ring"), [jumpToKey]);

  // Search deep-link landing (Story 5.4, FR-34). When a `focusEvent` is pending for
  // the open room and its timeline has loaded, resolve the hit's `eventId` to the
  // opaque render key via the backend (no event id is ever added to a timeline VM),
  // scroll to it and apply the `search-highlight` tint for 2 s. When the event is
  // not yet in the loaded window, best-effort `paginateBackwards` in bounded rounds
  // and retry; if still unreachable, leave the Chat open with an honest note —
  // never a wrong jump, never a silent no-op. The pending focus is cleared once
  // handled so it fires exactly once.
  useEffect(() => {
    if (
      focusEvent === null ||
      accountId === null ||
      selectedRoomId === null ||
      focusEvent.accountId !== accountId ||
      focusEvent.roomId !== selectedRoomId ||
      !loaded
    ) {
      return;
    }
    // Start the landing at most once per distinct focus target: a re-render (e.g.
    // pagination prepends new items) must not spawn a second concurrent attempt.
    const targetKey = `${focusEvent.accountId}|${focusEvent.roomId}|${focusEvent.eventId}`;
    if (handledFocusRef.current === targetKey) {
      return;
    }
    handledFocusRef.current = targetKey;
    const targetAccount = accountId;
    const targetRoom = selectedRoomId;
    const targetEvent = focusEvent.eventId;
    let cancelled = false;
    // Bounded live paginate rounds (archive-first seek is Story 5.6). Each round
    // pages a batch of older events, then re-resolves. `hitStart` from the paginate
    // stops early when the room's homeserver start is reached.
    const MAX_ROUNDS = 5;
    const BATCH = 40;
    setDeepLinkNote(null);

    const tryLand = async () => {
      for (let round = 0; round <= MAX_ROUNDS; round += 1) {
        if (cancelled) {
          return;
        }
        let key: string | null;
        try {
          key = await resolveTimelineEventKey(targetAccount, targetRoom, targetEvent);
        } catch {
          // An unparsable id (should not happen for a real hit) — degrade honestly.
          key = null;
          break;
        }
        if (cancelled) {
          return;
        }
        if (key !== null) {
          // The event is loaded (the resolver found it in the timeline). It may
          // not be painted yet (a just-prepended row); retry the DOM jump a few
          // times as React commits. Paginating older history cannot help an
          // already-loaded event, so on a persistent paint-miss degrade honestly
          // rather than burn pagination rounds.
          for (let attempt = 0; attempt < 3; attempt += 1) {
            await Promise.resolve();
            if (cancelled) {
              return;
            }
            if (jumpToKey(key, "search")) {
              return;
            }
            await new Promise((resolve) => window.setTimeout(resolve, 50));
            if (cancelled) {
              return;
            }
          }
          break;
        }
        if (round === MAX_ROUNDS) {
          break;
        }
        // Not loaded yet: page older history and retry. Stop early at room start.
        try {
          const reachedStart = await paginateBackwards(targetAccount, targetRoom, BATCH);
          if (reachedStart) {
            break;
          }
        } catch {
          break;
        }
        // Give the prepend ops a frame to apply to the store/DOM before re-resolving.
        await new Promise((resolve) => window.setTimeout(resolve, 60));
      }
      if (!cancelled) {
        setDeepLinkNote("This message is further back in history than keeper has loaded yet.");
      }
    };
    // Run the landing to completion, then clear the pending focus. Clearing here
    // (not synchronously) avoids re-triggering/cancelling the in-flight attempt; the
    // ref guard already prevents a duplicate start before this resolves.
    void tryLand().finally(() => {
      // Clear only the focus we actually handled — a newer requestFocus for a
      // different Chat (even one that coincidentally shares this event id) must
      // survive, so compare the full account|room|event identity.
      const current = roomsStore.getState().focusEvent;
      if (
        current !== null &&
        current.accountId === targetAccount &&
        current.roomId === targetRoom &&
        current.eventId === targetEvent
      ) {
        roomsStore.getState().clearFocus();
      }
      // Release the once-guard now the attempt has finished, so re-activating the
      // *same* hit later re-lands instead of being a silent no-op (spec invariant).
      // The in-flight window was already protected by the ref for `tryLand`'s
      // duration; a superseding focus has since overwritten the ref and must not
      // be released here.
      if (handledFocusRef.current === targetKey) {
        handledFocusRef.current = null;
      }
    });
    return () => {
      cancelled = true;
    };
  }, [focusEvent, accountId, selectedRoomId, loaded, jumpToKey]);

  // Drop the deep-link note whenever the open room changes (a new Chat starts clean).
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset keyed on the room pair, not the note value
  useEffect(() => {
    setDeepLinkNote(null);
  }, [accountId, selectedRoomId]);

  // Keyboard affordances (epic): ↑/↓ select a message; `r` reply the selected;
  // `e` edit the selected (own only); ⌫/Delete opens the delete-for-everyone
  // confirmation for the selected own message (Story 3.8, FR-15); `↑` in an empty
  // composer edits the last own message; Esc clears the pending context / selection.
  const onKeyDown = useCallback(
    (e: KeyboardEvent<HTMLDivElement>) => {
      // Ignore keys typed into the composer's textarea (except the empty-composer
      // ↑, handled by the composer/its own guard below via `target` check).
      const inTextarea = (e.target as HTMLElement).tagName === "TEXTAREA";
      const messageKeys = items
        .filter((it): it is MessageVm => it.kind === "message")
        .map((it) => it.key);

      if (e.key === "Escape") {
        composerStore.getState().clear();
        composerStore.getState().clearSelection();
        return;
      }

      if (inTextarea) {
        return;
      }

      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        if (messageKeys.length === 0) {
          return;
        }
        e.preventDefault();
        const cur = composerStore.getState().selectedKey;
        const idx = cur === null ? -1 : messageKeys.indexOf(cur);
        const nextIdx =
          e.key === "ArrowUp"
            ? Math.max(0, (idx === -1 ? messageKeys.length : idx) - 1)
            : Math.min(messageKeys.length - 1, idx + 1);
        composerStore.getState().select(messageKeys[nextIdx]);
        return;
      }

      const sel = composerStore.getState().selectedKey;
      if (e.key === "r" && sel !== null) {
        e.preventDefault();
        onReply(sel);
        return;
      }
      if (e.key === "e" && sel !== null) {
        e.preventDefault();
        onEdit(sel);
        return;
      }
      if (
        (e.key === "Backspace" || e.key === "Delete") &&
        !e.metaKey &&
        !e.ctrlKey &&
        !e.altKey &&
        sel !== null
      ) {
        // Delete-for-everyone only applies to the user's OWN, already-sent selected
        // message (Story 3.8, FR-15). Only intercept the key when the target is
        // actually deletable — a bare ⌫ on someone else's (or an unsent) message
        // keeps its default behavior instead of being silently swallowed. Modifier
        // chords (⌘/Ctrl/Alt+⌫, e.g. delete-word) are left alone.
        const target = items.find((it): it is MessageVm => it.kind === "message" && it.key === sel);
        if (target?.isOwn && target.sendState === null) {
          e.preventDefault();
          onDelete(sel);
        }
      }
    },
    [items, onReply, onEdit, onDelete],
  );

  // `↑` in an empty composer edits the last own text message (epic affordance).
  const onComposerArrowUp = useCallback(() => {
    const lastOwn = [...items]
      .reverse()
      .find((it): it is MessageVm => it.kind === "message" && it.isOwn);
    if (lastOwn) {
      onEdit(lastOwn.key);
    }
  }, [items, onEdit]);

  return (
    <main className="flex h-full min-w-0 flex-1 flex-col bg-background">
      <div className="flex shrink-0 items-center justify-between gap-2 border-border border-b p-2">
        <ConversationHeaderIdentity accountId={accountId} />
        <div className="flex shrink-0 items-center gap-1">
          {accountId !== null && selectedRoomId !== null && (
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="Export this chat"
              onClick={() =>
                exportStore.getState().open({
                  scope: "chat",
                  accountId,
                  roomId: selectedRoomId,
                })
              }
              className="focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
            >
              <Download aria-hidden="true" />
            </Button>
          )}
          <Button
            ref={toggleRef}
            type="button"
            variant="ghost"
            size="icon"
            aria-label="Toggle detail panel"
            aria-pressed={detailOpen}
            onClick={onToggleDetail}
            className="shrink-0 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            <PanelRight aria-hidden="true" />
          </Button>
        </div>
      </div>
      {/* Non-dismissible in-conversation re-link banner (Story 6.5, UX-DR11): shown iff
          the open room's (accountId, networkId) session is unhealthy → opens the login
          stepper for that exact bridge. Persistent until the session recovers. */}
      <ConversationHealthBanner accountId={accountId} networkId={selectedNetworkId} />
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
        // biome-ignore lint/a11y/noStaticElementInteractions: message-list keyboard affordances (↑/↓/r/e) live on the scroll region; individual actions have their own labeled buttons.
        <div
          ref={scrollRef}
          className="flex min-h-0 flex-1 flex-col overflow-y-auto"
          onKeyDown={onKeyDown}
          onScroll={onScroll}
        >
          <ol
            aria-label="Messages"
            className="mx-auto mt-auto flex w-full max-w-[720px] flex-col px-4 py-4"
          >
            {/* Top-of-timeline history boundary (Story 3.9): spinner while
                paginating, offline stop, or "start of the conversation". */}
            <li aria-hidden={boundaryState === "idle"}>
              <HistoryBoundary state={boundaryState} onRetry={onRetryPagination} />
            </li>
            {rows.map((row) =>
              row.kind === "utd" ? (
                <li key={row.item.key}>
                  <UtdStub />
                </li>
              ) : row.kind === "redacted" ? (
                <li key={row.item.key}>
                  <RedactedStub />
                </li>
              ) : (
                <li key={row.item.key}>
                  <MessageBubble
                    item={row.item}
                    accountId={accountId ?? undefined}
                    roomId={selectedRoomId ?? undefined}
                    grouped={row.grouped}
                    groupTail={row.groupTail}
                    onRetry={onRetry}
                    offline={offline}
                    onReply={onReply}
                    onEdit={onEdit}
                    onDelete={onDelete}
                    onJumpTo={onJumpTo}
                    selected={selectedKey === row.item.key}
                    onToggleReaction={onToggleReaction}
                    onOpenPreview={onOpenPreview}
                    onCancelSend={onCancelSend}
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
            {/* Honest, non-blocking search deep-link fallback (Story 5.4): shown when
                the matched message is further back than the loaded window reaches
                (archive-first seek-to-event is Story 5.6). Never a wrong jump. */}
            {deepLinkNote !== null && (
              <p role="status" className="mb-2 text-xs text-muted-foreground">
                {deepLinkNote}
              </p>
            )}
            {/* Typing indicator (Story 3.9): "<name> is typing…" between the
                timeline and composer; renders an empty live region when idle. */}
            <TypingIndicator typists={typists} />
            <Composer
              key={selectedRoomId}
              onSend={onSend}
              onSendAttachments={onSendAttachments}
              disabled={!roomLoaded}
              pending={pending}
              editPrefill={editPrefill}
              onCancelPending={onCancelPending}
              onEmptyArrowUp={onComposerArrowUp}
              onTyping={onTyping}
            />
          </div>
        </div>
      )}
      {/* Quick-Look media preview overlay (Story 3.6). Rendered once; open state
          is driven by the resolved media VM (null closes it). Esc/backdrop close
          and radix returns focus to the timeline bubble. */}
      <MediaPreviewOverlay media={previewMedia} onClose={onClosePreview} />
      {/* Delete-for-everyone confirmation (Story 3.8). Controlled by `deleteKey`;
          on open it probes the bridged Network label and frames the copy honestly.
          Only mounted with a live account/room so the confirm can dispatch. */}
      {accountId !== null && selectedRoomId !== null && (
        <DeleteMessageDialog
          accountId={accountId}
          roomId={selectedRoomId}
          itemKey={deleteKey}
          onClose={() => setDeleteKey(null)}
        />
      )}
    </main>
  );
}
