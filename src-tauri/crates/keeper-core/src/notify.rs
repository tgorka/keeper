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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, Relation,
};
use matrix_sdk::{Client, Room};

use matrix_sdk::event_handler::EventHandlerHandle;

use crate::platform::Platform;

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
}

impl NotifyConfig {
    /// Construct with the given initial "message previews" state (seeded from the
    /// persisted registry value in [`AccountManager::new`](crate::account::AccountManager)).
    pub fn new(previews_enabled: bool) -> Self {
        Self {
            previews_enabled: AtomicBool::new(previews_enabled),
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
}

/// The extracted, SDK-free context a single [`dispatch`] decision operates on. Built
/// from the message event by [`register_notify_handler`] so the rules never touch the
/// SDK types directly.
pub struct NotifyContext {
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
}

/// Whether a message should raise a notification (pure rule).
///
/// A notification is raised only when the message is **not** our own echo, is **not**
/// pre-session backlog (`event_ts_ms >= baseline_ms`), and is a notifying message type.
/// Backlog suppression drops cold-launch history (the inbox already shows it) while
/// still notifying messages that arrive during a live background session.
pub fn should_notify(is_self: bool, event_ts_ms: u64, baseline_ms: u64, notifies: bool) -> bool {
    !is_self && notifies && event_ts_ms >= baseline_ms
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
    if !should_notify(ctx.is_self, ctx.event_ts_ms, baseline_ms, ctx.notifies) {
        return;
    }
    let (title, body) = format_notification(
        &ctx.chat,
        &ctx.sender,
        &ctx.preview,
        config.previews_enabled(),
    );
    if let Err(e) = platform.notify(&title, &body) {
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
    client.add_event_handler(move |ev: OriginalSyncRoomMessageEvent, room: Room| {
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

            // Cheap early-out on the pure gates before any display-name resolution
            // (avoids a member/room lookup for our own echo or suppressed backlog).
            if !should_notify(is_self, event_ts_ms, baseline_ms, notifies) {
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
                chat,
                sender: sender_name,
                preview,
                is_self,
                event_ts_ms,
                notifies,
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
    })
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

    /// A capturing [`Platform`] double recording every `(title, body)` posted through
    /// `notify`, so the dispatch matrix is covered without a homeserver. `notify` can be
    /// made to fail to exercise the swallow path.
    struct CapturingPlatform {
        calls: Mutex<Vec<(String, String)>>,
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
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().expect("lock calls").clone()
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
        fn notify(&self, title: &str, body: &str) -> Result<(), CoreError> {
            if self.fail {
                return Err(CoreError::Unsupported("notify failed in test".to_owned()));
            }
            self.calls
                .lock()
                .expect("lock calls")
                .push((title.to_owned(), body.to_owned()));
            Ok(())
        }
        fn sidecar_path(&self, _name: &str) -> Result<PathBuf, CoreError> {
            Err(CoreError::Unsupported("sidecar unused in tests".to_owned()))
        }
    }

    fn text(body: &str) -> MessageType {
        MessageType::Text(TextMessageEventContent::plain(body))
    }

    fn ctx(
        chat: &str,
        sender: &str,
        preview: &str,
        is_self: bool,
        ts: u64,
        notifies: bool,
    ) -> NotifyContext {
        NotifyContext {
            chat: chat.to_owned(),
            sender: sender.to_owned(),
            preview: preview.to_owned(),
            is_self,
            event_ts_ms: ts,
            notifies,
        }
    }

    // ── should_notify ──────────────────────────────────────────────────────────
    #[test]
    fn should_notify_true_for_live_other_message() {
        assert!(should_notify(false, 100, 50, true));
    }

    #[test]
    fn should_notify_false_for_own_echo() {
        assert!(!should_notify(true, 100, 50, true));
    }

    #[test]
    fn should_notify_false_for_backlog() {
        // origin_ts strictly before the baseline is suppressed backlog.
        assert!(!should_notify(false, 40, 50, true));
    }

    #[test]
    fn should_notify_true_at_exact_baseline() {
        assert!(should_notify(false, 50, 50, true));
    }

    #[test]
    fn should_notify_false_for_non_notifying_type() {
        assert!(!should_notify(false, 100, 50, false));
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
