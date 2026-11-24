//! The graph's catalogue against the contract (123.2 AC5, AC12): the
//! hand-written schemas against `syncular/knowledge-graph.vectors.json`,
//! and the two pure functions over their committed vectors files.

use std::path::{Path, PathBuf};

use keeper_graph_api::{propose_gate, to_proposal, wire_gate, OPERATIONS};
use keeper_graph_protocol::tag::normalise;
use keeper_graph_protocol::{change, proposal, value};
use keeper_runtime_api::{has_pointer, EdgeFacts, Effect, Facts, Gate, OperationSpec};
use serde_json::{json, Map, Value};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read_json(path: &str) -> Value {
    let text = std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn contract() -> Value {
    read_json("syncular/knowledge-graph.vectors.json")
}

fn op(id: &str) -> &'static OperationSpec {
    OPERATIONS
        .iter()
        .find(|spec| spec.id == id)
        .unwrap_or_else(|| panic!("{id}"))
}

fn validator(id: &str) -> jsonschema::Validator {
    jsonschema::draft202012::new((op(id).input)().as_value()).expect("a valid 2020-12 schema")
}

fn target() -> Value {
    json!({"server": "https://hq.example", "domain": "work"})
}

fn targeted(payload: &Value) -> Value {
    let mut input = payload.as_object().cloned().unwrap_or_default();
    input.insert("target".into(), target());
    Value::Object(input)
}

fn change_accepted(payload: &Value) -> bool {
    change::parse(payload, normalise).is_ok()
}

/// AC5: every change vector the rules accept validates against
/// `graph.apply`'s input schema (and `graph.enqueue`'s, the same one).
#[test]
fn accepted_changes_validate_against_the_apply_schema() {
    let apply = validator("graph.apply");
    let enqueue = validator("graph.enqueue");
    let mut accepted = 0;
    for entry in contract()["changes"].as_array().expect("changes") {
        let text = entry["json"].as_str().expect("json text");
        let Ok(payload) = value::parse(text.as_bytes()) else {
            continue;
        };
        if !change_accepted(&payload) {
            continue;
        }
        accepted += 1;
        let input = targeted(&payload);
        for (name, schema) in [("apply", &apply), ("enqueue", &enqueue)] {
            let errors: Vec<String> = schema.iter_errors(&input).map(|e| e.to_string()).collect();
            assert!(errors.is_empty(), "{name} {}: {errors:?}", entry["id"]);
        }
    }
    assert!(accepted >= 20, "only {accepted} accepted change vectors");
}

/// AC5: an extra or a missing top-level key is refused by the schema and
/// by `change::parse` alike.
#[test]
fn an_extra_or_missing_top_level_key_is_refused_by_both() {
    let apply = validator("graph.apply");
    let good = json!({"ops": [{"op": "create_node", "id": "01JF0000000000000000000005",
        "kind": "Concept", "title": "T", "description": null, "properties": {}}]});
    assert!(change_accepted(&good));
    assert!(apply.is_valid(&targeted(&good)));
    let mut extra = good.clone();
    extra["surprise"] = json!(1);
    let missing = json!({"approval_id": "a"});
    for (label, payload) in [("extra", extra), ("missing ops", missing)] {
        assert!(!change_accepted(&payload), "{label}: parse");
        assert!(!apply.is_valid(&targeted(&payload)), "{label}: schema");
    }
    // The target itself is required.
    assert!(!apply.is_valid(&good));
}

/// AC5: on the propose vectors, `graph.propose`'s schema accepts what
/// `proposal::parse` accepts.
#[test]
fn the_propose_schema_accepts_what_the_parser_accepts() {
    let propose = validator("graph.propose");
    let decide = validator("graph.decide");
    let (mut proposals, mut decisions) = (0, 0);
    for entry in contract()["proposals"].as_array().expect("proposals") {
        if let Some(payload) = entry.get("payload") {
            if proposal::parse(payload, normalise).is_ok() {
                proposals += 1;
                let errors: Vec<String> = propose
                    .iter_errors(&targeted(payload))
                    .map(|e| e.to_string())
                    .collect();
                assert!(errors.is_empty(), "{}: {errors:?}", entry["id"]);
            }
        }
        if let Some(payload) = entry.get("decide") {
            let parsed = proposal::parse_decide(payload).is_ok();
            decisions += 1;
            assert_eq!(
                decide.is_valid(&targeted(payload)),
                parsed,
                "decide {}",
                entry["id"]
            );
        }
    }
    assert!(proposals >= 4, "{proposals}");
    assert!(decisions >= 4, "{decisions}");
}

/// Every entry: a graph id, its wire pointers in its input schema, and the
/// four decide operations of class decide.
#[test]
fn every_operation_is_well_formed() {
    assert_eq!(OPERATIONS.len(), 17);
    for spec in OPERATIONS {
        assert!(spec.id.starts_with("graph."), "{}", spec.id);
        let input = (spec.input)();
        assert!(has_pointer(&input, "/target"), "{}", spec.id);
        for pointer in spec
            .wire_forbid
            .iter()
            .chain(spec.wire_fix.iter().map(|(p, _)| p))
        {
            assert!(has_pointer(&input, pointer), "{} {pointer}", spec.id);
        }
        for path in spec.vectors {
            assert!(root().join(path).is_file(), "{} {path}", spec.id);
        }
    }
    let mut decide: Vec<&str> = OPERATIONS
        .iter()
        .filter(|s| s.effect == Effect::Decide)
        .map(|s| s.id)
        .collect();
    decide.sort_unstable();
    assert_eq!(
        decide,
        [
            "graph.acknowledge",
            "graph.decide",
            "graph.discard",
            "graph.reverse"
        ]
    );
}

/// The replica's facts as a vectors case gives them.
struct CaseFacts(Value);

impl Facts for CaseFacts {
    fn node_kind(&self, id: &str) -> Option<String> {
        self.0["nodes"][id].as_str().map(str::to_owned)
    }
    fn edge(&self, id: &str) -> Option<EdgeFacts> {
        let edge = self.0["edges"].get(id)?;
        Some(EdgeFacts {
            edge_type: edge["type"].as_str()?.to_owned(),
            from_kind: edge["from_kind"].as_str().map(str::to_owned),
            to_kind: edge["to_kind"].as_str().map(str::to_owned),
        })
    }
}

fn expected(gate: &Gate) -> Value {
    match gate {
        Gate::Pass => json!("pass"),
        Gate::Refuse(reason) => json!({"refuse": reason}),
        Gate::ProposeOnly(reason) => json!({"propose_only": reason}),
    }
}

fn cases(path: &str) -> Vec<Value> {
    read_json(path)["cases"].as_array().cloned().expect("cases")
}

/// AC12: `to_proposal` of every change vector is the committed output,
/// and once the dispatcher's fixed fields are set, a payload
/// `proposal::parse` accepts.
#[test]
fn to_proposal_over_its_vectors() {
    let file = op("graph.apply").vectors[0];
    let cases = cases(file);
    assert!(!cases.is_empty());
    for case in cases {
        let input = &case["input"];
        let mut change = input.as_object().cloned().expect("object");
        change.remove("target");
        assert!(change_accepted(&Value::Object(change)), "{}", case["id"]);
        let output = to_proposal(input).expect("to_proposal");
        assert_eq!(output, case["expect"], "{}", case["id"]);
        let mut payload: Map<String, Value> = output.as_object().cloned().expect("object");
        payload.remove("target");
        payload.insert("proposal_id".into(), json!("01JF0000000000000000000099"));
        payload.insert("base_revision".into(), Value::Null);
        payload.insert("reviewers".into(), json!(["ana"]));
        payload.insert("expires_at_ms".into(), json!(1_700_000_000_000_u64));
        let parsed = proposal::parse(&Value::Object(payload), normalise);
        assert!(parsed.is_ok(), "{}: {parsed:?}", case["id"]);
    }
}

/// AC12: the gates over their vectors files: refuse `verify_assertion`,
/// `ProposeOnly` the four gated cases, pass an ordinary `create_node`.
#[test]
fn the_gates_over_their_vectors() {
    let required = [
        "create-node-passes",
        "verify-assertion-refused",
        "guidance-create-propose-only",
        "guidance-scope-update-edge-by-id",
        "guidance-scope-archive-edge-by-id",
        "unknown-node-propose-only",
    ];
    for (id, gate) in [
        ("graph.apply", wire_gate as fn(&Value, &dyn Facts) -> Gate),
        ("graph.enqueue", wire_gate),
        ("graph.propose", propose_gate),
    ] {
        let spec = op(id);
        assert_eq!(
            spec.wire_gate.map(|g| g as usize),
            Some(gate as usize),
            "{id}"
        );
        let file = spec
            .vectors
            .iter()
            .find(|p| p.ends_with(".wire_gate.json"))
            .unwrap_or_else(|| panic!("{id}: a wire_gate vectors file"));
        let cases = cases(file);
        for name in required {
            assert!(cases.iter().any(|c| c["id"] == name), "{file}: {name}");
        }
        for case in cases {
            let answer = gate(&case["input"], &CaseFacts(case["facts"].clone()));
            assert_eq!(expected(&answer), case["expect"], "{file} {}", case["id"]);
        }
    }
}
