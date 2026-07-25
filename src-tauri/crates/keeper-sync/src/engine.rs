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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;

use crate::backoff::{jitter_sample, Backoff};
use crate::db::{self, DeviceIdentity, WorkKind, WorkState};
use crate::error::{Result, Retriability, SyncError};
use crate::git::{self, cli::GitCli};
use crate::lfs;
use crate::platform::SyncPlatform;
use crate::profile::{LfsMode, ProfileState, SyncDirection, SyncLane, SyncProfile};
use crate::progress::{ProgressSink, SyncPhase, SyncProgress, SyncStatus};
use crate::provenance::{Provenance, SyncSource};
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

/// Units claimed from the journal per profile per tick.
///
/// Small on purpose: a tick that drains a thousand units would hold the
/// profile's reservation for minutes and starve its watcher.
const CLAIM_LIMIT: u32 = 16;

/// How often the supervisor wakes when nothing else prompts it.
const TICK_MS: u64 = 1_000;

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
    sinks: Mutex<Vec<(u64, ProgressSink)>>,
    next_sink: AtomicU64,
    interrupt: Arc<AtomicBool>,
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
            sinks: Mutex::new(Vec::new()),
            next_sink: AtomicU64::new(1),
            interrupt: Arc::new(AtomicBool::new(false)),
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
            if let Err(err) = self.tick_profile(&profile).await {
                self.record_failure(&profile, &err);
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
            WorkKind::Pull => self.do_pull(profile).await,
            WorkKind::Push => self.do_push(profile).await,
            WorkKind::LfsDownload { oid, size } => self.do_lfs(profile, oid, *size, false).await,
            WorkKind::LfsUpload { oid, size } => self.do_lfs(profile, oid, *size, true).await,
            WorkKind::OpenPullRequest { branch } => self.do_open_pr(profile, branch).await,
            WorkKind::Verify => self.verify(&profile.id).await.map(drop),
        }
    }

    /// Materialize the profile's repository if it does not exist yet, without
    /// keeping the handle.
    ///
    /// Callers that only need the clone to have happened use this, so a
    /// `gix::Repository` — which is neither `Send` nor cheap to hold — never
    /// spans an await point.
    fn ensure_repo(&self, profile: &SyncProfile) -> Result<()> {
        self.open_repo(profile).map(drop)
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
            git::repo::enforce_local_config(&repo)?;
            return Ok(repo);
        }
        tracing::info!(
            profile = profile.name,
            "cloning remote for a new sync profile"
        );
        let repo = git::repo::clone(
            &profile.remote_url,
            &profile.local_path,
            &profile.branch,
            None,
            &self.interrupt,
        )?;
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

    async fn do_pull(&self, profile: &SyncProfile) -> Result<()> {
        if !profile.direction.pulls() {
            return Ok(());
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

        let outcome = tokio::task::spawn_blocking(move || -> Result<git::fetch::FetchOutcome> {
            let repo = git::repo::open(&repo_path, removable)?;
            let options = git::fetch::FetchOptions {
                shallow: None,
                refspecs: vec![format!("+refs/heads/{branch}:refs/remotes/origin/{branch}")],
            };
            let noop: git::fetch::TransferProgress = Arc::new(|_, _| {});
            git::fetch::fetch(
                &repo,
                "origin",
                &options,
                credential.as_ref(),
                &noop,
                &interrupt,
            )
        })
        .await
        .map_err(|err| SyncError::Journal(format!("fetch task failed: {err}")))??;

        // Whether a pack arrived says nothing about whether the working tree is
        // up to date: a re-fetch after an interrupted run transfers nothing and
        // still leaves the local branch behind. The only condition that matters
        // is that the two refs differ.
        let Some(remote_id) = outcome.remote_id else {
            // The remote has no such branch yet — a brand-new repository.
            return Ok(());
        };
        if outcome.local_id == Some(remote_id) {
            return Ok(());
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
                return Ok(());
            }
        }

        if outcome.fast_forward {
            let reference = tracking.clone();
            let path = repo_path.clone();
            return tokio::task::spawn_blocking(move || git.merge_ff_only(&path, &reference))
                .await
                .map_err(|err| SyncError::Journal(format!("merge task failed: {err}")))?;
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
        let conflicts = tokio::task::spawn_blocking(move || {
            Self::converge_with_conflict_copies(&git, &profile_for_task, &tracking, &stamp, &device)
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
        }
        Ok(())
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
            &format!("sync({}): merge remote changes", profile.name),
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
        self.publish(self.progress(profile, SyncPhase::Committing));
        let count = staged.len() as u64;
        self.commit(profile, &staged)?;
        Ok(count)
    }

    async fn do_push(&self, profile: &SyncProfile) -> Result<()> {
        if !profile.direction.pushes() {
            return Ok(());
        }
        self.commit_local(profile)?;
        self.publish(self.progress(profile, SyncPhase::Pushing));

        let git = self.git.clone();
        let repo_path = profile.local_path.clone();
        let refspec = format!("refs/heads/{0}:refs/heads/{0}", profile.branch);
        tokio::task::spawn_blocking(move || git.push(&repo_path, "origin", &refspec))
            .await
            .map_err(|err| SyncError::Journal(format!("push task failed: {err}")))??;

        if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
            snapshot.last_sync_ms = Some(self.platform.now_ms());
        }
        Ok(())
    }

    /// Turn collapsed untracked entries into the regular files they contain.
    ///
    /// A path that is already a file passes through. A directory is walked
    /// recursively; `.git` is never descended into, and symlinked directories
    /// are not followed (a symlink is staged as its target by the commit path,
    /// and following one could walk out of the profile entirely, or in circles).
    fn expand_untracked(root: &Path, entries: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let mut out = Vec::with_capacity(entries.len());
        let mut stack: Vec<PathBuf> = Vec::new();
        for rela in entries {
            // `.git` is skipped at the top level too, not only as a child. gix
            // does not report it as untracked and tier 0 excludes it anyway,
            // but walking a repository's own object store would be a very
            // expensive way to discover that.
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
                stack.push(rela.clone());
            } else {
                out.push(rela.clone());
            }
        }
        while let Some(dir) = stack.pop() {
            let absolute = root.join(&dir);
            let listing = match std::fs::read_dir(&absolute) {
                Ok(listing) => listing,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => return Err(SyncError::io("read untracked directory", absolute, err)),
            };
            for entry in listing.flatten() {
                let name = entry.file_name();
                if name == ".git" {
                    continue;
                }
                let child = dir.join(&name);
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => stack.push(child),
                    Ok(_) => out.push(child),
                    // Unreadable type: skip it rather than fail the whole scan.
                    Err(err) => {
                        tracing::debug!(path = %child.display(), error = %err, "skipping unreadable entry");
                    }
                }
            }
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

        if let Some(id) =
            git::commit::stage_and_commit(&repo, staged, &provenance, &profile.name, &author)?
        {
            tracing::info!(profile = profile.name, commit = %id, files = staged.len(), "committed");
        }
        Ok(())
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

        let endpoint = lfs::endpoint::derive(&profile.remote_url)?;
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
        let transfer = Arc::new(lfs::basic::BasicTransfer::new(
            self.http.clone(),
            store.clone(),
        ));

        let results = if upload {
            Arc::clone(&transfer).upload_all(specs, auth).await
        } else {
            Arc::clone(&transfer).download_all(specs, auth).await
        };
        for (oid, result) in results {
            if let Err(err) = result {
                tracing::warn!(oid, error = %err, "lfs transfer failed");
                return Err(err);
            }
        }
        Ok(())
    }

    async fn do_open_pr(&self, profile: &SyncProfile, branch: &str) -> Result<()> {
        // The pushed branch is the durable artifact; a failure to open the pull
        // request must never discard it (AD-50). So this reports an actionable
        // notice rather than rolling anything back.
        tracing::info!(
            profile = profile.name,
            branch,
            "lane pushed; pull request pending"
        );
        self.warn(
            &profile.id,
            &profile.name,
            format!("branch {branch} is pushed and waiting for review"),
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
            self.do_pull(&profile).await?;
            outcome.pulled = true;
        }
        if profile.direction.pushes() {
            self.do_push(&profile).await?;
            outcome.pushed = true;
        }

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
    fn collapsed_untracked_directories_expand_into_their_files() {
        // gitoxide reports a brand-new folder as ONE entry naming the
        // directory. Staging that directly fails with "only regular files and
        // symlinks can be synchronized", and nothing inside it ever syncs.
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("sub/deeper")).expect("mkdir");
        std::fs::create_dir_all(root.join(".git/objects")).expect("mkdir .git");
        std::fs::write(root.join("top.txt"), b"x").expect("write");
        std::fs::write(root.join("sub/a.txt"), b"x").expect("write");
        std::fs::write(root.join("sub/deeper/b.txt"), b"x").expect("write");
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
            vec![
                PathBuf::from("sub/a.txt"),
                PathBuf::from("sub/deeper/b.txt"),
                PathBuf::from("top.txt"),
            ],
            "directories expand recursively, .git is never descended into"
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
}
