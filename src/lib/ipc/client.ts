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
export type { EditVersionVm } from "./gen/EditVersionVm";
export type { EncryptionStatus } from "./gen/EncryptionStatus";
export type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
export type { InboxBatch } from "./gen/InboxBatch";
export type { InboxOp } from "./gen/InboxOp";
export type { InboxRoomVm } from "./gen/InboxRoomVm";
export type { IpcError } from "./gen/IpcError";
export type { IpcErrorCode } from "./gen/IpcErrorCode";
export type { MediaKindVm } from "./gen/MediaKindVm";
export type { MediaVm } from "./gen/MediaVm";
export type { NetworksSnapshot } from "./gen/NetworksSnapshot";
export type { NetworkVm } from "./gen/NetworkVm";
export type { PaginationState } from "./gen/PaginationState";
export type { PaginationStatusBatch } from "./gen/PaginationStatusBatch";
export type { PingVm } from "./gen/PingVm";
export type { Provider } from "./gen/Provider";
export type { ReactionGroupVm } from "./gen/ReactionGroupVm";
export type { ReplyPreviewVm } from "./gen/ReplyPreviewVm";
export type { RoomListBatch } from "./gen/RoomListBatch";
export type { RoomListOp } from "./gen/RoomListOp";
export type { RoomVm } from "./gen/RoomVm";
export type { SasEmojiVm } from "./gen/SasEmojiVm";
export type { SendState } from "./gen/SendState";
export type { SpacesSnapshot } from "./gen/SpacesSnapshot";
export type { SpaceVm } from "./gen/SpaceVm";
export type { TimelineBatch } from "./gen/TimelineBatch";
export type { TimelineItemVm } from "./gen/TimelineItemVm";
export type { TimelineOp } from "./gen/TimelineOp";
export type { TypingBatch } from "./gen/TypingBatch";
export type { TypistVm } from "./gen/TypistVm";
export type { VerificationFlowVm } from "./gen/VerificationFlowVm";
export type { VerificationPhase } from "./gen/VerificationPhase";

import type { AccountVm } from "./gen/AccountVm";
import type { BackupStatus } from "./gen/BackupStatus";
import type { ConnectionStatusBatch } from "./gen/ConnectionStatusBatch";
import type { EditVersionVm } from "./gen/EditVersionVm";
import type { EncryptionStatusBatch } from "./gen/EncryptionStatusBatch";
import type { InboxBatch } from "./gen/InboxBatch";
import type { NetworksSnapshot } from "./gen/NetworksSnapshot";
import type { PaginationStatusBatch } from "./gen/PaginationStatusBatch";
import type { RoomListBatch } from "./gen/RoomListBatch";
import type { SpacesSnapshot } from "./gen/SpacesSnapshot";
import type { TimelineBatch } from "./gen/TimelineBatch";
import type { TypingBatch } from "./gen/TypingBatch";
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
 * Read a message's edit history from the Local Archive (FR-11, Story 5.2).
 * `itemKey` is the message's opaque render `key` (`unique_id`); the Rust core
 * resolves it to the original event id and reads the version chain from
 * `archive.db` — never a homeserver fetch. Resolves with an ordered
 * {@link EditVersionVm}[] (oldest→newest, the last flagged `isCurrent`), or an
 * empty array when the item is unresolvable or has no local history.
 */
export async function getEditHistory(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<EditVersionVm[]> {
  return await invoke<EditVersionVm[]>("edit_history_get", { accountId, roomId, itemKey });
}

/**
 * Read the app-wide "honor remote deletions locally" policy (FR-36, Story 5.2).
 * Resolves with `true` only when explicitly enabled; absent/off ⇒ `false`
 * (preserve). Read-time policy only — flipping it is never retroactive.
 */
export async function honorRemoteDeletions(): Promise<boolean> {
  return await invoke<boolean>("honor_remote_deletions");
}

/**
 * Persist the app-wide "honor remote deletions locally" policy (FR-36, Story
 * 5.2). Affects subsequent reads only (not retroactive). Resolves once persisted.
 */
export async function setHonorRemoteDeletions(enabled: boolean): Promise<void> {
  await invoke<void>("set_honor_remote_deletions", { enabled });
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
 * AD-20, Story 4.2 + 4.3 + 4.4). Opens **four** `Channel`s over one subscription
 * and forwards the recency-ordered Inbox window to `onInbox`, the Archive window
 * to `onArchive`, the Pins window (pinned rooms, user order) to `onPins`, and the
 * Favorites window (favourited rooms, recency order) to `onFavourites` (each a
 * `Reset` window that updates as accounts sync or as archive/pin/favourite state
 * changes). Resolves with the inbox subscription id — one
 * {@link unsubscribeInbox} tears down all four. Ordering and the four-way split
 * are computed in Rust — never re-derived here. Rejects with the {@link IpcError}
 * envelope (`code: "syncUnavailable"`) on a stream-start failure.
 *
 * All channels arm their `onmessage` before `invoke` (the ordering is
 * load-bearing per AD-8, so no batch sent by a spawned task is dropped). The Rust
 * command's params are `channel` (inbox), `archive`, `pins`, `favourites`,
 * `spaces`, and `networks`. The fifth channel (Story 4.5) delivers the aggregated
 * Space list as a whole {@link SpacesSnapshot}; the sixth (Story 4.6) delivers the
 * distinct-Networks list as a whole {@link NetworksSnapshot} (no diff protocol for
 * either — the frontend replaces its list).
 */
export async function subscribeInbox(
  onInbox: (batch: InboxBatch) => void,
  onArchive: (batch: InboxBatch) => void,
  onPins: (batch: InboxBatch) => void,
  onFavourites: (batch: InboxBatch) => void,
  onSpaces: (snapshot: SpacesSnapshot) => void,
  onNetworks: (snapshot: NetworksSnapshot) => void,
): Promise<number> {
  const channel = new Channel<InboxBatch>();
  const archive = new Channel<InboxBatch>();
  const pins = new Channel<InboxBatch>();
  const favourites = new Channel<InboxBatch>();
  const spaces = new Channel<SpacesSnapshot>();
  const networks = new Channel<NetworksSnapshot>();
  channel.onmessage = onInbox;
  archive.onmessage = onArchive;
  pins.onmessage = onPins;
  favourites.onmessage = onFavourites;
  spaces.onmessage = onSpaces;
  networks.onmessage = onNetworks;
  return await invoke<number>("inbox_subscribe", {
    channel,
    archive,
    pins,
    favourites,
    spaces,
    networks,
  });
}

/**
 * Set (or clear) the ephemeral Space filter on the merged inbox (Story 4.5,
 * FR-22). Pass an `accountId` + `spaceId` to narrow every inbox window to that
 * Space's joined children (the Rust merger re-emits all four windows filtered);
 * pass `null`/`null` to clear and restore the full inbox. The selection is
 * ephemeral — never persisted, cleared on relaunch. Best-effort: callers may
 * fire-and-forget and swallow rejection (the stream is truth). Rejects with the
 * {@link IpcError} envelope only on an unexpected backend failure.
 */
export async function setSpaceFilter(
  accountId: string | null,
  spaceId: string | null,
): Promise<void> {
  await invoke<void>("set_space_filter", { accountId, spaceId });
}

/**
 * Set (or clear) the ephemeral Network filter on the merged inbox (Story 4.6,
 * FR-24). Pass a Network `name` to narrow every inbox window to rooms bridged to
 * that Network (the Rust merger re-emits all four windows filtered, across all
 * accounts — the selection is name-keyed); pass `null` to clear and restore the
 * full inbox. Composes AND with any active Space filter. The selection is ephemeral
 * — never persisted, cleared on relaunch. Best-effort: callers may fire-and-forget
 * and swallow rejection (the stream is truth). Rejects with the {@link IpcError}
 * envelope only on an unexpected backend failure.
 */
export async function setNetworkFilter(network: string | null): Promise<void> {
  await invoke<void>("set_network_filter", { network });
}

/**
 * Unsubscribe the merged inbox, aborting every per-account producer feeding it
 * (AD-20). Idempotent — a mismatched/unknown id is a no-op. Covers the Inbox,
 * Archive, Pins, and Favorites channels (Story 4.2 + 4.3 + 4.4).
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
 * Delete an own message for everyone by issuing a Matrix redaction (FR-15, AD-13,
 * Story 3.8). `itemKey` is the message's opaque render `key` (`unique_id`); the
 * Rust core resolves it and calls the SDK's `redact` through the single dispatch
 * gate (no reason). The `Set` diff that turns the message into a "Message deleted"
 * stub in place arrives back over the room's existing timeline subscription
 * (nothing is synthesized on the frontend). Resolves on successful dispatch;
 * rejects with the {@link IpcError} envelope (`code: "sendFailed"`) on failure —
 * `retriable: false` when the target is gone, `retriable: true` on an SDK dispatch
 * error the dialog can retry.
 */
export async function deleteMessage(
  accountId: string,
  roomId: string,
  itemKey: string,
): Promise<void> {
  await invoke<void>("delete_message", { accountId, roomId, itemKey });
}

/**
 * Resolve the bridged Network label for the delete confirmation on demand (FR-15,
 * UX-DR17, Story 3.8). The Rust core reads the Room's MSC2346 `m.bridge` (and
 * legacy `uk.half-shot.bridge`) state event and returns the Network's display name
 * ("Telegram", "WhatsApp", …), or `null` for a native Matrix Room (no bridge
 * state). The Rust `Option<String>` serializes to `string | null` — only the
 * resolved, non-secret label crosses. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) on an unknown room/account.
 */
export async function roomNetworkLabel(accountId: string, roomId: string): Promise<string | null> {
  return await invoke<string | null>("room_network_label", { accountId, roomId });
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

/**
 * Mark a room read (Story 3.9 receipts, Story 4.1, AD-14). The Rust core dispatches
 * a public `m.read` receipt on the room's latest event through the receipt/typing
 * signals seam — other Matrix clients observe the advance — and clears any manual
 * `m.marked_unread` flag. Works for any inbox row whether or not its timeline is
 * open. Best-effort: a dispatch failure is swallowed in the core (never a UI error),
 * so this resolves even then. Callers may fire-and-forget and swallow rejections.
 * Rejects with the {@link IpcError} envelope (`code: "timelineUnavailable"`) only on
 * an unknown room/inactive account.
 */
export async function markRoomRead(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("mark_room_read", { accountId, roomId });
}

/**
 * Manually mark a room unread (Story 4.1). The Rust core sets the `m.marked_unread`
 * account-data flag (`Room::set_unread_flag(true)`) so the row renders unread and the
 * flag syncs to the user's other Matrix clients. Best-effort: a dispatch failure is
 * swallowed in the core (never a UI error), so this resolves even then. Callers may
 * fire-and-forget and swallow rejections. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function markRoomUnread(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("mark_room_unread", { accountId, roomId });
}

/**
 * Archive a room (Story 4.2). The Rust core sets the Matrix low-priority tag
 * (`m.lowpriority`) via `Room::set_is_low_priority(true, None)` so the row moves into
 * the Archive window (unless it is unread) and the tag persists and syncs to the
 * user's other Matrix clients. Best-effort: a dispatch failure is swallowed in the
 * core (never a UI error), so this resolves even then. Callers may fire-and-forget
 * and swallow rejections. Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function archiveRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("archive_room", { accountId, roomId });
}

/**
 * Unarchive a room (Story 4.2). The Rust core clears the Matrix low-priority tag
 * (`m.lowpriority`) via `Room::set_is_low_priority(false, None)` so the row returns to
 * its chronological Inbox position. Best-effort: a dispatch failure is swallowed in
 * the core (never a UI error), so this resolves even then. Callers may
 * fire-and-forget and swallow rejections. Rejects with the {@link IpcError} envelope
 * (`code: "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function unarchiveRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unarchive_room", { accountId, roomId });
}

/**
 * Favourite a room (Story 4.4, FR-21). The Rust core sets the Matrix favourite tag
 * (`m.favourite`) via `Room::set_is_favourite(true, None)`. Because `m.favourite`
 * is a *notable* tag, the row moves into the Favorites window on the SDK's live
 * re-emit and the tag persists and syncs to the user's other Matrix clients (no
 * out-of-band merger poke). Best-effort: a dispatch failure is swallowed in the
 * core (never a UI error), so this resolves even then. Callers may fire-and-forget
 * and swallow rejections. Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`) only on an unknown room/inactive account.
 */
export async function favoriteRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("favourite_room", { accountId, roomId });
}

/**
 * Unfavourite a room (Story 4.4). The Rust core clears the Matrix favourite tag
 * (`m.favourite`) via `Room::set_is_favourite(false, None)` so the row returns to
 * its chronological Inbox position on the SDK's live re-emit. Best-effort: a
 * dispatch failure is swallowed in the core (never a UI error), so this resolves
 * even then. Callers may fire-and-forget and swallow rejections. Rejects with the
 * {@link IpcError} envelope (`code: "timelineUnavailable"`) only on an unknown
 * room/inactive account.
 */
export async function unfavoriteRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unfavourite_room", { accountId, roomId });
}

/**
 * Read the Favorites section's persisted collapse state (Story 4.4). Pure UI
 * chrome, stored in the app-level `settings` table in `keeper.db` (survives
 * restart and re-login). Resolves `false` (expanded) when unset. Rejects with the
 * {@link IpcError} envelope only on a registry read failure.
 */
export async function getFavoritesCollapsed(): Promise<boolean> {
  return await invoke<boolean>("get_favorites_collapsed");
}

/**
 * Persist the Favorites section's collapse state (Story 4.4). Stores the boolean
 * in the app-level `settings` table so it survives restart and re-login.
 * Best-effort: callers may fire-and-forget and swallow rejections. Rejects with
 * the {@link IpcError} envelope only on a registry write failure.
 */
export async function setFavoritesCollapsed(collapsed: boolean): Promise<void> {
  await invoke<void>("set_favorites_collapsed", { collapsed });
}

/**
 * Pin a room (Story 4.3, FR-22). The Rust core appends the pin at the end of the
 * keeper-local ordered list, persists it to `keeper.db` (pins have no Matrix
 * representation), and re-emits the Pins/Inbox/Archive windows so the strip
 * updates within one frame. Best-effort: callers fire-and-forget and swallow
 * rejection — the stream is truth. Rejects with the {@link IpcError} envelope
 * (`code: "internal"`) only on a registry write failure.
 */
export async function pinRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("pin_room", { accountId, roomId });
}

/**
 * Unpin a room (Story 4.3). The Rust core removes the keeper-local pin ref and
 * re-emits the windows so the row returns to its chronological Inbox (or Archive)
 * position. Best-effort: callers fire-and-forget and swallow rejection. Rejects
 * with the {@link IpcError} envelope only on a registry write failure.
 */
export async function unpinRoom(accountId: string, roomId: string): Promise<void> {
  await invoke<void>("unpin_room", { accountId, roomId });
}

/**
 * Reorder the pins to the exact `order` given (Story 4.3). Each entry is a
 * `{ accountId, roomId }` ref; the Rust core rewrites the keeper-local order to
 * contiguous `0..n` and re-emits the Pins window in that order (authoritative —
 * no optimistic TS overlay). Best-effort: callers fire-and-forget and swallow
 * rejection. Rejects with the {@link IpcError} envelope only on a registry write
 * failure.
 */
export async function reorderPins(order: { accountId: string; roomId: string }[]): Promise<void> {
  await invoke<void>("reorder_pins", { order });
}

/**
 * Set (or clear) the account's typing notice in the open room (Story 3.9, typing,
 * AD-14). The Rust core emits a normal (non-private) typing notification through
 * the receipt/typing signals seam. Best-effort: a dispatch failure is swallowed in
 * the core (typing is never a UI error). Callers fire-and-forget and swallow
 * rejections.
 */
export async function setTyping(accountId: string, roomId: string, typing: boolean): Promise<void> {
  await invoke<void>("set_typing", { accountId, roomId, typing });
}

/**
 * Back-paginate the open room's timeline (Story 3.9, pagination). The Rust core
 * fetches up to `numEvents` older events; they arrive back over the room's existing
 * timeline subscription (no second channel — the store applies the prepend ops).
 * Resolves with whether the homeserver start of the room was reached (no more older
 * history). Rejects with the {@link IpcError} envelope (`code:
 * "timelineUnavailable"`, `retriable: true`) on a pagination failure so the
 * boundary can show a retriable inline error, not an infinite spinner.
 */
export async function paginateBackwards(
  accountId: string,
  roomId: string,
  numEvents: number,
): Promise<boolean> {
  return await invoke<boolean>("paginate_backwards", { accountId, roomId, numEvents });
}

/**
 * Subscribe to the open room's typing notifications (Story 3.9, typing, AD-8,
 * AD-14). Opens a `Channel`, forwards each {@link TypingBatch} (the current set of
 * *other* members typing, each with a resolved display name) to `onBatch` in
 * arrival order (an initial empty snapshot before any change), and resolves with
 * the subscription id. Only opaque user ids + display names cross IPC. Rejects with
 * the {@link IpcError} envelope (`code: "timelineUnavailable"`) if the room isn't
 * open.
 */
export async function subscribeTyping(
  accountId: string,
  roomId: string,
  onBatch: (batch: TypingBatch) => void,
): Promise<number> {
  return await subscribe<TypingBatch>("typing_subscribe", onBatch, { accountId, roomId });
}

/**
 * Unsubscribe exactly one typing subscription, aborting its backend producer task
 * and dropping the SDK typing event handler (AD-19). Idempotent — an unknown id is
 * a no-op.
 */
export async function unsubscribeTyping(accountId: string, id: number): Promise<void> {
  await invoke<void>("typing_unsubscribe", { accountId, subscriptionId: id });
}

/**
 * Subscribe to the open room's live back-pagination status (Story 3.9, pagination,
 * AD-8). Opens a `Channel`, forwards each {@link PaginationStatusBatch} (a scalar
 * snapshot: `paginating`/`idle` + `hitStart`) to `onBatch` in arrival order (an
 * initial snapshot before any change), and resolves with the subscription id. The
 * status drives the honest history-boundary row; older events themselves arrive
 * over the timeline subscription, never here. Rejects with the {@link IpcError}
 * envelope (`code: "timelineUnavailable"`) if the room isn't open.
 */
export async function subscribePaginationStatus(
  accountId: string,
  roomId: string,
  onBatch: (batch: PaginationStatusBatch) => void,
): Promise<number> {
  return await subscribe<PaginationStatusBatch>("pagination_status_subscribe", onBatch, {
    accountId,
    roomId,
  });
}

/**
 * Unsubscribe exactly one pagination-status subscription, aborting its backend
 * producer task (AD-19). Idempotent — an unknown id is a no-op.
 */
export async function unsubscribePaginationStatus(accountId: string, id: number): Promise<void> {
  await invoke<void>("pagination_status_unsubscribe", { accountId, subscriptionId: id });
}
