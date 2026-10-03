//! The agents zone, seeded: the guide, the rules, `_drive.toml`, the template
//! and the catalogue's agents, as text (AD-360, AD-361, AD-362; FR-788).
//!
//! Pure. [`SeedChoices::new`] checks what a person asked for — the drive, its
//! owner and readers, the bot the seeded agents run on, which agents — and
//! [`plan`] says which files that is and which of them are already there:
//! every file that exists is left, never overwritten (`keeper-agent`'s
//! `seed::apply` writes with create-new semantics, so a file appearing after
//! the plan is left too). The seeds are the owner's files from their first
//! commit; no keeper tool writes them again (AD-362).
//!
//! The bot has no default (S-20): the seeded agents run on the bot the person
//! names, and nothing here names a provider for them.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use ulid::Ulid;

use super::agentd::DrivePin;
use super::drive::{self, DriveDecl, DEFAULT_UNTRUSTED};
use super::home::{not_local_bot, serves_local_models, BotRef};
use super::mount::pin_matches;
use crate::bots::store::ProviderRow;
use crate::bots::Bot;
use crate::bots::ProviderKind;
use crate::org_account::settings_sync::ProviderRef;

const README: &str = include_str!("zone/README.md");
const RULES: &str = include_str!("zone/AGENTS.md");
const MEMORY: &str = include_str!("zone/memory.md");
const TEMPLATE_AGENT: &str = include_str!("zone/_template/agent.toml");
const TEMPLATE_SOUL: &str = include_str!("zone/_template/SOUL.md");
const STEWARD_MENU: &str = include_str!("zone/steward-menu.toml");

/// The folder `agents new` copies.
pub const TEMPLATE_DIR: &str = "_template";

/// One agent the seed can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Catalogued {
    /// The folder and the id (R19).
    pub id: &'static str,
    pub name: &'static str,
    /// `agent.toml`'s `kind`.
    pub kind: &'static str,
    /// The drive its soul was written for.
    pub home_drive: &'static str,
    agent_toml: &'static str,
    soul: &'static str,
}

/// Nixi, Dr Tola Grey and Dr Lucyna Novak (P1, R19).
pub const CATALOGUE: [Catalogued; 3] = [
    Catalogued {
        id: "nixi",
        name: "Nixi",
        kind: "proxy",
        home_drive: "tgdrive",
        agent_toml: include_str!("zone/nixi/agent.toml"),
        soul: include_str!("zone/nixi/SOUL.md"),
    },
    Catalogued {
        id: "tola-grey",
        name: "Dr Tola Grey",
        kind: "steward",
        home_drive: "tgdrive",
        agent_toml: include_str!("zone/tola-grey/agent.toml"),
        soul: include_str!("zone/tola-grey/SOUL.md"),
    },
    Catalogued {
        id: "lucyna-novak",
        name: "Dr Lucyna Novak",
        kind: "steward",
        home_drive: "neuradrive",
        agent_toml: include_str!("zone/lucyna-novak/agent.toml"),
        soul: include_str!("zone/lucyna-novak/SOUL.md"),
    },
];

/// The catalogue's ids, as a refusal names them.
fn catalogue_ids() -> String {
    CATALOGUE
        .iter()
        .map(|agent| agent.id)
        .collect::<Vec<_>>()
        .join(", ")
}

/// What `agents init` and *Set up agents* refuse without (S-20).
pub const NO_BOT: &str = "Name the bot the seeded agents run on: keeper never picks one for them.";

/// One file of the seed, zone-relative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedFile {
    pub path: String,
    pub text: String,
}

/// What a seed writes and what it leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedPlan {
    /// The files to write, in the order they are written.
    pub write: Vec<SeedFile>,
    /// Zone-relative paths already there, left as they are.
    pub left: Vec<String>,
}

/// What a person asked to seed, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedChoices {
    /// `_drive.toml` as the flags declare it.
    pub decl: DriveDecl,
    /// `bot:{kind}:{base}#{target}`, normalised.
    pub bot: String,
    bot_kind: ProviderKind,
    /// The catalogue's agents to write, in catalogue order.
    pub with: Vec<&'static Catalogued>,
}

impl SeedChoices {
    /// Check a seed's inputs; each refusal is one sentence a person can act
    /// on. `readers` must hold `owner` (*Data formats*: the owner is a
    /// reader); `bot` is required (S-20) and, on a `local_only` drive, must
    /// run locally; every id in `with` is the catalogue's.
    pub fn new(
        drive_id: &str,
        principal: &str,
        owner: &str,
        readers: &[String],
        local_only: bool,
        bot: Option<&str>,
        with: &[String],
    ) -> Result<SeedChoices, String> {
        let bot = bot
            .map(str::trim)
            .filter(|bot| !bot.is_empty())
            .ok_or_else(|| NO_BOT.to_owned())?;
        let bot = BotRef::parse(bot)?;
        let bot_kind = bot.kind;
        let bot = format!(
            "bot:{}:{}#{}",
            bot.kind.as_registry_str(),
            bot.base,
            bot.target
        );
        let mut chosen: Vec<&'static Catalogued> = Vec::new();
        for id in with {
            let agent = CATALOGUE
                .iter()
                .find(|agent| agent.id == id.trim())
                .ok_or_else(|| {
                    format!(
                        "\"{id}\" is not in the catalogue; the catalogue is {}.",
                        catalogue_ids()
                    )
                })?;
            if !chosen.iter().any(|known| known.id == agent.id) {
                chosen.push(agent);
            }
        }
        chosen.sort_by_key(|agent| CATALOGUE.iter().position(|c| c.id == agent.id));
        let decl = drive::parse(&drive_toml(drive_id, principal, owner, readers, local_only))
            .map_err(|refusal| refusal.sentence())?;
        let choices = SeedChoices {
            decl,
            bot,
            bot_kind,
            with: chosen,
        };
        choices.check_bot(&choices.decl, None)?;
        Ok(choices)
    }

    /// The homeserver the seeded agents' Matrix users are on: the owner's.
    pub fn homeserver(&self) -> &str {
        self.decl.owner.server_name().as_str()
    }

    /// Whether the seed holds a proxy, whose DM `agents init` makes.
    pub fn proxy(&self) -> Option<&'static Catalogued> {
        self.with
            .iter()
            .copied()
            .find(|agent| agent.kind == "proxy")
    }

    /// The seed against the declaration the zone will host under — the
    /// zone's own `_drive.toml` when it has one, else the flags' — and this
    /// host's pin for the drive (S-15): a zone that differs from the pin
    /// hosts nothing, and on a `local_only` drive every seeded agent would
    /// be refused at sign-in on a bot that does not run locally, so both are
    /// refused now, naming why.
    pub fn check_hosting(&self, hosting: &DriveDecl, pin: Option<&DrivePin>) -> Result<(), String> {
        if let Some(pin) = pin {
            pin_matches(hosting, pin).map_err(|difference| {
                format!(
                    "The owner, readers and local_only must be what this host pinned for {}, or the zone would host nothing: {}.",
                    pin.id,
                    difference.differences.join("; ")
                )
            })?;
        }
        self.check_bot(hosting, pin)
    }

    /// The bot against the hosting declaration's `local_only`, and the pin's
    /// (a host ORs them, review R34-03), as `home.rs` would refuse it.
    fn check_bot(&self, hosting: &DriveDecl, pin: Option<&DrivePin>) -> Result<(), String> {
        let local_only = hosting.local_only || pin.is_some_and(|pin| pin.local_only);
        if local_only && !serves_local_models(self.bot_kind) {
            let why = format!("_drive.toml's local_only is true for {}", hosting.id);
            return Err(format!("{}.", not_local_bot(&why, self.bot_kind)));
        }
        Ok(())
    }
}

/// A TOML basic string.
fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// `_drive.toml` from the flags, readers sorted, with its `[integrity]`
/// table written out (S-02).
fn drive_toml(
    id: &str,
    principal: &str,
    owner: &str,
    readers: &[String],
    local_only: bool,
) -> String {
    let readers: BTreeSet<&str> = readers.iter().map(|r| r.trim()).collect();
    let readers: Vec<String> = readers.into_iter().map(quoted).collect();
    let untrusted: Vec<String> = DEFAULT_UNTRUSTED.iter().map(|glob| quoted(glob)).collect();
    format!(
        "# Who may read this drive, and so who may see what its agents read (AD-361).\n\
         # Seeded by keeper; yours to edit from its first commit.\n\
         version    = 1\n\
         id         = {id}\n\
         title      = {id}\n\
         principal  = {principal}\n\
         owner      = {owner}\n\
         readers    = [{readers}]\n\
         # true: every agent homed here runs on a model on your own machines (AD-377).\n\
         local_only = {local_only}\n\
         \n\
         # What lands in these zones is other people's words: a file there is read as\n\
         # untrusted, whoever synced it. Change the list to match this drive.\n\
         [integrity]\n\
         untrusted = [{untrusted}]\n",
        id = quoted(id.trim()),
        principal = quoted(principal.trim()),
        owner = quoted(owner.trim()),
        readers = readers.join(", "),
        untrusted = untrusted.join(", "),
    )
}

/// `text` with `agents init`'s tokens filled: the drive, the homeserver, the
/// owner and the bot. `{{id}}`, `{{name}}` and `{{date}}` stay, for
/// `agents new`.
fn render(text: &str, choices: &SeedChoices) -> String {
    text.replace("{{steward_menu}}", STEWARD_MENU)
        .replace("{{drive}}", &choices.decl.id)
        .replace("{{homeserver}}", choices.homeserver())
        .replace("{{owner}}", &quoted(choices.decl.owner.as_str()))
        .replace("{{bot}}", &quoted(&choices.bot))
}

/// Every file the seed holds, zone-relative, whether or not it is there.
pub fn files(choices: &SeedChoices) -> Vec<SeedFile> {
    let file = |path: String, text: String| SeedFile { path, text };
    let mut files = vec![
        file("README.md".to_owned(), render(README, choices)),
        file("AGENTS.md".to_owned(), RULES.to_owned()),
        file(
            drive::FILE_NAME.to_owned(),
            drive_toml(
                &choices.decl.id,
                &choices.decl.principal,
                choices.decl.owner.as_str(),
                &choices
                    .decl
                    .readers
                    .iter()
                    .map(|r| r.to_string())
                    .collect::<Vec<_>>(),
                choices.decl.local_only,
            ),
        ),
    ];
    let home = |dir: &str, agent_toml: String, soul: String| {
        vec![
            file(format!("{dir}/agent.toml"), agent_toml),
            file(format!("{dir}/SOUL.md"), soul),
            file(format!("{dir}/USER.md"), MEMORY.to_owned()),
            file(format!("{dir}/MEMORY.md"), MEMORY.to_owned()),
            file(format!("{dir}/journal/.keep"), String::new()),
            file(format!("{dir}/proposals/.keep"), String::new()),
        ]
    };
    files.extend(home(
        TEMPLATE_DIR,
        render(TEMPLATE_AGENT, choices),
        TEMPLATE_SOUL.to_owned(),
    ));
    for agent in &choices.with {
        files.extend(home(
            agent.id,
            render(agent.agent_toml, choices),
            render(agent.soul, choices),
        ));
    }
    files
}

/// The seed against the zone as it is: `exists` says whether a
/// zone-relative path is there (anything at all — a file, a folder, a link).
pub fn plan(choices: &SeedChoices, exists: impl Fn(&str) -> bool) -> SeedPlan {
    let mut write = Vec::new();
    let mut left = Vec::new();
    for file in files(choices) {
        if exists(&file.path) {
            left.push(file.path);
        } else {
            write.push(file);
        }
    }
    SeedPlan { write, left }
}

/// A new agent's name as `agents new` takes it: 1 to 64 characters, none
/// that would break the quoted strings the template puts it in.
pub fn check_name(name: &str) -> Result<(), String> {
    let chars = name.chars().count();
    if name.trim().is_empty() || chars > 64 {
        return Err(format!(
            "a name is 1 to 64 characters, and \"{name}\" is {chars}"
        ));
    }
    if name
        .chars()
        .any(|c| c == '"' || c == '\\' || c.is_control())
    {
        return Err(format!(
            "\"{name}\" holds a quote, a backslash or a control character; a name holds none"
        ));
    }
    Ok(())
}

/// The template's files for a new agent: `_template/`'s files (relative to
/// it) with `{{id}}`, `{{name}}` and `{{date}}` expanded, under `<id>/`. With
/// `soul`, that text is the new `SOUL.md` instead of the template's.
pub fn from_template(
    template: &[(String, String)],
    id: &str,
    name: &str,
    date: &str,
    soul: Option<&str>,
) -> Vec<SeedFile> {
    let expand = |text: &str| {
        text.replace("{{id}}", id)
            .replace("{{name}}", name)
            .replace("{{date}}", date)
    };
    template
        .iter()
        .map(|(rel, text)| SeedFile {
            path: format!("{id}/{rel}"),
            text: match soul {
                Some(soul) if rel == super::soul::FILE_NAME => soul.to_owned(),
                _ => expand(text),
            },
        })
        .collect()
}

/// The id of a proxy's `main` session in `drive`: derived from (drive,
/// agent, `main`), so every run of `agents init` and every host names the
/// same session (AD-368's caller-supplied id).
pub fn main_session_id(drive: &str, agent: &str) -> Ulid {
    let digest =
        Sha256::digest(format!("keeper.agents.session\n{drive}\n{agent}\nmain").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Ulid::from_bytes(bytes)
}

// ---------------------------------------------------------------------------
// Settings › Agents › Set up agents (UX-DR133)
// ---------------------------------------------------------------------------

/// One agent of the catalogue, as *Set up agents* lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedAgentVm {
    pub id: String,
    pub name: String,
    /// `proxy` or `steward`.
    pub kind: String,
    /// The drive the agent's soul was written for.
    pub home_drive: String,
}

/// A bot the person may run the seeded agents on: one of their own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedBotVm {
    /// `bot:{kind}:{base}#{target}`: what the request carries back.
    pub reference: String,
    /// The bot's name.
    pub name: String,
    /// Its provider's name.
    pub provider: String,
}

/// A synced folder that keeps agents (`[folder.agents]`), with what the form
/// starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedFolderVm {
    pub profile_id: String,
    /// The folder's name, as Sync shows it.
    pub name: String,
    /// The drive id `_drive.toml` names, or the one the form starts from.
    pub drive: String,
    /// The owner the form starts from: `_drive.toml`'s, else the signed-in
    /// Matrix account.
    pub owner: String,
    /// The readers the form starts from, sorted.
    pub readers: Vec<String>,
    /// The `local_only` the form starts from: `_drive.toml`'s, else this
    /// Mac's pin for the folder, else `false`.
    pub local_only: bool,
    /// Whether the zone already has a `_drive.toml` that reads: its id,
    /// owner, readers and `local_only` are then the file's, and
    /// `_drive.toml` is left.
    pub declared: bool,
    /// The catalogue ids ticked when the form opens: the agents whose soul
    /// was written for this folder's drive.
    pub preselected: Vec<String>,
    /// Why this folder cannot be seeded from this Mac, in one sentence.
    pub problem: Option<String>,
}

/// What *Set up agents* offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedOfferVm {
    pub folders: Vec<AgentSeedFolderVm>,
    pub catalogue: Vec<AgentSeedAgentVm>,
    /// Never preselected (S-20).
    pub bots: Vec<AgentSeedBotVm>,
    /// The signed-in Matrix accounts, for the owner and readers.
    pub accounts: Vec<String>,
}

/// One *Set up agents* request: preview and write take the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedReq {
    pub profile_id: String,
    pub drive: String,
    pub owner: String,
    pub readers: Vec<String>,
    pub local_only: bool,
    /// `None` until the person picks one; a request without it is refused.
    pub bot: Option<String>,
    /// Catalogue ids.
    pub with: Vec<String>,
}

/// What a seed would write and what it would leave, zone-relative.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedPlanVm {
    pub write: Vec<String>,
    pub left: Vec<String>,
}

/// What a seed wrote and left, and the agents written, each of which signs
/// in on its Settings › Agents row (`agents_copy_sign_in`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentSeedResultVm {
    pub profile_id: String,
    pub written: Vec<String>,
    pub left: Vec<String>,
    /// The ids of the agents whose `agent.toml` this seed wrote.
    pub agents: Vec<String>,
}

/// The catalogue as the form lists it.
pub fn catalogue_vm() -> Vec<AgentSeedAgentVm> {
    CATALOGUE
        .iter()
        .map(|agent| AgentSeedAgentVm {
            id: agent.id.to_owned(),
            name: agent.name.to_owned(),
            kind: agent.kind.to_owned(),
            home_drive: agent.home_drive.to_owned(),
        })
        .collect()
}

/// The catalogue ids whose soul was written for `drive`: what *Set up
/// agents* ticks for a folder of that drive.
pub fn preselected(drive: &str) -> Vec<String> {
    CATALOGUE
        .iter()
        .filter(|agent| agent.home_drive == drive)
        .map(|agent| agent.id.to_owned())
        .collect()
}

/// The person's own bots as seed choices, in their pin order: each bot whose
/// provider this build can speak to.
pub fn bot_choices(providers: &[ProviderRow], bots: &[Bot]) -> Vec<AgentSeedBotVm> {
    bots.iter()
        .filter_map(|bot| {
            let provider = providers
                .iter()
                .find(|row| row.provider.id == bot.provider_id)?;
            Some(AgentSeedBotVm {
                reference: ProviderRef::new(
                    provider.provider.kind.as_registry_str(),
                    &provider.provider.base_url,
                )
                .bot_reference(&bot.target),
                name: bot.name.clone(),
                provider: provider.provider.name.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
