//! The nightly consolidation on an always-on host (AD-401, story 95.2).
//!
//! Once a night — `0 3 * * *` at the host's offset, the latest such instant
//! at or before now being the night's window W — the host that holds the
//! drive's one maintenance claim `maintain:<drive>` in the principal's
//! control room ([`crate::maintain`]), the night's completion under
//! `consolidate:<drive>` not naming W yet, pulls the
//! drive (`Engine::sync_once`), and for each agent homed there reads the
//! agent's files, its `agent.toml` and the drive's `_drive.toml` at the
//! fetched `HEAD`, has [`keeper_core::agents::consolidate::plan`] decide, and
//! writes what it answers in one `Engine::commit_paths` per agent: the files,
//! guarded by the blob ids the plan read (the two declarations among them);
//! the settled proposals moved to `proposals/done/` beside their verdicts;
//! and, when a change waits for a person, the review session with its card,
//! the before/after previews and one T2 `memory_apply` record each (R128).
//! Nothing here composes prose.
//!
//! A host consolidates a drive only when it serves every agent homed there,
//! each review room made by that agent's own copy. A night is owed while the
//! lease's claim does not say, released, that W ran (R129): a host that
//! missed any number of nights runs once. The claim is renewed while the
//! night runs, and every effect — a room, a commit's publication — is
//! fenced on it: a holder that cannot renew stops before its next one. Who
//! the agent answers to is read again right before a room is made and
//! before each commit is published. A night is remembered done only once
//! every agent of it settled — every commit it published followed by its
//! files, every decision a person took carried out — its claim's release
//! was accepted and its completion recorded under the claim; a claim held
//! elsewhere — by the night of another host or by another maintenance job
//! — or a failed agent is tried again later.
//!
//! A person's decision on a record is carried out only once the existing
//! consume-once authorization of that approval succeeded — the session's
//! worker judged the sender, wrote the decision, won the room's `consumed`
//! and logged it (R206) — exactly as previewed, or rejects the proposals in
//! that person's name on a denial; a refused or expired record changes
//! nothing; a target changed since is never written over. The commit that
//! carries a decision out names its record (`Approval-Record`), and a record
//! named anywhere in the drive's history — read from the commit graph, not
//! from commit dates — is carried out: never again, whatever the files say
//! since — a person who reverts it is not overruled. A history that cannot
//! be read carries nothing out and frees nothing: the record keeps its
//! file and its proposals until it is known.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use keeper_core::agents::approval::{self, Decision, FilePin};
use keeper_core::agents::claim;
use keeper_core::agents::consolidate::{
    self as night, sink_refusal, After, ApplyArgs, FileChange, Night as NightInput, Plan, Review,
    SessionFacts, Settled, Verdict, REVIEW_SESSION_INFIX,
};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::home::{self as agent_home, AgentConfig};
use keeper_core::agents::label::{Integrity, Label};
use keeper_core::agents::log::{reader::read_session, ApprovalState, LineBody};
use keeper_core::agents::proposal::{self, Proposal};
use keeper_core::agents::session::{
    compose_session_agent_toml, parse_session_agent_toml, SessionAgent, SessionKind,
    FILE_NAME as SESSION_FILE,
};
use keeper_core::agents::tier::APPROVALS_DIR;
use keeper_core::sessions::model::ACTIVE_DIR;
use keeper_sync::tasks::TaskSchedule;
use keeper_sync::{
    blob_id, CommitFence, CommitPaths, CommitRequest, Engine, MemoryTrailer, SyncProfile,
};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};
use ulid::Ulid;

use crate::approvals::{read_stored_decision, read_stored_record, stored_ids};
use crate::claims::{Rtt, ServerClock};
use crate::maintain::{maintain, Maintained};
use crate::zone::{read_text, AgentHome};

/// The card a review session opens with.
pub const REVIEW_CARD: &str = "memory-review.md";

/// What every effect of a night asks first: whether the drive's
/// maintenance claim still lets this host write.
pub type Fence = CommitFence;

/// The night window W at `now_ms` read at `offset` minutes: the latest
/// [`night::SCHEDULE`] instant at or before now, epoch ms.
pub fn night_window(now_ms: i64, offset: i32) -> Option<i64> {
    latest_window(night::SCHEDULE, 2, now_ms, offset)
}

/// The latest instant of `schedule` at or before `now_ms` at `offset`
/// minutes, epoch ms, found within the `lookback_days` before now.
pub fn latest_window(schedule: &str, lookback_days: i64, now_ms: i64, offset: i32) -> Option<i64> {
    let schedule = TaskSchedule::parse(schedule).ok()?;
    let first = schedule
        .next_due_after(
            now_ms.saturating_sub(lookback_days * 24 * 60 * 60_000),
            offset,
        )
        .filter(|first| *first <= now_ms)?;
    Some(crate::cards::latest_fire(&schedule, first, now_ms, offset))
}

/// One agent's home as the night reads it.
#[derive(Debug, Clone)]
pub struct Home {
    pub drive: String,
    pub profile_id: String,
    /// The drive's checkout.
    pub root: PathBuf,
    /// The agents zone's folder in the drive.
    pub agents: String,
    /// The sessions zone's folder in the drive.
    pub sessions: String,
    /// The home's folder in the agents zone.
    pub folder: String,
    /// As the host's scan admitted it; read again at the night's `HEAD`.
    pub config: AgentConfig,
    /// As the host admitted the drive; read again at the night's `HEAD`.
    pub decl: DriveDecl,
    /// This host's slug.
    pub host: String,
}

impl Home {
    /// `home` of the drive checked out by `profile`; `None` when the drive
    /// has no agents or sessions zone.
    pub fn of(profile: &SyncProfile, home: &AgentHome, host: &str) -> Option<Home> {
        Some(Home {
            drive: home.config.drive.clone(),
            profile_id: profile.id.clone(),
            root: profile.local_path.clone(),
            agents: profile.agents.as_ref()?.subfolder.clone(),
            sessions: profile.sessions.as_ref()?.subfolder.clone(),
            folder: home.dir.file_name()?.to_string_lossy().into_owned(),
            config: home.config.clone(),
            decl: home.drive.clone(),
            host: host.to_owned(),
        })
    }

    /// A path of the agents zone, drive-relative.
    pub(crate) fn in_agents(&self, rel: &str) -> String {
        format!("{}/{rel}", self.agents)
    }

    /// The session `path` (drive-relative) names, its facts.
    fn session(&self, path: &str) -> Option<SessionFacts> {
        crate::zone::session_facts(&self.root, &self.sessions, path)
    }
}

/// `rel`'s text in the commit at `HEAD`; `None` when it is not there.
pub(crate) fn at_head(root: &Path, rel: &str) -> Result<Option<String>, String> {
    text_at(root, "HEAD", rel)
}

/// `rel`'s text in the commit `rev` names; `None` when it is not there.
pub(crate) fn text_at(root: &Path, rev: &str, rel: &str) -> Result<Option<String>, String> {
    keeper_sync::git::history::blob_at(root, rev, rel)
        .map_err(|error| format!("{rel} could not be read at {rev}: {error}"))?
        .map(|bytes| String::from_utf8(bytes).map_err(|_| format!("{rel} is not text")))
        .transpose()
}

/// The commit at `HEAD`, hex.
pub(crate) fn head(root: &Path) -> Result<String, String> {
    let repo = keeper_sync::git::repo::open(root, false).map_err(|error| error.to_string())?;
    keeper_sync::git::repo::head_commit_id(&repo)
        .map_err(|error| error.to_string())?
        .map(|id| id.to_hex().to_string())
        .ok_or_else(|| "the drive has no commit yet".to_owned())
}

/// The names of the entries of `dir` on the disk that `keep` admits, sorted.
pub(crate) fn names(dir: &Path, keep: fn(&std::fs::DirEntry) -> bool) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(keep)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

pub(crate) fn is_dir(entry: &std::fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_dir())
}

pub(crate) fn is_file(entry: &std::fs::DirEntry) -> bool {
    entry.file_type().is_ok_and(|kind| kind.is_file())
}

/// Who a home answers to at the night's `HEAD`: its `agent.toml` and the
/// drive's `_drive.toml` read again after the pull — the drive's unchanged
/// from what this host admitted, the agent still the one it serves — and
/// their blob ids, which every commit of the night is guarded on, so an
/// edit to either before the commit writes nothing (R95C-14).
#[derive(Debug, Clone)]
pub struct Authority {
    pub config: AgentConfig,
    pub decl: DriveDecl,
    pub(crate) guards: Vec<(String, Option<String>)>,
}

impl Authority {
    /// Read at `HEAD` of `home`'s drive.
    pub fn at_head(home: &Home) -> Result<Authority, String> {
        Authority::at(home, "HEAD")
    }

    /// Read in the commit `rev` of `home`'s drive.
    pub fn at(home: &Home, rev: &str) -> Result<Authority, String> {
        let decl_path = home.in_agents(drive::FILE_NAME);
        let config_path = home.in_agents(&format!("{}/{}", home.folder, agent_home::FILE_NAME));
        let decl_text = text_at(&home.root, rev, &decl_path)?
            .ok_or_else(|| format!("{decl_path} is not in the drive"))?;
        let decl = drive::parse(&decl_text).map_err(|refusal| refusal.to_string())?;
        if decl != home.decl {
            return Err(format!(
                "{decl_path} changed since this host admitted the drive; its night waits for \
                 the host's next scan"
            ));
        }
        let config_text = text_at(&home.root, rev, &config_path)?
            .ok_or_else(|| format!("{config_path} is not in the drive"))?;
        let config = agent_home::parse_agent_toml(&config_text, &home.folder, &decl)
            .map_err(|refusal| refusal.to_string())?;
        if config.id != home.config.id
            || config.matrix_user != home.config.matrix_user
            || config.drive != home.config.drive
        {
            return Err(format!(
                "{config_path} names another agent than the one this host serves; its night \
                 waits for the host's next scan"
            ));
        }
        Ok(Authority {
            config,
            guards: vec![
                (decl_path, Some(blob_id(decl_text.as_bytes()))),
                (config_path, Some(blob_id(config_text.as_bytes()))),
            ],
            decl,
        })
    }
}

/// Where a host action stands, as its review session's log says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Undecided, or decided and not consumed yet.
    Open,
    /// Consumed: the consume-once authorization succeeded on a copy of the
    /// agent (R75), so the claim's holder may carry it out — once: the
    /// commit that does names the record ([`Waiting::carried`]).
    Consumed,
    /// A person declined it.
    Denied,
    /// Refused or expired: it ends with no effect.
    Ended,
}

/// The standing of each approval of the session at `dir`, by id.
fn standings(dir: &Path) -> BTreeMap<String, Standing> {
    let mut out: BTreeMap<String, Standing> = BTreeMap::new();
    for line in read_session(dir).lines {
        let LineBody::Approval(body) = &line.body else {
            continue;
        };
        let now = match body.state {
            ApprovalState::Consumed => Standing::Consumed,
            ApprovalState::Expired | ApprovalState::Refused => Standing::Ended,
            ApprovalState::Decided if body.is_terminal() => Standing::Denied,
            _ => continue,
        };
        let entry = out.entry(body.id.clone()).or_insert(Standing::Open);
        // A `consumed` from any copy is final; refused and expired end it;
        // a denial stands unless one of those came too.
        *entry = match (*entry, now) {
            (Standing::Consumed, _) | (_, Standing::Consumed) => Standing::Consumed,
            (Standing::Ended, _) | (_, Standing::Ended) => Standing::Ended,
            _ => now,
        };
    }
    out
}

/// Whether the drive's history holds the commit carrying out a decided
/// record (`Approval-Record`): every commit `HEAD` reaches, whatever its
/// date says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Application {
    /// It does: the decision is terminal, applied or rejected.
    Carried,
    /// It does not, or the record is not decided: nothing was carried out.
    Waiting,
    /// The history could not be read: nothing is carried out on it, and the
    /// record keeps its file and its proposals until it is known.
    Unknown,
}

/// A review record of the agent's, with the decision taken on it — one
/// whose digest is the record's, else none — and where it stands.
#[derive(Debug, Clone)]
pub struct Waiting {
    /// Its review session, drive-relative.
    pub session: String,
    pub record: approval::ApprovalRecord,
    pub args: ApplyArgs,
    pub decision: Option<approval::DecisionRecord>,
    pub standing: Standing,
    /// Whether the commit that carried its decision out is in the drive's
    /// history.
    pub carried: Application,
    /// Whether every proposal it was previewed from is still pending at
    /// `HEAD` — asked of an open or consumed record whose decision is not
    /// carried out; `true` for every other.
    pub whole: bool,
}

impl Waiting {
    /// Whether its change lost a proposal it was previewed from — taken
    /// back, or settled otherwise — before its decision was carried out:
    /// that change is never applied, whatever is decided, and it holds
    /// nothing, so what is still pending is previewed again.
    pub fn withdrawn(&self) -> bool {
        !self.whole
            && self.carried == Application::Waiting
            && matches!(self.standing, Standing::Open | Standing::Consumed)
    }

    /// Whether it still reserves its file: open and in time, or approved
    /// and not known to be carried out — and no proposal it was previewed
    /// from withdrawn ([`Self::withdrawn`]). Every terminal record —
    /// applied, denied, refused, expired — releases it, so the next
    /// proposal for the file is reviewed.
    pub fn holds_target(&self, now: DateTime<Utc>) -> bool {
        if self.withdrawn() {
            return false;
        }
        match self.standing {
            Standing::Open => self.record.expires() > now,
            Standing::Consumed => self.carried != Application::Carried,
            Standing::Denied | Standing::Ended => false,
        }
    }

    /// Whether it still holds its proposals: as its file, or denied and its
    /// rejection not known to be committed.
    fn holds_proposals(&self, now: DateTime<Utc>) -> bool {
        self.holds_target(now)
            || (self.standing == Standing::Denied && self.carried != Application::Carried)
    }

    /// Whether a decision was taken on it: consumed or denied.
    fn decided(&self) -> bool {
        matches!(self.standing, Standing::Consumed | Standing::Denied)
    }
}

/// The `Approval-Record` line of the commit carrying out record `id`.
fn carried_line(id: &str) -> String {
    MemoryTrailer::ApprovalRecord(id.to_owned())
        .line()
        .unwrap_or_default()
}

/// Every `memory_apply`/`skill_apply` record in `home`'s review sessions,
/// read through the protected store reader every approval reader uses (no
/// link followed), and only those that are this home's own: the record's
/// id its file's, its session the folder it is in — a review session of
/// this agent on this drive — its agent and drive this home's, every pin on
/// this drive, and its arguments ones this home's consolidator could write.
/// Whether each decided one was carried out is read in one walk of the
/// drive's whole history.
pub fn waiting(home: &Home) -> Vec<Waiting> {
    let active = home.root.join(&home.sessions).join(ACTIVE_DIR);
    let suffix = format!("-{REVIEW_SESSION_INFIX}-{}", home.config.id);
    let mut out = Vec::new();
    for name in names(&active, is_dir) {
        if !name.ends_with(&suffix) {
            continue;
        }
        let session = format!("{}/{ACTIVE_DIR}/{name}", home.sessions);
        let dir = active.join(&name);
        let owned = read_text(&home.root, &format!("{session}/{SESSION_FILE}"))
            .ok()
            .flatten()
            .and_then(|text| parse_session_agent_toml(&text).ok())
            .is_some_and(|agent| agent.agent == home.config.id && agent.drive == home.drive);
        if !owned {
            continue;
        }
        let standing = standings(&dir);
        for id in stored_ids(&dir) {
            let Ok((record, value)) = read_stored_record(&dir, &id) else {
                continue;
            };
            if !night::is_host_action(&record)
                || record.session != session
                || record.agent != home.config.id
                || record.drive != home.drive
                || record
                    .preconditions
                    .files
                    .iter()
                    .any(|pin| pin.drive != home.drive)
            {
                continue;
            }
            let Ok(args) = serde_json::from_value::<ApplyArgs>(value.clone()) else {
                continue;
            };
            if let Err(why) = args.belongs_to(&home.config.id, &home.folder, &record.action.tool) {
                tracing::warn!(approval = %id, %why, "agents: a review record is not this agent's; it is ignored");
                continue;
            }
            let digest = record.recomputed_digest(&value).ok();
            let decision = read_stored_decision(&dir, &id)
                .filter(|decision| digest.as_deref() == Some(decision.binding_digest.as_str()));
            let standing = standing.get(&id).copied().unwrap_or(Standing::Open);
            out.push(Waiting {
                session: session.clone(),
                standing,
                record,
                args,
                decision,
                carried: Application::Waiting,
                whole: true,
            });
        }
    }
    let lines: HashSet<String> = out
        .iter()
        .filter(|waiting| waiting.decided())
        .map(|waiting| carried_line(&waiting.record.id))
        .collect();
    let found = keeper_sync::git::history::lines_in_history(&home.root, &lines);
    if let Err(error) = &found {
        tracing::warn!(drive = %home.drive, %error, "agents: the drive's history could not be read; its decisions wait");
    }
    for waiting in out.iter_mut().filter(|waiting| waiting.decided()) {
        waiting.carried = match &found {
            Ok(found) if found.contains(&carried_line(&waiting.record.id)) => Application::Carried,
            Ok(_) => Application::Waiting,
            Err(_) => Application::Unknown,
        };
    }
    for waiting in out.iter_mut().filter(|waiting| {
        waiting.carried == Application::Waiting
            && matches!(waiting.standing, Standing::Open | Standing::Consumed)
    }) {
        let ids: Vec<Ulid> = waiting
            .args
            .proposal_ids()
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        // A `HEAD` that does not read withdraws nothing: the record keeps
        // holding, and carrying it out reads `HEAD` again.
        waiting.whole = !pending(home, &ids).is_ok_and(|left| left.len() < ids.len());
    }
    out
}

/// The proposals of `ids` still pending in `home` at `HEAD` — each one of
/// this agent's — with their blob ids.
fn pending(home: &Home, ids: &[Ulid]) -> Result<BTreeMap<Ulid, String>, String> {
    let mut out = BTreeMap::new();
    for id in ids {
        let (pending, _, _) = night::proposal_paths(&home.folder, id);
        if let Some(text) = at_head(&home.root, &home.in_agents(&pending))? {
            let ours = Proposal::parse(&id.to_string(), &text)
                .is_ok_and(|proposal| proposal.agent == home.config.id);
            if ours {
                out.insert(*id, blob_id(text.as_bytes()));
            }
        }
    }
    Ok(out)
}

/// The moves, verdicts and guards that settle `settled` over `commit`: the
/// pending proposal at the blob read, its `done/` place and its verdict
/// where nothing is — a file a person put there stays theirs, and the
/// commit is not made.
pub(crate) fn settle_into(
    home: &Home,
    settled: &[Settled],
    blobs: &BTreeMap<Ulid, String>,
    commit: &str,
    at: DateTime<Utc>,
    request: &mut CommitRequest,
) {
    for one in settled {
        let (pending, done, verdict) = night::proposal_paths(&home.folder, &one.id);
        let (pending, done) = (home.in_agents(&pending), home.in_agents(&done));
        request
            .guards
            .push((pending.clone(), blobs.get(&one.id).cloned()));
        request.guards.push((done.clone(), None));
        request.moves.push((pending, done));
        let verdict = home.in_agents(&verdict);
        request.guards.push((verdict.clone(), None));
        request.writes.push((
            verdict,
            Some(night::verdict_file(one, commit, at).render().into_bytes()),
        ));
    }
}

/// `change` (agents-zone-relative) into `request`, guarded on its bytes
/// before. A folder moved whole is guarded file by file as the commit
/// `rev` holds it — its `SKILL.md` on `change.before` — and its new place
/// where nothing is: a file of it added, changed or removed since, or one
/// at its new place, writes nothing (R208).
pub(crate) fn change_into(
    home: &Home,
    change: &FileChange,
    rev: &str,
    request: &mut CommitRequest,
) -> Result<(), String> {
    let path = home.in_agents(&change.path);
    let before = change
        .before
        .as_deref()
        .map(|text| blob_id(text.as_bytes()));
    match &change.after {
        After::Text(text) => {
            request.guards.push((path.clone(), before));
            request.writes.push((path, Some(text.clone().into_bytes())));
        }
        After::MovedTo(to) => {
            let skill = format!("{path}/SKILL.md");
            let files = keeper_sync::git::history::files_at(&home.root, rev, &path)
                .map_err(|error| format!("{path} could not be read at {rev}: {error}"))?;
            request.guards.extend(
                files
                    .into_iter()
                    .filter(|file| file.path != skill)
                    .map(|file| (file.path, Some(file.id))),
            );
            request.guards.push((skill, before));
            let to = home.in_agents(to);
            request.guards.push((to.clone(), None));
            request.moves.push((path, to));
        }
    }
    Ok(())
}

pub(crate) fn request(
    subject: String,
    origin: &str,
    sources: &BTreeSet<String>,
    authority: &Authority,
    approval: Option<&str>,
) -> CommitRequest {
    let mut trailers = vec![MemoryTrailer::MemoryOrigin(origin.to_owned())];
    trailers.extend(sources.iter().cloned().map(MemoryTrailer::SourceSession));
    trailers.extend(approval.map(|id| MemoryTrailer::ApprovalRecord(id.to_owned())));
    CommitRequest {
        subject,
        trailers,
        guards: authority.guards.clone(),
        ..CommitRequest::default()
    }
}

/// The commit rejecting `blobs`' proposals for `reason`, in `by`'s name,
/// carrying out the decision on `approval`.
#[allow(clippy::too_many_arguments)]
fn rejection(
    home: &Home,
    authority: &Authority,
    approval: &str,
    blobs: &BTreeMap<Ulid, String>,
    reason: &str,
    by: &str,
    commit: &str,
    now: DateTime<Utc>,
) -> CommitRequest {
    let origin = night::origin_of_host(&home.host);
    let settled: Vec<Settled> = blobs
        .keys()
        .map(|id| Settled {
            id: *id,
            verdict: Verdict::Rejected,
            reason: reason.to_owned(),
            decided_by: by.to_owned(),
        })
        .collect();
    let mut out = request(
        format!(
            "memory: {} — 0 promoted, {} rejected",
            home.config.id,
            settled.len()
        ),
        &origin,
        &BTreeSet::new(),
        authority,
        Some(approval),
    );
    settle_into(home, &settled, blobs, commit, now, &mut out);
    out
}

/// The commits that carry out the decisions people took on `home`'s
/// reviews, read at `HEAD` after the night's pull. Only a record whose
/// approval was consumed — the consume-once authorization every approval
/// passes — is applied, and only by its decision's approver, with the
/// approvers it binds still who the drive and the sessions make them and
/// its label still letting it reach the drive's readers; a denial rejects
/// the proposals in the person's name; anything else changes nothing. Each
/// commit names its record; a record the drive's history names already was
/// carried out, and is never again.
pub fn decided(home: &Home, now: DateTime<Utc>) -> Result<Vec<CommitRequest>, String> {
    let authority = Authority::at_head(home)?;
    let origin = night::origin_of_host(&home.host);
    let commit = head(&home.root)?;
    let facts = |path: &str| home.session(path);
    let mut out = Vec::new();
    for waiting in waiting(home) {
        if waiting.carried != Application::Waiting {
            continue;
        }
        let id = waiting.record.id.as_str();
        let wanted = match waiting.standing {
            Standing::Consumed => Decision::Approve,
            Standing::Denied => Decision::Deny,
            Standing::Open | Standing::Ended => continue,
        };
        let Some(decision) = waiting
            .decision
            .as_ref()
            .filter(|decision| decision.decision == wanted)
        else {
            tracing::warn!(approval = %waiting.record.id, "agents: a decided review has no decision of its kind beside it; nothing is carried out");
            continue;
        };
        let sources: BTreeSet<Ulid> = waiting.args.proposal_ids().into_iter().collect();
        let blobs = pending(home, &sources.iter().copied().collect::<Vec<_>>())?;
        if blobs.is_empty() {
            continue;
        }
        // An approved change is applied whole or not at all: a proposal it
        // was previewed from that is no longer pending — taken back since —
        // leaves it unapplied, and what is still pending is previewed again.
        if wanted == Decision::Approve && blobs.len() < sources.len() {
            tracing::info!(approval = %waiting.record.id, "agents: a proposal an approved change was previewed from was taken back; the change is not applied and the rest is reviewed again");
            continue;
        }
        let approvers =
            match waiting
                .args
                .approvers_now(&authority.decl.owner, &authority.decl.readers, &facts)
            {
                Ok(approvers) => approvers,
                Err(why) => {
                    out.push(rejection(
                        home, &authority, id, &blobs, &why, &origin, &commit, now,
                    ));
                    continue;
                }
            };
        let by = decision.decided_by.user.clone();
        if !OwnedUserId::try_from(by.as_str()).is_ok_and(|user| approvers.contains(&user)) {
            tracing::warn!(approval = %waiting.record.id, "agents: a review was decided by someone it does not name; nothing is carried out");
            continue;
        }
        if wanted == Decision::Deny {
            out.push(rejection(
                home,
                &authority,
                id,
                &blobs,
                "a person declined it on review",
                &by,
                &commit,
                now,
            ));
            continue;
        }
        if let Some(why) = sink_refusal(
            &waiting.args.label,
            &authority.decl.readers,
            authority.decl.local_only,
        ) {
            let reason = format!("its drive is read by more people than it may reach: {why}");
            out.push(rejection(
                home, &authority, id, &blobs, &reason, &origin, &commit, now,
            ));
            continue;
        }
        let settled: Vec<Settled> = blobs
            .keys()
            .map(|id| Settled {
                id: *id,
                verdict: Verdict::Promoted,
                reason: "a person approved it on review".to_owned(),
                decided_by: by.clone(),
            })
            .collect();
        let mut ask = request(
            format!(
                "memory: {} — {} promoted, 0 rejected",
                home.config.id,
                settled.len()
            ),
            &origin,
            &waiting.args.sessions.iter().cloned().collect(),
            &authority,
            Some(id),
        );
        change_into(home, &waiting.args.change, &commit, &mut ask)?;
        settle_into(home, &settled, &blobs, &commit, now, &mut ask);
        out.push(ask);
    }
    Ok(out)
}

/// What an approved change whose target moved since comes to: its
/// proposals rejected, nothing written over. `None` once none is pending,
/// or once one of them is not: a change that lost a proposal is reviewed
/// again ([`Waiting::withdrawn`]), never settled without it.
pub fn changed_since_approved(
    home: &Home,
    ask: &CommitRequest,
    now: DateTime<Utc>,
) -> Result<Option<CommitRequest>, String> {
    let authority = Authority::at_head(home)?;
    let commit = head(&home.root)?;
    let origin = night::origin_of_host(&home.host);
    let prefix = home.in_agents(&format!("{}/{}/", home.folder, proposal::DIR));
    let ids: Vec<Ulid> = ask
        .moves
        .iter()
        .filter_map(|(from, _)| from.strip_prefix(&prefix)?.strip_suffix(".md"))
        .filter_map(|id| Ulid::from_string(id).ok())
        .collect();
    let blobs = pending(home, &ids)?;
    if blobs.is_empty() || blobs.len() < ids.len() {
        return Ok(None);
    }
    let approval = ask
        .trailers
        .iter()
        .find_map(|trailer| match trailer {
            MemoryTrailer::ApprovalRecord(id) => Some(id.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    Ok(Some(rejection(
        home,
        &authority,
        approval,
        &blobs,
        "the file it changes changed after it was approved; nothing was written over",
        &origin,
        &commit,
        now,
    )))
}

/// One agent's night, read and planned.
#[derive(Debug, Clone)]
pub struct Planned {
    pub plan: Plan,
    /// Who the agent answered to, as the plan read it.
    pub authority: Authority,
    /// The commit the plan read.
    pub commit: String,
    /// The writes, moves and guards of what the night settles and applies.
    pub request: CommitRequest,
}

impl Planned {
    /// Whether a person is told anything: a change waits, or the night
    /// left a file alone and says why.
    pub fn needs_review(&self) -> bool {
        !self.plan.reviews.is_empty() || !self.plan.notes.is_empty()
    }
}

/// Read `home`'s files at `HEAD` — its declarations among them — and plan
/// its night at `now`. `drive` is every home of the drive: a skill is the
/// drive's, so another agent's open review of one holds it here too.
pub fn plan_home(home: &Home, drive: &[Home], now: DateTime<Utc>) -> Result<Planned, String> {
    let authority = Authority::at_head(home)?;
    let commit = head(&home.root)?;
    let read = |rel: &str| at_head(&home.root, &home.in_agents(rel));
    let user = read(&format!("{}/USER.md", home.folder))?;
    let memory = read(&format!("{}/MEMORY.md", home.folder))?;
    let mut skills = BTreeMap::new();
    for name in names(&home.root.join(&home.agents).join("_skills"), is_dir) {
        if name.starts_with('.') {
            continue;
        }
        if let Some(text) = read(&format!("_skills/{name}/SKILL.md"))? {
            skills.insert(name, text);
        }
    }
    let proposals_dir = home
        .root
        .join(&home.agents)
        .join(&home.folder)
        .join(proposal::DIR);
    let mut proposals = Vec::new();
    let mut blobs = BTreeMap::new();
    for name in names(&proposals_dir, is_file) {
        let Some(stem) = name.strip_suffix(".md") else {
            continue;
        };
        let Ok(id) = Ulid::from_string(stem) else {
            continue;
        };
        // A proposal not committed yet waits for the next night.
        if let Some(text) = read(&format!("{}/{}/{name}", home.folder, proposal::DIR))? {
            blobs.insert(id, blob_id(text.as_bytes()));
            proposals.push((stem.to_owned(), text));
        }
    }
    let own = waiting(home);
    let under_review: BTreeSet<Ulid> = own
        .iter()
        .filter(|waiting| waiting.holds_proposals(now))
        .flat_map(|waiting| waiting.args.proposal_ids())
        .collect();
    let mut reviewing: BTreeSet<String> = own
        .iter()
        .filter(|waiting| waiting.holds_target(now))
        .map(|waiting| waiting.args.change.path.clone())
        .collect();
    for other in drive
        .iter()
        .filter(|other| other.config.id != home.config.id)
    {
        reviewing.extend(
            waiting(other)
                .into_iter()
                .filter(|waiting| waiting.holds_target(now))
                .map(|waiting| waiting.args.change.path)
                .filter(|path| path.starts_with("_skills/")),
        );
    }
    let facts = |path: &str| home.session(path);
    let decided_by = night::origin_of_host(&home.host);
    let plan = night::plan(&NightInput {
        agent: &home.config.id,
        home: &home.folder,
        decided_by: &decided_by,
        owner: &authority.decl.owner,
        readers: &authority.decl.readers,
        local_only: authority.decl.local_only,
        promote: authority.config.memory.promote,
        user: user.as_deref(),
        memory: memory.as_deref(),
        skills: &skills,
        proposals: &proposals,
        under_review: &under_review,
        reviewing: &reviewing,
        declined: &BTreeMap::new(),
        sessions: &facts,
        now,
    });
    let mut out = request(
        plan.subject(&home.config.id),
        &decided_by,
        &plan.sources,
        &authority,
        None,
    );
    for change in &plan.writes {
        change_into(home, change, &commit, &mut out)?;
    }
    settle_into(home, &plan.settled, &blobs, &commit, now, &mut out);
    Ok(Planned {
        plan,
        authority,
        commit,
        request: out,
    })
}

/// The review session of `home` for the night `window`, drive-relative.
fn review_session(home: &Home, window: DateTime<Utc>) -> String {
    format!(
        "{}/{ACTIVE_DIR}/{}",
        home.sessions,
        night::review_session_name(&home.config.id, window)
    )
}

/// The room of `home`'s review session for the night `window`, when that
/// session is committed already: a night run again reuses it.
pub fn review_room_of(home: &Home, window: DateTime<Utc>) -> Result<Option<OwnedRoomId>, String> {
    let rel = format!("{}/{SESSION_FILE}", review_session(home, window));
    let Some(text) = at_head(&home.root, &rel)? else {
        return Ok(None);
    };
    let agent = parse_session_agent_toml(&text).map_err(|refusal| refusal.to_string())?;
    if agent.agent != home.config.id || agent.drive != home.drive {
        return Err(format!("{rel} is not this agent's review session"));
    }
    Ok(Some(agent.room))
}

/// The review session `planned` opens — or adds to — for `home` on the
/// night `window`, in `room`, in the night's commit. A new session gets its
/// `agent.toml`; one committed already keeps its own, guarded, and its
/// earlier previews and records are never written again. Per change that
/// waits: one before/after preview and one T2 record (R128), both named by
/// the record's id and written only where nothing is. The card lists every
/// preview of the session.
pub fn add_review(
    home: &Home,
    planned: &mut Planned,
    window: DateTime<Utc>,
    room: &OwnedRoomId,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let session = review_session(home, window);
    let agent_toml = format!("{session}/{SESSION_FILE}");
    let mut files = Vec::new();
    match at_head(&home.root, &agent_toml)? {
        Some(text) => planned
            .request
            .guards
            .push((agent_toml.clone(), Some(blob_id(text.as_bytes())))),
        None => {
            let agent = SessionAgent {
                id: night::review_session_id(&home.drive, &home.config.id, window),
                agent: home.config.id.clone(),
                drive: home.drive.clone(),
                kind: SessionKind::Scheduled,
                title: format!("Memory review — {}", home.config.name),
                requested_by: home.decl.owner.clone(),
                parent: None,
                room: room.clone(),
                drives: vec![home.drive.clone()],
                label: Label::opening(&home.decl, Integrity::Owner),
                needs: None,
                pin: None,
                hop: 0,
                dispatch_chain: vec![home.decl.owner.clone(), home.config.matrix_user.clone()],
                limits: None,
                workflow: None,
                checkpoints: None,
                outputs: Vec::new(),
                created_at: now,
            };
            files.push((SESSION_FILE.to_owned(), compose_session_agent_toml(&agent)));
        }
    }
    let mut previews: Vec<String> = names(&home.root.join(&session).join("artifacts"), is_file)
        .into_iter()
        .filter(|name| name.starts_with("memory-review-") && name.ends_with(".md"))
        .map(|name| format!("artifacts/{name}"))
        .collect();
    for review in &planned.plan.reviews {
        // Every proposal the preview is made from is guarded in the commit
        // that publishes it: one taken back by that commit's re-read, right
        // before its publication, publishes no review (not one step: DW-901).
        let sources = pending(home, &review.proposals)?;
        if sources.len() < review.proposals.len() {
            return Err(format!(
                "a proposal {}'s review is made from is no longer pending; it is reviewed again",
                review.change.path
            ));
        }
        for (source, blob) in sources {
            let (path, _, _) = night::proposal_paths(&home.folder, &source);
            planned
                .request
                .guards
                .push((home.in_agents(&path), Some(blob)));
        }
        let id = Ulid::new();
        let preview = format!("artifacts/memory-review-{id}.md");
        let text = night::preview(&home.config.id, review);
        let args = ApplyArgs::of(&home.config.id, &home.folder, review, &preview, &text);
        let pinned = match &review.change.after {
            After::MovedTo(_) => format!("{}/SKILL.md", review.change.path),
            After::Text(_) => review.change.path.clone(),
        };
        let pinned = home.in_agents(&pinned);
        let pin = FilePin {
            drive: home.drive.clone(),
            path: pinned.clone(),
            landing: Some(pinned),
            sha256: review
                .change
                .before
                .as_deref()
                .map(|text| night::sha256_hex(text.as_bytes())),
        };
        let mut record =
            night::review_record(&id, now, &session, &home.drive, &home.host, &args, pin)?;
        if let Some((sha, bytes)) = record.externalise_args() {
            files.push((format!("{APPROVALS_DIR}/blobs/{sha}.json"), bytes));
        }
        let record = serde_json::to_string_pretty(&record).map_err(|error| error.to_string())?;
        files.push((format!("{APPROVALS_DIR}/{id}.json"), record));
        files.push((preview.clone(), text));
        previews.push(preview);
    }
    for (rel, text) in files {
        let path = format!("{session}/{rel}");
        planned.request.guards.push((path.clone(), None));
        planned.request.writes.push((path, Some(text.into_bytes())));
    }
    let card = format!("{session}/{REVIEW_CARD}");
    let card_before = at_head(&home.root, &card)?.map(|text| blob_id(text.as_bytes()));
    planned.request.guards.push((card.clone(), card_before));
    planned.request.writes.push((
        card,
        Some(
            format!(
                "---\ntags: [task]\ntitle: \"Memory review: {}\"\nstatus: todo\nassignee: {}\n---\n\n{}",
                home.config.name,
                home.config.id,
                night::review_card(&home.config.id, &previews, &planned.plan.notes)
            )
            .into_bytes(),
        ),
    ));
    Ok(())
}

/// What one agent's night did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Its commit, when it made one.
    Committed(Option<String>),
    /// A guarded file changed since the plan's read: nothing of this agent
    /// was written tonight.
    Skipped { path: String },
    /// The lease no longer let this host write: nothing more was.
    Fenced,
}

/// Whether who `home` answers to is still what `planned` read — at `HEAD`
/// and on the disk — and every change waiting for a person may still reach
/// the drive's readers: asked right before a review room is made, whose
/// invitations go to those readers (R95CR-11, R95C-03). The path that
/// changed, or why it may not reach them, when not.
fn authority_holds(home: &Home, planned: &Planned) -> Result<(), String> {
    let now = Authority::at_head(home)?;
    for ((path, read), (_, at_head)) in planned.authority.guards.iter().zip(&now.guards) {
        let on_disk = std::fs::read(home.root.join(path))
            .ok()
            .map(|bytes| blob_id(&bytes));
        if at_head != read || on_disk.as_ref() != read.as_ref() {
            return Err(path.clone());
        }
    }
    planned
        .plan
        .reviews
        .iter()
        .find_map(|review: &Review| {
            sink_refusal(&review.label, &now.decl.readers, now.decl.local_only)
        })
        .map_or(Ok(()), Err)
}

/// `work` on a blocking thread, off the task that polls the night: the
/// claim's renewal never waits for the disk, Git or a pull.
async fn off_the_task<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}

/// The night's pull of the drive `profile_id` — the settling of a commit a
/// kill left, the walk, the fetch and the merge — on a blocking thread off
/// the task that polls the night, whatever the runtime.
pub(crate) async fn pull(engine: &Arc<Engine>, profile_id: &str) -> Result<(), String> {
    let (engine, id) = (Arc::clone(engine), profile_id.to_owned());
    let runtime = tokio::runtime::Handle::current();
    off_the_task(move || {
        runtime
            .block_on(engine.sync_once(&id, keeper_sync::SyncSource::Bot))
            .map(drop)
            .map_err(|error| error.to_string())
    })
    .await?
}

/// Run `home`'s night at `now` on the night `window`: the decisions people
/// took first, then the plan, written through `engine`. `drive` is every
/// home of the drive. `room` makes the review session's room when anything
/// waits for a person and the night's session has none yet — once `fence`
/// says the claim still lets this host write, and who the agent answers to
/// is still what the plan read. Every commit asks `fence` right before it
/// is published.
pub async fn run_home<F, Fut>(
    engine: &Arc<Engine>,
    home: &Home,
    drive: &[Home],
    window: DateTime<Utc>,
    now: DateTime<Utc>,
    room: F,
    fence: &Fence,
) -> Result<Outcome, String>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<OwnedRoomId, String>>,
{
    if settle_decided(engine, home, now, fence).await? == Outcome::Fenced {
        return Ok(Outcome::Fenced);
    }
    let (mut planned, existing) = {
        let (home, drive) = (home.clone(), drive.to_vec());
        off_the_task(move || {
            let planned = plan_home(&home, &drive, now)?;
            let existing = review_room_of(&home, window)?;
            Ok::<_, String>((planned, existing))
        })
        .await??
    };
    if planned.needs_review() {
        let room = match existing {
            Some(room) => room,
            None if !fence() => return Ok(Outcome::Fenced),
            None => {
                let holds = {
                    let (home, planned) = (home.clone(), planned.clone());
                    off_the_task(move || authority_holds(&home, &planned)).await?
                };
                if let Err(path) = holds {
                    tracing::info!(agent = %home.config.id, %path, "agents: who the agent answers to changed since the night read it; nothing of it is written tonight");
                    return Ok(Outcome::Skipped { path });
                }
                // Asked again: the claim may have lapsed while the check
                // waited for the disk and Git.
                if !fence() {
                    return Ok(Outcome::Fenced);
                }
                room().await?
            }
        };
        planned = {
            let home = home.clone();
            off_the_task(move || {
                add_review(&home, &mut planned, window, &room, now)?;
                Ok::<_, String>(planned)
            })
            .await??
        };
    }
    if planned.request.writes.is_empty() && planned.request.moves.is_empty() {
        return Ok(Outcome::Committed(None));
    }
    match engine
        .commit_paths(&home.profile_id, &planned.request, Arc::clone(fence))
        .await
        .map_err(|error| error.to_string())?
    {
        CommitPaths::Committed { commit } => Ok(Outcome::Committed(Some(commit))),
        CommitPaths::Unchanged => Ok(Outcome::Committed(None)),
        CommitPaths::Guarded { path } => {
            tracing::info!(agent = %home.config.id, %path, "agents: a memory file changed since the night read it; nothing of this agent is written tonight");
            Ok(Outcome::Skipped { path })
        }
        CommitPaths::Fenced => Ok(Outcome::Fenced),
    }
}

/// Carry out the decisions people took on `home`'s reviews, each commit
/// fenced: [`Outcome::Fenced`] when the claim stopped one, else
/// `Committed(None)`.
pub async fn settle_decided(
    engine: &Arc<Engine>,
    home: &Home,
    now: DateTime<Utc>,
    fence: &Fence,
) -> Result<Outcome, String> {
    let asks = {
        let home = home.clone();
        off_the_task(move || decided(&home, now)).await??
    };
    for ask in asks {
        let done = engine
            .commit_paths(&home.profile_id, &ask, Arc::clone(fence))
            .await
            .map_err(|error| error.to_string())?;
        match done {
            CommitPaths::Fenced => return Ok(Outcome::Fenced),
            CommitPaths::Guarded { path } => {
                tracing::info!(agent = %home.config.id, %path, "agents: an approved memory change's file changed since; its proposals are rejected");
                let reject = {
                    let (home, ask) = (home.clone(), ask.clone());
                    off_the_task(move || changed_since_approved(&home, &ask, now)).await??
                };
                if let Some(reject) = reject {
                    if engine
                        .commit_paths(&home.profile_id, &reject, Arc::clone(fence))
                        .await
                        .map_err(|error| error.to_string())?
                        == CommitPaths::Fenced
                    {
                        return Ok(Outcome::Fenced);
                    }
                }
            }
            CommitPaths::Committed { .. } | CommitPaths::Unchanged => {}
        }
    }
    Ok(Outcome::Committed(None))
}

/// Whether any of `homes` has a decision waiting to be carried out: a
/// consumed approval or a denial the history does not name — or cannot be
/// read to name — whose proposals are still pending; an approval whose
/// change lost one of them is never carried out ([`Waiting::withdrawn`]).
pub fn any_decided(homes: &[Home]) -> bool {
    homes.iter().any(|home| {
        waiting(home).iter().any(|waiting| {
            waiting.decided()
                && waiting.carried != Application::Carried
                && waiting.decision.is_some()
                && !waiting.withdrawn()
                && pending(home, &waiting.args.proposal_ids()).is_ok_and(|left| !left.is_empty())
        })
    })
}

/// Whether everything the nights of `homes` began is finished: every
/// commit published on their drive followed by its files, and no decision
/// a person took waiting to be carried out ([`any_decided`]). A window is
/// complete only then.
pub async fn settled(engine: &Arc<Engine>, homes: &[Home]) -> Result<bool, String> {
    let (engine, homes) = (Arc::clone(engine), homes.to_vec());
    off_the_task(move || {
        let profiles: BTreeSet<&str> = homes.iter().map(|home| home.profile_id.as_str()).collect();
        for profile in profiles {
            if engine
                .unsettled_commit(profile)
                .map_err(|error| error.to_string())?
            {
                return Ok(false);
            }
        }
        Ok(!any_decided(&homes))
    })
    .await?
}

/// One agent of a drive this host consolidates, with the copy serving it:
/// its review room is made as that agent.
#[derive(Clone)]
pub(crate) struct NightHome {
    pub(crate) home: Home,
    pub(crate) copy: Arc<dyn crate::hosts::CopyPort>,
}

/// One drive this host consolidates: every agent homed there — this host
/// serves them all — and the copy that holds the lease for it.
#[derive(Clone)]
pub(crate) struct NightDrive {
    pub(crate) id: String,
    pub(crate) homes: Vec<NightHome>,
    pub(crate) copy: Arc<dyn crate::hosts::CopyPort>,
}

impl NightDrive {
    pub(crate) fn homes(&self) -> Vec<Home> {
        self.homes.iter().map(|one| one.home.clone()).collect()
    }
}

/// What a night needs of its host, owned: it runs off the host's tick.
#[derive(Clone)]
pub(crate) struct NightRound {
    pub(crate) me: keeper_core::agents::log::HostSlug,
    pub(crate) control: OwnedRoomId,
    pub(crate) clock: Arc<ServerClock>,
    pub(crate) rtt: Arc<Rtt>,
    /// The server's time now, and this machine's offset then.
    pub(crate) now_ms: i64,
    pub(crate) offset: i32,
    pub(crate) drives: Vec<NightDrive>,
}

/// What a host remembers of a drive's night.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Remembered {
    /// The night of this window is done: every agent settled, and the
    /// release naming it was accepted.
    Done(i64),
    /// Not done — held elsewhere, failed, or stopped: tried again at `at`.
    Again(tokio::time::Instant),
}

/// How long a drive whose night is held elsewhere or did not finish waits
/// before this host tries again: a lapsed holder's claim is free by then.
pub(crate) const AGAIN: Duration = claim::TTL;

/// A host's nights: one run in flight, started from the tick and never
/// waited on by it; what each drive's night came to remembered, so a night
/// done costs no request until the next and one not done is tried again.
pub(crate) struct Consolidator {
    engine: Arc<Engine>,
    running: Option<tokio::task::JoinHandle<()>>,
    remembered: Arc<Mutex<BTreeMap<String, Remembered>>>,
}

/// Whether `drive` is owed a run in `window` by what is `remembered`.
pub(crate) fn owed(
    remembered: &BTreeMap<String, Remembered>,
    drive: &NightDrive,
    window: i64,
) -> bool {
    match remembered.get(&drive.id) {
        Some(Remembered::Done(done)) if *done == window => any_decided(&drive.homes()),
        Some(Remembered::Again(at)) => tokio::time::Instant::now() >= *at,
        _ => true,
    }
}

impl Consolidator {
    pub(crate) fn new(engine: Arc<Engine>) -> Consolidator {
        Consolidator {
            engine,
            running: None,
            remembered: Arc::default(),
        }
    }

    /// Start a run when a drive of `round` is owed its night, or has a
    /// person's decision to carry out, and none runs.
    pub(crate) fn tick(&mut self, round: Option<NightRound>) {
        if self
            .running
            .as_ref()
            .is_some_and(|task| !task.is_finished())
        {
            return;
        }
        let Some(round) = round else {
            return;
        };
        let Some(window) = night_window(round.now_ms, round.offset) else {
            return;
        };
        let any = {
            let remembered = self.remembered.lock().unwrap_or_else(|p| p.into_inner());
            round
                .drives
                .iter()
                .any(|drive| owed(&remembered, drive, window))
        };
        if any {
            self.running = Some(tokio::spawn(run_round(
                Arc::clone(&self.engine),
                round,
                window,
                Arc::clone(&self.remembered),
            )));
        }
    }
}

/// Each drive of `round` owed a run: its night `window` under the drive's
/// maintenance claim, or a person's decisions carried out under it; what
/// each came to goes to `remembered` — done only for a run the claim's
/// helper says recorded ([`Maintained::Ran`]).
pub(crate) async fn run_round(
    engine: Arc<Engine>,
    round: NightRound,
    window: i64,
    remembered: Arc<Mutex<BTreeMap<String, Remembered>>>,
) {
    let window_at = DateTime::<Utc>::from_timestamp_millis(window).unwrap_or_default();
    let remember = |drive: &str, what: Remembered| {
        remembered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(drive.to_owned(), what);
    };
    let again = || Remembered::Again(tokio::time::Instant::now() + AGAIN);
    for drive in &round.drives {
        let due = owed(
            &remembered.lock().unwrap_or_else(|p| p.into_inner()),
            drive,
            window,
        );
        if !due {
            continue;
        }
        let me = crate::hosts::claimant(&round.me, drive.copy.as_ref());
        let (lease_key, done_key) = (
            claim::maintenance_key(&drive.id),
            claim::completion_key(night::JOB, &drive.id),
        );
        let lease_port = drive.copy.keyed_claims(&round.control, &lease_key);
        let done_port = drive.copy.keyed_claims(&round.control, &done_key);
        let now = Utc::now();
        let homes = drive.homes();
        // A night already done still carries out a decision taken since,
        // under the same claim, recording no night.
        let night = remembered
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&drive.id)
            .is_none_or(|was| *was != Remembered::Done(window));
        let ran = maintain(
            &lease_port,
            done_port.as_ref(),
            &me,
            &round.clock,
            &round.rtt,
            night.then_some(window),
            |fence| night_of(&engine, drive, &homes, night, window_at, now, fence),
        )
        .await;
        match ran {
            Ok(Maintained::Done) => {
                remember(&drive.id, Remembered::Done(window));
                if any_decided(&homes) {
                    // Done elsewhere, and a decision waits: the next tick
                    // carries it out under the claim.
                    tracing::info!(drive = %drive.id, "agents: the night of {} ran; a decision since waits for the claim", drive.id);
                }
            }
            Ok(Maintained::HeldBy(host)) => {
                tracing::info!(drive = %drive.id, "agents: the maintenance of {} is held by {host}", drive.id);
                remember(&drive.id, again());
            }
            Ok(Maintained::Ran { recorded, .. }) => {
                if recorded {
                    remember(&drive.id, Remembered::Done(window));
                } else {
                    tracing::warn!(drive = %drive.id, "agents: the night of {} did not finish; it is tried again", drive.id);
                    remember(&drive.id, again());
                }
            }
            Err(error) => {
                tracing::warn!(drive = %drive.id, %error, "agents: the drive's maintenance claim could not be read");
                remember(&drive.id, again());
            }
        }
    }
}

/// The work of one drive's night under its claim: the pull, then each
/// agent's night — or, `night` false, only the decisions people took.
/// Whether every agent of it settled and everything it began is finished
/// ([`settled`]).
async fn night_of(
    engine: &Arc<Engine>,
    drive: &NightDrive,
    homes: &[Home],
    night: bool,
    window: DateTime<Utc>,
    now: DateTime<Utc>,
    fence: Fence,
) -> ((), bool) {
    let Some(first) = drive.homes.first() else {
        return ((), true);
    };
    if let Err(error) = pull(engine, &first.home.profile_id).await {
        tracing::warn!(drive = %drive.id, %error, "agents: the drive could not be pulled; its night waits");
        return ((), false);
    }
    let mut whole = true;
    for one in &drive.homes {
        // A holder that cannot renew stops before its next agent.
        if !fence() {
            return ((), false);
        }
        let outcome = if night {
            let copy = Arc::clone(&one.copy);
            let room = || async move { copy.review_room().await };
            run_home(engine, &one.home, homes, window, now, room, &fence).await
        } else {
            settle_decided(engine, &one.home, now, &fence).await
        };
        match outcome {
            Ok(Outcome::Fenced) => {
                tracing::warn!(drive = %drive.id, "agents: the maintenance claim was lost; this host writes nothing more tonight");
                return ((), false);
            }
            Ok(outcome) => {
                tracing::info!(agent = %one.home.config.id, ?outcome, "agents: a night of memory consolidation")
            }
            Err(error) => {
                tracing::warn!(agent = %one.home.config.id, %error, "agents: this agent's night failed; it is tried again");
                whole = false;
            }
        }
    }
    if !whole {
        return ((), false);
    }
    match settled(engine, homes).await {
        Ok(true) => ((), true),
        Ok(false) => {
            tracing::warn!(drive = %drive.id, "agents: a commit or a decision of the night of {} is not finished; it is tried again", drive.id);
            ((), false)
        }
        Err(error) => {
            tracing::warn!(drive = %drive.id, %error, "agents: whether the night of {} finished could not be read; it is tried again", drive.id);
            ((), false)
        }
    }
}
