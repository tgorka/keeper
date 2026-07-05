//! Per-open-room timeline producer (AD-4, AD-8, AD-9, AD-19, AD-20).
//!
//! Obtains a room's matrix-sdk-ui [`Timeline`], subscribes to its
//! snapshot-then-diff stream, and forwards it verbatim as index-based
//! [`TimelineOp`]s. Ordering is owned entirely by the SDK: keeper maps each SDK
//! `TimelineItem` to exactly one [`TimelineItemVm`] (never dropping items, so
//! diff indices stay aligned) and forwards each `VectorDiff` one-to-one — no
//! sorting or filtering here or in TypeScript (AD-9, AD-20).
//!
//! Secret containment (NFR-9): a VM carries only a stable opaque render key, the
//! sender user id, a resolved display name, the decoded **text** body of an
//! already-decrypted message, the timestamp, and `is_own`. No tokens, session
//! material, event raw JSON, or crypto state cross IPC; `tracing` logs carry the
//! opaque room id only — never a message body.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use matrix_sdk::Client;
use matrix_sdk_ui::eyeball_im::{Vector, VectorDiff};
use matrix_sdk_ui::timeline::{
    EventSendState, MsgLikeKind, RoomExt, Timeline, TimelineDetails, TimelineItem,
    TimelineItemContent, TimelineItemKind,
};

use crate::account::TimelineSink;
use crate::error::TimelineError;
use crate::media::{self, MediaVariant};
use crate::vm::{
    MediaKindVm, MediaVm, ReactionGroupVm, ReplyPreviewVm, SendState, TimelineBatch,
    TimelineItemVm, TimelineOp,
};

/// A Rust-side index of `event_id → render key` (unique_id), maintained by the
/// timeline producer while it maps items. It lets a reply's quoted-original
/// preview carry the *original's* opaque render `key` — never its event id — so
/// the frontend jump target stays event-id-free across IPC (NFR-9, AD-1). An
/// original that is not (yet) mapped simply isn't in the index, yielding a
/// `null` jump key on the reply preview.
type ReplyIndex = HashMap<OwnedEventId, String>;

/// Defensive upper bound on a decoded message body before it crosses IPC.
const MAX_BODY_CHARS: usize = 4096;

/// Extract the plain-text body of a message when its msgtype is renderable text.
///
/// `text`/`notice`/`emote` yield their body string; every other msgtype (media,
/// location, verification, …) yields `None`. An empty or whitespace-only body
/// also yields `None`, so a degenerate empty-body message renders as `Other`
/// (skipped) rather than an empty bubble. Pure — unit-tested via ruma
/// constructors.
pub fn text_body(msgtype: &MessageType) -> Option<String> {
    let body = match msgtype {
        MessageType::Text(content) => &content.body,
        MessageType::Notice(content) => &content.body,
        MessageType::Emote(content) => &content.body,
        _ => return None,
    };
    if body.trim().is_empty() {
        return None;
    }
    Some(body.clone())
}

/// Saturating conversion of a ruma `UInt` (a `u64` newtype) to `u32` for a VM
/// numeric field: a value beyond `u32::MAX` clamps to `u32::MAX` (honest, never a
/// wrapping `as` cast). Pure.
fn uint_to_u32(value: matrix_sdk::ruma::UInt) -> u32 {
    u32::try_from(u64::from(value)).unwrap_or(u32::MAX)
}

/// Map a media `MessageType` (`Image`/`Video`/`Audio`/`File`) to a [`MediaVm`]
/// carrying only opaque `keeper-media://` URLs + display metadata (Story 3.6,
/// FR-13, AD-4, NFR-9). Pure — no `MediaSource`, key, `mxc`, or event id ever
/// enters the returned VM; the URLs are built from the opaque
/// `account_id`/`room_id`/`item_key` coordinates by [`media::media_url`].
///
/// A text/notice/emote/other msgtype yields `None`. `thumbnail_url` is set only
/// for image/video (where a bubble thumbnail is renderable); audio/file get a
/// `null` thumbnail (the bubble renders inline audio / a file chip instead).
/// `filename` uses `.filename()` (falls back to the body); dimensions/mimetype/
/// size come from `info` when present. The caption is the message body when it is
/// a distinct caption (`.caption()`), else `None`.
pub fn media_vm(
    msgtype: &MessageType,
    account_id: &str,
    room_id: &str,
    item_key: &str,
) -> Option<MediaVm> {
    let full_url = media::media_url(account_id, room_id, item_key, MediaVariant::Full);
    let thumb_url = || media::media_url(account_id, room_id, item_key, MediaVariant::Thumbnail);

    match msgtype {
        MessageType::Image(c) => {
            let info = c.info.as_deref();
            Some(MediaVm {
                kind: MediaKindVm::Image,
                url: full_url,
                thumbnail_url: Some(thumb_url()),
                filename: c.filename().to_owned(),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(uint_to_u32),
                width: info.and_then(|i| i.width).map(uint_to_u32),
                height: info.and_then(|i| i.height).map(uint_to_u32),
                caption: c.caption().map(str::to_owned),
            })
        }
        MessageType::Video(c) => {
            let info = c.info.as_deref();
            Some(MediaVm {
                kind: MediaKindVm::Video,
                url: full_url,
                thumbnail_url: Some(thumb_url()),
                filename: c.filename().to_owned(),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(uint_to_u32),
                width: info.and_then(|i| i.width).map(uint_to_u32),
                height: info.and_then(|i| i.height).map(uint_to_u32),
                caption: c.caption().map(str::to_owned),
            })
        }
        MessageType::Audio(c) => {
            let info = c.info.as_deref();
            Some(MediaVm {
                kind: MediaKindVm::Audio,
                url: full_url,
                thumbnail_url: None,
                filename: c.filename().to_owned(),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(uint_to_u32),
                width: None,
                height: None,
                caption: c.caption().map(str::to_owned),
            })
        }
        MessageType::File(c) => {
            let info = c.info.as_deref();
            Some(MediaVm {
                kind: MediaKindVm::File,
                url: full_url,
                thumbnail_url: None,
                filename: c.filename().to_owned(),
                mimetype: info.and_then(|i| i.mimetype.clone()),
                size: info.and_then(|i| i.size).map(uint_to_u32),
                width: None,
                height: None,
                caption: c.caption().map(str::to_owned),
            })
        }
        _ => None,
    }
}

/// Map an SDK [`EventSendState`] (a local echo's send state) to the VM
/// [`SendState`] tag (FR-9, AD-13). Pure — unit-tested via the constructible
/// variants.
///
/// - `NotSentYet` → `Sending` (enqueued / in flight).
/// - `Sent` → `Sent` (server-acknowledged).
/// - `SendingFailed { is_recoverable: true }` → `Sending` — a transient failure
///   the send queue is still auto-retrying, so it reads as in-flight, not failed.
/// - `SendingFailed { is_recoverable: false }` → `Failed` — an unrecoverable
///   failure surfaced to the user as the persistent `Failed — Retry` caption.
fn map_send_state(state: &EventSendState) -> SendState {
    match state {
        EventSendState::NotSentYet { .. } => SendState::Sending,
        EventSendState::Sent { .. } => SendState::Sent,
        EventSendState::SendingFailed { is_recoverable, .. } => {
            if *is_recoverable {
                SendState::Sending
            } else {
                SendState::Failed
            }
        }
    }
}

/// Truncate a decoded body to [`MAX_BODY_CHARS`] characters (by `char`, so a
/// multi-byte grapheme is never split mid-byte).
fn truncate_body(body: String) -> String {
    // Fast path: a byte length within the char cap guarantees the char count is
    // too (bytes ≥ chars), so skip the full O(n) `chars().count()` scan that
    // would otherwise run for every message on the snapshot/diff hot path.
    if body.len() <= MAX_BODY_CHARS || body.chars().count() <= MAX_BODY_CHARS {
        body
    } else {
        body.chars().take(MAX_BODY_CHARS).collect()
    }
}

/// Derive the quoted-original preview for a reply message from its
/// `content.in_reply_to()`, resolving the original's opaque render `key` through
/// the producer's `event_id → unique_id` `index` (Story 3.4, FR-10, NFR-9).
///
/// Pure: `content` and `index` are the only inputs. Returns `None` when the
/// message is not a reply. When it is:
/// - `in_reply_to_key` = `index.get(&details.event_id)` — the original's opaque
///   render key when it is currently mapped in the timeline, else `null` (the
///   quote still renders honestly but isn't clickable). Never an event id.
/// - sender / display-name / body come from the embedded original when its
///   details are `Ready`; otherwise fall back to empty/`None`. The body reuses
///   [`text_body`] and is empty for a non-text original.
///
/// No event ids, txn ids, or raw event JSON cross into the returned VM (AD-1).
pub fn reply_preview(content: &TimelineItemContent, index: &ReplyIndex) -> Option<ReplyPreviewVm> {
    let details = content.in_reply_to()?;
    Some(reply_preview_from_details(&details, index))
}

/// Pure mapping of an [`InReplyToDetails`] + the `event_id → unique_id` `index`
/// into a [`ReplyPreviewVm`]. Split from [`reply_preview`] so the key-resolution
/// and details-availability branches are unit-testable without an SDK `Message`
/// (whose fields are crate-private).
fn reply_preview_from_details(
    details: &matrix_sdk_ui::timeline::InReplyToDetails,
    index: &ReplyIndex,
) -> ReplyPreviewVm {
    let in_reply_to_key = index.get(&details.event_id).cloned();

    let (sender, sender_display_name, body) = match &details.event {
        TimelineDetails::Ready(embedded) => {
            let sender = embedded.sender.to_string();
            let sender_display_name = match &embedded.sender_profile {
                TimelineDetails::Ready(profile) => profile.display_name.clone(),
                _ => None,
            };
            let body = embedded
                .content
                .as_message()
                .and_then(|m| text_body(m.msgtype()))
                .map(truncate_body)
                .unwrap_or_default();
            (sender, sender_display_name, body)
        }
        // The original's details are not loaded: render an honest but sparse
        // quote (empty sender/body), still not clickable if the key is absent.
        _ => (String::new(), None, String::new()),
    };

    ReplyPreviewVm {
        in_reply_to_key,
        sender,
        sender_display_name,
        body,
    }
}

/// Aggregate a message's emoji reactions into per-emoji [`ReactionGroupVm`]s
/// (Story 3.5, FR-12, NFR-9).
///
/// Pure: `content` and `own_user_id` are the only inputs. Reads
/// `content.reactions()` (`ReactionsByKeyBySender`, i.e.
/// `IndexMap<emoji, IndexMap<OwnedUserId, ReactionInfo>>`) and emits one group per
/// key **in the SDK's per-key insertion order**, with:
/// - `count` = the inner sender-map length (per-sender uniqueness is guaranteed by
///   the SDK, so this is the number of distinct reactors), and
/// - `is_own` = the account's own `user_id` is a key in that emoji's inner sender
///   map (catches a confirmed remote reaction and a pending local one alike).
///
/// Returns an empty vec for a non-reacted or non-message content. NO per-sender
/// user id or reaction event id crosses into the returned VMs (AD-1) — only the
/// aggregated `{ emoji, count, is_own }`.
pub fn reaction_groups(
    content: &TimelineItemContent,
    own_user_id: &UserId,
) -> Vec<ReactionGroupVm> {
    let Some(reactions) = content.reactions() else {
        return Vec::new();
    };
    // `ReactionsByKeyBySender` derefs to `IndexMap<emoji, IndexMap<OwnedUserId,
    // ReactionInfo>>` (per-key insertion order, per-sender uniqueness). Project
    // each key's inner sender map to `(emoji, count, is_own)` via the pure,
    // dependency-free [`aggregate_reactions`] helper (the SDK types have no public
    // constructor, so the aggregation logic is tested on that plain shape).
    let groups = reactions.iter().map(|(emoji, by_sender)| {
        (
            emoji.as_str(),
            by_sender.len(),
            by_sender.contains_key(own_user_id),
        )
    });
    aggregate_reactions(groups)
}

/// Pure aggregation of an ordered `(emoji, distinct_reactor_count, is_own)`
/// sequence into per-emoji [`ReactionGroupVm`]s, preserving the input order
/// (which mirrors the SDK's per-key insertion order) (Story 3.5, FR-12).
///
/// Split from [`reaction_groups`] so the count / own-highlight / order mapping is
/// unit-testable without an SDK `TimelineItemContent` (whose reaction map has no
/// public constructor). Introduces no new crate dependency — it names none of the
/// SDK's `IndexMap`/`ReactionInfo` types.
fn aggregate_reactions<'a>(
    groups: impl Iterator<Item = (&'a str, usize, bool)>,
) -> Vec<ReactionGroupVm> {
    groups
        .map(|(emoji, count, is_own)| ReactionGroupVm {
            emoji: emoji.to_owned(),
            // Saturate rather than wrap: an absurd reactor count stays honest
            // (`u32::MAX`) instead of a truncated `as` cast (project no-silent-loss).
            count: u32::try_from(count).unwrap_or(u32::MAX),
            is_own,
        })
        .collect()
}

/// Map one SDK [`TimelineItem`] to exactly one [`TimelineItemVm`], resolving a
/// reply's quoted-original key through the producer's `event_id → unique_id`
/// `index` and aggregating reactions against the account's own `own_user_id`.
///
/// An event item carrying a text `m.room.message` becomes a
/// [`TimelineItemVm::Message`] (with `is_edited` from `message.is_edited()` and a
/// `reply` preview from `content.in_reply_to()` via [`reply_preview`]); an event
/// the SDK could not decrypt (`MsgLikeKind::UnableToDecrypt`) becomes a
/// [`TimelineItemVm::Utd`] carrying only its stable key, sender, resolved display
/// name, and timestamp — never any ciphertext, session id, or key material
/// (NFR-9, AD-1) — so the frontend can render an honest stub instead of a blank
/// row. A redacted message (`MsgLikeKind::Redacted`) becomes a
/// [`TimelineItemVm::Redacted`] carrying the same non-secret render data, so a
/// deletion shows an explicit "Message deleted" stub rather than a silent gap
/// (Story 3.8). Everything else (non-text msgtype, other content kinds, and
/// virtual items) becomes a [`TimelineItemVm::Other`] carrying only the stable
/// opaque key, so diff indices stay aligned. All accessors are sync
/// (`VectorDiff::map` is sync).
pub fn item_to_vm(
    item: &TimelineItem,
    index: &ReplyIndex,
    own_user_id: &UserId,
    account_id: &str,
    room_id: &str,
) -> TimelineItemVm {
    let key = item.unique_id().0.clone();
    let TimelineItemKind::Event(ev) = item.kind() else {
        return TimelineItemVm::Other { key };
    };
    let TimelineItemContent::MsgLike(msg_like) = ev.content() else {
        return TimelineItemVm::Other { key };
    };

    let sender_display_name = match ev.sender_profile() {
        TimelineDetails::Ready(profile) => profile.display_name.clone(),
        _ => None,
    };

    match &msg_like.kind {
        MsgLikeKind::Message(message) => {
            let msgtype = message.msgtype();
            let media = media_vm(msgtype, account_id, room_id, &key);
            // The body is the text body for a text message, or the media caption
            // (which may be empty) for a media message. A non-text, non-media
            // msgtype with no media VM (e.g. location) stays an `Other`.
            let body = match (&media, text_body(msgtype)) {
                // Media message: the caption is the body (empty when none).
                (Some(m), _) => m.caption.clone().unwrap_or_default(),
                // Text message: its decoded body.
                (None, Some(text)) => text,
                // Neither media nor renderable text (e.g. location): render nothing.
                (None, None) => return TimelineItemVm::Other { key },
            };
            TimelineItemVm::Message {
                key,
                sender: ev.sender().to_string(),
                sender_display_name,
                body: truncate_body(body),
                timestamp: i64::from(ev.timestamp().0),
                is_own: ev.is_own(),
                send_state: ev.send_state().map(map_send_state),
                is_edited: message.is_edited(),
                reply: reply_preview(ev.content(), index),
                reactions: reaction_groups(ev.content(), own_user_id),
                media: media.map(Box::new),
            }
        }
        // An event that cannot be decrypted yet: surface an honest stub. No
        // ciphertext/session material is read from the `EncryptedMessage` — only
        // the sender, display name, and timestamp cross IPC (NFR-9, AD-1).
        MsgLikeKind::UnableToDecrypt(_) => TimelineItemVm::Utd {
            key,
            sender: ev.sender().to_string(),
            sender_display_name,
            timestamp: i64::from(ev.timestamp().0),
        },
        // A redacted (deleted-for-everyone) message: surface an explicit honest
        // stub in place of a blank gap. The redacted event has no body to read;
        // only the sender, display name, and timestamp cross IPC (NFR-9, AD-1)
        // (Story 3.8).
        MsgLikeKind::Redacted => TimelineItemVm::Redacted {
            key,
            sender: ev.sender().to_string(),
            sender_display_name,
            timestamp: i64::from(ev.timestamp().0),
        },
        _ => TimelineItemVm::Other { key },
    }
}

/// Pure conversion of an already-`TimelineItemVm` `VectorDiff` into a
/// [`TimelineOp`].
///
/// This is the unit-tested seam: it needs no live `Client`. Every eyeball-im
/// variant maps one-to-one to the corresponding op, preserving indices.
pub fn timeline_diff_to_op(diff: VectorDiff<TimelineItemVm>) -> TimelineOp {
    match diff {
        VectorDiff::Append { values } => TimelineOp::Append {
            items: values.into_iter().collect(),
        },
        VectorDiff::Clear => TimelineOp::Clear,
        VectorDiff::PushFront { value } => TimelineOp::PushFront { item: value },
        VectorDiff::PushBack { value } => TimelineOp::PushBack { item: value },
        VectorDiff::PopFront => TimelineOp::PopFront,
        VectorDiff::PopBack => TimelineOp::PopBack,
        VectorDiff::Insert { index, value } => TimelineOp::Insert {
            index: index as u32,
            item: value,
        },
        VectorDiff::Set { index, value } => TimelineOp::Set {
            index: index as u32,
            item: value,
        },
        VectorDiff::Remove { index } => TimelineOp::Remove {
            index: index as u32,
        },
        VectorDiff::Truncate { length } => TimelineOp::Truncate {
            length: length as u32,
        },
        VectorDiff::Reset { values } => TimelineOp::Reset {
            items: values.into_iter().collect(),
        },
    }
}

/// Record an event item's `event_id → unique_id` mapping into `index` so a later
/// reply whose original is this item can resolve its opaque jump key. A virtual
/// item, or an event item with no event id yet (an unsent local echo), is not
/// indexed. Idempotent per event id.
fn index_item(index: &mut ReplyIndex, item: &TimelineItem) {
    if let Some(ev) = item.as_event() {
        if let Some(event_id) = ev.event_id() {
            index.insert(event_id.to_owned(), item.unique_id().0.clone());
        }
    }
}

/// Map one `Arc<TimelineItem>` diff to a [`TimelineItemVm`] diff while keeping the
/// producer's `event_id → unique_id` `index` current.
///
/// Every carried item's `event_id → unique_id` is inserted **before** the batch is
/// mapped, so a reply and its already-mapped original resolve within the same
/// snapshot/diff pass. `Clear`/`Reset` reset the index (the whole set of loaded
/// originals is being replaced). The resulting VMs read the (now-updated) index so
/// each reply's preview carries its original's opaque render key.
fn map_diff_indexing(
    diff: VectorDiff<Arc<TimelineItem>>,
    index: &mut ReplyIndex,
    own_user_id: &UserId,
    account_id: &str,
    room_id: &str,
) -> VectorDiff<TimelineItemVm> {
    match &diff {
        VectorDiff::Clear => index.clear(),
        VectorDiff::Reset { values } => {
            index.clear();
            for item in values {
                index_item(index, item);
            }
        }
        VectorDiff::Append { values } => {
            for item in values {
                index_item(index, item);
            }
        }
        VectorDiff::PushFront { value }
        | VectorDiff::PushBack { value }
        | VectorDiff::Insert { value, .. }
        | VectorDiff::Set { value, .. } => index_item(index, value),
        // Removals/truncations leave stale entries in the index; that is harmless
        // — a stale key only ever produces an unresolvable jump the frontend
        // guards, and a later `Reset` rebuilds the index cleanly.
        VectorDiff::PopFront
        | VectorDiff::PopBack
        | VectorDiff::Remove { .. }
        | VectorDiff::Truncate { .. } => {}
    }
    diff.map(|item| item_to_vm(&item, index, own_user_id, account_id, room_id))
}

/// A boxed, `Send` timeline diff stream (the concrete `impl Stream` from
/// `Timeline::subscribe` named so it can cross the `open_timeline` return and
/// move into the producer task).
type TimelineDiffStream = Pin<Box<dyn Stream<Item = Vec<VectorDiff<Arc<TimelineItem>>>> + Send>>;

/// The live building blocks of a subscribed room timeline: the `Timeline` handle
/// (kept alive so its drop handle can later cancel the SDK's background tasks),
/// the cached snapshot, and the live diff stream.
///
/// The `Timeline` is an `Arc` so the exact same instance is shared between the
/// forwarder task and the account's send/retry lookup — `unique_id`s are only
/// stable within one `Timeline`, so send/retry MUST operate on the instance that
/// produced the subscribed items (AD-19).
pub struct OpenTimeline {
    timeline: Arc<Timeline>,
    initial: Vector<Arc<TimelineItem>>,
    stream: TimelineDiffStream,
    /// The account's own Matrix user id (from `client.user_id()` at open time).
    /// Threaded into [`item_to_vm`] so reaction aggregation can flag the user's
    /// own reaction (`reaction senders are separate from `ev.is_own()`).
    own_user_id: OwnedUserId,
    /// The opaque keeper account id, threaded into [`item_to_vm`] so a media
    /// message's `keeper-media://` URLs carry the correct account coordinate
    /// (Story 3.6, AD-4). Never a token or secret — the same opaque id the
    /// frontend already holds.
    account_id: String,
}

impl OpenTimeline {
    /// The shared `Arc<Timeline>` to register on the account's supervision state
    /// so send/retry can reach the exact instance that produced the items.
    pub fn timeline(&self) -> Arc<Timeline> {
        self.timeline.clone()
    }
}

/// Build a room's timeline and open its snapshot-then-diff subscription.
///
/// This is deliberately **synchronous** with respect to the subscribe command:
/// a missing room or a build failure returns a [`TimelineError`] to the caller
/// so it funnels to `TimelineUnavailable` (an honest inline error, not a silent
/// spinner — AC-4) and lets the caller tear down a just-activated partial
/// account before any producer task is spawned (AD-21). Only the diff-forwarding
/// loop ([`forward_timeline`]) runs in the background.
pub async fn open_timeline(
    client: &Client,
    room_id: &RoomId,
    account_id: &str,
) -> Result<OpenTimeline, TimelineError> {
    let room = client
        .get_room(room_id)
        .ok_or(TimelineError::RoomNotFound)?;
    // The account's own user id, captured once at open time so reaction
    // aggregation can flag the user's own reaction (reaction senders are separate
    // from an event's own-ness). A live, restored account always has one.
    let own_user_id = client
        .user_id()
        .ok_or_else(|| TimelineError::Build("no user id on the live client".to_owned()))?
        .to_owned();
    let timeline = room
        .timeline()
        .await
        .map_err(|e| TimelineError::Build(e.to_string()))?;
    let (initial, stream) = timeline.subscribe().await;
    Ok(OpenTimeline {
        timeline: Arc::new(timeline),
        initial,
        stream: Box::pin(stream),
        own_user_id,
        account_id: account_id.to_owned(),
    })
}

/// Emit the cached snapshot as a `Reset`, then forward each `VectorDiff` batch
/// verbatim to `sink`.
///
/// The `Timeline` is kept alive for the whole loop — its drop handle cancels the
/// SDK's background timeline tasks (AD-19). The producer breaks when the channel
/// closes (`sink` returns `false`) or the stream ends.
pub async fn forward_timeline(open: OpenTimeline, room_id: OwnedRoomId, sink: TimelineSink) {
    let OpenTimeline {
        timeline,
        initial,
        mut stream,
        own_user_id,
        account_id,
    } = open;

    // The room id string is stable for the whole producer; capture it once for the
    // media-URL coordinates threaded into every mapped item.
    let room_id_str = room_id.to_string();

    // The producer-owned `event_id → unique_id` index. Built from the snapshot,
    // then kept current across each diff so a reply resolves an earlier-mapped
    // original's opaque render key (never an event id crosses IPC).
    let mut index: ReplyIndex = HashMap::new();
    for item in initial.iter() {
        index_item(&mut index, item);
    }

    let reset = TimelineOp::Reset {
        items: initial
            .iter()
            .map(|i| item_to_vm(i, &index, &own_user_id, &account_id, &room_id_str))
            .collect(),
    };
    if !sink(TimelineBatch { ops: vec![reset] }) {
        tracing::info!(room_id = %room_id, "timeline channel closed before first batch");
        return;
    }

    while let Some(diffs) = stream.next().await {
        let ops = diffs
            .into_iter()
            .map(|d| {
                timeline_diff_to_op(map_diff_indexing(
                    d,
                    &mut index,
                    &own_user_id,
                    &account_id,
                    &room_id_str,
                ))
            })
            .collect();
        if !sink(TimelineBatch { ops }) {
            tracing::info!(room_id = %room_id, "timeline channel closed, stopping producer");
            break;
        }
    }
    tracing::info!(room_id = %room_id, "timeline stream ended");
    // `timeline` stays in scope until here; dropping this `Arc` reference now
    // releases the producer's hold. The SDK's background timeline tasks are
    // cancelled once the last reference is gone — the account also drops its
    // stored `Arc<Timeline>` on the same teardown path (natural completion /
    // unsubscribe), so nothing leaks (AD-19).
    drop(timeline);
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::ruma::events::room::message::{
        EmoteMessageEventContent, MessageType, NoticeMessageEventContent, TextMessageEventContent,
    };
    use matrix_sdk_ui::eyeball_im::Vector;

    fn message(key: &str) -> TimelineItemVm {
        TimelineItemVm::Message {
            key: key.to_owned(),
            sender: "@bob:example.org".to_owned(),
            sender_display_name: None,
            body: "hi".to_owned(),
            timestamp: 1,
            is_own: false,
            send_state: None,
            is_edited: false,
            reply: None,
            reactions: Vec::new(),
            media: None,
        }
    }

    fn other(key: &str) -> TimelineItemVm {
        TimelineItemVm::Other {
            key: key.to_owned(),
        }
    }

    #[test]
    fn redacted_vm_serializes_to_tagged_camel_case_shape() {
        // The `MsgLikeKind::Redacted` arm of `item_to_vm` produces this VM; an SDK
        // `EventTimelineItem` can't be constructed outside the SDK crate (its
        // constructor is `pub(super)`), so pin the wire contract the frontend
        // switches on — `kind: "redacted"` with the same non-secret render fields
        // as the UTD stub, and no body/content field ever crossing IPC.
        let vm = TimelineItemVm::Redacted {
            key: "item-7".to_owned(),
            sender: "@alice:example.org".to_owned(),
            sender_display_name: Some("Alice".to_owned()),
            timestamp: 1_720_000_000_000,
        };
        let json = serde_json::to_value(&vm).expect("VM serializes");
        assert_eq!(json["kind"], "redacted");
        assert_eq!(json["key"], "item-7");
        assert_eq!(json["sender"], "@alice:example.org");
        assert_eq!(json["senderDisplayName"], "Alice");
        assert_eq!(json["timestamp"], 1_720_000_000_000_i64);
        // No body/content is carried on a redacted stub (nothing to render).
        assert!(json.get("body").is_none());
    }

    #[test]
    fn map_send_state_not_sent_yet_is_sending() {
        let state = EventSendState::NotSentYet { progress: None };
        assert_eq!(map_send_state(&state), SendState::Sending);
    }

    #[test]
    fn map_send_state_sent_is_sent() {
        use matrix_sdk::ruma::owned_event_id;
        let state = EventSendState::Sent {
            event_id: owned_event_id!("$evt:example.org"),
        };
        assert_eq!(map_send_state(&state), SendState::Sent);
    }

    #[test]
    fn map_send_state_recoverable_failure_stays_sending() {
        let state = EventSendState::SendingFailed {
            error: Arc::new(matrix_sdk::Error::AuthenticationRequired),
            is_recoverable: true,
        };
        assert_eq!(map_send_state(&state), SendState::Sending);
    }

    #[test]
    fn map_send_state_unrecoverable_failure_is_failed() {
        let state = EventSendState::SendingFailed {
            error: Arc::new(matrix_sdk::Error::AuthenticationRequired),
            is_recoverable: false,
        };
        assert_eq!(map_send_state(&state), SendState::Failed);
    }

    #[test]
    fn text_body_extracts_text() {
        let mt = MessageType::Text(TextMessageEventContent::plain("hello"));
        assert_eq!(text_body(&mt), Some("hello".to_owned()));
    }

    #[test]
    fn text_body_extracts_notice() {
        let mt = MessageType::Notice(NoticeMessageEventContent::plain("notice body"));
        assert_eq!(text_body(&mt), Some("notice body".to_owned()));
    }

    #[test]
    fn text_body_extracts_emote() {
        let mt = MessageType::Emote(EmoteMessageEventContent::plain("waves"));
        assert_eq!(text_body(&mt), Some("waves".to_owned()));
    }

    fn json_object(value: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        value.as_object().expect("object literal").clone()
    }

    #[test]
    fn text_body_ignores_empty_and_whitespace() {
        assert_eq!(
            text_body(&MessageType::Text(TextMessageEventContent::plain(""))),
            None
        );
        assert_eq!(
            text_body(&MessageType::Text(TextMessageEventContent::plain(
                "   \n\t "
            ))),
            None
        );
    }

    #[test]
    fn text_body_ignores_image() {
        let mt = MessageType::new(
            "m.image",
            "photo.png".to_owned(),
            json_object(serde_json::json!({ "url": "mxc://example.org/abc" })),
        )
        .expect("construct image msgtype");
        assert_eq!(text_body(&mt), None);
    }

    fn image_msgtype() -> MessageType {
        MessageType::new(
            "m.image",
            "photo.png".to_owned(),
            json_object(serde_json::json!({
                "url": "mxc://example.org/abc",
                "info": { "w": 800, "h": 600, "mimetype": "image/png", "size": 12345 }
            })),
        )
        .expect("construct image msgtype")
    }

    #[test]
    fn media_vm_maps_image_with_dims_and_thumbnail() {
        let vm = media_vm(&image_msgtype(), "acct", "!room:h", "unique-1")
            .expect("image maps to a media vm");
        assert_eq!(vm.kind, MediaKindVm::Image);
        assert_eq!(vm.filename, "photo.png");
        assert_eq!(vm.mimetype.as_deref(), Some("image/png"));
        assert_eq!(vm.size, Some(12345));
        assert_eq!(vm.width, Some(800));
        assert_eq!(vm.height, Some(600));
        // Both variant URLs are opaque `keeper-media://` (never mxc).
        assert!(
            vm.url.starts_with("keeper-media://media/"),
            "url: {}",
            vm.url
        );
        assert!(!vm.url.contains("mxc"), "url leaked mxc: {}", vm.url);
        let thumb = vm.thumbnail_url.expect("image carries a thumbnail url");
        assert!(thumb.ends_with("/thumb"), "thumb: {thumb}");
        assert!(vm.url.ends_with("/full"), "full: {}", vm.url);
    }

    #[test]
    fn media_vm_maps_video_with_thumbnail() {
        let mt = MessageType::new(
            "m.video",
            "clip.mp4".to_owned(),
            json_object(serde_json::json!({
                "url": "mxc://example.org/vid",
                "info": { "w": 1280, "h": 720, "mimetype": "video/mp4", "size": 999 }
            })),
        )
        .expect("construct video msgtype");
        let vm = media_vm(&mt, "acct", "!room:h", "k").expect("video maps");
        assert_eq!(vm.kind, MediaKindVm::Video);
        assert_eq!(vm.width, Some(1280));
        assert_eq!(vm.height, Some(720));
        assert!(vm.thumbnail_url.is_some(), "video carries a thumbnail url");
    }

    #[test]
    fn media_vm_maps_audio_inline_no_thumbnail() {
        let mt = MessageType::new(
            "m.audio",
            "clip.ogg".to_owned(),
            json_object(serde_json::json!({
                "url": "mxc://example.org/aud",
                "info": { "mimetype": "audio/ogg", "size": 4096 }
            })),
        )
        .expect("construct audio msgtype");
        let vm = media_vm(&mt, "acct", "!room:h", "k").expect("audio maps");
        assert_eq!(vm.kind, MediaKindVm::Audio);
        assert_eq!(vm.mimetype.as_deref(), Some("audio/ogg"));
        assert_eq!(vm.size, Some(4096));
        // Audio has no bubble thumbnail and no dimensions.
        assert_eq!(vm.thumbnail_url, None);
        assert_eq!(vm.width, None);
        assert_eq!(vm.height, None);
    }

    #[test]
    fn media_vm_maps_file_chip_no_thumbnail() {
        let mt = MessageType::new(
            "m.file",
            "report.pdf".to_owned(),
            json_object(serde_json::json!({
                "url": "mxc://example.org/file",
                "info": { "mimetype": "application/pdf", "size": 54321 }
            })),
        )
        .expect("construct file msgtype");
        let vm = media_vm(&mt, "acct", "!room:h", "k").expect("file maps");
        assert_eq!(vm.kind, MediaKindVm::File);
        assert_eq!(vm.filename, "report.pdf");
        assert_eq!(vm.mimetype.as_deref(), Some("application/pdf"));
        assert_eq!(vm.size, Some(54321));
        assert_eq!(vm.thumbnail_url, None);
    }

    #[test]
    fn media_vm_returns_none_for_text() {
        let mt = MessageType::Text(TextMessageEventContent::plain("hello"));
        assert_eq!(media_vm(&mt, "a", "r", "k"), None);
    }

    #[test]
    fn media_vm_uses_body_as_filename_fallback() {
        // No explicit `filename` field → `.filename()` falls back to the body.
        let mt = MessageType::new(
            "m.image",
            "the body caption".to_owned(),
            json_object(serde_json::json!({ "url": "mxc://example.org/x" })),
        )
        .expect("construct image msgtype");
        let vm = media_vm(&mt, "a", "r", "k").expect("image maps");
        assert_eq!(vm.filename, "the body caption");
        // No `info` → no dimensions / mimetype / size.
        assert_eq!(vm.mimetype, None);
        assert_eq!(vm.size, None);
    }

    #[test]
    fn text_body_ignores_other_msgtype() {
        let mt = MessageType::new(
            "m.location",
            "here".to_owned(),
            json_object(serde_json::json!({ "geo_uri": "geo:1,2" })),
        )
        .expect("construct location msgtype");
        assert_eq!(text_body(&mt), None);
    }

    #[test]
    fn reply_preview_resolves_key_from_index_when_original_loaded() {
        use matrix_sdk::ruma::owned_event_id;
        use matrix_sdk_ui::timeline::{InReplyToDetails, TimelineDetails};

        let event_id = owned_event_id!("$orig:example.org");
        let mut index = ReplyIndex::new();
        index.insert(event_id.clone(), "unique-orig".to_owned());

        // Details unavailable (an SDK `Message` can't be built here), so the
        // sender/body fall back to empty — but the jump key still resolves from
        // the index, which is the event-id-free mapping under test.
        let details = InReplyToDetails {
            event_id,
            event: TimelineDetails::Unavailable,
        };
        let vm = reply_preview_from_details(&details, &index);
        assert_eq!(vm.in_reply_to_key, Some("unique-orig".to_owned()));
        assert_eq!(vm.sender, "");
        assert_eq!(vm.body, "");
    }

    #[test]
    fn aggregate_reactions_empty_yields_no_groups() {
        let out = aggregate_reactions(std::iter::empty());
        assert!(out.is_empty());
    }

    #[test]
    fn aggregate_reactions_counts_per_emoji_and_flags_own_preserving_order() {
        // Mirrors the I/O matrix "aggregate multiple reactors" row: 👍 by 3 (not
        // own), ❤️ by 1 (own). Input order is the SDK's per-key insertion order.
        let groups = vec![("👍", 3usize, false), ("❤️", 1usize, true)];
        let out = aggregate_reactions(groups.into_iter());
        assert_eq!(
            out,
            vec![
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
            ]
        );
    }

    #[test]
    fn aggregate_reactions_flags_own_true_when_present() {
        let out = aggregate_reactions(std::iter::once(("🔥", 2usize, true)));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].emoji, "🔥");
        assert_eq!(out[0].count, 2);
        assert!(out[0].is_own);
    }

    #[test]
    fn reply_preview_yields_null_key_when_original_not_indexed() {
        use matrix_sdk::ruma::owned_event_id;
        use matrix_sdk_ui::timeline::{InReplyToDetails, TimelineDetails};

        // The original is not in the index (not loaded / already scrolled away):
        // the quote still renders honestly but is not clickable (`null` key).
        let details = InReplyToDetails {
            event_id: owned_event_id!("$missing:example.org"),
            event: TimelineDetails::Unavailable,
        };
        let vm = reply_preview_from_details(&details, &ReplyIndex::new());
        assert_eq!(vm.in_reply_to_key, None);
    }

    #[test]
    fn truncate_body_caps_long_bodies_by_char() {
        let long = "é".repeat(MAX_BODY_CHARS + 100);
        let out = truncate_body(long);
        assert_eq!(out.chars().count(), MAX_BODY_CHARS);
        assert!(out.chars().all(|c| c == 'é'));
    }

    #[test]
    fn truncate_body_keeps_short_bodies() {
        assert_eq!(truncate_body("hello".to_owned()), "hello");
    }

    #[test]
    fn op_reset() {
        let diff = VectorDiff::Reset {
            values: Vector::from_iter([message("a"), other("b")]),
        };
        assert_eq!(
            timeline_diff_to_op(diff),
            TimelineOp::Reset {
                items: vec![message("a"), other("b")],
            }
        );
    }

    #[test]
    fn op_append() {
        let diff = VectorDiff::Append {
            values: Vector::from_iter([message("a")]),
        };
        assert_eq!(
            timeline_diff_to_op(diff),
            TimelineOp::Append {
                items: vec![message("a")],
            }
        );
    }

    #[test]
    fn op_clear() {
        assert_eq!(
            timeline_diff_to_op(VectorDiff::<TimelineItemVm>::Clear),
            TimelineOp::Clear
        );
    }

    #[test]
    fn op_push_front_and_back() {
        assert_eq!(
            timeline_diff_to_op(VectorDiff::PushFront {
                value: message("a"),
            }),
            TimelineOp::PushFront { item: message("a") }
        );
        assert_eq!(
            timeline_diff_to_op(VectorDiff::PushBack { value: other("b") }),
            TimelineOp::PushBack { item: other("b") }
        );
    }

    #[test]
    fn op_pop_front_and_back() {
        assert_eq!(
            timeline_diff_to_op(VectorDiff::<TimelineItemVm>::PopFront),
            TimelineOp::PopFront
        );
        assert_eq!(
            timeline_diff_to_op(VectorDiff::<TimelineItemVm>::PopBack),
            TimelineOp::PopBack
        );
    }

    #[test]
    fn op_insert_and_set() {
        assert_eq!(
            timeline_diff_to_op(VectorDiff::Insert {
                index: 2,
                value: message("a"),
            }),
            TimelineOp::Insert {
                index: 2,
                item: message("a"),
            }
        );
        assert_eq!(
            timeline_diff_to_op(VectorDiff::Set {
                index: 5,
                value: other("b"),
            }),
            TimelineOp::Set {
                index: 5,
                item: other("b"),
            }
        );
    }

    #[test]
    fn op_remove_and_truncate() {
        assert_eq!(
            timeline_diff_to_op(VectorDiff::<TimelineItemVm>::Remove { index: 4 }),
            TimelineOp::Remove { index: 4 }
        );
        assert_eq!(
            timeline_diff_to_op(VectorDiff::<TimelineItemVm>::Truncate { length: 3 }),
            TimelineOp::Truncate { length: 3 }
        );
    }
}
