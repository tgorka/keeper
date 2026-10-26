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

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::port::{Rejection, RowOp, CONFLICT};
use crate::hq::sessions::{
    self, authored, int, json, names, object, opt_text, read, text, AgentSessionRef, Source,
};
use crate::hq::timeline::{self, SessionRow};
use crate::hq::{Hq, HqError};

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

/// This person may change the session's drives: a person member (`owner`
/// or `person`) of a session that is not archived (V17).
fn person_member(source: &Source, session_id: &str, session: &SessionRow) -> bool {
    !session.archived
        && read(
            source,
            "SELECT role FROM session_members WHERE session_id = ?1 AND actor_id = ?2",
            &[Value::from(session_id), Value::from(source.actor.as_str())],
        )
        .first()
        .is_some_and(|row| matches!(text(row, "role"), "owner" | "person"))
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
        matches!(self.verdict, Verdict::NotOffered | Verdict::Audience(_))
    }

    /// Why it cannot be ticked; `None` when it may be, and for a ticked
    /// drive, whose reason is the selection's ([`Stale`]).
    fn reason(&self, agent: &str, database: &str) -> Option<String> {
        let name = &self.name;
        match &self.verdict {
            _ if self.ticked => None,
            Verdict::Allowed | Verdict::Unknown => None,
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
            if let Verdict::Audience(outside) = &drive.verdict {
                let it = if alone { "it" } else { drive.name.as_str() };
                clauses.push(format!(
                    "{} but cannot read {it}",
                    readers(outside, database)
                ));
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
                of(|v| *v == Verdict::Unknown)
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
    let writes = match source.replica.row_writes(TABLE, session_id) {
        Ok(writes) => writes,
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "the drives' delivery could not be read");
            return (false, None);
        }
    };
    let Some(newest) = writes.refused.into_iter().next() else {
        return (writes.pending, None);
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
    (writes.pending, Some((newest.rejection, proposed)))
}

/// Every drive keeper knows of in the session, judged, and the agent's
/// name; `None` where this person has no control (see [`view`]).
fn judge(
    source: &Source,
    session_id: &str,
    session: &SessionRow,
    sources: &[Source],
) -> Option<(Vec<Judged>, String)> {
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
    let judged = known
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
    Some((judged, agent))
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
    let (judged, agent) = judge(source, session_id, session, sources)?;
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
    let (pending, refusal) = delivery(source, session_id);
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
pub fn set(
    source: &Source,
    session_id: &str,
    drive: &str,
    tick: bool,
    sources: &[Source],
    now_ms: i64,
) -> Result<(), DriveRefused> {
    let session = timeline::session_row(source, session_id)
        .ok_or_else(|| DriveRefused("This conversation is not on this device.".to_owned()))?;
    let (judged, agent) = judge(source, session_id, &session, sources).ok_or_else(|| {
        DriveRefused("Only a person in this chat may choose the drives it reads.".to_owned())
    })?;
    let database = source.database.as_str();
    let (mut drives, version) = ticked(source, session_id);
    if drives.iter().any(|ticked| ticked == drive) == tick {
        return Ok(());
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
    let standing: Vec<String> = source
        .replica
        .row_writes(TABLE, session_id)
        .map(|writes| writes.refused.into_iter().map(|r| r.commit_id).collect())
        .unwrap_or_default();
    let row = RowOp::Upsert {
        table: TABLE.to_owned(),
        values: authored(source, selection(session_id, &drives, now_ms)),
        base_version: version.filter(|v| *v > 0),
    };
    let commit = source
        .replica
        .mutate(vec![row])
        .map_err(|error| DriveRefused(error.to_string()))?;
    if !standing.is_empty() {
        if let Err(error) = source.replica.supersede(standing, &commit) {
            tracing::debug!(database = %source.database, %error, "earlier drive refusals were not set aside");
        }
    }
    source.replica.sync_now();
    Ok(())
}

/// The row's values: the drives as a JSON array, `[]` for none.
fn selection(session_id: &str, drives: &[String], now_ms: i64) -> Map<String, Value> {
    object(serde_json::json!({
        "session_id": session_id,
        "drives": Value::from(drives.to_vec()).to_string(),
        "updated_at_ms": now_ms,
    }))
}

impl Hq {
    /// Tick or untick one drive for a session's agent to read.
    pub fn set_session_drive(
        &self,
        key: &AgentSessionRef,
        drive: &str,
        tick: bool,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        set(
            &source,
            &key.session_id,
            drive,
            tick,
            &self.sources(),
            sessions::now_ms(),
        )
        .map_err(|refused| HqError::Sentence(refused.0))?;
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
#[path = "drives_tests.rs"]
mod tests;
