//! The agents' Matrix client: one `matrix_sdk::Client` per copy, beside the
//! messenger and not inside it (AD-370, AD-371; story 90.4).
//!
//! A copy is an agent's own device on one host. This client logs in with a
//! password once, keeps its session and store passphrase under the host's
//! secret keys `agents/<user>/session` and `agents/<user>/sdk-passphrase`,
//! reuses its device id on every later login, and syncs with a plain
//! `Client::sync` loop. It shares `StoredSession` and the store layout with the
//! messenger and none of its handlers: no archive, no notifications, no
//! drafts. `AccountManager` is not involved.
//!
//! **Every send disables the SDK's retry** (`RequestConfig::disable_retry`),
//! so a `M_LIMIT_EXCEEDED` reaches the caller with its `retry_after_ms`
//! instead of being retried without limit inside the SDK (R18, F25). State
//! sends go through `Client::send(send_state_event::v3::Request::new_raw(..))`
//! because, with keeper's features, `Room::send_state_event_raw` is a plain
//! async fn with no request-config hook (ruling D1).

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use matrix_sdk::config::{RequestConfig, SyncSettings};
use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::api::client::filter::{FilterDefinition, RoomEventFilter, RoomFilter};
use matrix_sdk::ruma::api::client::room::create_room;
use matrix_sdk::ruma::api::client::state::{get_state_events, send_state_event};
use matrix_sdk::ruma::api::client::sync::sync_events;
use matrix_sdk::ruma::api::error::{ErrorKind, RetryAfter};
use matrix_sdk::ruma::events::StateEventType;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{
    DeviceId, EventId, MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId,
    TransactionId, UInt, UserId,
};
use matrix_sdk::{Client, RoomState};
use serde_json::{json, Value};

use crate::agents::events::{self, CONTROL_ROOM_TYPE, SESSION_ROOM_TYPE};
use crate::agents::label::{check_sink, Label, Readers, Sink, SinkVerdict};
use crate::agents::session::SessionKind;
use crate::auth::StoredSession;

/// The secret key of a copy's stored session.
pub fn session_key(user: &UserId) -> String {
    format!("agents/{user}/session")
}

/// The secret key of a copy's store passphrase.
pub fn passphrase_key(user: &UserId) -> String {
    format!("agents/{user}/sdk-passphrase")
}

/// A copy's sqlite store: `<data>/agents/<user>/sdk`.
pub fn store_dir(data_dir: &Path, user: &UserId) -> PathBuf {
    data_dir.join("agents").join(user.as_str()).join("sdk")
}

/// The passphrase a sign-in opens `store` with: `stored`, the one in the
/// host's secrets, or a fresh one. Before a fresh one, a store already at
/// `store` is removed: it was left by a sign-in that failed before its
/// passphrase was kept, so it is encrypted under a passphrase nobody has
/// and would refuse every later sign-in, and no session can open it.
pub fn sign_in_passphrase(
    stored: Option<String>,
    store: &Path,
) -> Result<String, AgentMatrixError> {
    if let Some(passphrase) = stored {
        return Ok(passphrase);
    }
    match std::fs::remove_dir_all(store) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(AgentMatrixError::Other(format!(
                "could not remove {}: {err}",
                store.display()
            )))
        }
    }
    Ok(crate::auth::generate_store_passphrase())
}

/// The device id of a stored session's JSON, so a later sign-in reuses it
/// and the agent's user never collects stale devices. A password session
/// flattens the SDK's `MatrixSession`; an OAuth one nests it under
/// `user.meta`.
pub fn device_of_session(json: &str) -> Option<String> {
    let value: Value = serde_json::from_str(json).ok()?;
    value["device_id"]
        .as_str()
        .or_else(|| value["user"]["meta"]["device_id"].as_str())
        .map(str::to_owned)
}

/// What a homeserver call ended in, in the terms a host acts on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentMatrixError {
    /// `M_LIMIT_EXCEEDED`: wait `retry_after_ms` when given, then send once.
    #[error("the homeserver asks to wait{}", retry_after_ms.map(|ms| format!(" {ms} ms")).unwrap_or_default())]
    RateLimited { retry_after_ms: Option<u64> },
    /// `M_TOO_LARGE`: the event is over the server's cap.
    #[error("the event is larger than the homeserver accepts")]
    TooLarge,
    /// `M_FORBIDDEN`: power levels or membership refuse it.
    #[error("the homeserver refused it: {0}")]
    Forbidden(String),
    /// `M_NOT_FOUND`, or a room this copy has not joined.
    #[error("not found: {0}")]
    NotFound(String),
    /// The request never got an answer.
    #[error("the homeserver could not be reached: {0}")]
    Network(String),
    /// Refused before any request: the session's label does not reach whom
    /// it would add (AD-391); the sentence says who.
    #[error("{0}")]
    Label(String),
    #[error("{0}")]
    Other(String),
}

/// Classify a homeserver answer: its `errcode` when it had one, else whether
/// the transport failed. Pure, so every arm is tested without a server.
pub fn classify(
    kind: Option<&ErrorKind>,
    transport_failed: bool,
    text: String,
) -> AgentMatrixError {
    match kind {
        Some(ErrorKind::LimitExceeded(data)) => AgentMatrixError::RateLimited {
            retry_after_ms: data.retry_after.as_ref().map(retry_after_ms),
        },
        Some(ErrorKind::TooLarge) => AgentMatrixError::TooLarge,
        Some(ErrorKind::Forbidden) => AgentMatrixError::Forbidden(text),
        Some(ErrorKind::NotFound) => AgentMatrixError::NotFound(text),
        _ if transport_failed => AgentMatrixError::Network(text),
        _ => AgentMatrixError::Other(text),
    }
}

fn retry_after_ms(retry: &RetryAfter) -> u64 {
    match retry {
        RetryAfter::Delay(delay) => delay.as_millis() as u64,
        RetryAfter::DateTime(at) => at
            .duration_since(std::time::SystemTime::now())
            .unwrap_or(Duration::ZERO)
            .as_millis() as u64,
    }
}

fn from_http(error: matrix_sdk::HttpError) -> AgentMatrixError {
    let transport = matches!(error, matrix_sdk::HttpError::Reqwest(_));
    classify(error.client_api_error_kind(), transport, error.to_string())
}

fn from_sdk(error: matrix_sdk::Error) -> AgentMatrixError {
    let transport = matches!(
        &error,
        matrix_sdk::Error::Http(http) if matches!(**http, matrix_sdk::HttpError::Reqwest(_))
    );
    classify(error.client_api_error_kind(), transport, error.to_string())
}

/// How many timeline events one `/sync` hands out per room.
///
/// The server's default is a handful, and a room that received more than
/// that between two rounds comes back `limited`: the events cut from the gap
/// never reach a handler. Measured on Synapse (2026-10-02): at 20 events a
/// second, 12% of a copy's events were lost that way with the default.
pub const SYNC_TIMELINE_LIMIT: u32 = 500;

/// How many pages, of [`FORWARD_PAGE`] events, a forward read of a room
/// goes through at most; a read that reaches it with history left says so
/// ([`ForwardStates::complete`]).
pub const FORWARD_PAGES: usize = 50;
/// Events per forward page.
pub const FORWARD_PAGE: u32 = 100;

/// The sync settings every copy's loop uses ([`SYNC_TIMELINE_LIMIT`]).
pub fn sync_settings() -> SyncSettings {
    let mut timeline = RoomEventFilter::default();
    timeline.limit = Some(SYNC_TIMELINE_LIMIT.into());
    let mut room = RoomFilter::default();
    room.timeline = timeline;
    let mut definition = FilterDefinition::default();
    definition.room = room;
    SyncSettings::default().filter(sync_events::v3::Filter::FilterDefinition(definition))
}

/// No SDK retry: the caller paces, and a 429 must reach it (R18).
fn no_retry() -> RequestConfig {
    RequestConfig::new().disable_retry()
}

/// Which room to create.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomKind {
    Session(SessionKind),
    Control,
}

/// A state event as the server holds it now (C9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerState {
    pub event_id: OwnedEventId,
    pub sender: OwnedUserId,
    pub origin_server_ts: MilliSecondsSinceUnixEpoch,
    pub content: Value,
}

impl ServerState {
    /// A state event's JSON as a [`ServerState`]; `None` when it lacks a field.
    pub fn from_event(value: &Value) -> Option<ServerState> {
        Some(ServerState {
            event_id: OwnedEventId::try_from(value["event_id"].as_str()?).ok()?,
            sender: OwnedUserId::try_from(value["sender"].as_str()?).ok()?,
            origin_server_ts: MilliSecondsSinceUnixEpoch(
                value["origin_server_ts"].as_u64()?.try_into().ok()?,
            ),
            content: value["content"].clone(),
        })
    }
}

/// What a forward read of a room found, and whether it read to the room's
/// end (R75, R179).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForwardStates {
    /// The matching state events, in room order from where the read began.
    pub found: Vec<ServerState>,
    /// `false`: the read stopped at [`FORWARD_PAGES`] with history left, so
    /// an event it did not find may still be there. What it found is still
    /// in order from where it began: the first of them is the first.
    pub complete: bool,
}

/// One page of a forward read: its events' JSON, and the token of the next
/// page, `None` at the room's end.
pub type ForwardPage = (Vec<Value>, Option<String>);

/// Read forward page by page from `token` through `page` — no more than
/// [`FORWARD_PAGES`] pages — keeping each `(event_type, state_key)` state
/// event. A page may be empty while more history follows (a filtered read
/// skips what it does not match): only a missing next token ends the room.
pub async fn read_forward<F, Fut>(
    mut token: Option<String>,
    mut page: F,
    event_type: &str,
    state_key: &str,
) -> Result<ForwardStates, AgentMatrixError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<ForwardPage, AgentMatrixError>>,
{
    let mut found = Vec::new();
    for _ in 0..FORWARD_PAGES {
        let (events, next) = page(token.clone()).await?;
        found.extend(
            events
                .iter()
                .filter(|value| value["type"] == event_type && value["state_key"] == state_key)
                .filter_map(ServerState::from_event),
        );
        match next {
            Some(next) if Some(&next) != token.as_ref() => token = Some(next),
            _ => {
                return Ok(ForwardStates {
                    found,
                    complete: true,
                })
            }
        }
    }
    Ok(ForwardStates {
        found,
        complete: false,
    })
}

/// One copy's client.
#[derive(Debug, Clone)]
pub struct AgentClient {
    client: Client,
}

impl AgentClient {
    /// A client over `homeserver` with its encrypted sqlite store at `store`.
    /// No MSC4186 probe: a bot account needs none (§13 #24).
    pub async fn open(
        homeserver: &str,
        store: &Path,
        passphrase: &str,
    ) -> Result<AgentClient, AgentMatrixError> {
        std::fs::create_dir_all(store).map_err(|err| {
            AgentMatrixError::Other(format!("could not create {}: {err}", store.display()))
        })?;
        let client = Client::builder()
            .homeserver_url(homeserver)
            .sqlite_store(store, Some(passphrase))
            .handle_refresh_tokens()
            .build()
            .await
            .map_err(|err| AgentMatrixError::Other(format!("could not build the client: {err}")))?;
        Ok(AgentClient { client })
    }

    /// Password login as `user`, reusing `device_id` when the copy has one,
    /// displayed `<agent>@<host>`. Returns the session to store.
    pub async fn login(
        &self,
        user: &str,
        password: &str,
        device_id: Option<&str>,
        display_name: &str,
    ) -> Result<StoredSession, AgentMatrixError> {
        let mut login = self
            .client
            .matrix_auth()
            .login_username(user, password)
            .initial_device_display_name(display_name);
        if let Some(device) = device_id {
            login = login.device_id(device);
        }
        login.send().await.map_err(from_sdk)?;
        StoredSession::from_client(&self.client)
            .ok_or_else(|| AgentMatrixError::Other("the login left no session".to_owned()))
    }

    /// Restore a stored session; the next sync resumes from the store's token.
    pub async fn restore(&self, session: StoredSession) -> Result<(), AgentMatrixError> {
        session
            .restore_into(&self.client)
            .await
            .map_err(|err| AgentMatrixError::Other(err.to_string()))
    }

    /// The SDK client, for event handlers and the sync loop.
    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn user_id(&self) -> Option<&UserId> {
        self.client.user_id()
    }

    pub fn device_id(&self) -> Option<String> {
        self.client.device_id().map(|id| id.to_string())
    }

    /// One `/sync` round.
    pub async fn sync_once(&self) -> Result<(), AgentMatrixError> {
        self.client
            .sync_once(sync_settings().timeout(Duration::from_secs(0)))
            .await
            .map(|_| ())
            .map_err(from_sdk)
    }

    /// Create a typed, encrypted room as [`create_room_request`] makes it.
    pub async fn create_room(
        &self,
        kind: RoomKind,
        name: &str,
        invite: Vec<OwnedUserId>,
        agents: &[OwnedUserId],
    ) -> Result<OwnedRoomId, AgentMatrixError> {
        let me = self
            .client
            .user_id()
            .ok_or_else(|| AgentMatrixError::Other("not signed in".to_owned()))?
            .to_owned();
        let request = create_room_request(&kind, name, invite, &me, agents)?;
        // The SDK also records a direct room in the creator's `m.direct`.
        let room = self.client.create_room(request).await.map_err(from_sdk)?;
        Ok(room.room_id().to_owned())
    }

    /// Invite `user` into `room` — the one door an invite takes besides a
    /// room's creation — when what the room holds, labelled `label`, may
    /// reach them (AD-391, R160): a known agent through its own `audience`,
    /// anyone else as a person. A refusal sends nothing.
    pub async fn invite(
        &self,
        room: &RoomId,
        user: &UserId,
        label: &Label,
        audience: Option<Readers>,
    ) -> Result<(), AgentMatrixError> {
        let sink = match audience {
            Some(audience) => Sink::Room {
                humans: Default::default(),
                agent_audiences: vec![audience],
            },
            None => Sink::Room {
                humans: [user.to_owned()].into(),
                agent_audiences: Vec::new(),
            },
        };
        if let SinkVerdict::Block { reason, .. } = check_sink(label, &sink) {
            return Err(AgentMatrixError::Label(reason));
        }
        self.joined(room)?
            .invite_user_by_id(user)
            .await
            .map_err(from_sdk)
    }

    pub async fn join(&self, room: &RoomId) -> Result<(), AgentMatrixError> {
        self.client
            .join_room_by_id(room)
            .await
            .map(|_| ())
            .map_err(from_sdk)
    }

    /// Send a timeline event (encrypted in an encrypted room), once. A retry
    /// of the same event passes the same `txn` so the server deduplicates it.
    pub async fn send(
        &self,
        room: &RoomId,
        event_type: &str,
        content: Value,
        txn: Option<&TransactionId>,
    ) -> Result<OwnedEventId, AgentMatrixError> {
        let room = self.joined(room)?;
        let mut send = room
            .send_raw(event_type, content)
            .with_request_config(no_retry());
        if let Some(txn) = txn {
            send = send.with_transaction_id(txn);
        }
        Ok(send.await.map_err(from_sdk)?.response.event_id)
    }

    /// Send a state event (unencrypted), once.
    pub async fn send_state(
        &self,
        room: &RoomId,
        event_type: &str,
        state_key: &str,
        content: &Value,
    ) -> Result<OwnedEventId, AgentMatrixError> {
        self.joined(room)?;
        let request = send_state_event::v3::Request::new_raw(
            room.to_owned(),
            StateEventType::from(event_type),
            state_key.to_owned(),
            raw(content)?,
        );
        let response = self
            .client
            .send(request)
            .with_request_config(no_retry())
            .await
            .map_err(from_http)?;
        Ok(response.event_id)
    }

    /// The state event `(event_type, state_key)` as the server holds it now,
    /// read with `GET /rooms/{id}/state` — never the cached copy, which can be
    /// a sync behind (C9).
    pub async fn server_state(
        &self,
        room: &RoomId,
        event_type: &str,
        state_key: &str,
    ) -> Result<Option<ServerState>, AgentMatrixError> {
        let response = self
            .client
            .send(get_state_events::v3::Request::new(room.to_owned()))
            .with_request_config(no_retry())
            .await
            .map_err(from_http)?;
        for event in response.room_state {
            let Ok(value) = event.deserialize_as::<Value>() else {
                continue;
            };
            if value["type"] != event_type || value["state_key"] != state_key {
                continue;
            }
            return Ok(ServerState::from_event(&value));
        }
        Ok(None)
    }

    /// Every `event_type` state event of `room` as the last sync left it, by
    /// state key: what placement reads, never what a claim is decided on (C9).
    pub async fn cached_states(
        &self,
        room: &RoomId,
        event_type: &str,
    ) -> Vec<(String, ServerState)> {
        let Some(room) = self.client.get_room(room) else {
            return Vec::new();
        };
        let Ok(events) = room
            .get_state_events(StateEventType::from(event_type))
            .await
        else {
            return Vec::new();
        };
        events
            .into_iter()
            .filter_map(|event| {
                let value = match event {
                    RawAnySyncOrStrippedState::Sync(raw) => raw.deserialize_as::<Value>().ok()?,
                    RawAnySyncOrStrippedState::Stripped(_) => return None,
                };
                let key = value["state_key"].as_str()?.to_owned();
                Some((key, ServerState::from_event(&value)?))
            })
            .collect()
    }

    /// Every `(event_type, state_key)` state event in `room`'s timeline, in
    /// room order, read forward from just after `from` (its `/context` end
    /// token) — or from the room's first visible event when `from` is
    /// unknown — the server filtering by type, over at most
    /// [`FORWARD_PAGES`] pages, saying whether it reached the room's end
    /// (R75: the first one is what counts; the server's current state shows
    /// only the last).
    pub async fn state_events_from(
        &self,
        room: &RoomId,
        from: Option<&EventId>,
        event_type: &str,
        state_key: &str,
    ) -> Result<ForwardStates, AgentMatrixError> {
        let room = self.joined(room)?;
        let token = match from {
            Some(event) => {
                room.event_with_context(event, false, UInt::from(0u32), Some(no_retry()))
                    .await
                    .map_err(from_sdk)?
                    .next_batch_token
            }
            None => None,
        };
        let room = &room;
        read_forward(
            token,
            |token| async move {
                let mut options = MessagesOptions::forward().from(token.as_deref());
                options.limit = UInt::from(FORWARD_PAGE);
                options.filter.types = Some(vec![event_type.to_owned()]);
                let page = room.messages(options).await.map_err(from_sdk)?;
                let events = page
                    .chunk
                    .iter()
                    .filter_map(|event| event.raw().deserialize_as::<Value>().ok())
                    .collect();
                Ok((events, page.end))
            },
            event_type,
            state_key,
        )
        .await
    }

    /// Encrypt `bytes` and upload them: the `EncryptedFile` a message names
    /// (R86: an approval's large arguments travel only encrypted).
    pub async fn upload_encrypted(&self, bytes: &[u8]) -> Result<Value, AgentMatrixError> {
        let mut reader = std::io::Cursor::new(bytes);
        let file = self
            .client
            .upload_encrypted_file(&mut reader)
            .await
            .map_err(from_sdk)?;
        serde_json::to_value(&file)
            .map_err(|err| AgentMatrixError::Other(format!("could not encode the file: {err}")))
    }

    /// What `user`'s homeserver publishes now of their `device` and their
    /// identity (93.3's adapter, R182): one `/keys/query` asked for this
    /// call alone, judged on its own answer by
    /// [`crate::agents::trust::published_in`] — the master key and the
    /// device's signature chain from the same answer, never the crypto
    /// store, which keeps an earlier identity when an answer lacks one. An
    /// answer that is not whole is an error naming
    /// [`crate::agents::trust::KEYS_UNKNOWN`].
    pub async fn published(
        &self,
        user: &UserId,
        device: &DeviceId,
    ) -> Result<crate::agents::trust::Published, AgentMatrixError> {
        let answer = self.keys_answer(user, Some(device)).await?;
        crate::agents::trust::published_in(&answer, user, Some(device.as_str()))
            .map_err(|unknown| AgentMatrixError::Other(unknown.to_owned()))
    }

    /// `user`'s master key as their homeserver publishes it now, asked for
    /// fresh; `None` when they publish no cross-signing identity (R88).
    pub async fn published_master_key(
        &self,
        user: &UserId,
    ) -> Result<Option<String>, AgentMatrixError> {
        let answer = self.keys_answer(user, None).await?;
        crate::agents::trust::published_in(&answer, user, None)
            .map(|published| published.master_key)
            .map_err(|unknown| AgentMatrixError::Other(unknown.to_owned()))
    }

    /// One `/keys/query` for `user` (their `device` alone, or none), as its
    /// answer came.
    async fn keys_answer(
        &self,
        user: &UserId,
        device: Option<&DeviceId>,
    ) -> Result<crate::agents::trust::KeysAnswer, AgentMatrixError> {
        use matrix_sdk::ruma::api::client::keys::get_keys;
        let mut request = get_keys::v3::Request::new();
        request.timeout = Some(std::time::Duration::from_secs(10));
        request.device_keys = std::collections::BTreeMap::from([(
            user.to_owned(),
            device.map(ToOwned::to_owned).into_iter().collect(),
        )]);
        let response = self.client.send(request).await.map_err(from_http)?;
        let value = |raw: &serde_json::value::RawValue| serde_json::from_str(raw.get()).ok();
        Ok(crate::agents::trust::KeysAnswer {
            failed: !response.failures.is_empty(),
            master_key: response
                .master_keys
                .get(user)
                .and_then(|raw| value(raw.json())),
            self_signing_key: response
                .self_signing_keys
                .get(user)
                .and_then(|raw| value(raw.json())),
            device: device.and_then(|device| {
                response
                    .device_keys
                    .get(user)
                    .and_then(|devices| devices.get(device))
                    .and_then(|raw| value(raw.json()))
            }),
        })
    }

    fn joined(&self, room: &RoomId) -> Result<matrix_sdk::Room, AgentMatrixError> {
        match self.client.get_room(room) {
            Some(room) if room.state() == RoomState::Joined => Ok(room),
            _ => Err(AgentMatrixError::NotFound(format!(
                "this copy has not joined {room}"
            ))),
        }
    }
}

fn raw<T>(value: &Value) -> Result<Raw<T>, AgentMatrixError> {
    serde_json::value::to_raw_value(value)
        .map(Raw::from_json)
        .map_err(|err| AgentMatrixError::Other(format!("could not encode the event: {err}")))
}

/// The `createRoom` request for a typed, encrypted room, inviting `invite`,
/// with `creator` at 100 and `agents` at 50: a session room's power levels
/// from [`events::power_levels`], a control room's from
/// [`events::control_power_levels`]. A `main` session room is a proxy's DM
/// with its person (AD-372), so it is `is_direct`; no other room is.
pub fn create_room_request(
    kind: &RoomKind,
    name: &str,
    invite: Vec<OwnedUserId>,
    creator: &UserId,
    agents: &[OwnedUserId],
) -> Result<create_room::v3::Request, AgentMatrixError> {
    let mut request = create_room::v3::Request::new();
    request.name = Some(name.to_owned());
    request.invite = invite;
    request.preset = Some(create_room::v3::RoomPreset::PrivateChat);
    request.is_direct = *kind == RoomKind::Session(SessionKind::Main);
    let room_type = match kind {
        RoomKind::Session(_) => SESSION_ROOM_TYPE,
        RoomKind::Control => CONTROL_ROOM_TYPE,
    };
    request.creation_content = Some(raw(&json!({ "type": room_type }))?);
    request.initial_state = vec![raw(&json!({
        "type": "m.room.encryption",
        "state_key": "",
        "content": { "algorithm": "m.megolm.v1.aes-sha2" },
    }))?];
    let levels = match kind {
        RoomKind::Session(session) => events::power_levels(*session, creator, agents),
        RoomKind::Control => events::control_power_levels(creator, agents),
    };
    request.power_level_content_override = Some(raw(&levels)?);
    Ok(request)
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::api::error::Error as ClientApiError;
    use matrix_sdk::ruma::api::EndpointError;
    use matrix_sdk::ruma::exports::http;

    use super::*;

    /// The error the SDK hands back for a homeserver answer, built from the
    /// answer's status and body as the SDK builds it.
    fn answer(status: u16, body: &str) -> AgentMatrixError {
        let response = http::Response::builder()
            .status(status)
            .body(body.as_bytes().to_vec())
            .expect("response");
        let error = ClientApiError::from_http_response(response);
        classify(error.error_kind(), false, error.to_string())
    }

    /// The DM `agents init` makes and a room made for a delegated session,
    /// through the one request builder (F1, R46): only the DM is direct, and
    /// only the DM lets the person talk and set the scope in clear. A
    /// person's state write is refused in both (`state_default` 50), which
    /// the live test against Synapse proves.
    #[test]
    fn only_the_main_room_is_a_dm_where_the_person_talks() {
        let nixi = UserId::parse("@nixi:example.org").expect("user");
        let person = UserId::parse("@tgorka:example.org").expect("user");
        let request = |kind: SessionKind| {
            create_room_request(
                &RoomKind::Session(kind),
                "nixi",
                vec![person.clone()],
                &nixi,
                &[],
            )
            .expect("request")
        };
        let content = |request: &create_room::v3::Request| -> Value {
            serde_json::from_str(
                request
                    .power_level_content_override
                    .as_ref()
                    .expect("levels")
                    .json()
                    .get(),
            )
            .expect("json")
        };

        let dm = request(SessionKind::Main);
        assert!(dm.is_direct);
        assert_eq!(dm.invite, vec![person.clone()]);
        let created: Value =
            serde_json::from_str(dm.creation_content.as_ref().expect("create").json().get())
                .expect("json");
        assert_eq!(created["type"], SESSION_ROOM_TYPE);
        let levels = content(&dm);
        assert_eq!(levels["events"]["m.room.message"], 0);
        assert_eq!(levels["events"][events::SCOPE], 0);
        assert_eq!(levels["users"][nixi.as_str()], 100);
        assert_eq!(levels["users_default"], 0);
        assert_eq!(levels["state_default"], 50);

        let delegated = request(SessionKind::Delegated);
        assert!(!delegated.is_direct);
        let levels = content(&delegated);
        assert!(levels["events"].get("m.room.message").is_none());
        assert!(levels["events"].get(events::SCOPE).is_none());
        assert_eq!(levels["events"][events::APPROVAL_DECISION], 0);
        assert_eq!(levels["state_default"], 50);

        for kind in [SessionKind::Conversation, SessionKind::Scheduled] {
            assert!(!request(kind).is_direct, "{kind}");
        }
        let control =
            create_room_request(&RoomKind::Control, "c", vec![person.clone()], &nixi, &[])
                .expect("control");
        assert!(!control.is_direct);
    }

    #[test]
    fn a_429_carries_its_retry_after() {
        assert_eq!(
            answer(
                429,
                r#"{"errcode":"M_LIMIT_EXCEEDED","error":"slow down","retry_after_ms":2000}"#
            ),
            AgentMatrixError::RateLimited {
                retry_after_ms: Some(2000)
            }
        );
        assert_eq!(
            answer(429, r#"{"errcode":"M_LIMIT_EXCEEDED","error":"slow down"}"#),
            AgentMatrixError::RateLimited {
                retry_after_ms: None
            }
        );
    }

    #[test]
    fn forbidden_not_found_and_too_large_are_named() {
        assert!(matches!(
            answer(403, r#"{"errcode":"M_FORBIDDEN","error":"power"}"#),
            AgentMatrixError::Forbidden(_)
        ));
        assert!(matches!(
            answer(404, r#"{"errcode":"M_NOT_FOUND","error":"gone"}"#),
            AgentMatrixError::NotFound(_)
        ));
        assert_eq!(
            answer(413, r#"{"errcode":"M_TOO_LARGE","error":"big"}"#),
            AgentMatrixError::TooLarge
        );
        assert!(matches!(
            answer(500, r#"{"errcode":"M_UNKNOWN","error":"x"}"#),
            AgentMatrixError::Other(_)
        ));
        assert!(matches!(
            classify(None, true, "connection refused".to_owned()),
            AgentMatrixError::Network(_)
        ));
    }

    fn consumed_at(n: usize) -> Value {
        json!({
            "type": events::APPROVAL_CONSUMED,
            "state_key": "A",
            "event_id": format!("$c{n}:example.org"),
            "sender": "@nixi:example.org",
            "origin_server_ts": 1,
            "content": {"host": format!("h{n}")},
        })
    }

    /// R93P-14: a read that spends its pages with history left says it is
    /// incomplete — it is no proof that nothing was consumed — while what
    /// it found first is still the first.
    #[tokio::test]
    async fn a_forward_read_past_its_bound_says_it_is_incomplete() {
        let endless = |token: Option<String>| async move {
            let at: usize = token.as_deref().map_or(0, |t| t.parse().unwrap_or(0));
            let events = if at == 3 {
                vec![consumed_at(at)]
            } else {
                Vec::new()
            };
            Ok::<_, AgentMatrixError>((events, Some((at + 1).to_string())))
        };
        let read = read_forward(None, endless, events::APPROVAL_CONSUMED, "A")
            .await
            .expect("read");
        assert!(!read.complete);
        assert_eq!(read.found.len(), 1);
        assert_eq!(read.found[0].event_id.as_str(), "$c3:example.org");

        let none = |token: Option<String>| async move {
            let at: usize = token.as_deref().map_or(0, |t| t.parse().unwrap_or(0));
            Ok::<_, AgentMatrixError>((Vec::new(), Some((at + 1).to_string())))
        };
        let read = read_forward(None, none, events::APPROVAL_CONSUMED, "A")
            .await
            .expect("read");
        assert_eq!((read.found.len(), read.complete), (0, false));
    }

    /// An empty page with a next token is not the room's end: a filtered
    /// read goes on past it to the match; no next token is the end.
    #[tokio::test]
    async fn empty_pages_do_not_end_a_forward_read() {
        let pages = |token: Option<String>| async move {
            let at: usize = token.as_deref().map_or(0, |t| t.parse().unwrap_or(0));
            let page = match at {
                0..=9 => (Vec::new(), Some((at + 1).to_string())),
                10 => (
                    vec![consumed_at(10), consumed_at(11)],
                    Some("11".to_owned()),
                ),
                _ => (Vec::new(), None),
            };
            Ok::<_, AgentMatrixError>(page)
        };
        let read = read_forward(None, pages, events::APPROVAL_CONSUMED, "A")
            .await
            .expect("read");
        assert!(read.complete);
        let ids: Vec<&str> = read.found.iter().map(|s| s.event_id.as_str()).collect();
        assert_eq!(ids, ["$c10:example.org", "$c11:example.org"]);
    }
}
