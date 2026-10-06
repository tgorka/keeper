//! What a BMAD skill assumes an agent can do, and how keeper answers it
//! (AD-397, story 94.2).
//!
//! - [`CAPABILITIES`]: the 19 capabilities BMAD assumes without naming a
//!   tool (G4 §5), each answered by the tools of the closed vocabulary the
//!   turn is offered and by sentences that hold for that offer: one that
//!   tells the model to use a tool is said only while it is offered, one
//!   that says a tool is not offered only while it is not — never neither,
//!   so a skill run under keeper never improvises one.
//! - The argument grammars of the BMAD and skill tools an agent's host
//!   serves itself (R38: no ⌘9 bot is offered them).
//! - The session frame's BMAD lines: where `{project-root}` is read and
//!   written (R96), and the capability map as this turn's offer answers it.
//!
//! Pure: the host hands over what the turn is offered and reads the files.

use serde_json::{json, Map, Value};

use crate::agents::session::Checkpoints;
use crate::bots::chat::ToolSpec;

/// `resolve_config.py` and `resolve_customization.py`, ported.
pub const BMAD_CONFIG: &str = "bmad_config";
/// `render_skill.py`, ported: a format-B skill rendered into the session.
pub const BMAD_RENDER: &str = "bmad_render";
/// `memlog.py`, ported: the run's `.memlog.md` under `artifacts/`.
pub const BMAD_MEMLOG: &str = "bmad_memlog";
/// `resolve_party.py`, ported.
pub const BMAD_PARTY: &str = "bmad_party";
/// The skills offered to the agent.
pub const SKILLS_LIST: &str = "skills_list";
/// One offered skill's `SKILL.md`, or one file inside it.
pub const SKILL_VIEW: &str = "skill_view";
/// A question for the person the work is for, through their proxy (R99).
pub const ASK_HUMAN: &str = "ask_human";

/// The tools through which a session follows BMAD's skills and
/// workflows: a session offered any of them is told where BMAD's project
/// root is and what BMAD may assume there.
const FRAMED: [&str; 7] = [
    BMAD_CONFIG,
    BMAD_RENDER,
    BMAD_MEMLOG,
    BMAD_PARTY,
    SKILLS_LIST,
    SKILL_VIEW,
    "workflow_start",
];

/// Stands in a capability row for any MCP tool: they are named
/// `mcp__<server>__<tool>` on the wire (ruling R24(7)), never in the
/// vocabulary.
pub const MCP: &str = "mcp__";

/// When one sentence of a capability's answer is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// Whatever the turn is offered.
    Always,
    /// While none of these is offered: the sentence says they are not.
    Without(&'static [&'static str]),
    /// While this tool is offered: the sentence tells the model to use it.
    With(&'static str),
}

/// One capability BMAD assumes, and keeper's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    /// What a BMAD skill assumes it can do (G4 §5).
    pub assumes: &'static str,
    /// The tools that answer it, whichever of them the turn is offered.
    pub tools: &'static [&'static str],
    /// What the model is told besides, each sentence while its [`When`]
    /// holds for the turn's offer.
    pub said: &'static [(When, &'static str)],
}

/// G4 §5's capabilities, in its order.
pub const CAPABILITIES: [Capability; 19] = [
    Capability {
        assumes: "read a whole file or a range",
        tools: &["drive_read"],
        said: &[(
            When::Without(&["drive_read"]),
            "`drive_read` is not offered to this agent, so it reads no file.",
        )],
    },
    Capability {
        assumes: "write files; edit YAML frontmatter in place",
        tools: &["session_write", "drive_edit"],
        said: &[(
            When::Without(&["drive_edit"]),
            "Writing outside this session needs `drive_edit`, which this agent is not offered.",
        )],
    },
    Capability {
        assumes: "list / glob",
        tools: &["drive_list", "drive_glob"],
        said: &[(
            When::Without(&["drive_list", "drive_glob"]),
            "`drive_list` and `drive_glob` are not offered to this agent, so it lists no folder.",
        )],
    },
    Capability {
        assumes: "grep the code; read `git log`",
        tools: &["drive_grep", "run"],
        said: &[(
            When::Without(&["run"]),
            "`git log` runs only through `run`, which this agent is not offered.",
        )],
    },
    Capability {
        assumes: "run commands (`uv run`, Python ≥ 3.11)",
        tools: &[BMAD_CONFIG, BMAD_RENDER, BMAD_MEMLOG, BMAD_PARTY, "run"],
        said: &[(
            When::Without(&["run"]),
            "keeper never runs BMAD's Python helpers; a script with no Rust port (DW-382) runs only through `run`, which is not offered.",
        )],
    },
    Capability {
        assumes: "git `rev-parse`, diff to a temp file, commit",
        tools: &["run"],
        said: &[(
            When::Always,
            "keeper commits this drive itself; an agent never runs git on a drive.",
        )],
    },
    Capability {
        assumes: "run tests / linters",
        tools: &["run"],
        said: &[(
            When::Without(&["run"]),
            "Tests run only through `run`, which this agent is not offered.",
        )],
    },
    Capability {
        assumes: "ask the user and wait (HALT, menus)",
        tools: &["ask_human"],
        said: &[(
            When::Without(&["ask_human"]),
            "`ask_human` is not offered here: no person can be asked from this session, so a step that waits for an answer takes its stated default, and one with none ends the turn saying what it needs.",
        )],
    },
    Capability {
        assumes: "invoke a skill by name, forwarding intent",
        tools: &[SKILL_VIEW, "workflow_start"],
        said: &[
            (
                When::With(SKILL_VIEW),
                "A skill a step invokes is loaded with `skill_view` and followed here.",
            ),
            (
                When::Without(&[SKILL_VIEW]),
                "`skill_view` is not offered to this agent, so a skill a step invokes is not loaded; say which skill the step needs and go on without it.",
            ),
            (
                When::Without(&["workflow_start"]),
                "Handing off to the next workflow needs `workflow_start`, which this agent is not offered.",
            ),
        ],
    },
    Capability {
        assumes: "spawn a sync or parallel context-free subagent",
        tools: &["helper"],
        said: &[(
            When::Without(&["helper"]),
            "`helper` is not offered to this agent; do the work inline, as the skill's fallback says.",
        )],
    },
    Capability {
        assumes: "re-address a live subagent by id",
        tools: &["delegate"],
        said: &[
            (
                When::Always,
                "A helper keeps no identity after it returns; continue the work yourself.",
            ),
            (
                When::With("delegate"),
                "A delegated session takes its next round through `delegate`.",
            ),
        ],
    },
    Capability {
        assumes: "agent teams + a capability probe",
        tools: &["delegate"],
        said: &[
            (
                When::Always,
                "keeper runs a party in one mind: BMAD's `subagent`, `agent-team` and `auto` party modes run as `session`.",
            ),
            (
                When::With("delegate"),
                "A persona that must think on its own is a delegation through `delegate`.",
            ),
            (
                When::Without(&["delegate"]),
                "`delegate` is not offered to this agent, so every persona thinks in this session.",
            ),
        ],
    },
    Capability {
        assumes: "per-agent model choice",
        tools: &["delegate"],
        said: &[(
            When::Without(&["delegate"]),
            "Every step runs on this agent's own model; another model is another agent, reached with `delegate`, which this agent is not offered.",
        )],
    },
    // No offered tool is known to search the web: an MCP tool's name says
    // which server it is on, not what it does (R195).
    Capability {
        assumes: "web search",
        tools: &[],
        said: &[(
            When::Always,
            "keeper has no web search of its own (DW-381), and no tool offered to this agent is known to search the web, so it searches none.",
        )],
    },
    Capability {
        assumes: "MCP / external systems",
        tools: &[MCP],
        said: &[(
            When::Without(&[MCP]),
            "No MCP server is configured for this agent on this host.",
        )],
    },
    Capability {
        assumes: "headless/TTY detection, environment variables",
        tools: &[],
        said: &[(
            When::Always,
            "No environment variables are visible to an agent; this frame says whether a person can be asked.",
        )],
    },
    Capability {
        assumes: "open an editor or an HTML report",
        tools: &["surface_open"],
        said: &[(
            When::Without(&["surface_open"]),
            "Only a person's proxy opens a note on their screen (`surface_open`); name the report's path in your answer.",
        )],
    },
    Capability {
        assumes: "token counting, the current date",
        tools: &[],
        said: &[(When::Always, "The date and time are this frame's `Now:`.")],
    },
    Capability {
        assumes: "session lifecycle hooks, tmux",
        tools: &[],
        said: &[(
            When::Always,
            "bmad-loop does not run under keeper; a card or a delegation is the loop.",
        )],
    },
];

/// A capability answered for one turn's offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityAnswer {
    /// The row's tools the turn is offered, in the row's order.
    pub tools: Vec<&'static str>,
    /// What the model is told besides, in the row's order.
    pub sentences: Vec<&'static str>,
    /// The tools those sentences say are not offered.
    pub missing: Vec<&'static str>,
}

/// Whether `tool` — a vocabulary name, or [`MCP`] for any MCP tool — is
/// among the names the turn is `offered`.
fn is_offered(tool: &str, offered: &[&str]) -> bool {
    if tool == MCP {
        offered.iter().any(|name| name.starts_with(MCP))
    } else {
        offered.contains(&tool)
    }
}

/// `capability` answered for a turn offered `offered` (wire names): the
/// row's tools it is offered, and each of the row's sentences whose
/// [`When`] holds for that offer.
pub fn capability_answer(capability: &Capability, offered: &[&str]) -> CapabilityAnswer {
    let tools = capability
        .tools
        .iter()
        .copied()
        .filter(|tool| is_offered(tool, offered))
        .collect();
    let mut sentences = Vec::new();
    let mut missing = Vec::new();
    for (when, sentence) in capability.said {
        let holds = match when {
            When::Always => true,
            When::With(tool) => is_offered(tool, offered),
            When::Without(absent) => {
                let holds = !absent.iter().any(|tool| is_offered(tool, offered));
                if holds {
                    missing.extend(absent.iter().copied());
                }
                holds
            }
        };
        if holds {
            sentences.push(*sentence);
        }
    }
    CapabilityAnswer {
        tools,
        sentences,
        missing,
    }
}

/// The session frame's BMAD lines for a turn offered `offered`, in a
/// session whose `artifacts/` is `artifacts` (drive-relative) on the home
/// drive `drive`: none unless the turn is offered a tool through which it
/// follows a BMAD skill or workflow ([`FRAMED`]). Where `{project-root}`
/// is read and where it is written (R96), then every capability BMAD
/// assumes with this turn's answer.
pub fn frame_lines(drive: &str, artifacts: &str, offered: &[&str]) -> Vec<String> {
    if !FRAMED.iter().any(|tool| offered.contains(tool)) {
        return Vec::new();
    }
    let config = if offered.contains(&BMAD_CONFIG) {
        " bmad_config answers the TOML configuration and lists each path key's read and write location."
    } else {
        ""
    };
    let mut lines = vec![
        format!(
            "BMAD's project root is {drive}'s root. Its install, `{{project-root}}/_bmad/`, is read there and never written; so are its modules' `config.yaml` files.{config}"
        ),
        format!(
            "Every other `{{project-root}}` path is read under {drive}'s root and written under this session's `{artifacts}/`."
        ),
        "What BMAD assumes, and keeper's answer here:".to_owned(),
    ];
    for capability in &CAPABILITIES {
        let answer = capability_answer(capability, offered);
        let tools: Vec<&str> = answer
            .tools
            .iter()
            .map(|tool| if *tool == MCP { "your MCP tools" } else { tool })
            .collect();
        let said = answer.sentences.join(" ");
        let said = match (tools.is_empty(), said.is_empty()) {
            (false, false) => format!("{}; {said}", tools.join(", ")),
            (false, true) => tools.join(", "),
            (true, false) => said,
            (true, true) => continue,
        };
        lines.push(format!("- {}: {said}", capability.assumes));
    }
    lines
}

/// What `bmad_config` reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigScope {
    /// BMAD's central configuration, the four layers of `_bmad/`
    /// (`resolve_config.py`).
    Central,
    /// A skill's `customize.toml` with the drive's overlays
    /// (`resolve_customization.py`): the offered skill `skill` under
    /// `_skills/`, or, without one, the workflow the session runs.
    Customization { skill: Option<String> },
}

/// One `bmad_config` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigCall {
    pub scope: ConfigScope,
    /// The resolvers' `--key`s: dotted keys, in the order asked; `None`
    /// is the whole table.
    pub keys: Option<Vec<String>>,
}

/// Which of `resolve_party.py`'s three projections `bmad_party` prints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartyCall {
    /// The room to load on entry.
    Roster,
    /// `--list-groups`.
    Groups,
    /// `--party <id>`.
    Group(String),
}

/// One `skill_view` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewCall {
    /// The skill, as `skills_list` names it.
    pub name: String,
    /// A file inside the skill's folder; `None` is its `SKILL.md`.
    pub path: Option<String>,
}

/// One `bmad_render` call: the format-B skill to render — the offered
/// skill `skill` under `_skills/`, or, without one, the workflow the
/// session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderCall {
    pub skill: Option<String>,
}

/// Which memlog a `bmad_memlog` call writes, as `memlog.py` addresses it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemlogTarget {
    /// `--workspace`: the run folder; the memlog is `<folder>/.memlog.md`.
    Workspace(String),
    /// `--path`: the memlog file itself.
    Path(String),
}

/// `memlog.py`'s three commands and their flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemlogCommand {
    /// `init --field key=value …`.
    Init { fields: Vec<String> },
    /// `append --text … [--type …] [--by …]`.
    Append {
        text: String,
        entry_type: Option<String>,
        by: Option<String>,
    },
    /// `set --key … --value …`.
    Set { key: String, value: String },
}

/// One `bmad_memlog` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemlogCall {
    pub target: MemlogTarget,
    pub command: MemlogCommand,
}

/// The specs of this module's tools that `allow` names, in the
/// vocabulary's order.
pub fn specs(allow: &[String]) -> Vec<ToolSpec> {
    let allowed = |name: &str| allow.iter().any(|allowed| allowed == name);
    let mut specs = Vec::new();
    if allowed(SKILLS_LIST) {
        specs.push(ToolSpec {
            name: SKILLS_LIST.to_owned(),
            description: "List the skills offered to you, by name and purpose, and every skill in the drive's _skills/ that is not offered, with the reason.".to_owned(),
            parameters: json!({"type": "object", "properties": {}, "additionalProperties": false}),
        });
    }
    if allowed(SKILL_VIEW) {
        specs.push(ToolSpec {
            name: SKILL_VIEW.to_owned(),
            description: "Load an offered skill's instructions (its SKILL.md), or one file inside the skill's folder, to follow it here.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "The skill's name, as skills_list gives it."},
                    "path": {"type": "string", "description": "A file inside the skill's folder, e.g. references/a.md. Without it, SKILL.md."}
                },
                "required": ["name"],
                "additionalProperties": false
            }),
        });
    }
    if allowed(BMAD_CONFIG) {
        specs.push(ToolSpec {
            name: BMAD_CONFIG.to_owned(),
            description: "BMAD's configuration, as its resolve_config.py and resolve_customization.py print it, read from the drive's _bmad/: the central configuration with where each {project-root} path is read and written, or a skill's customization with the drive's overlays.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "enum": ["central", "customization"], "description": "central: the four layers of _bmad/. customization: a skill's customize.toml and its _bmad/custom/ overlays."},
                    "skill": {"type": "string", "description": "customization only: an offered skill under _skills/. Without it, the workflow this session runs."},
                    "keys": {"type": "array", "items": {"type": "string"}, "description": "Dotted keys to print, e.g. [\"agents\"] or [\"workflow\"]; a key not found is left out. Without it, everything."}
                },
                "required": ["scope"],
                "additionalProperties": false
            }),
        });
    }
    if allowed(BMAD_RENDER) {
        specs.push(ToolSpec {
            name: BMAD_RENDER.to_owned(),
            description: "Render a format-B BMAD skill as its render_skill.py does, with the drive's BMAD configuration and this session's write location for {project-root}: publishes it once under this session's workspace/bmad-render/ and answers \"read and follow <path>/workflow.md\", or \"HALT: <why>\" — then stop as the skill says.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "skill": {"type": "string", "description": "An offered skill under _skills/. Without it, the workflow this session runs."}
                },
                "additionalProperties": false
            }),
        });
    }
    if allowed(BMAD_MEMLOG) {
        specs.push(ToolSpec {
            name: BMAD_MEMLOG.to_owned(),
            description: "BMAD's memlog.py inside this session: init, append to or set a field of a run's .memlog.md under artifacts/, written atomically; answers memlog.py's one-line acknowledgement.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "enum": ["init", "append", "set"]},
                    "workspace": {"type": "string", "description": "The run folder under artifacts/; the memlog is its .memlog.md (--workspace). Session-relative, or drive-relative as bmad_config's write locations give it."},
                    "path": {"type": "string", "description": "The memlog file itself, named .memlog.md, instead of workspace (--path)."},
                    "fields": {"type": "array", "items": {"type": "string"}, "description": "init: frontmatter fields, each key=value (--field)."},
                    "text": {"type": "string", "description": "append: the entry; whitespace runs collapse to one line (--text)."},
                    "type": {"type": "string", "description": "append: the entry's kind, rendered as a tag (--type)."},
                    "by": {"type": "string", "description": "append: who the entry came from (--by)."},
                    "key": {"type": "string", "description": "set: the frontmatter field (--key)."},
                    "value": {"type": "string", "description": "set: its value (--value)."}
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        });
    }
    if allowed(BMAD_PARTY) {
        specs.push(ToolSpec {
            name: BMAD_PARTY.to_owned(),
            description: "BMAD's party roster, as its resolve_party.py prints it for this drive's install: the room to load on entry, the menu of groups, or one group in full.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "list_groups": {"type": "boolean", "description": "Only the menu of groups (--list-groups)."},
                    "party": {"type": "string", "description": "One group by id, in full (--party)."}
                },
                "additionalProperties": false
            }),
        });
    }
    specs
}

/// The arguments' object, refusing a key the tool does not take.
fn object<'a>(
    tool: &str,
    args: &'a Value,
    takes: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);
    let keys = match args {
        Value::Null => return Ok(&EMPTY),
        Value::Object(keys) => keys,
        _ => return Err(format!("{tool} takes an object of arguments.")),
    };
    if let Some(other) = keys.keys().find(|key| !takes.contains(&key.as_str())) {
        let taken = match takes {
            [] => "no arguments".to_owned(),
            [one] => one.to_string(),
            [init @ .., last] => format!("{} and {last}", init.join(", ")),
        };
        return Err(format!("{tool} takes {taken}; {other} is none of them."));
    }
    Ok(keys)
}

/// A non-empty string argument, when given.
fn text(tool: &str, keys: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match keys.get(key) {
        None => Ok(None),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(Some(text.clone())),
        Some(_) => Err(format!("{tool}'s {key} is a non-empty string.")),
    }
}

/// Read a `bmad_config` call's arguments.
pub fn parse_config(args: &Value) -> Result<ConfigCall, String> {
    let keys = object(BMAD_CONFIG, args, &["scope", "skill", "keys"])?;
    let skill = text(BMAD_CONFIG, keys, "skill")?;
    let scope = match keys.get("scope").and_then(Value::as_str) {
        Some("central") if skill.is_some() => {
            return Err("bmad_config's skill names a skill whose customization is read; the central configuration is no skill's.".to_owned())
        }
        Some("central") => ConfigScope::Central,
        Some("customization") => ConfigScope::Customization { skill },
        _ => return Err("bmad_config needs scope: \"central\" or \"customization\".".to_owned()),
    };
    let keys = match keys.get("keys") {
        None => None,
        Some(Value::Array(items)) if !items.is_empty() => Some(
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .filter(|key| !key.is_empty())
                        .map(str::to_owned)
                        .ok_or_else(|| {
                            "bmad_config's keys are dotted keys, each a non-empty string."
                                .to_owned()
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => return Err(
            "bmad_config's keys is a list of at least one dotted key; leave it out for everything."
                .to_owned(),
        ),
    };
    Ok(ConfigCall { scope, keys })
}

/// Read a `bmad_render` call's arguments.
pub fn parse_render(args: &Value) -> Result<RenderCall, String> {
    let keys = object(BMAD_RENDER, args, &["skill"])?;
    Ok(RenderCall {
        skill: text(BMAD_RENDER, keys, "skill")?,
    })
}

/// Read a `bmad_memlog` call's arguments as `memlog.py`'s argparse reads
/// its command line: one of `workspace` and `path`, and the command's own
/// flags only.
pub fn parse_memlog(args: &Value) -> Result<MemlogCall, String> {
    const TARGET: [&str; 3] = ["command", "workspace", "path"];
    let all = object(
        BMAD_MEMLOG,
        args,
        &[
            "command",
            "workspace",
            "path",
            "fields",
            "text",
            "type",
            "by",
            "key",
            "value",
        ],
    )?;
    let command = all.get("command").and_then(Value::as_str);
    let flags: &[&str] = match command {
        Some("init") => &["fields"],
        Some("append") => &["text", "type", "by"],
        Some("set") => &["key", "value"],
        _ => return Err("bmad_memlog needs command: \"init\", \"append\" or \"set\".".to_owned()),
    };
    let command = command.unwrap_or_default();
    if let Some(other) = all
        .keys()
        .find(|key| !TARGET.contains(&key.as_str()) && !flags.contains(&key.as_str()))
    {
        return Err(format!(
            "bmad_memlog's {command} takes {}; {other} is none of them.",
            flags.join(", ")
        ));
    }
    let target =
        match (
            text(BMAD_MEMLOG, all, "workspace")?,
            text(BMAD_MEMLOG, all, "path")?,
        ) {
            (Some(folder), None) => MemlogTarget::Workspace(folder),
            (None, Some(file)) => MemlogTarget::Path(file),
            _ => return Err(
                "bmad_memlog needs one of workspace (the run folder) and path (the memlog file)."
                    .to_owned(),
            ),
        };
    let string = |key: &str| match all.get(key) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("bmad_memlog's {key} is a string.")),
    };
    let required =
        |key: &str| string(key)?.ok_or_else(|| format!("bmad_memlog's {command} needs {key}."));
    let command = match command {
        "init" => MemlogCommand::Init {
            fields: match all.get("fields") {
                None => Vec::new(),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| {
                        "bmad_memlog's fields are strings, each key=value.".to_owned()
                    })?,
                Some(_) => {
                    return Err("bmad_memlog's fields is a list of key=value strings.".to_owned())
                }
            },
        },
        "append" => MemlogCommand::Append {
            text: required("text")?,
            entry_type: string("type")?.filter(|kind| !kind.is_empty()),
            by: string("by")?.filter(|by| !by.is_empty()),
        },
        _ => MemlogCommand::Set {
            key: text(BMAD_MEMLOG, all, "key")?
                .ok_or_else(|| "bmad_memlog's set needs key.".to_owned())?,
            value: required("value")?,
        },
    };
    Ok(MemlogCall { target, command })
}

/// Read a `bmad_party` call's arguments. As `resolve_party.py` reads its
/// flags: `list_groups` wins over `party`, and an empty `party` is none.
pub fn parse_party(args: &Value) -> Result<PartyCall, String> {
    let keys = object(BMAD_PARTY, args, &["list_groups", "party"])?;
    let list_groups = match keys.get("list_groups") {
        None => false,
        Some(Value::Bool(on)) => *on,
        Some(_) => return Err("bmad_party's list_groups is true or false.".to_owned()),
    };
    let party = match keys.get("party") {
        None => None,
        Some(Value::String(id)) => Some(id.clone()).filter(|id| !id.is_empty()),
        Some(_) => return Err("bmad_party's party is a group's id.".to_owned()),
    };
    Ok(match (list_groups, party) {
        (true, _) => PartyCall::Groups,
        (false, Some(id)) => PartyCall::Group(id),
        (false, None) => PartyCall::Roster,
    })
}

/// Read a `skills_list` call's arguments: it takes none.
pub fn parse_list(args: &Value) -> Result<(), String> {
    object(SKILLS_LIST, args, &[]).map(drop)
}

/// Read a `skill_view` call's arguments.
pub fn parse_view(args: &Value) -> Result<ViewCall, String> {
    let keys = object(SKILL_VIEW, args, &["name", "path"])?;
    let name = text(SKILL_VIEW, keys, "name")?
        .ok_or_else(|| "skill_view needs the skill's name.".to_owned())?;
    Ok(ViewCall {
        name,
        path: text(SKILL_VIEW, keys, "path")?,
    })
}

/// One `ask_human` call: the question, the answers it offers in order, and
/// the one an unattended run takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskCall {
    pub question: String,
    pub choices: Vec<String>,
    pub default: Option<String>,
}

/// `ask_human`'s spec: offered by the session's kind, not by `[tools].allow`
/// (R102).
pub fn ask_spec() -> ToolSpec {
    ToolSpec {
        name: ASK_HUMAN.to_owned(),
        description: "Ask the person this work is for a question — a HALT, a menu, a checkpoint — through their proxy, who asks them in its own words. This ends your turn: say what you are waiting for and stop; their answer arrives as the next message, naming the choice it picks. Where nobody can be asked, the default is the answer at once.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "The question, whole: the person reads nothing else of this session."},
                "choices": {"type": "array", "items": {"type": "string"}, "description": "The answers it offers, e.g. [\"Continue\", \"Stop\"]; an answer picks one by its number or its text."},
                "default": {"type": "string", "description": "The answer when nobody can be asked: one of choices, when there are choices."}
            },
            "required": ["question"],
            "additionalProperties": false
        }),
    }
}

/// The most bytes an ask's question, choices and default take together: its
/// event carries them twice — the body a device shows and the ask — and is
/// encrypted under the homeserver's 64 KiB cap ([`crate::agents::ask::ASK_EVENT_BYTES`]).
pub const ASK_TEXT_BYTES: usize = 12 * 1024;

/// Read an `ask_human` call's arguments: a question; distinct choices, each
/// told apart from the others however it is cased; a default among them
/// when both are given; all of it within [`ASK_TEXT_BYTES`].
pub fn parse_ask(args: &Value) -> Result<AskCall, String> {
    let keys = object(ASK_HUMAN, args, &["question", "choices", "default"])?;
    let question = text(ASK_HUMAN, keys, "question")?
        .ok_or_else(|| "ask_human needs the question.".to_owned())?;
    let choices: Vec<String> = match keys.get("choices") {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::String(choice) if !choice.trim().is_empty() => Ok(choice.trim().to_owned()),
                _ => Err("ask_human's choices are non-empty strings.".to_owned()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err("ask_human's choices are a list of strings.".to_owned()),
    };
    let mut folded: Vec<String> = choices.iter().map(|choice| choice.to_lowercase()).collect();
    folded.sort();
    if folded.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("ask_human's choices are each different, however they are cased.".to_owned());
    }
    let default = text(ASK_HUMAN, keys, "default")?.map(|default| default.trim().to_owned());
    if let Some(default) = &default {
        if !choices.is_empty() && !choices.contains(default) {
            return Err(format!(
                "ask_human's default is one of its choices, and {default} is none of them."
            ));
        }
    }
    let bytes = question.len()
        + choices.iter().map(String::len).sum::<usize>()
        + default.as_ref().map_or(0, String::len);
    if bytes > ASK_TEXT_BYTES {
        return Err(format!(
            "ask_human's question, choices and default take {bytes} bytes, and an ask carries at most {ASK_TEXT_BYTES}: ask a shorter question."
        ));
    }
    Ok(AskCall {
        question,
        choices,
        default,
    })
}

/// Opens a workflow's run in a session of its own (R104).
pub const WORKFLOW_START: &str = "workflow_start";
/// The header a `_workflows/<name>/` folder carries.
pub const WORKFLOW_FILE: &str = "workflow.toml";
/// The longest `description`, in characters.
pub const DESCRIPTION_MAX: usize = 280;
/// A folder of `_workflows/` without its header.
pub const NOT_A_WORKFLOW: &str = "not a workflow: no workflow.toml";
/// `workflow_start` in a proxy's own `main` or `conversation` (AD-380).
pub const IN_THE_DM: &str = "a workflow is started by delegation or a card, never inside the DM";
/// The continuations one run may take (R106).
pub const CONTINUATIONS_PER_RUN: u32 = 3;
/// What the host says when it continues a run whose rounds ran out (R106).
pub const CONTINUE: &str = "continue";

/// A `[[inputs]]` entry's `type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    /// Free text.
    Text,
    /// A path in a drive in scope.
    Path,
    /// A drive in scope, by id.
    Drive,
    /// An existing session, by id.
    Session,
}

impl InputKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Path => "path",
            Self::Drive => "drive",
            Self::Session => "session",
        }
    }
}

/// One `[[inputs]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowInput {
    pub name: String,
    pub kind: InputKind,
    pub required: bool,
}

/// One `[[outputs]]` entry: `path` is under the session's `artifacts/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowOutput {
    pub name: String,
    pub path: String,
}

/// `[trigger]` (R108): `schedule` is a card's, never the header's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigger {
    /// `workflow_start` and a menu may start it.
    pub manual: bool,
    /// A workflow card may start it.
    pub card: bool,
}

/// A read `workflow.toml` (AD-398, the architecture's *Data formats*).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    /// A file inside the folder.
    pub entry: String,
    pub inputs: Vec<WorkflowInput>,
    pub outputs: Vec<WorkflowOutput>,
    /// Names of the vocabulary the run needs.
    pub tools: Vec<String>,
    /// Drive ids, or `home`.
    pub drives: Vec<String>,
    pub trigger: Trigger,
    pub checkpoints: Checkpoints,
}

const WORKFLOW_KEYS: [&str; 10] = [
    "version",
    "name",
    "description",
    "entry",
    "inputs",
    "outputs",
    "tools",
    "drives",
    "trigger",
    "checkpoints",
];

/// Whether `path` is a plain relative path: no root, no `.` or `..`, no
/// empty segment, no backslash.
fn plain(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// `[a-z0-9][a-z0-9-]{0,31}`: a drive's id.
fn drive_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && bytes[0] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// The `{{…}}` tokens of `path`, or `Err` for one never closed.
fn tokens(path: &str) -> Result<Vec<&str>, ()> {
    let mut found = Vec::new();
    let mut rest = path;
    while let Some(at) = rest.find("{{") {
        let after = &rest[at + 2..];
        let end = after.find("}}").ok_or(())?;
        found.push(&after[..end]);
        rest = &after[end + 2..];
    }
    Ok(found)
}

/// Read `_workflows/<folder>/workflow.toml`'s `text`; `has_file` says
/// whether a folder-relative path is a file of the folder. Closed: every
/// refusal is one sentence naming the key.
pub fn parse_workflow_toml(
    folder: &str,
    text: &str,
    has_file: &dyn Fn(&str) -> bool,
) -> Result<Workflow, String> {
    use toml::Value as T;
    let say = |why: String| format!("_workflows/{folder}/{WORKFLOW_FILE}: {why}");
    let table: toml::Table = crate::toml_order::from_str(text).map_err(|error| {
        say(format!(
            "it is not valid TOML: {}",
            error.message().lines().next().unwrap_or("")
        ))
    })?;
    if let Some(key) = table
        .keys()
        .find(|key| !WORKFLOW_KEYS.contains(&key.as_str()))
    {
        return Err(say(format!("`{key}` is not one of its keys.")));
    }
    let string = |key: &str| -> Result<Option<String>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(T::String(value)) => Ok(Some(value.clone())),
            Some(_) => Err(say(format!("`{key}` is text in quotes."))),
        }
    };
    let strings = |key: &str| -> Result<Option<Vec<String>>, String> {
        match table.get(key) {
            None => Ok(None),
            Some(T::Array(items)) => items
                .iter()
                .map(|item| match item {
                    T::String(value) => Ok(value.clone()),
                    _ => Err(say(format!("`{key}` is a list of text."))),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(say(format!("`{key}` is a list of text."))),
        }
    };
    let tables = |key: &str| -> Result<Vec<toml::Table>, String> {
        match table.get(key) {
            None => Ok(Vec::new()),
            Some(T::Array(items)) => items
                .iter()
                .map(|item| match item {
                    T::Table(entry) => Ok(entry.clone()),
                    _ => Err(say(format!("`{key}` is a list of tables, [[{key}]]."))),
                })
                .collect(),
            Some(_) => Err(say(format!("`{key}` is a list of tables, [[{key}]]."))),
        }
    };
    let closed = |entry: &toml::Table, key: &str, keys: &[&str]| -> Result<(), String> {
        match entry.keys().find(|k| !keys.contains(&k.as_str())) {
            Some(other) => Err(say(format!("`{key}.{other}` is not one of its keys."))),
            None => Ok(()),
        }
    };
    let entry_text = |entry: &toml::Table, key: &str, field: &str| -> Result<String, String> {
        match entry.get(field) {
            Some(T::String(value)) if !value.trim().is_empty() => Ok(value.clone()),
            Some(_) => Err(say(format!("`{key}.{field}` is non-empty text."))),
            None => Err(say(format!("each of `{key}` needs `{field}`."))),
        }
    };

    match table.get("version") {
        Some(T::Integer(1)) => {}
        Some(T::Integer(version)) => {
            return Err(say(format!(
                "it is version {version}, which this keeper cannot read: it reads version 1."
            )))
        }
        Some(_) => return Err(say("`version` is a whole number.".to_owned())),
        None => return Err(say("it needs `version`.".to_owned())),
    }
    let name = string("name")?.ok_or_else(|| say("it needs `name`.".to_owned()))?;
    if name != folder {
        return Err(say(format!(
            "`name` is {name}, but a workflow is named by its folder, {folder}."
        )));
    }
    let description =
        string("description")?.ok_or_else(|| say("it needs `description`.".to_owned()))?;
    let chars = description.chars().count();
    if chars > DESCRIPTION_MAX {
        return Err(say(format!(
            "`description` is {chars} characters long; it is at most {DESCRIPTION_MAX}."
        )));
    }
    let entry = string("entry")?.unwrap_or_else(|| "SKILL.md".to_owned());
    if !plain(&entry) {
        return Err(say(format!(
            "`entry` is {entry}, which is not a path inside the folder."
        )));
    }
    if !has_file(&entry) {
        return Err(say(format!(
            "`entry` is {entry}, which is not a file in the folder."
        )));
    }
    let mut inputs = Vec::new();
    for input in tables("inputs")? {
        closed(&input, "inputs", &["name", "type", "required"])?;
        let name = entry_text(&input, "inputs", "name")?;
        let word = entry_text(&input, "inputs", "type")?;
        let kind = match word.as_str() {
            "text" => InputKind::Text,
            "path" => InputKind::Path,
            "drive" => InputKind::Drive,
            "session" => InputKind::Session,
            _ => {
                return Err(say(format!(
                    "the input {name}'s type is {word}; it is one of text, path, drive, session."
                )))
            }
        };
        let required = match input.get("required") {
            None => false,
            Some(T::Boolean(required)) => *required,
            Some(_) => return Err(say("`inputs.required` is true or false.".to_owned())),
        };
        if inputs
            .iter()
            .any(|known: &WorkflowInput| known.name == name)
        {
            return Err(say(format!("the input {name} is declared twice.")));
        }
        inputs.push(WorkflowInput {
            name,
            kind,
            required,
        });
    }
    let mut outputs = Vec::new();
    for output in tables("outputs")? {
        closed(&output, "outputs", &["name", "path"])?;
        let name = entry_text(&output, "outputs", "name")?;
        let path = entry_text(&output, "outputs", "path")?;
        let found = tokens(&path).map_err(|()| {
            say(format!(
                "the output {name}'s path {path} opens a token it never closes."
            ))
        })?;
        if let Some(other) = found
            .iter()
            .find(|token| !matches!(**token, "date" | "slug"))
        {
            return Err(say(format!(
                "the output {name}'s path names {{{{{other}}}}}; only {{{{date}}}} and {{{{slug}}}} are filled in."
            )));
        }
        if !plain(&path) {
            return Err(say(format!(
                "the output {name}'s path {path} is not a path under the session's artifacts/."
            )));
        }
        outputs.push(WorkflowOutput { name, path });
    }
    let tools = strings("tools")?.unwrap_or_default();
    if let Some(other) = tools
        .iter()
        .find(|tool| !crate::agents::home::TOOL_VOCABULARY.contains(&tool.as_str()))
    {
        return Err(say(format!(
            "`tools` names {other}, which is not a tool of the agents' vocabulary."
        )));
    }
    let drives = strings("drives")?.unwrap_or_else(|| vec!["home".to_owned()]);
    if let Some(bad) = drives
        .iter()
        .find(|drive| *drive != "home" && !drive_id(drive))
    {
        return Err(say(format!(
            "`drives` names {bad}, which is neither a drive id nor home."
        )));
    }
    let trigger = match table.get("trigger") {
        None => Trigger {
            manual: true,
            card: true,
        },
        Some(T::Table(trigger)) => {
            if trigger.contains_key("schedule") {
                return Err(say("`trigger.schedule` is not read: a schedule is a workflow card's `schedule:`, never the workflow's.".to_owned()));
            }
            closed(trigger, "trigger", &["manual", "card"])?;
            let flag = |key: &str| match trigger.get(key) {
                None => Ok(true),
                Some(T::Boolean(on)) => Ok(*on),
                Some(_) => Err(say(format!("`trigger.{key}` is true or false."))),
            };
            Trigger {
                manual: flag("manual")?,
                card: flag("card")?,
            }
        }
        Some(_) => return Err(say("`trigger` is a table, [trigger].".to_owned())),
    };
    let checkpoints = match string("checkpoints")? {
        None => Checkpoints::Proxy,
        Some(word) => word.parse().map_err(|()| {
            say(format!(
                "`checkpoints` is {word}; it is proxy or unattended."
            ))
        })?,
    };
    Ok(Workflow {
        name,
        description,
        entry,
        inputs,
        outputs,
        tools,
        drives,
        trigger,
        checkpoints,
    })
}

/// Whether `agent`'s session may run `workflow`, the run offered `offered`
/// (wire names) and in scope `scope`, its home drive `home`: every tool it
/// names is offered ([`tools_check`]), and every drive it works in is in
/// scope ([`run_drives`]). `Ok` holds the run's drives, its home first.
pub fn start_check(
    workflow: &Workflow,
    agent: &str,
    offered: &[&str],
    scope: &[String],
    home: &str,
) -> Result<Vec<String>, String> {
    tools_check(workflow, agent, offered)?;
    run_drives(workflow, scope, home)
}

/// Whether every tool `workflow` names is among `offered`, the wire names a
/// turn of its run would be offered; else the sentence naming the first
/// that is not.
pub fn tools_check(workflow: &Workflow, agent: &str, offered: &[&str]) -> Result<(), String> {
    if let Some(tool) = workflow
        .tools
        .iter()
        .find(|tool| !offered.contains(&tool.as_str()))
    {
        return Err(format!(
            "`{}` needs `{tool}`, which `{agent}` is not allowed.",
            workflow.name
        ));
    }
    Ok(())
}

/// The drives `workflow`'s run works in, its home `home` first, each in
/// `scope`; else the sentence naming the first that is not.
pub fn run_drives(
    workflow: &Workflow,
    scope: &[String],
    home: &str,
) -> Result<Vec<String>, String> {
    let mut drives = vec![home.to_owned()];
    for drive in &workflow.drives {
        let drive = if drive == "home" { home } else { drive };
        if !scope.iter().any(|in_scope| in_scope == drive) {
            return Err(format!(
                "`{}` works in {drive}, which is not in this session's scope.",
                workflow.name
            ));
        }
        if !drives.iter().any(|known| known == drive) {
            drives.push(drive.to_owned());
        }
    }
    Ok(drives)
}

/// An output's path with its tokens filled: `{{date}}` the run's day
/// (`YYYY-MM-DD`), `{{slug}}` the workflow's name.
pub fn expand_output(path: &str, date: &str, slug: &str) -> String {
    path.replace("{{date}}", date).replace("{{slug}}", slug)
}

/// The sentence a reply carries for a declared output the run did not
/// write (R107).
pub fn missing_output(path: &str) -> String {
    format!("declared output `{path}` was not written")
}

fn derived(parts: &[&str]) -> ulid::Ulid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("keeper.agents.workflow\n{}", parts.join("\n")).as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    ulid::Ulid::from_bytes(bytes)
}

/// The run a workflow card opens in `window` (RFC 3339): the same on every
/// host for the card `card` of the session `session` (Q5).
pub fn run_id(session: &str, card: &str, window: &str) -> ulid::Ulid {
    derived(&[session, card, window])
}

/// The run a `workflow_start` call `call` of `session` opens: a call run
/// again — a replay, a resumed park — opens the same session (AD-368).
pub fn start_id(session: &str, call: &str) -> ulid::Ulid {
    derived(&[session, "call", call])
}

/// The delegation the card `card` of the session `session` (its id) is
/// handed on as when a run of another session hands it on: the same on
/// every host and in every run, so a card is handed on once (R202).
pub fn handoff_id(session: &str, card: &str) -> ulid::Ulid {
    derived(&[session, "handoff", card])
}

/// One `workflow_start` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartCall {
    pub name: String,
    /// Each input as named, its value as given.
    pub inputs: Vec<(String, String)>,
}

/// `workflow_start`'s spec: offered as `[tools].allow` says, never in a
/// proxy's own conversation.
pub fn start_spec() -> ToolSpec {
    ToolSpec {
        name: WORKFLOW_START.to_owned(),
        description: "Start a workflow of the drive's _workflows/ in a session of its own, by its folder's name, a menu code or a skill:action of the drive's bmad-help.csv. It runs there and replies to this session when it is done.".to_owned(),
        parameters: json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The workflow: its folder under _workflows/, or a BMAD menu code or skill:action naming it."},
                "inputs": {"type": "object", "additionalProperties": {"type": "string"}, "description": "The workflow's declared inputs, by name: text, a drive path, a drive id, or a session id."}
            },
            "required": ["name"],
            "additionalProperties": false
        }),
    }
}

/// Read a `workflow_start` call's arguments.
pub fn parse_start(args: &Value) -> Result<StartCall, String> {
    let keys = object(WORKFLOW_START, args, &["name", "inputs"])?;
    let name = text(WORKFLOW_START, keys, "name")?
        .ok_or_else(|| "workflow_start needs the workflow's name.".to_owned())?
        .trim()
        .to_owned();
    let inputs = match keys.get("inputs") {
        None => Vec::new(),
        Some(Value::Object(given)) => given
            .iter()
            .map(|(name, value)| match value {
                Value::String(value) => Ok((name.clone(), value.clone())),
                _ => Err(format!("workflow_start's input {name} is a string.")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => {
            return Err("workflow_start's inputs are an object of strings, by name.".to_owned())
        }
    };
    Ok(StartCall { name, inputs })
}

/// The inputs `given` checked against `workflow`'s declarations, in
/// declared order: none undeclared, every required one given, none empty.
pub fn check_inputs<'w>(
    workflow: &'w Workflow,
    given: &[(String, String)],
) -> Result<Vec<(&'w WorkflowInput, String)>, String> {
    if let Some((other, _)) = given
        .iter()
        .find(|(name, _)| !workflow.inputs.iter().any(|input| input.name == *name))
    {
        return Err(format!("`{}` declares no input {other}.", workflow.name));
    }
    let mut checked = Vec::new();
    for input in &workflow.inputs {
        match given.iter().find(|(name, _)| *name == input.name) {
            Some((_, value)) if value.trim().is_empty() => {
                return Err(format!(
                    "`{}`'s input {} is empty.",
                    workflow.name, input.name
                ))
            }
            Some((_, value)) => checked.push((input, value.trim().to_owned())),
            None if input.required => {
                return Err(format!(
                    "`{}` needs the input {} ({}).",
                    workflow.name,
                    input.name,
                    input.kind.as_str()
                ))
            }
            None => {}
        }
    }
    Ok(checked)
}

/// The run's brief — its card's body: the workflow, where its entry is
/// (`folder`, drive-relative), its inputs, and the outputs it declares.
pub fn brief(workflow: &Workflow, folder: &str, inputs: &[(&WorkflowInput, String)]) -> String {
    let mut text = format!(
        "Run the workflow {}: {}\nRead and follow {folder}/{}.",
        workflow.name, workflow.description, workflow.entry
    );
    if !inputs.is_empty() {
        text.push_str("\n\nInputs:");
        for (input, value) in inputs {
            text.push_str(&format!(
                "\n- {} ({}): {value}",
                input.name,
                input.kind.as_str()
            ));
        }
    }
    if !workflow.outputs.is_empty() {
        text.push_str("\n\nIt writes, under artifacts/:");
        for output in &workflow.outputs {
            text.push_str(&format!("\n- {}: {}", output.name, output.path));
        }
    }
    text.push_str("\n\nWhen the run is done, reply.");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::home::TOOL_VOCABULARY;

    /// The row of `assumes`.
    fn row(assumes: &str) -> &'static Capability {
        CAPABILITIES
            .iter()
            .find(|row| row.assumes == assumes)
            .expect("a row")
    }

    /// The vocabulary names a sentence names, as it names a tool: in
    /// backticks.
    fn named(sentence: &str) -> Vec<&str> {
        sentence
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|name| TOOL_VOCABULARY.contains(name))
            .collect()
    }

    /// 94.2 acceptance 1: G4 §5's rows, in order, each answered for every
    /// offer by the tools of AD-397's vocabulary it is offered or by a
    /// sentence — never by neither — and no sentence tells the model to use
    /// a tool the turn is not offered: a tool it names is offered, or the
    /// answer says it is not (R195).
    #[test]
    fn every_bmad_capability_is_answered_for_every_offer() {
        let assumed: Vec<&str> = CAPABILITIES.iter().map(|row| row.assumes).collect();
        assert_eq!(
            assumed,
            [
                "read a whole file or a range",
                "write files; edit YAML frontmatter in place",
                "list / glob",
                "grep the code; read `git log`",
                "run commands (`uv run`, Python ≥ 3.11)",
                "git `rev-parse`, diff to a temp file, commit",
                "run tests / linters",
                "ask the user and wait (HALT, menus)",
                "invoke a skill by name, forwarding intent",
                "spawn a sync or parallel context-free subagent",
                "re-address a live subagent by id",
                "agent teams + a capability probe",
                "per-agent model choice",
                "web search",
                "MCP / external systems",
                "headless/TTY detection, environment variables",
                "open an editor or an HTML report",
                "token counting, the current date",
                "session lifecycle hooks, tmux",
            ]
        );
        for row in &CAPABILITIES {
            for tool in row.tools {
                assert!(
                    *tool == MCP || TOOL_VOCABULARY.contains(tool),
                    "{tool} is not in the vocabulary ({})",
                    row.assumes
                );
            }
        }

        let unrelated = "mcp__github__get_me";
        let everything: Vec<&str> = TOOL_VOCABULARY.iter().copied().chain([unrelated]).collect();
        let mut offers: Vec<Vec<&str>> = vec![Vec::new(), vec![unrelated], everything];
        offers.extend(TOOL_VOCABULARY.iter().map(|tool| vec![*tool]));
        offers.push(vec![SKILL_VIEW, "workflow_start"]);
        for offered in &offers {
            for row in &CAPABILITIES {
                let answer = capability_answer(row, offered);
                let at = format!("{} offered {offered:?}", row.assumes);
                assert!(
                    !answer.tools.is_empty() || !answer.sentences.is_empty(),
                    "{at}"
                );
                for tool in &answer.tools {
                    assert!(is_offered(tool, offered), "{tool}: {at}");
                }
                for tool in &answer.missing {
                    assert!(!is_offered(tool, offered), "{tool}: {at}");
                }
                // A sentence that names a tool the turn is not offered is
                // one that says so: it is said only while that tool is
                // missing.
                for sentence in &answer.sentences {
                    let (when, _) = row
                        .said
                        .iter()
                        .find(|(_, said)| said == sentence)
                        .expect("a sentence of the row");
                    for tool in named(sentence) {
                        let says_missing =
                            matches!(when, When::Without(absent) if absent.contains(&tool));
                        assert!(
                            offered.contains(&tool)
                                || (says_missing && answer.missing.contains(&tool)),
                            "{sentence:?} names {tool}: {at}"
                        );
                    }
                }
            }
        }

        // Inline invocation and the workflow handoff are answered apart.
        let invoke = row("invoke a skill by name, forwarding intent");
        for (offered, tools, missing) in [
            (
                vec![BMAD_CONFIG],
                vec![],
                vec![SKILL_VIEW, "workflow_start"],
            ),
            (vec![SKILL_VIEW], vec![SKILL_VIEW], vec!["workflow_start"]),
            (
                vec!["workflow_start"],
                vec!["workflow_start"],
                vec![SKILL_VIEW],
            ),
            (
                vec![SKILL_VIEW, "workflow_start"],
                vec![SKILL_VIEW, "workflow_start"],
                vec![],
            ),
        ] {
            let answer = capability_answer(invoke, &offered);
            assert_eq!(answer.tools, tools, "{offered:?}");
            assert_eq!(answer.missing, missing, "{offered:?}");
        }
        assert_eq!(
            capability_answer(row("agent teams + a capability probe"), &[BMAD_CONFIG]).missing,
            ["delegate"]
        );

        // An MCP tool is an MCP tool, never a web search.
        assert_eq!(
            capability_answer(row("MCP / external systems"), &[unrelated]).tools,
            [MCP]
        );
        let search = row("web search");
        assert_eq!(
            capability_answer(search, &[unrelated]),
            capability_answer(search, &[])
        );
        assert!(capability_answer(search, &[unrelated]).tools.is_empty());
    }

    /// The frame tells a turn offered a tool through which it follows a
    /// BMAD skill or workflow where `{project-root}` is read and written
    /// (R96) and answers every capability for its offer; a turn offered
    /// none is told nothing of BMAD.
    #[test]
    fn the_frame_states_the_roots_and_the_map_for_its_offer() {
        let artifacts = "60-sessions/active/s/artifacts";
        assert!(frame_lines("tgdrive", artifacts, &["drive_read", "session_write"]).is_empty());
        for framed in FRAMED {
            let lines = frame_lines("tgdrive", artifacts, &["drive_read", framed]);
            let frame = lines.join("\n");
            assert!(frame.contains("tgdrive's root"), "{framed}: {frame}");
            assert!(
                frame.contains(&format!("`{artifacts}/`")),
                "{framed}: {frame}"
            );
            for capability in &CAPABILITIES {
                let answering = lines
                    .iter()
                    .filter(|line| line.contains(capability.assumes))
                    .count();
                assert_eq!(answering, 1, "{framed}: {}", capability.assumes);
            }
        }
        // bmad_config is pointed to only where it is offered.
        let skill_only = frame_lines("tgdrive", artifacts, &[SKILL_VIEW]).join("\n");
        assert!(!skill_only.contains(BMAD_CONFIG), "{skill_only}");
        let with_config = frame_lines("tgdrive", artifacts, &[SKILL_VIEW, BMAD_CONFIG]).join("\n");
        assert!(with_config.contains(BMAD_CONFIG), "{with_config}");
    }

    #[test]
    fn the_tools_arguments_read_as_their_scripts_flags() {
        assert_eq!(
            parse_config(&json!({"scope": "central"})),
            Ok(ConfigCall {
                scope: ConfigScope::Central,
                keys: None
            })
        );
        assert_eq!(
            parse_config(&json!({"scope": "central", "keys": ["agents", "core.user_name"]})),
            Ok(ConfigCall {
                scope: ConfigScope::Central,
                keys: Some(vec!["agents".to_owned(), "core.user_name".to_owned()])
            })
        );
        assert_eq!(
            parse_config(
                &json!({"scope": "customization", "skill": "bmad-architecture", "keys": ["workflow"]})
            ),
            Ok(ConfigCall {
                scope: ConfigScope::Customization {
                    skill: Some("bmad-architecture".to_owned())
                },
                keys: Some(vec!["workflow".to_owned()])
            })
        );
        assert!(parse_config(&json!({"scope": "central", "skill": "x"})).is_err());
        assert!(parse_config(&json!({"scope": "all"})).is_err());
        assert!(parse_config(&json!({"scope": "central", "keys": []})).is_err());
        assert!(parse_config(&json!({"scope": "central", "keys": [""]})).is_err());
        assert!(parse_config(&json!({"scope": "central", "key": "agents"})).is_err());

        assert_eq!(parse_party(&Value::Null), Ok(PartyCall::Roster));
        assert_eq!(parse_party(&json!({"party": ""})), Ok(PartyCall::Roster));
        assert_eq!(
            parse_party(&json!({"party": "code-review-crew"})),
            Ok(PartyCall::Group("code-review-crew".to_owned()))
        );
        assert_eq!(
            parse_party(&json!({"party": "code-review-crew", "list_groups": true})),
            Ok(PartyCall::Groups)
        );
        assert!(parse_party(&json!({"list_groups": "yes"})).is_err());

        assert_eq!(parse_list(&json!({})), Ok(()));
        assert!(parse_list(&json!({"name": "x"})).is_err());
        assert_eq!(
            parse_view(&json!({"name": "x", "path": "references/a.md"})),
            Ok(ViewCall {
                name: "x".to_owned(),
                path: Some("references/a.md".to_owned())
            })
        );
        assert!(parse_view(&json!({"path": "SKILL.md"})).is_err());

        assert_eq!(parse_render(&Value::Null), Ok(RenderCall { skill: None }));
        assert_eq!(
            parse_render(&json!({"skill": "bmad-build"})),
            Ok(RenderCall {
                skill: Some("bmad-build".to_owned())
            })
        );
        assert!(parse_render(&json!({"project_root": "/x"})).is_err());

        assert_eq!(
            parse_memlog(
                &json!({"command": "init", "workspace": "artifacts/run", "fields": ["topic=T"]})
            ),
            Ok(MemlogCall {
                target: MemlogTarget::Workspace("artifacts/run".to_owned()),
                command: MemlogCommand::Init {
                    fields: vec!["topic=T".to_owned()]
                }
            })
        );
        // `--type` and `--by` given empty are no tag, as `args.type or ""`.
        assert_eq!(
            parse_memlog(
                &json!({"command": "append", "path": "artifacts/run/.memlog.md", "text": "an idea", "type": "", "by": "user"})
            ),
            Ok(MemlogCall {
                target: MemlogTarget::Path("artifacts/run/.memlog.md".to_owned()),
                command: MemlogCommand::Append {
                    text: "an idea".to_owned(),
                    entry_type: None,
                    by: Some("user".to_owned())
                }
            })
        );
        assert_eq!(
            parse_memlog(&json!({"command": "set", "workspace": "w", "key": "phase", "value": ""})),
            Ok(MemlogCall {
                target: MemlogTarget::Workspace("w".to_owned()),
                command: MemlogCommand::Set {
                    key: "phase".to_owned(),
                    value: String::new()
                }
            })
        );
        // argparse's refusals: one target exactly, the command's own flags,
        // the required ones present.
        assert!(parse_memlog(&json!({"command": "init"})).is_err());
        assert!(parse_memlog(&json!({"command": "init", "workspace": "w", "path": "p"})).is_err());
        assert_eq!(
            parse_memlog(&json!({"command": "init", "workspace": "w", "text": "x"})),
            Err("bmad_memlog's init takes fields; text is none of them.".to_owned())
        );
        assert_eq!(
            parse_memlog(&json!({"command": "append", "workspace": "w"})),
            Err("bmad_memlog's append needs text.".to_owned())
        );
        assert!(parse_memlog(&json!({"command": "set", "workspace": "w", "key": "k"})).is_err());
        assert!(parse_memlog(&json!({"command": "drop", "workspace": "w"})).is_err());
    }

    /// 94.2 acceptance 7's grammar: a default beside choices is one of
    /// them; choices that differ only in case could not be told apart by an
    /// answer, so they are refused.
    #[test]
    fn ask_humans_default_is_one_of_its_choices() {
        assert_eq!(
            parse_ask(
                &json!({"question": "Go on?", "choices": ["Continue", "Stop"], "default": "Continue"})
            ),
            Ok(AskCall {
                question: "Go on?".to_owned(),
                choices: vec!["Continue".to_owned(), "Stop".to_owned()],
                default: Some("Continue".to_owned()),
            })
        );
        assert_eq!(
            parse_ask(
                &json!({"question": "Go on?", "choices": ["Continue", "Stop"], "default": "Later"})
            ),
            Err("ask_human's default is one of its choices, and Later is none of them.".to_owned())
        );
        // A free question takes any default.
        assert_eq!(
            parse_ask(&json!({"question": "Which file?", "default": "a.md"}))
                .map(|call| call.default),
            Ok(Some("a.md".to_owned()))
        );
        assert!(parse_ask(&json!({"question": "Go on?", "choices": ["Stop", "stop"]})).is_err());
        assert!(parse_ask(&json!({"choices": ["Continue"]})).is_err());
    }

    /// R94A-12: an ask's question, choices and default together fit in
    /// [`ASK_TEXT_BYTES`]; one byte more, in any of them, is refused before
    /// anything is asked.
    #[test]
    fn an_ask_is_bounded_before_it_is_asked() {
        let at = |question: usize, choice: usize, default: usize| {
            parse_ask(&json!({
                "question": "q".repeat(question),
                "choices": ["c".repeat(choice), "d".repeat(default)],
                "default": "d".repeat(default),
            }))
        };
        let third = ASK_TEXT_BYTES / 4;
        let rest = ASK_TEXT_BYTES - 3 * third;
        assert!(at(rest, third, third).is_ok());
        assert!(at(rest + 1, third, third).is_err());
        assert!(at(rest, third + 1, third).is_err());
        assert!(at(rest, third, third + 1).is_err());
    }

    const HEADER: &str = r#"version = 1
name = "bmad-build"
description = "Clarify, plan, implement, review and present."
entry = "workflow.md"
tools = ["drive_read", "session_write", "run"]
checkpoints = "unattended"

[[inputs]]
name = "intent"
type = "text"
required = true

[[inputs]]
name = "spec"
type = "path"

[[outputs]]
name = "spec"
path = "_bmad-output/implementation-artifacts/spec-{{slug}}-{{date}}.md"

[trigger]
card = false
"#;

    fn read(text: &str) -> Result<Workflow, String> {
        parse_workflow_toml("bmad-build", text, &|path| {
            ["SKILL.md", "workflow.md"].contains(&path)
        })
    }

    /// The refusal `text` gets, without its file prefix.
    fn why(text: &str) -> String {
        let refused = read(text).expect_err("refused");
        refused
            .strip_prefix("_workflows/bmad-build/workflow.toml: ")
            .expect("names the file")
            .to_owned()
    }

    fn with(line: &str, replacement: &str) -> String {
        assert!(HEADER.contains(line), "{line}");
        HEADER.replacen(line, replacement, 1)
    }

    /// 94.3 acceptance 1 (R108): the header is closed — one row per rule,
    /// each refused with its sentence — and its defaults are the
    /// architecture's.
    #[test]
    fn workflow_toml_grammar() {
        let read_whole = read(HEADER).expect("reads");
        assert_eq!(read_whole.entry, "workflow.md");
        assert_eq!(read_whole.inputs[0].kind, InputKind::Text);
        assert!(read_whole.inputs[0].required);
        assert!(!read_whole.inputs[1].required);
        assert_eq!(read_whole.checkpoints, Checkpoints::Unattended);
        assert_eq!(
            read_whole.trigger,
            Trigger {
                manual: true,
                card: false
            }
        );

        let minimal = read("version = 1\nname = \"bmad-build\"\ndescription = \"Build.\"\n")
            .expect("defaults");
        assert_eq!(minimal.entry, "SKILL.md");
        assert_eq!(minimal.drives, ["home"]);
        assert_eq!(
            minimal.trigger,
            Trigger {
                manual: true,
                card: true
            }
        );
        assert_eq!(minimal.checkpoints, Checkpoints::Proxy);
        assert!(
            minimal.inputs.is_empty() && minimal.outputs.is_empty() && minimal.tools.is_empty()
        );

        let rows: [(String, &str); 12] = [
            (with("version = 1\n", "version = 1\nmood = \"x\"\n"), "`mood` is not one of its keys."),
            (with("version = 1", "version = 2"), "it is version 2, which this keeper cannot read: it reads version 1."),
            (with("name = \"bmad-build\"", "name = \"build\""), "`name` is build, but a workflow is named by its folder, bmad-build."),
            (
                with(
                    "description = \"Clarify, plan, implement, review and present.\"",
                    &format!("description = \"{}\"", "d".repeat(DESCRIPTION_MAX + 1)),
                ),
                "`description` is 281 characters long; it is at most 280.",
            ),
            (with("entry = \"workflow.md\"", "entry = \"../other/SKILL.md\""), "`entry` is ../other/SKILL.md, which is not a path inside the folder."),
            (with("entry = \"workflow.md\"", "entry = \"steps/missing.md\""), "`entry` is steps/missing.md, which is not a file in the folder."),
            (with("type = \"path\"", "type = \"url\""), "the input spec's type is url; it is one of text, path, drive, session."),
            (with("{{slug}}-{{date}}", "{{slug}}-{{user}}"), "the output spec's path names {{user}}; only {{date}} and {{slug}} are filled in."),
            (with("\"run\"]", "\"shell\"]"), "`tools` names shell, which is not a tool of the agents' vocabulary."),
            (with("card = false", "schedule = \"@daily\""), "`trigger.schedule` is not read: a schedule is a workflow card's `schedule:`, never the workflow's."),
            (with("checkpoints = \"unattended\"", "checkpoints = \"never\""), "`checkpoints` is never; it is proxy or unattended."),
            (with("card = false", "card = false\nevery = 1"), "`trigger.every` is not one of its keys."),
        ];
        for (text, sentence) in rows {
            assert_eq!(why(&text), sentence);
        }
        assert!(read(&with(
            "description = \"Clarify, plan, implement, review and present.\"",
            &format!("description = \"{}\"", "d".repeat(DESCRIPTION_MAX))
        ))
        .is_ok());
    }

    /// 94.3 acceptance 2: a workflow whose tools the session is not offered,
    /// or whose drives are not in its scope, is refused for that agent by
    /// name; offered all, it runs in its drives, home first.
    #[test]
    fn a_workflow_is_refused_for_an_agent_lacking_its_tools() {
        let workflow = read(HEADER).expect("reads");
        let scope = vec!["tgdrive".to_owned(), "neuradrive".to_owned()];
        assert_eq!(
            start_check(
                &workflow,
                "amelia",
                &["drive_read", "session_write"],
                &scope,
                "tgdrive"
            ),
            Err("`bmad-build` needs `run`, which `amelia` is not allowed.".to_owned())
        );
        assert_eq!(
            start_check(
                &workflow,
                "amelia",
                &["drive_read", "session_write", "run"],
                &scope,
                "tgdrive"
            ),
            Ok(vec!["tgdrive".to_owned()])
        );
        let elsewhere = Workflow {
            drives: vec!["home".to_owned(), "neuradrive".to_owned()],
            tools: Vec::new(),
            ..workflow
        };
        assert_eq!(
            start_check(&elsewhere, "amelia", &[], &scope, "tgdrive"),
            Ok(vec!["tgdrive".to_owned(), "neuradrive".to_owned()])
        );
        assert_eq!(
            start_check(&elsewhere, "amelia", &[], &scope[..1], "tgdrive"),
            Err(
                "`bmad-build` works in neuradrive, which is not in this session's scope."
                    .to_owned()
            )
        );
    }

    /// 94.3 acceptance 3's pure half: undeclared, missing and empty inputs
    /// are refused; a run's id is the same for the same call or window.
    #[test]
    fn inputs_are_checked_and_runs_are_named_alike_everywhere() {
        let workflow = read(HEADER).expect("reads");
        let given = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(
            check_inputs(&workflow, &given(&[])).map(|_| ()),
            Err("`bmad-build` needs the input intent (text).".to_owned())
        );
        assert_eq!(
            check_inputs(&workflow, &given(&[("intent", "x"), ("colour", "red")])).map(|_| ()),
            Err("`bmad-build` declares no input colour.".to_owned())
        );
        assert_eq!(
            check_inputs(&workflow, &given(&[("intent", " ")])).map(|_| ()),
            Err("`bmad-build`'s input intent is empty.".to_owned())
        );
        let checked =
            check_inputs(&workflow, &given(&[("spec", "a.md"), ("intent", "x")])).expect("ok");
        assert_eq!(
            checked
                .iter()
                .map(|(input, value)| (input.name.as_str(), value.as_str()))
                .collect::<Vec<_>>(),
            [("intent", "x"), ("spec", "a.md")]
        );
        assert_eq!(start_id("s", "c1"), start_id("s", "c1"));
        assert_ne!(start_id("s", "c1"), start_id("s", "c2"));
        assert_eq!(
            run_id("s", "card.md", "2026-10-06T00:00:00Z"),
            run_id("s", "card.md", "2026-10-06T00:00:00Z")
        );
        assert_ne!(
            run_id("s", "card.md", "2026-10-06T00:00:00Z"),
            run_id("s", "card.md", "2026-10-07T00:00:00Z")
        );
        assert_eq!(
            expand_output(&workflow.outputs[0].path, "2026-10-06", "bmad-build"),
            "_bmad-output/implementation-artifacts/spec-bmad-build-2026-10-06.md"
        );
    }
}
