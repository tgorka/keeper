//! Story 61.9 — the slash-command registry, its resolution and its refusals.
//!
//! Every assertion here is about a *decision*: which command a spelling means,
//! whether it may run, and what keeper says when it may not. The surface's own
//! obligations — that a refusal is shown and nothing is sent, that Escape
//! dismisses the menu — are proven in `bot-composer.test.tsx` against these
//! verdicts, because those are facts about a textarea and these are facts about
//! the rules.
//!
//! The registry is taken as an argument by [`resolve_in`], so the four ordering
//! rules are tested against a registry that makes the nesting explicit rather
//! than against whichever shipped names happen to nest this month. The shipped
//! list is then tested for the properties it must have.

use keeper_core::bots::commands::{
    availability, command_token, menu, menu_in, nearest_in, note, offered, resolve, resolve_in,
    ArgMode, Availability, CommandNeed, CommandSpec, Context, Resolution, COMMANDS, ESCAPE_HINT,
    GRANT_TOOLS_UNKNOWN, HERMES_COMMANDS_NOT_PROXIED, HERMES_TOOLS_SERVER_SIDE, NEED_BOT,
    NEED_PROVIDER,
};
use keeper_core::bots::ProviderKind;
use keeper_core::vm::{BotCommandContextReq, BotCommandPreviewVm, BotCommandVerdictVm};

/// A context in which every shipped command's preconditions are met.
fn ready() -> Context {
    Context {
        kind: Some(ProviderKind::Ollama),
        has_provider: true,
        has_bot: true,
        has_session: true,
        model_tools: Some(true),
    }
}

/// A two-command registry whose first entry's name *contains* the second's, so
/// "exact beats prefix" has something to beat. `news` is listed first, which is
/// what a prefix search over `new` would reach.
const NESTED: &[CommandSpec] = &[
    CommandSpec {
        name: "news",
        aliases: &["headlines"],
        description: "The wrong answer for /new.",
        args: ArgMode::None,
        arg_hint: None,
        needs: &[],
        ollama_tools_only: false,
    },
    CommandSpec {
        name: "new",
        aliases: &["blank"],
        description: "The right answer for /new.",
        args: ArgMode::None,
        arg_hint: None,
        needs: &[],
        ollama_tools_only: false,
    },
];

/// The command a resolution names, or a panic describing what it was instead.
fn commanded(resolution: Resolution) -> (&'static str, Option<String>) {
    match resolution {
        Resolution::Command { name, args } => (name, args),
        other => panic!("expected a command, got {other:?}"),
    }
}

/// The refusal sentence, or a panic describing what it was instead.
fn refused(resolution: Resolution) -> String {
    match resolution {
        Resolution::Refusal(message) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

/// The prose to send, or a panic describing what it was instead.
fn prose(resolution: Resolution) -> String {
    match resolution {
        Resolution::Prose(text) => text,
        other => panic!("expected prose, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Rule 1 — exact beats prefix, and an alias is exact.
// ---------------------------------------------------------------------------

/// Rule 1. `/new` is the command *named* `new`, even though `news` is listed
/// first and a prefix search reaches it first. This is the rule that stops the
/// menu's own ordering from deciding what a fully-typed name means.
#[test]
fn commands_exact_name_beats_an_earlier_prefix_match() {
    let (name, _) = commanded(resolve_in(NESTED, "/new", &ready()));
    assert_eq!(name, "new");
    // And the prefix path is genuinely live: one character less resolves the
    // other way, so the test above is not passing by accident.
    let (shorter, _) = commanded(resolve_in(NESTED, "/ne", &ready()));
    assert_eq!(shorter, "news");
}

/// Rule 1, second half. `/h` is `/help` because `h` is a registered alias,
/// while a prefix search over the shipped registry reaches `history` first —
/// which the second assertion pins, so the alias is doing the work.
#[test]
fn commands_exact_alias_beats_an_earlier_prefix_match() {
    let (name, _) = commanded(resolve("/h", &ready()));
    assert_eq!(name, "help");
    let (prefixed, _) = commanded(resolve("/hi", &ready()));
    assert_eq!(prefixed, "history");
}

/// Rule 2. Two commands share the prefix `m`; the earlier registry entry wins,
/// and `model` is earlier deliberately. `/m` is also a registered alias of
/// `model`, so this asserts the *prefix* case with a token that is not.
#[test]
fn commands_ambiguity_resolves_to_the_first_registry_match() {
    let (name, _) = commanded(resolve("/me hello", &ready()));
    assert_eq!(name, "metadata");
    let names: Vec<String> = menu("/m", &ready())
        .into_iter()
        .map(|spec| spec.name.to_string())
        .collect();
    assert_eq!(names, vec!["model".to_string()]);
}

/// Case is not part of a name. Somebody who types `/NEW` at the start of a
/// sentence means the command.
#[test]
fn commands_resolution_is_case_insensitive() {
    let (name, _) = commanded(resolve("/NeW", &ready()));
    assert_eq!(name, "new");
}

// ---------------------------------------------------------------------------
// Rule 3 — an unknown command is a refusal, and it names the nearest match.
// ---------------------------------------------------------------------------

/// Rule 3, and the whole reason this module exists: a typo does not become a
/// paid request. The refusal names what was typed, names the nearest command,
/// says nothing was sent, and teaches the escape.
#[test]
fn commands_unknown_name_refuses_and_names_the_nearest_match() {
    let message = refused(resolve("/histroy", &ready()));
    assert!(message.contains("/histroy"), "{message}");
    assert!(message.contains("/history"), "{message}");
    assert!(message.contains("Nothing was sent"), "{message}");
    assert!(message.contains(ESCAPE_HINT), "{message}");
}

/// The other half of rule 3, and the reason the nearest match has a bound: a
/// word that resembles nothing gets an honest "no command by that name"
/// instead of a guess dressed as help. `/etc/hosts` is the real case — a path,
/// typed unescaped.
#[test]
fn commands_unknown_name_with_no_near_match_names_none() {
    let message = refused(resolve("/etc/hosts", &ready()));
    assert!(message.contains("no command by that name"), "{message}");
    assert!(!message.contains("The closest is"), "{message}");
    assert!(message.contains(ESCAPE_HINT), "{message}");
}

/// A refusal is not prose, and this is the assertion that says so directly: no
/// arm of [`Resolution`] carries an unknown command onward, so there is no
/// shape in which `/histroy` reaches an endpoint.
#[test]
fn commands_unknown_name_is_never_prose() {
    assert!(matches!(
        resolve("/histroy", &ready()),
        Resolution::Refusal(_)
    ));
    let preview = BotCommandPreviewVm::compose("/histroy", &ready());
    assert!(matches!(
        preview.verdict,
        BotCommandVerdictVm::Refusal { .. }
    ));
}

/// The nearest match is searched over aliases too — `/session` is a misspelling
/// of the alias `sessions`, not of the name `history` — but the refusal prints
/// the *name*, because that is the spelling that reaches the handler.
#[test]
fn commands_nearest_match_searches_aliases_and_prints_the_name() {
    let spec = nearest_in(COMMANDS, "sessons").expect("a nearest match");
    assert_eq!(spec.name, "history");
}

// ---------------------------------------------------------------------------
// Rule 4 — the escape.
// ---------------------------------------------------------------------------

/// Rule 4. `//etc` sends `/etc`, because a model may legitimately be asked
/// about `/etc` and the doubled slash is the one thing about this surface a
/// person cannot guess from the menu.
#[test]
fn commands_doubled_slash_sends_one_slash_as_text() {
    assert_eq!(prose(resolve("//etc", &ready())), "/etc");
    assert_eq!(
        prose(resolve("//new is a command", &ready())),
        "/new is a command"
    );
    // Exactly one slash is removed: a path that really is `//server/share`
    // survives as `/server/share`, and a third slash survives untouched.
    assert_eq!(prose(resolve("///etc", &ready())), "//etc");
}

/// Ordinary prose is not touched, including prose with a slash inside it — the
/// note editor's own narrowness (`slash-menu.ts:4-5`: "a slash inside a path or
/// a fraction stays a slash").
#[test]
fn commands_prose_is_untouched() {
    assert_eq!(
        prose(resolve("what is in /etc/hosts?", &ready())),
        "what is in /etc/hosts?"
    );
    assert_eq!(prose(resolve("2/3 of it", &ready())), "2/3 of it");
}

/// A command must be its own line. A multi-line draft that starts with a slash
/// is a message, not a command, so a pasted diff cannot resolve to one.
#[test]
fn commands_a_multiline_draft_is_prose() {
    let draft = "/new\nand then some prose";
    assert_eq!(prose(resolve(draft, &ready())), draft);
    assert_eq!(command_token(draft), None);
    assert!(menu(draft, &ready()).is_empty());
}

/// A bare slash is what somebody has typed one character of, and the menu opens
/// on it — the note editor's `OPEN_SLASH` behaviour, which shows the whole list
/// before a letter is typed.
#[test]
fn commands_a_bare_slash_opens_the_whole_menu_and_sends_nothing_new() {
    assert_eq!(prose(resolve("/", &ready())), "/");
    assert_eq!(menu("/", &ready()).len(), COMMANDS.len());
}

// ---------------------------------------------------------------------------
// Rule 5 — availability, and the difference between absent and not-yet.
// ---------------------------------------------------------------------------

/// A command that needs a bot is *listed* with the reason when none is chosen,
/// and running it refuses with the same sentence. Listed rather than hidden
/// because "choose a bot" is a fact about this moment, and the menu that
/// vanishes as you configure the app reads as a bug.
#[test]
fn commands_needing_a_bot_are_unavailable_with_a_reason() {
    let ctx = Context {
        kind: None,
        has_provider: true,
        has_bot: false,
        has_session: false,
        model_tools: None,
    };
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == "new")
        .expect("/new is registered");
    assert_eq!(
        availability(spec, &ctx),
        Availability::Unavailable(NEED_BOT)
    );
    let message = refused(resolve("/new", &ctx));
    assert!(message.contains(NEED_BOT), "{message}");
    // Still offered, so the reason is reachable.
    assert!(offered(spec, &ctx));
    let row = menu("/new", &ctx)
        .first()
        .map(|spec| keeper_core::vm::BotCommandRowVm::compose(spec, &ctx))
        .expect("the row is listed");
    assert!(!row.available);
    assert_eq!(row.reason.as_deref(), Some(NEED_BOT));
}

/// Needs are reported outermost-first, so somebody with nothing configured is
/// told to add a provider rather than to choose a bot they cannot have.
#[test]
fn commands_report_the_outermost_missing_need_first() {
    let ctx = Context::default();
    let message = refused(resolve("/new", &ctx));
    assert!(message.contains(NEED_PROVIDER), "{message}");
}

/// A command with no needs works in an empty app: `/help` is how somebody finds
/// out what to do next, so gating it on a configured provider would be a
/// closed door with the instructions behind it.
#[test]
fn commands_help_works_with_nothing_configured() {
    let (name, args) = commanded(resolve("/help", &Context::default()));
    assert_eq!(name, "help");
    assert_eq!(args, None);
}

/// `ArgMode::None` means nothing may follow, and the refusal says which command
/// and what to do — Open WebUI's `/compact` rule (R5 §2), with a sentence.
#[test]
fn commands_taking_no_argument_refuse_a_trailing_one() {
    let message = refused(resolve("/new tomorrow", &ready()));
    assert!(message.contains("takes nothing after it"), "{message}");
    assert!(message.contains("/new"), "{message}");
}

/// `ArgMode::Required` with nothing after it is a refusal naming the hint, not
/// a command that quietly did nothing.
#[test]
fn commands_requiring_an_argument_refuse_without_one() {
    let message = refused(resolve("/bot", &ready()));
    assert!(message.contains("a bot name"), "{message}");
    let (name, args) = commanded(resolve("/bot  work hermes ", &ready()));
    assert_eq!(name, "bot");
    // Trimmed at the ends and verbatim inside: a bot named with two words keeps
    // its space.
    assert_eq!(args.as_deref(), Some("work hermes"));
}

/// `ArgMode::OptionalRest` accepts both, so `/history` lists and
/// `/history invoice` searches.
#[test]
fn commands_with_an_optional_argument_accept_both() {
    assert_eq!(commanded(resolve("/history", &ready())).1, None);
    assert_eq!(
        commanded(resolve("/history invoice", &ready()))
            .1
            .as_deref(),
        Some("invoice")
    );
}

// ---------------------------------------------------------------------------
// The two Hermes facts.
// ---------------------------------------------------------------------------

/// The grant affordance exists only where keeper would run the tools. On a
/// Hermes bot the row is **absent** rather than greyed (AD-27), and typing the
/// name says why in Hermes' own terms.
#[test]
fn commands_grant_is_absent_on_a_hermes_bot_and_says_why() {
    let ctx = Context {
        kind: Some(ProviderKind::Hermes),
        has_provider: true,
        has_bot: true,
        has_session: true,
        model_tools: Some(true),
    };
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == "grant")
        .expect("/grant is registered");
    assert!(!offered(spec, &ctx));
    assert!(menu("/grant", &ctx).is_empty());
    assert_eq!(refused(resolve("/grant", &ctx)), HERMES_TOOLS_SERVER_SIDE);
    // And it is present on the Ollama bot whose model advertises tools.
    assert!(offered(spec, &ready()));
    assert_eq!(commanded(resolve("/grant", &ready())).0, "grant");
}

/// A model whose tool support the endpoint *refused* loses the row: a grant
/// that could never be used is the affordance AD-27 forbids. A model the
/// endpoint said nothing about keeps the row **and stays runnable**, carrying
/// keeper's own caveat — unknown is not no, and `grant::grant_offer` offers the
/// pane's own grant control on exactly the same input. Two layers must not
/// disagree about whether a person may set a grant right now.
#[test]
fn commands_grant_distinguishes_a_refused_capability_from_an_unread_one() {
    let refused_tools = Context {
        model_tools: Some(false),
        ..ready()
    };
    let unread = Context {
        model_tools: None,
        ..ready()
    };
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == "grant")
        .expect("/grant is registered");
    assert!(!offered(spec, &refused_tools));
    assert!(offered(spec, &unread));
    assert_eq!(
        availability(spec, &unread),
        Availability::AvailableWithWarning(GRANT_TOOLS_UNKNOWN)
    );
    // A caveat is not a refusal: the command runs, and the surface prints the
    // sentence beside it.
    assert!(availability(spec, &unread).is_available());
    assert_eq!(availability(spec, &unread).reason(), None);
    assert_eq!(
        availability(spec, &unread).warning(),
        Some(GRANT_TOOLS_UNKNOWN)
    );
    assert_eq!(commanded(resolve("/grant", &unread)).0, "grant");
}

/// Hermes' own commands are not proxied, and the sentence appears exactly where
/// a Hermes bot is selected — not on Ollama, where there is nothing to
/// disclose, and not before a bot is chosen, where it would be a fact about
/// nobody.
#[test]
fn commands_the_hermes_sentence_appears_only_for_a_hermes_bot() {
    let hermes = Context {
        kind: Some(ProviderKind::Hermes),
        ..ready()
    };
    assert_eq!(note(&hermes), Some(HERMES_COMMANDS_NOT_PROXIED));
    assert_eq!(note(&ready()), None);
    assert_eq!(note(&Context::default()), None);
    assert_eq!(
        BotCommandPreviewVm::compose("/", &hermes).note.as_deref(),
        Some(HERMES_COMMANDS_NOT_PROXIED)
    );
    assert_eq!(BotCommandPreviewVm::compose("/", &ready()).note, None);
}

// ---------------------------------------------------------------------------
// The menu projection, and the wire shape the composer reads.
// ---------------------------------------------------------------------------

/// A fully-typed name is the whole menu: the rows that merely share its prefix
/// are noise once Enter is about to run this one.
#[test]
fn commands_menu_narrows_to_one_row_on_an_exact_name() {
    let rows = menu("/help", &ready());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "help");
}

/// A token nothing matches draws no menu, and the refusal is what the surface
/// shows instead — an empty list is the "draw nothing" signal, so the caller
/// never asks a second question.
#[test]
fn commands_menu_is_empty_for_an_unmatched_token() {
    assert!(menu("/zzz", &ready()).is_empty());
    assert!(menu("hello", &ready()).is_empty());
    assert!(menu("//etc", &ready()).is_empty());
}

/// The registry is a parameter, so `menu_in` filters the given list and nothing
/// else — the property that lets the ordering rules be tested against `NESTED`.
#[test]
fn commands_menu_in_uses_the_registry_it_is_given() {
    let rows = menu_in(NESTED, "/ne", &ready());
    let names: Vec<&str> = rows.iter().map(|spec| spec.name).collect();
    assert_eq!(names, vec!["news", "new"]);
}

/// The preview echoes the draft it answers, because replies can land out of
/// order and a refusal shown against text the field no longer holds is worse
/// than no refusal at all.
#[test]
fn commands_preview_echoes_its_draft_and_carries_the_escape_hint() {
    let preview = BotCommandPreviewVm::compose("/hel", &ready());
    assert_eq!(preview.draft, "/hel");
    assert_eq!(preview.escape_hint, ESCAPE_HINT);
    assert_eq!(preview.rows.len(), 1);
    assert_eq!(preview.rows[0].name, "help");
    assert_eq!(preview.rows[0].aliases, vec!["h", "commands"]);
}

/// The request the shell hands over describes exactly the context the registry
/// reasons about, so the shell makes no decision of its own (AD-55/AD-56).
#[test]
fn commands_context_request_maps_to_the_registry_context() {
    let req = BotCommandContextReq {
        provider_kind: Some(ProviderKind::Ollama),
        has_provider: true,
        has_bot: true,
        has_session: false,
        model_tools: Some(true),
    };
    let ctx = req.context();
    assert_eq!(ctx.kind, Some(ProviderKind::Ollama));
    assert!(ctx.has_bot);
    assert!(!ctx.has_session);
    assert_eq!(ctx.model_tools, Some(true));
}

/// Every need declared in the shipped registry is one the context can answer,
/// which is what stops a command from being permanently unavailable for a
/// reason nothing can satisfy.
#[test]
fn commands_every_declared_need_is_satisfiable() {
    for spec in COMMANDS {
        for need in spec.needs {
            assert!(
                matches!(
                    need,
                    CommandNeed::Provider | CommandNeed::Bot | CommandNeed::Session
                ),
                "/{} declares an unknown need",
                spec.name
            );
        }
        if offered(spec, &ready()) {
            assert_eq!(
                availability(spec, &ready()),
                Availability::Available,
                "/{} is never available",
                spec.name
            );
        }
    }
}
