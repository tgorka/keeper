//! Whether an agent's host took a person's message (story 102.4, R465).
//!
//! In an agent session room a read receipt says nothing about the agent: no
//! host sends one, and matrix-sdk-ui counts the agent's own later events —
//! its claim renewed every minute — as it having read up to the newest
//! event before them, so every message looked "Read" within a minute,
//! taken or dropped. What the room does say is the turn: a host that takes
//! a person's message answers it with an anchor naming it
//! (`dev.keeper.agent.turn.question`, which hosts before 0.10.4 send too),
//! and a host that cannot take it replies to it with a
//! notice. A message with neither, while its host keeps writing into the
//! room and its newest status says it is idle or done, was not taken; a
//! host whose status is busy, absent or unreadable is never read as having
//! dropped it.
//!
//! [`turn_marks`] folds the loaded timeline into that, keyed by each own
//! message's render key; the timeline producer draws it in place of the
//! agent's receipt.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::agents::claim::RENEW_EVERY;
use crate::agents::events::{RunState, TURN};

/// How long after a person's message — and after its host's newest status —
/// the host must have written into the room with no answer to it before the
/// message reads as not taken: two claim renewals, so a host that queued it
/// behind a turn, or that is taking its file in, is never said to have
/// dropped it.
pub const NOT_TAKEN_AFTER_MS: u64 = 2 * RENEW_EVERY.as_millis() as u64;

/// What an own message in an agent session room shows of its turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum TurnMarkVm {
    /// An anchor of the agent names it: its host took it.
    Taken,
    /// Its host wrote into the room well after it, is not busy, and
    /// neither answered it nor said why: the sentence says so.
    NotTaken { sentence: String },
}

/// One loaded timeline message as the fold reads it.
#[derive(Debug, Clone, Copy)]
pub struct MarkInput<'a> {
    /// Its render key.
    pub key: &'a str,
    /// Its event id; `None` for a local echo not yet sent.
    pub event: Option<&'a str>,
    /// Whether this account sent it.
    pub own: bool,
    /// Its origin time, ms.
    pub ts: u64,
    /// Whether its sender holds an agent's power in the room.
    pub agent: bool,
    /// Its original event, when the SDK kept it.
    pub json: Option<&'a str>,
}

/// What the room says of its host besides its messages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostWrites {
    /// The newest origin time (ms) of any event an agent of the room sent —
    /// a claim renewal, a status, a message; `None` before one.
    pub last_write: Option<u64>,
    /// What its newest status says.
    pub run: HostRun,
}

/// What a host's newest status says of its run (R489/06): unknown — none
/// yet, or one this device cannot read — is its own state, never idle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HostRun {
    /// No status yet.
    #[default]
    NoStatus,
    /// The newest status cannot be read here — a newer host's, most likely.
    Unreadable,
    /// The newest status's run and its time (ms).
    Read(RunState, u64),
}

/// The sentence for a message `agent`'s host did not take.
pub fn not_taken(agent: &str) -> String {
    format!("{agent}'s host did not take this message — it may be older than this kind of message.")
}

/// The turn state of each own message among `items` (loaded, any order),
/// keyed by render key: [`TurnMarkVm::Taken`] when an agent's anchor names
/// it; nothing when an agent replied to it (its notice says why) or while
/// neither is known; [`TurnMarkVm::NotTaken`], with `agent`'s name, only on
/// positive evidence: the host's newest status is `idle` or `done`, and
/// `host` wrote at least [`NOT_TAKEN_AFTER_MS`] after both the message and
/// that status. A busy host (`running`, `blocked`, `waiting`) may have
/// queued it; one with no status, or one this device cannot read, is not
/// known to be anything (R489/06).
pub fn turn_marks(
    items: &[MarkInput<'_>],
    host: HostWrites,
    agent: &str,
) -> HashMap<String, TurnMarkVm> {
    let mut answered = HashSet::new();
    let mut replied = HashSet::new();
    for json in items
        .iter()
        .filter(|item| item.agent)
        .filter_map(|item| item.json)
    {
        let Ok(event) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        let content = &event["content"];
        if let Some(question) = content[TURN]["question"].as_str() {
            answered.insert(question.to_owned());
        }
        if let Some(about) = content["m.relates_to"]["m.in_reply_to"]["event_id"].as_str() {
            replied.insert(about.to_owned());
        }
    }
    // When the host said it was settled — idle or done — and nothing else.
    let settled_at = match host.run {
        HostRun::Read(RunState::Idle | RunState::Done, at) => Some(at),
        HostRun::Read(..) | HostRun::Unreadable | HostRun::NoStatus => None,
    };
    let mut marks = HashMap::new();
    for item in items {
        let (true, Some(event)) = (item.own, item.event) else {
            continue;
        };
        if answered.contains(event) {
            marks.insert(item.key.to_owned(), TurnMarkVm::Taken);
        } else if !replied.contains(event)
            && settled_at.is_some_and(|since| {
                host.last_write.is_some_and(|wrote| {
                    wrote >= item.ts.max(since).saturating_add(NOT_TAKEN_AFTER_MS)
                })
            })
        {
            marks.insert(
                item.key.to_owned(),
                TurnMarkVm::NotTaken {
                    sentence: not_taken(agent),
                },
            );
        }
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const MIN: u64 = 60_000;

    fn person<'a>(key: &'a str, event: &'a str, ts: u64) -> MarkInput<'a> {
        MarkInput {
            key,
            event: Some(event),
            own: true,
            ts,
            agent: false,
            json: None,
        }
    }

    fn agent(json: &str, ts: u64) -> MarkInput<'_> {
        MarkInput {
            key: "agent",
            event: Some("$agent"),
            own: false,
            ts,
            agent: true,
            json: Some(json),
        }
    }

    fn idle_since(at: u64, last_write: u64) -> HostWrites {
        HostWrites {
            last_write: Some(last_write),
            run: HostRun::Read(RunState::Idle, at),
        }
    }

    /// R465: an anchor naming the message takes it, whatever the host did
    /// after; a message no anchor names and no notice answers reads as not
    /// taken only once the idle host wrote two renewals after it.
    #[test]
    fn a_message_is_taken_by_its_anchor_and_not_taken_after_a_quiet_host() {
        let anchor = json!({"content": {"body": "…", TURN: {
            "session": "s", "line": "l", "question": "$asked"}}})
        .to_string();
        let items = [
            person("asked", "$asked", 0),
            agent(&anchor, 1_000),
            person("dropped", "$dropped", 2 * MIN),
        ];
        let quiet = idle_since(1_000, 2 * MIN + NOT_TAKEN_AFTER_MS);
        let marks = turn_marks(&items, quiet, "Nixi");
        assert_eq!(marks.get("asked"), Some(&TurnMarkVm::Taken));
        assert_eq!(
            marks.get("dropped"),
            Some(&TurnMarkVm::NotTaken {
                sentence: not_taken("Nixi")
            })
        );

        // A write short of two renewals after the message says nothing yet.
        let early = turn_marks(
            &items,
            idle_since(1_000, 2 * MIN + NOT_TAKEN_AFTER_MS - 1),
            "Nixi",
        );
        assert_eq!(early.get("dropped"), None);
        assert_eq!(early.get("asked"), Some(&TurnMarkVm::Taken));
    }

    /// A busy host — running, blocked on an approval, or waited for — may
    /// have queued the message; a host that went idle only now is given two
    /// renewals from that status; no write at all says nothing.
    #[test]
    fn a_busy_or_silent_host_never_reads_as_dropping() {
        let items = [person("p", "$p", 0)];
        for run in [RunState::Running, RunState::Blocked, RunState::Waiting] {
            let host = HostWrites {
                last_write: Some(10 * MIN),
                run: HostRun::Read(run, 0),
            };
            assert!(turn_marks(&items, host, "Nixi").is_empty(), "{run:?}");
        }
        assert!(turn_marks(&items, idle_since(9 * MIN, 10 * MIN), "Nixi").is_empty());
        let done = HostWrites {
            last_write: Some(10 * MIN),
            run: HostRun::Read(RunState::Done, MIN),
        };
        assert!(turn_marks(&items, done, "Nixi").contains_key("p"));
        assert!(turn_marks(&items, HostWrites::default(), "Nixi").is_empty());
    }

    /// R489/06: a host that keeps renewing its claim long after the message,
    /// with no anchor for it, is still not said to have dropped it while its
    /// newest status is unknown — none yet, or one this device cannot read,
    /// as a newer host's: only an idle or done status is evidence.
    #[test]
    fn an_unknown_status_is_never_read_as_idle() {
        let items = [person("p", "$p", 0)];
        for run in [HostRun::Unreadable, HostRun::NoStatus] {
            let host = HostWrites {
                last_write: Some(60 * MIN),
                run,
            };
            assert!(turn_marks(&items, host, "Nixi").is_empty(), "{run:?}");
        }
    }

    /// A host's notice replying to the message (it could not decrypt it,
    /// or its turn could not open) says why itself: no mark. An anchor from
    /// a sender without an agent's power, and another person's message,
    /// change nothing.
    #[test]
    fn a_hosts_reply_speaks_for_the_message() {
        let notice = json!({"content": {"body": "Send it again.",
            "m.relates_to": {"m.in_reply_to": {"event_id": "$p"}}}})
        .to_string();
        let quiet = idle_since(0, 10 * MIN);
        let replied = [person("p", "$p", 0), agent(&notice, MIN)];
        assert!(turn_marks(&replied, quiet, "Nixi").is_empty());

        let anchor =
            json!({"content": {TURN: {"session": "s", "line": "l", "question": "$p"}}}).to_string();
        let forged = MarkInput {
            agent: false,
            ..agent(&anchor, MIN)
        };
        let theirs = MarkInput {
            own: false,
            ..person("q", "$q", 0)
        };
        let marks = turn_marks(&[person("p", "$p", 0), forged, theirs], quiet, "Nixi");
        assert!(matches!(marks.get("p"), Some(TurnMarkVm::NotTaken { .. })));
        assert_eq!(marks.get("q"), None);
    }
}
