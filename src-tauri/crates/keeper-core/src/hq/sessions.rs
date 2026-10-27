//! The Agents window's list (AD-424, AD-433; FR-847, FR-871): every session
//! this person is a member of, in every database the device holds, merged
//! into one list read from the replicas alone — title, agent, archived, last
//! activity, the database it lives in, and the attention state Rust decides
//! from rows. Reading it never waits on the network.
//!
//! The windows (SPEC §4.8) are decided here too: every membership's session
//! row is always local; an active session keeps its newest month and the
//! current one of `messages` (and, for a reader, of `approvals` and
//! `tool_calls`) and its streamed chunks; an opened session widens month by
//! month as the person scrolls back; archiving evicts these windowed rows
//! (the client keeps rows the outbox still holds). Session settings remain
//! subscribed for every member session, including archived sessions.
//!
//! A session is named by `(database, session id)` everywhere: two sessions
//! with the same id in the two databases are two rows.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::port::{
    RefusedCommit, Rejection, Replica, ReplicaError, RowOp, WindowBase, CONFLICT,
};

/// A session's state in the Agents list; a session in none of them carries
/// none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentAttentionVm {
    /// An approval or a question waits for this person.
    NeedsYou,
    /// keeper could not read all it needs to say nothing waits for this
    /// person.
    Unknown,
    /// The session is blocked.
    Failed,
    /// The session is running.
    Working,
    /// The session finished after the person last read it.
    DoneUnread,
}

/// One database this device holds, and who this device is in it.
#[derive(Clone)]
pub struct Source {
    pub database: String,
    pub replica: Arc<dyn Replica>,
    /// The person's actor id.
    pub actor: String,
    /// This device's client id.
    pub client: String,
}

/// A session, named as it must be wherever the two databases meet.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionRef {
    pub database: String,
    pub session_id: String,
}

/// What became of the renames written on this device, read from the
/// replica's outbox and outcome journal — the same per-row delivery the
/// instance choice reads ([`crate::hq::port::Replica::row_writes`]) — never
/// inferred from the rename call returning: that only says the outbox took
/// it. The session's own creation is not a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentSessionTitleWriteVm {
    /// No rename written here stands refused, and none waits on its own:
    /// while the chat's creation still waits, a rename queued behind it is
    /// owed with the chat and reads as confirmed.
    Confirmed,
    /// A rename written here waits in the outbox for the server.
    Pending,
    /// The server refused the newest rename written here: the title it
    /// proposed, for a retry, and why, as the person reads it.
    Refused { title: String, sentence: String },
    /// The replica could not say.
    Unknown,
}

/// One row of the Agents window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionRowVm {
    /// The database the session lives in: the row's label.
    pub database: String,
    pub session_id: String,
    pub title: String,
    pub title_write: AgentSessionTitleWriteVm,
    pub display_name: String,
    pub handle: String,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    pub trust: String,
    pub instances: Vec<String>,
    pub agent_actor: String,
    /// The agent's name from the database's directory; its id until the
    /// directory arrives.
    pub agent_name: String,
    /// The letter drawn where the agent has no picture.
    pub agent_mark: String,
    pub archived: bool,
    #[ts(type = "number")]
    pub last_activity_ms: i64,
    /// Needs you, failed, working or done since the person last read it;
    /// absent for a session in none of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attention: Option<AgentAttentionVm>,
    /// This person has the session's drive control (109.23,
    /// [`crate::hq::drives::may_choose`] and an agent that says what it
    /// reads): the row's menu offers Drives….
    pub chooses_drives: bool,
    /// This person may change the session's model, and its agent offers
    /// more than one ([`crate::hq::settings::agents_choosing_model`]): the
    /// row's menu offers Model… (A109-05), which opens the composer's picker.
    pub chooses_model: bool,
}

impl AgentSessionRowVm {
    fn key(&self) -> (&str, &str) {
        (&self.database, &self.session_id)
    }
}

/// One index-based change of the list, applied in order to a plain array
/// the frontend never re-sorts — the reducer the inbox's ops use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum AgentSessionsOp {
    /// Replace everything with `rows`.
    Reset { rows: Vec<AgentSessionRowVm> },
    /// Insert `row` at `index`.
    Insert {
        #[ts(type = "number")]
        index: u32,
        row: AgentSessionRowVm,
    },
    /// Replace the row at `index`.
    Set {
        #[ts(type = "number")]
        index: u32,
        row: AgentSessionRowVm,
    },
    /// Remove the row at `index`.
    Remove {
        #[ts(type = "number")]
        index: u32,
    },
}

/// A batch of list changes over the subscription's channel. The first batch
/// of a stream is a reset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionsBatch {
    pub ops: Vec<AgentSessionsOp>,
    #[ts(type = "number")]
    pub total: u32,
}

/// An agent a new session can be opened with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentChoiceVm {
    pub agent_actor: String,
    pub agent_name: String,
    pub agent_mark: String,
    /// The databases this device holds whose directory names the agent.
    pub databases: Vec<String>,
    /// The database a new session goes to unless the person picks another:
    /// the only one, or the person's last choice for this agent.
    pub database: Option<String>,
    /// Whether the composer asks which database (more than one is shared).
    pub ask: bool,
}

/// What launch and the docks open: the default agent's main conversation,
/// or its new-session composer when it has none yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentDefaultSessionVm {
    pub agent_actor: String,
    pub agent_name: String,
    /// The database of `session_id`, or where a new one would go.
    pub database: Option<String>,
    /// The newest active session with the agent; `None` opens the composer.
    pub session_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Reading rows
// ---------------------------------------------------------------------------

pub(crate) fn text<'a>(row: &'a Map<String, Value>, column: &str) -> &'a str {
    row.get(column).and_then(Value::as_str).unwrap_or("")
}

pub(crate) fn opt_text(row: &Map<String, Value>, column: &str) -> Option<String> {
    row.get(column)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

pub(crate) fn int(row: &Map<String, Value>, column: &str) -> Option<i64> {
    match row.get(column)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// A boolean column, however the replica hands it back.
pub(crate) fn truthy(row: &Map<String, Value>, column: &str) -> bool {
    match row.get(column) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().is_some_and(|n| n != 0),
        Some(Value::String(s)) => s == "true" || s == "1",
        _ => false,
    }
}

/// A JSON column: written as JSON text, read back as text or as JSON.
pub(crate) fn json(row: &Map<String, Value>, column: &str) -> Value {
    match row.get(column) {
        Some(Value::String(raw)) => serde_json::from_str(raw).unwrap_or(Value::Null),
        Some(other) => other.clone(),
        None => Value::Null,
    }
}

/// Every actor id a JSON array column names.
pub(crate) fn names(row: &Map<String, Value>, column: &str) -> Vec<String> {
    match json(row, column) {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

pub(crate) fn object(values: Value) -> Map<String, Value> {
    match values {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

/// `query`, logged and empty when the replica cannot answer: one database
/// that cannot be read never blanks the other's rows.
pub(crate) fn read(source: &Source, sql: &str, params: &[Value]) -> Vec<Map<String, Value>> {
    match source.replica.query(sql, params) {
        Ok(rows) => rows,
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "an agents replica could not be read");
            Vec::new()
        }
    }
}

/// The agents' and people's names in a database's directory.
pub(crate) fn directory(source: &Source) -> HashMap<String, String> {
    read(source, "SELECT id, display_name FROM actors", &[])
        .into_iter()
        .filter_map(|row| {
            let id = opt_text(&row, "id")?;
            let name = opt_text(&row, "display_name").unwrap_or_else(|| id.clone());
            Some((id, name))
        })
        .collect()
}

/// The letter an actor is drawn by where it has no picture.
pub(crate) fn mark(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default()
}

pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

// ---------------------------------------------------------------------------
// Months and page keys
// ---------------------------------------------------------------------------

/// The UTC month of `ms`, `yyyy-mm`.
pub fn month(ms: i64) -> String {
    DateTime::<Utc>::from_timestamp_millis(ms)
        .unwrap_or_default()
        .format("%Y-%m")
        .to_string()
}

/// The month before `month`, `yyyy-mm`.
pub fn previous_month(month: &str) -> Option<String> {
    let (year, number) = month.split_once('-')?;
    let (year, number): (i32, u32) = (year.parse().ok()?, number.parse().ok()?);
    let (year, number) = if number <= 1 {
        (year - 1, 12)
    } else {
        (year, number - 1)
    };
    Some(format!("{year:04}-{number:02}"))
}

/// The page key of a session's rows created in `month` (SPEC §4.8 W2).
pub fn page_key(session_id: &str, month: &str) -> String {
    format!("{session_id}/{month}")
}

/// The months a session's newest page spans: the month of its last
/// activity, and the current month (an answer written since).
pub fn newest_months(created_ms: i64, last_activity_ms: i64, now_ms: i64) -> Vec<String> {
    let newest = month(last_activity_ms.max(created_ms));
    let current = month(now_ms.max(created_ms));
    if newest == current {
        vec![newest]
    } else {
        vec![newest, current]
    }
}

/// The `widen` months before the newest page, never before the session's
/// creation month, newest first.
pub fn earlier_months(created_ms: i64, last_activity_ms: i64, widen: u32) -> Vec<String> {
    let first = month(created_ms);
    let mut at = month(last_activity_ms.max(created_ms));
    let mut out = Vec::new();
    for _ in 0..widen {
        if at <= first {
            break;
        }
        let Some(before) = previous_month(&at) else {
            break;
        };
        out.push(before.clone());
        at = before;
    }
    out
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// The windows a replica holds, each along one scope variable. Every held
/// session's members are held beside its row, so the People sheet reads
/// who else is in it.
pub const WINDOWS: [(&str, &str); 13] = [
    ("sessions", "session"),
    ("session_members", "session"),
    ("turn_chunks", "session"),
    ("claims", "session"),
    ("session_tool_switches", "session"),
    ("session_settings", "session"),
    ("session_drives", "session"),
    ("attachments", "session"),
    ("messages", "page"),
    ("approvals", "trace"),
    ("reasoning", "trace"),
    ("tool_calls", "trace"),
    // One person's grants about one agent ([`crate::hq::grants::units`]).
    ("standing_grants", "person_agent"),
];

pub fn window(table: &str) -> WindowBase {
    let variable = WINDOWS
        .iter()
        .find(|(t, _)| *t == table)
        .map_or("session", |(_, v)| *v);
    WindowBase {
        table: table.to_owned(),
        variable: variable.to_owned(),
        fixed_scopes: Vec::new(),
    }
}

/// A membership as the window plan reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub id: String,
    pub reads_trace: bool,
    /// `None` until the session's row is local.
    pub row: Option<HeldRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeldRow {
    pub archived: bool,
    pub created_ms: i64,
    pub last_activity_ms: i64,
}

/// The person's memberships in one database, with their sessions' rows.
pub fn held(source: &Source) -> Vec<Held> {
    read(
        source,
        "SELECT m.session_id AS id, m.reads_trace AS reads_trace, s.id AS row_id, \
         s.archived AS archived, s.created_at_ms AS created_at_ms, \
         s.last_activity_ms AS last_activity_ms \
         FROM session_members m LEFT JOIN sessions s ON s.id = m.session_id \
         WHERE m.actor_id = ?1 ORDER BY m.session_id",
        &[Value::from(source.actor.as_str())],
    )
    .into_iter()
    .filter_map(|row| {
        let id = opt_text(&row, "id")?;
        let row_held = opt_text(&row, "row_id").map(|_| HeldRow {
            archived: truthy(&row, "archived"),
            created_ms: int(&row, "created_at_ms").unwrap_or(0),
            last_activity_ms: int(&row, "last_activity_ms").unwrap_or(0),
        });
        Some(Held {
            id,
            reads_trace: truthy(&row, "reads_trace"),
            row: row_held,
        })
    })
    .collect()
}

/// Whether this person's membership of `session_id` reads its trace now: no
/// membership (removed, or left) reads nothing. What a view shows of the
/// trace asks this, never only whether the rows are still on the device.
pub fn reads_trace(source: &Source, session_id: &str) -> bool {
    read(
        source,
        "SELECT reads_trace FROM session_members WHERE session_id = ?1 AND actor_id = ?2",
        &[Value::from(session_id), Value::from(source.actor.as_str())],
    )
    .first()
    .is_some_and(|row| truthy(row, "reads_trace"))
}

/// Each window's units for one database: every session row, its members and settings;
/// for each active session its streamed chunks, its claim, its tool switches,
/// its drives and its attachments' rows (never their bytes), and
/// its newest months (widened by `widen` for an opened one); its trace
/// months only where the person reads the trace, and their thinking only
/// while `reasoning` is shown. An archived session keeps its session, member
/// and settings rows, plus its attachments' rows while opened (in `widen`): its messages come
/// through the server's `sessionPage`, and its files' names, sizes and the
/// right to open them come with these rows, which go again when it closes.
pub fn plan(
    held: &[Held],
    widen: &HashMap<String, u32>,
    reasoning: bool,
    now_ms: i64,
) -> Vec<(&'static str, Vec<String>)> {
    let mut sessions = BTreeSet::new();
    let mut turns = BTreeSet::new();
    let mut files = BTreeSet::new();
    let mut pages = BTreeSet::new();
    let mut trace = BTreeSet::new();
    for membership in held {
        sessions.insert(membership.id.clone());
        let Some(row) = membership.row else { continue };
        if row.archived {
            if widen.contains_key(&membership.id) {
                files.insert(membership.id.clone());
            }
            continue;
        }
        turns.insert(membership.id.clone());
        files.insert(membership.id.clone());
        let mut months = newest_months(row.created_ms, row.last_activity_ms, now_ms);
        let extra = widen.get(&membership.id).copied().unwrap_or(0);
        months.extend(earlier_months(row.created_ms, row.last_activity_ms, extra));
        for month in months {
            let key = page_key(&membership.id, &month);
            if membership.reads_trace {
                trace.insert(key.clone());
            }
            pages.insert(key);
        }
    }
    let trace: Vec<String> = trace.into_iter().collect();
    let turns: Vec<String> = turns.into_iter().collect();
    let sessions: Vec<String> = sessions.into_iter().collect();
    vec![
        ("sessions", sessions.clone()),
        ("session_settings", sessions.clone()),
        ("session_members", sessions),
        ("turn_chunks", turns.clone()),
        ("claims", turns.clone()),
        ("session_tool_switches", turns.clone()),
        ("session_drives", turns),
        ("attachments", files.into_iter().collect()),
        ("messages", pages.into_iter().collect()),
        ("approvals", trace.clone()),
        (
            "reasoning",
            if reasoning { trace.clone() } else { Vec::new() },
        ),
        ("tool_calls", trace),
    ]
}

// ---------------------------------------------------------------------------
// The list
// ---------------------------------------------------------------------------

/// Display-only identities, read and hashed once per database projection.
pub struct SessionNames {
    members: HashMap<String, Vec<String>>,
    handles: HashMap<String, String>,
}

impl SessionNames {
    pub fn read(source: &Source) -> Self {
        let mut members: HashMap<String, Vec<String>> = HashMap::new();
        for row in read(source,
            "SELECT m.session_id, m.actor_id FROM session_members m JOIN actors a ON a.id = m.actor_id \
             WHERE a.kind = 'agent' ORDER BY m.actor_id", &[]) {
            members.entry(text(&row, "session_id").to_owned()).or_default()
                .push(text(&row, "actor_id").to_owned());
        }
        let mut handles = HashMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for row in read(source, "SELECT id FROM sessions", &[]) {
            let id = text(&row, "id");
            let handle = super::vocabulary::handle(id).unwrap_or_else(|| id.to_owned());
            *counts.entry(handle.clone()).or_default() += 1;
            handles.insert(id.to_owned(), handle);
        }
        for (id, handle) in &mut handles {
            if counts.get(handle).copied().unwrap_or(0) > 1 {
                *handle = format!("{handle}-{}", id.rsplit('.').next().unwrap_or(id));
            }
        }
        Self { members, handles }
    }

    /// Session `id`'s handle as the list shows it: with its session number
    /// when another session here shares the words. A session this replica
    /// does not hold has no neighbours to tell apart from: its bare handle.
    pub fn handle(&self, id: &str) -> String {
        self.handles
            .get(id)
            .cloned()
            .or_else(|| super::vocabulary::handle(id))
            .unwrap_or_else(|| id.to_owned())
    }

    pub fn presentation(&self, id: &str, agent: &str, title: &str) -> (String, String) {
        let mut agents = vec![agent];
        if let Some(members) = self.members.get(id) {
            agents.extend(
                members
                    .iter()
                    .map(String::as_str)
                    .filter(|member| *member != agent),
            );
        }
        let handle = self
            .handles
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned());
        let suffix = if title.trim().is_empty() {
            &handle
        } else {
            title
        };
        (format!("{} · {suffix}", agents.join(",")), handle)
    }
}

/// The one column only a session's creation writes: a write carrying it is
/// the session arriving, not a rename.
const CREATED: &str = "created_at_ms";

/// A refused rename, by the server's code, matched exactly.
fn rename_refusal(rejection: &Rejection) -> String {
    match rejection.code.as_str() {
        "hq.v3.not_member" => {
            "The server refused this rename: you are no longer a member of this chat.".to_owned()
        }
        CONFLICT => {
            "The server refused this rename: another device changed this chat first.".to_owned()
        }
        _ => format!("The server refused this rename: {rejection}."),
    }
}

/// What became of the renames of session `id` written here.
fn title_write(source: &Source, id: &str, refused: &[RefusedCommit]) -> AgentSessionTitleWriteVm {
    let read = |field| {
        source
            .replica
            .row_writes("sessions", id, Some(field))
            .map_err(|error| {
                tracing::debug!(database = %source.database, %error, "the rename's delivery could not be read");
            })
    };
    let (Ok(titles), Ok(creation)) = (read("title"), read(CREATED)) else {
        return AgentSessionTitleWriteVm::Unknown;
    };
    // Newest first: the first refused rename is the one to show.
    for refusal in &titles.refused {
        let Some(values) = refused
            .iter()
            .find(|commit| commit.commit_id == refusal.commit_id)
            .and_then(|commit| {
                commit.rows.iter().find_map(|(table, values)| {
                    (table == "sessions" && values.get("id").and_then(Value::as_str) == Some(id))
                        .then_some(values)
                })
            })
        else {
            // Refused between the two reads, or journalled without its row.
            return AgentSessionTitleWriteVm::Unknown;
        };
        if values.contains_key(CREATED) {
            continue;
        }
        let Some(title) = values.get("title").and_then(Value::as_str) else {
            return AgentSessionTitleWriteVm::Unknown;
        };
        return AgentSessionTitleWriteVm::Refused {
            title: title.to_owned(),
            sentence: rename_refusal(&refusal.rejection),
        };
    }
    // While the creation itself waits, every title write waiting is the
    // creation's or rides behind it: the chat, not a rename, is owed.
    if !titles.pending.is_empty() && creation.pending.is_empty() {
        AgentSessionTitleWriteVm::Pending
    } else {
        AgentSessionTitleWriteVm::Confirmed
    }
}

/// One database's rows of the list.
pub fn rows(source: &Source, now_ms: i64) -> Vec<AgentSessionRowVm> {
    let names = directory(source);
    let mut instances: HashMap<String, Vec<String>> = HashMap::new();
    let identities = SessionNames::read(source);
    let refused_titles = source.replica.refused_commits("sessions");
    let sessions = read(
        source,
        "SELECT s.id AS id, s.title AS title, s.status AS status, s.agent_actor AS agent_actor, \
         s.archived AS archived, s.last_activity_ms AS last_activity_ms, s.created_at_ms AS created_at_ms, s.trust AS trust, \
         m.read_until AS read_until, m.role AS role \
         FROM session_members m JOIN sessions s ON s.id = m.session_id \
         WHERE m.actor_id = ?1",
        &[Value::from(source.actor.as_str())],
    );
    if sessions.is_empty() {
        return Vec::new();
    }
    let needs = needs_you(source, now_ms);
    let saying = super::drives::agents_saying(source);
    let choosing = super::settings::agents_choosing_model(source);
    let working: HashSet<String> = read(source, "SELECT DISTINCT session_id FROM turn_chunks", &[])
        .iter()
        .filter_map(|row| opt_text(row, "session_id"))
        .collect();
    // Each session's newest agent message: a turn that ended in an error is
    // a failure, not an answer (the runtime writes `metadata.error`).
    let newest_answer: HashMap<String, (i64, bool)> = read(
        source,
        "SELECT m.session_id AS session_id, m.created_at_ms AS at, m.metadata AS metadata \
         FROM messages m WHERE m.role = 'agent' AND NOT EXISTS (SELECT 1 FROM messages later \
         WHERE later.session_id = m.session_id AND later.role = 'agent' \
         AND (later.created_at_ms > m.created_at_ms \
         OR (later.created_at_ms = m.created_at_ms AND later.id > m.id)))",
        &[],
    )
    .iter()
    .filter_map(|row| {
        let failed = json(row, "metadata")
            .get("error")
            .is_some_and(|error| !error.is_null());
        Some((opt_text(row, "session_id")?, (int(row, "at")?, failed)))
    })
    .collect();
    sessions
        .iter()
        .filter_map(|row| {
            let id = opt_text(row, "id")?;
            let agent = text(row, "agent_actor").to_owned();
            let agent_name = names.get(&agent).cloned().unwrap_or_else(|| agent.clone());
            let answer = newest_answer.get(&id);
            let attention = if needs.contains(&id) {
                Some(AgentAttentionVm::NeedsYou)
            } else if working.contains(&id) {
                // A turn under way supersedes how the last one ended.
                Some(AgentAttentionVm::Working)
            } else if text(row, "status") == "failed" || answer.is_some_and(|(_, failed)| *failed) {
                Some(AgentAttentionVm::Failed)
            } else if answer
                .is_some_and(|(at, _)| int(row, "read_until").is_none_or(|read| *at > read))
            {
                Some(AgentAttentionVm::DoneUnread)
            } else {
                None
            };
            let (display_name, handle) = identities.presentation(&id, &agent, text(row, "title"));
            let instance_labels = instances
                .entry(agent.clone())
                .or_insert_with(|| {
                    super::instances::instances_of(source, &agent)
                        .into_iter()
                        .map(|instance| instance.label)
                        .collect()
                })
                .clone();
            // The drive control and the model picker are a person member's
            // of a live session alike.
            let may_choose = super::drives::may_choose(text(row, "role"), truthy(row, "archived"));
            Some(AgentSessionRowVm {
                database: source.database.clone(),
                title_write: match &refused_titles {
                    Ok(refused) => title_write(source, &id, refused),
                    Err(_) => AgentSessionTitleWriteVm::Unknown,
                },
                session_id: id,
                title: text(row, "title").to_owned(),
                display_name,
                handle,
                created_at_ms: int(row, "created_at_ms").unwrap_or(0),
                trust: opt_text(row, "trust").unwrap_or_else(|| "Not reported".to_owned()),
                instances: instance_labels,
                agent_mark: mark(&agent_name),
                agent_name,
                chooses_drives: may_choose && saying.contains(&agent),
                chooses_model: may_choose && choosing.contains(&agent),
                agent_actor: agent,
                archived: truthy(row, "archived"),
                last_activity_ms: int(row, "last_activity_ms").unwrap_or(0),
                attention,
            })
        })
        .collect()
}

/// The sessions where something waits for this person: an approval naming
/// them that is still pending, or an agent's ask no later message of the
/// person answered — the timeline's rule: an agent message after the ask
/// does not close it.
fn needs_you(source: &Source, now_ms: i64) -> HashSet<String> {
    let mut waiting: HashSet<String> = read(
        source,
        "SELECT session_id, approvers, expires_at_ms FROM approvals WHERE state = 'pending'",
        &[],
    )
    .iter()
    .filter(|row| int(row, "expires_at_ms").is_some_and(|at| at > now_ms))
    .filter(|row| names(row, "approvers").contains(&source.actor))
    .filter_map(|row| opt_text(row, "session_id"))
    .collect();
    let unanswered = read(
        source,
        "SELECT m.session_id AS session_id, m.metadata AS metadata \
         FROM messages m WHERE m.role = 'agent' AND NOT EXISTS (SELECT 1 FROM messages p \
         WHERE p.session_id = m.session_id AND p.role = 'person' \
         AND (p.metadata IS NULL OR json_extract(p.metadata, '$.kind') IS NOT 'heard') \
         AND (p.created_at_ms > m.created_at_ms \
         OR (p.created_at_ms = m.created_at_ms AND p.id > m.id)))",
        &[],
    );
    for row in unanswered {
        if is_ask(&json(&row, "metadata")) {
            if let Some(id) = opt_text(&row, "session_id") {
                waiting.insert(id);
            }
        }
    }
    waiting
}

/// An agent message whose metadata says it asks the person (R-HQ-28).
pub fn is_ask(metadata: &Value) -> bool {
    metadata.get("kind").and_then(Value::as_str) == Some("ask")
}

/// Whether a message's metadata makes it a person's heard row (R-HQ-68,
/// AD-411): where the person stopped hearing a spoken answer, written for
/// the claim holder to copy into `sessions.heard_until`. It is no message:
/// the Agents window draws none, and it answers no ask.
pub fn is_heard(metadata: &Value) -> bool {
    metadata.get("kind").and_then(Value::as_str) == Some(crate::hq::spoken::HEARD_KIND)
}

/// Every database's rows in one list: the most recent activity first, then
/// by database and id, so the order never depends on which replica answered
/// first.
pub fn list(sources: &[Source], now_ms: i64) -> Vec<AgentSessionRowVm> {
    let mut all: Vec<AgentSessionRowVm> = sources.iter().flat_map(|s| rows(s, now_ms)).collect();
    all.sort_by(|a, b| {
        b.last_activity_ms
            .cmp(&a.last_activity_ms)
            .then_with(|| a.database.cmp(&b.database))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    all
}

/// The ops that turn `old` into `new`: one insert or one remove where a
/// single session came or went (a new session lands on top), in-place sets
/// for rows that changed where they stand, a reset for anything else.
pub fn diff(old: &[AgentSessionRowVm], new: &[AgentSessionRowVm]) -> Vec<AgentSessionsOp> {
    let index = |at: usize| u32::try_from(at).unwrap_or(u32::MAX);
    let first_change = old
        .iter()
        .zip(new)
        .position(|(a, b)| a.key() != b.key())
        .unwrap_or(old.len().min(new.len()));
    let mut ops = Vec::new();
    // `shown` is the frontend's array after the structural op.
    let shown: Vec<&AgentSessionRowVm> = if old.len() == new.len() {
        if first_change == old.len() {
            old.iter().collect()
        } else {
            // One session moved up (new activity): out of its old place,
            // into its new one.
            let moved = new[first_change].key();
            let Some(from) = old.iter().position(|row| row.key() == moved) else {
                return vec![AgentSessionsOp::Reset { rows: new.to_vec() }];
            };
            let mut shown: Vec<&AgentSessionRowVm> = old.iter().collect();
            shown.remove(from);
            shown.insert(first_change, &new[first_change]);
            if !shown.iter().zip(new).all(|(a, b)| a.key() == b.key()) {
                return vec![AgentSessionsOp::Reset { rows: new.to_vec() }];
            }
            ops.push(AgentSessionsOp::Remove { index: index(from) });
            ops.push(AgentSessionsOp::Insert {
                index: index(first_change),
                row: new[first_change].clone(),
            });
            shown
        }
    } else if new.len() == old.len() + 1
        && old[first_change..]
            .iter()
            .zip(&new[first_change + 1..])
            .all(|(a, b)| a.key() == b.key())
    {
        ops.push(AgentSessionsOp::Insert {
            index: index(first_change),
            row: new[first_change].clone(),
        });
        let mut shown: Vec<&AgentSessionRowVm> = old.iter().collect();
        shown.insert(first_change, &new[first_change]);
        shown
    } else if old.len() == new.len() + 1
        && old[first_change + 1..]
            .iter()
            .zip(&new[first_change..])
            .all(|(a, b)| a.key() == b.key())
    {
        ops.push(AgentSessionsOp::Remove {
            index: index(first_change),
        });
        let mut shown: Vec<&AgentSessionRowVm> = old.iter().collect();
        shown.remove(first_change);
        shown
    } else {
        return vec![AgentSessionsOp::Reset { rows: new.to_vec() }];
    };
    ops.extend(
        shown
            .iter()
            .zip(new)
            .enumerate()
            .filter(|(_, (a, b))| **a != *b)
            .map(|(at, (_, b))| AgentSessionsOp::Set {
                index: index(at),
                row: b.clone(),
            }),
    );
    ops
}

// ---------------------------------------------------------------------------
// New sessions and the database they go to
// ---------------------------------------------------------------------------

/// The person's last database choice per agent, kept on this device.
fn choices_path(data_dir: &Path) -> std::path::PathBuf {
    crate::hq::databases::root(data_dir).join("choices.json")
}

pub fn last_choices(data_dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(choices_path(data_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn remember_choice(data_dir: &Path, agent: &str, database: &str) {
    let mut choices = last_choices(data_dir);
    if choices.get(agent).map(String::as_str) == Some(database) {
        return;
    }
    choices.insert(agent.to_owned(), database.to_owned());
    let path = choices_path(data_dir);
    let written = serde_json::to_string(&choices)
        .map_err(std::io::Error::other)
        .and_then(|raw| std::fs::write(&path, raw));
    if let Err(error) = written {
        tracing::warn!(%error, "the agent's database choice could not be kept");
    }
}

/// The agents a new session can be opened with: every agent of every held
/// database's directory, each with the databases it shares with this device.
pub fn choices(sources: &[Source], last: &HashMap<String, String>) -> Vec<AgentChoiceVm> {
    let mut agents: Vec<AgentChoiceVm> = Vec::new();
    for source in sources {
        let rows = read(
            source,
            "SELECT id, display_name FROM actors WHERE kind = 'agent' ORDER BY display_name, id",
            &[],
        );
        for row in rows {
            let Some(id) = opt_text(&row, "id") else {
                continue;
            };
            match agents.iter_mut().find(|a| a.agent_actor == id) {
                Some(known) => known.databases.push(source.database.clone()),
                None => {
                    let name = opt_text(&row, "display_name").unwrap_or_else(|| id.clone());
                    agents.push(AgentChoiceVm {
                        agent_mark: mark(&name),
                        agent_name: name,
                        agent_actor: id,
                        databases: vec![source.database.clone()],
                        database: None,
                        ask: false,
                    });
                }
            }
        }
    }
    for agent in &mut agents {
        agent.databases.sort();
        agent.ask = agent.databases.len() > 1;
        agent.database = if agent.ask {
            last.get(&agent.agent_actor)
                .filter(|db| agent.databases.contains(db))
                .cloned()
        } else {
            agent.databases.first().cloned()
        };
    }
    agents.sort_by(|a, b| {
        a.agent_name
            .to_lowercase()
            .cmp(&b.agent_name.to_lowercase())
            .then_with(|| a.agent_actor.cmp(&b.agent_actor))
    });
    agents
}

/// The default agent as `agents.default` names it — a Matrix user id whose
/// localpart is the agent's actor id in keeper-hq — and its main
/// conversation: the newest active session with it in any held database.
pub fn default_session(
    sources: &[Source],
    chosen: Option<&str>,
    last: &HashMap<String, String>,
    now_ms: i64,
) -> Option<AgentDefaultSessionVm> {
    let chosen = chosen?;
    let actor = chosen
        .strip_prefix('@')
        .and_then(|rest| rest.split(':').next())
        .unwrap_or(chosen);
    let choice = choices(sources, last)
        .into_iter()
        .find(|c| c.agent_actor == actor)?;
    let newest = list(sources, now_ms)
        .into_iter()
        .find(|row| row.agent_actor == actor && !row.archived);
    Some(match newest {
        Some(row) => AgentDefaultSessionVm {
            agent_actor: choice.agent_actor,
            agent_name: choice.agent_name,
            database: Some(row.database),
            session_id: Some(row.session_id),
        },
        None => AgentDefaultSessionVm {
            agent_actor: choice.agent_actor,
            agent_name: choice.agent_name,
            database: choice.database,
            session_id: None,
        },
    })
}

/// The title a session takes from its first message: its first line, cut
/// at a word near 80 characters.
pub fn title_from(body: &str) -> String {
    const MAX: usize = 80;
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.chars().count() <= MAX {
        return line.to_owned();
    }
    let cut: String = line.chars().take(MAX).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{}…", cut.trim_end())
}

/// The next session id this device may mint in `source`'s database:
/// `<clientId>.<n>`, one past the highest it has used.
pub fn next_session_id(source: &Source) -> String {
    let prefix = format!("{}.", source.client);
    let highest = read(
        source,
        "SELECT id FROM sessions WHERE substr(id, 1, ?2) = ?1",
        &[
            Value::from(prefix.as_str()),
            Value::from(prefix.chars().count() as i64),
        ],
    )
    .iter()
    .filter_map(|row| text(row, "id").strip_prefix(&prefix)?.parse::<u64>().ok())
    .max()
    .unwrap_or(0);
    format!("{prefix}{}", highest + 1)
}

/// The author columns every row a device writes carries (V1).
pub(crate) fn authored(source: &Source, mut values: Map<String, Value>) -> Map<String, Value> {
    values.insert(
        "author_actor".to_owned(),
        Value::from(source.actor.as_str()),
    );
    values.insert(
        "author_client".to_owned(),
        Value::from(source.client.as_str()),
    );
    values
}

/// A person message row `id`, written now.
pub(crate) fn person_message(
    source: &Source,
    session_id: &str,
    id: &str,
    body: &str,
    at: i64,
) -> RowOp {
    RowOp::Upsert {
        table: "messages".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "id": id,
                "session_id": session_id,
                "page_key": page_key(session_id, &month(at)),
                "turn_id": null,
                "role": "person",
                "body": body,
                "body_blob": null,
                "metadata": null,
                "created_at_ms": at,
            })),
        ),
        base_version: None,
    }
}

/// Held from reading the highest session id to committing the next one, so
/// two creations at once never mint the same id: the committed row is in
/// the replica (optimistically) before the next creation reads.
static CREATING: Mutex<()> = Mutex::new(());

/// Create a session with `agent` in `source`'s database and its first
/// message, offline if need be (the server grants each client its next
/// session ids). `pick` is this device's last choice of model and effort for
/// `agent`: what the agent still offers of it starts the session's settings
/// (Q109-16 b). See [`create_with_first`] for the commits it writes.
pub fn create(
    source: &Source,
    agent: &str,
    body: &str,
    pick: Option<&crate::hq::settings::AgentSettingsPick>,
) -> Result<String, ReplicaError> {
    create_with_first(source, agent, body, &ulid::Ulid::new().to_string(), pick)
}

/// [`create`], its first message's id `first` — what a spoken question's
/// answer names (`crate::hq::spoken`).
pub fn create_with_first(
    source: &Source,
    agent: &str,
    body: &str,
    first: &str,
    pick: Option<&crate::hq::settings::AgentSettingsPick>,
) -> Result<String, ReplicaError> {
    create_in(source, agent, None, &title_from(body), body, first, pick).map(|(id, _)| id)
}

/// A session of `agent` started by this person from `parent` (109.24): the
/// child's row naming its parent, the person's owner membership and `body`
/// as its first message, in one commit — the shape V3 lets a person member
/// of the parent insert. Its id and the commit's.
pub(crate) fn create_child(
    source: &Source,
    parent: &str,
    agent: &str,
    title: &str,
    body: &str,
) -> Result<(String, String), ReplicaError> {
    create_in(
        source,
        agent,
        Some(parent),
        title,
        body,
        &ulid::Ulid::new().to_string(),
        None,
    )
}

/// The one way a session is created here: its row, the owner's membership
/// and, unless `body` is empty, its first message, in one commit — with a
/// model/effort `pick`, in three (R10918-07). The session's id and the
/// commit that created it.
fn create_in(
    source: &Source,
    agent: &str,
    parent: Option<&str>,
    title: &str,
    body: &str,
    first: &str,
    pick: Option<&crate::hq::settings::AgentSettingsPick>,
) -> Result<(String, String), ReplicaError> {
    let _creating = CREATING.lock().unwrap_or_else(PoisonError::into_inner);
    let at = now_ms();
    let id = next_session_id(source);
    let session = RowOp::Upsert {
        table: "sessions".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "id": id,
                "title": title,
                "status": "open",
                "agent_actor": agent,
                "created_by_actor": source.actor,
                "created_by_client": source.client,
                "preferred_client": null,
                "parent_session": parent,
                "invite": "[]",
                "archived": false,
                "heard_until": null,
                "created_at_ms": at,
                "last_activity_ms": at,
            })),
        ),
        base_version: None,
    };
    let member = RowOp::Upsert {
        table: "session_members".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "id": format!("{id}/{}", source.actor),
                "session_id": id,
                "actor_id": source.actor,
                "role": "owner",
                "reads_trace": true,
                "added_by_actor": source.actor,
                "added_at_ms": at,
                "read_until": null,
            })),
        ),
        base_version: None,
    };
    let settings = pick
        .map(|pick| crate::hq::settings::initial(source, agent, &id, pick, at))
        .transpose()?
        .flatten();
    let message = (!body.is_empty()).then(|| person_message(source, &id, first, body, at));
    let commit = match settings {
        // Nothing of a pick to write: one commit, as a creation always was.
        // A mention's child is always this one commit (109.24, V3).
        None => {
            let mut ops = vec![session, member];
            ops.extend(message);
            source.replica.mutate(ops)?
        }
        // Three commits, in this order (R10918-07). The pick's settings row
        // is its own commit: V16 judges it against the server's offers, which
        // this device may not hold yet (written offline, or behind a
        // republish), and a refusal rolls back only the commit it is in —
        // never the conversation or its first message. The refusal then shows
        // on the new session's picker (`settings::write_state`). The server
        // applies a client's commits in order, so the settings commit is
        // applied before the message, and the runtime starts no turn before
        // it holds the session's settings (keeper-hq `worker.rs`,
        // `start_turn`): the first turn uses the pick.
        Some(row) => {
            let created = source.replica.mutate(vec![session, member])?;
            source.replica.mutate(vec![row])?;
            if let Some(message) = message {
                source.replica.mutate(vec![message])?;
            }
            created
        }
    };
    source.replica.sync_now();
    Ok((id, commit))
}

/// Rename changes only the mutable title, preserving session identity. The
/// newest title write decides the row's rename state: every earlier title
/// write of this row — refused, or still waiting and so not refused yet — is
/// set aside by this one, in the same step of the replica's owner that
/// writes it ([`Replica::mutate_superseding`]). The chat's own creation is
/// not a rename and is never set aside by one. The one title writer: the
/// menu and `/rename` both come here.
pub fn rename(source: &Source, session_id: &str, title: &str) -> Result<(), ReplicaError> {
    let titles = source
        .replica
        .row_writes("sessions", session_id, Some("title"))?;
    let creation = source
        .replica
        .row_writes("sessions", session_id, Some(CREATED))?;
    let created: Vec<String> = creation
        .pending
        .into_iter()
        .chain(
            creation
                .refused
                .into_iter()
                .map(|refusal| refusal.commit_id),
        )
        .collect();
    let earlier: Vec<String> = titles
        .refused
        .into_iter()
        .map(|refusal| refusal.commit_id)
        .chain(titles.pending)
        .filter(|commit| !created.contains(commit))
        .collect();
    source.replica.mutate_superseding(
        vec![RowOp::Patch {
            table: "sessions".to_owned(),
            values: authored(
                source,
                object(serde_json::json!({ "id": session_id, "title": title })),
            ),
            base_version: None,
        }],
        earlier,
    )?;
    source.replica.sync_now();
    Ok(())
}

/// Set a session's `archived` flag. The window follows from the row.
pub fn set_archived(source: &Source, session_id: &str, archived: bool) -> Result<(), ReplicaError> {
    source.replica.mutate(vec![RowOp::Patch {
        table: "sessions".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({ "id": session_id, "archived": archived })),
        ),
        base_version: None,
    }])?;
    source.replica.sync_now();
    Ok(())
}

/// Mark the person's read point in a session (`read_until`, V4: a member
/// changes only its own) when a newer message is on screen.
pub fn mark_read(source: &Source, session_id: &str, newest_ms: i64) -> Result<(), ReplicaError> {
    let current = read(
        source,
        "SELECT read_until FROM session_members WHERE id = ?1",
        &[Value::from(format!("{session_id}/{}", source.actor))],
    );
    let Some(row) = current.first() else {
        return Ok(());
    };
    if int(row, "read_until").is_some_and(|read| read >= newest_ms) {
        return Ok(());
    }
    source.replica.mutate(vec![RowOp::Patch {
        table: "session_members".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({
                "id": format!("{session_id}/{}", source.actor),
                "read_until": newest_ms,
            })),
        ),
        base_version: None,
    }])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn months_step_back_across_a_year_and_stop_at_creation() {
        assert_eq!(previous_month("2026-01").as_deref(), Some("2025-12"));
        assert_eq!(previous_month("2026-10").as_deref(), Some("2026-09"));
        // 2026-08-15 created, last active 2026-10-02.
        let created = 1_786_752_000_000;
        let last = 1_790_928_000_000;
        assert_eq!(month(created), "2026-08");
        assert_eq!(month(last), "2026-10");
        assert_eq!(earlier_months(created, last, 5), vec!["2026-09", "2026-08"]);
        assert_eq!(earlier_months(created, last, 1), vec!["2026-09"]);
        assert!(earlier_months(created, created, 3).is_empty());
    }

    #[test]
    fn the_newest_page_spans_last_activity_and_the_current_month() {
        let october = 1_790_928_000_000;
        let december = 1_796_083_200_000;
        assert_eq!(newest_months(october, october, october), vec!["2026-10"]);
        assert_eq!(
            newest_months(october, october, december),
            vec!["2026-10", "2026-12"]
        );
    }

    fn membership(id: &str, archived: bool, reads_trace: bool) -> Held {
        let october = 1_790_928_000_000;
        Held {
            id: id.to_owned(),
            reads_trace,
            row: Some(HeldRow {
                archived,
                created_ms: october,
                last_activity_ms: october,
            }),
        }
    }

    #[test]
    fn an_archived_session_keeps_its_row_and_nothing_else() {
        let held = vec![
            membership("c.1", false, true),
            membership("c.2", true, true),
            membership("c.3", false, false),
            Held {
                id: "c.4".to_owned(),
                reads_trace: true,
                row: None,
            },
        ];
        let october = 1_790_928_000_000;
        let shown: HashMap<_, _> = plan(&held, &HashMap::new(), true, october)
            .into_iter()
            .collect();
        assert_eq!(shown["sessions"], vec!["c.1", "c.2", "c.3", "c.4"]);
        // Who else is in a session is held while the session is, archived too.
        assert_eq!(shown["session_members"], vec!["c.1", "c.2", "c.3", "c.4"]);
        assert_eq!(shown["turn_chunks"], vec!["c.1", "c.3"]);
        assert_eq!(shown["claims"], vec!["c.1", "c.3"]);
        assert_eq!(shown["session_tool_switches"], vec!["c.1", "c.3"]);
        assert_eq!(shown["session_drives"], vec!["c.1", "c.3"]);
        // An active session's attachments' rows (never their bytes), not an
        // archived one's.
        assert_eq!(shown["attachments"], vec!["c.1", "c.3"]);
        assert_eq!(shown["messages"], vec!["c.1/2026-10", "c.3/2026-10"]);
        // Only a reader's trace is asked for: the server grants no other.
        assert_eq!(shown["approvals"], vec!["c.1/2026-10"]);
        assert_eq!(shown["tool_calls"], vec!["c.1/2026-10"]);
        assert_eq!(shown["reasoning"], vec!["c.1/2026-10"]);
        // Thinking nobody shows is not fetched; the rest of the trace is.
        let hidden: HashMap<_, _> = plan(&held, &HashMap::new(), false, october)
            .into_iter()
            .collect();
        assert!(hidden["reasoning"].is_empty());
        assert_eq!(hidden["tool_calls"], vec!["c.1/2026-10"]);

        // Opened, the archived session holds its attachments' rows — what
        // its files are and that they may be opened — and nothing else of
        // it; closed again, they go.
        let opened: HashMap<_, _> = plan(
            &held,
            &HashMap::from([("c.2".to_owned(), 0)]),
            true,
            october,
        )
        .into_iter()
        .collect();
        assert_eq!(opened["attachments"], vec!["c.1", "c.2", "c.3"]);
        assert_eq!(opened["turn_chunks"], vec!["c.1", "c.3"]);
        assert_eq!(opened["messages"], vec!["c.1/2026-10", "c.3/2026-10"]);
        assert_eq!(opened["approvals"], vec!["c.1/2026-10"]);
    }

    #[test]
    fn an_opened_session_widens_back_month_by_month() {
        let created = 1_786_752_000_000; // 2026-08
        let last = 1_790_928_000_000; // 2026-10
        let held = vec![Held {
            id: "c.1".to_owned(),
            reads_trace: false,
            row: Some(HeldRow {
                archived: false,
                created_ms: created,
                last_activity_ms: last,
            }),
        }];
        let widen = HashMap::from([("c.1".to_owned(), 9)]);
        let plan: HashMap<_, _> = plan(&held, &widen, false, last).into_iter().collect();
        assert_eq!(
            plan["messages"],
            vec!["c.1/2026-08", "c.1/2026-09", "c.1/2026-10"]
        );
    }

    fn row(db: &str, id: &str, at: i64) -> AgentSessionRowVm {
        AgentSessionRowVm {
            database: db.to_owned(),
            session_id: id.to_owned(),
            title: id.to_owned(),
            title_write: AgentSessionTitleWriteVm::Confirmed,
            display_name: format!("nixie · {id}"),
            handle: id.to_owned(),
            created_at_ms: at,
            trust: "native".to_owned(),
            instances: Vec::new(),
            agent_actor: "nixie".to_owned(),
            agent_name: "Nixie".to_owned(),
            agent_mark: "N".to_owned(),
            archived: false,
            last_activity_ms: at,
            attention: None,
            chooses_drives: false,
            chooses_model: false,
        }
    }

    /// What the frontend's reducer does with a batch.
    fn apply(mut rows: Vec<AgentSessionRowVm>, ops: &[AgentSessionsOp]) -> Vec<AgentSessionRowVm> {
        for op in ops {
            match op {
                AgentSessionsOp::Reset { rows: all } => rows = all.clone(),
                AgentSessionsOp::Insert { index, row } => rows.insert(*index as usize, row.clone()),
                AgentSessionsOp::Set { index, row } => rows[*index as usize] = row.clone(),
                AgentSessionsOp::Remove { index } => {
                    rows.remove(*index as usize);
                }
            }
        }
        rows
    }

    #[test]
    fn the_ops_turn_the_shown_list_into_the_new_one_with_the_smallest_change() {
        let a = row("alpha", "c.1", 4);
        let b = row("beta", "c.1", 3);
        let c = row("alpha", "c.2", 2);
        let d = row("alpha", "c.3", 1);
        let old = vec![a.clone(), b.clone(), c.clone()];
        let mut working = b.clone();
        working.attention = Some(AgentAttentionVm::Working);
        let mut bumped = c.clone();
        bumped.last_activity_ms = 9;
        let cases: Vec<(&str, Vec<AgentSessionRowVm>, usize)> = vec![
            ("unchanged", old.clone(), 0),
            (
                "one row changed in place",
                vec![a.clone(), working.clone(), c.clone()],
                1,
            ),
            (
                "a new session on top",
                vec![d.clone(), a.clone(), b.clone(), c.clone()],
                1,
            ),
            (
                "one at the end",
                vec![a.clone(), b.clone(), c.clone(), d.clone()],
                1,
            ),
            ("one gone", vec![a.clone(), c.clone()], 1),
            (
                "one moved up and changed",
                vec![bumped.clone(), a.clone(), b.clone()],
                2,
            ),
            (
                "reordered beyond one move",
                vec![c.clone(), b.clone(), a.clone(), d.clone()],
                1,
            ),
        ];
        for (what, new, count) in cases {
            let ops = diff(&old, &new);
            assert_eq!(apply(old.clone(), &ops), new, "{what}: {ops:?}");
            assert_eq!(ops.len(), count, "{what}: {ops:?}");
        }
        assert!(matches!(
            diff(&old, &[c.clone(), b.clone(), a.clone(), d.clone()])[..],
            [AgentSessionsOp::Reset { .. }]
        ));
    }

    #[test]
    fn a_title_is_the_first_line_cut_at_a_word() {
        assert_eq!(title_from("\n  plan the week \nmore"), "plan the week");
        let long = "word ".repeat(30);
        let title = title_from(&long);
        assert!(
            title.ends_with('…') && title.chars().count() <= 81,
            "{title}"
        );
    }
}
