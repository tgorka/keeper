//! The payload schemas, written by hand from the contract (A-4; MA/AD-1):
//! `change::parse`, `proposal::parse` and `proposal::parse_decide` read the
//! payloads by hand-written rules, so their schemas are written beside
//! them and tested against `syncular/knowledge-graph.vectors.json`. They
//! say which members an object has and of what JSON type; what needs more
//! than that (a ULID's alphabet, a property's value rules, sorted
//! reviewers) is the parser's alone.

use keeper_graph_protocol::change::{MAX_EVENT_TIME_MS, MAX_OPERATIONS};
use keeper_graph_protocol::proposal::MAX_REVIEWERS;
use serde_json::{json, Map, Value};

fn text() -> Value {
    json!({"type": "string"})
}

fn nullable_text() -> Value {
    json!({"type": ["string", "null"]})
}

fn object() -> Value {
    json!({"type": "object"})
}

/// An object with exactly `required` and at most `optional` besides.
fn exactly(required: &[(&str, Value)], optional: &[(&str, Value)]) -> Value {
    let mut properties = Map::new();
    for (name, schema) in required.iter().chain(optional) {
        properties.insert((*name).to_owned(), schema.clone());
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": required.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        "additionalProperties": false,
    })
}

fn op(name: &str, required: &[(&str, Value)], optional: &[(&str, Value)]) -> Value {
    let mut all = vec![("op", json!({"const": name}))];
    all.extend(required.iter().cloned());
    exactly(&all, optional)
}

fn edge_ends() -> [(&'static str, Value); 2] {
    [
        ("from_lifecycle_revision", nullable_text()),
        ("to_lifecycle_revision", nullable_text()),
    ]
}

fn list(items: Value, min: usize) -> Value {
    json!({"type": "array", "items": items, "minItems": min})
}

/// The `graph.apply` payload's operations (`change.rs`'s table).
fn ops() -> Vec<Value> {
    let [from, to] = edge_ends();
    let edge = |name: &str, optional: &[(&str, Value)]| {
        op(
            name,
            &[
                ("id", text()),
                ("expected_revision", text()),
                from.clone(),
                to.clone(),
            ],
            optional,
        )
    };
    let properties_change = [
        ("set_properties", object()),
        ("remove_properties", list(text(), 0)),
    ];
    let node_fields = exactly(
        &[],
        &[
            ("title", text()),
            ("description", nullable_text()),
            ("description_source", nullable_text()),
        ],
    );
    let revised = |id: &str| exactly(&[(id, text()), ("expected_revision", text())], &[]);
    vec![
        op(
            "create_node",
            &[
                ("id", text()),
                ("kind", text()),
                ("title", text()),
                ("description", nullable_text()),
                ("properties", object()),
            ],
            &[],
        ),
        op(
            "update_node",
            &[("id", text()), ("expected_revision", text())],
            &[
                ("set", node_fields),
                properties_change[0].clone(),
                properties_change[1].clone(),
            ],
        ),
        op(
            "archive_node",
            &[("id", text()), ("expected_revision", text())],
            &[],
        ),
        op(
            "unarchive_node",
            &[("id", text()), ("expected_revision", text())],
            &[],
        ),
        op(
            "create_edge",
            &[
                ("id", text()),
                ("from_id", text()),
                ("to_id", text()),
                ("type", text()),
                ("properties", object()),
                from.clone(),
                to.clone(),
            ],
            &[],
        ),
        edge("update_edge", &properties_change),
        edge("archive_edge", &[]),
        edge("unarchive_edge", &[]),
        op(
            "add_tag",
            &[
                ("node_id", text()),
                ("tag", text()),
                ("expected_revision", nullable_text()),
            ],
            &[],
        ),
        op(
            "remove_tag",
            &[
                ("node_id", text()),
                ("tag", text()),
                ("expected_revision", nullable_text()),
            ],
            &[],
        ),
        op(
            "merge_nodes",
            &[
                ("target_id", text()),
                ("target_revision", text()),
                ("absorbed", list(revised("id"), 1)),
                (
                    "edges",
                    list(
                        exactly(
                            &[
                                ("id", text()),
                                ("expected_revision", text()),
                                from.clone(),
                                to.clone(),
                            ],
                            &[],
                        ),
                        0,
                    ),
                ),
            ],
            &[],
        ),
        op(
            "create_content",
            &[
                ("id", text()),
                ("owner_node_id", text()),
                ("body", object()),
            ],
            &[],
        ),
        op(
            "update_content",
            &[
                ("id", text()),
                ("owner_node_id", text()),
                ("expected_revision", text()),
                ("body", object()),
            ],
            &[],
        ),
        op(
            "verify_assertion",
            &[
                ("id", text()),
                ("expected_revision", text()),
                (
                    "sources",
                    list(
                        exactly(
                            &[
                                ("edge_id", text()),
                                ("edge_revision", text()),
                                ("ref_id", text()),
                                ("ref_revision", text()),
                            ],
                            &[],
                        ),
                        0,
                    ),
                ),
                (
                    "contents",
                    list(exactly(&[("id", text()), ("revision", text())], &[]), 0),
                ),
            ],
            &[],
        ),
    ]
}

/// The `graph.apply` payload: `{ops, approval_id?, event_time_ms?}`.
/// Named members of an object schema.
type Members = Vec<(&'static str, Value)>;

fn change_members() -> (Members, Members) {
    (
        vec![(
            "ops",
            json!({
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_OPERATIONS,
                "items": {"oneOf": ops()},
            }),
        )],
        vec![
            ("approval_id", text()),
            (
                "event_time_ms",
                json!({"type": ["integer", "null"], "minimum": 0, "maximum": MAX_EVENT_TIME_MS}),
            ),
        ],
    )
}

/// The `graph.apply` payload's schema, without a target.
#[must_use]
pub fn change() -> Value {
    let (required, optional) = change_members();
    exactly(&required, &optional)
}

/// The `/target` member every graph input has.
#[must_use]
pub fn target() -> Value {
    exactly(&[("server", text()), ("domain", text())], &[])
}

fn with_target(mut required: Members, optional: &[(&str, Value)]) -> Value {
    required.push(("target", target()));
    let mut schema = exactly(&required, optional);
    if let Some(object) = schema.as_object_mut() {
        object.insert(
            "$schema".into(),
            json!("https://json-schema.org/draft/2020-12/schema"),
        );
    }
    schema
}

/// `graph.apply`'s and `graph.enqueue`'s input: the change payload and `target`.
#[must_use]
pub fn change_input() -> Value {
    let (required, optional) = change_members();
    with_target(required, &optional)
}

/// `graph.propose`'s input: `{proposal_id, base_revision, change, reviewers,
/// expires_at_ms}` and `target`.
#[must_use]
pub fn propose_input() -> Value {
    with_target(
        vec![
            ("proposal_id", text()),
            (
                "base_revision",
                json!({"type": ["string", "null"], "pattern": "^[1-9][0-9]{0,19}$"}),
            ),
            ("change", change()),
            (
                "reviewers",
                json!({"type": "array", "items": {"type": "string"}, "minItems": 1,
                       "maxItems": MAX_REVIEWERS, "uniqueItems": true}),
            ),
            (
                "expires_at_ms",
                json!({"type": ["integer", "null"], "minimum": 0}),
            ),
        ],
        &[],
    )
}

/// `graph.decide`'s input: `{proposal_id, proposal_revision,
/// proposal_digest, decision}` and `target`.
#[must_use]
pub fn decide_input() -> Value {
    with_target(
        vec![
            ("proposal_id", text()),
            (
                "proposal_revision",
                json!({"type": "string", "pattern": "^[1-9][0-9]{0,19}$"}),
            ),
            (
                "proposal_digest",
                json!({"type": "string", "pattern": "^[0-9a-f]{64}$"}),
            ),
            ("decision", json!({"enum": ["approve", "reject"]})),
        ],
        &[],
    )
}

/// A graph operation's output: a document whose `ok` says how it went, and
/// the members its success carries (AC6: removing one is a breaking change).
/// A member whose JSON type depends on the build or the server is left
/// untyped (`{}`); its presence is still the contract.
fn answer(members: &[(&str, Value)]) -> Value {
    let mut properties = Map::new();
    properties.insert("ok".to_owned(), json!({"type": "boolean"}));
    for (name, schema) in members {
        properties.insert((*name).to_owned(), schema.clone());
    }
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": ["ok"],
    })
}

fn any() -> Value {
    json!({})
}

/// `graph.enqueue`'s and `graph.discard`'s output: the entry and its state.
#[must_use]
pub fn queued_answer() -> Value {
    answer(&[("request_id", text()), ("state", text())])
}

/// `graph.pending`'s and `graph.resume`'s output: the queue's entries.
#[must_use]
pub fn entries_answer() -> Value {
    answer(&[("entries", list(object(), 0))])
}

fn sent_members() -> Members {
    vec![
        ("request_id", text()),
        ("descriptor", text()),
        ("outcome", object()),
        ("fenced", json!({"type": ["boolean", "null"]})),
    ]
}

/// A sent command's output (`graph.apply`, `graph.propose`, `graph.decide`,
/// `graph.acknowledge`): its request id, descriptor, outcome and fence.
#[must_use]
pub fn sent_answer() -> Value {
    answer(&sent_members())
}

/// `graph.reverse`'s output: a sent command's, or (not sent) the preview
/// and why.
#[must_use]
pub fn reverse_answer() -> Value {
    let mut members = sent_members();
    members.extend([("error", text()), ("preview", any())]);
    answer(&members)
}

/// `graph.sync`'s output.
#[must_use]
pub fn sync_answer() -> Value {
    answer(&[
        ("graph_seq", any()),
        ("built", any()),
        ("completeness", text()),
    ])
}

fn read_members() -> Members {
    vec![
        ("rows", any()),
        ("domain", text()),
        ("graph_seq", any()),
        ("checkpoint", any()),
        ("contract_schema", any()),
        ("projection_version", any()),
        ("engine", any()),
        ("completeness", text()),
    ]
}

/// `graph.query`'s output: the rows and the projection they were read from.
#[must_use]
pub fn query_answer() -> Value {
    answer(&read_members())
}

/// `graph.inspect`'s output: the projection's rows, or the typed reads'
/// (which say so in `source`).
#[must_use]
pub fn inspect_answer() -> Value {
    let mut members = read_members();
    members.push(("source", text()));
    answer(&members)
}

/// `graph.history`'s and `graph.unreviewed`'s output: one page.
#[must_use]
pub fn page_answer() -> Value {
    answer(&[("page", any())])
}

/// `graph.proposal`'s output.
#[must_use]
pub fn revisions_answer() -> Value {
    answer(&[("revisions", any())])
}

/// `graph.preview_reversal`'s output.
#[must_use]
pub fn preview_answer() -> Value {
    answer(&[("preview", any())])
}

/// `graph.status`'s output.
#[must_use]
pub fn status_answer() -> Value {
    answer(&[
        ("head", any()),
        ("checkpoint", any()),
        ("runtime", text()),
        ("fence", any()),
        ("queue", any()),
    ])
}
