//! The weekly curator on an always-on host (AD-402, story 95.3).
//!
//! Once a week — `0 4 * * 0` at the host's offset, the latest such instant
//! at or before now being the sweep's window — the host that holds the
//! drive's one maintenance claim `maintain:<drive>` ([`crate::maintain`],
//! the night's claim too, so the two never run on one drive at once), the
//! week's completion under `curate:<drive>` not naming that window yet,
//! pulls the drive and reads it at the one commit the pull left: every
//! skill under `_skills/` with its last change from git (the curator's own
//! commits, known by keeper's own `Memory-Origin` trailer, left out), the
//! names its agents' `[tools].skills` and its `_workflows/` files give,
//! the drive's and the agents' declarations, and the pending proposals of
//! the homes this host serves — all from git's objects, so no link on the
//! disk leads a read anywhere. [`keeper_core::agents::curate`] decides;
//! what it answers is one `Engine::commit_paths` carrying
//! `Memory-Origin: curator@<host>`: the stale marks set or cleared, the
//! folders moved whole to `_skills/.archive/`, the expired gate proposals
//! moved to `proposals/done/` beside their verdicts — every path it
//! touches, every file a folder holds and every declaration and name it
//! read guarded by the blob it was planned over, its publication fenced on
//! the claim. Nothing is deleted, nothing composed; the push rings the
//! doorbell as any push does.
//!
//! A week is remembered done only once its sweep settled — its commit, if
//! any, published and its files followed — its completion was recorded and
//! the claim's release accepted; a claim held elsewhere, a failed pull or
//! sweep, a sweep held at its publication by something it read changing,
//! or a lost claim is tried again once the claim could have lapsed. A host
//! that missed any number of Sundays sweeps once.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use keeper_core::agents::claim;
use keeper_core::agents::consolidate::{proposal_paths, Settled};
use keeper_core::agents::curate::{self, Skill, Sweep, SweepPlan};
use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::home::{parse_agent_toml, FILE_NAME as AGENT_FILE};
use keeper_core::agents::proposal;
use keeper_core::agents::zone::is_zone_own;
use keeper_sync::git::history::{self, TreeFile};
use keeper_sync::{blob_id, CommitPaths, CommitRequest, Engine, MemoryTrailer};
use ulid::Ulid;

use crate::claims::ServerClock;
use crate::consolidate::{
    change_into, head, latest_window, pull, request, settle_into, text_at, Authority, Fence, Home,
    NightDrive, NightRound, Outcome, Remembered, AGAIN,
};
use crate::maintain::{maintain, Maintained};

/// How many of the newest commits that changed a skill's folder are read
/// for its last change; past them its age is not known.
const LOG_DEPTH: usize = 32;
/// The most `_workflows/` files one sweep reads for skill names.
const WORKFLOW_FILES: usize = 2_000;
/// A `_workflows/` file larger than this is not read for skill names.
const WORKFLOW_FILE_BYTES: u64 = 256 * 1024;

/// The sweep's window at `now_ms` read at `offset` minutes: the latest
/// [`curate::SCHEDULE`] instant at or before now, epoch ms.
pub fn sweep_window(now_ms: i64, offset: i32) -> Option<i64> {
    latest_window(curate::SCHEDULE, 8, now_ms, offset)
}

/// Whether `message` is a curator's commit: keeper's own trailer block
/// names a curator's `Memory-Origin` — never a line a body quotes.
fn by_curator(message: &str) -> bool {
    MemoryTrailer::of_message(message)
        .iter()
        .any(|trailer| matches!(trailer, MemoryTrailer::MemoryOrigin(origin) if curate::is_curator_origin(origin)))
}

/// A skill folder's last change in `rev`'s history: its newest commit
/// that is not the curator's; `None` when the [`LOG_DEPTH`] newest that
/// changed it are all the curator's.
fn last_change(
    root: &std::path::Path,
    rev: &str,
    rel: &str,
) -> Result<Option<DateTime<Utc>>, String> {
    let log = history::path_changes(root, rev, rel, LOG_DEPTH)
        .map_err(|error| format!("{rel}'s history could not be read: {error}"))?;
    Ok(log
        .iter()
        .find(|revision| !by_curator(&revision.message))
        .and_then(|revision| DateTime::from_timestamp(revision.committed_secs, 0)))
}

/// The names skills are protected by, read at the sweep's commit, the
/// blobs they were read from, and why they are not all of them when not.
#[derive(Default)]
struct Protection {
    listed: BTreeSet<String>,
    workflows: Vec<String>,
    incomplete: Option<String>,
    guards: Vec<(String, Option<String>)>,
}

/// Read the names `zone`'s agents and workflows give in the commit `rev`
/// of `home`'s drive, their declarations read through `decl`. `dirty` is
/// what the disk holds not committed: a change there to a workflow or an
/// agent's `agent.toml` may name a skill the commit does not, so the read
/// is incomplete. So it is past [`WORKFLOW_FILES`], for a workflow larger
/// than [`WORKFLOW_FILE_BYTES`], one that is a link or not text, and an
/// `agent.toml` that does not parse.
fn protection(
    home: &Home,
    decl: &DriveDecl,
    rev: &str,
    zone: &[TreeFile],
    dirty: &HashSet<String>,
) -> Result<Protection, String> {
    let mut out = Protection::default();
    let workflows = home.in_agents("_workflows");
    let agent_file = |rel: &str| -> Option<String> {
        let rest = rel.strip_prefix(&format!("{}/", home.agents))?;
        let (folder, file) = rest.split_once('/').unwrap_or((rest, ""));
        (!is_zone_own(folder)
            && !folder.starts_with('.')
            && (file.is_empty() || file == AGENT_FILE))
            .then(|| folder.to_owned())
    };
    let mut changed: Vec<&String> = dirty
        .iter()
        .filter(|path| path.starts_with(&workflows) || agent_file(path).is_some())
        .collect();
    changed.sort();
    if let Some(path) = changed.first() {
        out.incomplete = Some(format!("{path} holds a change not committed yet"));
        return Ok(out);
    }
    let flows: Vec<&TreeFile> = zone
        .iter()
        .filter(|file| file.path == workflows || file.path.starts_with(&format!("{workflows}/")))
        .collect();
    if flows.len() > WORKFLOW_FILES {
        out.incomplete = Some(format!(
            "{workflows} holds more than {WORKFLOW_FILES} files"
        ));
        return Ok(out);
    }
    for file in flows {
        if !file.regular {
            out.incomplete = Some(format!("{} is not a plain file", file.path));
            return Ok(out);
        }
        if file.size > WORKFLOW_FILE_BYTES {
            out.incomplete = Some(format!(
                "{} is larger than {WORKFLOW_FILE_BYTES} bytes",
                file.path
            ));
            return Ok(out);
        }
        let Ok(Some(text)) = text_at(&home.root, rev, &file.path) else {
            out.incomplete = Some(format!("{} could not be read as text", file.path));
            return Ok(out);
        };
        out.guards.push((file.path.clone(), Some(file.id.clone())));
        out.workflows.push(text);
    }
    for file in zone {
        let Some(folder) = agent_file(&file.path) else {
            continue;
        };
        if !file.path.ends_with(&format!("/{AGENT_FILE}")) {
            continue;
        }
        let text = text_at(&home.root, rev, &file.path)?.unwrap_or_default();
        match parse_agent_toml(&text, &folder, decl) {
            Ok(config) => out
                .listed
                .extend(config.skills.into_iter().filter(|name| name != "*")),
            Err(refusal) => {
                out.incomplete = Some(format!("{} does not parse: {refusal}", file.path));
                return Ok(out);
            }
        }
        out.guards.push((file.path.clone(), Some(file.id.clone())));
    }
    Ok(out)
}

/// One drive's sweep, read and planned.
#[derive(Debug, Clone)]
pub struct Swept {
    pub plan: SweepPlan,
    /// The writes, moves and guards of what the sweep changes.
    pub request: CommitRequest,
    /// The commit it was read at.
    pub commit: String,
}

/// Read the drive `homes` are homed in at the commit `HEAD` names now —
/// every read at that one commit — and plan its sweep at `now`; the
/// proposals read are those of `homes`, whose declarations must still be
/// what this host admitted. `None` without a home.
pub fn plan_sweep(homes: &[Home], now: DateTime<Utc>) -> Result<Option<Swept>, String> {
    let Some(first) = homes.first() else {
        return Ok(None);
    };
    let root = first.root.as_path();
    let commit = head(root)?;
    let mut authorities = Vec::with_capacity(homes.len());
    for home in homes {
        authorities.push(Authority::at(home, &commit)?);
    }
    let zone = history::files_at(root, &commit, &first.agents)
        .map_err(|error| format!("{} could not be read at {commit}: {error}", first.agents))?;
    let dirty = history::dirty_paths(root, &format!("{}/", first.agents))
        .map_err(|error| format!("{}'s status could not be read: {error}", first.agents))?;
    let protected = protection(first, &authorities[0].decl, &commit, &zone, &dirty)?;

    let skills_dir = first.in_agents("_skills");
    let archive_dir = format!(
        "{skills_dir}/{}/",
        keeper_core::agents::consolidate::ARCHIVE_DIR
    );
    let mut folders = BTreeSet::new();
    let mut archived = BTreeSet::new();
    for file in &zone {
        if let Some(rest) = file.path.strip_prefix(&archive_dir) {
            if let Some((name, _)) = rest.split_once('/') {
                archived.insert(name.to_owned());
            }
        } else if let Some(rest) = file.path.strip_prefix(&format!("{skills_dir}/")) {
            if let Some((name, _)) = rest.split_once('/') {
                if !name.starts_with('.') {
                    folders.insert(name.to_owned());
                }
            }
        }
    }
    let mut skills = Vec::new();
    for name in folders {
        let folder = format!("{skills_dir}/{name}");
        let Some(text) = text_at(root, &commit, &format!("{folder}/SKILL.md"))? else {
            continue;
        };
        let uncommitted = dirty
            .iter()
            .any(|path| path == &folder || path.starts_with(&format!("{folder}/")));
        skills.push(Skill {
            last_change: last_change(root, &commit, &folder)?,
            name,
            text,
            uncommitted,
        });
    }

    let held: BTreeSet<&str> = zone.iter().map(|file| file.path.as_str()).collect();
    let mut proposals = Vec::new();
    let mut blobs: BTreeMap<String, BTreeMap<Ulid, String>> = BTreeMap::new();
    for home in homes {
        let dir = home.in_agents(&format!("{}/{}/", home.folder, proposal::DIR));
        for file in &zone {
            let Some(name) = file.path.strip_prefix(&dir) else {
                continue;
            };
            let Some(stem) = name.strip_suffix(".md").filter(|stem| !stem.contains('/')) else {
                continue;
            };
            let Ok(id) = Ulid::from_string(stem) else {
                continue;
            };
            let Some(text) = text_at(root, &commit, &file.path)? else {
                continue;
            };
            blobs
                .entry(home.folder.clone())
                .or_default()
                .insert(id, blob_id(text.as_bytes()));
            proposals.push((home.folder.clone(), stem.to_owned(), text));
        }
    }

    let decided_by = curate::origin_of_host(&first.host);
    let mut plan = curate::sweep(&Sweep {
        skills: &skills,
        archived: &archived,
        listed: &protected.listed,
        workflows: &protected.workflows,
        incomplete: protected.incomplete.as_deref(),
        proposals: &proposals,
        decided_by: &decided_by,
        now,
    });
    // A done proposal or a verdict a person put in place stays theirs: the
    // proposal waits, unresolved.
    let mut notes = Vec::new();
    plan.expired.retain(|(folder, settled)| {
        let (_, done, verdict) = proposal_paths(folder, &settled.id);
        let taken = [done, verdict]
            .into_iter()
            .map(|rel| first.in_agents(&rel))
            .find(|path| held.contains(path.as_str()));
        if let Some(path) = &taken {
            notes.push(format!(
                "{path} exists already, so {} stays pending",
                settled.id
            ));
        }
        taken.is_none()
    });
    plan.notes.extend(notes);

    let mut out = request(
        plan.subject(&first.drive),
        &decided_by,
        &BTreeSet::new(),
        &authorities[0],
        None,
    );
    for authority in &authorities[1..] {
        out.guards.extend(authority.guards.iter().cloned());
    }
    out.guards.extend(protected.guards);
    for change in plan.marks.iter().chain(&plan.archives) {
        change_into(first, change, &commit, &mut out)?;
    }
    for home in homes {
        let settled: Vec<Settled> = plan
            .expired
            .iter()
            .filter(|(folder, _)| *folder == home.folder)
            .map(|(_, settled)| settled.clone())
            .collect();
        if let Some(blobs) = blobs.get(&home.folder) {
            settle_into(home, &settled, blobs, &commit, now, &mut out);
        }
    }
    Ok(Some(Swept {
        plan,
        request: out,
        commit,
    }))
}

/// Sweep the drive `homes` are homed in at `now`, written through `engine`
/// as one commit whose publication asks `fence` — the claim may still
/// write — right before it is made.
pub async fn run_sweep(
    engine: &Arc<Engine>,
    homes: &[Home],
    now: DateTime<Utc>,
    fence: &Fence,
) -> Result<Outcome, String> {
    let swept = {
        let homes = homes.to_vec();
        tokio::task::spawn_blocking(move || plan_sweep(&homes, now))
            .await
            .map_err(|error| error.to_string())??
    };
    let (Some(swept), Some(first)) = (swept, homes.first()) else {
        return Ok(Outcome::Committed(None));
    };
    for note in &swept.plan.notes {
        tracing::info!(drive = %first.drive, "agents: the curator left something alone: {note}");
    }
    if !swept.plan.commits() {
        return Ok(Outcome::Committed(None));
    }
    match engine
        .commit_paths(&first.profile_id, &swept.request, Arc::clone(fence))
        .await
        .map_err(|error| error.to_string())?
    {
        CommitPaths::Committed { commit } => Ok(Outcome::Committed(Some(commit))),
        CommitPaths::Unchanged => Ok(Outcome::Committed(None)),
        CommitPaths::Guarded { path } => {
            tracing::info!(drive = %first.drive, %path, "agents: something the sweep read changed since; nothing is written until it is tried again");
            Ok(Outcome::Skipped { path })
        }
        CommitPaths::Fenced => Ok(Outcome::Fenced),
    }
}

/// One drive's week under its claim: the pull, then the sweep at the
/// server's time — `clock`, read right before the plan, never this
/// machine's own — fenced on `fence`.
pub async fn sweep_drive(
    engine: &Arc<Engine>,
    homes: &[Home],
    clock: &ServerClock,
    fence: &Fence,
) -> Result<Outcome, String> {
    let Some(first) = homes.first() else {
        return Ok(Outcome::Committed(None));
    };
    pull(engine, &first.profile_id)
        .await
        .map_err(|error| format!("the drive could not be pulled: {error}"))?;
    let now = i64::try_from(clock.now())
        .ok()
        .and_then(DateTime::<Utc>::from_timestamp_millis)
        .ok_or("the server's clock reads no time")?;
    run_sweep(engine, homes, now, fence).await
}

/// A host's sweeps: one run in flight, started from the tick and never
/// waited on by it; what each drive's week came to remembered.
pub(crate) struct Curator {
    engine: Arc<Engine>,
    running: Option<tokio::task::JoinHandle<()>>,
    remembered: Arc<Mutex<BTreeMap<String, Remembered>>>,
}

/// Whether `drive` is owed its sweep of `window` by what is `remembered`.
fn owed(remembered: &BTreeMap<String, Remembered>, drive: &str, window: i64) -> bool {
    match remembered.get(drive) {
        Some(Remembered::Done(done)) => *done != window,
        Some(Remembered::Again(at)) => tokio::time::Instant::now() >= *at,
        None => true,
    }
}

impl Curator {
    pub(crate) fn new(engine: Arc<Engine>) -> Curator {
        Curator {
            engine,
            running: None,
            remembered: Arc::default(),
        }
    }

    /// Start a run when a drive of `round` is owed its sweep and none runs.
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
        let Some(window) = sweep_window(round.now_ms, round.offset) else {
            return;
        };
        let any = {
            let remembered = self.remembered.lock().unwrap_or_else(|p| p.into_inner());
            round
                .drives
                .iter()
                .any(|drive| owed(&remembered, &drive.id, window))
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

/// Each drive of `round` owed its sweep `window`: run under the drive's
/// maintenance claim; what each came to goes to `remembered` — done only
/// for a run the claim's helper says recorded ([`Maintained::Ran`]).
pub(crate) async fn run_round(
    engine: Arc<Engine>,
    round: NightRound,
    window: i64,
    remembered: Arc<Mutex<BTreeMap<String, Remembered>>>,
) {
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
            &drive.id,
            window,
        );
        if !due {
            continue;
        }
        match week(&engine, &round, drive, window).await {
            Ok(Maintained::Done) => remember(&drive.id, Remembered::Done(window)),
            Ok(Maintained::HeldBy(host)) => {
                tracing::info!(drive = %drive.id, "agents: the maintenance of {} is held by {host}", drive.id);
                remember(&drive.id, again());
            }
            Ok(Maintained::Ran { recorded: true, .. }) => {
                remember(&drive.id, Remembered::Done(window));
            }
            Ok(Maintained::Ran {
                recorded: false, ..
            }) => {
                tracing::warn!(drive = %drive.id, "agents: the sweep of {} did not finish; it is tried again", drive.id);
                remember(&drive.id, again());
            }
            Err(error) => {
                tracing::warn!(drive = %drive.id, %error, "agents: the drive's maintenance claim could not be read");
                remember(&drive.id, again());
            }
        }
    }
}

/// `drive`'s sweep of `window` under the drive's maintenance claim, its
/// completion the curator's own: the one place the claim is asked for.
async fn week(
    engine: &Arc<Engine>,
    round: &NightRound,
    drive: &NightDrive,
    window: i64,
) -> Result<Maintained<()>, keeper_core::agents::matrix::AgentMatrixError> {
    let me = crate::hosts::claimant(&round.me, drive.copy.as_ref());
    let (lease_key, done_key) = (
        claim::maintenance_key(&drive.id),
        claim::completion_key(curate::JOB, &drive.id),
    );
    let lease_port = drive.copy.keyed_claims(&round.control, &lease_key);
    let done_port = drive.copy.keyed_claims(&round.control, &done_key);
    let homes = drive.homes();
    let clock = round.clock.as_ref();
    maintain(
        &lease_port,
        done_port.as_ref(),
        &me,
        &round.clock,
        &round.rtt,
        Some(window),
        |fence| async move {
            match sweep_drive(engine, &homes, clock, &fence).await {
                Ok(Outcome::Committed(commit)) => {
                    tracing::info!(drive = %drive.id, ?commit, "agents: a week of skill curation");
                    match followed(engine, &homes).await {
                        Ok(true) => ((), true),
                        Ok(false) => {
                            tracing::warn!(drive = %drive.id, "agents: the sweep's commit has not all reached the disk yet; it is tried again");
                            ((), false)
                        }
                        Err(error) => {
                            tracing::warn!(drive = %drive.id, %error, "agents: whether the sweep finished could not be read; it is tried again");
                            ((), false)
                        }
                    }
                }
                // The accepted plan was refused at its publication: nothing
                // of it — no mark, no archive, no expiry — happened, so the
                // week is still owed.
                Ok(Outcome::Skipped { path }) => {
                    tracing::info!(drive = %drive.id, %path, "agents: the sweep was held at its publication; it is tried again");
                    ((), false)
                }
                Ok(Outcome::Fenced) => {
                    tracing::warn!(drive = %drive.id, "agents: the maintenance claim was lost; nothing more is written this week");
                    ((), false)
                }
                Err(error) => {
                    tracing::warn!(drive = %drive.id, %error, "agents: this drive's sweep failed; it is tried again");
                    ((), false)
                }
            }
        },
    )
    .await
}

/// Whether every commit published on the drive `homes` are homed in has
/// had its files follow it: a week is complete only then.
async fn followed(engine: &Arc<Engine>, homes: &[Home]) -> Result<bool, String> {
    let Some(first) = homes.first() else {
        return Ok(true);
    };
    let (engine, profile) = (Arc::clone(engine), first.profile_id.clone());
    tokio::task::spawn_blocking(move || engine.unsettled_commit(&profile))
        .await
        .map_err(|error| error.to_string())?
        .map(|unsettled| !unsettled)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> i64 {
        DateTime::parse_from_rfc3339(text)
            .expect("time")
            .timestamp_millis()
    }

    /// 95.3 acceptance 6, the window: the week is the latest Sunday 04:00 at
    /// the host's offset at or before now — before it, last week's — so a
    /// host back after missing three Sundays is owed one sweep.
    #[test]
    fn the_week_is_the_latest_sunday_four_oclock() {
        assert_eq!(
            sweep_window(at("2026-10-06T10:00:00Z"), 0),
            Some(at("2026-10-04T04:00:00Z"))
        );
        assert_eq!(
            sweep_window(at("2026-10-04T03:59:59Z"), 0),
            Some(at("2026-09-27T04:00:00Z"))
        );
        assert_eq!(
            sweep_window(at("2026-10-04T04:00:00Z"), 0),
            Some(at("2026-10-04T04:00:00Z"))
        );
        assert_eq!(
            sweep_window(at("2026-10-04T03:00:00Z"), 120),
            Some(at("2026-10-04T02:00:00Z")),
            "04:00 at UTC+2"
        );
    }
}
