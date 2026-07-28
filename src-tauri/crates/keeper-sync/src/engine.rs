//! The sync driver — the one place that turns a profile into real work
//! (AD-42, AD-49, AD-52).
//!
//! Everything else in this crate is a capability: `git` knows how to fetch and
//! commit, `lfs` knows how to move a large object, `stability` knows whether a
//! file is finished, `volume` knows whether a pendrive is attached. This module
//! is the only thing that decides *when* to use them, and it is shared verbatim
//! by the Tauri app and by `keeper-syncd` — AD-52's whole point is that there is
//! no second implementation of this policy on the server.
//!
//! # Why the shape is what it is
//!
//! * **The journal is the plan.** [`Engine::run`] never keeps intent in memory.
//!   A unit is written to `sync.db` before it is attempted and deleted only once
//!   its effect is durable, so a `kill -9` costs a repeat rather than a loss
//!   (NFR-24). Anything found `running` at startup is re-queued.
//! * **Blocking work is fenced.** gitoxide's HTTP transport and every
//!   filesystem walk are blocking, so they run inside `spawn_blocking`; only the
//!   LFS transfers are natively async.
//! * **One operation per profile.** Profiles are otherwise fully concurrent, but
//!   two operations on one working tree would race on the index.
//! * **Absence is not deletion.** Every tick re-checks a removable profile's
//!   volume *before* it looks at the filesystem, because a detached drive looks
//!   exactly like a user deleting every file in it (AD-48).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::backoff::{jitter_sample, Backoff};
use crate::db::{self, ActivityKind, ActivityRow, DeviceIdentity, WorkKind, WorkState};
use crate::error::{Result, Retriability, SyncError};
use crate::git::{self, cli::GitCli};
use crate::lfs;
use crate::platform::SyncPlatform;
use crate::profile::{LfsMode, ProfileState, SyncDirection, SyncLane, SyncProfile};
use crate::progress::{ProgressSink, SyncPhase, SyncProgress, SyncStatus, TransferTally};
use crate::provenance::{commit_message, Provenance, SyncSource};
use crate::stability::{StabilityGate, StabilityVerdict};
use crate::volume::{self, VolumeStatus};

/// What one `sync_once` actually did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// The commit created locally, if any.
    pub committed: Option<String>,
    pub pushed: bool,
    pub pulled: bool,
    pub files_changed: u64,
    /// Bytes this run moved over the network: the pack a fetch received plus
    /// every LFS object transferred, including the ones drained from the
    /// journal after the push leg.
    ///
    /// The LFS half is exact. The fetch half is gitoxide's own transfer
    /// counter, which `git::fetch` flattens without its unit, so on a pack
    /// dominated by object bookkeeping rather than payload it is an estimate.
    /// An estimate of the dominant term beats omitting the pack entirely,
    /// which for a text repository would report nearly every sync as zero.
    pub bytes: u64,
    /// Conflict copies created during this run, as repository-relative paths.
    pub conflicts: Vec<String>,
}

/// Result of a verification pass (Story 25.6).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked: u64,
    /// `(path, reason)` for everything that failed.
    pub bad: Vec<(String, String)>,
}

/// Why a path is not synced yet (Story 32.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Internally tagged, like `WorkKind`: the UI reads this as a discriminated
// union, and `kind` rather than `reason` keeps it from nesting as
// `reason.reason` inside a `PendingFile`.
//
// `rename_all` on an enum renames the VARIANTS only — `rename_all_fields` is
// what reaches the payload of a struct variant, and without it `since_ms`
// would cross the boundary as snake_case while every neighbouring field is
// camelCase.
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum PendingReason {
    /// Held by the completeness gate: the bytes are still moving, or the last
    /// observation is not yet old enough to prove they stopped. `since_ms` is
    /// when this waiting episode began, so a UI can say how long it has been.
    Settling { since_ms: i64 },
    /// On disk and git has never heard of it.
    Untracked,
    /// Tracked, and its content or mode differs from the index.
    Modified,
    /// Staged as new, not yet committed.
    Added,
    /// Tracked and no longer on disk.
    Deleted,
}

/// One path the folder is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFile {
    /// Repository-relative.
    pub path: String,
    pub reason: PendingReason,
}

/// A unit of work the engine has given up on, as a human needs to see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParkedUnit {
    pub id: i64,
    /// The journal's own discriminant (`push`, `lfsUpload`, …) rather than a
    /// re-serialized payload, so a unit parked *for* an unreadable payload is
    /// still describable.
    pub kind: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Everything currently wrong with one profile (Story 32.2).
///
/// Assembled on demand from three sources that each already know a piece of
/// the truth, rather than maintained as a fourth copy that could disagree with
/// them (AD-S3).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemReport {
    /// Sticky warning from the live status snapshot.
    pub warning: Option<String>,
    /// Terminal error from the live status snapshot.
    pub error: Option<String>,
    /// Stopped work, which [`db::pending_count`] deliberately excludes and
    /// which therefore has no other surface at all.
    pub parked: Vec<ParkedUnit>,
    /// Conflict copies still sitting in the working tree, repository-relative.
    pub conflicts: Vec<String>,
}

/// Units claimed from the journal per profile per tick.
///
/// Small on purpose: a tick that drains a thousand units would hold the
/// profile's reservation for minutes and starve its watcher.
const CLAIM_LIMIT: u32 = 16;

/// How often the supervisor wakes when nothing else prompts it.
const TICK_MS: u64 = 1_000;

/// How many consecutive transient failures before a profile stops calling
/// itself healthy.
///
/// Low enough that a genuinely stuck profile surfaces within a few ticks,
/// high enough that a single blip - a momentarily locked file, an EINTR -
/// never raises a notification for something that fixed itself.
const TRANSIENT_FAILURES_BEFORE_WARNING: u32 = 3;

pub struct Engine {
    platform: Arc<dyn SyncPlatform>,
    /// Single connection behind a mutex. `sync.db` is small and every access is
    /// short; a pool would add contention management for no measurable gain,
    /// and the mutex makes "never hold a `Connection` across an `.await`"
    /// checkable by inspection — every guard here is dropped inside a sync
    /// helper before any await point.
    db: Mutex<Connection>,
    device: DeviceIdentity,
    git: GitCli,
    http: reqwest::Client,
    /// Live status per profile id, the polled snapshot the tray reads.
    status: Mutex<HashMap<String, SyncStatus>>,
    /// Per-profile completeness gates, retained across ticks so a settling file
    /// is remembered rather than re-observed from scratch every time.
    gates: Mutex<HashMap<String, StabilityGate>>,
    /// Profiles with an operation in flight (the one-per-profile rule).
    busy: Mutex<HashMap<String, ()>>,
    /// Consecutive transient failures per profile.
    ///
    /// A transient error is retried and, on its own, is not worth alarming
    /// anyone about. A transient error that never stops recurring is a profile
    /// that has silently stopped syncing, and reporting that one as healthy is
    /// the dishonesty this counter exists to prevent. Reset by any success.
    transient_failures: Mutex<HashMap<String, u32>>,
    /// Absolute path to the binary git should invoke as the `lfs` filter.
    ///
    /// `None` when the running executable cannot be resolved — the filter is an
    /// interoperability convenience for humans using plain `git`, never a
    /// correctness requirement for keeper itself, so a missing path degrades
    /// rather than fails.
    filter_program: Option<PathBuf>,
    sinks: Mutex<Vec<(u64, ProgressSink)>>,
    next_sink: AtomicU64,
    interrupt: Arc<AtomicBool>,
    /// Bytes moved over the network per profile, monotonic for the life of the
    /// process.
    ///
    /// A running total rather than a per-operation one because the work that
    /// moves bytes is reached through call paths that share no return value: a
    /// fetch inside [`Engine::do_pull`], an LFS unit drained from the journal
    /// by [`Engine::drain_journal`] long after its enqueuer returned.
    /// [`Engine::sync_once`] reads this before and after its run and reports
    /// the difference, which is exactly "what this run moved".
    transferred: Mutex<HashMap<String, u64>>,
}

impl Engine {
    /// Open the durable store, recover interrupted work, and probe `git`.
    ///
    /// A missing `git` is fatal here rather than at first use: AD-41 makes it a
    /// declared prerequisite, and discovering it mid-push would leave a profile
    /// half-applied.
    pub fn open(platform: Arc<dyn SyncPlatform>) -> Result<Self> {
        let data_dir = platform.data_dir()?;
        let conn = db::open(&data_dir)?;
        let device = db::device_identity(&conn, &platform.host_label())?;
        let now = platform.now_ms();
        db::recover_running(&conn, now)?;

        let program = platform.git_program()?;
        let git = GitCli::new(program);
        let capabilities = git.capabilities()?;
        if !capabilities.meets_floor() {
            return Err(SyncError::GitMissing {
                reason: format!(
                    "git {}.{} is too old; {}.{} or newer is required for cone sparse-checkout",
                    capabilities.major,
                    capabilities.minor,
                    crate::git::cli::MIN_GIT_MAJOR,
                    crate::git::cli::MIN_GIT_MINOR
                ),
            });
        }

        let http = reqwest::Client::builder()
            .user_agent(crate::AGENT)
            .build()
            .map_err(|err| SyncError::Config(format!("could not build an HTTP client: {err}")))?;

        let engine = Self {
            platform,
            db: Mutex::new(conn),
            device,
            git,
            http,
            status: Mutex::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashMap::new()),
            transient_failures: Mutex::new(HashMap::new()),
            // `current_exe` is the daemon in a CLI run and the app binary in a
            // desktop run; both understand `lfs clean|smudge`.
            filter_program: std::env::current_exe().ok(),
            sinks: Mutex::new(Vec::new()),
            next_sink: AtomicU64::new(1),
            interrupt: Arc::new(AtomicBool::new(false)),
            transferred: Mutex::new(HashMap::new()),
        };
        engine.seed_status()?;
        Ok(engine)
    }

    /// Poison-tolerant lock. A panic in one profile's handler must not take
    /// every other profile down with it — the mutexes here guard plain data,
    /// so a poisoned guard is still safe to use.
    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn with_db<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = Self::lock(&self.db);
        f(&guard)
    }

    fn seed_status(&self) -> Result<()> {
        let profiles = self.with_db(db::list_profiles)?;
        let mut status = Self::lock(&self.status);
        for profile in profiles {
            let pending = self.pending_for(&profile.id).unwrap_or(0);
            let mut snapshot = SyncStatus::idle(&profile.id, &profile.name);
            snapshot.pending = pending;
            snapshot.state = if !profile.enabled {
                ProfileState::Paused
            } else {
                // Restore what the last run observed, so a one-shot `status`
                // tells the truth about a detached drive or a profile that
                // stopped needing attention. Transient in-flight states are
                // deliberately NOT restored — nothing is syncing yet.
                let stored = self
                    .with_db(|conn| db::get_profile_state(conn, &profile.id))
                    .unwrap_or(None);
                match stored {
                    Some(state @ (ProfileState::MediaAbsent | ProfileState::NeedsAttention)) => {
                        state
                    }
                    _ => ProfileState::Idle,
                }
            };
            status.insert(profile.id.clone(), snapshot);
        }
        Ok(())
    }

    fn pending_for(&self, profile_id: &str) -> Result<u32> {
        self.with_db(|conn| db::pending_count(conn, profile_id))
    }

    // -----------------------------------------------------------------------
    // Profile management
    // -----------------------------------------------------------------------

    pub fn list_profiles(&self) -> Result<Vec<SyncProfile>> {
        self.with_db(db::list_profiles)
    }

    pub fn upsert_profile(&self, profile: &SyncProfile) -> Result<()> {
        let now = self.platform.now_ms();
        self.with_db(|conn| db::upsert_profile(conn, profile, now))?;
        let mut status = Self::lock(&self.status);
        let entry = status
            .entry(profile.id.clone())
            .or_insert_with(|| SyncStatus::idle(&profile.id, &profile.name));
        entry.profile_name = profile.name.clone();
        if !profile.enabled {
            entry.state = ProfileState::Paused;
        } else if entry.state == ProfileState::Paused {
            entry.state = ProfileState::Idle;
        }
        // A changed root or exclude set invalidates every remembered sample.
        Self::lock(&self.gates).remove(&profile.id);
        Ok(())
    }

    pub fn remove_profile(&self, id: &str) -> Result<()> {
        self.with_db(|conn| db::delete_profile(conn, id))?;
        Self::lock(&self.status).remove(id);
        Self::lock(&self.gates).remove(id);
        Ok(())
    }

    /// Pause or resume a profile.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let Some(mut profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        profile.enabled = enabled;
        self.upsert_profile(&profile)?;
        if enabled {
            // Work deferred while paused becomes eligible again immediately;
            // making the user wait out a backoff they did not cause is rude.
            let now = self.platform.now_ms();
            self.with_db(|conn| db::undefer_profile(conn, id, now))?;
        }
        Ok(())
    }

    pub fn status(&self, id: &str) -> Result<SyncStatus> {
        Self::lock(&self.status)
            .get(id)
            .cloned()
            .ok_or_else(|| SyncError::Config(format!("no such sync profile: {id}")))
    }

    pub fn statuses(&self) -> Result<Vec<SyncStatus>> {
        let mut all: Vec<SyncStatus> = Self::lock(&self.status).values().cloned().collect();
        all.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
        Ok(all)
    }

    // -----------------------------------------------------------------------
    // Progress fan-out
    // -----------------------------------------------------------------------

    pub fn subscribe(&self, sink: ProgressSink) -> u64 {
        let id = self.next_sink.fetch_add(1, Ordering::SeqCst);
        Self::lock(&self.sinks).push((id, sink));
        id
    }

    pub fn unsubscribe(&self, id: u64) {
        Self::lock(&self.sinks).retain(|(existing, _)| *existing != id);
    }

    /// Publish a progress event and fold it into the polled snapshot.
    ///
    /// A sink returning `false` has gone away (a closed IPC channel), and is
    /// dropped here rather than accumulating for the life of the process.
    fn publish(&self, event: SyncProgress) {
        {
            let mut status = Self::lock(&self.status);
            if let Some(snapshot) = status.get_mut(&event.profile_id) {
                snapshot.phase = event.phase;
                snapshot.files_done = event.files_done;
                snapshot.files_total = event.files_total;
                snapshot.bytes_done = event.bytes_done;
                snapshot.bytes_total = event.bytes_total;
                if event.phase.is_active() {
                    snapshot.state = ProfileState::Syncing;
                }
            }
        }
        let mut sinks = Self::lock(&self.sinks);
        sinks.retain(|(_, sink)| sink(event.clone()));
    }

    fn set_state(&self, profile_id: &str, state: ProfileState) {
        if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
            snapshot.state = state;
        }
        // Persist so a separate `keeper-syncd status` invocation reports the
        // truth rather than a fresh "idle". Best-effort: failing to record a
        // status must never fail the sync that produced it.
        if let Err(err) = self.with_db(|conn| db::set_profile_state(conn, profile_id, state)) {
            tracing::debug!(error = %err, "could not persist sync profile state");
        }
    }

    /// Record a sticky warning and notify exactly once on its onset.
    ///
    /// The onset check and the notification are deliberately split: the
    /// notification is raised *after* the status lock is released, mirroring
    /// `fold_recording_event`'s discipline in the shell, because a notifier can
    /// block and holding a lock across it would stall the tray tick.
    fn warn(&self, profile_id: &str, profile_name: &str, message: String) {
        let is_onset = {
            let mut status = Self::lock(&self.status);
            match status.get_mut(profile_id) {
                Some(snapshot) => {
                    let onset = snapshot.warning.as_deref() != Some(message.as_str());
                    snapshot.warning = Some(message.clone());
                    onset
                }
                None => false,
            }
        };
        if is_onset {
            tracing::warn!(profile = profile_name, message = %message, "sync warning");
            self.platform
                .notify(&format!("Sync — {profile_name}"), &message);
        }
    }

    fn clear_warning(&self, profile_id: &str) {
        if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
            snapshot.warning = None;
            snapshot.error = None;
        }
    }

    // -----------------------------------------------------------------------
    // The supervisor
    // -----------------------------------------------------------------------

    /// Drive every enabled profile until `shutdown` flips to `true`.
    ///
    /// This owns the whole lifecycle: startup recovery, the per-tick volume
    /// gate, claiming journal units, executing them, backoff rescheduling, and
    /// a bounded graceful finalize. Both hosts call exactly this.
    pub async fn run(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) -> Result<()> {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(TICK_MS));
        // Delay, not Burst: after a long stall we want one catch-up tick, not a
        // backlog of them fired back to back at a git server.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        tracing::info!(device = %self.device.id, "sync supervisor started");
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if let Err(err) = self.tick().await {
                        // A tick failure is never fatal to the supervisor: one
                        // bad profile must not stop every other one.
                        tracing::error!(error = %err, "sync tick failed");
                    }
                }
                changed = shutdown.changed() => {
                    // A dropped sender means the host is gone; treat it exactly
                    // like an explicit shutdown rather than spinning forever.
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            }
        }

        self.interrupt.store(true, Ordering::SeqCst);
        self.finalize()?;
        tracing::info!("sync supervisor stopped");
        Ok(())
    }

    /// Return in-flight units to the queue so a restart resumes them.
    fn finalize(&self) -> Result<()> {
        let now = self.platform.now_ms();
        self.with_db(|conn| db::recover_running(conn, now))
            .map(drop)
    }

    async fn tick(&self) -> Result<()> {
        for profile in self.list_profiles()? {
            if !profile.enabled {
                continue;
            }
            match self.tick_profile(&profile).await {
                Ok(()) => {
                    Self::lock(&self.transient_failures).remove(&profile.id);
                }
                Err(err) => self.record_failure(&profile, &err),
            }
        }
        Ok(())
    }

    async fn tick_profile(&self, profile: &SyncProfile) -> Result<()> {
        // The volume gate runs before anything touches the filesystem. A
        // detached drive is indistinguishable from a mass deletion once you
        // start walking the tree, so we never start walking (AD-48).
        if !self.volume_ready(profile)? {
            return Ok(());
        }

        let _reservation = match self.reserve(&profile.id) {
            Some(guard) => guard,
            // Still working from a previous tick. Not an error.
            None => return Ok(()),
        };

        self.drain_journal(profile).await
    }

    /// Execute every journal unit that is ready for this profile.
    ///
    /// Shared by the supervisor and by `sync_once`: a one-shot sync that
    /// committed a pointer but never ran the LFS transfer it queued would leave
    /// the remote holding a pointer to content it does not have.
    ///
    /// The caller owns the profile reservation.
    async fn drain_journal(&self, profile: &SyncProfile) -> Result<()> {
        let now = self.platform.now_ms();
        let claimed = self.with_db(|conn| db::claim_ready(conn, &profile.id, now, CLAIM_LIMIT))?;
        if claimed.is_empty() {
            // Nothing queued — look for new local work.
            self.scan_and_enqueue(profile)?;
            return Ok(());
        }

        for item in claimed {
            match self.execute(profile, &item.kind).await {
                Ok(()) => {
                    self.with_db(|conn| db::complete(conn, item.id))?;
                    self.clear_warning(&profile.id);
                }
                Err(err) => {
                    self.reschedule_after(profile, item.id, item.attempts, &err)?;
                }
            }
        }
        self.refresh_pending(&profile.id);
        Ok(())
    }

    /// Apply the retry policy for a failed unit.
    fn reschedule_after(
        &self,
        profile: &SyncProfile,
        item_id: i64,
        attempts: u32,
        err: &SyncError,
    ) -> Result<()> {
        let now = self.platform.now_ms();
        let (state, not_before) = match err.retriability() {
            Retriability::Transient => (
                WorkState::Pending,
                Backoff::default().not_before_ms(now, attempts, jitter_sample()),
            ),
            // Deferred work waits on a condition, not a clock — parking it at
            // `now` would make it spin every tick against an absent volume.
            Retriability::Deferred => (WorkState::Deferred, now),
            Retriability::Permanent => (WorkState::Parked, now),
        };
        self.with_db(|conn| {
            db::reschedule(conn, item_id, state, not_before, Some(&err.to_string()))
        })?;
        self.record_failure(profile, err);
        Ok(())
    }

    fn record_failure(&self, profile: &SyncProfile, err: &SyncError) {
        match err.retriability() {
            Retriability::Deferred => {
                self.set_state(&profile.id, ProfileState::MediaAbsent);
            }
            Retriability::Transient if matches!(err, SyncError::Network { .. }) => {
                // Offline is a state, not a failure: local git keeps working
                // and the queue drains when connectivity returns (AD-49).
                self.set_state(&profile.id, ProfileState::Offline);
                tracing::debug!(profile = profile.name, error = %err, "sync offline");
            }
            Retriability::Transient => {
                tracing::warn!(profile = profile.name, error = %err, "sync retrying");
                // One failure is noise; a run of them is a profile that has
                // stopped syncing while still reporting itself healthy. Past
                // the threshold it says so, through the same sticky
                // once-per-onset channel every other warning uses - and any
                // success clears both the count and the warning.
                let consecutive = {
                    let mut counts = Self::lock(&self.transient_failures);
                    let counter = counts.entry(profile.id.clone()).or_insert(0);
                    *counter = counter.saturating_add(1);
                    *counter
                };
                if consecutive >= TRANSIENT_FAILURES_BEFORE_WARNING {
                    self.warn(
                        &profile.id,
                        &profile.name,
                        format!("sync has failed {consecutive} times in a row: {err}"),
                    );
                }
            }
            Retriability::Permanent => {
                self.set_state(&profile.id, ProfileState::NeedsAttention);
                if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
                    snapshot.error = Some(err.to_string());
                }
                if err.needs_user_action() {
                    self.warn(&profile.id, &profile.name, err.to_string());
                }
            }
        }
    }

    fn refresh_pending(&self, profile_id: &str) {
        if let Ok(pending) = self.pending_for(profile_id) {
            if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
                snapshot.pending = pending;
                if pending == 0 && snapshot.state == ProfileState::Syncing {
                    snapshot.state = ProfileState::Watching;
                    snapshot.phase = SyncPhase::Idle;
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Removable media
    // -----------------------------------------------------------------------

    /// `Ok(false)` means "skip this profile, nothing is wrong".
    fn volume_ready(&self, profile: &SyncProfile) -> Result<bool> {
        if !profile.removable {
            return Ok(true);
        }
        match volume::scan(&profile.local_path, None)? {
            VolumeStatus::Present { .. } => {
                let previously_absent = Self::lock(&self.status).get(&profile.id).map(|s| s.state)
                    == Some(ProfileState::MediaAbsent);
                if previously_absent {
                    tracing::info!(profile = profile.name, "removable volume re-attached");
                    let now = self.platform.now_ms();
                    self.with_db(|conn| db::undefer_profile(conn, &profile.id, now))?;
                    self.set_state(&profile.id, ProfileState::Watching);
                    self.clear_warning(&profile.id);
                }
                Ok(true)
            }
            VolumeStatus::Absent => {
                self.set_state(&profile.id, ProfileState::MediaAbsent);
                Ok(false)
            }
            VolumeStatus::Foreign { found_id } => {
                // A different volume mounted where this profile lives. Adopting
                // it would sync a stranger's disk; refusing is the only safe
                // answer, and it needs a human.
                self.warn(
                    &profile.id,
                    &profile.name,
                    format!(
                        "a different volume ({found_id}) is mounted at this profile's path — not syncing"
                    ),
                );
                self.set_state(&profile.id, ProfileState::NeedsAttention);
                Ok(false)
            }
        }
    }

    fn reserve(&self, profile_id: &str) -> Option<Reservation<'_>> {
        let mut busy = Self::lock(&self.busy);
        if busy.contains_key(profile_id) {
            return None;
        }
        busy.insert(profile_id.to_owned(), ());
        Some(Reservation {
            engine: self,
            profile_id: profile_id.to_owned(),
        })
    }

    // -----------------------------------------------------------------------
    // Work execution
    // -----------------------------------------------------------------------

    async fn execute(&self, profile: &SyncProfile, kind: &WorkKind) -> Result<()> {
        match kind {
            // The conflict copies are already recorded and warned about;
            // a journaled pull has no caller to hand them back to.
            WorkKind::Pull => self.do_pull(profile).await.map(drop),
            WorkKind::Push => self.do_push(profile).await,
            WorkKind::LfsDownload { oid, size } => self.do_lfs(profile, oid, *size, false).await,
            WorkKind::LfsUpload { oid, size } => self.do_lfs(profile, oid, *size, true).await,
            WorkKind::OpenPullRequest { branch } => self.do_open_pr(profile, branch).await,
            WorkKind::Verify => self.verify(&profile.id).await.map(drop),
        }
    }

    /// The branch a review lane publishes on (AD-50).
    ///
    /// Stable per profile rather than per run: a long-lived bot folder should
    /// accumulate onto one branch behind one pull request, not strew a new
    /// branch across the remote every time the supervisor ticks. The profile id
    /// keeps it unique, and the `keeper/` prefix keeps it unmistakable.
    fn lane_branch(profile: &SyncProfile) -> String {
        let slug: String = profile
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        format!("keeper/{}/{}", slug.trim_matches('-'), profile.id)
    }

    /// The branch this profile writes on: its own for a lane, the tracked
    /// branch otherwise.
    fn working_branch(profile: &SyncProfile) -> String {
        if profile.lane == SyncLane::Worktree {
            Self::lane_branch(profile)
        } else {
            profile.branch.clone()
        }
    }

    /// Put a lane profile on its own branch before anything is committed.
    ///
    /// Idempotent, and deliberately never touches the base branch: that is the
    /// entire guarantee a lane exists to make.
    fn ensure_lane(&self, profile: &SyncProfile) -> Result<()> {
        if profile.lane != SyncLane::Worktree {
            return Ok(());
        }
        let branch = Self::lane_branch(profile);
        let current = self.git.current_branch(&profile.local_path)?;
        if current.as_deref() == Some(branch.as_str()) {
            return Ok(());
        }
        tracing::info!(profile = profile.name, %branch, "switching the review lane onto its branch");
        self.git.ensure_branch(&profile.local_path, &branch)
    }

    /// Materialize the profile's repository if it does not exist yet, without
    /// keeping the handle.
    ///
    /// Callers that only need the clone to have happened use this, so a
    /// `gix::Repository` — which is neither `Send` nor cheap to hold — never
    /// spans an await point.
    fn ensure_repo(&self, profile: &SyncProfile) -> Result<()> {
        self.open_repo(profile)?;
        self.ensure_lane(profile)
    }

    /// Open the profile's repository, cloning it if the folder is not one yet.
    ///
    /// Blocking; callers wrap it.
    fn open_repo(&self, profile: &SyncProfile) -> Result<gix::Repository> {
        // Removable media is opened with full trust, but only after the volume
        // marker proved the media is ours — see AD-48 for the silent
        // filter-drop this avoids.
        let trust_full = profile.removable;
        let git_dir = profile.local_path.join(".git");
        if git_dir.exists() {
            let repo = git::repo::open(&profile.local_path, trust_full)?;
            git::repo::enforce_local_config_with_filter(&repo, self.filter_program.as_deref())?;
            // A kill between `gix::init` and the config write in `adopt` leaves a
            // repository with no remote, and this branch — taken from then on,
            // because `.git` exists — would otherwise fail every future sync with
            // "the remote named origin did not exist". Restore it instead.
            if git::repo::ensure_remote(&repo, &profile.remote_url)? {
                tracing::warn!(
                    profile = %profile.id,
                    "sync: restored the missing origin remote (interrupted setup)"
                );
            }
            return Ok(repo);
        }

        // Cloning refuses a non-empty destination — and "sync this folder I
        // already have" is the ordinary case, not an edge one. So a directory
        // with content in it is ADOPTED instead: the repository is initialized
        // in place and the remote attached, after which the normal flow commits
        // the existing files as a root commit and the divergence path (AD-43)
        // reconciles them with whatever the remote already holds. Nothing is
        // overwritten and nothing is deleted to make this work.
        let empty = std::fs::read_dir(&profile.local_path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(true);

        let repo = if empty {
            tracing::info!(
                profile = profile.name,
                "cloning remote for a new sync profile"
            );
            match git::repo::clone(
                &profile.remote_url,
                &profile.local_path,
                &profile.branch,
                None,
                &self.interrupt,
            ) {
                Ok(repo) => repo,
                // A repository freshly created in the forge has no commits, so
                // there is nothing to clone and gitoxide says so. That is the
                // start of a normal life, not a failure: initialize in place
                // and let the first push create the branch. Any other error
                // still propagates untouched.
                Err(err) if git::repo::is_empty_remote(&err) => {
                    tracing::info!(
                        profile = profile.name,
                        branch = profile.branch,
                        "remote has no commits yet: initializing and pushing the first one"
                    );
                    // Clone can leave a partial `.git` behind before it fails.
                    // Adoption must start from a clean slate, and only the
                    // metadata is removed - never the user's files.
                    let partial = profile.local_path.join(".git");
                    if partial.exists() {
                        std::fs::remove_dir_all(&partial).map_err(|err| {
                            SyncError::io("clear the partial clone", &partial, err)
                        })?;
                    }
                    git::repo::adopt(&profile.local_path, &profile.remote_url, &profile.branch)?
                }
                Err(err) => return Err(err),
            }
        } else {
            tracing::info!(
                profile = profile.name,
                "adopting an existing folder: initializing a repository in place"
            );
            git::repo::adopt(&profile.local_path, &profile.remote_url, &profile.branch)?
        };
        git::repo::enforce_local_config_with_filter(&repo, self.filter_program.as_deref())?;
        if !profile.subpaths.is_empty() {
            self.git
                .sparse_set(&profile.local_path, &profile.subpaths)?;
        }
        Ok(repo)
    }

    fn credential(&self, profile: &SyncProfile) -> Result<Option<git::fetch::Credential>> {
        let Some(secret) = self.platform.secret_get(&profile.secret_key())? else {
            return Ok(None);
        };
        // Forgejo and GitHub both accept a token as the username with an inert
        // password, which is the shape that works for both Basic and PAT auth.
        Ok(Some(git::fetch::Credential {
            username: secret,
            secret: String::new(),
        }))
    }

    /// Fetch and apply, returning the conflict copies the apply had to write.
    ///
    /// The paths are repository-relative and are the *copies*, never the
    /// canonical files. They are returned rather than only counted because
    /// they are the one thing a merge leaves behind that the user has to act
    /// on, and the working tree stops naming them as soon as they are
    /// committed.
    async fn do_pull(&self, profile: &SyncProfile) -> Result<Vec<String>> {
        if !profile.direction.pulls() {
            return Ok(Vec::new());
        }
        // The very first sync of a profile has no working tree yet, so the
        // repository has to be materialized before anything can fetch into it.
        // `open_repo` clones when `.git` is absent and is idempotent after
        // that; skipping it here made the first-ever pull fail with "does not
        // appear to be a git repository".
        self.ensure_repo(profile)?;
        // Commit settled local work FIRST. `git merge` refuses to run against a
        // dirty tree, and it is right to: overwriting an uncommitted edit would
        // be exactly the silent data loss AD-43 exists to prevent. Committing
        // first also turns a divergence into commit-vs-commit, which is the
        // shape the conflict-copy path can actually resolve.
        self.commit_local(profile)?;
        self.publish(self.progress(profile, SyncPhase::Fetching));

        let profile = profile.clone();
        let credential = self.credential(&profile)?;
        let interrupt = Arc::clone(&self.interrupt);
        let repo_path = profile.local_path.clone();
        let removable = profile.removable;
        let branch = profile.branch.clone();

        // gitoxide's progress tree is the only place a fetch's volume is
        // observable, and it is throttled to one call per 100 ms per node
        // (`git::fetch::REPORT_INTERVAL_MS`). The callback has to be `'static`
        // to cross into `spawn_blocking`, so it cannot borrow the engine to
        // publish for itself; a channel carries the numbers back out. Before
        // this the callback was a no-op, which is why the bar never moved.
        let (report_tx, report_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
        let fetching = tokio::task::spawn_blocking(move || -> Result<git::fetch::FetchOutcome> {
            let repo = git::repo::open(&repo_path, removable)?;
            let options = git::fetch::FetchOptions {
                shallow: None,
                refspecs: vec![format!("+refs/heads/{branch}:refs/remotes/origin/{branch}")],
            };
            let report: git::fetch::TransferProgress = Arc::new(move |done, total| {
                // A closed receiver means the publisher gave up first; the
                // fetch is journaled work and carries on regardless.
                let _ = report_tx.send((done, total));
            });
            git::fetch::fetch(
                &repo,
                "origin",
                &options,
                credential.as_ref(),
                &report,
                &interrupt,
            )
        });

        // Every node of the tree keeps its OWN counter, so the reported figures
        // are not cumulative across phases. The live bar tracks whichever phase
        // is running — that is `git::fetch`'s documented design — while the
        // high-water mark is the honest answer to "how much did this move".
        let mut fetched = 0u64;
        let outcome = self
            .publish_while(
                &profile,
                SyncPhase::Fetching,
                report_rx,
                |event, (done, total)| {
                    fetched = fetched.max(done);
                    event.bytes_done = done;
                    event.bytes_total = (total > 0).then_some(total);
                },
                fetching,
            )
            .await
            .map_err(|err| SyncError::Journal(format!("fetch task failed: {err}")))??;

        // Only a pack is transferred content. Without a pack the counters
        // observed above are negotiation and ref-advertisement bookkeeping, and
        // charging those as bytes moved would report traffic for a no-op fetch.
        if outcome.received_pack {
            self.add_transferred(&profile.id, fetched);
        }

        // Whether a pack arrived says nothing about whether the working tree is
        // up to date: a re-fetch after an interrupted run transfers nothing and
        // still leaves the local branch behind. The only condition that matters
        // is that the two refs differ.
        let Some(remote_id) = outcome.remote_id else {
            // The remote has no such branch yet — a brand-new repository.
            return Ok(Vec::new());
        };
        if outcome.local_id == Some(remote_id) {
            return Ok(Vec::new());
        }

        // A fetch only moves `refs/remotes/origin/<branch>`; without an apply
        // step the working tree stays behind and the next push is rejected as
        // non-fast-forward. gitoxide implements no merge/reset/checkout
        // workflow, so this goes through the shim (AD-41).
        self.publish(self.progress(&profile, SyncPhase::Applying));
        let tracking = format!("refs/remotes/origin/{}", profile.branch);
        let git = self.git.clone();
        let repo_path = profile.local_path.clone();

        // A fetch leaves one of three shapes behind, and `fast_forward` alone
        // cannot tell them apart: it is false both when we are AHEAD of the
        // remote and when the histories genuinely diverged. Treating "ahead" as
        // "diverged" makes the supervisor merge-loop every tick against a
        // remote it is simply ahead of, and never push. Ask about ancestry.
        {
            let git = git.clone();
            let path = repo_path.clone();
            let reference = tracking.clone();
            let ahead =
                tokio::task::spawn_blocking(move || git.is_ancestor(&path, &reference, "HEAD"))
                    .await
                    .map_err(|err| SyncError::Journal(format!("ancestry task failed: {err}")))??;
            if ahead {
                // Nothing to apply — we hold every commit the remote has, plus
                // our own. The push leg publishes them.
                return Ok(Vec::new());
            }
        }

        if outcome.fast_forward {
            let reference = tracking.clone();
            let path = repo_path.clone();
            tokio::task::spawn_blocking(move || git.merge_ff_only(&path, &reference))
                .await
                .map_err(|err| SyncError::Journal(format!("merge task failed: {err}")))??;
            // A fast-forward takes the remote's history wholesale: nothing was
            // contested, so nothing was copied aside.
            return Ok(Vec::new());
        }

        // Diverged. A one-way lane stops here because a human deciding is the
        // entire point of a lane (AD-50).
        if profile.lane == SyncLane::Worktree || profile.direction == SyncDirection::PushOnly {
            return Err(SyncError::Diverged {
                profile: profile.name.clone(),
                reason: "the remote branch moved; a human must review this lane".to_owned(),
            });
        }

        let device = self.device.label.clone();
        let stamp = conflict_stamp(self.platform.now_ms());
        let profile_for_task = profile.clone();
        // A merge is a sync action like any other, and until now it was the one
        // commit in the history that said nothing about who made it. "Which
        // machine resolved this divergence" is exactly the question provenance
        // exists to answer.
        let provenance = Provenance::new(
            &profile.name,
            &self.device.label,
            &self.device.id,
            self.platform.host_label(),
            SyncSource::Watch,
        )
        .with_tags(profile.tags.clone());
        let conflicts = tokio::task::spawn_blocking(move || {
            Self::converge_with_conflict_copies(
                &git,
                &profile_for_task,
                &tracking,
                &stamp,
                &device,
                &provenance,
            )
        })
        .await
        .map_err(|err| SyncError::Journal(format!("converge task failed: {err}")))??;

        if conflicts.is_empty() {
            tracing::info!(profile = profile.name, "merged diverged history cleanly");
        } else {
            // Non-blocking by contract: both revisions survive, so there is
            // nothing for the user to decide before syncing continues.
            self.warn(
                &profile.id,
                &profile.name,
                format!(
                    "{} file(s) changed on both sides — your version was kept alongside as .sync-conflict-…",
                    conflicts.len()
                ),
            );
            // Until now the warning counted the copies and nothing named them,
            // so the one artifact the user has to deal with was unfindable
            // once the notification was dismissed.
            let rows: Vec<(ActivityKind, String)> = conflicts
                .iter()
                .map(|path| (ActivityKind::Conflict, path.clone()))
                .collect();
            let now = self.platform.now_ms();
            self.with_db(|conn| db::record_activity(conn, &profile.id, now, &rows))?;
        }
        Ok(conflicts)
    }

    /// Converge a diverged branch without asking anyone (AD-43).
    ///
    /// The remote keeps the canonical path and the local revision is preserved
    /// beside it, so the `-X theirs` merge that follows can never lose content:
    /// by the time it runs, every contested file already exists twice.
    ///
    /// Blocking.
    fn converge_with_conflict_copies(
        git: &GitCli,
        profile: &SyncProfile,
        tracking: &str,
        stamp: &str,
        device: &str,
        provenance: &Provenance,
    ) -> Result<Vec<String>> {
        let repo_path = &profile.local_path;
        let base = git.merge_base(repo_path, "HEAD", tracking)?;
        let ours: std::collections::HashSet<PathBuf> = git
            .diff_names(repo_path, &base, "HEAD")?
            .into_iter()
            .collect();
        let theirs: std::collections::HashSet<PathBuf> = git
            .diff_names(repo_path, &base, tracking)?
            .into_iter()
            .collect();

        let mut copied = Vec::new();
        for rela in ours.intersection(&theirs) {
            let source = repo_path.join(rela);
            // A path deleted locally has nothing to preserve.
            if !source.is_file() {
                continue;
            }
            let copy_name = git::conflict::conflict_name(rela, stamp, device);
            let destination = match rela.parent() {
                Some(parent) if !parent.as_os_str().is_empty() => {
                    repo_path.join(parent).join(&copy_name)
                }
                _ => repo_path.join(&copy_name),
            };
            std::fs::copy(&source, &destination)
                .map_err(|err| SyncError::io("write conflict copy", destination.clone(), err))?;
            copied.push(
                destination
                    .strip_prefix(repo_path)
                    .unwrap_or(&destination)
                    .to_string_lossy()
                    .into_owned(),
            );
        }

        git.merge_theirs(
            repo_path,
            tracking,
            &commit_message(
                &format!("sync({}): merge remote changes", profile.name),
                "",
                provenance,
            ),
        )?;
        Ok(copied)
    }

    /// Scan, gate and commit whatever settled. Returns how many paths landed.
    ///
    /// Shared by both legs so the working tree is always clean before a merge
    /// and always current before a push.
    fn commit_local(&self, profile: &SyncProfile) -> Result<u64> {
        if !profile.direction.pushes() {
            // A pull-only profile never commits; a local edit there is
            // preserved by the merge's own conflict handling instead.
            return Ok(0);
        }
        let staged = self.collect_stable_changes(profile)?;
        if staged.is_empty() {
            return Ok(0);
        }
        let count = staged.len() as u64;
        // The staged set is the one place in the engine where a file count is
        // known before the work happens, which makes it the only place
        // `fraction()` can mean anything for the commit and push legs.
        let mut event = self.progress(profile, SyncPhase::Committing);
        event.files_total = Some(count);
        event.current = Self::first_staged(&staged);
        self.publish(event);
        self.commit(profile, &staged)?;
        // The commit is durable, so every staged path landed. Reporting it
        // leaves the bar full rather than stranded mid-way when the phase
        // changes underneath it.
        let mut event = self.progress(profile, SyncPhase::Committing);
        event.files_total = Some(count);
        event.files_done = count;
        self.publish(event);
        Ok(count)
    }

    async fn do_push(&self, profile: &SyncProfile) -> Result<()> {
        if !profile.direction.pushes() {
            return Ok(());
        }
        let count = self.commit_local(profile)?;

        // A folder whose files are all still inside the settle window has no
        // commits yet, and neither does a fresh profile on an empty remote.
        // git rejects a push with no matching source ref as a hard error, so
        // the honest answer is to publish nothing this tick and let the next
        // one - after the files settle - do the work.
        let repo = self.open_repo(profile)?;
        if git::repo::head_commit_id(&repo)?.is_none() {
            tracing::debug!(
                profile = profile.name,
                "nothing committed yet, so there is nothing to push"
            );
            return Ok(());
        }
        drop(repo);

        // A push with nothing freshly committed is republishing commits whose
        // file count is not known without diffing the remote, and spending a
        // git invocation on a denominator is not worth it: `None` renders an
        // indeterminate meter, which is the truth.
        let mut event = self.progress(profile, SyncPhase::Pushing);
        event.files_total = (count > 0).then_some(count);
        self.publish(event);

        let git = self.git.clone();
        let repo_path = profile.local_path.clone();
        // A lane publishes ONLY its own branch. The base branch is never in
        // the refspec, so pushing over a human's work is impossible by
        // construction rather than by care (AD-50).
        let working = Self::working_branch(profile);
        let refspec = format!("refs/heads/{working}:refs/heads/{working}");
        tokio::task::spawn_blocking(move || git.push(&repo_path, "origin", &refspec))
            .await
            .map_err(|err| SyncError::Journal(format!("push task failed: {err}")))??;

        if count > 0 {
            let mut event = self.progress(profile, SyncPhase::Pushing);
            event.files_total = Some(count);
            event.files_done = count;
            self.publish(event);
        }

        if profile.lane == SyncLane::Worktree {
            // The branch is on the remote now, which is the durable artifact.
            // Opening the pull request is a separate journaled unit so a
            // failure there can never discard the push.
            let now = self.platform.now_ms();
            let unit = WorkKind::OpenPullRequest {
                branch: Self::lane_branch(profile),
            };
            self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now).map(drop))?;
        }

        if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
            snapshot.last_sync_ms = Some(self.platform.now_ms());
        }
        Ok(())
    }

    /// Pass untracked entries through, refusing anything that is not a file.
    ///
    /// This used to walk a collapsed directory itself, because gitoxide's
    /// dirwalk defaulted to `CollapseDirectory` and reported a brand-new folder
    /// as one entry. That hand-rolled walk was a **data leak**: it read the
    /// filesystem directly and knew nothing about `.gitignore`, so an ignored
    /// file inside a NEW directory was staged and pushed while the identical
    /// file one level up was correctly skipped — a new folder containing
    /// `node_modules/`, build output or a `.env` went to the remote in full.
    ///
    /// `status_paths` now asks the dirwalk to emit untracked content file by
    /// file, so git decides what is ignored and a directory never reaches here.
    /// If one somehow does, it is skipped and logged rather than walked: not
    /// syncing a folder is visible and recoverable, whereas publishing a
    /// secret because a walk ignored `.gitignore` is neither.
    fn expand_untracked(root: &Path, entries: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let mut out = Vec::with_capacity(entries.len());
        for rela in entries {
            if rela.components().any(|c| c.as_os_str() == ".git") {
                continue;
            }
            let absolute = root.join(rela);
            let metadata = match std::fs::symlink_metadata(&absolute) {
                Ok(metadata) => metadata,
                // Vanished between the walk and here: an ordinary outcome.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(SyncError::io("stat untracked entry", absolute, err)),
            };
            if metadata.is_dir() {
                tracing::warn!(
                    path = %rela.display(),
                    "sync: skipping a collapsed untracked directory — git should have listed its \
                     files individually, and walking it here would ignore .gitignore"
                );
                continue;
            }
            out.push(rela.clone());
        }
        out.sort();
        Ok(out)
    }

    /// Walk the working tree and keep only what the completeness gate passes.
    ///
    /// Blocking, and deliberately synchronous: it is pure filesystem work with
    /// no await points, so it holds no lock across a suspension.
    fn collect_stable_changes(&self, profile: &SyncProfile) -> Result<git::commit::StagedChange> {
        self.publish(self.progress(profile, SyncPhase::Scanning));
        let repo = self.open_repo(profile)?;
        let status = git::repo::status_paths(&repo)?;
        let now = self.platform.now_ms();

        let mut gates = Self::lock(&self.gates);
        let gate = match gates.get_mut(&profile.id) {
            Some(existing) => existing,
            None => {
                // First use in this process: seed the gate from `file_state`.
                // Without this a one-shot `sync --once` would never reach a
                // second observation, so every file would be held forever, and
                // an app restart would silently restart every in-flight
                // quiescence window.
                let mut fresh = StabilityGate::for_profile(profile)?;
                let saved = self.with_db(|conn| db::load_file_state(conn, &profile.id))?;
                fresh.import(saved);
                gates.insert(profile.id.clone(), fresh);
                gates
                    .get_mut(&profile.id)
                    .ok_or_else(|| SyncError::Journal("gate vanished after insert".to_owned()))?
            }
        };

        let mut staged = git::commit::StagedChange::default();
        let mut held = 0usize;
        // `added` and `untracked` both land in the same bucket, so the buckets
        // are collected first and moved into `staged` afterwards — taking two
        // simultaneous `&mut` borrows of one field would not compile.
        let mut new_paths: Vec<PathBuf> = Vec::new();
        let mut changed_paths: Vec<PathBuf> = Vec::new();
        // gitoxide's dirwalk reports untracked content with `CollapseDirectory`,
        // so a brand-new folder arrives as ONE entry naming the directory. The
        // commit path can only stage regular files and symlinks, so a collapsed
        // entry has to be expanded here — otherwise every new subdirectory
        // raises "only regular files and symlinks can be synchronized" forever
        // and nothing inside it ever syncs.
        let untracked = Self::expand_untracked(&profile.local_path, &status.untracked)?;
        let groups: [(&Vec<PathBuf>, bool); 3] = [
            (&status.added, true),
            (&untracked, true),
            (&status.modified, false),
        ];
        // Everything the gate is legitimately allowed to remember this round.
        // Anything else it still holds is stale and must be pruned, or the
        // durable cache grows entries that can never resolve.
        let mut observed: std::collections::HashSet<PathBuf> =
            std::collections::HashSet::with_capacity(untracked.len() + status.modified.len());
        for (paths, is_new) in groups {
            for rela in paths {
                let absolute = profile.local_path.join(rela);
                observed.insert(absolute.clone());
                match gate.is_stable(&absolute, now) {
                    StabilityVerdict::Stable => {
                        if is_new {
                            new_paths.push(rela.clone());
                        } else if let Some(indexed) = lfs::stage::indexed_pointer(&repo, rela) {
                            // An LFS-tracked path. git re-reads content for any
                            // entry whose mtime is not older than the index
                            // ("racily clean"), so right after staging it will
                            // report the worktree bytes as differing from the
                            // pointer blob. That is not an edit, and
                            // re-committing it would loop forever.
                            if !lfs::stage::is_false_modification(&indexed, &absolute) {
                                changed_paths.push(rela.clone());
                            }
                        } else {
                            changed_paths.push(rela.clone());
                        }
                    }
                    StabilityVerdict::Excluded | StabilityVerdict::Vanished => {}
                    StabilityVerdict::Settling { .. } => held += 1,
                    StabilityVerdict::Dataless => {
                        // Opening it would silently pull the whole object down
                        // from iCloud, so it is skipped and the user is told.
                        self.warn(
                            &profile.id,
                            &profile.name,
                            format!(
                                "{} is a cloud placeholder and was skipped — download it locally to sync it",
                                rela.display()
                            ),
                        );
                    }
                }
            }
        }
        staged.added = new_paths;
        staged.modified = changed_paths;
        // A deletion has no file left to sample, so the gate does not apply.
        staged.deleted.extend(status.deleted.iter().cloned());

        // Persist whatever is still mid-episode so the next run — this tick's
        // successor, or a whole new process — continues the window instead of
        // restarting it. The export happens while the gate lock is held and
        // the write is a short synchronous transaction, so nothing is awaited
        // in between.
        gate.retain(&observed);
        let pending = gate.export();
        drop(gates);
        self.with_db(|conn| db::save_file_state(conn, &profile.id, &pending))?;

        if held > 0 {
            tracing::debug!(profile = profile.name, held, "files still settling");
        }
        Ok(staged)
    }

    fn commit(&self, profile: &SyncProfile, staged: &git::commit::StagedChange) -> Result<()> {
        let repo = self.open_repo(profile)?;
        let (name, email) = git::commit::author_for(profile, &self.device);
        let when = gix::date::Time::new(self.platform.now_ms() / 1_000, 0);
        let author = gix::actor::Signature {
            name: name.into(),
            email: email.into(),
            time: when,
        };
        let provenance = Provenance::new(
            &profile.name,
            &self.device.label,
            &self.device.id,
            self.platform.host_label(),
            SyncSource::Watch,
        )
        .with_tags(profile.tags.clone());

        // Route anything over the threshold into LFS BEFORE the commit: the
        // blob written for those paths is the pointer, while the index entry
        // keeps the worktree file's stat so status stays clean (AD-46).
        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        let candidates: Vec<PathBuf> = staged
            .added
            .iter()
            .chain(staged.modified.iter())
            .cloned()
            .collect();
        let staging = lfs::stage::prepare(profile, &store, &candidates)?;

        // A changed `.gitattributes` must land in the SAME commit as the files
        // it governs, or a peer cloning that commit would not know the pointers
        // are pointers.
        let mut staged = staged.clone();
        if staging.attributes_changed {
            let attributes = PathBuf::from(".gitattributes");
            if !staged.added.contains(&attributes) && !staged.modified.contains(&attributes) {
                staged.added.push(attributes);
            }
        }

        let Some(id) = git::commit::stage_and_commit(
            &repo,
            &staged,
            &provenance,
            &profile.name,
            &author,
            &staging.substitutions,
        )?
        else {
            return Ok(());
        };
        tracing::info!(
            profile = profile.name,
            commit = %id,
            files = staged.len(),
            lfs = staging.uploads.len(),
            "committed"
        );

        // The commit is proven to exist, so this is the first moment an
        // activity row can honestly claim it. Recording in `commit_local`
        // instead would also cover the `stage_and_commit` → `None` case, where
        // every staged path turned out byte-identical to `HEAD` and nothing
        // was committed at all.
        //
        // `staged` is the local, `.gitattributes`-augmented copy on purpose:
        // it is exactly the set of paths this commit changed.
        self.record_commit_activity(profile, &staged)?;

        // Only now, with the pointer durably committed, is an upload worth
        // journaling: a crash before this point loses nothing, and a crash
        // after it re-drives the transfer.
        let now = self.platform.now_ms();
        for object in &staging.uploads {
            let unit = WorkKind::LfsUpload {
                oid: object.oid.clone(),
                size: object.size,
            };
            self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now).map(drop))?;
        }
        Ok(())
    }

    /// Write the recently-synced entries one commit produced (Story 32.1).
    ///
    /// The commit path is the only place that knows *which* paths moved:
    /// `commit_local` reduces the whole `StagedChange` to a count, and by the
    /// time the push leg runs the working tree is clean again and the
    /// information is gone. Recording here is what turns "3 files synced" into
    /// a list a user can actually recognise their work in.
    ///
    /// Only ever called once a commit object exists.
    fn record_commit_activity(
        &self,
        profile: &SyncProfile,
        staged: &git::commit::StagedChange,
    ) -> Result<()> {
        let mut rows: Vec<(ActivityKind, String)> = Vec::with_capacity(staged.len());
        let buckets = [
            (ActivityKind::Added, &staged.added),
            (ActivityKind::Modified, &staged.modified),
            (ActivityKind::Deleted, &staged.deleted),
        ];
        for (kind, paths) in buckets {
            for path in paths {
                // Repository-relative already — `StagedChange` holds nothing
                // else — so this never leaks a home directory into the UI.
                rows.push((kind, path.to_string_lossy().into_owned()));
            }
        }
        let now = self.platform.now_ms();
        self.with_db(|conn| db::record_activity(conn, &profile.id, now, &rows))
    }

    async fn do_lfs(
        &self,
        profile: &SyncProfile,
        oid: &str,
        size: u64,
        upload: bool,
    ) -> Result<()> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        self.publish(self.progress(profile, SyncPhase::TransferringLfs));

        // `.lfsconfig` at the repository root overrides the derived endpoint —
        // the documented precedence, and the shape a self-hosted LFS server
        // beside a non-HTTP git remote actually takes.
        let lfsconfig = match std::fs::read_to_string(profile.local_path.join(".lfsconfig")) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(SyncError::io(
                    "read .lfsconfig",
                    profile.local_path.join(".lfsconfig"),
                    err,
                ));
            }
        };
        let endpoint = lfs::endpoint::resolve(&profile.remote_url, lfsconfig.as_deref(), "origin")?;
        let auth = self
            .platform
            .secret_get(&profile.secret_key())?
            .map(|secret| format!("Bearer {secret}"));
        let client = lfs::batch::BatchClient::new(self.http.clone(), endpoint, auth.clone())
            .with_ref(format!("refs/heads/{}", profile.branch));

        let want = vec![lfs::batch::ObjectId::new(oid, size)];
        let specs = if upload {
            client.upload(&want).await?
        } else {
            client.download(&want).await?
        };

        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        store.ensure_layout()?;
        // Without a sink `Reporter::emit` returns immediately and every byte
        // counter for the largest files in the profile stays at zero. The sink
        // must be `'static` — it is cloned into the `JoinSet` that runs up to
        // `DEFAULT_CONCURRENT_TRANSFERS` objects — so it hands events back over
        // a channel rather than touching the engine directly.
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let transfer = Arc::new(
            lfs::basic::BasicTransfer::new(self.http.clone(), store.clone()).with_sink(Box::new(
                move |event| {
                    // `false` detaches the reporter for good, so this says
                    // "stop" only once the receiver is genuinely gone.
                    event_tx.send(event).is_ok()
                },
            )),
        );

        let mut tally = TransferTally::default();
        let transferring = async {
            if upload {
                Arc::clone(&transfer).upload_all(specs, auth).await
            } else {
                Arc::clone(&transfer).download_all(specs, auth).await
            }
        };
        let results = self
            .publish_while(
                profile,
                SyncPhase::TransferringLfs,
                event_rx,
                |event, transfer_event| {
                    tally.fold(&transfer_event);
                    tally.apply(event);
                },
                transferring,
            )
            .await;

        // Recorded before the failure check: bytes that crossed the wire before
        // an object failed still crossed it, and a resumed retry will not send
        // them again.
        self.add_transferred(&profile.id, tally.bytes_done());
        for (oid, result) in results {
            if let Err(err) = result {
                tracing::warn!(oid, error = %err, "lfs transfer failed");
                return Err(err);
            }
        }

        if !upload {
            // The object is in the store; the worktree still holds the pointer
            // that was checked out. Replacing it is what makes a peer's clone
            // contain real bytes rather than a text stub.
            self.materialize_pending(profile, &store)?;
        }
        Ok(())
    }

    /// Replace every checked-out pointer whose object is already local.
    ///
    /// Runs after a download and after applying remote changes. Objects that
    /// are not in the store yet are queued for transfer instead — so a partial
    /// fetch materializes what it can and returns for the rest.
    fn materialize_pending(
        &self,
        profile: &SyncProfile,
        store: &lfs::store::LfsStore,
    ) -> Result<()> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        let repo = self.open_repo(profile)?;
        let tracked = git::repo::tracked_paths(&repo)?;
        let pending = lfs::stage::pending_smudges(&profile.local_path, &tracked)?;
        if pending.is_empty() {
            return Ok(());
        }

        let now = self.platform.now_ms();
        let mut materialized = 0usize;
        for smudge in &pending {
            if store.contains(&smudge.pointer.oid, smudge.pointer.size) {
                // Pointer-only mode leaves excluded content as a pointer on
                // purpose; it is the only lever that reduces LFS traffic,
                // because git-lfs is entirely sparse-checkout-unaware.
                if profile.lfs_mode == LfsMode::PointerOnly {
                    continue;
                }
                lfs::stage::materialize(store, &profile.local_path, smudge)?;
                materialized += 1;
                continue;
            }
            if profile.lfs_mode == LfsMode::PointerOnly {
                continue;
            }
            let unit = WorkKind::LfsDownload {
                oid: smudge.pointer.oid.clone(),
                size: smudge.pointer.size,
            };
            self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now).map(drop))?;
        }
        if materialized > 0 {
            tracing::info!(
                profile = profile.name,
                materialized,
                "materialized LFS content"
            );
            // The worktree files just changed size, so the index entries carry
            // the pointer's stat and status would call every one of them
            // modified. Re-stat them against the real files.
            git::repo::refresh_index_stat(
                &repo,
                &pending.iter().map(|s| s.path.clone()).collect::<Vec<_>>(),
            )?;
        }
        Ok(())
    }

    /// Hand a pushed lane to a human as a pull request (Story 28.4, AD-50).
    ///
    /// This is the one place in the engine where a human decision is the
    /// *point* rather than a failure, so the posture inverts: the pushed branch
    /// is the durable artifact and must survive whatever happens here. A
    /// missing token, an unreachable API or an already-open request all resolve
    /// to an actionable notice naming the branch — never a rollback, never a
    /// retry storm.
    async fn do_open_pr(&self, profile: &SyncProfile, branch: &str) -> Result<()> {
        let Some(target) = forge_api_target(&profile.remote_url) else {
            self.warn(
                &profile.id,
                &profile.name,
                format!("branch {branch} is pushed and waiting for review"),
            );
            return Ok(());
        };
        let Some(token) = self.platform.secret_get(&profile.secret_key())? else {
            // Without a credential we cannot call the API, and prompting is not
            // this engine's job. Say exactly what is waiting, and where.
            self.warn(
                &profile.id,
                &profile.name,
                format!(
                    "branch {branch} is pushed; open a pull request at {}",
                    target.base
                ),
            );
            return Ok(());
        };

        let url = format!(
            "{}/api/v1/repos/{}/{}/pulls",
            target.base, target.owner, target.repo
        );
        let body = serde_json::json!({
            "head": branch,
            "base": profile.branch,
            "title": format!("{}: changes from {}", profile.name, self.device.label),
            "body": format!(
                "Opened by keeper-sync for profile `{}` on `{}`.",
                profile.name, self.device.label
            ),
        });
        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("token {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|err| SyncError::Network {
                host: target.host.clone(),
                reason: err.to_string(),
            })?;

        let status = response.status();
        if status.is_success() {
            let number = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|v| v.get("number").and_then(serde_json::Value::as_i64));
            tracing::info!(
                profile = profile.name,
                branch,
                ?number,
                "opened a pull request"
            );
            self.warn(
                &profile.id,
                &profile.name,
                match number {
                    Some(n) => format!("pull request #{n} is open for review"),
                    None => format!("a pull request is open for {branch}"),
                },
            );
            return Ok(());
        }
        // 409 is how Forgejo answers when a request for this head already
        // exists, which from the lane's point of view is success: a human
        // already has it.
        if status.as_u16() == 409 {
            tracing::debug!(
                profile = profile.name,
                branch,
                "a pull request is already open"
            );
            return Ok(());
        }
        self.warn(
            &profile.id,
            &profile.name,
            format!("branch {branch} is pushed, but opening a pull request failed ({status})"),
        );
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Public operations
    // -----------------------------------------------------------------------

    /// Run one complete sync for a profile, ignoring the schedule.
    pub async fn sync_once(&self, id: &str, source: SyncSource) -> Result<SyncOutcome> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        if !self.volume_ready(&profile)? {
            return Err(SyncError::MediaAbsent);
        }
        let _reservation = self
            .reserve(&profile.id)
            .ok_or_else(|| SyncError::Config(format!("{} is already syncing", profile.name)))?;

        tracing::info!(
            profile = profile.name,
            source = source.as_str(),
            "sync requested"
        );
        // Read before any work: `bytes` is what THIS run moved, and the counter
        // it is read from is process-lifetime cumulative.
        let transferred_before = self.transferred_bytes(&profile.id);
        let mut outcome = SyncOutcome::default();

        // Order is load-bearing: commit, then pull, then push.
        //
        // Committing first means the merge never meets a dirty tree (git
        // refuses, correctly, rather than overwriting an uncommitted edit) and
        // a divergence arrives as commit-vs-commit, which is the only shape the
        // conflict-copy path can resolve. Pulling before pushing means we never
        // hand the remote a non-fast-forward it would just reject.
        self.ensure_repo(&profile)?;
        outcome.files_changed = self.commit_local(&profile)?;
        if outcome.files_changed > 0 {
            outcome.committed = Some(profile.branch.clone());
        }

        if profile.direction.pulls() {
            outcome.conflicts = self.do_pull(&profile).await?;
            outcome.pulled = true;
            // Whatever the apply checked out may include pointers. Materialize
            // what is already local and queue the rest, BEFORE the push leg —
            // otherwise the scan would see a pointer-sized file where the index
            // records the full length and call it an edit.
            let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
            self.materialize_pending(&profile, &store)?;
        }
        if profile.direction.pushes() {
            self.do_push(&profile).await?;
            outcome.pushed = true;
        }

        // Commit may have queued LFS transfers. A one-shot sync that returned
        // here would leave the remote holding a pointer to content it does not
        // have, so the queue is drained before this call is allowed to claim
        // success.
        self.drain_journal(&profile).await?;
        outcome.bytes = self
            .transferred_bytes(&profile.id)
            .saturating_sub(transferred_before);

        self.set_state(&profile.id, ProfileState::Watching);
        self.publish(self.progress(&profile, SyncPhase::Idle));
        self.refresh_pending(&profile.id);
        Ok(outcome)
    }

    /// Re-verify stored content for a profile (Story 25.6).
    pub async fn verify(&self, id: &str) -> Result<VerifyReport> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        self.publish(self.progress(&profile, SyncPhase::Verifying));

        let root = profile.local_path.clone();
        let report = tokio::task::spawn_blocking(move || -> Result<VerifyReport> {
            let mut report = VerifyReport::default();
            let store = lfs::store::LfsStore::in_git_dir(root.join(".git"));
            let mut stack = vec![root.clone()];
            while let Some(dir) = stack.pop() {
                let entries = match std::fs::read_dir(&dir) {
                    Ok(entries) => entries,
                    Err(err) => {
                        report
                            .bad
                            .push((dir.display().to_string(), format!("unreadable: {err}")));
                        continue;
                    }
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.file_name().is_some_and(|n| n == ".git") {
                        continue;
                    }
                    if path.is_dir() {
                        stack.push(path);
                        continue;
                    }
                    report.checked += 1;
                    // A pointer's referenced object must actually be in the
                    // store at the right length, or the worktree is lying about
                    // content it claims to have.
                    if let Ok(head) = read_head(&path, lfs::pointer::MAX_POINTER_BYTES) {
                        if lfs::pointer::is_pointer_candidate(&head) {
                            if let Some(pointer) = lfs::pointer::Pointer::parse(&head) {
                                if !store.contains(&pointer.oid, pointer.size) {
                                    report.bad.push((
                                        display_relative(&root, &path),
                                        format!("LFS object {} is missing locally", pointer.oid),
                                    ));
                                }
                                continue;
                            }
                        }
                    }
                    if let Err(err) = crate::stability::verify_while_reading(&path) {
                        report
                            .bad
                            .push((display_relative(&root, &path), err.to_string()));
                    }
                }
            }
            Ok(report)
        })
        .await
        .map_err(|err| SyncError::Journal(format!("verify task failed: {err}")))??;

        self.publish(self.progress(&profile, SyncPhase::Idle));
        Ok(report)
    }

    // -----------------------------------------------------------------------
    // Visibility (Stories 32.1, 32.2)
    //
    // Three questions a user asks about a folder that syncs itself: what has
    // it just done, what is it about to do, and what has gone wrong. Only the
    // first is stored — the other two are derived on demand from the state
    // that already decides the engine's behaviour, so a visible answer can
    // never drift from the real one (AD-S3).
    // -----------------------------------------------------------------------

    /// The most recent files this profile synced, newest first (Story 32.1).
    pub async fn activity(&self, profile_id: &str, limit: usize) -> Result<Vec<ActivityRow>> {
        self.with_db(|conn| db::list_activity(conn, profile_id, limit))
    }

    /// Everything this profile has not synced yet, and why (Story 32.2).
    ///
    /// Computed, never stored. A stored pending list would be a second answer
    /// to a question git and the completeness gate already answer, and the two
    /// would disagree the moment a file changed while the app was closed.
    ///
    /// The two sources overlap on purpose: a path can be dirty *and* still
    /// inside its settle window, and [`PendingReason::Settling`] wins there
    /// because it is the reason the file is not moving. Saying "modified"
    /// about a file the engine is deliberately holding would make the user
    /// think sync was broken.
    pub async fn pending(&self, profile_id: &str) -> Result<Vec<PendingFile>> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            return Err(SyncError::Config(format!(
                "no such sync profile: {profile_id}"
            )));
        };

        // Settling paths are absolute in `file_state` (that is what the gate
        // samples), and everything the user sees must be repository-relative.
        let settling = self.with_db(|conn| db::load_file_state(conn, profile_id))?;
        let mut out: Vec<PendingFile> = Vec::with_capacity(settling.len());
        let mut named: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(settling.len());
        for (path, entry) in settling {
            let relative = path
                .strip_prefix(&profile.local_path)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if named.insert(relative.clone()) {
                out.push(PendingFile {
                    path: relative,
                    reason: PendingReason::Settling {
                        since_ms: entry.pending_since_ms,
                    },
                });
            }
        }

        // A folder that is not a repository yet has nothing git can classify,
        // and materializing one is a clone — far too much for a poll. The
        // first sync adopts it and the next call is complete.
        if profile.local_path.join(".git").exists() {
            let repo_path = profile.local_path.clone();
            let removable = profile.removable;
            // The status walk and the untracked expansion are both blocking
            // filesystem work on a tree that may hold a hundred thousand
            // files; running them on the async runtime would stall every other
            // profile while a UI poll finished.
            let (status, untracked) = tokio::task::spawn_blocking(
                move || -> Result<(git::repo::RepoStatus, Vec<PathBuf>)> {
                    let repo = git::repo::open(&repo_path, removable)?;
                    let status = git::repo::status_paths(&repo)?;
                    // gitoxide collapses a brand-new folder into one entry
                    // naming the directory; listing that would tell the user
                    // "sub/" is waiting instead of the files they created.
                    let untracked = Self::expand_untracked(&repo_path, &status.untracked)?;
                    Ok((status, untracked))
                },
            )
            .await
            .map_err(|err| SyncError::Journal(format!("pending scan task failed: {err}")))??;

            let buckets: [(&Vec<PathBuf>, PendingReason); 4] = [
                (&status.added, PendingReason::Added),
                (&status.modified, PendingReason::Modified),
                (&status.deleted, PendingReason::Deleted),
                (&untracked, PendingReason::Untracked),
            ];
            for (paths, reason) in buckets {
                for rela in paths {
                    let relative = rela.to_string_lossy().into_owned();
                    // Already named as settling, or reported by two status
                    // buckets at once (staged as added, then edited again).
                    if named.insert(relative.clone()) {
                        out.push(PendingFile {
                            path: relative,
                            reason: reason.clone(),
                        });
                    }
                }
            }
        }

        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Everything currently wrong with this profile (Story 32.2).
    pub async fn problems(&self, profile_id: &str) -> Result<ProblemReport> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            return Err(SyncError::Config(format!(
                "no such sync profile: {profile_id}"
            )));
        };
        let snapshot = Self::lock(&self.status).get(profile_id).cloned();

        let parked = self
            .with_db(|conn| db::list_parked(conn, profile_id))?
            .into_iter()
            .map(|row| ParkedUnit {
                id: row.id,
                kind: row.kind,
                attempts: row.attempts,
                last_error: row.last_error,
            })
            .collect();

        // A conflict copy the user has already dealt with — merged by hand and
        // deleted, or simply thrown away — is resolved, and keeping it on a
        // problems list would make the list impossible to ever clear. The file
        // still being on disk IS the open-problem condition.
        let recent = self.with_db(|conn| db::list_activity(conn, profile_id, db::ACTIVITY_CAP))?;
        let root = profile.local_path.clone();
        let conflicts = tokio::task::spawn_blocking(move || {
            recent
                .into_iter()
                .filter(|row| row.kind == ActivityKind::Conflict)
                .filter(|row| root.join(&row.path).exists())
                .map(|row| row.path)
                .collect::<Vec<String>>()
        })
        .await
        .map_err(|err| SyncError::Journal(format!("conflict scan task failed: {err}")))?;

        Ok(ProblemReport {
            warning: snapshot.as_ref().and_then(|s| s.warning.clone()),
            error: snapshot.and_then(|s| s.error),
            parked,
            conflicts,
        })
    }

    /// Put one parked unit back in the queue (Story 32.2).
    ///
    /// Scoped to the profile that owns it, so a caller holding one profile's
    /// id can never re-drive another's work. Refusing loudly rather than
    /// silently succeeding matters: a UI that showed "retrying" for a unit
    /// nothing was done to would be lying.
    pub async fn retry_parked(&self, profile_id: &str, unit_id: i64) -> Result<()> {
        let moved = self.with_db(|conn| db::unpark(conn, profile_id, unit_id))?;
        if !moved {
            return Err(SyncError::Config(format!(
                "work item {unit_id} is not parked work belonging to {profile_id}"
            )));
        }
        // The unit is claimable again, so the profile is no longer stopped on
        // it — reflect that in the count the tray polls rather than waiting for
        // the next tick to notice.
        self.refresh_pending(profile_id);
        Ok(())
    }

    /// Does the local branch hold commits the remote-tracking ref does not?
    ///
    /// Answered from the local clone alone — no network — so it is safe to ask
    /// on every tick, including while offline. A missing tracking ref (nothing
    /// fetched yet) counts as "unpushed": there is provably nothing on the
    /// remote side to compare against.
    fn has_unpushed_commits(&self, profile: &SyncProfile) -> Result<bool> {
        let tracking = format!("refs/remotes/origin/{}", profile.branch);
        match self.git.is_ancestor(&profile.local_path, "HEAD", &tracking) {
            Ok(contained) => Ok(!contained),
            // An unborn branch or an absent tracking ref makes the question
            // unanswerable rather than false. That is not a failure: let the
            // push leg decide, since it is a no-op when there is genuinely
            // nothing to send.
            Err(SyncError::GitCommand { .. }) => Ok(false),
            Err(other) => Err(other),
        }
    }

    /// Queue the work a profile needs, based on what the tree looks like now.
    fn scan_and_enqueue(&self, profile: &SyncProfile) -> Result<()> {
        let now = self.platform.now_ms();
        // A pointer left in the worktree by an earlier apply is work too, and
        // the supervisor is the only thing that will notice it.
        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        if let Err(err) = self.materialize_pending(profile, &store) {
            tracing::warn!(profile = profile.name, error = %err, "could not materialize LFS content");
        }
        if profile.direction.pulls() {
            self.with_db(|conn| {
                db::enqueue_unique(conn, &profile.id, &WorkKind::Pull, now, now).map(drop)
            })?;
        }
        if profile.direction.pushes() {
            // A push is needed when the tree has settled changes to commit OR
            // when commits already exist that the remote has not seen. Only
            // checking the former stranded work permanently: once a change was
            // committed the tree went clean, no push was ever queued, and the
            // local branch sat ahead of the remote forever.
            let staged = self.collect_stable_changes(profile)?;
            let unpushed = staged.is_empty() && self.has_unpushed_commits(profile)?;
            if !staged.is_empty() || unpushed {
                self.with_db(|conn| {
                    db::enqueue_unique(conn, &profile.id, &WorkKind::Push, now, now).map(drop)
                })?;
            }
        }
        self.refresh_pending(&profile.id);
        Ok(())
    }

    /// The first staged path, for the progress detail line.
    ///
    /// Repository-relative by construction: `StagedChange` holds nothing else,
    /// and an absolute path here would leak home directory names into logs and
    /// screenshots.
    fn first_staged(staged: &git::commit::StagedChange) -> Option<String> {
        staged
            .added
            .iter()
            .chain(staged.modified.iter())
            .chain(staged.deleted.iter())
            .next()
            .map(|path| path.to_string_lossy().into_owned())
    }

    /// Add to a profile's cumulative transferred-byte counter.
    fn add_transferred(&self, profile_id: &str, bytes: u64) {
        if bytes == 0 {
            return;
        }
        let mut totals = Self::lock(&self.transferred);
        let total = totals.entry(profile_id.to_owned()).or_insert(0);
        *total = total.saturating_add(bytes);
    }

    fn transferred_bytes(&self, profile_id: &str) -> u64 {
        Self::lock(&self.transferred)
            .get(profile_id)
            .copied()
            .unwrap_or(0)
    }

    /// Publish `phase` progress from `events` while `work` runs, then return
    /// its result.
    ///
    /// Both producers that can actually measure transfer volume — gitoxide's
    /// progress tree and the LFS transfer sink — live behind a `'static`
    /// boundary (`spawn_blocking`, and a `JoinSet` inside `BasicTransfer`), so
    /// neither can borrow the engine to publish for itself. A channel is the
    /// cheapest bridge that does not force `Engine` into an `Arc`.
    ///
    /// Everything already queued is drained before a single snapshot goes out.
    /// Each producer is throttled at source (100 ms per node in `git::fetch`,
    /// `DEFAULT_PROGRESS_INTERVAL` per object in `lfs::basic`), but eight
    /// concurrent objects still tick independently against a tray that
    /// repaints at ~1 Hz; draining first turns that burst into one publish
    /// instead of eight.
    async fn publish_while<T, E, F>(
        &self,
        profile: &SyncProfile,
        phase: SyncPhase,
        mut events: tokio::sync::mpsc::UnboundedReceiver<E>,
        mut fold: impl FnMut(&mut SyncProgress, E),
        work: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        let mut event = self.progress(profile, phase);
        tokio::pin!(work);
        loop {
            tokio::select! {
                outcome = &mut work => {
                    // The terminal events — the `Completed` that retires an
                    // object's last bytes — are still queued when the work
                    // future resolves, because the producer emits them on its
                    // way out. Returning without them leaves the final frame
                    // reporting less than actually transferred.
                    let mut trailing = false;
                    while let Ok(next) = events.try_recv() {
                        fold(&mut event, next);
                        trailing = true;
                    }
                    if trailing {
                        self.publish(event);
                    }
                    return outcome;
                }
                // A `None` from the channel disables this branch rather than
                // spinning: the producer finished, and `work` is still pending.
                Some(first) = events.recv() => {
                    fold(&mut event, first);
                    while let Ok(next) = events.try_recv() {
                        fold(&mut event, next);
                    }
                    self.publish(event.clone());
                }
            }
        }
    }

    fn progress(&self, profile: &SyncProfile, phase: SyncPhase) -> SyncProgress {
        let mut event = SyncProgress::idle(&profile.id, &profile.name);
        event.phase = phase;
        event
    }
}

/// Releases a profile's one-operation-at-a-time reservation on every exit path,
/// including a panic — the `LiveFolderReservation` idiom from the shell.
struct Reservation<'a> {
    engine: &'a Engine,
    profile_id: String,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        Engine::lock(&self.engine.busy).remove(&self.profile_id);
    }
}

/// UTC `yyyymmdd-hhmmss` for a conflict filename.
///
/// Hand-rolled because `keeper-sync` deliberately has no `chrono`: the engine
/// is time-agnostic and takes wall-clock milliseconds from the platform port.
/// Civil-date arithmetic from a Unix timestamp is Howard Hinnant's
/// `days_from_civil` inverse, which is exact for every date we can represent.
fn conflict_stamp(now_ms: i64) -> String {
    let secs = now_ms.div_euclid(1_000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (tod / 3_600, (tod % 3_600) / 60, tod % 60);

    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = era * 400 + yoe + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{hh:02}{mm:02}{ss:02}")
}

fn read_head(path: &Path, cap: usize) -> Result<Vec<u8>> {
    use std::io::Read as _;
    let mut file =
        std::fs::File::open(path).map_err(|err| SyncError::io("open for inspection", path, err))?;
    let mut buffer = vec![0u8; cap];
    let read = file
        .read(&mut buffer)
        .map_err(|err| SyncError::io("read header", path, err))?;
    buffer.truncate(read);
    Ok(buffer)
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_run_of_transient_failures_stops_calling_the_profile_healthy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        // A single blip must stay quiet: it is retried, and warning about
        // something that fixes itself trains people to ignore warnings.
        let err = SyncError::Git("could not write the index".to_owned());
        engine.record_failure(&p, &err);
        let snapshot = engine.status(&p.id).expect("a status");
        assert!(
            snapshot.warning.is_none(),
            "one retriable failure is not worth alarming anyone about"
        );

        // A run of them is a profile that has silently stopped syncing.
        for _ in 1..TRANSIENT_FAILURES_BEFORE_WARNING {
            engine.record_failure(&p, &err);
        }
        let snapshot = engine.status(&p.id).expect("a status");
        let warning = snapshot
            .warning
            .expect("a profile that keeps failing must say so");
        assert!(
            warning.contains("could not write the index"),
            "the warning must name the actual cause, got: {warning}"
        );

        // And it must be exactly one notification, not one per tick.
        let notifications = platform
            .notifications
            .lock()
            .map(|n| n.len())
            .unwrap_or_default();
        assert_eq!(notifications, 1, "a sustained failure notifies once");
    }

    #[test]
    fn a_merge_commit_carries_the_same_provenance_as_any_other() {
        // The merge used to be the one commit in the history that said nothing
        // about which machine made it - the exact question provenance exists to
        // answer, missed precisely where two machines disagreed.
        let provenance = Provenance::new(
            "media",
            "electra",
            "01KYDKP6SN2HR4SJBJ9JTBVC2Z",
            "electra",
            SyncSource::Watch,
        );
        let message = commit_message("sync(media): merge remote changes", "", &provenance);

        assert!(message.starts_with("sync(media): merge remote changes\n"));
        let parsed = Provenance::parse(&message).expect("a merge commit is attributable");
        assert_eq!(parsed.device_label, "electra");
        assert_eq!(parsed.device_id, "01KYDKP6SN2HR4SJBJ9JTBVC2Z");
        assert_eq!(parsed.profile, "media");
    }

    use super::*;
    use crate::platform::TestPlatform;

    fn engine(dir: &Path) -> Option<Engine> {
        let platform = Arc::new(TestPlatform::new(dir));
        // A machine without a usable git cannot host the engine at all, which
        // is exactly AD-41's contract — skip rather than fake it.
        Engine::open(platform).ok()
    }

    fn profile(dir: &Path) -> SyncProfile {
        SyncProfile::new(
            "01JTESTPROFILE",
            "fixture",
            dir.join("work"),
            "https://git.invalid/x/y.git",
        )
    }

    #[test]
    fn forge_api_targets_are_derived_from_both_remote_shapes() {
        let https = forge_api_target("https://forgejo.example.com/dev/notes.git").expect("https");
        assert_eq!(https.base, "https://forgejo.example.com");
        assert_eq!(
            (https.owner.as_str(), https.repo.as_str()),
            ("dev", "notes")
        );

        // scp-style is not a URL and has to be split by hand.
        let scp = forge_api_target("git@forgejo.example.com:dev/notes.git").expect("scp");
        assert_eq!(scp.base, "https://forgejo.example.com");
        assert_eq!((scp.owner.as_str(), scp.repo.as_str()), ("dev", "notes"));

        // A token in the URL must never be rebuilt into the API base.
        let userinfo =
            forge_api_target("https://tok3n:x@forgejo.example.com/dev/notes").expect("userinfo");
        assert_eq!(userinfo.base, "https://forgejo.example.com");
        assert!(!userinfo.base.contains("tok3n"));

        // Nothing to open a pull request against.
        assert!(forge_api_target("/srv/git/notes.git").is_none());
        assert!(forge_api_target("file:///srv/git/notes.git").is_none());
        assert!(forge_api_target("https://host/onlyowner").is_none());
    }

    #[test]
    fn a_lane_branch_is_stable_unique_and_never_the_base_branch() {
        let mut p = SyncProfile::new("01JABC", "Agent Drafts!", "/w", "https://git.invalid/r.git");
        p.direction = SyncDirection::PushOnly;
        p.lane = SyncLane::Worktree;

        let branch = Engine::lane_branch(&p);
        assert_eq!(
            branch, "keeper/Agent-Drafts/01JABC",
            "punctuation is folded and edges trimmed"
        );
        assert_ne!(
            branch, p.branch,
            "a lane must never publish on the base branch"
        );
        assert_eq!(branch, Engine::lane_branch(&p), "stable across calls");
        assert_eq!(Engine::working_branch(&p), branch);

        // A normal profile writes on the branch it tracks.
        p.lane = SyncLane::Main;
        p.direction = SyncDirection::Bidirectional;
        assert_eq!(Engine::working_branch(&p), p.branch);
    }

    #[test]
    fn a_collapsed_directory_is_skipped_rather_than_walked_without_ignore_rules() {
        // This used to expand recursively, and that was a data leak: the walk
        // read the filesystem directly, so a `.gitignore`d file inside a NEW
        // folder was staged and pushed while the same file one level up was
        // correctly skipped. `status_paths` now has git list untracked content
        // file by file, so a directory should never arrive here — and if one
        // does, not syncing it is visible and recoverable, where publishing a
        // secret is neither.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub/deeper")).expect("mkdir");
        std::fs::create_dir_all(root.join(".git/objects")).expect("mkdir .git");
        std::fs::write(root.join("top.txt"), b"x").expect("write");
        std::fs::write(root.join("sub/a.txt"), b"x").expect("write");
        std::fs::write(root.join(".git/objects/pack"), b"x").expect("write");

        let expanded = Engine::expand_untracked(
            root,
            &[
                PathBuf::from("top.txt"),
                PathBuf::from("sub"),
                PathBuf::from(".git"),
            ],
        )
        .expect("expand");

        assert_eq!(
            expanded,
            vec![PathBuf::from("top.txt")],
            "files pass through; a directory is refused, not walked"
        );
    }

    #[test]
    fn expanding_tolerates_an_entry_that_vanished() {
        // The walk and the expansion are not atomic; a deleted file in between
        // is an ordinary outcome, not an error.
        let dir = tempfile::tempdir().expect("tempdir");
        let expanded =
            Engine::expand_untracked(dir.path(), &[PathBuf::from("gone.txt")]).expect("expand");
        assert!(expanded.is_empty());
    }

    #[test]
    fn conflict_stamps_are_correct_utc_civil_dates() {
        // Hand-rolled civil-date arithmetic is exactly the kind of code that is
        // silently wrong for years, so pin real epochs including a leap day and
        // the pre-epoch direction.
        assert_eq!(conflict_stamp(0), "19700101-000000");
        assert_eq!(conflict_stamp(1_000), "19700101-000001");
        assert_eq!(conflict_stamp(951_782_400_000), "20000229-000000");
        assert_eq!(conflict_stamp(1_709_164_800_000), "20240229-000000");
        assert_eq!(conflict_stamp(1_753_444_800_000), "20250725-120000");
        // A clock before the epoch must not produce a negative-looking name.
        assert_eq!(conflict_stamp(-1_000), "19691231-235959");
    }

    #[test]
    fn a_machine_without_git_cannot_open_the_engine() {
        // AD-41: git is a declared prerequisite, and discovering it missing
        // mid-push would leave a profile half-applied.
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()).without_git());
        let Err(err) = Engine::open(platform) else {
            panic!("a machine without git must not yield an engine");
        };
        assert_eq!(err.code(), "gitMissing");
    }

    #[test]
    fn profiles_round_trip_and_pausing_is_reflected_in_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(engine.list_profiles().expect("list").len(), 1);
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Idle
        );

        engine.set_enabled(&p.id, false).expect("pause");
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Paused
        );
        engine.set_enabled(&p.id, true).expect("resume");
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Idle
        );
    }

    #[test]
    fn a_profile_can_only_have_one_operation_in_flight() {
        // Two operations on one working tree would race on the index.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let first = engine.reserve("p").expect("first reservation");
        assert!(engine.reserve("p").is_none(), "second must be refused");
        assert!(
            engine.reserve("other").is_some(),
            "other profiles are unaffected"
        );
        drop(first);
        assert!(engine.reserve("p").is_some(), "released on drop");
    }

    #[test]
    fn an_unknown_profile_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        assert!(engine.status("nope").is_err());
        assert!(engine.set_enabled("nope", true).is_err());
    }

    #[test]
    fn removing_a_profile_clears_its_status_and_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        engine.remove_profile(&p.id).expect("remove");
        assert!(engine.statuses().expect("statuses").is_empty());
    }

    #[test]
    fn a_dead_progress_sink_is_dropped_rather_than_retained_forever() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        engine.subscribe(Box::new(|_| false));
        let live = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&live);
        engine.subscribe(Box::new(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        }));

        engine.publish(engine.progress(&p, SyncPhase::Pushing));
        engine.publish(engine.progress(&p, SyncPhase::Pushing));
        assert_eq!(
            Engine::lock(&engine.sinks).len(),
            1,
            "the closed sink must be dropped"
        );
        assert_eq!(live.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn progress_folds_into_the_polled_snapshot_for_the_tray() {
        // The tray must render correctly with no webview subscribed.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let mut event = engine.progress(&p, SyncPhase::TransferringLfs);
        event.bytes_done = 42;
        engine.publish(event);
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.phase, SyncPhase::TransferringLfs);
        assert_eq!(snapshot.bytes_done, 42);
        assert_eq!(snapshot.state, ProfileState::Syncing);
    }

    #[test]
    fn a_warning_notifies_once_per_onset_not_once_per_tick() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        engine.warn(&p.id, &p.name, "rename required".to_owned());
        engine.warn(&p.id, &p.name, "rename required".to_owned());
        let count = platform
            .notifications
            .lock()
            .map(|n| n.len())
            .unwrap_or_default();
        assert_eq!(count, 1, "a sustained warning must notify exactly once");

        engine.clear_warning(&p.id);
        engine.warn(&p.id, &p.name, "rename required".to_owned());
        let count = platform
            .notifications
            .lock()
            .map(|n| n.len())
            .unwrap_or_default();
        assert_eq!(count, 2, "a recurrence after clearing must notify again");
    }

    #[test]
    fn an_absent_removable_volume_pauses_instead_of_failing() {
        // The single most important behaviour in the whole subsystem: an
        // unplugged drive must never be read as a mass deletion (AD-48).
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.removable = true;
        std::fs::create_dir_all(&p.local_path).expect("create work dir");
        engine.upsert_profile(&p).expect("upsert");

        assert!(
            !engine.volume_ready(&p).expect("scan"),
            "must skip, not error"
        );
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::MediaAbsent
        );
    }

    #[test]
    fn a_non_removable_profile_never_consults_the_volume_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        assert!(engine.volume_ready(&p).expect("scan"));
    }

    #[test]
    fn deferred_work_is_not_rescheduled_on_a_timer() {
        // Backing off against an absent volume would spin every tick forever.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0))
            .expect("enqueue");
        engine
            .reschedule_after(&p, id, 1, &SyncError::MediaAbsent)
            .expect("reschedule");

        let far_future = engine.platform.now_ms() + 86_400_000;
        let claimed = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, far_future, 10))
            .expect("claim");
        assert!(
            claimed.is_empty(),
            "deferred work waits on the volume, not the clock"
        );
    }

    #[test]
    fn a_permanent_failure_parks_the_unit_and_flags_the_profile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0))
            .expect("enqueue");
        engine
            .reschedule_after(
                &p,
                id,
                1,
                &SyncError::Auth {
                    host: "git.invalid".to_owned(),
                },
            )
            .expect("reschedule");

        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::NeedsAttention
        );
        let far_future = engine.platform.now_ms() + 86_400_000;
        assert!(
            engine
                .with_db(|conn| db::claim_ready(conn, &p.id, far_future, 10))
                .expect("claim")
                .is_empty(),
            "a parked unit must never be retried unchanged"
        );
    }

    #[test]
    fn a_network_failure_reads_as_offline_not_as_broken() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        engine.record_failure(
            &p,
            &SyncError::Network {
                host: "git.invalid".to_owned(),
                reason: "connection reset".to_owned(),
            },
        );
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.state, ProfileState::Offline);
        assert!(snapshot.error.is_none(), "offline is a state, not an error");
    }

    #[test]
    fn interrupted_work_is_requeued_when_the_engine_reopens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(first) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        first.upsert_profile(&p).expect("upsert");
        first
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0))
            .expect("enqueue");
        first
            .with_db(|conn| db::claim_ready(conn, &p.id, 0, 10))
            .expect("claim");
        drop(first);

        let Some(second) = engine(dir.path()) else {
            return;
        };
        let claimed = second
            .with_db(|conn| db::claim_ready(conn, &p.id, second.platform.now_ms(), 10))
            .expect("claim");
        assert_eq!(
            claimed.len(),
            1,
            "work interrupted by a restart must come back"
        );
    }

    /// A profile whose folder exists and can be adopted in place, so the
    /// commit path runs for real against a local repository with no remote
    /// reachable — adoption is `git init` plus a remote config, never network.
    fn adoptable(dir: &Path) -> SyncProfile {
        let p = profile(dir);
        std::fs::create_dir_all(&p.local_path).expect("work dir");
        p
    }

    /// Drive the commit path to the point where the gate lets a file through.
    ///
    /// The gate needs two identical observations a settle window apart, so one
    /// pass only opens the episode. Returns how many paths the commit carried.
    fn commit_after_settling(engine: &Engine, platform: &TestPlatform, p: &SyncProfile) -> u64 {
        engine
            .commit_local(p)
            .expect("first pass opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine.commit_local(p).expect("second pass commits")
    }

    #[tokio::test]
    async fn a_commit_records_exactly_the_paths_it_carried() {
        // `commit_local` reduces a whole `StagedChange` to a count and the
        // working tree goes clean immediately afterwards, so this is the only
        // moment the individual paths still exist anywhere.
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("a.txt"), b"one").expect("write");
        std::fs::write(p.local_path.join("b.txt"), b"two").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        engine.commit_local(&p).expect("first pass");
        assert!(
            engine
                .activity(&p.id, 10)
                .await
                .expect("activity")
                .is_empty(),
            "nothing may be recorded before a commit exists"
        );

        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(engine.commit_local(&p).expect("commit"), 2);

        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let mut seen: Vec<(ActivityKind, String)> =
            rows.iter().map(|r| (r.kind, r.path.clone())).collect();
        seen.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            seen,
            vec![
                (ActivityKind::Added, "a.txt".to_owned()),
                (ActivityKind::Added, "b.txt".to_owned()),
            ],
            "exactly the committed paths, repository-relative, as additions"
        );
        assert!(
            rows.iter().all(|r| r.ts_ms == platform.now_ms()),
            "the timestamp comes from the platform clock, not the wall clock"
        );

        // A second round: one path edited, one removed. Both must be reported
        // as what they were, not as another addition.
        //
        // They land in two commits, not one: a deletion has no file left to
        // sample, so the gate does not apply to it and it goes out at once,
        // while the edit has to serve a fresh settle window.
        std::fs::write(p.local_path.join("a.txt"), b"one edited").expect("edit");
        std::fs::remove_file(p.local_path.join("b.txt")).expect("remove");
        assert_eq!(engine.commit_local(&p).expect("removal"), 1);
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(engine.commit_local(&p).expect("edit"), 1);

        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let mut latest: Vec<(ActivityKind, String)> = rows
            .iter()
            .take(2)
            .map(|r| (r.kind, r.path.clone()))
            .collect();
        latest.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            latest,
            vec![
                (ActivityKind::Modified, "a.txt".to_owned()),
                (ActivityKind::Deleted, "b.txt".to_owned()),
            ],
            "newest first, and each path carries the kind that actually happened"
        );
        assert_eq!(rows.len(), 4, "earlier entries are kept, not replaced");
    }

    #[tokio::test]
    async fn a_commit_that_turned_out_empty_records_nothing() {
        // `stage_and_commit` returns `None` when every staged path is
        // byte-identical to `HEAD`. An activity row there would claim a commit
        // that does not exist.
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("a.txt"), b"one").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        let before = engine.activity(&p.id, 10).await.expect("activity").len();

        // Rewrite the identical bytes: the scan sees a changed stat, the commit
        // machinery sees no change at all.
        std::fs::write(p.local_path.join("a.txt"), b"one").expect("rewrite");
        commit_after_settling(&engine, &platform, &p);

        assert_eq!(
            engine.activity(&p.id, 10).await.expect("activity").len(),
            before,
            "no commit, no activity"
        );
    }

    #[tokio::test]
    async fn a_file_inside_its_settle_window_is_pending_as_settling() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("held.txt"), b"still writing").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // One pass opens the quiescence episode and persists it to `file_state`.
        let opened_at = platform.now_ms();
        engine.commit_local(&p).expect("scan");

        let pending = engine.pending(&p.id).await.expect("pending");
        assert_eq!(
            pending,
            vec![PendingFile {
                path: "held.txt".to_owned(),
                reason: PendingReason::Settling {
                    since_ms: opened_at
                },
            }],
            "a held file reports why it is held and since when — not `untracked`, \
             which would read as sync being broken"
        );

        // Once it settles and lands, it is not pending at all any more.
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(engine.commit_local(&p).expect("commit"), 1);
        assert!(engine.pending(&p.id).await.expect("pending").is_empty());
    }

    #[tokio::test]
    async fn pending_names_the_files_in_a_new_folder_not_the_folder() {
        // gitoxide collapses untracked content into one entry naming the
        // directory; listing that would tell the user "sub" is waiting.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::create_dir_all(p.local_path.join("sub/deeper")).expect("mkdir");
        std::fs::write(p.local_path.join("sub/deeper/x.txt"), b"x").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        engine.ensure_repo(&p).expect("adopt");

        let pending = engine.pending(&p.id).await.expect("pending");
        assert_eq!(
            pending,
            vec![PendingFile {
                path: "sub/deeper/x.txt".to_owned(),
                reason: PendingReason::Untracked,
            }]
        );
    }

    #[tokio::test]
    async fn pending_refuses_an_unknown_profile_rather_than_answering_nothing() {
        // "No such profile" and "nothing pending" are very different answers.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        assert!(engine.pending("nope").await.is_err());
        assert!(engine.problems("nope").await.is_err());
    }

    #[tokio::test]
    async fn a_conflict_copy_is_a_problem_only_while_it_is_still_on_disk() {
        // A copy the user already merged by hand and deleted is resolved, and
        // a problems list that could never be cleared is a list people learn
        // to ignore.
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        // Exactly what `do_pull` writes when the apply has to preserve a local
        // revision beside the remote's.
        let copy = "notes.sync-conflict-20250725-120000-test-host.md";
        std::fs::write(p.local_path.join(copy), b"my revision").expect("write copy");
        engine
            .with_db(|conn| {
                db::record_activity(
                    conn,
                    &p.id,
                    platform.now_ms(),
                    &[(ActivityKind::Conflict, copy.to_owned())],
                )
            })
            .expect("record");

        let report = engine.problems(&p.id).await.expect("problems");
        assert_eq!(report.conflicts, vec![copy.to_owned()]);

        std::fs::remove_file(p.local_path.join(copy)).expect("resolve it");
        let report = engine.problems(&p.id).await.expect("problems");
        assert!(
            report.conflicts.is_empty(),
            "a conflict copy the user dealt with is not a problem any more"
        );
        // The history of it happening survives; only the open problem clears.
        let rows = engine.activity(&p.id, 10).await.expect("activity");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, ActivityKind::Conflict);
    }

    #[tokio::test]
    async fn problems_surfaces_parked_work_that_the_pending_count_hides() {
        // Parked units are deliberately excluded from `pending_count`, so
        // without this they have no surface at all: the profile looks idle
        // while its work sits stopped forever.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0))
            .expect("enqueue");
        engine
            .reschedule_after(
                &p,
                id,
                1,
                &SyncError::Auth {
                    host: "git.invalid".to_owned(),
                },
            )
            .expect("park");

        assert_eq!(engine.status(&p.id).expect("status").pending, 0);
        let report = engine.problems(&p.id).await.expect("problems");
        assert_eq!(report.parked.len(), 1);
        assert_eq!(report.parked[0].id, id);
        assert_eq!(report.parked[0].kind, "push");
        assert!(
            report.parked[0]
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("git.invalid")),
            "a parked unit must say why it stopped, got: {:?}",
            report.parked[0].last_error
        );
        assert_eq!(report.error, engine.status(&p.id).expect("status").error);
    }

    #[tokio::test]
    async fn retrying_parked_work_requeues_it_and_never_crosses_profiles() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, 0, 0))
            .expect("enqueue");
        engine
            .with_db(|conn| db::claim_ready(conn, &p.id, 0, 10))
            .expect("claim");
        engine
            .with_db(|conn| db::reschedule(conn, id, WorkState::Parked, i64::MAX, Some("no auth")))
            .expect("park");

        // Holding another profile's id must never be enough to re-drive this.
        assert!(
            engine.retry_parked("01JOTHERPROFILE", id).await.is_err(),
            "one profile must never retry another's work"
        );
        assert_eq!(
            engine.problems(&p.id).await.expect("problems").parked.len(),
            1,
            "the refused retry must not have moved anything"
        );

        engine.retry_parked(&p.id, id).await.expect("retry");
        assert!(engine
            .problems(&p.id)
            .await
            .expect("problems")
            .parked
            .is_empty());
        let claimed = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, 0, 10))
            .expect("claim");
        assert_eq!(
            claimed.len(),
            1,
            "`not_before_ms` is cleared, so the retry is ready now rather than \
             at the parked unit's old deadline"
        );
        assert_eq!(engine.status(&p.id).expect("status").pending, 1);

        // A unit that is not parked is refused loudly: a UI that showed
        // "retrying" for a unit nothing happened to would be lying.
        assert!(engine.retry_parked(&p.id, id).await.is_err());
    }

    #[test]
    fn the_visibility_types_cross_the_ipc_boundary_as_camel_case() {
        // These are rendered directly by the webview, so the field spelling is
        // a contract, not an implementation detail. `PendingReason` is
        // internally tagged so it arrives as a discriminated union rather than
        // as `reason.reason`.
        let pending = PendingFile {
            path: "notes/a.md".to_owned(),
            reason: PendingReason::Settling { since_ms: 42 },
        };
        assert_eq!(
            serde_json::to_string(&pending).expect("serialize"),
            r#"{"path":"notes/a.md","reason":{"kind":"settling","sinceMs":42}}"#
        );

        let report = ProblemReport {
            warning: None,
            error: Some("boom".to_owned()),
            parked: vec![ParkedUnit {
                id: 7,
                kind: "lfsUpload".to_owned(),
                attempts: 3,
                last_error: Some("401".to_owned()),
            }],
            conflicts: vec!["a.sync-conflict-x.md".to_owned()],
        };
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains(r#""lastError":"401""#), "got: {json}");
        assert!(json.contains(r#""attempts":3"#), "got: {json}");

        let row = ActivityRow {
            ts_ms: 1,
            kind: ActivityKind::Conflict,
            path: "a.md".to_owned(),
        };
        assert_eq!(
            serde_json::to_string(&row).expect("serialize"),
            r#"{"tsMs":1,"kind":"conflict","path":"a.md"}"#
        );
    }

    /// Everything published while `body` runs, in order.
    fn recording(engine: &Engine) -> Arc<Mutex<Vec<SyncProgress>>> {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&log);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event);
            true
        }));
        log
    }

    #[tokio::test]
    async fn a_transfer_sink_turns_events_into_a_rising_byte_count() {
        // Before this the sink was never installed, so `Reporter::emit`
        // returned immediately and every byte counter the tray reads stayed at
        // zero for the whole of the largest transfers in the product.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let published = recording(&engine);

        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        // Byte-for-byte the sink `do_lfs` installs on `BasicTransfer`.
        let sink: lfs::basic::TransferSink = Box::new(move |event| event_tx.send(event).is_ok());
        let script = vec![
            lfs::basic::TransferEvent::Started {
                oid: "a".to_owned(),
                size: 1_000,
            },
            lfs::basic::TransferEvent::Started {
                oid: "b".to_owned(),
                size: 3_000,
            },
            lfs::basic::TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 400,
            },
            lfs::basic::TransferEvent::Progress {
                oid: "b".to_owned(),
                bytes_done: 1_500,
            },
            lfs::basic::TransferEvent::Progress {
                oid: "a".to_owned(),
                bytes_done: 900,
            },
            lfs::basic::TransferEvent::Completed {
                oid: "a".to_owned(),
            },
            // b dies part-way: its 1500 bytes really moved and its 3000 was
            // really queued, so neither figure may be rewritten.
            lfs::basic::TransferEvent::Failed {
                oid: "b".to_owned(),
                code: "network",
                error: "connection reset".to_owned(),
            },
        ];

        let mut tally = TransferTally::default();
        let transferring = async {
            for event in script {
                assert!(sink(event), "the receiver is alive for the whole run");
                // Let the publisher observe each event separately; a real
                // transfer arrives over milliseconds, not in one poll.
                tokio::task::yield_now().await;
            }
        };
        engine
            .publish_while(
                &p,
                SyncPhase::TransferringLfs,
                event_rx,
                |event, transfer_event| {
                    tally.fold(&transfer_event);
                    tally.apply(event);
                },
                transferring,
            )
            .await;

        let published = Engine::lock(&published).clone();
        assert!(
            !published.is_empty(),
            "the sink must have produced at least one progress event"
        );
        // The denominator GROWS: `download_all` announces objects as it starts
        // them, so a bar drawn from the first frame must survive the second one
        // widening it. What must never happen is either figure shrinking.
        let mut previous_done = 0;
        let mut previous_total = 0;
        for event in &published {
            assert_eq!(event.phase, SyncPhase::TransferringLfs);
            let total = event
                .bytes_total
                .expect("a started object gives the bar a denominator");
            assert!(
                total >= previous_total,
                "the denominator shrank: {previous_total} -> {total}"
            );
            assert!(
                event.bytes_done >= previous_done,
                "the byte count walked backwards: {previous_done} -> {}",
                event.bytes_done
            );
            assert!(
                event.bytes_done <= total,
                "{} of {total} is not a fraction",
                event.bytes_done
            );
            assert!(
                event.fraction().is_some(),
                "a known total must make the bar determinate"
            );
            previous_done = event.bytes_done;
            previous_total = total;
        }
        assert!(
            published.iter().any(|event| event.bytes_done > 0),
            "a run that moved bytes must publish at least one non-zero frame"
        );

        // 1000 for the object that completed, 1500 for the one that failed
        // part-way: consistent, and short of full exactly because it failed.
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.bytes_done, 2_500);
        assert_eq!(snapshot.bytes_total, Some(4_000));
        assert_eq!(snapshot.state, ProfileState::Syncing);
        assert_eq!(tally.bytes_done(), 2_500);
    }

    #[tokio::test]
    async fn a_dead_subscriber_never_stalls_a_transfer() {
        // The sink answers `false` once the receiver is gone, which detaches
        // the reporter for good. The transfer is journaled work and must run to
        // completion regardless of who is watching.
        let (event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
        let sink: lfs::basic::TransferSink = Box::new(move |event| event_tx.send(event).is_ok());
        drop(event_rx);
        assert!(!sink(lfs::basic::TransferEvent::Completed {
            oid: "a".to_owned()
        }));
    }

    #[tokio::test]
    async fn a_commit_publishes_a_file_count_the_bar_can_use() {
        // `fraction()` returned `None` for every phase before this, so the
        // commit and push legs rendered as an indeterminate spinner even though
        // the staged set is known exactly.
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("a.txt"), b"one").expect("write");
        std::fs::write(p.local_path.join("b.txt"), b"two").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        let published = recording(&engine);

        assert_eq!(commit_after_settling(&engine, &platform, &p), 2);

        let committing: Vec<SyncProgress> = Engine::lock(&published)
            .iter()
            .filter(|event| event.phase == SyncPhase::Committing)
            .cloned()
            .collect();
        assert_eq!(
            committing.len(),
            2,
            "one publish entering the commit and one on its completion"
        );
        assert_eq!(committing[0].files_total, Some(2));
        assert_eq!(committing[0].files_done, 0);
        assert_eq!(committing[0].fraction(), Some(0.0));
        assert_eq!(
            committing[0].current.as_deref(),
            Some("a.txt"),
            "the detail line names a repository-relative path, never an absolute one"
        );
        assert_eq!(committing[1].files_done, 2);
        assert_eq!(committing[1].fraction(), Some(1.0));

        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.files_total, Some(2));
        assert_eq!(snapshot.files_done, 2);
    }

    /// Advance a bare repository's `main` by one commit holding `content`.
    ///
    /// Written with gix plumbing rather than a second engine because the point
    /// is only that the remote moved: the local side is what is under test.
    fn advance_remote(remote_dir: &Path, file: &str, content: &str) {
        let remote = gix::open(remote_dir).expect("open bare remote");
        let tip = remote
            .find_reference("refs/heads/main")
            .expect("the pushed branch")
            .id()
            .detach();
        let blob = remote
            .write_blob(content.as_bytes())
            .expect("blob")
            .detach();
        let tree = gix::objs::Tree {
            entries: vec![gix::objs::tree::Entry {
                mode: gix::objs::tree::EntryKind::Blob.into(),
                filename: file.into(),
                oid: blob,
            }],
        };
        let tree = remote.write_object(&tree).expect("tree").detach();
        let mut buf = gix::date::parse::TimeBuf::default();
        let author = gix::actor::Signature {
            name: "Peer".into(),
            email: "peer@keeper.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        };
        let author = author.to_ref(&mut buf);
        remote
            .commit_as(author, author, "refs/heads/main", content, tree, vec![tip])
            .expect("advance the remote");
    }

    #[tokio::test]
    async fn a_sync_reports_the_conflict_copies_its_converge_made() {
        // AD-43 keeps both revisions, and the caller has to be able to say so:
        // a copy nobody is told about is a file the user will never find.
        let dir = tempfile::tempdir().expect("tempdir");
        let remote_dir = tempfile::tempdir().expect("tempdir");
        if gix::init_bare(remote_dir.path()).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.remote_url = remote_dir.path().to_string_lossy().into_owned();
        engine.upsert_profile(&p).expect("upsert");

        // A shared root, published to the remote so both sides have an ancestor
        // for `merge-base` to find.
        std::fs::write(p.local_path.join("notes.md"), b"root").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("publish the shared root to the local bare remote");

        // Both sides now edit the same path. This is the only shape that
        // produces a conflict copy rather than a clean merge.
        advance_remote(remote_dir.path(), "notes.md", "theirs");
        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("notes.md"), b"ours").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let outcome = engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("converge");
        assert_eq!(
            outcome.conflicts.len(),
            1,
            "the one contested path must be reported, got {:?}",
            outcome.conflicts
        );
        let copy = &outcome.conflicts[0];
        assert!(
            copy.starts_with("notes.sync-conflict-"),
            "the reported path must be the copy, not the original: {copy}"
        );
        let preserved = std::fs::read(p.local_path.join(copy)).expect("the copy is on disk");
        assert_eq!(
            preserved, b"ours",
            "the copy holds the local revision the merge gave away"
        );
        assert_eq!(
            std::fs::read(p.local_path.join("notes.md")).expect("canonical path"),
            b"theirs",
            "the remote keeps the canonical name (AD-43)"
        );
        // The same run proves the fetch counter is wired: `bytes` was declared
        // and never assigned, so every sync reported zero traffic no matter how
        // much it moved.
        assert!(
            outcome.bytes > 0,
            "a run that received a pack must report the bytes it moved, got {}",
            outcome.bytes
        );
    }
}

/// Where a git remote's forge API lives, when the URL says enough to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ForgeTarget {
    /// API root, e.g. `https://forgejo.example.com`.
    base: String,
    host: String,
    owner: String,
    repo: String,
}

/// Derive the forge API target from a git remote URL.
///
/// Handles the two shapes a Forgejo remote actually takes — `https://host/o/r`
/// and scp-style `git@host:o/r.git`, the latter of which is deliberately NOT a
/// valid URL and has to be split by hand. A `file://` or local-path remote has
/// no forge behind it and yields `None`, which the caller treats as "say the
/// branch is waiting" rather than as an error.
fn forge_api_target(remote_url: &str) -> Option<ForgeTarget> {
    let trimmed = remote_url.trim().trim_end_matches('/');
    let (scheme_host, path) = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
    {
        let secure = trimmed.starts_with("https://");
        let (host, path) = rest.split_once('/')?;
        // Strip any userinfo: a token embedded in the URL must never be
        // rebuilt into the API base and logged.
        let host = host.rsplit('@').next()?;
        (
            format!("{}://{host}", if secure { "https" } else { "http" }),
            path.to_owned(),
        )
    } else if let Some((user_host, path)) = trimmed.split_once(':') {
        // scp-style. Reject anything that looks like a scheme we did not
        // handle, and anything without a host.
        if user_host.contains('/') || path.starts_with('/') {
            return None;
        }
        let host = user_host.rsplit('@').next()?;
        if host.is_empty() {
            return None;
        }
        (format!("https://{host}"), path.to_owned())
    } else {
        return None;
    };

    let path = path.trim_end_matches(".git");
    let (owner, repo) = path.split_once('/')?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return None;
    }
    let host = scheme_host
        .split_once("://")
        .map(|(_, h)| h.to_owned())
        .unwrap_or_else(|| scheme_host.clone());
    Some(ForgeTarget {
        base: scheme_host,
        host,
        owner: owner.to_owned(),
        repo: repo.to_owned(),
    })
}
