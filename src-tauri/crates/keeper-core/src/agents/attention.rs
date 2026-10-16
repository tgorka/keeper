//! An agent room's state in the list (story 101.6, R421): whether it needs
//! the person, failed, is working or finished since they last read it.
//!
//! One state per agent room, by precedence (the order t3code's and Paseo's
//! sidebars rank theirs in):
//!
//! 1. **needs you** — an approval card waits here that this person may
//!    decide ([`ApprovalFold::waits_for`]), or a question an agent asked the
//!    person waits for its answer *here*: in the room the host puts it to
//!    them — the proxy's conversation that delegated the asking session, else
//!    the proxy's DM ([`crate::agents::ask::ask_target`]) — read by 101.3's
//!    one projection ([`crate::agents::waiting`], R431): only the answer the
//!    host itself takes ends it ([`crate::agents::ask::relayed_answer`]: the
//!    ask's proxy, sealed, `m.text`, no edit), or the person's own answer sent
//!    there (R453);
//! 2. **unknown** — keeper could not read all it needs to say there is
//!    nothing for the person: a read-back of this room or of a room a
//!    question for them may wait in failed, a question could not be placed,
//!    the room's power levels or status were not read. An incomplete reading
//!    is never *nothing pending* (R381, R453);
//! 3. **failed** — the newest readable status says `blocked`;
//! 4. **working** — it says `running`;
//! 5. **done, unread** — it says `done` — what a host's turn that finished
//!    with nothing left waiting publishes as it ends (`StatusBoard::finish`,
//!    R437); `idle` (a seeded room, a stopped or failed turn, a session
//!    waiting on a delegation or an ask) is no *done* — and it was sent after
//!    the event the person's read marker stands on (their read receipt,
//!    public or private, or their own newest message — the SDK's
//!    `latest_active`). Reading the room moves the marker to its newest
//!    event, so the state clears there, on every device of theirs. A room
//!    the person never read nor wrote in has no marker and is never *done,
//!    unread*: a session they only watch finishing is not news to them
//!    (t3code: never visited counts as read);
//! 6. otherwise nothing.
//!
//! The room's own events are read as 101.3 reads an asking room
//! ([`crate::agents::waiting::read_room`]): back through the event cache to
//! [`crate::agents::waiting::ASK_READ_BACK`] events or the room's start, so
//! a card behind the chunk a restart restored still counts (R431, R453).
//!
//! A card's expiry changes the state with no event in the room, so a reading
//! names when to read the room again ([`Attention::recheck_at`]), and
//! [`RowAttention`] keeps, per row, what it said and when to look again
//! (R436), and which rows show questions asked in other rooms
//! ([`RowAttention::ask_rooms`]): those are read again when any agent room
//! changes.
//!
//! Rust decides it from the room's events; the rows, the notes dock and the
//! agent's DM draw the word it names and never derive it (R421).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::approval_card::{ApprovalFold, Senders};
use crate::agents::events::{
    RunState, APPROVAL_CONSUMED, APPROVAL_DECISION, APPROVAL_REQUEST, STATUS,
};
use crate::agents::room::AgentRoomState;
use crate::agents::waiting::{AgentAsksVm, AskState};

/// An agent room's state in the list; a room in none of them carries none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AgentAttentionVm {
    /// An approval or a question waits for this person.
    NeedsYou,
    /// keeper could not read all it needs to say nothing waits for this
    /// person.
    Unknown,
    /// The session is blocked.
    Failed,
    /// The session is running.
    Working,
    /// The session finished after the person last read the room.
    DoneUnread,
}

/// One event of the room, in the room's order, as JSON (decrypted), and
/// whether a device the SDK links to its sender sealed it.
pub struct RoomEvent<'a> {
    pub event: &'a Value,
    pub sealed: bool,
}

/// Who reads the room and whose word counts in it.
pub struct Reader<'a> {
    /// The person.
    pub own: &'a UserId,
    /// Whether a user holds an agent's power in the room now.
    pub agent: &'a dyn Fn(&UserId) -> bool,
    /// The session's own agents (R185): whose approval requests count.
    pub owners: &'a [OwnedUserId],
    /// The event id the person's read marker stands on; `None` when they
    /// never read nor wrote in the room.
    pub marker: Option<&'a str>,
    /// Whether the room's events were read back to its start or to the
    /// bound ([`crate::agents::waiting::Reach`]) and its power levels read:
    /// `false` when a card or a status may lie unread.
    pub whole: bool,
}

/// What the questions answered in a room add to its state (R453).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asks {
    /// A question waits for the person's answer here.
    pub waiting: bool,
    /// Every room a question for the person may wait in was read, and every
    /// question placed.
    pub whole: bool,
}

impl Asks {
    /// The questions 101.3's projection says are answered in a room
    /// ([`AgentAsksVm`]): one waits until the person's answer to it is sent
    /// — open, or held for Undo-Send, which gives the question back when
    /// undone — and the reading is whole when nothing is said beside it.
    pub fn of(asks: &AgentAsksVm) -> Asks {
        Asks {
            waiting: asks.asks.iter().any(|ask| ask.state != AskState::Answered),
            whole: asks.note.is_none(),
        }
    }
}

/// An agent room's state as one reading found it, and when a card's expiry
/// may change it without another event: the earliest expiry of a card still
/// open ([`ApprovalFold::next_expiry`]). `ask_room`: questions asked in other
/// rooms are answered here, so a change there may change this room's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Attention {
    pub state: Option<AgentAttentionVm>,
    pub recheck_at: Option<DateTime<Utc>>,
    pub ask_room: bool,
}

/// The state of an agent room whose events, in order, are `events`, as
/// `reader` reads it at `now`; `asks`: the questions answered here, `None`
/// for a room no question is answered in.
pub fn attention<'a>(
    events: impl IntoIterator<Item = RoomEvent<'a>>,
    reader: &Reader<'_>,
    asks: Option<Asks>,
    now: DateTime<Utc>,
) -> Attention {
    let owner = |user: &UserId| reader.owners.iter().any(|owner| owner == user);
    let senders = Senders {
        own: reader.own,
        agent: reader.agent,
        owner: &owner,
    };
    let mut status = AgentRoomState::new(reader.own.to_owned());
    let mut approvals = ApprovalFold::default();
    // `origin_server_ts` of the event the marker stands on: 0 when it stands
    // before every event read (older than the room's loaded history).
    let mut marker_at: Option<u64> = reader.marker.map(|_| 0);

    for RoomEvent { event, sealed } in events {
        if let (Some(marker), Some(id)) = (reader.marker, event["event_id"].as_str()) {
            if marker == id {
                marker_at = Some(event["origin_server_ts"].as_u64().unwrap_or(0));
            }
        }
        match event["type"].as_str().unwrap_or("") {
            STATUS => {
                status.apply(event, reader.agent);
            }
            APPROVAL_REQUEST | APPROVAL_DECISION | APPROVAL_CONSUMED => {
                approvals.apply(event, sealed, &senders);
            }
            _ => {}
        }
    }

    let recheck_at = approvals.next_expiry(now);
    let questions = asks.unwrap_or(Asks {
        waiting: false,
        whole: true,
    });
    let state = if questions.waiting || approvals.waits_for(reader.own, now) {
        Some(AgentAttentionVm::NeedsYou)
    } else if !(reader.whole && questions.whole) {
        Some(AgentAttentionVm::Unknown)
    } else {
        match status.run_at() {
            Some((RunState::Blocked, _)) => Some(AgentAttentionVm::Failed),
            Some((RunState::Running, _)) => Some(AgentAttentionVm::Working),
            Some((RunState::Done, at)) if marker_at.is_some_and(|read| at > read) => {
                Some(AgentAttentionVm::DoneUnread)
            }
            _ => None,
        }
    };
    Attention {
        state,
        recheck_at,
        ask_room: asks.is_some(),
    }
}

/// What each agent room's row last said of its state, and when each must be
/// read again because a card in it expires then (story 101.6, R436): the
/// inbox redraws a row only when a fresh reading changed what it said, and
/// reads a room again at its recheck time with no event having arrived.
#[derive(Debug, Default)]
pub struct RowAttention {
    rows: Mutex<Rows>,
}

#[derive(Debug, Default)]
struct Rows {
    said: HashMap<OwnedRoomId, Option<AgentAttentionVm>>,
    recheck: HashMap<OwnedRoomId, DateTime<Utc>>,
    ask_rooms: HashSet<OwnedRoomId>,
}

impl RowAttention {
    fn rows(&self) -> std::sync::MutexGuard<'_, Rows> {
        self.rows.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// `room`'s row says what `read` found from now on, and is read again at
    /// its recheck time, or never for a card's sake when it has none, and
    /// whenever another agent room changes when questions are answered in
    /// it. Whether that changed what the row said.
    pub fn note(&self, room: &RoomId, read: &Attention) -> bool {
        let mut rows = self.rows();
        match read.recheck_at {
            Some(at) => rows.recheck.insert(room.to_owned(), at),
            None => rows.recheck.remove(room),
        };
        if read.ask_room {
            rows.ask_rooms.insert(room.to_owned());
        } else {
            rows.ask_rooms.remove(room);
        }
        rows.said.insert(room.to_owned(), read.state) != Some(read.state)
    }

    /// The earliest time a room must be read again.
    pub fn next_recheck(&self) -> Option<DateTime<Utc>> {
        self.rows().recheck.values().min().copied()
    }

    /// The rooms due to be read again at `now`, each taken off the clock
    /// until its next reading names another time.
    pub fn due(&self, now: DateTime<Utc>) -> Vec<OwnedRoomId> {
        let mut rows = self.rows();
        let due: Vec<OwnedRoomId> = rows
            .recheck
            .iter()
            .filter(|(_, at)| **at <= now)
            .map(|(room, _)| room.clone())
            .collect();
        for room in &due {
            rows.recheck.remove(room);
        }
        due
    }

    /// The rooms whose rows show questions asked in other rooms: read again
    /// when an agent room changes (an ask, a relayed answer).
    pub fn ask_rooms(&self) -> Vec<OwnedRoomId> {
        self.rows().ask_rooms.iter().cloned().collect()
    }
}

/// The state of the agent room `room`: its events read back as 101.3 reads
/// an asking room ([`crate::agents::waiting::read_room`]), its power levels,
/// its session's own agents, the person's read marker as the SDK last
/// computed it, and `asks` — 101.3's projection of the questions answered
/// in it, `None` for a room none is answered in. A room whose events or
/// power levels cannot be read is logged at WARN and read as not whole.
pub async fn of_room(room: &matrix_sdk::Room, asks: Option<&AgentAsksVm>) -> Attention {
    use crate::agents::room::{holds_agent_power, session_agents};
    use crate::agents::waiting::{read_room, Reach};

    let levels = match room.power_levels().await {
        Ok(levels) => Some(levels),
        Err(error) => {
            tracing::warn!(room = %room.room_id(), %error, "agents: a room's power levels could not be read for its state");
            None
        }
    };
    let owners = session_agents(room).await;
    let (seen, reach) = match read_room(room).await {
        Ok(read) => read,
        Err(error) => {
            tracing::warn!(room = %room.room_id(), %error, "agents: a room's events could not be read for its state");
            (Vec::new(), Reach::Part)
        }
    };
    let marker = room
        .read_receipts()
        .latest_active
        .map(|receipt| receipt.event_id.to_string());
    let agent = |user: &UserId| holds_agent_power(levels.as_ref(), user);
    attention(
        seen.iter().map(|seen| RoomEvent {
            event: &seen.value,
            sealed: seen.sealed,
        }),
        &Reader {
            own: room.own_user_id(),
            agent: &agent,
            owners: &owners,
            marker: marker.as_deref(),
            whole: reach != Reach::Part && levels.is_some(),
        },
        asks.map(Asks::of),
        Utc::now(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Duration;
    use serde_json::json;

    use crate::agents::ask::{ask_content, AskContent};
    use crate::agents::events::{ANSWER, CONTENT_VERSION};
    use crate::agents::waiting::{answer_state, answered_in, fold, Mine, Seen};

    const ME: &str = "@tgorka:example.org";
    const NIXI: &str = "@nixi:example.org";
    const OTHER: &str = "@marta:example.org";
    const ASK_ID: &str = "01J9ZQ3W4K5M6N7P8Q9R0STVWX";

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-15T12:00:00Z")
            .expect("now")
            .with_timezone(&Utc)
    }

    fn status(id: &str, ts: u64, run: &str) -> Value {
        json!({
            "type": STATUS,
            "event_id": id,
            "sender": NIXI,
            "origin_server_ts": ts,
            "content": {
                "v": CONTENT_VERSION,
                "session": "01J9ZQ3W4K5M6N7P8Q9R0SESSN",
                "kind": "main",
                "title": "Nixi",
                "agent": NIXI,
                "host": "electra",
                "epoch": 1,
                "run": run,
            },
        })
    }

    fn message(id: &str, ts: u64, sender: &str, content: Value) -> Value {
        json!({
            "type": "m.room.message",
            "event_id": id,
            "sender": sender,
            "origin_server_ts": ts,
            "content": content,
        })
    }

    /// A T2 card for a drive write, its summary and digest made as a host
    /// makes them, so a device binds it (R185).
    fn approval(id: &str, ts: u64, approvers: &[&str], expires: DateTime<Utc>) -> Value {
        use crate::agents::approval::{binding_digest, summary_of};
        use crate::agents::tier::AgentTool;
        let args = json!({"profile": "tgdrive", "path": "notes/plan.md", "content": "hello"});
        let preconditions = json!({
            "files": [{"drive": "tgdrive", "path": "notes/plan.md",
                "landing": "notes/plan.md", "sha256": null}],
            "workspace": null, "screen": null, "max_staleness_s": null,
        });
        let record = "01J9ZQ3W4K5M6N7P8Q9R0APPRV";
        let session = "60-sessions/active/2026-10-15-chat";
        let digest = binding_digest(
            record,
            session,
            "nixi",
            "drive_write",
            &args,
            &Value::Null,
            "c0ffee",
            &preconditions,
        )
        .expect("digest");
        json!({
            "type": APPROVAL_REQUEST,
            "event_id": id,
            "sender": NIXI,
            "origin_server_ts": ts,
            "content": {
                "v": CONTENT_VERSION, "id": record, "session": session,
                "room": "!s:example.org", "agent": "nixi", "tier": 2,
                "summary": summary_of(AgentTool::DriveWrite, &args, &Value::Null),
                "action": {"tool": "drive_write", "args": args, "exec_binding": null},
                "checkpoint_sha256": "c0ffee",
                "preconditions": preconditions,
                "binding_digest": digest, "scopes": ["once"],
                "expires_at": expires.to_rfc3339(),
                "approvers": approvers, "dispatch_chain": [ME, NIXI],
            },
        })
    }

    fn reader_at(
        events: &[Value],
        marker: Option<&str>,
        whole: bool,
        asks: Option<Asks>,
        at: DateTime<Utc>,
    ) -> Attention {
        let own = UserId::parse(ME).expect("me");
        let agent = |user: &UserId| user.as_str() == NIXI;
        let owners = [OwnedUserId::try_from(NIXI).expect("nixi")];
        attention(
            events.iter().map(|event| RoomEvent {
                event,
                sealed: true,
            }),
            &Reader {
                own: &own,
                agent: &agent,
                owners: &owners,
                marker,
                whole,
            },
            asks,
            at,
        )
    }

    fn read_at(events: &[Value], marker: Option<&str>, at: DateTime<Utc>) -> Attention {
        reader_at(events, marker, true, None, at)
    }

    fn state(events: &[Value], marker: Option<&str>) -> Option<AgentAttentionVm> {
        read_at(events, marker, now()).state
    }

    const TOLA: &str = "@tola-grey:example.org";
    const AMELIA: &str = "@amelia:example.org";
    const DM: &str = "!dm:example.org";
    const CONVERSATION: &str = "!test:example.org";

    fn seen(sender: &str, ts: u64, content: Value) -> Seen {
        Seen {
            value: message(&format!("${ts}"), ts, sender, content),
            sealed: true,
        }
    }

    /// Tola's question for the person, through Nixi, in Tola's delegated
    /// session room.
    fn tola_asks(ts: u64) -> Seen {
        seen(
            TOLA,
            ts,
            ask_content(&AskContent {
                v: CONTENT_VERSION,
                id: ASK_ID.to_owned(),
                question: "Ship it?".to_owned(),
                choices: vec!["yes".to_owned(), "no".to_owned()],
                default: None,
                to: OwnedUserId::try_from(ME).expect("to"),
                via: OwnedUserId::try_from(NIXI).expect("via"),
                label: crate::agents::label::Label::top(),
            }),
        )
    }

    /// A message carrying the answer to that question, from `sender`.
    fn answer_from(sender: &str, ts: u64, msgtype: &str, edit: bool) -> Seen {
        let mut content = json!({
            "msgtype": msgtype, "body": "yes",
            ANSWER: {"v": CONTENT_VERSION, "id": ASK_ID},
        });
        if edit {
            content["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$1"});
        }
        seen(sender, ts, content)
    }

    /// Nixi's sealed brief delegating Tola's room from Nixi's conversation.
    fn brief_from_conversation(ts: u64) -> Seen {
        use crate::agents::delegation::{
            brief_content, DelegateContent, DelegateFrom, DelegateLimits,
        };
        seen(
            NIXI,
            ts,
            brief_content(&DelegateContent {
                v: CONTENT_VERSION,
                id: "01J8BR1EFAAAAAAAAAAAAAAAAA".to_owned(),
                from: DelegateFrom {
                    agent: OwnedUserId::try_from(NIXI).expect("nixi"),
                    drive: "tgdrive".to_owned(),
                    session: "sessions/nixi/test".to_owned(),
                    room: OwnedRoomId::try_from(CONVERSATION).expect("room"),
                },
                to: OwnedUserId::try_from(TOLA).expect("tola"),
                brief: "Plan the trip.".to_owned(),
                drives: vec!["tgdrive".to_owned()],
                label: crate::agents::label::Label::top(),
                hop: 1,
                limits: DelegateLimits {
                    rounds_per_exchange: 4,
                    tokens: 1000,
                },
                card: None,
                dispatch_chain: Vec::new(),
            }),
        )
    }

    /// The state of the proxy room `here` while Tola's delegated room holds
    /// `asking` and the person's messages in `here` are `mine`: the asks
    /// 101.3's projection folds from Tola's room (`waiting::fold`, its
    /// answer admission), places (`waiting::answered_in` → `ask_target`),
    /// and dates against `mine` (`waiting::answer_state`), as
    /// `account::asks_answered_in` does, then this room's own run.
    fn proxy_room(here: &str, asking: &[Seen], mine: &[Mine]) -> Option<AgentAttentionVm> {
        let me = UserId::parse(ME).expect("me");
        let via = UserId::parse(NIXI).expect("via");
        let agents = |user: &UserId| [TOLA, NIXI, AMELIA].contains(&user.as_str());
        let folded = fold(asking.iter().cloned(), &me, &via, &agents);
        let dm = RoomId::parse(DM).expect("dm");
        let proxy_kind = |room: &RoomId| {
            (room.as_str() == CONVERSATION)
                .then_some(crate::agents::session::SessionKind::Conversation)
        };
        let placed = answered_in(
            Some(crate::agents::session::SessionKind::Delegated),
            &folded,
            true,
            &proxy_kind,
            Some(&*dm),
        );
        let asks = AgentAsksVm {
            asks: folded
                .waiting
                .iter()
                .filter(|_| placed.as_deref().map(RoomId::as_str) == Some(here))
                .map(|open| crate::agents::waiting::AgentAskVm {
                    id: open.ask.id.clone(),
                    room_id: "!tola:example.org".to_owned(),
                    asked_by: "Tola".to_owned(),
                    question: open.ask.question.clone(),
                    choices: open.ask.choices.clone(),
                    default: open.ask.default.clone(),
                    asked_at: open.at,
                    state: answer_state(open, mine),
                })
                .collect(),
            note: None,
        };
        reader_at(
            &[status("$s", 5, "idle")],
            Some("$s"),
            true,
            Some(Asks::of(&asks)),
            now(),
        )
        .state
    }

    #[test]
    fn a_runs_word_names_the_rows_state() {
        assert_eq!(
            state(&[status("$1", 10, "running")], Some("$1")),
            Some(AgentAttentionVm::Working)
        );
        assert_eq!(
            state(&[status("$1", 10, "blocked")], Some("$1")),
            Some(AgentAttentionVm::Failed)
        );
        assert_eq!(state(&[status("$1", 10, "idle")], Some("$1")), None);
        assert_eq!(state(&[status("$1", 10, "waiting")], Some("$1")), None);
        assert_eq!(state(&[], Some("$1")), None);
    }

    #[test]
    fn a_finish_is_unread_until_the_marker_passes_it() {
        let read_before = [
            message("$0", 5, ME, json!({"msgtype": "m.text", "body": "go"})),
            status("$1", 10, "running"),
            status("$2", 20, "done"),
        ];
        // The marker on the person's own message, before the finish.
        assert_eq!(
            state(&read_before, Some("$0")),
            Some(AgentAttentionVm::DoneUnread)
        );
        // Read: the marker on the finish itself, or past it.
        assert_eq!(state(&read_before, Some("$2")), None);
        let mut later = read_before.to_vec();
        later.push(message(
            "$3",
            30,
            NIXI,
            json!({"msgtype": "m.text", "body": "ok"}),
        ));
        assert_eq!(state(&later, Some("$3")), None);
        // A marker older than everything loaded stands before the finish.
        assert_eq!(
            state(&read_before, Some("$gone")),
            Some(AgentAttentionVm::DoneUnread)
        );
        // Never read nor written in: a finish there is not news.
        assert_eq!(state(&read_before, None), None);
    }

    #[test]
    fn an_approval_this_person_may_decide_needs_them_above_any_run() {
        let open = now() + Duration::hours(1);
        let events = [status("$1", 10, "running"), approval("$2", 11, &[ME], open)];
        assert_eq!(state(&events, Some("$2")), Some(AgentAttentionVm::NeedsYou));
        // Someone else's to decide: the run is what the row says.
        let theirs = [
            status("$1", 10, "running"),
            approval("$2", 11, &[OTHER], open),
        ];
        assert_eq!(state(&theirs, Some("$2")), Some(AgentAttentionVm::Working));
        // Expired: nobody waits on it any more.
        let expired = [
            status("$1", 10, "done"),
            approval("$2", 11, &[ME], now() - Duration::minutes(1)),
        ];
        assert_eq!(state(&expired, Some("$2")), None);
    }

    /// R436 (review 08): a card this person may decide expires with no event
    /// in the room. The reading names its expiry as when to look again; at
    /// that time the room is due, the same events read again say what the
    /// run says, and the row's state changed — so it is redrawn — while a
    /// reading that changes nothing is not. A reading that finds the card no
    /// longer open cancels the room's wakeup, and once no card is open,
    /// nothing is scheduled for the room.
    #[test]
    fn a_card_expiring_with_no_event_is_read_again_at_its_expiry() {
        let room = RoomId::parse("!nixi:example.org").expect("room");
        let expires = now() + Duration::minutes(5);
        let events = [
            status("$1", 10, "running"),
            approval("$2", 11, &[ME], expires),
        ];
        let rows = RowAttention::default();
        let before = read_at(&events, Some("$2"), now());
        assert_eq!(before.state, Some(AgentAttentionVm::NeedsYou));
        assert_eq!(before.recheck_at, Some(expires));
        assert!(rows.note(&room, &before), "a first reading is news");
        assert!(!rows.note(&room, &before), "the same reading is not");
        assert_eq!(rows.next_recheck(), Some(expires));
        assert!(rows.due(expires - Duration::seconds(1)).is_empty());

        // The next reading finds no open card (the card's room moved on):
        // the wakeup is cancelled; finding the card again sets it again.
        let closed = read_at(&[status("$1", 10, "running")], Some("$1"), now());
        assert!(rows.note(&room, &closed));
        assert_eq!(rows.next_recheck(), None);
        assert!(rows.note(&room, &before));
        assert_eq!(rows.next_recheck(), Some(expires));

        let due = rows.due(expires);
        assert_eq!(due, std::slice::from_ref(&room));
        let after = read_at(&events, Some("$2"), expires);
        assert_eq!(after.state, Some(AgentAttentionVm::Working));
        assert!(rows.note(&room, &after), "the expiry changed the row");
        assert_eq!(after.recheck_at, None);
        assert_eq!(rows.next_recheck(), None);
    }

    /// R453 (review 03): a question waiting in the proxy's DM ends only on
    /// the answer the asking session's host takes (101.3's
    /// `ask::relayed_answer`): sealed, `m.text`, no edit, from the ask's
    /// proxy. Another agent's answer, an edit carrying the answer and an
    /// answer of another kind leave the person needed.
    #[test]
    fn only_the_answer_the_host_takes_clears_needs_you() {
        let asked = tola_asks(10);
        assert_eq!(
            proxy_room(DM, std::slice::from_ref(&asked), &[]),
            Some(AgentAttentionVm::NeedsYou)
        );
        for (refused, why) in [
            (
                answer_from(AMELIA, 11, "m.text", false),
                "another agent's answer",
            ),
            (answer_from(NIXI, 11, "m.text", true), "an edit"),
            (answer_from(NIXI, 11, "m.notice", false), "not m.text"),
        ] {
            assert_eq!(
                proxy_room(DM, &[asked.clone(), refused], &[]),
                Some(AgentAttentionVm::NeedsYou),
                "{why} ends nothing"
            );
        }
        let mut unsealed = answer_from(NIXI, 11, "m.text", false);
        unsealed.sealed = false;
        assert_eq!(
            proxy_room(DM, &[asked.clone(), unsealed], &[]),
            Some(AgentAttentionVm::NeedsYou),
            "an unsealed answer ends nothing"
        );
        assert_eq!(
            proxy_room(DM, &[asked, answer_from(NIXI, 11, "m.text", false)], &[]),
            None,
            "the proxy's relay ends the wait"
        );
    }

    /// R453 (review 05): a delegated child's question, relayed through the
    /// proxy, makes the room the person answers in *Needs you* — the proxy's
    /// DM, or the proxy's conversation that delegated the child — and not
    /// the other. The person's sent answer there clears it; one held for
    /// Undo-Send does not, as undoing it gives the question back; the
    /// proxy's relay clears it as well.
    #[test]
    fn a_delegated_question_needs_the_person_where_they_answer_it() {
        let asked = tola_asks(10);
        assert_eq!(
            proxy_room(DM, std::slice::from_ref(&asked), &[]),
            Some(AgentAttentionVm::NeedsYou)
        );
        assert_eq!(
            proxy_room(CONVERSATION, std::slice::from_ref(&asked), &[]),
            None,
            "a question no conversation delegated is answered in the DM"
        );

        let from_conversation = [brief_from_conversation(5), asked.clone()];
        assert_eq!(
            proxy_room(CONVERSATION, &from_conversation, &[]),
            Some(AgentAttentionVm::NeedsYou)
        );
        assert_eq!(proxy_room(DM, &from_conversation, &[]), None);

        let mine = |held: bool| Mine {
            body: "yes".to_owned(),
            at: 12,
            held,
        };
        assert_eq!(
            proxy_room(DM, std::slice::from_ref(&asked), &[mine(true)]),
            Some(AgentAttentionVm::NeedsYou),
            "a held answer may still be undone"
        );
        assert_eq!(proxy_room(DM, &[asked], &[mine(false)]), None);
    }

    /// R453 (review 04): an incomplete reading is never *nothing pending*.
    /// A restart restores only a room's newest chunk; read back, the card
    /// behind it still needs the person, while the chunk alone — a newer
    /// status and nothing else — read as complete would say *Working*. When
    /// the read-back fails, or a room a question may wait in could not be
    /// read, the row says *Unknown*, never the run's word; what was found
    /// waiting still says *Needs you*.
    #[test]
    fn an_incomplete_reading_is_never_nothing_pending() {
        let open = now() + Duration::hours(1);
        let read_back = [
            status("$1", 10, "running"),
            approval("$2", 11, &[ME], open),
            status("$3", 30, "running"),
        ];
        let chunk = [status("$3", 30, "running")];
        let read = |events: &[Value], whole: bool, asks: Option<Asks>| {
            reader_at(events, Some("$3"), whole, asks, now()).state
        };
        assert_eq!(
            read(&read_back, true, None),
            Some(AgentAttentionVm::NeedsYou)
        );
        assert_eq!(
            read(&chunk, false, None),
            Some(AgentAttentionVm::Unknown),
            "the chunk read in part"
        );
        assert_eq!(
            read(&chunk, true, None),
            Some(AgentAttentionVm::Working),
            "the chunk read to its bound"
        );
        assert_eq!(
            read(&read_back, false, None),
            Some(AgentAttentionVm::NeedsYou),
            "what was found still waits"
        );

        let unread_asks = AgentAsksVm {
            asks: Vec::new(),
            note: Some(crate::agents::waiting::ASKS_PARTIAL.to_owned()),
        };
        assert_eq!(
            read(&chunk, true, Some(Asks::of(&unread_asks))),
            Some(AgentAttentionVm::Unknown),
            "a room a question may wait in was not read"
        );
        assert_eq!(
            read(&chunk, true, Some(Asks::of(&AgentAsksVm::default()))),
            Some(AgentAttentionVm::Working)
        );
    }

    /// R453 (review 05): a row that shows questions asked elsewhere is read
    /// again when another agent room changes; one that stops showing them
    /// is not.
    #[test]
    fn a_row_showing_questions_asked_elsewhere_is_read_again_on_their_change() {
        let room = RoomId::parse(DM).expect("room");
        let rows = RowAttention::default();
        let asks = Asks::of(&AgentAsksVm::default());
        let running = [status("$1", 10, "running")];
        let reading = reader_at(&running, Some("$1"), true, Some(asks), now());
        rows.note(&room, &reading);
        assert_eq!(rows.ask_rooms(), vec![room.clone()]);
        rows.note(&room, &read_at(&running, Some("$1"), now()));
        assert!(rows.ask_rooms().is_empty());
    }
}
