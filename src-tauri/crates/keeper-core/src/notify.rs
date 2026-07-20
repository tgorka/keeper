//! Native notifications from the sync loop (Story 10.1, AD-18).
//!
//! Taps the account-wide post-decryption message stream (the same
//! `OriginalSyncRoomMessageEvent` handler pattern [`register_archive_handler`] uses),
//! applies minimal rules (skip own messages, skip pre-session backlog, gate on
//! message type), and posts sender + Chat + preview through the existing
//! [`Platform::notify`] port to the OS.
//!
//! All notification *decision* and *formatting* logic lives here (AD-18); the SDK
//! glue in [`register_notify_handler`] is a thin extractor over the pure functions
//! + [`dispatch`], so the whole I/O matrix is unit-testable without a homeserver.
//!
//! Notification content originates **only** from the local decrypting sync loop and
//! is delivered **only** through the `Platform::notify` port → OS (NFR-11): no push
//! gateway, no network egress here. The preview is a short derived string; message
//! bodies are never logged (NFR-9). A `Platform::notify` failure is logged at `warn`
//! and swallowed — it must never block sync, panic, or abort the account.
//!
//! [`register_archive_handler`]: crate::account

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, Relation,
};
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::push::Action;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::{Client, Room};

use matrix_sdk::event_handler::{EventHandlerHandle, RawEvent};

use crate::bridge;
use crate::platform::Platform;
use crate::vm::NotifyTarget;

/// The app-wide "message previews" toggle (Story 10.1). Holds the single
/// [`AtomicBool`] the desktop shell reads/writes through the two Settings commands
/// and every account's notify handler consults when formatting a notification.
///
/// This is the *only* notification-related shared state in `keeper-core`; it lives on
/// the [`AccountManager`](crate::account::AccountManager) as an `Arc<NotifyConfig>`
/// (not a `static`), so there is no new global mutable state.
#[derive(Debug)]
pub struct NotifyConfig {
    previews_enabled: AtomicBool,
    /// The global Do-Not-Disturb switch (Story 10.2). When `true`, the notify decision
    /// silences every account/Chat while unread still accrues everywhere. Seeded from
    /// the persisted registry value; a lock-free atomic so the handler reads it per event.
    dnd_enabled: AtomicBool,
    /// The keeper-local muted-Network set (Story 10.2), keyed by the Network's display
    /// label. A message whose room's bridged Network is in this set does not notify.
    /// Seeded from the registry and replaced wholesale on each per-Network toggle;
    /// read under a short `RwLock` in the notify decision.
    muted_networks: RwLock<HashSet<String>>,
    /// The `(account_id, room_id)` of the currently-visible Chat, or `None` when no Chat
    /// is on screen (Story 14.3, AD-18). A message for exactly this Chat is suppressed —
    /// its content is already visible, so a banner would be redundant. Reported by the
    /// iOS shell from `roomsStore.selected` **only on the reduced tier** (desktop never
    /// sets it, so desktop notification behavior is unchanged). Ephemeral process state,
    /// never persisted; read under a short `RwLock` in the notify decision.
    active_room: RwLock<Option<(String, String)>>,
}

impl NotifyConfig {
    /// Construct with the given initial "message previews" state (seeded from the
    /// persisted registry value in [`AccountManager::new`](crate::account::AccountManager)).
    /// DND defaults off and the muted-Network set defaults empty; both are seeded from
    /// the registry via the fuller [`NotifyConfig::with_state`] in `AccountManager::new`.
    pub fn new(previews_enabled: bool) -> Self {
        Self::with_state(previews_enabled, false, HashSet::new())
    }

    /// Construct with the full persisted notification state (Story 10.2): previews,
    /// global DND, and the muted-Network set. Seeded once in
    /// [`AccountManager::new`](crate::account::AccountManager) from the registry.
    pub fn with_state(
        previews_enabled: bool,
        dnd_enabled: bool,
        muted_networks: HashSet<String>,
    ) -> Self {
        Self {
            previews_enabled: AtomicBool::new(previews_enabled),
            dnd_enabled: AtomicBool::new(dnd_enabled),
            muted_networks: RwLock::new(muted_networks),
            active_room: RwLock::new(None),
        }
    }

    /// Whether message previews are currently enabled.
    pub fn previews_enabled(&self) -> bool {
        self.previews_enabled.load(Ordering::Relaxed)
    }

    /// Update the in-memory "message previews" state (the caller also persists it via
    /// [`registry::set_notify_previews`](crate::registry::set_notify_previews)).
    pub fn set_previews_enabled(&self, enabled: bool) {
        self.previews_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether global Do-Not-Disturb is currently on (Story 10.2).
    pub fn dnd_enabled(&self) -> bool {
        self.dnd_enabled.load(Ordering::Relaxed)
    }

    /// Update the in-memory global DND state (the caller also persists it via
    /// [`registry::set_dnd_global`](crate::registry::set_dnd_global)).
    pub fn set_dnd_enabled(&self, enabled: bool) {
        self.dnd_enabled.store(enabled, Ordering::Relaxed);
    }

    /// Whether the given Network label is currently muted (Story 10.2). A poisoned lock
    /// fails open (treated as "not muted") rather than panicking — mute is a comfort
    /// feature and must never abort the notify path.
    pub fn is_network_muted(&self, network: &str) -> bool {
        match self.muted_networks.read() {
            Ok(set) => set.contains(network),
            Err(poisoned) => {
                tracing::warn!("muted-networks lock poisoned; failing open (not muted)");
                poisoned.into_inner().contains(network)
            }
        }
    }

    /// Add or remove a single Network label from the in-memory muted set (Story 10.2).
    /// The caller also persists it via
    /// [`registry::set_network_muted`](crate::registry::set_network_muted). A poisoned
    /// lock is recovered rather than propagated.
    pub fn set_network_muted(&self, network: &str, muted: bool) {
        let mut set = match self.muted_networks.write() {
            Ok(set) => set,
            Err(poisoned) => poisoned.into_inner(),
        };
        if muted {
            set.insert(network.to_owned());
        } else {
            set.remove(network);
        }
    }

    /// Replace the whole in-memory muted-Network set (Story 10.2), e.g. to re-seed from
    /// the registry. A poisoned lock is recovered rather than propagated.
    pub fn replace_muted_networks(&self, networks: HashSet<String>) {
        let mut set = match self.muted_networks.write() {
            Ok(set) => set,
            Err(poisoned) => poisoned.into_inner(),
        };
        *set = networks;
    }

    /// Record the currently-visible Chat (Story 14.3, AD-18). Reported by the iOS shell
    /// from `roomsStore.selected` on the reduced tier; a message for exactly this
    /// `(account_id, room_id)` is then suppressed in [`should_notify`]. A poisoned lock is
    /// recovered rather than propagated — visible-Chat suppression is a comfort feature and
    /// must never abort the notify path.
    pub fn set_active_room(&self, account_id: &str, room_id: &str) {
        let mut slot = match self.active_room.write() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        *slot = Some((account_id.to_owned(), room_id.to_owned()));
    }

    /// Clear the currently-visible Chat (Story 14.3) — no Chat is on screen, so no message
    /// is suppressed on this ground. A poisoned lock is recovered rather than propagated.
    pub fn clear_active_room(&self) {
        let mut slot = match self.active_room.write() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        *slot = None;
    }

    /// Whether `(account_id, room_id)` is the currently-visible Chat (Story 14.3). A
    /// poisoned lock fails open (treated as "not active" ⇒ still notifies) rather than
    /// panicking — over-notifying is safer than dropping a genuine notification.
    pub fn is_active_room(&self, account_id: &str, room_id: &str) -> bool {
        match self.active_room.read() {
            Ok(slot) => slot
                .as_ref()
                .is_some_and(|(a, r)| a == account_id && r == room_id),
            Err(poisoned) => {
                tracing::warn!("active-room lock poisoned; failing open (not the active room)");
                poisoned
                    .into_inner()
                    .as_ref()
                    .is_some_and(|(a, r)| a == account_id && r == room_id)
            }
        }
    }
}

/// The extracted, SDK-free context a single [`dispatch`] decision operates on. Built
/// from the message event by [`register_notify_handler`] so the rules never touch the
/// SDK types directly.
pub struct NotifyContext {
    /// The message's Matrix event id, carried into the [`NotifyTarget::Message`]
    /// click-through payload (Story 10.4). Never rendered — only the coarse landing
    /// seam and (in Epic 11) exact-message routing consume it.
    pub event_id: String,
    /// The rendered Chat name (room display name, or the room id as a fallback).
    pub chat: String,
    /// The rendered sender name (member display name, or the localpart fallback).
    pub sender: String,
    /// The message's derived preview string (body for text/notice/emote, a type
    /// descriptor for media). Only used when previews are enabled.
    pub preview: String,
    /// `true` iff the message was sent by this account's own user (drop → no self-notify).
    pub is_self: bool,
    /// The message's `origin_server_ts` in milliseconds.
    pub event_ts_ms: u64,
    /// Whether this message type notifies at all (text/notice/emote/media yes;
    /// verification-request / server-notice / unknown no).
    pub notifies: bool,
    /// Whether the room's synced Matrix push rules elected to notify *this* event
    /// (Story 10.2): the room's `event_push_actions` contained an `Action::Notify`.
    /// This folds per-Chat mute and mention-only (and mention/reply detection) into the
    /// standard ruleset. Fail-open `true` when the push-rule lookup errors — never drop
    /// a genuine notification because a rule read failed.
    pub room_push_notifies: bool,
    /// Whether the room's bridged Network is in the keeper-local muted set (Story 10.2).
    /// Fail-open `false` (not muted) when the Network cannot be resolved.
    pub network_muted: bool,
}

/// Whether a message should raise a notification (pure rule).
///
/// The golden decision (Story 10.1 + 10.2). A notification is raised only when **all**
/// of these hold:
/// - not our own echo (`!is_self`),
/// - a notifying message type (`notifies`),
/// - not pre-session backlog (`event_ts_ms >= baseline_ms`),
/// - the room's synced push rules elected to notify this event (`room_push_notifies`) —
///   this ANDs in per-Chat mute and mention-only,
/// - global Do-Not-Disturb is off (`!dnd_enabled`),
/// - the room's Network is not muted (`!network_muted`),
/// - the message's Chat is not the one currently on screen (`!is_active_room`).
///
/// The first three gates are the unchanged Story 10.1 rules; the next three are the
/// Story 10.2 suppression layer; the last is the Story 14.3 visible-Chat suppression
/// (AD-18) — a banner for the Chat already on screen is redundant. Backlog suppression
/// drops cold-launch history (the inbox already shows it) while still notifying messages
/// that arrive during a live background session. `room_push_notifies` / `network_muted` /
/// `is_active_room` are computed fail-open by the caller, so a transient read error
/// over-notifies rather than dropping a genuine notification.
#[allow(clippy::too_many_arguments)]
pub fn should_notify(
    is_self: bool,
    event_ts_ms: u64,
    baseline_ms: u64,
    notifies: bool,
    room_push_notifies: bool,
    dnd_enabled: bool,
    network_muted: bool,
    is_active_room: bool,
) -> bool {
    !is_self
        && notifies
        && event_ts_ms >= baseline_ms
        && room_push_notifies
        && !dnd_enabled
        && !network_muted
        && !is_active_room
}

/// Derive the preview string for a message type (pure rule).
///
/// Returns `(preview, notifies)`:
/// - text / notice / emote → the body (trimmed); notifies.
/// - image / video / audio / file / location → a type descriptor (never a
///   filename / URL / body leak); notifies.
/// - any other type (verification-request, server-notice, unknown) → empty preview,
///   does **not** notify.
///
/// Only the descriptor / body crosses out of this function — never media bytes,
/// `MediaSource`, `mxc`, or a filename (NFR-9).
pub fn preview_for(msgtype: &MessageType) -> (String, bool) {
    match msgtype {
        MessageType::Text(c) => text_preview(&c.body),
        MessageType::Notice(c) => text_preview(&c.body),
        MessageType::Emote(c) => text_preview(&c.body),
        MessageType::Image(_) => ("Photo".to_owned(), true),
        MessageType::Video(_) => ("Video".to_owned(), true),
        MessageType::Audio(_) => ("Audio message".to_owned(), true),
        MessageType::File(_) => ("File".to_owned(), true),
        MessageType::Location(_) => ("Location".to_owned(), true),
        // Verification requests, server notices, and any future / unknown message
        // type do not notify (Story 10.1 scope).
        _ => (String::new(), false),
    }
}

/// Preview for a text/notice/emote body: the trimmed body when it has content,
/// else `(empty, false)` — a whitespace-only / empty message carries nothing worth
/// notifying about and would otherwise render a contentless `"{sender}: "` body.
fn text_preview(body: &str) -> (String, bool) {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        (String::new(), false)
    } else {
        (trimmed.to_owned(), true)
    }
}

/// Build the `(title, body)` a notification carries (pure rule).
///
/// - Title collapses to just the sender when `sender == chat` (a DM), else the Chat
///   name is the title.
/// - Body is `"{sender}: {preview}"` when previews are enabled, else just the sender
///   name — so the sender **and** Chat still appear (title carries the Chat), but no
///   message content leaks. Showing the sender when previews are off is required by the
///   Settings copy and the acceptance criteria ("show sender/Chat but no content"); a
///   fixed "New message" body would drop the sender in a group Chat.
pub fn format_notification(
    chat: &str,
    sender: &str,
    preview: &str,
    previews_enabled: bool,
) -> (String, String) {
    // DM collapse: when the Chat name equals the sender, the title is just the sender.
    let title = if sender == chat {
        sender.to_owned()
    } else {
        chat.to_owned()
    };
    let body = if previews_enabled {
        format!("{sender}: {preview}")
    } else {
        sender.to_owned()
    };
    (title, body)
}

/// Run the full rule → format → post pipeline for one message against a [`Platform`]
/// (testable seam).
///
/// Applies [`should_notify`] (self / backlog / type gates); when it passes, builds the
/// `(title, body)` with [`format_notification`] and posts it through
/// [`Platform::notify`]. A notifier failure is logged at `warn` and swallowed — it must
/// never block sync, panic, or propagate (matches the archive handler's error posture).
/// The message body / preview is never logged (NFR-9); `account_id`/`room_id` are safe.
pub fn dispatch(
    platform: &dyn Platform,
    config: &NotifyConfig,
    account_id: &str,
    room_id: &str,
    baseline_ms: u64,
    ctx: &NotifyContext,
) {
    // Read global DND and the currently-visible Chat from the shared config at decision
    // time; the per-Chat push verdict and per-Network mute are already resolved into the
    // context by the handler. Visible-Chat suppression (Story 14.3, AD-18) drops a banner
    // for the Chat already on screen — the signal is set only on the reduced tier, so
    // desktop (where `active_room` is always `None`) behaves exactly as before.
    if !should_notify(
        ctx.is_self,
        ctx.event_ts_ms,
        baseline_ms,
        ctx.notifies,
        ctx.room_push_notifies,
        config.dnd_enabled(),
        ctx.network_muted,
        config.is_active_room(account_id, room_id),
    ) {
        return;
    }
    let (title, body) = format_notification(
        &ctx.chat,
        &ctx.sender,
        &ctx.preview,
        config.previews_enabled(),
    );
    // Attach the typed click-through target: the exact `(account_id, room_id, event_id)`
    // this notification was raised for (Story 10.4). Under Option B the shell records it
    // as the "last notification target" and lands coarsely on the Inbox — never exact.
    let target = NotifyTarget::Message {
        account_id: account_id.to_owned(),
        room_id: room_id.to_owned(),
        event_id: ctx.event_id.clone(),
    };
    if let Err(e) = platform.notify(&title, &body, &target) {
        // Best-effort: a notifier failure never blocks sync. Log ids only — never the
        // title/body (they carry message content).
        tracing::warn!(
            account_id = %account_id,
            room_id = %room_id,
            error = %e,
            "notify: could not post native notification; swallowing"
        );
    }
}

/// Post the FR-28 bridge-disconnected native notification (Story 10.4).
///
/// The single entry point the bridge-health machine calls on a per-session transition
/// **into** `Disconnected`. The body copy is exactly
/// `"{network_name} disconnected — re-link to keep receiving messages."` (Network-named),
/// and the click-through target is [`NotifyTarget::Bridge`] carrying `(account_id,
/// network_id)` — coarse landing routes to the Bridges view.
///
/// Suppression policy differs from message notifications: this respects **only** global
/// Do-Not-Disturb (`NotifyConfig::dnd_enabled`); per-Chat and per-Network mute do NOT
/// apply (bridge integrity is not chat noise). The persistent Story 6.5 in-app surfaces
/// stand regardless of whether this native toast fires. A notifier failure (or an unset
/// port on a headless build) is logged at `warn` and swallowed — it must never block the
/// health machine or panic.
pub fn notify_bridge_disconnected(
    platform: &dyn Platform,
    config: &NotifyConfig,
    account_id: &str,
    network_id: &str,
    network_name: &str,
) {
    // Global DND silences the bridge alert too; per-Network / per-Chat mute never does.
    if config.dnd_enabled() {
        return;
    }
    let title = "Bridge disconnected".to_owned();
    let body = format!("{network_name} disconnected — re-link to keep receiving messages.");
    let target = NotifyTarget::Bridge {
        account_id: account_id.to_owned(),
        network_id: network_id.to_owned(),
    };
    if let Err(e) = platform.notify(&title, &body, &target) {
        // Best-effort: a notifier failure never blocks the health machine. Log ids only.
        tracing::warn!(
            account_id = %account_id,
            network_id = %network_id,
            error = %e,
            "notify: could not post bridge-disconnected notification; swallowing"
        );
    }
}

/// Post the Story 18.4 recording-fault native notification — one leg of the
/// loud-failure triad (tray error badge + this notification + in-app banner).
///
/// The single entry point the shell's recording driver calls on a fault **onset**
/// (the snapshot's `error` transitioning `None → Some`); the caller owns the
/// exactly-once dedup. The copy names the honest reason and points at the app,
/// where the banner's one-click **Restart recording** lives (the desktop
/// `tauri-plugin-notification` backend has no per-notification action buttons —
/// Epic 11).
///
/// Suppression policy: **none**. Unlike message notifications (full
/// [`should_notify`] gating) and even [`notify_bridge_disconnected`] (which still
/// honors global DND), a recording fault bypasses global Do-Not-Disturb *and*
/// per-Network mute entirely — it is a local loss-risk event and a mandated triad
/// leg, not chat noise; the tray + banner legs fire regardless, so silencing this
/// leg would only desynchronize the triad. By construction this function consults
/// no [`NotifyConfig`] at all, so no future gate can creep in silently. The
/// message-notification pipeline ([`dispatch`]/[`should_notify`]) is untouched.
///
/// The click-through target is [`NotifyTarget::None`] — coarse activation
/// summons/focuses the window, where the Recording view's banner carries the
/// Restart action. A notifier failure (or an unset port on a headless build) is
/// logged at `warn` and swallowed — it must never block the recording driver.
pub fn notify_recording_fault(platform: &dyn Platform, reason: &str) {
    let title = "Recording failed".to_owned();
    let body = format!("{reason} — open keeper to restart the recording.");
    if let Err(e) = platform.notify(&title, &body, &NotifyTarget::None) {
        // Best-effort: a notifier failure never blocks the recording driver.
        tracing::warn!(
            error = %e,
            "notify: could not post recording-fault notification; swallowing"
        );
    }
}

/// Post the Story 18.4 recording-**warning** native notification (closing Story
/// 19.4's deferred native-notification leg for non-fatal sticky warnings, e.g. a
/// microphone hot-unplug).
///
/// Called by the shell's recording driver on a warning **onset** only (the
/// snapshot's `warning` transitioning `None → Some`); a sticky warning that
/// merely repeats/updates never re-fires (the caller owns the dedup). The copy
/// says the recording continues — a warning is not a fault. Same suppression
/// policy as [`notify_recording_fault`]: bypasses DND and per-Network mute by
/// consulting no [`NotifyConfig`]; same [`NotifyTarget::None`] coarse target;
/// same swallowed-failure posture.
pub fn notify_recording_warning(platform: &dyn Platform, message: &str) {
    let title = "Recording warning".to_owned();
    let body = format!("{message} — the recording is still running.");
    if let Err(e) = platform.notify(&title, &body, &NotifyTarget::None) {
        // Best-effort: a notifier failure never blocks the recording driver.
        tracing::warn!(
            error = %e,
            "notify: could not post recording-warning notification; swallowing"
        );
    }
}

/// Post the Story 18.5 recording-**stopped** native notification for the live
/// disk guard's graceful hard-floor stop (`free < RECORDING_MIN_FREE_BYTES`).
///
/// A distinct entry from [`notify_recording_warning`] on purpose: the warning
/// copy asserts "the recording is still running", which would flatly contradict
/// a message announcing that the recording just stopped. This one carries the
/// stop reason verbatim (the core-owned copy, e.g. "Recording stopped — low
/// disk") with no such suffix. Same suppression policy as the fault/warning
/// legs: bypasses DND and per-Network mute by consulting no [`NotifyConfig`],
/// same [`NotifyTarget::None`] coarse target, same swallowed-failure posture.
pub fn notify_recording_stopped(platform: &dyn Platform, reason: &str) {
    let title = "Recording stopped".to_owned();
    let body = reason.to_owned();
    if let Err(e) = platform.notify(&title, &body, &NotifyTarget::None) {
        // Best-effort: a notifier failure never blocks the recording driver.
        tracing::warn!(
            error = %e,
            "notify: could not post recording-stopped notification; swallowing"
        );
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch (UTC), used as the
/// backlog baseline captured at handler registration. Saturates rather than panicking.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => u64::try_from(d.as_millis()).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

/// Register the account-wide post-decryption notify event handler on `client` and
/// return its [`EventHandlerHandle`] (Story 10.1).
///
/// Captures the backlog `baseline_ms` (client clock) **once, at registration**, so
/// cold-launch history (`origin_server_ts < baseline_ms`) is suppressed while messages
/// arriving during a live background session still notify. The handler fires for every
/// `m.room.message` the SDK delivers post-decryption (encrypted rooms included). For
/// each, it best-effort extracts `(sender, chat, ts, msgtype)` — member display name →
/// localpart fallback for the sender, `room.display_name()` → room id fallback for the
/// Chat, own-id via `client.user_id()` for the self check — then hands a pure
/// [`NotifyContext`] to [`dispatch`]. Extraction and decision never block sync and never
/// log the body (NFR-9).
///
/// Story 10.2 threads three suppression gates through the same pipeline: the room's
/// synced push-rule verdict for this event (`room.event_push_actions` over the raw JSON
/// supplied by the [`RawEvent`] context arg → contains [`Action::Notify`]), the global
/// DND switch (read from `config` in [`dispatch`]), and the per-Network mute set
/// (resolved via [`bridge::room_bridge_network`] against `config`). Every new read is
/// fail-open: a push-rule/network error is logged at `warn` and treated as
/// "would notify" / "not muted", so a transient failure over-notifies rather than
/// silently dropping a genuine notification.
pub fn register_notify_handler(
    client: &Client,
    account_id: &str,
    platform: Arc<dyn Platform>,
    config: Arc<NotifyConfig>,
) -> EventHandlerHandle {
    let account_id = account_id.to_owned();
    // Capture the backlog baseline once, at registration (handler is registered once
    // per account lifetime, not per reconnect), so cold-launch history is suppressed.
    let baseline_ms = now_ms();
    // Resolve our own user id once so the self-echo check needs no per-event lookup.
    let own_user_id = client.user_id().map(|u| u.as_str().to_owned());
    client.add_event_handler(
        move |ev: OriginalSyncRoomMessageEvent, room: Room, raw: RawEvent| {
            let account_id = account_id.clone();
            let platform = platform.clone();
            let config = config.clone();
            let own_user_id = own_user_id.clone();
            async move {
                let room_id = room.room_id().to_owned();
                let sender = ev.sender.clone();
                let is_self = own_user_id
                    .as_deref()
                    .is_some_and(|own| own == sender.as_str());

                // An edit (`m.replace`) is delivered as a fresh `m.room.message`; it is not a
                // new incoming message, so it must not notify (the previewed body would be the
                // `* edited text` fallback). Mirrors the archive handler's Replacement guard.
                if matches!(ev.content.relates_to, Some(Relation::Replacement(_))) {
                    return;
                }

                let (preview, notifies) = preview_for(&ev.content.msgtype);
                let event_ts_ms = u64::from(ev.origin_server_ts.get());

                // Cheap, I/O-free gates first: drop our own echo, pre-session backlog,
                // non-notifying types, and global DND before touching the push ruleset,
                // the Network resolution, or any display-name lookup. This keeps the
                // common suppressed-message path (self-echoes, backlog) as light as the
                // 10.1 handler — the per-event push-rule evaluation and bridge state read
                // below run only for messages that survive these gates.
                if is_self || !notifies || event_ts_ms < baseline_ms || config.dnd_enabled() {
                    return;
                }

                // Resolve the room's synced push-rule verdict for THIS event from the raw
                // JSON the `RawEvent` context arg supplies (Story 10.2). Fail-open `true`:
                // never drop a genuine notification because a rule lookup failed.
                let room_push_notifies = room_push_notifies(&room, &raw).await;
                // Resolve the room's bridged Network and check the keeper-local muted set.
                // Fail-open `false` (not muted) when no Network resolves.
                let network_muted = match bridge::room_bridge_network(&room).await {
                    Some(net) => config.is_network_muted(&net),
                    None => false,
                };

                // Per-Chat mute / mention-only (synced push rules) and per-Network mute.
                if !room_push_notifies || network_muted {
                    return;
                }

                // Sender display name → localpart fallback (no network round-trip). An empty
                // display name falls back too, so the sender is never blank.
                let sender_name = room
                    .get_member_no_sync(&sender)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|m| m.display_name().map(str::to_owned))
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| localpart_fallback(sender.as_str()));

                // Chat display name → room id fallback (an empty name falls back too).
                let chat = match room.display_name().await {
                    Ok(name) if !name.to_string().trim().is_empty() => name.to_string(),
                    _ => room_id.as_str().to_owned(),
                };

                let ctx = NotifyContext {
                    event_id: ev.event_id.to_string(),
                    chat,
                    sender: sender_name,
                    preview,
                    is_self,
                    event_ts_ms,
                    notifies,
                    room_push_notifies,
                    network_muted,
                };
                dispatch(
                    platform.as_ref(),
                    &config,
                    &account_id,
                    room_id.as_str(),
                    baseline_ms,
                    &ctx,
                );
            }
        },
    )
}

/// Resolve the room's synced push-rule verdict for one raw event (Story 10.2).
///
/// Runs the room's server-synced push ruleset over the event's raw JSON via
/// [`matrix_sdk::Room::event_push_actions`] and returns whether the resulting actions
/// contain [`Action::Notify`] — i.e. whether per-Chat mute / mention-only elected to
/// notify this event. Fail-open `true` on any error (a raw-JSON reparse failure or a
/// push-actions error): a comfort feature must never silently drop a genuine
/// notification. The event id / body is never logged (NFR-9).
async fn room_push_notifies(room: &Room, raw: &RawEvent) -> bool {
    // Rebuild a typed `Raw<AnySyncTimelineEvent>` from the handler-supplied raw JSON;
    // `event_push_actions` runs the room's synced ruleset over it.
    let raw_event: Raw<AnySyncTimelineEvent> = Raw::from_json(raw.0.clone());
    match room.event_push_actions(&raw_event).await {
        Ok(actions) => actions
            .unwrap_or_default()
            .iter()
            .any(|a| matches!(a, Action::Notify)),
        Err(e) => {
            tracing::warn!(
                room_id = %room.room_id(),
                error = %e,
                "notify: push-actions lookup failed; failing open (would notify)"
            );
            true
        }
    }
}

/// The localpart of a Matrix user id (`@alice:example.org` → `alice`), used as the
/// sender-name fallback when no member display name is known. A malformed id (no `@` /
/// no `:`) falls back to the whole string.
fn localpart_fallback(user_id: &str) -> String {
    user_id
        .strip_prefix('@')
        .and_then(|rest| rest.split_once(':').map(|(local, _)| local))
        .unwrap_or(user_id)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use matrix_sdk::ruma::events::room::message::{
        EmoteMessageEventContent, FileMessageEventContent, ImageMessageEventContent,
        KeyVerificationRequestEventContent, LocationMessageEventContent, NoticeMessageEventContent,
        TextMessageEventContent,
    };
    use matrix_sdk::ruma::events::room::{ImageInfo, MediaSource};
    use matrix_sdk::ruma::owned_mxc_uri;

    use crate::error::CoreError;

    /// A capturing [`Platform`] double recording every `(title, body, target)` posted
    /// through `notify`, so the dispatch matrix is covered without a homeserver. `notify`
    /// can be made to fail to exercise the swallow path.
    struct CapturingPlatform {
        calls: Mutex<Vec<(String, String, NotifyTarget)>>,
        fail: bool,
    }

    impl CapturingPlatform {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail: false,
            }
        }
        fn failing() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                fail: true,
            }
        }
        /// The `(title, body)` of every posted notification (the target is asserted
        /// separately via [`CapturingPlatform::targets`]).
        fn calls(&self) -> Vec<(String, String)> {
            self.calls
                .lock()
                .expect("lock calls")
                .iter()
                .map(|(title, body, _)| (title.clone(), body.clone()))
                .collect()
        }
        /// The click-through [`NotifyTarget`] of every posted notification.
        fn targets(&self) -> Vec<NotifyTarget> {
            self.calls
                .lock()
                .expect("lock calls")
                .iter()
                .map(|(_, _, target)| target.clone())
                .collect()
        }
    }

    impl Platform for CapturingPlatform {
        fn data_dir(&self) -> Result<PathBuf, CoreError> {
            Ok(PathBuf::from("/tmp/keeper-notify-test"))
        }
        fn keychain_set(&self, _key: &str, _value: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn keychain_get(&self, _key: &str) -> Result<Option<String>, CoreError> {
            Ok(None)
        }
        fn keychain_delete(&self, _key: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn open_url(&self, _url: &str) -> Result<(), CoreError> {
            Ok(())
        }
        fn notify(&self, title: &str, body: &str, target: &NotifyTarget) -> Result<(), CoreError> {
            if self.fail {
                return Err(CoreError::Unsupported("notify failed in test".to_owned()));
            }
            self.calls.lock().expect("lock calls").push((
                title.to_owned(),
                body.to_owned(),
                target.clone(),
            ));
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
        fn exclude_from_backup(&self, _path: &std::path::Path) -> Result<(), CoreError> {
            Ok(())
        }
        fn set_badge_count(&self, _count: Option<u32>) -> Result<(), CoreError> {
            Ok(())
        }
    }

    fn text(body: &str) -> MessageType {
        MessageType::Text(TextMessageEventContent::plain(body))
    }

    /// A default-notifying context (Story 10.1 shape): the new 10.2 gates default to
    /// "would notify" (`room_push_notifies = true`, `network_muted = false`), so the
    /// existing dispatch tests exercise exactly the 10.1 behavior. The mute-specific
    /// tests build their contexts with [`ctx_gated`].
    fn ctx(
        chat: &str,
        sender: &str,
        preview: &str,
        is_self: bool,
        ts: u64,
        notifies: bool,
    ) -> NotifyContext {
        NotifyContext {
            event_id: "$ev:example.org".to_owned(),
            chat: chat.to_owned(),
            sender: sender.to_owned(),
            preview: preview.to_owned(),
            is_self,
            event_ts_ms: ts,
            notifies,
            room_push_notifies: true,
            network_muted: false,
        }
    }

    /// A context with explicit 10.2 suppression gates, for the mute / mention-only /
    /// network-muted dispatch matrix.
    fn ctx_gated(ts: u64, room_push_notifies: bool, network_muted: bool) -> NotifyContext {
        NotifyContext {
            event_id: "$ev:example.org".to_owned(),
            chat: "Weekend plans".to_owned(),
            sender: "Alice".to_owned(),
            preview: "who's in?".to_owned(),
            is_self: false,
            event_ts_ms: ts,
            notifies: true,
            room_push_notifies,
            network_muted,
        }
    }

    // ── should_notify ──────────────────────────────────────────────────────────
    // The last four args are the suppression gates: room_push_notifies (10.2),
    // dnd_enabled (10.2), network_muted (10.2), is_active_room (14.3). The default
    // "would notify" tuple is (…, true, false, false, false).
    #[test]
    fn should_notify_true_for_live_other_message() {
        assert!(should_notify(
            false, 100, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_false_for_own_echo() {
        assert!(!should_notify(
            true, 100, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_false_for_backlog() {
        // origin_ts strictly before the baseline is suppressed backlog.
        assert!(!should_notify(
            false, 40, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_true_at_exact_baseline() {
        assert!(should_notify(
            false, 50, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_false_for_non_notifying_type() {
        assert!(!should_notify(
            false, 100, 50, false, true, false, false, false
        ));
    }

    // ── should_notify: Story 10.2 suppression gates ─────────────────────────────
    #[test]
    fn should_notify_false_when_room_push_rules_suppress() {
        // Chat muted / mention-only non-mention: the synced ruleset did NOT elect to
        // notify this event → no notification, even though every 10.1 gate passes.
        assert!(!should_notify(
            false, 100, 50, true, false, false, false, false
        ));
    }

    #[test]
    fn should_notify_true_when_room_push_rules_notify() {
        // Mention-only with a mention/reply (ruleset yielded Action::Notify) → notifies.
        assert!(should_notify(
            false, 100, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_false_when_dnd_enabled() {
        // Global DND silences every account/Chat regardless of the other gates.
        assert!(!should_notify(
            false, 100, 50, true, true, true, false, false
        ));
    }

    #[test]
    fn should_notify_false_when_network_muted() {
        // The room's Network is in the muted set → no notification.
        assert!(!should_notify(
            false, 100, 50, true, true, false, true, false
        ));
    }

    // ── should_notify: Story 14.3 visible-Chat suppression ──────────────────────
    #[test]
    fn should_notify_false_when_active_room() {
        // The message is for the Chat currently on screen → suppressed, even though every
        // other gate passes (its content is already visible).
        assert!(!should_notify(
            false, 100, 50, true, true, false, false, true
        ));
    }

    #[test]
    fn should_notify_true_when_not_active_room() {
        // The visible-Chat gate off (a different Chat is open, or none) → notifies.
        assert!(should_notify(
            false, 100, 50, true, true, false, false, false
        ));
    }

    #[test]
    fn should_notify_requires_all_gates_together() {
        // Every gate must hold; flipping any one to its suppressing value drops it.
        assert!(should_notify(
            false, 100, 50, true, true, false, false, false
        ));
        assert!(!should_notify(
            true, 100, 50, true, true, false, false, false
        )); // self
        assert!(!should_notify(
            false, 100, 50, false, true, false, false, false
        )); // type
        assert!(!should_notify(
            false, 40, 50, true, true, false, false, false
        )); // backlog
        assert!(!should_notify(
            false, 100, 50, true, false, false, false, false
        )); // push-rule
        assert!(!should_notify(
            false, 100, 50, true, true, true, false, false
        )); // dnd
        assert!(!should_notify(
            false, 100, 50, true, true, false, true, false
        )); // network
        assert!(!should_notify(
            false, 100, 50, true, true, false, false, true
        )); // active room
    }

    // ── NotifyConfig: Story 14.3 active-room set/clear/round-trip ────────────────
    #[test]
    fn notify_config_active_room_round_trips() {
        let config = NotifyConfig::new(true);
        // Default: no active room, so nothing is ever the active room.
        assert!(!config.is_active_room("acct", "!room:example.org"));

        // Set: exactly that (account, room) is active; a different account or room is not.
        config.set_active_room("acct", "!room:example.org");
        assert!(config.is_active_room("acct", "!room:example.org"));
        assert!(!config.is_active_room("acct", "!other:example.org"));
        assert!(!config.is_active_room("other", "!room:example.org"));

        // Replacing the active room moves the suppression to the new Chat.
        config.set_active_room("acct", "!other:example.org");
        assert!(!config.is_active_room("acct", "!room:example.org"));
        assert!(config.is_active_room("acct", "!other:example.org"));

        // Clear: no Chat is active again.
        config.clear_active_room();
        assert!(!config.is_active_room("acct", "!other:example.org"));
    }

    // ── preview_for ────────────────────────────────────────────────────────────
    #[test]
    fn preview_for_text_notice_emote_yield_body() {
        assert_eq!(
            preview_for(&text("hey there")),
            ("hey there".to_owned(), true)
        );
        assert_eq!(
            preview_for(&MessageType::Notice(NoticeMessageEventContent::plain(
                "a notice"
            ))),
            ("a notice".to_owned(), true)
        );
        assert_eq!(
            preview_for(&MessageType::Emote(EmoteMessageEventContent::plain(
                "waves"
            ))),
            ("waves".to_owned(), true)
        );
    }

    #[test]
    fn preview_for_text_trims_whitespace() {
        assert_eq!(
            preview_for(&text("  padded  ")),
            ("padded".to_owned(), true)
        );
    }

    #[test]
    fn preview_for_empty_or_whitespace_body_does_not_notify() {
        // An empty / whitespace-only message carries nothing to notify about; suppressing
        // it also avoids a contentless "{sender}: " body.
        assert_eq!(preview_for(&text("")), (String::new(), false));
        assert_eq!(preview_for(&text("   ")), (String::new(), false));
        assert_eq!(
            preview_for(&MessageType::Notice(NoticeMessageEventContent::plain(" "))),
            (String::new(), false)
        );
    }

    #[test]
    fn preview_for_media_yields_type_descriptor_never_filename() {
        let src = MediaSource::Plain(owned_mxc_uri!("mxc://example.org/abc"));
        let image = MessageType::Image(ImageMessageEventContent::new(
            "secret-filename.png".to_owned(),
            src.clone(),
        ));
        let (preview, notifies) = preview_for(&image);
        assert_eq!(preview, "Photo");
        assert!(notifies);
        // The filename never leaks into the preview.
        assert!(!preview.contains("secret-filename"));

        let file = MessageType::File(FileMessageEventContent::new("dossier.pdf".to_owned(), src));
        assert_eq!(preview_for(&file), ("File".to_owned(), true));

        let location = MessageType::Location(LocationMessageEventContent::new(
            "here".to_owned(),
            "geo:0,0".to_owned(),
        ));
        assert_eq!(preview_for(&location), ("Location".to_owned(), true));
    }

    #[test]
    fn preview_for_verification_request_does_not_notify() {
        let content = KeyVerificationRequestEventContent::new(
            "verify me".to_owned(),
            vec![],
            matrix_sdk::ruma::device_id!("DEV").to_owned(),
            matrix_sdk::ruma::user_id!("@bob:example.org").to_owned(),
        );
        let (preview, notifies) = preview_for(&MessageType::VerificationRequest(content));
        assert_eq!(preview, "");
        assert!(!notifies);
    }

    // ── format_notification ────────────────────────────────────────────────────
    #[test]
    fn format_collapses_title_for_dm_with_previews_on() {
        // Golden: sender == chat (DM) collapses the title; body carries "sender: preview".
        assert_eq!(
            format_notification("Alice", "Alice", "hey there", true),
            ("Alice".to_owned(), "Alice: hey there".to_owned())
        );
    }

    #[test]
    fn format_uses_chat_title_for_group_with_previews_on() {
        assert_eq!(
            format_notification("Weekend plans", "Alice", "who's in?", true),
            ("Weekend plans".to_owned(), "Alice: who's in?".to_owned())
        );
    }

    #[test]
    fn format_hides_content_when_previews_off() {
        // Previews off: sender + Chat both still present (Chat in the title, sender in the
        // body), but NO message content. A group Chat must not drop the sender.
        assert_eq!(
            format_notification("Weekend plans", "Alice", "who's in?", false),
            ("Weekend plans".to_owned(), "Alice".to_owned())
        );
        // DM: title collapses to the sender; body repeats the sender — still no content.
        assert_eq!(
            format_notification("Alice", "Alice", "hey there", false),
            ("Alice".to_owned(), "Alice".to_owned())
        );
    }

    // ── dispatch (capturing Platform double) ────────────────────────────────────
    #[test]
    fn dispatch_posts_one_notification_for_live_other_message() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Weekend plans", "Alice", "who's in?", false, 100, true),
        );
        assert_eq!(
            platform.calls(),
            vec![("Weekend plans".to_owned(), "Alice: who's in?".to_owned())]
        );
    }

    #[test]
    fn dispatch_previews_off_hides_content() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(false);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Weekend plans", "Alice", "who's in?", false, 100, true),
        );
        assert_eq!(
            platform.calls(),
            vec![("Weekend plans".to_owned(), "Alice".to_owned())]
        );
    }

    #[test]
    fn dispatch_skips_own_echo() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Alice", "Alice", "hi me", true, 100, true),
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn dispatch_skips_backlog() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            // origin_ts (40) < baseline (50) → suppressed backlog.
            &ctx("Alice", "Alice", "old news", false, 40, true),
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn dispatch_skips_non_notifying_type() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Alice", "Alice", "", false, 100, false),
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn dispatch_swallows_notifier_failure() {
        let platform = CapturingPlatform::failing();
        let config = NotifyConfig::new(true);
        // Must not panic; the error is swallowed (logged at warn).
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Weekend plans", "Alice", "who's in?", false, 100, true),
        );
        // No successful call was recorded (the fake returned Err before pushing).
        assert!(platform.calls().is_empty());
    }

    // ── dispatch: Story 10.2 suppression gates (capturing Platform double) ───────
    #[test]
    fn dispatch_skips_when_room_push_rules_suppress() {
        // Chat muted / mention-only non-mention: the room's synced ruleset elected NOT
        // to notify (`room_push_notifies = false`) → no notification posts.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, false, false),
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn dispatch_notifies_when_mention_reply_yields_notify_action() {
        // Mention-only with a mention/reply: the ruleset yielded Action::Notify
        // (`room_push_notifies = true`) → one notification.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert_eq!(
            platform.calls(),
            vec![("Weekend plans".to_owned(), "Alice: who's in?".to_owned())]
        );
    }

    #[test]
    fn dispatch_skips_when_network_muted() {
        // The room's Network is in the muted set → no notification (unread still accrues
        // elsewhere; that is the inbox path, not exercised here).
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, true),
        );
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn dispatch_suppresses_active_room_and_resumes_after_clear() {
        // Story 14.3: a message for the currently-visible Chat is suppressed; once the Chat
        // is cleared (closed), the same context notifies again.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        config.set_active_room("acct", "!room:example.org");
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert!(platform.calls().is_empty());

        // A message for a DIFFERENT room still notifies while the first Chat is open.
        dispatch(
            &platform,
            &config,
            "acct",
            "!other:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert_eq!(platform.calls().len(), 1);

        // Closing the Chat clears the suppression → the original room notifies again.
        config.clear_active_room();
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert_eq!(platform.calls().len(), 2);
    }

    #[test]
    fn dispatch_skips_every_chat_when_dnd_enabled() {
        // Global DND (read from config in dispatch) silences a would-notify message.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        config.set_dnd_enabled(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert!(platform.calls().is_empty());
        // Turning DND back off restores notifications for the same context.
        config.set_dnd_enabled(false);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert_eq!(platform.calls().len(), 1);
    }

    #[test]
    fn dispatch_fail_open_over_notifies_on_push_rule_error() {
        // The handler resolves `room_push_notifies` fail-open to `true` on a rule-read
        // error; `dispatch` then behaves exactly like the 10.1 path — one notification.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            // room_push_notifies = true models the fail-open verdict.
            &ctx_gated(100, true, false),
        );
        assert_eq!(platform.calls().len(), 1);
    }

    // ── Story 10.4: message click-through target attach at dispatch ─────────────
    #[test]
    fn dispatch_attaches_message_target_with_exact_ids() {
        // The posted notification carries the exact (account_id, room_id, event_id) as a
        // NotifyTarget::Message — the click-through payload ships even though MVP landing
        // is coarse.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        let mut context = ctx("Weekend plans", "Alice", "who's in?", false, 100, true);
        context.event_id = "$evt-42:example.org".to_owned();
        dispatch(
            &platform,
            &config,
            "acct-7",
            "!room:example.org",
            50,
            &context,
        );
        assert_eq!(
            platform.targets(),
            vec![NotifyTarget::Message {
                account_id: "acct-7".to_owned(),
                room_id: "!room:example.org".to_owned(),
                event_id: "$evt-42:example.org".to_owned(),
            }]
        );
    }

    #[test]
    fn dispatch_suppressed_message_attaches_no_target() {
        // A suppressed message (own echo) never posts, so no target is recorded either.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx("Alice", "Alice", "hi me", true, 100, true),
        );
        assert!(platform.targets().is_empty());
    }

    // ── Story 10.4: bridge-disconnected notify entry point ──────────────────────
    #[test]
    fn bridge_notify_posts_exact_copy_and_bridge_target() {
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        notify_bridge_disconnected(&platform, &config, "acct-1", "signal", "Signal");
        assert_eq!(
            platform.calls(),
            vec![(
                "Bridge disconnected".to_owned(),
                "Signal disconnected — re-link to keep receiving messages.".to_owned(),
            )]
        );
        assert_eq!(
            platform.targets(),
            vec![NotifyTarget::Bridge {
                account_id: "acct-1".to_owned(),
                network_id: "signal".to_owned(),
            }]
        );
    }

    #[test]
    fn bridge_notify_suppressed_by_global_dnd() {
        // Global DND silences the bridge alert (the in-app 6.5 surfaces still update —
        // not exercised here). Per-Network mute would NOT suppress it (asserted below).
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        config.set_dnd_enabled(true);
        notify_bridge_disconnected(&platform, &config, "acct-1", "signal", "Signal");
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn bridge_notify_ignores_per_network_mute() {
        // Per-Network mute is chat noise, not bridge integrity: a muted Network must
        // STILL raise the bridge-disconnected alert.
        let platform = CapturingPlatform::new();
        let mut seed = HashSet::new();
        seed.insert("Signal".to_owned());
        let config = NotifyConfig::with_state(true, false, seed);
        assert!(config.is_network_muted("Signal"));
        notify_bridge_disconnected(&platform, &config, "acct-1", "signal", "Signal");
        assert_eq!(platform.calls().len(), 1);
    }

    #[test]
    fn bridge_notify_swallows_notifier_failure() {
        // A notifier failure (or an unset port) never panics or blocks the health machine.
        let platform = CapturingPlatform::failing();
        let config = NotifyConfig::new(true);
        notify_bridge_disconnected(&platform, &config, "acct-1", "signal", "Signal");
        assert!(platform.calls().is_empty());
    }

    // ── Story 18.4: recording-fault / recording-warning notify entry points ─────
    #[test]
    fn recording_fault_posts_reason_copy_and_no_target() {
        let platform = CapturingPlatform::new();
        notify_recording_fault(&platform, "keeper-rec exited unexpectedly");
        assert_eq!(
            platform.calls(),
            vec![(
                "Recording failed".to_owned(),
                "keeper-rec exited unexpectedly — open keeper to restart the recording.".to_owned(),
            )]
        );
        // Coarse activation only: the banner owns the one-click Restart.
        assert_eq!(platform.targets(), vec![NotifyTarget::None]);
    }

    #[test]
    fn recording_warning_posts_still_running_copy_and_no_target() {
        let platform = CapturingPlatform::new();
        notify_recording_warning(&platform, "microphone disconnected — no microphone input");
        assert_eq!(
            platform.calls(),
            vec![(
                "Recording warning".to_owned(),
                "microphone disconnected — no microphone input — the recording is still running."
                    .to_owned(),
            )]
        );
        assert_eq!(platform.targets(), vec![NotifyTarget::None]);
    }

    #[test]
    fn recording_stopped_posts_reason_verbatim_without_still_running() {
        // Story 18.5 hard-floor stop: a DISTINCT entry from the warning leg —
        // the reason rides verbatim, and the "the recording is still running"
        // suffix that would contradict a stop is absent.
        let platform = CapturingPlatform::new();
        notify_recording_stopped(&platform, "Recording stopped — low disk");
        assert_eq!(
            platform.calls(),
            vec![(
                "Recording stopped".to_owned(),
                "Recording stopped — low disk".to_owned(),
            )]
        );
        assert!(!platform.calls()[0].1.contains("still running"));
        assert_eq!(platform.targets(), vec![NotifyTarget::None]);
    }

    #[test]
    fn recording_stopped_bypasses_dnd_and_network_mute() {
        // The graceful stop-and-finalize alert is a loss-risk event, not chat
        // noise: it consults no NotifyConfig, so the harshest suppression state
        // (DND + every Network muted) never silences it.
        let platform = CapturingPlatform::new();
        let mut seed = HashSet::new();
        seed.insert("Signal".to_owned());
        let config = NotifyConfig::with_state(true, true, seed);
        assert!(config.dnd_enabled());
        assert!(config.is_network_muted("Signal"));
        notify_recording_stopped(&platform, "Recording stopped — low disk");
        assert_eq!(platform.calls().len(), 1);
    }

    #[test]
    fn recording_stopped_swallows_notifier_failure() {
        // A notifier failure (or an unset port on a headless build) never panics
        // or blocks the recording driver.
        let platform = CapturingPlatform::failing();
        notify_recording_stopped(&platform, "Recording stopped — low disk");
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn recording_fault_bypasses_dnd_and_network_mute() {
        // Global DND on AND every plausible Network muted: the recording fault/
        // warning legs still post — they are loss-risk events, not chat noise.
        // The entries take no NotifyConfig by construction, so this asserts the
        // bypass holds even with the harshest suppression state configured.
        let platform = CapturingPlatform::new();
        let mut seed = HashSet::new();
        seed.insert("Signal".to_owned());
        let config = NotifyConfig::with_state(true, true, seed);
        assert!(config.dnd_enabled());
        assert!(config.is_network_muted("Signal"));
        notify_recording_fault(&platform, "writer stalled");
        notify_recording_warning(&platform, "microphone disconnected");
        assert_eq!(platform.calls().len(), 2);
    }

    #[test]
    fn recording_fault_does_not_touch_message_gating() {
        // The message pipeline's should_notify verdicts are unchanged by the
        // recording entries existing: DND still silences a chat message even
        // right after a recording fault posted.
        let platform = CapturingPlatform::new();
        let config = NotifyConfig::new(true);
        config.set_dnd_enabled(true);
        notify_recording_fault(&platform, "capture device lost");
        assert_eq!(platform.calls().len(), 1, "the fault posted despite DND");
        dispatch(
            &platform,
            &config,
            "acct",
            "!room:example.org",
            50,
            &ctx_gated(100, true, false),
        );
        assert_eq!(
            platform.calls().len(),
            1,
            "the DND-gated chat message still did not post"
        );
    }

    #[test]
    fn recording_notify_swallows_notifier_failure() {
        // A notifier failure (or an unset port) never panics or blocks the driver.
        let platform = CapturingPlatform::failing();
        notify_recording_fault(&platform, "keeper-rec exited unexpectedly");
        notify_recording_warning(&platform, "microphone disconnected");
        assert!(platform.calls().is_empty());
    }

    #[test]
    fn notify_config_round_trips() {
        let config = NotifyConfig::new(true);
        assert!(config.previews_enabled());
        config.set_previews_enabled(false);
        assert!(!config.previews_enabled());
        config.set_previews_enabled(true);
        assert!(config.previews_enabled());
    }

    #[test]
    fn notify_config_dnd_and_muted_networks_round_trip() {
        let mut seed = HashSet::new();
        seed.insert("Telegram".to_owned());
        let config = NotifyConfig::with_state(true, true, seed);
        // Seeded state is readable.
        assert!(config.dnd_enabled());
        assert!(config.is_network_muted("Telegram"));
        assert!(!config.is_network_muted("Signal"));

        // DND toggles independently.
        config.set_dnd_enabled(false);
        assert!(!config.dnd_enabled());

        // Per-Network mute add/remove.
        config.set_network_muted("Signal", true);
        assert!(config.is_network_muted("Signal"));
        config.set_network_muted("Telegram", false);
        assert!(!config.is_network_muted("Telegram"));

        // Wholesale replace re-seeds the set.
        let mut next = HashSet::new();
        next.insert("WhatsApp".to_owned());
        config.replace_muted_networks(next);
        assert!(config.is_network_muted("WhatsApp"));
        assert!(!config.is_network_muted("Signal"));
    }

    #[test]
    fn localpart_fallback_extracts_localpart() {
        assert_eq!(localpart_fallback("@alice:example.org"), "alice");
        // Malformed ids fall back to the whole string.
        assert_eq!(localpart_fallback("weird"), "weird");
    }

    #[test]
    fn image_info_is_ignored_by_preview() {
        // A media message with rich info still yields only the descriptor.
        let mut content = ImageMessageEventContent::new(
            "photo.png".to_owned(),
            MediaSource::Plain(owned_mxc_uri!("mxc://example.org/xyz")),
        );
        content.info = Some(Box::new(ImageInfo::new()));
        assert_eq!(
            preview_for(&MessageType::Image(content)),
            ("Photo".to_owned(), true)
        );
    }
}
