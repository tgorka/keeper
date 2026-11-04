//! The direct-JCS event digest must accept, refuse and hash exactly as the previous
//! `to_value` + `canonical_bytes` construction did.
use keeper_graph_protocol::wire::{
    canonical_bytes, operation_digest, Change, EntityType, Operation, MAX_EVENT_BYTES,
};
use keeper_graph_protocol::Refusal;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The previous implementation, kept here as the reference.
fn reference(operations: &[Operation]) -> Result<String, Refusal> {
    if operations.len() > 128 {
        return Err(Refusal::QueryLimit);
    }
    let value = serde_json::to_value(operations).map_err(|_| Refusal::QueryUnsupported)?;
    let bytes = canonical_bytes(&value)?;
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(Refusal::QueryLimit);
    }
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn op(ordinal: u32, before: Option<Value>, after: Option<Value>) -> Operation {
    Operation {
        ordinal,
        entity_type: EntityType::Node,
        entity_id: "01J00000000000000000000000".into(),
        operation: if after.is_some() {
            Change::Put
        } else {
            Change::Remove
        },
        before,
        after,
    }
}

fn nested(depth: usize) -> Value {
    (0..depth).fold(json!(1), |inner, _| json!([inner]))
}

#[test]
fn direct_jcs_digest_matches_the_value_tree_digest_on_acceptance_and_refusal() {
    let mut cases: Vec<Vec<Operation>> = vec![
        vec![op(
            0,
            None,
            Some(json!({"id": "x", "title": "t", "properties": {"score": 1.0}})),
        )],
        vec![op(
            0,
            Some(json!({"a": -0.0, "b": 1e15, "c": 0.1})),
            Some(json!({"é": "\u{1F600}", "a": [1, 2.5, null, true]})),
        )],
        vec![op(
            0,
            None,
            Some(json!({"": "", "\u{10000}": 1, "\u{FFFF}": 2})),
        )],
        (0..128)
            .map(|i| op(i, None, Some(json!({"i": i}))))
            .collect(),
    ];
    // Depth: the value sits at depth 2 of the operation-array tree (limit 16).
    for depth in 12..=16 {
        cases.push(vec![op(0, None, Some(nested(depth)))]);
    }
    let mut long_id = op(0, None, Some(json!({})));
    long_id.entity_id = "x".repeat(65537);
    cases.push(vec![long_id]);
    cases.push(vec![op(0, None, Some(json!("y".repeat(65537))))]);
    cases.push(vec![op(0, None, Some(Value::Array(vec![json!(0); 8193])))]);
    let wide: serde_json::Map<String, Value> =
        (0..8193).map(|i| (i.to_string(), json!(i))).collect();
    cases.push(vec![op(0, None, Some(Value::Object(wide)))]);
    cases.push(vec![op(0, None, Some(json!({"k".repeat(8193): 1})))]);
    cases.push(vec![op(0, None, Some(json!(9_007_199_254_740_993.0_f64)))]);
    cases.push((0..129).map(|i| op(i, None, Some(json!(i)))).collect());
    let mut accepted = 0;
    for (i, operations) in cases.iter().enumerate() {
        let expected = reference(operations);
        let actual = operation_digest(operations);
        assert_eq!(
            format!("{actual:?}"),
            format!("{expected:?}"),
            "case {i} diverged"
        );
        accepted += usize::from(actual.is_ok());
    }
    // Both sides of every boundary were exercised.
    assert!(
        accepted >= 5 && accepted < cases.len(),
        "accepted {accepted} of {}",
        cases.len()
    );
}
