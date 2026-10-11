//! The approval record (AD-393, FR-796): an action that waits for a person,
//! written once as `approvals/<ulid>.json` and bound by a digest to exactly
//! what will run, and the decision a host writes beside it as
//! `approvals/<ulid>.decision.json`.
//!
//! Pure: the host composes, writes and reads the files; this module decides
//! their shape, the canonical form the digest is taken over, keeper's own
//! one-sentence summary, which scopes a person may give, when a record
//! expires and whether a decision counts against it.
//!
//! **Canonical JSON** (rulings R25, R29 F11) is the RFC 8785 subset these
//! records need: object members sorted by their keys' UTF-16 code units, no
//! whitespace, strings escaped as RFC 8785 §3.2.2.2 says, and integers only —
//! an `i64` or a `u64` written as serde_json writes it. A float anywhere in
//! the digested object refuses the record, naming its JSON path, so two
//! hosts never compute two digests for one record. ruma's canonical JSON is
//! Matrix's flavour (±2^53, code-point order) and is not this one.

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::agents::label::Label;
use crate::agents::session::SessionKind;
use crate::agents::tier::{AgentTool, Classification, Raise, Tier};

/// The version of both files this keeper writes and reads.
pub const RECORD_VERSION: u32 = 1;

/// A record's arguments whose canonical bytes are longer than this go to
/// `approvals/blobs/<sha256>.json`, the log's blob discipline (R86); the
/// Matrix request carries them as an encrypted file.
pub const ARGS_INLINE_MAX: usize = 16 * 1024;

/// The sentence for a record or a decision from a newer keeper.
pub const NEWER_RECORD: &str =
    "This approval was written by a newer keeper. Update keeper to read it.";

/// What a person may give: this one call, or calls like it for the rest of
/// the session (at T2, outside a `main` session, at most 24 hours). There is
/// never an "always": a durable rule is a grant edit in Settings (AD-158).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ApprovalScope")]
pub enum Scope {
    Once,
    Session,
}

impl Scope {
    pub fn as_word(self) -> &'static str {
        match self {
            Scope::Once => "once",
            Scope::Session => "session",
        }
    }
}

/// The scopes a record at `tier` offers in a session of `kind` (R28 S-11):
/// in a `main` session — a proxy's DM, which lives for years — only `once`;
/// elsewhere `once` and `session` at T2 and `once` from T3.
pub fn scopes(tier: Tier, kind: SessionKind) -> Vec<Scope> {
    if kind != SessionKind::Main && tier <= Tier::T2 {
        vec![Scope::Once, Scope::Session]
    } else {
        vec![Scope::Once]
    }
}

/// When a `session` allowance decided at `decided_at` ends: 24 hours later,
/// or when the session closes, whichever is sooner.
pub fn session_scope_ends(
    decided_at: DateTime<Utc>,
    session_closed_at: Option<DateTime<Utc>>,
) -> DateTime<Utc> {
    let day = decided_at + Duration::hours(24);
    session_closed_at.map_or(day, |closed| closed.min(day))
}

/// What approving `tool` with `args` for the session grants (R78), in a
/// person's words: the same tool, in the same drive, on any path under
/// the approved path's folder, until the session closes and for at most
/// 24 hours after the decision. A `run`'s is its program and `cwd` (R146).
pub fn session_reach(tool: &str, args: &Value) -> String {
    if tool == AgentTool::Run.as_wire() {
        return crate::agents::run::session_reach(args);
    }
    let drive = args["profile"]
        .as_str()
        .or_else(|| args["drive"].as_str())
        .unwrap_or("this session's files");
    let path = args["path"].as_str().unwrap_or("");
    let folder = match path.trim_end_matches('/').rsplit_once('/') {
        Some((folder, _)) if !folder.is_empty() => format!("`{folder}/`"),
        _ => "the top folder".to_owned(),
    };
    format!(
        "Also lets this session run `{tool}` again in {drive}, on anything in {folder}, without asking, until the session closes and for at most 24 hours."
    )
}

/// How long a record waits for its decision: an hour at T4, a day below.
pub fn expires_after(tier: Tier) -> Duration {
    if tier >= Tier::T4 {
        Duration::hours(1)
    } else {
        Duration::hours(24)
    }
}

/// A time as both files write it: RFC 3339, UTC, milliseconds.
pub fn stamp(at: DateTime<Utc>) -> String {
    at.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// A float in a digested object, by its JSON path (`args.timeout`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path} is a number with a fraction or an exponent; an approval binds integers only, so this action cannot wait for a person")]
pub struct FloatAt {
    pub path: String,
}

/// `value` in canonical JSON, or the path of the first float in it.
pub fn canonical(value: &Value) -> Result<String, FloatAt> {
    let mut out = String::new();
    write_canonical(value, "", &mut out)?;
    Ok(out)
}

fn child(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.to_owned()
    } else {
        format!("{path}.{key}")
    }
}

fn write_canonical(value: &Value, path: &str, out: &mut String) -> Result<(), FloatAt> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => match (number.as_i64(), number.as_u64()) {
            (Some(n), _) => out.push_str(&n.to_string()),
            (None, Some(n)) => out.push_str(&n.to_string()),
            (None, None) => {
                return Err(FloatAt {
                    path: path.to_owned(),
                })
            }
        },
        Value::String(text) => write_string(text, out),
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_canonical(item, &format!("{path}[{at}]"), out)?;
            }
            out.push(']');
        }
        Value::Object(members) => {
            let mut keys: Vec<(Vec<u16>, &String)> = members
                .keys()
                .map(|key| (key.encode_utf16().collect(), key))
                .collect();
            keys.sort();
            out.push('{');
            for (at, (_, key)) in keys.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_canonical(&members[key], &child(path, key), out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// RFC 8785 §3.2.2.2: `"` and `\` escaped, the five short forms, every
/// other control character as `\u00xx`, everything else literal.
fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// `sha256:` and the hex SHA-256 of the canonical JSON of
/// `{id, session, agent, tool, args, exec_binding, checkpoint_sha256,
/// preconditions}` (R28 S-24: the record's own id, session and agent are
/// bound, so a decision never pairs with another record of the same action).
#[allow(clippy::too_many_arguments)]
pub fn binding_digest(
    id: &str,
    session: &str,
    agent: &str,
    tool: &str,
    args: &Value,
    exec_binding: &Value,
    checkpoint_sha256: &str,
    preconditions: &Value,
) -> Result<String, FloatAt> {
    let bound = json!({
        "id": id,
        "session": session,
        "agent": agent,
        "tool": tool,
        "args": args,
        "exec_binding": exec_binding,
        "checkpoint_sha256": checkpoint_sha256,
        "preconditions": preconditions,
    });
    Ok(format!(
        "sha256:{}",
        sha256_hex(canonical(&bound)?.as_bytes())
    ))
}

fn arg<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().unwrap_or("")
}

fn bytes_of(args: &Value, key: &str) -> usize {
    args[key].as_str().map_or(0, str::len)
}

/// keeper's one sentence for a call of `tool` with `args` (R28 S-10): a
/// template per tool over the parsed arguments, never words the model wrote
/// in its message. A `run`'s reads its `exec_binding` too, which keeper
/// wrote and the digest binds: whether it runs code the session holds.
pub fn summary_of(tool: AgentTool, args: &Value, exec_binding: &Value) -> String {
    let (drive, path) = (arg(args, "profile"), arg(args, "path"));
    match tool {
        AgentTool::DriveList => format!("List `{path}` in {drive}"),
        AgentTool::DriveRead => format!("Read `{path}` in {drive}"),
        AgentTool::DriveGlob => {
            format!("Find files matching `{}` in {drive}", arg(args, "pattern"))
        }
        AgentTool::DriveGrep => format!("Search `{path}` in {drive} for `{}`", arg(args, "needle")),
        AgentTool::DriveStat => format!("Look up `{path}` in {drive}"),
        AgentTool::DriveSearch => {
            let drives: Vec<&str> = args["drives"]
                .as_array()
                .map(|drives| drives.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let scope = if drives.is_empty() {
                "the drives in scope".to_owned()
            } else {
                drives.join(", ")
            };
            format!("Search {scope} for `{}`", arg(args, "query"))
        }
        AgentTool::DriveWrite => format!(
            "Write `{path}` in {drive} ({} bytes)",
            bytes_of(args, "content")
        ),
        AgentTool::DriveEdit => format!(
            "Edit `{path}` in {drive}, replacing {} bytes with {} bytes",
            bytes_of(args, "old_text"),
            bytes_of(args, "new_text")
        ),
        AgentTool::SessionWrite => format!(
            "Write `{path}` in this session ({} bytes)",
            bytes_of(args, "content")
        ),
        AgentTool::CardUpdate => {
            let keys: Vec<&str> = args["fields"]
                .as_object()
                .map(|fields| fields.keys().map(String::as_str).collect())
                .unwrap_or_default();
            format!(
                "Change {} on the card `{}`",
                keys.join(", "),
                arg(args, "card")
            )
        }
        AgentTool::Delegate => match args["session"].as_str() {
            Some(session) => format!("Send the next round to the session {session}"),
            None => format!("Hand work to {}", arg(args, "agent")),
        },
        AgentTool::Reply => match args["ask"].as_str() {
            Some(ask) => format!("Relay your person's answer to the question {ask}"),
            None => "Reply to the session that handed this work on".to_owned(),
        },
        AgentTool::AskHuman => {
            "Ask the person this work is for a question, through their proxy".to_owned()
        }
        AgentTool::WorkflowStart => format!(
            "Start the workflow {} in a session of its own",
            arg(args, "name")
        ),
        AgentTool::SurfaceOpen => format!("Open `{path}` in {} on your screen", arg(args, "drive")),
        AgentTool::SurfaceHighlight => {
            format!(
                "Highlight a passage of `{path}` in {} on your screen",
                arg(args, "drive")
            )
        }
        AgentTool::SurfacePoint => {
            format!(
                "Point at a place in `{path}` in {} on your screen",
                arg(args, "drive")
            )
        }
        AgentTool::SurfaceScroll => {
            format!("Scroll `{path}` in {} on your screen", arg(args, "drive"))
        }
        AgentTool::SurfaceProposeEdit => {
            format!(
                "Propose an edit to `{path}` in {} on your screen",
                arg(args, "drive")
            )
        }
        AgentTool::BmadConfig => match args["skill"].as_str() {
            Some(skill) => format!("Read the BMAD customization of the skill {skill}"),
            None if arg(args, "scope") == "customization" => {
                "Read the BMAD customization of this session's workflow".to_owned()
            }
            None => "Read BMAD's central configuration".to_owned(),
        },
        AgentTool::BmadRender => match args["skill"].as_str() {
            Some(skill) => format!("Render the BMAD skill {skill} into this session"),
            None => "Render this session's workflow into this session".to_owned(),
        },
        AgentTool::BmadMemlog => {
            let memlog = args["path"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| {
                    args["workspace"]
                        .as_str()
                        .map(|run| format!("{run}/.memlog.md"))
                })
                .unwrap_or_default();
            match args["command"].as_str() {
                Some("init") => format!("Start the memlog `{memlog}`"),
                Some("set") => format!("Set `{}` in the memlog `{memlog}`", arg(args, "key")),
                _ => format!("Add an entry to the memlog `{memlog}`"),
            }
        }
        AgentTool::BmadParty => "Read BMAD's party roster".to_owned(),
        AgentTool::SkillsList => "List the skills offered to this agent".to_owned(),
        AgentTool::SkillView => match args["path"].as_str() {
            Some(file) => format!("Read `{file}` of the skill {}", arg(args, "name")),
            None => format!("Read the skill {}", arg(args, "name")),
        },
        AgentTool::Helper => match args["lens"].as_str() {
            Some(lens) => format!("Run the review layer {lens} as a read-only helper"),
            None => "Hand one piece of work to a read-only helper".to_owned(),
        },
        AgentTool::JournalAppend => format!(
            "Add an entry to this agent's journal ({} bytes)",
            bytes_of(args, "text")
        ),
        AgentTool::MemoryPropose => {
            let file = match arg(args, "target") {
                "user" => "USER.md",
                _ => "MEMORY.md",
            };
            format!("Propose to {} an entry of {file}", arg(args, "op"))
        }
        AgentTool::SkillPropose => format!(
            "Propose to {} the skill {}",
            arg(args, "op"),
            arg(args, "name")
        ),
        AgentTool::Declassify => {
            let readers: Vec<&str> = args["readers"]
                .as_array()
                .map(|readers| readers.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let sha = arg(args, "sha256");
            format!(
                "Let {} read {} ({})",
                readers.join(", "),
                arg(args, "what"),
                sha.get(..12).unwrap_or(sha)
            )
        }
        AgentTool::MemoryApply | AgentTool::SkillApply => {
            let proposals = args["proposals"].as_array().map_or(0, Vec::len);
            format!(
                "Change {}'s {} as {proposals} proposal(s) ask",
                arg(args, "agent"),
                args["change"]["path"].as_str().unwrap_or("")
            )
        }
        AgentTool::Run => crate::agents::run::summary(args, exec_binding),
    }
}

/// The call a record waits for: its `tool_call` line and wire id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRef {
    pub line: String,
    pub call_id: String,
}

/// Where the session's log stood when the call parked: the chunk, its last
/// line, and the SHA-256 of the chunk's bytes through that line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    /// Session-relative: `log/2026-10-05.electra.1.jsonl`.
    pub chunk: String,
    pub through: String,
    pub sha256: String,
}

/// The action, exactly as it will run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub tool: String,
    /// The call's arguments; `null` when they are in [`Self::args_blob`].
    pub args: Value,
    /// The SHA-256 of `approvals/blobs/<sha256>.json`, the canonical
    /// arguments, when they are over [`ARGS_INLINE_MAX`] (R86).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args_blob: Option<String>,
    /// A `run`'s argv, cwd, env, executable and operand hashes; `null` for
    /// every other tool (R79).
    pub exec_binding: Value,
    pub summary: String,
    pub preview: Option<Value>,
}

/// How risky the action is, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    pub tier: u8,
    pub base_tier: u8,
    pub raised_by: Vec<String>,
    pub categories: Vec<String>,
    pub reversible: bool,
    pub taint: Vec<String>,
    pub rules: Vec<String>,
}

/// One file the action relied on, by drive and drive-relative path as the
/// call named it; where that resolved on the disk then — the canonical
/// drive-relative landing, through every link — and that file's SHA-256,
/// `None` when it did not exist (R79, R174). An alias retargeted between
/// two files of equal bytes changes the landing, so it is drift.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilePin {
    pub drive: String,
    pub path: String,
    /// `None` when keeper-sync refused to resolve it.
    #[serde(default)]
    pub landing: Option<String>,
    pub sha256: Option<String>,
}

/// What must still hold when the approval is consumed. No claim epoch is
/// here or anywhere digested (R25): holding the claim is checked, not bound.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preconditions {
    pub files: Vec<FilePin>,
    pub workspace: Option<Value>,
    pub screen: Option<Value>,
    /// How old the facts above may be when the approval is used; `None`
    /// when the hashes are the whole check.
    pub max_staleness_s: Option<u64>,
}

/// `approvals/<ulid>.json`, written once and never edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalRecord {
    pub v: u32,
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
    /// Drive-relative: `60-sessions/active/2026-10-05-chat`.
    pub session: String,
    pub agent: String,
    pub drive: String,
    /// The host and claim epoch that wrote it: informational, not digested.
    pub host: String,
    pub epoch: u64,
    pub call: CallRef,
    pub dispatch_chain: Vec<String>,
    pub checkpoint: Checkpoint,
    pub action: Action,
    pub risk: Risk,
    pub label: Label,
    pub preconditions: Preconditions,
    pub scopes: Vec<Scope>,
    pub binding_digest: String,
    pub matrix_event: Option<String>,
}

/// Everything a host knows when a call parks; [`ApprovalRecord::new`]
/// derives the rest.
#[derive(Debug, Clone)]
pub struct Parking<'a> {
    pub id: &'a str,
    pub created_at: DateTime<Utc>,
    pub session: &'a str,
    pub session_kind: SessionKind,
    pub agent: &'a str,
    pub drive: &'a str,
    pub host: &'a str,
    pub epoch: u64,
    pub call: CallRef,
    pub dispatch_chain: Vec<String>,
    pub checkpoint: Checkpoint,
    pub args: &'a Value,
    /// What a `run` binds beyond its arguments; `null` for every other
    /// tool.
    pub exec_binding: Value,
    pub classification: &'a Classification,
    pub label: &'a Label,
    pub preconditions: Preconditions,
}

impl ApprovalRecord {
    /// The record a host writes for `parking`: the summary from
    /// [`summary_of`] (it takes none), the scopes for its tier and session
    /// kind, its expiry and its digest. A float in the digested fields
    /// refuses it, named.
    pub fn new(parking: Parking<'_>) -> Result<ApprovalRecord, FloatAt> {
        let classification = parking.classification;
        let tool = classification.tool.as_wire();
        let exec_binding = parking.exec_binding;
        let preconditions = serde_json::to_value(&parking.preconditions).unwrap_or(Value::Null);
        let binding_digest = binding_digest(
            parking.id,
            parking.session,
            parking.agent,
            tool,
            parking.args,
            &exec_binding,
            &parking.checkpoint.sha256,
            &preconditions,
        )?;
        let words = |raises: &[Raise]| raises.iter().map(|r| r.as_word().to_owned()).collect();
        let untrusted = classification.raised_by.contains(&Raise::Untrusted);
        Ok(ApprovalRecord {
            v: RECORD_VERSION,
            id: parking.id.to_owned(),
            created_at: stamp(parking.created_at),
            expires_at: stamp(parking.created_at + expires_after(classification.tier)),
            session: parking.session.to_owned(),
            agent: parking.agent.to_owned(),
            drive: parking.drive.to_owned(),
            host: parking.host.to_owned(),
            epoch: parking.epoch,
            call: parking.call,
            dispatch_chain: parking.dispatch_chain,
            checkpoint: parking.checkpoint,
            action: Action {
                tool: tool.to_owned(),
                args: parking.args.clone(),
                args_blob: None,
                summary: summary_of(classification.tool, parking.args, &exec_binding),
                exec_binding,
                preview: None,
            },
            risk: Risk {
                tier: classification.tier.as_u8(),
                base_tier: classification.base_tier.as_u8(),
                raised_by: words(&classification.raised_by),
                categories: Vec::new(),
                reversible: classification.tier < Tier::T4,
                taint: if untrusted {
                    vec!["untrusted".to_owned()]
                } else {
                    Vec::new()
                },
                rules: Vec::new(),
            },
            label: parking.label.clone(),
            preconditions: parking.preconditions,
            scopes: scopes(classification.tier, parking.session_kind),
            binding_digest,
            matrix_event: None,
        })
    }

    /// Move a payload whose canonical bytes are over [`ARGS_INLINE_MAX`] out
    /// of the record: the blob's SHA-256 and bytes, for the host to write
    /// before the record. A call's payload is its arguments; a `run`'s is
    /// everything its digest is over beyond the log — the arguments, the
    /// `exec_binding` and the workspace set it releases — attached whole
    /// ([`attached_run`], R213), so no part of it rides inline in a Matrix
    /// event. The digest is over the same values either way.
    pub fn externalise_args(&mut self) -> Option<(String, String)> {
        let run = self.action.tool == AgentTool::Run.as_wire();
        let payload = if run {
            json!({
                "args": self.action.args,
                "exec_binding": self.action.exec_binding,
                "workspace": self.preconditions.workspace,
            })
        } else {
            self.action.args.clone()
        };
        let bytes = canonical(&payload).ok()?;
        if bytes.len() <= ARGS_INLINE_MAX {
            return None;
        }
        let sha = sha256_hex(bytes.as_bytes());
        self.action.args = Value::Null;
        if run {
            self.action.exec_binding = Value::Null;
            self.preconditions.workspace = None;
        }
        self.action.args_blob = Some(sha.clone());
        Some((sha, bytes))
    }

    /// The arguments: inline, or from `blob` (the text of
    /// `approvals/blobs/<sha256>.json`) when its SHA-256 is the record's.
    pub fn args(&self, blob: Option<&str>) -> Option<Value> {
        self.whole(blob).map(|record| record.action.args)
    }

    /// The record as its digest is over it: inline, or with what
    /// [`Self::externalise_args`] moved out read back from `blob` when its
    /// SHA-256 is the record's.
    pub fn whole(&self, blob: Option<&str>) -> Option<ApprovalRecord> {
        let mut record = self.clone();
        match (&self.action.args_blob, blob) {
            (None, _) => {}
            (Some(sha), Some(text)) if sha256_hex(text.as_bytes()) == *sha => {
                let value: Value = serde_json::from_str(text).ok()?;
                if self.action.tool == AgentTool::Run.as_wire() {
                    let (args, exec_binding, workspace) = attached_run(value)?;
                    record.action.args = args;
                    record.action.exec_binding = exec_binding;
                    record.preconditions.workspace = workspace;
                } else {
                    record.action.args = value;
                }
            }
            (Some(_), _) => return None,
        }
        Some(record)
    }

    /// The digest of this record recomputed over `args` — what a resume
    /// compares with the decision's, so a record edited on disk is drift.
    pub fn recomputed_digest(&self, args: &Value) -> Result<String, FloatAt> {
        binding_digest(
            &self.id,
            &self.session,
            &self.agent,
            &self.action.tool,
            args,
            &self.action.exec_binding,
            &self.checkpoint.sha256,
            &serde_json::to_value(&self.preconditions).unwrap_or(Value::Null),
        )
    }

    /// When it expires, read back; a record whose time does not read has
    /// expired.
    pub fn expires(&self) -> DateTime<Utc> {
        read_time(&self.expires_at).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    /// Whether `max_staleness_s` has passed since it was written at `now`.
    pub fn stale(&self, now: DateTime<Utc>) -> bool {
        let Some(limit) = self.preconditions.max_staleness_s else {
            return false;
        };
        let Some(created) = read_time(&self.created_at) else {
            return true;
        };
        now - created > Duration::seconds(i64::try_from(limit).unwrap_or(i64::MAX))
    }

    /// The classification the record was made from, as its `risk` keeps
    /// it: what a host that took over writes the call's one audit row with
    /// (R172). `None` for a tool or a tier this build does not know.
    pub fn classification(&self) -> Option<Classification> {
        Some(Classification {
            tool: AgentTool::from_wire(&self.action.tool)?,
            tier: Tier::from_u8(self.risk.tier)?,
            base_tier: Tier::from_u8(self.risk.base_tier)?,
            raised_by: self
                .risk
                .raised_by
                .iter()
                .map(|word| Raise::from_word(word))
                .collect::<Option<Vec<Raise>>>()?,
        })
    }
}

/// A `run`'s attached payload read back: its arguments, its
/// `exec_binding` and the workspace set (`None` without network); `None`
/// when it is not one.
pub fn attached_run(payload: Value) -> Option<(Value, Value, Option<Value>)> {
    let Value::Object(mut fields) = payload else {
        return None;
    };
    if fields.len() != 3 {
        return None;
    }
    let args = fields.remove("args")?;
    let exec_binding = fields.remove("exec_binding")?;
    let workspace = fields.remove("workspace")?;
    Some((
        args,
        exec_binding,
        (!workspace.is_null()).then_some(workspace),
    ))
}

fn read_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|at| at.with_timezone(&Utc))
}

/// Why a file under `approvals/` is not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordRefusal {
    #[error("{NEWER_RECORD}")]
    Newer,
    #[error("keeper cannot read this approval: {0}")]
    Unreadable(String),
}

fn parse_strict<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, RecordRefusal> {
    let value: Value =
        serde_json::from_str(text).map_err(|e| RecordRefusal::Unreadable(e.to_string()))?;
    if value["v"]
        .as_u64()
        .is_some_and(|v| v > u64::from(RECORD_VERSION))
    {
        return Err(RecordRefusal::Newer);
    }
    let parsed: T =
        serde_json::from_value(value).map_err(|e| RecordRefusal::Unreadable(e.to_string()))?;
    Ok(parsed)
}

/// Read an approval record strictly: an unknown key — `claim_epoch`
/// included — is refused, named, and a `v` above 1 is a newer keeper's.
pub fn parse_record(text: &str) -> Result<ApprovalRecord, RecordRefusal> {
    let record: ApprovalRecord = parse_strict(text)?;
    if record.v != RECORD_VERSION {
        return Err(RecordRefusal::Unreadable(format!("v is {}", record.v)));
    }
    Ok(record)
}

/// A person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, rename = "ApprovalDecision")]
pub enum Decision {
    Approve,
    Deny,
}

impl Decision {
    pub fn as_word(self) -> &'static str {
        match self {
            Decision::Approve => "approve",
            Decision::Deny => "deny",
        }
    }
}

/// Who decided, from which device, and whether the host found it verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecidedBy {
    pub user: String,
    pub device: String,
    pub verified: bool,
}

/// The host and claim epoch that wrote a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WrittenBy {
    pub host: String,
    pub epoch: u64,
}

/// `approvals/<ulid>.decision.json`, written once by the host holding the
/// session's claim when a person's decision counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRecord {
    pub v: u32,
    pub id: String,
    pub decision: Decision,
    pub scope: Scope,
    pub note: Option<String>,
    pub binding_digest: String,
    pub decided_by: DecidedBy,
    pub decided_at: String,
    pub matrix_event: Option<String>,
    pub written_by: WrittenBy,
}

/// Why a decision does not count against a record.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecisionRefusal {
    #[error("this decision is for another approval")]
    OtherRecord,
    #[error("this decision was given for something other than what would run")]
    Digest,
    #[error("this approval does not offer \"{0}\"")]
    Scope(&'static str),
    #[error("this approval had expired")]
    Expired,
}

impl DecisionRecord {
    /// Whether this decision counts against `record` at `now`: the same
    /// record, the same digest, a scope the record offers, not expired.
    /// Who decided and from which device is 93.3's to check.
    pub fn admissible(
        &self,
        record: &ApprovalRecord,
        now: DateTime<Utc>,
    ) -> Result<(), DecisionRefusal> {
        if self.id != record.id {
            return Err(DecisionRefusal::OtherRecord);
        }
        if self.binding_digest != record.binding_digest {
            return Err(DecisionRefusal::Digest);
        }
        if !record.scopes.contains(&self.scope) {
            return Err(DecisionRefusal::Scope(self.scope.as_word()));
        }
        if now >= record.expires() {
            return Err(DecisionRefusal::Expired);
        }
        Ok(())
    }
}

/// Read a decision strictly, as [`parse_record`] reads a record.
pub fn parse_decision(text: &str) -> Result<DecisionRecord, RecordRefusal> {
    let decision: DecisionRecord = parse_strict(text)?;
    if decision.v != RECORD_VERSION {
        return Err(RecordRefusal::Unreadable(format!("v is {}", decision.v)));
    }
    Ok(decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::label::{Integrity, Readers};
    use crate::agents::tier::{classify, CallFacts, Context};

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .expect("time")
            .with_timezone(&Utc)
    }

    fn classification(tool: AgentTool, tier: Tier) -> Classification {
        let mut classified = classify(
            tool,
            &CallFacts::default(),
            &Context {
                delegated: false,
                unattended: false,
                integrity: Integrity::Owner,
                via_kvm: false,
                grant: None,
            },
        );
        classified.tier = tier;
        classified.base_tier = tier;
        classified
    }

    fn label() -> Label {
        Label {
            readers: Readers::Only(
                [matrix_sdk::ruma::OwnedUserId::try_from("@tgorka:h").expect("user")].into(),
            ),
            ..Label::top()
        }
    }

    fn write_args() -> Value {
        json!({"profile": "tgdrive", "path": "10-notes/a.md", "content": "x".repeat(120)})
    }

    fn record_with(id: &str, args: &Value, tier: Tier, kind: SessionKind) -> ApprovalRecord {
        let classified = classification(AgentTool::DriveWrite, tier);
        let label = label();
        ApprovalRecord::new(Parking {
            id,
            created_at: at("2026-10-05T10:00:00Z"),
            session: "60-sessions/active/2026-10-05-chat",
            session_kind: kind,
            agent: "nixi",
            drive: "tgdrive",
            host: "electra",
            epoch: 4,
            call: CallRef {
                line: "01JLINE".to_owned(),
                call_id: "w1".to_owned(),
            },
            dispatch_chain: vec!["@tgorka:h".to_owned(), "@nixi:h".to_owned()],
            checkpoint: Checkpoint {
                chunk: "log/2026-10-05.electra.1.jsonl".to_owned(),
                through: "01JLINE".to_owned(),
                sha256: "c".repeat(64),
            },
            args,
            exec_binding: Value::Null,
            classification: &classified,
            label: &label,
            preconditions: Preconditions {
                files: vec![FilePin {
                    drive: "tgdrive".to_owned(),
                    path: "10-notes/a.md".to_owned(),
                    landing: Some("10-notes/a.md".to_owned()),
                    sha256: Some("a".repeat(64)),
                }],
                ..Preconditions::default()
            },
        })
        .expect("record")
    }

    fn record() -> ApprovalRecord {
        record_with("01JREC", &write_args(), Tier::T2, SessionKind::Conversation)
    }

    fn decision_for(record: &ApprovalRecord, scope: Scope) -> DecisionRecord {
        DecisionRecord {
            v: 1,
            id: record.id.clone(),
            decision: Decision::Approve,
            scope,
            note: None,
            binding_digest: record.binding_digest.clone(),
            decided_by: DecidedBy {
                user: "@tgorka:h".to_owned(),
                device: "KALYPSO".to_owned(),
                verified: true,
            },
            decided_at: "2026-10-05T10:05:00.000Z".to_owned(),
            matrix_event: Some("$d:h".to_owned()),
            written_by: WrittenBy {
                host: "electra".to_owned(),
                epoch: 4,
            },
        }
    }

    const KINDS: [SessionKind; 6] = [
        SessionKind::Main,
        SessionKind::Conversation,
        SessionKind::Delegated,
        SessionKind::Scheduled,
        SessionKind::Workflow,
        SessionKind::Gate,
    ];
    const TIERS: [Tier; 6] = [Tier::T0, Tier::T1, Tier::T2, Tier::T3, Tier::T4, Tier::T5];

    #[test]
    fn scopes_never_include_always() {
        // There is no word for it: a decision saying "always" does not read.
        let mut always = serde_json::to_value(decision_for(&record(), Scope::Once)).expect("json");
        always["scope"] = json!("always");
        assert!(parse_decision(&always.to_string()).is_err());
        for tier in TIERS {
            for kind in KINDS {
                let offered = scopes(tier, kind);
                assert_eq!(offered[0], Scope::Once, "{tier:?} {kind}");
                let session = offered.contains(&Scope::Session);
                let expected = kind != SessionKind::Main && tier <= Tier::T2;
                assert_eq!(session, expected, "{tier:?} {kind}");
            }
        }
        assert_eq!(
            scopes(Tier::T2, SessionKind::Conversation),
            [Scope::Once, Scope::Session]
        );
        assert_eq!(scopes(Tier::T2, SessionKind::Main), [Scope::Once]);
        assert_eq!(scopes(Tier::T3, SessionKind::Delegated), [Scope::Once]);
        assert_eq!(scopes(Tier::T4, SessionKind::Scheduled), [Scope::Once]);
    }

    #[test]
    fn a_session_scope_lapses_at_24_hours_or_close() {
        let decided = at("2026-10-05T10:00:00Z");
        assert_eq!(
            session_scope_ends(decided, None),
            at("2026-10-06T10:00:00Z")
        );
        assert_eq!(
            session_scope_ends(decided, Some(at("2026-10-05T18:00:00Z"))),
            at("2026-10-05T18:00:00Z")
        );
        assert_eq!(
            session_scope_ends(decided, Some(at("2026-10-07T00:00:00Z"))),
            at("2026-10-06T10:00:00Z")
        );
        for (tier, hours) in [(Tier::T2, 24), (Tier::T3, 24), (Tier::T4, 1)] {
            assert_eq!(expires_after(tier), Duration::hours(hours), "{tier:?}");
        }
        assert_eq!(record().expires_at, "2026-10-06T10:00:00.000Z");
        let t4 = record_with("01JT4", &write_args(), Tier::T4, SessionKind::Conversation);
        assert_eq!(t4.expires_at, "2026-10-05T11:00:00.000Z");
    }

    #[test]
    fn canonical_json_is_rfc_8785_over_integers() {
        // RFC 8785 §3.2.3's example keys: by UTF-16 code units, so the
        // non-BMP character (a surrogate pair, 0xD83D…) sorts before U+FB33
        // although its code point is higher.
        let keys = json!({
            "\u{20ac}": "Euro Sign",
            "\r": "Carriage Return",
            "\u{fb33}": "Hebrew Letter Dalet With Dagesh",
            "1": "One",
            "\u{1f600}": "Emoji: Grinning Face",
            "\u{80}": "Control",
            "\u{f6}": "Latin Small Letter O With Diaeresis",
        });
        assert_eq!(
            canonical(&keys).expect("canonical"),
            "{\"\\r\":\"Carriage Return\",\"1\":\"One\",\"\u{80}\":\"Control\",\"\u{f6}\":\"Latin Small Letter O With Diaeresis\",\"\u{20ac}\":\"Euro Sign\",\"\u{1f600}\":\"Emoji: Grinning Face\",\"\u{fb33}\":\"Hebrew Letter Dalet With Dagesh\"}"
        );
        let numbers: Value =
            serde_json::from_str(&format!("[{}, {}, 0, -1]", i64::MIN, u64::MAX)).expect("json");
        assert_eq!(
            canonical(&numbers).expect("canonical"),
            format!("[{},{},0,-1]", i64::MIN, u64::MAX)
        );
        assert_eq!(
            canonical(&json!("\u{8}\t\n\u{c}\r\u{1}\u{1f}\"\\/é")).expect("canonical"),
            "\"\\b\\t\\n\\f\\r\\u0001\\u001f\\\"\\\\/é\""
        );
        assert_eq!(
            canonical(&json!({"b": [true, null], "a": {"y": 1, "x": 2}})).expect("canonical"),
            "{\"a\":{\"x\":2,\"y\":1},\"b\":[true,null]}"
        );
        for float in ["1.5", "1e3", "1.0", "18446744073709551616"] {
            let args: Value =
                serde_json::from_str(&format!("{{\"timeout\": {float}, \"argv\": [\"a\"]}}"))
                    .expect("json");
            assert_eq!(
                binding_digest("i", "s", "a", "run", &args, &Value::Null, "c", &json!({})),
                Err(FloatAt {
                    path: "args.timeout".to_owned()
                }),
                "{float}"
            );
        }
        let nested = json!({"files": [{"size": 1.5}]});
        assert_eq!(
            canonical(&nested),
            Err(FloatAt {
                path: "files[0].size".to_owned()
            })
        );
        let classified = classification(AgentTool::DriveWrite, Tier::T2);
        let refused = ApprovalRecord::new(Parking {
            args: &json!({"profile": "tgdrive", "path": "a.md", "timeout": 1.5}),
            classification: &classified,
            ..parking_of(&record())
        });
        assert_eq!(
            refused.map(|_| ()),
            Err(FloatAt {
                path: "args.timeout".to_owned()
            })
        );
    }

    /// A [`Parking`] reproducing `record`, its arguments and classification
    /// left to the caller.
    fn parking_of(record: &ApprovalRecord) -> Parking<'static> {
        use std::sync::LazyLock;
        static LABEL: LazyLock<Label> = LazyLock::new(label);
        static CLASSIFIED: LazyLock<Classification> =
            LazyLock::new(|| classification(AgentTool::DriveWrite, Tier::T2));
        static ARGS: LazyLock<Value> = LazyLock::new(write_args);
        Parking {
            id: "01JREC",
            created_at: at("2026-10-05T10:00:00Z"),
            session: "60-sessions/active/2026-10-05-chat",
            session_kind: SessionKind::Conversation,
            agent: "nixi",
            drive: "tgdrive",
            host: "electra",
            epoch: 4,
            call: record.call.clone(),
            dispatch_chain: record.dispatch_chain.clone(),
            checkpoint: record.checkpoint.clone(),
            args: &ARGS,
            exec_binding: Value::Null,
            classification: &CLASSIFIED,
            label: &LABEL,
            preconditions: record.preconditions.clone(),
        }
    }

    #[test]
    fn the_digest_binds_exactly_what_will_run() {
        let base = record();
        let digest = |record: &ApprovalRecord, args: &Value, exec: &Value| {
            binding_digest(
                &record.id,
                &record.session,
                &record.agent,
                &record.action.tool,
                args,
                exec,
                &record.checkpoint.sha256,
                &serde_json::to_value(&record.preconditions).expect("json"),
            )
            .expect("digest")
        };
        let exec = json!({"argv": ["git", "push"], "cwd": "workspace/repo",
            "env": {"GIT_TERMINAL_PROMPT": "0"}, "exe": "/usr/bin/git",
            "exe_sha256": "e".repeat(64), "operands": [{"path": "a", "sha256": "0".repeat(64)}]});
        let of = |record: &ApprovalRecord| digest(record, &record.action.args, &exec);
        let original = of(&base);
        assert_eq!(
            base.binding_digest,
            digest(&base, &base.action.args, &Value::Null)
        );

        let mut bound: Vec<(&str, ApprovalRecord)> = Vec::new();
        let mut changed = |what: &'static str, change: &dyn Fn(&mut ApprovalRecord)| {
            let mut record = base.clone();
            change(&mut record);
            bound.push((what, record));
        };
        changed("id", &|r| r.id = "01JOTHER".to_owned());
        changed("session", &|r| r.session.push('x'));
        changed("agent", &|r| r.agent = "tola".to_owned());
        changed("tool", &|r| r.action.tool = "drive_edit".to_owned());
        changed("an argument", &|r| r.action.args["content"] = json!("y"));
        changed("checkpoint", &|r| r.checkpoint.sha256 = "d".repeat(64));
        changed("a precondition", &|r| {
            r.preconditions.files[0].sha256 = None;
        });
        changed("staleness", &|r| r.preconditions.max_staleness_s = Some(60));
        for (what, record) in &bound {
            assert_ne!(of(record), original, "{what}");
        }
        for (what, exec_changed) in [
            ("argv", json!(["git", "pull"])),
            ("cwd", json!("workspace/other")),
        ] {
            let mut moved = exec.clone();
            moved[what] = exec_changed;
            assert_ne!(digest(&base, &base.action.args, &moved), original, "{what}");
        }
        for pointer in [
            "/env/GIT_TERMINAL_PROMPT",
            "/exe_sha256",
            "/operands/0/sha256",
        ] {
            let mut moved = exec.clone();
            *moved.pointer_mut(pointer).expect("field") = json!("1");
            assert_ne!(
                digest(&base, &base.action.args, &moved),
                original,
                "{pointer}"
            );
        }

        // Member order is not bound.
        let reordered: Value = serde_json::from_str(&format!(
            "{{\"content\": {}, \"path\": \"10-notes/a.md\", \"profile\": \"tgdrive\"}}",
            json!("x".repeat(120))
        ))
        .expect("json");
        assert_eq!(digest(&base, &reordered, &exec), original);

        // Nor is what a person reads beside the action.
        let mut shown = base.clone();
        shown.action.summary = "Something else".to_owned();
        shown.risk.tier = 4;
        shown.label = Label::top();
        shown.scopes = vec![Scope::Once];
        shown.matrix_event = Some("$x:h".to_owned());
        assert_eq!(of(&shown), original);
        assert_eq!(
            shown.recomputed_digest(&shown.action.args),
            Ok(base.binding_digest.clone())
        );

        // Two records of identical action bytes: a decision for one is not
        // one for the other.
        let twin = record_with(
            "01JTWIN",
            &write_args(),
            Tier::T2,
            SessionKind::Conversation,
        );
        assert_ne!(twin.binding_digest, base.binding_digest);
        let mut crossed = decision_for(&base, Scope::Once);
        crossed.id = twin.id.clone();
        assert_eq!(
            crossed.admissible(&twin, at("2026-10-05T10:05:00Z")),
            Err(DecisionRefusal::Digest)
        );
    }

    #[test]
    fn records_are_strict() {
        let record = record();
        let text = serde_json::to_string(&record).expect("json");
        assert_eq!(parse_record(&text), Ok(record.clone()));

        let mut epoch = serde_json::to_value(&record).expect("json");
        epoch["preconditions"]["claim_epoch"] = json!(4);
        let refusal = parse_record(&epoch.to_string()).expect_err("refused");
        assert!(refusal.to_string().contains("claim_epoch"), "{refusal}");
        let mut unknown = serde_json::to_value(&record).expect("json");
        unknown["note"] = json!("hi");
        assert!(parse_record(&unknown.to_string())
            .expect_err("refused")
            .to_string()
            .contains("note"));
        let mut newer = serde_json::to_value(&record).expect("json");
        newer["v"] = json!(2);
        assert_eq!(parse_record(&newer.to_string()), Err(RecordRefusal::Newer));
        assert_eq!(RecordRefusal::Newer.to_string(), NEWER_RECORD);

        let decision = decision_for(&record, Scope::Once);
        let text = serde_json::to_string(&decision).expect("json");
        assert_eq!(parse_decision(&text), Ok(decision.clone()));
        let mut unknown = serde_json::to_value(&decision).expect("json");
        unknown["claim_epoch"] = json!(4);
        assert!(parse_decision(&unknown.to_string())
            .expect_err("refused")
            .to_string()
            .contains("claim_epoch"));
        let mut newer = serde_json::to_value(&decision).expect("json");
        newer["v"] = json!(2);
        assert_eq!(
            parse_decision(&newer.to_string()),
            Err(RecordRefusal::Newer)
        );

        let now = at("2026-10-05T10:05:00Z");
        assert_eq!(decision.admissible(&record, now), Ok(()));
        assert_eq!(
            decision_for(&record, Scope::Session).admissible(&record, now),
            Ok(())
        );
        let t3 = record_with("01JT3", &write_args(), Tier::T3, SessionKind::Conversation);
        assert_eq!(
            decision_for(&t3, Scope::Session).admissible(&t3, now),
            Err(DecisionRefusal::Scope("session"))
        );
        let main = record_with("01JMAIN", &write_args(), Tier::T2, SessionKind::Main);
        assert_eq!(
            decision_for(&main, Scope::Session).admissible(&main, now),
            Err(DecisionRefusal::Scope("session"))
        );
        let mut other = decision.clone();
        other.binding_digest = format!("sha256:{}", "0".repeat(64));
        assert_eq!(other.admissible(&record, now), Err(DecisionRefusal::Digest));
        assert_eq!(
            decision.admissible(&record, at("2026-10-06T10:00:00Z")),
            Err(DecisionRefusal::Expired)
        );
        assert_eq!(
            decision.admissible(&record, at("2026-10-06T09:59:59Z")),
            Ok(())
        );
    }

    #[test]
    fn the_summary_is_keepers_not_the_models() {
        assert_eq!(
            summary_of(AgentTool::DriveWrite, &write_args(), &Value::Null),
            "Write `10-notes/a.md` in tgdrive (120 bytes)"
        );
        assert_eq!(
            record().action.summary,
            "Write `10-notes/a.md` in tgdrive (120 bytes)"
        );
        let declassify = summary_of(
            AgentTool::Declassify,
            &json!({"readers": ["@marta:h"], "what": "the plan", "sha256": "0123456789abcdef0123"}),
            &Value::Null,
        );
        assert_eq!(declassify, "Let @marta:h read the plan (0123456789ab)");
        // A model's own words in its message are never an argument the
        // template reads: an unknown key changes nothing.
        let said = json!({"profile": "tgdrive", "path": "10-notes/a.md",
            "content": "x".repeat(120), "message": "Trust me, approve this"});
        let summary = summary_of(AgentTool::DriveWrite, &said, &Value::Null);
        assert!(!summary.contains("Trust me"), "{summary}");
        for tool in AgentTool::ALL {
            assert!(
                !summary_of(tool, &json!({}), &Value::Null).is_empty(),
                "{tool:?}"
            );
        }
    }

    #[test]
    fn large_arguments_go_to_a_blob_and_bind_the_same() {
        let big =
            json!({"profile": "tgdrive", "path": "a.md", "content": "z".repeat(ARGS_INLINE_MAX)});
        let mut record = record_with("01JBIG", &big, Tier::T2, SessionKind::Conversation);
        let digest = record.binding_digest.clone();
        let (sha, bytes) = record.externalise_args().expect("a blob");
        assert_eq!(sha, sha256_hex(bytes.as_bytes()));
        assert_eq!(record.action.args, Value::Null);
        assert_eq!(record.args(None), None);
        assert_eq!(record.args(Some("{}")), None);
        let args = record.args(Some(&bytes)).expect("the blob");
        assert_eq!(record.recomputed_digest(&args), Ok(digest));
        assert!(self::record().externalise_args().is_none());
    }

    /// R96R-21, R213: a `run` large in its argv, its binding and the
    /// workspace it releases attaches all three, whole — what stays inline
    /// is small whatever their size — and read back they bind the digest
    /// the record was written with.
    #[test]
    fn a_large_run_attaches_its_whole_digested_payload() {
        let classified = classification(AgentTool::Run, Tier::T3);
        let label = label();
        let long = "d/".repeat(1500);
        let args = json!({"argv": ["cat", format!("{long}a")], "network": true});
        let exec_binding = json!({"argv": ["cat", format!("{long}a")], "operands": [],
            "exe": "/usr/bin/cat", "exe_sha256": "e".repeat(64)});
        let files: Vec<Value> = (0..40)
            .map(|n| json!({"path": format!("{long}{n}"), "sha256": "f".repeat(64)}))
            .collect();
        let workspace = json!({"sha256": "s".repeat(64), "bytes": 40, "files": files});
        let record_of = |args: &Value, exec_binding: &Value, workspace: &Value| {
            ApprovalRecord::new(Parking {
                id: "01JRUN",
                created_at: at("2026-10-05T10:00:00Z"),
                session: "60-sessions/active/2026-10-05-chat",
                session_kind: SessionKind::Conversation,
                agent: "nixi",
                drive: "tgdrive",
                host: "electra",
                epoch: 4,
                call: CallRef {
                    line: "01JLINE".to_owned(),
                    call_id: "r1".to_owned(),
                },
                dispatch_chain: vec!["@tgorka:h".to_owned()],
                checkpoint: Checkpoint {
                    chunk: "log/2026-10-05.electra.1.jsonl".to_owned(),
                    through: "01JLINE".to_owned(),
                    sha256: "c".repeat(64),
                },
                args,
                exec_binding: exec_binding.clone(),
                classification: &classified,
                label: &label,
                preconditions: Preconditions {
                    workspace: Some(workspace.clone()),
                    ..Preconditions::default()
                },
            })
            .expect("record")
        };
        let mut record = record_of(&args, &exec_binding, &workspace);
        let digest = record.binding_digest.clone();
        // Each part alone is under the inline bound; together they are not.
        for part in [&args, &exec_binding] {
            assert!(canonical(part).expect("canonical").len() < ARGS_INLINE_MAX);
        }
        let (sha, bytes) = record.externalise_args().expect("a blob");
        assert_eq!(sha, sha256_hex(bytes.as_bytes()));
        assert_eq!(record.action.args, Value::Null);
        assert_eq!(record.action.exec_binding, Value::Null);
        assert_eq!(record.preconditions.workspace, None);
        let inline = serde_json::to_string(&record).expect("json");
        assert!(inline.len() < 4096, "{} bytes stay inline", inline.len());
        assert_eq!(record.whole(Some("{}")), None);
        let whole = record.whole(Some(&bytes)).expect("the blob");
        assert_eq!(whole.action.exec_binding, exec_binding);
        assert_eq!(whole.preconditions.workspace, Some(workspace.clone()));
        assert_eq!(whole.recomputed_digest(&whole.action.args), Ok(digest));
        // A small run stays inline, binding and set with it.
        let small = json!({"argv": ["ls"]});
        let mut inline = record_of(&small, &json!({"argv": ["ls"]}), &json!({}));
        assert!(inline.externalise_args().is_none());
        assert_eq!(inline.action.exec_binding, json!({"argv": ["ls"]}));
    }
}
