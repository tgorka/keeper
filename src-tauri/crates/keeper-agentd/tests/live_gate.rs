//! `keeper-agentd agents gate` against a real homeserver: a gate's room and
//! its session, made once (story 99.1, acceptance 5), and a room the outside
//! system cannot take (acceptance 8).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users.
//! Endpoints and secrets come from the environment, never from this
//! repository (S-20):
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_gate -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The gate agent is `nixi-smoke`; the outside system's account (the peer)
//! is `nixi-paced`, a user no home of the test drive names; the person is
//! `tgorka-smoke`, the drive's reader, pinned in `[[trust]]` so the gate's
//! rooms invite them as their approver.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use keeper_core::agents::events::{self, SESSION_ROOM_TYPE};
use keeper_core::agents::gate::gate_session_id;
use keeper_core::agents::label::{Integrity, Readers};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::session::{parse_session_agent_toml, SessionAgent, SessionKind};
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId};
use serde_json::{json, Value};

mod common;

use common::{bare_drive, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
/// Never CLIProxyAPI's URL (S-20); making a room needs no model.
const BOT: &str = "bot:openai:https://provider.example:8452#m";
/// A pinned key's shape; nothing here verifies a device with it.
const PIN: &str = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

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

    /// A run refused as a person must act (exit 2); what it said.
    fn refused(&self, args: &[&str]) -> String {
        let out = self.run(args);
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.status.code(), Some(2), "{args:?}: {said}");
        said
    }

    fn drive(&self) -> PathBuf {
        self.home.join("data/keeper-agentd/drives/smoke")
    }

    /// The active sessions: each folder, its `agent.toml` and what it says.
    fn sessions(&self) -> Vec<(PathBuf, String, SessionAgent)> {
        let Ok(entries) = std::fs::read_dir(self.drive().join("60-sessions/active")) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .map(|dir| {
                let text = std::fs::read_to_string(dir.join("agent.toml")).expect("agent.toml");
                let agent = parse_session_agent_toml(&text).expect("a session agent.toml");
                (dir, text, agent)
            })
            .collect()
    }

    fn session_of(&self, system: &str) -> (PathBuf, String, SessionAgent) {
        let mut of: Vec<_> = self
            .sessions()
            .into_iter()
            .filter(|(_, _, agent)| agent.title == system)
            .collect();
        assert_eq!(of.len(), 1, "one {system} gate session");
        of.remove(0)
    }
}

fn soul(name: &str) -> String {
    format!("---\nname: {name}\ntitle: a smoke gate\nicon: \"*\"\nrole: The door for the smoke systems.\nidentity: \"A test gate.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\n{name} answers.\n")
}

/// The room's current state, as the server holds it.
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

fn state_of<'a>(state: &'a [Value], kind: &str, key: &str) -> Option<&'a Value> {
    state
        .iter()
        .find(|e| e["type"] == kind && e["state_key"] == key)
}

/// Leave every session room `localpart` is in, so this run makes its own
/// rather than adopting an earlier run's.
async fn leave_session_rooms(smoke: &Smoke, dir: &Path, localpart: &str, password: &str) {
    let client = smoke.client(dir, localpart, password).await;
    for room in client.client().joined_rooms() {
        if room
            .room_type()
            .is_some_and(|kind| kind.to_string() == SESSION_ROOM_TYPE)
        {
            room.leave().await.expect("leave an old session room");
            let _ = room.forget().await;
        }
    }
}

/// One agentd host over a drive holding the gate `forge` (the user
/// `nixi-smoke`) with two systems: `github`, encrypted, its audience the
/// peer and the person; `ci`, clear, its audience anyone. The gate's copy
/// is signed in once a first `agents gate` has said it is not.
async fn gate_host(smoke: &Smoke, root: &Path) -> Agentd {
    let agent = smoke.user("nixi-smoke");
    let peer = smoke.user("nixi-paced");
    let person = smoke.user("tgorka-smoke");
    leave_session_rooms(
        smoke,
        &root.join("stale-agent"),
        "nixi-smoke",
        smoke.secret("NIXI_SMOKE_PASSWORD"),
    )
    .await;
    leave_session_rooms(
        smoke,
        &root.join("stale-peer"),
        "nixi-paced",
        smoke.secret("NIXI_PACED_PASSWORD"),
    )
    .await;
    smoke.clear_devices(&agent).await;
    smoke.clear_devices(&peer).await;
    smoke.set_ratelimit(&agent, 0, 0).await;
    smoke.set_ratelimit(&peer, 0, 0).await;

    let drive_toml = format!(
        "version = 1\nid = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    );
    let forge = format!(
        "version = 1\nid = \"forge\"\nname = \"Forge\"\nkind = \"gate\"\nmatrix_user = \"{agent}\"\n\n\
         [[gate]]\nsystem = \"github\"\npeer = \"{peer}\"\naudience = [\"{peer}\", \"{person}\"]\nmodes = [\"sync\", \"async\"]\n\n\
         [[gate]]\nsystem = \"ci\"\npeer = \"{peer}\"\naudience = [\"*\"]\nmodes = [\"async\"]\nencrypted = false\n\n\
         [model]\nbot = \"{BOT}\"\n"
    );
    let bare = bare_drive(
        root,
        &[
            ("80-agents/_drive.toml".to_owned(), drive_toml),
            ("80-agents/forge/agent.toml".to_owned(), forge),
            ("80-agents/forge/SOUL.md".to_owned(), soul("Forge")),
        ],
    );
    let home = root.join("electra-sim");
    let config_dir = home.join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra-sim\"\nalways_on = true\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"forge\"]\n\n[[trust]]\nuser = \"{person}\"\nmaster_key = \"{PIN}\"\n",
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
    let said = agentd.refused(&["agents", "gate", "forge", "github"]);
    assert!(said.contains(agent.as_str()), "the copy to sign in: {said}");
    assert!(agentd.sessions().is_empty());
    agentd.ok(&[
        "login",
        "smoke/forge",
        "--password-credential",
        "agent_password",
    ]);
    agentd
}

/// 99.1 #5: run twice, one room and one folder; the room typed
/// `dev.keeper.agent.session`, the session `kind = "gate"` under its derived
/// id, asked for by the drive's owner, untrusted for its audience; the peer
/// and the approver invited. A folder lost before `run` pushed it is made
/// again naming the same room, which is adopted, not made twice — also once
/// the peer has declined (R366 as amended). A room at levels the gate does
/// not make, or one someone else made under a gate room's name, is refused
/// and nothing is made (R99R1-01).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn agents_gate_is_idempotent() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let agent = smoke.user("nixi-smoke");
    let peer = smoke.user("nixi-paced");
    let person = smoke.user("tgorka-smoke");
    let agentd = gate_host(&smoke, root.path()).await;

    agentd.ok(&["agents", "gate", "forge", "github"]);
    assert_eq!(agentd.sessions().len(), 1);
    let (dir, text, session) = agentd.session_of("github");
    assert!(text.contains("kind = \"gate\"\n"), "{text}");
    assert_eq!(session.kind, SessionKind::Gate);
    assert_eq!(session.id, gate_session_id("smoke", "forge", "github"));
    assert_eq!(session.requested_by, person);
    assert_eq!(session.dispatch_chain, [person.clone(), agent.clone()]);
    assert_eq!(session.label.integrity, Integrity::Untrusted);
    assert_eq!(
        session.label.readers,
        Readers::Only([peer.clone(), person.clone()].into_iter().collect())
    );
    let gate = session.gate.clone().expect("[gate]");
    assert_eq!(gate.system, "github");
    assert_eq!(gate.peer, peer);
    assert!(gate.encrypted);
    let room = session.room.clone();

    let state = admin_state(&smoke, &room).await;
    let create = state_of(&state, "m.room.create", "").expect("m.room.create");
    assert_eq!(create["content"]["type"], SESSION_ROOM_TYPE);
    assert!(state_of(&state, "m.room.encryption", "").is_some());
    for invited in [&peer, &person] {
        let member = state_of(&state, "m.room.member", invited.as_str()).expect("member");
        assert_eq!(member["content"]["membership"], "invite", "{invited}");
    }
    let members = state
        .iter()
        .filter(|e| e["type"] == "m.room.member")
        .count();
    assert_eq!(members, 3, "the gate, its peer and its approver");

    // Again, by <drive>/<id>: nothing.
    agentd.ok(&["agents", "gate", "smoke/forge", "github"]);
    assert_eq!(agentd.sessions().len(), 1);
    assert_eq!(
        std::fs::read_to_string(dir.join("agent.toml")).expect("toml"),
        text
    );

    // The folder lost before `run` pushed it: the room is adopted.
    std::fs::remove_dir_all(&dir).expect("lose the folder");
    agentd.ok(&["agents", "gate", "forge", "github"]);
    assert_eq!(agentd.sessions().len(), 1);
    let (dir, _, adopted) = agentd.session_of("github");
    assert_eq!(adopted.room, room, "the same room, adopted");

    // The peer declines: the room is still the gate's, by its own status.
    let peer_client = smoke
        .client(
            &root.path().join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    peer_client.sync_once().await.expect("sync");
    peer_client
        .client()
        .get_room(&room)
        .expect("invited")
        .leave()
        .await
        .expect("the peer declines");
    let state = admin_state(&smoke, &room).await;
    let member = state_of(&state, "m.room.member", peer.as_str()).expect("peer");
    assert_eq!(member["content"]["membership"], "leave");
    std::fs::remove_dir_all(&dir).expect("lose the folder");
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (dir, _, adopted) = agentd.session_of("github");
    assert_eq!(adopted.room, room, "adopted, the peer gone");

    // The room's levels changed by another of the gate's devices: refused,
    // nothing made; restored, adopted again.
    let agent_client = smoke
        .client(
            &root.path().join("agent"),
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
        )
        .await;
    agent_client.sync_once().await.expect("sync");
    let levels = state_of(&state, "m.room.power_levels", "").expect("levels")["content"].clone();
    let mut raised = levels.clone();
    raised["users"][person.as_str()] = json!(50);
    let as_gate = agent_client.client().get_room(&room).expect("joined");
    as_gate
        .send_state_event_raw("m.room.power_levels", "", raised)
        .await
        .expect("the gate raises the person");
    std::fs::remove_dir_all(&dir).expect("lose the folder");
    let said = agentd.refused(&["agents", "gate", "forge", "github"]);
    assert!(said.contains(room.as_str()), "{said}");
    assert!(agentd.sessions().is_empty(), "nothing made: {said}");
    as_gate
        .send_state_event_raw("m.room.power_levels", "", levels)
        .await
        .expect("the gate restores the levels");
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (_, _, adopted) = agentd.session_of("github");
    assert_eq!(adopted.room, room, "adopted at the gate's levels");

    // A room the peer made under the ci gate room's name, the gate's copy
    // in it: refused, and no ci session.
    let foreign = peer_client
        .create_room(
            RoomKind::Gate { encrypted: false },
            "Forge · ci",
            vec![agent.clone()],
            &[],
        )
        .await
        .expect("the peer makes a room");
    agent_client.sync_once().await.expect("sync");
    agent_client.join(&foreign).await.expect("the gate joins");
    let said = agentd.refused(&["agents", "gate", "forge", "ci"]);
    assert!(said.contains(foreign.as_str()), "{said}");
    assert!(
        agentd
            .sessions()
            .iter()
            .all(|(_, _, session)| session.title != "ci"),
        "{said}"
    );

    // A system the gate has no [[gate]] for is refused, naming it.
    let said = agentd.refused(&["agents", "gate", "forge", "gitlab"]);
    assert!(said.contains("gitlab"), "{said}");
}

/// What the homeserver said to one of the peer's writes.
fn answer<T, E: std::fmt::Display>(result: Result<T, E>) -> Result<(), String> {
    result.map(|_| ()).map_err(|error| error.to_string())
}

fn forbidden(what: &str, result: Result<(), String>) {
    let error = result.expect_err(what);
    assert!(error.contains("M_FORBIDDEN"), "{what}: {error}");
}

/// 99.1 #8 (codemap Q3): in both of the gate's rooms — encrypted, and clear
/// — the peer's message is accepted, and its claim, its power levels, an
/// invite, a kick and a redaction are refused by the homeserver.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_room_power_levels_hold() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let peer = smoke.user("nixi-paced");
    let person = smoke.user("tgorka-smoke");
    let stranger = OwnedUserId::try_from(
        smoke
            .admin(
                reqwest::Method::GET,
                "/_matrix/client/v3/account/whoami",
                None,
            )
            .await["user_id"]
            .as_str()
            .expect("the admin's user id"),
    )
    .expect("user");
    let agentd = gate_host(&smoke, root.path()).await;
    agentd.ok(&["agents", "gate", "forge", "github"]);
    agentd.ok(&["agents", "gate", "forge", "ci"]);

    let peer_client = smoke
        .client(
            &root.path().join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    for (system, encrypted) in [("github", true), ("ci", false)] {
        let (_, _, session) = agentd.session_of(system);
        let room = session.room.clone();
        assert_eq!(
            session.gate.as_ref().map(|gate| gate.encrypted),
            Some(encrypted)
        );
        let state = admin_state(&smoke, &room).await;
        assert_eq!(
            state_of(&state, "m.room.encryption", "").is_some(),
            encrypted,
            "{system}"
        );
        let levels = state_of(&state, "m.room.power_levels", "").expect("levels");
        let levels_event = OwnedEventId::try_from(levels["event_id"].as_str().expect("event id"))
            .expect("event id");
        let mut taken = levels["content"].clone();
        taken["users"][peer.as_str()] = json!(100);

        peer_client.join(&room).await.expect("the peer joins");
        peer_client.sync_once().await.expect("sync");
        peer_client
            .send(
                &room,
                "m.room.message",
                json!({"msgtype": "m.text", "body": "what is the build's state?"}),
                None,
            )
            .await
            .unwrap_or_else(|error| panic!("{system}: the peer's message: {error}"));

        let joined = peer_client.client().get_room(&room).expect("joined");
        forbidden(
            "a claim",
            answer(
                joined
                    .send_state_event_raw(events::CLAIM, "", json!({"v": 1}))
                    .await,
            ),
        );
        forbidden(
            "the power levels",
            answer(
                joined
                    .send_state_event_raw("m.room.power_levels", "", taken)
                    .await,
            ),
        );
        forbidden(
            "an invite",
            answer(joined.invite_user_by_id(&stranger).await),
        );
        forbidden(
            "a kick",
            answer(joined.kick_user(&person, Some("the peer's")).await),
        );
        forbidden(
            "a redaction",
            answer(joined.redact(&levels_event, Some("the peer's"), None).await),
        );
    }
}
