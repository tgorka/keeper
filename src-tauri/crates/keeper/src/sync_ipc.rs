//! The folder-sync command surface (Story 29.1, AD-51).
//!
//! View models live here rather than in `keeper-core::vm` because sync is not
//! part of the Matrix hexagon and `keeper-core` must never learn about it
//! (AD-40) — the `lifecycle::LifecyclePhase` precedent for a shell-owned DTO.
//! They still follow every `vm.rs` convention: serde `camelCase`, `#[ts(export)]`
//! into `src/lib/ipc/gen/`, timestamps as `i64` ms.
//!
//! Every command is a thin projection over `keeper_sync::Engine`. Policy stays
//! in the engine; this layer only translates types and maps errors into the one
//! `IpcError` envelope the frontend already understands.

use std::sync::Arc;

use keeper_core::vm::{IpcError, IpcErrorCode};
use keeper_sync::engine::PendingReason;
use keeper_sync::profile::{LfsMode, ProfileState, SyncDirection, SyncLane};
use keeper_sync::progress::{SyncPhase, SyncStatus};
use keeper_sync::provenance::SyncSource;
use keeper_sync::{ActivityKind, SyncError, SyncPlatform, SyncProfile};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::AppState;

/// Rows returned when a caller does not say how many it wants.
const ACTIVITY_LIMIT_DEFAULT: u32 = 100;
/// The ceiling on one activity read. The engine trims each profile to
/// `db::ACTIVITY_CAP`, so asking for more can only return the same rows.
const ACTIVITY_LIMIT_MAX: u32 = 500;

/// The wire spelling of an activity kind.
///
/// Written out rather than derived from serde so the wire contract is visible
/// at the boundary and cannot change under a rename, matching `state_str` and
/// `phase_str` below.
fn activity_kind_str(kind: ActivityKind) -> &'static str {
    match kind {
        ActivityKind::Added => "added",
        ActivityKind::Modified => "modified",
        ActivityKind::Deleted => "deleted",
        ActivityKind::Conflict => "conflict",
    }
}

/// One configured folder↔repository binding, as the UI sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncProfileVm {
    pub id: String,
    pub name: String,
    pub local_path: String,
    pub remote_url: String,
    pub branch: String,
    pub direction: String,
    pub lane: String,
    pub subpaths: Vec<String>,
    pub excludes: Vec<String>,
    pub removable: bool,
    pub lfs_mode: String,
    #[ts(type = "number")]
    pub lfs_threshold_bytes: u64,
    #[ts(type = "number")]
    pub settle_ms: u64,
    pub tags: Vec<String>,
    /// Overrides the commit author, in any of `Name <email>`, a bare address,
    /// or a bare display name. `None` keeps the device identity and its
    /// non-routable `sync@<device-id>.keeper.invalid` address.
    pub author_override: Option<String>,
    pub enabled: bool,
}

impl From<&SyncProfile> for SyncProfileVm {
    fn from(p: &SyncProfile) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            local_path: p.local_path.to_string_lossy().into_owned(),
            remote_url: p.remote_url.clone(),
            branch: p.branch.clone(),
            direction: direction_str(p.direction).to_owned(),
            lane: lane_str(p.lane).to_owned(),
            subpaths: p.subpaths.clone(),
            excludes: p.excludes.clone(),
            removable: p.removable,
            lfs_mode: lfs_str(p.lfs_mode).to_owned(),
            lfs_threshold_bytes: p.lfs_threshold_bytes,
            settle_ms: p.settle_ms,
            tags: p.tags.clone(),
            author_override: p.author_override.clone(),
            enabled: p.enabled,
        }
    }
}

/// What a profile is doing right now — the polled snapshot the tray reads.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusVm {
    pub profile_id: String,
    pub profile_name: String,
    /// `idle` | `watching` | `syncing` | `offline` | `mediaAbsent` | `paused` |
    /// `needsAttention`.
    pub state: String,
    /// `scanning` | `fetching` | … | `idle`.
    pub phase: String,
    /// The single line the tray shows, composed in Rust so the tray and the
    /// window can never word it differently.
    pub line: String,
    #[ts(type = "number")]
    pub files_done: u64,
    #[ts(type = "number | null")]
    pub files_total: Option<u64>,
    #[ts(type = "number")]
    pub bytes_done: u64,
    #[ts(type = "number | null")]
    pub bytes_total: Option<u64>,
    #[ts(type = "number")]
    pub pending: u32,
    /// Sticky, last-write-wins, cleared only by a clean run — the same shape as
    /// `RecordingStatusVm::warning` so the banner behaves identically.
    pub warning: Option<String>,
    pub error: Option<String>,
    #[ts(type = "number | null")]
    pub last_sync_ms: Option<i64>,
    /// Whether this condition needs a human before the profile can progress.
    /// Drives the split between a passive amber line and an actionable notice.
    pub needs_attention: bool,
}

impl From<&SyncStatus> for SyncStatusVm {
    fn from(s: &SyncStatus) -> Self {
        Self {
            profile_id: s.profile_id.clone(),
            profile_name: s.profile_name.clone(),
            state: state_str(s.state).to_owned(),
            phase: phase_str(s.phase).to_owned(),
            line: keeper_sync::progress::status_line(s),
            files_done: s.files_done,
            files_total: s.files_total,
            bytes_done: s.bytes_done,
            bytes_total: s.bytes_total,
            pending: s.pending,
            warning: s.warning.clone(),
            error: s.error.clone(),
            last_sync_ms: s.last_sync_ms,
            needs_attention: s.state.is_warning() || s.error.is_some(),
        }
    }
}

/// What one manual sync did.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncOutcomeVm {
    pub committed: bool,
    pub pushed: bool,
    pub pulled: bool,
    #[ts(type = "number")]
    pub files_changed: u64,
    pub conflicts: Vec<String>,
}

/// One recorded thing sync did to one file.
///
/// Paths are repo-relative and the row carries nothing else — never contents,
/// never a second copy of anything. The engine trims each profile to its newest
/// `ACTIVITY_CAP` rows, so this list is recent history, not an audit log.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncActivityVm {
    #[ts(type = "number")]
    pub ts_ms: i64,
    /// `added` | `modified` | `deleted` | `conflict`.
    pub kind: String,
    pub path: String,
}

/// One file sync has seen but not yet carried.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncPendingVm {
    pub path: String,
    /// `settling` | `untracked` | `modified` | `added` | `deleted`.
    pub reason: String,
    /// When the quiescence episode began, for `settling` only. The UI renders
    /// it as "waiting for writes to stop", not as a countdown: the window
    /// restarts on every write, so a promised finish time would be a guess.
    #[ts(type = "number | null")]
    pub since_ms: Option<i64>,
}

/// A unit of work that failed permanently and stopped being retried.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncParkedVm {
    #[ts(type = "number")]
    pub id: i64,
    pub kind: String,
    #[ts(type = "number")]
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Everything currently wrong with one profile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncProblemsVm {
    pub warning: Option<String>,
    pub error: Option<String>,
    pub parked: Vec<SyncParkedVm>,
    /// Conflict copies still on disk. A copy the user has already dealt with
    /// and deleted is resolved, so it leaves this list on its own.
    pub conflicts: Vec<String>,
}

/// Fields a caller may set when creating or updating a profile.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncProfileReq {
    /// Absent creates a profile; present updates that one.
    pub id: Option<String>,
    pub name: String,
    pub local_path: String,
    pub remote_url: String,
    pub branch: String,
    pub direction: String,
    pub lane: String,
    #[serde(default)]
    pub subpaths: Vec<String>,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub removable: bool,
    pub lfs_mode: String,
    #[ts(type = "number | null")]
    pub lfs_threshold_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub settle_ms: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Absent leaves whatever the stored profile already has, so a caller that
    /// does not know about this field cannot erase it. An explicit empty string
    /// clears the override back to the device identity.
    #[serde(default)]
    pub author_override: Option<String>,
}

/// Mint an opaque, sortable, collision-free profile id.
///
/// ULID-shaped — a 48-bit millisecond timestamp in Crockford base32 followed by
/// randomness — so ids sort by creation and read like the engine's own, without
/// pulling a second copy of the `ulid` crate into the shell.
///
/// Randomness comes from `RandomState`, which the standard library seeds once
/// per process from the OS. This only has to avoid collision between profiles a
/// human creates; it is not a security boundary, so a CSPRNG would be theatre.
fn new_profile_id() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();

    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(millis);
    let entropy = hasher.finish();

    let mut out = String::with_capacity(26);
    // 10 characters = 50 bits, covering the 48-bit timestamp.
    for i in (0..10).rev() {
        out.push(ALPHABET[((millis >> (i * 5)) & 0x1f) as usize] as char);
    }
    // 12 characters = 60 bits of the 64-bit hash.
    for i in (0..12).rev() {
        out.push(ALPHABET[((entropy >> (i * 5)) & 0x1f) as usize] as char);
    }
    out
}

fn direction_str(d: SyncDirection) -> &'static str {
    match d {
        SyncDirection::Bidirectional => "bidirectional",
        SyncDirection::PushOnly => "pushOnly",
        SyncDirection::PullOnly => "pullOnly",
    }
}

fn lane_str(l: SyncLane) -> &'static str {
    match l {
        SyncLane::Main => "main",
        SyncLane::Worktree => "worktree",
    }
}

fn lfs_str(m: LfsMode) -> &'static str {
    match m {
        LfsMode::Materialize => "materialize",
        LfsMode::PointerOnly => "pointerOnly",
        LfsMode::Disabled => "disabled",
    }
}

fn state_str(s: ProfileState) -> &'static str {
    match s {
        ProfileState::Idle => "idle",
        ProfileState::Watching => "watching",
        ProfileState::Syncing => "syncing",
        ProfileState::Offline => "offline",
        ProfileState::MediaAbsent => "mediaAbsent",
        ProfileState::Paused => "paused",
        ProfileState::NeedsAttention => "needsAttention",
    }
}

fn phase_str(p: SyncPhase) -> &'static str {
    match p {
        SyncPhase::Scanning => "scanning",
        SyncPhase::Fetching => "fetching",
        SyncPhase::Applying => "applying",
        SyncPhase::Staging => "staging",
        SyncPhase::Committing => "committing",
        SyncPhase::Pushing => "pushing",
        SyncPhase::TransferringLfs => "transferringLfs",
        SyncPhase::Verifying => "verifying",
        SyncPhase::Idle => "idle",
    }
}

/// Map a `SyncError` into the one envelope the frontend understands.
///
/// Kept beside the sync surface rather than folded into `to_ipc_error`, because
/// that funnel is exhaustive over `CoreError` and sync is deliberately not part
/// of it (AD-40). The classification itself is not re-derived here: the error
/// already knows whether it is retriable and whether it needs a human.
pub fn sync_ipc_error(err: &SyncError) -> IpcError {
    let code = match err {
        SyncError::GitMissing { .. } => IpcErrorCode::Unsupported,
        SyncError::Auth { .. } => IpcErrorCode::InvalidCredentials,
        SyncError::Network { .. } => IpcErrorCode::ServerUnreachable,
        _ => IpcErrorCode::Internal,
    };
    IpcError {
        code,
        message: err.to_string(),
        account_id: None,
        retriable: matches!(
            err.retriability(),
            keeper_sync::error::Retriability::Transient
        ),
    }
}

/// Build the profile to store from a request, carrying forward everything the
/// request cannot express.
///
/// `prior` is the stored profile when this is an update. It matters because
/// `db::upsert_profile` replaces the whole JSON row, so any field this function
/// leaves at its constructor default is silently ERASED on save. That is not
/// hypothetical: before `prior` existed, saving an edit to a paused profile
/// reset `enabled` to `true` and quietly resumed syncing a folder the user had
/// deliberately stopped, and wiped any `author_override` set through the daemon.
fn parse_req(req: &SyncProfileReq, prior: Option<&SyncProfile>) -> Result<SyncProfile, IpcError> {
    let direction = match req.direction.as_str() {
        "bidirectional" => SyncDirection::Bidirectional,
        "pushOnly" => SyncDirection::PushOnly,
        "pullOnly" => SyncDirection::PullOnly,
        other => {
            return Err(sync_ipc_error(&SyncError::Config(format!(
                "unknown sync direction: {other}"
            ))));
        }
    };
    let lane = match req.lane.as_str() {
        "main" => SyncLane::Main,
        "worktree" => SyncLane::Worktree,
        other => {
            return Err(sync_ipc_error(&SyncError::Config(format!(
                "unknown sync lane: {other}"
            ))));
        }
    };
    let lfs_mode = match req.lfs_mode.as_str() {
        "materialize" => LfsMode::Materialize,
        "pointerOnly" => LfsMode::PointerOnly,
        "disabled" => LfsMode::Disabled,
        other => {
            return Err(sync_ipc_error(&SyncError::Config(format!(
                "unknown LFS mode: {other}"
            ))));
        }
    };

    // A new profile needs an opaque stable id. The engine's ULID crate is not
    // a dependency of the shell and does not need to be: any collision-free
    // opaque string satisfies the contract, and the engine treats it as
    // entirely opaque.
    let id = req.id.clone().unwrap_or_else(new_profile_id);
    let mut profile = SyncProfile::new(id, &req.name, &req.local_path, &req.remote_url);
    profile.branch = req.branch.clone();
    profile.direction = direction;
    profile.lane = lane;
    profile.subpaths = req.subpaths.clone();
    profile.excludes = req.excludes.clone();
    profile.removable = req.removable;
    profile.lfs_mode = lfs_mode;
    if let Some(bytes) = req.lfs_threshold_bytes {
        profile.lfs_threshold_bytes = bytes;
    }
    if let Some(ms) = req.settle_ms {
        profile.settle_ms = ms;
    }
    profile.tags = req.tags.clone();
    // Fields the request cannot carry survive from the stored profile, because
    // the upsert replaces the whole row. `enabled` is only ever moved by the
    // explicit pause/resume command; an edit must not resume a paused folder.
    if let Some(prior) = prior {
        profile.enabled = prior.enabled;
        profile.author_override = prior.author_override.clone();
    }
    // An explicit value overrides; an explicit empty string clears back to the
    // device identity, which is how the form offers "use the default".
    if let Some(author) = req.author_override.as_ref() {
        let trimmed = author.trim();
        profile.author_override = (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    // Validate here so a bad profile is rejected at the edge with an actionable
    // message rather than deep inside the engine.
    profile.validate().map_err(|err| sync_ipc_error(&err))?;
    Ok(profile)
}

fn engine_of(state: &AppState) -> Result<Arc<keeper_sync::engine::Engine>, IpcError> {
    crate::sync::engine(Arc::clone(&state.platform)).map_err(|err| sync_ipc_error(&err))
}

/// Every configured profile.
#[tauri::command]
pub async fn sync_profiles(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SyncProfileVm>, IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    Ok(profiles.iter().map(SyncProfileVm::from).collect())
}

/// A status snapshot for every profile, newest state first read.
#[tauri::command]
pub async fn sync_statuses(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SyncStatusVm>, IpcError> {
    let engine = engine_of(&state)?;
    let statuses = engine.statuses().map_err(|e| sync_ipc_error(&e))?;
    Ok(statuses.iter().map(SyncStatusVm::from).collect())
}

/// Create or update a profile, returning the stored result.
#[tauri::command]
pub async fn sync_profile_save(
    state: tauri::State<'_, AppState>,
    req: SyncProfileReq,
) -> Result<SyncProfileVm, IpcError> {
    let engine = engine_of(&state)?;
    // Read the stored profile first so the edit merges onto it rather than
    // replacing it: the upsert writes the whole row (see `parse_req`).
    let prior = match req.id.as_deref() {
        Some(id) => engine
            .list_profiles()
            .map_err(|e| sync_ipc_error(&e))?
            .into_iter()
            .find(|p| p.id == id),
        None => None,
    };
    let profile = parse_req(&req, prior.as_ref())?;
    engine
        .upsert_profile(&profile)
        .map_err(|e| sync_ipc_error(&e))?;
    Ok(SyncProfileVm::from(&profile))
}

/// Forget a profile. The folder and its repository are left on disk untouched —
/// removing a profile is a configuration change, never a deletion of content.
#[tauri::command]
pub async fn sync_profile_remove(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    engine.remove_profile(&id).map_err(|e| sync_ipc_error(&e))
}

/// Pause or resume a profile.
#[tauri::command]
pub async fn sync_profile_set_enabled(
    state: tauri::State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<SyncStatusVm, IpcError> {
    let engine = engine_of(&state)?;
    engine
        .set_enabled(&id, enabled)
        .map_err(|e| sync_ipc_error(&e))?;
    let status = engine.status(&id).map_err(|e| sync_ipc_error(&e))?;
    Ok(SyncStatusVm::from(&status))
}

/// Sync one profile now, ignoring the schedule.
///
/// Named `sync_folder_now` rather than `sync_now`: the latter is already the
/// Matrix sync kick, and two unrelated operations sharing a command name is
/// how a caller ends up invoking the wrong one.
#[tauri::command]
pub async fn sync_folder_now(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<SyncOutcomeVm, IpcError> {
    let engine = engine_of(&state)?;
    let outcome = engine
        .sync_once(&id, SyncSource::Manual)
        .await
        .map_err(|e| sync_ipc_error(&e))?;
    Ok(SyncOutcomeVm {
        committed: outcome.committed.is_some(),
        pushed: outcome.pushed,
        pulled: outcome.pulled,
        files_changed: outcome.files_changed,
        conflicts: outcome.conflicts,
    })
}

/// What sync has done to this folder's files lately, newest first.
#[tauri::command]
pub async fn sync_activity(
    state: tauri::State<'_, AppState>,
    id: String,
    limit: Option<u32>,
) -> Result<Vec<SyncActivityVm>, IpcError> {
    let engine = engine_of(&state)?;
    // A caller asking for everything gets the engine's own cap rather than an
    // unbounded read: the table is already trimmed to it, so a larger number
    // could only ever return the same rows.
    let limit = limit
        .unwrap_or(ACTIVITY_LIMIT_DEFAULT)
        .min(ACTIVITY_LIMIT_MAX) as usize;
    let rows = engine
        .activity(&id, limit)
        .await
        .map_err(|e| sync_ipc_error(&e))?;
    Ok(rows
        .into_iter()
        .map(|row| SyncActivityVm {
            ts_ms: row.ts_ms,
            kind: activity_kind_str(row.kind).to_owned(),
            path: row.path,
        })
        .collect())
}

/// What this folder is waiting to carry, and why.
#[tauri::command]
pub async fn sync_pending(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<SyncPendingVm>, IpcError> {
    let engine = engine_of(&state)?;
    let files = engine.pending(&id).await.map_err(|e| sync_ipc_error(&e))?;
    Ok(files
        .into_iter()
        .map(|file| {
            let (reason, since_ms) = match file.reason {
                PendingReason::Settling { since_ms } => ("settling", Some(since_ms)),
                PendingReason::Untracked => ("untracked", None),
                PendingReason::Modified => ("modified", None),
                PendingReason::Added => ("added", None),
                PendingReason::Deleted => ("deleted", None),
            };
            SyncPendingVm {
                path: file.path,
                reason: reason.to_owned(),
                since_ms,
            }
        })
        .collect())
}

/// Everything currently wrong with this folder.
#[tauri::command]
pub async fn sync_problems(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<SyncProblemsVm, IpcError> {
    let engine = engine_of(&state)?;
    let report = engine.problems(&id).await.map_err(|e| sync_ipc_error(&e))?;
    Ok(SyncProblemsVm {
        warning: report.warning,
        error: report.error,
        parked: report
            .parked
            .into_iter()
            .map(|unit| SyncParkedVm {
                id: unit.id,
                kind: unit.kind,
                attempts: unit.attempts,
                last_error: unit.last_error,
            })
            .collect(),
        conflicts: report.conflicts,
    })
}

/// Put a parked unit back in the queue.
///
/// Parked means the engine gave up: a permanent error, or a payload it could
/// not read. Nothing retries it on its own, which is exactly why it needs a
/// button — before this, such a unit sat in the journal invisible and inert.
#[tauri::command]
pub async fn sync_retry_parked(
    state: tauri::State<'_, AppState>,
    id: String,
    unit_id: i64,
) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    engine
        .retry_parked(&id, unit_id)
        .await
        .map_err(|e| sync_ipc_error(&e))
}

/// Store the access token a private remote needs, in the OS keychain.
///
/// This closes a real hole rather than adding a convenience: `SyncPlatform`
/// has had `secret_set`/`secret_delete` since Story 24.2 and the engine calls
/// NEITHER, so until now a profile pointing at a private remote could not be
/// authenticated from the app at all — the only ways in were a daemon env var
/// or a 0600 file placed by hand. The secret goes to the keychain under the
/// profile's own `sync/<id>/credential` key and is never written where the
/// engine's own persistence can see it (never `sync.db`, never a config file),
/// which is the port's stated contract.
///
/// Write-only by design: there is no command that reads a token back out. The
/// form can replace it or clear it, and that is the whole surface.
#[tauri::command]
pub async fn sync_set_credential(
    state: tauri::State<'_, AppState>,
    id: String,
    token: String,
) -> Result<(), IpcError> {
    let profile = profile_by_id(&state, &id)?;
    let platform = crate::sync::sync_platform(Arc::clone(&state.platform));
    platform
        .secret_set(&profile.secret_key(), &token)
        .map_err(|err| sync_ipc_error(&err))
}

/// Forget a profile's stored token. Idempotent — clearing an absent secret is
/// not an error, because the user's intent ("there should be no token here") is
/// already satisfied.
#[tauri::command]
pub async fn sync_clear_credential(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), IpcError> {
    let profile = profile_by_id(&state, &id)?;
    let platform = crate::sync::sync_platform(Arc::clone(&state.platform));
    platform
        .secret_delete(&profile.secret_key())
        .map_err(|err| sync_ipc_error(&err))
}

/// Resolve a profile by id, or report it as a config error naming the id.
fn profile_by_id(state: &AppState, id: &str) -> Result<keeper_sync::SyncProfile, IpcError> {
    let engine =
        crate::sync::engine(Arc::clone(&state.platform)).map_err(|e| sync_ipc_error(&e))?;
    engine
        .list_profiles()
        .map_err(|e| sync_ipc_error(&e))?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| sync_ipc_error(&SyncError::Config(format!("no such sync profile: {id}"))))
}

/// Re-read a profile's tracked files and report the ones that failed.
///
/// NOT a digest comparison — keeper records no per-file hash (`file_state` has
/// no digest column). Each file is read and fails only if it changed under the
/// read (a torn read), and each LFS pointer's object must be present at its
/// recorded size. Worth having, but the earlier "against its recorded digests"
/// wording described a check that does not exist.
#[tauri::command]
pub async fn sync_verify(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Vec<String>, IpcError> {
    let engine = engine_of(&state)?;
    let report = engine.verify(&id).await.map_err(|e| sync_ipc_error(&e))?;
    Ok(report
        .bad
        .into_iter()
        .map(|(path, reason)| format!("{path}: {reason}"))
        .collect())
}

/// One streamed progress update (Story 29.1, AD-51).
///
/// Streaming exists alongside the polled [`SyncStatusVm`] rather than replacing
/// it: a subscribed window gets sub-second detail, while the tray keeps
/// rendering correctly with no webview subscribed at all.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgressVm {
    pub profile_id: String,
    pub profile_name: String,
    pub phase: String,
    #[ts(type = "number")]
    pub files_done: u64,
    #[ts(type = "number | null")]
    pub files_total: Option<u64>,
    #[ts(type = "number")]
    pub bytes_done: u64,
    #[ts(type = "number | null")]
    pub bytes_total: Option<u64>,
    /// Repository-relative path of the item in flight, never an absolute one:
    /// absolute paths leak home-directory names into logs and screenshots.
    pub current: Option<String>,
    /// Completion in [0,1], or `null` when the total is not yet known and the
    /// UI must render an indeterminate meter rather than invent a denominator.
    #[ts(type = "number | null")]
    pub fraction: Option<f64>,
}

/// Stream sync progress until the channel closes.
///
/// Returns a subscription id. The engine drops a sink as soon as it returns
/// `false`, which `Channel::send` does once the webview is gone — so a closed
/// window unsubscribes itself and a reload cannot accumulate dead sinks.
#[tauri::command]
pub async fn sync_subscribe_progress(
    state: tauri::State<'_, AppState>,
    channel: tauri::ipc::Channel<SyncProgressVm>,
) -> Result<u64, IpcError> {
    let engine = engine_of(&state)?;
    let id = engine.subscribe(Box::new(move |event| {
        let vm = SyncProgressVm {
            profile_id: event.profile_id.clone(),
            profile_name: event.profile_name.clone(),
            phase: phase_str(event.phase).to_owned(),
            files_done: event.files_done,
            files_total: event.files_total,
            bytes_done: event.bytes_done,
            bytes_total: event.bytes_total,
            current: event.current.clone(),
            fraction: event.fraction(),
        };
        channel.send(vm).is_ok()
    }));
    Ok(id)
}

/// Stop a progress subscription. Unsubscribing an unknown id is a no-op, so a
/// double-unsubscribe from a racing unmount is not an error.
#[tauri::command]
pub async fn sync_unsubscribe_progress(
    state: tauri::State<'_, AppState>,
    id: u64,
) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    engine.unsubscribe(id);
    Ok(())
}

/// The tray's view of folder sync: one composed state and one line.
///
/// Called by the ~1 Hz tray tick, so it is deliberately cheap and total:
/// the engine is only consulted if it already exists (building it opens a
/// database, which a UI tick must never do), and any failure degrades to
/// "nothing to show" rather than propagating. A tray that stops repainting
/// because sync had a bad second would be a worse bug than a missing glyph.
///
/// The line is composed in Rust by `keeper_sync::progress::status_line`, the
/// same function the window renders, so the two can never word a state
/// differently.
pub fn tray_snapshot(app: &tauri::AppHandle) -> (keeper_sync::progress::TraySyncState, String) {
    use keeper_sync::progress::{status_line, tray_state, TraySyncState};
    use tauri::Manager as _;

    let Some(engine) = crate::sync::engine_if_open() else {
        return (TraySyncState::Absent, String::new());
    };
    // Touching AppState only to keep the signature uniform with the recording
    // snapshot; the engine is process-wide and needs nothing from it.
    let _ = app.try_state::<AppState>();

    let Ok(statuses) = engine.statuses() else {
        return (TraySyncState::Absent, String::new());
    };
    let state = tray_state(&statuses);
    if state == TraySyncState::Absent {
        return (state, String::new());
    }

    // With several profiles the tray has room for one line, so it shows the
    // most urgent: the one whose state the composed glyph is actually
    // reporting. Ties break on name for stability — a line that reshuffles
    // every tick is unreadable.
    let mut ranked: Vec<&keeper_sync::progress::SyncStatus> = statuses.iter().collect();
    ranked.sort_by_key(|s| {
        let urgency = if s.error.is_some() || s.warning.is_some() || s.state.is_warning() {
            0
        } else if s.state.is_active() || s.phase.is_active() {
            1
        } else if matches!(s.state, keeper_sync::ProfileState::Paused) {
            2
        } else {
            3
        };
        (urgency, s.profile_name.clone())
    });
    let line = ranked.first().map(|s| status_line(s)).unwrap_or_default();
    (state, line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> SyncProfileReq {
        SyncProfileReq {
            id: None,
            name: "tgdrive".into(),
            local_path: "/home/u/tgdrive".into(),
            remote_url: "https://git.example/u/tgdrive.git".into(),
            branch: "main".into(),
            direction: "bidirectional".into(),
            lane: "main".into(),
            subpaths: vec![],
            excludes: vec![],
            removable: false,
            lfs_mode: "materialize".into(),
            lfs_threshold_bytes: None,
            settle_ms: None,
            tags: vec![],
            author_override: None,
        }
    }

    /// The bug this guards: `db::upsert_profile` replaces the whole JSON row,
    /// and `parse_req` rebuilds the profile from a request that carries neither
    /// `enabled` nor `author_override`. Editing a PAUSED folder therefore
    /// resumed it — keeper quietly restarting sync on a folder the user had
    /// deliberately stopped — and erased an author override set through the
    /// daemon. Nothing in the UI hinted at either.
    #[test]
    fn editing_a_profile_keeps_what_the_request_cannot_carry() {
        let mut prior = parse_req(&req(), None).expect("valid");
        prior.enabled = false;
        prior.author_override = Some("Ada <ada@example.org>".into());

        let mut edit = req();
        edit.id = Some(prior.id.clone());
        edit.name = "renamed".into();
        let merged = parse_req(&edit, Some(&prior)).expect("valid");

        assert_eq!(merged.name, "renamed", "the edit still applies");
        assert!(!merged.enabled, "an edit must not resume a paused folder");
        assert_eq!(
            merged.author_override.as_deref(),
            Some("Ada <ada@example.org>"),
            "an edit must not erase an override it cannot express"
        );
    }

    #[test]
    fn an_explicit_author_override_is_set_and_an_empty_one_clears_it() {
        let prior = {
            let mut p = parse_req(&req(), None).expect("valid");
            p.author_override = Some("Ada <ada@example.org>".into());
            p
        };

        let mut set = req();
        set.author_override = Some("  Grace <grace@example.org>  ".into());
        assert_eq!(
            parse_req(&set, Some(&prior))
                .expect("valid")
                .author_override
                .as_deref(),
            Some("Grace <grace@example.org>"),
            "an explicit value wins and is trimmed"
        );

        // "Use the device identity" is expressible: the form sends an empty
        // string, which is different from omitting the field entirely.
        let mut cleared = req();
        cleared.author_override = Some("   ".into());
        assert_eq!(
            parse_req(&cleared, Some(&prior))
                .expect("valid")
                .author_override,
            None,
            "an empty override falls back to the device identity"
        );
    }
    #[test]
    fn minted_ids_are_unique_sortable_and_shaped_like_a_ulid() {
        let ids: std::collections::BTreeSet<String> = (0..500).map(|_| new_profile_id()).collect();
        assert_eq!(ids.len(), 500, "ids must not collide");
        for id in &ids {
            assert_eq!(id.len(), 22);
            assert!(
                id.bytes()
                    .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b)),
                "unexpected character in {id}"
            );
        }
        // The timestamp prefix makes them sort by creation, which is what makes
        // a profile list stable without a separate ordering column.
        let first = new_profile_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(new_profile_id() > first);
    }

    #[test]
    fn a_request_round_trips_into_a_profile_and_back_into_a_view_model() {
        let parsed = parse_req(&req(), None).expect("valid");
        assert!(!parsed.id.is_empty(), "a new profile is given an id");
        let vm = SyncProfileVm::from(&parsed);
        assert_eq!(vm.direction, "bidirectional");
        assert_eq!(vm.lane, "main");
        assert_eq!(vm.lfs_mode, "materialize");
        assert!(vm.enabled);
    }

    #[test]
    fn an_unknown_enum_string_is_rejected_at_the_edge() {
        // Better a precise error here than a silent fallback that syncs the
        // wrong way round.
        for (field, value) in [
            ("direction", "sideways"),
            ("lane", "branch"),
            ("lfsMode", "on"),
        ] {
            let mut r = req();
            match field {
                "direction" => r.direction = value.into(),
                "lane" => r.lane = value.into(),
                _ => r.lfs_mode = value.into(),
            }
            let err = parse_req(&r, None).expect_err("must reject");
            assert_eq!(err.code, IpcErrorCode::Internal);
            assert!(err.message.contains(value), "message names the bad value");
        }
    }

    #[test]
    fn an_invalid_profile_is_refused_before_it_reaches_the_engine() {
        let mut r = req();
        r.local_path = "relative/path".into();
        assert!(parse_req(&r, None).is_err());
    }

    #[test]
    fn a_worktree_lane_must_be_push_only_here_too() {
        let mut r = req();
        r.lane = "worktree".into();
        assert!(
            parse_req(&r, None).is_err(),
            "a bidirectional lane would leak the airlock"
        );
        r.direction = "pushOnly".into();
        assert!(parse_req(&r, None).is_ok());
    }

    #[test]
    fn errors_keep_the_classification_the_engine_already_made() {
        let missing = sync_ipc_error(&SyncError::GitMissing { reason: "x".into() });
        assert_eq!(missing.code, IpcErrorCode::Unsupported);
        assert!(!missing.retriable);

        let net = sync_ipc_error(&SyncError::Network {
            host: "h".into(),
            reason: "reset".into(),
        });
        assert_eq!(net.code, IpcErrorCode::ServerUnreachable);
        assert!(net.retriable, "a network blip must be retriable");

        let auth = sync_ipc_error(&SyncError::Auth { host: "h".into() });
        assert_eq!(auth.code, IpcErrorCode::InvalidCredentials);
        assert!(
            !auth.retriable,
            "retrying rejected credentials gets an account locked"
        );
    }
}
