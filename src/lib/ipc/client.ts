/**
 * Thin typed IPC client (AD-7, AD-8).
 *
 * The only hand-written TypeScript in `src/lib/ipc/`: wrappers around the Tauri
 * `invoke`/`Channel` primitives that carry the generated view-model types and
 * surface the {@link IpcError} envelope on rejection. All view-model types are
 * generated into `./gen/` by the Rust ts-rs export step — never hand-edited.
 */
import { Channel, invoke as tauriInvoke } from "@tauri-apps/api/core";
import type { IpcError } from "./gen/IpcError";

export type { AccountVm } from "./gen/AccountVm";
export type { BackupStatus } from "./gen/BackupStatus";
export type { ConnectionStatus } from "./gen/ConnectionStatus";
export type { ConnectionStatusBatch } from "./gen/ConnectionStatusBatch";
export type { DemoBatch } from "./gen/DemoBatch";
export type { DemoItem } from "./gen/DemoItem";
export type { EncryptionStatus } from "./gen/EncryptionStatus";
export type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
export type { InboxBatch } from "./gen/InboxBatch";
export type { InboxOp } from "./gen/InboxOp";
export type { InboxRoomVm } from "./gen/InboxRoomVm";
export type { IpcError } from "./gen/IpcError";
export type { IpcErrorCode } from "./gen/IpcErrorCode";
export type { MediaKindVm } from "./gen/MediaKindVm";
export type { MediaVm } from "./gen/MediaVm";
export type { PingVm } from "./gen/PingVm";
export type { Provider } from "./gen/Provider";
export type { ReactionGroupVm } from "./gen/ReactionGroupVm";
export type { ReplyPreviewVm } from "./gen/ReplyPreviewVm";
export type { RoomListBatch } from "./gen/RoomListBatch";
export type { RoomListOp } from "./gen/RoomListOp";
export type { RoomVm } from "./gen/RoomVm";
export type { SasEmojiVm } from "./gen/SasEmojiVm";
export type { SendState } from "./gen/SendState";
export type { TimelineBatch } from "./gen/TimelineBatch";
export type { TimelineItemVm } from "./gen/TimelineItemVm";
export type { TimelineOp } from "./gen/TimelineOp";
export type { VerificationFlowVm } from "./gen/VerificationFlowVm";
export type { VerificationPhase } from "./gen/VerificationPhase";

import type { AccountVm } from "./gen/AccountVm";
import type { BackupStatus } from "./gen/BackupStatus";
import type { ConnectionStatusBatch } from "./gen/ConnectionStatusBatch";
import type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
import type { InboxBatch } from "./gen/InboxBatch";
import type { RoomListBatch } from "./gen/RoomListBatch";
import type { TimelineBatch } from "./gen/TimelineBatch";
import type { VerificationFlowVm } from "./gen/VerificationFlowVm";

/**
 * Structural guard for the {@link IpcError} envelope so we can rethrow it
 * faithfully rather than as an opaque value.
 */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.code === "string" &&
    typeof candidate.message === "string" &&
    typeof candidate.retriable === "boolean"
  );
}

/**
 * Typed one-shot command invocation. Resolves with the command's view model or
 * rejects with the {@link IpcError} envelope (never a raw string).
 */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await tauriInvoke<T>(cmd, args);
  } catch (raw) {
    if (isIpcError(raw)) {
      throw raw;
    }
    // Backend contract guarantees an IpcError; anything else is unexpected.
    throw {
      code: "internal",
      message: typeof raw === "string" ? raw : "unexpected IPC failure",
      accountId: null,
      retriable: false,
    } satisfies IpcError;
  }
}

/**
 * Password login (FR-1, FR-5). Sends the homeserver, username, and password to
 * the Rust core, which runs the store-less SSS probe, logs in, persists the
 * session to the Keychain, and writes the account registry row. Resolves with
 * the non-secret {@link AccountVm}; rejects with the {@link IpcError} envelope
 * (whose `code` distinguishes bad credentials / unreachable / unsupported login
 * type / non-SSS). The password is transient — it is never returned or stored.
 */
export async function loginPassword(
  homeserver: string,
  username: string,
  password: string,
): Promise<AccountVm> {
  return await invoke<AccountVm>("login_password", { homeserver, username, password });
}

/**
 * OIDC (OAuth 2.0 / MSC3861) login (Story 2.2). Sends the homeserver to the Rust
 * core, which runs the store-less SSS probe, opens the system browser for OAuth
 * consent, awaits the `keeper://oauth/callback` deep link, finishes the token
 * exchange, persists the session to the Keychain, and writes the registry row.
 * Resolves with the non-secret {@link AccountVm}; rejects with the
 * {@link IpcError} envelope (whose `code` distinguishes non-SSS / OIDC
 * unsupported / timed out / cancelled / failed). No token or authorization
 * `code`/`state` ever crosses back to JavaScript.
 *
 * This call stays pending for the whole browser round-trip; use
 * {@link cancelOidc} to abort it.
 */
export async function loginOidc(homeserver: string): Promise<AccountVm> {
  return await invoke<AccountVm>("login_oidc", { homeserver });
}

/**
 * Cancel any in-progress OIDC flow (Story 2.2). The pending {@link loginOidc}
 * call then rejects with `code: "oauthCancelled"` and the Rust core rolls back
 * any partial state. Idempotent — a no-op when no flow is pending.
 */
export async function cancelOidc(): Promise<void> {
  await invoke<void>("cancel_oidc");
}

/**
 * Request a Beeper email login code (Story 2.3, step 1). Sends the email to the
 * Rust core, which runs Beeper's unofficial `POST /user/login` → `POST
 * /user/login/email` and stores the intermediate request id server-side (keyed
 * by email) so it never crosses IPC. Resolves once a code has been emailed;
 * rejects with the {@link IpcError} envelope (`code: "beeperUnavailable"`,
 * `retriable: true`) on any Beeper failure — a non-2xx, timeout, transport error,
 * or a private-API shape change. No bearer token or request id crosses IPC.
 */
export async function beeperRequestCode(email: string): Promise<void> {
  await invoke<void>("beeper_request_code", { email });
}

/**
 * Complete a Beeper email-code login (Story 2.3, step 2). Sends the email and the
 * emailed code to the Rust core, which takes the stored request id, runs `POST
 * /user/login/response` to obtain the Beeper JWT, then completes login via
 * `org.matrix.login.jwt` against `matrix.beeper.com` through the shared
 * add-account pipeline. Resolves with the non-secret {@link AccountVm}; rejects
 * with the {@link IpcError} envelope (`code: "beeperUnavailable"`, `retriable:
 * true`) on any Beeper failure (including an abandoned flow with no stored
 * request id). The emailed `code` is transient — never returned or stored.
 */
export async function loginBeeper(email: string, code: string): Promise<AccountVm> {
  return await invoke<AccountVm>("login_beeper", { email, code });
}

/**
 * Cancel any in-progress Beeper login flow (Story 2.3). The Rust core clears the
 * registry so no pending request id lingers; nothing is persisted. Idempotent —
 * a no-op when no flow is pending.
 */
export async function cancelBeeper(): Promise<void> {
  await invoke<void>("cancel_beeper");
}

/**
 * Persist the app-wide at-rest encryption posture (Story 2.6, AD-22). Sends the
 * chosen posture (`true` = encrypt SDK stores with a per-account passphrase,
 * `false` = FileVault only) to the Rust core, which writes it to `keeper.db`. The
 * passphrase itself is generated and stored (Keychain only) later, inside the
 * next account add — nothing secret crosses IPC. Resolves once persisted.
 */
export async function setEncryptionPosture(enabled: boolean): Promise<void> {
  await invoke<void>("set_encryption_posture", { enabled });
}

/**
 * Read the app-wide at-rest encryption posture (Story 2.6). Resolves with `true`
 * (on), `false` (off), or `null` (unchosen — the fresh-install state that gates
 * the first-run choice). The Rust `Option<bool>` serializes to `boolean | null`.
 */
export async function encryptionPosture(): Promise<boolean | null> {
  return await invoke<boolean | null>("encryption_posture");
}

/**
 * Report every persisted account that can be restored on launch (FR-8, AD-20).
 * Identity only — the Rust core lists the registry rows and returns each whose
 * Keychain session is present as a non-secret {@link AccountVm} (with hue).
 * Resolves with an array (empty on a cold install); a row whose session is gone
 * is skipped. No token or session material ever crosses IPC.
 */
export async function sessionRestore(): Promise<AccountVm[]> {
  return await invoke<AccountVm[]>("session_restore");
}

/**
 * Sign out an account locally (AD-10, Story 1.8). The Rust core tears down the
 * account's live supervision tasks then deletes exactly its SDK store dir,
 * Keychain session entry, and registry row — no server-side logout, works
 * offline, idempotent. Rejects with the {@link IpcError} envelope on a cleanup
 * failure.
 */
export async function signOut(accountId: string): Promise<void> {
  await invoke<void>("sign_out", { accountId });
}

/**
 * Open a streaming subscription. Creates a `Channel`, forwards each delivered
 * batch to `onBatch` in arrival order (snapshot before any diff, per AD-8), and
 * resolves with the backend-assigned subscription id.
 */
export async function subscribe<TBatch>(
  cmd: string,
  onBatch: (batch: TBatch) => void,
  args?: Record<string, unknown>,
): Promise<number> {
  const channel = new Channel<TBatch>();
  // Arm `onmessage` BEFORE invoking: this ordering is load-bearing. The demo
  // command delivers synchronously, but real streams will emit asynchronously
  // from a spawned task after the id-returning command resolves — batches sent
  // before the handler is set would be dropped. Keep this order when copying.
  channel.onmessage = onBatch;
  return await invoke<number>(cmd, { ...args, channel });
}

/**
 * Subscribe to an account's sliding-sync room list (FR-8, AD-8). Opens a
 * `Channel`, forwards each {@link RoomListBatch} to `onBatch` in arrival order
 * (a `Reset` snapshot before any diff), and resolves with the subscription id.
 * Rejects with the {@link IpcError} envelope (`code: "syncUnavailable"`) if the
 * account cannot start syncing.
 */
export async function subscribeRoomList(
  accountId: string,
  onBatch: (batch: RoomListBatch) => void,
): Promise<number> {
  return await subscribe<RoomListBatch>("room_list_subscribe", onBatch, { accountId });
}

/**
 * Unsubscribe exactly one room-list subscription, aborting its backend producer
 * task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeRoomList(accountId: string, id: number): Promise<void> {
  await invoke<void>("room_list_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to the merged unified inbox across every restorable account (FR-18,
 * AD-20). Opens a `Channel`, forwards each {@link InboxBatch} to `onBatch` in
 * arrival order (a recency-ordered `Reset` window that updates as accounts sync
 * or are added/removed), and resolves with the inbox subscription id. Ordering
 * and filtering are computed in Rust — never re-derived here. Rejects with the
 * {@link IpcError} envelope (`code: "syncUnavailable"`) on a stream-start failure.
 */
export async function subscribeInbox(onBatch: (batch: InboxBatch) => void): Promise<number> {
  return await subscribe<InboxBatch>("inbox_subscribe", onBatch);
}

/**
 * Unsubscribe the merged inbox, aborting every per-account producer feeding it
 * (AD-20). Idempotent — a mismatched/unknown id is a no-op.
 */
export async function unsubscribeInbox(id: number): Promise<void> {
  await invoke<void>("inbox_unsubscribe", { subscriptionId: id });
}

/**
 * Subscribe to a room's timeline (FR-8, FR-9, AD-4/AD-8). Opens a `Channel`,
 * forwards each {@link TimelineBatch} to `onBatch` in arrival order (a `Reset`
 * snapshot before any diff), and resolves with the subscription id. Rejects with
 * the {@link IpcError} envelope (`code: "timelineUnavailable"`) if the room's
 * timeline cannot be opened.
 */
export async function subscribeTimeline(
  accountId: string,
  roomId: string,
  onBatch: (batch: TimelineBatch) => void,
): Promise<number> {
  return await subscribe<TimelineBatch>("timeline_subscribe", onBatch, { accountId, roomId });
}

/**
 * Unsubscribe exactly one timeline subscription, aborting its backend producer
 * task and dropping its `Timeline` (AD-19). Idempotent — unsubscribing an
 * unknown id is a no-op.
 */
export async function unsubscribeTimeline(accountId: string, id: number): Promise<void> {
  await invoke<void>("timeline_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to an account's connection status (FR-8/FR-9, UX-DR18, AD-8). Opens a
 * `Channel`, forwards each {@link ConnectionStatusBatch} to `onBatch` in arrival
 * order (an initial snapshot before any change), and resolves with the
 * subscription id. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeConnectionStatus(
  accountId: string,
  onBatch: (batch: ConnectionStatusBatch) => void,
): Promise<number> {
  return await subscribe<ConnectionStatusBatch>("connection_status_subscribe", onBatch, {
    accountId,
  });
}

/**
 * Unsubscribe exactly one connection-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeConnectionStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("connection_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to an account's encryption (device-verification) status (Story 3.1,
 * AD-8). Opens a `Channel`, forwards each {@link EncryptionStatusBatch} to
 * `onBatch` in arrival order (an initial snapshot before any change), and resolves
 * with the subscription id. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeEncryptionStatus(
  accountId: string,
  onBatch: (batch: EncryptionStatusBatch) => void,
): Promise<number> {
  return await subscribe<EncryptionStatusBatch>("encryption_status_subscribe", onBatch, {
    accountId,
  });
}

/**
 * Unsubscribe exactly one encryption-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeEncryptionStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("encryption_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to an account's interactive device self-verification flow (Story 3.2,
 * FR-14, AD-1). Opens a `Channel`, forwards each {@link VerificationFlowVm}
 * snapshot to `onBatch` in arrival order (the flow's state machine: waiting →
 * compare emoji / show QR → confirmed → done/cancelled/failed), and resolves with
 * the subscription id. An *incoming* request the peer started surfaces here as a
 * `requested` snapshot so the UI can auto-open the modal. NO crypto/key/plaintext
 * crosses IPC — only the rendered VM. Rejects with the {@link IpcError} envelope
 * (`code: "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeVerification(
  accountId: string,
  onBatch: (batch: VerificationFlowVm) => void,
): Promise<number> {
  return await subscribe<VerificationFlowVm>("verification_subscribe", onBatch, { accountId });
}

/**
 * Unsubscribe exactly one verification subscription, aborting its backend producer
 * task and clearing the account's flow sender (AD-19). Idempotent — unsubscribing
 * an unknown id is a no-op.
 */
export async function unsubscribeVerification(accountId: string, id: number): Promise<void> {
  await invoke<void>("verification_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Start an interactive self-verification from keeper against the user's other
 * session (Story 3.2, FR-14). The Rust core requests the verification and streams
 * the resulting flow over the existing verification subscription. Requires an
 * active verification subscription. Rejects with the {@link IpcError} envelope
 * (`code: "verificationFailed"`) on failure.
 */
export async function verificationStart(accountId: string): Promise<void> {
  await invoke<void>("verification_start", { accountId });
}

/**
 * Accept an incoming verification request the peer started (Story 3.2). Moves the
 * flow from `requested` to `ready`. `flowId` is the flow's opaque id from the
 * streamed {@link VerificationFlowVm}. Rejects with the {@link IpcError} envelope
 * (`code: "verificationFailed"`) on failure.
 */
export async function verificationAccept(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_accept", { accountId, flowId });
}

/**
 * Start the emoji/SAS sub-flow on a ready request (Story 3.2). The SAS state
 * transition arrives over the verification stream. Rejects with the
 * {@link IpcError} envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationStartSas(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_start_sas", { accountId, flowId });
}

/**
 * Confirm the SAS emoji match on our side (Story 3.2). When both sides confirm,
 * the SDK completes verification and Story 3.1's encryption-status stream flips
 * the account to `verified`. Rejects with the {@link IpcError} envelope (`code:
 * "verificationFailed"`) on failure.
 */
export async function verificationConfirm(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_confirm", { accountId, flowId });
}

/**
 * Signal that the SAS emoji do NOT match (Story 3.2). Cancels the flow with the
 * SDK mismatch code, which surfaces as `failed`. Rejects with the {@link IpcError}
 * envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationMismatch(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_mismatch", { accountId, flowId });
}

/**
 * Cancel the verification flow (Story 3.2) — the user closed the modal or pressed
 * Esc. Cancels the active SAS or the request; a missing flow is a no-op. Rejects
 * with the {@link IpcError} envelope (`code: "verificationFailed"`) on failure.
 */
export async function verificationCancel(accountId: string, flowId: string): Promise<void> {
  await invoke<void>("verification_cancel", { accountId, flowId });
}

/**
 * Subscribe to an account's server-side key-backup status (Story 3.3, FR-14,
 * AD-8). Opens a `Channel`, forwards each {@link BackupStatus} to `onStatus` in
 * arrival order (an initial snapshot before any change), and resolves with the
 * subscription id. NO recovery key or secret-storage material crosses IPC — only
 * the enum tag. Rejects with the {@link IpcError} envelope (`code:
 * "syncUnavailable"`) if the account cannot start syncing.
 */
export async function subscribeBackupStatus(
  accountId: string,
  onStatus: (status: BackupStatus) => void,
): Promise<number> {
  return await subscribe<BackupStatus>("backup_status_subscribe", onStatus, { accountId });
}

/**
 * Unsubscribe exactly one backup-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — unsubscribing an unknown id is a no-op.
 */
export async function unsubscribeBackupStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("backup_status_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Enable server-side key backup for the account (Story 3.3, FR-14). The Rust core
 * creates the backup + secret store and returns the base58 **recovery key** once —
 * the deliberate boundary exception, meant for the human to save (shown once in
 * `mono`, never persisted in a JS store beyond the modal's lifecycle). A race with
 * an existing server backup rejects with the {@link IpcError} envelope (`code:
 * "backupExists"`) so the modal can offer restore; any other failure rejects with
 * `code: "backupFailed"`.
 */
export async function backupEnable(accountId: string): Promise<string> {
  return await invoke<string>("backup_enable", { accountId });
}

/**
 * Restore from server-side key backup with a recovery key (Story 3.3, FR-14). The
 * Rust core opens the secret store and imports secrets; the SDK then downloads
 * room keys automatically, so Story 3.1's streams re-render previously
 * undecryptable rows with no extra code. An invalid key rejects with the
 * {@link IpcError} envelope carrying a *named* code — `"backupMalformedKey"` (not
 * decodable) vs `"backupIncorrectKey"` (well-formed but wrong) — never a generic
 * failure. `recoveryKey` is transient — never stored in a JS store beyond the
 * modal's lifecycle.
 */
export async function backupRestore(accountId: string, recoveryKey: string): Promise<void> {
  await invoke<void>("backup_restore", { accountId, recoveryKey });
}

/**
 * Save a recovery key to the OS Keychain (Story 3.3, FR-14) — the user's opt-in
 * after seeing the key once. The Rust core writes it at `recovery_key/<accountId>`
 * via the platform keychain port. Rejects with the {@link IpcError} envelope on a
 * write failure so the modal can keep the key visible for manual copy.
 */
export async function backupSaveRecoveryKey(accountId: string, recoveryKey: string): Promise<void> {
  await invoke<void>("backup_save_recovery_key", { accountId, recoveryKey });
}

/**
 * Read a previously-saved recovery key from the OS Keychain (Story 3.3) to prefill
 * the restore textarea, or `null` if none was saved. The Rust `Option<String>`
 * serializes to `string | null`.
 */
export async function backupSavedRecoveryKey(accountId: string): Promise<string | null> {
  return await invoke<string | null>("backup_saved_recovery_key", { accountId });
}

/**
 * Send a plain-text message to a room (FR-9, AD-13). Delegates to the single Rust
 * dispatch gate; the message's local echo and every send-state transition arrive
 * back over the room's existing timeline subscription (no echo is synthesized
 * here). Resolves on successful enqueue; rejects with the {@link IpcError}
 * envelope (`code: "sendFailed"`, `retriable: true`) on an enqueue-time failure.
 */
export async function sendText(accountId: string, roomId: string, body: string): Promise<void> {
  await invoke<void>("send_text", { accountId, roomId, body });
}

/**
 * Send a plain-text reply to a message (FR-10, AD-13, Story 3.4). `inReplyToKey`
 * is the *original* message's opaque render `key` (`unique_id`); the Rust core
 * resolves it to the event id and enqueues the reply through the single dispatch
 * gate. The reply's local echo (with its own quoted-original preview) and every
 * send-state transition arrive back over the room's existing timeline
 * subscription (no echo is synthesized here). Resolves on successful enqueue;
 * rejects with the {@link IpcError} envelope (`code: "sendFailed"`) on failure —
 * `retriable: false` when the reply target is gone.
 */
export async function sendReply(
  accountId: string,
  roomId: string,
  inReplyToKey: string,
  body: string,
): Promise<void> {
  await invoke<void>("send_reply", { accountId, roomId, inReplyToKey, body });
}

/**
 * Edit an own text message in place (FR-11, AD-13, Story 3.4). `itemKey` is the
 * message's opaque render `key` (`unique_id`); the Rust core resolves it, gates on
 * editability (own + text), and enqueues the edit through the single dispatch
 * gate. The `Set` diff that updates the content in place (and flips `isEdited`)
 * arrives back over the room's existing timeline subscription. Resolves on
 * successful enqueue; rejects with the {@link IpcError} envelope (`code:
 * "sendFailed"`) on failure — `retriable: false` when the target is gone or not
 * editable.
 */
export async function editMessage(
  accountId: string,
  roomId: string,
  itemKey: string,
  body: string,
): Promise<void> {
  await invoke<void>("edit_message", { accountId, roomId, itemKey, body });
}

/**
 * Toggle the account's emoji reaction on a message (FR-12, AD-13, Story 3.5).
 * `itemKey` is the message's opaque render `key` (`unique_id`); the Rust core
 * resolves it and calls the SDK's `toggle_reaction` through the single dispatch
 * gate — adding the reaction if absent, retracting it if the account already
 * reacted with `emoji`. The updated pill state arrives back over the room's
 * existing timeline subscription as a `Set` diff (nothing is stored or synthesized
 * on the frontend). Resolves on successful dispatch; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on failure — `retriable: false`
 * when the target is gone.
 */
export async function toggleReaction(
  accountId: string,
  roomId: string,
  itemKey: string,
  emoji: string,
): Promise<void> {
  await invoke<void>("toggle_reaction", { accountId, roomId, itemKey, emoji });
}

/**
 * Retry a failed outgoing message by re-driving its wedged local echo through the
 * controlled send path (`unwedge`, not a new dispatch). `itemKey` is the timeline
 * item's opaque `key` (`unique_id`). Rejects with the {@link IpcError} envelope
 * (`code: "sendFailed"`) if the echo is gone or the room has no open timeline.
 */
export async function retrySend(accountId: string, roomId: string, itemKey: string): Promise<void> {
  await invoke<void>("send_retry", { accountId, roomId, itemKey });
}

/**
 * Send a media attachment from an OS file path (FR-13, AD-4, Story 3.7). The
 * composer attach button and native drag-drop both deliver a **path** — the Rust
 * core reads the file itself, so no media bytes cross IPC. `caption` is the trimmed
 * composer text (omit when empty). The local echo + every send-state transition
 * arrive back over the room's existing timeline subscription (no echo is
 * synthesized here). Resolves on successful enqueue; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on an enqueue-time failure.
 */
export async function sendAttachmentPath(
  accountId: string,
  roomId: string,
  path: string,
  caption?: string,
): Promise<void> {
  await invoke<void>("send_attachment_path", {
    accountId,
    roomId,
    path,
    caption: caption ?? null,
  });
}

/**
 * Send a path-less pasted clipboard image (FR-13, AD-4, Story 3.7). The image
 * **bytes** ride as a **raw binary IPC body** (never base64/JSON — the sanctioned
 * exception for pastes with no OS path), with `accountId`/`roomId`/`filename`/
 * `mime`/`caption` in **request headers** (filename + caption percent-encoded so
 * non-ASCII survives an ASCII-only header). The Rust core reads the raw body,
 * decodes the headers, and enqueues the attachment through the single dispatch
 * gate; the local echo + send-state transitions arrive over the room's existing
 * timeline subscription. Resolves on successful enqueue; rejects with the
 * {@link IpcError} envelope (`code: "sendFailed"`) on failure.
 */
export async function sendAttachmentBytes(
  accountId: string,
  roomId: string,
  bytes: ArrayBuffer,
  filename: string,
  mime: string,
  caption?: string,
): Promise<void> {
  const headers: Record<string, string> = {
    "x-account-id": accountId,
    "x-room-id": roomId,
    // Percent-encode text that may contain non-ASCII (filename/caption); the Rust
    // side percent-decodes. ASCII-safe values (ids/mime) ride verbatim.
    "x-filename": encodeURIComponent(filename),
    "x-mime": mime,
  };
  if (caption != null && caption !== "") {
    headers["x-caption"] = encodeURIComponent(caption);
  }
  try {
    // Raw-body invoke: the `ArrayBuffer` becomes the `InvokeBody::Raw` payload;
    // metadata rides in headers. `invoke` in `@tauri-apps/api/core` maps a
    // rejection to a value, so mirror the shared client's IpcError normalization.
    await tauriInvoke<void>("send_attachment_bytes", bytes, { headers });
  } catch (raw) {
    if (isIpcError(raw)) {
      throw raw;
    }
    throw {
      code: "internal",
      message: typeof raw === "string" ? raw : "unexpected IPC failure",
      accountId: null,
      retriable: false,
    } satisfies IpcError;
  }
}

/**
 * Cancel an in-flight outgoing echo by aborting its SDK send handle (best-effort,
 * Story 3.7). `itemKey` is the echo's opaque render `key` (`unique_id`). If the
 * send already dispatched, the abort is a no-op and the message stays sent (the
 * echo's removal or its no-op arrives over the room's existing timeline
 * subscription). Rejects with the {@link IpcError} envelope (`code: "sendFailed"`)
 * if the echo is gone or the room has no open timeline.
 */
export async function cancelSend(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<void> {
  await invoke<void>("cancel_send", { accountId, roomId, itemKey });
}
