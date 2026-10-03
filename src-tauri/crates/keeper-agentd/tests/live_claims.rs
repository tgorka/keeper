//! Two `keeper-agentd run` hosts sharing one agent over one drive, against a
//! real homeserver (story 90.6, acceptance 9–13).
//!
//! `electra-sim` (`always_on = true`) and `hesperia-sim` each sign in their
//! own copy of `nixi-smoke` — two devices of one user — over one bare drive,
//! with one control room for their manifests. Every test is `#[ignore]`;
//! endpoints and secrets come from the environment, as in `live_turn.rs`:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_claims -- --ignored --nocapture --test-threads=1
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_agent::claims::{acquire, Acquired, RoomClaims, Rtt, ServerClock};
use keeper_core::agents::claim::{Claimant, TTL};
use keeper_core::agents::events::{CLAIM, HOST, STATUS};
use keeper_core::agents::host::HostManifest;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::reader::read_session;
use keeper_core::agents::log::{ClaimAction, LineBody};
use keeper_core::agents::matrix::{AgentClient, RoomKind};
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, record, stub, syncing, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const MAIN: &str = "active/2026-10-03-main";
const MAC: &str = "active/2026-10-03-mac";
const ANSWER: &str = "Answered.";

/// One `keeper-agentd` host.
struct Sim {
    slug: &'static str,
    root: PathBuf,
    env: Vec<(String, String)>,
    child: Option<Child>,
    log: PathBuf,
}

impl Sim {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command.args(args).env_clear();
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
        self.child = Some(
            self.command(&["run"])
                .stdout(Stdio::null())
                .stderr(log)
                .spawn()
                .expect("keeper-agentd run"),
        );
    }

    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// SIGTERM, then wait for the clean shutdown.
    fn terminate(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let pid = child.id().to_string();
        let sent = Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .expect("kill");
        assert!(sent.success());
        let status = child.wait().expect("wait");
        assert!(status.success(), "{status}: {}", self.log_text());
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// The epochs this host logged `claim acquired` at for `room`.
    fn acquired(&self, room: &OwnedRoomId) -> Vec<u64> {
        self.log_text()
            .lines()
            .filter(|line| line.contains("agents: claim acquired") && line.contains(room.as_str()))
            .filter_map(|line| {
                line.split_whitespace()
                    .find_map(|word| word.strip_prefix("epoch="))
                    .and_then(|v| v.parse().ok())
            })
            .collect()
    }

    /// The checkout of the drive on this host.
    fn session_dir(&self, path: &str) -> PathBuf {
        self.root
            .join("data/keeper-agentd/drives/smoke/60-sessions")
            .join(path)
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Pair {
    _root: tempfile::TempDir,
    maker: AgentClient,
    main: OwnedRoomId,
    mac: OwnedRoomId,
    control: OwnedRoomId,
    electra: Sim,
    hesperia: Sim,
    person: AgentClient,
    seen: Arc<Mutex<Vec<(Instant, Value)>>>,
    _syncs: Vec<tokio::task::JoinHandle<()>>,
}

fn session_agent(room: &OwnedRoomId, person: &OwnedUserId, mac: bool) -> SessionAgent {
    let drive_toml = drive_toml(person);
    let decl = keeper_core::agents::drive::parse(&drive_toml).expect("decl");
    SessionAgent {
        id: ulid::Ulid::new(),
        agent: "nixi".to_owned(),
        drive: "smoke".to_owned(),
        kind: if mac {
            SessionKind::Delegated
        } else {
            SessionKind::Main
        },
        title: if mac { "mac" } else { "main" }.to_owned(),
        requested_by: person.clone(),
        parent: None,
        room: room.clone(),
        drives: vec!["smoke".to_owned()],
        label: Label {
            readers: Readers::Only([person.clone()].into_iter().collect()),
            ..Label::opening(&decl, Integrity::Owner)
        },
        needs: mac.then(|| vec!["screen:mac".to_owned()]),
        pin: mac.then(|| "hesperia-sim".to_owned()),
        hop: 0,
        limits: None,
        workflow: None,
        created_at: chrono::Utc::now(),
    }
}

fn drive_toml(person: &OwnedUserId) -> String {
    format!(
        "version = 1\nid = \"smoke\"\ntitle = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    )
}

/// The two hosts over one drive, signed in, not started.
async fn pair(smoke: &Smoke, model_url: &str) -> Pair {
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    let person = smoke.user("tgorka-smoke");
    smoke.clear_devices(&agent).await;
    smoke.clear_devices(&person).await;
    smoke.set_ratelimit(&agent, 0, 0).await;
    let password = smoke.secret("NIXI_SMOKE_PASSWORD").to_owned();

    let maker = smoke
        .client(&root.path().join("maker"), "nixi-smoke", &password)
        .await;
    let mut rooms = Vec::new();
    for (name, kind) in [("main", SessionKind::Main), ("mac", SessionKind::Delegated)] {
        rooms.push(
            maker
                .create_room(RoomKind::Session(kind), name, vec![person.clone()], &[])
                .await
                .expect("room"),
        );
    }
    let control = maker
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![person.clone()],
            &[],
        )
        .await
        .expect("control room");
    let (main, mac) = (rooms[0].clone(), rooms[1].clone());

    let soul = "---\nname: Nixi\ntitle: the smoke proxy\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test proxy.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\nNixi answers.\n";
    let bare = bare_drive(
        root.path(),
        &[
            ("80-agents/_drive.toml".to_owned(), drive_toml(&person)),
            (
                "80-agents/nixi/agent.toml".to_owned(),
                format!(
                    "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"{agent}\"\nhuman = \"{person}\"\n\n[model]\nbot = \"bot:openai:{model_url}#stub\"\n\n[tools]\nallow = [\"drive_list\", \"drive_read\"]\ndrives = [\"smoke\"]\n\n[host]\nprefer_always_on = true\n"
                ),
            ),
            ("80-agents/nixi/SOUL.md".to_owned(), soul.to_owned()),
            (
                format!("60-sessions/{MAIN}/agent.toml"),
                compose_session_agent_toml(&session_agent(&main, &person, false)),
            ),
            (
                format!("60-sessions/{MAC}/agent.toml"),
                compose_session_agent_toml(&session_agent(&mac, &person, true)),
            ),
        ],
    );

    let sim = |slug: &'static str, always_on: bool| {
        let home = root.path().join(slug);
        let config = format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"{slug}\"\nalways_on = {always_on}\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"{control}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{model_url}\"\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"nixi\"]\n",
            smoke.homeserver,
            bare.display(),
        );
        let config_dir = home.join("config/keeper-agentd");
        std::fs::create_dir_all(&config_dir).expect("config dir");
        std::fs::write(config_dir.join("agentd.toml"), config).expect("config");
        let env = vec![
            ("HOME".to_owned(), home.display().to_string()),
            (
                "XDG_CONFIG_HOME".to_owned(),
                home.join("config").display().to_string(),
            ),
            (
                "XDG_DATA_HOME".to_owned(),
                home.join("data").display().to_string(),
            ),
            (
                "XDG_STATE_HOME".to_owned(),
                home.join("state").display().to_string(),
            ),
            (
                "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
                password.clone(),
            ),
            ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
        ];
        let sim = Sim {
            slug,
            log: home.join("agentd.log"),
            root: home,
            env,
            child: None,
        };
        let login = sim
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
            "login {slug}: {}{}",
            String::from_utf8_lossy(&login.stdout),
            String::from_utf8_lossy(&login.stderr)
        );
        sim
    };
    let electra = sim("electra-sim", true);
    let hesperia = sim("hesperia-sim", false);

    let person_client = smoke
        .client(
            &root.path().join("person"),
            "tgorka-smoke",
            smoke.secret("TGORKA_SMOKE_PASSWORD"),
        )
        .await;
    for room in [&main, &mac, &control] {
        person_client.join(room).await.expect("join");
    }
    person_client.sync_once().await.expect("sync");
    let seen = record(&person_client);
    let syncs = vec![syncing(&person_client), syncing(&maker)];
    Pair {
        _root: root,
        maker,
        main,
        mac,
        control,
        electra,
        hesperia,
        person: person_client,
        seen,
        _syncs: syncs,
    }
}

async fn wait_for(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + within;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// The claim the server holds now: `(event id, origin_server_ts, content)`.
async fn server_claim(client: &AgentClient, room: &OwnedRoomId) -> Option<(String, u64, Value)> {
    client
        .server_state(room, CLAIM, "")
        .await
        .expect("read")
        .map(|state| {
            (
                state.event_id.to_string(),
                u64::from(state.origin_server_ts.get()),
                state.content,
            )
        })
}

async fn manifest(client: &AgentClient, control: &OwnedRoomId, slug: &str) -> (HostManifest, u64) {
    let state = client
        .server_state(control, HOST, slug)
        .await
        .expect("read")
        .expect("a manifest");
    (
        serde_json::from_value(state.content).expect("manifest"),
        u64::from(state.origin_server_ts.get()),
    )
}

/// Ask in the main room and wait for the answer's final edit; its anchor.
async fn ask(pair: &Pair, question: &str) -> Value {
    let sent = pair
        .person
        .send(
            &pair.main,
            "m.room.message",
            json!({"msgtype":"m.text","body":question}),
            None,
        )
        .await
        .expect("ask");
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        let events: Vec<Value> = pair
            .seen
            .lock()
            .expect("lock")
            .iter()
            .map(|(_, v)| v.clone())
            .collect();
        let at = events.iter().position(|e| e["event_id"] == sent.as_str());
        if let Some(at) = at {
            let after = &events[at..];
            if let Some(anchor) = after
                .iter()
                .find(|e| e["content"]["dev.keeper.agent.turn"].is_object())
            {
                let id = anchor["event_id"].as_str().expect("id");
                if after.iter().any(|e| {
                    e["content"]["m.relates_to"]["event_id"] == id
                        && e["content"]["m.new_content"]["body"] == ANSWER
                }) {
                    return anchor.clone();
                }
            }
        }
        assert!(Instant::now() < deadline, "no answer to {question}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Every `claim` line of the main session in `sim`'s checkout.
fn claim_lines(sim: &Sim) -> Vec<(String, u64, ClaimAction)> {
    read_session(&sim.session_dir(MAIN))
        .lines
        .into_iter()
        .filter_map(|line| match line.body {
            LineBody::Claim(claim) => {
                Some((line.host.as_str().to_owned(), claim.epoch, claim.action))
            }
            _ => None,
        })
        .collect()
}

/// Acceptance 9: both hosts start within 50 ms and exactly one acquires the
/// first epoch; then two copies race for one claim in the same round trip,
/// exactly one wins, and the order Synapse gave the two writes is printed for
/// `docs/agents.md`.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_start_race_has_one_winner() {
    let smoke = Smoke::from_env();
    let model = stub(ANSWER, 1, Duration::from_millis(10));
    let mut pair = pair(&smoke, &model.url).await;
    let started = Instant::now();
    pair.electra.start();
    pair.hesperia.start();
    assert!(started.elapsed() < Duration::from_millis(50));

    let main = pair.main.clone();
    wait_for("a claim acquired", Duration::from_secs(120), || {
        !pair.electra.acquired(&main).is_empty() || !pair.hesperia.acquired(&main).is_empty()
    })
    .await;
    // Long enough for a loser's settle and a second tick on both hosts.
    tokio::time::sleep(Duration::from_secs(20)).await;
    let first: Vec<&str> = [&pair.electra, &pair.hesperia]
        .iter()
        .filter(|sim| sim.acquired(&main).first() == Some(&1))
        .map(|sim| sim.slug)
        .collect();
    assert_eq!(first.len(), 1, "epoch 1 acquired by {first:?}");

    let yielded: Vec<&str> = [&pair.electra, &pair.hesperia]
        .iter()
        .filter(|sim| sim.log_text().contains("claim yielded"))
        .map(|sim| sim.slug)
        .collect();
    println!("start race of two agentd: epoch 1 acquired by {first:?}; yielded: {yielded:?}");
    pair.electra.terminate();
    pair.hesperia.terminate();

    // Two processes rarely reach their first claim inside one round trip, so
    // the race itself is run too: two copies take one room's claim at once
    // through the `claims::acquire` the hosts use.
    let room = pair
        .maker
        .create_room(
            RoomKind::Session(SessionKind::Main),
            "race",
            Vec::new(),
            &[],
        )
        .await
        .expect("room");
    let password = smoke.secret("NIXI_SMOKE_PASSWORD").to_owned();
    let mut copies = Vec::new();
    for slug in ["electra-sim", "hesperia-sim"] {
        let client = smoke
            .client(
                &pair.electra.root.join(format!("race-{slug}")),
                "nixi-smoke",
                &password,
            )
            .await;
        let (rounds, seen) = tokio::sync::watch::channel(0u64);
        let syncer = client.client().clone();
        tokio::spawn(async move {
            let rounds = &rounds;
            let _ = syncer
                .sync_with_callback(
                    keeper_core::agents::matrix::sync_settings(),
                    |_| async move {
                        rounds.send_modify(|n| *n += 1);
                        matrix_sdk::LoopCtrl::Continue
                    },
                )
                .await;
        });
        let me = Claimant {
            host: slug.to_owned(),
            device: client.device_id().expect("device"),
            agent: smoke.user("nixi-smoke"),
        };
        copies.push((RoomClaims::new(client, room.clone(), seen), me));
    }
    let (clock, rtt) = (ServerClock::default(), Rtt::default());
    let (a, b) = tokio::join!(
        acquire(&copies[0].0, &copies[0].1, &clock, &rtt, None),
        acquire(&copies[1].0, &copies[1].1, &clock, &rtt, None),
    );
    let outcomes = [a.expect("electra-sim"), b.expect("hesperia-sim")];
    let won: Vec<&str> = outcomes
        .iter()
        .zip(["electra-sim", "hesperia-sim"])
        .filter(|(outcome, _)| matches!(outcome, Acquired::Won { .. }))
        .map(|(_, slug)| slug)
        .collect();
    let messages = smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/messages?dir=f&limit=100"),
            None,
        )
        .await;
    let writes: Vec<(u64, String, u64)> = messages["chunk"]
        .as_array()
        .expect("chunk")
        .iter()
        .filter(|e| e["type"] == CLAIM)
        .map(|e| {
            (
                e["origin_server_ts"].as_u64().unwrap_or(0),
                e["content"]["host"].as_str().unwrap_or("?").to_owned(),
                e["content"]["epoch"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    assert_eq!(writes.len(), 2, "both wrote: {writes:?}");
    let (_, _, held) = server_claim(&pair.maker, &room).await.expect("claim");
    println!(
        "two concurrent claim writes on Synapse: outcomes {outcomes:?}; the server holds {}'s; the writes in the order the server stamped them (origin_server_ts, host, epoch): {writes:?}",
        held["host"]
    );
    assert_eq!(won.len(), 1, "one winner: {outcomes:?}");
    assert_eq!(held["host"], won[0]);
}

/// Acceptance 10 and 13: the manifest carries what the host offers and is
/// renewed every 60 s; after `kill -9` of the holder the other host acquires
/// `epoch + 1` no sooner than 180 s after the last renewal's server time,
/// continues the session from its files, and the restarted old holder takes
/// the session back at a new epoch (C10), never writing at its old one.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn the_other_host_takes_over_after_expiry() {
    let smoke = Smoke::from_env();
    let model = stub(ANSWER, 1, Duration::from_millis(10));
    let mut pair = pair(&smoke, &model.url).await;
    pair.electra.start();
    let main = pair.main.clone();
    wait_for("electra's claim", Duration::from_secs(120), || {
        !pair.electra.acquired(&main).is_empty()
    })
    .await;
    pair.hesperia.start();

    let (first, first_ts) = manifest(&pair.maker, &pair.control, "electra-sim").await;
    assert!(first.always_on);
    assert_eq!(first.principal, "tgorka");
    assert_eq!(first.agents, ["smoke/nixi"]);
    assert_eq!(first.bots.len(), 1);
    assert_eq!(first.bots[0].len(), 16);
    assert_eq!(first.drives.len(), 1);
    assert!(first.drives[0].present);
    assert_eq!(
        serde_json::to_value(first.drives[0].materialized).expect("word"),
        json!("full")
    );
    assert!(!serde_json::to_string(&first)
        .expect("text")
        .contains("http"));

    ask(&pair, "first").await;

    // The renewal: a newer manifest within 60 s and a tick.
    tokio::time::sleep(Duration::from_secs(62)).await;
    let (_, renewed_ts) = manifest(&pair.maker, &pair.control, "electra-sim").await;
    assert!(renewed_ts > first_ts, "the manifest was renewed");
    assert!(
        renewed_ts - first_ts <= 62_000,
        "{} ms",
        renewed_ts - first_ts
    );
    let (_, last_renewal, held) = server_claim(&pair.maker, &main).await.expect("claim");
    assert_eq!(held["host"], "electra-sim");
    let epoch = held["epoch"].as_u64().expect("epoch");

    pair.electra.kill();
    wait_for("hesperia's takeover", Duration::from_secs(300), || {
        pair.hesperia.acquired(&main).contains(&(epoch + 1))
    })
    .await;
    let (_, taken_ts, taken) = server_claim(&pair.maker, &main).await.expect("claim");
    assert_eq!(taken["host"], "hesperia-sim");
    assert_eq!(taken["epoch"], epoch + 1);
    let (_, renewal_now) = manifest(&pair.maker, &pair.control, "electra-sim").await;
    let lapsed_after = taken_ts.saturating_sub(last_renewal.max(renewal_now));
    assert!(
        taken_ts - last_renewal >= TTL.as_millis() as u64,
        "taken {} ms after the last renewal",
        taken_ts - last_renewal
    );
    let (killed, _) = manifest(&pair.maker, &pair.control, "electra-sim").await;
    assert!(
        !killed.is_live(taken_ts + 1_000),
        "a killed host is not live after 180 s"
    );
    println!("takeover {} ms after electra's last claim renewal; {lapsed_after} ms after its last manifest", taken_ts - last_renewal);
    let log = pair.hesperia.log_text();
    assert!(
        log.contains("server_ts="),
        "the acquired log names its server time"
    );

    // The session continues from its files on hesperia.
    ask(&pair, "second").await;
    let lines = read_session(&pair.hesperia.session_dir(MAIN)).lines;
    let users: Vec<String> = lines
        .iter()
        .filter_map(|line| match &line.body {
            LineBody::User(user) => Some(format!("{}:{}", line.host, user.text)),
            _ => None,
        })
        .collect();
    assert!(
        users.contains(&"hesperia-sim:second".to_owned()),
        "{users:?}"
    );
    let chunks = std::fs::read_dir(pair.hesperia.session_dir(MAIN).join("log"))
        .expect("log dir")
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    assert!(
        chunks.iter().any(|name| name.contains(".hesperia-sim.")),
        "{chunks:?}"
    );
    println!("hesperia's view of the session's users after takeover: {users:?}");

    // The old holder restarts: it never writes at its old epoch, and takes
    // the session back at a new one when hesperia is idle.
    pair.electra.start();
    wait_for("the hand-back", Duration::from_secs(180), || {
        pair.electra.acquired(&main).iter().any(|e| *e > epoch + 1)
    })
    .await;
    assert!(pair
        .hesperia
        .log_text()
        .contains("handing the session back"));
    let restarted = pair.electra.acquired(&main);
    assert!(restarted.iter().all(|e| *e != epoch + 1));
    let problems = read_session(&pair.electra.session_dir(MAIN)).problems;
    assert!(
        problems.iter().all(|p| !p.sentence.contains("newer epoch")),
        "{problems:?}"
    );
    pair.electra.terminate();
    pair.hesperia.terminate();
}

/// Acceptance 11: SIGTERM writes `released: true` and the other host's
/// claim follows within two ticks.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_clean_shutdown_releases_and_the_other_takes_over_within_two_ticks() {
    let smoke = Smoke::from_env();
    let model = stub(ANSWER, 1, Duration::from_millis(10));
    let mut pair = pair(&smoke, &model.url).await;
    let claims_seen = record(&pair.maker);
    pair.electra.start();
    let main = pair.main.clone();
    wait_for("electra's claim", Duration::from_secs(120), || {
        !pair.electra.acquired(&main).is_empty()
    })
    .await;
    pair.hesperia.start();
    tokio::time::sleep(Duration::from_secs(10)).await;
    assert!(
        pair.hesperia.acquired(&main).is_empty(),
        "electra is preferred"
    );

    pair.electra.terminate();
    assert!(pair.electra.log_text().contains("agents: claim released"));
    wait_for("hesperia's claim", Duration::from_secs(60), || {
        !pair.hesperia.acquired(&main).is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let writes: Vec<Value> = claims_seen
        .lock()
        .expect("lock")
        .iter()
        .map(|(_, e)| e.clone())
        .filter(|e| e["type"] == CLAIM)
        .collect();
    let released = writes
        .iter()
        .find(|e| e["content"]["released"] == true && e["content"]["host"] == "electra-sim")
        .expect("electra wrote released: true");
    let taken = writes
        .iter()
        .find(|e| e["content"]["host"] == "hesperia-sim")
        .expect("hesperia's claim");
    let gap = taken["origin_server_ts"].as_u64().expect("ts")
        - released["origin_server_ts"].as_u64().expect("ts");
    println!("hesperia's claim {gap} ms after electra's release");
    assert!(gap <= 2_000, "{gap} ms: more than two ticks");
    let lines = claim_lines(&pair.electra);
    assert!(
        lines
            .iter()
            .any(|(host, _, action)| host == "electra-sim" && *action == ClaimAction::Released),
        "{lines:?}"
    );
    pair.hesperia.terminate();
}

/// Acceptance 12: a session pinned to `hesperia-sim` and needing
/// `screen:mac`, which no host offers, shows
/// `waiting: hesperia-sim — screen:mac` on its status, said once.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_session_no_live_host_can_serve_waits_named() {
    let smoke = Smoke::from_env();
    let model = stub(ANSWER, 1, Duration::from_millis(10));
    let mut pair = pair(&smoke, &model.url).await;
    pair.electra.start();
    pair.hesperia.start();
    let seen = Arc::clone(&pair.seen);
    let statuses = move || -> Vec<Value> {
        seen.lock()
            .expect("lock")
            .iter()
            .map(|(_, e)| e.clone())
            // The main session is placed, so only the pinned one waits.
            .filter(|e| e["type"] == STATUS && e["content"]["run"] == "waiting")
            .collect()
    };
    wait_for("the waiting status", Duration::from_secs(120), || {
        !statuses().is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let waiting: Vec<String> = statuses()
        .iter()
        .map(|e| {
            e["content"]["waiting"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    // Before hesperia's manifest is seen the pinned host is not live; once
    // it is, the status names what it lacks. Each is said once.
    assert_eq!(
        waiting.last().map(String::as_str),
        Some("hesperia-sim — screen:mac"),
        "{waiting:?}"
    );
    let mut said = waiting.clone();
    said.dedup();
    assert_eq!(said, waiting, "each said once");
    assert!(pair.electra.acquired(&pair.mac).is_empty());
    assert!(pair.hesperia.acquired(&pair.mac).is_empty());
    pair.electra.terminate();
    pair.hesperia.terminate();
}
