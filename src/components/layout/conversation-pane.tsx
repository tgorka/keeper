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
import { PanelRight } from "lucide-react";
import { type KeyboardEvent, type Ref, useCallback, useEffect, useRef, useState } from "react";
import { Composer } from "@/components/chat/composer";
import { DeleteMessageDialog } from "@/components/chat/delete-message-dialog";
import { MediaPreviewOverlay } from "@/components/chat/media-preview-overlay";
import { MessageBubble, type MessageVm } from "@/components/chat/message-bubble";
import { RedactedStub } from "@/components/chat/redacted-stub";
import { UtdStub } from "@/components/chat/utd-stub";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import type { TimelineBatch, TimelineItemVm } from "@/lib/ipc/client";
import {
  cancelSend,
  editMessage,
  retrySend,
  sendAttachmentBytes,
  sendAttachmentPath,
  sendReply,
  sendText,
  subscribeTimeline,
  toggleReaction,
  unsubscribeTimeline,
} from "@/lib/ipc/client";
import { useAccountStatus } from "@/lib/stores/account-status";
import { attachmentId, attachmentsStore, type PendingAttachment } from "@/lib/stores/attachments";
import { composerStore, useComposerStore } from "@/lib/stores/composer";
import { useRoomsStore } from "@/lib/stores/rooms";
import { timelineStore, useTimelineStore } from "@/lib/stores/timeline";

/** Trim a body to a short single-line preview for the reply banner/quote. */
function previewOf(body: string): string {
  const collapsed = body.replace(/\s+/g, " ").trim();
  return collapsed.length > 120 ? `${collapsed.slice(0, 120)}…` : collapsed;
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

export function ConversationPane({ detailOpen, onToggleDetail, toggleRef }: ConversationPaneProps) {
  const selected = useRoomsStore((s) => s.selected);
  const accountId = selected?.accountId ?? null;
  const selectedRoomId = selected?.roomId ?? null;
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
  const scrollRef = useRef<HTMLDivElement>(null);

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
      return;
    }

    setErrored(false);
    setLoaded(false);
    setPreviewKey(null);
    setDeleteKey(null);
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

  const onJumpTo = useCallback((key: string) => {
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-msg-key="${CSS.escape(key)}"]`);
    if (!el) {
      return;
    }
    el.scrollIntoView({ block: "center", behavior: "smooth" });
    // Brief highlight so the jump target is obvious.
    el.classList.add("ring-2", "ring-ring", "ring-offset-1", "ring-offset-background");
    window.setTimeout(() => {
      el.classList.remove("ring-2", "ring-ring", "ring-offset-1", "ring-offset-background");
    }, 1200);
  }, []);

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
        // biome-ignore lint/a11y/noStaticElementInteractions: message-list keyboard affordances (↑/↓/r/e) live on the scroll region; individual actions have their own labeled buttons.
        <div
          ref={scrollRef}
          className="flex min-h-0 flex-1 flex-col overflow-y-auto"
          onKeyDown={onKeyDown}
        >
          <ol
            aria-label="Messages"
            className="mx-auto mt-auto flex w-full max-w-[720px] flex-col px-4 py-4"
          >
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
            <Composer
              key={selectedRoomId}
              onSend={onSend}
              onSendAttachments={onSendAttachments}
              disabled={!roomLoaded}
              pending={pending}
              editPrefill={editPrefill}
              onCancelPending={onCancelPending}
              onEmptyArrowUp={onComposerArrowUp}
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
