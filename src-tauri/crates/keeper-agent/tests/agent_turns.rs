//! An agent's turns in a session, end to end against a local model stub and
//! a recording room port (story 90.5, acceptance 5, 6, 8–10, 16, 18–20).
//!
//! No homeserver and no git: the drives are folders, the room is a fake
//! [`EditPort`] that accepts every send, and the provider is an
//! OpenAI-shaped SSE stub that records every request body.
#![cfg(unix)]

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::agent::{
    arm_agent, reply_of, trail_of, AgentDeps, AgentProfiles, Arrived, ConversationPort, HeldFocus,
    Outcome, Probe, RoomFuture, ServedSession, SessionRef, Trail, TurnEnding, BRIEF_REFUSED,
    BRIEF_UNSENT, BUDGET_SPENT, JOIN_POLL, LOCAL_ONLY_REFUSAL, NARROWER_THAN_ROOM,
    NOT_THIS_DELEGATION, ROUNDS_SPENT,
};
use keeper_agent::claims::{blocked_status, conflict_line, conflict_of, Lease};
use keeper_agent::delegate::{
    reply_content, BoolFuture, BriefRoomFuture, DelegationPort, EventsFuture, MembersFuture,
    NOT_DELEGATED,
};
use keeper_agent::host::UNATTENDED_REFUSAL;
use keeper_agent::matrix_sink::{EditPort, SendFuture};
use keeper_agent::rooms::{
    Arrival, BriefRoom, Known, KnownAgent, NOT_THE_PERSON, NO_AGENT_POWER, OBSERVER_TEXT,
    UNSIGNED_DEVICE,
};
use keeper_agent::runtime::Router;
use keeper_agent::turn::{DrivePorts, TurnEnv};
use keeper_agent::writer::WriterError;
use keeper_agent::zone::{read_zone, AgentHome};
use keeper_core::agents::delegation::{read_brief, DelegateContent};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::{
    RunState, StatusContent, FINAL_CUT_BYTES, SCOPE, SESSION_ROOM_TYPE, STATUS,
};
use keeper_core::agents::focus::FOCUS_TTL;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::reader::{hydrate_blob, read_session};
use keeper_core::agents::log::replay::replay;
use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
use keeper_core::agents::log::{
    AssistantBody, ClaimAction, ClaimBody, HostSlug, LineBody, LineKind, LogLine, ToolCallBody,
    ToolOutcomeWord, ToolResultBody, UserBody, LINE_VERSION,
};
use keeper_core::agents::log::{DelegateBody, DelegateReply, DelegateState, PeerBody};
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use keeper_core::bots::chat::{self, CancelHandle};
use keeper_core::bots::{store, Bot, Provider, ProviderKind};
use keeper_core::error::CoreError;
use keeper_core::platform::Platform;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::events::room::power_levels::{RoomPowerLevels, RoomPowerLevelsEventContent};
use matrix_sdk::ruma::room_version_rules::AuthorizationRules;
use matrix_sdk::ruma::{
    OwnedEventId, OwnedRoomId, OwnedTransactionId, OwnedUserId, RoomId, UserId,
};
use serde_json::{json, Value};

const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";
const SESSION: &str = "active/2026-10-02-chat";
/// In a scripted completion: the delegation id the request names.
const DELEGATION: &str = "@DELEGATION@";
/// What a `delegate` result says just before the delegation's id.
const DELEGATION_SAID: &str = "as delegation ";

struct DataDir(PathBuf);

impl Platform for DataDir {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.0.clone())
    }
    fn keychain_set(&self, _: &str, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn keychain_get(&self, _: &str) -> Result<Option<String>, CoreError> {
        Ok(None)
    }
    fn keychain_delete(&self, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn open_url(&self, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str, _: &keeper_core::vm::NotifyTarget) -> Result<(), CoreError> {
        Ok(())
    }
    fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported("no sidecar".to_owned()))
    }
    fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

/// A room that accepts every send and remembers it; with `stop` set, its
/// first send cancels the turn. Its members are whom the test says, now.
#[derive(Default)]
struct Room {
    sent: Mutex<Vec<(String, Value)>>,
    stop: Mutex<Option<CancelHandle>>,
    members: Mutex<BTreeSet<OwnedUserId>>,
    /// Its members cannot be read.
    unread: std::sync::atomic::AtomicBool,
    /// Its next send is asked to wait, and meanwhile its members become
    /// these.
    limited: Mutex<Option<Vec<&'static str>>>,
    /// As `limited`, for its next approval request only.
    limited_request: Mutex<Option<Vec<&'static str>>>,
    /// An approvals folder whose records are counted when a request is
    /// sent: what was on disk before the room heard of it.
    witness: Mutex<Option<PathBuf>>,
    records_at_request: Mutex<Option<usize>>,
}

impl Room {
    fn of(members: &[&str]) -> Room {
        let room = Room::default();
        room.set_members(members);
        room
    }

    fn set_members(&self, members: &[&str]) {
        *self.members.lock().expect("lock") = members.iter().map(|m| user(m)).collect();
    }

    fn sent(&self) -> Vec<(String, Value)> {
        self.sent.lock().expect("lock").clone()
    }
}

impl EditPort for Room {
    fn send<'a>(
        &'a self,
        event_type: &'a str,
        content: Value,
        _: OwnedTransactionId,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            if let Some(stop) = self.stop.lock().expect("lock").take() {
                stop.cancel();
            }
            if event_type == keeper_core::agents::events::APPROVAL_REQUEST {
                if let Some(dir) = self.witness.lock().expect("lock").as_ref() {
                    let records = std::fs::read_dir(dir).map_or(0, |entries| entries.count());
                    *self.records_at_request.lock().expect("lock") = Some(records);
                }
            }
            if event_type == keeper_core::agents::events::APPROVAL_REQUEST {
                if let Some(members) = self.limited_request.lock().expect("lock").take() {
                    self.set_members(&members);
                    return Err(keeper_core::agents::matrix::AgentMatrixError::RateLimited {
                        retry_after_ms: Some(1),
                    });
                }
            }
            if let Some(members) = self.limited.lock().expect("lock").take() {
                self.set_members(&members);
                return Err(keeper_core::agents::matrix::AgentMatrixError::RateLimited {
                    retry_after_ms: Some(1),
                });
            }
            let mut sent = self.sent.lock().expect("lock");
            sent.push((event_type.to_owned(), content));
            Ok(OwnedEventId::try_from(format!("$sent{}:example.org", sent.len())).expect("id"))
        })
    }

    fn members(&self) -> MembersFuture<'_> {
        Box::pin(async move {
            if self.unread.load(Ordering::SeqCst) {
                return Err("unreadable".to_owned());
            }
            Ok(self.members.lock().expect("lock").clone())
        })
    }
}

/// One completion: the SSE `data:` payloads it streams.
type Completion = Vec<Value>;

fn prose(text: &str) -> Completion {
    vec![
        json!({"model":"model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
        json!({"model":"model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    ]
}

fn calls(calls: &[(&str, &str, Value)]) -> Completion {
    let tool_calls: Vec<Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, name, args))| {
            json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})
        })
        .collect();
    vec![
        json!({"model":"model","choices":[{"index":0,"delta":{"tool_calls":tool_calls},"finish_reason":"tool_calls"}]}),
    ]
}

/// A completion that streams `text` and then fails: the provider reports an
/// error in the stream.
fn broken(text: &str) -> Completion {
    vec![
        json!({"model":"model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]}),
        json!({"error":{"message":"the upstream went away","type":"server_error"}}),
    ]
}

/// An OpenAI-shaped provider: each chat request gets the next scripted
/// completion (then a plain "ok."), anything else a 404. `hits` counts every
/// request, a probe included.
struct Stub {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    hits: Arc<AtomicUsize>,
}

impl Stub {
    fn start(script: Vec<Completion>) -> Stub {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let hits = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&requests);
        let counted = Arc::clone(&hits);
        let script = Arc::new(Mutex::new(script.into_iter().rev().collect::<Vec<_>>()));
        std::thread::spawn(move || {
            for socket in listener.incoming() {
                let Ok(mut socket) = socket else { continue };
                counted.fetch_add(1, Ordering::SeqCst);
                let seen = Arc::clone(&seen);
                let script = Arc::clone(&script);
                std::thread::spawn(move || {
                    let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
                    let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                    let mut first = String::new();
                    if reader.read_line(&mut first).is_err() {
                        return;
                    }
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty()
                        {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0; length];
                    let _ = reader.read_exact(&mut body);
                    if !first.contains("/chat/completions") {
                        let _ = write!(socket, "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        return;
                    }
                    seen.lock()
                        .expect("lock")
                        .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                    let completion = script
                        .lock()
                        .expect("lock")
                        .pop()
                        .unwrap_or_else(|| prose("ok."));
                    // `@DELEGATION@` in a scripted frame is the delegation id
                    // the request itself carries — what a model would read
                    // in its `delegate` result — never one the test knows.
                    let request = String::from_utf8_lossy(&body);
                    let named = request.rfind(DELEGATION_SAID).map(|at| {
                        let from = at + DELEGATION_SAID.len();
                        request[from..(from + 26).min(request.len())].to_owned()
                    });
                    // A `{"pause_ms": n}` entry is no frame: the stream
                    // waits there, so the edits in between are paced.
                    let mut parts: Vec<(String, u64)> = Vec::new();
                    for data in completion {
                        match data["pause_ms"].as_u64() {
                            Some(pause) => parts.push((String::new(), pause)),
                            None => {
                                let mut frame = format!("data: {data}\n\n");
                                if let Some(id) = &named {
                                    frame = frame.replace(DELEGATION, id);
                                }
                                parts.push((frame, 0))
                            }
                        }
                    }
                    parts.push(("data: [DONE]\n\n".to_owned(), 0));
                    let length: usize = parts.iter().map(|(frame, _)| frame.len()).sum();
                    let _ = write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                    );
                    for (frame, pause) in parts {
                        std::thread::sleep(Duration::from_millis(pause));
                        let _ = socket.write_all(frame.as_bytes());
                        let _ = socket.flush();
                    }
                });
            }
        });
        Stub {
            url,
            requests,
            hits,
        }
    }

    fn requests(&self) -> Vec<Value> {
        self.requests.lock().expect("lock").clone()
    }
}

fn user(id: &str) -> OwnedUserId {
    OwnedUserId::try_from(id).expect("user")
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

fn decl(id: &str, readers: &[&str], local_only: bool) -> DriveDecl {
    let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
    drive::parse(&format!(
        "version = 1\nid = \"{id}\"\ntitle = \"{id}\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [{}]\nlocal_only = {local_only}\n",
        readers.join(", ")
    ))
    .expect("a declaration")
}

fn profile(id: &str, path: &Path) -> SyncProfile {
    let mut profile = SyncProfile::new(id, id, path, "unused");
    profile.agents = Some(Default::default());
    profile.sessions = Some(Default::default());
    profile
}

/// tgdrive (read by tgorka and marta) with Nixi and one conversation, and
/// a `local_only` drive read by tgorka alone.
struct World {
    _root: tempfile::TempDir,
    tgdrive: PathBuf,
    deps: AgentDeps,
    stub: Stub,
    room: Arc<Room>,
    next: usize,
}

const SOUL: &str = "---\nname: Nixi\ntitle: tgorka's proxy\nicon: \"*\"\nrole: Talks with tgorka.\nidentity: \"A quiet companion.\"\ncommunication_style: Short.\nprinciples:\n  - Ask first.\n---\n\nNixi answers from the drive.\n";

fn world(kind: ProviderKind, allow: &[&str], script: Vec<Completion>) -> World {
    world_read_by(&[TGORKA, MARTA], kind, allow, script)
}

/// The world with tgdrive read by `readers`.
fn world_read_by(
    readers: &[&str],
    kind: ProviderKind,
    allow: &[&str],
    script: Vec<Completion>,
) -> World {
    let root = tempfile::tempdir().expect("tempdir");
    let tgdrive = root.path().join("tgdrive");
    let private = root.path().join("private");
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).expect("data");
    let tg_decl = decl("tgdrive", readers, false);
    let private_decl = decl("private", &[TGORKA], true);
    let quoted: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
    write(
        &tgdrive,
        "80-agents/_drive.toml",
        &format!("version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [{}]\n", quoted.join(", ")),
    );
    let allow: Vec<String> = allow.iter().map(|a| format!("\"{a}\"")).collect();
    write(
        &tgdrive,
        "80-agents/nixi/agent.toml",
        &format!(
            "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"{TGORKA}\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[tools]\nallow = [{}]\ndrives = [\"tgdrive\", \"private\"]\n",
            allow.join(", ")
        ),
    );
    write(&tgdrive, "80-agents/nixi/SOUL.md", SOUL);
    write(
        &tgdrive,
        "80-agents/nixi/MEMORY.md",
        "tgorka likes short answers.\n",
    );
    write(&tgdrive, "notes/hello.md", "first line\nsecond line\n");
    write(&tgdrive, "notes/secret-plan.md", "the plan\n");
    write(&tgdrive, "10-notes/a.md", "needle here\n");
    write(&tgdrive, "00-inbox/x.md", "from outside\n");
    write(&private, "diary.md", "dear diary\n");
    session(&tgdrive, SESSION, &tg_decl);

    let tg_profile = profile("tgdrive", &tgdrive);
    let zone = read_zone("tgdrive", &tg_profile, Some(&tg_decl));
    let home: AgentHome = zone
        .homes
        .into_iter()
        .find_map(|(folder, home)| (folder == "nixi").then_some(home))
        .expect("nixi's folder")
        .expect("nixi reads");

    let stub = Stub::start(script);
    let provider = Provider {
        id: "provider".to_owned(),
        kind,
        name: "stub".to_owned(),
        base_url: stub.url.clone(),
        created_ms: 1,
    };
    store::insert_provider(&data, &provider).expect("provider");
    let row = store::get_provider(&data, "provider")
        .expect("read")
        .expect("row");
    let deps = AgentDeps {
        env: TurnEnv {
            drive: Some(DrivePorts {
                profiles: Arc::new(AgentProfiles::new([
                    ("tgdrive".to_owned(), tg_profile.clone()),
                    ("private".to_owned(), profile("private", &private)),
                ])),
                vault: None,
                approval: None,
            }),
            ..TurnEnv::new(Arc::new(DataDir(data.clone())))
        },
        data_dir: data,
        row,
        bot: Bot {
            id: "agent:tgdrive/nixi".to_owned(),
            provider_id: "provider".to_owned(),
            target: "model".to_owned(),
            name: "Nixi".to_owned(),
            pin_order: 0,
            identity: Default::default(),
            created_ms: 0,
        },
        home,
        host: HostSlug::new("electra").expect("slug"),
        drives: BTreeMap::from([
            ("tgdrive".to_owned(), tg_decl),
            ("private".to_owned(), private_decl),
        ]),
        sessions_zone: tg_profile.sessions_root().expect("sessions"),
        sessions_subfolder: "60-sessions".to_owned(),
        lfs_threshold_bytes: 1_000_000,
        decisions: None,
    };
    World {
        _root: root,
        tgdrive,
        deps,
        stub,
        room: Arc::new(Room::of(
            &readers
                .iter()
                .copied()
                .chain(["@nixi:example.org"])
                .collect::<Vec<_>>(),
        )),
        next: 0,
    }
}

fn session(drive: &Path, path: &str, home: &DriveDecl) -> SessionAgent {
    session_of(
        drive,
        path,
        home,
        "nixi",
        SessionKind::Conversation,
        "!room:example.org",
    )
}

/// A session of `agent`, of `kind`, in `room`, at `path` of `drive`.
fn session_of(
    drive: &Path,
    path: &str,
    home: &DriveDecl,
    agent: &str,
    kind: SessionKind,
    room: &str,
) -> SessionAgent {
    let agent = SessionAgent {
        id: ulid::Ulid::new(),
        agent: agent.to_owned(),
        drive: "tgdrive".to_owned(),
        kind,
        title: "chat".to_owned(),
        requested_by: user(TGORKA),
        parent: None,
        room: room.try_into().expect("room"),
        drives: vec!["tgdrive".to_owned(), "private".to_owned()],
        label: Label::opening(home, Integrity::Owner),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: Vec::new(),
        limits: None,
        workflow: None,
        created_at: chrono::Utc::now(),
    };
    write(
        drive,
        &format!("60-sessions/{path}/agent.toml"),
        &compose_session_agent_toml(&agent),
    );
    agent
}

impl World {
    fn dir(&self, path: &str) -> PathBuf {
        self.tgdrive.join("60-sessions").join(path)
    }

    fn open(&self, path: &str) -> ServedSession {
        self.open_under(path, None).expect("served")
    }

    /// Open `path` as the holder of `lease`.
    fn open_under(
        &self,
        path: &str,
        lease: Option<Arc<Lease>>,
    ) -> Result<ServedSession, keeper_agent::agent::ServeOpenError> {
        let text = std::fs::read_to_string(self.dir(path).join("agent.toml")).expect("agent.toml");
        let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
        ServedSession::open(
            &self.deps,
            &self.dir(path),
            SessionRef {
                drive: "tgdrive".to_owned(),
                path: path.to_owned(),
            },
            agent,
            lease,
        )
    }

    fn arrived(&mut self, sender: &str, text: &str) -> Arrived {
        self.next += 1;
        Arrived {
            event_id: OwnedEventId::try_from(format!("$in{}:example.org", self.next)).expect("id"),
            sender: user(sender),
            arrival: Arrival::Text,
            text: text.to_owned(),
            content: json!({"msgtype":"m.text","body":text}),
            received_at: tokio::time::Instant::now(),
            replay: false,
            via: None,
            device: None,
        }
    }

    async fn ask(&mut self, served: &mut ServedSession, text: &str) -> Outcome {
        let arrived = self.arrived(TGORKA, text);
        self.serve(served, arrived).await
    }

    async fn serve(&self, served: &mut ServedSession, arrived: Arrived) -> Outcome {
        let (_handle, signal) = chat::cancellation();
        served
            .serve(&self.deps, self.room.clone(), arrived, signal)
            .await
            .expect("served")
    }

    fn lines(&self, path: &str) -> Vec<LogLine> {
        read_session(&self.dir(path)).lines
    }
}

fn kinds(lines: &[LogLine], kind: LineKind) -> Vec<&LogLine> {
    lines.iter().filter(|line| line.kind() == kind).collect()
}

fn report(outcome: Outcome) -> keeper_agent::agent::TurnReport {
    match outcome {
        Outcome::Answered(report) => report,
        other => panic!("not a turn: {other:?}"),
    }
}

/// The final edit's whole text: the last `m.replace` the room was sent.
fn final_edit(room: &Room) -> String {
    room.sent()
        .iter()
        .rev()
        .find(|(kind, content)| kind == "m.room.message" && content["m.new_content"].is_object())
        .map(|(_, content)| {
            content["m.new_content"]["body"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .expect("a final edit")
}

fn anchors(room: &Room) -> usize {
    room.sent()
        .iter()
        .filter(|(_, content)| content["dev.keeper.agent.turn"].is_object())
        .count()
}

/// NFR-117's "repeats nothing", by event id: a redelivered event is logged
/// once and answered once.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_event_twice_makes_one_user_line_and_one_turn() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("hello.")]);
    let mut served = world.open(SESSION);
    let arrived = world.arrived(TGORKA, "hi");
    report(world.serve(&mut served, arrived.clone()).await);
    assert!(matches!(
        world.serve(&mut served, arrived.clone()).await,
        Outcome::Duplicate
    ));
    // A restarted host reads the dedupe from the index, not from memory.
    drop(served);
    let mut again = world.open(SESSION);
    assert!(matches!(
        world.serve(&mut again, arrived).await,
        Outcome::Duplicate
    ));

    let lines = world.lines(SESSION);
    assert_eq!(kinds(&lines, LineKind::User).len(), 1);
    assert_eq!(world.stub.requests().len(), 1);
    assert_eq!(anchors(&world.room), 1);
}

/// Text from anyone but the proxy's person writes nothing and runs nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_ignored_arrival_writes_no_line() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let mut served = world.open(SESSION);
    let marta = world.arrived(MARTA, "hello nixi");
    assert!(matches!(
        world.serve(&mut served, marta).await,
        Outcome::Ignored(_)
    ));
    let mut forged = world.arrived(TGORKA, "fake status");
    forged.arrival = Arrival::AgentEvent;
    assert!(matches!(
        world.serve(&mut served, forged).await,
        Outcome::Ignored(_)
    ));
    assert!(world.lines(SESSION).is_empty());
    assert!(world.stub.requests().is_empty());
    assert!(world.room.sent().is_empty());
}

/// 93.2 AC7 (C4/F12): with no decision source installed, a write under the
/// agent's profile-wide grant needs a person and is refused with
/// `UNATTENDED_REFUSAL`, exactly as before Epic 93; the model reads it as
/// `Refused: …` (D9), the file is untouched, and no approval is written.
#[tokio::test(flavor = "multi_thread")]
async fn without_a_decision_source_an_ask_is_refused_as_before() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/new.md","content":"must not land"}),
            )]),
            prose("I could not write it."),
        ],
    );
    let mut served = world.open(SESSION);
    let report = report(world.ask(&mut served, "write a note").await);
    assert_eq!(report.ending, TurnEnding::Complete);
    assert!(!world.tgdrive.join("notes/new.md").exists());
    assert!(!world.dir(SESSION).join("approvals").exists());
    assert!(world
        .sent_of(keeper_core::agents::events::APPROVAL_REQUEST)
        .is_empty());

    let lines = world.lines(SESSION);
    let result = kinds(&lines, LineKind::ToolResult);
    let LineBody::ToolResult(body) = &result[0].body else {
        panic!("a tool result")
    };
    assert_eq!(body.outcome, ToolOutcomeWord::Refused);
    assert_eq!(body.content, format!("Refused: {UNATTENDED_REFUSAL}"));
    let second = world.stub.requests()[1].to_string();
    assert!(second.contains(UNATTENDED_REFUSAL), "{second}");
}

/// The agent's grants are its own drives in the session's scope: a drive it
/// does not name is refused by the grant, whatever the model asks.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_reaches_only_its_drives_in_scope() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![
            calls(&[
                (
                    "r1",
                    "drive_read",
                    json!({"profile":"tgdrive","path":"notes/hello.md"}),
                ),
                (
                    "r2",
                    "drive_read",
                    json!({"profile":"neuradrive","path":"x.md"}),
                ),
            ]),
            prose("done."),
        ],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "read").await);
    let lines = world.lines(SESSION);
    let outcomes: Vec<(ToolOutcomeWord, String)> = kinds(&lines, LineKind::ToolResult)
        .iter()
        .map(|line| match &line.body {
            LineBody::ToolResult(body) => (body.outcome, body.content.clone()),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(outcomes[0].0, ToolOutcomeWord::Ok);
    assert!(outcomes[0].1.contains("first line"));
    assert_eq!(outcomes[1].0, ToolOutcomeWord::Refused);
}

/// C6: a `user` line with no answer is closed with an `error` line and one
/// message after a restart, and never run again.
#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_turn_is_not_rerun_after_a_restart() {
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    {
        // The host died after logging the question.
        let mut served = world.open(SESSION);
        let ServedSession {
            context, writer, ..
        } = &mut served;
        writer
            .write(
                context,
                None,
                Some(OwnedEventId::try_from("$asked:example.org").expect("id")),
                LineBody::User(UserBody {
                    sender: user(TGORKA),
                    text: "do the thing".to_owned(),
                    attachments: Vec::new(),
                }),
            )
            .expect("write");
        writer.sync().expect("sync");
    }
    let mut served = world.open(SESSION);
    assert!(served
        .recover(&world.deps, world.room.clone(), &Trail::default())
        .await
        .expect("recover"));
    let mut again = world.open(SESSION);
    assert!(!again
        .recover(&world.deps, world.room.clone(), &Trail::default())
        .await
        .expect("recover"));

    let lines = world.lines(SESSION);
    assert_eq!(kinds(&lines, LineKind::User).len(), 1);
    assert_eq!(kinds(&lines, LineKind::Error).len(), 1);
    assert_eq!(world.room.sent().len(), 1);
    assert!(world.room.sent()[0].1["body"]
        .as_str()
        .is_some_and(|body| body.contains("cut off when electra restarted")));
    assert_eq!(anchors(&world.room), 0);
    assert!(world.stub.requests().is_empty());
}

/// C6 after a crash mid tool loop: the round's `assistant` line is not an
/// answer, so the question is still closed — by an edit of its anchor, which
/// the room's timeline names — and a status left `running` is set `idle`.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_cut_off_mid_tool_loop_is_closed_by_editing_its_anchor() {
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let user_line = {
        let mut served = world.open(SESSION);
        let ServedSession {
            context, writer, ..
        } = &mut served;
        let asked = writer
            .write(
                context,
                None,
                Some(OwnedEventId::try_from("$asked:example.org").expect("id")),
                LineBody::User(UserBody {
                    sender: user(TGORKA),
                    text: "read the notes".to_owned(),
                    attachments: Vec::new(),
                }),
            )
            .expect("user");
        let round = writer
            .write(
                context,
                None,
                None,
                LineBody::Assistant(AssistantBody {
                    text: String::new(),
                    model: "model".to_owned(),
                    finish: "tool_calls".to_owned(),
                    usage: Default::default(),
                    ttft_ms: None,
                    duration_ms: 0,
                    anchor_event: None,
                }),
            )
            .expect("round");
        let call = writer
            .write(
                context,
                Some(round.id),
                None,
                LineBody::ToolCall(ToolCallBody {
                    call_id: "r1".to_owned(),
                    tool: "drive_read".to_owned(),
                    args: "{}".to_owned(),
                    tier: 0,
                    grant_id: None,
                }),
            )
            .expect("call");
        writer
            .write(
                context,
                Some(call.id),
                None,
                LineBody::ToolResult(ToolResultBody {
                    call_id: "r1".to_owned(),
                    outcome: ToolOutcomeWord::Ok,
                    content: "first line".to_owned(),
                    truncated: None,
                    label: context.label.clone(),
                }),
            )
            .expect("result");
        writer.sync().expect("sync");
        asked.id
    };
    // What the room shows after the question: the anchor and a running status.
    let timeline = vec![
        json!({"type":"m.room.message","event_id":"$anchor:example.org","sender":"@nixi:example.org",
               "content":{"msgtype":"m.text","body":"…","dev.keeper.agent.turn":{"session":SESSION,"line":user_line.to_string()}}}),
        json!({"type":STATUS,"event_id":"$status:example.org","sender":"@nixi:example.org",
               "content":{"run":"running","detail":"1 tool call"}}),
        json!({"type":"m.room.message","event_id":"$forged:example.org","sender":TGORKA,
               "content":{"msgtype":"m.text","body":"…","dev.keeper.agent.turn":{"session":SESSION,"line":user_line.to_string()}}}),
    ];
    let trail = trail_of(&timeline, &user("@nixi:example.org"), Some(user_line));
    assert_eq!(
        trail.anchor,
        Some(OwnedEventId::try_from("$anchor:example.org").expect("id"))
    );

    let mut served = world.open(SESSION);
    assert_eq!(served.context.unanswered, Some(user_line));
    assert!(served
        .recover(&world.deps, world.room.clone(), &trail)
        .await
        .expect("recover"));

    let sent = world.room.sent();
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0].1["m.relates_to"]["event_id"], "$anchor:example.org");
    assert!(sent[0].1["m.new_content"]["body"]
        .as_str()
        .is_some_and(|body| body.contains("cut off when electra restarted")));
    assert_eq!(sent[1].0, STATUS);
    assert_eq!(sent[1].1["run"], "idle");
    assert_eq!(sent[1].1["anchor"], "$status:example.org");
    let lines = world.lines(SESSION);
    assert_eq!(kinds(&lines, LineKind::Error).len(), 1);
    assert!(world.stub.requests().is_empty());
}

fn room_id() -> OwnedRoomId {
    OwnedRoomId::try_from("!room:example.org").expect("room")
}

/// A message that arrives before its session has a worker (a fresh store's
/// first sync, a session folder that syncs after the message) is kept and
/// answered once the worker starts.
#[tokio::test(flavor = "multi_thread")]
async fn a_message_sent_before_the_worker_exists_is_still_answered() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("hello.")]);
    let router = Arc::new(Router::default());
    router.route(&room_id(), world.arrived(TGORKA, "are you there?"));

    let (hand, taken) = std::sync::mpsc::channel();
    let worker = router
        .spawn(&room_id(), SESSION, move |arrivals| {
            let _ = hand.send(arrivals);
            async {}
        })
        .expect("a worker");
    worker.await.expect("worker");
    let mut arrivals = taken.recv().expect("the worker's channel");
    let mut served = world.open(SESSION);
    let (_stop, signal) = chat::cancellation();
    served
        .serve_arrivals(
            &world.deps,
            world.room.clone(),
            Vec::new(),
            &mut arrivals,
            signal,
            &keeper_agent::agent::Activity::default(),
        )
        .await;

    let lines = world.lines(SESSION);
    let users = kinds(&lines, LineKind::User);
    assert_eq!(users.len(), 1);
    let LineBody::User(body) = &users[0].body else {
        panic!("a user line")
    };
    assert_eq!(body.text, "are you there?");
    assert_eq!(final_edit(&world.room), "hello.");
}

/// A worker that ends — its session could not be opened, say — frees its
/// room: the next scan starts a new one, and the status no longer lists it.
#[tokio::test(flavor = "multi_thread")]
async fn a_worker_that_ends_frees_its_room_for_the_next() {
    let router = Arc::new(Router::default());
    let first = router
        .spawn(&room_id(), SESSION, |_arrivals| async {})
        .expect("a worker");
    first.await.expect("worker");
    assert!(router.served().is_empty(), "a dead worker is not served");
    let (hold, held) = tokio::sync::oneshot::channel::<()>();
    let second = router.spawn(&room_id(), SESSION, |_arrivals| async move {
        let _ = held.await;
    });
    assert!(second.is_some(), "the room is free for the next worker");
    assert!(
        router
            .spawn(&room_id(), SESSION, |_arrivals| async {})
            .is_none(),
        "one worker per room"
    );
    assert_eq!(
        router.served().get(&room_id()).map(String::as_str),
        Some(SESSION)
    );
    drop(hold);
}

/// On shutdown an arrival that has not started is left alone: no `user`
/// line, nothing in the room, so the next start reads it from the timeline
/// and answers it.
#[tokio::test(flavor = "multi_thread")]
async fn a_queued_arrival_is_not_started_on_shutdown() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let (queue, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    queue.send(world.arrived(TGORKA, "one")).expect("queued");
    queue.send(world.arrived(TGORKA, "two")).expect("queued");
    drop(queue);
    let mut served = world.open(SESSION);
    let (stop, signal) = chat::cancellation();
    stop.cancel();
    served
        .serve_arrivals(
            &world.deps,
            world.room.clone(),
            Vec::new(),
            &mut arrivals,
            signal,
            &keeper_agent::agent::Activity::default(),
        )
        .await;
    assert!(world.lines(SESSION).is_empty());
    assert!(world.room.sent().is_empty());
    assert!(world.stub.requests().is_empty());
}

/// A turn whose stream breaks after some prose: the room saw it, so the log
/// holds it too (an `assistant` line, `finish: "failed"`), before the
/// `error` line, and the next turn's model reads it.
#[tokio::test(flavor = "multi_thread")]
async fn the_prose_of_a_failed_round_is_logged_before_its_error() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![broken("Half an answer")],
    );
    let mut served = world.open(SESSION);
    let turn = report(world.ask(&mut served, "go").await);
    assert_eq!(turn.ending, TurnEnding::Failed);
    let lines = world.lines(SESSION);
    let kinds_in_order: Vec<LineKind> = lines.iter().map(LogLine::kind).collect();
    assert_eq!(
        &kinds_in_order[kinds_in_order.len() - 2..],
        &[LineKind::Assistant, LineKind::Error]
    );
    let LineBody::Assistant(partial) = &kinds(&lines, LineKind::Assistant)[0].body else {
        panic!("an assistant line")
    };
    assert_eq!(partial.text, "Half an answer");
    assert_eq!(partial.finish, "failed");
    assert!(messages_text(&served.context.messages).contains("Half an answer"));
    assert!(final_edit(&world.room).starts_with("Half an answer"));
}

/// R23 when the artifact cannot be written: the room is not pointed at a
/// file that does not exist, but at the log, which holds the whole answer.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_answer_whose_artifact_fails_points_to_the_log() {
    let answer = "ab".repeat(100 * 1024);
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose(&answer)]);
    // `artifacts` is a file, so nothing can be written under it.
    write(&world.dir(SESSION), "artifacts", "in the way\n");
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "write a lot").await);
    let message = final_edit(&world.room);
    assert_eq!(
        message,
        format!(
            "{}\n\n(the full answer is in this session's log)",
            &answer[..FINAL_CUT_BYTES]
        )
    );
}

/// S-17 in the room: an answer that quotes a secret reaches the room with
/// the log's redaction, in every edit and in the final one.
#[tokio::test(flavor = "multi_thread")]
async fn the_room_gets_the_logs_redaction() {
    let secret = "sk-proj4n7Qx2Lm9Vb8Rt6Yp3Kd1Zs";
    // The secret, then a pause long enough for a paced edit to carry it.
    let delta = |text: &str| json!({"model":"model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]});
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![vec![
            delta(&format!("The key is {secret}.")),
            json!({"pause_ms": 900}),
            delta(" Keep it safe."),
            json!({"model":"model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        ]],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "what is the key?").await);
    let edits = world
        .room
        .sent()
        .iter()
        .filter(|(_, content)| content["m.new_content"].is_object())
        .count();
    assert!(edits >= 2, "a paced edit before the final one");
    for (_, content) in world.room.sent() {
        assert!(!content.to_string().contains(secret), "{content}");
    }
    let message = final_edit(&world.room);
    assert!(
        message.starts_with("The key is [REDACTED secret-like: sha256:"),
        "{message}"
    );
    assert!(message.ends_with(" Keep it safe."), "{message}");
}

/// `status --session --no-probe`: arming for a read-only verb reaches no
/// provider; a turn's arming asks it what its model supports.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_arm_reaches_no_provider() {
    let world = world(ProviderKind::Ollama, &["drive_read"], vec![]);
    let served = world.open(SESSION);
    arm_agent(&served.context, &world.deps, Probe::Skip).await;
    assert_eq!(world.stub.hits.load(Ordering::SeqCst), 0);
    arm_agent(&served.context, &world.deps, Probe::Ask).await;
    assert!(world.stub.hits.load(Ordering::SeqCst) > 0, "a turn probes");
}

/// AD-364: core memory is read once per context; an edit on disk between two
/// turns does not move the prompt, and a new session sees it.
#[tokio::test(flavor = "multi_thread")]
async fn the_memory_snapshot_does_not_move_during_a_session() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let mut served = world.open(SESSION);
    let first = report(world.ask(&mut served, "one").await);
    write(
        &world.tgdrive,
        "80-agents/nixi/MEMORY.md",
        "tgorka now likes long answers.\n",
    );
    let second = report(world.ask(&mut served, "two").await);
    assert_eq!(first.prompt_sha256, second.prompt_sha256);
    assert_eq!(kinds(&world.lines(SESSION), LineKind::Open).len(), 1);

    let tg = world.deps.drives["tgdrive"].clone();
    session(&world.tgdrive, "active/2026-10-03-new", &tg);
    let mut fresh = world.open("active/2026-10-03-new");
    let third = report(world.ask(&mut fresh, "three").await);
    let memory = |path: &str| match &kinds(&world.lines(path), LineKind::Open)[0].body {
        LineBody::Open(open) => open.memory_sha256.clone(),
        _ => unreachable!(),
    };
    assert_ne!(memory(SESSION), memory("active/2026-10-03-new"));
    assert_ne!(first.prompt_sha256, third.prompt_sha256);
    assert!(world.stub.requests()[2]
        .to_string()
        .contains("long answers"));
}

/// R23: a 200 KiB answer is sent as its first `FINAL_CUT_BYTES` and a link;
/// the artifact holds all of it, as the log does.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_over_the_cut_is_cut_with_a_link_and_the_artifact_holds_all_of_it() {
    let answer = "ab".repeat(100 * 1024);
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose(&answer)]);
    let mut served = world.open(SESSION);
    let report = report(world.ask(&mut served, "write a lot").await);
    let artifact = format!("artifacts/answer-{}.md", report.user_line);
    let message = final_edit(&world.room);
    assert_eq!(
        message,
        format!(
            "{}\n\nThe full answer is in {artifact}",
            &answer[..FINAL_CUT_BYTES]
        )
    );
    let stored = std::fs::read_to_string(world.dir(SESSION).join(&artifact)).expect("artifact");
    assert_eq!(stored, answer);
    let dir = world.dir(SESSION);
    let lines = world.lines(SESSION);
    let last = kinds(&lines, LineKind::Assistant).pop().expect("assistant");
    let body = match &last.body {
        LineBody::Blob(blob) => {
            LineBody::decode(blob.kind, hydrate_blob(&dir, &blob.sha256).expect("blob"))
                .expect("body")
        }
        other => other.clone(),
    };
    let LineBody::Assistant(body) = body else {
        panic!("an assistant line")
    };
    assert_eq!(body.text, stored);
}

/// S-04: once a read of a `local_only` drive joins the label, nothing more
/// goes to a model that does not run locally; the same turn on a local
/// (`ollama`) model goes on.
#[tokio::test(flavor = "multi_thread")]
async fn a_remote_model_is_never_sent_a_local_only_label() {
    let script = || {
        vec![
            calls(&[(
                "d1",
                "drive_read",
                json!({"profile":"private","path":"diary.md"}),
            )]),
            prose("the diary says hello."),
        ]
    };
    let mut remote = world(ProviderKind::OpenAi, &["drive_read"], script());
    let mut served = remote.open(SESSION);
    let turn = report(remote.ask(&mut served, "read my diary").await);
    assert_eq!(turn.ending, TurnEnding::LocalOnly);
    assert_eq!(remote.stub.requests().len(), 1, "no request after the read");
    let lines = remote.lines(SESSION);
    let errors = kinds(&lines, LineKind::Error);
    let LineBody::Error(error) = &errors[0].body else {
        panic!("an error line")
    };
    assert_eq!(error.sentence, LOCAL_ONLY_REFUSAL);
    assert!(final_edit(&remote.room).ends_with(LOCAL_ONLY_REFUSAL));

    // The session was opened for tgorka and marta; the diary is tgorka's
    // alone, so the room gets one sentence and the log the answer (S-16).
    let mut local = world(ProviderKind::Ollama, &["drive_read"], script());
    let mut served = local.open(SESSION);
    let turn = report(local.ask(&mut served, "read my diary").await);
    assert_eq!(turn.ending, TurnEnding::Complete);
    assert_eq!(local.stub.requests().len(), 2);
    assert_eq!(final_edit(&local.room), NARROWER_THAN_ROOM);
    let lines = local.lines(SESSION);
    let LineBody::Assistant(answer) = &kinds(&lines, LineKind::Assistant)
        .last()
        .expect("answer")
        .body
    else {
        panic!("an assistant line")
    };
    assert_eq!(answer.text, "the diary says hello.");
}

/// 89.4's A1 through the turn: an inbox read lowers the integrity, a
/// `local_only` read sets `local_only`, each with a `label` line naming the
/// read, and the next frame states the narrowed readers.
#[tokio::test(flavor = "multi_thread")]
async fn every_read_and_message_joins_the_session_label() {
    let mut world = world(
        ProviderKind::Ollama,
        &["drive_read"],
        vec![
            calls(&[
                (
                    "i1",
                    "drive_read",
                    json!({"profile":"tgdrive","path":"00-inbox/x.md"}),
                ),
                (
                    "p1",
                    "drive_read",
                    json!({"profile":"private","path":"diary.md"}),
                ),
            ]),
            prose("read both."),
        ],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "read them").await);
    let lines = world.lines(SESSION);
    let causes: Vec<(String, Label)> = kinds(&lines, LineKind::Label)
        .iter()
        .map(|line| match &line.body {
            LineBody::Label(body) => (body.cause.reference.clone(), body.label()),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(causes[0].0, "tgdrive/00-inbox/x.md");
    assert_eq!(causes[0].1.integrity, Integrity::Untrusted);
    assert_eq!(causes[1].0, "private/diary.md");
    assert!(causes[1].1.local_only);
    assert_eq!(
        causes[1].1.readers,
        Readers::Only([user(TGORKA)].into_iter().collect())
    );
    assert_eq!(served.context.label, causes[1].1);
    let frame = served.context.compose(&world.deps, None).text;
    assert!(
        frame.contains(&format!("may be shown only to: {TGORKA}.")),
        "{frame}"
    );
    assert!(!frame.contains(MARTA), "{frame}");

    // The room was opened for marta too: nothing read from the diary reaches
    // it, streamed or final, and the log says why.
    for (_, content) in world.room.sent() {
        assert!(!content.to_string().contains("read both"), "{content}");
    }
    assert_eq!(final_edit(&world.room), NARROWER_THAN_ROOM);
    let errors = kinds(&lines, LineKind::Error);
    let LineBody::Error(why) = &errors.last().expect("an error line").body else {
        panic!("an error line")
    };
    assert_eq!(why.code, "label");
    assert_eq!(why.sentence, NARROWER_THAN_ROOM);
}

/// S-16: the status anchor carries counts, never a path a tool named.
#[tokio::test(flavor = "multi_thread")]
async fn tool_progress_carries_counts_not_paths() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_grep"],
        vec![
            calls(&[
                (
                    "r1",
                    "drive_read",
                    json!({"profile":"tgdrive","path":"notes/secret-plan.md"}),
                ),
                (
                    "g1",
                    "drive_grep",
                    json!({"profile":"tgdrive","path":"10-notes","needle":"needle"}),
                ),
            ]),
            prose("done."),
        ],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "look").await);
    let statuses: Vec<Value> = world
        .room
        .sent()
        .into_iter()
        .filter(|(kind, _)| kind == STATUS)
        .map(|(_, content)| content)
        .collect();
    assert!(!statuses.is_empty());
    for status in &statuses {
        let text = status.to_string();
        for forbidden in ["secret-plan", "notes/", "10-notes", "needle"] {
            assert!(!text.contains(forbidden), "{forbidden} in {text}");
        }
    }
    let last = statuses.last().expect("a status");
    assert_eq!(last["detail"], "reading 2 files, 2 tool calls");
    assert_eq!(last["run"], "idle");
}

/// AD-384: a turn's status says `running` from its start and `idle` only
/// once the answer's final edit is sent — tool calls or none — so a device
/// following a spoken question reads the status leaving `running` as the
/// answer being whole. The next turn edits the same status anchor.
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_is_running_until_its_final_edit_is_sent() {
    let mut world = world(
        ProviderKind::OpenAi,
        &[],
        vec![prose("The sky is blue."), prose("Still blue.")],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "sky?").await);
    let sent = world.room.sent();
    let anchor = sent
        .iter()
        .position(|(_, content)| content["dev.keeper.agent.turn"].is_object())
        .expect("the answer's anchor");
    let statuses: Vec<usize> = (0..sent.len()).filter(|&i| sent[i].0 == STATUS).collect();
    let (first, last) = (statuses[0], *statuses.last().expect("a status"));
    assert!(first > anchor, "{sent:?}");
    assert_eq!(sent[first].1["run"], "running");
    assert!(
        sent[first].1.get("anchor").is_none(),
        "the session's first status"
    );
    assert!(sent[first].1.get("detail").is_none(), "{}", sent[first].1);
    assert_eq!(
        last,
        sent.len() - 1,
        "the idle status is the turn's last send"
    );
    assert_eq!(sent[last].1["run"], "idle");
    let final_edit_at = (0..sent.len())
        .rev()
        .find(|&i| sent[i].1["m.relates_to"]["rel_type"] == "m.replace")
        .expect("the final edit");
    assert!(final_edit_at < last, "{sent:?}");
    assert_eq!(final_edit(&world.room), "The sky is blue.");
    let status_anchor = format!("$sent{}:example.org", first + 1);
    assert_eq!(sent[last].1["anchor"], status_anchor.as_str());

    report(world.ask(&mut served, "and now?").await);
    let again = world.room.sent();
    let running = (sent.len()..again.len())
        .find(|&i| again[i].0 == STATUS)
        .expect("the second turn's status");
    assert_eq!(again[running].1["run"], "running");
    assert_eq!(again[running].1["anchor"], status_anchor.as_str());
    assert_eq!(again.last().expect("a send").1["run"], "idle");
}

/// AD-384, review fix R5-F3: each answer's anchor names the person's
/// message it answers, so a device that asked follows its own question's
/// answer — not the one to a question queued before it.
#[tokio::test(flavor = "multi_thread")]
async fn an_answer_names_the_message_it_answers() {
    let mut world = world(
        ProviderKind::OpenAi,
        &[],
        vec![prose("One."), prose("Two.")],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "first?").await);
    report(world.ask(&mut served, "second?").await);
    let answered: Vec<Value> = world
        .room
        .sent()
        .iter()
        .filter_map(|(_, content)| content.get("dev.keeper.agent.turn").cloned())
        .map(|turn| turn["question"].clone())
        .collect();
    assert_eq!(
        answered,
        [json!("$in1:example.org"), json!("$in2:example.org")]
    );
}

fn messages_text(messages: &[keeper_core::bots::chat::ChatMessage]) -> String {
    format!("{messages:?}")
}

/// F2: after three turns with tool calls, one refused, the context in memory
/// equals a fresh replay of the files.
#[tokio::test(flavor = "multi_thread")]
async fn the_context_equals_a_fresh_replay() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"tgdrive","path":"notes/hello.md"}),
            )]),
            prose("It says first line."),
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/x.md","content":"no"}),
            )]),
            prose("Refused."),
            prose("Third."),
        ],
    );
    let mut served = world.open(SESSION);
    for question in ["read", "write", "again"] {
        report(world.ask(&mut served, question).await);
    }
    let dir = world.dir(SESSION);
    let fresh = replay(&read_session(&dir), &|sha| hydrate_blob(&dir, sha)).expect("replay");
    assert_eq!(
        messages_text(&served.context.messages),
        messages_text(&fresh.messages)
    );
    assert!(served.context.messages.len() >= 10);
}

/// NFR-116: once a session is served, a turn opens no file under `log/`;
/// opening it again (a restart) does.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread")]
async fn a_served_sessions_second_turn_opens_no_file_under_log() {
    use notify::event::{AccessKind, EventKind};
    use notify::{RecursiveMode, Watcher};

    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "first").await);
    let log = world.dir(SESSION).join("log");
    let (sender, opened) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event {
            if matches!(event.kind, EventKind::Access(AccessKind::Open(_))) {
                let _ = sender.send(event.paths);
            }
        }
    })
    .expect("inotify");
    watcher
        .watch(&log, RecursiveMode::Recursive)
        .expect("watch");
    let opens = |wait: Duration| {
        std::thread::sleep(wait);
        opened.try_iter().count()
    };
    assert_eq!(opens(Duration::from_millis(200)), 0);

    report(world.ask(&mut served, "second").await);
    assert_eq!(
        opens(Duration::from_millis(300)),
        0,
        "the second turn read the log"
    );

    drop(served);
    let _restarted = world.open(SESSION);
    assert!(
        opens(Duration::from_millis(300)) > 0,
        "a restart loads the log once more"
    );
}

/// The writer's lines replay in the order they were written, even when two
/// share a millisecond.
#[test]
fn lines_written_in_one_millisecond_keep_their_order() {
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let mut served = world.open(SESSION);
    let ServedSession {
        context, writer, ..
    } = &mut served;
    for n in 0..50 {
        writer
            .write(
                context,
                None,
                None,
                LineBody::User(UserBody {
                    sender: user(TGORKA),
                    text: format!("{n}"),
                    attachments: Vec::new(),
                }),
            )
            .expect("write");
    }
    let texts: Vec<String> = world
        .lines(SESSION)
        .iter()
        .map(|line| match &line.body {
            LineBody::User(body) => body.text.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(texts, (0..50).map(|n| n.to_string()).collect::<Vec<_>>());
}

// ---------------------------------------------------------------------------
// Claims (story 90.6)
// ---------------------------------------------------------------------------

fn lease(epoch: u64, event: &str) -> Arc<Lease> {
    Arc::new(Lease::new(
        epoch,
        OwnedEventId::try_from(event).expect("event"),
        1_790_000_000_000,
        None,
        keeper_agent::claims::Moment::now(),
    ))
}

/// A line `host` writes straight into its chunk, past any lease.
fn forced(dir: &Path, host: &str, epoch: u64, claim: &str, body: LineBody) {
    let mut chunks = ChunkWriter::open(
        dir,
        &HostSlug::new(host).expect("slug"),
        rotate_at(1 << 20),
        chrono::Utc::now().date_naive(),
    )
    .expect("chunk");
    let now = chrono::Utc::now();
    chunks
        .append(&LogLine {
            v: LINE_VERSION,
            id: ulid::Ulid::new(),
            parent: None,
            ts: chrono::DateTime::from_timestamp_millis(now.timestamp_millis()).expect("ts"),
            host: HostSlug::new(host).expect("slug"),
            epoch,
            claim: Some(claim.to_owned()),
            matrix_event: None,
            body,
        })
        .expect("append");
    std::thread::sleep(Duration::from_millis(3));
}

fn acquired(epoch: u64, event: &str) -> LineBody {
    LineBody::Claim(ClaimBody {
        epoch,
        action: ClaimAction::Acquired,
        from_host: None,
        claim_event: event.to_owned(),
        server_ts: "2026-10-03T12:00:00.000Z".to_owned(),
    })
}

/// S-05: every line a holder writes names the epoch and the claim event it
/// confirmed, its `claim acquired` line first.
#[tokio::test(flavor = "multi_thread")]
async fn every_line_carries_the_claims_epoch_and_event() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("hello.")]);
    let held = lease(3, "$claim3:example.org");
    let mut served = world
        .open_under(SESSION, Some(Arc::clone(&held)))
        .expect("served");
    served
        .writer
        .write_claim(
            &mut served.context,
            held.line(ClaimAction::Acquired, Some("hesperia".to_owned())),
        )
        .expect("claim line");
    report(world.ask(&mut served, "hi").await);

    let lines = world.lines(SESSION);
    assert!(lines.len() >= 4, "{lines:?}");
    match &lines[0].body {
        LineBody::Claim(body) => {
            assert_eq!(body.action, ClaimAction::Acquired);
            assert_eq!(body.claim_event, "$claim3:example.org");
            assert_eq!(body.from_host.as_deref(), Some("hesperia"));
            assert_eq!(body.server_ts, "2026-09-21T14:13:20.000Z");
        }
        other => panic!("the first line is the claim: {other:?}"),
    }
    for line in &lines {
        assert_eq!(line.epoch, 3, "{line:?}");
        assert_eq!(
            line.claim.as_deref(),
            Some("$claim3:example.org"),
            "{line:?}"
        );
    }
}

/// NFR-120 and the fence: a holder whose claim is lost writes nothing more,
/// and a line forced into its chunk after the taker's `acquired` is dropped
/// by every reader — the log reader, replay, and the taker's context.
#[tokio::test(flavor = "multi_thread")]
async fn a_lost_claims_late_line_is_dropped_by_every_reader() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("hello.")]);
    let electra = lease(1, "$e1:example.org");
    let mut served = world
        .open_under(SESSION, Some(Arc::clone(&electra)))
        .expect("served");
    served
        .writer
        .write_claim(
            &mut served.context,
            electra.line(ClaimAction::Acquired, None),
        )
        .expect("claim line");
    report(world.ask(&mut served, "before").await);

    // hesperia takes the session over at epoch 2.
    std::thread::sleep(Duration::from_millis(3));
    forced(
        &world.dir(SESSION),
        "hesperia",
        2,
        "$h2:example.org",
        acquired(2, "$h2:example.org"),
    );

    electra.lose();
    let refused = served.writer.write(
        &mut served.context,
        None,
        None,
        LineBody::User(UserBody {
            sender: user(TGORKA),
            text: "refused".to_owned(),
            attachments: Vec::new(),
        }),
    );
    assert!(matches!(refused, Err(WriterError::NoClaim)), "{refused:?}");
    // The `lost` line is still written: the fence never drops a transition.
    served
        .writer
        .write_claim(&mut served.context, electra.line(ClaimAction::Lost, None))
        .expect("lost line");

    // A stale writer that ignores its lease.
    forced(
        &world.dir(SESSION),
        "electra",
        1,
        "$e1:example.org",
        LineBody::User(UserBody {
            sender: user(TGORKA),
            text: "LATE".to_owned(),
            attachments: Vec::new(),
        }),
    );

    let log = read_session(&world.dir(SESSION));
    assert!(log
        .lines
        .iter()
        .all(|line| !format!("{line:?}").contains("LATE")));
    assert!(log
        .problems
        .iter()
        .any(|p| p.sentence.contains("newer epoch")));
    assert!(
        log.lines
            .iter()
            .any(|line| matches!(&line.body, LineBody::Claim(c) if c.action == ClaimAction::Lost)),
        "the lost line stays"
    );
    let dir = world.dir(SESSION);
    let replayed = replay(&log, &|sha| hydrate_blob(&dir, sha)).expect("replay");
    assert!(!messages_text(&replayed.messages).contains("LATE"));
    let taker = world
        .open_under(SESSION, Some(lease(2, "$h2:example.org")))
        .expect("taker");
    assert!(!messages_text(&taker.context.messages).contains("LATE"));
    assert!(messages_text(&taker.context.messages).contains("before"));
}

/// S-05: two `acquired` lines at one epoch with different claim events.
/// No host loads the session; its status is parked once with the sentence;
/// `status` names both events.
#[tokio::test(flavor = "multi_thread")]
async fn a_conflicted_session_is_served_by_no_host() {
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("never.")]);
    let dir = world.dir(SESSION);
    forced(
        &dir,
        "electra",
        2,
        "$a:example.org",
        acquired(2, "$a:example.org"),
    );
    forced(
        &dir,
        "hesperia",
        2,
        "$b:example.org",
        acquired(2, "$b:example.org"),
    );

    let refused = world.open_under(SESSION, Some(lease(3, "$c:example.org")));
    assert!(
        matches!(refused, Err(keeper_agent::agent::ServeOpenError::Load(_))),
        "no context is loaded"
    );
    let conflict = conflict_of(&dir).expect("conflicted");
    assert_eq!(conflict.epoch, 2);
    assert_eq!(
        conflict_line(&conflict),
        "conflicted at epoch 2: claim events $a:example.org and $b:example.org"
    );
    let base = StatusContent {
        v: 1,
        session: "60-sessions/active/2026-10-02-chat".to_owned(),
        kind: SessionKind::Conversation,
        title: "chat".to_owned(),
        agent: user("@nixi:example.org"),
        host: "electra".to_owned(),
        epoch: 0,
        run: RunState::Running,
        detail: None,
        waiting: None,
        anchor: None,
    };
    let parked = blocked_status(base, &conflict);
    assert_eq!(parked.run, RunState::Blocked);
    assert_eq!(parked.epoch, 2);
    assert_eq!(
        parked.detail.as_deref(),
        Some("Two hosts wrote this session at once (epoch 2). It waits for you.")
    );
    assert!(world.stub.requests().is_empty(), "no message was answered");
}

// ---------------------------------------------------------------------------
// Story 91.2: the one door, the scope, the focus, new conversations
// ---------------------------------------------------------------------------

const DM: &str = "active/2026-10-03-main";
const DELEGATED: &str = "active/2026-10-03-delegated";
const TOLAS: &str = "active/2026-10-03-tola";

impl World {
    /// `served` opened under `deps` rather than Nixi's.
    fn open_as(&self, deps: &AgentDeps, path: &str) -> ServedSession {
        let text = std::fs::read_to_string(self.dir(path).join("agent.toml")).expect("agent.toml");
        let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
        ServedSession::open(
            deps,
            &self.dir(path),
            SessionRef {
                drive: "tgdrive".to_owned(),
                path: path.to_owned(),
            },
            agent,
            None,
        )
        .expect("served")
    }

    /// An arrival of `arrival` from `sender` carrying `content`.
    fn event(&mut self, sender: &str, arrival: Arrival, content: Value) -> Arrived {
        let mut arrived = self.arrived(sender, "");
        arrived.arrival = arrival;
        arrived.content = content;
        arrived
    }

    fn scope(&mut self, sender: &str, drives: &[&str], focus: Option<Value>) -> Arrived {
        let mut content = json!({
            "v": 1,
            "set_by": sender,
            "drives": drives.iter().map(|id| json!({"id": id, "title": id})).collect::<Vec<_>>(),
        });
        if let Some(focus) = focus {
            content["focus"] = focus;
        }
        self.event(sender, Arrival::Scope { owner_signed: true }, content)
    }

    fn sent_of(&self, event_type: &str) -> Vec<Value> {
        self.room
            .sent()
            .into_iter()
            .filter(|(kind, _)| kind == event_type)
            .map(|(_, content)| content)
            .collect()
    }
}

/// Dr Tola Grey, a steward of tgdrive, beside Nixi: her own deps over the
/// same drives and provider.
fn tolas_deps(world: &World) -> AgentDeps {
    write(
        &world.tgdrive,
        "80-agents/tola/agent.toml",
        "version = 1\nid = \"tola\"\nname = \"Dr Tola Grey\"\nkind = \"steward\"\nmatrix_user = \"@tola:example.org\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"tgdrive\"]\n",
    );
    write(
        &world.tgdrive,
        "80-agents/tola/SOUL.md",
        &SOUL.replace("Nixi", "Dr Tola Grey"),
    );
    let tg_decl = world.deps.drives["tgdrive"].clone();
    let zone = read_zone(
        "tgdrive",
        &profile("tgdrive", &world.tgdrive),
        Some(&tg_decl),
    );
    let home = zone
        .homes
        .into_iter()
        .find_map(|(folder, home)| (folder == "tola").then_some(home))
        .expect("tola's folder")
        .expect("tola reads");
    AgentDeps {
        env: world.deps.env.clone(),
        data_dir: world.deps.data_dir.clone(),
        row: world.deps.row.clone(),
        bot: world.deps.bot.clone(),
        home,
        host: world.deps.host.clone(),
        drives: world.deps.drives.clone(),
        sessions_zone: world.deps.sessions_zone.clone(),
        sessions_subfolder: world.deps.sessions_subfolder.clone(),
        lfs_threshold_bytes: world.deps.lfs_threshold_bytes,
        decisions: None,
    }
}

/// 91.2 acceptance 1, the one door: a person's message in Nixi's DM and in
/// a second conversation with her opens a turn; the same message in Dr Tola
/// Grey's session room, in a delegated session and in the control room
/// opens none and writes no `user` line; Marta's message in Nixi's DM opens
/// none (she is not Nixi's `human`).
#[tokio::test(flavor = "multi_thread")]
async fn a_person_is_answered_only_by_their_proxy() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let tg_decl = world.deps.drives["tgdrive"].clone();
    session_of(
        &world.tgdrive,
        DM,
        &tg_decl,
        "nixi",
        SessionKind::Main,
        "!dm:example.org",
    );
    session_of(
        &world.tgdrive,
        DELEGATED,
        &tg_decl,
        "nixi",
        SessionKind::Delegated,
        "!delegated:example.org",
    );
    session_of(
        &world.tgdrive,
        TOLAS,
        &tg_decl,
        "tola",
        SessionKind::Main,
        "!tola:example.org",
    );
    let tola = tolas_deps(&world);

    for path in [DM, SESSION] {
        let mut served = world.open(path);
        assert!(
            matches!(world.ask(&mut served, "hello").await, Outcome::Answered(_)),
            "{path}"
        );
        assert_eq!(kinds(&world.lines(path), LineKind::User).len(), 1, "{path}");
    }

    let mut delegated = world.open(DELEGATED);
    assert!(matches!(
        world.ask(&mut delegated, "hello").await,
        Outcome::Ignored(OBSERVER_TEXT)
    ));
    let mut tolas = world.open_as(&tola, TOLAS);
    let hello = world.arrived(TGORKA, "hello");
    let (_stop, signal) = chat::cancellation();
    assert!(matches!(
        tolas
            .serve(&tola, world.room.clone(), hello, signal)
            .await
            .expect("served"),
        Outcome::Ignored(OBSERVER_TEXT)
    ));
    let mut dm = world.open(DM);
    let martas = world.arrived(MARTA, "hello nixi");
    assert!(matches!(
        world.serve(&mut dm, martas).await,
        Outcome::Ignored(NOT_THE_PERSON)
    ));
    for path in [DELEGATED, TOLAS] {
        assert!(world.lines(path).is_empty(), "{path}");
    }
    assert_eq!(kinds(&world.lines(DM), LineKind::User).len(), 1);

    // The control room names no session, so no worker serves it: what
    // arrives there is kept, bounded, for a worker that never starts.
    let router = Arc::new(Router::default());
    let control = OwnedRoomId::try_from("!control:example.org").expect("room");
    router.route(&control, world.arrived(TGORKA, "hello"));
    assert!(router.served().is_empty());
    assert_eq!(
        world.stub.requests().len(),
        2,
        "two turns, the DM's and the conversation's"
    );
}

/// 91.2 acceptance 3 (keeper-agent half): the person's scope within the
/// agent's `[tools].drives` (tgdrive and private here) is logged as one
/// `scope` line with `set_by` and echoed for the chips; a drive outside it
/// is refused by name in the status and changes nothing; a scope without
/// the home keeps the home; a scope from anyone but the person is ignored;
/// and the next turn reaches exactly the drives in scope.
#[tokio::test(flavor = "multi_thread")]
async fn drives_in_scope_are_the_persons_choice_within_the_agents_allow() {
    let read_both = || {
        calls(&[
            (
                "t1",
                "drive_read",
                json!({"profile":"tgdrive","path":"notes/hello.md"}),
            ),
            (
                "p1",
                "drive_read",
                json!({"profile":"private","path":"diary.md"}),
            ),
        ])
    };
    let mut world = world(
        ProviderKind::Ollama,
        &["drive_read"],
        vec![read_both(), prose("read."), read_both(), prose("read.")],
    );
    let mut served = world.open(SESSION);
    let scope_lines = |world: &World| -> Vec<(Vec<String>, OwnedUserId)> {
        kinds(&world.lines(SESSION), LineKind::Scope)
            .iter()
            .map(|line| match &line.body {
                LineBody::Scope(body) => (body.drives.clone(), body.set_by.clone()),
                _ => unreachable!(),
            })
            .collect()
    };

    // {tgdrive} alone: one line, one echo.
    let only_home = world.scope(TGORKA, &["tgdrive"], None);
    assert!(matches!(
        world.serve(&mut served, only_home).await,
        Outcome::Scoped(ref scope) if scope == &["tgdrive".to_owned()]
    ));
    assert_eq!(
        scope_lines(&world),
        vec![(vec!["tgdrive".to_owned()], user(TGORKA))]
    );
    let echoes = world.sent_of(SCOPE);
    assert_eq!(echoes.len(), 1);
    assert_eq!(
        echoes[0]["drives"],
        json!([{"id": "tgdrive", "title": "tgdrive"}])
    );
    assert_eq!(echoes[0]["set_by"], TGORKA);
    assert!(echoes[0]["label"].is_object(), "the label chip's source");

    // The next turn reaches tgdrive and is refused private.
    report(world.ask(&mut served, "read both").await);
    let outcomes = |world: &World| -> Vec<ToolOutcomeWord> {
        kinds(&world.lines(SESSION), LineKind::ToolResult)
            .iter()
            .map(|line| match &line.body {
                LineBody::ToolResult(body) => body.outcome,
                _ => unreachable!(),
            })
            .collect()
    };
    assert_eq!(
        outcomes(&world),
        [ToolOutcomeWord::Ok, ToolOutcomeWord::Refused]
    );
    let frame = served.context.compose(&world.deps, None).text;
    assert!(frame.contains("- tgdrive: tgdrive"), "{frame}");
    assert!(!frame.contains("- private: private"), "{frame}");

    // A drive outside the allow: refused by name, nothing changes.
    let marta_drive = world.scope(TGORKA, &["tgdrive", "marta-drive"], None);
    assert!(matches!(
        world.serve(&mut served, marta_drive).await,
        Outcome::ScopeRefused(ref sentence) if sentence.contains("marta-drive")
    ));
    let refused = world.sent_of(STATUS);
    let detail = refused.last().expect("a status")["detail"].clone();
    assert!(
        detail.as_str().is_some_and(|d| d.contains("marta-drive")),
        "{detail}"
    );
    assert_eq!(scope_lines(&world).len(), 1);
    assert_eq!(served.context.scope, ["tgdrive"]);

    // Marta's scope is not hers to set; an unsigned device's is ignored.
    let martas = world.scope(MARTA, &["tgdrive", "private"], None);
    assert!(matches!(
        world.serve(&mut served, martas).await,
        Outcome::Ignored(NOT_THE_PERSON)
    ));
    let mut unsigned = world.scope(TGORKA, &["tgdrive", "private"], None);
    unsigned.arrival = Arrival::Scope {
        owner_signed: false,
    };
    assert!(matches!(
        world.serve(&mut served, unsigned).await,
        Outcome::Ignored(UNSIGNED_DEVICE)
    ));
    assert_eq!(scope_lines(&world).len(), 1);

    // {private} without the home keeps the home: {tgdrive, private}.
    let without_home = world.scope(TGORKA, &["private"], None);
    assert!(matches!(
        world.serve(&mut served, without_home).await,
        Outcome::Scoped(ref scope) if scope == &["tgdrive".to_owned(), "private".to_owned()]
    ));
    assert_eq!(scope_lines(&world).len(), 2);
    report(world.ask(&mut served, "read both again").await);
    // Each turn changed the label — the person's message, then the diary's
    // read — and the chips were told, as after each `scope` line; but the
    // diary narrowed the label below the room, so that last change is not
    // echoed into a room Marta reads (R64).
    let echoes = world.sent_of(SCOPE);
    let labels: Vec<(Value, Value)> = echoes
        .iter()
        .map(|echo| {
            (
                echo["label"]["integrity"].clone(),
                echo["label"]["readers"].clone(),
            )
        })
        .collect();
    assert_eq!(labels.len(), 3, "{echoes:?}");
    assert_eq!(labels[0].0, "owner");
    assert_eq!(labels[1].0, "agent", "after the first turn");
    assert!(labels
        .iter()
        .all(|(_, readers)| *readers != json!([TGORKA])));

    // A host that restarts reads the scope from the log.
    drop(served);
    let mut again = world.open(SESSION);
    assert_eq!(again.context.scope, ["tgdrive", "private"]);

    // F5: read back on that start, a scope that changes nothing was echoed
    // when it first arrived and is not echoed again. Sent live it would be,
    // but this session read the diary: its label is below the room, so no
    // scope is echoed into it at all (R64).
    let echoed = world.sent_of(SCOPE).len();
    let mut replayed = world.scope(TGORKA, &["private"], None);
    replayed.replay = true;
    assert!(matches!(
        world.serve(&mut again, replayed).await,
        Outcome::Scoped(_)
    ));
    assert_eq!(world.sent_of(SCOPE).len(), echoed);
    let live = world.scope(TGORKA, &["private"], None);
    world.serve(&mut again, live).await;
    assert_eq!(world.sent_of(SCOPE).len(), echoed);
}

/// R41: the docked note's focus is held in memory and stated in the next
/// turn's frame — never logged, and never named when its drive is out of
/// scope; a scope event without a focus clears it.
#[tokio::test(flavor = "multi_thread")]
async fn the_focus_is_told_to_the_next_turn_and_never_logged() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let mut served = world.open(SESSION);
    let focus =
        json!({"drive": "tgdrive", "path": "notes/secret-plan.md", "heading": "Plans › Q3"});
    let focused = world.event(
        TGORKA,
        Arrival::Scope { owner_signed: true },
        json!({"v": 1, "set_by": TGORKA, "focus": focus}),
    );
    assert!(matches!(
        world.serve(&mut served, focused).await,
        Outcome::Focused
    ));
    report(world.ask(&mut served, "what am I reading?").await);
    let told =
        "The person is looking at notes/secret-plan.md in tgdrive, under the heading Plans › Q3.";
    let system = world.stub.requests()[0]["messages"][0]["content"].to_string();
    assert!(system.contains(told), "{system}");
    for line in world.lines(SESSION) {
        let json = serde_json::to_string(&line).expect("line");
        assert!(!json.contains("secret-plan"), "{json}");
    }
    assert!(kinds(&world.lines(SESSION), LineKind::Scope).is_empty());

    // Out of scope: held, not named.
    let hers = keeper_core::agents::events::Focus {
        drive: "marta-drive".to_owned(),
        path: "notes/hers.md".to_owned(),
        heading: None,
    };
    served.context.focus = Some(HeldFocus {
        focus: hers,
        heard: tokio::time::Instant::now(),
    });
    let frame = served.context.compose(&world.deps, None).text;
    assert!(!frame.contains("hers.md"), "{frame}");

    // D3: a focus not heard again within the TTL — the dock's clear was
    // lost with a quit or a crash — is no longer stated.
    let mine = keeper_core::agents::events::Focus {
        drive: "tgdrive".to_owned(),
        path: "notes/stale.md".to_owned(),
        heading: None,
    };
    let heard = tokio::time::Instant::now();
    served.context.focus = Some(HeldFocus {
        focus: mine.clone(),
        heard,
    });
    let frame = served.context.compose(&world.deps, None).text;
    assert!(frame.contains("stale.md"), "heard now: {frame}");
    served.context.focus = Some(HeldFocus {
        focus: mine,
        heard: heard
            .checked_sub(FOCUS_TTL + Duration::from_secs(1))
            .expect("an instant that long ago"),
    });
    let frame = served.context.compose(&world.deps, None).text;
    assert!(!frame.contains("stale.md"), "heard too long ago: {frame}");

    // A scope event without a focus clears it.
    let cleared = world.event(
        TGORKA,
        Arrival::Scope { owner_signed: true },
        json!({"v": 1, "set_by": TGORKA}),
    );
    world.serve(&mut served, cleared).await;
    assert_eq!(served.context.focus, None);
    let frame = served.context.compose(&world.deps, None).text;
    assert!(!frame.contains("The person is looking at"), "{frame}");
}

/// The agent's own rooms, as a recording fake: every room made, every event
/// sent into one, every room discarded; the person has joined once
/// `person_joined` is set.
#[derive(Default)]
struct Rooms {
    made: Mutex<Vec<(String, OwnedUserId, OwnedRoomId)>>,
    sent: Mutex<Vec<(OwnedRoomId, String, Value)>>,
    discarded: Mutex<Vec<OwnedRoomId>>,
    person_joined: std::sync::atomic::AtomicBool,
}

impl Rooms {
    fn statuses(&self) -> Vec<Value> {
        self.sent
            .lock()
            .expect("lock")
            .iter()
            .filter(|(_, kind, _)| kind == STATUS)
            .map(|(_, _, content)| content.clone())
            .collect()
    }
}

impl ConversationPort for Rooms {
    fn create<'a>(&'a self, title: &'a str, person: &'a UserId) -> RoomFuture<'a> {
        Box::pin(async move {
            let mut made = self.made.lock().expect("lock");
            let room = OwnedRoomId::try_from(format!("!conv{}:example.org", made.len() + 1))
                .expect("room");
            made.push((title.to_owned(), person.to_owned(), room.clone()));
            Ok(room)
        })
    }

    fn send<'a>(&'a self, room: &'a RoomId, event_type: &'a str, content: Value) -> SendFuture<'a> {
        Box::pin(async move {
            self.sent
                .lock()
                .expect("lock")
                .push((room.to_owned(), event_type.to_owned(), content));
            Ok(OwnedEventId::try_from("$anchor:example.org").expect("id"))
        })
    }

    fn discard<'a>(
        &'a self,
        room: &'a RoomId,
        _: &'a UserId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        Box::pin(async move { self.discarded.lock().expect("lock").push(room.to_owned()) })
    }

    fn joined<'a>(
        &'a self,
        _: &'a RoomId,
        _: &'a UserId,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>> {
        Box::pin(async move { self.person_joined.load(Ordering::SeqCst) })
    }
}

/// 91.2 acceptance 2 (keeper-agent half, R36): Nixi's person asking in the
/// DM makes one room and one `conversation` session folder naming it, with
/// a status anchor saying `kind: "conversation"`, and the DM is told; the
/// same request served again — after a restart too — makes nothing more;
/// anyone else's request, or one outside the DM, makes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_proxy_conversation_is_made_once() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let tg_decl = world.deps.drives["tgdrive"].clone();
    session_of(
        &world.tgdrive,
        DM,
        &tg_decl,
        "nixi",
        SessionKind::Main,
        "!dm:example.org",
    );
    let rooms = Arc::new(Rooms::default());
    let mut dm = world.open(DM);
    dm.conversations = Some(rooms.clone());
    let ask = world.event(
        TGORKA,
        Arrival::ConversationRequest { owner_signed: true },
        json!({"v": 1, "title": "Trip to Lisbon"}),
    );
    let Outcome::Conversation { path, made: true } = world.serve(&mut dm, ask.clone()).await else {
        panic!("a conversation")
    };
    let made = rooms.made.lock().expect("lock").clone();
    assert_eq!(made.len(), 1);
    // The room is named after the proxy: the title is in the encrypted
    // status, never in the room's clear state (F7).
    assert_eq!(made[0].0, "Nixi");
    assert_eq!(made[0].1, user(TGORKA));
    let text = std::fs::read_to_string(world.dir(&path).join("agent.toml")).expect("agent.toml");
    let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
    assert_eq!(agent.kind, SessionKind::Conversation);
    assert_eq!(agent.room, made[0].2);
    assert_eq!(agent.agent, "nixi");
    assert_eq!(agent.requested_by, user(TGORKA));
    assert_eq!(
        agent.id,
        keeper_core::agents::proxy::conversation_session_id("tgdrive", "nixi", &ask.event_id)
    );
    let sent = rooms.sent.lock().expect("lock").clone();
    assert_eq!(sent.len(), 1);
    assert_eq!((&sent[0].0, sent[0].1.as_str()), (&made[0].2, STATUS));
    assert_eq!(sent[0].2["kind"], "conversation");
    assert_eq!(sent[0].2["session"], format!("60-sessions/{path}"));
    assert_eq!(sent[0].2["title"], "Trip to Lisbon");
    let told = world.sent_of("m.room.message");
    assert_eq!(told.len(), 1);
    assert!(told[0]["body"]
        .as_str()
        .is_some_and(|b| b.contains("Trip to Lisbon")));

    // Served again, and again after a restart: nothing more is made.
    assert!(matches!(
        world.serve(&mut dm, ask.clone()).await,
        Outcome::Conversation { made: false, .. }
    ));
    drop(dm);
    let mut restarted = world.open(DM);
    restarted.conversations = Some(rooms.clone());
    assert!(matches!(
        world.serve(&mut restarted, ask).await,
        Outcome::Conversation { made: false, .. }
    ));
    // Marta, an unsigned device, and a request outside the DM make nothing.
    let martas = world.event(
        MARTA,
        Arrival::ConversationRequest { owner_signed: true },
        json!({"v": 1}),
    );
    assert!(matches!(
        world.serve(&mut restarted, martas).await,
        Outcome::Ignored(NOT_THE_PERSON)
    ));
    let unsigned = world.event(
        TGORKA,
        Arrival::ConversationRequest {
            owner_signed: false,
        },
        json!({"v": 1}),
    );
    assert!(matches!(
        world.serve(&mut restarted, unsigned).await,
        Outcome::Ignored(UNSIGNED_DEVICE)
    ));
    let mut conversation = world.open(SESSION);
    conversation.conversations = Some(rooms.clone());
    let elsewhere = world.event(
        TGORKA,
        Arrival::ConversationRequest { owner_signed: true },
        json!({"v": 1}),
    );
    assert!(matches!(
        world.serve(&mut conversation, elsewhere).await,
        Outcome::Ignored(_)
    ));
    assert_eq!(rooms.made.lock().expect("lock").len(), 1);
    assert!(rooms.discarded.lock().expect("lock").is_empty());
    let conversations = std::fs::read_dir(world.dir("active"))
        .expect("active")
        .filter(|entry| {
            entry
                .as_ref()
                .is_ok_and(|entry| entry.path().join("agent.toml").is_file())
        })
        .count();
    assert_eq!(
        conversations, 3,
        "the chat, the DM and one new conversation"
    );
}

/// F6: the new conversation's status anchor went out with the person only
/// invited, so a device of theirs the host did not know then can never read
/// it; once the person has joined, the host says the status again — an
/// edit of the anchor, the same title and kind — and only once.
#[tokio::test(start_paused = true)]
async fn a_new_conversation_says_its_status_again_once_its_person_joined() {
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let tg_decl = world.deps.drives["tgdrive"].clone();
    session_of(
        &world.tgdrive,
        DM,
        &tg_decl,
        "nixi",
        SessionKind::Main,
        "!dm:example.org",
    );
    let rooms = Arc::new(Rooms::default());
    let mut dm = world.open(DM);
    dm.conversations = Some(rooms.clone());
    let ask = world.event(
        TGORKA,
        Arrival::ConversationRequest { owner_signed: true },
        json!({"v": 1, "title": "Trip to Lisbon"}),
    );
    assert!(matches!(
        world.serve(&mut dm, ask).await,
        Outcome::Conversation { made: true, .. }
    ));
    tokio::time::sleep(JOIN_POLL * 5).await;
    assert_eq!(rooms.statuses().len(), 1, "nothing again before the join");

    rooms.person_joined.store(true, Ordering::SeqCst);
    tokio::time::sleep(JOIN_POLL * 2).await;
    let statuses = rooms.statuses();
    assert_eq!(statuses.len(), 2, "{statuses:?}");
    assert_eq!(statuses[1]["anchor"], "$anchor:example.org");
    assert_eq!(statuses[1]["kind"], "conversation");
    assert_eq!(statuses[1]["title"], "Trip to Lisbon");
    tokio::time::sleep(JOIN_POLL * 5).await;
    assert_eq!(rooms.statuses().len(), 2, "once");
}

// ---------------------------------------------------------------------------
// Story 91.3: the surface tools
// ---------------------------------------------------------------------------

const FIVE: [&str; 5] = [
    "surface_open",
    "surface_highlight",
    "surface_point",
    "surface_scroll",
    "surface_propose_edit",
];

/// The agent homed in tgdrive's `folder` with `agent_toml`, beside Nixi:
/// its own deps over the same drives and provider.
fn deps_of(world: &World, folder: &str, agent_toml: &str) -> AgentDeps {
    write(
        &world.tgdrive,
        &format!("80-agents/{folder}/agent.toml"),
        agent_toml,
    );
    let name = agent_toml
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == "name").then(|| value.trim())
        })
        .and_then(|value| value.strip_prefix('"')?.strip_suffix('"'))
        .expect("a name");
    write(
        &world.tgdrive,
        &format!("80-agents/{folder}/SOUL.md"),
        &SOUL.replace("Nixi", name),
    );
    let tg_decl = world.deps.drives["tgdrive"].clone();
    let zone = read_zone(
        "tgdrive",
        &profile("tgdrive", &world.tgdrive),
        Some(&tg_decl),
    );
    let home = zone
        .homes
        .into_iter()
        .find_map(|(found, home)| (found == folder).then_some(home))
        .expect("the folder")
        .expect("it reads");
    AgentDeps {
        home,
        env: world.deps.env.clone(),
        data_dir: world.deps.data_dir.clone(),
        row: world.deps.row.clone(),
        bot: world.deps.bot.clone(),
        host: world.deps.host.clone(),
        drives: world.deps.drives.clone(),
        sessions_zone: world.deps.sessions_zone.clone(),
        sessions_subfolder: world.deps.sessions_subfolder.clone(),
        lfs_threshold_bytes: world.deps.lfs_threshold_bytes,
        decisions: None,
    }
}

fn steward_toml(id: &str, name: &str, allow: &[&str]) -> String {
    let allow: Vec<String> = allow.iter().map(|a| format!("\"{a}\"")).collect();
    format!(
        "version = 1\nid = \"{id}\"\nname = \"{name}\"\nkind = \"steward\"\nmatrix_user = \"@{id}:example.org\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[tools]\nallow = [{}]\ndrives = [\"tgdrive\"]\n",
        allow.join(", ")
    )
}

/// The surface tools a turn of `deps`'s agent is offered, in a session of
/// its own.
async fn surface_offer(world: &World, deps: &AgentDeps) -> Vec<String> {
    let id = deps.home.config.id.clone();
    let path = format!("active/2026-10-03-{id}");
    let tg_decl = world.deps.drives["tgdrive"].clone();
    session_of(
        &world.tgdrive,
        &path,
        &tg_decl,
        &id,
        SessionKind::Delegated,
        "!offer:example.org",
    );
    let served = world.open_as(deps, &path);
    arm_agent(&served.context, deps, Probe::Skip)
        .await
        .request
        .tools
        .into_iter()
        .map(|spec| spec.name)
        .filter(|name| name.starts_with("surface_"))
        .collect()
}

/// 91.3 acceptance 8 (AD-383, Q10): Nixi (a proxy, audience {tgorka}) is
/// offered all five; Dr Tola Grey (a steward of the same drive) none by
/// default and all five when her `allow` names them; Dr Lucyna Novak, whose
/// audience is {tgorka, Marta}, none even when her `allow` names them.
#[tokio::test(flavor = "multi_thread")]
async fn surface_tools_are_offered_only_to_the_persons_own_agent() {
    let mut allow = vec!["drive_read"];
    allow.extend(FIVE);
    let mine = world_read_by(&[TGORKA], ProviderKind::OpenAi, &allow, vec![]);
    assert_eq!(surface_offer(&mine, &mine.deps).await, FIVE);
    let tola = deps_of(
        &mine,
        "tola",
        &steward_toml("tola", "Dr Tola Grey", &["drive_read", "card_update"]),
    );
    assert!(surface_offer(&mine, &tola).await.is_empty());
    let tola_allowed = deps_of(&mine, "tola", &steward_toml("tola", "Dr Tola Grey", &allow));
    assert_eq!(surface_offer(&mine, &tola_allowed).await, FIVE);

    let shared = world_read_by(&[TGORKA, MARTA], ProviderKind::OpenAi, &allow, vec![]);
    let lucyna = deps_of(
        &shared,
        "lucyna",
        &steward_toml("lucyna", "Dr Lucyna Novak", &allow),
    );
    assert!(surface_offer(&shared, &lucyna).await.is_empty());
    assert!(
        surface_offer(&shared, &shared.deps).await.is_empty(),
        "a proxy whose drive Marta reads is not tgorka's alone"
    );
}

/// The session room's side of a surface call: tgorka's iPhone in front, and
/// a device that answers each request `done` — after Marta answers it too.
struct Surface {
    room: OwnedRoomId,
    requests: Mutex<Vec<Value>>,
}

impl keeper_agent::surface::SurfacePort for Surface {
    fn room(&self) -> &RoomId {
        &self.room
    }

    fn presences(&self) -> keeper_agent::surface::PresenceFuture<'_> {
        use keeper_core::agents::events::PresencePlatform;
        use keeper_core::agents::presence::{presence_content, DevicePresence, Published};
        let now = u64::try_from(chrono::Utc::now().timestamp_millis()).expect("now");
        let state = DevicePresence {
            platform: PresencePlatform::Ios,
            focused: true,
            view: "notes".to_owned(),
        };
        let content = presence_content(&user(TGORKA), "KALYPSO", &state, now);
        Box::pin(async move {
            vec![Published {
                state_key: "KALYPSO".to_owned(),
                sender: user(TGORKA),
                content: serde_json::to_value(content).expect("presence"),
            }]
        })
    }

    fn request(&self, content: Value) -> SendFuture<'_> {
        self.requests.lock().expect("lock").push(content.clone());
        let room = self.room.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let answer =
                json!({"v": 1, "request": content["id"], "device": "KALYPSO", "outcome": "done"});
            // Marta's answer to the same request is nobody's.
            assert!(!keeper_agent::surface::deliver(
                &room,
                &user(MARTA),
                true,
                &answer
            ));
            assert!(keeper_agent::surface::deliver(
                &room,
                &user(TGORKA),
                true,
                &answer
            ));
        });
        Box::pin(async { Ok(OwnedEventId::try_from("$request:example.org").expect("id")) })
    }
}

/// 91.3: a surface call in a turn goes to the device in front, the turn
/// waits for its answer and the model reads it; the log has the call at T1,
/// its result, and the `surface` line naming the device.
#[tokio::test(flavor = "multi_thread")]
async fn a_surface_call_is_answered_by_the_device_and_logged() {
    let script = vec![
        calls(&[(
            "c1",
            "surface_highlight",
            json!({"drive": "tgdrive", "path": "notes/hello.md", "range": {"from": 2, "to": 2}}),
        )]),
        prose("Highlighted."),
    ];
    let mut world = world_read_by(
        &[TGORKA],
        ProviderKind::OpenAi,
        &["drive_read", "surface_highlight"],
        script,
    );
    let mut served = world.open(SESSION);
    let surface = Arc::new(Surface {
        room: room_id(),
        requests: Mutex::new(Vec::new()),
    });
    served.surface = Some(surface.clone());
    report(world.ask(&mut served, "show me the second line").await);

    let requests = surface.requests.lock().expect("lock").clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["device"], "KALYPSO");
    assert_eq!(requests[0]["tool"], "highlight");
    assert_eq!(requests[0]["args"]["range"], json!({"from": 2, "to": 2}));
    // The model's second round reads the device's answer.
    let second = &world.stub.requests()[1];
    let told = second["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("a tool result")
        .clone();
    assert!(
        told["content"] == "done" || told["content"][0]["text"] == "done",
        "{told}"
    );

    let lines = world.lines(SESSION);
    let call = kinds(&lines, LineKind::ToolCall);
    let LineBody::ToolCall(call) = &call[0].body else {
        panic!("a tool call");
    };
    assert_eq!((call.tool.as_str(), call.tier), ("surface_highlight", 1));
    let surfaced = kinds(&lines, LineKind::Surface);
    let LineBody::Surface(line) = &surfaced[0].body else {
        panic!("a surface line");
    };
    assert_eq!(line.id, requests[0]["id"].as_str().expect("id"));
    assert_eq!(line.device, "KALYPSO");
    assert_eq!(line.outcome.as_deref(), Some("done"));
}

// ---------------------------------------------------------------------------
// Story 92.1: delegation
// ---------------------------------------------------------------------------

/// A room a delegation made: its name, its invites, its agents at 50.
type MadeRoom = (String, Vec<OwnedUserId>, Vec<OwnedUserId>, OwnedRoomId);

/// The rooms a delegation goes through, as a recording fake: every room
/// made, every brief sent, who has joined which room, who else is in it,
/// what its timeline holds since the brief, and what fails.
#[derive(Default)]
struct Delegations {
    known: Mutex<Arc<Known>>,
    made: Mutex<Vec<MadeRoom>>,
    sent: Mutex<Vec<(OwnedRoomId, Value, String)>>,
    joined: Mutex<Vec<(OwnedRoomId, OwnedUserId)>>,
    watched: Mutex<Vec<(OwnedRoomId, OwnedRoomId)>>,
    /// `watch <room>` and `send <room>`, in order.
    ops: Mutex<Vec<String>>,
    /// People added to a room after it was made.
    added: Mutex<Vec<(OwnedRoomId, OwnedUserId)>>,
    /// The events since the newest brief, per room, oldest first.
    history: Mutex<Vec<(OwnedRoomId, Value)>>,
    /// Sends that fail before one succeeds, and every attempt's
    /// transaction id.
    failing_sends: std::sync::atomic::AtomicUsize,
    attempts: Mutex<Vec<String>>,
    /// Reads back that fail before one succeeds.
    failing_reads: std::sync::atomic::AtomicUsize,
    /// The room a brief arrives in, when not the one made by Nixi with
    /// Tola at 50.
    facts: Mutex<Option<BriefRoom>>,
}

impl Delegations {
    fn over(known: Known) -> Arc<Delegations> {
        Arc::new(Delegations {
            known: Mutex::new(Arc::new(known)),
            ..Delegations::default()
        })
    }

    fn made(&self) -> Vec<MadeRoom> {
        self.made.lock().expect("lock").clone()
    }

    fn sent(&self) -> Vec<(OwnedRoomId, Value, String)> {
        self.sent.lock().expect("lock").clone()
    }

    /// Who is in `room`: Nixi, who made it, everyone it invited, and anyone
    /// added since; Nixi and Tola in a room this fake did not make.
    fn people(&self, room: &RoomId) -> BTreeSet<OwnedUserId> {
        let mut people = BTreeSet::from([user(NIXI)]);
        match self.made().into_iter().find(|made| made.3 == room) {
            Some((_, invites, _, _)) => people.extend(invites),
            None => {
                people.insert(user(TOLA));
            }
        }
        people.extend(
            self.added
                .lock()
                .expect("lock")
                .iter()
                .filter(|(r, _)| r == room)
                .map(|(_, who)| who.clone()),
        );
        people
    }
}

/// Power levels as `events::power_levels` makes them for a delegated room
/// Nixi made with Tola at 50, with `changes` applied.
fn delegated_levels(changes: impl FnOnce(&mut Value)) -> RoomPowerLevels {
    let mut json = keeper_core::agents::events::power_levels(
        SessionKind::Delegated,
        &user(NIXI),
        &[user(TOLA)],
    );
    changes(&mut json);
    let content: RoomPowerLevelsEventContent = serde_json::from_value(json).expect("levels");
    RoomPowerLevels::new(
        content.into(),
        &AuthorizationRules::V1,
        Vec::<OwnedUserId>::new(),
    )
}

impl DelegationPort for Delegations {
    fn known(&self) -> Arc<Known> {
        Arc::clone(&self.known.lock().expect("lock"))
    }

    fn create<'a>(
        &'a self,
        name: &'a str,
        invite: Vec<OwnedUserId>,
        agents: Vec<OwnedUserId>,
    ) -> RoomFuture<'a> {
        Box::pin(async move {
            let mut made = self.made.lock().expect("lock");
            let room = OwnedRoomId::try_from(format!("!child{}:example.org", made.len() + 1))
                .expect("room");
            made.push((name.to_owned(), invite, agents, room.clone()));
            Ok(room)
        })
    }

    fn send<'a>(
        &'a self,
        room: &'a RoomId,
        content: Value,
        txn: OwnedTransactionId,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            self.attempts.lock().expect("lock").push(txn.to_string());
            let failing = &self.failing_sends;
            if failing.load(Ordering::SeqCst) > 0 {
                failing.fetch_sub(1, Ordering::SeqCst);
                return Err(keeper_core::agents::matrix::AgentMatrixError::Network(
                    "unreachable".to_owned(),
                ));
            }
            self.ops.lock().expect("lock").push(format!("send {room}"));
            let mut sent = self.sent.lock().expect("lock");
            sent.push((room.to_owned(), content, txn.to_string()));
            Ok(OwnedEventId::try_from(format!("$brief{}:example.org", sent.len())).expect("id"))
        })
    }

    fn joined<'a>(&'a self, room: &'a RoomId, user: &'a UserId) -> BoolFuture<'a> {
        Box::pin(async move {
            self.joined
                .lock()
                .expect("lock")
                .iter()
                .any(|(r, u)| r == room && u == user)
        })
    }

    fn members<'a>(&'a self, room: &'a RoomId) -> MembersFuture<'a> {
        Box::pin(async move { Ok(self.people(room)) })
    }

    fn since_brief<'a>(&'a self, room: &'a RoomId, _me: &'a UserId) -> EventsFuture<'a> {
        Box::pin(async move {
            let failing = &self.failing_reads;
            if failing.load(Ordering::SeqCst) > 0 {
                failing.fetch_sub(1, Ordering::SeqCst);
                return Err("messages: 502".to_owned());
            }
            Ok(self
                .history
                .lock()
                .expect("lock")
                .iter()
                .filter(|(r, _)| r == room)
                .map(|(_, event)| event.clone())
                .collect())
        })
    }

    fn brief_room<'a>(&'a self, room: &'a RoomId) -> BriefRoomFuture<'a> {
        Box::pin(async move {
            if let Some(facts) = self.facts.lock().expect("lock").clone() {
                return Some(facts);
            }
            Some(BriefRoom {
                room_type: Some(SESSION_ROOM_TYPE.to_owned()),
                creators: vec![user(NIXI)],
                levels: Some(delegated_levels(|_| {})),
                members: self.people(room),
            })
        })
    }

    fn watch(&self, child: &RoomId, parent: &RoomId) {
        self.ops
            .lock()
            .expect("lock")
            .push(format!("watch {child}"));
        self.watched
            .lock()
            .expect("lock")
            .push((child.to_owned(), parent.to_owned()));
    }
}

const NIXI: &str = "@nixi:example.org";
const TOLA: &str = "@tola:example.org";
const LUCYNA: &str = "@lucyna:example.org";

fn known_agent(drive: &str, id: &str, name: &str, user_id: &str, readers: &[&str]) -> KnownAgent {
    let readers = Readers::Only(readers.iter().map(|r| user(r)).collect());
    KnownAgent {
        id: id.to_owned(),
        drive: drive.to_owned(),
        name: name.to_owned(),
        matrix_user: user(user_id),
        kind: keeper_core::agents::home::AgentKind::Steward,
        human: None,
        hosted: false,
        home_readers: readers.clone(),
        opening: Label {
            readers,
            ..Label::top()
        },
        drives: vec![drive.to_owned()],
    }
}

/// Nixi and Dr Tola Grey in tgdrive (read by `readers`), and Dr Lucyna
/// Novak in neuradrive, read by tgorka and Marta.
fn known(readers: &[&str]) -> Known {
    Known {
        agents: vec![
            known_agent("tgdrive", "nixi", "Nixi", NIXI, readers),
            known_agent("tgdrive", "tola", "Dr Tola Grey", TOLA, readers),
            known_agent(
                "neuradrive",
                "lucyna",
                "Dr Lucyna Novak",
                LUCYNA,
                &[TGORKA, MARTA],
            ),
        ],
        trust: Vec::new(),
    }
}

fn tolas(world: &World, allow: &[&str]) -> AgentDeps {
    deps_of(world, "tola", &steward_toml("tola", "Dr Tola Grey", allow))
}

fn delegate_call(id: &str, args: Value) -> Completion {
    calls(&[(id, "delegate", args)])
}

fn hand_inbox() -> Completion {
    delegate_call(
        "d1",
        json!({"agent": "tgdrive/tola", "brief": "Sort the inbox.", "card": {"title": "Inbox"}}),
    )
}

/// The `delegate` lines of `lines`, in order.
fn delegate_lines(lines: &[LogLine]) -> Vec<DelegateBody> {
    kinds(lines, LineKind::Delegate)
        .iter()
        .map(|line| match &line.body {
            LineBody::Delegate(body) => body.clone(),
            _ => unreachable!(),
        })
        .collect()
}

fn tool_results(lines: &[LogLine]) -> Vec<ToolResultBody> {
    kinds(lines, LineKind::ToolResult)
        .iter()
        .map(|line| match &line.body {
            LineBody::ToolResult(body) => body.clone(),
            _ => unreachable!(),
        })
        .collect()
}

fn card_field(world: &World, path: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(
        world
            .dir(path)
            .join(keeper_core::agents::delegation::CARD_FILE),
    )
    .expect("the card");
    keeper_core::notes::frontmatter::Frontmatter::parse(&text)
        .0
        .as_string(key)
        .map(str::to_owned)
}

impl World {
    /// Nixi's session, served with `rooms` for its delegations.
    fn delegating(&self, rooms: &Arc<Delegations>) -> ServedSession {
        let mut served = self.open(SESSION);
        served.delegations = Some(rooms.clone() as Arc<dyn DelegationPort>);
        served
    }

    /// The target's join of the room Nixi made, routed to her session.
    fn joined(&mut self, child: &OwnedRoomId) -> Arrived {
        let mut arrived = self.event(TOLA, Arrival::Joined, json!({"membership": "join"}));
        arrived.via = Some(child.clone());
        arrived
    }

    /// The brief `content` (as sent) arriving in its room, from Nixi.
    fn brief(&mut self, content: &Value) -> Arrived {
        let mut arrived = self.event(NIXI, Arrival::Brief, content.clone());
        arrived.text = content["body"].as_str().unwrap_or_default().to_owned();
        arrived
    }

    /// Make the session `brief` opens for `target` in `room`, as the placed
    /// host does: its zone-relative path.
    fn create_child(
        &self,
        target: &AgentDeps,
        room: &OwnedRoomId,
        brief: &DelegateContent,
    ) -> String {
        keeper_agent::hosts::create_delegated(
            &self.deps.sessions_zone,
            &target.home.config,
            room,
            brief,
            chrono::Utc::now(),
        )
        .expect("created");
        keeper_agent::sessions::verbs::find(&self.deps.sessions_zone, &brief.id)
            .expect("the child session")
            .path
    }

    /// Tola's session at `path`, served with `rooms` as her host serves it.
    fn child(&self, tola: &AgentDeps, path: &str, rooms: &Arc<Delegations>) -> ServedSession {
        let mut served = self.open_as(tola, path);
        served.delegations = Some(rooms.clone() as Arc<dyn DelegationPort>);
        served
    }

    /// Tola's reply `content` in `child`, as the host routes it to Nixi.
    fn reply(&mut self, child: &OwnedRoomId, content: &Value) -> Arrived {
        self.next += 1;
        let event = json!({
            "type": "m.room.message",
            "sender": TOLA,
            "event_id": format!("$reply{}:example.org", self.next),
            "content": content,
        });
        reply_of(&event, &user(TOLA), child, tokio::time::Instant::now()).expect("a reply")
    }
}

/// Serve `arrived` in Tola's session `served` under her deps, into `room`.
async fn serve_as(
    tola: &AgentDeps,
    served: &mut ServedSession,
    room: &Arc<Room>,
    arrived: Arrived,
) -> Outcome {
    let (_stop, signal) = chat::cancellation();
    served
        .serve(tola, room.clone(), arrived, signal)
        .await
        .expect("served")
}

/// Nixi hands the inbox to Dr Tola Grey and Tola joins: the brief, as sent.
async fn handed_over(
    world: &mut World,
    rooms: &Arc<Delegations>,
) -> (ServedSession, OwnedRoomId, Value) {
    let mut nixi = world.delegating(rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);
    let child = rooms.made()[0].3.clone();
    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut nixi, joined).await,
        Outcome::BriefSent(_)
    ));
    let brief = rooms.sent().last().expect("the brief").1.clone();
    (nixi, child, brief)
}

fn relabel(world: &World, path: &str, integrity: Integrity) -> Label {
    let file = world.dir(path).join("agent.toml");
    let text = std::fs::read_to_string(&file).expect("agent.toml");
    let mut agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
    agent.label.integrity = integrity;
    std::fs::write(&file, compose_session_agent_toml(&agent)).expect("write");
    agent.label
}

/// 92.1 acceptance 2, 5 and 7: Nixi (tgdrive) delegating to Dr Tola Grey
/// (tgdrive) makes a room inviting exactly Tola and the label's readers,
/// Tola at 50; nothing goes in until Tola joins, and the session says it
/// waits; then one brief, once, however often the join is delivered; and
/// the session Tola's host makes from it is hers — `kind = "delegated"`,
/// her id, Nixi as requester and parent, Nixi's session label (not
/// tgdrive's opening) — with one card, `run: queued`.
#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_makes_a_session_in_the_targets_home() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let label = relabel(&world, SESSION, Integrity::Agent);
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);

    let made = rooms.made();
    assert_eq!(made.len(), 1);
    let (name, invites, agents, child) = made[0].clone();
    assert!(name.starts_with("tola 20"), "{name}");
    assert_eq!(invites, vec![user(TOLA), user(MARTA), user(TGORKA)]);
    assert_eq!(agents, vec![user(TOLA)]);
    assert!(rooms.sent().is_empty(), "nothing before the join");
    let waiting = world.sent_of(STATUS);
    assert_eq!(
        waiting.last().expect("a status")["detail"],
        "waiting for Dr Tola Grey to join"
    );
    let opened = delegate_lines(&world.lines(SESSION));
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].state, DelegateState::Opened);
    assert_eq!(opened[0].room.as_ref(), Some(&child));

    // The join, twice: one brief, sent under the delegation's id.
    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut nixi, joined.clone()).await,
        Outcome::BriefSent(_)
    ));
    assert!(matches!(
        world.serve(&mut nixi, joined).await,
        Outcome::Duplicate
    ));
    let sent = rooms.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, child);
    assert_eq!(sent[0].2, opened[0].id);
    assert_eq!(sent[0].1["body"], "Sort the inbox.");
    let brief = read_brief(&sent[0].1).expect("a brief");
    assert_eq!(brief.label, label);
    assert_eq!(brief.from.session, SESSION);
    assert_eq!(brief.hop, 1);
    assert_eq!(
        delegate_lines(&world.lines(SESSION))
            .iter()
            .map(|line| line.state)
            .collect::<Vec<_>>(),
        [DelegateState::Opened, DelegateState::Sent]
    );

    let path = world.create_child(&tola, &child, &brief);
    let text = std::fs::read_to_string(world.dir(&path).join("agent.toml")).expect("agent.toml");
    let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
    assert_eq!(agent.kind, SessionKind::Delegated);
    assert_eq!(agent.agent, "tola");
    assert_eq!(agent.requested_by, user(NIXI));
    let parent = agent.parent.expect("a parent");
    assert_eq!((parent.session.as_str(), parent.room), (SESSION, room_id()));
    assert_eq!(agent.label, label);
    assert_eq!(agent.room, child);
    // R76: tgorka started it in Nixi's conversation; Nixi handed it on.
    assert_eq!(agent.dispatch_chain, vec![user(TGORKA), user(NIXI)]);
    assert_eq!(brief.dispatch_chain, agent.dispatch_chain);
    assert_eq!(
        card_field(&world, &path, "assignee").as_deref(),
        Some("tola")
    );
    assert_eq!(
        card_field(&world, &path, "requested_by").as_deref(),
        Some(NIXI)
    );
    assert_eq!(card_field(&world, &path, "run").as_deref(), Some("queued"));
}

/// 92.1 acceptance 6: Tola's `reply` sends one message carrying its
/// artifact's `{drive, path}` and her session's label, sets her card
/// `run: review`; Nixi's session logs `delegate replied`, gets a `peer` line
/// naming the artifact and runs a turn — in which her own `reply`, outside
/// a delegated session, is refused. The model is told who replied and which
/// files it handed over, in that turn and after a reload.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_closes_the_exchange() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            calls(&[(
                "r1",
                "reply",
                json!({"text": "Inbox sorted.", "artifacts": ["artifacts/report.md"]}),
            )]),
            prose("Replied."),
            calls(&[("r2", "reply", json!({"text": "me too"}))]),
            prose("Tola sorted it."),
        ],
    );
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, brief) = handed_over(&mut world, &rooms).await;
    let content = read_brief(&brief).expect("a brief");
    let path = world.create_child(&tola, &child, &content);
    write(&world.dir(&path), "artifacts/report.md", "# Inbox\n");

    let mut tolas_session = world.child(&tola, &path, &rooms);
    let tolas_room = Arc::new(Room::default());
    let arrived = world.brief(&brief);
    report(serve_as(&tola, &mut tolas_session, &tolas_room, arrived).await);
    let reply = tolas_room
        .sent()
        .into_iter()
        .find(|(_, content)| content["dev.keeper.agent.artifacts"].is_array())
        .expect("a reply")
        .1;
    assert_eq!(reply["body"], "Inbox sorted.");
    assert_eq!(
        keeper_agent::delegate::reply_label(&reply),
        Some(tolas_session.context.label.clone()),
        "the reply carries the session's label"
    );
    let artifact = format!("60-sessions/{path}/artifacts/report.md");
    assert_eq!(
        reply["dev.keeper.agent.artifacts"],
        json!([{"drive": "tgdrive", "path": artifact}])
    );
    // The reply's one row names the room it went into (R90).
    let replies: Vec<_> = audit_list(&world, &tolas_session)
        .into_iter()
        .filter(|row| row.tool == "reply")
        .collect();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(
        replies[0].subpath,
        tolas_session.context.agent.room.as_str()
    );
    assert_eq!(
        replies[0].outcome,
        keeper_core::bots::audit::AuditOutcome::Ok
    );
    assert_eq!(card_field(&world, &path, "run").as_deref(), Some("review"));
    let childs = world.lines(&path);
    assert_eq!(delegate_lines(&childs)[0].state, DelegateState::Accepted);
    let LineBody::Peer(peer) = &kinds(&childs, LineKind::Peer)[0].body else {
        panic!("a peer line");
    };
    assert_eq!(
        (peer.sender.as_str(), peer.text.as_str()),
        (NIXI, "Sort the inbox.")
    );
    assert!(kinds(&childs, LineKind::Run).iter().any(|line| matches!(
        &line.body,
        LineBody::Run(run) if run.state == keeper_core::agents::log::RunState::Review
    )));

    // The reply reaches Nixi's session, as the host routes it.
    let replied = world.reply(&child, &reply);
    report(world.serve(&mut nixi, replied.clone()).await);
    assert!(matches!(
        world.serve(&mut nixi, replied).await,
        Outcome::Duplicate
    ));
    let lines = world.lines(SESSION);
    assert_eq!(
        delegate_lines(&lines).last().expect("a line").state,
        DelegateState::Replied
    );
    let LineBody::Peer(peer) = &kinds(&lines, LineKind::Peer)[0].body else {
        panic!("a peer line");
    };
    assert_eq!(
        (peer.sender.as_str(), peer.text.as_str()),
        (TOLA, "Inbox sorted.")
    );
    assert_eq!(peer.artifacts, Some(vec![format!("tgdrive/{artifact}")]));
    let refused = tool_results(&lines);
    let last = refused.last().expect("a result");
    assert_eq!(last.outcome, ToolOutcomeWord::Refused);
    assert!(last.content.contains(NOT_DELEGATED), "{}", last.content);
    let told =
        format!("From {TOLA}:\\nInbox sorted.\\n\\nFiles handed over:\\n- tgdrive/{artifact}");
    let requests = world.stub.requests();
    assert_eq!(requests.len(), 6);
    assert!(requests[4].to_string().contains(&told), "{}", requests[4]);

    // After a reload the replayed conversation says the same.
    drop(nixi);
    let mut again = world.delegating(&rooms);
    report(world.ask(&mut again, "what did Tola send?").await);
    let requests = world.stub.requests();
    assert!(requests[6].to_string().contains(&told), "{}", requests[6]);
}

/// 92.1 acceptance 4: Nixi's {tgorka} session handing work to Dr Lucyna
/// Novak, whose audience adds Marta, is refused naming Marta: no room, no
/// invite, no event, and a `delegate refused` line with the reason.
#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_beyond_the_label_is_refused_before_the_room_exists() {
    let mut world = world_read_by(
        &[TGORKA],
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            delegate_call(
                "d1",
                json!({"agent": "neuradrive/lucyna", "brief": "Summarise my diary."}),
            ),
            prose("I could not."),
        ],
    );
    let rooms = Delegations::over(known(&[TGORKA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "ask Lucyna").await);
    assert!(rooms.made().is_empty());
    assert!(rooms.sent().is_empty());
    assert!(rooms.watched.lock().expect("lock").is_empty());
    let lines = world.lines(SESSION);
    let refused = delegate_lines(&lines);
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].state, DelegateState::Refused);
    assert_eq!(refused[0].room, None);
    let reason = refused[0].reason.clone().expect("a reason");
    assert!(reason.contains(MARTA), "{reason}");
    let result = &tool_results(&lines)[0];
    assert_eq!(result.outcome, ToolOutcomeWord::Refused);
    assert!(result.content.contains(MARTA));
}

/// 92.1 acceptance 8, the hand-off: a card with a schedule needs a person,
/// so before Epic 93 the call gets `UNATTENDED_REFUSAL` and nothing reaches
/// the homeserver.
#[tokio::test(flavor = "multi_thread")]
async fn a_delegated_schedule_is_refused_before_epic_93() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            delegate_call(
                "d1",
                json!({"agent": "tola", "brief": "Every morning.", "card": {"title": "Digest", "schedule": "@daily"}}),
            ),
            prose("It needs you."),
        ],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "a daily digest from Tola").await);
    assert!(rooms.made().is_empty());
    assert!(rooms.sent().is_empty());
    let lines = world.lines(SESSION);
    let result = &tool_results(&lines)[0];
    assert_eq!(result.content, format!("Refused: {UNATTENDED_REFUSAL}"));
    assert_eq!(delegate_lines(&lines)[0].state, DelegateState::Refused);
}

/// 92.1 acceptance 8, inside the child: a write that would ask is refused
/// with `UNATTENDED_REFUSAL` and logged; the file is never written.
#[tokio::test(flavor = "multi_thread")]
async fn an_action_needing_a_person_in_a_delegated_session_is_refused() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile": "tgdrive", "path": "notes/sorted.md", "content": "must not land"}),
            )]),
            prose("That needs tgorka."),
        ],
    );
    let tola = tolas(&world, &["drive_read", "drive_write"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (_nixi, child, brief) = handed_over(&mut world, &rooms).await;
    let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let arrived = world.brief(&brief);
    report(
        serve_as(
            &tola,
            &mut tolas_session,
            &Arc::new(Room::default()),
            arrived,
        )
        .await,
    );
    assert!(!world.tgdrive.join("notes/sorted.md").exists());
    let result = &tool_results(&world.lines(&path))[0];
    assert_eq!(result.outcome, ToolOutcomeWord::Refused);
    assert_eq!(result.content, format!("Refused: {UNATTENDED_REFUSAL}"));
}

/// 92.1 acceptance 1, the rounds: Nixi's fourth message in one exchange is
/// refused and she is told which bound; Tola's session, three rounds in
/// without a reply, parks its card `run: blocked` with detail `rounds`, and
/// a fourth brief that reaches it anyway is not a turn. Nixi names the
/// delegation only by what her `delegate` result told her model.
#[tokio::test(flavor = "multi_thread")]
async fn a_fourth_round_parks_the_requester() {
    let rounds: Vec<(String, Value)> = (2..=4)
        .map(|n| {
            (
                format!("m{n}"),
                json!({"agent": "tola", "brief": format!("And round {n}."), "session": DELEGATION}),
            )
        })
        .collect();
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            calls(
                &rounds
                    .iter()
                    .map(|(call, args)| (call.as_str(), "delegate", args.clone()))
                    .collect::<Vec<_>>(),
            ),
            prose("Told her."),
            prose("Working on it."),
            prose("Still working."),
            prose("Nearly."),
        ],
    );
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, brief) = handed_over(&mut world, &rooms).await;

    report(world.ask(&mut nixi, "say more to Tola").await);
    let results = tool_results(&world.lines(SESSION));
    let outcomes: Vec<ToolOutcomeWord> = results.iter().map(|r| r.outcome).collect();
    assert_eq!(
        &outcomes[outcomes.len() - 3..],
        [
            ToolOutcomeWord::Ok,
            ToolOutcomeWord::Ok,
            ToolOutcomeWord::Refused
        ]
    );
    let told = &results.last().expect("a result").content;
    assert!(told.contains("3 rounds"), "{told}");
    let sent = rooms.sent();
    assert_eq!(sent.len(), 3, "the brief and two more rounds");
    let last = delegate_lines(&world.lines(SESSION)).pop().expect("a line");
    assert_eq!(last.state, DelegateState::Refused);

    // Tola's session hears the three rounds and replies to none.
    let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let room = Arc::new(Room::default());
    for (n, (_, content, _)) in sent.iter().enumerate() {
        let arrived = world.brief(content);
        report(serve_as(&tola, &mut tolas_session, &room, arrived).await);
        let run = card_field(&world, &path, "run");
        if n < 2 {
            assert_eq!(run.as_deref(), Some("queued"), "round {}", n + 1);
        } else {
            assert_eq!(run.as_deref(), Some("blocked"));
        }
    }
    let parked = kinds(&world.lines(&path), LineKind::Run)
        .iter()
        .filter_map(|line| match &line.body {
            LineBody::Run(run) => run.detail.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(parked, ["rounds"]);

    // A fourth brief, however it got there, is not a turn.
    let asked = world.stub.requests().len();
    let fourth = world.brief(&sent[2].1);
    assert!(matches!(
        serve_as(&tola, &mut tolas_session, &room, fourth).await,
        Outcome::Ignored(ROUNDS_SPENT)
    ));
    assert_eq!(world.stub.requests().len(), asked);
}

/// A reply the tool refused — missing text, a file outside `artifacts/` —
/// is no reply: it does not close the exchange, so three rounds still park
/// the card. Only a reply that went out does (its `delegate replied` line).
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_reply_keeps_the_rounds_counted() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            calls(&[("r1", "reply", json!({"artifacts": ["artifacts/report.md"]}))]),
            prose("No text."),
            calls(&[(
                "r2",
                "reply",
                json!({"text": "Here.", "artifacts": ["../agent.toml"]}),
            )]),
            prose("Bad path."),
            prose("Still nothing."),
        ],
    );
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (_nixi, child, brief) = handed_over(&mut world, &rooms).await;
    let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let room = Arc::new(Room::default());
    for n in 0..3 {
        let arrived = world.brief(&brief);
        report(serve_as(&tola, &mut tolas_session, &room, arrived).await);
        assert_eq!(tolas_session.context.exchange_rounds, n + 1);
    }
    let refused: Vec<ToolOutcomeWord> = tool_results(&world.lines(&path))
        .iter()
        .map(|result| result.outcome)
        .collect();
    assert_eq!(
        refused,
        [ToolOutcomeWord::Refused, ToolOutcomeWord::Refused]
    );
    assert!(room
        .sent()
        .iter()
        .all(|(_, content)| !content["dev.keeper.agent.artifacts"].is_array()));
    assert_eq!(card_field(&world, &path, "run").as_deref(), Some("blocked"));
}

/// 92.1 acceptance 1, the budget (R69): a scripted provider reports 1200
/// tokens on the first round of a child whose budget is 1000; the run stops
/// after that step, the round's `assistant` line carries its usage, and the
/// reply names what was spent and the bound; the card goes `run: blocked`.
#[tokio::test(flavor = "multi_thread")]
async fn the_token_budget_stops_the_run_and_says_what_it_spent() {
    let mut round = calls(&[(
        "r1",
        "drive_read",
        json!({"profile": "tgdrive", "path": "notes/hello.md"}),
    )]);
    round.push(json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 200, "total_tokens": 1200}}));
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![round, prose("never asked")],
    );
    let tola = tolas(&world, &["drive_read"]);
    let content = DelegateContent {
        v: 1,
        id: ulid::Ulid::new().to_string(),
        from: keeper_core::agents::delegation::DelegateFrom {
            agent: user(NIXI),
            drive: "tgdrive".to_owned(),
            session: SESSION.to_owned(),
            room: room_id(),
        },
        to: user(TOLA),
        brief: "Read hello.".to_owned(),
        drives: vec!["tgdrive".to_owned()],
        label: Label::opening(&world.deps.drives["tgdrive"], Integrity::Agent),
        hop: 1,
        limits: keeper_core::agents::delegation::DelegateLimits {
            rounds_per_exchange: 3,
            tokens: 1000,
        },
        card: None,
        dispatch_chain: Vec::new(),
    };
    let child = OwnedRoomId::try_from("!child:example.org").expect("room");
    let path = world.create_child(&tola, &child, &content);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let tolas_room = Arc::new(Room::default());
    let arrived = world.brief(&keeper_core::agents::delegation::brief_content(&content));
    let ran = report(serve_as(&tola, &mut tolas_session, &tolas_room, arrived).await);
    assert_eq!(ran.ending, TurnEnding::Bounded);
    assert_eq!(world.stub.requests().len(), 1, "stopped after that step");
    let lines = world.lines(&path);
    let round = kinds(&lines, LineKind::Assistant)
        .iter()
        .find_map(|line| match &line.body {
            LineBody::Assistant(body) if body.finish == "tool_calls" => Some(body.usage),
            _ => None,
        })
        .expect("the round's line");
    assert_eq!((round.prompt, round.completion), (Some(1000), Some(200)));
    let reply = tolas_room
        .sent()
        .into_iter()
        .find(|(_, content)| content["dev.keeper.agent.artifacts"].is_array())
        .expect("a reply")
        .1;
    let said = reply["body"].as_str().expect("text");
    assert!(said.contains("1200") && said.contains("1000"), "{said}");
    assert_eq!(card_field(&world, &path, "run").as_deref(), Some("blocked"));
    let LineBody::Error(error) = &kinds(&lines, LineKind::Error)[0].body else {
        panic!("an error line");
    };
    assert_eq!(error.code, "tokens");
}

/// The budget crossed by a turn's last completion — no tool round, so no
/// gate before another request — parks the session as the gate would: the
/// reply names the bound, the card goes `run: blocked`, once. After a
/// reload the spent budget still holds: another brief is not a turn.
#[tokio::test(flavor = "multi_thread")]
async fn a_budget_crossed_by_the_last_completion_parks_the_session() {
    let mut answer = prose("Read it.");
    answer.push(json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 200, "total_tokens": 1200}}));
    let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![answer]);
    let tola = tolas(&world, &["drive_read"]);
    let content = DelegateContent {
        v: 1,
        id: ulid::Ulid::new().to_string(),
        from: keeper_core::agents::delegation::DelegateFrom {
            agent: user(NIXI),
            drive: "tgdrive".to_owned(),
            session: SESSION.to_owned(),
            room: room_id(),
        },
        to: user(TOLA),
        brief: "Read hello.".to_owned(),
        drives: vec!["tgdrive".to_owned()],
        label: Label::opening(&world.deps.drives["tgdrive"], Integrity::Agent),
        hop: 1,
        limits: keeper_core::agents::delegation::DelegateLimits {
            rounds_per_exchange: 3,
            tokens: 1000,
        },
        card: None,
        dispatch_chain: Vec::new(),
    };
    let child = OwnedRoomId::try_from("!child:example.org").expect("room");
    let path = world.create_child(&tola, &child, &content);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let tolas_room = Arc::new(Room::default());
    let brief = keeper_core::agents::delegation::brief_content(&content);
    let arrived = world.brief(&brief);
    let ran = report(serve_as(&tola, &mut tolas_session, &tolas_room, arrived).await);
    assert_eq!(ran.ending, TurnEnding::Complete);
    let replies: Vec<Value> = tolas_room
        .sent()
        .into_iter()
        .filter(|(_, content)| content["dev.keeper.agent.artifacts"].is_array())
        .map(|(_, content)| content)
        .collect();
    assert_eq!(replies.len(), 1, "{replies:?}");
    let said = replies[0]["body"].as_str().expect("text");
    assert!(said.contains("1200") && said.contains("1000"), "{said}");
    assert_eq!(card_field(&world, &path, "run").as_deref(), Some("blocked"));
    let blocked = kinds(&world.lines(&path), LineKind::Run)
        .iter()
        .filter_map(|line| match &line.body {
            LineBody::Run(run) => run.detail.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocked, ["tokens"]);

    // Reloaded, the session has still spent its budget.
    drop(tolas_session);
    let mut again = world.child(&tola, &path, &rooms);
    assert!(again.context.token_bound().is_some());
    let asked = world.stub.requests().len();
    let next = world.brief(&brief);
    assert!(matches!(
        serve_as(&tola, &mut again, &tolas_room, next).await,
        Outcome::Ignored(BUDGET_SPENT)
    ));
    assert_eq!(world.stub.requests().len(), asked);
}

/// 92.1 acceptance 11 (R29 F5), the delegating half: no brief exists until
/// the target's join; a host that restarts after the join sends it exactly
/// once, and a join delivered after that sends nothing more.
#[tokio::test(flavor = "multi_thread")]
async fn the_brief_is_sent_only_after_the_target_joins() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);
    let child = rooms.made()[0].3.clone();
    assert!(nixi.resume_delegations(&world.deps).await.is_empty());
    assert!(rooms.sent().is_empty(), "not joined: nothing sent");

    // The host stops; Tola joins; the host starts again.
    drop(nixi);
    rooms
        .joined
        .lock()
        .expect("lock")
        .push((child.clone(), user(TOLA)));
    let mut restarted = world.delegating(&rooms);
    restarted.resume_delegations(&world.deps).await;
    restarted.resume_delegations(&world.deps).await;
    assert_eq!(rooms.sent().len(), 1, "once");
    assert!(rooms
        .watched
        .lock()
        .expect("lock")
        .iter()
        .any(|(c, parent)| *c == child && *parent == room_id()));
    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut restarted, joined).await,
        Outcome::Ignored(_)
    ));
    assert_eq!(rooms.sent().len(), 1);
}

/// The `delegate` and `reply` specs a turn of `served` is offered.
async fn delegation_offer(served: &ServedSession, deps: &AgentDeps) -> Vec<String> {
    arm_agent(&served.context, deps, Probe::Skip)
        .await
        .request
        .tools
        .into_iter()
        .map(|spec| spec.name)
        .filter(|name| name == "delegate" || name == "reply")
        .collect()
}

/// R48: `delegate` is offered as `[tools].allow` says; `reply` in a
/// delegated session whatever it says, and in no other.
#[tokio::test(flavor = "multi_thread")]
async fn delegate_follows_the_allow_and_reply_the_session_kind() {
    let offer = delegation_offer;
    let world = world(ProviderKind::OpenAi, &["drive_read", "delegate"], vec![]);
    let nixi = world.open(SESSION);
    assert_eq!(offer(&nixi, &world.deps).await, ["delegate"]);
    let tola = tolas(&world, &["drive_read"]);
    let tg_decl = world.deps.drives["tgdrive"].clone();
    session_of(
        &world.tgdrive,
        TOLAS,
        &tg_decl,
        "tola",
        SessionKind::Delegated,
        "!tola:example.org",
    );
    let delegated = world.open_as(&tola, TOLAS);
    assert_eq!(offer(&delegated, &tola).await, ["reply"]);
    session_of(
        &world.tgdrive,
        DM,
        &tg_decl,
        "tola",
        SessionKind::Main,
        "!dm:example.org",
    );
    let main = world.open_as(&tola, DM);
    assert!(offer(&main, &tola).await.is_empty());
}

/// R55: a `peer` line a crash left unanswered — a brief, a delegation's
/// reply — is closed on restart like a `user` line, never run again.
#[tokio::test(flavor = "multi_thread")]
async fn an_unanswered_peer_line_is_closed_after_a_restart() {
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    {
        let mut served = world.open(SESSION);
        let ServedSession {
            context, writer, ..
        } = &mut served;
        writer
            .write(
                context,
                None,
                None,
                LineBody::Peer(keeper_core::agents::log::PeerBody {
                    sender: user(TOLA),
                    text: "The inbox is sorted.".to_owned(),
                    ask: None,
                    artifacts: None,
                }),
            )
            .expect("write");
        writer.sync().expect("sync");
    }
    let mut again = world.open(SESSION);
    assert!(again
        .recover(&world.deps, world.room.clone(), &Trail::default())
        .await
        .expect("recover"));
    assert_eq!(kinds(&world.lines(SESSION), LineKind::Error).len(), 1);
    assert!(world.stub.requests().is_empty(), "never run again");
}

/// Narrow `served`'s label to `readers`, as a read of a file only they may
/// read would.
fn narrow(served: &mut ServedSession, readers: &[&str]) {
    use keeper_core::agents::label::{LabelBody, LabelCause, LabelCauseKind};
    let label = Label {
        readers: Readers::Only(readers.iter().map(|r| user(r)).collect()),
        ..served.context.label.clone()
    };
    let ServedSession {
        context, writer, ..
    } = served;
    writer
        .write(
            context,
            None,
            None,
            LineBody::Label(LabelBody::new(
                &label,
                LabelCause {
                    kind: LabelCauseKind::ToolResult,
                    reference: "a read".to_owned(),
                },
            )),
        )
        .expect("a label line");
}

/// R93 in the session that serves a brief: the admission the live intake
/// and the read-back use — a creator demoted since is no agent, and a brief
/// naming another parent room is not this session's. Neither is a turn,
/// and no model is asked; the genuine brief is.
#[tokio::test(flavor = "multi_thread")]
async fn a_brief_the_admission_refuses_is_not_a_turn_in_the_child() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on."), prose("On it.")],
    );
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (_nixi, child, brief) = handed_over(&mut world, &rooms).await;
    let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    let room = Arc::new(Room::default());
    let asked = world.stub.requests().len();

    *rooms.facts.lock().expect("lock") = Some(BriefRoom {
        room_type: Some(SESSION_ROOM_TYPE.to_owned()),
        creators: vec![user(NIXI)],
        levels: Some(delegated_levels(|json| json["users"][NIXI] = json!(0))),
        members: rooms.people(&child),
    });
    let demoted = world.brief(&brief);
    assert!(matches!(
        serve_as(&tola, &mut tolas_session, &room, demoted).await,
        Outcome::Ignored(NO_AGENT_POWER)
    ));
    *rooms.facts.lock().expect("lock") = None;
    let mut elsewhere = read_brief(&brief).expect("a brief");
    elsewhere.from.room = OwnedRoomId::try_from("!other:example.org").expect("room");
    let foreign = world.brief(&keeper_core::agents::delegation::brief_content(&elsewhere));
    assert!(matches!(
        serve_as(&tola, &mut tolas_session, &room, foreign).await,
        Outcome::Ignored(NOT_THIS_DELEGATION)
    ));
    assert_eq!(world.stub.requests().len(), asked);
    assert!(kinds(&world.lines(&path), LineKind::Peer).is_empty());

    let genuine = world.brief(&brief);
    report(serve_as(&tola, &mut tolas_session, &room, genuine).await);
}

/// R94: the label is checked again when the brief goes in, not only when
/// the room was made. Nixi's session read something only tgorka may read
/// between opening the room for tgorka and Marta and Tola's join: the brief
/// is never sent, the refusal is logged naming Marta, and the delegation
/// is over.
#[tokio::test(flavor = "multi_thread")]
async fn a_brief_the_label_no_longer_lets_in_is_never_sent() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);
    let child = rooms.made()[0].3.clone();
    narrow(&mut nixi, &[TGORKA]);

    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut nixi, joined).await,
        Outcome::Ignored(BRIEF_REFUSED)
    ));
    assert!(rooms.sent().is_empty());
    let refused = delegate_lines(&world.lines(SESSION)).pop().expect("a line");
    assert_eq!(refused.state, DelegateState::Refused);
    assert!(
        refused.reason.as_deref().is_some_and(|r| r.contains(MARTA)),
        "{refused:?}"
    );
    assert!(
        nixi.context.delegations.is_empty(),
        "the delegation is over"
    );
}

/// R94: someone invited into the delegation's room after it was made is
/// one more reader of the brief; the label decides with them in it.
#[tokio::test(flavor = "multi_thread")]
async fn a_person_added_to_the_room_since_it_opened_blocks_the_brief() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);
    let child = rooms.made()[0].3.clone();
    let mallory = "@mallory:example.org";
    rooms
        .added
        .lock()
        .expect("lock")
        .push((child.clone(), user(mallory)));
    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut nixi, joined).await,
        Outcome::Ignored(BRIEF_REFUSED)
    ));
    assert!(rooms.sent().is_empty());
    let refused = delegate_lines(&world.lines(SESSION)).pop().expect("a line");
    assert!(
        refused
            .reason
            .as_deref()
            .is_some_and(|r| r.contains(mallory)),
        "{refused:?}"
    );
}

/// R94 on the rounds after the first and on the reply: a next round after
/// Nixi's session read something only tgorka may read, and a reply from a
/// Tola who read the same, are refused naming Marta — nothing is sent, and
/// Tola's refused reply leaves her exchange open.
#[tokio::test(flavor = "multi_thread")]
async fn a_round_or_a_reply_the_label_no_longer_lets_in_is_refused() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            delegate_call(
                "m2",
                json!({"agent": "tola", "brief": "And the plan.", "session": DELEGATION}),
            ),
            prose("I could not."),
            calls(&[("r1", "reply", json!({"text": "Done."}))]),
            prose("Refused."),
        ],
    );
    let tola = tolas(&world, &["drive_read"]);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, brief) = handed_over(&mut world, &rooms).await;
    narrow(&mut nixi, &[TGORKA]);
    report(world.ask(&mut nixi, "tell Tola the plan").await);
    let round = tool_results(&world.lines(SESSION)).pop().expect("a result");
    assert_eq!(round.outcome, ToolOutcomeWord::Refused);
    assert!(round.content.contains(MARTA), "{}", round.content);
    assert_eq!(rooms.sent().len(), 1, "the brief alone");
    let last = delegate_lines(&world.lines(SESSION)).pop().expect("a line");
    assert_eq!(
        (last.state, last.room.as_ref()),
        (DelegateState::Refused, Some(&child))
    );

    let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
    let mut tolas_session = world.child(&tola, &path, &rooms);
    narrow(&mut tolas_session, &[TGORKA]);
    let room = Arc::new(Room::default());
    let arrived = world.brief(&brief);
    report(serve_as(&tola, &mut tolas_session, &room, arrived).await);
    assert!(room
        .sent()
        .iter()
        .all(|(_, content)| !content["dev.keeper.agent.artifacts"].is_array()));
    let reply = tool_results(&world.lines(&path)).pop().expect("a result");
    assert_eq!(reply.outcome, ToolOutcomeWord::Refused);
    assert!(reply.content.contains(MARTA), "{}", reply.content);
    assert_eq!(tolas_session.context.exchange_rounds, 1);
    // R90: each blocked call is one classified row, never a sink row
    // beside it; the reply's names the room it was bound for.
    let delegates: Vec<_> = audit_list(&world, &nixi)
        .into_iter()
        .filter(|row| row.tool == "delegate")
        .collect();
    assert_eq!(delegates.len(), 2, "{delegates:?}");
    assert!(delegates.iter().all(|row| row.tier == Some(1)));
    assert_eq!(
        delegates[1].outcome,
        keeper_core::bots::audit::AuditOutcome::Refused
    );
    let replies: Vec<_> = audit_list(&world, &tolas_session)
        .into_iter()
        .filter(|row| row.tool == "reply")
        .collect();
    assert_eq!(replies.len(), 1, "{replies:?}");
    assert_eq!(
        (replies[0].tier, replies[0].subpath.as_str()),
        (Some(1), child.as_str())
    );
}

/// R94 at the delegating side: a reply's label is joined into the session
/// it comes back to. Tola read something local-only and outside content,
/// for tgorka alone, and replied to the same people: Nixi's session is now
/// local-only, untrusted and tgorka's alone, so the reply never reaches
/// Nixi's remote model. A reply carrying no label is not taken.
#[tokio::test(flavor = "multi_thread")]
async fn a_replys_label_narrows_the_delegating_session() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, _) = handed_over(&mut world, &rooms).await;
    let unlabelled = json!({
        "type": "m.room.message",
        "sender": TOLA,
        "event_id": "$bare:example.org",
        "content": {"msgtype": "m.text", "body": "Done.", "dev.keeper.agent.artifacts": []},
    });
    assert!(reply_of(
        &unlabelled,
        &user(TOLA),
        &child,
        tokio::time::Instant::now()
    )
    .is_none());

    let read = Label {
        readers: Readers::Only(BTreeSet::from([user(TGORKA)])),
        integrity: Integrity::Untrusted,
        local_only: true,
    };
    let replied = world.reply(
        &child,
        &reply_content("Your diary says Friday.", Vec::new(), &read),
    );
    let asked = world.stub.requests().len();
    let ran = report(world.serve(&mut nixi, replied).await);
    assert_eq!(ran.ending, TurnEnding::LocalOnly);
    assert_eq!(world.stub.requests().len(), asked, "no remote model saw it");
    let label = &nixi.context.label;
    assert!(label.local_only);
    assert_eq!(label.integrity, Integrity::Untrusted);
    assert_eq!(label.readers, read.readers);
}

/// R55 across a restart: a child that already replied stays watched, so
/// the next round's answer comes back to the delegating session and runs
/// its turn; the round registers its room before it is sent. The round
/// names the delegation by the id the replayed `delegate` result carries.
#[tokio::test(flavor = "multi_thread")]
async fn a_replied_child_is_continued_after_a_restart() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            hand_inbox(),
            prose("Handed on."),
            prose("Tola sorted it."),
            delegate_call(
                "m2",
                json!({"agent": "tola", "brief": "And the archive.", "session": DELEGATION}),
            ),
            prose("Asked again."),
            prose("Tola sorted that too."),
        ],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, _) = handed_over(&mut world, &rooms).await;
    let label = nixi.context.label.clone();
    let first = world.reply(&child, &reply_content("Sorted.", Vec::new(), &label));
    report(world.serve(&mut nixi, first).await);

    // The host stops; a fresh copy routes nothing yet.
    drop(nixi);
    rooms.watched.lock().expect("lock").clear();
    rooms.ops.lock().expect("lock").clear();
    let mut again = world.delegating(&rooms);
    assert!(again.resume_delegations(&world.deps).await.is_empty());
    assert!(rooms
        .watched
        .lock()
        .expect("lock")
        .contains(&(child.clone(), room_id())));

    rooms.ops.lock().expect("lock").clear();
    report(world.ask(&mut again, "ask Tola about the archive").await);
    assert_eq!(
        *rooms.ops.lock().expect("lock"),
        [format!("watch {child}"), format!("send {child}")]
    );
    let second = world.reply(&child, &reply_content("Archived.", Vec::new(), &label));
    report(world.serve(&mut again, second).await);
    let states: Vec<DelegateState> = delegate_lines(&world.lines(SESSION))
        .iter()
        .map(|line| line.state)
        .collect();
    assert_eq!(
        states,
        [
            DelegateState::Opened,
            DelegateState::Sent,
            DelegateState::Replied,
            DelegateState::Sent,
            DelegateState::Replied
        ]
    );
}

/// A crash between a reply's receipt and its `peer` line loses nothing.
/// The log a crash leaves after the receipt's append — the receipt and no
/// more — is restored on the next start: the `peer` line is written from
/// the receipt, its label joined, and closed as interrupted; the reply's
/// event stays seen, and the next turn's model reads the reply.
#[tokio::test(flavor = "multi_thread")]
async fn a_reply_whose_peer_line_was_lost_is_restored_from_its_receipt() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let (mut nixi, child, _) = handed_over(&mut world, &rooms).await;
    let id = nixi.context.delegations.keys().next().expect("one").clone();
    let event = OwnedEventId::try_from("$reply9:example.org").expect("event");
    let read = Label {
        readers: Readers::Only(BTreeSet::from([user(TGORKA)])),
        ..nixi.context.label.clone()
    };
    {
        let ServedSession {
            context, writer, ..
        } = &mut nixi;
        writer
            .write(
                context,
                None,
                Some(event.clone()),
                LineBody::Delegate(DelegateBody {
                    id,
                    to: TOLA.to_owned(),
                    room: Some(child.clone()),
                    child: None,
                    state: DelegateState::Replied,
                    reason: None,
                    reply: Some(DelegateReply {
                        text: "Sorted.".to_owned(),
                        artifacts: vec!["tgdrive/60-sessions/x/artifacts/report.md".to_owned()],
                        label: read.clone(),
                    }),
                }),
            )
            .expect("the receipt");
        writer.sync().expect("sync");
    }
    drop(nixi);

    let mut again = world.delegating(&rooms);
    assert!(again
        .recover(&world.deps, world.room.clone(), &Trail::default())
        .await
        .expect("recover"));
    let lines = world.lines(SESSION);
    let LineBody::Peer(peer) = &kinds(&lines, LineKind::Peer).last().expect("a peer").body else {
        unreachable!()
    };
    assert_eq!(
        *peer,
        PeerBody {
            sender: user(TOLA),
            text: "Sorted.".to_owned(),
            ask: None,
            artifacts: Some(vec!["tgdrive/60-sessions/x/artifacts/report.md".to_owned()]),
        }
    );
    assert_eq!(again.context.label.readers, read.readers);
    let LineBody::Error(closed) = &kinds(&lines, LineKind::Error).last().expect("closed").body
    else {
        unreachable!()
    };
    assert_eq!(closed.code, "interrupted");

    let redelivered = reply_of(
        &json!({
            "type": "m.room.message",
            "sender": TOLA,
            "event_id": event.as_str(),
            "content": reply_content("Sorted.", Vec::new(), &read),
        }),
        &user(TOLA),
        &child,
        tokio::time::Instant::now(),
    )
    .expect("a reply");
    assert!(matches!(
        world.serve(&mut again, redelivered).await,
        Outcome::Duplicate
    ));
    report(world.ask(&mut again, "what did Tola say?").await);
    let last = world.stub.requests().pop().expect("a request").to_string();
    assert!(last.contains("From @tola:example.org:\\nSorted."), "{last}");
}

/// A brief whose send failed while its target sat joined is not left
/// waiting for a join that will not come again: while the worker runs, its
/// clock sends it again under the same transaction id, once it can.
#[tokio::test(flavor = "multi_thread")]
async fn a_brief_that_failed_to_send_is_sent_again_on_the_clock() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![hand_inbox(), prose("Handed on.")],
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    rooms.failing_sends.store(1, Ordering::SeqCst);
    let mut nixi = world.delegating(&rooms);
    report(world.ask(&mut nixi, "hand the inbox to Tola").await);
    let child = rooms.made()[0].3.clone();
    let joined = world.joined(&child);
    assert!(matches!(
        world.serve(&mut nixi, joined).await,
        Outcome::Ignored(BRIEF_UNSENT)
    ));
    assert!(rooms.sent().is_empty());

    let (keep, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (_stop, signal) = chat::cancellation();
    let busy = keeper_agent::agent::Activity::default();
    let _ = tokio::time::timeout(
        Duration::from_secs(3),
        nixi.serve_arrivals(
            &world.deps,
            world.room.clone(),
            Vec::new(),
            &mut arrivals,
            signal,
            &busy,
        ),
    )
    .await;
    drop(keep);
    let id = delegate_lines(&world.lines(SESSION))[0].id.clone();
    assert_eq!(rooms.sent().len(), 1);
    assert_eq!(*rooms.attempts.lock().expect("lock"), [id.clone(), id]);
    assert_eq!(
        delegate_lines(&world.lines(SESSION))
            .iter()
            .map(|line| line.state)
            .collect::<Vec<_>>(),
        [DelegateState::Opened, DelegateState::Sent]
    );
}

/// 92.3 AC6 with no decision source (93.2 AC7): a scheduled run is a turn
/// whose brief is the card's body, opened by `run: running` with the window
/// as `last_run`; an action in it that needs a person gets
/// `UNATTENDED_REFUSAL` as its tool result and is logged; the card ends
/// `review`; and the same window routed again is no second turn. With a
/// source it parks instead: `parks::a_scheduled_run_parks_and_holds_its_next_window`.
#[tokio::test(flavor = "multi_thread")]
async fn a_scheduled_run_refuses_what_needs_a_person_without_a_decision_source() {
    use keeper_agent::agent::scheduled_arrival;
    use keeper_agent::cards::Scheduled;
    use keeper_core::agents::card::{CardAgent, Field, Run};
    use keeper_core::agents::log::RunState as LogRun;
    const SCHEDULED: &str = "active/2026-10-05-sort";
    let world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/new.md","content":"must not land"}),
            )]),
            prose("I could not write it."),
        ],
    );
    session_of(
        &world.tgdrive,
        SCHEDULED,
        &decl("tgdrive", &[TGORKA, MARTA], false),
        "nixi",
        SessionKind::Scheduled,
        "!sort:example.org",
    );
    let card = "---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\nassignee: nixi\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nWrite a note about what came in.\n";
    write(
        &world.tgdrive,
        &format!("60-sessions/{SCHEDULED}/card.md"),
        card,
    );
    let mut served = world.open(SCHEDULED);
    let window = "2026-10-05T09:00:00.000Z";
    let arrival = || {
        scheduled_arrival(
            &user("@nixi:example.org"),
            &Scheduled::Run {
                card: "card.md".to_owned(),
                window: window.to_owned(),
                now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-05T09:30:00Z")
                    .expect("an instant")
                    .timestamp_millis(),
                utc_offset_minutes: 0,
            },
        )
        .expect("an arrival")
    };
    let report = report(world.serve(&mut served, arrival()).await);
    assert_eq!(report.ending, TurnEnding::Complete);
    assert!(!world.tgdrive.join("notes/new.md").exists());

    let lines = world.lines(SCHEDULED);
    let runs: Vec<LogRun> = kinds(&lines, LineKind::Run)
        .iter()
        .map(|line| match &line.body {
            LineBody::Run(body) => body.state,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(runs, [LogRun::Running, LogRun::Review]);
    let peers = kinds(&lines, LineKind::Peer);
    let LineBody::Peer(peer) = &peers[0].body else {
        panic!("a peer line")
    };
    assert_eq!(peer.text, "Write a note about what came in.");
    let result = kinds(&lines, LineKind::ToolResult);
    let LineBody::ToolResult(body) = &result[0].body else {
        panic!("a tool result")
    };
    assert_eq!(body.content, format!("Refused: {UNATTENDED_REFUSAL}"));
    let text = std::fs::read_to_string(world.dir(SCHEDULED).join("card.md")).expect("card");
    let keys = CardAgent::of_text(&text).expect("keys");
    assert_eq!(keys.run, Some(Field::Read(Run::Review)), "{text}");
    assert!(
        matches!(&keys.last_run, Some(Field::Read(at)) if at.to_rfc3339_opts(chrono::SecondsFormat::Millis, true) == window),
        "{text}"
    );

    // The host's clock routing the same window again is the same event.
    assert!(matches!(
        world.serve(&mut served, arrival()).await,
        Outcome::Duplicate
    ));
    assert_eq!(world.stub.requests().len(), 2);
}

/// Dr Tola Grey as the seed writes her into tgdrive — her menu is 91.5's
/// and she has no `[tools].allow`, so her tools are the steward's defaults
/// — beside Nixi, on the world's provider.
fn seeded_tola(world: &World) -> AgentDeps {
    use keeper_core::agents::seed::{self, SeedChoices, CATALOGUE};
    let choices = SeedChoices::new(
        "tgdrive",
        "tgorka",
        TGORKA,
        &[TGORKA.to_owned(), MARTA.to_owned()],
        false,
        Some("bot:openai:http://127.0.0.1:9#model"),
        &CATALOGUE
            .iter()
            .map(|a| a.id.to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("choices");
    let toml = seed::files(&choices)
        .into_iter()
        .find(|file| file.path.ends_with("tola-grey/agent.toml"))
        .expect("Tola's home")
        .text;
    deps_of(world, "tola-grey", &toml)
}

/// Her `duty` session, made as the host that won its creation claim makes
/// it, without the room: its zone path.
fn stewards_session(world: &World, tola: &AgentDeps, duty: keeper_agent::stewards::Duty) -> String {
    use keeper_agent::sessions::verbs::{create_carded_session, CreateOutcome};
    use keeper_agent::stewards::{folder_files, session};
    let decl = &tola.home.drive;
    let room = OwnedRoomId::try_from(format!("!{}:example.org", duty.name())).expect("room");
    let now = chrono::Local::now();
    let agent = session(&tola.home.config, decl, duty, &room, now);
    let files =
        folder_files(&tola.home.config, decl, duty, &world.deps.sessions_zone).expect("her files");
    match create_carded_session(&world.deps.sessions_zone, &agent, files, now).expect("made") {
        CreateOutcome::Created { path, .. } => path,
        CreateOutcome::Existed { path, .. } => path,
    }
}

/// 92.5 acceptance 2: Dr Tola Grey's triage card runs on its schedule as
/// one turn whose brief is her `TR` then `DS` prompts. She reads the inbox
/// (`untrusted`), writes a card into her triage session with
/// `session_write` — `assignee`, `requested_by`, a body, and
/// `integrity: untrusted` from the stamp — and hands it to Nixi with
/// `delegate`, naming it as the source: the child is opened at her
/// session's label, `untrusted`, the session Nixi's host makes from it
/// carries that label and its card, and her card says `run: running`. Her
/// triage card ends `review`. Nixi's reply sets her card `review`, and the
/// next day's window, the inbox unchanged, hands nothing on again: the
/// card names the delegation it went to.
#[tokio::test(flavor = "multi_thread")]
async fn triage_writes_cards_and_dispatch_hands_them_on() {
    use keeper_agent::agent::scheduled_arrival;
    use keeper_agent::cards::Scheduled;
    use keeper_agent::stewards::Duty;
    use keeper_core::agents::card::{CardAgent, Field, Run};
    let card = "---\ntags: [task]\ntitle: Answer x\nstatus: todo\nassignee: nixi\nrequested_by: \"@tgorka:example.org\"\n---\n\nAnswer the letter in 00-inbox/x.md; done is a reply sent.\n";
    let hand_x = || {
        delegate_call(
            "d1",
            json!({"agent": "tgdrive/nixi", "brief": "Answer the letter in 00-inbox/x.md.", "card": {"title": "Answer x"}, "source": "answer-x.md"}),
        )
    };
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile": "tgdrive", "path": "00-inbox/x.md"}),
            )]),
            calls(&[(
                "w1",
                "session_write",
                json!({"path": "answer-x.md", "content": card}),
            )]),
            hand_x(),
            prose("Answer x, nixi, @tgorka:example.org. Handed Answer x to Nixi."),
            prose("Nixi answered x."),
            hand_x(),
            prose("Nothing new to hand on."),
        ],
    );
    let tola = seeded_tola(&world);
    let triage = stewards_session(&world, &tola, Duty::Triage);
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut served = world.open_as(&tola, &triage);
    served.delegations = Some(rooms.clone() as Arc<dyn DelegationPort>);

    let run = scheduled_arrival(
        &tola.home.config.matrix_user,
        &Scheduled::Run {
            card: Duty::Triage.card_file(),
            window: "2026-10-05T00:00:00.000Z".to_owned(),
            now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-05T00:00:00.000Z")
                .expect("a time")
                .timestamp_millis(),
            utc_offset_minutes: 0,
        },
    )
    .expect("an arrival");
    let report = report(serve_as(&tola, &mut served, &world.room, run).await);
    assert_eq!(report.ending, TurnEnding::Complete);
    let lines = world.lines(&triage);
    let peers = kinds(&lines, LineKind::Peer);
    let LineBody::Peer(brief) = &peers[0].body else {
        panic!("a peer line")
    };
    assert!(brief.text.contains("Triage what came in"), "{}", brief.text);
    assert!(brief.text.contains("hand its work"), "{}", brief.text);

    // TR: the card she wrote, stamped from the inbox she read.
    let written =
        std::fs::read_to_string(world.dir(&triage).join("answer-x.md")).expect("her card");
    let keys = CardAgent::of_text(&written).expect("keys");
    assert_eq!(keys.assignee, Some(Field::Read("nixi".to_owned())));
    assert_eq!(keys.requested_by, Some(Field::Read(user(TGORKA))));
    assert_eq!(keys.integrity, Some(Field::Read(Integrity::Untrusted)));
    assert!(written.contains("done is a reply sent"), "{written}");

    // DS: one room for Nixi, and the brief carries the untrusted label.
    let made = rooms.made();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].2, vec![user(NIXI)]);
    let child = made[0].3.clone();
    let opened = delegate_lines(&world.lines(&triage));
    assert_eq!(opened[0].state, DelegateState::Opened);
    let mut joined = world.event(NIXI, Arrival::Joined, json!({"membership": "join"}));
    joined.via = Some(child.clone());
    assert!(matches!(
        serve_as(&tola, &mut served, &world.room, joined).await,
        Outcome::BriefSent(_)
    ));
    let sent = rooms.sent().last().expect("the brief").1.clone();
    let handed = read_brief(&sent).expect("a brief");
    assert_eq!(handed.label.integrity, Integrity::Untrusted);
    let path = world.create_child(&world.deps, &child, &handed);
    let text = std::fs::read_to_string(world.dir(&path).join("agent.toml")).expect("agent.toml");
    let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
    assert_eq!(agent.label.integrity, Integrity::Untrusted);
    assert_eq!(agent.agent, "nixi");
    assert_eq!(
        card_field(&world, &path, "assignee").as_deref(),
        Some("nixi")
    );

    let triage_card = std::fs::read_to_string(world.dir(&triage).join(Duty::Triage.card_file()))
        .expect("the triage card");
    let keys = CardAgent::of_text(&triage_card).expect("keys");
    assert_eq!(keys.run, Some(Field::Read(Run::Review)), "{triage_card}");

    let source_run = |world: &World| {
        let text =
            std::fs::read_to_string(world.dir(&triage).join("answer-x.md")).expect("her card");
        CardAgent::of_text(&text).expect("keys").run
    };
    assert_eq!(source_run(&world), Some(Field::Read(Run::Running)));

    // Nixi replies: her card goes to review with it.
    let reply = keeper_agent::delegate::reply_content("x answered.", Vec::new(), &handed.label);
    let event = json!({
        "type": "m.room.message",
        "sender": NIXI,
        "event_id": "$nixi-reply:example.org",
        "content": reply,
    });
    let replied =
        reply_of(&event, &user(NIXI), &child, tokio::time::Instant::now()).expect("a reply");
    let _ = serve_as(&tola, &mut served, &world.room, replied).await;
    assert_eq!(source_run(&world), Some(Field::Read(Run::Review)));

    // The next day's window: the same card is not handed on again.
    let next = scheduled_arrival(
        &tola.home.config.matrix_user,
        &Scheduled::Run {
            card: Duty::Triage.card_file(),
            window: "2026-10-06T00:00:00.000Z".to_owned(),
            now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-06T00:00:00.000Z")
                .expect("a time")
                .timestamp_millis(),
            utc_offset_minutes: 0,
        },
    )
    .expect("an arrival");
    let _ = serve_as(&tola, &mut served, &world.room, next).await;
    assert_eq!(rooms.made().len(), 1, "one child, however many windows");
    let results = tool_results(&world.lines(&triage));
    let again = results.last().expect("the second delegate");
    assert!(
        again.content.contains("was handed on already"),
        "{}",
        again.content
    );
    assert_eq!(source_run(&world), Some(Field::Read(Run::Review)));
}

/// What the holder of the harvest session at `harvest` hands its worker
/// now: one step of a harvester that has seen nothing yet.
fn closed_now(world: &World, harvest: &str) -> Vec<keeper_agent::stewards::Closed> {
    let text = std::fs::read_to_string(world.dir(harvest).join("agent.toml")).expect("agent.toml");
    let agent = keeper_core::agents::session::parse_session_agent_toml(&text).expect("parse");
    keeper_agent::stewards::Harvester::default().step(
        &world.deps.sessions_zone,
        harvest,
        &agent,
        tokio::time::Instant::now(),
    )
}

/// A person's session of tgdrive `title`, archived: its id and path.
fn archived_session(world: &World, title: &str) -> (String, String) {
    use keeper_agent::sessions::verbs::{self, CreateReq};
    let zone = &world.deps.sessions_zone;
    let id = ulid::Ulid::new();
    verbs::create(
        zone,
        CreateReq {
            id,
            title: title.to_owned(),
            pattern_id: None,
            now: chrono::Local::now(),
        },
    )
    .expect("created");
    verbs::archive(zone, &id.to_string(), Vec::new(), false, 2026).expect("closed");
    let path = verbs::find(zone, &id.to_string()).expect("found").path;
    (id.to_string(), path)
}

/// 92.5 acceptance 3, the turn (R61): a session of tgdrive closed — found
/// under `archive/` — is one turn in Dr Tola Grey's harvest session, its
/// brief her `HV` prompt naming the closed session's path, its `peer` line
/// carrying the closed session's id. Routed again, after the index is
/// rebuilt and the session reopened, it is no second turn; routed into any
/// session but her harvest one it is no turn at all.
#[tokio::test(flavor = "multi_thread")]
async fn a_closed_session_wakes_its_stewards_harvest_once() {
    use keeper_agent::agent::{harvest_arrival, NO_HARVEST};
    use keeper_agent::stewards::Duty;
    let world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![prose(
            "It learned the tax office's new address; it belongs in notes/taxes.md.",
        )],
    );
    let tola = seeded_tola(&world);
    let harvest = stewards_session(&world, &tola, Duty::Harvest);
    let triage = stewards_session(&world, &tola, Duty::Triage);
    let zone = world.deps.sessions_zone.clone();
    let (taxes, _) = archived_session(&world, "taxes");

    let closed = closed_now(&world, &harvest);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].id, taxes);
    let arrival = || harvest_arrival(&tola.home.config.matrix_user, &closed[0]).expect("arrival");

    let mut elsewhere = world.open_as(&tola, &triage);
    assert!(matches!(
        serve_as(&tola, &mut elsewhere, &world.room, arrival()).await,
        Outcome::Ignored(note) if note == NO_HARVEST
    ));

    // Her harvest room: the drive's readers, and no agent she does not know.
    world.room.set_members(&[TGORKA, MARTA]);
    let mut served = world.open_as(&tola, &harvest);
    let report = report(serve_as(&tola, &mut served, &world.room, arrival()).await);
    assert_eq!(report.ending, TurnEnding::Complete);
    let lines = world.lines(&harvest);
    let peers = kinds(&lines, LineKind::Peer);
    assert_eq!(peers.len(), 1);
    let LineBody::Peer(peer) = &peers[0].body else {
        panic!("a peer line")
    };
    assert!(
        peer.text.contains("A session of this drive has closed"),
        "{}",
        peer.text
    );
    assert!(
        peer.text
            .contains(&format!("60-sessions/{}", closed[0].path)),
        "{}",
        peer.text
    );
    assert!(peer.text.contains(&taxes), "{}", peer.text);
    assert_eq!(peers[0].matrix_event.as_ref(), Some(&arrival().event_id));

    assert!(matches!(
        serve_as(&tola, &mut served, &world.room, arrival()).await,
        Outcome::Duplicate
    ));
    keeper_core::agents::index::Index::open(&zone)
        .and_then(|mut index| index.rebuild())
        .expect("rebuilt");
    let mut reopened = world.open_as(&tola, &harvest);
    assert!(matches!(
        serve_as(&tola, &mut reopened, &world.room, arrival()).await,
        Outcome::Duplicate
    ));
    assert_eq!(world.stub.requests().len(), 1);
}

/// Write `agent` as the `agent.toml` of the archived session at `path`.
fn as_agents(world: &World, path: &str, agent: &keeper_core::agents::session::SessionAgent) {
    std::fs::write(
        world.dir(path).join("agent.toml"),
        keeper_core::agents::session::compose_session_agent_toml(agent),
    )
    .expect("agent.toml");
}

/// R166: the harvest session carries the closed session's label. A closed
/// session read by tgorka alone — by its `agent.toml`, or narrowed so by a
/// `label` line of its log — and one that may go only to a local model,
/// are refused before anything of them is logged, sent to the room or to
/// the (remote) provider, each refusal audited (R65): the harvest room or
/// the model it was kept from. An untrusted one is harvested, its
/// integrity joined by a `label` line before the `peer` line, before the
/// model reads a word.
#[tokio::test(flavor = "multi_thread")]
async fn a_harvest_carries_the_closed_sessions_label_and_refuses_what_it_cannot_reach() {
    use keeper_agent::agent::{harvest_arrival, HARVEST, HARVEST_REFUSED};
    use keeper_agent::stewards::{session, Duty};
    use keeper_core::agents::label::{LabelBody, LabelCause, LabelCauseKind};
    use keeper_core::bots::audit::{AuditOutcome, AuditVerdict};
    use keeper_core::bots::grant::Effect;
    let world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![prose("It learned nothing the drive should keep.")],
    );
    let tola = seeded_tola(&world);
    let harvest = stewards_session(&world, &tola, Duty::Harvest);
    let decl = &tola.home.drive;
    let room = OwnedRoomId::try_from("!closed:example.org").expect("room");
    let theirs = |id: &str, label: Label| keeper_core::agents::session::SessionAgent {
        id: id.parse().expect("ulid"),
        kind: SessionKind::Delegated,
        label,
        ..session(
            &tola.home.config,
            decl,
            Duty::Triage,
            &room,
            chrono::Local::now(),
        )
    };
    let wide = Label::opening(decl, Integrity::Owner);
    let tgorka_only = Label {
        readers: Readers::Only([user(TGORKA)].into_iter().collect()),
        ..wide.clone()
    };

    let (narrow_id, narrow) = archived_session(&world, "narrow");
    as_agents(&world, &narrow, &theirs(&narrow_id, tgorka_only.clone()));
    let (narrowed_id, narrowed) = archived_session(&world, "narrowed later");
    as_agents(&world, &narrowed, &theirs(&narrowed_id, wide.clone()));
    forced(
        &world.dir(&narrowed),
        "electra",
        1,
        "$c1:example.org",
        acquired(1, "$c1:example.org"),
    );
    forced(
        &world.dir(&narrowed),
        "electra",
        1,
        "$c1:example.org",
        LineBody::Label(LabelBody::new(
            &tgorka_only,
            LabelCause {
                kind: LabelCauseKind::DriveRead,
                reference: "a private note".to_owned(),
            },
        )),
    );
    let (local_id, local) = archived_session(&world, "local");
    let local_only = Label {
        local_only: true,
        ..wide.clone()
    };
    as_agents(&world, &local, &theirs(&local_id, local_only));
    let (outside_id, outside) = archived_session(&world, "outside");
    let untrusted = Label {
        integrity: Integrity::Untrusted,
        ..wide.clone()
    };
    as_agents(&world, &outside, &theirs(&outside_id, untrusted));

    let closed = closed_now(&world, &harvest);
    assert_eq!(closed.len(), 4);
    world.room.set_members(&[TGORKA, MARTA]);
    let mut served = world.open_as(&tola, &harvest);
    for id in [&narrow_id, &narrowed_id, &local_id] {
        let source = closed.iter().find(|c| &c.id == id).expect("found");
        let arrived = harvest_arrival(&tola.home.config.matrix_user, source).expect("arrival");
        assert!(matches!(
            serve_as(&tola, &mut served, &world.room, arrived).await,
            Outcome::Ignored(note) if note == HARVEST_REFUSED
        ));
    }
    assert!(world.lines(&harvest).is_empty());
    assert!(world.stub.requests().is_empty());
    assert_eq!(anchors(&world.room), 0);
    let rows = keeper_core::bots::audit::list_audit(
        &world.deps.data_dir,
        Some(&served.context.agent.id.to_string()),
        None,
    )
    .expect("audit");
    let of = |tool: &str| rows.iter().filter(|row| row.tool == tool).count();
    assert_eq!(
        (of(HARVEST), of("model"), rows.len()),
        (2, 1, 3),
        "{rows:?}"
    );
    for row in &rows {
        assert_eq!(row.verdict, Some(AuditVerdict::Deny), "{row:?}");
        assert_eq!(row.outcome, AuditOutcome::Refused, "{row:?}");
        assert_eq!(row.effect, Some(Effect::Write), "{row:?}");
        let at = if row.tool == HARVEST {
            served.context.agent.room.as_str()
        } else {
            tola.bot.target.as_str()
        };
        assert_eq!(row.subpath, at, "{row:?}");
    }

    let source = closed.iter().find(|c| c.id == outside_id).expect("found");
    let arrived = harvest_arrival(&tola.home.config.matrix_user, source).expect("arrival");
    report(serve_as(&tola, &mut served, &world.room, arrived).await);
    let lines = world.lines(&harvest);
    let label_at = lines
        .iter()
        .position(|line| matches!(&line.body, LineBody::Label(body) if body.label().integrity == Integrity::Untrusted))
        .expect("the untrusted join");
    let peer_at = lines
        .iter()
        .position(|line| line.kind() == LineKind::Peer)
        .expect("the peer line");
    assert!(label_at < peer_at);
    assert_eq!(served.context.label.integrity, Integrity::Untrusted);
    assert_eq!(world.stub.requests().len(), 1);
}

/// The line `host` wrote for the harvest `event` straight into its own
/// chunk under its claim: pushed here, never seen by this host's index.
fn begun_on(dir: &Path, host: &str, event: &OwnedEventId, sender: &OwnedUserId) {
    forced(
        dir,
        host,
        1,
        "$c1:example.org",
        acquired(1, "$c1:example.org"),
    );
    let mut chunks = ChunkWriter::open(
        dir,
        &HostSlug::new(host).expect("slug"),
        rotate_at(1 << 20),
        chrono::Utc::now().date_naive(),
    )
    .expect("chunk");
    let now = chrono::Utc::now();
    chunks
        .append(&LogLine {
            v: LINE_VERSION,
            id: ulid::Ulid::new(),
            parent: None,
            ts: chrono::DateTime::from_timestamp_millis(now.timestamp_millis()).expect("ts"),
            host: HostSlug::new(host).expect("slug"),
            epoch: 1,
            claim: Some("$c1:example.org".to_owned()),
            matrix_event: Some(event.clone()),
            body: LineBody::Peer(PeerBody {
                sender: sender.clone(),
                text: "harvest".to_owned(),
                ask: None,
                artifacts: None,
            }),
        })
        .expect("append");
}

/// A harvest another host began is never a second turn here: one whose
/// `peer` line arrived in electra's chunk — this host's index never saw
/// it — and one whose anchor electra left in the room before it stopped
/// without pushing (the worker's read-back names it).
#[tokio::test(flavor = "multi_thread")]
async fn a_harvest_another_host_began_is_never_run_again_here() {
    use keeper_agent::agent::harvest_arrival;
    use keeper_agent::stewards::Duty;
    let world = world(ProviderKind::OpenAi, &["drive_read"], vec![]);
    let tola = seeded_tola(&world);
    let harvest = stewards_session(&world, &tola, Duty::Harvest);
    archived_session(&world, "pushed");
    archived_session(&world, "unpushed");
    let closed = closed_now(&world, &harvest);
    let me = &tola.home.config.matrix_user;
    let arrival = |n: usize| harvest_arrival(me, &closed[n]).expect("arrival");

    begun_on(&world.dir(&harvest), "electra", &arrival(0).event_id, me);
    world.room.set_members(&[TGORKA, MARTA]);
    let mut served = world.open_as(&tola, &harvest);
    assert!(matches!(
        serve_as(&tola, &mut served, &world.room, arrival(0)).await,
        Outcome::Duplicate
    ));

    let anchored = arrival(1).event_id.to_string();
    served.context.started([anchored.as_str()].into_iter());
    assert!(matches!(
        serve_as(&tola, &mut served, &world.room, arrival(1)).await,
        Outcome::Duplicate
    ));
    assert!(world.stub.requests().is_empty());
}

// ---------------------------------------------------------------------------
// 92.6: labels enforced at every sink
// ---------------------------------------------------------------------------

const CARD: &str = "---\ntags: [task]\ntitle: Inbox\nstatus: todo\n---\n\nSort the inbox.\n";

/// The audit rows of `served`'s session, oldest first.
fn audit_list(world: &World, served: &ServedSession) -> Vec<keeper_core::bots::audit::AuditRow> {
    let mut rows = keeper_core::bots::audit::list_audit(
        &world.deps.data_dir,
        Some(&served.context.agent.id.to_string()),
        None,
    )
    .expect("audit");
    rows.reverse();
    rows
}

/// The audit rows of `served`'s session by tool, the newest of each. A
/// tool any classified row names has exactly one row: a call has one
/// however it ended (R90), never a sink's own row beside it. A send of the
/// host's (no tier) has one per refusal (R65), so may repeat.
fn audit_rows(
    world: &World,
    served: &ServedSession,
) -> BTreeMap<String, keeper_core::bots::audit::AuditRow> {
    let rows = audit_list(world, served);
    let mut by_tool = BTreeMap::new();
    for row in rows {
        let classified = row.tier.is_some();
        if let Some(earlier) = by_tool.insert(row.tool.clone(), row) {
            assert!(
                !classified && earlier.tier.is_none(),
                "two audit rows for {}: {earlier:?}",
                earlier.tool
            );
        }
    }
    by_tool
}

/// Nixi as tgorka's proxy, Dr Tola Grey and Dr Lucyna Novak.
fn known_with_proxy() -> Known {
    let mut known = known(&[TGORKA, MARTA]);
    let nixi = &mut known.agents[0];
    nixi.kind = keeper_core::agents::home::AgentKind::Proxy;
    nixi.human = Some(user(TGORKA));
    known
}

/// Nixi's DM with tgorka: her `main` session, under its derived id.
fn nixis_dm(world: &World) {
    let tg_decl = world.deps.drives["tgdrive"].clone();
    let mut agent = session_of(
        &world.tgdrive,
        DM,
        &tg_decl,
        "nixi",
        SessionKind::Main,
        "!dm:example.org",
    );
    agent.id = keeper_core::agents::seed::main_session_id("tgdrive", "nixi");
    write(
        &world.tgdrive,
        &format!("60-sessions/{DM}/agent.toml"),
        &compose_session_agent_toml(&agent),
    );
    write(
        &world.tgdrive,
        &format!("60-sessions/{DM}/README.md"),
        &format!("---\nid: {}\n---\n\n# Nixi\n", agent.id),
    );
}

/// tgorka's proxy DM as this host runs it: what was told through it, by
/// event type, and how many sends fail before one goes.
#[derive(Default)]
struct Doors {
    told: Mutex<Vec<(String, Value)>>,
    failing: AtomicUsize,
    forwards: keeper_agent::deciding::Forwards,
}

impl keeper_agent::sinks::ProxyDoors for Doors {
    fn dm(&self, person: &UserId) -> Option<OwnedRoomId> {
        (person.as_str() == TGORKA).then(|| OwnedRoomId::try_from("!dm:example.org").expect("dm"))
    }

    fn members<'a>(&'a self, _: &'a UserId) -> MembersFuture<'a> {
        Box::pin(async { Ok(BTreeSet::from([user(TGORKA)])) })
    }

    fn tell<'a>(&'a self, _: &'a UserId, event_type: &'a str, content: Value) -> SendFuture<'a> {
        Box::pin(async move {
            if self.failing.load(Ordering::SeqCst) > 0 {
                self.failing.fetch_sub(1, Ordering::SeqCst);
                return Err(keeper_core::agents::matrix::AgentMatrixError::Network(
                    "unreachable".to_owned(),
                ));
            }
            self.told
                .lock()
                .expect("lock")
                .push((event_type.to_owned(), content));
            Ok(OwnedEventId::try_from("$told:example.org").expect("id"))
        })
    }

    fn forwards(&self) -> &keeper_agent::deciding::Forwards {
        &self.forwards
    }
}

impl Doors {
    fn bodies(&self) -> Vec<String> {
        self.told
            .lock()
            .expect("lock")
            .iter()
            .filter(|(kind, _)| kind == "m.room.message")
            .map(|(_, content)| content["body"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn of(&self, event_type: &str) -> Vec<Value> {
        self.told
            .lock()
            .expect("lock")
            .iter()
            .filter(|(kind, _)| kind == event_type)
            .map(|(_, content)| content.clone())
            .collect()
    }
}

fn result_of<'l>(results: &'l [ToolResultBody], call: &str) -> &'l ToolResultBody {
    results
        .iter()
        .find(|result| result.call_id == call)
        .unwrap_or_else(|| panic!("a result for {call}"))
}

/// One producer of a sink, driven past the label in a world of its own: it
/// asserts that nothing left the session and that the refusal was recorded.
type Scenario = Pin<Box<dyn Future<Output = ()>>>;
type Producer = fn() -> Scenario;

/// Who produces into one kind of sink.
enum Producers {
    Driven(Vec<(&'static str, Producer)>),
    /// Nothing produces into it yet; the epic that adds a producer adds it
    /// here.
    NotImplemented(&'static str),
}

fn by(what: &'static str, producer: Producer) -> (&'static str, Producer) {
    (what, producer)
}

/// Every producer of each kind of sink (92.6 acceptance 1). A new `Sink`
/// variant does not compile until it has an arm here: its producers, or
/// the decision that it has none yet.
fn producers(sink: &keeper_core::agents::label::Sink) -> Producers {
    use keeper_core::agents::label::Sink;
    match sink {
        Sink::Room { .. } => Producers::Driven(vec![
            by(
                "a turn's answer, status and scope echo after a read",
                a_read_narrows_every_send_of_its_turn,
            ),
            by(
                "an answer after an outsider was invited",
                an_invite_since_the_last_turn_blocks_the_next_answer,
            ),
            by(
                "a harvest's anchor naming its closed session",
                a_harvest_into_a_wider_room_names_no_closed_session,
            ),
            by(
                "a surface request",
                a_surface_request_into_a_wider_room_is_refused,
            ),
            by(
                "a new conversation's notice",
                a_new_conversations_notice_leaves_out_its_title,
            ),
            by(
                "a delegated session's budget reply",
                a_budget_reply_into_a_wider_room_is_refused,
            ),
        ]),
        Sink::Delegation { .. } => Producers::Driven(vec![
            by("an opening brief", a_read_narrows_every_send_of_its_turn),
            by(
                "a next round naming another agent",
                a_next_round_naming_another_agent_is_refused,
            ),
        ]),
        Sink::DriveWrite { .. } => Producers::Driven(vec![
            by(
                "drive_write, drive_edit, session_write and card_update",
                a_read_narrows_every_send_of_its_turn,
            ),
            by(
                "a long answer's artifact",
                a_long_answer_wider_than_its_drive_writes_no_artifact,
            ),
        ]),
        Sink::MemoryWrite { .. } => {
            Producers::NotImplemented("no agent tool writes memory before epic 95")
        }
        Sink::Model { .. } => Producers::Driven(vec![
            by(
                "a round after a local_only read",
                a_local_only_read_stops_a_remote_round,
            ),
            by(
                "a local_only context file",
                a_local_only_context_file_never_reaches_a_remote_model,
            ),
        ]),
        Sink::External { .. } => Producers::NotImplemented("no MCP server or KVM before epic 96"),
    }
}

/// 92.6 acceptance 1: every producer of every kind of sink, driven past
/// the label, sends and writes nothing and records its refusal.
#[tokio::test(flavor = "multi_thread")]
async fn every_sink_refuses_a_wider_audience() {
    use keeper_core::agents::label::Sink;
    let mut driven = BTreeSet::new();
    for sink in [
        Sink::Room {
            humans: BTreeSet::new(),
            agent_audiences: Vec::new(),
        },
        Sink::Delegation {
            target_audience: Readers::Anyone,
            room_members: BTreeSet::new(),
        },
        Sink::DriveWrite {
            drive_readers: Readers::Anyone,
        },
        Sink::MemoryWrite {
            home_readers: Readers::Anyone,
        },
        Sink::Model { local: false },
        Sink::External {
            readers: Readers::Anyone,
        },
    ] {
        let producers = match producers(&sink) {
            Producers::Driven(producers) => producers,
            Producers::NotImplemented(why) => {
                eprintln!("{sink:?}: {why}");
                continue;
            }
        };
        assert!(!producers.is_empty(), "{sink:?}");
        for (what, producer) in producers {
            if driven.insert(producer as usize) {
                eprintln!("producer: {what}");
                producer().await;
            }
        }
    }
}

/// A session opened for tgorka and Marta reads tgorka's diary, so it is
/// {tgorka} in a room with Marta. Every sink its turn tries is refused —
/// a drive write and an edit into tgdrive (read by both), a session file,
/// a card, a delegation to Dr Lucyna Novak, the answer — nothing reaches
/// the disk or the rooms, each leaves its audit row (R65) and its refused
/// line; every status says only the fixed sentence (UX-DR135), no scope is
/// echoed, each suppression is audited, and the detail goes once to
/// tgorka's DM with Nixi (92.6 acceptance 9).
fn a_read_narrows_every_send_of_its_turn() -> Scenario {
    Box::pin(async {
        use keeper_agent::sinks::NARROWED_STATUS;
        use keeper_core::bots::audit::{AuditOutcome, AuditVerdict};
        use keeper_core::bots::grant::Effect;
        let script = vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"private","path":"diary.md"}),
            )]),
            calls(&[
                (
                    "w1",
                    "drive_write",
                    json!({"profile":"tgdrive","path":"notes/out.md","content":"dear diary"}),
                ),
                (
                    "e1",
                    "drive_edit",
                    json!({"profile":"tgdrive","path":"notes/hello.md","old_text":"first line","new_text":"dear diary"}),
                ),
                (
                    "s1",
                    "session_write",
                    json!({"path":"notes.md","content":"dear diary"}),
                ),
                (
                    "c1",
                    "card_update",
                    json!({"card":"card.md","fields":{"status":"done"}}),
                ),
                (
                    "d1",
                    "delegate",
                    json!({"agent":"neuradrive/lucyna","brief":"dear diary"}),
                ),
            ]),
            prose("the diary says hello."),
        ];
        let mut world = world(
            ProviderKind::Ollama,
            &[
                "drive_read",
                "drive_write",
                "drive_edit",
                "session_write",
                "card_update",
                "delegate",
            ],
            script,
        );
        write(&world.dir(SESSION), "card.md", CARD);
        nixis_dm(&world);
        let delegations = Delegations::over(known_with_proxy());
        let doors = Arc::new(Doors::default());
        let mut served = world.delegating(&delegations);
        served.doors = Some(doors.clone());
        let turn = report(world.ask(&mut served, "read my diary").await);
        assert_eq!(turn.ending, TurnEnding::Complete);

        let lines = world.lines(SESSION);
        let results = tool_results(&lines);
        for call in ["w1", "e1", "s1", "c1", "d1"] {
            let result = result_of(&results, call);
            assert_eq!(result.outcome, ToolOutcomeWord::Refused, "{call}");
            assert!(
                result.content.contains(MARTA)
                    && result
                        .content
                        .contains(keeper_core::agents::label::NEEDS_APPROVAL),
                "{call}: {}",
                result.content
            );
        }
        // Nothing reached the disk or a room.
        assert!(!world.tgdrive.join("notes/out.md").exists());
        assert_eq!(
            std::fs::read_to_string(world.tgdrive.join("notes/hello.md")).expect("hello"),
            "first line\nsecond line\n"
        );
        assert!(!world.dir(SESSION).join("notes.md").exists());
        assert_eq!(
            std::fs::read_to_string(world.dir(SESSION).join("card.md")).expect("card"),
            CARD
        );
        assert!(delegations.made().is_empty());
        assert!(delegations.sent().is_empty());
        assert_eq!(final_edit(&world.room), NARROWER_THAN_ROOM);
        assert!(world
            .room
            .sent()
            .iter()
            .all(|(_, content)| !content.to_string().contains("diary says hello")));

        // Once narrowed, every status says the fixed sentence, no detail.
        let statuses = world.sent_of(STATUS);
        let first = statuses
            .iter()
            .position(|status| status["title"] == NARROWED_STATUS)
            .expect("a narrowed status");
        for status in &statuses[first..] {
            assert_eq!(status["title"], NARROWED_STATUS, "{status}");
            assert!(status.get("detail").is_none_or(Value::is_null), "{status}");
        }
        // A scope the person sets now is taken, and not echoed.
        let narrower = world.scope(TGORKA, &["tgdrive"], None);
        assert!(matches!(
            world.serve(&mut served, narrower).await,
            Outcome::Scoped(_)
        ));
        assert!(world.sent_of(SCOPE).is_empty());

        // Each sink's audit row (R65), the suppressed sends' too.
        let rows = audit_rows(&world, &served);
        for tool in [
            "drive_write",
            "drive_edit",
            "session_write",
            "card_update",
            "delegate",
            "answer",
            "status",
            "scope",
        ] {
            let row = rows
                .get(tool)
                .unwrap_or_else(|| panic!("an audit row for {tool}"));
            assert_eq!(row.verdict, Some(AuditVerdict::Deny), "{tool}");
            assert_eq!(row.outcome, AuditOutcome::Refused, "{tool}");
            assert_eq!(row.effect, Some(Effect::Write), "{tool}");
        }
        // An agent's call: its one row carries its tier (R90).
        for tool in [
            "drive_write",
            "drive_edit",
            "session_write",
            "card_update",
            "delegate",
        ] {
            assert!(rows[tool].tier.is_some(), "{tool}");
        }
        assert_eq!(rows["delegate"].profile_id, "neuradrive");
        assert_eq!(rows["delegate"].subpath, LUCYNA);
        assert_eq!(rows["drive_edit"].profile_id, "tgdrive");
        assert_eq!(rows["drive_edit"].subpath, "notes/hello.md");
        for tool in ["answer", "status", "scope"] {
            assert_eq!(rows[tool].subpath, "!room:example.org", "{tool}");
        }

        // The detail went to tgorka's DM with Nixi, once, never the room.
        let told = doors.bodies();
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(kinds(&world.lines(SESSION), LineKind::Told).len(), 1);
        assert!(
            told[0].contains(&format!("60-sessions/{SESSION}")),
            "{}",
            told[0]
        );
        assert!(world.room.sent().iter().all(|(_, content)| !content
            .to_string()
            .contains(&format!("60-sessions/{SESSION})"))));

        // The next turn's status is narrowed from its first edit.
        let before = world.sent_of(STATUS).len();
        let _ = world.ask(&mut served, "and now?").await;
        for status in &world.sent_of(STATUS)[before..] {
            assert_eq!(status["title"], NARROWED_STATUS, "{status}");
        }
        // Outside a turn too: a refused scope's status says no drive.
        let elsewhere = world.scope(TGORKA, &["marta-drive"], None);
        assert!(matches!(
            world.serve(&mut served, elsewhere).await,
            Outcome::ScopeRefused(_)
        ));
        let refused = world.sent_of(STATUS).last().expect("a status").clone();
        assert_eq!(refused["title"], NARROWED_STATUS, "{refused}");
        assert!(
            refused.get("detail").is_none_or(Value::is_null),
            "{refused}"
        );
        assert!(world.sent_of(SCOPE).is_empty());
    })
}

/// R168: the room is read at the send. A {tgorka, Marta} session answers
/// in its room; then an outsider is invited, and the next turn's answer
/// is the fixed sentence, audited — the room as it opened is not the
/// room it is now.
fn an_invite_since_the_last_turn_blocks_the_next_answer() -> Scenario {
    Box::pin(async {
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read"],
            vec![prose("first."), prose("the plan is to sell in March.")],
        );
        let mut served = world.open(SESSION);
        report(world.ask(&mut served, "hi").await);
        assert_eq!(final_edit(&world.room), "first.");
        world
            .room
            .set_members(&[TGORKA, MARTA, NIXI, "@eve:example.org"]);
        report(world.ask(&mut served, "and the plan?").await);
        assert_eq!(final_edit(&world.room), NARROWER_THAN_ROOM);
        assert!(world
            .room
            .sent()
            .iter()
            .all(|(_, content)| !content.to_string().contains("sell in March")));
        let rows = audit_rows(&world, &served);
        assert_eq!(
            rows["answer"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
        assert_eq!(rows["answer"].subpath, "!room:example.org");
    })
}

/// R166 under R168: a harvest's anchor names its closed session, so the
/// harvest room as it is now must be one the joined label reaches. With
/// an outsider invited, with a known agent whose audience is wider, and
/// with members no one can read, the closed session is refused before
/// anything is logged, asked of the model or sent — no anchor, no id,
/// hex or plain — and each refusal is audited; an unreadable room is
/// handed again. A known agent within the label lets it run, once. A
/// room that widens while the anchor waits out a 429 hears only that a
/// harvest ran, and its answer is withheld.
fn a_harvest_into_a_wider_room_names_no_closed_session() -> Scenario {
    Box::pin(async {
        use keeper_agent::agent::{harvest_arrival, ServeError, HARVEST, HARVEST_REFUSED};
        use keeper_agent::stewards::Duty;
        const EVE: &str = "@eve:example.org";
        let world = world(
            ProviderKind::OpenAi,
            &["drive_read"],
            vec![prose("Nothing to keep."), prose("The pension plan.")],
        );
        let tola = seeded_tola(&world);
        let harvest = stewards_session(&world, &tola, Duty::Harvest);
        let (taxes, _) = archived_session(&world, "taxes");
        let (pension, _) = archived_session(&world, "pension");
        let closed = closed_now(&world, &harvest);
        let arrival = |id: &str| {
            let source = closed.iter().find(|c| c.id == id).expect("found");
            harvest_arrival(&tola.home.config.matrix_user, source).expect("arrival")
        };
        let named = |id: &str, sent: &[(String, Value)]| {
            let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
            sent.iter().any(|(_, content)| {
                let text = content.to_string();
                text.contains(&hex) || text.contains(id)
            })
        };
        let rooms = Delegations::over(known(&[TGORKA, MARTA, EVE]));
        let mut served = world.open_as(&tola, &harvest);
        served.delegations = Some(rooms.clone() as Arc<dyn DelegationPort>);

        for members in [[TGORKA, MARTA, EVE], [TGORKA, MARTA, NIXI]] {
            world.room.set_members(&members);
            assert!(matches!(
                serve_as(&tola, &mut served, &world.room, arrival(&taxes)).await,
                Outcome::Ignored(note) if note == HARVEST_REFUSED
            ));
        }
        world.room.unread.store(true, Ordering::SeqCst);
        let (_stop, signal) = chat::cancellation();
        assert!(matches!(
            served
                .serve(&tola, world.room.clone(), arrival(&taxes), signal)
                .await,
            Err(ServeError::MembersUnread)
        ));
        world.room.unread.store(false, Ordering::SeqCst);
        assert!(world.lines(&harvest).is_empty());
        assert!(world.stub.requests().is_empty());
        assert!(world.room.sent().is_empty(), "{:?}", world.room.sent());
        let rows = keeper_core::bots::audit::list_audit(
            &world.deps.data_dir,
            Some(&served.context.agent.id.to_string()),
            None,
        )
        .expect("audit");
        assert_eq!(rows.len(), 3, "{rows:?}");
        for row in &rows {
            assert_eq!(row.tool, HARVEST);
            assert_eq!(row.subpath, served.context.agent.room.as_str());
            assert_eq!(
                row.verdict,
                Some(keeper_core::bots::audit::AuditVerdict::Deny)
            );
        }

        // Nixi's audience within the label: she counts through it.
        *rooms.known.lock().expect("lock") = Arc::new(known(&[TGORKA, MARTA]));
        report(serve_as(&tola, &mut served, &world.room, arrival(&taxes)).await);
        assert_eq!(world.stub.requests().len(), 1);
        assert_eq!(anchors(&world.room), 1);
        assert!(matches!(
            serve_as(&tola, &mut served, &world.room, arrival(&taxes)).await,
            Outcome::Duplicate
        ));

        // Eve is invited while the anchor waits out a 429.
        let before = world.room.sent().len();
        *world.room.limited.lock().expect("lock") = Some(vec![TGORKA, MARTA, NIXI, EVE]);
        report(serve_as(&tola, &mut served, &world.room, arrival(&pension)).await);
        let sent = world.room.sent()[before..].to_vec();
        assert!(!named(&pension, &sent), "{sent:?}");
        assert_eq!(anchors(&world.room), 2);
        assert_eq!(final_edit(&world.room), NARROWER_THAN_ROOM);
        assert!(sent
            .iter()
            .all(|(_, content)| !content.to_string().contains("pension plan")));
    })
}

/// R169 for a room that widened: a session whose label still reaches
/// everyone it was opened for has an outsider invited; its answer is
/// withheld and its status fixed, and its person is told once through
/// their proxy — the first send failing, the worker's restart sending it
/// again — with one `told` line.
#[tokio::test(flavor = "multi_thread")]
async fn a_room_grown_wider_than_the_label_tells_its_person() {
    use keeper_agent::sinks::NARROWED_STATUS;
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![prose("first."), prose("the plan is to sell in March.")],
    );
    nixis_dm(&world);
    let delegations = Delegations::over(known_with_proxy());
    let doors = Arc::new(Doors::default());
    doors.failing.store(1, Ordering::SeqCst);
    let mut served = world.delegating(&delegations);
    served.doors = Some(doors.clone());
    let opening = served.context.label.clone();
    report(world.ask(&mut served, "hi").await);
    world
        .room
        .set_members(&[TGORKA, MARTA, NIXI, "@eve:example.org"]);
    report(world.ask(&mut served, "and the plan?").await);
    assert_eq!(served.context.label, opening);
    assert_eq!(final_edit(&world.room), NARROWER_THAN_ROOM);
    assert!(world
        .room
        .sent()
        .iter()
        .all(|(_, content)| !content.to_string().contains("sell in March")));
    let last = world.sent_of(STATUS).last().cloned().expect("a status");
    assert_eq!(last["title"], NARROWED_STATUS, "{last}");
    assert!(doors.bodies().is_empty());
    assert!(kinds(&world.lines(SESSION), LineKind::Told).is_empty());
    drop(served);

    let mut served = world.delegating(&delegations);
    served.doors = Some(doors.clone());
    let (keep, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (_stop, signal) = chat::cancellation();
    let busy = keeper_agent::agent::Activity::default();
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        served.serve_arrivals(
            &world.deps,
            world.room.clone(),
            Vec::new(),
            &mut arrivals,
            signal,
            &busy,
        ),
    )
    .await;
    drop(keep);
    let told = doors.bodies();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(&format!("60-sessions/{SESSION}")));
    assert_eq!(kinds(&world.lines(SESSION), LineKind::Told).len(), 1);
}

/// R168: a surface request goes into the session's room, not to the
/// device — once Marta is invited to tgorka's room, a request carrying
/// his note's line is refused, nothing is sent, and it is audited.
fn a_surface_request_into_a_wider_room_is_refused() -> Scenario {
    Box::pin(async {
        let script = vec![
            calls(&[(
                "c1",
                "surface_highlight",
                json!({"drive": "tgdrive", "path": "notes/hello.md", "range": {"from": 2, "to": 2}}),
            )]),
            prose("Highlighted."),
        ];
        let mut world = world_read_by(
            &[TGORKA],
            ProviderKind::OpenAi,
            &["drive_read", "surface_highlight"],
            script,
        );
        world.room.set_members(&[TGORKA, NIXI, MARTA]);
        let mut served = world.open(SESSION);
        let surface = Arc::new(Surface {
            room: room_id(),
            requests: Mutex::new(Vec::new()),
        });
        served.surface = Some(surface.clone());
        report(world.ask(&mut served, "show me the second line").await);

        assert!(surface.requests.lock().expect("lock").is_empty());
        let c1 = result_of(&tool_results(&world.lines(SESSION)), "c1").clone();
        assert_eq!(c1.outcome, ToolOutcomeWord::Refused);
        assert!(c1.content.contains(MARTA), "{}", c1.content);
        assert!(kinds(&world.lines(SESSION), LineKind::Surface).is_empty());
        let rows = audit_rows(&world, &served);
        assert_eq!(
            rows["surface_highlight"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
        assert_eq!(
            rows["surface_highlight"].tier,
            Some(1),
            "one classified row (R90)"
        );
    })
}

/// R64, R65: Nixi's DM read tgorka's diary, so it is {tgorka} with Marta
/// in the room; a new conversation he asks for is made, and the DM hears
/// only that one was opened — not its title — and the notice is audited.
fn a_new_conversations_notice_leaves_out_its_title() -> Scenario {
    Box::pin(async {
        let mut world = world(
            ProviderKind::Ollama,
            &["drive_read"],
            vec![
                calls(&[(
                    "r1",
                    "drive_read",
                    json!({"profile":"private","path":"diary.md"}),
                )]),
                prose("read."),
            ],
        );
        nixis_dm(&world);
        let rooms = Arc::new(Rooms::default());
        let mut dm = world.open(DM);
        dm.conversations = Some(rooms.clone());
        report(world.ask(&mut dm, "read my diary").await);
        let before = world.sent_of("m.room.message").len();
        let ask = world.event(
            TGORKA,
            Arrival::ConversationRequest { owner_signed: true },
            json!({"v": 1, "title": "Trip to Lisbon"}),
        );
        assert!(matches!(
            world.serve(&mut dm, ask).await,
            Outcome::Conversation { made: true, .. }
        ));
        let notices = world.sent_of("m.room.message");
        assert_eq!(notices.len(), before + 1);
        let notice = notices.last().expect("a notice").to_string();
        assert!(!notice.contains("Lisbon"), "{notice}");
        let rows = audit_rows(&world, &dm);
        assert_eq!(
            rows["notice"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
        assert_eq!(rows["notice"].subpath, "!dm:example.org");
    })
}

/// R94 for the host's own reply: Tola's delegated session spends its
/// budget in a room someone outside its label has joined; the reply that
/// says so — it carries the session's label — is refused and audited like
/// a reply of the model's, and nothing goes into the room.
fn a_budget_reply_into_a_wider_room_is_refused() -> Scenario {
    Box::pin(async {
        let mut round = calls(&[(
            "r1",
            "drive_read",
            json!({"profile": "tgdrive", "path": "notes/hello.md"}),
        )]);
        round.push(json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 200, "total_tokens": 1200}}));
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read"],
            vec![round, prose("never asked")],
        );
        let tola = tolas(&world, &["drive_read"]);
        let content = DelegateContent {
            v: 1,
            id: ulid::Ulid::new().to_string(),
            from: keeper_core::agents::delegation::DelegateFrom {
                agent: user(NIXI),
                drive: "tgdrive".to_owned(),
                session: SESSION.to_owned(),
                room: room_id(),
            },
            to: user(TOLA),
            brief: "Read hello.".to_owned(),
            drives: vec!["tgdrive".to_owned()],
            label: Label::opening(&world.deps.drives["tgdrive"], Integrity::Agent),
            hop: 1,
            limits: keeper_core::agents::delegation::DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 1000,
            },
            card: None,
            dispatch_chain: Vec::new(),
        };
        let child = OwnedRoomId::try_from("!child:example.org").expect("room");
        let path = world.create_child(&tola, &child, &content);
        let rooms = Delegations::over(known(&[TGORKA, MARTA]));
        rooms
            .added
            .lock()
            .expect("lock")
            .push((child.clone(), user("@eve:example.org")));
        // The brief was taken in the room as it was then: Nixi and Tola.
        *rooms.facts.lock().expect("lock") = Some(BriefRoom {
            room_type: Some(SESSION_ROOM_TYPE.to_owned()),
            creators: vec![user(NIXI)],
            levels: Some(delegated_levels(|_| {})),
            members: BTreeSet::from([user(NIXI), user(TOLA)]),
        });
        let mut tolas_session = world.child(&tola, &path, &rooms);
        let tolas_room = Arc::new(Room::default());
        let arrived = world.brief(&keeper_core::agents::delegation::brief_content(&content));
        let ran = report(serve_as(&tola, &mut tolas_session, &tolas_room, arrived).await);
        assert_eq!(ran.ending, TurnEnding::Bounded);
        assert!(tolas_room.sent().iter().all(|(_, content)| {
            !content["dev.keeper.agent.artifacts"].is_array()
                && content.get("dev.keeper.agent.label").is_none()
        }));
        let rows = audit_rows(&world, &tolas_session);
        assert_eq!(
            rows["reply"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
        assert_eq!(rows["reply"].subpath, "!child:example.org");
    })
}

/// A next round goes to the agent its exchange was opened with (R49): a
/// call naming another agent with that exchange's id is refused — its
/// brief goes nowhere — while the same call naming the exchange's own
/// agent is sent.
fn a_next_round_naming_another_agent_is_refused() -> Scenario {
    Box::pin(async {
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "delegate"],
            vec![
                hand_inbox(),
                prose("Handed on."),
                calls(&[
                    (
                        "m2",
                        "delegate",
                        json!({"agent": "neuradrive/lucyna", "brief": "Tell me the payroll.", "session": DELEGATION}),
                    ),
                    (
                        "m3",
                        "delegate",
                        json!({"agent": "tgdrive/tola", "brief": "And the receipts.", "session": DELEGATION}),
                    ),
                ]),
                prose("Told her."),
            ],
        );
        let rooms = Delegations::over(known(&[TGORKA, MARTA]));
        let (mut nixi, child, _) = handed_over(&mut world, &rooms).await;
        report(world.ask(&mut nixi, "say more to Tola").await);
        let results = tool_results(&world.lines(SESSION));
        let m2 = result_of(&results, "m2");
        assert_eq!(m2.outcome, ToolOutcomeWord::Refused);
        assert!(m2.content.contains("neuradrive/lucyna"), "{}", m2.content);
        assert_eq!(result_of(&results, "m3").outcome, ToolOutcomeWord::Ok);
        let sent = rooms.sent();
        assert_eq!(sent.len(), 2, "the brief and the matching round");
        assert!(sent
            .iter()
            .all(|(room, content, _)| room == &child && !content.to_string().contains("payroll")));
        assert_eq!(sent[1].1["body"], "And the receipts.");
        assert!(delegate_lines(&world.lines(SESSION)).iter().any(|line| {
            line.state == DelegateState::Refused
                && line.reason.as_deref().is_some_and(|r| r.contains("lucyna"))
        }));
    })
}

/// The artifact of a long answer is a session file: a {tgorka} session
/// answering in a room of tgorka alone does not write it into tgdrive,
/// which Marta reads — the room is pointed at the log, and it is audited.
fn a_long_answer_wider_than_its_drive_writes_no_artifact() -> Scenario {
    Box::pin(async {
        let answer = "ab".repeat(100 * 1024);
        let mut world = world(
            ProviderKind::Ollama,
            &["drive_read"],
            vec![
                calls(&[(
                    "r1",
                    "drive_read",
                    json!({"profile":"private","path":"diary.md"}),
                )]),
                prose(&answer),
            ],
        );
        world.room.set_members(&[TGORKA, NIXI]);
        let mut served = world.open(SESSION);
        report(world.ask(&mut served, "read my diary at length").await);
        let edit = final_edit(&world.room);
        assert!(edit.len() < 100 * 1024 && !edit.contains("artifacts/"));
        assert!(!world.dir(SESSION).join("artifacts").exists());
        let rows = audit_rows(&world, &served);
        let row = &rows["session_write"];
        assert_eq!(
            row.verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
        assert!(
            row.subpath
                .starts_with(&format!("60-sessions/{SESSION}/artifacts/answer-")),
            "{}",
            row.subpath
        );
    })
}

/// A `local_only` label stops a model round on a provider that is not
/// local: the diary read in the first round never reaches a second.
fn a_local_only_read_stops_a_remote_round() -> Scenario {
    Box::pin(async {
        let mut remote = world_of_remote();
        let mut served = remote.open(SESSION);
        let turn = report(remote.ask(&mut served, "read my diary").await);
        assert_eq!(turn.ending, TurnEnding::LocalOnly);
        assert_eq!(remote.stub.requests().len(), 1);
        let rows = audit_rows(&remote, &served);
        assert_eq!(
            rows["model"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
    })
}

/// R168: a context file the prompt carries is a read of it. tgorka's
/// `local_only` drive has an `AGENTS.md`; a session with that drive in
/// scope on a remote provider never sends it — no request carries it, the
/// session is `local_only` from the turn's start, and the round is
/// refused and audited.
fn a_local_only_context_file_never_reaches_a_remote_model() -> Scenario {
    Box::pin(async {
        const SENTINEL: &str = "kalypso-4471-never-remote";
        let mut world = world(ProviderKind::OpenAi, &["drive_read"], vec![prose("never.")]);
        write(
            &world._root.path().join("private"),
            "AGENTS.md",
            &format!("Answer in Polish. {SENTINEL}\n"),
        );
        let mut served = world.open(SESSION);
        let turn = report(world.ask(&mut served, "hello").await);
        assert_eq!(turn.ending, TurnEnding::LocalOnly);
        assert!(served.context.label.local_only);
        assert!(world
            .stub
            .requests()
            .iter()
            .all(|request| !request.to_string().contains(SENTINEL)));
        let rows = audit_rows(&world, &served);
        assert_eq!(
            rows["model"].verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Deny)
        );
    })
}

/// R169: the detail of a narrowed session whose first send into its
/// person's DM failed is not marked told; the worker's start sends it
/// again, once, and only then is the `told` line written.
#[tokio::test(flavor = "multi_thread")]
async fn a_narrowed_detail_whose_first_send_failed_is_sent_again() {
    let mut world = world(
        ProviderKind::Ollama,
        &["drive_read"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"private","path":"diary.md"}),
            )]),
            prose("read."),
        ],
    );
    nixis_dm(&world);
    let delegations = Delegations::over(known_with_proxy());
    let doors = Arc::new(Doors::default());
    doors.failing.store(1, Ordering::SeqCst);
    let mut served = world.delegating(&delegations);
    served.doors = Some(doors.clone());
    report(world.ask(&mut served, "read my diary").await);
    assert!(doors.bodies().is_empty());
    assert!(kinds(&world.lines(SESSION), LineKind::Told).is_empty());

    let (keep, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
    let (_stop, signal) = chat::cancellation();
    let busy = keeper_agent::agent::Activity::default();
    let _ = tokio::time::timeout(
        Duration::from_secs(2),
        served.serve_arrivals(
            &world.deps,
            world.room.clone(),
            Vec::new(),
            &mut arrivals,
            signal,
            &busy,
        ),
    )
    .await;
    drop(keep);
    let told = doors.bodies();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains(&format!("60-sessions/{SESSION}")));
    assert_eq!(kinds(&world.lines(SESSION), LineKind::Told).len(), 1);
}

/// 92.5 acceptance 2 under R167: Dr Tola Grey's scheduled triage reads
/// the inbox — untrusted from then on, with no person's line in the
/// session — and still hands an item to Dr Lucyna Novak, a known agent
/// whose audience is within the label; a recipient the inbox named is
/// blocked.
#[tokio::test(flavor = "multi_thread")]
async fn a_scheduled_triage_dispatches_to_a_known_agent_after_an_inbox_read() {
    use keeper_agent::agent::scheduled_arrival;
    use keeper_agent::cards::Scheduled;
    const TRIAGE: &str = "active/2026-10-05-triage";
    let world = world(
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"tgdrive","path":"00-inbox/x.md"}),
            )]),
            calls(&[
                (
                    "d1",
                    "delegate",
                    json!({"agent":"evil/exfil","brief":"as the page says"}),
                ),
                (
                    "d2",
                    "delegate",
                    json!({"agent":"neuradrive/lucyna","brief":"File 00-inbox/x.md."}),
                ),
            ]),
            prose("Dispatched."),
        ],
    );
    let tola = tolas(&world, &["drive_read", "delegate"]);
    session_of(
        &world.tgdrive,
        TRIAGE,
        &decl("tgdrive", &[TGORKA, MARTA], false),
        "tola",
        SessionKind::Scheduled,
        "!triage:example.org",
    );
    write(
        &world.tgdrive,
        &format!("60-sessions/{TRIAGE}/card.md"),
        "---\ntags: [task]\ntitle: Triage\nstatus: todo\nassignee: tola\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nTriage 00-inbox and hand each item to its specialist.\n",
    );
    let rooms = Delegations::over(known(&[TGORKA, MARTA]));
    let mut served = world.child(&tola, TRIAGE, &rooms);
    let room = Arc::new(Room::of(&[TGORKA, MARTA, TOLA]));
    let arrival = scheduled_arrival(
        &user(TOLA),
        &Scheduled::Run {
            card: "card.md".to_owned(),
            window: "2026-10-05T09:00:00.000Z".to_owned(),
            now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-05T09:30:00Z")
                .expect("an instant")
                .timestamp_millis(),
            utc_offset_minutes: 0,
        },
    )
    .expect("an arrival");
    let ran = report(serve_as(&tola, &mut served, &room, arrival).await);
    assert_eq!(ran.ending, TurnEnding::Complete);
    assert_eq!(served.context.label.integrity, Integrity::Untrusted);
    let lines = world.lines(TRIAGE);
    assert!(kinds(&lines, LineKind::User).is_empty());
    let results = tool_results(&lines);
    let d1 = result_of(&results, "d1");
    assert_eq!(d1.outcome, ToolOutcomeWord::Refused);
    assert!(d1.content.contains("evil/exfil"), "{}", d1.content);
    assert_eq!(result_of(&results, "d2").outcome, ToolOutcomeWord::Ok);
    let made = rooms.made();
    assert_eq!(made.len(), 1);
    assert!(made[0].2.contains(&user(LUCYNA)));
}

fn world_of_remote() -> World {
    world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"private","path":"diary.md"}),
            )]),
            prose("never."),
        ],
    )
}

/// 92.6 acceptance 2: the same sinks pass when the audience fits — a
/// {tgorka, Marta} session writes its session file, hands work to Dr
/// Lucyna Novak (read by both) and answers in a room of both. Its drive
/// write is past the label and stops only at the grant's first-write ask.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_sinks_pass_when_the_audience_fits() {
    let script = vec![
        calls(&[
            (
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/out.md","content":"shared"}),
            ),
            (
                "s1",
                "session_write",
                json!({"path":"notes.md","content":"shared"}),
            ),
            (
                "d1",
                "delegate",
                json!({"agent":"neuradrive/lucyna","brief":"Sort the shared inbox."}),
            ),
        ]),
        prose("done."),
    ];
    let mut world = world(
        ProviderKind::Ollama,
        &["drive_read", "drive_write", "session_write", "delegate"],
        script,
    );
    let delegations = Delegations::over(known_with_proxy());
    let mut served = world.delegating(&delegations);
    report(world.ask(&mut served, "share it").await);
    let lines = world.lines(SESSION);
    let results = tool_results(&lines);
    assert_eq!(result_of(&results, "w1").outcome, ToolOutcomeWord::Refused);
    assert!(
        result_of(&results, "w1")
            .content
            .contains(UNATTENDED_REFUSAL),
        "{}",
        result_of(&results, "w1").content
    );
    assert_eq!(result_of(&results, "s1").outcome, ToolOutcomeWord::Ok);
    assert!(world.dir(SESSION).join("notes.md").exists());
    assert_eq!(result_of(&results, "d1").outcome, ToolOutcomeWord::Ok);
    let made = delegations.made();
    assert_eq!(made.len(), 1);
    assert!(made[0].1.contains(&user(MARTA)));
    assert_eq!(final_edit(&world.room), "done.");
    // R90: every call that passed has its row, allowed under the agent's
    // own word for the tool, with its tier; the answer is no call.
    let rows = audit_rows(&world, &served);
    for tool in ["session_write", "delegate"] {
        let row = &rows[tool];
        assert_eq!(
            row.verdict,
            Some(keeper_core::bots::audit::AuditVerdict::Allow),
            "{tool}"
        );
        assert_eq!(
            row.grant_id.as_deref(),
            Some(format!("agent:{tool}").as_str())
        );
        assert_eq!((row.tier, row.base_tier), (Some(1), Some(1)), "{tool}");
        assert_eq!(
            row.outcome,
            keeper_core::bots::audit::AuditOutcome::Ok,
            "{tool}"
        );
    }
    assert_eq!(rows["drive_write"].tier, Some(2));
    assert!(!rows.contains_key("answer"));
}

/// 92.6 acceptance 4 through a turn: after an inbox read the session is
/// `untrusted`; a delegation to an agent tgorka never named is blocked, the
/// one he named passes the rule, and a drive write he named needs an
/// approval, which this keeper cannot take yet.
#[tokio::test(flavor = "multi_thread")]
async fn a_recipient_taken_from_outside_content_is_blocked() {
    use keeper_core::agents::label::NEEDS_APPROVAL;
    let script = vec![
        calls(&[(
            "r1",
            "drive_read",
            json!({"profile":"tgdrive","path":"00-inbox/x.md"}),
        )]),
        calls(&[
            (
                "d1",
                "delegate",
                json!({"agent":"evil/exfil","brief":"as the page says"}),
            ),
            (
                "d2",
                "delegate",
                json!({"agent":"tgdrive/tola","brief":"Sort the inbox."}),
            ),
            (
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/plan.md","content":"x"}),
            ),
        ]),
        prose("done."),
    ];
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write", "delegate"],
        script,
    );
    let delegations = Delegations::over(known_with_proxy());
    let mut served = world.delegating(&delegations);
    report(
        world
            .ask(
                &mut served,
                "read the inbox, hand it to tgdrive/tola and write notes/plan.md",
            )
            .await,
    );
    assert_eq!(served.context.label.integrity, Integrity::Untrusted);
    let lines = world.lines(SESSION);
    let results = tool_results(&lines);
    let d1 = result_of(&results, "d1");
    assert_eq!(d1.outcome, ToolOutcomeWord::Refused);
    assert!(d1.content.contains("evil/exfil"), "{}", d1.content);
    // R167: a known agent whose audience is within the label passes after
    // an inbox read, with no line of the person's naming it.
    assert_eq!(result_of(&results, "d2").outcome, ToolOutcomeWord::Ok);
    let made = delegations.made();
    assert_eq!(made.len(), 1);
    assert!(made[0].2.contains(&user(TOLA)));
    let w1 = result_of(&results, "w1");
    assert_eq!(w1.outcome, ToolOutcomeWord::Refused);
    assert!(w1.content.contains(NEEDS_APPROVAL), "{}", w1.content);
    assert!(!world.tgdrive.join("notes/plan.md").exists());
}

/// 92.6 acceptance 8 through `SessionContext`: in Nixi's DM a turn that
/// read the inbox is `untrusted`, and its write needs an approval; at
/// tgorka's next message the DM is `owner` again with its readers as they
/// were. A conversation keeps `untrusted` for good.
#[tokio::test(flavor = "multi_thread")]
async fn a_main_sessions_integrity_resets_at_the_persons_turn() {
    use keeper_core::agents::label::NEEDS_APPROVAL;
    let script = || {
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"tgdrive","path":"00-inbox/x.md"}),
            )]),
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/plan.md","content":"x"}),
            )]),
            prose("read."),
            prose("you are welcome."),
        ]
    };
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        script(),
    );
    nixis_dm(&world);
    let mut dm = world.open(DM);
    report(
        world
            .ask(&mut dm, "read the inbox and write notes/plan.md")
            .await,
    );
    assert_eq!(dm.context.label.integrity, Integrity::Untrusted);
    let readers = dm.context.label.readers.clone();
    let lines = world.lines(DM);
    let w1 = result_of(&tool_results(&lines), "w1").clone();
    assert!(w1.content.contains(NEEDS_APPROVAL), "{}", w1.content);
    report(world.ask(&mut dm, "thanks").await);
    assert_eq!(dm.context.label.integrity, Integrity::Owner);
    assert_eq!(dm.context.label.readers, readers);
    // The reset is a `label` line, so a reload reads the same.
    let reloaded = world.open(DM);
    assert_eq!(reloaded.context.label, dm.context.label);

    let mut world = world_read_by(
        &[TGORKA, MARTA],
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        script(),
    );
    let mut conversation = world.open(SESSION);
    report(
        world
            .ask(&mut conversation, "read the inbox and write notes/plan.md")
            .await,
    );
    report(world.ask(&mut conversation, "thanks").await);
    assert_eq!(conversation.context.label.integrity, Integrity::Untrusted);
}

// ---------------------------------------------------------------------------
// 93.1 and 93.4: every call has a tier, one stricter when nobody watches
// ---------------------------------------------------------------------------

const ELSEWHERE: &str = "active/2026-10-05-elsewhere";

/// A session of Nixi's of `kind` at `path`, `hop` deep, its label at
/// `integrity`.
fn session_kind(world: &World, path: &str, kind: SessionKind, hop: u8, integrity: Integrity) {
    let tg_decl = world.deps.drives["tgdrive"].clone();
    let mut agent = session_of(
        &world.tgdrive,
        path,
        &tg_decl,
        "nixi",
        kind,
        "!elsewhere:example.org",
    );
    agent.hop = hop;
    agent.label.integrity = integrity;
    write(
        &world.tgdrive,
        &format!("60-sessions/{path}/agent.toml"),
        &compose_session_agent_toml(&agent),
    );
}

/// One turn in the session of `kind` at `path`: tgorka's message where he
/// talks with Nixi, else the session card's scheduled window.
async fn one_turn(world: &mut World, path: &str, kind: SessionKind) -> ServedSession {
    use keeper_agent::agent::scheduled_arrival;
    use keeper_agent::cards::Scheduled;
    if kind == SessionKind::Scheduled {
        write(
            &world.tgdrive,
            &format!("60-sessions/{path}/card.md"),
            "---\ntags: [task]\ntitle: Notes\nstatus: todo\nassignee: nixi\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nDo it.\n",
        );
    }
    let mut served = world.open(path);
    let arrived = if kind == SessionKind::Scheduled {
        scheduled_arrival(
            &user(NIXI),
            &Scheduled::Run {
                card: "card.md".to_owned(),
                window: "2026-10-05T09:00:00.000Z".to_owned(),
                now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-05T09:30:00Z")
                    .expect("an instant")
                    .timestamp_millis(),
                utc_offset_minutes: 0,
            },
        )
        .expect("an arrival")
    } else {
        world.arrived(TGORKA, "do it")
    };
    report(world.serve(&mut served, arrived).await);
    served
}

/// The `tool_call` line's tier of call `id`.
fn line_tier(lines: &[LogLine], id: &str) -> u8 {
    kinds(lines, LineKind::ToolCall)
        .into_iter()
        .find_map(|line| match &line.body {
            LineBody::ToolCall(body) if body.call_id == id => Some(body.tier),
            _ => None,
        })
        .unwrap_or_else(|| panic!("a tool_call line for {id}"))
}

/// 93.4 acceptance 1 and 2: one `drive_write` outside the session, in four
/// sessions. In Nixi's DM, at a turn tgorka started, it is T2 with no
/// reason; handed on it is T3 `[delegated]`; in a scheduled run T3
/// `[unattended]`; in a session that is all three T3 once, with all three
/// reasons. Each is refused — nobody can approve yet — the file is never
/// written, and the audit row and the `tool_call` line say the tier.
#[tokio::test(flavor = "multi_thread")]
async fn the_same_write_is_stricter_when_nobody_watches() {
    use keeper_core::agents::label::NEEDS_APPROVAL;
    let script = || {
        vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"notes/out.md","content":"must not land"}),
            )]),
            prose("It needs a person."),
        ]
    };
    for (kind, hop, integrity, tier, raised, refusal) in [
        (
            SessionKind::Main,
            0,
            Integrity::Owner,
            2,
            None,
            UNATTENDED_REFUSAL,
        ),
        // Handed on: a hop of 1 in the person's own conversation.
        (
            SessionKind::Conversation,
            1,
            Integrity::Agent,
            3,
            Some("delegated"),
            UNATTENDED_REFUSAL,
        ),
        (
            SessionKind::Scheduled,
            0,
            Integrity::Agent,
            3,
            Some("unattended"),
            UNATTENDED_REFUSAL,
        ),
        // A scheduled session's kind is not delegated, but its hop is.
        (
            SessionKind::Scheduled,
            1,
            Integrity::Untrusted,
            3,
            Some("delegated,unattended,untrusted"),
            NEEDS_APPROVAL,
        ),
    ] {
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "drive_write"],
            script(),
        );
        let path = if kind == SessionKind::Main {
            nixis_dm(&world);
            DM
        } else {
            session_kind(&world, ELSEWHERE, kind, hop, integrity);
            ELSEWHERE
        };
        let served = one_turn(&mut world, path, kind).await;
        assert!(!world.tgdrive.join("notes/out.md").exists(), "{kind:?}");
        let lines = world.lines(path);
        let result = result_of(&tool_results(&lines), "w1").clone();
        assert_eq!(result.content, format!("Refused: {refusal}"), "{kind:?}");
        assert_eq!(line_tier(&lines, "w1"), tier, "{kind:?}");
        let row = &audit_rows(&world, &served)["drive_write"];
        assert_eq!(
            (row.tier, row.base_tier, row.raised_by.as_deref()),
            (Some(tier), Some(2), raised),
            "{kind:?}"
        );
        assert_eq!(row.outcome, keeper_core::bots::audit::AuditOutcome::Refused);
    }
}

/// 93.4 acceptance 6: in a scheduled run a write to the agent's own
/// machine file is T5 and never done; the model is told why, for the
/// person, and the row says T5.
#[tokio::test(flavor = "multi_thread")]
async fn a_forbidden_action_is_never_done() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write"],
        vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile":"tgdrive","path":"80-agents/nixi/agent.toml","content":"version = 1\n"}),
            )]),
            prose("I may not."),
        ],
    );
    let before = std::fs::read_to_string(world.tgdrive.join("80-agents/nixi/agent.toml"))
        .expect("agent.toml");
    session_kind(
        &world,
        ELSEWHERE,
        SessionKind::Scheduled,
        0,
        Integrity::Agent,
    );
    let served = one_turn(&mut world, ELSEWHERE, SessionKind::Scheduled).await;
    assert_eq!(
        std::fs::read_to_string(world.tgdrive.join("80-agents/nixi/agent.toml"))
            .expect("agent.toml"),
        before
    );
    let lines = world.lines(ELSEWHERE);
    let result = result_of(&tool_results(&lines), "w1").clone();
    assert_eq!(
        result.content,
        format!("Refused: {}", keeper_core::agents::tier::FORBIDDEN)
    );
    assert_eq!(line_tier(&lines, "w1"), 5);
    let second = world.stub.requests()[1].to_string();
    assert!(
        second.contains("keeper never lets an agent do this"),
        "{second}"
    );
    let row = &audit_rows(&world, &served)["drive_write"];
    assert_eq!(
        (row.tier, row.base_tier, row.raised_by.as_deref()),
        (Some(5), Some(5), Some("unattended"))
    );
}

/// 93.4 acceptance 1 (R171) and 3: T0 and T1 calls run in a session that
/// is delegated, unattended and untrusted at once — not raised, their row
/// naming the reasons that held — each audited before it ran; and when
/// the audit row cannot be written, the call is refused and nothing runs.
#[tokio::test(flavor = "multi_thread")]
async fn every_classification_is_audited_before_the_effect() {
    let script = || {
        vec![
            calls(&[
                (
                    "r1",
                    "drive_read",
                    json!({"profile":"tgdrive","path":"notes/hello.md"}),
                ),
                (
                    "s1",
                    "session_write",
                    json!({"path":"notes.md","content":"kept"}),
                ),
            ]),
            prose("done."),
        ]
    };
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "session_write"],
        script(),
    );
    session_kind(
        &world,
        ELSEWHERE,
        SessionKind::Scheduled,
        1,
        Integrity::Untrusted,
    );
    let served = one_turn(&mut world, ELSEWHERE, SessionKind::Scheduled).await;
    let lines = world.lines(ELSEWHERE);
    let results = tool_results(&lines);
    assert_eq!(result_of(&results, "r1").outcome, ToolOutcomeWord::Ok);
    assert_eq!(result_of(&results, "s1").outcome, ToolOutcomeWord::Ok);
    assert!(world.dir(ELSEWHERE).join("notes.md").exists());
    assert_eq!((line_tier(&lines, "r1"), line_tier(&lines, "s1")), (0, 1));
    let rows = audit_rows(&world, &served);
    for (tool, tier) in [("drive_read", 0), ("session_write", 1)] {
        let row = &rows[tool];
        assert_eq!(
            (row.tier, row.base_tier, row.raised_by.as_deref()),
            (
                Some(tier),
                Some(tier),
                Some("delegated,unattended,untrusted")
            ),
            "{tool}"
        );
        assert_eq!(row.outcome, keeper_core::bots::audit::AuditOutcome::Ok);
    }

    // `keeper.db` is a folder now: no row can be written, so nothing runs.
    let mut world = crate::world(
        ProviderKind::OpenAi,
        &["drive_read", "session_write"],
        script(),
    );
    session_kind(
        &world,
        ELSEWHERE,
        SessionKind::Scheduled,
        1,
        Integrity::Untrusted,
    );
    let broken = world.tgdrive.with_file_name("broken");
    std::fs::create_dir_all(broken.join("keeper.db")).expect("a folder");
    world.deps.data_dir = broken;
    one_turn(&mut world, ELSEWHERE, SessionKind::Scheduled).await;
    let results = tool_results(&world.lines(ELSEWHERE));
    for call in ["r1", "s1"] {
        let result = result_of(&results, call);
        assert_eq!(result.outcome, ToolOutcomeWord::Refused, "{call}");
        assert!(
            result.content.contains("could not record this tool call"),
            "{call}: {}",
            result.content
        );
    }
    assert!(!world.dir(ELSEWHERE).join("notes.md").exists());
}

/// 93.1 acceptance 1 through a turn (S-21): a `card_update` that sets a
/// schedule is T3 even in the person's own conversation, so it asks a
/// person and — with nobody to ask — is refused; the card is unchanged.
/// A status change of the same card is T1 and lands.
#[tokio::test(flavor = "multi_thread")]
async fn a_schedule_an_agent_sets_needs_a_person() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "card_update"],
        vec![
            calls(&[
                (
                    "c1",
                    "card_update",
                    json!({"card": "card.md", "fields": {"schedule": "@daily"}}),
                ),
                (
                    "c2",
                    "card_update",
                    json!({"card": "card.md", "fields": {"status": "done"}}),
                ),
            ]),
            prose("Scheduling needs you."),
        ],
    );
    write(&world.dir(SESSION), "card.md", CARD);
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "run it daily, and close it").await);
    let lines = world.lines(SESSION);
    let results = tool_results(&lines);
    assert_eq!(
        result_of(&results, "c1").content,
        format!("Refused: {UNATTENDED_REFUSAL}")
    );
    assert_eq!(result_of(&results, "c2").outcome, ToolOutcomeWord::Ok);
    assert_eq!((line_tier(&lines, "c1"), line_tier(&lines, "c2")), (3, 1));
    let card = std::fs::read_to_string(world.dir(SESSION).join("card.md")).expect("card");
    assert!(
        card.contains("status: done") && !card.contains("schedule"),
        "{card}"
    );
}

/// The rows of `tool` in `served`'s session, oldest first.
fn rows_of(
    world: &World,
    served: &ServedSession,
    tool: &str,
) -> Vec<keeper_core::bots::audit::AuditRow> {
    audit_list(world, served)
        .into_iter()
        .filter(|row| row.tool == tool)
        .collect()
}

/// AD-392 T5 where the write lands: keeper's own files are forbidden
/// whatever the call spelled — another case of the name, a folder link
/// inside the session that leads to its `approvals/`, a second mounted
/// drive — and a link from this session into another session is a write
/// outside it (T2). Each is refused, nothing lands, and its one row says
/// the tier the landing gave.
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_write_is_classified_where_it_lands() {
    use keeper_core::agents::tier::FORBIDDEN;
    let other = format!("60-sessions/{SESSION}/workspace/other/x.md");
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write", "session_write"],
        vec![
            calls(&[
                (
                    "w1",
                    "drive_write",
                    json!({"profile":"tgdrive","path":"80-agents/nixi/Agent.toml","content":"x"}),
                ),
                (
                    "s1",
                    "session_write",
                    json!({"path":"Approvals/01J.json","content":"{}"}),
                ),
                (
                    "s2",
                    "session_write",
                    json!({"path":"workspace/back/approvals/01J.json","content":"{}"}),
                ),
                (
                    "w2",
                    "drive_write",
                    json!({"profile":"private","path":"60-sessions/active/2026-10-05-x/approvals/01J.json","content":"{}"}),
                ),
                (
                    "w3",
                    "drive_write",
                    json!({"profile":"tgdrive","path": other,"content":"x"}),
                ),
            ]),
            prose("None of it was mine to do."),
        ],
    );
    session_kind(
        &world,
        ELSEWHERE,
        SessionKind::Conversation,
        0,
        Integrity::Agent,
    );
    let here = world.dir(SESSION);
    std::fs::create_dir_all(here.join("workspace")).expect("workspace");
    std::os::unix::fs::symlink("..", here.join("workspace/back")).expect("a link to the session");
    std::os::unix::fs::symlink(world.dir(ELSEWHERE), here.join("workspace/other"))
        .expect("a link to another session");
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "write them").await);
    let lines = world.lines(SESSION);
    let results = tool_results(&lines);
    for call in ["w1", "s1", "s2"] {
        assert_eq!(
            result_of(&results, call).content,
            format!("Refused: {FORBIDDEN}"),
            "{call}"
        );
        assert_eq!(line_tier(&lines, call), 5, "{call}");
    }
    assert_eq!(result_of(&results, "w2").outcome, ToolOutcomeWord::Refused);
    assert_eq!(line_tier(&lines, "w2"), 5);
    assert_eq!(
        result_of(&results, "w3").content,
        format!("Refused: {UNATTENDED_REFUSAL}")
    );
    assert_eq!(line_tier(&lines, "w3"), 2);
    assert!(!here.join("approvals").exists());
    assert!(!world.dir(ELSEWHERE).join("x.md").exists());
    let writes: Vec<_> = rows_of(&world, &served, "drive_write")
        .iter()
        .map(|row| (row.tier, row.outcome))
        .collect();
    use keeper_core::bots::audit::AuditOutcome::Refused;
    assert_eq!(
        writes,
        [(Some(5), Refused), (Some(5), Refused), (Some(2), Refused)]
    );
    let sessions: Vec<_> = rows_of(&world, &served, "session_write")
        .iter()
        .map(|row| (row.tier, row.outcome))
        .collect();
    assert_eq!(sessions, [(Some(5), Refused), (Some(5), Refused)]);
}

/// R90 on every branch: a drive verb the agent was not given, a read of a
/// folder keeper does not hold and a card tool it was not given are each
/// refused with exactly one row carrying the call's tier.
#[tokio::test(flavor = "multi_thread")]
async fn a_call_refused_before_its_grant_has_one_classified_row() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read"],
        vec![
            calls(&[
                (
                    "w1",
                    "drive_write",
                    json!({"profile":"tgdrive","path":"notes/x.md","content":"x"}),
                ),
                (
                    "r1",
                    "drive_read",
                    json!({"profile":"nowhere","path":"a.md"}),
                ),
                (
                    "c1",
                    "card_update",
                    json!({"card":"card.md","fields":{"status":"done"}}),
                ),
            ]),
            prose("I could not."),
        ],
    );
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "try").await);
    let results = tool_results(&world.lines(SESSION));
    for call in ["w1", "r1", "c1"] {
        assert_eq!(
            result_of(&results, call).outcome,
            ToolOutcomeWord::Refused,
            "{call}"
        );
    }
    let rows: Vec<_> = audit_list(&world, &served)
        .iter()
        .map(|row| (row.tool.clone(), row.tier, row.outcome))
        .collect();
    use keeper_core::bots::audit::AuditOutcome::Refused;
    assert_eq!(
        rows,
        [
            ("drive_write".to_owned(), Some(2), Refused),
            ("drive_read".to_owned(), Some(0), Refused),
            ("card_update".to_owned(), Some(1), Refused),
        ]
    );
}

/// R82: T5 is never an approval's to give. In an `untrusted` session a
/// write to the agent's own `agent.toml` and a `session_write` into
/// `approvals/` answer FORBIDDEN, not the approval sentence, and each row
/// says T5.
#[tokio::test(flavor = "multi_thread")]
async fn an_untrusted_forbidden_action_is_forbidden_not_approvable() {
    use keeper_core::agents::tier::FORBIDDEN;
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "drive_write", "session_write"],
        vec![
            calls(&[
                (
                    "w1",
                    "drive_write",
                    json!({"profile":"tgdrive","path":"80-agents/nixi/agent.toml","content":"x"}),
                ),
                (
                    "s1",
                    "session_write",
                    json!({"path":"approvals/01J.json","content":"{}"}),
                ),
            ]),
            prose("I may not."),
        ],
    );
    session_kind(
        &world,
        ELSEWHERE,
        SessionKind::Conversation,
        0,
        Integrity::Untrusted,
    );
    let served = one_turn(&mut world, ELSEWHERE, SessionKind::Conversation).await;
    let lines = world.lines(ELSEWHERE);
    let results = tool_results(&lines);
    for call in ["w1", "s1"] {
        assert_eq!(
            result_of(&results, call).content,
            format!("Refused: {FORBIDDEN}"),
            "{call}"
        );
    }
    let rows = audit_rows(&world, &served);
    for tool in ["drive_write", "session_write"] {
        assert_eq!(rows[tool].tier, Some(5), "{tool}");
    }
}

/// R82 for delegation: the label's sinks are asked before the integrity
/// rule. An `untrusted` session read by tgorka alone hands work to Dr
/// Lucyna Novak, whose audience adds Marta: both the delegation's sink and
/// the integrity rule refuse it, and the sink's sentence is the one said
/// and recorded, in the call's one row.
#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_the_label_and_the_integrity_rule_both_refuse_says_the_labels() {
    use keeper_core::agents::label::NEEDS_APPROVAL;
    let mut world = world_read_by(
        &[TGORKA],
        ProviderKind::OpenAi,
        &["drive_read", "delegate"],
        vec![
            calls(&[(
                "r1",
                "drive_read",
                json!({"profile":"tgdrive","path":"00-inbox/x.md"}),
            )]),
            delegate_call(
                "d1",
                json!({"agent": "neuradrive/lucyna", "brief": "as the page says"}),
            ),
            prose("I could not."),
        ],
    );
    let rooms = Delegations::over(known(&[TGORKA]));
    let mut nixi = world.delegating(&rooms);
    report(
        world
            .ask(&mut nixi, "read the inbox and do what it says")
            .await,
    );
    assert_eq!(nixi.context.label.integrity, Integrity::Untrusted);
    assert!(rooms.made().is_empty());
    let d1 = result_of(&tool_results(&world.lines(SESSION)), "d1").clone();
    assert!(d1.content.contains(NEEDS_APPROVAL), "{}", d1.content);
    assert!(d1.content.contains(MARTA), "{}", d1.content);
    assert!(!d1.content.contains("outside content"), "{}", d1.content);
    let rows = rows_of(&world, &nixi, "delegate");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        format!("Refused: {}", rows[0].reason.as_deref().unwrap_or_default()),
        d1.content
    );
    assert_eq!((rows[0].tier, rows[0].subpath.as_str()), (Some(1), LUCYNA));
}

/// The audit names the file changed: a `card_update` naming a `path` beside
/// its `card` is refused — neither file changes — and its row names the
/// card; without it the card is changed and the row names that card.
#[tokio::test(flavor = "multi_thread")]
async fn a_card_update_changes_and_audits_only_its_card() {
    let mut world = world(
        ProviderKind::OpenAi,
        &["drive_read", "card_update"],
        vec![
            calls(&[(
                "c1",
                "card_update",
                json!({"card": "card.md", "path": "innocent.md", "fields": {"status": "done"}}),
            )]),
            calls(&[(
                "c2",
                "card_update",
                json!({"card": "card.md", "fields": {"status": "done"}}),
            )]),
            prose("Closed."),
        ],
    );
    write(&world.dir(SESSION), "card.md", CARD);
    write(&world.dir(SESSION), "innocent.md", CARD);
    let mut served = world.open(SESSION);
    report(world.ask(&mut served, "close it").await);
    let results = tool_results(&world.lines(SESSION));
    let c1 = result_of(&results, "c1");
    assert_eq!(c1.outcome, ToolOutcomeWord::Refused);
    assert!(c1.content.contains("path"), "{}", c1.content);
    assert_eq!(result_of(&results, "c2").outcome, ToolOutcomeWord::Ok);
    let read = |name: &str| std::fs::read_to_string(world.dir(SESSION).join(name)).expect("file");
    assert_eq!(read("innocent.md"), CARD);
    assert!(read("card.md").contains("status: done"));
    let card = format!("60-sessions/{SESSION}/card.md");
    let rows: Vec<_> = rows_of(&world, &served, "card_update")
        .iter()
        .map(|row| (row.subpath.clone(), row.outcome))
        .collect();
    use keeper_core::bots::audit::AuditOutcome;
    assert_eq!(
        rows,
        [
            (card.clone(), AuditOutcome::Refused),
            (card, AuditOutcome::Ok)
        ]
    );
}

/// Epic 93.2: a run that parks and resumes, consumed exactly once.
mod parks {
    use super::*;
    use keeper_agent::approvals::{
        decision_content, effect_unknown, ApprovalRoom, Consumed, ConsumedRead, DecisionSource,
        RoomFuture, ALREADY_DECIDED, APPROVAL_ENDED, DECISION_UNWRITTEN, DENIED, EXPIRED,
        NOT_HOLDER, NOT_RUN, SUPERSEDED, SUPERSEDED_RESULT, UNREADABLE_DECISION,
    };
    use keeper_core::agents::agentd::TrustEntry;
    use keeper_core::agents::approval::{parse_record, sha256_hex, ApprovalRecord, Decision};
    use keeper_core::agents::events::{ConsumedContent, APPROVAL_REQUEST};
    use keeper_core::agents::log::{ApprovalBody, ApprovalState};
    use keeper_core::agents::matrix::AgentMatrixError;
    use keeper_core::agents::trust::{
        Anchor, OwnAccount, Published, KEYS_UNKNOWN, KEY_MOVED, NOT_AN_APPROVER, THIS_DEVICE,
        UNSIGNED_DEVICE,
    };
    use matrix_sdk::ruma::DeviceId;

    /// tgorka's master key, as pinned and as published.
    const KEY: &str = "ed25519:AbCdEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";
    /// Marta's.
    const MARTAS: &str = "ed25519:MaRtAfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";
    /// tgorka's after he reset his identity.
    const RESET: &str = "ed25519:ZZZZEfGhIjKlMnOpQrStUvWxYz0123456789+/AbCdE";
    /// A device of tgorka's that nobody signed: a fresh login.
    const NEW: &str = "NEW";

    /// The trust adapter's double: what each person's homeserver publishes
    /// (every device cross-signed by its owner but [`NEW`]; each person's
    /// key, which a test may move) under this host's anchor.
    struct Admit {
        anchor: Anchor,
        keys: Mutex<std::collections::HashMap<String, String>>,
        /// The homeserver's answer is not whole: the lookup fails, with an
        /// error that quotes [`SECRET`].
        unknown: std::sync::atomic::AtomicBool,
        /// Run once while the next lookup is in flight.
        during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    /// A value a sender or a server wrote that keeper must never repeat in
    /// its own logs.
    const SECRET: &str = "SECRET-7f3a9c";

    impl Admit {
        fn over(anchor: Anchor) -> Arc<Admit> {
            Arc::new(Admit {
                anchor,
                keys: Mutex::new(
                    [(TGORKA, KEY), (MARTA, MARTAS)]
                        .map(|(user, key)| (user.to_owned(), key.to_owned()))
                        .into(),
                ),
                unknown: Default::default(),
                during: Mutex::new(None),
            })
        }

        /// A headless host with tgorka and Marta pinned.
        fn pinned() -> Arc<Admit> {
            Admit::over(Anchor::Pinned(
                [(TGORKA, KEY), (MARTA, MARTAS)]
                    .map(|(person, key)| TrustEntry {
                        user: user(person),
                        master_key: Some(key.to_owned()),
                        proxy: None,
                    })
                    .into(),
            ))
        }

        /// The desktop signed in as tgorka, verified, its messenger device
        /// `MAC` (R87).
        fn desktop() -> Arc<Admit> {
            Admit::over(Anchor::Desktop(vec![OwnAccount {
                user: user(TGORKA),
                device_id: "MAC".to_owned(),
                own_identity_verified: true,
                master_key: Some(KEY.to_owned()),
            }]))
        }

        fn publish(&self, person: &str, key: &str) {
            self.keys
                .lock()
                .expect("lock")
                .insert(person.to_owned(), key.to_owned());
        }
    }

    impl DecisionSource for Admit {
        fn anchor(&self) -> &Anchor {
            &self.anchor
        }

        fn published<'a>(
            &'a self,
            user: &'a UserId,
            device: &'a DeviceId,
        ) -> RoomFuture<'a, Published> {
            Box::pin(async move {
                let during = self.during.lock().expect("lock").take();
                if let Some(during) = during {
                    tokio::task::yield_now().await;
                    during();
                }
                if self.unknown.load(Ordering::SeqCst) {
                    return Err(AgentMatrixError::Other(format!(
                        "federation failed: {SECRET}"
                    )));
                }
                Ok(Published {
                    cross_signed_by_owner: device.as_str() != NEW,
                    master_key: self.keys.lock().expect("lock").get(user.as_str()).cloned(),
                })
            })
        }
    }

    /// The session room as every host of the test sees it: its `consumed`
    /// events in the server's order, whether this host holds the claim, and
    /// whether the server accepts a `consumed` event. Each request yields
    /// once, so two hosts interleave as two clients would.
    #[derive(Default)]
    struct Approvals {
        consumed: Mutex<Vec<Consumed>>,
        elsewhere: std::sync::atomic::AtomicBool,
        down: std::sync::atomic::AtomicBool,
        uploads: AtomicUsize,
        /// The server answers no read of the room.
        unread: std::sync::atomic::AtomicBool,
        /// Every read stops at its bound with history left, finding none.
        truncated: std::sync::atomic::AtomicBool,
        /// An upload fails.
        no_uploads: std::sync::atomic::AtomicBool,
        /// Where each read of the room began.
        froms: Mutex<Vec<Option<OwnedEventId>>>,
        /// A `consumed` the server takes is followed by reads that fail,
        /// until the test lets them through: accepted, not read back.
        unread_after_consume: std::sync::atomic::AtomicBool,
        /// The worker whose busy flag each request of the room reads.
        probe: Mutex<Option<Arc<keeper_agent::agent::Activity>>>,
        /// What the probe read, request by request.
        busy_seen: Mutex<Vec<bool>>,
    }

    impl Approvals {
        fn look(&self) {
            if let Some(activity) = self.probe.lock().expect("lock").as_ref() {
                self.busy_seen
                    .lock()
                    .expect("lock")
                    .push(activity.busy.load(Ordering::SeqCst));
            }
        }
    }

    impl Approvals {
        fn events(&self) -> Vec<Consumed> {
            self.consumed.lock().expect("lock").clone()
        }

        /// Another copy's `consumed` event, accepted by the server.
        fn consumed_by(&self, id: &str, host: &str) {
            let mut consumed = self.consumed.lock().expect("lock");
            consumed.push(Consumed {
                event: OwnedEventId::try_from(format!("${host}:example.org")).expect("id"),
                content: ConsumedContent {
                    v: 1,
                    id: id.to_owned(),
                    epoch: 4,
                    host: host.to_owned(),
                },
            });
        }
    }

    impl ApprovalRoom for Approvals {
        fn upload(&self, bytes: Vec<u8>) -> RoomFuture<'_, Value> {
            Box::pin(async move {
                if self.no_uploads.load(Ordering::SeqCst) {
                    return Err(AgentMatrixError::Network("the upload failed".to_owned()));
                }
                self.uploads.fetch_add(1, Ordering::SeqCst);
                Ok(json!({"url": "mxc://example.org/args", "bytes": bytes.len()}))
            })
        }

        fn consume(&self, content: ConsumedContent) -> RoomFuture<'_, OwnedEventId> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                self.look();
                if self.down.load(Ordering::SeqCst) {
                    return Err(AgentMatrixError::Network("the server went away".to_owned()));
                }
                if self.unread_after_consume.load(Ordering::SeqCst) {
                    self.unread.store(true, Ordering::SeqCst);
                }
                let mut consumed = self.consumed.lock().expect("lock");
                let event =
                    OwnedEventId::try_from(format!("$consumed{}:example.org", consumed.len() + 1))
                        .expect("id");
                consumed.push(Consumed {
                    event: event.clone(),
                    content,
                });
                Ok(event)
            })
        }

        fn consumed<'a>(
            &'a self,
            id: &'a str,
            from: Option<&'a matrix_sdk::ruma::EventId>,
        ) -> RoomFuture<'a, ConsumedRead> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                self.look();
                self.froms
                    .lock()
                    .expect("lock")
                    .push(from.map(ToOwned::to_owned));
                if self.unread.load(Ordering::SeqCst) {
                    return Err(AgentMatrixError::Network("the read failed".to_owned()));
                }
                if self.truncated.load(Ordering::SeqCst) {
                    return Ok(ConsumedRead::default());
                }
                Ok(ConsumedRead {
                    consumed: self
                        .events()
                        .into_iter()
                        .filter(|consumed| consumed.content.id == id)
                        .collect(),
                    complete: true,
                })
            })
        }

        fn holds(&self, _: u64) -> RoomFuture<'_, bool> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                Ok(!self.elsewhere.load(Ordering::SeqCst))
            })
        }
    }

    const NOTE: &str = "10-notes/a.md";
    /// What `NOTE` holds before anything is approved.
    const ORIGINAL: &str = "needle here\n";

    fn write_note(id: &str, content: &str) -> (&'static str, &'static str, Value) {
        let id: &'static str = Box::leak(id.to_owned().into_boxed_str());
        (
            id,
            "drive_write",
            json!({"profile": "tgdrive", "path": NOTE, "content": content}),
        )
    }

    /// Nixi with a decision source, and the room double her sessions use.
    fn deciding(script: Vec<Completion>) -> (World, Arc<Approvals>) {
        let mut world = world(ProviderKind::OpenAi, &["drive_read", "drive_write"], script);
        world.deps.decisions = Some(Admit::pinned());
        (world, Arc::new(Approvals::default()))
    }

    fn open(world: &World, approvals: &Arc<Approvals>) -> ServedSession {
        let mut served = world.open(SESSION);
        served.approval_room = Some(Arc::clone(approvals) as Arc<dyn ApprovalRoom>);
        served
    }

    impl World {
        fn approvals(&self) -> PathBuf {
            self.dir(SESSION).join("approvals")
        }

        /// The session's one approval record, read strictly.
        fn record(&self) -> ApprovalRecord {
            self.record_in(SESSION)
        }

        /// The one approval record of the session at `path`, read strictly.
        fn record_in(&self, path: &str) -> ApprovalRecord {
            let mut records: Vec<PathBuf> = std::fs::read_dir(self.dir(path).join("approvals"))
                .expect("approvals")
                .map(|entry| entry.expect("entry").path())
                .filter(|path| {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    name.ends_with(".json")
                        && !name.ends_with(".decision.json")
                        && !name.ends_with(".round.json")
                })
                .collect();
            assert_eq!(records.len(), 1, "{records:?}");
            parse_record(&std::fs::read_to_string(records.remove(0)).expect("read"))
                .expect("a strict record")
        }

        /// Rewrite the record on disk, as only a test does.
        fn rewrite(&self, record: &ApprovalRecord) {
            std::fs::write(
                self.approvals().join(format!("{}.json", record.id)),
                serde_json::to_string(record).expect("json"),
            )
            .expect("rewrite");
        }

        /// tgorka's decision on `record`, from his cross-signed phone.
        fn decision(&mut self, record: &ApprovalRecord, decision: Decision) -> Arrived {
            self.decision_from(TGORKA, "PHONE", decision_content(record, decision, None))
        }

        /// A decision with `content`, sealed by `sender`'s `device`.
        fn decision_from(&mut self, sender: &str, device: &str, content: Value) -> Arrived {
            let mut arrived = self.event(sender, Arrival::Decision { sealed: true }, content);
            arrived.device = Some(device.into());
            arrived
        }

        fn approval_lines(&self) -> Vec<ApprovalBody> {
            kinds(&self.lines(SESSION), LineKind::Approval)
                .iter()
                .map(|line| match &line.body {
                    LineBody::Approval(body) => body.clone(),
                    _ => unreachable!(),
                })
                .collect()
        }

        fn note(&self) -> Option<String> {
            std::fs::read_to_string(self.tgdrive.join(NOTE)).ok()
        }
    }

    /// Park Nixi on a write of `content`, the turn's next completions
    /// `after`.
    async fn parked(
        content: &str,
        after: Vec<Completion>,
    ) -> (World, Arc<Approvals>, ServedSession, ApprovalRecord) {
        let mut script = vec![calls(&[write_note("w1", content)])];
        script.extend(after);
        let (mut world, approvals) = deciding(script);
        let mut served = open(&world, &approvals);
        let report = report(world.ask(&mut served, "write a note").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();
        (world, approvals, served, record)
    }

    fn results(world: &World) -> Vec<ToolResultBody> {
        tool_results(&world.lines(SESSION))
    }

    /// 93.2 AC1: the record is on disk, strict, before the request is sent;
    /// its checkpoint is the SHA-256 of the chunk through the named line;
    /// the call has no result and nothing ran; no thread waits — the worker
    /// returned and `busy` is clear.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_run_that_needs_a_person_parks_and_holds_nothing() {
        let (mut world, approvals) = deciding(vec![calls(&[write_note("w1", "after approval")])]);
        *world.room.witness.lock().expect("lock") = Some(world.approvals());
        let mut served = open(&world, &approvals);
        let (queue, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
        queue
            .send(world.arrived(TGORKA, "write a note"))
            .expect("queued");
        drop(queue);
        let busy = keeper_agent::agent::Activity::default();
        let (_handle, signal) = chat::cancellation();
        served
            .serve_arrivals(
                &world.deps,
                world.room.clone(),
                Vec::new(),
                &mut arrivals,
                signal,
                &busy,
            )
            .await;
        assert!(
            !busy.busy.load(Ordering::SeqCst),
            "the worker holds nothing"
        );
        assert!(served.waiting());
        assert_eq!(world.note().as_deref(), Some(ORIGINAL), "nothing ran");

        let record = world.record();
        assert_eq!(
            record.action.summary,
            "Write `10-notes/a.md` in tgdrive (14 bytes)"
        );
        assert_eq!(record.risk.tier, 2);
        assert_eq!(
            record.preconditions.files[0].sha256.as_deref(),
            Some(sha256_hex(ORIGINAL.as_bytes()).as_str())
        );
        assert_eq!(
            *world.room.records_at_request.lock().expect("lock"),
            Some(2),
            "the round file and the record were on disk when the request went out"
        );
        let requests = world.sent_of(APPROVAL_REQUEST);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["id"], record.id.as_str());
        assert_eq!(
            requests[0]["binding_digest"],
            record.binding_digest.as_str()
        );

        let chunk =
            std::fs::read(world.dir(SESSION).join(&record.checkpoint.chunk)).expect("chunk");
        let named = format!("\"id\":\"{}\"", record.checkpoint.through);
        let at = String::from_utf8_lossy(&chunk)
            .find(&named)
            .expect("the named line");
        let end = at
            + chunk[at..]
                .iter()
                .position(|b| *b == b'\n')
                .expect("its end")
            + 1;
        assert_eq!(sha256_hex(&chunk[..end]), record.checkpoint.sha256);

        assert!(results(&world).is_empty(), "the parked call has no result");
        let approvals_logged = world.approval_lines();
        assert_eq!(approvals_logged.len(), 1);
        assert_eq!(approvals_logged[0].state, ApprovalState::Requested);
        let status = world.sent_of(keeper_core::agents::events::STATUS);
        assert_eq!(status.last().expect("a status")["run"], "blocked");
    }

    /// 93.2 AC2: a valid decision runs the action once; the same decision
    /// again, or a second one, runs nothing; a restart runs nothing more.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approval_is_consumed_exactly_once() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided.clone()).await,
            Outcome::Decided
        ));
        assert_eq!(world.note().as_deref(), Some("once"));
        assert_eq!(approvals.events().len(), 1);
        assert!(world
            .approvals()
            .join(format!("{}.decision.json", record.id))
            .exists());
        let logged = world.approval_lines();
        let states: Vec<ApprovalState> = logged.iter().map(|body| body.state).collect();
        assert_eq!(
            states,
            [
                ApprovalState::Requested,
                ApprovalState::Decided,
                ApprovalState::Consumed
            ]
        );
        assert_eq!(logged[1].decision.as_deref(), Some("approve"));
        let consumed_line = kinds(&world.lines(SESSION), LineKind::Approval)[2].clone();
        assert_eq!(
            consumed_line.matrix_event.as_deref().map(|e| e.as_str()),
            Some(approvals.events()[0].event.as_str())
        );
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Ok);
        assert!(!served.waiting());
        // The park's row is the run's row, closed `ok` (R172).
        let rows = rows_of(&world, &served, "drive_write");
        assert_eq!(
            rows.iter()
                .map(|row| (row.outcome, row.approval.as_deref()))
                .collect::<Vec<_>>(),
            [(
                keeper_core::bots::audit::AuditOutcome::Ok,
                Some(record.id.as_str())
            )]
        );

        // The file is touched by hand: nothing below may write it again.
        std::fs::write(world.tgdrive.join(NOTE), "touched").expect("touch");
        assert!(matches!(
            world.serve(&mut served, decided).await,
            Outcome::Duplicate
        ));
        // Another decision after it ran is logged ignored and runs nothing
        // (R80).
        let again = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, again).await,
            Outcome::Duplicate
        ));
        assert_eq!(
            world
                .approval_lines()
                .pop()
                .expect("a line")
                .reason
                .as_deref(),
            Some(APPROVAL_ENDED)
        );
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(world.note().as_deref(), Some("touched"));
        assert_eq!(approvals.events().len(), 1);
        assert_eq!(tool_results(&world.lines(SESSION)).len(), 1);
    }

    /// A `drive_write` to a note that is not there classifies by where it
    /// would land (T2 outside the session) and parks, its record pinning
    /// the file as absent; the call's one audit row waits pending, marked
    /// with the approval. Approved, the drive refuses it — `drive_write`
    /// never creates a file (AD-102) — and that same row closes refused
    /// (R172): nothing is created and no second row is written.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approved_write_to_a_missing_note_keeps_its_one_row() {
        use keeper_core::bots::audit::AuditOutcome;
        const NEW: &str = "10-notes/fresh/b.md";
        let (mut world, approvals) = deciding(vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile": "tgdrive", "path": NEW, "content": "brand new"}),
            )]),
            prose("It is not there."),
        ]);
        let mut served = open(&world, &approvals);
        let report = report(world.ask(&mut served, "write a new note").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();
        assert_eq!(record.risk.tier, 2);
        assert_eq!(record.preconditions.files[0].sha256, None);
        let parked = rows_of(&world, &served, "drive_write");
        assert_eq!(
            parked
                .iter()
                .map(|row| (row.outcome, row.approval.as_deref()))
                .collect::<Vec<_>>(),
            [(AuditOutcome::Pending, Some(record.id.as_str()))]
        );

        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided).await,
            Outcome::Decided
        ));
        assert!(!world.tgdrive.join(NEW).exists());
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Refused, "{results:?}");
        let rows = rows_of(&world, &served, "drive_write");
        assert_eq!(
            rows.iter()
                .map(|row| (row.id, row.outcome, row.approval.as_deref()))
                .collect::<Vec<_>>(),
            [(
                parked[0].id,
                AuditOutcome::Refused,
                Some(record.id.as_str())
            )]
        );
    }

    /// R172 through the session tools: a `card_update` that sets a schedule
    /// (T3) parks with its row pending; approved, it lands, and the run
    /// closes that same row `ok` — one row for the call, marked with its
    /// approval.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approved_card_update_closes_the_row_its_park_left() {
        use keeper_core::bots::audit::AuditOutcome;
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "card_update"],
            vec![
                calls(&[(
                    "c1",
                    "card_update",
                    json!({"card": "card.md", "fields": {"schedule": "@daily"}}),
                )]),
                prose("Scheduled."),
            ],
        );
        world.deps.decisions = Some(Admit::pinned());
        let approvals = Arc::new(Approvals::default());
        write(&world.dir(SESSION), "card.md", CARD);
        let mut served = open(&world, &approvals);
        let report = report(world.ask(&mut served, "run it daily").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();
        assert_eq!(record.risk.tier, 3);
        let parked = rows_of(&world, &served, "card_update");
        assert_eq!(
            parked
                .iter()
                .map(|row| (row.outcome, row.approval.as_deref()))
                .collect::<Vec<_>>(),
            [(AuditOutcome::Pending, Some(record.id.as_str()))]
        );

        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided).await,
            Outcome::Decided
        ));
        let card = std::fs::read_to_string(world.dir(SESSION).join("card.md")).expect("card");
        assert!(card.contains("@daily"), "{card}");
        let rows = rows_of(&world, &served, "card_update");
        assert_eq!(
            rows.iter()
                .map(|row| (row.id, row.outcome, row.approval.as_deref()))
                .collect::<Vec<_>>(),
            [(parked[0].id, AuditOutcome::Ok, Some(record.id.as_str()))]
        );
    }

    /// R85: a request its room may not carry — Eve was invited, and the
    /// label reaches only tgorka and Marta — is not sent into the room; it
    /// goes to each approver's proxy DM this host has a door to (tgorka's;
    /// Marta's proxy is not run here), and the room's status says only
    /// R64's fixed sentence, never what waits.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_the_room_may_not_carry_goes_to_the_approvers_dms() {
        use keeper_agent::sinks::NARROWED_STATUS;
        let (mut world, approvals) = deciding(vec![calls(&[write_note("w1", "after")])]);
        nixis_dm(&world);
        let delegations = Delegations::over(known_with_proxy());
        let doors = Arc::new(Doors::default());
        let mut served = world.delegating(&delegations);
        served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        served.doors = Some(doors.clone());
        world
            .room
            .set_members(&[TGORKA, MARTA, NIXI, "@eve:example.org"]);
        let report = report(world.ask(&mut served, "write a note").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();

        assert!(world.sent_of(APPROVAL_REQUEST).is_empty());
        let requests = doors.of(APPROVAL_REQUEST);
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(requests[0]["id"], record.id.as_str());
        assert_eq!(
            requests[0]["binding_digest"],
            record.binding_digest.as_str()
        );
        let status = world.sent_of(STATUS).last().cloned().expect("a status");
        assert_eq!(status["title"], NARROWED_STATUS, "{status}");
        assert!(!status.to_string().contains(NOTE), "{status}");
    }

    /// 93.4 AC4, the hand-off, with a decision source: the twin of
    /// `a_delegated_schedule_is_refused_before_epic_93`. A card with a
    /// schedule needs a person (T3), so the `delegate` call parks on a
    /// record once its own sinks passed — no room is made, nothing is sent
    /// to Tola, and the call has neither a result nor a refused line.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_delegated_schedule_parks_with_a_source() {
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "delegate"],
            vec![
                delegate_call(
                    "d1",
                    json!({"agent": "tola", "brief": "Every morning.", "card": {"title": "Digest", "schedule": "@daily"}}),
                ),
                prose("It needs you."),
            ],
        );
        world.deps.decisions = Some(Admit::pinned());
        let rooms = Delegations::over(known(&[TGORKA, MARTA]));
        let mut nixi = world.delegating(&rooms);
        let report = report(world.ask(&mut nixi, "a daily digest from Tola").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        assert!(nixi.waiting());
        assert!(rooms.made().is_empty());
        assert!(rooms.sent().is_empty());
        let lines = world.lines(SESSION);
        assert!(tool_results(&lines).is_empty());
        assert!(delegate_lines(&lines).is_empty());
        let record = world.record();
        assert_eq!(
            (record.action.tool.as_str(), record.risk.tier),
            ("delegate", 3)
        );
        assert_eq!(world.sent_of(APPROVAL_REQUEST).len(), 1);
    }

    /// 93.4 AC4, inside the child, with a decision source: the twin of
    /// `an_action_needing_a_person_in_a_delegated_session_is_refused`.
    /// Tola's write outside her session is T2 raised to T3 `[delegated]`;
    /// it parks in her session — its record there, its request into her
    /// room — and the file is never written while it waits.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_action_needing_a_person_in_a_delegated_session_parks_with_a_source() {
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "delegate"],
            vec![
                hand_inbox(),
                prose("Handed on."),
                calls(&[(
                    "w1",
                    "drive_write",
                    json!({"profile": "tgdrive", "path": "notes/sorted.md", "content": "after approval"}),
                )]),
                prose("That needs tgorka."),
            ],
        );
        let mut tola = tolas(&world, &["drive_read", "drive_write"]);
        tola.decisions = Some(Admit::pinned());
        let rooms = Delegations::over(known(&[TGORKA, MARTA]));
        let (_nixi, child, brief) = handed_over(&mut world, &rooms).await;
        let path = world.create_child(&tola, &child, &read_brief(&brief).expect("a brief"));
        let mut tolas_session = world.child(&tola, &path, &rooms);
        let arrived = world.brief(&brief);
        let room = Arc::new(Room::default());
        let report = report(serve_as(&tola, &mut tolas_session, &room, arrived).await);
        assert_eq!(report.ending, TurnEnding::Parked);
        assert!(tolas_session.waiting());
        assert!(!world.tgdrive.join("notes/sorted.md").exists());
        assert!(tool_results(&world.lines(&path)).is_empty());
        let record = world.record_in(&path);
        assert_eq!(
            (
                record.risk.tier,
                record.risk.base_tier,
                record.risk.raised_by.clone()
            ),
            (3, 2, vec!["delegated".to_owned()])
        );
        let requests = room
            .sent()
            .into_iter()
            .filter(|(kind, _)| kind == APPROVAL_REQUEST)
            .count();
        assert_eq!(requests, 1);
    }

    /// 93.2 AC2/AC5: a host that does not hold the claim takes no decision
    /// and never consumes; the host that takes over, given the decision,
    /// reads the room first and finds another copy's `consumed` — though no
    /// chunk it has says so — and runs nothing, reporting the effect unknown.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_consume_that_never_pushed_is_seen_by_the_host_that_takes_over() {
        let (mut world, approvals, mut served, record) =
            parked("hers", vec![prose("I will check.")]).await;
        approvals.elsewhere.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided.clone()).await,
            Outcome::Ignored(NOT_HOLDER)
        ));
        assert!(approvals.events().is_empty(), "no claim, no consume");
        assert!(served.waiting());
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));

        // Hesperia consumed it, ran it and died before pushing its chunk.
        approvals.consumed_by(&record.id, "hesperia");
        std::fs::write(world.tgdrive.join(NOTE), "hers").expect("its effect");
        approvals.elsewhere.store(false, Ordering::SeqCst);
        let mut taker = open(&world, &approvals);
        world.serve(&mut taker, decided).await;
        assert_eq!(approvals.events().len(), 1, "nothing consumed twice");
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Refused);
        assert!(results[0].content.contains(&effect_unknown("hesperia")));
        let consumed = world.approval_lines().pop().expect("a line");
        assert_eq!(
            (
                consumed.state,
                consumed.result.as_deref(),
                consumed.by.as_deref()
            ),
            (ApprovalState::Consumed, Some("unknown"), Some("hesperia"))
        );
        assert!(!taker.waiting());
    }

    /// 93.2 AC3: a `consumed` the server did not accept runs nothing and the
    /// run stays parked; after a restart the decision beside the record
    /// runs it, once. A crash after the local `consumed` line is never a
    /// second run: the effect is reported unknown.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_crash_before_the_consumed_event_is_accepted_runs_it_once_after_restart() {
        let (mut world, approvals, mut served, record) =
            parked("after", vec![prose("Written.")]).await;
        approvals.down.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(served.waiting());
        approvals.down.store(false, Ordering::SeqCst);
        drop(served);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(world.note().as_deref(), Some("after"));
        assert_eq!(approvals.events().len(), 1);
        assert_eq!(results(&world).len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_crash_after_the_consumed_event_never_runs_it_again() {
        let (world, approvals, mut served, record) =
            parked("never", vec![prose("I do not know if it was written.")]).await;
        // The host consumed and logged it, then died before the effect.
        let ServedSession {
            context, writer, ..
        } = &mut served;
        let call_line = context.parked[&record.id].call_line;
        writer
            .write(
                context,
                Some(call_line),
                Some(OwnedEventId::try_from("$mine:example.org").expect("id")),
                LineBody::Approval(ApprovalBody {
                    id: record.id.clone(),
                    state: ApprovalState::Consumed,
                    decision: None,
                    by: Some("electra".to_owned()),
                    result: None,
                    reason: None,
                    scope: None,
                }),
            )
            .expect("line");
        writer.sync().expect("sync");
        drop(served);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(approvals.events().is_empty());
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains(&effect_unknown("electra")));
    }

    /// 93.2 AC4: a pinned file that changed, `max_staleness_s` passed, a
    /// sink the label now blocks, and an expired record each refuse, log
    /// what drifted and run nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn drift_refuses_and_says_what_moved() {
        let refused = |world: &World, what: &str| {
            let results = results(world);
            assert_eq!(results.len(), 1, "{what}");
            assert_eq!(results[0].outcome, ToolOutcomeWord::Refused, "{what}");
            results[0].content.clone()
        };

        // The file it would write changed meanwhile.
        let (mut world, _, mut served, record) = parked("mine", vec![prose("Moved.")]).await;
        std::fs::write(world.tgdrive.join(NOTE), "someone else's").expect("write");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some("someone else's"));
        let line = world.approval_lines().pop().expect("a line");
        assert_eq!(line.state, ApprovalState::Refused);
        assert!(line
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("tgdrive/10-notes/a.md")));
        assert!(refused(&world, "file").contains("tgdrive/10-notes/a.md changed"));

        // What it relied on is older than it may be.
        let (mut world, _, mut served, mut record) = parked("mine", vec![prose("Stale.")]).await;
        record.preconditions.max_staleness_s = Some(0);
        record.created_at = "2026-01-01T00:00:00.000Z".to_owned();
        record.binding_digest = record
            .recomputed_digest(&record.action.args)
            .expect("digest");
        world.rewrite(&record);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(refused(&world, "stale").contains("passed since what it relied on was read"));

        // The label no longer lets the write reach its drive.
        let (mut world, _, mut served, record) = parked("mine", vec![prose("Blocked.")]).await;
        narrow(&mut served, &[TGORKA]);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        let line = world.approval_lines().pop().expect("a line");
        assert_eq!(line.state, ApprovalState::Refused);
        refused(&world, "sink");

        // Expired: the decision does not count, and the record expires.
        let (mut world, approvals, mut served, mut record) =
            parked("mine", vec![prose("Too late.")]).await;
        record.expires_at = "2026-01-01T00:00:00.000Z".to_owned();
        world.rewrite(&record);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        let ignored = world.approval_lines().pop().expect("a line");
        assert_eq!(
            (ignored.state, ignored.decision.as_deref()),
            (ApprovalState::Decided, None)
        );
        assert!(ignored
            .reason
            .as_deref()
            .is_some_and(|r| r.contains("expired")));
        drop(served);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert_eq!(
            world.approval_lines().pop().expect("a line").state,
            ApprovalState::Expired
        );
        assert!(refused(&world, "expired").contains(EXPIRED));
        assert!(approvals.events().is_empty());
    }

    /// A person's deny: the call is refused, their note is a `peer` line
    /// the model reads after it, and nothing ran.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_deny_refuses_and_passes_the_note() {
        let (mut world, approvals, mut served, record) =
            parked("mine", vec![prose("Understood.")]).await;
        let mut denied = world.decision(&record, Decision::Deny);
        denied.content["note"] = json!("Put it in 10-notes instead.");
        world.serve(&mut served, denied).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(approvals.events().is_empty());
        let results = results(&world);
        assert!(results[0].content.contains(DENIED));
        let lines = world.lines(SESSION);
        let peers = kinds(&lines, LineKind::Peer);
        assert_eq!(peers.len(), 1);
        let asked = world.stub.requests().last().expect("a request").to_string();
        assert!(
            asked.find(DENIED) < asked.find("Put it in 10-notes instead."),
            "{asked}"
        );
    }

    /// 93.2 AC6: of the calls in a round the first runs, the second parks,
    /// the later ones wait unrun; after approval the second's result and
    /// then the third's reach the model in order, nothing run twice.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_round_with_several_calls_resumes_where_it_parked() {
        let (mut world, approvals) = deciding(vec![
            calls(&[
                (
                    "r1",
                    "drive_read",
                    json!({"profile": "tgdrive", "path": "notes/hello.md"}),
                ),
                write_note("w2", "second"),
                (
                    "r3",
                    "drive_read",
                    json!({"profile": "tgdrive", "path": "notes/secret-plan.md"}),
                ),
                (
                    "r4",
                    "drive_read",
                    json!({"profile": "tgdrive", "path": "00-inbox/x.md"}),
                ),
            ]),
            prose("All three."),
        ]);
        let mut served = open(&world, &approvals);
        let report = report(world.ask(&mut served, "do three things").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let ids = |world: &World| -> Vec<String> {
            results(world).iter().map(|r| r.call_id.clone()).collect()
        };
        assert_eq!(ids(&world), ["r1"]);
        assert_eq!(kinds(&world.lines(SESSION), LineKind::ToolCall).len(), 4);
        let record = world.record();
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(ids(&world), ["r1", "w2", "r3", "r4"]);
        assert_eq!(world.note().as_deref(), Some("second"));
        let asked = world.stub.requests().last().expect("a request").to_string();
        let at = |id: &str| asked.find(&format!("\"tool_call_id\":\"{id}\"")).expect(id);
        assert!(at("w2") < at("r3") && at("r3") < at("r4"), "{asked}");
        assert!(asked.contains("the plan"), "the third call ran");
        assert_eq!(kinds(&world.lines(SESSION), LineKind::ToolCall).len(), 4);
    }

    /// R74 in the person's own conversation: their new message denies what
    /// waits, superseded, and then runs as a turn over a whole transcript.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_new_message_from_the_person_supersedes_what_waits() {
        let (mut world, approvals, mut served, _) =
            parked("mine", vec![prose("Not writing it, then.")]).await;
        let report = report(world.ask(&mut served, "never mind").await);
        assert_eq!(report.ending, TurnEnding::Complete);
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(approvals.events().is_empty());
        let line = world.approval_lines().pop().expect("a line");
        assert_eq!(
            (line.state, line.decision.as_deref(), line.reason.as_deref()),
            (ApprovalState::Decided, Some("deny"), Some(SUPERSEDED))
        );
        assert!(results(&world)[0].content.contains(SUPERSEDED_RESULT));
        assert!(!served.waiting());
        let asked = world.stub.requests().last().expect("a request").to_string();
        assert!(asked.contains(SUPERSEDED_RESULT), "{asked}");
    }

    /// R74 for what is not the person's message: held, not served, while a
    /// call waits, said once in the status; served once it ends.
    #[tokio::test(flavor = "multi_thread")]
    async fn what_arrives_while_a_call_waits_is_held() {
        let (mut world, _, mut served, record) = parked("mine", vec![prose("Denied.")]).await;
        let statuses = world.sent_of(keeper_core::agents::events::STATUS).len();
        let scope = world.scope(TGORKA, &["tgdrive"], None);
        assert!(matches!(
            world.serve(&mut served, scope).await,
            Outcome::Held
        ));
        let again = world.scope(TGORKA, &["tgdrive"], None);
        assert!(matches!(
            world.serve(&mut served, again).await,
            Outcome::Held
        ));
        assert_eq!(
            world.sent_of(keeper_core::agents::events::STATUS).len(),
            statuses + 1,
            "said once"
        );
        assert!(kinds(&world.lines(SESSION), LineKind::Scope).is_empty());
        let denied = world.decision(&record, Decision::Deny);
        world.serve(&mut served, denied).await;
        assert!(!served.waiting());
    }

    /// R74/C6: after a restart a parked turn is waiting, not cut off: no
    /// `error` line, nothing said in the room, still parked.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_restart_keeps_a_parked_run_parked() {
        let (world, approvals, served, _) = parked("mine", vec![]).await;
        drop(served);
        let sent = world.room.sent().len();
        let mut restarted = open(&world, &approvals);
        assert!(!restarted
            .recover(&world.deps, world.room.clone(), &Trail::default())
            .await
            .expect("recover"));
        assert!(restarted.waiting());
        assert!(kinds(&world.lines(SESSION), LineKind::Error).is_empty());
        assert_eq!(world.room.sent().len(), sent);
    }

    /// R84: nothing decided by `expires_at` — the worker's own timer
    /// expires it, with no arrival, and the turn goes on refused.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approval_nobody_decides_expires_on_the_workers_timer() {
        let (world, approvals, served, mut record) =
            parked("mine", vec![prose("It expired.")]).await;
        drop(served);
        record.expires_at = keeper_core::agents::approval::stamp(
            chrono::Utc::now() + chrono::Duration::milliseconds(300),
        );
        world.rewrite(&record);
        let mut restarted = open(&world, &approvals);
        let (_queue, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
        let busy = keeper_agent::agent::Activity::default();
        let (_handle, signal) = chat::cancellation();
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            restarted.serve_arrivals(
                &world.deps,
                world.room.clone(),
                Vec::new(),
                &mut arrivals,
                signal,
                &busy,
            ),
        )
        .await;
        assert_eq!(
            world.approval_lines().pop().expect("a line").state,
            ApprovalState::Expired
        );
        assert!(results(&world)[0].content.contains(EXPIRED));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// 93.2 AC9: two hosts consume one approval at once — the one whose
    /// claim was just taken and its taker. Only the host whose `consumed`
    /// is first in the room's order runs it; the other logs it spent.
    #[tokio::test(flavor = "multi_thread")]
    async fn two_hosts_consuming_at_once_run_it_once() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Done."), prose("Spent.")]).await;
        // The decision is written beside the record by the holder, whose
        // `consumed` the server did not take; nobody consumed yet.
        approvals.down.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert!(approvals.events().is_empty());
        approvals.down.store(false, Ordering::SeqCst);
        drop(served);
        let mut deps_b = tolas_free_deps(&world);
        deps_b.host = keeper_core::agents::log::HostSlug::new("hesperia").expect("slug");
        let mut a = open(&world, &approvals);
        let mut b = world.open_as(&deps_b, SESSION);
        b.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        let (_ha, sa) = chat::cancellation();
        let (_hb, sb) = chat::cancellation();
        tokio::join!(
            a.resume_approvals(&world.deps, world.room.clone(), sa),
            b.resume_approvals(&deps_b, world.room.clone(), sb),
        );
        let events = approvals.events();
        assert_eq!(events.len(), 2, "both sent");
        assert_eq!(world.note().as_deref(), Some("once"));
        let lines = world.lines(SESSION);
        let ran = tool_results(&lines)
            .iter()
            .filter(|r| r.outcome == ToolOutcomeWord::Ok)
            .count();
        assert_eq!(ran, 1, "run once");
        let spent = world
            .approval_lines()
            .into_iter()
            .filter(|line| line.state == ApprovalState::Refused)
            .count();
        assert_eq!(spent, 1);
        let _ = NOT_RUN;
    }

    /// R84 / 93.2 AC1 for a scheduled run with a source: the action parks
    /// (T3: unattended), the card reads `run: blocked`, and the host's next
    /// arrival for the session is held while the approval waits.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_scheduled_run_parks_and_holds_its_next_window() {
        use keeper_agent::agent::scheduled_arrival;
        use keeper_agent::cards::Scheduled;
        use keeper_core::agents::card::{CardAgent, Field, Run};
        const SCHEDULED: &str = "active/2026-10-05-sort";
        let (world, approvals) = deciding(vec![calls(&[write_note("w1", "sorted")])]);
        session_of(
            &world.tgdrive,
            SCHEDULED,
            &decl("tgdrive", &[TGORKA, MARTA], false),
            "nixi",
            SessionKind::Scheduled,
            "!sort:example.org",
        );
        let card = "---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\nassignee: nixi\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nWrite a note about what came in.\n";
        write(
            &world.tgdrive,
            &format!("60-sessions/{SCHEDULED}/card.md"),
            card,
        );
        let mut served = world.open(SCHEDULED);
        served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        let arrival = |window: &str, now: &str| {
            scheduled_arrival(
                &user("@nixi:example.org"),
                &Scheduled::Run {
                    card: "card.md".to_owned(),
                    window: window.to_owned(),
                    now_ms: chrono::DateTime::parse_from_rfc3339(now)
                        .expect("an instant")
                        .timestamp_millis(),
                    utc_offset_minutes: 0,
                },
            )
            .expect("an arrival")
        };
        let report = report(
            world
                .serve(
                    &mut served,
                    arrival("2026-10-05T09:00:00.000Z", "2026-10-05T09:30:00Z"),
                )
                .await,
        );
        assert_eq!(report.ending, TurnEnding::Parked);
        assert!(served.waiting());
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        let text = std::fs::read_to_string(world.dir(SCHEDULED).join("card.md")).expect("card");
        let keys = CardAgent::of_text(&text).expect("keys");
        assert_eq!(keys.run, Some(Field::Read(Run::Blocked)), "{text}");
        let record = world.record_in(SCHEDULED);
        assert_eq!(
            (record.risk.tier, record.risk.raised_by.clone()),
            (3, vec!["unattended".to_owned()])
        );
        assert!(matches!(
            world
                .serve(
                    &mut served,
                    arrival("2026-10-05T10:00:00.000Z", "2026-10-05T10:30:00Z")
                )
                .await,
            Outcome::Held
        ));
    }

    /// The one `decided` line written for a decision that did not count:
    /// no decision, its reason.
    fn ignored(world: &World) -> ApprovalBody {
        let last = world.approval_lines().pop().expect("a line");
        assert_eq!(last.state, ApprovalState::Decided, "{last:?}");
        assert_eq!(last.decision, None, "{last:?}");
        last
    }

    fn decision_file(world: &World, record: &ApprovalRecord) -> bool {
        world
            .approvals()
            .join(format!("{}.decision.json", record.id))
            .exists()
    }

    /// 93.3 AC2: a host without the claim writes nothing — no file, no
    /// line; a decision with another digest or a scope there is no such
    /// thing as (`always`) writes no file and is logged; the first valid
    /// decision is written once, and a second valid one after it writes
    /// nothing more and is logged.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_decision_file_is_written_once_by_the_claim_holder() {
        use keeper_core::agents::approval::DecisionRefusal;
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;

        approvals.elsewhere.store(true, Ordering::SeqCst);
        let elsewhere = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, elsewhere).await,
            Outcome::Ignored(NOT_HOLDER)
        ));
        assert!(!decision_file(&world, &record));
        assert_eq!(world.approval_lines().len(), 1, "only the request's line");
        approvals.elsewhere.store(false, Ordering::SeqCst);

        let mut other = decision_content(&record, Decision::Approve, None);
        other["binding_digest"] = json!(sha256_hex(b"something else"));
        let other = world.decision_from(TGORKA, "PHONE", other);
        assert!(matches!(
            world.serve(&mut served, other).await,
            Outcome::Decided
        ));
        assert_eq!(
            ignored(&world).reason,
            Some(DecisionRefusal::Digest.to_string())
        );
        let mut always = decision_content(&record, Decision::Approve, None);
        always["scope"] = json!("always");
        let always = world.decision_from(TGORKA, "PHONE", always);
        world.serve(&mut served, always).await;
        assert_eq!(ignored(&world).reason.as_deref(), Some(UNREADABLE_DECISION));
        assert!(!decision_file(&world, &record));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));

        // The first valid decision is written; its consume fails, so the
        // run stays parked and a second valid decision arrives.
        approvals.down.store(true, Ordering::SeqCst);
        let first = world.decision(&record, Decision::Approve);
        world.serve(&mut served, first).await;
        assert!(decision_file(&world, &record));
        let written = std::fs::read(
            world
                .approvals()
                .join(format!("{}.decision.json", record.id)),
        )
        .expect("decision");
        assert!(served.waiting());
        let second = world.decision(&record, Decision::Deny);
        assert!(matches!(
            world.serve(&mut served, second).await,
            Outcome::Duplicate
        ));
        assert_eq!(ignored(&world).reason.as_deref(), Some(ALREADY_DECIDED));
        assert_eq!(
            std::fs::read(
                world
                    .approvals()
                    .join(format!("{}.decision.json", record.id))
            )
            .expect("decision"),
            written,
            "the first decision stands"
        );
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// R183: the claim moves while the decision's trust is being looked
    /// up. The holder's read before the lookup said yes; the one after it
    /// says no, so no decision file, no line and no consume.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_claim_lost_during_the_trust_lookup_writes_no_decision() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        let admit = Admit::pinned();
        world.deps.decisions = Some(admit.clone());
        let taken = Arc::clone(&approvals);
        *admit.during.lock().expect("lock") = Some(Box::new(move || {
            taken.elsewhere.store(true, Ordering::SeqCst);
        }));
        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided).await,
            Outcome::Ignored(NOT_HOLDER)
        ));
        assert!(admit.during.lock().expect("lock").is_none(), "it was asked");
        assert!(!decision_file(&world, &record));
        assert_eq!(world.approval_lines().len(), 1, "only the request's line");
        assert!(approvals.events().is_empty());
        assert!(served.waiting());
    }

    /// Every write of the process log, kept.
    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Captured {
            self.clone()
        }
    }

    /// R182, R3-05: keys the sender's homeserver could not give whole are
    /// "unknown" — the decision is logged ignored with that reason, and
    /// the server's error is not repeated; a decision whose scope is text a
    /// sender chose is unreadable, and that text reaches neither the
    /// session's log nor the process log.
    #[tokio::test(flavor = "multi_thread")]
    async fn unknown_keys_and_unreadable_decisions_are_logged_without_what_they_quote() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        let admit = Admit::pinned();
        world.deps.decisions = Some(admit.clone());
        let captured = Captured::default();
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(captured.clone())
                .with_ansi(false)
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        );

        admit.unknown.store(true, Ordering::SeqCst);
        let unknown = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, unknown).await,
            Outcome::Decided
        ));
        assert_eq!(ignored(&world).reason.as_deref(), Some(KEYS_UNKNOWN));
        admit.unknown.store(false, Ordering::SeqCst);

        let mut scoped = decision_content(&record, Decision::Approve, None);
        scoped["scope"] = json!(SECRET);
        let scoped = world.decision_from(TGORKA, "PHONE", scoped);
        world.serve(&mut served, scoped).await;
        assert_eq!(ignored(&world).reason.as_deref(), Some(UNREADABLE_DECISION));

        let process = String::from_utf8(captured.0.lock().expect("lock").clone()).expect("utf-8");
        assert!(process.contains("a decision is ignored"), "{process}");
        assert!(!process.contains(SECRET), "{process}");
        assert!(!format!("{:?}", world.lines(SESSION)).contains(SECRET));
        assert!(!decision_file(&world, &record));
        assert!(approvals.events().is_empty());
        assert!(served.waiting());
    }

    /// R3-06: what is at the decision's name but no decision — a partial
    /// file — and a folder that takes no file are faults, not a decision
    /// taken already: no line, the event stays unseen, and its redelivery
    /// is taken once the fault clears.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_decision_that_could_not_be_stored_is_taken_once_the_fault_clears() {
        use std::os::unix::fs::PermissionsExt;
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        let name = world
            .approvals()
            .join(format!("{}.decision.json", record.id));
        let first = world.decision(&record, Decision::Approve);

        std::fs::write(&name, "{\"v\":").expect("a partial file");
        assert!(matches!(
            world.serve(&mut served, first.clone()).await,
            Outcome::Ignored(DECISION_UNWRITTEN)
        ));
        assert_eq!(world.approval_lines().len(), 1, "only the request's line");
        std::fs::remove_file(&name).expect("cleared");

        let mode = |mode| {
            std::fs::set_permissions(world.approvals(), std::fs::Permissions::from_mode(mode))
                .expect("mode");
        };
        mode(0o555);
        let unwritable = world.serve(&mut served, first.clone()).await;
        mode(0o755);
        assert!(matches!(unwritable, Outcome::Ignored(DECISION_UNWRITTEN)));
        assert!(!name.exists());
        assert_eq!(world.approval_lines().len(), 1, "only the request's line");
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));

        assert!(matches!(
            world.serve(&mut served, first).await,
            Outcome::Decided
        ));
        assert!(parse_decision_file(&world, &record).decided_by.verified);
        assert_eq!(world.note().as_deref(), Some("once"));
        assert_eq!(approvals.events().len(), 1);
    }

    /// R3-07 (R80): after an approval ran, or was denied, a later decision
    /// on it — another event — is logged ignored and changes nothing; that
    /// event's redelivery writes nothing more.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_later_decision_on_an_ended_approval_is_logged_and_changes_nothing() {
        for (first, then) in [
            (Decision::Approve, Decision::Deny),
            (Decision::Deny, Decision::Approve),
        ] {
            let (mut world, approvals, mut served, record) =
                parked("once", vec![prose("Written.")]).await;
            let decided = world.decision(&record, first);
            assert!(matches!(
                world.serve(&mut served, decided).await,
                Outcome::Decided
            ));
            assert!(!served.waiting());
            let lines = world.approval_lines().len();
            let later = world.decision(&record, then);
            assert!(matches!(
                world.serve(&mut served, later.clone()).await,
                Outcome::Duplicate
            ));
            let line = ignored(&world);
            assert_eq!(line.reason.as_deref(), Some(APPROVAL_ENDED));
            assert_eq!(line.id, record.id.to_string());
            assert_eq!(world.approval_lines().len(), lines + 1);
            world.serve(&mut served, later).await;
            assert_eq!(world.approval_lines().len(), lines + 1, "seen once");
            let ran = first == Decision::Approve;
            assert_eq!(
                world.note().as_deref(),
                Some(if ran { "once" } else { ORIGINAL })
            );
            assert_eq!(approvals.events().len(), usize::from(ran));
            assert_eq!(parse_decision_file(&world, &record).decision, first);
        }
    }

    /// 93.3 AC2: a `session` scope on a T3 record is not admitted, and is
    /// logged.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_session_scope_at_t3_writes_nothing() {
        use keeper_core::agents::approval::DecisionRefusal;
        let mut world = world(
            ProviderKind::OpenAi,
            &["drive_read", "card_update"],
            vec![calls(&[(
                "c1",
                "card_update",
                json!({"card": "card.md", "fields": {"schedule": "@daily"}}),
            )])],
        );
        world.deps.decisions = Some(Admit::pinned());
        let approvals = Arc::new(Approvals::default());
        write(&world.dir(SESSION), "card.md", CARD);
        let mut served = open(&world, &approvals);
        report(world.ask(&mut served, "run it daily").await);
        let record = world.record();
        assert_eq!(record.risk.tier, 3);
        let mut session = decision_content(&record, Decision::Approve, None);
        session["scope"] = json!("session");
        let session = world.decision_from(TGORKA, "PHONE", session);
        world.serve(&mut served, session).await;
        assert_eq!(
            ignored(&world).reason,
            Some(DecisionRefusal::Scope("session").to_string())
        );
        assert!(!decision_file(&world, &record));
        assert!(served.waiting());
    }

    /// 93.3 AC3 over the trust adapter's double: tgorka's decision from a
    /// fresh login nobody signed is logged ignored with why, and so is one
    /// after his master key moved; neither runs anything. One in clear is
    /// not even a decision. From his cross-signed phone, with his pinned
    /// key published, it counts and the run resumes, once.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_decision_from_a_cross_signed_device_counts_and_one_from_a_new_device_does_not() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        let admit = Admit::pinned();
        world.deps.decisions = Some(admit.clone());

        let fresh = world.decision_from(
            TGORKA,
            NEW,
            decision_content(&record, Decision::Approve, None),
        );
        assert!(matches!(
            world.serve(&mut served, fresh).await,
            Outcome::Decided
        ));
        let line = ignored(&world);
        assert_eq!(line.reason.as_deref(), Some(UNSIGNED_DEVICE));
        assert_eq!(line.by.as_deref(), Some(TGORKA));

        admit.publish(TGORKA, RESET);
        let moved = world.decision(&record, Decision::Approve);
        world.serve(&mut served, moved).await;
        assert_eq!(ignored(&world).reason.as_deref(), Some(KEY_MOVED));

        let mut clear = world.decision(&record, Decision::Approve);
        clear.arrival = Arrival::Decision { sealed: false };
        assert!(matches!(
            world.serve(&mut served, clear).await,
            Outcome::Ignored(keeper_agent::rooms::UNTRUSTED_DECISION)
        ));
        assert!(!decision_file(&world, &record));
        assert!(approvals.events().is_empty());
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(served.waiting());

        admit.publish(TGORKA, KEY);
        // A reader now who did not read the label the action parked under
        // is not one of its approvers.
        let mut narrowed = record.clone();
        narrowed.label.readers = Readers::Only(BTreeSet::from([user(MARTA)]));
        world.rewrite(&narrowed);
        let before = world.decision(&record, Decision::Approve);
        world.serve(&mut served, before).await;
        assert_eq!(ignored(&world).reason.as_deref(), Some(NOT_AN_APPROVER));
        world.rewrite(&record);

        let phone = world.decision(&record, Decision::Approve);
        world.serve(&mut served, phone).await;
        assert_eq!(world.note().as_deref(), Some("once"));
        assert_eq!(approvals.events().len(), 1);
        let decided = parse_decision_file(&world, &record);
        assert_eq!(
            (
                decided.decided_by.user.as_str(),
                decided.decided_by.device.as_str(),
                decided.decided_by.verified
            ),
            (TGORKA, "PHONE", true)
        );
        assert!(!served.waiting());
    }

    fn parse_decision_file(
        world: &World,
        record: &ApprovalRecord,
    ) -> keeper_core::agents::approval::DecisionRecord {
        keeper_core::agents::approval::parse_decision(
            &std::fs::read_to_string(
                world
                    .approvals()
                    .join(format!("{}.decision.json", record.id)),
            )
            .expect("decision"),
        )
        .expect("a strict decision")
    }

    /// 93.3 AC9 with the desktop's anchor (R87): on a T4 record the
    /// requester's approval from this app's own device writes nothing and
    /// logs "decide on another device"; from his cross-signed phone it
    /// counts.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_t4_decision_needs_the_requester_on_another_device() {
        use keeper_core::agents::approval::Scope;
        let (mut world, approvals, mut served, mut record) =
            parked("asked", vec![prose("Written.")]).await;
        world.deps.decisions = Some(Admit::desktop());
        // Neither the tier nor the chain is bound: the record is made T4,
        // asked by tgorka, as an irreversible action's would be.
        record.risk.tier = 4;
        record.scopes = vec![Scope::Once];
        record.dispatch_chain = vec![TGORKA.to_owned()];
        world.rewrite(&record);

        let mac = world.decision_from(
            TGORKA,
            "MAC",
            decision_content(&record, Decision::Approve, None),
        );
        world.serve(&mut served, mac).await;
        assert_eq!(ignored(&world).reason.as_deref(), Some(THIS_DEVICE));
        assert!(!decision_file(&world, &record));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));

        let phone = world.decision(&record, Decision::Approve);
        world.serve(&mut served, phone).await;
        assert!(decision_file(&world, &record));
        assert_eq!(world.note().as_deref(), Some("asked"));
        assert_eq!(approvals.events().len(), 1);
    }

    /// R89: a request its room may not carry went to tgorka's proxy DM;
    /// his decision there reaches the DM's worker, which hands it — and
    /// only a decision on that request — to the session that asked. That
    /// session alone writes the decision and runs the call.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_decision_in_the_approvers_dm_goes_home_to_the_session_that_asked() {
        let (mut world, approvals) =
            deciding(vec![calls(&[write_note("w1", "after")]), prose("Written.")]);
        nixis_dm(&world);
        let delegations = Delegations::over(known_with_proxy());
        let doors = Arc::new(Doors::default());
        let mut served = world.delegating(&delegations);
        served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        served.doors = Some(doors.clone());
        let (home, mut inbox) = tokio::sync::mpsc::unbounded_channel::<Arrived>();
        served.inbox = Some(Arc::new(move |arrived| {
            let _ = home.send(arrived);
        }));
        world
            .room
            .set_members(&[TGORKA, MARTA, NIXI, "@eve:example.org"]);
        report(world.ask(&mut served, "write a note").await);
        let record = world.record();
        assert_eq!(doors.of(APPROVAL_REQUEST).len(), 1);

        let mut dm = world.open(DM);
        dm.doors = Some(doors.clone());
        let mut elsewhere = decision_content(&record, Decision::Approve, None);
        elsewhere["id"] = json!(ulid::Ulid::new().to_string());
        let elsewhere = world.decision_from(TGORKA, "PHONE", elsewhere);
        assert!(!matches!(
            world.serve(&mut dm, elsewhere).await,
            Outcome::Forwarded
        ));
        assert!(inbox.try_recv().is_err(), "nothing else goes home");

        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut dm, decided.clone()).await,
            Outcome::Forwarded
        ));
        assert!(!decision_file(&world, &record), "the DM writes nothing");
        let forwarded = inbox.try_recv().expect("the decision went home");
        assert_eq!(
            forwarded.via.as_ref().map(|room| room.as_str()),
            Some("!dm:example.org")
        );
        // R184: the same event again is not handed home a second time, and
        // a decision that was forwarded is never forwarded on.
        assert!(matches!(
            world.serve(&mut dm, decided).await,
            Outcome::Forwarded
        ));
        assert!(!matches!(
            world.serve(&mut dm, forwarded.clone()).await,
            Outcome::Forwarded
        ));
        assert!(inbox.try_recv().is_err(), "handed home once");
        assert!(matches!(
            world.serve(&mut served, forwarded).await,
            Outcome::Decided
        ));
        assert!(decision_file(&world, &record));
        assert_eq!(world.note().as_deref(), Some("after"));
    }

    /// R184: a proxy's `main` session is its person's DM, so a route from
    /// that DM to that session's own inbox would hand a decision back to
    /// the worker it came from, forever. No such route is kept, and a
    /// decision there is the session's own to take.
    #[tokio::test]
    async fn a_dm_never_forwards_a_decision_to_its_own_session() {
        let mut world = world(ProviderKind::OpenAi, &[], vec![]);
        let forwards = keeper_agent::deciding::Forwards::default();
        let room = matrix_sdk::ruma::room_id!("!dm:example.org");
        forwards.expect(
            room,
            "01J0000000000000000000000X",
            room,
            Arc::new(|_| panic!("circled")),
        );
        let mut arrived = world.event(
            TGORKA,
            Arrival::Decision { sealed: true },
            json!({"id": "01J0000000000000000000000X"}),
        );
        assert!(!forwards.forward(room, &arrived));
        arrived.via = Some(room.to_owned());
        assert!(!forwards.forward(room, &arrived));
    }

    /// Nixi's deps again, for a second host of the same agent.
    fn tolas_free_deps(world: &World) -> AgentDeps {
        AgentDeps {
            env: TurnEnv {
                drive: world.deps.env.drive.as_ref().map(|drive| DrivePorts {
                    profiles: Arc::clone(&drive.profiles),
                    vault: None,
                    approval: None,
                }),
                ..TurnEnv::new(Arc::new(DataDir(world.deps.data_dir.clone())))
            },
            data_dir: world.deps.data_dir.clone(),
            row: world.deps.row.clone(),
            bot: world.deps.bot.clone(),
            home: world.deps.home.clone(),
            host: world.deps.host.clone(),
            drives: world.deps.drives.clone(),
            sessions_zone: world.deps.sessions_zone.clone(),
            sessions_subfolder: world.deps.sessions_subfolder.clone(),
            lfs_threshold_bytes: world.deps.lfs_threshold_bytes,
            decisions: world.deps.decisions.clone(),
        }
    }

    /// Every audit row on `data_dir` carrying `approval`, oldest first.
    fn approval_rows(
        data_dir: &Path,
        approval: &str,
    ) -> Vec<keeper_core::bots::audit::AuditOutcome> {
        let mut rows = keeper_core::bots::audit::list_audit(data_dir, None, None).expect("audit");
        rows.reverse();
        rows.into_iter()
            .filter(|row| row.approval.as_deref() == Some(approval))
            .map(|row| row.outcome)
            .collect()
    }

    /// The log as a stop right after the record left it: the chunk cut
    /// after the line the checkpoint names, before `approval requested`.
    fn stopped_after_the_record(world: &World, record: &ApprovalRecord) {
        let chunk = world.dir(SESSION).join(&record.checkpoint.chunk);
        let bytes = std::fs::read(&chunk).expect("chunk");
        let named = format!("\"id\":\"{}\"", record.checkpoint.through);
        let at = String::from_utf8_lossy(&bytes).find(&named).expect("named");
        let end = at + bytes[at..].iter().position(|b| *b == b'\n').expect("end") + 1;
        assert_eq!(sha256_hex(&bytes[..end]), record.checkpoint.sha256);
        std::fs::write(&chunk, &bytes[..end]).expect("cut");
    }

    /// R93P-01 / R174: what runs after approval is the record's exact
    /// bytes — a secret-shaped argument the log redacts is written as the
    /// person saw it — and a log changed under the checkpoint is drift.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approval_runs_the_bytes_it_bound_never_the_logs_copy() {
        const SECRET: &str = "key AKIAIOSFODNN7EXAMPLE here";
        let (mut world, _, mut served, record) = parked(SECRET, vec![prose("Written.")]).await;
        let logged: Vec<String> = kinds(&world.lines(SESSION), LineKind::ToolCall)
            .iter()
            .map(|line| match &line.body {
                LineBody::ToolCall(call) => call.args.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert!(
            !logged[0].contains("AKIAIOSFODNN7EXAMPLE"),
            "the log redacts it: {logged:?}"
        );
        assert_eq!(record.action.args["content"], SECRET);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(SECRET));

        let (mut world, approvals, mut served, record) =
            parked("mine", vec![prose("Changed.")]).await;
        let chunk = world.dir(SESSION).join(&record.checkpoint.chunk);
        let text = std::fs::read_to_string(&chunk).expect("chunk");
        assert!(text.contains("write a note"));
        std::fs::write(&chunk, text.replace("write a note", "WRITE A NOTE")).expect("edit");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(approvals.events().is_empty(), "drift consumes nothing");
        let results = results(&world);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Refused);
        assert!(
            results[0].content.contains("the session's log changed"),
            "{}",
            results[0].content
        );
    }

    /// R93P-02 / R174: the record pins where a write's alias landed; the
    /// alias retargeted to another file of the same bytes is drift, and
    /// neither file is written.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_alias_retargeted_between_equal_files_is_drift() {
        const ALIAS: &str = "10-notes/current.md";
        let (mut world, approvals) = deciding(vec![
            calls(&[(
                "w1",
                "drive_write",
                json!({"profile": "tgdrive", "path": ALIAS, "content": "through the alias"}),
            )]),
            prose("Moved."),
        ]);
        let b = world.tgdrive.join("10-notes/b.md");
        std::fs::write(&b, ORIGINAL).expect("b");
        let link = world.tgdrive.join(ALIAS);
        std::os::unix::fs::symlink("a.md", &link).expect("link");
        let mut served = open(&world, &approvals);
        let report = report(world.ask(&mut served, "write through the alias").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();
        let pin = &record.preconditions.files[0];
        assert_eq!(
            (pin.path.as_str(), pin.landing.as_deref()),
            (ALIAS, Some(NOTE))
        );
        std::fs::remove_file(&link).expect("unlink");
        std::os::unix::fs::symlink("b.md", &link).expect("relink");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert_eq!(std::fs::read_to_string(&b).expect("b"), ORIGINAL);
        assert!(approvals.events().is_empty());
        let results = results(&world);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Refused);
        assert!(
            results[0]
                .content
                .contains("tgdrive/10-notes/current.md changed"),
            "{}",
            results[0].content
        );
    }

    /// R93P-03 / R175: the request's first send is rate-limited and Eve
    /// joins before the retry. The retry asks the room again: the request
    /// never enters the widened room, it goes to the approvers' DMs, and
    /// the status says only the fixed sentence.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_retried_after_the_room_widened_goes_to_the_doors() {
        use keeper_agent::sinks::NARROWED_STATUS;
        let (mut world, approvals) = deciding(vec![calls(&[write_note("w1", "after")])]);
        nixis_dm(&world);
        let delegations = Delegations::over(known_with_proxy());
        let doors = Arc::new(Doors::default());
        let mut served = world.delegating(&delegations);
        served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        served.doors = Some(doors.clone());
        world.room.set_members(&[TGORKA, MARTA, NIXI]);
        *world.room.limited_request.lock().expect("lock") =
            Some(vec![TGORKA, MARTA, NIXI, "@eve:example.org"]);
        let report = report(world.ask(&mut served, "write a note").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        assert!(
            world.room.limited_request.lock().expect("lock").is_none(),
            "the first send was refused"
        );
        let record = world.record();
        assert!(world.sent_of(APPROVAL_REQUEST).is_empty());
        let requests = doors.of(APPROVAL_REQUEST);
        assert_eq!(requests.len(), 1, "{requests:?}");
        assert_eq!(requests[0]["id"], record.id.as_str());
        let status = world.sent_of(STATUS).last().cloned().expect("a status");
        assert_eq!(status["title"], NARROWED_STATUS, "{status}");
        assert!(!status.to_string().contains(NOTE), "{status}");
        assert!(served.waiting());
    }

    /// R93P-04 / R176: a host that does not hold the session's claim
    /// writes no decision — neither a deny nor an approve — and no line.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_host_without_the_claim_writes_no_decision() {
        use keeper_agent::approvals::NOT_HOLDER;
        let (mut world, approvals, mut served, record) =
            parked("mine", vec![prose("Never.")]).await;
        approvals.elsewhere.store(true, Ordering::SeqCst);
        for decision in [Decision::Deny, Decision::Approve] {
            let decided = world.decision(&record, decision);
            assert!(matches!(
                world.serve(&mut served, decided).await,
                Outcome::Ignored(NOT_HOLDER)
            ));
        }
        assert!(!world
            .approvals()
            .join(format!("{}.decision.json", record.id))
            .exists());
        let states: Vec<ApprovalState> = world
            .approval_lines()
            .iter()
            .map(|body| body.state)
            .collect();
        assert_eq!(states, [ApprovalState::Requested]);
        assert!(results(&world).is_empty());
        assert!(served.waiting());
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// R93P-05 / R176: a stop after the record and before its `approval
    /// requested` line loses nothing: after a restart the turn is not cut
    /// off, the request is announced again and the approval runs once; a
    /// record past its time expires instead, its call answered.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_record_whose_requested_line_was_lost_is_announced_again() {
        let (mut world, approvals, served, record) = parked("again", vec![prose("Written.")]).await;
        drop(served);
        stopped_after_the_record(&world, &record);
        assert!(world.approval_lines().is_empty());
        let requests = world.sent_of(APPROVAL_REQUEST).len();
        let mut restarted = open(&world, &approvals);
        assert!(!restarted
            .recover(&world.deps, world.room.clone(), &Trail::default())
            .await
            .expect("recover"));
        assert!(kinds(&world.lines(SESSION), LineKind::Error).is_empty());
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(world.sent_of(APPROVAL_REQUEST).len(), requests + 1);
        let states: Vec<ApprovalState> = world
            .approval_lines()
            .iter()
            .map(|body| body.state)
            .collect();
        assert_eq!(states, [ApprovalState::Requested]);
        assert!(restarted.waiting());
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut restarted, decided).await;
        assert_eq!(world.note().as_deref(), Some("again"));
        assert_eq!(approvals.events().len(), 1);
        assert_eq!(results(&world).len(), 1);

        let (world, approvals, served, mut record) = parked("late", vec![prose("Too late.")]).await;
        drop(served);
        stopped_after_the_record(&world, &record);
        record.expires_at = "2026-01-01T00:00:00.000Z".to_owned();
        world.rewrite(&record);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(
            world.approval_lines().pop().expect("a line").state,
            ApprovalState::Expired
        );
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains(EXPIRED));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(!restarted.waiting());
    }

    /// R93P-06 / R176: a stop after the approved call's result and before
    /// the rest of its round: after a restart the call that may have run
    /// is told so and never run again, the next runs, and the model reads
    /// a result for every call. A stop inside a deny's answers likewise
    /// leaves no call of the round unanswered.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_continuation_a_stop_cut_answers_every_call_of_its_round() {
        use keeper_agent::approvals::INTERRUPTED;
        let round = || {
            calls(&[
                write_note("w1", "first"),
                (
                    "r2",
                    "drive_read",
                    json!({"profile": "tgdrive", "path": "notes/hello.md"}),
                ),
                (
                    "r3",
                    "drive_read",
                    json!({"profile": "tgdrive", "path": "notes/secret-plan.md"}),
                ),
            ])
        };
        let cut = |served: &mut ServedSession, record: &ApprovalRecord, approval: ApprovalBody| {
            let ServedSession {
                context, writer, ..
            } = served;
            let call_line = context.parked[&record.id].call_line;
            writer
                .write(context, Some(call_line), None, LineBody::Approval(approval))
                .expect("line");
            let label = context.label.clone();
            writer
                .write(
                    context,
                    Some(call_line),
                    None,
                    LineBody::ToolResult(ToolResultBody {
                        call_id: "w1".to_owned(),
                        outcome: ToolOutcomeWord::Ok,
                        content: "Wrote it.".to_owned(),
                        truncated: None,
                        label,
                    }),
                )
                .expect("result");
            writer.sync().expect("sync");
        };
        let ids = |world: &World| -> Vec<String> {
            results(world).iter().map(|r| r.call_id.clone()).collect()
        };

        // Consumed here and run; the stop came before r2's result.
        let (mut world, approvals) = deciding(vec![round(), prose("Two of three.")]);
        let mut served = open(&world, &approvals);
        report(world.ask(&mut served, "do three things").await);
        let record = world.record();
        let mut consumed = line_of(&record.id, ApprovalState::Consumed);
        consumed.by = Some("electra".to_owned());
        cut(&mut served, &record, consumed);
        drop(served);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(ids(&world), ["w1", "r2", "r3"]);
        let answered = results(&world);
        assert!(
            answered[1].content.contains(INTERRUPTED),
            "{}",
            answered[1].content
        );
        assert_eq!(answered[2].outcome, ToolOutcomeWord::Ok);
        let asked = world.stub.requests().last().expect("a request").to_string();
        for id in ["w1", "r2", "r3"] {
            assert!(
                asked.contains(&format!("\"tool_call_id\":\"{id}\"")),
                "{id}: {asked}"
            );
        }
        assert!(asked.contains("the plan"), "r3 ran");
        assert!(restarted.context.parked.is_empty());

        // Denied; the stop came between the parked call's answer and r2's.
        let (mut world, approvals) = deciding(vec![round(), prose("Not done.")]);
        let mut served = open(&world, &approvals);
        report(world.ask(&mut served, "do three things").await);
        let record = world.record();
        let mut denied = line_of(&record.id, ApprovalState::Decided);
        denied.decision = Some("deny".to_owned());
        cut(&mut served, &record, denied);
        drop(served);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        assert_eq!(ids(&world), ["w1", "r2", "r3"]);
        let answered = results(&world);
        assert!(answered[1].content.contains(NOT_RUN));
        assert!(answered[2].content.contains(NOT_RUN));
        assert!(restarted.context.parked.is_empty());
    }

    fn line_of(id: &str, state: ApprovalState) -> ApprovalBody {
        ApprovalBody {
            id: id.to_owned(),
            state,
            decision: None,
            by: None,
            result: None,
            reason: None,
            scope: None,
        }
    }

    /// R93P-07 / R176: every end of a parked call closes its one audit row
    /// — a deny, an expiry, a supersede, drift, an effect another copy
    /// spent, a refusal before the executor took the row — and a host that
    /// took over writes exactly one row of its own carrying the approval.
    #[tokio::test(flavor = "multi_thread")]
    async fn every_end_of_a_parked_call_closes_its_one_row() {
        use keeper_core::bots::audit::AuditOutcome::{Pending, Refused};
        let one_refused = |world: &World, served: &ServedSession, id: &str, what: &str| {
            assert_eq!(approval_rows(&world.deps.data_dir, id), [Refused], "{what}");
            assert_eq!(rows_of(world, served, "drive_write").len(), 1, "{what}");
        };

        let (mut world, _, mut served, record) = parked("mine", vec![prose("No.")]).await;
        assert_eq!(approval_rows(&world.deps.data_dir, &record.id), [Pending]);
        let denied = world.decision(&record, Decision::Deny);
        world.serve(&mut served, denied).await;
        one_refused(&world, &served, &record.id, "deny");

        let (world, approvals, served, mut record) = parked("mine", vec![prose("Late.")]).await;
        drop(served);
        record.expires_at = "2026-01-01T00:00:00.000Z".to_owned();
        world.rewrite(&record);
        let mut restarted = open(&world, &approvals);
        let (_handle, signal) = chat::cancellation();
        restarted
            .resume_approvals(&world.deps, world.room.clone(), signal)
            .await;
        one_refused(&world, &restarted, &record.id, "expiry");

        let (mut world, _, mut served, record) = parked("mine", vec![prose("Fine.")]).await;
        world.ask(&mut served, "never mind").await;
        one_refused(&world, &served, &record.id, "supersede");

        let (mut world, _, mut served, record) = parked("mine", vec![prose("Moved.")]).await;
        std::fs::write(world.tgdrive.join(NOTE), "someone else's").expect("write");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        one_refused(&world, &served, &record.id, "drift");

        let (mut world, approvals, mut served, record) =
            parked("mine", vec![prose("Spent.")]).await;
        approvals.consumed_by(&record.id, "hesperia");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        one_refused(&world, &served, &record.id, "spent elsewhere");

        let (mut world, _, mut served, record) = parked("mine", vec![prose("Gone.")]).await;
        world
            .deps
            .home
            .config
            .allow
            .retain(|tool| tool != "drive_write");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert_eq!(results(&world)[0].outcome, ToolOutcomeWord::Refused);
        one_refused(&world, &served, &record.id, "refused before the executor");

        // A host that took over, on a machine of its own, denies it.
        let (mut world, approvals, served, record) = parked("mine", vec![prose("No.")]).await;
        drop(served);
        let theirs = tempfile::tempdir().expect("tmp");
        let mut deps_b = tolas_free_deps(&world);
        deps_b.host = keeper_core::agents::log::HostSlug::new("hesperia").expect("slug");
        deps_b.data_dir = theirs.path().to_path_buf();
        let mut taker = world.open_as(&deps_b, SESSION);
        taker.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        let denied = world.decision(&record, Decision::Deny);
        let (_handle, signal) = chat::cancellation();
        taker
            .serve(&deps_b, world.room.clone(), denied, signal)
            .await
            .expect("served");
        assert_eq!(approval_rows(theirs.path(), &record.id), [Refused]);
    }

    /// Run `served`'s worker for `secs`, nothing arriving, under `activity`.
    async fn worker_for(
        world: &World,
        served: &mut ServedSession,
        activity: &keeper_agent::agent::Activity,
        secs: u64,
    ) {
        let (_queue, mut arrivals) = tokio::sync::mpsc::unbounded_channel();
        let (_handle, signal) = chat::cancellation();
        let _ = tokio::time::timeout(
            Duration::from_secs(secs),
            served.serve_arrivals(
                &world.deps,
                world.room.clone(),
                Vec::new(),
                &mut arrivals,
                signal,
                activity,
            ),
        )
        .await;
    }

    /// `record`'s expiry moved `ms` from now, as only a test does.
    fn expiring(world: &World, record: &mut ApprovalRecord, ms: i64) {
        record.expires_at = keeper_core::agents::approval::stamp(
            chrono::Utc::now() + chrono::Duration::milliseconds(ms),
        );
        world.rewrite(record);
    }

    /// R93P-08 / R177, the worker's half: it says a run waits for a person
    /// before it clears busy — after the park and again after a restart
    /// found it at serve start — and says it no longer waits once the
    /// approval ended.
    #[tokio::test(flavor = "multi_thread")]
    async fn the_worker_says_its_run_waits_before_it_is_idle() {
        let (world, approvals, served, mut record) =
            parked("mine", vec![prose("It expired.")]).await;
        drop(served);
        let mut restarted = open(&world, &approvals);
        let activity = keeper_agent::agent::Activity::starting();
        worker_for(&world, &mut restarted, &activity, 1).await;
        assert!(!activity.busy.load(Ordering::SeqCst));
        assert!(
            activity.parked.load(Ordering::SeqCst),
            "found at serve start"
        );
        expiring(&world, &mut record, 200);
        let mut again = open(&world, &approvals);
        worker_for(&world, &mut again, &activity, 3).await;
        assert!(results(&world)[0].content.contains(EXPIRED));
        assert!(!activity.parked.load(Ordering::SeqCst), "it ended");
    }

    /// R93P-12 / R177: what a worker does outside an arrival — the
    /// continuation of a decision found at serve start, a settlement tried
    /// again, an expiry — runs with busy set, so no hand-back and no
    /// window happens under it; busy clears when it is done.
    #[tokio::test(flavor = "multi_thread")]
    async fn continuations_outside_an_arrival_run_busy() {
        let (mut world, approvals, mut served, record) = parked("once", vec![prose("Done.")]).await;
        approvals.down.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        approvals.down.store(false, Ordering::SeqCst);
        drop(served);
        let activity = Arc::new(keeper_agent::agent::Activity::default());
        *approvals.probe.lock().expect("lock") = Some(Arc::clone(&activity));
        let mut restarted = open(&world, &approvals);
        worker_for(&world, &mut restarted, &activity, 1).await;
        assert_eq!(world.note().as_deref(), Some("once"), "it ran at start");
        let seen = approvals.busy_seen.lock().expect("lock").clone();
        assert!(
            !seen.is_empty() && seen.iter().all(|busy| *busy),
            "{seen:?}"
        );
        assert!(!activity.busy.load(Ordering::SeqCst));

        // The timer's expiry, on a worker that is otherwise idle.
        let (world, approvals, served, mut record) =
            parked("mine", vec![prose("It expired.")]).await;
        drop(served);
        expiring(&world, &mut record, 300);
        let activity = Arc::new(keeper_agent::agent::Activity::default());
        *approvals.probe.lock().expect("lock") = Some(Arc::clone(&activity));
        let mut restarted = open(&world, &approvals);
        worker_for(&world, &mut restarted, &activity, 3).await;
        assert!(results(&world)[0].content.contains(EXPIRED));
        let seen = approvals.busy_seen.lock().expect("lock").clone();
        assert!(
            !seen.is_empty() && seen.iter().all(|busy| *busy),
            "{seen:?}"
        );
        assert!(!activity.busy.load(Ordering::SeqCst));
    }

    /// R93P-11 / R179: the server took this copy's `consumed` but the read
    /// back failed; the network comes back, and the same worker — no
    /// restart — tries again on its clock, finds its own event first and
    /// runs the write once, sending no second `consumed`.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_settlement_whose_read_failed_runs_once_when_the_network_returns() {
        let (mut world, approvals, mut served, record) =
            parked("once", vec![prose("Written.")]).await;
        approvals.unread_after_consume.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        assert!(matches!(
            world.serve(&mut served, decided).await,
            Outcome::Decided
        ));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL), "nothing ran yet");
        assert_eq!(approvals.events().len(), 1, "accepted");
        approvals
            .unread_after_consume
            .store(false, Ordering::SeqCst);
        // The worker runs on while reads still fail — its serve start's
        // try fails too — and the network comes back under it.
        let activity = keeper_agent::agent::Activity::default();
        let back = async {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            assert_eq!(
                world.note().as_deref(),
                Some(ORIGINAL),
                "not while it is down"
            );
            approvals.unread.store(false, Ordering::SeqCst);
        };
        tokio::join!(worker_for(&world, &mut served, &activity, 4), back);
        assert_eq!(world.note().as_deref(), Some("once"));
        assert_eq!(approvals.events().len(), 1, "no second consumed");
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Ok);
        assert!(!served.waiting());
    }

    /// R93P-11 / R179: another copy's `consumed` was accepted and never
    /// mirrored here; the record expires. The room is read first: the
    /// model is told the effect is unknown — never that nothing changed.
    #[tokio::test(flavor = "multi_thread")]
    async fn an_expiry_after_a_remote_consume_says_the_effect_is_unknown() {
        let (world, approvals, served, mut record) = parked("mine", vec![prose("Unknown.")]).await;
        drop(served);
        approvals.consumed_by(&record.id, "electra");
        expiring(&world, &mut record, 300);
        let mut restarted = open(&world, &approvals);
        let activity = keeper_agent::agent::Activity::default();
        worker_for(&world, &mut restarted, &activity, 3).await;
        let last = world.approval_lines().pop().expect("a line");
        assert_eq!(last.state, ApprovalState::Consumed);
        assert_eq!(last.result.as_deref(), Some("unknown"));
        let said = &results(&world)[0].content;
        assert!(said.contains(&effect_unknown("electra")), "{said}");
        assert!(!said.contains(EXPIRED), "{said}");
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// R93P-14 / R179: a read of the room that stops at its bound finding
    /// nothing is no evidence: a decision sends no `consumed` over it and
    /// runs nothing; at expiry the call ends refused, its effect unknown.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_read_cut_at_its_bound_never_counts_as_nothing_consumed() {
        use keeper_agent::approvals::{HISTORY_UNREAD, HISTORY_UNREAD_RESULT};
        let (mut world, approvals, mut served, mut record) =
            parked("mine", vec![prose("Unknown.")]).await;
        approvals.truncated.store(true, Ordering::SeqCst);
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        assert!(approvals.events().is_empty(), "no consumed sent blind");
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
        assert!(served.waiting());
        drop(served);
        expiring(&world, &mut record, 300);
        let mut restarted = open(&world, &approvals);
        let activity = keeper_agent::agent::Activity::default();
        worker_for(&world, &mut restarted, &activity, 3).await;
        let last = world.approval_lines().pop().expect("a line");
        assert_eq!(
            (last.state, last.reason.as_deref()),
            (ApprovalState::Refused, Some(HISTORY_UNREAD))
        );
        assert!(results(&world)[0].content.contains(HISTORY_UNREAD_RESULT));
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// R93P-14 / R179: a request that went to the approvers' DMs has no
    /// event in the session room; its `approval requested` line names the
    /// status sent into the room before it, and every read of the room for
    /// it starts there — never at the room's start.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_dm_routed_request_is_read_from_the_status_before_it() {
        let (mut world, approvals) =
            deciding(vec![calls(&[write_note("w1", "after")]), prose("Written.")]);
        nixis_dm(&world);
        let delegations = Delegations::over(known_with_proxy());
        let doors = Arc::new(Doors::default());
        let mut served = world.delegating(&delegations);
        served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
        served.doors = Some(doors.clone());
        world
            .room
            .set_members(&[TGORKA, MARTA, NIXI, "@eve:example.org"]);
        let report = report(world.ask(&mut served, "write a note").await);
        assert_eq!(report.ending, TurnEnding::Parked);
        let record = world.record();
        assert!(world.sent_of(APPROVAL_REQUEST).is_empty());
        let requested = kinds(&world.lines(SESSION), LineKind::Approval)[0].clone();
        let cursor = requested.matrix_event.clone().expect("a cursor");
        let sent = world.room.sent();
        let at: usize = cursor
            .as_str()
            .trim_start_matches("$sent")
            .trim_end_matches(":example.org")
            .parse()
            .expect("one of the room's sends");
        assert_eq!(sent[at - 1].0, STATUS, "the cursor is the status event");
        let decided = world.decision(&record, Decision::Approve);
        world.serve(&mut served, decided).await;
        let froms = approvals.froms.lock().expect("lock").clone();
        assert!(!froms.is_empty());
        assert!(
            froms.iter().all(|from| from.as_ref() == Some(&cursor)),
            "{froms:?}"
        );
        assert_eq!(world.note().as_deref(), Some("after"));
    }

    /// R93P-13 / R180: arguments over 16 KiB go as an encrypted file; when
    /// the upload fails no request is sent anywhere — one without them
    /// could be approved unseen — and the call is refused, its turn
    /// closed saying why.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_request_whose_arguments_did_not_upload_is_never_sent() {
        use keeper_agent::approvals::UNATTACHED;
        let large = "x".repeat(20 * 1024);
        let (mut world, approvals) = deciding(vec![calls(&[write_note("w1", &large)])]);
        approvals.no_uploads.store(true, Ordering::SeqCst);
        let mut served = open(&world, &approvals);
        let _ = world.ask(&mut served, "write a long note").await;
        assert!(world.sent_of(APPROVAL_REQUEST).is_empty());
        assert!(world
            .approval_lines()
            .iter()
            .all(|line| line.state != ApprovalState::Requested));
        assert!(!served.waiting());
        let results = results(&world);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome, ToolOutcomeWord::Refused);
        let errors = kinds(&world.lines(SESSION), LineKind::Error)
            .iter()
            .map(|line| match &line.body {
                LineBody::Error(error) => error.sentence.clone(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();
        assert!(errors.iter().any(|e| e.contains(UNATTACHED)), "{errors:?}");
        assert_eq!(world.note().as_deref(), Some(ORIGINAL));
    }

    /// R93P-15 / R180: a synced session whose `approvals/` — or whose
    /// `approvals/blobs/` — is a link out of the session gets nothing
    /// written through it: the park is refused and the folder it points at
    /// stays empty.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_approvals_folder_linked_out_of_the_session_is_refused() {
        for blobs in [false, true] {
            let content = if blobs {
                "y".repeat(20 * 1024)
            } else {
                "small".to_owned()
            };
            let (mut world, approvals) = deciding(vec![calls(&[write_note("w1", &content)])]);
            let outside = tempfile::tempdir().expect("outside");
            if blobs {
                std::fs::create_dir(world.approvals()).expect("approvals");
                std::os::unix::fs::symlink(outside.path(), world.approvals().join("blobs"))
                    .expect("link");
            } else {
                std::os::unix::fs::symlink(outside.path(), world.approvals()).expect("link");
            }
            let mut served = open(&world, &approvals);
            let _ = world.ask(&mut served, "write a note").await;
            assert_eq!(
                std::fs::read_dir(outside.path()).expect("outside").count(),
                0,
                "nothing written out of the session (blobs: {blobs})"
            );
            assert!(world.sent_of(APPROVAL_REQUEST).is_empty());
            assert!(!served.waiting());
            assert_eq!(results(&world)[0].outcome, ToolOutcomeWord::Refused);
        }
    }

    /// The run lines of the session at `path`, in order.
    fn run_lines(world: &World, path: &str) -> Vec<keeper_core::agents::log::RunState> {
        world
            .lines(path)
            .iter()
            .filter_map(|line| match &line.body {
                LineBody::Run(run) => Some(run.state),
                _ => None,
            })
            .collect()
    }

    /// R93P-09 and R93P-10 / R178: a scheduled run's `card_update` of its
    /// own card parks — the card says `run: blocked` before the park pins
    /// it, so the host's own write is no drift — and, approved, the change
    /// lands and the continuation ends the run: the card reads
    /// `run: review` and the log `running, blocked, review`. A denied one
    /// ends it too, its change not made.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_scheduled_runs_own_card_update_is_approved_and_ends_the_run() {
        use keeper_agent::agent::scheduled_arrival;
        use keeper_agent::cards::Scheduled;
        use keeper_core::agents::card::{CardAgent, Field, Run};
        use keeper_core::agents::log::RunState as LogRun;
        const SCHEDULED: &str = "active/2026-10-05-sort";
        for decision in [Decision::Approve, Decision::Deny] {
            let mut world = world(
                ProviderKind::OpenAi,
                &["drive_read", "card_update"],
                vec![
                    calls(&[(
                        "c1",
                        "card_update",
                        json!({"card": "card.md", "fields": {"schedule": "@daily"}}),
                    )]),
                    prose("Done."),
                ],
            );
            world.deps.decisions = Some(Admit::pinned());
            let approvals = Arc::new(Approvals::default());
            session_of(
                &world.tgdrive,
                SCHEDULED,
                &decl("tgdrive", &[TGORKA, MARTA], false),
                "nixi",
                SessionKind::Scheduled,
                "!sort:example.org",
            );
            let card = "---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\nassignee: nixi\nschedule: \"@hourly\"\nlast_run: \"2026-10-05T08:00:00Z\"\n---\n\nSort what came in.\n";
            write(
                &world.tgdrive,
                &format!("60-sessions/{SCHEDULED}/card.md"),
                card,
            );
            let mut served = world.open(SCHEDULED);
            served.approval_room = Some(Arc::clone(&approvals) as Arc<dyn ApprovalRoom>);
            let arrival = scheduled_arrival(
                &user("@nixi:example.org"),
                &Scheduled::Run {
                    card: "card.md".to_owned(),
                    window: "2026-10-05T09:00:00.000Z".to_owned(),
                    now_ms: chrono::DateTime::parse_from_rfc3339("2026-10-05T09:30:00Z")
                        .expect("an instant")
                        .timestamp_millis(),
                    utc_offset_minutes: 0,
                },
            )
            .expect("an arrival");
            let parked = report(world.serve(&mut served, arrival).await);
            assert_eq!(parked.ending, TurnEnding::Parked);
            let card_at = world.dir(SCHEDULED).join("card.md");
            let card_now = || {
                let text = std::fs::read_to_string(&card_at).expect("card");
                (CardAgent::of_text(&text).expect("keys"), text)
            };
            assert_eq!(card_now().0.run, Some(Field::Read(Run::Blocked)));
            // An unattended run raises the card change to T4, which only the
            // person at the head of the record's `dispatch_chain` decides; a
            // scheduled run names none (DW-510), so the test names tgorka.
            let mut record = world.record_in(SCHEDULED);
            record.dispatch_chain = vec![TGORKA.to_owned()];
            std::fs::write(
                world
                    .dir(SCHEDULED)
                    .join(format!("approvals/{}.json", record.id)),
                serde_json::to_string(&record).expect("json"),
            )
            .expect("rewrite");
            let decided = world.decision(&record, decision);
            assert!(matches!(
                world.serve(&mut served, decided).await,
                Outcome::Decided
            ));
            let (keys, text) = card_now();
            assert_eq!(keys.run, Some(Field::Read(Run::Review)), "{text}");
            let results = tool_results(&world.lines(SCHEDULED));
            if decision == Decision::Approve {
                assert_eq!(results[0].outcome, ToolOutcomeWord::Ok, "{results:?}");
                assert!(text.contains("@daily"), "{text}");
            } else {
                assert!(results[0].content.contains(DENIED), "{results:?}");
                assert!(!text.contains("@daily"), "{text}");
            }
            assert_eq!(
                run_lines(&world, SCHEDULED),
                [LogRun::Running, LogRun::Blocked, LogRun::Review]
            );
            assert!(!served.waiting());
        }
    }
}
