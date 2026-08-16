//! The lifecycle executor: plans run with a journal beside them (AD-111,
//! NFR-38).
//!
//! `keeper_core::sessions::plan` compiles; this runs. One plan at a time per
//! zone (a `Mutex` — lifecycle verbs are human-paced), each step idempotent,
//! and the journal row in `<zone>/.keeper/sessions-journal.json` written
//! BEFORE the first step and cleared AFTER the last — so a crash leaves a
//! resumable record naming the verb, the plan and the completed prefix. On
//! registry start, an incomplete journal resumes by re-running the remaining
//! steps; idempotency is what makes "re-run" the whole recovery story.
//!
//! Nothing here decides. A plan arrives compiled; refusals (`GuardedWrite`
//! mismatch, a missing source) surface as errors the IPC layer sentences.

use std::path::{Path, PathBuf};

use keeper_core::sessions::plan::{Plan, PlanStep};

/// The journal file, zone-relative. Inside `.keeper/` so it never syncs.
const JOURNAL_REL: &str = ".keeper/sessions-journal.json";

/// One persisted run: the plan and how far it got.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JournalRow {
    plan: Plan,
    /// Steps completed, a prefix of `plan.steps`.
    done: usize,
}

/// Everything the executor can refuse or fail with. `Refused` is a decision
/// (the caller re-plans); `Failed` is the disk saying no.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("{0}")]
    Refused(String),
    #[error("step {step} of {verb} failed: {reason}")]
    Failed {
        verb: String,
        step: usize,
        reason: String,
    },
}

/// Run a plan against a zone root, journaled. Synchronous — lifecycle verbs
/// are single-digit file counts, and the callers run on blocking tasks.
pub fn run(zone: &Path, plan: Plan) -> Result<(), ExecError> {
    let journal = zone.join(JOURNAL_REL);
    if journal.exists() {
        // An unfinished earlier run: resume it first rather than interleave.
        resume(zone)?;
    }
    write_journal(
        &journal,
        &JournalRow {
            plan: plan.clone(),
            done: 0,
        },
    )?;
    run_from(zone, &journal, plan, 0)
}

/// Resume the zone's journaled run, if one is pending. Called at registry
/// start and before any new plan. A journal that cannot be read is renamed
/// aside rather than deleted — evidence, not litter.
pub fn resume(zone: &Path) -> Result<(), ExecError> {
    let journal = zone.join(JOURNAL_REL);
    if !journal.exists() {
        return Ok(());
    }
    let row: JournalRow = match std::fs::read_to_string(&journal)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
    {
        Some(row) => row,
        None => {
            let aside = journal.with_extension("json.unreadable");
            let _ = std::fs::rename(&journal, &aside);
            tracing::warn!(path = %aside.display(), "sessions: unreadable journal set aside");
            return Ok(());
        }
    };
    tracing::info!(
        verb = %row.plan.verb,
        session = %row.plan.session,
        done = row.done,
        total = row.plan.steps.len(),
        "sessions: resuming a journaled run"
    );
    let done = row.done;
    run_from(zone, &journal, row.plan, done)
}

fn run_from(zone: &Path, journal: &Path, plan: Plan, from: usize) -> Result<(), ExecError> {
    for (index, step) in plan.steps.iter().enumerate().skip(from) {
        run_step(zone, step).map_err(|error| match error {
            ExecError::Refused(_) => {
                // A refusal abandons the plan: the journal clears, because a
                // re-run would refuse identically and the caller re-plans.
                let _ = std::fs::remove_file(journal);
                error
            }
            other => other,
        })?;
        write_journal(
            journal,
            &JournalRow {
                plan: plan.clone(),
                done: index + 1,
            },
        )?;
    }
    std::fs::remove_file(journal).map_err(|error| ExecError::Failed {
        verb: plan.verb.clone(),
        step: plan.steps.len(),
        reason: format!("could not clear the journal: {error}"),
    })
}

/// One idempotent step. The idempotency table is the resume contract:
/// re-running a completed step is a no-op, never an error.
fn run_step(zone: &Path, step: &PlanStep) -> Result<(), ExecError> {
    let failed = |reason: String| ExecError::Refused(reason);
    match step {
        PlanStep::MkDir { path } => std::fs::create_dir_all(zone.join(rel(path)?))
            .map_err(|e| failed(format!("mkdir {path}: {e}"))),
        PlanStep::CopyFile { from, to } => {
            let source = zone.join(rel(from)?);
            let target = zone.join(rel(to)?);
            if !source.exists() && target.exists() {
                // The copy already happened and the source has since gone
                // (an archive resume after its own EmptyDirKeep): complete.
                return Ok(());
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::copy(&source, &target)
                .map(|_| ())
                .map_err(|e| failed(format!("copy {from} → {to}: {e}")))
        }
        PlanStep::WriteFile { path, content } => atomic_write(&zone.join(rel(path)?), content)
            .map_err(|e| failed(format!("write {path}: {e}"))),
        PlanStep::GuardedWrite {
            path,
            expect_len,
            content,
        } => {
            let target = zone.join(rel(path)?);
            let current = std::fs::read_to_string(&target)
                .map_err(|e| failed(format!("read {path}: {e}")))?;
            if current.len() != *expect_len {
                // Idempotency first: a resume re-running a completed guarded
                // write sees its own output. Then the real guard.
                if current == *content {
                    return Ok(());
                }
                return Err(ExecError::Refused(format!(
                    "{path} changed while this was being planned; nothing was written — try again"
                )));
            }
            atomic_write(&target, content).map_err(|e| failed(format!("write {path}: {e}")))
        }
        PlanStep::MoveDir { from, to } => {
            let source = zone.join(rel(from)?);
            let target = zone.join(rel(to)?);
            if !source.exists() && target.exists() {
                return Ok(()); // already moved
            }
            // A target that exists AND is a different directory is the refusal
            // this step is here to make. A target that exists and IS the source
            // is not: on APFS and NTFS `_template/interview` exists the moment
            // `_template/Interview` does, so a case-only rename — the one that
            // normalises a hand-made name — would be refused by its own source.
            if target.exists() && !same_directory(&target, &source) {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
            std::fs::rename(&source, &target)
                .map_err(|e| failed(format!("move {from} → {to}: {e}")))
        }
        PlanStep::MoveFile { from, to } => {
            let source = zone.join(rel(from)?);
            let target = zone.join(rel(to)?);
            // **No already-moved short-circuit here, unlike `MoveDir` above.**
            // That one infers "this plan already ran" from a gone source and a
            // present target, and the inference holds where it lives: a resumed
            // journal's only writer produced that exact pair. `MoveFile` has no
            // crash-resume caller — its one caller is
            // `sessions_template_rename_entry`, which stats the source through
            // `entry_kind` and runs the plan straight away — so the same test
            // proves nothing about the target: if the source disappears in that
            // window (a sync pull, an agent, a move in Finder) and the typed
            // destination happens to name an existing neighbour, the step would
            // answer Ok, clear the journal, and hand the room the subpath of a
            // file it never touched. A missing source is a stale list, and the
            // rename error is what says so.
            // `MoveDir`'s guard above, verbatim in its reasoning and sharing its
            // predicate: a target that exists AND is a different file is a
            // neighbour a rename must not eat, while a target that exists and IS
            // the source is the case-only rename that normalises a hand-made
            // name — `_template/x/About.md` → `about.md` — and on APFS the
            // destination of that one exists because it is the file being
            // renamed. `exists()` alone reads them as one thing.
            if target.exists() && !same_directory(&target, &source) {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
            // No `create_dir_all` for the target's parent, unlike `CopyFile`:
            // a rename moves a file inside a directory that is already there,
            // and inventing a parent here would turn a typo in a plan into a
            // new directory on somebody's drive.
            std::fs::rename(&source, &target)
                .map_err(|e| failed(format!("move {from} → {to}: {e}")))
        }
        PlanStep::TrashDir { path, trash_key } => {
            let source = zone.join(rel(path)?);
            let trash = zone.join(".keeper/trash").join(trash_key);
            if !source.exists() && trash.exists() {
                return Ok(()); // already trashed
            }
            if let Some(parent) = trash.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::rename(&source, &trash).map_err(|e| failed(format!("trash {path}: {e}")))
        }
        PlanStep::TrashFile { path, trash_key } => {
            let source = zone.join(rel(path)?);
            // The basename rides along, so what lands in the trash is
            // `.keeper/trash/<key>/tasks.md` — recoverable by looking at it.
            let name = source
                .file_name()
                .ok_or_else(|| failed(format!("trash {path}: not a file name")))?
                .to_owned();
            let dir = zone.join(".keeper/trash").join(trash_key);
            let target = dir.join(&name);
            if !source.exists() && target.exists() {
                return Ok(()); // already trashed
            }
            std::fs::create_dir_all(&dir).map_err(|e| failed(format!("trash {path}: {e}")))?;
            std::fs::rename(&source, &target).map_err(|e| failed(format!("trash {path}: {e}")))
        }
        PlanStep::EmptyDirKeep { path } => {
            let dir = zone.join(rel(path)?);
            if !dir.exists() {
                std::fs::create_dir_all(&dir).map_err(|e| failed(format!("mkdir {path}: {e}")))?;
            }
            let entries =
                std::fs::read_dir(&dir).map_err(|e| failed(format!("read {path}: {e}")))?;
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name.to_string_lossy() == ".gitkeep" {
                    continue;
                }
                let target = entry.path();
                let removed = if target.is_dir() {
                    std::fs::remove_dir_all(&target)
                } else {
                    std::fs::remove_file(&target)
                };
                removed.map_err(|e| failed(format!("empty {path}: {e}")))?;
            }
            let keep = dir.join(".gitkeep");
            if !keep.exists() {
                std::fs::write(&keep, b"").map_err(|e| failed(format!("gitkeep {path}: {e}")))?;
            }
            Ok(())
        }
    }
}

/// Whether two paths are the **same** thing on the disk, rather than two
/// spellings of it that only look different. Named for the directory move it
/// was extracted from, and asked by [`PlanStep::MoveFile`] too:
/// `canonicalize` does not care whether the path names a file, and a case-only
/// rename of `About.md` is the same trap as one of `Interview/`.
///
/// Asked wherever a move has to tell "the destination is taken" from "the
/// destination IS the source". On APFS and NTFS `_template/interview` exists the
/// moment `_template/Interview` does, so a case-only rename would be refused by
/// the very directory it is renaming; `exists()` alone cannot tell those apart.
///
/// `canonicalize` is the filesystem's own answer, which is why it is the one
/// asked: a lowercased path comparison would invent case-insensitivity on ext4,
/// where two such names are two directories and the refusal is correct. A path
/// that is not there canonicalises to nothing and is never "the same", so an
/// absent destination is not a collision either way.
///
/// Shared with [`crate::sessions_ipc`], which makes the same distinction one
/// layer up so the operator gets a sentence instead of an executor refusal.
/// Two copies of this would be two chances for the two layers to disagree about
/// which moves a zone accepts.
pub(crate) fn same_directory(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left)
        .ok()
        .zip(std::fs::canonicalize(right).ok())
        .is_some_and(|(left, right)| left == right)
}

/// A zone-relative plan path, refused if it escapes — the executor's own
/// containment, independent of who compiled the plan.
fn rel(path: &str) -> Result<PathBuf, ExecError> {
    if path.is_empty()
        || Path::new(path).is_absolute()
        || path.split('/').any(|part| part == ".." || part.is_empty())
    {
        return Err(ExecError::Refused(format!(
            "plan path {path} is not zone-relative"
        )));
    }
    Ok(PathBuf::from(path))
}

/// Write bytes atomically: temp file beside the target, then rename.
fn atomic_write(target: &Path, content: &str) -> std::io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.keeper-tmp",
        target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_owned())
    ));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, target)
}

fn write_journal(journal: &Path, row: &JournalRow) -> Result<(), ExecError> {
    let text = serde_json::to_string_pretty(row).map_err(|e| ExecError::Failed {
        verb: row.plan.verb.clone(),
        step: row.done,
        reason: format!("could not encode the journal: {e}"),
    })?;
    atomic_write(journal, &text).map_err(|e| ExecError::Failed {
        verb: row.plan.verb.clone(),
        step: row.done,
        reason: format!("could not write the journal: {e}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use keeper_core::sessions::plan::{
        compile_archive, compile_create, compile_delete, ArchiveDecision,
    };

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

    /// A create runs end to end: template copied, README stamped, journal
    /// cleared. The plan compiled in core; the executor only obeys.
    #[test]
    fn a_create_plan_lands_a_session_and_clears_the_journal() {
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
        run(zone.path(), plan).expect("runs");
        let readme =
            std::fs::read_to_string(zone.path().join("active/2026-08-12-research/README.md"))
                .expect("readme");
        assert!(readme.contains("# research"));
        assert!(zone
            .path()
            .join("active/2026-08-12-research/workspace")
            .is_dir());
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// The archive's crash story (NFR-38): kill the run before the move, and
    /// a resume completes it — promotes idempotent, move-last honoured.
    #[test]
    fn a_journaled_archive_resumes_after_a_crash_before_the_move() {
        let zone = zone();
        let session = zone.path().join("active/2026-08-10-keeper");
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        std::fs::create_dir_all(session.join("artifacts")).expect("mkdir");
        std::fs::write(session.join("README.md"), "# keeper\n").expect("write");
        std::fs::write(session.join("workspace/draft.md"), "the draft").expect("write");

        let plan = compile_archive(
            "active/2026-08-10-keeper",
            &ArchiveDecision {
                promotes: vec![(
                    "workspace/draft.md".to_owned(),
                    "artifacts/report.md".to_owned(),
                )],
                empty_workspace: true,
                year: 2026,
            },
        );
        // Simulate the crash: journal written, first step done, process gone.
        let journal = zone.path().join(JOURNAL_REL);
        write_journal(
            &journal,
            &JournalRow {
                plan: plan.clone(),
                done: 0,
            },
        )
        .expect("journal");
        run_step(zone.path(), &plan.steps[0]).expect("the promote copy");
        write_journal(
            &journal,
            &JournalRow {
                plan: plan.clone(),
                done: 1,
            },
        )
        .expect("journal");

        // Relaunch: resume finishes the remaining steps.
        resume(zone.path()).expect("resumes");
        let moved = zone.path().join("archive/2026/2026-08-10-keeper");
        assert!(
            moved.join("artifacts/report.md").exists(),
            "the promote landed"
        );
        assert!(
            moved.join("workspace/.gitkeep").exists(),
            "workspace emptied"
        );
        assert!(!moved.join("workspace/draft.md").exists());
        assert!(!zone.path().join("active/2026-08-10-keeper").exists());
        assert!(!journal.exists(), "journal cleared after resume");
    }

    /// A delete is a recoverable trash move — the folder, workspace and all,
    /// sits under .keeper/trash keyed by id.
    #[test]
    fn a_delete_lands_in_the_zone_trash_recoverable() {
        let zone = zone();
        let session = zone.path().join("active/x");
        std::fs::create_dir_all(session.join("workspace")).expect("mkdir");
        std::fs::write(session.join("workspace/scratch.md"), "s").expect("write");
        run(
            zone.path(),
            compile_delete("active/x", "01J5AAAAAAAAAAAAAAAAAAAAAA"),
        )
        .expect("runs");
        assert!(!session.exists());
        assert!(zone
            .path()
            .join(".keeper/trash/01J5AAAAAAAAAAAAAAAAAAAAAA/workspace/scratch.md")
            .exists());
    }

    /// A guarded write refuses when the file moved under it — and the refusal
    /// clears the journal so the next attempt re-plans instead of resuming a
    /// stale plan.
    #[test]
    fn a_guarded_write_against_a_moved_file_refuses_and_clears_the_journal() {
        let zone = zone();
        std::fs::create_dir_all(zone.path().join("active/s")).expect("mkdir");
        std::fs::write(zone.path().join("active/s/README.md"), "original").expect("write");
        let plan = Plan {
            verb: "log-today".to_owned(),
            session: "active/s".to_owned(),
            steps: vec![PlanStep::GuardedWrite {
                path: "active/s/README.md".to_owned(),
                expect_len: "stale-length-that-is-wrong".len(),
                content: "clobber".to_owned(),
            }],
        };
        let error = run(zone.path(), plan).expect_err("refuses");
        assert!(matches!(error, ExecError::Refused(_)));
        assert_eq!(
            std::fs::read_to_string(zone.path().join("active/s/README.md")).expect("read"),
            "original",
            "nothing was written"
        );
        assert!(
            !zone.path().join(JOURNAL_REL).exists(),
            "refusal clears the journal"
        );
    }

    /// The executor's own containment: a plan path that escapes refuses, no
    /// matter who compiled it.
    #[test]
    fn an_escaping_plan_path_is_refused_by_the_executor_itself() {
        let zone = zone();
        let plan = Plan {
            verb: "create".to_owned(),
            session: "active/x".to_owned(),
            steps: vec![PlanStep::WriteFile {
                path: "../outside.md".to_owned(),
                content: "no".to_owned(),
            }],
        };
        assert!(matches!(run(zone.path(), plan), Err(ExecError::Refused(_))));
    }

    /// The refusal the `MoveDir` guard exists for, unchanged by the
    /// source-identity carve-out beside it: a target that is a *different*
    /// directory is a neighbour, and a move must not eat one. Asserted on the
    /// sentence, because the IPC layer shows it to the operator.
    #[test]
    fn a_move_onto_a_different_directory_is_still_refused() {
        let zone = zone();
        for name in ["_template/interview", "_template/kick-off"] {
            std::fs::create_dir_all(zone.path().join(name)).expect("mkdir");
        }
        std::fs::write(zone.path().join("_template/kick-off/about.md"), "theirs").expect("write");
        let plan = Plan {
            verb: "template-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveDir {
                from: "_template/interview".to_owned(),
                to: "_template/kick-off".to_owned(),
            }],
        };
        let error = run(zone.path(), plan).expect_err("refuses");
        assert!(matches!(
            &error,
            ExecError::Refused(said)
                if said == "_template/kick-off already exists; nothing was moved"
        ));
        assert!(zone.path().join("_template/interview").is_dir());
        assert_eq!(
            std::fs::read_to_string(zone.path().join("_template/kick-off/about.md")).expect("read"),
            "theirs",
            "the neighbour was not touched"
        );
    }

    /// The carve-out: a target that *resolves to the source* is not a
    /// collision, so the move runs instead of being refused by the directory it
    /// is renaming.
    ///
    /// The case that motivates it — `_template/Interview/` → `interview` on
    /// APFS — cannot be reproduced on a case-sensitive volume, so what is
    /// asserted here is the property the carve-out rests on, spelled a way any
    /// filesystem can produce: two paths for one directory. On macOS the two
    /// paths differ in case instead, and the branch taken is this one.
    #[test]
    fn a_move_whose_target_resolves_to_the_source_is_not_a_collision() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "mine").expect("write");
        let plan = Plan {
            verb: "template-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveDir {
                from: "_template/interview".to_owned(),
                to: "_template/./interview".to_owned(),
            }],
        };
        run(zone.path(), plan).expect("the source is not its own collision");
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "renaming a directory onto itself keeps it"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// Row 1 of the matrix: a file rename runs end to end — at the new path,
    /// gone from the old, journal cleared. The bytes are asserted rather than
    /// only the existence, because a copy-then-delete would also satisfy
    /// "present at `to`, absent at `from`" and this step is a move.
    #[test]
    fn a_move_file_lands_the_file_at_its_new_name_and_clears_the_journal() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "the record").expect("write");
        let plan = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/record.md".to_owned(),
            }],
        };
        run(zone.path(), plan).expect("runs");
        assert_eq!(
            std::fs::read_to_string(dir.join("record.md")).expect("read"),
            "the record"
        );
        assert!(!dir.join("about.md").exists(), "the old name is gone");
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// Row 2: the refusal, and the carve-out beside it — `MoveDir`'s pair of
    /// tests asked of a file, because the two arms share `same_directory` and a
    /// file rename is where the case-only case actually bites (`About.md` is a
    /// name people capitalise; `Interview/` is one they rarely do).
    ///
    /// Both halves in one test because they are one rule: a destination that
    /// exists is a collision exactly when it is a *different* file.
    #[test]
    fn a_move_file_refuses_a_different_file_and_allows_its_own_source() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("about.md"), "mine").expect("write");
        std::fs::write(dir.join("questions.md"), "theirs").expect("write");

        let onto_a_neighbour = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/questions.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_a_neighbour).expect_err("refuses");
        assert!(matches!(
            &error,
            ExecError::Refused(said)
                if said == "_template/interview/questions.md already exists; nothing was moved"
        ));
        assert_eq!(
            std::fs::read_to_string(dir.join("questions.md")).expect("read"),
            "theirs",
            "the neighbour was not written over"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "and the source stayed where it was"
        );

        // The carve-out. The motivating case — `About.md` → `about.md` on APFS —
        // cannot be reproduced on a case-sensitive volume, so this asserts the
        // property it rests on in a spelling every filesystem produces: two paths
        // for one file. On macOS the two differ in case instead, and the branch
        // taken is this one.
        let onto_itself = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/./about.md".to_owned(),
            }],
        };
        run(zone.path(), onto_itself).expect("the source is not its own collision");
        assert_eq!(
            std::fs::read_to_string(dir.join("about.md")).expect("read"),
            "mine",
            "renaming a file onto itself keeps it"
        );
        assert!(!zone.path().join(JOURNAL_REL).exists(), "journal cleared");
    }

    /// A `MoveFile` whose source vanished between the shell's stat and the plan
    /// running answers with the failure, never with somebody else's file.
    ///
    /// `MoveDir`'s already-moved short-circuit reads "source gone, target there"
    /// as "this plan already ran", which is sound for a resumed journal whose
    /// only writer produced that pair. Copied onto `MoveFile` it was unsound: the
    /// rename's source is stated by whoever typed the name, so a neighbour at the
    /// destination satisfies the same test, and the command would have cleared
    /// the journal and answered the room with the subpath of a file it never
    /// touched. Both spellings of the vanished source are asserted, because they
    /// answer differently and both answers must be about THIS plan: a neighbour
    /// is the collision refusal, and nothing at all is the rename error.
    #[test]
    fn a_move_file_whose_source_vanished_never_reports_a_neighbour_as_moved() {
        let zone = zone();
        let dir = zone.path().join("_template/interview");
        std::fs::create_dir_all(&dir).expect("mkdir");
        // No `about.md`: the source the plan names is already gone.
        std::fs::write(dir.join("questions.md"), "theirs").expect("write");

        let onto_a_neighbour = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/questions.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_a_neighbour).expect_err("must not answer Ok");
        assert!(
            matches!(
                &error,
                ExecError::Refused(said)
                    if said == "_template/interview/questions.md already exists; nothing was moved"
            ),
            "a neighbour is a collision, not a completed move: {error}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("questions.md")).expect("read"),
            "theirs",
            "and the neighbour is untouched"
        );

        let onto_nothing = Plan {
            verb: "template-entry-rename".to_owned(),
            session: "_template/interview".to_owned(),
            steps: vec![PlanStep::MoveFile {
                from: "_template/interview/about.md".to_owned(),
                to: "_template/interview/record.md".to_owned(),
            }],
        };
        let error = run(zone.path(), onto_nothing).expect_err("a missing source is a failure");
        // `run_step` reports a failed rename through `Refused` too (its own
        // `failed` closure), so the variant is not what distinguishes this from a
        // collision — the sentence is, and it names the move that did not happen.
        assert!(
            matches!(
                &error,
                ExecError::Refused(said)
                    if said.starts_with(
                        "move _template/interview/about.md → _template/interview/record.md"
                    )
            ),
            "the rename error is what says the list was stale: {error}"
        );
        assert!(!dir.join("record.md").exists(), "and nothing was created");
    }
}
