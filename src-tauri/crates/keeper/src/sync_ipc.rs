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
use keeper_sync::engine::{PendingReason, SyncOutcome};
use keeper_sync::profile::{
    LfsMode, ProfileState, SyncDirection, SyncLane, DEFAULT_POLL_INTERVAL_MS, DEFAULT_SETTLE_MS,
};
use keeper_sync::progress::{format_bytes, SyncPhase, SyncStatus};
use keeper_sync::provenance::SyncSource;
use keeper_sync::{ActivityKind, DeliveryState, SyncError, SyncPlatform, SyncProfile};
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

/// The wire spelling of a delivery state, written out for the same reason
/// [`activity_kind_str`] is.
fn delivery_str(delivery: DeliveryState) -> &'static str {
    match delivery {
        DeliveryState::Success => "success",
        DeliveryState::InProgress => "inProgress",
        DeliveryState::Failed => "failed",
        DeliveryState::Abandoned => "abandoned",
        DeliveryState::Unknown => "unknown",
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
    /// The quiescence window this profile PINS, or `None` when it pins none and
    /// keeper picks (Story 34.5, AD-34-8). Not the same as the window in force:
    /// see `effective_settle_ms`. The distinction is load-bearing — a form that
    /// showed a substituted number as a pinned one would turn "let keeper
    /// choose" into a hard-coded user value on the next save.
    #[ts(type = "number | null")]
    pub settle_ms: Option<u64>,
    /// The quiescence window actually in force, substitutions included: 10 s on
    /// removable media that pins nothing (`REMOVABLE_SETTLE_MS`), otherwise the
    /// pinned value clamped to the ceiling. Every numeric knob has to be able to
    /// show the number in force (AD-34-8), and this is that number.
    #[ts(type = "number")]
    pub effective_settle_ms: u64,
    /// The scan cadence this profile pins, or `None` when it pins none.
    ///
    /// The knob that actually governs sync latency: the engine paces its tree
    /// walk by it (`scan_is_due`, DW-116). Until Story 34.5 it was reachable
    /// only from `keeper-syncd`'s CLI.
    #[ts(type = "number | null")]
    pub poll_interval_ms: Option<u64>,
    /// The scan cadence actually in force — the pinned value floored at
    /// `MIN_POLL_INTERVAL_MS`, because a zero would re-stat the tree every tick.
    #[ts(type = "number")]
    pub effective_poll_interval_ms: u64,
    pub tags: Vec<String>,
    /// Shapes the generated commit subject; empty means keeper's own
    /// `sync(<profile>): 3 added, 1 modified`. Placeholders are a closed set
    /// (`provenance::SUBJECT_PLACEHOLDERS`) and an unknown one is refused on
    /// save. The trailer block is not shapeable — provenance is not decoration.
    pub commit_subject_template: String,
    /// Overrides the commit author, in any of `Name <email>`, a bare address,
    /// or a bare display name. `None` keeps the device identity and its
    /// non-routable `sync@<device-id>.keeper.invalid` address.
    pub author_override: Option<String>,
    pub enabled: bool,
    /// Whether this folder contains a notes vault (FR-94, AD-54).
    ///
    /// A vault is not a configured object: it is this flag plus a subfolder, so
    /// the vault list IS a filter over the profile list and there is no second
    /// registry to keep consistent with the first.
    pub notes: bool,
    /// Where inside the folder the vault lives, when it is one. `None` when it is
    /// not — the form then shows the real default (`notes/`) rather than a blank
    /// box, so what it displays is the value that would actually be in force
    /// (AD-34-8).
    pub notes_subfolder: Option<String>,
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
            settle_ms: (p.settle_ms != DEFAULT_SETTLE_MS).then_some(p.settle_ms),
            effective_settle_ms: p.effective_settle_ms(),
            poll_interval_ms: (p.poll_interval_ms != DEFAULT_POLL_INTERVAL_MS)
                .then_some(p.poll_interval_ms),
            effective_poll_interval_ms: p.effective_poll_interval_ms(),
            tags: p.tags.clone(),
            commit_subject_template: p.commit_subject_template.clone(),
            author_override: p.author_override.clone(),
            enabled: p.enabled,
            notes: p.notes.is_some(),
            notes_subfolder: p.notes.as_ref().map(|notes| notes.subfolder.clone()),
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
    /// Files the completeness gate is holding inside their quiescence window.
    ///
    /// Distinct from `pending`, which counts journal rows: a folder where a
    /// thousand files are still being written has none of those, so `pending`
    /// alone reported it as up to date (AD-34-10). `line` already says this in
    /// words; the number is here so a surface can show it without parsing prose.
    #[ts(type = "number")]
    pub settling: u32,
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
            settling: s.settling,
            warning: s.warning.clone(),
            error: s.error.clone(),
            last_sync_ms: s.last_sync_ms,
            needs_attention: s.state.is_warning() || s.error.is_some(),
        }
    }
}

/// What one manual sync did.
///
/// Both the raw counts and the sentence composed from them, for the same
/// reason [`SyncStatusVm`] carries both: the Sync view and the Settings row
/// render the sentence verbatim, so the two surfaces cannot word one result
/// two different ways, and a caller that wants to branch on the numbers still
/// can.
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
    /// Bytes this run moved over the network — the received pack plus every
    /// LFS object transferred. Zero for a pass that found nothing to do, which
    /// is the common and correct answer, never an error.
    #[ts(type = "number")]
    pub bytes: u64,
    /// One sentence naming what happened, ready to render. See
    /// [`outcome_line`].
    pub line: String,
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
    /// How big the file was, or `null` when nobody measured it: a row recorded
    /// before sizes existed, or a deletion the repository no longer remembers
    /// the size of. The list renders nothing at all for `null` — never `0 B`,
    /// which would claim the file was empty.
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
    /// How far this file has got toward the remote:
    ///
    /// * `success` — the work unit that had to deliver it completed.
    /// * `inProgress` — a unit is queued, running, or waiting on a condition.
    /// * `failed` — a unit failed and keeper is still retrying it.
    /// * `abandoned` — keeper stopped retrying; a human must ask again.
    /// * `unknown` — no unit is accountable for this row.
    ///
    /// `unknown` is a real answer, not a gap to paper over: a row recorded
    /// before this column existed, or a conflict copy the merge wrote whose
    /// publication belongs to a commit that does not exist yet. The list renders
    /// no glyph at all for it, because inventing one would claim a fact.
    pub delivery: String,
    /// The last error recorded against the delivering unit, verbatim, or `null`.
    ///
    /// Present for `failed` and `abandoned`, and also on `inProgress` when the
    /// unit is being retried after an earlier failure or is waiting on a named
    /// condition. The Activity row shows it in a popover, which is the whole
    /// point: before this, the only way to learn why a file had not arrived was
    /// the Problems section far below, and that section names the unit rather
    /// than the file.
    pub failure: Option<String>,
    /// The delivering unit, present only while it still exists.
    ///
    /// The same id [`sync_retry_parked`] takes, which is what lets a file row
    /// offer Retry for the work that is actually stuck.
    #[ts(type = "number | null")]
    pub unit_id: Option<i64>,
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
///
/// Every `Option` here means the same thing when it is `None`: **the caller did
/// not express this field**, so `parse_req` leaves whatever the stored profile
/// already has (AD-34-9). None of them is a "reset to the default" instruction;
/// a caller that wants the default sends it. That rule is why adding a field to
/// `SyncProfile` can no longer silently erase it on the next save from the app.
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
    /// The quiescence window to pin. Sending `DEFAULT_SETTLE_MS` is how "let
    /// keeper choose the wait" is expressed, which is what `effective_settle_ms`
    /// reads as unpinned — so a removable folder gets its longer window back.
    #[ts(type = "number | null")]
    pub settle_ms: Option<u64>,
    /// The scan cadence to pin, in the same shape. The engine paces its tree
    /// walk by it, so this is the knob that governs how soon a change is noticed
    /// (DW-116); before Story 34.5 the app could not send it at all and every
    /// save reset a daemon-configured cadence back to 15 s.
    #[ts(type = "number | null")]
    pub poll_interval_ms: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Absent leaves whatever the stored profile already has, so a caller that
    /// does not know about this field cannot erase it. An explicit empty string
    /// clears the override back to the device identity.
    #[serde(default)]
    pub author_override: Option<String>,
    /// Shapes the generated commit subject. An explicit empty string clears it
    /// back to keeper's mechanical `sync(<profile>): 3 added, 1 modified`, which
    /// is a real value here rather than a clearing sentinel — an empty template
    /// IS the default. An unknown placeholder is refused with a message naming
    /// it, before the profile is stored.
    #[serde(default)]
    pub commit_subject_template: Option<String>,
    /// Flag or unflag this folder as a notes vault. `None` leaves the flag alone,
    /// so a form that does not show it cannot clear it (AD-34-9) — which matters
    /// more here than for most fields, because clearing it would make a whole
    /// vault disappear from the UI on the next unrelated save.
    #[serde(default)]
    pub notes: Option<bool>,
    /// The vault subfolder to pin. Only meaningful together with `notes`, and
    /// `None` keeps whatever is stored — including when `notes: Some(true)` is
    /// re-sent for an already-flagged folder, which must not reset a subfolder the
    /// user changed.
    #[serde(default)]
    pub notes_subfolder: Option<String>,
}

/// Mint an opaque, sortable, collision-free id.
///
/// A real ULID: a 48-bit millisecond timestamp in Crockford base32 followed by
/// 80 bits of randomness, 26 characters in total, so ids sort by creation and
/// read like the engine's own without pulling a second copy of the `ulid` crate
/// into the shell.
///
/// The length is load-bearing rather than cosmetic. Notes validate their
/// frontmatter `id` against the ULID shape (`notes_vault::is_ulid`) and index a
/// note whose id is not one under a path-derived identity instead. This
/// generator emitted 22 characters until 2026-08-03, so every note keeper wrote
/// failed keeper's own check: the note was indexed by path, flagged
/// `unstable_identity`, and could not be opened by the id its own frontmatter
/// carried.
///
/// Randomness comes from two independently seeded `RandomState` hashers — the
/// standard library seeds each from the OS — because one 64-bit hash is short of
/// the 80 bits the format wants. This only has to avoid collision between things
/// a human or an agent creates in one vault; it is not a security boundary, so a
/// CSPRNG would be theatre.
///
/// Shared with the notes surface (Phase 5), which needs exactly the same thing for
/// note ids, temp-file names and trash directories — one generator rather than two
/// that could drift in shape.
pub(crate) fn new_ulid() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();

    let entropy = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(millis ^ salt);
        hasher.finish()
    };
    let (high, low) = (entropy(0), entropy(u64::MAX));

    let mut out = String::with_capacity(26);
    // 10 characters = 50 bits, covering the 48-bit timestamp.
    for i in (0..10).rev() {
        out.push(ALPHABET[((millis >> (i * 5)) & 0x1f) as usize] as char);
    }
    // 16 characters = 80 bits, taken 8 from each hasher.
    for source in [high, low] {
        for i in (0..8).rev() {
            out.push(ALPHABET[((source >> (i * 5)) & 0x1f) as usize] as char);
        }
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
        SyncPhase::UploadingLfs => "uploadingLfs",
        SyncPhase::DownloadingLfs => "downloadingLfs",
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
///
/// **Exhaustive by construction — no `_` arm**, for the reason
/// `keeper-syncd`'s `sync_exit_code` has none: which code a new variant gets is
/// a decision, and `_` makes it `internal` silently. `internal` is what the app
/// says when it has hit a bug, so both variants Epic 34 added landed there — an
/// LFS 403 a person provoked by pressing "Sync now" reached the frontend
/// indistinguishable from a panic, and no affordance could be built for either.
pub fn sync_ipc_error(err: &SyncError) -> IpcError {
    let code = match err {
        SyncError::GitMissing { .. } => IpcErrorCode::Unsupported,
        // Beside `Auth`, as `sync_exit_code` puts it: both are a credential the
        // remote would not act on, and the remedies differ only in which thing a
        // human has to change. `IpcErrorCode` has no `forbidden`, and minting
        // one would be a frontend contract change to draw a distinction the two
        // `Display` strings already draw in the message the user reads.
        SyncError::Auth { .. } | SyncError::Forbidden { .. } => IpcErrorCode::InvalidCredentials,
        SyncError::Network { .. } => IpcErrorCode::ServerUnreachable,
        // A wait, not a fault: the push is held until this folder's own uploads
        // reach the remote. `syncUnavailable` is the app's "sync cannot continue
        // right now" code, which is exactly what this is. `retriable` stays
        // `false` on its own account — the wait is on the uploads, not on a
        // clock, and pressing "Sync now" again cannot shorten it.
        SyncError::LfsUploadPending { .. } => IpcErrorCode::SyncUnavailable,
        // Everything else keeps the `internal` this funnel has always given it.
        // Spelled out rather than defaulted so that growing the taxonomy asks
        // the question here instead of answering it.
        SyncError::GitCommand { .. }
        | SyncError::InvalidPathForRemote { .. }
        | SyncError::MediaAbsent
        | SyncError::Integrity { .. }
        | SyncError::Quota { .. }
        | SyncError::Diverged { .. }
        | SyncError::Journal(_)
        | SyncError::Git(_)
        | SyncError::Io { .. }
        | SyncError::Config(_)
        | SyncError::Cancelled => IpcErrorCode::Internal,
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

/// Build the profile to store from a request.
///
/// `prior` is the stored profile when this is an update, and it is the **base**
/// rather than a source of fix-ups (AD-34-9): `db::upsert_profile` replaces the
/// whole JSON row, so anything this function does not carry is erased on save.
/// Cloning `prior` makes that structural — a field added to `SyncProfile` is
/// preserved by default and has to be opted INTO the request, which is the
/// inverse of the old shape and the reason this bug class is now closed.
///
/// It bit twice. The first shape built from `SyncProfile::new` with no `prior`
/// at all, so saving an edit to a paused profile reset `enabled` to `true` and
/// quietly resumed syncing a folder the user had deliberately stopped, and wiped
/// any `author_override` set through the daemon. The fix for that re-added three
/// survivors by name — and `poll_interval_ms`, which the engine started pacing
/// its scans by on 2026-07-28 (DW-116), was not one of them, so every save from
/// the app silently pulled the cadence back to 15 s.
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

    // The stored profile IS the starting point. `SyncProfile::new` is reached
    // only when there is nothing to start from, and even then the assignments
    // below cover the same fields, so the two paths converge on one list of
    // "everything a request expresses" that can be read top to bottom.
    let mut profile = match prior {
        // The id is deliberately not reassigned from the request: `prior` IS the
        // row `req.id` named (`sync_profile_save` looked it up by exactly that),
        // and a request naming a different one would be re-pointing a row rather
        // than editing it.
        Some(prior) => prior.clone(),
        // A new profile needs an opaque stable id. The engine's ULID crate is
        // not a dependency of the shell and does not need to be: any
        // collision-free opaque string satisfies the contract, and the engine
        // treats it as entirely opaque.
        None => SyncProfile::new(
            req.id.clone().unwrap_or_else(new_ulid),
            &req.name,
            &req.local_path,
            &req.remote_url,
        ),
    };
    profile.name = req.name.clone();
    profile.local_path = req.local_path.clone().into();
    profile.remote_url = req.remote_url.clone();
    profile.branch = req.branch.clone();
    profile.direction = direction;
    profile.lane = lane;
    profile.subpaths = req.subpaths.clone();
    profile.excludes = req.excludes.clone();
    profile.removable = req.removable;
    profile.lfs_mode = lfs_mode;
    // Every `Option` below means "the caller did not express this", never "reset
    // it": a form that shows a knob sends the value it showed, and one that does
    // not show it must not be able to move it. `enabled` and `volume_id` have no
    // slot at all and so cannot be touched from here — `enabled` moves only via
    // the explicit pause/resume command, and the volume binding is minted by the
    // engine on first sight of the media, where dropping it would leave the
    // profile unbound and free to adopt whatever stick is mounted at its path,
    // including one that would otherwise have been refused as `Foreign`.
    if let Some(bytes) = req.lfs_threshold_bytes {
        profile.lfs_threshold_bytes = bytes;
    }
    if let Some(ms) = req.settle_ms {
        profile.settle_ms = ms;
    }
    if let Some(ms) = req.poll_interval_ms {
        profile.poll_interval_ms = ms;
    }
    profile.tags = req.tags.clone();
    // An explicit value overrides; an explicit empty string clears back to the
    // device identity, which is how the form offers "use the default".
    if let Some(author) = req.author_override.as_ref() {
        let trimmed = author.trim();
        profile.author_override = (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }
    // An empty template is a real value rather than a clearing sentinel: it IS
    // keeper's mechanical subject.
    if let Some(template) = req.commit_subject_template.as_ref() {
        profile.commit_subject_template = template.trim().to_owned();
    }
    // The notes flag, under the same rule as everything above it: `prior` is the
    // base, so flagging a vault cannot reset a knob the form did not show, and a
    // request that says nothing about notes leaves an existing vault exactly as it
    // is. Unflagging removes no files — it is a flag write and nothing else
    // (AD-54).
    match req.notes {
        Some(true) => {
            let mut config = profile.notes.clone().unwrap_or_default();
            if let Some(subfolder) = notes_subfolder(req) {
                config.subfolder = subfolder;
            }
            profile.notes = Some(config);
        }
        Some(false) => profile.notes = None,
        // Not expressed: a subfolder edit on its own still lands, so the vault
        // settings form can move the subfolder without re-asserting the flag.
        None => {
            if let (Some(config), Some(subfolder)) = (profile.notes.as_mut(), notes_subfolder(req))
            {
                config.subfolder = subfolder;
            }
        }
    }
    // Validate here so a bad profile is rejected at the edge with an actionable
    // message rather than deep inside the engine.
    profile.validate().map_err(|err| sync_ipc_error(&err))?;
    Ok(profile)
}

/// The subfolder a request expresses, normalised — or `None` when it expresses
/// none.
///
/// Trimmed of whitespace and of leading and trailing slashes, because a form field
/// commonly carries `notes/` and `SyncProfile::validate` refuses an absolute path.
/// An empty string after trimming is not an expression: the caller sent a blank
/// box, and the stored value (or the `notes` default) is the right answer.
fn notes_subfolder(req: &SyncProfileReq) -> Option<String> {
    let trimmed = req.notes_subfolder.as_ref()?.trim().trim_matches('/');
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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
///
/// The one thing it deletes outside `sync.db` is the profile's stored remote
/// credential, which `Engine::remove_profile` clears before it touches a row
/// (AD-34-14). That is not an exception to the sentence above: the secret is
/// keeper's own configuration, it never lived in the folder, and its keychain
/// key is derived from the profile id — so a secret that outlived its profile
/// could never be found again.
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

/// `s`, unless there is exactly one of whatever it is.
fn plural(count: u64) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

/// The answer when a sync had nothing to do.
///
/// This is the common case for a folder that is already in sync, and it is a
/// result rather than a failure. Saying it out loud is most of the point of
/// AD-34-12: a pass that stages nothing finishes in milliseconds, far inside
/// the 2 s status poll, so without a sentence of its own the button reads as
/// dead.
const NOTHING_TO_SYNC: &str = "Nothing to sync — this folder already matches the remote.";

/// One sentence naming what a sync did.
///
/// Composed in Rust for the same reason `SyncStatusVm.line` is: the Sync view
/// and the Settings row both render it verbatim, and two hand-written copies
/// of one wording drift.
///
/// `pushed` and `pulled` are deliberately not reported as work. They say which
/// legs the profile's direction allows, not that either leg carried anything,
/// and announcing "pushed" over a no-op push is the same class of lie as
/// `Keeper-Source: watch` on a manual sync. What is left is what actually
/// happened: commits made, bytes moved, revisions kept aside. A failed pass
/// never reaches here — the command returns `Err` and the caller renders that.
fn outcome_line(outcome: &SyncOutcome) -> String {
    let mut clauses: Vec<String> = Vec::new();
    if outcome.files_changed > 0 {
        // Only a profile that pushes ever commits, and a push that fails fails
        // the whole pass, so by here these files really are on the remote.
        clauses.push(format!(
            "committed and pushed {} file{}",
            outcome.files_changed,
            plural(outcome.files_changed)
        ));
    }
    if outcome.bytes > 0 {
        clauses.push(format!("moved {}", format_bytes(outcome.bytes)));
    }
    if !outcome.conflicts.is_empty() {
        let count = outcome.conflicts.len() as u64;
        clauses.push(format!(
            "kept your version of {count} file{} that changed in both places, \
             alongside the remote's",
            plural(count)
        ));
    }
    if clauses.is_empty() {
        return NOTHING_TO_SYNC.to_owned();
    }

    let mut line = clauses.join(", ");
    line.push('.');
    // Sentence case. Every clause above opens with an ASCII verb, so this is
    // the sentence's first letter and nothing else.
    if let Some(first) = line.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    line
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
        bytes: outcome.bytes,
        line: outcome_line(&outcome),
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
            size_bytes: row.size_bytes,
            delivery: delivery_str(row.delivery).to_owned(),
            failure: row.failure,
            unit_id: row.unit_id,
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
/// Writing is the common direction, but not the only one: [`sync_get_credential`]
/// reads the same key back when a person explicitly asks for it.
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

/// Read a profile's stored access token, on demand only.
///
/// Deliberately its own command rather than a field on [`SyncProfileVm`]
/// (AD-34-7): loading a profile must not carry a secret to the frontend, so
/// the token crosses the boundary only when someone asks for it by name. That
/// keeps the keychain prompt — on the platforms that raise one — attached to a
/// user action instead of to opening a form.
///
/// `Ok(None)` is the ordinary "no token stored" state rather than a failure: a
/// profile on a public remote has none, and the caller needs to be able to say
/// so instead of showing an empty box that reads like a broken read.
#[tauri::command]
pub async fn sync_get_credential(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<String>, IpcError> {
    let profile = profile_by_id(&state, &id)?;
    let platform = crate::sync::sync_platform(Arc::clone(&state.platform));
    platform
        .secret_get(&profile.secret_key())
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

/// This installation's identity, as Settings shows it (Story 34.5).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncDeviceVm {
    /// The stable id, shown but not editable. Every commit's `Keeper-Device`
    /// trailer records it beside the label so two machines the user has called
    /// the same thing stay distinguishable in a shared history, and the git
    /// author address is derived from it — so a rename must not move it.
    pub id: String,
    /// The editable name. Reaches the next commit; rewrites no old one.
    pub label: String,
}

impl From<keeper_sync::db::DeviceIdentity> for SyncDeviceVm {
    fn from(d: keeper_sync::db::DeviceIdentity) -> Self {
        Self {
            id: d.id,
            label: d.label,
        }
    }
}

/// This device's identity — the name that rides every commit keeper makes.
///
/// Minted once from the machine's hostname at first open and the user's from
/// then on, which is why it is read from the engine rather than re-derived: a
/// renamed device must not answer with its hostname again.
///
/// Rejects with: `unsupported` (no usable git), `internal`.
#[tauri::command]
pub async fn sync_device(state: tauri::State<'_, AppState>) -> Result<SyncDeviceVm, IpcError> {
    let engine = engine_of(&state)?;
    Ok(engine.device().into())
}

/// Rename this device, returning the identity as stored.
///
/// Returns the stored form rather than echoing the argument, because the store
/// trims it and refuses an empty one — a caller that assumed its own string had
/// been kept verbatim would render a label the trailers do not use.
///
/// Rejects with: `unsupported`, `internal` (an empty label).
#[tauri::command]
pub async fn sync_device_set_label(
    state: tauri::State<'_, AppState>,
    label: String,
) -> Result<SyncDeviceVm, IpcError> {
    let engine = engine_of(&state)?;
    engine
        .set_device_label(&label)
        .map_err(|err| sync_ipc_error(&err))?;
    Ok(engine.device().into())
}

/// Find `id` among the stored profiles, or report it as a config error naming
/// the id.
///
/// Pure over the list so the refusal of an id nothing stored is testable
/// without an engine behind it — the property [`sync_open_path`] leans on.
fn find_profile<'a>(profiles: &'a [SyncProfile], id: &str) -> Result<&'a SyncProfile, IpcError> {
    profiles
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| sync_ipc_error(&SyncError::Config(format!("no such sync profile: {id}"))))
}

/// Resolve a profile by id, or report it as a config error naming the id.
fn profile_by_id(state: &AppState, id: &str) -> Result<keeper_sync::SyncProfile, IpcError> {
    let engine =
        crate::sync::engine(Arc::clone(&state.platform)).map_err(|e| sync_ipc_error(&e))?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    find_profile(&profiles, id).cloned()
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

/// Forget what this profile remembers about its own tree, and look again.
///
/// The counterpart to [`sync_verify`], and a different question: verify asks
/// "is what I have intact", this asks "is what I *think* I have still what is
/// there". A file copied in with its modification time preserved can match the
/// remembered row exactly, and no amount of re-scanning finds it — the scan is
/// asking the wrong question. Clearing the memory is what changes the answer.
///
/// Returns nothing: the effect shows up as the folder's own Pending list on the
/// next walk, which is the thing the user was looking at when they pressed it.
#[tauri::command]
pub async fn sync_rescan(state: tauri::State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    engine.rescan(&id).map_err(|e| sync_ipc_error(&e))
}

/// An `IpcError` carrying a message written for the person who will read it.
///
/// Sync's errors normally funnel through [`sync_ipc_error`], which takes the
/// wording from the `SyncError`'s own `Display`. The open path has none to take:
/// a folder that is not on disk is not a failure the engine raised, and
/// `SyncError::Config`'s "invalid sync configuration" prefix would blame the
/// settings for a volume that is merely unplugged. Non-retriable, because
/// nothing changes until someone plugs the media back in or moves the folder
/// back.
fn open_failure(message: String) -> IpcError {
    IpcError {
        code: IpcErrorCode::Internal,
        message,
        account_id: None,
        retriable: false,
    }
}

/// Why this profile's folder cannot be opened, and what to do about it.
///
/// Both cases have to be said out loud, and they need different next steps: a
/// removable profile binds to media that gets unplugged (AD-48), which is a
/// pause rather than a fault, while a fixed folder that is gone was moved or
/// deleted outside keeper and needs the profile re-pointed. Reporting either as
/// a silent no-op — the shape a bare `reveal_item_in_dir` failure would take —
/// leaves a person clicking a path that does nothing.
fn unavailable_sentence(profile: &SyncProfile) -> String {
    let next_step = if profile.removable {
        "This folder lives on removable media — reattach the volume, then open it again."
    } else {
        "It was moved, renamed or deleted outside keeper — use Edit folder to point keeper at it."
    };
    format!("{} is not there. {next_step}", profile.local_path.display())
}

/// Open a profile's folder in the OS file manager (Story 32.4).
///
/// Takes the profile id and nothing else. The folder comes from the stored
/// profile, resolved here, so this command cannot be used to open an arbitrary
/// location: a webview that asked for `/etc` would have to have persuaded the
/// engine to store a profile pointing there first. `reveal_path` remains the
/// command for a path the frontend legitimately already holds (an export it
/// just produced); sync deliberately does not widen that reach, because the
/// only path it has to offer is one it can look up itself.
///
/// Reveals through the same `tauri_plugin_opener::reveal_item_in_dir` seam as
/// the recordings folder, the export reveal and the tray, so keeper has one way
/// of showing a folder rather than a second one that could behave differently.
///
/// Rejects with: `internal` (no such profile, the folder is gone or its volume
/// is not attached, the file manager refused).
#[tauri::command]
pub async fn sync_open_path(state: tauri::State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?;
    // Checked before the reveal, not left to it: the plugin fails the same way
    // whether the volume is out or the folder was deleted, and on some platforms
    // it succeeds at showing an empty window instead.
    if !profile.local_path.is_dir() {
        return Err(open_failure(unavailable_sentence(profile)));
    }
    tauri_plugin_opener::reveal_item_in_dir(&profile.local_path).map_err(|e| {
        open_failure(format!(
            "could not open {} in the file manager: {e}",
            profile.local_path.display()
        ))
    })
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
    /// Whole bytes per second, or `null` when there is no honest figure — too
    /// little time measured, or nothing moving. Never zero: "0 B/s" would claim
    /// a measurement of an idle wire, so the UI renders `null` as nothing.
    #[ts(type = "number | null")]
    pub bytes_per_second: Option<u64>,
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
            bytes_per_second: event.bytes_per_second,
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
            poll_interval_ms: None,
            tags: vec![],
            author_override: None,
            commit_subject_template: None,
            notes: None,
            notes_subfolder: None,
        }
    }

    /// Every serialized field of `SyncProfile`, split by whether a request can
    /// express it. These are the exact keys `db::upsert_profile` writes into the
    /// row, which is why the split is stated over the JSON rather than over the
    /// struct: the bug is a lost KEY, and serde is what decides what a key is.
    ///
    /// A field the request has a slot for.
    const EXPRESSED: [&str; 17] = [
        "name",
        "localPath",
        "remoteUrl",
        "branch",
        "direction",
        "lane",
        "subpaths",
        "excludes",
        "removable",
        "lfsMode",
        "lfsThresholdBytes",
        "settleMs",
        "pollIntervalMs",
        "tags",
        "authorOverride",
        "commitSubjectTemplate",
        "notes",
    ];

    /// A field no request can express, which `parse_req` must therefore never
    /// touch. `enabled` moves only through pause/resume, `volumeId` is minted by
    /// the engine on first sight of the media, `id` names the row, and the two
    /// LFS knobs (`lfsNever`, `lfsPruneLocal`) are configured through
    /// `keeper-syncd`'s profile file with no slot in the app's form.
    const PRESERVED: [&str; 5] = ["id", "volumeId", "enabled", "lfsNever", "lfsPruneLocal"];

    fn json_fields(profile: &SyncProfile) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(profile).expect("a profile serializes") {
            serde_json::Value::Object(map) => map,
            other => panic!("a profile is a JSON object, got {other}"),
        }
    }

    /// AD-34-9, mechanized. The old `parse_req` rebuilt the profile from
    /// `SyncProfile::new` and re-added three survivors by name, so a field added
    /// to `SyncProfile` after that list was written was silently reset by every
    /// save from the app — `poll_interval_ms` for two months (DW-116).
    ///
    /// This is written to fail when a twelfth field arrives and nobody thinks
    /// about it, which takes three assertions rather than one:
    ///
    /// 1. The classification is COMPLETE, so a new field fails here until it is
    ///    named as expressible or preserved — one line away from `parse_req`.
    /// 2. Nothing preserved moved, which is the property itself.
    /// 3. Every preserved field holds a value a fresh profile would NOT have, so
    ///    assertion 2 cannot pass by coincidence. Add a preserved field and this
    ///    fails until the fixture below gives it a distinctive value — at which
    ///    point assertion 2 genuinely bites.
    ///
    /// No reflection is involved: serde's own field names are the same mechanism
    /// the row is written with, so the test cannot disagree with the store.
    #[test]
    fn a_save_cannot_move_a_field_no_request_can_express() {
        // Everything unexpressible, set to something a fresh profile never has.
        let mut prior = parse_req(&req(), None).expect("valid");
        prior.enabled = false;
        prior.volume_id = Some("01VOLUME".into());
        prior.lfs_never = vec!["*.psd".into()];
        prior.lfs_prune_local = true;

        // An edit that moves every field it CAN move, so nothing below passes by
        // standing still.
        let mut edit = req();
        edit.id = Some(prior.id.clone());
        edit.name = "renamed".into();
        edit.local_path = "/home/u/elsewhere".into();
        edit.remote_url = "https://git.example/u/elsewhere.git".into();
        edit.branch = "trunk".into();
        // A worktree lane is legal only together with `pushOnly`, so these two
        // move as a pair.
        edit.direction = "pushOnly".into();
        edit.lane = "worktree".into();
        edit.subpaths = vec!["notes".into()];
        edit.excludes = vec!["*.tmp".into()];
        edit.removable = true;
        edit.lfs_mode = "pointerOnly".into();
        edit.lfs_threshold_bytes = Some(8 * 1024 * 1024);
        edit.settle_ms = Some(9_000);
        edit.poll_interval_ms = Some(45_000);
        edit.tags = vec!["drive".into()];
        edit.author_override = Some("Ada <ada@example.org>".into());
        edit.commit_subject_template = Some("{profile}: {changed}".into());
        // Flagging the folder as a vault moves `notes` from `None` to `Some`.
        edit.notes = Some(true);
        edit.notes_subfolder = Some("second-brain".into());
        let merged = parse_req(&edit, Some(&prior)).expect("valid");

        let before = json_fields(&prior);
        let after = json_fields(&merged);
        let fresh = json_fields(&SyncProfile::new(
            "00DIFFERENTID",
            "unused",
            "/unused",
            "unused",
        ));

        let mut classified: Vec<&str> = EXPRESSED.iter().chain(PRESERVED.iter()).copied().collect();
        classified.sort_unstable();
        let mut actual: Vec<&str> = before.keys().map(String::as_str).collect();
        actual.sort_unstable();
        assert_eq!(
            actual, classified,
            "SyncProfile gained or lost a field: name it in EXPRESSED (parse_req \
             assigns it from the request) or in PRESERVED (parse_req must never \
             touch it), and make sure parse_req agrees"
        );

        for key in PRESERVED {
            assert_eq!(
                before.get(key),
                after.get(key),
                "saving moved `{key}`, which no request can express"
            );
            assert_ne!(
                before.get(key),
                fresh.get(key),
                "the fixture leaves `{key}` at a fresh profile's value, so the \
                 assertion above would pass however parse_req behaved — give it \
                 a distinctive value in `prior`"
            );
        }

        for key in EXPRESSED {
            assert_ne!(
                before.get(key),
                after.get(key),
                "`{key}` is listed as expressible but the edit did not move it: \
                 either parse_req does not assign it, or this test's `edit` does \
                 not change it"
            );
        }
    }

    /// The notes flag under the same rule, spelled out because its failure mode is
    /// the loudest one in the phase: a save from a form that does not show the flag
    /// would make an entire vault — its list, its tray section, its capture
    /// destination — disappear from the UI, while every file stayed exactly where
    /// it was. Silent and baffling, which is why it gets its own assertion.
    #[test]
    fn a_save_that_says_nothing_about_notes_leaves_a_vault_flagged() {
        let mut prior = parse_req(&req(), None).expect("valid");
        prior.notes = Some(keeper_sync::profile::NotesConfig {
            subfolder: "second-brain".into(),
            ..Default::default()
        });

        // A form with no notes control at all.
        let merged = parse_req(&req(), Some(&prior)).expect("valid");
        let notes = merged.notes.expect("the vault flag survives a save");
        assert_eq!(
            notes.subfolder, "second-brain",
            "and so does the subfolder the user chose"
        );

        // A subfolder edit on its own lands without re-asserting the flag.
        let mut edit = req();
        edit.notes_subfolder = Some("/vault/".into());
        let moved = parse_req(&edit, Some(&prior)).expect("valid");
        assert_eq!(
            moved.notes.expect("still a vault").subfolder,
            "vault",
            "the slashes a form field carries are trimmed, since an absolute \
             subfolder is refused by validate"
        );

        // An explicit unflag is the one thing that clears it.
        let mut unflag = req();
        unflag.notes = Some(false);
        assert!(parse_req(&unflag, Some(&prior))
            .expect("valid")
            .notes
            .is_none());

        // A blank subfolder box is not an expression: the stored value wins.
        let mut blank = req();
        blank.notes = Some(true);
        blank.notes_subfolder = Some("   ".into());
        assert_eq!(
            parse_req(&blank, Some(&prior))
                .expect("valid")
                .notes
                .expect("still a vault")
                .subfolder,
            "second-brain"
        );

        // Flagging a folder that was never a vault gets the shipped default.
        let mut fresh = req();
        fresh.notes = Some(true);
        assert_eq!(
            parse_req(&fresh, None)
                .expect("valid")
                .notes
                .expect("now a vault")
                .subfolder,
            keeper_sync::profile::NotesConfig::default().subfolder
        );
    }

    /// The concrete case AD-34-9 was written for, spelled out beside the general
    /// one: `poll_interval_ms` became load-bearing on 2026-07-28 (DW-116) and
    /// `parse_req` did not know about it, so every save from the app pulled a
    /// cadence configured through `keeper-syncd` back to 15 s — silently, with
    /// nothing in the UI to hint that the folder had just got slower.
    #[test]
    fn saving_an_edit_does_not_reset_a_daemon_configured_scan_cadence() {
        let mut prior = parse_req(&req(), None).expect("valid");
        prior.poll_interval_ms = 45_000;

        let mut edit = req();
        edit.id = Some(prior.id.clone());
        edit.name = "renamed".into();
        // The form did not show the cadence, so it says nothing about it.
        assert!(edit.poll_interval_ms.is_none());

        let merged = parse_req(&edit, Some(&prior)).expect("valid");
        assert_eq!(merged.poll_interval_ms, 45_000);
        assert_eq!(merged.effective_poll_interval_ms(), 45_000);
    }

    #[test]
    fn a_commit_subject_template_round_trips_and_a_bad_one_is_refused_at_the_edge() {
        let mut set = req();
        set.commit_subject_template = Some("  {profile}: {changed} files  ".into());
        let stored = parse_req(&set, None).expect("valid");
        assert_eq!(stored.commit_subject_template, "{profile}: {changed} files");
        assert_eq!(
            SyncProfileVm::from(&stored).commit_subject_template,
            "{profile}: {changed} files"
        );

        // An empty string is a real value here, not an omission: it IS keeper's
        // mechanical subject, so it clears a template rather than keeping one.
        let mut cleared = req();
        cleared.commit_subject_template = Some(String::new());
        assert_eq!(
            parse_req(&cleared, Some(&stored))
                .expect("valid")
                .commit_subject_template,
            ""
        );

        // A typo is refused where the user can still see the field, with a
        // message naming it — not rendered into every commit from here on.
        let mut typo = req();
        typo.commit_subject_template = Some("{Profile} moved".into());
        let err = parse_req(&typo, None).expect_err("must reject");
        assert_eq!(err.code, IpcErrorCode::Internal);
        assert!(
            err.message.contains("{Profile}"),
            "the message must name the placeholder: {}",
            err.message
        );
    }

    /// AD-34-8. A form that showed a number the backend was about to substitute
    /// would be lying, and one that turned that substitution into a pinned value
    /// on the next save would take "let keeper choose" away for good. So the view
    /// model says both things: what the profile pins, and what is in force.
    #[test]
    fn the_view_model_separates_a_pinned_knob_from_the_one_in_force() {
        let mut unpinned = parse_req(&req(), None).expect("valid");
        let vm = SyncProfileVm::from(&unpinned);
        assert_eq!(vm.settle_ms, None, "a profile that pins nothing says so");
        assert_eq!(vm.effective_settle_ms, DEFAULT_SETTLE_MS);
        assert_eq!(vm.poll_interval_ms, None);
        assert_eq!(vm.effective_poll_interval_ms, DEFAULT_POLL_INTERVAL_MS);

        // The measured bug: a removable folder showed 5 while 10 was in force.
        unpinned.removable = true;
        let vm = SyncProfileVm::from(&unpinned);
        assert_eq!(vm.settle_ms, None);
        assert_eq!(vm.effective_settle_ms, 10_000);

        let mut pinned = unpinned.clone();
        pinned.settle_ms = 30_000;
        pinned.poll_interval_ms = 1_000;
        let vm = SyncProfileVm::from(&pinned);
        assert_eq!(vm.settle_ms, Some(30_000));
        assert_eq!(
            vm.effective_settle_ms, 30_000,
            "a pin beats the substitution"
        );
        assert_eq!(vm.poll_interval_ms, Some(1_000));
        assert_eq!(
            vm.effective_poll_interval_ms, 2_000,
            "a cadence below the floor is floored, and the form has to say so"
        );
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

    /// The generator's SHAPE is a contract, not a detail. Notes validate a
    /// frontmatter `id` against the ULID form and index a note whose id is not
    /// one under a path-derived identity instead — so when this emitted 22
    /// characters, every note keeper wrote failed keeper's own check and could
    /// not be opened by the id its own frontmatter carried. Observed on the
    /// agent-desktop run of 2026-08-03; the length assertion below is what stops
    /// it coming back.
    #[test]
    fn minted_ids_are_unique_sortable_and_shaped_like_a_ulid() {
        let ids: std::collections::BTreeSet<String> = (0..500).map(|_| new_ulid()).collect();
        assert_eq!(ids.len(), 500, "ids must not collide");
        for id in &ids {
            assert_eq!(id.len(), 26, "a ULID is 26 characters: {id}");
            assert!(
                id.bytes()
                    .all(|b| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&b)),
                "unexpected character in {id}"
            );
        }
        // The timestamp prefix makes them sort by creation, which is what makes
        // a profile list stable without a separate ordering column.
        let first = new_ulid();
        std::thread::sleep(std::time::Duration::from_millis(2));
        assert!(new_ulid() > first);
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

    /// The two conditions Epic 34 added, both of which the `_` arm sent to
    /// `internal` — the code the app reserves for its own bugs.
    ///
    /// Neither is one. A 403 is a permission a human has to grant, reachable
    /// from a user-initiated `sync_now` on an LFS transfer; a held push is a
    /// wait on this folder's own uploads. Arriving as `internal` left the
    /// frontend unable to tell either from a panic, so neither could ever be
    /// given an affordance.
    #[test]
    fn the_two_conditions_epic_34_added_do_not_arrive_as_internal_faults() {
        let forbidden = sync_ipc_error(&SyncError::Forbidden {
            host: "git.example.org".into(),
        });
        assert_ne!(forbidden.code, IpcErrorCode::Internal);
        // Beside `Auth`, its documented sibling: a credential the remote would
        // not act on. The message carries which of the two it is.
        assert_eq!(forbidden.code, IpcErrorCode::InvalidCredentials);
        assert!(
            !forbidden.retriable,
            "the same token gets the same 403 every time"
        );
        assert!(
            forbidden.message.contains("git.example.org"),
            "{}",
            forbidden.message
        );

        let pending = sync_ipc_error(&SyncError::LfsUploadPending { objects: 3 });
        assert_ne!(pending.code, IpcErrorCode::Internal, "a wait is not a bug");
        assert_eq!(pending.code, IpcErrorCode::SyncUnavailable);
        assert!(
            !pending.retriable,
            "the wait is on the uploads landing, not on a clock"
        );
    }

    /// The five wire spellings `SyncActivityVm.delivery` carries.
    ///
    /// Asserted here, where they are produced, because nothing else can reach
    /// them: the generated TypeScript types `delivery` as a bare `string`, so
    /// `tsc` cannot check a value, and `keeper-sync`'s camelCase serialization
    /// test covers `db::ActivityRow` through serde's derive — a different code
    /// path from this hand-written match, which is what [`sync_activity`]
    /// actually puts on the wire.
    ///
    /// A typo like `"inprogress"` makes `SYNC_DELIVERY_STATES[row.delivery]`
    /// `undefined`, `SyncDeliveryMark` return `null`, and every delivery glyph,
    /// popover and Retry button vanish from the Sync pane — with the Rust and
    /// TypeScript suites both green, because the frontend tests mock the IPC
    /// client and nothing in Rust touched this function.
    #[test]
    fn every_delivery_state_keeps_the_camel_case_spelling_the_ui_indexes_by() {
        for (state, wire) in [
            (DeliveryState::Success, "success"),
            (DeliveryState::InProgress, "inProgress"),
            (DeliveryState::Failed, "failed"),
            (DeliveryState::Abandoned, "abandoned"),
            (DeliveryState::Unknown, "unknown"),
        ] {
            assert_eq!(delivery_str(state), wire);
        }
    }

    /// AD-34-12. `Sync now` returned a full outcome and the UI rendered none
    /// of it, so a successful click produced no visible statement at all — the
    /// reported symptom was "even after clicking Sync now I cannot see that
    /// sync works". Every case has to say something, and each has to say
    /// something different.
    #[test]
    fn every_kind_of_pass_states_what_it_did() {
        // Nothing to do is a result, not silence and not a failure.
        assert_eq!(
            outcome_line(&SyncOutcome {
                pulled: true,
                pushed: true,
                ..SyncOutcome::default()
            }),
            NOTHING_TO_SYNC
        );

        // Work that happened, named.
        assert_eq!(
            outcome_line(&SyncOutcome {
                committed: Some("main".to_owned()),
                pushed: true,
                files_changed: 3,
                bytes: 2_048,
                ..SyncOutcome::default()
            }),
            "Committed and pushed 3 files, moved 2 KB."
        );

        // A pull that carried bytes but committed nothing locally still moved
        // something, and must not read as "nothing to do".
        assert_eq!(
            outcome_line(&SyncOutcome {
                pulled: true,
                bytes: 3_072,
                ..SyncOutcome::default()
            }),
            "Moved 3 KB."
        );

        // A conflict is not a checkmark: both revisions survive and the user
        // has to look.
        let line = outcome_line(&SyncOutcome {
            pulled: true,
            conflicts: vec!["notes.sync-conflict-20250725-120000-host.md".to_owned()],
            ..SyncOutcome::default()
        });
        assert_eq!(
            line,
            "Kept your version of 1 file that changed in both places, alongside the remote's."
        );
    }

    /// A pass whose legs all ran and moved nothing is the single most common
    /// outcome, and reporting it as "pushed" would be the same lie the
    /// provenance half of this story exists to remove.
    #[test]
    fn a_leg_that_ran_is_never_reported_as_work_it_did_not_do() {
        let ran_everything = SyncOutcome {
            pushed: true,
            pulled: true,
            ..SyncOutcome::default()
        };
        assert_eq!(outcome_line(&ran_everything), NOTHING_TO_SYNC);
        assert!(!outcome_line(&ran_everything).contains("push"));
    }

    /// Story 32.4. The command takes an id and looks the folder up, which is the
    /// whole reason it is not `reveal_path(path)`: the frontend cannot name a
    /// path here, so it cannot open one keeper does not already sync.
    #[test]
    fn opening_a_folder_takes_the_path_from_the_stored_profile() {
        let profiles = vec![
            SyncProfile::new("p1", "tgdrive", "/Users/alice/tgdrive", "u1"),
            SyncProfile::new("p2", "notes", "/Users/alice/notes", "u2"),
        ];
        let found = find_profile(&profiles, "p2").expect("p2 is stored");
        assert_eq!(found.local_path.to_str(), Some("/Users/alice/notes"));
    }

    /// An id nothing stored is refused rather than resolved to the first profile
    /// or to a default, either of which would open a folder nobody asked for.
    #[test]
    fn opening_an_unknown_profile_is_refused_and_names_the_id() {
        let profiles = vec![SyncProfile::new(
            "p1",
            "tgdrive",
            "/Users/alice/tgdrive",
            "u1",
        )];
        let err = find_profile(&profiles, "nope").expect_err("must refuse");
        assert_eq!(err.code, IpcErrorCode::Internal);
        assert!(err.message.contains("nope"), "the message names the id");
        assert!(!err.retriable);
        assert!(
            find_profile(&[], "p1").is_err(),
            "an empty store refuses rather than falling back"
        );
    }

    /// A folder that is not on disk has to say what to do about it, and the two
    /// cases do not say the same thing: removable media is unplugged and comes
    /// back (AD-48), a fixed folder was moved and has to be re-pointed. Neither
    /// may be a silent no-op.
    #[test]
    fn an_absent_folder_reports_something_a_person_can_act_on() {
        let fixed = SyncProfile::new("p1", "notes", "/Users/alice/notes", "u1");
        let moved = unavailable_sentence(&fixed);
        assert!(moved.contains("/Users/alice/notes"), "names the folder");
        assert!(
            moved.contains("Edit folder"),
            "names the control that re-points it"
        );

        let mut stick = SyncProfile::new("p2", "field", "/Volumes/stick/field", "u2");
        stick.removable = true;
        let detached = unavailable_sentence(&stick);
        assert!(
            detached.contains("/Volumes/stick/field"),
            "names the folder"
        );
        assert!(
            detached.contains("reattach"),
            "says to plug the volume back in"
        );
        assert_ne!(
            detached, moved,
            "a detached volume and a deleted folder need different next steps"
        );

        // Non-retriable either way: nothing changes until a person acts.
        let err = open_failure(detached);
        assert_eq!(err.code, IpcErrorCode::Internal);
        assert!(!err.retriable);
    }
}
