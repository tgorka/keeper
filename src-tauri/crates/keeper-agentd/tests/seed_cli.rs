//! `keeper-agentd agents init` and `agents new` from the command line, over
//! temporary folders (story 91.5, acceptance 3, 4 and 6). Nothing here
//! reaches a homeserver: the DM is `live_seed.rs`'s.
// The shared helpers hold matrix-sdk futures deep enough to need it.
#![recursion_limit = "256"]
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

const BIN: &str = env!("CARGO_BIN_EXE_keeper-agentd");
/// Never CLIProxyAPI's URL (S-20).
const BOT: &str = "bot:openai:https://provider.example:8452#m";
const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";

struct Env {
    root: tempfile::TempDir,
}

impl Env {
    fn new() -> Env {
        Env {
            root: tempfile::tempdir().expect("tempdir"),
        }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn command(&self, args: &[&str]) -> Command {
        let home = self.path().join("home");
        let mut command = Command::new(BIN);
        command
            .args(args)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_STATE_HOME", home.join("state"))
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("keeper-agentd")
    }

    /// A checkout to seed with `--into`.
    fn checkout(&self) -> PathBuf {
        let dir = self.path().join("tgdrive");
        std::fs::create_dir_all(&dir).expect("checkout");
        dir
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Every file under `dir`, relative, sorted.
fn listing(dir: &Path) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.push(
                    path.strip_prefix(root)
                        .expect("inside")
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn init_into(env: &Env, into: &Path, extra: &[&str]) -> Output {
    let into = into.to_string_lossy().into_owned();
    let mut args = vec![
        "agents",
        "init",
        "tgdrive",
        "--principal",
        "tgorka",
        "--owner",
        TGORKA,
        "--reader",
        TGORKA,
        "--into",
        &into,
    ];
    args.extend_from_slice(extra);
    env.run(&args)
}

#[test]
fn the_catalogue_is_a_choice() {
    let env = Env::new();
    let drive = env.checkout();
    let zone = drive.join("80-agents");

    let out = init_into(&env, &drive, &["--bot", BOT, "--with", "nixi,tola-grey"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(zone.join("nixi/agent.toml").is_file());
    assert!(zone.join("tola-grey/SOUL.md").is_file());
    assert!(!zone.join("lucyna-novak").exists());
    // Nixi is seeded, so the follow-up names the DM's command; --into makes
    // no session.
    let said = text(&out);
    assert!(
        said.contains("keeper-agentd login tgdrive/nixi")
            && said.contains("keeper-agentd agents init tgdrive --with nixi,tola-grey"),
        "{said}"
    );
    assert!(!drive.join("60-sessions").exists());

    // An unknown id is refused naming the catalogue, and writes nothing.
    let env = Env::new();
    let drive = env.checkout();
    let out = init_into(&env, &drive, &["--bot", BOT, "--with", "nixi,naia"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("the catalogue is nixi, tola-grey, lucyna-novak"),
        "{}",
        text(&out)
    );
    assert!(listing(&drive).is_empty());

    // No --with: the zone, and no agent.
    let out = init_into(&env, &drive, &["--bot", BOT]);
    assert!(out.status.success(), "{}", text(&out));
    let files = listing(&drive.join("80-agents"));
    assert!(files.iter().all(|f| f.starts_with("_template/")
        || ["AGENTS.md", "README.md", "_drive.toml"].contains(&f.as_str())));
    assert!(
        files.contains(&"_template/agent.toml".to_owned()),
        "{files:?}"
    );

    // The owner is a reader, or the seed is refused.
    let env = Env::new();
    let drive = env.checkout();
    let into = drive.to_string_lossy().into_owned();
    let out = env.run(&[
        "agents",
        "init",
        "tgdrive",
        "--principal",
        "tgorka",
        "--owner",
        TGORKA,
        "--reader",
        MARTA,
        "--bot",
        BOT,
        "--into",
        &into,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("the owner must be a reader"),
        "{}",
        text(&out)
    );
    assert!(listing(&drive).is_empty());
}

/// No `--bot` is Rust's refusal, the one *Set up agents* gives, and nothing
/// is written (S-20).
#[test]
fn agents_init_requires_a_bot() {
    let env = Env::new();
    let drive = env.checkout();
    let out = init_into(&env, &drive, &["--with", "nixi"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains(keeper_core::agents::seed::NO_BOT),
        "{}",
        text(&out)
    );
    assert!(listing(&drive).is_empty());
}

/// `--local-only` declares it in `_drive.toml`, and the seeded agents must
/// then run on a local bot: a remote one is refused before anything is
/// written. A hand-written `_drive.toml` saying `local_only = true` is
/// left, and flags that do not say so are refused naming it.
#[test]
fn a_local_only_drive_is_seeded_on_a_local_bot() {
    const OLLAMA: &str = "bot:ollama:http://electra.example.org:11434#qwen3:32b";
    let env = Env::new();
    let drive = env.checkout();
    let out = init_into(
        &env,
        &drive,
        &["--bot", BOT, "--local-only", "--with", "nixi"],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("must be an ollama model that runs locally, and this one is openai"),
        "{}",
        text(&out)
    );
    assert!(listing(&drive).is_empty());

    let out = init_into(
        &env,
        &drive,
        &["--bot", OLLAMA, "--local-only", "--with", "nixi"],
    );
    assert!(out.status.success(), "{}", text(&out));
    let declared =
        std::fs::read_to_string(drive.join("80-agents/_drive.toml")).expect("_drive.toml");
    assert!(declared.contains("local_only = true"), "{declared}");

    let out = init_into(&env, &drive, &["--bot", OLLAMA, "--with", "nixi"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("local_only = true"), "{}", text(&out));
}

/// The zone goes where the checkout's `.keeper/keeper.toml` puts it, as
/// the engine reads it; a checkout without `[folder.agents]` gets the
/// default `80-agents/`.
#[test]
fn the_zone_is_where_the_checkouts_keeper_toml_puts_it() {
    let env = Env::new();
    let drive = env.checkout();
    std::fs::create_dir_all(drive.join(".keeper")).expect(".keeper");
    std::fs::write(
        drive.join(".keeper/keeper.toml"),
        "[folder.agents]\nsubfolder = \"70-agents\"\n",
    )
    .expect("keeper.toml");
    let out = init_into(&env, &drive, &["--bot", BOT]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(drive.join("70-agents/_drive.toml").is_file());
    assert!(!drive.join("80-agents").exists());

    let into = drive.to_string_lossy().into_owned();
    let out = env.run(&["agents", "new", "amelia", "--into", &into]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(drive.join("70-agents/amelia/agent.toml").is_file());
}

/// With `--into`, an `agentd.toml` that is there but does not read is
/// refused naming it, never taken as absent (which would drop the pin).
#[test]
fn a_broken_agentd_toml_is_refused_not_ignored() {
    let env = Env::new();
    let config_dir = env.path().join("home/config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(config_dir.join("agentd.toml"), "version = \n").expect("agentd.toml");
    let drive = env.checkout();
    let out = init_into(&env, &drive, &["--bot", BOT]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("agentd.toml is refused"),
        "{}",
        text(&out)
    );
    assert!(listing(&drive).is_empty());
}

/// `run` holds the copies' lock while it serves, and `agents init` against
/// agentd's checkout refuses beside it, naming the host and the unit to
/// stop; once `run` has stopped, the same seed — the host's pin, matching
/// — writes the zone into the checkout.
#[test]
fn agents_init_waits_for_run_and_then_seeds_the_pinned_checkout() {
    let env = Env::new();
    let bare = common::bare_drive(
        env.path(),
        &[("README.md".to_owned(), "the drive\n".to_owned())],
    );
    let config_dir = env.path().join("home/config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\nalways_on = true\n\n[homeserver]\nurl = \"http://127.0.0.1:9\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\n",
            bare.display()
        ),
    )
    .expect("agentd.toml");
    let init: [&str; 9] = [
        "agents", "init", "tgdrive", "--owner", TGORKA, "--reader", TGORKA, "--bot", BOT,
    ];

    /// `run`, stopped however the test ends: a panic must not leave it
    /// serving (and holding whatever it inherited).
    struct Running(std::process::Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut run = Running(env.command(&["run"]).spawn().expect("run"));
    let lock = env.path().join("home/data/keeper-agentd/agentd.lock");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !std::fs::read_to_string(&lock).is_ok_and(|said| said.contains("electra")) {
        assert!(
            std::time::Instant::now() < deadline,
            "run never took the lock"
        );
        if let Some(status) = run.0.try_wait().expect("wait") {
            panic!("run stopped: {status}");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let refused = env.run(&init);
    drop(run);
    assert_eq!(refused.status.code(), Some(2), "{}", text(&refused));
    let said = text(&refused);
    assert!(
        said.contains("keeper-agentd run is serving tgorka's agents on electra")
            && said.contains("stop keeper-agentd@tgorka"),
        "{said}"
    );

    let out = env.run(&init);
    assert!(out.status.success(), "{}", text(&out));
    let zone = env
        .path()
        .join("home/data/keeper-agentd/drives/tgdrive/80-agents");
    let declared = std::fs::read_to_string(zone.join("_drive.toml")).expect("_drive.toml");
    assert!(declared.contains(TGORKA), "{declared}");
    assert!(zone.join("_template/agent.toml").is_file());
}

/// Against agentd's own checkout, readers that are not the pin's are
/// refused naming the difference, before anything is read or written (S-15).
#[test]
fn the_seed_must_be_what_the_host_pinned() {
    let env = Env::new();
    let config_dir = env.path().join("home/config/keeper-agentd");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("agentd.toml"),
        format!(
            "version = 1\nprincipal = \"tgorka\"\nhost = \"electra\"\nalways_on = true\n\n[homeserver]\nurl = \"https://m.example.org\"\ncontrol_room = \"\"\n\n[[drives]]\nid = \"tgdrive\"\nremote = \"{}\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\n\n[[agents]]\ndrive = \"tgdrive\"\nids = [\"nixi\"]\n",
            env.path().join("tgdrive.git").display()
        ),
    )
    .expect("agentd.toml");
    let out = env.run(&[
        "agents", "init", "tgdrive", "--with", "nixi", "--owner", TGORKA, "--reader", TGORKA,
        "--reader", MARTA, "--bot", BOT,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let said = text(&out);
    assert!(
        said.contains("pinned for tgdrive") && said.contains(MARTA),
        "{said}"
    );
    assert!(!env.path().join("home/data/keeper-agentd/drives").exists());

    // A drive the host does not mount is refused too, naming --into.
    let out = env.run(&[
        "agents",
        "init",
        "neuradrive",
        "--owner",
        TGORKA,
        "--reader",
        TGORKA,
        "--bot",
        BOT,
    ]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("--into"), "{}", text(&out));
}

#[test]
fn agents_new_copies_the_template_and_never_takes_a_folder() {
    let env = Env::new();
    let drive = env.checkout();
    let out = init_into(&env, &drive, &["--bot", BOT]);
    assert!(out.status.success(), "{}", text(&out));
    let into = drive.to_string_lossy().into_owned();
    let zone = drive.join("80-agents");

    let out = env.run(&[
        "agents", "new", "amelia", "--name", "Amelia", "--into", &into,
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let toml = std::fs::read_to_string(zone.join("amelia/agent.toml")).expect("agent.toml");
    assert!(toml.contains("id          = \"amelia\""), "{toml}");
    assert!(toml.contains("name        = \"Amelia\""), "{toml}");
    assert!(
        toml.contains("matrix_user = \"@amelia:example.org\""),
        "{toml}"
    );
    let soul = std::fs::read_to_string(zone.join("amelia/SOUL.md")).expect("SOUL.md");
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    assert!(soul.contains(&today) && !soul.contains("{{"), "{soul}");
    assert!(zone.join("amelia/journal/.keep").is_file());

    // An existing folder is refused, and left as it is.
    std::fs::write(zone.join("amelia/SOUL.md"), "mine\n").expect("edit");
    let out = env.run(&["agents", "new", "amelia", "--into", &into]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("already in the zone"), "{}", text(&out));
    assert_eq!(
        std::fs::read_to_string(zone.join("amelia/SOUL.md")).expect("read"),
        "mine\n"
    );

    // --from-bmad writes the merged soul and lists what it did not import.
    let skill = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../keeper-ported/tests/fixtures/bmad/bmad-agent-architect");
    let skill = skill.to_string_lossy().into_owned();
    let out = env.run(&[
        "agents",
        "new",
        "winston",
        "--from-bmad",
        &skill,
        "--into",
        &into,
    ]);
    assert!(out.status.success(), "{}", text(&out));
    let expected = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../keeper-core/tests/fixtures/agents/winston-SOUL.md"),
    )
    .expect("fixture");
    assert_eq!(
        std::fs::read_to_string(zone.join("winston/SOUL.md")).expect("soul"),
        expected
    );
    let toml = std::fs::read_to_string(zone.join("winston/agent.toml")).expect("agent.toml");
    assert!(toml.contains("name        = \"Winston\""), "{toml}");
    let said = text(&out);
    assert!(
        said.contains("Not imported:") && said.contains("menu CA → skill bmad-architecture"),
        "{said}"
    );
}
