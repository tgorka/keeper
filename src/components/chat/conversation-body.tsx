/**
 * One open room's timeline and composer (FR-8/FR-9, AD-4/AD-8/AD-19), drawn
 * wherever a conversation is held open: the chat's conversation pane and the
 * notes view's dock (UX-DR130).
 *
 * On a `(accountId, roomId)` change it clears its timeline store (newest-mount-
 * wins), subscribes to the room's timeline channel, and mirrors the streamed ops
 * into the ordered timeline store (never sorting). Which stores it is the
 * caller's: under no provider they are the chat's singletons, and the dock
 * provides its own, so the two never share a reply context, a selection or a
 * tray ({@link useConversationStores}). Rendered `Message` items become grouped
 * {@link MessageBubble}s inside a bottom-anchored scroll region with a 720 px-max
 * centered column; `Other` items are skipped (they exist only to keep diff
 * indices aligned). Cleanup — StrictMode double-mount, room change, unmount —
 * unsubscribes the backend task and clears the store, so timelines never leak
 * or stack. A failed subscribe surfaces an honest inline error instead of a
 * silent spinner (AD-21). A bottom {@link Composer} footer (720 px-centered,
 * `border-t`) sends via the single Rust dispatch gate — disabled until a room's
 * timeline is loaded — and outgoing bubbles carry a Rust-authoritative
 * send-state caption with a persistent `Failed — Retry` (FR-9, AD-13).
 *
 * Renders a fragment: the caller's column is the flex container.
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  type KeyboardEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { ApprovalRequest } from "@/components/agents/approval-card";
import { AgentRoomHeader } from "@/components/chat/agent-room-header";
import { Composer } from "@/components/chat/composer";
import { DeleteMessageDialog } from "@/components/chat/delete-message-dialog";
import { HistoryBoundary, type HistoryBoundaryState } from "@/components/chat/history-boundary";
import { MediaPreviewOverlay } from "@/components/chat/media-preview-overlay";
import { MessageBubble, type MessageVm } from "@/components/chat/message-bubble";
import { RedactedStub } from "@/components/chat/redacted-stub";
import { TypingIndicator } from "@/components/chat/typing-indicator";
import { UndoSendPill } from "@/components/chat/undo-send-pill";
import { UtdStub } from "@/components/chat/utd-stub";
import { Skeleton } from "@/components/ui/skeleton";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type {
  AgentRoomHeaderVm,
  ApprovalVm,
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
  subscribeOutbox,
  subscribePaginationStatus,
  subscribeTimeline,
  subscribeTyping,
  toggleReaction,
  unsubscribeOutbox,
  unsubscribePaginationStatus,
  unsubscribeTimeline,
  unsubscribeTyping,
} from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { attachmentId, type PendingAttachment } from "@/lib/stores/attachments";
import { useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import {
  useConversationComposer,
  useConversationStores,
  useConversationTimeline,
} from "@/lib/stores/conversation-stores";
import { refreshIncognito, useIncognitoPolicyVersion } from "@/lib/stores/incognito";
import { outboxStore, undoHeldSend, useHeldSends } from "@/lib/stores/outbox";
import { roomsStore, useRoomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

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

interface ConversationBodyProps {
  /** The room's owning account, or `null` when no conversation is open. */
  accountId: string | null;
  /** The open room, or `null` (the chat with nothing selected). */
  roomId: string | null;
  /**
   * Whether this conversation lands the search deep-link focus
   * (`roomsStore.focusEvent`, Story 5.4). The chat's does; the dock's does not —
   * a search hit opens the chat, never the dock.
   */
  followsSearch: boolean;
  /**
   * Whether a file dropped anywhere on the window goes into this composer's tray
   * (Story 3.7). The chat's does; the dock sits beside a note, where a drop is
   * the note's.
   */
  acceptsWindowDrops: boolean;
  /**
   * A control drawn in an agent room's header beside its scope chip — the
   * dock's scope editor. Absent, the scope is read-only.
   */
  scopeControl?: (header: AgentRoomHeaderVm) => ReactNode;
}

/** The `utd`-variant of {@link TimelineItemVm} (rendered as an honest stub). */
type UtdVm = Extract<TimelineItemVm, { kind: "utd" }>;

/** The `redacted`-variant of {@link TimelineItemVm} (rendered as an honest stub). */
type RedactedVm = Extract<TimelineItemVm, { kind: "redacted" }>;

/** The `approval`-variant of {@link TimelineItemVm}: where an agent's request arrived. */
type ApprovalItemVm = Extract<TimelineItemVm, { kind: "approval" }>;

/**
 * A renderable timeline row. A `message` row is a text bubble paired with whether
 * it continues a same-sender run (`grouped`) and whether it ends one (`groupTail`
 * — the transient send-state caption renders only on the tail). A `utd` row is an
 * undecryptable-event stub and a `redacted` row is a deleted-message stub (Story
 * 3.8); both are never grouped, break same-sender runs, and are emitted (not
 * skipped like `other`), so they render inline and never blank. An `approval`
 * row is an agent's request with its cards, drawn where it arrived (UX-DR136),
 * and breaks a run the same way.
 */
type RenderedRow =
  | { kind: "message"; item: MessageVm; grouped: boolean; groupTail: boolean }
  | { kind: "utd"; item: UtdVm }
  | { kind: "redacted"; item: RedactedVm }
  | { kind: "approval"; item: ApprovalItemVm; approval: ApprovalVm };

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
function toRenderedRows(items: TimelineItemVm[], approvals: ApprovalVm[]): RenderedRow[] {
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
    if (item.kind === "approval") {
      // Its cards travel beside the stream, in the batch that placed it; an
      // item whose cards are not here draws nothing, but still ends the run.
      closeRun();
      const approval = approvals.find((a) => a.id === item.id);
      if (approval !== undefined) {
        rendered.push({ kind: "approval", item, approval });
      }
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

export function ConversationBody({
  accountId,
  roomId,
  followsSearch,
  acceptsWindowDrops,
  scopeControl,
}: ConversationBodyProps) {
  const { timeline, composer, attachments: tray } = useConversationStores();
  // Phone tier (Story 13.5): gates the composer footer's keyboard/safe-area
  // bottom inset and the keyboard-resize bottom-pin. Desktop/tablet renders
  // byte-for-byte as before.
  const { phone } = useShellLayout();
  // A pending search deep-link focus target (Story 5.4): resolved to a timeline
  // render key, scrolled to, and tinted once the target room's timeline is loaded.
  const focusEvent = useRoomsStore((s) => (followsSearch ? s.focusEvent : null));
  const items = useConversationTimeline((s) => s.items);
  // An agent room's header (status line, scope and label chips), `null` in
  // every other room. Drawn on the phone too: it is not part of the pane's
  // own header row.
  const agentHeader = useConversationTimeline((s) => s.header);
  // An agent room's approval requests, by id; the stream's `approval` items say
  // where each is drawn.
  const approvals = useConversationTimeline((s) => s.approvals);
  // The answer that draws the growing caret: Rust names it only while the run
  // is `running`, and the run is checked here as well so a header that moved
  // on to `done` can never leave a caret behind.
  const caretKey = agentHeader?.status?.run === "running" ? agentHeader.caretKey : null;
  const pending = useConversationComposer((s) => s.pending);
  const selectedKey = useConversationComposer((s) => s.selectedKey);
  // The open conversation's account status drives the "Queued" caption. An empty
  // key (no room open) reads as `undefined` → not offline.
  const offline = useAccountStatus(accountId ?? "") === "offline";
  // Reduced-capability (iOS/phone) tier flag (Story 14.6), resolved here — where
  // `offline` is derived — and threaded to each bubble the same way: on the
  // reduced tier the offline "Queued …" caption reads "sends when keeper is open
  // and back online" (foreground-only sync honesty); desktop wording unchanged.
  const reducedCapability = useIsReducedCapabilityPlatform();
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
  // The conversation's whole extent, for shortcuts that belong to it alone.
  const rootRef = useRef<HTMLDivElement>(null);
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
    if (accountId === null || roomId === null) {
      // No conversation open, or the account went away (e.g. sign-out): drop any
      // rendered timeline so a previous room's / account's messages never
      // linger, and reset the load/error state.
      timeline.getState().clear();
      composer.getState().clear();
      composer.getState().clearSelection();
      tray.getState().clear();
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
    timeline.getState().clear();
    // A room switch drops any pending reply/edit context, selection, and the
    // attachment tray.
    composer.getState().clear();
    composer.getState().clearSelection();
    tray.getState().clear();
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
        timeline.getState().applyBatch(b);
        setLoaded(true);
      }
    };
    subscribeTimeline(accountId, roomId, onBatch)
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
      timeline.getState().clear();
    };
  }, [accountId, roomId, timeline, composer, tray]);

  // Undo-Send held-send subscription (Story 8.3): opened on room view and torn down on
  // room change / unmount (mirroring the timeline subscription lifecycle). Each snapshot
  // is a full, oldest-first set that REPLACES this room's mirrored held rows. The
  // `outbox` table in `keeper.db` is the source of truth — the store is a pure mirror,
  // keyed by room and held per conversation showing it: tearing down drops this room's
  // rows only when no other conversation (the chat and the dock on one room) still
  // shows them.
  useEffect(() => {
    if (accountId === null || roomId === null) {
      return;
    }
    const roomAccount = accountId;
    const room = roomId;
    let subscriptionId: number | null = null;
    let cancelled = false;
    outboxStore.getState().hold(roomAccount, room);
    subscribeOutbox(roomAccount, room, (batch) => {
      if (!cancelled) {
        outboxStore.getState().applySnapshot(roomAccount, room, batch.rows);
      }
    })
      .then((id) => {
        if (cancelled) {
          void unsubscribeOutbox(roomAccount, id);
          return;
        }
        subscriptionId = id;
      })
      .catch(() => {
        // A held-send subscription failure is non-fatal: the Chat still works, just
        // without live held-send surfaces (they re-appear on the next successful open).
      });
    return () => {
      cancelled = true;
      if (subscriptionId !== null) {
        void unsubscribeOutbox(roomAccount, subscriptionId);
      }
      outboxStore.getState().release(roomAccount, room);
    };
  }, [accountId, roomId]);

  // Typing + back-pagination status subscriptions (Story 3.9). Both are opened on
  // room view and torn down on room change / unmount (mirroring the timeline
  // subscription lifecycle). The typing set and pagination status are pure
  // Rust-streamed mirrors — the frontend renders them, never derives them. Marking
  // the room read on view emits a read receipt (best-effort) whose type — public
  // `m.read` or private `m.read.private` — the Rust core picks from the effective
  // Incognito policy (Story 8.1); the frontend never decides it.
  useEffect(() => {
    if (accountId === null || roomId === null) {
      return;
    }
    let cancelled = false;
    let typingSub: number | null = null;
    let paginationSub: number | null = null;

    subscribeTyping(accountId, roomId, (b: TypingBatch) => {
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

    subscribePaginationStatus(accountId, roomId, (b: PaginationStatusBatch) => {
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
    markRoomRead(accountId, roomId).catch(() => {});

    return () => {
      cancelled = true;
      if (typingSub !== null) {
        void unsubscribeTyping(accountId, typingSub);
      }
      if (paginationSub !== null) {
        void unsubscribePaginationStatus(accountId, paginationSub);
      }
    };
  }, [accountId, roomId]);

  // Refresh the effective Incognito VM into the mirror store on room open (Story 8.1),
  // so the header chip and composer ring reflect the resolved state for this chat.
  // Also re-runs on `incognitoPolicyVersion` bumps, so a global (Settings) or
  // per-account (menu) toggle reconciles the open chat without a room reopen.
  // Best-effort — a read failure leaves the last-observed state; a read that resolves
  // after a room switch is dropped via the `cancelled` guard.
  const incognitoPolicyVersion = useIncognitoPolicyVersion();
  // biome-ignore lint/correctness/useExhaustiveDependencies: `incognitoPolicyVersion` is a deliberate re-run trigger — a broad-scope (global/per-account) toggle bumps it to force this open chat to re-read its effective VM; it is not read in the body.
  useEffect(() => {
    if (accountId === null || roomId === null) {
      return;
    }
    let cancelled = false;
    void refreshIncognito(accountId, roomId, () => cancelled);
    return () => {
      cancelled = true;
    };
  }, [accountId, roomId, incognitoPolicyVersion]);

  // Re-mark the room read when new content settles while it stays open (Story 3.9),
  // so the user's read receipt advances past messages read in place — not only
  // at room-open. The receipt type (public `m.read` vs private `m.read.private`)
  // stays a Rust-side decision from the effective Incognito policy (Story 8.1).
  // Debounced so a burst of incoming events emits a single receipt on
  // the newest item; best-effort (swallow rejections). The mark-on-view above still
  // handles the initial open promptly.
  useEffect(() => {
    if (accountId === null || roomId === null || items.length === 0) {
      return;
    }
    const newestKey = items[items.length - 1]?.key ?? null;
    if (newestKey === null || newestKey === lastMarkedKey.current) {
      return;
    }
    const timer = setTimeout(() => {
      lastMarkedKey.current = newestKey;
      markRoomRead(accountId, roomId).catch(() => {});
    }, MARK_READ_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [items, accountId, roomId]);

  // Native drag-drop ingestion (Story 3.7): while a room is open, a file dropped
  // anywhere on the window yields OS **paths** (Rust reads the files — no bytes
  // cross IPC), which are pushed into the composer's pending-attachment tray. The
  // listener is torn down on room close / unmount. `onDragDropEvent` resolves
  // asynchronously, so a late listener is unlistened immediately if we already
  // unmounted.
  useEffect(() => {
    if (!acceptsWindowDrops || accountId === null || roomId === null) {
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
        tray.getState().addMany(
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
  }, [accountId, roomId, acceptsWindowDrops, tray]);

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

  const rows = toRenderedRows(items, approvals);
  const roomLoaded = accountId !== null && roomId !== null && loaded && !errored;
  const hasRows = rows.length > 0;

  // Keep a bottom-pinned timeline pinned across keyboard open/dismiss (Story
  // 13.5, UX-DR25): on phone the kb-inset/safe-area footer padding resizes the
  // scroller, which would otherwise strand the view off the bottom. A
  // ResizeObserver tracks whether the user was near the bottom *before* each
  // resize and re-anchors only then — reading up-timeline is never yanked, and
  // dismissal leaves no stranded offset. Phone-only, so desktop window-resize
  // behavior is untouched; jsdom's no-op ResizeObserver stub makes it inert in
  // tests. Batch-driven scrolling stays owned by the layout effect above.
  useEffect(() => {
    // The scroller only renders for a loaded room with visible rows; the
    // `roomLoaded`/`hasRows` gates re-run the effect when it (conditionally
    // rendered) mounts for a freshly loaded room, so the observer always
    // attaches to the live element.
    if (!phone || !roomLoaded || !hasRows) {
      return;
    }
    const el = scrollRef.current;
    if (el === null || typeof ResizeObserver === "undefined") {
      return;
    }
    let pinned = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
    const trackPin = () => {
      pinned = el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_BOTTOM_PX;
    };
    el.addEventListener("scroll", trackPin);
    const observer = new ResizeObserver(() => {
      if (pinned) {
        el.scrollTop = el.scrollHeight;
        trackPin();
      }
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      el.removeEventListener("scroll", trackPin);
    };
  }, [phone, roomLoaded, hasRows]);

  // Held sends for this Chat (Story 8.3), oldest-first — a pure mirror of the Rust
  // `outbox` stream. Rendered as amber "Held" bubbles at the timeline tail, distinct
  // from SDK local echoes; a row disappears when the scheduler dispatches it (the SDK
  // "Sending…" echo then takes over) or the user undoes it.
  const heldSends = useHeldSends(accountId ?? "", roomId ?? "");

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
      if (accountId === null || roomId === null) {
        return;
      }
      // Route the dispatch by the pending context: reply / edit / plain text
      // (all through the single Rust send gate).
      const current = composer.getState().pending;
      if (current?.mode === "reply") {
        await sendReply(accountId, roomId, current.targetKey, body);
      } else if (current?.mode === "edit") {
        await editMessage(accountId, roomId, current.targetKey, body);
      } else {
        await sendText(accountId, roomId, body);
      }
      // Clear only the context we just dispatched: if the user started a *new*
      // reply/edit during the in-flight enqueue, it must survive.
      const afterSend = composer.getState().pending;
      if (
        current !== null &&
        afterSend?.mode === current.mode &&
        afterSend?.targetKey === current.targetKey
      ) {
        composer.getState().clear();
      }
    },
    [accountId, roomId, composer],
  );

  // Emit the account's typing notice (Story 3.9). Best-effort: swallow rejections so
  // a typing dispatch is never an unhandled promise or a UI error.
  const onTyping = useCallback(
    (typing: boolean) => {
      if (accountId === null || roomId === null) {
        return;
      }
      setTyping(accountId, roomId, typing).catch(() => {});
    },
    [accountId, roomId],
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
    if (accountId === null || roomId === null || paginatingRef.current) {
      return;
    }
    paginatingRef.current = true;
    paginateBackwards(accountId, roomId, PAGINATE_BATCH)
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
  }, [accountId, roomId]);

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
      if (accountId === null || roomId === null) {
        return;
      }
      // A failed retry (e.g. the echo reconciled away → `EchoNotFound`) leaves
      // the persistent `Failed — Retry` caption in place, inviting another
      // attempt; swallow the rejection so it is never an unhandled promise.
      retrySend(accountId, roomId, key).catch(() => {});
    },
    [accountId, roomId],
  );

  // Dispatch each pending attachment through the single Rust send gate (Story
  // 3.7): a path attachment via `sendAttachmentPath` (Rust reads the file), a
  // pasted-bytes attachment via `sendAttachmentBytes` (raw binary IPC body). The
  // caption (single-attachment only) rides on the first dispatch. Rejects if any
  // enqueue fails so the composer keeps the tray for retry.
  const onSendAttachments = useCallback(
    async (toSend: PendingAttachment[], caption?: string) => {
      if (accountId === null || roomId === null) {
        return;
      }
      for (const attachment of toSend) {
        if (attachment.kind === "path") {
          await sendAttachmentPath(accountId, roomId, attachment.path, caption);
        } else {
          await sendAttachmentBytes(
            accountId,
            roomId,
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
        tray.getState().remove(attachment.id);
      }
    },
    [accountId, roomId, tray],
  );

  // Cancel an in-flight outgoing media echo by aborting its queued send (Story
  // 3.7). Best-effort: if it already dispatched, the abort is a no-op and the
  // message stays sent. A rejection (e.g. the echo reconciled away) is swallowed
  // so it is never an unhandled promise.
  const onCancelSend = useCallback(
    (key: string) => {
      if (accountId === null || roomId === null) {
        return;
      }
      cancelSend(accountId, roomId, key).catch(() => {});
    },
    [accountId, roomId],
  );

  const onReply = useCallback(
    (key: string) => {
      const target = items.find((it) => it.kind === "message" && it.key === key);
      if (target?.kind !== "message") {
        return;
      }
      composer.getState().startReply({
        targetKey: key,
        sender: target.senderDisplayName ?? target.sender,
        bodyPreview: previewOf(target.body),
      });
    },
    [items, composer],
  );

  const onEdit = useCallback(
    (key: string) => {
      const target = items.find((it) => it.kind === "message" && it.key === key);
      // Only own text messages are editable (Rust also gates on `is_editable()`).
      if (target?.kind !== "message" || !target.isOwn) {
        return;
      }
      composer.getState().startEdit({ targetKey: key, body: target.body }, "");
    },
    [items, composer],
  );

  // Resolve a Delete intent on a HELD (still-in-window) bubble as an UNDO, never a
  // redaction (Story 8.4). Held render keys are `held:<id>`. Returns whether the key was
  // a held key — a `held:` key is ALWAYS consumed so callers return before any redaction
  // path (a held id must never reach `deleteMessage`). The effect is the shared
  // `undoHeldSend`, byte-identical to the undo-send pill's Undo.
  const tryUndoHeld = useCallback(
    (key: string): boolean => {
      // Not a held bubble: let the caller fall through to the redaction path.
      if (!key.startsWith("held:")) {
        return false;
      }
      // The undo only fires when the row is still live in the snapshot and the room
      // context is resolved; a stale held selection (its window already elapsed and the
      // row dispatched) cleanly no-ops here instead of leaking to the redaction path or
      // to the browser's native key handling. Either way a `held:` key is consumed.
      if (accountId !== null && roomId !== null) {
        const id = key.slice("held:".length);
        if (heldSends.some((h) => h.id === id)) {
          void undoHeldSend(accountId, roomId, id, composer);
        }
      }
      return true;
    },
    [heldSends, accountId, roomId, composer],
  );

  // Open the delete-for-everyone confirmation for an own message (Story 3.8,
  // FR-15). Fired by the action-bar Delete button and the ⌫/Delete key. A held-row
  // key (`held:<id>`, Story 8.4) resolves as an undo BEFORE any redaction path — a held
  // id can never reach the redaction dialog. Otherwise only own messages are deletable
  // (Rust also gates redaction dispatch); a non-own or missing target is a no-op. The
  // actual redaction runs from the dialog's confirm.
  const onDelete = useCallback(
    (key: string) => {
      // A held bubble's Delete is an undo, not a redaction — branch first and return.
      if (tryUndoHeld(key)) {
        return;
      }
      const target = items.find((it) => it.kind === "message" && it.key === key);
      // Delete-for-everyone is scoped to an own message that has actually been sent;
      // an unsent/failed echo (`sendState !== null`) has no remote event to redact.
      if (target?.kind !== "message" || !target.isOwn || target.sendState !== null) {
        return;
      }
      setDeleteKey(key);
    },
    [items, tryUndoHeld],
  );

  // Toggle an emoji reaction on a message (Story 3.5, FR-12). Fired by both the
  // action-bar Popover pick and a click on an existing pill. Reactions are
  // stateless on the frontend: fire the IPC and let the diff stream re-render the
  // pills. A rejection (e.g. the target reconciled away → `TargetNotFound`) is
  // swallowed so it is never an unhandled promise.
  const onToggleReaction = useCallback(
    (key: string, emoji: string) => {
      if (accountId === null || roomId === null) {
        return;
      }
      toggleReaction(accountId, roomId, key, emoji).catch(() => {});
    },
    [accountId, roomId],
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

  const onCancelPending = useCallback(() => composer.getState().cancel(), [composer]);

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
      roomId === null ||
      focusEvent.accountId !== accountId ||
      focusEvent.roomId !== roomId ||
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
    const targetRoom = roomId;
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
  }, [focusEvent, accountId, roomId, loaded, jumpToKey]);

  // Drop the deep-link note whenever the open room changes (a new Chat starts clean).
  // biome-ignore lint/correctness/useExhaustiveDependencies: reset keyed on the room pair, not the note value
  useEffect(() => {
    setDeepLinkNote(null);
  }, [accountId, roomId]);

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
        // Only consume Escape when there is pending composer context or a selected
        // message to clear; otherwise let it bubble so the phone stack shell can pop
        // a level (UX-DR28, Story 13.2). preventDefault is a no-op on desktop
        // (nothing else reads it) and drives the shell's `defaultPrevented` guard.
        const { pending, selectedKey } = composer.getState();
        if (pending !== null || selectedKey !== null) {
          composer.getState().clear();
          composer.getState().clearSelection();
          e.preventDefault();
        }
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
        const cur = composer.getState().selectedKey;
        const idx = cur === null ? -1 : messageKeys.indexOf(cur);
        const nextIdx =
          e.key === "ArrowUp"
            ? Math.max(0, (idx === -1 ? messageKeys.length : idx) - 1)
            : Math.min(messageKeys.length - 1, idx + 1);
        composer.getState().select(messageKeys[nextIdx]);
        return;
      }

      const sel = composer.getState().selectedKey;
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
        // A held bubble (`held:<id>`, Story 8.4) selected: ⌫ resolves as an UNDO, never a
        // redaction — branch first so a held id never reaches the redaction dialog.
        if (tryUndoHeld(sel)) {
          e.preventDefault();
          return;
        }
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
    [items, onReply, onEdit, onDelete, tryUndoHeld, composer],
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
    <div ref={rootRef} className="contents">
      {roomId !== null && agentHeader !== null && (
        <AgentRoomHeader header={agentHeader} scopeControl={scopeControl?.(agentHeader)} />
      )}
      {roomId === null ? (
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
          // `overscroll-contain` (Story 13.5, phone-gated): the timeline's scroll
          // never chains into the page, so a keyboard open/dismiss or an
          // over-scroll flick leaves no stranded body offset behind the fixed
          // phone shell. Gated on the phone tier so desktop stays byte-for-byte.
          className={cn(
            "flex min-h-0 flex-1 flex-col overflow-y-auto",
            phone && "overscroll-contain",
          )}
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
              ) : row.kind === "approval" ? (
                <li key={row.item.key}>
                  <ApprovalRequest
                    approval={row.approval}
                    accountId={accountId ?? ""}
                    roomId={roomId}
                  />
                </li>
              ) : (
                <li key={row.item.key}>
                  <MessageBubble
                    item={row.item}
                    accountId={accountId ?? undefined}
                    roomId={roomId ?? undefined}
                    grouped={row.grouped}
                    groupTail={row.groupTail}
                    onRetry={onRetry}
                    offline={offline}
                    reducedCapability={reducedCapability}
                    onReply={onReply}
                    onEdit={onEdit}
                    onDelete={onDelete}
                    onJumpTo={onJumpTo}
                    selected={selectedKey === row.item.key}
                    onToggleReaction={onToggleReaction}
                    onOpenPreview={onOpenPreview}
                    onCancelSend={onCancelSend}
                    growing={caretKey === row.item.key}
                  />
                </li>
              ),
            )}
            {/* Held sends (Story 8.3): amber "Held" bubbles at the timeline tail, one
                per held send, oldest-first. Rendered from the outbox VM (never SDK
                timeline items) so the SDK send-state mapping stays honest. Each bubble is
                selectable (selected key `held:<id>`) so `⌫` can target it, and carries a
                Delete affordance (Story 8.4) — both resolve as an UNDO (`undoHeldSend`),
                never a redaction, because a held message has no remote event to redact. */}
            {heldSends.map((row) => {
              const heldKey = `held:${row.id}`;
              return (
                <li key={heldKey} data-testid="held-bubble">
                  <div className="group flex items-center justify-end gap-2 px-3 py-0.5">
                    <button
                      type="button"
                      // Hover-revealed on a pointer that can hover; a coarse
                      // pointer never hovers, so `pointer-coarse:` puts it on
                      // screen there (Story 66.1, AD-197) — the same fallback
                      // the account row's menu wears (`account-footer.tsx`).
                      className="rounded-full border border-held/40 px-2 py-0.5 text-held text-xs opacity-0 transition-opacity focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-held group-hover:opacity-100 pointer-coarse:opacity-100"
                      onClick={() => onDelete(heldKey)}
                      data-testid="held-delete-button"
                      // Honest naming: this Delete undoes an unsent hold (text returns to
                      // the composer), unlike the destructive redaction Delete on a sent
                      // message.
                      aria-label="Discard held message and return it to the composer"
                    >
                      Delete
                    </button>
                    <button
                      type="button"
                      onClick={() => composer.getState().select(heldKey)}
                      className={cn(
                        "max-w-[75%] rounded-[14px] border border-held/40 bg-held/10 px-3 py-2 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-held",
                        selectedKey === heldKey && "ring-2 ring-held",
                      )}
                      aria-pressed={selectedKey === heldKey}
                    >
                      {/* Block <span> (not <p>) so the bubble stays valid phrasing
                          content inside a <button>. */}
                      <span className="block whitespace-pre-wrap break-words text-sm">
                        {row.body}
                      </span>
                      <span className="mt-0.5 block text-held text-xs">Held</span>
                    </button>
                  </div>
                </li>
              );
            })}
          </ol>
        </div>
      )}
      {roomId !== null && (
        <div
          data-testid="composer-footer"
          // Phone (Story 13.5): the footer bottom-insets by the live keyboard
          // height plus the home-indicator safe area, floating the composer (and
          // the undo-send pill / typing indicator stacked above it) over the
          // on-screen keyboard. Both vars are 0px when idle/off-phone; desktop
          // carries no inset class at all.
          className={cn(
            "shrink-0 border-border border-t",
            phone && "pb-[calc(var(--kb-inset,0px)_+_var(--safe-bottom))]",
          )}
        >
          <div className="mx-auto w-full max-w-[720px] px-4 py-3">
            {/* Honest, non-blocking search deep-link fallback (Story 5.4): shown when
                the matched message is further back than the loaded window reaches
                (archive-first seek-to-event is Story 5.6). Never a wrong jump. */}
            {deepLinkNote !== null && (
              <p role="status" className="mb-2 text-xs text-muted-foreground">
                {deepLinkNote}
              </p>
            )}
            {/* Undo-Send pill(s) (Story 8.3): floating above the composer, one per
                held send in this Chat, stacked oldest-first with a countdown ring.
                Renders nothing when there are no held sends. */}
            {accountId !== null && (
              <UndoSendPill accountId={accountId} roomId={roomId} scope={rootRef} />
            )}
            {/* Typing indicator (Story 3.9): "<name> is typing…" between the
                timeline and composer; renders an empty live region when idle. */}
            <TypingIndicator typists={typists} />
            <Composer
              // Key on the full (account, room) identity, not roomId alone: a draft is
              // keyed by (accountId, roomId) and roomId is not unique across accounts
              // (rooms come from different accounts — see rooms.ts). Keying on roomId
              // only would keep the same Composer instance mounted when switching to a
              // same-roomId chat under another account, leaking one account's draft into
              // the other. The composite key forces a remount + fresh restore (Story 7.1).
              key={`${accountId ?? ""}:${roomId}`}
              accountId={accountId ?? ""}
              roomId={roomId}
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
      {accountId !== null && roomId !== null && (
        <DeleteMessageDialog
          accountId={accountId}
          roomId={roomId}
          itemKey={deleteKey}
          onClose={() => setDeleteKey(null)}
        />
      )}
    </div>
  );
}
