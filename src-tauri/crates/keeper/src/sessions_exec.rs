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
            if target.exists() {
                return Err(ExecError::Refused(format!(
                    "{to} already exists; nothing was moved"
                )));
            }
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
}
