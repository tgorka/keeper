//! A mounted drive's agents zone and sessions zone, read from disk into
//! Epic 89's pure functions (AD-361; story 90.5).
//!
//! Every file is reached through `keeper_sync::browse::resolve`, so a link
//! inside a zone that points out of the drive is a refusal, never a read.
//! This module decides nothing: `zone::assess` says which folders are homes,
//! `home::parse_agent_toml` reads each, `session::parse_session_agent_toml`
//! each session.

use std::path::{Path, PathBuf};

use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::home::{self, AgentConfig, HomeRefusal};
use keeper_core::agents::log::reader::read_session;
use keeper_core::agents::log::LineKind;
use keeper_core::agents::session::{self, SessionAgent};
use keeper_core::agents::skills::{self, SkillFilter, SkillsIndex};
use keeper_core::agents::zone::{self, ZoneAssessment, ZoneEntry};
use keeper_core::sessions::model::ACTIVE_DIR;
use keeper_sync::browse;
use keeper_sync::SyncProfile;

/// One agent this host may serve: its config, where its home is, and the
/// declaration of the drive it is homed in.
#[derive(Debug, Clone)]
pub struct AgentHome {
    pub config: AgentConfig,
    /// The home folder.
    pub dir: PathBuf,
    /// The agents zone the home is in (`_skills/` lives there).
    pub zone: PathBuf,
    /// The home drive's declaration, as the pin accepted it.
    pub drive: DriveDecl,
}

/// One drive's agents zone, read.
#[derive(Debug, Clone)]
pub struct ZoneRead {
    pub drive: String,
    /// Epic 89's verdict over the listing and `_drive.toml`.
    pub assessment: ZoneAssessment,
    /// Each home folder: its agent, or the sentence refusing it.
    pub homes: Vec<(String, Result<AgentHome, String>)>,
}

/// One session found in a sessions zone.
#[derive(Debug, Clone)]
pub struct FoundSession {
    /// Zone-relative: `active/<folder>`.
    pub path: String,
    pub dir: PathBuf,
    /// Its `agent.toml`, or the sentence refusing it.
    pub agent: Result<SessionAgent, String>,
    /// The cards carrying `schedule:` of a `kind = scheduled` session and
    /// whether the bounded read found them all, read at the rescan (92.3);
    /// empty for any other session, whose cards are not read for a schedule.
    pub scheduled: crate::cards::ScheduledScan,
    /// Whether this checkout's log of a session following a Paseo run holds
    /// the run's captured end, read at the rescan (R277); `false` for any
    /// other session, whose log is not read here.
    pub paseo_captured: bool,
}

/// A file's text under `root`, reached through `browse::resolve_known`;
/// `None` only when the disk says it is not there. A link out of `root`, a
/// dangling link, a folder on the way that cannot be searched, or anything
/// but a regular file (a pipe would never end a read), is refused: what is
/// there is not known, which is not the same as nothing.
pub fn read_text(root: &Path, rel: &str) -> Result<Option<String>, String> {
    match browse::resolve_known(root, rel).map(browse::Known::landed) {
        Ok(Some(landing)) if !landing.path().is_file() => Err(format!("{rel} is not a file")),
        Ok(Some(landing)) => match std::fs::read_to_string(landing.path()) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("{rel} could not be read: {error}")),
        },
        Ok(None) => Ok(None),
        Err(refusal) => Err(format!("{rel} is refused: {refusal}")),
    }
}

/// The real directories directly under `dir`, by name, sorted; a link is
/// never followed.
fn listing(dir: &Path) -> Vec<ZoneEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut listed: Vec<ZoneEntry> = entries
        .filter_map(Result::ok)
        .map(|entry| ZoneEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            is_dir: entry.file_type().is_ok_and(|kind| kind.is_dir()),
        })
        .collect();
    listed.sort_by(|a, b| a.name.cmp(&b.name));
    listed
}

/// Read `profile`'s agents zone. `pinned` is the declaration the pin
/// accepted (`headless::zone_verdicts`); `None` when the drive has no zone
/// or it hosts nothing, which reads as a zone with no homes.
pub fn read_zone(drive_id: &str, profile: &SyncProfile, pinned: Option<&DriveDecl>) -> ZoneRead {
    let Some(zone_dir) = profile.agents_root() else {
        return ZoneRead {
            drive: drive_id.to_owned(),
            assessment: zone::assess(&[], None),
            homes: Vec::new(),
        };
    };
    let drive_text = read_text(&zone_dir, drive::FILE_NAME).ok().flatten();
    let assessment = zone::assess(&listing(&zone_dir), drive_text.as_deref());
    let mut homes = Vec::with_capacity(assessment.homes.len());
    if let Some(decl) = pinned {
        let mut read: Vec<(String, Result<AgentHome, String>)> = assessment
            .homes
            .iter()
            .map(|folder| (folder.clone(), read_home(&zone_dir, folder, decl)))
            .collect();
        // One Matrix user is one agent (AD-374): both homes sharing one are
        // refused, naming each other.
        let configs: Vec<AgentConfig> = read
            .iter()
            .filter_map(|(_, home)| home.as_ref().ok().map(|home| home.config.clone()))
            .collect();
        for refusal in home::shared_matrix_users(&configs) {
            if let HomeRefusal::SharedMatrixUser { first, second, .. } = &refusal {
                for (folder, home) in &mut read {
                    if folder == first || folder == second {
                        *home = Err(refusal.to_string());
                    }
                }
            }
        }
        homes = read;
    }
    ZoneRead {
        drive: drive_id.to_owned(),
        assessment,
        homes,
    }
}

fn read_home(zone_dir: &Path, folder: &str, decl: &DriveDecl) -> Result<AgentHome, String> {
    let rel = format!("{folder}/{}", home::FILE_NAME);
    let text = read_text(zone_dir, &rel)?
        .ok_or_else(|| format!("{folder}/ has no {}.", home::FILE_NAME))?;
    let config = home::parse_agent_toml(&text, folder, decl).map_err(|r| r.to_string())?;
    Ok(AgentHome {
        config,
        dir: zone_dir.join(folder),
        zone: zone_dir.to_owned(),
        drive: decl.clone(),
    })
}

/// The session `path` (drive-relative, under the sessions zone `sessions`)
/// names, where the zone holds it now: there, or — archived or moved since
/// — the session of the same folder name under `active/` or
/// `archive/<year>/`, the zone's own location rules. Its `agent.toml` is
/// read as every zone file is ([`read_text`]).
pub fn session_facts(
    root: &Path,
    sessions: &str,
    path: &str,
) -> Option<keeper_core::agents::consolidate::SessionFacts> {
    let rel = path.strip_prefix(sessions)?.strip_prefix('/')?;
    keeper_core::sessions::model::classify(rel)?;
    let name = rel.rsplit('/').next()?;
    let zone = root.join(sessions);
    let moved = crate::sessions::scan::session_dirs(&zone)
        .into_iter()
        .filter(|candidate| candidate.rsplit('/').next() == Some(name));
    std::iter::once(rel.to_owned())
        .chain(moved)
        .find_map(|rel| {
            let text = read_text(&zone, &format!("{rel}/{}", session::FILE_NAME)).ok()??;
            session::parse_session_agent_toml(&text).ok()
        })
        .map(|agent| keeper_core::agents::consolidate::SessionFacts {
            requested_by: agent.requested_by,
        })
}

/// The active sessions of `profile`'s sessions zone that hold an
/// `agent.toml`. Archived sessions are never served.
pub fn active_sessions(profile: &SyncProfile) -> Vec<FoundSession> {
    let Some(zone_dir) = profile.sessions_root() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in listing(&zone_dir.join(ACTIVE_DIR)) {
        if !entry.is_dir || entry.name.starts_with('.') {
            continue;
        }
        let path = format!("{ACTIVE_DIR}/{}", entry.name);
        let rel = format!("{path}/{}", session::FILE_NAME);
        let agent = match read_text(&zone_dir, &rel) {
            Ok(None) => continue,
            Ok(Some(text)) => {
                session::parse_session_agent_toml(&text).map_err(|refusal| refusal.sentence())
            }
            Err(sentence) => Err(sentence),
        };
        let dir = zone_dir.join(&path);
        let (scheduled, paseo_captured) = match &agent {
            Ok(agent) if agent.kind == session::SessionKind::Scheduled => (
                crate::cards::scheduled_cards(&path, &dir),
                paseo_captured(agent, &dir),
            ),
            _ => (crate::cards::ScheduledScan::default(), false),
        };
        found.push(FoundSession {
            dir,
            path,
            agent,
            scheduled,
            paseo_captured,
        });
    }
    found
}

/// Whether `agent`, a session in `dir`, follows a Paseo run whose end its
/// log there holds captured (R277).
pub fn paseo_captured(agent: &SessionAgent, dir: &Path) -> bool {
    crate::agent::follows_a_run(agent)
        && read_session(dir)
            .lines
            .iter()
            .any(|line| line.kind() == LineKind::Paseo)
}

/// The zone's `_skills/<name>/SKILL.md`, offered as the agent's
/// `[tools].skills` says.
pub fn skills_of(home: &AgentHome) -> SkillsIndex {
    let mut found = Vec::new();
    if let Ok(entries) = std::fs::read_dir(home.zone.join("_skills")) {
        for entry in entries.filter_map(Result::ok) {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Ok(Some(text)) = read_text(&home.zone, &format!("_skills/{name}/SKILL.md")) {
                found.push((name, text));
            }
        }
    }
    skills::index(&found, &SkillFilter::from_list(&home.config.skills))
}
