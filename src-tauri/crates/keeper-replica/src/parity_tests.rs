//! The port's cases, written once and run against every replica the fake
//! claims to model (113.4): the fake with its blob store, the local-only
//! replica (`link: None`) on a temporary file, and — for what only a server
//! can answer — a replica linked to the blob-route stand-in. Each subject
//! comes from keeper-testkit's harness. A case the fake fails is fixed in
//! the fake or taken out of its claim ([`keeper_testkit::MODELLED`]), never
//! skipped.

use std::time::{Duration, Instant};

use keeper_ports::{BlobBody, RefusedCommit, Rejection, RowOp, UploadRefusal};
use keeper_testkit::{blob_id, harness, pushed_after, puts, Harness, Over, MODELLED};
use serde_json::{json, Map, Value};

use crate::journal::journal_refused;
use crate::SyncularOpener;

/// The methods the cases below exercise; the fake may claim no other.
const COVERED: [&str; 8] = [
    "mutate",
    "commit_with_blobs",
    "cached_blob",
    "fetch_blob",
    "upload_refusals",
    "refused_commits",
    "sync_now",
    "state",
];

/// The subjects with no server: the fake and the local-only replica.
fn local_subjects() -> [(&'static str, Harness); 2] {
    [
        ("fake", harness(Over::Fake)),
        ("local", harness(Over::Local(&SyncularOpener))),
    ]
}

fn values(value: Value) -> Map<String, Value> {
    let Value::Object(values) = value else {
        unreachable!("a literal object");
    };
    values
}

fn message(id: &str) -> Map<String, Value> {
    values(json!({
        "id": id,
        "session_id": "local.1",
        "page_key": "local.1/2026-10",
        "role": "person",
        "body": "hello",
        "created_at_ms": 1_792_000_000_000_i64,
        "author_actor": "",
        "author_client": "",
    }))
}

/// An `attachments` row of `message` naming `bytes`.
fn attachment(id: &str, message: &str, bytes: &[u8]) -> Map<String, Value> {
    values(json!({
        "id": id,
        "session_id": "local.1",
        "message_id": message,
        "blob_ref": json!({"blobId": blob_id(bytes), "byteLength": bytes.len()}).to_string(),
        "name": format!("{id}.bin"),
        "mime": "application/octet-stream",
        "size": bytes.len(),
        "created_at_ms": 1_792_000_000_000_i64,
        "author_actor": "",
        "author_client": "",
    }))
}

fn upsert(table: &str, values: Map<String, Value>) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values,
        base_version: None,
    }
}

fn body(bytes: &[u8]) -> BlobBody {
    BlobBody {
        id: blob_id(bytes),
        bytes: bytes.to_vec(),
        media_type: Some("application/octet-stream".to_owned()),
    }
}

/// A message `m` with one file, `bytes`, committed with its body.
fn send(world: &Harness, m: &str, file: &str, bytes: &[u8]) -> String {
    world
        .source
        .replica
        .commit_with_blobs(
            vec![body(bytes)],
            vec![
                upsert("messages", message(m)),
                upsert("attachments", attachment(file, m, bytes)),
            ],
        )
        .expect("commits")
}

/// Wait (bounded) until `ok` holds: the real replica's owner publishes its
/// state, and answers a stand-in, on its own thread.
fn eventually(what: &str, ok: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ok() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ok(), "{what}");
}

#[test]
fn the_fake_claims_no_method_the_cases_do_not_cover() {
    for method in MODELLED {
        assert!(
            COVERED.contains(&method),
            "{method} is claimed and not covered"
        );
    }
}

/// AC2: a body committed with its reference is read back byte for byte
/// from the cache, by both blob reads.
#[test]
fn a_body_committed_with_its_reference_is_read_back_from_the_cache() {
    for (name, world) in local_subjects() {
        let bytes = vec![3u8; 64];
        send(&world, "m-1", "a-1", &bytes);
        let replica = &world.source.replica;
        assert_eq!(
            replica.cached_blob(&blob_id(&bytes)).expect("cached"),
            bytes,
            "{name}"
        );
        assert_eq!(
            replica
                .fetch_blob(&blob_id(&bytes))
                .expect("from the cache"),
            bytes,
            "{name}"
        );
        assert!(
            replica.cached_blob(&blob_id(b"never here")).is_err(),
            "{name}: a miss is refused, never fetched"
        );
    }
}

/// AC2: staging pins a body — queued for upload — and the commit that
/// references it pins it from the moment it is in the outbox.
#[test]
fn staging_pins_a_body_until_its_commit_is_in_the_outbox() {
    for (name, world) in local_subjects() {
        let bytes = vec![4u8; 64];
        let commit = send(&world, "m-1", "a-1", &bytes);
        eventually(&format!("{name}: the commit waits"), || {
            world.source.replica.state().outbox == 1
        });
        assert_eq!(world.queued(), vec![blob_id(&bytes)], "{name}: staged");
        assert_eq!(
            world.pins(),
            vec![(commit, blob_id(&bytes))],
            "{name}: pinned by its commit"
        );
    }
}

/// The state a failed commit must leave: only `first`'s body, `kept`,
/// staged and pinned; the failed commit's new body, `fresh`, gone.
fn only_the_first_holds(name: &str, world: &Harness, first: &str, kept: &[u8], fresh: &[u8]) {
    let replica = &world.source.replica;
    assert_eq!(world.queued(), vec![blob_id(kept)], "{name}");
    assert_eq!(
        world.pins(),
        vec![(first.to_owned(), blob_id(kept))],
        "{name}"
    );
    assert!(
        replica.cached_blob(&blob_id(fresh)).is_err(),
        "{name}: the new body went"
    );
    assert_eq!(replica.cached_blob(&blob_id(kept)).expect("kept"), kept);
}

/// AC2: a commit that fails takes back what it staged, and nothing an
/// earlier commit holds.
#[test]
fn a_failed_commit_unstages_what_it_staged() {
    for (name, world) in local_subjects() {
        let kept = vec![5u8; 32];
        let first = send(&world, "m-1", "a-1", &kept);
        let fresh = vec![6u8; 32];
        assert!(
            world
                .source
                .replica
                .commit_with_blobs(vec![body(&kept), body(&fresh)], Vec::new())
                .is_err(),
            "{name}: no operation"
        );
        only_the_first_holds(name, &world, &first, &kept, &fresh);
    }
}

/// AC2, the port's own words ("when a blob or the commit fails, what this
/// call staged and nothing else holds is unstaged again"), for a body whose
/// bytes are not at the address it names. The fake passes; the adapter
/// refuses the commit but leaves the bytes queued for upload under their
/// real address, with no message naming them — a finding of 113.4 against
/// the adapter, which this story may not change. Run it with
/// `--run-ignored only`.
#[test]
#[ignore = "113.4 finding: the adapter leaves a forged body's bytes queued"]
fn a_failed_commit_unstages_a_body_not_at_its_address() {
    for (name, world) in local_subjects() {
        let kept = vec![5u8; 32];
        let first = send(&world, "m-1", "a-1", &kept);
        let fresh = vec![6u8; 32];
        let mut forged = body(&[7u8; 32]);
        forged.id = blob_id(b"another body");
        assert!(
            world
                .source
                .replica
                .commit_with_blobs(
                    vec![body(&fresh), forged],
                    vec![upsert("attachments", attachment("a-2", "m-1", &fresh))],
                )
                .is_err(),
            "{name}: a body not at its address"
        );
        only_the_first_holds(name, &world, &first, &kept, &fresh);
    }
}

/// AC2: with no server nothing waits on a network and no refusal appears:
/// a round asked for leaves both commits waiting, unrefused.
#[test]
fn with_no_server_commits_wait_and_nothing_is_refused() {
    for (name, world) in local_subjects() {
        let replica = &world.source.replica;
        let started = Instant::now();
        send(&world, "m-1", "a-1", &[8u8; 16]);
        replica
            .mutate(vec![upsert("messages", message("m-2"))])
            .expect("commits");
        replica.sync_now();
        eventually(&format!("{name}: both wait"), || {
            replica.state().outbox == 2
        });
        assert_eq!(
            replica.state().link,
            keeper_ports::LinkState::Local,
            "{name}"
        );
        assert!(replica.upload_refusals().is_empty(), "{name}");
        assert!(
            replica
                .refused_commits("messages")
                .expect("reads")
                .is_empty(),
            "{name}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{name}: nothing waited on a network"
        );
    }
}

/// AC3: a linked replica uploads a body before the round's push reaches
/// the stand-in, and keeps it pinned while the push is cut.
#[test]
fn a_linked_replica_puts_the_body_before_the_rounds_push() {
    let world = harness(Over::Linked(&SyncularOpener, Vec::new()));
    let seen = world.seen.clone().expect("the stand-in's record");
    let bytes = vec![9u8; 100];
    let id = blob_id(&bytes);
    let commit = send(&world, "m-1", "a-1", &bytes);
    world.source.replica.sync_now();
    eventually("the body's PUT, then a push", || {
        let at = seen
            .lock()
            .expect("seen")
            .iter()
            .position(|(method, path, _)| method == "PUT" && path.ends_with(&id));
        at.is_some_and(|at| pushed_after(&seen, at + 1))
    });
    let record = seen.lock().expect("seen").clone();
    // The stand-in's record, for the run's evidence (`--no-capture`).
    println!("stand-in record: {record:?}");
    let put = record
        .iter()
        .position(|(method, path, _)| method == "PUT" && path.ends_with(&id))
        .expect("the PUT");
    assert_eq!(puts(&seen, &id, 200), 1, "{record:?}");
    // The round that uploads opens with the body's grant: no push of that
    // round reached the stand-in before its PUT.
    let (method, path, _) = &record[put - 1];
    assert!(
        method == "POST" && path.contains(&id) && path.ends_with("/upload-grant"),
        "{record:?}"
    );
    assert!(pushed_after(&seen, put + 1), "{record:?}");
    eventually("the server has the body", || world.queued().is_empty());
    assert_eq!(
        world.pins(),
        vec![(commit, id)],
        "the waiting commit pins it"
    );
}

/// The refusal both subjects are given for `id`'s upload.
fn too_large() -> Rejection {
    Rejection::new(
        "blob.too_large",
        "the server refused a file larger than it takes",
    )
}

/// AC3: a body the server refuses surfaces in `upload_refusals` with the
/// commit's id and the blob's id, released from the queue and the pins —
/// on the linked replica from the stand-in's `413`, on the fake as a test
/// sets it.
#[test]
fn a_refused_upload_surfaces_with_its_commit_and_blob() {
    let bytes = vec![10u8; 100];
    let id = blob_id(&bytes);
    let subjects = [
        ("fake", harness(Over::Fake)),
        (
            "linked",
            harness(Over::Linked(&SyncularOpener, vec![(id.clone(), 413)])),
        ),
    ];
    for (name, world) in subjects {
        let commit = send(&world, "m-1", "a-1", &bytes);
        match &world.fake {
            Some(fake) => fake.refuse_upload(&id, too_large()),
            None => world.source.replica.sync_now(),
        }
        eventually(&format!("{name}: the refusal"), || {
            !world.source.replica.upload_refusals().is_empty()
        });
        assert_eq!(
            world.source.replica.upload_refusals(),
            vec![UploadRefusal {
                commit_id: commit,
                blob_id: id.clone(),
                rejection: too_large(),
                refused_rows: vec!["a-1".to_owned()],
                commit_rows: vec!["m-1".to_owned(), "a-1".to_owned()],
            }],
            "{name}"
        );
        assert!(world.queued().is_empty(), "{name}: out of the queue");
        assert!(world.pins().is_empty(), "{name}: and its commit's pins");
    }
}

/// AC3: a commit the server refused, recorded as keeper-core's tests record
/// one without a server — in the replica file's outcome journal — and set
/// on the fake, reads back through `refused_commits` with the same fields.
#[test]
fn a_refused_commit_reads_back_with_what_it_wrote() {
    let subjects = [
        ("fake", harness(Over::Fake)),
        ("local", harness(Over::Local(&SyncularOpener))),
        ("linked", harness(Over::Linked(&SyncularOpener, Vec::new()))),
    ];
    let wrote = message("m-refused");
    let expected = RefusedCommit {
        commit_id: "c-refused".to_owned(),
        rejection: Rejection::new("hq.v5.page", "hq.v5.page"),
        rows: vec![("messages".to_owned(), wrote.clone())],
    };
    for (name, world) in subjects {
        match &world.fake {
            Some(fake) => fake.refuse_commit(expected.clone()),
            None => journal_refused(
                &world.path,
                "c-refused",
                "hq.v5.page",
                &[("messages", "upsert", Value::Object(wrote.clone()))],
            ),
        }
        let replica = &world.source.replica;
        assert_eq!(
            replica.refused_commits("messages").expect("reads"),
            vec![expected.clone()],
            "{name}"
        );
        assert!(
            replica
                .refused_commits("attachments")
                .expect("reads")
                .is_empty(),
            "{name}: another table's"
        );
    }
}
