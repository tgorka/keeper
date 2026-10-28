//! A `drive_write` approval on the deciding person's own device (109.27,
//! R-HQ-87, contract §§ 5, 8): what its card shows, when it offers *Allow*,
//! and how the write is carried out as that person — in this order, and
//! nothing writes the drive before the fourth step:
//!
//! 1. **Before Allow is offered** the request is decidable, the digest
//!    recomputed from the rows here equals `approvals.binding_digest`, and
//!    the apply checks ([`assess`]) pass; otherwise the card offers *Deny*
//!    only and says why ([`AgentDriveWriteVm::blocked`]).
//! 2. **Decide**: the ordinary `pending → approved` commit, waited for.
//! 3. **Consume**: `approved → consumed` by this device, on the version it
//!    read, waited for; refused → nothing is written, and the agent is told
//!    the server's reason.
//! 4. **Effect**: the apply checks again, then one authored commit through
//!    the [`DriveWrites`] port, carrying `Approval-Record: <approval id>`.
//! 5. **Outcome**: the `applied` message (V18) in its own commit.
//!
//! Steps 3–5 follow 2 in the same call; whatever the wait leaves to the
//! server is picked up by [`Writes::carry_on`] when the replica has the
//! answer. A device stopped between 3 and 5 settles on its next start: the
//! commit carrying the approval is looked for by its trailer — found, it is
//! reported `applied`; not found, `not_applied` (§ 5.3). A device stopped
//! between 2 and 3 leaves the row approved until the claim holder expires
//! it (§ 12).
//!
//! Every decision and sentence is here; the port answers what the checkout
//! holds with keeper-sync's own path rule, and commits.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use ts_rs::TS;

use super::{
    decidable, decision_op, refused_because, settle, AgentApprovalActionVm, AgentApprovalStateVm,
    AgentApprovalVm, AgentDecisionOutcomeVm, AgentDecisionStateVm, Decisions, Refusal,
    DECISION_WAIT,
};
use crate::agents::drive_write::{self, Change, LineKind, Op, Preview, Request};
use crate::hq::port::{
    Delivery, DriveAbsent, DriveCommit, DriveCommitted, DriveFile, DriveTarget, DriveWrites,
    LinkState, RowOp,
};
use crate::hq::sessions::{
    authored, int, json as json_column, month, names, object, opt_text, page_key, read, text,
    Source,
};

/// `metadata.kind` of the message that says what became of a write (V18).
pub const KIND: &str = "applied";

/// How long one look at a write's checkout is shown before it is looked at
/// again: an open session is drawn on every change, an answer's tail
/// included, and the look reads the drive's index.
pub const LOOK_FOR: Duration = Duration::from_secs(2);

/// Said when the digest recomputed from the rows here is not the one the
/// request was parked with.
pub const CHANGED: &str = "This request changed since it was shown.";

/// Said when the device was stopped after it took the approval and before it
/// said what became of it, and no commit carries it (§ 5.3).
pub const STOPPED: &str = "keeper stopped before it wrote this; nothing was written.";

/// Said when the request reaches a build with no drive writer.
pub const NO_WRITER: &str = "keeper on this device cannot write to drives.";

/// Said when a drive write is approved anywhere but through
/// [`Writes::allow`], which carries it out.
pub const ON_ITS_CARD: &str =
    "A drive write is approved on its card in keeper, which carries it out.";

/// The id of the message saying what became of `approval` (V18).
pub fn message_id(approval: &str) -> String {
    format!("applied.{approval}")
}

/// Whether a message's metadata makes it an `applied` control row: no
/// message, drawn on its approval's card, never as a bubble.
pub fn is_applied(metadata: &Value) -> bool {
    metadata.get("kind").and_then(Value::as_str) == Some(KIND)
}

// ─── The card ────────────────────────────────────────────────────────────────

/// What the write does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentDriveWriteOpVm {
    Create,
    Replace,
    Edit,
    Append,
    Move,
    Delete,
}

impl From<Op> for AgentDriveWriteOpVm {
    fn from(op: Op) -> Self {
        match op {
            Op::Create => Self::Create,
            Op::Replace => Self::Replace,
            Op::Edit => Self::Edit,
            Op::Append => Self::Append,
            Op::Move => Self::Move,
            Op::Delete => Self::Delete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentWriteLineKindVm {
    Context,
    Add,
    Remove,
}

/// One line of a preview, its text without the line break. Text, never
/// markup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentWriteLineVm {
    pub kind: AgentWriteLineKindVm,
    pub text: String,
}

/// One run of changes and up to three lines of context around it; the
/// 1-based numbers of its first line on each side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentWriteHunkVm {
    pub old_start: u32,
    pub new_start: u32,
    pub lines: Vec<AgentWriteLineVm>,
}

/// A text's first lines, and how many more it has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentWriteExcerptVm {
    pub lines: Vec<String>,
    pub more: u32,
}

/// What the person is shown of the change: a line diff's hunks; or the new
/// text's beginning (`whole`) when a side is too long to diff; or the
/// deleted text's beginning. Empty for a move, and wherever this device did
/// not look at the file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentWritePreviewVm {
    pub hunks: Vec<AgentWriteHunkVm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub whole: Option<AgentWriteExcerptVm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub deleted: Option<AgentWriteExcerptVm>,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl From<Preview> for AgentWritePreviewVm {
    fn from(preview: Preview) -> Self {
        let excerpt = |e: drive_write::Excerpt| AgentWriteExcerptVm {
            lines: e.lines,
            more: count(e.more),
        };
        Self {
            hunks: preview
                .hunks
                .into_iter()
                .map(|hunk| AgentWriteHunkVm {
                    old_start: count(hunk.old_start),
                    new_start: count(hunk.new_start),
                    lines: hunk
                        .lines
                        .into_iter()
                        .map(|line| AgentWriteLineVm {
                            kind: match line.kind {
                                LineKind::Context => AgentWriteLineKindVm::Context,
                                LineKind::Add => AgentWriteLineKindVm::Add,
                                LineKind::Remove => AgentWriteLineKindVm::Remove,
                            },
                            text: line.text,
                        })
                        .collect(),
                })
                .collect(),
            whole: preview.whole.map(excerpt),
            deleted: preview.deleted.map(excerpt),
        }
    }
}

/// A `drive_write` request on its card (contract § 8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentDriveWriteVm {
    pub op: AgentDriveWriteOpVm,
    pub drive: String,
    pub path: String,
    /// A move's new path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub to: Option<String>,
    pub preview: AgentWritePreviewVm,
    /// The session read outside content: the card says so, beside the
    /// session's own marker.
    pub untrusted: bool,
    /// Who reads the drive beyond the people of the session's database
    /// (`Also read by: Marta.`); absent when nobody does, or this device
    /// cannot tell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub audience: Option<String>,
    /// Why *Allow* is not offered on this device: the card offers *Deny*
    /// only, and a denial tells the agent this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked: Option<String>,
    /// What became of it, from its `applied` message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome: Option<String>,
}

// ─── The rows a write is read from ───────────────────────────────────────────

/// A `drive_write` approval and its call, as this replica holds them.
#[derive(Debug, Clone)]
struct Call {
    approval: String,
    session: String,
    digest: String,
    state: String,
    decided_by_client: Option<String>,
    consumed_by_client: Option<String>,
    version: Option<i64>,
    call: String,
    turn: String,
    args: Value,
    agent: String,
    untrusted: bool,
}

const CALL: &str = "SELECT a.id AS id, a.session_id AS session_id, a.binding_digest AS digest, \
     a.state AS state, a.decided_by_client AS decided_by_client, \
     a.consumed_by_client AS consumed_by_client, \
     a._sync_version AS version, t.id AS call_id, t.turn_id AS turn_id, t.args AS args, \
     s.agent_actor AS agent_actor, s.trust AS trust \
     FROM approvals a JOIN tool_calls t ON t.id = a.tool_call_id \
     JOIN sessions s ON s.id = a.session_id";

impl Call {
    fn of(row: &Map<String, Value>) -> Option<Call> {
        Some(Call {
            approval: opt_text(row, "id")?,
            session: opt_text(row, "session_id")?,
            digest: text(row, "digest").to_owned(),
            state: text(row, "state").to_owned(),
            decided_by_client: opt_text(row, "decided_by_client"),
            consumed_by_client: opt_text(row, "consumed_by_client"),
            version: int(row, "version"),
            call: opt_text(row, "call_id")?,
            turn: text(row, "turn_id").to_owned(),
            args: json_column(row, "args"),
            agent: text(row, "agent_actor").to_owned(),
            untrusted: text(row, "trust") == "untrusted",
        })
    }

    fn read(source: &Source, approval: &str) -> Option<Call> {
        read(
            source,
            &format!("{CALL} WHERE a.id = ?1 AND t.name = ?2"),
            &[Value::from(approval), Value::from(drive_write::TOOL)],
        )
        .first()
        .and_then(Call::of)
    }
}

/// This device as the person's other devices list it (`clients.name`).
fn device_name(source: &Source) -> String {
    read(
        source,
        "SELECT name FROM clients WHERE id = ?1",
        &[Value::from(source.client.as_str())],
    )
    .first()
    .and_then(|row| opt_text(row, "name"))
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| source.client.clone())
}

fn agent_name(source: &Source, agent: &str) -> String {
    read(
        source,
        "SELECT display_name FROM actors WHERE id = ?1",
        &[Value::from(agent)],
    )
    .first()
    .and_then(|row| opt_text(row, "display_name"))
    .filter(|name| !name.is_empty())
    .unwrap_or_else(|| agent.to_owned())
}

/// The `applied` message of `approval`, if this replica holds it.
fn applied_message(source: &Source, approval: &str) -> Option<Value> {
    read(
        source,
        "SELECT metadata FROM messages WHERE id = ?1",
        &[Value::from(message_id(approval).as_str())],
    )
    .first()
    .map(|row| json_column(row, "metadata"))
    .filter(is_applied)
}

/// The card's line for an `applied` message.
fn outcome_line(metadata: &Value) -> Option<String> {
    let said = |key: &str| {
        metadata
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let device = said("device");
    match said("outcome") {
        "applied" => Some(format!(
            "Written as {} on {device}.",
            said("commit").chars().take(7).collect::<String>()
        )),
        "unchanged" => Some(format!(
            "The file already was as asked on {device}; nothing was committed."
        )),
        "not_applied" => Some(format!("Not written: {}", said("reason"))),
        _ => None,
    }
}

// ─── The checks ──────────────────────────────────────────────────────────────

/// The card's (and the agent's) sentence for a refusal of the request on
/// this device: keeper-sync's own for a path; "changed on this device" for
/// a file that is not the one the agent read; any other as the model would
/// read it, without its "nothing was asked".
fn reason_of(refusal: &drive_write::Refusal, path: &str, device: &str) -> String {
    use drive_write::Refusal as R;
    match refusal {
        R::Path { sentence } => sentence.clone(),
        R::Stale { .. } | R::Absent { .. } | R::Exists { .. } => changed(path, device),
        other => {
            let said = other.to_string();
            said.strip_suffix(" Nothing was asked.")
                .map_or(said.clone(), str::to_owned)
        }
    }
}

fn changed(path: &str, device: &str) -> String {
    format!("\"{path}\" changed on {device} since the agent read it.")
}

fn absent_reason(absent: DriveAbsent, drive: &str) -> String {
    match absent {
        DriveAbsent::NotKept => format!("This device does not keep {drive}."),
        DriveAbsent::PullsOnly => {
            format!("{drive} only pulls on this device, so nothing is committed here.")
        }
        DriveAbsent::NotAttached => {
            format!("{drive}'s folder is not here (its disk is not attached).")
        }
    }
}

/// A write that passed every check: what the commit is made of.
#[derive(Debug, Clone)]
struct Plan {
    request: Request,
    path: DriveFile,
    to: Option<DriveFile>,
    /// The file's text after the change; `None` for a move or a delete.
    post: Option<String>,
}

impl Plan {
    /// The file's text here before the change, when it is text.
    fn before(&self) -> Option<&str> {
        self.path
            .disk
            .as_ref()
            .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
    }

    fn blob(&self) -> Option<String> {
        self.path.disk.as_ref().map(|(blob, _)| blob.clone())
    }

    fn preview(&self) -> AgentWritePreviewVm {
        match self.request.change {
            Change::Move { .. } => AgentWritePreviewVm::default(),
            _ => drive_write::preview(self.before(), self.post.as_deref()).into(),
        }
    }

    /// The commit of exactly this request (contract § 5.2), over the keys
    /// the path check answered.
    fn commit(&self, subject: String, approval: &str) -> DriveCommit {
        let key = self.path.key.clone();
        let guard = (key.clone(), self.request.expected.clone());
        let (writes, moves) = match (&self.request.change, &self.to) {
            (Change::Move { .. }, Some(to)) => (Vec::new(), vec![(key, to.key.clone())]),
            (Change::Delete, _) => (vec![(key, None)], Vec::new()),
            _ => (
                vec![(key, self.post.clone().map(String::into_bytes))],
                Vec::new(),
            ),
        };
        DriveCommit {
            writes,
            moves,
            guards: vec![guard],
            subject,
            approval: approval.to_owned(),
        }
    }
}

/// The session ticks `drive` now.
fn ticked(source: &Source, session: &str, drive: &str) -> bool {
    read(
        source,
        "SELECT drives FROM session_drives WHERE session_id = ?1",
        &[Value::from(session)],
    )
    .first()
    .is_some_and(|row| names(row, "drives").iter().any(|d| d == drive))
}

/// The session's switches turn `drive_write` off, or `drive`, which turns it
/// off too.
fn switched_off(source: &Source, session: &str) -> bool {
    read(
        source,
        "SELECT tools_off FROM session_tool_switches WHERE session_id = ?1",
        &[Value::from(session)],
    )
    .first()
    .is_some_and(|row| {
        names(row, "tools_off")
            .iter()
            .any(|tool| tool == drive_write::TOOL || tool == "drive")
    })
}

/// The apply checks (contract § 5 step 1, § 5.1), in order: the digest, the
/// arguments, this device keeps the drive and commits to it, the session
/// still ticks it and leaves the tool on, the path rule over this checkout,
/// the file is the one the agent read — at `HEAD` and on the disk — and the
/// change is within the caps. The plan, or why not, in the card's words.
fn assess(source: &Source, port: &dyn DriveWrites, call: &Call) -> Result<Plan, String> {
    let recomputed = drive_write::digest(
        &call.approval,
        &call.session,
        &call.agent,
        &call.args,
        &call.turn,
        &call.call,
    );
    if recomputed.ok().as_deref() != Some(call.digest.as_str()) {
        return Err(CHANGED.to_owned());
    }
    let device = device_name(source);
    let request = Request::parse(&call.args).map_err(|refusal| reason_of(&refusal, "", &device))?;
    let reason = |refusal: drive_write::Refusal| reason_of(&refusal, &request.path, &device);
    port.keeps(&request.drive)
        .map_err(|absent| absent_reason(absent, &request.drive))?;
    if !ticked(source, &call.session, &request.drive) {
        return Err(format!(
            "{} is no longer ticked for this conversation.",
            request.drive
        ));
    }
    if switched_off(source, &call.session) {
        return Err("drive_write is switched off for this conversation.".to_owned());
    }
    let target = match request.op() {
        Op::Create => DriveTarget::New,
        _ => DriveTarget::Existing,
    };
    let path = port
        .target(&request.drive, &request.path, target)
        .map_err(reason)?;
    let to = match request.to() {
        Some(to) => Some(
            port.target(&request.drive, to, DriveTarget::New)
                .map_err(|refusal| match refusal {
                    // Something is at the new path: nothing is moved over it.
                    drive_write::Refusal::Exists { .. } => reason_of(
                        &drive_write::Refusal::ToExists { to: to.to_owned() },
                        to,
                        &device,
                    ),
                    other => reason_of(&other, to, &device),
                })?,
        ),
        None => None,
    };
    if let Some(expected) = &request.expected {
        let on_disk = path.disk.as_ref().map(|(blob, _)| blob);
        if path.head.as_ref() != Some(expected) || on_disk != Some(expected) {
            return Err(changed(&request.path, &device));
        }
    }
    let post = request
        .post_image(path.disk.as_ref().map(|(_, bytes)| bytes.as_slice()))
        .map_err(reason)?;
    Ok(Plan {
        request,
        path,
        to,
        post,
    })
}

/// Who reads `drive` beyond the people of `source`'s database: the people of
/// the database named after it (AD-433), where this device holds it.
fn audience(sources: &[Source], source: &Source, drive: &str) -> Option<String> {
    let people = |of: &Source| -> BTreeSet<String> {
        read(
            of,
            "SELECT id FROM actors WHERE dir = ?1 AND kind = 'person'",
            &[Value::from(of.database.as_str())],
        )
        .iter()
        .filter_map(|row| opt_text(row, "id"))
        .collect()
    };
    let theirs = sources.iter().find(|s| s.database == drive)?;
    let here = people(source);
    let directory = crate::hq::sessions::directory(theirs);
    let beyond: Vec<String> = people(theirs)
        .difference(&here)
        .map(|id| directory.get(id).cloned().unwrap_or_else(|| id.clone()))
        .collect();
    (!beyond.is_empty()).then(|| format!("Also read by: {}.", beyond.join(", ")))
}

// ─── The rows a write leaves ─────────────────────────────────────────────────

/// What became of a write, as its `applied` message says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied { commit: String },
    Unchanged,
    NotApplied { reason: String },
}

/// The most characters `metadata.reason` holds (V18), counted as the server
/// counts a string: in UTF-16 code units.
const MAX_REASON: usize = 1_000;

/// Said where a refusal came with no words of its own: V18 refuses an empty
/// `reason`.
const NO_REASON: &str = "keeper was given no reason.";

/// `reason` as V18 takes it: non-empty, at most [`MAX_REASON`] UTF-16 code
/// units, cut at a character.
fn wire_reason(reason: &str) -> String {
    let reason = reason.trim();
    if reason.is_empty() {
        return NO_REASON.to_owned();
    }
    let mut units = 0;
    reason
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= MAX_REASON
        })
        .collect()
}

/// Whether `commit` is a commit id as V18 takes it: 40 lowercase hex.
fn is_commit_id(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The `applied` message of `call` (V18): its id, a person message in the
/// approval's session, `metadata` with exactly the contract's keys.
fn outcome_op(
    source: &Source,
    call: &Call,
    summary: &str,
    outcome: &Outcome,
    denied: bool,
) -> RowOp {
    let device = device_name(source);
    let mut metadata = json!({
        "kind": KIND,
        "approval": call.approval,
        "call": call.call,
        "device": device,
    });
    let body = match outcome {
        Outcome::Applied { commit } => {
            metadata["outcome"] = json!("applied");
            metadata["commit"] = json!(commit);
            format!(
                "Approved and written: {summary} ({}).",
                commit.chars().take(7).collect::<String>()
            )
        }
        Outcome::Unchanged => {
            metadata["outcome"] = json!("unchanged");
            format!("Approved: {summary}; the file already was as asked, so nothing was committed.")
        }
        Outcome::NotApplied { reason } => {
            let reason = wire_reason(reason);
            metadata["outcome"] = json!("not_applied");
            metadata["reason"] = json!(reason);
            if denied {
                format!("Declined: {summary}. {reason}")
            } else {
                format!("Approved, but not written: {summary}. {reason}")
            }
        }
    };
    let at = crate::hq::sessions::now_ms();
    RowOp::Upsert {
        table: "messages".to_owned(),
        values: authored(
            source,
            object(json!({
                "id": message_id(&call.approval),
                "session_id": call.session,
                "page_key": page_key(&call.session, &month(at)),
                "turn_id": null,
                "role": "person",
                "body": body,
                "body_blob": null,
                "metadata": metadata.to_string(),
                "created_at_ms": at,
            })),
        ),
        base_version: None,
    }
}

fn summary_of(call: &Call) -> String {
    Request::parse(&call.args).map_or_else(|_| drive_write::TOOL.to_owned(), |r| r.summary())
}

/// The patch taking `call`'s approval (V8 as amended): `consumed` by this
/// device, on the version read.
fn consume_op(source: &Source, call: &Call) -> RowOp {
    RowOp::Patch {
        table: "approvals".to_owned(),
        values: authored(
            source,
            object(json!({
                "id": call.approval,
                "state": "consumed",
                "consumed_by_client": source.client,
            })),
        ),
        base_version: call.version.filter(|v| *v > 0),
    }
}

/// Wait up to [`DECISION_WAIT`], while connected, for the server's word on
/// `commit`: `Some(Ok)` taken, `Some(Err)` refused, `None` not yet.
fn heard(source: &Source, commit: &str) -> Option<Result<(), String>> {
    source.replica.sync_now();
    let deadline = if source.replica.state().link == LinkState::Connected {
        Instant::now() + DECISION_WAIT
    } else {
        Instant::now()
    };
    loop {
        match source.replica.delivery(commit) {
            Ok(Delivery::Applied) => return Some(Ok(())),
            Ok(Delivery::Refused(rejection)) => return Some(Err(refused_because(&rejection))),
            Ok(Delivery::Pending | Delivery::Unknown) | Err(_) => {}
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

// ─── The device's state ──────────────────────────────────────────────────────

type Key = (String, String);

/// What this process handed to [`Writes::carry_on`]: the server had not
/// answered yet when the call that started it returned.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owed {
    /// Approved here; the consume follows once the server has the decision.
    Decided,
    /// Consumed here by `commit`; the effect follows once the server took it.
    Consumed { commit: String },
    /// Committed under an id V18 cannot record: nothing more is done here.
    Unrecordable,
}

/// A look at one write's checkout, kept for [`LOOK_FOR`].
#[derive(Debug, Clone)]
struct Look {
    at: Instant,
    blocked: Option<String>,
    /// The file's blob id here when the preview was made, and the preview.
    preview: Option<(Option<String>, AgentWritePreviewVm)>,
}

/// This device's drive writes: the port, the looks the cards show, the
/// writes being carried out now, and what the server still owes.
#[derive(Default)]
pub struct Writes {
    port: OnceLock<Arc<dyn DriveWrites>>,
    looks: Mutex<HashMap<Key, Look>>,
    busy: Mutex<HashSet<Key>>,
    owed: Mutex<HashMap<Key, Owed>>,
}

/// One write held by the call carrying it out; let go when dropped.
struct Held<'a> {
    writes: &'a Writes,
    key: Key,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        lock(&self.writes.busy).remove(&self.key);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Writes {
    /// Whether `approval` is a `drive_write` request, by its call here.
    pub fn is_write(source: &Source, approval: &str) -> bool {
        !read(
            source,
            "SELECT 1 AS one FROM approvals a JOIN tool_calls t ON t.id = a.tool_call_id \
             WHERE a.id = ?1 AND t.name = ?2",
            &[Value::from(approval), Value::from(drive_write::TOOL)],
        )
        .is_empty()
    }

    /// Set the drive writer, once; a later call is ignored.
    pub fn set_port(&self, port: Arc<dyn DriveWrites>) {
        if self.port.set(port).is_err() {
            tracing::debug!("the drive writer was set already");
        }
    }

    fn port(&self) -> Option<Arc<dyn DriveWrites>> {
        self.port.get().cloned()
    }

    /// Hold `key` for this call, or `None` while another call holds it. The
    /// lock is let go before the holder is made, and the holder is made only
    /// on success: a refused one must not release the other call's hold, nor
    /// lock `busy` again while it is held (R606-01).
    fn hold(&self, key: Key) -> Option<Held<'_>> {
        let taken = lock(&self.busy).insert(key.clone());
        taken.then(|| Held { writes: self, key })
    }

    fn owe(&self, key: Key, owed: Owed) {
        lock(&self.owed).insert(key, owed);
    }

    fn settled(&self, key: &Key) {
        lock(&self.owed).remove(key);
    }

    /// A look at `call`'s write: kept for [`LOOK_FOR`], the preview made
    /// again only when the file here changed.
    fn look(&self, source: &Source, port: &dyn DriveWrites, call: &Call) -> Look {
        let key = (source.database.clone(), call.approval.clone());
        if let Some(look) = lock(&self.looks).get(&key) {
            if look.at.elapsed() < LOOK_FOR {
                return look.clone();
            }
        }
        let kept = lock(&self.looks)
            .get(&key)
            .and_then(|look| look.preview.clone());
        let look = match assess(source, port, call) {
            Ok(plan) => {
                let blob = plan.blob();
                let preview = match kept {
                    Some((was, preview)) if was == blob => preview,
                    _ => plan.preview(),
                };
                Look {
                    at: Instant::now(),
                    blocked: None,
                    preview: Some((blob, preview)),
                }
            }
            Err(reason) => Look {
                at: Instant::now(),
                blocked: Some(reason),
                preview: kept,
            },
        };
        lock(&self.looks).insert(key, look.clone());
        look
    }

    /// Draw every `drive_write` request among `approvals` (contract § 8):
    /// its write view, and — while it waits for this person here — the
    /// preview from this checkout and *Allow* only where every check passes.
    /// A request decided here keeps the last preview it was shown with.
    pub fn draw(
        &self,
        sources: &[Source],
        source: &Source,
        approvals: &mut [&mut AgentApprovalVm],
    ) {
        for approval in approvals.iter_mut() {
            if approval.tool_name != drive_write::TOOL || approval.database != source.database {
                continue;
            }
            self.draw_one(sources, source, approval);
        }
    }

    fn draw_one(&self, sources: &[Source], source: &Source, approval: &mut AgentApprovalVm) {
        let Some(call) = Call::read(source, &approval.id) else {
            return;
        };
        let request = match Request::parse(&call.args) {
            Ok(request) => request,
            Err(refusal) => {
                // Nothing a person could allow: the runtime never parks it.
                approval
                    .actions
                    .retain(|action| *action != AgentApprovalActionVm::AllowOnce);
                if approval.can_decide {
                    approval.sentence = reason_of(&refusal, "", &device_name(source));
                }
                return;
            }
        };
        let key = (source.database.clone(), call.approval.clone());
        let mut vm = AgentDriveWriteVm {
            op: request.op().into(),
            drive: request.drive.clone(),
            path: request.path.clone(),
            to: request.to().map(str::to_owned),
            preview: AgentWritePreviewVm::default(),
            untrusted: call.untrusted,
            audience: audience(sources, source, &request.drive),
            blocked: None,
            outcome: applied_message(source, &call.approval).and_then(|m| outcome_line(&m)),
        };
        if approval.can_decide {
            let look = match self.port() {
                Some(port) => self.look(source, port.as_ref(), &call),
                None => Look {
                    at: Instant::now(),
                    blocked: Some(NO_WRITER.to_owned()),
                    preview: None,
                },
            };
            if let Some(reason) = look.blocked {
                approval
                    .actions
                    .retain(|action| *action != AgentApprovalActionVm::AllowOnce);
                vm.blocked = Some(reason);
            }
            if let Some((_, preview)) = look.preview {
                vm.preview = preview;
            }
        } else if call.decided_by_client.as_deref() == Some(source.client.as_str()) {
            if let Some((_, preview)) = lock(&self.looks).get(&key).and_then(|l| l.preview.clone())
            {
                vm.preview = preview;
            }
        }
        if approval.state == AgentApprovalStateVm::Consumed {
            // The agent runs nothing here: the deciding device writes it.
            approval.sentence = format!(
                "Approved by {}.",
                approval.decided_by.as_deref().unwrap_or("someone")
            );
        }
        approval.write = Some(vm);
    }

    /// *Allow* on a `drive_write` request: the apply checks, the decision,
    /// then — in this call while the server answers — the consume, the
    /// commit and the `applied` message. Blocking. What the server has not
    /// answered within the wait is carried on by [`Writes::carry_on`].
    pub fn allow(
        &self,
        source: &Source,
        decisions: &Decisions,
        session_id: &str,
        approval_id: &str,
        binding_digest: &str,
        now_ms: i64,
    ) -> Result<AgentDecisionOutcomeVm, Refusal> {
        let port = self.port().ok_or_else(|| Refusal(NO_WRITER.to_owned()))?;
        let key = (source.database.clone(), approval_id.to_owned());
        let _held = self
            .hold(key.clone())
            .ok_or_else(|| Refusal("keeper is carrying this request out already.".to_owned()))?;
        decidable(
            source,
            decisions,
            session_id,
            approval_id,
            binding_digest,
            true,
            now_ms,
        )?;
        let call = Call::read(source, approval_id)
            .ok_or_else(|| Refusal("This request is not on this device any more.".to_owned()))?;
        // The look stays: the card decided here keeps the diff it was allowed on.
        assess(source, port.as_ref(), &call).map_err(Refusal)?;
        let decided = settle(
            source,
            decisions,
            vec![decision_op(source, approval_id, true, call.version)],
            approval_id,
            Vec::new(),
            "Approved.",
        )?;
        if decided.state == AgentDecisionStateVm::Waiting {
            self.owe(key, Owed::Decided);
            return Ok(AgentDecisionOutcomeVm {
                state: AgentDecisionStateVm::Waiting,
                sentence: format!(
                    "Approved here; keeper writes it once {} has the decision.",
                    source.database
                ),
            });
        }
        // The decision's version is the one the consume patches.
        let call = Call::read(source, approval_id).unwrap_or(call);
        Ok(self.consume(source, port.as_ref(), &call, &key))
    }

    /// *Deny* on a `drive_write` request: the ordinary denial and, where
    /// *Allow* was not offered, its reason as the `applied` message in the
    /// same commit, so the agent learns why.
    pub fn deny(
        &self,
        source: &Source,
        decisions: &Decisions,
        session_id: &str,
        approval_id: &str,
        binding_digest: &str,
        now_ms: i64,
    ) -> Result<AgentDecisionOutcomeVm, Refusal> {
        let row = decidable(
            source,
            decisions,
            session_id,
            approval_id,
            binding_digest,
            false,
            now_ms,
        )?;
        let mut ops = vec![decision_op(
            source,
            approval_id,
            false,
            int(&row, "version"),
        )];
        if let Some(call) = Call::read(source, approval_id) {
            let blocked = match self.port() {
                Some(port) => assess(source, port.as_ref(), &call).err(),
                None => Some(NO_WRITER.to_owned()),
            };
            if let Some(reason) = blocked {
                ops.push(outcome_op(
                    source,
                    &call,
                    &summary_of(&call),
                    &Outcome::NotApplied { reason },
                    true,
                ));
            }
        }
        settle(source, decisions, ops, approval_id, Vec::new(), "Denied.")
    }

    /// Step 3 on: take the approval, then carry it out.
    fn consume(
        &self,
        source: &Source,
        port: &dyn DriveWrites,
        call: &Call,
        key: &Key,
    ) -> AgentDecisionOutcomeVm {
        let commit = match source.replica.mutate(vec![consume_op(source, call)]) {
            Ok(commit) => commit,
            Err(error) => {
                return self.finish(
                    source,
                    call,
                    key,
                    Outcome::NotApplied {
                        reason: error.to_string(),
                    },
                )
            }
        };
        match heard(source, &commit) {
            Some(Ok(())) => self.effect(source, port, call, key),
            Some(Err(reason)) => self.finish(source, call, key, Outcome::NotApplied { reason }),
            None => {
                self.owe(key.clone(), Owed::Consumed { commit });
                AgentDecisionOutcomeVm {
                    state: AgentDecisionStateVm::Waiting,
                    sentence: format!(
                        "Approved; keeper writes it once {} hands it over.",
                        source.database
                    ),
                }
            }
        }
    }

    /// Step 4 on: the checks again, the commit, the outcome.
    fn effect(
        &self,
        source: &Source,
        port: &dyn DriveWrites,
        call: &Call,
        key: &Key,
    ) -> AgentDecisionOutcomeVm {
        let outcome = match assess(source, port, call) {
            Err(reason) => Outcome::NotApplied { reason },
            Ok(plan) => {
                let subject = format!(
                    "{}: {}",
                    agent_name(source, &call.agent),
                    plan.request.summary()
                );
                match port.commit(&plan.request.drive, plan.commit(subject, &call.approval)) {
                    Ok(DriveCommitted::Committed(commit)) => Outcome::Applied { commit },
                    Ok(DriveCommitted::Unchanged) => Outcome::Unchanged,
                    Ok(DriveCommitted::Guarded(path)) => Outcome::NotApplied {
                        reason: changed(&path, &device_name(source)),
                    },
                    Err(reason) => Outcome::NotApplied { reason },
                }
            }
        };
        self.finish(source, call, key, outcome)
    }

    /// Step 5: the `applied` message, and the person's sentence.
    fn finish(
        &self,
        source: &Source,
        call: &Call,
        key: &Key,
        outcome: Outcome,
    ) -> AgentDecisionOutcomeVm {
        self.settled(key);
        let summary = summary_of(call);
        let commit = match &outcome {
            Outcome::Applied { commit } => Some(commit.to_ascii_lowercase()),
            _ => None,
        };
        let outcome = match commit {
            Some(commit) => Outcome::Applied { commit },
            None => outcome,
        };
        match &outcome {
            // V18 takes a 40-hex commit only: a row it would refuse is not
            // written, and the agent hears that no word came back, which is
            // the truth (R66-01).
            Outcome::Applied { commit } if !is_commit_id(commit) => {
                tracing::error!(database = %source.database, commit, "a drive write was committed under an id the agents server cannot record");
                self.owe(key.clone(), Owed::Unrecordable);
            }
            _ => {
                let op = outcome_op(source, call, &summary, &outcome, false);
                if let Err(error) = source.replica.mutate(vec![op]) {
                    tracing::warn!(database = %source.database, %error, "what became of a drive write could not be written");
                }
            }
        }
        source.replica.sync_now();
        let sentence = match &outcome {
            Outcome::Applied { commit } => format!(
                "Approved and written as {}.",
                commit.chars().take(7).collect::<String>()
            ),
            Outcome::Unchanged => {
                "Approved; the file already was as asked, so nothing was committed.".to_owned()
            }
            Outcome::NotApplied { reason } => {
                format!("Approved, but not written: {}", wire_reason(reason))
            }
        };
        AgentDecisionOutcomeVm {
            state: AgentDecisionStateVm::Sent,
            sentence,
        }
    }

    /// Carry on every `drive_write` approval this device decided whose
    /// outcome it has not written: what a call left to the server's answer,
    /// and — after a stop between taking an approval and saying what became
    /// of it — recovery by the commit's trailer (§ 5.3). A row whose last
    /// write here is still in the outbox waits for it. Blocking.
    pub fn carry_on(&self, sources: &[Source]) {
        let Some(port) = self.port() else { return };
        for source in sources {
            let rows = read(
                source,
                &format!(
                    "{CALL} WHERE t.name = ?1 AND a.decided_by_client = ?2 \
                     AND a.state IN ('approved', 'consumed') \
                     AND NOT EXISTS (SELECT 1 FROM messages m WHERE m.id = 'applied.' || a.id)"
                ),
                &[
                    Value::from(drive_write::TOOL),
                    Value::from(source.client.as_str()),
                ],
            );
            for call in rows.iter().filter_map(Call::of) {
                self.carry_one(source, port.as_ref(), &call);
            }
        }
    }

    fn carry_one(&self, source: &Source, port: &dyn DriveWrites, call: &Call) {
        let key = (source.database.clone(), call.approval.clone());
        let Some(_held) = self.hold(key.clone()) else {
            return;
        };
        let waiting = source
            .replica
            .row_writes("approvals", &call.approval, None)
            .map_or(true, |writes| !writes.pending.is_empty());
        if waiting {
            return;
        }
        let owed = lock(&self.owed).get(&key).cloned();
        let mine = call.consumed_by_client.as_deref() == Some(source.client.as_str());
        match (call.state.as_str(), owed) {
            (_, Some(Owed::Unrecordable)) => {}
            ("approved", Some(Owed::Decided)) => {
                self.consume(source, port, call, &key);
            }
            ("approved", Some(Owed::Consumed { commit })) => {
                if let Ok(Delivery::Refused(rejection)) = source.replica.delivery(&commit) {
                    self.finish(
                        source,
                        call,
                        &key,
                        Outcome::NotApplied {
                            reason: refused_because(&rejection),
                        },
                    );
                }
            }
            ("consumed", Some(_)) if mine => {
                self.effect(source, port, call, &key);
            }
            ("consumed", None) if mine => self.recover(source, port, call, &key),
            _ => {}
        }
    }

    /// § 5.3: the commit carrying the approval, found by its trailer, is
    /// what became of it; none is [`STOPPED`].
    fn recover(&self, source: &Source, port: &dyn DriveWrites, call: &Call, key: &Key) {
        let drive = match Request::parse(&call.args) {
            Ok(request) => request.drive,
            Err(refusal) => {
                let reason = reason_of(&refusal, "", &device_name(source));
                self.finish(source, call, key, Outcome::NotApplied { reason });
                return;
            }
        };
        match port.carried(&drive, &call.approval) {
            Ok(Some(commit)) => {
                self.finish(source, call, key, Outcome::Applied { commit });
            }
            Ok(None) => {
                self.finish(
                    source,
                    call,
                    key,
                    Outcome::NotApplied {
                        reason: STOPPED.to_owned(),
                    },
                );
            }
            Err(error) => {
                tracing::warn!(database = %source.database, %error, "a drive write taken here could not be looked for");
            }
        }
    }
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
