use std::collections::BTreeMap;

use super::*;
use crate::sessions::scan::scan_zone;

/// Rows 4 and 7 of Story 51.7: the detail's own pool, under the folder
/// contract.
///
/// A `task`-tagged file at a folder-shaped session's root is a board card —
/// this reader is what the board is drawn from, and it used to hand back
/// `Pool::default()` for this shape, so the owner's board was hidden with the
/// reason "a folder-shaped one has no pool to tag". Story 51.1 made that
/// false.
///
/// And the record is not accused: `README.md` declares no kind, so a pool
/// that took it would name the session's own identity file as *unfiled*.
#[test]
fn the_detail_pool_finds_a_folder_sessions_tasks_and_leaves_its_record_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = dir.path();
    for rel in ["refs", "prompts"] {
        std::fs::create_dir_all(session.join(rel)).expect("mkdir");
    }
    let write = |rel: &str, body: &str| {
        std::fs::write(session.join(rel), body).expect("write");
    };
    write(
        "README.md",
        "# The session\n\n## Log\n\n### 2026-08-16 first\n",
    );
    write(
        "ship-it.md",
        "---\ntags: [task]\nstatus: todo\norder: 1.5\n---\n# Ship it\n",
    );
    write("notes.md", "# Something nobody filed\n");
    write("refs/inputs.md", "---\ntags: [ref]\n---\n# Filed inputs\n");

    let (sources, _truncated, shape) = read_ref_sources(session, DETAIL_SCAN_BUDGET);
    assert_eq!(shape, Shape::Folder);
    let pool = detail_pool(&sources, shape);

    assert_eq!(
        pool.tasks
            .iter()
            .map(|entry| entry.rel.as_str())
            .collect::<Vec<_>>(),
        vec!["ship-it.md"],
        "the board's cards, on a shape whose board was hidden because it \
         was said to have no pool to tag"
    );
    assert_eq!(
        pool.unfiled
            .iter()
            .map(|entry| entry.rel.as_str())
            .collect::<Vec<_>>(),
        vec!["notes.md"],
        "root markdown declaring no kind is reported in this shape too, and \
         the record is not in the list: it is the file keeper reads the \
         session out of, not a file nobody filed"
    );
    assert!(
        pool.about.is_empty(),
        "and it is not carried in as an ordinary entry either"
    );
}

/// The other half of the same reader: a flat session's pool is what it was.
/// Its record carries `tags: [about]`, so it needs no exclusion — and adding
/// one would have taken the record out of the About space. Story 52.1: the
/// file this asserts about is `README.md` now, and the exclusion the folder
/// contract applies to that very name is still not applied here, because
/// under the flat contract it is a tagged pool member.
#[test]
fn the_detail_pool_leaves_a_flat_sessions_record_in_the_pool_where_it_was() {
    let dir = tempfile::tempdir().expect("tempdir");
    let session = dir.path();
    let write = |rel: &str, body: &str| {
        std::fs::write(session.join(rel), body).expect("write");
    };
    write("README.md", "---\ntags: [about]\n---\n# The session\n");
    write("AGENTS.md", "how to read this folder\n");
    write(
        "ship-it.md",
        "---\ntags: [task]\nstatus: todo\n---\n# Ship it\n",
    );

    let (sources, _truncated, shape) = read_ref_sources(session, DETAIL_SCAN_BUDGET);
    assert_eq!(shape, Shape::Flat);
    let pool = detail_pool(&sources, shape);

    assert_eq!(
        pool.about
            .iter()
            .map(|entry| entry.rel.as_str())
            .collect::<Vec<_>>(),
        vec!["README.md"]
    );
    assert_eq!(pool.tasks.len(), 1, "the board is unchanged");
    assert_eq!(
        pool.unfiled
            .iter()
            .map(|entry| entry.rel.as_str())
            .collect::<Vec<_>>(),
        vec!["AGENTS.md"],
        "and so is the unfiled list, `AGENTS.md` included"
    );
}

/// An agent session's `agent.toml`, as a host writes it.
const AGENT_TOML: &str = r#"version = 1
id = "01J9Z3K4M5N6P7Q8R9S0T1V2W3"
agent = "amelia"
drive = "tgdrive"
kind = "delegated"
title = "Release notes for 0.9"
requested_by = "@tola-grey:h"
room = "!sess:h"
drives = ["tgdrive"]
needs = ["git"]
pin = ""
hop = 1
dispatch_chain = ["@tgorka:h", "@nixi:h", "@tola-grey:h"]
created_at = "2026-09-30T08:15:03.120Z"

[parent]
drive = "tgdrive"
session = "active/2026-09-30-triage"
room = "!parent:h"

[label]
readers = ["@tgorka:h"]
integrity = "peer"

[limits]
rounds_per_exchange = 8
tokens = 200000
"#;

/// One card as the board shows it: its column and its agent keys, each as
/// `value` or `value?` when the key is there and does not read.
type Card = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn key(vm: Option<&keeper_core::agents::card::CardKeyVm>) -> Option<String> {
    vm.map(|key| {
        if key.readable {
            key.value.clone()
        } else {
            format!("{}?", key.value)
        }
    })
}

/// 98.2 #3 — the same board, from the same code: the Mac's `sessions_detail`
/// and the phone's are this one function over the zone, so a fixture zone's
/// board is pinned here, on Linux. Four columns, cards carrying `run`,
/// `assignee`, `host` and `requested_by`, a stray whose status does not read
/// (on the board, in no column) and keys that do not read shown as such;
/// where the session runs, from its log's claim; the session's room, which
/// the phone's card opens to decide an approval.
#[test]
fn board_projection_fixture() {
    let zone = tempfile::tempdir().expect("tempdir");
    let session = zone.path().join("active/2026-09-30-release-notes");
    std::fs::create_dir_all(&session).expect("mkdir");
    let write = |rel: &str, body: &str| {
        std::fs::write(session.join(rel), body).expect("write");
    };
    write(
        "README.md",
        "---\ntags: [about]\n---\n# Release notes\n\n## Summary\n\nNotes for 0.9.\n",
    );
    write("agent.toml", AGENT_TOML);
    write(
        "draft.md",
        "---\ntags: [task]\nstatus: in-preparation\norder: 1\nassignee: amelia\n\
         requested_by: \"@tgorka:h\"\n---\n# Draft the notes\n",
    );
    write(
        "collect.md",
        "---\ntags: [task]\nstatus: todo\norder: 1\nrun: running\nassignee: amelia\n\
         host: electra\n---\n# Collect the merged PRs\n",
    );
    write(
        "review.md",
        "---\ntags: [task]\nstatus: done\norder: 2\nrun: review\nassignee: amelia\n---\n\
         # Review the draft\n",
    );
    write(
        "publish.md",
        "---\ntags: [task]\nstatus: deferred\nhost: Not A Host\nrequested_by: nobody\n---\n\
         # Publish\n",
    );
    write(
        "stray.md",
        "---\ntags: [task]\nstatus: someday\nrun: sprinting\n---\n# A stray\n",
    );

    // electra holds the session: one claim line in its log chunk.
    claimed_by_electra(&session);

    let row = scan_zone(zone.path())
        .into_iter()
        .find(|row| row.path == "active/2026-09-30-release-notes")
        .expect("the scan lists the session");
    let board = detail(zone.path(), row);

    assert_eq!(board.room.as_deref(), Some("!sess:h"), "the session's room");
    assert_eq!(board.title, "Release notes");
    assert_eq!(board.summary, "Notes for 0.9.");
    let cards: BTreeMap<String, Card> = board
        .tasks
        .iter()
        .map(|task| {
            let agent = task.agent.as_ref();
            (
                task.rel_path.clone(),
                (
                    task.status.clone(),
                    key(agent.and_then(|a| a.run.as_ref())),
                    key(agent.and_then(|a| a.assignee.as_ref())),
                    key(agent.and_then(|a| a.host.as_ref())),
                    key(agent.and_then(|a| a.requested_by.as_ref())),
                ),
            )
        })
        .collect();
    let some = |text: &str| Some(text.to_owned());
    let expected: BTreeMap<String, Card> = [
        (
            "draft.md",
            (
                some("in-preparation"),
                None,
                some("amelia"),
                None,
                some("@tgorka:h"),
            ),
        ),
        (
            "collect.md",
            (
                some("todo"),
                some("running"),
                some("amelia"),
                some("electra"),
                None,
            ),
        ),
        (
            "review.md",
            (some("done"), some("review"), some("amelia"), None, None),
        ),
        (
            "publish.md",
            (
                some("deferred"),
                None,
                None,
                some("Not A Host?"),
                some("nobody?"),
            ),
        ),
        ("stray.md", (None, some("sprinting?"), None, None, None)),
    ]
    .into_iter()
    .map(|(rel, card)| (rel.to_owned(), card))
    .collect();
    assert_eq!(cards, expected);
    assert!(
        board
            .tasks
            .iter()
            .filter_map(|task| task.agent.as_ref())
            .all(|agent| agent.running_on.as_deref() == Some("electra")),
        "every agent card says where its session runs, from the log's claim"
    );
}

/// One `claim acquired` line by `electra`, as a host's writer leaves it.
fn claimed_by_electra(session: &std::path::Path) {
    use chrono::{TimeZone, Utc};
    use keeper_core::agents::log::{
        ClaimAction, ClaimBody, HostSlug, LineBody, LogLine, LINE_VERSION, LOG_DIR,
    };

    let ts = Utc
        .with_ymd_and_hms(2026, 9, 30, 9, 0, 0)
        .single()
        .expect("ts");
    let line = LogLine {
        v: LINE_VERSION,
        id: ulid::Ulid::from_parts(ts.timestamp_millis() as u64, 1),
        parent: None,
        ts,
        host: HostSlug::new("electra").expect("host"),
        epoch: 1,
        claim: None,
        matrix_event: None,
        body: LineBody::Claim(ClaimBody {
            epoch: 1,
            action: ClaimAction::Acquired,
            from_host: None,
            claim_event: "$claim".to_owned(),
            server_ts: "2026-09-30T09:00:00Z".to_owned(),
        }),
    };
    let dir = session.join(LOG_DIR);
    std::fs::create_dir_all(&dir).expect("log");
    std::fs::write(
        dir.join("2026-09-30.electra.1.jsonl"),
        format!("{}\n", line.to_json().expect("json")),
    )
    .expect("chunk");
}

/// A person's session has no `agent.toml`, so no room to open.
#[test]
fn a_persons_session_names_no_room() {
    let zone = tempfile::tempdir().expect("tempdir");
    let session = zone.path().join("active/2026-09-30-mine");
    std::fs::create_dir_all(&session).expect("mkdir");
    std::fs::write(session.join("README.md"), "# Mine\n").expect("write");
    let row = scan_zone(zone.path())
        .into_iter()
        .next()
        .expect("the scan lists the session");
    assert_eq!(detail(zone.path(), row).room, None);
}
