//! The two pure functions of the graph's write operations (DR-6, DR-8,
//! CR-4, SR-19): `to_proposal` turns a `graph.apply` input into
//! `graph.propose`'s, and the wire gate decides what a caller on the wire
//! may write directly, only as a proposal, or not at all.

use std::collections::BTreeSet;

use keeper_runtime_api::{Facts, Gate, OpError};
use serde_json::{json, Value};

/// The node kinds a wire caller changes only through a proposal.
const GATED_KINDS: [&str; 2] = ["Guidance", "Vocabulary"];
/// The edge type a wire caller changes only through a proposal.
const GATED_EDGE: &str = "guidance_scope";

fn gated_kind(kind: &str) -> bool {
    GATED_KINDS.contains(&kind)
}

fn text<'a>(op: &'a Value, key: &str) -> Option<&'a str> {
    op.get(key).and_then(Value::as_str)
}

/// `graph.apply`'s input → `graph.propose`'s, before the dispatcher's
/// fixed fields (`proposal_id`, `base_revision`, `reviewers`,
/// `expires_at_ms`): `{"target", "change": <the payload>}`.
pub fn to_proposal(input: &Value) -> Result<Value, OpError> {
    let Some(fields) = input.as_object() else {
        return Err(OpError::invalid_argument("", "the input is not an object"));
    };
    let Some(target) = fields.get("target") else {
        return Err(OpError::invalid_argument(
            "target",
            "missing field `target`",
        ));
    };
    let change: serde_json::Map<String, Value> = fields
        .iter()
        .filter(|(key, _)| key.as_str() != "target")
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    Ok(json!({"target": target, "change": change}))
}

/// The gate over a change payload (`{ops, ...}`; a `target` beside it is
/// ignored). `Refuse` a change holding `verify_assertion`; `ProposeOnly` a
/// change that creates or changes a `Guidance` or `Vocabulary` node, names
/// one the replica knows as such, names an edge of type `guidance_scope` or
/// with such an end, or names a node or an edge the change does not create
/// and the replica does not know (CR-4); otherwise `Pass`.
pub fn wire_gate(input: &Value, facts: &dyn Facts) -> Gate {
    let ops: &[Value] = input
        .get("ops")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    if ops
        .iter()
        .any(|op| text(op, "op") == Some("verify_assertion"))
    {
        return Gate::Refuse("verify_assertion".into());
    }
    // What this change creates, and the kinds it creates them with.
    let mut created_nodes = std::collections::BTreeMap::new();
    let mut created_edges = BTreeSet::new();
    for op in ops {
        match text(op, "op") {
            Some("create_node") => {
                if let Some(id) = text(op, "id") {
                    created_nodes.insert(id, text(op, "kind").unwrap_or_default());
                }
            }
            Some("create_edge") => {
                if let Some(id) = text(op, "id") {
                    created_edges.insert(id);
                }
            }
            _ => {}
        }
    }
    let mut verdict: Option<String> = None;
    let mut propose = |reason: String| {
        verdict.get_or_insert(reason);
    };
    // A node named: gated if created as, or known as, a gated kind; unknown
    // if neither created here nor known to the replica.
    let node = |id: &str, propose: &mut dyn FnMut(String)| -> Option<String> {
        if let Some(kind) = created_nodes.get(id) {
            if gated_kind(kind) {
                propose(format!("gated_kind:{kind}"));
            }
            return Some((*kind).to_owned());
        }
        match facts.node_kind(id) {
            Some(kind) => {
                if gated_kind(&kind) {
                    propose(format!("gated_kind:{kind}"));
                }
                Some(kind)
            }
            None => {
                propose("unknown_node".into());
                None
            }
        }
    };
    for op in ops {
        let name = text(op, "op").unwrap_or_default();
        match name {
            "create_node" => {
                let kind = text(op, "kind").unwrap_or_default();
                if gated_kind(kind) {
                    propose(format!("gated_kind:{kind}"));
                }
            }
            "update_node" | "archive_node" | "unarchive_node" => {
                if let Some(id) = text(op, "id") {
                    node(id, &mut propose);
                }
            }
            "add_tag" | "remove_tag" => {
                if let Some(id) = text(op, "node_id") {
                    node(id, &mut propose);
                }
            }
            "create_content" | "update_content" => {
                if let Some(id) = text(op, "owner_node_id") {
                    node(id, &mut propose);
                }
            }
            "create_edge" => {
                if text(op, "type") == Some(GATED_EDGE) {
                    propose(format!("gated_edge:{GATED_EDGE}"));
                }
                for end in ["from_id", "to_id"] {
                    if let Some(id) = text(op, end) {
                        node(id, &mut propose);
                    }
                }
            }
            "update_edge" | "archive_edge" | "unarchive_edge" => {
                if let Some(id) = text(op, "id") {
                    edge(id, &created_edges, facts, &mut propose);
                }
            }
            "merge_nodes" => {
                if let Some(id) = text(op, "target_id") {
                    node(id, &mut propose);
                }
                for absorbed in op
                    .get("absorbed")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = text(absorbed, "id") {
                        node(id, &mut propose);
                    }
                }
                for rewritten in op
                    .get("edges")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = text(rewritten, "id") {
                        edge(id, &created_edges, facts, &mut propose);
                    }
                }
            }
            // An op the parser refuses anyway: never passed on the gate's word.
            _ => propose("unknown_op".into()),
        }
    }
    match verdict {
        Some(reason) => Gate::ProposeOnly(reason),
        None => Gate::Pass,
    }
}

/// An edge named by its id alone: its type and ends come from the replica.
fn edge(id: &str, created: &BTreeSet<&str>, facts: &dyn Facts, propose: &mut dyn FnMut(String)) {
    if created.contains(id) {
        return;
    }
    match facts.edge(id) {
        None => propose("unknown_edge".into()),
        Some(edge) => {
            if edge.edge_type == GATED_EDGE {
                propose(format!("gated_edge:{GATED_EDGE}"));
            }
            for kind in [&edge.from_kind, &edge.to_kind] {
                match kind {
                    Some(kind) if gated_kind(kind) => propose(format!("gated_kind:{kind}")),
                    Some(_) => {}
                    // An end the replica does not know: never passed.
                    None => propose("unknown_node".into()),
                }
            }
        }
    }
}

/// `graph.propose`'s gate: the same gate over its `/change`.
pub fn propose_gate(input: &Value, facts: &dyn Facts) -> Gate {
    wire_gate(input.get("change").unwrap_or(&Value::Null), facts)
}
