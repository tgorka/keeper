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

/// The decisions a message declines with; these sessions hold no request,
/// so nothing is ever written to it.
fn decisions() -> crate::hq::approvals::Decisions {
    crate::hq::approvals::Decisions::load(
        std::env::temp_dir().join(format!("keeper-attach-{}.json", ulid::Ulid::new())),
    )
}

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
    let message = attach(
        &alpha,
        &decisions(),
        "dev1.1",
        "what is the price of tea?",
        files,
        &[],
        None,
    )
    .expect("attaches");
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
fn a_messages_chips_keep_the_order_the_files_were_attached_in() {
    // Neither alphabetical nor any other order a sort could stumble into.
    const NAMES: [&str; 8] = [
        "zeta.txt",
        "alpha.txt",
        "mid.txt",
        "beta.txt",
        "omega.txt",
        "delta.txt",
        "kappa.txt",
        "eta.txt",
    ];
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    // Every file of one commit shares its millisecond, so random ids inside
    // it would pass one round in 8! — never 200 in a row.
    for round in 0..200 {
        let session = format!("dev1.{}", round + 1);
        seed_session(&alpha, &session, 1_000, false);
        let files = NAMES
            .iter()
            .map(|name| {
                stage_bytes(name, "text/plain", format!("{round} {name}").into_bytes())
                    .expect("stages")
            })
            .collect();
        let message =
            attach(&alpha, &decisions(), &session, "", files, &[], None).expect("attaches");
        let (view, _) = timeline::view(
            &alpha,
            &session,
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
        let order: Vec<&str> = shown.attachments.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(order, NAMES, "round {round}: the chips in attach order");
    }
}

#[test]
fn nothing_attached_an_archived_or_unknown_session_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, true);
    let before = outbox(dir.path());
    let file = || vec![stage_bytes("a.txt", "", b"a".to_vec()).expect("stages")];
    assert_eq!(
        attach(&alpha, &decisions(), "dev1.1", "", Vec::new(), &[], None),
        Err(AttachError::Nothing)
    );
    assert_eq!(
        attach(&alpha, &decisions(), "dev1.1", "", file(), &[], None),
        Err(AttachError::Archived)
    );
    assert_eq!(
        attach(&alpha, &decisions(), "dev1.9", "", file(), &[], None),
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

fn key(session_id: &str) -> sessions::AgentSessionRef {
    sessions::AgentSessionRef {
        database: "alpha".to_owned(),
        session_id: session_id.to_owned(),
    }
}

/// The messages of `session_id` whose text is `body`.
fn messages_saying(source: &Source, session_id: &str, body: &str) -> Vec<String> {
    source
        .replica
        .query(
            "SELECT id FROM messages WHERE session_id = ?1 AND body = ?2",
            &[json!(session_id), json!(body)],
        )
        .expect("reads")
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect()
}

/// The names of the files attached to `message`, sorted.
fn attached_to(source: &Source, message: &str) -> Vec<String> {
    let mut names: Vec<String> = source
        .replica
        .query(
            "SELECT name FROM attachments WHERE message_id = ?1",
            &[json!(message)],
        )
        .expect("reads")
        .iter()
        .filter_map(|row| row["name"].as_str().map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// R1071C-02: a selection staged by path and by bytes, then sent once with
/// its caption, is one message holding every file — the caption written
/// once — and staging alone writes nothing.
#[test]
fn a_mixed_selection_staged_under_handles_is_one_message_with_its_caption_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let before = outbox(dir.path());
    let csv = dir.path().join("prices.csv");
    std::fs::write(&csv, b"item,price\ntea,3\n").expect("writes");
    let staging = Staging::default();
    let chat = key("dev1.1");
    let handles = vec![
        staging.stage(&chat, stage_path(&csv).expect("stages")),
        staging.stage(
            &chat,
            stage_bytes("shot.png", "image/png", b"\x89PNG".to_vec()).expect("stages"),
        ),
        staging.stage(
            &chat,
            stage_bytes("notes.txt", "", b"remember".to_vec()).expect("stages"),
        ),
    ];
    assert_eq!(outbox(dir.path()), before, "staging writes nothing");
    assert_eq!(
        count(
            &file(dir.path()),
            "SELECT count(*) FROM _syncular_blob_uploads"
        ),
        0,
        "nothing staged reaches the upload queue"
    );

    let files = staging
        .take(&chat, &handles)
        .expect("every handle is staged");
    let message = attach(
        &alpha,
        &decisions(),
        "dev1.1",
        "compare these",
        files,
        &[],
        None,
    )
    .expect("attaches");
    assert_eq!(outbox(dir.path()), before + 1, "one commit");
    assert_eq!(
        messages_saying(&alpha, "dev1.1", "compare these"),
        vec![message.clone()],
        "the caption is one message's text, once"
    );
    assert_eq!(
        attached_to(&alpha, &message),
        ["notes.txt", "prices.csv", "shot.png"]
    );
    assert_eq!(
        count(&file(dir.path()), "SELECT count(*) FROM messages"),
        1,
        "no captionless message beside it"
    );
    assert!(staging.is_empty(), "a committed handle is spent");
    assert_eq!(
        staging.take(&chat, &handles),
        Err(AttachError::Released),
        "and cannot be sent twice"
    );
}

/// R1071C-02: several pasted files, one Send, one message.
#[test]
fn several_pasted_files_sent_once_are_one_message() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let before = outbox(dir.path());
    let staging = Staging::default();
    let chat = key("dev1.1");
    let handles: Vec<String> = ["first.csv", "second.csv", "third.csv"]
        .iter()
        .map(|name| {
            staging.stage(
                &chat,
                stage_bytes(name, "text/csv", name.as_bytes().to_vec()).expect("stages"),
            )
        })
        .collect();
    let files = staging.take(&chat, &handles).expect("staged");
    let message = attach(
        &alpha,
        &decisions(),
        "dev1.1",
        "Compare these files",
        files,
        &[],
        None,
    )
    .expect("attaches");
    assert_eq!(outbox(dir.path()), before + 1);
    assert_eq!(
        attached_to(&alpha, &message),
        ["first.csv", "second.csv", "third.csv"]
    );
    assert_eq!(
        messages_saying(&alpha, "dev1.1", "Compare these files").len(),
        1
    );
    assert_eq!(count(&file(dir.path()), "SELECT count(*) FROM messages"), 1);
}

/// R1071C-02: a handle never committed is released — by the composer, by
/// its session closing, or by a Send that names one no longer staged, which
/// then commits nothing and releases the rest; another session's handle is
/// neither taken nor released.
#[test]
fn a_handle_never_committed_is_released_and_a_send_naming_one_commits_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let before = outbox(dir.path());
    let staging = Staging::default();
    let (chat, other) = (key("dev1.1"), key("dev1.2"));
    let bytes = |name: &str| stage_bytes(name, "", name.as_bytes().to_vec()).expect("stages");

    // Removed from the tray.
    let dropped = staging.stage(&chat, bytes("dropped.txt"));
    let theirs = staging.stage(&other, bytes("theirs.txt"));
    staging.release(&chat, std::slice::from_ref(&theirs));
    assert_eq!(
        staging.len(),
        2,
        "another session's handle is not this one's to release"
    );
    staging.release(&chat, std::slice::from_ref(&dropped));
    assert_eq!(staging.len(), 1);
    assert_eq!(
        staging.take(&chat, std::slice::from_ref(&dropped)),
        Err(AttachError::Released)
    );

    // A Send naming a released handle beside a staged one: nothing is
    // committed, and the staged one is released with it.
    let kept = staging.stage(&chat, bytes("kept.txt"));
    assert_eq!(
        staging.take(&chat, &[kept.clone(), dropped.clone()]),
        Err(AttachError::Released)
    );
    assert_eq!(staging.len(), 1, "only the other session's file is left");
    // Another session's handle cannot be sent from this one, and stays.
    assert_eq!(
        staging.take(&chat, std::slice::from_ref(&theirs)),
        Err(AttachError::Released)
    );
    assert_eq!(staging.len(), 1);

    // The session closed with a file still staged.
    let left = staging.stage(&chat, bytes("left.txt"));
    staging.release_session(&chat);
    assert_eq!(
        staging.take(&chat, std::slice::from_ref(&left)),
        Err(AttachError::Released)
    );
    assert_eq!(staging.len(), 1, "the other session's file is not touched");
    staging.release_session(&other);
    assert!(staging.is_empty());
    assert_eq!(staging.take(&chat, &[]), Err(AttachError::Nothing));

    assert_eq!(outbox(dir.path()), before, "nothing was ever written");
    let conn = file(dir.path());
    assert_eq!(count(&conn, "SELECT count(*) FROM _syncular_blobs"), 0);
    assert_eq!(count(&conn, "SELECT count(*) FROM messages"), 0);
}

#[test]
fn offline_a_staged_blob_survives_a_relaunch_pinned_and_the_same_bytes_are_one_upload() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let bytes = b"%PDF-1.7 the report".to_vec();
    let report =
        || vec![stage_bytes("report.pdf", "application/pdf", bytes.clone()).expect("stages")];
    attach(&alpha, &decisions(), "dev1.1", "", report(), &[], None).expect("attaches");
    attach(&alpha, &decisions(), "dev1.1", "again", report(), &[], None).expect("attaches twice");
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
        &decisions(),
        "dev1.1",
        "",
        vec![stage_bytes("page.html", "text/html", page.clone()).expect("stages")],
        &[],
        None,
    )
    .expect("attaches");
    let opened = open_blob(&alpha, &sha256(&page)).expect("opens");
    assert_eq!(opened.mime, "application/octet-stream");
    // Bytes cached in the replica that no attachment names are not served.
    file(dir.path())
        .execute(
            "INSERT INTO _syncular_blobs(blob_id, bytes, byte_length, media_type, created_at_ms) \
             VALUES (?1, ?2, 5, NULL, 1)",
            rusqlite::params![sha256(b"loose"), b"loose".to_vec()],
        )
        .expect("caches");
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

    let (held, src) = fake(vec![row("dev1", 7)], LinkState::Offline);
    *held.upload_refusals.lock().expect("lock") = vec![upload_refusal("att-1", &["att-1"])];
    assert_eq!(chips(&src, "dev1.1")["m-1"][0].state, Sent, "already sent");
}

/// The server's refusal of `blob.too_large` for the commit writing
/// `commit_rows`, of the blob `refused` carry.
fn upload_refusal(refused: &str, commit_rows: &[&str]) -> crate::hq::port::UploadRefusal {
    crate::hq::port::UploadRefusal {
        commit_id: "c-1".to_owned(),
        blob_id: sha256(b"huge"),
        rejection: Rejection::new("blob.too_large", "too large"),
        refused_rows: vec![refused.to_owned()],
        commit_rows: commit_rows.iter().map(|id| (*id).to_owned()).collect(),
    }
}

#[test]
fn a_refused_upload_marks_its_own_file_refused_its_messages_others_blocked_and_no_other_file() {
    use AgentAttachmentStateVm::*;
    let file = |id: &str, message: &str, name: &str| {
        json!({"id": id, "message_id": message, "name": name, "mime": "text/plain", "size": 3,
               "blob_ref": format!(r#"{{"blobId":"{}","byteLength":3}}"#, sha256(name.as_bytes())),
               "author_client": "dev1", "version": -1})
    };
    let (held, src) = fake(
        vec![
            file("a-huge", "m-1", "huge.zip"),
            file("a-tiny", "m-1", "tiny.txt"),
            file("a-else", "m-2", "else.txt"),
        ],
        LinkState::Connected,
    );
    *held.upload_refusals.lock().expect("lock") = vec![upload_refusal(
        "a-huge",
        &["m-1", "a-huge", "a-tiny", "dev1.1"],
    )];
    let chips = chips(&src, "dev1.1");
    let huge = &chips["m-1"][0];
    assert_eq!(huge.state, Refused);
    assert_eq!(
        huge.sentence.as_deref(),
        Some(refusal_sentence(&Rejection::new("blob.too_large", ""), "huge.zip").as_str())
    );
    let tiny = &chips["m-1"][1];
    assert_eq!(
        tiny.state, Blocked,
        "never accused of being too large itself"
    );
    assert_eq!(
        tiny.sentence.as_deref(),
        Some(blocked_sentence("tiny.txt", &["huge.zip".to_owned()]).as_str())
    );
    assert!(!tiny
        .sentence
        .as_deref()
        .unwrap_or_default()
        .contains("larger"));
    // A file of another message, another commit: on its way, not refused.
    assert_eq!(chips["m-2"][0].state, Uploading);
    assert_eq!(chips["m-2"][0].sentence, None);
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
        &decisions(),
        "dev1.1",
        "",
        vec![stage_bytes("b.txt", "", b"b".to_vec()).expect("stages")],
        &[],
        None,
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
    assert!(
        refused.mine,
        "a refused line is this replica's own, so it stays the person's bubble"
    );
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
fn a_message_refused_for_one_files_upload_shows_that_file_refused_and_its_other_blocked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    let at = 1_792_000_000_000_i64;
    seed_session(&alpha, "dev1.1", at, false);
    close(alpha);
    let (huge, tiny) = (sha256(b"huge"), sha256(b"tiny"));
    let page = sessions::page_key("dev1.1", &sessions::month(at + 5));
    let attachment = |id: &str, blob: &str, name: &str| {
        json!({"id": id, "session_id": "dev1.1", "message_id": "m-1",
               "blob_ref": format!(r#"{{"blobId":"{blob}","byteLength":4}}"#),
               "name": name, "mime": "text/plain", "size": 4,
               "created_at_ms": at + 5, "author_actor": ANA, "author_client": "dev1"})
    };
    // The server refused the commit, which went without the refused body.
    journal_refusal(
        dir.path(),
        "c-1",
        &[
            (
                "messages",
                json!({"id": "m-1", "session_id": "dev1.1", "page_key": page,
                       "role": "person", "body": "two files", "created_at_ms": at + 5,
                       "author_actor": ANA, "author_client": "dev1"}),
            ),
            ("attachments", attachment("a-huge", &huge, "huge.zip")),
            ("attachments", attachment("a-tiny", &tiny, "tiny.txt")),
        ],
        "blob.not_found",
    );
    // …after its upload was refused, which the replica kept beside itself.
    crate::hq::blob_cache::AccessLog::open(&crate::hq::blob_cache::AccessLog::beside(
        &dir.path().join("alpha.sqlite"),
    ))
    .expect("opens")
    .refuse(
        &[crate::hq::port::UploadRefusal {
            commit_id: "c-1".to_owned(),
            blob_id: huge.clone(),
            rejection: Rejection::new("blob.too_large", "too large"),
            refused_rows: vec!["a-huge".to_owned()],
            commit_rows: vec!["m-1".to_owned(), "a-huge".to_owned(), "a-tiny".to_owned()],
        }],
        1,
    )
    .expect("keeps");
    let alpha = open(dir.path());
    let (view, _) = timeline::view(
        &alpha,
        "dev1.1",
        &Opened::default(),
        &Default::default(),
        at + 10,
    )
    .expect("a view");
    let message = &view.messages[0];
    assert_eq!(message.body, "two files", "the caption stays");
    assert_eq!(message.state, Some(AgentMessageStateVm::Refused));
    let states: Vec<_> = message
        .attachments
        .iter()
        .map(|chip| (chip.name.as_str(), chip.state, chip.sentence.clone()))
        .collect();
    assert_eq!(
        states,
        [
            (
                "huge.zip",
                AgentAttachmentStateVm::Refused,
                Some(refusal_sentence(
                    &Rejection::new("blob.too_large", ""),
                    "huge.zip"
                ))
            ),
            (
                "tiny.txt",
                AgentAttachmentStateVm::Blocked,
                Some(blocked_sentence("tiny.txt", &["huge.zip".to_owned()]))
            ),
        ],
        "the upload's refusal, not the commit's not_found, and only for its own file"
    );
}

/// Wait up to a minute (the replica backs off after each failed round) for
/// `ready`.
fn eventually(what: &str, ready: impl Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while !ready() {
        assert!(std::time::Instant::now() < deadline, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_refused_upload_is_resolved_so_every_other_write_goes_on_and_it_stays_explained_after_a_relaunch(
) {
    use crate::hq::syncular::tests::{blob_server, bootstrapped, pins, puts, queued};
    use AgentAttachmentStateVm::*;
    let (huge, tiny, note) = (
        b"pretend these are 26 MB".to_vec(),
        b"tea,3\n".to_vec(),
        b"a note".to_vec(),
    );
    let (port, seen) = blob_server(vec![(sha256(&huge), 413)]);
    let dir = tempfile::tempdir().expect("tempdir");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    seed_session(&alpha, "dev1.2", 1_000, false);
    close(alpha);
    bootstrapped(&dir.path().join("alpha.sqlite"));
    let linked = || {
        let mut spec = spec(dir.path());
        spec.link = Some(crate::hq::port::Link {
            base_url: format!("http://127.0.0.1:{port}/alpha"),
            token: "0123456789abcdef".to_owned(),
        });
        source(Arc::new(SyncularReplica::open(spec).expect("opens")))
    };
    let alpha = linked();
    // A file the server refuses ahead of a tiny one, in one message; another
    // session's message after them.
    attach(
        &alpha,
        &decisions(),
        "dev1.1",
        "two files",
        vec![
            stage_bytes("huge.zip", "application/zip", huge.clone()).expect("stages"),
            stage_bytes("tiny.csv", "text/csv", tiny.clone()).expect("stages"),
        ],
        &[],
        None,
    )
    .expect("attaches");
    attach(
        &alpha,
        &decisions(),
        "dev1.2",
        "elsewhere",
        vec![stage_bytes("note.txt", "text/plain", note.clone()).expect("stages")],
        &[],
        None,
    )
    .expect("attaches");
    let refused_at = || {
        seen.lock()
            .expect("seen")
            .iter()
            .position(|(_, path, status)| *status == 413 && path.contains(&sha256(&huge)))
    };
    eventually(
        "the server refuses huge.zip, then a round goes past it",
        || {
            refused_at().is_some_and(|at| {
                crate::hq::syncular::tests::pushed_after(&seen, at + 1)
                    && puts(&seen, &sha256(&note), 200) >= 1
                    && puts(&seen, &sha256(&tiny), 200) >= 1
            })
        },
    );
    let path = dir.path().join("alpha.sqlite");
    assert!(!queued(&path).contains(&sha256(&huge)), "no longer staged");
    assert!(
        pins(&path).iter().all(|(_, blob)| *blob != sha256(&huge)),
        "no longer pinned"
    );
    assert!(
        pins(&path).iter().any(|(_, blob)| *blob == sha256(&tiny)),
        "the other body its commit names stays pinned"
    );
    let explained = |alpha: &Source| {
        let held = alpha.replica.upload_refusals();
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(held[0].blob_id, sha256(&huge));
        assert_eq!(held[0].rejection.code, "blob.too_large");
        let one = chips(alpha, "dev1.1");
        let mut files: Vec<_> = one
            .values()
            .flatten()
            .map(|chip| (chip.name.as_str(), chip.state))
            .collect();
        files.sort_by_key(|(name, _)| *name);
        assert_eq!(files, [("huge.zip", Refused), ("tiny.csv", Blocked)]);
        let other = chips(alpha, "dev1.2");
        let note = &other.values().next().expect("its message")[0];
        assert!(
            matches!(note.state, Uploading | Waiting),
            "another session's file is not refused: {note:?}"
        );
        assert_eq!(note.sentence, None);
    };
    explained(&alpha);
    close(alpha);

    // A relaunch: the refusal is still explained, still resolved.
    let pushes = seen.lock().expect("seen").len();
    let alpha = linked();
    explained(&alpha);
    eventually("a round after the relaunch reaches its push", || {
        crate::hq::syncular::tests::pushed_after(&seen, pushes)
    });
    let refusals = seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|(_, path, status)| *status == 413 && path.contains(&sha256(&huge)))
        .count();
    assert_eq!(refusals, 1, "never tried again");
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

fn thumbnail_fixture() -> Vec<u8> {
    include_bytes!("../../../../../dev/fixtures/agent-photo.png").to_vec()
}

fn thumbnail_dimensions(bytes: &[u8]) -> (u32, u32) {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .expect("PNG format")
        .into_dimensions()
        .expect("PNG dimensions")
}

#[test]
fn thumbnail_path_scheme_serves_only_staged_snapshots_at_256_and_refuses_traversal() {
    use crate::asset_url::{compose, UrlForm};
    use crate::hq::thumbnails;
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("photo.png");
    std::fs::write(&path, thumbnail_fixture()).expect("image");
    let staging = Staging::default();
    let key = AgentSessionRef {
        database: "alpha".into(),
        session_id: "dev1.1".into(),
    };
    let handle = staging.stage(&key, stage_path(&path).expect("stage path"));
    // A replaced path cannot change the selected snapshot or escape its allow-set.
    std::fs::write(&path, b"not the selected file").expect("replace");
    for form in [UrlForm::Scheme, UrlForm::Localhost] {
        let prefix = if form == UrlForm::Scheme {
            "localhost/"
        } else {
            ""
        };
        let uri = compose(form, SCHEME, &format!("{prefix}{handle}?thumbnail=1"));
        assert_eq!(
            thumbnail_dimensions(&thumbnails::staged(&staging, &uri).expect("thumbnail")),
            (256, 128)
        );
        for denied in [
            "/etc/passwd",
            "../photo.png",
            "%2e%2e%2fphoto.png",
            "notstaged",
            "foo/bar",
        ] {
            assert_eq!(
                thumbnails::staged(
                    &staging,
                    &compose(form, SCHEME, &format!("{prefix}{denied}?thumbnail=1"))
                ),
                Err(OpenError::NotHere)
            );
        }
    }
    staging.release(&key, std::slice::from_ref(&handle));
    let uri = compose(
        UrlForm::Scheme,
        SCHEME,
        &format!("localhost/{handle}?thumbnail=1"),
    );
    assert_eq!(thumbnails::staged(&staging, &uri), Err(OpenError::NotHere));
}

#[test]
fn thumbnail_rejects_invalid_and_oversized_images_without_panicking() {
    use crate::hq::thumbnails::render;
    assert_eq!(render(b"not an image"), Err(OpenError::NotHere));
    let mut oversized = thumbnail_fixture();
    oversized.resize(ATTACHMENT_MAX_BYTES as usize + 1, 0);
    assert_eq!(render(&oversized), Err(OpenError::NotHere));
}

#[test]
fn thumbnail_blob_is_own_person_only_and_cache_only_even_after_restart() {
    use crate::hq::thumbnails;
    let dir = tempfile::tempdir().expect("directory");
    let alpha = open(dir.path());
    seed_session(&alpha, "dev1.1", 1_000, false);
    let bytes = thumbnail_fixture();
    let blob = sha256(&bytes);
    attach(
        &alpha,
        &decisions(),
        "dev1.1",
        "look",
        vec![stage_bytes("photo.png", "image/png", bytes).expect("staged")],
        &[],
        None,
    )
    .expect("sent");
    close(alpha);
    let mut alpha = open(dir.path());
    assert_eq!(
        thumbnail_dimensions(
            &thumbnails::blob(&alpha, &blob).expect("cached thumbnail after restart")
        ),
        (256, 128)
    );
    assert!(chips(&alpha, "dev1.1")
        .values()
        .flatten()
        .all(|chip| chip.thumbnail_url.is_some()));
    alpha.actor = "another-person".into();
    assert_eq!(thumbnails::blob(&alpha, &blob), Err(OpenError::NotHere));
    assert!(chips(&alpha, "dev1.1")
        .values()
        .flatten()
        .all(|chip| chip.thumbnail_url.is_none()));
    alpha.actor = ANA.into();
    file(dir.path())
        .execute("UPDATE messages SET role = 'agent'", [])
        .expect("agent message");
    assert_eq!(thumbnails::blob(&alpha, &blob), Err(OpenError::NotHere));
    assert!(chips(&alpha, "dev1.1")
        .values()
        .flatten()
        .all(|chip| chip.thumbnail_url.is_none()));
    file(dir.path())
        .execute("UPDATE messages SET role = 'person'", [])
        .expect("person message");
    file(dir.path())
        .execute("DELETE FROM _syncular_blobs", [])
        .expect("cache miss");
    assert_eq!(
        thumbnails::blob(&alpha, &blob),
        Err(OpenError::NotHere),
        "a thumbnail cache entry cannot authorize a download or outlive the blob"
    );
}

#[test]
fn thumbnail_cache_miss_never_uses_the_download_port_even_when_online() {
    let (replica, source) = fake(vec![json!({"mime": "image/png"})], LinkState::Connected);
    assert_eq!(
        crate::hq::thumbnails::blob(&source, "missing-photo"),
        Err(OpenError::NotHere)
    );
    assert!(
        replica.fetched_blobs.lock().expect("downloads").is_empty(),
        "showing a thumbnail must never attempt to download an uncached image"
    );
}
