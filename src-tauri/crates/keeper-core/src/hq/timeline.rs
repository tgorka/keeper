//! An opened session (AD-424; FR-848, NFR-127): its messages as the window
//! holds them, newest months first and widened month by month as the person
//! scrolls back, each message with its turn state (R465: *waiting* until the
//! server has it, *sent*, *taken* once the agent's turn names it — never a
//! read receipt) and an agent's ask (R-HQ-28); and its approvals.
//!
//! An archived session holds no rows here. Opening it reads its newest page
//! through the server's registered `sessionPage` query, read-only, page by
//! page and month by month on scroll-back; a row this device still holds in
//! its outbox is shown beside them.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

use crate::hq::approvals::{self, AgentApprovalVm};
use crate::hq::port::ReplicaError;
use crate::hq::sessions::{
    earlier_months, int, is_ask, json, mark, month, newest_months, opt_text, page_key,
    previous_month, read, text, truthy, window, Source,
};

/// The registered `sessionPage` query's operation id, as `syncular/`'s
/// generated `syncular.queries.ts` names it.
pub const SESSION_PAGE: &str =
    "sha256:9e32b5abb163f2d4dbacb19817c20004771f4b2a9016c304706c023c4f70dff8/sessionPage";

/// Messages per remote page: the newest page an archived session opens on.
pub const PAGE_SIZE: usize = 50;

/// The largest number SQLite and JSON agree on: the first page's cursor.
const NEWEST: i64 = 9_007_199_254_740_991;

/// How many empty months a fetch looks back through before it stops for
/// this page.
const MAX_EMPTY_MONTHS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentMessageRoleVm {
    Person,
    Agent,
}

/// A person's message, by its turn (R465).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentMessageStateVm {
    /// Written here; the server does not have it yet.
    Waiting,
    /// The server has it.
    Sent,
    /// The agent's turn names it.
    Taken,
}

/// An agent's question to the person (R-HQ-28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionAskVm {
    /// The answers it offers; empty for a free answer.
    pub options: Vec<String>,
    /// No later message of the person answered it yet.
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentMessageVm {
    pub id: String,
    pub role: AgentMessageRoleVm,
    pub author_actor: String,
    pub author_name: String,
    pub body: String,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    /// This device's messages only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub state: Option<AgentMessageStateVm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub ask: Option<AgentSessionAskVm>,
}

/// What an opened session shows, whole, every time it changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSessionViewVm {
    pub database: String,
    pub session_id: String,
    pub title: String,
    pub agent_actor: String,
    pub agent_name: String,
    pub agent_mark: String,
    pub archived: bool,
    /// Read through the server, not held here: nothing can be written.
    pub read_only: bool,
    /// Oldest first.
    pub messages: Vec<AgentMessageVm>,
    /// Oldest first; empty for a member who does not read the trace.
    pub approvals: Vec<AgentApprovalVm>,
    /// Earlier messages exist beyond what is shown.
    pub has_earlier: bool,
    /// An earlier page is on its way.
    pub loading_earlier: bool,
    /// The newest page is all here; until then it may still be filling.
    pub complete: bool,
    /// Why something is missing, as the person reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
}

/// The pages of an archived session read so far through the server.
#[derive(Debug, Clone, Default)]
pub struct RemotePages {
    /// Newest first, as the query answers.
    pub rows: Vec<Map<String, Value>>,
    /// The month the next page reads, and the cursor inside it.
    pub month: Option<String>,
    pub cursor: Option<(i64, String)>,
    /// Every month back to the session's creation has been read.
    pub exhausted: bool,
    /// A fetch is under way.
    pub loading: bool,
    /// At least one fetch has ended.
    pub fetched: bool,
    pub error: Option<String>,
}

/// One opened session's state on this device.
#[derive(Debug, Clone, Default)]
pub struct Opened {
    /// The streams showing it.
    pub streams: usize,
    /// Months widened back beyond the newest page.
    pub widen: u32,
    pub remote: RemotePages,
}

/// A session's own row, as the view reads it.
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub title: String,
    pub agent_actor: String,
    pub archived: bool,
    pub created_ms: i64,
    pub last_activity_ms: i64,
}

pub fn session_row(source: &Source, session_id: &str) -> Option<SessionRow> {
    let rows = read(
        source,
        "SELECT title, agent_actor, archived, created_at_ms, last_activity_ms \
         FROM sessions WHERE id = ?1",
        &[Value::from(session_id)],
    );
    let row = rows.first()?;
    Some(SessionRow {
        title: text(row, "title").to_owned(),
        agent_actor: text(row, "agent_actor").to_owned(),
        archived: truthy(row, "archived"),
        created_ms: int(row, "created_at_ms").unwrap_or(0),
        last_activity_ms: int(row, "last_activity_ms").unwrap_or(0),
    })
}

const MESSAGES: &str = "SELECT id, role, body, metadata, created_at_ms, author_actor, \
     author_client, turn_id, _sync_version AS version FROM messages \
     WHERE session_id = ?1 ORDER BY created_at_ms, id";

/// The view of an opened session, and the newest message's time (what the
/// person has now read). Reads the replica and `opened`'s pages only.
pub fn view(
    source: &Source,
    session_id: &str,
    opened: &Opened,
    now_ms: i64,
) -> Result<(AgentSessionViewVm, Option<i64>), String> {
    let Some(session) = session_row(source, session_id) else {
        return Err("This conversation is not on this device.".to_owned());
    };
    let directory = crate::hq::sessions::directory(source);
    let agent_name = directory
        .get(&session.agent_actor)
        .cloned()
        .unwrap_or_else(|| session.agent_actor.clone());
    let (mut rows, has_earlier, loading_earlier, complete, note) = if session.archived {
        let local = read(source, MESSAGES, &[Value::from(session_id)]);
        let remote = &opened.remote;
        let held: HashSet<&str> = local.iter().map(|row| text(row, "id")).collect();
        let mut rows: Vec<Map<String, Value>> = remote
            .rows
            .iter()
            .filter(|row| !held.contains(text(row, "id")))
            .cloned()
            .collect();
        rows.extend(local.iter().cloned());
        rows.sort_by(|a, b| {
            int(a, "created_at_ms")
                .cmp(&int(b, "created_at_ms"))
                .then_with(|| text(a, "id").cmp(text(b, "id")))
        });
        (
            rows,
            !remote.exhausted,
            remote.loading && remote.fetched,
            remote.fetched,
            remote.error.clone(),
        )
    } else {
        let newest: Vec<String> =
            newest_months(session.created_ms, session.last_activity_ms, now_ms)
                .iter()
                .map(|m| page_key(session_id, m))
                .collect();
        let earlier = earlier_months(session.created_ms, session.last_activity_ms, opened.widen);
        let mut units = newest.clone();
        units.extend(earlier.iter().map(|m| page_key(session_id, m)));
        let (rows, incomplete) = source
            .replica
            .query_covered(MESSAGES, &[Value::from(session_id)], &window("messages"), &units)
            .unwrap_or_else(|error| {
                tracing::debug!(database = %source.database, %error, "an opened session could not be read");
                (Vec::new(), units.clone())
            });
        let first = month(session.created_ms);
        let oldest_shown = earlier
            .last()
            .cloned()
            .unwrap_or_else(|| month(session.last_activity_ms.max(session.created_ms)));
        let complete = !newest.iter().any(|unit| incomplete.contains(unit));
        let loading = earlier
            .iter()
            .any(|m| incomplete.contains(&page_key(session_id, m)));
        (rows, oldest_shown > first, loading, complete, None)
    };
    let taken = taken(source, session_id, &rows);
    let messages = messages(source, &mut rows, &directory, &taken);
    let newest_ms = messages.last().map(|m| m.created_at_ms);
    Ok((
        AgentSessionViewVm {
            database: source.database.clone(),
            session_id: session_id.to_owned(),
            title: session.title,
            agent_mark: mark(&agent_name),
            agent_name,
            agent_actor: session.agent_actor,
            archived: session.archived,
            read_only: session.archived,
            messages,
            approvals: approvals::of_session(source, session_id, &directory, now_ms),
            has_earlier,
            loading_earlier,
            complete,
            note,
        },
        newest_ms,
    ))
}

/// The person's messages a turn names: by an agent message or a streamed
/// chunk whose `turn_id` is the message's id.
fn taken(source: &Source, session_id: &str, rows: &[Map<String, Value>]) -> HashSet<String> {
    let mut taken: HashSet<String> = rows
        .iter()
        .filter(|row| text(row, "role") == "agent")
        .filter_map(|row| opt_text(row, "turn_id"))
        .collect();
    taken.extend(
        read(
            source,
            "SELECT DISTINCT turn_id FROM turn_chunks WHERE session_id = ?1",
            &[Value::from(session_id)],
        )
        .iter()
        .filter_map(|row| opt_text(row, "turn_id")),
    );
    taken
}

fn messages(
    source: &Source,
    rows: &mut [Map<String, Value>],
    directory: &HashMap<String, String>,
    taken: &HashSet<String>,
) -> Vec<AgentMessageVm> {
    let last_person = rows.iter().rposition(|row| text(row, "role") == "person");
    rows.iter()
        .enumerate()
        .filter_map(|(at, row)| {
            let id = opt_text(row, "id")?;
            let author = text(row, "author_actor").to_owned();
            let role = if text(row, "role") == "agent" {
                AgentMessageRoleVm::Agent
            } else {
                AgentMessageRoleVm::Person
            };
            let state = (role == AgentMessageRoleVm::Person
                && text(row, "author_client") == source.client)
                .then(|| {
                    if int(row, "version").is_some_and(|v| v < 0) {
                        AgentMessageStateVm::Waiting
                    } else if taken.contains(&id) {
                        AgentMessageStateVm::Taken
                    } else {
                        AgentMessageStateVm::Sent
                    }
                });
            let metadata = json(row, "metadata");
            let ask = (role == AgentMessageRoleVm::Agent && is_ask(&metadata)).then(|| {
                AgentSessionAskVm {
                    options: metadata
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .filter_map(|o| o.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                    open: last_person.is_none_or(|person| person < at),
                }
            });
            Some(AgentMessageVm {
                id,
                role,
                author_name: directory
                    .get(&author)
                    .cloned()
                    .unwrap_or_else(|| author.clone()),
                author_actor: author,
                body: text(row, "body").to_owned(),
                created_at_ms: int(row, "created_at_ms").unwrap_or(0),
                state,
                ask,
            })
        })
        .collect()
}

/// What a remote fetch read: rows newest first and where the next page
/// starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Fetched {
    pub rows: Vec<Map<String, Value>>,
    pub month: Option<String>,
    pub cursor: Option<(i64, String)>,
    pub exhausted: bool,
}

/// Read the next page of an archived session through `sessionPage`: from
/// `pages`' month and cursor (the newest month on the first fetch), moving
/// to an earlier month when one runs out, until a page has rows or the
/// session's creation month is passed. Blocking; one request per month read.
pub fn fetch_page(
    source: &Source,
    session_id: &str,
    session: &SessionRow,
    pages: &RemotePages,
) -> Result<Fetched, ReplicaError> {
    let first = month(session.created_ms);
    let mut at = pages
        .month
        .clone()
        .unwrap_or_else(|| month(session.last_activity_ms.max(session.created_ms)));
    let mut cursor = pages.cursor.clone();
    let mut rows = Vec::new();
    for _ in 0..MAX_EMPTY_MONTHS {
        let (before_ms, before_id) = cursor.clone().unwrap_or((NEWEST, String::new()));
        let page = source.replica.remote_query(
            SESSION_PAGE,
            json!({
                "pageKey": page_key(session_id, &at),
                "beforeMs": before_ms,
                "beforeId": before_id,
                "pageSize": PAGE_SIZE,
            }),
        )?;
        let full = page.len() >= PAGE_SIZE;
        if let Some(last) = page.last() {
            cursor = Some((
                int(last, "created_at_ms").unwrap_or(0),
                text(last, "id").to_owned(),
            ));
        }
        rows.extend(page);
        if full {
            return Ok(Fetched {
                rows,
                month: Some(at),
                cursor,
                exhausted: false,
            });
        }
        let exhausted = at <= first;
        match previous_month(&at) {
            Some(before) if !exhausted => {
                at = before;
                cursor = None;
            }
            _ => {
                return Ok(Fetched {
                    rows,
                    month: None,
                    cursor: None,
                    exhausted: true,
                })
            }
        }
        if !rows.is_empty() {
            return Ok(Fetched {
                rows,
                month: Some(at),
                cursor: None,
                exhausted: false,
            });
        }
    }
    Ok(Fetched {
        rows,
        month: Some(at),
        cursor,
        exhausted: false,
    })
}

/// The sentence an archived session shows when the server could not be
/// asked.
pub fn fetch_sentence(database: &str, error: &ReplicaError) -> String {
    match error {
        ReplicaError::NotSyncing => format!(
            "This conversation is archived on the server; keeper will fetch it when {database} is reachable."
        ),
        other => format!("keeper could not fetch this archived conversation from {database}: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_page_id_is_the_contracts() {
        let generated = include_str!("../../../../../syncular/syncular.queries.ts");
        assert!(
            generated.contains(&format!("id: '{SESSION_PAGE}'")),
            "syncular.queries.ts names another sessionPage id; update SESSION_PAGE"
        );
    }
}
