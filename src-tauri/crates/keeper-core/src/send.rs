//! The single outgoing-send dispatch gate (FR-41, AD-13).
//!
//! [`dispatch`] is the *only* function in the whole crate that feeds new content to
//! the SDK send queue — the sole call site of `Timeline::send(..)`. [`submit`] (the
//! immediate user path, window == 0) and the outbox scheduler (the deferred
//! completion of an already-approved hold once its Undo-Send window elapses,
//! Story 8.3) both funnel through it; the scheduler is the only non-[`submit`]
//! caller of [`dispatch`]. A `#[cfg(test)]` source scan asserts this invariant
//! holds. [`retry`] re-drives an
//! *already-dispatched* wedged local echo via its `SendHandle::unwedge()` (same
//! `unique_id`, no duplicate bubble); it feeds no new content, so it does not
//! violate the single-gate rule.
//!
//! # The explicit-approval airlock invariant (AD-13) — a binding contract
//!
//! This is the contract that the future agent-proposal features are built on;
//! treat it as binding, not advisory.
//!
//! - **Exactly two user-initiated dispatch triggers exist:**
//!   [`SendTrigger::ComposerSend`] (the user sends from the composer) and
//!   [`SendTrigger::ApprovalPaneApprove`] (the user approves a pending draft in the
//!   approval pane, Story 7.3). Both — and only these two — flow through the single
//!   [`submit`] gate. [`SendTrigger`] is a **closed set** of exactly these two.
//! - **No background, scheduled, automated, or bulk dispatch path exists or may be
//!   added.** There is no timer, no queue drainer, no `approve-all`, and no
//!   *unattended*, scheduled, or bulk send API. Every new plain-text message that
//!   goes through [`submit`] is the direct, per-message result of a user action.
//!   ([`submit`] is `pub` so the two triggers can call it — the guard is that every
//!   dispatch carries a caller-supplied user-initiated trigger, not that the function
//!   is private.)
//! - **Agents may *propose*; only the *user* approves.** A future agent contributes
//!   by *writing a draft* (`dev.keeper.draft` account data) — a proposal is a stored
//!   draft, never a dispatch. Turning a proposal into a sent message always passes
//!   through the user pressing approve in the pane ([`SendTrigger::ApprovalPaneApprove`]).
//!   Writing a draft never reaches this gate.
//! - **Adding a trigger or any unattended send path is a new planning-level
//!   decision, not merely a code change.** A third [`SendTrigger`] variant, a third
//!   [`submit`] caller, or any background/scheduled/automated/bulk send path is an
//!   invariant breach: it must be raised as a planning decision, not slipped in as
//!   an edit. Never add a `_ =>` wildcard arm to a [`SendTrigger`] match — it would
//!   silently absorb a new variant and defeat the exhaustiveness gate.
//!
//! **Scope.** This two-trigger airlock governs *new plain-text message origination*
//! through [`submit`]. The sibling gates [`submit_reply`], [`submit_edit`],
//! [`toggle_reaction`], [`redact`], and [`submit_attachment`] dispatch replies,
//! edits, reactions, redactions, and media — each locked to a single call site by
//! `submit_is_the_sole_send_dispatch_gate` and carrying no [`SendTrigger`]; read
//! receipts and typing are AD-14 signals; draft mirroring writes account data. Those
//! sit outside the [`SendTrigger`] accounting by design: the airlock is about the
//! compose-vs-send separation for new messages, not every outbound verb.
//!
//! Enforcing guard tests (all in this module's `#[cfg(test)]` block plus the sibling
//! `account.rs` scan they read): `submit_is_the_sole_send_dispatch_gate` (the SDK
//! send verbs each stay one call site), `exactly_two_legal_dispatch_triggers` (the
//! wildcard-free exhaustiveness gate — a third variant fails to compile), and
//! `submit_has_exactly_the_two_user_initiated_callers` (a source scan of production
//! `account.rs` — a third or background [`submit`] caller fails the count).
//!
//! Secret containment (NFR-9): neither the message body, a txn id, an event id,
//! nor a token ever reaches `tracing` — logs carry the opaque room id only, via
//! the caller. This module itself logs nothing secret.

use matrix_sdk::room::edit::EditedContent;
use matrix_sdk::ruma::events::room::message::{
    RoomMessageEventContent, RoomMessageEventContentWithoutRelation, TextMessageEventContent,
};
use matrix_sdk::ruma::events::AnyMessageLikeEventContent;
use matrix_sdk::ruma::OwnedEventId;
use matrix_sdk_ui::timeline::{AttachmentConfig, AttachmentSource, Timeline};
use mime::Mime;

use crate::error::SendError;

/// What caused a content dispatch through the single send gate (AD-13).
///
/// This is a **closed set** of exactly two user-initiated triggers, the only two the
/// send-gate contract names: [`SendTrigger::ComposerSend`] (a composer send) and
/// [`SendTrigger::ApprovalPaneApprove`] (approving a pending draft in the approval
/// pane, Story 7.3). These are the only two legal dispatch triggers; both flow
/// through the single [`submit`] gate. Adding a third variant is an AD-13 invariant
/// breach requiring a new planning-level decision (see the module-level airlock
/// contract), and it will fail the wildcard-free exhaustiveness gate
/// (`exactly_two_legal_dispatch_triggers`) at compile time. No match over this enum
/// may use a `_ =>` wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendTrigger {
    /// A message the user composed and sent from the composer.
    ComposerSend,
    /// A message dispatched by approving a pending draft in the approval pane
    /// (Story 7.3).
    ApprovalPaneApprove,
}

impl SendTrigger {
    /// A non-secret label for this trigger, safe to log (NFR-9): it carries no
    /// body, no txn/event id, and no token.
    fn as_label(self) -> &'static str {
        match self {
            SendTrigger::ComposerSend => "composer_send",
            SendTrigger::ApprovalPaneApprove => "approval_pane_approve",
        }
    }
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
    // Record only the non-secret trigger label (never the body / ids / tokens).
    tracing::debug!(
        trigger = trigger.as_label(),
        "dispatching content through send gate"
    );
    dispatch(timeline, text).await
}

/// The sole SDK-enqueue primitive (FR-41, AD-13): enqueue `text` as a plain-text
/// `m.room.message` on `timeline`'s send queue. **This is the only place in the crate
/// that calls `Timeline::send(..)`.**
///
/// Both legal completion paths funnel through here: the *immediate* user path
/// ([`submit`], window == 0) and the *deferred* completion path (the outbox scheduler,
/// which finishes a hold already approved under one of the two [`SendTrigger`]s once its
/// Undo-Send window elapses — Story 8.3). The scheduler is the only non-[`submit`]
/// caller of `dispatch`; it mints no new [`SendTrigger`] because completing an
/// already-approved hold is not a new dispatch decision (AD-13). The resulting
/// `SendHandle` is intentionally dropped — the local echo and every send-state
/// transition arrive through the room's existing `Timeline::subscribe()` diff stream, so
/// keeper never synthesizes echo.
pub(crate) async fn dispatch(timeline: &Timeline, text: &str) -> Result<(), SendError> {
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

/// Toggle the current account's emoji reaction on the message addressed by
/// `item_key` (its opaque `unique_id`) through the send gate (FR-41, AD-13,
/// Story 3.5, FR-12).
///
/// Resolves the key to the live timeline item and its `identifier()`
/// (`TimelineEventItemId`) via the same items scan as [`submit_edit`], then calls
/// `Timeline::toggle_reaction` — the sole call site of that SDK method. The call
/// adds the reaction if absent and retracts it if the account already reacted with
/// `emoji` (symmetric toggle); the returned added/removed bool is ignored — the
/// updated reaction set arrives as a `Set` diff through the room's existing
/// `Timeline::subscribe()` stream, so keeper synthesizes nothing.
///
/// Errors: an unresolvable key → [`SendError::TargetNotFound`]; an SDK dispatch
/// failure → [`SendError::Dispatch`].
pub async fn toggle_reaction(
    timeline: &Timeline,
    item_key: &str,
    emoji: &str,
) -> Result<(), SendError> {
    let item_id = {
        let items = timeline.items().await;
        items
            .iter()
            .find(|item| item.unique_id().0 == item_key)
            .and_then(|item| item.as_event())
            .map(|ev| ev.identifier())
            .ok_or(SendError::TargetNotFound)?
    };
    // SOLE-REACTION-GATE: the one and only `Timeline::toggle_reaction` call site
    // (FR-41). The added/removed bool is intentionally ignored — the diff stream
    // carries the resulting pill state.
    timeline
        .toggle_reaction(&item_id, emoji)
        .await
        .map_err(|e| SendError::Dispatch(e.to_string()))?;
    Ok(())
}

/// Redact (delete for everyone) the message addressed by `item_key` (its opaque
/// `unique_id`) through the single dispatch gate (FR-15, FR-41, AD-13, Story 3.8).
///
/// Resolves the key to the live timeline item and its `identifier()`
/// (`TimelineEventItemId`) via the same items scan as [`submit_edit`], then calls
/// `Timeline::redact` — the sole call site of that SDK method. `reason` is an
/// optional non-secret redaction reason (MVP passes `None`). The `Set` diff that
/// turns the item into a redacted stub in place arrives through the room's
/// existing `Timeline::subscribe()` stream, so keeper synthesizes nothing.
///
/// Errors: an unresolvable key, or a target that is not the account's own message
/// → [`SendError::TargetNotFound`]; an SDK dispatch failure → [`SendError::Dispatch`].
pub async fn redact(
    timeline: &Timeline,
    item_key: &str,
    reason: Option<&str>,
) -> Result<(), SendError> {
    let item_id = {
        let items = timeline.items().await;
        let event = items
            .iter()
            .find(|item| item.unique_id().0 == item_key)
            .and_then(|item| item.as_event())
            .ok_or(SendError::TargetNotFound)?;
        // Defense-in-depth: delete-for-everyone is scoped to the user's OWN messages
        // (Story 3.8). The webview only offers Delete on own bubbles, but the
        // destructive, network-visible protocol decision is enforced here in Rust too
        // (mirrors `submit_edit`'s `is_editable()` gate) — a misbehaving or compromised
        // caller cannot redact someone else's message through this command.
        if !event.is_own() {
            return Err(SendError::TargetNotFound);
        }
        event.identifier()
    };
    // SOLE-REDACT-GATE: the one and only `Timeline::redact` call site (FR-41).
    timeline
        .redact(&item_id, reason)
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

/// The sole media-dispatch gate (FR-41, AD-13, Story 3.7, FR-13): enqueue an
/// attachment (`bytes` + `filename` + `mime`) on `timeline`'s send queue as an
/// `m.room.message` media event, optionally carrying `caption` as its caption.
///
/// This is the only place in the crate that calls `Timeline::send_attachment(..)`.
/// `.use_send_queue()` makes the whole text send-plumbing apply to media: the SDK
/// produces a local-echo timeline item (which the 3.6 receive path renders), drives
/// the upload in its background send-queue task, encrypts automatically in E2EE
/// rooms, emits send-state transitions over the room's existing
/// `Timeline::subscribe()` diff stream, and lets `retry` (`unwedge`) / `cancel`
/// (`abort`) operate on the echo — so keeper synthesizes nothing. MVP sends with a
/// minimal `AttachmentConfig` (no client-generated thumbnail / `info`); receivers
/// fall back to the full asset per 3.6. Bytes never touch `tracing`.
pub async fn submit_attachment(
    timeline: &Timeline,
    bytes: Vec<u8>,
    filename: &str,
    mime: Mime,
    caption: Option<&str>,
) -> Result<(), SendError> {
    let source = AttachmentSource::Data {
        bytes,
        filename: filename.to_owned(),
    };
    // The timeline's `AttachmentConfig` exposes public fields (no builder); MVP
    // sends with only an optional caption — no client-generated thumbnail / `info`.
    let config = AttachmentConfig {
        caption: caption
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(TextMessageEventContent::plain),
        ..AttachmentConfig::default()
    };
    // SOLE-ATTACHMENT-GATE: the one and only `Timeline::send_attachment` call site
    // (FR-41 guard). `.use_send_queue()` routes it through the text send plumbing.
    timeline
        .send_attachment(source, mime, config)
        .use_send_queue()
        .await
        .map_err(|e| SendError::Upload(e.to_string()))?;
    Ok(())
}

/// Cancel an in-flight (or queued) local echo by aborting its SDK send handle —
/// best-effort, symmetric with [`retry`]'s `unwedge` (Story 3.7).
///
/// Locates the timeline item whose `unique_id().0 == item_key`, takes its
/// `local_echo_send_handle()`, and calls `abort()`. `Ok(true)` means the send was
/// aborted (the SDK emits a `CancelledLocalEvent` diff that removes the echo);
/// `Ok(false)` means it had already been dispatched — a no-op that leaves the
/// message sent. Feeds no new content, so it does not touch the single-gate rule.
pub async fn cancel(timeline: &Timeline, item_key: &str) -> Result<(), SendError> {
    let items = timeline.items().await;
    let handle = items
        .iter()
        .find(|item| item.unique_id().0 == item_key)
        .and_then(|item| item.as_event())
        .and_then(|ev| ev.local_echo_send_handle())
        .ok_or(SendError::EchoNotFound)?;
    // Best-effort: `Ok(false)` (already dispatched) is not an error — the message
    // simply stays sent.
    handle
        .abort()
        .await
        .map_err(|e| SendError::Upload(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SendTrigger;

    /// AD-13 exhaustiveness gate: `SendTrigger` is the closed set of exactly the two
    /// user-initiated dispatch triggers. This test is the **planning gate** — if a
    /// future change adds a third variant, the wildcard-free `match` below fails to
    /// COMPILE here (and the length assert fails), forcing the change to be raised as
    /// a planning-level decision rather than slipped in as an edit.
    ///
    /// Do NOT add a `_ =>` arm to the match: a wildcard would silently absorb a new
    /// variant and defeat this gate.
    #[test]
    fn exactly_two_legal_dispatch_triggers() {
        const ALL_TRIGGERS: &[SendTrigger] =
            &[SendTrigger::ComposerSend, SendTrigger::ApprovalPaneApprove];

        // Wildcard-free exhaustive match: a new `SendTrigger` variant makes this
        // non-exhaustive and the crate fails to compile HERE. NO `_ =>` arm.
        for trigger in ALL_TRIGGERS {
            match trigger {
                SendTrigger::ComposerSend => {}
                SendTrigger::ApprovalPaneApprove => {}
            }
        }

        assert_eq!(
            ALL_TRIGGERS.len(),
            2,
            "AD-13: exactly two legal dispatch triggers must exist; found {}. \
             A third trigger is an invariant breach requiring a planning decision.",
            ALL_TRIGGERS.len()
        );
    }

    /// AD-13 caller gate: `send::submit` has exactly the two user-initiated callers —
    /// `send_text` (`ComposerSend`) and `send_approval` (`ApprovalPaneApprove`) — and
    /// no third, background, scheduled, or bulk caller, and no other public API
    /// dispatches. Scans the PRODUCTION slice of `account.rs` (everything before its
    /// sole `#[cfg(test)]` marker) so this guard's own literals never self-match.
    ///
    /// A future third `send::submit(` call site (or a background/bulk dispatcher)
    /// changes the count and fails this test, surfacing the change as an invariant
    /// breach needing a planning decision.
    #[test]
    fn submit_has_exactly_the_two_user_initiated_callers() {
        let full = include_str!("account.rs");

        // The scan splits production off the test module on the sole `#[cfg(test)]`
        // marker; assert there is exactly one, so a future second `#[cfg(test)]`
        // block (which would make `.split(..).next()` silently truncate the scanned
        // slice and could hide a `send::submit` caller after it) fails loudly here
        // instead of passing on a partial scan.
        let markers = full.matches("#[cfg(test)]").count();
        assert_eq!(
            markers, 1,
            "AD-13 caller scan assumes a single `#[cfg(test)]` boundary in account.rs; \
             found {markers}. A second one would truncate the scanned production slice \
             and could hide a `send::submit` caller — the scan must be updated before \
             it can be trusted."
        );
        let source = full
            .split("#[cfg(test)]")
            .next()
            .expect("production source precedes the test module");

        // Whitespace-normalize before counting so a rustfmt-reformatted or multi-line
        // call site (`send::submit(\n    &tl, ..)`, `SendTrigger::ComposerSend)\n .await`)
        // still counts — the guard must not be defeated by mere reformatting. A
        // `crate::send::submit(` prefix still matches `send::submit(` as a substring.
        let normalized: String = source.split_whitespace().collect();

        // The gate must have exactly two call sites. The prose reference in rustdoc
        // is `[`send::submit`]` (no `(`), so it does not match.
        let submit_calls = normalized.matches("send::submit(").count();
        assert_eq!(
            submit_calls, 2,
            "AD-13: `send::submit(` must have exactly two production callers \
             (composer + approval); found {submit_calls}. A third or background \
             caller is an invariant breach requiring a planning decision."
        );

        // Call forms (with `).await`) dodge the bracketed `[`SendTrigger::…`]` rustdoc
        // reference at account.rs:2172. Each user-initiated trigger appears exactly
        // once at a real call site.
        let composer_calls = normalized
            .matches("SendTrigger::ComposerSend).await")
            .count();
        assert_eq!(
            composer_calls, 1,
            "AD-13: exactly one `ComposerSend` dispatch call site expected; found {composer_calls}."
        );
        let approval_calls = normalized
            .matches("SendTrigger::ApprovalPaneApprove).await")
            .count();
        assert_eq!(
            approval_calls, 1,
            "AD-13: exactly one `ApprovalPaneApprove` dispatch call site expected; found {approval_calls}."
        );

        // AD-13 (Story 8.3): the deferred completion path. `send::dispatch(` is the
        // SDK-enqueue primitive; besides `submit` (which lives in send.rs, not
        // account.rs) its ONLY caller is the outbox scheduler in account.rs. Exactly
        // one `send::dispatch(` call site may appear in production `account.rs` — the
        // scheduler. A second one (a new background/bulk drainer) changes this count
        // and fails the guard, surfacing it as an invariant breach needing a planning
        // decision. The rustdoc references in this file are `[`send::dispatch`]`
        // (no `(`), so they don't match.
        let dispatch_calls = normalized.matches("send::dispatch(").count();
        assert_eq!(
            dispatch_calls, 1,
            "AD-13: `send::dispatch(` must have exactly one non-`submit` production \
             caller in account.rs (the outbox scheduler); found {dispatch_calls}. A \
             second caller is an invariant breach requiring a planning decision."
        );
    }

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

        let submit_reply_start = source
            .find("pub async fn submit_reply")
            .expect("submit_reply fn must exist");
        let submit_edit_start = source
            .find("pub async fn submit_edit")
            .expect("submit_edit fn must exist");
        let toggle_reaction_start = source
            .find("pub async fn toggle_reaction")
            .expect("toggle_reaction fn must exist");
        let redact_start = source
            .find("pub async fn redact")
            .expect("redact fn must exist");
        let retry_start = source
            .find("pub async fn retry")
            .expect("retry fn must exist");
        let submit_attachment_start = source
            .find("pub async fn submit_attachment")
            .expect("submit_attachment fn must exist");
        let cancel_start = source
            .find("pub async fn cancel")
            .expect("cancel fn must exist");
        let dispatch_start = source
            .find("pub(crate) async fn dispatch")
            .expect("dispatch fn must exist");
        let call = call_sites[0];
        // The single `.send(content)` must live in `dispatch` — after `submit`,
        // before `submit_reply` (the first fn following `dispatch` in source order).
        // `submit` delegates to `dispatch`; both funnel here, preserving the single
        // SDK-enqueue call site across the Story 8.3 refactor (AD-13).
        assert!(
            call > dispatch_start && call < submit_reply_start,
            "the sole `timeline.send(` call must be inside `dispatch` (offset {call} not within {dispatch_start}..{submit_reply_start})"
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
            edit_call > submit_edit_start && edit_call < toggle_reaction_start,
            "the sole `.edit(` call must be inside `submit_edit` (offset {edit_call} not within {submit_edit_start}..{toggle_reaction_start})"
        );

        // The single reaction-dispatch call site: `.toggle_reaction(`. Doc
        // references say `Timeline::toggle_reaction` (no `.`), so they don't match.
        let reaction_sites: Vec<usize> = source
            .match_indices(".toggle_reaction(")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            reaction_sites.len(),
            1,
            "expected exactly one `.toggle_reaction(` call site (the sole reaction gate); found {}",
            reaction_sites.len()
        );
        let reaction_call = reaction_sites[0];
        assert!(
            reaction_call > toggle_reaction_start && reaction_call < redact_start,
            "the sole `.toggle_reaction(` call must be inside `toggle_reaction` (offset {reaction_call} not within {toggle_reaction_start}..{redact_start})"
        );

        // The single redaction-dispatch call site: `.redact(`. Doc references say
        // `Timeline::redact` (no `.`), so they don't match; `redact` sits between
        // `toggle_reaction` and `retry` in source order.
        let redact_sites: Vec<usize> = source.match_indices(".redact(").map(|(i, _)| i).collect();
        assert_eq!(
            redact_sites.len(),
            1,
            "expected exactly one `.redact(` call site (the sole redaction gate); found {}",
            redact_sites.len()
        );
        let redact_call = redact_sites[0];
        assert!(
            redact_call > redact_start && redact_call < retry_start,
            "the sole `.redact(` call must be inside `redact` (offset {redact_call} not within {redact_start}..{retry_start})"
        );

        // The single attachment-dispatch call site: `.send_attachment(`. Doc
        // references say `Timeline::send_attachment` (no `.`), so they don't match;
        // `submit_attachment` sits between `retry` and `cancel` in source order.
        let attachment_sites: Vec<usize> = source
            .match_indices(".send_attachment(")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            attachment_sites.len(),
            1,
            "expected exactly one `.send_attachment(` call site (the sole attachment gate); found {}",
            attachment_sites.len()
        );
        let attachment_call = attachment_sites[0];
        assert!(
            attachment_call > submit_attachment_start && attachment_call < cancel_start,
            "the sole `.send_attachment(` call must be inside `submit_attachment` (offset {attachment_call} not within {submit_attachment_start}..{cancel_start})"
        );
    }
}
