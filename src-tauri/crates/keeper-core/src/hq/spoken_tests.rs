//! The spoken question's pure halves: *Speak to*'s value, which turn answers,
//! what is heard of it and when the turn gives up (108.2). The heard row's
//! metadata is `tests/voice_heard_mark.rs`'.

use super::*;

fn turn(turn_id: &str, text: &str, stopped: bool) -> AgentLiveTurnVm {
    AgentLiveTurnVm {
        turn_id: turn_id.to_owned(),
        text: text.to_owned(),
        stopped,
    }
}

fn sentences(steps: &[SpokenStep]) -> Vec<&str> {
    steps
        .iter()
        .filter_map(|step| match step {
            SpokenStep::Sentence(sentence) => Some(sentence.text.as_str()),
            _ => None,
        })
        .collect()
}

/// `agent:<agent>` names an agent; a value naming one of the agents' old
/// Matrix rooms (`agent:!room…`) names none and is told apart, so it is
/// read as unset with its notice (UX-DR-E108-1); a bot id is neither.
#[test]
fn speak_to_names_an_agent_and_an_old_room_value_names_none() {
    assert_eq!(agent_target("nixie"), "agent:nixie");
    assert_eq!(target_agent("agent:nixie"), Some("nixie"));
    assert_eq!(target_agent(" agent: nixie "), Some("nixie"));
    for none in ["agent:", "agent:  ", "agent:!dm:example.org", "01JBOT", ""] {
        assert_eq!(target_agent(none), None, "{none}");
    }
    assert!(is_old_room_target("agent:!dm:example.org"));
    assert!(is_old_room_target(" agent: !dm:example.org"));
    assert!(!is_old_room_target("agent:nixie"));
    assert!(!is_old_room_target("01JBOT"));
}

/// The list is every agent the held databases name, each with the value
/// that chooses it.
#[test]
fn speak_to_lists_every_agent_of_the_databases() {
    let choice = |agent: &str, name: &str| sessions::AgentChoiceVm {
        agent_actor: agent.to_owned(),
        agent_name: name.to_owned(),
        agent_mark: name[..1].to_owned(),
        databases: vec!["tgdrive".to_owned()],
        database: Some("tgdrive".to_owned()),
        ask: false,
    };
    assert_eq!(
        targets(&[
            choice("nixie", "Nixie"),
            choice("tola-grey", "Dr Tola Grey")
        ]),
        [
            VoiceAgentTargetVm {
                target: "agent:nixie".to_owned(),
                agent: "nixie".to_owned(),
                name: "Nixie".to_owned(),
            },
            VoiceAgentTargetVm {
                target: "agent:tola-grey".to_owned(),
                agent: "tola-grey".to_owned(),
                name: "Dr Tola Grey".to_owned(),
            },
        ]
    );
}

/// Only the turn naming the question is followed: another question's
/// answer streaming meanwhile is never heard. Each closed sentence is
/// handed out once as the text grows, and the agent's row completes it with
/// what follows the last one.
#[test]
fn only_the_turn_naming_the_question_is_heard_sentence_by_sentence() {
    let start = Instant::now();
    let mut watch = SpokenWatch::new("q1".to_owned(), "Nixie".to_owned(), start);
    let other = turn("q0", "An older answer. Still going.", false);
    assert_eq!(watch.see(std::slice::from_ref(&other), None, start), []);

    let later = start + Duration::from_millis(700);
    let first = watch.see(
        &[other.clone(), turn("q1", "It is sunny. It", false)],
        None,
        later,
    );
    assert_eq!(first[0], SpokenStep::Anchored);
    assert_eq!(first[1], SpokenStep::FirstText { after_ms: 700 });
    assert_eq!(sentences(&first), ["It is sunny."]);

    let more = watch.see(
        &[turn("q1", "It is sunny. It will rain later. Bring", false)],
        None,
        later,
    );
    assert_eq!(sentences(&more), ["It will rain later."], "said once each");

    let done = watch.see(
        &[],
        Some(&AnswerRow {
            body: "It is sunny. It will rain later. Bring a coat".to_owned(),
            failed: false,
        }),
        later,
    );
    assert_eq!(
        done.last(),
        Some(&SpokenStep::Complete(Some(Sentence {
            text: "Bring a coat".to_owned(),
            end: 45,
        })))
    );
    assert!(watch.is_done());
    assert_eq!(watch.deadline(), None);
    assert_eq!(watch.see(&[turn("q1", "more", false)], None, later), []);
}

/// An answer that arrives whole — no live turn seen — is spoken whole; a
/// turn the runtime ended in an error says so, naming the agent.
#[test]
fn a_whole_answer_is_spoken_and_a_failed_one_says_so() {
    let at = Instant::now();
    let mut whole = SpokenWatch::new("q1".to_owned(), "Nixie".to_owned(), at);
    let steps = whole.see(
        &[],
        Some(&AnswerRow {
            body: "Done. All good.".to_owned(),
            failed: false,
        }),
        at,
    );
    assert_eq!(steps[0], SpokenStep::Anchored);
    assert_eq!(sentences(&steps), ["Done."]);
    assert!(
        matches!(steps.last(), Some(SpokenStep::Complete(Some(rest))) if rest.text == "All good.")
    );

    let mut failed = SpokenWatch::new("q2".to_owned(), "Nixie".to_owned(), at);
    let steps = failed.see(
        &[],
        Some(&AnswerRow {
            body: String::new(),
            failed: true,
        }),
        at,
    );
    assert_eq!(
        steps.last(),
        Some(&SpokenStep::Failed("Nixie could not answer.".to_owned()))
    );
}

/// No answer within a minute, or nothing new for two minutes once it
/// answered, or an instance that stopped without ending its turn, ends the
/// turn with a sentence naming the agent.
#[test]
fn a_silent_or_stopped_agent_ends_the_turn() {
    let at = Instant::now();
    let mut silent = SpokenWatch::new("q1".to_owned(), "Nixie".to_owned(), at);
    assert_eq!(silent.deadline(), Some(at + ANSWER_START_WAIT));
    assert_eq!(
        silent.expire(at + ANSWER_START_WAIT - Duration::from_millis(1)),
        None
    );
    assert_eq!(
        silent.expire(at + ANSWER_START_WAIT),
        Some(SpokenStep::Failed(
            "Nixie has not answered: no instance of it may be running right now.".to_owned()
        ))
    );
    assert!(silent.is_done());

    let mut quiet = SpokenWatch::new("q1".to_owned(), "Nixie".to_owned(), at);
    let began = at + Duration::from_secs(5);
    quiet.see(&[turn("q1", "Let me look", false)], None, began);
    assert_eq!(quiet.deadline(), Some(began + ANSWER_QUIET_WAIT));
    let grew = began + Duration::from_secs(90);
    quiet.see(&[turn("q1", "Let me look at it", false)], None, grew);
    assert_eq!(
        quiet.deadline(),
        Some(grew + ANSWER_QUIET_WAIT),
        "growth moves it"
    );
    quiet.see(
        &[turn("q1", "Let me look at it", false)],
        None,
        grew + Duration::from_secs(60),
    );
    assert_eq!(
        quiet.deadline(),
        Some(grew + ANSWER_QUIET_WAIT),
        "the same text again is nothing new"
    );
    assert_eq!(
        quiet.expire(grew + ANSWER_QUIET_WAIT),
        Some(SpokenStep::Failed("Nixie stopped answering.".to_owned()))
    );

    let mut stopped = SpokenWatch::new("q1".to_owned(), "Nixie".to_owned(), at);
    let steps = stopped.see(&[turn("q1", "Half an", true)], None, at);
    assert_eq!(
        steps.last(),
        Some(&SpokenStep::Failed("Nixie stopped answering.".to_owned()))
    );
}

/// Only the voice turn's current question's follower is kept.
#[test]
fn only_the_current_questions_follower_is_kept() {
    let mut slot: FollowSlot<&str> = FollowSlot::new();
    assert_eq!(slot.install(1, Some(1), "one"), None);
    assert_eq!(
        slot.install(2, Some(1), "late"),
        Some("late"),
        "not the turn's"
    );
    assert_eq!(slot.install(1, Some(1), "again"), Some("one"));
    assert_eq!(slot.keep_only(Some(1)), None);
    assert_eq!(slot.keep_only(Some(2)), Some("again"));
    assert_eq!(slot.keep_only(Some(2)), None);
}
