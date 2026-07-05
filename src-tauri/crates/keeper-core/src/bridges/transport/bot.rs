//! The Bridge Bot chat **fallback** transport (Story 6.4, FR-27, AD-16).
//!
//! [`BotDriver`] is the second [`BridgeTransport`] impl — the seam Story 6.3 built
//! for exactly this. It drives a bridge login by sending **Bridge Bot chat
//! commands** to the bot's DM room and parsing the bot's reply events into the
//! *same* [`LoginStepResponse`] wire shapes the generic
//! [`drive_login`](crate::bridges::login::drive_login) already consumes — so the
//! emitted stepper states are indistinguishable from the provisioning path, with
//! zero driver changes.
//!
//! **Pure core, impure shell.** The reply→step classifier [`classify_bot_reply`]
//! and the [`BotReply`] normalization are pure and fully unit-tested (the whole I/O
//! matrix). The impure Matrix shell — sending a command via the SDK room API and
//! awaiting the bot's next reply with a timeout — cannot be exercised against a live
//! bot unattended and is a documented residual risk (as with 6.2's discovery and
//! 6.3's provisioning shells).
//!
//! **No image-QR decoding.** A bot reply that presents its QR only as an `m.image`
//! (no extractable scannable payload) maps to [`LoginStep::Unknown`], which the
//! driver renders as the existing honest unsupported-in-bot-mode failure (which
//! already names the Bridge Bot chat) — never a blank QR panel, never a fake
//! success.
//!
//! **Verbatim errors.** A reply that matches no rule, or a bot error line, is
//! surfaced as [`BridgeError::Bot`] carrying the bot's raw reply verbatim
//! (length-capped like the provisioning error cap) so `drive_login` shows it
//! verbatim — keeper never guesses at output it can't classify.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use matrix_sdk::event_handler::EventHandlerHandle;
use matrix_sdk::ruma::events::room::message::{
    MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
};
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::{Client, Room};
use tokio::sync::mpsc;

use crate::bridges::data::BotProtocol;
use crate::bridges::transport::{
    BridgeTransport, DisplayData, LoginFlow, LoginStep, LoginStepResponse,
};
use crate::error::BridgeError;

/// The synthetic login id / step id every `BotDriver` step carries. The bot login
/// has no server-side login id, so the driver synthesizes opaque handles the
/// generic driver treads as opaque strings (it never routes them anywhere but back
/// to the bot transport, which ignores them).
const SYNTHETIC_LOGIN_ID: &str = "bot-login";
/// The single synthetic flow id (`drive_login` auto-starts when there is one flow).
const SYNTHETIC_FLOW_ID: &str = "bot";

/// The reply timeout for a `login_start` / `user_input` submit — the user just
/// acted, so the bot should answer promptly. On timeout the transport surfaces a
/// [`BridgeError::Bot`] "didn't respond" failure (Retry), never an infinite spinner.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// The generous long-wait for a `display_and_wait` poll — the user is scanning a QR
/// or acting on a peer device. On timeout the driver re-emits the current display
/// rather than failing (the poll loops).
const DISPLAY_WAIT_TIMEOUT: Duration = Duration::from_secs(120);

/// The maximum length (chars) of a surfaced bot message. A bot could reply with an
/// arbitrarily large body; capping it keeps an unbounded message from reaching the
/// VM/DOM verbatim (mirrors the provisioning `MAX_ERROR_MESSAGE_CHARS`).
const MAX_BOT_MESSAGE_CHARS: usize = 2000;

/// Truncate a surfaced bot message to [`MAX_BOT_MESSAGE_CHARS`] on a char boundary
/// (`chars().take(..)` never splits a codepoint).
fn cap_message(msg: &str) -> String {
    msg.chars().take(MAX_BOT_MESSAGE_CHARS).collect::<String>()
}

/// The honest "didn't respond" failure for a `login_start` / `user_input` reply
/// timeout (never an infinite spinner).
fn no_reply_error() -> BridgeError {
    BridgeError::Bot("The bridge bot didn't respond. Try again.".to_owned())
}

/// Wait for the next reply on an armed listener, bounded by `timeout`. A timeout or
/// a closed channel (all senders dropped) both yield `None`.
async fn recv_reply(
    mut rx: mpsc::UnboundedReceiver<BotReply>,
    timeout: Duration,
) -> Option<BotReply> {
    match tokio::time::timeout(timeout, rx.recv()).await {
        Ok(Some(reply)) => Some(reply),
        // A timeout (`Err`) or a closed channel (`Ok(None)`) both mean no reply.
        Ok(None) | Err(_) => None,
    }
}

/// Removes a registered room event handler on drop, so the handler never leaks past
/// the wait — even if the driver task is aborted (cancel / graceful shutdown)
/// mid-await, when an early `remove_event_handler` call would be skipped.
struct HandlerGuard {
    client: Client,
    handle: Option<EventHandlerHandle>,
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            self.client.remove_event_handler(handle);
        }
    }
}

/// A normalized Bridge Bot reply, the pure input to [`classify_bot_reply`]. The
/// impure shell builds one from a real `m.room.message`; tests build one directly.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BotReply {
    /// Whether the reply carried an `m.image` (a QR-as-image or any picture).
    pub has_image: bool,
    /// The reply's text body (empty for an image-only reply). Already trimmed.
    pub body: String,
}

/// The classification of a bot reply — either a login [`LoginStep`] the driver can
/// render, or a verbatim error to surface. Split out from `classify_bot_reply`'s
/// return so the transport can map the `Err` arm to [`BridgeError::Bot`] while the
/// classifier itself stays a pure total function testable without I/O.
///
/// A reply that matches no rule, or a recognized bot error line, is an `Err`
/// (verbatim). Everything else is an `Ok(step)`.
type ClassifyResult = Result<LoginStep, String>;

/// Whether a line looks like a bot **error** (a failure the bot reported), so it is
/// surfaced verbatim rather than treated as an input prompt.
fn is_error_line(lower: &str) -> bool {
    const ERROR_MARKERS: [&str; 6] = [
        "error",
        "failed",
        "failure",
        "invalid",
        "couldn't",
        "could not",
    ];
    ERROR_MARKERS.iter().any(|m| lower.contains(m))
}

/// Whether a line looks like a **success** confirmation (the login completed).
fn is_success_line(lower: &str) -> bool {
    // Specific terminal phrasings only. A bare "logged in as" is deliberately NOT a
    // marker: it appears in instructional copy ("once you're logged in as your
    // user…") and would flip the stepper to a false Success before the login
    // actually completed. "successfully logged in" already covers the real case.
    const SUCCESS_MARKERS: [&str; 4] = [
        "successfully logged in",
        "login successful",
        "successfully connected",
        "you're now logged in",
    ];
    SUCCESS_MARKERS.iter().any(|m| lower.contains(m))
}

/// Whether a line is an **input prompt** (the bot is asking the user to type
/// something), from imperative keywords.
fn is_prompt_line(lower: &str) -> bool {
    const PROMPT_MARKERS: [&str; 6] = [
        "enter",
        "please send",
        "send me",
        "reply with",
        "type",
        "input",
    ];
    PROMPT_MARKERS.iter().any(|m| lower.contains(m))
}

/// Infer the provisioning field type from an input-prompt line's keywords, matching
/// the field types `step_to_vm` treats specially (`2fa_code` → code input,
/// `password` → masked). Keyword precedence: code, then phone, then password, else
/// plain text.
fn infer_field_type(lower: &str) -> &'static str {
    if lower.contains("code") || lower.contains("2fa") || lower.contains("otp") {
        "2fa_code"
    } else if lower.contains("phone") || lower.contains("number") {
        "phone_number"
    } else if lower.contains("password") || lower.contains("passphrase") {
        "password"
    } else {
        "text"
    }
}

/// Whether the body carries a scannable QR **payload** as text — a mautrix QR-login
/// bot commonly posts the payload string (e.g. WhatsApp's `2@…` linking code) as
/// text alongside or instead of an image. A heuristic: a single long token with no
/// spaces that isn't obviously prose. Returns the payload when found.
fn extract_qr_payload(body: &str) -> Option<String> {
    let trimmed = body.trim();
    // A QR payload is a single, long, space-free token. Multi-line / multi-word
    // prose is not a payload.
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }
    // Long enough to plausibly be a linking payload, not a short word.
    if trimmed.chars().count() < 20 {
        return None;
    }
    // Exclude tokens that are clearly NOT a scannable QR payload but which the
    // length/no-space rule would otherwise accept, so a bot that DMs one of these
    // never renders as a bogus, unscannable QR panel:
    // - a plain web URL (login QR payloads use app-specific schemes like WhatsApp's
    //   `2@…` or Signal's `sgnl://…`, never a bare `http(s)://` web link);
    // - a Matrix identifier (user `@…:…` / room `!…:…` / alias `#…:…`).
    let lower = trimmed.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    if matches!(trimmed.chars().next(), Some('@') | Some('!') | Some('#')) && trimmed.contains(':')
    {
        return None;
    }
    Some(trimmed.to_owned())
}

/// Classify a normalized bot reply into a login [`LoginStep`] (the **pure**,
/// unit-tested core), given the network's [`BotProtocol`] (reserved for future
/// per-network grammar tuning; the default classification is protocol-agnostic).
///
/// Precedence (honest, most-specific first):
/// 1. a bot **error** line → `Err(verbatim)` (surface the bot's message);
/// 2. a **success** line → `Ok(Complete)`;
/// 3. a scannable QR **payload** in the text → `Ok(DisplayAndWait{Qr{data}})`;
/// 4. an image-only reply with no payload → `Ok(Unknown)` (honest unsupported-in-bot
///    failure, never a fake QR);
/// 5. an input **prompt** → `Ok(UserInput{one field, inferred type})`;
/// 6. anything else → `Err(verbatim)` (keeper never guesses).
pub fn classify_bot_reply(reply: &BotReply, _proto: &BotProtocol) -> ClassifyResult {
    let body = reply.body.trim();
    let lower = body.to_lowercase();

    // 1. A recognized bot error line is surfaced verbatim first — an error that also
    //    happens to contain a prompt keyword must still fail, not solicit input.
    if !body.is_empty() && is_error_line(&lower) {
        return Err(cap_message(body));
    }

    // 2. Success.
    if is_success_line(&lower) {
        return Ok(LoginStep::Complete {
            user_login_id: None,
        });
    }

    // 3. A scannable QR payload present as text → native QR panel (indistinguishable
    //    from the provisioning path). Preferred over the image-only fallback so a
    //    reply carrying BOTH an image and the payload text still renders natively.
    if let Some(payload) = extract_qr_payload(body) {
        return Ok(LoginStep::DisplayAndWait {
            step_id: SYNTHETIC_FLOW_ID.to_owned(),
            display_and_wait: DisplayData::Qr {
                data: Some(payload),
                image_url: None,
            },
        });
    }

    // 4. Image-only QR (has_image, no extractable payload) → honest unsupported
    //    state (drive_login renders the failure that names the Bridge Bot chat).
    if reply.has_image {
        return Ok(LoginStep::Unknown);
    }

    // 5. An input prompt → a single user_input field with an inferred type. The
    //    prompt line becomes the field name so the stepper shows the bot's own copy.
    if is_prompt_line(&lower) {
        let field_type = infer_field_type(&lower);
        return Ok(LoginStep::UserInput {
            step_id: SYNTHETIC_FLOW_ID.to_owned(),
            fields: vec![crate::bridges::transport::LoginField {
                id: "value".to_owned(),
                field_type: field_type.to_owned(),
                name: cap_message(body),
                description: None,
                pattern: None,
                default_value: None,
            }],
        });
    }

    // 6. Unclassifiable / empty → surface verbatim (or an honest empty-reply note),
    //    never a guess.
    if body.is_empty() {
        Err("The bridge bot sent an empty reply.".to_owned())
    } else {
        Err(cap_message(body))
    }
}

/// The internal synthetic-cursor state a [`BotDriver`] carries behind an
/// `Arc<Mutex<…>>` so the transport stays `Clone + Send`. Tracks the last step so a
/// `display_and_wait` re-emit (on poll timeout) can reproduce the current display.
#[derive(Debug, Default)]
struct BotState {
    /// The last classified step (so a `display_and_wait` poll timeout re-emits it
    /// rather than failing). `None` before `login_start`.
    last_step: Option<LoginStep>,
}

/// The Bridge Bot chat fallback transport (Story 6.4). Holds the live `Client`, the
/// resolved bot DM `Room`, the bot's MXID (to filter its replies), the network id,
/// the resolved command [`BotProtocol`], and the synthetic cursor state.
///
/// `Clone` (like [`Provisioning`](super::provisioning::Provisioning)) so an explicit
/// cancel / graceful-shutdown drain can best-effort send the bot's cancel command on
/// a retained clone: `Client` and `Room` are cheap handle clones; the cursor state
/// is shared via `Arc`.
#[derive(Clone)]
pub struct BotDriver {
    client: Client,
    room: Room,
    bot_mxid: OwnedUserId,
    protocol: BotProtocol,
    state: Arc<Mutex<BotState>>,
}

impl BotDriver {
    /// Build a bot driver bound to a resolved bot DM `room` and its `bot_mxid` on
    /// the account's `client`, driving `network_id` with `protocol`'s commands.
    pub fn new(client: Client, room: Room, bot_mxid: OwnedUserId, protocol: BotProtocol) -> Self {
        Self {
            client,
            room,
            bot_mxid,
            protocol,
            state: Arc::new(Mutex::new(BotState::default())),
        }
    }

    /// Send `command` as a plain-text bot chat message via the SDK room API (a
    /// control message, like a verification event — NOT the composer send-gate).
    async fn send_command(&self, command: &str) -> Result<(), BridgeError> {
        self.room
            .send(RoomMessageEventContent::text_plain(command))
            .await
            .map_err(|e| BridgeError::Bot(format!("could not send the bridge bot command: {e}")))?;
        Ok(())
    }

    /// Arm a listener for the bot's next reply **before** a command is sent, so a
    /// fast reply that lands during the send round-trip is not missed (registering
    /// the handler after the send would race it). Returns the receiver plus a
    /// [`HandlerGuard`] that removes the handler on drop — so it never leaks, even
    /// when the driver task is aborted mid-await (the cancel / Sheet-close path this
    /// feature explicitly supports). This is the impure Matrix shell (residual risk).
    fn arm_reply_listener(&self) -> (HandlerGuard, mpsc::UnboundedReceiver<BotReply>) {
        let (tx, rx) = mpsc::unbounded_channel::<BotReply>();
        let bot_mxid = self.bot_mxid.clone();
        let handle = self
            .room
            .add_event_handler(move |ev: OriginalSyncRoomMessageEvent| {
                let tx = tx.clone();
                let bot_mxid = bot_mxid.clone();
                async move {
                    // Only the bot's own replies count — ignore our own echoes and
                    // any other sender in the room.
                    if ev.sender != bot_mxid {
                        return;
                    }
                    // A closed receiver (we already got our reply and dropped it) is
                    // fine to ignore — best-effort.
                    let _ = tx.send(normalize_message(&ev.content.msgtype));
                }
            });
        (
            HandlerGuard {
                client: self.client.clone(),
                handle: Some(handle),
            },
            rx,
        )
    }

    /// Send `command` then await the bot's next reply, bounded by `timeout`. The
    /// listener is armed before the send (no race) and torn down by the guard on
    /// return or abort. A timeout / closed channel yields `None`; the caller decides
    /// (re-emit a display vs. an honest failure).
    async fn send_and_await(
        &self,
        command: &str,
        timeout: Duration,
    ) -> Result<Option<BotReply>, BridgeError> {
        let (_guard, rx) = self.arm_reply_listener();
        self.send_command(command).await?;
        Ok(recv_reply(rx, timeout).await)
    }

    /// Await + classify the bot's next reply after sending `command` (short
    /// timeout): a timeout is an honest "didn't respond" failure.
    async fn send_and_step(&self, command: &str) -> Result<LoginStepResponse, BridgeError> {
        match self.send_and_await(command, REPLY_TIMEOUT).await? {
            Some(reply) => self.classify_into_response(&reply),
            None => Err(no_reply_error()),
        }
    }

    /// Classify a reply into a [`LoginStepResponse`], recording the step so a later
    /// `display_and_wait` poll timeout can re-emit the current display. The `Err`
    /// arm carries the bot's verbatim message as [`BridgeError::Bot`].
    fn classify_into_response(&self, reply: &BotReply) -> Result<LoginStepResponse, BridgeError> {
        let step = classify_bot_reply(reply, &self.protocol).map_err(BridgeError::Bot)?;
        if let Ok(mut guard) = self.state.lock() {
            guard.last_step = Some(step.clone());
        }
        Ok(LoginStepResponse {
            login_id: SYNTHETIC_LOGIN_ID.to_owned(),
            step,
        })
    }

    /// The current step to re-emit on a `display_and_wait` poll timeout (the user is
    /// still scanning) — the last classified step, or an honest failure if somehow
    /// none was recorded.
    fn re_emit_current(&self) -> Result<LoginStepResponse, BridgeError> {
        let last = self.state.lock().ok().and_then(|g| g.last_step.clone());
        match last {
            Some(step) => Ok(LoginStepResponse {
                login_id: SYNTHETIC_LOGIN_ID.to_owned(),
                step,
            }),
            None => Err(BridgeError::Bot(
                "The bridge bot didn't respond. Try again.".to_owned(),
            )),
        }
    }
}

/// Normalize a Matrix message `msgtype` into a [`BotReply`] (the impure→pure
/// boundary): an image carries `has_image`; text/notice/emote carry their body. The
/// body is trimmed. Split out so the classification input stays a plain value.
fn normalize_message(msgtype: &MessageType) -> BotReply {
    match msgtype {
        // Keep the image's caption (some QR-login bots post the scannable payload as
        // the image body): a single-token caption can still render a native QR
        // (classifier step 3 runs before the image-only fallback), while a
        // descriptive multi-word caption has whitespace and correctly falls through
        // to the image-only unsupported state.
        MessageType::Image(c) => BotReply {
            has_image: true,
            body: c.body.trim().to_owned(),
        },
        MessageType::Text(c) => BotReply {
            has_image: false,
            body: c.body.trim().to_owned(),
        },
        MessageType::Notice(c) => BotReply {
            has_image: false,
            body: c.body.trim().to_owned(),
        },
        MessageType::Emote(c) => BotReply {
            has_image: false,
            body: c.body.trim().to_owned(),
        },
        // Any other message type carries no classifiable text and no QR image.
        other => BotReply {
            has_image: false,
            body: other.body().trim().to_owned(),
        },
    }
}

impl BridgeTransport for BotDriver {
    // The trait declares these as `fn -> impl Future + Send` (static dispatch, no
    // `async-trait`); the impl uses `async fn` bodies, whose futures the compiler
    // infers `Send` — clippy's `manual_async_fn` prefers this shorter form and it is
    // equivalent (the same pattern as the `FakeTransport` in `login.rs`).
    async fn login_flows(&self) -> Result<Vec<LoginFlow>, BridgeError> {
        // A bot presents a single login path → `drive_login` auto-starts (no
        // ChoosingMethod), matching the indistinguishable-flow contract.
        Ok(vec![LoginFlow {
            id: SYNTHETIC_FLOW_ID.to_owned(),
            name: "Bridge Bot".to_owned(),
            description: None,
        }])
    }

    async fn login_start(&self, _flow_id: &str) -> Result<LoginStepResponse, BridgeError> {
        self.send_and_step(&self.protocol.login_command).await
    }

    async fn login_step(
        &self,
        _login_id: &str,
        _step_id: &str,
        step_type: &str,
        body: &BTreeMap<String, String>,
    ) -> Result<LoginStepResponse, BridgeError> {
        match step_type {
            // A typed-input step: send the value(s) as a bot message, then await
            // and classify the next reply (short timeout).
            "user_input" => {
                // Join submitted field values with a newline (a single-field
                // classifier normally yields one value; this stays honest for any
                // future multi-field prompt).
                let value = body.values().cloned().collect::<Vec<_>>().join("\n");
                self.send_and_step(&value).await
            }
            // A display_and_wait poll: await the next reply with the generous
            // long-wait; on timeout re-emit the current display rather than
            // failing (the user is still scanning / acting). No command is sent —
            // the listener is armed and we simply wait for the bot's next event.
            "display_and_wait" => {
                let (_guard, rx) = self.arm_reply_listener();
                match recv_reply(rx, DISPLAY_WAIT_TIMEOUT).await {
                    Some(reply) => self.classify_into_response(&reply),
                    None => self.re_emit_current(),
                }
            }
            // The generic driver only ever sends the two step types above; an
            // unexpected type is surfaced honestly rather than silently swallowed
            // into a long wait.
            other => Err(BridgeError::Bot(format!(
                "unexpected bridge bot login step type: {other}"
            ))),
        }
    }

    async fn login_cancel(&self, _login_id: &str) {
        // Best-effort: send the cancel command; log and swallow any error —
        // cancel must never surface an error.
        match self.send_command(&self.protocol.cancel_command).await {
            Ok(()) => tracing::debug!("bridge bot login cancel command sent"),
            Err(e) => tracing::debug!(error = %e, "bridge bot cancel failed (best-effort)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proto() -> BotProtocol {
        BotProtocol {
            login_command: "login".to_owned(),
            cancel_command: "cancel".to_owned(),
        }
    }

    fn text(body: &str) -> BotReply {
        BotReply {
            has_image: false,
            body: body.to_owned(),
        }
    }

    // --- The pure classifier I/O matrix ------------------------------------

    #[test]
    fn prompt_maps_to_user_input_with_inferred_code_type() {
        let step = classify_bot_reply(&text("Enter the 2FA code sent to your device"), &proto())
            .expect("classifies");
        match step {
            LoginStep::UserInput { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].field_type, "2fa_code");
                assert!(
                    fields[0].name.contains("Enter the 2FA code"),
                    "the field name carries the bot's own prompt: {}",
                    fields[0].name
                );
            }
            other => panic!("expected user_input, got {other:?}"),
        }
    }

    #[test]
    fn prompt_infers_phone_and_password_and_text_types() {
        let phone = classify_bot_reply(&text("Please send your phone number"), &proto())
            .expect("classifies");
        assert!(matches!(
            phone,
            LoginStep::UserInput { ref fields, .. } if fields[0].field_type == "phone_number"
        ));

        let password =
            classify_bot_reply(&text("Enter your password"), &proto()).expect("classifies");
        assert!(matches!(
            password,
            LoginStep::UserInput { ref fields, .. } if fields[0].field_type == "password"
        ));

        // A prompt with no type keyword falls back to plain text.
        let plain = classify_bot_reply(&text("Type your username to continue"), &proto())
            .expect("classifies");
        assert!(matches!(
            plain,
            LoginStep::UserInput { ref fields, .. } if fields[0].field_type == "text"
        ));
    }

    #[test]
    fn text_qr_payload_maps_to_display_and_wait_qr() {
        // A long, space-free token is treated as a scannable QR payload.
        let payload = "2@AbCdEf1234567890GhIjKlMnOpQr";
        let step = classify_bot_reply(&text(payload), &proto()).expect("classifies");
        match step {
            LoginStep::DisplayAndWait {
                display_and_wait: DisplayData::Qr { data, image_url },
                ..
            } => {
                assert_eq!(data.as_deref(), Some(payload));
                assert!(image_url.is_none());
            }
            other => panic!("expected display_and_wait qr, got {other:?}"),
        }
    }

    #[test]
    fn non_payload_long_tokens_are_not_treated_as_qr() {
        // A bare web URL and a Matrix identifier are long, space-free tokens the
        // length rule would accept, but they are NOT scannable login QR payloads —
        // they must not render as a bogus QR panel. With no other rule matching,
        // they fall through to a verbatim surface rather than a fake QR.
        for token in [
            "https://example.org/some/very/long/path/here",
            "@whatsappbot:example.org",
            "!averylongroomidentifier:example.org",
        ] {
            let step = classify_bot_reply(&text(token), &proto());
            assert!(
                !matches!(
                    step,
                    Ok(LoginStep::DisplayAndWait {
                        display_and_wait: DisplayData::Qr { .. },
                        ..
                    })
                ),
                "token must not be a QR payload: {token}"
            );
        }
        // A custom-scheme login payload (e.g. Signal's `sgnl://…`) IS still a QR.
        let signal = "sgnl://linkdevice?uuid=abcdEFGH1234&pubkey=zzzz";
        assert!(matches!(
            classify_bot_reply(&text(signal), &proto()),
            Ok(LoginStep::DisplayAndWait {
                display_and_wait: DisplayData::Qr { .. },
                ..
            })
        ));
    }

    #[test]
    fn instructional_logged_in_as_is_not_a_false_success() {
        // "logged in as" inside instructional copy must NOT flip the stepper to a
        // premature Success before the login actually completed.
        let step = classify_bot_reply(
            &text("You will be logged in as @alice once this finishes."),
            &proto(),
        );
        assert!(
            !matches!(step, Ok(LoginStep::Complete { .. })),
            "instructional copy must not be a false success: {step:?}"
        );
    }

    #[test]
    fn image_caption_carrying_the_payload_renders_native_qr() {
        // A real m.image whose caption body is the scannable payload must keep the
        // caption (not drop it) so it renders a native QR rather than the image-only
        // unsupported state.
        use matrix_sdk::ruma::events::room::message::ImageMessageEventContent;
        use matrix_sdk::ruma::OwnedMxcUri;
        let reply = normalize_message(&MessageType::Image(ImageMessageEventContent::plain(
            "2@AbCdEf1234567890GhIjKlMnOpQr".to_owned(),
            OwnedMxcUri::from("mxc://example.org/qr"),
        )));
        assert!(reply.has_image);
        assert_eq!(reply.body, "2@AbCdEf1234567890GhIjKlMnOpQr");
        assert!(matches!(
            classify_bot_reply(&reply, &proto()),
            Ok(LoginStep::DisplayAndWait {
                display_and_wait: DisplayData::Qr { .. },
                ..
            })
        ));
    }

    #[test]
    fn image_only_qr_without_payload_is_unknown() {
        // An image with no extractable payload → honest unsupported (drive_login
        // renders the failure that names the Bridge Bot chat), never a fake QR.
        let reply = BotReply {
            has_image: true,
            body: String::new(),
        };
        let step = classify_bot_reply(&reply, &proto()).expect("classifies");
        assert_eq!(step, LoginStep::Unknown);
    }

    #[test]
    fn image_with_payload_text_still_renders_native_qr() {
        // A reply carrying BOTH an image and the payload text prefers the native QR.
        let reply = BotReply {
            has_image: true,
            body: "2@AbCdEf1234567890GhIjKlMnOpQr".to_owned(),
        };
        let step = classify_bot_reply(&reply, &proto()).expect("classifies");
        assert!(matches!(
            step,
            LoginStep::DisplayAndWait {
                display_and_wait: DisplayData::Qr { .. },
                ..
            }
        ));
    }

    #[test]
    fn success_line_maps_to_complete() {
        let step = classify_bot_reply(&text("Successfully logged in as @alice"), &proto())
            .expect("classifies");
        assert!(matches!(step, LoginStep::Complete { .. }));
    }

    #[test]
    fn error_line_is_surfaced_verbatim() {
        let msg = "Error: this phone number is not registered on WhatsApp";
        let err = classify_bot_reply(&text(msg), &proto()).expect_err("is an error");
        assert_eq!(err, msg);
    }

    #[test]
    fn error_line_beats_a_prompt_keyword() {
        // A reply that reports a failure AND contains a prompt keyword must still
        // surface verbatim, not solicit input.
        let msg = "Login failed, please try again with a valid code";
        let err = classify_bot_reply(&text(msg), &proto()).expect_err("is an error");
        assert_eq!(err, msg);
    }

    #[test]
    fn unclassifiable_reply_is_surfaced_verbatim() {
        let msg = "Some chatty prose the bot said that matches no rule.";
        let err = classify_bot_reply(&text(msg), &proto()).expect_err("is an error");
        assert_eq!(err, msg);
    }

    #[test]
    fn empty_reply_is_an_honest_error() {
        let err = classify_bot_reply(&text("   "), &proto()).expect_err("is an error");
        assert!(err.contains("empty"), "names the empty reply: {err}");
    }

    #[test]
    fn verbatim_error_is_length_capped() {
        let huge = format!("error {}", "x".repeat(MAX_BOT_MESSAGE_CHARS + 5000));
        let err = classify_bot_reply(&text(&huge), &proto()).expect_err("is an error");
        assert_eq!(err.chars().count(), MAX_BOT_MESSAGE_CHARS);
    }

    #[test]
    fn normalize_message_marks_image_and_trims_text() {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        let text_reply = normalize_message(&MessageType::Text(TextMessageEventContent::plain(
            "  hello bot  ",
        )));
        assert_eq!(text_reply.body, "hello bot");
        assert!(!text_reply.has_image);
    }

    // --- The scripted-fake driver test: prove BotDriver's produced steps drive
    // --- drive_login to the same BridgeLoginPhase sequence as Provisioning ----

    /// A `BotDriver`-shaped transport over a scripted reply source: it classifies a
    /// queued list of [`BotReply`]s exactly as `BotDriver` does, WITHOUT any Matrix
    /// I/O. This proves the classifier + step synthesis drive `drive_login` to the
    /// same phase sequence a Provisioning script would — the "indistinguishable"
    /// contract — without a live bot (the impure shell is documented residual risk).
    struct ScriptedBotDriver {
        protocol: BotProtocol,
        replies: Mutex<std::collections::VecDeque<BotReply>>,
        state: Arc<Mutex<BotState>>,
    }

    impl ScriptedBotDriver {
        fn new(replies: Vec<BotReply>) -> Self {
            Self {
                protocol: proto(),
                replies: Mutex::new(replies.into()),
                state: Arc::new(Mutex::new(BotState::default())),
            }
        }

        fn classify_next(&self) -> Result<LoginStepResponse, BridgeError> {
            let reply = self.replies.lock().expect("lock").pop_front();
            match reply {
                Some(reply) => {
                    let step =
                        classify_bot_reply(&reply, &self.protocol).map_err(BridgeError::Bot)?;
                    self.state.lock().expect("lock").last_step = Some(step.clone());
                    Ok(LoginStepResponse {
                        login_id: SYNTHETIC_LOGIN_ID.to_owned(),
                        step,
                    })
                }
                None => Err(BridgeError::Bot("script exhausted".to_owned())),
            }
        }
    }

    impl BridgeTransport for ScriptedBotDriver {
        async fn login_flows(&self) -> Result<Vec<LoginFlow>, BridgeError> {
            Ok(vec![LoginFlow {
                id: SYNTHETIC_FLOW_ID.to_owned(),
                name: "Bridge Bot".to_owned(),
                description: None,
            }])
        }

        async fn login_start(&self, _flow_id: &str) -> Result<LoginStepResponse, BridgeError> {
            self.classify_next()
        }

        async fn login_step(
            &self,
            _login_id: &str,
            _step_id: &str,
            _step_type: &str,
            _body: &BTreeMap<String, String>,
        ) -> Result<LoginStepResponse, BridgeError> {
            self.classify_next()
        }

        async fn login_cancel(&self, _login_id: &str) {}
    }

    #[tokio::test]
    async fn scripted_bot_driver_drives_qr_then_complete_like_provisioning() {
        use crate::bridges::login::drive_login;
        use crate::vm::{BridgeLoginPhase, BridgeLoginVm};

        // A QR payload then a success line — the same shape a WhatsApp QR login takes.
        let transport = ScriptedBotDriver::new(vec![
            text("2@AbCdEf1234567890GhIjKlMnOpQr"),
            text("Successfully logged in as @alice"),
        ]);

        let captured = Arc::new(Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: crate::bridges::login::BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (_tx, rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(None));
        drive_login(transport, "whatsapp", sink, rx, slot).await;

        let vms = captured.lock().expect("lock");
        let phases: Vec<_> = vms.iter().map(|v| v.phase).collect();
        // waiting → qr → success (single flow auto-starts, no ChoosingMethod) —
        // identical to the Provisioning QR path.
        assert_eq!(
            phases,
            vec![
                BridgeLoginPhase::Waiting,
                BridgeLoginPhase::Qr,
                BridgeLoginPhase::Success,
            ]
        );
    }

    #[tokio::test]
    async fn scripted_bot_driver_drives_prompt_then_complete() {
        use crate::bridges::login::drive_login;
        use crate::vm::{BridgeLoginInput, BridgeLoginPhase, BridgeLoginVm};

        // A code prompt then success — the text/code login path.
        let transport = ScriptedBotDriver::new(vec![
            text("Enter the code sent to your phone"),
            text("Successfully connected"),
        ]);

        let captured = Arc::new(Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: crate::bridges::login::BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (tx, rx) = mpsc::unbounded_channel();
        // Feed the field value so the code-entry step advances to the next reply.
        let mut values = BTreeMap::new();
        values.insert("value".to_owned(), "123456".to_owned());
        tx.send(BridgeLoginInput::Fields { values }).expect("send");
        let slot = Arc::new(Mutex::new(None));
        drive_login(transport, "signal", sink, rx, slot).await;

        let vms = captured.lock().expect("lock");
        let phases: Vec<_> = vms.iter().map(|v| v.phase).collect();
        // waiting → codeEntry → success.
        assert_eq!(
            phases,
            vec![
                BridgeLoginPhase::Waiting,
                BridgeLoginPhase::CodeEntry,
                BridgeLoginPhase::Success,
            ]
        );
    }

    #[tokio::test]
    async fn scripted_bot_driver_surfaces_a_verbatim_error() {
        use crate::bridges::login::drive_login;
        use crate::vm::{BridgeLoginPhase, BridgeLoginVm};

        let transport = ScriptedBotDriver::new(vec![text("Error: this number is not registered")]);
        let captured = Arc::new(Mutex::new(Vec::<BridgeLoginVm>::new()));
        let sink_captured = captured.clone();
        let sink: crate::bridges::login::BridgeLoginSink = Box::new(move |vm| {
            sink_captured.lock().expect("lock").push(vm);
            true
        });
        let (_tx, rx) = mpsc::unbounded_channel();
        let slot = Arc::new(Mutex::new(None));
        drive_login(transport, "whatsapp", sink, rx, slot).await;

        let vms = captured.lock().expect("lock");
        let last = vms.last().expect("a failure vm");
        assert_eq!(last.phase, BridgeLoginPhase::Failure);
        assert_eq!(
            last.error.as_deref(),
            Some("bridge login failed: Error: this number is not registered")
        );
    }
}
