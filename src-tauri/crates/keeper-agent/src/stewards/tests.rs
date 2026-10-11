use std::path::Path;

use keeper_core::agents::card::{CardAgent, Field};
use keeper_core::agents::home::{self, AgentConfig};
use keeper_core::agents::seed::{self, SeedChoices, CATALOGUE};
use keeper_core::agents::session::{compose_session_agent_toml, FILE_NAME};
use matrix_sdk::ruma::OwnedRoomId;

use super::*;
use crate::cards::{due, Due};
use crate::sessions::verbs::{self, CreateReq};

const TGORKA: &str = "@tgorka:example.org";

fn choices() -> SeedChoices {
    SeedChoices::new(
        "tgdrive",
        "tgorka",
        TGORKA,
        &[TGORKA.to_owned()],
        false,
        Some("bot:openai:https://provider.example:8452#m"),
        &CATALOGUE
            .iter()
            .map(|a| a.id.to_owned())
            .collect::<Vec<_>>(),
    )
    .expect("choices")
}

/// Dr Tola Grey as the seed writes her: her menu is 91.5's.
fn tola(choices: &SeedChoices) -> AgentConfig {
    let file = seed::files(choices)
        .into_iter()
        .find(|file| file.path.ends_with("tola-grey/agent.toml"))
        .expect("Tola's home");
    home::parse_agent_toml(&file.text, "tola-grey", &choices.decl).expect("her agent.toml")
}

fn room(name: &str) -> OwnedRoomId {
    OwnedRoomId::try_from(format!("!{name}:example.org")).expect("room")
}

fn at(text: &str) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::parse_from_rfc3339(text)
        .expect("an instant")
        .with_timezone(&chrono::Local)
}

/// Make `duty`'s session as her host does, its room `room`.
fn make(
    zone: &Path,
    config: &AgentConfig,
    decl: &DriveDecl,
    duty: Duty,
    room_name: &str,
    now: chrono::DateTime<chrono::Local>,
) -> crate::seed::Settled {
    let agent = session(config, decl, duty, &room(room_name), now);
    let files = folder_files(config, decl, duty, zone).expect("her files");
    crate::seed::make_folder(zone, &agent, files, true, now).expect("made")
}

fn card_text(zone: &Path, path: &str, duty: Duty) -> Option<String> {
    std::fs::read_to_string(zone.join(path).join(duty.card_file())).ok()
}

/// 92.5 acceptance 6: each duty's session is made once, under its derived
/// id, holding one `@daily` card of hers whose body is her menu's prompts,
/// with no `scheduled_by` — so it is due without anyone's tick. Made again,
/// the folder is found, the room just made is the one to discard, and the
/// card is never written again: the owner's edit is kept and a deleted card
/// stays deleted.
#[test]
fn a_stewards_own_cards_are_made_once_and_kept_as_the_owner_left_them() {
    let choices = choices();
    let tola = tola(&choices);
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    let now = at("2026-10-05T09:30:00+00:00");

    for (duty, codes) in [
        (Duty::Triage, &["TR", "DS"][..]),
        (Duty::Harvest, &["HV"][..]),
    ] {
        let made = make(&zone, &tola, &choices.decl, duty, duty.name(), now);
        assert!(made.folder_made);
        let toml = std::fs::read_to_string(zone.join(&made.path).join(FILE_NAME)).expect("toml");
        let agent = keeper_core::agents::session::parse_session_agent_toml(&toml).expect("parse");
        assert_eq!(agent.id, session_id(&tola, duty));
        assert_eq!(agent.kind, SessionKind::Scheduled);
        assert_eq!(agent.agent, "tola-grey");
        assert_eq!(is_harvest(&agent), duty == Duty::Harvest);

        let text = card_text(&zone, &made.path, duty).expect("the card");
        for code in codes {
            let prompt = tola
                .menu
                .iter()
                .find(|item| item.code == *code)
                .map(|item| match &item.action {
                    home::MenuAction::Prompt(prompt) => prompt.trim().to_owned(),
                    home::MenuAction::Workflow(_) => unreachable!(),
                })
                .expect("the prompt");
            assert!(text.contains(&prompt), "{code} in {text}");
        }
        let keys = CardAgent::of_text(&text).expect("agent keys");
        assert_eq!(keys.assignee, Some(Field::Read("tola-grey".to_owned())));
        assert_eq!(keys.schedule.as_deref(), Some("@daily"));
        assert_eq!(keys.scheduled_by, None);
        assert!(matches!(
            due(&keys, now.timestamp_millis(), 0),
            Due::Due { .. }
        ));

        // Her host starts again, in another checkout's race: nothing new.
        let again = make(&zone, &tola, &choices.decl, duty, "again", now);
        assert!(!again.folder_made);
        assert_eq!(again.path, made.path);
        assert_eq!(again.room, room(duty.name()));
        assert_eq!(again.discard, Some(room("again")));

        // The owner's edit is kept; a deleted card is not made again.
        let edited = text.replace("@daily", "@weekly");
        std::fs::write(zone.join(&made.path).join(duty.card_file()), &edited).expect("edit");
        make(&zone, &tola, &choices.decl, duty, "third", now);
        assert_eq!(card_text(&zone, &made.path, duty), Some(edited));
        std::fs::remove_file(zone.join(&made.path).join(duty.card_file())).expect("delete");
        make(&zone, &tola, &choices.decl, duty, "fourth", now);
        assert_eq!(card_text(&zone, &made.path, duty), None);
    }
}

/// A person's session `title` opened at `now`, archived: its id and path.
fn closed(zone: &Path, title: &str, now: chrono::DateTime<chrono::Local>) -> (String, String) {
    let id = opened(zone, title, now);
    archive(zone, &id)
}

/// A person's session `title` opened at `now`: its id.
fn opened(zone: &Path, title: &str, now: chrono::DateTime<chrono::Local>) -> String {
    let id = ulid::Ulid::new();
    verbs::create(
        zone,
        CreateReq {
            id,
            title: title.to_owned(),
            pattern_id: None,
            now,
        },
    )
    .expect("created");
    id.to_string()
}

/// The session `id` archived: its id and path.
fn archive(zone: &Path, id: &str) -> (String, String) {
    verbs::archive(zone, id, false, 2026).expect("archived");
    let path = verbs::find(zone, id).expect("found").path;
    (id.to_owned(), path)
}

/// Every closed session `harvester` hands over `steps` steps, each
/// acknowledged as settled, by id.
fn harvest_all(
    harvester: &mut Harvester,
    zone: &Path,
    harvest: &crate::seed::Settled,
    agent: &SessionAgent,
    now: Instant,
    steps: usize,
) -> Vec<String> {
    let mut ids = Vec::new();
    for _ in 0..steps {
        for closed in harvester.step(zone, &harvest.path, agent, now) {
            harvester.acknowledged(&closed.id, true, now);
            ids.push(closed.id);
        }
    }
    ids.sort();
    ids
}

/// 92.5 acceptance 3, what wakes it (R61): a session archived after her
/// harvest session was made, however long ago it opened — one still
/// active then and closed later, one of her own delegated sessions — keyed
/// by its id. Not one archived before (the baseline, same day or not),
/// not one still active, not her own triage or harvest session, and not
/// with her harvest folder renamed either.
#[tokio::test(start_paused = true)]
async fn what_closes_after_her_harvest_began_is_harvested_whenever_it_opened() {
    let choices = choices();
    let tola = tola(&choices);
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    let now = at("2026-10-05T09:30:00+00:00");
    std::fs::create_dir_all(&zone).expect("zone");
    let (before, _) = closed(
        &zone,
        "closed this morning",
        at("2026-10-05T08:00:00+00:00"),
    );
    let long = opened(&zone, "long running", at("2026-10-01T10:00:00+00:00"));
    let harvest = make(&zone, &tola, &choices.decl, Duty::Harvest, "harvest", now);
    let triage = make(&zone, &tola, &choices.decl, Duty::Triage, "triage", now);
    let agent = session(&tola, &choices.decl, Duty::Harvest, &room("harvest"), now);
    let baseline =
        std::fs::read_to_string(zone.join(&harvest.path).join(BASELINE)).expect("baseline");
    assert!(baseline.contains(&before), "{baseline}");

    archive(&zone, &long);
    let (taxes, _) = closed(&zone, "taxes", at("2026-10-06T10:00:00+00:00"));
    opened(&zone, "still open", at("2026-10-06T10:00:00+00:00"));
    // One of her delegated sessions, closed: harvested like any other.
    let (delegated, delegated_path) =
        closed(&zone, "her delegation", at("2026-10-06T11:00:00+00:00"));
    let mut hers = session(&tola, &choices.decl, Duty::Triage, &room("hers"), now);
    hers.id = delegated.parse().expect("ulid");
    hers.kind = SessionKind::Delegated;
    std::fs::write(
        zone.join(&delegated_path).join(FILE_NAME),
        compose_session_agent_toml(&hers),
    )
    .expect("her toml");
    // Her own triage session, closed: never.
    archive(&zone, &session_id(&tola, Duty::Triage).to_string());
    assert!(triage.folder_made);

    let mut want = vec![long.clone(), taxes.clone(), delegated.clone()];
    want.sort();
    let start = Instant::now();
    let mut harvester = Harvester::default();
    assert_eq!(
        harvest_all(&mut harvester, &zone, &harvest, &agent, start, 4),
        want
    );

    // Her harvest folder renamed: a new harvester, the same answer.
    let renamed = crate::seed::Settled {
        path: "active/renamed".to_owned(),
        ..harvest.clone()
    };
    std::fs::rename(zone.join(&harvest.path), zone.join(&renamed.path)).expect("renamed");
    let mut again = Harvester::default();
    assert_eq!(
        harvest_all(&mut again, &zone, &renamed, &agent, start, 4),
        want
    );
}

/// What a folder that arrived before its files is keyed by: nothing yet.
/// A session folder whose record has not synced, and one whose
/// `agent.toml` does not read yet, are read again after [`RETRY`] and then
/// harvested under their real id — never under a path.
#[tokio::test(start_paused = true)]
async fn an_archive_without_its_identity_yet_is_read_again_and_keyed_by_its_id() {
    let choices = choices();
    let tola = tola(&choices);
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    let now = at("2026-10-05T09:30:00+00:00");
    let harvest = make(&zone, &tola, &choices.decl, Duty::Harvest, "harvest", now);
    let agent = session(&tola, &choices.decl, Duty::Harvest, &room("harvest"), now);
    let (taxes, path) = closed(&zone, "taxes", at("2026-10-06T10:00:00+00:00"));
    let record = std::fs::read_to_string(zone.join(&path).join("README.md")).expect("record");
    std::fs::remove_file(zone.join(&path).join("README.md")).expect("not synced yet");
    let (garden, garden_path) = closed(&zone, "garden", at("2026-10-06T11:00:00+00:00"));
    let mut theirs = session(&tola, &choices.decl, Duty::Triage, &room("garden"), now);
    theirs.id = garden.parse().expect("ulid");
    let toml = compose_session_agent_toml(&theirs);
    std::fs::write(
        zone.join(&garden_path).join(FILE_NAME),
        &toml[..toml.len() / 2],
    )
    .expect("half written");

    let start = Instant::now();
    let mut harvester = Harvester::default();
    assert!(harvest_all(&mut harvester, &zone, &harvest, &agent, start, 2).is_empty());

    std::fs::write(zone.join(&path).join("README.md"), record).expect("synced");
    std::fs::write(zone.join(&garden_path).join(FILE_NAME), toml).expect("whole");
    assert!(harvest_all(&mut harvester, &zone, &harvest, &agent, start, 2).is_empty());
    let mut want = vec![taxes, garden];
    want.sort();
    assert_eq!(
        harvest_all(&mut harvester, &zone, &harvest, &agent, start + RETRY, 2),
        want
    );
}

/// A large archive is read [`READS_PER_STEP`] folders a step and handed
/// [`IN_FLIGHT`] at a time, an unchanged archive is not listed again, and
/// every new session is handed once in the end.
#[tokio::test(start_paused = true)]
async fn a_large_archive_is_read_in_bounded_steps_and_every_new_one_handed_once() {
    let choices = choices();
    let tola = tola(&choices);
    let root = tempfile::tempdir().expect("tempdir");
    let zone = root.path().join("60-sessions");
    let now = at("2026-10-05T09:30:00+00:00");
    let harvest = make(&zone, &tola, &choices.decl, Duty::Harvest, "harvest", now);
    let agent = session(&tola, &choices.decl, Duty::Harvest, &room("harvest"), now);
    let count = READS_PER_STEP * 2 + 3;
    let mut want: Vec<String> = (0..count)
        .map(|n| closed(&zone, &format!("s{n}"), at("2026-10-06T10:00:00+00:00")).0)
        .collect();
    want.sort();

    let start = Instant::now();
    let mut harvester = Harvester::default();
    let first = harvester.step(&zone, &harvest.path, &agent, start);
    assert_eq!(first.len(), IN_FLIGHT);
    assert_eq!(harvester.unread.len(), count - READS_PER_STEP);
    assert!(harvester
        .step(&zone, &harvest.path, &agent, start)
        .is_empty());
    assert_eq!(harvester.unread.len(), count - 2 * READS_PER_STEP);
    let met = harvester.met.len();

    let mut handed: Vec<String> = Vec::new();
    for closed in first {
        harvester.acknowledged(&closed.id, true, start);
        handed.push(closed.id);
    }
    handed.extend(harvest_all(
        &mut harvester,
        &zone,
        &harvest,
        &agent,
        start,
        20,
    ));
    handed.sort();
    assert_eq!(handed, want);
    assert_eq!(harvester.met.len(), met, "nothing new to meet");
}
