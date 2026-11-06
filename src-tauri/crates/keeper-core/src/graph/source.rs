//! Where a native consumer reads a domain's replicated graph history: the
//! narrow [`GraphSource`] port. The prefix reader ([`super::prefix`]) and,
//! later, the query projection read only through it, so the trusted agent
//! runtime can implement it over its own replica without implementing the
//! whole [`Replica`](crate::hq::port::Replica) trait.

use serde_json::{Map, Value};

use crate::hq::port::ReplicaError;
use crate::hq::sessions::Source;

/// One read snapshot of a domain's history: its head and the events (with
/// their operation rows) at graph positions `after + 1 ..= after + limit`.
/// Rows are as the replica holds them: JSON columns may be text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PrefixBatch {
    /// `kg_head.graph_seq`, or `None` while no head has replicated.
    pub head: Option<u64>,
    pub events: Vec<Map<String, Value>>,
    pub ops: Vec<Map<String, Value>>,
}

/// Why a source could not be read.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum SourceError {
    #[error(transparent)]
    Replica(#[from] ReplicaError),
    /// The source answered with something that is not a batch.
    #[error("unreadable graph source: {0}")]
    Unreadable(String),
}

/// A domain's replicated graph, read in bounded snapshots.
pub trait GraphSource: Send + Sync {
    /// The domain (database) this source holds.
    fn domain(&self) -> &str;
    /// One snapshot: the head plus the events and operations with graph
    /// position in `(after, after + limit]`.
    fn prefix_batch(&self, after: u64, limit: usize) -> Result<PrefixBatch, SourceError>;
}

/// One statement, so one SQLite snapshot: the head, the batch's headers and
/// their operations, as a single JSON document.
const BATCH: &str = "SELECT json_object(\
    'head', (SELECT graph_seq FROM kg_head WHERE id = ?1), \
    'events', (SELECT json_group_array(json(e.row)) FROM (SELECT json_object(\
        'id', id, 'graph', graph, 'graph_seq', graph_seq, 'descriptor', descriptor, \
        'request_id', request_id, 'request_digest', request_digest, 'actor_id', actor_id, \
        'client_id', client_id, 'origin_class', origin_class, 'origin_actor', origin_actor, \
        'origin_chain', origin_chain, 'context_id', context_id, 'approval_basis', approval_basis, \
        'reverses_event_id', reverses_event_id, 'event_time_ms', event_time_ms, \
        'recorded_at_ms', recorded_at_ms, 'op_count', op_count, 'delta_digest', delta_digest\
      ) AS row FROM kg_events WHERE graph = ?1 \
        AND CAST(graph_seq AS INTEGER) > ?2 AND CAST(graph_seq AS INTEGER) <= ?3) e), \
    'ops', (SELECT json_group_array(json(o.row)) FROM (SELECT json_object(\
        'event_id', p.event_id, 'ordinal', p.ordinal, 'entity_type', p.entity_type, \
        'entity_id', p.entity_id, 'operation', p.operation, \
        'before_revision', p.before_revision, 'after_revision', p.after_revision, \
        'before_value', p.before_value, 'after_value', p.after_value\
      ) AS row FROM kg_event_ops p JOIN kg_events e ON e.id = p.event_id \
      WHERE e.graph = ?1 AND CAST(e.graph_seq AS INTEGER) > ?2 \
        AND CAST(e.graph_seq AS INTEGER) <= ?3) o)\
  ) AS batch";

impl GraphSource for Source {
    fn domain(&self) -> &str {
        &self.database
    }

    fn prefix_batch(&self, after: u64, limit: usize) -> Result<PrefixBatch, SourceError> {
        let upto = after.saturating_add(u64::try_from(limit).unwrap_or(u64::MAX));
        let bound = |n: u64| Value::from(i64::try_from(n).unwrap_or(i64::MAX));
        let rows = self.replica.query(
            BATCH,
            &[
                Value::from(self.database.as_str()),
                bound(after),
                bound(upto),
            ],
        )?;
        let unreadable = |what: &str| SourceError::Unreadable(what.to_owned());
        let batch = rows
            .into_iter()
            .next()
            .and_then(|mut row| row.remove("batch"))
            .ok_or_else(|| unreadable("no batch"))?;
        let batch: Value = match batch {
            Value::String(text) => serde_json::from_str(&text).map_err(|_| unreadable("batch"))?,
            other => other,
        };
        let head = match &batch["head"] {
            Value::Null => None,
            Value::String(seq) => Some(seq.parse().map_err(|_| unreadable("head"))?),
            _ => return Err(unreadable("head")),
        };
        let rows = |key: &str| -> Result<Vec<Map<String, Value>>, SourceError> {
            match &batch[key] {
                Value::Null => Ok(Vec::new()),
                Value::Array(items) => items
                    .iter()
                    .map(|item| item.as_object().cloned().ok_or_else(|| unreadable(key)))
                    .collect(),
                _ => Err(unreadable(key)),
            }
        };
        Ok(PrefixBatch {
            head,
            events: rows("events")?,
            ops: rows("ops")?,
        })
    }
}
