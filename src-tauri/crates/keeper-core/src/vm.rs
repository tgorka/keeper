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
    },
    /// Any non-text item (non-text msgtype, state/membership/profile change,
    /// redacted, undecryptable, or a virtual date-divider/read-marker item).
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

/// Non-secret account registry projection returned to the frontend on a
/// successful login (FR-1, NFR-9).
///
/// Carries **only** the opaque keeper account id, the Matrix user id, the
/// resolved homeserver URL, and the per-account hue index. Tokens, refresh
/// tokens, device/crypto keys, and any `MatrixSession` material never appear
/// here — they live only in the macOS Keychain and never cross IPC back to
/// TypeScript.
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
/// are added/removed. `total` is the sum of the per-account known totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InboxBatch {
    /// The ordered ops to apply, in sequence.
    pub ops: Vec<InboxOp>,
    /// The total number of rooms across all accounts the servers know about,
    /// when known.
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
        };
        let json = serde_json::to_string(&vm).expect("serialize account vm");
        assert!(json.contains("\"accountId\":"), "json was: {json}");
        assert!(json.contains("\"userId\":"), "json was: {json}");
        assert!(json.contains("\"homeserverUrl\":"), "json was: {json}");
        assert!(json.contains("\"hueIndex\":3"), "json was: {json}");
        // No token/session material is present on the VM.
        assert!(!json.contains("token"), "json leaked a token field: {json}");
        let back: AccountVm = serde_json::from_str(&json).expect("deserialize account vm");
        assert_eq!(back, vm);
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
    fn timeline_item_vm_message_with_send_state_round_trips() {
        let vm = TimelineItemVm::Message {
            key: "unique-1".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            body: "outgoing".to_owned(),
            timestamp: 1_720_000_000_000,
            is_own: true,
            send_state: Some(SendState::Sending),
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
        }
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
        };
        let json = serde_json::to_string(&vm).expect("serialize");
        assert!(
            json.contains("\"senderDisplayName\":null"),
            "json was: {json}"
        );
        assert!(json.contains("\"sendState\":null"), "json was: {json}");
        let back: TimelineItemVm = serde_json::from_str(&json).expect("deserialize");
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
}
