//! Attachments over real on-disk replicas with no server: staging and the
//! contract's bound, the one commit, the pin across a relaunch, the chips'
//! states, a refused commit's chips, opening only what a row names, and the
//! server's refusal codes as sentences.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::hq::port::fake::{FakeOpener, FakeReplica};
use crate::hq::port::{LinkState, Opener, Rejection, ReplicaSpec, RowOp};
use crate::hq::sessions::{self, object, Source};
use crate::hq::syncular::SyncularReplica;
use crate::hq::timeline::{self, AgentMessageStateVm, Opened};

const ANA: &str = "ana";

fn spec(dir: &Path) -> ReplicaSpec {
    ReplicaSpec {
        database: "alpha".to_owned(),
        path: dir.join("alpha.sqlite"),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    }
}

fn source(replica: Arc<dyn crate::hq::port::Replica>) -> Source {
    Source {
        database: "alpha".to_owned(),
        replica,
        actor: ANA.to_owned(),
        client: "dev1".to_owned(),
    }
}

fn open(dir: &Path) -> Source {
    source(Arc::new(SyncularReplica::open(spec(dir)).expect("opens")))
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

/// A session of nixie's with ana as its owner, created at `at`.
fn seed_session(source: &Source, id: &str, at: i64, archived: bool) {
    source
        .replica
        .mutate(vec![
            upsert(
                "sessions",
                json!({"id": id, "title": "files", "status": "open", "agent_actor": "nixie",
                       "created_by_actor": ANA, "created_by_client": "dev1",
                       "preferred_client": null, "parent_session": null, "invite": "[]",
                       "archived": archived, "heard_until": null, "created_at_ms": at,
                       "last_activity_ms": at, "author_actor": ANA, "author_client": "dev1"}),
            ),
            upsert(
                "session_members",
                json!({"id": format!("{id}/{ANA}"), "session_id": id, "actor_id": ANA,
                       "role": "owner", "reads_trace": true, "added_by_actor": ANA,
                       "added_at_ms": at, "read_until": null,
                       "author_actor": ANA, "author_client": "dev1"}),
            ),
        ])
        .expect("seeds");
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(bytes)))
}

/// The replica file read as the client keeps it.
fn file(dir: &Path) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(dir.join("alpha.sqlite")).expect("opens");
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .expect("timeout");
    conn
}

fn count(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |row| row.get(0)).expect("counts")
}

/// The commits waiting in the replica's outbox, read from its file.
fn outbox(dir: &Path) -> i64 {
    count(&file(dir), "SELECT count(*) FROM _syncular_outbox")
}

fn close(source: Source) {
    source.replica.close();
    source.replica.join();
}

#[test]
fn the_bound_is_the_contracts_number_and_a_size_reads_whatever_a_file_claims() {
    let limits: Value =
        serde_json::from_str(include_str!("../../../../../syncular/limits.json")).expect("json");
    assert_eq!(limits["attachmentMaxBytes"], json!(ATTACHMENT_MAX_BYTES));
    assert_eq!(ATTACHMENT_MAX_BYTES, 26_214_400);
    assert!(too_large("x", u64::MAX).contains("17592186044416.0 MB"));
    assert!(too_large("x", ATTACHMENT_MAX_BYTES).starts_with("x is 25.0 MB"));
    assert!(too_large("x", ATTACHMENT_MAX_BYTES + 1).starts_with("x is 25.1 MB"));
    assert_eq!(human(1024 * 1024 - 1), "1024 kB");
}

#[test]
fn a_file_over_the_bound_is_refused_before_anything_is_staged_or_written() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let waiting = outbox(dir.path());

    let over = dir.path().join("huge.zip");
    std::fs::write(&over, vec![0u8; ATTACHMENT_MAX_BYTES as usize + 1]).expect("writes");
    let sentence = too_large("huge.zip", ATTACHMENT_MAX_BYTES + 1);
    assert_eq!(
        stage_path(&over),
        Err(AttachError::TooLarge(sentence.clone()))
    );
    assert_eq!(
        stage_bytes(
            "huge.zip",
            "application/zip",
            vec![0u8; ATTACHMENT_MAX_BYTES as usize + 1]
        ),
        Err(AttachError::TooLarge(sentence))
    );
    // Exactly the bound is a file an agent takes.
    let at_bound = dir.path().join("big.bin");
    std::fs::write(&at_bound, vec![1u8; ATTACHMENT_MAX_BYTES as usize]).expect("writes");
    assert_eq!(
        stage_path(&at_bound).expect("stages").len(),
        ATTACHMENT_MAX_BYTES
    );
    // Nothing of the refused file reached the replica.
    assert_eq!(outbox(dir.path()), waiting);
    let conn = file(dir.path());
    assert_eq!(count(&conn, "SELECT count(*) FROM _syncular_blobs"), 0);
    assert_eq!(
        count(&conn, "SELECT count(*) FROM _syncular_blob_uploads"),
        0
    );
}

#[test]
fn attaching_stages_each_blob_and_commits_the_message_and_its_rows_in_one_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let before = outbox(dir.path());
    let csv = dir.path().join("prices.csv");
    std::fs::write(&csv, b"item,price\ntea,3\n").expect("writes");
    let files = vec![
        stage_path(&csv).expect("stages"),
        stage_bytes("folder/../paste.png", "image/png", b"\x89PNG-ish".to_vec()).expect("stages"),
    ];
    let message = attach(&alpha, "dev1.1", "what is the price of tea?", files).expect("attaches");
    assert_eq!(
        outbox(dir.path()),
        before + 1,
        "the message and its rows are one commit"
    );

    let rows = alpha
        .replica
        .query(
            "SELECT name, mime, size, blob_ref, message_id FROM attachments \
             WHERE session_id = 'dev1.1' ORDER BY name",
            &[],
        )
        .expect("reads");
    assert_eq!(rows.len(), 2);
    let csv_id = sha256(b"item,price\ntea,3\n");
    assert_eq!(rows[1]["name"], "prices.csv");
    assert_eq!(rows[1]["mime"], "text/csv");
    assert_eq!(rows[1]["size"], 17);
    assert_eq!(rows[1]["message_id"], json!(message));
    // Canonical BlobRef JSON, in its key order.
    assert_eq!(
        rows[1]["blob_ref"],
        json!(format!(
            r#"{{"blobId":"{csv_id}","byteLength":17,"mediaType":"text/csv","name":"prices.csv"}}"#
        ))
    );
    assert_eq!(
        rows[0]["name"], "paste.png",
        "a pasted name never names a folder"
    );
    let body = alpha
        .replica
        .query("SELECT body FROM messages WHERE id = ?1", &[json!(message)])
        .expect("reads");
    assert_eq!(body[0]["body"], "what is the price of tea?");
    // Both blobs are staged for upload under their content address.
    let conn = file(dir.path());
    let staged: Vec<String> = conn
        .prepare("SELECT blob_id FROM _syncular_blob_uploads ORDER BY blob_id")
        .expect("prepares")
        .query_map([], |row| row.get(0))
        .expect("reads")
        .collect::<Result<_, _>>()
        .expect("reads");
    let mut want = vec![csv_id.clone(), sha256(b"\x89PNG-ish")];
    want.sort();
    assert_eq!(staged, want);

    // Shown at once: one message with its two chips, waiting (no server).
    let (view, _) = timeline::view(
        &alpha,
        "dev1.1",
        &Opened::default(),
        &Default::default(),
        2_000,
    )
    .expect("a view");
    let shown = view
        .messages
        .iter()
        .find(|m| m.id == message)
        .expect("the message");
    assert_eq!(shown.attachments.len(), 2);
    let chip = &shown.attachments[0];
    assert_eq!(chip.name, "prices.csv");
    assert_eq!(chip.size_label, "1 kB");
    assert_eq!(chip.state, AgentAttachmentStateVm::Waiting);
    assert_eq!(chip.sentence, None);
    assert_eq!(
        parse_blob_url(&chip.url),
        Some(("alpha".to_owned(), csv_id))
    );
}

#[test]
fn nothing_attached_an_archived_or_unknown_session_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, true);
    let before = outbox(dir.path());
    let file = || vec![stage_bytes("a.txt", "", b"a".to_vec()).expect("stages")];
    assert_eq!(
        attach(&alpha, "dev1.1", "", Vec::new()),
        Err(AttachError::Nothing)
    );
    assert_eq!(
        attach(&alpha, "dev1.1", "", file()),
        Err(AttachError::Archived)
    );
    assert_eq!(
        attach(&alpha, "dev1.9", "", file()),
        Err(AttachError::Unknown)
    );
    assert_eq!(outbox(dir.path()), before);
    assert_eq!(
        count(
            &self::file(dir.path()),
            "SELECT count(*) FROM _syncular_blobs"
        ),
        0
    );
}

#[test]
fn offline_a_staged_blob_survives_a_relaunch_pinned_and_the_same_bytes_are_one_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let bytes = b"%PDF-1.7 the report".to_vec();
    let report =
        || vec![stage_bytes("report.pdf", "application/pdf", bytes.clone()).expect("stages")];
    attach(&alpha, "dev1.1", "", report()).expect("attaches");
    attach(&alpha, "dev1.1", "again", report()).expect("attaches twice");
    let waiting = outbox(dir.path());
    assert_eq!(waiting, 3, "the session and both messages");
    close(alpha);

    // A new process: nothing of the last one but the file.
    let alpha = open(dir.path());
    assert_eq!(outbox(dir.path()), waiting, "every commit still waits");
    assert_eq!(alpha.replica.state().outbox, 3);
    let conn = file(dir.path());
    let id = sha256(&bytes);
    assert_eq!(
        count(&conn, "SELECT count(*) FROM _syncular_blob_uploads"),
        1,
        "one blob to upload, however often it is attached"
    );
    assert_eq!(
        conn.query_row("SELECT blob_id FROM _syncular_blob_uploads", [], |row| {
            row.get::<_, String>(0)
        })
        .expect("reads"),
        id
    );
    assert_eq!(
        count(
            &conn,
            "SELECT count(DISTINCT commit_id) FROM _syncular_blob_commit_refs"
        ),
        2,
        "each waiting commit pins it"
    );
    // The body is still here, and opens from the cache with no server.
    let opened = open_blob(&alpha, &id).expect("opens");
    assert_eq!(opened.bytes, bytes);
    assert_eq!(opened.mime, "application/pdf");
    assert_eq!(opened.name, "report.pdf");
}

#[test]
fn only_a_blob_an_attachment_names_opens_and_only_as_a_type_that_cannot_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let page = b"<script>alert(1)</script>".to_vec();
    attach(
        &alpha,
        "dev1.1",
        "",
        vec![stage_bytes("page.html", "text/html", page.clone()).expect("stages")],
    )
    .expect("attaches");
    let opened = open_blob(&alpha, &sha256(&page)).expect("opens");
    assert_eq!(opened.mime, "application/octet-stream");
    // Staged bytes no attachment names are not served.
    alpha
        .replica
        .upload_blob(b"loose".to_vec(), None)
        .expect("stages");
    assert_eq!(
        open_blob(&alpha, &sha256(b"loose")),
        Err(OpenError::NotHere)
    );
    assert_eq!(served_mime("image/svg+xml"), "application/octet-stream");
    assert_eq!(served_mime("IMAGE/PNG"), "image/png");
    assert_eq!(served_mime("text/csv"), "text/plain; charset=utf-8");
}

#[test]
fn a_blob_url_names_its_database_and_blob_in_either_spelling_and_nothing_else() {
    let id = sha256(b"x");
    let scheme = blob_url_in(UrlForm::Scheme, "alpha db", &id);
    let localhost = blob_url_in(UrlForm::Localhost, "alpha db", &id);
    assert!(scheme.starts_with("keeper-hq-blob://blob/alpha%20db/sha256%3A"));
    assert!(localhost.starts_with("http://keeper-hq-blob.localhost/blob/alpha%20db/"));
    for url in [&scheme, &localhost] {
        assert_eq!(
            parse_blob_url(url),
            Some(("alpha db".to_owned(), id.clone()))
        );
    }
    assert_eq!(
        parse_blob_url(&format!("{scheme}?retry=2")),
        Some(("alpha db".to_owned(), id.clone()))
    );
    for foreign in [
        format!("keeper-media://blob/alpha/{id}"),
        format!("keeper-hq-blob://other/alpha/{id}"),
        "keeper-hq-blob://blob/alpha/sha256:XYZ".to_owned(),
        format!("keeper-hq-blob://blob/alpha/{id}/extra"),
        format!("keeper-hq-blob://blob//{id}"),
    ] {
        assert_eq!(parse_blob_url(&foreign), None, "{foreign}");
    }
}

/// A fake replica holding `rows` as every query's answer, at `link`.
fn fake(rows: Vec<Value>, link: LinkState) -> (Arc<FakeReplica>, Source) {
    let opener = FakeOpener::default();
    let replica = opener.open(spec(Path::new("/nowhere"))).expect("opens");
    let held = opener.all().remove(0);
    held.replicate(rows.into_iter().map(object).collect());
    held.set(link);
    (held, source(replica))
}

fn row(author_client: &str, version: i64) -> Value {
    json!({"id": "att-1", "message_id": "m-1", "name": "a.csv", "mime": "text/csv", "size": 3,
           "blob_ref": format!(r#"{{"blobId":"{}","byteLength":3}}"#, sha256(b"abc")),
           "author_client": author_client, "version": version})
}

#[test]
fn a_chip_is_uploading_while_the_server_is_reachable_waiting_while_not_and_sent_once_it_has_it() {
    let state = |author: &str, version: i64, link: LinkState| {
        let (_, src) = fake(vec![row(author, version)], link);
        chips(&src, "dev1.1")["m-1"][0].state
    };
    use AgentAttachmentStateVm::*;
    assert_eq!(state("dev1", -1, LinkState::Connected), Uploading);
    assert_eq!(state("dev1", -1, LinkState::Connecting), Uploading);
    assert_eq!(state("dev1", -1, LinkState::Offline), Waiting);
    assert_eq!(state("dev1", -1, LinkState::Local), Waiting);
    assert_eq!(state("dev1", -1, LinkState::Refused), Waiting);
    assert_eq!(state("dev1", 4, LinkState::Offline), Sent);
    // Another device's or person's is the server's, whatever this link.
    assert_eq!(state("ben.phone", -1, LinkState::Offline), Sent);

    // An upload the server refused: this device's waiting chips say why.
    let (held, src) = fake(vec![row("dev1", -1)], LinkState::Offline);
    *held.upload_refusal.lock().expect("lock") =
        Some(Rejection::new("blob.too_large", "too large"));
    let chip = &chips(&src, "dev1.1")["m-1"][0];
    assert_eq!(chip.state, Refused);
    assert_eq!(
        chip.sentence.as_deref(),
        Some(refusal_sentence(&Rejection::new("blob.too_large", ""), "a.csv").as_str())
    );
    let (held, src) = fake(vec![row("dev1", 7)], LinkState::Offline);
    *held.upload_refusal.lock().expect("lock") = Some(Rejection::new("blob.too_large", ""));
    assert_eq!(chips(&src, "dev1.1")["m-1"][0].state, Sent, "already sent");
}

/// Record, as a round would, the server refusing commit `commit` that wrote
/// `ops` (each a table and its values).
fn journal_refusal(dir: &Path, commit: &str, ops: &[(&str, Value)], code: &str) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let operations: Vec<CommitOperation> = ops
        .iter()
        .map(|(table, values)| CommitOperation {
            table: (*table).to_owned(),
            row_id: values["id"].as_str().expect("an id").to_owned(),
            op: "upsert".to_owned(),
            base_version: None,
            values: values.as_object().cloned(),
        })
        .collect();
    let results = vec![CommitOperationOutcome::Error {
        rejection: RejectionRecord {
            client_commit_id: commit.to_owned(),
            op_index: 1,
            code: code.to_owned(),
            message: code.to_owned(),
            retryable: false,
            details: None,
            operation: None,
        },
    }];
    file(dir)
        .execute(
            "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
             results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
            rusqlite::params![
                commit,
                serde_json::to_string(&results).expect("results"),
                serde_json::to_string(&operations).expect("operations")
            ],
        )
        .expect("records");
}

#[test]
fn a_message_whose_files_the_server_refused_stays_shown_refused_with_the_reason() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    let at = 1_792_000_000_000_i64;
    seed_session(&alpha, "dev1.1", at, false);
    let id = sha256(b"gone");
    let page = sessions::page_key("dev1.1", &sessions::month(at + 5));
    journal_refusal(
        dir.path(),
        "c-refused",
        &[
            (
                "messages",
                json!({"id": "m-refused", "session_id": "dev1.1", "page_key": page,
                       "role": "person", "body": "see the file", "created_at_ms": at + 5,
                       "author_actor": ANA, "author_client": "dev1"}),
            ),
            (
                "attachments",
                json!({"id": "a-refused", "session_id": "dev1.1", "message_id": "m-refused",
                       "blob_ref": format!(r#"{{"blobId":"{id}","byteLength":4}}"#),
                       "name": "gone.txt", "mime": "text/plain", "size": 4,
                       "created_at_ms": at + 5, "author_actor": ANA, "author_client": "dev1"}),
            ),
            (
                "sessions",
                json!({"id": "dev1.1", "last_activity_ms": at + 5}),
            ),
        ],
        "blob.not_found",
    );
    // Another session's refusal is not this one's.
    journal_refusal(
        dir.path(),
        "c-elsewhere",
        &[
            (
                "messages",
                json!({"id": "m-other", "session_id": "dev1.2", "body": "", "created_at_ms": at}),
            ),
            (
                "attachments",
                json!({"id": "a-other", "session_id": "dev1.2", "message_id": "m-other",
                       "blob_ref": format!(r#"{{"blobId":"{id}","byteLength":4}}"#),
                       "name": "x", "mime": "text/plain", "size": 4}),
            ),
        ],
        "hq.attachment_writer",
    );
    let later = attach(
        &alpha,
        "dev1.1",
        "",
        vec![stage_bytes("b.txt", "", b"b".to_vec()).expect("stages")],
    )
    .expect("attaches");
    let (view, _) = timeline::view(
        &alpha,
        "dev1.1",
        &Opened::default(),
        &Default::default(),
        at + 10,
    )
    .expect("a view");
    let ids: Vec<&str> = view.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(
        ids,
        ["m-refused", later.as_str()],
        "by time, the refused one first"
    );
    let refused = &view.messages[0];
    assert_eq!(refused.state, Some(AgentMessageStateVm::Refused));
    assert_eq!(refused.body, "see the file");
    assert_eq!(refused.attachments.len(), 1);
    assert_eq!(
        refused.attachments[0].state,
        AgentAttachmentStateVm::Refused
    );
    assert_eq!(
        refused.attachments[0].sentence.as_deref(),
        Some(
            "The agents server does not hold gone.txt, so it refused the message that names it. \
             Attach it again."
        )
    );
}

#[test]
fn every_refusal_code_the_server_sends_reads_as_a_sentence_naming_the_file() {
    let say = |code: &str| refusal_sentence(&Rejection::new(code, code), "plan.pdf");
    let codes = [
        "blob.too_large",
        "blob.hash_mismatch",
        "blob.not_found",
        "blob.forbidden",
        "hq.attachment_writer",
        "hq.attachment_message",
        "hq.attachment_size",
        "hq.attachment_blob_ref",
        "hq.attachment_blob_size",
        "hq.v1.author_actor",
        "hq.v1.author_client",
        crate::hq::port::CONFLICT,
    ];
    for code in codes {
        let sentence = say(code);
        assert!(sentence.contains("plan.pdf"), "{code}: {sentence}");
        assert!(
            !sentence.contains(code),
            "{code} is keeper's words: {sentence}"
        );
    }
    assert!(say("blob.too_large").contains("25.0 MB"));
    assert_eq!(say("blob.too_large"), say("hq.attachment_size"));
    assert_eq!(say("hq.v1.author_actor"), say("hq.v1.author_client"));
    // Each other code has words of its own.
    let distinct: std::collections::HashSet<String> = codes.iter().map(|c| say(c)).collect();
    assert_eq!(distinct.len(), codes.len() - 2);
    // A code keeper does not know is said as the server said it.
    assert_eq!(
        refusal_sentence(&Rejection::new("hq.new_rule", "not today"), "plan.pdf"),
        "The agents server refused plan.pdf: hq.new_rule: not today."
    );
}
