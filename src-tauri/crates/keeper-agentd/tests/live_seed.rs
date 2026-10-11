//! `keeper-agentd agents init` against a real homeserver: the proxy's DM
//! (story 91.5, acceptance 5).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users.
//! Endpoints and secrets come from the environment, never from this
//! repository (S-20):
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_seed -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The seeded Nixi is `@nixi:<server>`; the test homeserver's agent user is
//! `nixi-smoke`, so between the two runs the test makes the edit an owner
//! would make to the seeded `agent.toml` — the seed is theirs after its
//! first run, and the second run leaves it.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use keeper_core::agents::events::{self, SESSION_ROOM_TYPE, STATUS};
use keeper_core::agents::seed::main_session_id;
use keeper_core::agents::session::{parse_session_agent_toml, SessionKind};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::OwnedRoomId;
use serde_json::{json, Value};

mod common;

use common::{bare_drive, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
/// Never CLIProxyAPI's URL (S-20); the DM needs no model.
const BOT: &str = "bot:openai:https://provider.example:8452#m";

struct Agentd {
    home: PathBuf,
    env: Vec<(String, String)>,
}

impl Agentd {
    fn run(&self, args: &[&str]) -> Output {
        let mut command = Command::new(BIN);
        command.args(args).env_clear();
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.output().expect("keeper-agentd")
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{args:?}: {said}");
        said
    }

    fn drive(&self) -> PathBuf {
        self.home.join("data/keeper-agentd/drives/smoke")
    }

    /// The active sessions' folders.
    fn sessions(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.drive().join("60-sessions/active")) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect()
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

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn agents_init_makes_the_proxys_dm_once() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    let person = smoke.user("tgorka-smoke");
    // The agent user keeps the DMs of earlier runs, and `agents init` would
    // rightly adopt one of them: leave every session room first, so this
    // run makes its own.
    {
        let stale = smoke
            .client(
                &root.path().join("stale"),
                "nixi-smoke",
                smoke.secret("NIXI_SMOKE_PASSWORD"),
            )
            .await;
        stale.sync_once().await.expect("sync");
        for room in stale.client().joined_rooms() {
            if room
                .room_type()
                .is_some_and(|kind| kind.to_string() == SESSION_ROOM_TYPE)
            {
                room.leave().await.expect("leave an old DM");
                let _ = room.forget().await;
            }
        }
    }
    smoke.clear_devices(&agent).await;
    smoke.clear_devices(&person).await;
    smoke.set_ratelimit(&agent, 0, 0).await;

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
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra-sim\"\nalways_on = true\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"nixi\"]\n",
            smoke.homeserver,
            bare.display()
        ),
    )
    .expect("agentd.toml");
    let agentd = Agentd {
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
        ],
        home,
    };
    let person_text = person.to_string();
    let init: Vec<&str> = vec![
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
    ];

    // First run: the zone is written into agentd's checkout; Nixi is not
    // signed in, so there is no DM yet.
    let said = agentd.ok(&init);
    assert!(said.contains("is not signed in here yet"), "{said}");
    assert!(agentd.sessions().is_empty());
    let nixi_toml = agentd.drive().join("80-agents/nixi/agent.toml");
    let seeded = std::fs::read_to_string(&nixi_toml).expect("seeded agent.toml");
    let owned = seeded.replace("\"@nixi:", "\"@nixi-smoke:");
    assert_ne!(owned, seeded);
    std::fs::write(&nixi_toml, &owned).expect("the owner's edit");
    agentd.ok(&[
        "login",
        "smoke/nixi",
        "--password-credential",
        "agent_password",
    ]);

    // The person's device is there before the room is, as a person's is:
    // the anchor is encrypted for the devices the room's members have.
    let person_client = smoke
        .client(
            &root.path().join("person"),
            "tgorka-smoke",
            smoke.secret("TGORKA_SMOKE_PASSWORD"),
        )
        .await;

    // Second run: every file left, and the DM made.
    let said = agentd.ok(&init);
    assert!(said.contains("Wrote 0 files"), "{said}");
    assert!(said.contains("Made Nixi's DM"), "{said}");
    assert_eq!(std::fs::read_to_string(&nixi_toml).expect("read"), owned);
    let sessions = agentd.sessions();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let session_toml = std::fs::read_to_string(sessions[0].join("agent.toml")).expect("toml");
    let session = parse_session_agent_toml(&session_toml).expect("session agent.toml");
    assert_eq!(session.kind, SessionKind::Main);
    assert_eq!(session.id, main_session_id("smoke", "nixi"));
    assert_eq!(session.requested_by, person);
    let room = session.room.clone();

    // The room: typed, direct, the person invited, their talk allowed.
    let state = admin_state(&smoke, &room).await;
    let of = |t: &str, key: &str| {
        state
            .iter()
            .find(|e| e["type"] == t && e["state_key"] == key)
            .unwrap_or_else(|| panic!("{t} {key} in {state:?}"))
            .clone()
    };
    assert_eq!(
        of("m.room.create", "")["content"]["type"],
        SESSION_ROOM_TYPE
    );
    let invite = of("m.room.member", person.as_str());
    assert_eq!(invite["content"]["membership"], "invite");
    assert_eq!(invite["content"]["is_direct"], true, "{invite}");
    let levels = &of("m.room.power_levels", "")["content"];
    assert_eq!(levels["events"]["m.room.message"], 0);
    assert_eq!(levels["events"][events::SCOPE], 0);
    assert_eq!(levels["users"][agent.as_str()], 100);
    assert_eq!(levels["state_default"], 50);

    // The person joins, reads the status anchor, and talks.
    person_client.join(&room).await.expect("the person joins");
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        person_client.sync_once().await.expect("sync");
        let joined = person_client.client().get_room(&room).expect("room");
        let page = joined
            .messages(MessagesOptions::backward())
            .await
            .expect("messages");
        let found = page.chunk.iter().find_map(|event| {
            let value: Value = event.raw().deserialize_as().ok()?;
            (value["type"] == STATUS).then_some(value)
        });
        if let Some(found) = found {
            break found;
        }
        assert!(
            Instant::now() < deadline,
            "no status anchor reached the person"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(status["sender"], agent.as_str());
    assert_eq!(status["content"]["kind"], "main");
    assert_eq!(status["content"]["run"], "idle");
    assert_eq!(status["content"]["agent"], agent.as_str());
    assert!(status["content"].get("anchor").is_none(), "{status}");
    person_client
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "hi Nixi"}),
            None,
        )
        .await
        .expect("the person talks in the DM");
    person_client
        .send(
            &room,
            events::SCOPE,
            json!({"drives": ["smoke"], "set_by": person.as_str()}),
            None,
        )
        .await
        .expect("the person sets the scope in the DM");
    let refused = person_client
        .client()
        .get_room(&room)
        .expect("room")
        .send_state_event_raw(events::CLAIM, "", json!({"v": 1}))
        .await
        .expect_err("a person writes no state in the DM (R30)");
    assert!(refused.to_string().contains("M_FORBIDDEN"), "{refused}");

    // Third run: nothing new.
    let said = agentd.ok(&init);
    assert!(said.contains("nothing made"), "{said}");
    assert_eq!(agentd.sessions().len(), 1);
    let again = std::fs::read_to_string(sessions[0].join("agent.toml")).expect("toml");
    assert_eq!(again, session_toml);

    // A checkout made again before `run` pushed the folder: the folder is
    // gone, the room is not. The DM is adopted, not made twice.
    std::fs::remove_dir_all(&sessions[0]).expect("lose the folder");
    let said = agentd.ok(&init);
    assert!(said.contains("had no main session here"), "{said}");
    let sessions = agentd.sessions();
    assert_eq!(sessions.len(), 1, "{sessions:?}");
    let adopted = parse_session_agent_toml(
        &std::fs::read_to_string(sessions[0].join("agent.toml")).expect("toml"),
    )
    .expect("session agent.toml");
    assert_eq!(adopted.room, room, "the same room, adopted");
    assert_eq!(
        adopted.label.readers,
        keeper_core::agents::label::Readers::Only([person.clone()].into_iter().collect())
    );
}
