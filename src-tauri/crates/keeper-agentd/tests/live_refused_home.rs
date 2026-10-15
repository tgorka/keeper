//! A delegation to a home this host refuses says why — to a session that
//! home's drive may reach, and to no other — against a real homeserver
//! (story 101.6 #3, R422, R436).
//!
//! `#[ignore]`: it needs the Synapse test homeserver and its users, from
//! the environment as `live_turn.rs` reads them:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_refused_home -- --ignored --nocapture
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use keeper_core::agents::label::{Integrity, Label};
use keeper_core::agents::matrix::RoomKind;
use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
use matrix_sdk::ruma::OwnedUserId;
use serde_json::{json, Value};

mod common;

use common::{bare_drive, calls_at, record, syncing, Smoke};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
const SESSION: &str = "active/2026-10-15-smoke";

/// One `keeper-agentd run` hosting Nixi.
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
        for active in sessions(self.root.path()) {
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

/// Every `active` directory of a sessions zone under `root`.
fn sessions(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) || path.ends_with(".git") {
                continue;
            }
            if path.ends_with("active") && path.parent().is_some_and(|p| p.ends_with("60-sessions"))
            {
                out.push(path.clone());
            }
            stack.push(path);
        }
    }
    out
}

fn agent_toml(id: &str, name: &str, kind: &str, user: &OwnedUserId, extra: &str) -> String {
    format!(
        "version = 1\nid = \"{id}\"\nname = \"{name}\"\nkind = \"{kind}\"\nmatrix_user = \"{user}\"\n{extra}\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#stub\"\n\n[tools]\nallow = [\"drive_read\", \"delegate\"]\ndrives = [\"smoke\"]\n"
    )
}

fn soul(name: &str) -> String {
    format!("---\nname: {name}\ntitle: a smoke agent\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test agent.\"\ncommunication_style: Short.\nprinciples:\n  - Answer.\n---\n\n{name} answers.\n")
}

/// 101.6 #3 (R422, R436): Dr Tola Grey's `agent.toml` carries a key this
/// build refuses, so its home does not read on Nixi's host; so does Kit's,
/// in a second drive the host mounts — the closet, `local_only`: what it
/// holds reaches local models only. Nixi's session is on a remote one (the
/// mount rule keeps every mounted drive's readers a superset of the home's,
/// so the model is the boundary a live host can show). Its hand-off to
/// `closet/kit` gets the answer a name no drive has gets — neither Kit's
/// sentence nor that the home exists — and its hand-off to `smoke/tola` is
/// refused with that home's own sentence, never "no agent named smoke/tola
/// is known". No room is made for either.
#[ignore = "live: Synapse on delectra"]
#[tokio::test(flavor = "multi_thread")]
async fn a_delegation_to_a_refused_home_says_why() {
    let smoke = Smoke::from_env();
    let scratch = tempfile::tempdir().expect("tempdir");
    let person = smoke.user("tgorka-smoke");
    let nixi = smoke.user("nixi-smoke");
    let tola = smoke.user("tola-smoke");
    let kit = smoke.user("kit-smoke");
    smoke.clear_devices(&nixi).await;
    smoke.set_ratelimit(&nixi, 0, 0).await;

    let nixis_model = calls_at(
        vec![
            (
                1,
                "delegate",
                json!({"agent": "closet/kit", "brief": "Sort the closet.", "card": {"title": "Closet"}}),
            ),
            (
                2,
                "delegate",
                json!({"agent": "smoke/tola", "brief": "Sort the inbox.", "card": {"title": "Inbox"}}),
            ),
        ],
        "Neither can take it.",
    );

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
        reply: None,
        room: dm.clone(),
        drives: vec!["smoke".to_owned()],
        // The drive's opening: tgorka reads it, on any model.
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
        gate: None,
    };
    let nixis_toml = agent_toml(
        "nixi",
        "Nixi",
        "proxy",
        &nixi,
        &format!("human = \"{person}\"\n"),
    )
    .replace(
        "bot:openai:http://127.0.0.1:9#stub",
        &format!("bot:openai:{}#stub", nixis_model.url),
    );
    let files = vec![
        ("80-agents/_drive.toml".to_owned(), drive_toml.clone()),
        ("80-agents/nixi/agent.toml".to_owned(), nixis_toml),
        ("80-agents/nixi/SOUL.md".to_owned(), soul("Nixi")),
        // A key this build does not know: the whole home is refused here.
        (
            "80-agents/tola/agent.toml".to_owned(),
            agent_toml(
                "tola",
                "Dr Tola Grey",
                "steward",
                &tola,
                "colour = \"teal\"\n",
            ),
        ),
        ("80-agents/tola/SOUL.md".to_owned(), soul("Dr Tola Grey")),
        (
            format!("60-sessions/{SESSION}/agent.toml"),
            compose_session_agent_toml(&main),
        ),
    ];
    let bare = bare_drive(scratch.path(), &files);
    // The closet: tgorka's too, but for local models only, holding one
    // home this build refuses.
    let closet_toml = format!(
        "version = 1\nid = \"closet\"\ntitle = \"closet\"\nprincipal = \"tgorka\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\nlocal_only = true\n"
    );
    let closet = bare_drive(
        &scratch.path().join("closet"),
        &[
            ("80-agents/_drive.toml".to_owned(), closet_toml),
            (
                "80-agents/kit/agent.toml".to_owned(),
                agent_toml("kit", "Kit", "steward", &kit, "colour = \"grey\"\n"),
            ),
            ("80-agents/kit/SOUL.md".to_owned(), soul("Kit")),
        ],
    );

    let root = tempfile::tempdir().expect("tempdir");
    let config = format!(
        "version = 1\nprincipal = \"tgorka\"\nhost = \"smoke-a\"\n\n[homeserver]\nurl = \"{}\"\n\n[[drives]]\nid = \"smoke\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\n\n[[drives]]\nid = \"closet\"\nremote = \"{}\"\nowner = \"{person}\"\nreaders = [\"{person}\"]\nlocal_only = true\n\n[[providers]]\nkind = \"openai\"\nbase_url = \"{}\"\n\n[[agents]]\ndrive = \"smoke\"\nids = [\"nixi\"]\n",
        smoke.homeserver,
        bare.display(),
        closet.display(),
        nixis_model.url,
    );
    let config_dir = root.path().join("config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("agentd.toml"), config).expect("config");
    let dir = |name: &str| root.path().join(name).display().to_string();
    let mut host = Host {
        env: vec![
            ("HOME".to_owned(), dir("")),
            ("XDG_CONFIG_HOME".to_owned(), dir("config")),
            ("XDG_DATA_HOME".to_owned(), dir("data")),
            ("XDG_STATE_HOME".to_owned(), dir("state")),
            (
                "KEEPER_AGENTD_SECRET_AGENT_PASSWORD".to_owned(),
                smoke.secret("NIXI_SMOKE_PASSWORD").to_owned(),
            ),
            ("RUST_LOG".to_owned(), "info,keeper_agent=debug".to_owned()),
        ],
        root,
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
    host.start();

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

    // Nixi's `delegate` to Tola is answered with Tola's home's own sentence.
    let said = "smoke/tola's agent.toml does not read on this host: ";
    let deadline = Instant::now() + Duration::from_secs(180);
    let results = loop {
        let lines = host.lines();
        let results: Vec<Value> = lines
            .iter()
            .filter(|l| l["kind"] == "tool_result")
            .cloned()
            .collect();
        if results.iter().any(|l| l.to_string().contains(said)) {
            break results;
        }
        assert!(
            Instant::now() < deadline,
            "no refusal logged.\nlines: {lines:#?}\nhost:\n{}",
            host.log_text()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };
    assert_eq!(results.len(), 2, "{results:#?}");
    let (told, unknown): (Vec<String>, Vec<String>) = results
        .iter()
        .map(Value::to_string)
        .partition(|text| text.contains(said));
    // The closet's home, to a session on a remote model: a name nobody has.
    let unknown = &unknown[0];
    assert!(
        unknown.contains("No agent named closet/kit is known on this host."),
        "{unknown}"
    );
    assert!(
        !unknown.contains("does not read") && !unknown.contains("colour"),
        "{unknown}"
    );
    let text = &told[0];
    assert!(
        text.contains("colour"),
        "the sentence names the key: {text}"
    );
    assert!(!text.contains("is known on this host"), "{text}");
    // Each refusal is logged as `delegate refused`; nothing was opened.
    let delegates: Vec<Value> = host
        .lines()
        .into_iter()
        .filter(|l| l["kind"] == "delegate")
        .collect();
    assert_eq!(delegates.len(), 2, "{delegates:#?}");
    assert!(
        delegates.iter().all(|l| l["body"]["state"] == "refused"),
        "no hand-off was opened: {delegates:#?}"
    );

    // The host did mount the closet and refuse Kit's home: the silence
    // above was the boundary, not a drive it never read.
    if let Some(mut child) = host.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let listed = host
        .command(&["agents", "list"])
        .output()
        .expect("agents list");
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.contains("closet (closet): its agents zone hosts")
            && listed.contains("  kit: refused.")
            && listed.contains("colour"),
        "{listed}"
    );
}
