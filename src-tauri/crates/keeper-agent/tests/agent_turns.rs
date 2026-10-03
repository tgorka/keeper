//! An agent's turns in a session, end to end against a local model stub and
//! a recording room port (story 90.5, acceptance 5, 6, 8–10, 16, 18–20).
//!
//! No homeserver and no git: the drives are folders, the room is a fake
//! [`EditPort`] that accepts every send, and the provider is an
//! OpenAI-shaped SSE stub that records every request body.
#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::agent::{
    arm_agent, trail_of, AgentDeps, AgentProfiles, Arrived, Outcome, Probe, ServedSession,
    SessionRef, Trail, TurnEnding, LOCAL_ONLY_REFUSAL, NARROWER_THAN_ROOM,
};
use keeper_agent::host::UNATTENDED_REFUSAL;
use keeper_agent::matrix_sink::{EditPort, SendFuture, FINAL_CUT_BYTES};
use keeper_agent::rooms::Arrival;
use keeper_agent::runtime::Router;
use keeper_agent::turn::{DrivePorts, TurnEnv};
use keeper_agent::zone::{read_zone, AgentHome};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::STATUS;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::reader::{hydrate_blob, read_session};
use keeper_core::agents::log::replay::replay;
use keeper_core::agents::log::{
    AssistantBody, HostSlug, LineBody, LineKind, LogLine, ToolCallBody, ToolOutcomeWord,
    ToolResultBody, UserBody,
};
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use keeper_core::bots::chat::{self, CancelHandle};
use keeper_core::bots::{store, Bot, Provider, ProviderKind};
use keeper_core::error::CoreError;
use keeper_core::platform::Platform;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedTransactionId, OwnedUserId};
use serde_json::{json, Value};

const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";
const SESSION: &str = "active/2026-10-02-chat";

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
/// first send cancels the turn.
#[derive(Default)]
struct Room {
    sent: Mutex<Vec<(String, Value)>>,
    stop: Mutex<Option<CancelHandle>>,
}

impl Room {
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
            let mut sent = self.sent.lock().expect("lock");
            sent.push((event_type.to_owned(), content));
            Ok(OwnedEventId::try_from(format!("$sent{}:example.org", sent.len())).expect("id"))
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
                    // A `{"pause_ms": n}` entry is no frame: the stream
                    // waits there, so the edits in between are paced.
                    let mut parts: Vec<(String, u64)> = Vec::new();
                    for data in completion {
                        match data["pause_ms"].as_u64() {
                            Some(pause) => parts.push((String::new(), pause)),
                            None => parts.push((format!("data: {data}\n\n"), 0)),
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
    let root = tempfile::tempdir().expect("tempdir");
    let tgdrive = root.path().join("tgdrive");
    let private = root.path().join("private");
    let data = root.path().join("data");
    std::fs::create_dir_all(&data).expect("data");
    let tg_decl = decl("tgdrive", &[TGORKA, MARTA], false);
    let private_decl = decl("private", &[TGORKA], true);
    write(
        &tgdrive,
        "80-agents/_drive.toml",
        &format!("version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\", \"{MARTA}\"]\n"),
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
    };
    World {
        _root: root,
        tgdrive,
        deps,
        stub,
        room: Arc::new(Room::default()),
        next: 0,
    }
}

fn session(drive: &Path, path: &str, home: &DriveDecl) -> SessionAgent {
    let agent = SessionAgent {
        id: ulid::Ulid::new(),
        agent: "nixi".to_owned(),
        drive: "tgdrive".to_owned(),
        kind: SessionKind::Conversation,
        title: "chat".to_owned(),
        requested_by: user(TGORKA),
        parent: None,
        room: "!room:example.org".try_into().expect("room"),
        drives: vec!["tgdrive".to_owned(), "private".to_owned()],
        label: Label::opening(home, Integrity::Owner),
        needs: None,
        pin: None,
        hop: 0,
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
            0,
            None,
        )
        .expect("served")
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

/// C4/F12: a write under the agent's profile-wide grant is an ask, and an
/// unattended ask is refused with `UNATTENDED_REFUSAL`; the model reads it
/// as `Refused: …` (D9) and the file is untouched.
#[tokio::test(flavor = "multi_thread")]
async fn a_write_needing_approval_is_refused_before_epic_93() {
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
        let ServedSession { context, writer } = &mut served;
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
        .recover(&world.deps, world.room.as_ref(), &Trail::default())
        .await
        .expect("recover"));
    let mut again = world.open(SESSION);
    assert!(!again
        .recover(&world.deps, world.room.as_ref(), &Trail::default())
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
        let ServedSession { context, writer } = &mut served;
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
        .recover(&world.deps, world.room.as_ref(), &trail)
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
    let ServedSession { context, writer } = &mut served;
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
