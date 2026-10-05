//! What a pushed range rings (R59, R60): the paths a push published,
//! sorted into the four doorbell reasons and the place each is rung.
//!
//! The paths are the engine's (`Engine::changed_paths`), repository-relative
//! and `/`-separated; this module only reads where each one sits in the
//! drive's two zones. A path outside both rings nothing.

use std::collections::BTreeMap;

use crate::agents::events::DoorbellReason;
use crate::agents::log::LOG_DIR;
use crate::agents::session::FILE_NAME as SESSION_FILE;
use crate::sessions::model::{skipped, ACTIVE_DIR, ARTIFACTS_DIR, WORKSPACE_DIR};

/// Where one push rings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rings {
    /// Each active session the push touched, by its folder relative to the
    /// sessions zone (`active/<name>`), with the one reason its room is rung
    /// for: a new or changed `agent.toml` is `session`, else a card is
    /// `card`, else `artifact`.
    pub sessions: BTreeMap<String, DoorbellReason>,
    /// The agents zone changed: `memory`, rung in control rooms.
    pub memory: bool,
}

impl Rings {
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty() && !self.memory
    }
}

/// What `paths` ring for a drive whose sessions zone is `sessions` and
/// whose agents zone is `agents` (each a subfolder of the drive).
pub fn rings(paths: &[String], sessions: Option<&str>, agents: Option<&str>) -> Rings {
    let mut out = Rings::default();
    for path in paths {
        if let Some((session, reason)) = sessions.and_then(|zone| in_session(path, zone)) {
            let reason = out
                .sessions
                .get(&session)
                .map_or(reason, |held| strongest(*held, reason));
            out.sessions.insert(session, reason);
        } else if agents.is_some_and(|zone| under(path, zone).is_some()) {
            out.memory = true;
        }
    }
    out
}

/// `path` below the folder `zone`, when it is.
fn under<'a>(path: &'a str, zone: &str) -> Option<&'a str> {
    let zone = zone.trim().trim_matches('/');
    if zone.is_empty() {
        return None;
    }
    path.strip_prefix(zone)?.strip_prefix('/')
}

/// The active session `path` belongs to in the sessions zone `zone`, and
/// the reason it rings; `None` for the log, the workspace, a dotted or
/// `_` name, an archived session, and anything outside a session.
fn in_session(path: &str, zone: &str) -> Option<(String, DoorbellReason)> {
    let mut parts = under(path, zone)?.split('/');
    if parts.next()? != ACTIVE_DIR {
        return None;
    }
    let name = parts.next()?;
    if skipped(name) {
        return None;
    }
    let rest: Vec<&str> = parts.collect();
    let reason = match rest.as_slice() {
        [] => return None,
        [file] if *file == SESSION_FILE => DoorbellReason::Session,
        [first, ..] if *first == ARTIFACTS_DIR => DoorbellReason::Artifact,
        [first, ..] if *first == LOG_DIR || *first == WORKSPACE_DIR => return None,
        parts if parts.iter().any(|part| part.starts_with('.')) => return None,
        [.., file] if file.to_ascii_lowercase().ends_with(".md") => DoorbellReason::Card,
        _ => return None,
    };
    Some((format!("{ACTIVE_DIR}/{name}"), reason))
}

/// Of two reasons for one room, the one it is rung for.
fn strongest(a: DoorbellReason, b: DoorbellReason) -> DoorbellReason {
    let rank = |reason| match reason {
        DoorbellReason::Session => 0,
        DoorbellReason::Card => 1,
        DoorbellReason::Artifact => 2,
        DoorbellReason::Memory => 3,
    };
    if rank(b) < rank(a) {
        b
    } else {
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|path| (*path).to_owned()).collect()
    }

    const SESSIONS: Option<&str> = Some("60-sessions");
    const AGENTS: Option<&str> = Some("80-agents");

    #[test]
    fn a_push_rings_each_session_once_for_its_strongest_reason_and_the_zone_for_memory() {
        let pushed = paths(&[
            "60-sessions/active/2026-10-05-new/agent.toml",
            "60-sessions/active/2026-10-05-new/brief.md",
            "60-sessions/active/2026-10-05-new/log/0001.jsonl",
            "60-sessions/active/a/notes/plan.md",
            "60-sessions/active/a/artifacts/out.pdf",
            "60-sessions/active/b/artifacts/report.md",
            "60-sessions/active/c/log/0001.jsonl",
            "60-sessions/active/c/workspace/scratch.md",
            "60-sessions/active/_template/brief.md",
            "60-sessions/active/d/.hidden/x.md",
            "60-sessions/archive/2026/e/brief.md",
            "80-agents/nixi/MEMORY.md",
            "10-notes/today.md",
        ]);
        let rung = rings(&pushed, SESSIONS, AGENTS);
        assert_eq!(
            rung.sessions,
            BTreeMap::from([
                ("active/2026-10-05-new".to_owned(), DoorbellReason::Session),
                ("active/a".to_owned(), DoorbellReason::Card),
                ("active/b".to_owned(), DoorbellReason::Artifact),
            ])
        );
        assert!(rung.memory);
    }

    #[test]
    fn notes_alone_or_a_drive_without_zones_ring_nothing() {
        assert!(rings(
            &paths(&["10-notes/today.md", "80-agentsx/a.md"]),
            SESSIONS,
            AGENTS
        )
        .is_empty());
        assert!(rings(&paths(&["60-sessions/active/a/brief.md"]), None, None).is_empty());
    }
}
