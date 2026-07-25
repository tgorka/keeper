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
use keeper_sync::profile::{LfsMode, ProfileState, SyncDirection, SyncLane};
use keeper_sync::progress::{SyncPhase, SyncStatus};
use keeper_sync::provenance::SyncSource;
use keeper_sync::{SyncError, SyncProfile};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ipc::AppState;

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

fn parse_req(req: &SyncProfileReq) -> Result<SyncProfile, IpcError> {
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
    let profile = parse_req(&req)?;
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

/// Re-verify a profile's stored content against its recorded digests.
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
        }
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
        let parsed = parse_req(&req()).expect("valid");
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
            let err = parse_req(&r).expect_err("must reject");
            assert_eq!(err.code, IpcErrorCode::Internal);
            assert!(err.message.contains(value), "message names the bad value");
        }
    }

    #[test]
    fn an_invalid_profile_is_refused_before_it_reaches_the_engine() {
        let mut r = req();
        r.local_path = "relative/path".into();
        assert!(parse_req(&r).is_err());
    }

    #[test]
    fn a_worktree_lane_must_be_push_only_here_too() {
        let mut r = req();
        r.lane = "worktree".into();
        assert!(
            parse_req(&r).is_err(),
            "a bidirectional lane would leak the airlock"
        );
        r.direction = "pushOnly".into();
        assert!(parse_req(&r).is_ok());
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
