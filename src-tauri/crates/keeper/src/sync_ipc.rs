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

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use keeper_core::vm::{
    ExportReceiptVm, FilesDeleteDestinationVm, FilesDeletePlanVm, FilesDeleteReceiptVm,
    FilesDeleteRefusalVm, FilesEntryFacts, FilesEntrySyncVm, FilesEntryVm, FilesListingState,
    FilesListingVm, FilesSyncStatusVm, FilesWriteVm, IpcError, IpcErrorCode,
};
use keeper_sync::browse;
use keeper_sync::engine::{PendingReason, SyncOutcome};
use keeper_sync::exclude::ExcludeSet;
use keeper_sync::export::{self, ExportRefusal};
use keeper_sync::files_write::{self, WriteRefusal, WriteRoute, WriteScope};
use keeper_sync::profile::{
    LfsMode, ProfileState, SyncDirection, SyncLane, DEFAULT_POLL_INTERVAL_MS,
    DEFAULT_RECORDINGS_SUBFOLDER, DEFAULT_SESSIONS_SUBFOLDER, DEFAULT_SETTLE_MS,
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
    /// Whether this folder holds recordings (Story 41.1, AD-66).
    ///
    /// Beside the notes flag above and meaning the same kind of thing: a
    /// recordings root is not a configured object with a life of its own, it is
    /// this flag plus a subfolder on a profile that already exists. Story 41.7
    /// is what made it reachable — the block existed and nothing in the app
    /// could write one, so the destination picker 41.2 built never had a profile
    /// to offer.
    pub recordings: bool,
    /// The recordings subfolder that would be **in force**: the stored one when
    /// this folder holds recordings, and `RecordingsConfig`'s own default when it
    /// does not.
    ///
    /// Never `None`, which is the one place this deliberately does not mirror
    /// `notes_subfolder` directly above. That field is `None` for a folder that
    /// is not a vault, so the form has to prefill its box from a copy of the
    /// default it keeps in TypeScript (`SYNC_NOTES_DEFAULT_SUBFOLDER`) — a second
    /// spelling of a Rust constant, which is exactly the drift
    /// `keeper_sync::profile` spells its defaults once to prevent. Resolving it
    /// here instead means the form prefills from the value that would actually be
    /// used (AD-34-8) and never spells `recordings` at all.
    pub recordings_subfolder: String,
    /// Whether this folder contains a sessions zone (FR-222, AD-107).
    ///
    /// Beside the notes and recordings flags above and meaning the same kind of
    /// thing: a sessions root is not a configured object with a life of its
    /// own, it is this flag plus a subfolder on a profile that already exists.
    pub sessions: bool,
    /// The sessions subfolder that would be **in force**: the stored one when
    /// this folder holds sessions, and `SessionsConfig`'s own default when it
    /// does not. Never `None`, following `recordings_subfolder` directly above
    /// rather than `notes_subfolder` — the form prefills from the value that
    /// would actually be used, and `60-sessions` is spelled once, in Rust.
    pub sessions_subfolder: String,
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
            recordings: p.recordings.is_some(),
            // The stored value, or the default that would be used if this folder
            // were flagged right now. Resolved here rather than in the form, so
            // `DEFAULT_RECORDINGS_SUBFOLDER` is spelled once in the whole product.
            recordings_subfolder: p.recordings.as_ref().map_or_else(
                || DEFAULT_RECORDINGS_SUBFOLDER.to_owned(),
                |recordings| recordings.subfolder.clone(),
            ),
            sessions: p.sessions.is_some(),
            sessions_subfolder: p.sessions.as_ref().map_or_else(
                || DEFAULT_SESSIONS_SUBFOLDER.to_owned(),
                |sessions| sessions.subfolder.clone(),
            ),
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
    /// Transfers still to be delivered, and their bytes. `line` says both in
    /// words; the numbers are here so a surface can show them without parsing
    /// prose, the same reason `settling` is.
    #[ts(type = "number")]
    pub queued_files: u32,
    #[ts(type = "number")]
    pub queued_bytes: u64,
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
            queued_files: s.queued_files,
            queued_bytes: s.queued_bytes,
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
    /// Paths that took the remote's version with no copy kept, because the
    /// profile declares them regenerable. Nothing is on disk to clean up —
    /// these want the tool that writes them to run again.
    pub stale: Vec<String>,
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
    /// `settling` | `untracked` | `modified` | `added` | `deleted` | `incoming`.
    ///
    /// The first five are what this machine changed; `incoming` is an LFS
    /// object queued to arrive, which `git status` cannot see and which used to
    /// leave a folder pulling 53 GB reporting nothing as pending at all.
    pub reason: String,
    /// Announced size, for `incoming` only — the one thing worth knowing about
    /// an object that has not arrived. `null` for every other reason, where the
    /// file is already on this disk and its size is not what the row is about.
    #[ts(type = "number | null")]
    pub size_bytes: Option<u64>,
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

/// A file whose name is not text, as the Problems pane shows it (Story 47.2,
/// DW-200).
///
/// The shell-side shape of `keeper_sync::UnspellableName`, for the same reason
/// [`SyncParkedVm`] is the shell-side shape of a parked unit: the engine type
/// is not a view model and does not derive [`TS`].
///
/// **Both renderings, and neither alone is enough.** `display` is what a person
/// reads and it is LOSSY and non-injective — two different files can produce
/// one `display`, which is the whole defect story 47.2 closed. `escaped` is
/// byte-exact ASCII, and it is what makes the row actionable: a person can
/// paste it into a shell and find the file. A row carrying only the lossy
/// rendering names a file the reader cannot then locate; one carrying only the
/// escaped form is unreadable.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SyncUnspellableVm {
    /// `U+FFFD`-substituted rendering: what the row reads as.
    pub display: String,
    /// Byte-exact ASCII, `\xNN` per byte outside printable ASCII: what the row
    /// can be acted on with.
    pub escaped: String,
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
    /// Files in this folder whose names are not text (Story 47.2, DW-200).
    ///
    /// Reported rather than silent. Before this, keeper rendered such a name
    /// lossily and said nothing, and the rendering could be joined back to a
    /// *different real file* — so a delete confirmed against one row removed
    /// another. `browse::plain_segments` now refuses the rendering; this list is
    /// how the person finds out the file exists at all.
    pub unspellable: Vec<SyncUnspellableVm>,
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
    /// Flag or unflag this folder as holding recordings (Story 41.7, AD-66).
    /// `None` leaves the flag alone under exactly the rule `notes` follows above:
    /// a caller with no control for it must not be able to clear it, and clearing
    /// it would take a folder out of the Recording destination picker while every
    /// file stayed where it was.
    #[serde(default)]
    pub recordings: Option<bool>,
    /// The recordings subfolder to pin. `None` keeps whatever is stored — or,
    /// when this flags the folder for the first time, lets `RecordingsConfig`'s
    /// own default stand, which is how a form that shows an untouched box says
    /// "keeper picks".
    ///
    /// Unlike `notes_subfolder`, an explicit empty string is a value and NOT an
    /// omission: see [`recordings_subfolder`] for why nothing here is tidied up
    /// on the caller's behalf.
    #[serde(default)]
    pub recordings_subfolder: Option<String>,
    /// Flag or unflag this folder as holding a sessions zone (FR-222, AD-107).
    /// `None` leaves the flag alone under exactly the rule `notes` and
    /// `recordings` follow above: a caller with no control for it must not be
    /// able to clear it, and clearing it would take a whole zone off the
    /// Sessions surface while every file stayed where it was.
    #[serde(default)]
    pub sessions: Option<bool>,
    /// The sessions subfolder to pin. `None` keeps whatever is stored — or,
    /// when this flags the folder for the first time, lets `SessionsConfig`'s
    /// own default (`60-sessions`) stand. Follows `recordings_subfolder`'s
    /// verbatim rule, not `notes_subfolder`'s tidying: the validator refuses by
    /// name, and a silent correction would save against a folder nobody named.
    #[serde(default)]
    pub sessions_subfolder: Option<String>,
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
        // The same shape one line up: a wait rather than a fault. Another
        // machine reached the shared branch first, so this pass published
        // nothing and the reconcile it queued is what makes the next one land
        // (DW-207). `syncUnavailable` says "sync cannot continue right now",
        // which is the truth for the moment it lasts — and `internal` would
        // dress a routine race as a defect in front of the user.
        SyncError::RemoteMoved { .. } => IpcErrorCode::SyncUnavailable,
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
    // The recordings flag (Story 41.7, AD-66) — the notes block directly above,
    // applied a second time and deliberately not reinvented. `RecordingsConfig`
    // and its validation shipped in Story 41.1 and the destination picker in
    // 41.2, and neither was reachable, because no request could express this
    // field: the app could read a recordings block written by `keeper-syncd` and
    // could never write one. Unflagging REMOVES the block rather than storing an
    // empty one, because `None` is "holds no recordings" and a default-filled
    // block would nominate the folder as a destination nobody chose.
    match req.recordings {
        Some(true) => {
            let mut config = profile.recordings.clone().unwrap_or_default();
            if let Some(subfolder) = recordings_subfolder(req) {
                config.subfolder = subfolder;
            }
            profile.recordings = Some(config);
        }
        Some(false) => profile.recordings = None,
        // Not expressed: as for notes, a subfolder edit on its own still lands on
        // a folder that already holds recordings, and says nothing about one that
        // does not.
        None => {
            if let (Some(config), Some(subfolder)) =
                (profile.recordings.as_mut(), recordings_subfolder(req))
            {
                config.subfolder = subfolder;
            }
        }
    }
    // The sessions flag (FR-222, AD-107) — the recordings block directly above,
    // applied a third time and deliberately not reinvented. Unflagging REMOVES
    // the block rather than storing an empty one, because `None` is "holds no
    // sessions zone" and a default-filled block would put a Sessions surface
    // over a folder nobody flagged. Unflagging removes no files — it is a flag
    // write and nothing else.
    match req.sessions {
        Some(true) => {
            let mut config = profile.sessions.clone().unwrap_or_default();
            if let Some(subfolder) = sessions_subfolder(req) {
                config.subfolder = subfolder;
            }
            profile.sessions = Some(config);
        }
        Some(false) => profile.sessions = None,
        // Not expressed: as above, a subfolder edit on its own still lands on a
        // folder that already holds a zone, and says nothing about one that
        // does not.
        None => {
            if let (Some(config), Some(subfolder)) =
                (profile.sessions.as_mut(), sessions_subfolder(req))
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

/// The recordings subfolder a request expresses, or `None` when it expresses
/// none.
///
/// Whitespace-trimmed and otherwise **verbatim**, which is the one way this does
/// not copy [`notes_subfolder`] above (Story 41.7). Stripping slashes there is a
/// convenience; here it would be a silent correction of the exact inputs
/// `RecordingsConfig::validate` exists to refuse by name — `/tmp` would become
/// `tmp`, and a save the owner should have been told about would succeed against
/// a folder they did not name. An empty string is passed through for the same
/// reason, so the validator gets to say "recordings subfolder must not be empty"
/// itself rather than have the answer guessed at here.
///
/// `None` — the key absent from the request altogether — is still the AD-34-9
/// omission that leaves whatever is stored alone.
fn recordings_subfolder(req: &SyncProfileReq) -> Option<String> {
    req.recordings_subfolder
        .as_ref()
        .map(|raw| raw.trim().to_owned())
}

/// The sessions subfolder a request expresses, or `None` when it expresses
/// none. Verbatim after a whitespace trim, for the reason
/// [`recordings_subfolder`] is: `SessionsConfig::validate` refuses bad shapes
/// by name, and correcting them here would hide the refusal.
fn sessions_subfolder(req: &SyncProfileReq) -> Option<String> {
    req.sessions_subfolder
        .as_ref()
        .map(|raw| raw.trim().to_owned())
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
    app: tauri::AppHandle,
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
    // The sessions flag rides this save (FR-222), and the root registry is a
    // filter over the profile list (AD-107) — so the registry re-reads here,
    // the same move `notes_vault_flag` makes for vaults. Idempotent and cheap
    // when nothing sessions-shaped changed.
    crate::sessions_root::refresh(&app);
    Ok(SyncProfileVm::from(&profile))
}

/// Forget a profile. The folder and its repository are left on disk untouched —
/// removing a profile is a configuration change, never a deletion of content.
///
/// What it deletes outside `sync.db` is the profile's secrets, both of them:
/// the stored remote credential, which `Engine::remove_profile` clears before
/// it touches a row (AD-34-14), and the `Authorization` the engine minted from
/// it over ssh for LFS, which is dropped in the same step. Neither is an
/// exception to the sentence above: they are keeper's own configuration, they
/// never lived in the folder, and the keychain key is derived from the profile
/// id — so a secret that outlived its profile could never be found again.
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
    if !outcome.stale.is_empty() {
        // Deliberately not phrased as a conflict: nothing was kept, nothing has
        // to be deleted, and the only action is to run whatever writes the file.
        let count = outcome.stale.len() as u64;
        clauses.push(format!(
            "took the remote's version of {count} generated file{} — regenerate {}",
            plural(count),
            if count == 1 { "it" } else { "them" }
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
        stale: outcome.stale.clone(),
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
            let (reason, since_ms, size_bytes) = match file.reason {
                PendingReason::Settling { since_ms } => ("settling", Some(since_ms), None),
                PendingReason::Incoming { size_bytes } => ("incoming", None, Some(size_bytes)),
                PendingReason::Untracked => ("untracked", None, None),
                PendingReason::Modified => ("modified", None, None),
                PendingReason::Added => ("added", None, None),
                PendingReason::Deleted => ("deleted", None, None),
            };
            SyncPendingVm {
                path: file.path,
                reason: reason.to_owned(),
                since_ms,
                size_bytes,
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
        // Hand-projected like every field above it, so a field added to
        // `ProblemReport` reaches the pane by someone deciding it should rather
        // than by inheriting a derive (Story 47.2, DW-200).
        unspellable: report
            .unspellable
            .into_iter()
            .map(|name| SyncUnspellableVm {
                display: name.display,
                escaped: name.escaped,
            })
            .collect(),
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
/// reads the same key back — since Story 34.12, as an edit form opens.
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

/// Read a profile's stored access token.
///
/// Deliberately its own command rather than a field on [`SyncProfileVm`]
/// (AD-34-7): loading the profile list must not carry a secret to the
/// frontend, so the token crosses the boundary only for the one profile a
/// caller names. Story 34.12 then made the edit form call this on mount, which
/// overrode the second half of AD-34-7 — the read is no longer tied to a
/// user's press, and the doc that said so was wrong from that story onward.
///
/// # Why this is not gated
///
/// The epic-34 security review (finding 4 against Story 34.12) asked for a
/// user-presence check, a confirmation or a rate limit in front of this
/// command, on the grounds that anything executing in the webview can call
/// `sync_profiles` for the ids and then read every stored token. The
/// observation is correct. The gate is still refused, and the reasoning is
/// recorded here rather than left implicit:
///
/// * **Every `#[tauri::command]` in the invoke handler is equally reachable.**
///   `capabilities/*.json` gates plugin permissions, not these functions. So a
///   gate here would protect one command out of dozens, several of which are
///   strictly more dangerous to the same attacker than a token read — most
///   obviously `sync_profile_save`, which can repoint a folder's remote at a
///   host of the attacker's choosing and let the next sync push the entire
///   folder there. Hardening one read while that stays open buys nothing.
/// * **The renderer is inside the trust boundary, by construction.** The
///   webview loads `frontendDist` — keeper's own bundled assets — and no
///   remote origin; no message, note or profile field is rendered as raw HTML
///   (there is no `dangerouslySetInnerHTML` anywhere in `src/`). Script running
///   in there means the bundle itself is compromised, and an attacker in that
///   position already holds the notes, recordings and vaults the token merely
///   fetches — and can plant a token of their own through
///   `sync_set_credential`. The honest caveat: `tauri.conf.json` sets
///   `"csp": null`, so nothing would constrain where such a script sent what it
///   read. That is an argument for a CSP, which is a build-configuration
///   change covering every command at once, not for a gate on this one.
/// * **A prompt here would be theatre, not a control.** The platforms that
///   raise a keychain prompt already raise it inside `secret_get`; a second,
///   keeper-drawn confirmation would train the user to click through the real
///   one. And a caller who can invoke commands can invoke them again, so a rate
///   limit only slows an enumeration that has all day.
///
/// What genuinely limits exposure is not a gate but *how long the plaintext
/// lives in the renderer*, which is a UI question Story 34.12 owns: the field
/// is masked, the form unmounts on close, and no token reaches `SyncProfileVm`
/// or any log. The one narrowing still worth having — the mount read fires on
/// Edit even when the Advanced disclosure that shows the field is never opened,
/// so the secret is pulled in wider than the surface that displays it — is a
/// change to the form, not to this command, and is filed as DW-147.
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
/// The remote half rides along (DW-208). "Recheck all files" is the one action
/// whose plain meaning is *look at everything again*, and until now it looked
/// only at the worktree — which cannot see the failure that actually loses
/// content, a pointer whose object never reached the server. Nothing else in
/// the engine ever retries one: uploads are queued only for files being freshly
/// staged, so an obligation dropped once is dropped for good.
///
/// It runs after the rescan and its failure is not the button's failure: the
/// local half has already taken effect, and a folder whose remote is
/// unreachable must still be able to forget what it remembers. The repair is
/// reported through the profile's own warnings, where the paths it could not
/// recover are the part a human has to act on.
#[tauri::command]
pub async fn sync_rescan(state: tauri::State<'_, AppState>, id: String) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    engine.rescan(&id).map_err(|e| sync_ipc_error(&e))?;
    match engine.republish_missing_objects(&id).await {
        Ok(report) if report.missing > 0 => tracing::info!(
            profile = id,
            missing = report.missing,
            queued = report.queued,
            unrecoverable = report.unrecoverable.len(),
            "recheck: re-publishing what the server was missing"
        ),
        Ok(_) => {}
        Err(err) => tracing::warn!(
            profile = id,
            error = %err,
            "recheck: could not ask the server what it is missing"
        ),
    }
    Ok(())
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

/// One directory of one synced folder, for the Files tab (Story 43.8, FR-153,
/// AD-74, AD-75, AD-65).
///
/// **Listing is the shell's job, and this is the only place it is done.** The
/// frontend hands back a `subpath` this command previously produced and never
/// composes one; `keeper_sync::browse` resolves it against the profile's own
/// root, refuses anything that is not a plain descendant of it — lexically and
/// again after symlinks are followed — and applies the profile's tier-0
/// exclusions. A webview that asked for `../../.ssh` would have to have
/// persuaded the engine to store a profile pointing there first.
///
/// **Lazy, one directory per call.** These trees hold a hundred thousand files.
/// Nothing here descends, and the cap is reported rather than hidden.
///
/// **Read-only about the engine, not only about the disk.** `browse` takes a
/// `&SyncProfile` and not the engine, so looking at a folder cannot spend the
/// stability gate's verdict, clear `file_state`, move the scan clock or wake
/// the watcher. The listing runs on the blocking pool because a directory on a
/// pendrive can take hundreds of milliseconds to stat, and stalling the async
/// runtime would freeze every other profile's poll behind a click.
///
/// **The sync mark is read, not recomputed** (Story 44.17, FR-173).
/// [`keeper_sync::engine::Engine::pending`] is already the one derived answer
/// to "what has this folder not synced yet, and why", and [`sync_pending`]
/// renders the same list as the Pending card. Calling it here rather than
/// asking git a second question is what keeps the two surfaces from ever
/// wording the same file differently — and it is the reason a mark cannot
/// become a second source of sync truth.
///
/// An engine that cannot produce that list does not fail the listing. A folder
/// whose repository is unreadable is exactly the folder somebody most needs to
/// look inside, so the entries still come back, marked
/// [`FilesSyncStatusVm::Unknown`] with the engine's own words attached.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a malformed
/// profile exclude pattern, a subpath that escapes the root, an unreadable
/// directory).
/// How long a listing waits for the sync marks before showing the folder without
/// them.
///
/// [`Engine::pending`] is a `git status` over the whole worktree plus an
/// untracked expansion that `lstat`s every candidate. On a folder of tens of
/// thousands of files, on a drive already saturated by its own transfers, that
/// is minutes — and every one of them was spent with the Files pane showing
/// nothing at all, because the listing waited for marks it does not need in
/// order to name a directory's entries.
const BROWSE_MARKS_BUDGET: Duration = Duration::from_secs(3);

/// How long one answer is reused by later listings.
///
/// Short, because it decorates rows a person is looking at. Long enough to
/// cover the burst that matters: the Files pane's refresh re-reads EVERY open
/// directory, so ten expanded folders used to mean ten whole-repository walks
/// of the same tree at the same moment.
const BROWSE_MARKS_TTL: Duration = Duration::from_secs(3);

/// What the row's `unavailable` reason says when the walk outran its budget.
const BROWSE_MARKS_SLOW_SENTENCE: &str =
    "This folder is busy, so the sync marks are not ready yet. They appear on the next listing.";

/// The last pending view per profile, and whether one is being computed.
///
/// A process-wide memo rather than a field on `AppState`: it is a cache of an
/// answer the engine already owns, it must be shared by every window, and
/// nothing outside this one command reads it. `Instant` is stamped when the
/// answer arrives, not when it was asked for, so a walk that took two minutes
/// does not hand back a view that is two minutes stale the moment it lands.
static BROWSE_MARKS: OnceLock<Mutex<HashMap<String, MarkSlot>>> = OnceLock::new();

#[derive(Default)]
struct MarkSlot {
    answered: Option<(Instant, browse::PendingView)>,
    walking: bool,
}

/// What a listing should do about one profile's marks.
#[derive(Debug, PartialEq, Eq)]
enum MarkPlan {
    /// Use this answer as it stands.
    Serve(browse::PendingView),
    /// A walk is already running and this is the best answer there is. `None`
    /// means there has never been one, and the row says so rather than
    /// claiming every entry is clean.
    ServeWhileWalking(Option<browse::PendingView>),
    /// Nothing usable and nobody walking: start one.
    Walk,
}

impl MarkSlot {
    /// The policy, separated from the plumbing so it can be read and tested
    /// without an engine, a disk or a clock.
    fn plan(&self, ttl: Duration) -> MarkPlan {
        if let Some((at, view)) = self.answered.as_ref() {
            if at.elapsed() < ttl {
                return MarkPlan::Serve(view.clone());
            }
        }
        if self.walking {
            return MarkPlan::ServeWhileWalking(self.answered.as_ref().map(|(_, v)| v.clone()));
        }
        MarkPlan::Walk
    }
}

fn browse_marks() -> &'static Mutex<HashMap<String, MarkSlot>> {
    BROWSE_MARKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The sync marks for one profile: cached, or computed within a budget.
///
/// The walk is spawned rather than awaited in place, and the task stores its
/// own result — so a walk that outruns [`BROWSE_MARKS_BUDGET`] is not wasted.
/// Dropping the `JoinHandle` (which is what the timeout does) does not cancel a
/// tokio task, so it runs to completion and the NEXT listing finds the answer
/// waiting. Without that, a folder slow enough to time out once would never
/// show a mark at all.
async fn browse_marks_for(
    engine: &Arc<keeper_sync::engine::Engine>,
    id: &str,
) -> (browse::PendingView, Option<String>) {
    {
        let mut marks = browse_marks().lock().expect("browse marks lock");
        let slot = marks.entry(id.to_owned()).or_default();
        match slot.plan(BROWSE_MARKS_TTL) {
            MarkPlan::Serve(view) => return (view, None),
            // One walk at a time per folder. A second would read the same tree
            // off the same disk for the same answer, and on this hardware that
            // is the difference the transfers feel.
            MarkPlan::ServeWhileWalking(Some(view)) => return (view, None),
            MarkPlan::ServeWhileWalking(None) => {
                return (
                    browse::PendingView::Unavailable,
                    Some(BROWSE_MARKS_SLOW_SENTENCE.to_owned()),
                )
            }
            MarkPlan::Walk => slot.walking = true,
        }
    }

    let walker = {
        let engine = Arc::clone(engine);
        let id = id.to_owned();
        tokio::spawn(async move {
            let answered = engine.pending(&id).await;
            let mut marks = browse_marks().lock().expect("browse marks lock");
            let slot = marks.entry(id).or_default();
            slot.walking = false;
            match &answered {
                Ok(files) => {
                    slot.answered = Some((
                        Instant::now(),
                        browse::PendingView::from_pending(files.clone()),
                    ));
                }
                Err(_) => slot.answered = None,
            }
            answered.map(browse::PendingView::from_pending)
        })
    };

    match tokio::time::timeout(BROWSE_MARKS_BUDGET, walker).await {
        Ok(Ok(Ok(view))) => (view, None),
        Ok(Ok(Err(error))) => (browse::PendingView::Unavailable, Some(error.to_string())),
        // The task panicked. The slot stays flagged as walking, which is the
        // safe way round: it stops a panicking walk being re-entered on every
        // keystroke, and a restart clears it.
        Ok(Err(join)) => (browse::PendingView::Unavailable, Some(join.to_string())),
        Err(_) => (
            browse::PendingView::Unavailable,
            Some(BROWSE_MARKS_SLOW_SENTENCE.to_owned()),
        ),
    }
}

#[tauri::command]
pub async fn sync_browse(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<FilesListingVm, IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?.clone();
    let excludes = ExcludeSet::new(&profile.excludes).map_err(|e| sync_ipc_error(&e))?;

    // Before the walk, so one answer covers every entry in the directory and a
    // thousand-row folder asks the engine once — and bounded, so a folder whose
    // walk takes minutes still lists its entries now. See `browse_marks_for`.
    let (pending, unavailable) = browse_marks_for(&engine, &id).await;

    let listing = {
        let profile = profile.clone();
        let subpath = subpath.clone();
        tokio::task::spawn_blocking(move || browse::browse(&profile, &subpath, &excludes, &pending))
            .await
            .map_err(|err| open_failure(format!("could not read the folder: {err}")))?
    }
    .map_err(|refusal| open_failure(refusal.to_string()))?;

    // Built from the vault keeper can actually REACH, not from the profile's
    // stored configuration (Story 45.3). A profile whose `notes` block names a
    // vault the registry has no slot for is a profile whose write commands will
    // refuse, and a listing that said "writable" from the config would put a
    // Delete button over exactly that case.
    let (_vault, scope) = vault_and_scope(&profile);
    Ok(files_listing_vm(
        &profile,
        subpath,
        listing,
        unavailable.as_deref(),
        &scope,
    ))
}

/// Project one [`BrowseListing`] into the VM the Files tab renders.
///
/// The `entries`/`state` pairing is the contract the surface depends on:
/// `Some` only under [`FilesListingState::Listed`], so a pane cannot render
/// "this folder is empty" for a drive that is out without first unwrapping and
/// meeting the state that says otherwise.
///
/// `engine_failure` is the engine's own words for why it could not produce a
/// pending list, threaded through so an `unknown` mark carries a reason
/// instead of a shrug.
///
/// The folder roles come off the profile that is already in hand (Story 45.5):
/// `notes.subfolder` and `recordings.subfolder` are the only evidence that a
/// folder is the vault or the recordings root, and reading them here — rather
/// than letting a surface match a name — is why a vault called `Second Brain`
/// is marked and an ordinary folder called `10-notes` is not. Borrowed for the
/// whole listing, so a thousand rows resolve against two `&str`.
fn files_listing_vm(
    profile: &SyncProfile,
    subpath: String,
    listing: browse::BrowseListing,
    engine_failure: Option<&str>,
    scope: &files_write::WriteScope<'_>,
) -> FilesListingVm {
    let roles = keeper_core::vm::FilesFolderRoles {
        notes_subfolder: profile.notes.as_ref().map(|notes| notes.subfolder.as_str()),
        recordings_subfolder: profile
            .recordings
            .as_ref()
            .map(|recordings| recordings.subfolder.as_str()),
    };
    let (state, entries, detail, truncated) = match listing {
        browse::BrowseListing::Listed(dir) => {
            let entries = dir
                .entries
                .into_iter()
                .map(|entry| {
                    let sync = sync_mark(&entry.sync, engine_failure);
                    // AD-102: three answers, not two. `owner` is the lexical
                    // half of the same classifier `sync_write_entry` routes
                    // through, so the flag a row renders and the writer the
                    // command picks cannot come apart — and it is lexical
                    // precisely so a thousand rows do not cost a thousand
                    // `canonicalize` calls to re-learn the `is_dir` the dirent
                    // just supplied.
                    let write = match scope.owner(&entry.relative_path, entry.is_dir) {
                        Ok(files_write::WriteOwner::Vault) => FilesWriteVm::allowed(),
                        Ok(files_write::WriteOwner::Unmanaged) => FilesWriteVm::unmanaged(
                            scope.unmanaged_caveat(&entry.name),
                            // Story 53.3: the same fact in one line, for the
                            // surface that folds the caveat away. Composed here
                            // beside the whole sentence, because a webview that
                            // clipped one to make the other would be
                            // paraphrasing the clause that names what is
                            // missing.
                            scope.unmanaged_caveat_short(&entry.name),
                        ),
                        Err(refusal) => FilesWriteVm::refused(refusal.to_string()),
                    };
                    FilesEntryVm::new(FilesEntryFacts {
                        name: entry.name,
                        relative_path: entry.relative_path,
                        absolute_path: entry.absolute_path.to_string_lossy().into_owned(),
                        is_dir: entry.is_dir,
                        sync,
                        size_bytes: entry.size_bytes,
                        roles,
                        write,
                    })
                })
                .collect::<Vec<_>>();
            let detail = dir.truncated.then(|| {
                format!(
                    "This folder holds more than {cap} items — showing the first \
                     {cap}. Open it in Finder to see the rest.",
                    cap = browse::LISTING_CAP,
                )
            });
            (
                FilesListingState::Listed,
                Some(entries),
                detail,
                dir.truncated,
            )
        }
        // The same sentence `sync_open_path` refuses with, because it is the
        // same fact: this folder is not reachable right now, and the next step
        // depends only on whether it lives on removable media.
        browse::BrowseListing::MediaAbsent => (
            FilesListingState::MediaAbsent,
            None,
            Some(unavailable_sentence(profile)),
            false,
        ),
        browse::BrowseListing::MediaUnexpected { found_id } => (
            FilesListingState::MediaUnexpected,
            None,
            Some(format!(
                "A different volume ({found_id}) is mounted where {} lives. \
                 keeper will not list a folder it cannot prove is yours — \
                 eject it and reattach the right drive.",
                profile.name
            )),
            false,
        ),
        browse::BrowseListing::Missing => (
            FilesListingState::Missing,
            None,
            Some(missing_sentence(profile, &subpath)),
            false,
        ),
    };
    // The directory's OWN verdict — "can a file be created in here" — which is
    // a different question from any entry's. Refused outright for every state
    // that produced no entries: a folder keeper could not read is not a folder
    // keeper will write into, and offering New file over an unplugged drive is
    // the "action that will fail" this field exists to remove.
    let write = if state == FilesListingState::Listed {
        FilesWriteVm::from_verdict(&scope.directory(&subpath))
    } else {
        FilesWriteVm::refused(detail.clone().unwrap_or_else(|| {
            "keeper could not read this folder, so it will not write in it.".to_owned()
        }))
    };
    FilesListingVm {
        profile_id: profile.id.clone(),
        subpath,
        state,
        entries,
        detail,
        truncated,
        write,
    }
}

/// Word one entry's sync state (Story 44.17, FR-173).
///
/// **The sentence is composed here and not in TypeScript** — the same rule the
/// listing's own `detail` follows. The reason is not tidiness: the browser and
/// the Pending card describe the same engine state, and a second copy of these
/// words in the frontend is a second copy that will be edited once.
///
/// A folder's roll-up carries no [`PendingReason`] of its own, and it is worded
/// as a folder rather than borrowing whichever descendant's word came first
/// alphabetically. "This folder is untracked" about a folder holding one new
/// file would be a small, confident lie.
fn sync_mark(status: &browse::EntrySyncStatus, engine_failure: Option<&str>) -> FilesEntrySyncVm {
    match status {
        browse::EntrySyncStatus::Synced => FilesEntrySyncVm::plain(FilesSyncStatusVm::Synced),
        browse::EntrySyncStatus::Waiting { reason } => FilesEntrySyncVm::explained(
            FilesSyncStatusVm::Waiting,
            match reason {
                Some(PendingReason::Settling { .. }) => {
                    "keeper is waiting for this file to finish being written."
                }
                Some(PendingReason::Untracked) => {
                    "This file is new and has not been committed yet."
                }
                Some(PendingReason::Modified) => {
                    "This file has changed and has not been committed yet."
                }
                Some(PendingReason::Added) => "This file is staged and has not been committed yet.",
                Some(PendingReason::Deleted) => {
                    "This file has been deleted and the deletion has not been committed yet."
                }
                Some(PendingReason::Incoming { .. }) => {
                    "This file's content is still on the remote and has not been downloaded yet."
                }
                None => "Something in this folder is waiting to sync.",
            },
        ),
        browse::EntrySyncStatus::Excluded => FilesEntrySyncVm::explained(
            FilesSyncStatusVm::Excluded,
            "A pattern in this folder's sync settings excludes it, so keeper will never \
             copy it.",
        ),
        browse::EntrySyncStatus::NotInRepository => FilesEntrySyncVm::explained(
            FilesSyncStatusVm::NotInRepository,
            "This folder is not a repository yet. The first sync sets one up and takes \
             everything in it.",
        ),
        // The engine's own words, verbatim, for the same reason an unreadable
        // directory shows the OS's: a reason someone can act on beats a
        // sentence that only says something went wrong.
        browse::EntrySyncStatus::Unknown => FilesEntrySyncVm::explained(
            FilesSyncStatusVm::Unknown,
            match engine_failure {
                Some(reason) => {
                    format!("keeper could not read this folder's sync state: {reason}")
                }
                None => "keeper could not read this folder's sync state.".to_owned(),
            },
        ),
    }
}

/// Why a directory that should be there is not.
///
/// The profile root and a folder inside it are different sentences: the root
/// being gone is a profile problem with a profile remedy, which
/// [`unavailable_sentence`] already words. A subfolder that vanished is not —
/// re-pointing the profile would be the wrong advice — so it is named by its
/// own relative path, and by the profile's NAME rather than its absolute path,
/// which keeps a home-directory name out of a surface people screenshot.
fn missing_sentence(profile: &SyncProfile, subpath: &str) -> String {
    if subpath.is_empty() {
        return unavailable_sentence(profile);
    }
    format!(
        "{subpath} is no longer in {}. It was moved, renamed or deleted outside keeper.",
        profile.name
    )
}

/// Hand one file inside a synced folder to the system's default handler
/// (Story 43.8, FR-153, AD-65).
///
/// **Why this is not `recording_open_path`.** That command's containment root
/// is the *recordings destination*, deliberately and permanently: it is the one
/// place keeper serves recordings from, and AD-74 says the Files tab must not
/// reach for it to browse folders that are not recordings roots. Pointed at a
/// note in a vault it would refuse, correctly, and a browser whose Open works
/// for one folder in five is worse than one with no Open at all.
///
/// So the root here is the profile's own, and the containment rule is not a
/// second one: it is [`browse::resolve`], the same function the listing uses,
/// which refuses `..` lexically and refuses a symlink out of the tree after
/// canonicalisation.
///
/// Takes a profile id and a profile-relative subpath, never a path: a webview
/// that asked for `/etc/passwd` would have to have persuaded the engine to
/// store a profile pointing there first. This opens; it does not stream, and it
/// has no counterpart that writes.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a subpath that
/// escapes the root, a file that is no longer on disk, an opener failure).
#[tauri::command]
pub async fn sync_open_entry(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<(), IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?;
    let resolved = browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| open_failure(refusal.to_string()))?
        .ok_or_else(|| open_failure(missing_sentence(profile, &subpath)))?;
    tauri_plugin_opener::open_path(&resolved, None::<&str>).map_err(|error| {
        open_failure(format!(
            "could not open {subpath} with the system's default application: {error}"
        ))
    })
}

/// Read one file inside a synced folder as editable text (Story 45.6, FR-179,
/// AD-65).
///
/// **The counterpart of `sync_write_entry`, and deliberately not part of it.**
/// Story 45.3 reversed AD-75 and gave the Files surface a write path; this is
/// the read half, and it is a separate command because reading is a separate
/// capability: a file outside a vault can be listed and viewed but not written
/// (the epic's own rule), so a single read-write command would have to refuse
/// half of itself.
///
/// **Containment is not restated here.** The subpath is one the listing already
/// produced, and [`browse::resolve`] is the same function `sync_browse` and
/// `sync_open_entry` use: it refuses `..` lexically and refuses a symlink out
/// of the tree after canonicalisation. This command composes no path of its
/// own, so there is no second rule to keep in step with the first.
///
/// **Every decision is in `keeper-core`.** Whether the bytes are text, whether
/// the file is too large to edit, how big it is in words a person reads — all
/// of it is [`keeper_core::text_file::open_text_file`], which compiles and is
/// tested on any machine. This crate does not build on Linux, so a threshold
/// or a sentence written here would be one nobody could exercise until macOS
/// (AD-55, AD-56).
///
/// Runs on the blocking pool: a file on a pendrive or a network share can take
/// hundreds of milliseconds to open, and stalling the async runtime would
/// freeze every other profile's poll behind one click.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a subpath that
/// escapes the root, a file that is no longer on disk, an unreadable file).
#[tauri::command]
pub async fn sync_read_text(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<keeper_core::text_file::TextFileVm, IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?;
    let resolved = browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| open_failure(refusal.to_string()))?
        .ok_or_else(|| open_failure(missing_sentence(profile, &subpath)))?;
    let named = subpath.clone();
    tokio::task::spawn_blocking(move || keeper_core::text_file::open_text_file(&resolved))
        .await
        .map_err(|err| open_failure(format!("could not read {named}: {err}")))?
        .map_err(|err| open_failure(format!("could not read {subpath}: {err}")))
}

/// Read one file inside a synced folder as a document (Story 45.8, FR-181,
/// FR-182, AD-65).
///
/// **The third reader beside `sync_read_text` and `sync_browse`, and separate
/// for the same reason they are separate from each other.** A document is not
/// text: it cannot be edited, its bytes never cross this boundary, and what the
/// webview receives is a bounded projection rather than a file. Folding it into
/// `sync_read_text` would have meant a `TextFileVm` with four document-shaped
/// fields that are `None` for every text file, and a viewer deciding which half
/// of one command's answer it was looking at.
///
/// **Containment is not restated here.** The subpath is one the listing already
/// produced, and [`browse::resolve`] is the same function `sync_browse`,
/// `sync_open_entry` and `sync_read_text` use. This command composes no path of
/// its own (AD-65).
///
/// **Every decision is in `keeper-core`.** Which format the bytes are, every
/// cap, every refusal sentence and the whole of the parsing is
/// [`keeper_core::document::open_document`], which compiles and is tested on
/// any machine. This crate does not build on Linux, so a cap written here would
/// be one nobody could exercise until macOS (AD-55, AD-56).
///
/// **This does not serve the PDF's pages.** Those come from Story 45.7's
/// `keeper-file://` protocol, Range-served straight into the webview's own PDF
/// renderer, so a 400-page document costs one element and no marshalling. What
/// this command adds for a PDF is the header facts — version, page count,
/// whether it is encrypted — that the chrome around the embed shows.
///
/// Runs on the blocking pool: inflating a container can take hundreds of
/// milliseconds, and stalling the async runtime would freeze every other
/// profile's poll behind one click.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a subpath that
/// escapes the root, a file that is no longer on disk, an unreadable file). A
/// file that is readable but is not a document keeper knows is NOT a rejection
/// — it is a `DocumentVm` carrying a sentence, because the viewer draws that.
#[tauri::command]
pub async fn sync_read_document(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<keeper_core::document::DocumentVm, IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, &id)?;
    let resolved = browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| open_failure(refusal.to_string()))?
        .ok_or_else(|| open_failure(missing_sentence(profile, &subpath)))?;
    let named = subpath.clone();
    tokio::task::spawn_blocking(move || keeper_core::document::open_document(&resolved))
        .await
        .map_err(|err| open_failure(format!("could not read {named}: {err}")))?
        .map_err(|err| open_failure(format!("could not read {subpath}: {err}")))
}

// ---------------------------------------------------------------------------
// Export (Story 45.21, FR-199)
// ---------------------------------------------------------------------------

/// An export refusal, as the frontend receives it and as the log records it.
///
/// One function for both export commands — `notes_ipc::notes_export` calls this
/// one rather than wording its own, because a refusal the user reads must not
/// depend on which surface they pressed the button from.
///
/// `warn!` rather than `info!`, on the same reasoning as [`write_refused`]:
/// `GatedMakeWriter` only writes `INFO` to the file when debug mode is on, and
/// a refusal is exactly the thing somebody asks about an hour later (DW-162).
pub(crate) fn export_refused(refusal: &ExportRefusal) -> IpcError {
    tracing::warn!(%refusal, "export: refused");
    IpcError {
        code: IpcErrorCode::Internal,
        message: refusal.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// Copy one file out of a synced folder to a location the user picked
/// (Story 45.21, FR-199, AD-65).
///
/// **Not a write command, and deliberately not beside the four below.** Export
/// reads inside the profile and writes outside it, so it needs no vault and
/// refuses nothing for being outside one: a PDF in a synced folder that keeper
/// will not edit is still a PDF the owner can take a copy of. `writable_profile`
/// is not used here for exactly that reason.
///
/// **The destination is the one path the webview supplies**, and it is the one
/// path AD-65 permits it to: the user picked it from the OS folder chooser, it
/// is not composed from anything keeper holds, and nothing under it is read.
/// The source is still an id plus a relative path, re-resolved through
/// `browse::resolve` like every other reader on this surface.
///
/// Every decision — is the destination there, is the name taken, does a partial
/// copy get cleaned up — is [`keeper_sync::export`], which compiles and is
/// tested on any machine. This crate does not build on Linux (AD-55, AD-56).
///
/// Runs on the blocking pool: copying a 2 GB video off a pendrive is not
/// something to do on the async runtime.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a subpath that
/// escapes the root, a file that is gone, a folder, a destination that is
/// missing / is a file / is inside the profile / already holds the name, or a
/// copy the disk refused).
#[tauri::command]
pub async fn sync_export_entry(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
    destination: String,
) -> Result<ExportReceiptVm, IpcError> {
    let engine = engine_of(&state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    // The root is cloned out immediately: nothing about the profile is needed
    // after this point, and a borrow held across the await below would be a
    // borrow of a local `Vec` in a future that has to be `Send`.
    let root = find_profile(&profiles, &id)?.local_path.clone();
    let named = subpath.clone();
    let target = std::path::PathBuf::from(&destination);

    let done = tokio::task::spawn_blocking(move || export::export_entry(&root, &subpath, &target))
        .await
        .map_err(|err| open_failure(format!("could not export {named}: {err}")))?
        .map_err(|refusal| export_refused(&refusal))?;

    tracing::info!(rel = %named, "files: exported a file out of keeper");
    Ok(ExportReceiptVm::file(
        &destination,
        done.path.display().to_string(),
        export::file_name_of(&named),
    ))
}

// ---------------------------------------------------------------------------
// The Files surface writes (Story 45.3, FR-175, FR-176, AD-89)
// ---------------------------------------------------------------------------
//
// AD-75 said the Files surface never writes. AD-89 retired it, deliberately and
// by the owner, and the four commands below are the whole of what replaced it.
// The reasoning lives in `keeper_sync::files_write`'s module doc; what matters
// at this call site is the shape it imposes:
//
//   * every byte goes through `notes_vault::write_vault_file` + `mark_dirty`,
//     the same path notes and Story 44.16's CSV editor use — never a second
//     writer, never a reach into the sync engine;
//   * every removal goes through `notes_vault::trash_note`, which the
//     reconciler already understands, so a deletion is announced rather than
//     discovered on the next scan;
//   * every decision — is this inside a vault, is it a folder, is the name a
//     name, is it already taken — is made in `keeper_sync::files_write`, which
//     compiles and is tested on any machine. This crate does not build on Linux
//     (AD-55, AD-56), so a rule written here would be a rule guarding a write
//     that nobody could exercise until macOS.

/// The vault this profile holds and the write scope over it.
///
/// **From the LIVE vault, never from `profile.notes`.** The listing's
/// `writable` flag and the write command's answer have to be the same question
/// asked twice, and a profile configured with a vault the registry has no slot
/// for — unflagged, root moved, still starting — is exactly where those two
/// would diverge. A pane told "writable" by configuration and refused by the
/// registry is a pane offering an action that will fail, which is the one thing
/// this story's own rule forbids.
fn vault_and_scope(profile: &SyncProfile) -> (Option<crate::notes_vault::Vault>, WriteScope<'_>) {
    let vault = crate::notes_vault::vault(&profile.id);
    let scope = WriteScope::new(
        &profile.name,
        vault.as_ref().map(|vault| vault.config.subfolder.as_str()),
    )
    // The workspace fence (Phase 7, AD-113): with the sessions zone named,
    // the scope refuses every write under a session's `workspace/` — scratch
    // keeper reads and never touches. From the stored profile rather than the
    // registry, because the fence must hold even while the sessions registry
    // is still starting: an unregistered zone widens nothing.
    .with_sessions(
        profile
            .sessions
            .as_ref()
            .map(|sessions| sessions.subfolder.as_str()),
    );
    (vault, scope)
}

/// A write refusal, as the frontend receives it — and as the log records it.
///
/// `warn!` rather than `info!`, on 44.16's reasoning: `GatedMakeWriter` only
/// writes `INFO` to the file when debug mode is on and lets `WARN` and above
/// through always. A refusal is the thing the user asks about later, so it has
/// to already be on disk (DW-162).
fn write_refused(refusal: &WriteRefusal) -> IpcError {
    tracing::warn!(%refusal, "files: write refused");
    IpcError {
        code: IpcErrorCode::Internal,
        message: refusal.to_string(),
        account_id: None,
        retriable: false,
    }
}

/// The profile and its live vault, or the refusal that ends the call.
///
/// **The CREATE path's opener, and after Story 46.14 only that.** Creating is
/// still vault-only (AD-102 widened editing and deleting, not creating), so
/// this is the one command that may still decline a whole profile for holding
/// no vault. Everything that changes an existing file starts at
/// [`routable_profile`] instead.
fn writable_profile(
    state: &tauri::State<'_, AppState>,
    id: &str,
) -> Result<(SyncProfile, crate::notes_vault::Vault), IpcError> {
    let engine = engine_of(state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    let profile = find_profile(&profiles, id)?.clone();
    // The refusal is `WriteRefusal`'s own sentence rather than one worded here,
    // so the reason a person reads when a command declines is the same reason
    // the listing already showed them where the control should have been.
    let Some(vault) = crate::notes_vault::vault(&profile.id) else {
        return Err(write_refused(&WriteRefusal::NoVault {
            profile_name: profile.name.clone(),
        }));
    };
    Ok((profile, vault))
}

/// The profile a command may change a file in, whether or not it holds a vault
/// (Story 46.14, AD-102).
///
/// **The vault question no longer ends the call, and that is the whole
/// shell-side change.** [`writable_profile`] refused a whole profile for
/// holding no reachable vault before `WriteScope` was ever consulted, which is
/// what made the owner's `AGENTS.md` unreachable: it is inside a sync profile,
/// outside the vault. Editing and deleting now start here and let
/// [`WriteScope::route`] decide the fork, in `keeper-sync`, where it is
/// asserted on every machine.
///
/// Deliberately does NOT return a vault. The vault a command writes through
/// must be the same one its scope was built from, and the only way to make
/// that structural is for both to come out of [`vault_and_scope`]'s single
/// registry lookup — a second lookup here is exactly the "writable by config,
/// refused by the registry" divergence that function exists to prevent.
fn routable_profile(state: &tauri::State<'_, AppState>, id: &str) -> Result<SyncProfile, IpcError> {
    let engine = engine_of(state)?;
    let profiles = engine.list_profiles().map_err(|e| sync_ipc_error(&e))?;
    Ok(find_profile(&profiles, id)?.clone())
}

// ---------------------------------------------------------------------------
// What the sessions tree borrows (Phase 7, FR-254, AD-117)
// ---------------------------------------------------------------------------
//
// A sessions root IS a sync profile (AD-107), so the session tree needs the
// same three facts a listing needs: the profile, its write scope, and one
// `pending` answer. These three re-export the machinery above rather than
// letting `sessions_ipc` build its own — the whole point of the tree's sync
// mark is that it is not a second opinion, and it stops being one the moment
// the sessions surface starts asking git its own questions.
//
// Nothing new is computed here. Each is one line over a private helper,
// visible to the crate and to nobody else.

/// The profile behind one sessions root.
pub(crate) fn sessions_profile(
    state: &tauri::State<'_, AppState>,
    root_id: &str,
) -> Result<SyncProfile, IpcError> {
    routable_profile(state, root_id)
}

/// The live vault and the write scope over it — including the AD-113 fence,
/// which is what the tree renders its read-only lock from.
pub(crate) fn sessions_scope(
    profile: &SyncProfile,
) -> (Option<crate::notes_vault::Vault>, WriteScope<'_>) {
    vault_and_scope(profile)
}

/// One `Engine::pending` answer for the whole tree, the engine's own words on
/// failure.
pub(crate) async fn sessions_pending(
    state: &tauri::State<'_, AppState>,
    root_id: &str,
) -> Result<Vec<keeper_sync::engine::PendingFile>, String> {
    let engine = engine_of(state).map_err(|error| error.message)?;
    engine
        .pending(root_id)
        .await
        .map_err(|error| error.to_string())
}

/// The five sentences, verbatim — [`sync_mark`], for the sessions tree.
pub(crate) fn sessions_sync_mark(
    status: &browse::EntrySyncStatus,
    engine_failure: Option<&str>,
) -> FilesEntrySyncVm {
    sync_mark(status, engine_failure)
}

/// Save one file inside a synced folder (Story 45.3, FR-175, AD-89, AD-65;
/// Story 46.14, AD-102).
///
/// **Two writers, and `WriteScope::route` picks.** Which one is not decided
/// here — it is decided in `keeper-sync`, where it is asserted on Linux, and it
/// arrives as a `WriteRoute` whose vault arm already carries the vault. This
/// command's job is to spend the verdict, not to reach it.
///
/// *In the vault:* `write_vault_file` is the same temp-and-rename `write_note`
/// uses, under the `.keeper.<ulid>.tmp` name that is already a tier-0 sync
/// exclusion, so a `kill -9` between write and rename leaves no torn file.
/// `mark_dirty` is the announcement `import_attachment` already makes: the
/// commit cadence runs and the change is committed and synced. `touch` is
/// included, where Story 44.16 deliberately left it out — 44.16's target is an
/// embedded `.csv` the notes walk never collects, whereas this surface can save
/// a `.md` *inside the vault*, which is a note, so the index has to be told.
///
/// *Outside every vault:* `write_unmanaged`, a plain atomic write with no
/// `mark_dirty` and no `touch` — because there is no vault to mark and no index
/// this file belongs in. Neither call is reachable from that arm: the route
/// hands over no vault, and `write_unmanaged`'s signature has nowhere to put
/// one. The surface said so before the first keystroke
/// (`FilesWriteVm::caveat`); an edit that quietly does less than the vault path
/// does would be strictly worse than the refusal it replaces.
///
/// **A path that is not on disk is refused rather than created**, by either
/// writer. Saving is not creating: `sync_create_entry` is, it is still
/// vault-only, and it is the one with the collision rule. A stale editor whose
/// file was deleted elsewhere must not put it back.
///
/// Content is written as exact bytes — no trailing-newline normalisation, no
/// re-encoding — for the reason 44.16's parser records spans instead of
/// re-serialising: a file the user did not change must not change.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a folder, a path
/// that escapes the profile root, a path that is gone, a vault that is
/// configured but not yet live, a disk failure).
#[tauri::command]
pub async fn sync_write_entry(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
    content: String,
) -> Result<(), IpcError> {
    let profile = routable_profile(&state, &id)?;
    let (vault, scope) = vault_and_scope(&profile);
    let route = scope
        .route(vault, &profile.local_path, &subpath)
        .map_err(|refusal| write_refused(&refusal))?;

    // On the blocking pool for the same reason the listing is: a folder on a
    // pendrive or a network share can take hundreds of milliseconds to write,
    // and stalling the async runtime would freeze every other profile's poll
    // behind one save.
    let written = {
        let route = route.clone();
        // `subpath` is not captured by the blocking closure — the route
        // already holds every path either writer needs — so it is still here
        // to name the file if the task itself panics.
        tokio::task::spawn_blocking(move || match &route {
            WriteRoute::Vault { vault, path } => {
                crate::notes_vault::write_vault_file(vault, path.as_str(), &content)
                    .map_err(WriteOutcome::Vault)
            }
            WriteRoute::Unmanaged(target) => {
                files_write::write_unmanaged(target, &content).map_err(WriteOutcome::Plain)
            }
        })
        .await
        .map_err(|err| open_failure(format!("could not save {subpath}: {err}")))?
    };
    written.map_err(|outcome| match outcome {
        WriteOutcome::Vault(error) => notes_write_error(error),
        WriteOutcome::Plain(refusal) => write_refused(&refusal),
    })?;

    match route {
        WriteRoute::Vault { vault, path } => {
            crate::notes_vault::touch(&vault.id, vec![path.as_str().to_owned()]);
            crate::notes_vault::mark_dirty(&vault.id);
            tracing::info!(rel = %path.as_str(), "files: wrote a vault file from the Files surface");
        }
        // Logged at `info!` and not `debug!` for DW-162's reason, and worth a
        // line of its own: "keeper wrote this and told nothing about it" is
        // exactly what a person asks about later.
        WriteRoute::Unmanaged(target) => tracing::info!(
            rel = %target.profile_relative(),
            "files: wrote a file no vault manages — no mark_dirty, no touch (AD-102)"
        ),
    }
    Ok(())
}

/// Which writer failed, so the sentence a person reads comes from the right
/// vocabulary.
///
/// The two errors are genuinely different types — `NotesError` names a
/// vault-relative path and `WriteRefusal` names the one the surface shows —
/// and flattening them to a `String` inside the blocking task would throw away
/// the `warn!` each of them already logs on the way out.
enum WriteOutcome {
    Vault(keeper_core::notes::NotesError),
    Plain(WriteRefusal),
}

// ---------------------------------------------------------------------------
// A file's own properties (Story 50.4, FR-283, AD-120)
// ---------------------------------------------------------------------------
//
// Here rather than in `notes_ipc` because the address decides ownership: this
// pair is `(profile id, profile-relative subpath)`, which is what every command
// in this file speaks and what no command in `notes_ipc` does — a note is a
// vault id and a note id. The file these serve most often has no vault behind
// it at all (a session's `README.md` under a sessions zone, AD-107), and
// reaching it through `notes_ipc` would mean inventing a vault for a file that
// is not in one.
//
// What they do NOT own is a second frontmatter parser or a second frontmatter
// writer. Both are `keeper_core::file_properties`, over
// `keeper_core::notes::frontmatter` — the same span-recording scanner the notes
// side splices through — so the two surfaces cannot come to disagree about
// where one file's block ends.

/// The bytes a properties edit may be applied to, or Rust's own reason it may
/// not be.
///
/// One helper for both commands, so the read cannot offer a panel over a file
/// the write would then decline. A prefix is refused rather than edited: with
/// `oversize` the string is the first megabyte and writing it back would delete
/// the rest of the file (`text_file`'s module note).
fn editable_source(
    file: keeper_core::text_file::TextFileVm,
    subpath: &str,
) -> Result<String, IpcError> {
    match file.text {
        Some(text) if !file.oversize => Ok(text),
        _ => Err(open_failure(file.detail.unwrap_or_else(|| {
            format!("keeper cannot read {subpath} as text, so it has no properties to show")
        }))),
    }
}

/// Read one file's frontmatter block (Story 50.4, FR-283).
///
/// **Routed, not merely resolved, and with the same call the write makes.**
/// `WriteScope::route` is what refuses a `workspace/` path (AD-113), a
/// directory, and a subpath that leaves the profile — in `keeper-sync`, where
/// it is asserted on Linux. Asking it here as well is what makes matrix row 8
/// true: a file whose properties keeper would refuse to write gets no panel at
/// all, rather than a panel that refuses on the first keystroke. Story 45.3's
/// rule, applied to a second pair of commands.
///
/// It routes AND resolves because those are two questions: `route` answers
/// "may keeper write here", and `browse::resolve` — the same call
/// `sync_read_text` makes — answers "where is it". The route's own path is
/// deliberately opaque (`UnmanagedPath` keeps its absolute form private so a
/// `PathBuf` cannot reach a writer from anywhere else), so the second `stat` is
/// the price of that, and it is one `stat` per panel open.
///
/// Returns the block **verbatim**, `""` for a file that has none — the same
/// shape `NoteBodyBatch.frontmatter` and `NoteWriteVm.frontmatter` carry, so
/// the properties panel consumes one thing from two addresses and does not
/// fork. A view-model wrapping one string would be a `gen/` type whose only
/// field is the string.
///
/// Runs on the blocking pool for `sync_read_text`'s reason: a file on a
/// pendrive can take hundreds of milliseconds to open.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a path that
/// escapes the root or sits in `workspace/`, a directory, a file that is gone,
/// a file that is not editable text).
#[tauri::command]
pub async fn sync_read_frontmatter(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
) -> Result<String, IpcError> {
    let profile = routable_profile(&state, &id)?;
    let (vault, scope) = vault_and_scope(&profile);
    scope
        .route(vault, &profile.local_path, &subpath)
        .map_err(|refusal| write_refused(&refusal))?;
    let resolved = browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| open_failure(refusal.to_string()))?
        .ok_or_else(|| open_failure(missing_sentence(&profile, &subpath)))?;

    let named = subpath.clone();
    tokio::task::spawn_blocking(move || {
        let file = keeper_core::text_file::open_text_file(&resolved)
            .map_err(|err| open_failure(format!("could not read {subpath}: {err}")))?;
        let source = editable_source(file, &subpath)?;
        Ok(keeper_core::file_properties::block_of(&source).to_owned())
    })
    .await
    .map_err(|err| open_failure(format!("could not read {named}: {err}")))?
}

/// Write one file's frontmatter block, and nothing else in the file
/// (Story 50.4, FR-283, FR-233, AD-120).
///
/// **`expect` is the block the surface read, and the write refuses if the block
/// on disk is no longer it.** The guard is the block rather than the file
/// because the body written is the body just read: a person or an agent typing
/// in the body loses nothing and is refused nothing, and the only edit this
/// command could drop is a concurrent edit to the properties themselves. The
/// reasoning, and why that is a better precondition than `GuardedWrite`'s
/// length or the notes side's whole-file fingerprint, is in
/// `keeper_core::file_properties` — with the byte tests, which this crate could
/// not carry (AD-55, AD-56).
///
/// A refusal rather than a conflict copy. `notes_save` writes one because it is
/// saving a document somebody typed; a property is a field they can set again,
/// and a second file on disk would be a worse answer than a sentence.
///
/// **Nothing is stamped.** `notes_save` puts `updated` into the block on every
/// write and `notes_create` puts an `id` in; neither happens here, because this
/// is a file keeper did not author and the sessions contract is explicit about
/// it (`docs/sessions.md`). What lands is exactly the block the person edited.
///
/// The two writers, and `WriteScope::route` picking between them, are
/// `sync_write_entry`'s — including `touch` and `mark_dirty` for a file that
/// turns out to be inside a vault, and neither for one that is not (AD-102).
///
/// Returns the block as it now stands on disk, for the panel to render.
///
/// Rejects with: `unsupported`, `internal` (no such profile, a path that
/// escapes the root or sits in `workspace/`, a directory, a file that is gone,
/// a file that is not editable text, a block that changed underneath, a block
/// that is not well formed, a disk failure).
#[tauri::command]
pub async fn sync_write_frontmatter(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
    expect: String,
    frontmatter: String,
) -> Result<String, IpcError> {
    // Scoped to this function: nothing else in this module emits, and the
    // module deliberately spells every other `tauri::` name in full.
    use tauri::Emitter as _;

    let profile = routable_profile(&state, &id)?;
    let (vault, scope) = vault_and_scope(&profile);
    let route = scope
        .route(vault, &profile.local_path, &subpath)
        .map_err(|refusal| write_refused(&refusal))?;
    let resolved = browse::resolve(&profile.local_path, &subpath)
        .map_err(|refusal| open_failure(refusal.to_string()))?
        .ok_or_else(|| open_failure(missing_sentence(&profile, &subpath)))?;

    let named = subpath.clone();
    let landed = {
        let route = route.clone();
        let subpath = subpath.clone();
        // Read, splice and write in ONE blocking task. Reading on the async
        // side and writing here would open a window between the guard and the
        // write that is wider than the one the guard exists to close.
        tokio::task::spawn_blocking(move || {
            let file = keeper_core::text_file::open_text_file(&resolved)
                .map_err(|err| open_failure(format!("could not read {subpath}: {err}")))?;
            let source = editable_source(file, &subpath)?;
            let next = keeper_core::file_properties::replace_block(
                &source,
                &expect,
                &frontmatter,
                &subpath,
            )
            .map_err(|refusal| {
                // `warn!` on `write_refused`'s reasoning (DW-162): a refusal is
                // exactly what somebody asks about an hour later.
                tracing::warn!(%refusal, "files: a properties write was refused");
                IpcError {
                    code: IpcErrorCode::Internal,
                    message: refusal.to_string(),
                    account_id: None,
                    retriable: false,
                }
            })?;
            match &route {
                WriteRoute::Vault { vault, path } => {
                    crate::notes_vault::write_vault_file(vault, path.as_str(), &next)
                        .map_err(notes_write_error)?;
                }
                WriteRoute::Unmanaged(target) => {
                    files_write::write_unmanaged(target, &next)
                        .map_err(|refusal| write_refused(&refusal))?;
                }
            }
            Ok::<String, IpcError>(keeper_core::file_properties::block_of(&next).to_owned())
        })
        .await
        .map_err(|err| open_failure(format!("could not save {named}: {err}")))??
    };

    match route {
        WriteRoute::Vault { vault, path } => {
            crate::notes_vault::touch(&vault.id, vec![path.as_str().to_owned()]);
            crate::notes_vault::mark_dirty(&vault.id);
            tracing::info!(rel = %path.as_str(), "files: wrote a vault file's properties");
        }
        WriteRoute::Unmanaged(target) => tracing::info!(
            rel = %target.profile_relative(),
            "files: wrote the properties of a file no vault manages (AD-102)"
        ),
    }

    // The re-read, and the narrowest hook there is: the sessions surfaces
    // already listen for this event and re-issue both space reads on it
    // (`session-detail.tsx`), so a file that just became `tag:ref` appears in
    // References without a manual refresh and without one line of new frontend
    // plumbing. The zone watcher would have said the same thing a debounce
    // later; saying it now is what turns that race into a guarantee, which is
    // the reason the detail surface already keeps an explicit re-read beside
    // the event.
    //
    // Only for a profile that holds a sessions zone. A tag written in an
    // ordinary synced folder changes no space, and an event nobody is listening
    // for is still a re-read for every open board.
    if profile.sessions.is_some() {
        let _ = app.emit(crate::sessions_root::SESSIONS_CHANGED_EVENT, id);
    }
    Ok(landed)
}

/// Word what deleting this selection would do, before it is done (Story 45.3,
/// FR-175, UX-DR66).
///
/// **A separate call, and that is what makes the confirmation honest.** The
/// plan is built by the same code the delete runs — the same scope, the same
/// resolution, the same sync answer — so the dialog cannot promise something
/// the command will then refuse, and a file that vanished between the click and
/// the confirmation is named as a refusal rather than silently dropped.
///
/// The sentences are composed by [`FilesDeletePlanVm::compose`], in
/// `keeper-core`, which is pure and therefore asserted on every machine. This
/// command's whole job is gathering the facts it needs per path: may keeper
/// delete it, does it sync, and — since Story 46.14 — which trash it is bound
/// for. That third fact is per file rather than per call because one drag over
/// a vault and the folder beside it selects both.
///
/// Rejects with: `unsupported`, `internal` (no such profile).
#[tauri::command]
pub async fn sync_delete_plan(
    state: tauri::State<'_, AppState>,
    id: String,
    subpaths: Vec<String>,
) -> Result<FilesDeletePlanVm, IpcError> {
    let profile = routable_profile(&state, &id)?;
    let (vault, scope) = vault_and_scope(&profile);
    let excludes = ExcludeSet::new(&profile.excludes).map_err(|e| sync_ipc_error(&e))?;
    let engine = engine_of(&state)?;
    // Once for the whole selection, exactly as the listing asks once for a
    // whole directory.
    let (pending, unavailable) = match engine.pending(&id).await {
        Ok(files) => (browse::PendingView::from_pending(files), None),
        Err(error) => (browse::PendingView::Unavailable, Some(error.to_string())),
    };

    let mut files = Vec::new();
    let mut refusals = Vec::new();
    for subpath in subpaths {
        match scope.route(vault.clone(), &profile.local_path, &subpath) {
            Ok(route) => {
                // `false` and not a re-`stat`: `route` refuses every directory,
                // so anything that got this far is a file. The old
                // `DeleteTarget.is_dir` could only ever be `false` here too —
                // it was a fact carried past the check that made it constant.
                let status =
                    browse::status_of(&profile.local_path, &subpath, false, &excludes, &pending);
                files.push((
                    subpath,
                    sync_mark(&status, unavailable.as_deref()).status,
                    destination_of(&route),
                ));
            }
            Err(refusal) => refusals.push(FilesDeleteRefusalVm {
                relative_path: subpath,
                reason: refusal.to_string(),
            }),
        }
    }
    Ok(FilesDeletePlanVm::compose(&profile.name, files, refusals))
}

/// Where one routed path's bytes are about to go, for the confirmation's
/// recovery sentence (Story 46.14, AD-102).
///
/// A projection of the route rather than a second reading of the scope: the
/// dialog and the command have to name the same trash, and the only way to
/// guarantee that is for both to read the same verdict.
fn destination_of<V>(route: &WriteRoute<V>) -> FilesDeleteDestinationVm {
    match route {
        WriteRoute::Vault { .. } => FilesDeleteDestinationVm::VaultTrash,
        WriteRoute::Unmanaged(_) => FilesDeleteDestinationVm::SystemTrash,
    }
}

/// Move a selection of files into a trash — the vault's or the operating
/// system's (Story 45.3, FR-175, AD-89, NFR-30; Story 46.14, AD-102).
///
/// **Never an `unlink`, whichever trash it is**, and that is the promise AD-102
/// relocated rather than weakened.
///
/// *In the vault:* `trash_note` is the removal path the reconciler already
/// understands. It renames the file into `<vault>/.keeper/trash/<ulid>/<rel>` —
/// `.keeper` is a tier-0 sync exclusion, so git sees a deletion — then
/// `touch`es the path so the index drops the note, and `mark_dirty`s the vault
/// so the commit cadence carries the removal. The bytes stay recoverable
/// locally *and* from history, and the commit that deletes the file is preceded
/// by one that still holds it.
///
/// *Outside every vault:* `trash_unmanaged`, which is `NSFileManager
/// trashItem` on macOS and the freedesktop.org home trash elsewhere. There is
/// no vault trash to reach and no note history to record in, and the
/// confirmation said exactly that before the click
/// (`FilesDeletePlanVm::recovery`).
///
/// **Every path is re-checked here, not trusted from the plan.** The plan is
/// advice a person read; this is the authority. A path that became a folder, a
/// path that moved into or out of the vault, a path that was already deleted —
/// each answers for itself, and the receipt reports the split rather than
/// failing the batch. Failing the whole call would leave four files trashed and
/// an error on screen saying nothing happened.
///
/// Rejects with: `unsupported`, `internal` (no such profile).
#[tauri::command]
pub async fn sync_delete_entries(
    state: tauri::State<'_, AppState>,
    id: String,
    subpaths: Vec<String>,
) -> Result<FilesDeleteReceiptVm, IpcError> {
    let profile = routable_profile(&state, &id)?;

    let (outcome, dirty) = {
        let profile = profile.clone();
        tokio::task::spawn_blocking(move || {
            // One registry lookup for the vault AND the scope, so the vault a
            // path is trashed into and the vault the scope measured it against
            // cannot be two different vaults.
            let (vault, scope) = vault_and_scope(&profile);
            // Resolved once for the batch and never per file: `os_trash` reads
            // the environment, and on a machine with no home directory it is
            // the same refusal every time.
            let trash = files_write::os_trash();
            let stamp = files_write::local_now_ms();
            let mut deleted = Vec::new();
            let mut refusals = Vec::new();
            let mut dirty: Option<String> = None;
            for subpath in subpaths {
                let outcome = scope
                    .route(vault.clone(), &profile.local_path, &subpath)
                    .and_then(|route| match route {
                        WriteRoute::Vault { vault, path } => {
                            crate::notes_vault::trash_note(&vault, path.as_str())
                                .map(|grave| (grave, Some(vault.id.clone())))
                                .map_err(|error| WriteRefusal::DeleteFailed {
                                    // The path the SURFACE shows, not the
                                    // vault-relative one: the sentence lands
                                    // beside the row the person selected, and
                                    // naming it differently there would read as
                                    // a different file.
                                    relative_path: subpath.clone(),
                                    reason: error.to_string(),
                                })
                        }
                        WriteRoute::Unmanaged(target) => match &trash {
                            // No vault to mark: there is none to reach, and
                            // marking one anyway would ask a reconciler to
                            // reconcile a path it has never indexed.
                            Ok(trash) => files_write::trash_unmanaged(&target, trash, stamp)
                                .map(|grave| (grave, None)),
                            // A machine with no trash keeps its file. The
                            // alternative is the `unlink` NFR-30 forbids.
                            Err(refusal) => Err(refusal.clone()),
                        },
                    });
                match outcome {
                    Ok((grave, marked)) => {
                        tracing::info!(
                            %subpath,
                            grave = %grave.display(),
                            "files: moved a file to the trash"
                        );
                        dirty = dirty.or(marked);
                        deleted.push(subpath);
                    }
                    Err(refusal) => {
                        tracing::warn!(%subpath, %refusal, "files: delete refused");
                        refusals.push(FilesDeleteRefusalVm {
                            relative_path: subpath,
                            reason: refusal.to_string(),
                        });
                    }
                }
            }
            (FilesDeleteReceiptVm { deleted, refusals }, dirty)
        })
        .await
        .map_err(|err| open_failure(format!("could not delete: {err}")))?
    };

    // Only a removal that actually happened, and only a vault one, moves a
    // vault's cadence.
    if let Some(vault_id) = dirty {
        crate::notes_vault::mark_dirty(&vault_id);
    } else if outcome.deleted.is_empty() {
        // DW-162: a path that declines to act says so where a person can find
        // it later, and `debug!` never reaches the packaged app's log.
        tracing::info!(profile = %profile.name, "files: delete removed nothing");
    }
    Ok(outcome)
}

/// Create an empty text file inside a synced folder's notes vault (Story 45.3,
/// FR-176, AD-89).
///
/// Takes the directory as a profile-relative subpath the listing produced, and
/// the name as its own argument — never a joined path (AD-65). Rust joins them,
/// once, in `WriteScope::create`, which is also where the name is checked.
///
/// **The collision refusal is the point.** `write_vault_file` renames over its
/// target, so a create that did not check would replace an existing file with
/// an empty one. The check is case-insensitive because APFS and NTFS are: an
/// exact-match check passes on the Linux box this is written on and destroys a
/// file on the Mac it ships to. A directory that cannot be read is a refusal,
/// never a cleared check — the shape of the epic-44 defect where `notes_create`
/// could overwrite a note through an unreadable directory.
///
/// Returns the new file's profile-relative path, so the surface can re-read the
/// folder and put the cursor on the row it just made without composing a path.
///
/// Rejects with: `unsupported`, `internal` (no such profile, no vault, a
/// directory outside the vault, a name that is not a name, a name already
/// taken, an unreadable directory, a disk failure).
#[tauri::command]
pub async fn sync_create_entry(
    state: tauri::State<'_, AppState>,
    id: String,
    subpath: String,
    name: String,
) -> Result<String, IpcError> {
    let (profile, vault) = writable_profile(&state, &id)?;
    let (_, scope) = vault_and_scope(&profile);
    let target = scope
        .create(&subpath, &name)
        .map_err(|refusal| write_refused(&refusal))?;

    let created = {
        let root = profile.local_path.clone();
        let vault = vault.clone();
        let target = target.clone();
        let wanted = name.clone();
        tokio::task::spawn_blocking(move || {
            let directory = files_write::resolve_existing(&root, &subpath)?;
            if files_write::collides(&directory, &wanted)? {
                return Err(WriteRefusal::NameTaken { name: wanted });
            }
            // Empty, and empty on purpose: a new text file with keeper's
            // boilerplate in it is a file the user has to delete something out
            // of before typing.
            crate::notes_vault::write_vault_file(&vault, &target.vault_relative, "").map_err(
                |error| WriteRefusal::WriteFailed {
                    relative_path: target.profile_relative.clone(),
                    reason: error.to_string(),
                },
            )?;
            Ok(target)
        })
        .await
        .map_err(|err| open_failure(format!("could not create {name}: {err}")))?
    };
    let target = created.map_err(|refusal| write_refused(&refusal))?;

    crate::notes_vault::touch(&vault.id, vec![target.vault_relative.clone()]);
    crate::notes_vault::mark_dirty(&vault.id);
    tracing::info!(
        rel = %target.profile_relative,
        "files: created a file from the Files surface"
    );
    Ok(target.profile_relative)
}

/// A `NotesError` from the one writer, as the frontend receives it.
///
/// Its `Display` already names the vault-relative path and the OS's own words,
/// which is what a person needs; wrapping it in a second sentence would bury
/// the only actionable part.
fn notes_write_error(error: keeper_core::notes::NotesError) -> IpcError {
    tracing::warn!(%error, "files: a vault write failed");
    IpcError {
        code: IpcErrorCode::Internal,
        message: error.to_string(),
        account_id: None,
        retriable: false,
    }
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
            recordings: None,
            recordings_subfolder: None,
            sessions: None,
            sessions_subfolder: None,
        }
    }

    /// Every serialized field of `SyncProfile`, split by whether a request can
    /// express it. These are the exact keys `db::upsert_profile` writes into the
    /// row, which is why the split is stated over the JSON rather than over the
    /// struct: the bug is a lost KEY, and serde is what decides what a key is.
    ///
    /// A field the request has a slot for.
    const EXPRESSED: [&str; 19] = [
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
        // Moved out of PRESERVED by Story 41.7. It was preserved because the app
        // had no control for it and `parse_req` must not touch what no form
        // shows; it is expressed now because the Sync form has the switch, and
        // that switch is the whole of what makes a folder a recording
        // destination.
        "recordings",
        // Expressed from birth (FR-222): the Sync form shipped its switch in the
        // same change that added the field, so it never had a PRESERVED phase.
        "sessions",
    ];

    /// A field no request can express, which `parse_req` must therefore never
    /// touch. `enabled` moves only through pause/resume, `volumeId` is minted by
    /// the engine on first sight of the media, `id` names the row, and the two
    /// LFS knobs (`lfsNever`, `lfsPruneLocal`) are configured through
    /// `keeper-syncd`'s profile file with no slot in the app's form — `parse_req`
    /// keeps them because it starts from `prior.clone()`, not because it copies
    /// them by name. `lfsPruneLocal` needs no slot on purpose: releasing the
    /// redundant object copy is what a profile does by default, and the opt-out
    /// is for a machine that is configured, not clicked.
    /// `regenerable` joins them for the same reason and one of its own: which
    /// paths a repository generates is a fact about that repository, so it is
    /// configured where the repository can say it — `.keeper/keeper.toml`, which
    /// travels with the folder — rather than clicked per machine. A save from a
    /// form that has never shown the list must not be able to empty it.
    const PRESERVED: [&str; 6] = [
        "id",
        "volumeId",
        "enabled",
        "lfsNever",
        "lfsPruneLocal",
        "regenerable",
    ];

    fn json_fields(profile: &SyncProfile) -> serde_json::Map<String, serde_json::Value> {
        match serde_json::to_value(profile).expect("a profile serializes") {
            serde_json::Value::Object(map) => map,
            other => panic!("a profile is a JSON object, got {other}"),
        }
    }

    /// The Files pane used to wait for `Engine::pending` before it would name a
    /// single entry — a whole-worktree `git status` plus an untracked
    /// expansion. On a folder of tens of thousands of files on a busy drive
    /// that is minutes of an empty pane, and the pane's own refresh asked for
    /// it once per open directory.
    #[test]
    fn a_fresh_answer_is_served_and_a_stale_one_starts_a_walk() {
        let view = browse::PendingView::Unavailable;
        let fresh = MarkSlot {
            answered: Some((Instant::now(), view.clone())),
            walking: false,
        };
        assert_eq!(fresh.plan(Duration::from_secs(3)), MarkPlan::Serve(view));

        // The same answer, now older than the window it is good for.
        assert_eq!(fresh.plan(Duration::from_nanos(1)), MarkPlan::Walk);

        assert_eq!(
            MarkSlot::default().plan(Duration::from_secs(3)),
            MarkPlan::Walk
        );
    }

    /// One walk at a time per folder: a second reads the same tree off the same
    /// disk for the same answer, which is exactly what made ten open
    /// directories ten whole-repository walks at once.
    #[test]
    fn a_walk_in_progress_serves_what_there_is_rather_than_starting_another() {
        let stale = browse::PendingView::Known(Default::default());
        let walking_with_answer = MarkSlot {
            answered: Some((Instant::now(), stale.clone())),
            walking: true,
        };
        // Stale but usable: a mark a few seconds old beats no mark at all.
        assert_eq!(
            walking_with_answer.plan(Duration::from_nanos(1)),
            MarkPlan::ServeWhileWalking(Some(stale))
        );

        let walking_first_time = MarkSlot {
            answered: None,
            walking: true,
        };
        assert_eq!(
            walking_first_time.plan(Duration::from_secs(3)),
            MarkPlan::ServeWhileWalking(None),
            "and with nothing to serve it says so, rather than calling every entry clean"
        );
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
        prior.regenerable = vec!["index.md".into()];
        // The opt-out, because a fresh profile now releases the redundant copy.
        prior.lfs_prune_local = false;
        // Story 41.1's block, set to something a fresh profile never has. It was
        // a PRESERVED field until Story 41.7 gave the form a switch for it; the
        // distinctive value stays, because it is now what makes the EXPRESSED
        // assertion below bite — an edit has to move it OFF this value.
        prior.recordings = Some(keeper_sync::profile::RecordingsConfig {
            subfolder: "sessions/raw".into(),
            media: keeper_sync::profile::MediaPolicy::PointerOnly,
            push: keeper_sync::profile::PushPolicy::Immediate,
        });

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
        // And flagging it as a recordings root moves `recordings` to a subfolder
        // that does not overlap the vault just configured above.
        edit.recordings = Some(true);
        edit.recordings_subfolder = Some("media/sessions".into());
        // And flagging it as a sessions zone moves `sessions` from `None`,
        // overlapping neither of the two subfolders above.
        edit.sessions = Some(true);
        edit.sessions_subfolder = Some("60-sessions".into());
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

    /// Story 41.7's whole point, at the layer that carries it: the switch writes
    /// a real `recordings` block, and turning it off REMOVES the block rather
    /// than storing an empty one. Before this, `RecordingsConfig` and the
    /// destination picker both existed and neither was reachable, because no
    /// request could express the field.
    #[test]
    fn flagging_a_folder_writes_a_recordings_block_and_unflagging_removes_it() {
        // Flagging a folder that has never held recordings, with the subfolder
        // box untouched: the block is written with the default that lives in
        // Rust, and nowhere else.
        let mut flag = req();
        flag.recordings = Some(true);
        let flagged = parse_req(&flag, None).expect("valid");
        assert_eq!(
            flagged
                .recordings
                .as_ref()
                .expect("the folder now holds recordings")
                .subfolder,
            keeper_sync::profile::RecordingsConfig::default().subfolder
        );
        assert_eq!(
            flagged.recordings_root(),
            Some(std::path::PathBuf::from("/home/u/tgdrive/recordings")),
            "which is what makes it offerable as a recording destination"
        );

        // A subfolder the owner chose is stored as given.
        let mut custom = req();
        custom.recordings = Some(true);
        custom.recordings_subfolder = Some("media/screen-recordings".into());
        assert_eq!(
            parse_req(&custom, None)
                .expect("valid")
                .recordings
                .expect("flagged")
                .subfolder,
            "media/screen-recordings"
        );

        // Unflagging clears the block. `None` is "holds no recordings", so a
        // default-filled block left behind would keep the folder in the picker
        // after the owner took it out.
        let mut unflag = req();
        unflag.recordings = Some(false);
        let cleared = parse_req(&unflag, Some(&flagged)).expect("valid");
        assert_eq!(cleared.recordings, None);
        assert_eq!(cleared.recordings_root(), None);

        // And a request that says nothing leaves the block exactly as it is —
        // AD-34-9, the rule that kept `recordings` alive while it was unreachable
        // and keeps a `keeper-syncd`-configured media/push policy alive now.
        let mut daemon_configured = flagged.clone();
        daemon_configured.recordings = Some(keeper_sync::profile::RecordingsConfig {
            subfolder: "sessions/raw".into(),
            media: keeper_sync::profile::MediaPolicy::PointerOnly,
            push: keeper_sync::profile::PushPolicy::Immediate,
        });
        assert_eq!(
            parse_req(&req(), Some(&daemon_configured))
                .expect("valid")
                .recordings,
            daemon_configured.recordings,
            "a form that does not show the flag cannot move it"
        );

        // Re-flagging an already-flagged folder keeps the policy fields the app
        // has no control for; only the subfolder the form showed moves.
        let mut refit = req();
        refit.recordings = Some(true);
        refit.recordings_subfolder = Some("sessions/final".into());
        let refitted = parse_req(&refit, Some(&daemon_configured))
            .expect("valid")
            .recordings
            .expect("still flagged");
        assert_eq!(refitted.subfolder, "sessions/final");
        assert_eq!(
            refitted.media,
            keeper_sync::profile::MediaPolicy::PointerOnly
        );
        assert_eq!(refitted.push, keeper_sync::profile::PushPolicy::Immediate);
    }

    /// Each refusal `RecordingsConfig::validate` owns, reaching the caller as its
    /// own sentence rather than as a corrected save.
    ///
    /// The correction is the failure mode worth a test: `notes_subfolder` trims
    /// slashes, so had this been written by copying it, `/tmp` would have been
    /// quietly stored as `tmp` and the owner's recordings would land in a folder
    /// they never named — a save that "worked" and put the files somewhere else.
    #[test]
    fn a_recordings_subfolder_the_validator_refuses_is_refused_in_its_own_words() {
        let mut vault = parse_req(&req(), None).expect("valid");
        vault.notes = Some(keeper_sync::profile::NotesConfig {
            subfolder: "10-notes".into(),
            ..Default::default()
        });

        for (subfolder, expected) in [
            (
                "",
                "recordings subfolder must not be empty: recordings live in a folder inside the \
                 profile, never at the profile root",
            ),
            (
                "   ",
                "recordings subfolder must not be empty: recordings live in a folder inside the \
                 profile, never at the profile root",
            ),
            (
                "/tmp",
                "recordings subfolder must be relative to the profile folder, got /tmp",
            ),
            (
                "../x",
                "recordings subfolder must not escape the profile folder: ../x",
            ),
            (
                "10-notes/rec",
                "recordings subfolder 10-notes/rec overlaps notes subfolder 10-notes: one folder \
                 cannot be both a vault and a recordings root",
            ),
        ] {
            let mut bad = req();
            bad.recordings = Some(true);
            bad.recordings_subfolder = Some(subfolder.into());
            let err = parse_req(&bad, Some(&vault))
                .expect_err("a subfolder the validator refuses must not be stored");
            assert_eq!(
                // `SyncError::Config` prints as `invalid sync configuration: {0}`,
                // and that envelope is the whole of what the boundary adds — the
                // sentence inside it is the validator's, word for word, which is
                // what the form puts on screen.
                err.message,
                format!("invalid sync configuration: {expected}"),
                "the validator's own sentence has to reach the form unaltered, since it is the \
                 only thing that says WHICH rule was broken"
            );
        }

        // The control: the same folder, the same vault, a subfolder that breaks
        // no rule. Without this the loop above would pass just as well if
        // flagging were refused outright.
        let mut fine = req();
        fine.recordings = Some(true);
        fine.recordings_subfolder = Some("20-recordings".into());
        assert_eq!(
            parse_req(&fine, Some(&vault))
                .expect("a subfolder beside the vault is not an overlap")
                .recordings
                .expect("flagged")
                .subfolder,
            "20-recordings"
        );
    }

    /// The VM answers "where would recordings go" for a folder that does not hold
    /// any yet, which is what lets the form prefill its subfolder box without
    /// keeping a TypeScript copy of `DEFAULT_RECORDINGS_SUBFOLDER`.
    ///
    /// The notes pair is the cautionary tale sitting right beside it:
    /// `notes_subfolder` is `None` for a folder that is not a vault, so
    /// `add-folder-form.tsx` spells `"notes"` itself, and the two copies are one
    /// rename apart from disagreeing about where a vault lives.
    #[test]
    fn the_profile_vm_answers_where_recordings_would_go_before_a_folder_holds_any() {
        let mut profile = parse_req(&req(), None).expect("valid");
        let vm = SyncProfileVm::from(&profile);
        assert!(!vm.recordings, "an unflagged folder holds no recordings");
        assert_eq!(
            vm.recordings_subfolder, DEFAULT_RECORDINGS_SUBFOLDER,
            "and the form still learns the subfolder flagging it would use"
        );

        profile.recordings = Some(keeper_sync::profile::RecordingsConfig {
            subfolder: "media/screen-recordings".into(),
            ..Default::default()
        });
        let vm = SyncProfileVm::from(&profile);
        assert!(vm.recordings);
        assert_eq!(
            vm.recordings_subfolder, "media/screen-recordings",
            "once it holds them, the stored answer is the one in force"
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

    /// A regenerable path that took the remote's version is neither a conflict
    /// nor silence. Nothing was kept and nothing has to be deleted, so the
    /// sentence must not send anybody looking for a `.sync-conflict-` file that
    /// does not exist — the only action is to run whatever writes the file.
    #[test]
    fn a_regenerable_path_reads_as_regenerate_not_as_conflict() {
        let one = outcome_line(&SyncOutcome {
            pulled: true,
            stale: vec!["10-notes/index.md".to_owned()],
            ..SyncOutcome::default()
        });
        assert_eq!(
            one,
            "Took the remote's version of 1 generated file — regenerate it."
        );
        assert!(!one.contains("Kept your version"));

        let many = outcome_line(&SyncOutcome {
            pulled: true,
            stale: vec!["a/index.md".to_owned(), "b/index.md".to_owned()],
            ..SyncOutcome::default()
        });
        assert_eq!(
            many,
            "Took the remote's version of 2 generated files — regenerate them."
        );

        // Both can happen in one convergence, and the sentence has to carry
        // both without conflating them.
        let both = outcome_line(&SyncOutcome {
            pulled: true,
            conflicts: vec!["notes.sync-conflict-20250725-120000-host.md".to_owned()],
            stale: vec!["index.md".to_owned()],
            ..SyncOutcome::default()
        });
        assert!(both.contains("Kept your version of 1 file"), "{both}");
        assert!(both.contains("1 generated file"), "{both}");
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

    /// The load-bearing half of AD-34-7, and the premise the refusal to gate
    /// `sync_get_credential` rests on (finding 4 of the epic-34 review): the
    /// token crosses the IPC boundary only for a profile a caller *names*, so
    /// the two-second profile poll never carries one and a webview that never
    /// invokes the read never holds a secret.
    ///
    /// Asserted against the serialized shape rather than the struct definition,
    /// because it is the wire form that would leak: a `token` field added to
    /// `SyncProfile` and forwarded here would compile, generate valid TS, and
    /// put every folder's secret in the polled list.
    #[test]
    fn the_polled_profile_list_carries_no_field_that_could_hold_a_secret() {
        let mut profile = SyncProfile::new("p1", "notes", "/Users/alice/notes", "u1");
        profile.author_override = Some("Alice <alice@example.org>".into());
        let vm = SyncProfileVm::from(&profile);
        let json = serde_json::to_value(&vm).expect("a profile VM serializes");
        let serde_json::Value::Object(fields) = json else {
            panic!("a profile VM is a JSON object");
        };
        for name in fields.keys() {
            let lowered = name.to_lowercase();
            assert!(
                !(lowered.contains("token")
                    || lowered.contains("secret")
                    || lowered.contains("credential")
                    || lowered.contains("password")),
                "`{name}` looks like a secret on the polled profile list; the keychain \
                 read is a command of its own for exactly this reason (AD-34-7)"
            );
        }
        // And the key the secret actually lives under is derivable but never
        // carried: the VM has the id, and nothing more.
        assert_eq!(profile.secret_key(), "sync/p1/credential");
        assert!(fields.contains_key("id"));
    }
}
