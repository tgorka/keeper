//! Many people in one session (FR-851, FR-852; AD-421, AD-433): who is in
//! it, read from the session's `session_members` rows and the directory of
//! its own database (`actors` on `dir:<database>`), and the owner's changes
//! to it.
//!
//! The owner adds a person by naming them in the session's `invite`, a row
//! write that waits in the outbox like any other; the server's reaction then
//! writes the member row and re-scopes their devices. Only people of the
//! session's database are offered, and the server refuses any other (V11).
//! Whether a person reads the thinking and tool results (D-34's reader flag)
//! and removing someone are the server's registered commands, run by the
//! owner while the database syncs; leaving is the member deleting their own
//! row. What the windows hold follows the rows ([`crate::hq::sessions::plan`]):
//! a withdrawn flag or a removal evicts what that member may no longer read.
//!
//! A message's author is never chosen here: it is the `author_actor` the
//! server stamped on the row ([`crate::hq::timeline`]).

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::hq::port::{Delivery, ReplicaError, RowOp};
use crate::hq::sessions::{
    authored, mark, names, object, opt_text, read, text, truthy, AgentSessionRef, Source,
};
use crate::hq::{lock, Hq, HqError};

/// A member's part in a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentMemberRoleVm {
    /// Opened the session: adds, removes and grants.
    Owner,
    Person,
    Agent,
}

/// One member of a session, or a person added who has not arrived yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMemberVm {
    pub actor_id: String,
    pub name: String,
    /// The letter drawn where there is no picture.
    pub mark: String,
    pub role: AgentMemberRoleVm,
    /// Reads the session's thinking and tool results.
    pub reads_trace: bool,
    /// This person.
    pub me: bool,
    /// Added, and their membership not on this device yet.
    pub joining: bool,
    /// The owner may grant or withdraw this member's reader flag.
    pub can_set_reader: bool,
    /// The owner may remove this member.
    pub can_remove: bool,
}

/// A person of the session's database the owner may add.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentDirectoryPersonVm {
    pub actor_id: String,
    pub name: String,
    pub mark: String,
}

/// The People sheet of one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionMembersVm {
    pub database: String,
    pub session_id: String,
    /// This person owns the session.
    pub owner: bool,
    /// The owner first, then the people by name, then the agents.
    pub members: Vec<AgentMemberVm>,
    /// The people of this session's database who are not in it yet; empty
    /// unless this person is the owner.
    pub addable: Vec<AgentDirectoryPersonVm>,
    /// This person may leave: a member who is not the owner (an owner who
    /// left would leave nobody able to change who is in it).
    pub can_leave: bool,
    /// The server's refusal of an addition written here, as a sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub notice: Option<String>,
}

/// A person added to a session: its database, the session and the person.
type Addition = (String, String, String);

/// Additions written here whose outcome is still owed: the commit to look up.
static ADDING: LazyLock<Mutex<HashMap<Addition, String>>> = LazyLock::new(Mutex::default);

/// Refusals of additions, kept until the person reads them once.
static REFUSED: LazyLock<Mutex<HashMap<(String, String), String>>> = LazyLock::new(Mutex::default);

struct Row {
    actor_id: String,
    role: AgentMemberRoleVm,
    reads_trace: bool,
}

fn role(raw: &str) -> AgentMemberRoleVm {
    match raw {
        "owner" => AgentMemberRoleVm::Owner,
        "agent" => AgentMemberRoleVm::Agent,
        _ => AgentMemberRoleVm::Person,
    }
}

fn members_of(source: &Source, session_id: &str) -> Vec<Row> {
    read(
        source,
        "SELECT actor_id, role, reads_trace FROM session_members WHERE session_id = ?1",
        &[Value::from(session_id)],
    )
    .iter()
    .filter_map(|row| {
        Some(Row {
            actor_id: opt_text(row, "actor_id")?,
            role: role(text(row, "role")),
            reads_trace: truthy(row, "reads_trace"),
        })
    })
    .collect()
}

/// The people of `source`'s database directory, by id: (name, is a person).
fn directory(source: &Source) -> HashMap<String, (String, bool)> {
    read(
        source,
        "SELECT id, kind, display_name FROM actors WHERE dir = ?1",
        &[Value::from(source.database.as_str())],
    )
    .into_iter()
    .filter_map(|row| {
        let id = opt_text(&row, "id")?;
        let name = opt_text(&row, "display_name").unwrap_or_else(|| id.clone());
        Some((id, (name, text(&row, "kind") == "person")))
    })
    .collect()
}

fn invite_of(source: &Source, session_id: &str) -> Option<Vec<String>> {
    read(
        source,
        "SELECT invite FROM sessions WHERE id = ?1",
        &[Value::from(session_id)],
    )
    .first()
    .map(|row| names(row, "invite"))
}

fn not_here() -> String {
    "This conversation is not on this device.".to_owned()
}

/// The People sheet of `session_id` as `source`'s rows hold it.
pub fn view(source: &Source, session_id: &str) -> Result<AgentSessionMembersVm, String> {
    let invite = invite_of(source, session_id).ok_or_else(not_here)?;
    let rows = members_of(source, session_id);
    let mine = rows.iter().find(|row| row.actor_id == source.actor);
    let Some(mine) = mine else {
        return Err(not_here());
    };
    let owner = mine.role == AgentMemberRoleVm::Owner;
    let names = directory(source);
    let name_of = |actor: &str| {
        names
            .get(actor)
            .map_or_else(|| actor.to_owned(), |(name, _)| name.clone())
    };
    let present: HashSet<&str> = rows.iter().map(|row| row.actor_id.as_str()).collect();
    let mut members: Vec<AgentMemberVm> = rows
        .iter()
        .map(|row| {
            let me = row.actor_id == source.actor;
            let person = row.role == AgentMemberRoleVm::Person;
            let name = name_of(&row.actor_id);
            AgentMemberVm {
                mark: mark(&name),
                name,
                actor_id: row.actor_id.clone(),
                role: row.role,
                reads_trace: row.reads_trace,
                me,
                joining: false,
                can_set_reader: owner && person && !me,
                can_remove: owner && person && !me,
            }
        })
        .collect();
    // Named in the invite and not a member yet: on its way, or refused.
    for actor in invite.iter().filter(|a| !present.contains(a.as_str())) {
        let Some((name, true)) = names.get(actor) else {
            continue;
        };
        members.push(AgentMemberVm {
            actor_id: actor.clone(),
            mark: mark(name),
            name: name.clone(),
            role: AgentMemberRoleVm::Person,
            reads_trace: false,
            me: false,
            joining: true,
            can_set_reader: false,
            can_remove: false,
        });
    }
    let rank = |m: &AgentMemberVm| match m.role {
        AgentMemberRoleVm::Owner => 0,
        AgentMemberRoleVm::Person => 1,
        AgentMemberRoleVm::Agent => 2,
    };
    members.sort_by(|a, b| {
        (rank(a), a.name.to_lowercase(), &a.actor_id).cmp(&(
            rank(b),
            b.name.to_lowercase(),
            &b.actor_id,
        ))
    });
    let named: HashSet<&str> = members.iter().map(|m| m.actor_id.as_str()).collect();
    let mut addable: Vec<AgentDirectoryPersonVm> = if owner {
        names
            .iter()
            .filter(|(id, (_, person))| *person && !named.contains(id.as_str()))
            .map(|(id, (name, _))| AgentDirectoryPersonVm {
                actor_id: id.clone(),
                name: name.clone(),
                mark: mark(name),
            })
            .collect()
    } else {
        Vec::new()
    };
    addable.sort_by(|a, b| {
        (a.name.to_lowercase(), &a.actor_id).cmp(&(b.name.to_lowercase(), &b.actor_id))
    });
    let notice = settle(source, session_id);
    Ok(AgentSessionMembersVm {
        database: source.database.clone(),
        session_id: session_id.to_owned(),
        owner,
        members,
        addable,
        can_leave: !owner,
        notice,
    })
}

/// Look up the additions written here for `session_id`: a refused one
/// becomes the sheet's notice, a settled one is forgotten.
fn settle(source: &Source, session_id: &str) -> Option<String> {
    let mut adding = lock(&ADDING);
    let mut refused = lock(&REFUSED);
    adding.retain(|(database, session, actor), commit| {
        if database != &source.database || session != session_id {
            return true;
        }
        match source.replica.delivery(commit) {
            Ok(Delivery::Pending) => true,
            Ok(Delivery::Refused(reason)) => {
                refused.insert(
                    (database.clone(), session.clone()),
                    refusal(actor, &source.database, &reason),
                );
                false
            }
            Ok(Delivery::Applied | Delivery::Unknown) | Err(_) => false,
        }
    });
    refused
        .get(&(source.database.clone(), session_id.to_owned()))
        .cloned()
}

fn refusal(actor: &str, database: &str, reason: &str) -> String {
    if reason.contains("v11") {
        format!("The server refused adding {actor}: they are not in {database}'s directory.")
    } else if reason.contains("owner_only") {
        format!("The server refused adding {actor}: only the session's owner adds people.")
    } else {
        format!("The server refused adding {actor}: {reason}")
    }
}

/// Name `actor` in the session's invite: the owner's write, offline if need
/// be. Only a person of this session's own database may be added.
pub fn add(source: &Source, session_id: &str, actor: &str) -> Result<(), String> {
    let vm = view(source, session_id)?;
    if !vm.owner {
        return Err("Only the session's owner adds people.".to_owned());
    }
    if vm.members.iter().any(|m| m.actor_id == actor) {
        return Err("They are already in this session.".to_owned());
    }
    if !vm.addable.iter().any(|p| p.actor_id == actor) {
        return Err(format!(
            "{actor} is not a person of {}'s directory; a session adds only people of its own database.",
            source.database
        ));
    }
    let mut invite = invite_of(source, session_id).ok_or_else(not_here)?;
    invite.push(actor.to_owned());
    let commit = source
        .replica
        .mutate(vec![RowOp::Patch {
            table: "sessions".to_owned(),
            values: authored(
                source,
                object(serde_json::json!({
                    "id": session_id,
                    "invite": Value::from(invite).to_string(),
                })),
            ),
            base_version: None,
        }])
        .map_err(|error| error.to_string())?;
    lock(&REFUSED).remove(&(source.database.clone(), session_id.to_owned()));
    lock(&ADDING).insert(
        (
            source.database.clone(),
            session_id.to_owned(),
            actor.to_owned(),
        ),
        commit,
    );
    source.replica.sync_now();
    Ok(())
}

/// The member `actor` the owner may change, or why not.
fn changeable(source: &Source, session_id: &str, actor: &str) -> Result<AgentMemberVm, String> {
    let vm = view(source, session_id)?;
    if !vm.owner {
        return Err("Only the session's owner changes who is in it.".to_owned());
    }
    vm.members
        .into_iter()
        .find(|m| m.actor_id == actor && !m.joining)
        .ok_or_else(|| "They are not a member of this session.".to_owned())
}

fn command(source: &Source, operation: &str, session_id: &str, actor: &str) -> Result<(), String> {
    source
        .replica
        .remote_command(
            operation,
            &ulid::Ulid::new().to_string(),
            serde_json::json!({ "sessionId": session_id, "actorId": actor }),
        )
        .map_err(|error| command_sentence(&source.database, &error))?;
    source.replica.sync_now();
    Ok(())
}

fn command_sentence(database: &str, error: &ReplicaError) -> String {
    match error {
        ReplicaError::NotSyncing | ReplicaError::Closed => format!(
            "This needs the server, and {database} is not syncing on this device now. Try again when it is."
        ),
        ReplicaError::Client(message) if message.contains("not_owner") => {
            "The server refused: only the session's owner changes who is in it.".to_owned()
        }
        ReplicaError::Client(message) if message.contains("not_member") => {
            "The server refused: they are not a member of this session.".to_owned()
        }
        ReplicaError::Client(message) if message.contains("operation.forbidden") => {
            "The server refused this change to the session's people.".to_owned()
        }
        ReplicaError::Client(message) => format!("The server could not make this change: {message}"),
    }
}

/// Grant or withdraw a person member's reader flag: the owner's
/// declassification (D-34), the server's `grant_trace` / `revoke_trace`.
pub fn set_reader(
    source: &Source,
    session_id: &str,
    actor: &str,
    reads: bool,
) -> Result<(), String> {
    let member = changeable(source, session_id, actor)?;
    if !member.can_set_reader {
        return Err("Only another person's reading can be changed.".to_owned());
    }
    if member.reads_trace == reads {
        return Ok(());
    }
    let operation = if reads { "grant_trace" } else { "revoke_trace" };
    command(source, operation, session_id, actor)
}

/// Remove a person member: the server's `remove_member`, which drops them
/// from the invite and deletes their membership, so their devices stop
/// receiving the session and evict it.
pub fn remove(source: &Source, session_id: &str, actor: &str) -> Result<(), String> {
    let member = changeable(source, session_id, actor)?;
    if !member.can_remove {
        return Err("Only another person can be removed.".to_owned());
    }
    command(source, "remove_member", session_id, actor)
}

/// Leave a session: delete this person's own membership. The windows
/// follow at once and evict its rows (rows still in the outbox stay until
/// sent); the server drops them from the invite.
pub fn leave(source: &Source, session_id: &str) -> Result<(), String> {
    let vm = view(source, session_id)?;
    if !vm.can_leave {
        return Err(
            "The owner cannot leave: nobody else could change who is in this session.".to_owned(),
        );
    }
    source
        .replica
        .mutate(vec![RowOp::Delete {
            table: "session_members".to_owned(),
            row_id: format!("{session_id}/{}", source.actor),
            base_version: None,
        }])
        .map_err(|error| error.to_string())?;
    source.replica.sync_now();
    Ok(())
}

impl Hq {
    /// The People sheet of a session, from its database's replica.
    pub fn session_members(&self, key: &AgentSessionRef) -> Result<AgentSessionMembersVm, HqError> {
        let source = self.source(&key.database)?;
        view(&source, &key.session_id).map_err(HqError::Sentence)
    }

    /// Add a person of the session's database. The owner's.
    pub fn add_member(&self, key: &AgentSessionRef, actor: &str) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        add(&source, &key.session_id, actor).map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }

    /// Grant or withdraw a member's reader flag. Blocking: it asks the
    /// server.
    pub fn set_member_reader(
        &self,
        key: &AgentSessionRef,
        actor: &str,
        reads: bool,
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        set_reader(&source, &key.session_id, actor, reads).map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }

    /// Remove a member. Blocking: it asks the server.
    pub fn remove_member(&self, key: &AgentSessionRef, actor: &str) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        remove(&source, &key.session_id, actor).map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }

    /// Leave a session this person does not own.
    pub fn leave_session(&self, key: &AgentSessionRef) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        leave(&source, &key.session_id).map_err(HqError::Sentence)?;
        self.touch();
        Ok(())
    }
}

#[cfg(test)]
#[path = "members_tests.rs"]
mod tests;
