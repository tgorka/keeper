//! The complete accepted prefix of a domain's replicated history (contract
//! §5, AD-10): what a native consumer may treat as the graph at position N.
//!
//! Current `kg_nodes`/`kg_edges` rows bootstrap independently of the events
//! and may be ahead of or behind them, so they are never read here. The
//! prefix is built from the events alone: the head observed at the start is
//! the target; events 1, 2, … are taken while each is present with exactly
//! its `op_count` operations, and every one must then verify — its ordinals
//! `0..op_count`, its `delta_digest`, and every before-image equal to the
//! state the events before it replayed to. An event not yet replicated (or
//! not all of its operations) ends the prefix early: the caller serves the
//! last complete state as stale or refuses. An event that is present and
//! wrong is corruption, never a shorter prefix.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::protocol::delta::Op;
use super::protocol::vocabulary::Operation;
use super::protocol::wire::{Event, EventHeader};
use super::source::{GraphSource, SourceError};

/// How many events one snapshot reads.
pub const BATCH_EVENTS: usize = 256;

/// A verified prefix: events `1..=n` of a domain whose head was `head`.
#[derive(Clone, Debug, PartialEq)]
pub struct Prefix {
    pub domain: String,
    /// The head observed when the read started (0 when none had replicated).
    pub head: u64,
    /// The last position of the complete prefix.
    pub n: u64,
    pub events: Vec<Event>,
    /// Every entity's image after event `n`, keyed by its `entity_type`
    /// (`node`, `edge`, `tag`, `node_tag`) and id; a removed membership is
    /// absent.
    pub state: BTreeMap<(String, String), Map<String, Value>>,
}

impl Prefix {
    /// Whether the prefix reaches the head it started from.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.n == self.head
    }
}

/// Why no prefix could be read.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PrefixError {
    #[error(transparent)]
    Source(#[from] SourceError),
    /// The event at `at` is present and does not verify.
    #[error("graph event {at} is corrupt: {reason}")]
    Corrupt { at: u64, reason: &'static str },
}

pub(crate) fn corrupt(at: u64, reason: &'static str) -> PrefixError {
    PrefixError::Corrupt { at, reason }
}

/// The complete prefix `source` holds now.
pub fn complete(source: &dyn GraphSource) -> Result<Prefix, PrefixError> {
    let first = source.prefix_batch(0, BATCH_EVENTS)?;
    let head = first.head.unwrap_or(0);
    let mut prefix = Prefix {
        domain: source.domain().to_owned(),
        head,
        n: 0,
        events: Vec::new(),
        state: BTreeMap::new(),
    };
    let mut batch = Some(first);
    while prefix.n < head {
        let current = match batch.take() {
            Some(batch) => batch,
            None => source.prefix_batch(prefix.n, BATCH_EVENTS)?,
        };
        let upto = prefix.n.saturating_add(BATCH_EVENTS as u64).min(head);
        let mut events = group(prefix.n, &current.events, &current.ops)?;
        while prefix.n < upto {
            let at = prefix.n + 1;
            let Some(event) = events.remove(&at) else {
                return Ok(prefix);
            };
            let Some(event) = event else {
                return Ok(prefix);
            };
            event.check().map_err(|_| corrupt(at, "digest or count"))?;
            replay(&mut prefix.state, at, &event.ops)?;
            prefix.events.push(event);
            prefix.n = at;
        }
    }
    Ok(prefix)
}

/// The batch's events by position; `None` for an event whose operations
/// have not all replicated.
pub(crate) fn group(
    after: u64,
    headers: &[Map<String, Value>],
    ops: &[Map<String, Value>],
) -> Result<BTreeMap<u64, Option<Event>>, PrefixError> {
    let mut by_event: BTreeMap<String, BTreeMap<u64, Op>> = BTreeMap::new();
    for row in ops {
        let event_id = text(row, "event_id").unwrap_or_default();
        let ordinal = row
            .get("ordinal")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX);
        let op = decode_op(row).ok_or_else(|| corrupt(after + 1, "operation row"))?;
        if by_event
            .entry(event_id)
            .or_default()
            .insert(ordinal, op)
            .is_some()
        {
            return Err(corrupt(after + 1, "duplicate ordinal"));
        }
    }
    let mut out = BTreeMap::new();
    for row in headers {
        let header = decode_header(row).ok_or_else(|| corrupt(after + 1, "event header"))?;
        let at: u64 = header
            .graph_seq
            .parse()
            .map_err(|_| corrupt(after + 1, "graph position"))?;
        let group = by_event.remove(&header.id).unwrap_or_default();
        let count = u64::from(header.op_count);
        // What no arriving row could make valid is corruption now: a count
        // outside 1..=128, an ordinal at or past the count, more rows than
        // the count. Only a group that may still complete is lag.
        let bound = super::protocol::delta::MAX_OPERATIONS as u64;
        if count == 0
            || count > bound
            || group.keys().any(|ordinal| *ordinal >= count)
            || group.len() as u64 > count
        {
            return Err(corrupt(at, "operation set"));
        }
        let event = if (group.len() as u64) < count {
            None
        } else {
            Some(Event {
                event: header,
                ops: group.into_values().collect(),
            })
        };
        if out.insert(at, event).is_some() {
            return Err(corrupt(at, "duplicate position"));
        }
    }
    Ok(out)
}

fn text(row: &Map<String, Value>, key: &str) -> Option<String> {
    row.get(key).and_then(Value::as_str).map(str::to_owned)
}

/// A JSON column as the replica holds it (text) or decoded.
fn json(value: Option<&Value>) -> Option<Value> {
    match value {
        None | Some(Value::Null) => Some(Value::Null),
        Some(Value::String(text)) => serde_json::from_str(text).ok(),
        Some(other) => Some(other.clone()),
    }
}

fn decode_header(row: &Map<String, Value>) -> Option<EventHeader> {
    let mut row = row.clone();
    row.remove("graph");
    for key in ["origin_chain", "approval_basis"] {
        let value = json(row.get(key))?;
        row.insert(key.to_owned(), value);
    }
    serde_json::from_value(Value::Object(row)).ok()
}

fn decode_op(row: &Map<String, Value>) -> Option<Op> {
    let mut row = row.clone();
    row.remove("event_id");
    for key in ["before_value", "after_value"] {
        let value = json(row.get(key))?;
        row.insert(key.to_owned(), value);
    }
    serde_json::from_value(Value::Object(row)).ok()
}

/// Apply one event's operations to `state`, each before-image equal to what
/// the state holds.
fn replay(
    state: &mut BTreeMap<(String, String), Map<String, Value>>,
    at: u64,
    ops: &[Op],
) -> Result<(), PrefixError> {
    for op in ops {
        let entity_type = serde_json::to_value(op.entity_type)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| corrupt(at, "entity type"))?;
        let key = (entity_type, op.entity_id.clone());
        let held = state.get(&key).cloned().map(Value::Object);
        if held != op.before_value {
            return Err(corrupt(at, "before-image"));
        }
        match (&op.operation, &op.after_value) {
            (Operation::Remove, None) => {
                state.remove(&key);
            }
            (_, Some(Value::Object(after))) => {
                state.insert(key, after.clone());
            }
            _ => return Err(corrupt(at, "after-image")),
        }
    }
    Ok(())
}
