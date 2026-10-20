//! The desktop host's own entry against a real homeserver (story 102.4 #5,
//! R490): `keeper_agent::desktop::DesktopHost`, signed in and built from a
//! scan's facts as the app's shell builds them, and ticked as its interval
//! ticks it.
//!
//! `harness = false`: this binary stands in for the app. Its `main` answers
//! the worker roles first, as the shell's `run` does, so the host's PDF
//! reader and photo copier — its own binary started again — are this
//! binary. It answers a test runner as libtest answers for one `#[ignore]`d
//! test: `--list` names it (cargo-nextest lists every test binary with
//! `--list --format terse`, then again with `--ignored`, and fails on any
//! other line), a plain run passes over it, and it runs with `--ignored` or
//! `--include-ignored`, with the environment `live_turn.rs` names:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-agentd --test live_desktop -- --ignored
//! ```

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]

use std::process::ExitCode;

#[cfg(target_os = "linux")]
mod common;

/// The one test this binary holds, as libtest would name it.
const TEST: &str = "every_kind_of_file_sent_to_a_desktop_hosted_agent_is_answered";

fn main() -> ExitCode {
    if let Some(code) = keeper_agent::attachments::served_as_worker(std::env::args_os().skip(1)) {
        return ExitCode::from(u8::try_from(code).unwrap_or(1));
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|arg| arg == flag);
    let ignored = has("--ignored") || has("--include-ignored");
    // A filter (any argument that is not a flag) that does not name this test
    // selects nothing, as libtest's does.
    let selected = args
        .iter()
        .filter(|arg| !arg.starts_with('-') && arg.as_str() != "terse")
        .all(|filter| TEST.contains(filter.as_str()));
    if has("--list") {
        if selected {
            println!("{TEST}: test");
        }
        return ExitCode::SUCCESS;
    }
    if !selected || !ignored {
        println!("live_desktop: 1 live test ignored (run it with --ignored)");
        return ExitCode::SUCCESS;
    }
    live::run()
}

#[cfg(not(target_os = "linux"))]
mod live {
    pub fn run() -> std::process::ExitCode {
        println!("live_desktop: the live test runs on Linux, against delectra's Synapse");
        std::process::ExitCode::SUCCESS
    }
}

#[cfg(target_os = "linux")]
mod live {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use keeper_core::agents::label::{Integrity, Label, Readers};
    use keeper_core::agents::matrix::RoomKind;
    use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};

    use crate::common::turns::{every_kind_is_answered, person, tells_what_it_got};
    use crate::common::{scripted, Smoke};

    const SESSION: &str = "active/2026-10-19-desktop-smoke";

    /// The live test on a multi-threaded runtime, as the app's is; a panic
    /// fails the run.
    pub fn run() -> std::process::ExitCode {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        runtime.block_on(every_kind_of_file_sent_to_a_desktop_hosted_agent_is_answered());
        println!("test every_kind_of_file_sent_to_a_desktop_hosted_agent_is_answered ... ok");
        std::process::ExitCode::SUCCESS
    }

    /// Story 102.4 #5 (R490), live, through the desktop host's own entry: the
    /// app's `keeper_agent::desktop::DesktopHost`, signed in and built from a
    /// scan's facts as the shell builds them — a folder flagged for agents, its
    /// pin, the person's account's homeserver, a provider row — and ticked as
    /// the app's 1 Hz interval ticks it, takes every kind of file as agentd
    /// does: each answered.
    async fn every_kind_of_file_sent_to_a_desktop_hosted_agent_is_answered() {
        use keeper_agent::desktop::{self, DesktopFacts, DesktopHost};
        use keeper_agent::headless::{HeadlessPlatform, SecretMap, SECRET_ENV_PREFIX};
        use keeper_core::agents::copy::{pin_from_shown, AgentPinReq};
        use keeper_core::bots::{store, Provider, ProviderKind};
        use keeper_core::platform::Platform;

        let smoke = Smoke::from_env();
        let agent = smoke.user("nixi-smoke");
        let me = smoke.user("tgorka-smoke");
        smoke.set_ratelimit(&agent, 0, 0).await;
        smoke.clear_devices(&agent).await;
        let root = tempfile::tempdir().expect("tempdir");
        let password = smoke.secret("NIXI_SMOKE_PASSWORD");
        let maker = smoke
            .client(&root.path().join("maker"), "nixi-smoke", password)
            .await;
        let room = maker
            .create_room(
                RoomKind::Session(SessionKind::Main),
                "smoke",
                vec![me.clone()],
                &[],
            )
            .await
            .expect("room");
        // The principal's control room, made by its agent: a Mac takes no
        // claim until a copy has found it.
        maker
            .create_room(
                RoomKind::Control,
                "smoke control",
                vec![me.clone()],
                std::slice::from_ref(&agent),
            )
            .await
            .expect("control room");

        let base_url = scripted(tells_what_it_got);
        let drive_toml = format!(
            "version = 1\nid = \"smoke\"\ntitle = \"smoke\"\nprincipal = \"tgorka\"\nowner = \"{me}\"\nreaders = [\"{me}\"]\nlocal_only = false\n"
        );
        let decl = keeper_core::agents::drive::parse(&drive_toml).expect("decl");
        let session = SessionAgent {
            id: ulid::Ulid::new(),
            agent: "nixi".to_owned(),
            drive: "smoke".to_owned(),
            kind: SessionKind::Main,
            title: "smoke".to_owned(),
            requested_by: me.clone(),
            parent: None,
            reply: None,
            room: room.clone(),
            drives: vec!["smoke".to_owned()],
            label: Label {
                readers: Readers::Only(std::iter::once(me.clone()).collect()),
                ..Label::opening(&decl, Integrity::Owner)
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
        // The drive as the app's sync profile holds it: a folder on this
        // machine, flagged for agents and pinned.
        let folder = root.path().join("smoke");
        for (path, text) in [
            ("80-agents/_drive.toml".to_owned(), drive_toml.clone()),
            (
                "80-agents/nixi/agent.toml".to_owned(),
                format!(
                    "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"{agent}\"\nhuman = \"{me}\"\n\n[model]\nbot = \"bot:openai:{base_url}#claude-smoke\"\n\n[tools]\nallow = [\"drive_list\", \"drive_read\", \"drive_grep\", \"drive_stat\"]\ndrives = [\"smoke\"]\n"
                ),
            ),
            (
                "80-agents/nixi/SOUL.md".to_owned(),
                "---\nname: Nixi\ntitle: the smoke proxy\nicon: \"*\"\nrole: Answers the smoke test.\nidentity: \"A test proxy.\"\ncommunication_style: Short.\nprinciples:\n  - Answer from the drive.\n---\n\nNixi answers from the drive.\n".to_owned(),
            ),
            (
                format!("60-sessions/{SESSION}/agent.toml"),
                compose_session_agent_toml(&session),
            ),
        ] {
            let path = folder.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, text).expect("write");
        }
        let mut profile = keeper_sync::SyncProfile::new(
            "p-smoke",
            "smoke",
            &folder,
            "git@forge:tgorka/smoke.git",
        );
        profile.agents = Some(Default::default());
        profile.sessions = Some(Default::default());
        let shown = AgentPinReq {
            owner: me.to_string(),
            readers: vec![me.to_string()],
            local_only: false,
        };
        let pin = pin_from_shown(&decl, &profile.remote_url, &shown).expect("pin");

        // The app's data: a provider row for the agent's bot, and the copy
        // signed in into the keychain as Settings › Agents signs it in.
        let data = root.path().join("data");
        store::insert_provider(
            &data,
            &Provider {
                id: "smoke".to_owned(),
                kind: ProviderKind::OpenAi,
                name: "smoke".to_owned(),
                base_url: base_url.clone(),
                created_ms: 1,
            },
        )
        .expect("provider");
        let platform: Arc<dyn Platform> = Arc::new(HeadlessPlatform::new(
            &data,
            Arc::new(SecretMap::new(keeper_sync::xdg::SecretStore::new(
                SECRET_ENV_PREFIX,
                root.path().join("secrets"),
            ))),
        ));
        desktop::sign_in(
            platform.as_ref(),
            &smoke.homeserver,
            &agent,
            password,
            "nixi@smokemac",
        )
        .await
        .expect("signed in");
        let facts = DesktopFacts {
            login: Some("tgorka".to_owned()),
            device: Some("smokemac".to_owned()),
            profiles: vec![profile],
            pins: std::collections::BTreeMap::from([("p-smoke".to_owned(), pin)]),
            homeservers: vec![(me.to_string(), smoke.homeserver.clone())],
            ..DesktopFacts::default()
        };
        let desk = Arc::new(tokio::sync::Mutex::new(DesktopHost::new(
            keeper_agent::turn::TurnEnv::new(Arc::clone(&platform)),
            data,
            "smoke",
        )));
        let ticking = {
            let desk = Arc::clone(&desk);
            tokio::spawn(async move {
                loop {
                    desk.lock().await.tick(Some(facts.clone())).await;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            })
        };
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            let (held, problems) = {
                let desk = desk.lock().await;
                (desk.held(), desk.problems().clone())
            };
            if held.iter().any(|claim| claim["room"] == room.as_str()) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the desktop host never held {room}: {problems:?} {held:?}"
            );
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let (client, seen, _sync) = person(&smoke, &root.path().join("person"), &room).await;
        every_kind_is_answered(&client, &seen, &agent, &room, &String::new).await;
        ticking.abort();
        desk.lock().await.stop().await;
    }
}
