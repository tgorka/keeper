//! A person's device's graph client for one database: changes sent through
//! `graph.apply` with their outcomes, and the typed reads.
//!
//! Sending is split in two so nothing is lost across a crash: [`GraphClient::mint`]
//! gives a change its request id once (the caller may persist
//! [`Prepared::request`] before anything is sent), and [`GraphClient::send`]
//! sends it, as often as needed, always under that id.
//! [`GraphClient::prepared`] restores a persisted request. An outcome comes
//! from the request's own receipt — on this replica, or from `graph.outcome`
//! — never from the transport alone ([`super::outcome`]).
//!
//! The graph is online-only until a durable pending queue exists: a change
//! is refused [`GraphClientError::Offline`] while the replica has no link,
//! never accepted optimistically.

use serde_json::{Map, Value};

use super::outcome::{Attempt, GraphOutcome, GraphRequestError, GraphRequests, RequestKey};
use super::protocol::candidate::MAX_REDIRECT_HOPS;
use super::protocol::change::{self, Change, ChangeOp, DESCRIPTOR};
use super::protocol::request::Request;
use super::protocol::scope::{
    client_scope, CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, RECEIPT_TABLE,
};
use super::protocol::wire::{
    self, decode_content, decode_edge, decode_history, decode_node, HistoryFilter, HistoryPage,
    NodeView,
};
use super::protocol::Refusal;
use crate::hq::port::{LinkState, ReplicaError, Subscription};
use crate::hq::sessions::Source;
use crate::notes::tags::normalise;

/// The subscriptions a replica of `database` holds for the graph: every
/// graph table under the database's `kg_graph` scope, and this device's own
/// receipts under its `kg_client` scope. Ids are the contract's
/// subscription names.
#[must_use]
pub fn subscriptions(database: &str, actor: &str, client: &str) -> Vec<Subscription> {
    const NAMES: [&str; 8] = [
        "kgEvents",
        "kgEventOps",
        "kgHead",
        "kgNodes",
        "kgEdges",
        "kgTags",
        "kgNodeTags",
        "kgContents",
    ];
    NAMES
        .iter()
        .zip(GRAPH_TABLES)
        .map(|(id, table)| Subscription {
            id: (*id).to_owned(),
            table: table.to_owned(),
            scopes: vec![(GRAPH_VARIABLE.to_owned(), vec![database.to_owned()])],
        })
        .chain(std::iter::once(Subscription {
            id: "kgReceipts".to_owned(),
            table: RECEIPT_TABLE.to_owned(),
            scopes: vec![(
                CLIENT_VARIABLE.to_owned(),
                vec![client_scope(actor, client)],
            )],
        }))
        .collect()
}

/// A change with its request id, ready to send (and to persist first).
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    request: Request,
    key: RequestKey,
}

impl Prepared {
    /// What to persist: the request id and the payload, which
    /// [`GraphClient::prepared`] turns back into this.
    #[must_use]
    pub fn request(&self) -> &Request {
        &self.request
    }

    #[must_use]
    pub fn key(&self) -> &RequestKey {
        &self.key
    }
}

/// A change [`GraphClient::apply`] gave a request id: the request, the
/// attempt when one was made, and what can be concluded so far (or why that
/// could not be read). Ask again with [`GraphClient::outcome`] while it is
/// pending or unreadable.
#[derive(Debug, PartialEq)]
pub struct Applied {
    pub prepared: Prepared,
    pub attempt: Option<Attempt>,
    pub outcome: Result<GraphOutcome, GraphClientError>,
}

/// Why a graph call did not happen or could not be read.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GraphClientError {
    /// The replica has no link: nothing was sent.
    #[error("this database is offline; graph changes need a connection")]
    Offline,
    /// The change or read is outside the contract.
    #[error("refused: {0}")]
    Refused(Refusal),
    #[error(transparent)]
    Request(#[from] GraphRequestError),
    #[error(transparent)]
    Replica(#[from] ReplicaError),
    /// The server answered with something this build does not read.
    #[error("unreadable answer: {0}")]
    Unreadable(Refusal),
}

/// One database's graph, as this device's person uses it.
pub struct GraphClient {
    requests: GraphRequests,
}

impl GraphClient {
    #[must_use]
    pub fn new(source: Source) -> Self {
        Self {
            requests: GraphRequests::new(source),
        }
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        self.requests.domain()
    }

    fn source(&self) -> &Source {
        self.requests.source()
    }

    /// A person's change of `ops` (tags under keeper's tag policy).
    pub fn change(
        &self,
        ops: Vec<ChangeOp>,
        event_time_ms: Option<u64>,
    ) -> Result<Change, Refusal> {
        Change::new(ops, None, event_time_ms, normalise)
    }

    /// Give `change` a new request id: the one every send of it uses.
    pub fn mint(&self, change: &Change) -> Result<Prepared, Refusal> {
        self.prepared(Request {
            request_id: ulid::Ulid::new().to_string(),
            context_id: None,
            payload: change.payload(),
        })
    }

    /// Restore a persisted request: its payload must still be a change and
    /// it must be a person's (no context).
    pub fn prepared(&self, request: Request) -> Result<Prepared, Refusal> {
        if request.context_id.is_some() || !super::protocol::is_ulid(&request.request_id) {
            return Err(Refusal::InvalidChange);
        }
        change::parse(&request.payload, normalise)?;
        let key = self.requests.key(DESCRIPTOR, &request)?;
        Ok(Prepared { request, key })
    }

    fn online(&self) -> Result<(), GraphClientError> {
        match self.source().replica.state().link {
            LinkState::Connected | LinkState::Connecting => Ok(()),
            _ => Err(GraphClientError::Offline),
        }
    }

    /// Send `prepared` once.
    pub fn send(&self, prepared: &Prepared) -> Result<Attempt, GraphClientError> {
        self.online()?;
        let attempt = self.requests.submit(&prepared.key, &prepared.request)?;
        if matches!(attempt.result(), Err(ReplicaError::NotSyncing)) {
            return Err(GraphClientError::Offline);
        }
        Ok(attempt)
    }

    /// What may be concluded about `prepared` now: from its receipt on this
    /// replica, else (online) from `graph.outcome`, with `attempt` lending
    /// its transport answer when there is one.
    pub fn outcome(
        &self,
        prepared: &Prepared,
        attempt: Option<&Attempt>,
    ) -> Result<GraphOutcome, GraphClientError> {
        let mut receipt = self.requests.receipt(&prepared.key)?;
        if receipt.is_none() && self.online().is_ok() {
            receipt = match self.requests.remote_receipt(&prepared.key, None) {
                Ok(receipt) => receipt,
                // Unreachable or refused now: the request stays pending.
                Err(GraphRequestError::Replica(_)) => None,
                Err(error) => return Err(error.into()),
            };
        }
        Ok(match attempt {
            Some(attempt) => self
                .requests
                .resolve(&prepared.key, attempt, receipt.as_ref()),
            None => self.requests.settle(&prepared.key, receipt.as_ref()),
        })
    }

    /// Mint, send and resolve `change` once. Refused (`Err`) only before a
    /// request id exists — no link, or a change outside the contract. From
    /// then on the answer is [`Applied`], which always carries the request
    /// (and the attempt, once one was made), whatever failed after: the
    /// command may have committed, so the same id must stay askable.
    pub fn apply(&self, change: &Change) -> Result<Applied, GraphClientError> {
        self.online()?;
        let prepared = self.mint(change).map_err(GraphClientError::Refused)?;
        let attempt = match self.send(&prepared) {
            Ok(attempt) => attempt,
            Err(error) => {
                return Ok(Applied {
                    prepared,
                    attempt: None,
                    outcome: Err(error),
                })
            }
        };
        let outcome = self.outcome(&prepared, Some(&attempt));
        Ok(Applied {
            prepared,
            attempt: Some(attempt),
            outcome,
        })
    }

    fn read(
        &self,
        operation: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, GraphClientError> {
        self.online()?;
        Ok(self.source().replica.remote_query(operation, params)?)
    }

    /// The node `id` (archived ones only with `include_archived`).
    pub fn get_node(
        &self,
        id: &str,
        include_archived: bool,
    ) -> Result<Option<NodeView>, GraphClientError> {
        let rows = self.read(wire::GET_NODE, wire::get_params(None, id, include_archived))?;
        decode_node(id, rows).map_err(GraphClientError::Unreadable)
    }

    /// The edge `id`: by default only while it and both endpoints are active.
    pub fn get_edge(
        &self,
        id: &str,
        include_archived: bool,
    ) -> Result<Option<Map<String, Value>>, GraphClientError> {
        let rows = self.read(wire::GET_EDGE, wire::get_params(None, id, include_archived))?;
        decode_edge(id, rows).map_err(GraphClientError::Unreadable)
    }

    /// The content `id`: by default only while its owner node is active (a
    /// content has no lifecycle of its own).
    pub fn get_content(
        &self,
        id: &str,
        include_archived: bool,
    ) -> Result<Option<Map<String, Value>>, GraphClientError> {
        let rows = self.read(
            wire::GET_CONTENT,
            wire::get_params(None, id, include_archived),
        )?;
        decode_content(id, rows).map_err(GraphClientError::Unreadable)
    }

    /// The redirect chain of node `id`: the ids its `merged_into_id` links
    /// lead through, in order, ending at the first unmerged node (empty for
    /// a node never merged; `None` when `id` does not exist). The absorbed
    /// node itself stays what [`Self::get_node`] answers for `id`: the
    /// survivor is reported, never substituted. A chain longer than the
    /// contract's [`MAX_REDIRECT_HOPS`], or one that loops or breaks, is an
    /// answer this build does not read.
    pub fn redirects(&self, id: &str) -> Result<Option<Vec<String>>, GraphClientError> {
        let Some(mut node) = self.get_node(id, true)? else {
            return Ok(None);
        };
        let mut chain: Vec<String> = Vec::new();
        while let Some(next) = node.image.get("merged_into_id").and_then(Value::as_str) {
            if chain.len() == MAX_REDIRECT_HOPS || next == id || chain.iter().any(|c| c == next) {
                return Err(GraphClientError::Unreadable(Refusal::UnsupportedVersion));
            }
            chain.push(next.to_owned());
            node = self
                .get_node(next, true)?
                .ok_or(GraphClientError::Unreadable(Refusal::UnsupportedVersion))?;
        }
        Ok(Some(chain))
    }

    /// One page of retained history after graph position `after`; resume
    /// from its `scanned_through`.
    pub fn history(
        &self,
        after: &str,
        limit: u32,
        filter: &HistoryFilter,
    ) -> Result<HistoryPage, GraphClientError> {
        let params =
            wire::history_params(None, after, limit, filter).map_err(GraphClientError::Refused)?;
        let rows = self.read(wire::HISTORY, params)?;
        decode_history(after, limit, rows).map_err(GraphClientError::Unreadable)
    }
}
