//! Story 61.10 — a grant you can see and revoke.
//!
//! Two halves, and they are tested differently on purpose.
//!
//! The **algebra** ([`keeper_core::bots::grant::decide`]) is pure, so every
//! boring case gets its own named test: the epic's own warning is that "every
//! previous epic that failed review failed because the assertion sat on a pure
//! function while the risk sat in the impure shell", and the answer to that is
//! not fewer pure tests — it is pure tests that are exhaustive plus impure
//! tests that are real.
//!
//! The **risk** is a revocation that must stop a call already in flight, and a
//! crash between the audit row and the effect. Those are tested against a real
//! `keeper.db` on disk, with a real file created and a real connection dropped
//! between the two reads — never against an in-memory stand-in, because the
//! property under test *is* that the row survived the connection that wrote it.

use std::path::{Path, PathBuf};

use keeper_core::bots::audit::{
    append_intent, complete, list_audit, AuditIntent, AuditOutcome, AuditVerdict,
};
use keeper_core::bots::grant::{
    check, decide, parse_subpath, Effect, Grant, GrantMode, GrantScope, GrantVerdict, SubpathError,
    ToolTarget, ASK_WRITE_WIDE_SCOPE, DENY_MODE_NONE, DENY_NO_GRANT, DENY_READ_ONLY, DENY_REVOKED,
    DENY_SUBPATH_UNSAFE,
};
use keeper_core::bots::store::{
    delete_bot, delete_provider, get_grant, insert_bot, insert_provider, list_grants,
    list_grants_for_bot, revoke_grant, save_grant,
};
use keeper_core::bots::{Bot, BotIdentity, Provider, ProviderKind};

/// A scratch directory no other test can land in — `bots/store.rs`'s helper and
/// its reason: the pid, a nanosecond stamp and a process-wide counter, because
/// two threads asking inside one clock tick otherwise open the same SQLite
/// file.
fn temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-grant-test-{}-{}-{n}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// A scratch directory with the two providers and two bots this file's grants
/// refer to already in it.
///
/// The foreign keys on `bot_grants` are **enforced** on this build —
/// `libsqlite3-sys`'s bundled SQLite is compiled with
/// `SQLITE_DEFAULT_FOREIGN_KEYS=1`, whatever the module comments inherited from
/// the account registry say — so a grant naming a provider that does not exist
/// is refused by the database. That is the right behaviour and this helper is
/// how a test earns the right to save one.
fn store_dir() -> PathBuf {
    let dir = temp_dir();
    for provider_id in ["prov-1", "prov-2"] {
        insert_provider(
            &dir,
            &Provider {
                id: provider_id.to_owned(),
                kind: ProviderKind::Ollama,
                name: format!("provider {provider_id}"),
                base_url: "http://localhost:11434".to_owned(),
                created_ms: NOW,
            },
        )
        .expect("the fixture provider must insert");
    }
    for (bot_id, target) in [("bot-1", "llama3.2:3b"), ("bot-2", "qwen3:8b")] {
        insert_bot(
            &dir,
            &Bot {
                id: bot_id.to_owned(),
                provider_id: "prov-1".to_owned(),
                target: target.to_owned(),
                name: format!("bot {bot_id}"),
                pin_order: 0,
                identity: BotIdentity::default(),
                created_ms: NOW,
            },
        )
        .expect("the fixture bot must insert");
    }
    dir
}

const NOW: i64 = 1_700_000_000_000;

fn grant(id: &str, scope: GrantScope, mode: GrantMode) -> Grant {
    Grant {
        id: id.to_owned(),
        provider_id: "prov-1".to_owned(),
        bot_id: None,
        scope,
        mode,
        created_ms: NOW,
    }
}

fn for_bot(mut grant: Grant, bot_id: &str) -> Grant {
    grant.bot_id = Some(bot_id.to_owned());
    grant
}

fn profile(profile_id: &str) -> GrantScope {
    GrantScope::Profile {
        profile_id: profile_id.to_owned(),
    }
}

fn subtree(profile_id: &str, subpath: &str) -> GrantScope {
    GrantScope::Subtree {
        profile_id: profile_id.to_owned(),
        subpath: subpath.to_owned(),
    }
}

fn target(profile_id: &str, subpath: &str) -> ToolTarget {
    ToolTarget::parse(profile_id, subpath).expect("the test's own path must pass the grammar")
}

fn allowed_by(verdict: &GrantVerdict) -> Option<&str> {
    match verdict {
        GrantVerdict::Allow { grant_id } => Some(grant_id),
        _ => None,
    }
}

fn denial(verdict: &GrantVerdict) -> Option<&str> {
    match verdict {
        GrantVerdict::Deny { reason } => Some(reason),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The scope algebra — one named test per boring case, because the boring cases
// are where a permission model is actually wrong.
// ---------------------------------------------------------------------------

#[test]
fn grant_no_grant_at_all_denies_and_says_how_to_fix_it() {
    let verdict = decide(&[], &target("work", "notes/a.md"), Effect::Read);
    assert_eq!(denial(&verdict), Some(DENY_NO_GRANT));
}

#[test]
fn grant_a_drive_scope_covers_every_profile_including_ones_it_never_named() {
    let grants = [grant("g-drive", GrantScope::Drive, GrantMode::Read)];
    for profile_id in ["work", "home", "a-profile-added-later"] {
        let verdict = decide(&grants, &target(profile_id, "notes/a.md"), Effect::Read);
        assert_eq!(
            allowed_by(&verdict),
            Some("g-drive"),
            "the whole drive is a statement about the drive, not about today's profile list"
        );
    }
}

#[test]
fn grant_a_profile_scope_covers_only_its_own_profile() {
    let grants = [grant("g-work", profile("work"), GrantMode::Read)];
    assert_eq!(
        allowed_by(&decide(
            &grants,
            &target("work", "notes/a.md"),
            Effect::Read
        )),
        Some("g-work")
    );
    assert_eq!(
        denial(&decide(
            &grants,
            &target("home", "notes/a.md"),
            Effect::Read
        )),
        Some(DENY_NO_GRANT),
        "a grant on one tenant says nothing about another"
    );
}

#[test]
fn grant_a_subtree_scope_covers_the_subtree_root_and_everything_below_it() {
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Read)];
    for subpath in ["notes", "notes/a.md", "notes/2026/q3/deep/a.md"] {
        assert_eq!(
            allowed_by(&decide(&grants, &target("work", subpath), Effect::Read)),
            Some("g-notes"),
            "{subpath} is at or below the granted subtree"
        );
    }
}

#[test]
fn grant_a_subtree_scope_does_not_cover_what_is_above_it() {
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Read)];
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "elsewhere/a.md"),
            Effect::Read
        )),
        Some(DENY_NO_GRANT)
    );
    assert_eq!(
        denial(&decide(
            &grants,
            &ToolTarget::profile_root("work"),
            Effect::Read
        )),
        Some(DENY_NO_GRANT),
        "a grant on a subtree is not a grant on the folder that contains it"
    );
}

/// The one that matters. A string prefix says `notes-old` is inside `notes`;
/// the segment comparison says it is not. This is the canonical production
/// defect in filesystem permissioning (R6 §5.2).
#[test]
fn grant_a_subtree_matches_by_segment_so_a_shared_prefix_is_not_inside_it() {
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Read)];
    for sibling in ["notes-old", "notes-old/a.md", "notesomething", "notes2"] {
        assert_eq!(
            denial(&decide(&grants, &target("work", sibling), Effect::Read)),
            Some(DENY_NO_GRANT),
            "{sibling} shares a prefix with notes and is a different folder"
        );
    }
}

#[test]
fn grant_a_deeper_subtree_also_matches_by_segment() {
    let grants = [grant(
        "g-deep",
        subtree("work", "notes/2026"),
        GrantMode::Read,
    )];
    assert_eq!(
        allowed_by(&decide(
            &grants,
            &target("work", "notes/2026/a.md"),
            Effect::Read
        )),
        Some("g-deep")
    );
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "notes/2026-draft/a.md"),
            Effect::Read
        )),
        Some(DENY_NO_GRANT)
    );
}

#[test]
fn grant_the_most_specific_grant_wins_over_a_wider_one() {
    // The drive says write; the profile says read only. The profile is the
    // narrower box, so it is the one that counts.
    let grants = [
        grant("g-drive", GrantScope::Drive, GrantMode::Write),
        grant("g-work", profile("work"), GrantMode::Read),
    ];
    assert_eq!(
        allowed_by(&decide(&grants, &target("work", "a.md"), Effect::Read)),
        Some("g-work"),
        "the narrower grant is the authority, so it is the one the audit log names"
    );
    assert_eq!(
        denial(&decide(&grants, &target("work", "a.md"), Effect::Write)),
        Some(DENY_READ_ONLY),
        "the wider write grant does not leak through the narrower read-only one"
    );
}

#[test]
fn grant_the_deepest_subtree_wins_over_a_shallower_one() {
    let grants = [
        grant("g-notes", subtree("work", "notes"), GrantMode::Write),
        grant("g-deep", subtree("work", "notes/2026"), GrantMode::Read),
    ];
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "notes/2026/a.md"),
            Effect::Write
        )),
        Some(DENY_READ_ONLY)
    );
    assert_eq!(
        allowed_by(&decide(
            &grants,
            &target("work", "notes/2025/a.md"),
            Effect::Write
        )),
        Some("g-notes"),
        "outside the narrower grant, the wider one is still in force"
    );
}

#[test]
fn grant_mode_none_is_an_explicit_refusal_that_beats_a_wider_read() {
    let grants = [
        grant("g-drive", GrantScope::Drive, GrantMode::Read),
        grant("g-secret", subtree("work", "secret"), GrantMode::None),
    ];
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "secret/keys.txt"),
            Effect::Read
        )),
        Some(DENY_MODE_NONE)
    );
    assert_eq!(
        allowed_by(&decide(&grants, &target("work", "open/a.md"), Effect::Read)),
        Some("g-drive"),
        "the refusal is about the folder it names and nothing else"
    );
}

#[test]
fn grant_mode_none_wins_a_tie_at_the_same_specificity() {
    // Deny comes first among equally specific grants — the order every
    // surveyed client evaluates in (R6 §2.1).
    let grants = [
        grant("g-yes", profile("work"), GrantMode::Write),
        grant("g-no", profile("work"), GrantMode::None),
    ];
    assert_eq!(
        denial(&decide(&grants, &target("work", "a.md"), Effect::Read)),
        Some(DENY_MODE_NONE)
    );
}

#[test]
fn grant_a_write_onto_a_read_only_scope_is_denied_with_the_read_only_reason() {
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Read)];
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "notes/a.md"),
            Effect::Write
        )),
        Some(DENY_READ_ONLY)
    );
    assert_eq!(
        allowed_by(&decide(
            &grants,
            &target("work", "notes/a.md"),
            Effect::Read
        )),
        Some("g-notes"),
        "the same grant still reads"
    );
}

/// FR-387, half one: a write under a scope the user approved *broadly* asks
/// every time. `Allow` for a write is never produced by breadth.
#[test]
fn grant_a_write_under_a_drive_wide_write_grant_asks_and_is_never_auto_approved() {
    let grants = [grant("g-drive", GrantScope::Drive, GrantMode::Write)];
    assert_eq!(
        decide(&grants, &target("work", "notes/a.md"), Effect::Write),
        GrantVerdict::Ask {
            grant_id: "g-drive".to_owned(),
            reason: ASK_WRITE_WIDE_SCOPE,
        }
    );
}

#[test]
fn grant_a_write_under_a_profile_wide_write_grant_also_asks() {
    let grants = [grant("g-work", profile("work"), GrantMode::Write)];
    assert_eq!(
        decide(&grants, &target("work", "notes/a.md"), Effect::Write),
        GrantVerdict::Ask {
            grant_id: "g-work".to_owned(),
            reason: ASK_WRITE_WIDE_SCOPE,
        }
    );
}

/// FR-387, half two: the only thing that auto-approves a write is a grant whose
/// own scope was approved for writing — the durable "always for this subtree".
#[test]
fn grant_a_write_is_allowed_only_inside_a_subtree_that_was_itself_approved_for_writing() {
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Write)];
    assert_eq!(
        allowed_by(&decide(
            &grants,
            &target("work", "notes/2026/a.md"),
            Effect::Write
        )),
        Some("g-notes")
    );
    assert_eq!(
        denial(&decide(
            &grants,
            &target("work", "elsewhere/a.md"),
            Effect::Write
        )),
        Some(DENY_NO_GRANT),
        "the approval is of that subtree, not of writing in general"
    );
}

#[test]
fn grant_a_wide_write_grant_still_reads_without_asking() {
    let grants = [grant("g-drive", GrantScope::Drive, GrantMode::Write)];
    assert_eq!(
        allowed_by(&decide(&grants, &target("work", "a.md"), Effect::Read)),
        Some("g-drive"),
        "asking before a read would be an approval prompt nobody learns anything from"
    );
}

#[test]
fn grant_a_grant_for_another_provider_never_speaks_for_this_one() {
    let mut other = grant("g-other", GrantScope::Drive, GrantMode::Write);
    other.provider_id = "prov-2".to_owned();
    assert!(!other.applies_to("prov-1", Some("bot-1")));
    assert!(other.applies_to("prov-2", Some("bot-1")));

    let dir = store_dir();
    save_grant(&dir, &other).expect("save should succeed");
    let verdict = check(
        &dir,
        "prov-1",
        Some("bot-1"),
        &target("work", "a.md"),
        Effect::Read,
    )
    .expect("check should succeed");
    assert_eq!(denial(&verdict), Some(DENY_NO_GRANT));
}

#[test]
fn grant_a_grant_for_another_bot_of_the_same_provider_never_speaks_for_this_bot() {
    let theirs = for_bot(
        grant("g-theirs", GrantScope::Drive, GrantMode::Write),
        "bot-2",
    );
    assert!(!theirs.applies_to("prov-1", Some("bot-1")));
    assert!(theirs.applies_to("prov-1", Some("bot-2")));
    assert!(
        !theirs.applies_to("prov-1", None),
        "a bot-scoped grant does not answer a question asked about no bot in particular"
    );

    let dir = store_dir();
    save_grant(&dir, &theirs).expect("save should succeed");
    assert_eq!(
        denial(
            &check(
                &dir,
                "prov-1",
                Some("bot-1"),
                &target("work", "a.md"),
                Effect::Read
            )
            .expect("check should succeed")
        ),
        Some(DENY_NO_GRANT)
    );
    assert_eq!(
        allowed_by(
            &check(
                &dir,
                "prov-1",
                Some("bot-2"),
                &target("work", "a.md"),
                Effect::Read
            )
            .expect("check should succeed")
        ),
        Some("g-theirs")
    );
}

#[test]
fn grant_a_bot_scoped_grant_beats_the_provider_wide_one_at_the_same_scope() {
    let grants = [
        grant("g-all-bots", profile("work"), GrantMode::Read),
        for_bot(
            grant("g-this-bot", profile("work"), GrantMode::None),
            "bot-1",
        ),
    ];
    assert_eq!(
        denial(&decide(&grants, &target("work", "a.md"), Effect::Read)),
        Some(DENY_MODE_NONE),
        "narrowing to one bot is the more specific statement"
    );

    let other_way = [
        grant("g-all-bots", profile("work"), GrantMode::None),
        for_bot(
            grant("g-this-bot", profile("work"), GrantMode::Read),
            "bot-1",
        ),
    ];
    assert_eq!(
        allowed_by(&decide(&other_way, &target("work", "a.md"), Effect::Read)),
        Some("g-this-bot"),
        "and it wins in both directions, not only the one that refuses"
    );
}

#[test]
fn grant_a_provider_wide_grant_speaks_for_every_bot() {
    let wide = grant("g-wide", GrantScope::Drive, GrantMode::Read);
    assert!(wide.applies_to("prov-1", Some("bot-1")));
    assert!(wide.applies_to("prov-1", Some("bot-9")));
    assert!(wide.applies_to("prov-1", None));
}

// ---------------------------------------------------------------------------
// The subpath grammar — refused before `decide` ever sees it.
// ---------------------------------------------------------------------------

#[test]
fn grant_the_grammar_refuses_an_empty_a_dot_and_a_dot_dot_subpath() {
    assert_eq!(parse_subpath(""), Err(SubpathError::Empty));
    assert_eq!(parse_subpath("/"), Err(SubpathError::Absolute));
    assert_eq!(parse_subpath("."), Err(SubpathError::Traversal));
    assert_eq!(parse_subpath(".."), Err(SubpathError::Traversal));
    assert_eq!(parse_subpath("notes/.."), Err(SubpathError::Traversal));
    assert_eq!(
        parse_subpath("notes/../../etc"),
        Err(SubpathError::Traversal)
    );
    assert_eq!(parse_subpath("notes/./a.md"), Err(SubpathError::Traversal));
    assert_eq!(parse_subpath("notes//a.md"), Err(SubpathError::Traversal));
    assert_eq!(parse_subpath("/etc/passwd"), Err(SubpathError::Absolute));
    assert_eq!(parse_subpath("notes\\a.md"), Err(SubpathError::Backslash));
    assert_eq!(parse_subpath("notes/a\0.md"), Err(SubpathError::Control));
    assert_eq!(
        parse_subpath(&"a/".repeat(600)),
        Err(SubpathError::TooLong),
        "a bound, because this is split on every tool call"
    );
    assert!(ToolTarget::parse("work", "notes/..").is_err());
}

#[test]
fn grant_the_grammar_normalizes_only_a_trailing_slash() {
    assert_eq!(parse_subpath("notes/"), Ok("notes".to_owned()));
    assert_eq!(parse_subpath("notes/2026"), Ok("notes/2026".to_owned()));
    assert_eq!(
        parse_subpath("Notes/A.md"),
        Ok("Notes/A.md".to_owned()),
        "case is not keeper's to change: it is the filesystem's answer, not this grammar's"
    );
}

#[test]
fn grant_an_unsafe_subpath_that_reached_decide_anyway_is_denied_and_never_matched() {
    // Belt and braces: the grammar is the door, and this is the check that a
    // target built around it is refused rather than matched against a subtree
    // it could be lying about.
    let grants = [grant("g-notes", subtree("work", "notes"), GrantMode::Write)];
    let smuggled = ToolTarget {
        profile_id: "work".to_owned(),
        subpath: "notes/../../etc/passwd".to_owned(),
    };
    assert_eq!(
        denial(&decide(&grants, &smuggled, Effect::Read)),
        Some(DENY_SUBPATH_UNSAFE)
    );
    assert_eq!(
        denial(&decide(&grants, &smuggled, Effect::Write)),
        Some(DENY_SUBPATH_UNSAFE)
    );
}

#[test]
fn grant_the_profile_root_is_a_real_target_and_only_the_two_wider_scopes_cover_it() {
    let root = ToolTarget::profile_root("work");
    assert_eq!(root.subpath, "");
    assert_eq!(root.display_path(), "work");
    assert_eq!(
        allowed_by(&decide(
            &[grant("g-work", profile("work"), GrantMode::Read)],
            &root,
            Effect::Read
        )),
        Some("g-work")
    );
    assert_eq!(
        allowed_by(&decide(
            &[grant("g-drive", GrantScope::Drive, GrantMode::Read)],
            &root,
            Effect::Read
        )),
        Some("g-drive")
    );
    assert_eq!(
        target("work", "notes/a.md").display_path(),
        "work/notes/a.md",
        "the log's path column is the path a person reads"
    );
}

// ---------------------------------------------------------------------------
// The store: a grant is a row, revocation is a state on that row.
// ---------------------------------------------------------------------------

#[test]
fn grant_a_grant_round_trips_and_saving_the_same_id_edits_it_in_place() {
    let dir = store_dir();
    let mut g = grant("g-1", subtree("work", "notes"), GrantMode::Read);
    save_grant(&dir, &g).expect("save should succeed");

    let row = get_grant(&dir, "g-1")
        .expect("read should succeed")
        .expect("row should exist");
    assert_eq!(row.grant, g);
    assert_eq!(row.revoked_ms, None);

    g.mode = GrantMode::Write;
    save_grant(&dir, &g).expect("an edit is a save of the same id");
    let listing = list_grants(&dir).expect("list should succeed");
    assert_eq!(listing.rows.len(), 1, "an edit is not a second permission");
    assert_eq!(listing.rows[0].grant.mode, GrantMode::Write);
    assert!(listing.unknown.is_empty());
}

#[test]
fn grant_a_store_refuses_a_subtree_that_never_passed_the_grammar() {
    let dir = store_dir();
    let bad = grant(
        "g-bad",
        subtree("work", "notes/../../etc"),
        GrantMode::Write,
    );
    assert!(
        save_grant(&dir, &bad).is_err(),
        "a scope nobody can match is a permission whose boundary nobody can state"
    );
    let unnormalized = grant("g-slash", subtree("work", "notes/"), GrantMode::Write);
    assert!(save_grant(&dir, &unnormalized).is_err());
}

#[test]
fn grant_revocation_keeps_the_row_so_the_audit_log_keeps_its_referent() {
    let dir = store_dir();
    save_grant(&dir, &grant("g-1", profile("work"), GrantMode::Read)).expect("save");

    assert!(revoke_grant(&dir, "g-1", NOW + 5).expect("revoke should succeed"));
    let row = get_grant(&dir, "g-1")
        .expect("read")
        .expect("the row survives");
    assert_eq!(row.revoked_ms, Some(NOW + 5));

    assert!(
        !revoke_grant(&dir, "g-1", NOW + 99).expect("revoke should succeed"),
        "a second revocation matches nothing and must not misdate the first"
    );
    assert_eq!(
        get_grant(&dir, "g-1")
            .expect("read")
            .expect("row")
            .revoked_ms,
        Some(NOW + 5)
    );
    assert!(
        !revoke_grant(&dir, "g-nope", NOW).expect("revoke should succeed"),
        "revoking into nothing reports it rather than claiming success"
    );
}

#[test]
fn grant_a_revoked_grant_is_still_listed_and_grants_nothing() {
    let dir = store_dir();
    save_grant(&dir, &grant("g-1", GrantScope::Drive, GrantMode::Write)).expect("save");
    revoke_grant(&dir, "g-1", NOW + 1).expect("revoke");

    let listing = list_grants(&dir).expect("list");
    assert_eq!(listing.rows.len(), 1, "the list is grants and their state");
    assert_eq!(listing.rows[0].revoked_ms, Some(NOW + 1));

    let live = list_grants_for_bot(&dir, "prov-1", Some("bot-1")).expect("read");
    assert!(live.live.is_empty());
    assert_eq!(live.revoked.len(), 1);
}

#[test]
fn grant_saving_a_revoked_grant_again_brings_it_back_to_life() {
    let dir = store_dir();
    let g = grant("g-1", profile("work"), GrantMode::Read);
    save_grant(&dir, &g).expect("save");
    revoke_grant(&dir, "g-1", NOW + 1).expect("revoke");
    save_grant(&dir, &g).expect("granting again is a save");

    assert_eq!(
        get_grant(&dir, "g-1")
            .expect("read")
            .expect("row")
            .revoked_ms,
        None,
        "a row present in the list and dead on every check is the affordance that lies"
    );
}

#[test]
fn grant_a_grant_cannot_name_a_provider_that_does_not_exist() {
    let dir = store_dir();
    let mut orphan = grant("g-orphan", GrantScope::Drive, GrantMode::Write);
    orphan.provider_id = "prov-nope".to_owned();
    assert!(
        save_grant(&dir, &orphan).is_err(),
        "a permission for an endpoint keeper does not have is a permission about nothing"
    );

    let mut ghost_bot = grant("g-ghost", GrantScope::Drive, GrantMode::Write);
    ghost_bot.bot_id = Some("bot-nope".to_owned());
    assert!(save_grant(&dir, &ghost_bot).is_err());
}

#[test]
fn grant_deleting_a_provider_takes_its_grants_with_it_and_is_not_blocked_by_them() {
    let dir = store_dir();
    save_grant(&dir, &grant("g-wide", GrantScope::Drive, GrantMode::Read)).expect("save");
    save_grant(
        &dir,
        &for_bot(grant("g-bot", profile("work"), GrantMode::Write), "bot-1"),
    )
    .expect("save");

    delete_bot(&dir, "bot-1").expect("a bot with a grant on it still deletes");
    let after_bot = list_grants(&dir).expect("list");
    assert_eq!(
        after_bot
            .rows
            .iter()
            .map(|r| r.grant.id.as_str())
            .collect::<Vec<_>>(),
        vec!["g-wide"],
        "a grant for a bot that no longer exists permits nothing and is not listed"
    );

    delete_provider(&dir, "prov-1").expect("a provider with a grant on it still deletes");
    assert!(
        list_grants(&dir).expect("list").rows.is_empty(),
        "a permission must not outlive the endpoint it was about"
    );
}

#[test]
fn grant_a_row_this_build_cannot_read_is_listed_and_grants_nothing() {
    use rusqlite::Connection;

    let dir = store_dir();
    // Create the schema through the sanctioned path first.
    save_grant(&dir, &grant("g-ok", profile("work"), GrantMode::Read)).expect("save");
    // Then write a row a newer build might have written.
    let conn = Connection::open(dir.join("keeper.db")).expect("open");
    conn.execute(
        "INSERT INTO bot_grants(id, provider_id, bot_id, scope_kind, profile_id, subtree, \
             mode, created_ms, revoked_ms) \
         VALUES('g-future', 'prov-1', NULL, 'device', NULL, NULL, 'append', 1, NULL)",
        [],
    )
    .expect("insert");
    drop(conn);

    let listing = list_grants(&dir).expect("list");
    assert_eq!(listing.rows.len(), 1);
    assert_eq!(
        listing.unknown.len(),
        1,
        "an ignored permission row is one the user believes they have"
    );
    assert_eq!(listing.unknown[0].id, "g-future");
    assert_eq!(listing.unknown[0].scope_kind, "device");
    assert_eq!(listing.unknown[0].mode, "append");

    let live = list_grants_for_bot(&dir, "prov-1", Some("bot-1")).expect("read");
    assert_eq!(live.live.len(), 1, "the unreadable row grants nothing");
    assert_eq!(live.live[0].id, "g-ok");
}

// ---------------------------------------------------------------------------
// The impure half: revocation mid-flight, and the audit row before the effect.
// ---------------------------------------------------------------------------

/// What a tool call does, in the order NFR-47 requires: check, record the
/// intent, run the effect, close the row. The shell's `ToolHost` is this shape
/// with a real filesystem body behind it.
fn run_tool(dir: &Path, root: &Path, subpath: &str, effect: Effect) -> GrantVerdict {
    let target = target("work", subpath);
    let verdict = check(dir, "prov-1", Some("bot-1"), &target, effect).expect("check");
    let audit_id = append_intent(
        dir,
        &AuditIntent {
            started_ms: NOW,
            provider_id: "prov-1",
            bot_id: Some("bot-1"),
            session_id: "sess-1",
            message_id: Some("msg-1"),
            tool: match effect {
                Effect::Read => "read",
                Effect::Write => "write",
            },
            target: &target,
            effect,
            verdict: &verdict,
        },
    )
    .expect("the intent row is written before anything happens");

    let (outcome, bytes) = match &verdict {
        GrantVerdict::Allow { .. } => {
            let path = root.join(subpath);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("fixture dir");
            }
            match effect {
                Effect::Write => {
                    std::fs::write(&path, b"body").expect("fixture write");
                    (AuditOutcome::Ok, Some(4))
                }
                Effect::Read => {
                    let body = std::fs::read(&path).unwrap_or_default();
                    (AuditOutcome::Ok, Some(body.len() as i64))
                }
            }
        }
        GrantVerdict::Ask { .. } | GrantVerdict::Deny { .. } => (AuditOutcome::Refused, None),
    };
    complete(dir, audit_id, outcome, bytes, false, NOW + 1).expect("complete");
    verdict
}

/// The story's real risk: a grant revoked between two calls of one sequence.
#[test]
fn grant_a_revocation_between_two_calls_denies_the_second_with_the_revocation_reason() {
    let dir = store_dir();
    let root = temp_dir();
    std::fs::create_dir_all(&root).expect("fixture root");
    save_grant(
        &dir,
        &for_bot(
            grant("g-notes", subtree("work", "notes"), GrantMode::Write),
            "bot-1",
        ),
    )
    .expect("save");

    let first = run_tool(&dir, &root, "notes/a.md", Effect::Write);
    assert_eq!(allowed_by(&first), Some("g-notes"));
    assert!(
        root.join("notes/a.md").exists(),
        "the first call really wrote"
    );

    // The user revokes while the conversation is mid-sequence.
    assert!(revoke_grant(&dir, "g-notes", NOW + 1).expect("revoke"));

    let second = run_tool(&dir, &root, "notes/b.md", Effect::Write);
    assert_eq!(
        denial(&second),
        Some(DENY_REVOKED),
        "the sentence names the revocation, because that is the fact the person needs"
    );
    assert!(
        !root.join("notes/b.md").exists(),
        "the second call touched nothing"
    );

    // And the log says both, newest first.
    let rows = list_audit(&dir, Some("sess-1"), None).expect("list");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].display_path, "work/notes/b.md");
    assert_eq!(rows[0].verdict, Some(AuditVerdict::Deny));
    assert_eq!(rows[0].outcome, AuditOutcome::Refused);
    assert_eq!(rows[0].reason.as_deref(), Some(DENY_REVOKED));
    assert_eq!(rows[0].grant_id, None);
    assert_eq!(rows[1].display_path, "work/notes/a.md");
    assert_eq!(rows[1].verdict, Some(AuditVerdict::Allow));
    assert_eq!(rows[1].grant_id.as_deref(), Some("g-notes"));
    assert_eq!(rows[1].bytes, Some(4));
}

#[test]
fn grant_the_check_re_reads_the_store_so_a_new_grant_takes_effect_on_the_next_call() {
    let dir = store_dir();
    let t = target("work", "notes/a.md");
    assert_eq!(
        denial(&check(&dir, "prov-1", Some("bot-1"), &t, Effect::Read).expect("check")),
        Some(DENY_NO_GRANT)
    );
    save_grant(
        &dir,
        &grant("g-notes", subtree("work", "notes"), GrantMode::Read),
    )
    .expect("save");
    assert_eq!(
        allowed_by(&check(&dir, "prov-1", Some("bot-1"), &t, Effect::Read).expect("check")),
        Some("g-notes"),
        "nothing between the store and the verdict caches a grant set"
    );
}

#[test]
fn grant_a_revocation_of_one_grant_leaves_a_wider_live_grant_in_force() {
    let dir = store_dir();
    save_grant(&dir, &grant("g-drive", GrantScope::Drive, GrantMode::Read)).expect("save");
    save_grant(
        &dir,
        &grant("g-notes", subtree("work", "notes"), GrantMode::None),
    )
    .expect("save");
    let t = target("work", "notes/a.md");
    assert_eq!(
        denial(&check(&dir, "prov-1", Some("bot-1"), &t, Effect::Read).expect("check")),
        Some(DENY_MODE_NONE)
    );

    revoke_grant(&dir, "g-notes", NOW + 1).expect("revoke");
    assert_eq!(
        allowed_by(&check(&dir, "prov-1", Some("bot-1"), &t, Effect::Read).expect("check")),
        Some("g-drive"),
        "revoking a refusal is not the same as revoking everything"
    );
}

/// NFR-47. The row is appended and committed **before** the effect, so a crash
/// between the two leaves an intent row: the connection that wrote it is
/// dropped by `append_intent` itself, and this reads it back through a fresh
/// one.
#[test]
fn grant_audit_a_crash_between_the_row_and_the_effect_leaves_a_readable_pending_row() {
    let dir = store_dir();
    let root = temp_dir();
    std::fs::create_dir_all(&root).expect("fixture root");
    save_grant(
        &dir,
        &grant("g-notes", subtree("work", "notes"), GrantMode::Write),
    )
    .expect("save");

    let t = target("work", "notes/doomed.md");
    let verdict = check(&dir, "prov-1", Some("bot-1"), &t, Effect::Write).expect("check");
    assert_eq!(allowed_by(&verdict), Some("g-notes"));
    let audit_id = append_intent(
        &dir,
        &AuditIntent {
            started_ms: NOW,
            provider_id: "prov-1",
            bot_id: Some("bot-1"),
            session_id: "sess-1",
            message_id: None,
            tool: "write",
            target: &t,
            effect: Effect::Write,
            verdict: &verdict,
        },
    )
    .expect("append");
    // …and here the process dies. `complete` is never called, and the effect
    // never happens.
    assert!(!root.join("notes/doomed.md").exists());

    // A fresh connection, as a relaunched app would open.
    let rows = list_audit(&dir, Some("sess-1"), None).expect("list");
    assert_eq!(
        rows.len(),
        1,
        "the row outlived the connection that wrote it"
    );
    assert_eq!(rows[0].id, audit_id);
    assert_eq!(
        rows[0].outcome,
        AuditOutcome::Pending,
        "a pending row is the evidence: a write was about to happen and may not have"
    );
    assert_eq!(rows[0].finished_ms, None);
    assert_eq!(rows[0].display_path, "work/notes/doomed.md");
    assert_eq!(rows[0].effect, Some(Effect::Write));
    assert_eq!(rows[0].verdict, Some(AuditVerdict::Allow));
    assert_eq!(rows[0].grant_id.as_deref(), Some("g-notes"));
    assert_eq!(
        rows[0].bytes, None,
        "nothing was counted, so nothing is claimed"
    );
}

#[test]
fn grant_audit_completing_a_row_records_the_outcome_and_the_bytes() {
    let dir = store_dir();
    let t = target("work", "notes/a.md");
    let verdict = GrantVerdict::Allow {
        grant_id: "g-notes".to_owned(),
    };
    let audit_id = append_intent(
        &dir,
        &AuditIntent {
            started_ms: NOW,
            provider_id: "prov-1",
            bot_id: None,
            session_id: "sess-1",
            message_id: None,
            tool: "read",
            target: &t,
            effect: Effect::Read,
            verdict: &verdict,
        },
    )
    .expect("append");
    assert!(complete(
        &dir,
        audit_id,
        AuditOutcome::Ok,
        Some(65_536),
        true,
        NOW + 7
    )
    .expect("done"));
    assert!(
        !complete(&dir, audit_id + 1000, AuditOutcome::Ok, None, false, NOW).expect("done"),
        "closing a row that is not there is reported, not swallowed"
    );

    let rows = list_audit(&dir, None, None).expect("list");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].outcome, AuditOutcome::Ok);
    assert_eq!(rows[0].bytes, Some(65_536));
    assert!(rows[0].truncated, "the bound is part of the record");
    assert_eq!(rows[0].finished_ms, Some(NOW + 7));
}

#[test]
fn grant_audit_a_refusal_is_recorded_with_the_sentence_the_user_was_shown() {
    let dir = store_dir();
    let root = temp_dir();
    std::fs::create_dir_all(&root).expect("fixture root");
    save_grant(
        &dir,
        &grant("g-notes", subtree("work", "notes"), GrantMode::Read),
    )
    .expect("save");

    let verdict = run_tool(&dir, &root, "notes/a.md", Effect::Write);
    assert_eq!(denial(&verdict), Some(DENY_READ_ONLY));

    let rows = list_audit(&dir, Some("sess-1"), None).expect("list");
    assert_eq!(rows.len(), 1, "a refused call is still an audited call");
    assert_eq!(rows[0].reason.as_deref(), Some(DENY_READ_ONLY));
    assert_eq!(rows[0].outcome, AuditOutcome::Refused);
    assert_eq!(rows[0].effect, Some(Effect::Write));
    assert_eq!(rows[0].grant_id, None);
}

#[test]
fn grant_audit_an_ask_is_recorded_as_an_ask_under_the_grant_that_would_permit_it() {
    let dir = store_dir();
    let root = temp_dir();
    std::fs::create_dir_all(&root).expect("fixture root");
    save_grant(&dir, &grant("g-drive", GrantScope::Drive, GrantMode::Write)).expect("save");

    let verdict = run_tool(&dir, &root, "notes/a.md", Effect::Write);
    assert!(matches!(verdict, GrantVerdict::Ask { .. }));
    assert!(
        !root.join("notes/a.md").exists(),
        "an Ask is not a write; the shell has not asked anyone yet"
    );

    let rows = list_audit(&dir, Some("sess-1"), None).expect("list");
    assert_eq!(rows[0].verdict, Some(AuditVerdict::Ask));
    assert_eq!(rows[0].grant_id.as_deref(), Some("g-drive"));
    assert_eq!(rows[0].reason.as_deref(), Some(ASK_WRITE_WIDE_SCOPE));
}

#[test]
fn grant_audit_the_log_reads_newest_first_and_filters_by_conversation() {
    let dir = store_dir();
    let t = target("work", "a.md");
    for (session, tool) in [("sess-1", "read"), ("sess-2", "glob"), ("sess-1", "stat")] {
        let verdict = GrantVerdict::Deny {
            reason: DENY_NO_GRANT.to_owned(),
        };
        append_intent(
            &dir,
            &AuditIntent {
                started_ms: NOW,
                provider_id: "prov-1",
                bot_id: None,
                session_id: session,
                message_id: None,
                tool,
                target: &t,
                effect: Effect::Read,
                verdict: &verdict,
            },
        )
        .expect("append");
    }

    let one = list_audit(&dir, Some("sess-1"), None).expect("list");
    assert_eq!(
        one.iter().map(|r| r.tool.as_str()).collect::<Vec<_>>(),
        vec!["stat", "read"],
        "newest first, by rowid, so rows sharing a millisecond still order"
    );
    let all = list_audit(&dir, None, None).expect("list");
    assert_eq!(all.len(), 3);
    assert_eq!(
        list_audit(&dir, Some("sess-1"), Some(1))
            .expect("list")
            .len(),
        1,
        "the limit is honoured"
    );
}
