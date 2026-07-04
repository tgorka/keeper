//! The single outgoing-send dispatch gate (FR-41, AD-13).
//!
//! [`submit`] is the *only* function in the whole crate that feeds new content to
//! the SDK send queue — the sole call site of `Timeline::send(..)`. A
//! `#[cfg(test)]` source scan asserts this invariant holds. [`retry`] re-drives an
//! *already-dispatched* wedged local echo via its `SendHandle::unwedge()` (same
//! `unique_id`, no duplicate bubble); it feeds no new content, so it does not
//! violate the single-gate rule.
//!
//! Secret containment (NFR-9): neither the message body, a txn id, an event id,
//! nor a token ever reaches `tracing` — logs carry the opaque room id only, via
//! the caller. This module itself logs nothing secret.

use matrix_sdk::room::edit::EditedContent;
use matrix_sdk::ruma::events::room::message::{
    RoomMessageEventContent, RoomMessageEventContentWithoutRelation,
};
use matrix_sdk::ruma::events::AnyMessageLikeEventContent;
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk_ui::timeline::Timeline;

use crate::error::SendError;

/// What caused a content dispatch through the single send gate (AD-13).
///
/// Seeds the two triggers the send-gate contract names; only [`SendTrigger::ComposerSend`]
/// is used this story. [`SendTrigger::ApprovalPaneApprove`] is reserved for the
/// later approval-pane epic and is intentionally unused here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTrigger {
    /// A message the user composed and sent from the composer.
    ComposerSend,
    /// A message dispatched by approving it in the approval pane (later epic).
    ApprovalPaneApprove,
}

/// The sole content-dispatch gate (FR-41, AD-13): enqueue `text` as a plain-text
/// `m.room.message` on `timeline`'s send queue.
///
/// This is the only place in the crate that calls `Timeline::send(..)`. The
/// resulting `SendHandle` is intentionally dropped — the message's local echo and
/// every subsequent send-state transition arrive through the room's existing
/// `Timeline::subscribe()` diff stream, so keeper never synthesizes echo. The
/// caller is responsible for the trim-guard; an empty `text` is treated as a
/// no-op here defensively.
pub async fn submit(
    timeline: &Timeline,
    text: &str,
    trigger: SendTrigger,
) -> Result<(), SendError> {
    if text.trim().is_empty() {
        // Defensive: the composer already guards this, but never feed an empty
        // body to the queue.
        return Ok(());
    }
    let _ = trigger;
    let content =
        AnyMessageLikeEventContent::RoomMessage(RoomMessageEventContent::text_plain(text));
    // SOLE-SEND-GATE: the one and only `Timeline::send` call site (FR-41 guard).
    timeline
        .send(content)
        .await
        .map_err(|e| SendError::Dispatch(e.to_string()))?;
    Ok(())
}

/// Dispatch a plain-text reply to the message addressed by `in_reply_to_key`
/// (the *original* item's opaque `unique_id`) through the send gate (FR-41,
/// AD-13).
///
/// Resolves the key to the original's `OwnedEventId` by scanning
/// `timeline.items()` (mirroring [`retry`]'s item scan), then enqueues the reply
/// via `Timeline::send_reply` — the sole call site of that SDK method. The reply's
/// local echo (carrying its own `reply` preview) and every send-state transition
/// arrive through the room's existing `Timeline::subscribe()` diff stream, so
/// keeper synthesizes nothing. An empty body is a defensive no-op (the composer
/// already guards it).
///
/// Errors: an unresolvable key / an original with no event id →
/// [`SendError::TargetNotFound`]; an SDK enqueue failure → [`SendError::Dispatch`].
pub async fn submit_reply(
    timeline: &Timeline,
    in_reply_to_key: &str,
    text: &str,
) -> Result<(), SendError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let event_id: OwnedEventId = {
        let items = timeline.items().await;
        items
            .iter()
            .find(|item| item.unique_id().0 == in_reply_to_key)
            .and_then(|item| item.as_event())
            .and_then(|ev| ev.event_id())
            .map(|id| id.to_owned())
            .ok_or(SendError::TargetNotFound)?
    };
    let content = RoomMessageEventContentWithoutRelation::text_plain(text);
    // SOLE-REPLY-GATE: the one and only `Timeline::send_reply` call site (FR-41).
    timeline
        .send_reply(content, event_id)
        .await
        .map_err(|e| SendError::Dispatch(e.to_string()))?;
    Ok(())
}

/// Dispatch an in-place text edit of the message addressed by `item_key` (its
/// opaque `unique_id`) through the send gate (FR-41, AD-13).
///
/// Resolves the key to the live timeline item, gates on
/// `EventTimelineItem::is_editable()` (own + text), takes its `identifier()`
/// (`TimelineEventItemId`), and enqueues the edit via `Timeline::edit` — the sole
/// call site of that SDK method. The `Set` diff that replaces the content in place
/// (and flips `is_edited`) arrives through the existing `Timeline::subscribe()`
/// stream. An empty body is a defensive no-op.
///
/// Errors: an unresolvable key → [`SendError::TargetNotFound`]; a non-editable
/// target (not own / not text) → [`SendError::NotEditable`]; an SDK enqueue
/// failure → [`SendError::Dispatch`].
pub async fn submit_edit(
    timeline: &Timeline,
    item_key: &str,
    new_text: &str,
) -> Result<(), SendError> {
    if new_text.trim().is_empty() {
        return Ok(());
    }
    let item_id = {
        let items = timeline.items().await;
        let event = items
            .iter()
            .find(|item| item.unique_id().0 == item_key)
            .and_then(|item| item.as_event())
            .ok_or(SendError::TargetNotFound)?;
        if !event.is_editable() {
            return Err(SendError::NotEditable);
        }
        event.identifier()
    };
    let content =
        EditedContent::RoomMessage(RoomMessageEventContentWithoutRelation::text_plain(new_text));
    // SOLE-EDIT-GATE: the one and only `Timeline::edit` call site (FR-41).
    timeline
        .edit(&item_id, content)
        .await
        .map_err(|e| SendError::Dispatch(e.to_string()))?;
    Ok(())
}

/// Re-drive an already-dispatched, wedged local echo (NOT a new dispatch).
///
/// Locates the timeline item whose `unique_id().0 == item_key`, takes its
/// `local_echo_send_handle()`, and calls `unwedge()` — re-driving the *existing*
/// request in place (same `unique_id`, no duplicate bubble). Because it feeds no
/// new content, it does not touch the single-gate invariant.
pub async fn retry(timeline: &Timeline, item_key: &str) -> Result<(), SendError> {
    let items = timeline.items().await;
    let handle = items
        .iter()
        .find(|item| item.unique_id().0 == item_key)
        .and_then(|item| item.as_event())
        .and_then(|ev| ev.local_echo_send_handle())
        .ok_or(SendError::EchoNotFound)?;
    handle
        .unwedge()
        .await
        .map_err(|e| SendError::Dispatch(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    /// FR-41 / AD-13 single-dispatch-gate guard: the SDK content-send call
    /// (`.send(content)` on a `Timeline`) appears exactly once in this module,
    /// and that one call site is inside `submit`.
    ///
    /// The scan is robust to false matches: `submit` calls it as `.send(content)`;
    /// `retry` re-drives via `.unwedge()` (never `.send`), and the `retry`
    /// item-scan uses `.send_state`/`local_echo_send_handle` — none of which match
    /// the `.send(content)` pattern. The production source is isolated from this
    /// `#[cfg(test)]` module (whose text mentions the call form) before scanning.
    #[test]
    fn submit_is_the_sole_send_dispatch_gate() {
        // Scan only the production source: split off this `#[cfg(test)]` module so
        // the guard's own string literals (which mention the call form) never
        // count as call sites. The split marker below is the sole `mod tests`
        // opener in this file.
        let full = include_str!("send.rs");
        let source = full
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source precedes the test module");

        // The single SDK content-dispatch call site: `.send(content)`. Doc/comment
        // references say `Timeline::send`, not `.send(content)`, so they don't
        // match; `retry` never uses `.send(`.
        let call_sites: Vec<usize> = source
            .match_indices(".send(content)")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            call_sites.len(),
            1,
            "expected exactly one `.send(content)` call site (the single gate); found {}",
            call_sites.len()
        );

        let submit_start = source
            .find("pub async fn submit")
            .expect("submit fn must exist");
        let submit_reply_start = source
            .find("pub async fn submit_reply")
            .expect("submit_reply fn must exist");
        let submit_edit_start = source
            .find("pub async fn submit_edit")
            .expect("submit_edit fn must exist");
        let retry_start = source
            .find("pub async fn retry")
            .expect("retry fn must exist");
        let call = call_sites[0];
        // The single `.send(content)` must live in `submit` — before `submit_reply`
        // (the first fn following `submit`).
        assert!(
            call > submit_start && call < submit_reply_start,
            "the sole `timeline.send(` call must be inside `submit` (offset {call} not within {submit_start}..{submit_reply_start})"
        );

        // The single reply-dispatch call site: `.send_reply(`. Doc references say
        // `Timeline::send_reply`, not `.send_reply(`, so they don't match.
        let reply_sites: Vec<usize> = source
            .match_indices(".send_reply(")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            reply_sites.len(),
            1,
            "expected exactly one `.send_reply(` call site (the sole reply gate); found {}",
            reply_sites.len()
        );
        let reply_call = reply_sites[0];
        assert!(
            reply_call > submit_reply_start && reply_call < submit_edit_start,
            "the sole `.send_reply(` call must be inside `submit_reply` (offset {reply_call} not within {submit_reply_start}..{submit_edit_start})"
        );

        // The single edit-dispatch call site: `.edit(`. Doc references say
        // `Timeline::edit` (no `.edit(`) and `is_editable()` is a different token,
        // so neither matches.
        let edit_sites: Vec<usize> = source.match_indices(".edit(").map(|(i, _)| i).collect();
        assert_eq!(
            edit_sites.len(),
            1,
            "expected exactly one `.edit(` call site (the sole edit gate); found {}",
            edit_sites.len()
        );
        let edit_call = edit_sites[0];
        assert!(
            edit_call > submit_edit_start && edit_call < retry_start,
            "the sole `.edit(` call must be inside `submit_edit` (offset {edit_call} not within {submit_edit_start}..{retry_start})"
        );
    }
}
