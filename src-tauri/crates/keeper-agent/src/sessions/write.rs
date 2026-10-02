//! `session_write`: the one way an agent puts a file into a session (AD-368).
//!
//! Through the journaled executor, so the write holds the zone and survives a
//! crash like every other plan. Only two places take it: `artifacts/`, with
//! the extensions a person's create-file takes (`files::compile_new`), and
//! `workspace/`, the session's scratch, with any extension. keeper's own files
//! — the log, the approvals, `agent.toml`, the record and the navigation
//! contract — are never an agent's to write.

use std::path::Path;

use keeper_core::sessions::files;
use keeper_core::sessions::model::{classify, ARTIFACTS_DIR, WORKSPACE_DIR};
use keeper_core::sessions::plan::{Plan, PlanStep};

use super::exec;
use super::verbs::VerbError;

/// Top-level session entries an agent never writes: keeper keeps them.
const KEEPERS_FILES: [&str; 5] = ["log", "approvals", "agent.toml", "README.md", "AGENTS.md"];

/// Create or replace `rel` inside the session at `session` (zone-relative,
/// `active/<name>` or `archive/<year>/<name>`) with `content`. The session
/// must already be there: a write never makes one.
pub fn session_write(
    zone: &Path,
    session: &str,
    rel: &str,
    content: &str,
) -> Result<(), VerbError> {
    if classify(session).is_none() {
        return Err(VerbError::Refused(format!(
            "{session} is not a session folder"
        )));
    }
    let segments: Vec<&str> = rel.split('/').collect();
    if rel.is_empty()
        || Path::new(rel).is_absolute()
        || segments
            .iter()
            .any(|part| part.is_empty() || *part == ".." || part.starts_with('.'))
    {
        return Err(VerbError::Refused(format!(
            "{rel} is not a plain path inside the session"
        )));
    }
    let top = segments[0];
    if KEEPERS_FILES.contains(&top) {
        return Err(VerbError::Refused(format!(
            "{top} is keeper's own record of this session; an agent writes under {ARTIFACTS_DIR}/ or {WORKSPACE_DIR}/"
        )));
    }
    let plan = if top == ARTIFACTS_DIR && segments.len() > 1 {
        files::compile_new(session, rel, content)
            .map_err(|refusal| VerbError::Refused(refusal.to_string()))?
    } else if top == WORKSPACE_DIR && segments.len() > 1 {
        let mut steps = Vec::with_capacity(2);
        if let Some((parent, _)) = rel.rsplit_once('/') {
            steps.push(PlanStep::MkDir {
                path: format!("{session}/{parent}"),
            });
        }
        steps.push(PlanStep::WriteFile {
            path: format!("{session}/{rel}"),
            content: content.to_owned(),
        });
        Plan {
            verb: "session-write".to_owned(),
            session: session.to_owned(),
            steps,
        }
    } else {
        return Err(VerbError::Refused(format!(
            "{rel} is outside {ARTIFACTS_DIR}/ and {WORKSPACE_DIR}/, where an agent writes"
        )));
    };
    let held = exec::hold(zone)?;
    // The scan's own rule for what is a session: a real folder, not a link.
    let there =
        std::fs::symlink_metadata(held.zone().join(session)).is_ok_and(|meta| meta.is_dir());
    if !there {
        return Err(VerbError::NoSuchSession(session.to_owned()));
    }
    Ok(exec::run_held(plan, &held)?)
}
