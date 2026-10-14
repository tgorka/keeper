//! Story 100.5 — the messenger's slash commands: what a `/` line means in a
//! chat and in an agent room, and what keeper says when it means nothing.
//!
//! Every assertion is about a decision. That the composer shows a refusal and
//! sends nothing, that Escape dismisses the menu, that `/leave` asks first —
//! those are facts about a textarea, proven in `composer.test.tsx` against these
//! verdicts.

use keeper_core::bots::commands::MAX_COMMAND_CHARS;
use keeper_core::chat::commands::{
    availability, menu, resolve, Availability, Context, Need, RefusalReason, Resolution, RoomKind,
    COMMANDS, SHRUG,
};
use keeper_core::vm::{ChatCommandPreviewVm, ChatCommandVerdictVm};

/// A chat where the person may do everything.
fn chat() -> Context {
    Context {
        room: RoomKind::Chat,
        may_set_topic: true,
        may_invite: true,
        replying: false,
        agent_home_here: true,
    }
}

fn agent_dm() -> Context {
    Context {
        room: RoomKind::AgentMain,
        ..chat()
    }
}

fn agent_conversation() -> Context {
    Context {
        room: RoomKind::Agent,
        ..chat()
    }
}

fn commanded(resolution: Resolution) -> (&'static str, Option<String>) {
    match resolution {
        Resolution::Command { name, args } => (name, args),
        other => panic!("expected a command, got {other:?}"),
    }
}

/// Why a draft was refused — the decision, never the sentence that says it.
fn refused(resolution: Resolution) -> RefusalReason {
    match resolution {
        Resolution::Refusal { reason, .. } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn text(resolution: Resolution) -> String {
    match resolution {
        Resolution::Text(text) => text,
        other => panic!("expected text, got {other:?}"),
    }
}

fn names(draft: &str, ctx: &Context) -> Vec<&'static str> {
    menu(draft, ctx).into_iter().map(|spec| spec.name).collect()
}

// ---------------------------------------------------------------------------
// Which commands a room offers
// ---------------------------------------------------------------------------

/// A chat lists the chat set and help; an agent DM lists the agent set; an
/// agent conversation lists only help — absent, not greyed (AD-27).
#[test]
fn each_room_lists_only_the_commands_it_offers() {
    assert_eq!(
        names("/", &chat()),
        ["me", "shrug", "plain", "topic", "invite", "leave", "help"]
    );
    assert_eq!(names("/", &agent_dm()), ["new", "model", "help"]);
    assert_eq!(names("/", &agent_conversation()), ["help"]);
}

/// A real command typed where it is not offered is refused with "not here",
/// never run, never sent as text, and never mistaken for its neighbour.
#[test]
fn a_command_from_the_other_set_is_refused_where_it_is_not_offered() {
    assert_eq!(
        refused(resolve("/new plans", &chat())),
        RefusalReason::NotHere { name: "new" }
    );
    assert_eq!(
        refused(resolve("/leave", &agent_dm())),
        RefusalReason::NotHere { name: "leave" }
    );
    assert_eq!(
        refused(resolve("/new", &agent_conversation())),
        RefusalReason::NotHere { name: "new" }
    );
}

/// A prefix reaches only what this room offers: `/c` is help's alias in a chat
/// and the new conversation's alias in the agent DM.
#[test]
fn a_prefix_reaches_only_what_the_room_offers() {
    assert_eq!(commanded(resolve("/c", &chat())).0, "help");
    assert_eq!(commanded(resolve("/c", &agent_dm())).0, "new");
}

// ---------------------------------------------------------------------------
// Resolution order
// ---------------------------------------------------------------------------

#[test]
fn an_alias_is_exact_and_beats_a_prefix() {
    // `h` is help's alias, though no name begins with it before help anyway;
    // `part` is leave's, and `emote` is me's.
    assert_eq!(commanded(resolve("/h", &chat())).0, "help");
    assert_eq!(commanded(resolve("/part", &chat())).0, "leave");
    assert_eq!(
        resolve("/emote waves", &chat()),
        Resolution::Emote("waves".to_owned())
    );
}

#[test]
fn the_first_prefix_in_registry_order_wins_and_case_does_not_matter() {
    assert_eq!(
        resolve("/M waves", &chat()),
        Resolution::Emote("waves".to_owned())
    );
    assert_eq!(
        commanded(resolve("/t a new topic", &chat())),
        ("topic", Some("a new topic".to_owned()))
    );
    assert_eq!(text(resolve("/sh", &chat())), SHRUG);
}

/// Once a name is typed in full the menu is that row alone.
#[test]
fn a_full_name_is_the_whole_menu() {
    assert_eq!(names("/topic", &chat()), ["topic"]);
    assert_eq!(names("/topic Friday", &chat()), ["topic"]);
    assert!(names("hello", &chat()).is_empty());
    assert!(names("//etc", &chat()).is_empty());
}

// ---------------------------------------------------------------------------
// What each command becomes
// ---------------------------------------------------------------------------

#[test]
fn me_is_an_action_and_shrug_and_plain_are_text() {
    assert_eq!(
        resolve("/me  waves  ", &chat()),
        Resolution::Emote("waves".to_owned())
    );
    assert_eq!(text(resolve("/shrug", &chat())), SHRUG);
    assert_eq!(
        text(resolve("/shrug no idea", &chat())),
        format!("{SHRUG} no idea")
    );
    // `/plain` is how a line that looks like a command is sent as typed.
    assert_eq!(text(resolve("/plain /topic", &chat())), "/topic");
}

#[test]
fn topic_invite_leave_new_and_help_are_acts_on_the_room() {
    assert_eq!(
        commanded(resolve("/topic Release on Friday", &chat())),
        ("topic", Some("Release on Friday".to_owned()))
    );
    assert_eq!(
        commanded(resolve("/invite @bob:example.org", &chat())),
        ("invite", Some("@bob:example.org".to_owned()))
    );
    assert_eq!(commanded(resolve("/leave", &chat())), ("leave", None));
    assert_eq!(
        commanded(resolve("/new Trip plans", &agent_dm())),
        ("new", Some("Trip plans".to_owned()))
    );
    assert_eq!(commanded(resolve("/new", &agent_dm())), ("new", None));
    assert_eq!(
        commanded(resolve("/help", &agent_conversation())),
        ("help", None)
    );
}

/// An invitee that is not a Matrix id is refused before anything leaves.
#[test]
fn invite_refuses_what_is_not_a_matrix_id() {
    assert_eq!(
        refused(resolve("/invite bob", &chat())),
        RefusalReason::NotAMatrixId
    );
}

#[test]
fn arguments_are_checked_against_what_the_command_takes() {
    assert_eq!(
        refused(resolve("/leave now", &chat())),
        RefusalReason::TakesNothing { name: "leave" }
    );
    for (draft, name) in [("/me", "me"), ("/topic   ", "topic"), ("/plain", "plain")] {
        assert_eq!(
            refused(resolve(draft, &chat())),
            RefusalReason::NeedsArgument { name },
            "{draft}"
        );
    }
}

// ---------------------------------------------------------------------------
// What the room allows right now
// ---------------------------------------------------------------------------

/// A power level that does not allow the act lists the row with its reason
/// and refuses to run it.
#[test]
fn power_levels_list_the_row_with_a_reason_and_refuse_it() {
    let ctx = Context {
        may_set_topic: false,
        may_invite: false,
        ..chat()
    };
    let spec = |name: &str| {
        COMMANDS
            .iter()
            .find(|spec| spec.name == name)
            .expect("registered")
    };
    assert_eq!(
        availability(spec("topic"), &ctx),
        Availability::Unavailable(Need::SetTopic)
    );
    assert_eq!(
        availability(spec("invite"), &ctx),
        Availability::Unavailable(Need::Invite)
    );
    assert_eq!(availability(spec("me"), &ctx), Availability::Available);
    // The menu still lists the unavailable rows, each saying why.
    let preview = ChatCommandPreviewVm::compose("/", &ctx);
    let row = |name: &str| {
        preview
            .rows
            .iter()
            .find(|row| row.name == name)
            .cloned()
            .expect("row listed")
    };
    assert!(!row("topic").available && row("topic").reason.is_some());
    assert!(!row("invite").available && row("invite").reason.is_some());
    assert!(row("me").available && row("me").reason.is_none());
    assert_eq!(
        refused(resolve("/topic x", &ctx)),
        RefusalReason::Unavailable {
            name: "topic",
            need: Need::SetTopic
        }
    );
    assert_eq!(
        refused(resolve("/invite @bob:example.org", &ctx)),
        RefusalReason::Unavailable {
            name: "invite",
            need: Need::Invite
        }
    );
}

/// An action cannot be a reply, so `/me` with a reply pending is refused —
/// the reply is not silently dropped.
#[test]
fn me_is_refused_while_a_reply_is_pending() {
    let ctx = Context {
        replying: true,
        ..chat()
    };
    assert_eq!(
        refused(resolve("/me waves", &ctx)),
        RefusalReason::Unavailable {
            name: "me",
            need: Need::NoReply
        }
    );
    // Text commands still send; they ride the reply like any text.
    assert_eq!(text(resolve("/shrug", &ctx)), SHRUG);
}

// ---------------------------------------------------------------------------
// Unknown commands, the escape, and prose
// ---------------------------------------------------------------------------

/// DW-1431 (R415): `/model` in the agent DM is an act carrying what followed
/// it; where the agent's home is not on this device it is listed with its
/// reason and refused, never run; a chat does not offer it.
#[test]
fn model_is_the_agent_dms_and_needs_the_home_here() {
    assert_eq!(
        commanded(resolve("/model Opus high", &agent_dm())),
        ("model", Some("Opus high".to_owned()))
    );
    assert_eq!(commanded(resolve("/model", &agent_dm())), ("model", None));
    assert_eq!(commanded(resolve("/mo", &agent_dm())).0, "model");
    let away = Context {
        agent_home_here: false,
        ..agent_dm()
    };
    let spec = COMMANDS
        .iter()
        .find(|spec| spec.name == "model")
        .expect("registered");
    assert_eq!(
        availability(spec, &away),
        Availability::Unavailable(Need::HomeHere)
    );
    assert_eq!(names("/mod", &away), ["model"], "listed, with its reason");
    assert_eq!(
        refused(resolve("/model Opus", &away)),
        RefusalReason::Unavailable {
            name: "model",
            need: Need::HomeHere
        }
    );
    assert_eq!(
        refused(resolve("/model", &chat())),
        RefusalReason::NotHere { name: "model" }
    );
}

/// An unknown command is a refusal that names the nearest offered command and
/// is never text — and the nearest is chosen among what this room offers.
#[test]
fn an_unknown_command_is_refused_and_names_the_nearest_offered() {
    assert_eq!(
        refused(resolve("/tpoic Friday", &chat())),
        RefusalReason::Unknown {
            nearest: Some("topic")
        }
    );
    // In the agent DM `/nwe` is near /new, which a chat does not offer.
    assert_eq!(
        refused(resolve("/nwe", &agent_dm())),
        RefusalReason::Unknown {
            nearest: Some("new")
        }
    );
    // A chat never names it.
    let RefusalReason::Unknown { nearest } = refused(resolve("/nwe", &chat())) else {
        panic!("expected an unknown-command refusal");
    };
    assert_ne!(nearest, Some("new"));
    assert_eq!(
        refused(resolve("/qqqqqqq", &chat())),
        RefusalReason::Unknown { nearest: None }
    );
}

#[test]
fn a_doubled_slash_sends_one_and_prose_is_untouched() {
    assert_eq!(text(resolve("//etc/hosts", &chat())), "/etc/hosts");
    assert_eq!(text(resolve("//me", &agent_dm())), "/me");
    assert_eq!(text(resolve("see /etc", &chat())), "see /etc");
    assert_eq!(text(resolve("/", &chat())), "/");
    // A multi-line draft is prose even when it starts with a slash.
    assert_eq!(text(resolve("/me\nwaves", &chat())), "/me\nwaves");
}

// ---------------------------------------------------------------------------
// The preview the composer reads
// ---------------------------------------------------------------------------

/// The verdict the composer obeys is the decision's kind: an action, text, or
/// a refusal that sends nothing.
#[test]
fn the_preview_verdict_is_the_kind_the_composer_obeys() {
    assert_eq!(
        ChatCommandPreviewVm::compose("/me waves", &chat()).verdict,
        ChatCommandVerdictVm::Emote {
            text: "waves".to_owned()
        }
    );
    assert_eq!(
        ChatCommandPreviewVm::compose("hello", &chat()).verdict,
        ChatCommandVerdictVm::Text {
            text: "hello".to_owned()
        }
    );
    assert!(matches!(
        ChatCommandPreviewVm::compose("/zz", &chat()).verdict,
        ChatCommandVerdictVm::Refusal { .. }
    ));
}

/// R393: a one-line command past the bound is refused, never sent as the
/// text of a command nobody ran — a real name, a typo, ASCII or multibyte —
/// while `//` stays text at any length and a multi-line draft stays prose.
#[test]
fn a_command_line_past_the_bound_is_refused_and_the_escape_is_text_at_any_length() {
    let topic = |len: usize| format!("/topic {}", "a".repeat(len - "/topic ".len()));
    // At the bound it is still the command; one byte over, a refusal.
    assert_eq!(
        commanded(resolve(&topic(MAX_COMMAND_CHARS), &chat())).0,
        "topic"
    );
    assert_eq!(
        refused(resolve(&topic(MAX_COMMAND_CHARS + 1), &chat())),
        RefusalReason::TooLong
    );
    // An unknown name past the bound is refused too, not sent.
    let typo = format!("/tpoic {}", "a".repeat(MAX_COMMAND_CHARS));
    assert_eq!(refused(resolve(&typo, &chat())), RefusalReason::TooLong);
    // The bound is bytes: 300 two-byte letters fit in characters, not in bytes.
    let multibyte = format!("/me {}", "ż".repeat(300));
    assert!(multibyte.chars().count() <= MAX_COMMAND_CHARS);
    assert!(multibyte.len() > MAX_COMMAND_CHARS);
    assert_eq!(
        refused(resolve(&multibyte, &chat())),
        RefusalReason::TooLong
    );
    // `//` is the escape at any length: one slash off, the rest untouched.
    let long_path = format!("//{}", "x".repeat(MAX_COMMAND_CHARS * 2));
    assert_eq!(text(resolve(&long_path, &chat())), &long_path[1..]);
    // A multi-line draft is prose, however long.
    let prose = format!("/me\n{}", "y".repeat(MAX_COMMAND_CHARS));
    assert_eq!(text(resolve(&prose, &chat())), prose);
}
