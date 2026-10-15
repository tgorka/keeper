//! An agent's model and reasoning level, as a person sees and changes them
//! (story 100.2, R382).
//!
//! The home's `agent.toml` `[model]` is the one place both live: `bot` names
//! the bot, `effort` the level ([`crate::agents::home`]). Settings › Agents
//! lists them per agent with the bots the person may choose — their own
//! (`bots.toml` / ⌘9 rows), and only those that run locally where the agent
//! is `local_only` — and a save writes exactly those two keys
//! ([`crate::agents::home::with_model`]). Everything shown and every refusal
//! is decided here; the shell and the front decide none of it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::agents::home::{serves_local_models, AgentConfig, BotRef};
use crate::agents::seed::AgentSeedBotVm;
use crate::bots::chat::{effort_refusal, Effort};
use crate::bots::quirks::quirks;

/// What a save is refused with when the file's model is no longer the one
/// the row showed: someone else changed it meanwhile.
pub const CHANGED_UNDERNEATH: &str = "This agent's agent.toml names another model than the one shown here: it changed on the drive meanwhile. Look at it again and choose.";

/// What a save naming a bot that is not the person's is refused with.
pub const NOT_YOUR_BOT: &str =
    "That bot is not one of yours on this Mac. Choose one of the bots listed, or add it under Bots.";

/// What a person reads where their bots would be offered when `keeper.db`
/// could not be read (R381, R405): not an empty list, which would say they
/// have none.
pub const BOTS_UNREAD: &str = "Your bots could not be read on this Mac, so no other bot can be offered here. The reason is in keeper's log; look at Settings › Bots, then open this again.";

/// The person's bots as this Mac read them, or [`BOTS_UNREAD`]'s sentence
/// when they could not be read.
pub type Choices<'a> = Result<&'a [AgentSeedBotVm], &'a str>;

/// Why `drive`'s agents' models cannot be changed on a Mac that only pulls
/// it: a change made here would never reach the drive.
pub fn pull_only(drive: &str) -> String {
    format!("This Mac only pulls {drive}, so a change made here would never reach the drive. Change the model where the drive is written.")
}

/// What a save is refused with when `name`'s `agent.toml` changed between
/// the save's read and its write (R405): nothing was written over it.
pub fn changed_while_saving(name: &str) -> String {
    format!(
        "{}'s agent.toml changed while you were saving — reopen and try again.",
        capitalised(name)
    )
}

/// What a turn ends with in its room when the provider refused the level
/// it was sent (R381, R405): the agent, its bot and the level, and where to
/// change it.
pub fn level_refused(name: &str, bot: &str, effort: Effort) -> String {
    let name = capitalised(name);
    format!(
        "{name}'s bot {bot} does not accept reasoning level {}. Choose another level, or none, for {name} in Settings › Agents.",
        effort.as_str()
    )
}

/// One bot a person may run an agent on, with the levels its kind is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentModelBotVm {
    /// `bot:{kind}:{base}#{target}`: what a save carries back.
    pub reference: String,
    /// The bot's name, or the model it names when it is none of the
    /// person's bots (the agent's current one, kept so it can be shown).
    pub name: String,
    /// Its provider's name; empty for a bot that is none of the person's.
    pub provider: String,
    /// The levels this bot's kind is sent, lowest first; empty: none.
    pub efforts: Vec<Effort>,
}

/// An agent's model on its Settings › Agents row (100.2 #4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentModelVm {
    /// `[model].bot`, normalised.
    pub bot: String,
    /// `[model].effort`; `None`: the provider's own default.
    pub effort: Option<Effort>,
    /// The bots offered, the current one first when it is none of them.
    pub bots: Vec<AgentModelBotVm>,
    /// Why the model cannot be changed from this Mac, in one sentence.
    pub locked: Option<String>,
    /// Why the person's bots are not offered — they could not be read — in
    /// the sentence shown where they would be (R381, R405); `None` when
    /// `bots` is the person's list, however short.
    pub unread: Option<String>,
}

/// A save of one agent's model: the choice, and what the row showed, so a
/// file changed meanwhile is refused rather than overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentModelReq {
    pub profile_id: String,
    pub agent: String,
    pub bot: String,
    pub effort: Option<Effort>,
    pub shown_bot: String,
    pub shown_effort: Option<Effort>,
}

/// The model line an agent room's header draws where the home is on this
/// device (100.2 #7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentModelLineVm {
    /// The bot's name among the person's bots, else the model it names.
    pub bot: String,
    pub effort: Option<Effort>,
}

/// The levels `bot`'s kind is sent.
fn efforts_of(bot: &BotRef) -> Vec<Effort> {
    quirks(bot.kind).efforts.to_vec()
}

/// The person's bots `config`'s agent may run on: every one, or only those
/// that run locally where the agent is `local_only` (effective: its file's
/// or its drive's).
pub fn offered(config: &AgentConfig, choices: &[AgentSeedBotVm]) -> Vec<AgentModelBotVm> {
    choices
        .iter()
        .filter_map(|choice| {
            let bot = BotRef::parse(&choice.reference).ok()?;
            (!config.local_only || serves_local_models(bot.kind)).then(|| AgentModelBotVm {
                reference: bot.reference(),
                name: choice.name.clone(),
                provider: choice.provider.clone(),
                efforts: efforts_of(&bot),
            })
        })
        .collect()
}

/// The row's model: what `agent.toml` says, the bots offered, and `locked`
/// when this Mac cannot write the file. Bots that could not be read leave
/// the current one alone on offer, with the sentence saying why.
pub fn model_vm(
    config: &AgentConfig,
    choices: Choices<'_>,
    locked: Option<String>,
) -> AgentModelVm {
    let current = config.bot.reference();
    let (mut bots, unread) = match choices {
        Ok(choices) => (offered(config, choices), None),
        Err(sentence) => (Vec::new(), Some(sentence.to_owned())),
    };
    if !bots.iter().any(|bot| bot.reference == current) {
        bots.insert(
            0,
            AgentModelBotVm {
                reference: current.clone(),
                name: config.bot.target.clone(),
                provider: String::new(),
                efforts: efforts_of(&config.bot),
            },
        );
    }
    AgentModelVm {
        bot: current,
        effort: config.effort,
        bots,
        locked,
        unread,
    }
}

/// The header's line for `config`'s agent.
pub fn line(config: &AgentConfig, choices: &[AgentSeedBotVm]) -> AgentModelLineVm {
    let current = config.bot.reference();
    let bot = choices
        .iter()
        .find(|choice| BotRef::parse(&choice.reference).is_ok_and(|bot| bot.reference() == current))
        .map_or_else(|| config.bot.target.clone(), |choice| choice.name.clone());
    AgentModelLineVm {
        bot,
        effort: config.effort,
    }
}

/// `req` against the home as it reads now: the row showed this very model,
/// the bot is one offered (or the current one, its level changed), and the
/// level is one its kind is sent. Another bot than the current one, where
/// the person's bots could not be read, is refused with that sentence. The
/// file is parsed again before it is written, so this is the sentence a
/// person reads first, not the only guard.
pub fn check(
    req: &AgentModelReq,
    config: &AgentConfig,
    choices: Choices<'_>,
) -> Result<BotRef, String> {
    let shown = BotRef::parse(&req.shown_bot).map(|bot| bot.reference());
    if shown.as_deref() != Ok(config.bot.reference().as_str()) || req.shown_effort != config.effort
    {
        return Err(CHANGED_UNDERNEATH.to_owned());
    }
    let bot = BotRef::parse(&req.bot).map_err(|reason| format!("{reason}."))?;
    let reference = bot.reference();
    if reference != config.bot.reference()
        && !offered(config, choices.map_err(str::to_owned)?)
            .iter()
            .any(|offer| offer.reference == reference)
    {
        return Err(NOT_YOUR_BOT.to_owned());
    }
    if let Some(reason) = req
        .effort
        .and_then(|effort| effort_refusal(bot.kind, effort))
    {
        return Err(format!("{}: {reason}.", capitalised(&config.name)));
    }
    Ok(bot)
}

/// `/model` where the agent's home is not on this device (R415): only a
/// device with its drive can write its `agent.toml`.
pub const HOME_NOT_HERE: &str = "Your agent's home is not on this device, so its model is changed where its drive is: type /model there, or use Settings › Agents on that Mac.";

/// The word after `/model` that sets the provider's own level.
pub const DEFAULT_LEVEL: &str = "default";

/// What `/model` in an agent's DM asks for (story 100.5, DW-1431, R415).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelCommand {
    /// Nothing followed the name: say what the agent runs on.
    Show,
    /// Save this bot (`bot:` reference) and level, as Settings › Agents does.
    Set { bot: String, effort: Option<Effort> },
}

/// What follows `/model`, read against `config` — the agent's `agent.toml` as
/// it reads now — and the person's bots. The last word, when it is a level
/// (`low`, `medium`, `high`, `xhigh`) or [`DEFAULT_LEVEL`], is the level; the
/// rest names the bot by its name among the bots this agent is offered (any
/// case) or its `bot:` reference, and nothing there keeps the current bot. A
/// level left out stays as it is. Whether the level is one the bot's kind is
/// sent is [`check`]'s, as for a save from Settings.
pub fn command(
    args: Option<&str>,
    config: &AgentConfig,
    choices: Choices<'_>,
) -> Result<ModelCommand, String> {
    let Some(args) = args.map(str::trim).filter(|args| !args.is_empty()) else {
        return Ok(ModelCommand::Show);
    };
    let (named, level) = match args.rsplit_once(char::is_whitespace) {
        Some((rest, last)) => match level_word(last) {
            Some(level) => (rest.trim(), Some(level)),
            None => (args, None),
        },
        None => match level_word(args) {
            Some(level) => ("", Some(level)),
            None => (args, None),
        },
    };
    let effort = level.unwrap_or(config.effort);
    let current = config.bot.reference();
    if named.is_empty() {
        return Ok(ModelCommand::Set {
            bot: current,
            effort,
        });
    }
    let reference = BotRef::parse(named).ok().map(|bot| bot.reference());
    let is_current = reference.as_deref() == Some(current.as_str())
        || named.eq_ignore_ascii_case(&config.bot.target);
    let offers = match choices {
        Ok(choices) => offered(config, choices),
        Err(_) if is_current => Vec::new(),
        Err(sentence) => return Err(sentence.to_owned()),
    };
    let bot = offers
        .iter()
        .find(|offer| {
            offer.name.eq_ignore_ascii_case(named) || Some(&offer.reference) == reference.as_ref()
        })
        .map(|offer| offer.reference.clone())
        .or_else(|| is_current.then(|| current.clone()));
    match bot {
        Some(bot) => Ok(ModelCommand::Set { bot, effort }),
        None => Err(not_a_bot(&config.name, named, &offers)),
    }
}

/// A level word: one of [`Effort`]'s, or [`DEFAULT_LEVEL`] (`Some(None)`).
fn level_word(word: &str) -> Option<Option<Effort>> {
    let word = word.to_ascii_lowercase();
    if word == DEFAULT_LEVEL {
        return Some(None);
    }
    Effort::parse(&word).map(Some)
}

/// `/model` naming a bot this agent is not offered.
fn not_a_bot(name: &str, named: &str, offers: &[AgentModelBotVm]) -> String {
    let names: Vec<&str> = offers.iter().map(|offer| offer.name.as_str()).collect();
    let list = if names.is_empty() {
        "none of your bots is offered to it on this device".to_owned()
    } else {
        format!("it can run on {}", names.join(", "))
    };
    format!(
        "{} cannot run on \"{named}\": {list}. Nothing was changed.",
        capitalised(name)
    )
}

/// What `/model` alone says: the agent's bot as the person's bots name it,
/// and its level.
pub fn running(config: &AgentConfig, choices: &[AgentSeedBotVm]) -> String {
    format!(
        "{} runs on {}.",
        capitalised(&config.name),
        shown(&line(config, choices))
    )
}

/// What `/model` says once its save landed: the agent's next turn runs on it.
pub fn now_running(name: &str, model: &AgentModelLineVm) -> String {
    format!(
        "{} now runs on {}, from its next turn.",
        capitalised(name),
        shown(model)
    )
}

fn shown(model: &AgentModelLineVm) -> String {
    match model.effort {
        Some(effort) => format!("{} at reasoning level {}", model.bot, effort.as_str()),
        None => format!("{} at the provider's own reasoning level", model.bot),
    }
}

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Each `agent.toml` key keeper writes that an older build refuses, with
/// the first keeper release that reads it and whether a file, as read,
/// carries it (story 101.6, R422; R460). A host whose build predates a key
/// refuses the whole home once the key reaches its drive — its key set is
/// closed — and stops serving the agent. `[model].effort` came with story
/// 100.2 (bc87bc37); `web_search` as a tool, `[web_search]` and `[run]`
/// with epic 101 (stories 101.4 and 101.5): all first released after
/// v0.10.3.
pub const KEY_SINCE: &[KeySince] = &[
    ("[model].effort", "0.10.4", |config| config.effort.is_some()),
    (
        "web_search in [tools].allow",
        "0.10.4",
        crate::agents::web_search::is_on,
    ),
    ("[web_search]", "0.10.4", |config| config.web_search_said),
    ("[run]", "0.10.4", |config| config.run.said),
];

/// One row of [`KEY_SINCE`]: the key as a save's sentence names it, the
/// first keeper release that reads it, and whether a parsed `agent.toml`
/// carries it.
pub type KeySince = (&'static str, &'static str, fn(&AgentConfig) -> bool);

/// A host serving agents, as its live manifest says: its slug, its build's
/// keeper version and the `<drive>/<id>` of each agent it hosts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServingHost {
    pub host: String,
    pub version: String,
    pub agents: Vec<String>,
}

/// What a save that writes a key some build cannot read says when this Mac
/// could not read which hosts serve the agent (story 101.6, R436): it cannot
/// tell that no older host serves it, so nothing is written.
pub const HOSTS_UNREAD: &str = "keeper could not read which hosts serve your agents, so it cannot tell whether an older one would stop serving this agent; nothing was saved. The reason is in keeper's log; try again.";

/// The other live hosts serving this person's agents, as this Mac read their
/// manifests, or [`HOSTS_UNREAD`]'s sentence when they could not be read. A
/// successful read that found none is an empty list, never an error.
pub type Inventory<'a> = Result<&'a [ServingHost], &'a str>;

/// The live hosts `manifests` name — each `(state key, sender, content)` of
/// a `dev.keeper.agent.host` in a principal's control room, believed only
/// from one of `agents` ([`crate::agents::host::accept`]) — other than `me`
/// (this device's own host, whose build is the one saving), live at
/// `now_ms`; each host once (story 101.6, R422).
pub fn serving_from<'a>(
    manifests: impl IntoIterator<Item = (&'a str, &'a matrix_sdk::ruma::UserId, &'a serde_json::Value)>,
    agents: &[matrix_sdk::ruma::OwnedUserId],
    now_ms: u64,
    me: Option<&str>,
) -> Vec<ServingHost> {
    let mut hosts: Vec<ServingHost> = manifests
        .into_iter()
        .filter_map(|(key, sender, content)| {
            crate::agents::host::accept(key, sender, content, agents).ok()
        })
        .filter(|manifest| Some(manifest.host.as_str()) != me && manifest.is_live(now_ms))
        .map(|manifest| ServingHost {
            host: manifest.host,
            version: manifest.version,
            agents: manifest.agents,
        })
        .collect();
    hosts.sort_by(|a, b| (&a.host, &a.version, &a.agents).cmp(&(&b.host, &b.version, &b.agents)));
    hosts.dedup();
    hosts
}

/// Whether a build of keeper `version` reads a key first released in
/// `since`. The numbers are compared (`major.minor.patch`; anything after a
/// `-` or `+` is not read); a version that does not read as one is no proof
/// the key is read, so it does not.
pub fn reads_since(version: &str, since: &str) -> bool {
    fn triple(version: &str) -> Option<(u64, u64, u64)> {
        let core = version.trim().trim_start_matches('v');
        let core = core.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|part| part.parse::<u64>().ok());
        let triple = (parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(triple)
    }
    match (triple(version), triple(since)) {
        (Some(have), Some(need)) => have >= need,
        _ => false,
    }
}

/// The keys of [`KEY_SINCE`] `config` carries.
fn written_keys(config: &AgentConfig) -> Vec<(&'static str, &'static str)> {
    KEY_SINCE
        .iter()
        .filter(|(_, _, carried)| carried(config))
        .map(|(key, since, _)| (*key, *since))
        .collect()
}

/// Whether saving `config` — the `agent.toml` of `agent` (`<drive>/<id>`)
/// as a person's save would leave it — would hand a key to a host serving
/// that agent whose build predates it: that host would refuse the home and
/// stop serving `name` (story 101.6, R422; every save of Settings › Agents,
/// R460). Each such host is one sentence naming the keys it does not read;
/// nothing is written while any stands. A save writing such a key while the
/// hosts could not be read is refused with the inventory's sentence (R436);
/// one writing none — a file a build before them reads — is not held to
/// the hosts at all.
pub fn older_hosts(
    config: &AgentConfig,
    agent: &str,
    serving: Inventory<'_>,
) -> Result<(), String> {
    let keys = written_keys(config);
    if keys.is_empty() {
        return Ok(());
    }
    let said: Vec<String> = serving
        .map_err(str::to_owned)?
        .iter()
        .filter(|host| host.agents.iter().any(|served| served == agent))
        .filter_map(|host| {
            let unread: Vec<(&str, &str)> = keys
                .iter()
                .copied()
                .filter(|(_, since)| !reads_since(&host.version, since))
                .collect();
            (!unread.is_empty()).then(|| predates(host, &unread, &config.name))
        })
        .collect();
    if said.is_empty() {
        Ok(())
    } else {
        Err(said.join(" "))
    }
}

/// What a save says when `host` serves `name` on a build older than each
/// of `keys` (`(key, since)`): the keys, and the release that reads them
/// all.
fn predates(host: &ServingHost, keys: &[(&str, &str)], name: &str) -> String {
    let since = keys
        .iter()
        .map(|(_, since)| *since)
        .reduce(|a, b| if reads_since(a, b) { a } else { b })
        .unwrap_or_default();
    let names: Vec<&str> = keys.iter().map(|(key, _)| *key).collect();
    let listed = match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    };
    format!(
        "{} runs keeper {}, which does not read {listed} (keeper {since} does): saving it would stop {} serving {}. Update keeper on {} first; nothing was saved.",
        host.host,
        host.version,
        host.host,
        capitalised(name),
        host.host
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{drive, home};

    const OPUS: &str = "bot:openai:https://provider.example:8452#claude-opus";
    const QWEN: &str = "bot:ollama:http://localhost:11434#qwen3";
    const HERMES: &str = "bot:hermes:http://localhost:8642#nixie";

    fn choices() -> Vec<AgentSeedBotVm> {
        [(OPUS, "Opus"), (QWEN, "Qwen"), (HERMES, "Nixie")]
            .into_iter()
            .map(|(reference, name)| AgentSeedBotVm {
                reference: reference.to_owned(),
                name: name.to_owned(),
                provider: "p".to_owned(),
                efforts: Vec::new(),
            })
            .collect()
    }

    fn agent(bot: &str, effort: &str, local_only: bool) -> AgentConfig {
        let decl = drive::parse(&format!(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\nlocal_only = {local_only}\n"
        ))
        .expect("drive");
        home::parse_agent_toml(
            &format!("version = 1\nid = \"amelia\"\nname = \"amelia\"\nkind = \"steward\"\nmatrix_user = \"@amelia:example.org\"\n\n[model]\nbot = \"{bot}\"\n{effort}"),
            "amelia",
            &decl,
        )
        .expect("home")
    }

    fn req(config: &AgentConfig, bot: &str, effort: Option<Effort>) -> AgentModelReq {
        AgentModelReq {
            profile_id: "tg".to_owned(),
            agent: "amelia".to_owned(),
            bot: bot.to_owned(),
            effort,
            shown_bot: config.bot.reference(),
            shown_effort: config.effort,
        }
    }

    /// 100.2 #4: the person's bots, each with the levels its kind is sent;
    /// a `local_only` agent is offered only the ones that run locally.
    #[test]
    fn an_agent_is_offered_the_persons_bots_its_home_allows() {
        let open = model_vm(
            &agent(OPUS, "effort = \"xhigh\"\n", false),
            Ok(&choices()),
            None,
        );
        assert_eq!(open.effort, Some(Effort::Xhigh));
        let offered: Vec<(&str, Vec<Effort>)> = open
            .bots
            .iter()
            .map(|bot| (bot.name.as_str(), bot.efforts.clone()))
            .collect();
        assert_eq!(
            offered,
            vec![
                ("Opus", Effort::ALL.to_vec()),
                ("Qwen", vec![Effort::Low, Effort::Medium, Effort::High]),
                ("Nixie", vec![]),
            ]
        );

        let local = model_vm(&agent(QWEN, "", true), Ok(&choices()), None);
        let names: Vec<&str> = local.bots.iter().map(|bot| bot.name.as_str()).collect();
        assert_eq!(names, vec!["Qwen"]);

        // The agent's own bot, none of the person's, is listed first so the
        // row can show it.
        let other = "bot:openai:https://elsewhere.example#gpt";
        let theirs = model_vm(&agent(other, "", false), Ok(&choices()), None);
        assert_eq!(theirs.bots[0].name, "gpt");
        assert_eq!(theirs.bots.len(), 4);
        assert_eq!(line(&agent(other, "", false), &choices()).bot, "gpt");
        assert_eq!(
            line(&agent(OPUS, "effort = \"high\"\n", false), &choices()),
            AgentModelLineVm {
                bot: "Opus".to_owned(),
                effort: Some(Effort::High)
            }
        );
    }

    /// 100.2 #4, #8: a save is checked against the file as it reads now —
    /// changed meanwhile, a bot that is not the person's, a level its kind
    /// is not sent — each refused with its own sentence.
    #[test]
    fn a_save_is_refused_when_the_file_moved_or_the_choice_is_not_offered() {
        let config = agent(OPUS, "effort = \"high\"\n", false);
        assert!(check(
            &req(&config, QWEN, Some(Effort::Low)),
            &config,
            Ok(&choices())
        )
        .is_ok());

        let mut stale = req(&config, QWEN, None);
        stale.shown_effort = Some(Effort::Xhigh);
        assert_eq!(
            check(&stale, &config, Ok(&choices())),
            Err(CHANGED_UNDERNEATH.to_owned())
        );
        assert_eq!(
            check(
                &req(&config, "bot:openai:https://elsewhere.example#gpt", None),
                &config,
                Ok(&choices())
            ),
            Err(NOT_YOUR_BOT.to_owned())
        );
        let local = agent(QWEN, "", true);
        assert_eq!(
            check(&req(&local, OPUS, None), &local, Ok(&choices())),
            Err(NOT_YOUR_BOT.to_owned())
        );
        assert_eq!(
            check(
                &req(&config, QWEN, Some(Effort::Xhigh)),
                &config,
                Ok(&choices())
            ),
            Err(
                "Amelia: ollama bots take low, medium or high, and xhigh is none of them."
                    .to_owned()
            )
        );
        // The current bot stays choosable for a level change.
        let other = agent("bot:openai:https://elsewhere.example#gpt", "", false);
        let keep = req(
            &other,
            "bot:openai:https://elsewhere.example#gpt",
            Some(Effort::Low),
        );
        assert!(check(&keep, &other, Ok(&choices())).is_ok());
    }

    /// R405 (R100M1-04): bots that could not be read are not an empty list.
    /// The row offers its current bot alone with the sentence saying why;
    /// its level can still change, another bot is refused with the same
    /// sentence. A person with no bots at all gets no sentence.
    #[test]
    fn bots_that_could_not_be_read_say_so_where_the_choices_would_be() {
        let config = agent(OPUS, "effort = \"high\"\n", false);
        let unread = model_vm(&config, Err(BOTS_UNREAD), None);
        assert_eq!(unread.unread.as_deref(), Some(BOTS_UNREAD));
        let offered: Vec<&str> = unread
            .bots
            .iter()
            .map(|bot| bot.reference.as_str())
            .collect();
        assert_eq!(offered, vec![config.bot.reference().as_str()]);
        let none = model_vm(&config, Ok(&[]), None);
        assert_eq!(none.unread, None);

        assert!(check(
            &req(&config, OPUS, Some(Effort::Low)),
            &config,
            Err(BOTS_UNREAD)
        )
        .is_ok());
        assert_eq!(
            check(&req(&config, QWEN, None), &config, Err(BOTS_UNREAD)),
            Err(BOTS_UNREAD.to_owned())
        );
    }

    /// DW-1431 (R415): `/model` names a bot by its name or reference and a
    /// level by its word, last; a level alone keeps the bot, a bot alone
    /// keeps the level, `default` is the provider's own, nothing is a look.
    #[test]
    fn model_typed_in_the_dm_reads_a_bot_and_a_level() {
        let config = agent(OPUS, "effort = \"high\"\n", false);
        let set = |args: &str| command(Some(args), &config, Ok(&choices()));
        let qwen = BotRef::parse(QWEN).expect("bot").reference();
        let opus = config.bot.reference();
        assert_eq!(
            command(None, &config, Ok(&choices())),
            Ok(ModelCommand::Show)
        );
        assert_eq!(
            command(Some("  "), &config, Ok(&choices())),
            Ok(ModelCommand::Show)
        );
        assert_eq!(
            set("qwen low"),
            Ok(ModelCommand::Set {
                bot: qwen.clone(),
                effort: Some(Effort::Low)
            })
        );
        assert_eq!(
            set(QWEN),
            Ok(ModelCommand::Set {
                bot: qwen,
                effort: Some(Effort::High)
            }),
            "a bot alone keeps the level"
        );
        assert_eq!(
            set("XHIGH"),
            Ok(ModelCommand::Set {
                bot: opus.clone(),
                effort: Some(Effort::Xhigh)
            }),
            "a level alone keeps the bot"
        );
        assert_eq!(
            set("Opus default"),
            Ok(ModelCommand::Set {
                bot: opus,
                effort: None
            })
        );
        // A bot this agent is not offered is refused naming the ones it is.
        let refused = set("gpt-9 low").expect_err("not a bot");
        assert!(refused.contains("Opus, Qwen, Nixie"), "{refused}");
        // A local-only agent is not offered a remote bot by name.
        let local = agent(QWEN, "", true);
        assert!(command(Some("Opus"), &local, Ok(&choices())).is_err());
        // Bots that could not be read: the current bot's level still changes,
        // another bot is refused with that sentence.
        assert!(matches!(
            command(Some("claude-opus medium"), &config, Err(BOTS_UNREAD)),
            Ok(ModelCommand::Set {
                effort: Some(Effort::Medium),
                ..
            })
        ));
        assert_eq!(
            command(Some("Qwen"), &config, Err(BOTS_UNREAD)),
            Err(BOTS_UNREAD.to_owned())
        );
    }

    /// 101.6 #3 (R422): a version is compared by its numbers; a version
    /// that does not read as one is no proof of reading the key.
    #[test]
    fn a_version_reads_a_key_from_its_first_release_on() {
        assert!(reads_since("0.10.4", "0.10.4"));
        assert!(reads_since("0.11.0", "0.10.4"));
        assert!(reads_since("1.0.0", "0.10.4"));
        assert!(reads_since("0.10.10", "0.10.4"));
        assert!(reads_since("v0.10.4+mac", "0.10.4"));
        assert!(!reads_since("0.10.3", "0.10.4"));
        assert!(!reads_since("0.9.99", "0.10.4"));
        assert!(!reads_since("electra", "0.10.4"));
        assert!(!reads_since("0.10", "0.10.4"));
    }

    /// 101.6 #3 (R422): saving `[model].effort` for an agent a host serves
    /// on a build that predates it is refused naming that host; a host that
    /// reads it, one serving another agent, or a save without the key,
    /// passes.
    #[test]
    fn a_save_a_serving_host_cannot_read_is_refused_naming_the_host() {
        let host = |slug: &str, version: &str, agents: &[&str]| ServingHost {
            host: slug.to_owned(),
            version: version.to_owned(),
            agents: agents.iter().map(|agent| (*agent).to_owned()).collect(),
        };
        let with = agent(OPUS, "effort = \"high\"\n", false);
        let without = agent(OPUS, "", false);
        let electra = host("electra", "0.10.3", &["tgdrive/amelia"]);
        let said = older_hosts(&with, "tgdrive/amelia", Ok(std::slice::from_ref(&electra)))
            .expect_err("electra predates it");
        assert!(
            said.contains("electra") && said.contains("0.10.3"),
            "{said}"
        );
        assert_eq!(
            older_hosts(
                &without,
                "tgdrive/amelia",
                Ok(std::slice::from_ref(&electra))
            ),
            Ok(())
        );
        assert_eq!(
            older_hosts(
                &with,
                "tgdrive/amelia",
                Ok(&[host("electra", "0.10.4", &["tgdrive/amelia"])])
            ),
            Ok(())
        );
        assert_eq!(
            older_hosts(
                &with,
                "tgdrive/amelia",
                Ok(&[host("electra", "0.10.3", &["tgdrive/nixi"])])
            ),
            Ok(())
        );
        let both = older_hosts(
            &with,
            "tgdrive/amelia",
            Ok(&[electra, host("kalypso", "0.9.0", &["tgdrive/amelia"])]),
        )
        .expect_err("both predate it");
        assert!(
            both.contains("electra") && both.contains("kalypso"),
            "{both}"
        );
    }

    /// 101.6 #3 (R436, review 07): a save writing a key some build cannot
    /// read is refused when the hosts could not be read, with the inventory's
    /// own sentence; a read that found no host lets it through, and a save
    /// writing no such key is not held to the hosts at all.
    #[test]
    fn an_unread_host_inventory_refuses_a_save_only_a_new_build_reads() {
        let with = agent(OPUS, "effort = \"high\"\n", false);
        let without = agent(OPUS, "", false);
        assert_eq!(
            older_hosts(&with, "tgdrive/amelia", Err(HOSTS_UNREAD)),
            Err(HOSTS_UNREAD.to_owned())
        );
        assert_eq!(older_hosts(&with, "tgdrive/amelia", Ok(&[])), Ok(()));
        assert_eq!(
            older_hosts(&without, "tgdrive/amelia", Err(HOSTS_UNREAD)),
            Ok(())
        );
    }

    /// R460: every key epic 101 adds to `agent.toml` — `web_search` named
    /// in `[tools].allow`, a `[web_search]` table (even one that only says
    /// `ask = true`), a `[run]` table — is held to the serving hosts as
    /// `[model].effort` is: a 0.10.3 host is named, once, with each key it
    /// does not read; a 0.10.4 one is not; an unread inventory refuses; a
    /// file a build before them reads passes either way.
    #[test]
    fn every_key_epic_101_writes_is_held_to_the_serving_hosts() {
        let host = |version: &str| ServingHost {
            host: "electra".to_owned(),
            version: version.to_owned(),
            agents: vec!["tgdrive/amelia".to_owned()],
        };
        let (old, new) = ([host("0.10.3")], [host("0.10.4")]);
        let plain = agent(OPUS, "\n[tools]\nallow = [\"drive_read\"]\n", false);
        for (extra, key) in [
            (
                "\n[tools]\nallow = [\"drive_read\", \"web_search\"]\n",
                "web_search in [tools].allow",
            ),
            ("\n[web_search]\nask = true\n", "[web_search]"),
            ("\n[run]\nmode = \"off\"\npaths = []\n", "[run]"),
        ] {
            let with = agent(OPUS, extra, false);
            let said = older_hosts(&with, "tgdrive/amelia", Ok(&old)).expect_err(key);
            assert!(
                said.starts_with(&format!(
                    "electra runs keeper 0.10.3, which does not read {key} (keeper 0.10.4 does)"
                )),
                "{said}"
            );
            assert_eq!(older_hosts(&with, "tgdrive/amelia", Ok(&new)), Ok(()));
            assert_eq!(
                older_hosts(&with, "tgdrive/amelia", Err(HOSTS_UNREAD)),
                Err(HOSTS_UNREAD.to_owned())
            );
        }
        let all = agent(
            OPUS,
            "effort = \"high\"\n\n[tools]\nallow = [\"web_search\", \"run\"]\n\n[web_search]\nask = false\n\n[run]\nmode = \"full\"\n",
            false,
        );
        let said = older_hosts(&all, "tgdrive/amelia", Ok(&old)).expect_err("all four");
        assert_eq!(said.matches("electra runs keeper").count(), 1, "{said}");
        assert!(
            said.contains(
                "[model].effort, web_search in [tools].allow, [web_search] or [run] (keeper 0.10.4 does)"
            ),
            "{said}"
        );
        assert_eq!(older_hosts(&plain, "tgdrive/amelia", Ok(&old)), Ok(()));
        assert_eq!(
            older_hosts(&plain, "tgdrive/amelia", Err(HOSTS_UNREAD)),
            Ok(())
        );
    }

    /// 101.6 #3 (R422): the hosts a save is checked against are the live
    /// manifests an agent of these zones sent, this device's own left out.
    #[test]
    fn the_hosts_a_save_is_checked_against_are_the_live_ones_agents_sent() {
        use matrix_sdk::ruma::{OwnedUserId, UserId};
        let manifest = |host: &str, expires: &str| {
            serde_json::json!({
                "v": 1, "host": host, "principal": "tgorka", "version": "0.10.3",
                "always_on": true, "tools": [], "drives": [], "bots": [],
                "agents": ["tgdrive/tola-grey"],
                "renewed_at": "2026-10-15T11:59:00Z", "expires_at": expires,
            })
        };
        let tola = UserId::parse("@tola:example.org").expect("tola");
        let eve = UserId::parse("@eve:example.org").expect("eve");
        let agents: Vec<OwnedUserId> = vec![tola.clone()];
        let now = 1_760_529_600_000; // 2025-10-15T12:00:00Z
        let live = manifest("electra", "2099-01-01T00:00:00Z");
        let gone = manifest("delectra", "2020-01-01T00:00:00Z");
        let mine = manifest("hesperia", "2099-01-01T00:00:00Z");
        let forged = manifest("kalypso", "2099-01-01T00:00:00Z");
        let hosts = serving_from(
            [
                ("electra", &*tola, &live),
                ("electra", &*tola, &live),
                ("delectra", &*tola, &gone),
                ("hesperia", &*tola, &mine),
                ("kalypso", &*eve, &forged),
            ],
            &agents,
            now,
            Some("hesperia"),
        );
        assert_eq!(
            hosts,
            [ServingHost {
                host: "electra".to_owned(),
                version: "0.10.3".to_owned(),
                agents: vec!["tgdrive/tola-grey".to_owned()],
            }]
        );
    }
}
