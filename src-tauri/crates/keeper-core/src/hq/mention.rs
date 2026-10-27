//! `@mention` brings another agent in (109.24, Q109-21 a; R-HQ-76).
//!
//! A person's line that starts with `@<handle>` is not a message to the
//! session's own agent. keeper, acting as the person, inserts a **child
//! session** for the agent the handle names — `parent_session` this one, in
//! the same database — with the person's owner membership and the line as
//! its first message, in one commit: the shape keeper-hq's V3 lets a person
//! member of the parent insert (V11 keeps it in this database). The session's
//! own agent is not asked about it; only people prompt, so no agent prompts
//! another here.
//!
//! What may be named is decided here, never in the webview: the database's
//! directory agents other than the session's own, by handle (the actor id)
//! and name. The session's own agent, an agent of another database and a
//! handle no agent has are refused before anything is written, with a
//! sentence. A refusal the server returns later is read from the replica's
//! outcome journal and shown on the person's line, in plain words.
//!
//! The parent's view carries the threads it started ([`AgentChildSessionVm`])
//! within the months it shows, the newest [`MAX_INLINE_CHILDREN`]: each one's
//! agent, the person's line, only its latest turn through 109.13's projection
//! (`items`, `folds`) with the answer cut at [`MAX_INLINE_ANSWER`], and its
//! session id for "open thread". A thread is told apart by its first line's
//! id, never its session id: a refused child's id is minted again by the
//! next creation (R10924K-01).
//!
//! A mention declines what the session's banner showed, as any message does
//! (Q109-12 a, R10924K-03): the local checks run first, so a mention refused
//! here declines nothing; then the declines; then the child's commit.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::approvals::{self, Decisions, ShownApprovalVm};
use crate::hq::live::{AgentLiveTurnVm, AgentViewPrefsVm};
use crate::hq::port::{Rejection, ReplicaError, CONFLICT};
use crate::hq::send::{self, SendError};
use crate::hq::sessions::{
    self, int, mark, month, opt_text, read, text, title_from, truthy, AgentSessionRef, Source,
};
use crate::hq::timeline::{
    self, AgentMessageRoleVm, AgentMessageStateVm, AgentMessageVm, AgentTimelineItemVm as Item,
    AgentTurnFoldVm,
};
use crate::hq::{Hq, HqError};
use crate::vm::{ChatInlineKindVm, ChatInlinePreviewVm, ChatInlineRowVm};

/// The longest handle query worth asking about (the composer's bound too).
pub const MAX_QUERY: usize = 64;

/// Why files and a mention do not go together, as the person reads it.
pub const WITH_FILES: &str = "A mention brings another agent in without files: send the @ line on its own, then attach the files in that agent's thread.";

/// A conversation started from this one by a person's `@mention`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentChildSessionVm {
    /// The thread's identity in this view: its first line's id, which no
    /// later write mints again (a refused child's session id is). The `child`
    /// item names it; so does any list keyed by thread.
    pub thread_id: String,
    /// The child's id in this database: what "open thread" opens.
    pub session_id: String,
    pub agent_actor: String,
    pub agent_name: String,
    pub agent_mark: String,
    #[ts(type = "number")]
    pub created_at_ms: i64,
    /// The person's line, the child's first message, with its state:
    /// `refused` when the server refused the commit that carried it.
    /// Absent only when the replica no longer holds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub prompt: Option<AgentMessageVm>,
    /// Why the server refused to start it, in plain words; then the child
    /// never existed and nothing below is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub refusal: Option<String>,
    /// The thread is archived: its rows are read only when it is opened, so
    /// nothing of it is drawn inline but that it is archived.
    pub archived: bool,
    /// An answer below was cut at [`MAX_INLINE_ANSWER`] characters: the
    /// rest is read in the thread.
    pub truncated: bool,
    /// The child's latest turn, as 109.13 projects it: from its latest fold
    /// on. Empty while the agent has not started on it.
    pub items: Vec<Item>,
    /// The folds `items` names.
    pub folds: Vec<AgentTurnFoldVm>,
    /// The messages `items` and the folds' lead-ins name.
    pub messages: Vec<AgentMessageVm>,
    /// The answers `items` names still being written.
    pub live: Vec<AgentLiveTurnVm>,
}

/// A line's mention: the handle as typed, and what follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mention<'a> {
    pub handle: &'a str,
    pub rest: &'a str,
}

/// The mention a person's line starts with: `@` and a handle up to the first
/// whitespace, a trailing `,:;.!?` not part of it. `None` for any other line
/// — a mention later in the line is text for the session's own agent.
pub fn parse(line: &str) -> Option<Mention<'_>> {
    let after = line.trim_start().strip_prefix('@')?;
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    let handle = after[..end].trim_end_matches([',', ':', ';', '.', '!', '?']);
    (!handle.is_empty()).then(|| Mention {
        handle,
        rest: after[end..].trim(),
    })
}

/// Why a mention was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MentionError {
    #[error("{name} already answers this conversation; write without @{handle}.")]
    Own { name: String, handle: String },
    #[error(
        "{name} is an agent of {elsewhere}, not {database}: a conversation brings in only agents of its own database."
    )]
    OtherDatabase {
        name: String,
        elsewhere: String,
        database: String,
    },
    #[error("No agent of {database} is called @{handle}.")]
    Unknown { handle: String, database: String },
    #[error("Write something for {name} after @{handle}.")]
    Empty { name: String, handle: String },
    /// A request the banner showed could not be declined; nothing written.
    #[error("{0}")]
    Declines(String),
    /// The child could not be written after the requests it declines were.
    #[error("Your message was not written ({0}); the requests it declines stay declined.")]
    AfterDeclines(ReplicaError),
    #[error(transparent)]
    Send(#[from] SendError),
}

/// One agent of a database's directory.
struct Agent {
    id: String,
    name: String,
}

fn agents(source: &Source) -> Vec<Agent> {
    read(
        source,
        "SELECT id, display_name FROM actors WHERE kind = 'agent' ORDER BY display_name, id",
        &[],
    )
    .into_iter()
    .filter_map(|row| {
        let id = opt_text(&row, "id")?;
        let name = opt_text(&row, "display_name").unwrap_or_else(|| id.clone());
        Some(Agent { id, name })
    })
    .collect()
}

fn named<'a>(agents: &'a [Agent], handle: &str) -> Option<&'a Agent> {
    agents
        .iter()
        .find(|agent| agent.id.eq_ignore_ascii_case(handle))
}

/// The most threads a parent's view draws inline: the newest of those
/// started within the months it shows (R10924K-02). Older ones open from the
/// session list.
pub const MAX_INLINE_CHILDREN: usize = 30;

/// The longest answer drawn inline, in characters: past it, "Open thread".
pub const MAX_INLINE_ANSWER: usize = 1_200;

/// Start the child `mention` asks for from `parent` of `source`'s database,
/// `line` its first message — or refuse, writing nothing. `sources` are every
/// database this device holds: an agent found only in another is named so.
/// `decline` declines what the banner showed, after every local check and
/// before the child's commit; it says whether it declined anything. The
/// child's id and its commit's.
pub fn start(
    sources: &[Source],
    source: &Source,
    parent: &str,
    mention: Mention<'_>,
    line: &str,
    decline: impl FnOnce() -> Result<bool, String>,
) -> Result<(String, String), MentionError> {
    let session = timeline::session_row(source, parent).ok_or(SendError::Unknown)?;
    if session.archived {
        return Err(SendError::Archived.into());
    }
    let here = agents(source);
    let Some(agent) = named(&here, mention.handle) else {
        let elsewhere = sources
            .iter()
            .filter(|other| other.database != source.database)
            .find_map(|other| {
                named(&agents(other), mention.handle)
                    .map(|agent| (agent.name.clone(), other.database.clone()))
            });
        return Err(match elsewhere {
            Some((name, elsewhere)) => MentionError::OtherDatabase {
                name,
                elsewhere,
                database: source.database.clone(),
            },
            None => MentionError::Unknown {
                handle: mention.handle.to_owned(),
                database: source.database.clone(),
            },
        });
    };
    if agent.id == session.agent_actor {
        return Err(MentionError::Own {
            name: agent.name.clone(),
            handle: agent.id.clone(),
        });
    }
    if mention.rest.is_empty() {
        return Err(MentionError::Empty {
            name: agent.name.clone(),
            handle: agent.id.clone(),
        });
    }
    let declined = decline().map_err(MentionError::Declines)?;
    sessions::create_child(source, parent, &agent.id, &title_from(mention.rest), line).map_err(
        |error| match declined {
            true => MentionError::AfterDeclines(error),
            false => SendError::Replica(error).into(),
        },
    )
}

/// What an `@` at the start of the draft offers: the database's agents other
/// than the session's own whose handle, or a word of whose name, starts with
/// what follows the `@`, by name. No token — the caret not in a leading
/// `@` word — offers nothing.
pub fn preview(
    source: &Source,
    session_id: &str,
    before: &str,
    after: &str,
) -> ChatInlinePreviewVm {
    let mut preview = ChatInlinePreviewVm {
        before: before.to_owned(),
        after: after.to_owned(),
        token: None,
        rows: Vec::new(),
        note: None,
    };
    let token = before.trim_start();
    let Some(query) = token.strip_prefix('@') else {
        return preview;
    };
    if query.chars().count() > MAX_QUERY
        || query.contains(char::is_whitespace)
        || after.chars().next().is_some_and(|c| !c.is_whitespace())
    {
        return preview;
    }
    let own = timeline::session_row(source, session_id).map(|session| session.agent_actor);
    let query = query.to_lowercase();
    preview.token = Some(token.to_owned());
    preview.rows = agents(source)
        .into_iter()
        .filter(|agent| own.as_deref() != Some(agent.id.as_str()))
        .filter(|agent| {
            agent.id.to_lowercase().starts_with(&query)
                || agent
                    .name
                    .to_lowercase()
                    .split_whitespace()
                    .any(|word| word.starts_with(&query))
        })
        .map(|agent| ChatInlineRowVm {
            kind: ChatInlineKindVm::Mention,
            label: format!("@{}", agent.id),
            insert: format!("@{}", agent.id),
            detail: agent.name,
            key: agent.id,
        })
        .collect();
    preview
}

/// The threads a person started from `parent` since `since` (the oldest
/// month the parent shows), the newest [`MAX_INLINE_CHILDREN`], oldest
/// first: those the replica holds, each with only its latest turn, and those
/// the server refused, from the outcome journal. With them, how many older
/// ones of those months were left out (R10924K-08).
pub(crate) fn children(
    source: &Source,
    parent: &str,
    since: &str,
    directory: &HashMap<String, String>,
    prefs: AgentViewPrefsVm,
) -> (Vec<AgentChildSessionVm>, usize) {
    let rows: Vec<Map<String, Value>> = read(
        source,
        "SELECT s.id AS id, s.agent_actor AS agent_actor, s.created_at_ms AS created_at_ms, \
         s.archived AS archived FROM sessions s JOIN actors a ON a.id = s.created_by_actor \
         WHERE s.parent_session = ?1 AND a.kind = 'person' ORDER BY s.created_at_ms, s.id",
        &[Value::from(parent)],
    )
    .into_iter()
    .filter(|row| month(int(row, "created_at_ms").unwrap_or(0)).as_str() >= since)
    .collect();
    let name_of = |actor: &str| {
        directory
            .get(actor)
            .cloned()
            .unwrap_or_else(|| actor.to_owned())
    };
    // Only the newest are projected; a refused one competes for the same
    // places by when it was started.
    let refused = refused(source, parent, since, &name_of);
    enum Started<'a> {
        Held(&'a Map<String, Value>),
        Refused(Box<AgentChildSessionVm>),
    }
    let mut started: Vec<(i64, Started)> = rows
        .iter()
        .map(|row| (int(row, "created_at_ms").unwrap_or(0), Started::Held(row)))
        .chain(
            refused
                .into_iter()
                .map(|child| (child.created_at_ms, Started::Refused(Box::new(child)))),
        )
        .collect();
    started.sort_by_key(|(at, _)| *at);
    let skip = started.len().saturating_sub(MAX_INLINE_CHILDREN);
    let mut children: Vec<AgentChildSessionVm> = started
        .into_iter()
        .skip(skip)
        .filter_map(|(at, started)| match started {
            Started::Refused(refused) => Some(*refused),
            Started::Held(row) => {
                let id = opt_text(row, "id")?;
                let agent = text(row, "agent_actor");
                Some(held(
                    source,
                    &id,
                    agent,
                    &name_of(agent),
                    at,
                    truthy(row, "archived"),
                    directory,
                    prefs,
                ))
            }
        })
        .collect();
    children.sort_by(|a, b| {
        (a.created_at_ms, a.thread_id.as_str()).cmp(&(b.created_at_ms, b.thread_id.as_str()))
    });
    (children, skip)
}

/// What the parent says of the threads [`children`] left out, if any.
pub(crate) fn left_out(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some(
            "1 earlier thread started here is not shown; open it from the sessions list."
                .to_owned(),
        ),
        n => Some(format!(
            "{n} earlier threads started here are not shown; open them from the sessions list."
        )),
    }
}

/// A thread the replica holds: its first line and its latest turn, the
/// answer cut for the parent; an archived one, nothing of it.
#[allow(clippy::too_many_arguments)]
fn held(
    source: &Source,
    id: &str,
    agent: &str,
    agent_name: &str,
    created_at_ms: i64,
    archived: bool,
    directory: &HashMap<String, String>,
    prefs: AgentViewPrefsVm,
) -> AgentChildSessionVm {
    let mut child = AgentChildSessionVm {
        thread_id: id.to_owned(),
        session_id: id.to_owned(),
        agent_actor: agent.to_owned(),
        agent_mark: mark(agent_name),
        agent_name: agent_name.to_owned(),
        created_at_ms,
        prompt: None,
        refusal: None,
        archived,
        truncated: false,
        items: Vec::new(),
        folds: Vec::new(),
        messages: Vec::new(),
        live: Vec::new(),
    };
    if archived {
        return child;
    }
    let turn = timeline::latest_turn(source, id, directory, prefs);
    if let Some(prompt) = &turn.prompt {
        child.thread_id = prompt.id.clone();
    }
    // The person's line is cut as the answer is (R10924K-07): the thread
    // holds it whole.
    let mut truncated = false;
    child.prompt = turn.prompt.map(|prompt| AgentMessageVm {
        body: cut(&prompt.body, &mut truncated),
        ..prompt
    });
    child.items = turn.items;
    let turns: HashSet<&str> = child
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Fold { turn_id } | Item::LiveAnswer { turn_id } => Some(turn_id.as_str()),
            _ => None,
        })
        .collect();
    let named: HashSet<&str> = child
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Message { message_id } => Some(message_id.as_str()),
            _ => None,
        })
        .collect();
    // The parent draws a fold as its one summary line, Rust's sentence
    // (R10924K-13, -14): its steps, their failures, the thinking, the
    // lead-ins and its requests are the thread's to show, behind "Open
    // thread" — so the view does not grow with the steps of a thread's
    // newest turn.
    child.folds = turn
        .folds
        .into_iter()
        .filter(|fold| turns.contains(fold.turn_id.as_str()))
        .map(|fold| AgentTurnFoldVm {
            lead_in_ids: Vec::new(),
            reasoning: None,
            steps: Vec::new(),
            failures: Vec::new(),
            approvals: Vec::new(),
            waiting: Vec::new(),
            ..fold
        })
        .collect();
    child.messages = turn
        .messages
        .into_iter()
        .filter(|message| named.contains(message.id.as_str()))
        .map(|message| AgentMessageVm {
            body: cut(&message.body, &mut truncated),
            details: None,
            ..message
        })
        .collect();
    child.live = turn
        .live
        .into_iter()
        .filter(|live| turns.contains(live.turn_id.as_str()))
        .map(|live| AgentLiveTurnVm {
            text: cut(&live.text, &mut truncated),
            ..live
        })
        .collect();
    child.truncated = truncated;
    child
}

/// `text` cut at [`MAX_INLINE_ANSWER`] characters, at a word where one is
/// near, with an ellipsis; `truncated` set when it was.
fn cut(text: &str, truncated: &mut bool) -> String {
    let Some((at, _)) = text.char_indices().nth(MAX_INLINE_ANSWER) else {
        return text.to_owned();
    };
    *truncated = true;
    let head = &text[..at];
    let head = head
        .rfind(char::is_whitespace)
        .filter(|space| *space > at / 2)
        .map_or(head, |space| &head[..space]);
    format!("{}…", head.trim_end())
}

/// The threads of `parent` started since `since` whose commit the server
/// refused: gone from the replica, kept by its outcome journal with the
/// person's line and why. Each is its own thread, told by its line's id.
fn refused(
    source: &Source,
    parent: &str,
    since: &str,
    name_of: &dyn Fn(&str) -> String,
) -> Vec<AgentChildSessionVm> {
    let commits = match source.replica.refused_commits("sessions") {
        Ok(commits) => commits,
        Err(error) => {
            tracing::debug!(database = %source.database, %error, "refused mentions could not be read");
            return Vec::new();
        }
    };
    commits
        .into_iter()
        .filter_map(|commit| {
            // A child's creation names its parent; a later write to it does not.
            let session = commit.rows.iter().find_map(|(table, values)| {
                (table == "sessions"
                    && text(values, "parent_session") == parent
                    && values.contains_key("created_at_ms"))
                .then_some(values)
            })?;
            let id = text(session, "id");
            let created_at_ms = int(session, "created_at_ms").unwrap_or(0);
            if id.is_empty() || month(created_at_ms).as_str() < since {
                return None;
            }
            let agent = text(session, "agent_actor");
            let agent_name = name_of(agent);
            let line = commit
                .rows
                .iter()
                .find(|(table, values)| table == "messages" && text(values, "session_id") == id)
                .map(|(_, values)| values);
            // A refusal stands unless the replica holds that same thread again —
            // its line, never one that only reuses its session id — and is
            // dropped before the threads are counted (R10924K-01, -11).
            if line.is_some_and(|values| held_line(source, text(values, "id"))) {
                return None;
            }
            let prompt = line.map(|values| refused_line(source, values, &name_of(&source.actor)));
            Some(AgentChildSessionVm {
                thread_id: prompt
                    .as_ref()
                    .map_or_else(|| commit.commit_id.clone(), |line| line.id.clone()),
                session_id: id.to_owned(),
                agent_actor: agent.to_owned(),
                agent_mark: mark(&agent_name),
                created_at_ms,
                refusal: Some(refusal_sentence(
                    &commit.rejection,
                    &agent_name,
                    &source.database,
                )),
                agent_name,
                prompt,
                archived: false,
                truncated: false,
                items: Vec::new(),
                folds: Vec::new(),
                messages: Vec::new(),
                live: Vec::new(),
            })
        })
        .collect()
}

/// Whether the replica holds message `id`: a refused thread's line come back.
fn held_line(source: &Source, id: &str) -> bool {
    !id.is_empty()
        && !read(
            source,
            "SELECT 1 AS held FROM messages WHERE id = ?1 LIMIT 1",
            &[Value::from(id)],
        )
        .is_empty()
}

fn refused_line(source: &Source, values: &Map<String, Value>, author: &str) -> AgentMessageVm {
    AgentMessageVm {
        id: text(values, "id").to_owned(),
        role: AgentMessageRoleVm::Person,
        author_actor: source.actor.clone(),
        mine: true,
        author_name: author.to_owned(),
        // Cut as a held line is; a refused thread has nowhere to read the rest,
        // so nothing says it continues.
        body: cut(text(values, "body"), &mut false),
        created_at_ms: int(values, "created_at_ms").unwrap_or(0),
        state: Some(AgentMessageStateVm::Refused),
        ask: None,
        details: None,
        attachments: Vec::new(),
    }
}

/// Why the server refused to start a mention's child, by its code (V3,
/// V11), matched exactly; any other refusal in the server's own words.
pub fn refusal_sentence(rejection: &Rejection, agent: &str, database: &str) -> String {
    let why = match rejection.code.as_str() {
        "hq.v3.parent_not_member" => "you are no longer a member of this conversation".to_owned(),
        "hq.v11.agent_outside_directory" => format!("{agent} is not an agent of {database}"),
        "hq.v11.parent_outside_database" => format!("this conversation is not in {database}"),
        "hq.v3.created_at_window" => {
            "this device's clock was too far off when you sent it".to_owned()
        }
        CONFLICT => "another device changed this conversation first".to_owned(),
        _ => rejection.to_string(),
    };
    format!("The agents server refused to bring {agent} in: {why}. Nothing was sent to {agent}.")
}

/// Put each child's item where it was started among the parent's: before
/// the first message written after it, and above the fold that opens that
/// message's turn; after everything when nothing is later. `children` is
/// oldest first.
pub(crate) fn place(
    items: &mut Vec<Item>,
    messages: &[AgentMessageVm],
    children: &[AgentChildSessionVm],
) {
    let written: HashMap<&str, i64> = messages
        .iter()
        .map(|message| (message.id.as_str(), message.created_at_ms))
        .collect();
    for child in children {
        let mut at = items
            .iter()
            .position(|item| match item {
                Item::Message { message_id } => written
                    .get(message_id.as_str())
                    .is_some_and(|written| *written > child.created_at_ms),
                Item::LiveAnswer { .. } => true,
                Item::Fold { .. } | Item::Child { .. } => false,
            })
            .unwrap_or(items.len());
        if at > 0 && at < items.len() && matches!(items[at - 1], Item::Fold { .. }) {
            at -= 1;
        }
        items.insert(
            at,
            Item::Child {
                thread_id: child.thread_id.clone(),
            },
        );
    }
}

impl Hq {
    /// What an `@` at the start of a session's draft offers.
    pub fn mention_preview(
        &self,
        key: &AgentSessionRef,
        before: &str,
        after: &str,
    ) -> Result<ChatInlinePreviewVm, HqError> {
        let source = self.source(&key.database)?;
        Ok(preview(&source, &key.session_id, before, after))
    }

    /// The composer's line ([`send_line`]) in `key`'s database.
    pub fn send_line(
        &self,
        key: &AgentSessionRef,
        line: &str,
        shown: &[ShownApprovalVm],
    ) -> Result<(), HqError> {
        let source = self.source(&key.database)?;
        let sent = send_line(
            &self.sources(),
            &source,
            self.decisions()?,
            &key.session_id,
            line,
            shown,
        );
        self.touch();
        sent.map_err(HqError::Sentence)
    }
}

/// The composer's line: a mention starts its agent's child in `session_id`,
/// any other line is a message to the session's own agent. Either declines
/// first what the banner showed (`shown`, Q109-12 a): a mention after its
/// local checks, so one refused here declines nothing (R10924K-03, -10).
pub(crate) fn send_line(
    sources: &[Source],
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    line: &str,
    shown: &[ShownApprovalVm],
) -> Result<(), String> {
    match parse(line) {
        Some(mention) => start(sources, source, session_id, mention, line, || {
            decline(source, decisions, session_id, shown)
        })
        .map(drop)
        .map_err(|error| error.to_string()),
        None => send::send(source, decisions, session_id, line, shown)
            .map_err(|error| error.to_string()),
    }
}

/// Decline the requests of `session_id` the banner showed (`shown`) that
/// still wait for this person, as a message does (109.11's
/// [`approvals::decline_pending`]); whether any was.
fn decline(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    shown: &[ShownApprovalVm],
) -> Result<bool, String> {
    approvals::decline_pending(source, decisions, session_id, shown, sessions::now_ms())
        .map(|declined| !declined.is_empty())
}

#[cfg(test)]
#[path = "mention_tests.rs"]
mod tests;
