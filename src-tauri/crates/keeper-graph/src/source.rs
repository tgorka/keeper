//! Where a native consumer reads a domain's replicated graph history: the
//! narrow [`GraphSource`] port. The prefix reader ([`super::prefix`]) and,
//! later, the query projection read only through it, so the trusted agent
//! runtime can implement it over its own replica without implementing the
//! whole [`Replica`](keeper_ports::Replica) trait.

use serde_json::{Map, Value};

use keeper_ports::ReplicaError;
use keeper_ports::Source;

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
    /// The graph subscriptions' state now: whether the server grants this
    /// replica the domain's graph, and the Syncular commit checkpoint they
    /// have pulled to (a transport visibility bound, never graph order).
    fn checkpoint(&self) -> Result<Checkpoint, SourceError>;
}

/// What the server answered this replica's graph subscriptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeState {
    /// Every history subscription is active and holds the domain.
    Granted,
    /// The server revoked a history subscription.
    Revoked,
    /// A subscription has not completed its first pull yet.
    Pending,
}

/// [`GraphSource::checkpoint`]'s answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub scope: ScopeState,
    /// The lowest commit cursor of the history subscriptions, while granted.
    pub cursor: Option<i64>,
}

/// The tables a projection reads from: the graph's ledger (the events,
/// their operations, the head and the reviews), all under `kg_history`.
pub use crate::protocol::scope::HISTORY_TABLES;
use crate::protocol::scope::HISTORY_VARIABLE;

/// Every unit the replica holds, with its scope map, status, the cursor it
/// has pulled to (its own while it bootstraps or once it is no longer live,
/// the replica's while it is live), whether a bootstrap is under way, and
/// the echo of its last pull. syncular-client keeps them in
/// `_syncular_units`; the replica cursor in `_syncular_meta`.
pub(crate) const UNITS: &str = "SELECT u.scopes AS scopes, u.status AS status, \
    COALESCE(u.cursor, (SELECT CAST(m.value AS INTEGER) FROM _syncular_meta m \
      WHERE m.key = 'replicaCursor')) AS cursor, \
    u.bootstrap_state IS NOT NULL AS bootstrapping, u.echo AS echo \
  FROM _syncular_units u";

/// A unit row's scope map; `None` when it does not parse.
fn unit_scopes(row: &Map<String, Value>) -> Option<Map<String, Value>> {
    match row.get("scopes") {
        Some(Value::String(text)) => serde_json::from_str(text).ok(),
        Some(Value::Object(scopes)) => Some(scopes.clone()),
        _ => None,
    }
}

/// Whether a unit row's last pull revoked `table` (its echo's record).
fn echo_revoked(row: &Map<String, Value>, table: &str) -> bool {
    let echo: Option<Vec<Value>> = match row.get("echo") {
        Some(Value::String(text)) => serde_json::from_str(text).ok(),
        Some(Value::Array(echo)) => Some(echo.clone()),
        _ => None,
    };
    echo.into_iter().flatten().any(|record| {
        record.get("table").and_then(Value::as_str) == Some(table)
            && record.get("status").and_then(Value::as_str) == Some("revoked")
    })
}

/// The checkpoint of rows of [`UNITS`] for `domain`'s ledger unit
/// (`kg_history` = `domain`): revoked when the unit is revoked or its last
/// pull revoked any ledger table, pending until it is active and its first
/// pull is done, granted at its cursor otherwise.
pub(crate) fn checkpoint_of(domain: &str, rows: &[Map<String, Value>]) -> Checkpoint {
    let pending = Checkpoint {
        scope: ScopeState::Pending,
        cursor: None,
    };
    let ledger = rows.iter().find(|row| {
        unit_scopes(row).is_some_and(|scopes| {
            scopes.len() == 1
                && scopes.get(HISTORY_VARIABLE).and_then(Value::as_str) == Some(domain)
        })
    });
    let Some(row) = ledger else {
        return pending;
    };
    let revoked = row.get("status").and_then(Value::as_str) == Some("revoked")
        || HISTORY_TABLES.iter().any(|table| echo_revoked(row, table));
    if revoked {
        return Checkpoint {
            scope: ScopeState::Revoked,
            cursor: None,
        };
    }
    let bootstrapping = matches!(row.get("bootstrapping"), Some(Value::Bool(true)))
        || row.get("bootstrapping").and_then(Value::as_i64) == Some(1);
    match (
        row.get("status").and_then(Value::as_str),
        row.get("cursor").and_then(Value::as_i64),
    ) {
        (Some("active"), Some(cursor)) if cursor >= 0 && !bootstrapping => Checkpoint {
            scope: ScopeState::Granted,
            cursor: Some(cursor),
        },
        _ => pending,
    }
}

/// The most positions one [`Source`] batch reads, whatever is asked.
pub const MAX_BATCH_EVENTS: usize = 1_024;

/// One statement, so one SQLite snapshot: the head, the batch's headers and
/// their operations, as a single JSON document. Positions are looked up
/// exactly (`?2` is the JSON array of their decimal texts), so every lookup
/// seeks the `(graph, graph_seq)` index and the ops' `(event_id, ordinal)`
/// index; a range over a cast text sequence would scan the whole graph's
/// history on every batch. A subquery's value reaches `json_object` as
/// text, without the JSON subtype (the SQLite that libsqlite3-sys 0.39
/// bundles keeps to that strictly), so each array is passed through
/// `json()` again to embed it as an array rather than a string.
pub(crate) const BATCH: &str = "SELECT json_object(\
    'head', (SELECT graph_seq FROM kg_head WHERE id = ?1), \
    'events', json((SELECT json_group_array(json(e.row)) FROM (SELECT json_object(\
        'id', id, 'graph', graph, 'graph_seq', graph_seq, 'descriptor', descriptor, \
        'request_id', request_id, 'request_digest', request_digest, 'actor_id', actor_id, \
        'client_id', client_id, 'origin_class', origin_class, 'origin_actor', origin_actor, \
        'origin_chain', origin_chain, 'context_id', context_id, 'approval_basis', approval_basis, \
        'reverses_event_id', reverses_event_id, 'event_time_ms', event_time_ms, \
        'recorded_at_ms', recorded_at_ms, 'op_count', op_count, 'delta_digest', delta_digest\
      ) AS row FROM kg_events WHERE graph = ?1 \
        AND graph_seq IN (SELECT value FROM json_each(?2))) e)), \
    'ops', json((SELECT json_group_array(json(o.row)) FROM (SELECT json_object(\
        'event_id', p.event_id, 'ordinal', p.ordinal, 'entity_type', p.entity_type, \
        'entity_id', p.entity_id, 'operation', p.operation, \
        'before_revision', p.before_revision, 'after_revision', p.after_revision, \
        'before_value', p.before_value, 'after_value', p.after_value\
      ) AS row FROM kg_event_ops p WHERE p.event_id IN (SELECT id FROM kg_events \
        WHERE graph = ?1 AND graph_seq IN (SELECT value FROM json_each(?2)))) o))\
  ) AS batch";

/// One read of a replica's file: `sql` with `params`, its rows. keeper's is
/// [`keeper_ports::Replica::query`]; keeper-hq's is its own replica's
/// fallible query channel. Both answer from the same Syncular tables.
pub trait Query: Fn(&str, &[Value]) -> Result<Vec<Map<String, Value>>, SourceError> {}
impl<F: Fn(&str, &[Value]) -> Result<Vec<Map<String, Value>>, SourceError>> Query for F {}

/// [`GraphSource::checkpoint`] of `domain` over any replica `query`: the
/// state of its ledger unit ([`checkpoint_of`]).
pub fn checkpoint_with(domain: &str, query: impl Query) -> Result<Checkpoint, SourceError> {
    let rows = query(UNITS, &[])?;
    Ok(checkpoint_of(domain, &rows))
}

/// How far a server scope revocation reached the replica's graph units
/// (112.1 CC-7, CC-8): the Syncular client deletes a revoked unit's rows in
/// the transaction that records it revoked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Revocation {
    /// No unit covering these tables is revoked.
    None,
    /// Some are revoked: a revocation signal, its purge not complete.
    Partial,
    /// Every one of these tables is covered by a unit, and every unit
    /// covering it is revoked: their rows are purged.
    Purged,
}

/// [`Revocation`] of the units covering `tables` over any replica `query`.
/// A table no unit covers is not purged (fail closed).
pub fn revocation_with(tables: &[&str], query: impl Query) -> Result<Revocation, SourceError> {
    let rows = query(UNITS, &[])?;
    Ok(revocation_of(tables, &rows))
}

/// The scope variable of a graph table, receipts and proposals included.
fn variable_of(table: &str) -> Option<&'static str> {
    use crate::protocol::scope::{
        CLIENT_VARIABLE, GRAPH_VARIABLE, PROPOSAL_TABLE, PROPOSAL_VARIABLE, RECEIPT_TABLE,
        STATE_TABLES,
    };
    if HISTORY_TABLES.contains(&table) {
        Some(HISTORY_VARIABLE)
    } else if STATE_TABLES.contains(&table) {
        Some(GRAPH_VARIABLE)
    } else if table == RECEIPT_TABLE {
        Some(CLIENT_VARIABLE)
    } else if table == PROPOSAL_TABLE {
        Some(PROPOSAL_VARIABLE)
    } else {
        None
    }
}

/// [`Revocation`] of the rows of [`UNITS`] for `tables`: a unit covers a
/// graph table when its scope map is that table's one variable; it stands
/// revoked for it when the unit is, or its last pull revoked the table.
pub(crate) fn revocation_of(tables: &[&str], rows: &[Map<String, Value>]) -> Revocation {
    let covering = |table: &str| -> Vec<bool> {
        let Some(variable) = variable_of(table) else {
            return Vec::new();
        };
        rows.iter()
            .filter(|row| {
                unit_scopes(row)
                    .is_some_and(|scopes| scopes.len() == 1 && scopes.contains_key(variable))
            })
            .map(|row| {
                row.get("status").and_then(Value::as_str) == Some("revoked")
                    || echo_revoked(row, table)
            })
            .collect()
    };
    let per_table: Vec<Vec<bool>> = tables.iter().map(|table| covering(table)).collect();
    if !per_table.iter().flatten().any(|revoked| *revoked) {
        return Revocation::None;
    }
    if per_table
        .iter()
        .all(|units| !units.is_empty() && units.iter().all(|revoked| *revoked))
    {
        Revocation::Purged
    } else {
        Revocation::Partial
    }
}

/// [`GraphSource::prefix_batch`] of `domain` over any replica `query`: one
/// statement, so one snapshot, of the head and the events and operations
/// at positions `after + 1 ..= after + limit` (at most
/// [`MAX_BATCH_EVENTS`]).
pub fn prefix_batch_with(
    domain: &str,
    after: u64,
    limit: usize,
    query: impl Query,
) -> Result<PrefixBatch, SourceError> {
    let limit = limit.min(MAX_BATCH_EVENTS);
    let upto = after.saturating_add(u64::try_from(limit).unwrap_or(u64::MAX));
    let positions: Vec<String> = (after.saturating_add(1)..=upto)
        .map(|n| n.to_string())
        .collect();
    let rows = query(
        BATCH,
        &[
            Value::from(domain),
            Value::from(serde_json::to_string(&positions).unwrap_or_default()),
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

impl GraphSource for Source {
    fn domain(&self) -> &str {
        &self.database
    }

    fn checkpoint(&self) -> Result<Checkpoint, SourceError> {
        checkpoint_with(&self.database, |sql, params| {
            Ok(self.replica.query(sql, params)?)
        })
    }

    fn prefix_batch(&self, after: u64, limit: usize) -> Result<PrefixBatch, SourceError> {
        prefix_batch_with(&self.database, after, limit, |sql, params| {
            Ok(self.replica.query(sql, params)?)
        })
    }
}
