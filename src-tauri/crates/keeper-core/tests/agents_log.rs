//! An agent session's log over real files (story 89.5, AD-365, AD-366,
//! NFR-116, NFR-117): the chunk writer's torn tail, rotation, blobs and
//! symlink refusal; the reader's merge and epoch fence; replay against a real
//! tool loop on a real socket; and the `.keeper/` index with the logs gone.

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use keeper_core::agents::index::Index;
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::reader::{hydrate_blob, read_session};
use keeper_core::agents::log::replay::{replay, COMPACT_HEADING};
use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
use keeper_core::agents::log::{
    AssistantBody, ChunkName, ClaimAction, ClaimBody, CompactBody, ErrorBody, HostSlug, LineBody,
    LogError, LogLine, OpenBody, RunBody, RunState, ToolCallBody, ToolOutcomeWord, ToolResultBody,
    Usage, UserBody, LINE_VERSION,
};
use keeper_core::agents::redact::redact_secrets;
use keeper_core::agents::session::SessionKind;
use keeper_core::bots::chat::{
    build_body, cancellation, ChatEvent, ChatMessage, ChatOptions, ChatRequest, Role,
};
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::GrantMode;
use keeper_core::bots::http::client;
use keeper_core::bots::tools::{
    self, render_result, run_tool_loop_reporting, ToolCall, ToolHost, ToolLoop, ToolLoopEvent,
    ToolLoopOptions, ToolOutcome,
};
use keeper_core::bots::{Endpoint, ProviderKind};
use matrix_sdk::ruma::{OwnedUserId, UserId};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use ulid::Ulid;

// ---------------------------------------------------------------------------
// Scratch folders and lines
// ---------------------------------------------------------------------------

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("keeper-agents-log-{}", Ulid::new()));
        fs::create_dir_all(&dir).expect("scratch");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn host(slug: &str) -> HostSlug {
    HostSlug::new(slug).expect("host")
}

fn user(id: &str) -> OwnedUserId {
    UserId::parse(id).expect("user")
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).expect("date")
}

fn at(ms: i64) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 30, 8, 0, 0)
        .single()
        .expect("ts")
        + chrono::Duration::milliseconds(ms)
}

fn tg_label() -> Label {
    Label {
        readers: Readers::Only([user("@tgorka:h")].into_iter().collect()),
        integrity: Integrity::Owner,
        local_only: false,
    }
}

/// A line whose id sorts with its time, as a monotonic generator's would.
fn line(slug: &str, epoch: u64, ts: DateTime<Utc>, seq: u64, body: LineBody) -> LogLine {
    LogLine {
        v: LINE_VERSION,
        id: Ulid::from_parts(ts.timestamp_millis() as u64, u128::from(seq)),
        parent: None,
        ts,
        host: host(slug),
        epoch,
        claim: Some(format!("$claim{epoch}")),
        matrix_event: None,
        body,
    }
}

fn user_line(slug: &str, epoch: u64, ts: DateTime<Utc>, seq: u64, text: &str) -> LogLine {
    line(
        slug,
        epoch,
        ts,
        seq,
        LineBody::User(UserBody {
            sender: user("@tgorka:h"),
            text: text.to_owned(),
            attachments: Vec::new(),
        }),
    )
}

fn claim_line(slug: &str, epoch: u64, ts: DateTime<Utc>, seq: u64, event: &str) -> LogLine {
    let mut line = line(
        slug,
        epoch,
        ts,
        seq,
        LineBody::Claim(ClaimBody {
            epoch,
            action: ClaimAction::Acquired,
            from_host: None,
            claim_event: event.to_owned(),
            server_ts: ts.to_rfc3339(),
        }),
    );
    line.claim = Some(event.to_owned());
    line
}

fn chunk_path(session: &Path, name: &str) -> PathBuf {
    session.join("log").join(name)
}

fn texts(log: &keeper_core::agents::log::reader::SessionLog) -> Vec<String> {
    log.lines
        .iter()
        .filter_map(|line| match &line.body {
            LineBody::User(body) => Some(body.text.clone()),
            _ => None,
        })
        .collect()
}

const ROTATE: u64 = 192 * 1024;

// ---------------------------------------------------------------------------
// Torn tails (NFR-117)
// ---------------------------------------------------------------------------

#[test]
fn a_torn_tail_of_the_hosts_own_chunk_is_truncated_on_open_and_the_rest_reads() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    {
        let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
        writer
            .append(&user_line("electra", 1, at(0), 1, "one"))
            .expect("one");
        writer
            .append(&user_line("electra", 1, at(1), 2, "two"))
            .expect("two");
        writer.sync().expect("sync");
    }
    let path = chunk_path(session, "2026-09-30.electra.1.jsonl");
    let whole = fs::read(&path).expect("chunk");
    // The host died half way through a third line.
    let mut torn = whole.clone();
    torn.extend_from_slice(br#"{"v":1,"id":"01K6DQ"#);
    fs::write(&path, &torn).expect("tear");

    let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("reopen");
    assert_eq!(
        fs::read(&path).expect("chunk"),
        whole,
        "truncated to the last newline"
    );
    writer
        .append(&user_line("electra", 1, at(2), 3, "three"))
        .expect("three");
    writer.sync().expect("sync");

    let log = read_session(session);
    assert!(log.problems.is_empty(), "{:?}", log.problems);
    assert_eq!(texts(&log), ["one", "two", "three"]);
}

#[test]
fn another_hosts_torn_tail_is_left_alone_and_skipped() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    {
        let mut hesperia =
            ChunkWriter::open(session, &host("hesperia"), ROTATE, day()).expect("open");
        hesperia
            .append(&user_line("hesperia", 1, at(0), 1, "h1"))
            .expect("h1");
        hesperia.sync().expect("sync");
    }
    let theirs = chunk_path(session, "2026-09-30.hesperia.1.jsonl");
    let mut torn = fs::read(&theirs).expect("chunk");
    torn.extend_from_slice(br#"{"v":1,"id":"01K6"#);
    fs::write(&theirs, &torn).expect("tear");

    let mut electra = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
    electra
        .append(&user_line("electra", 1, at(5), 2, "e1"))
        .expect("e1");
    electra.sync().expect("sync");

    assert_eq!(
        fs::read(&theirs).expect("chunk"),
        torn,
        "hesperia's bytes untouched"
    );
    let log = read_session(session);
    assert_eq!(texts(&log), ["h1", "e1"]);
    assert_eq!(log.problems.len(), 1, "{:?}", log.problems);
    assert_eq!(log.problems[0].chunk, "2026-09-30.hesperia.1.jsonl");
    assert!(log.problems[0].sentence.contains("half a line"));
    assert_eq!(
        fs::read(&theirs).expect("chunk"),
        torn,
        "reading changes nothing"
    );
}

// ---------------------------------------------------------------------------
// Rotation (NFR-116)
// ---------------------------------------------------------------------------

#[test]
fn rotate_at_keeps_chunks_under_three_quarters_of_the_lfs_threshold() {
    assert_eq!(rotate_at(4 * 1024 * 1024), 192 * 1024);
    assert_eq!(rotate_at(256 * 1024), 192 * 1024);
    assert_eq!(rotate_at(128 * 1024), 96 * 1024);
}

fn chunks_of(session: &Path) -> Vec<(ChunkName, u64)> {
    let mut out: Vec<(ChunkName, u64)> = fs::read_dir(session.join("log"))
        .expect("log")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.parse::<ChunkName>().ok()?;
            Some((name, entry.metadata().ok()?.len()))
        })
        .collect();
    out.sort();
    out
}

#[test]
fn no_chunk_ever_reaches_rotate_at() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let limit = rotate_at(256 * 1024);
    let mut writer = ChunkWriter::open(session, &host("electra"), limit, day()).expect("open");
    // Realistic sizes: mostly short messages, some long answers, now and
    // then a tool result just under the blob threshold.
    for i in 0..10_000u64 {
        let len = match i % 20 {
            0 => 15_000,
            1..=3 => 2_500,
            _ => 40 + (i * 37 % 400) as usize,
        };
        let text = "x".repeat(len);
        writer
            .append(&user_line("electra", 1, at(i as i64), i, &text))
            .expect("append");
    }
    writer.sync().expect("sync");

    let chunks = chunks_of(session);
    assert!(chunks.len() > 10, "{} chunks", chunks.len());
    for (index, (name, bytes)) in chunks.iter().enumerate() {
        assert!(*bytes < limit, "{name} is {bytes} bytes");
        assert_eq!(name.n as usize, index + 1, "numbered 1…n with no gap");
    }
    let log = read_session(session);
    assert_eq!(log.lines.len(), 10_000);
    assert!(log.problems.is_empty());
}

#[test]
fn a_utc_date_change_starts_a_new_chunk() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut writer = ChunkWriter::open(session, &host("electra"), 40 * 1024, day()).expect("open");
    let late = Utc
        .with_ymd_and_hms(2026, 9, 30, 23, 59, 59)
        .single()
        .expect("ts");
    // Two chunks on the first day, by size.
    for i in 0..4 {
        writer
            .append(&user_line("electra", 1, late, i, &"y".repeat(12_000)))
            .expect("append");
    }
    let midnight = Utc
        .with_ymd_and_hms(2026, 10, 1, 0, 0, 0)
        .single()
        .expect("ts");
    let receipt = writer
        .append(&user_line("electra", 1, midnight, 9, "tomorrow"))
        .expect("append");
    assert_eq!(receipt.chunk.to_string(), "2026-10-01.electra.1.jsonl");
    assert_eq!(receipt.offset, 0);
    let names: Vec<String> = chunks_of(session)
        .iter()
        .map(|(n, _)| n.to_string())
        .collect();
    assert_eq!(
        names,
        [
            "2026-09-30.electra.1.jsonl",
            "2026-09-30.electra.2.jsonl",
            "2026-10-01.electra.1.jsonl"
        ]
    );
}

// ---------------------------------------------------------------------------
// Blobs
// ---------------------------------------------------------------------------

fn result_line(seq: u64, content: String) -> LogLine {
    line(
        "electra",
        1,
        at(seq as i64),
        seq,
        LineBody::ToolResult(ToolResultBody {
            call_id: "call_1".to_owned(),
            outcome: ToolOutcomeWord::Ok,
            content,
            truncated: None,
            label: tg_label(),
        }),
    )
}

#[test]
fn a_body_over_16_kib_becomes_a_blob_written_before_its_line() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
    let content = "the changelog\n".repeat(1_400);
    let first = result_line(1, content.clone());
    let receipt = writer.append(&first).expect("append");
    let sha = receipt.blob.clone().expect("a blob");
    assert_eq!(receipt.line, first, "the receipt carries the body inline");

    let blob_path = session.join("log/blobs").join(format!("{sha}.json"));
    let stored = fs::read(&blob_path).expect("blob exists");
    assert_eq!(
        hex::encode(Sha256::digest(&stored)),
        sha,
        "named by the hash of its bytes"
    );

    let chunk = fs::read_to_string(chunk_path(session, &receipt.chunk.to_string())).expect("chunk");
    let written: serde_json::Value = serde_json::from_str(chunk.trim_end()).expect("line");
    assert_eq!(written["kind"], "tool_result");
    assert_eq!(
        written["body"],
        json!({ "blob": sha, "bytes": stored.len() })
    );

    // The same body again reuses the blob.
    let again = writer.append(&result_line(2, content)).expect("again");
    assert_eq!(again.blob.as_deref(), Some(sha.as_str()));
    assert_eq!(
        fs::read_dir(session.join("log/blobs"))
            .expect("blobs")
            .count(),
        1
    );

    let log = read_session(session);
    let hydrated = hydrate_blob(session, &sha).expect("hydrate");
    let replayed = replay(&log, &|name| hydrate_blob(session, name)).expect("replay");
    assert_eq!(replayed.messages.len(), 2);
    assert_eq!(hydrated["call_id"], "call_1");

    // A blob that was changed no longer matches its name.
    fs::write(&blob_path, b"{}").expect("tamper");
    assert!(matches!(
        hydrate_blob(session, &sha),
        Err(LogError::BlobMismatch { .. })
    ));
    assert!(matches!(
        hydrate_blob(session, "../../etc/passwd"),
        Err(LogError::BadBlobName { .. })
    ));
}

#[test]
fn a_line_still_over_64_kib_after_blobbing_is_refused_and_not_written() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
    writer
        .append(&user_line("electra", 1, at(0), 1, "before"))
        .expect("before");
    let mut huge = user_line("electra", 1, at(1), 2, "small body");
    huge.claim = Some("c".repeat(70 * 1024));
    let before = fs::read(chunk_path(session, "2026-09-30.electra.1.jsonl")).expect("chunk");
    assert!(matches!(
        writer.append(&huge),
        Err(LogError::LineTooLong { .. })
    ));
    assert_eq!(
        fs::read(chunk_path(session, "2026-09-30.electra.1.jsonl")).expect("chunk"),
        before
    );
}

// ---------------------------------------------------------------------------
// One writer, never a symlink
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn the_writer_refuses_a_symlinked_log_directory() {
    let scratch = Scratch::new();
    let elsewhere = Scratch::new();
    let session = scratch.0.join("session");
    fs::create_dir_all(&session).expect("session");
    std::os::unix::fs::symlink(&elsewhere.0, session.join("log")).expect("link");
    assert!(matches!(
        ChunkWriter::open(&session, &host("electra"), ROTATE, day()),
        Err(LogError::Symlink { .. })
    ));
    assert_eq!(fs::read_dir(&elsewhere.0).expect("target").count(), 0);

    // A chunk that is a link is refused as well.
    let other = Scratch::new();
    fs::create_dir_all(other.0.join("log")).expect("log");
    let target = elsewhere.0.join("x.jsonl");
    fs::write(&target, b"").expect("target");
    std::os::unix::fs::symlink(&target, chunk_path(&other.0, "2026-09-30.electra.1.jsonl"))
        .expect("link");
    assert!(ChunkWriter::open(&other.0, &host("electra"), ROTATE, day()).is_err());
    assert_eq!(fs::read(&target).expect("target"), b"");
}

#[test]
fn a_writer_never_appends_a_line_of_another_host() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    {
        let mut hesperia =
            ChunkWriter::open(session, &host("hesperia"), ROTATE, day()).expect("open");
        hesperia
            .append(&user_line("hesperia", 1, at(0), 1, "mine"))
            .expect("mine");
    }
    let theirs = chunk_path(session, "2026-09-30.hesperia.1.jsonl");
    let before = fs::read(&theirs).expect("chunk");
    let mut electra = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
    assert!(matches!(
        electra.append(&user_line("hesperia", 1, at(1), 2, "forged")),
        Err(LogError::ForeignHost { .. })
    ));
    assert_eq!(fs::read(&theirs).expect("chunk"), before);
    assert_eq!(chunks_of(session).len(), 1, "electra opened nothing");
}

// ---------------------------------------------------------------------------
// Merge and the epoch fence
// ---------------------------------------------------------------------------

#[test]
fn two_hosts_chunks_merge_by_ts_host_id() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut electra = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("e");
    let mut hesperia = ChunkWriter::open(session, &host("hesperia"), ROTATE, day()).expect("h");
    electra
        .append(&user_line("electra", 1, at(0), 1, "e@0"))
        .expect("w");
    electra
        .append(&user_line("electra", 1, at(20), 2, "e@20"))
        .expect("w");
    electra
        .append(&user_line("electra", 1, at(30), 3, "e@30"))
        .expect("w");
    hesperia
        .append(&user_line("hesperia", 1, at(10), 4, "h@10"))
        .expect("w");
    hesperia
        .append(&user_line("hesperia", 1, at(30), 5, "h@30"))
        .expect("w");
    // Same ts and host: the id decides.
    hesperia
        .append(&user_line("hesperia", 1, at(40), 7, "h@40b"))
        .expect("w");
    hesperia
        .append(&user_line("hesperia", 1, at(40), 6, "h@40a"))
        .expect("w");
    let log = read_session(session);
    assert_eq!(
        texts(&log),
        ["e@0", "h@10", "e@20", "e@30", "h@30", "h@40a", "h@40b"]
    );
}

#[test]
fn a_superseded_epochs_late_lines_are_dropped() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let t = 60_000;
    let mut electra = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("e");
    let mut hesperia = ChunkWriter::open(session, &host("hesperia"), ROTATE, day()).expect("h");
    electra
        .append(&claim_line("electra", 1, at(0), 1, "$c1"))
        .expect("w");
    electra
        .append(&user_line("electra", 1, at(t - 5_000), 2, "before"))
        .expect("w");
    hesperia
        .append(&claim_line("hesperia", 2, at(t), 3, "$c2"))
        .expect("w");
    electra
        .append(&user_line("electra", 1, at(t + 5_000), 4, "late"))
        .expect("w");
    hesperia
        .append(&user_line("hesperia", 2, at(t + 6_000), 5, "after"))
        .expect("w");
    // The loser notices: its `lost` is written after the takeover by nature.
    let mut lost = claim_line("electra", 1, at(t + 7_000), 6, "$c1");
    if let LineBody::Claim(body) = &mut lost.body {
        body.action = ClaimAction::Lost;
    }
    electra.append(&lost).expect("w");

    let log = read_session(session);
    assert_eq!(texts(&log), ["before", "after"]);
    assert!(!log.conflicted);
    assert_eq!(log.problems.len(), 1);
    assert!(log.problems[0].sentence.contains("newer epoch"));
    // The dropped line is named where it is: electra's chunk, line 3.
    assert_eq!(log.problems[0].chunk, "2026-09-30.electra.1.jsonl");
    assert_eq!(log.problems[0].line, Some(3));
    assert!(
        log.lines.iter().any(|line| matches!(
            &line.body,
            LineBody::Claim(claim) if claim.action == ClaimAction::Lost
        )),
        "a claim transition is never fenced"
    );

    // Only an acquisition fences: a release at a newer epoch drops nothing,
    // and two releases of one epoch with different events are no conflict.
    let calm = Scratch::new();
    let mut only = ChunkWriter::open(&calm.0, &host("electra"), ROTATE, day()).expect("e");
    only.append(&claim_line("electra", 1, at(0), 1, "$c1"))
        .expect("w");
    for (seq, event) in [(2, "$r1"), (3, "$r2")] {
        let mut released = claim_line("electra", 2, at(t), seq, event);
        if let LineBody::Claim(body) = &mut released.body {
            body.action = ClaimAction::Released;
        }
        only.append(&released).expect("w");
    }
    only.append(&user_line("electra", 1, at(t + 5_000), 4, "kept"))
        .expect("w");
    let log = read_session(&calm.0);
    assert_eq!(texts(&log), ["kept"]);
    assert!(!log.conflicted, "{:?}", log.problems);
}

#[test]
fn a_double_acquire_at_one_epoch_is_a_conflict_and_replay_refuses() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut electra = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("e");
    let mut hesperia = ChunkWriter::open(session, &host("hesperia"), ROTATE, day()).expect("h");
    electra
        .append(&claim_line("electra", 2, at(0), 1, "$a"))
        .expect("w");
    hesperia
        .append(&claim_line("hesperia", 2, at(10), 2, "$b"))
        .expect("w");
    // The same claim event logged twice is not a conflict.
    hesperia
        .append(&claim_line("hesperia", 2, at(20), 3, "$b"))
        .expect("w");
    let log = read_session(session);
    assert!(log.conflicted);
    assert!(log
        .problems
        .iter()
        .any(|p| p.sentence.contains("conflicted")));
    let refusal = replay(&log, &|name| hydrate_blob(session, name)).expect_err("refused");
    assert!(refusal.sentence().contains("two truths"));

    let calm = Scratch::new();
    let mut only = ChunkWriter::open(&calm.0, &host("electra"), ROTATE, day()).expect("e");
    only.append(&claim_line("electra", 2, at(0), 1, "$a"))
        .expect("w");
    only.append(&claim_line("electra", 2, at(10), 2, "$a"))
        .expect("w");
    assert!(!read_session(&calm.0).conflicted);
}

// ---------------------------------------------------------------------------
// Compaction
// ---------------------------------------------------------------------------

#[test]
fn compact_replaces_the_lines_through_it_with_one_summary() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
    writer
        .append(&user_line("electra", 1, at(0), 1, "first"))
        .expect("w");
    let answer = line(
        "electra",
        1,
        at(1),
        2,
        LineBody::Assistant(AssistantBody {
            text: "first answer".into(),
            model: "m".into(),
            finish: "stop".into(),
            usage: Usage::default(),
            ttft_ms: None,
            duration_ms: 1,
            anchor_event: None,
        }),
    );
    writer.append(&answer).expect("w");
    writer
        .append(&user_line("electra", 1, at(2), 3, "second"))
        .expect("w");
    writer
        .append(&line(
            "electra",
            1,
            at(3),
            4,
            LineBody::Compact(CompactBody {
                summary: "They asked one thing and got an answer.".into(),
                replaces_through: answer.id,
            }),
        ))
        .expect("w");
    let replayed = replay(&read_session(session), &|n| hydrate_blob(session, n)).expect("replay");
    assert_eq!(replayed.messages.len(), 2);
    assert_eq!(replayed.messages[0].role, Role::System);
    let body = build_body(
        ProviderKind::Ollama,
        &ChatRequest {
            model: "m".into(),
            messages: replayed.messages,
            ..ChatRequest::default()
        },
    )
    .expect("body");
    assert_eq!(
        body["messages"][0]["content"],
        format!("{COMPACT_HEADING}\n\nThey asked one thing and got an answer.")
    );
    assert_eq!(body["messages"][1]["content"], "second");
}

// ---------------------------------------------------------------------------
// Replay against a real tool loop (FR-774)
// ---------------------------------------------------------------------------

type Script = Arc<dyn Fn(u32) -> Vec<u8> + Send + Sync + 'static>;

struct TestServer {
    addr: SocketAddr,
    bodies: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    async fn start(script: Script) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&bodies);
        let task = tokio::spawn(async move {
            let mut nth = 0;
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let script = Arc::clone(&script);
                let seen = Arc::clone(&seen);
                let this = nth;
                nth += 1;
                tokio::spawn(async move {
                    let body = read_request(&mut stream).await;
                    if let Ok(mut bodies) = seen.lock() {
                        bodies.push(body);
                    }
                    let _ = stream.write_all(&script(this)).await;
                    let _ = stream.flush().await;
                });
            }
        });
        Self { addr, bodies, task }
    }

    fn bodies(&self) -> Vec<String> {
        self.bodies.lock().expect("bodies").clone()
    }
}

async fn read_request(stream: &mut TcpStream) -> String {
    let mut buffer = Vec::new();
    let mut scratch = [0u8; 8192];
    loop {
        let Ok(read) = stream.read(&mut scratch).await else {
            return String::new();
        };
        if read == 0 {
            return String::new();
        }
        buffer.extend_from_slice(&scratch[..read]);
        let Some(head_end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_ascii_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        if buffer.len() >= head_end + 4 + length {
            return String::from_utf8_lossy(&buffer[head_end + 4..head_end + 4 + length])
                .into_owned();
        }
    }
}

fn sse(frames: &[serde_json::Value]) -> Vec<u8> {
    let mut body = String::new();
    for frame in frames {
        body.push_str(&format!("data: {frame}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    let mut out =
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n".to_vec();
    out.extend_from_slice(body.as_bytes());
    out
}

/// A completion that says something and then calls tools.
fn tool_round(text: &str, calls: &[(&str, &str, serde_json::Value)]) -> Vec<u8> {
    let fragments: Vec<serde_json::Value> = calls
        .iter()
        .enumerate()
        .map(|(index, (id, name, args))| {
            json!({
                "index": index, "id": id, "type": "function",
                "function": { "name": name, "arguments": args.to_string() },
            })
        })
        .collect();
    let mut frames = Vec::new();
    if !text.is_empty() {
        frames.push(json!({ "choices": [{ "index": 0, "delta": { "content": text } }] }));
    }
    frames.push(json!({ "choices": [{ "index": 0, "delta": { "tool_calls": fragments } }] }));
    frames.push(json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }] }));
    sse(&frames)
}

fn prose_round(text: &str) -> Vec<u8> {
    sse(&[
        json!({ "choices": [{ "index": 0, "delta": { "content": text } }] }),
        json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] }),
    ])
}

/// A drive that answers from a table, refusing one path.
struct Drive {
    secret: Option<String>,
}

impl ToolHost for Drive {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        let path = call.target.display_path();
        if path.contains("private") {
            return Ok(ToolOutcome::Refused {
                reason: "notes/private.md is outside this grant.".into(),
            });
        }
        let mut body = format!("# {path}\n\nShip the agents rung on Friday.");
        if let Some(secret) = &self.secret {
            body.push_str(&format!("\nThe deploy token is {secret}."));
        }
        Ok(ToolOutcome::Text {
            body,
            truncated_at: None,
            of_bytes: None,
            okf: None,
        })
    }
}

const SYSTEM: &str = "You are amelia. What you read here may be shown only to: tgorka.";
const ASK: &str = "What does the plan say, and what is in notes/b.md?";

fn turn_request() -> ChatRequest {
    ChatRequest {
        model: "llama3.1".to_owned(),
        messages: vec![
            ChatMessage::text(Role::System, SYSTEM),
            ChatMessage::text(Role::User, ASK),
        ],
        tools: tools::tool_specs(GrantMode::Read),
        ..ChatRequest::default()
    }
}

/// What the turn loop's caller keeps while it logs one turn.
struct Logging {
    writer: ChunkWriter,
    seq: u64,
    text: String,
    finish: String,
    assistant: Option<Ulid>,
    last_call: Option<Ulid>,
}

impl Logging {
    fn next(&mut self, parent: Option<Ulid>, body: LineBody) -> Ulid {
        self.seq += 1;
        let mut entry = line("electra", 1, at(self.seq as i64 * 10), self.seq, body);
        entry.parent = parent;
        let id = entry.id;
        self.writer.append(&entry).expect("append");
        id
    }

    fn assistant(&mut self) -> Ulid {
        if let Some(id) = self.assistant {
            return id;
        }
        let body = LineBody::Assistant(AssistantBody {
            text: std::mem::take(&mut self.text),
            model: "llama3.1".into(),
            finish: self.finish.clone(),
            usage: Usage::default(),
            ttft_ms: None,
            duration_ms: 0,
            anchor_event: None,
        });
        let id = self.next(None, body);
        self.assistant = Some(id);
        id
    }
}

/// Run one real tool loop against the stub, logging every step through a
/// `ChunkWriter`; return what the stub received last and what replay builds.
async fn logged_turn(session: &Path, secret: Option<&str>) -> (String, String) {
    // With a secret, the model echoes it into a tool call's arguments too.
    let b_path = match secret {
        Some(secret) => format!("notes/b-{secret}.md"),
        None => "notes/b.md".to_owned(),
    };
    let server = TestServer::start(Arc::new(move |nth| match nth {
        0 => tool_round(
            "Let me look.",
            &[
                ("call_a", "drive_read", json!({ "path": "notes/plan.md" })),
                (
                    "call_b",
                    "drive_read",
                    json!({ "path": "notes/private.md" }),
                ),
            ],
        ),
        1 => tool_round(
            "",
            &[("call_c", "drive_read", json!({ "path": b_path.as_str() }))],
        ),
        _ => prose_round("The plan says ship on Friday; b.md agrees."),
    }))
    .await;
    let endpoint = Endpoint {
        kind: ProviderKind::Ollama,
        base_url: format!("http://{}", server.addr),
        bot: None,
        token: None,
    };
    let http = client(Duration::from_secs(5)).expect("client");
    let drive = Drive {
        secret: secret.map(str::to_owned),
    };
    let context = ToolLoop {
        client: &http,
        endpoint: &endpoint,
        host: &drive,
        default_profile_id: "tgdrive",
    };
    let options = ChatOptions {
        read_timeout: Duration::from_secs(5),
        max_attempts: 1,
        retry_backoff: Duration::from_millis(10),
        ..ChatOptions::default()
    };

    let state = Arc::new(Mutex::new(Logging {
        writer: ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("writer"),
        seq: 0,
        text: String::new(),
        finish: String::new(),
        assistant: None,
        last_call: None,
    }));
    {
        let mut log = state.lock().expect("state");
        log.next(
            None,
            LineBody::Open(OpenBody {
                agent: "amelia".into(),
                drive: "tgdrive".into(),
                kind: SessionKind::Main,
                title: "The plan".into(),
                requested_by: user("@tgorka:h"),
                label: tg_label(),
                drives: vec!["tgdrive".into()],
                model: "bot:ollama:http://127.0.0.1:11434#llama3.1".into(),
                prompt_sha256: hex::encode(Sha256::digest(SYSTEM.as_bytes())),
                memory_sha256: hex::encode(Sha256::digest(b"")),
            }),
        );
        log.next(
            None,
            LineBody::User(UserBody {
                sender: user("@tgorka:h"),
                text: ASK.into(),
                attachments: Vec::new(),
            }),
        );
    }

    let for_sink = Arc::clone(&state);
    let mut sink = move |event: ToolLoopEvent| {
        let mut log = for_sink.lock().expect("state");
        match event {
            ToolLoopEvent::RoundStarted { .. } => {
                log.text.clear();
                log.assistant = None;
            }
            ToolLoopEvent::Chat(ChatEvent::ContentDelta(delta)) => log.text.push_str(&delta),
            ToolLoopEvent::Chat(ChatEvent::Finished { reason }) => {
                log.finish = format!("{reason:?}").to_lowercase();
            }
            _ => {}
        }
    };
    let for_report = Arc::clone(&state);
    let mut report = move |_: &tools::ToolCallRecord,
                           wire: &keeper_core::bots::chat::ToolCall,
                           outcome: &ToolOutcome| {
        let mut log = for_report.lock().expect("state");
        let assistant = log.assistant();
        let call = log.next(
            Some(assistant),
            LineBody::ToolCall(ToolCallBody {
                call_id: wire.id.clone(),
                tool: wire.name.clone(),
                args: wire.arguments_raw.clone(),
                tier: 0,
                grant_id: None,
            }),
        );
        log.last_call = Some(call);
        log.next(
            Some(call),
            LineBody::ToolResult(ToolResultBody {
                call_id: wire.id.clone(),
                outcome: match outcome {
                    ToolOutcome::Refused { .. } => ToolOutcomeWord::Refused,
                    _ => ToolOutcomeWord::Ok,
                },
                content: render_result(outcome),
                truncated: None,
                label: tg_label(),
            }),
        );
    };
    let (_handle, cancel) = cancellation();
    let outcome = run_tool_loop_reporting(
        &context,
        &turn_request(),
        &options,
        &ToolLoopOptions::default(),
        cancel,
        &mut sink,
        &mut report,
    )
    .await
    .expect("loop");
    assert_eq!(outcome.rounds, 3);
    assert_eq!(outcome.calls.len(), 3);
    assert!(outcome.calls[1].refusal.is_some(), "one call refused");
    state.lock().expect("state").writer.sync().expect("sync");

    let bodies = server.bodies();
    assert_eq!(bodies.len(), 3);
    let last = bodies[2].clone();

    // A fresh read of the files, as another host would do it.
    let log = read_session(session);
    assert!(log.problems.is_empty(), "{:?}", log.problems);
    let replayed = replay(&log, &|name| hydrate_blob(session, name)).expect("replay");
    assert!(replayed.last_open.is_some());
    let mut messages = vec![ChatMessage::text(Role::System, SYSTEM)];
    messages.extend(replayed.messages);
    let rebuilt = build_body(
        ProviderKind::Ollama,
        &ChatRequest {
            messages,
            ..turn_request()
        },
    )
    .expect("body");
    (last, serde_json::to_string(&rebuilt).expect("json"))
}

#[tokio::test]
async fn replay_reproduces_the_request_body_byte_for_byte() {
    let scratch = Scratch::new();
    let (sent, replayed) = logged_turn(&scratch.0, None).await;
    assert!(
        sent.contains("\"tool_calls\""),
        "the last request replays tool calls"
    );
    assert!(sent.contains("\"role\":\"tool\""));
    assert!(
        sent.contains("outside this grant"),
        "the refusal is in the trace"
    );
    assert_eq!(replayed, sent);
}

#[tokio::test]
async fn a_secret_never_reaches_the_log_and_replay_differs_exactly_at_it() {
    let scratch = Scratch::new();
    let secret = "ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";
    let (sent, replayed) = logged_turn(&scratch.0, Some(secret)).await;
    assert!(sent.contains(secret), "the model saw it");
    let marker = redact_secrets(secret).text;
    assert_ne!(marker, secret);
    assert_eq!(replayed, sent.replace(secret, &marker));
    assert!(
        sent.contains(&format!("notes/b-{secret}.md")),
        "echoed in arguments"
    );

    // Every other field a model or a tool writes: a summary, an error, and a
    // result large enough to become a blob.
    let mut writer =
        ChunkWriter::open(&scratch.0, &host("electra"), ROTATE, day()).expect("writer");
    let summary = format!("Earlier the deploy token {secret} was read.");
    for (seq, body) in [
        (
            901,
            LineBody::Compact(CompactBody {
                summary: summary.clone(),
                replaces_through: Ulid::from_parts(0, 1),
            }),
        ),
        (
            902,
            LineBody::Error(ErrorBody {
                sentence: format!("The push with {secret} was refused."),
                code: "push".into(),
            }),
        ),
    ] {
        writer
            .append(&line("electra", 1, at(seq * 10), seq as u64, body))
            .expect("append");
    }
    let big = format!("{}\nThe token is {secret}.", "x".repeat(20_000));
    let receipt = writer.append(&result_line(903, big)).expect("append");
    assert!(receipt.blob.is_some(), "the result became a blob");
    writer.sync().expect("sync");

    let mut files = vec![scratch.0.join("log")];
    let mut read = 0;
    while let Some(dir) = files.pop() {
        for entry in fs::read_dir(&dir).expect("dir").filter_map(Result::ok) {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                files.push(entry.path());
            } else {
                let bytes = fs::read_to_string(entry.path()).expect("file");
                assert!(!bytes.contains(secret), "{}", entry.path().display());
                read += 1;
            }
        }
    }
    assert!(read >= 2, "chunks and the blob were read");
    assert!(scratch.0.join("log/blobs").is_dir());
}

#[test]
fn yesterdays_torn_tail_is_repaired_when_the_host_reopens_today() {
    let scratch = Scratch::new();
    let session = &scratch.0;
    {
        let mut writer = ChunkWriter::open(session, &host("electra"), ROTATE, day()).expect("open");
        writer
            .append(&user_line("electra", 1, at(0), 1, "yesterday"))
            .expect("w");
        writer.sync().expect("sync");
    }
    let path = chunk_path(session, "2026-09-30.electra.1.jsonl");
    let whole = fs::read(&path).expect("chunk");
    let mut torn = whole.clone();
    torn.extend_from_slice(br#"{"v":1,"id":"01K6"#);
    fs::write(&path, &torn).expect("tear");

    let tomorrow = day().succ_opt().expect("tomorrow");
    let writer = ChunkWriter::open(session, &host("electra"), ROTATE, tomorrow).expect("reopen");
    assert_eq!(
        fs::read(&path).expect("chunk"),
        whole,
        "truncated whatever its date"
    );
    assert_eq!(
        writer.current_chunk(),
        None,
        "yesterday's chunk is not resumed"
    );
    assert!(read_session(session).problems.is_empty());
}

#[test]
fn a_chunk_bound_too_small_for_an_inline_body_is_refused() {
    let scratch = Scratch::new();
    assert!(matches!(
        ChunkWriter::open(&scratch.0, &host("electra"), rotate_at(16 * 1024), day()),
        Err(LogError::RotateTooSmall { .. })
    ));
    assert!(ChunkWriter::open(&scratch.0, &host("electra"), 32 * 1024, day()).is_ok());
}

#[test]
fn a_raw_line_after_midnight_starts_a_new_chunk() {
    let scratch = Scratch::new();
    let mut writer = ChunkWriter::open(&scratch.0, &host("electra"), ROTATE, day()).expect("open");
    let first = writer.append_line(day(), "{}").expect("first");
    let next = writer
        .append_line(day().succ_opt().expect("tomorrow"), "{}")
        .expect("next");
    assert_eq!(first.chunk.to_string(), "2026-09-30.electra.1.jsonl");
    assert_eq!(next.chunk.to_string(), "2026-10-01.electra.1.jsonl");
}

#[cfg(unix)]
#[test]
fn a_chunk_that_is_a_link_is_named_as_a_problem() {
    let scratch = Scratch::new();
    let elsewhere = Scratch::new();
    fs::create_dir_all(scratch.0.join("log")).expect("log");
    let target = elsewhere.0.join("x.jsonl");
    fs::write(&target, b"").expect("target");
    std::os::unix::fs::symlink(
        &target,
        chunk_path(&scratch.0, "2026-09-30.hesperia.1.jsonl"),
    )
    .expect("link");
    let log = read_session(&scratch.0);
    assert_eq!(log.problems.len(), 1, "{:?}", log.problems);
    assert_eq!(log.problems[0].chunk, "2026-09-30.hesperia.1.jsonl");
}

// ---------------------------------------------------------------------------
// The index (NFR-116's hot-path half)
// ---------------------------------------------------------------------------

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("dir");
    for entry in fs::read_dir(from).expect("read").filter_map(Result::ok) {
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

const FIXTURE_SESSION: &str = "active/2026-09-30-release-notes";

#[test]
fn the_index_answers_with_the_logs_gone() {
    let scratch = Scratch::new();
    let zone = scratch.0.join("60-sessions");
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agents/sessions"),
        &zone,
    );
    let session_dir = zone.join(FIXTURE_SESSION);

    let mut index = Index::open(&zone).expect("open");
    assert!(index.needs_rebuild());
    let report = index.rebuild().expect("rebuild");
    assert_eq!(report.sessions, 1);
    assert!(report.problems.is_empty(), "{:?}", report.problems);

    // The logs go away; the index still answers.
    fs::rename(session_dir.join("log"), scratch.0.join("log-away")).expect("rename");
    let row = index.session(FIXTURE_SESSION).expect("query").expect("row");
    assert_eq!(row.agent, "amelia");
    assert_eq!(row.kind, "delegated");
    assert_eq!(
        row.label,
        Label {
            readers: Readers::Only([user("@tgorka:h")].into_iter().collect()),
            integrity: Integrity::Agent,
            local_only: false,
        }
    );
    assert_eq!(row.scope, ["tgdrive", "neuradrive"]);
    assert_eq!(row.run.as_deref(), Some("blocked"));
    assert_eq!(row.claim_host.as_deref(), Some("hesperia"));
    assert_eq!(row.claim_epoch, Some(2));
    assert_eq!(row.lines, 11);
    assert_eq!(row.last_ts.as_deref(), Some("2026-09-30T09:00:03.000Z"));
    assert!(index.seen(FIXTURE_SESSION, "$brief:h").expect("seen"));
    assert!(!index.seen(FIXTURE_SESSION, "$other:h").expect("seen"));
    let chunks = index.chunks(FIXTURE_SESSION).expect("chunks");
    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[1].host, "hesperia");

    let cards = index.cards(FIXTURE_SESSION).expect("cards");
    assert_eq!(cards.len(), 1, "only the file tagged task");
    assert_eq!(cards[0].rel, "card-release-notes.md");
    assert_eq!(cards[0].run.as_deref(), Some("blocked"));
    assert_eq!(cards[0].assignee.as_deref(), Some("amelia"));
    assert_eq!(cards[0].host.as_deref(), Some("electra"));
    assert_eq!(cards[0].requested_by.as_deref(), Some("@tgorka:h"));

    // `apply` after an append changes exactly that session's row.
    fs::rename(scratch.0.join("log-away"), session_dir.join("log")).expect("restore");
    let other_agent = keeper_core::agents::session::parse_session_agent_toml(
        &fs::read_to_string(session_dir.join("agent.toml"))
            .expect("toml")
            .replace("amelia", "winston"),
    )
    .expect("parse");
    index
        .add_session("active/2026-09-30-other", &other_agent)
        .expect("add");
    let other_before = index.session("active/2026-09-30-other").expect("q");
    let mut writer =
        ChunkWriter::open(&session_dir, &host("hesperia"), ROTATE, day()).expect("writer");
    let mut appended = line(
        "hesperia",
        2,
        Utc.with_ymd_and_hms(2026, 9, 30, 9, 5, 0)
            .single()
            .expect("ts"),
        12,
        LineBody::Run(RunBody {
            state: RunState::Review,
            detail: None,
        }),
    );
    appended.claim = Some("$c2".into());
    let receipt = writer.append(&appended).expect("append");
    index.apply(FIXTURE_SESSION, &receipt).expect("apply");
    let after = index.session(FIXTURE_SESSION).expect("q").expect("row");
    assert_eq!(after.lines, 12);
    assert_eq!(after.run.as_deref(), Some("review"));
    assert_eq!(after.last_ts.as_deref(), Some("2026-09-30T09:05:00.000Z"));
    assert_eq!(
        index.chunks(FIXTURE_SESSION).expect("chunks")[1].last_offset,
        receipt.offset
    );
    assert_eq!(
        index.session("active/2026-09-30-other").expect("q"),
        other_before,
        "the other session is untouched"
    );
    // A rebuild from the files agrees with what apply projected.
    index.rebuild().expect("rebuild");
    let rebuilt = index.session(FIXTURE_SESSION).expect("q").expect("row");
    assert_eq!(rebuilt, after);

    // A schema of another version rebuilds rather than erroring.
    drop(index);
    let conn = rusqlite::Connection::open(zone.join(".keeper/agents.db")).expect("db");
    conn.pragma_update(None, "user_version", 99)
        .expect("pragma");
    drop(conn);
    let mut index = Index::open(&zone).expect("reopen");
    assert!(index.needs_rebuild());
    assert!(index.sessions().expect("sessions").is_empty());
    index.rebuild().expect("rebuild");
    assert_eq!(index.session(FIXTURE_SESSION).expect("q"), Some(rebuilt));
}
