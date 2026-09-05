//! One conversation, one identity — the Hermes side of it (Epic 63, Story
//! 63.6, FR-423, FR-424, AD-176, AD-181).
//!
//! # The problem this module is the answer to
//!
//! A conversation held on the phone was invisible on the Mac, and the reason
//! was not that Hermes forgets: `GET /api/sessions` is a real, paginated,
//! `last_active`-ordered list and `GET /api/sessions/{id}/messages` a real
//! history read. The reason was that keeper never told Hermes which session a
//! turn belonged to, so Hermes derived one per client from a fingerprint of
//! the system prompt plus the first user message — and two devices minted two
//! sessions for one conversation.
//!
//! AD-176 fixes the identity: **the Hermes session id is the conversation's
//! cross-device identity.** keeper mints it on the first turn, stores it on
//! its own row ([`crate::bots::session::BotSession::remote_session_id`]),
//! sends it as [`SESSION_HEADER`] on every turn, and a second device that
//! reads the gateway's list adopts a local row carrying the same id
//! ([`reconcile`]). From then on both devices continue one session.
//!
//! # What this module reads, and what it refuses to invent
//!
//! Every route below was verified against `NousResearch/hermes-agent` at
//! `main` on 2026-09-03, and nothing else is called:
//!
//! * `GET /v1/capabilities` — feature flags, among them `session_*` and
//!   `session_continuity_header`. [`probe_capabilities`] reads it once per
//!   provider and the shell caches the answer ([`CapabilityCache`]).
//! * `GET /api/sessions` — `limit` (default 50, max 200), `offset`, `source`,
//!   `title`, `include_hidden` (honoured only beside `title`), ordered by
//!   `last_active`; `{object:"list", data:[…], limit, offset, has_more}`.
//! * `GET /api/sessions/{id}/messages` — `limit` (default and max 500),
//!   `offset`, `order=oldest|latest`.
//! * The continuity header on the OpenAI-compatible path,
//!   `X-Hermes-Session-Id`.
//!
//! # Absence, never a broken control
//!
//! An endpoint that answers nothing on `/v1/capabilities` — an older Hermes,
//! or Ollama, which has no server-side conversation at all — gets
//! [`SessionCapabilities::NONE`], and everything then behaves exactly as it
//! did before this story: no header, no list read, the local row is the
//! record. No error reaches the user for a feature the endpoint simply lacks
//! (AD-27's spirit). Ollama is not even asked: its quirk row says `No`.
//!
//! # The hidden "Bot Chat" row
//!
//! Hermes lists a hidden session only when the caller names its title *and*
//! passes `include_hidden=true`, so a plain page walk would silently miss the
//! canonical [`HIDDEN_BOT_CHAT_TITLE`] session. [`list_sessions`] therefore
//! makes that second read every time and merges it by id.

use std::collections::{HashMap, HashSet};

use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use serde::Deserialize;
use serde_json::Value;

use crate::bots::error::BotsError;
use crate::bots::http;
use crate::bots::quirks::{quirks, Support};
use crate::bots::session::{self, BotMessage, BotSession};
use crate::bots::Endpoint;
use crate::error::CoreError;
use crate::vm::BotTranscriptSource;

/// The request header that names the server-side session a turn continues.
pub const SESSION_HEADER: &str = "X-Hermes-Session-Id";

/// The route that says what the gateway can do.
pub const CAPABILITIES_PATH: &str = "/v1/capabilities";

/// The session list.
pub const SESSIONS_PATH: &str = "/api/sessions";

/// The title of the hidden canonical session the gateway keeps for its bot
/// chat, listed only when asked for by name with `include_hidden=true`.
pub const HIDDEN_BOT_CHAT_TITLE: &str = "Bot Chat";

/// The largest page the list route serves.
pub const REMOTE_SESSION_PAGE: usize = 200;

/// How many list pages one refresh will walk. A thousand sessions is far past
/// what a conversation list renders, and a gateway that keeps saying
/// `has_more` past that is not one keeper should keep paging on a list read.
pub const MAX_SESSION_PAGES: usize = 5;

/// The largest page the messages route serves — its default and its maximum.
pub const REMOTE_MESSAGE_PAGE: usize = 500;

/// How many message pages one open will read. Two thousand rows is a
/// transcript no pane scrolls; a longer one is read at step granularity by
/// Story 63.7.
pub const MAX_MESSAGE_PAGES: usize = 4;

/// The flag that means the continuity header is honoured.
const CONTINUITY_FLAG: &str = "session_continuity_header";

/// The prefix every session-API flag carries.
const SESSION_FLAG_PREFIX: &str = "session_";

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

/// What one endpoint said it can do about sessions.
///
/// Two booleans and not a `Support` tri-state, because by the time this is
/// built the endpoint has been asked: an answer keeper could not read is
/// [`SessionCapabilities::NONE`], which is "behave as before" and not a
/// claim about the gateway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionCapabilities {
    /// `X-Hermes-Session-Id` on a chat request names the session to continue.
    pub continuity_header: bool,
    /// `GET /api/sessions` and `GET /api/sessions/{id}/messages` are there.
    pub session_api: bool,
}

impl SessionCapabilities {
    /// Today's behaviour: no header, no remote list, the local row is the
    /// record.
    pub const NONE: Self = Self {
        continuity_header: false,
        session_api: false,
    };

    /// Whether anything at all changes on this endpoint.
    pub fn any(self) -> bool {
        self.continuity_header || self.session_api
    }
}

/// Read the flags out of a `/v1/capabilities` body, however it spells them.
///
/// Lenient on purpose: the body is a feature-flag document whose shape the
/// research recorded as "advertises feature flags including `session_*` and
/// `session_continuity_header`", so a flag counts when it appears as a string
/// in any array, or as a key whose value is `true`, at any depth. A flag set to
/// `false` is an endpoint saying no, and is not counted.
pub fn parse_capabilities(body: &Value) -> SessionCapabilities {
    let mut flags = Vec::new();
    collect_flags(body, &mut flags);
    let continuity_header = flags.contains(&CONTINUITY_FLAG);
    let session_api = flags
        .iter()
        .any(|flag| *flag != CONTINUITY_FLAG && flag.starts_with(SESSION_FLAG_PREFIX));
    SessionCapabilities {
        continuity_header,
        session_api,
    }
}

fn collect_flags<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::Array(items) => {
            for item in items {
                match item {
                    Value::String(flag) => out.push(flag.as_str()),
                    other => collect_flags(other, out),
                }
            }
        }
        Value::Object(map) => {
            for (key, inner) in map {
                match inner {
                    Value::Bool(true) => out.push(key.as_str()),
                    Value::Bool(false) => {}
                    other => collect_flags(other, out),
                }
            }
        }
        _ => {}
    }
}

/// Ask one endpoint what it can do about sessions.
///
/// Never an `Err`, and never a sentence for the user: an endpoint that has no
/// `/v1/capabilities`, answers a non-success status, or answers something that
/// is not JSON is an endpoint without the feature, and the answer is
/// [`SessionCapabilities::NONE`]. An Ollama endpoint is not asked at all — its
/// quirk row already says it keeps no server-side conversation.
pub async fn probe_capabilities(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> SessionCapabilities {
    if quirks(endpoint.kind).server_sessions == Support::No {
        return SessionCapabilities::NONE;
    }
    let request = match http::authorize(
        client.get(endpoint.url(CAPABILITIES_PATH)),
        endpoint.token.as_deref(),
    ) {
        Ok(request) => request,
        Err(error) => {
            tracing::debug!(%error, "bots: capabilities not read");
            return SessionCapabilities::NONE;
        }
    };
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::debug!(error = %BotsError::transport(error), "bots: capabilities not read");
            return SessionCapabilities::NONE;
        }
    };
    if !response.status().is_success() {
        tracing::debug!(
            status = response.status().as_u16(),
            "bots: no capabilities route"
        );
        return SessionCapabilities::NONE;
    }
    match json_body::<Value>(response, "GET /v1/capabilities").await {
        Ok(body) => parse_capabilities(&body),
        Err(error) => {
            tracing::debug!(%error, "bots: capabilities body not read");
            SessionCapabilities::NONE
        }
    }
}

/// What the shell remembers per provider between probes.
///
/// Keyed by provider id **and** base URL, so an edit that points a provider at
/// a different gateway misses the cache and re-probes rather than carrying the
/// old gateway's answer to the new one. A pure data structure: the shell owns
/// the instance and the lock around it, and this decides only what a key is.
#[derive(Debug, Default)]
pub struct CapabilityCache {
    entries: HashMap<String, SessionCapabilities>,
}

impl CapabilityCache {
    fn key(provider_id: &str, base_url: &str) -> String {
        format!("{provider_id}\n{base_url}")
    }

    /// The remembered answer, if this exact provider and URL were probed.
    pub fn get(&self, provider_id: &str, base_url: &str) -> Option<SessionCapabilities> {
        self.entries.get(&Self::key(provider_id, base_url)).copied()
    }

    /// Remember a probe's answer.
    pub fn remember(&mut self, provider_id: &str, base_url: &str, caps: SessionCapabilities) {
        self.entries.insert(Self::key(provider_id, base_url), caps);
    }

    /// Forget every answer for one provider — on edit or removal.
    pub fn forget(&mut self, provider_id: &str) {
        let prefix = format!("{provider_id}\n");
        self.entries.retain(|key, _| !key.starts_with(&prefix));
    }
}

// ---------------------------------------------------------------------------
// The two decisions a turn and an open make
// ---------------------------------------------------------------------------

/// Which session id this turn sends, if any (AD-176).
///
/// `Some` only where the endpoint honours the continuity header; then the id
/// the row already holds, or a freshly minted one on a conversation's first
/// turn. The mint is a closure so the store can be written before the request
/// goes out — the id must be on the row before the far side has heard it, or
/// a crash between the two would leave a session keeper cannot find again.
pub fn continuity_id(
    caps: SessionCapabilities,
    held: Option<&str>,
    mint: impl FnOnce() -> String,
) -> Option<String> {
    if !caps.continuity_header {
        return None;
    }
    Some(held.map_or_else(mint, str::to_owned))
}

/// Where a conversation's transcript is read from (AD-181).
///
/// Remote when the endpoint has a session API **and** the row names a session
/// there; local otherwise. A row with a remote id on an endpoint that has since
/// lost its session API — a downgraded gateway, a provider edited onto an
/// older host — reads locally, because the id names nothing keeper can reach.
pub fn transcript_source(caps: SessionCapabilities, session: &BotSession) -> BotTranscriptSource {
    if caps.session_api && session.remote_session_id.is_some() {
        BotTranscriptSource::Remote
    } else {
        BotTranscriptSource::Local
    }
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/// One session as the gateway lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSession {
    /// The gateway's id — the identity.
    pub id: String,
    /// Its title, where the gateway has one.
    pub title: Option<String>,
    /// Which door wrote it (`api`, `cli`, a bridge), where stated.
    pub source: Option<String>,
    /// When it started, ms since the Unix epoch, where stated.
    pub started_ms: Option<i64>,
    /// When it was last active, ms since the Unix epoch, where stated.
    pub last_active_ms: Option<i64>,
    /// How many messages it holds, where stated.
    pub message_count: Option<i64>,
    /// The gateway's preview line, where it sent one.
    pub preview: Option<String>,
    /// Whether the gateway hides it from its own default list.
    pub hidden: bool,
    /// Whether the gateway archived it.
    pub archived: bool,
}

/// Every session the gateway lists for this endpoint, including the hidden
/// canonical bot-chat row, newest activity first.
///
/// Two reads, merged by id: the page walk, and the named read that is the
/// only way the hidden row is served. Pages stop at `has_more == false` or
/// [`MAX_SESSION_PAGES`], whichever first.
pub async fn list_sessions(
    client: &reqwest::Client,
    endpoint: &Endpoint,
) -> Result<Vec<RemoteSession>, BotsError> {
    let mut out: Vec<RemoteSession> = Vec::new();
    let mut offset = 0usize;
    for _ in 0..MAX_SESSION_PAGES {
        let path = format!("{SESSIONS_PATH}?limit={REMOTE_SESSION_PAGE}&offset={offset}");
        let page: ListBody<SessionRow> =
            get_json(client, endpoint, &path, "GET /api/sessions").await?;
        let count = page.data.len();
        out.extend(page.data.into_iter().filter_map(SessionRow::into_session));
        offset += count;
        if !page.has_more || count == 0 {
            break;
        }
    }
    let hidden_path = format!(
        "{SESSIONS_PATH}?title={}&include_hidden=true&limit={REMOTE_SESSION_PAGE}",
        utf8_percent_encode(HIDDEN_BOT_CHAT_TITLE, NON_ALPHANUMERIC)
    );
    let hidden: ListBody<SessionRow> =
        get_json(client, endpoint, &hidden_path, "GET /api/sessions").await?;
    for row in hidden.data.into_iter().filter_map(SessionRow::into_session) {
        if !out.iter().any(|seen| seen.id == row.id) {
            out.push(row);
        }
    }
    Ok(out)
}

/// One message as the gateway serves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteMessage {
    /// The gateway's message id.
    pub id: String,
    /// `user` | `assistant` | `system` | `tool`, verbatim.
    pub role: String,
    /// The text; empty where the gateway sent none.
    pub content: String,
    /// When it was written, ms since the Unix epoch, where stated.
    pub timestamp_ms: Option<i64>,
    /// The gateway's token count for this row, where stated.
    pub token_count: Option<i64>,
    /// Why the model stopped, in the gateway's word.
    pub finish_reason: Option<String>,
    /// How many tool calls this row carries.
    pub tool_call_count: i64,
}

/// The whole history of one remote session, oldest first.
///
/// Pages of [`REMOTE_MESSAGE_PAGE`] until a short page or
/// [`MAX_MESSAGE_PAGES`], whichever first.
pub async fn fetch_messages(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    remote_session_id: &str,
) -> Result<Vec<RemoteMessage>, BotsError> {
    let encoded = utf8_percent_encode(remote_session_id, NON_ALPHANUMERIC);
    let mut out = Vec::new();
    let mut offset = 0usize;
    for _ in 0..MAX_MESSAGE_PAGES {
        let path = format!(
            "{SESSIONS_PATH}/{encoded}/messages?limit={REMOTE_MESSAGE_PAGE}&offset={offset}&order=oldest"
        );
        let page: MessagesBody =
            get_json(client, endpoint, &path, "GET /api/sessions/{id}/messages").await?;
        let rows = page.rows();
        let count = rows.len();
        out.extend(rows.into_iter().filter_map(MessageRow::into_message));
        offset += count;
        if count < REMOTE_MESSAGE_PAGE {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Reconciling the list with keeper.db
// ---------------------------------------------------------------------------

/// What one reconcile pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Remote sessions no local row named, now adopted as rows.
    pub adopted: usize,
    /// Local rows whose remote activity changed since the last pass.
    pub refreshed: usize,
}

/// Fold the gateway's list for one bot into `keeper.db` (AD-176, AD-181).
///
/// A remote session already named by a local row gets that row's
/// `remote_last_active_ms` and `remote_source` refreshed; one no row names is
/// **adopted** — a new local row carrying the gateway's id, so it is listable,
/// openable and, above all, continuable here under the same identity. A local
/// row the gateway no longer lists is left alone: keeper's copy is still a
/// record of something that happened. A session the person deleted here is
/// never adopted back ([`session::list_dismissed_remote_ids`]).
///
/// The join is one read of every row of this provider that carries a remote
/// id, so a two-hundred-row page costs two hundred comparisons and not two
/// hundred connections.
pub fn reconcile(
    data_dir: &std::path::Path,
    provider_id: &str,
    bot_id: &str,
    remote: &[RemoteSession],
    now_ms: i64,
) -> Result<Reconciled, CoreError> {
    let local = session::list_sessions_with_remote_id(data_dir, provider_id)?;
    // A set, not a scan: this runs once per remote row on every reconcile, and the
    // tombstone list only ever grows.
    let dismissed: HashSet<String> = session::list_dismissed_remote_ids(data_dir, provider_id)?
        .into_iter()
        .collect();
    let by_remote: HashMap<&str, &BotSession> = local
        .iter()
        .filter_map(|row| row.remote_session_id.as_deref().map(|id| (id, row)))
        .collect();
    let mut done = Reconciled::default();
    for found in remote {
        if dismissed.contains(&found.id) {
            continue;
        }
        match by_remote.get(found.id.as_str()) {
            Some(row) => {
                if row.remote_last_active_ms != found.last_active_ms
                    || row.remote_source != found.source
                {
                    session::set_session_remote_activity(
                        data_dir,
                        &row.id,
                        found.last_active_ms,
                        found.source.as_deref(),
                    )?;
                    done.refreshed += 1;
                }
            }
            None => {
                session::insert_session(data_dir, &adopted(provider_id, bot_id, found, now_ms))?;
                done.adopted += 1;
            }
        }
    }
    Ok(done)
}

/// The local row a remote session becomes when this device first sees it.
///
/// The title is the gateway's where it has one, run through
/// [`session::mint_title`] so the list draws it under the same four rules
/// every local title obeys; failing that the preview line; failing that
/// [`session::UNTITLED_SESSION`]. The clock is the gateway's where it stated
/// one and `now_ms` where it did not — a row with no time would sort as if
/// nothing had ever happened to it.
pub fn adopted(provider_id: &str, bot_id: &str, found: &RemoteSession, now_ms: i64) -> BotSession {
    let title = found
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .or(found.preview.as_deref())
        .map(session::mint_title)
        .unwrap_or_else(|| session::UNTITLED_SESSION.to_owned());
    let updated_ms = found.last_active_ms.unwrap_or(now_ms);
    BotSession {
        id: ulid::Ulid::new().to_string(),
        bot_id: bot_id.to_owned(),
        provider_id: provider_id.to_owned(),
        title,
        created_ms: found.started_ms.unwrap_or(updated_ms),
        updated_ms,
        archived: found.archived,
        remote_session_id: Some(found.id.clone()),
        remote_last_active_ms: found.last_active_ms,
        remote_source: found.source.clone(),
    }
}

/// The gateway's history as rows of `local_session` (AD-181).
///
/// Every number the gateway did not state stays absent, and the role and the
/// finish reason are carried verbatim, for `BotMessageVm`'s reason. `partial`
/// is always false: a row the gateway serves is a row it finished writing.
pub fn compose_messages(
    remote: &[RemoteMessage],
    local_session_id: &str,
    provider_id: &str,
    fallback_ms: i64,
) -> Vec<BotMessage> {
    remote
        .iter()
        .enumerate()
        .map(|(index, row)| BotMessage {
            id: row.id.clone(),
            session_id: local_session_id.to_owned(),
            seq: i64::try_from(index).unwrap_or(i64::MAX),
            role: row.role.clone(),
            content: row.content.clone(),
            model: None,
            provider_id: Some(provider_id.to_owned()),
            prompt_tokens: None,
            completion_tokens: None,
            total_tokens: row.token_count,
            ttft_ms: None,
            duration_ms: None,
            finish_reason: row.finish_reason.clone(),
            request_id: None,
            tool_call_count: row.tool_call_count,
            partial: false,
            created_ms: row.timestamp_ms.unwrap_or(fallback_ms),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Wire shapes and plumbing
// ---------------------------------------------------------------------------

/// `{object:"list", data:[…], limit, offset, has_more}`.
#[derive(Debug, Deserialize)]
struct ListBody<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
    #[serde(default)]
    has_more: bool,
}

/// One `/api/sessions` row. Every field but `id` is optional and a row without
/// an id names nothing keeper could continue, so it is dropped.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SessionRow {
    id: Option<String>,
    source: Option<String>,
    title: Option<String>,
    started_at: Option<Value>,
    last_active: Option<Value>,
    message_count: Option<i64>,
    preview: Option<String>,
    hidden: Option<bool>,
    archived: Option<bool>,
}

impl SessionRow {
    fn into_session(self) -> Option<RemoteSession> {
        let id = self.id.filter(|id| !id.is_empty())?;
        Some(RemoteSession {
            id,
            title: self.title.filter(|title| !title.is_empty()),
            source: self.source.filter(|source| !source.is_empty()),
            started_ms: epoch_ms(self.started_at.as_ref()),
            last_active_ms: epoch_ms(self.last_active.as_ref()),
            message_count: self.message_count,
            preview: self.preview.filter(|preview| !preview.is_empty()),
            hidden: self.hidden.unwrap_or(false),
            archived: self.archived.unwrap_or(false),
        })
    }
}

/// The messages route's body: a list object, a `messages` array, or a bare
/// array — one parser for the three spellings a history read may come in.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MessagesBody {
    Bare(Vec<MessageRow>),
    Listed {
        #[serde(default = "Vec::new")]
        data: Vec<MessageRow>,
        #[serde(default = "Vec::new")]
        messages: Vec<MessageRow>,
    },
}

impl MessagesBody {
    fn rows(self) -> Vec<MessageRow> {
        match self {
            MessagesBody::Bare(rows) => rows,
            MessagesBody::Listed { data, messages } => {
                if data.is_empty() {
                    messages
                } else {
                    data
                }
            }
        }
    }
}

/// One `/api/sessions/{id}/messages` row.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MessageRow {
    id: Option<Value>,
    role: Option<String>,
    content: Option<Value>,
    tool_calls: Option<Value>,
    timestamp: Option<Value>,
    token_count: Option<i64>,
    finish_reason: Option<String>,
}

impl MessageRow {
    fn into_message(self) -> Option<RemoteMessage> {
        let id = match self.id? {
            Value::String(id) => id,
            Value::Number(number) => number.to_string(),
            _ => return None,
        };
        let content = match self.content {
            Some(Value::String(text)) => text,
            Some(Value::Array(parts)) => parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            _ => String::new(),
        };
        let tool_call_count = match &self.tool_calls {
            Some(Value::Array(calls)) => i64::try_from(calls.len()).unwrap_or(i64::MAX),
            _ => 0,
        };
        Some(RemoteMessage {
            id,
            role: self.role.unwrap_or_default(),
            content,
            timestamp_ms: epoch_ms(self.timestamp.as_ref()),
            token_count: self.token_count,
            finish_reason: self.finish_reason,
            tool_call_count,
        })
    }
}

/// A gateway timestamp as ms since the Unix epoch.
///
/// A number — or a numeric string — is read as seconds when it is small enough
/// to be seconds and as milliseconds otherwise; the boundary (`1e11`) is the
/// year 5138 in seconds and 1973 in milliseconds, so no honest clock sits on
/// the wrong side of it. Anything else is `None`: an unreadable time is an
/// absent time, never a guessed one.
fn epoch_ms(value: Option<&Value>) -> Option<i64> {
    let number = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    let ms = if number < 1e11 {
        number * 1000.0
    } else {
        number
    };
    Some(ms.round() as i64)
}

/// `GET` a route and parse its bounded JSON body.
async fn get_json<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    endpoint: &Endpoint,
    path: &str,
    label: &str,
) -> Result<T, BotsError> {
    let request = http::authorize(client.get(endpoint.url(path)), endpoint.token.as_deref())?;
    let response = request.send().await.map_err(BotsError::transport)?;
    let status = response.status();
    if !status.is_success() {
        let body = bounded_body(response, label).await.unwrap_or_default();
        let detail = match String::from_utf8_lossy(&body).trim() {
            "" => "no body".to_owned(),
            text => text.to_owned(),
        };
        return Err(BotsError::status(status.as_u16(), label, &detail));
    }
    json_body(response, label).await
}

/// Read a body in bounded chunks — `discover::bounded_body`'s rule, for its
/// reason: never `Response::text` on a URL the user typed.
async fn bounded_body(mut response: reqwest::Response, label: &str) -> Result<Vec<u8>, BotsError> {
    let mut body = Vec::new();
    loop {
        match response.chunk().await.map_err(BotsError::transport)? {
            None => return Ok(body),
            Some(chunk) => {
                if body.len() + chunk.len() > crate::bots::discover::MAX_DISCOVERY_BODY {
                    return Err(BotsError::Protocol {
                        detail: format!(
                            "{label} answered with more than {} bytes, which no honest session \
                             list needs",
                            crate::bots::discover::MAX_DISCOVERY_BODY
                        ),
                    });
                }
                body.extend_from_slice(&chunk);
            }
        }
    }
}

async fn json_body<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
    label: &str,
) -> Result<T, BotsError> {
    let body = bounded_body(response, label).await?;
    serde_json::from_slice(&body).map_err(|err| BotsError::Parse {
        endpoint: label.to_owned(),
        detail: err.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flags_are_read_from_arrays_and_from_true_keys_at_any_depth() {
        let listed = json!({ "features": ["session_list", "session_continuity_header"] });
        assert_eq!(
            parse_capabilities(&listed),
            SessionCapabilities {
                continuity_header: true,
                session_api: true
            }
        );
        let keyed = json!({ "capabilities": { "sessions": { "session_messages": true,
            "session_continuity_header": false } } });
        assert_eq!(
            parse_capabilities(&keyed),
            SessionCapabilities {
                continuity_header: false,
                session_api: true
            }
        );
    }

    #[test]
    fn a_body_with_no_session_flags_is_none() {
        assert_eq!(
            parse_capabilities(&json!({ "features": ["runs", "tool_progress"] })),
            SessionCapabilities::NONE
        );
        assert_eq!(
            parse_capabilities(&json!("nonsense")),
            SessionCapabilities::NONE
        );
    }

    #[test]
    fn the_continuity_id_is_sent_only_where_the_header_is_honoured() {
        let on = SessionCapabilities {
            continuity_header: true,
            session_api: false,
        };
        assert_eq!(
            continuity_id(on, Some("held"), || "minted".to_owned()),
            Some("held".to_owned())
        );
        assert_eq!(
            continuity_id(on, None, || "minted".to_owned()),
            Some("minted".to_owned())
        );
        assert_eq!(
            continuity_id(SessionCapabilities::NONE, Some("held"), || "minted"
                .to_owned()),
            None,
            "an endpoint that never said it honours the header gets no header"
        );
    }

    #[test]
    fn the_transcript_is_remote_only_with_both_an_api_and_an_id() {
        let api = SessionCapabilities {
            continuity_header: true,
            session_api: true,
        };
        let mut row = adopted(
            "prov",
            "bot",
            &RemoteSession {
                id: "h-1".to_owned(),
                title: None,
                source: None,
                started_ms: None,
                last_active_ms: None,
                message_count: None,
                preview: None,
                hidden: false,
                archived: false,
            },
            1_700_000_000_000,
        );
        assert_eq!(transcript_source(api, &row), BotTranscriptSource::Remote);
        assert_eq!(
            transcript_source(SessionCapabilities::NONE, &row),
            BotTranscriptSource::Local
        );
        row.remote_session_id = None;
        assert_eq!(transcript_source(api, &row), BotTranscriptSource::Local);
    }

    #[test]
    fn an_adopted_row_takes_the_gateway_title_then_the_preview_then_the_placeholder() {
        let base = RemoteSession {
            id: "h".to_owned(),
            title: Some("  Weekly   plan 🚀 ".to_owned()),
            source: Some("api".to_owned()),
            started_ms: Some(1_000),
            last_active_ms: Some(2_000),
            message_count: Some(3),
            preview: Some("what is in my drive".to_owned()),
            hidden: true,
            archived: false,
        };
        let titled = adopted("prov", "bot", &base, 9_000);
        assert_eq!(titled.title, "Weekly plan");
        assert_eq!(titled.created_ms, 1_000);
        assert_eq!(titled.updated_ms, 2_000);
        assert_eq!(titled.remote_session_id.as_deref(), Some("h"));
        assert_eq!(titled.remote_last_active_ms, Some(2_000));
        assert_eq!(titled.remote_source.as_deref(), Some("api"));

        let previewed = adopted(
            "prov",
            "bot",
            &RemoteSession {
                title: Some("   ".to_owned()),
                ..base.clone()
            },
            9_000,
        );
        assert_eq!(previewed.title, "what is in my drive");

        let bare = adopted(
            "prov",
            "bot",
            &RemoteSession {
                title: None,
                preview: None,
                started_ms: None,
                last_active_ms: None,
                ..base
            },
            9_000,
        );
        assert_eq!(bare.title, session::UNTITLED_SESSION);
        assert_eq!((bare.created_ms, bare.updated_ms), (9_000, 9_000));
    }

    #[test]
    fn gateway_timestamps_are_read_as_seconds_or_milliseconds() {
        assert_eq!(
            epoch_ms(Some(&json!(1_700_000_000.5))),
            Some(1_700_000_000_500)
        );
        assert_eq!(
            epoch_ms(Some(&json!(1_700_000_000_000_i64))),
            Some(1_700_000_000_000)
        );
        assert_eq!(
            epoch_ms(Some(&json!("1700000000"))),
            Some(1_700_000_000_000)
        );
        assert_eq!(epoch_ms(Some(&json!("2026-09-03T10:00:00Z"))), None);
        assert_eq!(epoch_ms(Some(&json!(0))), None);
        assert_eq!(epoch_ms(None), None);
    }

    #[test]
    fn a_message_row_reads_the_shapes_the_gateway_may_send() {
        let row: MessageRow = serde_json::from_value(json!({
            "id": 7, "role": "assistant",
            "content": [{"type": "text", "text": "hel"}, {"type": "text", "text": "lo"}],
            "tool_calls": [{}, {}], "timestamp": 1_700_000_000, "token_count": 12,
            "finish_reason": "stop"
        }))
        .expect("row");
        let message = row.into_message().expect("message");
        assert_eq!(message.id, "7");
        assert_eq!(message.content, "hello");
        assert_eq!(message.tool_call_count, 2);
        assert_eq!(message.timestamp_ms, Some(1_700_000_000_000));
        assert_eq!(message.token_count, Some(12));

        let bodies: MessagesBody =
            serde_json::from_value(json!({ "data": [{ "id": "a" }] })).expect("listed");
        assert_eq!(bodies.rows().len(), 1);
        let bare: MessagesBody =
            serde_json::from_value(json!([{ "id": "a" }, { "id": "b" }])).expect("bare");
        assert_eq!(bare.rows().len(), 2);
    }

    #[test]
    fn the_cache_is_keyed_by_provider_and_url_and_forgets_by_provider() {
        let mut cache = CapabilityCache::default();
        let caps = SessionCapabilities {
            continuity_header: true,
            session_api: true,
        };
        cache.remember("p1", "http://a", caps);
        assert_eq!(cache.get("p1", "http://a"), Some(caps));
        assert_eq!(cache.get("p1", "http://b"), None, "an edited URL re-probes");
        cache.forget("p1");
        assert_eq!(cache.get("p1", "http://a"), None);
    }
}
