//! A mounted drive's agents zone and sessions zone, read from disk into
//! Epic 89's pure functions (AD-361; story 90.5).
//!
//! Every file is reached through `keeper_sync::browse::resolve`, so a link
//! inside a zone that points out of the drive is a refusal, never a read.
//! This module decides nothing: `zone::assess` says which folders are homes,
//! `home::parse_agent_toml` reads each, `session::parse_session_agent_toml`
//! each session.

use std::path::{Path, PathBuf};

use keeper_core::agents::avatar;
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::home::{self, AgentConfig, HomeRefusal};
#[cfg(not(phone))]
use keeper_core::agents::log::{reader::read_session, LineKind};
use keeper_core::agents::session;
#[cfg(not(phone))]
use keeper_core::agents::session::SessionAgent;
use keeper_core::agents::skills::{self, SkillFilter, SkillsIndex};
use keeper_core::agents::zone::{self, ZoneAssessment, ZoneEntry};
#[cfg(not(phone))]
use keeper_core::sessions::model::ACTIVE_DIR;
use keeper_sync::browse;
use keeper_sync::lfs::pointer;
use keeper_sync::lfs::store::LfsStore;
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
#[cfg(not(phone))]
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

/// A home's picture as read under a root: `avatar::scan`'s verdict, and
/// the root-relative path the chosen file landed at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HomeAvatar {
    pub scan: avatar::AvatarScan,
    /// Where the picture landed, `/`-joined under the root
    /// (`browse::Landing::relative`); `None` without a picture.
    pub path: Option<String>,
}

/// The picture of the home `home` (root-relative) under `root` (story 100.4):
/// each candidate reached through `browse::resolve_known` as every zone file
/// is, its size read before its bytes; a git-LFS pointer is parsed and its
/// object's size read from it before anything else (R399): past
/// [`avatar::MAX_BYTES`] it is too large, an object in `store` (the drive's
/// `.git/lfs`) is read as the picture's bytes, and an object not on this
/// device is no picture here. The home's own entries are listed for any
/// other `avatar.*`. `avatar::scan` decides.
pub fn read_avatar(root: &Path, home: &str, store: &LfsStore) -> HomeAvatar {
    let names: Vec<String> = match browse::resolve_known(root, home).map(browse::Known::landed) {
        Ok(Some(landing)) => std::fs::read_dir(landing.path())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut landed: Vec<(&'static str, String)> = Vec::new();
    let scan = avatar::scan(&names, |file| {
        let rel = format!("{home}/{file}");
        let landing = match browse::resolve_known(root, &rel).map(browse::Known::landed) {
            Ok(Some(landing)) => landing,
            Ok(None) => return avatar::Candidate::Absent,
            Err(refusal) => return avatar::Candidate::Unreadable(refusal.to_string()),
        };
        let size = match std::fs::metadata(landing.path()) {
            Ok(meta) if meta.is_file() => meta.len(),
            Ok(_) => return avatar::Candidate::Unreadable("it is not a file".to_owned()),
            Err(error) => return avatar::Candidate::Unreadable(error.to_string()),
        };
        if size > avatar::MAX_BYTES {
            return avatar::Candidate::TooLarge(size);
        }
        let bytes = match std::fs::read(landing.path()) {
            Ok(bytes) => bytes,
            Err(error) => return avatar::Candidate::Unreadable(error.to_string()),
        };
        if !pointer::is_pointer_candidate(&bytes) {
            landed.push((file, landing.relative()));
            return avatar::Candidate::Bytes(bytes);
        }
        let Some(lfs) = pointer::Pointer::parse(&bytes) else {
            return avatar::Candidate::Unreadable(
                "it is a git-LFS pointer keeper cannot read".to_owned(),
            );
        };
        if lfs.size > avatar::MAX_BYTES {
            return avatar::Candidate::TooLarge(lfs.size);
        }
        if !store.contains(&lfs.oid, lfs.size) {
            return avatar::Candidate::NotHere;
        }
        match std::fs::read(store.object_path(&lfs.oid)) {
            Ok(object) => {
                landed.push((file, landing.relative()));
                avatar::Candidate::Bytes(object)
            }
            Err(error) => avatar::Candidate::Unreadable(error.to_string()),
        }
    });
    let path = scan.avatar.as_ref().and_then(|picture| {
        landed
            .iter()
            .find(|(file, _)| *file == picture.file)
            .map(|(_, path)| path.clone())
    });
    HomeAvatar { scan, path }
}

/// The git-LFS object store of the drive checked out at `drive_root`.
pub fn lfs_store(drive_root: &Path) -> LfsStore {
    LfsStore::in_git_dir(drive_root.join(".git"))
}

/// [`read_avatar`] of `home`, under its agents zone, its pointers' objects
/// in the store of the drive checked out at `drive_root`.
pub fn home_avatar(home: &AgentHome, drive_root: &Path) -> HomeAvatar {
    read_avatar(&home.zone, &home.config.id, &lfs_store(drive_root))
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
        // A gate's delegates are agents of this zone, and its peer is none
        // of them (99.1 #1): the gate is refused, its first fault named.
        for (folder, refusal) in home::gate_zone_refusals(&configs) {
            if let Some((_, home)) = read.iter_mut().find(|(at, _)| *at == folder) {
                if home.is_ok() {
                    *home = Err(refusal.to_string());
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

/// One home's `agent.toml`, read and parsed against `decl`: the scan's read,
/// and every turn's re-read of its model (100.2 #5).
pub(crate) fn read_home(
    zone_dir: &Path,
    folder: &str,
    decl: &DriveDecl,
) -> Result<AgentHome, String> {
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
#[cfg(not(phone))]
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
#[cfg(not(phone))]
pub fn paseo_captured(agent: &SessionAgent, dir: &Path) -> bool {
    follows_a_run(agent)
        && read_session(dir)
            .lines
            .iter()
            .any(|line| line.kind() == LineKind::Paseo)
}

/// Whether `agent` is a session keeper made to follow a Paseo run: a
/// scheduled one whose `[reply]` names the run.
#[cfg(not(phone))]
pub(crate) fn follows_a_run(agent: &SessionAgent) -> bool {
    agent.kind == session::SessionKind::Scheduled
        && agent
            .reply
            .as_ref()
            .is_some_and(|reply| reply.run.is_some())
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
