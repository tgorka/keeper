//! A steward's own work (AD-389): the triage and harvest sessions her host
//! makes for her at start, and what wakes her harvest.
//!
//! Each is a `kind = scheduled` session under an id derived from (drive,
//! agent, duty), so every start of every host names the same one (R66). It
//! holds one card keeper writes from her home's menu — `@daily`, assigned
//! to her, its body the menu's prompts (`TR` then `DS`; `HV`) — in the plan
//! that makes the folder, and never again: an owner's edit of it is kept,
//! and a card the owner deleted stays deleted. The menu is the seeded
//! configuration a person chose, so the card carries no `scheduled_by` and
//! runs on its schedule without anyone allowing it (Q16). Which host makes
//! the room and the folder is decided through Matrix (R165): the claim keyed
//! by the session's id in the principal's control room
//! (`hosts::make_steward`).
//!
//! Harvest (R61, R166): a session of her drive found under `archive/` is one
//! turn in her harvest session, keyed by the closed session's id and
//! carrying its label. What was archived when her harvest session was made
//! is its baseline ([`BASELINE`], written in the same plan) and is never
//! harvested; anything archived since is, however long ago it opened. Only
//! her own triage and harvest sessions are never harvested.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::time::{Duration, SystemTime};

use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::home::{AgentConfig, MenuAction};
use keeper_core::agents::label::{Integrity, Label, Readers};
use keeper_core::agents::log::reader::read_session;
use keeper_core::agents::log::LineBody;
use keeper_core::agents::seed::steward_session_id;
use keeper_core::agents::session::{self, SessionAgent, SessionKind};
use keeper_core::notes::frontmatter::Frontmatter;
use keeper_core::sessions::model::{self, SessionStatus, ARCHIVE_DIR, README};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use ulid::Ulid;

use crate::sessions::scan;

/// The two sessions a steward keeps for herself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duty {
    Triage,
    Harvest,
}

impl Duty {
    pub const ALL: [Duty; 2] = [Duty::Triage, Duty::Harvest];

    /// The session's title, its card's, and the last part of its id.
    pub fn name(self) -> &'static str {
        match self {
            Duty::Triage => "triage",
            Duty::Harvest => "harvest",
        }
    }

    /// The menu codes whose prompts are the card's body, in order.
    fn codes(self) -> &'static [&'static str] {
        match self {
            Duty::Triage => &["TR", "DS"],
            Duty::Harvest => &["HV"],
        }
    }

    /// The card's file name at the session's root.
    pub fn card_file(self) -> String {
        format!("{}.md", self.name())
    }
}

/// The id of `config`'s `duty` session.
pub fn session_id(config: &AgentConfig, duty: Duty) -> Ulid {
    steward_session_id(&config.drive, &config.id, duty.name())
}

/// The name of `config`'s `duty` session room: what tells a room her host
/// made for it from any other (R165).
pub fn room_name(config: &AgentConfig, duty: Duty) -> String {
    format!("{} — {}", config.name, duty.name())
}

/// The prompt of `config`'s menu item `code`.
fn prompt<'c>(config: &'c AgentConfig, code: &str) -> Option<&'c str> {
    config
        .menu
        .iter()
        .find(|item| item.code == code)
        .and_then(|item| match &item.action {
            MenuAction::Prompt(prompt) => Some(prompt.trim()),
            MenuAction::Workflow(_) => None,
        })
}

/// The prompts of `config`'s menu that are `duty`'s card's body, in order;
/// the sentence when her menu lacks one.
pub fn prompts(config: &AgentConfig, duty: Duty) -> Result<Vec<&str>, String> {
    duty.codes()
        .iter()
        .map(|code| {
            prompt(config, code).ok_or_else(|| {
                format!(
                    "{}'s menu has no {code} prompt, so her {} session is not made.",
                    config.name,
                    duty.name()
                )
            })
        })
        .collect()
}

/// The card of `config`'s `duty` session: `@daily`, hers, asked for by the
/// drive's owner, its body her menu's prompts; the sentence when her menu
/// lacks one.
pub fn card(config: &AgentConfig, decl: &DriveDecl, duty: Duty) -> Result<String, String> {
    let prompts = prompts(config, duty)?;
    Ok(format!(
        "---\ntags: [task]\ntitle: {}\nstatus: todo\nassignee: {}\nrequested_by: \"{}\"\nschedule: \"@daily\"\n---\n\n{}\n",
        duty.name(),
        config.id,
        decl.owner,
        prompts.join("\n\n")
    ))
}

/// `config`'s `duty` session in `room`, made at `now`: hers, asked for by
/// the drive's owner, over her drive, at its opening label.
pub fn session(
    config: &AgentConfig,
    decl: &DriveDecl,
    duty: Duty,
    room: &matrix_sdk::ruma::RoomId,
    now: chrono::DateTime<chrono::Local>,
) -> SessionAgent {
    SessionAgent {
        id: session_id(config, duty),
        agent: config.id.clone(),
        drive: config.drive.clone(),
        kind: SessionKind::Scheduled,
        title: duty.name().to_owned(),
        requested_by: decl.owner.clone(),
        parent: None,
        room: room.to_owned(),
        drives: vec![config.drive.clone()],
        label: Label::opening(decl, Integrity::Owner),
        needs: None,
        pin: None,
        hop: 0,
        dispatch_chain: vec![decl.owner.clone(), config.matrix_user.clone()],
        limits: None,
        workflow: None,
        created_at: now.with_timezone(&chrono::Utc),
    }
}

/// Whether `agent` is a steward's harvest session.
pub fn is_harvest(agent: &SessionAgent) -> bool {
    agent.kind == SessionKind::Scheduled
        && agent.id == steward_session_id(&agent.drive, &agent.agent, Duty::Harvest.name())
}

/// The harvest session's file naming, one id a line, the sessions already
/// archived when it was made: they are never harvested.
pub const BASELINE: &str = "harvest-baseline.txt";

/// The files `duty`'s session folder is made with, beside its `agent.toml`:
/// its card, and for harvest the baseline of the archive under `zone` now.
pub fn folder_files(
    config: &AgentConfig,
    decl: &DriveDecl,
    duty: Duty,
    zone: &Path,
) -> Result<Vec<(String, String)>, String> {
    let mut files = vec![(duty.card_file(), card(config, decl, duty)?)];
    if duty == Duty::Harvest {
        let mut ids: Vec<String> = archived(zone)
            .iter()
            .filter_map(|rel| keeper_id(zone, rel))
            .collect();
        ids.sort();
        ids.dedup();
        let mut text = ids.join("\n");
        text.push('\n');
        files.push((BASELINE.to_owned(), text));
    }
    Ok(files)
}

/// A closed session a harvest turn reads (R166): its id, its path and the
/// label it closed under — what the harvest session joins before its turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Closed {
    /// Its id: what keeps it to one harvest turn.
    pub id: String,
    /// Zone-relative: `archive/<year>/<folder>`.
    pub path: String,
    pub label: Label,
}

/// Why a harvest session does not take `closed` in (R166): its readers do
/// not reach everyone the harvest room is for, or it may go only to a
/// model on the readers' own machines and hers is not. `None`: it may.
pub fn refusal(closed: &Closed, audience: &Readers, local_model: bool) -> Option<&'static str> {
    if !closed.label.may_reach(audience) {
        Some("its readers are fewer than the harvest room's")
    } else if !closed.label.may_use_model(local_model) {
        Some("it may go only to a model on its readers' own machines")
    } else {
        None
    }
}

/// The brief of the harvest turn for `closed`: her `HV` prompt, then the
/// closed session's drive path and id. `None` when her menu has no `HV`.
pub fn harvest_brief(
    config: &AgentConfig,
    sessions_subfolder: &str,
    closed: &Closed,
) -> Option<String> {
    let prompt = prompt(config, "HV")?;
    Some(format!(
        "{prompt}\n\nThe session that closed: {sessions_subfolder}/{} (session {}).",
        closed.path, closed.id
    ))
}

/// The session folders under `zone`'s `archive/`, zone-relative.
fn archived(zone: &Path) -> Vec<String> {
    scan::session_dirs(zone)
        .into_iter()
        .filter(|rel| matches!(model::classify(rel), Some(SessionStatus::Archived(_))))
        .collect()
}

/// The id keeper gave the session at `rel`: its `agent.toml`'s, else its
/// record's frontmatter `id`. `None` while neither can be read.
fn keeper_id(zone: &Path, rel: &str) -> Option<String> {
    let toml = crate::zone::read_text(zone, &format!("{rel}/{}", session::FILE_NAME)).ok()?;
    if let Some(text) = toml {
        return session::parse_session_agent_toml(&text)
            .ok()
            .map(|agent| agent.id.to_string());
    }
    crate::zone::read_text(zone, &format!("{rel}/{README}"))
        .ok()
        .flatten()
        .and_then(|text| record_id(&text))
}

fn record_id(text: &str) -> Option<String> {
    let (fm, _) = Frontmatter::parse(text);
    fm.as_string("id")
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
}

/// What one archived folder is to a harvest.
#[derive(Debug, PartialEq, Eq)]
enum Found {
    Closed(Closed),
    /// Never harvested: one of her own duty sessions.
    Never,
    /// Its identity cannot be read yet — a folder that synced before its
    /// files, a file half written: read again later, never keyed by path.
    Later,
}

/// The archived folder `rel` of `zone` as the harvest session `harvest`
/// reads it. A keeper agent session is keyed by its `agent.toml`, its label
/// that file's joined with every `label` line of its log; a person's by its
/// record's `id`, at the drive's opening label.
fn found(zone: &Path, rel: &str, harvest: &SessionAgent) -> Found {
    match crate::zone::read_text(zone, &format!("{rel}/{}", session::FILE_NAME)) {
        Err(_) => Found::Later,
        Ok(Some(text)) => match session::parse_session_agent_toml(&text) {
            Err(_) => Found::Later,
            Ok(agent)
                if agent.drive == harvest.drive
                    && agent.agent == harvest.agent
                    && Duty::ALL.iter().any(|duty| {
                        agent.id == steward_session_id(&agent.drive, &agent.agent, duty.name())
                    }) =>
            {
                Found::Never
            }
            Ok(agent) => {
                let label = read_session(&zone.join(rel)).lines.iter().fold(
                    agent.label.clone(),
                    |label, line| match &line.body {
                        LineBody::Label(body) => label.join(&body.label()),
                        _ => label,
                    },
                );
                Found::Closed(Closed {
                    id: agent.id.to_string(),
                    path: rel.to_owned(),
                    label,
                })
            }
        },
        Ok(None) => match crate::zone::read_text(zone, &format!("{rel}/{README}")) {
            Ok(Some(text)) => match record_id(&text) {
                Some(id) => Found::Closed(Closed {
                    id,
                    path: rel.to_owned(),
                    label: harvest.label.clone(),
                }),
                None => Found::Later,
            },
            _ => Found::Later,
        },
    }
}

/// How many archived folders one step reads.
pub const READS_PER_STEP: usize = 16;
/// How many closed sessions wait in the harvest worker's queue at once.
pub const IN_FLIGHT: usize = 4;
/// How long a folder that could not be read, or a harvest that failed
/// before it started, waits before it is tried again.
pub const RETRY: Duration = Duration::from_secs(60);
/// How long a closed session handed to the worker waits for its answer
/// before it is handed again.
pub const UNANSWERED: Duration = Duration::from_secs(15 * 60);

/// What a held harvest session's host knows of the archive (R61): each
/// step lists only the year folders that changed, reads at most
/// [`READS_PER_STEP`] folders and keeps at most [`IN_FLIGHT`] handed to the
/// worker unacknowledged. A folder is read once; what it keeps is its name.
/// A takeover starts again from the archive, and the session's log keeps
/// every harvest to one turn.
#[derive(Debug, Default)]
pub struct Harvester {
    /// The ids of [`BASELINE`], once read.
    baseline: Option<HashSet<String>>,
    /// Each `archive/<year>` folder's modification time when it was listed.
    listed: HashMap<String, SystemTime>,
    /// Every archived folder met.
    met: HashSet<String>,
    /// Folders met and not read yet, in the order they were met.
    unread: VecDeque<String>,
    /// Folders to read again, and when.
    later: VecDeque<(String, Instant)>,
    /// Read, waiting for room in the worker's queue.
    ready: VecDeque<Closed>,
    /// Handed to the worker and not acknowledged, by id, and when.
    in_flight: HashMap<String, (Closed, Instant)>,
}

impl Harvester {
    /// One step over `zone` for the harvest session `harvest` at
    /// `harvest_path`: the closed sessions to hand its worker now. Nothing
    /// before the session's baseline has synced.
    pub fn step(
        &mut self,
        zone: &Path,
        harvest_path: &str,
        harvest: &SessionAgent,
        now: Instant,
    ) -> Vec<Closed> {
        if self.baseline.is_none() {
            match crate::zone::read_text(zone, &format!("{harvest_path}/{BASELINE}")) {
                Ok(Some(text)) => {
                    self.baseline = Some(
                        text.lines()
                            .map(str::trim)
                            .filter(|id| !id.is_empty())
                            .map(str::to_owned)
                            .collect(),
                    )
                }
                _ => return Vec::new(),
            }
        }
        self.relist(zone);
        for _ in 0..READS_PER_STEP {
            let rel = if self.later.front().is_some_and(|(_, at)| *at <= now) {
                self.later.pop_front().map(|(rel, _)| rel)
            } else {
                self.unread.pop_front()
            };
            let Some(rel) = rel else {
                break;
            };
            match found(zone, &rel, harvest) {
                Found::Closed(closed)
                    if self
                        .baseline
                        .as_ref()
                        .is_some_and(|baseline| baseline.contains(&closed.id)) => {}
                Found::Closed(closed) => self.ready.push_back(closed),
                Found::Never => {}
                Found::Later => self.later.push_back((rel, now + RETRY)),
            }
        }
        // One handed long ago and never answered — an arrival the router
        // dropped, a worker that stopped — is handed again; the session's
        // log keeps it to one turn.
        let lapsed: Vec<String> = self
            .in_flight
            .iter()
            .filter(|(_, (_, at))| now.saturating_duration_since(*at) >= UNANSWERED)
            .map(|(id, _)| id.clone())
            .collect();
        for id in lapsed {
            if let Some((closed, _)) = self.in_flight.remove(&id) {
                self.ready.push_front(closed);
            }
        }
        let mut handed = Vec::new();
        while self.in_flight.len() < IN_FLIGHT {
            let Some(closed) = self.ready.pop_front() else {
                break;
            };
            if self.in_flight.contains_key(&closed.id) {
                continue;
            }
            self.in_flight
                .insert(closed.id.clone(), (closed.clone(), now));
            handed.push(closed);
        }
        handed
    }

    /// The worker said what became of `id`: `done` — a turn ran, it was a
    /// turn already, or it was refused — settles it; otherwise it is read
    /// and handed again after [`RETRY`].
    pub fn acknowledged(&mut self, id: &str, done: bool, now: Instant) {
        if let Some((closed, _)) = self.in_flight.remove(id) {
            if !done {
                self.later.push_back((closed.path, now + RETRY));
            }
        }
    }

    /// Meet the folders of every year folder of `zone`'s archive that
    /// changed since it was listed.
    fn relist(&mut self, zone: &Path) {
        let archive = zone.join(ARCHIVE_DIR);
        for year in scan::dir_names(&archive) {
            let dir = archive.join(&year);
            let Ok(modified) = std::fs::metadata(&dir).and_then(|meta| meta.modified()) else {
                continue;
            };
            if self.listed.get(&year) == Some(&modified) {
                continue;
            }
            for name in scan::dir_names(&dir) {
                let rel = format!("{ARCHIVE_DIR}/{year}/{name}");
                if matches!(model::classify(&rel), Some(SessionStatus::Archived(_)))
                    && self.met.insert(rel.clone())
                {
                    self.unread.push_back(rel);
                }
            }
            self.listed.insert(year, modified);
        }
    }
}

#[cfg(all(unix, test))]
mod tests;
