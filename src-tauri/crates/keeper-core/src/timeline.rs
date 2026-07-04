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

use std::pin::Pin;
use std::sync::Arc;

use futures_util::{Stream, StreamExt};
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::{OwnedRoomId, RoomId};
use matrix_sdk::Client;
use matrix_sdk_ui::eyeball_im::{Vector, VectorDiff};
use matrix_sdk_ui::timeline::{
    EventSendState, MsgLikeKind, RoomExt, Timeline, TimelineDetails, TimelineItem,
    TimelineItemContent, TimelineItemKind,
};

use crate::account::TimelineSink;
use crate::error::TimelineError;
use crate::vm::{SendState, TimelineBatch, TimelineItemVm, TimelineOp};

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

/// Map one SDK [`TimelineItem`] to exactly one [`TimelineItemVm`].
///
/// An event item carrying a text `m.room.message` becomes a
/// [`TimelineItemVm::Message`]; everything else (non-text msgtype, other
/// content kinds, redacted, undecryptable, and virtual items) becomes a
/// [`TimelineItemVm::Other`] carrying only the stable opaque key, so diff
/// indices stay aligned. All accessors are sync (`VectorDiff::map` is sync).
pub fn item_to_vm(item: &TimelineItem) -> TimelineItemVm {
    let key = item.unique_id().0.clone();
    let TimelineItemKind::Event(ev) = item.kind() else {
        return TimelineItemVm::Other { key };
    };
    let TimelineItemContent::MsgLike(msg_like) = ev.content() else {
        return TimelineItemVm::Other { key };
    };
    let MsgLikeKind::Message(message) = &msg_like.kind else {
        return TimelineItemVm::Other { key };
    };
    let Some(body) = text_body(message.msgtype()) else {
        return TimelineItemVm::Other { key };
    };

    let sender_display_name = match ev.sender_profile() {
        TimelineDetails::Ready(profile) => profile.display_name.clone(),
        _ => None,
    };

    TimelineItemVm::Message {
        key,
        sender: ev.sender().to_string(),
        sender_display_name,
        body: truncate_body(body),
        timestamp: i64::from(ev.timestamp().0),
        is_own: ev.is_own(),
        send_state: ev.send_state().map(map_send_state),
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
) -> Result<OpenTimeline, TimelineError> {
    let room = client
        .get_room(room_id)
        .ok_or(TimelineError::RoomNotFound)?;
    let timeline = room
        .timeline()
        .await
        .map_err(|e| TimelineError::Build(e.to_string()))?;
    let (initial, stream) = timeline.subscribe().await;
    Ok(OpenTimeline {
        timeline: Arc::new(timeline),
        initial,
        stream: Box::pin(stream),
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
    } = open;

    let reset = TimelineOp::Reset {
        items: initial.iter().map(|i| item_to_vm(i)).collect(),
    };
    if !sink(TimelineBatch { ops: vec![reset] }) {
        tracing::info!(room_id = %room_id, "timeline channel closed before first batch");
        return;
    }

    while let Some(diffs) = stream.next().await {
        let ops = diffs
            .into_iter()
            .map(|d| timeline_diff_to_op(d.map(|i| item_to_vm(&i))))
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
        }
    }

    fn other(key: &str) -> TimelineItemVm {
        TimelineItemVm::Other {
            key: key.to_owned(),
        }
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
