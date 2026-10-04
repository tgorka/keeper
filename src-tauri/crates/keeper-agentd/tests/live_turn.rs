//! `keeper-agentd run` against a real homeserver, a real git remote and a
//! model (story 90.5, acceptance 13, 14, 17 live, 21).
//!
//! Every test is `#[ignore]`: it needs the Synapse test homeserver and its
//! users. Endpoints and secrets come from the environment, never from this
//! repository (S-20):
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! KEEPER_OPENAI_SMOKE_BASE_URL=<CLIProxyAPI's base URL> \
//! KEEPER_OPENAI_SMOKE_TOKEN_FILE=$HOME/.omp/cliproxyapi.token \
//! KEEPER_OPENAI_SMOKE_MODEL=<a model it serves> \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_turn -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The secrets file holds `SERVER_NAME`, `ADMIN_TOKEN`,
//! `NIXI_SMOKE_PASSWORD`, `NIXI_PACED_PASSWORD` and `TGORKA_SMOKE_PASSWORD`.
//! The admin token clears the agent users' devices, sets their rate limits
//! through Synapse's `override_ratelimit` (ruling D2) and makes the stranger
//! of the invite test; the server's configuration is never changed.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::account::spoken_send;
use keeper_core::agents::events;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError, RoomKind};
use keeper_core::agents::proxy::{AgentProxies, ProxyFacts, ProxyListEventContent};
use keeper_core::agents::room::AgentKinds;
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use keeper_core::agents::spoken::SpokenStep;
use keeper_core::error::CoreError;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, git, record, stub, syncing, Smoke, Stub};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const SESSION: &str = "active/2026-10-03-smoke";
const LURE: &str = "active/2026-10-03-lure";
const HOST: &str = "smoke";

/// The model an agent is given: the stub, or CLIProxyAPI from the
/// environment with its token from a file.
struct Model {
    base_url: String,
    target: String,
    token: Option<String>,
}

struct Host {
    root: tempfile::TempDir,
    bare: PathBuf,
    room: OwnedRoomId,
    env: Vec<(String, String)>,
    child: Option<Child>,
    log: PathBuf,
}

impl Host {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command.args(args);
        command.env_clear();
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    fn start(&mut self) {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log)
            .expect("log");
        let child = self
            .command(&["run"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("keeper-agentd run");
        self.child = Some(child);
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn pid(&self) -> u32 {
        self.child.as_ref().expect("running").id()
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// Wait until the running host's status names `user`'s copy in the room.
    async fn wait_serving(&self, room: &OwnedRoomId) {
        let status = self.root.path().join("state/keeper-agentd/status.json");
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            if let Ok(text) = std::fs::read_to_string(&status) {
                if text.contains(room.as_str()) {
                    return;
                }
            }
            assert!(
                Instant::now() < deadline,
                "the host never served {room}:\n{}",
                self.log_text()
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A drive holding a proxy for `person` whose Matrix user is `agent`, one
/// `main` session in a room the harness creates as the agent, and an
/// `agentd.toml` pointing at it; the copy signed in with `login`. With
/// `lure`, a second session names that room, which the agent never joined.
async fn host(
    smoke: &Smoke,
    agent: &str,
    password_key: &str,
    model: &Model,
    lure: Option<&OwnedRoomId>,
) -> Host {
    let root = tempfile::tempdir().expect("tempdir");
    let agent_user = smoke.user(agent);
    let person = smoke.user("tgorka-smoke");
    smoke.clear_devices(&agent_user).await;

    // The room, created as the agent from a harness device (power 100).
    let maker = smoke
        .client(
            &root.path().join("maker"),
            agent,
            smoke.secret(password_key),
        )
        .await;
    let room = maker
        .create_room(
            RoomKind::Session(SessionKind::Main),
            "smoke",
            vec![person.clone()],
            &[],
        )
        .await
        .expect("room");

    let readers = [person.clone()].into_iter().collect();
    let drive_toml = format!(
        "version = 1\nid = \"smoke\"\ntitle = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    );
    let decl = keeper_core::agents::drive::parse(&drive_toml).expect("decl");
    let session = SessionAgent {
        id: ulid::Ulid::new(),
        agent: "nixi".to_owned(),
        drive: "smoke".to_owned(),
        kind: SessionKind::Main,
        title: "smoke".to_owned(),
        requested_by: person.clone(),
        parent: None,
        room: room.clone(),
        drives: vec!["smoke".to_owned()],
        label: Label {
            readers: Readers::Only(readers),
            ..Label::opening(&decl, Integrity::Owner)
        },
        needs: None,
        pin: None,
        hop: 0,
        limits: None,
        workflow: None,
        created_at: chrono::Utc::now(),
    };
    let soul = "---\nname: Nixi\ntitle: the smoke proxy\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test proxy.\"\ncommunication_style: Short.\nprinciples:\n  - Answer from the drive.\n---\n\nNixi answers from the drive.\n";
    let mut files = vec![
        ("80-agents/_drive.toml".to_owned(), drive_toml.clone()),
        (
            "80-agents/nixi/agent.toml".to_owned(),
            format!(
                "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"{agent_user}\"\nhuman = \"{person}\"\n\n[model]\nbot = \"bot:openai:{}#{}\"\n\n[tools]\nallow = [\"drive_list\", \"drive_read\", \"drive_grep\", \"drive_stat\"]\ndrives = [\"smoke\"]\n",
                model.base_url, model.target
            ),
        ),
        ("80-agents/nixi/SOUL.md".to_owned(), soul.to_owned()),
        ("notes/hello.md".to_owned(), "The first line is: keeper says hello.\nA second line.\n".to_owned()),
        (
            format!("60-sessions/{SESSION}/agent.toml"),
            compose_session_agent_toml(&session),
        ),
    ];
    if let Some(lure) = lure {
        let named = SessionAgent {
            id: ulid::Ulid::new(),
            room: lure.clone(),
            ..session.clone()
        };
        files.push((
            format!("60-sessions/{LURE}/agent.toml"),
            compose_session_agent_toml(&named),
        ));
    }
    let bare = bare_drive(root.path(), &files);

    let provider_credential = if model.token.is_some() {
        "credential = \"secret:provider\"\n"
    } else {
        ""
    };
    let config = format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"{HOST}\"\n\n[homeserver]\nurl = \"{}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{}\"\n{provider_credential}\n[[agents]]\ndrive = \"smoke\"\nids = [\"nixi\"]\n",
        smoke.homeserver,
        bare.display(),
        model.base_url
    );
    let config_dir = root.path().join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("agentd.toml"), config).expect("config");

    let mut env = vec![
        ("HOME".to_owned(), root.path().display().to_string()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            root.path().join("config").display().to_string(),
        ),
        (
            "XDG_DATA_HOME".to_owned(),
            root.path().join("data").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".to_owned(),
            root.path().join("state").display().to_string(),
        ),
        (
            "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
            smoke.secret(password_key).to_owned(),
        ),
        ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
    ];
    if let Some(token) = &model.token {
        env.push(("KEEPER_AGENTD_SECRET_PROVIDER".to_owned(), token.clone()));
    }
    let host = Host {
        log: root.path().join("agentd.log"),
        root,
        bare,
        room,
        env,
        child: None,
    };
    let login = host
        .command(&[
            "login",
            "smoke/nixi",
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

/// The person's client, joined to the session room and syncing.
async fn person(
    smoke: &Smoke,
    dir: &Path,
    room: &OwnedRoomId,
) -> (
    AgentClient,
    Arc<Mutex<Vec<(Instant, Value)>>>,
    tokio::task::JoinHandle<()>,
) {
    let me = smoke.user("tgorka-smoke");
    smoke.clear_devices(&me).await;
    let client = smoke
        .client(dir, "tgorka-smoke", smoke.secret("TGORKA_SMOKE_PASSWORD"))
        .await;
    client.join(room).await.expect("join");
    client.sync_once().await.expect("sync");
    let seen = record(&client);
    let sync = syncing(&client);
    (client, seen, sync)
}

/// One answered turn as the person saw it.
#[derive(Debug)]
struct Seen {
    /// Every edit's `origin_server_ts`, the anchor's first, the final last.
    times: Vec<u64>,
    final_text: String,
}

/// Ask `question` and wait for the final edit whose text `done` accepts.
async fn ask(
    client: &AgentClient,
    seen: &Arc<Mutex<Vec<(Instant, Value)>>>,
    room: &OwnedRoomId,
    agent: &OwnedUserId,
    question: &str,
    done: &dyn Fn(&str) -> bool,
) -> Seen {
    let sent = say(client, room, question).await;
    answer_to(seen, agent, &sent, question, done).await
}

/// Send `text` as the person.
async fn say(client: &AgentClient, room: &OwnedRoomId, text: &str) -> String {
    client
        .send(
            room,
            "m.room.message",
            json!({"msgtype":"m.text","body":text}),
            None,
        )
        .await
        .expect("ask")
        .to_string()
}

/// Wait for the agent's final edit, answering the event `sent`, whose text
/// `done` accepts.
async fn answer_to(
    seen: &Arc<Mutex<Vec<(Instant, Value)>>>,
    agent: &OwnedUserId,
    sent: &str,
    question: &str,
    done: &dyn Fn(&str) -> bool,
) -> Seen {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let events: Vec<Value> = seen
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        let anchor = events.iter().find(|e| {
            e["sender"] == agent.as_str()
                && e["content"][events::TURN].is_object()
                && events.iter().any(|q| q["event_id"] == sent)
                && e["origin_server_ts"].as_u64()
                    >= events
                        .iter()
                        .find(|q| q["event_id"] == sent)
                        .and_then(|q| q["origin_server_ts"].as_u64())
        });
        if let Some(anchor) = anchor {
            let anchor_id = anchor["event_id"].as_str().expect("id").to_owned();
            let edits: Vec<&Value> = events
                .iter()
                .filter(|e| e["content"]["m.relates_to"]["event_id"] == anchor_id.as_str())
                .collect();
            if let Some(last) = edits.last() {
                let text = last["content"]["m.new_content"]["body"]
                    .as_str()
                    .unwrap_or_default();
                if done(text) {
                    let mut times = vec![anchor["origin_server_ts"].as_u64().expect("ts")];
                    times.extend(edits.iter().filter_map(|e| e["origin_server_ts"].as_u64()));
                    return Seen {
                        times,
                        final_text: text.to_owned(),
                    };
                }
            }
        }
        assert!(Instant::now() < deadline, "no answer to {question:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `(anchor_ms, final_ms)` of every answered turn in the host's log.
fn timings(log: &str) -> Vec<(u64, u64)> {
    log.lines()
        .filter(|line| line.contains("a turn was answered"))
        .filter_map(|line| {
            let field = |name: &str| {
                line.split_whitespace()
                    .find_map(|word| word.strip_prefix(&format!("{name}=")))
                    .and_then(|v| v.parse::<u64>().ok())
            };
            Some((field("anchor_ms")?, field("final_ms")?))
        })
        .collect()
}

fn p95(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    let at = ((values.len() as f64) * 0.95).ceil() as usize;
    values[at.saturating_sub(1).min(values.len() - 1)]
}

fn assert_paced(seen: &Seen) {
    for pair in seen.times.windows(2) {
        assert!(
            pair[1] - pair[0] >= 400,
            "edits {} ms apart: {:?}",
            pair[1] - pair[0],
            seen.times
        );
    }
}

const ANSWER: &str = "The answer is forty-two, as it always was, and it arrives in pieces so the edits have something to carry. Here is the rest of it, a little longer, so the stream lasts the whole two seconds.";

/// NFR-113 on Synapse: over 50 turns with the stub streaming a fixed answer
/// over 2 s, the anchor is accepted within 1 s of the request reaching the
/// host and the final edit within 1 s of the stream's end (p95, the host's
/// monotonic clock), and edits stay ≥ 400 ms apart by `origin_server_ts`.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn nfr_113_holds_over_fifty_turns() {
    let smoke = Smoke::from_env();
    let stub = stub(ANSWER, 20, Duration::from_secs(2));
    let model = Model {
        base_url: stub.url.clone(),
        target: "stub".to_owned(),
        token: None,
    };
    let agent = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let mut host = host(&smoke, "nixi-smoke", "NIXI_SMOKE_PASSWORD", &model, None).await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let dir = host.root.path().join("person");
    let (client, seen, _sync) = person(&smoke, &dir, &host.room).await;

    for turn in 0..50 {
        let answer = ask(
            &client,
            &seen,
            &host.room,
            &agent,
            &format!("turn {turn}"),
            &|t| t == ANSWER,
        )
        .await;
        assert_paced(&answer);
    }
    let measured = timings(&host.log_text());
    assert!(measured.len() >= 50, "{} turns logged", measured.len());
    let anchor = p95(measured.iter().map(|m| m.0).collect());
    let last = p95(measured.iter().map(|m| m.1).collect());
    println!(
        "NFR-113 on Synapse over {} turns: p95 anchor {anchor} ms, p95 final edit {last} ms",
        measured.len()
    );
    assert!(anchor <= 1000, "p95 anchor {anchor} ms");
    assert!(last <= 1000, "p95 final edit {last} ms");
    assert_eq!(*stub.requests.lock().expect("lock"), 50);
}

/// Acceptance 13 with CLIProxyAPI: a real model reads a drive file through
/// the host and quotes it; the log holds the turn and reaches the remote; a
/// `kill -9` and a restart answer nothing twice; the host talks only to the
/// homeserver and the provider.
#[ignore = "live: Synapse on delectra + CLIProxyAPI"]
#[tokio::test(flavor = "multi_thread")]
async fn a_turn_with_cliproxyapi_reads_the_drive_and_lands_in_the_log() {
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
    let model = Model {
        base_url: base_url.clone(),
        target,
        token: Some(token),
    };
    let agent = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let mut host = host(&smoke, "nixi-smoke", "NIXI_SMOKE_PASSWORD", &model, None).await;
    // Another non-dumpable process of this user's would be counted as
    // agentd's: what such processes hold before agentd starts is not.
    let before = connections_of_non_dumpable_processes();
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let dir = host.root.path().join("person");
    let (client, seen, _sync) = person(&smoke, &dir, &host.room).await;

    // Outbound connections while it runs.
    let pid = host.pid();
    let answer = ask(
        &client,
        &seen,
        &host.room,
        &agent,
        "Read notes/hello.md in the smoke drive and tell me its first line.",
        &|t| t.contains("keeper says hello"),
    )
    .await;
    assert_paced(&answer);
    let allowed: Vec<std::net::SocketAddr> = [&smoke.homeserver, &base_url]
        .iter()
        .filter_map(|url| url.split_once("://"))
        .map(|(scheme, rest)| {
            (
                scheme,
                rest.split('/').next().unwrap_or_default().to_owned(),
            )
        })
        .flat_map(|(scheme, authority)| {
            use std::net::ToSocketAddrs;
            let with_port = if authority.contains(':') {
                authority
            } else if scheme == "http" {
                format!("{authority}:80")
            } else {
                format!("{authority}:443")
            };
            with_port
                .to_socket_addrs()
                .map(|a| a.collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    let reached: Vec<std::net::SocketAddr> = connections_of_non_dumpable_processes()
        .into_iter()
        .filter(|peer| !before.contains(peer))
        .collect();
    assert!(
        !reached.is_empty(),
        "agentd's connections were not found (pid {pid})"
    );
    for peer in &reached {
        println!("agentd connection: {peer}");
        assert!(
            allowed.contains(peer),
            "agentd reached {peer}, which agentd.toml does not name ({allowed:?})"
        );
    }
    println!("answer: {}", answer.final_text);

    // The log, then the remote.
    let chunk_dir = host.root.path().join(format!(
        "data/keeper-agentd/drives/smoke/60-sessions/{SESSION}/log"
    ));
    let chunk = std::fs::read_dir(&chunk_dir)
        .expect("log dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .find(|name| name.ends_with(&format!(".{HOST}.1.jsonl")))
        .expect("this host's chunk");
    let text = std::fs::read_to_string(chunk_dir.join(&chunk)).expect("chunk");
    for kind in [
        "\"kind\":\"user\"",
        "\"kind\":\"assistant\"",
        "\"kind\":\"tool_call\"",
        "\"kind\":\"tool_result\"",
    ] {
        assert!(text.contains(kind), "{kind} missing from {chunk}");
    }
    assert!(
        text.contains("\"matrix_event\":\"$"),
        "the user line names its event"
    );
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let names = git(&host.bare, &["log", "--all", "--name-only", "--format="]);
        if names.contains(&chunk) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the chunk never reached the remote"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }

    // kill -9 and a restart: no second answer.
    let anchors_before = anchors(&seen, &agent);
    host.kill();
    host.start();
    host.wait_serving(&host.room.clone()).await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert_eq!(
        anchors(&seen, &agent),
        anchors_before,
        "a restart answered again"
    );
}

/// The remote ends of this user's established TCP connections that no
/// process it can inspect owns. agentd makes itself non-dumpable (S-07), so
/// its `/proc/<pid>/fd` is closed to its own user — `ss -p` cannot name it.
/// Another non-dumpable process of the user's would show here too, so the
/// caller drops what was here before agentd started.
fn connections_of_non_dumpable_processes() -> Vec<std::net::SocketAddr> {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;

    let uid = std::fs::metadata("/proc/self").expect("self").uid();
    let mut owned: HashSet<u64> = HashSet::new();
    for entry in std::fs::read_dir("/proc")
        .expect("proc")
        .filter_map(Result::ok)
    {
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.filter_map(Result::ok) {
            if let Ok(target) = std::fs::read_link(fd.path()) {
                let text = target.to_string_lossy();
                if let Some(inode) = text
                    .strip_prefix("socket:[")
                    .and_then(|t| t.strip_suffix(']'))
                {
                    if let Ok(inode) = inode.parse() {
                        owned.insert(inode);
                    }
                }
            }
        }
    }
    let mut peers = Vec::new();
    for (file, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for line in text.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 10 || fields[3] != "01" {
                continue;
            }
            let (Ok(line_uid), Ok(inode)) = (fields[7].parse::<u32>(), fields[9].parse::<u64>())
            else {
                continue;
            };
            if line_uid != uid || owned.contains(&inode) {
                continue;
            }
            if let Some(peer) = parse_proc_addr(fields[2], v6) {
                peers.push(peer);
            }
        }
    }
    peers
}

/// `/proc/net/tcp`'s `HEXIP:HEXPORT`: each 32-bit word of the address is
/// printed as a little-endian host's number.
fn parse_proc_addr(text: &str, v6: bool) -> Option<std::net::SocketAddr> {
    let (ip, port) = text.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let words: Vec<u32> = (0..ip.len() / 8)
        .map(|i| u32::from_str_radix(&ip[i * 8..i * 8 + 8], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let ip: std::net::IpAddr = if v6 {
        let array: [u8; 16] = bytes.try_into().ok()?;
        let v6 = std::net::Ipv6Addr::from(array);
        v6.to_ipv4_mapped()
            .map_or(std::net::IpAddr::V6(v6), std::net::IpAddr::V4)
    } else {
        let array: [u8; 4] = bytes.try_into().ok()?;
        std::net::IpAddr::V4(std::net::Ipv4Addr::from(array))
    };
    Some(std::net::SocketAddr::new(ip, port))
}

fn anchors(seen: &Arc<Mutex<Vec<(Instant, Value)>>>, agent: &OwnedUserId) -> usize {
    seen.lock()
        .expect("lock")
        .iter()
        .filter(|(_, e)| e["sender"] == agent.as_str() && e["content"][events::TURN].is_object())
        .count()
}

/// R18 against a homeserver that asks the sender to wait: at least one 429
/// is met, and the final edit still lands with the whole answer.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_paced_agent_meets_a_429_and_still_lands_the_final_edit() {
    let smoke = Smoke::from_env();
    let stub = stub(ANSWER, 40, Duration::from_secs(4));
    let model = Model {
        base_url: stub.url.clone(),
        target: "stub".to_owned(),
        token: None,
    };
    let agent = smoke.user("nixi-paced");
    smoke.set_ratelimit(&agent, 1, 2).await;
    let mut host = host(&smoke, "nixi-paced", "NIXI_PACED_PASSWORD", &model, None).await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let dir = host.root.path().join("person");
    let (client, seen, _sync) = person(&smoke, &dir, &host.room).await;
    let answer = ask(&client, &seen, &host.room, &agent, "go", &|t| t == ANSWER).await;
    assert_eq!(answer.final_text, ANSWER);
    let log = host.log_text();
    let waits = log
        .lines()
        .filter(|l| l.contains("asked to wait") || l.contains("edits wait for the homeserver"))
        .count();
    println!("nixi-paced met {waits} 429s; the final edit carried the whole answer");
    assert!(waits > 0, "no 429 was met:\n{log}");
}

/// A fresh user of the test homeserver's, signed in under `dir`, and a
/// session-typed room it made inviting `agent`.
async fn stranger_room(
    smoke: &Smoke,
    dir: &Path,
    agent: &OwnedUserId,
) -> (AgentClient, OwnedRoomId) {
    let stranger = format!("stranger-{}", ulid::Ulid::new().to_string().to_lowercase());
    let password = ulid::Ulid::new().to_string();
    smoke
        .admin(
            reqwest::Method::PUT,
            &format!("/_synapse/admin/v2/users/{}", smoke.user(&stranger)),
            Some(json!({ "password": password, "admin": false })),
        )
        .await;
    let client = smoke.client(dir, &stranger, &password).await;
    let room = client
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "lure",
            vec![agent.clone()],
            &[],
        )
        .await
        .expect("room");
    (client, room)
}

/// `user`'s membership of `room`, as the server holds it.
async fn membership(smoke: &Smoke, room: &OwnedRoomId, user: &OwnedUserId) -> Option<String> {
    let state = smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/state"),
            None,
        )
        .await;
    state["state"]
        .as_array()
        .expect("state")
        .iter()
        .find(|e| e["type"] == "m.room.member" && e["state_key"] == user.as_str())
        .map(|e| {
            e["content"]["membership"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
}

fn stub_model(stub: &Stub) -> Model {
    Model {
        base_url: stub.url.clone(),
        target: "stub".to_owned(),
        token: None,
    }
}

/// F5 live: an invite from a user this host does not know stays `invite`
/// after two sync rounds.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn an_invite_from_an_unknown_user_stays_pending() {
    let smoke = Smoke::from_env();
    let stub = stub("ok", 1, Duration::from_millis(10));
    let agent = smoke.user("nixi-smoke");
    let mut host = host(
        &smoke,
        "nixi-smoke",
        "NIXI_SMOKE_PASSWORD",
        &stub_model(&stub),
        None,
    )
    .await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let (_stranger, room) = stranger_room(&smoke, &host.root.path().join("stranger"), &agent).await;
    // Two of the host's sync rounds, with margin.
    tokio::time::sleep(Duration::from_secs(15)).await;
    assert_eq!(
        membership(&smoke, &room, &agent).await.as_deref(),
        Some("invite")
    );
    assert!(host.log_text().contains("an invite stays pending"));
}

/// F5 live: a session file in the home drive — which any reader can write —
/// naming a stranger's room is no reason to join it: only the invite rule
/// joins, and the session is not served.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_session_file_naming_a_strangers_room_does_not_join_it() {
    let smoke = Smoke::from_env();
    let stub = stub("ok", 1, Duration::from_millis(10));
    let agent = smoke.user("nixi-smoke");
    let scratch = tempfile::tempdir().expect("tempdir");
    let (_stranger, lure) = stranger_room(&smoke, scratch.path(), &agent).await;
    let mut host = host(
        &smoke,
        "nixi-smoke",
        "NIXI_SMOKE_PASSWORD",
        &stub_model(&stub),
        Some(&lure),
    )
    .await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    // Three of the host's zone scans, with margin.
    tokio::time::sleep(Duration::from_secs(20)).await;
    assert_eq!(
        membership(&smoke, &lure, &agent).await.as_deref(),
        Some("invite")
    );
    let status = std::fs::read_to_string(host.root.path().join("state/keeper-agentd/status.json"))
        .expect("status");
    assert!(!status.contains(lure.as_str()), "{status}");
}

/// A question the person asked while the host was down is answered once it
/// starts again, whether the restart's first sync hands it over before the
/// session's worker exists (it is kept for the worker) or the worker's read
/// of the room's timeline finds it.
///
/// The host runs once first: a device that never synced has published no
/// keys, so the person's client could not encrypt for it.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_question_asked_while_the_host_is_down_is_answered() {
    let smoke = Smoke::from_env();
    let stub = stub(ANSWER, 5, Duration::from_millis(500));
    let agent = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let mut host = host(
        &smoke,
        "nixi-smoke",
        "NIXI_SMOKE_PASSWORD",
        &stub_model(&stub),
        None,
    )
    .await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let dir = host.root.path().join("person");
    let (client, seen, _sync) = person(&smoke, &dir, &host.room).await;
    host.kill();

    let question = "Are you there?";
    let sent = say(&client, &host.room, question).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    host.start();
    let answer = answer_to(&seen, &agent, &sent, question, &|t| t == ANSWER).await;
    assert_eq!(answer.final_text, ANSWER);
    assert_eq!(anchors(&seen, &agent), 1, "answered once");
}

/// Write the person's `dev.keeper.agent.proxies` list and wait until their
/// syncing client reads it back.
async fn set_proxy_list(client: &AgentClient, agents: &[&OwnedUserId]) {
    let list = agents.iter().map(|agent| (*agent).clone()).collect();
    client
        .client()
        .account()
        .set_account_data(ProxyListEventContent::of(&list))
        .await
        .expect("the list is written");
    proxy_list_becomes(client, &list).await;
}

/// Wait until the person's client reads `list` as their proxy list.
async fn proxy_list_becomes(client: &AgentClient, list: &std::collections::BTreeSet<OwnedUserId>) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let read = client
            .client()
            .account()
            .account_data::<ProxyListEventContent>()
            .await
            .expect("account data")
            .and_then(|raw| raw.deserialize().ok())
            .and_then(|content| content.agents());
        if read.as_ref() == Some(list) {
            return;
        }
        assert!(Instant::now() < deadline, "the list reads {read:?}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Story 91.4, acceptance 6 (AD-384, rulings R31, R72), at the spoken-send
/// boundary (`keeper_core::account::spoken_send`, which the app's
/// `agent_spoken_send` calls): a room whose agent nobody vouched for is
/// refused and nothing is sent; so is one whose agent this device's zone
/// says is someone else's — and the person's keeper takes it off their
/// list. With the agent on the person's list (the phone), what the voice
/// turn heard goes into the proxy's DM as one `m.room.message` from the
/// person and reaches the host as a turn; the device's watch over the
/// room's event cache, taken before the send and told the question's event
/// by the send queue, hears the answer's first words, each closed sentence
/// once and in order as the edits grow it, and the tail once the turn's
/// status says `idle` — which the host sets only after the final edit.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_spoken_question_is_one_message_and_its_answer_follows() {
    let smoke = Smoke::from_env();
    let stub = stub(ANSWER, 12, Duration::from_secs(3));
    let agent = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let mut host = host(
        &smoke,
        "nixi-smoke",
        "NIXI_SMOKE_PASSWORD",
        &stub_model(&stub),
        None,
    )
    .await;
    host.start();
    host.wait_serving(&host.room.clone()).await;
    let dir = host.root.path().join("person");
    let (client, seen, _sync) = person(&smoke, &dir, &host.room).await;
    let room = client
        .client()
        .get_room(&host.room)
        .expect("the person's room");
    let me = client.user_id().expect("signed in").to_owned();
    client
        .client()
        .account()
        .mark_as_dm(&host.room, std::slice::from_ref(&agent))
        .await
        .expect("the DM");
    // A first turn, typed, so the room has the session's status — a room is
    // read as a proxy conversation by its status, never guessed.
    ask(&client, &seen, &host.room, &agent, "hello", &|text| {
        text == ANSWER
    })
    .await;
    let kinds = AgentKinds::default();
    let refused = |sent: Result<_, CoreError>| match sent {
        Err(CoreError::Unsupported(why)) => why,
        Err(other) => panic!("refused as unsupported, not {other:?}"),
        Ok(_) => panic!("refused"),
    };

    // Nobody vouched for Nixi on this device: no zone, an empty list.
    set_proxy_list(&client, &[]).await;
    let why =
        refused(spoken_send(&room, &kinds, &AgentProxies::default(), None, "not for you").await);
    assert_eq!(why, "keeper has not been told this agent is your proxy.");
    // The list names Nixi, but this device's zone says it is Marta's: the
    // zone wins, and the person's keeper takes Nixi off their list.
    set_proxy_list(&client, &[&agent]).await;
    let martas = AgentProxies::default();
    martas.replace(std::collections::BTreeMap::from([(
        agent.clone(),
        ProxyFacts {
            human: OwnedUserId::try_from("@marta:example.org").expect("user"),
            allowed: Vec::new(),
        },
    )]));
    let why = refused(spoken_send(&room, &kinds, &martas, None, "not for marta").await);
    assert_eq!(why, "This agent is someone else's proxy.");
    proxy_list_becomes(&client, &std::collections::BTreeSet::new()).await;

    // The phone, with the list the person's Mac keeps.
    set_proxy_list(&client, &[&agent]).await;
    let question = "what is the answer";
    let mut answer = spoken_send(&room, &kinds, &AgentProxies::default(), None, question)
        .await
        .expect("sent to the person's own proxy");
    assert!(!answer.agent_name().is_empty());

    // Followed on its own task, as the app's shell follows it.
    let heard = tokio::spawn(async move {
        let mut steps = Vec::new();
        while let Some(step) = answer.next().await {
            steps.push(step);
        }
        steps
    });
    let steps = tokio::time::timeout(Duration::from_secs(180), heard)
        .await
        .expect("the answer ends")
        .expect("the follower ran");
    println!("spoken steps: {steps:?}");

    // The question: one message from the person; the refused ones never
    // left.
    let mine: Vec<Value> = seen
        .lock()
        .expect("lock")
        .iter()
        .map(|(_, e)| e.clone())
        .filter(|e| {
            e["sender"] == me.as_str()
                && e["type"] == "m.room.message"
                && e["content"]["m.relates_to"].is_null()
        })
        .collect();
    let bodies: Vec<&str> = mine
        .iter()
        .filter_map(|e| e["content"]["body"].as_str())
        .collect();
    assert_eq!(bodies, ["hello", question], "{mine:?}");

    // The host took it as a turn, once.
    assert_eq!(*stub.requests.lock().expect("lock"), 2);
    assert_eq!(anchors(&seen, &agent), 2, "answered once each");
    assert!(host.log_text().contains("a turn was answered"));

    // What was heard: the first words, the closed sentence once, the tail.
    let (first, rest) = steps.split_first().expect("something was heard");
    assert!(
        matches!(first, SpokenStep::FirstText { after_ms } if *after_ms < 60_000),
        "{first:?}"
    );
    assert!(
        !steps.iter().any(|s| matches!(s, SpokenStep::Failed(_))),
        "{steps:?}"
    );
    let (closing, sentences) = rest.split_last().expect("the answer completes");
    let sentences: Vec<&str> = sentences
        .iter()
        .map(|s| match s {
            SpokenStep::Sentence(sentence) => sentence.as_str(),
            other => panic!("a sentence, not {other:?}"),
        })
        .collect();
    let SpokenStep::Complete(tail) = closing else {
        panic!("the answer completes: {closing:?}");
    };
    let (first_sentence, second) = ANSWER.split_once(". ").expect("two sentences");
    assert_eq!(sentences, [format!("{first_sentence}.")]);
    assert_eq!(tail, second);
    host.kill();
}

/// Acceptance 14: the largest final edit whose encrypted event the server
/// accepts, binary-searched; `FINAL_CUT_BYTES` is set below it.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn the_largest_final_message_that_fits_encrypted() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let client = smoke
        .client(
            root.path(),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    let room = client
        .create_room(
            RoomKind::Session(SessionKind::Main),
            "cut",
            vec![smoke.user("tgorka-smoke")],
            &[],
        )
        .await
        .expect("room");
    client.sync_once().await.expect("sync");
    let anchor = client
        .send(
            &room,
            "m.room.message",
            json!({"msgtype":"m.text","body":"…"}),
            None,
        )
        .await
        .expect("anchor");
    let fits = |n: usize| {
        let client = client.clone();
        let room = room.clone();
        let anchor = anchor.clone();
        async move {
            match client
                .send(
                    &room,
                    "m.room.message",
                    events::edit_content(&anchor, &"a".repeat(n)),
                    None,
                )
                .await
            {
                Ok(_) => true,
                Err(AgentMatrixError::TooLarge) => false,
                Err(error) => panic!("at {n} bytes: {error}"),
            }
        }
    };
    let (mut low, mut high) = (1024usize, 128 * 1024usize);
    assert!(fits(low).await);
    assert!(!fits(high).await);
    while high - low > 64 {
        let mid = (low + high) / 2;
        if fits(mid).await {
            low = mid;
        } else {
            high = mid;
        }
    }
    let cut = keeper_core::agents::events::FINAL_CUT_BYTES;
    println!(
        "the largest final edit that fits encrypted: {low} bytes of text; FINAL_CUT_BYTES = {cut}"
    );
    assert!(
        cut + 512 <= low,
        "FINAL_CUT_BYTES {cut} plus its link does not fit under {low}"
    );
}
