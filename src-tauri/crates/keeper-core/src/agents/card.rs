//! An agent's card: a session task file with the agent keys of AD-386
//! (stories 92.1, 92.2).
//!
//! A card is an ordinary task (`tags: [task]`, one of the four `status:`
//! columns) whose frontmatter may also carry nine agent keys. `run:` is the
//! agent's run, never a column; the host writes it. `scheduled_by:` marks a
//! schedule an agent wrote, which runs only after a person's tick (Q16), and
//! `integrity: untrusted` marks a card made from outside content (Q17). An
//! agent's write never removes either mark: [`stamp_agent_write`] is the one
//! function every door an agent writes a card through passes.

use chrono::{DateTime, FixedOffset};
use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::home::is_agent_id;
use crate::agents::label::Integrity;
use crate::agents::log::HostSlug;
use crate::notes::frontmatter::{FieldValue, Frontmatter};

/// The agent's run on the card.
pub const RUN: &str = "run";
/// The agent the card is for: an agent id of the card's drive.
pub const ASSIGNEE: &str = "assignee";
/// A host slug the card's run is pinned to.
pub const HOST: &str = "host";
/// Who asked: a Matrix user id.
pub const REQUESTED_BY: &str = "requested_by";
/// When it runs, kept raw: keeper-sync's parser answers whether it reads.
pub const SCHEDULE: &str = "schedule";
/// The window the last run ran in, RFC 3339.
pub const LAST_RUN: &str = "last_run";
/// The workflow it runs.
pub const WORKFLOW: &str = "workflow";
/// The agent whose write set the schedule or the workflow.
pub const SCHEDULED_BY: &str = "scheduled_by";
/// `untrusted`: the card was made from outside content.
pub const INTEGRITY: &str = "integrity";

/// The nine agent keys, in the order a card keeps them.
pub const KEYS: [&str; 9] = [
    RUN,
    ASSIGNEE,
    HOST,
    REQUESTED_BY,
    SCHEDULE,
    LAST_RUN,
    WORKFLOW,
    SCHEDULED_BY,
    INTEGRITY,
];

/// The person whose tick allowed a schedule an agent wrote (R76): the
/// requester of the card's scheduled runs. A person's key, as
/// `scheduled_by:` is keeper's: no agent's write sets or changes it.
pub const ALLOWED_BY: &str = "allowed_by";

/// The six values of `run:` (`waiting` by ruling R25).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Run {
    Queued,
    Running,
    Waiting,
    Blocked,
    Review,
    Failed,
}

impl Run {
    pub const ALL: [Run; 6] = [
        Run::Queued,
        Run::Running,
        Run::Waiting,
        Run::Blocked,
        Run::Review,
        Run::Failed,
    ];

    /// The word the card holds.
    pub fn as_str(self) -> &'static str {
        match self {
            Run::Queued => "queued",
            Run::Running => "running",
            Run::Waiting => "waiting",
            Run::Blocked => "blocked",
            Run::Review => "review",
            Run::Failed => "failed",
        }
    }

    /// The value a card's `run:` holds, trimmed and lower-cased as a
    /// `status:` is; `None` for anything else, a column's name included.
    pub fn parse(word: &str) -> Option<Run> {
        let word = word.trim().to_ascii_lowercase();
        Run::ALL.into_iter().find(|run| run.as_str() == word)
    }
}

/// One agent key as the card holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field<T> {
    Read(T),
    /// Present, but not of the key's grammar: kept verbatim.
    Unreadable(String),
}

impl<T> Field<T> {
    fn of(raw: &str, read: impl FnOnce(&str) -> Option<T>) -> Field<T> {
        read(raw.trim()).map_or_else(|| Field::Unreadable(raw.to_owned()), Field::Read)
    }
}

/// A card's nine agent keys, each read by its grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardAgent {
    pub run: Option<Field<Run>>,
    /// An agent id of the card's drive.
    pub assignee: Option<Field<String>>,
    /// The pin: a host slug.
    pub host: Option<Field<HostSlug>>,
    pub requested_by: Option<Field<OwnedUserId>>,
    /// Kept raw: whether it reads is keeper-sync's parser's answer.
    pub schedule: Option<String>,
    pub last_run: Option<Field<DateTime<FixedOffset>>>,
    /// A folder name under `_workflows/`.
    pub workflow: Option<Field<String>>,
    pub scheduled_by: Option<Field<OwnedUserId>>,
    /// Only `untrusted` reads.
    pub integrity: Option<Field<Integrity>>,
}

impl CardAgent {
    /// The agent keys of a card's text, or `None` when its frontmatter has
    /// none of the nine.
    pub fn of_text(text: &str) -> Option<CardAgent> {
        Self::of_frontmatter(&Frontmatter::parse(text).0)
    }

    /// The agent keys of a card's frontmatter, read from the file's own
    /// entries: each key present is either read by its grammar or kept as
    /// unreadable — a value of another type, a construct the parser does not
    /// model (`run: |`, `!!str`), or a key written twice. An empty value is
    /// an absent key.
    pub fn of_frontmatter(fm: &Frontmatter) -> Option<CardAgent> {
        if KEYS.iter().all(|key| raw(fm, key).is_none()) {
            return None;
        }
        let user = |text: &str| UserId::parse(text).ok();
        let at = |key: &str| raw(fm, key);
        Some(CardAgent {
            run: at(RUN).map(|v| v.field(Run::parse)),
            assignee: at(ASSIGNEE).map(|v| v.field(|id| is_agent_id(id).then(|| id.to_owned()))),
            host: at(HOST).map(|v| v.field(|slug| HostSlug::new(slug).ok())),
            requested_by: at(REQUESTED_BY).map(|v| v.field(user)),
            schedule: at(SCHEDULE).map(Raw::text),
            last_run: at(LAST_RUN).map(|v| v.field(|at| DateTime::parse_from_rfc3339(at).ok())),
            workflow: at(WORKFLOW)
                .map(|v| v.field(|name| is_workflow_name(name).then(|| name.to_owned()))),
            scheduled_by: at(SCHEDULED_BY).map(|v| v.field(user)),
            integrity: at(INTEGRITY).map(|v| {
                v.field(|word| {
                    word.eq_ignore_ascii_case(Integrity::Untrusted.as_word())
                        .then_some(Integrity::Untrusted)
                })
            }),
        })
    }

    /// Whether the card carries a schedule an agent wrote that no person has
    /// allowed: such a card is never due. An unreadable mark still marks.
    pub fn marked(&self) -> bool {
        self.scheduled_by.is_some()
    }
}

/// One agent key as the file holds it.
#[derive(Debug, Clone)]
enum Raw {
    /// A single string value.
    Text(String),
    /// Present, but not one string: kept as the file spells it.
    Opaque(String),
}

impl Raw {
    fn field<T>(self, read: impl FnOnce(&str) -> Option<T>) -> Field<T> {
        match self {
            Raw::Text(text) => Field::of(&text, read),
            Raw::Opaque(raw) => Field::Unreadable(raw),
        }
    }

    fn text(self) -> String {
        match self {
            Raw::Text(text) | Raw::Opaque(text) => text,
        }
    }
}

fn raw(fm: &Frontmatter, key: &str) -> Option<Raw> {
    match (fm.count(key), fm.get(key)) {
        (0, _) => None,
        (1, Some(FieldValue::Str(text))) if text.trim().is_empty() => None,
        (1, Some(FieldValue::Str(text))) => Some(Raw::Text(text.clone())),
        (1, _) => Some(Raw::Opaque(
            fm.raw_value(key).unwrap_or_default().trim().to_owned(),
        )),
        _ => Some(Raw::Opaque(fm.lines_of(key).trim().to_owned())),
    }
}

/// `key`'s value as the board and the index keep it: the string a single
/// string value holds, or the file's own spelling of anything else.
pub fn raw_key(fm: &Frontmatter, key: &str) -> Option<String> {
    raw(fm, key).map(Raw::text)
}

/// A `_workflows/` folder name, as an agent's `[[menu]]` names one.
fn is_workflow_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.')
}

/// Whether a file's own frontmatter says it was made from outside content
/// (`integrity: untrusted`, Q17): the fact a read of it is labelled by.
pub fn marked_untrusted(text: &str) -> bool {
    let (fm, _) = Frontmatter::parse(text);
    let untrusted = Integrity::Untrusted.as_word();
    if fm.count(INTEGRITY) > 1 {
        // A key written twice reads as neither; any line saying it marks.
        return fm
            .lines_of(INTEGRITY)
            .to_ascii_lowercase()
            .contains(untrusted);
    }
    fm.as_string(INTEGRITY)
        .is_some_and(|word| word.trim().eq_ignore_ascii_case(untrusted))
}

/// One agent key, for the board: its value as the card holds it, and
/// whether it reads by its grammar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CardKeyVm {
    /// Trimmed when it reads (and lower-cased for `run:`); verbatim when not.
    pub value: String,
    pub readable: bool,
}

impl CardKeyVm {
    fn of<T>(field: &Field<T>, word: impl FnOnce(&T) -> String) -> CardKeyVm {
        match field {
            Field::Read(value) => CardKeyVm {
                value: word(value),
                readable: true,
            },
            Field::Unreadable(raw) => CardKeyVm {
                value: raw.clone(),
                readable: false,
            },
        }
    }
}

/// Who works a card and where (AD-386, UX-DR134): its agent keys, and the
/// two facts the card's session's log adds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct CardAgentVm {
    /// `queued | running | waiting | blocked | review | failed`; never a
    /// column, which `status:` alone decides.
    pub run: Option<CardKeyVm>,
    /// The agent id the card is for.
    pub assignee: Option<CardKeyVm>,
    /// The host the card is pinned to — not where it runs.
    pub host: Option<CardKeyVm>,
    /// Who asked, a Matrix user id.
    pub requested_by: Option<CardKeyVm>,
    /// `readable` is keeper-sync's parser's answer.
    pub schedule: Option<CardKeyVm>,
    /// The window the last run ran in, RFC 3339.
    pub last_run: Option<CardKeyVm>,
    pub workflow: Option<CardKeyVm>,
    /// The agent whose write set the schedule or the workflow: present, the
    /// card waits for a person's *Allow* and never runs before it.
    pub scheduled_by: Option<CardKeyVm>,
    /// `untrusted`: the card was made from outside content.
    pub integrity: Option<CardKeyVm>,
    /// The host holding the card's session now, from its log's claims —
    /// never the `host:` pin. `null` outside an agent's session or while no
    /// host holds it.
    pub running_on: Option<String>,
    /// Why no host can run it, from the session's latest `run` line while
    /// that line says `waiting` (`hesperia — a live host`).
    pub waiting: Option<String>,
}

impl CardAgentVm {
    /// The board's view of a card's agent keys. `schedule_readable` is
    /// keeper-sync's parser's answer about `schedule:`; `running_on` and
    /// `waiting` are the card's session's, from `.keeper/agents.db`.
    pub fn of(
        card: &CardAgent,
        schedule_readable: bool,
        running_on: Option<&str>,
        waiting: Option<&str>,
    ) -> CardAgentVm {
        let text = |value: &String| value.clone();
        let user = |user: &OwnedUserId| user.to_string();
        CardAgentVm {
            run: card
                .run
                .as_ref()
                .map(|f| CardKeyVm::of(f, |run| run.as_str().to_owned())),
            assignee: card.assignee.as_ref().map(|f| CardKeyVm::of(f, text)),
            host: card
                .host
                .as_ref()
                .map(|f| CardKeyVm::of(f, |slug| slug.as_str().to_owned())),
            requested_by: card.requested_by.as_ref().map(|f| CardKeyVm::of(f, user)),
            schedule: card.schedule.clone().map(|raw| CardKeyVm {
                value: raw,
                readable: schedule_readable,
            }),
            last_run: card
                .last_run
                .as_ref()
                .map(|f| CardKeyVm::of(f, DateTime::to_rfc3339)),
            workflow: card.workflow.as_ref().map(|f| CardKeyVm::of(f, text)),
            scheduled_by: card.scheduled_by.as_ref().map(|f| CardKeyVm::of(f, user)),
            integrity: card
                .integrity
                .as_ref()
                .map(|f| CardKeyVm::of(f, |word| word.as_word().to_owned())),
            running_on: running_on.map(str::to_owned),
            waiting: waiting.map(str::to_owned),
        }
    }
}

/// The keys a person's move of a card writes (`compile_move`).
const MOVE_KEYS: [&str; 2] = [
    crate::sessions::tasks::TASK_STATUS_KEY,
    crate::notes::order::NOTE_ORDER_KEY,
];

/// The host's own keys: what its run writer sets, and what an agent's write
/// never changes.
pub const HOST_KEYS: [&str; 2] = [RUN, LAST_RUN];

/// `text` with the host's keys set — `run:`, and `last_run:` when the run
/// took a window — or `None` when the card already says both, once each, so
/// the host writes only on a transition. A key the card lacks goes in on
/// its own line beside the agent keys, with an unchanged line between it
/// and the `status:` and `order:` a person's move writes, so a line merge of
/// the two edits never finds them touching (Q10); a key written twice is
/// written once.
pub fn set_host_keys(text: &str, run: Run, last_run: Option<&str>) -> Option<String> {
    let (frontmatter, _) = Frontmatter::parse(text);
    let says = |key: &str, value: &str| {
        frontmatter.count(key) == 1 && frontmatter.as_string(key).map(str::trim) == Some(value)
    };
    let new_run = !(frontmatter.count(RUN) == 1
        && frontmatter.as_string(RUN).and_then(Run::parse) == Some(run));
    let new_window = last_run.filter(|at| !says(LAST_RUN, at));
    if !new_run && new_window.is_none() {
        return None;
    }
    let set = |out: &str, key: &str, value: &str| {
        let out = if Frontmatter::parse(out).0.count(key) > 1 {
            Frontmatter::remove_all_in(out, key)
        } else {
            out.to_owned()
        };
        Frontmatter::set_apart_in(
            &out,
            key,
            FieldValue::Str(value.to_owned()),
            &KEYS,
            &MOVE_KEYS,
        )
    };
    let mut out = text.to_owned();
    if new_run {
        out = set(&out, RUN, run.as_str());
    }
    if let Some(at) = new_window {
        out = set(&out, LAST_RUN, at);
    }
    Some(out)
}

/// Whether `text` is a card: a file tagged `task`.
fn is_task(text: &str) -> bool {
    let (fm, body_at) = Frontmatter::parse(text);
    let tags = crate::notes::tags::note_tags(&fm, text.get(body_at..).unwrap_or(""));
    crate::sessions::shape::KindTag::of(&tags) == Some(crate::sessions::shape::KindTag::Task)
}

/// What an agent's write of a session file stores, given its bytes before
/// it (`None` for a new file), the bytes the agent wrote, the agent's user
/// and the integrity of the session it wrote from. It runs on every
/// markdown file an agent writes, card or not, so a note retagged as a card
/// carries what the stamp put on it (R119):
///
/// - a write that sets or changes `schedule:` or `workflow:` — or makes a
///   file with either into a card — stores `scheduled_by: <agent>`,
///   whatever the agent wrote there, and drops `allowed_by:`;
/// - any other write keeps `scheduled_by:` and `allowed_by:` exactly as the
///   file had them — dropped, replaced, duplicated or newly written, they
///   are put back (or taken out);
/// - a write from a session at `untrusted` integrity stores
///   `integrity: untrusted`; an agent may write that itself; any other
///   write keeps the file's `integrity:` exactly as it was;
/// - `run:` and `last_run:` are the host's: every agent write keeps them
///   exactly as the file had them.
///
/// "Exactly" is the file's own lines, so a key the parser cannot read, or
/// one written twice, is put back as it was, and a write that duplicates a
/// protected key stores the single line the file had. Every other byte is
/// the agent's.
pub fn stamp_agent_write(
    old: Option<&str>,
    new: &str,
    agent: &UserId,
    integrity: Integrity,
) -> String {
    let before = old.map(|text| Frontmatter::parse(text).0);
    let (after, _) = Frontmatter::parse(new);
    let was = |key: &str| {
        before
            .as_ref()
            .map(|fm| fm.lines_of(key))
            .unwrap_or_default()
    };
    let changes = |key: &str| after.count(key) > 0 && after.lines_of(key) != was(key);
    let becomes_card = is_task(new)
        && !old.is_some_and(is_task)
        && (after.count(SCHEDULE) > 0 || after.count(WORKFLOW) > 0);
    let schedules = changes(SCHEDULE) || changes(WORKFLOW) || becomes_card;
    // The file's own lines for `key`, back in place of whatever the write
    // says; untouched when the write already says exactly that.
    let keep = |out: String, key: &str| {
        let lines = was(key);
        if Frontmatter::parse(&out).0.lines_of(key) == lines {
            out
        } else {
            Frontmatter::replace_lines_in(&out, key, &lines, &KEYS)
        }
    };
    // One fresh value, written once.
    let fresh = |out: String, key: &str, value: &str| {
        let (fm, _) = Frontmatter::parse(&out);
        if fm.count(key) == 1 && fm.as_string(key) == Some(value) {
            return out;
        }
        let out = Frontmatter::remove_all_in(&out, key);
        Frontmatter::set_after_in(&out, &KEYS, key, FieldValue::Str(value.to_owned()))
    };
    let mut out = new.to_owned();
    for key in HOST_KEYS {
        out = keep(out, key);
    }
    if schedules {
        out = fresh(out, SCHEDULED_BY, agent.as_str());
        out = Frontmatter::remove_all_in(&out, ALLOWED_BY);
    } else {
        out = keep(out, SCHEDULED_BY);
        out = keep(out, ALLOWED_BY);
    }
    let untrusted = Integrity::Untrusted.as_word();
    let lowers = after.count(INTEGRITY) == 1
        && after
            .as_string(INTEGRITY)
            .is_some_and(|word| word.trim().eq_ignore_ascii_case(untrusted));
    // An agent may mark its own file untrusted; only a person lifts it.
    if !lowers {
        out = if integrity == Integrity::Untrusted {
            fresh(out, INTEGRITY, untrusted)
        } else {
            keep(out, INTEGRITY)
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use matrix_sdk::ruma::OwnedUserId;

    use super::*;

    fn nixi() -> OwnedUserId {
        UserId::parse("@nixi:h").expect("user")
    }

    fn field(text: &str, key: &str) -> Option<String> {
        Frontmatter::parse(text).0.as_string(key).map(str::to_owned)
    }

    const CARD: &str = "---\ntags: [task]\ntitle: Tidy the inbox\nstatus: todo\nassignee: tola-grey\n---\n\nSort what came in.\n";

    #[test]
    fn run_reads_its_six_words_and_nothing_else() {
        for run in Run::ALL {
            assert_eq!(Run::parse(run.as_str()), Some(run));
        }
        assert_eq!(Run::parse(" Running "), Some(Run::Running));
        assert_eq!(Run::parse("todo"), None, "a column is not a run");
        assert_eq!(Run::parse("blocked!"), None);
    }

    /// A card's text with `pairs` quoted after `tags: [task]`.
    fn card_of(pairs: &[(&str, &str)]) -> String {
        let keys: String = pairs
            .iter()
            .map(|(key, value)| format!("{key}: \"{value}\"\n"))
            .collect();
        format!("---\ntags: [task]\n{keys}---\n\nBody.\n")
    }

    /// AC1: each key reads by its grammar or is kept verbatim as unreadable;
    /// a card with none of the nine has no agent block.
    #[test]
    fn card_agent_keys_parse_or_say_unreadable() {
        for run in Run::ALL {
            let card = CardAgent::of_text(&card_of(&[(RUN, run.as_str())])).expect("an agent card");
            assert_eq!(card.run, Some(Field::Read(run)));
        }
        let readable = CardAgent::of_text(&card_of(&[
            (RUN, "Running "),
            (ASSIGNEE, "tola-grey"),
            (HOST, "hesperia"),
            (REQUESTED_BY, "@nixi:h"),
            (SCHEDULE, "@daily"),
            (LAST_RUN, "2026-10-04T09:00:00+02:00"),
            (WORKFLOW, "triage"),
            (SCHEDULED_BY, "@nixi:h"),
            (INTEGRITY, "untrusted"),
        ]))
        .expect("an agent card");
        assert_eq!(readable.run, Some(Field::Read(Run::Running)));
        assert_eq!(readable.assignee, Some(Field::Read("tola-grey".to_owned())));
        assert!(matches!(readable.host, Some(Field::Read(_))));
        assert!(matches!(readable.requested_by, Some(Field::Read(_))));
        assert!(matches!(readable.last_run, Some(Field::Read(_))));
        assert!(matches!(readable.workflow, Some(Field::Read(_))));
        assert!(matches!(readable.scheduled_by, Some(Field::Read(_))));
        assert_eq!(readable.integrity, Some(Field::Read(Integrity::Untrusted)));

        let unreadable = CardAgent::of_text(&card_of(&[
            (RUN, "todo"),
            (ASSIGNEE, "Nixi"),
            (HOST, "Hesperia!"),
            (REQUESTED_BY, "nixi"),
            (LAST_RUN, "yesterday"),
            (WORKFLOW, "../triage"),
            (SCHEDULED_BY, "Nixi"),
            (INTEGRITY, "agent"),
        ]))
        .expect("an agent card");
        fn kept<T>(raw: &str) -> Option<Field<T>> {
            Some(Field::Unreadable(raw.to_owned()))
        }
        assert_eq!(unreadable.run, kept("todo"), "a column is not a run");
        assert_eq!(unreadable.assignee, kept("Nixi"));
        assert_eq!(unreadable.host, kept("Hesperia!"));
        assert_eq!(unreadable.requested_by, kept("nixi"));
        assert_eq!(unreadable.last_run, kept("yesterday"));
        assert_eq!(unreadable.workflow, kept("../triage"));
        assert_eq!(unreadable.scheduled_by, kept("Nixi"));
        assert_eq!(unreadable.integrity, kept("agent"));
        assert!(unreadable.marked(), "an unreadable mark still marks");
    }

    /// R119 (R4-07): a key the parser does not model, a value of another
    /// type and a key written twice are present and unreadable — read from
    /// the file's own bytes, BOM and CRLF included — never absent, and never
    /// flattened into a valid value.
    #[test]
    fn an_unreadable_key_stays_visible_from_the_raw_file() {
        let text = "\u{feff}---\r\ntags: [task]\r\nrun: [running]\r\nscheduled_by: !!str \"@nixi:h\"\r\nhost: |\r\n  hesperia\r\nassignee: tola-grey\r\nassignee: nixi\r\n---\r\n\r\nBody.\r\n";
        let card = CardAgent::of_text(text).expect("an agent card");
        assert!(
            matches!(card.run, Some(Field::Unreadable(_))),
            "{:?}",
            card.run
        );
        assert!(
            matches!(card.scheduled_by, Some(Field::Unreadable(_))),
            "{:?}",
            card.scheduled_by
        );
        assert!(card.marked(), "an opaque mark still marks");
        assert!(
            matches!(card.host, Some(Field::Unreadable(_))),
            "{:?}",
            card.host
        );
        assert!(
            matches!(card.assignee, Some(Field::Unreadable(_))),
            "a key written twice reads as neither: {:?}",
            card.assignee
        );
        let vm = CardAgentVm::of(&card, true, None, None);
        assert_eq!(vm.run.map(|key| key.readable), Some(false));
        assert!(marked_untrusted(
            "---\ntags: [task]\nintegrity: agent\nintegrity: untrusted\n---\n"
        ));
    }

    #[test]
    fn a_card_without_agent_keys_has_no_agent_block() {
        let text = "---\ntags: [task]\nstatus: todo\norder: 1\nrun: \" \"\n---\n";
        assert_eq!(CardAgent::of_text(text), None);
    }

    /// The board's view: each key with whether it reads, the schedule's
    /// readability as keeper-sync's parser answered, and the session's facts.
    #[test]
    fn the_board_sees_each_key_and_where_it_runs() {
        let card = CardAgent::of_text(&card_of(&[
            (RUN, "Waiting"),
            (ASSIGNEE, "tola-grey"),
            (HOST, "hesperia"),
            (SCHEDULE, "every 30s"),
        ]))
        .expect("an agent card");
        let vm = CardAgentVm::of(
            &card,
            false,
            Some("electra"),
            Some("hesperia — a live host"),
        );
        let key = |value: &str, readable| {
            Some(CardKeyVm {
                value: value.to_owned(),
                readable,
            })
        };
        assert_eq!(vm.run, key("waiting", true));
        assert_eq!(vm.host, key("hesperia", true));
        assert_eq!(vm.schedule, key("every 30s", false));
        assert_eq!(vm.scheduled_by, None);
        assert_eq!(vm.running_on.as_deref(), Some("electra"));
        assert_eq!(vm.waiting.as_deref(), Some("hesperia — a live host"));
    }

    /// R122 (R4-14): on a card whose agent keys sit next to `status:` and
    /// `order:`, or that has none, the host's first `run:` goes where an
    /// unchanged line separates it from both; a duplicated `run:` is
    /// written once.
    #[test]
    fn host_keys_never_touch_the_lines_a_move_writes() {
        let before =
            "---\ntags: [task]\nassignee: tola-grey\nstatus: todo\norder: 1\n---\n\nBody.\n";
        assert_eq!(
            set_host_keys(before, Run::Queued, None).expect("a transition"),
            "---\ntags: [task]\nrun: queued\nassignee: tola-grey\nstatus: todo\norder: 1\n---\n\nBody.\n"
        );
        let bare = "---\ntags: [task]\ntitle: Sort\nstatus: todo\norder: 1\n---\n\nBody.\n";
        assert_eq!(
            set_host_keys(bare, Run::Queued, None).expect("a transition"),
            "---\ntags: [task]\nrun: queued\ntitle: Sort\nstatus: todo\norder: 1\n---\n\nBody.\n"
        );
        let unmoved = "---\ntags: [task]\ntitle: Sort\n---\n";
        assert_eq!(
            set_host_keys(unmoved, Run::Queued, None).expect("a transition"),
            "---\ntags: [task]\nrun: queued\ntitle: Sort\n---\n",
            "a move adds status before the closing fence, so run goes elsewhere"
        );
        let twice = "---\ntags: [task]\nrun: queued\nrun: queued\n---\n";
        assert_eq!(
            set_host_keys(twice, Run::Queued, None).expect("written once"),
            "---\nrun: queued\ntags: [task]\n---\n"
        );
    }

    /// Q10: the host writes only on a transition, one key at a time, and a
    /// key the card lacks lands among the agent keys — below `status:` and
    /// `order:` with a line it does not change between.
    #[test]
    fn host_keys_are_written_on_a_transition_beside_the_agent_keys() {
        let card = "---\ntags: [task]\nstatus: todo\norder: 2\nassignee: tola-grey\nrequested_by: \"@nixi:h\"\n---\n\nBody.\n";
        let queued = set_host_keys(card, Run::Queued, None).expect("a transition");
        assert_eq!(
            queued,
            "---\ntags: [task]\nstatus: todo\norder: 2\nassignee: tola-grey\nrequested_by: \"@nixi:h\"\nrun: queued\n---\n\nBody.\n"
        );
        assert_eq!(set_host_keys(&queued, Run::Queued, None), None);
        let running = set_host_keys(&queued, Run::Running, Some("2026-10-04T09:00:00+02:00"))
            .expect("a transition");
        assert_eq!(
            running,
            queued.replace(
                "run: queued\n",
                "run: running\nlast_run: 2026-10-04T09:00:00+02:00\n"
            )
        );
        assert_eq!(
            set_host_keys(&running, Run::Running, Some("2026-10-04T09:00:00+02:00")),
            None
        );
        let review = set_host_keys(&running, Run::Review, None).expect("a transition");
        assert_eq!(review, running.replace("run: running\n", "run: review\n"));
    }

    /// R76: an agent never writes who allowed a schedule; a new schedule
    /// drops it, so it waits for a new tick.
    #[test]
    fn an_agent_neither_writes_nor_keeps_a_stale_allowed_by() {
        let allowed = CARD.replace(
            "assignee",
            "schedule: \"@daily\"\nallowed_by: \"@tgorka:h\"\nassignee",
        );
        let forged = allowed.replace("@tgorka:h", "@marta:h");
        let kept = stamp_agent_write(Some(&allowed), &forged, &nixi(), Integrity::Agent);
        assert_eq!(field(&kept, ALLOWED_BY).as_deref(), Some("@tgorka:h"));
        let dropped = stamp_agent_write(
            Some(&allowed),
            &allowed.replace("allowed_by: \"@tgorka:h\"\n", ""),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&dropped, ALLOWED_BY).as_deref(), Some("@tgorka:h"));
        let invented = stamp_agent_write(
            Some(CARD),
            &CARD.replace("assignee", "allowed_by: \"@tgorka:h\"\nassignee"),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&invented, ALLOWED_BY), None);
        let rescheduled = stamp_agent_write(
            Some(&allowed),
            &allowed.replace("@daily", "@weekly"),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&rescheduled, ALLOWED_BY), None);
        assert_eq!(
            field(&rescheduled, SCHEDULED_BY).as_deref(),
            Some("@nixi:h")
        );
    }

    #[test]
    fn a_file_marked_untrusted_says_so() {
        assert!(marked_untrusted(
            &CARD.replace("assignee", "integrity: Untrusted\nassignee")
        ));
        assert!(!marked_untrusted(CARD));
        assert!(
            !marked_untrusted("integrity: untrusted\n"),
            "a body line is not frontmatter"
        );
    }

    /// Q16, Q17: an agent's schedule is marked and stays marked; a session
    /// at `untrusted` integrity marks the card; nothing else is touched.
    #[test]
    fn an_agents_card_write_is_stamped_and_never_unstamped() {
        let scheduled = CARD.replace("assignee", "schedule: \"@daily\"\nassignee");
        let stamped = stamp_agent_write(None, &scheduled, &nixi(), Integrity::Agent);
        assert_eq!(field(&stamped, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&stamped, INTEGRITY), None);

        // A rewrite that drops the mark gets it back; one that leaves the
        // schedule as it was adds nothing more.
        let dropped = stamp_agent_write(Some(&stamped), &scheduled, &nixi(), Integrity::Agent);
        assert_eq!(field(&dropped, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        let plain = stamp_agent_write(Some(CARD), CARD, &nixi(), Integrity::Owner);
        assert_eq!(plain, CARD);

        // A forged mark is overwritten by the writer's own.
        let forged = scheduled.replace("assignee", "scheduled_by: \"@tgorka:h\"\nassignee");
        let honest = stamp_agent_write(Some(CARD), &forged, &nixi(), Integrity::Agent);
        assert_eq!(field(&honest, SCHEDULED_BY).as_deref(), Some("@nixi:h"));

        let untrusted = stamp_agent_write(None, CARD, &nixi(), Integrity::Untrusted);
        assert_eq!(field(&untrusted, INTEGRITY).as_deref(), Some("untrusted"));
        let kept = stamp_agent_write(Some(&untrusted), CARD, &nixi(), Integrity::Owner);
        assert_eq!(field(&kept, INTEGRITY).as_deref(), Some("untrusted"));
        assert!(kept.ends_with("\nSort what came in.\n"));
        let rewritten = stamp_agent_write(
            Some(&untrusted),
            &untrusted.replace("integrity: untrusted", "integrity: owner"),
            &nixi(),
            Integrity::Owner,
        );
        assert_eq!(field(&rewritten, INTEGRITY).as_deref(), Some("untrusted"));
        let reassigned = stamp_agent_write(
            Some(&stamped),
            &stamped.replace("@nixi:h", "@tgorka:h"),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&reassigned, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
    }

    /// An ordinary rewrite — the schedule as it was — that replaces either
    /// mark's value, or writes one the card never had, leaves the card's
    /// marks exactly as they were.
    #[test]
    fn an_agents_rewrite_cannot_replace_a_mark() {
        let scheduled = CARD.replace("assignee", "schedule: \"@daily\"\nassignee");
        let stamped = stamp_agent_write(None, &scheduled, &nixi(), Integrity::Untrusted);
        let rewrite = stamped
            .replace("scheduled_by: \"@nixi:h\"", "scheduled_by: \"@tgorka:h\"")
            .replace("integrity: untrusted", "integrity: owner")
            .replace("Sort what came in.", "Sorted.");
        assert_eq!(field(&rewrite, SCHEDULED_BY).as_deref(), Some("@tgorka:h"));
        assert_eq!(field(&rewrite, INTEGRITY).as_deref(), Some("owner"));
        let kept = stamp_agent_write(Some(&stamped), &rewrite, &nixi(), Integrity::Owner);
        assert_eq!(field(&kept, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&kept, INTEGRITY).as_deref(), Some("untrusted"));
        assert!(kept.ends_with("\nSorted.\n"), "{kept}");

        // A mark the card never had is not the agent's to add.
        let forged = CARD.replace(
            "assignee",
            "scheduled_by: \"@tgorka:h\"\nintegrity: owner\nassignee",
        );
        let plain = stamp_agent_write(Some(CARD), &forged, &nixi(), Integrity::Owner);
        assert_eq!(field(&plain, SCHEDULED_BY), None);
        assert_eq!(field(&plain, INTEGRITY), None);
    }

    /// R119 (R4-03): `run:` and `last_run:` are the host's on every agent
    /// write — a rewrite cannot change, drop or add them, and a new file
    /// cannot introduce them.
    #[test]
    fn an_agent_cannot_forge_the_hosts_keys() {
        let running = CARD.replace(
            "assignee",
            "run: running\nlast_run: 2026-10-04T09:00:00+02:00\nassignee",
        );
        let forged = running
            .replace("run: running", "run: review")
            .replace("last_run: 2026-10-04T09:00:00+02:00\n", "");
        let kept = stamp_agent_write(Some(&running), &forged, &nixi(), Integrity::Agent);
        let (fm, _) = Frontmatter::parse(&kept);
        assert_eq!(fm.lines_of(RUN), "run: running\n");
        assert_eq!(
            fm.lines_of(LAST_RUN),
            "last_run: 2026-10-04T09:00:00+02:00\n"
        );
        assert!(kept.ends_with("\nSort what came in.\n"));
        let invented = stamp_agent_write(
            None,
            &CARD.replace(
                "assignee",
                "run: review\nlast_run: 2099-01-01T00:00:00Z\nassignee",
            ),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(invented, CARD);
    }

    /// R119 (R4-04): a schedule staged in a note and the note then retagged
    /// as a card is stamped as the agent's, and an `allowed_by:` the note
    /// carried is never taken as a person's; marks survive being retagged
    /// away and back.
    #[test]
    fn retagging_never_launders_a_schedule() {
        let note = "---\ntags: [note]\ntitle: Daily\nschedule: \"@daily\"\nallowed_by: \"@tgorka:h\"\n---\n\nRun daily.\n";
        let staged = stamp_agent_write(None, note, &nixi(), Integrity::Agent);
        assert_eq!(field(&staged, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&staged, ALLOWED_BY), None);
        // Even a person's note an agent turns into a scheduled card.
        let persons = note.replace("allowed_by: \"@tgorka:h\"\n", "");
        let carded = stamp_agent_write(
            Some(&persons),
            &persons.replace("[note]", "[task]"),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&carded, SCHEDULED_BY).as_deref(), Some("@nixi:h"));

        let marked = CARD.replace(
            "assignee",
            "schedule: \"@daily\"\nscheduled_by: \"@nixi:h\"\nintegrity: untrusted\nassignee",
        );
        let away = stamp_agent_write(
            Some(&marked),
            &marked
                .replace("[task]", "[note]")
                .replace("scheduled_by: \"@nixi:h\"\nintegrity: untrusted\n", ""),
            &nixi(),
            Integrity::Agent,
        );
        let gone = away.replace("scheduled_by: \"@nixi:h\"\nintegrity: untrusted\n", "");
        let stripped = stamp_agent_write(Some(&away), &gone, &nixi(), Integrity::Agent);
        let back = stamp_agent_write(
            Some(&stripped),
            &stripped.replace("[note]", "[task]"),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(field(&back, SCHEDULED_BY).as_deref(), Some("@nixi:h"));
        assert_eq!(field(&back, INTEGRITY).as_deref(), Some("untrusted"));
    }

    /// R119 (R4-05): a write that repeats a protected key stores exactly
    /// the single line the card had, or none.
    #[test]
    fn duplicated_protected_keys_normalise_to_the_cards_own() {
        let doubled = CARD.replace(
            "assignee",
            "allowed_by: \"@tgorka:h\"\nallowed_by: \"@tgorka:h\"\nassignee",
        );
        let none = stamp_agent_write(Some(CARD), &doubled, &nixi(), Integrity::Agent);
        assert_eq!(none, CARD);
        let allowed = CARD.replace("assignee", "allowed_by: \"@marta:h\"\nassignee");
        let one = stamp_agent_write(
            Some(&allowed),
            &doubled.replace(
                "assignee",
                "scheduled_by: \"@x:h\"\nscheduled_by: \"@x:h\"\nassignee",
            ),
            &nixi(),
            Integrity::Agent,
        );
        assert_eq!(one, allowed);
    }
}
