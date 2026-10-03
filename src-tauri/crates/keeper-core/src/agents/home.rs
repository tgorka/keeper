//! An agent's `agent.toml`: the machine config, its tool vocabulary and kind defaults (AD-360, AD-362, story 89.3).
//!
//! The file is a contract: every key is validated, an unknown one is refused
//! with its name, and every refusal is one sentence a person can act on. An
//! agent is identified by `(home drive id, agent id)` — the same folder in two
//! drives is two agents with two audiences (AD-360). `kind` chooses the
//! default tools and whether `human` is required; it is never a power.
//!
//! Parsing is pure: the host reads `agent.toml` through `browse::resolve` and
//! hands the text here with the folder name and the drive's declaration.

use std::collections::BTreeSet;

use matrix_sdk::ruma::{OwnedUserId, UserId};
use toml::{Table, Value};

use super::drive::DriveDecl;
use crate::bots::{parse_base_url, ProviderKind};

/// The file's name inside a home.
pub const FILE_NAME: &str = "agent.toml";

/// The grammar this build reads.
pub const GRAMMAR_VERSION: i64 = 1;

/// AD-397's closed tool vocabulary, without the `mcp:<server>/<tool>` form
/// (MCP tools are named by `[tools].mcp`). A name this build does not
/// implement is accepted here; the host lists it as not offered (90.5).
pub const TOOL_VOCABULARY: &[&str] = &[
    "drive_list",
    "drive_read",
    "drive_glob",
    "drive_grep",
    "drive_stat",
    "drive_write",
    "drive_edit",
    "drive_search",
    "session_write",
    "card_update",
    "journal_append",
    "memory_propose",
    "skill_propose",
    "skills_list",
    "skill_view",
    "delegate",
    "reply",
    "ask_human",
    "workflow_start",
    "bmad_config",
    "bmad_render",
    "bmad_memlog",
    "bmad_party",
    "helper",
    "run",
    "surface_open",
    "surface_highlight",
    "surface_point",
    "surface_scroll",
    "surface_propose_edit",
    "kvm_snapshot",
    "kvm_act",
];

const PROXY_DEFAULTS: &[&str] = &[
    "drive_list",
    "drive_read",
    "drive_glob",
    "drive_grep",
    "drive_stat",
    "drive_search",
    "delegate",
    "reply",
    "surface_open",
    "surface_highlight",
    "surface_point",
    "surface_scroll",
    "surface_propose_edit",
    "journal_append",
    "memory_propose",
    "skill_propose",
    "skills_list",
    "skill_view",
];

const SPECIALIST_DEFAULTS: &[&str] = &[
    "drive_list",
    "drive_read",
    "drive_glob",
    "drive_grep",
    "drive_stat",
    "drive_search",
    "session_write",
    "card_update",
    "workflow_start",
    "bmad_config",
    "bmad_render",
    "bmad_memlog",
    "bmad_party",
    "helper",
    "journal_append",
    "memory_propose",
    "skill_propose",
    "skills_list",
    "skill_view",
];

/// A specialist's set plus `card_update`, `delegate` and `workflow_start`
/// (AD-389, choice C8); the specialist already has two of the three.
const STEWARD_DEFAULTS: &[&str] = &[
    "drive_list",
    "drive_read",
    "drive_glob",
    "drive_grep",
    "drive_stat",
    "drive_search",
    "session_write",
    "card_update",
    "workflow_start",
    "bmad_config",
    "bmad_render",
    "bmad_memlog",
    "bmad_party",
    "helper",
    "journal_append",
    "memory_propose",
    "skill_propose",
    "skills_list",
    "skill_view",
    "delegate",
];

const GATE_DEFAULTS: &[&str] = &["reply", "delegate", "journal_append"];

/// What an agent is for (AD-360). It chooses defaults, never powers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    /// A person's one door (AD-380); names its `human`.
    Proxy,
    /// Plans, dispatches and harvests a drive's work (AD-389).
    Steward,
    /// Does one kind of work, usually a BMAD persona.
    Specialist,
    /// The door for an outside system (AD-415).
    Gate,
}

impl AgentKind {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "proxy" => Some(Self::Proxy),
            "steward" => Some(Self::Steward),
            "specialist" => Some(Self::Specialist),
            "gate" => Some(Self::Gate),
            _ => None,
        }
    }

    /// The word `agent.toml` spells it with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proxy => "proxy",
            Self::Steward => "steward",
            Self::Specialist => "specialist",
            Self::Gate => "gate",
        }
    }
}

/// The tools a kind gets when `[tools].allow` is absent.
pub fn default_allow(kind: AgentKind) -> &'static [&'static str] {
    match kind {
        AgentKind::Proxy => PROXY_DEFAULTS,
        AgentKind::Steward => STEWARD_DEFAULTS,
        AgentKind::Specialist => SPECIALIST_DEFAULTS,
        AgentKind::Gate => GATE_DEFAULTS,
    }
}

/// The host-independent provider reference `bot:{kind}:{base}#{target}`
/// settings sync already writes (`ProviderRef::bot_reference`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotRef {
    pub kind: ProviderKind,
    /// The base URL, normalised as a provider row stores it.
    pub base: String,
    /// The model or profile the bot names.
    pub target: String,
}

impl BotRef {
    /// Parse `bot:{kind}:{base}#{target}`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let shape = || {
            format!("\"{text}\" is not a bot reference: write it as bot:<kind>:<base URL>#<model>")
        };
        let rest = text.strip_prefix("bot:").ok_or_else(shape)?;
        let (kind, rest) = rest.split_once(':').ok_or_else(shape)?;
        let (base, target) = rest.split_once('#').ok_or_else(shape)?;
        let kind = ProviderKind::from_registry_str(kind).ok_or_else(|| {
            format!(
                "\"{kind}\" is not a provider kind this keeper knows, so the bot cannot be reached"
            )
        })?;
        let base = parse_base_url(base)
            .map_err(|error| format!("the bot's base URL is refused: {error}"))?
            .normalized;
        if target.trim().is_empty() {
            return Err(format!("\"{text}\" names no model after its #"));
        }
        Ok(Self {
            kind,
            base,
            target: target.to_owned(),
        })
    }
}

/// One `[[menu]]` item: BMAD's `[[agent.menu]]` shape (§10.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// Two to four uppercase letters, unique in the file.
    pub code: String,
    pub description: String,
    pub action: MenuAction,
}

/// What a menu item runs: exactly one of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    /// A folder under `_workflows/`.
    Workflow(String),
    /// A prompt of at most 2 KiB.
    Prompt(String),
}

/// `[host]`: where this agent's sessions may be placed (AD-379).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPrefs {
    pub needs: Vec<String>,
    /// A host slug, or empty.
    pub pin: String,
    pub prefer_always_on: bool,
}

/// `[limits]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub rounds_per_turn: u32,
    /// `0` means no budget beyond the model's.
    pub tokens_per_turn: u64,
    pub tokens_per_delegation: u64,
    pub hop_limit: u32,
    pub rounds_per_exchange: u32,
    pub max_concurrent_sessions: u32,
}

/// `[memory]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySettings {
    /// `0` is off.
    pub nudge_user_turns: u32,
    /// `0` is off.
    pub nudge_tool_iterations: u32,
    pub promote: bool,
}

/// A home's `agent.toml`, read against its drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// The home drive's id: with `id`, what names this agent.
    pub drive: String,
    pub id: String,
    pub name: String,
    pub kind: AgentKind,
    pub matrix_user: OwnedUserId,
    /// The person a proxy is the door for; `None` for every other kind.
    pub human: Option<OwnedUserId>,
    pub bot: BotRef,
    /// Effective: `true` when the file says so or the drive is `local_only`.
    pub local_only: bool,
    pub allow: Vec<String>,
    /// Drive ids in scope, the home drive first.
    pub drives: Vec<String>,
    pub mcp: Vec<String>,
    pub skills: Vec<String>,
    pub menu: Vec<MenuItem>,
    pub host: HostPrefs,
    pub limits: Limits,
    pub memory: MemorySettings,
    /// The home drive's readers: who may see what this agent reads.
    pub audience: BTreeSet<OwnedUserId>,
}

impl AgentConfig {
    /// `(home drive id, agent id)`: two drives, two agents (AD-360).
    pub fn key(&self) -> (&str, &str) {
        (&self.drive, &self.id)
    }
}

/// Why a home cannot be read. Each is the sentence the host shows.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HomeRefusal {
    #[error("agent.toml is not valid TOML: {0}")]
    Syntax(String),
    #[error("agent.toml has a key keeper does not know: {0}. Remove it or fix its spelling.")]
    UnknownKey(String),
    #[error("agent.toml needs {0}.")]
    Missing(String),
    #[error("agent.toml's {key} must be {expected}.")]
    WrongType { key: String, expected: &'static str },
    #[error("agent.toml's {key} is refused: {reason}.")]
    Invalid { key: String, reason: String },
    #[error("agent.toml says id = \"{id}\", but its folder is {folder}/. The folder name is the id: make them the same.")]
    IdNotFolder { id: String, folder: String },
    #[error("{folder}/ starts with \"_\", which is reserved for the zone's own folders, so it is not an agent's home.")]
    Reserved { folder: String },
    #[error("{first}/ and {second}/ both use the Matrix user {user}. One user is one agent: give each its own.")]
    SharedMatrixUser {
        user: String,
        first: String,
        second: String,
    },
}

const ROOT_KEYS: &[&str] = &[
    "version",
    "id",
    "name",
    "kind",
    "matrix_user",
    "human",
    "model",
    "tools",
    "menu",
    "host",
    "limits",
    "memory",
];
const MODEL_KEYS: &[&str] = &["bot", "local_only"];
const TOOLS_KEYS: &[&str] = &["allow", "drives", "mcp", "skills"];
const MENU_KEYS: &[&str] = &["code", "description", "workflow", "prompt"];
const HOST_KEYS: &[&str] = &["needs", "pin", "prefer_always_on"];
const LIMITS_KEYS: &[&str] = &[
    "rounds_per_turn",
    "tokens_per_turn",
    "tokens_per_delegation",
    "hop_limit",
    "rounds_per_exchange",
    "max_concurrent_sessions",
];
const MEMORY_KEYS: &[&str] = &["nudge_user_turns", "nudge_tool_iterations", "promote"];

const NAME_MAX: usize = 64;
const MENU_DESCRIPTION_MAX: usize = 120;
const MENU_PROMPT_MAX_BYTES: usize = 2048;

/// One table of the file, with the dotted path its keys are named by.
struct Section<'a> {
    path: String,
    table: &'a Table,
}

impl<'a> Section<'a> {
    fn new(path: &str, table: &'a Table, known: &[&str]) -> Result<Self, HomeRefusal> {
        let section = Self {
            path: path.to_owned(),
            table,
        };
        if let Some(unknown) = table.keys().find(|k| !known.contains(&k.as_str())) {
            return Err(HomeRefusal::UnknownKey(section.name(unknown)));
        }
        Ok(section)
    }

    fn name(&self, key: &str) -> String {
        if self.path.is_empty() {
            key.to_owned()
        } else {
            format!("{}.{key}", self.path)
        }
    }

    fn invalid(&self, key: &str, reason: impl Into<String>) -> HomeRefusal {
        HomeRefusal::Invalid {
            key: self.name(key),
            reason: reason.into(),
        }
    }

    fn wrong(&self, key: &str, expected: &'static str) -> HomeRefusal {
        HomeRefusal::WrongType {
            key: self.name(key),
            expected,
        }
    }

    fn str(&self, key: &str) -> Result<Option<&'a str>, HomeRefusal> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(_) => Err(self.wrong(key, "a string")),
        }
    }

    fn required_str(&self, key: &str) -> Result<&'a str, HomeRefusal> {
        self.str(key)?
            .ok_or_else(|| HomeRefusal::Missing(self.name(key)))
    }

    fn bool(&self, key: &str, default: bool) -> Result<bool, HomeRefusal> {
        match self.table.get(key) {
            None => Ok(default),
            Some(Value::Boolean(b)) => Ok(*b),
            Some(_) => Err(self.wrong(key, "true or false")),
        }
    }

    fn int(&self, key: &str, default: i64) -> Result<i64, HomeRefusal> {
        match self.table.get(key) {
            None => Ok(default),
            Some(Value::Integer(i)) => Ok(*i),
            Some(_) => Err(self.wrong(key, "a whole number")),
        }
    }

    /// An integer within `range`, named with the range when it is not.
    fn bounded(
        &self,
        key: &str,
        default: i64,
        accepts: impl Fn(i64) -> bool,
        rule: &str,
    ) -> Result<i64, HomeRefusal> {
        let value = self.int(key, default)?;
        if accepts(value) {
            Ok(value)
        } else {
            Err(self.invalid(key, format!("{value} is outside {rule}")))
        }
    }

    fn strings(&self, key: &str) -> Result<Option<Vec<String>>, HomeRefusal> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| self.wrong(key, "an array of strings"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(self.wrong(key, "an array of strings")),
        }
    }

    fn table(&self, key: &str, known: &[&str]) -> Result<Option<Section<'a>>, HomeRefusal> {
        match self.table.get(key) {
            None => Ok(None),
            Some(Value::Table(table)) => Section::new(&self.name(key), table, known).map(Some),
            Some(_) => Err(self.wrong(key, "a table")),
        }
    }
}

fn is_agent_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && id.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn is_drive_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && id.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn user_id(section: &Section<'_>, key: &str, text: &str) -> Result<OwnedUserId, HomeRefusal> {
    UserId::parse(text).map_err(|_| {
        section.invalid(
            key,
            format!("\"{text}\" is not a Matrix user id (@name:server)"),
        )
    })
}

/// Read a home's `agent.toml`. `folder` is the home's folder name inside the
/// zone; `drive` is the zone's `_drive.toml`, already read.
pub fn parse_agent_toml(
    text: &str,
    folder: &str,
    drive: &DriveDecl,
) -> Result<AgentConfig, HomeRefusal> {
    if folder.starts_with('_') {
        return Err(HomeRefusal::Reserved {
            folder: folder.to_owned(),
        });
    }
    let table: Table =
        toml::from_str(text).map_err(|error| HomeRefusal::Syntax(error.message().to_owned()))?;
    let root = Section::new("", &table, ROOT_KEYS)?;

    match root.table.get("version") {
        None => return Err(HomeRefusal::Missing("version".to_owned())),
        Some(Value::Integer(GRAMMAR_VERSION)) => {}
        Some(Value::Integer(other)) => {
            return Err(root.invalid(
                "version",
                format!(
                    "this keeper reads version {GRAMMAR_VERSION}, and the file is version {other}"
                ),
            ))
        }
        Some(_) => return Err(root.wrong("version", "a whole number")),
    }

    let id = root.required_str("id")?;
    if id != folder {
        return Err(HomeRefusal::IdNotFolder {
            id: id.to_owned(),
            folder: folder.to_owned(),
        });
    }
    if !is_agent_id(id) {
        return Err(root.invalid(
            "id",
            format!("\"{id}\" must be a lowercase letter, then up to 31 lowercase letters, digits or hyphens"),
        ));
    }

    let name = root.required_str("name")?;
    let name_chars = name.chars().count();
    if name.trim().is_empty() || name_chars > NAME_MAX {
        return Err(root.invalid(
            "name",
            format!("a name is 1 to {NAME_MAX} characters, and this one is {name_chars}"),
        ));
    }

    let kind_text = root.required_str("kind")?;
    let kind = AgentKind::parse(kind_text).ok_or_else(|| {
        root.invalid(
            "kind",
            format!("\"{kind_text}\" is not one of proxy, steward, specialist or gate"),
        )
    })?;

    let matrix_user = user_id(&root, "matrix_user", root.required_str("matrix_user")?)?;

    let human = match (kind, root.str("human")?) {
        (AgentKind::Proxy, None) => {
            return Err(root.invalid(
                "human",
                "a proxy is a person's door, so it names that person's Matrix id",
            ))
        }
        (AgentKind::Proxy, Some(text)) => {
            let human = user_id(&root, "human", text)?;
            if !drive.readers.contains(&human) {
                return Err(root.invalid(
                    "human",
                    format!(
                        "{human} is not among {}'s readers in _drive.toml, so this drive cannot be their door",
                        drive.id
                    ),
                ));
            }
            Some(human)
        }
        (_, Some(_)) => {
            return Err(root.invalid(
                "human",
                format!(
                    "only a proxy names a human, and this agent is a {}",
                    kind.as_str()
                ),
            ))
        }
        (_, None) => None,
    };

    let model = root
        .table("model", MODEL_KEYS)?
        .ok_or_else(|| HomeRefusal::Missing("[model].bot".to_owned()))?;
    let bot =
        BotRef::parse(model.required_str("bot")?).map_err(|reason| model.invalid("bot", reason))?;
    let says_local = model.bool("local_only", false)?;
    let local_only = says_local || drive.local_only;
    if local_only && !serves_local_models(bot.kind) {
        let why = if says_local {
            "model.local_only is true".to_owned()
        } else {
            format!("_drive.toml's local_only is true for {}", drive.id)
        };
        return Err(model.invalid(
            "bot",
            format!(
                "{why}, so the bot must be an ollama model that runs locally, and this one is {}",
                bot.kind.as_registry_str()
            ),
        ));
    }

    let tools = root.table("tools", TOOLS_KEYS)?;
    let field = |key: &str| match &tools {
        Some(section) => section.strings(key),
        None => Ok(None),
    };
    let tools_name = |key: &str| format!("tools.{key}");
    let allow = match field("allow")? {
        Some(names) => {
            for tool in &names {
                if tool.starts_with("mcp:") {
                    return Err(HomeRefusal::Invalid {
                        key: tools_name("allow"),
                        reason: format!(
                            "\"{tool}\" is an MCP tool, and MCP tools are named by [tools].mcp"
                        ),
                    });
                }
                if !TOOL_VOCABULARY.contains(&tool.as_str()) {
                    return Err(HomeRefusal::Invalid {
                        key: tools_name("allow"),
                        reason: format!(
                            "\"{tool}\" is not a tool keeper has; the tools are {}",
                            TOOL_VOCABULARY.join(", ")
                        ),
                    });
                }
            }
            names
        }
        None => default_allow(kind).iter().map(|&t| t.to_owned()).collect(),
    };
    let mut drives = vec![drive.id.clone()];
    for other in field("drives")?.unwrap_or_default() {
        if !is_drive_id(&other) {
            return Err(HomeRefusal::Invalid {
                key: tools_name("drives"),
                reason: format!("\"{other}\" is not a drive id"),
            });
        }
        if !drives.contains(&other) {
            drives.push(other);
        }
    }
    let mcp = field("mcp")?.unwrap_or_default();
    if let Some(empty) = mcp
        .iter()
        .find(|server| server.trim().is_empty() || server.contains('/'))
    {
        return Err(HomeRefusal::Invalid {
            key: tools_name("mcp"),
            reason: format!("\"{empty}\" is not an MCP server name"),
        });
    }
    let skills = field("skills")?.unwrap_or_else(|| vec!["*".to_owned()]);

    let menu = parse_menu(&root)?;

    let host = match root.table("host", HOST_KEYS)? {
        Some(section) => HostPrefs {
            needs: match section.strings("needs")? {
                Some(needs) => {
                    for need in &needs {
                        if !is_need(need) {
                            return Err(section.invalid(
                                "needs",
                                format!("\"{need}\" is not one of sandbox, mcp:<name>, screen:mac, kvm:<id> or voice"),
                            ));
                        }
                    }
                    needs
                }
                None => derived_needs(&allow, &mcp),
            },
            pin: section.str("pin")?.unwrap_or_default().to_owned(),
            prefer_always_on: section.bool("prefer_always_on", true)?,
        },
        None => HostPrefs {
            needs: derived_needs(&allow, &mcp),
            pin: String::new(),
            prefer_always_on: true,
        },
    };

    let empty = Table::new();
    let limits_table = root.table("limits", LIMITS_KEYS)?;
    let limits = limits_table.unwrap_or(Section {
        path: "limits".to_owned(),
        table: &empty,
    });
    let limits = Limits {
        rounds_per_turn: bounded_u32(&limits, "rounds_per_turn", 8, 1, 8)?,
        tokens_per_turn: limits.bounded("tokens_per_turn", 0, |v| v >= 0, "0 or more")? as u64,
        tokens_per_delegation: limits.bounded(
            "tokens_per_delegation",
            200_000,
            |v| v >= 1000,
            "1000 or more",
        )? as u64,
        hop_limit: bounded_u32(&limits, "hop_limit", 3, 0, 3)?,
        rounds_per_exchange: bounded_u32(&limits, "rounds_per_exchange", 3, 1, 3)?,
        max_concurrent_sessions: bounded_u32(&limits, "max_concurrent_sessions", 4, 1, 16)?,
    };

    let memory_table = root.table("memory", MEMORY_KEYS)?;
    let memory = memory_table.unwrap_or(Section {
        path: "memory".to_owned(),
        table: &empty,
    });
    let memory = MemorySettings {
        nudge_user_turns: off_or(&memory, "nudge_user_turns", 10, 5, 50)?,
        nudge_tool_iterations: off_or(&memory, "nudge_tool_iterations", 15, 5, 100)?,
        promote: memory.bool("promote", true)?,
    };

    Ok(AgentConfig {
        drive: drive.id.clone(),
        id: id.to_owned(),
        name: name.to_owned(),
        kind,
        matrix_user,
        human,
        bot,
        local_only,
        allow,
        drives,
        mcp,
        skills,
        menu,
        host,
        limits,
        memory,
        audience: drive.readers.clone(),
    })
}

/// Whether a provider kind is one a `local_only` agent may use, and the
/// `local` a turn asks `Label::may_use_model` with (S-04): an exhaustive
/// match, so the next kind decides here rather than by an equality test.
pub fn serves_local_models(kind: ProviderKind) -> bool {
    match kind {
        ProviderKind::Ollama => true,
        ProviderKind::Hermes | ProviderKind::OpenAi => false,
    }
}

fn bounded_u32(
    section: &Section<'_>,
    key: &str,
    default: i64,
    lo: i64,
    hi: i64,
) -> Result<u32, HomeRefusal> {
    let value = section.bounded(
        key,
        default,
        |v| (lo..=hi).contains(&v),
        &format!("{lo} to {hi}"),
    )?;
    u32::try_from(value)
        .map_err(|_| section.invalid(key, format!("{value} is outside {lo} to {hi}")))
}

fn off_or(
    section: &Section<'_>,
    key: &str,
    default: i64,
    lo: i64,
    hi: i64,
) -> Result<u32, HomeRefusal> {
    let rule = format!("0 (off) or {lo} to {hi}");
    let value = section.bounded(key, default, |v| v == 0 || (lo..=hi).contains(&v), &rule)?;
    u32::try_from(value).map_err(|_| section.invalid(key, format!("{value} is outside {rule}")))
}

fn is_need(need: &str) -> bool {
    match need {
        "sandbox" | "screen:mac" | "voice" => true,
        _ => need
            .strip_prefix("mcp:")
            .or_else(|| need.strip_prefix("kvm:"))
            .is_some_and(|rest| !rest.trim().is_empty()),
    }
}

/// What a host must offer for these tools: a sandbox for `run`, and each MCP
/// server named.
fn derived_needs(allow: &[String], mcp: &[String]) -> Vec<String> {
    let mut needs = Vec::new();
    if allow.iter().any(|tool| tool == "run") {
        needs.push("sandbox".to_owned());
    }
    needs.extend(mcp.iter().map(|server| format!("mcp:{server}")));
    needs
}

fn parse_menu(root: &Section<'_>) -> Result<Vec<MenuItem>, HomeRefusal> {
    let items = match root.table.get("menu") {
        None => return Ok(Vec::new()),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(root.wrong("menu", "an array of [[menu]] tables")),
    };
    let mut menu: Vec<MenuItem> = Vec::with_capacity(items.len());
    for (at, item) in items.iter().enumerate() {
        let Value::Table(table) = item else {
            return Err(root.wrong("menu", "an array of [[menu]] tables"));
        };
        let section = Section::new(&format!("menu[{}]", at + 1), table, MENU_KEYS)?;
        let code = section.required_str("code")?;
        if !(2..=4).contains(&code.len()) || !code.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(section.invalid(
                "code",
                format!("\"{code}\" must be 2 to 4 uppercase letters"),
            ));
        }
        if menu.iter().any(|earlier| earlier.code == code) {
            return Err(section.invalid(
                "code",
                format!("\"{code}\" is already used by an earlier [[menu]] item"),
            ));
        }
        let description = section.required_str("description")?;
        let length = description.chars().count();
        if length > MENU_DESCRIPTION_MAX {
            return Err(section.invalid(
                "description",
                format!("it is {length} characters, and the most is {MENU_DESCRIPTION_MAX}"),
            ));
        }
        let action = match (section.str("workflow")?, section.str("prompt")?) {
            (Some(workflow), None) => {
                if workflow.is_empty()
                    || workflow.contains(['/', '\\'])
                    || workflow.starts_with('.')
                {
                    return Err(section.invalid(
                        "workflow",
                        format!("\"{workflow}\" is not a folder name under _workflows/"),
                    ));
                }
                MenuAction::Workflow(workflow.to_owned())
            }
            (None, Some(prompt)) => {
                if prompt.len() > MENU_PROMPT_MAX_BYTES {
                    return Err(section.invalid(
                        "prompt",
                        format!(
                            "it is {} bytes, and the most is {MENU_PROMPT_MAX_BYTES}",
                            prompt.len()
                        ),
                    ));
                }
                MenuAction::Prompt(prompt.to_owned())
            }
            (Some(_), Some(_)) => {
                return Err(
                    section.invalid("workflow", "an item runs a workflow or a prompt, not both")
                )
            }
            (None, None) => {
                return Err(section.invalid(
                    "workflow",
                    "an item names a workflow or a prompt, and this one names neither",
                ))
            }
        };
        menu.push(MenuItem {
            code: code.to_owned(),
            description: description.to_owned(),
            action,
        });
    }
    Ok(menu)
}

/// The homes in one zone that share a Matrix user, each pair refused naming
/// both folders (AD-374: one user per agent).
pub fn shared_matrix_users(homes: &[AgentConfig]) -> Vec<HomeRefusal> {
    let mut refusals = Vec::new();
    for (at, home) in homes.iter().enumerate() {
        if let Some(first) = homes[..at]
            .iter()
            .find(|earlier| earlier.matrix_user == home.matrix_user)
        {
            refusals.push(HomeRefusal::SharedMatrixUser {
                user: home.matrix_user.to_string(),
                first: first.id.clone(),
                second: home.id.clone(),
            });
        }
    }
    refusals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{drive, soul};

    fn fixture(path: &str) -> String {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/agents/");
        std::fs::read_to_string(format!("{root}{path}")).expect("fixture is readable")
    }

    fn tgdrive() -> DriveDecl {
        drive::parse(&fixture("zone-ok/_drive.toml")).expect("tgdrive's _drive.toml parses")
    }

    fn neuradrive() -> DriveDecl {
        drive::parse(
            r#"version = 1
id = "neuradrive"
title = "neuradrive"
principal = "neuraffica"
owner = "@tgorka:example.org"
readers = ["@marta:example.org", "@tgorka:example.org"]
"#,
        )
        .expect("neuradrive parses")
    }

    fn nixi() -> String {
        fixture("zone-ok/nixi/agent.toml")
    }

    /// Parse Nixi with one substitution, which must apply.
    fn nixi_with(from: &str, to: &str) -> Result<AgentConfig, HomeRefusal> {
        let text = nixi();
        assert!(text.contains(from), "the fixture holds {from:?}");
        parse_agent_toml(&text.replacen(from, to, 1), "nixi", &tgdrive())
    }

    fn sentence(result: Result<AgentConfig, HomeRefusal>) -> String {
        result.expect_err("refused").to_string()
    }

    fn minimal(kind: &str, extra: &str) -> String {
        format!(
            "version = 1\nid = \"amelia\"\nname = \"Amelia\"\nkind = \"{kind}\"\n\
             matrix_user = \"@amelia-tgdrive:example.org\"\n{extra}\n\
             [model]\nbot = \"bot:ollama:http://localhost:11434#qwen3\"\n"
        )
    }

    /// The architecture's Nixi example (`ARCHITECTURE-AGENTS.md`,
    /// `<agent>/agent.toml`) with its placeholders filled by reserved example
    /// hosts, never the owner's endpoint (S-20).
    const ARCHITECTURE_NIXI: &str = r#"version     = 1
id          = "nixi"
name        = "Nixi"
kind        = "proxy"
matrix_user = "@nixi:example.org"
human       = "@tgorka:example.org"

[model]
bot        = "bot:openai:https://provider.example:8452#claude-opus"
local_only = false

[tools]
allow  = ["drive_list", "drive_read", "drive_glob", "drive_grep", "drive_stat", "drive_search",
          "delegate", "reply", "surface_open", "surface_highlight", "surface_point",
          "surface_scroll", "surface_propose_edit", "journal_append", "memory_propose",
          "skills_list", "skill_view"]
drives = ["tgdrive", "neuradrive"]
mcp    = []
skills = ["*"]

[[menu]]
code        = "TR"
description = "Triage what came in today"
workflow    = "triage"

[host]
needs            = []
pin              = ""
prefer_always_on = true

[limits]
rounds_per_turn        = 8
tokens_per_turn        = 0
tokens_per_delegation  = 200000
hop_limit              = 3
rounds_per_exchange    = 3
max_concurrent_sessions = 4

[memory]
nudge_user_turns      = 10
nudge_tool_iterations = 15
promote               = true
"#;

    #[test]
    fn the_fixture_is_the_architecture_example_with_an_ollama_bot() {
        // One change, named in the fixture's first line (F17).
        let fixture = nixi();
        let (comment, body) = fixture.split_once('\n').expect("a first line");
        assert!(
            comment.starts_with("# The architecture's Nixi example"),
            "{comment}"
        );
        let differ: Vec<(&str, &str)> = body
            .lines()
            .zip(ARCHITECTURE_NIXI.lines())
            .filter(|(ours, theirs)| ours != theirs)
            .collect();
        assert_eq!(
            differ,
            [(
                "bot        = \"bot:ollama:http://electra.example.org:11434#qwen3:32b\"",
                "bot        = \"bot:openai:https://provider.example:8452#claude-opus\""
            )]
        );
        assert_eq!(body.lines().count(), ARCHITECTURE_NIXI.lines().count());

        let config = parse_agent_toml(&fixture, "nixi", &tgdrive()).expect("Nixi parses");
        assert_eq!(config.key(), ("tgdrive", "nixi"));
        assert_eq!(config.kind, AgentKind::Proxy);
        assert_eq!(
            config.human.as_ref().map(|h| h.as_str()),
            Some("@tgorka:example.org")
        );
        assert_eq!(
            config.bot,
            BotRef {
                kind: ProviderKind::Ollama,
                base: "http://electra.example.org:11434".to_owned(),
                target: "qwen3:32b".to_owned(),
            }
        );
        assert_eq!(config.drives, vec!["tgdrive", "neuradrive"]);
        assert_eq!(config.menu.len(), 1);
        assert_eq!(
            config.menu[0].action,
            MenuAction::Workflow("triage".to_owned())
        );

        let shouted = nixi_with(
            "bot:ollama:http://electra.example.org:11434#",
            "bot:ollama:HTTP://Electra.example.org:11434/#",
        )
        .expect("an unnormalised base URL parses");
        assert_eq!(
            shouted.bot.base, "http://electra.example.org:11434",
            "normalised"
        );

        let unknown = sentence(nixi_with("\"bot:ollama:", "\"bot:anthropic:"));
        assert!(unknown.contains("model.bot"), "{unknown}");
        assert!(
            unknown.contains("\"anthropic\" is not a provider kind"),
            "{unknown}"
        );
    }

    #[test]
    fn the_architectures_nixi_parses_verbatim_with_its_openai_bot() {
        let config = parse_agent_toml(ARCHITECTURE_NIXI, "nixi", &tgdrive()).expect("parses");
        assert_eq!(
            config.bot,
            BotRef {
                kind: ProviderKind::OpenAi,
                base: "https://provider.example:8452".to_owned(),
                target: "claude-opus".to_owned(),
            }
        );
        // An OpenAI-compatible endpoint is not a local model.
        let mut local_drive = tgdrive();
        local_drive.local_only = true;
        let refused =
            parse_agent_toml(ARCHITECTURE_NIXI, "nixi", &local_drive).expect_err("refused");
        assert!(
            refused.to_string().contains("and this one is openai"),
            "{refused}"
        );
    }

    #[test]
    fn an_unknown_key_is_refused_naming_it() {
        let refusal = sentence(nixi_with("[tools]\nallow", "[tools]\ntool = []\nallow"));
        assert_eq!(
            refusal,
            "agent.toml has a key keeper does not know: tools.tool. Remove it or fix its spelling."
        );
        let root = sentence(nixi_with(
            "version     = 1",
            "version = 1\ncolour = \"red\"",
        ));
        assert!(root.contains("know: colour."), "{root}");
    }

    #[test]
    fn the_id_is_the_folder_and_underscore_folders_are_the_zones() {
        let refusal = sentence(nixi_with("id          = \"nixi\"", "id = \"nixie\""));
        assert!(refusal.contains("id = \"nixie\""), "{refusal}");
        assert!(refusal.contains("nixi/"), "{refusal}");

        let reserved = parse_agent_toml(
            &nixi().replace("\"nixi\"", "\"_nixi\""),
            "_nixi",
            &tgdrive(),
        );
        assert_eq!(
            reserved,
            Err(HomeRefusal::Reserved {
                folder: "_nixi".to_owned()
            })
        );
    }

    #[test]
    fn the_name_must_be_the_souls() {
        let config = parse_agent_toml(&nixi(), "nixi", &tgdrive()).expect("Nixi parses");
        let soul_text = fixture("zone-ok/nixi/SOUL.md");
        assert!(soul::parse_soul(&soul_text, &config.name).is_ok());
        let renamed = nixi_with("name        = \"Nixi\"", "name = \"Nyx\"").expect("parses");
        let refusal = soul::parse_soul(&soul_text, &renamed.name).expect_err("refused");
        assert_eq!(
            refusal.to_string(),
            "SOUL.md names Nixi, but agent.toml names Nyx. They must be the same."
        );
    }

    #[test]
    fn human_is_a_proxys_and_a_reader_of_the_drive() {
        let missing = sentence(nixi_with("human       = \"@tgorka:example.org\"\n", ""));
        assert!(missing.contains("agent.toml's human"), "{missing}");

        let outsider = sentence(nixi_with(
            "@tgorka:example.org\"\n",
            "@marta:example.org\"\n",
        ));
        assert!(
            outsider.contains("@marta:example.org is not among tgdrive's readers"),
            "{outsider}"
        );

        let steward = sentence(parse_agent_toml(
            &minimal("steward", "human = \"@tgorka:example.org\""),
            "amelia",
            &tgdrive(),
        ));
        assert!(steward.contains("only a proxy names a human"), "{steward}");
        assert!(parse_agent_toml(&minimal("steward", ""), "amelia", &tgdrive()).is_ok());
    }

    #[test]
    fn a_bot_with_userinfo_or_no_target_is_refused() {
        let userinfo = sentence(nixi_with("http://electra", "http://u:p@electra"));
        assert!(userinfo.contains("model.bot"), "{userinfo}");
        assert!(
            userinfo.contains("userinfo") || userinfo.contains("user"),
            "{userinfo}"
        );
        let no_target = sentence(nixi_with("#qwen3:32b", "#"));
        assert!(no_target.contains("names no model"), "{no_target}");
    }

    #[test]
    fn local_only_needs_an_ollama_bot_and_the_drive_can_force_it() {
        const OLLAMA: &str = "bot:ollama:http://electra.example.org:11434#qwen3:32b";
        let hermes = sentence(nixi_with(
            &format!("{OLLAMA}\"\nlocal_only = false"),
            "bot:hermes:http://localhost:8642#nixie\"\nlocal_only = true",
        ));
        assert!(hermes.contains("model.local_only is true"), "{hermes}");
        assert!(hermes.contains("this one is hermes"), "{hermes}");

        let mut local_drive = tgdrive();
        local_drive.local_only = true;
        let remote = nixi().replace(OLLAMA, "bot:hermes:http://localhost:8642#nixie");
        let forced = parse_agent_toml(&remote, "nixi", &local_drive).expect_err("refused");
        assert!(
            forced
                .to_string()
                .contains("_drive.toml's local_only is true for tgdrive"),
            "{forced}"
        );
        let config = parse_agent_toml(&nixi(), "nixi", &local_drive).expect("ollama is local");
        assert!(config.local_only, "the drive makes it effective");
        let own = parse_agent_toml(&nixi(), "nixi", &tgdrive()).expect("parses");
        assert!(!own.local_only);
    }

    #[test]
    fn mcp_names_and_unknown_tools_are_refused_from_allow() {
        let mcp = sentence(nixi_with(
            "\"skill_view\"]",
            "\"skill_view\", \"mcp:paseo/x\"]",
        ));
        assert!(mcp.contains("MCP tools are named by [tools].mcp"), "{mcp}");
        let shell = sentence(nixi_with("\"skill_view\"]", "\"skill_view\", \"shell\"]"));
        assert!(
            shell.contains("\"shell\" is not a tool keeper has"),
            "{shell}"
        );
        assert!(
            shell.contains("drive_list, drive_read"),
            "names the vocabulary: {shell}"
        );
        // R24 completed the vocabulary with the KVM tools, which parse before
        // any host offers them.
        let kvm = nixi_with(
            "\"skill_view\"]",
            "\"skill_view\", \"kvm_snapshot\", \"kvm_act\"]",
        )
        .expect("the KVM tools are in the vocabulary");
        assert!(kvm
            .allow
            .ends_with(&["kvm_snapshot".to_owned(), "kvm_act".to_owned()]));
    }

    #[test]
    fn a_menu_item_is_unique_uppercase_and_runs_exactly_one_thing() {
        let item = |body: &str| {
            parse_agent_toml(
                &minimal("specialist", "").replace("[model]", &format!("{body}\n[model]")),
                "amelia",
                &tgdrive(),
            )
        };
        let ok = "[[menu]]\ncode = \"TR\"\ndescription = \"d\"\nprompt = \"p\"\n";
        assert!(item(ok).is_ok());
        let duplicate = sentence(item(&format!("{ok}{ok}")));
        assert!(
            duplicate.contains("menu[2].code") && duplicate.contains("already used"),
            "{duplicate}"
        );
        let lower = sentence(item(
            "[[menu]]\ncode = \"x\"\ndescription = \"d\"\nprompt = \"p\"\n",
        ));
        assert!(lower.contains("2 to 4 uppercase letters"), "{lower}");
        let both = sentence(item(
            "[[menu]]\ncode = \"TR\"\ndescription = \"d\"\nprompt = \"p\"\nworkflow = \"w\"\n",
        ));
        assert!(both.contains("not both"), "{both}");
        let neither = sentence(item("[[menu]]\ncode = \"TR\"\ndescription = \"d\"\n"));
        assert!(neither.contains("names neither"), "{neither}");
    }

    #[test]
    fn every_bound_is_refused_one_past_its_edge_and_accepted_on_it() {
        let with = |section: &str, key: &str, value: i64| {
            parse_agent_toml(
                &format!(
                    "{}[{section}]\n{key} = {value}\n",
                    minimal("specialist", "")
                ),
                "amelia",
                &tgdrive(),
            )
        };
        for (section, key, refused, accepted) in [
            ("limits", "rounds_per_turn", &[0, 9][..], &[1, 8][..]),
            ("limits", "hop_limit", &[4, -1], &[0, 3]),
            ("limits", "rounds_per_exchange", &[0, 4], &[1, 3]),
            ("limits", "max_concurrent_sessions", &[0, 17], &[1, 16]),
            ("limits", "tokens_per_delegation", &[999], &[1000]),
            ("limits", "tokens_per_turn", &[-1], &[0]),
            ("memory", "nudge_user_turns", &[4, 51], &[0, 5, 50]),
            ("memory", "nudge_tool_iterations", &[4, 101], &[0, 5, 100]),
        ] {
            for value in refused {
                let refusal = sentence(with(section, key, *value));
                assert!(refusal.contains(&format!("{section}.{key}")), "{refusal}");
                assert!(
                    refusal.contains(&format!("{value} is outside")),
                    "{refusal}"
                );
            }
            for value in accepted {
                assert!(
                    with(section, key, *value).is_ok(),
                    "{section}.{key} = {value}"
                );
            }
        }
        let defaults =
            parse_agent_toml(&minimal("specialist", ""), "amelia", &tgdrive()).expect("parses");
        assert_eq!(
            defaults.limits,
            Limits {
                rounds_per_turn: 8,
                tokens_per_turn: 0,
                tokens_per_delegation: 200_000,
                hop_limit: 3,
                rounds_per_exchange: 3,
                max_concurrent_sessions: 4,
            }
        );
        assert_eq!(
            defaults.memory,
            MemorySettings {
                nudge_user_turns: 10,
                nudge_tool_iterations: 15,
                promote: true,
            }
        );
    }

    #[test]
    fn two_homes_sharing_a_matrix_user_are_refused_naming_both() {
        let nixi = parse_agent_toml(&nixi(), "nixi", &tgdrive()).expect("parses");
        let twin_text =
            minimal("specialist", "").replace("@amelia-tgdrive:example.org", "@nixi:example.org");
        let twin = parse_agent_toml(&twin_text, "amelia", &tgdrive()).expect("parses alone");
        let tola = parse_agent_toml(
            &fixture("zone-ok/tola-grey/agent.toml"),
            "tola-grey",
            &tgdrive(),
        )
        .expect("Tola parses");
        assert_eq!(
            shared_matrix_users(&[nixi.clone(), tola.clone(), twin]),
            vec![HomeRefusal::SharedMatrixUser {
                user: "@nixi:example.org".to_owned(),
                first: "nixi".to_owned(),
                second: "amelia".to_owned(),
            }]
        );
        assert_eq!(shared_matrix_users(&[nixi, tola]), Vec::new());
    }

    #[test]
    fn a_kind_only_chooses_defaults() {
        for kind in [
            AgentKind::Proxy,
            AgentKind::Steward,
            AgentKind::Specialist,
            AgentKind::Gate,
        ] {
            let human = if kind == AgentKind::Proxy {
                "human = \"@tgorka:example.org\""
            } else {
                ""
            };
            let config = parse_agent_toml(&minimal(kind.as_str(), human), "amelia", &tgdrive())
                .expect("parses");
            assert_eq!(config.allow, default_allow(kind), "{kind:?}");
        }
        // The defaults as the epic pins them: AD-397's rule, and a steward is
        // a specialist plus card_update, delegate and workflow_start (AD-389).
        let set = |parts: &[&[&str]]| -> BTreeSet<&'static str> {
            parts
                .iter()
                .flat_map(|part| part.iter())
                .map(|tool| {
                    *TOOL_VOCABULARY
                        .iter()
                        .find(|t| *t == tool)
                        .expect("in the vocabulary")
                })
                .collect()
        };
        let defaults = |kind| -> BTreeSet<&'static str> {
            let tools = default_allow(kind);
            let set: BTreeSet<&'static str> = tools.iter().copied().collect();
            assert_eq!(set.len(), tools.len(), "{kind:?} lists a tool twice");
            set
        };
        let reads: &[&str] = &[
            "drive_list",
            "drive_read",
            "drive_glob",
            "drive_grep",
            "drive_stat",
        ];
        let memory_tools: &[&str] = &[
            "journal_append",
            "memory_propose",
            "skill_propose",
            "skills_list",
            "skill_view",
        ];
        let surface: &[&str] = &[
            "surface_open",
            "surface_highlight",
            "surface_point",
            "surface_scroll",
            "surface_propose_edit",
        ];
        let specialist: &[&str] = &[
            "drive_search",
            "session_write",
            "card_update",
            "workflow_start",
            "bmad_config",
            "bmad_render",
            "bmad_memlog",
            "bmad_party",
            "helper",
        ];
        assert_eq!(
            defaults(AgentKind::Proxy),
            set(&[
                reads,
                &["drive_search", "delegate", "reply"],
                surface,
                memory_tools
            ])
        );
        assert_eq!(
            defaults(AgentKind::Specialist),
            set(&[reads, specialist, memory_tools])
        );
        assert_eq!(
            defaults(AgentKind::Steward),
            set(&[
                reads,
                specialist,
                memory_tools,
                &["card_update", "delegate", "workflow_start"]
            ])
        );
        assert_eq!(
            defaults(AgentKind::Gate),
            set(&[&["reply", "delegate", "journal_append"]])
        );

        let reads = "[tools]\nallow = [\"drive_list\", \"drive_read\", \"drive_glob\", \"drive_grep\", \"drive_stat\"]\n";
        let gate = parse_agent_toml(
            &minimal("gate", "").replace("[model]", &format!("{reads}[model]")),
            "amelia",
            &tgdrive(),
        )
        .expect("a gate with reads parses");
        assert_eq!(
            gate.allow,
            [
                "drive_list",
                "drive_read",
                "drive_glob",
                "drive_grep",
                "drive_stat"
            ]
        );

        // Same file, another non-proxy kind: everything but `kind` is equal.
        let as_specialist = parse_agent_toml(
            &minimal("specialist", "").replace("[model]", &format!("{reads}[model]")),
            "amelia",
            &tgdrive(),
        )
        .expect("parses");
        assert_eq!(
            AgentConfig {
                kind: AgentKind::Gate,
                ..as_specialist
            },
            gate
        );
    }

    #[test]
    fn amelia_in_two_drives_is_two_agents() {
        let amelia = minimal("specialist", "");
        let in_tgdrive = parse_agent_toml(&amelia, "amelia", &tgdrive()).expect("parses");
        let in_neuradrive = parse_agent_toml(&amelia, "amelia", &neuradrive()).expect("parses");
        assert_eq!(in_tgdrive.key(), ("tgdrive", "amelia"));
        assert_eq!(in_neuradrive.key(), ("neuradrive", "amelia"));
        assert_ne!(in_tgdrive.key(), in_neuradrive.key());
        let names = |config: &AgentConfig| {
            config
                .audience
                .iter()
                .map(|u| u.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names(&in_tgdrive), ["@tgorka:example.org"]);
        assert_eq!(
            names(&in_neuradrive),
            ["@marta:example.org", "@tgorka:example.org"]
        );
        assert_eq!(in_tgdrive.drives, ["tgdrive"], "the home drive is in scope");
        assert_eq!(in_neuradrive.drives, ["neuradrive"]);
    }
}
