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

fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
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
}
