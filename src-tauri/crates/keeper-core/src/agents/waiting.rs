//! The questions waiting for this person (Story 101.3, R418, R430, R431).
//!
//! An agent's `ask_human` is an ordinary message in its session's room
//! carrying `dev.keeper.agent.ask` (story 94.2): it names the person asked
//! (`to`) and the proxy that relays it (`via`). The proxy's host puts it to
//! the person in the session [`ask_target`] names — the proxy's own `main` or
//! `conversation` that delegated into the asking room, else its `main` DM —
//! and relays the person's answer there into the asking room as
//! `dev.keeper.agent.answer` naming the ask (R100).
//!
//! So a question waits for this person, as far as this device can read,
//! while a room it reads holds the ask — sealed by its sender's device, sent
//! by a member at an agent's power there, naming this person and this proxy —
//! and no answer the proxy relayed for it ([`relayed_answer`], the host's own
//! rule). It is drawn above the composer of the room the host puts it to,
//! and nowhere else; choosing an option sends the choice as the person's own
//! message in that room, which is what typing it would send. Nothing here is
//! a new protocol.
//!
//! What this device can know is bounded (R431): the rooms it reads, each read
//! back at most [`ASK_READ_BACK`] events through the room's event cache. An
//! ask in a room it does not read is not here — the room it was asked in
//! still shows it.

use std::time::Duration;

use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use matrix_sdk::Room;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::ask::{ask_target, choice_of, enveloped_ask, relayed_answer, AskContent};
use crate::agents::delegation::read_brief;
use crate::agents::room::sealed_by_sender;
use crate::agents::session::SessionKind;

/// The most events of one room read back for its asks and its brief: a
/// room whose loaded events are fewer is paginated back through its event
/// cache — the store first, the homeserver at a gap — until it holds this
/// many or its start.
pub const ASK_READ_BACK: u16 = 300;

/// How long one room's read-back may take before the room is counted as
/// read in part.
pub const ASK_READ_BACK_WAIT: Duration = Duration::from_secs(5);

/// What the panel says when a room an ask may wait in could not be read
/// whole, or an ask's room could not be placed (R381).
pub const ASKS_PARTIAL: &str = "keeper could not read every room a question for you may wait in, so one may be missing here. It still shows in the room it was asked in.";

/// What the panel says when the questions could not be read at all (R381).
pub const ASKS_UNAVAILABLE: &str =
    "keeper could not read the questions waiting for you here. They still show in the rooms they were asked in.";

/// One event of a room, as the fold reads it: the whole event as JSON (type,
/// sender, content, origin_server_ts) and whether its sender's device sealed
/// it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub value: Value,
    pub sealed: bool,
}

/// An ask that waits for its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingAsk {
    pub ask: AskContent,
    /// The agent that asked.
    pub asker: OwnedUserId,
    /// When it was asked, ms since the Unix epoch.
    pub at: u64,
}

/// One room's asks for this person, folded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoomAsks {
    pub waiting: Vec<WaitingAsk>,
    /// The room its brief says delegated into it, when the proxy sealed and
    /// sent that brief as its own delegation: the session the host watches
    /// the room for.
    pub delegated_from: Option<OwnedRoomId>,
}

/// `events`, one room's in its order, folded into the asks waiting for `me`
/// through the proxy `via`: an ask counts when its sender's device sealed it
/// and `agent` says its sender holds an agent's power in the room; only the
/// answer [`relayed_answer`] takes ends its wait. The room's brief counts
/// when `via` sealed and sent it as its own delegation.
pub fn fold(
    events: impl IntoIterator<Item = Seen>,
    me: &UserId,
    via: &UserId,
    agent: &dyn Fn(&UserId) -> bool,
) -> RoomAsks {
    let mut room = RoomAsks::default();
    for seen in events {
        let content = &seen.value["content"];
        let Some(event_type) = seen.value["type"].as_str() else {
            continue;
        };
        let Some(sender) = seen.value["sender"]
            .as_str()
            .and_then(|sender| UserId::parse(sender).ok())
        else {
            continue;
        };
        let waiting = &room.waiting;
        if let Some(answer) = relayed_answer(event_type, content, seen.sealed, &sender, |id| {
            waiting
                .iter()
                .find(|open| open.ask.id == id)
                .map(|open| &*open.ask.via)
        }) {
            room.waiting.retain(|open| open.ask.id != answer.id);
            continue;
        }
        if room.delegated_from.is_none() && seen.sealed && sender == via {
            if let Some(brief) = read_brief(content).filter(|brief| brief.from.agent == via) {
                room.delegated_from = Some(brief.from.room);
                continue;
            }
        }
        let Some(ask) = enveloped_ask(event_type, content) else {
            continue;
        };
        if !seen.sealed
            || !agent(&sender)
            || ask.to != me
            || ask.via != via
            || room.waiting.iter().any(|open| open.ask.id == ask.id)
        {
            continue;
        }
        room.waiting.push(WaitingAsk {
            ask,
            asker: sender,
            at: seen.value["origin_server_ts"].as_u64().unwrap_or(0),
        });
    }
    room
}

/// Where a room's waiting asks are put to the person ([`ask_target`]): the
/// room that delegated into it when it is one of the proxy's own rooms
/// (`proxy_kind` names its kind), else `main`. `None` — not placed — when
/// the room's kind is unread, when it is a delegation or a workflow's run
/// whose brief keeper did not read and could not read back to its start
/// (`read_whole`), or when the proxy's DM is not known: a guess would put the
/// question where its answer is not taken.
pub fn answered_in(
    kind: Option<SessionKind>,
    asks: &RoomAsks,
    read_whole: bool,
    proxy_kind: &dyn Fn(&RoomId) -> Option<SessionKind>,
    main: Option<&RoomId>,
) -> Option<OwnedRoomId> {
    let delegated = matches!(kind, Some(SessionKind::Delegated | SessionKind::Workflow));
    if kind.is_none() || (delegated && asks.delegated_from.is_none() && !read_whole) {
        return None;
    }
    let parent = asks
        .delegated_from
        .as_ref()
        .and_then(|room| proxy_kind(room).map(|kind| (room.clone(), kind)));
    ask_target(parent, main.map(ToOwned::to_owned))
}

/// One message of the person's in the room an ask is answered in: in its
/// timeline (`held: false`, `at` its server time), still in the send queue
/// (`held: false`, `at` when it was queued) or held for Undo-Send (`held:
/// true`, `at` when it was held). A send that failed is not one. Every one is
/// dated: an undated message cannot be shown to come after the ask (R447).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mine {
    pub body: String,
    pub at: u64,
    pub held: bool,
}

/// Where a waiting ask's answer stands in the room it is answered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AskState {
    /// Nothing of the person's answers it yet: its options are offered.
    Open,
    /// An answer is held for Undo-Send: Undo puts the question back.
    Held,
    /// The person answered; the proxy has not relayed it yet.
    Answered,
}

/// Where `ask`'s answer stands, by the person's messages `mine` in the room
/// it is answered in: one sent, queued or held at or after the ask that
/// answers it — picks one of its choices ([`choice_of`]), or says anything
/// for a free answer — answered it, or holds it while it is held for
/// Undo-Send. A held answer is no answer: undone or failed, it leaves the
/// question open. A message written before the question answers nothing
/// (R447), however long it waited in the queue.
pub fn answer_state(ask: &WaitingAsk, mine: &[Mine]) -> AskState {
    let answers = |message: &&Mine| {
        message.at >= ask.at
            && if ask.ask.choices.is_empty() {
                !message.body.trim().is_empty()
            } else {
                choice_of(&message.body, &ask.ask.choices).is_some()
            }
    };
    let mut state = AskState::Open;
    for message in mine.iter().filter(answers) {
        if !message.held {
            return AskState::Answered;
        }
        state = AskState::Held;
    }
    state
}

/// How far back one room's events were read ([`read_room`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// To the room's start: nothing older exists.
    Start,
    /// To the bound, [`ASK_READ_BACK`] events, as R431 reads every room:
    /// what lies behind it is not looked for.
    Bound,
    /// Less than either: a read-back failed or took longer than
    /// [`ASK_READ_BACK_WAIT`], so something pending may lie behind what was
    /// read (R381, R453). Never a complete reading.
    Part,
}

/// One room's events as the fold reads them, and how far back they reach:
/// the loaded events, read back through the room's event cache to
/// [`ASK_READ_BACK`] when fewer are loaded. A read-back that fails or takes
/// longer than [`ASK_READ_BACK_WAIT`] leaves what was loaded, read in part
/// ([`Reach::Part`]; a WARN says so); only a room whose cache cannot be read
/// at all is an error.
pub async fn read_room(room: &Room) -> Result<(Vec<Seen>, Reach), String> {
    let (cache, _handles) = room.event_cache().await.map_err(|e| e.to_string())?;
    let mut events = cache.events().await.map_err(|e| e.to_string())?;
    let mut reach = Reach::Bound;
    let loaded = u16::try_from(events.len()).unwrap_or(u16::MAX);
    if loaded < ASK_READ_BACK {
        match tokio::time::timeout(
            ASK_READ_BACK_WAIT,
            cache
                .pagination()
                .run_backwards_until(ASK_READ_BACK - loaded),
        )
        .await
        {
            Ok(Ok(outcome)) => {
                if outcome.reached_start {
                    reach = Reach::Start;
                }
                events = cache.events().await.map_err(|e| e.to_string())?;
            }
            Ok(Err(error)) => {
                tracing::warn!(room = %room.room_id(), %error, "agents: a room's asks could not be read back; read in part");
                reach = Reach::Part;
            }
            Err(_) => {
                tracing::warn!(room = %room.room_id(), "agents: a room's asks took too long to read back; read in part");
                reach = Reach::Part;
            }
        }
    }
    let seen = events
        .iter()
        .filter_map(|event| {
            Some(Seen {
                value: event.raw().deserialize_as::<Value>().ok()?,
                sealed: sealed_by_sender(event.encryption_info().map(|info| &**info)),
            })
        })
        .collect();
    Ok((seen, reach))
}

/// A question waiting for this person, as the decisions panel draws it
/// (Story 101.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentAskVm {
    /// The ask's id.
    pub id: String,
    /// The room it was asked in.
    pub room_id: String,
    /// Who asked, by the name the asking room shows.
    pub asked_by: String,
    /// The question, as the agent wrote it.
    pub question: String,
    /// The answers it offers, in order; choosing one sends it as the
    /// person's message. Empty for a free answer, typed in the composer.
    pub choices: Vec<String>,
    /// The answer an unattended run takes, when the question names one.
    pub default: Option<String>,
    /// When it was asked, ms since the Unix epoch.
    #[ts(type = "number")]
    pub asked_at: u64,
    /// Where its answer stands here.
    pub state: AskState,
}

/// The questions answered in one room, and what keeper could not read of
/// them (R381, R431).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentAsksVm {
    /// Oldest first.
    pub asks: Vec<AgentAskVm>,
    /// The sentence the panel says beside them when keeper read them in
    /// part ([`ASKS_PARTIAL`]) or not at all ([`ASKS_UNAVAILABLE`]).
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::agents::ask::{answer_content, ask_content};
    use crate::agents::delegation::{brief_content, DelegateContent, DelegateFrom, DelegateLimits};
    use crate::agents::events::CONTENT_VERSION;
    use crate::agents::label::Label;

    const ME: &str = "@tgorka:example.org";
    const NIXI: &str = "@nixi:example.org";
    const TOLA: &str = "@tola-grey:example.org";
    const ID: &str = "01J8ASKAAAAAAAAAAAAAAAAAAA";
    const OTHER: &str = "01J8ASKBBBBBBBBBBBBBBBBBBB";
    const DM: &str = "!dm:example.org";
    const CONVERSATION: &str = "!test:example.org";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn room(id: &str) -> OwnedRoomId {
        OwnedRoomId::try_from(id).expect("room")
    }

    fn ask(id: &str, to: &str, via: &str) -> AskContent {
        AskContent {
            v: CONTENT_VERSION,
            id: id.to_owned(),
            question: "Which trip?".to_owned(),
            choices: vec!["Tatry".to_owned(), "Bieszczady".to_owned()],
            default: Some("Tatry".to_owned()),
            to: user(to),
            via: user(via),
            label: Label::top(),
        }
    }

    fn message(sender: &str, ts: u64, content: Value, sealed: bool) -> Seen {
        Seen {
            value: json!({
                "type": "m.room.message",
                "sender": sender,
                "origin_server_ts": ts,
                "content": content,
            }),
            sealed,
        }
    }

    fn brief(from_agent: &str, from_room: &str) -> Value {
        brief_content(&DelegateContent {
            v: CONTENT_VERSION,
            id: "01J8BR1EFAAAAAAAAAAAAAAAAA".to_owned(),
            from: DelegateFrom {
                agent: user(from_agent),
                drive: "tgdrive".to_owned(),
                session: "sessions/nixi/test".to_owned(),
                room: room(from_room),
            },
            to: user(TOLA),
            brief: "Plan the trip.".to_owned(),
            drives: vec!["tgdrive".to_owned()],
            label: Label::top(),
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 4,
                tokens: 1000,
            },
            card: None,
            dispatch_chain: Vec::new(),
        })
    }

    fn agents(user: &UserId) -> bool {
        user.as_str() == TOLA
    }

    fn ids(room: &RoomAsks) -> Vec<&str> {
        room.waiting
            .iter()
            .map(|open| open.ask.id.as_str())
            .collect()
    }

    fn fold_one(events: Vec<Seen>) -> RoomAsks {
        fold(events, &user(ME), &user(NIXI), &agents)
    }

    /// An ask for this person through this proxy waits until its proxy's
    /// answer names it; one for someone else, through another proxy,
    /// unsealed or from a member without an agent's power never waits.
    #[test]
    fn only_a_sealed_agents_ask_for_me_through_this_proxy_waits_until_answered() {
        let asked = message(TOLA, 10, ask_content(&ask(ID, ME, NIXI)), true);
        let waiting = fold_one(vec![asked.clone()]);
        assert_eq!(ids(&waiting), vec![ID]);
        assert_eq!(waiting.waiting[0].asker, user(TOLA));
        assert_eq!(waiting.waiting[0].at, 10);

        let answered = message(NIXI, 20, answer_content("Tatry", ID), true);
        assert!(fold_one(vec![asked.clone(), answered]).waiting.is_empty());

        for (case, seen) in [
            (
                "for someone else",
                message(
                    TOLA,
                    10,
                    ask_content(&ask(OTHER, "@marta:example.org", NIXI)),
                    true,
                ),
            ),
            (
                "through another proxy",
                message(
                    TOLA,
                    10,
                    ask_content(&ask(OTHER, ME, "@iris:example.org")),
                    true,
                ),
            ),
            (
                "unsealed",
                message(TOLA, 10, ask_content(&ask(OTHER, ME, NIXI)), false),
            ),
            (
                "below an agent's power",
                message(
                    "@marta:example.org",
                    10,
                    ask_content(&ask(OTHER, ME, NIXI)),
                    true,
                ),
            ),
        ] {
            assert!(fold_one(vec![seen]).waiting.is_empty(), "{case}");
        }
    }

    /// The same ask seen twice waits once; an answer to another ask leaves
    /// it waiting.
    #[test]
    fn an_ask_waits_once_and_only_its_own_answer_ends_it() {
        let asked = message(TOLA, 10, ask_content(&ask(ID, ME, NIXI)), true);
        let other = message(NIXI, 20, answer_content("Tatry", OTHER), true);
        assert_eq!(ids(&fold_one(vec![asked.clone(), asked, other])), vec![ID]);
    }

    /// R430 03: only the ask's proxy, sealed and unedited, ends the wait — a
    /// marker from anyone else, in clear or in an edit leaves the question.
    #[test]
    fn an_answer_ends_the_wait_only_from_the_asks_proxy_sealed_and_unedited() {
        let asked = message(TOLA, 10, ask_content(&ask(ID, ME, NIXI)), true);
        let mut edit = answer_content("Tatry", ID);
        edit["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$a"});
        for (case, answer) in [
            (
                "wrong sender",
                message(TOLA, 20, answer_content("Tatry", ID), true),
            ),
            (
                "unsealed",
                message(NIXI, 20, answer_content("Tatry", ID), false),
            ),
            ("an edit carrying it", message(NIXI, 20, edit, true)),
        ] {
            assert_eq!(
                ids(&fold_one(vec![asked.clone(), answer])),
                vec![ID],
                "{case}"
            );
        }
        let valid = message(NIXI, 20, answer_content("Tatry", ID), true);
        assert!(fold_one(vec![asked, valid]).waiting.is_empty());
    }

    /// R430 02: an ask in a room the proxy delegated into from a named
    /// conversation is answered in that conversation; from its main DM, in
    /// the DM; with no brief of the proxy's own, in the DM. A delegation
    /// whose brief keeper could not read back to the room's start is not
    /// placed.
    #[test]
    fn an_ask_is_answered_where_the_host_puts_it() {
        let asked = message(TOLA, 10, ask_content(&ask(ID, ME, NIXI)), true);
        let proxy_kind = |room: &RoomId| match room.as_str() {
            DM => Some(SessionKind::Main),
            CONVERSATION => Some(SessionKind::Conversation),
            _ => None,
        };
        let dm = room(DM);
        let place = |events: Vec<Seen>, whole: bool| {
            answered_in(
                Some(SessionKind::Delegated),
                &fold_one(events),
                whole,
                &proxy_kind,
                Some(&dm),
            )
        };
        let from_test = message(NIXI, 1, brief(NIXI, CONVERSATION), true);
        assert_eq!(
            place(vec![from_test, asked.clone()], false),
            Some(room(CONVERSATION)),
            "/new test"
        );
        let from_dm = message(NIXI, 1, brief(NIXI, DM), true);
        assert_eq!(
            place(vec![from_dm, asked.clone()], false),
            Some(room(DM)),
            "main"
        );
        // Another agent's delegation, or one only claimed for the proxy,
        // is no session of the proxy's: the DM.
        let tolas = message(TOLA, 1, brief(TOLA, CONVERSATION), true);
        let claimed = message(TOLA, 1, brief(NIXI, CONVERSATION), true);
        let unsealed = message(NIXI, 1, brief(NIXI, CONVERSATION), false);
        let relayed = message(NIXI, 1, brief(TOLA, CONVERSATION), true);
        for (case, first) in [
            ("another agent's", tolas),
            ("claimed", claimed),
            ("unsealed", unsealed),
            ("the proxy's copy of another agent's", relayed),
        ] {
            assert_eq!(
                place(vec![first, asked.clone()], true),
                Some(room(DM)),
                "{case}"
            );
        }
        assert_eq!(place(vec![asked.clone()], false), None, "brief unread");
        assert_eq!(
            answered_in(
                Some(SessionKind::Scheduled),
                &fold_one(vec![asked]),
                false,
                &proxy_kind,
                Some(&dm)
            ),
            Some(room(DM)),
            "a scheduled run has no parent"
        );
    }

    /// R430 05: a held answer is no answer — undone, the question is open
    /// again; the person's message choosing an option answers it; one sent
    /// before the ask, or one picking no option, does not.
    #[test]
    fn a_held_answer_holds_the_question_and_only_a_sent_one_answers_it() {
        let waiting = WaitingAsk {
            ask: ask(ID, ME, NIXI),
            asker: user(TOLA),
            at: 100,
        };
        let mine = |body: &str, at: u64, held: bool| Mine {
            body: body.to_owned(),
            at,
            held,
        };
        assert_eq!(
            answer_state(&waiting, &[mine("Bieszczady", 150, true)]),
            AskState::Held
        );
        assert_eq!(answer_state(&waiting, &[]), AskState::Open, "undone");
        assert_eq!(
            answer_state(&waiting, &[mine("2", 150, false)]),
            AskState::Answered,
            "in the send queue"
        );
        assert_eq!(
            answer_state(
                &waiting,
                &[mine("Tatry", 150, true), mine("bieszczady", 120, false)]
            ),
            AskState::Answered
        );
        assert_eq!(
            answer_state(&waiting, &[mine("Tatry", 50, false)]),
            AskState::Open,
            "before the ask"
        );
        assert_eq!(
            answer_state(&waiting, &[mine("Zakopane?", 150, false)]),
            AskState::Open,
            "no option"
        );
    }

    /// R447 (R101C2-05): a message queued before the question was asked
    /// answers nothing, however long it waits in the send queue — a choice
    /// typed earlier, or any earlier text for a free answer; one queued after
    /// the ask still does.
    #[test]
    fn a_message_queued_before_the_question_does_not_answer_it() {
        let chosen = WaitingAsk {
            ask: ask(ID, ME, NIXI),
            asker: user(TOLA),
            at: 100,
        };
        let free = WaitingAsk {
            ask: AskContent {
                choices: Vec::new(),
                ..ask(ID, ME, NIXI)
            },
            asker: user(TOLA),
            at: 100,
        };
        let queued = |body: &str, at: u64| Mine {
            body: body.to_owned(),
            at,
            held: false,
        };
        for waiting in [&chosen, &free] {
            assert_eq!(
                answer_state(waiting, &[queued("1", 99)]),
                AskState::Open,
                "queued before the ask"
            );
            assert_eq!(
                answer_state(waiting, &[queued("1", 99), queued("Tatry", 101)]),
                AskState::Answered,
                "queued after it"
            );
        }
    }
}
