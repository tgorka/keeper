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

use crate::bots::chat::ToolSpec;

/// `resolve_config.py` and `resolve_customization.py`, ported.
pub const BMAD_CONFIG: &str = "bmad_config";
/// `resolve_party.py`, ported.
pub const BMAD_PARTY: &str = "bmad_party";
/// The skills offered to the agent.
pub const SKILLS_LIST: &str = "skills_list";
/// One offered skill's `SKILL.md`, or one file inside it.
pub const SKILL_VIEW: &str = "skill_view";

/// The tools through which a session follows BMAD's skills and
/// workflows: a session offered any of them is told where BMAD's project
/// root is and what BMAD may assume there.
const FRAMED: [&str; 7] = [
    BMAD_CONFIG,
    "bmad_render",
    "bmad_memlog",
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
        tools: &[BMAD_CONFIG, "bmad_render", "bmad_memlog", BMAD_PARTY, "run"],
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
    }
}
