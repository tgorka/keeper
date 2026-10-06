//! A question for a person, asked through their proxy (AD-380, story 94.2;
//! rulings R99–R103).
//!
//! An agent's `ask_human` ends its turn (R99): its host sends one
//! `m.room.message` into the session's own room whose body is the question
//! and whose content carries an [`AskContent`] ([`ask_content`]), the
//! person's proxy invited to read it. The proxy's host takes it into the
//! proxy's `main` DM, the person answers there in their own words, and the
//! proxy relays the answer into the asking room with `reply(text, ask)`
//! ([`answer_content`], R100) — the answer is the next arrival of the asking
//! session, never a thread waiting.
//!
//! Who is asked is the head of the session's dispatch chain, through their
//! proxy ([`answerer`], R102); when there is nobody, the question's stated
//! default is the answer, or the call is refused with [`NO_ONE_TO_ASK`]
//! (R103). Pure: the host knows the agents and sends.

use std::collections::BTreeSet;

use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ulid::Ulid;

use crate::agents::events::{ANSWER, ASK, CONTENT_VERSION};
use crate::agents::home::AgentKind;
use crate::agents::label::{Label, Readers, Sink};
use crate::agents::session::SessionKind;

/// What `ask_human` says when nobody can be asked and the question names
/// no default (the epic's Q4).
pub const NO_ONE_TO_ASK: &str = "No person can answer this run and the question names no default.";

/// `dev.keeper.agent.ask`, inside the ask's encrypted content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AskContent {
    pub v: u32,
    /// A ULID: what the answer names.
    pub id: String,
    pub question: String,
    /// The answers the question offers, in order; empty for a free answer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// The answer an unattended run takes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// The person asked.
    pub to: OwnedUserId,
    /// Their proxy, which relays the question and the answer.
    pub via: OwnedUserId,
    /// The asking session's label when it asked: the proxy's DM joins it
    /// for the turn that relays the question (S-09), and the answer goes
    /// back as the person's own message to that session's readers (R101).
    pub label: Label,
}

/// `dev.keeper.agent.answer`, inside the relayed answer's encrypted content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnswerContent {
    pub v: u32,
    /// The ask it answers.
    pub id: String,
}

/// The text an ask shows a person: the question, then each choice on its
/// own line under its number — the number is an answer too.
pub fn ask_text(question: &str, choices: &[String]) -> String {
    let mut text = question.to_owned();
    for (n, choice) in choices.iter().enumerate() {
        text.push_str(&format!("\n{}. {choice}", n + 1));
    }
    text
}

/// The most bytes an ask's content takes before it is encrypted: Megolm's
/// base64 adds a third, and the whole event stays under the homeserver's
/// 64 KiB cap with room for its envelope (R199, R94A-12).
pub const ASK_EVENT_BYTES: usize = 40 * 1024;

/// The ask's `m.room.message` content: an ordinary text message, so a
/// device shows the question (R113), carrying the ask.
pub fn ask_content(ask: &AskContent) -> Value {
    json!({
        "msgtype": "m.text",
        "body": ask_text(&ask.question, &ask.choices),
        ASK: ask,
    })
}

/// The ask a message's content carries, when it carries one this build
/// reads.
pub fn read_ask(content: &Value) -> Option<AskContent> {
    let read: AskContent = serde_json::from_value(content.get(ASK)?.clone()).ok()?;
    (read.v == CONTENT_VERSION && Ulid::from_string(&read.id).is_ok()).then_some(read)
}

/// The ask a message carries when it travels as one: an `m.room.message`
/// of `m.text`, no edit, whose body is the question it carries with its
/// choices — what a person is shown is what the proxy is asked.
pub fn enveloped_ask(event_type: &str, content: &Value) -> Option<AskContent> {
    if event_type != "m.room.message"
        || content["msgtype"] != "m.text"
        || content["m.relates_to"]["rel_type"] == "m.replace"
    {
        return None;
    }
    read_ask(content).filter(|ask| content["body"] == ask_text(&ask.question, &ask.choices))
}

/// A relayed answer's `m.room.message` content: the person's answer as the
/// proxy relays it, naming the ask `id`.
pub fn answer_content(text: &str, id: &str) -> Value {
    json!({
        "msgtype": "m.text",
        "body": text,
        ANSWER: AnswerContent {
            v: CONTENT_VERSION,
            id: id.to_owned(),
        },
    })
}

/// The ask a message's content answers, when it carries an answer this
/// build reads.
pub fn read_answer(content: &Value) -> Option<AnswerContent> {
    let read: AnswerContent = serde_json::from_value(content.get(ANSWER)?.clone()).ok()?;
    (read.v == CONTENT_VERSION && Ulid::from_string(&read.id).is_ok()).then_some(read)
}

/// Whether a session of `session` kind, of an agent of `agent` kind, is
/// offered `ask_human` (R102): every session but a proxy's own `main` and
/// `conversation`, where its person is already the one it talks to —
/// whatever `[tools].allow` says.
pub fn offered(agent: AgentKind, session: SessionKind) -> bool {
    !(agent == AgentKind::Proxy && matches!(session, SessionKind::Main | SessionKind::Conversation))
}

/// Who answers an ask, and through which proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answerer {
    pub person: OwnedUserId,
    pub via: OwnedUserId,
}

/// Who answers an ask of a session whose dispatch chain is `chain` (R102):
/// its head, when it is a person, through `chain[1]` when that is their
/// proxy, else through the proxy `proxies` (`(proxy, human)`, the agents a
/// host knows) names for them, else through the proxy of their `[[trust]]`
/// entry in `trusted` (`(person, proxy)`). `None`: nobody can be asked.
pub fn answerer(
    chain: &[OwnedUserId],
    proxies: &[(OwnedUserId, OwnedUserId)],
    trusted: &[(OwnedUserId, OwnedUserId)],
    is_agent: impl Fn(&UserId) -> bool,
) -> Option<Answerer> {
    let person = chain.first().filter(|head| !is_agent(head))?;
    let known = || proxies.iter().filter(|(_, human)| human == person);
    let pinned = || trusted.iter().filter(|(user, _)| user == person);
    let proxy_of = |user: &OwnedUserId| {
        known().any(|(proxy, _)| proxy == user) || pinned().any(|(_, proxy)| proxy == user)
    };
    let via = chain
        .get(1)
        .filter(|next| proxy_of(next))
        .or_else(|| known().map(|(proxy, _)| proxy).next())
        .or_else(|| pinned().map(|(_, proxy)| proxy).next())?;
    Some(Answerer {
        person: person.clone(),
        via: via.clone(),
    })
}

/// The sink an ask is (AD-391, NFR-115): a send into `person`'s DM with
/// their proxy, whose audience is `proxy_audience` when the host knows it.
/// A session whose label does not reach them asks nobody.
pub fn proxy_dm(person: &UserId, proxy_audience: Option<Readers>) -> Sink {
    Sink::Room {
        humans: BTreeSet::from([person.to_owned()]),
        agent_audiences: proxy_audience.into_iter().collect(),
    }
}

/// The choice `answer` picks among `choices`: its 1-based number, or its
/// text compared case-folded, surrounding space aside. `None` when it picks
/// none, or there are none.
pub fn choice_of(answer: &str, choices: &[String]) -> Option<String> {
    let answer = answer.trim();
    if let Ok(n) = answer.parse::<usize>() {
        return n.checked_sub(1).and_then(|at| choices.get(at)).cloned();
    }
    let folded = answer.to_lowercase();
    choices
        .iter()
        .find(|choice| choice.trim().to_lowercase() == folded)
        .cloned()
}

/// What an asking session's status and `run` line say while its ask waits.
pub fn waiting_detail(person: &UserId, proxy: &str) -> String {
    format!("waiting for {}, through {proxy}", person.localpart())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::label::{check_sink, Integrity, SinkVerdict};

    fn user(id: &str) -> OwnedUserId {
        UserId::parse(id).expect("user")
    }

    const TGORKA: &str = "@tgorka:h";
    const NIXI: &str = "@nixi:h";

    fn readers(ids: &[&str]) -> Readers {
        Readers::Only(ids.iter().map(|id| user(id)).collect())
    }

    /// 94.2 acceptance 6's sink: a session read by tgorka and Marta asks
    /// tgorka through Nixi (audience tgorka); a session tgorka does not
    /// read — or whose proxy's audience goes wider than it — asks nobody,
    /// and the block names who it would reach.
    #[test]
    fn ask_human_is_a_send_into_the_proxys_dm() {
        let label = |ids: &[&str]| Label {
            readers: readers(ids),
            integrity: Integrity::Owner,
            local_only: false,
        };
        let nixi = || Some(readers(&[TGORKA]));
        let dm = proxy_dm(&user(TGORKA), nixi());
        assert_eq!(
            check_sink(&label(&[TGORKA, "@marta:h"]), &dm),
            SinkVerdict::Allow
        );
        assert!(matches!(
            check_sink(&label(&["@marta:h"]), &dm),
            SinkVerdict::Block { wider, .. } if wider == BTreeSet::from([user(TGORKA)])
        ));
        // A proxy whose home Marta reads too is wider than tgorka's session.
        let wide = proxy_dm(&user(TGORKA), Some(readers(&[TGORKA, "@marta:h"])));
        assert!(matches!(
            check_sink(&label(&[TGORKA]), &wide),
            SinkVerdict::Block { .. }
        ));
        // A proxy this host does not know is its person's door alone.
        assert_eq!(
            check_sink(&label(&[TGORKA]), &proxy_dm(&user(TGORKA), None)),
            SinkVerdict::Allow
        );
    }
    const MIRA: &str = "@mira:h";
    const TOLA: &str = "@tola:h";
    const LENA: &str = "@lena:h";

    fn ask() -> AskContent {
        AskContent {
            v: CONTENT_VERSION,
            id: Ulid::new().to_string(),
            question: "Go on to step 3?".to_owned(),
            choices: vec!["Continue".to_owned(), "Stop".to_owned()],
            default: Some("Continue".to_owned()),
            to: user(TGORKA),
            via: user(NIXI),
            label: Label {
                readers: Readers::Only(BTreeSet::from([user(TGORKA)])),
                integrity: Integrity::Owner,
                local_only: false,
            },
        }
    }

    /// 94.2 acceptance 7: `1`, `continue` and `Continue` all pick
    /// `Continue`; an answer that picks nothing — another number, other
    /// text, any answer to a free question — is `None`.
    #[test]
    fn ask_human_defaults_and_choices() {
        let choices = vec!["Continue".to_owned(), "Stop".to_owned()];
        for answer in ["1", "continue", "Continue", "  CONTINUE "] {
            assert_eq!(
                choice_of(answer, &choices).as_deref(),
                Some("Continue"),
                "{answer}"
            );
        }
        assert_eq!(choice_of("2", &choices).as_deref(), Some("Stop"));
        for answer in ["0", "3", "go on", "Continue please", ""] {
            assert_eq!(choice_of(answer, &choices), None, "{answer}");
        }
        assert_eq!(choice_of("1", &[]), None);
    }

    /// R102: the chain's head, through `chain[1]` when it is their proxy,
    /// else the known proxy whose person they are, else their `[[trust]]`
    /// entry's; an agent at the head, or a person nobody relays for, is
    /// nobody.
    #[test]
    fn the_head_of_the_chain_answers_through_their_proxy() {
        let is_agent = |id: &UserId| [NIXI, MIRA, TOLA, LENA].contains(&id.as_str());
        let proxies = vec![(user(NIXI), user(TGORKA))];
        let trusted = vec![(user(TGORKA), user(MIRA))];
        let of = |chain: &[&str],
                  proxies: &[(OwnedUserId, OwnedUserId)],
                  trusted: &[(OwnedUserId, OwnedUserId)]| {
            let chain: Vec<OwnedUserId> = chain.iter().map(|id| user(id)).collect();
            answerer(&chain, proxies, trusted, is_agent)
                .map(|a| (a.person.to_string(), a.via.to_string()))
        };
        let through = |via: &str| Some((TGORKA.to_owned(), via.to_owned()));
        // Nixi handed it on: she relays.
        assert_eq!(of(&[TGORKA, NIXI, TOLA], &proxies, &trusted), through(NIXI));
        // A pinned proxy that handed it on relays, a known one beside it
        // notwithstanding.
        assert_eq!(of(&[TGORKA, MIRA, TOLA], &proxies, &trusted), through(MIRA));
        // Another agent handed it on: the known proxy, then the pinned one.
        assert_eq!(of(&[TGORKA, LENA, TOLA], &proxies, &trusted), through(NIXI));
        assert_eq!(of(&[TGORKA, LENA, TOLA], &[], &trusted), through(MIRA));
        // Nobody relays for tgorka; an agent heads the chain; no chain.
        assert_eq!(of(&[TGORKA, LENA, TOLA], &[], &[]), None);
        assert_eq!(of(&[TOLA, TOLA], &proxies, &trusted), None);
        assert_eq!(of(&[], &proxies, &trusted), None);
        // Marta has no proxy here, whoever relays for tgorka.
        assert_eq!(of(&["@marta:h", NIXI], &proxies, &trusted), None);
    }

    /// R102: offered in every session but a proxy's own two.
    #[test]
    fn every_session_but_a_proxys_own_is_offered_ask_human() {
        for session in SessionKind::ALL {
            let own = matches!(session, SessionKind::Main | SessionKind::Conversation);
            assert_eq!(offered(AgentKind::Proxy, session), !own, "{session:?}");
            for agent in [AgentKind::Specialist, AgentKind::Steward, AgentKind::Gate] {
                assert!(offered(agent, session), "{agent:?} {session:?}");
            }
        }
    }

    /// An ask is the question a person is shown, choices numbered; its body
    /// must say what it carries, an edit or a notice carries none, and an
    /// answer names its ask.
    #[test]
    fn an_ask_carries_what_its_body_says() {
        let ask = ask();
        let content = ask_content(&ask);
        assert_eq!(content["body"], "Go on to step 3?\n1. Continue\n2. Stop");
        assert_eq!(enveloped_ask("m.room.message", &content), Some(ask.clone()));
        let mut said_else = content.clone();
        said_else["body"] = json!("Go on to step 4?\n1. Continue\n2. Stop");
        assert_eq!(enveloped_ask("m.room.message", &said_else), None);
        let mut notice = content.clone();
        notice["msgtype"] = json!("m.notice");
        assert_eq!(enveloped_ask("m.room.message", &notice), None);
        let mut edit = content.clone();
        edit["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$x"});
        assert_eq!(enveloped_ask("m.room.message", &edit), None);
        let mut newer = content.clone();
        newer[ASK]["v"] = json!(2);
        assert_eq!(read_ask(&newer), None);

        let answer = answer_content("continue", &ask.id);
        assert_eq!(answer["body"], "continue");
        assert_eq!(read_answer(&answer).map(|a| a.id), Some(ask.id.clone()));
        assert_eq!(read_answer(&answer_content("x", "not-a-ulid")), None);
    }
}
