//! A delta built natively and the same delta read from its wire form are
//! judged alike: what one digests the other digests to the same value, and
//! what one refuses the other refuses.

use keeper_graph_protocol::delta::{digest, Op};
use keeper_graph_protocol::vocabulary::{EntityType, Operation};
use keeper_graph_protocol::Refusal;
use serde_json::{json, Value};

const NODE: &str = "01JD0000000000000000000001";

fn create(after_value: Option<Value>) -> Op {
    Op {
        ordinal: 0,
        entity_type: EntityType::Node,
        entity_id: NODE.into(),
        operation: Operation::Create,
        before_revision: None,
        after_revision: Some("1".into()),
        before_value: None,
        after_value,
    }
}

/// The digest of `ops` once they have crossed the wire.
fn wire_digest(ops: &[Op]) -> Result<String, Refusal> {
    let wire = serde_json::to_string(ops).expect("serialise");
    let read: Vec<Op> = serde_json::from_str(&wire).map_err(|_| Refusal::InvalidChange)?;
    digest(&read)
}

#[test]
fn a_native_delta_and_its_wire_form_agree() {
    // Explicit nulls inside an image are values and survive the round trip.
    let ops = [create(Some(
        json!({"kind": "Concept", "title": "A", "properties": {"gone": null, "n": 1.0}}),
    ))];
    let native = digest(&ops).expect("native");
    assert_eq!(wire_digest(&ops), Ok(native));
    for image in [Value::Null, json!(1), json!("x"), json!([])] {
        let ops = [create(Some(image.clone()))];
        assert_eq!(digest(&ops), Err(Refusal::InvalidChange), "native {image}");
        assert_eq!(
            wire_digest(&ops),
            Err(Refusal::InvalidChange),
            "wire {image}"
        );
    }
}
