//! The extracted turn loop reproduces the goldens captured from the code
//! before the move (90.1 acceptance 5 and 6). The goldens were written on a
//! Mac by a throwaway shell test driving the same three scenarios against the
//! same stub; nothing here regenerates them. The same stub also drives the
//! approval round trip a turn with a person at it makes.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_agent::approval::{self, SinkApprover};
use keeper_agent::drive;
use keeper_agent::ports::{ApprovalPort, ProfileSource, TurnSink};
use keeper_agent::task::TaskRunner;
use keeper_agent::turn::{self, DrivePorts, TurnEnv, TurnOrigin};
use keeper_core::bots::audit::{self, AuditOutcome, AuditVerdict};
use keeper_core::bots::grant::{Grant, GrantMode, GrantScope};
use keeper_core::bots::{session, store, Bot, Provider, ProviderKind};
use keeper_core::error::CoreError;
use keeper_core::platform::Platform;
use keeper_core::vm::{BotChatSendReq, BotStreamEvent};
use keeper_sync::platform::{BotTaskRunner, BotTaskSpec};
use keeper_sync::SyncProfile;
use serde_json::{json, Value};

struct TestPlatform {
    dir: PathBuf,
}

impl Platform for TestPlatform {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.dir.clone())
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
        Err(CoreError::Unsupported("no sidecar in a test".to_owned()))
    }
    fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

/// One answer the stub gives to a POST: SSE chunks written 100 ms apart, the
/// row observed after each; `stall` keeps the socket open until released.
struct Reply {
    chunks: Vec<Value>,
    stall: bool,
}

#[derive(Default)]
struct StubLog {
    queue: VecDeque<Reply>,
    lines: Vec<String>,
    requests: Vec<Value>,
    partial_at_request: Vec<bool>,
    flushed: Vec<usize>,
    release: bool,
}

struct Stub {
    base: String,
    log: Arc<Mutex<StubLog>>,
}

fn newest_assistant(dir: &Path) -> Option<session::BotMessage> {
    let sessions = session::list_sessions(dir, true).ok()?;
    sessions
        .iter()
        .flat_map(|row| session::list_messages(dir, &row.id).unwrap_or_default())
        .filter(|message| message.role == "assistant")
        .max_by_key(|message| (message.created_ms, message.seq))
}

fn stub(dir: PathBuf) -> Stub {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind stub");
    let base = format!("http://{}", listener.local_addr().expect("stub address"));
    let log = Arc::new(Mutex::new(StubLog::default()));
    let served = Arc::clone(&log);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            let _ = socket.set_read_timeout(Some(Duration::from_secs(10)));
            let mut reader = BufReader::new(socket.try_clone().expect("clone socket"));
            let mut first = String::new();
            if reader.read_line(&mut first).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse::<usize>().unwrap_or(0);
                }
            }
            let mut words = first.split_whitespace();
            let method = words.next().unwrap_or_default().to_owned();
            let path = words.next().unwrap_or_default().to_owned();
            served
                .lock()
                .expect("log")
                .lines
                .push(format!("{method} {path}"));
            if method != "POST" {
                let _ = write!(
                    socket,
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                continue;
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let partial =
                newest_assistant(&dir).is_some_and(|row| row.partial && row.content.is_empty());
            let reply = {
                let mut log = served.lock().expect("log");
                log.requests
                    .push(serde_json::from_slice(&body).unwrap_or(Value::Null));
                log.partial_at_request.push(partial);
                log.queue.pop_front()
            };
            let Some(reply) = reply else {
                let _ = write!(
                    socket,
                    "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                continue;
            };
            let parts: Vec<String> = reply
                .chunks
                .iter()
                .map(|chunk| format!("data: {chunk}\n\n"))
                .collect();
            let done = if reply.stall { "" } else { "data: [DONE]\n\n" };
            let total = parts.iter().map(String::len).sum::<usize>()
                + done.len()
                + if reply.stall { 1 } else { 0 };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
            );
            for part in &parts {
                let _ = socket.write_all(part.as_bytes());
                let _ = socket.flush();
                std::thread::sleep(Duration::from_millis(100));
                let seen = newest_assistant(&dir).map_or(0, |row| row.content.len());
                served.lock().expect("log").flushed.push(seen);
            }
            if reply.stall {
                for _ in 0..200 {
                    if served.lock().expect("log").release {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            } else {
                let _ = socket.write_all(done.as_bytes());
            }
        }
    });
    Stub { base, log }
}

fn tool_call(id: &str, name: &str, arguments: Value) -> Value {
    json!({"model":"model","choices":[{"index":0,"delta":{"tool_calls":[
        {"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}
    ]},"finish_reason":"tool_calls"}]})
}

/// Ten deltas of 100 bytes each, then the finish.
fn long_answer() -> Vec<Value> {
    let mut chunks: Vec<Value> = (0..10)
        .map(|n| {
            let text = format!("{:<99}\n", format!("Line {n} of the answer. The first line is: First line."));
            json!({"model":"model","choices":[{"index":0,"delta":{"content":text},"finish_reason":null}]})
        })
        .collect();
    chunks.push(
        json!({"model":"model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":11,"completion_tokens":22,"total_tokens":33}}),
    );
    chunks
}

fn stalled_answer() -> Vec<Value> {
    long_answer().into_iter().take(6).collect()
}

fn is_ulid(word: &str) -> bool {
    word.len() == 26
        && word
            .bytes()
            .all(|b| b.is_ascii_digit() || (b.is_ascii_uppercase() && !b"ILOU".contains(&b)))
}

fn mask_text(text: &str, roots: &[(&str, &str)]) -> String {
    let mut text = text.to_owned();
    for (from, to) in roots {
        text = text.replace(from, to);
    }
    let mut out = String::with_capacity(text.len());
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once('\0')) {
        if c.is_ascii_alphanumeric() {
            word.push(c);
            continue;
        }
        out.push_str(if is_ulid(&word) { "<ulid>" } else { &word });
        word.clear();
        if c != '\0' {
            out.push(c);
        }
    }
    out
}

fn mask(value: Value, roots: &[(&str, &str)]) -> Value {
    match value {
        Value::String(text) => Value::String(mask_text(&text, roots)),
        Value::Array(items) => Value::Array(items.into_iter().map(|v| mask(v, roots)).collect()),
        Value::Object(map) => Value::Object(
            map.into_iter()
                .filter(|(key, _)| !key.ends_with("Ms") && !key.ends_with("_ms"))
                .map(|(key, v)| (key, mask(v, roots)))
                .collect(),
        ),
        other => other,
    }
}

fn rows(dir: &Path, session_id: &str) -> Value {
    let messages = session::list_messages(dir, session_id).expect("messages");
    Value::Array(
        messages
            .iter()
            .map(|m| {
                json!({"role":m.role,"partial":m.partial,"finish_reason":m.finish_reason,
                    "tool_call_count":m.tool_call_count,"content":m.content})
            })
            .collect(),
    )
}

fn audit_rows(dir: &Path, session_prefix: &str) -> Value {
    let mut found: Vec<audit::AuditRow> = audit::list_audit(dir, None, None)
        .expect("audit")
        .into_iter()
        .filter(|row| row.session_id.starts_with(session_prefix))
        .collect();
    found.reverse();
    Value::Array(
        found
            .iter()
            .map(|row| {
                json!({"tool":row.tool,"effect":format!("{:?}",row.effect),
                    "verdict":format!("{:?}",row.verdict),"outcome":format!("{:?}",row.outcome)})
            })
            .collect(),
    )
}

struct Events(Arc<Mutex<Vec<Value>>>);

impl Events {
    fn kinds(&self) -> Vec<String> {
        self.0
            .lock()
            .expect("events")
            .iter()
            .map(|e| e["kind"].as_str().unwrap_or_default().to_owned())
            .collect()
    }
    fn first(&self, kind: &str) -> Value {
        self.0
            .lock()
            .expect("events")
            .iter()
            .find(|e| e["kind"] == kind)
            .cloned()
            .unwrap_or(Value::Null)
    }
    fn count(&self, kind: &str) -> usize {
        self.kinds().iter().filter(|k| k.as_str() == kind).count()
    }
    async fn wait_closed(&self) {
        for _ in 0..1000 {
            if self.count("closed") > 0 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("the turn never closed: {:?}", self.kinds());
    }
}

impl TurnSink for Events {
    fn event(&self, event: BotStreamEvent) -> bool {
        self.0
            .lock()
            .expect("events")
            .push(serde_json::to_value(event).expect("event JSON"));
        true
    }
}

fn new_sink() -> (Arc<dyn TurnSink>, Events) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    (Arc::new(Events(Arc::clone(&seen))), Events(seen))
}

struct Profiles(PathBuf);

impl ProfileSource for Profiles {
    fn profiles(&self) -> Vec<SyncProfile> {
        vec![SyncProfile::new(
            "folder",
            "Folder",
            self.0.clone(),
            "unused",
        )]
    }
}

fn take_log(log: &Arc<Mutex<StubLog>>) -> Value {
    let mut log = log.lock().expect("log");
    let out = json!({
        "http": std::mem::take(&mut log.lines),
        "requests": std::mem::take(&mut log.requests),
        "partial_at_request": std::mem::take(&mut log.partial_at_request),
        "flushed": std::mem::take(&mut log.flushed),
    });
    log.release = false;
    out
}

fn seed(dir: &Path, root: &Path) {
    std::fs::create_dir_all(root.join("notes")).expect("notes");
    std::fs::write(root.join("notes/hello.md"), "First line.\nSecond line.\n").expect("hello");
    std::fs::write(root.join("notes/AGENTS.md"), "Answer briefly.\n").expect("agents");
    store::insert_provider(
        dir,
        &Provider {
            id: "provider".to_owned(),
            kind: ProviderKind::Ollama,
            name: "Stub".to_owned(),
            base_url: String::new(),
            created_ms: 1,
        },
    )
    .expect("provider");
    store::insert_bot(
        dir,
        &Bot {
            id: "bot".to_owned(),
            provider_id: "provider".to_owned(),
            target: "model".to_owned(),
            name: "Stub bot".to_owned(),
            pin_order: 0,
            identity: Default::default(),
            created_ms: 1,
        },
    )
    .expect("bot");
    store::save_grant(
        dir,
        &Grant {
            id: "grant".to_owned(),
            provider_id: "provider".to_owned(),
            bot_id: Some("bot".to_owned()),
            scope: GrantScope::Subtree {
                profile_id: "folder".to_owned(),
                subpath: "notes".to_owned(),
            },
            mode: GrantMode::Read,
            created_ms: 1,
        },
    )
    .expect("grant");
}

fn golden(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/characterisation")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("golden")).expect("golden JSON")
}

fn read_then_answer() -> [Reply; 2] {
    [
        Reply {
            chunks: vec![tool_call(
                "read-1",
                "drive_read",
                json!({"path":"notes/hello.md"}),
            )],
            stall: false,
        },
        Reply {
            chunks: long_answer(),
            stall: false,
        },
    ]
}

/// A data directory and a drive seeded by [`seed`], the provider pointed at a
/// fresh stub, and a turn environment over them whose asks go to `approval`.
struct World {
    _data: tempfile::TempDir,
    _drive: tempfile::TempDir,
    dir: PathBuf,
    root: PathBuf,
    stub: Stub,
    env: TurnEnv,
}

fn world(approval: Option<Arc<dyn ApprovalPort>>) -> World {
    let data = tempfile::tempdir().expect("data dir");
    let drive_dir = tempfile::tempdir().expect("drive");
    let dir = data.path().canonicalize().expect("canonical data dir");
    let root = drive_dir.path().canonicalize().expect("canonical drive");
    seed(&dir, &root);
    let stub = stub(dir.clone());
    let mut provider = store::get_provider(&dir, "provider")
        .expect("read provider")
        .expect("provider row")
        .provider;
    provider.base_url = stub.base.clone();
    store::update_provider(&dir, &provider).expect("point at stub");
    let platform: Arc<dyn Platform> = Arc::new(TestPlatform { dir: dir.clone() });
    let env = TurnEnv {
        platform,
        account: None,
        drive: Some(DrivePorts {
            profiles: Arc::new(Profiles(root.clone())),
            vault: None,
            approval,
        }),
    };
    World {
        _data: data,
        _drive: drive_dir,
        dir,
        root,
        stub,
        env,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_extracted_turn_reproduces_the_goldens() {
    let World {
        dir,
        root,
        stub,
        env,
        _data,
        _drive,
    } = world(None);
    let dir_text = dir.display().to_string();
    let root_text = root.display().to_string();
    let base_text = stub.base.clone();
    let roots: [(&str, &str); 3] = [
        (root_text.as_str(), "<drive>"),
        (dir_text.as_str(), "<data>"),
        (base_text.as_str(), "<stub>"),
    ];

    for (name, spoken) in [("typed", false), ("spoken", true)] {
        stub.log
            .lock()
            .expect("log")
            .queue
            .extend(read_then_answer());
        let origin = move |_: &Path| {
            if spoken {
                TurnOrigin::Spoken {
                    language: "pl-PL".to_owned(),
                }
            } else {
                TurnOrigin::Typed
            }
        };
        let (sink, events) = new_sink();
        let opened = turn::open_turn(
            &env,
            BotChatSendReq {
                session_id: None,
                bot_id: "bot".to_owned(),
                model: "model".to_owned(),
                text: "Read notes/hello.md and tell me its first line.".to_owned(),
                attachment_ids: Vec::new(),
            },
            &origin,
        )
        .await
        .expect("open");
        drive::spawn_turn(opened, sink);
        events.wait_closed().await;
        let session_id = events.first("opened")["session"]["id"]
            .as_str()
            .expect("session id")
            .to_owned();
        let mut produced = json!({
            "events": events.kinds(),
            "stub": take_log(&stub.log),
            "messages": rows(&dir, &session_id),
            "audit": audit_rows(&dir, &session_id),
        });
        if !spoken {
            stub.log.lock().expect("log").queue.push_back(Reply {
                chunks: stalled_answer(),
                stall: true,
            });
            let (sink, stopped) = new_sink();
            let opened = turn::open_turn(
                &env,
                BotChatSendReq {
                    session_id: Some(session_id.clone()),
                    bot_id: "bot".to_owned(),
                    model: "model".to_owned(),
                    text: "Again, slowly.".to_owned(),
                    attachment_ids: Vec::new(),
                },
                &origin,
            )
            .await
            .expect("open the stopped turn");
            let subscription = drive::spawn_turn(opened, sink);
            for _ in 0..500 {
                if stopped.count("delta") >= 6 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            tokio::time::sleep(Duration::from_millis(300)).await;
            drive::stop(&subscription);
            stopped.wait_closed().await;
            stub.log.lock().expect("log").release = true;
            tokio::time::sleep(Duration::from_millis(200)).await;
            produced["stopped"] = json!({
                "events": stopped.kinds(),
                "closed_reason": stopped.first("closed")["reason"],
                "stub": take_log(&stub.log),
                "messages": rows(&dir, &session_id),
            });
        }
        assert_eq!(
            mask(produced, &roots),
            golden(name),
            "the {name} scenario moved"
        );
    }

    stub.log
        .lock()
        .expect("log")
        .queue
        .extend(read_then_answer());
    let record = TaskRunner::new(env.clone())
        .run(BotTaskSpec {
            task_id: "task".to_owned(),
            profile_id: "folder".to_owned(),
            bot_id: "bot".to_owned(),
            model: Some("model".to_owned()),
            prompt_subpath: "prompt.md".to_owned(),
            prompt_text: "# Task\n\nRead notes/hello.md and report its first line.\n".to_owned(),
        })
        .await;
    let produced = json!({
        "record": {
            "outcome": format!("{:?}", record.outcome),
            "answer": record.answer,
            "tool_calls": record.tool_calls,
            "warnings": record.warnings,
            "errors": record.errors,
            "prompt_tokens": record.prompt_tokens,
            "completion_tokens": record.completion_tokens,
            "finish_reason": record.finish_reason,
            "model": record.model,
        },
        "stub": take_log(&stub.log),
        "audit": audit_rows(&dir, "task:"),
    });
    assert_eq!(
        mask(produced, &roots),
        golden("task"),
        "the task scenario moved"
    );
}

/// The spoken turn's case: its stream is an app-wide event rather than a
/// pane's channel, and its approvals go down that same stream. The ask shows
/// on the stream, nothing is written while it waits, and the answer given by
/// the ask's id releases the tool call — the write lands and the turn ends.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ask_sent_down_the_turns_stream_is_answered_by_its_id_and_releases_the_turn() {
    let (sink, events) = new_sink();
    let World {
        dir,
        root,
        stub,
        env,
        _data,
        _drive,
    } = world(Some(Arc::new(SinkApprover::new(Arc::clone(&sink)))));
    store::save_grant(
        &dir,
        &Grant {
            id: "grant".to_owned(),
            provider_id: "provider".to_owned(),
            bot_id: Some("bot".to_owned()),
            scope: GrantScope::Profile {
                profile_id: "folder".to_owned(),
            },
            mode: GrantMode::Write,
            created_ms: 1,
        },
    )
    .expect("a write grant that asks");
    stub.log.lock().expect("log").queue.extend([
        Reply {
            chunks: vec![tool_call(
                "write-1",
                "drive_write",
                json!({"path":"notes/hello.md","content":"Written.\n"}),
            )],
            stall: false,
        },
        Reply {
            chunks: long_answer(),
            stall: false,
        },
    ]);
    let spoken = |_: &Path| TurnOrigin::Spoken {
        language: "pl-PL".to_owned(),
    };
    let opened = turn::open_turn(
        &env,
        BotChatSendReq {
            session_id: None,
            bot_id: "bot".to_owned(),
            model: "model".to_owned(),
            text: "Write that down.".to_owned(),
            attachment_ids: Vec::new(),
        },
        &spoken,
    )
    .await
    .expect("open");
    let subscription = drive::spawn_turn(opened, sink);

    let mut asked = Value::Null;
    for _ in 0..500 {
        asked = events.first("approvalAsked");
        if !asked.is_null() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let request_id = asked["request"]["requestId"].as_str().map(str::to_owned);
    let written = root.join("notes/hello.md");
    let early =
        std::fs::read_to_string(&written).expect("the note") != "First line.\nSecond line.\n";
    if let Some(request_id) = &request_id {
        approval::answer(request_id, true);
    }
    let mut closed = false;
    for _ in 0..500 {
        closed = events.count("closed") > 0;
        if closed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if !closed {
        // Release the waiting tool call before failing, so the runtime can
        // shut down.
        drive::stop(&subscription);
        events.wait_closed().await;
    }

    assert!(
        request_id.is_some(),
        "the ask never reached the turn's stream: {:?}",
        events.kinds()
    );
    assert!(!early, "the write landed before anyone answered");
    assert!(
        closed,
        "the answer did not release the turn: {:?}",
        events.kinds()
    );
    assert_eq!(
        std::fs::read_to_string(&written).expect("the approved write"),
        "Written.\n"
    );
    assert!(events.first("closed")["reason"].is_null());
    let audit = audit::list_audit(&dir, None, None).expect("audit");
    assert_eq!(audit.len(), 1);
    assert_eq!(audit[0].verdict, Some(AuditVerdict::Ask));
    assert_eq!(audit[0].outcome, AuditOutcome::Ok);
}
