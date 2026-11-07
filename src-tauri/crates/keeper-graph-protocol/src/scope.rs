//! Scope values and row keys of the graph tables (contract §9.2).
//!
//! | tables | scope variable | value |
//! | --- | --- | --- |
//! | `kg_events`, `kg_event_ops`, `kg_head`, `kg_nodes`, `kg_edges`, `kg_tags`, `kg_node_tags`, `kg_contents` | `kg_graph` | the database's (domain's) name |
//! | `kg_receipts` | `kg_client` | [`client_scope`] of the requesting actor and client |
//!
//! No other variable reaches these tables. Bindings, contexts, session
//! sources, authority epochs and reference indexes are the server's own
//! tables and have no scope at all.

/// The `kg_graph` scope variable.
pub const GRAPH_VARIABLE: &str = "kg_graph";
/// The `kg_client` scope variable.
pub const CLIENT_VARIABLE: &str = "kg_client";

/// The tables scoped by `kg_graph`, in bootstrap order.
pub const GRAPH_TABLES: [&str; 8] = [
    "kg_events",
    "kg_event_ops",
    "kg_head",
    "kg_nodes",
    "kg_edges",
    "kg_tags",
    "kg_node_tags",
    "kg_contents",
];
/// The table scoped by `kg_client`.
pub const RECEIPT_TABLE: &str = "kg_receipts";

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
