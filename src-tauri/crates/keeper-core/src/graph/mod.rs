//! The knowledge graph's native client contract.
//!
//! Graph data lives in the agents' database (`syncular/`, schema 6): the
//! server's graph commands write it, every replica allowed to read a
//! database's graph receives it under the `kg_graph` scope, and a client
//! reads the outcome of its own requests under `kg_client`. The shared rules —
//! vocabulary, canonical values, digests, scope values — are
//! [`protocol`] (`keeper-graph-protocol`). A tag's canonical text is
//! [`crate::notes::tags::normalise`]'s answer, the one tag policy keeper has.
//!
//! [`outcome::GraphRequests`] is one database's owner of this device's
//! graph requests: it mints their keys, sends them, reads their receipts
//! from that database's own replica and turns them into what the client may
//! conclude, by the contract's outcome table. An acknowledgement alone never
//! means accepted, and nothing of another database speaks for a request.

pub use keeper_graph_protocol as protocol;

pub mod outcome;

#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod outcome_tests;
