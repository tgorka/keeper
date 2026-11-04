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

#[derive(Debug, PartialEq)]
enum Expect {
    Accepted,
    Limit,
    Unsupported,
}

fn with_id(mut operation: Operation, id: String) -> Operation {
    operation.entity_id = id;
    operation
}

#[test]
fn direct_jcs_digest_matches_the_value_tree_digest_on_acceptance_and_refusal() {
    use Expect::{Accepted, Limit, Unsupported};
    let max = 9_007_199_254_740_991.0_f64;
    let over = 9_007_199_254_740_993.0_f64; // rounds to MAX_NUMBER + 1
    let wide =
        |n: usize| -> Value { Value::Object((0..n).map(|i| (i.to_string(), json!(i))).collect()) };
    // A value sits at depth 2 of the operation-array tree; the limit refuses depth > 16.
    let cases: Vec<(&str, Vec<Operation>, Expect)> = vec![
        (
            "plain",
            vec![op(
                0,
                None,
                Some(json!({"id": "x", "title": "t", "properties": {"score": 1.0}})),
            )],
            Accepted,
        ),
        (
            "numbers_and_unicode",
            vec![op(
                0,
                Some(json!({"a": -0.0, "b": 1e15, "c": 0.1})),
                Some(json!({"é": "\u{1F600}", "a": [1, 2.5, null, true]})),
            )],
            Accepted,
        ),
        (
            "utf16_key_order",
            vec![op(
                0,
                None,
                Some(json!({"": "", "\u{10000}": 1, "\u{FFFF}": 2})),
            )],
            Accepted,
        ),
        ("depth_16", vec![op(0, None, Some(nested(14)))], Accepted),
        ("depth_17", vec![op(0, None, Some(nested(15)))], Limit),
        (
            "string_65536",
            vec![op(0, None, Some(json!("y".repeat(65536))))],
            Accepted,
        ),
        (
            "string_65537",
            vec![op(0, None, Some(json!("y".repeat(65537))))],
            Limit,
        ),
        (
            "array_8192",
            vec![op(0, None, Some(Value::Array(vec![json!(0); 8192])))],
            Accepted,
        ),
        (
            "array_8193",
            vec![op(0, None, Some(Value::Array(vec![json!(0); 8193])))],
            Limit,
        ),
        ("object_8192", vec![op(0, None, Some(wide(8192)))], Accepted),
        ("object_8193", vec![op(0, None, Some(wide(8193)))], Limit),
        (
            "key_8192",
            vec![op(0, None, Some(json!({"k".repeat(8192): 1})))],
            Accepted,
        ),
        (
            "key_8193",
            vec![op(0, None, Some(json!({"k".repeat(8193): 1})))],
            Limit,
        ),
        ("number_max", vec![op(0, None, Some(json!(max)))], Accepted),
        (
            "number_over",
            vec![op(0, None, Some(json!(over)))],
            Unsupported,
        ),
        (
            "entity_id_65536",
            vec![with_id(op(0, None, Some(json!({}))), "x".repeat(65536))],
            Accepted,
        ),
        (
            "entity_id_65537",
            vec![with_id(op(0, None, Some(json!({}))), "x".repeat(65537))],
            Limit,
        ),
        (
            "operations_128",
            (0..128)
                .map(|i| op(i, None, Some(json!({"i": i}))))
                .collect(),
            Accepted,
        ),
        (
            "operations_129",
            (0..129).map(|i| op(i, None, Some(json!(i)))).collect(),
            Limit,
        ),
        // Final encoded-size cap: 31 vs 32 operations of one 65536-byte string each
        // straddle MAX_EVENT_BYTES (2 MiB) with every per-value limit satisfied.
        (
            "encoded_under_cap",
            (0..31)
                .map(|i| op(i, None, Some(json!("z".repeat(65536)))))
                .collect(),
            Accepted,
        ),
        (
            "encoded_over_cap",
            (0..32)
                .map(|i| op(i, None, Some(json!("z".repeat(65536)))))
                .collect(),
            Limit,
        ),
        // Mixed invalidity: the old Value tree visits `after`, then `before`, then
        // `entity_id` (sorted keys), and operations in order; the first refusal wins.
        (
            "mixed_before_long_after_number",
            vec![op(0, Some(json!("y".repeat(65537))), Some(json!(over)))],
            Unsupported,
        ),
        (
            "mixed_before_number_after_long",
            vec![op(0, Some(json!(over)), Some(json!("y".repeat(65537))))],
            Limit,
        ),
        (
            "mixed_long_id_after_number",
            vec![with_id(op(0, None, Some(json!(over))), "x".repeat(65537))],
            Unsupported,
        ),
        (
            "mixed_long_id_before_deep",
            vec![with_id(op(0, Some(nested(15)), None), "x".repeat(65537))],
            Limit,
        ),
        (
            "mixed_across_operations",
            vec![
                with_id(op(0, None, Some(json!({}))), "x".repeat(65537)),
                op(1, None, Some(json!(over))),
            ],
            Limit,
        ),
        (
            "mixed_across_operations_reversed",
            vec![
                op(0, None, Some(json!(over))),
                with_id(op(1, None, Some(json!({}))), "x".repeat(65537)),
            ],
            Unsupported,
        ),
    ];
    for (name, operations, expect) in &cases {
        let reference = reference(operations);
        let actual = operation_digest(operations);
        assert_eq!(
            format!("{actual:?}"),
            format!("{reference:?}"),
            "{name}: diverged from the Value-tree construction"
        );
        let outcome = match actual {
            Ok(_) => Accepted,
            Err(Refusal::QueryLimit) => Limit,
            Err(Refusal::QueryUnsupported) => Unsupported,
        };
        assert_eq!(&outcome, expect, "{name}");
    }
}
