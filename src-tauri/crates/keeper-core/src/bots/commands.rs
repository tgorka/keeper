//! Slash commands — the registry, and the refusal (Story 61.9, FR-385).
//!
//! **A slash command is keeper's act, never the model's.** Everything in this
//! module decides one thing: given what somebody typed into the composer, is it
//! prose for the model, a command for keeper, or a refusal? Those three are the
//! whole of [`Resolution`], and the third is the reason this file exists — a
//! composer that shrugged and posted `/gran` to the endpoint as prose would
//! have turned a typo into a paid request and an answer about nothing.
//!
//! # The five rules
//!
//! 1. **Exact beats prefix, and an alias is exact.** `/h` is `/help` because
//!    `h` is a registered alias, even though `/history` is the first entry a
//!    prefix search would reach. Telegram's own resolution is an ordered
//!    fallback list rather than a merge (R5 §1.1) and Claude Code highlights a
//!    full name over a partial one (R5 §2); both land here as: name, then
//!    alias, then prefix.
//! 2. **Ambiguity resolves to the first registry match**, so the order of
//!    [`COMMANDS`] is a decision and not an accident. `/m` is `/model` because
//!    `model` is listed before `metadata`.
//! 3. **An unknown command is a refusal that names the nearest match**, and it
//!    is *not* sent to the model. Claude Code's deliberate refusal to guess —
//!    "`Enter` submits your text as typed and reports Unknown command" (R5
//!    §2) — is the behaviour worth copying, minus the part where the text still
//!    goes out.
//! 4. **A literal leading slash is escaped by doubling it.** `//etc` sends
//!    `/etc`, because a model may legitimately be asked about `/etc` and no
//!    surveyed client documents an escape at all (R5 §2, "Escaping a literal
//!    leading slash is documented nowhere"). [`ESCAPE_HINT`] is the one
//!    sentence that teaches it, quoted by the empty state and by `/help`.
//! 5. **A command that cannot work here is absent, not disabled** (AD-27) —
//!    and a command whose *precondition* is merely unmet right now is listed
//!    with the reason, because "choose a bot first" is a fact about this
//!    moment and not about this build. [`offered`] draws that line;
//!    [`availability`] fills in the reason.
//!
//! # Why the registry is here and not in the composer
//!
//! One list feeds the picker rows, the refusal sentence, the argument hint and
//! the availability reason. Two lists — a Rust registry and a TypeScript
//! matcher beside it — would be two opinions about what `/mod` means, and the
//! one a person is looking at is always the other one. So the *decision* is
//! this module and the composer is a renderer: it asks
//! [`crate::vm::BotCommandPreviewVm`] what the draft is and obeys, exactly as
//! the palette's frontend "only renders and dispatches by id, never filters or
//! re-orders" (AD-20).
//!
//! # What is deliberately not here
//!
//! **Hermes' own commands are not proxied.** They are enumerable only over the
//! TUI-gateway websocket RPC `commands.catalog`, which keeper does not speak
//! (research §14), so this registry never lists one and
//! [`HERMES_COMMANDS_NOT_PROXIED`] says so where a Hermes bot is selected. A
//! picker that showed `/compact` and silently dropped it would be the
//! affordance-that-lies AD-27 forbids.
//!
//! **`/image` is not shipped by this story.** Story 61.12 owns the paste path,
//! and a command that could not attach anything is rule 5's own bad example.
//! Pasting is the gesture that surface offers; there is nothing for a command
//! to add.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::ProviderKind;

// ---------------------------------------------------------------------------
// Description copy. One `const` sentence per command, stored once, because the
// picker row, `/help` and the refusal all quote the same words — the mistake
// `layout/panel-strip.tsx:20-25` already records for the absent drive.
// Sentence case, no exclamation, no "please" (UX-DR10).
// ---------------------------------------------------------------------------

/// `/new`
pub const DESC_NEW: &str = "Start a new conversation with the bot you have chosen.";
/// `/bot`
pub const DESC_BOT: &str = "Switch to another bot by name.";
/// `/model`
pub const DESC_MODEL: &str = "Send the rest of this conversation to another model.";
/// `/metadata`
pub const DESC_METADATA: &str = "Show or hide the model, tokens and timing under each answer.";
/// `/grant`
pub const DESC_GRANT: &str = "Review what this bot is allowed to read and write on the drive.";
/// `/history`
pub const DESC_HISTORY: &str = "List your conversations, or search them by a word.";
/// `/help`
pub const DESC_HELP: &str = "List every command keeper knows.";

// ---------------------------------------------------------------------------
// Refusal and disclosure copy. Each names what did not happen and the one act
// that would change it — the shape `session-actions.tsx:54-55` uses.
// ---------------------------------------------------------------------------

/// How to send a message that really does begin with a slash.
///
/// The one sentence the empty state and `/help` both teach, because rule 4 is
/// the only part of this surface a person cannot guess from the menu.
pub const ESCAPE_HINT: &str =
    "To send a message that starts with a slash, double it: //etc sends /etc as text.";

/// Nothing in the registry is close to what was typed.
pub const UNKNOWN_NO_NEAR: &str =
    "keeper has no command by that name, and nothing in the list is close to it. Nothing was \
     sent. Type /help for the list.";

/// Why Hermes' own commands never appear in this menu (research §14).
pub const HERMES_COMMANDS_NOT_PROXIED: &str =
    "Hermes' own commands stay on Hermes: they are listed only over a channel keeper does not \
     speak, so this menu holds keeper's commands and never Hermes'.";

/// Why a Hermes bot has no grant to give (epic's 2026-09-02 correction).
pub const HERMES_TOOLS_SERVER_SIDE: &str =
    "Hermes runs its own tools on its own host under its own permissions, so there is no keeper \
     grant to give it. Nothing was sent.";

/// No provider is configured at all.
pub const NEED_PROVIDER: &str =
    "No provider is configured, so this has nothing to act on. Add one in Settings → Bots.";

/// A provider exists but no bot is chosen.
pub const NEED_BOT: &str = "No bot is chosen, so this has nothing to act on. Choose one above.";

/// The command acts on a conversation and there is none open yet.
pub const NEED_SESSION: &str =
    "This conversation has not started yet, so this has nothing to act on. Send a message first.";

/// The endpoint did not say whether the chosen model takes tools.
///
/// A **warning carried by an available command**, not a refusal: an unread
/// capability is `unknown` and never `false` (AD-27), and
/// [`grant::grant_offer`](crate::bots::grant::grant_offer) reaches the same
/// verdict for the pane's grant bar — it offers the control and warns. The two
/// layers must not disagree about whether a person may set a grant right now,
/// and refusing here would also strand every Ollama older than the
/// `capabilities` array with no route to the feature at all.
pub const GRANT_TOOLS_UNKNOWN: &str =
    "keeper could not read whether this model takes tools, so a grant here may reach nothing. \
     Probe the model in Settings → Bots.";

/// The longest draft this module will reason about as a possible command.
///
/// A bound rather than a taste: the draft is tokenized on every keystroke, and
/// a pasted megabyte beginning with a slash would be scanned on each one. A
/// command line is one short line by construction, so anything longer is prose
/// and is treated as prose without being measured twice.
pub const MAX_COMMAND_CHARS: usize = 512;

/// The furthest a typo may be from a command name and still be named as the
/// nearest match.
///
/// Three edits is `histroy → history` and `metdata → metadata`; it is not
/// `hello → help`. Beyond it the honest answer is [`UNKNOWN_NO_NEAR`] — naming
/// a "closest" command that shares nothing with what was typed is a guess
/// dressed as help.
const NEAREST_MAX_DISTANCE: usize = 3;

/// What a command does with the text after its name (Story 61.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ArgMode {
    /// Nothing may follow. Typed on its own, the way Open WebUI's `/compact`
    /// and `/status` are (R5 §2).
    None,
    /// One argument is required, and its absence is a refusal naming the hint
    /// rather than a command that quietly did nothing.
    Required,
    /// Everything after the name is one optional argument, verbatim.
    OptionalRest,
}

/// What must exist before a command can act (Story 61.9).
///
/// Not a wire type: it never leaves this crate, because the surface reads the
/// *reason* a command cannot act and never the name of the missing thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandNeed {
    /// At least one configured provider.
    Provider,
    /// A chosen bot.
    Bot,
    /// A conversation that has started.
    Session,
}

/// One command in the registry (Story 61.9, FR-385).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandSpec {
    /// The name after the slash, lowercase.
    pub name: &'static str,
    /// Other exact spellings that mean this command. Matched before any
    /// prefix, which is rule 1.
    pub aliases: &'static [&'static str],
    /// The one sentence the picker and `/help` both show.
    pub description: &'static str,
    /// What may follow the name.
    pub args: ArgMode,
    /// What the argument is, in the words the hint shows — Claude Code's
    /// `argument-hint` (R5 §1.2). `None` exactly when [`ArgMode::None`].
    pub arg_hint: Option<&'static str>,
    /// What must exist first.
    pub needs: &'static [CommandNeed],
    /// Whether this command exists only where keeper itself would run the
    /// tools: an Ollama bot whose model advertises them.
    pub ollama_tools_only: bool,
}

/// Every command keeper knows, in resolution order (rule 2).
///
/// `model` precedes `metadata` so that `/m` is the model picker: switching
/// model is the frequent act and toggling a caption is the rare one, and the
/// frequent act is the one that earns the short spelling. `history` precedes
/// `help` so that `/h` has to be an alias to reach `/help`, which is rule 1
/// made load-bearing rather than incidental.
pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "new",
        aliases: &["clear", "reset"],
        description: DESC_NEW,
        args: ArgMode::None,
        arg_hint: None,
        needs: &[CommandNeed::Provider, CommandNeed::Bot],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "bot",
        aliases: &["bots"],
        description: DESC_BOT,
        args: ArgMode::Required,
        arg_hint: Some("a bot name"),
        needs: &[CommandNeed::Provider],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "model",
        aliases: &["m"],
        description: DESC_MODEL,
        args: ArgMode::Required,
        arg_hint: Some("a model to send to"),
        needs: &[CommandNeed::Provider, CommandNeed::Bot],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "metadata",
        aliases: &["meta"],
        description: DESC_METADATA,
        args: ArgMode::OptionalRest,
        arg_hint: Some("on or off"),
        needs: &[],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "grant",
        aliases: &["grants"],
        description: DESC_GRANT,
        args: ArgMode::None,
        arg_hint: None,
        needs: &[CommandNeed::Provider, CommandNeed::Bot],
        ollama_tools_only: true,
    },
    CommandSpec {
        name: "history",
        aliases: &["sessions"],
        description: DESC_HISTORY,
        args: ArgMode::OptionalRest,
        arg_hint: Some("a word to search for"),
        needs: &[CommandNeed::Provider],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "help",
        aliases: &["h", "commands"],
        description: DESC_HELP,
        args: ArgMode::None,
        arg_hint: None,
        needs: &[],
        ollama_tools_only: false,
    },
];

/// What the composer knows about right now (Story 61.9).
///
/// Four facts, all of them things the pane already holds: a chosen bot's
/// provider kind, whether a bot is chosen at all, whether the conversation has
/// started, and the tool capability of the chosen model as the endpoint stated
/// it — a tri-state, because an unread capability is `unknown` and never
/// `false` (AD-27).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    /// The chosen bot's provider kind, or `None` when no bot is chosen.
    pub kind: Option<ProviderKind>,
    /// Whether any provider is configured.
    pub has_provider: bool,
    /// Whether a bot is chosen.
    pub has_bot: bool,
    /// Whether the open conversation has started.
    pub has_session: bool,
    /// Whether the chosen model takes tools: `Some(true)` the endpoint said
    /// yes, `Some(false)` it said no, `None` it did not say.
    pub model_tools: Option<bool>,
}

/// Whether a command may act right now, and why not (Story 61.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// It can run.
    Available,
    /// It can run, and the surface owes the person this sentence first.
    ///
    /// The distinction earns its variant: an unread tool capability must not
    /// stop somebody setting a grant (`grant::grant_offer` offers the same
    /// control and warns), but a menu that said nothing would let them expect
    /// a tool call that may never come.
    AvailableWithWarning(&'static str),
    /// It cannot, for this stated reason.
    Unavailable(&'static str),
}

impl Availability {
    /// The reason it cannot run, or `None` when it can.
    ///
    /// A warning is not a reason: [`Self::AvailableWithWarning`] answers `None`
    /// here so no caller can turn a caveat into a refusal by accident.
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Availability::Available | Availability::AvailableWithWarning(_) => None,
            Availability::Unavailable(reason) => Some(reason),
        }
    }

    /// The sentence the surface owes before the command runs, if any.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            Availability::AvailableWithWarning(warning) => Some(warning),
            Availability::Available | Availability::Unavailable(_) => None,
        }
    }

    /// Whether the command may run at all.
    pub fn is_available(self) -> bool {
        !matches!(self, Availability::Unavailable(_))
    }
}

/// What one draft is (Story 61.9, FR-385).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Send this to the model, verbatim. Covers ordinary prose and the escaped
    /// leading slash, which is the same answer for the same reason: the model
    /// is being asked about text.
    Prose(String),
    /// Run this command. `args` is `None` when nothing followed the name, and
    /// is trimmed but otherwise verbatim — a search term keeps its spaces.
    Command {
        /// The registry name, never the alias that was typed: one spelling
        /// reaches the handler.
        name: &'static str,
        /// What followed the name.
        args: Option<String>,
    },
    /// Send nothing, and say this.
    Refusal(String),
}

/// Whether a command exists at all in this context (rule 5, AD-27).
///
/// The line between this and [`availability`] is the line between "this build
/// cannot do that here" and "not yet, and here is what to do". `/grant` on a
/// Hermes bot is the first: Hermes executes its own tools, so no keeper grant
/// could ever be used and the row must be gone rather than greyed. `/new` with
/// no bot chosen is the second.
pub fn offered(spec: &CommandSpec, ctx: &Context) -> bool {
    if !spec.ollama_tools_only {
        return true;
    }
    match ctx.kind {
        // No bot chosen: the command exists, and `availability` says a bot is
        // needed. Hiding it here would make the menu shrink as somebody picks
        // a bot, which reads as a bug rather than as a rule.
        None => true,
        Some(ProviderKind::Hermes) => false,
        // The endpoint refused the capability, so a grant could never be used.
        // `None` is *unknown* and stays listed with `GRANT_TOOLS_UNKNOWN`.
        Some(ProviderKind::Ollama) => ctx.model_tools != Some(false),
    }
}

/// Why a command cannot act right now (Story 61.9).
///
/// Needs are checked in declaration order, so the reason a person is given is
/// the outermost missing thing: "add a provider" before "choose a bot", never
/// the other way round.
pub fn availability(spec: &CommandSpec, ctx: &Context) -> Availability {
    for need in spec.needs {
        let missing = match need {
            CommandNeed::Provider => (!ctx.has_provider).then_some(NEED_PROVIDER),
            CommandNeed::Bot => (!ctx.has_bot).then_some(NEED_BOT),
            CommandNeed::Session => (!ctx.has_session).then_some(NEED_SESSION),
        };
        if let Some(reason) = missing {
            return Availability::Unavailable(reason);
        }
    }
    if spec.ollama_tools_only && ctx.model_tools.is_none() {
        return Availability::AvailableWithWarning(GRANT_TOOLS_UNKNOWN);
    }
    Availability::Available
}

/// The one sentence a Hermes bot's pane owes about commands (research §14).
///
/// `None` for Ollama and for no bot at all, because there is nothing to
/// disclose: the menu is the whole truth there.
pub fn note(ctx: &Context) -> Option<&'static str> {
    match ctx.kind {
        Some(ProviderKind::Hermes) => Some(HERMES_COMMANDS_NOT_PROXIED),
        _ => None,
    }
}

/// The token typed after a leading slash, lowercased, or `None` when the draft
/// is not a command line at all (Story 61.9).
///
/// A command line is **one line, one slash, nothing before it** — the note
/// editor's `slash-menu.ts:22` narrowness restated for a textarea, and Open
/// WebUI's "typed on their own" (R5 §2). A doubled slash is the escape and is
/// therefore not a command line, which is why this returns `None` for it
/// rather than a token of `"/etc"`.
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

/// The commands to show for a draft, in registry order (Story 61.9).
///
/// Empty exactly when the draft is not a command line or nothing matches, so
/// the caller never has to ask a second question to decide whether to draw a
/// menu. Every row is [`offered`]; unavailable rows are included, because the
/// reason is the useful part.
pub fn menu_in<'a>(
    registry: &'a [CommandSpec],
    draft: &str,
    ctx: &Context,
) -> Vec<&'a CommandSpec> {
    let Some(token) = command_token(draft) else {
        return Vec::new();
    };
    let token = token.to_lowercase();
    // An exact hit is the whole menu: once a name is typed in full, the rows
    // that merely share its prefix are noise, and Enter is about to run this
    // one anyway.
    if let Some(spec) = exact_in(registry, &token) {
        if offered(spec, ctx) {
            return vec![spec];
        }
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

/// What one draft is, resolved against a registry (Story 61.9, FR-385).
///
/// Taking the registry as an argument is the note editor's own shape
/// (`slash-menu.ts:98-100` defaults its list the same way): the rules are
/// testable against a two-command registry that makes one nesting explicit,
/// rather than against whichever shipped names happen to nest today.
pub fn resolve_in(registry: &[CommandSpec], draft: &str, ctx: &Context) -> Resolution {
    let Some(token) = command_token(draft) else {
        // Not a command line. The escape is unwrapped here and nowhere else:
        // one leading slash is removed, everything after it is untouched.
        let text = match draft.strip_prefix("//") {
            Some(rest) if !draft.contains('\n') && draft.len() <= MAX_COMMAND_CHARS => {
                format!("/{rest}")
            }
            _ => draft.to_string(),
        };
        return Resolution::Prose(text);
    };
    if token.is_empty() {
        // A bare slash is not yet a command and is not a refusal either — it is
        // what somebody has typed one character of.
        return Resolution::Prose(draft.to_string());
    }
    let lowered = token.to_lowercase();
    let args = draft[1 + token.len()..].trim();
    let Some(spec) = exact_in(registry, &lowered).or_else(|| prefix_in(registry, &lowered)) else {
        return Resolution::Refusal(unknown_refusal(registry, &lowered));
    };
    if !offered(spec, ctx) {
        // The name is real and this is not the place it works. The one context
        // where that happens today is a Hermes bot and a grant, so the sentence
        // is that fact rather than a generic "unavailable".
        return Resolution::Refusal(HERMES_TOOLS_SERVER_SIDE.to_string());
    }
    if let Availability::Unavailable(reason) = availability(spec, ctx) {
        return Resolution::Refusal(format!("/{} — {reason}", spec.name));
    }
    match (spec.args, args.is_empty()) {
        (ArgMode::None, false) => Resolution::Refusal(format!(
            "/{name} takes nothing after it, so nothing was sent. Send /{name} on its own.",
            name = spec.name
        )),
        (ArgMode::Required, true) => Resolution::Refusal(format!(
            "/{name} needs {hint}, so nothing was sent. Type /{name} followed by it.",
            name = spec.name,
            // Every `Required` spec carries a hint; the fallback keeps the
            // sentence whole rather than panicking on a registry mistake.
            hint = spec.arg_hint.unwrap_or("an argument")
        )),
        (_, empty) => Resolution::Command {
            name: spec.name,
            args: if empty { None } else { Some(args.to_string()) },
        },
    }
}

/// [`resolve_in`] over the shipped registry.
pub fn resolve(draft: &str, ctx: &Context) -> Resolution {
    resolve_in(COMMANDS, draft, ctx)
}

/// The refusal for a name no command answers to (rule 3).
fn unknown_refusal(registry: &[CommandSpec], token: &str) -> String {
    match nearest_in(registry, token) {
        Some(spec) => format!(
            "keeper has no /{token} command. The closest is /{name} — {description} Nothing was \
             sent. {ESCAPE_HINT}",
            name = spec.name,
            description = spec.description
        ),
        None => format!("{UNKNOWN_NO_NEAR} {ESCAPE_HINT}"),
    }
}

/// The command whose name or alias is exactly `token` (already lowercased).
fn exact_in<'a>(registry: &'a [CommandSpec], token: &str) -> Option<&'a CommandSpec> {
    registry
        .iter()
        .find(|spec| spec.name == token)
        .or_else(|| registry.iter().find(|spec| spec.aliases.contains(&token)))
}

/// The first command whose name or an alias begins with `token`.
fn prefix_in<'a>(registry: &'a [CommandSpec], token: &str) -> Option<&'a CommandSpec> {
    registry.iter().find(|spec| matches_prefix(spec, token))
}

/// Whether a name or an alias begins with `token`.
fn matches_prefix(spec: &CommandSpec, token: &str) -> bool {
    spec.name.starts_with(token) || spec.aliases.iter().any(|alias| alias.starts_with(token))
}

/// The registered spelling closest to `token`, or `None` when nothing is
/// within [`NEAREST_MAX_DISTANCE`] edits of it.
///
/// Aliases are searched too — `/hep` should reach `/help` through `help`, and
/// `/session` should reach `/history` through `sessions` — but the *name* is
/// what the refusal prints, because that is the spelling the handler takes.
pub fn nearest_in<'a>(registry: &'a [CommandSpec], token: &str) -> Option<&'a CommandSpec> {
    registry
        .iter()
        .filter_map(|spec| {
            std::iter::once(spec.name)
                .chain(spec.aliases.iter().copied())
                .map(|spelling| distance(spelling, token))
                .min()
                .filter(|best| *best <= NEAREST_MAX_DISTANCE)
                .map(|best| (best, spec))
        })
        // `min_by_key` keeps the first of equal keys, so a tie resolves to the
        // earlier registry entry — rule 2, applied to the guess as well.
        .min_by_key(|(best, _)| *best)
        .map(|(_, spec)| spec)
}

/// [`nearest_in`] over the shipped registry.
pub fn nearest(token: &str) -> Option<&'static CommandSpec> {
    nearest_in(COMMANDS, token)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Required` command owes an argument hint, and every `None`
    /// command owes the absence of one — the refusal sentence quotes the hint,
    /// so a missing one would print "needs an argument" and teach nothing.
    #[test]
    fn commands_hints_match_their_argument_modes() {
        for spec in COMMANDS {
            match spec.args {
                ArgMode::None => assert!(spec.arg_hint.is_none(), "/{} has a hint", spec.name),
                _ => assert!(spec.arg_hint.is_some(), "/{} has no hint", spec.name),
            }
        }
    }

    /// Names and aliases share one namespace, so a duplicate would make
    /// resolution depend on which loop ran first.
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
