/**
 * Message composer (FR-9, UX-DR5; reply/edit context — Story 3.4, FR-10/FR-11).
 *
 * A controlled {@link Textarea} that autogrows to eight lines then scrolls, with
 * a send {@link Button}. Enter sends; ⇧Enter inserts a newline; a whitespace-only
 * body never dispatches. The draft lives in local `useState` (no IPC round-trip
 * on keystroke, so input stays under one frame) and is cleared on a successful
 * send. This component owns no IPC knowledge for the send path — the parent wires
 * `onSend` (which routes to reply / edit / text based on `pending`).
 *
 * The draft is **durable** per `(accountId, roomId)` (Story 7.1, AD-15): on mount it
 * is restored from `keeper.db` (unless entering edit mode, whose prefill wins), and
 * each keystroke schedules a ~200 ms debounced, fire-and-forget `saveDraft`
 * (trimmed-empty → `clearDraft`) plus a `draftsStore` marker update — never a
 * synchronous IPC write on the keystroke path. The pending save is flushed on unmount
 * (room switch), and the row + marker are cleared on a successful send.
 *
 * When `pending` is set, a context banner renders above the textarea (the quoted
 * sender/preview for a reply, "Editing your message" for an edit) with a cancel
 * (×) control. `Esc` cancels the pending context: a reply keeps the typed draft; an
 * edit restores the pre-edit stashed draft (both "cancel without losing composer
 * text"). Entering edit prefills the textarea with the message body (`editPrefill`).
 */
import { open } from "@tauri-apps/plugin-dialog";
import { Paperclip, Plus, X } from "lucide-react";
import {
  type ClipboardEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { useShellLayout } from "@/hooks/use-shell-layout";
import {
  clearDraft,
  clearDraftMirror,
  loadDraft,
  loadRemoteDraft,
  mirrorDraft,
  saveDraft,
} from "@/lib/ipc/client";
import {
  attachmentId,
  attachmentsStore,
  type PendingAttachment,
  useAttachmentsStore,
} from "@/lib/stores/attachments";
import type { PendingContext } from "@/lib/stores/composer";
import { composerStore, useComposerStore } from "@/lib/stores/composer";
import { draftsStore, useRemoteDraft } from "@/lib/stores/drafts";
import { useIncognito } from "@/lib/stores/incognito";
import { cn } from "@/lib/utils";

/** Derive a chip display name for a pending attachment (its filename). */
function chipLabel(attachment: PendingAttachment): string {
  return attachment.filename;
}

/** Format a byte count as a short human-readable size (e.g. `1.2 MB`). */
function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** The display filename derived from an OS file path (its basename). */
function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

interface ComposerProps {
  /**
   * The open conversation's owning account id (Story 7.1). Keys the persistent
   * per-chat draft together with {@link roomId}.
   */
  accountId: string;
  /**
   * The open conversation's room id (Story 7.1). Keys the persistent per-chat draft
   * together with {@link accountId}.
   */
  roomId: string;
  /**
   * Dispatch the trimmed body. Resolves on success (the draft then clears);
   * rejects if the send could not be enqueued (the draft is kept so the user can
   * retry). The parent routes this to `sendReply` / `editMessage` / `sendText`
   * based on the current `pending`.
   */
  onSend: (body: string) => Promise<void>;
  /**
   * Dispatch the pending attachments (Story 3.7). `caption` is the trimmed
   * composer text, passed only when exactly one attachment is pending (otherwise
   * `undefined` — a caption maps to a single media event). The parent routes each
   * attachment to `sendAttachmentPath` / `sendAttachmentBytes`. Resolves when all
   * are enqueued (the tray + draft then clear); rejects to keep the tray so the
   * user can retry. Absent → the attach/paste affordances are inert.
   */
  onSendAttachments?: (attachments: PendingAttachment[], caption?: string) => Promise<void>;
  /** When `true`, the composer is inert (no room loaded). */
  disabled?: boolean;
  /** The active reply/edit context, or `null`. Drives the banner + Esc routing. */
  pending?: PendingContext | null;
  /**
   * The message body to prefill the textarea with when entering **edit** mode
   * (`null` outside edit). Applied once per edit target.
   */
  editPrefill?: string | null;
  /**
   * Cancel the pending context (Esc / banner ×). Returns the draft the composer
   * should restore (the stashed pre-edit draft for an edit) or `null` for a reply
   * (whose typed draft is kept). The parent wires this to the composer store's
   * `cancel`.
   */
  onCancelPending?: () => string | null;
  /**
   * `↑` pressed in an empty composer with no pending context (caret at start):
   * the parent opens edit on the last own message (Story 3.4 / epic affordance).
   */
  onEmptyArrowUp?: () => void;
  /**
   * Emit (or clear) the account's typing notice (Story 3.9, typing). Called
   * `true` when the user is actively typing (throttled here to ≤1/3s) and `false`
   * on send / clear / blur / ~5 s idle. Best-effort — the parent swallows any
   * dispatch failure. Absent → no typing is emitted.
   */
  onTyping?: (typing: boolean) => void;
}

/** Minimum interval between `setTyping(true)` emits while typing (≤1/3 s). */
const TYPING_THROTTLE_MS = 3000;
/** Idle timeout after the last keystroke before emitting `setTyping(false)`. */
const TYPING_IDLE_MS = 5000;

/** Debounce before a keystroke persists the draft (fire-and-forget, Story 7.1). */
const DRAFT_SAVE_DEBOUNCE_MS = 200;

/**
 * Debounce before a draft is mirrored cross-device (Story 7.2, AD-15). Deliberately
 * looser than the local save so bursts of typing coalesce into few homeserver writes;
 * mirroring runs off the keystroke path and is best-effort.
 */
const DRAFT_MIRROR_DEBOUNCE_MS = 1000;

export function Composer({
  accountId,
  roomId,
  onSend,
  onSendAttachments,
  disabled = false,
  pending = null,
  editPrefill = null,
  onCancelPending,
  onEmptyArrowUp,
  onTyping,
}: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState(false);
  // Phone-tier composer deltas (Story 13.5, UX-DR25): ≥44pt send/attach targets,
  // a 5-line autogrow cap, and Enter→newline (send is button-only on phone —
  // WKWebView cannot reliably distinguish a hardware keyboard, so the touch
  // default wins). Everything is tier-gated on the shared layout hook — never
  // user-agent sniffing, never a forked composer.
  const { phone } = useShellLayout();
  // The remote (cross-device) draft body currently offered as a local-wins conflict
  // chip (Story 7.2), or `null` when nothing is offered. Set when a differing remote
  // draft arrives against non-empty local text; cleared on adopt / when it stops
  // differing. Local text is never overwritten without the user tapping "Use that
  // version".
  const [remoteOffer, setRemoteOffer] = useState<string | null>(null);

  // Whether Incognito is effective for this chat (Story 8.1). Mirrored from the
  // Rust-resolved VM; drives the violet composer focus ring while it applies.
  const incognitoEffective = useIncognito(accountId, roomId)?.effective ?? false;

  // The textarea handle, focused programmatically when the composer store's focus
  // nonce is *bumped* (Story 6.6 — e.g. after a new chat is resolved and opened).
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const focusNonce = useComposerStore((s) => s.focusNonce);
  // Seed to the current nonce so a fresh Composer mount (every room switch clears &
  // remounts the pane) does NOT self-focus off a stale, already-bumped nonce — only a
  // genuine change after mount steals focus into the composer.
  const seenFocusNonce = useRef(focusNonce);
  useEffect(() => {
    if (focusNonce !== seenFocusNonce.current) {
      seenFocusNonce.current = focusNonce;
      textareaRef.current?.focus();
    }
  }, [focusNonce]);

  // Typing-notice emission (Story 3.9): mirror the callback + local typing state
  // in refs so the throttle/idle timers don't re-run effects or capture stale
  // closures. `typingActive` tracks whether we've announced typing (so we emit
  // `false` exactly once on stop), `lastTypingEmit` throttles the `true` emits.
  const onTypingRef = useRef(onTyping);
  onTypingRef.current = onTyping;
  const typingActive = useRef(false);
  const lastTypingEmit = useRef(0);
  const idleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  /** Announce typing (throttled ≤1/3 s) and arm the ~5 s idle-stop timer. */
  const startTyping = useCallback(() => {
    const now = Date.now();
    if (!typingActive.current || now - lastTypingEmit.current >= TYPING_THROTTLE_MS) {
      typingActive.current = true;
      lastTypingEmit.current = now;
      onTypingRef.current?.(true);
    }
    if (idleTimer.current !== null) {
      clearTimeout(idleTimer.current);
    }
    idleTimer.current = setTimeout(() => {
      idleTimer.current = null;
      if (typingActive.current) {
        typingActive.current = false;
        onTypingRef.current?.(false);
      }
    }, TYPING_IDLE_MS);
  }, []);

  /** Stop typing immediately (send / clear / blur), emitting `false` once. */
  const stopTyping = useCallback(() => {
    if (idleTimer.current !== null) {
      clearTimeout(idleTimer.current);
      idleTimer.current = null;
    }
    if (typingActive.current) {
      typingActive.current = false;
      onTypingRef.current?.(false);
    }
  }, []);

  // Clear typing on unmount / room change (the composer is keyed by room), so a
  // lingering "typing" is never left announced after the user leaves.
  useEffect(() => stopTyping, [stopTyping]);

  // Persistent per-chat draft (Story 7.1, AD-15). The composer is keyed by room in the
  // parent, so mount == open-a-chat and unmount == leave-it. The account/room ids are
  // mirrored in refs so the debounce timer and unmount flush read the latest without
  // re-arming per keystroke. `pendingDraft` holds the body a debounced save will
  // persist; `null` means nothing is queued.
  const accountIdRef = useRef(accountId);
  accountIdRef.current = accountId;
  const roomIdRef = useRef(roomId);
  roomIdRef.current = roomId;
  const draftSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingDraft = useRef<string | null>(null);
  // Cross-device mirror (Story 7.2, AD-15) runs on its own looser debounce, off the
  // keystroke path. `pendingMirror` holds the body a debounced mirror will write;
  // `null` means nothing is queued. Best-effort — never blocks or fails local persistence.
  const draftMirrorTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingMirror = useRef<string | null>(null);
  // The mount restore (below) runs after an async `loadDraft`. If anything establishes
  // the composer's content during that window — the user types, sends, or enters edit
  // (prefill) — a late restore must not clobber it. This latch is set by those paths so
  // the restore bails, instead of relying on a mount-time `pending` snapshot or a
  // momentarily-empty draft (both of which miss the type-then-clear / send-during-load
  // races). (Story 7.1)
  const restoreConsumed = useRef(false);

  /** Mirror the queued draft now (trimmed-empty tombstones). Best-effort, fire-and-forget. */
  const flushMirror = useCallback(() => {
    if (draftMirrorTimer.current !== null) {
      clearTimeout(draftMirrorTimer.current);
      draftMirrorTimer.current = null;
    }
    const body = pendingMirror.current;
    if (body === null) {
      return;
    }
    pendingMirror.current = null;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    const trimmed = body.trim();
    // Best-effort: a mirror failure must never block or fail local persistence — the
    // only symptom is the absent cross-device echo. The Rust core dedupes by body.
    if (trimmed.length > 0) {
      void mirrorDraft(a, r, trimmed).catch(() => {});
    } else {
      void clearDraftMirror(a, r).catch(() => {});
    }
  }, []);

  /** Cross-device clear: tombstone the mirror (best-effort) and drop any queued mirror. */
  const clearMirror = useCallback(() => {
    if (draftMirrorTimer.current !== null) {
      clearTimeout(draftMirrorTimer.current);
      draftMirrorTimer.current = null;
    }
    pendingMirror.current = null;
    void clearDraftMirror(accountIdRef.current, roomIdRef.current).catch(() => {});
  }, []);

  /** Queue `body` for a debounced cross-device mirror write (Story 7.2). */
  const scheduleMirror = useCallback(
    (body: string) => {
      pendingMirror.current = body;
      if (draftMirrorTimer.current !== null) {
        clearTimeout(draftMirrorTimer.current);
      }
      draftMirrorTimer.current = setTimeout(flushMirror, DRAFT_MIRROR_DEBOUNCE_MS);
    },
    [flushMirror],
  );

  /** Persist the queued draft now (trimmed-empty deletes the row). Fire-and-forget. */
  const flushDraft = useCallback(() => {
    if (draftSaveTimer.current !== null) {
      clearTimeout(draftSaveTimer.current);
      draftSaveTimer.current = null;
    }
    const body = pendingDraft.current;
    if (body === null) {
      return;
    }
    pendingDraft.current = null;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    const trimmed = body.trim();
    // Fire-and-forget: a persist failure must never block or surface on the keystroke
    // path — the composer's local state stays the visible truth.
    if (trimmed.length > 0) {
      void saveDraft(a, r, trimmed).catch(() => {});
    } else {
      void clearDraft(a, r).catch(() => {});
    }
    draftsStore.getState().mark(a, r, trimmed.length > 0);
  }, []);

  /**
   * Delete the persisted draft + its marker after a successful send / composer clear
   * (Story 7.1). Cancels any queued debounced save so it can't re-write a row we just
   * deleted. Fire-and-forget — a delete failure never blocks the send path.
   */
  const clearPersistedDraft = useCallback(() => {
    if (draftSaveTimer.current !== null) {
      clearTimeout(draftSaveTimer.current);
      draftSaveTimer.current = null;
    }
    pendingDraft.current = null;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    void clearDraft(a, r).catch(() => {});
    draftsStore.getState().mark(a, r, false);
    // Tombstone the cross-device mirror too (Story 7.2) so other devices stop showing
    // the sent/cleared draft. Best-effort — never blocks the send/clear path.
    clearMirror();
  }, [clearMirror]);

  /** Queue `body` for a ~200 ms debounced persist, updating the marker immediately. */
  const scheduleDraftSave = useCallback(
    (body: string) => {
      pendingDraft.current = body;
      // The inbox marker reflects the live composer state at once (not after the debounce)
      // so the amber pencil never lags a keystroke; the DB write is what is debounced.
      draftsStore.getState().mark(accountIdRef.current, roomIdRef.current, body.trim().length > 0);
      if (draftSaveTimer.current !== null) {
        clearTimeout(draftSaveTimer.current);
      }
      draftSaveTimer.current = setTimeout(flushDraft, DRAFT_SAVE_DEBOUNCE_MS);
      // Also mirror cross-device (Story 7.2) on a looser, separate debounce off the
      // keystroke path — best-effort, never blocks typing or local persistence.
      scheduleMirror(body);
    },
    [flushDraft, scheduleMirror],
  );

  const attachments = useAttachmentsStore((s) => s.pending);
  // The attach/paste affordances are available only when the parent wires the
  // attachment dispatcher and the composer is enabled.
  const attachEnabled = onSendAttachments != null && !disabled;

  // Mirror the live draft in a ref so the prefill effect can stash it without
  // taking `draft` as a dependency (which would re-run the effect every keystroke).
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // Mirror the pending mode in a ref so the remote-reconcile adopt callback can check
  // "am I editing?" without taking `pending` as a dependency (Story 7.2).
  const pendingModeRef = useRef(pending?.mode);
  pendingModeRef.current = pending?.mode;
  // The draft that was in the composer just before entering the current edit,
  // restored verbatim on Esc/cancel so an edit "cancels without losing composer
  // text" (Story 3.4, FR-11). Owned here because the draft lives in local state.
  const preEditDraft = useRef("");

  // Prefill the draft with the target's body when entering edit mode (once per
  // edit target). Keyed on the edit target key so re-entering edit on a different
  // message re-prefills, but typing within one edit is not clobbered. The outgoing
  // draft is stashed first so cancel can restore it.
  const editTargetKey = pending?.mode === "edit" ? pending.targetKey : null;
  const prefilledFor = useRef<string | null>(null);
  useEffect(() => {
    if (editTargetKey !== null && prefilledFor.current !== editTargetKey) {
      prefilledFor.current = editTargetKey;
      preEditDraft.current = draftRef.current;
      setDraft(editPrefill ?? "");
      setError(false);
      // Entering edit establishes the composer text (the edit body); a late draft
      // restore must not overwrite it, even if the prefill body is empty. (Story 7.1)
      restoreConsumed.current = true;
    }
    if (editTargetKey === null) {
      prefilledFor.current = null;
    }
  }, [editTargetKey, editPrefill]);

  /**
   * Adopt a remote draft body into the composer (Story 7.2). The single path by which
   * remote text enters the composer — auto (empty untouched composer) or user-tapped
   * (conflict chip). Establishes the content (latches `restoreConsumed`), persists it
   * locally, and dismisses any offered chip; the mirror is deduped so the adopt→save
   * echo does not storm. A no-op for an empty body.
   */
  const adoptRemote = useCallback(
    (body: string) => {
      // Edit mode owns the composer (the edit body, not this room's persistent draft),
      // so a remote draft must never be adopted into it — that would overwrite the edit
      // and persist/mirror it as the draft (Story 7.2/3.4). Defensive: callers already
      // gate on edit mode.
      if (body.length === 0 || pendingModeRef.current === "edit") {
        return;
      }
      restoreConsumed.current = true;
      setDraft(body);
      setRemoteOffer(null);
      // Persist the adopted body locally so the draft is durable and the marker shows;
      // this also schedules a mirror (deduped by body, so it is effectively a no-op).
      scheduleDraftSave(body);
    },
    [scheduleDraftSave],
  );

  // Undo-Send restore (Story 8.3): when a held send is cancelled, `cancelHeldSend`
  // returns the held body and the pill calls `composerStore.restore(accountId, roomId,
  // body)`, bumping `restoreNonce`. Apply it here, establishing the composer text
  // (latching `restoreConsumed` so a late mount-restore can't clobber it) and persisting
  // it as the durable draft — replacing current composer content per the documented
  // "restored text is the user's most recent intent" trade-off. Seeded to the current
  // nonce so a fresh mount does not self-apply a stale, already-consumed restore. The
  // restore only applies when its target matches this composer's chat, so a restore that
  // resolves after the user switched rooms never lands in the wrong room's composer.
  const restoreNonce = useComposerStore((s) => s.restoreNonce);
  const seenRestoreNonce = useRef(restoreNonce);
  useEffect(() => {
    if (restoreNonce === seenRestoreNonce.current) {
      return;
    }
    seenRestoreNonce.current = restoreNonce;
    const { restoreBody: body, restoreTarget: target } = composerStore.getState();
    if (body === null || pendingModeRef.current === "edit") {
      return;
    }
    if (
      target === null ||
      target.accountId !== accountIdRef.current ||
      target.roomId !== roomIdRef.current
    ) {
      return;
    }
    restoreConsumed.current = true;
    setDraft(body);
    setError(false);
    scheduleDraftSave(body);
  }, [restoreNonce, scheduleDraftSave]);

  // Restore the persisted draft and reconcile the remote mirror once on mount (Story
  // 7.1 + 7.2). The composer remounts per (account, room) in the parent, so this runs
  // on each chat open. Local always wins: a differing remote draft is only ever *offered*
  // via the conflict chip; it replaces the composer only into an empty, untouched one
  // (drafts follow the user). Reads refs only, so `[]` deps are exhaustive; it never
  // overwrites content the user has already established (see `restoreConsumed`). A load
  // failure is swallowed (fall back to local / no chip, never a crash).
  useEffect(() => {
    let cancelled = false;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    void Promise.all([
      loadDraft(a, r).catch(() => null),
      loadRemoteDraft(a, r).catch(() => null),
    ]).then(([local, remote]) => {
      // Bail if unmounted or the composer's content was already established during the
      // async load — the user typed, sent, or entered edit (`restoreConsumed`), any of
      // which local-wins must not clobber.
      if (cancelled || restoreConsumed.current || draftRef.current.length > 0) {
        return;
      }
      const localBody = local ?? "";
      const remoteBody = remote?.body ?? null;
      if (localBody.length > 0) {
        // Restore the local draft (7.1). If a differing remote exists, offer it — local
        // wins, remote is only surfaced for one-tap adoption.
        setDraft(localBody);
        if (remoteBody !== null && remoteBody !== localBody) {
          setRemoteOffer(remoteBody);
        }
      } else if (remoteBody !== null) {
        // Empty, untouched composer with a present remote draft: adopt it (drafts follow
        // the user across devices).
        adoptRemote(remoteBody);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [adoptRemote]);

  // Live remote reconcile (Story 7.2): react to a remote draft arriving/updating via the
  // app-wide mirror subscription while this composer is open. Local always wins — an
  // empty, untouched composer auto-adopts (drafts follow the user); otherwise a differing
  // remote raises the conflict chip for one-tap adoption. A remote that equals the local
  // text (or a tombstone) dismisses the chip. Never overwrites non-empty local text.
  const remoteDraft = useRemoteDraft(accountId, roomId);
  const inEdit = pending?.mode === "edit";
  useEffect(() => {
    if (inEdit) {
      // Edit mode owns the composer (the edit body, not this room's draft): never
      // reconcile a remote draft against it or raise the chip. Withdraw any offer
      // shown before the edit began; it re-reconciles on leaving edit (Story 7.2/3.4).
      setRemoteOffer(null);
      return;
    }
    const remoteBody = remoteDraft?.body ?? null;
    if (remoteBody === null) {
      // Remote cleared / tombstoned: withdraw any offer.
      setRemoteOffer(null);
      return;
    }
    if (!restoreConsumed.current && draftRef.current.length === 0) {
      // Empty and untouched: adopt (auto-follow).
      adoptRemote(remoteBody);
      return;
    }
    if (remoteBody === draftRef.current) {
      // Remote now equals local text: nothing to offer.
      setRemoteOffer(null);
      return;
    }
    setRemoteOffer(remoteBody);
  }, [remoteDraft, adoptRemote, inEdit]);

  // Flush the pending debounced save AND the pending cross-device mirror on unmount
  // (room switch / composer close) so the latest keystroke is durable and mirrored even
  // if it fell inside a debounce window (Story 7.1 + 7.2). The mirror flush is
  // best-effort — it never blocks the room switch.
  useEffect(
    () => () => {
      flushDraft();
      flushMirror();
    },
    [flushDraft, flushMirror],
  );

  const hasAttachments = attachments.length > 0;
  // Send is enabled when there is a trimmed body OR at least one pending
  // attachment (an attachment can be sent with no caption). An edit never carries
  // attachments.
  const canSend =
    (draft.trim().length > 0 || (hasAttachments && pending?.mode !== "edit")) &&
    !disabled &&
    !sending;

  async function send() {
    // Capture the mode before awaiting: an edit sends onto an existing message and
    // must not touch this room's persistent draft, so it restores the pre-edit text
    // instead of clearing (Story 7.1). A reply/text send owns the draft and clears it.
    const wasEdit = pending?.mode === "edit";
    const body = draft.trim();
    const trayAttachments = attachmentsStore.getState().pending;
    const dispatchAttachments =
      onSendAttachments != null && pending?.mode !== "edit" && trayAttachments.length > 0;
    if ((body.length === 0 && !dispatchAttachments) || disabled || sending) {
      // Whitespace-only with no attachment / disabled / in-flight: never dispatch.
      return;
    }
    // A dispatch consumes/replaces the composer content; a late mount restore must not
    // resurrect a just-sent draft into the emptied composer. (Story 7.1)
    restoreConsumed.current = true;
    // Cancel any queued debounced persist before the (possibly slow) send: otherwise
    // a flush landing mid-send could re-`saveDraft` a row we then `clearDraft`, and the
    // two fire-and-forget writes could reorder — leaving an orphan draft + amber marker
    // on an already-sent chat that survives relaunch (Story 7.1).
    if (draftSaveTimer.current !== null) {
      clearTimeout(draftSaveTimer.current);
      draftSaveTimer.current = null;
    }
    pendingDraft.current = null;
    // Same hazard for the cross-device mirror (Story 7.2): a queued debounced mirror
    // landing during a slow `onSend` would write the draft to account data, and then
    // reorder after the post-send tombstone (`clearPersistedDraft → clearMirror`),
    // resurrecting the sent draft on other devices. Cancel it before the send; a
    // failed non-edit send re-schedules the mirror in the catch below.
    if (draftMirrorTimer.current !== null) {
      clearTimeout(draftMirrorTimer.current);
      draftMirrorTimer.current = null;
    }
    pendingMirror.current = null;
    // Sending stops typing (Story 3.9): clear the notice once as the message goes.
    stopTyping();
    setSending(true);
    setError(false);
    try {
      if (dispatchAttachments && onSendAttachments != null) {
        // A caption maps to a single media event, so it rides only when exactly
        // one attachment is pending; with multiple, the text is sent separately.
        const caption = trayAttachments.length === 1 && body.length > 0 ? body : undefined;
        await onSendAttachments(trayAttachments, caption);
        // If the text did not ride as a caption (multiple attachments) but the
        // user typed a body, dispatch it as its own message.
        if (caption === undefined && body.length > 0) {
          await onSend(body);
        }
        // Clear only on success so a failed enqueue keeps the tray + text. Attachments
        // never ride an edit (guarded above), so the draft is always cleared here.
        attachmentsStore.getState().clear();
        setDraft("");
        clearPersistedDraft();
      } else {
        await onSend(body);
        // Clear only on success so a failed enqueue keeps the user's text.
        if (wasEdit) {
          // Editing an existing message leaves the persistent draft untouched: restore
          // the pre-edit composer text (the real draft) and keep the stored row/marker.
          setDraft(preEditDraft.current);
        } else {
          setDraft("");
          clearPersistedDraft();
        }
      }
    } catch {
      // Enqueue-time failure produces no timeline echo to fall back on, so
      // surface an honest inline error (AD-21) and keep the draft/tray so the
      // user can resend. Async delivery failures instead show as the message's
      // Failed send-state caption. Re-persist the retained draft (the queued save was
      // cancelled above) so a failed non-edit send stays durable across relaunch. An
      // edit never touched the stored draft, so there is nothing to re-persist.
      // Read the live draft via `draftRef` (not the `draft` closure captured at send
      // time): if the user retyped during the in-flight send, the composer now shows
      // that newer text, and persisting the stale pre-send body would diverge from it.
      setError(true);
      if (!wasEdit) {
        scheduleDraftSave(draftRef.current);
      }
    } finally {
      setSending(false);
    }
  }

  /** Open the native file picker and add each chosen path to the tray. */
  async function pickFiles() {
    if (!attachEnabled) {
      return;
    }
    try {
      const selection = await open({ multiple: true });
      if (selection == null) {
        // Dialog cancelled → no-op.
        return;
      }
      const paths = Array.isArray(selection) ? selection : [selection];
      attachmentsStore.getState().addMany(
        paths.map((path) => ({
          id: attachmentId(),
          kind: "path" as const,
          path,
          filename: basename(path),
        })),
      );
    } catch {
      // A dialog failure is non-fatal — the user can retry; nothing to surface.
    }
  }

  /**
   * Intercept a paste that carries an image: add it as a raw-bytes attachment
   * (dispatched later as a raw binary IPC body, never base64). A non-image paste
   * falls through to the default text paste unchanged.
   */
  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    if (!attachEnabled) {
      return;
    }
    const imageItem = Array.from(e.clipboardData.items).find((it) => it.type.startsWith("image/"));
    if (!imageItem) {
      // Not an image → let the default text paste proceed.
      return;
    }
    const file = imageItem.getAsFile();
    if (!file) {
      return;
    }
    e.preventDefault();
    void file.arrayBuffer().then((bytes) => {
      const ext = file.type.split("/")[1] || "png";
      attachmentsStore.getState().add({
        id: attachmentId(),
        kind: "bytes",
        bytes,
        filename: file.name && file.name !== "" ? file.name : `pasted-image.${ext}`,
        mime: file.type,
        size: file.size,
      });
    });
  }

  /** Remove a pending attachment (a pre-upload cancel). */
  function removeAttachment(id: string) {
    attachmentsStore.getState().remove(id);
  }

  function cancelPending() {
    const wasEdit = pending?.mode === "edit";
    // Clear the pending context in the store (its return value is unused — this
    // component owns the pre-edit draft it restores).
    onCancelPending?.();
    if (wasEdit) {
      // Edit: restore the draft the user had before entering edit.
      setDraft(preEditDraft.current);
    }
    // Reply: leave the typed draft untouched.
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape" && pending) {
      // Esc cancels the pending reply/edit without losing composer text.
      e.preventDefault();
      cancelPending();
      return;
    }
    if (
      e.key === "ArrowUp" &&
      !pending &&
      draft.length === 0 &&
      e.currentTarget.selectionStart === 0 &&
      onEmptyArrowUp
    ) {
      // ↑ in an empty composer opens edit on the last own message.
      e.preventDefault();
      onEmptyArrowUp();
      return;
    }
    // Desktop: Enter sends; ⇧Enter (or any modifier) inserts a newline. Phone
    // (Story 13.5): the on-screen return key inserts a newline — the send branch
    // is skipped entirely, making the ≥44pt send button the sole send path (the
    // FR-41 approval trigger). The IME `isComposing` guard is shared: a
    // composing Enter never sends on any tier.
    if (!phone && e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  return (
    <div className="flex flex-col gap-1">
      {pending && (
        <div className="flex items-center gap-2 rounded-md border border-border bg-muted/50 px-3 py-1.5">
          <div className="min-w-0 flex-1">
            {pending.mode === "reply" ? (
              <>
                <span className="block font-medium text-muted-foreground text-xs">
                  Replying to {pending.sender}
                </span>
                <span className="block truncate text-foreground text-xs">
                  {pending.bodyPreview}
                </span>
              </>
            ) : (
              <span className="block font-medium text-muted-foreground text-xs">
                Editing your message
              </span>
            )}
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label="Cancel"
            onClick={cancelPending}
          >
            ×
          </Button>
        </div>
      )}
      {/* Local-wins conflict chip (Story 7.2, AD-15): a differing remote draft is
          offered for one-tap adoption. Local text stays put until the user taps —
          adoption is the only way remote text enters a non-empty composer. Never
          shown while editing an existing message (the composer is then the edit body). */}
      {remoteOffer !== null && pending?.mode !== "edit" && (
        <div className="flex items-center gap-2 rounded-md border border-border bg-muted/50 px-3 py-1.5">
          <span className="min-w-0 flex-1 truncate text-muted-foreground text-xs">
            Edited on another device
          </span>
          <Button type="button" variant="ghost" size="xs" onClick={() => adoptRemote(remoteOffer)}>
            Use that version
          </Button>
        </div>
      )}
      {/* Pending-attachment tray (Story 3.7): removable chips above the textarea,
          each showing the filename (+ size for pasted bytes). Removing a chip is a
          pre-upload cancel. */}
      {hasAttachments && pending?.mode !== "edit" && (
        <ul aria-label="Pending attachments" className="flex flex-wrap gap-1.5">
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex items-center gap-1.5 rounded-md border border-border bg-muted/50 py-1 pr-1 pl-2"
            >
              <span className="max-w-[180px] truncate text-xs" title={chipLabel(attachment)}>
                {chipLabel(attachment)}
              </span>
              {attachment.kind === "bytes" && (
                <span className="text-muted-foreground text-xs">{formatSize(attachment.size)}</span>
              )}
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Remove ${chipLabel(attachment)}`}
                onClick={() => removeAttachment(attachment.id)}
              >
                <X aria-hidden="true" className="size-3" />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-end gap-2">
        {attachEnabled && pending?.mode !== "edit" && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label="Attach file"
            disabled={disabled}
            onClick={() => void pickFiles()}
            // Phone (Story 13.5): the attach affordance is a ≥44pt `+`
            // presenting the same native picker; desktop keeps the 36px paperclip.
            className={cn(phone && "size-11")}
          >
            {phone ? <Plus aria-hidden="true" /> : <Paperclip aria-hidden="true" />}
          </Button>
        )}
        <Textarea
          ref={textareaRef}
          aria-label="Message"
          placeholder="Write a message…"
          value={draft}
          disabled={disabled}
          onChange={(e) => {
            const next = e.target.value;
            setDraft(next);
            // The user has typed into this composer; a late mount restore must not
            // clobber their input (even after they clear it back to empty). (Story 7.1)
            restoreConsumed.current = true;
            if (error) {
              setError(false);
            }
            // Reconcile the conflict chip against the typed text (Story 7.2): dismiss it
            // once the local text matches the offered remote (nothing to adopt).
            if (remoteOffer !== null && remoteOffer === next) {
              setRemoteOffer(null);
            }
            // Persist the draft (Story 7.1): debounced, fire-and-forget so the keystroke
            // path never blocks on IPC; also updates the inbox marker at once. NOT while
            // editing an existing message — the composer text is then the edit body, not
            // this room's persistent draft, so it must never overwrite the stored draft.
            if (pending?.mode !== "edit") {
              scheduleDraftSave(next);
            }
            // Typing-notice (Story 3.9): a non-empty edit announces typing
            // (throttled); clearing to empty stops it. An edit-mode composer still
            // emits typing — the peer sees the user is composing regardless.
            if (next.trim().length > 0) {
              startTyping();
            } else {
              stopTyping();
            }
          }}
          onKeyDown={onKeyDown}
          onBlur={stopTyping}
          onPaste={onPaste}
          rows={1}
          // Autogrow via `field-sizing-content` (from the shadcn base) capped at
          // eight lines (five on the phone tier, Story 13.5), then scroll. While
          // Incognito is effective for this chat (Story 8.1), tint the focus ring
          // violet with the reserved `--incognito` token, overriding the base
          // ring/border color.
          className={cn(
            "min-h-9 resize-none",
            phone ? "max-h-[calc(5*1.5rem+1rem)]" : "max-h-[calc(8*1.5rem+1rem)]",
            incognitoEffective && "focus-visible:border-incognito focus-visible:ring-incognito/50",
          )}
          data-incognito={incognitoEffective ? "true" : undefined}
        />
        <Button
          type="button"
          onClick={() => void send()}
          disabled={!canSend}
          aria-label={pending?.mode === "edit" ? "Save edit" : "Send message"}
          // Phone (Story 13.5): a ≥44pt primary-tinted hit target — the sole send
          // path on touch (tap = the FR-41 approval trigger). Desktop keeps the
          // default button size.
          className={cn(phone && "h-11 min-w-11")}
        >
          {pending?.mode === "edit" ? "Save" : "Send"}
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-destructive text-xs">
          Couldn't send. Check your connection and try again.
        </p>
      )}
    </div>
  );
}
