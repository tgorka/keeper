//! Scope values and row keys of the graph tables (contract §9.2).
//!
//! | tables | scope variable | value |
//! | --- | --- | --- |
//! | `kg_nodes`, `kg_edges`, `kg_tags`, `kg_node_tags`, `kg_contents` (the state) | `kg_graph` | the database's (domain's) name |
//! | `kg_events`, `kg_event_ops`, `kg_head`, `kg_reviews` (the ledger) | `kg_history` | the database's (domain's) name |
//! | `kg_receipts` | `kg_client` | [`client_scope`] of the requesting actor and client |
//! | `kg_proposals` | `kg_proposal` | the proposer's actor id: a replica subscribes to its own actor's only |
//!
//! The state and the ledger were one variable, `kg_graph`, through schema
//! version 8; version 9 (story 115.6) puts the ledger under `kg_history`, so a
//! replica holds the state without the history unless it asks for both.
//! `kg_head` names the event it points at, and a reference never crosses a
//! scope boundary, so the head is the ledger's. No other variable reaches
//! these tables. An enumerated reviewer reads a proposal through an
//! authorised remote query, never through `kg_proposal`. Bindings, contexts,
//! session sources, authority epochs and reference indexes are the server's
//! own tables and have no scope at all.

/// The `kg_graph` scope variable: the graph's state.
pub const GRAPH_VARIABLE: &str = "kg_graph";
/// The `kg_history` scope variable: the graph's ledger.
pub const HISTORY_VARIABLE: &str = "kg_history";
/// The `kg_client` scope variable.
pub const CLIENT_VARIABLE: &str = "kg_client";

/// The `kg_proposal` scope variable.
pub const PROPOSAL_VARIABLE: &str = "kg_proposal";

/// Every table whose scope value is the domain's name, in bootstrap order:
/// [`HISTORY_TABLES`] and [`STATE_TABLES`] together.
pub const GRAPH_TABLES: [&str; 9] = [
    "kg_events",
    "kg_event_ops",
    "kg_head",
    "kg_nodes",
    "kg_edges",
    "kg_tags",
    "kg_node_tags",
    "kg_contents",
    "kg_reviews",
];
/// The ledger: the tables scoped by `kg_history`.
pub const HISTORY_TABLES: [&str; 4] = ["kg_events", "kg_event_ops", "kg_head", "kg_reviews"];
/// The state: the tables scoped by `kg_graph`.
pub const STATE_TABLES: [&str; 5] = [
    "kg_nodes",
    "kg_edges",
    "kg_tags",
    "kg_node_tags",
    "kg_contents",
];
/// The table scoped by `kg_client`.
pub const RECEIPT_TABLE: &str = "kg_receipts";
/// The table scoped by `kg_proposal`.
pub const PROPOSAL_TABLE: &str = "kg_proposals";

/// The `kg_client` value of one actor's client:
/// `<a>:<actor_id><c>:<client_id>`, `a` and `c` the UTF-8 byte lengths of
/// each id in decimal, so no two (actor, client) pairs share a value
/// whatever the ids hold.
#[must_use]
pub fn client_scope(actor_id: &str, client_id: &str) -> String {
    format!(
        "{}:{actor_id}{}:{client_id}",
        actor_id.len(),
        client_id.len()
    )
}

/// A receipt's row id: `<descriptor>/<request_id>/<client_scope>`. The
/// descriptor and the request id (a ULID) hold no `/`, so the id splits one
/// way only.
#[must_use]
pub fn receipt_id(descriptor: &str, request_id: &str, client_scope: &str) -> String {
    format!("{descriptor}/{request_id}/{client_scope}")
}

/// An event operation's row id: `<event_id>/<ordinal>`.
#[must_use]
pub fn event_op_id(event_id: &str, ordinal: u32) -> String {
    format!("{event_id}/{ordinal}")
}
