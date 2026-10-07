//! A person asked through their proxy on a real homeserver (story 94.2,
//! acceptance 6; rulings R99–R101).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users, from
//! the environment as `live_turn.rs` reads them:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_ask -- --ignored --nocapture
//! ```
//!
//! The admin token makes (or re-passwords) the steward's user, `tola-smoke`,
//! and the delegating agent's, `lena-smoke`, whom the harness plays.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use keeper_core::agents::delegation::{
    brief_content, DelegateCard, DelegateContent, DelegateFrom, DelegateLimits,
};
use keeper_core::agents::events::CONTENT_VERSION;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, calls_at, record, syncing, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const DM: &str = "active/2026-10-06-nixi";

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
        let log = std::fs::File::create(self.root.path().join("agentd.log")).expect("log");
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

/// A host named `slug` hosting `agent` of the drive at `bare`, signed in;
/// the drive read by `person` alone.
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
    model: &str,
) -> String {
    let human = human.map_or_else(String::new, |h| format!("human = \"{h}\"\n"));
    format!(
        "version = 1\nid = \"{id}\"\nname = \"{name}\"\nkind = \"{kind}\"\nmatrix_user = \"{user}\"\n{human}\n[model]\nbot = \"bot:openai:{model}#stub\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"smoke\"]\n"
    )
}

fn soul(name: &str) -> String {
    format!("---\nname: {name}\ntitle: a smoke agent\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test agent.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\n{name} answers.\n")
}

/// `user`'s membership of `room`, as the server holds it now.
async fn membership(smoke: &Smoke, room: &OwnedRoomId, user: &OwnedUserId) -> Option<String> {
    let state = smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/state"),
            None,
        )
        .await;
    state["state"]
        .as_array()?
        .iter()
        .find(|e| e["type"] == "m.room.member" && e["state_key"] == user.as_str())
        .and_then(|e| e["content"]["membership"].as_str().map(str::to_owned))
}

/// Wait until `done` holds, or fail saying `what` with both hosts' logs.
async fn until(what: &str, hosts: [&Host; 2], mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(300);
    while !done() {
        assert!(
            Instant::now() < deadline,
            "{what}.\nnixi's host:\n{}\ntola's host:\n{}",
            hosts[0].log_text(),
            hosts[1].log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn of_kind<'a>(lines: &'a [Value], kind: &str) -> Vec<&'a Value> {
    lines.iter().filter(|l| l["kind"] == kind).collect()
}

/// 94.2 acceptance 6 on two agentd hosts: Dr Lena (the harness, an agent of
/// the drive no host serves) hands Dr Tola Grey a plan whose chain starts at
/// tgorka. Tola's `ask_human` invites Nixi, tgorka's proxy, into her
/// session's room; Nixi's host joins it (R101's arm), and only then does
/// the encrypted question go in. Nixi's host takes it into her DM with
/// tgorka; tgorka answers there; Nixi relays the answer into Tola's room
/// with `reply(…, ask)` and leaves it. Tola's session takes the answer —
/// its choice `Continue` — and runs on; Nixi is no longer a member.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn ask_human_round_trip_on_synapse() {
    let smoke = Smoke::from_env();
    let scratch = tempfile::tempdir().expect("tempdir");
    let person = smoke.user("tgorka-smoke");
    let nixi = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let lena = smoke.user("lena-smoke");
    let (tola_password, lena_password) =
        (ulid::Ulid::new().to_string(), ulid::Ulid::new().to_string());
    for (user, password) in [(&tola, &tola_password), (&lena, &lena_password)] {
        smoke
            .admin(
                reqwest::Method::PUT,
                &format!("/_synapse/admin/v2/users/{user}"),
                Some(json!({ "password": password, "admin": false })),
            )
            .await;
    }
    for agent in [&nixi, &tola, &lena] {
        smoke.clear_devices(agent).await;
        smoke.set_ratelimit(agent, 0, 0).await;
    }

    // Tola asks at once and goes on once answered; Nixi asks tgorka in her
    // own words, then relays his answer.
    let tolas_model = calls_at(
        vec![(
            1,
            "ask_human",
            json!({"question": "Go on to step 3?", "choices": ["Continue", "Stop"], "default": "Stop"}),
        )],
        "Going on to step 3.",
    );
    let nixis_model = calls_at(
        vec![(2, "reply", json!({"text": "Continue", "ask": "@ASK@"}))],
        "Dr Tola Grey asks whether to go on to step 3.",
    );

    // Nixi's DM, made as Nixi from a harness device, under its derived id.
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
    let readers = Readers::Only(BTreeSet::from([person.clone()]));
    let main = SessionAgent {
        id: keeper_core::agents::seed::main_session_id("smoke", "nixi"),
        agent: "nixi".to_owned(),
        drive: "smoke".to_owned(),
        kind: SessionKind::Main,
        title: "smoke".to_owned(),
        requested_by: person.clone(),
        parent: None,
        reply: None,
        room: dm.clone(),
        drives: vec!["smoke".to_owned()],
        label: Label::opening(&decl, Integrity::Owner),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: Vec::new(),
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
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
                &tolas_model.url,
            ),
        ),
        ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
        (
            "80-agents/lena/agent.toml".to_owned(),
            agent_toml("lena", "Dr Lena", "steward", &lena, None, &tolas_model.url),
        ),
        ("80-agents/lena/SOUL.md".to_owned(), soul("Dr Lena")),
        (
            format!("60-sessions/{DM}/agent.toml"),
            compose_session_agent_toml(&main),
        ),
        // The DM is found by its id, as `agents init` writes it.
        (
            format!("60-sessions/{DM}/README.md"),
            format!("---\nid: {}\n---\n\n# Nixi\n", main.id),
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

    // tgorka in his DM with Nixi.
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

    // Lena hands Tola the plan: her room, Tola at 50, tgorka observing.
    let lenas = smoke
        .client(&scratch.path().join("lena"), "lena-smoke", &lena_password)
        .await;
    let _lenas_sync = syncing(&lenas);
    let room = lenas
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "tola smoke",
            vec![tola.clone(), person.clone()],
            std::slice::from_ref(&tola),
        )
        .await
        .expect("the delegated room");
    let deadline = Instant::now() + Duration::from_secs(120);
    while membership(&smoke, &room, &tola).await.as_deref() != Some("join") {
        assert!(
            Instant::now() < deadline,
            "tola never joined.\n{}",
            tolas_host.log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    tokio::time::sleep(Duration::from_secs(5)).await;
    let brief = DelegateContent {
        v: CONTENT_VERSION,
        id: ulid::Ulid::new().to_string(),
        from: DelegateFrom {
            agent: lena.clone(),
            drive: "smoke".to_owned(),
            session: "active/2026-10-06-lena".to_owned(),
            room: room.clone(),
        },
        to: tola.clone(),
        brief: "Plan the epics; ask tgorka before step 3.".to_owned(),
        drives: vec!["smoke".to_owned()],
        label: Label {
            readers: readers.clone(),
            integrity: Integrity::Owner,
            local_only: false,
        },
        hop: 1,
        limits: DelegateLimits {
            rounds_per_exchange: 3,
            tokens: 1_000_000,
        },
        card: Some(DelegateCard {
            title: "Epics".to_owned(),
            schedule: None,
            workflow: None,
        }),
        dispatch_chain: vec![person.clone(), lena.clone()],
    };
    lenas
        .send(&room, "m.room.message", brief_content(&brief), None)
        .await
        .expect("the brief");

    // Tola asks: Nixi is invited, joins, and the question goes in.
    until(
        "tola never sent the ask",
        [&nixis_host, &tolas_host],
        || {
            of_kind(&tolas_host.lines(), "ask")
                .iter()
                .any(|l| l["body"]["state"] == "sent")
        },
    )
    .await;
    assert_eq!(
        membership(&smoke, &room, &nixi).await.as_deref(),
        Some("join")
    );
    let tolas = tolas_host.lines();
    let asked = of_kind(&tolas, "ask")[0].clone();
    assert_eq!(asked["body"]["state"], "asked");
    assert_eq!(asked["body"]["to"], person.as_str());
    assert_eq!(asked["body"]["via"], nixi.as_str());
    let id = asked["body"]["id"].as_str().expect("an id").to_owned();
    assert!(of_kind(&tolas, "run")
        .iter()
        .any(|l| l["body"]["state"] == "blocked"
            && l["body"]["detail"] == "waiting for tgorka-smoke, through Nixi"));

    // Nixi's host takes it into her DM, and Nixi asks tgorka there.
    until(
        "nixi never took the ask",
        [&nixis_host, &tolas_host],
        || {
            of_kind(&nixis_host.lines(), "peer")
                .iter()
                .any(|l| l["body"]["ask"]["id"] == id.as_str())
        },
    )
    .await;
    until(
        "nixi never asked tgorka",
        [&nixis_host, &tolas_host],
        || {
            of_kind(&nixis_host.lines(), "assistant")
                .iter()
                .any(|l| l["body"]["finish"] == "stop")
        },
    )
    .await;
    client
        .send(
            &dm,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "Continue"}),
            None,
        )
        .await
        .expect("the answer");

    // Nixi relays and leaves; Tola takes the answer and goes on.
    until(
        "tola never took the answer",
        [&nixis_host, &tolas_host],
        || {
            of_kind(&tolas_host.lines(), "ask")
                .iter()
                .any(|l| l["body"]["state"] == "answered")
        },
    )
    .await;
    let tolas = tolas_host.lines();
    let answer = of_kind(&tolas, "peer")
        .into_iter()
        .find(|l| l["body"]["answers"]["id"] == id.as_str())
        .expect("the answer's peer line")
        .clone();
    assert_eq!(answer["body"]["sender"], nixi.as_str());
    assert_eq!(answer["body"]["text"], "Continue");
    assert_eq!(answer["body"]["answers"]["choice"], "Continue");
    let nixis = nixis_host.lines();
    assert!(of_kind(&nixis, "ask")
        .iter()
        .any(|l| l["body"]["state"] == "answered" && l["body"]["id"] == id.as_str()));
    let deadline = Instant::now() + Duration::from_secs(60);
    while membership(&smoke, &room, &nixi).await.as_deref() != Some("leave") {
        assert!(
            Instant::now() < deadline,
            "nixi is still in tola's room.\n{}",
            nixis_host.log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    until("tola never went on", [&nixis_host, &tolas_host], || {
        of_kind(&tolas_host.lines(), "assistant")
            .iter()
            .any(|l| l["body"]["text"] == "Going on to step 3.")
    })
    .await;
    println!("ask {id}: asked, sent, relayed by {nixi}, answered `Continue`; {nixi} left {room}");
}
