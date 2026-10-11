//! `ask_human`: an agent asks the person its work is for, through their
//! proxy (AD-380, story 94.2; rulings R99–R103, R197–R199).
//!
//! # Asking
//!
//! `ask_human` is offered by the session's kind (R102): everywhere but a
//! proxy's own `main` and `conversation`. Who answers is the head of the
//! session's dispatch chain, through their proxy ([`answerer`]). With
//! nobody to ask, the question's default is the answer at once, or the call
//! is refused with [`NO_ONE_TO_ASK`] (R103).
//!
//! Otherwise the call checks the ask as a send into the person's DM with
//! their proxy ([`proxy_dm`], AD-391) and into the session's own room as it
//! is now (R168), writes an `ask asked` line — the ask's intent, carrying
//! all its worker needs to send it — and `run: blocked` ("waiting for
//! tgorka, through Nixi"), and returns at once telling the model to end its
//! turn (R99): no thread waits for a person. The round gate refuses the
//! turn's next round once it asked. Nothing is published by the call: once
//! the turn's lines are on disk, the worker invites the proxy, waits for its
//! join (R197) and sends the question — an ordinary message carrying
//! `dev.keeper.agent.ask` — through one final check of the room as it is
//! then (`ServedSession::send_asks`, R199).
//!
//! # Relaying
//!
//! The proxy's host takes the ask into the proxy's DM (`rooms::admit_ask`,
//! the runtime's interception and read-back); its person answers there; the
//! proxy's `reply(ask, text?)` ([`relay`], R100) sends the person's own
//! message — the host's copy of it, never words the model wrote (R199) —
//! into the asking room under the person's own label for that session's
//! readers (R101); the host leaves the room once no ask there waits for its
//! relay (S-27). The asking session takes it as `Arrival::Answer`: a `peer`
//! line naming the ask and a turn.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use keeper_core::agents::ask::{
    answer_content, answerer, ask_content, choice_of, proxy_dm, waiting_detail, AskContent,
    ASK_EVENT_BYTES, NO_ONE_TO_ASK,
};
use keeper_core::agents::card::Run;
use keeper_core::agents::events::CONTENT_VERSION;
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::label::{Destination, Integrity, Label, Readers};
use keeper_core::agents::log::{AskBody, AskState, LineBody, RunBody, RunState};
use keeper_core::agents::workflow::{parse_ask, ASK_HUMAN};
use keeper_core::bots::chat::ToolCall as WireToolCall;
use keeper_core::bots::tools::ToolOutcome;
use matrix_sdk::ruma::{OwnedRoomId, OwnedTransactionId, OwnedUserId, UserId};
use serde_json::{json, Value};
use ulid::Ulid;

use crate::delegate::{block_on, room_now, set_card_run, DelegationPort, Delegator, TurnView};
use crate::rooms::{Known, KnownAgent};
use crate::sinks::{CallAudit, RoomGate, Sinks, Withheld};

/// What `ask_human` says where it is not offered.
pub const NOT_OFFERED: &str =
    "ask_human is not offered in your person's own conversation: ask them here, in your own words.";
/// What `reply` with `ask` says where no question waits for an answer.
pub const NO_RELAY: &str =
    "reply with ask relays your person's answer to a question another agent asked them, and none waits here.";
/// What the round gate says once the turn asked: the turn ends (R99).
pub const ASKED: &str =
    "This turn asked a person and ends here; their answer arrives as the next message.";
/// What `reply` with `ask` says while the person said nothing since the
/// question (R199).
pub const NOT_ANSWERED_YET: &str =
    "Your person has said nothing here since the question: ask them, and relay their answer once they have.";
/// What `reply` with `ask` says when its `text` is none of the person's
/// messages since the question (R199).
pub const NOT_THEIR_WORDS: &str =
    "A relay carries your person's own message, exactly as they wrote it since the question: quote one of theirs as text, or leave text out to relay their first.";

/// An ask this session made that waits for its answer, as its log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAsk {
    pub id: String,
    /// The person asked.
    pub to: OwnedUserId,
    /// Their proxy.
    pub via: OwnedUserId,
    pub question: String,
    pub choices: Vec<String>,
    pub default: Option<String>,
    /// The scheduled card whose run asked: its windows wait for the answer,
    /// whose turn finishes it (R199).
    pub card: Option<String>,
    /// Whether the question went into the room.
    pub sent: bool,
}

impl OpenAsk {
    /// The ask as it goes into the room, under the session's label now.
    pub fn content(&self, label: Label) -> AskContent {
        AskContent {
            v: CONTENT_VERSION,
            id: self.id.clone(),
            question: self.question.clone(),
            choices: self.choices.clone(),
            default: self.default.clone(),
            to: self.to.clone(),
            via: self.via.clone(),
            label,
        }
    }

    /// The `ask` line of `state` for it.
    pub fn line(&self, state: AskState) -> AskBody {
        AskBody {
            id: self.id.clone(),
            state,
            to: Some(self.to.clone()),
            via: Some(self.via.clone()),
            room: None,
            question: Some(self.question.clone()),
            choices: self.choices.clone(),
            default: self.default.clone(),
            card: self.card.clone(),
            answer: None,
            choice: None,
            reason: None,
        }
    }

    /// The ask an `ask asked` line describes.
    pub fn of(line: &AskBody) -> Option<OpenAsk> {
        Some(OpenAsk {
            id: line.id.clone(),
            to: line.to.clone()?,
            via: line.via.clone()?,
            question: line.question.clone()?,
            choices: line.choices.clone(),
            default: line.default.clone(),
            card: line.card.clone(),
            sent: false,
        })
    }
}

/// A question another agent asked this proxy's person, waiting for the
/// proxy to relay their answer (R100).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    pub id: String,
    /// The room it was asked in, where the answer goes.
    pub room: OwnedRoomId,
    /// The agent that asked.
    pub asker: OwnedUserId,
    /// The asking session's label when it asked.
    pub label: Label,
    /// The person's own messages in this session since the question, in
    /// order, as its `user` lines hold them: all a relay may carry (R199).
    pub said: Vec<String>,
}

/// The transaction an ask goes into its room under: one per ask, so a send
/// tried again — on the clock, or after a restart — is the same event.
pub fn ask_txn(id: &str) -> OwnedTransactionId {
    OwnedTransactionId::from(format!("ask-{id}"))
}

/// Pairs of users: `(proxy, human)` or `(person, proxy)`.
type Pairs = Vec<(OwnedUserId, OwnedUserId)>;

/// The proxies `known` names, `(proxy, human)`, and the pinned `[[trust]]`
/// proxies, `(person, proxy)`: who can relay a question.
fn relayers(known: &Known) -> (Pairs, Pairs) {
    let proxies = known
        .agents
        .iter()
        .filter(|agent| agent.kind == AgentKind::Proxy)
        .filter_map(|agent| Some((agent.matrix_user.clone(), agent.human.clone()?)))
        .collect();
    let trusted = known
        .trust
        .iter()
        .filter(|trust| trust.master_key.is_some())
        .filter_map(|trust| Some((trust.user.clone(), trust.proxy.clone()?)))
        .collect();
    (proxies, trusted)
}

/// `known` with each proxy of a pinned `[[trust]]` entry that no agent of a
/// mounted drive is, as the proxy of that entry's person — whose DM, and
/// so whose audience, is that person alone (AD-391, R199): what an ask's
/// invite and every check of a room the proxy is in read. A user pinned as
/// the proxy of two people, or that a mounted drive already names, is
/// added as nothing.
pub fn with_pinned_proxies(known: &Known) -> Known {
    let (_, trusted) = relayers(known);
    let mut out = known.clone();
    for (person, proxy) in &trusted {
        let known_here = known.agents.iter().any(|agent| agent.matrix_user == *proxy);
        let contradicted = trusted
            .iter()
            .any(|(other, again)| again == proxy && other != person);
        if known_here || contradicted {
            continue;
        }
        let readers = Readers::Only(BTreeSet::from([person.clone()]));
        out.agents.push(KnownAgent {
            id: proxy.localpart().to_owned(),
            drive: String::new(),
            name: proxy.localpart().to_owned(),
            matrix_user: proxy.clone(),
            kind: AgentKind::Proxy,
            human: Some(person.clone()),
            hosted: false,
            home_readers: readers.clone(),
            opening: Label {
                readers,
                ..Label::top()
            },
            drives: Vec::new(),
        });
    }
    out
}

/// The audience an ask's proxy `via` is checked through: a known agent's
/// home readers, a pinned proxy's person ([`with_pinned_proxies`]).
pub fn proxy_audience(known: &Known, via: &UserId) -> Option<Readers> {
    with_pinned_proxies(known)
        .agents
        .into_iter()
        .find(|agent| agent.matrix_user == via)
        .map(|agent| agent.home_readers)
}

/// Whether anyone could answer an ask of a session whose dispatch chain is
/// `chain`, `me` asking, with `known` (R102): what a workflow's run is
/// stamped by as it opens (R103).
pub fn can_ask(chain: &[OwnedUserId], me: &UserId, known: &Known) -> bool {
    let (proxies, trusted) = relayers(known);
    let is_agent =
        |user: &UserId| user == me || known.agents.iter().any(|agent| agent.matrix_user == user);
    answerer(chain, &proxies, &trusted, is_agent).is_some()
}

/// One turn's `ask_human`.
pub struct AskTools<'t> {
    pub from: Delegator,
    pub port: Option<Arc<dyn DelegationPort>>,
    /// The room's boundary at the call (R168); the send's is the worker's.
    pub gate: Arc<RoomGate>,
    pub view: &'t dyn TurnView,
    /// Whether this session is offered `ask_human` (R102).
    pub offered: bool,
    /// A workflow's run stamped `checkpoints = "unattended"`: nobody is
    /// asked, and every question takes its default (R103).
    pub unattended: bool,
    pub sinks: &'t Sinks,
    /// The scheduled card whose run this turn is, if any.
    pub scheduled: Option<String>,
    lines: Mutex<Vec<LineBody>>,
    asked: AtomicBool,
}

fn refused(reason: impl Into<String>) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused {
        reason: reason.into(),
    })
}

impl<'t> AskTools<'t> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from: Delegator,
        port: Option<Arc<dyn DelegationPort>>,
        gate: Arc<RoomGate>,
        view: &'t dyn TurnView,
        offered: bool,
        unattended: bool,
        sinks: &'t Sinks,
        scheduled: Option<String>,
    ) -> AskTools<'t> {
        AskTools {
            from,
            port,
            gate,
            view,
            offered,
            unattended,
            sinks,
            scheduled,
            lines: Mutex::new(Vec::new()),
            asked: AtomicBool::new(false),
        }
    }

    /// The `ask` and `run` lines written since the last take.
    pub fn take_lines(&self) -> Vec<LineBody> {
        std::mem::take(&mut *self.lines.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Whether this turn asked a person: its next round is refused (R99).
    pub fn asked(&self) -> bool {
        self.asked.load(Ordering::SeqCst)
    }

    fn line(&self, body: LineBody) {
        self.lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(body);
    }

    fn refuse(&self, id: &str, question: Option<&str>, reason: String) -> Option<ToolOutcome> {
        self.line(LineBody::Ask(AskBody {
            id: id.to_owned(),
            state: AskState::Refused,
            to: None,
            via: None,
            room: None,
            question: question.map(str::to_owned),
            choices: Vec::new(),
            default: None,
            card: None,
            answer: None,
            choice: None,
            reason: Some(reason.clone()),
        }));
        refused(reason)
    }

    /// Run `wire` when it is `ask_human`; `None` for any other name. `audit`
    /// is the call's one row (R90): a sink's block is written in it, and
    /// once the sinks pass it is admitted before the intent is written.
    pub fn run(&self, wire: &WireToolCall, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        if wire.name != ASK_HUMAN {
            return None;
        }
        if !self.offered {
            return refused(NOT_OFFERED);
        }
        let args = wire.arguments.as_ref().unwrap_or(&Value::Null);
        let call = match parse_ask(args) {
            Ok(call) => call,
            Err(sentence) => return refused(sentence),
        };
        let id = Ulid::new().to_string();
        let known = self
            .port
            .as_ref()
            .map_or_else(|| Arc::new(Known::default()), |port| port.known());
        let (proxies, trusted) = relayers(&known);
        let is_agent = |user: &UserId| {
            user == self.from.user || known.agents.iter().any(|agent| agent.matrix_user == user)
        };
        let found = (!self.unattended)
            .then(|| answerer(&self.from.chain, &proxies, &trusted, is_agent))
            .flatten();
        let Some(answerer) = found else {
            // Nobody to ask: the stated default answers at once, or the
            // call is refused and the skill's own HALT ends the turn (R103).
            let Some(default) = call.default else {
                return self.refuse(&id, Some(&call.question), NO_ONE_TO_ASK.to_owned());
            };
            let choice = choice_of(&default, &call.choices);
            self.line(LineBody::Ask(AskBody {
                id,
                state: AskState::Defaulted,
                to: None,
                via: None,
                room: None,
                question: Some(call.question),
                choices: call.choices,
                default: Some(default.clone()),
                card: None,
                answer: Some(default.clone()),
                choice: choice.clone(),
                reason: None,
            }));
            return Some(ToolOutcome::Answered {
                text: json!({"answer": default, "choice": choice, "by": "default"}).to_string(),
            });
        };
        if self.port.is_none() {
            return refused(crate::delegate::NO_ROOMS);
        }
        let known = with_pinned_proxies(&known);
        let proxy = known
            .agents
            .iter()
            .find(|agent| agent.matrix_user == answerer.via);
        let audience = proxy.map(|proxy| proxy.home_readers.clone());
        let name = proxy.map_or_else(
            || answerer.via.localpart().to_owned(),
            |proxy| proxy.name.clone(),
        );
        let open = OpenAsk {
            id: id.clone(),
            to: answerer.person.clone(),
            via: answerer.via.clone(),
            question: call.question,
            choices: call.choices,
            default: call.default,
            card: self.scheduled.clone(),
            sent: false,
        };
        let label = self.view.label();
        let effect = ask_content(&open.content(label.clone())).to_string();
        if effect.len() > ASK_EVENT_BYTES {
            return self.refuse(
                &id,
                Some(&open.question),
                format!(
                    "The ask would take {} bytes, more than the {ASK_EVENT_BYTES} one message carries: ask a shorter question.",
                    effect.len()
                ),
            );
        }
        // A send into the person's DM with their proxy, then into this
        // session's room as it is now; the worker asks the room again at
        // the send.
        let admitted = self
            .sinks
            .verdict(
                ASK_HUMAN,
                &Destination::Person {
                    user: answerer.person.clone(),
                },
                &label,
                &proxy_dm(&answerer.person, audience),
                effect.as_bytes(),
                None,
            )
            .or_else(|blocked| audit.blocked(blocked))
            .and_then(|()| {
                block_on(self.gate.verdict(ASK_HUMAN, effect.as_bytes()))
                    .or_else(|blocked| audit.blocked(blocked))
            })
            .and_then(|()| audit.admit("", self.from.room.as_str()));
        match admitted {
            Ok(()) => {}
            Err(Withheld::Refused(reason)) => {
                return self.refuse(&id, Some(&open.question), reason)
            }
            Err(Withheld::Parked(approval)) => return Some(ToolOutcome::Parked { approval }),
        }
        self.line(LineBody::Ask(AskBody {
            room: Some(self.from.room.clone()),
            ..open.line(AskState::Asked)
        }));
        let detail = waiting_detail(&answerer.person, &name);
        self.line(LineBody::Run(RunBody {
            state: RunState::Blocked,
            detail: Some(detail),
            step: None,
        }));
        let (zone, session) = (&self.from.zone, &self.from.session);
        if let Err(error) = set_card_run(zone, session, Run::Blocked, &|| self.view.may_write()) {
            tracing::warn!(%session, %error, "agents: an asking session's card could not be set blocked");
        }
        self.asked.store(true, Ordering::SeqCst);
        Some(ToolOutcome::Answered {
            text: format!(
                "Asked {} through {name} (question {id}). End your turn now, saying what you wait for: their answer arrives as the next message.",
                answerer.person
            ),
        })
    }
}

/// The message of the person's `said` since the question a relay carries:
/// the one `picked` quotes, surrounding space aside, else their first; the
/// refusal when they said nothing yet or `picked` is none of theirs (R199).
pub fn relayed<'r>(said: &'r [String], picked: Option<&str>) -> Result<&'r str, &'static str> {
    if said.is_empty() {
        return Err(NOT_ANSWERED_YET);
    }
    match picked {
        None => Ok(&said[0]),
        Some(picked) => said
            .iter()
            .find(|message| message.trim() == picked.trim())
            .map(String::as_str)
            .ok_or(NOT_THEIR_WORDS),
    }
}

/// The relay of a `reply(ask, text?)` in a proxy's own session (R100,
/// R101, R199): the person's own message since the question — the one
/// `picked` quotes, else their first ([`relayed`]), as this host logged it
/// — into the room the question `relay` came from, as the person's own
/// message to that session's readers, checked against that room as it is
/// now; then the room is left once no other ask there waits (S-27). The
/// `ask answered` line is pushed to `line`.
pub fn relay(
    port: &dyn DelegationPort,
    sinks: &Sinks,
    me: &UserId,
    relay: &Relay,
    picked: Option<&str>,
    audit: &CallAudit<'_>,
    line: &dyn Fn(LineBody),
) -> Option<ToolOutcome> {
    let answer = match relayed(&relay.said, picked) {
        Ok(answer) => answer,
        Err(reason) => return refused(reason),
    };
    let label = Label {
        readers: relay.label.readers.clone(),
        integrity: Integrity::Owner,
        local_only: relay.label.local_only,
    };
    let content = answer_content(answer, &relay.id);
    let known = port.known();
    let audience = crate::delegate::audience_of(&known, &relay.asker)
        .map_or_else(Vec::new, |readers| vec![readers]);
    let admitted = block_on(room_now(port, &relay.room, [me, &relay.asker], audience))
        .map_err(|unread| crate::sinks::Blocked {
            drive: String::new(),
            at: relay.room.to_string(),
            sentence: unread,
            flow: None,
        })
        .and_then(|sink| {
            sinks.verdict(
                crate::delegate::REPLY,
                &Destination::Room {
                    room: relay.room.clone(),
                },
                &label,
                &sink,
                content.to_string().as_bytes(),
                None,
            )
        })
        .or_else(|blocked| audit.blocked(blocked))
        .and_then(|()| audit.admit("", relay.room.as_str()));
    if let Err(withheld) = admitted {
        return Some(withheld.into());
    }
    if let Err(error) = block_on(port.send(
        &relay.room,
        content,
        ask_txn(&format!("{}-answer", relay.id)),
    )) {
        return refused(format!("The answer could not be relayed: {error}"));
    }
    port.depart(&relay.room);
    line(LineBody::Ask(AskBody {
        id: relay.id.clone(),
        state: AskState::Answered,
        to: None,
        via: None,
        room: Some(relay.room.clone()),
        question: None,
        choices: Vec::new(),
        default: None,
        card: None,
        answer: Some(answer.to_owned()),
        choice: None,
        reason: None,
    }));
    Some(ToolOutcome::Answered {
        text: format!(
            "Relayed your person's message \"{answer}\" to {}.",
            relay.asker
        ),
    })
}

#[cfg(test)]
mod tests {
    use keeper_core::agents::agentd::TrustEntry;

    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn agent(id: &str, kind: AgentKind, human: Option<&str>) -> KnownAgent {
        let readers = Readers::Only(BTreeSet::from([user("@tgorka:h")]));
        KnownAgent {
            id: id.to_owned(),
            drive: "tgdrive".to_owned(),
            name: id.to_owned(),
            matrix_user: user(&format!("@{id}:h")),
            kind,
            human: human.map(user),
            hosted: false,
            home_readers: readers,
            opening: Label::top(),
            drives: Vec::new(),
        }
    }

    fn trust(person: &str, proxy: &str, pinned: bool) -> TrustEntry {
        TrustEntry {
            user: user(person),
            proxy: Some(user(proxy)),
            master_key: pinned.then(|| "ed25519:x".to_owned()),
        }
    }

    /// The proxies a host can ask through: a known proxy with a person, and
    /// only a pinned `[[trust]]` entry's proxy.
    #[test]
    fn only_a_known_proxy_or_a_pinned_ones_relays() {
        let known = Known {
            agents: vec![
                agent("nixi", AgentKind::Proxy, Some("@tgorka:h")),
                agent("tola", AgentKind::Steward, None),
                agent("orphan", AgentKind::Proxy, None),
            ],
            trust: vec![
                trust("@marta:h", "@mira:h", true),
                trust("@lucyna:h", "@luna:h", false),
            ],
        };
        let (proxies, trusted) = relayers(&known);
        assert_eq!(proxies, vec![(user("@nixi:h"), user("@tgorka:h"))]);
        assert_eq!(trusted, vec![(user("@marta:h"), user("@mira:h"))]);
    }

    /// R94A-02: a pinned proxy no mounted drive names is checked through
    /// its person alone; an unpinned one, one a mounted drive names, and
    /// one pinned for two people are not made proxies by it.
    #[test]
    fn a_pinned_proxy_is_its_persons_audience_and_no_one_elses() {
        let only = |id: &str| Some(Readers::Only(BTreeSet::from([user(id)])));
        let known = Known {
            agents: vec![agent("tola", AgentKind::Steward, None)],
            trust: vec![
                trust("@marta:h", "@mira:h", true),
                trust("@lucyna:h", "@luna:h", false),
                trust("@tgorka:h", "@tola:h", true),
                trust("@ada:h", "@two:h", true),
                trust("@bea:h", "@two:h", true),
            ],
        };
        assert_eq!(proxy_audience(&known, &user("@mira:h")), only("@marta:h"));
        assert_eq!(proxy_audience(&known, &user("@luna:h")), None);
        assert_eq!(proxy_audience(&known, &user("@tola:h")), only("@tgorka:h"));
        assert_eq!(
            with_pinned_proxies(&known)
                .agents
                .iter()
                .find(|a| a.matrix_user == "@tola:h")
                .map(|a| a.kind),
            Some(AgentKind::Steward)
        );
        assert_eq!(proxy_audience(&known, &user("@two:h")), None);
    }

    /// R94A-01: a relay is one of the person's own messages since the
    /// question — their first, or the one quoted — and nothing before they
    /// spoke, nothing they did not say.
    #[test]
    fn a_relay_is_only_the_persons_own_message() {
        let said = vec!["what do you mean?".to_owned(), "2".to_owned()];
        assert_eq!(relayed(&[], None), Err(NOT_ANSWERED_YET));
        assert_eq!(relayed(&[], Some("1")), Err(NOT_ANSWERED_YET));
        assert_eq!(relayed(&said, None), Ok("what do you mean?"));
        assert_eq!(relayed(&said, Some(" 2 ")), Ok("2"));
        assert_eq!(relayed(&said, Some("1")), Err(NOT_THEIR_WORDS));
        assert_eq!(
            relayed(&said, Some("2, and the key is x")),
            Err(NOT_THEIR_WORDS)
        );
    }
}
