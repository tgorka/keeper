//! `session_write`: the one way an agent puts a file into a session (AD-368).
//!
//! Through the journaled executor, so the write holds the zone and survives a
//! crash like every other plan. Three places take it: `artifacts/`, with the
//! extensions a person's create-file takes plus BMAD's output kinds
//! (`files::compile_agent_file`, R112); `workspace/`, the session's scratch,
//! with any extension; and the session's own pool — a card, a log, a note —
//! with a person's extensions, where an existing file is replaced through a
//! guarded write. keeper's own files — the log, the approvals, `agent.toml`,
//! the record and the navigation contract — are never an agent's to write.
//!
//! Two doors beside it serve the BMAD tools: [`memlog_write`], the one
//! dotted file (`artifacts/**/.memlog.md`), and [`publish_generation`], a
//! render generation published whole into `workspace/`.

use std::collections::HashMap;
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
    session_write_with(zone, session, rel, may_write, |_, _| Ok(content.to_owned()))
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
    land(zone, session, rel, None)
}

/// [`landing`], where the last name may be the dotted `leaf` — the one
/// exception [`memlog_write`] makes — and no other name is dotted.
fn land(zone: &Path, session: &str, rel: &str, leaf: Option<&str>) -> Result<String, VerbError> {
    if classify(session).is_none() {
        return Err(VerbError::Refused(format!(
            "{session} is not a session folder"
        )));
    }
    let plain = || VerbError::Refused(format!("{rel} is not a plain path inside the session"));
    let dotted = |names: &[&str]| {
        names.iter().enumerate().any(|(at, name)| {
            name.starts_with('.') && !(at + 1 == names.len() && leaf == Some(*name))
        })
    };
    let segments = browse::plain_segments(rel).map_err(|_| plain())?;
    let requested: Vec<&str> = segments
        .iter()
        .map(|part| part.to_str().unwrap_or("."))
        .collect();
    if requested.is_empty() || dotted(&requested) {
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
    let landed_names: Vec<&str> = landed.iter().map(String::as_str).collect();
    if dotted(&landed_names) {
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
/// write lands — the seam an agent's write is stamped through (R52, R119),
/// and refused through with `compose`'s sentence, nothing written (the
/// `artifacts/knowledge/**` rule, R140). An existing file is replaced
/// through a write guarded on the exact bytes composed from (R120).
pub fn session_write_with(
    zone: &Path,
    session: &str,
    rel: &str,
    may_write: &dyn Fn() -> bool,
    compose: impl FnOnce(&str, Option<&str>) -> Result<String, String>,
) -> Result<(), VerbError> {
    let held = exec::hold(zone)?;
    let landed = landing(held.zone(), session, rel)?;
    let refused = |refusal: files::FileVerbError| VerbError::Refused(refusal.to_string());
    let mut parts = landed.split('/');
    let top = parts.next().unwrap_or_default();
    let nested = parts.next().is_some();
    let place = if top == ARTIFACTS_DIR && nested {
        files::check_agent_file(&landed).map_err(refused)?;
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
    let content = compose(&landed, old.as_deref()).map_err(VerbError::Refused)?;
    let path = format!("{session}/{landed}");
    let mut steps = Vec::with_capacity(2);
    if let Some((parent, _)) = landed.rsplit_once('/') {
        steps.push(PlanStep::MkDir {
            path: format!("{session}/{parent}"),
        });
    }
    let plan = match (place, old) {
        (Place::Artifact, _) => {
            files::compile_agent_file(session, &landed, &content).map_err(refused)?
        }
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

/// Write the memlog at `rel`, session-relative, in the session at
/// `session`, with what `compose` makes of its bytes as they are while the
/// zone is held (`None` when there is no memlog yet), or refuse with the
/// sentence `compose` gives. The door is `files::check_memlog`'s, asked of
/// the path requested and of where it lands; the write is one journaled,
/// atomic and durable replace, guarded on the bytes composed from (R120),
/// made only while `may_write` — the session's claim — says this host may.
pub fn memlog_write(
    zone: &Path,
    session: &str,
    rel: &str,
    may_write: &dyn Fn() -> bool,
    compose: impl FnOnce(Option<&str>) -> Result<String, String>,
) -> Result<(), VerbError> {
    let refused = |refusal: files::FileVerbError| VerbError::Refused(refusal.to_string());
    files::check_memlog(rel).map_err(refused)?;
    let held = exec::hold(zone)?;
    let landed = land(held.zone(), session, rel, Some(files::MEMLOG))?;
    files::check_memlog(&landed).map_err(refused)?;
    let dir = browse::lexical_join(held.zone(), session)
        .map_err(|_| VerbError::NoSuchSession(session.to_owned()))?;
    let file = browse::lexical_join(&dir, &landed)
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    // Only a memlog that is not there is absent: one that is there but
    // cannot be read is evidence, never started over.
    let old = match std::fs::read_to_string(file) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(VerbError::Refused(format!(
                "{rel} is there but could not be read ({error}); nothing was written."
            )))
        }
    };
    let content = compose(old.as_deref()).map_err(VerbError::Refused)?;
    let plan =
        files::compile_memlog(session, &landed, old.as_deref(), &content).map_err(refused)?;
    if !may_write() {
        return Err(VerbError::Refused(NO_CLAIM.to_owned()));
    }
    Ok(exec::run_held(plan, &held)?)
}

/// What [`publish_generation`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Published {
    /// The generation was written.
    Wrote,
    /// It was there already, and `verify` passed it: nothing was written.
    Verified,
}

/// Publish a render generation as the folder `dir` (session-relative,
/// under `workspace/`) of the session at `session`, holding `files`
/// (relative to `dir`), as `render_skill.py`'s `_publish` does: written
/// into a staging folder beside it and moved into place, all in one
/// journaled plan, so the folder appears whole or not at all. The staging
/// folder is this plan's own — a name no other run uses, made new and real
/// inside the session — and its whole tree is checked to be exactly
/// `files` before the move. A folder already there is handed to `verify`
/// with every file it holds, by relative path, or with why its tree is not
/// only folders and regular files (a link, another kind of entry, a read
/// that failed), and nothing is written; `verify`'s sentence refuses.
pub fn publish_generation(
    zone: &Path,
    session: &str,
    dir: &str,
    files: &[(String, String)],
    verify: impl FnOnce(Result<&HashMap<String, Vec<u8>>, String>) -> Result<(), String>,
    may_write: &dyn Fn() -> bool,
) -> Result<Published, VerbError> {
    let held = exec::hold(zone)?;
    let landed = landing(held.zone(), session, dir)?;
    let Some((parent, leaf)) = landed
        .rsplit_once('/')
        .filter(|(parent, _)| parent.split('/').next() == Some(WORKSPACE_DIR))
        .filter(|_| dir.split('/').next() == Some(WORKSPACE_DIR))
    else {
        return Err(VerbError::Refused(format!(
            "{dir} is not a folder of this session's workspace/"
        )));
    };
    let session_dir = browse::lexical_join(held.zone(), session)
        .map_err(|_| VerbError::NoSuchSession(session.to_owned()))?;
    let target = browse::lexical_join(&session_dir, &landed)
        .map_err(|refusal| VerbError::Refused(refusal.to_string()))?;
    if target.symlink_metadata().is_ok() {
        let found = exec::regular_files(&target);
        verify(found.as_ref().map_err(Clone::clone)).map_err(VerbError::Refused)?;
        return Ok(Published::Verified);
    }
    let staging = format!("{session}/{parent}/.staging-{leaf}-{}", ulid::Ulid::new());
    let mut steps = vec![
        PlanStep::MkDir {
            path: format!("{session}/{parent}"),
        },
        PlanStep::MkDirNew {
            path: staging.clone(),
        },
    ];
    let mut folders = std::collections::BTreeSet::new();
    for (name, _) in files {
        let mut at = 0;
        while let Some(slash) = name[at..].find('/') {
            at += slash;
            folders.insert(&name[..at]);
            at += 1;
        }
    }
    // Sorted, a folder comes before every folder inside it.
    steps.extend(folders.into_iter().map(|folder| PlanStep::MkDirNew {
        path: format!("{staging}/{folder}"),
    }));
    for (name, content) in files {
        steps.push(PlanStep::WriteFile {
            path: format!("{staging}/{name}"),
            content: content.clone(),
        });
    }
    steps.push(PlanStep::PublishDir {
        from: staging,
        to: format!("{session}/{landed}"),
        files: files
            .iter()
            .map(|(name, content)| {
                (
                    name.clone(),
                    keeper_core::agents::approval::sha256_hex(content.as_bytes()),
                )
            })
            .collect(),
    });
    let plan = Plan {
        verb: "render-publish".to_owned(),
        session: session.to_owned(),
        steps,
    };
    if !may_write() {
        return Err(VerbError::Refused(NO_CLAIM.to_owned()));
    }
    exec::run_held(plan, &held)?;
    Ok(Published::Wrote)
}

/// `path`, a file of the session whose folder is `dir` (drive-relative),
/// as the session's doors take it: session-relative — as given, or with
/// `dir/` taken off its front, the spelling of `bmad_config`'s and
/// `bmad_render`'s write locations (R96). One rule for every tool that
/// writes into the session and for where its audit row says it wrote.
pub fn in_session<'p>(dir: &str, path: &'p str) -> &'p str {
    path.strip_prefix(dir)
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}
