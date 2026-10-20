//! An approval parked behind a record port: the park and the resume of a
//! call that needs a person, for a host whose record is not the Matrix
//! room's (`approvals/<ulid>.json` and its `consumed` state event,
//! [`crate::approvals`]) — a row in a database, a test.
//!
//! The tier and the digest are D-33's, unchanged: the host classifies the
//! call with [`keeper_core::agents::tier::classify`] and composes what it
//! binds (a `run`'s [`crate::run::Prepared::exec_binding`], an MCP call's
//! listed binding); [`ApprovalRecord::new`] takes the digest over exactly
//! that, as it does for a record file.
//!
//! **Park.** [`park`] writes the record through the host's
//! [`ApprovalRecords`] and answers [`ToolOutcome::Parked`]: the loop returns
//! right after it and the turn ends. Nothing waits — no thread, no lock, no
//! socket is held for the decision.
//!
//! **Resume.** When the decision lands, or the record's time is up, the host
//! calls [`resume`] with the call composed again from what is there now.
//! A pending record past its time is expired; a denial and an expiry end the
//! call; an approved one is checked against a digest recomputed over the
//! fresh call — anything it relies on that changed is drift, and the call
//! ends unrun — then consumed through the port, which is a versioned update:
//! a second consumer loses, and only the winner runs it.

use chrono::{DateTime, Utc};
use keeper_core::agents::approval::{
    sha256_hex, ApprovalRecord, CallRef, Checkpoint, FloatAt, Parking, Preconditions,
};
use keeper_core::agents::label::Label;
use keeper_core::agents::session::SessionKind;
use keeper_core::agents::tier::Classification;
use keeper_core::bots::tools::ToolOutcome;
use serde_json::{json, Value};

use crate::approvals::{not_done, APPROVED_UNUSED, CHANGED, DENIED, EXPIRED, INTERRUPTED};

/// Where an approval stands, as its record says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Pending,
    Approved,
    Denied,
    Consumed,
    Expired,
}

impl State {
    /// The record's word.
    pub fn as_word(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Approved => "approved",
            State::Denied => "denied",
            State::Consumed => "consumed",
            State::Expired => "expired",
        }
    }

    /// The state `word` names, when it names one.
    pub fn from_word(word: &str) -> Option<State> {
        [
            State::Pending,
            State::Approved,
            State::Denied,
            State::Consumed,
            State::Expired,
        ]
        .into_iter()
        .find(|state| state.as_word() == word)
    }
}

/// An approval's record as the host reads it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    pub state: State,
    /// The digest it was asked with.
    pub binding_digest: String,
    pub expires_at: DateTime<Utc>,
}

/// How a consumption ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumed {
    /// This host consumed it: it runs the call, once.
    Won,
    /// Another consumer's update came first (or the record moved since it
    /// was read): this host does not run it. Why, in the port's words.
    Lost(String),
}

/// Where a host keeps its approval records.
pub trait ApprovalRecords: Send + Sync {
    /// Write `record` as a new pending approval that `approvers` may
    /// decide. `Err` when it was not written: the call is then refused.
    fn park(&self, record: &ApprovalRecord, approvers: &[String]) -> Result<(), String>;
    /// The record `id` as it stands, `None` when there is none.
    fn standing(&self, id: &str) -> Result<Option<Standing>, String>;
    /// `approved → consumed`, carrying the version of the record it read,
    /// answered only once the record's owner has taken or refused it.
    /// `Err` when that is not known yet.
    fn consume(&self, id: &str) -> Result<Consumed, String>;
    /// `pending → expired`, versioned as [`Self::consume`] is.
    fn expire(&self, id: &str) -> Result<(), String>;
}

/// A call that needs a person, as its host composed it.
#[derive(Debug, Clone)]
pub struct Call<'a> {
    /// The record's id: a ULID the host chose when the call parked.
    pub id: &'a str,
    pub session: &'a str,
    pub session_kind: SessionKind,
    pub agent: &'a str,
    /// What the record names as its drive; a host without one names its
    /// database.
    pub drive: &'a str,
    pub host: &'a str,
    pub call: CallRef,
    pub checkpoint: Checkpoint,
    pub args: &'a Value,
    pub exec_binding: Value,
    pub preconditions: Preconditions,
    pub classification: &'a Classification,
    pub label: &'a Label,
}

/// Where a call stood when it parked on a host that keeps no session log:
/// the turn and the call, in place of a log chunk and its line.
pub fn checkpoint(turn: &str, call: &str) -> Checkpoint {
    let bound = json!({ "turn": turn, "call": call });
    Checkpoint {
        chunk: turn.to_owned(),
        through: call.to_owned(),
        sha256: sha256_hex(bound.to_string().as_bytes()),
    }
}

/// The record of `call`, as [`ApprovalRecord::new`] makes it, written
/// `created_at`.
pub fn record(call: &Call<'_>, created_at: DateTime<Utc>) -> Result<ApprovalRecord, FloatAt> {
    ApprovalRecord::new(Parking {
        id: call.id,
        created_at,
        session: call.session,
        session_kind: call.session_kind,
        agent: call.agent,
        drive: call.drive,
        host: call.host,
        epoch: 0,
        call: call.call.clone(),
        dispatch_chain: Vec::new(),
        checkpoint: call.checkpoint.clone(),
        args: call.args,
        exec_binding: call.exec_binding.clone(),
        classification: call.classification,
        label: call.label,
        preconditions: call.preconditions.clone(),
    })
}

/// Park `call` for `approvers`: its record written, the call answered
/// [`ToolOutcome::Parked`]. With nobody to ask, or a record that cannot be
/// made or written, it is refused and nothing waits.
pub fn park(
    records: &dyn ApprovalRecords,
    call: &Call<'_>,
    approvers: &[String],
    now: DateTime<Utc>,
) -> ToolOutcome {
    let refused = |reason: String| ToolOutcome::Refused { reason };
    if approvers.is_empty() {
        return refused(crate::host::UNATTENDED_REFUSAL.to_owned());
    }
    let Ok(approval) = ulid::Ulid::from_string(call.id) else {
        return refused(crate::approvals::UNATTACHED.to_owned());
    };
    let record = match record(call, now) {
        Ok(record) => record,
        Err(float) => return refused(float.to_string()),
    };
    match records.park(&record, approvers) {
        Ok(()) => ToolOutcome::Parked { approval },
        Err(why) => {
            tracing::warn!(approval = %call.id, %why, "agents: an approval record was not written; the call is refused");
            refused(crate::approvals::UNATTACHED.to_owned())
        }
    }
}

/// What the host does next with a parked call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resume {
    /// Nothing yet: undecided until `until` at the latest.
    Wait { until: DateTime<Utc> },
    /// Consumed here: run exactly the call composed for this resume.
    Run,
    /// The call is over unrun, and the model is told this.
    Ended(String),
    /// Another consumer won: this host writes nothing for the call.
    Elsewhere(String),
}

/// Resume the call parked on record `id`, `fresh` composing it from what is
/// there now (`Err`: why it cannot be composed — drift too).
pub fn resume<'a>(
    records: &dyn ApprovalRecords,
    id: &str,
    fresh: impl FnOnce() -> Result<Call<'a>, String>,
    now: DateTime<Utc>,
) -> Resume {
    let standing = match records.standing(id) {
        Ok(Some(standing)) => standing,
        Ok(None) => return Resume::Ended(not_done("its approval record is gone")),
        Err(why) => {
            tracing::warn!(approval = %id, %why, "agents: an approval record could not be read; it is read again later");
            return Resume::Wait { until: now };
        }
    };
    match standing.state {
        State::Pending if now < standing.expires_at => Resume::Wait {
            until: standing.expires_at,
        },
        State::Pending => match records.expire(id) {
            Ok(()) => Resume::Ended(EXPIRED.to_owned()),
            // Decided in the same moment, or not answered yet: read again.
            Err(why) => {
                tracing::info!(approval = %id, %why, "agents: an expiry was not taken; the record is read again");
                Resume::Wait { until: now }
            }
        },
        State::Denied => Resume::Ended(DENIED.to_owned()),
        State::Expired => Resume::Ended(EXPIRED.to_owned()),
        // Spent before this resume, here or by another consumer: whether
        // it took effect is not known here, and it never runs again.
        State::Consumed => Resume::Ended(INTERRUPTED.to_owned()),
        State::Approved if now >= standing.expires_at => Resume::Ended(APPROVED_UNUSED.to_owned()),
        State::Approved => {
            let call = match fresh() {
                Ok(call) => call,
                Err(why) => return Resume::Ended(not_done(&why)),
            };
            let digest = record(&call, now).map(|record| record.binding_digest);
            if digest.as_deref() != Ok(standing.binding_digest.as_str()) {
                tracing::info!(approval = %id, "agents: an approved call changed since it was asked; it is not run");
                return Resume::Ended(not_done(CHANGED));
            }
            match records.consume(id) {
                Ok(Consumed::Won) => Resume::Run,
                Ok(Consumed::Lost(why)) => Resume::Elsewhere(why),
                Err(why) => {
                    tracing::warn!(approval = %id, %why, "agents: a consumption was not answered; the record is read again");
                    Resume::Wait { until: now }
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use keeper_core::agents::tier::{classify, AgentTool, CallFacts, Context};
    use std::sync::Mutex;

    /// Records held in memory; a consumption versioned by the state read.
    #[derive(Default)]
    struct Memory {
        rows: Mutex<Vec<(String, Standing, Vec<String>)>>,
        consumed: Mutex<usize>,
    }

    impl Memory {
        fn set(&self, id: &str, state: State) {
            let mut rows = self
                .rows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let row = rows.iter_mut().find(|(i, ..)| i == id).expect("record");
            row.1.state = state;
        }
    }

    impl ApprovalRecords for Memory {
        fn park(&self, record: &ApprovalRecord, approvers: &[String]) -> Result<(), String> {
            self.rows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((
                    record.id.clone(),
                    Standing {
                        state: State::Pending,
                        binding_digest: record.binding_digest.clone(),
                        expires_at: record.expires(),
                    },
                    approvers.to_vec(),
                ));
            Ok(())
        }
        fn standing(&self, id: &str) -> Result<Option<Standing>, String> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|(i, ..)| i == id)
                .map(|(_, s, _)| s.clone()))
        }
        fn consume(&self, id: &str) -> Result<Consumed, String> {
            let mut rows = self
                .rows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let row = rows.iter_mut().find(|(i, ..)| i == id).expect("record");
            if row.1.state != State::Approved {
                return Ok(Consumed::Lost("sync.version_conflict".to_owned()));
            }
            row.1.state = State::Consumed;
            *self
                .consumed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
            Ok(Consumed::Won)
        }
        fn expire(&self, id: &str) -> Result<(), String> {
            self.set(id, State::Expired);
            Ok(())
        }
    }

    const ID: &str = "01JA0000000000000000000000";

    fn classification() -> Classification {
        classify(
            AgentTool::Run,
            &CallFacts::default(),
            &Context {
                delegated: false,
                unattended: false,
                integrity: keeper_core::agents::label::Integrity::Owner,
                via_kvm: false,
                grant: None,
            },
        )
    }

    fn call<'a>(args: &'a Value, binding: Value, c: &'a Classification, l: &'a Label) -> Call<'a> {
        Call {
            id: ID,
            session: "s1",
            session_kind: SessionKind::Conversation,
            agent: "nixie",
            drive: "tgdrive",
            host: "electra",
            call: CallRef {
                line: "row-1".to_owned(),
                call_id: "call-1".to_owned(),
            },
            checkpoint: checkpoint("turn-1", "row-1"),
            args,
            exec_binding: binding,
            preconditions: Preconditions::default(),
            classification: c,
            label: l,
        }
    }

    fn parked(records: &Memory, args: &Value, c: &Classification, l: &Label, now: DateTime<Utc>) {
        let outcome = park(
            records,
            &call(args, json!({"exe_sha256": "a"}), c, l),
            &["tgorka".to_owned()],
            now,
        );
        assert!(matches!(outcome, ToolOutcome::Parked { .. }), "{outcome:?}");
    }

    #[test]
    fn a_parked_call_writes_a_pending_record_bound_to_its_digest() {
        let (records, args, c, l) = (
            Memory::default(),
            json!({"argv": ["true"]}),
            classification(),
            Label::top(),
        );
        let now = Utc::now();
        parked(&records, &args, &c, &l, now);
        let expected = record(&call(&args, json!({"exe_sha256": "a"}), &c, &l), now).unwrap();
        let (standing, approvers) = {
            let rows = records
                .rows
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (rows[0].1.clone(), rows[0].2.clone())
        };
        assert_eq!(standing.state, State::Pending);
        assert_eq!(standing.binding_digest, expected.binding_digest);
        assert_eq!(approvers, ["tgorka"]);
        assert_eq!(
            resume(&records, ID, || Err("unused".into()), now),
            Resume::Wait {
                until: standing.expires_at
            }
        );
    }

    #[test]
    fn nobody_to_ask_refuses_and_writes_nothing() {
        let (records, args, c, l) = (Memory::default(), json!({}), classification(), Label::top());
        let outcome = park(&records, &call(&args, Value::Null, &c, &l), &[], Utc::now());
        assert!(matches!(outcome, ToolOutcome::Refused { .. }));
        assert!(records
            .rows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty());
    }

    #[test]
    fn an_approved_call_runs_once_and_a_second_consumer_loses() {
        let (records, args, c, l) = (
            Memory::default(),
            json!({"argv": ["true"]}),
            classification(),
            Label::top(),
        );
        let now = Utc::now();
        parked(&records, &args, &c, &l, now);
        records.set(ID, State::Approved);
        let fresh = || Ok(call(&args, json!({"exe_sha256": "a"}), &c, &l));
        assert_eq!(resume(&records, ID, fresh, now), Resume::Run);
        assert_eq!(
            resume(
                &records,
                ID,
                || Ok(call(&args, json!({"exe_sha256": "a"}), &c, &l)),
                now
            ),
            Resume::Ended(INTERRUPTED.to_owned()),
            "a consumed record never runs again"
        );
        assert_eq!(
            *records
                .consumed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            1
        );
    }

    #[test]
    fn drift_since_the_ask_ends_the_call_unconsumed() {
        let (records, args, c, l) = (
            Memory::default(),
            json!({"argv": ["sh", "go.sh"]}),
            classification(),
            Label::top(),
        );
        let now = Utc::now();
        parked(&records, &args, &c, &l, now);
        records.set(ID, State::Approved);
        let changed = || Ok(call(&args, json!({"exe_sha256": "b"}), &c, &l));
        assert_eq!(
            resume(&records, ID, changed, now),
            Resume::Ended(not_done(CHANGED))
        );
        assert_eq!(
            *records
                .consumed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            0
        );
        assert_eq!(
            records.standing(ID).unwrap().unwrap().state,
            State::Approved
        );
    }

    #[test]
    fn a_lost_consumption_runs_nothing_here() {
        struct Losing(Memory);
        impl ApprovalRecords for Losing {
            fn park(&self, r: &ApprovalRecord, a: &[String]) -> Result<(), String> {
                self.0.park(r, a)
            }
            fn standing(&self, id: &str) -> Result<Option<Standing>, String> {
                self.0.standing(id)
            }
            fn consume(&self, _: &str) -> Result<Consumed, String> {
                Ok(Consumed::Lost("sync.version_conflict".to_owned()))
            }
            fn expire(&self, id: &str) -> Result<(), String> {
                self.0.expire(id)
            }
        }
        let (records, args, c, l) = (
            Losing(Memory::default()),
            json!({}),
            classification(),
            Label::top(),
        );
        let now = Utc::now();
        assert!(matches!(
            park(
                &records,
                &call(&args, Value::Null, &c, &l),
                &["tgorka".to_owned()],
                now
            ),
            ToolOutcome::Parked { .. }
        ));
        records.0.set(ID, State::Approved);
        assert_eq!(
            resume(&records, ID, || Ok(call(&args, Value::Null, &c, &l)), now),
            Resume::Elsewhere("sync.version_conflict".to_owned())
        );
    }

    #[test]
    fn past_its_time_a_pending_record_expires_and_an_approved_one_is_unused() {
        let (records, args, c, l) = (Memory::default(), json!({}), classification(), Label::top());
        let now = Utc::now();
        parked(&records, &args, &c, &l, now);
        let later = now + chrono::Duration::days(2);
        assert_eq!(
            resume(&records, ID, || Err("unused".into()), later),
            Resume::Ended(EXPIRED.to_owned())
        );
        assert_eq!(records.standing(ID).unwrap().unwrap().state, State::Expired);

        let (records, args, c, l) = (Memory::default(), json!({}), classification(), Label::top());
        parked(&records, &args, &c, &l, now);
        records.set(ID, State::Approved);
        assert_eq!(
            resume(&records, ID, || Ok(call(&args, Value::Null, &c, &l)), later),
            Resume::Ended(APPROVED_UNUSED.to_owned())
        );
        assert_eq!(
            *records
                .consumed
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            0
        );
    }

    #[test]
    fn a_denial_is_final() {
        let (records, args, c, l) = (Memory::default(), json!({}), classification(), Label::top());
        let now = Utc::now();
        parked(&records, &args, &c, &l, now);
        records.set(ID, State::Denied);
        assert_eq!(
            resume(&records, ID, || Err("unused".into()), now),
            Resume::Ended(DENIED.to_owned())
        );
    }
}
