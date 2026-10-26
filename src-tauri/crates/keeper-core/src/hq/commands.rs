//! Agent slash commands (109.19). Parsing and capability filtering share the
//! messenger engine; the runtime only receives explicit control rows, never slash prose.
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::bots::chat::Effort;
use crate::chat::commands::{self, ArgMode, CommandSpec, Context, Place, Resolution, RoomKind};
use crate::hq::approvals::{self, Decisions, ShownApprovalVm};
use crate::hq::port::RowOp;
use crate::hq::sessions::{self, AgentSessionRef, Source};
use crate::hq::settings::{self, AgentSettingsChange, AgentSettingsPick};
use crate::hq::{drives, live, timeline, Hq, HqError};
use crate::vm::{ChatCommandPreviewVm, ChatCommandRowVm, ChatCommandVerdictVm};

const fn spec(
    name: &'static str,
    description: &'static str,
    args: ArgMode,
    hint: Option<&'static str>,
) -> CommandSpec {
    CommandSpec {
        name,
        aliases: &[],
        description,
        args,
        arg_hint: hint,
        place: Place::Everywhere,
        needs: &[],
    }
}

/// The default twelve. A capability without its operation is never advertised.
const REGISTRY: &[CommandSpec] = &[
    spec(
        "clean",
        "Start fresh with this agent; keep the old conversation.",
        ArgMode::None,
        None,
    ),
    spec(
        "new",
        "Start a new conversation with this agent.",
        ArgMode::OptionalRest,
        Some("title"),
    ),
    spec(
        "rename",
        "Rename this conversation.",
        ArgMode::Required,
        Some("title"),
    ),
    spec(
        "model",
        "Choose the model from the next answer on.",
        ArgMode::Required,
        Some("model id"),
    ),
    spec(
        "effort",
        "Choose the reasoning effort from the next answer on.",
        ArgMode::Required,
        Some("level"),
    ),
    spec(
        "stop",
        "Stop the running turn without repeating it.",
        ArgMode::None,
        None,
    ),
    spec(
        "tools",
        "Open this session's tool switches.",
        ArgMode::None,
        None,
    ),
    spec(
        "context",
        "Open this conversation's context details.",
        ArgMode::None,
        None,
    ),
    spec(
        "drives",
        "Choose the drives this session may read.",
        ArgMode::None,
        None,
    ),
    spec("people", "Open this session's people.", ArgMode::None, None),
    spec(
        "archive",
        "Archive this conversation, keeping its history.",
        ArgMode::None,
        None,
    ),
    spec("help", "List this session's commands.", ArgMode::None, None),
];

fn context() -> Context {
    Context {
        room: RoomKind::Chat,
        may_set_topic: false,
        may_invite: false,
        replying: false,
    }
}

/// What a session offers, so a command without its operation is absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offers {
    /// The agent offers tools here.
    pub tools: bool,
    /// This person has the session's drive control (109.23).
    pub drives: bool,
    /// The person may choose among the agent's models.
    pub models: bool,
    /// The efforts the agent offers, lowest first: `/effort` lists these
    /// and only these, and is absent without them.
    pub efforts: Vec<Effort>,
}

impl Offers {
    /// What `sid` offers this reader now. The model and effort choices
    /// exist only where the pickers do (109.18); `/drives` only for whoever
    /// has the drive control (109.23).
    pub fn of(source: &Source, sid: &str, session: &timeline::SessionRow) -> Offers {
        let picker = settings::picker(source, sid).ok();
        Offers {
            tools: !live::tools(source, sid, &session.agent_actor).is_empty(),
            drives: drives::available(source, sid, session),
            models: picker.as_ref().is_some_and(|p| !p.models.is_empty()),
            efforts: picker
                .map(|p| p.efforts.into_iter().map(|e| e.effort).collect())
                .unwrap_or_default(),
        }
    }
}

fn registry(offers: &Offers) -> Vec<CommandSpec> {
    REGISTRY
        .iter()
        .copied()
        .filter(|s| match s.name {
            "tools" => offers.tools,
            "drives" => offers.drives,
            "model" => offers.models,
            "effort" => !offers.efforts.is_empty(),
            _ => true,
        })
        .collect()
}

/// Menu and verdict are one Rust answer, like the messenger's.
pub fn preview(draft: &str, offers: &Offers) -> ChatCommandPreviewVm {
    let registry = registry(offers);
    let ctx = context();
    let verdict = match commands::resolve_in(&registry, draft, &ctx) {
        Resolution::Text(text) => ChatCommandVerdictVm::Text { text },
        Resolution::Command { name, args } => ChatCommandVerdictVm::Command {
            name: name.to_owned(),
            args,
        },
        Resolution::Refusal { message, .. } => ChatCommandVerdictVm::Refusal { message },
        Resolution::Emote(_) => unreachable!("agent registry contains no emote"),
    };
    ChatCommandPreviewVm {
        draft: draft.to_owned(),
        verdict,
        rows: commands::menu_in(&registry, draft, &ctx)
            .into_iter()
            .map(|s| {
                let mut row = ChatCommandRowVm::compose(s, &ctx);
                // The levels this agent takes, in the words its menu shows.
                if row.name == "effort" {
                    row.arg_hint = Some(settings::effort_hint(&offers.efforts));
                }
                row
            })
            .collect(),
        escape_hint: commands::ESCAPE_HINT.to_owned(),
    }
}

/// A local surface to reveal; the operation, not command text, crosses IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentCommandSurface {
    Tools,
    Drives,
    Context,
    People,
    Help,
}

/// The result of a command, including navigation after a fresh session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentCommandResultVm {
    pub session: Option<AgentSessionRef>,
    pub surface: Option<AgentCommandSurface>,
    pub message: String,
    /// After `/model` or `/effort`: the session's choice, which this device
    /// keeps as its last pick for the agent, as the pickers' change does.
    pub pick: Option<AgentSettingsPick>,
}

/// Execute against the same replica used by the session's ordinary controls.
/// `pick` is this device's last model and effort for the session's agent: a
/// fresh session (`/new`, `/clean`) starts with what the agent still offers.
/// `/clean` also carries the drives ticked here that this person may still
/// tick (Q109-17 b, A109-06; `sources`, every database this device holds,
/// are for who reads each drive); `/new` starts with none (Q109-20 b).
/// `/stop` declines the requests the banner showed (`shown`) that still wait
/// for this person, before it writes the stop (R10911-07): a stopped turn's
/// request has nothing left to decide. No other command declines anything —
/// none of them steers the agent's turn.
pub fn run(
    source: &Source,
    sources: &[Source],
    decisions: &Decisions,
    sid: &str,
    draft: &str,
    pick: Option<&AgentSettingsPick>,
    shown: &[ShownApprovalVm],
) -> Result<AgentCommandResultVm, String> {
    let session =
        timeline::session_row(source, sid).ok_or("This conversation is not on this device.")?;
    if session.archived {
        return Err("Unarchive this conversation first.".to_owned());
    }
    let registry = registry(&Offers::of(source, sid, &session));
    let (name, args) = match commands::resolve_in(&registry, draft, &context()) {
        Resolution::Command { name, args } => (name, args),
        Resolution::Refusal { message, .. } => return Err(message),
        _ => return Err("Choose a command first.".to_owned()),
    };
    let mut result = AgentCommandResultVm {
        session: None,
        surface: None,
        message: String::new(),
        pick: None,
    };
    match name {
        "clean" | "new" => {
            // Empty body creates no message and therefore no phantom model turn.
            let id = sessions::create(source, &session.agent_actor, "", pick)
                .map_err(|e| e.to_string())?;
            if name == "clean" {
                drives::carry(source, sid, &session, &id, sources, sessions::now_ms())
                    .map_err(|e| e.to_string())?;
            }
            if let Some(title) = args {
                sessions::rename(source, &id, &title).map_err(|e| e.to_string())?;
            }
            result.session = Some(AgentSessionRef {
                database: source.database.clone(),
                session_id: id,
            });
        }
        "rename" => sessions::rename(source, sid, args.as_deref().unwrap_or_default())
            .map_err(|e| e.to_string())?,
        "archive" => sessions::set_archived(source, sid, true).map_err(|e| e.to_string())?,
        "model" | "effort" => {
            let arg = args.unwrap_or_default();
            let arg = arg.trim();
            let what = if name == "model" {
                AgentSettingsChange::Model {
                    model: arg.to_owned(),
                }
            } else {
                AgentSettingsChange::Effort {
                    effort: settings::effort_named(source, sid, arg).map_err(|e| e.to_string())?,
                }
            };
            let changed = settings::change(source, sid, what, sessions::now_ms())
                .map_err(|e| e.to_string())?;
            let done = if name == "model" {
                format!("The model is now {arg}.")
            } else {
                format!(
                    "The effort is now {}.",
                    changed
                        .pick
                        .effort
                        .map(settings::effort_word)
                        .unwrap_or_default()
                )
            };
            result.message = match changed.note {
                Some(note) => format!("{done} {note}"),
                None => done,
            };
            result.pick = Some(changed.pick);
        }
        "stop" => {
            let turns = live::running_turns(source, sid).map_err(|e| e.to_string())?;
            if turns.is_empty() {
                return Err("There is no running turn to stop.".to_owned());
            }
            let at = sessions::now_ms();
            approvals::decline_pending(source, decisions, sid, shown, at)?;
            let ops = turns
                .iter()
                .map(|turn| {
                    let mut row = sessions::person_message(
                        source,
                        sid,
                        &ulid::Ulid::new().to_string(),
                        "Stopped this turn.",
                        at,
                    );
                    if let RowOp::Upsert { values, .. } = &mut row {
                        values.insert(
                            "metadata".to_owned(),
                            json!({"kind": "stop", "turn": turn}).to_string().into(),
                        );
                    }
                    row
                })
                .collect();
            source.replica.mutate(ops).map_err(|e| e.to_string())?;
            source.replica.sync_now();
            result.message = "Stop requested.".to_owned();
        }
        "tools" => result.surface = Some(AgentCommandSurface::Tools),
        "drives" => result.surface = Some(AgentCommandSurface::Drives),
        "context" => result.surface = Some(AgentCommandSurface::Context),
        "people" => result.surface = Some(AgentCommandSurface::People),
        "help" => result.surface = Some(AgentCommandSurface::Help),
        _ => unreachable!("every offered agent command has an operation"),
    }
    Ok(result)
}

impl Hq {
    pub fn command_preview(
        &self,
        key: &AgentSessionRef,
        draft: &str,
    ) -> Result<ChatCommandPreviewVm, HqError> {
        let source = self.source(&key.database)?;
        let session = timeline::session_row(&source, &key.session_id).ok_or_else(|| {
            HqError::Sentence("This conversation is not on this device.".to_owned())
        })?;
        Ok(preview(
            draft,
            &Offers::of(&source, &key.session_id, &session),
        ))
    }

    /// Run a slash command; `shown` is what the banner showed, which only
    /// `/stop` declines.
    pub fn command(
        &self,
        key: &AgentSessionRef,
        draft: &str,
        pick: Option<&AgentSettingsPick>,
        shown: &[ShownApprovalVm],
    ) -> Result<AgentCommandResultVm, HqError> {
        let source = self.source(&key.database)?;
        let result = run(
            &source,
            &self.sources(),
            self.decisions()?,
            &key.session_id,
            draft,
            pick,
            shown,
        );
        self.touch();
        result.map_err(HqError::Sentence)
    }
}
