use std::collections::BTreeSet;

use keeper_core::agents::home;
use keeper_core::agents::label::Readers;
use keeper_core::agents::seed::CATALOGUE;
use keeper_core::agents::session::{self, SessionAgent, SessionKind};
use matrix_sdk::ruma::OwnedUserId;

use super::*;
use crate::sessions::verbs::{self, CreateOutcome};

const BOT: &str = "bot:openai:https://provider.example:8452#m";
const TGORKA: &str = "@tgorka:example.org";
const MARTA: &str = "@marta:example.org";

fn choices(readers: &[&str]) -> SeedChoices {
    SeedChoices::new(
        "tgdrive",
        "tgorka",
        TGORKA,
        &readers.iter().map(|r| (*r).to_owned()).collect::<Vec<_>>(),
        false,
        Some(BOT),
        &CATALOGUE
            .iter()
            .map(|a| a.id.to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("choices")
}

fn every_path(choices: &SeedChoices) -> BTreeSet<String> {
    seed::files(choices).into_iter().map(|f| f.path).collect()
}

fn read(zone: &Path, rel: &str) -> String {
    std::fs::read_to_string(zone.join(rel)).expect(rel)
}

#[test]
fn agents_init_never_overwrites_and_says_what_it_left() {
    let choices = choices(&[TGORKA]);

    // An empty zone gets every file.
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("80-agents");
    let applied = apply(plan_at(&choices, &zone), &zone).expect("apply");
    assert_eq!(
        applied.written.iter().cloned().collect::<BTreeSet<_>>(),
        every_path(&choices)
    );
    assert!(applied.left.is_empty());
    for file in seed::files(&choices) {
        assert_eq!(read(&zone, &file.path), file.text, "{}", file.path);
    }

    // Twice: the second run writes nothing and names every file as left.
    let again = apply(plan_at(&choices, &zone), &zone).expect("again");
    assert!(again.written.is_empty(), "{:?}", again.written);
    assert_eq!(again.left.len(), every_path(&choices).len());

    // An edited soul and an existing guide are left, byte for byte.
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("80-agents");
    std::fs::create_dir_all(zone.join("nixi")).expect("mkdir");
    let soul = "---\nname: Nixi\n---\nmine, edited\n";
    std::fs::write(zone.join("nixi/SOUL.md"), soul).expect("soul");
    std::fs::write(zone.join("README.md"), "my guide\n").expect("readme");
    let plan = plan_at(&choices, &zone);
    assert_eq!(plan.left, ["README.md", "nixi/SOUL.md"]);

    // A file that appears between the plan and the write is left too.
    std::fs::create_dir_all(zone.join("tola-grey")).expect("mkdir");
    std::fs::write(zone.join("tola-grey/agent.toml"), "raced\n").expect("race");
    let applied = apply(plan, &zone).expect("apply");
    assert_eq!(
        applied.left,
        ["README.md", "nixi/SOUL.md", "tola-grey/agent.toml"]
    );
    assert_eq!(applied.written.len(), every_path(&choices).len() - 3);
    assert_eq!(read(&zone, "nixi/SOUL.md"), soul);
    assert_eq!(read(&zone, "README.md"), "my guide\n");
    assert_eq!(read(&zone, "tola-grey/agent.toml"), "raced\n");
    assert!(zone.join("nixi/agent.toml").is_file());
}

/// A folder on the way that is a link is refused, never followed out of
/// the drive.
#[cfg(unix)]
#[test]
fn a_link_on_the_way_is_refused_not_followed() {
    let root = tempfile::tempdir().expect("tempdir");
    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    let zone = root.path().join("80-agents");
    std::fs::create_dir_all(&zone).expect("zone");
    std::os::unix::fs::symlink(&outside, zone.join("nixi")).expect("link");
    let choices = choices(&[TGORKA]);
    let plan = SeedPlan {
        write: seed::files(&choices)
            .into_iter()
            .filter(|f| f.path == "nixi/agent.toml")
            .collect(),
        left: Vec::new(),
    };
    let refused = apply(plan, &zone).expect_err("refused");
    assert!(
        refused.contains("is not a folder keeper may write into"),
        "{refused}"
    );
    assert!(std::fs::read_dir(&outside).expect("ls").next().is_none());
}

fn user(id: &str) -> OwnedUserId {
    OwnedUserId::try_from(id).expect("user")
}

/// Nixi's `main` session in `room` as `main_dm` writes it, on tgdrive read
/// by tgorka and marta.
fn main_session(room: &str) -> SessionAgent {
    let choices = choices(&[TGORKA, MARTA]);
    let files = seed::files(&choices);
    let toml = files
        .iter()
        .find(|file| file.path == "nixi/agent.toml")
        .expect("nixi");
    let nixi = home::parse_agent_toml(&toml.text, "nixi", &choices.decl).expect("nixi parses");
    let room = matrix_sdk::ruma::RoomId::parse(room).expect("room");
    super::main_session(
        &nixi,
        &choices.decl,
        &user(TGORKA),
        &room,
        chrono::Local::now(),
    )
}

/// One folder per id, its `agent.toml` written with it: a second create with
/// the same id finds the first and writes nothing (AD-368). The `main`
/// session's label is the person's alone, though the drive has two readers.
#[test]
fn an_agent_session_is_created_once_with_its_agent_toml() {
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    std::fs::create_dir_all(&zone).expect("zone");
    let now = chrono::Local::now();
    let first =
        verbs::create_agent_session(&zone, &main_session("!dm:example.org"), now).expect("create");
    let CreateOutcome::Created { path, .. } = first else {
        panic!("{first:?}");
    };
    let text = read(&zone, &format!("{path}/{}", session::FILE_NAME));
    let parsed = session::parse_session_agent_toml(&text).expect("agent.toml");
    assert_eq!(parsed.room.as_str(), "!dm:example.org");
    assert_eq!(parsed.kind, SessionKind::Main);
    assert_eq!(parsed.id, seed::main_session_id("tgdrive", "nixi"));
    assert_eq!(
        parsed.label.readers,
        Readers::Only([user(TGORKA)].into_iter().collect())
    );
    assert!(zone.join(&path).join("README.md").is_file());

    let second = verbs::create_agent_session(&zone, &main_session("!other:example.org"), now)
        .expect("again");
    assert!(
        matches!(&second, CreateOutcome::Existed { path: again, .. } if *again == path),
        "{second:?}"
    );
    let folders: Vec<_> = std::fs::read_dir(zone.join("active"))
        .expect("ls")
        .filter_map(Result::ok)
        .collect();
    assert_eq!(folders.len(), 1);
    assert_eq!(read(&zone, &format!("{path}/{}", session::FILE_NAME)), text);
}

/// A folder that appears between the look for it and the write wins: the DM
/// is the room it names, and the room this run made is to be discarded,
/// never given the anchor. A room adopted rather than made is never
/// discarded.
#[test]
fn the_main_dm_is_the_room_its_folder_names() {
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    let now = chrono::Local::now();

    let made = make_folder(&zone, &main_session("!first:example.org"), true, now).expect("first");
    assert_eq!(made.room.as_str(), "!first:example.org");
    assert!(made.folder_made);
    assert_eq!(made.discard, None);

    // Another checkout's run wrote the folder while this one made a room.
    let raced = make_folder(&zone, &main_session("!second:example.org"), true, now).expect("raced");
    assert_eq!(raced.room.as_str(), "!first:example.org");
    assert_eq!(raced.path, made.path);
    assert!(!raced.folder_made);
    assert_eq!(
        raced.discard.as_ref().map(|room| room.as_str()),
        Some("!second:example.org")
    );

    let adopted =
        make_folder(&zone, &main_session("!adopted:example.org"), false, now).expect("adopted");
    assert_eq!(adopted.room.as_str(), "!first:example.org");
    assert_eq!(adopted.discard, None);
}

/// The copy's DM with its person, found among its rooms: a session room of
/// the two of them, its newest status saying `main`, or — before any status
/// — marked direct. A conversation room of the same two is not it.
#[test]
fn a_main_dm_is_known_by_its_room() {
    use keeper_core::agents::events::SESSION_ROOM_TYPE;
    let me = user("@nixi:example.org");
    let human = user(TGORKA);
    let two: BTreeSet<OwnedUserId> = [me.clone(), human.clone()].into_iter().collect();
    let dm = |kind, members: &BTreeSet<OwnedUserId>, direct, status| {
        is_main_dm(kind, members, &me, &human, direct, status)
    };
    let session = Some(SESSION_ROOM_TYPE);
    assert!(dm(session, &two, false, Some("main")));
    assert!(dm(session, &two, true, None));
    assert!(!dm(session, &two, false, None), "neither direct nor main");
    assert!(!dm(session, &two, true, Some("conversation")));
    assert!(!dm(
        Some("dev.keeper.agent.control"),
        &two,
        true,
        Some("main")
    ));
    assert!(!dm(None, &two, true, Some("main")));
    let three: BTreeSet<OwnedUserId> = two.iter().cloned().chain([user(MARTA)]).collect();
    assert!(!dm(session, &three, true, Some("main")));
    let other: BTreeSet<OwnedUserId> = [me.clone(), user(MARTA)].into_iter().collect();
    assert!(!dm(session, &other, true, Some("main")));
}

fn profile(root: &Path) -> SyncProfile {
    let mut profile = SyncProfile::new(
        "P1".to_owned(),
        "tgdrive".to_owned(),
        root.to_owned(),
        "https://forge.example/tgdrive.git".to_owned(),
    );
    profile.sessions = Some(Default::default());
    profile.agents = Some(Default::default());
    profile
}

fn req(readers: &[&str]) -> AgentSeedReq {
    AgentSeedReq {
        profile_id: "P1".to_owned(),
        drive: "tgdrive".to_owned(),
        owner: TGORKA.to_owned(),
        readers: readers.iter().map(|r| (*r).to_owned()).collect(),
        local_only: false,
        bot: Some(BOT.to_owned()),
        with: vec!["nixi".to_owned()],
    }
}

/// *Set up agents* on a Mac: refused with no bot (S-20), against this Mac's
/// pin when the readers differ (S-15), and against a `_drive.toml` the seed
/// would leave saying something else.
#[test]
fn set_up_agents_refuses_what_would_host_nothing() {
    let root = tempfile::tempdir().expect("tempdir");
    let profiles = vec![profile(root.path())];
    let login = Some("tgorka");

    let no_bot = AgentSeedReq {
        bot: None,
        ..req(&[TGORKA])
    };
    assert_eq!(
        desktop_choices(&profiles, None, login, &no_bot).expect_err("no bot"),
        seed::NO_BOT
    );
    assert_eq!(
        desktop_choices(&profiles, None, None, &req(&[TGORKA])).expect_err("no login"),
        NO_PRINCIPAL
    );

    let pin = DrivePin {
        id: "tgdrive".to_owned(),
        remote: "https://forge.example/tgdrive.git".to_owned(),
        credential: None,
        owner: OwnedUserId::try_from(TGORKA).expect("user"),
        readers: [OwnedUserId::try_from(TGORKA).expect("user")]
            .into_iter()
            .collect(),
        local_only: false,
    };
    let refused =
        desktop_choices(&profiles, Some(&pin), login, &req(&[TGORKA, MARTA])).expect_err("pin");
    assert!(
        refused.contains(MARTA) && refused.contains("pinned"),
        "{refused}"
    );
    let (choices, zone) =
        desktop_choices(&profiles, Some(&pin), login, &req(&[TGORKA])).expect("as pinned");
    assert_eq!(zone, root.path().join("80-agents"));
    assert_eq!(choices.decl.principal, "tgorka");

    let applied = apply(plan_at(&choices, &zone), &zone).expect("seed");
    assert_eq!(
        result_vm("P1", &choices, applied).agents,
        ["nixi".to_owned()]
    );
    let refused =
        desktop_choices(&profiles, None, login, &req(&[TGORKA, MARTA])).expect_err("file");
    assert!(refused.contains("already names tgdrive"), "{refused}");
}

const OLLAMA: &str = "bot:ollama:http://electra.example.org:11434#qwen3:32b";

fn local_pin() -> DrivePin {
    DrivePin {
        id: "tgdrive".to_owned(),
        remote: "https://forge.example/tgdrive.git".to_owned(),
        credential: None,
        owner: user(TGORKA),
        readers: [user(TGORKA)].into_iter().collect(),
        local_only: true,
    }
}

/// A Mac that pinned tgdrive `local_only` (AD-377): a hand-written
/// `_drive.toml` saying `local_only = true` is seeded on a local bot, the
/// seed being checked against the file it leaves; a remote bot is refused
/// now, with the sentence sign-in would give; and a request that says
/// otherwise than the file is refused naming `local_only`.
#[test]
fn a_local_only_drive_is_seeded_on_a_local_bot() {
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("80-agents");
    std::fs::create_dir_all(&zone).expect("zone");
    std::fs::write(
        zone.join("_drive.toml"),
        format!("version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{TGORKA}\"\nreaders = [\"{TGORKA}\"]\nlocal_only = true\n"),
    )
    .expect("_drive.toml");
    let profiles = vec![profile(root.path())];
    let pin = local_pin();
    let local = AgentSeedReq {
        local_only: true,
        bot: Some(OLLAMA.to_owned()),
        ..req(&[TGORKA])
    };
    let (choices, zone) =
        desktop_choices(&profiles, Some(&pin), Some("tgorka"), &local).expect("seeded");
    let applied = apply(plan_at(&choices, &zone), &zone).expect("seed");
    assert!(applied.left.contains(&"_drive.toml".to_owned()));
    assert!(applied.written.contains(&"nixi/agent.toml".to_owned()));

    let remote = AgentSeedReq {
        bot: Some(BOT.to_owned()),
        ..local.clone()
    };
    assert_eq!(
        desktop_choices(&profiles, Some(&pin), Some("tgorka"), &remote).expect_err("remote"),
        "_drive.toml's local_only is true for tgdrive, so the bot must be an ollama model that runs locally, and this one is openai."
    );
    let unsaid = AgentSeedReq {
        local_only: false,
        ..local
    };
    let refused =
        desktop_choices(&profiles, Some(&pin), Some("tgorka"), &unsaid).expect_err("unsaid");
    assert!(refused.contains("local_only = true"), "{refused}");
}

/// What the form starts from is decided here: a declared zone's file, its
/// agents ticked by the drive their souls were written for, the pin's
/// `local_only` for a zone with none, and no form at all without a Matrix
/// account to name as owner.
#[test]
fn the_form_starts_from_the_zone_and_the_pin() {
    let declared = tempfile::tempdir().expect("tempdir");
    let zone = declared.path().join("80-agents");
    std::fs::create_dir_all(&zone).expect("zone");
    std::fs::write(
        zone.join("_drive.toml"),
        format!("version = 1\nid = \"neuradrive\"\nprincipal = \"neuraffica\"\nowner = \"{TGORKA}\"\nreaders = [\"{MARTA}\", \"{TGORKA}\"]\nlocal_only = true\n"),
    )
    .expect("_drive.toml");
    let fresh = tempfile::tempdir().expect("tempdir");
    let mut other = profile(fresh.path());
    other.id = "P2".to_owned();
    let profiles = vec![profile(declared.path()), other];
    let pins = [("P2".to_owned(), local_pin())].into_iter().collect();
    let accounts = [TGORKA.to_owned()];

    let offered = offer(&profiles, &pins, &accounts, Some("tgorka"), Vec::new());
    let [file, new] = offered.folders.as_slice() else {
        panic!("{:?}", offered.folders);
    };
    assert!(file.declared && file.local_only);
    assert_eq!(file.drive, "neuradrive");
    assert_eq!(file.preselected, ["lucyna-novak"]);
    assert!(!new.declared);
    assert!(new.local_only, "the pin's");
    assert_eq!(new.preselected, ["nixi", "tola-grey"]);
    assert_eq!(new.problem, None);

    let no_account = offer(&profiles, &pins, &[], Some("tgorka"), Vec::new());
    assert_eq!(
        no_account.folders[0].problem, None,
        "the file names the owner"
    );
    assert_eq!(no_account.folders[1].problem.as_deref(), Some(NO_ACCOUNT));
}

/// `agents new` copies what an owner put in `_template/`: text with its
/// tokens filled, a file that is not text byte for byte, an empty folder
/// as a folder; a link is refused, never followed.
#[cfg(unix)]
#[test]
fn agents_new_copies_any_template_and_refuses_a_link() {
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("80-agents");
    apply(plan_at(&choices(&[TGORKA]), &zone), &zone).expect("seed");
    let png = [0x89, b'P', b'N', b'G', 0xff, 0x00, 0xfe];
    std::fs::write(zone.join("_template/avatar.png"), png).expect("png");
    std::fs::create_dir_all(zone.join("_template/notes/empty")).expect("empty");

    let applied = new_agent(&zone, "amelia", "Amelia", "2026-10-03", None).expect("new");
    assert_eq!(
        std::fs::read(zone.join("amelia/avatar.png")).expect("png"),
        png
    );
    assert!(zone.join("amelia/notes/empty").is_dir());
    assert!(applied.written.contains(&"amelia/avatar.png".to_owned()));
    assert!(applied.written.contains(&"amelia/notes/empty/".to_owned()));
    assert!(read(&zone, "amelia/agent.toml").contains("\"Amelia\""));

    let outside = root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    std::os::unix::fs::symlink(&outside, zone.join("_template/shared")).expect("link");
    let refused = new_agent(&zone, "bea", "Bea", "2026-10-03", None).expect_err("link");
    assert!(refused.contains("_template/shared is a link"), "{refused}");
    assert!(!zone.join("bea").exists(), "nothing written");
}
