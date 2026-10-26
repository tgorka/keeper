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
/// before it stopped being allowed: it may be unticked, never ticked again.
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

/// Why `drive` may not be ticked in `source`'s database, by the audience
/// rule and what this device knows; `None` when it may, or when keeper
/// cannot know who reads the drive (the server decides then).
fn audience_reason(
    source: &Source,
    drive: &str,
    audiences: &Audiences,
    directory: &HashMap<String, String>,
) -> Option<String> {
    if drive == source.database {
        return None;
    }
    let here = audiences.of(&source.database)?;
    let there = audiences.of(drive)?;
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
        return None;
    }
    let verb = if outside.len() == 1 { "reads" } else { "read" };
    Some(format!(
        "{} {verb} {} but cannot read {drive}, so it cannot be ticked here.",
        listed(&outside),
        source.database
    ))
}

/// What the server's refusal of a selection says about `drive`, one it
/// would have changed, or about the selection when `drive` is `None`.
fn refusal_sentence(
    rejection: &Rejection,
    drive: Option<&str>,
    database: &str,
    agent: &str,
) -> String {
    match (rejection.code.as_str(), drive) {
        ("hq.v17.audience", Some(drive)) => {
            format!("The server refused {drive}: not everyone who reads {database} may read it.")
        }
        ("hq.v17.not_offered", Some(drive)) => {
            format!("The server refused {drive}: {agent} does not offer it.")
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

    let mut drives: Vec<AgentSessionDriveVm> = known
        .into_iter()
        .map(|name| {
            let reason = if offered.contains(&name) {
                audience_reason(source, &name, &audiences, &directory)
            } else {
                Some(format!("{agent} does not offer {name}."))
            };
            AgentSessionDriveVm {
                ticked: ticked.contains(&name),
                name,
                reason,
                refused: None,
            }
        })
        .collect();
    let (pending, refusal) = delivery(source, session_id);
    let mut refused = None;
    if let Some((rejection, proposed)) = refusal {
        let mut said: Vec<String> = Vec::new();
        for drive in &mut drives {
            // The drives the refused write would have ticked or unticked.
            if proposed
                .as_ref()
                .is_some_and(|proposed| proposed.contains(&drive.name) != drive.ticked)
            {
                let sentence =
                    refusal_sentence(&rejection, Some(&drive.name), &source.database, &agent);
                if !said.contains(&sentence) {
                    said.push(sentence.clone());
                }
                drive.refused = Some(sentence);
            }
        }
        refused = Some(if said.is_empty() {
            refusal_sentence(&rejection, None, &source.database, &agent)
        } else {
            said.join(" ")
        });
    }
    drives.sort_by(|a, b| (a.reason.is_some(), &a.name).cmp(&(b.reason.is_some(), &b.name)));
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
/// overwrite; unticking the last drive writes `[]`, never a delete. A
/// selection written sets aside the refusals of the ones before it.
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
    let vm = view(source, session_id, &session, sources).ok_or_else(|| {
        DriveRefused("Only a person in this chat may choose the drives it reads.".to_owned())
    })?;
    let (mut drives, version) = ticked(source, session_id);
    if drives.iter().any(|ticked| ticked == drive) == tick {
        return Ok(());
    }
    if tick {
        match vm.drives.iter().find(|row| row.name == drive) {
            Some(AgentSessionDriveVm { reason: None, .. }) => {}
            Some(AgentSessionDriveVm {
                reason: Some(reason),
                ..
            }) => return Err(DriveRefused(reason.clone())),
            None => {
                return Err(DriveRefused(format!(
                    "{drive} is not a drive this chat's agent offers."
                )))
            }
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
