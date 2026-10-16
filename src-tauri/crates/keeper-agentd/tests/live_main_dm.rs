//! A person's ask for their proxy's DM, answered by a running host against a
//! real homeserver (story 101.1 #4 and #8, R416).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users.
//! Endpoints and secrets come from the environment, never from this
//! repository (S-20):
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_main_dm -- --ignored --nocapture --test-threads=1
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::events::{HOST, MAIN_DM_ANSWER, MAIN_DM_REQUEST, SESSION_ROOM_TYPE};
use keeper_core::agents::matrix::{AgentClient, RoomKind};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, bootstrap, record, syncing, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
/// Never CLIProxyAPI's URL (S-20); the DM needs no model.
const BOT: &str = "bot:openai:https://provider.example:8452#m";

struct Agentd {
    home: PathBuf,
    env: Vec<(String, String)>,
    child: Option<Child>,
}

impl Agentd {
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(BIN);
        command.args(args).env_clear();
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.command(args).output().expect("keeper-agentd");
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?}: {said}");
        said
    }

    fn log(&self) -> PathBuf {
        self.home.join("agentd.log")
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
}

impl Drop for Agentd {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

async fn admin_state(smoke: &Smoke, room: &OwnedRoomId) -> Vec<Value> {
    smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/state"),
            None,
        )
        .await["state"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The answer `agent` sent to `request`, once `seen` holds it.
async fn answer_to(
    seen: &Arc<Mutex<Vec<(Instant, Value)>>>,
    agent: &OwnedUserId,
    request: &str,
    log: &std::path::Path,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let found = seen.lock().expect("lock").iter().find_map(|(_, event)| {
            (event["type"] == MAIN_DM_ANSWER
                && event["sender"] == agent.as_str()
                && event["content"]["request"] == request)
                .then(|| event["content"].clone())
        });
        if let Some(found) = found {
            return found;
        }
        assert!(
            Instant::now() < deadline,
            "no answer to {request}:\n{}",
            std::fs::read_to_string(log).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// R416: with no DM, the person asks through their control room; the
/// proxy's running host makes the DM with the same `seed::main_dm` and says
/// which room, the person's invite to it is direct; anyone else asking is
/// refused with a sentence naming whose proxy it is.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_running_host_answers_the_persons_ask_for_their_dm() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    let person = smoke.user("tgorka-smoke");
    let stranger = smoke.user("nixi-paced");
    // The agent user keeps the DMs of earlier runs, which the host would
    // rightly adopt: leave every session room first, so this run makes one.
    let maker = smoke
        .client(
            &root.path().join("maker"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    for room in maker.client().joined_rooms() {
        if room
            .room_type()
            .is_some_and(|kind| kind.to_string() == SESSION_ROOM_TYPE)
        {
            room.leave().await.expect("leave an old DM");
            let _ = room.forget().await;
        }
    }
    smoke.set_ratelimit(&agent, 0, 0).await;
    // The control room `keeper-agentd init` makes: the proxy's, the person
    // and a stranger in it.
    let control = maker
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![person.clone(), stranger.clone()],
            &[],
        )
        .await
        .expect("control room");
    drop(maker);
    smoke.clear_devices(&agent).await;
    smoke.clear_devices(&person).await;
    smoke.clear_devices(&stranger).await;

    let bare = bare_drive(
        root.path(),
        &[("README.md".to_owned(), "the smoke drive\n".to_owned())],
    );
    let home = root.path().join("electra-sim");
    let config_dir = home.join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra-sim\"\nalways_on = true\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"{control}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"https://provider.example:8452\"\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"nixi\"]\n",
            smoke.homeserver,
            bare.display()
        ),
    )
    .expect("agentd.toml");
    let mut agentd = Agentd {
        env: vec![
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
                smoke.secret("NIXI_SMOKE_PASSWORD").to_owned(),
            ),
            ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
        ],
        home,
        child: None,
    };
    let person_text = person.to_string();
    // The zone, with Nixi not signed in: no DM is made.
    let said = agentd.ok(&[
        "agents",
        "init",
        "smoke",
        "--with",
        "nixi",
        "--owner",
        &person_text,
        "--reader",
        &person_text,
        "--bot",
        BOT,
    ]);
    assert!(said.contains("is not signed in here yet"), "{said}");
    let nixi_toml = agentd
        .home
        .join("data/keeper-agentd/drives/smoke/80-agents/nixi/agent.toml");
    let seeded = std::fs::read_to_string(&nixi_toml).expect("seeded agent.toml");
    std::fs::write(&nixi_toml, seeded.replace("\"@nixi:", "\"@nixi-smoke:"))
        .expect("the owner's edit");
    agentd.ok(&[
        "login",
        "smoke/nixi",
        "--password-credential",
        "agent_password",
    ]);

    // The person and the stranger, verified by their own identities, in the
    // control room.
    let asker = smoke
        .client(
            &root.path().join("person"),
            "tgorka-smoke",
            smoke.secret("TGORKA_SMOKE_PASSWORD"),
        )
        .await;
    bootstrap(&asker, &person, smoke.secret("TGORKA_SMOKE_PASSWORD")).await;
    asker.join(&control).await.expect("the person joins");
    let other = smoke
        .client(
            &root.path().join("stranger"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    bootstrap(&other, &stranger, smoke.secret("NIXI_PACED_PASSWORD")).await;
    other.join(&control).await.expect("the stranger joins");

    agentd.start();
    // Serving: its manifest is in the control room.
    let deadline = Instant::now() + Duration::from_secs(120);
    while !admin_state(&smoke, &control)
        .await
        .iter()
        .any(|event| event["type"] == HOST && event["sender"] == agent.as_str())
    {
        assert!(
            Instant::now() < deadline,
            "the host never published its manifest:\n{}",
            std::fs::read_to_string(agentd.log()).unwrap_or_default()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let seen = record(&asker);
    let syncs = [syncing(&asker), syncing(&other)];
    // Its copy's device is known before the ask is encrypted for it.
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Someone else asks: refused, naming whose proxy Nixi is.
    let refused = send(&other, &control, &agent).await;
    let answer = answer_to(&seen, &agent, &refused, &agentd.log()).await;
    assert_eq!(answer["room"], Value::Null, "{answer}");
    assert!(
        answer["refused"]
            .as_str()
            .is_some_and(|sentence| sentence.contains(person.as_str())),
        "{answer}"
    );

    // The person asks: the DM, made, the person's invite to it direct.
    let asked = send(&asker, &control, &agent).await;
    let answer = answer_to(&seen, &agent, &asked, &agentd.log()).await;
    assert_eq!(answer["refused"], Value::Null, "{answer}");
    let room = OwnedRoomId::try_from(answer["room"].as_str().expect("a room")).expect("room id");
    let state = admin_state(&smoke, &room).await;
    let invite = state
        .iter()
        .find(|e| e["type"] == "m.room.member" && e["state_key"] == person.as_str())
        .unwrap_or_else(|| panic!("the person's membership in {state:?}"));
    assert_eq!(invite["content"]["membership"], "invite");
    assert_eq!(invite["content"]["is_direct"], true, "{invite}");
    let create = state
        .iter()
        .find(|e| e["type"] == "m.room.create")
        .expect("create");
    assert_eq!(create["content"]["type"], SESSION_ROOM_TYPE);
    assert_eq!(create["sender"], agent.as_str());

    // Asked again: the same DM, not a second one.
    let again = send(&asker, &control, &agent).await;
    let answer = answer_to(&seen, &agent, &again, &agentd.log()).await;
    assert_eq!(answer["room"], json!(room), "{answer}");
    for sync in syncs {
        sync.abort();
    }
}

/// Send the person's ask for `agent`'s DM into `control` as `client`, as
/// keeper does — state keyed by the asker; its id.
async fn send(client: &AgentClient, control: &OwnedRoomId, agent: &OwnedUserId) -> String {
    let me = client.user_id().expect("signed in").to_string();
    client
        .send_state(
            control,
            MAIN_DM_REQUEST,
            &me,
            &json!({"v": 1, "agent": agent, "ask": ulid::Ulid::new().to_string()}),
        )
        .await
        .expect("the ask")
        .to_string()
}
