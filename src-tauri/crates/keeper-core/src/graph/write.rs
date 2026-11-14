//! Where a native caller's graph requests go: the narrow [`GraphWrite`]
//! port (112.1 CC-1), beside the read ports [`super::source::GraphSource`]
//! and [`super::query::GraphRemote`].
//!
//! [`super::outcome::GraphRequests`], [`super::client::GraphClient`] and the
//! intent queue reach a database only through it: a command send, a read of
//! this client's own receipt row, a remote query (`graph.outcome` and the
//! typed reads) and the link's state. keeper's [`Source`] implements it over
//! its replica; the trusted agent runtime implements it over its own remote
//! and replica channels without implementing the whole
//! [`Replica`](crate::hq::port::Replica) trait. There is one command path
//! and one decoder ([`super::outcome`]) whichever side implements it.

use serde_json::{Map, Value};

use crate::hq::port::{CommandResult, LinkState, ReplicaError};
use crate::hq::sessions::Source;

/// One database's graph write boundary.
pub trait GraphWrite: Send + Sync {
    /// Send the `descriptor` command `request_id` with its version-1
    /// `envelope`, once.
    fn command(
        &self,
        descriptor: &str,
        request_id: &str,
        envelope: Value,
    ) -> Result<CommandResult, ReplicaError>;
    /// The replicated `kg_receipts` row whose id is `receipt_id`, if this
    /// side replicates receipts (an agent runtime does not: none).
    fn receipt(&self, receipt_id: &str) -> Result<Vec<Map<String, Value>>, ReplicaError>;
    /// One remote query (`graph.outcome`, a typed read) and its rows.
    fn remote_query(
        &self,
        operation: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError>;
    /// The link's state now.
    fn link(&self) -> LinkState;
}

/// Who sends through a [`GraphWrite`]: the database (domain) and this
/// device's (or agent instance's) actor and client in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Writer {
    pub domain: String,
    pub actor: String,
    pub client: String,
}

impl Writer {
    /// The identity `source` was opened under.
    #[must_use]
    pub fn of(source: &Source) -> Self {
        Self {
            domain: source.database.clone(),
            actor: source.actor.clone(),
            client: source.client.clone(),
        }
    }
}

/// On whose authority a request is sent: a person's device (no context;
/// re-authorised at acceptance) or an agent instance under the context it
/// holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Caller {
    Person,
    Agent { context_id: String },
}

impl Caller {
    /// The context every request of this caller names.
    #[must_use]
    pub fn context_id(&self) -> Option<&str> {
        match self {
            Self::Person => None,
            Self::Agent { context_id } => Some(context_id),
        }
    }
}

impl GraphWrite for Source {
    fn command(
        &self,
        descriptor: &str,
        request_id: &str,
        envelope: Value,
    ) -> Result<CommandResult, ReplicaError> {
        self.replica
            .remote_command(descriptor, request_id, envelope)
    }

    fn receipt(&self, receipt_id: &str) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        self.replica.query(
            "SELECT * FROM kg_receipts WHERE id = ?",
            &[Value::from(receipt_id)],
        )
    }

    fn remote_query(
        &self,
        operation: &str,
        params: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        self.replica.remote_query(operation, params)
    }

    fn link(&self) -> LinkState {
        self.replica.state().link
    }
}
