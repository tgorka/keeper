/**
 * Message composer (FR-9, UX-DR5; reply/edit context — Story 3.4, FR-10/FR-11).
 *
 * A controlled {@link Textarea} that autogrows to eight lines then scrolls, with
 * a send {@link Button}. Enter sends; ⇧Enter inserts a newline; a whitespace-only
 * body never dispatches. The draft lives in local `useState` (no IPC round-trip
 * on keystroke, so input stays under one frame) and is cleared on a successful
 * send. The parent wires `onSend` (which routes to reply / edit / text based on
 * `pending`); the only IPC detail the composer reads back is the rejected
 * {@link IpcError}'s `retriable` flag, which decides whether the inline failure copy
 * may honestly suggest retrying.
 *
 * The draft is **durable** per `(accountId, roomId)` (Story 7.1, AD-15): on mount it
 * is restored from `keeper.db` (unless entering edit mode, whose prefill wins), and
 * each keystroke schedules a ~200 ms debounced, fire-and-forget `saveDraft`
 * (trimmed-empty → `clearDraft`) plus a `draftsStore` marker update — never a
 * synchronous IPC write on the keystroke path. The pending save is flushed on unmount
 * (room switch), and the row + marker are cleared on a successful reply/text send. An
 * edit-send instead re-queues the restored pre-edit draft, so the stored row keeps
 * tracking the composer even when the edit cancelled a still-debounced keystroke.
 *
 * When `pending` is set, a context banner renders above the textarea (the quoted
 * sender/preview for a reply, "Editing your message" for an edit) with a cancel
 * (×) control. `Esc` cancels the pending context: a reply keeps the typed draft; an
 * edit restores the pre-edit stashed draft (both "cancel without losing composer
 * text"). Entering edit prefills the textarea with the message body (`editPrefill`).
 *
 * A leading `/` opens the slash menu (Story 100.5, R385) — the shared
 * {@link SlashMenuList}, anchored above the field. What a `/` line means is
 * `keeper_core::chat::commands`' answer through {@link chatCommandPreview}: text,
 * an action (`/me`), an act on the room, or a refusal shown here with the draft
 * kept. A line with no leading slash is sent without asking, so prose costs no
 * round trip. Commands are off while editing (the text is that message's body)
 * and while attachments are pending (the text is their caption).
 */
import { type DialogFilter, open } from "@tauri-apps/plugin-dialog";
import { Camera, Image, Paperclip, Plus, X } from "lucide-react";
import {
  type ClipboardEvent,
  type KeyboardEvent,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { CameraSheet, photoFilename } from "@/components/chat/camera-sheet";
import type { ChatCommandInvocation, ChatCommandOutcome } from "@/components/chat/chat-commands";
import { InlineSlashList } from "@/components/slash-menu/inline-slash-list";
import { isCommandLine, type SlashRow } from "@/components/slash-menu/slash-menu";
import { SlashHelp, SlashMenuList } from "@/components/slash-menu/slash-menu-list";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { IconHint } from "@/components/ui/tooltip";
import { useInlineSlash } from "@/hooks/use-inline-slash";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { useSlashMenu } from "@/hooks/use-slash-menu";
import { logFailure, sayFailure } from "@/lib/failure";
import { formatFileSize } from "@/lib/file-size";
import {
  type ChatCommandPreviewVm,
  type ChatInlinePreviewVm,
  chatCommandPreview,
  chatInlinePreview,
  clearDraft,
  clearDraftMirror,
  type IpcError,
  loadDraft,
  loadRemoteDraft,
  mirrorDraft,
  saveDraft,
} from "@/lib/ipc/client";
import { asShownPhoto } from "@/lib/shown-photo";
import { attachmentId, type PendingAttachment } from "@/lib/stores/attachments";
import type { PendingContext } from "@/lib/stores/composer";
import {
  useConversationAttachments,
  useConversationComposer,
  useConversationStores,
} from "@/lib/stores/conversation-stores";
import { draftsStore, useRemoteDraft } from "@/lib/stores/drafts";
import { useIncognito } from "@/lib/stores/incognito";
import { syncErrorMessage } from "@/lib/stores/sync";
import { cn } from "@/lib/utils";

/** Derive a chip display name for a pending attachment (its filename). */
function chipLabel(attachment: PendingAttachment): string {
  return attachment.filename;
}

/** The display filename derived from an OS file path (its basename). */
function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

/**
 * Where a submission goes, fixed at the Enter or click that submits it (R411):
 * the room, and the reply or edit it was submitted under (`null`: a new
 * message). It travels with the request, so a `/` line Rust answers later is
 * sent where it was submitted — never into a reply or edit begun meanwhile.
 */
export interface SendContext {
  accountId: string;
  roomId: string;
  target: {
    mode: PendingContext["mode"];
    targetKey: string;
    /** A reply an Undo gave back names its original by event (R447). */
    eventId?: string;
  } | null;
}

interface ComposerProps {
  /** Database sessions do not use Matrix drafts or room state. */
  transport?: "matrix" | "session";
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
   * Dispatch the trimmed body into `context` — `sendReply` / `editMessage` /
   * `sendText` by its target, never by whatever is pending when the dispatch
   * runs (R411). Resolves on success (the draft then clears); rejects if the
   * send could not be enqueued (the draft is kept so the user can retry).
   * `line` is the `/` line the person typed when the body is what Rust resolved
   * it to (`//etc` → `/etc`, Story 100.5): a held send keeps it for Undo (R393).
   */
  onSend: (body: string, context: SendContext, line?: string) => Promise<void>;
  /**
   * Dispatch the pending attachments. Session batches carry one caption; Matrix
   * media carry a caption only for a single attachment. Resolves when enqueued;
   * rejects to keep unaccepted selections and the draft available for retry.
   * Absent → the attach/paste affordances are inert.
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
  /**
   * Send an action — the `m.emote` a `/me` line resolves to (Story 100.5) —
   * into `context`'s room. `line` is the `/me …` line as typed, which a held
   * send keeps for Undo (R393). Rejects like {@link onSend}, and the same inline
   * failure copy applies; Rust logs the failure at WARN.
   */
  onSendEmote?: (body: string, context: SendContext, line: string) => Promise<void>;
  /**
   * Run a command Rust resolved (Story 100.5): resolves to what to say beside
   * the composer, or `null` when the act has its own surface (`/leave`'s
   * confirmation). Never called for `/help`, which the composer answers.
   * Absent (with {@link onSendEmote}) → no slash menu.
   */
  onCommand?: (command: ChatCommandInvocation) => Promise<ChatCommandOutcome | null>;
  /**
   * How a `/` line is resolved. The default asks Rust about this room; a test
   * passes its own.
   */
  resolveCommand?: (draft: string, replying: boolean) => Promise<ChatCommandPreviewVm>;
  /**
   * What a `/` at the caret offers mid-text (Story 101.3). The default asks
   * Rust about this room; a test passes its own.
   */
  resolveInline?: (before: string, after: string) => Promise<ChatInlinePreviewVm>;
}

/**
 * What the composer says when it could not find out what a `/` line was: the
 * one thing it refuses is sending text nobody resolved.
 */
export const COMPOSER_COMMAND_UNRESOLVED =
  "keeper couldn't tell whether that was a command, so nothing was sent.";

/**
 * What the composer says when the reply a `/` line was submitted under was
 * cancelled or changed before Rust answered: the line is not sent anywhere else.
 */
export const COMPOSER_CONTEXT_CHANGED =
  "Nothing was sent: the reply that line was for changed before keeper read it.";

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

/**
 * What the composer says when its draft could not be saved (Epic 100, R387):
 * leaving the chat or reloading would lose the words, so the person is told
 * — once, under the composer, not a toast per keystroke.
 */
export const DRAFT_NOT_SAVED =
  "keeper couldn't save this draft, so it is kept only while this chat stays open.";

/**
 * How long a pasted or picked batch may take to be read before it fails with its read-error
 * sentence and stops holding Send (R1071C-05). Generous: a large file or a photo the webview
 * re-encodes takes seconds, and the person can cancel the batch sooner from its chip or Escape.
 */
export const FILE_READ_TIMEOUT_MS = 60_000;

/** A pasted or picked batch still being read: its tray chip and the way to drop it. */
interface ReadingBatch {
  id: number;
  label: string;
  /** Settles the batch's read as cancelled: whatever its files resolve to later is ignored. */
  cancel: () => void;
}

/** What a batch's chip says it is reading: its one file's name, or how many files. */
function batchLabel(files: readonly File[], fallback: string): string {
  return files.length === 1 ? files[0].name || fallback : `${files.length} files`;
}

export function Composer({
  transport = "matrix",
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
  onSendEmote,
  onCommand,
  resolveCommand,
  resolveInline,
}: ComposerProps) {
  const [draft, setDraft] = useState("");
  // Where the caret is, `null` while text is selected: the mid-text `/` menu is
  // about the token right before it (Story 101.3).
  const [caret, setCaret] = useState<number | null>(null);
  // Where a mid-text insertion put the caret, applied once the field holds it.
  const insertedCaret = useRef<number | null>(null);
  const [sending, setSending] = useState(false);
  // The surfaced send failure, or `null`. `terminal` is an `IpcError` with
  // `retriable: false` — a reply/edit target that vanished or can no longer be
  // edited (`SendError::TargetNotFound | NotEditable`). Telling that user to check
  // their connection and try again would be a lie: no retry can ever succeed, so
  // the two cases get distinct copy (AD-21 honesty, Story 3.4).
  const [error, setError] = useState<"retriable" | "terminal" | { message: string } | null>(null);
  // The `account\0room` whose last draft save failed, or `null`: says
  // DRAFT_NOT_SAVED under that chat's composer until a save there succeeds.
  const [unsavedDraft, setUnsavedDraft] = useState<string | null>(null);
  // Phone-tier composer deltas (Story 13.5, UX-DR25): ≥44pt send/attach targets,
  // a 5-line autogrow cap, and Enter→newline (send is button-only on phone —
  // WKWebView cannot reliably distinguish a hardware keyboard, so the touch
  // default wins). Everything is tier-gated on the shared layout hook — never
  // user-agent sniffing, never a forked composer.
  const { phone } = useShellLayout();
  const [cameraOpen, setCameraOpen] = useState(false);
  const [photoError, setPhotoError] = useState<string | null>(null);
  // Batches of files still being read (a paste, a photo pick): counted from before the first
  // read, so Send waits for each batch to reach the tray whole (R1071C-04). Each one shows a
  // chip that cancels it, Escape cancels them all, and FILE_READ_TIMEOUT_MS fails a stuck one,
  // so no read holds Send forever (R1071C-05). The ref answers synchronous gates; the state
  // renders.
  const readingRef = useRef<readonly ReadingBatch[]>([]);
  const [reading, setReading] = useState<readonly ReadingBatch[]>([]);
  const nextBatch = useRef(0);
  const captureInput = useRef<HTMLInputElement>(null);
  const photoInput = useRef<HTMLInputElement>(null);
  // The remote (cross-device) draft body currently offered as a local-wins conflict
  // chip (Story 7.2), or `null` when nothing is offered. Set when a differing remote
  // draft arrives against non-empty local text; cleared on adopt / when it stops
  // differing. Local text is never overwritten without the user tapping "Use that
  // version".
  const [remoteOffer, setRemoteOffer] = useState<string | null>(null);
  // What the last slash command came to (Story 100.5): a quiet status, or a
  // refusal/failure shown as an alert with the draft kept. Editing retires it.
  const [commandNote, setCommandNote] = useState<ChatCommandOutcome | null>(null);
  // `/help`'s panel, held whole: it outlives the draft that opened it.
  const [help, setHelp] = useState<{ rows: readonly SlashRow[]; escapeHint: string } | null>(null);

  // Whether Incognito is effective for this chat (Story 8.1). Mirrored from the
  // Rust-resolved VM; drives the violet composer focus ring while it applies.
  const incognitoEffective = useIncognito(accountId, roomId)?.effective ?? false;
  // This conversation's composer state and tray: the chat's, or the notes dock's.
  const { composer, attachments: tray } = useConversationStores();

  // The textarea handle, focused programmatically when the composer store's focus
  // nonce is *bumped* (Story 6.6 — e.g. after a new chat is resolved and opened).
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const focusNonce = useConversationComposer((s) => s.focusNonce);
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

  // A focus request that waits for this room's composer (story 102.7, R463):
  // the room was opened before its timeline arrived, so the composer was
  // disabled then. Taken once it is enabled, and settled only when focus
  // actually landed — a disabled field refuses it, and the request waits on.
  const focusWhenReady = useConversationComposer((s) => s.focusWhenReady);
  useEffect(() => {
    if (
      disabled ||
      focusWhenReady === null ||
      focusWhenReady.accountId !== accountId ||
      focusWhenReady.roomId !== roomId
    ) {
      return;
    }
    const field = textareaRef.current;
    field?.focus();
    if (field !== null && document.activeElement === field) {
      composer.getState().focusLanded(focusWhenReady);
    }
  }, [focusWhenReady, disabled, accountId, roomId, composer]);

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
  const matrixDrafts = useRef(transport === "matrix");
  const draftSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingDraft = useRef<string | null>(null);
  // The latest draft write per `account\0room`: a save or deletion that
  // finishes after a newer one for the same chat neither clears nor sets that
  // chat's failure line (R410).
  const draftWrites = useRef(new Map<string, number>());
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
    if (!matrixDrafts.current) return;
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
      void mirrorDraft(a, r, trimmed).catch(logFailure("composer.mirrorDraft"));
    } else {
      void clearDraftMirror(a, r).catch(logFailure("composer.clearDraftMirror"));
    }
  }, []);

  /** Cross-device clear: tombstone the mirror (best-effort) and drop any queued mirror. */
  const clearMirror = useCallback(() => {
    if (!matrixDrafts.current) return;
    if (draftMirrorTimer.current !== null) {
      clearTimeout(draftMirrorTimer.current);
      draftMirrorTimer.current = null;
    }
    pendingMirror.current = null;
    void clearDraftMirror(accountIdRef.current, roomIdRef.current).catch(
      logFailure("composer.clearDraftMirror"),
    );
  }, []);

  /** Queue `body` for a debounced cross-device mirror write (Story 7.2). */
  const scheduleMirror = useCallback(
    (body: string) => {
      if (!matrixDrafts.current) return;
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
    if (!matrixDrafts.current) return;
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
    const key = `${a}\u0000${r}`;
    const write = (draftWrites.current.get(key) ?? 0) + 1;
    draftWrites.current.set(key, write);
    const latest = () => draftWrites.current.get(key) === write;
    // Never awaited on the keystroke path. A failed save is said once under the
    // composer and logged (de-duplicated by the funnel), and the chat is not marked
    // as holding a saved draft: leaving it would lose the words (R387). The line
    // goes once the chat's latest write lands — a save, or the deletion of a
    // draft the person emptied (nothing is left unsaved then); a deletion that
    // fails keeps it, because the chat may still hold older words (R410).
    const failed = (source: string) => (raw: unknown) => {
      logFailure(source)(raw);
      if (latest()) {
        draftsStore.getState().mark(a, r, false);
        setUnsavedDraft(key);
      }
    };
    const landed = () => {
      if (latest()) {
        setUnsavedDraft((unsaved) => (unsaved === key ? null : unsaved));
      }
    };
    if (trimmed.length > 0) {
      void saveDraft(a, r, trimmed).then(landed, failed("composer.saveDraft"));
    } else {
      void clearDraft(a, r).then(landed, failed("composer.clearDraft"));
    }
    draftsStore.getState().mark(a, r, trimmed.length > 0);
  }, []);

  /**
   * Delete the persisted draft + its marker after a successful send / composer clear
   * (Story 7.1). Cancels any queued debounced save so it can't re-write a row we just
   * deleted. Fire-and-forget — a delete failure never blocks the send path.
   */
  const clearPersistedDraft = useCallback(() => {
    if (!matrixDrafts.current) return;
    if (draftSaveTimer.current !== null) {
      clearTimeout(draftSaveTimer.current);
      draftSaveTimer.current = null;
    }
    pendingDraft.current = null;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    // Supersede any save still in flight: its late failure must not say a draft
    // was lost after the message was sent.
    const key = `${a}\u0000${r}`;
    draftWrites.current.set(key, (draftWrites.current.get(key) ?? 0) + 1);
    void clearDraft(a, r).catch(logFailure("composer.clearDraft"));
    draftsStore.getState().mark(a, r, false);
    // Nothing is left to save, so nothing is left unsaved.
    setUnsavedDraft(null);
    // Tombstone the cross-device mirror too (Story 7.2) so other devices stop showing
    // the sent/cleared draft. Best-effort — never blocks the send/clear path.
    clearMirror();
  }, [clearMirror]);

  /** Queue `body` for a ~200 ms debounced persist, updating the marker immediately. */
  const scheduleDraftSave = useCallback(
    (body: string) => {
      if (!matrixDrafts.current) return;
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

  const attachments = useConversationAttachments((s) => s.pending);
  // The attach/paste affordances are available only when the parent wires the
  // attachment dispatcher and the composer is enabled.
  const attachEnabled = onSendAttachments != null && !disabled;

  // Mirror the live draft in a ref so the prefill effect can stash it without
  // taking `draft` as a dependency (which would re-run the effect every keystroke).
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // Mirror the pending context in a ref so the remote-reconcile adopt callback can
  // check "am I editing?" without taking `pending` as a dependency (Story 7.2), and a
  // late `/` verdict can check whether its reply still stands (R411).
  const pendingRef = useRef(pending);
  pendingRef.current = pending;
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
      setError(null);
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
      if (body.length === 0 || pendingRef.current?.mode === "edit") {
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
  // returns the held body and the pill calls `composer.restore(accountId, roomId,
  // body)`, bumping `restoreNonce`. Apply it here, establishing the composer text
  // (latching `restoreConsumed` so a late mount-restore can't clobber it) and persisting
  // it as the durable draft — replacing current composer content per the documented
  // "restored text is the user's most recent intent" trade-off. Seeded to the current
  // nonce so a fresh mount does not self-apply a stale, already-consumed restore. The
  // restore only applies when its target matches this composer's chat, so a restore that
  // resolves after the user switched rooms never lands in the wrong room's composer.
  const restoreNonce = useConversationComposer((s) => s.restoreNonce);
  const seenRestoreNonce = useRef(restoreNonce);
  useEffect(() => {
    if (restoreNonce === seenRestoreNonce.current) {
      return;
    }
    seenRestoreNonce.current = restoreNonce;
    const { restoreBody: body, restoreTarget: target } = composer.getState();
    if (body === null || pendingRef.current?.mode === "edit") {
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
    setError(null);
    scheduleDraftSave(body);
  }, [restoreNonce, scheduleDraftSave, composer]);

  // Restore the persisted draft and reconcile the remote mirror once on mount (Story
  // 7.1 + 7.2). The composer remounts per (account, room) in the parent, so this runs
  // on each chat open. Local always wins: a differing remote draft is only ever *offered*
  // via the conflict chip; it replaces the composer only into an empty, untouched one
  // (drafts follow the user). Reads refs only, so `[]` deps are exhaustive; it never
  // overwrites content the user has already established (see `restoreConsumed`). A load
  // failure is logged (fall back to local / no chip, never a crash).
  useEffect(() => {
    if (!matrixDrafts.current) return;
    let cancelled = false;
    const a = accountIdRef.current;
    const r = roomIdRef.current;
    void Promise.all([
      loadDraft(a, r).catch((error: unknown) => {
        logFailure("composer.loadDraft")(error);
        return null;
      }),
      loadRemoteDraft(a, r).catch((error: unknown) => {
        logFailure("composer.loadRemoteDraft")(error);
        return null;
      }),
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
    if (!matrixDrafts.current) return;
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
    !sending &&
    reading.length === 0;

  /**
   * Dispatch the draft into `context`, the room and reply/edit it was submitted
   * under — or, for a `/` line Rust resolved (Story 100.5), the `message` it
   * resolved to: its text, whether it is an action, and the line the person
   * typed, which a held send keeps for Undo (R393).
   *
   * The draft is this send's only while it still holds the line that was sent
   * (R393, R411): then its queued save and mirror are cancelled before the
   * dispatch and the draft is cleared and persisted empty after it. Anything
   * else — text typed during a slow send, or while Rust read a `/` line — is the
   * person's next message: its text, its queued save and its mirror are left
   * exactly as they are.
   */
  async function send(
    context: SendContext,
    message?: { body: string; emote: boolean; line: string },
  ) {
    // An edit sends onto an existing message and must not touch this room's
    // persistent draft, so it restores the pre-edit text instead of clearing
    // (Story 7.1). A reply/text send owns the draft and clears it.
    const wasEdit = context.target?.mode === "edit";
    const ran = message?.line ?? draft.trim();
    const body = message?.body ?? draft.trim();
    const trayAttachments = tray.getState().pending;
    const dispatchAttachments =
      message === undefined && onSendAttachments != null && !wasEdit && trayAttachments.length > 0;
    if (
      (body.length === 0 && !dispatchAttachments) ||
      disabled ||
      sending ||
      readingRef.current.length > 0
    ) {
      // Whitespace-only with no attachment / disabled / in-flight / a pasted or picked file
      // still being read (it belongs to this message, R1071C-04): never dispatch.
      return;
    }
    // A dispatch consumes/replaces the composer content; a late mount restore must not
    // resurrect a just-sent draft into the emptied composer. (Story 7.1)
    restoreConsumed.current = true;
    // While the draft is the line being sent, its queued save and mirror are this
    // send's: cancel them before the (possibly slow) send. Otherwise a flush landing
    // mid-send could re-`saveDraft` a row we then `clearDraft` (or write the mirror
    // that the post-send tombstone then races), and the fire-and-forget writes could
    // reorder — leaving an orphan draft + amber marker, or a resurrected draft on
    // other devices, on an already-sent chat (Story 7.1, 7.2). A failed send
    // re-queues them in the catch below.
    const owned = draftIs(ran);
    if (owned) {
      if (draftSaveTimer.current !== null) {
        clearTimeout(draftSaveTimer.current);
        draftSaveTimer.current = null;
      }
      pendingDraft.current = null;
      if (draftMirrorTimer.current !== null) {
        clearTimeout(draftMirrorTimer.current);
        draftMirrorTimer.current = null;
      }
      pendingMirror.current = null;
    }
    // Sending stops typing (Story 3.9): clear the notice once as the message goes.
    stopTyping();
    setSending(true);
    setError(null);
    try {
      if (dispatchAttachments && onSendAttachments != null) {
        const caption =
          (transport === "session" || trayAttachments.length === 1) && body.length > 0
            ? body
            : undefined;
        await onSendAttachments(trayAttachments, caption);
        // If the text did not ride as a caption (multiple attachments) but the
        // user typed a body, dispatch it as its own message.
        if (caption === undefined && body.length > 0) {
          await onSend(body, context);
        }
        // A selection added while IPC was pending belongs to the next send.
        for (const attachment of trayAttachments) tray.getState().remove(attachment.id);
        spendDraft(ran);
      } else {
        if (message === undefined) {
          await onSend(body, context);
        } else if (message.emote && onSendEmote != null) {
          await onSendEmote(body, context, message.line);
        } else {
          await onSend(body, context, message.line);
        }
        // Clear only on success so a failed enqueue keeps the user's text.
        if (wasEdit) {
          // Editing an existing message leaves the persistent draft untouched: restore
          // the pre-edit composer text (the real draft) and keep the stored row/marker.
          setDraft(preEditDraft.current);
          // Re-queue that text for persistence, so the stored row tracks the restored
          // composer even if a pre-edit keystroke's save never landed (Story 7.1).
          scheduleDraftSave(preEditDraft.current);
        } else {
          spendDraft(ran);
        }
      }
    } catch (raw) {
      // Enqueue-time failure produces no timeline echo to fall back on, so surface an
      // honest inline error (AD-21) and keep the draft/tray so the user can resend.
      // Async delivery failures instead show as the message's Failed send-state caption.
      // A non-retriable envelope (a reply/edit target that vanished or can no longer be
      // edited) gets terminal copy: the generic "check your connection and try again"
      // would send the user chasing a retry that can never succeed. Anything that is not
      // a recognisable envelope is treated as retriable — the weaker, safer claim.
      logFailure("composer.send")(raw);
      const err = raw as IpcError | null | undefined;
      setError(
        transport === "session"
          ? { message: syncErrorMessage(raw, "keeper could not send this message.") }
          : err?.retriable === false
            ? "terminal"
            : "retriable",
      );
      // Re-queue what was cancelled above while the draft is still the line that
      // failed, so a failed send stays durable across relaunch. Text typed since is
      // the next message and queued its own save; an edit never touched the stored
      // draft, so there is nothing to re-persist. While an edit begun since borrows
      // the field, the room's draft is the line that edit set aside — the failed
      // line — never the edit's body (R100S3-01).
      if (owned && !wasEdit && draftIs(ran)) {
        scheduleDraftSave(roomDraft());
      }
    } finally {
      setSending(false);
    }
  }

  // Slash commands (Story 100.5) — off while editing (the text is that
  // message's body) and while attachments wait (the text is their caption).
  const commandsEnabled =
    onCommand != null && onSendEmote != null && pending?.mode !== "edit" && !hasAttachments;
  const replying = pending?.mode === "reply";
  const resolve = useCallback(
    (line: string) =>
      resolveCommand != null
        ? resolveCommand(line, replying)
        : chatCommandPreview(accountId, roomId, line, replying),
    [resolveCommand, accountId, roomId, replying],
  );

  // One submission at a time (R393): taken synchronously by the Enter or click
  // that starts it, before any preview is awaited, and released when the line
  // has been sent, run or refused — so a second Enter while it is held does
  // nothing, rather than running the same line twice.
  const inFlight = useRef(false);
  // This composer's room lifetime (it is keyed by room). An answer that arrives
  // after the room was left belongs to a composer nobody is looking at: it is
  // dropped, never obeyed — a late `/leave` must not ask about the room on screen.
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  /**
   * Whether this room's draft is still `ran`: the field — or, while an edit
   * borrows the field, the text that edit set aside (R411).
   */
  function draftIs(ran: string): boolean {
    return roomDraft().trim() === ran;
  }

  /** This room's draft: the field, or the text an edit borrowing it set aside. */
  function roomDraft(): string {
    return pendingRef.current?.mode === "edit" ? preEditDraft.current : draftRef.current;
  }

  /**
   * The draft is spent by what `ran` did — only if it is still that line, and
   * then it is cleared and persisted empty (R393, R411). A changed draft is the
   * person's next message and is not touched at all.
   */
  function spendDraft(ran: string) {
    if (!draftIs(ran)) {
      return;
    }
    restoreConsumed.current = true;
    if (pendingRef.current?.mode === "edit") {
      // The line was sent while an edit borrowed the field: the text that edit
      // will put back is spent, not the edit's body.
      preEditDraft.current = "";
    } else {
      setDraft("");
      stopTyping();
    }
    clearPersistedDraft();
  }

  /** Obey what Rust said `ran`, a `/` line submitted in `context`, is. */
  async function obey(answer: ChatCommandPreviewVm, ran: string, context: SendContext) {
    const verdict = answer.verdict;
    switch (verdict.kind) {
      case "text":
      case "emote": {
        // The reply it was submitted under must still be the one pending (R411); a
        // new message stands for the composer's whole life, which is its room's.
        const target = context.target;
        const now = pendingRef.current;
        if (target !== null && (now?.mode !== target.mode || now.targetKey !== target.targetKey)) {
          // Sent nowhere else: not as a new message, not into another reply.
          setCommandNote({ tone: "failed", message: COMPOSER_CONTEXT_CHANGED });
          return;
        }
        await send(context, { body: verdict.text, emote: verdict.kind === "emote", line: ran });
        return;
      }
      case "refusal":
        // The draft stays: it is one keystroke from being right. A refusal is
        // the answer, not a failure, so nothing goes to `keeper.log` (R393).
        setCommandNote({ tone: "failed", message: verdict.message });
        return;
      case "command": {
        if (verdict.name === "help") {
          spendDraft(ran);
          // The narrowed answer is still true about this token, so the panel
          // opens with it; the failed full list is said and logged once.
          const all = await resolve("/").catch((raw: unknown) => {
            menu.fail(raw);
            return answer;
          });
          setHelp({ rows: all.rows, escapeHint: all.escapeHint });
          return;
        }
        if (onCommand == null) {
          return;
        }
        setSending(true);
        try {
          const outcome = await onCommand({ name: verdict.name, args: verdict.args });
          setCommandNote(outcome);
          if (outcome === null || outcome.tone === "done") {
            spendDraft(ran);
          }
        } finally {
          setSending(false);
        }
        return;
      }
    }
  }

  /**
   * Enter / Send: prose goes straight out; a `/` line is asked about first. The
   * room and reply/edit are fixed here, synchronously, and travel with it (R411).
   */
  function submit() {
    if (inFlight.current || disabled || sending || readingRef.current.length > 0) {
      return;
    }
    inFlight.current = true;
    const release = () => {
      inFlight.current = false;
    };
    const context: SendContext = {
      accountId,
      roomId,
      target:
        pending === null
          ? null
          : pending.mode === "reply" && pending.eventId !== undefined
            ? { mode: pending.mode, targetKey: pending.targetKey, eventId: pending.eventId }
            : { mode: pending.mode, targetKey: pending.targetKey },
    };
    const text = draft.trim();
    if (!commandsEnabled || !text.startsWith("/")) {
      void send(context).finally(release);
      return;
    }
    setCommandNote(null);
    void resolve(text)
      .then((answer) => (alive.current ? obey(answer, text, context) : undefined))
      .catch((raw: unknown) => {
        // The shell could not say what the line was; sending it unresolved is
        // the one thing this must not do.
        menu.fail(raw);
        setCommandNote({ tone: "failed", message: COMPOSER_COMMAND_UNRESOLVED });
      })
      .finally(release);
  }

  const menu = useSlashMenu({
    draft,
    enabled: commandsEnabled,
    resolve,
    onComplete: (completed) => {
      restoreConsumed.current = true;
      setDraft(completed);
      setCommandNote(null);
      scheduleDraftSave(completed);
    },
    onRun: submit,
    reportAs: "chat-command-preview",
  });

  // The mid-text `/` (Story 101.3, R418): wherever the composer has a slash
  // menu, except over a whole-line command, which is that menu's.
  const inlineEnabled =
    onCommand != null && onSendEmote != null && !(commandsEnabled && isCommandLine(draft));
  const resolveInlineHere = useCallback(
    (before: string, after: string) =>
      resolveInline != null
        ? resolveInline(before, after)
        : chatInlinePreview(accountId, roomId, before, after),
    [resolveInline, accountId, roomId],
  );
  const inline = useInlineSlash({
    draft,
    caret,
    enabled: inlineEnabled,
    resolve: resolveInlineHere,
    onInsert: (next, at) => {
      restoreConsumed.current = true;
      insertedCaret.current = at;
      setDraft(next);
      setCaret(at);
      if (pendingRef.current?.mode !== "edit") {
        scheduleDraftSave(next);
      }
    },
    reportAs: "chat-inline-preview",
  });
  useLayoutEffect(() => {
    const at = insertedCaret.current;
    const field = textareaRef.current;
    if (at !== null && field !== null && field.value === draft) {
      insertedCaret.current = null;
      field.setSelectionRange(at, at);
    }
  }, [draft]);

  /** Open the native file picker and add each chosen path to the tray. */
  async function pickFiles(filters?: DialogFilter[]) {
    if (!attachEnabled) {
      return;
    }
    try {
      const selection = await open({ multiple: true, filters });
      if (selection == null) {
        // Dialog cancelled → no-op.
        return;
      }
      const paths = Array.isArray(selection) ? selection : [selection];
      tray.getState().addMany(
        paths.map((path) => ({
          id: attachmentId(),
          kind: "path" as const,
          path,
          filename: basename(path),
        })),
      );
    } catch (error) {
      // A dialog failure is non-fatal — the user can retry once told.
      sayFailure("composer.pickFiles", "keeper couldn't open the file picker.")(error);
    }
  }

  /**
   * Stop holding Send for batch `id`. Returns whether it was still pending: `false` means it was
   * already cancelled, timed out or landed, and whatever its read comes to is no longer wanted.
   */
  function settleBatch(id: number): boolean {
    const before = readingRef.current;
    const after = before.filter((batch) => batch.id !== id);
    if (after.length === before.length) {
      return false;
    }
    readingRef.current = after;
    setReading(after);
    return true;
  }

  /** Drop a batch still being read (its chip's remove control, or Escape): nothing of it joins. */
  function cancelReading(batch: ReadingBatch) {
    if (settleBatch(batch.id)) {
      batch.cancel();
    }
  }

  /**
   * Read a batch of files into the tray, whole or not at all. The batch is pending from before
   * its first read until it lands, and Send waits for every pending batch: a caption pasted with
   * a file never leaves without it, and no message carries part of a pasted or picked batch
   * (R1071C-04). A failed read adds none of the batch and throws for the caller to say so; what
   * was read is dropped. A batch that is cancelled resolves quietly and adds nothing, even when
   * its read finishes later; one still unread after FILE_READ_TIMEOUT_MS fails like a failed
   * read (R1071C-05).
   */
  async function readIntoTray(label: string, read: () => Promise<PendingAttachment[]>) {
    const id = nextBatch.current++;
    let cancel = () => {};
    const cancelled = new Promise<null>((resolve) => {
      cancel = () => resolve(null);
    });
    let timer: ReturnType<typeof setTimeout> | undefined;
    const deadline = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(new Error(`the file read took longer than ${FILE_READ_TIMEOUT_MS} ms`)),
        FILE_READ_TIMEOUT_MS,
      );
    });
    readingRef.current = [...readingRef.current, { id, label, cancel }];
    setReading(readingRef.current);
    try {
      const files = await Promise.race([read(), cancelled, deadline]);
      // Only a batch nobody cancelled joins; a cancelled one's late bytes go nowhere.
      if (settleBatch(id) && files !== null) {
        tray.getState().addMany(files);
      }
    } catch (raw) {
      if (settleBatch(id)) {
        throw raw;
      }
    } finally {
      clearTimeout(timer);
    }
  }

  async function readPhotos(input: HTMLInputElement) {
    const files = Array.from(input.files ?? []);
    setPhotoError(null);
    try {
      await readIntoTray(batchLabel(files, "photo"), async () => {
        const read: PendingAttachment[] = [];
        for (const file of files) {
          // A HEIC or other picture a model is not shown goes as JPEG when this
          // webview can decode it, else as it is (R444/05).
          const photo = await asShownPhoto(file, file.name || photoFilename());
          read.push({ id: attachmentId(), kind: "bytes", ...photo, size: photo.bytes.byteLength });
        }
        return read;
      });
    } catch (raw) {
      logFailure("composer.photo")(raw);
      setPhotoError("keeper couldn't read this photo. Choose it again to retry.");
    } finally {
      input.value = "";
    }
  }

  /**
   * Paste files into the tray. Plain-text-only paste keeps the browser's own; a paste that also
   * carries plain text (a screenshot with its caption) puts that text into the draft at the
   * caret, as the browser would have, while the files are staged. Text and HTML items never
   * become attachments (R1071C-01).
   */
  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    if (!attachEnabled || sending) return;
    const items = Array.from(e.clipboardData.items);
    const files = items
      .filter((item) =>
        transport === "session" ? item.kind === "file" : item.type.startsWith("image/"),
      )
      .map((item) => item.getAsFile())
      .filter((file): file is File => file !== null);
    if (files.length === 0) return;
    e.preventDefault();
    const text = items.some((item) => item.kind === "string" && item.type === "text/plain")
      ? e.clipboardData.getData("text/plain")
      : "";
    if (text !== "") {
      const field = e.currentTarget;
      const next =
        field.value.slice(0, field.selectionStart) + text + field.value.slice(field.selectionEnd);
      const at = field.selectionStart + text.length;
      restoreConsumed.current = true;
      insertedCaret.current = at;
      setDraft(next);
      setCaret(at);
      if (pendingRef.current?.mode !== "edit") {
        scheduleDraftSave(next);
      }
    }
    // A new paste retires an earlier read's failure or timeout, as a new photo pick does.
    setPhotoError(null);
    void readIntoTray(batchLabel(files, "pasted file"), async () => {
      const read: PendingAttachment[] = [];
      for (const file of files) {
        read.push({
          id: attachmentId(),
          kind: "bytes",
          bytes: await file.arrayBuffer(),
          filename: file.name || `pasted-file.${file.type.split("/")[1] || "bin"}`,
          mime: file.type || "application/octet-stream",
          size: file.size,
        });
      }
      return read;
    }).catch((raw: unknown) => {
      logFailure("composer.paste")(raw);
      setPhotoError("keeper couldn't read the pasted file. Paste it again to retry.");
    });
  }

  /** Remove a pending attachment (a pre-upload cancel). */
  function removeAttachment(id: string) {
    tray.getState().remove(id);
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
    // The open slash menu claims ↑/↓/Enter/Tab/Escape first; everything it does
    // not claim — and every key while it is closed — keeps its meaning below.
    if (menu.onKeyDown(e) || inline.onKeyDown(e)) {
      return;
    }
    if (e.key === "Escape" && readingRef.current.length > 0) {
      // Esc first drops every batch still being read (R1071C-05); the caption stays, and a
      // second Esc cancels a pending reply/edit.
      e.preventDefault();
      for (const batch of readingRef.current) {
        cancelReading(batch);
      }
      return;
    }
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
      submit();
    }
  }

  return (
    <div className="flex min-w-0 flex-col gap-1">
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
          <IconHint label="Cancel">
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Cancel"
              onClick={cancelPending}
            >
              ×
            </Button>
          </IconHint>
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
        <ul
          aria-label="Pending attachments"
          className="flex min-w-0 flex-wrap gap-1.5"
          aria-live="polite"
        >
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border bg-muted/50 py-1 pr-1 pl-2"
            >
              <span
                className="min-w-0 flex-1 break-words text-xs [overflow-wrap:anywhere]"
                title={chipLabel(attachment)}
              >
                {chipLabel(attachment)}
              </span>
              {attachment.kind === "bytes" && (
                <span className="shrink-0 text-muted-foreground text-xs">
                  {formatFileSize(attachment.size)}
                </span>
              )}
              {transport === "session" && attachment.kind === "path" && (
                <span className="shrink-0 text-muted-foreground text-xs">Size on send</span>
              )}
              <IconHint label={`Remove ${chipLabel(attachment)}`}>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Remove ${chipLabel(attachment)}`}
                  className={cn(transport === "session" && "size-11 shrink-0")}
                  disabled={sending}
                  onClick={() => removeAttachment(attachment.id)}
                >
                  <X aria-hidden="true" className="size-3" />
                </Button>
              </IconHint>
            </li>
          ))}
        </ul>
      )}
      {/* Batches still being read (R1071C-04/-05): not yet sendable, so not in the tray above,
          but each is visible and can be dropped; Send waits for the rest. */}
      {reading.length > 0 && (
        <ul aria-label="Files being read" className="flex min-w-0 flex-wrap gap-1.5">
          {reading.map((batch) => (
            <li
              key={batch.id}
              className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md border border-border border-dashed bg-muted/30 py-1 pr-1 pl-2"
            >
              <span
                className="min-w-0 flex-1 break-words text-xs [overflow-wrap:anywhere]"
                title={batch.label}
              >
                {batch.label}
              </span>
              <span className="shrink-0 text-muted-foreground text-xs">reading…</span>
              <IconHint label={`Cancel reading ${batch.label}`}>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={`Cancel reading ${batch.label}`}
                  className={cn(transport === "session" && "size-11 shrink-0")}
                  onClick={() => cancelReading(batch)}
                >
                  <X aria-hidden="true" className="size-3" />
                </Button>
              </IconHint>
            </li>
          ))}
        </ul>
      )}
      {help !== null && (
        <SlashHelp rows={help.rows} escapeHint={help.escapeHint} onClose={() => setHelp(null)} />
      )}
      <div
        className={cn(
          "relative flex min-w-0 items-end gap-2",
          transport === "session" && "flex-wrap",
        )}
      >
        <SlashMenuList menu={menu} />
        <InlineSlashList menu={inline} />
        {attachEnabled && pending?.mode !== "edit" && (
          <IconHint label="Attach file">
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
          </IconHint>
        )}
        {attachEnabled && pending?.mode !== "edit" && (
          <>
            {(phone || typeof navigator.mediaDevices?.getUserMedia === "function") && (
              <IconHint label="Take photo">
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label="Take photo"
                  className={cn(phone && "size-11")}
                  onClick={() => (phone ? captureInput.current?.click() : setCameraOpen(true))}
                >
                  <Camera aria-hidden="true" />
                </Button>
              </IconHint>
            )}
            <IconHint label="Choose photo">
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label="Choose photo"
                className={cn(phone && "size-11")}
                onClick={() =>
                  phone
                    ? photoInput.current?.click()
                    : // A path the host sends as it is: a HEIC chosen here
                      // reaches the agent with its host's note that its
                      // format can't be shown to the model (R444/05).
                      void pickFiles([
                        {
                          name: "Images",
                          extensions: ["png", "jpg", "jpeg", "webp", "gif", "heic"],
                        },
                      ])
                }
              >
                <Image aria-hidden="true" />
              </Button>
            </IconHint>
            {phone && (
              <>
                <input
                  ref={captureInput}
                  type="file"
                  accept="image/*"
                  capture="environment"
                  aria-label="Take a photo file"
                  hidden
                  onChange={(event) => void readPhotos(event.currentTarget)}
                />
                <input
                  ref={photoInput}
                  type="file"
                  accept="image/*"
                  multiple
                  aria-label="Choose photo files"
                  hidden
                  onChange={(event) => void readPhotos(event.currentTarget)}
                />
              </>
            )}
            {cameraOpen && !phone && (
              <CameraSheet
                onClose={() => setCameraOpen(false)}
                onPhoto={(photo) =>
                  tray.getState().add({ id: attachmentId(), kind: "bytes", ...photo })
                }
              />
            )}
          </>
        )}
        <Textarea
          ref={textareaRef}
          aria-label="Message"
          {...(inline.open ? inline.fieldProps : menu.fieldProps)}
          placeholder="Write a message…"
          value={draft}
          disabled={disabled}
          onSelect={(e) => {
            const field = e.currentTarget;
            setCaret(field.selectionStart === field.selectionEnd ? field.selectionStart : null);
          }}
          onChange={(e) => {
            const next = e.target.value;
            setDraft(next);
            setCaret(
              e.target.selectionStart === e.target.selectionEnd ? e.target.selectionStart : null,
            );
            // The user has typed into this composer; a late mount restore must not
            // clobber their input (even after they clear it back to empty). (Story 7.1)
            restoreConsumed.current = true;
            if (error !== null) {
              setError(null);
            }
            if (commandNote !== null) {
              setCommandNote(null);
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
            "min-h-9 min-w-0 flex-1 resize-none",
            transport === "session" && "order-first basis-full",
            phone ? "max-h-[calc(5*1.5rem+1rem)]" : "max-h-[calc(8*1.5rem+1rem)]",
            incognitoEffective && "focus-visible:border-incognito focus-visible:ring-incognito/50",
          )}
          data-incognito={incognitoEffective ? "true" : undefined}
        />
        <Button
          type="button"
          onClick={submit}
          disabled={!canSend}
          aria-label={pending?.mode === "edit" ? "Save edit" : "Send message"}
          // Phone (Story 13.5): a ≥44pt primary-tinted hit target — the sole send
          // path on touch (tap = the FR-41 approval trigger). Desktop keeps the
          // default button size.
          className={cn(phone && "h-11 min-w-11", transport === "session" && "ml-auto")}
        >
          {pending?.mode === "edit" ? "Save" : "Send"}
        </Button>
      </div>
      {reading.length > 0 && (
        <p role="status" className="text-muted-foreground text-xs">
          Reading the file to send…
        </p>
      )}
      {photoError !== null && (
        <p role="alert" className="text-destructive text-xs">
          {photoError}
        </p>
      )}
      {unsavedDraft === `${accountId}\u0000${roomId}` && (
        <p role="alert" className="text-destructive text-xs">
          {DRAFT_NOT_SAVED}
        </p>
      )}
      {/* One line about the slash path: the command's own sentence when there is
          one, else why the menu could not be asked (R381). */}
      {commandNote === null && menu.failure !== null && (
        <p role="alert" className="text-destructive text-xs">
          {menu.failure}
        </p>
      )}
      {commandNote === null && menu.failure === null && inline.failure !== null && (
        <p role="alert" className="text-destructive text-xs">
          {inline.failure}
        </p>
      )}
      {commandNote !== null &&
        (commandNote.tone === "failed" ? (
          <p role="alert" className="text-destructive text-xs">
            {commandNote.message}
          </p>
        ) : (
          <p role="status" className="text-muted-foreground text-xs">
            {commandNote.message}
          </p>
        ))}
      {error !== null && (
        <p role="alert" className="text-destructive text-xs">
          {typeof error === "object"
            ? error.message
            : error === "terminal"
              ? "Couldn't send. That message can no longer be edited or replied to."
              : "Couldn't send. Check your connection and try again."}
        </p>
      )}
    </div>
  );
}
