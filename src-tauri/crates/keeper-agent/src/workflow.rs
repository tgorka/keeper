//! A workflow's run (AD-398, story 94.3; rulings R103, R104, R106–R108).
//!
//! `_workflows/<name>/` holds a BMAD skill as written and its
//! `workflow.toml` ([`keeper_core::agents::workflow`]). A run is a session
//! of its own, of kind `workflow`, opened by the host of the session that
//! starts it — never as a brief to itself (R104): the host composes the
//! run's `agent.toml` (its workflow, its parent, `hop + 1`, the parent's
//! dispatch chain, its checkpoints, R103, and its declared outputs, R107),
//! makes its room — the agent, and the label's readers as observers —
//! creates the folder with its card, whose body is the brief and the
//! inputs, writes `delegate opened` and `sent` in the parent and watches
//! the room, so the run's `reply` comes home. The run's id is derived from
//! the call, or from the card and its window, so it is made once however
//! often it is asked for; an opening cut short goes on from what its parent
//! logged, and a run whose folder exists has its parent's lines written
//! again from it, nothing made twice (R202).
//!
//! - [`WorkflowTools`]: `workflow_start`, refused in a proxy's own `main`
//!   and `conversation` (AD-380) and for a workflow whose trigger says
//!   `manual = false` (R108).
//! - [`for_card`]: what a scheduled card naming a workflow opens instead of
//!   a turn (the epic's Q5): only when its trigger lets a card start it.
//! - [`open`]: the run's session, room and parent lines, under the parent's
//!   claim.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use keeper_core::agents::delegation::{
    child_card, session_title, workflow_session, DelegateCard, DelegateContent, DelegateFrom,
    DelegateLimits, Limits, CARD_FILE,
};
use keeper_core::agents::events::CONTENT_VERSION;
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::{Destination, Label, Readers, Sink};
use keeper_core::agents::log::{CardWindow, DelegateBody, DelegateState, LineBody};
use keeper_core::agents::session::{Checkpoints, SessionKind};
use keeper_core::agents::workflow::{
    brief, check_inputs, parse_start, parse_workflow_toml, run_drives, start_id, tools_check,
    InputKind, Workflow, IN_THE_DM, NOT_A_WORKFLOW, WORKFLOW_FILE, WORKFLOW_START,
};
use keeper_core::bots::chat::ToolCall as WireToolCall;
use keeper_core::bots::tools::ToolOutcome;
use keeper_sync::browse;
use matrix_sdk::ruma::OwnedRoomId;
use ulid::Ulid;

use crate::delegate::{block_on, Delegation, DelegationPort, Delegator, TurnView, NO_ROOMS};
use crate::sessions::verbs;
use crate::sessions::write::NO_CLAIM;
use crate::sinks::{CallAudit, Sinks};

/// Where BMAD's catalogue sits in the drive's install.
pub const HELP_CSV: &str = "_bmad/_config/bmad-help.csv";

/// What a name no folder of `_workflows/` and no catalogue row answers to
/// is told (capability row 9).
pub fn not_found(name: &str) -> String {
    format!("`{name}` is not a workflow in this drive's `_workflows/`.")
}

/// What a workflow whose trigger keeps it from a card says (R108).
pub fn not_by_card(name: &str) -> String {
    format!("`{name}` may not be started by a card")
}

/// What a workflow whose trigger keeps it from `workflow_start` and the
/// menu says (R108).
pub fn not_by_hand(name: &str) -> String {
    format!("`{name}` may not be started by hand or by workflow_start: its trigger says manual = false.")
}

/// What a run whose format-B generation changed since it rendered says on
/// another host (94.3 acceptance 8).
pub const SOURCES_CHANGED: &str = "this workflow's sources or BMAD configuration changed since this run rendered them; start it again";

/// Whether `name` is one plain folder name.
fn folder_name(name: &str) -> bool {
    !name.is_empty() && !name.contains(['/', '\\']) && name != "." && name != ".."
}

/// `workflow`'s declared outputs for a run opened `at`, tokens filled —
/// `{{date}}` that day, `{{slug}}` its workflow — session-relative, under
/// `artifacts/` (R107): stamped into the run's `agent.toml` as it opens.
pub fn declared_outputs(workflow: &Workflow, at: DateTime<Utc>) -> Vec<String> {
    let date = at.format("%Y-%m-%d").to_string();
    workflow
        .outputs
        .iter()
        .map(|output| {
            format!(
                "{}/{}",
                keeper_core::sessions::model::ARTIFACTS_DIR,
                keeper_core::agents::workflow::expand_output(&output.path, &date, &workflow.name)
            )
        })
        .collect()
}

/// `_workflows/<name>/workflow.toml` of the agents zone `zone` (drive
/// relative) under the drive `root`, read through `browse::resolve`.
pub fn read_workflow(root: &Path, zone: &str, name: &str) -> Result<Workflow, String> {
    if !folder_name(name) {
        return Err(not_found(name));
    }
    let folder = format!("{zone}/_workflows/{name}");
    let dir = match browse::resolve(root, &folder) {
        Ok(Some(dir)) if dir.is_dir() => dir,
        _ => return Err(not_found(name)),
    };
    let header = match browse::resolve(&dir, WORKFLOW_FILE) {
        Ok(Some(path)) if path.is_file() => path,
        _ => return Err(format!("_workflows/{name}: {NOT_A_WORKFLOW}")),
    };
    let text = std::fs::read_to_string(&header)
        .map_err(|error| format!("_workflows/{name}/{WORKFLOW_FILE} does not read: {error}"))?;
    parse_workflow_toml(
        name,
        &text,
        &|rel| matches!(browse::resolve(&dir, rel), Ok(Some(path)) if path.is_file()),
    )
}

/// The `_workflows/` folder `name` means: the folder itself, else the one
/// workflow folder among the skills a menu code or `skill:action` of the
/// drive's `bmad-help.csv` names (R109's hand-off, through 94.1's phase
/// graph). More than one is refused, naming them.
pub fn resolve_name(root: &Path, zone: &str, name: &str) -> Result<String, String> {
    let is_folder = |folder: &str| {
        folder_name(folder)
            && matches!(
                browse::resolve(root, &format!("{zone}/_workflows/{folder}")),
                Ok(Some(dir)) if dir.is_dir()
            )
    };
    if is_folder(name) {
        return Ok(name.to_owned());
    }
    let help = match browse::resolve(root, HELP_CSV) {
        Ok(Some(path)) if path.is_file() => std::fs::read_to_string(path)
            .ok()
            .and_then(|text| keeper_ported::bmad::help::parse(&text).ok()),
        _ => None,
    };
    let Some(help) = help else {
        return Err(not_found(name));
    };
    let mut found: Vec<String> = Vec::new();
    for row in help.named(name) {
        if is_folder(&row.skill) && !found.contains(&row.skill) {
            found.push(row.skill.clone());
        }
    }
    match found.len() {
        0 => Err(not_found(name)),
        1 => Ok(found.remove(0)),
        _ => Err(format!(
            "`{name}` names more than one workflow here: {}; name one by its folder.",
            found.join(", ")
        )),
    }
}

/// What opening a run reads and writes of the session that starts it.
pub trait Parent: Sync {
    /// The delegation its log holds as `id`.
    fn delegation(&self, id: &str) -> Option<Delegation>;
    /// `line` in its log now, synced: on the disk before the next effect.
    fn record(&self, line: LineBody) -> Result<(), String>;
    /// Whether its claim lets this host write now (R120).
    fn may_write(&self) -> bool;
}

/// A run to open.
pub struct Opening<'a> {
    /// The session that starts it.
    pub from: &'a Delegator,
    /// The agent's id in its home drive.
    pub agent: &'a str,
    pub id: Ulid,
    pub workflow: &'a Workflow,
    /// The run's drives, its home first.
    pub drives: Vec<String>,
    /// The card's body.
    pub brief: String,
    /// The starting session's label now: the run's at opening.
    pub label: Label,
    pub checkpoints: Checkpoints,
    /// The scheduled card and window that started it, when one did.
    pub window: Option<CardWindow>,
    pub at: DateTime<Utc>,
}

impl Opening<'_> {
    /// Where the opening's bytes land — the run's folder in its home drive,
    /// the one it has or the one it would be made in now — and those bytes:
    /// its brief, its inputs with it (R202). The same for the same call or
    /// window, so an approval of them names them again.
    pub fn effect(&self) -> (Destination, Vec<u8>) {
        let folder = match verbs::find(&self.from.zone, &self.id.to_string()) {
            Some(row) => row.path,
            None => verbs::new_session_path(
                &self.from.zone,
                &session_title(self.agent, self.at),
                self.at.with_timezone(&chrono::Local),
            ),
        };
        let path = format!("{}/{folder}", self.from.subfolder);
        let effect = serde_json::json!({
            "drive": self.from.drive,
            "path": path,
            "content": self.brief,
        })
        .to_string()
        .into_bytes();
        (
            Destination::Drive {
                drive: self.from.drive.clone(),
                path,
            },
            effect,
        )
    }
}

/// What [`open`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// A new run, in `room`, at `path` (zone-relative).
    New { room: OwnedRoomId, path: String },
    /// The id names a session already: nothing was made, and what its
    /// parent's log lacked of it was written.
    Existed { path: String },
}

/// The parent's `delegate` line of `state` for the run `id` in `room`.
fn parent_line(
    from: &Delegator,
    id: &str,
    room: &OwnedRoomId,
    state: DelegateState,
    window: Option<CardWindow>,
) -> LineBody {
    LineBody::Delegate(DelegateBody {
        id: id.to_owned(),
        to: from.user.to_string(),
        room: Some(room.clone()),
        child: None,
        state,
        reason: None,
        reply: None,
        window,
    })
}

/// Open `opening`'s run (R104) under `parent`'s claim: asked before any
/// room is made, and again under the zone's lock as the folder is (R202).
/// One session per id: a folder carrying it already makes nothing, its
/// room and the parent's `opened` and `sent` written again where the
/// parent's log lacks them; a room the parent logged is the run's, never a
/// second one. The parent's `opened` line, naming the room, is on the disk
/// before the folder is made.
pub fn open(
    port: &dyn DelegationPort,
    parent: &dyn Parent,
    opening: &Opening<'_>,
) -> Result<Opened, String> {
    let from = opening.from;
    let id = opening.id.to_string();
    if !parent.may_write() {
        return Err(NO_CLAIM.to_owned());
    }
    let logged = parent.delegation(&id);
    let record = |line| {
        parent
            .record(line)
            .map_err(|error| format!("The workflow's run could not be logged: {error}"))
    };
    if let Some(row) = verbs::find(&from.zone, &id) {
        let room = match &logged {
            Some(open) => open.room.clone(),
            None => {
                let text = std::fs::read_to_string(
                    from.zone
                        .join(&row.path)
                        .join(keeper_core::agents::session::FILE_NAME),
                )
                .map_err(|error| format!("The workflow's run does not read: {error}"))?;
                let run = keeper_core::agents::session::parse_session_agent_toml(&text)
                    .map_err(|refusal| refusal.sentence())?;
                if run.requested_by != from.user
                    || run.parent.as_ref().map(|p| p.session.as_str()) != Some(&from.session)
                {
                    return Err(format!("{id} is another session's, not this one's run."));
                }
                record(parent_line(
                    from,
                    &id,
                    &run.room,
                    DelegateState::Opened,
                    opening.window.clone(),
                ))?;
                run.room
            }
        };
        if !logged.as_ref().is_some_and(|open| open.sent) {
            record(parent_line(from, &id, &room, DelegateState::Sent, None))?;
        }
        port.watch(&room, &from.room, from.kind);
        return Ok(Opened::Existed { path: row.path });
    }
    // AD-385's bound stops a workflow that starts itself, as any hand-off.
    Limits::of(&from.limits)
        .check(from.hop, 0, 0)
        .map_err(|bound| bound.sentence())?;
    let name = &opening.workflow.name;
    let room = match &logged {
        // An opening cut short after its room: that room is the run's.
        Some(open) => open.room.clone(),
        None => {
            let invites: Vec<_> = match &opening.label.readers {
                Readers::Only(readers) => readers
                    .iter()
                    .filter(|reader| **reader != from.user)
                    .cloned()
                    .collect(),
                Readers::Anyone => Vec::new(),
            };
            if !parent.may_write() {
                return Err(NO_CLAIM.to_owned());
            }
            let room = block_on(port.create(
                SessionKind::Workflow,
                &session_title(opening.agent, opening.at),
                invites,
                Vec::new(),
            ))
            .map_err(|error| format!("The workflow's room could not be made: {error}"))?;
            record(parent_line(
                from,
                &id,
                &room,
                DelegateState::Opened,
                opening.window.clone(),
            ))?;
            room
        }
    };
    if !parent.may_write() {
        return Err(NO_CLAIM.to_owned());
    }
    port.watch(&room, &from.room, from.kind);
    let content = DelegateContent {
        v: CONTENT_VERSION,
        id: id.clone(),
        from: DelegateFrom {
            agent: from.user.clone(),
            drive: from.drive.clone(),
            session: from.session.clone(),
            room: from.room.clone(),
        },
        to: from.user.clone(),
        brief: opening.brief.clone(),
        drives: opening.drives.clone(),
        label: opening.label.clone(),
        hop: from.hop.saturating_add(1),
        limits: DelegateLimits {
            rounds_per_exchange: from.limits.rounds_per_exchange,
            tokens: from.limits.tokens_per_delegation,
        },
        card: Some(DelegateCard {
            title: format!("{name} run"),
            schedule: None,
            workflow: None,
        }),
        dispatch_chain: from.chain.clone(),
    };
    let mut agent = workflow_session(
        &content,
        opening.agent,
        &from.drive,
        &room,
        opening.at,
        name,
        opening.checkpoints,
    )
    .ok_or_else(|| "the run's id is not a ULID".to_owned())?;
    agent.outputs = declared_outputs(opening.workflow, opening.at);
    let made = verbs::create_claimed_session(
        &from.zone,
        &agent,
        vec![(CARD_FILE.to_owned(), child_card(&content, opening.agent))],
        opening.at.with_timezone(&chrono::Local),
        &|| parent.may_write(),
    )
    .map_err(|error| format!("The workflow's session could not be made: {error}"))?;
    let path = match made {
        verbs::CreateOutcome::Created { path, .. } | verbs::CreateOutcome::Existed { path, .. } => {
            path
        }
    };
    // Sent at once: no brief waits for a join, and the run's reply is taken
    // only once the delegation reads sent (R55).
    if !logged.as_ref().is_some_and(|open| open.sent) {
        record(parent_line(from, &id, &room, DelegateState::Sent, None))?;
    }
    Ok(Opened::New { room, path })
}

/// The checkpoints a run opens with: its workflow's, unless nobody could
/// be asked as it opens (R103).
pub fn checkpoints_of(workflow: &Workflow, anyone_to_ask: bool) -> Checkpoints {
    if anyone_to_ask {
        workflow.checkpoints
    } else {
        Checkpoints::Unattended
    }
}

/// The wire names a turn of a run working in the drives given would be
/// offered: its agent's offer for a session of kind `workflow`, armed as
/// every turn is (R202).
pub type RunOffer<'t> = dyn Fn(&[String]) -> Vec<String> + Send + Sync + 't;

/// One turn's `workflow_start`.
pub struct WorkflowTools<'t> {
    pub from: Delegator,
    pub port: Option<Arc<dyn DelegationPort>>,
    pub view: &'t dyn TurnView,
    /// The session that starts a run, as opening reads and writes it.
    pub parent: &'t dyn Parent,
    /// The agent's id in its home drive, and its kind.
    pub agent: String,
    pub kind: AgentKind,
    /// The home drive's root on this host, and its agents zone there.
    pub root: PathBuf,
    pub zone: String,
    /// Every mounted drive's root, by id: where a `path` input is read.
    pub roots: Vec<(String, PathBuf)>,
    /// The names this turn is offered.
    pub offered: Vec<String>,
    /// What a run would be offered: what its workflow's tools are checked
    /// against.
    pub run_offer: Box<RunOffer<'t>>,
    /// The drives in scope.
    pub scope: Vec<String>,
    /// Where an opening beyond the label is audited and routed (R202).
    pub sinks: &'t Sinks,
    /// The home drive's readers: who reads a run's brief on the disk.
    pub drive_readers: Readers,
}

fn refused(reason: impl Into<String>) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused {
        reason: reason.into(),
    })
}

impl WorkflowTools<'_> {
    /// Whether this session is a proxy's own conversation, where no
    /// workflow runs (AD-380).
    fn in_the_dm(&self) -> bool {
        self.kind == AgentKind::Proxy
            && matches!(
                self.from.kind,
                SessionKind::Main | SessionKind::Conversation
            )
    }

    /// Whether `workflow_start` is offered to this turn.
    pub fn offered(&self) -> bool {
        !self.in_the_dm() && self.offered.iter().any(|name| name == WORKFLOW_START)
    }

    /// A `path` input: `<drive>:<path>` in a drive of the run, or a path
    /// of its home drive, resolved inside its drive.
    fn path_input(&self, value: &str, drives: &[String]) -> Result<(), String> {
        let (drive, path) = match value.split_once(':') {
            Some((drive, path)) if drives.iter().any(|d| d == drive) => (drive, path),
            Some((drive, _)) if self.roots.iter().any(|(id, _)| id == drive) => {
                return Err(format!(
                    "{value} is in {drive}, which this run does not work in."
                ))
            }
            _ => (self.from.drive.as_str(), value),
        };
        let root = self
            .roots
            .iter()
            .find(|(id, _)| id == drive)
            .map_or(self.root.as_path(), |(_, root)| root.as_path());
        match browse::resolve(root, path) {
            Ok(Some(found)) if found.exists() => Ok(()),
            Ok(_) => Err(format!("{value} is not in {drive}.")),
            Err(refusal) => Err(format!("{value} is refused: {refusal}")),
        }
    }

    /// Run `wire` when it is `workflow_start`; `None` for any other name.
    /// `audit` is the call's one row: the opening's bytes are checked
    /// against the home drive's readers in it, and it is admitted, before
    /// anything is made.
    pub fn run(&self, wire: &WireToolCall, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        if wire.name != WORKFLOW_START {
            return None;
        }
        if self.in_the_dm() {
            return refused(format!("{IN_THE_DM}."));
        }
        if !self.offered.iter().any(|name| name == WORKFLOW_START) {
            return refused(format!(
                "{WORKFLOW_START} is not one of this agent's tools."
            ));
        }
        let args = wire.arguments.as_ref().unwrap_or(&serde_json::Value::Null);
        let call = match parse_start(args) {
            Ok(call) => call,
            Err(sentence) => return refused(sentence),
        };
        let Some(port) = self.port.clone() else {
            return refused(NO_ROOMS);
        };
        let name = match resolve_name(&self.root, &self.zone, &call.name) {
            Ok(name) => name,
            Err(sentence) => return refused(sentence),
        };
        let workflow = match read_workflow(&self.root, &self.zone, &name) {
            Ok(workflow) => workflow,
            Err(sentence) => return refused(sentence),
        };
        if !workflow.trigger.manual {
            return refused(not_by_hand(&name));
        }
        let drives = match run_drives(&workflow, &self.scope, &self.from.drive) {
            Ok(drives) => drives,
            Err(sentence) => return refused(sentence),
        };
        let offered = (self.run_offer)(&drives);
        let offered: Vec<&str> = offered.iter().map(String::as_str).collect();
        if let Err(sentence) = tools_check(&workflow, &self.agent, &offered) {
            return refused(sentence);
        }
        let inputs = match check_inputs(&workflow, &call.inputs) {
            Ok(inputs) => inputs,
            Err(sentence) => return refused(sentence),
        };
        for (input, value) in &inputs {
            let checked = match input.kind {
                InputKind::Text => Ok(()),
                InputKind::Path => self.path_input(value, &drives),
                InputKind::Drive if self.scope.iter().any(|drive| drive == value) => Ok(()),
                InputKind::Drive => Err(format!("{value} is not a drive in this session's scope.")),
                InputKind::Session if verbs::find(&self.from.zone, value).is_some() => Ok(()),
                InputKind::Session => Err(format!("{value} is no session of this drive.")),
            };
            if let Err(why) = checked {
                return refused(format!("`{name}`'s input {}: {why}", input.name));
            }
        }
        let folder = format!("{}/_workflows/{name}", self.zone);
        let anyone = crate::ask::can_ask(&self.from.chain, &self.from.user, &port.known());
        let id = start_id(&self.from.id, &wire.id);
        let opening = Opening {
            from: &self.from,
            agent: &self.agent,
            id,
            workflow: &workflow,
            drives,
            brief: brief(&workflow, &folder, &inputs),
            label: self.view.label(),
            checkpoints: checkpoints_of(&workflow, anyone),
            window: None,
            at: Utc::now(),
        };
        // The brief and its inputs land in the home drive, whose readers
        // read them whatever the run's label says (R202).
        let (destination, effect) = opening.effect();
        if let Err(blocked) = self.sinks.verdict(
            WORKFLOW_START,
            &destination,
            &opening.label,
            &Sink::DriveWrite {
                drive_readers: self.drive_readers.clone(),
            },
            &effect,
            None,
        ) {
            if let Err(withheld) = audit.blocked(blocked) {
                return Some(withheld.into());
            }
        }
        let (drive, at) = destination.target();
        if let Err(withheld) = audit.admit(drive, at) {
            return Some(withheld.into());
        }
        match open(port.as_ref(), self.parent, &opening) {
            Ok(Opened::New { .. }) => Some(ToolOutcome::Answered {
                text: format!(
                    "Started the workflow {name} as session {id}; it runs on its own and replies to this session when it is done."
                ),
            }),
            Ok(Opened::Existed { .. }) => Some(ToolOutcome::Answered {
                text: format!(
                    "The workflow {name} was started by this call already, as session {id}."
                ),
            }),
            Err(sentence) => refused(sentence),
        }
    }
}

/// A scheduled card naming a workflow, as its session holds it in a window.
pub struct CardRun<'a> {
    /// The session holding the card.
    pub from: &'a Delegator,
    /// The agent's id in its home drive.
    pub agent: &'a str,
    /// The home drive's root on this host, and its agents zone there.
    pub root: &'a Path,
    pub zone: &'a str,
    /// What a run would be offered.
    pub run_offer: &'a RunOffer<'a>,
    /// The session's drives in scope.
    pub scope: &'a [String],
    /// The session's label now.
    pub label: Label,
    /// Where an opening beyond the label is audited (R202).
    pub sinks: &'a Sinks,
    /// The home drive's readers.
    pub drive_readers: Readers,
}

/// What a scheduled card naming `name` opens instead of a turn (the epic's
/// Q5), as `run.from` holding the card `card` in `window`: the workflow
/// read, its trigger letting a card start it, its tools among those its
/// run would be offered and its drives in scope, its brief — the
/// workflow's and the card's body — let into the home drive by the
/// session's label (R202); the run unattended, as every scheduled card's
/// (R103). `Err` is the sentence the card's `run: failed` line says.
pub fn for_card(
    port: &dyn DelegationPort,
    parent: &dyn Parent,
    run: &CardRun<'_>,
    name: &str,
    card: &str,
    window: &str,
    body: &str,
) -> Result<Opened, String> {
    let workflow = read_workflow(run.root, run.zone, name)?;
    if !workflow.trigger.card {
        return Err(not_by_card(name));
    }
    let drives = run_drives(&workflow, run.scope, &run.from.drive)?;
    let offered = (run.run_offer)(&drives);
    let offered: Vec<&str> = offered.iter().map(String::as_str).collect();
    tools_check(&workflow, run.agent, &offered)?;
    let inputs = check_inputs(&workflow, &[])?;
    let folder = format!("{}/_workflows/{name}", run.zone);
    let mut text = brief(&workflow, &folder, &inputs);
    if !body.trim().is_empty() {
        text.push_str(&format!("\n\nThe card {card} says:\n{}", body.trim()));
    }
    let opening = Opening {
        from: run.from,
        agent: run.agent,
        id: keeper_core::agents::workflow::run_id(&run.from.id, card, window),
        workflow: &workflow,
        drives,
        brief: text,
        label: run.label.clone(),
        checkpoints: Checkpoints::Unattended,
        window: Some(CardWindow {
            card: card.to_owned(),
            window: window.to_owned(),
        }),
        at: Utc::now(),
    };
    let (destination, effect) = opening.effect();
    run.sinks.check(
        WORKFLOW_START,
        &destination,
        &opening.label,
        &Sink::DriveWrite {
            drive_readers: run.drive_readers.clone(),
        },
        &effect,
        None,
    )?;
    open(port, parent, &opening)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZONE: &str = "80-agents";

    /// A catalogue row of `module`'s `skill`, menu code `code`, `action`.
    fn row(module: &str, skill: &str, code: &str, action: &str) -> String {
        format!("{module},{skill},{skill},{code},Does it.,{action},,anytime,,,false,,\n")
    }

    /// R201: a name is a `_workflows/` folder, else the one workflow folder
    /// among the catalogue rows its menu code or `skill:action` names — a
    /// row naming no such folder does not count — and a name matching more
    /// than one is refused, naming them.
    #[test]
    fn a_workflow_is_named_by_its_folder_or_one_catalogue_row() {
        let root = tempfile::tempdir().expect("tempdir");
        let folder = |name: &str| {
            std::fs::create_dir_all(root.path().join(ZONE).join("_workflows").join(name))
                .expect("folder")
        };
        folder("bmad-create-epics-and-stories");
        let help = root.path().join(HELP_CSV);
        std::fs::create_dir_all(help.parent().expect("parent")).expect("mkdir");
        std::fs::write(
            &help,
            [
                format!("{}\n", keeper_ported::bmad::help::COLUMNS.join(",")),
                row(
                    "BMad Method",
                    "bmad-create-epics-and-stories",
                    "CE",
                    "create",
                ),
                row("Game Dev", "gds-create-epics-and-stories", "CE", "create"),
                row("BMad Method", "bmad-architecture", "CA", ""),
            ]
            .concat(),
        )
        .expect("catalogue");
        let resolved = |name: &str| resolve_name(root.path(), ZONE, name);

        for name in [
            "bmad-create-epics-and-stories",
            "CE",
            "bmad-create-epics-and-stories:create",
        ] {
            assert_eq!(
                resolved(name).as_deref(),
                Ok("bmad-create-epics-and-stories"),
                "{name}"
            );
        }
        assert_eq!(resolved("CA"), Err(not_found("CA")));
        assert_eq!(resolved("../x"), Err(not_found("../x")));

        folder("gds-create-epics-and-stories");
        assert_eq!(
            resolved("CE"),
            Err("`CE` names more than one workflow here: bmad-create-epics-and-stories, gds-create-epics-and-stories; name one by its folder.".to_owned())
        );
        assert_eq!(
            resolved("gds-create-epics-and-stories").as_deref(),
            Ok("gds-create-epics-and-stories")
        );
    }
}
