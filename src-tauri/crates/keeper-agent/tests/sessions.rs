//! The sessions runtime's guarantees (AD-368, FR-778, NFR-117's share), on
//! real zones on disk.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::time::{Duration, SystemTime};

use keeper_agent::sessions::exec::{self, ExecError};
use keeper_agent::sessions::scan;
use keeper_agent::sessions::verbs::{self, CreateOutcome, CreateReq};
use keeper_agent::sessions::write::session_write;
use keeper_core::sessions::model::SessionStatus;
use keeper_core::sessions::plan::{compile_create, Plan, PlanStep};

const JOURNAL: &str = ".keeper/sessions-journal.json";

fn zone() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    for sub in [
        "_template/workspace",
        "_template/artifacts",
        "active",
        "archive",
    ] {
        std::fs::create_dir_all(dir.path().join(sub)).expect("mkdir");
    }
    std::fs::write(dir.path().join("_template/README.md"), "# template\n").expect("write");
    dir
}

fn create_req(id: &str, title: &str) -> CreateReq {
    CreateReq {
        id: id.parse().expect("a ULID"),
        title: title.to_owned(),
        pattern_id: None,
        now: chrono::Local::now(),
    }
}

/// A plan of `steps` file writes into one session folder of its own.
fn writes(session: &str, steps: usize) -> Plan {
    Plan {
        verb: "test-writes".to_owned(),
        session: format!("active/{session}"),
        steps: (0..steps)
            .map(|n| PlanStep::WriteFile {
                path: format!("active/{session}/f{n:02}.md"),
                content: format!("{session} {n}\n"),
            })
            .collect(),
    }
}

/// One plan at a time per zone (90.2 acceptance 1). Two threads start a
/// 40-step plan each on one zone at once; a third reads the journal as fast as
/// it can. Every row it sees belongs to one plan, and once the second plan's
/// rows appear the first plan's never come back — no plan is resumed by the
/// other's run.
///
/// Either layer of the zone lock is enough for this on its own — `flock` locks
/// conflict between two descriptors of one process too — so it fails only
/// with both removed. The in-process gate alone is proved by
/// `lock::tests::the_gate_alone_holds_a_second_thread_off_until_the_first_lets_go`,
/// the file lock alone by `sessions_lock_process.rs`.
#[test]
fn two_plans_on_one_zone_never_interleave() {
    let zone = zone();
    let root = zone.path().to_path_buf();
    let start = Arc::new(Barrier::new(3));
    let done = Arc::new(AtomicBool::new(false));
    let runners: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|name| {
            let (root, start) = (root.clone(), Arc::clone(&start));
            std::thread::spawn(move || {
                start.wait();
                exec::run(&root, writes(name, 40))
            })
        })
        .collect();
    let watcher = {
        let (root, start, done) = (root.clone(), Arc::clone(&start), Arc::clone(&done));
        std::thread::spawn(move || {
            start.wait();
            let mut seen: Vec<(String, u64)> = Vec::new();
            while !done.load(Ordering::SeqCst) {
                let Ok(text) = std::fs::read_to_string(root.join(JOURNAL)) else {
                    continue;
                };
                let Ok(row) = serde_json::from_str::<serde_json::Value>(&text) else {
                    continue;
                };
                let session = row["plan"]["session"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                let at = row["done"].as_u64().unwrap_or_default();
                if seen.last() != Some(&(session.clone(), at)) {
                    seen.push((session, at));
                }
            }
            seen
        })
    };
    for runner in runners {
        runner.join().expect("runner").expect("the plan runs");
    }
    done.store(true, Ordering::SeqCst);
    let seen = watcher.join().expect("watcher");

    let mut finished: Vec<String> = Vec::new();
    let mut last: Option<(String, u64)> = None;
    for (session, at) in seen {
        assert!(
            !finished.contains(&session),
            "{session}'s journal came back after another plan had started"
        );
        if let Some((previous, previous_at)) = &last {
            if *previous == session {
                assert!(at >= *previous_at, "{session}'s progress went backwards");
            } else {
                finished.push(previous.clone());
            }
        }
        last = Some((session, at));
    }
    for name in ["a", "b"] {
        for n in 0..40 {
            assert_eq!(
                std::fs::read_to_string(root.join(format!("active/{name}/f{n:02}.md")))
                    .expect("both results are in the tree"),
                format!("{name} {n}\n")
            );
        }
    }
    assert!(!root.join(JOURNAL).exists(), "the journal is cleared");
}

/// A crash left a create journaled at step 2 of its steps; `resume_all`, what
/// a host calls at start, finishes it and clears the journal (90.2 acceptance
/// 3). The app's call of it from `sessions_root::refresh` is shell code, seen
/// by inspection.
#[test]
fn resume_all_finishes_an_interrupted_create_and_clears_its_journal() {
    let zone = zone();
    let plan = compile_create(
        "2026-08-12-research",
        "_template",
        &[
            ("README.md".to_owned(), false),
            ("workspace".to_owned(), true),
            ("artifacts".to_owned(), true),
        ],
        "---\nid: 01J5AAAAAAAAAAAAAAAAAAAAAA\n---\n# research\n",
    );
    assert!(
        plan.steps.len() > 2,
        "the plan has steps left after the crash"
    );
    std::fs::create_dir_all(zone.path().join(".keeper")).expect("mkdir");
    std::fs::write(
        zone.path().join(JOURNAL),
        serde_json::to_string(&serde_json::json!({ "plan": plan, "done": 2 })).expect("json"),
    )
    .expect("journal");

    let failed = keeper_agent::sessions::resume_all([zone.path()]);
    assert!(failed.is_empty(), "{failed:?}");
    let session = zone.path().join("active/2026-08-12-research");
    assert!(std::fs::read_to_string(session.join("README.md"))
        .expect("the record was written")
        .contains("# research"));
    assert!(session.join("workspace").is_dir());
    assert!(!zone.path().join(JOURNAL).exists(), "journal cleared");
}

/// A create retried with its id makes one session, and the retry's own title
/// changes nothing (90.2 acceptance 4).
#[test]
fn a_create_retried_with_the_same_id_makes_one_session() {
    let zone = zone();
    let id = "01J5BBBBBBBBBBBBBBBBBBBBBB";
    let first = verbs::create(zone.path(), create_req(id, "Research")).expect("create");
    let CreateOutcome::Created { path, title } = first else {
        panic!("the first create creates: {first:?}");
    };
    let again = verbs::create(zone.path(), create_req(id, "Research")).expect("retry");
    assert_eq!(
        again,
        CreateOutcome::Existed {
            path: path.clone(),
            title: title.clone()
        }
    );
    let renamed = verbs::create(zone.path(), create_req(id, "Something else")).expect("retry");
    assert_eq!(renamed, CreateOutcome::Existed { path, title });
    let folders = std::fs::read_dir(zone.path().join("active"))
        .expect("active")
        .count();
    assert_eq!(folders, 1, "one session folder");
}

/// The retried create FR-778 exists for, after a crash between the session's
/// folder and its record: the journal says one step is done, the folder is
/// there with no record in it. The retry finishes the first attempt before it
/// looks, so it finds the session by its id rather than making a second one
/// beside the half-made folder.
#[test]
fn a_create_retried_after_a_crash_mid_create_finishes_the_first_and_makes_no_second() {
    let zone = zone();
    let id = "01J5GGGGGGGGGGGGGGGGGGGGGG";
    let plan = compile_create(
        "2026-08-12-research",
        "_template",
        &[
            ("README.md".to_owned(), false),
            ("workspace".to_owned(), true),
            ("artifacts".to_owned(), true),
        ],
        &format!("---\nid: {id}\n---\n# research\n"),
    );
    assert!(
        matches!(&plan.steps[0], PlanStep::MkDir { path } if path == "active/2026-08-12-research"),
        "the create's first step makes the folder: {:?}",
        plan.steps[0]
    );
    std::fs::create_dir_all(zone.path().join("active/2026-08-12-research")).expect("step 1");
    std::fs::create_dir_all(zone.path().join(".keeper")).expect("mkdir");
    std::fs::write(
        zone.path().join(JOURNAL),
        serde_json::to_string(&serde_json::json!({ "plan": plan, "done": 1 })).expect("json"),
    )
    .expect("journal");

    let again = verbs::create(zone.path(), create_req(id, "research")).expect("retry");
    assert_eq!(
        again,
        CreateOutcome::Existed {
            path: "active/2026-08-12-research".to_owned(),
            title: "research".to_owned()
        }
    );
    let folders = std::fs::read_dir(zone.path().join("active"))
        .expect("active")
        .count();
    assert_eq!(folders, 1, "one session folder");
    assert!(!zone.path().join(JOURNAL).exists(), "journal cleared");
}

/// Two folders carrying one id — a sync-conflict copy — are acted on in the
/// board's order, so the pinned one, which the board lists first, is the one
/// archived, whichever the disk lists first.
#[test]
fn a_verb_on_an_id_two_folders_carry_acts_on_the_one_the_board_lists_first() {
    let zone = zone();
    let id = "01J5HHHHHHHHHHHHHHHHHHHHHH";
    let record = |name: &str, pinned: bool| {
        let dir = zone.path().join("active").join(name);
        std::fs::create_dir_all(&dir).expect("session");
        std::fs::write(
            dir.join("README.md"),
            format!("---\nid: {id}\npinned: {pinned}\n---\n# {name}\n"),
        )
        .expect("record");
    };
    for copy in 0..6 {
        record(&format!("copy-{copy}"), false);
    }
    record("pinned", true);
    verbs::archive(zone.path(), id, Vec::new(), false, 2026).expect("archive");
    assert!(zone.path().join("archive/2026/pinned").is_dir());
    assert!(!zone.path().join("active/pinned").exists());
    for copy in 0..6 {
        assert!(zone.path().join(format!("active/copy-{copy}")).is_dir());
    }
}

/// A verb right after a create finds the session by id on the disk, with no
/// scan in between (90.2 acceptance 5).
#[test]
fn a_verb_right_after_create_finds_the_session_by_id() {
    let zone = zone();
    let id = "01J5CCCCCCCCCCCCCCCCCCCCCC";
    let CreateOutcome::Created { path, .. } =
        verbs::create(zone.path(), create_req(id, "Keeper")).expect("create")
    else {
        panic!("created");
    };
    verbs::archive(zone.path(), id, Vec::new(), true, 2026).expect("archive finds it");
    let name = path.rsplit('/').next().expect("folder name");
    assert!(zone.path().join("archive/2026").join(name).is_dir());
    assert!(!zone.path().join(&path).exists());
    verbs::unarchive(zone.path(), id).expect("unarchive finds it in archive/");
    assert!(zone.path().join(&path).is_dir());
}

/// A symlinked folder inside a session cannot carry a write out of the zone
/// (90.2 acceptance 6). The lexical test passes the path; only the disk
/// knows where it goes.
#[cfg(unix)]
#[test]
fn a_symlinked_session_folder_cannot_redirect_a_step_outside_the_zone() {
    let zone = zone();
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let session = zone.path().join("active/s");
    std::fs::create_dir_all(&session).expect("session");
    std::os::unix::fs::symlink(elsewhere.path(), session.join("artifacts")).expect("symlink");
    for step in [
        PlanStep::WriteFile {
            path: "active/s/artifacts/x.md".to_owned(),
            content: "escaped".to_owned(),
        },
        PlanStep::MkDir {
            path: "active/s/artifacts/deeper".to_owned(),
        },
    ] {
        let refused = exec::run(
            zone.path(),
            Plan {
                verb: "test".to_owned(),
                session: "active/s".to_owned(),
                steps: vec![step],
            },
        );
        assert!(
            matches!(&refused, Err(ExecError::Refused(sentence)) if sentence.contains("leaves the zone")),
            "{refused:?}"
        );
    }
    assert_eq!(
        std::fs::read_dir(elsewhere.path())
            .expect("elsewhere")
            .count(),
        0,
        "nothing landed outside the zone"
    );
}

/// A step that moves or trashes a path acts on the link there, not through
/// it: a link out of the zone is trashed as a link and what it pointed at is
/// untouched, and a dangling link is trashed too.
#[cfg(unix)]
#[test]
fn a_link_is_trashed_as_a_link_and_its_target_is_left_alone() {
    let zone = zone();
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    std::fs::write(elsewhere.path().join("kept.md"), "kept\n").expect("outside file");
    let session = zone.path().join("active/s");
    std::fs::create_dir_all(&session).expect("session");
    std::os::unix::fs::symlink(elsewhere.path().join("kept.md"), session.join("linked.md"))
        .expect("symlink");
    std::os::unix::fs::symlink(elsewhere.path().join("gone"), session.join("dangling"))
        .expect("dangling symlink");
    exec::run(
        zone.path(),
        Plan {
            verb: "test".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![
                PlanStep::TrashFile {
                    path: "active/s/linked.md".to_owned(),
                    trash_key: "k1".to_owned(),
                },
                PlanStep::TrashDir {
                    path: "active/s/dangling".to_owned(),
                    trash_key: "k2".to_owned(),
                },
            ],
        },
    )
    .expect("both links are trashed");
    let trashed = zone.path().join(".keeper/trash/k1/linked.md");
    assert!(trashed
        .symlink_metadata()
        .expect("in the trash")
        .file_type()
        .is_symlink());
    assert!(zone
        .path()
        .join(".keeper/trash/k2")
        .symlink_metadata()
        .is_ok());
    assert!(!session.join("linked.md").exists());
    assert_eq!(
        std::fs::read_to_string(elsewhere.path().join("kept.md")).expect("outside file"),
        "kept\n"
    );
}

/// `session_write` takes artifacts and scratch, and never keeper's own files
/// (90.2 acceptance 7).
#[test]
fn session_write_refuses_log_approvals_and_keepers_files() {
    let zone = zone();
    std::fs::create_dir_all(zone.path().join("active/s")).expect("session");
    let write = |rel: &str| session_write(zone.path(), "active/s", rel, "text\n");
    write("artifacts/answer-01J5DDDDDDDDDDDDDDDDDDDDDD.md").expect("an artifact");
    write("workspace/run.jsonl").expect("scratch, any extension");
    assert_eq!(
        std::fs::read_to_string(zone.path().join("active/s/workspace/run.jsonl")).expect("read"),
        "text\n"
    );
    for refused in [
        "log/x.jsonl",
        "artifacts/x.jsonl",
        "approvals/a.json",
        "agent.toml",
        "README.md",
        "AGENTS.md",
        "notes.md",
        "artifacts/../README.md",
    ] {
        assert!(write(refused).is_err(), "{refused} was written");
    }
    assert!(!zone.path().join("active/s/log").exists());
    assert!(!zone.path().join("active/s/README.md").exists());
}

/// A write names a session that is there; it never makes one (a phantom the
/// board would then list under a `path:` id).
#[test]
fn session_write_into_a_session_that_is_not_there_makes_nothing() {
    let zone = zone();
    let refused = session_write(zone.path(), "active/missing", "workspace/x.md", "text\n");
    assert!(
        matches!(&refused, Err(verbs::VerbError::NoSuchSession(session)) if session == "active/missing"),
        "{refused:?}"
    );
    assert!(!zone.path().join("active/missing").exists());
}

/// Resuming a zone with nothing to finish only looks: no lock file is made in
/// it and nothing waits. A zone that is not there cannot be locked, and that
/// is the disk's answer, not a refusal to re-plan around.
#[test]
fn a_zone_with_no_journal_is_only_looked_at_and_a_missing_zone_is_not_a_refusal() {
    let zone = zone();
    let failed = keeper_agent::sessions::resume_all([zone.path()]);
    assert!(failed.is_empty(), "{failed:?}");
    assert!(!zone.path().join(".keeper/sessions.lock").exists());

    let missing = zone.path().join("not-there");
    let refused = exec::run(&missing, writes("s", 1));
    assert!(
        matches!(&refused, Err(ExecError::Failed { verb, .. }) if verb == "lock"),
        "{refused:?}"
    );
}

/// What an agent adds to a session — its log, blobs, approvals and
/// `agent.toml` — changes neither the board row nor the pool (90.2 acceptance
/// 8; 89.5 acceptance 10, the scan half).
#[test]
fn a_session_with_agent_files_scans_to_the_same_row_and_pool() {
    let zone = zone();
    let rel = "active/2026-09-30-release-notes";
    let session = zone.path().join(rel);
    std::fs::create_dir_all(session.join("artifacts")).expect("artifacts");
    std::fs::write(
        session.join("README.md"),
        "---\nid: 01J5EEEEEEEEEEEEEEEEEEEEEE\ntags: [about]\n---\n# Release notes\n\n## Summary\n\nShip it.\n",
    )
    .expect("record");
    std::fs::write(session.join("AGENTS.md"), "how to read this folder\n").expect("contract");
    std::fs::write(
        session.join("2026-09-30-0900-opened.md"),
        "---\ntags: [log]\n---\n# Opened\n\nStarted.\n",
    )
    .expect("log");
    let row = || scan::row_for(&session, rel, SessionStatus::Active).expect("row");
    let pool = || scan::read_session_pool(&session, rel.to_owned());
    let (row_before, pool_before) = (row(), pool());

    // Older than everything above, so the freshness signal is not what moves.
    let long_ago = SystemTime::now() - Duration::from_secs(86_400 * 365);
    let add = |path: &str, text: &str| {
        let file = session.join(path);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        std::fs::write(&file, text).expect("write");
        std::fs::File::options()
            .write(true)
            .open(&file)
            .and_then(|handle| handle.set_modified(long_ago))
            .expect("age the file");
    };
    add("log/2026-09-30.electra.1.jsonl", "{\"kind\":\"user\"}\n");
    add("log/blobs/ab.json", "{\"text\":\"a long body\"}\n");
    add("approvals/01J5FFFFFFFFFFFFFFFFFFFFFF.json", "{}\n");
    add("agent.toml", "agent = \"nixi\"\n");

    assert_eq!(format!("{:?}", row()), format!("{row_before:?}"));
    let pool_after = pool();
    assert_eq!(pool_after.files, pool_before.files, "no untagged residue");
    assert_eq!(pool_after.truncated, pool_before.truncated);
}

/// The scan's id rule and a verb's agree, so a session with no `id` in its
/// record is still found by the id its board row shows.
#[test]
fn a_session_without_a_recorded_id_is_found_by_its_path_id() {
    let zone = zone();
    std::fs::create_dir_all(zone.path().join("active/plain")).expect("session");
    std::fs::write(zone.path().join("active/plain/README.md"), "# Plain\n").expect("record");
    let row = verbs::find(zone.path(), "path:active/plain").expect("found by its path id");
    assert_eq!(row.title, "Plain");
    assert!(verbs::find(zone.path(), "path:active/other").is_none());
}
