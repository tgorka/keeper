//! Agent slash commands (109.19). Parsing and capability filtering share the
//! messenger engine; the runtime only receives explicit control rows, never slash prose.
use serde::{Deserialize, Serialize};
use serde_json::json;
use ts_rs::TS;

use crate::chat::commands::{self, ArgMode, CommandSpec, Context, Place, Resolution, RoomKind};
use crate::hq::port::RowOp;
use crate::hq::sessions::{self, AgentSessionRef, Source};
use crate::hq::{live, timeline, Hq, HqError};
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
        "Choose the model for the next turn.",
        ArgMode::Required,
        Some("model id"),
    ),
    spec(
        "effort",
        "Choose the reasoning effort for the next turn.",
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

// 109.18 and 109.23 attach their operation here when their backend exists.
// Neither a model/effort picker nor a drive selection exists on this base.
fn registry(has_tools: bool) -> Vec<CommandSpec> {
    REGISTRY
        .iter()
        .copied()
        .filter(|s| match s.name {
            "model" | "effort" | "drives" => false,
            "tools" => has_tools,
            _ => true,
        })
        .collect()
}

/// Menu and verdict are one Rust answer, like the messenger's.
pub fn preview(draft: &str, has_tools: bool) -> ChatCommandPreviewVm {
    let registry = registry(has_tools);
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
            .map(|s| ChatCommandRowVm::compose(s, &ctx))
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
}

/// Execute against the same replica used by the session's ordinary controls.
pub fn run(source: &Source, sid: &str, draft: &str) -> Result<AgentCommandResultVm, String> {
    let session =
        timeline::session_row(source, sid).ok_or("This conversation is not on this device.")?;
    if session.archived {
        return Err("Unarchive this conversation first.".to_owned());
    }
    let registry = registry(!live::tools(source, sid, &session.agent_actor).is_empty());
    let (name, args) = match commands::resolve_in(&registry, draft, &context()) {
        Resolution::Command { name, args } => (name, args),
        Resolution::Refusal { message, .. } => return Err(message),
        _ => return Err("Choose a command first.".to_owned()),
    };
    let mut result = AgentCommandResultVm {
        session: None,
        surface: None,
        message: String::new(),
    };
    match name {
        "clean" | "new" => {
            // Empty body creates no message and therefore no phantom model turn.
            let id =
                sessions::create(source, &session.agent_actor, "").map_err(|e| e.to_string())?;
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
        "stop" => {
            let turns = source.replica.query("SELECT DISTINCT turn_id AS id FROM turn_chunks WHERE session_id = ?1 AND turn_id NOT IN (SELECT turn_id FROM messages WHERE session_id = ?1 AND role = 'agent' AND turn_id IS NOT NULL)", &[sid.into()]).map_err(|e| e.to_string())?;
            if turns.is_empty() {
                return Err("There is no running turn to stop.".to_owned());
            }
            let at = sessions::now_ms();
            let ops = turns
                .iter()
                .filter_map(|turn| turn.get("id").and_then(serde_json::Value::as_str))
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
            !live::tools(&source, &key.session_id, &session.agent_actor).is_empty(),
        ))
    }

    pub fn command(
        &self,
        key: &AgentSessionRef,
        draft: &str,
    ) -> Result<AgentCommandResultVm, HqError> {
        let source = self.source(&key.database)?;
        let result = run(&source, &key.session_id, draft).map_err(HqError::Sentence)?;
        self.touch();
        Ok(result)
    }
}
