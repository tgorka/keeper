//! What a session costs the drive (AD-366's measurement, story 89.5).
//!
//! A 10 000-line session is written through `ChunkWriter` into a scratch git
//! repository with one commit per 20-line turn, using the system `git` at its
//! default settings, and the repository's size is read before and after
//! `git gc` from `git count-objects -vH`. It prints the numbers
//! `docs/agents.md` § What a session costs the drive records; it asserts only
//! the bounds the writer promises, because the sizes are git's.

use std::fs;
use std::path::Path;
use std::process::Command;

use chrono::{NaiveDate, TimeZone, Utc};
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
use keeper_core::agents::log::{
    AssistantBody, HostSlug, LineBody, LogLine, ToolCallBody, ToolOutcomeWord, ToolResultBody,
    Usage, UserBody, LINE_VERSION, MAX_LINE_BYTES,
};
use matrix_sdk::ruma::UserId;
use ulid::Ulid;

const LINES: u64 = 10_000;
const TURN: u64 = 20;
/// keeper-sync's default `lfs_threshold_bytes`.
const LFS_THRESHOLD: u64 = 4 * 1024 * 1024;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("system git");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn size_pack(repo: &Path) -> String {
    git(repo, &["count-objects", "-vH"])
        .lines()
        .filter(|line| {
            line.starts_with("size") || line.starts_with("count") || line.starts_with("in-pack")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Realistic lines, deterministic: a turn is a person's message, model steps
/// with tool calls and results of varied size, and an answer.
fn body(i: u64) -> LineBody {
    let person = UserId::parse("@tgorka:h").expect("user");
    let label = Label {
        readers: Readers::Only([person.clone()].into_iter().collect()),
        integrity: Integrity::Owner,
        local_only: false,
    };
    let prose = |len: usize| -> String {
        "The writer rotates before the chunk reaches its bound, so nothing becomes LFS. "
            .chars()
            .cycle()
            .take(len)
            .collect()
    };
    match i % TURN {
        0 => LineBody::User(UserBody {
            sender: person,
            text: prose(120 + (i * 7 % 300) as usize),
            attachments: Vec::new(),
        }),
        19 => LineBody::Assistant(AssistantBody {
            text: prose(600 + (i * 13 % 2400) as usize),
            model: "llama3.1".into(),
            finish: "stop".into(),
            usage: Usage {
                prompt: Some(4000),
                completion: Some(400),
            },
            ttft_ms: Some(400),
            duration_ms: 5200,
            anchor_event: None,
        }),
        n if n % 2 == 1 => LineBody::ToolCall(ToolCallBody {
            call_id: format!("call_{i}"),
            tool: "drive_read".into(),
            args: format!("{{\"path\":\"notes/{i}.md\"}}"),
            tier: 0,
            grant_id: None,
        }),
        _ => LineBody::ToolResult(ToolResultBody {
            call_id: format!("call_{}", i - 1),
            outcome: ToolOutcomeWord::Ok,
            content: prose(200 + (i * 31 % 6000) as usize),
            truncated: None,
            label,
        }),
    }
}

#[test]
#[ignore = "measurement: writes a 10 000-line session into a scratch git repo; run with --ignored --nocapture"]
fn a_ten_thousand_line_session_costs_the_drive() {
    let repo = std::env::temp_dir().join(format!("keeper-agents-growth-{}", Ulid::new()));
    let session = repo.join("60-sessions/active/2026-09-30-growth");
    fs::create_dir_all(&session).expect("session");
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "growth@keeper.invalid"]);
    git(&repo, &["config", "user.name", "growth"]);
    fs::write(repo.join("README.md"), "scratch\n").expect("readme");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "start"]);
    let before = size_pack(&repo);

    let limit = rotate_at(LFS_THRESHOLD);
    let day = NaiveDate::from_ymd_opt(2026, 9, 30).expect("day");
    let host = HostSlug::new("electra").expect("host");
    let mut writer = ChunkWriter::open(&session, &host, limit, day).expect("writer");
    let start = Utc
        .with_ymd_and_hms(2026, 9, 30, 0, 0, 0)
        .single()
        .expect("ts");
    let mut largest_line = 0;
    for i in 0..LINES {
        let ts = start + chrono::Duration::milliseconds(i as i64 * 800);
        let line = LogLine {
            v: LINE_VERSION,
            id: Ulid::from_parts(ts.timestamp_millis() as u64, u128::from(i)),
            parent: None,
            ts,
            host: host.clone(),
            epoch: 1,
            claim: Some("$claim:h".into()),
            matrix_event: None,
            body: body(i),
        };
        let receipt = writer.append(&line).expect("append");
        largest_line = largest_line.max(receipt.bytes);
        if i % TURN == TURN - 1 {
            writer.sync().expect("sync");
            git(&repo, &["add", "-A"]);
            git(
                &repo,
                &["commit", "-q", "-m", &format!("turn {}", i / TURN)],
            );
            if (i + 1) % 2_500 == 0 {
                println!("after {} lines: {}", i + 1, size_pack(&repo));
            }
        }
    }

    let mut chunks: Vec<(String, u64)> = fs::read_dir(session.join("log"))
        .expect("log")
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.metadata().map_or(0, |m| m.len()),
            )
        })
        .collect();
    chunks.sort();
    let log_bytes: u64 = chunks.iter().map(|(_, b)| b).sum();
    let largest_chunk = chunks.iter().map(|(_, b)| *b).max().unwrap_or(0);
    let after = size_pack(&repo);
    git(&repo, &["gc", "-q"]);
    let after_gc = size_pack(&repo);

    println!("lines: {LINES}, commits: {}", LINES / TURN);
    println!("rotate_at: {limit} bytes");
    println!("chunks: {}, log bytes: {log_bytes}", chunks.len());
    println!("largest chunk: {largest_chunk} bytes, largest line: {largest_line} bytes");
    println!("repo before: {before}");
    println!("repo after, before gc: {after}");
    println!("repo after gc: {after_gc}");
    assert!(largest_chunk < limit, "{largest_chunk} reaches {limit}");
    assert!(
        largest_line <= MAX_LINE_BYTES as u64,
        "a line of {largest_line} bytes"
    );
    let _ = fs::remove_dir_all(&repo);
}
