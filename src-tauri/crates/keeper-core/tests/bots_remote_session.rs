//! Story 63.6 — one conversation, one identity (AD-176, AD-181) — and Story
//! 63.7, following that conversation from the other device (AD-177).
//!
//! # Why this file stands up a fake Hermes
//!
//! The epic names this story's impure shell in as many words: *a real second
//! HTTP client against one Hermes session — two clients, one session id*. The
//! pure halves (`parse_capabilities`, `continuity_id`, `transcript_source`,
//! `adopted`) have unit tests beside them; what those cannot reach is whether
//! the header actually leaves the socket on both turns, whether the second
//! device's list read really finds the first device's session, and whether
//! the hidden canonical row is asked for the one way Hermes serves it.
//!
//! So the server below is a `tokio::net::TcpListener` speaking HTTP/1.1 by
//! hand, answering the four routes the research verified in the shapes it
//! recorded — `{object:"list", data, limit, offset, has_more}`, the
//! `title=…&include_hidden=true` rule, `X-Hermes-Session-Id` on the chat
//! path — and, crucially, **minting a fingerprint session when no header
//! arrives**, which is the exact Hermes behaviour that put two devices in two
//! sessions. Every request is recorded so a test can assert what left keeper
//! rather than what it meant to send.
//!
//! Two data directories stand for two devices. Nothing is mocked inside the
//! core.
//!
//! Story 63.7's tests drive the same server one flushed step at a time —
//! the user turn, the tool-call block, the tool result, the final text —
//! and read it back the way the shell's `bots_session_follow` does: one
//! history read, `follow::merge`, `follow::decide`. The fake also answers
//! `/v1/runs/{id}/events`, so the claim that keeper never asks for it is a
//! claim about the recorded paths and not about a 404.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::bots::chat::{
    cancellation, stream_chat, ChatEvent, ChatMessage, ChatOptions, ChatRequest, Role,
};
use keeper_core::bots::follow::{self, Follow, FollowSignals};
use keeper_core::bots::http::client;
use keeper_core::bots::remote::{
    self, continuity_id, fetch_messages, list_sessions, probe_capabilities, reconcile,
    transcript_source, SessionCapabilities, HIDDEN_BOT_CHAT_TITLE, SESSION_HEADER,
};
use keeper_core::bots::session::{
    self, delete_session, get_session, insert_session, search_sessions, BotSession, SessionQuery,
};
use keeper_core::bots::{Endpoint, ProviderKind};
use keeper_core::vm::{BotSessionListVm, BotTranscriptSource};
use keeper_core::voice::speech;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

// ---------------------------------------------------------------------------
// The fake Hermes
// ---------------------------------------------------------------------------

/// One recorded request: method, path with query, lower-cased headers, and
/// the body as JSON where it parsed as any — so a test can assert on the
/// messages that left keeper, not only on the headers (Epic 64).
#[derive(Debug, Clone)]
struct Seen {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

impl Seen {
    fn session_header(&self) -> Option<&str> {
        self.headers
            .get(&SESSION_HEADER.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// The `role` and text `content` of every message in the body, in
    /// order.
    fn messages(&self) -> Vec<(String, String)> {
        self.body["messages"]
            .as_array()
            .map(|messages| {
                messages
                    .iter()
                    .map(|m| {
                        (
                            m["role"].as_str().unwrap_or_default().to_owned(),
                            m["content"].as_str().unwrap_or_default().to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// One message as the fake keeps it — the step shape Hermes persists: a
/// tool-call block is an assistant row with `tool_calls` and no finish, a
/// tool result is a `tool` row, the final text an assistant row that
/// finished.
#[derive(Debug, Clone)]
struct FakeMessage {
    role: String,
    content: String,
    tool_calls: Option<Value>,
    finish_reason: Option<String>,
}

fn said(role: &str, content: &str) -> FakeMessage {
    FakeMessage {
        role: role.to_owned(),
        content: content.to_owned(),
        tool_calls: None,
        finish_reason: (role == "assistant").then(|| "stop".to_owned()),
    }
}

/// One session as the fake keeps it.
#[derive(Debug, Clone)]
struct FakeSession {
    title: String,
    source: String,
    hidden: bool,
    last_active: f64,
    messages: Vec<FakeMessage>,
}

#[derive(Debug, Default)]
struct FakeState {
    /// Insertion-ordered, so the list order is deterministic.
    sessions: Vec<(String, FakeSession)>,
    seen: Vec<Seen>,
    /// An older Hermes: no `/v1/capabilities` at all.
    old: bool,
    minted: u32,
}

struct FakeHermes {
    addr: SocketAddr,
    state: Arc<Mutex<FakeState>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for FakeHermes {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeHermes {
    async fn start(state: FakeState) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let state = Arc::new(Mutex::new(state));
        let shared = Arc::clone(&state);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let state = Arc::clone(&shared);
                tokio::spawn(serve(stream, state));
            }
        });
        Self { addr, state, task }
    }

    fn endpoint(&self, bot: Option<&str>) -> Endpoint {
        Endpoint {
            kind: ProviderKind::Hermes,
            base_url: format!("http://{}", self.addr),
            bot: bot.map(str::to_owned),
            token: Some("sk-test-do-not-leak".to_owned()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().expect("fake state")
    }

    fn seen(&self) -> Vec<Seen> {
        self.state().seen.clone()
    }

    fn sessions(&self) -> Vec<(String, FakeSession)> {
        self.state().sessions.clone()
    }

    /// The other device's turn advancing one step: what Hermes persists at
    /// turn start, before a tool round, after it, and at the loop's exit.
    /// This is the whole reason following works at all — the step is on
    /// the history route the moment it is flushed, long before the turn ends.
    fn flush(&self, session_id: &str, step: FakeMessage) {
        let mut state = self.state();
        let session = state
            .sessions
            .iter_mut()
            .find(|(id, _)| id == session_id)
            .map(|(_, s)| s)
            .expect("a session to flush into");
        session.messages.push(step);
        session.last_active = 1_700_000_000.0 + session.messages.len() as f64;
    }
}

fn fake_session(title: &str, hidden: bool, last_active: f64) -> FakeSession {
    FakeSession {
        title: title.to_owned(),
        source: "api".to_owned(),
        hidden,
        last_active,
        messages: vec![
            said("user", &format!("first line of {title}")),
            said("assistant", "noted"),
        ],
    }
}

async fn serve(mut stream: TcpStream, state: Arc<Mutex<FakeState>>) {
    let Some((head, body)) = read_request(&mut stream).await else {
        return;
    };
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or_default().to_owned();
    let headers: BTreeMap<String, String> = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect();
    let seen = Seen {
        method: method.clone(),
        target: target.clone(),
        headers,
        body: serde_json::from_slice(&body).unwrap_or(Value::Null),
    };
    let response = {
        let mut guard = state.lock().expect("fake state");
        guard.seen.push(seen.clone());
        route(&mut guard, &seen, &body)
    };
    let _ = stream.write_all(&response).await;
    let _ = stream.flush().await;
}

/// The path without the profile prefix, and the query as pairs.
fn split_target(target: &str) -> (String, BTreeMap<String, String>) {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let path = match path.strip_prefix("/p/") {
        Some(rest) => match rest.split_once('/') {
            Some((_bot, tail)) => format!("/{tail}"),
            None => "/".to_owned(),
        },
        None => path.to_owned(),
    };
    let query = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (key.to_owned(), percent_decode(value))
        })
        .collect();
    (path, query)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn route(state: &mut FakeState, seen: &Seen, body: &[u8]) -> Vec<u8> {
    let (path, query) = split_target(&seen.target);
    match (seen.method.as_str(), path.as_str()) {
        ("GET", "/v1/capabilities") => {
            if state.old {
                return json_response("404 Not Found", &json!({"detail": "Not Found"}));
            }
            json_response(
                "200 OK",
                &json!({
                    "features": ["runs", "session_list", "session_messages", "session_continuity_header"],
                    "session_continuity_header": SESSION_HEADER
                }),
            )
        }
        ("POST", "/v1/chat/completions") => {
            let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
            let last_user = parsed["messages"]
                .as_array()
                .and_then(|messages| {
                    messages
                        .iter()
                        .rev()
                        .find(|m| m["role"] == "user")
                        .and_then(|m| m["content"].as_str())
                })
                .unwrap_or("")
                .to_owned();
            // The real behaviour under test: no header means Hermes derives a
            // session from a fingerprint of the prompt, one per client.
            let id = match seen.session_header() {
                Some(id) => id.to_owned(),
                None => {
                    state.minted += 1;
                    format!("fingerprint-{}", state.minted)
                }
            };
            let position = match state.sessions.iter().position(|(sid, _)| *sid == id) {
                Some(position) => position,
                None => {
                    state.sessions.push((
                        id.clone(),
                        FakeSession {
                            title: last_user.clone(),
                            source: "api".to_owned(),
                            hidden: false,
                            last_active: 0.0,
                            messages: Vec::new(),
                        },
                    ));
                    state.sessions.len() - 1
                }
            };
            let session = &mut state.sessions[position].1;
            let reply = format!("reply {}", session.messages.len() / 2 + 1);
            session.messages.push(said("user", &last_user));
            session.messages.push(said("assistant", &reply));
            session.last_active = 1_700_000_000.0 + session.messages.len() as f64;
            sse_response(&reply)
        }
        ("GET", "/api/sessions") => {
            let limit: usize = query
                .get("limit")
                .and_then(|v| v.parse().ok())
                .unwrap_or(50)
                .min(200);
            let offset: usize = query
                .get("offset")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let title = query.get("title");
            let include_hidden =
                title.is_some() && query.get("include_hidden").map(String::as_str) == Some("true");
            let mut rows: Vec<&(String, FakeSession)> = state
                .sessions
                .iter()
                .filter(|(_, s)| include_hidden || !s.hidden)
                .filter(|(_, s)| title.is_none_or(|needle| s.title.contains(needle.as_str())))
                .collect();
            rows.sort_by(|a, b| b.1.last_active.total_cmp(&a.1.last_active));
            let total = rows.len();
            let page: Vec<Value> = rows
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|(id, s)| session_row(id, s))
                .collect();
            json_response(
                "200 OK",
                &json!({
                    "object": "list",
                    "data": page,
                    "limit": limit,
                    "offset": offset,
                    "has_more": offset + limit < total
                }),
            )
        }
        ("GET", messages)
            if messages.starts_with("/api/sessions/") && messages.ends_with("/messages") =>
        {
            let id = messages
                .trim_start_matches("/api/sessions/")
                .trim_end_matches("/messages");
            let id = percent_decode(id);
            match state.sessions.iter().find(|(sid, _)| *sid == id) {
                None => json_response("404 Not Found", &json!({"detail": "no such session"})),
                Some((sid, s)) => {
                    let data: Vec<Value> = s
                        .messages
                        .iter()
                        .enumerate()
                        .map(|(index, m)| {
                            json!({
                                "id": index + 1,
                                "session_id": sid,
                                "role": m.role,
                                "content": m.content,
                                "tool_call_id": null,
                                "tool_calls": m.tool_calls,
                                "tool_name": null,
                                "timestamp": 1_700_000_000.0 + index as f64,
                                "token_count": 3,
                                "finish_reason": m.finish_reason,
                                "reasoning": null,
                                "display_kind": "message"
                            })
                        })
                        .collect();
                    json_response(
                        "200 OK",
                        &json!({"object": "list", "data": data, "limit": 500, "offset": 0, "has_more": false}),
                    )
                }
            }
        }
        // The run-events stream: one `asyncio.Queue` per run with a
        // destructive read on the real gateway. The fake answers it so a
        // keeper that asked would get a body rather than a 404 — the test's
        // claim is that the path is never requested, not that it fails.
        ("GET", runs) if runs.starts_with("/v1/runs/") => {
            json_response("200 OK", &json!({"status": "running"}))
        }
        _ => json_response("404 Not Found", &json!({"detail": "Not Found"})),
    }
}

fn session_row(id: &str, s: &FakeSession) -> Value {
    json!({
        "id": id,
        "source": s.source,
        "model": "hermes-agent",
        "title": s.title,
        "started_at": 1_700_000_000.0,
        "ended_at": null,
        "end_reason": null,
        "message_count": s.messages.len(),
        "tool_call_count": 0,
        "parent_session_id": null,
        "last_active": s.last_active,
        "preview": s.messages.first().map(|m| m.content.clone()),
        "pinned": false,
        "archived": false,
        "hidden": s.hidden
    })
}

fn json_response(status: &str, body: &Value) -> Vec<u8> {
    let body = body.to_string();
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn sse_response(reply: &str) -> Vec<u8> {
    let chunk = json!({"id": "chatcmpl-1", "choices": [{"index": 0, "delta": {"content": reply}}]});
    let done = json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
         data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n"
    )
    .into_bytes()
}

/// The request head as text and its body, honouring `Content-Length`.
async fn read_request(stream: &mut TcpStream) -> Option<(String, Vec<u8>)> {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 4096];
    loop {
        let read = stream.read(&mut scratch).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&scratch[..read]);
        let Some(head_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        let content_length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        if buffer.len() >= head_end + 4 + content_length {
            let body = buffer[head_end + 4..head_end + 4 + content_length].to_vec();
            return Some((head, body));
        }
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// A scratch `keeper.db` directory — one per "device".
fn device_dir(label: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-bots-remote-{label}-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

const NOW: i64 = 1_756_000_000_000;
const PROVIDER: &str = "prov-hermes";
const BOT: &str = "bot-1";

fn local_session(id: &str, remote: Option<&str>) -> BotSession {
    BotSession {
        id: id.to_owned(),
        bot_id: BOT.to_owned(),
        provider_id: PROVIDER.to_owned(),
        title: "Started here".to_owned(),
        created_ms: NOW,
        updated_ms: NOW,
        archived: false,
        remote_session_id: remote.map(str::to_owned),
        remote_last_active_ms: None,
        remote_source: None,
    }
}

fn options() -> ChatOptions {
    ChatOptions {
        read_timeout: Duration::from_secs(5),
        max_attempts: 1,
        ..ChatOptions::default()
    }
}

/// One turn from one device: the shell's `adopt_identity` + `stream_chat`,
/// at the core level — the id is written to the row before the request goes
/// out, exactly as the command does.
async fn send_turn(
    fake: &FakeHermes,
    dir: &Path,
    local_id: &str,
    caps: SessionCapabilities,
    text: &str,
) -> String {
    send_turn_spoken(fake, dir, local_id, caps, text, None).await
}

/// [`send_turn`] with the turn's origin: `spoken_in` is the listening
/// locale of a voice turn, whose request opens with the per-turn
/// instruction the shell's `arm_turn` composes the same way (Epic 64,
/// AD-182) — `ChatMessage::instructions` over `speech::answer_instruction`
/// — and `None` is a typed turn, which opens with nothing.
async fn send_turn_spoken(
    fake: &FakeHermes,
    dir: &Path,
    local_id: &str,
    caps: SessionCapabilities,
    text: &str,
    spoken_in: Option<&str>,
) -> String {
    let row = get_session(dir, local_id).expect("read").expect("row");
    let continuity = continuity_id(caps, row.remote_session_id.as_deref(), || {
        format!("minted-{local_id}")
    });
    if continuity.is_some() && continuity != row.remote_session_id {
        session::set_session_remote_id(dir, local_id, continuity.as_deref()).expect("write id");
    }
    let mut messages = Vec::with_capacity(2);
    let instruction = spoken_in.map(speech::answer_instruction);
    if let Some(system) = ChatMessage::instructions(instruction.iter()) {
        messages.push(system);
    }
    messages.push(ChatMessage::text(Role::User, text));
    let request = ChatRequest {
        model: "hermes-agent".to_owned(),
        messages,
        session_id: continuity,
        ..ChatRequest::default()
    };
    let client = client(Duration::from_secs(5)).expect("client");
    let (_handle, cancel) = cancellation();
    let mut sink = |_: ChatEvent| {};
    let outcome = stream_chat(
        &client,
        &fake.endpoint(Some("nixie")),
        &request,
        &options(),
        cancel,
        &mut sink,
    )
    .await
    .expect("a turn completes");
    outcome.content
}

fn chat_requests(fake: &FakeHermes) -> Vec<Seen> {
    fake.seen()
        .into_iter()
        .filter(|seen| seen.method == "POST")
        .collect()
}

// ---------------------------------------------------------------------------
// Epic 64, Story 64.2 — the per-turn instruction on a spoken turn
// ---------------------------------------------------------------------------

/// AD-182, asserted on the wire: a turn the voice heard opens with one
/// system message asking for the answer in the listening language, and a
/// typed turn on the same conversation opens with nothing at all — the
/// user text is its first and only message, byte for byte as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_turn_carries_the_language_instruction_and_a_typed_one_does_not() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let dir = device_dir("spoken");
    insert_session(&dir, &local_session("local", None)).expect("insert");
    let caps = SessionCapabilities::NONE;

    send_turn_spoken(
        &fake,
        &dir,
        "local",
        caps,
        "what's the weather",
        Some("en-US"),
    )
    .await;
    send_turn_spoken(&fake, &dir, "local", caps, "and tomorrow?", None).await;

    let posts = chat_requests(&fake);
    assert_eq!(posts.len(), 2);

    let spoken = posts[0].messages();
    assert_eq!(spoken.len(), 2, "one instruction, one question: {spoken:?}");
    assert_eq!(spoken[0].0, "system");
    assert_eq!(
        spoken[0].1,
        "The person asked this aloud and your answer will be read aloud to them. Answer in English (en-US)."
    );
    assert_eq!(
        spoken[1],
        ("user".to_owned(), "what's the weather".to_owned())
    );

    let typed = posts[1].messages();
    assert_eq!(
        typed,
        vec![("user".to_owned(), "and tomorrow?".to_owned())],
        "a typed turn sends exactly what it sent before"
    );
    assert!(
        !posts[1].body.to_string().contains("read aloud"),
        "no instruction anywhere in a typed turn's body"
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_devices_two_turns_one_hermes_session() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let caps = probe_capabilities(&client, &fake.endpoint(None)).await;
    assert!(
        caps.continuity_header && caps.session_api,
        "the fake advertises both; keeper must read both"
    );

    // Device A starts the conversation. No remote id yet: one is minted and
    // written to the row before the first byte goes out.
    let dir_a = device_dir("a");
    insert_session(&dir_a, &local_session("local-a", None)).expect("insert");
    let reply_a = send_turn(&fake, &dir_a, "local-a", caps, "what changed in the drive").await;
    assert_eq!(reply_a, "reply 1");
    let row_a = get_session(&dir_a, "local-a").expect("read").expect("row");
    let identity = row_a
        .remote_session_id
        .clone()
        .expect("the first turn minted the conversation's identity");

    // Device B knows nothing of it. It lists the gateway and adopts a row
    // carrying the same identity.
    let dir_b = device_dir("b");
    let found = list_sessions(&client, &fake.endpoint(Some("nixie")))
        .await
        .expect("list");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, identity);
    let done = reconcile(&dir_b, PROVIDER, BOT, &found, NOW).expect("reconcile");
    assert_eq!(done.adopted, 1);
    let page = search_sessions(&dir_b, &SessionQuery::default()).expect("search");
    assert_eq!(page.rows.len(), 1);
    let adopted = &page.rows[0].session;
    assert_eq!(
        adopted.remote_session_id.as_deref(),
        Some(identity.as_str())
    );
    assert_eq!(adopted.title, "what changed in the drive");
    assert_eq!(adopted.remote_source.as_deref(), Some("api"));
    assert!(adopted.remote_last_active_ms.is_some());

    // Device B continues it — under the id it adopted, not one of its own.
    let reply_b = send_turn(&fake, &dir_b, &adopted.id, caps, "and the notes?").await;
    assert_eq!(reply_b, "reply 2");

    // The wire: both turns carried the same header value.
    let posts = chat_requests(&fake);
    assert_eq!(posts.len(), 2);
    assert_eq!(posts[0].session_header(), Some(identity.as_str()));
    assert_eq!(posts[1].session_header(), Some(identity.as_str()));
    assert!(
        posts[0].target.starts_with("/p/nixie/"),
        "the profile prefix is still the URL's business"
    );

    // The far side: ONE session, holding both devices' turns.
    let sessions = fake.sessions();
    assert_eq!(sessions.len(), 1, "no fingerprint session was minted");
    assert_eq!(sessions[0].0, identity);
    assert_eq!(sessions[0].1.messages.len(), 4);

    // And the shared transcript, as either device reads it (AD-181).
    let history = fetch_messages(&client, &fake.endpoint(Some("nixie")), &identity)
        .await
        .expect("history");
    let texts: Vec<&str> = history.iter().map(|m| m.content.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "what changed in the drive",
            "reply 1",
            "and the notes?",
            "reply 2"
        ]
    );
    let composed = remote::compose_messages(&history, &adopted.id, PROVIDER, NOW);
    assert_eq!(composed.len(), 4);
    assert!(composed.iter().all(|m| !m.partial));
    assert_eq!(composed[1].finish_reason.as_deref(), Some("stop"));
    assert_eq!(composed[0].seq, 0);
    assert_eq!(composed[3].seq, 3);
    assert_eq!(
        transcript_source(caps, adopted),
        BotTranscriptSource::Remote
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_endpoint_with_no_capabilities_behaves_exactly_as_today() {
    let fake = FakeHermes::start(FakeState {
        old: true,
        ..FakeState::default()
    })
    .await;
    let client = client(Duration::from_secs(5)).expect("client");

    // The probe answers nothing — and never an error.
    let caps = probe_capabilities(&client, &fake.endpoint(None)).await;
    assert_eq!(caps, SessionCapabilities::NONE);

    // A turn goes out with no header, so the row keeps no identity and the
    // gateway does what it did before: mints one per client.
    let dir = device_dir("old");
    insert_session(&dir, &local_session("local", None)).expect("insert");
    let reply = send_turn(&fake, &dir, "local", caps, "hello").await;
    assert_eq!(reply, "reply 1");
    let row = get_session(&dir, "local").expect("read").expect("row");
    assert_eq!(row.remote_session_id, None);
    let posts = chat_requests(&fake);
    assert_eq!(posts.len(), 1);
    assert_eq!(
        posts[0].session_header(),
        None,
        "today's request, byte for byte"
    );
    assert!(fake.sessions()[0].0.starts_with("fingerprint-"));

    // The list is the local list, labelled local, and nothing was asked of
    // the gateway beyond the one probe.
    let page = search_sessions(&dir, &SessionQuery::default()).expect("search");
    let listed = BotSessionListVm::compose(&page, |_| caps);
    assert_eq!(listed.rows[0].transcript, BotTranscriptSource::Local);
    let gets: Vec<Seen> = fake
        .seen()
        .into_iter()
        .filter(|seen| seen.method == "GET")
        .collect();
    assert_eq!(gets.len(), 1);
    assert!(gets[0].target.starts_with("/v1/capabilities"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ollama_is_never_asked() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let endpoint = Endpoint {
        kind: ProviderKind::Ollama,
        ..fake.endpoint(None)
    };
    assert_eq!(
        probe_capabilities(&client, &endpoint).await,
        SessionCapabilities::NONE
    );
    assert!(
        fake.seen().is_empty(),
        "no request left for an Ollama endpoint"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_hidden_bot_chat_row_is_found() {
    let mut state = FakeState::default();
    state.sessions.push((
        "visible-1".to_owned(),
        fake_session("Weekly plan", false, 1_700_000_100.0),
    ));
    state.sessions.push((
        "hidden-bot-chat".to_owned(),
        fake_session(HIDDEN_BOT_CHAT_TITLE, true, 1_700_000_200.0),
    ));
    let fake = FakeHermes::start(state).await;
    let client = client(Duration::from_secs(5)).expect("client");

    let found = list_sessions(&client, &fake.endpoint(Some("nixie")))
        .await
        .expect("list");
    let ids: Vec<&str> = found.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, vec!["visible-1", "hidden-bot-chat"]);
    assert!(found[1].hidden);

    // Asked for the one way Hermes serves it: by title, with include_hidden.
    let asked_hidden = fake.seen().into_iter().any(|seen| {
        seen.target.contains("title=Bot%20Chat") && seen.target.contains("include_hidden=true")
    });
    assert!(
        asked_hidden,
        "the hidden row needs title=… and include_hidden=true"
    );

    let dir = device_dir("hidden");
    let done = reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("reconcile");
    assert_eq!(done.adopted, 2);
    let page = search_sessions(&dir, &SessionQuery::default()).expect("search");
    let titles: Vec<&str> = page.rows.iter().map(|r| r.session.title.as_str()).collect();
    assert_eq!(titles, vec![HIDDEN_BOT_CHAT_TITLE, "Weekly plan"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_list_longer_than_one_page_is_walked_to_its_end() {
    let mut state = FakeState::default();
    for n in 0..201 {
        state.sessions.push((
            format!("s-{n:03}"),
            fake_session(&format!("session {n}"), false, 1_700_000_000.0 + n as f64),
        ));
    }
    let fake = FakeHermes::start(state).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let found = list_sessions(&client, &fake.endpoint(Some("nixie")))
        .await
        .expect("list");
    assert_eq!(found.len(), 201);
    let offsets: Vec<String> = fake
        .seen()
        .into_iter()
        .filter(|seen| seen.target.contains("offset="))
        .map(|seen| seen.target)
        .collect();
    assert!(offsets.iter().any(|t| t.contains("limit=200&offset=0")));
    assert!(offsets.iter().any(|t| t.contains("limit=200&offset=200")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rows_are_labelled_remote_or_local_by_where_their_transcript_lives() {
    let mut state = FakeState::default();
    state.sessions.push((
        "remote-1".to_owned(),
        fake_session("From the phone", false, 1_700_000_300.0),
    ));
    let fake = FakeHermes::start(state).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let caps = probe_capabilities(&client, &fake.endpoint(None)).await;

    let dir = device_dir("label");
    insert_session(&dir, &local_session("local-only", None)).expect("insert");
    let found = list_sessions(&client, &fake.endpoint(Some("nixie")))
        .await
        .expect("list");
    reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("reconcile");

    let page = search_sessions(&dir, &SessionQuery::default()).expect("search");
    let listed = BotSessionListVm::compose(&page, |_| caps);
    let labels: Vec<(&str, BotTranscriptSource)> = listed
        .rows
        .iter()
        .map(|r| (r.session.title.as_str(), r.transcript))
        .collect();
    assert_eq!(
        labels,
        vec![
            ("Started here", BotTranscriptSource::Local),
            ("From the phone", BotTranscriptSource::Remote),
        ]
    );
    let remote_row = &listed.rows[1].session;
    assert_eq!(remote_row.remote_last_active_ms, Some(1_700_000_300_000));
    assert_eq!(remote_row.remote_source.as_deref(), Some("api"));

    // The same rows on an endpoint that lost its session API read locally:
    // the id names nothing keeper can reach.
    let degraded = BotSessionListVm::compose(&page, |_| SessionCapabilities::NONE);
    assert!(degraded
        .rows
        .iter()
        .all(|r| r.transcript == BotTranscriptSource::Local));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_second_reconcile_refreshes_rather_than_duplicates() {
    let mut state = FakeState::default();
    state.sessions.push((
        "remote-1".to_owned(),
        fake_session("From the phone", false, 1_700_000_300.0),
    ));
    let fake = FakeHermes::start(state).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let dir = device_dir("twice");
    let endpoint = fake.endpoint(Some("nixie"));

    let found = list_sessions(&client, &endpoint).await.expect("list");
    assert_eq!(
        reconcile(&dir, PROVIDER, BOT, &found, NOW)
            .expect("first")
            .adopted,
        1
    );
    let again = reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("second");
    assert_eq!((again.adopted, again.refreshed), (0, 0));

    fake.state().sessions[0].1.last_active = 1_700_000_900.0;
    let found = list_sessions(&client, &endpoint).await.expect("list");
    let third = reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("third");
    assert_eq!((third.adopted, third.refreshed), (0, 1));
    let page = search_sessions(&dir, &SessionQuery::default()).expect("search");
    assert_eq!(page.rows.len(), 1);
    assert_eq!(
        page.rows[0].session.remote_last_active_ms,
        Some(1_700_000_900_000)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conversation_deleted_here_is_not_adopted_back() {
    let mut state = FakeState::default();
    state.sessions.push((
        "remote-1".to_owned(),
        fake_session("From the phone", false, 1_700_000_300.0),
    ));
    let fake = FakeHermes::start(state).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let dir = device_dir("dismiss");
    let endpoint = fake.endpoint(Some("nixie"));

    let found = list_sessions(&client, &endpoint).await.expect("list");
    reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("adopt");
    let page = search_sessions(&dir, &SessionQuery::default()).expect("search");
    let local_id = page.rows[0].session.id.clone();
    delete_session(&dir, &local_id, NOW).expect("delete");

    let again = reconcile(&dir, PROVIDER, BOT, &found, NOW).expect("reconcile after delete");
    assert_eq!(
        again.adopted, 0,
        "Delete would otherwise be an affordance that lies"
    );
    assert!(search_sessions(&dir, &SessionQuery::default())
        .expect("search")
        .rows
        .is_empty());
    // The gateway's copy is untouched: keeper never deletes on a server it
    // does not own.
    assert_eq!(fake.sessions().len(), 1);
}

// ---------------------------------------------------------------------------
// Story 63.7 — following from the other device
// ---------------------------------------------------------------------------

/// The messages route, as the fake records it — the one path a follow reads.
/// The id travels percent-encoded, so the comparison decodes the target.
fn is_history_read(seen: &Seen, remote_id: &str) -> bool {
    seen.method == "GET"
        && percent_decode(&seen.target).contains(&format!("/api/sessions/{remote_id}/messages?"))
}

/// What a step looks like on the gateway before the tools run.
fn tool_call_block() -> FakeMessage {
    FakeMessage {
        role: "assistant".to_owned(),
        content: String::new(),
        tool_calls: Some(
            json!([{"id": "call-1", "type": "function", "function": {"name": "search_notes", "arguments": "{}"}}]),
        ),
        finish_reason: None,
    }
}

/// The clock a follow decision on device B runs against: the fake's own
/// second-resolution clock (`1_700_000_000 + index`), a few seconds after
/// the newest row — so idle is measured on one clock, as it is in the shell
/// once the adopted row carries the gateway's `last_active`.
fn fake_now_ms(rows: usize) -> i64 {
    (1_700_000_000 + rows as i64) * 1_000 + 3_000
}

/// One follow read on device B, exactly as `bots_session_follow` composes
/// it: the history, the merge with keeper's own rows, and the decision.
async fn follow_once(
    fake: &FakeHermes,
    dir: &Path,
    local_id: &str,
    now_ms: i64,
) -> (Vec<session::BotMessage>, bool, Follow) {
    let client = client(Duration::from_secs(5)).expect("client");
    let row = get_session(dir, local_id).expect("read").expect("row");
    let remote_id = row.remote_session_id.as_deref().expect("a remote id");
    let found = fetch_messages(&client, &fake.endpoint(Some("nixie")), remote_id)
        .await
        .expect("history");
    let local = session::list_messages(dir, local_id).expect("local rows");
    let merged = follow::merge(&local, &found, &row.id, &row.provider_id, row.updated_ms);
    let live = follow::turn_open(&found);
    let next = follow::decide(&FollowSignals {
        now_ms,
        owns_turn: false,
        turn_open: live,
        newest_ms: follow::newest_ms(&found),
        last_active_ms: row.remote_last_active_ms,
        updated_ms: row.updated_ms,
    });
    (merged, live, next)
}

/// Device A starts the conversation and device B adopts it; both directories
/// and the identity come back.
async fn two_devices(fake: &FakeHermes) -> (PathBuf, PathBuf, String, String) {
    let client = client(Duration::from_secs(5)).expect("client");
    let caps = probe_capabilities(&client, &fake.endpoint(None)).await;
    let dir_a = device_dir("follow-a");
    insert_session(&dir_a, &local_session("local-a", None)).expect("insert");
    send_turn(fake, &dir_a, "local-a", caps, "what changed in the drive").await;
    let identity = get_session(&dir_a, "local-a")
        .expect("read")
        .expect("row")
        .remote_session_id
        .expect("minted");
    let dir_b = device_dir("follow-b");
    let found = list_sessions(&client, &fake.endpoint(Some("nixie")))
        .await
        .expect("list");
    reconcile(&dir_b, PROVIDER, BOT, &found, NOW).expect("reconcile");
    let adopted = search_sessions(&dir_b, &SessionQuery::default())
        .expect("search")
        .rows
        .remove(0)
        .session;
    (dir_a, dir_b, identity, adopted.id)
}

/// The acceptance, step by step: device B, with the conversation open,
/// sees device A's user turn, then the tool call as it starts, then the
/// answer when it completes — and never a token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_b_sees_device_a_turn_step_by_step() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let (_dir_a, dir_b, identity, local_b) = two_devices(&fake).await;

    // Open on B: the finished first turn, nothing in progress.
    let (opened, live, next) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(2)).await;
    assert_eq!(opened.len(), 2);
    assert!(!live);
    assert_eq!(
        next,
        Follow::Poll {
            after_ms: follow::SETTLED_INTERVAL_MS
        }
    );

    // Device A speaks. Hermes flushes the user turn at turn start.
    fake.flush(&identity, said("user", "and the notes?"));
    let (rows, live, next) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(3)).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2].role, "user");
    assert_eq!(rows[2].content, "and the notes?");
    assert!(live, "a question with no answer yet is a turn in progress");
    assert_eq!(
        next,
        Follow::Poll {
            after_ms: follow::LIVE_INTERVAL_MS
        }
    );

    // The tool-call block lands before the tool runs.
    fake.flush(&identity, tool_call_block());
    let (rows, live, _) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(4)).await;
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[3].role, "assistant");
    assert_eq!(rows[3].tool_call_count, 1);
    assert_eq!(rows[3].content, "");
    assert!(
        !rows[3].partial,
        "a row the gateway served is one it finished writing"
    );
    assert!(live);

    // The tool result, then the answer when it completes.
    fake.flush(&identity, said("tool", "{\"notes\": 2}"));
    let (rows, live, _) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(5)).await;
    assert_eq!(rows.len(), 5);
    assert!(live);
    fake.flush(&identity, said("assistant", "Two notes changed."));
    let (rows, live, next) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(6)).await;
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[5].content, "Two notes changed.");
    assert_eq!(rows[5].finish_reason.as_deref(), Some("stop"));
    assert!(!live, "the answer landed; the turn is closed");
    assert_eq!(
        next,
        Follow::Poll {
            after_ms: follow::SETTLED_INTERVAL_MS
        }
    );
    assert_eq!(
        rows.iter().map(|m| m.seq).collect::<Vec<_>>(),
        (0..6).collect::<Vec<i64>>()
    );

    // Every read of the follow was the history route: five of them, and
    // nothing else was asked of the gateway once the conversation was open.
    let reads: Vec<Seen> = fake
        .seen()
        .into_iter()
        .filter(|s| is_history_read(s, &identity))
        .collect();
    assert_eq!(reads.len(), 5);
    assert!(reads.iter().all(|s| s.target.contains("order=oldest")));
}

/// AD-177, enforced by evidence: across a whole follow — open, four steps,
/// the turn closing — the gateway never receives a request for
/// `/v1/runs/{id}/events`, or for any `/v1/runs/` route at all, though it
/// would answer one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn keeper_never_opens_the_run_events_stream_of_a_run_it_did_not_start() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let (_dir_a, dir_b, identity, local_b) = two_devices(&fake).await;

    fake.flush(&identity, said("user", "and the notes?"));
    fake.flush(&identity, tool_call_block());
    for rows in 3..=5 {
        follow_once(&fake, &dir_b, &local_b, fake_now_ms(rows)).await;
    }
    fake.flush(&identity, said("tool", "{}"));
    fake.flush(&identity, said("assistant", "Two."));
    follow_once(&fake, &dir_b, &local_b, fake_now_ms(6)).await;

    let seen = fake.seen();
    let runs: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.target.contains("/v1/runs"))
        .collect();
    assert!(
        runs.is_empty(),
        "keeper asked for a run it did not start: {runs:?}"
    );
    // And the positive half: everything device B asked after adopting the
    // session was the history route, on the profile prefix.
    let after_adopt: Vec<&Seen> = seen
        .iter()
        .filter(|s| s.method == "GET" && !s.target.contains("/api/sessions?"))
        .filter(|s| !s.target.ends_with("/v1/capabilities"))
        .collect();
    assert!(!after_adopt.is_empty());
    assert!(after_adopt
        .iter()
        .all(|s| is_history_read(s, &identity) && s.target.starts_with("/p/nixie/")));
}

/// Polling stops when the session goes quiet: the same transcript, read
/// later and later, backs off and then is not read again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn polling_backs_off_and_stops_when_the_session_goes_quiet() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let (_dir_a, dir_b, _identity, local_b) = two_devices(&fake).await;

    let newest = fake_now_ms(2) - 3_000;
    let (_, _, soon) = follow_once(&fake, &dir_b, &local_b, newest + 1_000).await;
    assert_eq!(
        soon,
        Follow::Poll {
            after_ms: follow::SETTLED_INTERVAL_MS
        }
    );
    let (_, _, later) = follow_once(&fake, &dir_b, &local_b, newest + follow::QUIET_AFTER_MS).await;
    assert_eq!(
        later,
        Follow::Poll {
            after_ms: follow::QUIET_INTERVAL_MS
        }
    );
    let (_, _, cold) = follow_once(&fake, &dir_b, &local_b, newest + follow::COLD_AFTER_MS).await;
    assert_eq!(cold, Follow::Stop);
}

/// The device that holds the turn does not read the transcript underneath
/// its own stream, whatever the transcript looks like.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_holding_the_turn_does_not_poll() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let (_dir_a, dir_b, identity, local_b) = two_devices(&fake).await;
    fake.flush(&identity, said("user", "and the notes?"));
    let client = client(Duration::from_secs(5)).expect("client");
    let found = fetch_messages(&client, &fake.endpoint(Some("nixie")), &identity)
        .await
        .expect("history");
    assert!(
        follow::turn_open(&found),
        "the transcript alone says: read every two seconds"
    );
    let row = get_session(&dir_b, &local_b).expect("read").expect("row");
    let next = follow::decide(&FollowSignals {
        now_ms: fake_now_ms(3),
        owns_turn: true,
        turn_open: true,
        newest_ms: follow::newest_ms(&found),
        last_active_ms: row.remote_last_active_ms,
        updated_ms: row.updated_ms,
    });
    assert_eq!(next, Follow::Stop);
}

/// The precedence rule against the real transcript: device B's own turn,
/// closed by its stream with measured usage, is shown as B wrote it and
/// not doubled by the gateway's copy; device A's turn is shown as the
/// gateway holds it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_this_device_sent_is_shown_as_it_wrote_it() {
    let fake = FakeHermes::start(FakeState::default()).await;
    let (_dir_a, dir_b, _identity, local_b) = two_devices(&fake).await;
    let client = client(Duration::from_secs(5)).expect("client");
    let caps = probe_capabilities(&client, &fake.endpoint(None)).await;

    // B's own turn: the shell writes the user row and the assistant row
    // before the request, closes the assistant row after.
    let now = fake_now_ms(2);
    let user = session::BotMessage {
        id: "B-user".to_owned(),
        session_id: local_b.clone(),
        seq: 0,
        role: "user".to_owned(),
        content: "and the notes?".to_owned(),
        model: None,
        provider_id: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        ttft_ms: None,
        duration_ms: None,
        finish_reason: None,
        request_id: None,
        tool_call_count: 0,
        partial: false,
        created_ms: now,
    };
    session::append_message(&dir_b, &user).expect("user row");
    let answer = session::BotMessage {
        id: "B-answer".to_owned(),
        role: "assistant".to_owned(),
        content: String::new(),
        partial: true,
        ..user.clone()
    };
    session::append_message(&dir_b, &answer).expect("assistant row");
    let reply = send_turn(&fake, &dir_b, &local_b, caps, "and the notes?").await;
    session::close_message(
        &dir_b,
        session::MessageClose {
            id: "B-answer",
            content: &reply,
            total_tokens: Some(42),
            duration_ms: Some(910),
            finish_reason: Some("stop"),
            partial: false,
            ..session::MessageClose::default()
        },
    )
    .expect("close");

    let (rows, live, _) = follow_once(&fake, &dir_b, &local_b, fake_now_ms(4)).await;
    assert!(!live);
    let ids: Vec<&str> = rows.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "B-user", "B-answer"]);
    assert_eq!(rows[3].content, "reply 2");
    assert_eq!(
        rows[3].total_tokens,
        Some(42),
        "what B's stream measured survives"
    );
    assert_eq!(rows[3].duration_ms, Some(910));
    // The gateway holds four rows; the screen holds four rows; nothing is
    // shown twice.
    assert_eq!(fake.sessions()[0].1.messages.len(), 4);
}
