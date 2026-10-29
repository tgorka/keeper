//! The drives a session's agent may read (109.23; Q109-20 b, V17, D-34):
//! the session's `session_drives` row, the drives the agent offers
//! (`agent_profiles.drives`), and a person member's ticks.
//!
//! Every decision is here; the window renders what [`view`] answers and the
//! shell decides nothing:
//!
//! - **None is pre-selected.** No row, or an empty array, is no drive: a
//!   drive is ticked only when the row names it.
//! - **Only a person member changes it.** A member whose role is `owner` or
//!   `person`, in a session not archived, gets a control at all; anyone else
//!   gets none ([`view`] answers `None`), as the server refuses them (V17).
//! - **The ceiling is the agent's.** A drive is offered when an
//!   `agent_profiles` row of the session's agent names it — any of them, as
//!   the server reads it. A drive keeper knows of that no such row names is
//!   shown with that reason, never ticked.
//! - **The audience rule** (D-34): drive D may be ticked in database S only
//!   when readers(S) ⊆ readers(D). A drive is named after the database that
//!   mirrors its audience (AD-433), so readers(D) are the people of D's
//!   directory, which this device holds only when the person is in D.
//!   Where it does not, the drive is shown as offered and the server decides
//!   (`hq.v17.audience`).
//! - **Unticking writes `[]`**, never a delete: syncular refuses a write to
//!   a deleted id (`sync.row_deleted`), so a deleted row could never be
//!   ticked again.
//! - **A refusal reaches the row.** What became of the selections written
//!   here is the replica's per-row outcome record
//!   ([`crate::hq::port::Replica::row_writes`]), the same record a refused
//!   rename and the instance choice read — never inferred from the write
//!   returning, which only says the outbox took it.
//! - **The risk is said where a drive is ticked** ([`WARNING`]; D-34's
//!   accepted risk, R-HQ-79).
//! - **The Notes Assistant's tick** ([`Hq::session_for_note`], R-HQ-86):
//!   opened on a note, the dock ticks the note's drive as the person, by the
//!   same rules a click does. Nowhere else is a drive ticked for the person.
//!   keeper never takes the tick back. Any ticked drive this person has not
//!   been shown [`WARNING`] for on this device ([`SeenTicks`]) draws it
//!   beside its chip, with Untick and OK, wherever the session is shown.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::port::{Rejection, ReplicaError, RowOp, CONFLICT};
use crate::hq::sessions::{
    self, authored, int, json, names, object, opt_text, read, text, AgentNoteSessionVm,
    AgentSessionRef, Source,
};
use crate::hq::timeline::{self, SessionRow};
use crate::hq::{lock, Hq, HqError};

const TABLE: &str = "session_drives";

/// D-34's accepted risk (R10922-01, R-HQ-79), said wherever a drive is
/// ticked: once ticked, the agent may read the drive and call the web
/// without asking, even in a session that has read outside content, so text
/// it read there can lead it to send a file's contents out.
pub const WARNING: &str = "Once you tick a drive, the agent may read it and send what it reads \
     to the web, in a search or a page it fetches, without asking you, even after this chat has \
     read outside content. Text it read on the web or in a drive can lead it to do that.";

/// One drive of the menu: ticked (`ticked`), offered (`reason` absent), or
/// not allowed (`reason` says why). A ticked drive with a reason was ticked
/// before it stopped being allowed: it holds every other change back (V17
/// checks the whole selection), and unticking it unticks every such drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionDriveVm {
    pub name: String,
    pub ticked: bool,
    /// Why this drive cannot be ticked here, as the person reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    /// The server refused the newest selection written here, and this
    /// drive is what it would have changed: why.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refused: Option<String>,
}

/// A session's drive selection, for a person member who may change it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionDrivesVm {
    /// The drives the agent may read here, then the ones it may not, each
    /// by name.
    pub drives: Vec<AgentSessionDriveVm>,
    /// What ticking a drive allows ([`WARNING`]).
    pub warning: String,
    /// A selection written here waits for the server.
    pub pending: bool,
    /// The server refused the newest selection written here, as a sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refused: Option<String>,
    /// Ticked drives this person has not been shown [`WARNING`] for on this
    /// device (round 6 ruling, [`SeenTicks`]): a tick the Notes Assistant,
    /// another device or a carry made, or one whose record was lost. Drawn
    /// beside the chips with Untick and OK wherever the session is shown;
    /// read from the same selection as the chips, so the two agree.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<AgentUnseenTickVm>>", optional)]
    pub unseen: Vec<AgentUnseenTickVm>,
}

/// Who reads each database this device holds: the people of its directory.
/// A database not held, or whose directory has not arrived, is not here.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audiences(HashMap<String, BTreeSet<String>>);

impl Audiences {
    pub fn read(sources: &[Source]) -> Audiences {
        Audiences(
            sources
                .iter()
                .filter_map(|source| {
                    let people = people(source);
                    (!people.is_empty()).then(|| (source.database.clone(), people))
                })
                .collect(),
        )
    }

    fn of(&self, database: &str) -> Option<&BTreeSet<String>> {
        self.0.get(database)
    }

    fn databases(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }
}

/// The people of `source`'s own database, by actor id.
fn people(source: &Source) -> BTreeSet<String> {
    read(
        source,
        "SELECT id FROM actors WHERE dir = ?1 AND kind = 'person'",
        &[Value::from(source.database.as_str())],
    )
    .iter()
    .filter_map(|row| opt_text(row, "id"))
    .collect()
}

/// The one rule for who has the drive control, wherever it is drawn — the
/// session header, `/drives`, the list's right-click menu: a person member
/// (`owner` or `person`, as V17 requires) of a session that is not archived.
pub(crate) fn may_choose(role: &str, archived: bool) -> bool {
    !archived && matches!(role, "owner" | "person")
}

/// This person may change the session's drives ([`may_choose`]).
fn person_member(source: &Source, session_id: &str, session: &SessionRow) -> bool {
    let role = read(
        source,
        "SELECT role FROM session_members WHERE session_id = ?1 AND actor_id = ?2",
        &[Value::from(session_id), Value::from(source.actor.as_str())],
    );
    role.first()
        .is_some_and(|row| may_choose(text(row, "role"), session.archived))
}

/// The agents of `source`'s database that say what drives they read: an
/// agent none of whose instances says has no drive control anywhere.
pub(crate) fn agents_saying(source: &Source) -> HashSet<String> {
    read(source, "SELECT actor_id, drives FROM agent_profiles", &[])
        .iter()
        .filter(|row| json(row, "drives").is_array())
        .filter_map(|row| opt_text(row, "actor_id"))
        .collect()
}

/// The drives the agent offers — every name any of its `agent_profiles`
/// rows says, as the server reads the ceiling — or `None` while no row of
/// it says anything (an instance older than the column): no picker then.
fn offered(source: &Source, agent_actor: &str) -> Option<BTreeSet<String>> {
    let rows = read(
        source,
        "SELECT drives FROM agent_profiles WHERE actor_id = ?1",
        &[Value::from(agent_actor)],
    );
    let mut said = rows.iter().filter(|row| json(row, "drives").is_array());
    let first = said.next()?;
    let mut offered: BTreeSet<String> = names(first, "drives").into_iter().collect();
    for row in said {
        offered.extend(names(row, "drives"));
    }
    Some(offered)
}

/// Every drive any agent of this database offers: the drives keeper knows
/// of here, beside the databases it holds.
fn named_here(source: &Source) -> BTreeSet<String> {
    read(source, "SELECT drives FROM agent_profiles", &[])
        .iter()
        .flat_map(|row| names(row, "drives"))
        .collect()
}

/// The ticked drives, distinct, in the order the row names them, and the
/// row's version. No row is no drive.
fn ticked(source: &Source, session_id: &str) -> (Vec<String>, Option<i64>) {
    let rows = read(
        source,
        "SELECT drives, _sync_version AS version FROM session_drives WHERE session_id = ?1",
        &[Value::from(session_id)],
    );
    let Some(row) = rows.first() else {
        return (Vec::new(), None);
    };
    let mut seen = BTreeSet::new();
    let drives = names(row, "drives")
        .into_iter()
        .filter(|drive| seen.insert(drive.clone()))
        .collect();
    (drives, int(row, "version"))
}

/// Each ticked drive's tick id (`session_drives.tick_ids`, schema 5;
/// 109.28, R605-14), as the row names it: `{"at": <the row's updated_at_ms>,
/// "drives": {<drive>: <ULID>}}`. A mismatched `at` makes a retained map
/// unusable. The server must also invalidate maps on schema-4 writes:
/// repeated timestamps cannot distinguish them (syncular/README.md).
/// A drive with no id is a tick no device can have acknowledged.
pub(crate) fn tick_ids(source: &Source, session_id: &str) -> BTreeMap<String, String> {
    let rows = read(
        source,
        "SELECT drives, updated_at_ms, tick_ids FROM session_drives WHERE session_id = ?1",
        &[Value::from(session_id)],
    );
    let Some(row) = rows.first() else {
        return BTreeMap::new();
    };
    let ticks = json(row, "tick_ids");
    if ticks.get("at").and_then(Value::as_i64) != int(row, "updated_at_ms") {
        return BTreeMap::new();
    }
    let ticked = names(row, "drives");
    ticks
        .get("drives")
        .and_then(Value::as_object)
        .map(|ids| {
            ids.iter()
                .filter(|(drive, _)| ticked.contains(drive))
                .filter_map(|(drive, id)| Some((drive.clone(), id.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default()
}

/// The tick ids a selection writes: the ids of the drives `drives` keeps
/// from `before`, and a fresh ULID for each drive it ticks anew.
fn next_ticks(before: &BTreeMap<String, String>, drives: &[String]) -> BTreeMap<String, String> {
    drives
        .iter()
        .map(|drive| {
            let id = before
                .get(drive)
                .cloned()
                .unwrap_or_else(|| ulid::Ulid::new().to_string());
            (drive.clone(), id)
        })
        .collect()
}

/// Whether this person has a drive control in the session at all: a person
/// member of a live session whose agent says what drives it reads.
pub fn available(source: &Source, session_id: &str, session: &SessionRow) -> bool {
    person_member(source, session_id, session) && offered(source, &session.agent_actor).is_some()
}

/// The names, joined for a sentence: "Ana", "Ana and Ben", "Ana, Ben and Cy".
fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// What keeper knows of one drive in one session's database.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    /// The agent offers it and everyone who reads this database reads it.
    Allowed,
    /// The agent offers it; who reads it this device cannot know (it holds
    /// no copy of that database), so the server decides.
    Unknown,
    /// The agent does not offer it.
    NotOffered,
    /// The agent offers it, and these people read this database but not it.
    Audience(Vec<String>),
    /// Ticked, its readers unknown here, and the standing server refusal of
    /// a selection written here names it by the audience rule: it fails as
    /// surely as one keeper judged, until a newer selection sets that
    /// refusal aside (R10923-08).
    Refused,
}

/// One drive of a session as keeper judges it.
struct Judged {
    name: String,
    ticked: bool,
    verdict: Verdict,
}

impl Judged {
    /// It cannot be (or stay) in a selection the server takes.
    fn fails(&self) -> bool {
        matches!(
            self.verdict,
            Verdict::NotOffered | Verdict::Audience(_) | Verdict::Refused
        )
    }

    /// Why it cannot be ticked; `None` when it may be, and for a ticked
    /// drive, whose reason is the selection's ([`Stale`]).
    fn reason(&self, agent: &str, database: &str) -> Option<String> {
        let name = &self.name;
        match &self.verdict {
            _ if self.ticked => None,
            Verdict::Allowed | Verdict::Unknown | Verdict::Refused => None,
            Verdict::NotOffered => Some(format!("{agent} does not offer {name}.")),
            Verdict::Audience(outside) => Some(format!(
                "{} but cannot read {name}, so it cannot be ticked here.",
                readers(outside, database)
            )),
        }
    }
}

/// The ticked drives that stopped being allowed. V17 checks every drive of
/// a selection, so while one stays ticked every write is refused: they go
/// together — unticking any of them unticks all of them in one write — and
/// nothing else changes until they are gone (R10923-01, R10923-06).
struct Stale<'a>(Vec<&'a Judged>);

impl<'a> Stale<'a> {
    fn of(judged: &'a [Judged]) -> Stale<'a> {
        Stale(judged.iter().filter(|d| d.ticked && d.fails()).collect())
    }

    fn contains(&self, name: &str) -> bool {
        self.0.iter().any(|d| d.name == name)
    }

    fn names(&self) -> Vec<String> {
        self.0.iter().map(|d| d.name.clone()).collect()
    }

    /// Why they fail: "Nixie no longer offers them", "Nixie no longer
    /// offers gamma; Ben reads alpha but cannot read beta".
    fn why(&self, agent: &str, database: &str) -> String {
        let alone = self.0.len() == 1;
        let not_offered: Vec<String> = self
            .0
            .iter()
            .filter(|d| d.verdict == Verdict::NotOffered)
            .map(|d| d.name.clone())
            .collect();
        let mut clauses = Vec::new();
        if !not_offered.is_empty() {
            let them = if not_offered.len() == self.0.len() {
                if alone { "it" } else { "them" }.to_owned()
            } else {
                listed(&not_offered)
            };
            clauses.push(format!("{agent} no longer offers {them}"));
        }
        for drive in &self.0 {
            let it = if alone { "it" } else { drive.name.as_str() };
            match &drive.verdict {
                Verdict::Audience(outside) => clauses.push(format!(
                    "{} but cannot read {it}",
                    readers(outside, database)
                )),
                Verdict::Refused => clauses.push(format!(
                    "the server refused {it} because not everyone who reads {database} may \
                     read it"
                )),
                _ => {}
            }
        }
        clauses.join("; ")
    }

    /// What each of them says, in the menu and under the chips, before the
    /// person acts; `None` when there are none.
    fn sentence(&self, agent: &str, database: &str) -> Option<String> {
        let names = listed(&self.names());
        let together = match self.0.len() {
            0 => return None,
            1 => "",
            2 => " Unticking either unticks both.",
            _ => " Unticking one unticks them all.",
        };
        Some(format!(
            "Untick {names} to change the other drives: {}.{together}",
            self.why(agent, database)
        ))
    }

    /// Why any other change is refused here.
    fn untick_first(&self, agent: &str, database: &str) -> String {
        format!(
            "Untick {} first: {}.",
            listed(&self.names()),
            self.why(agent, database)
        )
    }
}

/// "Ben reads alpha", "Ben and Cy read alpha".
fn readers(outside: &[String], database: &str) -> String {
    let verb = if outside.len() == 1 { "reads" } else { "read" };
    format!("{} {verb} {database}", listed(outside))
}

/// The audience rule for `drive` in `source`'s database, from what this
/// device holds.
fn audience(
    source: &Source,
    drive: &str,
    audiences: &Audiences,
    directory: &HashMap<String, String>,
) -> Verdict {
    if drive == source.database {
        return Verdict::Allowed;
    }
    let (Some(here), Some(there)) = (audiences.of(&source.database), audiences.of(drive)) else {
        return Verdict::Unknown;
    };
    let outside: Vec<String> = here
        .iter()
        .filter(|person| !there.contains(*person))
        .map(|person| {
            directory
                .get(person)
                .cloned()
                .unwrap_or_else(|| person.clone())
        })
        .collect();
    if outside.is_empty() {
        Verdict::Allowed
    } else {
        Verdict::Audience(outside)
    }
}

/// Whether everyone who reads `source`'s database reads `drive` too, as
/// this device can establish it (D-34's audience rule, the same judgement a
/// tick gets): the drive is the database itself, or both directories are
/// held and the database's people are among the drive's. Unknown readers
/// are not "everyone", so they answer `false` (R605-01).
pub(crate) fn read_by_every_reader(source: &Source, drive: &str, sources: &[Source]) -> bool {
    audience(source, drive, &Audiences::read(sources), &HashMap::new()) == Verdict::Allowed
}

/// What the server's refusal of a selection says about `drive`, a drive of
/// it keeper knows fails that way, or about the selection when `drive` is
/// `None`.
fn refusal_sentence(
    rejection: &Rejection,
    drive: Option<&str>,
    database: &str,
    agent: &str,
) -> String {
    match (rejection.code.as_str(), drive) {
        (AUDIENCE, Some(drive)) => {
            format!("The server refused {drive}: not everyone who reads {database} may read it.")
        }
        (AUDIENCE, None) => format!(
            "The server refused this change: not everyone who reads {database} may read one of \
             its drives."
        ),
        (NOT_OFFERED, Some(drive)) => {
            format!("The server refused {drive}: {agent} does not offer it.")
        }
        (NOT_OFFERED, None) => {
            format!("The server refused this change: {agent} does not offer one of its drives.")
        }
        ("hq.v17.person_member", _) => "The server refused this change: only a person in this \
             chat may choose its drives."
            .to_owned(),
        (CONFLICT, _) => "The server refused this change: another device changed this chat's \
             drives first."
            .to_owned(),
        _ => format!("The server refused this change: {rejection}."),
    }
}

const AUDIENCE: &str = "hq.v17.audience";
const NOT_OFFERED: &str = "hq.v17.not_offered";

/// The drives a refusal is about. V17 checks every drive of the proposed
/// selection and names none, so a refusal is hung on the drives of it
/// keeper knows fail that way — for the audience rule, failing those, the
/// ones whose readers this device cannot know — never on the drive the
/// person happened to click. Empty: said for the selection alone.
fn culprits<'a>(code: &str, proposed: &[String], judged: &'a [Judged]) -> Vec<&'a str> {
    let of = |want: fn(&Verdict) -> bool| -> Vec<&'a str> {
        judged
            .iter()
            .filter(|drive| proposed.contains(&drive.name) && want(&drive.verdict))
            .map(|drive| drive.name.as_str())
            .collect()
    };
    match code {
        NOT_OFFERED => of(|v| *v == Verdict::NotOffered),
        AUDIENCE => {
            let known = of(|v| matches!(v, Verdict::Audience(_)));
            if known.is_empty() {
                of(|v| matches!(v, Verdict::Unknown | Verdict::Refused))
            } else {
                known
            }
        }
        _ => Vec::new(),
    }
}

/// A selection's newest standing refusal: the rejection, and the drives the
/// refused write named (`None` when the journal kept no row of it).
type Refused = (Rejection, Option<Vec<String>>);

/// Whether a selection written here waits for the server, and its newest
/// standing refusal.
fn delivery(source: &Source, session_id: &str) -> (bool, Option<Refused>) {
    let writes = match source.replica.row_writes(TABLE, session_id, None) {
        Ok(writes) => writes,
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "the drives' delivery could not be read");
            return (false, None);
        }
    };
    let pending = !writes.pending.is_empty();
    let Some(newest) = writes.refused.into_iter().next() else {
        return (pending, None);
    };
    let proposed = source
        .replica
        .refused_commits(TABLE)
        .ok()
        .and_then(|commits| {
            commits
                .into_iter()
                .find(|commit| commit.commit_id == newest.commit_id)
        })
        .and_then(|commit| {
            commit.rows.into_iter().find_map(|(table, values)| {
                (table == TABLE
                    && values.get("session_id").and_then(Value::as_str) == Some(session_id))
                .then(|| names(&values, "drives"))
            })
        });
    (pending, Some((newest.rejection, proposed)))
}

/// What [`judge`] answers: every drive keeper knows of in the session,
/// judged; the agent's name; whether a selection written here waits; and the
/// newest standing refusal of one.
struct Judgement {
    judged: Vec<Judged>,
    agent: String,
    pending: bool,
    refusal: Option<Refused>,
}

/// Every drive keeper knows of in the session, judged, and the agent's
/// name; `None` where this person has no control (see [`view`]). A ticked
/// drive keeper cannot judge that the standing refusal names is
/// [`Verdict::Refused`]: without that, it and a drive keeper knows fails
/// would each hold the other's untick back (R10923-08).
fn judge(
    source: &Source,
    session_id: &str,
    session: &SessionRow,
    sources: &[Source],
) -> Option<Judgement> {
    if !person_member(source, session_id, session) {
        return None;
    }
    let offered = offered(source, &session.agent_actor)?;
    let audiences = Audiences::read(sources);
    let directory = sessions::directory(source);
    let agent = directory
        .get(&session.agent_actor)
        .cloned()
        .unwrap_or_else(|| session.agent_actor.clone());
    let (ticked, _) = ticked(source, session_id);
    let mut known: BTreeSet<String> = named_here(source);
    known.extend(audiences.databases().cloned());
    known.extend(offered.iter().cloned());
    known.extend(ticked.iter().cloned());
    let mut judged: Vec<Judged> = known
        .into_iter()
        .map(|name| Judged {
            verdict: if offered.contains(&name) {
                audience(source, &name, &audiences, &directory)
            } else {
                Verdict::NotOffered
            },
            ticked: ticked.contains(&name),
            name,
        })
        .collect();
    let (pending, refusal) = delivery(source, session_id);
    if let Some((rejection, Some(proposed))) = &refusal {
        let named: Vec<String> = culprits(&rejection.code, proposed, &judged)
            .into_iter()
            .map(str::to_owned)
            .collect();
        for drive in &mut judged {
            if drive.ticked && drive.verdict == Verdict::Unknown && named.contains(&drive.name) {
                drive.verdict = Verdict::Refused;
            }
        }
    }
    Some(Judgement {
        judged,
        agent,
        pending,
        refusal,
    })
}

/// The session's drives for this person, or `None` when there is no
/// control for them: not a person member, an archived session, or an agent
/// that has not said what it reads. `sources` are every database this
/// device holds, for who reads each drive.
pub fn view(
    source: &Source,
    session_id: &str,
    session: &SessionRow,
    sources: &[Source],
) -> Option<AgentSessionDrivesVm> {
    let Judgement {
        judged,
        agent,
        pending,
        refusal,
    } = judge(source, session_id, session, sources)?;
    let database = source.database.as_str();
    let stale = Stale::of(&judged);
    let held_back = stale.sentence(&agent, database);
    let mut drives: Vec<AgentSessionDriveVm> = judged
        .iter()
        .map(|drive| AgentSessionDriveVm {
            name: drive.name.clone(),
            ticked: drive.ticked,
            reason: if stale.contains(&drive.name) {
                held_back.clone()
            } else {
                drive.reason(&agent, database)
            },
            refused: None,
        })
        .collect();
    let mut refused = None;
    if let Some((rejection, proposed)) = refusal {
        let culprits = culprits(
            &rejection.code,
            proposed.as_deref().unwrap_or_default(),
            &judged,
        );
        let mut said: Vec<String> = Vec::new();
        for drive in &mut drives {
            if culprits.contains(&drive.name.as_str()) {
                let sentence = refusal_sentence(&rejection, Some(&drive.name), database, &agent);
                said.push(sentence.clone());
                drive.refused = Some(sentence);
            }
        }
        refused = Some(if said.is_empty() {
            refusal_sentence(&rejection, None, database, &agent)
        } else {
            said.join(" ")
        });
    }
    // Ticked drives that hold the others back first, then the ones that
    // may be ticked, then the ones that may not; each by name.
    let rank = |d: &AgentSessionDriveVm| match (d.ticked, d.reason.is_some()) {
        (true, true) => 0,
        (_, false) => 1,
        (false, true) => 2,
    };
    drives.sort_by(|a, b| (rank(a), &a.name).cmp(&(rank(b), &b.name)));
    Some(AgentSessionDrivesVm {
        drives,
        warning: WARNING.to_owned(),
        pending,
        refused,
        unseen: Vec::new(),
    })
}

/// Why a tick was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct DriveRefused(pub String);

/// Tick `drive` for the session's agent to read, or untick it. Written on
/// the version read, so two devices ticking at once conflict rather than
/// overwrite; unticking the last drive writes `[]`, never a delete. Ticked
/// drives keeper knows fail ([`Stale`]) go together: unticking any of them
/// unticks all of them, and every other change is refused here, naming
/// them. A selection written sets aside the refusals of the ones before it.
///
/// Returns this operation's chosen tick id (also for an already ticked
/// drive), or `None` for an untick or a legacy tick with no identity. A
/// caller recording this choice as seen must use this id, not reread a
/// selection another device may have replaced after the write.
pub fn set(
    source: &Source,
    session_id: &str,
    drive: &str,
    tick: bool,
    sources: &[Source],
    now_ms: i64,
) -> Result<Option<String>, DriveRefused> {
    let session = timeline::session_row(source, session_id)
        .ok_or_else(|| DriveRefused("This conversation is not on this device.".to_owned()))?;
    let Judgement { judged, agent, .. } =
        judge(source, session_id, &session, sources).ok_or_else(|| {
            DriveRefused("Only a person in this chat may choose the drives it reads.".to_owned())
        })?;
    let database = source.database.as_str();
    let (mut drives, version) = ticked(source, session_id);
    if drives.iter().any(|ticked| ticked == drive) == tick {
        return Ok(if tick {
            tick_ids(source, session_id).remove(drive)
        } else {
            None
        });
    }
    let stale = Stale::of(&judged);
    if !tick && stale.contains(drive) {
        // The way out: every drive that stopped being allowed goes in this
        // one write, so what is written is a selection V17 takes.
        drives.retain(|ticked| !stale.contains(ticked));
    } else if !stale.0.is_empty() {
        // Anything else would be refused for them: say so here, naming them.
        return Err(DriveRefused(stale.untick_first(&agent, database)));
    } else if tick {
        let Some(target) = judged.iter().find(|row| row.name == drive) else {
            return Err(DriveRefused(format!(
                "{drive} is not a drive this chat's agent offers."
            )));
        };
        if let Some(reason) = target.reason(&agent, database) {
            return Err(DriveRefused(reason));
        }
        drives.push(drive.to_owned());
    } else {
        drives.retain(|ticked| ticked != drive);
    }
    // The newest selection decides: every earlier one of this row — refused,
    // or still waiting and so not refused yet — is set aside by this write,
    // in the same step of the replica's owner (as a rename is, 109.16).
    let earlier: Vec<String> = source
        .replica
        .row_writes(TABLE, session_id, None)
        .map(|writes| {
            writes
                .refused
                .into_iter()
                .map(|r| r.commit_id)
                .chain(writes.pending)
                .collect()
        })
        .unwrap_or_default();
    let mut ticks = next_ticks(&tick_ids(source, session_id), &drives);
    let row = RowOp::Upsert {
        table: TABLE.to_owned(),
        values: authored(source, selection(session_id, &drives, &ticks, now_ms)),
        base_version: version.filter(|v| *v > 0),
    };
    source
        .replica
        .mutate_superseding(vec![row], earlier)
        .map_err(|error| DriveRefused(error.to_string()))?;
    let chosen = if tick { ticks.remove(drive) } else { None };
    source.replica.sync_now();
    Ok(chosen)
}

/// `/clean`'s carry-over (Q109-17 b: a fresh session with "the same agent,
/// database and drives"; A109-06): the drives ticked in `from` that this
/// person may still tick there — the menu's own judgement, so a drive the
/// agent no longer offers, the audience rule now refuses, or the server
/// refused is left behind — written as `to`'s selection, in the old row's
/// order. Nothing ticked carries nothing: no row is written, and the new
/// session has no drive, as one made any other way.
///
/// The selection is its own commit, after the session's: V17 would take it
/// in the session's commit (the server reads that commit's own rows, as it
/// does for a first message), but it judges the selection against offers
/// and readers this device may hold stale, and a refusal rolls back only
/// the commit it is in — never the conversation. The refusal then shows on
/// the new session's drive control, as the pick's settings row does
/// (`sessions::create_with_first`, R10918-07).
pub fn carry(
    source: &Source,
    from: &str,
    session: &SessionRow,
    to: &str,
    sources: &[Source],
    now_ms: i64,
) -> Result<(), ReplicaError> {
    let Some(Judgement { judged, .. }) = judge(source, from, session, sources) else {
        return Ok(());
    };
    let (ticked, _) = ticked(source, from);
    let carried: Vec<String> = ticked
        .into_iter()
        .filter(|name| {
            judged
                .iter()
                .any(|drive| &drive.name == name && !drive.fails())
        })
        .collect();
    if carried.is_empty() {
        return Ok(());
    }
    source.replica.mutate(vec![RowOp::Upsert {
        table: TABLE.to_owned(),
        values: authored(
            source,
            selection(
                to,
                &carried,
                &next_ticks(&BTreeMap::new(), &carried),
                now_ms,
            ),
        ),
        base_version: None,
    }])?;
    source.replica.sync_now();
    Ok(())
}

/// Said when the note's folder declares no drive.
pub const NO_DRIVE: &str = "This note's folder is not a drive, so no drive is ticked.";

/// The one lock the person's tick or untick, the Notes Assistant's tick,
/// and the seen record beside them go through (R605-09): a tick and its
/// record, or an untick and its record's removal, are one step to each
/// other. (`/clean`'s carry and replication write ticks without it; neither
/// records anything as seen.)
static DRIVE_WRITES: Mutex<()> = Mutex::new(());

/// The row's values: the drives as a JSON array, `[]` for none,
/// with each ticked drive's tick id, stamped with this write's `updated_at_ms`.
fn selection(
    session_id: &str,
    drives: &[String],
    ticks: &BTreeMap<String, String>,
    now_ms: i64,
) -> Map<String, Value> {
    object(serde_json::json!({
        "session_id": session_id,
        "drives": Value::from(drives.to_vec()).to_string(),
        "updated_at_ms": now_ms,
        "tick_ids": serde_json::json!({"at": now_ms, "drives": ticks}).to_string(),
    }))
}

/// The ticks this person has been shown [`WARNING`] for on this device and
/// answered (R-HQ-79; round 6 and round 7 rulings): for each (server, person,
/// database, session, drive), the id of the tick that was acknowledged. A
/// ticked drive warns unless this record holds **its current tick id**
/// ([`tick_ids`]), wherever its session is shown
/// ([`AgentSessionDrivesVm::unseen`]). An entry for another id is stale and
/// acknowledges nothing, so an untick whose removal was lost, or an untick
/// and re-tick made elsewhere and never seen here, warns again (R605-14). A
/// missing, unreadable or older record is no record; a tick with no id warns.
pub(crate) struct SeenTicks {
    path: std::path::PathBuf,
}

/// One entry's key: server, person, database, session, drive (R605-15).
type SeenKey = (String, String, String, String, String);

#[derive(Serialize, Deserialize)]
struct SeenEntry {
    server: String,
    person: String,
    database: String,
    session: String,
    drive: String,
    tick: String,
}

impl SeenTicks {
    pub(crate) fn at(root: &std::path::Path) -> SeenTicks {
        SeenTicks {
            path: root.join("drive-ticks-seen.json"),
        }
    }

    /// The record; anything that does not read as one is no record at all.
    fn read(&self) -> BTreeMap<SeenKey, String> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!(%error, "the drive ticks seen could not be read; every tick warns");
                }
                return BTreeMap::new();
            }
        };
        match serde_json::from_str::<Vec<SeenEntry>>(&raw) {
            Ok(entries) => entries
                .into_iter()
                .map(|e| ((e.server, e.person, e.database, e.session, e.drive), e.tick))
                .collect(),
            Err(error) => {
                tracing::warn!(%error, "the drive ticks seen do not read; every tick warns");
                BTreeMap::new()
            }
        }
    }

    /// Written whole to a new file and renamed over the old one, so a crash
    /// never leaves half a record.
    fn write(&self, seen: &BTreeMap<SeenKey, String>) -> std::io::Result<()> {
        let entries: Vec<SeenEntry> = seen
            .iter()
            .map(
                |((server, person, database, session, drive), tick)| SeenEntry {
                    server: server.clone(),
                    person: person.clone(),
                    database: database.clone(),
                    session: session.clone(),
                    drive: drive.clone(),
                    tick: tick.clone(),
                },
            )
            .collect();
        let raw = serde_json::to_string(&entries).map_err(std::io::Error::other)?;
        let fresh = self.path.with_extension("json.new");
        std::fs::write(&fresh, raw)?;
        std::fs::rename(&fresh, &self.path)
    }

    fn change(&self, change: impl FnOnce(&mut BTreeMap<SeenKey, String>)) -> std::io::Result<()> {
        let mut seen = self.read();
        let before = seen.clone();
        change(&mut seen);
        if seen == before {
            return Ok(());
        }
        self.write(&seen)
    }
}

impl Hq {
    fn seen_ticks(&self) -> Result<SeenTicks, HqError> {
        Ok(SeenTicks::at(&crate::hq::databases::root(
            &self.data_dir()?,
        )))
    }

    /// The server the held replicas belong to: the paired one, or the one
    /// last synced with while none is (its replicas stay held). Part of
    /// every seen key, so two servers' sessions never share an answer.
    fn seen_server(&self) -> String {
        let paired = lock(&self.inner).server.clone();
        paired
            .or_else(|| {
                self.data_dir()
                    .ok()
                    .and_then(|dir| crate::hq::databases::remembered_server(&dir))
            })
            .map(|server| server.as_str().to_owned())
            .unwrap_or_default()
    }

    fn seen_key(&self, source: &Source, session: &str, drive: &str) -> SeenKey {
        (
            self.seen_server(),
            source.actor.clone(),
            source.database.clone(),
            session.to_owned(),
            drive.to_owned(),
        )
    }

    /// Tick or untick one drive for a session's agent to read: the person's
    /// own choice in the drive menu, where [`WARNING`] stands beside every
    /// drive, so the tick it makes — its new tick id — is recorded as seen in
    /// the same step ([`DRIVE_WRITES`]). An untick needs no record: its
    /// entry names a tick that is gone, and acknowledges nothing again.
    pub fn set_session_drive(
        &self,
        key: &AgentSessionRef,
        drive: &str,
        tick: bool,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let seen = self.seen_ticks()?;
        let _writing = DRIVE_WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let chosen = set(
            &source,
            &key.session_id,
            drive,
            tick,
            &self.sources(),
            sessions::now_ms(),
        )
        .map_err(|refused| HqError::Sentence(refused.0))?;
        if let Some(id) = chosen {
            let entry = self.seen_key(&source, &key.session_id, drive);
            if let Err(error) = seen.change(|seen| {
                seen.insert(entry, id);
            }) {
                // Unrecorded, the person's own tick is only said once more.
                tracing::warn!(%error, "a drive tick seen could not be recorded");
            }
        }
        self.touch();
        Ok(())
    }

    /// The Notes Assistant opened on a note on `drive` (109.28, R-HQ-86;
    /// `None`: its folder declares no drive). The default agent's newest
    /// unarchived session in the drive's database, or one started there with
    /// this device's last `pick` for the agent, and the drive ticked in it as
    /// the person when it is not yet, under a fresh tick id no one has seen,
    /// so wherever the session is shown its warning stands beside the chip
    /// until a person says OK or unticks it. A drive the agent does not
    /// offer, or V17 refuses, stays unticked and the answer says why. `None`
    /// with no default agent.
    ///
    /// Nothing here is ever taken back: an answer the dock no longer wants
    /// leaves its tick as a tick of that session, warning and all.
    pub fn session_for_note(
        &self,
        drive: Option<&str>,
        pick: Option<&crate::hq::settings::AgentSettingsPick>,
    ) -> Result<Option<AgentNoteSessionVm>, HqError> {
        let data_dir = self.data_dir()?;
        let chosen = crate::registry::get_agents_default(&data_dir)?;
        let last = sessions::last_choices(&data_dir);
        let sources = self.sources();
        let now = sessions::now_ms();
        let answer = |target, drive: Option<&str>, reason: Option<String>| AgentNoteSessionVm {
            target,
            drive: drive.map(str::to_owned),
            reason,
        };
        let Some(drive) = drive else {
            return Ok(
                sessions::default_session(&sources, chosen.as_deref(), &last, now)
                    .map(|target| answer(target, None, Some(NO_DRIVE.to_owned()))),
            );
        };
        // Held from finding the drive's session to ticking it: two docks
        // opened on one drive at once start one session, not two, and the
        // person's own tick or untick runs before or after, never between.
        let _writing = DRIVE_WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let Some((mut target, elsewhere)) =
            sessions::note_session(&sources, chosen.as_deref(), &last, drive, now)
        else {
            return Ok(None);
        };
        if let Some(reason) = elsewhere {
            return Ok(Some(answer(target, Some(drive), Some(reason))));
        }
        let source = self.source(drive)?;
        let session_id = match &target.session_id {
            Some(id) => id.clone(),
            None => {
                let id = sessions::create(&source, &target.agent_actor, "", pick, None)
                    .map_err(|error| HqError::Sentence(error.to_string()))?;
                target.session_id = Some(id.clone());
                id
            }
        };
        let (drives, _) = ticked(&source, &session_id);
        if drives.iter().any(|ticked| ticked == drive) {
            return Ok(Some(answer(target, Some(drive), None)));
        }
        let ticked = set(&source, &session_id, drive, true, &sources, now);
        self.touch();
        Ok(Some(match ticked {
            Ok(_) => answer(target, Some(drive), None),
            Err(DriveRefused(reason)) => answer(target, Some(drive), Some(reason)),
        }))
    }

    /// A person was shown [`WARNING`] for the tick `tick` of `drive` in `key`
    /// and said OK (R-HQ-79): that tick is recorded as seen on this device,
    /// and every open view is told (R605-13). A warning shown for a tick that
    /// has since been replaced acknowledges nothing: the new one still warns.
    /// A tick with no id cannot be bound to the displayed warning: refuse
    /// it with the remedy of unticking and selecting again in this app.
    pub fn note_tick_seen(
        &self,
        key: &AgentSessionRef,
        drive: &str,
        tick: Option<&str>,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let seen = self.seen_ticks()?;
        let _writing = DRIVE_WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let (drives, _) = ticked(&source, &key.session_id);
        let current = tick_ids(&source, &key.session_id);
        let acknowledged = match (tick, current.get(drive)) {
            _ if !drives.iter().any(|ticked| ticked == drive) => None,
            (Some(shown), Some(now)) if shown == now => Some(now.clone()),
            (None, _) => {
                return Err(HqError::Sentence(format!(
                    "keeper cannot identify this tick of {drive}. Untick it, then select it again using this app."
                )));
            }
            _ => None,
        };
        if let Some(id) = acknowledged {
            let entry = self.seen_key(&source, &key.session_id, drive);
            seen.change(|seen| {
                seen.insert(entry, id);
            })
            .map_err(|error| HqError::Sentence(format!("keeper could not note that: {error}")))?;
        }
        self.touch();
        Ok(())
    }

    /// The ticked drives of `key` (`ticked_now`) this person has not been
    /// shown the warning for on this device, each with its tick id: every one
    /// whose current tick id the seen record does not hold. Each read also
    /// prunes the record (R605-12): an entry of this server and person, for a
    /// database this device holds, goes once its session is gone, its drive
    /// is no longer ticked, or it names another tick than the drive's
    /// current one. Entries of another server or person, or of a database
    /// not held, are left alone (R605-16). Pruning only ever makes a warning
    /// show again.
    pub(crate) fn unseen_ticks(
        &self,
        key: &AgentSessionRef,
        ticked_now: &[String],
    ) -> Vec<AgentUnseenTickVm> {
        let Ok(source) = self.source(&key.database) else {
            return Vec::new();
        };
        let current = tick_ids(&source, &key.session_id);
        let unseen = |held: &BTreeMap<SeenKey, String>| -> Vec<AgentUnseenTickVm> {
            ticked_now
                .iter()
                .filter(|drive| {
                    let tick = current.get(*drive);
                    tick.is_none()
                        || held.get(&self.seen_key(&source, &key.session_id, drive)) != tick
                })
                .map(|drive| AgentUnseenTickVm {
                    drive: drive.clone(),
                    tick: current.get(drive).cloned(),
                })
                .collect()
        };
        let Ok(seen) = self.seen_ticks() else {
            return unseen(&BTreeMap::new());
        };
        let (server, sources) = (self.seen_server(), self.sources());
        let _writing = DRIVE_WRITES.lock().unwrap_or_else(PoisonError::into_inner);
        let mut record = seen.read();
        // Decided on the record as read, by tick id alone: pruning below only
        // tidies, and acknowledges nothing whether or not it runs.
        let answer = unseen(&record);
        let before = record.len();
        record.retain(|(entry_server, person, database, session, drive), tick| {
            if *entry_server != server {
                return true;
            }
            let Some(held) = sources
                .iter()
                .find(|held| &held.database == database && &held.actor == person)
            else {
                return true;
            };
            timeline::session_row(held, session).is_some()
                && tick_ids(held, session).get(drive) == Some(tick)
        });
        if record.len() != before {
            if let Err(error) = seen.write(&record) {
                tracing::debug!(%error, "the drive ticks seen could not be pruned");
            }
        }
        answer
    }
}

/// A ticked drive this person has not been shown the warning for on this
/// device, and the id of its tick (`None`: a tick written without one, by a
/// schema-4 client), which OK names so that it acknowledges this tick only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentUnseenTickVm {
    pub drive: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tick: Option<String>,
}

#[cfg(test)]
#[path = "drives_tests.rs"]
mod tests;
