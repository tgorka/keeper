//! A session's `agent.toml`: the opening record of an agent's session
//! (AD-365, story 89.5).
//!
//! Written once, when the session is created, and never rewritten: what
//! changes afterwards — scope, label, run state, claims — is a log line, and
//! the `.keeper/` index projects the current value. The file is a contract:
//! an unknown key is refused with its name, and every bound is refused one
//! past its edge with a sentence naming the key.

use std::collections::BTreeSet;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, SecondsFormat, Utc};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use ulid::Ulid;

use crate::agents::label::{Integrity, Label, Readers};

/// The file's name inside a session folder.
pub const FILE_NAME: &str = "agent.toml";

/// The grammar this build reads and writes.
pub const GRAMMAR_VERSION: i64 = 1;

/// The longest `title`, in characters.
pub const TITLE_MAX: usize = 120;

/// The deepest delegation hop.
pub const HOP_MAX: i64 = 3;

const ROOT_KEYS: [&str; 18] = [
    "version",
    "id",
    "agent",
    "drive",
    "kind",
    "title",
    "requested_by",
    "parent",
    "room",
    "drives",
    "label",
    "needs",
    "pin",
    "hop",
    "dispatch_chain",
    "limits",
    "workflow",
    "created_at",
];
const PARENT_KEYS: [&str; 3] = ["drive", "session", "room"];
const LABEL_KEYS: [&str; 3] = ["readers", "integrity", "local_only"];
const LIMITS_KEYS: [&str; 2] = ["rounds_per_exchange", "tokens"];

/// What a session is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SessionKind {
    /// A proxy's DM with its person.
    Main,
    /// A proxy conversation the person started.
    Conversation,
    /// Opened by another agent's delegation.
    Delegated,
    /// A card's scheduled run.
    Scheduled,
    /// A workflow's run.
    Workflow,
    /// Ingress from an outside system.
    Gate,
}

impl SessionKind {
    /// Every kind, in the documented order.
    pub const ALL: [SessionKind; 6] = [
        Self::Main,
        Self::Conversation,
        Self::Delegated,
        Self::Scheduled,
        Self::Workflow,
        Self::Gate,
    ];

    /// The word the file uses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Conversation => "conversation",
            Self::Delegated => "delegated",
            Self::Scheduled => "scheduled",
            Self::Workflow => "workflow",
            Self::Gate => "gate",
        }
    }
}

impl fmt::Display for SessionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for SessionKind {
    type Err = ();

    fn from_str(word: &str) -> Result<Self, ()> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.as_str() == word)
            .ok_or(())
    }
}

/// The delegating session (AD-385).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionParent {
    /// The delegating session's home drive id.
    pub drive: String,
    /// Its zone-relative path.
    pub session: String,
    /// Its room.
    pub room: OwnedRoomId,
}

/// A delegated session's bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLimits {
    /// Model rounds per exchange.
    pub rounds_per_exchange: u32,
    /// Tokens for the whole session.
    pub tokens: u64,
}

/// A parsed session `agent.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionAgent {
    /// Caller-supplied, so a retried create is the same session.
    pub id: Ulid,
    /// The owning agent's id in its home drive.
    pub agent: String,
    /// The home drive's id.
    pub drive: String,
    /// What the session is for.
    pub kind: SessionKind,
    /// At most [`TITLE_MAX`] characters.
    pub title: String,
    /// A person or an agent user.
    pub requested_by: OwnedUserId,
    /// The delegating session, for a delegated one.
    pub parent: Option<SessionParent>,
    /// The session's Matrix room.
    pub room: OwnedRoomId,
    /// Drives in scope at opening; the home drive when the file names none.
    pub drives: Vec<String>,
    /// The label at opening.
    pub label: Label,
    /// Capabilities for placement; `None` means the agent's own.
    pub needs: Option<Vec<String>>,
    /// A host slug or `""`; `None` means the agent's own pin.
    pub pin: Option<String>,
    /// Delegation depth, 0–[`HOP_MAX`].
    pub hop: u8,
    /// Who the work came from, the person who started it first, then each
    /// agent that handed it on (R76); empty in a file that names none.
    pub dispatch_chain: Vec<OwnedUserId>,
    /// A delegated session's bounds; `None` means the agent's own.
    pub limits: Option<SessionLimits>,
    /// A folder under `_workflows/`.
    pub workflow: Option<String>,
    /// When the session was created.
    pub created_at: DateTime<Utc>,
}

/// Why a session `agent.toml` cannot be read. Each is one sentence.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionRefusal {
    #[error("The session's agent.toml is not valid TOML: {0}")]
    Syntax(String),
    #[error("The session's agent.toml has `{key}`, which is not one of its keys.")]
    UnknownKey { key: String },
    #[error("The session's agent.toml needs `{key}`.")]
    Missing { key: String },
    #[error("`{key}` in the session's agent.toml must be {expected}.")]
    WrongType { key: String, expected: &'static str },
    #[error("`{key}` in the session's agent.toml is {value}: {why}")]
    Invalid {
        key: String,
        value: String,
        why: &'static str,
    },
}

impl SessionRefusal {
    /// The refusal as the sentence a person reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }

    /// The key the refusal names, dotted for a key inside a table.
    pub fn key(&self) -> Option<&str> {
        match self {
            Self::Syntax(_) => None,
            Self::UnknownKey { key }
            | Self::Missing { key }
            | Self::WrongType { key, .. }
            | Self::Invalid { key, .. } => Some(key),
        }
    }
}

fn missing(key: &str) -> SessionRefusal {
    SessionRefusal::Missing {
        key: key.to_owned(),
    }
}

fn wrong(key: &str, expected: &'static str) -> SessionRefusal {
    SessionRefusal::WrongType {
        key: key.to_owned(),
        expected,
    }
}

fn invalid(key: &str, value: impl fmt::Display, why: &'static str) -> SessionRefusal {
    SessionRefusal::Invalid {
        key: key.to_owned(),
        value: value.to_string(),
        why,
    }
}

fn unknown_keys(table: &toml::Table, allowed: &[&str], prefix: &str) -> Result<(), SessionRefusal> {
    match table.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(SessionRefusal::UnknownKey {
            key: format!("{prefix}{key}"),
        }),
        None => Ok(()),
    }
}

fn text(table: &toml::Table, key: &str, name: &str) -> Result<Option<String>, SessionRefusal> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(wrong(name, "text in quotes")),
    }
}

fn required_text(table: &toml::Table, key: &str, name: &str) -> Result<String, SessionRefusal> {
    text(table, key, name)?.ok_or_else(|| missing(name))
}

fn text_list(table: &toml::Table, key: &str) -> Result<Option<Vec<String>>, SessionRefusal> {
    let Some(value) = table.get(key) else {
        return Ok(None);
    };
    let toml::Value::Array(items) = value else {
        return Err(wrong(key, "a list of text"));
    };
    items
        .iter()
        .map(|item| match item {
            toml::Value::String(value) => Ok(value.clone()),
            _ => Err(wrong(key, "a list of text")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn integer(table: &toml::Table, key: &str, name: &str) -> Result<Option<i64>, SessionRefusal> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(value)) => Ok(Some(*value)),
        Some(_) => Err(wrong(name, "a whole number")),
    }
}

/// `[a-z0-9][a-z0-9-]{0,31}`: an agent's or a drive's id.
fn fits_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn id_field(table: &toml::Table, key: &str) -> Result<String, SessionRefusal> {
    let value = required_text(table, key, key)?;
    if fits_id(&value) {
        Ok(value)
    } else {
        Err(invalid(
            key,
            format!("\"{value}\""),
            "an id is lowercase letters, digits and dashes, at most 32, not starting with a dash.",
        ))
    }
}

fn user(key: &str, raw: &str) -> Result<OwnedUserId, SessionRefusal> {
    UserId::parse(raw).map_err(|_| {
        invalid(
            key,
            format!("\"{raw}\""),
            "that is not a Matrix user id (@name:server).",
        )
    })
}

fn room(key: &str, raw: &str) -> Result<OwnedRoomId, SessionRefusal> {
    RoomId::parse(raw).map_err(|_| {
        invalid(
            key,
            format!("\"{raw}\""),
            "that is not a Matrix room id (!id:server).",
        )
    })
}

fn label(table: &toml::Table) -> Result<Label, SessionRefusal> {
    let Some(value) = table.get("label") else {
        return Err(missing("label"));
    };
    let toml::Value::Table(label) = value else {
        return Err(wrong("label", "a table, [label]"));
    };
    unknown_keys(label, &LABEL_KEYS, "label.")?;
    let readers = match label.get("readers") {
        None => return Err(missing("label.readers")),
        Some(toml::Value::String(star)) if star == "*" => Readers::Anyone,
        Some(toml::Value::Array(items)) => {
            let mut set = BTreeSet::new();
            for item in items {
                let toml::Value::String(raw) = item else {
                    return Err(wrong("label.readers", "\"*\" or a list of Matrix user ids"));
                };
                if !set.insert(user("label.readers", raw)?) {
                    return Err(invalid(
                        "label.readers",
                        format!("\"{raw}\""),
                        "a reader is listed twice.",
                    ));
                }
            }
            Readers::Only(set)
        }
        Some(_) => return Err(wrong("label.readers", "\"*\" or a list of Matrix user ids")),
    };
    let word = required_text(label, "integrity", "label.integrity")?;
    let integrity = match word.as_str() {
        "owner" => Integrity::Owner,
        "peer" => Integrity::Peer,
        "agent" => Integrity::Agent,
        "untrusted" => Integrity::Untrusted,
        _ => {
            return Err(invalid(
                "label.integrity",
                format!("\"{word}\""),
                "it is one of owner, peer, agent, untrusted.",
            ))
        }
    };
    let local_only = match label.get("local_only") {
        None => false,
        Some(toml::Value::Boolean(on)) => *on,
        Some(_) => return Err(wrong("label.local_only", "true or false")),
    };
    Ok(Label {
        readers,
        integrity,
        local_only,
    })
}

fn parent(table: &toml::Table) -> Result<Option<SessionParent>, SessionRefusal> {
    let Some(value) = table.get("parent") else {
        return Ok(None);
    };
    let toml::Value::Table(parent) = value else {
        return Err(wrong("parent", "a table, [parent]"));
    };
    unknown_keys(parent, &PARENT_KEYS, "parent.")?;
    let drive = required_text(parent, "drive", "parent.drive")?;
    if !fits_id(&drive) {
        return Err(invalid(
            "parent.drive",
            format!("\"{drive}\""),
            "a drive id is lowercase letters, digits and dashes, at most 32.",
        ));
    }
    let session = required_text(parent, "session", "parent.session")?;
    if session.is_empty() {
        return Err(invalid(
            "parent.session",
            "empty",
            "it names the delegating session's folder.",
        ));
    }
    let room = room(
        "parent.room",
        &required_text(parent, "room", "parent.room")?,
    )?;
    Ok(Some(SessionParent {
        drive,
        session,
        room,
    }))
}

fn limits(table: &toml::Table) -> Result<Option<SessionLimits>, SessionRefusal> {
    let Some(value) = table.get("limits") else {
        return Ok(None);
    };
    let toml::Value::Table(limits) = value else {
        return Err(wrong("limits", "a table, [limits]"));
    };
    unknown_keys(limits, &LIMITS_KEYS, "limits.")?;
    let rounds = integer(limits, "rounds_per_exchange", "limits.rounds_per_exchange")?
        .ok_or_else(|| missing("limits.rounds_per_exchange"))?;
    let rounds_per_exchange = u32::try_from(rounds)
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| invalid("limits.rounds_per_exchange", rounds, "it is at least 1."))?;
    let tokens =
        integer(limits, "tokens", "limits.tokens")?.ok_or_else(|| missing("limits.tokens"))?;
    let tokens = u64::try_from(tokens)
        .ok()
        .filter(|n| *n >= 1)
        .ok_or_else(|| invalid("limits.tokens", tokens, "it is at least 1."))?;
    Ok(Some(SessionLimits {
        rounds_per_exchange,
        tokens,
    }))
}

/// Read a session `agent.toml`.
pub fn parse_session_agent_toml(text_in: &str) -> Result<SessionAgent, SessionRefusal> {
    let table: toml::Table = toml::from_str(text_in).map_err(|error| {
        SessionRefusal::Syntax(error.message().lines().next().unwrap_or("").to_owned())
    })?;
    unknown_keys(&table, &ROOT_KEYS, "")?;

    let version = integer(&table, "version", "version")?.ok_or_else(|| missing("version"))?;
    if version != GRAMMAR_VERSION {
        return Err(invalid("version", version, "this keeper reads version 1."));
    }
    let raw_id = required_text(&table, "id", "id")?;
    let id = Ulid::from_string(&raw_id)
        .map_err(|_| invalid("id", format!("\"{raw_id}\""), "a session id is a ULID."))?;
    let agent = id_field(&table, "agent")?;
    let drive = id_field(&table, "drive")?;
    let kind_word = required_text(&table, "kind", "kind")?;
    let kind = kind_word.parse().map_err(|()| {
        invalid(
            "kind",
            format!("\"{kind_word}\""),
            "it is one of main, conversation, delegated, scheduled, workflow, gate.",
        )
    })?;
    let title = required_text(&table, "title", "title")?;
    let chars = title.chars().count();
    if chars > TITLE_MAX {
        return Err(invalid(
            "title",
            format!("{chars} characters long"),
            "a session's title is at most 120 characters.",
        ));
    }
    let requested_by = user(
        "requested_by",
        &required_text(&table, "requested_by", "requested_by")?,
    )?;
    let room = room("room", &required_text(&table, "room", "room")?)?;
    let drives = match text_list(&table, "drives")? {
        None => vec![drive.clone()],
        Some(drives) => {
            if let Some(bad) = drives.iter().find(|d| !fits_id(d)) {
                return Err(invalid(
                    "drives",
                    format!("\"{bad}\""),
                    "each is a drive id.",
                ));
            }
            drives
        }
    };
    let dispatch_chain = text_list(&table, "dispatch_chain")?
        .unwrap_or_default()
        .iter()
        .map(|raw| user("dispatch_chain", raw))
        .collect::<Result<Vec<_>, _>>()?;
    let hop = match integer(&table, "hop", "hop")? {
        None => 0,
        Some(hop) if (0..=HOP_MAX).contains(&hop) => hop as u8,
        Some(hop) => {
            return Err(invalid(
                "hop",
                hop,
                "a delegation is at most 3 hops deep (0–3).",
            ))
        }
    };
    let created_raw = required_text(&table, "created_at", "created_at")?;
    let created_at = DateTime::parse_from_rfc3339(&created_raw)
        .map_err(|_| {
            invalid(
                "created_at",
                format!("\"{created_raw}\""),
                "it is an RFC 3339 time.",
            )
        })?
        .with_timezone(&Utc);

    Ok(SessionAgent {
        id,
        agent,
        drive,
        kind,
        title,
        requested_by,
        parent: parent(&table)?,
        room,
        drives,
        label: label(&table)?,
        needs: text_list(&table, "needs")?,
        pin: text(&table, "pin", "pin")?,
        hop,
        dispatch_chain,
        limits: limits(&table)?,
        workflow: text(&table, "workflow", "workflow")?,
        created_at,
    })
}

fn quoted(value: &str) -> String {
    toml::Value::String(value.to_owned()).to_string()
}

fn quoted_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<String> = items.into_iter().map(quoted).collect();
    format!("[{}]", items.join(", "))
}

/// Write a session `agent.toml`: every key the session has, top-level keys
/// first in the documented order, then `[parent]`, `[label]` and `[limits]`.
pub fn compose_session_agent_toml(session: &SessionAgent) -> String {
    let mut out = String::new();
    let mut line = |key: &str, value: String| {
        out.push_str(key);
        out.push_str(" = ");
        out.push_str(&value);
        out.push('\n');
    };
    line("version", GRAMMAR_VERSION.to_string());
    line("id", quoted(&session.id.to_string()));
    line("agent", quoted(&session.agent));
    line("drive", quoted(&session.drive));
    line("kind", quoted(session.kind.as_str()));
    line("title", quoted(&session.title));
    line("requested_by", quoted(session.requested_by.as_str()));
    line("room", quoted(session.room.as_str()));
    line(
        "drives",
        quoted_list(session.drives.iter().map(String::as_str)),
    );
    if let Some(needs) = &session.needs {
        line("needs", quoted_list(needs.iter().map(String::as_str)));
    }
    if let Some(pin) = &session.pin {
        line("pin", quoted(pin));
    }
    line("hop", session.hop.to_string());
    if !session.dispatch_chain.is_empty() {
        line(
            "dispatch_chain",
            quoted_list(session.dispatch_chain.iter().map(|user| user.as_str())),
        );
    }
    if let Some(workflow) = &session.workflow {
        line("workflow", quoted(workflow));
    }
    line(
        "created_at",
        quoted(
            &session
                .created_at
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        ),
    );
    if let Some(parent) = &session.parent {
        out.push_str("\n[parent]\n");
        out.push_str(&format!("drive = {}\n", quoted(&parent.drive)));
        out.push_str(&format!("session = {}\n", quoted(&parent.session)));
        out.push_str(&format!("room = {}\n", quoted(parent.room.as_str())));
    }
    out.push_str("\n[label]\n");
    let readers = match &session.label.readers {
        Readers::Anyone => quoted("*"),
        Readers::Only(set) => quoted_list(set.iter().map(|user| user.as_str())),
    };
    out.push_str(&format!("readers = {readers}\n"));
    out.push_str(&format!(
        "integrity = {}\n",
        quoted(session.label.integrity.as_word())
    ));
    if session.label.local_only {
        out.push_str("local_only = true\n");
    }
    if let Some(limits) = &session.limits {
        out.push_str("\n[limits]\n");
        out.push_str(&format!(
            "rounds_per_exchange = {}\ntokens = {}\n",
            limits.rounds_per_exchange, limits.tokens
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = r#"version = 1
id = "01J9Z3K4M5N6P7Q8R9S0T1V2W3"
agent = "amelia"
drive = "tgdrive"
kind = "delegated"
title = "Release notes for 0.9"
requested_by = "@tola-grey:h"
room = "!sess:h"
drives = ["tgdrive"]
needs = ["git"]
pin = ""
hop = 1
dispatch_chain = ["@tgorka:h", "@nixi:h", "@tola-grey:h"]
workflow = "release-notes"
created_at = "2026-09-30T08:15:03.120Z"

[parent]
drive = "tgdrive"
session = "active/2026-09-30-triage"
room = "!parent:h"

[label]
readers = ["@tgorka:h"]
integrity = "peer"

[limits]
rounds_per_exchange = 8
tokens = 200000
"#;

    fn with(line: &str, replacement: &str) -> String {
        assert!(GOOD.contains(line), "{line}");
        GOOD.replacen(line, replacement, 1)
    }

    fn refused_key(text: &str) -> String {
        match parse_session_agent_toml(text) {
            Ok(session) => panic!("accepted: {session:?}"),
            Err(refusal) => {
                let key = refusal.key().unwrap_or("").to_owned();
                assert!(refusal.sentence().contains(&format!("`{key}`")));
                key
            }
        }
    }

    #[test]
    fn the_session_file_parses_and_round_trips() {
        let session = parse_session_agent_toml(GOOD).expect("parse");
        assert_eq!(session.kind, SessionKind::Delegated);
        assert_eq!(session.hop, 1);
        assert_eq!(session.label.integrity, Integrity::Peer);
        assert_eq!(session.dispatch_chain.len(), 3);
        assert_eq!(session.dispatch_chain[0].as_str(), "@tgorka:h");
        let composed = compose_session_agent_toml(&session);
        assert_eq!(
            parse_session_agent_toml(&composed).expect("reparse"),
            session
        );
        assert_eq!(composed, GOOD, "compose writes the documented layout");

        let minimal = r#"version = 1
id = "01J9Z3K4M5N6P7Q8R9S0T1V2W3"
agent = "nixi"
drive = "neuradrive"
kind = "main"
title = "DM"
requested_by = "@marta:h"
room = "!dm:h"
created_at = "2026-09-30T08:15:03Z"

[label]
readers = "*"
integrity = "owner"
local_only = true
"#;
        let session = parse_session_agent_toml(minimal).expect("parse");
        assert_eq!(session.drives, vec!["neuradrive".to_owned()]);
        assert_eq!(session.hop, 0);
        assert_eq!(session.needs, None);
        assert!(session.dispatch_chain.is_empty(), "a file before R76");
        assert!(session.label.local_only);
        let again = parse_session_agent_toml(&compose_session_agent_toml(&session)).expect("again");
        assert_eq!(again, session);
        // R25: a proxy conversation the person started is its own kind.
        let conversation = parse_session_agent_toml(
            &minimal.replace("kind = \"main\"", "kind = \"conversation\""),
        )
        .expect("conversation");
        assert_eq!(conversation.kind, SessionKind::Conversation);
        assert!(compose_session_agent_toml(&conversation).contains("kind = \"conversation\"\n"));
    }

    #[test]
    fn every_bound_is_refused_one_past_its_edge_naming_the_key() {
        let at_edge = "x".repeat(TITLE_MAX);
        assert!(parse_session_agent_toml(&with(
            "title = \"Release notes for 0.9\"",
            &format!("title = \"{at_edge}\"")
        ))
        .is_ok());
        let past = "x".repeat(TITLE_MAX + 1);
        assert_eq!(
            refused_key(&with(
                "title = \"Release notes for 0.9\"",
                &format!("title = \"{past}\"")
            )),
            "title"
        );
        assert!(parse_session_agent_toml(&with("hop = 1", "hop = 3")).is_ok());
        assert_eq!(refused_key(&with("hop = 1", "hop = 4")), "hop");
        assert_eq!(refused_key(&with("hop = 1", "hop = -1")), "hop");
        assert_eq!(
            refused_key(&with("kind = \"delegated\"", "kind = \"chat\"")),
            "kind"
        );
        assert_eq!(
            refused_key(&with("room = \"!sess:h\"", "room = \"#alias:h\"")),
            "room"
        );
        assert_eq!(
            refused_key(&with("room = \"!parent:h\"", "room = \"sess\"")),
            "parent.room"
        );
        assert_eq!(
            refused_key(&with("session = \"active/2026-09-30-triage\"\n", "")),
            "parent.session"
        );
        assert_eq!(
            refused_key(&with(
                "room = \"!parent:h\"",
                "room = \"!parent:h\"\nextra = 1"
            )),
            "parent.extra"
        );
        assert_eq!(
            refused_key(&with(
                "requested_by = \"@tola-grey:h\"",
                "requested_by = \"tola\""
            )),
            "requested_by"
        );
        assert_eq!(
            refused_key(&with("integrity = \"peer\"", "integrity = \"boss\"")),
            "label.integrity"
        );
        assert_eq!(
            refused_key(&with("id = \"01J9Z3K4M5N6P7Q8R9S0T1V2W3\"", "id = \"one\"")),
            "id"
        );
        assert_eq!(refused_key(&with("version = 1", "version = 2")), "version");
        assert_eq!(
            refused_key(&with("\"@nixi:h\", \"@tola", "\"nixi\", \"@tola")),
            "dispatch_chain"
        );
        assert_eq!(
            refused_key(&with("rounds_per_exchange = 8", "rounds_per_exchange = 0")),
            "limits.rounds_per_exchange"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_by_name() {
        assert_eq!(
            refused_key(&with("hop = 1", "hop = 1\nmood = \"cheerful\"")),
            "mood"
        );
        assert_eq!(
            refused_key(&with(
                "integrity = \"peer\"",
                "integrity = \"peer\"\nwhy = \"x\""
            )),
            "label.why"
        );
        assert_eq!(refused_key(&with("version = 1\n", "")), "version");
    }
}
