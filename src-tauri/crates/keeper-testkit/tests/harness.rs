//! The harness's own client: a module's test over the fake, with nothing
//! but keeper-ports and keeper-testkit in its binary.

use keeper_ports::{BlobBody, Rejection, RowOp};
use keeper_testkit::{blob_id, harness, Over};
use serde_json::{json, Value};

fn attachment(id: &str, bytes: &[u8]) -> RowOp {
    let Value::Object(values) = json!({
        "id": id,
        "blob_ref": json!({"blobId": blob_id(bytes), "byteLength": bytes.len()}).to_string(),
    }) else {
        unreachable!("a literal object");
    };
    RowOp::Upsert {
        table: "attachments".to_owned(),
        values,
        base_version: None,
    }
}

#[test]
fn a_module_over_the_fake_reads_back_what_it_committed_and_the_refusal_a_test_sets() {
    let world = harness(Over::Fake);
    let replica = &world.source.replica;
    let bytes = b"a body".to_vec();
    let id = blob_id(&bytes);
    let commit = replica
        .commit_with_blobs(
            vec![BlobBody {
                id: id.clone(),
                bytes: bytes.clone(),
                media_type: None,
            }],
            vec![attachment("a-1", &bytes)],
        )
        .expect("commits");
    assert_eq!(replica.cached_blob(&id).expect("cached"), bytes);
    assert_eq!(world.queued(), vec![id.clone()]);
    assert_eq!(world.pins(), vec![(commit.clone(), id.clone())]);
    assert_eq!(replica.state().outbox, 1);

    let fake = world.fake.as_ref().expect("the fake");
    fake.refuse_upload(&id, Rejection::new("blob.too_large", "too large"));
    let refusals = replica.upload_refusals();
    assert_eq!(refusals.len(), 1);
    assert_eq!(
        (refusals[0].commit_id.as_str(), refusals[0].blob_id.as_str()),
        (commit.as_str(), id.as_str())
    );
    assert!(world.queued().is_empty(), "released from the queue");
    assert!(world.pins().is_empty(), "and from its commit's pins");
}
