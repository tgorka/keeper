//! `session_write`: the one way an agent puts a file into a session (AD-368).
//!
//! Through the journaled executor, so the write holds the zone and survives a
//! crash like every other plan. Three places take it: `artifacts/`, with the
//! extensions a person's create-file takes (`files::compile_new`);
//! `workspace/`, the session's scratch, with any extension; and the session's
//! own pool — a card, a log, a note — with those same extensions, where an
//! existing file is replaced through a guarded write. keeper's own files —
//! the log, the approvals, `agent.toml`, the record and the navigation
//! contract — are never an agent's to write.

use std::path::Path;

use keeper_core::sessions::files;
use keeper_core::sessions::model::{classify, ARTIFACTS_DIR, WORKSPACE_DIR};
use keeper_core::sessions::plan::{Plan, PlanStep};
use keeper_sync::{browse, names};

use super::exec;
use super::verbs::VerbError;

/// Top-level session entries an agent never writes: keeper keeps them.
const KEEPERS_FILES: [&str; 5] = ["log", "approvals", "agent.toml", "README.md", "AGENTS.md"];

/// What a write whose host no longer holds the session's claim says.
pub const NO_CLAIM: &str =
    "This host no longer holds this session's claim, so nothing was written.";

/// Create or replace `rel` inside the session at `session` (zone-relative,
/// `active/<name>` or `archive/<year>/<name>`) with `content`, while
/// `may_write` — the session's claim, asked immediately before the write —
/// says this host may. The session must already be there: a write never
/// makes one.
pub fn session_write(
    zone: &Path,
    session: &str,
    rel: &str,
    content: &str,
    may_write: &dyn Fn() -> bool,
) -> Result<(), VerbError> {
    session_write_with(zone, session, rel, may_write, |_, _| content.to_owned())
}

/// Where in a session a write lands.
enum Place {
    Artifact,
    Workspace,
    Pool,
}

/// Where `rel` lands in the session at `session`, session-relative, or why
/// an agent may not write there (R119). The landing is the disk's, through
/// keeper-sync's [`browse::landing`], never the requested string: the
/// session must be a real folder of the zone reached through no link, the
/// landing must stay inside it — a folder link to another session or out of
/// the zone is refused — and no name of it may be dotted or one of keeper's
/// own, compared as the Mac's volume compares names (`readme.md`,
/// `Approvals/`), so a link inside `workspace/` cannot reach `agent.toml`.
pub fn landing(zone: &Path, session: &str, rel: &str) -> Result<String, VerbError> {
    if classify(session).is_none() {
        return Err(VerbError::Refused(format!(
            "{session} is not a session folder"
        )));
    }
    let plain = || VerbError::Refused(format!("{rel} is not a plain path inside the session"));
    let segments = browse::plain_segments(rel).map_err(|_| plain())?;
    if segments.is_empty()
        || segments
            .iter()
            .any(|part| part.to_string_lossy().starts_with('.'))
    {
        return Err(plain());
    }
    // The scan's own rule for what is a session: a real folder, not a link.
    let missing = || VerbError::NoSuchSession(session.to_owned());
    let dir = browse::lexical_join(zone, session).map_err(|_| missing())?;
    let real = std::fs::symlink_metadata(&dir).is_ok_and(|meta| meta.is_dir())
        && browse::landing(zone, session)
            .is_ok_and(|landed| landed.iter().map(String::as_str).eq(session.split('/')));
    if !real {
        return Err(missing());
    }
    let landed = browse::landing(&dir, rel).map_err(|refusal| {
        VerbError::Refused(format!(
            "{rel} does not stay inside this session: {refusal}"
        ))
    })?;
    let Some(top) = landed.first() else {
        return Err(VerbError::Refused(format!(
            "{rel} is this session's own folder, not a file in it"
        )));
    };
    if landed.iter().any(|name| name.starts_with('.')) {
        return Err(plain());
    }
    if KEEPERS_FILES
        .iter()
        .any(|kept| names::same_entry_folded(kept, top))
    {
        return Err(VerbError::Refused(format!(
            "{top} is keeper's own record of this session; an agent writes its own files beside it"
        )));
    }
    Ok(landed.join("/"))
}

/// [`session_write`], storing what `compose` makes of the file's bytes as
/// they are while the zone is held (`None` for a new file), given where the
/// write lands — the seam an agent's write is stamped through (R52, R119).
/// An existing file is replaced through a write guarded on the exact bytes
/// composed from (R120).
pub fn session_write_with(
    zone: &Path,
    session: &str,
    rel: &str,
    may_write: &dyn Fn() -> bool,
    compose: impl FnOnce(&str, Option<&str>) -> String,
) -> Result<(), VerbError> {
    let held = exec::hold(zone)?;
    let landed = landing(held.zone(), session, rel)?;
    let refused = |refusal: files::FileVerbError| VerbError::Refused(refusal.to_string());
    let mut parts = landed.split('/');
    let top = parts.next().unwrap_or_default();
    let nested = parts.next().is_some();
    let place = if top == ARTIFACTS_DIR && nested {
        files::check_rel(&landed).map_err(refused)?;
        Place::Artifact
    } else if top == WORKSPACE_DIR && nested && rel.split('/').next() == Some(WORKSPACE_DIR) {
        // Scratch takes any extension, so it is asked for by its own name:
        // `Workspace/x` lands there on the Mac's volume, and is refused as
        // it is on a volume where it would be another folder.
        Place::Workspace
    } else {
        files::check_rel(&landed).map_err(refused)?;
        Place::Pool
    };
    let dir = browse::lexical_join(held.zone(), session)
        .map_err(|_| VerbError::NoSuchSession(session.to_owned()))?;
    let file = browse::lexical_join(&dir, &landed)
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    let old = std::fs::read_to_string(file).ok();
    let content = compose(&landed, old.as_deref());
    let path = format!("{session}/{landed}");
    let mut steps = Vec::with_capacity(2);
    if let Some((parent, _)) = landed.rsplit_once('/') {
        steps.push(PlanStep::MkDir {
            path: format!("{session}/{parent}"),
        });
    }
    let plan = match (place, old) {
        (Place::Artifact, _) => files::compile_new(session, &landed, &content).map_err(refused)?,
        (Place::Pool, Some(old)) => {
            steps.push(PlanStep::guarded(path, &old, content));
            Plan {
                verb: "session-write".to_owned(),
                session: session.to_owned(),
                steps,
            }
        }
        (Place::Workspace | Place::Pool, _) => {
            steps.push(PlanStep::WriteFile { path, content });
            Plan {
                verb: "session-write".to_owned(),
                session: session.to_owned(),
                steps,
            }
        }
    };
    // The claim, after the zone's lock is held and right before the effect.
    if !may_write() {
        return Err(VerbError::Refused(NO_CLAIM.to_owned()));
    }
    Ok(exec::run_held(plan, &held)?)
}
