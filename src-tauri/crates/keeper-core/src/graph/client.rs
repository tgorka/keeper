//! A person's device's graph client for one database: its commands —
//! changes (`graph.apply`), proposals and decisions (`graph.propose`,
//! `graph.decide`), acknowledgements (`graph.acknowledge`) and reversals
//! (`graph.reverse`) — with their outcomes, and the typed reads, the
//! unreviewed feed, a reversal's preview and a proposal as its reviewer
//! reads it.
//!
//! Sending is split in two so nothing is lost across a crash: [`GraphClient::mint`]
//! (or [`GraphClient::mint_command`]) gives a command its request id once (the caller may persist
//! [`Prepared::request`] before anything is sent), and [`GraphClient::send`]
//! sends it, as often as needed, always under that id.
//! [`GraphClient::prepared`] restores a persisted request. An outcome comes
//! from the request's own receipt — on this replica, or from `graph.outcome`
//! — never from the transport alone ([`super::outcome`]).
//!
//! A change may also be kept as a durable intent first ([`super::queue`]),
//! minted here offline and sent later under its own id; it is never
//! accepted optimistically. [`GraphClient::send`] and the reads refuse
//! [`GraphClientError::Offline`] while the replica has no link.

use std::sync::Arc;

use serde_json::{Map, Value};

use super::outcome::{
    Attempt, GraphOutcome, GraphRequestError, GraphRequests, PendingCause, RequestKey,
};
use super::protocol::candidate::MAX_REDIRECT_HOPS;
use super::protocol::change::{self, Change, ChangeOp, DESCRIPTOR};
use super::protocol::inverse::{self, Preview, Reverse, REVERSE};
use super::protocol::proposal::{self, Decision, Proposal, DECIDE, PROPOSE};
use super::protocol::request::Request;
use super::protocol::review::{self, Target, ACKNOWLEDGE};
use super::protocol::scope::{
    client_scope, CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, PROPOSAL_TABLE, PROPOSAL_VARIABLE,
    RECEIPT_TABLE,
};
use super::protocol::wire::{
    self, decode_content, decode_edge, decode_history, decode_node, decode_proposals,
    HistoryFilter, HistoryPage, NodeView, ProposalRow,
};
use super::protocol::Refusal;
use super::write::{Caller, GraphWrite, Writer};
use crate::hq::port::{LinkState, ReplicaError, Subscription};
use crate::hq::sessions::Source;
use crate::notes::tags::normalise;

/// The tables [`subscriptions`] subscribes: every graph table, the
/// receipts and the proposals. A revocation of the domain purges all of
/// them ([`super::source::revocation_with`]).
#[must_use]
pub fn subscribed_tables() -> Vec<&'static str> {
    GRAPH_TABLES
        .into_iter()
        .chain([RECEIPT_TABLE, PROPOSAL_TABLE])
        .collect()
}

/// The subscriptions a replica of `database` holds for the graph: every
/// graph table under the database's `kg_graph` scope, this device's own
/// receipts under its `kg_client` scope, and this actor's own proposals
/// under its `kg_proposal` scope (never another proposer's). Ids are the
/// contract's subscription names.
#[must_use]
pub fn subscriptions(database: &str, actor: &str, client: &str) -> Vec<Subscription> {
    const NAMES: [&str; 9] = [
        "kgEvents",
        "kgEventOps",
        "kgHead",
        "kgNodes",
        "kgEdges",
        "kgTags",
        "kgNodeTags",
        "kgContents",
        "kgReviews",
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
        .chain(std::iter::once(Subscription {
            id: "kgProposals".to_owned(),
            table: PROPOSAL_TABLE.to_owned(),
            scopes: vec![(PROPOSAL_VARIABLE.to_owned(), vec![actor.to_owned()])],
        }))
        .collect()
}

/// A command with its request id, ready to send (and to persist first).
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared {
    request: Request,
    key: RequestKey,
}

impl Prepared {
    /// What to persist: the request id and the payload (and the key's
    /// descriptor), which [`GraphClient::prepared`] turns back into this.
    #[must_use]
    pub fn request(&self) -> &Request {
        &self.request
    }

    #[must_use]
    pub fn key(&self) -> &RequestKey {
        &self.key
    }
}

/// A command [`GraphClient::apply`] (or another command) gave a request id: the request, the
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
    /// A typed read names no context: an agent reads through its own
    /// sealed handles, never through this client.
    #[error("this read is a person's only")]
    PersonOnly,
}

/// One database's graph, as this device's person (or an agent instance,
/// for its approved changes only) uses it.
pub struct GraphClient {
    requests: GraphRequests,
    caller: Caller,
}

impl GraphClient {
    /// A person's client over keeper's own replica `source`.
    #[must_use]
    pub fn new(source: Source) -> Self {
        Self {
            requests: GraphRequests::new(source),
            caller: Caller::Person,
        }
    }

    /// `caller`'s client of `writer`'s database behind the narrow write
    /// `port` (the agent runtime's way in, CC-1).
    #[must_use]
    pub fn over(port: Arc<dyn GraphWrite>, writer: &Writer, caller: Caller) -> Self {
        Self {
            requests: GraphRequests::over(port, writer),
            caller,
        }
    }

    #[must_use]
    pub fn domain(&self) -> &str {
        self.requests.domain()
    }

    /// On whose authority this client sends.
    #[must_use]
    pub fn caller(&self) -> &Caller {
        &self.caller
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
        self.mint_command(DESCRIPTOR, change.payload())
    }

    /// Give the `descriptor` command `payload` a new request id, under this
    /// caller's context.
    pub fn mint_command(&self, descriptor: &str, payload: Value) -> Result<Prepared, Refusal> {
        self.prepared(
            descriptor,
            Request {
                request_id: ulid::Ulid::new().to_string(),
                context_id: self.caller.context_id().map(str::to_owned),
                payload,
            },
        )
    }

    /// Restore a persisted request to `descriptor`. It must name exactly
    /// this caller's context (none for a person) and its payload must still
    /// be that command's. A person sends a change, a proposal, a decision,
    /// an acknowledgement or a reversal; an agent only a change naming the
    /// approval a person gives it (`graph_apply`). No other descriptor is a
    /// command this client sends.
    pub fn prepared(&self, descriptor: &str, request: Request) -> Result<Prepared, Refusal> {
        if request.context_id.as_deref() != self.caller.context_id()
            || !super::protocol::is_ulid(&request.request_id)
        {
            return Err(Refusal::InvalidChange);
        }
        match (&self.caller, descriptor) {
            (Caller::Agent { .. }, DESCRIPTOR) => {
                if change::parse(&request.payload, normalise)?
                    .approval_id()
                    .is_none()
                {
                    return Err(Refusal::InvalidChange);
                }
            }
            (Caller::Agent { .. }, _) => return Err(Refusal::InvalidChange),
            (Caller::Person, DESCRIPTOR) => {
                change::parse(&request.payload, normalise).map(|_| ())?;
            }
            (Caller::Person, PROPOSE) => {
                proposal::parse(&request.payload, normalise).map(|_| ())?;
            }
            (Caller::Person, DECIDE) => proposal::parse_decide(&request.payload).map(|_| ())?,
            (Caller::Person, ACKNOWLEDGE) => {
                review::parse_acknowledge(&request.payload).map(|_| ())?;
            }
            (Caller::Person, REVERSE) => inverse::parse_reverse(&request.payload).map(|_| ())?,
            (Caller::Person, _) => return Err(Refusal::InvalidChange),
        }
        let key = self.requests.key(descriptor, &request)?;
        Ok(Prepared { request, key })
    }

    /// Whether a send may start now: the replica has a link. A queued
    /// entry is never moved to `submitting` without one.
    pub fn online(&self) -> Result<(), GraphClientError> {
        match self.requests.port().link() {
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
    /// its transport answer when there is one. A `graph.outcome` the server
    /// refused is that refusal ([`GraphOutcome::Pending`] with its
    /// [`PendingCause`]: a revocation signal when [`PendingCause::Refused`]),
    /// never a quiet pending; one that could not be asked leaves the
    /// attempt to speak.
    pub fn outcome(
        &self,
        prepared: &Prepared,
        attempt: Option<&Attempt>,
    ) -> Result<GraphOutcome, GraphClientError> {
        let mut receipt = self.requests.receipt(&prepared.key)?;
        if receipt.is_none() && self.online().is_ok() {
            receipt = match self
                .requests
                .remote_receipt(&prepared.key, self.caller.context_id())
            {
                Ok(receipt) => receipt,
                Err(GraphRequestError::Replica(error)) => match PendingCause::of(&error) {
                    // Unreachable now: the request stays as the attempt says.
                    PendingCause::Transport => None,
                    cause => return Ok(GraphOutcome::Pending { cause }),
                },
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
        self.run(DESCRIPTOR, change.payload())
    }

    /// Propose `proposal` (its change under keeper's tag policy) for the
    /// reviewers it names: `proposed` with `{proposal_id,
    /// proposal_revision, proposal_digest}`, never accepted.
    pub fn propose(&self, proposal: &Proposal) -> Result<Applied, GraphClientError> {
        self.run(PROPOSE, proposal.payload())
    }

    /// Decide one proposal revision as an enumerated reviewer: an approval
    /// is accepted (the proposal's event) or `rejected proposal_stale`; a
    /// rejection is `proposed` with the revision's new status.
    pub fn decide(&self, decision: &Decision) -> Result<Applied, GraphClientError> {
        self.run(DECIDE, decision.payload())
    }

    /// Acknowledge the accepted event `target` as a human reviewer: one new
    /// review event; the acknowledged event never changes and stays
    /// reversible.
    pub fn acknowledge(&self, target: &Target) -> Result<Applied, GraphClientError> {
        self.run(ACKNOWLEDGE, review::acknowledge_payload(target))
    }

    /// Reverse the event `reverse` names under the revisions it expects
    /// ([`Reverse::of`] a preview's plan). A precondition, dependency or
    /// race failure is a conflict ([`GraphOutcome::code`]:
    /// `reversal_conflict`), never a partial or forced reversal.
    pub fn reverse(&self, reverse: &Reverse) -> Result<Applied, GraphClientError> {
        self.run(REVERSE, reverse.payload())
    }

    /// Mint, send and resolve one `descriptor` command (the rules of
    /// [`Self::apply`]).
    fn run(&self, descriptor: &str, payload: Value) -> Result<Applied, GraphClientError> {
        self.online()?;
        let prepared = self
            .mint_command(descriptor, payload)
            .map_err(GraphClientError::Refused)?;
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
        // The typed reads name no context: a person's only.
        if self.caller != Caller::Person {
            return Err(GraphClientError::PersonOnly);
        }
        self.online()?;
        Ok(self.requests.port().remote_query(operation, params)?)
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

    /// One page of the unreviewed feed after graph position `after`: the
    /// agent-origin effect events no review targets (persons with the
    /// reviewer role only); resume from its `scanned_through`.
    pub fn unreviewed(&self, after: &str, limit: u32) -> Result<HistoryPage, GraphClientError> {
        let params =
            wire::unreviewed_params(None, after, limit).map_err(GraphClientError::Refused)?;
        let rows = self.read(wire::UNREVIEWED, params)?;
        decode_history(after, limit, rows).map_err(GraphClientError::Unreadable)
    }

    /// What reversing the accepted event `event_id` would do now: its plan,
    /// the revisions it requires and whether it would apply. Inspection
    /// only: neither an approval nor a reservation.
    pub fn preview_reversal(&self, event_id: &str) -> Result<Preview, GraphClientError> {
        let params = wire::preview_params(None, event_id).map_err(GraphClientError::Refused)?;
        let rows = self.read(wire::PREVIEW_REVERSAL, params)?;
        inverse::decode_preview(event_id, rows).map_err(GraphClientError::Unreadable)
    }

    /// The revisions of proposal `proposal_id` this person reviews (none
    /// when not enumerated on any, or not authorised now).
    pub fn proposal(&self, proposal_id: &str) -> Result<Vec<ProposalRow>, GraphClientError> {
        let params = wire::proposal_params(None, proposal_id).map_err(GraphClientError::Refused)?;
        let rows = self.read(wire::PROPOSAL, params)?;
        decode_proposals(proposal_id, rows).map_err(GraphClientError::Unreadable)
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
