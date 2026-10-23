//! Slash commands in the messenger composer (Story 100.5, R385; the agent
//! rooms' `/new` and `/model` left with the agents' Matrix path, 108.2).
//!
//! **What a `/` line means is decided here and nowhere else** (AD-55): the
//! composer asks [`crate::vm::ChatCommandPreviewVm`] what its draft is and obeys.
//! A matcher in TypeScript beside this one would be a second opinion about what
//! `/to` means, and the one a person is looking at is always the other one.
//!
//! # The rules
//!
//! 1. **Exact beats prefix, and an alias is exact** — name, then alias, then
//!    the first prefix in registry order ([`COMMANDS`] order is a decision).
//! 2. **A command that cannot work in this room is absent, not disabled**
//!    (AD-27). A command whose precondition is merely unmet right now — a
//!    power level, a reply in progress — is listed with the reason.
//! 3. **An unknown command is refused in place and is never sent as text.**
//!    The refusal names the nearest command offered here.
//! 4. **`//` sends a literal slash** — `//etc` is the text `/etc`
//!    ([`ESCAPE_HINT`]), at any length.
//! 5. **A command line has a bound, and crossing it is a refusal**
//!    ([`TOO_LONG`]): one line, one leading slash, more than
//!    [`MAX_COMMAND_CHARS`] bytes is
//!    refused in place, never sent as the text of a command nobody ran (R393).
//!
//! # Where each command acts
//!
//! Every command runs through an action keeper already had or that lives in
//! Rust beside the others: `/me` is an `m.emote` through the single send gate
//! (`send::submit`, the composer's own trigger), `/shrug` and `/plain` are
//! ordinary text through the same gate, `/topic`, `/invite` and `/leave` are
//! `Accounts::room_set_topic`, `room_invite` and `room_leave`. Nothing here
//! speaks Matrix: it only decides.

use matrix_sdk::ruma::UserId;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The one sentence the empty state and `/help` both teach, because rule 4 is
/// the only part of this surface a person cannot guess from the menu.
pub const ESCAPE_HINT: &str =
    "To send a message that starts with a slash, double it: //etc sends /etc as text.";

/// The longest draft this module will reason about as a possible command.
///
/// A bound rather than a taste: the draft is tokenized on every keystroke, and
/// a pasted megabyte beginning with a slash would be scanned on each one. A
/// command line is one short line by construction, so anything longer is prose
/// and is treated as prose without being measured twice.
pub const MAX_COMMAND_CHARS: usize = 512;

/// What a command does with the text after its name (Story 61.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ArgMode {
    /// Nothing may follow.
    None,
    /// One argument is required, and its absence is a refusal naming the hint
    /// rather than a command that quietly did nothing.
    Required,
    /// Everything after the name is one optional argument, verbatim.
    OptionalRest,
}

/// The token typed after a leading slash, lowercased, or `None` when the draft
/// is not a command line at all (Story 61.9).
///
/// A command line is **one line, one slash, nothing before it** — the note
/// editor's `slash-menu.ts:22` narrowness restated for a textarea. A doubled
/// slash is the escape and is therefore not a command line, which is why this
/// returns `None` for it rather than a token of `"/etc"`.
pub fn command_token(draft: &str) -> Option<&str> {
    if draft.len() > MAX_COMMAND_CHARS || draft.contains('\n') {
        return None;
    }
    let rest = draft.strip_prefix('/')?;
    if rest.starts_with('/') {
        return None;
    }
    Some(match rest.split_once(char::is_whitespace) {
        Some((token, _)) => token,
        None => rest,
    })
}

/// Levenshtein distance in characters, two rows of state.
///
/// Written here rather than taken as a dependency: it runs over a handful of
/// seven-character names once per refusal, and the crate's rule is that a new
/// dependency needs a reason bigger than twenty lines.
fn distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0usize; right.len() + 1];
    for (i, lc) in left.chars().enumerate() {
        current[0] = i + 1;
        for (j, rc) in right.iter().enumerate() {
            let substitute = previous[j] + usize::from(lc != *rc);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// `/me`
pub const DESC_ME: &str = "Send the text as an action, the way \"* Ana waves\" reads.";
/// `/shrug`
pub const DESC_SHRUG: &str = "Send ¯\\_(ツ)_/¯, before your text if you add some.";
/// `/plain`
pub const DESC_PLAIN: &str = "Send the text exactly as typed, even when it looks like a command.";
/// `/topic`
pub const DESC_TOPIC: &str = "Change this room's topic for everyone in it.";
/// `/invite`
pub const DESC_INVITE: &str = "Invite someone to this room by their Matrix id.";
/// `/leave`
pub const DESC_LEAVE: &str = "Leave this room, after you confirm.";
/// `/help`
pub const DESC_HELP: &str = "List the commands this room offers.";

/// What `/shrug` sends.
pub const SHRUG: &str = "¯\\_(ツ)_/¯";

/// The power levels do not let the person change the topic.
pub const NEED_TOPIC_POWER: &str =
    "Your power level in this room does not let you change its topic.";

/// The power levels do not let the person invite.
pub const NEED_INVITE_POWER: &str = "Your power level in this room does not let you invite people.";

/// `/me` while a reply is pending: an emote carries no reply.
pub const NEED_NO_REPLY: &str =
    "An action is sent as its own message, not as a reply. Cancel the reply first.";

/// A real command typed in a room that does not offer it.
pub const NOT_HERE: &str =
    "This room does not offer it, so nothing was sent. Type /help for the commands that are.";

/// Nothing offered here is close to what was typed.
pub const UNKNOWN_NO_NEAR: &str =
    "keeper has no command by that name here, and nothing in the list is close to it. Nothing \
     was sent. Type /help for the list.";

/// A one-line `/` draft over [`MAX_COMMAND_CHARS`] bytes (rule 5).
pub const TOO_LONG: &str = "That line is too long for a command, so nothing was sent. Shorten it, \
     or start it with // to send it as a message.";

/// How far a typo may be from a name and still be named (`histroy → history`,
/// not `hello → help`).
const NEAREST_MAX_DISTANCE: usize = 3;

/// Which room a composer is in, as far as commands care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomKind {
    /// A messenger room.
    Chat,
}

/// Which rooms offer a command (rule 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Chats only.
    Chat,
    /// Every room with a composer.
    Everywhere,
}

/// What must hold right now before a command may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Need {
    /// The person's power level allows `m.room.topic`.
    SetTopic,
    /// The person's power level allows inviting.
    Invite,
    /// No reply is pending in the composer.
    NoReply,
}

impl Need {
    /// What the person reads when this does not hold.
    pub fn sentence(self) -> &'static str {
        match self {
            Need::SetTopic => NEED_TOPIC_POWER,
            Need::Invite => NEED_INVITE_POWER,
            Need::NoReply => NEED_NO_REPLY,
        }
    }
}

/// One command in the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// The name after the slash, lowercase.
    pub name: &'static str,
    /// Other exact spellings.
    pub aliases: &'static [&'static str],
    /// The one sentence the menu and `/help` show.
    pub description: &'static str,
    /// What may follow the name.
    pub args: ArgMode,
    /// What the argument is, in the hint's words; `None` exactly when
    /// [`ArgMode::None`].
    pub arg_hint: Option<&'static str>,
    /// Where it is offered.
    pub place: Place,
    /// What must hold first.
    pub needs: &'static [Need],
}

/// Every command, in resolution order (rule 1).
///
/// The chat set first, then `/help`. `/me` precedes `/shrug` so that `/m` is
/// the action — the frequent act earns the short spelling.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "me",
        aliases: &["emote"],
        description: DESC_ME,
        args: ArgMode::Required,
        arg_hint: Some("what you do"),
        place: Place::Chat,
        needs: &[Need::NoReply],
    },
    CommandSpec {
        name: "shrug",
        aliases: &[],
        description: DESC_SHRUG,
        args: ArgMode::OptionalRest,
        arg_hint: Some("text"),
        place: Place::Chat,
        needs: &[],
    },
    CommandSpec {
        name: "plain",
        aliases: &[],
        description: DESC_PLAIN,
        args: ArgMode::Required,
        arg_hint: Some("text"),
        place: Place::Chat,
        needs: &[],
    },
    CommandSpec {
        name: "topic",
        aliases: &[],
        description: DESC_TOPIC,
        args: ArgMode::Required,
        arg_hint: Some("the new topic"),
        place: Place::Chat,
        needs: &[Need::SetTopic],
    },
    CommandSpec {
        name: "invite",
        aliases: &[],
        description: DESC_INVITE,
        args: ArgMode::Required,
        arg_hint: Some("@name:server"),
        place: Place::Chat,
        needs: &[Need::Invite],
    },
    CommandSpec {
        name: "leave",
        aliases: &["part"],
        description: DESC_LEAVE,
        args: ArgMode::None,
        arg_hint: None,
        place: Place::Chat,
        needs: &[],
    },
    CommandSpec {
        name: "help",
        aliases: &["h", "commands"],
        description: DESC_HELP,
        args: ArgMode::None,
        arg_hint: None,
        place: Place::Everywhere,
        needs: &[],
    },
];

/// What the composer's room allows right now — read by `Accounts` from the
/// room itself, never told by the webview (the reply excepted: that is the
/// composer's own state).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// Which room this is.
    pub room: RoomKind,
    /// Whether the person's power level allows changing the topic.
    pub may_set_topic: bool,
    /// Whether the person's power level allows inviting.
    pub may_invite: bool,
    /// Whether a reply is pending in the composer.
    pub replying: bool,
}

/// Whether a command may run right now, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// It can run.
    Available,
    /// It cannot, because this does not hold.
    Unavailable(Need),
}

impl Availability {
    /// The sentence saying why it cannot run, or `None` when it can.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Availability::Available => None,
            Availability::Unavailable(need) => Some(need.sentence()),
        }
    }
}

/// Why a draft is refused — the decision. The sentence the person reads
/// travels beside it in [`Resolution::Refusal`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    /// One line, one leading slash, past the bound (rule 5).
    TooLong,
    /// A real command this room does not offer (rule 2).
    NotHere {
        /// The registry name.
        name: &'static str,
    },
    /// An offered command whose precondition does not hold right now.
    Unavailable {
        /// The registry name.
        name: &'static str,
        /// What does not hold.
        need: Need,
    },
    /// Something after a command that takes nothing.
    TakesNothing {
        /// The registry name.
        name: &'static str,
    },
    /// Nothing after a command that needs an argument.
    NeedsArgument {
        /// The registry name.
        name: &'static str,
    },
    /// `/invite` with something that is not a Matrix id.
    NotAMatrixId,
    /// A name nothing offered here answers to (rule 3).
    Unknown {
        /// The offered command it is nearest to, if any is near.
        nearest: Option<&'static str>,
    },
}

/// What one draft is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Send this as an ordinary text message — prose, the escaped slash,
    /// `/plain` and `/shrug`.
    Text(String),
    /// Send this as an action (`m.emote`) — `/me`.
    Emote(String),
    /// Run this command: an act on the room, not a message.
    Command {
        /// The registry name, never the alias typed.
        name: &'static str,
        /// What followed the name, trimmed; `None` when nothing did.
        args: Option<String>,
    },
    /// Send nothing, and say `message`; `reason` is why.
    Refusal {
        /// Why the draft is refused.
        reason: RefusalReason,
        /// What the person reads.
        message: String,
    },
}

/// Whether a command is offered in this room at all (rule 2).
pub fn offered(spec: &CommandSpec, ctx: &Context) -> bool {
    match spec.place {
        Place::Everywhere => true,
        Place::Chat => ctx.room == RoomKind::Chat,
    }
}

/// Why a command cannot run right now, checked in declaration order.
pub fn availability(spec: &CommandSpec, ctx: &Context) -> Availability {
    for need in spec.needs {
        let holds = match need {
            Need::SetTopic => ctx.may_set_topic,
            Need::Invite => ctx.may_invite,
            Need::NoReply => !ctx.replying,
        };
        if !holds {
            return Availability::Unavailable(*need);
        }
    }
    Availability::Available
}

/// The rows to show for a draft, in registry order: empty exactly when the
/// draft is not a command line or nothing offered here matches.
pub fn menu_in<'a>(
    registry: &'a [CommandSpec],
    draft: &str,
    ctx: &Context,
) -> Vec<&'a CommandSpec> {
    let Some(token) = command_token(draft) else {
        return Vec::new();
    };
    let token = token.to_lowercase();
    // A name typed in full is the whole menu: the rows that merely share its
    // prefix are noise once Enter is about to run this one.
    if let Some(spec) = exact_in(registry, &token).filter(|spec| offered(spec, ctx)) {
        return vec![spec];
    }
    registry
        .iter()
        .filter(|spec| offered(spec, ctx) && matches_prefix(spec, &token))
        .collect()
}

/// [`menu_in`] over the shipped registry.
pub fn menu(draft: &str, ctx: &Context) -> Vec<&'static CommandSpec> {
    menu_in(COMMANDS, draft, ctx)
}

/// What one draft is, against a registry.
pub fn resolve_in(registry: &[CommandSpec], draft: &str, ctx: &Context) -> Resolution {
    let Some(token) = command_token(draft) else {
        // Not a command line. One line can still begin with a slash here: the
        // escape (unwrapped here and nowhere else, at any length), or a command
        // line past the bound — which is refused, because sending it would turn
        // `/topic …` into a message about a topic (rule 5).
        if !draft.contains('\n') {
            if let Some(rest) = draft.strip_prefix("//") {
                return Resolution::Text(format!("/{rest}"));
            }
            if draft.starts_with('/') {
                return Resolution::Refusal {
                    reason: RefusalReason::TooLong,
                    message: TOO_LONG.to_string(),
                };
            }
        }
        return Resolution::Text(draft.to_string());
    };
    if token.is_empty() {
        // A bare slash is one keystroke of a command, not a refusal.
        return Resolution::Text(draft.to_string());
    }
    let lowered = token.to_lowercase();
    let args = draft[1 + token.len()..].trim();
    // An exact spelling resolves even where it is not offered, so the person
    // hears "not here" rather than a guess; a prefix only reaches what is.
    let spec = match exact_in(registry, &lowered) {
        Some(spec) if !offered(spec, ctx) => {
            return Resolution::Refusal {
                reason: RefusalReason::NotHere { name: spec.name },
                message: format!("/{} — {NOT_HERE}", spec.name),
            };
        }
        Some(spec) => spec,
        None => match registry
            .iter()
            .find(|spec| offered(spec, ctx) && matches_prefix(spec, &lowered))
        {
            Some(spec) => spec,
            None => return unknown_refusal(registry, &lowered, ctx),
        },
    };
    if let Availability::Unavailable(need) = availability(spec, ctx) {
        return Resolution::Refusal {
            reason: RefusalReason::Unavailable {
                name: spec.name,
                need,
            },
            message: format!("/{} — {}", spec.name, need.sentence()),
        };
    }
    match (spec.args, args.is_empty()) {
        (ArgMode::None, false) => Resolution::Refusal {
            reason: RefusalReason::TakesNothing { name: spec.name },
            message: format!(
                "/{name} takes nothing after it, so nothing was sent. Send /{name} on its own.",
                name = spec.name
            ),
        },
        (ArgMode::Required, true) => Resolution::Refusal {
            reason: RefusalReason::NeedsArgument { name: spec.name },
            message: format!(
                "/{name} needs {hint}, so nothing was sent. Type /{name} followed by it.",
                name = spec.name,
                hint = spec.arg_hint.unwrap_or("an argument")
            ),
        },
        (_, empty) => act(spec, if empty { None } else { Some(args) }),
    }
}

/// [`resolve_in`] over the shipped registry.
pub fn resolve(draft: &str, ctx: &Context) -> Resolution {
    resolve_in(COMMANDS, draft, ctx)
}

/// What an available command with well-formed arguments becomes.
fn act(spec: &CommandSpec, args: Option<&str>) -> Resolution {
    match (spec.name, args) {
        ("me", Some(text)) => Resolution::Emote(text.to_string()),
        ("plain", Some(text)) => Resolution::Text(text.to_string()),
        ("shrug", Some(text)) => Resolution::Text(format!("{SHRUG} {text}")),
        ("shrug", None) => Resolution::Text(SHRUG.to_string()),
        ("invite", Some(user)) if UserId::parse(user).is_err() => Resolution::Refusal {
            reason: RefusalReason::NotAMatrixId,
            message: format!(
                "/invite needs a Matrix id like @name:server, and \"{user}\" is not one. Nothing \
                 was sent."
            ),
        },
        (name, args) => Resolution::Command {
            name,
            args: args.map(str::to_string),
        },
    }
}

/// The refusal for a name nothing offered here answers to (rule 3).
fn unknown_refusal(registry: &[CommandSpec], token: &str, ctx: &Context) -> Resolution {
    let nearest = nearest_in(registry, token, ctx);
    let message = match nearest {
        Some(spec) => format!(
            "keeper has no /{token} command. The closest is /{name} — {description} Nothing was \
             sent. {ESCAPE_HINT}",
            name = spec.name,
            description = spec.description
        ),
        None => format!("{UNKNOWN_NO_NEAR} {ESCAPE_HINT}"),
    };
    Resolution::Refusal {
        reason: RefusalReason::Unknown {
            nearest: nearest.map(|spec| spec.name),
        },
        message,
    }
}

/// The offered command whose name or alias is closest to `token`, within
/// [`NEAREST_MAX_DISTANCE`] edits; ties go to the earlier entry.
pub fn nearest_in<'a>(
    registry: &'a [CommandSpec],
    token: &str,
    ctx: &Context,
) -> Option<&'a CommandSpec> {
    registry
        .iter()
        .filter(|spec| offered(spec, ctx))
        .filter_map(|spec| {
            std::iter::once(spec.name)
                .chain(spec.aliases.iter().copied())
                .map(|spelling| distance(spelling, token))
                .min()
                .filter(|best| *best <= NEAREST_MAX_DISTANCE)
                .map(|best| (best, spec))
        })
        .min_by_key(|(best, _)| *best)
        .map(|(_, spec)| spec)
}

/// The command whose name, then alias, is exactly `token` (lowercased).
fn exact_in<'a>(registry: &'a [CommandSpec], token: &str) -> Option<&'a CommandSpec> {
    registry
        .iter()
        .find(|spec| spec.name == token)
        .or_else(|| registry.iter().find(|spec| spec.aliases.contains(&token)))
}

/// Whether a name or an alias begins with `token`.
fn matches_prefix(spec: &CommandSpec, token: &str) -> bool {
    spec.name.starts_with(token) || spec.aliases.iter().any(|alias| alias.starts_with(token))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The refusal quotes the hint, so a `Required` command without one would
    /// teach nothing, and a `None` command with one would promise an argument
    /// it refuses.
    #[test]
    fn commands_hints_match_their_argument_modes() {
        for spec in COMMANDS {
            match spec.args {
                ArgMode::None => assert!(spec.arg_hint.is_none(), "/{} has a hint", spec.name),
                _ => assert!(spec.arg_hint.is_some(), "/{} has no hint", spec.name),
            }
        }
    }

    /// Names and aliases share one namespace.
    #[test]
    fn commands_spellings_are_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for spec in COMMANDS {
            for spelling in std::iter::once(spec.name).chain(spec.aliases.iter().copied()) {
                assert!(!seen.contains(&spelling), "{spelling} is registered twice");
                seen.push(spelling);
            }
        }
    }
}
