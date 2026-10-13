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
use std::process::{Child, Command, Output, Stdio};
use std::time::Duration;

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
    /// The drive's bare remote.
    bare: PathBuf,
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

    /// `keeper-agentd run`, its log at `log`; stopped when dropped, a
    /// failed assertion included.
    fn start(&self, log: &Path) -> Running {
        let log = std::fs::File::create(log).expect("log");
        let mut command = Command::new(BIN);
        command.args(["run"]).env_clear();
        command.env("PATH", std::env::var("PATH").unwrap_or_default());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map(Running)
            .expect("keeper-agentd run")
    }

    /// Wait until the running host's status names `room`.
    async fn wait_serving(&self, room: &OwnedRoomId, log: &Path) {
        let status = self.home.join("state/keeper-agentd/status.json");
        let until = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if std::fs::read_to_string(&status).is_ok_and(|text| text.contains(room.as_str())) {
                return;
            }
            assert!(
                std::time::Instant::now() < until,
                "the host never served {room}:\n{}",
                std::fs::read_to_string(log).unwrap_or_default()
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
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

/// A running `keeper-agentd run`.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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
/// is signed in once a first `agents gate` has said it is not. With
/// `model`, the gate's model is the stub at that URL.
async fn gate_host(smoke: &Smoke, root: &Path, model: Option<&str>) -> Agentd {
    gate_host_with(smoke, root, model, Extra::default()).await
}

/// What a test adds to [`gate_host`]'s drive and host: `github`'s and
/// forge's extra lines, more files, the person's real pinned key.
#[derive(Default)]
struct Extra {
    github: String,
    forge: String,
    files: Vec<(String, String)>,
    pin: Option<String>,
    /// More agent ids of the drive the gate's host runs, `, "id"` each.
    hosted: String,
}

/// [`gate_host`] with `extra`.
async fn gate_host_with(smoke: &Smoke, root: &Path, model: Option<&str>, extra: Extra) -> Agentd {
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
         [[gate]]\nsystem = \"github\"\npeer = \"{peer}\"\naudience = [\"{peer}\", \"{person}\"]\nmodes = [\"sync\", \"async\"]\n{github}\n\n\
         [[gate]]\nsystem = \"ci\"\npeer = \"{peer}\"\naudience = [\"*\"]\nmodes = [\"async\"]\nencrypted = false\n\n\
         [model]\nbot = \"{bot}\"\n{forge_extra}",
        bot = model.map_or_else(|| BOT.to_owned(), |url| format!("bot:openai:{url}#m")),
        github = extra.github,
        forge_extra = extra.forge,
    );
    let providers = model.map_or_else(String::new, |url| {
        format!("\n[[providers]]\nkind = \"openai\"\nbase_url = \"{url}\"\n")
    });
    let mut files = vec![
        ("80-agents/_drive.toml".to_owned(), drive_toml),
        ("80-agents/forge/agent.toml".to_owned(), forge),
        ("80-agents/forge/SOUL.md".to_owned(), soul("Forge")),
    ];
    files.extend(extra.files);
    let bare = bare_drive(root, &files);
    let pin = extra.pin.unwrap_or_else(|| PIN.to_owned());
    let home = root.join("electra-sim");
    let config_dir = home.join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra-sim\"\nalways_on = true\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"forge\"{hosted}]\n\n[[trust]]\nuser = \"{person}\"\nmaster_key = \"{pin}\"\n{providers}",
            smoke.homeserver,
            bare.display(),
            hosted = extra.hosted,
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
        bare: bare.clone(),
    };
    let said = agentd.refused(&["agents", "gate", "forge", "github"]);
    assert!(said.contains(agent.as_str()), "the copy to sign in: {said}");
    assert!(!agentd
        .sessions()
        .iter()
        .any(|(_, _, agent)| agent.kind == SessionKind::Gate));
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
    let agentd = gate_host(&smoke, root.path(), None).await;

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
    let agentd = gate_host(&smoke, root.path(), None).await;
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

/// The gate's message about `request`, in `state`, once the peer's client
/// has it: its content and the homeserver's time.
async fn gate_says(
    seen: &std::sync::Mutex<Vec<(std::time::Instant, Value)>>,
    gate: &OwnedUserId,
    request: &str,
    state: &str,
    within: Duration,
) -> (Value, u64) {
    let until = std::time::Instant::now() + within;
    loop {
        let found = seen.lock().expect("lock").iter().find_map(|(_, event)| {
            let key = &event["content"][events::GATE];
            (event["sender"] == gate.as_str() && key["request"] == request && key["state"] == state)
                .then(|| {
                    (
                        event["content"].clone(),
                        event["origin_server_ts"].as_u64().expect("server time"),
                    )
                })
        });
        if let Some(found) = found {
            return found;
        }
        assert!(
            std::time::Instant::now() < until,
            "no {state} for {request} within {within:?}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The server time of the peer's own event `id`, as its client saw it.
fn server_time(seen: &std::sync::Mutex<Vec<(std::time::Instant, Value)>>, id: &str) -> u64 {
    seen.lock()
        .expect("lock")
        .iter()
        .find(|(_, event)| event["event_id"] == id)
        .and_then(|(_, event)| event["origin_server_ts"].as_u64())
        .expect("the request's server time")
}

/// 99.1 #7 (codemap §3 row 20): the sockets of this user's that listen —
/// a TCP row in state `0A`, any UDP row — and that no process it can inspect
/// owns, by inode. agentd makes itself non-dumpable (S-07), so its
/// `/proc/<pid>/fd` is closed to its own user and its sockets are the ones
/// no readable `fd` names; the caller drops what was there before it
/// started, as `live_turn`'s egress check does.
fn unowned_listeners() -> std::collections::BTreeSet<String> {
    use std::os::unix::fs::MetadataExt;
    let uid = std::fs::metadata("/proc/self").expect("self").uid();
    let mut owned = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir("/proc")
        .expect("proc")
        .filter_map(Result::ok)
    {
        let Ok(fds) = std::fs::read_dir(entry.path().join("fd")) else {
            continue;
        };
        for fd in fds.filter_map(Result::ok) {
            if let Some(inode) = std::fs::read_link(fd.path()).ok().and_then(|link| {
                link.to_str()?
                    .strip_prefix("socket:[")?
                    .strip_suffix(']')
                    .map(str::to_owned)
            }) {
                owned.insert(inode);
            }
        }
    }
    let mut found = std::collections::BTreeSet::new();
    for table in ["tcp", "tcp6", "udp", "udp6"] {
        let text = std::fs::read_to_string(format!("/proc/net/{table}")).unwrap_or_default();
        for row in text.lines().skip(1) {
            let fields: Vec<&str> = row.split_whitespace().collect();
            let (Some(state), Some(row_uid), Some(inode)) =
                (fields.get(3), fields.get(7), fields.get(9))
            else {
                continue;
            };
            let listens = table.starts_with("udp") || *state == "0A";
            if listens && row_uid.parse::<u32>().ok() == Some(uid) && !owned.contains(*inode) {
                found.insert(format!("{table} {}", fields[1]));
            }
        }
    }
    found
}

/// 99.1 #3 (codemap Q6, Q10) and #7: a gate host serving its room, the
/// outside system's sync request with `deadline_ms: 3000` gets one `answer`
/// referencing it when the model answers in 500 ms; when the model takes
/// 10 s it gets its `ticket` 3 000 ± 300 ms after the request by the
/// homeserver's timestamps, and later the work's `result` against that
/// ticket. Meanwhile the host listens on nothing.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_sync_answers_or_tickets_at_the_deadline() {
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let gate = smoke.user("nixi-smoke");
    let stub = common::calls_paced(
        vec![
            (
                1,
                "reply",
                json!({"text": "the build is green"}),
                Duration::from_millis(500),
            ),
            (
                3,
                "reply",
                json!({"text": "the long report"}),
                Duration::from_secs(10),
            ),
        ],
        "ok",
    );
    let agentd = gate_host(&smoke, root.path(), Some(&stub.url)).await;
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (_, _, session) = agentd.session_of("github");
    let room = session.room.clone();
    let log = root.path().join("agentd.log");
    let before = unowned_listeners();
    let _host = agentd.start(&log);
    agentd.wait_serving(&room, &log).await;

    let peer = smoke
        .client(
            &root.path().join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    let seen = common::record(&peer);
    peer.join(&room).await.expect("the peer joins");
    peer.sync_once().await.expect("sync");
    let _sync = common::syncing(&peer);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let ask = |request: &str| json!({"msgtype": "m.text", "body": format!("{request}: how is the build?"), (events::GATE): {"request": request, "deadline_ms": 3000}});
    let r1 = peer
        .send(&room, "m.room.message", ask("r1"), None)
        .await
        .expect("r1");
    let (answer, _) = gate_says(&seen, &gate, "r1", "answer", Duration::from_secs(30)).await;
    assert_eq!(answer["body"], "the build is green");
    assert_eq!(answer[events::GATE]["deadline_ms"], 3000);
    assert_eq!(answer["m.relates_to"]["event_id"], r1.as_str());
    assert_eq!(answer["m.relates_to"]["rel_type"], "m.reference");

    let r2 = peer
        .send(&room, "m.room.message", ask("r2"), None)
        .await
        .expect("r2");
    let (ticket, ticket_at) =
        gate_says(&seen, &gate, "r2", "ticket", Duration::from_secs(30)).await;
    let asked_at = server_time(&seen, r2.as_str());
    let late = ticket_at.saturating_sub(asked_at);
    eprintln!("the ticket came {late} ms after the request by the server's clock");
    assert!(
        (2_700..=3_300).contains(&late),
        "the ticket came {late} ms after the request by the server's clock"
    );
    assert_eq!(ticket["m.relates_to"]["event_id"], r2.as_str());
    let listening: Vec<String> = unowned_listeners().difference(&before).cloned().collect();
    assert!(listening.is_empty(), "the gate host listens: {listening:?}");

    let (result, _) = gate_says(&seen, &gate, "r2", "result", Duration::from_secs(60)).await;
    assert_eq!(
        result[events::GATE]["ticket"],
        ticket[events::GATE]["ticket"]
    );
    assert_eq!(result["body"], "the long report");
    assert!(
        seen.lock().expect("lock").iter().all(|(_, event)| {
            event["sender"] != gate.as_str()
                || event["content"][events::GATE]["request"] != "r1"
                || event["content"][events::GATE]["state"] == "answer"
        }),
        "r1 was answered once and nothing else was said of it"
    );
}

/// Dr Tola Grey's own host (`tola-smoke`, a steward) over the gate host's
/// drive at `bare`, signed in with `password`; `model` its stub; the person
/// pinned with `pin`.
fn steward_host(
    smoke: &Smoke,
    root: &Path,
    bare: &Path,
    model: &str,
    pin: &str,
    password: &str,
) -> Agentd {
    let person = smoke.user("tgorka-smoke");
    let home = root.join("tola-sim");
    let config_dir = home.join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"tola-sim\"\n\n[homeserver]\nurl = \"{}\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"tola\"]\n\n[[trust]]\nuser = \"{person}\"\nmaster_key = \"{pin}\"\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{model}\"\n",
            smoke.homeserver,
            bare.display()
        ),
    )
    .expect("agentd.toml");
    let dir = |name: &str| home.join(name).display().to_string();
    let agentd = Agentd {
        env: vec![
            ("HOME".to_owned(), dir("")),
            ("XDG_CONFIG_HOME".to_owned(), dir("config")),
            ("XDG_DATA_HOME".to_owned(), dir("data")),
            ("XDG_STATE_HOME".to_owned(), dir("state")),
            (
                "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
                password.to_owned(),
            ),
        ],
        home,
        bare: bare.to_owned(),
    };
    agentd.ok(&[
        "login",
        "smoke/tola",
        "--password-credential",
        "agent_password",
    ]);
    agentd
}

/// Every log line of every session of `agentd`'s checkout, as JSON.
fn lines_of(agentd: &Agentd) -> Vec<Value> {
    let mut out = Vec::new();
    let active = agentd.drive().join("60-sessions/active");
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
    out
}

/// What a failed live test shows of a host's lines: kind and state.
fn told(lines: &[Value]) -> String {
    lines
        .iter()
        .map(|l| {
            format!(
                "{} {} {}",
                l["kind"], l["body"]["state"], l["body"]["reason"]
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a failed live test shows of a host's log: its warnings and errors.
fn told_log(log: &Path) -> String {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("WARN") || line.contains("ERROR"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The room's events in order, as the server holds them.
async fn timeline(smoke: &Smoke, room: &str) -> Vec<Value> {
    smoke
        .admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v1/rooms/{room}/messages?dir=f&limit=500"),
            None,
        )
        .await["chunk"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// tgorka's device A, its identity made and signed by itself; the master
/// key a host pins for him.
async fn person_device(
    smoke: &Smoke,
    dir: &Path,
) -> (keeper_core::agents::matrix::AgentClient, String) {
    let person = smoke.user("tgorka-smoke");
    smoke.clear_devices(&person).await;
    let device = smoke
        .client(dir, "tgorka-smoke", smoke.secret("TGORKA_SMOKE_PASSWORD"))
        .await;
    common::bootstrap(&device, &person, smoke.secret("TGORKA_SMOKE_PASSWORD")).await;
    let pin = device
        .published_master_key(&person)
        .await
        .expect("the homeserver answers")
        .expect("tgorka publishes an identity");
    (device, pin)
}

/// The cards `seen` holds from `gate`: each `approval.request` event.
fn cards_of(
    seen: &std::sync::Mutex<Vec<(std::time::Instant, Value)>>,
    gate: &OwnedUserId,
) -> Vec<Value> {
    seen.lock()
        .expect("lock")
        .iter()
        .filter(|(_, event)| {
            event["type"] == events::APPROVAL_REQUEST && event["sender"] == gate.as_str()
        })
        .map(|(_, event)| event.clone())
        .collect()
}

/// A person's decision on the card record `record`.
fn decision_on(record: &Value, decision: keeper_core::agents::approval::Decision) -> Value {
    json!({
        "id": record["id"],
        "binding_digest": record["binding_digest"],
        "decision": decision,
        "scope": "once",
    })
}

async fn wait_for(what: &str, within: Duration, done: impl Fn() -> bool) {
    let until = std::time::Instant::now() + within;
    while !done() {
        assert!(std::time::Instant::now() < until, "{what}");
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// 99.1 #4 (codemap Q2, Q4, Q5): the peer's plain message is ticketed
/// within a second of it by the homeserver's clock, and its delegation is
/// asked for on the hour's card the person's device draws. The peer's own
/// decision on it changes nothing; the person's approval from his verified
/// device hands it to Dr Tola Grey's host in 92.1's order — the brief only
/// after Tola joined — her session made with the gate session as `parent`
/// and the gate's user as `requested_by`; her reply comes back to the
/// gate's callback turn, whose `reply` the room's sink blocks: Tola's prompt
/// read her home, which the peer may not read, so nothing reaches the peer;
/// its release is a declassification in tgorka's proxy DM (99.2 #3,
/// `gate_reply_carrying_drive_content_needs_declassification`), and this
/// host runs no proxy of his, so the request is refused `unreleased`.
/// A denied ticket is refused `denied`. The gate room's members at the end
/// are the gate, the peer and the person.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_async_ticket_session_callback() {
    use keeper_core::agents::approval::Decision;
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let gate = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let person = smoke.user("tgorka-smoke");
    let tola_password = ulid::Ulid::new().to_string();
    smoke
        .admin(
            reqwest::Method::PUT,
            &format!("/_synapse/admin/v2/users/{tola}"),
            Some(json!({ "password": tola_password, "admin": false })),
        )
        .await;
    smoke.clear_devices(&tola).await;
    smoke.set_ratelimit(&tola, 0, 0).await;
    let (device, pin) = person_device(&smoke, &root.path().join("person")).await;

    let gates_model = common::calls_at(
        vec![
            (
                1,
                "delegate",
                json!({"agent": "smoke/tola", "brief": "Say where the v1.2 notes are."}),
            ),
            (3, "reply", json!({"text": "The notes are in 30-releases."})),
            // The callback's turn ends at its held `reply` (99.2 #3): the
            // second request's turn is the model's fourth request.
            (
                4,
                "delegate",
                json!({"agent": "smoke/tola", "brief": "Tag v1.3."}),
            ),
        ],
        "ok",
    );
    let tolas_model = common::tool_then("reply", json!({"text": "In 30-releases."}), "Replied.");
    let tola_toml = format!(
        "version = 1\nid = \"tola\"\nname = \"Dr Tola Grey\"\nkind = \"steward\"\nmatrix_user = \"{tola}\"\n\n[model]\nbot = \"bot:openai:{}#stub\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"smoke\"]\n",
        tolas_model.url
    );
    let agentd = gate_host_with(
        &smoke,
        root.path(),
        Some(&gates_model.url),
        Extra {
            github: "delegates = [\"tola\"]".to_owned(),
            files: vec![
                ("80-agents/tola/agent.toml".to_owned(), tola_toml),
                ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
            ],
            pin: Some(pin.clone()),
            ..Extra::default()
        },
    )
    .await;
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (folder, _, session) = agentd.session_of("github");
    let room = session.room.clone();
    let log = root.path().join("agentd.log");
    let _host = agentd.start(&log);
    agentd.wait_serving(&room, &log).await;
    let stewards = steward_host(
        &smoke,
        root.path(),
        &agentd.bare,
        &tolas_model.url,
        &pin,
        &tola_password,
    );
    let tola_log = root.path().join("tola.log");
    let _tolas = stewards.start(&tola_log);

    let seen_by_person = common::record(&device);
    device.join(&room).await.expect("the person joins");
    let _person_sync = common::syncing(&device);
    let peer = smoke
        .client(
            &root.path().join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    let seen = common::record(&peer);
    peer.join(&room).await.expect("the peer joins");
    peer.sync_once().await.expect("sync");
    let _sync = common::syncing(&peer);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let asked = peer
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "where are the v1.2 notes?"}),
            None,
        )
        .await
        .expect("the request");
    let (ticket, ticket_at) = gate_says(
        &seen,
        &gate,
        asked.as_str(),
        "ticket",
        Duration::from_secs(30),
    )
    .await;
    let asked_at = server_time(&seen, asked.as_str());
    assert!(
        ticket_at.saturating_sub(asked_at) <= 1_000,
        "the ticket came {} ms after",
        ticket_at - asked_at
    );
    let ticket_id = ticket[events::GATE]["ticket"]
        .as_str()
        .expect("its ticket")
        .to_owned();

    wait_for(
        "no card reached the person",
        Duration::from_secs(60),
        || !cards_of(&seen_by_person, &gate).is_empty(),
    )
    .await;
    let card = cards_of(&seen_by_person, &gate).remove(0);
    let record = card["content"]["records"][0].clone();
    assert_eq!(record["action"]["tool"], "delegate", "{card}");
    assert_eq!(
        record["action"]["exec_binding"]["ticket"],
        ticket_id.as_str()
    );
    assert_eq!(record["approvers"], json!([person.as_str()]));
    assert_eq!(record["scopes"], json!(["once"]));

    // The outside system decides for itself: nothing moves.
    peer.send(
        &room,
        events::APPROVAL_DECISION,
        decision_on(&record, Decision::Approve),
        None,
    )
    .await
    .expect("the peer's decision is accepted by the server");
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert!(
        !lines_of(&agentd)
            .iter()
            .any(|l| l["kind"] == "delegate" && l["body"]["state"] == "opened"),
        "the peer's decision handed nothing on"
    );

    device
        .send(
            &room,
            events::APPROVAL_DECISION,
            decision_on(&record, Decision::Approve),
            None,
        )
        .await
        .expect("the person decides");
    // Tola's reply carries her home's label — her prompt read the drive,
    // whose readers are tgorka alone — so the callback's `reply` into a room
    // the peer reads is blocked by the room's sink, and nothing of it
    // reaches the peer; releasing it is a declassification (99.2 #3), which
    // nobody can be asked for from this host. The request ends refused.
    let until = std::time::Instant::now() + Duration::from_secs(240);
    while !seen.lock().expect("lock").iter().any(|(_, e)| {
        e["sender"] == gate.as_str()
            && e["content"][events::GATE]["request"] == asked.as_str()
            && e["content"][events::GATE]["state"] == "refused"
    }) {
        assert!(
            std::time::Instant::now() < until,
            "the callback never ended within 240 s.\ngate's lines:\n{}\ngate's host:\n{}\ntola's lines:\n{}\ntola's host:\n{}",
            told(&lines_of(&agentd)),
            told_log(&log),
            told(&lines_of(&stewards)),
            told_log(&tola_log),
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let lines = lines_of(&agentd);
    let narrowed = lines
        .iter()
        .skip_while(|l| !(l["kind"] == "delegate" && l["body"]["state"] == "replied"))
        .find(|l| l["kind"] == "label")
        .expect("the reply's label joined the callback's");
    assert_eq!(narrowed["body"]["readers"], json!([person.as_str()]));
    let blocked = lines
        .iter()
        .skip_while(|l| !(l["kind"] == "delegate" && l["body"]["state"] == "replied"))
        .find(|l| l["kind"] == "tool_result")
        .expect("the callback's reply");
    assert_eq!(blocked["body"]["outcome"], "refused", "{blocked}");
    let refused = lines
        .iter()
        .find(|l| l["kind"] == "gate" && l["body"]["state"] == "refused")
        .expect("the request's refusal");
    assert_eq!(refused["body"]["reason"], "unreleased", "{refused}");

    let lines = lines_of(&agentd);
    let states: Vec<&str> = lines
        .iter()
        .filter(|l| l["kind"] == "delegate" && l["body"]["id"] == ticket_id.as_str())
        .filter_map(|l| l["body"]["state"].as_str())
        .collect();
    assert_eq!(
        states,
        ["requested", "opened", "sent", "replied"],
        "{lines:#?}"
    );
    let child = lines
        .iter()
        .find(|l| l["kind"] == "delegate" && l["body"]["state"] == "opened")
        .and_then(|l| l["body"]["room"].as_str())
        .expect("the delegation's room")
        .to_owned();
    // F5: the brief went after Tola's join.
    let order: Vec<(String, String)> = timeline(&smoke, &child)
        .await
        .into_iter()
        .map(|e| {
            (
                e["type"].as_str().unwrap_or("").to_owned(),
                e["sender"].as_str().unwrap_or("").to_owned(),
            )
        })
        .collect();
    let joined_at = order
        .iter()
        .position(|(kind, sender)| kind == "m.room.member" && sender == tola.as_str())
        .expect("Tola joined");
    let brief_at = order
        .iter()
        .position(|(kind, sender)| kind == "m.room.encrypted" && sender == gate.as_str())
        .expect("the brief");
    assert!(
        joined_at < brief_at,
        "the brief waited for the join: {order:?}"
    );
    let tolas_session = stewards
        .sessions()
        .into_iter()
        .find(|(_, _, agent)| agent.kind == SessionKind::Delegated)
        .expect("Tola's session");
    assert_eq!(tolas_session.2.requested_by, gate);
    let folder = folder
        .file_name()
        .and_then(|name| name.to_str())
        .expect("its folder");
    assert_eq!(
        tolas_session
            .2
            .parent
            .as_ref()
            .map(|parent| parent.session.clone()),
        Some(format!("active/{folder}"))
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    let results = seen
        .lock()
        .expect("lock")
        .iter()
        .filter(|(_, e)| {
            e["sender"] == gate.as_str() && e["content"][events::GATE]["state"] == "result"
        })
        .count();
    assert_eq!(
        results, 0,
        "nothing narrower than the room reached the peer"
    );

    // A second request, denied.
    let again = peer
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "tag v1.3"}),
            None,
        )
        .await
        .expect("the second request");
    gate_says(
        &seen,
        &gate,
        again.as_str(),
        "ticket",
        Duration::from_secs(30),
    )
    .await;
    wait_for(
        "the second ticket reached no card",
        Duration::from_secs(60),
        || cards_of(&seen_by_person, &gate).len() >= 2,
    )
    .await;
    let edit = cards_of(&seen_by_person, &gate).remove(1);
    let second = edit["content"]["m.new_content"]["records"][1].clone();
    device
        .send(
            &room,
            events::APPROVAL_DECISION,
            decision_on(&second, Decision::Deny),
            None,
        )
        .await
        .expect("the person declines");
    let (refused, _) = gate_says(
        &seen,
        &gate,
        again.as_str(),
        "refused",
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(refused[events::GATE]["reason"], "denied");

    let members: std::collections::BTreeSet<String> = admin_state(&smoke, &room)
        .await
        .into_iter()
        .filter(|e| e["type"] == "m.room.member" && e["content"]["membership"] == "join")
        .filter_map(|e| e["state_key"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        members,
        [
            gate.to_string(),
            smoke.user("nixi-paced").to_string(),
            person.to_string()
        ]
        .into(),
    );
}

/// 99.1 #10 (S-23, codemap Q8): five tickets of one peer inside the hour
/// make one card — one request event and four edits whose new content
/// lists all five records, as the person's device receives them; two
/// decisions consume exactly their two records. (The hour's end opening a
/// new card is `gate_cards_coalesce_per_peer_per_window` in agent_turns,
/// on a clock the test moves.)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_cards_coalesce_per_peer_per_window() {
    use keeper_core::agents::approval::Decision;
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let gate = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let (device, pin) = person_device(&smoke, &root.path().join("person")).await;
    let hand = || json!({"agent": "smoke/tola", "brief": "Run the job."});
    let gates_model = common::calls_at(
        (0..5).map(|n| (2 * n + 1, "delegate", hand())).collect(),
        "ok",
    );
    let agentd = gate_host_with(
        &smoke,
        root.path(),
        Some(&gates_model.url),
        Extra {
            github: "delegates = [\"tola\"]".to_owned(),
            forge: "\n[limits]\nmax_concurrent_sessions = 8\n".to_owned(),
            files: vec![
                (
                    "80-agents/tola/agent.toml".to_owned(),
                    format!("version = 1\nid = \"tola\"\nname = \"Dr Tola Grey\"\nkind = \"steward\"\nmatrix_user = \"{tola}\"\n\n[model]\nbot = \"{BOT}\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"smoke\"]\n"),
                ),
                ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
            ],
            pin: Some(pin),
            ..Extra::default()
        },
    )
    .await;
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (_, _, session) = agentd.session_of("github");
    let room = session.room.clone();
    let log = root.path().join("agentd.log");
    let _host = agentd.start(&log);
    agentd.wait_serving(&room, &log).await;

    let seen_by_person = common::record(&device);
    device.join(&room).await.expect("the person joins");
    let _person_sync = common::syncing(&device);
    let peer = smoke
        .client(
            &root.path().join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    let seen = common::record(&peer);
    peer.join(&room).await.expect("the peer joins");
    peer.sync_once().await.expect("sync");
    let _sync = common::syncing(&peer);
    tokio::time::sleep(Duration::from_secs(2)).await;

    for n in 1..=5 {
        let asked = peer
            .send(
                &room,
                "m.room.message",
                json!({"msgtype": "m.text", "body": format!("job {n}")}),
                None,
            )
            .await
            .expect("a request");
        gate_says(
            &seen,
            &gate,
            asked.as_str(),
            "ticket",
            Duration::from_secs(30),
        )
        .await;
        wait_for("a ticket reached no card", Duration::from_secs(60), || {
            cards_of(&seen_by_person, &gate).len() >= n
        })
        .await;
    }
    let cards = cards_of(&seen_by_person, &gate);
    assert_eq!(cards.len(), 5, "one request and four edits");
    let original = cards[0]["event_id"].as_str().expect("its event");
    assert!(cards[0]["content"]["m.relates_to"].is_null());
    assert_eq!(
        cards[0]["content"]["records"].as_array().map(Vec::len),
        Some(1)
    );
    for (n, edit) in cards.iter().enumerate().skip(1) {
        assert_eq!(edit["content"]["m.relates_to"]["rel_type"], "m.replace");
        assert_eq!(edit["content"]["m.relates_to"]["event_id"], original);
        let records = edit["content"]["m.new_content"]["records"]
            .as_array()
            .expect("records");
        assert_eq!(records.len(), n + 1);
        assert_eq!(
            records[..n],
            cards[n - 1]["content"]
                .get("m.new_content")
                .unwrap_or(&cards[n - 1]["content"])["records"]
                .as_array()
                .expect("the earlier list")[..]
        );
    }
    let all = cards[4]["content"]["m.new_content"]["records"]
        .as_array()
        .expect("five")
        .clone();
    for record in &all[..2] {
        device
            .send(
                &room,
                events::APPROVAL_DECISION,
                decision_on(record, Decision::Approve),
                None,
            )
            .await
            .expect("the person decides");
    }
    wait_for(
        "two records were not consumed",
        Duration::from_secs(60),
        || {
            lines_of(&agentd)
                .iter()
                .filter(|l| l["kind"] == "approval" && l["body"]["state"] == "consumed")
                .count()
                >= 2
        },
    )
    .await;
    tokio::time::sleep(Duration::from_secs(5)).await;
    let consumed: Vec<String> = admin_state(&smoke, &room)
        .await
        .into_iter()
        .filter(|e| e["type"] == events::APPROVAL_CONSUMED)
        .filter_map(|e| e["state_key"].as_str().map(str::to_owned))
        .collect();
    let decided: std::collections::BTreeSet<String> = all[..2]
        .iter()
        .filter_map(|record| record["id"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        consumed
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        decided
    );
}

/// What tgorka's note says, and where it is: what a status or a scope in
/// the gate room must never name (99.2 #3).
const NOTE_PATH: &str = "10-notes/bluebird-v12-plan.md";
const NOTE_TITLE: &str = "Bluebird v1.2 plan";
const NOTE_HEADING: &str = "Launch codename Bluebird";
/// What the gate's callback answers with, once released.
const RELEASED: &str = "The v1.2 plan is to ship on Friday.";

/// A gate's ticket whose delegate read tgorka's note, held for his
/// declassification in his proxy's DM — everything a 99.2 #3 test goes on
/// from.
struct Held {
    _hosts: (Running, Running),
    agentd: Agentd,
    log: PathBuf,
    gate: OwnedUserId,
    room: OwnedRoomId,
    dm: OwnedRoomId,
    device: keeper_core::agents::matrix::AgentClient,
    /// What the peer's own client was handed.
    seen: std::sync::Arc<std::sync::Mutex<Vec<(std::time::Instant, Value)>>>,
    asked: OwnedEventId,
    ticket: String,
    /// The declassification's request, as tgorka's device drew it.
    card: Value,
    _syncs: Vec<tokio::task::JoinHandle<()>>,
}

/// The gate `forge` with Nixi, tgorka's proxy (the account `proxy-smoke`),
/// on one host, and Dr Tola Grey on another. The peer's request is
/// ticketed and approved on the hour's card; Tola reads tgorka's note and
/// replies; the callback's `reply` is held, and its declassification asked
/// in tgorka's DM with Nixi.
async fn held_in_the_proxy_dm(smoke: &Smoke, root: &Path) -> Held {
    use keeper_core::agents::approval::Decision;
    let gate = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let proxy = smoke.user("proxy-smoke");
    let person = smoke.user("tgorka-smoke");
    let tola_password = ulid::Ulid::new().to_string();
    let proxy_password = ulid::Ulid::new().to_string();
    for (user, password) in [(&tola, &tola_password), (&proxy, &proxy_password)] {
        smoke
            .admin(
                reqwest::Method::PUT,
                &format!("/_synapse/admin/v2/users/{user}"),
                Some(json!({ "password": password, "admin": false })),
            )
            .await;
        smoke.clear_devices(user).await;
        smoke.set_ratelimit(user, 0, 0).await;
    }
    let (device, pin) = person_device(smoke, &root.join("person")).await;

    // Nixi's DM with tgorka is its `main` session, under the id its door
    // is found by.
    let maker = smoke
        .client(&root.join("proxy"), "proxy-smoke", &proxy_password)
        .await;
    let dm = maker
        .create_room(
            RoomKind::Session(SessionKind::Main),
            "smoke",
            vec![person.clone()],
            &[],
        )
        .await
        .expect("the DM");
    let drive_toml = format!(
        "version = 1\nid = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n"
    );
    let decl = keeper_core::agents::drive::parse(&drive_toml).expect("decl");
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
        label: keeper_core::agents::label::Label {
            readers: Readers::Only(std::collections::BTreeSet::from([person.clone()])),
            ..keeper_core::agents::label::Label::opening(&decl, Integrity::Owner)
        },
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: Vec::new(),
        limits: None,
        workflow: None,
        checkpoints: None,
        outputs: Vec::new(),
        created_at: chrono::Utc::now(),
        gate: None,
    };

    let gates_model = common::calls_at(
        vec![
            (
                1,
                "delegate",
                json!({"agent": "smoke/tola", "brief": "What is the v1.2 plan?"}),
            ),
            (3, "reply", json!({ "text": RELEASED })),
        ],
        "ok",
    );
    let tolas_model = common::calls_at(
        vec![
            (
                1,
                "drive_read",
                json!({"profile": "smoke", "path": NOTE_PATH}),
            ),
            (2, "reply", json!({"text": "Ship on Friday."})),
        ],
        "Replied.",
    );
    let tola_toml = format!(
        "version = 1\nid = \"tola\"\nname = \"Dr Tola Grey\"\nkind = \"steward\"\nmatrix_user = \"{tola}\"\n\n[model]\nbot = \"bot:openai:{}#stub\"\n\n[tools]\nallow = [\"drive_read\"]\ndrives = [\"smoke\"]\n",
        tolas_model.url
    );
    let nixi_toml = format!(
        "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"{proxy}\"\nhuman = \"{person}\"\n\n[model]\nbot = \"bot:openai:{}#m\"\n",
        gates_model.url
    );
    let mut agentd = gate_host_with(
        smoke,
        root,
        Some(&gates_model.url),
        Extra {
            github: "delegates = [\"tola\"]".to_owned(),
            files: vec![
                ("80-agents/tola/agent.toml".to_owned(), tola_toml),
                ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
                ("80-agents/nixi/agent.toml".to_owned(), nixi_toml),
                ("80-agents/nixi/SOUL.md".to_owned(), soul("Nixi")),
                (
                    NOTE_PATH.to_owned(),
                    format!(
                        "---\ntitle: {NOTE_TITLE}\n---\n\n# {NOTE_HEADING}\n\nShip on Friday.\n"
                    ),
                ),
                (
                    "60-sessions/active/2026-10-12-nixi/agent.toml".to_owned(),
                    keeper_core::agents::session::compose_session_agent_toml(&main),
                ),
                (
                    "60-sessions/active/2026-10-12-nixi/README.md".to_owned(),
                    format!("---\nid: {}\ntitle: smoke\n---\n", main.id),
                ),
            ],
            pin: Some(pin.clone()),
            hosted: ", \"nixi\"".to_owned(),
            ..Extra::default()
        },
    )
    .await;
    agentd.env.push((
        "KEEPER_AGENTD_SECRET_PROXY_PASSWORD".to_owned(),
        proxy_password.clone(),
    ));
    agentd.ok(&[
        "login",
        "smoke/nixi",
        "--password-credential",
        "proxy_password",
    ]);
    agentd.ok(&["agents", "gate", "forge", "github"]);
    let (_, _, session) = agentd.session_of("github");
    let room = session.room.clone();
    let log = root.join("agentd.log");
    let gate_host = agentd.start(&log);
    agentd.wait_serving(&room, &log).await;
    let stewards = steward_host(
        smoke,
        root,
        &agentd.bare,
        &tolas_model.url,
        &pin,
        &tola_password,
    );
    let tola_log = root.join("tola.log");
    let tolas_host = stewards.start(&tola_log);

    let seen_by_person = common::record(&device);
    device
        .join(&room)
        .await
        .expect("the person joins the gate room");
    device.join(&dm).await.expect("the person joins the DM");
    let person_sync = common::syncing(&device);
    let peer = smoke
        .client(
            &root.join("peer"),
            "nixi-paced",
            smoke.secret("NIXI_PACED_PASSWORD"),
        )
        .await;
    let seen = common::record(&peer);
    peer.join(&room).await.expect("the peer joins");
    peer.sync_once().await.expect("sync");
    let peer_sync = common::syncing(&peer);
    tokio::time::sleep(Duration::from_secs(2)).await;

    let asked = peer
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "what is the v1.2 plan?"}),
            None,
        )
        .await
        .expect("the request");
    let (said, _) = gate_says(
        &seen,
        &gate,
        asked.as_str(),
        "ticket",
        Duration::from_secs(30),
    )
    .await;
    let ticket = said[events::GATE]["ticket"]
        .as_str()
        .expect("its ticket")
        .to_owned();
    wait_for(
        "no card reached the person",
        Duration::from_secs(60),
        || !cards_of(&seen_by_person, &gate).is_empty(),
    )
    .await;
    let record = cards_of(&seen_by_person, &gate).remove(0)["content"]["records"][0].clone();
    device
        .send(
            &room,
            events::APPROVAL_DECISION,
            decision_on(&record, Decision::Approve),
            None,
        )
        .await
        .expect("the person hands it on");

    // Tola reads the note and replies; the gate's callback is held, and
    // its declassification comes to tgorka's DM from Nixi.
    let in_dm = || {
        seen_by_person
            .lock()
            .expect("lock")
            .iter()
            .filter(|(_, e)| {
                e["type"] == events::APPROVAL_REQUEST
                    && e["sender"] == proxy.as_str()
                    && e["content"]["action"]["tool"] == "declassify"
            })
            .map(|(_, e)| e["content"].clone())
            .collect::<Vec<Value>>()
    };
    let until = std::time::Instant::now() + Duration::from_secs(240);
    while in_dm().is_empty() {
        assert!(
            std::time::Instant::now() < until,
            "no declassification reached tgorka's DM within 240 s.\ngate's lines:\n{}\ngate's host:\n{}\ntola's lines:\n{}\ntola's host:\n{}",
            told(&lines_of(&agentd)),
            told_log(&log),
            told(&lines_of(&stewards)),
            told_log(&tola_log),
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let card = in_dm().remove(0);
    Held {
        _hosts: (gate_host, tolas_host),
        agentd,
        log,
        gate,
        room,
        dm,
        device,
        seen,
        asked,
        ticket,
        card,
        _syncs: vec![person_sync, peer_sync],
    }
}

/// The gate's messages the peer saw about `request` in state `state`.
fn peer_saw(held: &Held, state: &str) -> Vec<Value> {
    held.seen
        .lock()
        .expect("lock")
        .iter()
        .filter(|(_, e)| {
            e["sender"] == held.gate.as_str()
                && e["content"][events::GATE]["request"] == held.asked.as_str()
                && e["content"][events::GATE]["state"] == state
        })
        .map(|(_, e)| e["content"].clone())
        .collect()
}

/// 99.2 #3 against the real server (R85, R89 in a gate): a ticket's
/// result whose delegate read tgorka's note is blocked from the room the
/// peer reads — the peer's sync holds no result — and its
/// declassification, T4 and `once`, is asked in tgorka's DM with Nixi
/// (Nixi never joins the gate room, S-27). His approval from his verified
/// device lets exactly that result go, once, as the ticket's `result`.
/// (A request needing no drive is answered as it was:
/// `gate_sync_answers_or_tickets_at_the_deadline`.)
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_reply_carrying_drive_content_needs_declassification() {
    use keeper_core::agents::approval::Decision;
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let held = held_in_the_proxy_dm(&smoke, root.path()).await;
    assert_eq!(held.card["tier"], 4, "{}", held.card);
    assert_eq!(held.card["scopes"], json!(["once"]));
    assert_eq!(
        held.card["action"]["args"]["readers"],
        json!([smoke.user("nixi-paced").as_str()])
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(
        peer_saw(&held, "result").is_empty(),
        "nothing reached the peer"
    );
    let lines = lines_of(&held.agentd);
    assert!(
        lines
            .iter()
            .any(|l| l["kind"] == "gate" && l["body"]["state"] == "held"),
        "{}",
        told(&lines)
    );

    held.device
        .send(
            &held.dm,
            events::APPROVAL_DECISION,
            decision_on(&held.card, Decision::Approve),
            None,
        )
        .await
        .expect("tgorka lets it out");
    let (result, _) = gate_says(
        &held.seen,
        &held.gate,
        held.asked.as_str(),
        "result",
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(result["body"], RELEASED, "{result}");
    assert_eq!(result[events::GATE]["ticket"], held.ticket.as_str());
    tokio::time::sleep(Duration::from_secs(5)).await;
    assert_eq!(peer_saw(&held, "result").len(), 1, "exactly once");
    let members: std::collections::BTreeSet<String> = admin_state(&smoke, &held.room)
        .await
        .into_iter()
        .filter(|e| e["type"] == "m.room.member" && e["content"]["membership"] == "join")
        .filter_map(|e| e["state_key"].as_str().map(str::to_owned))
        .collect();
    assert!(
        !members.contains(smoke.user("proxy-smoke").as_str()),
        "the proxy never joined the gate room: {members:?}"
    );
    let _ = &held.log;
}

/// 99.2 #3's second half against the real server (S-16): after the
/// delegate read tgorka's note, nothing the peer's client was handed — no
/// status, no scope, no message — names its path, its title or its
/// heading, and no status or scope reaches the gate room at all; tgorka
/// declining the declassification leaves the peer a `refused` result
/// naming why (`withheld`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn gate_status_never_names_what_it_read() {
    use keeper_core::agents::approval::Decision;
    let smoke = Smoke::from_env();
    let root = tempfile::tempdir().expect("tempdir");
    let held = held_in_the_proxy_dm(&smoke, root.path()).await;
    held.device
        .send(
            &held.dm,
            events::APPROVAL_DECISION,
            decision_on(&held.card, Decision::Deny),
            None,
        )
        .await
        .expect("tgorka declines");
    let (refused, _) = gate_says(
        &held.seen,
        &held.gate,
        held.asked.as_str(),
        "refused",
        Duration::from_secs(60),
    )
    .await;
    assert_eq!(refused[events::GATE]["reason"], "withheld", "{refused}");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let seen = held.seen.lock().expect("lock").clone();
    for (_, event) in &seen {
        let text = event.to_string();
        for named in [
            NOTE_PATH,
            "bluebird",
            NOTE_TITLE,
            NOTE_HEADING,
            "Ship on Friday",
        ] {
            assert!(!text.contains(named), "the peer saw {named:?} in {text}");
        }
        assert!(
            event["type"] != events::STATUS && event["type"] != events::SCOPE,
            "a status or scope reached the gate room: {text}"
        );
    }
    assert!(peer_saw(&held, "result").is_empty());
    let _ = (&held.log, &held.room, &held.ticket);
}
