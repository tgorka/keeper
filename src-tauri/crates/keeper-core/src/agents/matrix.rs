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

use std::path::{Path, PathBuf};
use std::time::Duration;

use matrix_sdk::config::{RequestConfig, SyncSettings};
use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState;
use matrix_sdk::ruma::api::client::filter::{FilterDefinition, RoomEventFilter, RoomFilter};
use matrix_sdk::ruma::api::client::room::create_room;
use matrix_sdk::ruma::api::client::state::{get_state_events, send_state_event};
use matrix_sdk::ruma::api::client::sync::sync_events;
use matrix_sdk::ruma::api::error::{ErrorKind, RetryAfter};
use matrix_sdk::ruma::events::StateEventType;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{
    MilliSecondsSinceUnixEpoch, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, TransactionId,
    UserId,
};
use matrix_sdk::{Client, RoomState};
use serde_json::{json, Value};

use crate::agents::events::{self, CONTROL_ROOM_TYPE, SESSION_ROOM_TYPE};
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

/// A fresh store passphrase for a new copy.
pub fn new_store_passphrase() -> String {
    crate::auth::generate_store_passphrase()
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

    /// Create a typed, encrypted room, inviting `invite`, with `agents` at 50:
    /// a session room's power levels from [`events::power_levels`], a control
    /// room's from [`events::control_power_levels`].
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
        let mut request = create_room::v3::Request::new();
        request.name = Some(name.to_owned());
        request.invite = invite;
        request.preset = Some(create_room::v3::RoomPreset::PrivateChat);
        let room_type = match &kind {
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
            RoomKind::Session(session) => events::power_levels(session, &me, agents),
            RoomKind::Control => events::control_power_levels(&me, agents),
        };
        request.power_level_content_override = Some(raw(&levels)?);
        let room = self.client.create_room(request).await.map_err(from_sdk)?;
        Ok(room.room_id().to_owned())
    }

    pub async fn invite(&self, room: &RoomId, user: &UserId) -> Result<(), AgentMatrixError> {
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
}
