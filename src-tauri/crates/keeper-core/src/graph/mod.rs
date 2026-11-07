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
//! [`client::GraphClient`] is a person's device's use of one database's
//! graph over it: changes through `graph.apply`, outcomes, typed reads.
//! [`prefix::complete`] reads the complete accepted prefix of a domain's
//! replicated history through the narrow [`source::GraphSource`] port.
//!
//! Queries (110.5): [`query::GraphReads`] answers bounded `KG-Cypher/1`
//! reads from a local projection of that prefix ([`projection`]), built and
//! queried by the separate single-use worker `keeper-graph-query`
//! ([`supervisor`]) only in an admitted runtime class ([`runtime`],
//! [`ledger`]), under the process's [`admission::Admission`]. No graph
//! engine is linked here. Desktop only: [`available`] is false on mobile.

pub use keeper_graph_protocol as protocol;

pub mod client;
pub mod outcome;
pub mod prefix;
pub mod queue;
pub mod source;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod admission;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod ledger;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod lifecycle;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod projection;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod query;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod runtime;
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub mod supervisor;

/// Whether this target runs graph queries at all (Linux and macOS
/// desktops). Elsewhere a query is unsupported.
#[must_use]
pub fn available() -> bool {
    cfg!(any(target_os = "linux", target_os = "macos"))
}

#[cfg(test)]
mod client_tests;
#[cfg(test)]
mod contract_tests;
#[cfg(test)]
mod outcome_tests;
