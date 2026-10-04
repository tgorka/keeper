//! A delegation between two hosts of one principal against a real
//! homeserver (story 92.1, acceptance 9).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users, from
//! the environment as `live_turn.rs` reads them:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_delegate -- --ignored --nocapture
//! ```
//!
//! The admin token also makes (or re-passwords) the steward's user,
//! `tola-smoke`.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, record, syncing, tool_then, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const SESSION: &str = "active/2026-10-04-smoke";

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

    fn log(&self) -> PathBuf {
        self.root.path().join("agentd.log")
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(self.log()).unwrap_or_default()
    }

    fn start(&mut self) {
        let log = std::fs::File::create(self.log()).expect("log");
        let child = self
            .command(&["run"])
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .expect("keeper-agentd run");
        self.child = Some(child);
    }

    /// Every log line of every session of this host's checkout.
    fn lines(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for active in find_dirs(self.root.path(), "active") {
            for session in std::fs::read_dir(active).into_iter().flatten().flatten() {
                for chunk in std::fs::read_dir(session.path().join("log"))
                    .into_iter()
                    .flatten()
                    .flatten()
                {
                    if chunk.path().extension().is_some_and(|ext| ext == "jsonl") {
                        let text = std::fs::read_to_string(chunk.path()).unwrap_or_default();
                        out.extend(text.lines().filter_map(|l| serde_json::from_str(l).ok()));
                    }
                }
            }
        }
        out
    }

    /// The delegated session folders of this host's checkout.
    fn delegated(&self) -> Vec<PathBuf> {
        find_dirs(self.root.path(), "active")
            .into_iter()
            .flat_map(|active| std::fs::read_dir(active).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .filter(|dir| {
                std::fs::read_to_string(dir.join("agent.toml"))
                    .is_ok_and(|text| text.contains("kind = \"delegated\""))
            })
            .collect()
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

/// A host named `slug` hosting `agent` of the drive at `bare`, signed in.
fn host(
    smoke: &Smoke,
    slug: &str,
    agent: &str,
    password: &str,
    bare: &Path,
    model: &str,
    person: &OwnedUserId,
) -> Host {
    let root = tempfile::tempdir().expect("tempdir");
    let config = format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"{slug}\"\n\n[homeserver]\nurl = \"{}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{model}\"\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"{agent}\"]\n",
        smoke.homeserver,
        bare.display(),
    );
    let config_dir = root.path().join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("agentd.toml"), config).expect("config");
    let dir = |name: &str| root.path().join(name).display().to_string();
    let host = Host {
        env: vec![
            ("HOME".to_owned(), dir("")),
            ("XDG_CONFIG_HOME".to_owned(), dir("config")),
            ("XDG_DATA_HOME".to_owned(), dir("data")),
            ("XDG_STATE_HOME".to_owned(), dir("state")),
            (
                "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
                password.to_owned(),
            ),
            ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
        ],
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

fn agent_toml(
    id: &str,
    name: &str,
    kind: &str,
    user: &OwnedUserId,
    human: Option<&OwnedUserId>,
    allow: &str,
    model: &str,
) -> String {
    let human = human.map_or_else(String::new, |h| format!("human = \"{h}\"\n"));
    format!(
        "version = 1\nid = \"{id}\"\nname = \"{name}\"\nkind = \"{kind}\"\nmatrix_user = \"{user}\"\n{human}\n[model]\nbot = \"bot:openai:{model}#stub\"\n\n[tools]\nallow = [{allow}]\ndrives = [\"smoke\"]\n"
    )
}

fn soul(name: &str) -> String {
    format!("---\nname: {name}\ntitle: a smoke agent\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test agent.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\n{name} answers.\n")
}

/// The room's events in order, as the server holds them: type, sender,
/// state key.
async fn timeline(smoke: &Smoke, room: &OwnedRoomId) -> Vec<Value> {
    smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/messages?dir=f&limit=200"),
            None,
        )
        .await["chunk"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// 92.1 acceptance 9: Nixi's host invites; Dr Tola Grey's host joins; only
/// then does Nixi's host send the brief; Tola's host reads it, makes the
/// session once and replies; Nixi's session logs `delegate replied`. The
/// room's members are exactly Nixi, Tola and tgorka as observer.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_round_trip_on_a_real_homeserver() {
    let smoke = Smoke::from_env();
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

    // Nixi hands the work on, then relays the reply; Tola replies.
    let nixis_model = tool_then(
        "delegate",
        json!({"agent": "smoke/tola", "brief": "Say the inbox is sorted.", "card": {"title": "Inbox"}}),
        "Tola says it is done.",
    );
    let tolas_model = tool_then("reply", json!({"text": "The inbox is sorted."}), "Replied.");

    // Nixi's DM, made as Nixi from a harness device.
    let maker = smoke
        .client(
            &scratch.path().join("maker"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    let dm = maker
        .create_room(
            RoomKind::Session(SessionKind::Main),
            "smoke",
            vec![person.clone()],
            &[],
        )
        .await
        .expect("room");
    let drive_toml = format!(
        "version = 1\nid = \"smoke\"\ntitle = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    );
    let decl = keeper_core::agents::drive::parse(&drive_toml).expect("decl");
    let main = SessionAgent {
        id: ulid::Ulid::new(),
        agent: "nixi".to_owned(),
        drive: "smoke".to_owned(),
        kind: SessionKind::Main,
        title: "smoke".to_owned(),
        requested_by: person.clone(),
        parent: None,
        room: dm.clone(),
        drives: vec!["smoke".to_owned()],
        label: Label {
            readers: Readers::Only(BTreeSet::from([person.clone()])),
            ..Label::opening(&decl, Integrity::Owner)
        },
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: Vec::new(),
        limits: None,
        workflow: None,
        created_at: chrono::Utc::now(),
    };
    let files = vec![
        ("80-agents/_drive.toml".to_owned(), drive_toml.clone()),
        (
            "80-agents/nixi/agent.toml".to_owned(),
            agent_toml(
                "nixi",
                "Nixi",
                "proxy",
                &nixi,
                Some(&person),
                "\"drive_read\", \"delegate\"",
                &nixis_model.url,
            ),
        ),
        ("80-agents/nixi/SOUL.md".to_owned(), soul("Nixi")),
        (
            "80-agents/tola/agent.toml".to_owned(),
            agent_toml(
                "tola",
                "Dr Tola Grey",
                "steward",
                &tola,
                None,
                "\"drive_read\"",
                &tolas_model.url,
            ),
        ),
        ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
        (
            format!("60-sessions/{SESSION}/agent.toml"),
            compose_session_agent_toml(&main),
        ),
    ];
    let bare = bare_drive(scratch.path(), &files);
    let mut nixis_host = host(
        &smoke,
        "smoke-a",
        "nixi",
        smoke.secret("NIXI_SMOKE_PASSWORD"),
        &bare,
        &nixis_model.url,
        &person,
    );
    let mut tolas_host = host(
        &smoke,
        "smoke-b",
        "tola",
        &tola_password,
        &bare,
        &tolas_model.url,
        &person,
    );
    nixis_host.start();
    tolas_host.start();

    // The person asks in the DM.
    smoke.clear_devices(&person).await;
    let client = smoke
        .client(
            &scratch.path().join("person"),
            "tgorka-smoke",
            smoke.secret("TGORKA_SMOKE_PASSWORD"),
        )
        .await;
    client.join(&dm).await.expect("join");
    let _seen = record(&client);
    let _sync = syncing(&client);
    tokio::time::sleep(Duration::from_secs(8)).await;
    client
        .send(
            &dm,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "Hand the inbox to Tola."}),
            None,
        )
        .await
        .expect("ask");

    // Nixi's session logs the reply.
    let deadline = Instant::now() + Duration::from_secs(240);
    let (child, replied) = loop {
        let lines = nixis_host.lines();
        let delegates: Vec<&Value> = lines.iter().filter(|l| l["kind"] == "delegate").collect();
        let child = delegates
            .iter()
            .find_map(|l| l["body"]["room"].as_str())
            .map(|room| OwnedRoomId::try_from(room).expect("room"));
        let replied = delegates.iter().any(|l| l["body"]["state"] == "replied");
        if let (Some(child), true) = (&child, replied) {
            break (
                child.clone(),
                delegates
                    .iter()
                    .map(|l| l["body"]["state"].as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>(),
            );
        }
        assert!(
            Instant::now() < deadline,
            "no reply logged.\nnixi's host:\n{}\ntola's host:\n{}",
            nixis_host.log_text(),
            tolas_host.log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(replied, ["opened", "sent", "replied"]);

    // The brief went in only after Tola's join.
    let events = timeline(&smoke, &child).await;
    let joined = events
        .iter()
        .position(|e| {
            e["type"] == "m.room.member"
                && e["state_key"] == tola.as_str()
                && e["content"]["membership"] == "join"
        })
        .expect("tola joined");
    let first_from_nixi = events
        .iter()
        .position(|e| e["type"] == "m.room.encrypted" && e["sender"] == nixi.as_str())
        .expect("the brief");
    assert!(joined < first_from_nixi, "{events:#?}");

    // Exactly Nixi, Tola and tgorka are members.
    let state = smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{child}/state"),
            None,
        )
        .await;
    let members: BTreeSet<String> = state["state"]
        .as_array()
        .expect("state")
        .iter()
        .filter(|e| e["type"] == "m.room.member")
        .filter_map(|e| e["state_key"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        members,
        BTreeSet::from([nixi.to_string(), tola.to_string(), person.to_string()])
    );

    // Tola's host made the session once, accepted it and replied.
    let made = tolas_host.delegated();
    assert_eq!(made.len(), 1, "{made:?}");
    let tolas = tolas_host.lines();
    assert!(tolas
        .iter()
        .any(|l| l["kind"] == "delegate" && l["body"]["state"] == "accepted"));
    assert!(tolas
        .iter()
        .any(|l| l["kind"] == "tool_call" && l["body"]["tool"] == "reply"));
    let card = std::fs::read_to_string(made[0].join("brief.md")).expect("the card");
    assert!(card.contains("run: review"), "{card}");
}
