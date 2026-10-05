//! A steward's own sessions against a real homeserver (story 92.5,
//! acceptance 7 — ruling R13's smoke, with the stub model and with a real
//! one).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users, from the
//! environment as `live_turn.rs` reads them; the real-model test also needs
//! CLIProxyAPI:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! KEEPER_OPENAI_SMOKE_BASE_URL=<CLIProxyAPI's base URL> \
//! KEEPER_OPENAI_SMOKE_TOKEN_FILE=$HOME/.omp/cliproxyapi.token \
//! KEEPER_OPENAI_SMOKE_MODEL=<a model it serves> \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_stewards -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The admin token also makes (or re-passwords) the steward's user,
//! `tola-smoke`.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use keeper_agent::host::UNATTENDED_REFUSAL;
use keeper_core::agents::events::{CLAIM, STEWARD_ROOM};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::seed::{self, steward_session_id, SeedChoices, CATALOGUE};
use keeper_core::agents::session::{parse_session_agent_toml, SessionKind};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");

/// A letter whose work Nixi can do.
const SUMMARY: &str =
    "From: @tgorka-smoke\n\nPlease ask Nixi to say in one line that the inbox is sorted.\n";
/// A letter asking for what only a person may allow: a schedule.
const STANDUP: &str = "From: @tgorka-smoke\n\nPlease put this on a schedule so it runs every day at nine: remind me of the standup.\n";

/// One `keeper-agentd run` hosting one agent of the drive.
struct Host {
    root: tempfile::TempDir,
    env: Vec<(String, String)>,
    child: Option<Child>,
}

impl Host {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command.args(args).env_clear();
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.root.path().join("agentd.log")).unwrap_or_default()
    }

    fn start(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.root.path().join("agentd.log"))
            .expect("log");
        let child = self
            .command(&["run"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("keeper-agentd run");
        self.child = Some(child);
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status();
            let deadline = Instant::now() + Duration::from_secs(30);
            while child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(200));
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// The session folders of this host's checkout under `active/`.
    fn sessions(&self) -> Vec<PathBuf> {
        find_dirs(self.root.path(), "active")
            .into_iter()
            .flat_map(|active| std::fs::read_dir(active).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect()
    }

    /// Every log line of the session folder `dir`.
    fn lines(dir: &Path) -> Vec<Value> {
        let mut out = Vec::new();
        for chunk in std::fs::read_dir(dir.join("log"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if chunk.path().extension().is_some_and(|ext| ext == "jsonl") {
                let text = std::fs::read_to_string(chunk.path()).unwrap_or_default();
                out.extend(text.lines().filter_map(|l| serde_json::from_str(l).ok()));
            }
        }
        out
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Every directory named `name` under `root` that sits in a sessions zone.
fn find_dirs(root: &Path, name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || path.ends_with(".git") {
                continue;
            }
            if path.ends_with(name) && path.parent().is_some_and(|p| p.ends_with("60-sessions")) {
                out.push(path.clone());
            }
            stack.push(path);
        }
    }
    out
}

/// The provider a host's agent runs on: its base URL and, for a real one,
/// its token (handed to agentd as `secret:provider`, never printed).
struct Provider {
    base_url: String,
    token: Option<String>,
}

/// What both hosts share: the drive's bare remote, its owner, and the
/// principal's control room.
struct Shared<'a> {
    bare: &'a Path,
    person: &'a OwnedUserId,
    control: &'a OwnedRoomId,
}

/// A host named `slug` hosting `agent` of the shared drive, with the
/// principal's control room, signed in.
fn host(
    smoke: &Smoke,
    slug: &str,
    agent: &str,
    password: &str,
    provider: &Provider,
    shared: &Shared,
) -> Host {
    let Shared {
        bare,
        person,
        control,
    } = shared;
    let root = tempfile::tempdir().expect("tempdir");
    let credential = if provider.token.is_some() {
        "credential = \"secret:provider\"\n"
    } else {
        ""
    };
    let config = format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"{slug}\"\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"{control}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{}\"\n{credential}\n[[agents]]\ndrive = \"smoke\"\nids = [\"{agent}\"]\n",
        smoke.homeserver,
        bare.display(),
        provider.base_url,
    );
    let config_dir = root.path().join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("agentd.toml"), config).expect("config");
    let dir = |name: &str| root.path().join(name).display().to_string();
    let mut env = vec![
        ("HOME".to_owned(), dir("")),
        ("XDG_CONFIG_HOME".to_owned(), dir("config")),
        ("XDG_DATA_HOME".to_owned(), dir("data")),
        ("XDG_STATE_HOME".to_owned(), dir("state")),
        (
            "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
            password.to_owned(),
        ),
        ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
    ];
    if let Some(token) = &provider.token {
        env.push(("KEEPER_AGENTD_SECRET_PROVIDER".to_owned(), token.clone()));
    }
    let host = Host {
        env,
        root,
        child: None,
    };
    let login = host
        .command(&[
            "login",
            &format!("smoke/{agent}"),
            "--password-credential",
            "agent_password",
        ])
        .output()
        .expect("login");
    assert!(
        login.status.success(),
        "login: {}{}",
        String::from_utf8_lossy(&login.stdout),
        String::from_utf8_lossy(&login.stderr)
    );
    host
}

fn soul(name: &str) -> String {
    format!("---\nname: {name}\ntitle: a smoke agent\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test agent.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\n{name} answers.\n")
}

/// Dr Tola Grey's `agent.toml` as the seed writes it for the drive `smoke`
/// — the steward kind, no `[tools].allow`, 91.5's menu — signed in as
/// `user` and run on `bot`.
fn seeded_tola(person: &OwnedUserId, user: &OwnedUserId, bot: &str) -> String {
    let choices = SeedChoices::new(
        "smoke",
        "tgorka",
        person.as_str(),
        &[person.to_string()],
        false,
        Some(bot),
        &CATALOGUE
            .iter()
            .map(|a| a.id.to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("choices");
    let text = seed::files(&choices)
        .into_iter()
        .find(|file| file.path.ends_with("tola-grey/agent.toml"))
        .expect("Tola's home")
        .text;
    text.lines()
        .map(|line| {
            if line.split('=').next().map(str::trim) == Some("matrix_user") {
                format!("matrix_user = \"{user}\"")
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// A model on a local port whose every answer `decide` makes from the
/// request's messages.
fn model(decide: fn(&[Value]) -> Value) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else { continue };
            std::thread::spawn(move || {
                let mut reader = BufReader::new(socket.try_clone().expect("clone"));
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).is_err() || line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                if !first.contains("/chat/completions") {
                    let _ = write!(
                        socket,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    );
                    return;
                }
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let messages = request["messages"].as_array().cloned().unwrap_or_default();
                let frame = decide(&messages);
                let _ = write!(socket, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n");
                let _ = write!(socket, "data: {frame}\n\ndata: [DONE]\n\n");
            });
        }
    });
    url
}

fn text(message: &Value) -> &str {
    message["content"].as_str().unwrap_or_default()
}

/// One tool call, under an id no other call of the run has.
fn call(name: &str, args: Value) -> Value {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = format!("call_{}", NEXT.fetch_add(1, Ordering::Relaxed));
    json!({"model":"stub","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}]})
}

fn prose(answer: &str) -> Value {
    json!({"model":"stub","choices":[{"index":0,"delta":{"content":answer},"finish_reason":"stop"}]})
}

/// The calls of the turn under way — everything after its brief, the newest
/// user message — each with its arguments and its result, in order.
fn turn_calls(messages: &[Value]) -> Vec<(String, Value, String)> {
    let start = messages
        .iter()
        .rposition(|m| m["role"] == "user")
        .map_or(0, |at| at + 1);
    let mut calls = Vec::new();
    for message in &messages[start..] {
        for tool_call in message["tool_calls"].as_array().into_iter().flatten() {
            let id = tool_call["id"].as_str().unwrap_or_default().to_owned();
            let name = tool_call["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
            let args = tool_call["function"]["arguments"]
                .as_str()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or(Value::Null);
            let result = messages[start..]
                .iter()
                .find(|m| m["role"] == "tool" && m["tool_call_id"] == id.as_str())
                .map(|m| text(m).to_owned())
                .unwrap_or_default();
            calls.push((name, args, result));
        }
    }
    calls
}

/// The steward's model. In her triage turn — its brief is her `TR`+`DS`
/// card — it lists `00-inbox` with `drive_list`, reads every letter there
/// with `drive_read`, and writes one card per letter with `session_write`,
/// asked by the letter's sender. A letter asking for something to run on a
/// schedule gets the schedule asked for with `card_update`; any other is
/// handed to Nixi with `delegate`, the card as its source. Every other turn
/// (her harvest's, a reply's) only answers.
fn stewards_model(messages: &[Value]) -> Value {
    let triage = messages
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .is_some_and(|m| text(m).contains("Triage what came in"));
    if !triage {
        return prose("Done.");
    }
    let done = turn_calls(messages);
    let made = |name: &str, key: &str, value: &str| {
        done.iter()
            .any(|(n, args, _)| n == name && args[key].as_str() == Some(value))
    };
    let Some((_, _, listed)) = done.iter().find(|(name, _, _)| name == "drive_list") else {
        return call("drive_list", json!({"path": "00-inbox"}));
    };
    let letters: Vec<String> = listed
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_'))
        .filter(|word| word.ends_with(".md"))
        .map(str::to_owned)
        .collect();
    for letter in &letters {
        let path = format!("00-inbox/{letter}");
        if !made("drive_read", "path", &path) {
            return call("drive_read", json!({ "path": path }));
        }
    }
    for (name, args, read) in &done {
        if name != "drive_read" {
            continue;
        }
        let card = args["path"]
            .as_str()
            .and_then(|path| path.rsplit('/').next())
            .unwrap_or_default()
            .to_owned();
        let sender = read
            .lines()
            .find_map(|line| line.trim().strip_prefix("From: "))
            .unwrap_or("@tgorka-smoke")
            .trim()
            .to_owned();
        let ask = read
            .lines()
            .map(str::trim)
            .rfind(|line| line.starts_with("Please"))
            .unwrap_or_default()
            .to_owned();
        if !made("session_write", "path", &card) {
            return call(
                "session_write",
                json!({"path": card, "content": format!("---\ntags: [task]\ntitle: {card}\nstatus: todo\nassignee: nixi\nrequested_by: \"{sender}\"\n---\n\n{ask}\n")}),
            );
        }
        if ask.contains("schedule") {
            if !made("card_update", "card", &card) {
                return call(
                    "card_update",
                    json!({"card": card, "fields": {"schedule": "@daily"}}),
                );
            }
        } else if !made("delegate", "source", &card) {
            return call(
                "delegate",
                json!({"agent": "smoke/nixi", "brief": ask, "card": {"title": card}, "source": card}),
            );
        }
    }
    prose("Done.")
}

/// Nixi's model: every brief is answered with `reply`.
fn nixis_model(messages: &[Value]) -> Value {
    match messages.last() {
        Some(last) if last["role"] == "tool" => prose("Replied."),
        _ => call("reply", json!({"text": "The inbox is sorted."})),
    }
}

/// The session folder of `host` whose `agent.toml` has `id`.
fn session_with(host: &Host, id: &str) -> Vec<PathBuf> {
    host.sessions()
        .into_iter()
        .filter(|dir| {
            std::fs::read_to_string(dir.join("agent.toml"))
                .ok()
                .and_then(|text| parse_session_agent_toml(&text).ok())
                .is_some_and(|agent| agent.id.to_string() == id)
        })
        .collect()
}

/// The two hosts — Nixi's on her stub, Tola's on `tolas` — over one drive
/// whose inbox holds [`SUMMARY`] and [`STANDUP`], with the principal's
/// control room made by Nixi's user, Tola among its agents at 50 and
/// invited.
struct Scene {
    _scratch: tempfile::TempDir,
    tola: OwnedUserId,
    control: OwnedRoomId,
    nixis_host: Host,
    tolas_host: Host,
    triage_id: String,
    harvest_id: String,
}

async fn scene(smoke: &Smoke, tolas: Provider, tolas_bot: &str) -> Scene {
    let scratch = tempfile::tempdir().expect("tempdir");
    let person = smoke.user("tgorka-smoke");
    let nixi = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let tola_password = ulid::Ulid::new().to_string();
    smoke
        .admin(
            reqwest::Method::PUT,
            &format!("/_synapse/admin/v2/users/{tola}"),
            Some(json!({ "password": tola_password, "admin": false })),
        )
        .await;
    for agent in [&nixi, &tola] {
        smoke.clear_devices(agent).await;
        smoke.set_ratelimit(agent, 0, 0).await;
    }
    let maker = smoke
        .client(
            &scratch.path().join("maker"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    let control = maker
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![person.clone(), tola.clone()],
            std::slice::from_ref(&tola),
        )
        .await
        .expect("control room");
    let nixis = Provider {
        base_url: model(nixis_model),
        token: None,
    };

    let drive_toml = format!(
        "version = 1\nid = \"smoke\"\ntitle = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    );
    let letter = |text: &str| text.replace("@tgorka-smoke", person.as_str());
    let files = vec![
        ("80-agents/_drive.toml".to_owned(), drive_toml),
        (
            "80-agents/nixi/agent.toml".to_owned(),
            format!(
                "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"specialist\"\nmatrix_user = \"{nixi}\"\n\n[model]\nbot = \"bot:openai:{}#stub\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"smoke\"]\n",
                nixis.base_url
            ),
        ),
        ("80-agents/nixi/SOUL.md".to_owned(), soul("Nixi")),
        (
            "80-agents/tola-grey/agent.toml".to_owned(),
            seeded_tola(&person, &tola, tolas_bot),
        ),
        ("80-agents/tola-grey/SOUL.md".to_owned(), soul("Dr Tola Grey")),
        ("00-inbox/summary.md".to_owned(), letter(SUMMARY)),
        ("00-inbox/standup.md".to_owned(), letter(STANDUP)),
    ];
    let bare = bare_drive(scratch.path(), &files);
    let shared = Shared {
        bare: &bare,
        person: &person,
        control: &control,
    };
    let nixis_host = host(
        smoke,
        "smoke-a",
        "nixi",
        smoke.secret("NIXI_SMOKE_PASSWORD"),
        &nixis,
        &shared,
    );
    let tolas_host = host(
        smoke,
        "smoke-b",
        "tola-grey",
        &tola_password,
        &tolas,
        &shared,
    );
    Scene {
        _scratch: scratch,
        tola,
        control,
        nixis_host,
        tolas_host,
        triage_id: steward_session_id("smoke", "tola-grey", "triage").to_string(),
        harvest_id: steward_session_id("smoke", "tola-grey", "harvest").to_string(),
    }
}

/// Wait up to `within` for `ready` to find a session folder of Tola's.
async fn wait_for_session(
    scene: &Scene,
    id: &str,
    within: Duration,
    what: &str,
    ready: impl Fn(&Path) -> bool,
) -> PathBuf {
    let deadline = Instant::now() + within;
    loop {
        if let Some(dir) = session_with(&scene.tolas_host, id)
            .into_iter()
            .find(|d| ready(d))
        {
            return dir;
        }
        assert!(
            Instant::now() < deadline,
            "{what}.\nher session's calls and cards:\n{:#?}\ntola's host:\n{}\nnixi's host:\n{}",
            session_with(&scene.tolas_host, id)
                .iter()
                .map(|dir| (calls_of(&Host::lines(dir)), cards(dir)))
                .collect::<Vec<_>>(),
            scene.tolas_host.log_text(),
            scene.nixis_host.log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Each steward session's folder names a scheduled session room Tola made,
/// and the control room records that room under the session's id beside
/// the released creation claim (R165): the rooms, by session id.
async fn assert_made_once(smoke: &Smoke, scene: &Scene) -> Vec<(String, String)> {
    let control = smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{}/state", scene.control),
            None,
        )
        .await;
    let control = control["state"].as_array().cloned().expect("state");
    let mut rooms = Vec::new();
    for id in [&scene.triage_id, &scene.harvest_id] {
        let dirs = session_with(&scene.tolas_host, id);
        assert_eq!(dirs.len(), 1, "{id}: {:?}", scene.tolas_host.sessions());
        let agent = parse_session_agent_toml(
            &std::fs::read_to_string(dirs[0].join("agent.toml")).expect("agent.toml"),
        )
        .expect("parse");
        let state = smoke
            .admin(
                reqwest::Method::GET,
                &format!("/_synapse/admin/v1/rooms/{}/state", agent.room),
                None,
            )
            .await;
        let create = state["state"]
            .as_array()
            .expect("state")
            .iter()
            .find(|e| e["type"] == "m.room.create")
            .cloned()
            .expect("created");
        assert_eq!(create["content"]["type"], "dev.keeper.agent.session");
        assert_eq!(create["sender"], scene.tola.as_str());
        let keyed = |kind: &str| {
            control
                .iter()
                .find(|e| e["type"] == kind && e["state_key"] == id.as_str())
                .cloned()
                .unwrap_or_else(|| panic!("no {kind} for {id}"))
        };
        assert_eq!(keyed(STEWARD_ROOM)["content"]["room"], agent.room.as_str());
        assert_eq!(keyed(CLAIM)["content"]["released"], true);
        rooms.push((id.clone(), agent.room.to_string()));
    }
    rooms
}

/// How many rooms Tola's user is in.
async fn tolas_rooms(smoke: &Smoke, tola: &OwnedUserId) -> usize {
    smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/users/{tola}/joined_rooms"),
            None,
        )
        .await["joined_rooms"]
        .as_array()
        .map_or(0, Vec::len)
}

/// Every tool call of `lines`, its tool, its arguments and its result.
fn calls_of(lines: &[Value]) -> Vec<(String, String, Option<Value>)> {
    lines
        .iter()
        .filter(|l| l["kind"] == "tool_call")
        .map(|l| {
            let id = &l["body"]["call_id"];
            let result = lines
                .iter()
                .find(|r| r["kind"] == "tool_result" && r["body"]["call_id"] == *id)
                .map(|r| r["body"].clone());
            (
                l["body"]["tool"].as_str().unwrap_or_default().to_owned(),
                l["body"]["args"].as_str().unwrap_or_default().to_owned(),
                result,
            )
        })
        .collect()
}

/// The cards of a session folder other than its own scheduled card.
fn cards(dir: &Path) -> Vec<(String, String)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(entry.path()).ok()?;
            (name.ends_with(".md") && name != "triage.md" && name != "README.md")
                .then_some((name, text))
        })
        .filter(|(_, text)| text.starts_with("---") && text.contains("task"))
        .collect()
}

/// No schedule ran: a card of the session that carries one was given it by
/// her, so it is marked `scheduled_by`, has no person's `allowed_by` and
/// never ran (Q16); every call that asked for one on a card was refused.
fn assert_no_schedule(triage: &Path) {
    for (name, text) in cards(triage) {
        if text.contains("schedule:") {
            assert!(text.contains("scheduled_by:"), "{name}: {text}");
            assert!(!text.contains("allowed_by:"), "{name}: {text}");
            assert!(!text.contains("run:"), "{name}: {text}");
        }
    }
    for (tool, args, result) in calls_of(&Host::lines(triage)) {
        let args: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let asked = match tool.as_str() {
            "card_update" => !args["fields"]["schedule"].is_null(),
            "delegate" => !args["card"]["schedule"].is_null(),
            _ => false,
        };
        if asked {
            let result = result.expect("a result");
            assert_eq!(result["outcome"], "refused", "{tool} {args}: {result}");
        }
    }
}

/// 92.5 acceptance 7 (stub model), R165 and R66: Dr Tola Grey's host makes
/// her triage and harvest sessions at start — each a scheduled session room
/// recorded under its id in the control room and a folder holding her
/// `@daily` card — and her triage card runs at once: she lists and reads
/// the inbox, writes a card per letter, hands the safe one to Nixi with the
/// card as its source, and the schedule the other asks for is refused.
/// Started again, her host makes nothing new.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_stewards_triage_runs_on_a_real_homeserver() {
    let smoke = Smoke::from_env();
    let tolas = Provider {
        base_url: model(stewards_model),
        token: None,
    };
    let bot = format!("bot:openai:{}#stub", tolas.base_url);
    let mut scene = scene(&smoke, tolas, &bot).await;
    scene.nixis_host.start();
    scene.tolas_host.start();

    let triage_id = scene.triage_id.clone();
    let triage = wait_for_session(
        &scene,
        &triage_id,
        Duration::from_secs(240),
        "no reply logged in her triage session",
        |dir| {
            Host::lines(dir)
                .iter()
                .any(|l| l["kind"] == "delegate" && l["body"]["state"] == "replied")
        },
    )
    .await;
    let rooms = assert_made_once(&smoke, &scene).await;

    // TR read the inbox and wrote a card per letter; DS handed the safe
    // one on as its source; the schedule was a person's to give.
    let calls = calls_of(&Host::lines(&triage));
    let tools: Vec<&str> = calls.iter().map(|(tool, _, _)| tool.as_str()).collect();
    assert_eq!(tools.first(), Some(&"drive_list"), "{tools:?}");
    for (tool, times) in [
        ("drive_read", 2),
        ("session_write", 2),
        ("delegate", 1),
        ("card_update", 1),
    ] {
        assert_eq!(
            tools.iter().filter(|t| **t == tool).count(),
            times,
            "{tool}: {tools:?}"
        );
    }
    let (_, _, refused) = calls
        .iter()
        .find(|(tool, _, _)| tool == "card_update")
        .expect("card_update");
    let refused = refused.as_ref().expect("its result");
    assert_eq!(refused["outcome"], "refused");
    assert_eq!(refused["content"], format!("Refused: {UNATTENDED_REFUSAL}"));
    let summary = std::fs::read_to_string(triage.join("summary.md")).expect("summary card");
    assert!(summary.contains("run: review"), "{summary}");
    assert_no_schedule(&triage);
    let triage_card = std::fs::read_to_string(triage.join("triage.md")).expect("triage card");
    assert!(
        triage_card.contains("schedule: \"@daily\""),
        "{triage_card}"
    );
    assert!(!triage_card.contains("scheduled_by"), "{triage_card}");

    // Started again: the same two sessions and rooms, nothing made.
    let joined = tolas_rooms(&smoke, &scene.tola).await;
    scene.tolas_host.stop();
    scene.tolas_host.start();
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(assert_made_once(&smoke, &scene).await, rooms);
    assert_eq!(tolas_rooms(&smoke, &scene.tola).await, joined);
    scene.nixis_host.stop();
    scene.tolas_host.stop();
}

/// 92.5 acceptance 7 with a real model (STW-11, R13): her triage card runs
/// on CLIProxyAPI. A model's words vary, so only the protocol is held: she
/// wrote a card, opened a delegation with a card as its source, and no
/// schedule ran.
#[ignore = "live: Synapse on delectra + CLIProxyAPI"]
#[tokio::test(flavor = "multi_thread")]
async fn a_stewards_triage_runs_on_a_real_model() {
    let smoke = Smoke::from_env();
    let base_url =
        std::env::var("KEEPER_OPENAI_SMOKE_BASE_URL").expect("KEEPER_OPENAI_SMOKE_BASE_URL");
    let token_file =
        std::env::var("KEEPER_OPENAI_SMOKE_TOKEN_FILE").expect("KEEPER_OPENAI_SMOKE_TOKEN_FILE");
    let token = std::fs::read_to_string(token_file)
        .expect("token")
        .trim()
        .to_owned();
    let target = std::env::var("KEEPER_OPENAI_SMOKE_MODEL").expect("KEEPER_OPENAI_SMOKE_MODEL");
    let bot = format!("bot:openai:{base_url}#{target}");
    let tolas = Provider {
        base_url,
        token: Some(token),
    };
    let mut scene = scene(&smoke, tolas, &bot).await;
    scene.nixis_host.start();
    scene.tolas_host.start();

    let triage_id = scene.triage_id.clone();
    let triage = wait_for_session(
        &scene,
        &triage_id,
        Duration::from_secs(300),
        "no card handed on as a delegation's source in her triage session",
        |dir| {
            cards(dir)
                .iter()
                .any(|(_, text)| text.contains("run: running") || text.contains("run: review"))
        },
    )
    .await;
    assert_made_once(&smoke, &scene).await;
    let lines = Host::lines(&triage);
    let opened = lines.iter().any(|l| {
        l["kind"] == "delegate" && l["body"]["state"] != "refused" && l["body"]["room"].is_string()
    });
    assert!(opened, "{lines:#?}");
    let calls = calls_of(&lines);
    let sourced = calls.iter().any(|(tool, args, result)| {
        tool == "delegate"
            && serde_json::from_str::<Value>(args)
                .is_ok_and(|args| args["source"].as_str().is_some_and(|s| !s.is_empty()))
            && result
                .as_ref()
                .is_some_and(|result| result["outcome"] == "ok")
    });
    assert!(sourced, "{calls:#?}");
    // Let the turn end before the last look at what it set.
    tokio::time::sleep(Duration::from_secs(30)).await;
    assert_no_schedule(&triage);
    let scheduled: Vec<String> = scene
        .tolas_host
        .sessions()
        .iter()
        .filter_map(|dir| std::fs::read_to_string(dir.join("agent.toml")).ok())
        .filter_map(|text| parse_session_agent_toml(&text).ok())
        .filter(|agent| agent.kind == SessionKind::Scheduled)
        .map(|agent| agent.id.to_string())
        .collect();
    for id in &scheduled {
        assert!(
            *id == scene.triage_id || *id == scene.harvest_id,
            "a scheduled session nobody allowed: {id}"
        );
    }
    scene.nixis_host.stop();
    scene.tolas_host.stop();
}
