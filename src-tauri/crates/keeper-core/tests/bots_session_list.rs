//! Story 61.6 — a list, an archive, and continue.
//!
//! Two halves again, and the split is the same one `bots_grant.rs` records.
//!
//! The **title minter** ([`mint_title`]) is pure, so each of its four rules
//! gets its own assertion in one named test: a title that carries a newline, an
//! emoji, a double space or a mid-scalar cut is a defect a list row shows and
//! nothing else catches.
//!
//! The **list** is not pure and is not tested as if it were. Every ordering,
//! search, archive and delete assertion below runs against a real `keeper.db`
//! on disk, written through one connection and read back through another —
//! because the properties under test are that the ordering survives a
//! reconnect, that archiving is a column flip a second read still sees, and
//! that a delete takes the messages with it rather than leaving rows no surface
//! can reach.

use std::path::{Path, PathBuf};

use keeper_core::bots::session::{
    append_message, delete_session, insert_session, list_messages, mint_title, search_sessions,
    set_session_archived, set_session_title, BotMessage, BotSession, SessionQuery, SessionScope,
    MAX_SESSION_PAGE, MAX_TITLE_CHARS, UNTITLED_SESSION,
};

/// A scratch directory no other test can land in — `bots/store.rs`'s helper and
/// its reason: the pid, a nanosecond stamp and a process-wide counter, because
/// two threads asking inside one clock tick otherwise open the same SQLite
/// file.
fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-bot-session-list-test-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

const NOW: i64 = 1_756_000_000_000;

fn session(id: &str, title: &str, updated_ms: i64) -> BotSession {
    BotSession {
        id: id.to_owned(),
        bot_id: "bot-1".to_owned(),
        provider_id: "prov-1".to_owned(),
        title: title.to_owned(),
        created_ms: NOW,
        updated_ms,
        archived: false,
        remote_session_id: None,
    }
}

fn message(id: &str, session_id: &str, content: &str, created_ms: i64) -> BotMessage {
    BotMessage {
        id: id.to_owned(),
        session_id: session_id.to_owned(),
        seq: 0,
        role: "user".to_owned(),
        content: content.to_owned(),
        model: None,
        provider_id: None,
        prompt_tokens: None,
        completion_tokens: None,
        total_tokens: None,
        ttft_ms: None,
        duration_ms: None,
        finish_reason: None,
        request_id: None,
        tool_call_count: 0,
        partial: false,
        created_ms,
    }
}

/// The ids a query returned, in the order it returned them.
fn ids(dir: &Path, query: &SessionQuery) -> Vec<String> {
    search_sessions(dir, query)
        .expect("the search must run")
        .rows
        .into_iter()
        .map(|row| row.session.id)
        .collect()
}

// ---------------------------------------------------------------------------
// The title minter (FR-381, DW-211)
// ---------------------------------------------------------------------------

/// Every rule the minter carries, asserted one by one.
///
/// It is one test rather than four because they are one function's contract and
/// a row that breaks any of them is the same defect: a name a list cannot draw.
#[test]
fn the_title_minter_takes_one_clean_line_from_the_first_message() {
    // The ordinary case: the person's own words, unchanged.
    assert_eq!(mint_title("What is in my drive?"), "What is in my drive?");

    // No newline reaches a title, and leading blank lines are not the title.
    assert_eq!(
        mint_title("\n\n  second line first  \nmore text below"),
        "second line first"
    );

    // No emoji: allowed as content, banned as chrome (DESIGN.md:358-361). The
    // gap a stripped emoji leaves closes rather than showing as a double space.
    assert_eq!(mint_title("ship it 🚀 today"), "ship it today");
    assert_eq!(mint_title("check\tthe   drive"), "check the drive");
    // A message that was ONLY emoji has no quotable words left.
    assert_eq!(mint_title("🚀🚀🚀"), UNTITLED_SESSION);
    // Including the joined-sequence shapes, which are several scalars each.
    assert_eq!(mint_title("👨‍👩‍👧‍👦 🇵🇱 ⭐️"), UNTITLED_SESSION);

    // Nothing quotable at all is named, never left blank.
    assert_eq!(mint_title(""), UNTITLED_SESSION);
    assert_eq!(mint_title("   \n\t "), UNTITLED_SESSION);

    // The clamp: the ellipsis is inside the budget, not added past it.
    let long = mint_title(&"a".repeat(200));
    assert_eq!(long.chars().count(), MAX_TITLE_CHARS);
    assert!(long.ends_with('…'));
    // And it never splits a scalar: 200 multi-byte characters clamp to the same
    // character count, which a byte-wise cut could not do.
    let wide = mint_title(&"ł".repeat(200));
    assert_eq!(wide.chars().count(), MAX_TITLE_CHARS);
    assert!(wide.ends_with('…'));
    // Exactly at the boundary nothing is dropped and no ellipsis appears.
    let exact = "b".repeat(MAX_TITLE_CHARS);
    assert_eq!(mint_title(&exact), exact);
}

// ---------------------------------------------------------------------------
// Ordering (FR-381)
// ---------------------------------------------------------------------------

/// Newest activity first, and the activity of a conversation with no messages
/// is its own `updated_ms` rather than nothing.
///
/// The middle row is the one that matters: `quiet` holds no message at all. A
/// list that read activity from `MAX(bot_messages.created_ms)` alone would sort
/// it below everything, which is the lie the fallback exists to prevent.
#[test]
fn the_session_list_orders_by_activity_with_and_without_messages() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-old", "oldest", NOW - 30_000)).expect("insert");
    insert_session(&dir, &session("s-quiet", "no messages", NOW - 10_000)).expect("insert");
    insert_session(&dir, &session("s-busy", "answered", NOW - 60_000)).expect("insert");
    // `s-busy` has the oldest `updated_ms` and the newest message, so activity
    // and `updated_ms` disagree — which is the case the ordering is about.
    append_message(&dir, &message("m-1", "s-busy", "the drive is fine", NOW)).expect("append");

    assert_eq!(
        ids(&dir, &SessionQuery::default()),
        vec!["s-busy", "s-quiet", "s-old"]
    );

    let rows = search_sessions(&dir, &SessionQuery::default())
        .expect("search")
        .rows;
    assert_eq!(rows[0].latest_activity_ms, NOW, "the newest message");
    assert_eq!(rows[0].message_count, 1);
    assert_eq!(
        rows[1].latest_activity_ms,
        NOW - 10_000,
        "a conversation with no messages still has an activity"
    );
    assert_eq!(rows[1].message_count, 0);
}

/// A rename moves a row to the front, and never behind a conversation older
/// than it.
///
/// `latest_activity` is the LATER of the two signals, so a rename of a
/// conversation whose last message is ancient is still the most recent thing
/// that happened to it.
#[test]
fn the_session_list_counts_a_rename_as_activity() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-a", "first", NOW - 50_000)).expect("insert");
    insert_session(&dir, &session("s-b", "second", NOW - 20_000)).expect("insert");
    append_message(&dir, &message("m-1", "s-a", "old question", NOW - 49_000)).expect("append");

    assert_eq!(ids(&dir, &SessionQuery::default()), vec!["s-b", "s-a"]);
    assert!(set_session_title(&dir, "s-a", "renamed", NOW).expect("rename"));
    assert_eq!(ids(&dir, &SessionQuery::default()), vec!["s-a", "s-b"]);
}

/// A tie on activity is broken by the id, so two reads of the same data return
/// the same order.
#[test]
fn the_session_list_breaks_a_tie_by_id() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-aaa", "one", NOW)).expect("insert");
    insert_session(&dir, &session("s-bbb", "two", NOW)).expect("insert");
    insert_session(&dir, &session("s-ccc", "three", NOW)).expect("insert");
    let first = ids(&dir, &SessionQuery::default());
    assert_eq!(first, vec!["s-ccc", "s-bbb", "s-aaa"]);
    assert_eq!(first, ids(&dir, &SessionQuery::default()), "stable");
}

/// A page is bounded and the total is not.
#[test]
fn the_session_list_pages_and_still_reports_the_real_total() {
    let dir = temp_dir();
    for n in 0..7 {
        insert_session(&dir, &session(&format!("s-{n}"), "a conversation", NOW + n))
            .expect("insert");
    }
    let page = search_sessions(
        &dir,
        &SessionQuery {
            limit: 3,
            ..Default::default()
        },
    )
    .expect("search");
    assert_eq!(page.rows.len(), 3, "the page is what was asked for");
    assert_eq!(page.total, 7, "the total is the set, not the page");

    // A caller that asks for more than the ceiling gets the ceiling, and a
    // caller that asks for nothing gets the default rather than nothing.
    let capped = search_sessions(
        &dir,
        &SessionQuery {
            limit: MAX_SESSION_PAGE + 500,
            ..Default::default()
        },
    )
    .expect("search");
    assert_eq!(capped.rows.len(), 7);
    let defaulted = search_sessions(&dir, &SessionQuery::default()).expect("search");
    assert_eq!(defaulted.rows.len(), 7);
}

// ---------------------------------------------------------------------------
// Search (FR-381)
// ---------------------------------------------------------------------------

/// Search reaches a message body whose words appear in no title.
///
/// The load-bearing assertion of the whole search: `s-body`'s title says
/// nothing about certificates, and the word is only in something the user
/// asked six turns ago.
#[test]
fn session_search_finds_a_word_that_is_only_in_a_body() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-body", "Tuesday", NOW - 1_000)).expect("insert");
    insert_session(
        &dir,
        &session("s-title", "certificate renewal", NOW - 2_000),
    )
    .expect("insert");
    append_message(
        &dir,
        &message(
            "m-1",
            "s-body",
            "when does the certificate expire?",
            NOW - 900,
        ),
    )
    .expect("append");

    // The body hit and the title hit both come back, newest activity first.
    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                text: "certificate".to_owned(),
                ..Default::default()
            }
        ),
        vec!["s-body", "s-title"]
    );
    // A word in the body alone still finds it.
    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                text: "expire".to_owned(),
                ..Default::default()
            }
        ),
        vec!["s-body"]
    );
    // The total is the matched set, not the table.
    assert_eq!(
        search_sessions(
            &dir,
            &SessionQuery {
                text: "expire".to_owned(),
                ..Default::default()
            }
        )
        .expect("search")
        .total,
        1
    );
    // Nothing matching is an empty page and a zero total, never an error.
    let miss = search_sessions(
        &dir,
        &SessionQuery {
            text: "kubernetes".to_owned(),
            ..Default::default()
        },
    )
    .expect("search");
    assert!(miss.rows.is_empty());
    assert_eq!(miss.total, 0);
}

/// Search is case-insensitive, and the needle's own `LIKE` metacharacters are
/// matched as the characters they are.
#[test]
fn session_search_is_case_insensitive_and_takes_wildcards_literally() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-a", "Certificate Renewal", NOW)).expect("insert");
    insert_session(&dir, &session("s-b", "100% of the drive", NOW - 1)).expect("insert");
    insert_session(&dir, &session("s-c", "nothing to see", NOW - 2)).expect("insert");

    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                text: "CERTIFICATE".to_owned(),
                ..Default::default()
            }
        ),
        vec!["s-a"]
    );
    // `%` is a character here, not "match everything".
    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                text: "100%".to_owned(),
                ..Default::default()
            }
        ),
        vec!["s-b"]
    );
    // And `_` is a character, not "match any one".
    assert!(ids(
        &dir,
        &SessionQuery {
            text: "1_0".to_owned(),
            ..Default::default()
        }
    )
    .is_empty());
}

/// An empty needle is no text predicate at all, not a scan that matches
/// everything by accident — the distinction matters because the two happen to
/// agree on the rows and disagree on the cost.
#[test]
fn session_search_with_no_text_returns_the_whole_scope() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-a", "one", NOW)).expect("insert");
    insert_session(&dir, &session("s-b", "two", NOW - 1)).expect("insert");
    assert_eq!(ids(&dir, &SessionQuery::default()), vec!["s-a", "s-b"]);
}

// ---------------------------------------------------------------------------
// Archive (FR-381)
// ---------------------------------------------------------------------------

/// Archiving hides a conversation from the live list, keeps it in the archive,
/// and is undone by the same setter — it is a column, never a delete.
#[test]
fn archiving_a_session_is_reversible_and_keeps_every_message() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-a", "live", NOW)).expect("insert");
    insert_session(&dir, &session("s-b", "filed", NOW - 1_000)).expect("insert");
    append_message(&dir, &message("m-1", "s-b", "the question", NOW - 900)).expect("append");

    assert!(set_session_archived(&dir, "s-b", true, NOW).expect("archive"));

    // Gone from the live list…
    assert_eq!(ids(&dir, &SessionQuery::default()), vec!["s-a"]);
    // …present in the archive alone…
    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                scope: SessionScope::Archived,
                ..Default::default()
            }
        ),
        vec!["s-b"]
    );
    // …and present in both, which is what a widening scope is for.
    assert_eq!(
        ids(
            &dir,
            &SessionQuery {
                scope: SessionScope::All,
                ..Default::default()
            }
        )
        .len(),
        2
    );
    // The messages never moved: archiving is not a delete.
    assert_eq!(list_messages(&dir, "s-b").expect("read").len(), 1);

    // And back again, with everything intact.
    assert!(set_session_archived(&dir, "s-b", false, NOW + 1).expect("unarchive"));
    assert_eq!(
        ids(&dir, &SessionQuery::default()),
        vec!["s-b", "s-a"],
        "unarchived, and at the front because unarchiving is activity"
    );
    assert_eq!(list_messages(&dir, "s-b").expect("read").len(), 1);
}

/// Archiving something that is not there reports it rather than claiming a
/// write — AD-27's rule, at the store.
#[test]
fn archiving_a_session_that_is_not_there_says_so() {
    let dir = temp_dir();
    assert!(!set_session_archived(&dir, "s-nope", true, NOW).expect("archive"));
    assert!(!set_session_title(&dir, "s-nope", "a name", NOW).expect("rename"));
}

// ---------------------------------------------------------------------------
// Delete (FR-381)
// ---------------------------------------------------------------------------

/// Deleting a conversation takes its messages with it, and takes nothing else.
///
/// Two conversations with messages, one deleted: the survivor's rows are all
/// still there. A `DELETE FROM bot_messages` with the wrong predicate — or none
/// — passes a test that only counts the deleted session's rows.
#[test]
fn deleting_a_session_deletes_its_messages_and_only_its_messages() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-gone", "to be deleted", NOW)).expect("insert");
    insert_session(&dir, &session("s-kept", "to be kept", NOW - 1_000)).expect("insert");
    for n in 0..3 {
        append_message(
            &dir,
            &message(&format!("m-gone-{n}"), "s-gone", "asked", NOW + n),
        )
        .expect("append");
    }
    append_message(&dir, &message("m-kept", "s-kept", "also asked", NOW)).expect("append");

    delete_session(&dir, "s-gone").expect("delete");

    assert_eq!(ids(&dir, &SessionQuery::default()), vec!["s-kept"]);
    assert!(
        list_messages(&dir, "s-gone").expect("read").is_empty(),
        "the messages went with the conversation"
    );
    assert_eq!(
        list_messages(&dir, "s-kept").expect("read").len(),
        1,
        "and nothing else did"
    );
    // The deleted rows are unreachable by search too, body and title alike.
    assert!(ids(
        &dir,
        &SessionQuery {
            text: "asked".to_owned(),
            scope: SessionScope::All,
            ..Default::default()
        }
    )
    .iter()
    .all(|id| id == "s-kept"));
}

/// Deleting an archived conversation works, and deleting nothing is not an
/// error — the rollback path of a half-finished create depends on it.
#[test]
fn deleting_a_session_is_idempotent_and_reaches_the_archive() {
    let dir = temp_dir();
    insert_session(&dir, &session("s-a", "filed", NOW)).expect("insert");
    assert!(set_session_archived(&dir, "s-a", true, NOW).expect("archive"));
    delete_session(&dir, "s-a").expect("delete");
    delete_session(&dir, "s-a").expect("deleting nothing is not an error");
    assert_eq!(
        search_sessions(
            &dir,
            &SessionQuery {
                scope: SessionScope::All,
                ..Default::default()
            }
        )
        .expect("search")
        .total,
        0
    );
}
