//! IPC view models (AD-7, AD-8).
//!
//! Every type that crosses the Tauri IPC boundary lives here, derives
//! `serde` + [`ts_rs::TS`], is `#[ts(export)]`, and renames fields to
//! camelCase. Timestamps are `i64` milliseconds since the Unix epoch (UTC) —
//! never strings. Bindings are emitted to `src/lib/ipc/gen/` by the ts-rs
//! export test step (`cargo nextest run`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Response of the `app_ping` liveness command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PingVm {
    /// Backend liveness marker, e.g. `"pong"`.
    pub message: String,
    /// Server-side timestamp: milliseconds since the Unix epoch (UTC).
    ///
    /// Emitted to TypeScript as `number`, not `bigint`: Tauri IPC delivers the
    /// `i64` as a JS number via `JSON.parse`, and ms-epoch values stay well
    /// within `Number.MAX_SAFE_INTEGER`. This keeps the binding matching the
    /// wire reality — the timestamp convention every later VM copies.
    #[ts(type = "number")]
    pub ts: i64,
}

/// Stable, string-serialized error taxonomy for the IPC envelope.
///
/// Variants serialize to their camelCase names (e.g. `"unsupported"`) and are
/// part of the frontend contract — rename with care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum IpcErrorCode {
    /// The requested capability is not supported on this platform/build.
    Unsupported,
    /// An unexpected internal error occurred in the backend.
    Internal,
    /// The homeserver does not support Simplified Sliding Sync (MSC4186).
    SlidingSyncUnsupported,
    /// The supplied username/password was rejected by the homeserver.
    InvalidCredentials,
    /// The homeserver could not be reached (DNS/connection/transport failure).
    ServerUnreachable,
    /// The homeserver does not offer password login (`m.login.password`).
    UnsupportedLoginType,
    /// The homeserver does not offer OIDC (OAuth 2.0 / MSC3861) login.
    /// Non-retriable — the user must pick a different login mechanism.
    /// Serializes as `"oauthUnsupported"`.
    OauthUnsupported,
    /// The OIDC browser round-trip did not complete before the timeout.
    /// Retriable — the sign-in may be started again. Serializes as
    /// `"oauthTimedOut"`.
    OauthTimedOut,
    /// The user cancelled the in-progress OIDC flow. Retriable — the sign-in may
    /// be started again; the UI returns quietly to the form. Serializes as
    /// `"oauthCancelled"`.
    OauthCancelled,
    /// The OIDC flow failed (a server `error=` callback or a token-exchange
    /// failure). Retriable — the sign-in may be started again. Serializes as
    /// `"oauthFailed"`.
    OauthFailed,
    /// The Beeper unofficial email-code login flow is unavailable (Story 2.3):
    /// a non-2xx / timeout / transport failure from `api.beeper.com`, a
    /// missing/renamed field (the private API changed shape), an abandoned flow,
    /// or a JWT / `org.matrix.login.jwt` rejection. Retriable — the UI returns to
    /// the email step to start a fresh flow. Serializes as `"beeperUnavailable"`.
    BeeperUnavailable,
    /// The account could not start (or continue) syncing: the persisted session
    /// was missing, session restore failed, or `SyncService` failed to start.
    /// Retriable — the subscribe may be attempted again.
    SyncUnavailable,
    /// A room's timeline could not be opened: the room was not found or the SDK
    /// `Timeline` failed to build. Retriable — the subscribe may be attempted
    /// again.
    TimelineUnavailable,
    /// An outgoing message could not be enqueued for send (room not found, no
    /// open timeline, the wedged echo was gone, or the SDK dispatch failed).
    /// Retriable — the send may be attempted again. Asynchronous delivery
    /// failures are *not* this code; they surface as the `Failed` send-state on
    /// the timeline item instead.
    SendFailed,
    /// An interactive device self-verification action failed (Story 3.2): crypto
    /// not ready, the flow id was not found, or an SDK action (accept / start_sas
    /// / confirm / mismatch / cancel / request) failed. Retriable — the user can
    /// restart verification. Serializes as `"verificationFailed"`.
    VerificationFailed,
    /// A recovery key pasted for key-backup restore could not be decoded — it is
    /// malformed (wrong length / not a valid base58 recovery key) (Story 3.3,
    /// FR-14). Named so the modal can say "that doesn't look like a recovery key"
    /// rather than a generic failure. Serializes as `"backupMalformedKey"`.
    BackupMalformedKey,
    /// A well-formed recovery key failed the MAC check for this account — it does
    /// not match (Story 3.3, FR-14). Named so the modal can say "recovery key
    /// didn't match this account" rather than a generic failure. Serializes as
    /// `"backupIncorrectKey"`.
    BackupIncorrectKey,
    /// Enabling key backup raced an existing server-side backup: a backup already
    /// exists on the homeserver (Story 3.3). Named so the modal can offer restore
    /// instead of a generic failure. Serializes as `"backupExists"`.
    BackupExists,
    /// A key-backup enable/restore action failed for another reason (crypto not
    /// ready, network, or another SDK error). Retriable — the user can try again.
    /// Serializes as `"backupFailed"`.
    BackupFailed,
    /// A best-effort receipt/typing signal dispatch failed (Story 3.9, AD-14).
    /// Non-retriable and best-effort: in practice receipts/typing are swallowed in
    /// the core (never surfaced to the UI), so this code exists only to keep the
    /// error funnel exhaustive. Serializes as `"signalDispatchFailed"`.
    SignalDispatchFailed,
}

/// The account's live server-side key-backup posture, mapped from the SDK
/// `client.encryption().recovery().state()` (Story 3.3, FR-14, AD-8).
///
/// A Rust-authoritative honest signal streamed over the backup-status channel:
/// `Unknown` before crypto has synced ("Checking…"), `Disabled` when no backup is
/// set up (offer "Set up backup"), `Enabled` once this device is connected to the
/// backup ("Backup on"), `Incomplete` when a backup exists on the server but this
/// device is not yet connected — the fresh-login restore case ("Needs your
/// recovery key"). The Settings backup row is a pure projection of this one
/// status. Only the enum tag crosses IPC — never any key or secret-storage
/// material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BackupStatus {
    /// The recovery state is not yet known — crypto has not synced. Renders
    /// "Checking…" (avoid a false claim before the OlmMachine reports).
    Unknown,
    /// No default secret-storage key exists / recovery is disabled — no backup is
    /// set up. The Settings row offers "Set up backup".
    Disabled,
    /// Secret storage is set up and this device has all the secrets locally —
    /// backup is on. The Settings row reads "Backup on".
    Enabled,
    /// A backup exists on the server but this device is missing some secrets — the
    /// fresh-login restore case. The Settings row offers "Restore".
    Incomplete,
}

/// The delivery state of an outgoing (local-echo) message (FR-9, AD-13, UX-DR10).
///
/// Derived from the SDK `EventSendState` of a local echo: a message being
/// enqueued or retried is `Sending`; a message the server acknowledged is
/// `Sent`; a message whose send failed unrecoverably is `Failed`. A remote
/// (received or reconciled) item has no send state and maps to `None` on the VM.
/// Only the enum tag crosses IPC — never the txn id, error object, or event id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SendState {
    /// The message is being enqueued or is in flight (including a transient,
    /// recoverable failure the send queue is still auto-retrying).
    Sending,
    /// The homeserver acknowledged the message.
    Sent,
    /// The message failed to send unrecoverably; it is actionable via Retry and
    /// its caption never auto-clears.
    Failed,
}

/// The account's live connectivity, as mapped from the SDK `SyncService` state
/// (FR-8/FR-9, UX-DR10, UX-DR18, AD-8).
///
/// A Rust-authoritative signal streamed over the connection-status channel:
/// `Online` when the `SyncService` is `Running`, `Offline` for every other state
/// (`Idle`, `Terminated`, `Error`, `Offline`). The frontend renders the offline
/// pill and the "Queued" send caption as pure projections of this one status —
/// no timeline item is invented or mutated. Only the enum tag crosses IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ConnectionStatus {
    /// The `SyncService` is `Running` — the account is connected and syncing.
    Online,
    /// The `SyncService` is not `Running` — the account is disconnected; sends
    /// queue in the SDK's persistent send queue until connectivity returns.
    Offline,
}

/// A batch delivered over the connection-status subscription's `Channel` (AD-8).
///
/// The status is a scalar snapshot, so each batch carries the full current
/// [`ConnectionStatus`] — inherently idempotent, safe to re-subscribe. The stream
/// opens with the current mapped status, then emits on change (deduped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ConnectionStatusBatch {
    /// The current connectivity status.
    pub status: ConnectionStatus,
}

/// The account's live device-verification (encryption) posture, mapped from the
/// SDK `client.encryption().verification_state()` (Story 3.1, FR, AD-8).
///
/// A Rust-authoritative honest signal streamed over the encryption-status
/// channel: `Unknown` before crypto has synced (never nag), `Verified` once this
/// device's user identity has signed it, `Unverified` for a freshly-logged-in
/// device that cannot yet read encrypted history. The "verify this device" banner
/// and the Settings badge are pure projections of this one status. Only the enum
/// tag crosses IPC — never any key, session, or crypto material.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum EncryptionStatus {
    /// The verification state is not yet known — crypto has not synced. No banner
    /// and no badge (avoid a false nag before the OlmMachine reports).
    Unknown,
    /// This device is verified — its user identity has signed it. The banner and
    /// badge both clear.
    Verified,
    /// This device is unverified — encrypted history is locked until the user
    /// verifies it (Story 3.2) or restores key backup (Story 3.3). Drives the
    /// banner / badge.
    Unverified,
}

/// A batch delivered over the encryption-status subscription's `Channel` (AD-8).
///
/// The status is a scalar snapshot, so each batch carries the full current
/// [`EncryptionStatus`] — inherently idempotent, safe to re-subscribe. The stream
/// opens with the current mapped status, then emits on change (deduped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct EncryptionStatusBatch {
    /// The current device-verification status.
    pub status: EncryptionStatus,
}

/// One emoji of the SAS short-authentication string (Story 3.2, FR-14, NFR-9).
///
/// A rendered projection of the SDK `Emoji` — its Unicode `symbol` and the
/// human-readable `name` (the SDK's `description`). Both are non-secret display
/// strings; NO SAS key, decimal, or crypto material crosses IPC on this VM. The
/// webview renders the symbol with its `name` in `mono` type (epic typography).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SasEmojiVm {
    /// The emoji symbol (e.g. `"🐶"`).
    pub symbol: String,
    /// The emoji's human-readable name (e.g. `"Dog"`).
    pub name: String,
}

/// The phase of an interactive self-verification flow (Story 3.2, FR-14,
/// UX verification-flow states).
///
/// A Rust-authoritative projection of the SDK's native `VerificationRequestState`
/// / `SasState` machine. The webview renders each phase distinctly (waiting,
/// comparing, confirmed, done, cancelled, failed) using the SDK's own vocabulary —
/// it never invents crypto UX. Only the enum tag crosses IPC. `Cancelled` and
/// `Failed` are intentionally distinct: a clean user/peer cancel is `Cancelled`;
/// a mismatch / timeout / other terminal cancel code is `Failed` (with a reason).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum VerificationPhase {
    /// A request exists but is not yet ready — waiting for the other device to
    /// accept (or for us to accept an incoming request).
    Requested,
    /// The request is ready; a QR code may be shown and SAS can be started.
    Ready,
    /// SAS keys are exchanged — the two sides compare the emoji.
    Comparing,
    /// We confirmed the emoji match; waiting for the other device to confirm.
    Confirmed,
    /// The verification completed successfully. Story 3.1's `verification_state()`
    /// stream then flips the account to `Verified`, clearing the banner/badge.
    Done,
    /// The flow was cleanly cancelled (by the user or the peer).
    Cancelled,
    /// The flow failed (emoji mismatch, timeout, or another terminal cancel
    /// code). Carries a human-readable `reason`.
    Failed,
}

/// A snapshot of an interactive self-verification flow, delivered over the
/// verification subscription's `Channel` (Story 3.2, FR-14, AD-1, NFR-9).
///
/// The single view model the webview renders for the whole flow. Carries **only**
/// non-secret render data: the opaque `flow_id`, the current [`VerificationPhase`],
/// the SAS emoji list (symbols + names) when comparing, a pre-rendered QR SVG
/// string when a QR is available, and a human `reason` on cancel/failure. NO
/// `Verification`/`Sas`/`QrVerification` object, SAS key, decimal, or plaintext
/// ever crosses IPC on this VM (AD-1). Actions reference the flow by `flow_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VerificationFlowVm {
    /// The SDK verification flow id (opaque, passed through verbatim). Actions
    /// (accept/start_sas/confirm/mismatch/cancel) reference the flow by this id.
    pub flow_id: String,
    /// The current flow phase.
    pub phase: VerificationPhase,
    /// The 7 SAS emoji to compare, present only in the `Comparing` phase.
    pub emojis: Option<Vec<SasEmojiVm>>,
    /// A pre-rendered QR-code SVG string (keeper's own QR for the peer to scan),
    /// present when a QR is available in the `Ready` phase.
    pub qr_code_svg: Option<String>,
    /// A human-readable reason, present on `Cancelled` / `Failed`.
    pub reason: Option<String>,
}

/// A single room row rendered in the chat list (FR-8, NFR-9, AD-20).
///
/// Carries **only** non-secret render data. `timestamp` is `i64` milliseconds
/// since the Unix epoch (UTC) — never an ISO string. `lastMessage` is the
/// plain-text body of the room's latest event when it is an `m.room.message`
/// (text/notice/emote); `null` for any other event kind. No tokens, session
/// material, or event ids cross IPC on this VM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomVm {
    /// Opaque Matrix room id (passed through verbatim as a string).
    pub room_id: String,
    /// The SDK-computed room display name.
    pub display_name: String,
    /// Plain-text preview of the latest `m.room.message`, or `null`.
    pub last_message: Option<String>,
    /// Latest-event timestamp: ms since the Unix epoch (UTC), or `null`.
    #[ts(type = "number | null")]
    pub timestamp: Option<i64>,
    /// Optional room avatar URL (an `mxc://` URI), or `null`.
    pub avatar_url: Option<String>,
    /// Authoritative unread flag: `true` when the room has unread messages,
    /// unread mentions, or the manual `m.marked_unread` flag set (AD-20). The
    /// frontend renders this directly (bold name + dot/badge) and never
    /// re-derives it from events.
    pub is_unread: bool,
    /// Count of unread mentions (client-side, precise for E2EE). Drives the
    /// filled primary mention badge; a value of 0 shows a plain dot when
    /// `is_unread` is otherwise set.
    #[ts(type = "number")]
    pub mention_count: u32,
    /// Authoritative archive flag: `true` when the room carries the Matrix
    /// low-priority tag (`m.lowpriority`) (Story 4.2, AD-20). The inbox merge
    /// partitions on this to place the room in the Archive window unless it is
    /// unread (auto-return is a pure view rule); the frontend never re-derives it.
    pub is_archived: bool,
    /// Authoritative favourite flag: `true` when the room carries the Matrix
    /// favourite tag (`m.favourite`) (Story 4.4, AD-20). This is a *notable* tag,
    /// so a change re-emits the room-list stream live and syncs cross-client. The
    /// inbox merge partitions on this to place the room in the Favorites window
    /// (removed from Inbox/Archive); the frontend never re-derives it.
    pub is_favourite: bool,
}

/// One index-based room-list operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-20).
///
/// The SDK's `entries_with_dynamic_adapters` stream is recency-sorted; keeper
/// forwards its `VectorDiff` sequence verbatim as these ops. The frontend
/// applies them to a plain array by index and **never** re-sorts. Serialized as
/// an internally tagged enum so the frontend can switch on `op`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum RoomListOp {
    /// Full reset — replace the current contents with `rooms`.
    Reset {
        /// The complete current window, in order.
        rooms: Vec<RoomVm>,
    },
    /// Append `rooms` to the end, in order.
    Append {
        /// Rooms to append.
        rooms: Vec<RoomVm>,
    },
    /// Remove all rooms.
    Clear,
    /// Insert `room` at the front (index 0).
    PushFront {
        /// The room to prepend.
        room: RoomVm,
    },
    /// Append `room` to the end.
    PushBack {
        /// The room to append.
        room: RoomVm,
    },
    /// Remove the first room.
    PopFront,
    /// Remove the last room.
    PopBack,
    /// Insert `room` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The room to insert.
        room: RoomVm,
    },
    /// Replace the room at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement room.
        room: RoomVm,
    },
    /// Remove the room at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
    /// Truncate the list to `length` rooms.
    Truncate {
        /// The new length.
        #[ts(type = "number")]
        length: u32,
    },
}

/// A batch of room-list ops delivered over the subscription's `Channel` (AD-8).
///
/// The stream always opens with a batch whose first op is a
/// [`RoomListOp::Reset`] carrying the current window, then diff batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RoomListBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<RoomListOp>,
    /// The total number of rooms the server knows about, when known.
    #[ts(type = "number | null")]
    pub total: Option<u32>,
}

/// The quoted-original preview of a reply message (Story 3.4, FR-10, NFR-9).
///
/// Derived in the timeline producer from `content.in_reply_to()`. Carries
/// **only** non-secret render data: the resolved *original* item's opaque render
/// `key` when it is loaded in the timeline (so the frontend can scroll to it),
/// the original sender's Matrix user id, a resolved display name, and the decoded
/// plain-text body (empty when the original is non-text). NO event ids, txn ids,
/// or raw event JSON cross IPC on this VM (AD-1) — the jump target is the same
/// opaque `key` (unique_id) used everywhere, resolved in Rust via the producer's
/// `event_id → unique_id` index. When the original is not loaded, `in_reply_to_key`
/// is `null` and the quote renders honestly but is not clickable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReplyPreviewVm {
    /// The *original* (replied-to) item's opaque render key (its `unique_id`)
    /// when that original is currently loaded in the timeline, else `null`. The
    /// frontend uses it to scroll the original into view; never an event id.
    pub in_reply_to_key: Option<String>,
    /// The original sender's Matrix user id (opaque, passed through verbatim).
    pub sender: String,
    /// The original sender's resolved display name, or `null` when unavailable.
    pub sender_display_name: Option<String>,
    /// The decoded plain-text body of the original message, or an empty string
    /// when the original is non-text or its details are unavailable.
    pub body: String,
}

/// One aggregated emoji-reaction group on a timeline message (Story 3.5, FR-12,
/// NFR-9).
///
/// Derived in the timeline producer from `content.reactions()` — one group per
/// distinct emoji key, in the SDK's per-key insertion order. Carries **only**
/// non-secret render data: the emoji string, the count of distinct reactors, and
/// whether the current account is one of them. NO per-sender user ids, reaction
/// event ids, or relation logic ever cross IPC on this VM (AD-1) — those stay
/// inside `keeper-core`. The frontend renders a click-to-toggle pill from these
/// three fields alone and dispatches a toggle by the message's opaque render key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ReactionGroupVm {
    /// The reaction emoji / key (an arbitrary Matrix reaction string, passed
    /// through verbatim).
    pub emoji: String,
    /// The number of distinct reactors for this emoji (per-sender uniqueness is
    /// guaranteed by the SDK, so this is the inner sender-map length).
    #[ts(type = "number")]
    pub count: u32,
    /// Whether the current account has reacted with this emoji (its own user id
    /// is present in the emoji's inner sender map). Drives the own-highlight pill.
    pub is_own: bool,
}

/// The media class of an attached message (Story 3.6, FR-13, AD-4, NFR-9).
///
/// A Rust-authoritative projection of the media `MessageType` (`Image`/`Video`/
/// `Audio`/`File`) — the only render-facing discriminant the frontend needs to
/// pick a renderer (thumbnail image / video poster / inline audio / file chip).
/// Serializes to its camelCase name. NO `mxc`/`EncryptedFile`/key material is ever
/// implied by this tag — the bytes travel only over the `keeper-media://` protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MediaKindVm {
    /// An image attachment (`m.image`). Renders a thumbnail; opens full-res in the
    /// preview overlay.
    Image,
    /// A video attachment (`m.video`). Renders a poster; plays via `<video>` over
    /// the Range protocol in the overlay.
    Video,
    /// An audio attachment (`m.audio`). Plays inline via `<audio controls>` over
    /// the protocol.
    Audio,
    /// An arbitrary file attachment (`m.file`). Renders a file chip (icon + name +
    /// size); no auto-download of bytes over IPC.
    File,
}

/// The render-facing metadata of a media attachment on a message (Story 3.6,
/// FR-13, AD-4, NFR-9).
///
/// Carries **only** opaque `keeper-media://` URL strings plus display metadata —
/// never a `MediaSource`, `EncryptedFile`, `mxc://` URI, decryption key, or event
/// id (those stay inside `keeper-core`). `url` is the full-content protocol URL;
/// `thumbnail_url` is the thumbnail-variant protocol URL when a thumbnail is
/// available. The decrypted bytes are served exclusively over the
/// `keeper-media://` custom protocol (AD-4) — never as base64/JSON over IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MediaVm {
    /// The media class (image/video/audio/file), driving the renderer choice.
    pub kind: MediaKindVm,
    /// The opaque `keeper-media://…/full` protocol URL for the full content. The
    /// preview overlay and inline audio/video load from this; the SDK decrypts
    /// E2EE bytes behind the protocol handler. Never an `mxc` URI.
    pub url: String,
    /// The opaque `keeper-media://…/thumb` protocol URL for the thumbnail variant,
    /// present when a thumbnail is renderable (image/video), else `null`. The
    /// bubble renders this before the full content loads. Never an `mxc` URI.
    pub thumbnail_url: Option<String>,
    /// The attachment's display filename (from `.filename()`, falling back to the
    /// message body). Rendered in the file chip and as the media alt text.
    pub filename: String,
    /// The attachment's MIME type from `info.mimetype` (e.g. `"image/png"`), or
    /// `null` when the sender omitted it.
    pub mimetype: Option<String>,
    /// The attachment size in bytes from `info.size`, or `null` when omitted. The
    /// file chip renders a human-readable size from this.
    #[ts(type = "number | null")]
    pub size: Option<u32>,
    /// The intrinsic width in pixels (image/video `info.w`), or `null`. Used to
    /// reserve layout so the thumbnail does not reflow on load.
    #[ts(type = "number | null")]
    pub width: Option<u32>,
    /// The intrinsic height in pixels (image/video `info.h`), or `null`. Used to
    /// reserve layout so the thumbnail does not reflow on load.
    #[ts(type = "number | null")]
    pub height: Option<u32>,
    /// The media caption (the message `body` when it differs from the filename),
    /// or `null`. Rendered under the attachment.
    pub caption: Option<String>,
}

/// A single timeline item rendered in the conversation pane (FR-8, NFR-9,
/// AD-8/AD-9/AD-20).
///
/// Carries **only** non-secret render data. `timestamp` is `i64` milliseconds
/// since the Unix epoch (UTC) — never an ISO string. Exactly one VM is produced
/// per SDK `TimelineItem` so diff indices stay aligned; virtual, state,
/// redacted, undecryptable, and non-text items become an [`TimelineItemVm::Other`]
/// carrying only a stable opaque `key`. No tokens, session material, event raw
/// JSON, or crypto state cross IPC on this VM. Serialized as an internally
/// tagged enum so the frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum TimelineItemVm {
    /// A renderable text message (`m.room.message` of msgtype text/notice/emote).
    Message {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The decoded plain-text body of the already-decrypted message
        /// (defensively truncated before crossing IPC).
        body: String,
        /// The message origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
        /// Whether the current account sent this message.
        is_own: bool,
        /// The delivery state of an outgoing local echo, or `null` for a remote
        /// (received or reconciled) message that carries no send state.
        send_state: Option<SendState>,
        /// Whether this message has been edited (`message.is_edited()`). The
        /// bubble renders an "Edited" caption when `true` (Story 3.4, FR-11).
        is_edited: bool,
        /// The quoted-original preview when this message is a reply
        /// (`content.in_reply_to()`), else `null` (Story 3.4, FR-10).
        reply: Option<ReplyPreviewVm>,
        /// The aggregated emoji-reaction groups on this message, in the SDK's
        /// per-key insertion order (empty when none) (Story 3.5, FR-12). Each
        /// group carries only `{ emoji, count, is_own }` — never a per-sender
        /// user id or reaction event id.
        reactions: Vec<ReactionGroupVm>,
        /// The media attachment when this message is an image/video/audio/file
        /// msgtype (Story 3.6, FR-13), else `null` for a text message. Carries only
        /// opaque `keeper-media://` URLs + display metadata — never a `MediaSource`,
        /// key, `mxc` URI, or event id (AD-4, NFR-9). `body` remains the caption.
        ///
        /// Boxed so the (media-less) text-message case does not pay the full
        /// [`MediaVm`] size on every timeline item (`clippy::large_enum_variant`);
        /// `Box` is serde/ts-rs-transparent, so the wire shape and the generated
        /// binding stay `MediaVm | null`.
        media: Option<Box<MediaVm>>,
        /// The *other* members whose latest read receipt sits on this item, as
        /// opaque Matrix user ids (Story 3.9, receipts). Populated from
        /// `EventTimelineItem::read_receipts()` keys with the account's own user id
        /// excluded (never render self as a reader), in the SDK's receipt-map
        /// order. Empty when no other member has read up to here. Only opaque ids
        /// cross IPC — no avatars, receipt event ids, or timestamps (NFR-9, AD-1);
        /// the frontend renders deterministic initials micro-avatars. An own
        /// message with a non-empty `readers` additionally shows a read tick.
        readers: Vec<String>,
    },
    /// An event that could not be decrypted yet (`MsgLikeKind::UnableToDecrypt`).
    /// Renders an explicit honest stub instead of a blank row (Story 3.1). Carries
    /// **only** non-secret render data — a stable opaque render key, the sender
    /// user id, a resolved display name, and the timestamp. NO ciphertext, session
    /// id, or any crypto/key material ever crosses IPC on this VM (NFR-9, AD-1).
    /// When room keys arrive later, the SDK re-maps this item to a
    /// [`TimelineItemVm::Message`] via a `Set` diff — no extra code needed.
    Utd {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The event origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
    },
    /// A message that has been redacted — deleted for everyone (Story 3.8, FR-15).
    /// Renders an explicit honest "Message deleted" stub instead of a blank row or
    /// a silent removal (the same honesty principle as [`TimelineItemVm::Utd`]).
    /// Carries **only** non-secret render data — a stable opaque render key, the
    /// sender user id, a resolved display name, and the timestamp. The redacted
    /// event has no body/content to read, and no tombstone/redaction reason crosses
    /// IPC (NFR-9, AD-1). The SDK turns a live message into this in place via a
    /// `Set` diff, so diff indices stay aligned — keeper never removes or re-indexes
    /// a redacted item (local archive retention is Story 5.2).
    Redacted {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
        /// The sender's Matrix user id (opaque, passed through verbatim).
        sender: String,
        /// The resolved sender display name, or `null` when unavailable.
        sender_display_name: Option<String>,
        /// The event origin timestamp: ms since the Unix epoch (UTC).
        #[ts(type = "number")]
        timestamp: i64,
    },
    /// Any non-text item (non-text msgtype, state/membership/profile change, or a
    /// virtual date-divider/read-marker item).
    /// Carried only to keep diff indices aligned; the frontend renders nothing.
    Other {
        /// Stable opaque render key (the item's `unique_id`).
        key: String,
    },
}

/// One index-based timeline operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-9, AD-20).
///
/// The SDK `Timeline`'s `subscribe` stream yields a `VectorDiff` sequence;
/// keeper forwards it verbatim as these ops (one VM per SDK item). The frontend
/// applies them to a plain array by index and **never** re-sorts, filters, or
/// re-indexes. Serialized as an internally tagged enum so the frontend can
/// switch on `op`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum TimelineOp {
    /// Full reset — replace the current contents with `items`.
    Reset {
        /// The complete current timeline, in order.
        items: Vec<TimelineItemVm>,
    },
    /// Append `items` to the end, in order.
    Append {
        /// Items to append.
        items: Vec<TimelineItemVm>,
    },
    /// Remove all items.
    Clear,
    /// Insert `item` at the front (index 0).
    PushFront {
        /// The item to prepend.
        item: TimelineItemVm,
    },
    /// Append `item` to the end.
    PushBack {
        /// The item to append.
        item: TimelineItemVm,
    },
    /// Remove the first item.
    PopFront,
    /// Remove the last item.
    PopBack,
    /// Insert `item` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The item to insert.
        item: TimelineItemVm,
    },
    /// Replace the item at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement item.
        item: TimelineItemVm,
    },
    /// Remove the item at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
    /// Truncate the timeline to `length` items.
    Truncate {
        /// The new length.
        #[ts(type = "number")]
        length: u32,
    },
}

/// A batch of timeline ops delivered over the subscription's `Channel` (AD-8).
///
/// The stream always opens with a batch whose first op is a
/// [`TimelineOp::Reset`] carrying the cached snapshot, then diff batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TimelineBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<TimelineOp>,
}

/// One member currently typing in the open room (Story 3.9, typing, AD-14,
/// NFR-9).
///
/// Carries **only** the opaque Matrix `user_id` and a resolved `display_name`
/// (best-effort, `null` when the member can't be resolved) so the typing row can
/// render "<name> is typing…" honestly. No presence, avatars, or crypto material
/// cross IPC on this VM (AD-1). The SDK already filters the account's own user id
/// out of the typing stream, so a typist is always another member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypistVm {
    /// The typing member's Matrix user id (opaque, passed through verbatim).
    pub user_id: String,
    /// The member's resolved display name for the "… is typing" copy, or `null`
    /// when it can't be resolved (the frontend then falls back to the user id).
    pub display_name: Option<String>,
}

/// A batch delivered over the typing subscription's `Channel` (Story 3.9, AD-8,
/// AD-14).
///
/// The full current set of *other* members typing in the open room — inherently
/// idempotent, safe to re-subscribe. An empty `typists` means nobody is typing
/// (the frontend renders nothing). The stream opens with the current set, then
/// emits on every change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct TypingBatch {
    /// The members currently typing (other than the account's own user).
    pub typists: Vec<TypistVm>,
}

/// Whether back-pagination is currently running (Story 3.9, pagination, AD-8).
///
/// A Rust-authoritative projection of the SDK `PaginationStatus`:  `Paginating`
/// while a back-pagination request is in flight (the boundary shows a spinner),
/// `Idle` otherwise. Serializes to its camelCase name. The homeserver-start signal
/// is carried separately on [`PaginationStatusBatch::hit_start`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PaginationState {
    /// A back-pagination request is in flight — the boundary shows a spinner.
    Paginating,
    /// No back-pagination is running.
    Idle,
}

/// A batch delivered over the pagination-status subscription's `Channel` (Story
/// 3.9, AD-8).
///
/// A scalar snapshot of the live back-pagination status, mapped from the SDK
/// `PaginationStatus`: `state` drives the boundary spinner, and `hit_start` is
/// `true` once the homeserver has no older history (the boundary then states the
/// conversation start and no further pagination is attempted). Inherently
/// idempotent — each batch carries the full current status. Older events
/// themselves arrive over the existing timeline diff stream (`PushFront`/`Insert`),
/// never here; this channel carries only the status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaginationStatusBatch {
    /// Whether back-pagination is currently in flight.
    pub state: PaginationState,
    /// Whether the homeserver start of the room has been reached (no more older
    /// history). `true` only alongside an `Idle` state.
    pub hit_start: bool,
}

/// The durable login-mechanism discriminant of an account (Story 2.5, AD-17).
///
/// Set once at add time by the authenticating [`AuthProvider`] and persisted in
/// the non-secret `keeper.db` registry row (never in the Keychain session blob,
/// never a secret). Surfaced on [`AccountVm::provider`] so the frontend can key
/// provider-specific UI (e.g. the Beeper coverage disclosure) off a stable tag
/// rather than the resolved homeserver host. Serializes to its lowercase name
/// (`"password" | "oidc" | "beeper"`) — the frontend wire contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Provider {
    /// A native Matrix password (`m.login.password`) login.
    Password,
    /// An OIDC (OAuth 2.0 / MSC3861) login.
    Oidc,
    /// A Beeper unofficial email-code (JWT) login against `matrix.beeper.com`.
    Beeper,
}

impl Provider {
    /// The lowercase string persisted in the `keeper.db` `provider` column and
    /// serialized over IPC (`"password" | "oidc" | "beeper"`).
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            Provider::Password => "password",
            Provider::Oidc => "oidc",
            Provider::Beeper => "beeper",
        }
    }

    /// Parse a registry `provider` column value back into a [`Provider`], or
    /// `None` for an unrecognized / absent tag (a legacy NULL row).
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "password" => Some(Provider::Password),
            "oidc" => Some(Provider::Oidc),
            "beeper" => Some(Provider::Beeper),
            _ => None,
        }
    }
}

/// Non-secret account registry projection returned to the frontend on a
/// successful login (FR-1, NFR-9).
///
/// Carries **only** the opaque keeper account id, the Matrix user id, the
/// resolved homeserver URL, the per-account hue index, and the durable
/// login-mechanism [`Provider`] tag. Tokens, refresh tokens, device/crypto keys,
/// and any `MatrixSession` material never appear here — they live only in the
/// macOS Keychain and never cross IPC back to TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountVm {
    /// Opaque keeper-generated account id (a ULID). Used in paths, rows, VMs,
    /// and Keychain entries.
    pub account_id: String,
    /// The Matrix user id this account signed in as (e.g. `@alice:example.org`).
    pub user_id: String,
    /// The resolved homeserver base URL (after well-known discovery).
    pub homeserver_url: String,
    /// The account's hue index (0–7) on the 8-hue wheel, assigned at add time
    /// and persisted in `keeper.db`. The frontend maps it to a CSS hue rendered
    /// as a 3 px chat-row edge bar and (later) a switcher dot.
    #[ts(type = "number")]
    pub hue_index: u8,
    /// The durable login-mechanism tag, stamped at add time and persisted in
    /// `keeper.db`. Drives provider-specific UI (e.g. Beeper coverage) off a
    /// stable discriminant rather than the resolved homeserver host.
    pub provider: Provider,
}

/// A single merged-inbox room row, attributed to its owning account (AD-20).
///
/// The unified inbox merges every active account's room-list stream into one
/// recency-ordered list. Each row is a [`RoomVm`]'s render data plus the opaque
/// keeper `accountId` it belongs to and that account's persisted `hueIndex`
/// (0–7). Carries **only** non-secret render data — no tokens, session material,
/// or event ids cross IPC on this VM. The frontend renders the hue as a 3 px
/// left edge bar and opens the row's timeline on the row's `accountId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InboxRoomVm {
    /// Opaque keeper account id this room belongs to. Drives timeline/send.
    pub account_id: String,
    /// The account's hue index (0–7) for the row's edge bar.
    #[ts(type = "number")]
    pub hue_index: u8,
    /// Opaque Matrix room id (passed through verbatim as a string).
    pub room_id: String,
    /// The SDK-computed room display name.
    pub display_name: String,
    /// Plain-text preview of the latest `m.room.message`, or `null`.
    pub last_message: Option<String>,
    /// Latest-event timestamp: ms since the Unix epoch (UTC), or `null`.
    #[ts(type = "number | null")]
    pub timestamp: Option<i64>,
    /// Optional room avatar URL (an `mxc://` URI), or `null`.
    pub avatar_url: Option<String>,
    /// Authoritative unread flag: `true` when the room has unread messages,
    /// unread mentions, or the manual `m.marked_unread` flag set (AD-20). The
    /// frontend renders this directly (bold name + dot/badge) and never
    /// re-derives it from events.
    pub is_unread: bool,
    /// Count of unread mentions (client-side, precise for E2EE). Drives the
    /// filled primary mention badge; a value of 0 shows a plain dot when
    /// `is_unread` is otherwise set.
    #[ts(type = "number")]
    pub mention_count: u32,
    /// Authoritative archive flag: `true` when the room carries the Matrix
    /// low-priority tag (`m.lowpriority`) (Story 4.2, AD-20). The merge
    /// partitions on this to place the row in the Archive window unless it is
    /// unread (auto-return is a pure view rule); the frontend never re-derives it.
    pub is_archived: bool,
    /// Authoritative favourite flag: `true` when the room carries the Matrix
    /// favourite tag (`m.favourite`) (Story 4.4, AD-20). A *notable* tag, so a
    /// change re-emits the room-list stream live and syncs cross-client (SDK-
    /// sourced, copied through like `is_archived` — not merger-owned like
    /// `is_pinned`). The merge partitions on this to place the row in the
    /// Favorites window (removed from Inbox/Archive), behind Pins in precedence;
    /// the frontend renders this directly (Favorite/Unfavorite gating) and never
    /// re-derives it.
    pub is_favourite: bool,
    /// Authoritative pin flag: `true` when the room is pinned in keeper-local
    /// state (Story 4.3, AD-20). Pins are keeper-local (no Matrix tag), owned by
    /// the merger, which places a pinned room in the Pins window (removed from
    /// Inbox/Archive). The frontend renders this directly (Pin/Unpin gating) and
    /// never re-derives it.
    pub is_pinned: bool,
}

/// One index-based merged-inbox operation mirroring an eyeball-im `VectorDiff`
/// (AD-8, AD-20).
///
/// The merged inbox is computed in `keeper-core::inbox`; keeper streams its
/// recency-ordered result as these ops. The frontend applies them to a plain
/// array by index and **never** re-sorts. Serialized as an internally tagged
/// enum so the frontend can switch on `op`. The variants mirror [`RoomListOp`]
/// so the shared frontend diff reducer applies both.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum InboxOp {
    /// Full reset — replace the current contents with `rooms`.
    Reset {
        /// The complete current merged window, in recency order.
        rooms: Vec<InboxRoomVm>,
    },
    /// Append `rooms` to the end, in order.
    Append {
        /// Rooms to append.
        rooms: Vec<InboxRoomVm>,
    },
    /// Remove all rooms.
    Clear,
    /// Insert `room` at `index`, shifting the tail right.
    Insert {
        /// The insertion index.
        #[ts(type = "number")]
        index: u32,
        /// The room to insert.
        room: InboxRoomVm,
    },
    /// Replace the room at `index` in place.
    Set {
        /// The index to overwrite.
        #[ts(type = "number")]
        index: u32,
        /// The replacement room.
        room: InboxRoomVm,
    },
    /// Remove the room at `index`, shifting the tail left.
    Remove {
        /// The index to remove.
        #[ts(type = "number")]
        index: u32,
    },
}

/// A batch of merged-inbox ops delivered over the subscription's `Channel`
/// (AD-8, AD-20).
///
/// The stream always opens with a batch whose first op is an [`InboxOp::Reset`]
/// carrying the current merged window, then further batches as accounts sync or
/// are added/removed. The merge is partitioned into an Inbox and an Archive
/// window (Story 4.2), and `total` is the length of *this* window's partition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InboxBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<InboxOp>,
    /// The number of rooms in this streamed window (the partition's own length),
    /// when known. Since Story 4.2 the merge is split into an Inbox and an
    /// Archive window, so this is per-window, not a cross-account server total.
    #[ts(type = "number | null")]
    pub total: Option<u32>,
}

/// The single error envelope every fallible command rejects with (AD-8, AD-21).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct IpcError {
    /// Stable machine-readable error code.
    pub code: IpcErrorCode,
    /// Human-readable message (never contains secrets or plaintext).
    pub message: String,
    /// Opaque keeper account id this error pertains to, when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    /// Whether retrying the same operation may succeed.
    pub retriable: bool,
}

/// A single demo item carried in snapshot/diff batches. Placeholder payload
/// that exercises the snapshot-then-diff channel pattern end-to-end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DemoItem {
    /// Stable item id.
    pub id: String,
    /// Display label.
    pub label: String,
}

/// A batch delivered over a demo subscription's `Channel` (AD-8).
///
/// The stream always opens with a [`DemoBatch::Snapshot`] (full reset) before
/// any [`DemoBatch::Diff`]. Serialized as an internally tagged enum so the
/// frontend can switch on `kind`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub enum DemoBatch {
    /// Full state reset — the complete current set of items.
    Snapshot {
        /// Every item currently present.
        items: Vec<DemoItem>,
    },
    /// Incremental change relative to the last delivered state.
    Diff {
        /// Items added or updated in this batch.
        added: Vec<DemoItem>,
        /// Ids removed in this batch.
        removed: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_error_code_serializes_camel_case() {
        let json = serde_json::to_string(&IpcErrorCode::Unsupported).expect("serialize code");
        assert_eq!(json, "\"unsupported\"");
        let back: IpcErrorCode = serde_json::from_str(&json).expect("deserialize code");
        assert_eq!(back, IpcErrorCode::Unsupported);
    }

    #[test]
    fn ipc_error_round_trips_camel_case_and_omits_none_account() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: None,
            retriable: true,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        // camelCase field name and absent account_id.
        assert!(json.contains("\"retriable\":true"), "json was: {json}");
        assert!(
            !json.contains("accountId"),
            "account_id should be omitted: {json}"
        );
        let back: IpcError = serde_json::from_str(&json).expect("deserialize error");
        assert_eq!(back, err);
    }

    #[test]
    fn ipc_error_serializes_account_id_camel_case_when_present() {
        let err = IpcError {
            code: IpcErrorCode::Internal,
            message: "boom".to_owned(),
            account_id: Some("01ABC".to_owned()),
            retriable: false,
        };
        let json = serde_json::to_string(&err).expect("serialize error");
        assert!(json.contains("\"accountId\":\"01ABC\""), "json was: {json}");
    }

    #[test]
    fn account_vm_round_trips_camel_case() {
        let vm = AccountVm {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            user_id: "@alice:example.org".to_owned(),
            homeserver_url: "https://matrix.example.org/".to_owned(),
            hue_index: 3,
            provider: Provider::Password,
        };
        let json = serde_json::to_string(&vm).expect("serialize account vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"userId\":"), "json was: {json}");
        assert!(json.contains("\"homeserverUrl\":"), "json was: {json}");
        assert!(json.contains("\"hueIndex\":3"), "json was: {json}");
        assert!(
            json.contains("\"provider\":\"password\""),
            "json was: {json}"
        );
        // No token/session material is present on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: AccountVm = serde_json::from_str(&json).expect("deserialize account vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn provider_serializes_lowercase_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&Provider::Password).expect("serialize password"),
            "\"password\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Oidc).expect("serialize oidc"),
            "\"oidc\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::Beeper).expect("serialize beeper"),
            "\"beeper\""
        );
        for provider in [Provider::Password, Provider::Oidc, Provider::Beeper] {
            let json = serde_json::to_string(&provider).expect("serialize provider");
            let back: Provider = serde_json::from_str(&json).expect("deserialize provider");
            assert_eq!(back, provider);
        }
    }

    #[test]
    fn provider_registry_str_round_trips() {
        for provider in [Provider::Password, Provider::Oidc, Provider::Beeper] {
            assert_eq!(
                Provider::from_registry_str(provider.as_registry_str()),
                Some(provider)
            );
        }
        assert_eq!(Provider::from_registry_str("unknown"), None);
        assert_eq!(Provider::from_registry_str(""), None);
    }

    fn sample_inbox_room() -> InboxRoomVm {
        InboxRoomVm {
            account_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_owned(),
            hue_index: 2,
            room_id: "!abc:example.org".to_owned(),
            display_name: "Alice".to_owned(),
            last_message: Some("hi there".to_owned()),
            timestamp: Some(1_720_000_000_000),
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
            is_pinned: false,
        }
    }

    #[test]
    fn inbox_room_vm_round_trips_camel_case_with_account_and_hue() {
        let vm = sample_inbox_room();
        let json = serde_json::to_string(&vm).expect("serialize inbox room vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"hueIndex\":2"), "json was: {json}");
        assert!(json.contains("\"roomId\":"), "json was: {json}");
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: InboxRoomVm = serde_json::from_str(&json).expect("deserialize inbox room vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn inbox_op_tags_and_round_trips() {
        let reset = InboxOp::Reset {
            rooms: vec![sample_inbox_room()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: InboxOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let remove = InboxOp::Remove { index: 2 };
        let json = serde_json::to_string(&remove).expect("serialize remove");
        assert!(json.contains("\"op\":\"remove\""), "json was: {json}");
        assert!(json.contains("\"index\":2"), "json was: {json}");
        let back: InboxOp = serde_json::from_str(&json).expect("deserialize remove");
        assert_eq!(back, remove);
    }

    #[test]
    fn inbox_batch_round_trips() {
        let batch = InboxBatch {
            ops: vec![InboxOp::Reset {
                rooms: vec![sample_inbox_room()],
            }],
            total: Some(11),
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"total\":11"), "json was: {json}");
        let back: InboxBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn new_error_codes_serialize_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SlidingSyncUnsupported)
                .expect("serialize sss code"),
            "\"slidingSyncUnsupported\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::InvalidCredentials).expect("serialize creds code"),
            "\"invalidCredentials\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::ServerUnreachable)
                .expect("serialize unreachable code"),
            "\"serverUnreachable\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::UnsupportedLoginType)
                .expect("serialize login-type code"),
            "\"unsupportedLoginType\""
        );
        // Story 2.2 OIDC codes — locked to the frontend wire contract.
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthUnsupported).expect("serialize oauth-unsup"),
            "\"oauthUnsupported\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthTimedOut).expect("serialize oauth-timeout"),
            "\"oauthTimedOut\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthCancelled).expect("serialize oauth-cancel"),
            "\"oauthCancelled\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::OauthFailed).expect("serialize oauth-failed"),
            "\"oauthFailed\""
        );
        // Story 2.3 Beeper code — locked to the frontend wire contract.
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BeeperUnavailable)
                .expect("serialize beeper-unavailable"),
            "\"beeperUnavailable\""
        );
    }

    #[test]
    fn verification_failed_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::VerificationFailed)
                .expect("serialize verification-failed code"),
            "\"verificationFailed\""
        );
    }

    #[test]
    fn backup_error_codes_serialize_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupMalformedKey)
                .expect("serialize backup-malformed code"),
            "\"backupMalformedKey\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupIncorrectKey)
                .expect("serialize backup-incorrect code"),
            "\"backupIncorrectKey\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupExists)
                .expect("serialize backup-exists code"),
            "\"backupExists\""
        );
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::BackupFailed)
                .expect("serialize backup-failed code"),
            "\"backupFailed\""
        );
    }

    #[test]
    fn backup_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&BackupStatus::Unknown).expect("serialize unknown"),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Disabled).expect("serialize disabled"),
            "\"disabled\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Enabled).expect("serialize enabled"),
            "\"enabled\""
        );
        assert_eq!(
            serde_json::to_string(&BackupStatus::Incomplete).expect("serialize incomplete"),
            "\"incomplete\""
        );
    }

    #[test]
    fn backup_status_round_trips() {
        for status in [
            BackupStatus::Unknown,
            BackupStatus::Disabled,
            BackupStatus::Enabled,
            BackupStatus::Incomplete,
        ] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: BackupStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn sync_unavailable_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SyncUnavailable).expect("serialize sync code"),
            "\"syncUnavailable\""
        );
    }

    fn sample_room() -> RoomVm {
        RoomVm {
            room_id: "!abc:example.org".to_owned(),
            display_name: "Alice".to_owned(),
            last_message: Some("hi there".to_owned()),
            timestamp: Some(1_720_000_000_000),
            avatar_url: Some("mxc://example.org/av".to_owned()),
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
        }
    }

    #[test]
    fn room_vm_round_trips_camel_case() {
        let vm = sample_room();
        let json = serde_json::to_string(&vm).expect("serialize room vm");
        assert!(json.contains("\"roomId\":"), "json was: {json}");
        assert!(json.contains("\"displayName\":"), "json was: {json}");
        assert!(json.contains("\"lastMessage\":"), "json was: {json}");
        assert!(json.contains("\"avatarUrl\":"), "json was: {json}");
        // No token/session material may appear on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize room vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn room_vm_null_fields_round_trip() {
        let vm = RoomVm {
            room_id: "!x:example.org".to_owned(),
            display_name: "Room".to_owned(),
            last_message: None,
            timestamp: None,
            avatar_url: None,
            is_unread: false,
            mention_count: 0,
            is_archived: false,
            is_favourite: false,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"lastMessage\":null"), "json was: {json}");
        assert!(json.contains("\"timestamp\":null"), "json was: {json}");
        let back: RoomVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn room_list_op_tags_and_round_trips() {
        let reset = RoomListOp::Reset {
            rooms: vec![sample_room()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: RoomListOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let insert = RoomListOp::Insert {
            index: 3,
            room: sample_room(),
        };
        let json = serde_json::to_string(&insert).expect("serialize insert");
        assert!(json.contains("\"op\":\"insert\""), "json was: {json}");
        assert!(json.contains("\"index\":3"), "json was: {json}");
        let back: RoomListOp = serde_json::from_str(&json).expect("deserialize insert");
        assert_eq!(back, insert);

        let clear = RoomListOp::Clear;
        assert_eq!(
            serde_json::to_string(&clear).expect("serialize clear"),
            "{\"op\":\"clear\"}"
        );
    }

    #[test]
    fn room_list_batch_round_trips() {
        let batch = RoomListBatch {
            ops: vec![
                RoomListOp::Reset {
                    rooms: vec![sample_room()],
                },
                RoomListOp::PopFront,
            ],
            total: Some(7),
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"total\":7"), "json was: {json}");
        let back: RoomListBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn timeline_unavailable_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::TimelineUnavailable)
                .expect("serialize timeline code"),
            "\"timelineUnavailable\""
        );
    }

    #[test]
    fn send_failed_code_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&IpcErrorCode::SendFailed).expect("serialize send-failed code"),
            "\"sendFailed\""
        );
    }

    #[test]
    fn send_state_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&SendState::Sending).expect("serialize sending"),
            "\"sending\""
        );
        assert_eq!(
            serde_json::to_string(&SendState::Sent).expect("serialize sent"),
            "\"sent\""
        );
        assert_eq!(
            serde_json::to_string(&SendState::Failed).expect("serialize failed"),
            "\"failed\""
        );
    }

    #[test]
    fn send_state_round_trips() {
        for state in [SendState::Sending, SendState::Sent, SendState::Failed] {
            let json = serde_json::to_string(&state).expect("serialize send state");
            let back: SendState = serde_json::from_str(&json).expect("deserialize send state");
            assert_eq!(back, state);
        }
    }

    #[test]
    fn connection_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Online).expect("serialize online"),
            "\"online\""
        );
        assert_eq!(
            serde_json::to_string(&ConnectionStatus::Offline).expect("serialize offline"),
            "\"offline\""
        );
    }

    #[test]
    fn connection_status_round_trips() {
        for status in [ConnectionStatus::Online, ConnectionStatus::Offline] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: ConnectionStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn connection_status_batch_round_trips() {
        let batch = ConnectionStatusBatch {
            status: ConnectionStatus::Offline,
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"status\":\"offline\""), "json was: {json}");
        let back: ConnectionStatusBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn encryption_status_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Unknown).expect("serialize unknown"),
            "\"unknown\""
        );
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Verified).expect("serialize verified"),
            "\"verified\""
        );
        assert_eq!(
            serde_json::to_string(&EncryptionStatus::Unverified).expect("serialize unverified"),
            "\"unverified\""
        );
    }

    #[test]
    fn encryption_status_round_trips() {
        for status in [
            EncryptionStatus::Unknown,
            EncryptionStatus::Verified,
            EncryptionStatus::Unverified,
        ] {
            let json = serde_json::to_string(&status).expect("serialize status");
            let back: EncryptionStatus = serde_json::from_str(&json).expect("deserialize status");
            assert_eq!(back, status);
        }
    }

    #[test]
    fn encryption_status_batch_round_trips() {
        let batch = EncryptionStatusBatch {
            status: EncryptionStatus::Unverified,
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(
            json.contains("\"status\":\"unverified\""),
            "json was: {json}"
        );
        let back: EncryptionStatusBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn sas_emoji_vm_round_trips_camel_case() {
        let vm = SasEmojiVm {
            symbol: "🐶".to_owned(),
            name: "Dog".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize emoji vm");
        assert!(json.contains("\"symbol\":\"🐶\""), "json was: {json}");
        assert!(json.contains("\"name\":\"Dog\""), "json was: {json}");
        let back: SasEmojiVm = serde_json::from_str(&json).expect("deserialize emoji vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn verification_phase_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Requested).expect("serialize requested"),
            "\"requested\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Ready).expect("serialize ready"),
            "\"ready\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Comparing).expect("serialize comparing"),
            "\"comparing\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Confirmed).expect("serialize confirmed"),
            "\"confirmed\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Done).expect("serialize done"),
            "\"done\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Cancelled).expect("serialize cancelled"),
            "\"cancelled\""
        );
        assert_eq!(
            serde_json::to_string(&VerificationPhase::Failed).expect("serialize failed"),
            "\"failed\""
        );
    }

    #[test]
    fn verification_phase_round_trips() {
        for phase in [
            VerificationPhase::Requested,
            VerificationPhase::Ready,
            VerificationPhase::Comparing,
            VerificationPhase::Confirmed,
            VerificationPhase::Done,
            VerificationPhase::Cancelled,
            VerificationPhase::Failed,
        ] {
            let json = serde_json::to_string(&phase).expect("serialize phase");
            let back: VerificationPhase = serde_json::from_str(&json).expect("deserialize phase");
            assert_eq!(back, phase);
        }
    }

    #[test]
    fn verification_flow_vm_round_trips_camel_case() {
        let vm = VerificationFlowVm {
            flow_id: "$flow123".to_owned(),
            phase: VerificationPhase::Comparing,
            emojis: Some(vec![
                SasEmojiVm {
                    symbol: "🐶".to_owned(),
                    name: "Dog".to_owned(),
                },
                SasEmojiVm {
                    symbol: "🐱".to_owned(),
                    name: "Cat".to_owned(),
                },
            ]),
            qr_code_svg: None,
            reason: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize flow vm");
        assert!(json.contains("\"flowId\":\"$flow123\""), "json was: {json}");
        assert!(json.contains("\"phase\":\"comparing\""), "json was: {json}");
        assert!(json.contains("\"qrCodeSvg\":null"), "json was: {json}");
        // No SAS key / decimal / crypto material may appear on the VM.
        assert!(!json.contains("key"), "json leaked a key field: {json}");
        assert!(
            !json.contains("decimal"),
            "json leaked a decimal field: {json}"
        );
        let back: VerificationFlowVm = serde_json::from_str(&json).expect("deserialize flow vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn verification_flow_vm_qr_and_reason_round_trip() {
        let vm = VerificationFlowVm {
            flow_id: "$flow456".to_owned(),
            phase: VerificationPhase::Failed,
            emojis: None,
            qr_code_svg: Some("<svg>…</svg>".to_owned()),
            reason: Some("The expected key did not match the verified one".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize flow vm");
        assert!(json.contains("\"qrCodeSvg\":\"<svg>"), "json was: {json}");
        assert!(
            json.contains("\"reason\":\"The expected"),
            "json was: {json}"
        );
        let back: VerificationFlowVm = serde_json::from_str(&json).expect("deserialize flow vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_utd_tags_and_round_trips() {
        let vm = TimelineItemVm::Utd {
            key: "unique-3".to_owned(),
            sender: "@carol:example.org".to_owned(),
            sender_display_name: Some("Carol".to_owned()),
            timestamp: 1_720_000_000_000,
        };
        let json = serde_json::to_string(&vm).expect("serialize utd vm");
        assert!(json.contains("\"kind\":\"utd\""), "json was: {json}");
        assert!(json.contains("\"key\":\"unique-3\""), "json was: {json}");
        assert!(
            json.contains("\"senderDisplayName\":\"Carol\""),
            "json was: {json}"
        );
        // No ciphertext / session / key material may appear on the VM.
        assert!(
            !json.contains("session"),
            "json leaked a session field: {json}"
        );
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize utd vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_utd_null_display_name_round_trips() {
        let vm = TimelineItemVm::Utd {
            key: "k".to_owned(),
            sender: "@a:example.org".to_owned(),
            sender_display_name: None,
            timestamp: 1,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"senderDisplayName\":null"),
            "json was: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_send_state_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "outgoing".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: Some(SendState::Sending),
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(
            json.contains("\"sendState\":\"sending\""),
            "json was: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    fn sample_message() -> TimelineItemVm {
        TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@bob:example.org".to_owned(),
            sender_display_name: Some("Bob".to_owned()),
            body: "hello world".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: false,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        }
    }

    #[test]
    fn reply_preview_vm_round_trips_camel_case() {
        let vm = ReplyPreviewVm {
            in_reply_to_key: Some("unique-orig".to_owned()),
            sender: "@carol:example.org".to_owned(),
            sender_display_name: Some("Carol".to_owned()),
            body: "original body".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize reply preview vm");
        assert!(
            json.contains("\"inReplyToKey\":\"unique-orig\""),
            "json was: {json}"
        );
        assert!(
            json.contains("\"senderDisplayName\":\"Carol\""),
            "json was: {json}"
        );
        // No event-id / txn-id material may appear on the VM.
        assert!(
            !json.contains("eventId") && !json.contains("$"),
            "json leaked event-id material: {json}"
        );
        let back: ReplyPreviewVm =
            serde_json::from_str(&json).expect("deserialize reply preview vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn reply_preview_vm_null_key_round_trips() {
        let vm = ReplyPreviewVm {
            in_reply_to_key: None,
            sender: "@carol:example.org".to_owned(),
            sender_display_name: None,
            body: String::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"inReplyToKey\":null"), "json was: {json}");
        let back: ReplyPreviewVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_reply_and_edited_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "unique-9".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "a reply".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: None,
            is_edited: true,
            reply: Some(ReplyPreviewVm {
                in_reply_to_key: Some("unique-orig".to_owned()),
                sender: "@bob:example.org".to_owned(),
                sender_display_name: Some("Bob".to_owned()),
                body: "the original".to_owned(),
            }),
            reactions: vec![
                ReactionGroupVm {
                    emoji: "👍".to_owned(),
                    count: 3,
                    is_own: false,
                },
                ReactionGroupVm {
                    emoji: "❤️".to_owned(),
                    count: 1,
                    is_own: true,
                },
            ],
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"isEdited\":true"), "json was: {json}");
        assert!(
            json.contains("\"inReplyToKey\":\"unique-orig\""),
            "json was: {json}"
        );
        // The reaction groups carry only emoji/count/is_own — no user-id or
        // event-id material.
        assert!(json.contains("\"emoji\":\"👍\""), "json was: {json}");
        assert!(json.contains("\"count\":3"), "json was: {json}");
        assert!(json.contains("\"isOwn\":true"), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_tags_and_round_trips() {
        let vm = sample_message();
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"kind\":\"message\""), "json was: {json}");
        assert!(
            json.contains("\"senderDisplayName\":\"Bob\""),
            "json was: {json}"
        );
        assert!(json.contains("\"isOwn\":false"), "json was: {json}");
        // No token/session/event-id material may appear on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_other_tags_and_round_trips() {
        let vm = TimelineItemVm::Other {
            key: "unique-2".to_owned(),
        };
        let json = serde_json::to_string(&vm).expect("serialize other vm");
        assert!(json.contains("\"kind\":\"other\""), "json was: {json}");
        assert!(json.contains("\"key\":\"unique-2\""), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize other vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_null_display_name_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "k".to_owned(),
            sender: "@a:example.org".to_owned(),
            sender_display_name: None,
            body: "hi".to_owned(),
            timestamp: 1,
            is_own: true,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"senderDisplayName\":null"),
            "json was: {json}"
        );
        assert!(json.contains("\"sendState\":null"), "json was: {json}");
        assert!(json.contains("\"reply\":null"), "json was: {json}");
        assert!(json.contains("\"media\":null"), "json was: {json}");
        // An empty reaction set serializes as an empty array (no pill row).
        assert!(json.contains("\"reactions\":[]"), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn reaction_group_vm_round_trips_camel_case_and_carries_no_identity() {
        let vm = ReactionGroupVm {
            emoji: "🎉".to_owned(),
            count: 4,
            is_own: true,
        };
        let json = serde_json::to_string(&vm).expect("serialize reaction group vm");
        assert!(json.contains("\"emoji\":\"🎉\""), "json was: {json}");
        assert!(json.contains("\"count\":4"), "json was: {json}");
        assert!(json.contains("\"isOwn\":true"), "json was: {json}");
        // Only emoji/count/is_own cross IPC — never a per-sender user id or a
        // reaction event id.
        assert!(
            !json.contains("sender") && !json.contains("userId") && !json.contains("eventId"),
            "json leaked identity material: {json}"
        );
        assert!(
            !json.contains('@') && !json.contains('$'),
            "json leaked user-id/event-id material: {json}"
        );
        let back: ReactionGroupVm =
            serde_json::from_str(&json).expect("deserialize reaction group vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_op_tags_and_round_trips() {
        let reset = TimelineOp::Reset {
            items: vec![sample_message()],
        };
        let json = serde_json::to_string(&reset).expect("serialize reset");
        assert!(json.contains("\"op\":\"reset\""), "json was: {json}");
        let back: TimelineOp = serde_json::from_str(&json).expect("deserialize reset");
        assert_eq!(back, reset);

        let insert = TimelineOp::Insert {
            index: 4,
            item: sample_message(),
        };
        let json = serde_json::to_string(&insert).expect("serialize insert");
        assert!(json.contains("\"op\":\"insert\""), "json was: {json}");
        assert!(json.contains("\"index\":4"), "json was: {json}");
        let back: TimelineOp = serde_json::from_str(&json).expect("deserialize insert");
        assert_eq!(back, insert);

        let clear = TimelineOp::Clear;
        assert_eq!(
            serde_json::to_string(&clear).expect("serialize clear"),
            "{\"op\":\"clear\"}"
        );
    }

    #[test]
    fn timeline_batch_round_trips() {
        let batch = TimelineBatch {
            ops: vec![
                TimelineOp::Reset {
                    items: vec![sample_message()],
                },
                TimelineOp::PushBack {
                    item: TimelineItemVm::Other {
                        key: "k2".to_owned(),
                    },
                },
            ],
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        assert!(json.contains("\"ops\":"), "json was: {json}");
        let back: TimelineBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn media_kind_vm_serializes_camel_case_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Image).expect("serialize image"),
            "\"image\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Video).expect("serialize video"),
            "\"video\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::Audio).expect("serialize audio"),
            "\"audio\""
        );
        assert_eq!(
            serde_json::to_string(&MediaKindVm::File).expect("serialize file"),
            "\"file\""
        );
        for kind in [
            MediaKindVm::Image,
            MediaKindVm::Video,
            MediaKindVm::Audio,
            MediaKindVm::File,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize kind");
            let back: MediaKindVm = serde_json::from_str(&json).expect("deserialize kind");
            assert_eq!(back, kind);
        }
    }

    #[test]
    fn media_vm_round_trips_camel_case_and_carries_no_key_material() {
        let vm = MediaVm {
            kind: MediaKindVm::Image,
            url: "keeper-media://media/acct/room/item/full".to_owned(),
            thumbnail_url: Some("keeper-media://media/acct/room/item/thumb".to_owned()),
            filename: "photo.png".to_owned(),
            mimetype: Some("image/png".to_owned()),
            size: Some(12_345),
            width: Some(800),
            height: Some(600),
            caption: Some("a nice photo".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize media vm");
        assert!(json.contains("\"kind\":\"image\""), "json was: {json}");
        assert!(
            json.contains("\"url\":\"keeper-media://"),
            "json was: {json}"
        );
        assert!(
            json.contains("\"thumbnailUrl\":\"keeper-media://"),
            "json was: {json}"
        );
        assert!(json.contains("\"size\":12345"), "json was: {json}");
        assert!(json.contains("\"width\":800"), "json was: {json}");
        // No mxc / EncryptedFile / key / event-id material may appear on the VM.
        assert!(!json.contains("mxc://"), "json leaked an mxc uri: {json}");
        assert!(!json.contains("mxc"), "json leaked mxc material: {json}");
        assert!(
            !json.contains("\"key\"") && !json.contains("iv") && !json.contains("hashes"),
            "json leaked EncryptedFile key material: {json}"
        );
        assert!(
            !json.contains("eventId") && !json.contains('$'),
            "json leaked event-id material: {json}"
        );
        let back: MediaVm = serde_json::from_str(&json).expect("deserialize media vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn media_vm_null_fields_round_trip() {
        let vm = MediaVm {
            kind: MediaKindVm::File,
            url: "keeper-media://media/a/r/i/full".to_owned(),
            thumbnail_url: None,
            filename: "report.pdf".to_owned(),
            mimetype: None,
            size: None,
            width: None,
            height: None,
            caption: None,
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(json.contains("\"thumbnailUrl\":null"), "json was: {json}");
        assert!(json.contains("\"mimetype\":null"), "json was: {json}");
        assert!(json.contains("\"size\":null"), "json was: {json}");
        let back: MediaVm = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, vm);
    }

    #[test]
    fn timeline_item_vm_message_with_media_round_trips_no_key_material() {
        let vm = TimelineItemVm::Message {
            key: "unique-media".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "look at this".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: false,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: Some(Box::new(MediaVm {
                kind: MediaKindVm::Video,
                url: "keeper-media://media/a/r/i/full".to_owned(),
                thumbnail_url: Some("keeper-media://media/a/r/i/thumb".to_owned()),
                filename: "clip.mp4".to_owned(),
                mimetype: Some("video/mp4".to_owned()),
                size: Some(999),
                width: Some(1280),
                height: Some(720),
                caption: None,
            })),
            readers: Vec::new(),
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(json.contains("\"media\":{"), "json was: {json}");
        assert!(json.contains("\"kind\":\"video\""), "json was: {json}");
        // No mxc / key / event-id material may cross on the media-carrying message.
        assert!(!json.contains("mxc"), "json leaked mxc material: {json}");
        assert!(!json.contains("eventId"), "json leaked event id: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn demo_batch_tags_variants() {
        let snap = DemoBatch::Snapshot {
            items: vec![DemoItem {
                id: "1".into(),
                label: "one".into(),
            }],
        };
        let json = serde_json::to_string(&snap).expect("serialize snapshot");
        assert!(json.contains("\"kind\":\"snapshot\""), "json was: {json}");
    }

    #[test]
    fn message_vm_carries_readers_as_opaque_ids() {
        // The receipts feature (Story 3.9): a message VM carries the *other*
        // members whose read receipt sits on it as opaque user ids under
        // `readers` — camelCase, an array of strings, no avatar/receipt-id fields.
        let vm = TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "read by others".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
            readers: vec![
                "@bob:example.org".to_owned(),
                "@carol:example.org".to_owned(),
            ],
        };
        let json = serde_json::to_string(&vm).expect("serialize message vm");
        assert!(
            json.contains("\"readers\":[\"@bob:example.org\",\"@carol:example.org\"]"),
            "json was: {json}"
        );
        // No receipt event id crosses on a reader.
        assert!(
            !json.contains("receiptId"),
            "json leaked receipt id: {json}"
        );
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize message vm");
        assert_eq!(back, vm);
    }

    #[test]
    fn typist_vm_round_trips_camel_case() {
        let vm = TypistVm {
            user_id: "@bob:example.org".to_owned(),
            display_name: Some("Bob".to_owned()),
        };
        let json = serde_json::to_string(&vm).expect("serialize typist");
        assert!(
            json.contains("\"userId\":\"@bob:example.org\""),
            "json was: {json}"
        );
        assert!(json.contains("\"displayName\":\"Bob\""), "json was: {json}");
        let back: TypistVm = serde_json::from_str(&json).expect("deserialize typist");
        assert_eq!(back, vm);
    }

    #[test]
    fn typing_batch_round_trips_and_empty_serializes() {
        let batch = TypingBatch {
            typists: vec![TypistVm {
                user_id: "@bob:example.org".to_owned(),
                display_name: None,
            }],
        };
        let json = serde_json::to_string(&batch).expect("serialize typing batch");
        assert!(json.contains("\"typists\":["), "json was: {json}");
        assert!(json.contains("\"displayName\":null"), "json was: {json}");
        let back: TypingBatch = serde_json::from_str(&json).expect("deserialize typing batch");
        assert_eq!(back, batch);

        let empty = TypingBatch { typists: vec![] };
        assert_eq!(
            serde_json::to_string(&empty).expect("serialize empty"),
            "{\"typists\":[]}"
        );
    }

    #[test]
    fn pagination_state_serializes_camel_case() {
        assert_eq!(
            serde_json::to_string(&PaginationState::Paginating).expect("serialize paginating"),
            "\"paginating\""
        );
        assert_eq!(
            serde_json::to_string(&PaginationState::Idle).expect("serialize idle"),
            "\"idle\""
        );
    }

    #[test]
    fn pagination_status_batch_round_trips_camel_case() {
        let batch = PaginationStatusBatch {
            state: PaginationState::Idle,
            hit_start: true,
        };
        let json = serde_json::to_string(&batch).expect("serialize pagination status");
        assert!(json.contains("\"state\":\"idle\""), "json was: {json}");
        assert!(json.contains("\"hitStart\":true"), "json was: {json}");
        let back: PaginationStatusBatch =
            serde_json::from_str(&json).expect("deserialize pagination status");
        assert_eq!(back, batch);

        let paginating = PaginationStatusBatch {
            state: PaginationState::Paginating,
            hit_start: false,
        };
        let json = serde_json::to_string(&paginating).expect("serialize paginating");
        assert!(
            json.contains("\"state\":\"paginating\""),
            "json was: {json}"
        );
        assert!(json.contains("\"hitStart\":false"), "json was: {json}");
    }
}
