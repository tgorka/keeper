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

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::backoff::{jitter_sample, Backoff};
use crate::credential::{challenge_accepts_basic, AccessToken};
use crate::db::{self, ActivityKind, ActivityRow, DeviceIdentity, WorkKind, WorkState};
use crate::error::{Result, Retriability, SyncError};
use crate::exclude::ExcludeSet;
use crate::git::{self, cli::GitCli};
use crate::lfs;
use crate::platform::SyncPlatform;
use crate::profile::{LfsMode, ProfileState, PushPolicy, SyncDirection, SyncLane, SyncProfile};
use crate::progress::{
    ProgressSink, RateMeter, SyncPhase, SyncProgress, SyncStatus, TransferTally,
};
use crate::provenance::{commit_message, Provenance, SyncSource};
use crate::sparse::SparseCone;
use crate::stability::{FileSample, StabilityGate, StabilityVerdict};
use crate::volume::{self, VolumeMarker, VolumeStatus};
use crate::watch::{FolderWatcher, WatchConfig, WatchEvent};

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
    /// Paths this run resolved to the remote **without** keeping a copy,
    /// because the profile declares them regenerable
    /// ([`SyncProfile::regenerable`]).
    ///
    /// Nothing is on disk to clean up, which is exactly why they are reported:
    /// the file is now whatever the other device generated, so it is stale
    /// until the tool that writes it runs again — and this run is the only
    /// moment anything knows that happened.
    pub stale: Vec<String>,
}

/// What one convergence had to do beyond merging.
///
/// Two lists rather than one because they ask different things of the user:
/// a copy is a file sitting in the worktree that somebody has to look at and
/// delete, while a stale path is nothing on disk at all — only a fact that the
/// file is now the other device's answer and wants regenerating.
#[derive(Debug, Default)]
struct Converged {
    /// Conflict copies written, as repository-relative paths.
    copies: Vec<String>,
    /// Regenerable paths resolved to the remote with no copy kept.
    stale: Vec<String>,
}

/// What is asking for a recordings push (Story 41.5, FR-136).
///
/// The policy is on the profile and the moment is the caller's, so the two
/// have to meet somewhere: this is that argument. It is deliberately not a
/// boolean — `push_now(profile, true)` at a call site says nothing about which
/// of the two moments in a recording session it is, and those two moments are
/// the entire content of [`PushPolicy`](crate::profile::PushPolicy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushTrigger {
    /// A segment just closed and was committed. Frequent — once a rotation,
    /// so 48 times in a four-hour session.
    SegmentCommitted,
    /// The session finalized. Once, and the last chance this session has to
    /// publish anything.
    SessionEnd,
}

/// What one profile's engine has actually done, as counts (Story 41.5).
///
/// Epic 41's acceptance is stated in counts and not in inspection — 48
/// commits, ONE `.gitattributes` write, one push — because that is what
/// distinguishes the intended behaviour from the failure it is guarding
/// against. A recorder that rewrites `.gitattributes` per rotation still
/// produces a correct tree; it just changes the working tree under a running
/// recorder 47 more times than it should, and only a count can see that.
///
/// Process-lifetime and per profile, like [`Engine::transferred`], and for the
/// same reason: the work being counted is reached through call paths that
/// share no return value — a commit from the supervisor's tick, a push drained
/// from the journal, an attribute write from either the session-start rule or
/// the commit path's own staging. A caller reads the snapshot before and after
/// and subtracts.
///
/// Not persisted. These count what *this process* did, which is exactly the
/// question a session asks; a restart mid-session has bigger news to report
/// than a reset counter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineCounters {
    /// Commit objects this engine created for the profile.
    pub commits: u64,
    /// Times `.gitattributes` was actually rewritten — by
    /// [`Engine::ensure_lfs_rule`] or by the commit path's LFS staging. A
    /// no-op call counts nothing, which is the whole point of counting.
    pub attribute_writes: u64,
    /// Pushes *attempted*: the count rises when `git push` is invoked, whether
    /// or not the remote was there to answer. Attempts rather than successes,
    /// because "we pushed once at session end" is a claim about what the
    /// engine did, and an unreachable remote must not make it read as though
    /// the policy never fired.
    pub pushes: u64,
}

/// How safe one recorded file already is, read locally (Story 41.6, FR-138).
///
/// Three facts and a sentence, in the order durability is actually acquired:
/// the bytes are in a commit, the commit is on the remote, the remote's copy
/// has been re-read and matched. Each is a strictly stronger promise than the
/// one before it, so a consumer can reduce them to one word by taking the
/// highest that holds — which is what the recording surface does.
///
/// Everything here is answered from this machine: the whole point is that a
/// banner polling at ~1 Hz while a capture runs can ask "would what I have
/// recorded so far survive this laptop being dropped?" without a network round
/// trip and without slowing the recorder down by a millisecond.
///
/// Not persisted, and deliberately not cached: it is a *reading* of the
/// repository and the engine's own health, so it cannot go stale or disagree
/// with the thing it describes (AD-S3, the same rule
/// [`ProblemReport`] follows).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathDurability {
    /// The path's bytes are in a commit, and the file on disk still matches
    /// what that commit recorded.
    pub committed: bool,
    /// The commit carrying it has reached the remote — see
    /// [`Engine::path_durability`] for the one approximation in that sentence.
    pub pushed: bool,
    /// The stored content has been re-read and matched. Reserved for
    /// [`Engine::verify`]; never guessed here.
    pub verified: bool,
    /// The last failure the engine recorded for this folder, in the words it
    /// recorded — the reason a rejected push is "recorded, not pushed" rather
    /// than a generic sync error.
    pub problem: Option<String>,
}

/// Result of a verification pass (Story 25.6, Story 56.6).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub checked: u64,
    /// Paths whose content is deliberately not on this machine: the index
    /// carries the committed pointer the worktree holds, and the folder's
    /// compiled [`lfs::virtual_policy::VirtualPolicy`] authorizes that content
    /// to stay away (AD-129).
    ///
    /// Counted rather than merely suppressed. A row nobody reports anywhere is
    /// indistinguishable from a check that stopped running, and the whole risk
    /// of this story is a check that went quiet.
    pub virtual_paths: u64,
    /// `(path, reason)` for everything that failed.
    pub bad: Vec<(String, String)>,
}

/// What one repair pass could and could not put back (DW-208).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepublishReport {
    /// Objects the server does not have, whatever became of them here.
    pub missing: u64,
    /// Of those, the ones this machine can still supply and has queued.
    pub queued: u64,
    /// What that will cost to send.
    pub queued_bytes: u64,
    /// The rest: named, because a human knowing which files to go looking for
    /// on which other machine is the only thing that can still recover them.
    pub unrecoverable: Vec<lfs::audit::MissingObject>,
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
    /// Queued to come IN: an LFS object this clone does not hold yet.
    ///
    /// `replacing` says whether this machine has ever held content for that
    /// path — the difference between a file arriving and a new version of one
    /// arriving. It cannot be inferred from the repository: a queued download
    /// always finds pointer text in the worktree, and history answers a
    /// different question, since a file added upstream a week ago and never
    /// fetched here is new to this clone however old it is there. It is read
    /// from what keeper itself recorded when content last landed.
    ///
    /// The other reasons are computed from `git status` and the completeness
    /// gate, which between them see only what this machine has changed. A
    /// folder pulling 53 GB therefore listed nothing as pending while 106
    /// objects sat in the journal — the list said "not synced yet" and meant
    /// "not synced yet, outbound".
    ///
    /// Carries the size because that is the whole question about an inbound
    /// object: a queue of 106 is two minutes or four days depending on it.
    Incoming { size_bytes: u64, replacing: bool },
}

/// One path the folder is waiting on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingFile {
    /// Repository-relative.
    pub path: String,
    pub reason: PendingReason,
    /// How big the thing waiting is, when that is knowable.
    ///
    /// Inbound rows have always carried it — the announced size of an object
    /// that has not arrived. Outbound rows are measured off the worktree, so a
    /// list holding both reads as one list rather than two half-populated
    /// columns. `None` where there is nothing to measure: a deletion has no
    /// file left, and a path that vanished between the walk and the stat is not
    /// worth failing a listing over.
    pub size_bytes: Option<u64>,
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
    /// Tracked files whose names are not valid UTF-8 (Story 47.2).
    ///
    /// **Reported because the alternative is silence.** git carries such a file
    /// perfectly well — it stores path bytes and never decodes them — so it
    /// commits, pushes and clones like any other, and nothing else in keeper
    /// has any reason to mention it. What keeper cannot do is *name* it: every
    /// surface renders it lossily, so the Files pane shows a row of
    /// substitution characters and no action keyed on that row works. The only
    /// way an owner found one was to go looking with `ls`, which is not a
    /// report.
    ///
    /// **Not an error and not a failure.** Nothing is stuck, nothing is
    /// parked, nothing needs retrying; the honest sentence is "this is here,
    /// this is what it looks like, and keeper cannot act on it by name". It
    /// sits on this report rather than getting a surface of its own because
    /// this is already the list of things about a folder a person should know
    /// and has no other way to learn (AD-S3).
    ///
    /// Read from the index, so it covers files committed long ago that no
    /// status walk would mention. Untracked ones are on the pending list
    /// instead, and once the first sync carries them they appear here.
    pub unspellable: Vec<crate::names::UnspellableName>,
}

/// Units claimed from the journal per profile per tick.
///
/// Small on purpose: a tick that drains a thousand units would hold the
/// profile's reservation for minutes and starve its watcher.
const CLAIM_LIMIT: u32 = 16;

/// Files repaired per pass when the record contradicts the rules.
///
/// Bounded for the same reason [`CLAIM_LIMIT`] is: the folder this was built
/// for has 54 872 of them, and a single commit of 54 872 files is neither a
/// commit anybody can read nor a pass anybody can interrupt. Each pass hashes
/// exactly this many files, publishes a count, and leaves the rest for the
/// next one - and every pass permanently removes that many full reads from
/// every future scan.
///
/// Larger than `CLAIM_LIMIT` because the work is local and cheap per item (one
/// read of a file that is, by construction, under the LFS threshold) where a
/// claimed unit can be a multi-gigabyte transfer.
const REPAIR_BATCH: usize = 500;

/// Index entries examined per repair pass.
///
/// The pass reads a pack for every *filtered* entry it examines, and on the
/// folder this was built for that is most of them. Unbounded, one pass held the
/// index lock for eight minutes at 0% CPU before it had converted anything -
/// which is the same mistake as the walk it replaces, one layer down. A window
/// plus a remembered cursor keeps each pass short and still covers the whole
/// index over successive passes.
const REPAIR_WINDOW: usize = 5_000;

/// The most rows one Pending poll puts in its list, per source.
///
/// A cap on the LIST, not on the truth. The headline count — "N waiting to
/// sync, M waiting for writes to stop" — is computed separately from indexed
/// `COUNT(*)`s (see [`crate::progress::status_line`]), so bounding the list
/// hides no work from the user.
///
/// It bounds two things that were both unbounded, and both measured on the
/// same folder:
///
/// * the repair backlog, ~37 500 paths, where [`REPAIR_WINDOW`] already bounds
///   the probe and this bounds what crosses the IPC boundary;
/// * the settling rows, **128 483** of them, where the poll used to `stat`
///   every single one — 128 483 blocking syscalls on a USB volume, per poll.
///
/// Larger than the list the pane renders, so scrolling never runs out of rows
/// before the engine converts or releases them.
const PENDING_LIST_CAP: usize = 500;

/// How often the supervisor wakes when nothing else prompts it.
const TICK_MS: u64 = 1_000;

/// How long a repair-backlog probe stays good enough to answer a poll with.
///
/// Longer than the pane's five-second poll by enough that a probe costing
/// seconds is amortized rather than repeated, and far shorter than the hours a
/// backlog of tens of thousands takes to drain. See [`Engine::repair_memo`].
const REPAIR_MEMO_TTL: Duration = Duration::from_secs(30);

/// How often a Pending poll pays for a directory walk to look for untracked
/// files.
///
/// The poll runs every five seconds and the directory walk is the expensive
/// half of a status: it `lstat`s the whole tree whatever changed. On the folder
/// this was written for that is 996 s of `lstat` per pass, measured, for rows
/// the stability gate is already holding — a new file is announced by the
/// watcher, recorded by the gate, and shown as `Settling` without any walk
/// finding it.
///
/// So the poll asks the index-only question, and pays for the directory walk on
/// this cadence instead: often enough that a file the watcher missed — a create
/// during a restart, an event the backend dropped — surfaces within the quarter
/// hour, rarely enough that it is no longer the reason the pane says
/// "Scanning". The first poll after a start always sweeps, because a process
/// that has just come up knows nothing about what happened while it was down.
///
/// A profile with no live watcher sweeps on every poll: there is nothing to
/// trust in that case, and correctness outranks the cost.
const UNTRACKED_SWEEP_INTERVAL: Duration = Duration::from_secs(900);

/// How often a folder's transfer scratch is swept.
///
/// Scratch is what an interrupted transfer leaves behind, and nothing else ever
/// reclaims it: it is not in the working tree, so no status walk sees it; it is
/// not an LFS object, so no prune considers it. Until this, the only thing that
/// swept it was the "Recheck all files" button — which meant the space came back
/// only for a person who already suspected something was wrong and went looking
/// for a button to press. On the folder that prompted this, that was 915 files
/// and 123 GB sitting behind a button nobody had a reason to press.
///
/// An hour, matching `SCRATCH_DEBRIS_AGE`: the sweep runs as often as a file can
/// become eligible for it, and no oftener.
const SWEEP_EVERY_MS: i64 = 3_600_000;

/// Paths one release sweep may attempt (Story 56.5, AD-126).
///
/// Bounded for [`CLAIM_LIMIT`]'s reason: the sweep runs inside the reservation
/// the sync tick already holds, and a pass that walked forty thousand eligible
/// paths would hold it for minutes and starve the folder's watcher. Whatever
/// this pass does not reach, the next successful sync does — which is true
/// because of [`Engine::release_cursor`] and was false without it: this bounds
/// ATTEMPTS, and a prefix of candidates that refuses identically on every pass
/// would otherwise hide the tail of the ledger forever.
///
/// `pub` for one reason: `tests/release_sweep.rs` builds its fixture from this
/// number rather than from a literal, so raising the budget does not silently
/// turn "more candidates than one pass allows" into "fewer".
pub const RELEASE_BUDGET_OBJECTS: usize = 32;

/// Bytes one release sweep may put through the content-identity proof.
///
/// The second dimension, and it is not decoration: 56.4's proof **reads every
/// byte** of every candidate before deleting it, so a count ceiling alone
/// permits 32 four-gigabyte files — 128 GB of hashing in a pass that is
/// supposed to be housekeeping. A gigabyte is a few seconds of disk and bounds
/// the actual cost rather than the item count.
///
/// The total **stops the pass** at the first candidate that would cross it,
/// rather than skipping that candidate and carrying on. Skipping would make an
/// object larger than the whole budget permanently unreleasable; stopping means
/// it is simply first in line next time and still gets its one attempt.
///
/// `pub` for [`RELEASE_BUDGET_OBJECTS`]' reason: this ceiling is a promise about
/// what one pass costs, and a suite deriving its object sizes from a literal
/// would stop exercising it the moment the number moved.
pub const RELEASE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// How often a folder's release candidates are looked at.
///
/// An hour, matching [`SWEEP_EVERY_MS`] and for the same shape of reason: the
/// TTL this gate serves is measured in days, so asking every successful sync —
/// which on a busy folder is every few seconds — would read the whole ledger
/// over and over to learn nothing. It is a *look* interval and not a schedule:
/// nothing fires on this clock, it only bounds how often the question is asked
/// on an edge that was going to happen anyway (AD-62 forbids a second
/// scheduler). It is also the grace a folder nothing has ever swept gets before
/// its first deletion — see [`Engine::release_is_due`].
///
/// `pub` for [`RELEASE_BUDGET_OBJECTS`]' reason: `tests/release_sweep.rs`
/// advances its injected clock by this interval to reach a second pass, and a
/// literal there would silently stop reaching one if this changed.
pub const RELEASE_LOOK_EVERY_MS: i64 = 3_600_000;

/// How many consecutive transient failures before a profile stops calling
/// itself healthy.
///
/// Low enough that a genuinely stuck profile surfaces within a few ticks,
/// high enough that a single blip - a momentarily locked file, an EINTR -
/// never raises a notification for something that fixed itself.
const TRANSIENT_FAILURES_BEFORE_WARNING: u32 = 3;

/// How long a profile waits before trying to arm a watcher again.
///
/// A watcher fails for reasons that do not fix themselves in a second — an
/// exhausted inotify instance limit, a mount that refuses to be watched — so
/// retrying at the tick rate would be a syscall storm against a condition that
/// needs a human. A minute is short enough that raising `max_user_watches`
/// takes effect without a restart, and the paced scan covers the whole interval
/// meanwhile.
const WATCH_REARM_INTERVAL_MS: i64 = 60_000;

/// The opening words of the "no watcher" warning.
///
/// A constant rather than a literal in one `format!` because the warning has to
/// be retired again without disturbing anything else on display, and matching
/// its own prefix is the only way to tell our message from someone else's.
const WATCH_DEGRADED_PREFIX: &str = "keeper cannot watch this folder for changes";

/// How many watch events [`Engine::watch_tap`] buffers for a slow subscriber.
///
/// A save storm — a branch checkout, an editor rewriting a directory — delivers
/// thousands of paths in a second, and a subscriber doing real work per path
/// will not keep up with that. 1 024 is generous enough that an ordinary burst
/// is never dropped, and bounded so that a subscriber which stops reading
/// costs a fixed amount of memory rather than one proportional to the tree. Past
/// it the subscriber is told it lagged and can rescan, which is both cheaper
/// and more correct than making the engine wait for it.
const WATCH_TAP_CAPACITY: usize = 1_024;

/// How many unapplied "this path is finished" assertions
/// [`Engine::finished_tap`] holds before it starts dropping them.
///
/// Small on purpose, and the opposite trade to [`WATCH_TAP_CAPACITY`]. A watch
/// event is one of thousands per second and losing one costs a rescan; an
/// assertion is one per closed segment — a few per hour, per recorder — and
/// losing one costs a settle window. A queue this deep can only fill if
/// nothing has drained it for hours, which means the supervisor is not
/// running, which means the assertion had nothing to do anyway.
const FINISHED_TAP_CAPACITY: usize = 64;

/// The producer's end of the finished-path seam: "this absolute path, in this
/// profile, is complete" (Story 41.4, AD-68).
///
/// The inbound sibling of [`Engine::watch_tap`], and it exists so that the
/// direction of the dependency can stay the way that one established. A
/// recorder holding an `Engine` would be a recording subsystem holding the git
/// layer: it could commit, push, pause a profile or delete one, and every one
/// of those is a thing it must never do. What it can do is state a fact about
/// a file it just closed. This handle carries exactly that fact and nothing
/// else, is cheap to clone, and outlives nothing — a send after the engine is
/// gone is a dropped message, not a dangling reference.
///
/// Delivery is asynchronous by design: the assertion is applied by the
/// supervisor's next tick, which is also the tick that would commit the file,
/// so nothing is gained by making the producer wait for a lock the walk holds.
#[derive(Clone, Debug)]
pub struct FinishedTap {
    tx: tokio::sync::mpsc::Sender<(String, PathBuf)>,
}

impl FinishedTap {
    /// Assert that `path` — absolute, inside `profile_id`'s recordings root —
    /// will never be written again.
    ///
    /// Returns whether the assertion was queued. A `false` is not a failure the
    /// caller has to handle (NFR-31): every rejected assertion means only that
    /// the file takes the ordinary settle path, which is what would have
    /// happened if this seam did not exist. The recorder must never stop
    /// recording over a sync detail, so there is nothing here to propagate.
    pub fn note_finished(&self, profile_id: &str, path: impl Into<PathBuf>) -> bool {
        // `try_send` rather than an await: the caller is a driver sink on a
        // capture path, and the one thing it cannot do is block on the sync
        // engine's cadence.
        match self.tx.try_send((profile_id.to_owned(), path.into())) {
            Ok(()) => true,
            Err(err) => {
                tracing::warn!(
                    profile_id,
                    %err,
                    "a finished-path assertion was dropped; the file takes the ordinary settle window"
                );
                false
            }
        }
    }
}

/// Both ends of the [`FinishedTap`] queue, held by the engine.
///
/// The receiver is behind its own mutex rather than owned by the supervisor
/// loop because `run` is not the only thing that may drain it and because the
/// sender has to be mintable from `&Engine`, which the loop's `&mut` receiver
/// could never be.
struct FinishedQueue {
    tx: tokio::sync::mpsc::Sender<(String, PathBuf)>,
    rx: Mutex<tokio::sync::mpsc::Receiver<(String, PathBuf)>>,
}

/// One profile's filesystem watcher, or the memory of it failing.
///
/// A single map holds both so there is exactly one place that answers "does
/// this profile have a watcher, and if not why not" — a second map for
/// failures would be a second thing to keep in step with profile lifetime.
enum ProfileWatch {
    Live {
        /// Dropping this joins the debouncer thread and the rescan thread, so
        /// replacing or removing an entry is what makes "a watcher never
        /// outlives its profile" true rather than aspirational. It is also
        /// asked for the root it was armed over — a profile whose folder moved
        /// needs a new watcher, and comparing against the watcher's own answer
        /// is one fewer copy of the same fact to keep in step.
        watcher: FolderWatcher,
        /// Drained without blocking at the top of every tick.
        events: tokio::sync::mpsc::UnboundedReceiver<WatchEvent>,
    },
    Failed {
        /// The root the failed attempt was for, so a profile that has since
        /// moved is retried at once rather than waiting out a window that was
        /// armed against a different folder.
        root: PathBuf,
        /// Rendered once, at the failure, and re-shown on every tick — the
        /// message names the fix (an inotify sysctl, most usefully) and it
        /// would be a lie to recompute it from a later, healthier world.
        reason: String,
        retry_at_ms: i64,
    },
}

/// How often a walk that is taking time says where it has got to.
///
/// The pace is the whole design. A walk over a small folder finishes inside one
/// interval and reports **nothing**, which is deliberate: publishing a phase on
/// every poll of an idle folder is what made the menu-bar glyph flip once a
/// second (DW-116), and that lesson stands. Only a walk that is genuinely
/// taking time gets to say so - and on the folder this was built for, one
/// second is 30 files out of 155 000, so the line moves visibly.
const WALK_REPORT_INTERVAL: Duration = Duration::from_secs(1);

pub struct Engine {
    platform: Arc<dyn SyncPlatform>,
    /// Single connection behind a mutex. `sync.db` is small and every access is
    /// short; a pool would add contention management for no measurable gain,
    /// and the mutex makes "never hold a `Connection` across an `.await`"
    /// checkable by inspection — every guard here is dropped inside a sync
    /// helper before any await point.
    db: Mutex<Connection>,
    /// This installation's identity.
    ///
    /// Behind a mutex because the LABEL is renameable while the engine runs
    /// (Story 34.5) and every commit reads it. `Engine` is only ever held as an
    /// `Arc`, so `&mut self` is not available; a snapshot accessor
    /// ([`Engine::device`]) is also what keeps the mutex from being held across
    /// a commit. The id inside it never moves — see `db::set_device_label`.
    device: Mutex<DeviceIdentity>,
    git: GitCli,
    http: reqwest::Client,
    /// The client LFS objects move through — see `http::transfer_client`.
    transfer_http: reqwest::Client,
    /// Live status per profile id, the polled snapshot the tray reads.
    status: Mutex<HashMap<String, SyncStatus>>,
    /// Per-profile completeness gates, retained across ticks so a settling file
    /// is remembered rather than re-observed from scratch every time.
    gates: Mutex<HashMap<String, StabilityGate>>,
    /// Profiles with an operation in flight (the one-per-profile rule).
    busy: Mutex<HashMap<String, ()>>,
    /// Untracked entries already reported as unsyncable nested repositories.
    ///
    /// The refusal in [`Self::expand_untracked`] is a standing fact about the
    /// tree, not an event: git reports a nested repository as one collapsed
    /// entry on every single walk, so a folder holding a few dozen of them
    /// re-derived and re-logged the identical warning once per walk, forever.
    /// On one real profile that was 178,000 of the last 200,000 log lines and
    /// a 1.2 GB log. Keyed by `(profile id, path)` and said once.
    collapsed_reported: Mutex<HashSet<(String, PathBuf)>>,
    /// Profiles whose queue has already been offered a name-filling walk in
    /// this process. One walk is enough: what it cannot name — an object whose
    /// path has since been deleted — it will not name on the next drain either,
    /// and repeating an index walk per drain to learn that again is how a
    /// display nicety becomes a cost.
    labels_backfilled: Mutex<HashSet<String>>,
    /// Consecutive transient failures per profile.
    ///
    /// A transient error is retried and, on its own, is not worth alarming
    /// anyone about. A transient error that never stops recurring is a profile
    /// that has silently stopped syncing, and reporting that one as healthy is
    /// the dishonesty this counter exists to prevent. Reset by any success.
    transient_failures: Mutex<HashMap<String, u32>>,
    /// When each profile may next be rescanned for local work.
    ///
    /// The supervisor ticks at 1 Hz because a queued unit should start almost
    /// at once, but WALKING THE TREE at that rate is a different thing
    /// entirely: it re-stats every file every second — on a pendrive, over a
    /// network mount — and it made the menu-bar glyph flip between armed and
    /// working once a second, because a scan that found nothing published the
    /// `Scanning` phase anyway (`collect_stable_changes` no longer does — but
    /// the re-stat itself is reason enough to pace). Draining stays at the tick;
    /// scanning is paced by the profile's own `poll_interval_ms`, which until
    /// now was parsed, validated, documented and read by nothing (DW-116).
    next_scan_ms: Mutex<HashMap<String, i64>>,
    /// When each profile's transfer scratch may next be swept.
    ///
    /// Absent means "now", so a folder is swept on the first tick after keeper
    /// starts. That is deliberate and it is the case that matters: the run that
    /// leaves scratch behind is the one that was killed, and the next start is
    /// the first moment anything can clean up after it.
    next_sweep_ms: Mutex<HashMap<String, i64>>,
    /// When each profile's release candidates may next be looked at (Story
    /// 56.5).
    ///
    /// Beside [`Self::next_sweep_ms`] and deliberately **not** in the same
    /// shape: absent means "an interval from now", not "now".
    /// [`Self::release_is_due`] arms this on first sight and declines the pass,
    /// because this sweep deletes where the other two read. It is removed
    /// wherever the other two windows are — a profile removal, an
    /// enable/disable, an explicit wake — because a window that survived a
    /// re-add would mean a folder somebody just re-attached releases on its very
    /// first sync instead of an interval later.
    ///
    /// **A disabled TTL never puts an entry here, and clears one that is
    /// there.** [`Self::release_is_due`] returns before it reads the clock, so
    /// turning the sweep back on starts a fresh interval rather than finding a
    /// window that has already opened.
    next_release_ms: Mutex<HashMap<String, i64>>,
    /// Where each profile's last release pass stopped, so successive passes
    /// cover the whole ledger (Story 56.5).
    ///
    /// A cursor rather than a fresh start, for [`Self::repair_cursor`]'s reason
    /// and with more at stake. [`RELEASE_BUDGET_OBJECTS`] counts ATTEMPTS, and
    /// the candidate list is re-derived in the same `ORDER BY path` order every
    /// pass — so a prefix that refuses identically every time (32 paths the
    /// owner edited in place; or, on every real host today, simply the first 32,
    /// because `open_file_state` defaults to `Unknown`) meant paths 33..n were
    /// never attempted once, and the promise that a later pass takes the
    /// remainder was false.
    ///
    /// The path and not an index, because rows come and go between passes and
    /// an index into a list that shrank points somewhere else. Wraps at the end;
    /// see [`RELEASE_BUDGET_OBJECTS`].
    release_cursor: Mutex<HashMap<String, String>>,
    /// Absolute path to the binary git should invoke as the `lfs` filter.
    ///
    /// `None` when the running executable cannot be resolved — the filter is an
    /// interoperability convenience for humans using plain `git`, never a
    /// correctness requirement for keeper itself, so a missing path degrades
    /// rather than fails.
    filter_program: Option<PathBuf>,
    /// Whether [`Self::filter_program`] answered the `lfs filter-process`
    /// handshake, and may therefore be registered as `filter.lfs.process`.
    filter_serves_process: bool,
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
    /// Transfer rate per profile, measured across the whole run rather than
    /// within one object.
    ///
    /// [`TransferTally`] carries a [`RateMeter`] of its own, but a tally lives
    /// for exactly one `do_lfs` call, and [`RateMeter`] withholds a figure until
    /// a full second of movement backs it — correctly, because one 100 ms
    /// producer tick can carry a whole buffered chunk. Those two facts together
    /// mean an object that finishes inside a second never reports a rate at all,
    /// so a folder of ordinary files showed throughput only for the occasional
    /// object above ~50 MB and nothing whatsoever for the hundred small ones
    /// around it — which reads as a stalled folder, and was reported as one.
    ///
    /// Fed from the same cumulative counter as [`Engine::transferred`], so what
    /// it measures is the run: previous objects' bytes and the time they took
    /// are exactly what makes a figure available while the current object is
    /// still too young to have one. The meter's own rules are untouched — a
    /// window that carries no bytes still falls silent within
    /// `RATE_WINDOW_MS`, so a genuine stall stops reporting rather than
    /// freezing the last number on screen.
    session_rate: Mutex<HashMap<String, RateMeter>>,
    /// Live filesystem watchers per profile id (AD-34-11).
    ///
    /// The supervisor owns these rather than either host, because the
    /// supervisor is the only consumer of what they emit and AD-52 forbids a
    /// second copy of this policy on the daemon side. `run` tears them all
    /// down on its way out.
    watchers: Mutex<HashMap<String, ProfileWatch>>,
    /// Profiles whose watcher has reported activity that no walk has answered.
    ///
    /// Separate from `watchers` because a wake has to survive its watcher and
    /// its tick: a tick that drains journal work instead of walking must not
    /// swallow one, or the change it announced goes back to waiting for the
    /// paced poll — which is the entire latency bug. Cleared by the walk that
    /// answers it.
    watch_wake: Mutex<HashSet<String>>,
    /// LFS credentials minted by `git-lfs-authenticate` over ssh, keyed by
    /// [`lfs::ssh::SshRemote::cache_key`] (Story 34.17).
    ///
    /// Cached because a credential is per *repository and operation*, while a
    /// journal unit is per *object*: without this, a folder committing forty
    /// large files would open forty ssh connections to be told the same thing
    /// forty times. Entries carry their own expiry — see
    /// [`lfs::ssh::DEFAULT_TTL_MS`] for why a silent server does not get an
    /// eternal one — and are dropped outright when the server rejects the
    /// credential they hold, so a rotated token costs one retry rather than a
    /// wait for the TTL.
    ///
    /// In-process only, like git-lfs's own. A token with a lifetime measured in
    /// hours is not worth a row in `sync.db`, and keeping it out of there keeps
    /// it out of a backup.
    lfs_ssh_credentials: Mutex<HashMap<String, CachedSshAnswer>>,
    /// The fan-out behind [`Engine::watch_tap`], created by the first caller.
    ///
    /// A `OnceLock` rather than a channel built in [`Engine::open`] because
    /// almost nobody taps: `keeper-syncd` never does, and neither does any test
    /// that is not about the tap. An engine nobody has tapped does no extra
    /// work and holds no extra buffer, and one that has been tapped pays a
    /// relaxed atomic load per drain.
    ///
    /// Not a `LazyLock`, which is otherwise this codebase's default for a
    /// value with a fixed initializer: the *fact* of initialization is the
    /// signal here. The drain has to be able to ask "has anyone ever tapped?",
    /// and a `LazyLock` answers that question by initializing — there is no way
    /// to look without becoming the first caller.
    watch_tap: OnceLock<tokio::sync::broadcast::Sender<(String, PathBuf)>>,
    /// The inbound queue behind [`Engine::finished_tap`], created by the first
    /// producer that asks for a handle.
    ///
    /// A `OnceLock` for the same reason [`Self::watch_tap`] is one, and not the
    /// `LazyLock` this codebase otherwise defaults to: the *fact* of
    /// initialization is the signal. The drain asks "has any producer ever
    /// taken a handle?" on every tick, and a `LazyLock` can only answer that
    /// question by becoming the first caller. The payoff is the same too — an
    /// engine nobody asserts to (`keeper-syncd`, every test that is not about
    /// this) allocates no channel, and its drain is one relaxed load per tick.
    finished_tap: OnceLock<FinishedQueue>,
    /// What this engine has done per profile, as counts ([`EngineCounters`]).
    ///
    /// A `HashMap` behind a mutex rather than atomics on the struct because
    /// the counts are per profile and the set of profiles is not known at
    /// construction. Contention is nil: three increments per commit at the
    /// most, against locks the same call already takes for the journal.
    counters: Mutex<HashMap<String, EngineCounters>>,
    /// How often a walk in progress is allowed to publish where it has got to.
    ///
    /// A field rather than a constant so a test can drive the reporting
    /// behaviour without waiting out real seconds per item. Production never
    /// changes it; see [`WALK_REPORT_INTERVAL`] for why the pace exists at all.
    walk_report_interval: Duration,
    /// Where each profile's next repair pass resumes in its tracked-path list.
    ///
    /// A cursor rather than a fresh start, because a pass that always began at
    /// the top would re-examine the same cheap prefix forever and never reach
    /// the inconsistencies deeper in the tree. Wraps at the end; see
    /// [`REPAIR_WINDOW`].
    repair_cursor: Mutex<HashMap<String, usize>>,
    /// The last repair-backlog probe per profile, and when it was taken.
    ///
    /// The probe is cheap next to the walk it replaces and expensive next to
    /// nothing: it opens the repository and materializes every tracked path
    /// before examining a window of them. Measured on a 155 662-entry folder,
    /// 3 to 21 seconds. The Pending pane polls every five seconds, so running
    /// it per poll meant the answer was never ready when the next question
    /// arrived — the pane sat on "Loading folders…" and the allocator churned
    /// a six-figure path list on a loop.
    ///
    /// A backlog of tens of thousands does not change meaningfully inside
    /// [`REPAIR_MEMO_TTL`], and the sweep that drains it publishes its own
    /// progress, so a slightly old answer here is honest.
    repair_memo: Mutex<HashMap<String, (Instant, Vec<PathBuf>)>>,

    /// Profiles with a full-tree status walk in flight.
    ///
    /// Measured on hesperia's `tgdrive`: **two** concurrent walks of the same
    /// 155 662-entry USB folder, in 26 of 26 one-second samples over seven
    /// minutes — two `keeper-status-watchdog` threads, two
    /// `gix::status::index_worktree::producer`s, two `index_as_worktree`
    /// scopes. Nothing bounded it: the commit leg and every Pending poll each
    /// called `status_paths_reported` directly, and a walk of that folder takes
    /// 72 minutes, so a five-second poll only had to reach the walk branch once
    /// to leave a second full-tree pass running for over an hour beside the
    /// first.
    ///
    /// Two walks of one tree are not twice the work — they are twice the work
    /// plus the seek contention of interleaving them on one external volume.
    /// The claim is `try`-only on both legs: nobody waits for a walk, because a
    /// caller that waits 72 minutes for a UI poll is the bug in a new place.
    walking: Mutex<std::collections::HashSet<String>>,

    /// When each profile's Pending poll last paid for a directory walk.
    ///
    /// Absent means "never in this run", and a run that has just started must
    /// sweep: whatever happened while the process was down was announced to
    /// nobody. In memory on purpose — the guarantee this offers is per run, and
    /// a persisted timestamp would let a restart skip the one sweep it most
    /// needs. See [`UNTRACKED_SWEEP_INTERVAL`].
    untracked_sweep: Mutex<HashMap<String, Instant>>,

    /// Profiles whose watcher has named a path the index does not carry.
    ///
    /// The directory scan exists to find files git has never seen, and this is
    /// the only signal that says one may be there. A tracked file being written
    /// sets the ordinary wake and is answered by the index-only walk; without
    /// the distinction every write cost a full-tree `lstat` — 996 s on the
    /// folder this was written for. Cleared by the walk that answers it.
    untracked_appeared: Mutex<HashSet<String>>,
}

/// A profile's in-flight full-tree walk, released when this is dropped.
///
/// A `Drop` guard rather than a matching pair of calls, because the walk it
/// covers can end four ways — completion, the watchdog's abandonment, an
/// unreadable-tree error, or a panic on a worker — and three of those do not
/// come back to the line that made the claim. A leaked claim is a folder that
/// never walks again, which is a far worse failure than the duplicate walk this
/// removes.
struct WalkClaim<'a> {
    walking: &'a Mutex<std::collections::HashSet<String>>,
    profile_id: String,
}

impl Drop for WalkClaim<'_> {
    fn drop(&mut self) {
        Engine::lock(self.walking).remove(&self.profile_id);
    }
}

/// One remembered answer from `git-lfs-authenticate`, with its own deadline.
///
/// The deadline is stored rather than recomputed because the two answers expire
/// for different reasons — a credential when the server says so (or after
/// [`lfs::ssh::DEFAULT_TTL_MS`] when it does not), an absent
/// `git-lfs-authenticate` after a window long enough that enabling it server-side
/// does not need a restart here.
struct CachedSshAnswer {
    answer: lfs::ssh::Answer,
    /// Absolute millisecond, on the platform clock, after which this is void.
    expires_ms: i64,
}

impl Engine {
    /// Open the durable store, recover interrupted work, and probe `git`.
    ///
    /// A missing `git` is fatal here rather than at first use: AD-41 makes it a
    /// declared prerequisite, and discovering it mid-push would leave a profile
    /// half-applied.
    pub fn open(platform: Arc<dyn SyncPlatform>) -> Result<Self> {
        // Before anything opens a repository. A Finder-launched macOS app
        // inherits launchd's 256 descriptors, and gitoxide's object store maps
        // one file per object it reads — a checkout of a few hundred exhausts
        // them mid-tree and the folder silently stops catching up. Once per
        // process; a second engine finds the limit already raised.
        static RAISED: std::sync::Once = std::sync::Once::new();
        RAISED.call_once(|| match crate::openfiles::raise() {
            crate::openfiles::Raised::Raised { from, to } => {
                tracing::info!(from, to, "raised the open-file limit");
            }
            crate::openfiles::Raised::AlreadyEnough(soft) => {
                tracing::debug!(soft, "open-file limit already sufficient");
            }
            crate::openfiles::Raised::Unchanged => {
                tracing::warn!(
                    "could not raise the open-file limit; a large checkout may fail with \
                     `Too many open files`"
                );
            }
        });

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

        // Timeouts live in `crate::http`, and the reason they are not inline
        // here is that their absence was invisible: a client built without them
        // never fails, it waits — and a wait on a dead socket parks the whole
        // profile until the process restarts.
        let http = crate::http::client(crate::AGENT)?;
        // A second client, and the only difference is its read ceiling: an
        // upload's guard is progress, not silence. See
        // `http::TRANSFER_READ_TIMEOUT`.
        let transfer_http = crate::http::transfer_client(crate::AGENT)?;

        let engine = Self {
            platform,
            db: Mutex::new(conn),
            device: Mutex::new(device),
            git,
            http,
            transfer_http,
            status: Mutex::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashMap::new()),
            collapsed_reported: Mutex::new(HashSet::new()),
            labels_backfilled: Mutex::new(HashSet::new()),
            transient_failures: Mutex::new(HashMap::new()),
            next_scan_ms: Mutex::new(HashMap::new()),
            next_sweep_ms: Mutex::new(HashMap::new()),
            next_release_ms: Mutex::new(HashMap::new()),
            release_cursor: Mutex::new(HashMap::new()),
            // `current_exe` is the daemon in a CLI run and the app binary in a
            // desktop run; both understand `lfs clean|smudge`.
            filter_program: std::env::current_exe().ok(),
            // ...but "whatever executable linked the engine" understands the
            // long-running protocol only if it says so, and a `process` driver
            // that cannot be launched wedges the folder rather than degrading
            // (DW-140). Asked once, here, rather than per repository open.
            filter_serves_process: std::env::current_exe()
                .ok()
                .is_some_and(|exe| lfs::filter::serves_process(&exe)),
            sinks: Mutex::new(Vec::new()),
            next_sink: AtomicU64::new(1),
            interrupt: Arc::new(AtomicBool::new(false)),
            transferred: Mutex::new(HashMap::new()),
            session_rate: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            watch_wake: Mutex::new(HashSet::new()),
            lfs_ssh_credentials: Mutex::new(HashMap::new()),
            watch_tap: OnceLock::new(),
            finished_tap: OnceLock::new(),
            counters: Mutex::new(HashMap::new()),
            walk_report_interval: WALK_REPORT_INTERVAL,
            repair_cursor: Mutex::new(HashMap::new()),
            repair_memo: Mutex::new(HashMap::new()),
            walking: Mutex::new(std::collections::HashSet::new()),
            untracked_sweep: Mutex::new(HashMap::new()),
            untracked_appeared: Mutex::new(HashSet::new()),
        };
        engine.seed_status()?;
        Ok(engine)
    }

    /// Claim the one full-tree walk this profile is allowed to have running.
    ///
    /// `None` means somebody else is already walking this folder. There is no
    /// waiting variant on purpose: the two callers are a commit pass and a UI
    /// poll, and on a folder whose walk takes over an hour, a caller that
    /// queued behind one would be indistinguishable from the freeze this
    /// exists to remove. Both legs have a correct answer for "not now" — the
    /// commit pass defers to the supervisor's next tick, and the poll answers
    /// from the index and the gate, which is what it does anyway whenever a
    /// repair backlog exists.
    fn claim_walk(&self, profile_id: &str) -> Option<WalkClaim<'_>> {
        Self::lock(&self.walking)
            .insert(profile_id.to_owned())
            .then(|| WalkClaim {
                walking: &self.walking,
                profile_id: profile_id.to_owned(),
            })
    }

    /// What a Pending poll asks of the walk it is about to run.
    ///
    /// The index-only question is answered from `lstat` per entry; the
    /// directory walk that finds untracked files costs the whole tree, every
    /// time, and on the folder this was written for that is 996 s. The poll
    /// therefore skips it and sweeps on [`UNTRACKED_SWEEP_INTERVAL`] — except
    /// where skipping would mean not knowing:
    ///
    /// * the first poll of a run, because nothing was watching while the
    ///   process was down;
    /// * a profile whose watcher is not live, because then there is no second
    ///   source for a new file at all.
    ///
    /// Records the sweep as taken, so this is called exactly once per poll.
    fn poll_walk_policy(&self, profile_id: &str) -> git::repo::WalkPolicy {
        let watched = matches!(
            Self::lock(&self.watchers).get(profile_id),
            Some(ProfileWatch::Live { .. })
        );
        let now = Instant::now();
        let mut swept = Self::lock(&self.untracked_sweep);
        let due = match swept.get(profile_id) {
            None => true,
            Some(last) => now.saturating_duration_since(*last) >= UNTRACKED_SWEEP_INTERVAL,
        };
        if !watched || due {
            swept.insert(profile_id.to_owned(), now);
            return git::repo::WalkPolicy::full();
        }
        git::repo::WalkPolicy::tracked_only()
    }

    /// What the commit leg asks of its walk.
    ///
    /// Same arithmetic as [`Self::poll_walk_policy`] plus the one input that
    /// matters here: this leg publishes untracked files, so it must not miss
    /// one. The watcher is what makes skipping safe — a file that appears sets
    /// the profile's wake, and a pass with no wake pending has nothing new for
    /// a directory scan to find.
    ///
    /// Measured on the field folder: a pass that spent the scan reported
    /// `elapsed_ms=9299355` — two and a half hours against a busy USB volume —
    /// to find twelve modified files, which the index-only question answers in
    /// under two seconds.
    fn commit_walk_policy(&self, profile: &SyncProfile) -> git::repo::WalkPolicy {
        // The precise question, not the ordinary wake: a tracked file being
        // written sets that too, and the index-only walk answers it in
        // milliseconds. Only a path the index does not carry needs the scan.
        //
        // Consumed rather than merely read — the scan this grants is the one
        // that answers it, and a flag left set makes every later pass pay again
        // for one file that appeared once.
        if Self::lock(&self.untracked_appeared).remove(&profile.id) {
            Self::lock(&self.untracked_sweep).insert(profile.id.clone(), Instant::now());
            return git::repo::WalkPolicy::full();
        }
        self.poll_walk_policy(&profile.id)
    }

    /// Report a walk's progress on every item, for tests.
    ///
    /// The pacing is what keeps an idle folder from publishing a phase once a
    /// second, so a test that wants to *see* a report either waits out a real
    /// interval per item or says this. It exists to make the long-operation
    /// behaviour testable, which is the only way it stays true.
    #[cfg(test)]
    fn report_every_walk_item(&mut self) {
        self.walk_report_interval = Duration::ZERO;
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

    /// A snapshot of this device's identity.
    ///
    /// A clone rather than a borrow: a rename can land at any moment, and
    /// handing out a guard would mean holding the mutex across a commit. A
    /// caller that needs both halves takes ONE snapshot, so no rename can slip
    /// in between reading the label and reading the id and produce a trailer
    /// naming a machine that never existed.
    pub fn device(&self) -> DeviceIdentity {
        Self::lock(&self.device).clone()
    }

    /// Rename this device.
    ///
    /// Takes effect on the next commit and rewrites nothing: `Keeper-Device` on
    /// an existing commit keeps the name this machine had when it made it, which
    /// is the only honest answer to "who wrote this". The id is untouched.
    ///
    /// The row is written first, so a label the store refuses (an empty one)
    /// never reaches the copy every commit reads.
    pub fn set_device_label(&self, label: &str) -> Result<()> {
        let stored = self.with_db(|conn| db::set_device_label(conn, label))?;
        // Updating the in-memory copy is what makes the rename visible without a
        // restart: it, not the row, is what `commit` reads.
        Self::lock(&self.device).label = stored;
        Ok(())
    }

    fn seed_status(&self) -> Result<()> {
        let profiles = self.with_db(db::list_profiles)?;
        let mut status = Self::lock(&self.status);
        for profile in profiles {
            let pending = self.pending_for(&profile.id).unwrap_or(0);
            let mut snapshot = SyncStatus::idle(&profile.id, &profile.name);
            snapshot.pending = pending;
            // Seeded, not left at zero, because a one-shot `keeper-syncd
            // status` never ticks: without this it would read the journal,
            // find nothing, and print "up to date" over a folder whose files
            // are all mid-window (AD-34-10). These are the windows the last run
            // left open — the first walk replaces the count with a measured
            // one, and until then the previous run's answer beats no answer.
            snapshot.settling = self
                .with_db(|conn| db::load_file_state(conn, &profile.id))
                .map(|held| held.len() as u32)
                .unwrap_or(0);
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

    /// Forget a profile: the secrets held for it, then its rows, then the
    /// in-memory state keyed by its id. The folder and its repository are left
    /// on disk untouched — removal is a configuration change, never a deletion
    /// of content.
    ///
    /// The credential goes first, and a keychain failure aborts the removal
    /// before anything is deleted (AD-34-14). The key is derived from the
    /// profile id, so a secret that outlived its row could never be found
    /// again, let alone deleted: the two failure modes are not comparable. This
    /// ordering fails with the profile intact and the removal retryable; the
    /// reverse would trade that for a permanent orphan.
    ///
    /// **Two stores hold a secret for a profile, not one** — finding 1 of the
    /// epic-34 review of Story 34.7. The keychain holds the token the user
    /// pasted. `lfs_ssh_credentials` holds the `Authorization` header
    /// `git-lfs-authenticate` minted from it, keyed by *remote and operation*
    /// rather than by id, which is exactly why the id-keyed sweep below walks
    /// straight past it. Left behind, it is a spendable credential for a folder
    /// keeper no longer syncs, live until the expiry the server named or
    /// `lfs::ssh::DEFAULT_TTL_MS` (ten minutes) when it named none. It is
    /// dropped inside the block below for two reasons: the cache key is derived
    /// from `remote_url`, which nothing can recover once the row is gone, and
    /// doing it before the row delete means even a removal that fails halfway
    /// has taken every secret with it.
    ///
    /// What a half-failed removal leaves, deliberately: if the row delete fails
    /// after both secrets are gone, the profile is still there and still
    /// removable — `remove_profile` is idempotent, and a `secret_delete` of an
    /// absent secret succeeds by contract. Until it is retried the folder
    /// reports an authentication failure on its next sync, which is the honest
    /// symptom and is fixed by entering the token again. The status, gates,
    /// watcher and journal rows are left alone in that case because the profile
    /// genuinely still exists: a live row whose engine state was swept is a
    /// worse thing to be left holding than a live row whose secret is gone.
    ///
    /// Lives here rather than in the Tauri command because the ordering above
    /// is only a guarantee if the deletes are one operation: split across
    /// crates, any second caller would have to re-derive it and would be free
    /// to get it wrong. Today the only caller outside tests is
    /// `sync_profile_remove` in `keeper`'s `sync_ipc`. An earlier version of
    /// this comment named `keeper-syncd` as a second caller; it is not one and
    /// never was — the daemon has no removal path at all, and its `reconcile`
    /// deliberately *reports* a profile the config stopped naming rather than
    /// deleting it (AD-48). The read side of the same secret already lives here
    /// (`Self::credential`), which is the reason that does hold.
    pub fn remove_profile(&self, id: &str) -> Result<()> {
        // Read before deleting: after the row is gone there is nothing left to
        // ask for the key. A profile that is already absent has no secret to
        // delete, and the row deletes below stay idempotent either way.
        if let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? {
            self.platform.secret_delete(&profile.secret_key())?;
            // The second store, and the one an id-keyed sweep cannot reach.
            self.forget_ssh_credentials(&profile);
        }
        self.with_db(|conn| db::delete_profile(conn, id))?;
        Self::lock(&self.status).remove(id);
        Self::lock(&self.gates).remove(id);
        Self::lock(&self.next_scan_ms).remove(id);
        Self::lock(&self.next_sweep_ms).remove(id);
        Self::lock(&self.next_release_ms).remove(id);
        Self::lock(&self.release_cursor).remove(id);
        self.drop_watcher(id);
        Ok(())
    }

    /// Pause or resume a profile.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<()> {
        let Some(mut profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        profile.enabled = enabled;
        self.upsert_profile(&profile)?;
        // Said out loud, because until now it was not.
        //
        // Pausing wrote a row and nothing else: no line in the log, and a
        // transfer already in flight kept its window before anything noticed.
        // So a person who pressed Pause on a folder that was retrying saw it go
        // on retrying, and had no way to tell whether the press had landed —
        // and neither did anybody reading the log afterwards, which is how a
        // report of "pause does not work" arrives with no evidence either way.
        tracing::info!(
            profile = profile.name,
            enabled,
            "sync profile paused or resumed on request"
        );
        if !enabled {
            // The state says so at once rather than at the next tick, which is
            // the tick this profile is about to stop having.
            self.set_state(&profile.id, ProfileState::Paused);
        }
        if enabled {
            // Work deferred while paused becomes eligible again immediately;
            // making the user wait out a backoff they did not cause is rude.
            let now = self.platform.now_ms();
            self.with_db(|conn| db::undefer_profile(conn, id, now))?;
            // ...and neither is making them wait out a scan window: resuming is
            // a request to look now.
            Self::lock(&self.next_scan_ms).remove(id);
            Self::lock(&self.next_sweep_ms).remove(id);
            Self::lock(&self.next_release_ms).remove(id);
            Self::lock(&self.release_cursor).remove(id);
        }
        Ok(())
    }

    pub fn status(&self, id: &str) -> Result<SyncStatus> {
        let mut snapshot = Self::lock(&self.status)
            .get(id)
            .cloned()
            .ok_or_else(|| SyncError::Config(format!("no such sync profile: {id}")))?;
        self.fill_queued(&mut snapshot);
        Ok(snapshot)
    }

    pub fn statuses(&self) -> Result<Vec<SyncStatus>> {
        let mut all: Vec<SyncStatus> = Self::lock(&self.status).values().cloned().collect();
        for snapshot in &mut all {
            self.fill_queued(snapshot);
        }
        all.sort_by(|a, b| a.profile_name.cmp(&b.profile_name));
        Ok(all)
    }

    /// Answer "how much is left" from the journal, at the moment of asking.
    ///
    /// These two are **derived**, and keeping them in the snapshot was the same
    /// mistake [`Self::pending`] refuses to make: a remembered answer to a
    /// question the journal already answers disagrees with it the moment
    /// anything changes. It did, and visibly — a unit completed, the Pending
    /// list dropped it (that list is computed), and the line above went on
    /// saying "94 files left, 48.7 GB" because the snapshot was written only
    /// after a whole claimed batch of up to [`CLAIM_LIMIT`] units had been
    /// attempted. On a slow link that is hours of a number that never moves.
    ///
    /// One indexed aggregate over a table holding a queue, on a path polled
    /// every couple of seconds. Cheaper than the bug it removes, and it cannot
    /// go stale because there is nothing left to go stale.
    ///
    /// A failed read leaves the pair at zero rather than propagating: the
    /// caller asked for a status, and a folder that cannot be counted is still
    /// a folder whose state, phase and error are worth reporting.
    fn fill_queued(&self, snapshot: &mut SyncStatus) {
        if let Ok((files, bytes)) =
            self.with_db(|conn| db::queued_transfers(conn, &snapshot.profile_id))
        {
            snapshot.queued_files = files;
            snapshot.queued_bytes = bytes;
        }
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
    ///
    /// A frame in a rate-carrying phase that arrives without a figure is given
    /// the run's own (see [`Engine::session_rate`]): the object in flight may be
    /// too young to have measured anything, and the transfer it is part of is
    /// not. This is the single choke point every frame passes through, so the
    /// HTTP leg, the filesystem-remote copy and the fetch leg all report a rate
    /// on the same terms instead of three of them deciding separately.
    fn publish(&self, mut event: SyncProgress) {
        if event.bytes_per_second.is_none() && event.phase.carries_rate() {
            // Observed rather than merely read, so the window still rolls while
            // frames arrive: a transfer that stops moving falls silent here
            // exactly as it does inside one object's tally.
            event.bytes_per_second = self.observe_session_rate(&event.profile_id, Instant::now());
        }
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

    /// Retire whatever in-flight progress a profile's snapshot is still
    /// carrying, leaving every other field alone.
    ///
    /// The snapshot is the authoritative channel — the tray reads it with no
    /// webview attached, `keeper-syncd status` reads it from another process
    /// entirely, and the window's `isSyncStatusActive` gates its bar on it — so
    /// a phase left set after the work stopped is not a stale pixel. It is
    /// three surfaces claiming a folder is busy. A push refused with a 401 sat
    /// in `NeedsAttention` with `phase = Pushing` for the life of the process,
    /// under a half-full bar and a frozen "4.1 MB/s", with the error text
    /// directly below it (Story 34.8, Story 34.10).
    ///
    /// The counters go with the phase. Leaving `files_done` and `bytes_total`
    /// behind an `Idle` phase would keep "4 of 9 files" available to any
    /// surface that reads the numbers without consulting the phase first, which
    /// is the same mistake one layer down.
    ///
    /// Nothing is published to the sinks, deliberately. [`Self::record_failure`]
    /// runs per journal unit, so a failed unit followed by a healthy one would
    /// emit an `Idle` frame between two live ones and blink the bar off and on;
    /// and the streamed channel is only ever allowed to refine what the polled
    /// snapshot already authorised, so clearing the snapshot is what actually
    /// retires the claim on every surface.
    fn clear_phase(&self, profile_id: &str) {
        if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
            snapshot.phase = SyncPhase::Idle;
            snapshot.files_done = 0;
            snapshot.files_total = None;
            snapshot.bytes_done = 0;
            snapshot.bytes_total = None;
        }
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

    /// Record a sticky warning and notify exactly once per onset (Story 29.6).
    ///
    /// **Onset means `None → Some` on the snapshot's `warning` — the presence
    /// of a problem appearing, never its wording changing.** That distinction
    /// is the entire story. The supervisor ticks at 1 Hz and three of the
    /// callers below embed a number that moves with the condition they are
    /// describing: [`Self::record_failure`]'s "sync has failed {n} times in a
    /// row", the conflict count [`Self::do_pull`] reports, and the skipped-file
    /// count on an iCloud-evicted tree. A rule keyed on the message *text*
    /// therefore read every one of those ticks as a fresh onset and posted a
    /// native notification for each — 3 600 an hour for a single folder that
    /// stayed broken, which is the failure mode AD-51 calls a notification
    /// storm and which trains a user to turn keeper's notifications off
    /// entirely.
    ///
    /// Keying on presence makes "the state changed" and "a notification
    /// happens" the same event by construction: the send site sits inside the
    /// one branch that performs the transition, so there is no de-duplication
    /// cache beside the state that could ever disagree with it. This mirrors
    /// `fold_recording_event`'s `onset.then_some(message)` idiom in the shell
    /// (Story 18.4's loud-failure triad), deliberately rather than inventing a
    /// second way to spell the same rule.
    ///
    /// The **level** still drives every surface a user can re-read: the message
    /// is written on every call, so a rising failure count, a growing conflict
    /// tally, or a permanent error arriving over a transient run is on the
    /// AD-51 banner and behind the tray glyph immediately. Only the native
    /// toast is edge-triggered, because it is the one surface that cannot be
    /// re-read and so must not be repeated. The `tracing::warn!` line is on the
    /// same edge for the same reason — a per-tick log line is the same storm in
    /// a different sink — and the Transient arm of [`Self::record_failure`]
    /// already logs every individual failure at `warn` for diagnosis.
    ///
    /// The edge re-arms only when the warning is retired
    /// ([`Self::clear_warning`] on any successful unit, or
    /// [`Self::clear_watch_warning`] when a watcher arms), which is what makes
    /// a cleared-then-recurring problem notify again: the `Some → None` write
    /// *is* the reset, so no separate "already notified" flag can be left
    /// stale behind it.
    ///
    /// A consequence worth naming: a *second, different* problem on a profile
    /// that is already warning does not post its own toast. That is the right
    /// trade at this cadence — the user has already been told this folder needs
    /// attention and the banner carries the current wording — and the
    /// alternative is exactly the text-keyed rule this replaces.
    ///
    /// The onset check and the notification are deliberately split: the
    /// notification is raised *after* the status lock is released, mirroring
    /// `fold_recording_event`'s discipline in the shell, because a notifier can
    /// block and holding a lock across it would stall the tray tick.
    fn warn(&self, profile_id: &str, profile_name: &str, message: String) {
        let raised = {
            let mut status = Self::lock(&self.status);
            match status.get_mut(profile_id) {
                Some(snapshot) => {
                    let onset = snapshot.warning.is_none();
                    // Cloned only on the edge: the repeat path runs on every
                    // tick a warning stays raised, and must not allocate a copy
                    // of a message nobody is going to send.
                    let raised = onset.then(|| message.clone());
                    snapshot.warning = Some(message);
                    raised
                }
                None => None,
            }
        };
        if let Some(message) = raised {
            tracing::warn!(profile = profile_name, message = %message, "sync warning");
            self.platform
                .notify(&format!("Sync — {profile_name}"), &message);
        }
    }

    /// Retire this profile's problem surfaces after a successful unit.
    ///
    /// This is also the reset for [`Self::warn`]'s onset edge, and deliberately
    /// the *only* one: because the edge is read off `warning` itself, wiping the
    /// field is what re-arms it, so a cleared-then-recurring problem notifies
    /// again (Story 29.6's second acceptance clause) without a separate
    /// "already notified" flag that a future caller could forget to clear here.
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

        tracing::info!(device = %self.device().id, "sync supervisor started");
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
        // The supervisor is the only consumer of what a watcher emits, so the
        // watchers stop with it. Leaving them armed would hold an inotify
        // instance and two threads per profile for the life of the process, and
        // a later `start_supervisor` would arm a second set beside them.
        self.stop_all_watchers();
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
        // Before the profile list, and outside the `enabled` filter below: an
        // assertion for a paused profile still has to be recorded, or the
        // segment waits out a settle window that the pause made meaningless.
        self.drain_finished_assertions();
        let profiles = self.list_profiles()?;
        // A watcher must not outlive its profile (AD-34-11). Pausing one or
        // removing one is visible only from here: `tick_profile` never runs for
        // a disabled profile, so it cannot be the thing that notices.
        self.retain_watchers(&profiles);
        for profile in profiles {
            if !profile.enabled {
                continue;
            }
            self.sweep_scratch_if_due(&profile).await;
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
            // A watcher on a detached drive is watching a path that no longer
            // exists; the tick after the volume returns arms a fresh one.
            self.drop_watcher(&profile.id);
            return Ok(());
        }

        let _reservation = match self.reserve(&profile.id) {
            Some(guard) => guard,
            // Still working from a previous tick. Not an error.
            None => return Ok(()),
        };

        // Arming happens after the volume gate — the root has to exist — and
        // inside the reservation, so nothing drains a wake on a tick that is
        // about to bail without answering it.
        self.ensure_watcher(profile);
        self.fold_watch_events(profile)?;

        let scan = self.scan_due(profile);
        // The supervisor's own pass. Nobody asked for this one — the clock
        // did — so it is the only genuinely `Watch` caller (AD-34-12).
        self.drain_journal(profile, scan, SyncSource::Watch).await
    }

    /// Whether this profile's tree may be walked on this tick, arming the next
    /// window when it may.
    ///
    /// Queued work is drained every tick; discovering NEW work is paced, because
    /// a scan is a full re-stat of the tree, on a pendrive or over a network
    /// mount. It also used to publish a `Scanning` phase whatever it found,
    /// which at 1 Hz made the menu-bar glyph flip continuously on a folder where
    /// nothing was happening; `collect_stable_changes` now reports only a scan
    /// that found work, and the pacing stands on the cost of the walk alone.
    ///
    /// The floor exists because the field is user-supplied: a zero would restore
    /// the every-tick behaviour this replaces, and a profile written before the
    /// field meant anything still deserves a sane cadence. It lives on the
    /// profile as `effective_poll_interval_ms` rather than here, because the
    /// form and the degraded-watcher warning have to name the cadence that is
    /// actually in force and must not re-derive it (AD-34-8).
    fn scan_is_due(&self, profile: &SyncProfile) -> bool {
        let now = self.platform.now_ms();
        let mut due = Self::lock(&self.next_scan_ms);
        match due.get(&profile.id) {
            // First sight of this profile: scan at once, so adding a folder does
            // not appear to do nothing for a quarter of a minute.
            None => {}
            Some(at) if now >= *at => {}
            Some(_) => return false,
        }
        let interval = profile.effective_poll_interval_ms() as i64;
        due.insert(profile.id.clone(), now.saturating_add(interval));
        true
    }

    /// Whether this profile's transfer scratch may be swept on this tick.
    ///
    /// Same shape as [`Self::scan_is_due`] and for the same reason: a schedule
    /// on the platform clock is a schedule a test can advance, and housekeeping
    /// that could only be observed by waiting an hour would not be tested.
    /// Delete transfer scratch this folder will never use again.
    ///
    /// Fenced in `spawn_blocking`: this is `read_dir` plus up to a few thousand
    /// `unlink`s on a volume that may be an external disk, and the supervisor
    /// ticks at 1 Hz. Awaited rather than detached, so two sweeps of the same
    /// folder can never overlap.
    ///
    /// `incomplete/` is swept alongside `tmp/` even though it exists to let a
    /// download resume, because the hasher state that would resume it lives in
    /// the process: once keeper restarts, a partial file there is bytes nothing
    /// can continue from. Keeping it would cost disk to preserve a resume that
    /// cannot happen.
    ///
    /// Never fails the tick. Housekeeping that could stop a folder syncing would
    /// have traded a disk-space problem for a sync problem.
    async fn sweep_scratch_if_due(&self, profile: &SyncProfile) {
        if !self.sweep_is_due(profile) {
            return;
        }
        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        let name = profile.name.clone();
        // The sweep reports its own anomaly when it removes anything, so there
        // is nothing to log here: a sweep that found nothing is the normal case
        // and has nothing to say.
        let _ = tokio::task::spawn_blocking(move || store.sweep_scratch(&name)).await;
        self.report_blobs_over_threshold(profile).await;
    }

    /// Say, once an hour, how much of this folder git is carrying as plain
    /// blobs that today's threshold would have sent to LFS.
    ///
    /// The threshold moves — this folder went from 4 MiB to 0.25 MB — and a
    /// commit cannot change its mind afterwards. So a drive quietly accumulates
    /// files the current rule disagrees with, and nothing says so until
    /// somebody runs the check by hand, which they only do once they already
    /// suspect something. Reported and not fixed: converting them means
    /// rewriting history, which is not a thing keeper does to a folder two
    /// people are syncing.
    async fn report_blobs_over_threshold(&self, profile: &SyncProfile) {
        let root = profile.local_path.clone();
        let threshold = profile.lfs_threshold_bytes;
        let name = profile.name.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let Ok(repo) = git::repo::open(&root, false) else {
                return;
            };
            let Ok(tracked) = git::repo::tracked_paths(&repo) else {
                return;
            };
            let (count, bytes) =
                crate::footprint::blobs_over_threshold(&repo, &root, &tracked, threshold);
            if count == 0 {
                return;
            }
            crate::anomaly::Anomaly {
                what: "files git carries as plain blobs that today's threshold would send to LFS",
                measured: format!("files={count} bytes={bytes} threshold={threshold}"),
                expected: "none, on a folder whose threshold has never moved",
                consequence: "history keeps them as blobs for good; the cost is permanent and \
                              known, and nothing new is being added to it",
            }
            .report(&name);
        })
        .await;
    }

    fn sweep_is_due(&self, profile: &SyncProfile) -> bool {
        let now = self.platform.now_ms();
        let mut due = Self::lock(&self.next_sweep_ms);
        match due.get(&profile.id) {
            None => {}
            Some(at) if now >= *at => {}
            Some(_) => return false,
        }
        due.insert(profile.id.clone(), now.saturating_add(SWEEP_EVERY_MS));
        true
    }

    /// Whether this profile's release candidates may be looked at now (Story
    /// 56.5).
    ///
    /// [`Self::sweep_is_due`]'s shape, with two differences, and both of them
    /// are the point.
    ///
    /// **A disabled TTL is answered before the clock is read, and it clears the
    /// window.** Both of the other two gates stamp a next-due instant as a side
    /// effect of being asked. If a `releaseTtlMs = 0` folder asked this one it
    /// would arm a window; an operator who turned the sweep on an hour later
    /// would find that window already open and see content released on the very
    /// next sync — the opposite of what turning a knob on should mean. So the
    /// zero returns `false` without reading the clock at all, and it `remove`s
    /// whatever entry a previous non-zero setting left behind. That `remove` is
    /// what makes `docs/sync.md`'s promise — it disables the sweep before any
    /// window is armed, so turning it back on later starts a fresh window rather
    /// than firing on the next sync — true rather than false: without it, the
    /// clock runs on while the sweep is off and re-enabling walks straight into
    /// a window that opened in the meantime. The test asserts the map is empty
    /// rather than only that the answer was `false`.
    ///
    /// **First sight ARMS and DECLINES**, where [`Self::scan_is_due`] arms and
    /// proceeds. That divergence is deliberate: a scan is a read, so a folder
    /// just added may as well be walked at once, while this sweep DELETES. A
    /// folder keeper has never swept — freshly added, resumed, re-enabled, or a
    /// daemon that has just restarted — is exactly the one whose ledger keeper
    /// knows least about: rows an older keeper wrote, clocks that moved while
    /// nothing was watching, an owner who may be halfway through attaching it.
    /// So it gets one whole [`RELEASE_LOOK_EVERY_MS`] of grace, and **no release
    /// happens on a profile's very first successful sync**.
    fn release_is_due(&self, profile: &SyncProfile) -> bool {
        // The lock first, because the zero arm has to be able to clear an entry
        // and must do it without reading the clock.
        let mut due = Self::lock(&self.next_release_ms);
        if profile.effective_release_ttl_ms().is_none() {
            due.remove(&profile.id);
            return false;
        }
        let now = self.platform.now_ms();
        let armed = now.saturating_add(RELEASE_LOOK_EVERY_MS);
        match due.get(&profile.id) {
            // First sight: arm the window and decline the pass. See this
            // method's doc — this is the divergence from `scan_is_due`, and it
            // is what buys a folder nothing has ever swept its interval of
            // grace before anything is deleted from it.
            None => {
                due.insert(profile.id.clone(), armed);
                false
            }
            Some(at) if now >= *at => {
                due.insert(profile.id.clone(), armed);
                true
            }
            Some(_) => false,
        }
    }

    /// Whether this profile's tree may be walked on this tick.
    ///
    /// Three independent reasons, and only the last of them existed before
    /// AD-34-11:
    ///
    /// 1. **The watcher saw something.** The low-latency path: a file that
    ///    lands is walked on the next tick rather than on the next poll.
    /// 2. **A held file's window has elapsed.** A settling path needs a
    ///    *second* observation to clear, and observations happen only inside a
    ///    walk — so before this the second one arrived on the paced poll, and a
    ///    file took two poll intervals to sync rather than its settle window.
    ///    That, and not the settle window, was the reported "waiting for writes
    ///    to stop takes ages".
    /// 3. **The paced backstop**, unchanged, because it is the only cover for
    ///    what a watcher cannot see: a dropped event queue, a mount that
    ///    notifies nothing (NFS, CIFS, some FUSE), and a watcher that failed to
    ///    arm at all.
    ///
    /// [`Self::scan_is_due`] is evaluated FIRST and unconditionally, never
    /// short-circuited away: it is the thing that advances the paced window, so
    /// letting an event-driven walk skip it would let a busy folder starve the
    /// backstop it depends on.
    ///
    /// None of this can walk faster than the 1 Hz tick, and both new reasons
    /// are self-limiting besides: the debouncer collapses a burst into one
    /// event per path per 500 ms, and a deadline is by construction at least
    /// `CLOSE_WRITE_SETTLE_MS` later than the observation that produced it.
    fn scan_due(&self, profile: &SyncProfile) -> bool {
        let paced = self.scan_is_due(profile);
        paced || self.watch_wake_pending(&profile.id) || self.settle_window_elapsed(profile)
    }

    /// Whether any path this profile is holding could be stable by now.
    ///
    /// Reads the live gate and deliberately never creates one: a gate is seeded
    /// by the first walk, and this question is asked *before* every walk.
    fn settle_window_elapsed(&self, profile: &SyncProfile) -> bool {
        let now = self.platform.now_ms();
        Self::lock(&self.gates)
            .get(&profile.id)
            .and_then(|gate| gate.next_stable_ms(now))
            .is_some_and(|at| now >= at)
    }

    // -----------------------------------------------------------------------
    // Filesystem watchers (AD-34-11)
    // -----------------------------------------------------------------------

    /// Arm this profile's watcher unless it already has the right one.
    ///
    /// Idempotent, and the healthy case is one hash lookup. Arming lives on the
    /// tick rather than at add / resume / re-attach time on purpose: "every
    /// enabled profile has a watcher" is then a property the supervisor
    /// re-establishes continuously, instead of three call sites that each have
    /// to remember.
    fn ensure_watcher(&self, profile: &SyncProfile) {
        let now = self.platform.now_ms();
        // `Some(reason)` means "still failing, and not yet due for another
        // attempt"; `None` means "arm one"; the early return means the profile
        // already has exactly the watcher it should have.
        let degraded = {
            let watchers = Self::lock(&self.watchers);
            match watchers.get(&profile.id) {
                Some(ProfileWatch::Live { watcher, .. })
                    if watcher.root() == profile.local_path.as_path() =>
                {
                    return;
                }
                Some(ProfileWatch::Failed {
                    root,
                    reason,
                    retry_at_ms,
                }) if *root == profile.local_path && now < *retry_at_ms => Some(reason.clone()),
                _ => None,
            }
        };
        if let Some(reason) = degraded {
            self.warn_watch_degraded(profile, &reason, false);
            return;
        }

        // `WatchConfig::default()` in full: a 500 ms debounce, comfortably below
        // the shortest quiescence window so debouncing never becomes the thing
        // that decides when a file syncs, plus the 15-minute sweep that covers a
        // dropped event queue. `force_poll` stays off deliberately —
        // `notify::PollWatcher` re-stats the whole tree every 30 s, which is
        // *slower* than this engine's own paced scan, so a poll backend would be
        // a worse fallback than the one already in place.
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        match FolderWatcher::start(&profile.local_path, WatchConfig::default(), tx) {
            Ok(watcher) => {
                let replaced = Self::lock(&self.watchers)
                    .insert(profile.id.clone(), ProfileWatch::Live { watcher, events });
                // Retiring the previous watcher hands its teardown to a thread
                // of its own and returns (`watch::retire`), so replacing a
                // watcher costs this tick nothing — it happens with the map
                // unlocked anyway, because an IPC-thread `remove_profile` must
                // never queue behind a tick that is arming.
                drop(replaced);
                self.clear_watch_warning(&profile.id);
            }
            Err(err) => {
                let reason = err.to_string();
                tracing::warn!(
                    profile = profile.name,
                    error = %reason,
                    "folder watch failed to arm; falling back to the paced scan"
                );
                let replaced = Self::lock(&self.watchers).insert(
                    profile.id.clone(),
                    ProfileWatch::Failed {
                        root: profile.local_path.clone(),
                        reason: reason.clone(),
                        retry_at_ms: now.saturating_add(WATCH_REARM_INTERVAL_MS),
                    },
                );
                drop(replaced);
                self.warn_watch_degraded(profile, &reason, true);
            }
        }
    }

    /// Say, in the UI, that this profile is on the paced scan alone.
    ///
    /// Two different things, deliberately separated. The **text** is a state,
    /// re-asserted on every tick the watcher is down, because
    /// [`Self::clear_warning`] retires whatever is on display as soon as any
    /// unit succeeds — and a degradation that stops announcing itself is exactly
    /// the silent downgrade AD-34-11 forbids. The **notification** is an event
    /// and belongs to the onset alone: routing the re-assertion through
    /// [`Self::warn`] would fire a fresh notification every time a successful
    /// unit happened to wipe the text, which on a busy profile is one per sync.
    ///
    /// `reason` is the watcher's own message, which for the one watcher failure
    /// a user can actually fix names the sysctl to raise. The text is otherwise
    /// fixed on purpose — not for [`Self::warn`]'s benefit, which keys its
    /// notification on presence and no longer reads the wording at all, but for
    /// the banner's: the level path below rewrites the line on every tick the
    /// watcher is down, and Story 29.5 keys its announcement on the state
    /// rather than the tick, so a countdown or any other moving number here
    /// would turn one sticky line into a fresh announcement every second. Only
    /// the opening words are matched when it is retired
    /// ([`Self::clear_watch_warning`]), so the tail is free to say anything —
    /// it just must not say something different each time.
    fn warn_watch_degraded(&self, profile: &SyncProfile, reason: &str, notify: bool) {
        let seconds = profile.effective_poll_interval_ms() / 1_000;
        let message = format!(
            "{WATCH_DEGRADED_PREFIX}, so a new file can take up to {seconds} s to sync \
             instead of a second or two. {reason}"
        );
        if notify {
            self.warn(&profile.id, &profile.name, message);
        } else if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
            snapshot.warning = Some(message);
        }
    }

    /// Retire the watcher warning, and only that one.
    ///
    /// [`Self::clear_warning`] wipes whatever is on display, so calling it from
    /// here would silently retire an unrelated problem the moment a watcher
    /// happened to arm. Matching our own opening words keeps the two
    /// independent.
    fn clear_watch_warning(&self, profile_id: &str) {
        if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
            if snapshot
                .warning
                .as_deref()
                .is_some_and(|shown| shown.starts_with(WATCH_DEGRADED_PREFIX))
            {
                snapshot.warning = None;
            }
        }
    }

    /// Drain everything this profile's watcher has delivered since the last
    /// tick, folding the close-writes into the gate.
    ///
    /// A close-write is the one event that means something on its own: it takes
    /// the path down the 1 s `CLOSE_WRITE_SETTLE_MS` route instead of the full
    /// window. That route has existed since Story 26.3 and was unreachable
    /// until now, because nothing outside `watch.rs`'s own tests ever built a
    /// watcher.
    ///
    /// Every other event means only "look at this profile now", and is recorded
    /// as a wake rather than acted on: `git status` decides what changed, and it
    /// is authoritative in a way an event stream is not. That is also why no
    /// [`crate::watch::EchoSuppressor`] registration is needed here — the
    /// engine's own checkouts leave the tree clean against the new HEAD, so they
    /// cost at most one extra fruitless walk rather than a re-commit loop.
    ///
    /// # Tier 0 decides the wake, not just the walk (Story 34.9)
    ///
    /// Every drained path is put to the profile's own
    /// [`StabilityGate::is_excluded`] before it is allowed to mean anything.
    /// The watcher is armed `RecursiveMode::Recursive` over the profile root,
    /// so it reports every write anywhere beneath it; tier 0 used to be
    /// consulted only *downstream*, in `is_stable` and in [`Self::pending`].
    /// A build writing continuously into `node_modules/`, `.venv/`,
    /// `__pycache__/`, `.next/` or `.cache/` therefore set the wake on every
    /// tick, made [`Self::scan_due`] true on every tick, and drove a full
    /// `git status` re-stat of the tree once a second for as long as the build
    /// ran — on a 100 000-file profile, the very cost `next_scan_ms` exists to
    /// pace. An exclusion applied after the walk cannot suppress the walk it
    /// was added to prevent, so it is applied here, at the seam where the wake
    /// is decided.
    ///
    /// **`.git` is excluded here too, and deliberately so.** It needs no
    /// special case: `BUILTIN_EXCLUDES` already carries `.git/**` and
    /// `**/.git/**`, so asking the same set the walk asks is what makes the
    /// engine's own commits, index writes, ref updates and LFS object writes
    /// stop costing a fruitless walk each. Those writes are engine state by
    /// definition — never user content, never committable — so there is no
    /// input under `.git/` for which "look at the worktree now" is the right
    /// answer, and the periodic rescan plus the paced backstop still cover
    /// anything a filtered event might have stood in for. What is *not*
    /// filtered, because tier 0's `.git` rules are subtree rules: an event on
    /// the bare `.git` directory node itself. That is rare, harmless, and
    /// costs at most the one walk it always did.
    ///
    /// A `.gitignore`d path is **not** filtered here, and that is also
    /// deliberate: reading `.gitignore` at this seam would be a second
    /// ignore engine beside git's, disagreeing with it at the margins, on the
    /// hot path. `git status` stays the authority for those.
    ///
    /// Filtering here rather than downstream also keeps two other things
    /// clean. Excluded close-writes no longer enter the gate's
    /// `pending_close_write` set, where nothing consumes them (`is_stable`
    /// answers `Excluded` before it would) and only a later
    /// [`StabilityGate::retain`] on a paced walk swept them out. And the tap
    /// no longer carries build noise, so a subscriber cannot be lagged out of
    /// the broadcast channel — and into a full rescan — by a `node_modules`
    /// storm it would have discarded anyway.
    ///
    /// Everything that survives the filter is also fanned out to
    /// [`Self::watch_tap`], which is an observer and nothing more — see there
    /// for why a failure to deliver is not a failure to sync.
    fn fold_watch_events(&self, profile: &SyncProfile) -> Result<()> {
        // Two phases, and never both locks at once. `try_recv` never blocks, so
        // the watcher map is held for exactly as long as it takes to move what
        // is already queued — an IPC-thread `remove_profile` must not queue
        // behind a tick, and matching a globset per path is not work to do
        // under that lock.
        let mut drained: Vec<WatchEvent> = Vec::new();
        {
            let mut watchers = Self::lock(&self.watchers);
            let Some(ProfileWatch::Live { events, .. }) = watchers.get_mut(&profile.id) else {
                return Ok(());
            };
            while let Ok(event) = events.try_recv() {
                drained.push(event);
            }
        }
        // Nothing delivered: no gate is seeded and no wake is noted, exactly as
        // before. The gate is only reached below because the exclusion lives on
        // it, and seeding one costs a `file_state` read the walk would have paid
        // moments later anyway.
        if drained.is_empty() {
            return Ok(());
        }

        let tap = self.watch_tap.get();
        let mut tapped: Vec<PathBuf> = Vec::new();
        let mut wake = false;
        // Whether any surviving event named a path the index does not carry.
        //
        // This is what decides whether the next commit pass owes a directory
        // scan. A tracked file being written is answered by the index-only
        // walk in milliseconds; only a path git has never seen needs the scan
        // that costs 996 s of `lstat` on this folder. Treating every event as
        // "something new appeared" is what left the scan in the common case —
        // measured at `elapsed_ms=608838` for twelve modified files.
        //
        // The index is read once for the batch, not once per event, and only
        // when there is a surviving event to ask about.
        let mut appeared = false;
        let index = None::<gix::worktree::Index>;
        let mut index = index;
        {
            let mut gates = Self::lock(&self.gates);
            let gate = self.ensure_gate(&mut gates, profile)?;
            for WatchEvent { path, close_write } in drained {
                if gate.is_excluded(&path) {
                    continue;
                }
                wake = true;
                if close_write {
                    gate.note_close_write(&path);
                }
                if !appeared {
                    if index.is_none() {
                        index = self
                            .open_repo(profile)
                            .ok()
                            .and_then(|repo| repo.index_or_empty().ok());
                    }
                    appeared = match (&index, path.strip_prefix(&profile.local_path)) {
                        (Some(index), Ok(rela)) => {
                            let key = gix::path::into_bstr(rela);
                            index.entry_index_by_path(key.as_ref()).is_err()
                        }
                        // No index to ask, or a path outside the folder: assume
                        // the scan is owed rather than skip a new file.
                        _ => true,
                    };
                }
                // Only the tap wants a path beyond this point, so an untapped
                // engine — every daemon, every other test — moves each survivor
                // straight into the drop instead of into a vector.
                if tap.is_some() {
                    tapped.push(path);
                }
            }
        }
        if let Some(tap) = tap {
            // Outside the lock, and every outcome discarded. `send` fails only
            // when nobody is subscribed, which is a subscriber's business and
            // never sync's.
            for path in tapped {
                let _ = tap.send((profile.id.clone(), path));
            }
        }
        // A batch that was entirely excluded is a batch that says nothing: it
        // must leave `scan_due` exactly as it found it, or the filter above buys
        // nothing.
        if wake {
            self.note_watch_wake(&profile.id);
        }
        if appeared {
            Self::lock(&self.untracked_appeared).insert(profile.id.clone());
        }
        Ok(())
    }

    /// Subscribe to every path this engine's watchers report, as
    /// `(profile id, absolute path)`.
    ///
    /// A read-only tap for a host that has to react to a change faster than the
    /// sync cadence does. It is deliberately not a second `notify` watcher:
    /// inotify instance limits, duplicate events, and a second echo-suppression
    /// implementation that would have to agree with this one exactly are all
    /// avoided by fanning out the stream the supervisor already drains.
    ///
    /// What a subscriber may rely on:
    ///
    /// * **It never affects sync.** No subscriber, a dropped subscriber and a
    ///   failed send are the same thing here: ignored. What the engine believes
    ///   changed still comes from `git status`, which is authoritative in a way
    ///   an event stream is not.
    /// * **It is lossy under load, and says so.** The channel holds 1 024
    ///   events; a subscriber that falls behind receives
    ///   `tokio::sync::broadcast::error::RecvError::Lagged` and should read
    ///   that as "rescan", not as an error.
    /// * **It is not filtered.** Tier-0 exclusions and the stability gate
    ///   decide what *sync* does with a path; a subscriber wanting some subset
    ///   of the tree applies its own rule. This reports what the filesystem
    ///   said.
    ///
    /// Callable any number of times, from any thread, and before any profile
    /// exists.
    pub fn watch_tap(&self) -> tokio::sync::broadcast::Receiver<(String, PathBuf)> {
        // The receiver `channel` hands back is dropped immediately: a broadcast
        // sender with no receivers is legal, and `subscribe` is what mints the
        // live ones. Keeping that first one alive would instead make the
        // channel buffer every event forever for a reader that never reads.
        self.watch_tap
            .get_or_init(|| tokio::sync::broadcast::channel(WATCH_TAP_CAPACITY).0)
            .subscribe()
    }

    /// Mint a producer handle for asserting that a path is finished
    /// (Story 41.4, AD-68).
    ///
    /// The inbound counterpart of [`Self::watch_tap`], and the *only* way a
    /// producer reaches [`Self::note_finished_path`] from another subsystem.
    /// The direction is the point: `watch_tap` established that the engine
    /// hands out a channel rather than itself, and this keeps the recording
    /// code on the far side of that line. A recorder holding an `Engine` could
    /// commit, push, pause a profile or remove one; a recorder holding a
    /// [`FinishedTap`] can state one fact about one file.
    ///
    /// What a producer may rely on:
    ///
    /// * **It never affects capture.** Every rejection — a full queue, an
    ///   unknown profile, a path outside the recordings root — is a `warn` and
    ///   the ordinary settle window, never an error to handle (NFR-31).
    /// * **It is applied on the supervisor's next tick**, which is also the
    ///   tick that would have walked the tree. An engine whose supervisor is
    ///   not running applies nothing, and nothing was going to be committed
    ///   there either.
    /// * **It is bounded, and drops rather than blocks.** See
    ///   [`FINISHED_TAP_CAPACITY`] for why the depth is small.
    ///
    /// Callable any number of times, from any thread, and before any profile
    /// exists.
    pub fn finished_tap(&self) -> FinishedTap {
        let queue = self.finished_tap.get_or_init(|| {
            let (tx, rx) = tokio::sync::mpsc::channel(FINISHED_TAP_CAPACITY);
            FinishedQueue {
                tx,
                rx: Mutex::new(rx),
            }
        });
        FinishedTap {
            tx: queue.tx.clone(),
        }
    }

    /// Apply everything producers have asserted since the last tick.
    ///
    /// Costs one relaxed load on an engine nobody has tapped, which is every
    /// engine that is not hosting a recorder.
    ///
    /// The queue is emptied into a local vector before a single assertion is
    /// applied, because applying one takes the gate lock and a SQLite
    /// transaction: holding the receiver across that would make a producer's
    /// `try_send` queue behind the engine's own disk I/O, which is exactly what
    /// a non-blocking seam exists to avoid.
    ///
    /// A failed assertion is logged and skipped rather than returned. This runs
    /// before the profile loop, and one malformed request must not cost every
    /// profile its tick.
    fn drain_finished_assertions(&self) {
        let Some(queue) = self.finished_tap.get() else {
            return;
        };
        let mut asserted: Vec<(String, PathBuf)> = Vec::new();
        {
            let mut rx = Self::lock(&queue.rx);
            while let Ok(assertion) = rx.try_recv() {
                asserted.push(assertion);
            }
        }
        for (profile_id, path) in asserted {
            match self.note_finished_path(&profile_id, &path) {
                Ok(true) => tracing::debug!(
                    profile_id,
                    path = %path.display(),
                    "a producer declared this path finished"
                ),
                Ok(false) => {}
                Err(err) => tracing::warn!(
                    profile_id,
                    path = %path.display(),
                    %err,
                    "a malformed finished-path assertion was discarded"
                ),
            }
        }
    }

    /// The watcher reported something no walk has answered yet.
    fn note_watch_wake(&self, profile_id: &str) {
        Self::lock(&self.watch_wake).insert(profile_id.to_owned());
    }

    fn watch_wake_pending(&self, profile_id: &str) -> bool {
        Self::lock(&self.watch_wake).contains(profile_id)
    }

    /// A walk has now answered whatever the watcher reported.
    fn clear_watch_wake(&self, profile_id: &str) {
        Self::lock(&self.watch_wake).remove(profile_id);
    }

    /// Tear down a profile's watcher, if it has one, and forget its wake.
    fn drop_watcher(&self, profile_id: &str) {
        let dropped = Self::lock(&self.watchers).remove(profile_id);
        self.clear_watch_wake(profile_id);
        if dropped.is_some() {
            tracing::debug!(profile_id, "folder watch retired");
        }
        // Retiring a live watcher never blocks: `watch::retire` moves the
        // teardown to its own thread precisely because this line is on the IPC
        // thread whenever `remove_profile` is what called us. The map is
        // unlocked first regardless, so nothing else serialises behind it
        // either.
        drop(dropped);
    }

    /// Drop the watcher of every profile that must not have one.
    ///
    /// Takes the whole profile list because *absence* is the signal: a removed
    /// profile is simply not in it, and a paused one is in it with `enabled`
    /// false. `tick_profile` cannot do this job — it never runs for a disabled
    /// profile, which is precisely the case that would leak a thread per resume.
    fn retain_watchers(&self, profiles: &[SyncProfile]) {
        let stale: Vec<String> = {
            let watchers = Self::lock(&self.watchers);
            watchers
                .keys()
                .filter(|id| {
                    let id = id.as_str();
                    !profiles.iter().any(|p| p.enabled && p.id == id)
                })
                .cloned()
                .collect()
        };
        for id in stale {
            self.drop_watcher(&id);
        }
    }

    /// Stop every watcher this engine armed, without waiting for any of them.
    ///
    /// This is the last thing between a shutdown request and `finalize`, so it
    /// is the one teardown that absolutely may not block: see `watch::retire`.
    fn stop_all_watchers(&self) {
        let dropped = std::mem::take(&mut *Self::lock(&self.watchers));
        Self::lock(&self.watch_wake).clear();
        drop(dropped);
    }

    /// This profile's completeness gate, seeded from `file_state` on first use
    /// in this process.
    ///
    /// Without the seeding a one-shot `sync --once` could never reach a second
    /// observation, so every file would be held forever, and an app restart
    /// would silently restart every in-flight quiescence window. Extracted from
    /// the walk so that the watcher's close-write notes and the walk itself
    /// reach the same gate instead of racing to create two.
    fn ensure_gate<'a>(
        &self,
        gates: &'a mut HashMap<String, StabilityGate>,
        profile: &SyncProfile,
    ) -> Result<&'a mut StabilityGate> {
        if !gates.contains_key(&profile.id) {
            let mut fresh = StabilityGate::for_profile(profile)?;
            let saved = self.with_db(|conn| db::load_file_state(conn, &profile.id))?;
            fresh.import(saved);
            gates.insert(profile.id.clone(), fresh);
        }
        gates
            .get_mut(&profile.id)
            .ok_or_else(|| SyncError::Journal("gate vanished after insert".to_owned()))
    }

    /// Declare that `paths` arrived in this profile by **rename** and are
    /// therefore already settled (Story 40.4, FR-129).
    ///
    /// git stores no rename metadata; it infers one at diff time, and
    /// `git log --follow` can only see the inference when the disappearance and
    /// the arrival share a commit. The gate would not let them:
    /// [`Self::collect_stable_changes`] admits a deletion unconditionally — no
    /// file is left to sample — while every moved file arrives at an absolute
    /// path tier 2 has never observed, and an unobserved path is `Settling` by
    /// construction. One move would become a deletion commit now and an
    /// addition commit a settle window later, with the history severed between
    /// them. Priming the destinations first is what collapses the two into one.
    ///
    /// Deliberately inert beyond that: no walk, no `git status`, no commit, no
    /// network. It records gate state and mirrors it to `file_state` the same
    /// way the walk does, so a restart between the move and the sync does not
    /// silently drop the prime; the in-memory gate remains authoritative, as it
    /// is for every other entry ([`StabilityGate::import`] lets it win).
    ///
    /// Paths outside the profile are skipped rather than refused, so a caller
    /// may hand over whatever a walk of the moved folder found without
    /// pre-filtering it, and the return value is how many were actually primed
    /// — [`StabilityGate::prime_stable`] also declines excluded and unstattable
    /// paths.
    pub fn prime_moved_paths(&self, profile_id: &str, paths: &[PathBuf]) -> Result<usize> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            return Err(SyncError::Config(format!(
                "no such sync profile: {profile_id}"
            )));
        };
        let now = self.platform.now_ms();
        let (primed, pending) = {
            let mut gates = Self::lock(&self.gates);
            let gate = self.ensure_gate(&mut gates, &profile)?;
            let mut primed = 0usize;
            for path in paths {
                if path.starts_with(&profile.local_path) && gate.prime_stable(path, now) {
                    primed += 1;
                }
            }
            (primed, gate.export())
        };
        // Outside the gate lock, exactly as the walk does it: every other
        // profile's scan queues behind that lock and a SQLite transaction has
        // no business holding it.
        self.with_db(|conn| db::save_file_state(conn, &profile.id, &pending))?;
        Ok(primed)
    }

    /// Apply one producer's assertion that `path` is finished (Story 41.4,
    /// FR-134, NFR-31).
    ///
    /// The guarded face of [`StabilityGate::note_finished`]: the path must be
    /// absolute and must resolve inside *this* profile's
    /// [`SyncProfile::recordings_root`]. That guard is the whole of the
    /// authorisation story (FR-135). The gate skips its quiescence test on the
    /// strength of the caller's claim, so the claim has to be bounded to the
    /// tree the caller actually produces — a recorder is entitled to say that
    /// the segment it just closed is complete, and entitled to say nothing
    /// whatsoever about the user's spreadsheets in the same folder.
    ///
    /// # What is a refusal, and what is an error
    ///
    /// `Ok(false)` and a `warn` line — never an `Err` — for every condition the
    /// producer could not have known and cannot act on:
    ///
    /// * an unknown profile id (it was removed between the close and the tick),
    /// * a profile that holds no recordings root at all,
    /// * a path somewhere else in the profile, or outside it entirely.
    ///
    /// All three mean the same thing to the file: it takes the ordinary settle
    /// path, which is what it would have taken had this API never been called.
    /// That is the NFR-31 contract in one sentence — the recorder is never
    /// handed an error, because there is no error it could usefully handle
    /// while a capture is running, and a sync detail must not become a failure
    /// on the capture path.
    ///
    /// `Err` is reserved for a request that is malformed rather than merely
    /// unusable: a relative path, or one containing a `..` component. Neither
    /// can be checked against the recordings root — [`Path::starts_with`]
    /// compares components and would happily accept
    /// `<root>/../../etc/passwd` — so accepting them would turn the guard into
    /// a suggestion. A producer cannot reach either state by accident: it names
    /// files it created itself. They are programming errors, and they are the
    /// one thing worth failing loudly for. [`Self::drain_finished_assertions`]
    /// logs and discards them, so even here nothing reaches the recorder.
    ///
    /// A **paused** profile is deliberately not on either list. The assertion
    /// is recorded exactly as it would be for a running one and mirrored to
    /// `file_state`, so the resume honours it: losing it would make the segment
    /// wait out a window that the pause itself made meaningless.
    ///
    /// Returns whether the gate recorded the assertion. `false` also covers the
    /// two answers only the gate can give — the path is excluded by tier 0 (a
    /// `.partial` mid-rotation), or it cannot be stat-ed.
    pub fn note_finished_path(&self, profile_id: &str, path: &Path) -> Result<bool> {
        if !path.is_absolute() || path.components().any(|c| c == Component::ParentDir) {
            return Err(SyncError::Config(format!(
                "a finished-path assertion needs an absolute path with no `..`, got {}",
                path.display()
            )));
        }
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            tracing::warn!(
                profile_id,
                path = %path.display(),
                "finished path asserted against a profile that does not exist; the ordinary settle window applies"
            );
            return Ok(false);
        };
        let Some(root) = profile.recordings_root() else {
            tracing::warn!(
                profile_id,
                path = %path.display(),
                "finished path asserted against a profile that holds no recordings; the ordinary settle window applies"
            );
            return Ok(false);
        };
        if !path.starts_with(&root) {
            tracing::warn!(
                profile_id,
                path = %path.display(),
                root = %root.display(),
                "finished path asserted outside the profile's recordings root; the ordinary settle window applies"
            );
            return Ok(false);
        }
        let now = self.platform.now_ms();
        let (noted, pending) = {
            let mut gates = Self::lock(&self.gates);
            let gate = self.ensure_gate(&mut gates, &profile)?;
            (gate.note_finished(path, now), gate.export())
        };
        // Outside the gate lock, and mirrored the way the walk mirrors it, so a
        // restart between the close and the commit does not silently drop the
        // assertion. See [`Self::prime_moved_paths`], which shares the shape.
        self.with_db(|conn| db::save_file_state(conn, &profile.id, &pending))?;
        Ok(noted)
    }

    /// Ensure this profile's `.gitattributes` carries the LFS rule for
    /// `extension` (Story 41.5, FR-137).
    ///
    /// Returns whether it wrote. A second call for the same extension writes
    /// nothing and returns `false`, and so does a call for an extension a
    /// human already tracked by hand, however they spelled the line —
    /// [`lfs::stage::ensure_attributes`] owns that judgement and this adds no
    /// second opinion.
    ///
    /// # Why an extension, and why now rather than at first commit
    ///
    /// The commit path already writes this rule: [`lfs::stage::prepare`] adds
    /// one for every path it routes through LFS, and a recording would get its
    /// rule the moment the first segment was committed. That is precisely the
    /// problem. A running recorder must not have the working tree change
    /// underneath it: a `.gitattributes` rewritten forty minutes into a session
    /// is a diff the user did not make and a commit nobody asked for, arriving
    /// while the thing it governs is still being written. Called once at
    /// session start, this moves that single write to the one moment where it
    /// is expected, and the commit path then finds nothing to add for the rest
    /// of the session — the count is one, not one plus forty-seven no-ops.
    ///
    /// The extension is all the session knows at that point. There is no file
    /// yet, and the rule is per-extension anyway
    /// ([`lfs::stage::pattern_for_extension`]), so nothing is lost.
    ///
    /// # What is a refusal
    ///
    /// `Ok(false)` and a `warn` — never an `Err` — for every condition the
    /// caller could not have known and cannot act on: an unknown profile, a
    /// paused one, one with LFS switched off, an extension that is not one, a
    /// folder that is not there (an unplugged drive), or a filesystem that
    /// refused the write. Each of them costs the session its LFS rule and
    /// nothing else: the commit path still writes one when the first segment
    /// is committed, which is where this started. A recorder is never handed
    /// an error, because there is no error it could usefully handle while a
    /// capture is running (NFR-31).
    ///
    /// `Err` is reserved for what the two sibling APIs reserve it for — a
    /// `sync.db` that will not answer, which is the engine being broken rather
    /// than this request being refused.
    pub fn ensure_lfs_rule(&self, profile_id: &str, extension: &str) -> Result<bool> {
        let ext = extension.trim().trim_start_matches('.');
        // A gitattributes line is whitespace-separated, so an extension holding
        // a space would not write the rule it reads as — it would write a
        // pattern and then garbage where the attributes belong, and every later
        // idempotence check against it would miss. A separator is refused for
        // the same reason `pattern_for` anchors a path rule: `*.a/b` is not an
        // extension, whatever produced it.
        if ext.is_empty() || ext.contains(['/', '\\']) || ext.chars().any(char::is_whitespace) {
            tracing::warn!(
                profile_id,
                extension,
                "not a usable file extension, so no LFS rule was written; the commit path will \
                 write one when the first segment lands"
            );
            return Ok(false);
        }
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            tracing::warn!(
                profile_id,
                extension = ext,
                "an LFS rule was asked for against a profile that does not exist"
            );
            return Ok(false);
        };
        if !profile.enabled {
            tracing::warn!(
                profile = profile.name,
                extension = ext,
                "this folder is paused, so its working tree is left exactly as the user left it"
            );
            return Ok(false);
        }
        if profile.lfs_mode == LfsMode::Disabled {
            tracing::warn!(
                profile = profile.name,
                extension = ext,
                "LFS is switched off for this folder, so an LFS rule would promise something \
                 nothing here will honour"
            );
            return Ok(false);
        }
        // Cheaper and more specific than a volume probe, and it is the
        // condition that actually matters: `ensure_attributes` writes into this
        // directory. An unplugged drive, a folder the user moved, and a profile
        // whose clone has not happened yet all arrive here as the same answer,
        // and all three want the same one-line warning rather than a failure.
        if !profile.local_path.is_dir() {
            tracing::warn!(
                profile = profile.name,
                path = %profile.local_path.display(),
                extension = ext,
                "the folder is not there, so no LFS rule was written"
            );
            return Ok(false);
        }
        let pattern = lfs::stage::pattern_for_extension(ext);
        match lfs::stage::ensure_attributes(&profile.local_path, std::slice::from_ref(&pattern)) {
            Ok(false) => Ok(false),
            Ok(true) => {
                self.bump_counters(&profile.id, |counters| counters.attribute_writes += 1);
                tracing::info!(
                    profile = profile.name,
                    pattern,
                    "recorded the session's LFS rule before the recorder started writing"
                );
                Ok(true)
            }
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    pattern,
                    error = %err,
                    "could not record the session's LFS rule; the commit path will write it when \
                     the first segment lands"
                );
                Ok(false)
            }
        }
    }

    /// Publish this profile's recordings now, if its own push policy says now
    /// (Story 41.5, FR-136, AD-70).
    ///
    /// `trigger` is the moment asking; [`PushPolicy`](crate::profile::PushPolicy)
    /// is the answer the folder gives:
    ///
    /// * [`Immediate`](crate::profile::PushPolicy::Immediate) — both moments.
    /// * [`SessionEnd`](crate::profile::PushPolicy::SessionEnd), the default —
    ///   only [`PushTrigger::SessionEnd`]. Committing is local, cheap and
    ///   already done; sending a multi-gigabyte object mid-meeting eats the
    ///   uplink the meeting runs on.
    /// * [`Window`](crate::profile::PushPolicy::Window) — either moment, but
    ///   only inside the quiet hours. Outside them the commits simply
    ///   accumulate, which is the point: they are durable already.
    ///
    /// Returns whether anything was published. `Ok(false)` covers both "the
    /// policy says not now" and "it said now and the attempt did not land",
    /// because to the caller — a recorder — those are the same instruction:
    /// carry on, nothing is lost. What was *attempted* is a different question
    /// and a different observation point: [`EngineCounters::pushes`].
    ///
    /// # Nothing is stranded by a refusal
    ///
    /// Every segment is committed regardless, and the supervisor's own scan
    /// enqueues a `Push` for a profile whose branch is ahead of its remote
    /// ([`Self::has_unpushed_commits`]). So a session that never reached its
    /// remote leaves ordinary unpushed commits, and the ordinary path drains
    /// them when the remote returns. That is why this can afford to be total.
    ///
    /// # What it reuses, deliberately
    ///
    /// [`Self::do_push`], with the same reservation and the same volume gate
    /// the supervisor takes. Not a second push path: `do_push` is where the
    /// `lfs_uploads_outstanding` gate lives, and that gate is the one thing
    /// that must never be bypassed — a push that publishes a pointer whose
    /// object has not landed is the failure nobody observes until a peer
    /// clones a tree of text stubs (Story 34.15).
    ///
    /// # What is a refusal
    ///
    /// `Ok(false)` and a log line for an unknown profile, a profile that holds
    /// no recordings, a paused one, a pull-only one, an absent volume, a
    /// profile already busy with another operation, and any failure of the
    /// push itself — an unreachable remote, a rejected ref, an upload still
    /// outstanding. A failure that says something about the folder's health is
    /// reported through [`Self::record_failure`], exactly as the supervisor
    /// reports its own, so a folder that has silently stopped publishing still
    /// says so in the tray.
    ///
    /// `Err`, as with [`Self::ensure_lfs_rule`], only for a `sync.db` that will
    /// not answer.
    pub async fn push_recordings_if_due(
        &self,
        profile_id: &str,
        trigger: PushTrigger,
    ) -> Result<bool> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            tracing::warn!(
                profile_id,
                "a recordings push was asked for against a profile that does not exist"
            );
            return Ok(false);
        };
        let Some(recordings) = profile.recordings.as_ref() else {
            tracing::warn!(
                profile = profile.name,
                "a recordings push was asked for against a folder that holds no recordings"
            );
            return Ok(false);
        };
        if !profile.enabled {
            tracing::info!(
                profile = profile.name,
                "this folder is paused, so nothing is published; the commits are here and the \
                 resume publishes them"
            );
            return Ok(false);
        }
        if !profile.direction.pushes() {
            tracing::info!(
                profile = profile.name,
                "this folder never pushes, so a recording lives here and nowhere else"
            );
            return Ok(false);
        }
        if !self.recordings_push_is_due(&profile, &recordings.push, trigger) {
            return Ok(false);
        }
        // The same gate `tick_profile` takes, before anything touches the
        // filesystem: a detached drive is indistinguishable from a mass
        // deletion once a walk starts, and `do_push` starts with a walk (AD-48).
        match self.volume_ready(&profile) {
            Ok(true) => {}
            Ok(false) => {
                tracing::info!(
                    profile = profile.name,
                    "the volume this folder lives on is not attached, so nothing is published; \
                     the segments are committed when it returns"
                );
                return Ok(false);
            }
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    error = %err,
                    "could not tell whether this folder's volume is attached, so nothing is \
                     published"
                );
                return Ok(false);
            }
        }
        let Some(_reservation) = self.reserve(&profile.id) else {
            // One operation per profile is not negotiable — two on one working
            // tree race on the index — and whatever holds it is doing this
            // work anyway, or the supervisor's next scan will.
            tracing::info!(
                profile = profile.name,
                "this folder is already syncing, so the recordings push rides along with it"
            );
            return Ok(false);
        };
        // `Watch`, because that is what this is: not a person asking, but the
        // engine acting on a policy the folder carries. A recorder-triggered
        // push does exactly what the supervisor's own pass does, and a commit
        // it creates should not claim otherwise (AD-34-12).
        match self.do_push(&profile, SyncSource::Watch, None).await {
            Ok(()) => {
                tracing::info!(
                    profile = profile.name,
                    trigger = ?trigger,
                    "published this folder's recordings"
                );
                Ok(true)
            }
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    trigger = ?trigger,
                    error = %err,
                    "the recordings push did not land; the commits are local and the next pass \
                     publishes them"
                );
                // Health is the supervisor's channel and this failure belongs
                // in it: a folder that keeps failing to publish must stop
                // calling itself healthy, however the push was triggered.
                self.record_failure(&profile, &err);
                Ok(false)
            }
        }
    }

    /// Does this policy fire for this trigger, right now?
    ///
    /// Split out so the decision is one readable match rather than a condition
    /// buried in the middle of the guards, and so the window arithmetic —
    /// [`within_quiet_window`], which is where the wrapping midnight case
    /// lives — is reached from exactly one place.
    fn recordings_push_is_due(
        &self,
        profile: &SyncProfile,
        policy: &PushPolicy,
        trigger: PushTrigger,
    ) -> bool {
        match policy {
            PushPolicy::Immediate => true,
            PushPolicy::SessionEnd => {
                let due = trigger == PushTrigger::SessionEnd;
                if !due {
                    tracing::debug!(
                        profile = profile.name,
                        "this folder publishes at the end of the session, so the segment is \
                         committed and stays here for now"
                    );
                }
                due
            }
            PushPolicy::Window {
                quiet_from,
                quiet_to,
            } => {
                let now = self.platform.now_ms();
                let offset = self.platform.utc_offset_minutes();
                match within_quiet_window(now, offset, quiet_from, quiet_to) {
                    Some(true) => true,
                    Some(false) => {
                        tracing::debug!(
                            profile = profile.name,
                            quiet_from,
                            quiet_to,
                            "outside this folder's quiet hours, so the commits accumulate"
                        );
                        false
                    }
                    // `RecordingsConfig::validate` refuses an unparseable
                    // window, so this is a row edited outside keeper. Refusing
                    // to push is the conservative reading — the alternative is
                    // saturating a metered uplink at noon on the strength of a
                    // value nobody can read.
                    None => {
                        tracing::warn!(
                            profile = profile.name,
                            quiet_from,
                            quiet_to,
                            "this folder's quiet hours cannot be read, so nothing is published \
                             until they are corrected"
                        );
                        false
                    }
                }
            }
        }
    }

    /// How durable one recorded file already is, on this machine, right now
    /// (Story 41.6, FR-138).
    ///
    /// The recording surface polls its own status at ~1 Hz while a capture is
    /// running, and this is the answer it carries: local, committed, pushed.
    /// So the two hard constraints are that it never touches the network and
    /// never costs the recorder anything. Both are met by refusing to ask any
    /// question whose honest answer needs a remote: what this returns is the
    /// last thing this machine *knows*, which is also the only thing a person
    /// asking "would this survive the laptop being dropped?" can act on.
    ///
    /// # What each field reuses, and what it costs
    ///
    /// * `committed` — `HEAD` plus the uncommitted set. The tree lookup goes
    ///   first because it is one object read per path component and it is the
    ///   whole answer for every segment of a session that has not been
    ///   committed yet, which is the state a poll spends most of its time in.
    ///   Only once `HEAD` carries the path is the second half worth its
    ///   syscalls: [`git::repo::status_paths`], the same gitoxide walk
    ///   [`Self::pending`] and [`Self::collect_stable_changes`] already use, so
    ///   "uncommitted" means here exactly what the Pending list means by it and
    ///   there is no second opinion to disagree with. That walk is the
    ///   expensive part of this call — it stats every tracked file and dirwalks
    ///   for untracked ones, bounded by the tree rather than by the session —
    ///   and no `git` process is spawned for it.
    /// * `pushed` — [`Self::has_unpushed_commits`], the same local ancestry
    ///   question the supervisor asks on every tick, guarded by the
    ///   remote-tracking ref existing. The guard is not decoration: that
    ///   helper deliberately answers "no unpushed commits" when the question is
    ///   unanswerable, so the push leg decides rather than the caller, and a
    ///   durability read must not inherit that optimism and report an
    ///   unfetched branch as published. It costs one `git merge-base
    ///   --is-ancestor`, and only for a path that is already committed.
    /// * `verified` — always `false`. Nothing local can answer it:
    ///   [`Self::verify`] re-reads content and returns a [`VerifyReport`] to
    ///   its caller without recording anything per path, so the field is
    ///   reserved for that pass to fill in rather than guessed here. A
    ///   guessed `verified` would be the one word in the set that promises
    ///   something nobody checked.
    /// * `problem` — the sentence [`Self::record_failure`] recorded, read from
    ///   the same in-memory snapshot [`Self::problems`] surfaces. Free: one map
    ///   lookup, no I/O, and no second store to keep in step.
    ///
    /// # The approximations, stated
    ///
    /// * **`pushed` is branch-level, not path-level.** A per-path answer means
    ///   asking the remote which commits it has, and that is a network round
    ///   trip this call may not make. So the claim made is the one the local
    ///   clone can prove: this path is in a commit, and this branch has no
    ///   commits the remote-tracking ref lacks. It is the honest reading of
    ///   "would this survive?" — a session whose branch is fully published has
    ///   published every segment in it — and it is conservative in the
    ///   direction that matters: a later uncommitted segment cannot make an
    ///   earlier one read as pushed, because `committed` gates `pushed`, while
    ///   an unpushed later commit correctly drags the whole branch back to
    ///   `committed`. The caller keeps a floor precisely so that
    ///   retreat is never shown to a person mid-session.
    /// * **`pushed` asks about `profile.branch`**, which is the branch
    ///   `has_unpushed_commits` asks about. A worktree lane publishes a
    ///   different branch ([`Self::lane_branch`]) and would read as unpushed
    ///   here; recordings are not a lane, and inventing a second ancestry
    ///   query for a combination the product does not offer would be two
    ///   answers where one is needed.
    /// * **`problem` is the profile's last recorded *permanent* failure, not a
    ///   push-specific one.** That is the engine's only record of a failure in
    ///   words. A failure still being retried deliberately carries no sentence
    ///   yet (`record_failure`'s policy, not this one), which is right: work
    ///   that is about to be retried is not something to hand a recorder words
    ///   about.
    ///
    ///   A refused push used to land here, because a rejected ref classified as
    ///   [`SyncError::Diverged`] whatever the profile was. On a shared
    ///   bidirectional branch it no longer does: being overtaken is a race, it
    ///   comes back as [`SyncError::RemoteMoved`], and it reconciles on its own
    ///   (DW-207). So the honest reading of a recording in that window is
    ///   `committed` without `pushed` and **no sentence** — the publication has
    ///   not failed, it is a step behind. A race that genuinely will not settle
    ///   still parks eventually, and parking is what records the words.
    ///
    /// # What is a refusal
    ///
    /// Total, like [`Self::note_finished_path`] and [`Self::ensure_lfs_rule`]:
    /// everything `false` with no problem and one `debug` line for an unknown
    /// profile, a folder that holds no recordings, a paused one, and a path
    /// outside the recordings root. A capture must never fail because a status
    /// query did (NFR-31), so a git read that fails is a `warn` and the same
    /// all-`false` answer — the caller holds the last known state, so nothing
    /// it shows regresses on a blip.
    ///
    /// `Err`, as with its siblings, only for a `sync.db` that will not answer.
    ///
    /// Blocking, and cheap enough to be called inline; a caller on an async
    /// runtime with a very large tree should still hand it to `spawn_blocking`,
    /// the way [`Self::pending`] does with the same walk.
    pub fn path_durability(&self, profile_id: &str, path: &Path) -> Result<PathDurability> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, profile_id))? else {
            tracing::debug!(
                profile_id,
                path = %path.display(),
                "a durability read asked about a profile that does not exist"
            );
            return Ok(PathDurability::default());
        };
        let Some(root) = profile.recordings_root() else {
            tracing::debug!(
                profile = profile.name,
                path = %path.display(),
                "a durability read asked about a folder that holds no recordings"
            );
            return Ok(PathDurability::default());
        };
        if !profile.enabled {
            tracing::debug!(
                profile = profile.name,
                "this folder is paused, so what is on the drive is all there is to say"
            );
            return Ok(PathDurability::default());
        }
        // The same boundary [`Self::note_finished_path`] enforces, for the same
        // reason: `Path::starts_with` compares components, so a `..` would walk
        // straight through it and let a caller ask about someone else's file.
        if !path.is_absolute()
            || path.components().any(|c| c == Component::ParentDir)
            || !path.starts_with(&root)
        {
            tracing::debug!(
                profile = profile.name,
                path = %path.display(),
                root = %root.display(),
                "a durability read asked about a path outside this folder's recordings"
            );
            return Ok(PathDurability::default());
        }
        let Ok(rela) = path.strip_prefix(&profile.local_path) else {
            // Unreachable while the recordings root is inside the folder, and
            // answered rather than asserted: nothing on a poll path a recorder
            // depends on is worth a panic.
            tracing::debug!(
                profile = profile.name,
                path = %path.display(),
                "a durability read asked about a path that is not inside this folder"
            );
            return Ok(PathDurability::default());
        };

        // Read first, and from memory, so the reason survives every early
        // return below: a folder whose publication is refused must be able to
        // say why even when nothing else can be read.
        let mut durability = PathDurability {
            problem: Self::lock(&self.status)
                .get(&profile.id)
                .and_then(|snapshot| snapshot.error.clone()),
            ..PathDurability::default()
        };

        // `pending`'s own guard, for `pending`'s own reason: a folder that is
        // not a repository yet has nothing git can classify, and making one is
        // a clone. [`Self::open_repo`] would do exactly that — it clones or
        // adopts — which is a network call and a write to the user's folder,
        // and neither belongs anywhere near a 1 Hz poll.
        if !profile.local_path.join(".git").exists() {
            tracing::debug!(
                profile = profile.name,
                "this folder is not a repository yet, so the recording is on this disk only"
            );
            return Ok(durability);
        }
        let repo = match git::repo::open(&profile.local_path, profile.removable) {
            Ok(repo) => repo,
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    error = %err,
                    "could not read this folder's repository, so its durability is unknown; \
                     the last known state stands"
                );
                return Ok(durability);
            }
        };

        // `HEAD` before the walk: this is the cheap half, and it is the whole
        // answer while a session is still local.
        let in_head = match repo.head_tree() {
            Ok(tree) => match tree.lookup_entry_by_path(rela) {
                Ok(entry) => entry.is_some(),
                Err(err) => {
                    tracing::warn!(
                        profile = profile.name,
                        path = %rela.display(),
                        error = %err,
                        "could not look this recording up in the committed tree"
                    );
                    return Ok(durability);
                }
            },
            // An unborn branch is the ordinary state of a session's first
            // minutes, not a failure — the same reading
            // [`git::repo::head_commit_id`] takes.
            Err(err) => {
                tracing::debug!(
                    profile = profile.name,
                    error = %err,
                    "nothing is committed on this branch yet"
                );
                false
            }
        };
        if !in_head {
            return Ok(durability);
        }

        let status = match git::repo::status_paths(&repo) {
            Ok(status) => status,
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    error = %err,
                    "could not read this folder's status, so the recording's durability is \
                     unknown; the last known state stands"
                );
                return Ok(durability);
            }
        };
        // A path the walk had to step over is a path whose state nobody knows,
        // and "unknown" must never round up to "safe" on this surface: the
        // whole question being asked is whether a recording is already durable,
        // and a wrong yes is the answer that loses one. The status succeeded,
        // which is exactly why this has to be checked — before the fix that let
        // it succeed, the failing branch above caught this case for free.
        if status
            .unreadable
            .iter()
            .any(|item| item.path.as_path() == rela)
        {
            tracing::warn!(
                profile = profile.name,
                path = %rela.display(),
                "this file could not be read, so the recording's durability is unknown; \
                 the last known state stands"
            );
            return Ok(durability);
        }

        // A path in `HEAD` that the status walk also names has moved since the
        // commit — staged again, edited again, or deleted — so the commit no
        // longer describes what is on the drive.
        let uncommitted = [
            &status.added,
            &status.modified,
            &status.deleted,
            &status.untracked,
        ]
        .into_iter()
        .any(|bucket| bucket.iter().any(|candidate| candidate == rela));
        durability.committed = !uncommitted;
        if uncommitted {
            return Ok(durability);
        }

        // Nothing has ever been fetched or pushed on this branch, so there is
        // provably no remote copy to be part of.
        let tracking = format!("refs/remotes/origin/{}", profile.branch);
        if repo.find_reference(&tracking).is_err() {
            tracing::debug!(
                profile = profile.name,
                tracking,
                "this branch has never reached the remote, so its commits are here only"
            );
            return Ok(durability);
        }
        match self.has_unpushed_commits(&profile) {
            Ok(unpushed) => durability.pushed = !unpushed,
            Err(err) => tracing::warn!(
                profile = profile.name,
                error = %err,
                "could not tell whether this folder's commits have reached the remote"
            ),
        }
        Ok(durability)
    }

    /// What this engine has done for one profile since it opened.
    ///
    /// See [`EngineCounters`] for why the acceptance of a recording session is
    /// stated in counts at all. A profile the engine has done nothing for
    /// answers all zeroes rather than nothing, because "no commits yet" and
    /// "no such profile" are the same answer to every question a count can be
    /// asked.
    pub fn counters(&self, profile_id: &str) -> EngineCounters {
        Self::lock(&self.counters)
            .get(profile_id)
            .copied()
            .unwrap_or_default()
    }

    fn bump_counters(&self, profile_id: &str, f: impl FnOnce(&mut EngineCounters)) {
        let mut counters = Self::lock(&self.counters);
        f(counters.entry(profile_id.to_owned()).or_default());
    }

    /// Execute every journal unit that is ready for this profile.
    ///
    /// Shared by the supervisor and by `sync_once`: a one-shot sync that
    /// committed a pointer but never ran the LFS transfer it queued would leave
    /// the remote holding a pointer to content it does not have.
    ///
    /// `scan_when_idle` is what separates the two callers. With nothing queued,
    /// the supervisor looks for new local work only when the profile's scan
    /// window is open; an explicit "sync now" always looks, because the user
    /// just asked.
    ///
    /// `source` rides along to whatever gets drained: a journaled push commits
    /// before it publishes, and that commit has to record who asked for it
    /// rather than claiming the watcher did (AD-34-12).
    ///
    /// The caller owns the profile reservation.
    async fn drain_journal(
        &self,
        profile: &SyncProfile,
        scan_when_idle: bool,
        source: SyncSource,
    ) -> Result<()> {
        let now = self.platform.now_ms();
        // Before a single unit is claimed, and on every drain: deferred work
        // waits on a condition rather than a clock, so the condition has to be
        // re-read by whoever is about to do work. See
        // [`Self::release_satisfied_waits`] for the two failures that shape it.
        self.release_satisfied_waits(profile, now)?;
        // Before claiming, because a unit claimed without a name reports the
        // folder for the whole transfer — an hour, on a slow link.
        self.backfill_labels(profile);
        let claimed = self.with_db(|conn| db::claim_ready(conn, &profile.id, now, CLAIM_LIMIT))?;
        if claimed.is_empty() {
            if scan_when_idle {
                self.scan_and_enqueue(profile, source)?;
            }
            return Ok(());
        }

        for item in claimed {
            match self
                .execute(profile, &item.kind, item.id, item.label.as_deref(), source)
                .await
            {
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

    /// Re-read every condition a deferred unit is waiting on, and release the
    /// ones that have been satisfied.
    ///
    /// A deferred unit has nothing that will wake it: `claim_ready` only ever
    /// offers `pending` rows, `recover_running` only rescues `running` ones, and
    /// `enqueue_unique` dedups against `deferred`, so no fresh unit is queued in
    /// its place either. A deferred unit nobody releases is therefore a unit
    /// that never runs again — while [`db::outstanding_count`] keeps counting
    /// it, which holds the push, which stops the folder publishing anything at
    /// all. So the conditions are re-evaluated somewhere that runs
    /// unconditionally, and this is it: once per drain, before any claim.
    ///
    /// Two conditions, and a distinct silent failure behind each:
    ///
    /// * **Every upload has landed** (Story 34.15), which releases a push held
    ///   by [`SyncError::LfsUploadPending`]. This used to be answered only from
    ///   an upload's own completion, across three separate transactions:
    ///   `complete`, then the count, then the release. A kill in that window —
    ///   or a `SQLITE_BUSY` whose `?` propagated out of the drain — left the
    ///   push `deferred` with nothing outstanding and nothing that would ever
    ///   look again, so the folder reported `Syncing` with one pending unit
    ///   forever and the held unit surfaced in no problems view. Asked here it
    ///   costs one indexed `COUNT(*)` per tick and the window cannot exist,
    ///   because the next drain simply asks again.
    /// * **A filesystem remote is reachable again** (Story 34.18). An upload to
    ///   an unmounted remote folder defers as [`SyncError::MediaAbsent`] and
    ///   nothing released it: [`db::undefer_profile`]'s only callers are a
    ///   resume and the volume re-attach arm of [`Self::volume_ready`], and that
    ///   arm returns early unless `profile.removable` — which describes the
    ///   *local* folder's volume, not the remote's. One transient unmount of the
    ///   remote therefore stopped that folder publishing permanently and
    ///   invisibly. The remote's own reachability is the condition, so the
    ///   remote is what gets asked.
    ///
    /// `now_ms` is the drain's own clock reading, which matters: a row released
    /// with `not_before_ms = now` is claimable by the very next statement, so a
    /// satisfied wait costs no extra tick.
    fn release_satisfied_waits(&self, profile: &SyncProfile, now_ms: i64) -> Result<()> {
        // The remote is asked first even though an upload released here is still
        // outstanding and so cannot let the push through in the same breath: the
        // order is what lets one pass handle a remote that came back *and* an
        // upload queue that emptied on an earlier one.
        if let Some(remote) = lfs::local::remote_store(&profile.remote_url) {
            if lfs::local::is_reachable(&remote) {
                let released = self.with_db(|conn| {
                    db::undefer_kind(conn, &profile.id, WorkKind::LFS_UPLOAD, now_ms)
                })?;
                if released > 0 {
                    tracing::info!(
                        profile = profile.name,
                        units = released,
                        "the remote folder is reachable again, so its large files can move"
                    );
                }
            }
        }
        if self.lfs_uploads_outstanding(profile)? == 0 {
            let released =
                self.with_db(|conn| db::undefer_kind(conn, &profile.id, WorkKind::PUSH, now_ms))?;
            if released > 0 {
                tracing::info!(
                    profile = profile.name,
                    units = released,
                    "large files are on the remote, so publishing can go ahead"
                );
            }
        }
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

    /// Record a failed unit or pass: retire the phase it was showing, then let
    /// the retriability decide the state word.
    ///
    /// The [`Self::clear_phase`] call is **before** the match, deliberately. A
    /// leg that raised is a leg that is no longer running, whatever the
    /// retriability says about trying it again, so the arms below decide only
    /// the word — and none of them can be the place a phase is forgotten. All
    /// three arms that set a state used to leave `phase` untouched, which is
    /// exactly how a folder that had permanently stopped publishing kept a
    /// moving bar over its own error message; a fourth arm added here would
    /// have repeated it, because remembering was the whole contract.
    fn record_failure(&self, profile: &SyncProfile, err: &SyncError) {
        self.clear_phase(&profile.id);
        match err.retriability() {
            // Both deferred conditions wait on a condition rather than a clock,
            // but they are waiting on utterly different things and a user reads
            // the state word, not the retriability. `MediaAbsent` renders as
            // "Large files missing", which for a held push would accuse an
            // attached drive of being unplugged — and would send someone hunting
            // for a pendrive while the truth is that keeper is uploading.
            Retriability::Deferred => {
                let state = match err {
                    // ...and a held push is only "syncing" for as long as
                    // something is actually moving. `outstanding_count`
                    // deliberately counts a *parked* upload, so a 403 on one
                    // object — permanent, abandoned, a human's problem — held the
                    // push forever while the profile read `Syncing` and
                    // `is_warning()` stayed false: tray and folder pane both
                    // showed healthy progress for a folder that had permanently
                    // stopped publishing. The parked upload does surface in the
                    // problems view; the state word has to agree with it.
                    //
                    // A count that cannot be read leaves the optimistic answer
                    // standing, the way `refresh_pending` treats its own query: a
                    // momentary `SQLITE_BUSY` is not evidence that anything has
                    // stopped, and the next failure asks again.
                    SyncError::LfsUploadPending { .. } => {
                        match self.lfs_uploads_still_moving(profile) {
                            Ok(0) => ProfileState::NeedsAttention,
                            Ok(_) => ProfileState::Syncing,
                            Err(err) => {
                                tracing::debug!(
                                    profile = profile.name,
                                    error = %err,
                                    "cannot tell whether the held uploads are still moving"
                                );
                                ProfileState::Syncing
                            }
                        }
                    }
                    _ => ProfileState::MediaAbsent,
                };
                self.set_state(&profile.id, state);
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
                    // And say it where the folder is looked at, not only where
                    // notifications go. A run of transient failures used to
                    // leave the card reading whatever it read before —
                    // `Idle`, or a stale `Offline` from the failure that
                    // started it — while the queue retried in a loop nobody
                    // could see. A folder that has failed this many times in a
                    // row is not idle, and the sentence belongs beside it.
                    self.set_state(&profile.id, ProfileState::NeedsAttention);
                    if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
                        snapshot.error = Some(err.to_string());
                    }
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

    /// Refresh both halves of "what is this profile waiting on".
    ///
    /// `pending` counts journal rows and `settling` counts paths the gate is
    /// holding, and they are refreshed together because they are two halves of
    /// one answer: a snapshot that updated one without the other is how "up to
    /// date" came to be printed over a folder still being written (AD-34-10).
    ///
    /// The gate is read, never recounted. An absent gate means nothing has been
    /// walked yet in this process, and the count seeded at open stands until the
    /// first walk measures a real one.
    fn refresh_pending(&self, profile_id: &str) {
        let now = self.platform.now_ms();
        let settling = Self::lock(&self.gates)
            .get(profile_id)
            .map(|gate| gate.tracked(now) as u32);
        if let Ok(pending) = self.pending_for(profile_id) {
            if let Some(snapshot) = Self::lock(&self.status).get_mut(profile_id) {
                snapshot.pending = pending;
                if let Some(settling) = settling {
                    snapshot.settling = settling;
                }
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
        let status = match volume::scan(&profile.local_path, profile.volume_id.as_deref())? {
            // Never bound, and no marker anywhere above the folder: this is the
            // first time keeper has seen this media. Adopting it here is what
            // makes `removable` work at all — the marker exists nowhere else,
            // so without this the profile would report `MediaAbsent` forever
            // and never sync a byte.
            VolumeStatus::Absent if profile.volume_id.is_none() => self.adopt_volume(profile)?,
            other => other,
        };
        match status {
            VolumeStatus::Present { marker } => {
                self.bind_volume(profile, &marker)?;
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

    /// Claim the volume holding this profile's folder, minting
    /// `.keeper-sync/volume.json` if it carries no marker yet.
    ///
    /// Called only for a profile with no `volume_id`, so a volume is adopted
    /// once and never re-adopted. Returns [`VolumeStatus::Absent`] — not an
    /// error — when the folder is not there to be looked at: that is the
    /// ordinary "stick is unplugged and always was" case, and it is also the
    /// guard that keeps a marker off the system disk (see
    /// [`volume::detect_mount_root`]).
    fn adopt_volume(&self, profile: &SyncProfile) -> Result<VolumeStatus> {
        let Some(root) = volume::detect_mount_root(&profile.local_path) else {
            return Ok(VolumeStatus::Absent);
        };
        // A removable volume is, by definition, not the one keeper lives on.
        // Checking that — rather than trying to ask the OS whether a device is
        // ejectable — is both portable and exactly the property that gives the
        // marker its meaning: only a volume that can go away makes "the folder
        // is gone" mean "the media is gone". Without this, marking a folder in
        // the home directory as removable would mint a marker on the system
        // data volume (its own mount on macOS, so the `/` check does not catch
        // it) and bind the profile to the whole machine.
        if let Ok(data_dir) = self.platform.data_dir() {
            if volume::detect_mount_root(&data_dir).as_deref() == Some(root.as_path()) {
                self.warn(
                    &profile.id,
                    &profile.name,
                    format!(
                        "{} is not on a separate volume, so it cannot be treated as removable \
                         media — clear that setting, or point the folder at the drive",
                        profile.local_path.display()
                    ),
                );
                return Ok(VolumeStatus::Absent);
            }
        }
        // The mount point's own name is the label a human already uses for the
        // stick ("merope"); the profile name is only a fallback for a volume
        // mounted somewhere nameless.
        let label = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| profile.name.clone());
        let marker = VolumeMarker::ensure(&root, &label, &profile.id, self.platform.now_ms())?;
        tracing::info!(
            profile = profile.name,
            volume = %marker.volume_id,
            root = %root.display(),
            "adopted the removable volume for this profile"
        );
        Ok(VolumeStatus::Present { marker })
    }

    /// Record which volume this profile lives on, both ways round: the id into
    /// the profile, the profile id into the marker.
    ///
    /// Idempotent, and cheap in the overwhelmingly common case where both sides
    /// already agree — this runs on every tick of every removable profile.
    fn bind_volume(&self, profile: &SyncProfile, marker: &VolumeMarker) -> Result<()> {
        let bound = profile.volume_id.as_deref() == Some(marker.volume_id.as_str());
        let listed = marker.profile_ids.iter().any(|id| id == &profile.id);
        if bound && listed {
            return Ok(());
        }
        let now = self.platform.now_ms();
        if !listed {
            // A second profile on a stick another one already marked: join the
            // existing marker rather than minting a competing one.
            if let Some(root) = volume::find_mount_root(&profile.local_path) {
                VolumeMarker::ensure(&root, &marker.label, &profile.id, now)?;
            }
        }
        if !bound {
            let mut bound_profile = profile.clone();
            bound_profile.volume_id = Some(marker.volume_id.clone());
            self.with_db(|conn| db::upsert_profile(conn, &bound_profile, now))?;
            tracing::info!(
                profile = profile.name,
                volume = %marker.volume_id,
                "bound profile to its removable volume"
            );
        }
        Ok(())
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

    /// `unit_id` is the journal row being executed. The push leg names it as
    /// the unit whose success delivers the activity rows its commit records
    /// (Story 34.16), and the LFS legs carry it down to
    /// [`Self::materialize_landed`], which asks the journal whether anybody is
    /// waiting for that row (Story 56.3).
    ///
    /// The id travels rather than the answer. A request that arrives while its
    /// covering download is already `running` — the ordinary case, since
    /// [`WorkKind::covered_while_running`] makes a running transfer count as
    /// cover — raises the urgency of a row this loop read before the person
    /// asked, so a `bool` copied out at claim time would drop that request
    /// silently. See [`db::unit_urgency`].
    async fn execute(
        &self,
        profile: &SyncProfile,
        kind: &WorkKind,
        unit_id: i64,
        label: Option<&str>,
        source: SyncSource,
    ) -> Result<()> {
        match kind {
            // The conflict copies are already recorded and warned about;
            // a journaled pull has no caller to hand them back to.
            WorkKind::Pull => {
                self.do_pull(profile, source).await?;
                self.mark_synced(profile).await;
                Ok(())
            }
            WorkKind::Push => {
                self.do_push(profile, source, Some(unit_id)).await?;
                self.mark_synced(profile).await;
                Ok(())
            }
            WorkKind::LfsDownload { oid, size } => {
                self.do_lfs(profile, oid, *size, label, false, Some(unit_id))
                    .await
            }
            WorkKind::LfsUpload { oid, size } => {
                self.do_lfs(profile, oid, *size, label, true, Some(unit_id))
                    .await
            }
            WorkKind::OpenPullRequest { branch } => self.do_open_pr(profile, branch).await,
            WorkKind::Verify => self.verify(&profile.id).await.map(drop),
        }
    }

    /// Record that a pass finished without error.
    ///
    /// "Synced" means one whole reconciliation leg came back clean: a pull that
    /// fetched and applied, a push that published — including one that found
    /// nothing to publish — or a complete [`Engine::sync_once`]. It
    /// deliberately does NOT mean "the supervisor ticked": a tick that claimed
    /// no work and was not due a scan checked nothing, and stamping that would
    /// leave the field reading "just now" forever.
    ///
    /// This used to live at the tail of `do_push` alone, so a pull-only
    /// profile — and a push profile whose tree had nothing to send, which
    /// returns early — never recorded that it had succeeded, and the UI could
    /// not tell "never synced" from "synced, nothing to do".
    /// Record that a pass succeeded — and release what that success made
    /// redundant.
    ///
    /// The prune lives HERE, and not at the end of `sync_once`, because
    /// `sync_once` is only one of the three ways a pass finishes. The ordinary
    /// watch-driven flow commits and enqueues a `Push`, which the journal drains
    /// through `execute` — a path that never reaches `sync_once`'s tail. Hooking
    /// the tail meant prune ran only on an explicit "sync now", which is exactly
    /// the manual step it exists to avoid. This function is the single point
    /// every path agrees means "it worked".
    ///
    /// Safe at each of them for the same reason: `do_push` refuses to publish
    /// while an upload is outstanding, so a push that returned has landed its
    /// objects, and the planner independently refuses anything the journal still
    /// references.
    ///
    /// Never fatal. Reclaiming space is housekeeping, and a pass that did
    /// everything asked of it must not report failure because a delete did not.
    /// The release sweep is hooked here for the same reason the prune is, and
    /// carries the same contract: a refusal is not a failure, an error is
    /// logged, and neither can turn a sync that worked into a sync that
    /// reported failure (Story 56.5).
    ///
    /// `async` since Story 56.5, because the sweep's per-object remote proof is
    /// a round trip. The `status` guard is scoped so no `MutexGuard` crosses
    /// the `.await`.
    async fn mark_synced(&self, profile: &SyncProfile) {
        {
            if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
                snapshot.last_sync_ms = Some(self.platform.now_ms());
            }
        }
        if profile.lfs_prune_local {
            if let Err(err) = self.prune_lfs_store(profile) {
                tracing::warn!(
                    profile = profile.name,
                    error = %err,
                    "released no LFS objects this pass",
                );
            }
        }
        if let Err(err) = self.release_expired(profile).await {
            tracing::warn!(
                profile = profile.name,
                error = %err,
                "released no expired content this pass",
            );
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
            match self.open_existing_repo(profile, trust_full) {
                Ok(repo) => {
                    // Not folded into `open_existing_repo`: a failure there is
                    // read as "this folder is not a usable repository" and can
                    // fall through to re-creating it, and a cone that could not
                    // be applied is not that. It must surface as itself.
                    self.reconcile_sparse_cone(profile, &repo)?;
                    return Ok(repo);
                }
                Err(err) => {
                    // A kill inside `gix::init` — the first thing `adopt` does
                    // — leaves a `.git` that exists and is not a repository:
                    // no `HEAD` yet, or no `config`. This branch is then taken
                    // forever *because* `.git` exists, `adopt` is never
                    // reached again, and every sync fails identically until a
                    // human deletes the directory. `discard_unfinished_init`
                    // removes it only when it can prove there is no history in
                    // it, and says so either way; if it refuses, this is a
                    // repository with real content that genuinely will not
                    // open, and that error is the user's answer.
                    if !git::repo::discard_unfinished_init(&git_dir)? {
                        return Err(err);
                    }
                    tracing::warn!(
                        profile = %profile.id,
                        error = %err,
                        "sync: re-creating a repository an interrupted setup left half-made"
                    );
                }
            }
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
                self.credential(profile)?.as_ref(),
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
        git::repo::enforce_local_config_with_filter(
            &repo,
            self.filter_program.as_deref(),
            self.filter_serves_process,
        )?;
        self.reconcile_sparse_cone(profile, &repo)?;
        Ok(repo)
    }

    /// Make the working tree match the profile's `subpaths[]` (Story 27.2,
    /// AD-47).
    ///
    /// Runs on **every** open, not only after a clone. The cone is profile
    /// configuration and the user can change it at any time: before this, a
    /// profile that gained subpaths after its first sync stayed a full checkout
    /// forever, and one that lost them stayed narrow forever, because the only
    /// call site was the branch that clones.
    ///
    /// Three things happen here, in this order, and the order matters.
    ///
    /// 1. The `index.sparse=false` invariant is re-checked *first*, and
    ///    unconditionally — including for a profile with no subpaths at all,
    ///    which is exactly the profile nobody would think to check. See
    ///    [`git::repo::clear_worktree_sparse_override`] for how a `false` in
    ///    `.git/config` stops being the effective value.
    /// 2. The cone is applied, but only when it actually differs from what git
    ///    already has. `sparse-checkout set` re-reads the tree and re-stats the
    ///    working copy; doing that on every sync tick would keep a pendrive
    ///    (AD-48) permanently awake for no change at all.
    /// 3. The invariant is re-checked *after* any mutation, because
    ///    `git sparse-checkout` is the very command that creates the
    ///    worktree-scoped configuration a shadowing `index.sparse` lives in.
    ///    Skipping this would mean the one AC that matters — `gix::status`
    ///    clean immediately after a sparse materialization — is verified
    ///    against a state keeper never actually leaves behind.
    ///
    /// Emptying `subpaths[]` returns the profile to a full checkout rather than
    /// leaving it silently narrow. `sparse-checkout disable` restores the files
    /// it was hiding; it deletes nothing.
    fn reconcile_sparse_cone(&self, profile: &SyncProfile, repo: &gix::Repository) -> Result<()> {
        if git::repo::clear_worktree_sparse_override(repo)? {
            tracing::warn!(
                profile = %profile.id,
                "sync: cleared an index.sparse=true a worktree config was shadowing; \
                 gix cannot read a sparse index"
            );
        }

        let cone = SparseCone::new(&profile.subpaths);
        let applied = git::repo::sparse_patterns(repo)?;
        let mutated = match (&applied, cone.is_full()) {
            // Full checkout wanted, full checkout in place.
            (None, true) => false,
            (Some(_), true) => {
                tracing::info!(
                    profile = profile.name,
                    "restoring the full checkout: this profile no longer names subpaths"
                );
                self.git.sparse_disable(&profile.local_path)?;
                true
            }
            (Some(patterns), false) if cone.is_already_applied(patterns) => false,
            (_, false) => {
                tracing::info!(
                    profile = profile.name,
                    roots = %cone.roots().join(", "),
                    "materializing the profile's cone through the git shim"
                );
                self.git.sparse_set(&profile.local_path, cone.roots())?;
                true
            }
        };

        if mutated && git::repo::clear_worktree_sparse_override(repo)? {
            tracing::warn!(
                profile = %profile.id,
                "sync: the sparse-checkout left index.sparse shadowed; cleared it"
            );
        }
        Ok(())
    }

    /// The `.git`-already-exists path: open it, pin the configuration the
    /// engine depends on, and restore an origin an interrupted setup lost.
    ///
    /// Split out from [`Self::open_repo`] so its caller can tell "this folder
    /// is not a usable repository" from "this folder is not a repository yet",
    /// which are the same symptom and opposite answers.
    fn open_existing_repo(
        &self,
        profile: &SyncProfile,
        trust_full: bool,
    ) -> Result<gix::Repository> {
        let repo = git::repo::open(&profile.local_path, trust_full)?;
        git::repo::enforce_local_config_with_filter(
            &repo,
            self.filter_program.as_deref(),
            self.filter_serves_process,
        )?;
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
        Ok(repo)
    }

    /// This profile's stored access token, if it has one.
    ///
    /// The single read point. Every consumer takes the [`AccessToken`] and
    /// asks it for the shape it needs, so no call site is in a position to
    /// invent a fourth spelling of one secret (see [`crate::credential`]).
    fn token(&self, profile: &SyncProfile) -> Result<Option<AccessToken>> {
        Ok(self
            .platform
            .secret_get(&profile.secret_key())?
            .map(AccessToken::new))
    }

    fn credential(&self, profile: &SyncProfile) -> Result<Option<git::fetch::Credential>> {
        Ok(self.token(profile)?.map(|token| token.git()))
    }

    /// Fold one `(done, total)` report from gitoxide's progress tree into the
    /// `Fetching` event, and time it.
    ///
    /// Named and clock-parameterised rather than left as a closure inside
    /// [`Self::do_pull`] for [`crate::progress::TransferTally::fold_at`]'s
    /// reason: this is one of only two places in the crate that stamp a
    /// `bytes_per_second` (the other is `TransferTally::apply`, which the LFS
    /// legs drive), so `SyncPhase::carries_rate`'s claim that `Fetching` carries
    /// a rate is only worth anything if a test can drive this and watch a figure
    /// appear. Inline in a `spawn_blocking`-fed closure it could not be, and the
    /// stamping line could have been deleted with the suite still green
    /// (Story 34.8, AD-34-13).
    ///
    /// The rate is measured off the high-water mark, never off `done`. `done`
    /// belongs to whichever node ticked last and restarts at zero on each
    /// phase, and its unit is that node's — objects while deltas resolve, not
    /// bytes. The mark is the figure the caller already treats as bytes received
    /// (see its `add_transferred` call), and it stays flat through a phase that
    /// is not moving bytes, which is exactly when the meter should fall silent
    /// rather than divide an object count by a second.
    fn fold_fetch_progress(
        event: &mut SyncProgress,
        (done, total): (u64, u64),
        fetched: &mut u64,
        rate: &mut RateMeter,
        now: Instant,
    ) {
        *fetched = (*fetched).max(done);
        event.bytes_done = done;
        event.bytes_total = (total > 0).then_some(total);
        event.bytes_per_second = rate.observe(*fetched, now);
    }

    /// Fetch and apply, returning the conflict copies the apply had to write.
    ///
    /// The paths are repository-relative and are the *copies*, never the
    /// canonical files. They are returned rather than only counted because
    /// they are the one thing a merge leaves behind that the user has to act
    /// on, and the working tree stops naming them as soon as they are
    /// committed.
    async fn do_pull(&self, profile: &SyncProfile, source: SyncSource) -> Result<Converged> {
        if !profile.direction.pulls() {
            return Ok(Converged::default());
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
        // `None`: this commit exists to give the merge a clean tree, and nothing
        // journaled has promised to publish it. The push that eventually does is
        // a different unit, made after these rows were written.
        self.commit_local(profile, source, None)?;
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
        let mut rate = RateMeter::default();
        let outcome = self
            .publish_while(
                &profile,
                SyncPhase::Fetching,
                // A fetch moves many paths at once and can only honestly name
                // the folder.
                None,
                report_rx,
                |event, sample| {
                    let now = Instant::now();
                    Self::fold_fetch_progress(event, sample, &mut fetched, &mut rate, now);
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
            return Ok(Converged::default());
        };
        if outcome.local_id == Some(remote_id) {
            return Ok(Converged::default());
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
                return Ok(Converged::default());
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
            return Ok(Converged::default());
        }

        // Diverged. A one-way lane stops here because a human deciding is the
        // entire point of a lane (AD-50).
        if profile.lane == SyncLane::Worktree || profile.direction == SyncDirection::PushOnly {
            return Err(SyncError::Diverged {
                profile: profile.name.clone(),
                reason: "the remote branch moved; a human must review this lane".to_owned(),
            });
        }

        let device = self.device();
        let stamp = conflict_stamp(self.platform.now_ms());
        let profile_for_task = profile.clone();
        // A merge is a sync action like any other, and until now it was the one
        // commit in the history that said nothing about who made it. "Which
        // machine resolved this divergence" is exactly the question provenance
        // exists to answer.
        let provenance = Provenance::new(
            &profile.name,
            &device.label,
            &device.id,
            self.platform.host_label(),
            Self::commit_source(&profile, source),
        )
        .with_tags(profile.tags.clone());
        let converged = tokio::task::spawn_blocking(move || {
            Self::converge_with_conflict_copies(
                &git,
                &profile_for_task,
                &tracking,
                &stamp,
                &device.label,
                &provenance,
            )
        })
        .await
        .map_err(|err| SyncError::Journal(format!("converge task failed: {err}")))??;

        if !converged.stale.is_empty() {
            // Nothing is on disk to act on, so this is a prompt rather than a
            // work item: the paths are declared regenerable, the remote's
            // revision won, and the tool that writes them has to run again
            // before they describe this device. Saying it here is the only
            // chance anybody gets — no later scan can tell a generated file
            // that is current from one that is not.
            self.warn(
                &profile.id,
                &profile.name,
                format!(
                    "{} generated file(s) took the remote's version — regenerate them",
                    converged.stale.len()
                ),
            );
        }

        if converged.copies.is_empty() {
            tracing::info!(profile = profile.name, "merged diverged history cleanly");
        } else {
            // Non-blocking by contract: both revisions survive, so there is
            // nothing for the user to decide before syncing continues.
            self.warn(
                &profile.id,
                &profile.name,
                format!(
                    "{} file(s) changed on both sides — your version was kept alongside as .sync-conflict-…",
                    converged.copies.len()
                ),
            );
            // Until now the warning counted the copies and nothing named them,
            // so the one artifact the user has to deal with was unfindable
            // once the notification was dismissed.
            let rows: Vec<db::ActivityEntry> = converged
                .copies
                .iter()
                .map(|path| {
                    // Unlike a deletion, the copy is sitting in the folder
                    // right now, so its size is simply readable — and this is
                    // the one row a user has to act on, so how much of their
                    // disk it is holding is worth saying.
                    let size = std::fs::metadata(profile.local_path.join(path))
                        .ok()
                        .map(|md| md.len());
                    db::ActivityEntry {
                        kind: ActivityKind::Conflict,
                        path: path.clone(),
                        size_bytes: size,
                        // A conflict copy is a file the merge just wrote into
                        // the worktree. No unit has promised to deliver it —
                        // the commit that will is not made yet — so it reports
                        // no delivery rather than inheriting the pull's.
                        unit_id: None,
                    }
                })
                .collect();
            let now = self.platform.now_ms();
            self.with_db(|conn| db::record_activity(conn, &profile.id, now, &rows))?;
        }
        Ok(converged)
    }

    /// Converge a diverged branch without asking anyone (AD-43).
    ///
    /// The remote keeps the canonical path and the local revision is preserved
    /// beside it, so the `-X theirs` merge that follows can never lose content:
    /// by the time it runs, every contested file already exists twice.
    ///
    /// **Changing the same path is not the same as disagreeing about it.** Two
    /// devices that ran the same formatter, or regenerated the same
    /// deterministic listing, both show the path in their diff against the
    /// merge base — and a copy made for that is pure litter: it is byte-identical
    /// to the file it sits beside, and the user has to open both to find that
    /// out. So the contested set is narrowed by one more question, one process
    /// rather than one per path: does `HEAD` actually differ from the remote tip
    /// *here*? A path that survives both filters is a real disagreement.
    ///
    /// [`SyncProfile::regenerable`] removes the remaining case, where the
    /// content genuinely differs but the local revision is worth nothing —
    /// a generated index listing two different new files. Only paths the user
    /// declared get that treatment; keeper never guesses that an edit is
    /// disposable.
    ///
    /// Blocking.
    fn converge_with_conflict_copies(
        git: &GitCli,
        profile: &SyncProfile,
        tracking: &str,
        stamp: &str,
        device: &str,
        provenance: &Provenance,
    ) -> Result<Converged> {
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
        // Tip against tip: what the two sides actually hold different bytes for.
        let differing: std::collections::HashSet<PathBuf> = git
            .diff_names(repo_path, "HEAD", tracking)?
            .into_iter()
            .collect();
        // Compiled here rather than per profile: divergence is the rare path,
        // and a set nobody configured costs one `is_empty` to skip.
        let regenerable = crate::exclude::PatternSet::new(&profile.regenerable)?;

        let mut converged = Converged::default();
        for rela in ours.intersection(&theirs) {
            // Both sides moved, and landed on the same content: there is
            // nothing to preserve and nothing to tell the user about.
            if !differing.contains(rela.as_path()) {
                continue;
            }
            if !regenerable.is_empty() && regenerable.matches(rela) {
                converged
                    .stale
                    .push(rela.to_string_lossy().replace('\\', "/"));
                continue;
            }
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
            converged.copies.push(
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
        // Sorted so a run is reportable in a stable order: a set iteration is
        // deliberately unordered and two devices comparing notes on the same
        // divergence should read the same list.
        converged.copies.sort();
        converged.stale.sort();
        Ok(converged)
    }

    /// The last repair-backlog probe for this profile, if it is still fresh.
    ///
    /// `None` means "ask again", not "there is no backlog" — the two are
    /// different answers and conflating them would make a poll during the TTL
    /// claim a folder is clean.
    fn remembered_repair(&self, profile_id: &str) -> Option<Vec<PathBuf>> {
        Self::lock(&self.repair_memo)
            .get(profile_id)
            .filter(|(taken, _)| taken.elapsed() < REPAIR_MEMO_TTL)
            .map(|(_, found)| found.clone())
    }

    /// Remember a probe, including an empty one: "this folder has no backlog"
    /// is exactly as expensive to establish as the opposite and exactly as
    /// worth not re-establishing five seconds later.
    fn remember_repair(&self, profile_id: &str, found: &[PathBuf]) {
        Self::lock(&self.repair_memo)
            .insert(profile_id.to_owned(), (Instant::now(), found.to_vec()));
    }

    /// A bounded batch of paths whose committed blob contradicts their
    /// attributes, or `None` when there are none.
    ///
    /// `Some` means "commit this instead of walking": these paths are modified
    /// by definition — the filter's output cannot equal a raw blob — so asking
    /// the filesystem to confirm it costs a read and a filter round trip each
    /// and answers what the index already said. See
    /// [`lfs::stage::mismatched_filtered_paths`].
    ///
    /// The stability gate is not consulted, and that is correct rather than a
    /// shortcut: the gate exists to hold a file whose *writer* has not
    /// finished, and nothing here is about the file's bytes changing. What is
    /// wrong is the record, and the record is not being written to.
    fn repair_recorded_pointers(
        &self,
        profile: &SyncProfile,
    ) -> Result<Option<git::commit::StagedChange>> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(None);
        }
        let repo = self.open_repo(profile)?;
        let tracked = git::repo::tracked_paths(&repo)?;
        let from = Self::lock(&self.repair_cursor)
            .get(&profile.id)
            .copied()
            .unwrap_or(0);
        let (found, next) = lfs::stage::mismatched_filtered_paths(
            &repo,
            &tracked,
            from,
            REPAIR_WINDOW,
            REPAIR_BATCH,
        );
        Self::lock(&self.repair_cursor).insert(profile.id.clone(), next);
        if found.is_empty() {
            return Ok(None);
        }
        // Said once per batch rather than once per path: a folder in this state
        // has tens of thousands, and the useful facts are how many are left and
        // that something is being done about it.
        tracing::info!(
            profile = profile.name,
            batch = found.len(),
            "these files are committed as plain blobs although this folder's rules filter them; \
             rewriting them as pointers so the next scan does not have to read them again"
        );
        Ok(Some(git::commit::StagedChange {
            modified: found,
            ..Default::default()
        }))
    }

    /// Scan, gate and commit whatever settled. Returns how many paths landed.
    ///
    /// Shared by both legs so the working tree is always clean before a merge
    /// and always current before a push.
    ///
    /// `push_unit` is forwarded to [`Self::commit`]; see its docs for what
    /// naming it buys and why `None` is a legitimate answer.
    fn commit_local(
        &self,
        profile: &SyncProfile,
        source: SyncSource,
        push_unit: Option<i64>,
    ) -> Result<u64> {
        if !profile.direction.pushes() {
            // A pull-only profile never commits; a local edit there is
            // preserved by the merge's own conflict handling instead.
            return Ok(0);
        }
        // Before the walk, not after it: a path whose recorded blob contradicts
        // its own attributes is already known to be work, and the walk is the
        // most expensive possible way to rediscover it. Each one costs a full
        // read and a filter round trip *per walk* until it is committed, which
        // is what turned this folder's 84-second stat floor into 25 to 74
        // minutes. The index knows the answer for nothing.
        let staged = match self.repair_recorded_pointers(profile)? {
            Some(repair) => repair,
            None => self.collect_stable_changes(profile)?,
        };
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
        self.commit(profile, &staged, source, push_unit)?;
        self.bump_counters(&profile.id, |counters| counters.commits += 1);
        // The commit is durable, so every staged path landed. Reporting it
        // leaves the bar full rather than stranded mid-way when the phase
        // changes underneath it.
        let mut event = self.progress(profile, SyncPhase::Committing);
        event.files_total = Some(count);
        event.files_done = count;
        self.publish(event);
        Ok(count)
    }

    /// Are this profile's committed pointers still waiting on their objects?
    ///
    /// The precondition on publishing anything (Story 34.15). A pointer names
    /// content by digest and carries none of it, so pushing a commit whose
    /// objects are not on the server yet produces the one failure nobody
    /// observes: git accepts the push, the remote is "up to date", and the next
    /// peer to clone checks out a tree of ~130-byte text stubs with no error
    /// anywhere. `keeper-sync`'s own `.git/lfs` still holds the bytes, so
    /// nothing is lost — but only the machine that made the commit can supply
    /// them, which is exactly the property sync exists to remove.
    ///
    /// Counted from the journal rather than by asking the server: the units are
    /// the record of what has not landed, they are durable across a restart, and
    /// the answer costs one indexed `COUNT(*)` on every push.
    fn lfs_uploads_outstanding(&self, profile: &SyncProfile) -> Result<u32> {
        self.with_db(|conn| db::outstanding_count(conn, &profile.id, WorkKind::LFS_UPLOAD))
    }

    /// Of the uploads this profile owes, how many are still being attempted?
    ///
    /// [`Self::lfs_uploads_outstanding`] counts a parked upload, and must: it
    /// answers whether publishing is safe, and an abandoned upload is the
    /// clearest possible "no". But the *state* the profile reports is a
    /// different question with the opposite answer, because a push held behind
    /// an abandoned upload is not in progress — it is stopped, and nothing will
    /// restart it but a human. Told apart here rather than at the call site so
    /// there is one place that knows the two counts differ and why.
    fn lfs_uploads_still_moving(&self, profile: &SyncProfile) -> Result<u32> {
        self.with_db(|conn| db::live_count(conn, &profile.id, WorkKind::LFS_UPLOAD))
    }

    /// `push_unit` is this push's own journal row, when a journaled unit is
    /// driving. It is handed to the commit so the rows it records can name the
    /// work that will publish them (Story 34.16).
    /// Push once, as the profile's own credential.
    fn push_once(
        &self,
        profile: &SyncProfile,
        refspec: &str,
    ) -> impl std::future::Future<Output = Result<()>> + Send {
        let git = self.git.clone();
        let repo_path = profile.local_path.clone();
        let refspec = refspec.to_owned();
        let credential = self.credential(profile);
        async move {
            let credential = credential?;
            tokio::task::spawn_blocking(move || {
                git.push(&repo_path, "origin", &refspec, credential.as_ref())
            })
            .await
            .map_err(|err| SyncError::Journal(format!("push task failed: {err}")))?
        }
    }

    /// What a refused push means for *this* profile (DW-207).
    ///
    /// git says the same thing however the folder is configured — "! [rejected]
    /// … (fetch first)", "failed to push some refs" — and
    /// [`git::cli::classify_message`] can only read that text, so it answers
    /// [`SyncError::Diverged`]: permanent, and needing a human. On a one-way
    /// lane that is exactly right, because a human deciding is the point
    /// (AD-50).
    ///
    /// On a **bidirectional profile sharing a branch** it is wrong. Being
    /// overtaken between the merge and the push is what two machines syncing
    /// one branch *do* — the other one pushed first, which is the entire point
    /// of sharing it — and the answer is the reconcile this engine already
    /// owns.
    ///
    /// # Why the reconcile happens HERE, inline
    ///
    /// The first cut of this queued a `Pull` and returned a transient error,
    /// leaving the retry to the scheduler. Measured in the field, that spun:
    /// **613 rejected pushes in 135 seconds**, ~4.5 per second, before the
    /// queued reconcile finally got its turn. It converged — the merge ran, the
    /// commits landed — but it hammered the forge six hundred times to do it
    /// and burned the profile's failure counter on the way, so a folder that
    /// was fixing itself told its owner "sync has failed 6 times in a row".
    ///
    /// The cause is queue ordering: every unpushed commit has queued its own
    /// `Push` unit, `claim_ready` takes them in front of a `Pull` enqueued
    /// afterwards, and each one meets the same unmoved remote. Fetching and
    /// merging *in this unit*, then retrying *this* push, removes the ordering
    /// question entirely: one reconcile, one retry, and the ordinary case ends
    /// with nothing surfaced at all — which is what a race between two machines
    /// deserves.
    ///
    /// Bounded on purpose: exactly one reconcile and one retry. If the remote
    /// moved *again* in that window the error comes back transient, the
    /// scheduler backs off, and the next pass tries the whole thing once more.
    async fn reconcile_and_retry_push(
        &self,
        profile: &SyncProfile,
        source: SyncSource,
        refspec: &str,
        err: SyncError,
    ) -> Result<()> {
        let SyncError::Diverged { reason, .. } = &err else {
            return Err(err);
        };
        // A lane, or a one-way profile: the remote moving is a decision, and
        // the human who owns the lane is the one who takes it.
        if profile.lane == SyncLane::Worktree || profile.direction == SyncDirection::PushOnly {
            return Err(err);
        }
        let reason = reason.clone();
        tracing::info!(
            profile = profile.name,
            %reason,
            "the remote moved while pushing; reconciling"
        );

        // Fetch and merge. `do_pull` commits settled work first, which is what
        // lets the merge run at all, and it never pushes — so this cannot
        // recurse back into here.
        self.do_pull(profile, source).await?;

        match self.push_once(profile, refspec).await {
            Ok(()) => {
                tracing::info!(
                    profile = profile.name,
                    "the reconcile landed and the push went through"
                );
                Ok(())
            }
            // Overtaken a second time inside one pass, or refused for a reason
            // the reconcile cannot address. Transient either way: the scheduler
            // backs off and the next pass reconciles again from scratch.
            Err(again) => {
                tracing::warn!(
                    profile = profile.name,
                    error = %again,
                    "the push was still refused after reconciling"
                );
                Err(SyncError::RemoteMoved {
                    profile: profile.name.clone(),
                    reason,
                })
            }
        }
    }

    async fn do_push(
        &self,
        profile: &SyncProfile,
        source: SyncSource,
        push_unit: Option<i64>,
    ) -> Result<()> {
        if !profile.direction.pushes() {
            return Ok(());
        }
        let count = self.commit_local(profile, source, push_unit)?;

        // After the commit, because the commit is what creates the debt: a
        // freshly staged large file queues its upload inside `commit_local`
        // above, and that upload must land before this push publishes the
        // pointer naming it. Before the `head_commit_id` check below, because a
        // profile whose only commits are held ones has nothing it may publish
        // regardless of what git would accept.
        let outstanding = self.lfs_uploads_outstanding(profile)?;
        if outstanding > 0 {
            tracing::info!(
                profile = profile.name,
                objects = outstanding,
                "holding the push until this folder's large files reach the remote"
            );
            return Err(SyncError::LfsUploadPending {
                objects: outstanding,
            });
        }

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

        // A lane publishes ONLY its own branch. The base branch is never in
        // the refspec, so pushing over a human's work is impossible by
        // construction rather than by care (AD-50).
        let working = Self::working_branch(profile);
        let refspec = format!("refs/heads/{working}:refs/heads/{working}");
        // Counted where the invocation is, not where it returns: an attempt
        // against an unreachable remote is still this policy firing, and a
        // session that claims "one push at the end" has to be able to show it
        // even when that push failed (Story 41.5).
        self.bump_counters(&profile.id, |counters| counters.pushes += 1);
        let pushed = self.push_once(profile, &refspec).await;
        if let Err(err) = pushed {
            self.reconcile_and_retry_push(profile, source, &refspec, err)
                .await?;
        }

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
    /// file, so git decides what is ignored. A **nested repository** still
    /// arrives collapsed and always will — its files belong to that repository,
    /// not to this one, and git will not list them — so the refusal is a
    /// standing property of the tree rather than an anomaly. It is returned
    /// rather than logged here: this is called on both the commit walk and
    /// every UI poll, and logging inside it reported the same folders on every
    /// pass. [`Self::report_collapsed`] says each one once.
    fn expand_untracked(root: &Path, entries: &[PathBuf]) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
        let mut out = Vec::with_capacity(entries.len());
        let mut collapsed = Vec::new();
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
                collapsed.push(rela.clone());
                continue;
            }
            out.push(rela.clone());
        }
        out.sort();
        Ok((out, collapsed))
    }

    /// Warn about each collapsed untracked directory once per profile.
    ///
    /// Once, because the condition does not change between walks and the folder
    /// is not going to start syncing: the honest report is a fact stated on
    /// discovery, not a metronome. The set is never pruned — a repository that
    /// stops being nested becomes an ordinary file entry that never reaches
    /// here again, and a few dozen remembered paths cost less than one line of
    /// the log they replace.
    fn report_collapsed(&self, profile_id: &str, collapsed: &[PathBuf]) {
        if collapsed.is_empty() {
            return;
        }
        let mut reported = Self::lock(&self.collapsed_reported);
        for rela in collapsed {
            if reported.insert((profile_id.to_owned(), rela.clone())) {
                tracing::warn!(
                    path = %rela.display(),
                    "sync: skipping an untracked directory git reported collapsed — it is its own \
                     repository, so its files are not this folder's to commit, and walking it here \
                     would ignore .gitignore"
                );
            }
        }
    }

    /// Raise the paths this pass could not read, if any.
    ///
    /// These need a human — a permission has to be restored, a file has to be
    /// released by whatever holds it, or the path has to be excluded — so this
    /// goes to the warning surface rather than the log alone. The engine keeps
    /// converging everything else meanwhile, which is the whole point of
    /// stepping over them (FR-89: convergence never waits on a prompt).
    ///
    /// One profile carries one warning string, so a second unreadable path
    /// cannot have its own line. It is counted rather than dropped: "and 3
    /// more" is the difference between a user who knows to keep looking and one
    /// who fixes a single file and assumes they are done.
    fn report_unreadable(&self, profile: &SyncProfile, unreadable: &[git::repo::UnreadablePath]) {
        let Some(first) = unreadable.first() else {
            return;
        };
        let others = match unreadable.len() - 1 {
            0 => String::new(),
            rest => format!(", and {rest} more"),
        };
        self.warn(
            &profile.id,
            &profile.name,
            format!(
                "could not read {first}{others} — everything else in this folder synchronized \
                 without it; restore access to the file or exclude it",
            ),
        );
    }

    /// Walk the working tree and keep only what the completeness gate passes.
    ///
    /// Blocking, and deliberately synchronous: it is pure filesystem work with
    /// no await points, so it holds no lock across a suspension.
    ///
    /// Publishes `Scanning` only once the walk knows there is work (AD-34-1).
    /// It used to publish on entry, and [`Self::publish`] promotes any active
    /// phase to [`ProfileState::Syncing`], so a walk that staged nothing still
    /// left the snapshot claiming the profile was busy until `refresh_pending`
    /// cleared it at the end of the same tick. The tray samples that snapshot on
    /// its own ~1 Hz timer, caught the window at random, and then held the busy
    /// glyph for three more ticks — which is how a folder where nothing was
    /// happening read as active for four seconds at a time.
    ///
    /// Announcing on entry and retracting at the end would not fix that: the
    /// window the tray can sample would be exactly as wide as it is now. So the
    /// report is deferred instead, and the trade is deliberate — a slow walk is
    /// silent until it knows whether it has anything to say, where before it was
    /// busy from the first instant. Work is never left unannounced: a walk that
    /// finds something reports it here, before the caller publishes `Committing`
    /// or enqueues the push.
    fn collect_stable_changes(&self, profile: &SyncProfile) -> Result<git::commit::StagedChange> {
        let repo = self.open_repo(profile)?;
        // One walk per folder. If a Pending poll is already inside one, this
        // pass has nothing to add by starting a second: it would read the same
        // tree, reach the same verdicts, and halve the throughput of the walk
        // already running. Staging nothing is an outcome this caller already
        // handles on every quiet pass, and the supervisor's next tick retries.
        let Some(_claim) = self.claim_walk(&profile.id) else {
            tracing::info!(
                profile = %profile.name,
                "a status walk is already in flight; deferring this pass to the next tick"
            );
            return Ok(git::commit::StagedChange::default());
        };
        // A walk that takes longer than a second says so, on the surface the
        // owner is looking at. Before this, the one step that can run for ten
        // minutes on a 155 000-file folder published nothing at all, so the pane
        // read `Idle - N waiting to sync` for the whole of it and there was no
        // way to tell work from a wedge. Fast folders still report nothing: the
        // pacing lives in `git::repo`, and DW-116's flipping glyph stays fixed.
        let status = {
            let report = |scanned: u64, entries: u64| {
                let mut event = self.progress(profile, SyncPhase::Scanning);
                event.files_done = scanned;
                // Both counts are index entries now (see [`git::repo::WalkReport`]),
                // so the numerator can no longer overrun the denominator and
                // the bar cannot render past its end. A zero denominator is
                // still not a denominator: an index the walk could not read
                // bounds nothing, and the count alone is the honest answer.
                event.files_total = (entries > 0).then_some(entries);
                self.publish(event);
            };
            // The directory scan is this leg's biggest single cost and it is not
            // proportional to what changed: `find . -type f` over this folder's
            // 157 490 files is 996 s, and a pass that spent it reported
            // `elapsed_ms=9299355` — two and a half hours — for twelve modified
            // files. `commit_walk_policy` spends it on the watcher's word, with
            // no watcher to trust, or on the sweep cadence.
            git::repo::status_paths_reported(
                &repo,
                Some(&report),
                self.walk_report_interval,
                self.commit_walk_policy(profile),
            )?
        };
        // The pass only got this far because those paths were stepped over, and
        // a folder that syncs while quietly omitting a file is the one outcome
        // worse than a folder that stops: nobody looks for what they were not
        // told about. Raised here rather than in `git::repo` because this is the
        // layer that knows which profile is speaking.
        self.report_unreadable(profile, &status.unreadable);
        let now = self.platform.now_ms();

        let mut gates = Self::lock(&self.gates);
        let gate = self.ensure_gate(&mut gates, profile)?;

        let mut staged = git::commit::StagedChange::default();
        let mut held = 0u32;
        // `added` and `untracked` both land in the same bucket, so the buckets
        // are collected first and moved into `staged` afterwards — taking two
        // simultaneous `&mut` borrows of one field would not compile.
        let mut new_paths: Vec<PathBuf> = Vec::new();
        let mut changed_paths: Vec<PathBuf> = Vec::new();
        // gitoxide's dirwalk reports untracked content file by file, but a
        // nested repository still arrives as ONE entry naming the directory.
        // The commit path can only stage regular files and symlinks, so those
        // are separated out here — otherwise every one of them raises "only
        // regular files and symlinks can be synchronized" forever.
        let (untracked, collapsed) =
            Self::expand_untracked(&profile.local_path, &status.untracked)?;
        self.report_collapsed(&profile.id, &collapsed);
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
                        } else if let Some(bytes) =
                            lfs::stage::truncated_media(&repo, rela, &absolute)
                        {
                            // Not an edit — an accident, and one whose commit
                            // is unrecoverable: cleaning an empty file emits a
                            // pointer to nothing, which then replaces the only
                            // reference every peer has to the real object
                            // (DW-140). Held out of the staged set and named
                            // out loud instead.
                            self.warn(
                                &profile.id,
                                &profile.name,
                                format!(
                                    "{} is empty but should hold {bytes} bytes — not committing it; restore it from the remote before syncing this folder",
                                    rela.display()
                                ),
                            );
                        } else if !lfs::stage::is_false_modification(&repo, rela, &absolute) {
                            // An LFS-tracked path whose entry is racily clean
                            // reports the worktree's bytes as differing from
                            // its pointer blob. That is not an edit, and
                            // staging it again would re-hash the whole file
                            // only to write back the pointer it already has.
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

        // How big each of those paths is, measured here because nothing after
        // this point can: `stage_and_commit` folds them into a tree, the
        // working tree goes clean, and the gate has already dropped the sample
        // it took a moment ago. One `lstat` on a file whose every byte is
        // about to be read costs nothing next to the read, and a file that
        // vanished in between simply has no size to record. It waits until
        // here on purpose: every other profile's scan queues behind the gate
        // lock, and this walk has no business holding it.
        for rela in staged.added.iter().chain(staged.modified.iter()) {
            if let Ok(Some(sample)) = FileSample::of(&profile.local_path.join(rela)) {
                staged.sizes.insert(rela.clone(), sample.size);
            }
        }
        for rela in &staged.deleted {
            // A deleted path cannot be stat'd, so the index is the only place
            // left to ask — and it answers an LFS entry with the size the
            // pointer names rather than the pointer's own ~130 bytes. This used
            // to be `Self::removed_size`, a second inlined copy of exactly that
            // derivation; `lfs::stage::indexed_size` is now the only one
            // (Story 56.2).
            if let Some(size) = lfs::stage::indexed_size(&repo, rela) {
                staged.sizes.insert(rela.clone(), size);
            }
        }

        // The walk is the only thing that knows this number, and it used to
        // throw it into a `debug!` and forget it. `pending` counts journal rows,
        // so a folder where five thousand files are mid-write has none of them,
        // reports `pending = 0`, and printed "up to date" over itself for as
        // long as the writing lasted (AD-34-10).
        //
        // Written even when zero: a walk that finds nothing settling is exactly
        // what retires the previous claim. It is also the more honest of the two
        // counts — `StabilityGate::tracked`, which [`Self::refresh_pending`]
        // reads between walks, cannot see a path held because its `lstat`
        // failed, because no entry is recorded for one.
        if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
            snapshot.settling = held;
        }
        if held > 0 {
            tracing::debug!(profile = profile.name, held, "files still settling");
        }
        // The deferred report (AD-34-1): the count is the one thing the walk
        // learned, and it is what makes the phase legible rather than a bare
        // "Scanning".
        if !staged.is_empty() {
            let mut event = self.progress(profile, SyncPhase::Scanning);
            event.files_total = Some(staged.len() as u64);
            self.publish(event);
        }
        Ok(staged)
    }

    /// The source recorded in a commit's trailers.
    ///
    /// Straight through, with one substitution: on a worktree lane the working
    /// copy belongs to an autonomous agent (AD-50), so a pass the supervisor
    /// started there is publishing a bot's work and `Bot` is the honest answer.
    /// An explicit `Manual` or `Cli` request is left alone even on a lane — a
    /// person really did ask for that commit to exist, which is precisely what
    /// `Keeper-Source` records.
    fn commit_source(profile: &SyncProfile, requested: SyncSource) -> SyncSource {
        match requested {
            SyncSource::Watch if profile.lane == SyncLane::Worktree => SyncSource::Bot,
            other => other,
        }
    }

    /// `push_unit` is the journal unit that will publish this commit, when one
    /// is driving: the activity rows for every path that is *not* routed through
    /// LFS name it as the unit whose success delivers them (Story 34.16). `None`
    /// where no journaled unit owns the publication — a `sync_once` running the
    /// legs inline, or the commit `do_pull` makes to clear the tree before a
    /// merge — and those rows honestly read as `DeliveryState::Unknown` rather
    /// than borrowing someone else's verdict.
    fn commit(
        &self,
        profile: &SyncProfile,
        staged: &git::commit::StagedChange,
        source: SyncSource,
        push_unit: Option<i64>,
    ) -> Result<()> {
        let repo = self.open_repo(profile)?;
        let device = self.device();
        let (name, email) = git::commit::author_for(profile, &device);
        let when = gix::date::Time::new(self.platform.now_ms() / 1_000, 0);
        let author = gix::actor::Signature {
            name: name.into(),
            email: email.into(),
            time: when,
        };
        let provenance = Provenance::new(
            &profile.name,
            &device.label,
            &device.id,
            self.platform.host_label(),
            Self::commit_source(profile, source),
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
        let staging = lfs::stage::prepare(&repo, profile, &store, &candidates)?;

        // The denominator for staging progress, read before the augmentation
        // below: it is the same count `commit_local` has already published, and
        // a counter that overshot its own denominator would read as a bug.
        let files_total = staged.len() as u64;

        // A changed `.gitattributes` must land in the SAME commit as the files
        // it governs, or a peer cloning that commit would not know the pointers
        // are pointers.
        let mut staged = staged.clone();
        if staging.attributes_changed {
            self.bump_counters(&profile.id, |counters| counters.attribute_writes += 1);
            let attributes = PathBuf::from(".gitattributes");
            if !staged.added.contains(&attributes) && !staged.modified.contains(&attributes) {
                staged.added.push(attributes);
            }
        }

        // Per-file staging progress (AD-34-13). Before this, `files_done` jumped
        // 0 -> total in one step and `current` was set once to the first staged
        // path and never revised, so a long commit looked stuck on file one.
        // Always `true`: `publish` drops its own dead subscribers, so the engine
        // itself is never the party that has stopped listening.
        let staging_progress = |files_done: u64, current: &Path| -> bool {
            // `commit_local`, this function's only caller, already published the
            // first path entering the phase — it has to, because `prepare` above
            // can spend real time hashing before staging starts. Publishing that
            // frame again here would be byte-identical noise, which AD-34-1 says
            // must not reach the tray.
            if files_done == 0 {
                return true;
            }
            let mut event = self.progress(profile, SyncPhase::Committing);
            event.files_total = Some(files_total);
            event.files_done = files_done.min(files_total);
            event.current = Some(current.to_string_lossy().into_owned());
            self.publish(event);
            true
        };

        // Journal the upload debt BEFORE the commit that creates it.
        //
        // This used to run after `stage_and_commit`, on the reasoning that a
        // crash before the commit "loses nothing" — which had the direction of
        // the risk exactly backwards. `stage_and_commit` writes the commit and
        // moves the ref, so the pointer is durable the instant it returns,
        // while these rows are one separate SQLite transaction each. The
        // Story 34.15 gate reads ONLY the journal
        // ([`Self::lfs_uploads_outstanding`]) and nothing re-derives the debt
        // from committed-but-unpushed pointers, so a kill anywhere between the
        // ref update and object N's INSERT lost the obligation outright: the
        // next scan queues a push, the count comes back 0, and the push
        // publishes a pointer whose bytes exist on this machine alone — exactly
        // the silent failure the gate exists to prevent, through a window that
        // was N transactions wide for an N-object commit.
        //
        // Enqueueing first inverts that failure into the harmless direction.
        // `lfs::stage::prepare` above has already written every one of these
        // objects into the local store, so a crash after these rows and before
        // the commit leaves an upload for content nothing references yet: it
        // runs, the remote gains an object no tree names, and the commit is
        // re-driven from the worktree on the next pass. An unreferenced object
        // on the remote costs disk; a lost obligation costs the file.
        //
        // The activity rows still go last, after the commit: each one names the
        // unit that has to deliver it, and those ids do not exist until here.
        let now = self.platform.now_ms();
        let mut lfs_units: HashMap<PathBuf, i64> = HashMap::with_capacity(staging.uploads.len());
        for object in &staging.uploads {
            let unit = WorkKind::LfsUpload {
                oid: object.oid.clone(),
                size: object.size,
            };
            let unit_id =
                self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now))?;
            // git's spelling, the frame the ledger and the release sweep both
            // key on. The label is what lets the completion edge below know
            // which path this unit confirms (Story 56.5).
            let key = lfs::stage::index_key(&object.path);
            self.with_db(|conn| db::label_unit(conn, unit_id, &key))?;
            // The provenance memo does NOT go here. It is a statement about a
            // ledger row, and it is written once `stage_and_commit` has proved a
            // commit exists — see below. These journal rows DO go here, for the
            // reason the long comment above gives, and that reason is unchanged:
            // an unreferenced object on the remote costs disk, a lost upload
            // obligation costs the file.
            lfs_units.insert(object.path.clone(), unit_id);
        }

        let Some(id) = git::commit::stage_and_commit(
            &repo,
            &staged,
            &provenance,
            profile,
            &author,
            &staging.substitutions,
            Some(&staging_progress),
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

        // The provenance memo, here and not in the enqueue loop above (Story
        // 56.5, FR-341, AD-131). This clone produced these bytes, so the
        // local-origin clock applies and the path is not eligible for release at
        // any age until the remote confirms it — and `note_local_authorship`
        // also clears any `synced_at_ms`, because the new bytes are not the
        // bytes upstream holds.
        //
        // Which is exactly why it may not run before the commit exists.
        // `stage_and_commit` can return `Err` — an `index.lock`, ENOSPC, a git
        // failure — or `Ok(None)`, meaning every staged path turned out
        // byte-identical to `HEAD` and nothing was committed at all. Written
        // first, a byte-identical rewrite of a path that ARRIVED from the remote
        // flipped it to `local_origin = 1` and erased a correct `synced_at_ms`,
        // making remote-origin content unreleasable until a now-pointless upload
        // unit drained — which offline is never. On any folder whose files are
        // being touched, the sweep then quietly reclaimed nothing.
        //
        // Read from `staging.uploads` rather than through `lfs_units`, whose
        // value is a unit id: the ledger wants the object's identity, and both
        // collections are built from that one list in that one order.
        for object in &staging.uploads {
            let key = lfs::stage::index_key(&object.path);
            self.with_db(|conn| {
                db::note_local_authorship(conn, &profile.id, &key, now, &object.oid, object.size)
            })?;
        }

        // The commit is proven to exist, so this is the first moment an
        // activity row can honestly claim it. Recording in `commit_local`
        // instead would also cover the `stage_and_commit` → `None` case, where
        // every staged path turned out byte-identical to `HEAD` and nothing
        // was committed at all.
        //
        // `staged` is the local, `.gitattributes`-augmented copy on purpose:
        // it is exactly the set of paths this commit changed.
        self.record_commit_activity(profile, &staged, &lfs_units, push_unit)
    }

    /// Write the recently-synced entries one commit produced (Story 32.1).
    ///
    /// The commit path is the only place that knows *which* paths moved:
    /// `commit_local` reduces the whole `StagedChange` to a count, and by the
    /// time the push leg runs the working tree is clean again and the
    /// information is gone. Recording here is what turns "3 files synced" into
    /// a list a user can actually recognise their work in.
    ///
    /// Every row also names the unit that has to *deliver* it (Story 34.16), and
    /// which unit that is depends on the path:
    ///
    /// * a path routed through LFS is delivered by its own object upload, so it
    ///   names that — a 3 GB video whose pointer is committed and whose bytes are
    ///   stuck is the case this whole column exists for;
    /// * every other path is delivered by the push that publishes the commit, so
    ///   they all name `push_unit` and move together, which is the truth: git
    ///   publishes a commit whole or not at all.
    ///
    /// Only ever called once a commit object exists.
    fn record_commit_activity(
        &self,
        profile: &SyncProfile,
        staged: &git::commit::StagedChange,
        lfs_units: &HashMap<PathBuf, i64>,
        push_unit: Option<i64>,
    ) -> Result<()> {
        let mut rows: Vec<db::ActivityEntry> = Vec::with_capacity(staged.len());
        let buckets = [
            (ActivityKind::Added, &staged.added),
            (ActivityKind::Modified, &staged.modified),
            (ActivityKind::Deleted, &staged.deleted),
        ];
        for (kind, paths) in buckets {
            for path in paths {
                // Repository-relative already — `StagedChange` holds nothing
                // else — so this never leaks a home directory into the UI.
                rows.push(db::ActivityEntry {
                    kind,
                    path: path.to_string_lossy().into_owned(),
                    size_bytes: staged.sizes.get(path).copied(),
                    unit_id: lfs_units.get(path).copied().or(push_unit),
                });
            }
        }
        let now = self.platform.now_ms();
        self.with_db(|conn| db::record_activity(conn, &profile.id, now, &rows))
    }

    /// Where this profile's LFS server is, and what to show it (Story 34.17).
    ///
    /// Three sources of authority, in this order, and the order is the whole
    /// content of this function:
    ///
    /// 1. **`.lfsconfig`.** A repository that names its LFS server has settled
    ///    the question; nothing below may second-guess it.
    /// 2. **`git-lfs-authenticate` over ssh**, for an `ssh://` or scp-style
    ///    remote. This is the case that was simply missing, and it is the reason
    ///    a folder synced over ssh pushed happily while every large file's
    ///    upload parked on a 401: `git push` authenticates with the user's ssh
    ///    key and needs no token, while the LFS batch API is HTTPS and needs
    ///    one, and there was no bridge between the two. The server mints a
    ///    `Bearer` JWT it signs itself — the one scheme [`crate::credential`]
    ///    documents as impossible for keeper to produce — and hands back the
    ///    endpoint to spend it at.
    /// 3. **The derived endpoint plus the stored token.** What every https
    ///    remote has always used, and the fallback when the remote turns out to
    ///    have no `git-lfs-authenticate` at all.
    ///
    /// A stored token still applies in case 2 when the server sends no
    /// `Authorization` of its own, which is legal and which the batch client
    /// then has something to answer a 401 with.
    async fn lfs_access(
        &self,
        profile: &SyncProfile,
        lfsconfig: Option<&str>,
        operation: lfs::ssh::Operation,
    ) -> Result<(url::Url, Option<String>)> {
        let stored = self.token(profile)?.map(|token| token.lfs_basic());
        if let Some(override_url) = lfs::endpoint::override_url(lfsconfig, "origin")? {
            return Ok((override_url, stored));
        }

        let Some(remote) = lfs::ssh::SshRemote::parse(&profile.remote_url) else {
            // An https remote: Basic, with the token as the user-id — the same
            // credential the push side hands git, dressed by the one type that
            // knows how (AD-53). `Bearer` here is what Forgejo answers 401 to,
            // because it reserves that scheme for the LFS JWT it signs itself.
            return Ok((lfs::endpoint::derive(&profile.remote_url)?, stored));
        };

        let key = remote.cache_key(operation);
        match self.cached_ssh_answer(&key) {
            Some(lfs::ssh::Answer::Granted(credential)) => {
                return self.spend(profile, &remote, credential, stored);
            }
            Some(lfs::ssh::Answer::NoSshLfs) => {
                return Ok((lfs::endpoint::derive(&profile.remote_url)?, stored));
            }
            None => {}
        }

        let answer = lfs::ssh::authenticate(&remote, operation).await?;
        let expires_ms = match &answer {
            lfs::ssh::Answer::Granted(credential) => credential.expires_ms(self.platform.now_ms()),
            // A server without LFS over ssh is remembered for the same window a
            // credential would live, so enabling it on the server takes effect
            // without a restart while forty objects in one commit still cost one
            // handshake between them.
            lfs::ssh::Answer::NoSshLfs => self.platform.now_ms() + lfs::ssh::DEFAULT_TTL_MS,
        };
        Self::lock(&self.lfs_ssh_credentials).insert(
            key,
            CachedSshAnswer {
                answer: answer.clone(),
                expires_ms,
            },
        );

        match answer {
            lfs::ssh::Answer::Granted(credential) => {
                tracing::info!(
                    profile = profile.name,
                    "the remote minted an LFS credential over ssh"
                );
                self.spend(profile, &remote, credential, stored)
            }
            lfs::ssh::Answer::NoSshLfs => Ok((lfs::endpoint::derive(&profile.remote_url)?, stored)),
        }
    }

    /// Turn a minted credential into the endpoint and header the batch client
    /// takes.
    ///
    /// `href` is used **verbatim** when the server named one: `/info/lfs` is a
    /// property of the *derivation*, and the server has already put it in the
    /// URL. Appending it again produces a 404 that reads plausibly enough in a
    /// log to cost an afternoon.
    ///
    /// # Which credential goes with which endpoint
    ///
    /// [`lfs::ssh`]'s response parser validates the `href`'s *scheme* and nothing
    /// else, which is the right division of labour — a transport should not have
    /// opinions about our keychain — but it does mean a server can name any host
    /// on the internet and be believed. So the two credentials are treated as the
    /// different things they are:
    ///
    /// * a **server-minted** `Authorization` is always honoured, at whatever
    ///   host the same response named. It is the server's own token, scoped by
    ///   the server, for the endpoint the server chose; that pairing is the
    ///   entire content of the handshake and second-guessing it would break the
    ///   CDN-and-object-store topologies it exists to express;
    /// * the **stored** token is the user's keychain PAT for *this remote*, and
    ///   it is attached only when the href stays on the remote's own host.
    ///   Without that check a server answering `git-lfs-authenticate` with an
    ///   href and no header — legal, and the case this fallback exists for —
    ///   could harvest the PAT for any host it cared to name, in cleartext for
    ///   an `http` href.
    fn spend(
        &self,
        profile: &SyncProfile,
        remote: &lfs::ssh::SshRemote,
        credential: lfs::ssh::Credential,
        stored: Option<String>,
    ) -> Result<(url::Url, Option<String>)> {
        let Some(href) = credential.href else {
            // No href: the endpoint is derived from the profile's own remote, so
            // it is this remote's host by construction and the stored token
            // belongs there. The error is propagated rather than swallowed —
            // this used to answer `https://invalid.localhost/` with the
            // credential still attached, and `*.localhost` resolves to loopback
            // on every mainstream resolver, so the "unreachable" fallback was a
            // real authenticated request to whatever happened to be listening on
            // local 443. The caller is in a `Result` context and always was.
            return Ok((
                lfs::endpoint::derive(&profile.remote_url)?,
                credential.authorization.or(stored),
            ));
        };
        let authorization = match credential.authorization {
            Some(minted) => Some(minted),
            None if Self::href_is_same_host(remote, &href) => stored,
            None => {
                tracing::warn!(
                    profile = profile.name,
                    href = %href,
                    "the remote named an LFS endpoint on another host and minted no \
                     credential for it, so the stored token is not sent there"
                );
                None
            }
        };
        Ok((href, authorization))
    }

    /// Does a server-named endpoint live on the host we authenticated to?
    ///
    /// `user_host` is `host` or `user@host` — the userinfo is ssh's business and
    /// says nothing about where an HTTPS request goes, so it is stripped.
    /// Compared case-insensitively because `Url` has already lowercased its own
    /// host while a remote URL carries whatever the user typed. A bracketed IPv6
    /// literal survives both steps intact: it contains no `@`, and `host_str`
    /// returns it bracketed too.
    fn href_is_same_host(remote: &lfs::ssh::SshRemote, href: &url::Url) -> bool {
        let host = remote
            .user_host
            .rsplit_once('@')
            .map_or(remote.user_host.as_str(), |(_, host)| host);
        href.host_str()
            .is_some_and(|named| named.eq_ignore_ascii_case(host))
    }

    /// The cached answer for one key, if it has not expired.
    fn cached_ssh_answer(&self, key: &str) -> Option<lfs::ssh::Answer> {
        let now = self.platform.now_ms();
        let mut cache = Self::lock(&self.lfs_ssh_credentials);
        let cached = cache.get(key)?;
        if cached.expires_ms <= now {
            cache.remove(key);
            return None;
        }
        Some(cached.answer.clone())
    }

    /// Forget every ssh-minted credential for this remote.
    ///
    /// Called when the server rejects one. A cached credential that has been
    /// refused is worse than none: it would be re-sent on every retry until its
    /// TTL ran out, turning a rotated key into minutes of identical 401s.
    /// Dropping both operations rather than the one that failed is deliberate —
    /// they come from the same key and the same session, so one being refused
    /// says nothing good about the other.
    fn forget_ssh_credentials(&self, profile: &SyncProfile) {
        let Some(remote) = lfs::ssh::SshRemote::parse(&profile.remote_url) else {
            return;
        };
        let mut cache = Self::lock(&self.lfs_ssh_credentials);
        for operation in [lfs::ssh::Operation::Upload, lfs::ssh::Operation::Download] {
            cache.remove(&remote.cache_key(operation));
        }
    }

    /// The credential refresher an LFS 401 consults (Story 34.15).
    ///
    /// A 401 carrying `WWW-Authenticate: Basic` is the server saying "not that
    /// credential, or not that shape". Two things can genuinely have changed
    /// since the request went out, and both are answered from the keychain
    /// without a round trip: the user pasted a new token while a transfer was
    /// in flight, and a unit re-driven from the journal is carrying whatever
    /// the *previous* process read.
    ///
    /// `None` back means "nothing new to say", which
    /// [`lfs::batch::BatchClient`] turns into [`SyncError::Auth`] rather than a
    /// second identical request — a retry storm against a git host is how an
    /// account gets locked. That is the answer in three cases: the challenge
    /// asks for a scheme we cannot mint (Forgejo's LFS `Bearer` is a JWT its
    /// own server signs, reachable only over SSH), the keychain has no token,
    /// or it still holds exactly what we already sent.
    ///
    /// `sent` is the header value in flight, not the token: comparing dressed
    /// values is what answers "would the retry be byte-identical", and it keeps
    /// the raw secret out of the closure's captured state.
    fn lfs_auth_refresh(
        &self,
        profile: &SyncProfile,
        sent: Option<String>,
    ) -> lfs::batch::AuthRefresh {
        let platform = Arc::clone(&self.platform);
        let key = profile.secret_key();
        let profile_name = profile.name.clone();
        Box::new(move |challenge| {
            if !challenge_accepts_basic(challenge) {
                tracing::debug!(
                    profile = profile_name,
                    "the LFS server asked for an authentication scheme keeper cannot supply"
                );
                return None;
            }
            // A keychain read that fails is not a place to raise: the caller
            // has a rejection in hand either way, and `None` is exactly "no
            // fresh credential".
            let fresh = platform
                .secret_get(&key)
                .ok()
                .flatten()
                .map(|secret| AccessToken::new(secret).lfs_basic())?;
            if Some(&fresh) == sent.as_ref() {
                return None;
            }
            tracing::info!(
                profile = profile_name,
                "retrying the LFS request with the stored credential"
            );
            Some(fresh)
        })
    }

    /// `unit` is the journal row this transfer belongs to, carried through to
    /// [`Self::materialize_landed`], which asks the journal whether anybody is
    /// waiting for it. `None` for a caller that is not draining a row. Ignored
    /// on the upload leg, which publishes nothing to the worktree.
    async fn do_lfs(
        &self,
        profile: &SyncProfile,
        oid: &str,
        size: u64,
        label: Option<&str>,
        upload: bool,
        unit: Option<i64>,
    ) -> Result<()> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        // The direction is in the phase because that is what the tray reads: an
        // upload and a download are different glyphs, so they have to be
        // different phases (see `SyncPhase::direction`).
        let phase = if upload {
            SyncPhase::UploadingLfs
        } else {
            SyncPhase::DownloadingLfs
        };
        // The file, not the folder. Every other leg of a sync moves many paths
        // at once and can only honestly name the profile; a transfer moves
        // exactly one object, and the path it lands at is the most useful thing
        // a progress line can say — especially on a slow link, where one object
        // owns the line for an hour.
        let mut event = self.progress(profile, phase);
        event.current = label.map(str::to_owned);
        self.publish(event);

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

        // A filesystem remote has no LFS server and never will, so the object
        // moves by copy. Ahead of the endpoint resolution below because there is
        // no endpoint to resolve: `endpoint::derive` refuses a local path
        // outright, which is correct and used to mean the object simply stayed
        // behind while the push published its pointer (Story 34.18). An explicit
        // `.lfsconfig` still wins — someone who has named an LFS server beside a
        // pendrive remote meant it.
        if lfsconfig.is_none() {
            if let Some(remote) = lfs::local::remote_store(&profile.remote_url) {
                return self
                    .copy_lfs_object(profile, remote, oid, size, upload, label, unit)
                    .await;
            }
        }
        let operation = if upload {
            lfs::ssh::Operation::Upload
        } else {
            lfs::ssh::Operation::Download
        };
        let (endpoint, auth) = self
            .lfs_access(profile, lfsconfig.as_deref(), operation)
            .await?;
        let client = lfs::batch::BatchClient::new(self.http.clone(), endpoint, auth.clone())
            .with_ref(format!("refs/heads/{}", profile.branch))
            // Without this the 401 path in `BatchClient` has no credential to
            // fall back on and turns every rejection straight into a permanent
            // `Auth` failure — a parked unit, from one round trip.
            .with_auth_refresh(self.lfs_auth_refresh(profile, auth.clone()));

        let want = vec![lfs::batch::ObjectId::new(oid, size)];
        let batched = if upload {
            client.upload(&want).await
        } else {
            client.download(&want).await
        };
        // A credential the server has refused must not be re-sent on the next
        // attempt: a rotated ssh key or an expired JWT would otherwise cost the
        // whole cache window in identical 401s, and the unit would park having
        // never tried a fresh one.
        let specs = match batched {
            Ok(specs) => specs,
            Err(err) => {
                if matches!(err, SyncError::Auth { .. } | SyncError::Forbidden { .. }) {
                    self.forget_ssh_credentials(profile);
                }
                return Err(err);
            }
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
            lfs::basic::BasicTransfer::new(self.transfer_http.clone(), store.clone())
                .with_sink(Box::new(move |event| {
                    // `false` detaches the reporter for good, so this says
                    // "stop" only once the receiver is genuinely gone.
                    event_tx.send(event).is_ok()
                }))
                // The object transfer can be rejected on its own — Forgejo
                // echoes the batch credential into the action headers, where it
                // can age out mid-transfer — so it gets the same refresher.
                .with_auth_refresh(self.lfs_auth_refresh(profile, auth.clone())),
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
                phase,
                label,
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
                if matches!(err, SyncError::Auth { .. } | SyncError::Forbidden { .. }) {
                    self.forget_ssh_credentials(profile);
                }
                return Err(err);
            }
        }

        if upload {
            // The remote now holds this object, and the unit's label is the
            // path it was queued for — `commit_local` names every upload unit
            // with `index_key`. This is the only per-path confirmation keeper
            // gets for content it authored, and without it a locally created
            // file is unreleasable at any age (Story 56.5, FR-341).
            self.note_unit_synced(profile, label, oid);
        } else {
            // The object is in the store; the worktree still holds the pointer
            // that was checked out. Replacing it is what makes a peer's clone
            // contain real bytes rather than a text stub.
            //
            // The path that just arrived, not the whole tree: see
            // [`Self::materialize_landed`] for the 40 hours the sweep spent here.
            self.materialize_landed(profile, &store, oid, label, unit)?;
        }
        Ok(())
    }

    /// Memo that the remote now holds the object one upload unit carried
    /// (Story 56.5, FR-341).
    ///
    /// The label is the unit's name, and `commit_local` sets it to
    /// [`lfs::stage::index_key`] of the path being uploaded, which is exactly
    /// the `materialized` ledger's key. An unlabelled unit — every upload
    /// queued before this story — writes nothing and is not an error: no label
    /// means no path this confirmation could honestly be attached to.
    ///
    /// # A stale unit must not forge a confirmation
    ///
    /// `oid` is what actually crossed the wire, and the memo is written only
    /// when the label's currently indexed pointer still names it. The ordinary
    /// sequence that makes that necessary: path A is committed as oid X and
    /// upload unit U is enqueued and labelled `A`; the server is unreachable so
    /// U waits; the owner edits A; A is committed as oid Y, and
    /// [`db::note_local_authorship`] correctly clears A's `synced_at_ms`; U
    /// finally completes and stamps `synced_at_ms = now` for A — recording that
    /// the remote holds A's content when what it holds is the superseded X.
    ///
    /// The bytes survived that only because [`Self::release_resolved`] takes a
    /// fresh [`Self::remote_serves`] proof at the moment of deletion, so
    /// AD-131's two independent barriers collapsed to one for exactly the path
    /// most likely to have been edited. A mismatch means the unit is stale and
    /// honestly confirms nothing: logged at `debug!` and dropped.
    ///
    /// Fail closed. A repository this cannot open, or a path the index no
    /// longer carries a pointer for, records nothing either — an unwritten memo
    /// costs one unreleasable path until the next upload of it, and a wrong one
    /// costs a guard.
    ///
    /// One index read, and free where it sits: this method is synchronous and
    /// runs immediately after a network transfer. The [`gix::Repository`] lives
    /// in a scoped block, as every other one in this file does.
    ///
    /// **Shared object, two paths: only the labelled one is confirmed.**
    /// `db::label_unit` is first-writer-wins so that a name does not flicker,
    /// which means one unit covers both paths but names one. The other keeps
    /// its `NULL` and stays unreleasable, which is the conservative direction
    /// and the one FR-341 asks for.
    ///
    /// Best-effort: the bytes reached the remote, and failing the unit because
    /// a memo did not land would re-upload them.
    fn note_unit_synced(&self, profile: &SyncProfile, label: Option<&str>, oid: &str) {
        let Some(path) = label else {
            return;
        };
        let current = {
            let Ok(repo) = git::repo::open(&profile.local_path, profile.removable) else {
                tracing::debug!(
                    profile = profile.name,
                    path = path,
                    "uploaded the object, but could not open the repository to check \
                     whether this path still names it",
                );
                return;
            };
            lfs::stage::indexed_pointer(&repo, &PathBuf::from(path))
        };
        if !current.is_some_and(|pointer| pointer.oid == oid) {
            tracing::debug!(
                profile = profile.name,
                path = path,
                oid = oid,
                "uploaded an object this path no longer commits; recording no \
                 confirmation, because the remote does not hold the bytes this \
                 path now names",
            );
            return;
        }
        let now = self.platform.now_ms();
        if let Err(err) = self.with_db(|conn| db::note_synced(conn, &profile.id, path, now)) {
            tracing::debug!(
                profile = profile.name,
                path = path,
                error = %err,
                "uploaded the object, but could not record the confirmation",
            );
        }
    }

    /// Move one LFS object to or from a filesystem remote (Story 34.18).
    ///
    /// The counterpart of [`Self::do_lfs`]'s HTTP path, for a remote that is a
    /// directory rather than a server: a pendrive, an SMB mount, a bare
    /// repository elsewhere on the disk. `git push` has always copied its own
    /// objects into such a remote; this is the same courtesy for the content the
    /// pointers name, and without it a pendrive carries a tree of stubs.
    ///
    /// Blocking, so it runs on `spawn_blocking`: the copy hashes every byte to
    /// verify it, which for this subsystem means gigabytes.
    ///
    /// `unit` rides through for [`Self::do_lfs`]'s reason: this is the
    /// filesystem-remote half of the same leg, so a requested unit delivered
    /// off a pendrive must publish on the same terms one delivered over HTTP
    /// does.
    #[allow(clippy::too_many_arguments)]
    async fn copy_lfs_object(
        &self,
        profile: &SyncProfile,
        remote: lfs::store::LfsStore,
        oid: &str,
        size: u64,
        upload: bool,
        label: Option<&str>,
        unit: Option<i64>,
    ) -> Result<()> {
        // An unmounted volume is absence, never failure (AD-48), and it must not
        // read as a corrupt or missing object. `Deferred` waits for the volume to
        // come back rather than spending the retry budget on a drive nobody has
        // plugged in.
        if !lfs::local::is_reachable(&remote) {
            return Err(SyncError::MediaAbsent);
        }

        let local = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        local.ensure_layout()?;
        let (from, to) = if upload {
            (local.clone(), remote)
        } else {
            (remote, local.clone())
        };
        let oid = oid.to_owned();
        let moved = {
            let oid = oid.clone();
            tokio::task::spawn_blocking(move || lfs::local::transfer(&from, &to, &oid, size))
                .await
                .map_err(|err| SyncError::Journal(format!("lfs copy task failed: {err}")))??
        };
        // Reported as traffic because that is what it is: on a pendrive or an
        // SMB mount these bytes cross a bus the user is waiting on, and a
        // transfer that reported zero would make the whole run look idle.
        self.add_transferred(&profile.id, moved);
        tracing::info!(
            profile = profile.name,
            bytes = moved,
            direction = if upload { "upload" } else { "download" },
            "copied a large object to a filesystem remote"
        );

        if upload {
            // The filesystem remote now holds the object, which is the same
            // per-path confirmation the HTTP leg records — a pendrive is a
            // remote (Story 56.5, FR-341).
            self.note_unit_synced(profile, label, &oid);
        } else {
            // The object is in the store; the worktree still holds the pointer
            // that was checked out. Replacing it is what makes this clone contain
            // real bytes rather than a text stub.
            //
            // The path that just arrived, not the whole tree: see
            // [`Self::materialize_landed`] for the 40 hours the sweep spent here.
            self.materialize_landed(profile, &local, &oid, label, unit)?;
        }
        Ok(())
    }

    /// Replace the pointer for the one path an object just landed for.
    ///
    /// [`Self::materialize_pending`] is the whole-tree sweep, and it is the
    /// right shape exactly once per pass: it reads the index and then stats —
    /// and reads — every tracked file small enough to be a pointer. Per
    /// *object* it is quadratic, and on real content that is not a slow path,
    /// it is a stopped one. Measured on the folder that reported this: 154,765
    /// tracked paths on a USB APFS volume at 3.9 ms per path cold is ~10
    /// minutes for one sweep, so a backlog of 238 objects would have spent ~40
    /// hours re-reading the same tree — one file materialized every 2 to 15
    /// minutes, the queue frozen at "238 files left, 4.5 GB", and no throughput
    /// figure at all, because the worker was inside a sweep instead of on the
    /// wire. Delivering one object must never cost a walk of the repository.
    ///
    /// A transfer names its path in the journal row's label (that is what
    /// `label_unit` is for), so the object that just arrived needs one stat, one
    /// read and one index refresh. An unlabelled row (`None`) defers to the
    /// sweep rather than paying for a walk here.
    ///
    /// # A request is for content, so it covers every path that content sits at
    ///
    /// Two paths holding identical bytes share one object and therefore one
    /// download — the case `label_unit`'s first-writer-wins rule exists for.
    /// For a *background* arrival the label is the whole answer: the second
    /// path keeps its pointer until the next sweep, one tick away in
    /// [`Self::scan_and_enqueue`], which is where a whole-tree question
    /// belongs.
    ///
    /// For a *requested* arrival the label is not enough, and the first version
    /// of this shipped believing it was. Ask for `a.mp4` and then for
    /// `a-copy.mp4` before delivery: the second request deduplicates onto the
    /// same row (correctly — the bytes are identical), the label still says
    /// `a.mp4`, and under `PointerOnly` nothing publishes `a-copy.mp4` ever,
    /// because the sweep is switched off in that mode. The request reported a
    /// unit id and silently never delivered. So a requested arrival asks the
    /// index which paths this oid belongs to and publishes each of them that
    /// still holds pointer text. The cost — one index read per requested
    /// completion, never per background one — is bounded by the thing that
    /// makes it safe: a human, or one click, produced it.
    ///
    /// # Whether it was requested is read from the journal, not passed in
    ///
    /// [`db::unit_urgency`] is consulted here rather than at claim time,
    /// because a request that arrives while its covering download is already
    /// `running` — which [`WorkKind::covered_while_running`] makes ordinary —
    /// raises the urgency of a row the supervisor read before the person asked.
    ///
    /// # Being requested bypasses the mode gate, and nothing else
    ///
    /// `lfs_mode != Materialize` is the right answer for *arrival*:
    /// [`LfsMode::PointerOnly`] means "leave what arrives as pointers", and
    /// [`LfsMode::Disabled`] has no pointers to replace. It is fatal for a
    /// *request*, because `PointerOnly` is the mode in which paths are virtual
    /// in the first place — so without this, asking for a file under
    /// `PointerOnly` would fetch the object into the store, return `Ok`, and
    /// leave the worktree holding pointer text: green tests over an inert
    /// feature (Story 56.3).
    ///
    /// It bypasses exactly that one gate. `Disabled` still refuses, because
    /// nothing routes the path through the clean filter there and real bytes
    /// would become a new blob in the next commit. The sparse cone, the
    /// pointer-text check and the store-contains check below all still hold,
    /// because each of them answers a question the request did not settle —
    /// what the worktree holds *now*, not what it held when the row was queued.
    fn materialize_landed(
        &self,
        profile: &SyncProfile,
        store: &lfs::store::LfsStore,
        oid: &str,
        label: Option<&str>,
        unit: Option<i64>,
    ) -> Result<()> {
        let requested = match unit {
            Some(id) => self.with_db(|conn| db::unit_urgency(conn, id))? == db::Urgency::Requested,
            None => false,
        };
        // Pointer-only leaves content as a pointer on purpose — the
        // whole-profile lever `materialize_pending` documents — and disabled has
        // no pointers to replace. A path somebody asked for is the exception,
        // and only for `PointerOnly`: see this function's doc.
        if profile.lfs_mode == LfsMode::Disabled
            || (profile.lfs_mode != LfsMode::Materialize && !requested)
        {
            return Ok(());
        }
        let Some(rela) = label.map(PathBuf::from) else {
            return Ok(());
        };
        let cone = SparseCone::new(&profile.subpaths);
        // The labelled path first: it is the one the progress line named and
        // the one a caller is most likely waiting on.
        let mut wanted: Vec<PathBuf> = Vec::new();
        if cone.includes(&rela) {
            wanted.push(rela.clone());
        }
        if requested {
            let repo = self.open_repo(profile)?;
            for (key, pointer) in lfs::stage::indexed_pointers(&repo) {
                if pointer.oid != oid {
                    continue;
                }
                let other = PathBuf::from(&key);
                if other != rela && cone.includes(&other) {
                    wanted.push(other);
                }
            }
        }

        let mut landed: Vec<PathBuf> = Vec::new();
        // The folder's virtualization policy, compiled at most once for this
        // delivery and only when something here could be refused by it (Story
        // 56.10, FR-328).
        //
        // `materialize_pending` no longer queues a transfer for an authorized
        // path, but that is not enough on its own, and two ordinary sequences
        // prove it. A unit queued on an earlier pass is still in the journal
        // when the owner adds the pattern that was supposed to stop exactly
        // those gigabytes — `.keepervirtual` is a committed file, so a *pull*
        // can install it — and this arm would then fetch and publish the very
        // path the same `sync_once` had just skipped. And every unit queued
        // before this story shipped delivers under whatever policy is
        // configured afterwards. Nothing reverses either one: the release sweep
        // needs a TTL, a window, remote proof and a platform that can say the
        // file is closed, and the trait default refuses on every real host today
        // (AD-125).
        //
        // **A requested delivery is exempt for its labelled path only.** That
        // path is a human's explicit authorization and outranks the policy
        // (AD-128, FR-330). Its *siblings* are not: a request fans out to every
        // indexed path carrying the same oid, which this function's own doc calls
        // ordinary for duplicated content, and nobody asked for those.
        let mut policy: Option<lfs::virtual_policy::VirtualPolicy> = None;
        let asks_the_policy = |candidate: &PathBuf| !(requested && *candidate == rela);
        if profile.lfs_mode == LfsMode::Materialize && wanted.iter().any(asks_the_policy) {
            policy = Some(lfs::virtual_policy::VirtualPolicy::compile(profile)?);
        }

        for candidate in &wanted {
            let one = std::slice::from_ref(candidate);
            let Some(smudge) = lfs::stage::pending_smudges(&profile.local_path, one)?.pop() else {
                // Not a pointer any more: another arm materialized it, or the
                // file has been replaced since the row was queued. Either way
                // there is nothing here to replace.
                continue;
            };
            // Resolved at the pointer's declared size, as every policy decision
            // is (AD-122), and only for a candidate nobody asked for. A size-0
            // pointer stands for the empty object, so there is no content for a
            // policy to keep away — carved out for `materialize_pending`'s
            // reason.
            if let Some(policy) = policy.as_ref() {
                if asks_the_policy(candidate)
                    && smudge.pointer.size > 0
                    && policy.resolve(&smudge.path, smudge.pointer.size)
                        == lfs::virtual_policy::Virtualization::Virtual
                {
                    continue;
                }
            }
            if !store.contains(&smudge.pointer.oid, smudge.pointer.size) {
                // The pointer in the worktree names a different object than the
                // one that landed — the path was edited since the row was
                // queued. The sweep owns that discovery, and queues the object
                // it now needs.
                continue;
            }
            lfs::stage::materialize(store, &profile.local_path, &smudge)?;
            // The one moment this is knowable: content for this path now exists
            // here, so the next object for it is a replacement rather than an
            // arrival.
            // git's spelling, from the very function 56.4's release and Story
            // 56.5's sweep key their rows through. A `\`-joined
            // `to_string_lossy` would, on Windows, write an arrival row the
            // sweep can never find and nothing would ever be released.
            let published = lfs::stage::index_key(&smudge.path);
            let now = self.platform.now_ms();
            self.with_db(|conn| db::remember_materialized(conn, &profile.id, &published, now))?;
            // The content came from upstream, and landing here is the first
            // thing that ever happened to it on this machine: the remote-origin
            // clock starts now (Story 56.5, AD-131). The identity rides along
            // because this is the one moment the pointer is in hand, and
            // `RELEASE_BUDGET_BYTES` can only bound a pass over rows that
            // recorded a size.
            self.with_db(|conn| {
                db::note_arrival(
                    conn,
                    &profile.id,
                    &published,
                    now,
                    &smudge.pointer.oid,
                    smudge.pointer.size,
                )
            })?;
            tracing::info!(
                profile = profile.name,
                path = published,
                "materialized LFS content"
            );
            landed.push(smudge.path);
        }
        if landed.is_empty() {
            return Ok(());
        }
        // Every worktree file just changed size, so its index entry carries the
        // pointer's stat and status would call it modified. Re-stat them in one
        // pass, which is what `refresh_index_stat` takes a slice for.
        let repo = self.open_repo(profile)?;
        git::repo::refresh_index_stat(&repo, &landed)?;
        Ok(())
    }

    /// Replace every checked-out pointer whose object is already local.
    ///
    /// Runs after a download and after applying remote changes. Objects that
    /// are not in the store yet are queued for transfer instead — so a partial
    /// fetch materializes what it can and returns for the rest.
    ///
    /// # The profile's `subpaths[]` filter the transfer, not just the checkout
    ///
    /// This is the second consumer of the cone (Story 27.2), and the one that
    /// actually saves bytes. A cone sparse-checkout on its own reduces no LFS
    /// traffic whatsoever, because git-lfs is entirely sparse-checkout-unaware
    /// — it is the `fetchinclude`/`fetchexclude` idiom Story 25.5 names, driven
    /// off the same list the checkout was, so the two can never drift.
    ///
    /// Filtering here rather than relying on the paths simply being absent is
    /// deliberate. A tracked file outside the cone can still be sitting in the
    /// working tree: `sparse-checkout set` refuses to remove one that has local
    /// modifications and says so ("the following paths are not up to date and
    /// were left despite sparse patterns"). Left unfiltered, that one stale
    /// pointer would pull down the gigabytes the profile exists to avoid.
    ///
    /// # The virtualization policy decides what arrives (Story 56.10, FR-328)
    ///
    /// The third filter, and the only one that is per path *and* survives a
    /// pull. Before Story 56.10 this function branched solely on
    /// `profile.lfs_mode == LfsMode::PointerOnly`, so `virtualPatterns` could
    /// not keep one single path virtual: the choice on offer was the whole
    /// profile or nothing, and FR-328's *"which paths are allowed to stay
    /// unmaterialized after a pull"* did not exist. A `Virtualization::Virtual`
    /// answer now leaves the committed pointer standing, publishes nothing out
    /// of the store even when the store holds the object, and queues no
    /// transfer — so the object is not on this clone either, which is the half
    /// of the request that is visible from outside keeper.
    ///
    /// It authorizes and never instructs (AD-123). Nothing here deletes,
    /// prunes or rewrites a worktree byte, in either direction: removing a
    /// pattern makes the next pull materialize the path, and adding one makes
    /// future arrivals stay away and existing materializations *eligible* for
    /// Story 56.5's sweep, which then applies every one of 56.4's refusals per
    /// object.
    fn materialize_pending(
        &self,
        profile: &SyncProfile,
        store: &lfs::store::LfsStore,
    ) -> Result<()> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        // Compiled once for the whole pass, never per path: `VirtualPolicy`'s
        // own doc forbids building a `GlobSet` inside a walk, and the common
        // case — no policy configured at all — must not pay for a feature it
        // does not use.
        //
        // A policy keeper cannot read is not a policy, and reading it as
        // "nothing is virtual" is the one answer unavailable here (FR-329): it
        // would materialize exactly the content the policy exists to keep away,
        // on whatever link this machine is on, and there is no undoing the
        // bytes. So `compile`'s `SyncError::Config` — which quotes the line as
        // typed and names the file to edit — propagates. `Self::sync_once`
        // reports it to the person who pressed the button; the supervisor's own
        // call site already logs it as one `warn!` and carries on with the rest
        // of the sync. Nothing is fetched and nothing is published either way.
        let policy = lfs::virtual_policy::VirtualPolicy::compile(profile)?;
        let repo = self.open_repo(profile)?;
        let cone = SparseCone::new(&profile.subpaths);
        // Pointer-sized entries only: see
        // [`git::repo::pointer_sized_tracked_paths`] for the ten minutes of
        // worktree I/O the unfiltered list costs on a real folder, and why the
        // index's own recorded size cannot lose a pointer.
        let tracked: Vec<PathBuf> =
            git::repo::pointer_sized_tracked_paths(&repo, lfs::pointer::MAX_POINTER_BYTES as u32)?
                .into_iter()
                .filter(|path| cone.includes(path))
                .collect();
        let pending = lfs::stage::pending_smudges(&profile.local_path, &tracked)?;
        if pending.is_empty() {
            return Ok(());
        }

        let now = self.platform.now_ms();
        let mut materialized = 0usize;
        for smudge in &pending {
            // The per-path lever, beside `subpaths[]` above and
            // `LfsMode::PointerOnly` below (FR-328, AD-122). Ahead of BOTH
            // branches, because one `Virtual` answer refuses the publish and
            // the transfer with one sentence — this content may live in the
            // object store and nowhere else — and a policy consulted separately
            // in each would be one edit away from leaving the worktree correct
            // while the bytes arrived anyway, which is the half of the ask a
            // person can actually see.
            //
            // Resolved at the POINTER's declared size, the only place the true
            // size is known before the content exists (AD-122). The worktree
            // file here is ~130 bytes of pointer text, so a `virtual_over_bytes`
            // floor compared against the stat would answer `Materialize` for
            // every path the floor exists to keep away.
            //
            // Pointer text spelling `size 0` stands for the **empty object**, so
            // there is no content anywhere for a policy to keep away — and a
            // `virtual_over_bytes` of `0`, the default, would answer `Virtual`
            // for it, because `0 < 0` is false. Left to the policy, that file
            // would read as ~90 bytes of pointer text to every application that
            // opens it, forever, with no bytes elsewhere to justify the
            // substitution and no verb that recovers it: the release side
            // refuses one as `AlreadyPointer`, and this sweep would skip it on
            // every later pass.
            //
            // Reachable only from text somebody else wrote, and that is worth
            // saying rather than leaving as a guess.
            // [`lfs::pointer::Pointer::render`] encodes the empty pointer as
            // *nothing at all* — upstream's `Pointer.Encoded()`, so an empty
            // file stays an empty file — so keeper's own checkout never leaves
            // one standing, while `Pointer::parse` accepts the text if another
            // tool or a hand produced it. That is exactly the input
            // `Self::release_resolved` carves out for, rather than leaving it to
            // the accident that `lfs::audit::tracked_objects` skips size-0
            // objects; carved out here too so the two verbs agree about the
            // empty object, which is the disagreement this whole story exists to
            // end.
            if smudge.pointer.size > 0
                && policy.resolve(&smudge.path, smudge.pointer.size)
                    == lfs::virtual_policy::Virtualization::Virtual
            {
                continue;
            }
            if store.contains(&smudge.pointer.oid, smudge.pointer.size) {
                // Pointer-only mode leaves content as a pointer on purpose: it
                // is the whole-profile lever, where `subpaths[]` and the
                // virtualization policy above are the per-path ones. All three
                // exist because git-lfs is entirely sparse-checkout-unaware.
                if profile.lfs_mode == LfsMode::PointerOnly {
                    continue;
                }
                lfs::stage::materialize(store, &profile.local_path, smudge)?;
                // The one moment this is knowable: content for this path now
                // exists here, so the next object for it is a replacement
                // rather than an arrival.
                // git's spelling, for `materialize_landed`'s reason: an arrival
                // row and a release must key the same row on every platform.
                let landed = lfs::stage::index_key(&smudge.path);
                self.with_db(|conn| db::remember_materialized(conn, &profile.id, &landed, now))?;
                // Arrived from upstream, so the remote-origin clock applies and
                // starts now (Story 56.5, AD-131). The identity rides along for
                // `materialize_landed`'s reason: this is the one moment the
                // pointer is in hand, and the byte ceiling can only bound rows
                // that recorded a size.
                self.with_db(|conn| {
                    db::note_arrival(
                        conn,
                        &profile.id,
                        &landed,
                        now,
                        &smudge.pointer.oid,
                        smudge.pointer.size,
                    )
                })?;
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
            // The path is carried beside the unit rather than inside it: the
            // payload is what `enqueue_unique` deduplicates on, and two paths
            // sharing one object — ordinary for duplicated content — must stay
            // one download. `label_unit` names the covering unit, so the second
            // path finds the first one's name already there and leaves it.
            let label = smudge.path.to_string_lossy().into_owned();
            self.with_db(|conn| {
                let id = db::enqueue_unique(conn, &profile.id, &unit, now, now)?;
                db::label_unit(conn, id, &label)
            })?;
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
        let Some(token) = self.token(profile)? else {
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
        // One snapshot for both strings: a rename between them would name two
        // different machines in the same pull request.
        let device = self.device();
        let body = serde_json::json!({
            "head": branch,
            "base": profile.branch,
            "title": format!("{}: changes from {}", profile.name, device.label),
            "body": format!(
                "Opened by keeper-sync for profile `{}` on `{}`.",
                profile.name, device.label
            ),
        });
        let response = self
            .http
            .post(&url)
            // `token <PAT>` — the REST API's own scheme, and the one place it
            // is right. It is not interchangeable with the Basic the LFS
            // endpoints want, which is why the spelling lives on the type.
            .header("Authorization", token.forge_api())
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
            .ok_or_else(|| SyncError::Busy(profile.name.clone()))?;

        tracing::info!(
            profile = profile.name,
            source = source.as_str(),
            "sync requested"
        );
        // Read before any work: `bytes` is what THIS run moved, and the counter
        // it is read from is process-lifetime cumulative.
        let transferred_before = self.transferred_bytes(&profile.id);
        let mut outcome = SyncOutcome::default();

        // Order is load-bearing: commit, then pull, then transfer, then push.
        //
        // Committing first means the merge never meets a dirty tree (git
        // refuses, correctly, rather than overwriting an uncommitted edit) and
        // a divergence arrives as commit-vs-commit, which is the only shape the
        // conflict-copy path can resolve. Pulling before pushing means we never
        // hand the remote a non-fast-forward it would just reject.
        self.ensure_repo(&profile)?;
        // `None`: the legs below run inline, so there is no journal row whose
        // success these rows can be pinned to. A push that fails here is
        // reported to this call's caller, and the supervisor's next scan queues
        // a real unit for the retry.
        outcome.files_changed = self.commit_local(&profile, source, None)?;
        if outcome.files_changed > 0 {
            outcome.committed = Some(profile.branch.clone());
        }

        // A fault in the arrival leg that must not also cost the person their
        // push. The only one this leg newly raises is a `.keepervirtual` that
        // does not compile (FR-329) — a **committed** file, so one typo'd glob
        // is shared by every clone, and aborting here made every explicit sync
        // on every clone commit locally, merge the remote in, and then stop:
        // the uploads that commit had just queued were never drained and the
        // branch was never pushed, until somebody edited the file. FR-329 asks
        // for the arrival to refuse loudly; it does not ask for the user's own
        // commits to be held hostage to it.
        //
        // So it is held and raised after the drain and push legs, which is
        // enough because nothing was fetched and nothing was published: the
        // commits are exactly as ready to publish as they were. The pass still
        // ends non-zero with the pattern quoted, and `Self::mark_synced` still
        // does not fire — a pass with a leg that raised is not a successful
        // pass, and the release sweep must not ride an edge that did not happen.
        let mut arrival_fault: Option<SyncError> = None;
        if profile.direction.pulls() {
            let converged = self.do_pull(&profile, source).await?;
            outcome.conflicts = converged.copies;
            outcome.stale = converged.stale;
            outcome.pulled = true;
            // Whatever the apply checked out may include pointers. Materialize
            // what is already local and queue the rest, BEFORE the push leg —
            // otherwise the scan would see a pointer-sized file where the index
            // records the full length and call it an edit.
            let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
            if let Err(err) = self.materialize_pending(&profile, &store) {
                arrival_fault = Some(err);
            }
        }

        // The commit above may have queued LFS transfers, and `do_push` refuses
        // to publish a pointer whose object is still outstanding. So the queue
        // is drained HERE, before the push, not after it: draining afterwards —
        // which is what this did — satisfied the letter of "the objects are
        // uploaded before the call claims success" while the pointer had already
        // been published a few lines earlier, which is the exact window a peer
        // clones a tree of pointer text through.
        //
        // Drained to quiescence, not once. `drain_journal` claims one bounded
        // batch — `CLAIM_LIMIT`, shared across every kind — so a single call
        // leaves the seventeenth large file of a healthy first sync outstanding,
        // the gate below refuses, and `sync_exit_code` turns that into a non-zero
        // exit: `keeper-syncd sync` under `Restart=on-failure` failed on a folder
        // where nothing whatsoever was wrong, and the app's "Sync now" reported
        // the same thing as an error.
        //
        // The loop condition is *strictly decreasing*, not merely non-zero, so it
        // is bounded by the queue rather than by the queue's health: an upload
        // that genuinely cannot land makes no progress on its second pass, the
        // loop stops, and the gate still refuses and still errors — which is the
        // outcome that case deserves. `u32::MAX` seeds it so the first pass
        // always runs, including the pass that discovers the debt: a `Push` unit
        // drained here commits, and that commit can queue uploads of its own.
        //
        // `false`, not `true`: these drains exist to settle work the commit just
        // created. Scanning again here would walk the tree a second time in one
        // pass for nothing.
        let mut previous = u32::MAX;
        loop {
            self.drain_journal(&profile, false, source).await?;
            let outstanding = self.lfs_uploads_outstanding(&profile)?;
            if outstanding == 0 || outstanding >= previous {
                break;
            }
            previous = outstanding;
        }

        if profile.direction.pushes() {
            self.do_push(&profile, source, None).await?;
            outcome.pushed = true;
        }

        // And again afterwards, for the units the push itself queues — a lane's
        // `OpenPullRequest` is the whole reason: a one-shot run may have no next
        // tick to pick it up.
        self.drain_journal(&profile, true, source).await?;
        // The user's commits are published; now say what went wrong. See where
        // this was captured for why it waited until here and no longer.
        if let Some(err) = arrival_fault {
            return Err(err);
        }
        outcome.bytes = self
            .transferred_bytes(&profile.id)
            .saturating_sub(transferred_before);

        // Every leg this profile's direction calls for ran and none of them
        // raised: that is the whole definition of a successful pass, and it is
        // the only definition under which a pull-only profile — or a push with
        // nothing staged — can ever record that it worked.
        self.mark_synced(&profile).await;

        self.set_state(&profile.id, ProfileState::Watching);
        self.publish(self.progress(&profile, SyncPhase::Idle));
        self.refresh_pending(&profile.id);
        Ok(outcome)
    }

    /// Release local LFS objects whose content is still in the worktree and
    /// whose upload the journal no longer owes.
    ///
    /// See [`crate::lfs::prune`] for why those two conditions are sufficient,
    /// and why this is safer than the heuristic `git lfs prune` has to use.
    fn prune_lfs_store(&self, profile: &SyncProfile) -> Result<()> {
        let repo = git::repo::open(&profile.local_path, false)?;
        let tracked = git::repo::tracked_paths(&repo)?;
        let owed = self.with_db(|conn| db::referenced_oids(conn, &profile.id))?;
        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        let releasable = lfs::prune::plan(&repo, &profile.local_path, &store, &tracked, &owed)?;
        if releasable.is_empty() {
            return Ok(());
        }
        let count = releasable.len();
        let reclaimed = lfs::prune::release(&store, &releasable)?;
        tracing::info!(
            profile = profile.name,
            objects = count,
            bytes = reclaimed,
            "released local LFS objects the remote already holds",
        );
        Ok(())
    }

    /// Let go of content whose release clock has run out (Story 56.5, FR-341,
    /// FR-342, AD-126, AD-131).
    ///
    /// Runs on [`Self::mark_synced`]'s success edge, inside the reservation the
    /// pass already holds, beside [`Self::prune_lfs_store`] — and on no timer.
    /// AD-62 forbids a second scheduler over one git repository absolutely, so
    /// the invocation point is an edge that was going to happen anyway: content
    /// is released by the first successful sync after its TTL expires, which is
    /// also the moment keeper has just proved it can reach the remote it would
    /// have to fetch the content back from.
    ///
    /// # A paused folder is not touched, and that is checked HERE
    ///
    /// The supervisor skips a disabled profile before `tick_profile`, so the
    /// obvious reading is that this can never see one. It is wrong.
    /// [`Self::sync_once`] checks the volume and the reservation and **not**
    /// `enabled`, so `keeper-syncd sync --once` on a paused folder, the app's
    /// `sync_folder_now`, and a notes vault's automatic push all reach
    /// `mark_synced` on a profile whose owner said *stop touching this*. Without
    /// the check below, each of those deleted up to
    /// [`RELEASE_BUDGET_OBJECTS`] files' content out of exactly that folder.
    ///
    /// [`Self::dehydrate_entry`] refuses the identical operation with
    /// [`lfs::hydrate::ContentRefusal::Paused`], and this method's own doc says
    /// the sweep is not a privileged caller — so a pause the request door
    /// honours and the sweep ignores would make that sentence false in the one
    /// direction that costs data. `docs/sync.md` states as fact that a paused
    /// folder never releases anything.
    ///
    /// It is the **first** statement, ahead of the TTL, the mode gate and the
    /// due gate, so a paused folder does not even arm a window that would be
    /// open the moment it is resumed.
    ///
    /// # A folder whose own config layer is broken releases nothing
    ///
    /// `releaseTtlMs` is a folder-tier field (FR-344, AD-132) and `0` is how a
    /// repository says *never release my content*. The folder applier is
    /// all-or-nothing: one bad key anywhere in a committed
    /// `.keeper/keeper.toml` discards the whole layer, the `0` with it, and the
    /// profile record's 24 h default takes over — turning "keep everything"
    /// into "delete after a day" on every clone that reads that file. A caller
    /// that deletes data must not read a permissive default out of a file the
    /// reader could not parse, so a faulted layer declines the pass. Fail
    /// closed, and ahead of the due gate for the pause's reason.
    ///
    /// # The candidate set is a value, not a side effect
    ///
    /// Eligibility is [`release_due_at`] and nothing else: a pure function over
    /// a ledger row. A pinned path and a locally-authored path the remote has
    /// never confirmed are not *considered*, which is a stronger property than
    /// being considered and refused — the latter would rest entirely on 56.4's
    /// guards, and AD-131 exists to stop one guard carrying the whole risk.
    ///
    /// # Every 56.4 refusal still applies
    ///
    /// The sweep is not a privileged caller. It runs the identical
    /// [`Self::release_resolved`] body — the same content-identity hash, the
    /// same per-object remote proof taken at the moment of deletion, the same
    /// fail-closed `open_file_state`, the same double pin read. A stored
    /// `synced_at_ms` selected the candidate; it authorized nothing. On a real
    /// host today every one of these therefore refuses `OpenUnknown`, which is
    /// 56.4's recorded and deliberate consequence.
    ///
    /// # A refusal is not a failure, and neither is it always silent
    ///
    /// [`SyncError::Refused`] is the *expected* answer for most candidates, so
    /// it is logged at `debug!` and the pass moves to the next one — with one
    /// exception, [`lfs::hydrate::ContentRefusal::AlreadyPointer`]. That
    /// refusal means the worktree holds pointer text, so the ledger row is a
    /// false claim that this machine holds the content: it can never release,
    /// it burns a budget slot on every pass forever, and it lies to
    /// [`db::materialized_paths`] — which feeds [`Self::pending`]'s
    /// `replacing` flag — and to `lfs::listing`. So the sweep retracts the row,
    /// best-effort. `db::forget_materialized` carries
    /// `AND COALESCE(pinned, 0) = 0`, so a pinned row cannot be discarded this
    /// way.
    ///
    /// Scoped to the sweep on purpose: [`Self::release_resolved`]'s own
    /// `AlreadyPointer` arms are unchanged, because Story 56.4's tests pin what
    /// the request door does with that answer, and a CLI verb reporting a no-op
    /// is not evidence that the ledger is wrong about anything.
    ///
    /// # One hard error does not abandon the rest of the pass
    ///
    /// A non-refusal error used to return, and the candidate list is
    /// `ORDER BY path` with stable causes: an EACCES on a mode-000 file reaches
    /// `lfs::stage::content_oid`, an EACCES/ELOOP/EIO on a parent reaches the
    /// `symlink_metadata` arm. So one badly-permissioned file early in the
    /// alphabet permanently prevented every alphabetically later path in that
    /// folder from ever being released, with no evidence but an hourly
    /// "released no expired content this pass" that reads like a quiet pass.
    ///
    /// Each one is therefore logged `warn!` with its path and the pass carries
    /// on — and the **first** error is still returned at the end, so
    /// [`Self::mark_synced`]'s swallow still gets its one line and the mutation
    /// proof of that swallow still bites. Continued past *and* returned: the
    /// two are answers to different questions, coverage and honesty.
    ///
    /// # Budgeted on both dimensions, and rotated
    ///
    /// [`RELEASE_BUDGET_OBJECTS`] bounds how long the reservation is held;
    /// [`RELEASE_BUDGET_BYTES`] bounds the hashing, because the identity proof
    /// reads every byte of every candidate. The count is checked *before* a
    /// candidate is attempted; the byte total is accumulated *after* one is,
    /// so the candidate that crosses the ceiling stops the pass having had its
    /// attempt. That asymmetry is deliberate: skipping it instead — the naive
    /// `if bytes + size > BUDGET { continue }` — would make an object larger
    /// than the whole budget permanently unreleasable.
    ///
    /// The count ceiling counts **attempts**, not releases, which is what makes
    /// [`Self::release_cursor`] load-bearing rather than a nicety. Re-derived
    /// from the top in the same `ORDER BY path` order every pass, a prefix of
    /// candidates that refuses the same way every time — 32 paths the owner
    /// edited in place, or on every real host today simply the first 32, since
    /// `open_file_state` defaults to `Unknown` and `release_resolved` refuses
    /// `OpenUnknown` — meant paths 33..n were never attempted once. Both
    /// [`RELEASE_BUDGET_OBJECTS`]' own doc and `docs/sync.md` promise the next
    /// pass takes the remainder; the rotation is what makes that true.
    async fn release_expired(&self, profile: &SyncProfile) -> Result<()> {
        // The pause, first and before anything reads a clock. See this method's
        // doc: `sync_once` does not check `enabled`, so this is the only thing
        // standing between a paused folder and a deletion — and it is ahead of
        // the due gate so such a folder arms no window either.
        if !profile.enabled {
            return Ok(());
        }
        // Fail closed on a folder whose own config layer will not parse. A `0`
        // in a committed `.keeper/keeper.toml` means "never release my
        // content", and one bad key elsewhere in that file discards the whole
        // layer and restores the 24 h default — so the permissive reading of an
        // unparseable file is the one answer this caller may not take.
        if crate::profile::folder::folder_config_is_faulted(&profile.local_path) {
            tracing::debug!(
                profile = profile.name,
                "release sweep declines a folder whose own config layer is faulted: \
                 `releaseTtlMs` may be set there and could not be read",
            );
            return Ok(());
        }
        // Before the clock: a zero TTL must not even arm a window. See
        // `release_is_due`, which repeats the check because it is the gate's own
        // invariant and not this caller's courtesy.
        let Some(ttl_ms) = profile.effective_release_ttl_ms() else {
            return Ok(());
        };
        // The mode alone, with no I/O at all, for the one mode that can never
        // release: a folder with large-file support off neither compiles a
        // policy nor arms a window.
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        if !self.release_is_due(profile) {
            return Ok(());
        }
        // The same gate the request door runs, so a mode that refuses one
        // refuses the other — but swallowed here rather than propagated. A
        // folder in the default mode is not a *failure*, it is a folder with
        // nothing for this sweep to do, and propagating would log a warning on
        // every successful sync of every ordinary folder.
        //
        // Since Story 56.10 that describes a folder in the default mode that
        // authorizes nothing: no `virtualPatterns` line and no committed
        // `.keepervirtual` permissive line, so there is nothing here to sweep. A
        // folder that authorizes something passes this gate and each of its rows
        // is asked again per path, by `release_path_gate`, at the committed
        // pointer's size.
        //
        // **Behind the due gate**, unlike before Story 56.10, because the gate
        // now reads `.keepervirtual` for a `Materialize` folder — which is most
        // of them. Behind it that read is paid once per `RELEASE_LOOK_EVERY_MS`
        // rather than once per successful sync, and an unparseable policy is
        // reported at the same hourly cadence instead of on every pass of a
        // push-only profile that never reaches the arrival path at all. The cost
        // is that such a folder arms a window before declining, which costs it
        // nothing: a folder with nothing to sweep wants to be asked less often,
        // not more.
        let policy = match release_mode_gate(profile) {
            Ok(policy) => policy,
            Err(err) => {
                tracing::debug!(
                    profile = profile.name,
                    reason = %err,
                    "release sweep has nothing to do in this folder",
                );
                return Ok(());
            }
        };

        let now = self.platform.now_ms();
        let rows = self.with_db(|conn| db::materialized_rows(conn, &profile.id))?;
        let candidates: Vec<&db::MaterializedRow> = rows
            .iter()
            .filter(|row| release_due_at(row, ttl_ms).is_some_and(|due| now >= due))
            // The policy, as a **pre-filter** on the budget rather than as the
            // authority. In the default mode the ledger holds a row for every
            // path this clone ever materialized while the policy typically names
            // one narrow zone, so without this the pass spends its
            // `RELEASE_BUDGET_OBJECTS` attempts — each a repository open and an
            // index read inside `release_resolved` — being refused, and
            // `RELEASE_BUDGET_BYTES` accumulates for those refusals too, so a
            // few large unauthorized rows can end a pass before an authorized
            // one is reached. Rotated cursor and all, an authorized path in a
            // large folder could take hundreds of syncs to be released while
            // `Self::release_schedules` showed its deadline long past.
            //
            // `unwrap_or(u64::MAX)`, never `0`: this filter may only ever be
            // wrong in the direction of letting a row through. A row written
            // before Story 56.5 recorded no size, and treating that as zero
            // would put it under any `virtualOverBytes` floor and drop it from
            // the candidate set for good — a filter deciding what
            // `release_resolved` is allowed to be asked about. The authority
            // stays where the honest size is, on the committed pointer.
            .filter(|row| {
                release_path_gate(
                    profile,
                    policy.as_ref(),
                    Path::new(&row.path),
                    row.size_bytes.unwrap_or(u64::MAX),
                )
                .is_ok()
            })
            .collect();
        if candidates.is_empty() {
            return Ok(());
        }

        // Rotate the window, mirroring `repair_cursor`/`REPAIR_WINDOW`: take
        // everything sorting after where the last pass stopped, then wrap
        // around to everything up to and including it. An absent cursor is the
        // empty string, which every path sorts after, so a first pass reads as
        // the plain ordered list and needs no branch of its own.
        let cursor = Self::lock(&self.release_cursor)
            .get(&profile.id)
            .cloned()
            .unwrap_or_default();
        let window = candidates
            .iter()
            .copied()
            .filter(|row| row.path > cursor)
            .chain(candidates.iter().copied().filter(|row| row.path <= cursor))
            .take(RELEASE_BUDGET_OBJECTS);

        let mut bytes = 0u64;
        let mut released = 0usize;
        let mut reclaimed = 0u64;
        // Kept and returned at the end rather than returned here: see this
        // method's doc for why the pass both continues past a hard error and
        // still reports the first one.
        let mut first_error: Option<SyncError> = None;
        let mut stopped_at: Option<String> = None;
        for row in window {
            // Where the next pass resumes, recorded per candidate rather than
            // after the loop, so a `break` on the byte ceiling leaves the same
            // honest mark a full window does.
            stopped_at = Some(row.path.clone());
            // The ledger's own size, when it has one. A row that never recorded
            // a size — every row written before Story 56.5 — counts as nothing
            // against the byte ceiling: that residual is real and is stated
            // here rather than left silent, and it is bounded by the object
            // ceiling, which counts such a row like any other.
            bytes = bytes.saturating_add(row.size_bytes.unwrap_or(0));

            // The row's path is already `index_key`'s spelling, which is the
            // frame `release_target` produces and `release_resolved` expects.
            let (rela, path) = match release_target(profile, &row.path) {
                Ok(target) => target,
                Err(SyncError::Refused(refusal)) => {
                    tracing::debug!(
                        profile = profile.name,
                        path = row.path,
                        refusal = %refusal,
                        "release sweep declined a candidate",
                    );
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        profile = profile.name,
                        path = row.path,
                        error = %err,
                        "release sweep could not resolve a candidate; the pass \
                         continues past it",
                    );
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                    continue;
                }
            };
            match self
                .release_resolved(profile, policy.as_ref(), &rela, path)
                .await
            {
                Ok(release) => {
                    released += 1;
                    reclaimed = reclaimed.saturating_add(release.size_bytes);
                }
                // The worktree holds pointer text, so the row claiming this
                // machine holds the content is false. Retracted here and only
                // here — `release_resolved`'s own arms are 56.4's and stay as
                // they are. See this method's doc.
                Err(SyncError::Refused(
                    refusal @ lfs::hydrate::ContentRefusal::AlreadyPointer { .. },
                )) => {
                    tracing::debug!(
                        profile = profile.name,
                        path = row.path,
                        refusal = %refusal,
                        "release sweep retracted a ledger row for a path that holds \
                         pointer text",
                    );
                    if let Err(err) =
                        self.with_db(|conn| db::forget_materialized(conn, &profile.id, &row.path))
                    {
                        tracing::debug!(
                            profile = profile.name,
                            path = row.path,
                            error = %err,
                            "could not retract that row; it will be reconsidered next pass",
                        );
                    }
                }
                Err(SyncError::Refused(refusal)) => {
                    tracing::debug!(
                        profile = profile.name,
                        path = row.path,
                        refusal = %refusal,
                        "release sweep declined a candidate",
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        profile = profile.name,
                        path = row.path,
                        error = %err,
                        "release sweep could not release a candidate; the pass \
                         continues past it",
                    );
                    if first_error.is_none() {
                        first_error = Some(err);
                    }
                }
            }
            // Accumulated, then checked — not checked, then accumulated. The
            // candidate that crosses the ceiling has already had its one
            // attempt, which is what stops an object larger than the whole
            // budget from being skipped on every pass forever.
            if bytes >= RELEASE_BUDGET_BYTES {
                break;
            }
        }
        if let Some(path) = stopped_at {
            Self::lock(&self.release_cursor).insert(profile.id.clone(), path);
        }
        if released > 0 {
            tracing::info!(
                profile = profile.name,
                paths = released,
                bytes = reclaimed,
                "released content whose retention window had expired",
            );
        }
        // `mark_synced` swallows this into one `warn!` line and the sync still
        // succeeds; every candidate after the failing one has already had its
        // attempt.
        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(())
    }

    /// Forget everything this profile remembers about its own tree and look again.
    ///
    /// The escape hatch for the one failure the engine cannot detect from the
    /// inside: it decides what changed by comparing each path against the
    /// `file_state` row it recorded last time, so a file whose size, mtime, ctime
    /// and inode all match a remembered row is *not* a change — and it is right
    /// about that almost always. Almost. A copy that preserves mtime (which
    /// [`crate::copy`] does deliberately), a restore from a backup, a clock that
    /// moved, a filesystem that recycles inodes: each can land a file that looks
    /// exactly like one already accounted for. Nothing on the next hundred scans
    /// will notice, because every one of them asks the same question and gets the
    /// same answer.
    ///
    /// So this deletes the rows rather than re-reading them. With nothing
    /// remembered every path on disk is new, the next walk queues whatever the
    /// tree actually holds, and the profile converges on the truth instead of on
    /// its own record of it.
    ///
    /// The settle gate goes too. A gate seeded from the old state would hold
    /// paths against observations that no longer exist, so the first walk after
    /// this would report files as settling that nothing is writing.
    ///
    /// Cheap in the ordinary case — a tree that genuinely matches its rows is
    /// re-recorded as it is walked and nothing is queued — and idempotent, so
    /// pressing it twice costs one extra walk and changes nothing.
    pub fn rescan(&self, id: &str) -> Result<()> {
        if self.with_db(|conn| db::get_profile(conn, id))?.is_none() {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        }
        self.with_db(|conn| db::clear_file_state(conn, id))?;
        Self::lock(&self.gates).remove(id);

        // Scratch first: a person pressing this button has a folder that is
        // misbehaving, and leaked transfer scratch is both a symptom of that and
        // a cost that nothing else will ever reclaim. Best effort, like the
        // repair below — housekeeping may not fail the button.
        if let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? {
            let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
            store.sweep_scratch(&profile.name);
        }

        // "Recheck all files" now means git's memory too, not only keeper's.
        //
        // Forgetting `file_state` makes keeper look at everything again; it
        // does nothing for the index entries whose cached stat stopped
        // describing the worktree, and those are what turn a status pass into
        // a filter conversion of the entire folder. A person whose folder has
        // stopped syncing presses this button — it should be able to repair
        // the thing that stopped it. See `repair_index_stat`.
        //
        // Best effort, deliberately: a repository this cannot open is one the
        // walk below will report on properly, and failing the button here would
        // take away the "forget and look again" half that always works.
        match self.repair_stat_invariant(id) {
            Ok(0) => {}
            Ok(corrected) => tracing::info!(
                profile = id,
                corrected,
                "recheck: restored the index stat for materialized content"
            ),
            Err(err) => tracing::warn!(
                profile = id,
                error = %err,
                "recheck: could not restore the index stat; looking again anyway"
            ),
        }

        self.wake_now(id);
        tracing::info!(profile = id, "forgot the remembered tree state on request");
        Ok(())
    }

    /// Re-stat every materialized LFS entry whose index stat has gone stale.
    ///
    /// Split out so `rescan` reads as the two things it now does, and so the
    /// repair can be reached from anywhere else that finds the invariant
    /// broken.
    fn repair_stat_invariant(&self, id: &str) -> Result<usize> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Ok(0);
        };
        let repo = git::repo::open(&profile.local_path, profile.removable)?;
        git::repo::repair_index_stat(&repo)
    }

    /// Ask for a walk **now**, remembering everything already known.
    ///
    /// The half of [`Self::rescan`] that is a request to *look*, without the
    /// half that is a request to *forget*. Anything that merely knows the tree
    /// changed wants this one. The part of `rescan` that does the damage is
    /// `clear_file_state`, not `gates.remove`: the durable rows are what carry
    /// an episode across a dropped gate — [`Self::ensure_gate`] imports them —
    /// so deleting them restarts the settle window for every unrelated file in
    /// the profile, and a caller on a short cadence does that forever.
    ///
    /// That is not hypothetical. The notes cadence called `rescan` for exactly
    /// this purpose — its comment said "asked to notice now" — and on a folder
    /// that is both a notes vault and a recordings destination it wiped the
    /// gate roughly once a second. A recording stops, writes its note stub, the
    /// stub makes the vault dirty, the cadence fires, and the segments the
    /// recording just closed lose the episode they need to be committed. The
    /// recording could not sync *because* it had written a note about itself.
    pub fn wake_now(&self, id: &str) {
        // Both halves, or neither works: the flag is what makes the next tick
        // walk at all, and the cleared deadline is what stops it waiting out the
        // rest of the poll interval first.
        Self::lock(&self.next_scan_ms).remove(id);
        Self::lock(&self.next_sweep_ms).remove(id);
        Self::lock(&self.next_release_ms).remove(id);
        // ...and the rotation with it, for the reason the window is dropped: a
        // remembered halfway point across a re-add would make the first pass
        // after it start in the middle of a ledger it has never walked.
        Self::lock(&self.release_cursor).remove(id);
        self.note_watch_wake(id);
    }

    /// Ask the server whether it holds every object this folder's pointers name
    /// (DW-140).
    ///
    /// The remote half of [`Self::verify`], and the half that finds permanent
    /// loss. A pointer whose object never reached the server is a valid git
    /// blob, a clean `git status`, a green folder — and content only one machine
    /// in the world still has. Nothing in the sync path notices; the push gate
    /// that should have prevented it is the thing being checked.
    ///
    /// Read-only and cheap: one batch round trip per few hundred objects, no
    /// transfer, no writes. The `download` operation is what asks the question,
    /// because that is the one whose per-object 404 means "I cannot serve this".
    ///
    /// A folder with no LFS at all answers instantly and intact, which is the
    /// honest answer rather than an error about a server it never uses.
    pub async fn audit_remote_objects(&self, id: &str) -> Result<lfs::audit::RemoteAudit> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(lfs::audit::RemoteAudit::default());
        }
        self.publish(self.progress(&profile, SyncPhase::Verifying));

        let repo = self.open_repo(&profile)?;
        let tracked = git::repo::tracked_paths(&repo)?;
        let (objects, paths) = lfs::audit::tracked_objects(&repo, &tracked);
        drop(repo);
        if objects.is_empty() {
            return Ok(lfs::audit::RemoteAudit::default());
        }
        let checked = paths.values().map(|used| used.len() as u64).sum();
        let bytes = objects.iter().map(|object| object.size).sum();

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
        // A filesystem remote has no batch API to ask. The objects either sit in
        // the directory or they do not, and `verify` on the peer answers that
        // better than a fabricated round trip would.
        if lfsconfig.is_none() && lfs::local::remote_store(&profile.remote_url).is_some() {
            return Ok(lfs::audit::RemoteAudit {
                checked,
                objects: objects.len() as u64,
                bytes,
                missing: Vec::new(),
            });
        }

        let (endpoint, auth) = self
            .lfs_access(
                &profile,
                lfsconfig.as_deref(),
                lfs::ssh::Operation::Download,
            )
            .await?;
        let client = lfs::batch::BatchClient::new(self.http.clone(), endpoint, auth.clone())
            .with_ref(format!("refs/heads/{}", profile.branch))
            .with_auth_refresh(self.lfs_auth_refresh(&profile, auth.clone()));
        let specs = client.download(&objects).await?;
        // The audit just asked the server about every object this folder's
        // pointers name, and got a per-object affirmative for each one it
        // serves. That is exactly the per-path proof `synced_at_ms` records, so
        // it is memoized here rather than thrown away and re-asked by the
        // release sweep (Story 56.5, FR-341).
        //
        // Only from here: the `LfsMode::Disabled` return and the filesystem
        // short-circuit above both leave without asking the server anything, so
        // neither may write a confirmation.
        //
        // Best-effort and logged. `verify --remote` reports what the remote
        // holds; it must not fail because a memo did not land.
        let now = self.platform.now_ms();
        let confirmed: Vec<&String> = objects
            .iter()
            .filter(|object| lfs::audit::serves(&specs, object))
            .map(|object| &object.oid)
            .collect();
        if let Err(err) = self.with_db(|conn| {
            for oid in &confirmed {
                for rela in paths.get(oid.as_str()).into_iter().flatten() {
                    db::note_synced(conn, &profile.id, &lfs::stage::index_key(rela), now)?;
                }
            }
            Ok(())
        }) {
            tracing::debug!(
                profile = profile.name,
                error = %err,
                "audited the remote, but could not record the confirmations",
            );
        }
        Ok(lfs::audit::report(&specs, &paths, checked, bytes))
    }

    /// Can the remote hand over **this one object**, asked now (Story 56.4,
    /// NFR-40, AD-123)?
    ///
    /// The proof [`Self::dehydrate_entry`] takes immediately before it deletes
    /// the only local copy. Per object, affirmative, and fresh: no age, no
    /// pattern, no ref comparison and no stored `synced_at_ms` authorizes a
    /// deletion here.
    ///
    /// # Why every failure is `false` and never an error
    ///
    /// [`lfs::batch::BatchClient`] maps a transport failure to
    /// [`SyncError::Network`] ([`Retriability::Transient`]), a batch-level 404
    /// to [`SyncError::Config`] and an unauthenticated answer to
    /// [`SyncError::Auth`]. Propagating any of them from a release would turn
    /// "I could not establish that these bytes exist elsewhere" into a fault a
    /// scheduler is entitled to retry — and 56.5 will drive this verb from a
    /// sweep. Collapsing them into `false`, which the caller renders as
    /// [`lfs::hydrate::ContentRefusal::UnprovenOnRemote`] (`Permanent`),
    /// preserves the only sentence that matters: keeper will not delete these
    /// bytes. Each reason is logged `warn!`, so an operator can still tell an
    /// unreachable server from an absent object.
    ///
    /// # Why a filesystem remote is proved directly, and per object
    ///
    /// [`Self::audit_remote_objects`] reports a filesystem remote intact
    /// **without asking anything**, which is defensible for a report and would
    /// be a fabricated authorisation for a deletion. Such a remote's LFS store
    /// is a real directory in the client layout
    /// ([`lfs::local::remote_store`]), so `contains` is a genuine,
    /// size-verified, per-object answer — strictly better evidence than a batch
    /// response, and what makes this story's happy path assertable with no
    /// network at all.
    ///
    /// The `.lfsconfig` read and the endpoint/auth/ref ingredients are
    /// [`Self::audit_remote_objects`]'s, unchanged: one way of addressing a
    /// server, not two.
    async fn remote_serves(&self, profile: &SyncProfile, oid: &str, size: u64) -> bool {
        let lfsconfig = match std::fs::read_to_string(profile.local_path.join(".lfsconfig")) {
            Ok(text) => Some(text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    oid,
                    error = %err,
                    "cannot read .lfsconfig, so nothing can be established about this object; \
                     keeping the local copy"
                );
                return false;
            }
        };
        // An explicit `.lfsconfig` names a server, so it wins over the shape of
        // the remote URL — the same precedence `lfs_access` applies.
        if lfsconfig.is_none() {
            if let Some(store) = lfs::local::remote_store(&profile.remote_url) {
                let held = store.contains(oid, size);
                if !held {
                    tracing::warn!(
                        profile = profile.name,
                        oid,
                        "the filesystem remote's object store does not hold this object at this \
                         size; keeping the local copy"
                    );
                }
                return held;
            }
        }

        let (endpoint, auth) = match self
            .lfs_access(profile, lfsconfig.as_deref(), lfs::ssh::Operation::Download)
            .await
        {
            Ok(access) => access,
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    oid,
                    error = %err,
                    "cannot address an LFS server for this folder, so nothing can be established \
                     about this object; keeping the local copy"
                );
                return false;
            }
        };
        let client = lfs::batch::BatchClient::new(self.http.clone(), endpoint, auth.clone())
            .with_ref(format!("refs/heads/{}", profile.branch))
            .with_auth_refresh(self.lfs_auth_refresh(profile, auth.clone()));
        let object = lfs::batch::ObjectId::new(oid, size);
        match client.download(std::slice::from_ref(&object)).await {
            // `serves` requires an affirmative row for THIS object, at this
            // size, with a download action on it — and no errored row for it
            // anywhere in the answer. An answer that mentioned nothing, or
            // mentioned something else, or owes an href it did not supply, has
            // said nothing.
            Ok(specs) => lfs::audit::serves(&specs, &object),
            Err(err) => {
                tracing::warn!(
                    profile = profile.name,
                    oid,
                    error = %err,
                    "the LFS server could not be asked whether it holds this object; keeping the \
                     local copy"
                );
                false
            }
        }
    }

    /// Every LFS path in this profile, and whether this machine holds its
    /// content (Story 56.2, FR-336, FR-337).
    ///
    /// The engine's answer rather than the caller's, for the reason every other
    /// verb here is: `keeper-syncd` and the desktop shell both need it, and a
    /// listing assembled twice is a listing that will eventually disagree with
    /// itself about one folder (AD-52). What the daemon adds on top is two
    /// renderings of this `Vec` and nothing else.
    ///
    /// # Cost
    ///
    /// One `SELECT`, one `lstat` per LFS path, and a bounded read of the paths
    /// whose length says they could be pointer text — the discipline
    /// [`lfs::stage::worktree_pointer`] documents. The index is read and parsed
    /// **once** through [`lfs::stage::indexed_pointers`], where the per-path
    /// `indexed_pointer` this could have been built from re-parses it per row.
    ///
    /// The index read is not the whole cost, and understating it would be the
    /// mistake `is_pointer_candidate` was written to avoid one layer down:
    /// recognising a pointer means asking the object database for a blob's
    /// **header** once per unconflicted index entry, and loading the blob for
    /// every entry whose header says 1..=1024 bytes. There is no cheaper
    /// prefilter available — an index entry's `stat.size` is the *worktree*
    /// file's for an LFS path, so `git::repo::pointer_sized_tracked_paths`'
    /// filter would drop every materialized LFS path — so on a hundred-thousand
    /// path folder this is a hundred thousand header lookups in one blocking
    /// call. That is the honest figure for a verb a human invokes, and the
    /// reason nothing calls it on a tick.
    ///
    /// # It reads; it does not create
    ///
    /// Deliberately **not** [`Self::open_repo`], which is the engine's
    /// *materialize* path: for a folder that is not a repository yet it clones
    /// over the network or adopts in place, then pins repo config and runs
    /// [`Self::reconcile_sparse_cone`], which adds and deletes worktree files.
    /// A listing that did any of that would be the opposite of what it is sold
    /// as — an operator asking which paths are virtual would pull gigabytes, or
    /// silently turn a plain folder into a keeper repository. So a folder with
    /// no `.git` answers with an **empty listing**: it has no index, therefore
    /// no LFS paths, and creating one is somebody else's verb.
    ///
    /// `lfs_mode` is deliberately **not** consulted. A profile switched to
    /// [`LfsMode::Disabled`] does not stop its committed pointers from being
    /// pointers, and short-circuiting on the mode would make this verb tell an
    /// operator "no LFS paths" about a folder whose Files pane shows a dozen —
    /// which is precisely the assembled-twice divergence AD-52 is about. A
    /// folder that never used LFS has no pointers in its index and answers
    /// empty for the honest reason.
    ///
    /// Blocking, like every other repository caller: a `gix::Repository` is
    /// neither `Send` nor cheap to hold, so it never spans an await point.
    pub fn lfs_files(&self, id: &str) -> Result<Vec<lfs::listing::LfsFile>> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        if !profile.local_path.join(".git").exists() {
            return Ok(Vec::new());
        }
        let pointers = {
            // Removable media is opened with full trust for the reason
            // `open_repo` states (AD-48); the difference is that this opens
            // what is there and never makes one.
            let repo = git::repo::open(&profile.local_path, profile.removable)?;
            lfs::stage::indexed_pointers(&repo)
        };
        if pointers.is_empty() {
            // Nothing to join a ledger against, and the `SELECT` would be a
            // query issued to answer about no rows.
            return Ok(Vec::new());
        }
        let ledger = self.with_db(|conn| db::materialized_rows(conn, &profile.id))?;
        Ok(lfs::listing::collect(
            &profile.local_path,
            &pointers,
            &ledger,
        ))
    }

    /// Ask for one virtual path's content (Story 56.3, FR-338, AD-128).
    ///
    /// The engine's single entry point for on-demand hydration, and both doors
    /// onto it — `keeper-syncd materialize` and the `sync_materialize_entry`
    /// command — call exactly this and add no policy of their own. Three
    /// answers, all of them `Ok`: the content was published here and now, it
    /// was already there, or a transfer was queued and the id of the unit that
    /// will deliver it is on the [`lfs::hydrate::Materialization`].
    ///
    /// # Nothing here is a second implementation
    ///
    /// [`lfs::stage::materialize`] stays the only publish,
    /// [`git::repo::refresh_index_stat`] the only index-stat repair,
    /// [`WorkKind::LfsDownload`] the only transfer and
    /// [`Self::materialize_landed`] the only completion arm. The per-path
    /// decision is [`lfs::hydrate::plan`]'s, the containment rule is
    /// [`crate::browse::plain_segments`]'s, and the queue is the journal's.
    ///
    /// Deliberately **not** [`Self::materialize_pending`], the whole-tree
    /// sweep: its own doc records the 40 hours lost to calling it per object,
    /// and a request for one path must cost one path.
    ///
    /// # The order of the checks is the contract
    ///
    /// Containment first, so an escaping subpath is refused whatever state the
    /// folder is in and the refusal cannot be probed for. Then the mode, which
    /// needs no disk. Then the repository — opened with
    /// [`git::repo::open`] and never [`Self::open_repo`], which for a folder
    /// that is not a repository yet clones over the network or adopts in place:
    /// a verb sold as "give me this file" must not be able to pull gigabytes or
    /// turn a plain folder into a keeper repository, which is the high-severity
    /// defect 56.2's review found in the sibling listing. Then the index, the
    /// cone, and only then the worktree.
    ///
    /// # One operation at a time, within this process
    ///
    /// Takes [`Self::reserve`], and a busy profile answers
    /// [`SyncError::Busy`] exactly as [`Self::sync_once`] does. The reservation
    /// is released on every exit path including a panic, and clears the
    /// progress phase with it (see `Reservation::drop`).
    ///
    /// It is an **in-process** exclusion, and the honest limit is worth writing
    /// down here because this is the first verb designed to be run *while* a
    /// daemon runs: `keeper-syncd materialize` is a different process from
    /// `keeper-syncd watch`, each with its own reservation map, so nothing
    /// stops the two from writing the index at the same time. Ordinary use is
    /// the app door, where the engine is a singleton and the exclusion is real.
    /// A cross-process lock is a change to every one-shot verb, not to this
    /// one (recorded in `deferred-work.md`).
    ///
    /// Blocking, like every other repository caller: a `gix::Repository` is
    /// neither `Send` nor cheap to hold, so each open is scoped to a block and
    /// never spans an await point.
    pub fn materialize_entry(
        &self,
        id: &str,
        subpath: &str,
    ) -> Result<lfs::hydrate::Materialization> {
        use lfs::hydrate::{ContentRefusal, Materialization, MaterializeOutcome, Plan};

        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };

        // The one lexical containment rule in the crate (AD-65), carried across
        // verbatim rather than restated. Ahead of the reservation, so a subpath
        // that may not be asked for is refused whatever the folder is doing —
        // and it costs no disk, so there is nothing to serialize yet.
        let segments = crate::browse::plain_segments(subpath)
            .map_err(ContentRefusal::from)
            .map_err(SyncError::Refused)?;
        if segments.is_empty() {
            // `plain_segments` answers the profile ROOT with no segments, which
            // is correct for a listing and meaningless here: a folder has no
            // content to materialize. Refused as an escape because that is what
            // asking for the root as a file is — browse's own sentence names the
            // subpath that was asked for.
            return Err(SyncError::Refused(ContentRefusal::Escapes(
                crate::browse::BrowseRefusal::Escapes {
                    subpath: subpath.to_owned(),
                },
            )));
        }
        // The other half of the same rule, and this verb needs it more than the
        // read verbs that already run it: `plain_segments` is lexical, so it
        // cannot see a *parent* component replaced by a symlink pointing out of
        // the folder — and this is a write. `browse::resolve` canonicalizes
        // both ends and refuses after resolution (AD-59). `Ok(None)` is a path
        // that is simply not on disk, which is [`ContentRefusal::Missing`]'s
        // business a few lines down, not an escape.
        crate::browse::resolve(&profile.local_path, subpath)
            .map_err(ContentRefusal::from)
            .map_err(SyncError::Refused)?;
        let rela = lfs::hydrate::joined(&segments);
        let path = rela.to_string_lossy().into_owned();

        // A paused folder is one keeper does not touch — the rule
        // `commit_local` and the watcher both state — and a request is no
        // exception: the bytes would land in a working tree the user took
        // keeper's hands off, and a queued unit would never be claimed, because
        // the tick skips a disabled profile entirely. Saying so is the only
        // honest answer available.
        if !profile.enabled {
            return Err(SyncError::Refused(ContentRefusal::Paused {
                profile: profile.name.clone(),
            }));
        }
        // An unplugged volume is absence, never failure (AD-48). Without this
        // the `.git` test below reports the folder as tracking nothing, which
        // tells the user their file is not tracked when their drive is merely
        // out.
        if !self.volume_ready(&profile)? {
            return Err(SyncError::MediaAbsent);
        }

        let _reservation = self
            .reserve(&profile.id)
            .ok_or_else(|| SyncError::Busy(profile.name.clone()))?;
        // Before the disk, because it needs none. A listing deliberately
        // ignores the mode; a write must not — see
        // [`ContentRefusal::LfsDisabled`].
        if profile.lfs_mode == LfsMode::Disabled {
            return Err(SyncError::Refused(ContentRefusal::LfsDisabled {
                profile: profile.name.clone(),
            }));
        }
        // A folder with no `.git` has no index, therefore no committed pointer,
        // therefore nothing to ask for — and creating one is somebody else's
        // verb, which is the whole reason this does not use `open_repo`.
        if !profile.local_path.join(".git").exists() {
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        }
        let indexed = {
            // Removable media is opened with full trust for the reason
            // `open_repo` states (AD-48); the difference is that this opens what
            // is there and never makes one.
            let repo = git::repo::open(&profile.local_path, profile.removable)?;
            lfs::stage::indexed_pointer(&repo, &rela)
        };
        let Some(indexed) = indexed else {
            // A plain tracked file and a path git does not track land here
            // together, deliberately: neither has a committed pointer.
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        };
        // The same cone `materialize_landed` applies, so a request cannot reach
        // content the profile exists to keep away (Story 27.2).
        if !SparseCone::new(&profile.subpaths).includes(&rela) {
            return Err(SyncError::Refused(ContentRefusal::OutsideSubpaths { path }));
        }

        let smudge = match lfs::hydrate::plan(&profile.local_path, &rela, &indexed)
            .map_err(SyncError::Refused)?
        {
            Plan::AlreadyHeld => {
                // The content is here, and this is also the only verb that can
                // notice its index entry still describing the pointer — after a
                // crash between the publish and the re-stat, or after a `git
                // lfs pull` that keeper never saw. `refresh_index_stat` writes
                // only when an entry's stat has actually gone stale, so for an
                // ordinary already-materialized path this reads the index and
                // stops. The alternative is a path that reads MODIFIED forever
                // and cannot be repaired by asking for it again.
                {
                    let repo = git::repo::open(&profile.local_path, profile.removable)?;
                    git::repo::refresh_index_stat(&repo, std::slice::from_ref(&rela))?;
                }
                return Ok(Materialization {
                    path,
                    oid: indexed.oid,
                    size_bytes: indexed.size,
                    outcome: MaterializeOutcome::AlreadyMaterialized,
                    unit_id: None,
                });
            }
            Plan::Publish(smudge) => smudge,
        };

        // Not `ensure_layout`: this reads the store to decide, and a verb that
        // created the store's directory tree on the way to refusing would be
        // writing to answer a question.
        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        if !store.contains(&indexed.oid, indexed.size) {
            // The object is not here. The journal is the queue, and
            // `enqueue_unique` returns whatever unit already covers this work —
            // so a repeat request is one row and one download, and a request for
            // an object already in flight is covered by
            // `WorkKind::covered_while_running`.
            let unit = WorkKind::LfsDownload {
                oid: indexed.oid.clone(),
                size: indexed.size,
            };
            let now = self.platform.now_ms();
            let unit_id = self.with_db(|conn| {
                let unit_id = db::enqueue_unique(conn, &profile.id, &unit, now, now)?;
                // The path is carried BESIDE the unit rather than inside it, and
                // so is the urgency: the payload is what `enqueue_unique`
                // deduplicates on, so either one folded into it would re-fetch
                // bytes this machine is already getting (`db::promote_unit`).
                db::label_unit(conn, unit_id, &path)?;
                // Marks the row and makes it claimable now: the covering row
                // may be one a backoff or an absent volume deferred, and
                // urgency on a row `claim_ready` cannot see is worth nothing.
                db::promote_unit(conn, unit_id, now)?;
                Ok(unit_id)
            })?;
            // The supervisor claims it, not this call: `keeper-syncd` has no
            // event loop to wait on and draining inline would make urgency
            // pointless. In this process that collapses the wait to the next
            // tick; from the CLI, whose engine is its own, the running daemon
            // picks the row up on its own next pass instead.
            self.wake_now(&profile.id);
            tracing::info!(
                profile = profile.name,
                path = path,
                unit = unit_id,
                "queued a requested LFS download"
            );
            return Ok(Materialization {
                path,
                oid: indexed.oid,
                size_bytes: indexed.size,
                outcome: MaterializeOutcome::Queued,
                unit_id: Some(unit_id),
            });
        }

        // One frame of the phase the queued branch already publishes, so a
        // local copy of a four-gigabyte object is not silent. No new phase, no
        // new field, no new channel.
        let mut event = self.progress(&profile, SyncPhase::DownloadingLfs);
        event.current = Some(path.clone());
        self.publish(event);

        lfs::stage::materialize(&store, &profile.local_path, &smudge)?;
        // The one moment this is knowable: content for this path now exists
        // here, so the next object for it is a replacement rather than an
        // arrival.
        let now = self.platform.now_ms();
        self.with_db(|conn| db::remember_materialized(conn, &profile.id, &path, now))?;
        // Arrived from upstream — this branch publishes an object over a
        // pointer — so the remote-origin clock applies and starts now (Story
        // 56.5, AD-131). `path` is already `index_key`'s spelling. The identity
        // rides along because this is the one moment the pointer is in hand:
        // `RELEASE_BUDGET_BYTES` can only bound a pass over rows that recorded
        // a size, and re-deriving it later means another index read per row.
        self.with_db(|conn| {
            db::note_arrival(conn, &profile.id, &path, now, &indexed.oid, indexed.size)
        })?;
        {
            // The worktree file just changed size, so its index entry carries
            // the pointer's stat and status would call it modified. Re-stat that
            // ONE path — `repair_index_stat` is the whole-repository button and
            // this is not it. Writing the index through a read-only open is the
            // existing `repair_stat_invariant` precedent.
            let repo = git::repo::open(&profile.local_path, profile.removable)?;
            git::repo::refresh_index_stat(&repo, std::slice::from_ref(&smudge.path))?;
        }
        tracing::info!(
            profile = profile.name,
            path = path,
            "materialized LFS content on request"
        );
        Ok(Materialization {
            path,
            oid: indexed.oid,
            size_bytes: indexed.size,
            outcome: MaterializeOutcome::Materialized,
            unit_id: None,
        })
    }

    /// Release one path's content back to the pointer this folder committed
    /// (Story 56.4, FR-332, FR-333).
    ///
    /// [`Self::materialize_entry`]'s inverse and the only entry point onto it.
    /// It writes the **committed blob byte for byte** through a sibling temp
    /// file and one `rename` ([`lfs::stage::dehydrate`]), forgets the ledger
    /// row, and re-stats that one index entry so the path reads clean.
    ///
    /// # This is the verb where a mistake destroys data
    ///
    /// So `Ok` means, and only means, "content was here, it is provably
    /// elsewhere, and it is now gone from this machine". Everything else is a
    /// [`lfs::hydrate::ContentRefusal`] the caller distinguishes by type —
    /// including [`lfs::hydrate::ContentRefusal::AlreadyPointer`], which
    /// `keeper-syncd` renders as a no-op at exit 0. One uniform contract beats
    /// an outcome enum whose "nothing happened" arm a caller can forget to
    /// check.
    ///
    /// # Only a folder, and a path, that is meant to hold pointers
    ///
    /// The gate is now two (Story 56.10). [`release_mode_gate`] asks whether
    /// this folder authorizes anything at all to stay away, and
    /// [`release_path_gate`] asks whether it authorizes **this path**, at the
    /// committed pointer's size, inside [`Self::release_resolved`].
    ///
    /// [`lfs::hydrate::ContentRefusal::AlwaysMaterializes`] is what either one
    /// refuses with, and the reason it ever existed was never the mode: in the
    /// default mode [`Self::materialize_pending`] re-materializes any tracked
    /// path inside the cone whose worktree holds a pointer — copying it back out
    /// of this machine's object store, or queueing a download of it — so a
    /// release there gave back nothing and cost a fetch, and a sweep driving
    /// this verb would fight the scan loop forever. So
    /// [`LfsMode::Materialize`] with **no** virtualization policy still refuses
    /// by name and for exactly that reason, while `LfsMode::Materialize` with a
    /// policy that names this path no longer does: Story 56.10 taught
    /// [`Self::materialize_pending`] the same policy, the two verbs now agree
    /// about this path, and the release is not undone on the next pass.
    ///
    /// # The order of the guards is the contract
    ///
    /// The free checks first, exactly as [`Self::materialize_entry`] has them:
    /// containment ([`release_target`]), the pause, the volume, the
    /// reservation, then the folder's mode and whether it has a policy at all
    /// ([`release_mode_gate`]) — and then, in [`Self::release_resolved`], the
    /// index, the cone, that folder's policy for **this path**
    /// ([`release_path_gate`]) and every release-specific guard, which is where
    /// they are listed and where they run.
    ///
    /// The split is Story 56.5's and it is what lets the release sweep run
    /// **every** one of those refusals without re-taking a reservation the
    /// sync tick is already holding: the sweep calls
    /// [`Self::release_mode_gate`]'s free counterpart and
    /// [`Self::release_resolved`] directly, so there is exactly one copy of the
    /// chain that deletes content. This door keeps its guards in this order
    /// because that order is what a caller arriving from outside — a CLI verb,
    /// an IPC command — is owed.
    pub async fn dehydrate_entry(&self, id: &str, subpath: &str) -> Result<lfs::hydrate::Release> {
        use lfs::hydrate::ContentRefusal;

        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };

        // The crate's one lexical containment rule (AD-65), ahead of the
        // reservation for `materialize_entry`'s reason: a subpath that may not
        // be asked for is refused whatever the folder is doing.
        let (rela, path) = release_target(&profile, subpath)?;

        if !profile.enabled {
            return Err(SyncError::Refused(ContentRefusal::Paused {
                profile: profile.name.clone(),
            }));
        }
        // An unplugged volume is absence, never failure (AD-48) — and never a
        // reason to delete anything: the content is not gone, the drive is out.
        if !self.volume_ready(&profile)? {
            return Err(SyncError::MediaAbsent);
        }

        let _reservation = self
            .reserve(&profile.id)
            .ok_or_else(|| SyncError::Busy(profile.name.clone()))?;
        // The gate compiles the folder's virtualization policy on the one mode
        // that needs it and hands it back, so the mode's own answer — a folder
        // with large-file support off — is never displaced by a fault in a file
        // that is inert for it, and `PointerOnly` reads nothing at all.
        let policy = release_mode_gate(&profile)?;
        self.release_resolved(&profile, policy.as_ref(), &rela, path)
            .await
    }

    /// Every release-specific guard, and the release itself, for one path whose
    /// door has already been passed (Story 56.4, extracted in Story 56.5).
    ///
    /// `rela` is the repository-relative path [`release_target`] resolved and
    /// `path` is its `index_key` spelling; the caller has already taken the
    /// profile's reservation, checked the pause and the volume, and passed
    /// [`release_mode_gate`]. Both callers — [`Self::dehydrate_entry`] and
    /// [`Self::release_expired`] — run this identical body, which is the point:
    /// **the sweep is not a privileged caller**, and a second copy of a chain
    /// that deletes content is the one duplication this epic cannot afford.
    ///
    /// # The order of the guards is the contract
    ///
    /// After the index and the cone, the folder's mode and its policy for this
    /// path ([`release_path_gate`]) — free, and the last question that is about
    /// the path rather than about the file. Then the release-specific guards,
    /// cheapest first:
    ///
    /// 1. **`Missing` / `Modified`** from one `lstat`.
    /// 2. **`AlreadyPointer`**, at most a kilobyte of read, so the common
    ///    already-released path costs nothing further. A `size 0` pointer lands
    ///    here too: it stands for the empty object, so there is no content on
    ///    this machine to release, and every guard below it is degenerate for
    ///    one.
    /// 3. **`Pinned`**, one `SELECT`. The hard floor goes before the two
    ///    expensive proofs: a pinned path must not pay for a full hash or a
    ///    round trip to be told no.
    /// 4. **`Modified` by content identity** — length, then SHA-256 against the
    ///    committed oid. A stat comparison would wave through a same-length
    ///    edit written back with the original mtime and then delete it; see
    ///    [`lfs::stage::content_oid`].
    /// 5. **`UnprovenOnRemote`**, the per-object proof taken at the moment of
    ///    the deletion ([`Self::remote_serves`]). A stored `synced_at_ms` — the
    ///    memo Story 56.5's sweep selects candidates by — authorizes
    ///    *eligibility* and never a deletion; NFR-40 rests on this line.
    /// 6. **`Open` / `OpenUnknown`**, last, because it is the answer most
    ///    likely to have changed while the steps above ran.
    /// 7. **`Pinned`, again**, immediately before the deletion. NFR-40 puts the
    ///    authorization at the moment of the deletion, and step 3 happened
    ///    before a whole-file hash and a round trip — the two slowest things
    ///    here. One indexed `SELECT` on a two-column primary key is what "at
    ///    the moment of the deletion" costs.
    ///
    /// One consequence of placing the path gate there, stated rather than
    /// discovered: under [`LfsMode::Materialize`], a stale ledger row for a path
    /// no pattern names is no longer retracted by [`Self::release_expired`]'s
    /// `AlreadyPointer` arm, because the path is refused before that arm can be
    /// reached. Not a regression — before Story 56.10 no folder in that mode
    /// reached this function at all — and the row is overwritten by the next
    /// materialization of that path.
    ///
    /// The local store is deliberately **not** a precondition, which is the one
    /// place this does not mirror [`Self::materialize_entry`]: that verb copies
    /// *from* the store, so absence is fatal there. Here `lfs_prune_local`
    /// deletes the store copy precisely when the worktree holds the content
    /// (`lfs::prune`'s condition 2), which is the definition of a materialized
    /// path — so requiring it would refuse in the commonest state of all.
    /// NFR-40 therefore rests entirely on the remote proof, which is where
    /// NFR-40 puts it.
    ///
    /// # Once the content is gone, the deletion is the fact
    ///
    /// The rename is the point of no return, so nothing after it may turn a
    /// completed release into a reported failure: a locked `sync.db` or a held
    /// `index.lock` would otherwise make this exit non-zero with the bytes
    /// already deleted, contradicting the contract `docs/sync.md` and
    /// `keeper-syncd`'s own `dehydrate` state — every refusal exits 1 and
    /// changes nothing on disk. Both bookkeeping steps therefore log at `warn!`
    /// and the release still succeeds. Neither loss is permanent: a stale
    /// ledger row is overwritten by the next materialization of that path, and
    /// a skipped re-stat is repaired by asking to release the path again —
    /// which is what the `AlreadyPointer` arm's own `refresh_index_stat` is
    /// there for, since `git::repo::repair_index_stat` only refreshes entries
    /// whose worktree file is **not** a pointer and so cannot reach a released
    /// one.
    ///
    /// [`git::repo::open`], never [`Self::open_repo`]: a verb sold as "let this
    /// file go" must not be able to clone a repository or adopt a plain folder.
    /// Every [`gix::Repository`] lives in a scoped block that ends before any
    /// `.await`, because one is neither `Send` nor cheap to hold and this
    /// method is `async` for the batch round trip.
    async fn release_resolved(
        &self,
        profile: &SyncProfile,
        policy: Option<&lfs::virtual_policy::VirtualPolicy>,
        rela: &Path,
        path: String,
    ) -> Result<lfs::hydrate::Release> {
        use crate::platform::OpenFileState;
        use lfs::hydrate::{ContentRefusal, Release};

        // No `.git` means no index, so no committed pointer and nothing to
        // release — and making one is somebody else's verb.
        if !profile.local_path.join(".git").exists() {
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        }
        let committed = {
            let repo = git::repo::open(&profile.local_path, profile.removable)?;
            // The blob's own BYTES, not a re-rendering of the parsed pointer: a
            // non-canonical committed pointer renders to a different blob hash
            // and a phantom modification forever (AD-124).
            lfs::stage::indexed_pointer_blob(&repo, rela)
        };
        let Some((pointer, blob)) = committed else {
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        };
        if !SparseCone::new(&profile.subpaths).includes(rela) {
            return Err(SyncError::Refused(ContentRefusal::OutsideSubpaths { path }));
        }
        // The folder's mode and its policy, for THIS path, at the committed
        // pointer's size — the first point at which the honest size is in hand,
        // and free, because the blob has just been read. It sits with the cone
        // rather than among the guards below because it asks the same class of
        // question — is this path in scope for this verb at all — and because
        // its refusal is the useful one: telling the owner of a path no pattern
        // names that his file is `Modified` would send him looking for an edit.
        release_path_gate(profile, policy, rela, pointer.size)?;

        let absolute = profile.local_path.join(rela);
        let metadata = match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(SyncError::Refused(ContentRefusal::Missing { path }));
            }
            // A path keeper cannot even SEE is not a path it can describe. An
            // EACCES on the parent, an ELOOP, an EIO off a failing disk is not
            // a modification, and `Modified`'s sentence — your file does not
            // hold the content this folder committed — would be false and would
            // send the owner looking for an edit instead of at their
            // permissions or their disk. Nothing has been written on this
            // branch, so the honest answer costs nothing.
            Err(err) => return Err(SyncError::io("stat the path to release", &absolute, err)),
        };
        // `is_file` rather than `!is_dir`: a fifo, a socket or a device node has
        // a length that is not a number of bytes anyone can read out of it, and
        // a directory standing here is something a user put there.
        if !metadata.is_file() {
            return Err(SyncError::Refused(ContentRefusal::Modified { path }));
        }
        // Nothing to release. Checked against the COMMITTED oid, so pointer
        // text for some other object is not mistaken for this one and falls to
        // the content-identity guard below as the modification it is.
        if lfs::stage::worktree_pointer(&absolute, &metadata)
            .is_some_and(|found| found.oid == pointer.oid)
        {
            // This is also the only verb that can notice a released path whose
            // index entry still describes the content it used to hold — after a
            // crash between the rename and the re-stat, or an `index.lock` held
            // by somebody else at that instant. `git::repo::repair_index_stat`
            // cannot reach it: that one only refreshes entries whose worktree
            // file is NOT a pointer. So asking again is the repair, exactly as
            // `materialize_entry`'s `AlreadyHeld` arm is for the other
            // direction, and for the same reason: without it the path reads
            // MODIFIED forever and every status walk pays a full content
            // comparison for it. `refresh_index_stat` writes only when an
            // entry's stat has actually gone stale, so the ordinary
            // already-released path reads the index and stops.
            {
                let repo = git::repo::open(&profile.local_path, profile.removable)?;
                git::repo::refresh_index_stat(&repo, &[rela.to_path_buf()])?;
            }
            return Err(SyncError::Refused(ContentRefusal::AlreadyPointer { path }));
        }
        // A pointer spelling `size 0` stands for the empty object, so there is
        // no content on this machine to release — and every guard below is
        // degenerate for one: `len != size` passes for any empty file,
        // `content_oid` of an empty file IS that pointer's oid, and
        // `stage::dehydrate` would accept any zero-byte file and write ~90
        // bytes over it while reporting `sizeBytes: 0`. Carved out here rather
        // than left to the accident that `lfs::audit::tracked_objects` skips
        // size-0 objects, which is that function's own carve-out for its own
        // reason: "the empty object is not content anyone can lose".
        if pointer.size == 0 {
            return Err(SyncError::Refused(ContentRefusal::AlreadyPointer { path }));
        }
        if self.with_db(|conn| db::is_pinned(conn, &profile.id, &path))? {
            return Err(SyncError::Refused(ContentRefusal::Pinned { path }));
        }

        // One frame of an existing phase, because what follows is a hash of a
        // possibly-enormous file and a round trip. No new phase, no new field.
        let mut event = self.progress(profile, SyncPhase::Verifying);
        event.current = Some(path.clone());
        self.publish(event);

        // Content identity. The length short-circuits the same predicate rather
        // than being a second guard, and the digest is what a same-length,
        // same-mtime edit cannot survive.
        if metadata.len() != pointer.size {
            return Err(SyncError::Refused(ContentRefusal::Modified { path }));
        }
        let (found_oid, read) = lfs::stage::content_oid(&absolute)?;
        if found_oid != pointer.oid || read != pointer.size {
            return Err(SyncError::Refused(ContentRefusal::Modified { path }));
        }

        if !self
            .remote_serves(profile, &pointer.oid, pointer.size)
            .await
        {
            return Err(SyncError::Refused(ContentRefusal::UnprovenOnRemote {
                path,
            }));
        }
        match self.platform.open_file_state(&absolute) {
            OpenFileState::Closed => {}
            OpenFileState::Open => {
                return Err(SyncError::Refused(ContentRefusal::Open { path }));
            }
            // The trait default, and therefore the answer on every real host
            // today. Fail-closed is the whole point (AD-125).
            OpenFileState::Unknown => {
                return Err(SyncError::Refused(ContentRefusal::OpenUnknown { path }));
            }
        }
        // The pin again, with nothing between this and the deletion. See this
        // method's guard list, step 7.
        if self.with_db(|conn| db::is_pinned(conn, &profile.id, &path))? {
            return Err(SyncError::Refused(ContentRefusal::Pinned { path }));
        }

        lfs::stage::dehydrate(
            &profile.local_path,
            &lfs::stage::PendingRelease {
                path: rela.to_path_buf(),
                pointer: pointer.clone(),
                blob,
                // The very `lstat` the content proof was taken against, so the
                // publish can require that this is still the same file.
                guard: crate::stability::FileSample::from_metadata(&metadata),
            },
        )?;
        // The content is gone. From here the deletion is the fact and the
        // bookkeeping is best-effort — see this method's doc.
        if let Err(err) = self.with_db(|conn| db::forget_materialized(conn, &profile.id, &path)) {
            tracing::warn!(
                profile = profile.name,
                path = path,
                error = %err,
                "released the content, but could not retract the ledger row: this folder will \
                 keep claiming it holds the content until that path is materialized again"
            );
        }
        // The worktree file just shrank to ~130 bytes, so its index entry
        // carries the content's stat and status would call it modified.
        // Re-stat that ONE path — `repair_index_stat` is the whole-repository
        // button and this is not it.
        let restat = {
            let opened = git::repo::open(&profile.local_path, profile.removable);
            opened.and_then(|repo| git::repo::refresh_index_stat(&repo, &[rela.to_path_buf()]))
        };
        if let Err(err) = restat {
            tracing::warn!(
                profile = profile.name,
                path = path,
                error = %err,
                "released the content, but could not refresh the index entry: git will report \
                 this path as modified until somebody asks to release it again, which repairs it"
            );
        }
        tracing::info!(
            profile = profile.name,
            path = path,
            oid = pointer.oid,
            size = pointer.size,
            "released LFS content on request"
        );
        Ok(Release {
            path,
            oid: pointer.oid,
            size_bytes: pointer.size,
        })
    }

    /// Record that somebody just read this path's content through keeper
    /// (Story 56.5, AD-126).
    ///
    /// The remote-origin release clock, and the reason it is keeper's own
    /// timestamp rather than the filesystem's `atime`: Linux has defaulted to
    /// `relatime` since 2.6.30, so `atime` on a file read twice an hour is a
    /// day stale, and on a `noatime` mount it never moves at all.
    ///
    /// **Best-effort and infallible by signature.** Every caller is on a read
    /// path — an open, a text or document read, an export, the head of a media
    /// stream — and none of them may fail because a memo could not be written.
    /// A path with no ledger row writes nothing ([`db::note_use`] is
    /// UPDATE-only) and is not an error: no row means this clone never recorded
    /// holding content here, so there is no clock to move.
    ///
    /// It takes no reservation and touches no file. The lexical validation is
    /// [`release_target`]'s, shared so that the key this writes is the key the
    /// sweep will later read — a `\`-joined spelling would write a row nothing
    /// ever consults.
    pub fn note_use(&self, id: &str, subpath: &str) {
        let logged = |what: &str, err: &SyncError| {
            tracing::debug!(
                profile = id,
                subpath = subpath,
                error = %err,
                "{what}",
            );
        };
        let profile = match self.with_db(|conn| db::get_profile(conn, id)) {
            Ok(Some(profile)) => profile,
            Ok(None) => return,
            Err(err) => {
                logged("could not read the profile to record a use", &err);
                return;
            }
        };
        let path = match release_target(&profile, subpath) {
            Ok((_, path)) => path,
            Err(err) => {
                logged("declined to record a use of this subpath", &err);
                return;
            }
        };
        let now = self.platform.now_ms();
        if let Err(err) = self.with_db(|conn| db::note_use(conn, &profile.id, &path, now)) {
            logged("could not record a use", &err);
        }
    }

    /// Set or clear the owner's standing instruction that this path's content
    /// stays on this machine (Story 56.5, FR-334).
    ///
    /// The writer [`db::is_pinned`] has been reading for since Story 56.2 —
    /// without it 56.4's hard floor is a guard nothing in production can ever
    /// arm, and the release sweep this story adds would have no absolute
    /// override at all.
    ///
    /// # What it checks, and what it deliberately does not
    ///
    /// It refuses a path this folder does not track
    /// ([`lfs::hydrate::ContentRefusal::NotTracked`]) and a path outside the
    /// sparse cone ([`lfs::hydrate::ContentRefusal::OutsideSubpaths`]), so a pin
    /// is never recorded against a path no sweep will ever look at — one index
    /// read for both. The cone check is not redundant with the tracked one: a
    /// skip-worktree entry still yields a pointer, so without it this verb
    /// exited 0 for a path [`Self::release_resolved`] refuses forever.
    ///
    /// It refuses a detached removable volume with [`SyncError::MediaAbsent`],
    /// for [`Self::dehydrate_entry`]'s reason: an unplugged volume is absence,
    /// never failure (AD-48), and never a folder that stopped tracking things.
    ///
    /// It does **not** require the content to be present, and it takes no
    /// reservation. A pin is a standing instruction about a
    /// path rather than an operation on content: requiring the bytes would mean
    /// an owner cannot pre-pin a path they are about to materialize, and would
    /// put a whole guard chain behind a single `UPDATE`. Nothing here writes to
    /// the worktree, so there is nothing for a reservation to serialize against.
    ///
    /// Idempotent: pinning a pinned path is the same answer as pinning an
    /// unpinned one, which is why [`lfs::hydrate::Pin`] reports a state rather
    /// than an outcome.
    pub fn pin_entry(&self, id: &str, subpath: &str, pinned: bool) -> Result<lfs::hydrate::Pin> {
        use lfs::hydrate::{ContentRefusal, Pin};

        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        let (rela, path) = release_target(&profile, subpath)?;
        // An unplugged volume is absence, never failure (AD-48), and it must be
        // said before the `.git` probe below — which is a filesystem question
        // whose answer for a detached removable folder is `false`. Without this
        // the operator was told `NotTracked`: that this folder does not track a
        // path it does track, sending them to look at their `.gitattributes`
        // instead of at the drive that is out. `dehydrate_entry` answers the
        // identical state with `MediaAbsent`, and the two verbs must not
        // describe one folder two ways.
        if !self.volume_ready(&profile)? {
            return Err(SyncError::MediaAbsent);
        }
        // No `.git` means no index, so nothing this folder tracks — and making
        // one is somebody else's verb, exactly as it is for a release.
        if !profile.local_path.join(".git").exists() {
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        }
        let tracked = {
            let repo = git::repo::open(&profile.local_path, profile.removable)?;
            lfs::stage::indexed_pointer_blob(&repo, &rela).is_some()
        };
        if !tracked {
            return Err(SyncError::Refused(ContentRefusal::NotTracked { path }));
        }
        // Beside the tracked check and for the same reason. A skip-worktree
        // index entry still yields a pointer, so a path outside the sparse cone
        // reads as tracked — and `release_resolved` refuses it
        // `OutsideSubpaths` forever. Without this, `pin` exited 0 and inserted a
        // row claiming this machine holds content the sparse checkout
        // guarantees it does not, against a path no release will ever consider:
        // exactly the promise nothing keeps that this verb's own doc says a pin
        // must never be.
        if !SparseCone::new(&profile.subpaths).includes(&rela) {
            return Err(SyncError::Refused(ContentRefusal::OutsideSubpaths { path }));
        }
        let now = self.platform.now_ms();
        self.with_db(|conn| db::set_pinned(conn, &profile.id, &path, pinned, now))?;
        tracing::info!(
            profile = profile.name,
            path = path,
            pinned = pinned,
            "recorded a pin instruction",
        );
        Ok(Pin { path, pinned })
    }

    /// Re-publish anything the server is missing that this machine can still
    /// supply (DW-208).
    ///
    /// # Why nothing else does this
    ///
    /// `WorkKind::LfsUpload` is enqueued in exactly one place — `commit_local`,
    /// for files being *freshly staged in that commit*. That is correct for the
    /// path it serves and it means an obligation, once dropped, is never picked
    /// up again: a file that is committed and unchanged produces no staging, so
    /// no upload, so a pointer whose object silently never reached the server
    /// stays that way forever. [`Self::rescan`] cannot help either — it forgets
    /// the remembered tree state and asks for a walk, and a walk finds nothing
    /// to stage in a file that has not changed.
    ///
    /// Measured on a real folder: 4 objects, 1.69 GB, committed 2026-08-10 and
    /// still absent from the server a week later, through dozens of syncs,
    /// several restarts and a "Recheck all files". Nothing in the engine was
    /// ever going to try again.
    ///
    /// [`Self::audit_remote_objects`] is the half that *finds* them. This is
    /// the half that fixes the ones that are fixable, and it is deliberately
    /// separate: detection is read-only and safe to run anywhere, repair moves
    /// gigabytes.
    ///
    /// # What "can still supply" means
    ///
    /// Two sources, in order of cost. The object may still be in this machine's
    /// own store, in which case the upload is queued and the bytes are already
    /// there. Otherwise the worktree file may *be* the content: re-cleaning it
    /// hashes it back into the store and proves it, because a file whose digest
    /// no longer matches the pointer is a different file and publishing it
    /// under that oid would be a lie.
    ///
    /// Anything neither source can produce is reported, not papered over. Those
    /// bytes exist on some other machine or nowhere, and the only thing that
    /// helps is a human knowing which files to go looking for.
    pub async fn republish_missing_objects(&self, id: &str) -> Result<RepublishReport> {
        let audit = self.audit_remote_objects(id).await?;
        let mut report = RepublishReport {
            missing: audit.missing.len() as u64,
            ..RepublishReport::default()
        };
        if audit.is_intact() {
            return Ok(report);
        }
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };

        let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
        store.ensure_layout()?;
        let now = self.platform.now_ms();
        // One upload per object, however many paths name it.
        let mut queued: std::collections::HashSet<String> = std::collections::HashSet::new();
        for object in &audit.missing {
            if queued.contains(&object.oid) {
                continue;
            }
            let absolute = profile.local_path.join(&object.path);
            let have = store.contains(&object.oid, object.size) || {
                // Not in the store. The worktree file may still be the content
                // — that is the ordinary state on the machine that recorded it
                // — so hash it back in and check what came out.
                //
                // Unless the worktree holds the pointer that names THIS
                // object, which since Epic 56 is the ordinary state of an
                // authorized virtual path. Asked BEFORE the call and not
                // around its answer, because `stage::clean` is not a probe: it
                // streams the file into the store, so re-cleaning ~130 bytes
                // of pointer text would deposit a junk object under the
                // pointer text's own sha — an object no pointer names, that
                // `prune` will not remove for want of a reference — and then
                // compare that sha to the oid it stands for, which can never
                // match. No read, no store write, no comparison.
                //
                // Only that pointer, and the oid comparison is what keeps the
                // gate the width of the sentence. A tracked file whose genuine
                // content IS pointer text — this project's own LFS
                // documentation under a small `lfs_threshold_bytes`, a test
                // fixture that holds an example pointer — is recoverable
                // exactly as it always was, and calling it beyond this machine
                // would be the loudest warning the product prints, told about
                // a file sitting right there.
                let holds_the_pointer_for_this_object = std::fs::metadata(&absolute)
                    .ok()
                    .and_then(|meta| lfs::stage::worktree_pointer(&absolute, &meta))
                    .is_some_and(|pointer| pointer.oid == object.oid);
                if holds_the_pointer_for_this_object {
                    false
                } else {
                    match lfs::stage::clean(&store, &absolute) {
                        Ok(pointer) => pointer.oid == object.oid && pointer.size == object.size,
                        Err(err) => {
                            tracing::debug!(
                                path = %object.path,
                                error = %err,
                                "cannot rebuild a missing object from the worktree"
                            );
                            false
                        }
                    }
                }
            };
            if !have {
                report.unrecoverable.push(object.clone());
                continue;
            }
            let unit = WorkKind::LfsUpload {
                oid: object.oid.clone(),
                size: object.size,
            };
            self.with_db(|conn| db::enqueue_unique(conn, &profile.id, &unit, now, now))?;
            queued.insert(object.oid.clone());
            report.queued += 1;
            report.queued_bytes += object.size;
        }

        if report.queued > 0 {
            // Look now: the whole point of pressing the button is not to wait.
            self.wake_now(&profile.id);
            tracing::info!(
                profile = profile.name,
                queued = report.queued,
                bytes = report.queued_bytes,
                "re-publishing objects the server was missing"
            );
        }
        for object in &report.unrecoverable {
            self.warn(
                &profile.id,
                &profile.name,
                format!(
                    "{} is not on the server and its content is not on this machine — {} bytes that only another machine can still supply",
                    object.path, object.size
                ),
            );
        }
        Ok(report)
    }

    /// Re-verify stored content for a profile (Story 25.6, Story 56.6).
    ///
    /// # Why the remote proof is NOT taken here
    ///
    /// A pointer whose object is not in this machine's store is either damage
    /// or the entirely normal state of a folder that keeps pointers
    /// (Epic 56). Telling the two apart from bytes on this disk is what the
    /// index and the compiled policy do below; telling them apart *for
    /// certain* would need the server, and this verb deliberately does not ask
    /// it. [`Self::remote_serves`] is one batch round trip **per object**, so
    /// composing it per path would answer NFR-41's own fixture — a folder with
    /// 10 000 authorized virtual paths — with 10 000 round trips, and would
    /// break the contract `docs/sync.md` states: that plain `verify` is the
    /// half answerable without a network.
    ///
    /// The proof lives where it is already batched, index-driven and paid for
    /// once: [`Self::audit_remote_objects`] behind `--remote`, whose
    /// `missing_total` carries its own non-zero exit gate in `keeper-syncd`.
    /// The offline half excuses; the remote half condemns.
    ///
    /// # What earns an excuse
    ///
    /// Four facts, and every one of them free. The **index** says the bytes on
    /// disk are the pointer this repository committed; the compiled
    /// **policy** says that path is authorized to stay away; the object is
    /// **absent** rather than sitting here truncated; and where the folder's
    /// remote is a directory this machine can see
    /// ([`filesystem_remote_store`]) that store **holds** the object. The
    /// fourth is what keeps the division of labour honest for an external
    /// drive, whose remote half asks nothing. A folder whose `lfs_mode` is
    /// `Disabled` earns nothing: with LFS off, no pass will ever materialize
    /// the path, so calling its absent content normal would be a promise
    /// nothing keeps.
    pub async fn verify(&self, id: &str) -> Result<VerifyReport> {
        let Some(profile) = self.with_db(|conn| db::get_profile(conn, id))? else {
            return Err(SyncError::Config(format!("no such sync profile: {id}")));
        };
        self.publish(self.progress(&profile, SyncPhase::Verifying));

        // Everything the walk needs, moved in whole — the policy compiled
        // INSIDE the closure and not before it. `verify` takes no reservation,
        // so `Reservation::drop` cannot clean up after it and the `Idle`
        // publish at the tail is the only thing that does; a `?` between the
        // `Verifying` publish and that line leaves the tray spinning and a bar
        // on the card until the process ends. That is Story 34.8, and a config
        // typo (FR-329) is a new way to reach it.
        let walked = profile.clone();
        let report = tokio::task::spawn_blocking(move || -> Result<VerifyReport> {
            let root = walked.local_path.clone();
            // Compiled ONCE per call and never per path: the struct's own doc
            // forbids building a `GlobSet` inside a walk. A policy that cannot
            // be parsed fails the verb quoting the line (FR-329) rather than
            // being read as a permissive "nothing is virtual" — the direction
            // 56.5 settled for a folder config that could not be read.
            let policy = lfs::virtual_policy::VirtualPolicy::compile(&walked)?;
            let mut report = VerifyReport::default();
            let store = lfs::store::LfsStore::in_git_dir(root.join(".git"));
            // Whether ANY path in this folder could be excused, settled once
            // because both halves are facts about the folder rather than about
            // a path. No source said what may stay away, so nothing may; or
            // LFS is off entirely, in which case nothing will ever materialize
            // a pointer and calling the path's absent content normal would be
            // dishonest. Settling it here is also what keeps the ordinary
            // folder — the overwhelming majority, carrying no policy at all —
            // from paying an index parse plus one object-header lookup per
            // tracked path on every verify, the scheduled ones included.
            let excusable = policy.tier() != lfs::virtual_policy::VirtualPolicyTier::Unset
                && walked.lfs_mode != LfsMode::Disabled;
            // One index read for the whole walk. A folder with no repository,
            // or one whose index cannot be read, answers with an empty map and
            // therefore excuses NOTHING: the index is the only thing that can
            // tell a checkout's committed pointer from a pointer-shaped file
            // somebody saved by hand, and `VirtualPolicy::resolve` does no I/O
            // by design (FR-328) so it cannot.
            let indexed = if excusable {
                git::repo::open(&root, walked.removable)
                    .ok()
                    .map(|repo| lfs::stage::indexed_pointers(&repo))
                    .unwrap_or_default()
            } else {
                std::collections::BTreeMap::new()
            };
            // The third fact, taken here because it is free: a remote that is
            // a directory this machine can see is asked with a `stat`, not a
            // round trip. Resolved once, and only when an excuse is possible.
            let remote = if excusable {
                filesystem_remote_store(&walked)
            } else {
                None
            };
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
                                    // The excuse is earned by every fact that
                                    // is free, never by one (AD-129). The
                                    // policy answer is an authorization about
                                    // a pattern and a size — "this path *may*
                                    // stay away" — and the index is what
                                    // proves the bytes on disk are the pointer
                                    // this repository committed rather than a
                                    // stray file that happens to start with
                                    // `version https://git-lfs...`. Either
                                    // alone is a guess, and everything
                                    // unproven stays reported, word for word
                                    // as before.
                                    let rela = path.strip_prefix(&root).unwrap_or(&path);
                                    let committed = indexed
                                        .get(&lfs::stage::index_key(rela))
                                        .is_some_and(|entry| {
                                            entry.oid == pointer.oid && entry.size == pointer.size
                                        });
                                    let authorized = policy.resolve(rela, pointer.size)
                                        == lfs::virtual_policy::Virtualization::Virtual;
                                    // `contains` is length-verified, so an
                                    // object sitting here at the WRONG length
                                    // answered `false` above — a truncated
                                    // blob left by a `kill -9` or a
                                    // half-restored backup, which is the very
                                    // damage `LfsStore::contains` exists to
                                    // catch. Only a genuinely ABSENT object is
                                    // the normal state of a virtual path.
                                    let absent = !store.object_path(&pointer.oid).exists();
                                    // The third fact, and without it "the
                                    // offline half excuses, the remote half
                                    // condemns" is simply false for an
                                    // external drive: `audit_remote_objects`
                                    // short-circuits a filesystem remote to
                                    // `missing: []` without asking it
                                    // anything, so an object that vanished
                                    // from the drive would be reported by
                                    // nothing at all. When the drive is there
                                    // the proof costs a `stat`, so it is taken.
                                    // `None` — a remote that is a URL, or a
                                    // drive that is out (absence is never
                                    // failure, AD-48) — leaves the two-fact
                                    // excuse exactly as it was.
                                    let elsewhere = remote.as_ref().is_none_or(|remote| {
                                        remote.contains(&pointer.oid, pointer.size)
                                    });
                                    if committed && authorized && absent && elsewhere {
                                        report.virtual_paths += 1;
                                    } else {
                                        report.bad.push((
                                            display_relative(&root, &path),
                                            format!(
                                                "LFS object {} is missing locally",
                                                pointer.oid
                                            ),
                                        ));
                                    }
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
        .map_err(|err| SyncError::Journal(format!("verify task failed: {err}")));

        // Published before the `?`, not after it, and this is the reason every
        // fallible step above lives inside the closure. `verify` takes no
        // reservation, so it is the one pass `Reservation::drop` does not
        // cover, and an unreadable tree — or a policy that will not compile —
        // used to leave `Verifying` on the tray and a bar on the card until the
        // process ended (Story 34.8).
        //
        // It moves nothing a user owns: no worktree file is written and no
        // object is added to the store. It is not *pure*, though, and the
        // claim is worth narrowing rather than repeating — opening the
        // repository to read the index is a `gix` open, which may clear a stale
        // `index.lock` and may write `.git/config` on first use.
        self.publish(self.progress(&profile, SyncPhase::Idle));
        report?
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
        // Tier 0 applies here exactly as it applies to the commit path. An
        // excluded path is invisible — "not staged, not queued, not counted, and
        // never reported as pending" is `crate::exclude`'s own contract — and
        // this list used to be the one place that broke it, so a `.DS_Store`
        // that was not in `.gitignore` sat in the Pending list forever while the
        // commit path correctly ignored it (AD-34-15).
        let excludes = ExcludeSet::new(&profile.excludes)?;

        // Settling paths are absolute in `file_state` (that is what the gate
        // samples), and everything the user sees must be repository-relative.
        //
        // **Bounded, and off the runtime.** This loop used to `stat` every row
        // the gate held, inline in an async fn. On the folder this was written
        // for the gate holds 128 483 of them, so one Pending poll made 128 483
        // blocking syscalls on a USB volume from a tokio worker — which is why
        // the Pending list never arrived AND why Activity, a single indexed
        // query, appeared to hang beside it: they share the runtime, and the
        // frontend renders the three legs together.
        //
        // Capping the LIST does not cost the count. "N waiting for writes to
        // stop" comes from `status.settling`, a `COUNT(*)`; see
        // [`crate::progress::status_line`]. What is bounded here is how many
        // rows cross the IPC boundary and get a size measured.
        let settling = self.with_db(|conn| db::load_file_state(conn, profile_id))?;
        let local_path = profile.local_path.clone();
        let filter = excludes.clone();
        let (mut out, mut named) = tokio::task::spawn_blocking(
            move || -> (Vec<PendingFile>, std::collections::HashSet<String>) {
                let mut out: Vec<PendingFile> = Vec::new();
                let mut named: std::collections::HashSet<String> = std::collections::HashSet::new();
                for (path, entry) in settling {
                    if out.len() >= PENDING_LIST_CAP {
                        break;
                    }
                    let relative = path.strip_prefix(&local_path).unwrap_or(&path);
                    // A row written before a pattern was added to the profile:
                    // the gate will drop it on the next walk, and until then it
                    // must not be shown.
                    if filter.is_excluded(relative) {
                        continue;
                    }
                    let relative = relative.to_string_lossy().into_owned();
                    if named.insert(relative.clone()) {
                        // `path` here is absolute — the gate stores it that
                        // way, which is the whole reason `relative` is derived
                        // above.
                        let size_bytes = std::fs::metadata(&path)
                            .ok()
                            .filter(|m| m.is_file())
                            .map(|m| m.len());
                        out.push(PendingFile {
                            path: relative,
                            reason: PendingReason::Settling {
                                since_ms: entry.pending_since_ms,
                            },
                            size_bytes,
                        });
                    }
                }
                (out, named)
            },
        )
        .await
        .map_err(|err| SyncError::Journal(format!("pending settling scan failed: {err}")))?;

        // A folder that is not a repository yet has nothing git can classify,
        // and materializing one is a clone — far too much for a poll. The
        // first sync adopts it and the next call is complete.
        let is_repository = profile.local_path.join(".git").exists();

        // Before the walk, not after it, and for exactly the reason
        // [`Self::repair_recorded_pointers`] gives on the commit leg: a path
        // whose recorded blob contradicts its own attributes is pending BY
        // DEFINITION — the index says so, for free — and the walk is the most
        // expensive possible way to rediscover it. The walk cleans the file,
        // gets a pointer, compares it to a raw blob and calls it modified: one
        // full read and one filter round trip per path, on every poll.
        //
        // The commit leg learned this and this one did not, which is what made
        // an open Sync pane a permanent full-tree scan. Measured on the folder
        // it was written for: ~37 500 such paths, 48 to 72 minutes per walk,
        // restarted the moment it finished because the pane polls every five
        // seconds and the poll before it had only just returned.
        //
        // Answering from the index instead is not a thinner list, it is the
        // first list that arrives at all: the walk it replaces never finished,
        // so Pending read "Loading…" for days. Bounded by the same window the
        // repair sweep uses and started from the same cursor — READ, never
        // advanced, because moving it here would make the sweep skip the work
        // it is the only leg that can do.
        let repair = if is_repository {
            match self.remembered_repair(&profile.id) {
                Some(remembered) => remembered,
                None => {
                    let repo_path = profile.local_path.clone();
                    let removable = profile.removable;
                    let filtered = profile.lfs_mode != LfsMode::Disabled;
                    let from = Self::lock(&self.repair_cursor)
                        .get(&profile.id)
                        .copied()
                        .unwrap_or(0);
                    let found = tokio::task::spawn_blocking(move || -> Vec<PathBuf> {
                        if !filtered {
                            return Vec::new();
                        }
                        let Ok(repo) = git::repo::open(&repo_path, removable) else {
                            return Vec::new();
                        };
                        let Ok(tracked) = git::repo::tracked_paths(&repo) else {
                            return Vec::new();
                        };
                        lfs::stage::mismatched_filtered_paths(
                            &repo,
                            &tracked,
                            from,
                            REPAIR_WINDOW,
                            PENDING_LIST_CAP,
                        )
                        .0
                    })
                    .await
                    .map_err(|err| {
                        SyncError::Journal(format!("pending repair probe failed: {err}"))
                    })?;
                    self.remember_repair(&profile.id, &found);
                    found
                }
            }
        } else {
            Vec::new()
        };
        for rela in &repair {
            if excludes.is_excluded(rela) {
                continue;
            }
            let relative = rela.to_string_lossy().into_owned();
            if named.insert(relative.clone()) {
                out.push(PendingFile {
                    path: relative,
                    reason: PendingReason::Modified,
                    size_bytes: std::fs::metadata(profile.local_path.join(rela))
                        .ok()
                        .filter(|m| m.is_file())
                        .map(|m| m.len()),
                });
            }
        }

        // The same one-walk-per-folder claim the commit leg takes, and the
        // reason the claim exists: this branch is reached from a five-second UI
        // poll, so without it a folder whose walk runs for over an hour ends up
        // with a second full-tree pass beside the first — measured, two at once
        // for as long as anyone watched. Losing the claim costs the poll only
        // the untracked rows; the settling rows and the repair backlog above
        // are already assembled, and the walk that does hold the claim is
        // computing the same verdicts for the commit path anyway.
        let walk_claim = (is_repository && repair.is_empty())
            .then(|| self.claim_walk(profile_id))
            .flatten();
        if walk_claim.is_some() {
            let repo_path = profile.local_path.clone();
            let removable = profile.removable;
            let filter = excludes.clone();
            let interval = self.walk_report_interval;
            // The expensive half of a walk is the directory scan, and a
            // five-second poll does not need it: a new file reaches this list
            // through the watcher and the stability gate long before a walk
            // would find it. Measured on tgdrive, `find . -type f` over 157 490
            // files takes 996 s on that volume — every poll, for rows the gate
            // already holds. So the poll asks about the index and sweeps for
            // untracked paths on a cadence; see `untracked_sweep_due`.
            let policy = self.poll_walk_policy(profile_id);
            // The walk runs on a blocking thread and cannot touch `&self`, so
            // its progress comes back over a channel and is published from
            // here. Without this the Pending list is the one surface that
            // still went silent for the whole walk - which is exactly where
            // the owner was looking when they asked what keeper was doing.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
            // The status walk and the untracked expansion are both blocking
            // filesystem work on a tree that may hold a hundred thousand
            // files; running them on the async runtime would stall every other
            // profile while a UI poll finished.
            let task = tokio::task::spawn_blocking(
                move || -> Result<(
                    git::repo::RepoStatus,
                    Vec<PathBuf>,
                    std::collections::HashMap<PathBuf, u64>,
                )> {
                    let repo = git::repo::open(&repo_path, removable)?;
                    let report = move |seen: u64, entries: u64| {
                        // A closed receiver means the caller has gone; the walk
                        // still has a list to finish, so the send is dropped.
                        let _ = tx.send((seen, entries));
                    };
                    let status =
                        git::repo::status_paths_reported(&repo, Some(&report), interval, policy)?;
                    // Asked here, where the repository is already open and on a
                    // blocking thread: a deleted path cannot be stat'd, and the
                    // index is the only thing that still knows how big it was.
                    let deleted_sizes = status
                        .deleted
                        .iter()
                        .filter_map(|rela| {
                            lfs::stage::indexed_size(&repo, rela).map(|size| (rela.clone(), size))
                        })
                        .collect();
                    // Tier 0 before the expansion, not only after it: that walk
                    // `lstat`s every entry, and a `node_modules` the user has
                    // not put in `.gitignore` would otherwise cost fifty
                    // thousand stats per poll to produce nothing.
                    let candidates: Vec<PathBuf> = status
                        .untracked
                        .iter()
                        .filter(|rela| !filter.is_excluded(rela.as_path()))
                        .cloned()
                        .collect();
                    // The poll reports what is waiting; the commit walk owns
                    // saying why a nested repository never will be.
                    let (untracked, _collapsed) = Self::expand_untracked(&repo_path, &candidates)?;
                    Ok((status, untracked, deleted_sizes))
                },
            );

            // Drain the walk's progress as it arrives, then take its result.
            //
            // The channel's own close is the signal: `tx` lives in the closure,
            // so `recv` yields every report and then `None` once the walk has
            // returned and dropped it. Nothing is lost and nothing is polled
            // twice.
            //
            // This was a `select!` between `recv` and the join handle, which is
            // wrong in a way only the macOS gate caught: `select!` picks a ready
            // branch at RANDOM, so a walk that finished before the first drain
            // could take the handle's branch and discard every queued report.
            // On Linux the timing hid it; the same test failed on the Mac.
            while let Some((scanned, entries)) = rx.recv().await {
                let mut event = self.progress(&profile, SyncPhase::Scanning);
                event.files_done = scanned;
                // Index entries on both sides; see [`git::repo::WalkReport`].
                event.files_total = (entries > 0).then_some(entries);
                self.publish(event);
            }
            let (status, untracked, deleted_sizes) = task
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
                    if excludes.is_excluded(rela) {
                        continue;
                    }
                    let relative = rela.to_string_lossy().into_owned();
                    // Already named as settling, or reported by two status
                    // buckets at once (staged as added, then edited again).
                    if named.insert(relative.clone()) {
                        // Measured off the worktree, and only where there is
                        // something to measure: a deleted path has no file, and
                        // a stat that fails is a size nobody can report rather
                        // than a listing that fails.
                        let size_bytes = match reason {
                            // Off the index, because the file is gone. What it
                            // reports is what was there — which is the whole
                            // question about something that has been deleted
                            // and not yet published.
                            PendingReason::Deleted => deleted_sizes.get(rela).copied(),
                            _ => std::fs::metadata(profile.local_path.join(rela))
                                .ok()
                                .filter(|m| m.is_file())
                                .map(|m| m.len()),
                        };
                        out.push(PendingFile {
                            path: relative,
                            reason: reason.clone(),
                            size_bytes,
                        });
                    }
                }
            }
        }

        // The inbound half, from the journal. Added after the status buckets so
        // `named` gives a local change precedence: if a path is somehow both
        // edited here and queued from the remote, what the operator did is the
        // more actionable of the two facts.
        let held = self
            .with_db(|conn| db::materialized_paths(conn, profile_id))
            .unwrap_or_default();
        for (label, oid, size_bytes) in
            self.with_db(|conn| db::queued_downloads(conn, profile_id))?
        {
            // An object whose path is not in the index cannot be named — it was
            // queued for a path since deleted. It is still work, and dropping
            // it would make this list disagree with the count in the status
            // line, so it says plainly what it is.
            let path =
                label.unwrap_or_else(|| format!("LFS object {}…", &oid[..oid.len().min(12)]));
            if named.insert(path.clone()) {
                let replacing = held.contains(&path);
                out.push(PendingFile {
                    path,
                    reason: PendingReason::Incoming {
                        size_bytes,
                        replacing,
                    },
                    size_bytes: Some(size_bytes),
                });
            }
        }

        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Every path this clone has held content for (Story 56.7, FR-345).
    ///
    /// The fact [`crate::browse::MaterializedView`] is built from, and the
    /// reason the Files pane can tell a hydrated LFS path from an ordinary
    /// file: `browse` opens no repository, so both are just regular files to
    /// it, and this ledger is what closes the gap.
    ///
    /// Synchronous like [`Self::list_profiles`], because it is one indexed
    /// `SELECT` over a table with a row per materialized path and there is
    /// nothing here to move off the runtime. Deliberately **not** folded into
    /// the shell's cached `browse_marks_for` walk alongside [`Self::pending`]:
    /// that cache exists to stop a repeated tree walk, a statement this cheap
    /// has no need of it, and caching the answer would let a stale row outlive
    /// a release — the pane would offer to free space that is already free.
    ///
    /// Narrower than [`db::materialized_rows`] on purpose, for the reason
    /// [`db::materialized_paths`] itself gives: this is one yes-or-no per
    /// path, which is the whole of what a mark needs and the whole of what it
    /// should be able to see. A caller that wants the timestamps, the pin or
    /// the recorded size is asking a listing question and wants
    /// [`Self::lfs_files`].
    pub fn materialized_paths(
        &self,
        profile_id: &str,
    ) -> Result<std::collections::HashSet<String>> {
        self.with_db(|conn| db::materialized_paths(conn, profile_id))
    }

    /// Why every path this clone holds content for will or will not be released
    /// (Story 56.9, FR-343).
    ///
    /// [`Self::materialized_paths`]' richer twin, and the reason the Files pane
    /// can put a countdown on a row: `browse` opens no repository and knows
    /// nothing about a TTL, so this ledger read plus [`release_schedule`] is the
    /// whole of where a deadline can come from.
    ///
    /// **One lock, one answer.** The profile and its rows are read inside a
    /// single [`Self::with_db`] closure, not two: two locks leave a window in
    /// which a profile edit lands between them, and every row in the listing
    /// would then be classified against a folder shape that no longer exists —
    /// a countdown drawn from a TTL nobody has any more, or a "Kept" for a mode
    /// that has just been switched off. Two statements, and synchronous for
    /// [`Self::materialized_paths`]' reason: both are indexed `SELECT`s over a
    /// table with a row per materialized path, and there is nothing here to move
    /// off the runtime. Deliberately **not** folded into the shell's cached
    /// `browse_marks_for` walk, for that method's reason and more sharply: a
    /// cached deadline is a clock that keeps counting down after the content it
    /// belonged to has already gone, so the pane would offer to free space that
    /// is already free *and* tell the owner when it would happen.
    ///
    /// The keys are the ledger's own path spelling — [`lfs::stage::index_key`]'s
    /// `/`-joined form, git's spelling, the same keys
    /// [`Self::materialized_paths`] hands back — so a caller that already
    /// matches marks against a listing matches these the same way, on every
    /// platform.
    ///
    /// The TTL, the mode and — since Story 56.10 — the compiled virtualization
    /// policy are read once for the folder rather than per row, because all
    /// three are facts about the folder; [`release_schedule`] is then a pure
    /// classification of each row against them. The only per-row work the
    /// policy adds is one [`lfs::virtual_policy::VirtualPolicy::resolve`], which
    /// does no I/O of any kind. The mode is handed over as
    /// a mode rather than as [`release_mode_gate`]'s verdict, because the gate
    /// refuses two opposite configurations with one `Err` and a row's cell has
    /// to tell them apart — see [`release_schedule`]'s own doc.
    pub fn release_schedules(
        &self,
        profile_id: &str,
    ) -> Result<std::collections::HashMap<String, ReleaseSchedule>> {
        let (profile, rows) = self.with_db(|conn| {
            let Some(profile) = db::get_profile(conn, profile_id)? else {
                return Err(SyncError::Config(format!(
                    "no such sync profile: {profile_id}"
                )));
            };
            let rows = db::materialized_rows(conn, profile_id)?;
            Ok((profile, rows))
        })?;
        let ttl_ms = profile.effective_release_ttl_ms();
        // Compiled once for the listing, never per row.
        //
        // Each row is resolved at the size the ledger recorded when the content
        // landed, which is the pointer's own number — `db::note_arrival` writes
        // it from the pointer in hand.
        //
        // `unwrap_or(u64::MAX)` for a row that recorded none, which is every row
        // written before Story 56.5, and the direction is the whole reason. Read
        // as `0`, such a row falls under any `virtualOverBytes` floor and draws
        // `ModeKeeps` — *this folder is set to keep large-file content* — while
        // `Self::release_expired`, which asks the committed pointer, releases
        // that very path with no countdown ever shown. A surface that promises a
        // release the deleting chain refuses only disappoints; one that promises
        // the content is kept and then loses it from the worktree surprises, and
        // that is the direction this must not be wrong in. So an absent size
        // means *unknown*, not *nothing*: it clears any floor, the row draws the
        // clock the sweep will honour, and the honest size arrives with the next
        // materialization of that path.
        let policy = lfs::virtual_policy::VirtualPolicy::compile(&profile)?;
        Ok(rows
            .into_iter()
            .map(|row| {
                let virtualization =
                    policy.resolve(Path::new(&row.path), row.size_bytes.unwrap_or(u64::MAX));
                let schedule = release_schedule(&row, ttl_ms, profile.lfs_mode, virtualization);
                (row.path, schedule)
            })
            .collect())
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

        // Read from the index, in the same blocking pool the conflict scan uses
        // and for the same reason: it is filesystem work on a file that may be
        // tens of megabytes on a large repository, and this endpoint is polled.
        //
        // A folder that is not a repository yet has no index and nothing to
        // say. An index that will not open is not this report's problem to
        // raise — `sync_once` will fail on it loudly and land in `error` above —
        // so it degrades to "nothing to add" rather than failing a call that
        // has three other answers to deliver.
        let repo_path = profile.local_path.clone();
        let removable = profile.removable;
        let unspellable = tokio::task::spawn_blocking(move || {
            if !repo_path.join(".git").exists() {
                return Vec::new();
            }
            git::repo::open(&repo_path, removable)
                .and_then(|repo| git::repo::unspellable_tracked_paths(&repo))
                .unwrap_or_default()
        })
        .await
        .map_err(|err| SyncError::Journal(format!("index name scan task failed: {err}")))?;

        Ok(ProblemReport {
            warning: snapshot.as_ref().and_then(|s| s.warning.clone()),
            error: snapshot.and_then(|s| s.error),
            parked,
            conflicts,
            unspellable,
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

    /// Commit whatever has settled and queue the work a profile needs.
    ///
    /// It commits rather than merely looking because **the walk is not a
    /// query**: [`StabilityGate::is_stable`] forgets a path the instant it
    /// reports `Stable`, so a scan that walked and then discarded the result
    /// consumed the very verdict the commit leg was waiting to act on. The
    /// path re-entered the gate as a first observation on the next pass, its
    /// settle episode restarted, and on a tree walked once a second it never
    /// survived long enough to be committed at all — the folder sat at "N
    /// waiting for writes to stop" forever, with the wait perpetually reading
    /// "under a minute so far" because it genuinely had just begun. Again.
    fn scan_and_enqueue(&self, profile: &SyncProfile, source: SyncSource) -> Result<()> {
        let now = self.platform.now_ms();
        // The walk this function is about to do answers whatever the watcher
        // reported, so the wake is spent here rather than at the point the
        // decision was taken — a tick that chose to drain journal work instead
        // must leave it set (AD-34-11). Cleared before the walk, not after: a
        // walk that fails would otherwise re-run on every tick against a
        // repository that is not going to get better, and the paced backstop
        // already covers a lost attempt.
        self.clear_watch_wake(&profile.id);
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
            let committed = self.commit_local(profile, source, None)?;
            let unpushed = committed == 0 && self.has_unpushed_commits(profile)?;
            if committed > 0 || unpushed {
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
    ///
    /// Every call is also an observation for the run's rate meter: this is the
    /// moment bytes are known to have landed, and on a folder of small objects
    /// it is the *only* moment, because each object's own tally dies with the
    /// `do_lfs` call that owned it.
    fn add_transferred(&self, profile_id: &str, bytes: u64) {
        if bytes == 0 {
            return;
        }
        {
            let mut totals = Self::lock(&self.transferred);
            let total = totals.entry(profile_id.to_owned()).or_insert(0);
            *total = total.saturating_add(bytes);
        }
        self.observe_session_rate(profile_id, Instant::now());
    }

    /// Fold the profile's cumulative transferred bytes into its run meter.
    ///
    /// The clock is a parameter for [`RateMeter`]'s reason: a rate's window
    /// boundaries are only testable if a test can choose when things happened.
    fn observe_session_rate(&self, profile_id: &str, now: Instant) -> Option<u64> {
        let total = self.transferred_bytes(profile_id);
        Self::lock(&self.session_rate)
            .entry(profile_id.to_owned())
            .or_default()
            .observe(total, now)
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
        current: Option<&str>,
        mut events: tokio::sync::mpsc::UnboundedReceiver<E>,
        mut fold: impl FnMut(&mut SyncProgress, E),
        work: F,
    ) -> T
    where
        F: Future<Output = T>,
    {
        // Stamped on the frame this function owns rather than on one published
        // before the work starts. Every frame from here overwrites the
        // snapshot, so a name set anywhere else survives exactly until the
        // first byte moves — which is to say, it is never seen.
        let mut event = self.progress(profile, phase);
        event.current = current.map(str::to_owned);
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

    /// Give queued transfers their names, for a queue that predates them.
    ///
    /// A unit is named when it is enqueued, which is the cheap moment: the path
    /// is right there. Rows queued before that column existed have no such
    /// moment left, and on an install that upgraded mid-backlog that is every
    /// row — 106 objects and 54.7 GB of them on the folder that prompted this,
    /// which would have shown the folder's name and nothing else for days.
    ///
    /// The index is the only reverse map from object to path, so this walks it
    /// once and answers every waiting unit in that pass. Runs at most once per
    /// profile per process (see `labels_backfilled`) and never fails a sync: a
    /// missing name is a worse line, not a worse outcome.
    fn backfill_labels(&self, profile: &SyncProfile) {
        if Self::lock(&self.labels_backfilled).contains(&profile.id) {
            return;
        }
        let Ok(wanted) = self.with_db(|conn| db::unlabelled_transfers(conn, &profile.id)) else {
            return;
        };
        Self::lock(&self.labels_backfilled).insert(profile.id.clone());
        if wanted.is_empty() {
            return;
        }
        let named = (|| -> Result<usize> {
            let repo = git::repo::open(&profile.local_path, false)?;
            let tracked = git::repo::tracked_paths(&repo)?;
            let mut named = 0usize;
            for rela in tracked {
                let Some(pointer) = lfs::stage::indexed_pointer(&repo, &rela) else {
                    continue; // an ordinary file, not an LFS path
                };
                let Some(ids) = wanted.get(&pointer.oid) else {
                    continue;
                };
                // git's spelling, not the platform's. This column stopped being
                // a display nicety in Story 56.5: `note_unit_synced` reads a
                // completed unit's label as a `materialized` LEDGER KEY, and
                // `db::unlabelled_transfers` selects any journal row carrying an
                // `oid` — which includes `LfsUpload`. So a `\`-joined label here
                // would, on Windows, stamp `synced_at_ms` against a key no
                // ledger row uses, and every locally authored path in a queue
                // that predates this story would stay unreleasable at any age —
                // silently inert on exactly the install this function exists
                // for. A no-op on Unix, where the two spellings agree.
                let label = lfs::stage::index_key(&rela);
                for id in ids {
                    self.with_db(|conn| db::label_unit(conn, *id, &label))?;
                    named += 1;
                }
            }
            Ok(named)
        })();
        match named {
            Ok(0) => {}
            Ok(named) => tracing::info!(profile = profile.name, named, "named queued transfers"),
            Err(err) => tracing::debug!(error = %err, "could not name queued transfers"),
        }
    }

    fn progress(&self, profile: &SyncProfile, phase: SyncPhase) -> SyncProgress {
        let mut event = SyncProgress::idle(&profile.id, &profile.name);
        event.phase = phase;
        event
    }
}

/// Releases a profile's one-operation-at-a-time reservation on every exit path,
/// including a panic — the `LiveFolderReservation` idiom from the shell — and
/// retires the progress phase with it, for the reason spelled out in `drop`.
struct Reservation<'a> {
    engine: &'a Engine,
    profile_id: String,
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        // Before the reservation is released, and unconditionally.
        //
        // [`Engine::record_failure`] covers every failure the journal routes
        // through it, but not every way a pass can end. `sync_once` calls it
        // nowhere: `ensure_repo`, `commit_local`, `do_pull`, the drain loop and
        // its own `do_push` all propagate with `?` straight past the tail that
        // publishes `Idle`. The supervisor's own pass has no such tail at all:
        // `tick_profile` returns straight out of `drain_journal`, and the only
        // reset on that route is `refresh_pending`'s, which fires only when
        // `pending == 0` — so a tick that drained one unit and left three
        // queued kept its last phase whether it succeeded or not. And a panic
        // unwinds through here without running any of it. Every one of those
        // left the snapshot in whatever phase the last published event named.
        //
        // Holding this reservation *is* the definition of "a pass is running"
        // for this profile, so releasing it is the one event true of every exit
        // path — including the ones nobody has written yet. That is why the
        // reset hangs off the drop rather than off the tails of the two
        // functions that reserve (Story 34.8, Story 34.10).
        //
        // Ordering is load-bearing: the phase is cleared while this profile is
        // still reserved, so a pass starting afterwards cannot have its first
        // published frame wiped by this one's cleanup.
        self.engine.clear_phase(&self.profile_id);
        Engine::lock(&self.engine.busy).remove(&self.profile_id);
    }
}

/// Turn a caller's `/`-spelled subpath into the repository-relative path and
/// the ledger key a release will act on, or refuse it (Story 56.4, extracted
/// in Story 56.5).
///
/// Every lexical containment rule a deleting verb owes, in one place, so
/// [`Engine::dehydrate_entry`] and [`Engine::release_expired`] cannot drift
/// apart about what a subpath is allowed to name. Pure apart from the one
/// `resolve` that has to touch the filesystem, and it takes no reservation:
/// a subpath that may not be asked for is refused whatever the folder is
/// doing.
fn release_target(profile: &SyncProfile, subpath: &str) -> Result<(PathBuf, String)> {
    use lfs::hydrate::ContentRefusal;

    let segments = crate::browse::plain_segments(subpath)
        .map_err(ContentRefusal::from)
        .map_err(SyncError::Refused)?;
    if segments.is_empty() {
        // `plain_segments` answers the profile ROOT with no segments. A
        // folder holds no content to release, and asking for one as a file
        // is an escape — browse's own sentence names the subpath.
        return Err(SyncError::Refused(ContentRefusal::Escapes(
            crate::browse::BrowseRefusal::Escapes {
                subpath: subpath.to_owned(),
            },
        )));
    }
    let rela = lfs::hydrate::joined(&segments);
    // A backslash that is not a separator on this platform ALIASES onto a
    // different index entry, and this verb deletes.
    //
    // On Unix `\` is an ordinary filename character, so `plain_segments`
    // hands back ONE component for `40-media\clip.mp4` — but
    // `lfs::stage::index_key` normalizes `\` to `/` before it asks the
    // index, so the committed pointer that comes back belongs to the real
    // `40-media/clip.mp4` while every step after it acts on the literal
    // backslash-named file beside it. If that file happens to be a copy of
    // the content, its length and digest match, the remote proves the real
    // object, and the rename destroys an untracked file while reporting a
    // successful release.
    //
    // Refusing is the safe direction for a deleting verb, and it is also
    // literally true: this folder tracks the slash-spelled path, not this
    // one. On Windows `\` IS a separator, `plain_segments` has already split
    // on it, and no segment can contain one — so this costs nothing there.
    if !std::path::is_separator('\\')
        && segments
            .iter()
            .any(|segment| segment.to_string_lossy().contains('\\'))
    {
        return Err(SyncError::Refused(ContentRefusal::NotTracked {
            path: rela.to_string_lossy().into_owned(),
        }));
    }
    // The canonicalizing half of the same rule (AD-59), which the lexical
    // half cannot do: a *parent* component replaced by a symlink pointing
    // out of the folder is every-segment-normal and still outside. This is
    // a write, so it runs it.
    crate::browse::resolve(&profile.local_path, subpath)
        .map_err(ContentRefusal::from)
        .map_err(SyncError::Refused)?;
    // git's spelling, from the very function that keys the index lookup —
    // and the `materialized` ledger's rows are written in that spelling too.
    // A `\`-joined `to_string_lossy` would, on Windows, miss a pin and
    // delete nothing from the ledger while reporting a path keeper never
    // used. On Unix the two agree, which the guard above is what guarantees.
    let path = lfs::stage::index_key(&rela);
    Ok((rela, path))
}

/// May this folder let **any** content go — and, when the answer depends on its
/// virtualization policy, the compiled policy the per-path gate will use? (Story
/// 56.4, extracted in Story 56.5, widened in Story 56.10.)
///
/// A question about the *folder*, asked before a path is named, and kept in that
/// position deliberately: [`Engine::release_expired`] runs on every successful
/// sync of every folder and every arrival writes a `materialized` row, so an
/// ordinary folder has to be able to decline without opening its repository and
/// resolving a target for each candidate row.
///
/// [`LfsMode::Materialize`] used to be a flat refusal, and that is what left
/// Story 56.5's retention sweep dead in production: a folder in the mode every
/// profile starts in declined before it read a clock, so a `releaseTtlMs` set by
/// hand did nothing whatever. The refusal was never really about the mode — it
/// was about the disagreement [`Engine::dehydrate_entry`]'s doc names, that in
/// this mode [`Engine::materialize_pending`] copied any pointer's object
/// straight back out of the store on the next pass. Story 56.10 taught the
/// arrival path the same policy, so the two verbs agree, and this gate now asks
/// the cheap half of the policy question: **is any path authorized at all?**
/// The per-path half is [`release_path_gate`]'s, taken at the committed
/// pointer's size inside [`Engine::release_resolved`] — and it is the check here
/// that keeps a folder with no policy behaving exactly as it did.
///
/// **[`lfs::virtual_policy::VirtualPolicy::authorizes_anything`], never
/// [`lfs::virtual_policy::VirtualPolicy::tier`].** A source consisting of
/// nothing but `!` protections says something, so its tier is
/// [`lfs::virtual_policy::VirtualPolicyTier::PatternFile`] while its permissive
/// set is empty — a natural thing for a repository owner to commit, and one that
/// authorizes no path whatever. Gating on the tier would let such a folder arm a
/// release window and spend a whole budget of repository opens and index reads
/// per sync, forever, to be refused per path. That is precisely the cheap
/// decline this gate exists to preserve.
///
/// **The policy is compiled here, on the one arm that needs it, and handed
/// back.** `Ok(None)` means the mode answered alone and no file was read:
/// [`LfsMode::PointerOnly`] authorizes every path by configuration, so
/// [`release_path_gate`] never asks a policy there and compiling one would be a
/// filesystem read taken to answer a question nobody puts. It also means a fault
/// in `.keepervirtual` — a file that is entirely inert for a folder with
/// large-file support off — cannot displace [`LfsMode::Disabled`]'s own accurate
/// sentence, which is what happened when the caller compiled first.
///
/// Exhaustive, so a fourth mode has to decide rather than inherit.
fn release_mode_gate(profile: &SyncProfile) -> Result<Option<lfs::virtual_policy::VirtualPolicy>> {
    use lfs::hydrate::ContentRefusal;

    match profile.lfs_mode {
        LfsMode::PointerOnly => Ok(None),
        LfsMode::Disabled => Err(SyncError::Refused(ContentRefusal::LfsDisabled {
            profile: profile.name.clone(),
        })),
        LfsMode::Materialize => {
            // FR-329: a policy keeper cannot read is not a policy, and for a
            // verb that deletes the direction is not a choice.
            let policy = lfs::virtual_policy::VirtualPolicy::compile(profile)?;
            if policy.authorizes_anything() {
                Ok(Some(policy))
            } else {
                // Nothing in this folder may stay away, so nothing may leave it.
                Err(SyncError::Refused(ContentRefusal::AlwaysMaterializes {
                    profile: profile.name.clone(),
                }))
            }
        }
    }
}

/// May **this path's** content go, in a folder that has already passed
/// [`release_mode_gate`]? (Story 56.10, AD-122, AD-123.)
///
/// `size` is the **committed pointer's** declared size, never the worktree's.
/// The file standing here holds the content, so its stat is the content's
/// length — but after a release it is ~130 bytes, and a gate that answered
/// differently before and after would make this verb non-idempotent and would
/// read a `virtual_over_bytes` floor backwards. The pointer is the one number
/// that is the same either way, and it is the number
/// [`lfs::virtual_policy::VirtualPolicy::resolve`]'s floor is written against.
///
/// **This makes a path eligible and authorizes nothing** (AD-123, NFR-40). It
/// runs *before* every one of Story 56.4's refusals and replaces none of them:
/// the pattern file says what may leave, per-object proof taken at the moment of
/// the deletion says what may be deleted, and collapsing those two is precisely
/// git-lfs#3092.
///
/// **Only the permissive half of the policy is a release authority, and only in
/// [`LfsMode::Materialize`].** A `!` protection is not consulted here at all:
/// under `PointerOnly` no policy is asked, and under `Materialize` a protection
/// reaches this gate only as the absence of an authorization. That is AD-126's
/// split, deliberately — *the pattern file is advisory about hydration; the pin
/// is what is enforced against release* — so an owner who wants bytes kept
/// against the sweep pins the path, and `!` lines keep meaning what they mean on
/// the arrival side. Stated here because
/// [`lfs::virtual_policy::VirtualPolicy::resolve`] calls a protection
/// unconditional, and it is unconditional over the question that function
/// answers, not over this one.
///
/// [`LfsMode::Disabled`] is refused here as well as in the folder gate. It
/// cannot be reached, because the folder gate runs first — but the arm has to be
/// *named* rather than inherited: nothing routes through the clean filter in
/// that mode, so a policy has no LFS pointer there to have an opinion about and
/// must never read as permission.
fn release_path_gate(
    profile: &SyncProfile,
    policy: Option<&lfs::virtual_policy::VirtualPolicy>,
    rela: &Path,
    size: u64,
) -> Result<()> {
    use lfs::hydrate::ContentRefusal;
    use lfs::virtual_policy::Virtualization;

    match profile.lfs_mode {
        // The whole-profile lever, and deliberately not narrowed by the
        // per-path one: in this mode every path is virtual by configuration, so
        // asking the policy could only take away a release that already worked.
        LfsMode::PointerOnly => Ok(()),
        LfsMode::Disabled => Err(SyncError::Refused(ContentRefusal::LfsDisabled {
            profile: profile.name.clone(),
        })),
        LfsMode::Materialize => match policy.map(|policy| policy.resolve(rela, size)) {
            Some(Virtualization::Virtual) => Ok(()),
            // `None` cannot arrive here: `release_mode_gate` answers this mode
            // with `Some` or refuses. It is answered conservatively rather than
            // left to an `unwrap`, because a missing policy is an absent
            // authorization and never a permission.
            Some(Virtualization::Materialize) | None => {
                Err(SyncError::Refused(ContentRefusal::NotVirtual {
                    path: lfs::stage::index_key(rela),
                }))
            }
        },
    }
}

/// The instant this row becomes releasable, or `None` for *never* (Story 56.5,
/// FR-341, AD-131).
///
/// A free function with no `self`, no I/O and no clock, because this is where
/// the whole feature's safety lives: the sweep reads rows, asks this, and
/// compares the answer to now. That makes both guards one line each,
/// unit-testable over hand-built rows, and — the property AC 2 asks for — it
/// makes the sweep's *candidate set* a value a test can assert on. "Was not
/// released" would also be true of a path that was considered and refused;
/// "was not a candidate" is the stronger claim, and only a pure predicate can
/// carry it.
///
/// **The pin is checked first, before any clock.** It is an absolute floor, not
/// a term in an expression, and putting it above the provenance branch means no
/// future clock can be added below it that a pinned row could reach.
///
/// **`row.synced_at_ms?` is FR-341.** For content this clone authored, a `NULL`
/// there means the remote has never been observed holding these bytes, so they
/// exist on exactly one machine and are not eligible **at any age** — the `?` is
/// the whole rule, and no `unwrap_or` may ever appear on that line. Content that
/// arrived from the remote measures from its last use instead, falling back to
/// `at_ms`: that column is `NOT NULL` and is a real instant the content was
/// here, so a folder whose rows predate this story is not inert.
///
/// `pub` rather than `pub(crate)` because AC 2 asks for the candidate set to be
/// asserted *as a set*, over the rows a real repository and a real sync
/// actually produced — and `tests/release_sweep.rs` cannot do that without
/// asking the predicate. Exposing a pure function with no `self`, no I/O and no
/// clock costs nothing: there is no state a caller could corrupt with it.
pub fn release_due_at(row: &db::MaterializedRow, ttl_ms: u64) -> Option<i64> {
    if row.pinned {
        return None;
    }
    let since = if row.local_origin {
        row.synced_at_ms?
    } else {
        row.last_used_ms.unwrap_or(row.at_ms)
    };
    Some(since.saturating_add(ttl_ms as i64))
}

/// Why a materialized row will or will not be released, in the shape a surface
/// can draw (Story 56.9, FR-343).
///
/// **The one thing on the wire is a moment.** A countdown is the single string
/// Rust cannot own: it is stale the instant it is serialized, and the Files
/// tree does not poll — its listings are on demand and this story does not
/// change that — so a `"23 hr"` resolved here would be whatever it was when
/// the folder was last browsed and would sit there, wrong, for as long as the
/// pane is open. An absolute epoch-ms instant is the only value that stays
/// true without anyone re-asking, so the countdown is subtracted where a clock
/// ticks and this enum carries the instant plus, for the rows that have no
/// instant, keeper's own words for why.
///
/// The five wordy variants are the five separate reasons a materialized row is
/// not on a clock — a pin, a folder that keeps large-file content, a folder
/// with large-file support switched off, a switched-off TTL, and FR-341's
/// never-confirmed row — and they are kept apart rather than folded into one
/// "held" case because each is a different sentence to the person who asked,
/// and because collapsing them would put the choice of words in the shell
/// crate, which does not compile on every host this is developed on.
///
/// **The two mode causes are two sentences and one word.**
/// [`release_mode_gate`] refuses [`LfsMode::Materialize`] and
/// [`LfsMode::Disabled`] alike, because its subject is the *request door*:
/// whether a caller may take content away. A row's cell is a different
/// subject — what is holding *this* file — and there the two modes are
/// opposite configurations. Telling the owner of a folder with large-file
/// support OFF that it "is set to keep large-file content" is false of the
/// folder and points them at a setting that reads the other way, so
/// [`ReleaseSchedule::ModeKeeps`] and [`ReleaseSchedule::LfsOff`] are separate
/// variants with separate sentences. They still draw the same word, because
/// from the row's side "kept" is the one fact either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseSchedule {
    /// Releases no earlier than this absolute epoch-ms instant.
    Due { at_ms: i64 },
    /// The path is pinned, so keeper keeps its content until the pin is
    /// lifted. The one reason that outranks every other, and the only one the
    /// person holding it can undo from the row itself.
    Pinned,
    /// FR-341: this clone authored the content and nothing has confirmed the
    /// remote holds it, so the bytes exist on exactly one machine and are on
    /// no clock at any age. The only variant whose word is "Not sent".
    Unconfirmed,
    /// The folder's release TTL is switched off (`releaseTtlMs = 0`), so its
    /// content stays until a person releases it by hand. Shares its word with
    /// [`Self::ModeKeeps`] and [`Self::LfsOff`]; what tells it apart is that
    /// its sentence points at the folder's *release interval*, which is the
    /// setting that would put these rows on a clock.
    Indefinite,
    /// The folder is in [`LfsMode::Materialize`], which re-materializes what
    /// it holds, so a release would be undone on the next pass and a countdown
    /// would promise something that never comes. Shares its word with
    /// [`Self::Indefinite`] and [`Self::LfsOff`]; what tells it apart is that
    /// its sentence names the folder deliberately *keeping* large-file
    /// content.
    ModeKeeps,
    /// The folder has large-file support off ([`LfsMode::Disabled`]) and still
    /// holds `materialized` rows — reachable, because a folder switched away
    /// from [`LfsMode::PointerOnly`] keeps the ledger rows it already had, and
    /// `browse::classify` goes on marking them materialized. Shares its word
    /// with [`Self::Indefinite`] and [`Self::ModeKeeps`]; what tells it apart
    /// is that its sentence names large-file support being OFF, which is the
    /// opposite configuration to [`Self::ModeKeeps`] and would be a lie in its
    /// sentence.
    LfsOff,
}

impl ReleaseSchedule {
    /// The instant, or the words. A `Result` rather than two `Option`s because
    /// the choice is exclusive by construction: no variant, present or future,
    /// can answer both or neither, and the compiler is what enforces it rather
    /// than a list in a test.
    ///
    /// [`Self::releases_after_ms`] is this `.ok()` and [`Self::hold`] is this
    /// `.err()`, so the pairing the shell reads off three wire fields is not
    /// two matches kept in step by hand — it is one match, and adding a
    /// variant is a compile error in exactly one place, here.
    fn instant_or_words(&self) -> std::result::Result<i64, &'static str> {
        match self {
            Self::Due { at_ms } => Ok(*at_ms),
            Self::Pinned => Err("Pinned"),
            Self::Unconfirmed => Err("Not sent"),
            // Three reasons, one word: see [`Self::hold`] for why, and
            // [`Self::sentence`] for what actually separates them.
            Self::Indefinite | Self::ModeKeeps | Self::LfsOff => Err("Kept"),
        }
    }

    /// The absolute instant a surface counts down to — `Some` exactly for
    /// [`Self::Due`].
    ///
    /// This and [`Self::hold`] are the mapping onto the wire, and they are two
    /// complementary `Option`s rather than one enum because the consumer is a
    /// serialized struct read by TypeScript: a row draws either a figure or a
    /// word, so two nullable fields is exactly the shape the renderer wants,
    /// and putting the split here means the shell does three field reads and
    /// has no pairing to get wrong. The promise that makes that safe —
    /// `releases_after_ms().is_some() == hold().is_none()` — is structural
    /// rather than tested: both sides are one [`Self::instant_or_words`] away,
    /// and that function's return type cannot express "both" or "neither", so
    /// a variant that broke the pairing would not compile. This crate's tests
    /// (`release_schedule_pairs_an_instant_with_words_exactly_one_way`) still
    /// assert it, because the crate that consumes it cannot be compiled on
    /// this host.
    pub fn releases_after_ms(&self) -> Option<i64> {
        self.instant_or_words().ok()
    }

    /// The one or two words a surface draws when there is no instant — `None`
    /// exactly for [`Self::Due`].
    ///
    /// Short because it stands where a figure would stand, in a cell sized for
    /// `23 hr`; the whole reason is [`Self::sentence`]'s job. "Not sent" rather
    /// than "Unconfirmed" because the person is being told something about
    /// their own file, not about keeper's bookkeeping, and the switched-off
    /// TTL, the mode that keeps large-file content and the folder with
    /// large-file support off all say "Kept" because from the row's side they
    /// are the same fact — the difference between the three is a sentence, not
    /// a word.
    pub fn hold(&self) -> Option<&'static str> {
        self.instant_or_words().err()
    }

    /// The sentence, written for the person who asked. Always present.
    ///
    /// Present for [`Self::Due`] too, and that one is load-bearing: the sweep
    /// runs on the first successful sync after an hourly due-gate and is
    /// budgeted to [`RELEASE_BUDGET_OBJECTS`] objects a pass, so a countdown
    /// reaching zero means *eligible*, never *released*. A cell that let the
    /// figure speak for itself would be claiming keeper deleted something at a
    /// moment nothing happened, so the sentence says what the clock actually
    /// buys.
    ///
    /// No mechanism words. These are read by someone who did not choose a TTL,
    /// has not heard of LFS and is looking at a file they own, so they name the
    /// content and the computer rather than pointers, sweeps and clocks.
    pub fn sentence(&self) -> &'static str {
        match self {
            Self::Due { .. } => {
                "keeper lets this content go on the first sync after the time runs out; \
                 the copy stays here until then"
            }
            Self::Pinned => {
                "This path is pinned, so keeper keeps its content on this computer until \
                 the pin is lifted"
            }
            Self::Unconfirmed => {
                "Nothing has confirmed this content reached the server, so keeper will not \
                 put it on a release clock"
            }
            Self::Indefinite => {
                "This folder keeps content on this computer until someone releases it"
            }
            Self::ModeKeeps => {
                "This folder is set to keep large-file content on this computer, so nothing \
                 is released on a clock"
            }
            Self::LfsOff => {
                "Large-file support is off for this folder, so keeper is not releasing \
                 anything from it on a clock"
            }
        }
    }
}

/// Classify one ledger row for a surface (Story 56.9, FR-343).
///
/// **A classifier, not a second clock.** There is no arithmetic here at all:
/// [`release_due_at`] already answers the only hard question — which of 56.5's
/// two clocks a row measures from, and that a locally authored row the remote
/// has never confirmed is never eligible — so this adds nothing but the three
/// facts that come *before* a clock: the pin, the folder's LFS mode and whether
/// the TTL is switched on at all. They are the same three the sweep asks
/// (`Engine::release_expired`), and the only difference is that the pin is
/// hoisted to the front here so a surface can name it. A second copy of the
/// clock selection is the one way this story could make the Files pane disagree
/// with the sweep about the same row.
///
/// **The pin is asked here as well as inside [`release_due_at`]** for two
/// reasons. A surface has to say "pinned" rather than "never", which the `None`
/// coming back cannot distinguish; and a classification that inferred the pin
/// from that `None` would depend on another function's internals, so adding a
/// third reason to refuse there would silently relabel every pinned row here.
/// One extra read of one boolean is the cheaper half of that trade.
///
/// **The mode arrives as a mode, not as [`release_mode_gate`]'s refusal.** The
/// gate answers a different question — may a *caller* take content out of this
/// folder — and it says no to [`LfsMode::Materialize`] and
/// [`LfsMode::Disabled`] alike. Those are opposite configurations, so a single
/// boolean derived from the gate told the owner of a folder with large-file
/// support OFF that it was "set to keep large-file content". Mapping the mode
/// belongs here because this is where keeper's words for a *row* are chosen;
/// the gate's refusal vocabulary is about the request door. Exhaustive, so a
/// fourth mode has to decide which sentence it draws rather than inherit one.
///
/// Since Story 56.10 the mode is no longer the whole of that fact: it is joined
/// by the virtualization policy's per-path answer, because a folder in
/// [`LfsMode::Materialize`] may name paths whose content is authorized to stay
/// away and those rows are on the sweep's clock. The answer is handed in
/// **already resolved** rather than compiled here, which is what keeps this
/// function what it is — no I/O, no compile, every variant reachable from a
/// hand-built row — and what guarantees the listing and the sweep read one
/// compiled policy rather than two.
///
/// Pure — no `self`, no I/O, no clock — like [`release_due_at`] and for its
/// reason: every variant is then reachable from a hand-built row, and the
/// precedence the shell's cells rest on is provable where a compiler runs.
pub fn release_schedule(
    row: &db::MaterializedRow,
    ttl_ms: Option<u64>,
    mode: LfsMode,
    virtualization: lfs::virtual_policy::Virtualization,
) -> ReleaseSchedule {
    if row.pinned {
        return ReleaseSchedule::Pinned;
    }
    match mode {
        LfsMode::PointerOnly => {}
        // Story 56.10: the mode is no longer the whole answer. A folder that
        // keeps everything by default may still name paths whose content is
        // authorized to stay away, and those rows are on the sweep's clock — so
        // `ModeKeeps`' sentence, which tells the owner this folder is set to
        // keep large-file content, would be false over a row that will be
        // released within the hour. The per-path answer arrives from the caller,
        // resolved against the same compiled policy the sweep asks.
        LfsMode::Materialize => match virtualization {
            lfs::virtual_policy::Virtualization::Virtual => {}
            lfs::virtual_policy::Virtualization::Materialize => return ReleaseSchedule::ModeKeeps,
        },
        LfsMode::Disabled => return ReleaseSchedule::LfsOff,
    }
    let Some(ttl_ms) = ttl_ms else {
        return ReleaseSchedule::Indefinite;
    };
    match release_due_at(row, ttl_ms) {
        Some(at_ms) => ReleaseSchedule::Due { at_ms },
        // `release_due_at` answers `None` for exactly two rows: a pinned one,
        // refused above, and FR-341's locally-authored row the remote has never
        // been observed holding. So this arm is that row and only that row.
        None => ReleaseSchedule::Unconfirmed,
    }
}

/// Is the local wall clock inside the quiet window `from`..`to` right now?
///
/// `now_ms` is UTC — the only clock the engine has, deliberately
/// ([`SyncPlatform::now_ms`]) — and `offset_minutes` is what turns it into the
/// clock on the wall the person was looking at when they typed `22:00`
/// ([`SyncPlatform::utc_offset_minutes`]). Doing that conversion here rather
/// than asking for a local timestamp keeps the one clock the engine trusts
/// injectable, which is what makes a window test the same test in every zone.
///
/// **Wrapping is the normal case.** `22:00`–`06:00` is one window across
/// midnight, not two, because that is the shape a person configures; a rule
/// written as `from <= t < to` alone would silently never open for it.
/// `from > to` therefore means "the window wraps", which is the only reading
/// available once equality is refused at validation.
///
/// Half-open, `[from, to)`, for the reason every other range in this codebase
/// is: a window ending at `06:00` and one starting at `06:00` must not both
/// claim that instant.
///
/// `None` for a window that cannot be parsed. That cannot come from
/// [`RecordingsConfig::validate`](crate::profile::RecordingsConfig::validate),
/// which refuses it, so it means a row edited outside keeper — and the caller
/// treats it as closed rather than guessing.
fn within_quiet_window(now_ms: i64, offset_minutes: i32, from: &str, to: &str) -> Option<bool> {
    let from = minute_of_day(from)?;
    let to = minute_of_day(to)?;
    let local_ms = now_ms.saturating_add(i64::from(offset_minutes) * 60_000);
    // `rem_euclid`, not `%`: a local clock west of UTC in the first hours of
    // 1970 is a negative millisecond count, and `%` would answer with a
    // negative minute-of-day that compares below every window.
    let minute = (local_ms.rem_euclid(86_400_000) / 60_000) as u16;
    Some(if from <= to {
        minute >= from && minute < to
    } else {
        minute >= from || minute < to
    })
}

/// `HH:MM` as minutes since local midnight.
///
/// A second parser beside
/// [`validate_quiet_time`](crate::profile::RecordingsConfig::validate)'s, and
/// deliberately so: that one exists to reject a value while a person is
/// looking at the field, this one to read a value already stored, and the
/// stored value may predate the rules or have been edited by hand. Both refuse
/// the same set; neither corrects anything.
fn minute_of_day(hhmm: &str) -> Option<u16> {
    let (hours, minutes) = hhmm.trim().split_once(':')?;
    let hours: u16 = hours.parse().ok()?;
    let minutes: u16 = minutes.parse().ok()?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// The hydration capability a verified copy asks for (Story 56.6).
///
/// `copy.rs` knows nothing about profiles, indexes or object stores and must
/// keep knowing nothing, so the path→profile rule lives here — in the crate
/// that owns both facts, and the one that compiles on every host.
impl crate::copy::ContentSource for Engine {
    fn materialize(&self, absolute: &Path) -> Result<()> {
        use lfs::hydrate::{ContentRefusal, MaterializeOutcome};

        let profiles = self.with_db(db::list_profiles)?;
        // Canonicalized before comparing, both sides, for the reason
        // `browse::resolve` canonicalizes both sides (AD-59): a lexical
        // `strip_prefix` over raw paths answers `NotTracked` for a synced
        // folder reached through a symlinked ancestor — `/tmp` and `/var` are
        // symlinks on macOS, and a relocated home directory is one everywhere
        // — or through an ancestor spelled with different case on APFS or
        // NTFS. That is a sentence the user knows to be false, and it would be
        // told about every large file that then silently failed to reach the
        // pendrive. The raw path is the fallback: a path that cannot be
        // canonicalized is one nobody can compare, and refusing to look would
        // be worse than comparing what was asked for.
        let target = absolute
            .canonicalize()
            .unwrap_or_else(|_| absolute.to_path_buf());
        // Longest matching root wins. A profile nested inside another one is
        // the folder that actually carries the path, and resolving against the
        // shorter root would ask the wrong repository for a subpath it does not
        // track.
        let owner = profiles
            .iter()
            .filter_map(|profile| {
                let root = profile
                    .local_path
                    .canonicalize()
                    .unwrap_or_else(|_| profile.local_path.clone());
                let rest = target.strip_prefix(&root).ok()?.to_path_buf();
                Some((profile, rest, root))
            })
            .max_by_key(|(_, _, root)| root.as_os_str().len());
        let Some((profile, rest, _)) = owner else {
            // Under no folder keeper syncs, so there is no committed pointer
            // and nothing to ask for — `NotTracked`'s own sentence, which
            // already collapses exactly these cases.
            return Err(SyncError::Refused(ContentRefusal::NotTracked {
                path: absolute.to_string_lossy().into_owned(),
            }));
        };
        // `/`-joined, the frame every subpath in this crate is already in;
        // rebuilt from components rather than string-replaced so a Windows `\`
        // never reaches `browse::plain_segments`.
        //
        // A component that is not valid UTF-8 is REFUSED rather than rendered,
        // and `browse::BrowseRefusal::Unspellable` is the sentence for exactly
        // this: `to_string_lossy` maps every invalid byte onto `U+FFFD`, so two
        // distinct names collapse onto one key and the index lookup can succeed
        // at the wrong entry — hydrating a different file than the one being
        // copied. The lossy rendering appears in the message, where it can
        // mislead nobody, and never in the key.
        let mut segments = Vec::with_capacity(rest.components().count());
        for part in rest.components() {
            let Some(name) = part.as_os_str().to_str() else {
                return Err(SyncError::Refused(ContentRefusal::Escapes(
                    crate::browse::BrowseRefusal::Unspellable {
                        subpath: absolute.to_string_lossy().into_owned(),
                    },
                )));
            };
            segments.push(name);
        }
        let subpath = segments.join("/");

        // Asked BEFORE `materialize_entry`, because by the time that answers
        // `Queued` it has already enqueued the download, labelled it, promoted
        // it and woken the supervisor — and `wake_now` also clears
        // `next_release_ms` and `release_cursor`, resetting 56.5's sweep
        // rotation. So a copy of a folder that holds nothing but pointers
        // would start fetching the whole zone the user never asked for, as a
        // side effect of being refused. This is a read of at most
        // `MAX_POINTER_BYTES` and one `stat` per path, and it decides the
        // common case without touching the journal at all.
        if let Some(pointer) = std::fs::symlink_metadata(absolute)
            .ok()
            .and_then(|meta| lfs::stage::worktree_pointer(absolute, &meta))
        {
            let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
            if !store.contains(&pointer.oid, pointer.size) {
                return Err(SyncError::Refused(ContentRefusal::ContentNotHere {
                    path: subpath,
                }));
            }
        }

        match self.materialize_entry(&profile.id, &subpath)?.outcome {
            MaterializeOutcome::Materialized | MaterializeOutcome::AlreadyMaterialized => Ok(()),
            // Belt and braces for the check above: a download queued between
            // the two means the object is not on this machine, a copy cannot
            // wait for one, and calling that hydrated is the silent-pointer-copy
            // bug in a new place.
            MaterializeOutcome::Queued => Err(SyncError::Refused(ContentRefusal::ContentNotHere {
                path: subpath,
            })),
        }
    }
}

/// UTC `yyyymmdd-hhmmss` for a conflict filename.
///
/// UTC deliberately, unlike the `.trashinfo` `DeletionDate`
/// [`crate::files_write`] writes: this name has to sort and compare the same
/// on both machines that produced the conflict, and a local stamp would put
/// two names an hour apart on one event.
///
/// The civil-date arithmetic itself is
/// [`crate::platform::civil_from_unix_ms`], which is where it lives now that
/// two modules format an instant into words.
fn conflict_stamp(now_ms: i64) -> String {
    let (year, month, day, hh, mm, ss) = crate::platform::civil_from_unix_ms(now_ms);
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

/// The object store belonging to this folder's remote, when that remote is a
/// directory this machine can see (Story 56.6).
///
/// [`Engine::remote_serves`]' filesystem fast path without the network half.
/// A remote that is a path has a real LFS store in the client layout
/// ([`lfs::local::remote_store`]), so [`lfs::store::LfsStore::contains`] is a
/// genuine, size-verified, per-object answer for the price of a `stat` — which
/// is what makes it affordable inside a walk that must stay offline.
///
/// `None` in three cases, each meaning "do not read anything into this":
///
/// * the remote is a URL, so it has a server and [`Engine::audit_remote_objects`]
///   is the half that asks it;
/// * an explicit `.lfsconfig` names a server, which wins over the shape of the
///   remote URL — the same precedence [`Engine::remote_serves`] and
///   `lfs_access` apply, and a folder that has one may be pushing its objects
///   somewhere the remote path knows nothing about;
/// * the store's root is not on disk, which is an unplugged drive. Absence is
///   never failure (AD-48), and a store nobody could ask must never be read as
///   a store that answered no.
fn filesystem_remote_store(profile: &SyncProfile) -> Option<lfs::store::LfsStore> {
    if profile.local_path.join(".lfsconfig").exists() {
        return None;
    }
    lfs::local::remote_store(&profile.remote_url).filter(|store| store.root().exists())
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
    /// A shared branch overtaken by the other machine is weather, not a
    /// decision — and the reconcile happens *in this unit* (DW-207).
    ///
    /// The first cut queued a `Pull` and let the scheduler retry. Measured in
    /// the field that spun 613 rejected pushes in 135 seconds before the
    /// reconcile got its turn, because every unpushed commit had queued its own
    /// `Push` and `claim_ready` takes those first. So the property under test
    /// is ordering: the fetch has to be attempted *before* anything is handed
    /// back to the scheduler.
    ///
    /// Asserted without a network by pointing the profile at a remote that
    /// cannot resolve: the error that comes back is then the *pull's*, not
    /// `RemoteMoved`. If the reconcile were skipped — deferred to a queue, as
    /// it once was — this would answer `RemoteMoved` instead, which is exactly
    /// the regression.
    #[tokio::test]
    async fn a_bidirectional_push_reconciles_before_it_retries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let profile = profile(dir.path());
        engine.upsert_profile(&profile).expect("store the profile");

        let err = engine
            .reconcile_and_retry_push(
                &profile,
                SyncSource::Manual,
                "refs/heads/main:refs/heads/main",
                SyncError::Diverged {
                    profile: profile.name.clone(),
                    reason: "! refs/heads/main:refs/heads/main [rejected] (fetch first)".to_owned(),
                },
            )
            .await
            .expect_err("the fixture's remote does not resolve");
        assert!(
            !matches!(err, SyncError::RemoteMoved { .. }),
            "the reconcile has to be attempted inline, not handed to the scheduler: {err:?}"
        );
    }

    /// The case the permanent classification was written for stays permanent:
    /// on a one-way lane a human deciding is the entire point (AD-50). Both
    /// shapes return before any git work, so neither touches the network.
    #[tokio::test]
    async fn a_one_way_lane_still_parks_on_a_rejected_push() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        // A worktree lane is only valid as pushOnly, so the two one-way shapes
        // are "a lane" and "a pushOnly profile on the main lane".
        for mutate in [
            (|p: &mut SyncProfile| {
                p.lane = SyncLane::Worktree;
                p.direction = SyncDirection::PushOnly;
            }) as fn(&mut SyncProfile),
            |p: &mut SyncProfile| p.direction = SyncDirection::PushOnly,
        ] {
            let mut profile = profile(dir.path());
            mutate(&mut profile);
            engine.upsert_profile(&profile).expect("store the profile");

            let err = engine
                .reconcile_and_retry_push(
                    &profile,
                    SyncSource::Manual,
                    "refs/heads/main:refs/heads/main",
                    SyncError::Diverged {
                        profile: profile.name.clone(),
                        reason: "(fetch first)".to_owned(),
                    },
                )
                .await
                .expect_err("a refused push on a lane is still an error");
            assert!(
                matches!(err, SyncError::Diverged { .. }),
                "a lane keeps its human: {err:?}"
            );
            assert_eq!(err.retriability(), Retriability::Permanent);
        }
    }

    /// Only a rejection is re-read. Everything else keeps the classification it
    /// arrived with — an auth failure that started retrying itself would spin
    /// against a credential that will never work.
    #[tokio::test]
    async fn any_other_push_failure_passes_through_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let profile = profile(dir.path());
        engine.upsert_profile(&profile).expect("store the profile");

        let err = engine
            .reconcile_and_retry_push(
                &profile,
                SyncSource::Manual,
                "refs/heads/main:refs/heads/main",
                SyncError::Auth {
                    host: "forge.example.com".to_owned(),
                },
            )
            .await
            .expect_err("an auth failure stays a failure");
        assert!(matches!(err, SyncError::Auth { .. }), "{err:?}");
        assert_eq!(err.retriability(), Retriability::Permanent);
    }

    /// The supervisor ticks at 1 Hz so queued work starts promptly. Walking the
    /// tree at that rate is what made the menu-bar glyph blink on an idle
    /// folder, and it re-stats every file on the drive once a second.
    #[test]
    fn scanning_is_paced_by_the_poll_interval_not_the_tick() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.poll_interval_ms = 15_000;

        // First sight scans at once: adding a folder must not look inert.
        assert!(engine.scan_is_due(&p), "a new profile scans immediately");
        assert!(!engine.scan_is_due(&p), "the very next tick does not");

        platform.advance_ms(14_999);
        assert!(!engine.scan_is_due(&p), "still inside the window");
        platform.advance_ms(1);
        assert!(engine.scan_is_due(&p), "the window has opened");
    }

    /// The bug this fixes was not in the sweep. It was that nothing called it.
    ///
    /// `sweep_scratch` shipped with exactly one caller — the "Recheck all files"
    /// button — so a folder accumulated scratch until somebody suspected a
    /// problem and went hunting for a button. The folder that prompted this had
    /// 915 files and 123 GB waiting behind that button.
    #[test]
    fn scratch_is_swept_on_the_first_tick_and_then_hourly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = profile(dir.path());

        // First sight sweeps: the run that leaves scratch behind is the one that
        // was killed, and the next start is the first chance to clean up.
        assert!(engine.sweep_is_due(&p), "a folder is swept when first seen");
        assert!(!engine.sweep_is_due(&p), "and not again on the next tick");

        platform.advance_ms(SWEEP_EVERY_MS - 1);
        assert!(!engine.sweep_is_due(&p), "still inside the hour");
        platform.advance_ms(1);
        assert!(engine.sweep_is_due(&p), "the hour has passed");
    }

    /// A ledger row with nothing recorded but the instant content landed.
    fn ledger_row(path: &str, at_ms: i64) -> db::MaterializedRow {
        db::MaterializedRow {
            path: path.to_owned(),
            at_ms,
            last_used_ms: None,
            synced_at_ms: None,
            oid: None,
            size_bytes: None,
            pinned: false,
            local_origin: false,
        }
    }

    /// Every branch of the eligibility predicate, over hand-built rows (Story
    /// 56.5, FR-341, AD-131).
    ///
    /// Asserted as a function and not only through the sweep, because this is
    /// where the safety lives: the sweep's *candidate set* is whatever this
    /// returns, so "a path was not released" and "a path was never considered"
    /// are different claims and only this one can make the second.
    #[test]
    fn release_due_at_selects_a_clock_by_provenance_and_refuses_two_rows_outright() {
        const TTL: u64 = 24 * 60 * 60 * 1000;

        // Arrived from the remote and used: the use clock.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.last_used_ms = Some(5_000);
        assert_eq!(release_due_at(&row, TTL), Some(5_000 + TTL as i64));

        // Arrived from the remote and never used since: `at_ms` is the fallback
        // rather than `None`, because it is NOT NULL and is a real instant the
        // content was here. Without it every row written before this story
        // would be inert forever.
        let row = ledger_row("clip.mp4", 1_000);
        assert_eq!(release_due_at(&row, TTL), Some(1_000 + TTL as i64));

        // Authored here and confirmed upstream: the confirmation clock.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.local_origin = true;
        row.synced_at_ms = Some(7_000);
        assert_eq!(release_due_at(&row, TTL), Some(7_000 + TTL as i64));

        // Authored here and never confirmed: not eligible at any age, and a
        // recent use does not rescue it. This is FR-341, and it is the `?`.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.local_origin = true;
        row.last_used_ms = Some(i64::MAX / 2);
        assert_eq!(
            release_due_at(&row, TTL),
            None,
            "content that exists on this machine alone is never a candidate, \
             whatever `last_used_ms` says — a use must not make it one"
        );

        // Pinned, before any clock is consulted. Both provenances, because the
        // pin is a floor rather than a term in one branch's expression.
        for local_origin in [false, true] {
            let mut row = ledger_row("clip.mp4", 1_000);
            row.pinned = true;
            row.local_origin = local_origin;
            row.last_used_ms = Some(1);
            row.synced_at_ms = Some(1);
            assert_eq!(
                release_due_at(&row, TTL),
                None,
                "a pinned path is never a candidate, at any age, on either clock"
            );
        }

        // The boundary, both sides of it — and asserted through the very
        // predicate the sweep filters candidates with rather than through
        // arithmetic. The first cut here said `assert!(due - 1 < due)`, which
        // is true for every `i64` this code can produce: it claimed to cover
        // the near side of the deadline and would have survived the sweep
        // firing a millisecond early, which is the one mistake this line
        // exists to catch.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.last_used_ms = Some(0);
        let due = release_due_at(&row, TTL).expect("a candidate");
        assert_eq!(due, TTL as i64, "due exactly a TTL after the clock moved");
        // `release_expired`'s own filter, verbatim.
        let eligible = |now: i64| release_due_at(&row, TTL).is_some_and(|at| now >= at);
        assert!(
            !eligible(due - 1),
            "one millisecond short of the deadline is not a candidate, and a \
             sweep that answered otherwise would delete content early"
        );
        assert!(eligible(due), "and the deadline itself is");

        // A zero TTL is never asked — `release_is_due` answers first — but the
        // predicate must still be honest if it is: due immediately, not never.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.last_used_ms = Some(4_000);
        assert_eq!(release_due_at(&row, 0), Some(4_000));
    }

    /// Every branch of the surface classifier, in the order it asks them
    /// (Story 56.9, FR-343).
    ///
    /// [`release_schedule`] adds no arithmetic to [`release_due_at`], so what is
    /// worth asserting is the *precedence*: which fact wins when two of them
    /// apply at once. Each pair below is a row where a naive ordering would
    /// answer a different variant, and every one of those wrong answers is a
    /// sentence shown to somebody about their own file.
    #[test]
    fn release_schedule_asks_the_pin_then_the_mode_then_the_ttl() {
        use lfs::virtual_policy::Virtualization;

        const TTL: u64 = 24 * 60 * 60 * 1000;

        // The pin is the first question, so it wins over a clock that ran out
        // long ago and over every mode. A surface must say "pinned" here:
        // "kept by the mode" would send the owner to change a setting that is
        // not what is holding this path.
        for mode in [
            LfsMode::PointerOnly,
            LfsMode::Materialize,
            LfsMode::Disabled,
        ] {
            let mut row = ledger_row("clip.mp4", 1_000);
            row.pinned = true;
            row.last_used_ms = Some(1);
            assert_eq!(
                release_schedule(&row, Some(TTL), mode, Virtualization::Materialize),
                ReleaseSchedule::Pinned,
                "the pin is asked before anything else, whatever the mode says"
            );
            // And with the sweep switched off entirely, the pin is still the
            // more specific truth.
            assert_eq!(
                release_schedule(&row, None, mode, Virtualization::Materialize),
                ReleaseSchedule::Pinned
            );
        }

        // The mode beats a due clock: in a folder that re-materializes what it
        // holds, a countdown would promise a release that never comes.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.last_used_ms = Some(5_000);
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::Materialize,
                Virtualization::Materialize
            ),
            ReleaseSchedule::ModeKeeps
        );
        // And it beats an absent TTL, because the mode is the more specific
        // reason of the two and both would draw the same word.
        assert_eq!(
            release_schedule(
                &row,
                None,
                LfsMode::Materialize,
                Virtualization::Materialize
            ),
            ReleaseSchedule::ModeKeeps
        );
        // Story 56.10: and it does NOT beat a policy that authorizes this path.
        // Such a row is on the sweep's clock, so a cell reading "this folder
        // keeps large-file content" would be lying about this story's own
        // change — it would tell the owner nothing is released on a clock while
        // the sweep releases this very path within the hour.
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::Materialize,
                Virtualization::Virtual
            ),
            ReleaseSchedule::Due {
                at_ms: 5_000 + TTL as i64
            },
            "an authorized path in the default mode falls through to the clock"
        );
        // And with the sweep switched off it draws the folder's own word rather
        // than the mode's: nothing is holding this path but the absent TTL.
        assert_eq!(
            release_schedule(&row, None, LfsMode::Materialize, Virtualization::Virtual),
            ReleaseSchedule::Indefinite
        );

        // The OTHER mode `release_mode_gate` refuses, and the reason this is
        // not one boolean: a folder with large-file support switched OFF still
        // carries the `materialized` rows it had while it was `PointerOnly`, so
        // it reaches here — and "set to keep large-file content" would be the
        // opposite of what its own settings say.
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::Disabled,
                Virtualization::Materialize
            ),
            ReleaseSchedule::LfsOff,
            "a folder with large-file support off is told that, not that it keeps content"
        );
        assert_eq!(
            release_schedule(&row, None, LfsMode::Disabled, Virtualization::Materialize),
            ReleaseSchedule::LfsOff
        );

        // `releaseTtlMs = 0` reaches here as `None` (`effective_release_ttl_ms`)
        // and is the operator's documented way to say "never release from this
        // folder" — not a very short TTL.
        assert_eq!(
            release_schedule(
                &row,
                None,
                LfsMode::PointerOnly,
                Virtualization::Materialize
            ),
            ReleaseSchedule::Indefinite
        );

        // FR-341: authored here, never confirmed upstream. Not on a clock at any
        // age, and a recent use does not rescue it — this is the one row
        // `release_due_at` answers `None` for that is not the pin.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.local_origin = true;
        row.last_used_ms = Some(i64::MAX / 2);
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::PointerOnly,
                Virtualization::Materialize
            ),
            ReleaseSchedule::Unconfirmed,
            "content that exists on this machine alone is never put on a clock"
        );

        // FR-341's other half, and the case the mutation above would otherwise
        // hide: authored here AND confirmed upstream. This row IS on a clock,
        // and the clock is the confirmation — the enormous `last_used_ms` beside
        // it must not move the instant, because for content this clone authored
        // a local use is not what makes the bytes safe to drop.
        let mut row = ledger_row("clip.mp4", 1_000);
        row.local_origin = true;
        row.synced_at_ms = Some(7_000);
        row.last_used_ms = Some(i64::MAX / 2);
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::PointerOnly,
                Virtualization::Materialize
            ),
            ReleaseSchedule::Due {
                at_ms: 7_000 + TTL as i64
            },
            "a confirmed locally-authored row counts down from the confirmation, \
             never from a use"
        );

        // The ordinary row, and the instant is `release_due_at`'s own answer
        // rather than a second sum: the use clock when there is one…
        let mut row = ledger_row("clip.mp4", 1_000);
        row.last_used_ms = Some(5_000);
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::PointerOnly,
                Virtualization::Materialize
            ),
            ReleaseSchedule::Due {
                at_ms: 5_000 + TTL as i64
            }
        );
        // …and `at_ms` when it has not been used since it landed.
        let row = ledger_row("clip.mp4", 1_000);
        assert_eq!(
            release_schedule(
                &row,
                Some(TTL),
                LfsMode::PointerOnly,
                Virtualization::Materialize
            ),
            ReleaseSchedule::Due {
                at_ms: 1_000 + TTL as i64
            }
        );
    }

    /// A `Virtual` answer widens the mode's arm and nothing else (Story 56.10).
    ///
    /// The precedence every one of this epic's safety claims rests on. The pin
    /// is a hard floor asked before any mode, and [`LfsMode::Disabled`] means
    /// nothing routes through the clean filter at all — so an authorizing policy
    /// must not be able to draw a countdown over either. A classifier that asked
    /// the policy first would promise a release the chain that deletes refuses,
    /// which is the one direction a surface may not be wrong in.
    #[test]
    fn a_virtual_answer_overrides_neither_the_pin_nor_large_file_support_being_off() {
        use lfs::virtual_policy::Virtualization;

        const TTL: u64 = 24 * 60 * 60 * 1000;

        let mut pinned = ledger_row("clip.mp4", 1_000);
        pinned.pinned = true;
        pinned.last_used_ms = Some(1);
        assert_eq!(
            release_schedule(
                &pinned,
                Some(TTL),
                LfsMode::Materialize,
                Virtualization::Virtual
            ),
            ReleaseSchedule::Pinned,
            "the pin is a floor: a policy authorizes eligibility, never a deletion"
        );

        let mut off = ledger_row("clip.mp4", 1_000);
        off.last_used_ms = Some(5_000);
        assert_eq!(
            release_schedule(&off, Some(TTL), LfsMode::Disabled, Virtualization::Virtual),
            ReleaseSchedule::LfsOff,
            "with large-file support off there is no pointer for a policy to have an \
             opinion about"
        );
    }

    /// **The invariant the shell relies on**: a schedule carries an instant or
    /// words, never both and never neither (Story 56.9, FR-343).
    ///
    /// `sync_ipc` maps this onto three wire fields and lives in a crate that
    /// cannot be compiled on every host this is developed on, so the pairing is
    /// asserted here, where a compiler and a test runner both are. The *type* is
    /// now what guarantees it — both accessors are one
    /// `ReleaseSchedule::instant_or_words` away and that `Result` cannot express
    /// "both" or "neither", so a variant which broke the pairing would not
    /// compile — and this test is still worth having: it is what catches an
    /// accessor rewired to the wrong side of the `Result`, and it names the
    /// consequence a compiler cannot, which is a materialized row with a release
    /// cell and nothing in it.
    #[test]
    fn release_schedule_pairs_an_instant_with_words_exactly_one_way() {
        let all = [
            ReleaseSchedule::Due { at_ms: 1_700_000 },
            ReleaseSchedule::Pinned,
            ReleaseSchedule::Unconfirmed,
            ReleaseSchedule::Indefinite,
            ReleaseSchedule::ModeKeeps,
            ReleaseSchedule::LfsOff,
        ];

        for schedule in &all {
            assert_eq!(
                schedule.releases_after_ms().is_some(),
                schedule.hold().is_none(),
                "exactly one of the instant and the words is present: {schedule:?}"
            );
            assert!(
                !schedule.sentence().is_empty(),
                "every schedule can say why: {schedule:?}"
            );
        }

        // Pairwise distinct, because the sentence is the only thing carrying the
        // difference between rows that draw the same word: `Indefinite`,
        // `ModeKeeps` and `LfsOff` all say "Kept", and they send the owner to
        // three different settings — one of them the opposite of another.
        let mut sentences: Vec<&str> = all.iter().map(ReleaseSchedule::sentence).collect();
        sentences.sort_unstable();
        let distinct = sentences.len();
        sentences.dedup();
        assert_eq!(
            sentences.len(),
            distinct,
            "six reasons, six sentences — a shared one would tell somebody the wrong \
             thing about their own file"
        );
    }

    /// The release look-gate ARMS AND DECLINES on first sight, which is the one
    /// place it diverges from [`Engine::scan_is_due`] (Story 56.5).
    ///
    /// A scan is a read, so a folder just added may as well be walked at once.
    /// This sweep DELETES, and a folder keeper has never swept — freshly added,
    /// resumed, re-enabled, or a daemon that has just restarted — is precisely
    /// the one whose ledger keeper knows least about. So the first successful
    /// sync buys the folder a whole interval of grace rather than spending it,
    /// and no release happens on a profile's very first sync.
    #[test]
    fn a_folder_gets_a_whole_interval_of_grace_before_its_first_release() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = profile(dir.path());

        assert!(
            !engine.release_is_due(&p),
            "the first successful sync after keeper starts arms the window and \
             releases nothing"
        );
        assert!(
            !Engine::lock(&engine.next_release_ms).is_empty(),
            "armed, though — declining forever would be a sweep that never runs"
        );
        assert!(!engine.release_is_due(&p), "and neither does the next one");

        platform.advance_ms(RELEASE_LOOK_EVERY_MS - 1);
        assert!(
            !engine.release_is_due(&p),
            "still inside the grace interval"
        );
        platform.advance_ms(1);
        assert!(engine.release_is_due(&p), "the interval has passed");
        assert!(
            !engine.release_is_due(&p),
            "and it is an interval, not a gate that stays open once reached"
        );
    }

    /// A disabled TTL is answered before the clock is read, it arms no window,
    /// and it DISCARDS one already armed (Story 56.5).
    ///
    /// If a zero asked the gate it would arm a window. An operator who turned
    /// the sweep on an hour later would then find that window already open and
    /// see content released on the very next sync — the opposite of what
    /// turning a knob on should mean. "The answer was `false`" would also be
    /// true of a gate that armed one, so the map itself is what is asserted.
    ///
    /// The `remove` is the second half of the same promise, and it is what
    /// makes `docs/sync.md`'s sentence about turning the sweep back on true
    /// rather than false: a window armed before the knob was turned off would
    /// otherwise sit there ageing while the sweep was disabled, and be wide
    /// open the moment it was re-enabled.
    #[test]
    fn a_zero_release_ttl_is_never_due_and_arms_no_window() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.release_ttl_ms = 0;

        assert!(!engine.release_is_due(&p), "switched off is switched off");
        platform.advance_ms(RELEASE_LOOK_EVERY_MS * 10);
        assert!(!engine.release_is_due(&p), "and stays off however long");
        assert!(
            Engine::lock(&engine.next_release_ms).is_empty(),
            "and it left no window behind for a later re-enable to walk into"
        );

        // Re-enabled, the folder waits a whole interval exactly as a folder
        // seen for the first time does, rather than firing on the next sync.
        p.release_ttl_ms = crate::profile::DEFAULT_RELEASE_TTL_MS;
        assert!(
            !engine.release_is_due(&p),
            "a folder whose sweep has just been turned on arms a window and \
             declines, like any folder nothing has ever swept"
        );
        platform.advance_ms(RELEASE_LOOK_EVERY_MS);
        assert!(engine.release_is_due(&p), "and sweeps an interval later");

        // Now the other direction, which is the one the `remove` is for. The
        // line above re-armed a window; turning the knob off has to throw it
        // away, or the clock runs on while the sweep is disabled and re-enabling
        // walks straight into a window that opened in the meantime.
        p.release_ttl_ms = 0;
        assert!(!engine.release_is_due(&p));
        assert!(
            Engine::lock(&engine.next_release_ms).is_empty(),
            "turning the sweep off discards the window it had armed"
        );
        platform.advance_ms(RELEASE_LOOK_EVERY_MS * 10);
        p.release_ttl_ms = crate::profile::DEFAULT_RELEASE_TTL_MS;
        assert!(
            !engine.release_is_due(&p),
            "so turning it back on starts a fresh interval rather than firing \
             on the next sync, which is what an operator is promised"
        );
    }

    /// A folder that never syncs never releases (Story 56.5).
    ///
    /// The sweep rides `mark_synced`, and a paused profile's supervisor skips
    /// `tick_profile` entirely — so the question is never even asked. Asserted
    /// through `tick`, which is where the skip lives, and on
    /// `next_release_ms`: the gate arms a window as a side effect of being
    /// asked, so an empty map is the proof that nothing asked.
    ///
    /// The direction matters. "Paused" means keeper is not touching this
    /// folder, and a housekeeping pass that deleted content from a folder the
    /// owner has paused would be the loudest possible contradiction of that.
    #[tokio::test]
    async fn a_paused_folder_never_reaches_the_release_sweep() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.enabled = false;
        engine.upsert_profile(&p).expect("upsert");

        platform.advance_ms(crate::profile::DEFAULT_RELEASE_TTL_MS as i64 * 100);
        let _ = engine.tick().await;

        assert!(
            Engine::lock(&engine.next_release_ms).is_empty(),
            "a paused folder's tick never gets as far as asking whether anything \
             may be released, however long the clock has run"
        );
        assert!(
            engine
                .status(&p.id)
                .expect("registered")
                .last_sync_ms
                .is_none(),
            "and it never reports a successful pass, which is the edge the sweep \
             rides — this is the same fact, said where the sweep reads it"
        );
    }

    /// A paused folder loses nothing, even when something drives a sync at it
    /// anyway (Story 56.5).
    ///
    /// The test above asserts the *supervisor's* skip. This one asserts the
    /// sweep's own, which is a different fact and the one that was missing:
    /// `sync_once` checks the volume and the reservation and NOT `enabled`, so
    /// `keeper-syncd sync --once` on a paused folder, the app's
    /// `sync_folder_now`, and a notes vault's automatic push all reach
    /// `mark_synced` on a profile whose owner told keeper to stop touching it.
    /// With no pause check of its own the sweep would delete content out of
    /// exactly that folder — while `dehydrate_entry` refuses the identical
    /// operation with `ContentRefusal::Paused`.
    ///
    /// `next_release_ms` is what makes this a fence rather than an observation:
    /// the gate arms a window as a side effect of being asked, so an empty map
    /// is proof the sweep declined *before* it asked anything. The mode is
    /// `PointerOnly` on purpose — in any other mode the mode gate would decline
    /// first and this test would still pass with the pause check deleted.
    #[tokio::test]
    async fn a_sync_driven_at_a_paused_folder_releases_nothing() {
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
        p.lfs_mode = LfsMode::PointerOnly;
        p.enabled = false;
        engine.upsert_profile(&p).expect("upsert");

        // A ledger row every clock says is long overdue.
        engine
            .with_db(|conn| db::remember_materialized(conn, &p.id, "media/clip.mp4", 1))
            .expect("a ledger row");
        platform.advance_ms(p.release_ttl_ms as i64 * 100);

        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("nothing in `sync_once` refuses a paused folder — that is the point");

        assert!(
            Engine::lock(&engine.next_release_ms).is_empty(),
            "the sweep declines a paused folder before it asks whether anything \
             is due, so it arms no window either"
        );
        assert_eq!(
            engine
                .with_db(|conn| db::materialized_paths(conn, &p.id))
                .expect("the ledger")
                .len(),
            1,
            "and the row saying this folder holds that content is still there"
        );
    }

    /// End to end through the call the tick actually makes, because the schedule
    /// being right is worth nothing if the wiring is not.
    #[tokio::test]
    async fn a_due_sweep_removes_debris_and_leaves_a_live_transfer_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = profile(dir.path());

        let tmp = p.local_path.join(".git").join("lfs").join("tmp");
        std::fs::create_dir_all(&tmp).expect("scratch dir");
        let debris = tmp.join(".tmpAbandoned");
        let live = tmp.join(".tmpInFlight");
        std::fs::write(&debris, b"left by a killed run").expect("write");
        std::fs::write(&live, b"being written right now").expect("write");
        // Backdated past the age gate. The gate is what tells the two apart —
        // the names are random by construction and carry nothing.
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(7_200);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&debris)
            .expect("open")
            .set_times(std::fs::FileTimes::new().set_modified(old))
            .expect("backdate");

        // Through `tick`, not through `sweep_scratch_if_due`. Calling the sweep
        // directly would pass just as well with the call removed from the tick,
        // and the call being absent from the tick *was* the bug.
        engine.upsert_profile(&p).expect("upsert");
        let _ = engine.tick().await;

        assert!(!debris.exists(), "an hour-old temp file is debris");
        assert!(live.exists(), "a temp file being written is not");
    }

    /// `poll_interval_ms` is user-supplied and older rows predate it meaning
    /// anything, so a zero must not restore per-tick scanning.
    #[test]
    fn a_zero_poll_interval_still_leaves_a_gap_between_scans() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.poll_interval_ms = 0;

        assert!(engine.scan_is_due(&p));
        platform.advance_ms(1_000);
        assert!(!engine.scan_is_due(&p), "one tick later is still too soon");
        platform.advance_ms(1_000);
        assert!(engine.scan_is_due(&p), "the floor is a gap, not a stall");
    }

    /// AD-34-1: a scan that stages nothing must not report a busy phase.
    ///
    /// The tray samples the polled snapshot on its own ~1 Hz timer and then
    /// holds a busy glyph for three more ticks, so a phase that existed only for
    /// the milliseconds of a fruitless walk read as four seconds of activity on
    /// a folder where nothing had happened.
    #[test]
    fn a_scan_that_stages_nothing_reports_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = adoptable(dir.path());
        // An empty folder sends `open_repo` down the clone path, and no test may
        // reach a network remote. Content makes the engine adopt the folder in
        // place instead (`git init` plus a remote config). The seed ignores
        // itself, so the tree the first scan walks is genuinely clean.
        std::fs::write(p.local_path.join(".gitignore"), b".gitignore\n").expect("seed");
        engine.upsert_profile(&p).expect("upsert");
        let published: Arc<Mutex<Vec<SyncPhase>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event.phase);
            true
        }));

        // A clean tree has nothing to say.
        assert!(engine.collect_stable_changes(&p).expect("scan").is_empty());
        // Nor has a file still inside its settle window: the walk saw it, but it
        // has nothing to hand the commit, so no work exists to report.
        std::fs::write(p.local_path.join("held.txt"), b"still writing").expect("write");
        assert!(engine.collect_stable_changes(&p).expect("scan").is_empty());

        let seen = Engine::lock(&published).clone();
        assert!(seen.is_empty(), "a fruitless scan published {seen:?}");
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.phase, SyncPhase::Idle);
        assert_ne!(
            snapshot.state,
            ProfileState::Syncing,
            "nothing was staged, so nothing was syncing"
        );

        // A scan that does find work still says so, or the tray could never show
        // a folder that is genuinely mid-sync.
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert!(!engine.collect_stable_changes(&p).expect("scan").is_empty());
        let seen = Engine::lock(&published).clone();
        assert_eq!(
            seen,
            vec![SyncPhase::Scanning],
            "a scan that found work has to report itself"
        );
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Syncing
        );
    }

    /// The repair runs INSTEAD of the walk, which is the whole saving.
    ///
    /// A path whose committed blob contradicts its own attributes is modified by
    /// definition, so asking the filesystem to confirm it buys nothing and costs
    /// a full read plus a filter round trip - about 41 ms each, four at a time,
    /// which is how 73 536 of them turned this folder's 84-second stat floor
    /// into 25 to 74 minutes per pass. The index answers for nothing, so the
    /// pass that repairs them must not walk at all.
    #[test]
    fn a_recorded_inconsistency_is_repaired_without_walking_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(mut engine) = Engine::open(platform.clone()) else {
            return;
        };
        // Reporting every item makes a walk impossible to miss: if this pass
        // walked, `Scanning` would appear in the stream below.
        engine.report_every_walk_item();
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1_000_000;
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // The state a per-extension rule leaves behind, built the way the field
        // built it. `prepare` will not create it any more (that is the fix in
        // `lfs::stage`), so the fixture commits the gif with the filter bypassed
        // - which is exactly what a keeper that only asked about size did: the
        // rule filters every `.gif`, and the blob is the file's own bytes.
        std::fs::write(
            p.local_path.join(".gitattributes"),
            "*.gif filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("rule");
        std::fs::write(p.local_path.join("small.gif"), vec![3u8; 512]).expect("write");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&p.local_path)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        };
        git(&[
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.clean=cat",
            "-c",
            "filter.lfs.required=false",
            "add",
            ".gitattributes",
            "small.gif",
        ]);
        git(&[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "raw",
        ]);

        let published: Arc<Mutex<Vec<SyncProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event);
            true
        }));

        // Nothing on disk changed, so a walk would find nothing - and would pay
        // for the discovery anyway.
        // The sweep names it from the index alone, with no walk in sight.
        assert_eq!(
            engine
                .repair_recorded_pointers(&p)
                .expect("sweep")
                .map(|staged| staged.modified),
            Some(vec![PathBuf::from("small.gif")])
        );
        let repaired = engine
            .commit_local(&p, SyncSource::Watch, None)
            .expect("repair pass");
        assert!(repaired > 0, "the inconsistency has to be repaired");

        let phases: Vec<SyncPhase> = Engine::lock(&published)
            .iter()
            .map(|event| event.phase)
            .collect();
        assert!(
            !phases.contains(&SyncPhase::Scanning),
            "the repair must not walk the folder: {phases:?}"
        );

        // And it is durable: that path is recorded as a pointer now, so the next
        // scan stats it like anything else instead of reading it, and it does
        // not come back on the repair list.
        let repo = engine.open_repo(&p).expect("open");
        assert!(
            lfs::stage::mismatched_filtered_paths(&repo, &[PathBuf::from("small.gif")], 0, 10, 10,)
                .0
                .is_empty(),
            "a repaired path must not be repaired again"
        );
    }

    /// And the POLL must not walk either, for the same reason.
    ///
    /// The commit leg learned this (above); the Pending poll did not, and that
    /// asymmetry is what made an open Sync pane a permanent full-tree scan. The
    /// pane polls every five seconds, the walk it triggered took 48 to 72
    /// minutes on the folder this was written for, and the next poll started it
    /// again — so the list the user was waiting for never arrived while the
    /// machine stayed pinned. Answering from the index is not a thinner answer,
    /// it is the first one that arrives.
    #[tokio::test]
    async fn a_pending_poll_names_the_repair_backlog_without_walking_the_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(mut engine) = Engine::open(platform.clone()) else {
            return;
        };
        // Reporting every item makes a walk impossible to miss: if this poll
        // walked, `Scanning` would appear in the stream below.
        engine.report_every_walk_item();
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1_000_000;
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // The same fixture as the sweep's test: a per-extension rule filters
        // every `.gif`, and the blob is the file's own raw bytes.
        std::fs::write(
            p.local_path.join(".gitattributes"),
            "*.gif filter=lfs diff=lfs merge=lfs -text\n",
        )
        .expect("rule");
        std::fs::write(p.local_path.join("small.gif"), vec![3u8; 512]).expect("write");
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&p.local_path)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        };
        git(&[
            "-c",
            "filter.lfs.process=",
            "-c",
            "filter.lfs.clean=cat",
            "-c",
            "filter.lfs.required=false",
            "add",
            ".gitattributes",
            "small.gif",
        ]);
        git(&[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "raw",
        ]);

        let published: Arc<Mutex<Vec<SyncProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event);
            true
        }));

        let pending = engine.pending(&p.id).await.expect("pending");
        assert!(
            pending.iter().any(|row| row.path == "small.gif"
                && matches!(row.reason, PendingReason::Modified)),
            "the backlog is what the folder is waiting on, and the index names \
             it for free: {pending:?}"
        );

        let phases: Vec<SyncPhase> = Engine::lock(&published)
            .iter()
            .map(|event| event.phase)
            .collect();
        assert!(
            !phases.contains(&SyncPhase::Scanning),
            "the poll must not walk the folder to rediscover what the index \
             already recorded: {phases:?}"
        );
    }

    /// With no backlog, the poll walks exactly as it always did.
    ///
    /// The other half of the same rule, and the one that keeps the shortcut
    /// honest: skipping the walk is only defensible while there is a cheaper
    /// answer, so a clean folder must still get the full one — untracked files
    /// included, which the index cannot know about.
    #[tokio::test]
    async fn a_pending_poll_with_no_backlog_still_walks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(mut engine) = Engine::open(platform.clone()) else {
            return;
        };
        engine.report_every_walk_item();
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // Nothing the index could name: a file git has never heard of.
        std::fs::write(p.local_path.join("fresh.txt"), b"new").expect("write");

        let pending = engine.pending(&p.id).await.expect("pending");
        assert!(
            pending
                .iter()
                .any(|row| row.path == "fresh.txt" && row.reason == PendingReason::Untracked),
            "an untracked path is only discoverable by walking: {pending:?}"
        );
    }

    /// The probe answers a poll; it must not BE the poll.
    ///
    /// It opens the repository and materializes every tracked path before it
    /// examines a window of them — 3 to 21 seconds on the 155 662-entry folder
    /// this was written for. The pane asks every five seconds, so running it
    /// per poll put the answer permanently behind the next question: the pane
    /// sat on "Loading folders…" and the allocator churned a six-figure path
    /// list on a loop, which is a regression the first version of this shortcut
    /// shipped with.
    ///
    /// Asserted on the memo's own timestamp rather than on timing, because a
    /// wall-clock assertion under load is a flake and this is a question about
    /// whether work happened at all.
    #[tokio::test]
    async fn a_second_pending_poll_reuses_the_probe_instead_of_repeating_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let first = engine.pending(&p.id).await.expect("pending");
        let taken = Engine::lock(&engine.repair_memo)
            .get(&p.id)
            .map(|(at, _)| *at)
            .expect("the first poll has to leave an answer behind");

        let second = engine.pending(&p.id).await.expect("pending");
        let taken_again = Engine::lock(&engine.repair_memo)
            .get(&p.id)
            .map(|(at, _)| *at)
            .expect("still memoized");

        assert_eq!(
            taken, taken_again,
            "the second poll re-ran a probe it already had the answer to"
        );
        assert_eq!(first, second, "and it must be the same answer");
    }

    /// The Pending list is bounded, and the count it sits under is not.
    ///
    /// The gate on the folder this was written for held **128 483** rows, and
    /// this poll used to `stat` every one of them, inline in an async fn. That
    /// is 128 483 blocking syscalls on a USB volume per poll, from a tokio
    /// worker — so the Pending list never arrived, and Activity, a single
    /// indexed query, appeared to hang beside it because the frontend renders
    /// the three legs together.
    ///
    /// Both halves are asserted: the list stops at [`PENDING_LIST_CAP`], and
    /// the count behind "N waiting for writes to stop" still reports every row,
    /// because it comes from a `COUNT(*)` and not from this list.
    #[tokio::test]
    async fn a_pending_poll_bounds_its_settling_list_without_understating_the_count() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        // More rows than the cap, each a real file so the stat that used to be
        // unbounded has something to measure.
        let held = PENDING_LIST_CAP + 25;
        let mut rows = Vec::with_capacity(held);
        for i in 0..held {
            let name = format!("held-{i:05}.bin");
            let absolute = p.local_path.join(&name);
            std::fs::write(&absolute, b"still writing").expect("write");
            rows.push((
                absolute,
                crate::stability::PersistedEntry {
                    sample: crate::stability::FileSample {
                        size: 13,
                        mtime_ns: 1,
                        ctime_ns: 1,
                        inode: i as u64 + 1,
                    },
                    unchanged_since_ms: 1,
                    pending_since_ms: 1,
                    close_write: false,
                },
            ));
        }
        engine
            .with_db(|conn| db::save_file_state(conn, &p.id, &rows))
            .expect("seed the gate");

        let pending = engine.pending(&p.id).await.expect("pending");
        let settling = pending
            .iter()
            .filter(|row| matches!(row.reason, PendingReason::Settling { .. }))
            .count();
        assert_eq!(
            settling, PENDING_LIST_CAP,
            "the list is what is bounded, and it has to be bounded exactly"
        );

        assert_eq!(
            engine
                .with_db(|conn| db::load_file_state(conn, &p.id))
                .expect("gate rows")
                .len(),
            held,
            "the gate still holds every row — the cap bounds what the poll \
             RENDERS, and the count behind \"N waiting for writes to stop\" is \
             taken from this state, not from the list (see \
             `crate::progress::status_line`). A cap that also shrank the state \
             would be a lie about how much work is waiting"
        );
    }

    /// One full-tree walk per folder, and the loser keeps working.
    ///
    /// Measured on hesperia: two concurrent walks of the same 155 662-entry USB
    /// folder in 26 of 26 samples over seven minutes — the commit leg and a
    /// five-second Pending poll, each calling the walk directly, on a tree
    /// whose walk takes 72 minutes. Two walks of one tree are twice the work
    /// plus the seek contention of interleaving them.
    ///
    /// Both halves matter. Losing the race must not raise an error and must not
    /// stall: `collect_stable_changes` stages nothing and lets the supervisor
    /// retry, which is the same outcome as any quiet pass.
    #[test]
    fn only_one_status_walk_runs_against_a_folder_at_a_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        std::fs::write(p.local_path.join("work.txt"), b"x").expect("write");
        // Open the settle episode and age it out, so the folder really does
        // hold something a walk would stage. Without this the assertion below
        // passes for the wrong reason - a freshly written file is held by the
        // stability gate whether the walk ran or not - and deleting the claim
        // leaves the test green.
        engine
            .collect_stable_changes(&p)
            .expect("the first pass opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);

        let held = engine.claim_walk(&p.id).expect("the first claim is free");
        assert!(
            engine.claim_walk(&p.id).is_none(),
            "a second walk was allowed against a folder already being walked"
        );
        // A different folder is a different tree; the claim is per profile, not
        // a global lock on walking.
        assert!(
            engine.claim_walk("another-profile").is_some(),
            "the claim locked out an unrelated folder"
        );

        // The commit leg's answer to losing: nothing staged, no error.
        let deferred = engine
            .collect_stable_changes(&p)
            .expect("a deferred pass is not a failure");
        assert!(
            deferred.added.is_empty()
                && deferred.modified.is_empty()
                && deferred.deleted.is_empty(),
            "a pass that could not walk reported changes it never looked for: {deferred:?}"
        );

        // The claim is released, so the next pass is not locked out for the
        // life of the process - and that pass finds the work the deferred one
        // stayed silent about, which is what makes the silence above a
        // consequence of the claim and not of an empty folder.
        drop(held);
        let ran = engine
            .collect_stable_changes(&p)
            .expect("the released folder walks");
        assert_eq!(
            ran.added,
            vec![PathBuf::from("work.txt")],
            "the folder had stageable work all along: {ran:?}"
        );
    }

    /// The maintenance guarantee: an operation that takes time SAYS SO.
    ///
    /// The field report this exists for: "pending took very long; if keeper has
    /// a process running it should display which one and give information that
    /// suggests when it will finish (e.g. how many files are left)". Before it,
    /// the walk over a 155 000-file folder published nothing for the ten minutes
    /// it took, so the only line on screen was `tgdrive - 34521 waiting to
    /// sync` - a fact about the queue, while the work itself was invisible and
    /// indistinguishable from a wedge.
    ///
    /// Simulating "slow" is done with a folder big enough to outlast a report
    /// tick rather than by making a disk slow: what has to stay true is that
    /// the *counts reach the surface the owner reads*, and a test that waited
    /// out real seconds to prove it would be a test nobody runs. The
    /// silent-when-fast half is `git::repo`'s
    /// `a_walk_that_finishes_immediately_reports_no_progress`.
    #[test]
    fn a_long_running_walk_reports_its_progress_where_the_owner_can_see_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(mut engine) = Engine::open(platform.clone()) else {
            return;
        };
        engine.report_every_walk_item();
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join(".gitignore"), b".gitignore\n").expect("seed");
        // A committed body the walk has to compare its way through. Progress is
        // published off a clock now (see `git::repo::WalkReport`), so a folder
        // that finishes inside one tick proves nothing about a folder that runs
        // for an hour, which is the case this exists for.
        const COMMITTED: usize = 400;
        for i in 0..COMMITTED {
            std::fs::write(p.local_path.join(format!("base-{i:05}.txt")), b"x").expect("write");
        }
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            COMMITTED as u64
        );

        // And work on top, so the second scan below has something to hand the
        // commit and the walk is a real one rather than a no-op.
        for i in 0..8 {
            std::fs::write(p.local_path.join(format!("file-{i}.txt")), b"content").expect("write");
        }
        let published: Arc<Mutex<Vec<SyncProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event);
            true
        }));

        // The stability gate admits a path only once a previous walk has seen
        // it, so the first scan is the observation and the second is the one
        // with work to hand the commit. Both walk, which is what is under test.
        engine.collect_stable_changes(&p).expect("observe");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert!(!engine.collect_stable_changes(&p).expect("scan").is_empty());

        let seen = Engine::lock(&published).clone();
        let walking: Vec<&SyncProgress> = seen
            .iter()
            .filter(|event| event.phase == SyncPhase::Scanning && event.files_done > 0)
            .collect();
        assert!(
            !walking.is_empty(),
            "a walk in progress published no progress at all: {seen:?}"
        );
        let counts: Vec<u64> = walking.iter().map(|event| event.files_done).collect();
        // It counts up, so a watcher can tell movement from a freeze - within a
        // walk. Each walk is its own operation and starts again, which is what a
        // drop back to a smaller number in this stream is.
        assert!(
            counts
                .windows(2)
                .all(|pair| pair[1] >= pair[0] || pair[1] <= COMMITTED as u64),
            "a count moved backwards without restarting: {counts:?}"
        );
        // And it reaches the index it is drawn against: a report that stopped
        // part way is a pane that freezes part way.
        assert_eq!(
            counts.iter().copied().max(),
            Some(COMMITTED as u64),
            "the report has to follow the walk to its end: {counts:?}"
        );
        assert!(
            walking
                .iter()
                .all(|event| event.files_total == Some(COMMITTED as u64)),
            "the denominator is the index entry count: {walking:?}"
        );
        // And the line the owner actually reads names the step and the numbers,
        // rather than only what is queued behind it.
        let mut status = engine.status(&p.id).expect("status");
        status.phase = SyncPhase::Scanning;
        status.files_done = walking.last().expect("a report").files_done;
        status.files_total = walking.last().expect("a report").files_total;
        let line = crate::progress::status_line(&status);
        assert!(
            line.starts_with("Scanning") && line.contains("files"),
            "the owner has to be able to read what is happening; got {line:?}"
        );
    }

    /// Resuming is a request to look now; the pacing must not hold a folder
    /// idle for a window the user never waited through.
    #[test]
    fn resuming_a_paused_profile_reopens_the_scan_window() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(platform.clone()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.poll_interval_ms = 60_000;
        engine.upsert_profile(&p).expect("store");

        assert!(engine.scan_is_due(&p), "first sight");
        assert!(!engine.scan_is_due(&p), "window closed");
        engine.set_enabled(&p.id, true).expect("resume");
        assert!(engine.scan_is_due(&p), "resume looks now");
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

        let (expanded, collapsed) = Engine::expand_untracked(
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
        assert_eq!(
            collapsed,
            vec![PathBuf::from("sub")],
            "the refusal is reported to the caller rather than logged per walk"
        );
    }

    #[test]
    fn expanding_tolerates_an_entry_that_vanished() {
        // The walk and the expansion are not atomic; a deleted file in between
        // is an ordinary outcome, not an error.
        let dir = tempfile::tempdir().expect("tempdir");
        let (expanded, collapsed) =
            Engine::expand_untracked(dir.path(), &[PathBuf::from("gone.txt")]).expect("expand");
        assert!(expanded.is_empty());
        assert!(collapsed.is_empty(), "a path that is gone is not a refusal");
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

    /// The engine's floor check and [`crate::git::resolve`]'s must answer the
    /// same question, because the desktop capability the Sync UI is gated on is
    /// the resolver's answer while the surface behind it is the engine's.
    /// Story 34.14: before this, `sync_available()` asked only whether a file
    /// called `git` existed, so the app could render a Sync section over an
    /// engine that had already refused to open — a section that did nothing at
    /// all, with a refusal logged at `debug`.
    #[test]
    fn the_engine_accepts_exactly_the_gits_the_resolver_chooses() {
        use crate::git::resolve::GitRequest;
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().expect("tempdir");
        let write = |name: &str, body: &str| {
            let program = dir.path().join(name);
            std::fs::write(&program, body).expect("fixture");
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))
                .expect("mode");
            program
        };
        let candidates = [
            write("git-good", "#!/bin/sh\necho 'git version 2.52.0'\n"),
            write("git-old", "#!/bin/sh\necho 'git version 2.23.0'\n"),
            write(
                "git-broken",
                "#!/bin/sh\necho 'fatal: bad config line 44' >&2\nexit 128\n",
            ),
        ];

        for program in candidates {
            let resolver_says = GitRequest::explicit(program.clone(), "install git")
                .resolve()
                .chosen()
                .is_some();
            // A fresh data dir per case: `Engine::open` recovers a journal, and
            // sharing one would make the second open depend on the first.
            let data = dir.path().join(format!(
                "data-{}",
                program.file_name().expect("name").to_string_lossy()
            ));
            std::fs::create_dir_all(&data).expect("data dir");
            let platform = Arc::new(TestPlatform::new(&data).with_git(&program));
            let engine_says = Engine::open(platform).is_ok();

            assert_eq!(
                engine_says,
                resolver_says,
                "{} — the engine and the resolver disagreed",
                program.display()
            );
        }
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

    /// Put a profile's snapshot exactly where a live push leg leaves it:
    /// mid-phase, with the numbers a determinate bar is drawn from. The rate
    /// rides the streamed channel rather than the snapshot, but the window
    /// gates it on this snapshot, so retiring these fields retires that too.
    fn arrange_a_pass_in_flight(engine: &Engine, p: &SyncProfile) {
        let mut event = engine.progress(p, SyncPhase::Pushing);
        event.files_total = Some(9);
        event.files_done = 4;
        event.bytes_total = Some(4_000_000);
        event.bytes_done = 1_800_000;
        engine.publish(event);
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.phase, SyncPhase::Pushing, "arranged");
        assert_eq!(snapshot.state, ProfileState::Syncing, "arranged");
    }

    /// Assert that nothing on this snapshot still claims work is in flight.
    fn assert_nothing_is_in_flight(engine: &Engine, p: &SyncProfile, arm: &str) {
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(
            snapshot.phase,
            SyncPhase::Idle,
            "{arm}: the pass is over, so no phase may still name a running leg"
        );
        assert_eq!(snapshot.files_done, 0, "{arm}: a stale numerator");
        assert_eq!(snapshot.files_total, None, "{arm}: a stale denominator");
        assert_eq!(snapshot.bytes_done, 0, "{arm}: a stale byte count");
        assert_eq!(snapshot.bytes_total, None, "{arm}: a stale byte total");
    }

    /// Stories 34.8 and 34.10: a failed pass leaves the error and nothing else.
    ///
    /// Every arm of `record_failure` set a state word and left `phase` alone, so
    /// a folder that had permanently stopped publishing kept the last frame of
    /// its own push on screen — a half-full bar and a frozen rate sitting
    /// directly above the error text that says it stopped. The snapshot is
    /// asserted rather than the render, because the snapshot is what the tray
    /// and `keeper-syncd status` read: a frontend-only guard would have left two
    /// surfaces telling the same lie.
    #[test]
    fn every_failure_arm_retires_the_phase_it_was_showing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let arms = [
            // Permanent — the 401 this finding is named for. Nothing will move
            // again on this folder until a human replaces the token.
            (
                "permanent",
                SyncError::Auth {
                    host: "git.invalid".to_owned(),
                },
                ProfileState::NeedsAttention,
            ),
            // Deferred — waiting on a volume, not on a clock.
            (
                "deferred",
                SyncError::MediaAbsent,
                ProfileState::MediaAbsent,
            ),
            // Transient-and-network, which reads as a state rather than a
            // failure: the queue drains when connectivity returns (AD-49).
            (
                "offline",
                SyncError::Network {
                    host: "git.invalid".to_owned(),
                    reason: "connection refused".to_owned(),
                },
                ProfileState::Offline,
            ),
            // The fourth arm, which nobody listed as a failure arm and which
            // therefore had nothing keeping it honest either: a retriable blip
            // leaves the profile syncing, but no leg is running between the
            // failure and its backoff.
            (
                "transient",
                SyncError::Git("could not write the index".to_owned()),
                ProfileState::Syncing,
            ),
        ];

        for (arm, err, expected) in arms {
            arrange_a_pass_in_flight(&engine, &p);
            engine.record_failure(&p, &err);
            assert_eq!(
                engine.status(&p.id).expect("status").state,
                expected,
                "{arm}: the state word still has to say what happened"
            );
            assert_nothing_is_in_flight(&engine, &p, arm);
        }
    }

    /// Stories 34.8 and 34.10: the exits that never reach `record_failure`.
    ///
    /// `sync_once` propagates `ensure_repo`, `commit_local`, `do_pull` and
    /// `do_push` with `?` and calls nothing on the way out, and a tick that
    /// finishes with work still queued never reaches `refresh_pending`'s `Idle`
    /// branch either. Releasing the reservation is the one event common to all
    /// of them, which is the whole reason the reset lives on the guard: this
    /// passes for a failure arm that has not been written yet.
    #[test]
    fn releasing_a_reservation_retires_the_phase_whatever_ended_the_pass() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let reservation = engine.reserve(&p.id).expect("a free profile reserves");
        arrange_a_pass_in_flight(&engine, &p);
        // No `record_failure`, no `publish(Idle)`, no `refresh_pending`: this is
        // the shape of a `?` returning straight out of the middle of a pass.
        drop(reservation);

        assert_nothing_is_in_flight(&engine, &p, "reservation released");
        assert!(
            engine.reserve(&p.id).is_some(),
            "clearing the phase must not cost the release itself"
        );
    }

    /// Story 34.8, AD-34-13: the fetch leg's half of `SyncPhase::carries_rate`.
    ///
    /// `Fetching` is one of only three phases the vocabulary says carries a
    /// rate, and this fold is the only thing that stamps one there. It was
    /// inline in `do_pull` behind a `spawn_blocking` channel, where no test
    /// could reach it — so the stamping line could have been deleted with the
    /// suite still green, and the phase would have joined `Pushing` in claiming
    /// a figure nothing produces.
    #[test]
    fn the_fetch_leg_stamps_a_rate_off_the_high_water_mark() {
        let t0 = Instant::now();
        let mut fetched = 0u64;
        let mut rate = RateMeter::default();
        let mut event = SyncProgress::idle("p", "fixture");
        event.phase = SyncPhase::Fetching;

        // One sample is a point, not a rate, and the denominator is unknown
        // while the remote is still negotiating.
        Engine::fold_fetch_progress(&mut event, (0, 0), &mut fetched, &mut rate, t0);
        assert_eq!(event.bytes_per_second, None);
        assert_eq!(event.bytes_total, None, "a zero total is not a denominator");

        // 6 MB over two seconds.
        Engine::fold_fetch_progress(
            &mut event,
            (6_000_000, 20_000_000),
            &mut fetched,
            &mut rate,
            t0 + std::time::Duration::from_millis(2_000),
        );
        assert_eq!(event.bytes_done, 6_000_000);
        assert_eq!(event.bytes_total, Some(20_000_000));
        assert_eq!(
            event.bytes_per_second,
            Some(3_000_000),
            "the phase claims a rate, so this producer has to stamp one"
        );

        // The next node of gitoxide's tree starts its own counter at zero. The
        // high-water mark holds, so the meter falls silent instead of dividing
        // an object count by a second — and never walks the figure backwards.
        Engine::fold_fetch_progress(
            &mut event,
            (12, 400),
            &mut fetched,
            &mut rate,
            t0 + std::time::Duration::from_millis(4_500),
        );
        assert_eq!(fetched, 6_000_000, "the mark never walked back");
        assert_eq!(event.bytes_per_second, None, "a stalled fetch has no rate");
    }

    /// A folder of small objects reports throughput, and a stalled one stops.
    ///
    /// The rate the UI draws comes from the progress stream, and every figure in
    /// it used to be measured inside one `do_lfs` call. `RateMeter` withholds a
    /// number until a full second of movement backs it — correctly — so an
    /// object that finished inside a second reported nothing, and a folder of
    /// ordinary files showed a rate only for the occasional object above ~50 MB.
    /// The owner's reading of that is the honest one: a hundred files landing
    /// with no throughput figure anywhere looks exactly like a folder that has
    /// stopped.
    ///
    /// So the run keeps its own meter, fed by the cumulative counter every
    /// completed transfer already updates, and `publish` stamps it on any
    /// rate-carrying frame that has nothing better. Both halves are asserted
    /// here: the figure appears for objects far too small to measure alone, and
    /// it is still withheld once the bytes stop — the meter's rules are reused,
    /// not weakened.
    #[test]
    fn small_objects_still_report_a_rate_because_the_run_is_measured_not_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let seen = Arc::new(Mutex::new(Vec::<Option<u64>>::new()));
        let sink = Arc::clone(&seen);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event.bytes_per_second);
            true
        }));

        // The run's meter is anchored at zero bytes, which is where a run
        // starts, and then four objects of 1 MB land — none of them on the wire
        // long enough to measure, because each `add_transferred` is one whole
        // completed transfer.
        let t0 = Instant::now();
        let at = |ms| t0 + std::time::Duration::from_millis(ms);
        engine.observe_session_rate(&p.id, t0);
        for _ in 0..4 {
            engine.add_transferred(&p.id, 1_000_000);
        }
        // Two seconds of the run have now passed — time the objects never had
        // individually, which is the whole point.
        engine.observe_session_rate(&p.id, at(2_000));

        let mut event = engine.progress(&p, SyncPhase::DownloadingLfs);
        event.current = Some("clip-05.mp4".to_owned());
        engine.publish(event);
        assert_eq!(
            Engine::lock(&seen).last().copied().flatten(),
            Some(2_000_000),
            "4 MB over two seconds of the run, on a frame whose object measured nothing"
        );

        // Nothing moves for a further window. The run's meter falls silent, so
        // the frame carries no figure rather than freezing the last one.
        engine.observe_session_rate(&p.id, at(4_000));
        engine.publish(engine.progress(&p, SyncPhase::DownloadingLfs));
        assert_eq!(
            Engine::lock(&seen).last().copied().flatten(),
            None,
            "a run that has stopped moving bytes has no rate to report"
        );

        // And a phase with no byte producer never borrows the run's figure:
        // `SyncPhase::carries_rate` is the whole gate.
        engine.add_transferred(&p.id, 8_000_000);
        engine.observe_session_rate(&p.id, at(6_000));
        engine.publish(engine.progress(&p, SyncPhase::Committing));
        assert_eq!(
            Engine::lock(&seen).last().copied().flatten(),
            None,
            "committing moves no bytes, whatever the run has been doing"
        );
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
    fn removing_a_profile_deletes_its_credential_and_only_its_credential() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        let other = SyncProfile::new(
            "01JOTHERPROFILE",
            "other",
            dir.path().join("other"),
            "https://git.invalid/x/z.git",
        );
        engine.upsert_profile(&p).expect("upsert");
        engine.upsert_profile(&other).expect("upsert the second");
        platform
            .secret_set(&p.secret_key(), "token-for-p")
            .expect("store a credential");
        platform
            .secret_set(&other.secret_key(), "token-for-other")
            .expect("store the second credential");

        engine.remove_profile(&p.id).expect("remove");

        // The key is derived from the profile id, and nothing re-derives the id
        // of a profile that is gone: a secret left behind here would outlive
        // every way of finding it (AD-34-14).
        assert_eq!(
            platform.secret_get(&p.secret_key()).expect("get"),
            None,
            "removing a profile must take its credential with it"
        );
        assert_eq!(
            platform.secret_get(&other.secret_key()).expect("get"),
            Some("token-for-other".to_owned()),
            "and must take nobody else's"
        );
    }

    /// A profile's secret lives in two stores, and only one of them is keyed by
    /// the profile id (finding 1 of the epic-34 review of Story 34.7). The
    /// sibling test above asserts through `SyncPlatform::secret_get` and so
    /// cannot see the second store at all — it passed while a spendable
    /// credential for the removed folder's remote stayed in this process.
    #[test]
    fn removing_a_profile_drops_the_lfs_credential_minted_for_its_remote() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = profile(dir.path());
        // Only an ssh remote has a `git-lfs-authenticate` handshake behind it,
        // so only an ssh remote can have one of these minted for it.
        p.remote_url = "git@git.invalid:x/y.git".to_owned();
        engine.upsert_profile(&p).expect("upsert");
        let remote = lfs::ssh::SshRemote::parse(&p.remote_url).expect("an ssh remote");
        Engine::lock(&engine.lfs_ssh_credentials).insert(
            remote.cache_key(lfs::ssh::Operation::Download),
            CachedSshAnswer {
                answer: lfs::ssh::Answer::Granted(lfs::ssh::Credential {
                    href: None,
                    authorization: Some("Bearer minted-for-the-removed-folder".to_owned()),
                    expires_in_secs: None,
                }),
                // Well inside its life: the claim under test is that removal
                // takes it, not that the clock eventually does.
                expires_ms: platform.now_ms() + 60_000,
            },
        );

        engine.remove_profile(&p.id).expect("remove");

        // Asserted against the map itself rather than through
        // `cached_ssh_answer`, which reports an unpurged entry as absent once it
        // expires and would therefore agree with a removal that purged nothing.
        assert!(
            Engine::lock(&engine.lfs_ssh_credentials).is_empty(),
            "removal must take the minted LFS credential with it, not leave it to time out"
        );
    }

    /// The removal's own failure path, which is the half the story is easiest to
    /// get wrong: the rows fail *after* both secrets are gone (Story 34.7).
    ///
    /// The failure is injected in band — a `BEFORE DELETE` trigger that aborts —
    /// because that is what a locked or corrupt database does to this statement,
    /// and because the ordering under test is only observable when the second
    /// half fails.
    #[test]
    fn a_removal_that_fails_at_the_rows_has_still_taken_both_secrets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = profile(dir.path());
        p.remote_url = "git@git.invalid:x/y.git".to_owned();
        engine.upsert_profile(&p).expect("upsert");
        platform
            .secret_set(&p.secret_key(), "token-for-p")
            .expect("store a credential");
        let remote = lfs::ssh::SshRemote::parse(&p.remote_url).expect("an ssh remote");
        Engine::lock(&engine.lfs_ssh_credentials).insert(
            remote.cache_key(lfs::ssh::Operation::Upload),
            CachedSshAnswer {
                answer: lfs::ssh::Answer::Granted(lfs::ssh::Credential {
                    href: None,
                    authorization: Some("Bearer minted-for-the-removed-folder".to_owned()),
                    expires_in_secs: None,
                }),
                expires_ms: platform.now_ms() + 60_000,
            },
        );
        engine
            .with_db(|conn| {
                conn.execute_batch(
                    "CREATE TRIGGER refuse_profile_delete BEFORE DELETE ON profiles \
                     BEGIN SELECT RAISE(ABORT, 'the row delete failed'); END;",
                )?;
                Ok(())
            })
            .expect("install the trigger");

        engine
            .remove_profile(&p.id)
            .expect_err("the row delete must fail");

        // Both secrets are gone even though the removal did not complete, which
        // is the deliberate direction to fail in: what is left is a profile that
        // reports an authentication failure and can be removed again, never a
        // secret with no profile left to name it.
        assert_eq!(
            platform.secret_get(&p.secret_key()).expect("get"),
            None,
            "a half-failed removal must still have taken the keychain secret"
        );
        assert!(
            Engine::lock(&engine.lfs_ssh_credentials).is_empty(),
            "and the minted credential with it — it is purged before the rows for exactly this"
        );
        let survivors = engine.list_profiles().expect("list");
        assert!(
            survivors.iter().any(|stored| stored.id == p.id),
            "the profile itself survives, so the removal is retryable rather than lost"
        );
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
        let mut event = engine.progress(&p, SyncPhase::UploadingLfs);
        event.bytes_done = 42;
        engine.publish(event);
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.phase, SyncPhase::UploadingLfs);
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

    /// Every notification the engine has posted, in order.
    ///
    /// [`SyncPlatform::notify`] is the only surface a notification is
    /// observable on, so `TestPlatform`'s own recording is the spy for all of
    /// Story 29.6 — a second test double could only ever disagree with the port
    /// the shell and the daemon actually implement.
    fn notifications_posted(platform: &TestPlatform) -> Vec<(String, String)> {
        platform
            .notifications
            .lock()
            .map(|posted| posted.to_vec())
            .unwrap_or_default()
    }

    /// Story 29.6's headline, in the shape that actually broke: a warning that
    /// stays raised while its **wording moves** is still one notification.
    ///
    /// The supervisor ticks at 1 Hz, so `TICKS` is an hour of a folder that
    /// stayed broken. Several real warnings carry a number that moves with the
    /// condition they describe — the consecutive-failure count, the conflict
    /// tally — and an onset keyed on the message *text* read every one of those
    /// ticks as a fresh onset. A text-keyed or level-triggered implementation
    /// fails the first assertion below by three and a half thousand.
    #[test]
    fn a_sustained_warning_whose_wording_moves_every_tick_still_notifies_once() {
        const TICKS: u32 = 3_600;
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        for n in 1..=TICKS {
            engine.warn(&p.id, &p.name, format!("sync has failed {n} times"));
        }
        let posted = notifications_posted(&platform);
        assert_eq!(posted.len(), 1, "an hour of one warning is one toast");
        assert_eq!(posted[0].0, "Sync — fixture", "the toast names the folder");
        assert_eq!(posted[0].1, "sync has failed 1 times", "and is the onset's");

        // The *level* still reaches every surface a user can re-read: the banner
        // carries the latest wording, not the one that happened to notify. That
        // half must not be sacrificed to win the assertion above — a warning
        // that stops tracking its condition is AD-34-11's silent downgrade.
        let shown = engine.status(&p.id).expect("status").warning;
        assert_eq!(shown, Some(format!("sync has failed {TICKS} times")));

        // And the edge re-arms on `Some → None`: the same condition returning
        // after a clean run is a new event, not a suppressed duplicate.
        engine.clear_warning(&p.id);
        engine.warn(&p.id, &p.name, "sync has failed 1 times".to_owned());
        let posted = notifications_posted(&platform);
        assert_eq!(posted.len(), 2, "a cleared-then-recurring warning notifies");
    }

    /// The same guarantee through the caller that produces a moving message,
    /// rather than through a test that hand-rolls one.
    ///
    /// [`Engine::record_failure`]'s transient arm embeds the consecutive count,
    /// so every failure past the threshold reaches `warn` with different text —
    /// the exact sequence that used to notify once per tick. The pre-existing
    /// coverage stops at the threshold itself, where the bug is invisible
    /// because only one message has been formatted yet.
    #[test]
    fn a_long_run_of_transient_failures_notifies_once_however_long_it_runs() {
        const FAILURES: u32 = 3_600;
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let err = SyncError::Git("could not write the index".to_owned());
        for _ in 0..FAILURES {
            engine.record_failure(&p, &err);
        }

        let posted = notifications_posted(&platform);
        assert_eq!(posted.len(), 1, "an hour of failures is one toast");
        let (title, body) = &posted[0];
        assert_eq!(title, "Sync — fixture", "the toast names the folder");
        let onset = TRANSIENT_FAILURES_BEFORE_WARNING;
        assert!(
            body.contains(&format!("failed {onset} times")),
            "the toast carries the onset's own message, got: {body}"
        );
        let shown = engine.status(&p.id).expect("status").warning;
        assert!(
            shown.is_some_and(|line| line.contains(&format!("failed {FAILURES} times"))),
            "the banner keeps counting up while the toast stays quiet"
        );
    }

    /// Two folders going wrong are two onsets, each naming itself.
    ///
    /// The edge lives in each profile's own snapshot, so it cannot degenerate
    /// into one process-wide "already warned" flag — which would silence the
    /// second folder entirely, the failure mode this asserts against.
    #[test]
    fn two_profiles_that_both_go_wrong_each_notify_and_each_names_itself() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let first = profile(dir.path());
        let second = SyncProfile::new(
            "01JTESTPROFILE2",
            "second",
            dir.path().join("other"),
            "https://git.invalid/x/z.git",
        );
        engine.upsert_profile(&first).expect("upsert first");
        engine.upsert_profile(&second).expect("upsert second");

        engine.warn(&first.id, &first.name, "rename required".to_owned());
        engine.warn(&second.id, &second.name, "the push was rejected".to_owned());
        let posted = notifications_posted(&platform);
        assert_eq!(posted.len(), 2, "two onsets are two events");
        assert_eq!(posted[0].0, "Sync — fixture");
        assert_eq!(posted[0].1, "rename required");
        assert_eq!(posted[1].0, "Sync — second");
        assert_eq!(posted[1].1, "the push was rejected");

        // Both raised and both sustained: neither re-notifies, and one folder's
        // raised warning suppresses nothing on the other.
        engine.warn(&first.id, &first.name, "rename required".to_owned());
        engine.warn(&second.id, &second.name, "the push was rejected".to_owned());
        assert_eq!(notifications_posted(&platform).len(), 2);
    }

    /// A warning already raised when a subscriber attaches notifies **nobody** —
    /// asserted deliberately, because it is the intended behaviour.
    ///
    /// A notification belongs to the onset, not to the observer: the toast is an
    /// interruption, and interrupting someone about a condition that has been
    /// true for the last twenty minutes because a window happened to open is
    /// exactly the noise AD-51 keeps out of this subsystem. What a late arrival
    /// gets instead is the level — `warning` on the polled snapshot, which the
    /// AD-51 banner renders on its first paint — so nothing is missed. It is
    /// also why the platform port is the notification's only sink: routing it
    /// through the progress fan-out would turn every new subscriber into a
    /// fresh notification for the same old problem.
    #[test]
    fn a_warning_already_raised_does_not_notify_a_late_subscriber() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        engine.warn(&p.id, &p.name, "rename required".to_owned());
        assert_eq!(notifications_posted(&platform).len(), 1, "the onset posted");

        let sink: ProgressSink = Box::new(|_| true);
        let id = engine.subscribe(sink);
        let posted = notifications_posted(&platform);
        assert_eq!(posted.len(), 1, "attaching an observer is not an onset");

        // What the late arrival reads instead, and why it misses nothing.
        let shown = engine.status(&p.id).expect("status").warning;
        assert_eq!(shown.as_deref(), Some("rename required"));
        engine.unsubscribe(id);
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
    fn an_absent_volume_is_never_marked_on_the_disk_keeper_itself_lives_on() {
        // The dangerous half of adoption. `TestPlatform`'s data dir and the
        // profile folder share a volume here, exactly as they would if someone
        // ticked "removable" for a folder in their home directory — and a
        // marker minted there would bind the profile to the whole machine,
        // after which an unplugged drive could never be detected because there
        // is no drive.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.removable = true;
        std::fs::create_dir_all(&p.local_path).expect("create work dir");
        engine.upsert_profile(&p).expect("upsert");

        assert!(!engine.volume_ready(&p).expect("scan"), "must skip");
        assert_eq!(
            volume::find_mount_root(&p.local_path),
            None,
            "no marker may have been written anywhere above the folder"
        );
        assert_eq!(
            engine
                .list_profiles()
                .expect("list")
                .into_iter()
                .find(|stored| stored.id == p.id)
                .expect("stored")
                .volume_id,
            None,
            "a refused adoption must not leave the profile bound"
        );
    }

    #[test]
    fn a_marked_volume_binds_the_profile_and_records_it_in_the_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.removable = true;
        std::fs::create_dir_all(&p.local_path).expect("create work dir");
        engine.upsert_profile(&p).expect("upsert");
        // Stand in for a stick another machine — or another profile — already
        // adopted: the marker exists, so nothing is minted and the id in it is
        // the one this profile must take.
        let marker = VolumeMarker::ensure(dir.path(), "Field Drive", "other-profile", 1)
            .expect("plant a marker");

        assert!(engine.volume_ready(&p).expect("scan"), "the media is here");

        let stored = engine
            .list_profiles()
            .expect("list")
            .into_iter()
            .find(|stored| stored.id == p.id)
            .expect("stored");
        assert_eq!(
            stored.volume_id.as_deref(),
            Some(marker.volume_id.as_str()),
            "the profile must remember which volume it is on"
        );
        let on_disk = VolumeMarker::read(dir.path())
            .expect("read")
            .expect("present");
        assert!(
            on_disk.profile_ids.iter().any(|id| id == &p.id),
            "the marker must list the profile that joined it: {:?}",
            on_disk.profile_ids
        );
        assert_eq!(
            on_disk.volume_id, marker.volume_id,
            "joining a marked volume must never re-mint its identity"
        );
    }

    #[test]
    fn a_different_volume_at_the_same_path_stops_the_profile() {
        // Only reachable once a profile records the volume it expects: while
        // every scan asked for "any marker", a stranger's stick mounted at this
        // path read as `Present` and would have been synced into.
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.removable = true;
        p.volume_id = Some("01VOLUMEWEEXPECT".to_owned());
        std::fs::create_dir_all(&p.local_path).expect("create work dir");
        engine.upsert_profile(&p).expect("upsert");
        VolumeMarker::ensure(dir.path(), "Someone Else's Stick", "their-profile", 1)
            .expect("plant a foreign marker");

        assert!(!engine.volume_ready(&p).expect("scan"), "must refuse");
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::NeedsAttention
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
            .commit_local(p, SyncSource::Watch, None)
            .expect("first pass opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine
            .commit_local(p, SyncSource::Watch, None)
            .expect("second pass commits")
    }

    /// A committed fixture with content at the root and in three directories.
    ///
    /// Every file is tiny on purpose. `Engine::open` registers
    /// `filter.lfs.clean` as `std::env::current_exe()`, which under `cargo test`
    /// is the libtest harness; staying far below `lfs_threshold_bytes` keeps
    /// `.gitattributes` unwritten and that filter unreachable, so these tests
    /// measure the sparse cone and nothing else.
    fn committed_tree(engine: &Engine, platform: &TestPlatform, dir: &Path) -> SyncProfile {
        let p = adoptable(dir);
        for path in [
            "README.md",
            "docs/d.md",
            "media/NOTES.md",
            "media/video/v.txt",
            "media/audio/a.txt",
        ] {
            let target = p.local_path.join(path);
            std::fs::create_dir_all(target.parent().expect("a parent")).expect("fixture dir");
            std::fs::write(&target, path.as_bytes()).expect("fixture file");
        }
        engine.upsert_profile(&p).expect("upsert");
        assert!(
            commit_after_settling(engine, platform, &p) > 0,
            "the fixture must actually be committed, or nothing below is sparse"
        );
        let repo = engine.open_repo(&p).expect("open the fixture");
        assert!(
            git::repo::status_paths(&repo).expect("status").is_empty(),
            "the fixture must start clean, or no assertion about a clean sparse \
             materialization proves anything"
        );
        p
    }

    /// Every file in the working tree, repository-relative and sorted.
    fn materialized(root: &Path) -> Vec<String> {
        fn walk(root: &Path, at: &Path, out: &mut Vec<String>) {
            for entry in std::fs::read_dir(at).expect("read dir") {
                let path = entry.expect("dir entry").path();
                if path.file_name().is_some_and(|name| name == ".git") {
                    continue;
                }
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.push(
                        path.strip_prefix(root)
                            .expect("under the root")
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    /// Story 27.2's acceptance criterion, both halves of it.
    ///
    /// The second half is the one that bites. A cone checkout leaves the index
    /// holding every path — the out-of-cone ones flagged `SKIP_WORKTREE` — while
    /// the working tree holds only some of them, and any disagreement between
    /// those two views reports as a tree full of deletions. It reads clean only
    /// because AD-47 pairs the cone with a **non-sparse** index: `gix::status`
    /// skips a `SKIP_WORKTREE` entry, and hard-fails outright on an index that
    /// is genuinely sparse.
    #[test]
    fn a_profile_scoped_to_one_directory_materializes_only_it_and_reads_clean() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());

        p.subpaths = vec!["media/video".to_owned()];
        let repo = engine.open_repo(&p).expect("opening applies the cone");

        assert_eq!(
            materialized(&p.local_path),
            [
                // Cone mode keeps every root file and every file on the way in
                // to a cone root; see `crate::sparse` for why the LFS filter has
                // to agree with that and not with the narrower reading.
                "README.md",
                "media/NOTES.md",
                "media/video/v.txt",
            ],
            "docs/ and media/audio/ must be gone"
        );
        assert!(
            git::repo::status_paths(&repo).expect("status").is_empty(),
            "the acceptance criterion: a sparse materialization reads clean"
        );
    }

    /// The invariant Story 24.1 established and Story 27.2 restates, checked
    /// where it is actually decided.
    ///
    /// `git sparse-checkout` moves the repository to worktree-scoped
    /// configuration, and `.git/config.worktree` outranks `.git/config` for both
    /// git and gitoxide. So "it is written in `.git/config`" is not the same
    /// claim as "it is false", and only the second one keeps `gix::status`
    /// working.
    #[test]
    fn the_sparse_index_stays_disabled_across_a_sparse_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());
        p.subpaths = vec!["media/video".to_owned()];
        engine.open_repo(&p).expect("open");

        // Compared with the whitespace removed: what matters is that the key is
        // durably in the local scope, not how gix chose to indent it.
        let text = std::fs::read_to_string(p.local_path.join(".git/config")).expect("read config");
        let squashed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            squashed.contains("sparse=false"),
            "index.sparse must still be written in .git/config itself: {text}"
        );
        let reopened = git::repo::open(&p.local_path, true).expect("reopen");
        assert_eq!(
            reopened.config_snapshot().boolean("index.sparse"),
            Some(false),
            "and it must still be the effective value once the worktree scope is loaded"
        );
        assert_eq!(
            reopened.config_snapshot().boolean("core.sparseCheckout"),
            Some(true),
            "the fixture must really be sparse, or this proves nothing"
        );
    }

    /// Widening the cone brings the added directory back.
    ///
    /// The regression this locks down is not the widening itself but *where* the
    /// cone is applied: it used to run only on the branch that clones, so a
    /// profile whose subpaths changed after its first sync kept the cone it was
    /// born with for the rest of its life.
    #[test]
    fn widening_the_cone_materializes_the_added_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());
        p.subpaths = vec!["media/video".to_owned()];
        engine.open_repo(&p).expect("narrow");
        assert!(!p.local_path.join("docs/d.md").exists());

        p.subpaths = vec!["media/video".to_owned(), "docs".to_owned()];
        let repo = engine.open_repo(&p).expect("widen");

        assert_eq!(
            materialized(&p.local_path),
            [
                "README.md",
                "docs/d.md",
                "media/NOTES.md",
                "media/video/v.txt"
            ]
        );
        assert!(
            git::repo::status_paths(&repo).expect("status").is_empty(),
            "a widened cone reads clean too"
        );
    }

    /// Emptying `subpaths[]` returns the profile to a full checkout.
    ///
    /// Without this the field is one-way: a user could narrow a profile and
    /// never get their folder back, because nothing would ever run
    /// `sparse-checkout disable`.
    #[test]
    fn clearing_the_subpaths_restores_the_full_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());
        let before = materialized(&p.local_path);
        p.subpaths = vec!["media/video".to_owned()];
        engine.open_repo(&p).expect("narrow");

        p.subpaths.clear();
        let repo = engine.open_repo(&p).expect("widen to everything");

        assert_eq!(materialized(&p.local_path), before);
        assert!(git::repo::status_paths(&repo).expect("status").is_empty());
    }

    /// Narrowing removes only what git can put back, and says so about the rest.
    ///
    /// Removing `docs/d.md` is not data loss: it is committed, its bytes are in
    /// the object database, and re-widening restores them exactly — that is what
    /// [`widening_the_cone_materializes_the_added_directory`] proves. What is
    /// *not* recoverable is content git has never seen, and git refuses to
    /// remove that: it leaves the untracked file where it is and prints
    /// "directory 'docs/' contains untracked files, but is not in the
    /// sparse-checkout cone".
    ///
    /// So the file that survives here is the whole point. If a future git — or a
    /// future keeper that reached for `--force` or swept the directory itself —
    /// ever deleted it, this test fails, and it should.
    #[test]
    fn narrowing_the_cone_never_removes_untracked_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());
        let scratch = p.local_path.join("docs/NEVER-COMMITTED.md");
        std::fs::write(&scratch, b"the user's own, and git has never seen it").expect("scratch");

        p.subpaths = vec!["media/video".to_owned()];
        engine.open_repo(&p).expect("narrow");

        assert_eq!(
            std::fs::read(&scratch).expect("untracked content must survive a narrowing"),
            b"the user's own, and git has never seen it"
        );
        assert_eq!(
            materialized(&p.local_path),
            [
                "README.md",
                // The only removal, and it is restorable from HEAD.
                "docs/NEVER-COMMITTED.md",
                "media/NOTES.md",
                "media/video/v.txt",
            ],
            "docs/d.md is the only thing that may leave the working tree"
        );
    }

    /// An unchanged cone must not be re-applied.
    ///
    /// `sparse-checkout set` re-reads the tree and re-stats the working copy,
    /// and the repository is opened on every sync tick — on a removable drive
    /// (AD-48) that is the difference between a background daemon and a device
    /// that never spins down.
    ///
    /// Observed through a comment planted in the pattern file: keeper's reader
    /// ignores comments, so the cone still reads as applied and no `git` runs;
    /// git's writer does not preserve them, so a re-apply would wipe it. Nothing
    /// here depends on timing.
    #[test]
    fn an_unchanged_cone_is_not_re_applied_on_every_open() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = committed_tree(&engine, &platform, dir.path());
        p.subpaths = vec!["media/video".to_owned()];
        engine.open_repo(&p).expect("apply the cone");

        let patterns = p.local_path.join(".git/info/sparse-checkout");
        let planted = format!(
            "# planted by the test\n{}",
            std::fs::read_to_string(&patterns).expect("read patterns")
        );
        std::fs::write(&patterns, &planted).expect("plant");

        engine.open_repo(&p).expect("re-open with the same cone");

        assert_eq!(
            std::fs::read_to_string(&patterns).expect("read patterns"),
            planted,
            "the same cone must not be handed to `git sparse-checkout set` again"
        );
    }

    /// The scan consults the LFS guard and honours a "this is real work"
    /// answer, which no LFS path reached until `lfs::stage::indexed_pointer`
    /// stopped rejecting every one of them.
    ///
    /// # What this cannot cover, and what does
    ///
    /// Only the guard's `false` answer is reachable from here. Its `true`
    /// answer needs a racily-clean entry, and git only reaches that state by
    /// re-reading the worktree **through `filter.lfs.clean`** — which `Engine`
    /// registers as `std::env::current_exe()` (see `Engine::open`), the libtest
    /// harness under `cargo test`. Driving that from here would spawn the
    /// harness as a filter it cannot be, print its argument-parser error into
    /// an otherwise green run, and prove nothing about the clean filter.
    /// `tests/lfs_roundtrip.rs` owns that instead: it registers the filter
    /// itself, so it can measure the decision with a working one and without.
    ///
    /// The state used here needs no worktree read at all. A pointer written to
    /// "Not synced yet" used to mean "not synced yet, outbound".
    ///
    /// The Pending list is computed from `git status` and the completeness
    /// gate, which between them see only what this machine changed — so a
    /// folder pulling 53 GB listed NOTHING while 106 objects sat in the journal
    /// waiting to arrive. The inbound half comes from the journal, and it is
    /// the half that answers "what is this folder still missing".
    #[tokio::test]
    async fn pending_lists_what_is_still_coming_in_not_only_what_is_going_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        std::fs::write(p.local_path.join("clip.mp4"), vec![42u8; 200_000]).expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        assert!(
            engine
                .pending(&p.id)
                .await
                .expect("pending")
                .iter()
                .all(|file| !matches!(file.reason, PendingReason::Incoming { .. })),
            "nothing is queued yet, so nothing is coming in"
        );

        let unit = WorkKind::LfsDownload {
            oid: "d".repeat(64),
            size: 405_800_000,
        };
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &unit, 1, 1))
            .expect("enqueue");
        engine
            .with_db(|conn| db::label_unit(conn, id, "70-comms/keeper-rec/camera-0001.mov"))
            .expect("label");

        let pending = engine.pending(&p.id).await.expect("pending");
        let incoming = pending
            .iter()
            .find(|file| matches!(file.reason, PendingReason::Incoming { .. }))
            .expect("the queued download is listed");
        assert_eq!(incoming.path, "70-comms/keeper-rec/camera-0001.mov");
        assert_eq!(
            incoming.reason,
            PendingReason::Incoming {
                size_bytes: 405_800_000,
                // Nothing was ever materialized for this path in this test, and
                // that is what `replacing` reports — not what the repository's
                // history says about the file.
                replacing: false,
            },
            "the size is the whole question about an object that has not arrived"
        );
    }

    /// The Pending list is the surface the owner was watching, and it ran its
    /// OWN walk - so wiring the commit scan alone left it silent.
    ///
    /// That walk happens on a blocking thread, which cannot touch the engine,
    /// so its counts come back over a channel and are published here. This is
    /// the maintenance guard for that path: a Pending poll over a folder with
    /// work in it publishes the walk's progress, and the numbers are the pair
    /// the line renders.
    #[tokio::test]
    async fn a_pending_poll_publishes_the_progress_of_its_own_walk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(mut engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        engine.report_every_walk_item();
        let p = adoptable(dir.path());
        // The seed ignores itself, so the first commit needs a file of its own.
        std::fs::write(p.local_path.join(".gitignore"), b".gitignore\n").expect("seed");
        // Four hundred of them, because progress is published off a clock
        // now: a seven-file walk finishes inside the first tick and a test
        // built on one would be asserting that the clock is fast, not that the
        // poll reports. See `git::repo::WalkReport`.
        for i in 0..400 {
            std::fs::write(p.local_path.join(format!("committed-{i:05}.txt")), b"x")
                .expect("write");
        }
        engine.upsert_profile(&p).expect("upsert");
        // `pending` only walks a folder that is already a repository - the
        // first sync is what adopts it - so the fixture commits once before
        // there is anything for a poll to find.
        assert_eq!(commit_after_settling(&engine, &platform, &p), 400);
        for i in 0..6 {
            std::fs::write(p.local_path.join(format!("waiting-{i}.txt")), b"x").expect("write");
        }

        let published: Arc<Mutex<Vec<SyncProgress>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        engine.subscribe(Box::new(move |event| {
            Engine::lock(&sink).push(event);
            true
        }));

        let listed = engine.pending(&p.id).await.expect("pending");
        assert!(!listed.is_empty(), "the fixture has work, or nothing walks");

        let counts: Vec<u64> = Engine::lock(&published)
            .iter()
            .filter(|event| event.phase == SyncPhase::Scanning && event.files_done > 0)
            .map(|event| event.files_done)
            .collect();
        assert!(
            !counts.is_empty(),
            "the Pending poll walked and said nothing: {:?}",
            Engine::lock(&published).clone()
        );
        // Non-decreasing, not strictly increasing: progress is published off a
        // clock now, so a tick that lands between two comparisons honestly
        // repeats the figure rather than inventing movement. The macOS gate
        // caught this with `[120, 140, 193, 292, 346, 375, 400, 400]` — a walk
        // that had reached the end of the index and said so twice.
        assert!(
            counts
                .windows(2)
                .all(|pair| pair[1] >= pair[0] || pair[1] <= 1),
            "a count moved backwards without restarting: {counts:?}"
        );
    }

    /// A second version arriving is not the same thing as a file arriving, and
    /// the repository cannot tell them apart: a queued download finds pointer
    /// text in the worktree either way. What separates them is whether THIS
    /// machine has ever held content for that path, which is why keeper records
    /// it when content lands.
    #[tokio::test]
    async fn an_object_for_a_path_this_clone_has_held_is_marked_as_replacing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let queue = |oid: &str, label: &str| {
            let unit = WorkKind::LfsDownload {
                oid: oid.to_owned(),
                size: 4_096,
            };
            let id = engine
                .with_db(|conn| db::enqueue(conn, &p.id, &unit, 1, 1))
                .expect("enqueue");
            engine
                .with_db(|conn| db::label_unit(conn, id, label))
                .expect("label");
        };
        queue(&"a".repeat(64), "media/held-before.mov");
        queue(&"b".repeat(64), "media/never-seen.mov");

        // Only the first path has ever had content on this machine.
        engine
            .with_db(|conn| db::remember_materialized(conn, &p.id, "media/held-before.mov", 1))
            .expect("remember");

        let pending = engine.pending(&p.id).await.expect("pending");
        let reason = |path: &str| {
            pending
                .iter()
                .find(|file| file.path == path)
                .map(|file| file.reason.clone())
                .expect("listed")
        };
        assert_eq!(
            reason("media/held-before.mov"),
            PendingReason::Incoming {
                size_bytes: 4_096,
                replacing: true
            }
        );
        assert_eq!(
            reason("media/never-seen.mov"),
            PendingReason::Incoming {
                size_bytes: 4_096,
                replacing: false
            },
            "new here, whatever the repository's history says about its age"
        );
    }

    /// An object queued for a path since deleted cannot be named, and dropping
    /// it would make this list disagree with the count the status line prints.
    #[tokio::test]
    async fn an_unnameable_incoming_object_is_still_listed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let unit = WorkKind::LfsDownload {
            oid: "e".repeat(64),
            size: 10,
        };
        engine
            .with_db(|conn| db::enqueue(conn, &p.id, &unit, 1, 1))
            .expect("enqueue");

        let pending = engine.pending(&p.id).await.expect("pending");
        let incoming = pending
            .iter()
            .find(|file| matches!(file.reason, PendingReason::Incoming { .. }))
            .expect("an unnamed object is still work");
        assert!(
            incoming.path.starts_with("LFS object eeee"),
            "it says what it is rather than pretending to be a path: {}",
            incoming.path
        );
    }

    /// The figures follow the journal, with nothing in between to go stale.
    ///
    /// They used to be written into the snapshot by the drain loop — after a
    /// whole claimed batch of up to `CLAIM_LIMIT` units had been attempted. So
    /// a completed download vanished from the Pending list (that one is
    /// computed) while the line above it kept saying "94 files left, 48.7 GB",
    /// for as long as the rest of the batch took: hours, on the link this was
    /// reported from. No drain runs in this test, and none should have to.
    #[test]
    fn the_queue_figures_follow_the_journal_without_waiting_for_a_drain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let first = engine
            .with_db(|conn| {
                db::enqueue(
                    conn,
                    &p.id,
                    &WorkKind::LfsDownload {
                        oid: "a".repeat(64),
                        size: 4_000,
                    },
                    1,
                    1,
                )
            })
            .expect("enqueue");
        engine
            .with_db(|conn| {
                db::enqueue(
                    conn,
                    &p.id,
                    &WorkKind::LfsDownload {
                        oid: "b".repeat(64),
                        size: 1_000,
                    },
                    1,
                    1,
                )
            })
            .expect("enqueue");

        let before = engine.status(&p.id).expect("status");
        assert_eq!((before.queued_files, before.queued_bytes), (2, 5_000));

        engine
            .with_db(|conn| db::complete(conn, first))
            .expect("complete");

        let after = engine.status(&p.id).expect("status");
        assert_eq!(
            (after.queued_files, after.queued_bytes),
            (1, 1_000),
            "one unit finished, and nothing had to remember to say so"
        );
    }

    /// The queue that already exists is exactly the one that needs names.
    ///
    /// Naming happens at enqueue, so an install that upgrades mid-backlog has a
    /// queue of rows that will never pass through that moment again: on the
    /// folder that prompted this, 106 objects and 54.7 GB of them, every one
    /// reporting the folder's name instead of a file for as long as it took to
    /// deliver. The index is the only reverse map from object to path.
    #[test]
    fn a_queue_older_than_its_names_gets_them_filled_in() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        std::fs::write(p.local_path.join("clip.mp4"), vec![42u8; 200_000]).expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let repo = engine.open_repo(&p).expect("open");
        let pointer = lfs::stage::indexed_pointer(&repo, Path::new("clip.mp4"))
            .expect("the committed blob is a pointer");

        // A row shaped exactly like one queued before the column existed.
        let unit = WorkKind::LfsDownload {
            oid: pointer.oid.clone(),
            size: pointer.size,
        };
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &unit, 1, 1))
            .expect("enqueue");

        engine.backfill_labels(&p);

        let claimed = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, 10, 10))
            .expect("claim");
        let named = claimed
            .iter()
            .find(|item| item.id == id)
            .expect("the queued download is still there");
        assert_eq!(
            named.label.as_deref(),
            Some("clip.mp4"),
            "an unnamed queue is what the walk exists to answer"
        );
    }

    /// the index and not yet committed is a `HEAD`-to-index difference, which
    /// git reports from objects alone.
    #[test]
    fn a_pointer_staged_ahead_of_head_survives_the_scan_and_is_committed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        let payload = vec![42u8; 200_000];
        std::fs::write(p.local_path.join("clip.mp4"), &payload).expect("write");
        engine.upsert_profile(&p).expect("upsert");

        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let repo = engine.open_repo(&p).expect("open");
        let committed = lfs::stage::indexed_pointer(&repo, Path::new("clip.mp4"))
            .expect("the committed blob is a pointer");

        // Exactly what a kill between `stage_and_commit`'s index write and its
        // commit leaves behind: the index names a pointer `HEAD` has never
        // heard of. The worktree is not touched, so its stat still matches and
        // nothing re-reads the file.
        let staged = lfs::pointer::Pointer::new("c".repeat(64), payload.len() as u64);
        let blob = repo
            .write_blob(staged.render().as_bytes())
            .expect("write blob")
            .detach();
        let shared = repo.index_or_empty().expect("index");
        let mut index: gix::index::File = (**shared).clone();
        index
            .entry_mut_by_path_and_stage(
                gix::bstr::BStr::new("clip.mp4"),
                gix::index::entry::Stage::Unconflicted,
            )
            .expect("clip.mp4 is indexed")
            .id = blob;
        index
            .write(gix::index::write::Options::default())
            .expect("write index");
        drop(shared);
        drop(repo);

        // Age the index past the file. Everything above happened inside one
        // wall-clock second, which makes the entry racily clean — git would
        // re-read the worktree through `filter.lfs.clean` and this test would
        // be measuring the filter instead of the guard. In production the
        // index is always written after the file was last touched; a second of
        // sleeping would say the same thing far more slowly.
        let unracy = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(p.local_path.join(".git").join("index"))
            .and_then(|index| index.set_modified(unracy))
            .expect("age the index");

        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            1,
            "the guard must not dismiss a pointer HEAD does not record, or the \
             commit the crash interrupted is never re-driven"
        );
        let repo = engine.open_repo(&p).expect("open");
        let after = lfs::stage::indexed_pointer(&repo, Path::new("clip.mp4"))
            .expect("the path is still LFS-tracked");
        assert_eq!(
            after, committed,
            "the re-drive re-cleans the worktree, so the committed pointer \
             describes the bytes on disk rather than the interrupted guess"
        );
    }

    /// The precondition on publishing anything, and the release that keeps it
    /// from being a deadlock (Stories 34.15, 34.16).
    ///
    /// This is the defect the whole gate exists for. `do_push` used to commit
    /// the pointer and push it in the same call, leaving the queued upload for a
    /// later tick — so the remote held a pointer to content only this machine
    /// had, git reported the push as fine, and the next peer to clone got a
    /// working tree of ~130-byte text stubs with no error anywhere. `sync_once`
    /// had a comment claiming it drained the queue to prevent exactly this; it
    /// drained it *after* the push.
    #[tokio::test]
    async fn a_pointer_is_not_published_until_its_object_is_on_the_remote() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        std::fs::write(p.local_path.join("clip.mp4"), vec![42u8; 200_000]).expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // Open the quiescence episode, then let it elapse: the commit inside
        // `do_push` below is the one that stages the pointer.
        engine
            .commit_local(&p, SyncSource::Watch, None)
            .expect("first pass opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);

        // The push unit, claimed the way the supervisor claims it.
        let push = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, platform.now_ms(), 0))
            .expect("enqueue");
        let claimed = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, platform.now_ms(), 10))
            .expect("claim");
        assert_eq!(claimed.len(), 1, "only the push is queued yet");

        // The gate fires before anything touches the network, so this never
        // reaches the remote `adoptable` does not have.
        let err = engine
            .do_push(&p, SyncSource::Watch, Some(push))
            .await
            .expect_err("the push must be held");
        assert!(
            matches!(err, SyncError::LfsUploadPending { objects: 1 }),
            "got {err:?}"
        );
        assert_eq!(
            engine.lfs_uploads_outstanding(&p).expect("count"),
            1,
            "the commit queued the upload it owes"
        );

        // A wait, not a breakage — and specifically not "Large files missing",
        // which is what every deferred failure used to report and would have
        // sent someone looking for an unplugged drive.
        engine
            .reschedule_after(&p, push, 1, &err)
            .expect("defer the push");
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Syncing
        );

        // One claim answers two questions: the held push is NOT offered, and the
        // upload is — which is the whole shape of the gate. It also hands over
        // the upload's id, which the activity rows below have to name.
        let ready = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, platform.now_ms(), 10))
            .expect("claim");
        assert_eq!(ready.len(), 1, "the deferred push is not offered");
        assert!(
            matches!(ready[0].kind, WorkKind::LfsUpload { .. }),
            "got {:?}",
            ready[0].kind
        );
        let upload = ready[0].id;

        // Both files this commit carried, and what each is waiting on. The
        // pointer names its own upload; `.gitattributes` — which HAS to land in
        // the same commit or a peer would not know the pointer is one — names
        // the push.

        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let clip = rows
            .iter()
            .find(|row| row.path == "clip.mp4")
            .expect("the pointer was recorded");
        assert_eq!(clip.unit_id, Some(upload));
        assert_eq!(clip.delivery, db::DeliveryState::InProgress);

        let attributes = rows
            .iter()
            .find(|row| row.path == ".gitattributes")
            .expect(".gitattributes rides along");
        assert_eq!(attributes.unit_id, Some(push));
        assert_eq!(attributes.delivery, db::DeliveryState::InProgress);
        assert!(
            attributes
                .failure
                .as_deref()
                .is_some_and(|reason| reason.contains("on hold")),
            "the row says what it is waiting for, got: {:?}",
            attributes.failure
        );

        // The upload lands, and the next drain notices. Nothing else can notice:
        // `claim_ready` only ever looks at `pending` and a deferred unit waits on
        // a condition rather than a clock.
        engine
            .with_db(|conn| db::complete(conn, upload))
            .expect("complete");
        engine
            .release_satisfied_waits(&p, platform.now_ms())
            .expect("release");
        let released = engine
            .with_db(|conn| db::claim_ready(conn, &p.id, platform.now_ms(), 10))
            .expect("claim");
        assert_eq!(
            released.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![push],
            "the held push is queued again once nothing is outstanding"
        );

        // And the file row now says it arrived, while the one still riding the
        // push does not.
        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let clip = rows
            .iter()
            .find(|row| row.path == "clip.mp4")
            .expect("still recorded");
        assert_eq!(
            clip.delivery,
            db::DeliveryState::Success,
            "the unit left the journal, which is the only way it ever does"
        );
        assert_eq!(clip.unit_id, None, "there is nothing left to retry");
    }

    /// The obligation is durable before the thing that creates it (Story 34.15).
    ///
    /// The gate reads the journal and nothing else — no code anywhere re-derives
    /// the debt from committed-but-unpushed pointers — so the journal has to know
    /// about an upload before the commit naming it exists. This used to run the
    /// other way round: `stage_and_commit` moved the ref, and only then did one
    /// SQLite transaction per object record what was owed. A kill in between lost
    /// the obligation outright, and the next scan queued a push that published a
    /// pointer to bytes no other machine could ever obtain.
    ///
    /// Observed from *inside* the commit rather than by killing a process. The
    /// staging sink always reports the last path of the change set (see
    /// `git::commit::report`), and the engine's own closure turns that into a
    /// `Committing` frame carrying a path — which happens after the index and the
    /// blobs are written and before the ref transaction runs. That frame is the
    /// exact instant a `kill -9` was fatal, so it is the instant the row has to
    /// already be there.
    ///
    /// The row is read over a *second* connection on purpose: the question is
    /// whether the debt is committed to the database, not whether it is visible
    /// inside the writer's own session.
    #[test]
    fn the_upload_debt_is_journaled_before_the_commit_that_creates_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        std::fs::write(p.local_path.join("clip.mp4"), vec![42u8; 200_000]).expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // `(uploads owed, is there a commit yet)`, sampled mid-commit.
        let observed: Arc<Mutex<Vec<(u32, bool)>>> = Arc::new(Mutex::new(Vec::new()));
        let samples = Arc::clone(&observed);
        let db_file = db::db_path(dir.path());
        let work = p.local_path.clone();
        let profile_id = p.id.clone();
        engine.subscribe(Box::new(move |event: SyncProgress| {
            // A frame with a path in it comes from the staging loop, which only
            // runs inside `stage_and_commit`; `commit_local`'s own opening and
            // closing frames carry none.
            if event.phase != SyncPhase::Committing
                || event.files_done == 0
                || event.current.is_none()
            {
                return true;
            }
            let conn = rusqlite::Connection::open(&db_file).expect("open the journal read-only");
            let owed = db::outstanding_count(&conn, &profile_id, WorkKind::LFS_UPLOAD)
                .expect("count the uploads owed");
            let committed = gix::open(&work)
                .ok()
                .is_some_and(|repo| repo.head_id().is_ok());
            Engine::lock(&samples).push((owed, committed));
            true
        }));

        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let samples = Engine::lock(&observed).clone();
        assert_eq!(
            samples.len(),
            1,
            "the staging loop reports its last path exactly once, and that frame \
             is the whole measurement; got {samples:?}"
        );
        assert_eq!(samples, vec![(1u32, false)], "SAMPLES");
        let (owed, committed) = samples[0];
        assert!(
            !committed,
            "the sample has to be taken before the ref moves, or it proves nothing"
        );
        assert_eq!(
            owed, 1,
            "the upload must already be journaled while the commit is still being \
             written: a kill here used to lose the obligation, and the pointer \
             would then have been published with its object on this machine alone"
        );
    }

    /// A remote folder that comes back has to release the uploads it turned away
    /// (Story 34.18).
    ///
    /// `copy_lfs_object` defers on an unreachable remote, which is right — an
    /// unmounted volume is absence, not failure (AD-48). Nothing released it,
    /// though: `undefer_profile`'s callers are a resume and the volume re-attach
    /// arm of `volume_ready`, which returns early unless `profile.removable` —
    /// and `removable` describes the *local* folder's volume, not the remote's.
    /// Meanwhile `enqueue_unique` dedups against the deferred row so no fresh
    /// unit was ever queued, and `outstanding_count` counts it regardless of
    /// state, so the push stayed held. One transient unmount of the remote
    /// stopped the folder publishing permanently, and `list_parked` shows only
    /// `parked` work, so nothing said so anywhere.
    #[tokio::test]
    async fn an_unreachable_remote_defers_the_upload_and_a_mounted_one_releases_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote_dir = tempfile::tempdir().expect("tempdir");
        // The pendrive, unplugged: the profile names a path that is not there.
        let remote = remote_dir.path().join("stick").join("media.git");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_threshold_bytes = 1024;
        p.remote_url = remote.to_string_lossy().into_owned();
        std::fs::write(p.local_path.join("clip.mp4"), vec![42u8; 200_000]).expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // The upload finds nothing to copy to and waits for the volume.
        engine
            .drain_journal(&p, false, SyncSource::Watch)
            .await
            .expect("drain");
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::MediaAbsent
        );
        assert!(
            engine
                .with_db(|conn| db::claim_ready(conn, &p.id, platform.now_ms(), 10))
                .expect("claim")
                .is_empty(),
            "a deferred unit waits on a condition, so no clock offers it again"
        );
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 1);

        // The volume comes back. Nothing about the profile changes — the local
        // folder was never removable — so the remote's own reachability is the
        // only thing that can say so.
        if gix::init_bare(&remote).is_err() {
            return;
        }
        engine
            .drain_journal(&p, false, SyncSource::Watch)
            .await
            .expect("drain");
        assert_eq!(
            engine.lfs_uploads_outstanding(&p).expect("count"),
            0,
            "the released upload ran in the same drain that released it"
        );
        let store = lfs::local::remote_store(&p.remote_url).expect("a filesystem remote");
        let repo = engine.open_repo(&p).expect("open");
        let pointer = lfs::stage::indexed_pointer(&repo, Path::new("clip.mp4"))
            .expect("the committed blob is a pointer");
        drop(repo);
        assert!(
            store.contains(&pointer.oid, pointer.size),
            "the object the pointer names must be on the remote: {}",
            store.object_path(&pointer.oid).display()
        );

        // And with nothing outstanding the push is free to publish.
        engine
            .do_push(&p, SyncSource::Watch, None)
            .await
            .expect("publish, now that the object is there");
        let published = gix::open(&remote).expect("open the remote");
        assert!(
            published
                .find_reference(&format!("refs/heads/{}", p.branch))
                .is_ok(),
            "the branch has to reach the remote once its objects have"
        );
    }

    /// One object landing costs one path, never a walk of the repository.
    ///
    /// This is the shape of the field failure that produced
    /// [`Engine::materialize_landed`]. The download leg finished by calling the
    /// whole-tree sweep, so every delivered object re-read the index and then
    /// stat-and-read every tracked file small enough to be a pointer. On the
    /// folder that reported it — 154,765 tracked paths on a USB volume, 3.9 ms
    /// per path cold, so ~10 minutes a sweep — a 238-object backlog moved one
    /// file every 2 to 15 minutes and showed no throughput whatsoever, because
    /// the worker spent its life inside a sweep rather than on the wire. Left
    /// alone it would have taken about 40 hours to deliver 4.5 GB over a LAN.
    ///
    /// The cost is not directly observable from a test, but its cause is: the
    /// sweep materializes *every* pointer whose object happens to be local,
    /// while a targeted materialization touches the one path the journal row
    /// names. So a second pointer whose object is already in the store, left
    /// untouched by a download of the first, is exactly the discriminator — and
    /// leaving it is safe, because the sweep in `scan_and_enqueue` is one tick
    /// away and owns that whole-tree question.
    #[tokio::test]
    async fn a_landed_object_materializes_its_own_path_and_does_not_sweep_the_tree() {
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
        p.lfs_threshold_bytes = 1024;
        p.remote_url = remote_dir.path().to_string_lossy().into_owned();
        let payload = |seed: u8| vec![seed; 4_096];
        std::fs::write(p.local_path.join("landed.mp4"), payload(1)).expect("write");
        std::fs::write(p.local_path.join("sibling.mp4"), payload(2)).expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 2);

        // The inbound state, which is what a clone without a smudge filter
        // checks out: both objects are in the local store — the commit above put
        // them there — and both worktree files are pointer text.
        let repo = engine.open_repo(&p).expect("open");
        let pointer = |rela: &str| {
            lfs::stage::indexed_pointer(&repo, Path::new(rela)).expect("the blob is a pointer")
        };
        let landed = pointer("landed.mp4");
        let sibling = pointer("sibling.mp4");
        drop(repo);
        let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
        for (rela, ptr) in [("landed.mp4", &landed), ("sibling.mp4", &sibling)] {
            assert!(
                store.contains(&ptr.oid, ptr.size),
                "{rela}'s object has to be local for this test to discriminate"
            );
            std::fs::write(p.local_path.join(rela), ptr.render()).expect("check out the pointer");
        }

        // Exactly one object arrives, and the row names the path it arrives for.
        let unit = WorkKind::LfsDownload {
            oid: landed.oid.clone(),
            size: landed.size,
        };
        let id = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &unit, platform.now_ms(), 0))
            .expect("enqueue");
        engine
            .with_db(|conn| db::label_unit(conn, id, "landed.mp4"))
            .expect("label");
        engine
            .drain_journal(&p, false, SyncSource::Watch)
            .await
            .expect("drain");

        assert_eq!(
            std::fs::read(p.local_path.join("landed.mp4")).expect("read"),
            payload(1),
            "the path the object landed for holds content"
        );
        assert_eq!(
            std::fs::read_to_string(p.local_path.join("sibling.mp4")).expect("read"),
            sibling.render(),
            "and no other path was even looked at — the sweep owns that"
        );
    }

    /// A push stranded in `deferred` has to be picked up by the next drain
    /// (Story 34.15).
    ///
    /// The release used to fire only from an upload's own completion, over three
    /// separate transactions: `complete`, then the count, then the release. A
    /// `kill -9` in that window — or a `SQLITE_BUSY` whose `?` propagated out of
    /// `drain_journal` — left the push `deferred` with nothing outstanding.
    /// `recover_running` only rescues `running` rows, `claim_ready` never offers
    /// a deferred one, and `enqueue_unique` dedups against it, so the folder
    /// silently stopped publishing forever while reporting `Syncing` with one
    /// pending unit. This is that state, planted rather than raced for.
    #[tokio::test]
    async fn a_push_stranded_in_deferred_is_released_by_the_next_drain() {
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
        std::fs::write(p.local_path.join("notes.md"), b"unpublished").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // Exactly what the crash leaves: the uploads are gone from the journal
        // and the push that was waiting for them is still deferred.
        let push = engine
            .with_db(|conn| db::enqueue(conn, &p.id, &WorkKind::Push, platform.now_ms(), 0))
            .expect("enqueue");
        engine
            .with_db(|conn| {
                db::reschedule(
                    conn,
                    push,
                    WorkState::Deferred,
                    platform.now_ms(),
                    Some("publishing is on hold until this folder's large files reach the remote"),
                )
            })
            .expect("strand the push");
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);

        engine
            .drain_journal(&p, false, SyncSource::Watch)
            .await
            .expect("drain");

        let published = gix::open(remote_dir.path()).expect("open the remote");
        assert!(
            published
                .find_reference(&format!("refs/heads/{}", p.branch))
                .is_ok(),
            "the stranded push must publish on the next drain, not wait for a \
             pause-and-resume that nobody knows to perform"
        );
        assert_eq!(
            engine.pending_for(&p.id).expect("pending"),
            0,
            "and it leaves the journal, so the folder stops claiming work"
        );
    }

    /// A healthy first sync of a folder with more large files than one claim
    /// batch must succeed (Story 34.15, Story 34.18).
    ///
    /// `drain_journal` claims `CLAIM_LIMIT` units at a time, so one call left the
    /// seventeenth upload outstanding, the gate refused, and `sync_exit_code`
    /// turned that into a non-zero exit: `keeper-syncd sync` under
    /// `Restart=on-failure` failed on a folder where nothing was wrong, and "Sync
    /// now" in the app reported the same thing as an error.
    #[tokio::test]
    async fn a_one_shot_sync_publishes_a_folder_with_more_objects_than_one_claim_batch() {
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
        p.lfs_threshold_bytes = 1024;
        p.remote_url = remote_dir.path().to_string_lossy().into_owned();
        // One more than the claim batch, which is the whole point: the count has
        // to come from `CLAIM_LIMIT` rather than from a number typed here.
        let objects = CLAIM_LIMIT as usize + 1;
        for i in 0..objects {
            std::fs::write(
                p.local_path.join(format!("clip-{i:02}.mp4")),
                vec![7u8; 4_096],
            )
            .expect("write");
        }
        engine.upsert_profile(&p).expect("upsert");

        // The first pass opens the quiescence episode; the second commits, and
        // that commit is what owes the objects.
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("the opening pass has nothing settled to do");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        let outcome = engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("a healthy folder with 17 large files is not a failed sync");

        assert!(outcome.pushed);
        assert_eq!(
            engine.lfs_uploads_outstanding(&p).expect("count"),
            0,
            "every object landed before the call returned"
        );
        let published = gix::open(remote_dir.path()).expect("open the remote");
        assert!(
            published
                .find_reference(&format!("refs/heads/{}", p.branch))
                .is_ok(),
            "and the commit naming them was published"
        );
        let store = lfs::local::remote_store(&p.remote_url).expect("a filesystem remote");
        let repo = engine.open_repo(&p).expect("open");
        for i in 0..objects {
            let rela = PathBuf::from(format!("clip-{i:02}.mp4"));
            let pointer =
                lfs::stage::indexed_pointer(&repo, &rela).expect("every clip is a pointer");
            assert!(
                store.contains(&pointer.oid, pointer.size),
                "{} must be on the remote: {}",
                rela.display(),
                store.object_path(&pointer.oid).display()
            );
        }
    }

    /// The stored keychain token belongs to the remote's host and nowhere else
    /// (Story 34.17).
    ///
    /// `git-lfs-authenticate` may answer with an `href` and no `Authorization`,
    /// which is legal and is why the stored token is a fallback at all. But the
    /// response parser validates only the href's *scheme*, so the host in it is
    /// whatever the server felt like sending — and pairing the two meant a server
    /// could name any host on the internet and be handed the user's PAT, in
    /// cleartext for an `http` href.
    #[test]
    fn a_server_named_endpoint_on_another_host_never_receives_the_stored_credential() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = profile(dir.path());
        p.remote_url = "ssh://git@git.example.com/o/r.git".to_owned();
        let remote = lfs::ssh::SshRemote::parse(&p.remote_url).expect("an ssh remote");
        let stored = || Some("Basic c3RvcmVkOnBhdA==".to_owned());
        let granted = |href: &str, authorization: Option<&str>| lfs::ssh::Credential {
            href: Some(url::Url::parse(href).expect("a valid href")),
            authorization: authorization.map(str::to_owned),
            expires_in_secs: None,
        };

        // Another host, no credential of its own: it gets the endpoint it asked
        // for and nothing else.
        let (endpoint, auth) = engine
            .spend(
                &p,
                &remote,
                granted("https://harvester.example.net/o/r.git/info/lfs", None),
                stored(),
            )
            .expect("spend");
        assert_eq!(
            endpoint.as_str(),
            "https://harvester.example.net/o/r.git/info/lfs",
            "the href is still used verbatim; it is the credential that is withheld"
        );
        assert_eq!(
            auth, None,
            "the stored PAT must not leave the remote's host"
        );

        // The remote's own host, spelled with the userinfo and a different case,
        // both of which say nothing about where the request goes.
        let (_, auth) = engine
            .spend(
                &p,
                &remote,
                granted("https://GIT.example.com/o/r.git/info/lfs", None),
                stored(),
            )
            .expect("spend");
        assert_eq!(
            auth,
            stored(),
            "on the remote's own host the stored token is the documented fallback"
        );

        // A credential the server minted for the endpoint it named is honoured
        // wherever it named it: that pairing is the whole content of the
        // handshake, and CDN-fronted object stores depend on it.
        let (endpoint, auth) = engine
            .spend(
                &p,
                &remote,
                granted("https://objects.example.net/lfs", Some("Bearer eyJhbGciOi")),
                stored(),
            )
            .expect("spend");
        assert_eq!(endpoint.as_str(), "https://objects.example.net/lfs");
        assert_eq!(auth.as_deref(), Some("Bearer eyJhbGciOi"));

        // And a credential with no href at all, for a remote no endpoint can be
        // derived from, is an error rather than a request. This used to answer
        // `https://invalid.localhost/` with the credential still attached, and
        // `*.localhost` resolves to loopback on every mainstream resolver — so
        // the "unreachable" fallback was a real authenticated request to whatever
        // was listening on local 443.
        p.remote_url = "/media/stick/media.git".to_owned();
        let err = engine
            .spend(
                &p,
                &remote,
                lfs::ssh::Credential {
                    href: None,
                    authorization: None,
                    expires_in_secs: None,
                },
                stored(),
            )
            .expect_err("an endpoint that cannot be derived is not a URL to invent");
        assert!(
            !err.to_string().contains("localhost"),
            "the failure must name the derivation, not a fabricated host: {err}"
        );
    }

    /// A folder held behind an abandoned upload is stopped, not syncing
    /// (Story 34.15).
    ///
    /// `outstanding_count` counts a parked upload deliberately — that is what
    /// keeps the pointer unpublished — but the *state word* read from the same
    /// fact was `Syncing`, and `is_warning()` for it is false. So a 403 on one
    /// object left the tray glyph and the folder pane both showing healthy
    /// progress for a folder that had permanently ceased publishing, with the
    /// only trace in a problems list nobody had been given a reason to open.
    #[test]
    fn a_permanently_parked_upload_stops_the_folder_calling_itself_healthy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let now = 1_700_000_000_000;
        let upload = engine
            .with_db(|conn| {
                db::enqueue(
                    conn,
                    &p.id,
                    &WorkKind::LfsUpload {
                        oid: "a".repeat(64),
                        size: 200_000,
                    },
                    now,
                    now,
                )
            })
            .expect("enqueue the upload");
        let held = SyncError::LfsUploadPending { objects: 1 };

        // While the upload is still being attempted, a held push is exactly what
        // it says: work in progress.
        engine.record_failure(&p, &held);
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::Syncing
        );

        // Once keeper has given up on it, nothing will move it but a human.
        engine
            .with_db(|conn| db::reschedule(conn, upload, WorkState::Parked, now, Some("403")))
            .expect("park the upload");
        engine.record_failure(&p, &held);
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::NeedsAttention,
            "a wait that can never end is not progress, and the state word is \
             what the tray reads"
        );
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
        std::fs::write(p.local_path.join("b.txt"), b"twelve bytes").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        engine
            .commit_local(&p, SyncSource::Watch, None)
            .expect("first pass");
        assert!(
            engine
                .activity(&p.id, 10)
                .await
                .expect("activity")
                .is_empty(),
            "nothing may be recorded before a commit exists"
        );

        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit"),
            2
        );

        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let mut seen: Vec<(ActivityKind, String, Option<u64>)> = rows
            .iter()
            .map(|r| (r.kind, r.path.clone(), r.size_bytes))
            .collect();
        seen.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            seen,
            vec![
                (ActivityKind::Added, "a.txt".to_owned(), Some(3)),
                (ActivityKind::Added, "b.txt".to_owned(), Some(12)),
            ],
            "exactly the committed paths, repository-relative, as additions, each \
             carrying the size it was committed at"
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
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("removal"),
            1
        );
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("edit"),
            1
        );

        let rows = engine.activity(&p.id, 10).await.expect("activity");
        let mut latest: Vec<(ActivityKind, String, Option<u64>)> = rows
            .iter()
            .take(2)
            .map(|r| (r.kind, r.path.clone(), r.size_bytes))
            .collect();
        latest.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            latest,
            vec![
                (ActivityKind::Modified, "a.txt".to_owned(), Some(10)),
                // Nothing is left on disk to measure, so this size can only
                // have come from what the index still recorded for the path.
                (ActivityKind::Deleted, "b.txt".to_owned(), Some(12)),
            ],
            "newest first, each path carrying the kind that actually happened and \
             the size that moved"
        );
        assert_eq!(rows.len(), 4, "earlier entries are kept, not replaced");
    }

    /// Story 40.4: a retitle MOVES a session folder, and `git log --follow`
    /// can only reach the pre-rename history when the disappearance and the
    /// arrival share a commit — git stores no rename metadata and infers one
    /// from content at diff time.
    ///
    /// The control half is the bug the priming API exists for, asserted rather
    /// than described: the deletion goes out at once (nothing is left to
    /// sample) while the arrival is an absolute path tier 2 has never observed,
    /// so one move becomes two commits a settle window apart.
    #[tokio::test]
    async fn priming_a_moved_file_commits_the_delete_and_the_add_together() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        // Byte-identical, because a rename is only inferable from content: two
        // files that differ would prove nothing about `--follow` either way.
        let bytes = b"one recording segment, byte for byte";
        std::fs::write(p.local_path.join("cold.bin"), bytes).expect("write");
        std::fs::write(p.local_path.join("warm.bin"), bytes).expect("write");
        engine.upsert_profile(&p).expect("upsert");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 2);

        // Control — no prime. Two commits, and the history is severed between
        // them.
        std::fs::rename(
            p.local_path.join("cold.bin"),
            p.local_path.join("cold-retitled.bin"),
        )
        .expect("rename");
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("cold move"),
            1,
            "an unprimed move commits its deletion alone"
        );
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("cold arrival"),
            1,
            "and its arrival a whole settle window later, in a second commit"
        );

        // Primed — one commit carrying both halves.
        let moved = p.local_path.join("warm-retitled.bin");
        std::fs::rename(p.local_path.join("warm.bin"), &moved).expect("rename");
        assert_eq!(
            engine
                .prime_moved_paths(&p.id, std::slice::from_ref(&moved))
                .expect("prime"),
            1
        );
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("warm move"),
            2,
            "a primed move commits the deletion and the arrival as one change"
        );

        // The count above is one `stage_and_commit` call, so both paths are in
        // one commit by construction. This is what that commit actually said.
        let rows = engine.activity(&p.id, 2).await.expect("activity");
        let mut latest: Vec<(ActivityKind, String)> =
            rows.iter().map(|r| (r.kind, r.path.clone())).collect();
        latest.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            latest,
            vec![
                (ActivityKind::Added, "warm-retitled.bin".to_owned()),
                (ActivityKind::Deleted, "warm.bin".to_owned()),
            ],
            "both halves of the move, recorded against the same commit"
        );
        assert!(
            rows.iter().all(|r| r.ts_ms == platform.now_ms()),
            "and at the same instant — the clock never advanced between them"
        );

        // Paths outside the profile are skipped rather than refused, so a
        // caller may hand over whatever a walk found.
        assert_eq!(
            engine
                .prime_moved_paths(&p.id, &[dir.path().join("elsewhere.bin")])
                .expect("prime"),
            0
        );
    }

    /// A profile that holds recordings, with its root already on disk.
    ///
    /// The default [`crate::profile::RecordingsConfig`] is what story 41.2
    /// hands an owner with a single recordings-flagged profile, so a test built
    /// on it is testing the shape the product actually ships.
    fn recording_profile(dir: &Path) -> (SyncProfile, PathBuf) {
        let mut p = adoptable(dir);
        p.recordings = Some(crate::profile::RecordingsConfig::default());
        let root = p
            .recordings_root()
            .expect("a recordings profile has a recordings root");
        std::fs::create_dir_all(&root).expect("recordings root");
        (p, root)
    }

    /// The one-tick proof (Story 41.4, FR-134).
    ///
    /// The property is a *count*, not a duration: the injected clock is never
    /// advanced anywhere in this test, so a segment that needed the settle
    /// window could not reach it however many passes it were given, and the
    /// budget below fails the test on the second pass rather than hanging.
    /// Elapsed time is deliberately not asserted on — it would pass on a fast
    /// machine and fail on a loaded one, and it is not what the story claims.
    ///
    /// The control half in the same profile is what makes it an assertion about
    /// the *assertion* rather than about the gate being lenient.
    #[tokio::test]
    async fn a_finished_segment_commits_on_the_next_pass_and_an_unasserted_one_waits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let (p, root) = recording_profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        // Control: an ordinary rotated segment nobody vouched for. Two passes
        // and a whole settle window, which is the latency this story exists to
        // remove.
        let unasserted = root.join("segment-000.mov");
        std::fs::write(&unasserted, b"a segment the recorder never announced").expect("write");
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("first pass"),
            0,
            "one observation is not two: the unasserted segment is held"
        );
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("second pass, still inside the window"),
            0,
            "and no number of passes inside the window will free it"
        );
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("the window has elapsed"),
            1
        );

        // The assertion, delivered the way the recorder delivers it: through a
        // handle that is not an `Engine` (AD-68), applied by the drain the tick
        // runs before it walks anything.
        let finished = root.join("segment-001.mov");
        std::fs::write(&finished, b"finishWriting returned; these bytes are final").expect("write");
        let tap = engine.finished_tap();
        assert!(
            tap.note_finished(&p.id, &finished),
            "the assertion is queued"
        );
        engine.drain_finished_assertions();

        let clock_at_assertion = platform.now_ms();
        /// One. That is the whole claim.
        const PASS_BUDGET: u32 = 1;
        let mut passes = 0u32;
        let committed = loop {
            passes += 1;
            assert!(
                passes <= PASS_BUDGET,
                "an asserted segment must commit on the pass that follows the assertion; \
                 this one was still held after {} pass(es)",
                passes - 1
            );
            let landed = engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("a pass over the tree");
            if landed > 0 {
                break landed;
            }
        };
        assert_eq!((passes, committed), (1, 1));
        assert_eq!(
            platform.now_ms(),
            clock_at_assertion,
            "and it got there without a millisecond of settle window"
        );
    }

    /// The guard is the authorisation story (FR-135): the producer may vouch
    /// for its own output and for nothing else. Every refusal here is `Ok`,
    /// because a recorder mid-capture has nothing to do with an error (NFR-31);
    /// only a request the engine cannot interpret at all is an `Err`.
    #[tokio::test]
    async fn a_finished_assertion_is_refused_unless_it_names_this_profile_s_recordings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let (p, root) = recording_profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        // Inside the profile, outside the recordings root. The interesting
        // case: the producer can reach the file, and still may not vouch for it.
        let elsewhere = p.local_path.join("taxes.ods");
        std::fs::write(&elsewhere, b"not the recorder's to declare finished").expect("write");
        assert!(
            !engine
                .note_finished_path(&p.id, &elsewhere)
                .expect("a refusal is not an error the recorder has to handle"),
            "a path outside the recordings root is refused"
        );
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("first pass"),
            0,
            "and it did not become stable early — it is on the ordinary path"
        );
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("the window has elapsed"),
            1,
            "which it walks to the end, exactly as if nothing had been asserted"
        );

        // A profile that was removed between the close and the tick.
        assert!(!engine
            .note_finished_path("01JNOSUCHPROFILEXXXXXXXXXX", &root.join("segment-000.mov"))
            .expect("an unknown profile degrades, it does not fail"));

        // A profile that holds no recordings at all has no root to be inside of.
        let mut plain = profile(dir.path());
        plain.id = "01JPLAINPROFILEXXXXXXXXXXX".to_owned();
        plain.local_path = dir.path().join("plain");
        std::fs::create_dir_all(&plain.local_path).expect("work dir");
        engine.upsert_profile(&plain).expect("upsert");
        assert!(!engine
            .note_finished_path(&plain.id, &plain.local_path.join("segment-000.mov"))
            .expect("a non-recordings profile degrades too"));

        // Malformed, and the only thing here worth an `Err`: neither can be
        // checked against the root, since `starts_with` compares components and
        // would accept the second one.
        assert!(engine
            .note_finished_path(&p.id, Path::new("segment-000.mov"))
            .is_err());
        assert!(engine
            .note_finished_path(&p.id, &root.join("../../etc/passwd"))
            .is_err());
    }

    /// A `.partial` is what the recorder writes *while* the segment is open
    /// (AD-69). Tier 0 outranks the assertion, and an engine that let a
    /// producer talk it out of an exclusion would commit a half-written file.
    #[tokio::test]
    async fn an_asserted_partial_is_still_invisible_to_the_engine() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let (p, root) = recording_profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let partial = root.join("segment-000.mov.partial");
        std::fs::write(&partial, b"mid-rotation, still open").expect("write");
        assert!(
            !engine
                .note_finished_path(&p.id, &partial)
                .expect("an excluded path is not an error either"),
            "tier 0 declines the assertion"
        );

        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("a pass over the tree"),
            0,
            "and the file stays out of the commit however long it sits there"
        );
    }

    /// Losing the assertion across a pause would be the worst place to lose it:
    /// the segment would wait out a settle window that the pause itself made
    /// meaningless, on top of however long the pause lasted.
    #[tokio::test]
    async fn a_paused_profile_records_a_finished_assertion_and_honours_it_on_resume() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let (p, root) = recording_profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        // One ordinary commit first, so the repository exists and the pause
        // below is the only thing that changes.
        std::fs::write(p.local_path.join("readme.md"), b"a folder with a history").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        engine.set_enabled(&p.id, false).expect("pause");
        let finished = root.join("segment-000.mov");
        std::fs::write(&finished, b"closed while the profile was paused").expect("write");
        assert!(
            engine
                .note_finished_path(&p.id, &finished)
                .expect("a paused profile is not a refusal"),
            "a pause suspends syncing, not the producer's knowledge"
        );

        engine.set_enabled(&p.id, true).expect("resume");
        assert!(
            !Engine::lock(&engine.gates).contains_key(&p.id),
            "resuming drops the in-memory gate, so what follows can only have \
             come back from `file_state`"
        );

        let clock_at_resume = platform.now_ms();
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("the first pass after the resume"),
            1,
            "the assertion survived the pause and the gate rebuild"
        );
        assert_eq!(
            platform.now_ms(),
            clock_at_resume,
            "with no settle window waited out on the far side"
        );
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
        engine
            .commit_local(&p, SyncSource::Watch, None)
            .expect("scan");

        let pending = engine.pending(&p.id).await.expect("pending");
        assert_eq!(
            pending,
            vec![PendingFile {
                path: "held.txt".to_owned(),
                reason: PendingReason::Settling {
                    since_ms: opened_at
                },
                // The bytes the gate is holding, measured off the worktree.
                size_bytes: Some(13),
            }],
            "a held file reports why it is held and since when — not `untracked`, \
             which would read as sync being broken"
        );

        // Once it settles and lands, it is not pending at all any more.
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit"),
            1
        );
        assert!(engine.pending(&p.id).await.expect("pending").is_empty());
    }

    /// The scan is not a query, and treating it as one lost the file.
    ///
    /// [`StabilityGate::is_stable`] ends a path's episode the instant it
    /// answers `Stable` — reading the verdict consumes it. `scan_and_enqueue`
    /// walked the tree only to decide whether to queue a push and then threw
    /// the staged set away, so the one pass on which a file finally settled was
    /// the pass that spent its verdict and committed nothing. The file was
    /// untracked again on the next walk: a first observation, with a brand-new
    /// window. On a tree scanned more often than its settle window it never
    /// survived to a commit at all, and the folder sat at "N waiting for writes
    /// to stop" indefinitely with every wait honestly reading "under a minute
    /// so far", because each one had genuinely just started.
    #[tokio::test]
    async fn a_scan_commits_what_settled_rather_than_spending_the_verdict() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("segment-000.mov"), b"a closed segment").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        // With an empty journal the scan is the ONLY thing the supervisor runs,
        // so it is the only thing that ever advances an episode. That is what
        // makes discarding its result fatal rather than merely wasteful.
        engine
            .scan_and_enqueue(&p, SyncSource::Watch)
            .expect("the first scan opens the episode");
        assert_eq!(
            engine.pending(&p.id).await.expect("pending").len(),
            1,
            "one observation is not two: the segment is still inside its window"
        );

        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine
            .scan_and_enqueue(&p, SyncSource::Watch)
            .expect("the window has elapsed");

        assert!(
            engine.pending(&p.id).await.expect("pending").is_empty(),
            "the pass that settles a file must be the pass that commits it"
        );
        assert!(
            engine
                .activity(&p.id, 10)
                .await
                .expect("activity")
                .iter()
                .any(|entry| entry.path == "segment-000.mov"),
            "and the commit is real — an emptied gate is not a synced file"
        );
    }

    /// A short-cadence caller that asks the engine to LOOK must not also make
    /// it FORGET.
    ///
    /// The notes cadence called [`Engine::rescan`] to mean "notice now" — but
    /// that is the "Recheck all files" button, and it drops the gate for the
    /// whole profile. On a folder that is both a notes vault and a recordings
    /// destination it ran about once a second, so nothing but a note could ever
    /// finish settling: a recording stopped, wrote its note stub, the stub made
    /// the vault dirty, the cadence fired, and the segments the recording had
    /// just closed lost the episode they needed. The recording could not sync
    /// because it had written a note about itself.
    ///
    /// Both halves are asserted here, because "wake keeps it" is only half the
    /// claim — the other half is that `rescan` still forgets, deliberately, and
    /// a fix that made them the same thing would break the button instead.
    #[tokio::test]
    async fn a_cadence_that_wakes_lets_a_file_settle_where_one_that_rescans_never_can() {
        // The control. A forget on every pass, and the file never lands however
        // long it has been quiet — the shape of the reported bug.
        let forgetting = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(forgetting.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(forgetting.path());
        std::fs::write(p.local_path.join("segment-000.mov"), b"a closed segment").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        for _ in 0..4 {
            engine.rescan(&p.id).expect("the cadence fires");
            engine
                .scan_and_enqueue(&p, SyncSource::Watch)
                .expect("and the supervisor walks");
            platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        }
        assert!(
            engine
                .activity(&p.id, 10)
                .await
                .expect("activity")
                .is_empty(),
            "a forget on every pass restarts every episode: nothing can ever settle"
        );

        // The same cadence, asking only to look.
        let waking = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(waking.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(waking.path());
        std::fs::write(p.local_path.join("segment-000.mov"), b"a closed segment").expect("write");
        engine.upsert_profile(&p).expect("upsert");

        for _ in 0..4 {
            engine.wake_now(&p.id);
            engine
                .scan_and_enqueue(&p, SyncSource::Watch)
                .expect("and the supervisor walks");
            platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        }
        assert!(
            engine
                .activity(&p.id, 10)
                .await
                .expect("activity")
                .iter()
                .any(|entry| entry.path == "segment-000.mov"),
            "a wake keeps the episode, so the second observation clears it"
        );
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
                // `b"x"`, measured off the worktree like every outbound row.
                size_bytes: Some(1),
            }]
        );
    }

    /// Story 47.2 — the Files pane and the commit path must not disagree about
    /// a file whose name is not valid UTF-8, and the folder must not be silent
    /// about it.
    ///
    /// The owner's report was "one file whose name is not valid UTF-8, which
    /// probably would not make it to the remote anyway". The second half is
    /// wrong and this test is where that is written down: git stores path
    /// bytes and never decodes them, so the file commits like any other. What
    /// was broken was that no surface ever said the file existed in a form a
    /// person could act on, and that the two surfaces which *did* see it could
    /// not be shown to be talking about the same file.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_files_pane_and_the_commit_path_agree_about_a_name_neither_can_spell() {
        use std::os::unix::ffi::OsStringExt;

        // Built from raw bytes. A string literal cannot express this name, and
        // a test that faked it with valid UTF-8 would pass over the bug.
        let odd_name = std::ffi::OsString::from_vec(b"doc-\xffepuap.txt".to_vec());

        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        let odd_path = p.local_path.join(&odd_name);
        if crate::names::create_unspellable(&p.local_path, b"doc-\xffepuap.txt").is_none() {
            eprintln!("{}", crate::names::UNSPELLABLE_UNAVAILABLE);
            return;
        }
        std::fs::write(&odd_path, b"a scan of a form").expect("write the odd one");
        std::fs::write(p.local_path.join("ordinary.txt"), b"x").expect("write the ordinary one");
        engine.upsert_profile(&p).expect("upsert");
        engine.ensure_repo(&p).expect("adopt");

        // 1. The engine's own answer to "what has this folder not synced yet".
        let pending = engine.pending(&p.id).await.expect("pending");
        let engine_paths: Vec<&str> = pending.iter().map(|f| f.path.as_str()).collect();
        assert!(
            engine_paths.contains(&"doc-\u{FFFD}epuap.txt"),
            "the engine must not drop it either; got {engine_paths:?}"
        );

        // 2. The Files pane, joined to that same answer.
        let view = crate::browse::PendingView::Known(
            pending
                .iter()
                .map(|f| (f.path.clone(), f.reason.clone()))
                .collect(),
        );
        let crate::browse::BrowseListing::Listed(listed) = crate::browse::browse(
            &p,
            "",
            &ExcludeSet::new(&p.excludes).expect("excludes"),
            &view,
            // The path is untracked, so `classify` answers at the waiting rung
            // and never reaches the ledger. Saying "no rows" is therefore the
            // honest input as well as the cheap one.
            &crate::browse::MaterializedView::none(),
        )
        .expect("no refusal") else {
            panic!("expected a listing");
        };
        let shown = listed
            .entries
            .iter()
            .find(|e| e.unspellable.is_some())
            .expect("the pane lists it and marks it");

        // THE AGREEMENT. Not "both surfaces mention something odd" — the same
        // file, reached the same way, in the same state. `absolute_path` is the
        // undecoded handle, so this compares bytes, not renderings.
        assert_eq!(
            shown.absolute_path,
            odd_path.canonicalize().expect("canonical"),
            "the pane's row and the file on disk are the same bytes"
        );
        assert!(
            matches!(
                shown.sync,
                crate::browse::EntrySyncStatus::Waiting {
                    reason: Some(PendingReason::Untracked)
                }
            ),
            "the pane must say what the engine says; got {:?}",
            shown.sync
        );

        // 3. The commit path selects the same file, by its real bytes.
        //
        // Two passes: the completeness gate needs one observation to open the
        // settling episode and a second, after the window, to close it. The
        // first must stage nothing — a file whose name is odd is not a file
        // that skips the gate.
        assert!(
            engine.collect_stable_changes(&p).expect("scan").is_empty(),
            "the gate applies to this file like any other"
        );
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        let staged = engine.collect_stable_changes(&p).expect("scan");
        assert!(
            staged.added.iter().any(|rela| rela.as_os_str() == odd_name),
            "the commit walk must stage the real name, not a rendering; got {:?}",
            staged.added
        );

        // A `Stable` verdict retires the gate's episode, so the scan above
        // consumed it and `commit_local`'s own scan would find nothing. One
        // more observation re-opens it. Spelled out rather than folded away
        // because the alternative — deleting the assertion above and trusting
        // the tree check below — would drop the direct evidence that the walk
        // stages the *real* bytes rather than a rendering that happens to work.
        assert!(engine.collect_stable_changes(&p).expect("scan").is_empty());
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        // 4. And it lands. The owner's guess that it "would not make it" was
        //    wrong, so nothing here tries to make it syncable — it already is.
        engine
            .commit_local(&p, SyncSource::Manual, None)
            .expect("a name that is not text is still a file git can carry");
        let repo = engine.open_repo(&p).expect("open");
        let tree = repo
            .head_commit()
            .expect("a commit exists")
            .tree()
            .expect("tree");
        assert!(
            tree.iter()
                .filter_map(std::result::Result::ok)
                .any(|entry| entry.filename() == b"doc-\xffepuap.txt".as_slice()),
            "git stores path bytes; the raw name must be in the tree"
        );

        // 5. Now that it is carried, it is *reported* — which is the whole
        //    story. Before this, a committed file with an unspellable name was
        //    invisible: clean status, nothing pending, nothing on any list, and
        //    the only way to find it was `ls`.
        let problems = engine.problems(&p.id).await.expect("problems");
        assert_eq!(
            problems
                .unspellable
                .iter()
                .map(|n| n.escaped.as_str())
                .collect::<Vec<_>>(),
            vec!["doc-\\xffepuap.txt"],
            "the report must name it byte-exactly, and must not name the ordinary file"
        );
        assert_eq!(
            problems.unspellable[0].display, "doc-\u{FFFD}epuap.txt",
            "and carry the rendering a person reads in a row"
        );
        // Not an error and not a failure: nothing is stuck.
        assert!(problems.error.is_none(), "a carried file is not a fault");
        assert!(problems.parked.is_empty());

        // 6. It stays reported once it is old news. This is the case the owner
        //    actually hit — a file committed long ago, clean in status, that no
        //    walk would ever mention again.
        assert!(
            engine.pending(&p.id).await.expect("pending").is_empty(),
            "the file is committed, so nothing is pending"
        );
        assert_eq!(
            engine
                .problems(&p.id)
                .await
                .expect("problems")
                .unspellable
                .len(),
            1,
            "the index is read, not the status walk, so a clean tree still reports"
        );
    }

    /// Story 41.3: while a rotation is in flight the destination holds a
    /// growing `<name>.mov.partial` beside the segments already closed. The
    /// in-progress one has to be invisible everywhere at once — the pending
    /// list, the index, the commit, the activity feed — because a
    /// half-written file committed once is a corrupt file forever, and
    /// because a segment that showed as "1 file pending" for the length of a
    /// meeting would read as sync being broken.
    ///
    /// The whole mechanism is one tier-0 name rule, so this is also the proof
    /// that the sync crate needs to know nothing else about recording: the
    /// rename at the end is the only event, and the next commit picks the
    /// segment up as an ordinary file.
    #[tokio::test]
    async fn a_segment_still_being_written_is_never_pending_and_never_committed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        // Exactly where recordings land: a session folder nested under the
        // profile's recordings root, so the rule is exercised at depth.
        let session = p
            .local_path
            .join("recordings")
            .join("keeper-rec 2026-08-07 11.02.13");
        std::fs::create_dir_all(&session).expect("session folder");
        std::fs::write(session.join("screen-0000.mov"), b"a segment that closed").expect("closed");
        std::fs::write(session.join("screen-0001.mov.partial"), b"still growing").expect("open");
        engine.upsert_profile(&p).expect("upsert");
        engine.ensure_repo(&p).expect("adopt");

        let closed = "recordings/keeper-rec 2026-08-07 11.02.13/screen-0000.mov";
        assert_eq!(
            engine.pending(&p.id).await.expect("pending"),
            vec![PendingFile {
                path: closed.to_owned(),
                reason: PendingReason::Untracked,
                size_bytes: Some(b"a segment that closed".len() as u64),
            }],
            "only the finished segment is waiting; the one being written is not a file sync has an opinion about"
        );

        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            1,
            "the commit carries the closed segment and nothing else"
        );
        let tracked = {
            let repo = engine.open_repo(&p).expect("open");
            git::repo::tracked_paths(&repo).expect("index")
        };
        assert!(
            !tracked
                .iter()
                .any(|path| path.to_string_lossy().ends_with(".partial")),
            "the index must never hold a segment mid-write, got {tracked:?}"
        );
        let activity = engine.activity(&p.id, 50).await.expect("activity");
        assert!(
            !activity.iter().any(|row| row.path.ends_with(".partial")),
            "the activity feed reports what synced, and a partial never syncs"
        );

        // The rename the sidecar performs the instant `finishWriting` returns:
        // same bytes, same inode, a name sync can finally see.
        std::fs::rename(
            session.join("screen-0001.mov.partial"),
            session.join("screen-0001.mov"),
        )
        .expect("publish the finished segment");
        assert_eq!(
            commit_after_settling(&engine, &platform, &p),
            1,
            "once published it is an ordinary file, committed like any other"
        );
        let tracked = {
            let repo = engine.open_repo(&p).expect("open");
            git::repo::tracked_paths(&repo).expect("index")
        };
        assert!(
            tracked
                .iter()
                .any(|path| path.to_string_lossy().ends_with("screen-0001.mov")),
            "the published segment is tracked, got {tracked:?}"
        );
        assert!(
            !tracked
                .iter()
                .any(|path| path.to_string_lossy().ends_with(".partial")),
            "and its temporary name never entered the repository at all"
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
                    &[db::ActivityEntry {
                        kind: ActivityKind::Conflict,
                        path: copy.to_owned(),
                        size_bytes: Some(11),
                        unit_id: None,
                    }],
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
            size_bytes: Some(4_096),
        };
        assert_eq!(
            serde_json::to_string(&pending).expect("serialize"),
            r#"{"path":"notes/a.md","reason":{"kind":"settling","sinceMs":42},"sizeBytes":4096}"#
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
            unspellable: vec![
                crate::names::UnspellableName::of_bytes(b"doc-\xffepuap.txt")
                    .expect("not valid UTF-8"),
            ],
        };
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains(r#""lastError":"401""#), "got: {json}");
        assert!(json.contains(r#""attempts":3"#), "got: {json}");
        // Both renderings cross the wire: the lossy one is what a row shows,
        // the escaped one is what makes the report actionable. A frontend given
        // only `display` could not tell two reported files apart.
        // `serde_json` writes U+FFFD as the character itself, not as a `\u`
        // escape, so this has to be a real replacement character rather than
        // the six letters that spell one.
        assert!(
            json.contains("\"display\":\"doc-\u{FFFD}epuap.txt\""),
            "got: {json}"
        );
        assert!(
            json.contains(r#""escaped":"doc-\\xffepuap.txt""#),
            "got: {json}"
        );

        let row = ActivityRow {
            ts_ms: 1,
            kind: ActivityKind::Conflict,
            path: "a.md".to_owned(),
            size_bytes: Some(4_096),
            delivery: db::DeliveryState::Abandoned,
            failure: Some("401".to_owned()),
            unit_id: Some(7),
        };
        assert_eq!(
            serde_json::to_string(&row).expect("serialize"),
            r#"{"tsMs":1,"kind":"conflict","path":"a.md","sizeBytes":4096,"delivery":"abandoned","failure":"401","unitId":7}"#
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
                SyncPhase::UploadingLfs,
                Some("40-media/clip.mov"),
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
            assert_eq!(event.phase, SyncPhase::UploadingLfs);
            // EVERY frame, not just the first. The name used to be stamped on a
            // frame published before the transfer began, and the first byte
            // that moved replaced it with `None` — so the surface that shows
            // the file being carried showed it for no observable time at all.
            assert_eq!(
                event.current.as_deref(),
                Some("40-media/clip.mov"),
                "the file being carried must survive its own progress"
            );
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
            3,
            "one entering the commit, one per staged path after the first, one on completion"
        );
        assert_eq!(committing[0].files_total, Some(2));
        assert_eq!(committing[0].files_done, 0);
        assert_eq!(committing[0].fraction(), Some(0.0));
        assert_eq!(
            committing[0].current.as_deref(),
            Some("a.txt"),
            "the detail line names a repository-relative path, never an absolute one"
        );
        // Staging reports the second path as it reaches it (Story 34.8). Before
        // this the middle frame did not exist: the count went straight from 0 to
        // 2 and the detail line never left `a.txt`.
        assert_eq!(committing[1].files_done, 1);
        assert_eq!(committing[1].current.as_deref(), Some("b.txt"));
        assert_eq!(committing[1].fraction(), Some(0.5));
        assert_eq!(committing[2].files_done, 2);
        assert_eq!(committing[2].fraction(), Some(1.0));

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

    #[tokio::test]
    async fn an_identical_change_on_both_sides_makes_no_conflict_copy() {
        // Two devices running the same formatter, or regenerating the same
        // deterministic file, both list the path in their diff against the
        // merge base — and the copy that used to produce was byte-identical to
        // the file beside it. The user could only discover that by opening
        // both. Changing a path is not the same as disagreeing about it.
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

        std::fs::write(p.local_path.join("index.md"), b"root").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("publish the shared root");

        // Same path, same resulting bytes, arrived at independently.
        advance_remote(remote_dir.path(), "index.md", "regenerated");
        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("index.md"), b"regenerated").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let outcome = engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("converge");
        assert!(
            outcome.conflicts.is_empty(),
            "identical content is not a conflict, got {:?}",
            outcome.conflicts
        );
        assert_eq!(
            std::fs::read(p.local_path.join("index.md")).expect("canonical path"),
            b"regenerated",
            "the content both sides agreed on must survive"
        );
        assert!(
            outcome.stale.is_empty(),
            "nothing was given away, so nothing is stale: {:?}",
            outcome.stale
        );
        let strays: Vec<_> = std::fs::read_dir(&p.local_path)
            .expect("read the worktree")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".sync-conflict-"))
            .collect();
        assert!(strays.is_empty(), "no copy may be left behind: {strays:?}");
    }

    #[tokio::test]
    async fn a_regenerable_path_converges_without_a_copy() {
        // A generated listing that two devices rewrote for two different
        // reasons genuinely differs — and the local revision is still worth
        // nothing, because the tool that wrote it rebuilds the right answer
        // from inputs that are themselves synced. Only a path the user declared
        // regenerable gets this; keeper never decides an edit is disposable.
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
        p.regenerable = vec!["index.md".to_owned()];
        engine.upsert_profile(&p).expect("upsert");

        std::fs::write(p.local_path.join("index.md"), b"root").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("publish the shared root");

        advance_remote(remote_dir.path(), "index.md", "theirs");
        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("index.md"), b"ours").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        let outcome = engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("converge");
        assert!(
            outcome.conflicts.is_empty(),
            "a declared-regenerable path must converge without a copy, got {:?}",
            outcome.conflicts
        );
        assert_eq!(
            outcome.stale,
            vec!["index.md".to_owned()],
            "silently is not the same as invisibly: the path is now the other \
             device's answer and the run has to say so"
        );
        assert_eq!(
            std::fs::read(p.local_path.join("index.md")).expect("canonical path"),
            b"theirs",
            "the remote still keeps the canonical path"
        );
    }

    /// The whole trailer block on the profile worktree's HEAD commit.
    fn head_provenance(p: &SyncProfile) -> Provenance {
        let repo = gix::open(&p.local_path).expect("open the worktree");
        let message = repo
            .head_commit()
            .expect("a commit")
            .message_raw_sloppy()
            .to_string();
        Provenance::parse(&message).expect("an engine commit carries trailers")
    }

    /// The `Keeper-Source` trailer on the profile worktree's HEAD commit.
    fn head_source(p: &SyncProfile) -> SyncSource {
        head_provenance(p).source
    }

    /// AD-34-12. `Keeper-Source` answers "what caused this commit to exist",
    /// and it said `watch` for every commit keeper had ever made: the source
    /// reached one `tracing::info!` in `sync_once` and both `Provenance::new`
    /// sites hard-coded `SyncSource::Watch`. `git log` on a folder a person had
    /// just synced by hand blamed a watcher that had not run.
    #[tokio::test]
    async fn a_commit_records_which_kind_of_request_caused_it() {
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

        // The scheduled path, which is the only genuinely watcher-initiated one.
        std::fs::write(p.local_path.join("watched.txt"), b"one").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        assert_eq!(head_source(&p), SyncSource::Watch);

        // A person clicking Sync now. Two passes because the stability gate
        // needs two identical observations a settle window apart.
        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("asked.txt"), b"two").expect("write");
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("the pass that opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("the pass that commits");
        assert_eq!(
            head_source(&p),
            SyncSource::Manual,
            "a sync a person asked for must not be recorded as the watcher's"
        );

        // And `keeper-syncd`, which is neither.
        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("cron.txt"), b"three").expect("write");
        engine
            .sync_once(&p.id, SyncSource::Cli)
            .await
            .expect("the pass that opens the episode");
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        engine
            .sync_once(&p.id, SyncSource::Cli)
            .await
            .expect("the pass that commits");
        assert_eq!(head_source(&p), SyncSource::Cli);
    }

    /// The fourth arm of `SyncSource`, which no caller passes and which could
    /// therefore never appear in a trailer. A worktree lane IS an agent's lane
    /// (AD-50), so an unattended pass there is a bot publishing its own work.
    #[test]
    fn an_unattended_pass_on_an_agent_lane_is_recorded_as_a_bot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut p = profile(dir.path());
        assert_eq!(
            Engine::commit_source(&p, SyncSource::Watch),
            SyncSource::Watch
        );

        p.lane = SyncLane::Worktree;
        p.direction = SyncDirection::PushOnly;
        assert_eq!(
            Engine::commit_source(&p, SyncSource::Watch),
            SyncSource::Bot
        );
        // Asking is asking. A human who clicks Sync now on a lane really did
        // cause that commit, and the trailer records the cause.
        assert_eq!(
            Engine::commit_source(&p, SyncSource::Manual),
            SyncSource::Manual
        );
        assert_eq!(Engine::commit_source(&p, SyncSource::Cli), SyncSource::Cli);
    }

    /// Story 34.5(a). `db::set_device_label` had existed with no caller since
    /// Story 23.4, so the name minted from `hostname` at first open was the name
    /// every commit carried forever. The rename has to reach the NEXT commit —
    /// which means the engine's in-memory copy, not just the row — and it must
    /// not move the id, which is what the author address is derived from and
    /// what tells two machines with the same name apart in a shared history.
    #[test]
    fn renaming_the_device_reaches_the_next_commit_and_leaves_the_id_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let minted = engine.device();

        std::fs::write(p.local_path.join("before.txt"), b"one").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        let first = head_provenance(&p);
        assert_eq!(first.device_label, minted.label);

        engine.set_device_label("  Studio Mac  ").expect("rename");
        assert_eq!(
            engine.device().id,
            minted.id,
            "a rename must not re-mint the identity"
        );

        platform.advance_ms(1_000);
        std::fs::write(p.local_path.join("after.txt"), b"two").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        let second = head_provenance(&p);
        assert_eq!(second.device_label, "Studio Mac");
        assert_eq!(second.device_id, minted.id);

        // History is not rewritten to match a new name: the earlier commit still
        // says what this machine was called when it made it.
        assert_eq!(first.device_label, minted.label);
        assert_eq!(first.device_id, minted.id);

        // An empty rename is refused, and refusing it leaves the live copy as it
        // was rather than blanking the trailer on every commit from here on.
        assert!(engine.set_device_label("   ").is_err());
        assert_eq!(engine.device().label, "Studio Mac");
    }

    /// `last_sync_ms` was stamped inside `do_push` and nowhere else, so a
    /// pull-only folder could sync correctly for a week and still report that
    /// it had never synced — indistinguishable, in the UI, from a folder sync
    /// has never once succeeded on.
    #[tokio::test]
    async fn a_pass_with_nothing_to_push_still_records_that_it_succeeded() {
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
        assert_eq!(
            engine.status(&p.id).expect("status").last_sync_ms,
            None,
            "nothing has synced yet"
        );

        // A pass that really does publish something, so the remote has a branch
        // for the later passes to compare against.
        std::fs::write(p.local_path.join("notes.md"), b"root").expect("write");
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("publish to the local bare remote");
        assert_eq!(
            engine.status(&p.id).expect("status").last_sync_ms,
            Some(platform.now_ms())
        );

        // Nothing has changed since. The pass still ran every leg and still
        // succeeded, so it still counts.
        platform.advance_ms(60_000);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("a pass with nothing to do");
        assert_eq!(
            engine.status(&p.id).expect("status").last_sync_ms,
            Some(platform.now_ms()),
            "a sync that found nothing to do still succeeded"
        );

        // And the case that never recorded anything at all: a folder that only
        // ever pulls reaches no push leg to be stamped by.
        p.direction = SyncDirection::PullOnly;
        engine.upsert_profile(&p).expect("upsert");
        platform.advance_ms(60_000);
        engine
            .sync_once(&p.id, SyncSource::Manual)
            .await
            .expect("a pull-only pass");
        assert_eq!(
            engine.status(&p.id).expect("status").last_sync_ms,
            Some(platform.now_ms()),
            "a pull-only folder must be able to report that it synced"
        );
    }

    // -----------------------------------------------------------------------
    // Story 34.9 — the watcher, the honest wait, and tier 0 in the list
    // -----------------------------------------------------------------------

    fn watch_is_live(engine: &Engine, id: &str) -> bool {
        matches!(
            Engine::lock(&engine.watchers).get(id),
            Some(ProfileWatch::Live { .. })
        )
    }

    fn watch_failure(engine: &Engine, id: &str) -> Option<String> {
        match Engine::lock(&engine.watchers).get(id) {
            Some(ProfileWatch::Failed { reason, .. }) => Some(reason.clone()),
            _ => None,
        }
    }

    fn armed_count(engine: &Engine) -> usize {
        Engine::lock(&engine.watchers).len()
    }

    /// A `FolderWatcher` owns two threads and an inotify instance, so the
    /// lifecycle is the part that can leak. Deliberately proves nothing about
    /// event *delivery*: this test never waits on the filesystem.
    #[test]
    fn a_watcher_is_armed_for_every_enabled_profile_and_outlives_none_of_them() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        engine.ensure_watcher(&p);
        assert!(
            watch_is_live(&engine, &p.id),
            "an enabled profile is watched"
        );

        // Idempotent: this runs once a second, and must not build a second
        // instance — nor a second pair of threads — per tick.
        engine.ensure_watcher(&p);
        engine.ensure_watcher(&p);
        assert_eq!(armed_count(&engine), 1);

        // Paused. `tick_profile` never runs for a disabled profile, so if the
        // reconciliation lived there the watcher would survive the pause.
        p.enabled = false;
        engine.retain_watchers(std::slice::from_ref(&p));
        assert_eq!(armed_count(&engine), 0, "a paused profile keeps no watcher");

        // Resumed, and still exactly one: a resume must not leak a thread per
        // cycle.
        p.enabled = true;
        engine.ensure_watcher(&p);
        engine.retain_watchers(std::slice::from_ref(&p));
        assert_eq!(armed_count(&engine), 1);

        // A profile the engine no longer knows about keeps nothing at all.
        engine.retain_watchers(&[]);
        assert_eq!(armed_count(&engine), 0);

        // Removal takes the watcher in the same call, not a tick later.
        engine.ensure_watcher(&p);
        engine.remove_profile(&p.id).expect("remove");
        assert_eq!(armed_count(&engine), 0);

        // A watcher armed for a root the profile no longer uses is replaced,
        // not kept beside the right one.
        engine.upsert_profile(&p).expect("re-add");
        engine.ensure_watcher(&p);
        let moved = dir.path().join("moved");
        std::fs::create_dir_all(&moved).expect("new root");
        p.local_path = moved;
        engine.ensure_watcher(&p);
        assert_eq!(armed_count(&engine), 1);
        assert!(watch_is_live(&engine, &p.id));

        // And the supervisor's own teardown leaves none armed, because the
        // supervisor is their only consumer.
        engine.stop_all_watchers();
        assert_eq!(armed_count(&engine), 0);
    }

    /// A watcher that cannot arm degrades to the paced scan, and says so where
    /// the user can see it — never silently (AD-34-11).
    #[test]
    fn a_watcher_that_cannot_arm_says_so_and_keeps_saying_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        // `profile` does not create the folder, and a recursive watch over a
        // path that does not exist fails on both backends we ship: inotify with
        // ENOENT, FSEvents with its own path-not-found.
        let p = profile(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        engine.ensure_watcher(&p);
        assert!(
            watch_failure(&engine, &p.id).is_some(),
            "the failure is remembered, so it is not retried at the tick rate"
        );
        let warning = engine
            .status(&p.id)
            .expect("status")
            .warning
            .expect("a degraded profile must say so where the user can see it");
        assert!(warning.starts_with(WATCH_DEGRADED_PREFIX), "got {warning}");
        assert!(
            warning.contains("to sync"),
            "the line has to say what the user loses by it: {warning}"
        );

        // Any successful unit clears the sticky text. The next tick must put it
        // back — that is the difference between degrading loudly and degrading
        // silently.
        engine.clear_warning(&p.id);
        engine.ensure_watcher(&p);
        assert!(
            engine.status(&p.id).expect("status").warning.is_some(),
            "a degradation that stops announcing itself is a silent downgrade"
        );

        // ...and re-asserting the text is not a fresh event. One notification
        // per onset, however many ticks the condition lasts.
        let notifications = platform
            .notifications
            .lock()
            .map(|n| n.len())
            .unwrap_or_default();
        assert_eq!(notifications, 1, "re-stating a state must not re-notify");

        // The folder appearing does not license an immediate retry: the window
        // exists so a real failure is not hammered.
        std::fs::create_dir_all(&p.local_path).expect("work dir");
        engine.ensure_watcher(&p);
        assert!(
            watch_failure(&engine, &p.id).is_some(),
            "still inside the re-arm window"
        );

        // Once it elapses the watcher arms, and retires its own warning without
        // touching anything else on display.
        platform.advance_ms(WATCH_REARM_INTERVAL_MS);
        engine.ensure_watcher(&p);
        assert!(watch_is_live(&engine, &p.id));
        assert!(
            engine.status(&p.id).expect("status").warning.is_none(),
            "a working watcher retires the warning it raised"
        );
    }

    /// `clear_watch_warning` must not become a general-purpose warning eraser.
    #[test]
    fn arming_a_watcher_does_not_retire_someone_elses_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        engine.warn(&p.id, &p.name, "the drive is nearly full".to_owned());

        engine.ensure_watcher(&p);
        assert_eq!(
            engine.status(&p.id).expect("status").warning.as_deref(),
            Some("the drive is nearly full"),
            "an unrelated problem must survive a watcher arming"
        );
    }

    /// The latency fix, stated as the two reasons that did not exist before.
    ///
    /// A settling path needs a *second* observation to clear, and observations
    /// happen only inside a walk. Before AD-34-11 the only thing that produced
    /// one was the paced poll, so a file that landed took two poll intervals —
    /// 30 s at the default — rather than its 5 s settle window.
    #[test]
    fn a_held_file_reopens_the_walk_on_its_own_window_not_the_poll_interval() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.poll_interval_ms = 15_000;

        // First sight walks at once; the tick after does not.
        assert!(engine.scan_due(&p), "a new profile is walked immediately");
        assert!(
            !engine.scan_due(&p),
            "the next tick is inside the paced window"
        );

        // One path mid-window. Its deadline, not the poll interval, is what the
        // supervisor now waits for.
        let now = platform.now_ms();
        let held = p.local_path.join("held.bin");
        let mut gate = StabilityGate::for_profile(&p).expect("gate");
        gate.observe(
            &held,
            FileSample {
                size: 128,
                mtime_ns: i128::from(now) * 1_000_000,
                ctime_ns: i128::from(now) * 1_000_000,
                inode: 11,
            },
            now,
            false,
        );
        Engine::lock(&engine.gates).insert(p.id.clone(), gate);

        assert!(
            !engine.scan_due(&p),
            "a file still inside its window is not yet a reason to walk"
        );
        platform.advance_ms(p.effective_settle_ms() as i64);
        assert!(
            engine.scan_due(&p),
            "the second observation a settling file needs must not wait out the poll interval"
        );

        // The other new reason: an event the watcher delivered. It survives a
        // tick that drained journal work instead of walking, because only the
        // walk itself spends it.
        Engine::lock(&engine.gates).remove(&p.id);
        platform.advance_ms(p.poll_interval_ms as i64);
        assert!(engine.scan_due(&p), "the paced window has come round");
        assert!(!engine.scan_due(&p), "and closed again");

        engine.note_watch_wake(&p.id);
        assert!(
            engine.scan_due(&p),
            "a delivered event opens the walk at once"
        );
        assert!(
            engine.scan_due(&p),
            "and stays open until a walk answers it — a tick that drained work \
             instead must not swallow the change"
        );
        engine.clear_watch_wake(&p.id);
        assert!(!engine.scan_due(&p), "the walk spent it");
    }

    /// The wiring, without waiting on a real event queue: the event is placed on
    /// the same channel a watcher would place it on, which is how `watch.rs`'s
    /// own tests keep timing out of the picture.
    #[test]
    fn a_delivered_close_write_reaches_the_gate_and_shortens_the_wait() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                // No sweep: this test owns no timers.
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        let path = p.local_path.join("saved.txt");
        std::fs::write(&path, b"done").expect("write");
        tx.send(WatchEvent {
            path: path.clone(),
            close_write: true,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");

        assert!(
            engine.watch_wake_pending(&p.id),
            "a delivered event is a reason to walk now"
        );

        // And the close-write took the path down the 1 s route — the branch that
        // has existed since Story 26.3 and was unreachable until the watcher was
        // wired up, because nothing outside `watch.rs` ever built one.
        let now = platform.now_ms();
        let mut gates = Engine::lock(&engine.gates);
        let gate = gates.get_mut(&p.id).expect("folding seeded the gate");
        assert!(matches!(
            gate.is_stable(&path, now),
            StabilityVerdict::Settling { .. }
        ));
        assert_eq!(
            gate.next_stable_ms(now),
            Some(now + i64::try_from(crate::profile::CLOSE_WRITE_SETTLE_MS).expect("fits")),
            "one second, not the full settle window"
        );
    }

    /// A watched folder pays for the directory walk once, then leaves new files
    /// to the watcher until the sweep falls due.
    ///
    /// The directory walk is the expensive half of a status and it is not
    /// proportional to what changed: measured on the field folder,
    /// `find . -type f` over 157 490 files takes 996 s, and the Pending poll
    /// was paying that every five seconds for rows the stability gate already
    /// held.
    #[test]
    fn a_watched_folder_sweeps_for_untracked_once_and_then_stops_walking_the_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let (_tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            _tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        // The first poll of a run always sweeps: nothing was watching while the
        // process was down.
        assert_eq!(
            engine.poll_walk_policy(&p.id),
            git::repo::WalkPolicy::full(),
            "the first poll after a start has to look for itself"
        );
        assert_eq!(
            engine.poll_walk_policy(&p.id),
            git::repo::WalkPolicy::tracked_only(),
            "and the next one must not walk the tree again five seconds later"
        );

        // Age the record past the interval: the sweep comes back, so a file the
        // watcher never announced is still found within the quarter hour.
        Engine::lock(&engine.untracked_sweep).insert(
            p.id.clone(),
            Instant::now()
                .checked_sub(UNTRACKED_SWEEP_INTERVAL + Duration::from_secs(1))
                .expect("the test clock is not at the epoch"),
        );
        assert_eq!(
            engine.poll_walk_policy(&p.id),
            git::repo::WalkPolicy::full(),
            "a due sweep is what catches an event the backend dropped"
        );
    }

    /// With no live watcher there is no second source for a new file, so every
    /// poll pays.
    ///
    /// This is the branch that keeps the cheap path honest: the saving is
    /// available precisely because something else is watching, and a folder on
    /// a filesystem that cannot notify (`ProfileWatch::Failed`, or a watcher
    /// that has not armed yet) gets the old behaviour rather than a quiet gap.
    #[test]
    fn an_unwatched_folder_keeps_sweeping_on_every_poll() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        for pass in 1..=3 {
            assert_eq!(
                engine.poll_walk_policy(&p.id),
                git::repo::WalkPolicy::full(),
                "pass {pass}: an unwatched folder has nothing else to trust"
            );
        }
    }

    /// The commit leg spends its directory scan on the watcher's word.
    ///
    /// It is the leg that publishes untracked files, so it may not miss one —
    /// and the watcher is what makes skipping safe. The scan is what a pass
    /// costs: `elapsed_ms=9299355` on the field folder, two and a half hours,
    /// to report twelve modified files.
    #[test]
    fn the_commit_leg_scans_when_the_watcher_says_something_happened() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx,
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        assert_eq!(
            engine.commit_walk_policy(&p),
            git::repo::WalkPolicy::full(),
            "the first pass after a start has to look for itself"
        );
        assert_eq!(
            engine.commit_walk_policy(&p),
            git::repo::WalkPolicy::tracked_only(),
            "nothing has happened since, so the scan buys nothing"
        );

        // An ordinary wake is not enough, and must not be: a tracked file
        // being written is what the index-only walk answers in milliseconds.
        engine.note_watch_wake(&p.id);
        assert_eq!(
            engine.commit_walk_policy(&p),
            git::repo::WalkPolicy::tracked_only(),
            "a write to a tracked file does not need the whole tree walked"
        );

        // A path the index does not carry is the one thing only a scan finds.
        Engine::lock(&engine.untracked_appeared).insert(p.id.clone());
        assert_eq!(
            engine.commit_walk_policy(&p),
            git::repo::WalkPolicy::full(),
            "a path git has never seen is what the scan exists for"
        );
        assert_eq!(
            engine.commit_walk_policy(&p),
            git::repo::WalkPolicy::tracked_only(),
            "and the scan that answered it consumes the signal"
        );
    }

    /// The signal is set by a path the index does not carry, and only by that.
    #[test]
    fn a_write_to_a_tracked_file_does_not_ask_for_a_directory_scan() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        std::fs::write(p.local_path.join("tracked.txt"), b"one").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        commit_after_settling(&engine, &platform, &p);

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        // A tracked path: the index carries it, so no scan is owed.
        tx.send(WatchEvent {
            path: p.local_path.join("tracked.txt"),
            close_write: true,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            !Engine::lock(&engine.untracked_appeared).contains(&p.id),
            "a write to a file git already carries needs no directory scan"
        );

        // A path the index has never seen: only a scan finds it.
        let fresh = p.local_path.join("appeared.txt");
        std::fs::write(&fresh, b"new").expect("write");
        tx.send(WatchEvent {
            path: fresh,
            close_write: true,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            Engine::lock(&engine.untracked_appeared).contains(&p.id),
            "a path git has never seen is exactly what the scan exists for"
        );
    }

    /// The tap is an observer: it sees what the drain saw, and its absence —
    /// or its subscriber's disappearance — cannot turn a drain into a failure.
    #[test]
    fn a_watch_event_reaches_a_tap_and_a_dropped_tap_cannot_break_the_drain() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        let mut tap = engine.watch_tap();
        let path = p.local_path.join("tapped.md");
        std::fs::write(&path, b"note").expect("write");
        tx.send(WatchEvent {
            path: path.clone(),
            // Not a close-write: the tap carries every path, not only the ones
            // the gate fast-tracks.
            close_write: false,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");

        assert_eq!(
            tap.try_recv().expect("the tap saw what the drain saw"),
            (p.id.clone(), path.clone())
        );

        // The subscriber goes away mid-flight, which is the ordinary case when
        // a window closes. The next drain must be indistinguishable from one on
        // an engine nobody ever tapped.
        drop(tap);
        tx.send(WatchEvent {
            path,
            close_write: false,
        })
        .expect("the supervisor is listening");
        engine
            .fold_watch_events(&p)
            .expect("a send with no receivers is not a sync failure");
        assert!(
            engine.watch_wake_pending(&p.id),
            "and the event still counted as a reason to walk"
        );
    }

    /// Story 34.9 / AD-45: tier 0 decides the **wake**, not only the walk.
    ///
    /// The watcher is armed `RecursiveMode::Recursive` over the profile root,
    /// so a build reports every byte it writes into `node_modules/`. Until this
    /// filter existed, each of those events set the sticky wake, `scan_due` was
    /// true on every 1 Hz tick, and the engine re-stat-ed the whole tree once a
    /// second — driven by the five directory names this very story added to
    /// `BUILTIN_EXCLUDES` *because* they are noise. Tier 0 was applied only in
    /// `is_stable` and in `pending`, downstream of the walk, where it cannot
    /// suppress the walk it was added to prevent.
    ///
    /// The assertion is on [`Engine::watch_wake_pending`] itself, and
    /// deliberately not on a downstream effect: the wake is the thing that arms
    /// the walk, and the walk is the whole cost.
    #[test]
    fn an_excluded_path_is_never_a_reason_to_walk_and_an_ordinary_one_still_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                // No sweep: this test owns no timers, and a rescan event would
                // be a wake this test did not ask for.
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        // One row per tier-0 directory Story 34.9 added, at the depths a real
        // build writes them, plus `.git/**` — which is in the same corpus, so
        // asking it here is also what stops the engine's own commits, index
        // writes, ref updates and LFS object writes from each costing a
        // fruitless walk.
        for relative in [
            "node_modules/react/index.js",
            ".venv/lib/python3.12/site-packages/pip/__init__.py",
            "src/__pycache__/mod.cpython-312.pyc",
            ".next/cache/webpack/client-development/0.pack",
            ".cache/yarn/v6/npm-left-pad/node_modules/left-pad/index.js",
            ".git/index",
            ".git/refs/heads/main",
            ".git/lfs/objects/ab/cd/abcdef0123456789",
            "notes/.keeper/index.json",
            "Downloads/film.mkv.crdownload",
        ] {
            tx.send(WatchEvent {
                path: p.local_path.join(relative),
                close_write: false,
            })
            .expect("the supervisor is listening");
            engine.fold_watch_events(&p).expect("fold");
            assert!(
                !engine.watch_wake_pending(&p.id),
                "{relative} is excluded, so no walk may be armed for it"
            );
        }

        // A close-write is a fast path through the gate, never a way around
        // tier 0: an excluded path that was closed for writing is still a path
        // nothing will ever commit.
        tx.send(WatchEvent {
            path: p.local_path.join("node_modules/.package-lock.json"),
            close_write: true,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            !engine.watch_wake_pending(&p.id),
            "a close-write does not exempt an excluded path"
        );

        // The control, through the same channel and the same drain. Without
        // this the filter could be passing by refusing everything.
        tx.send(WatchEvent {
            path: p.local_path.join("src/main.rs"),
            close_write: false,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            engine.watch_wake_pending(&p.id),
            "an ordinary change is still a reason to walk at once"
        );
    }

    /// The two boundaries of the `.git` decision, pinned so neither drifts.
    ///
    /// Everything **under** `.git/` is excluded, because tier 0 already carries
    /// `.git/**` and `**/.git/**` and there is no input down there for which
    /// "walk the worktree now" is the right answer — it is engine state by
    /// definition, never user content, never committable.
    ///
    /// The bare `.git` **directory node itself** is not excluded, because tier
    /// 0's `.git` rules are subtree rules and deliberately so (`.gitignore` and
    /// `.gitattributes` are sibling dotfiles that must stay visible). That is
    /// the right answer anyway: `.git` appearing or vanishing is a
    /// repository-level event, and looking then is exactly what adoption and
    /// re-adoption need. It costs the one walk it always did.
    ///
    /// The profile root is the third case and the load-bearing one: a queue
    /// overflow and the periodic sweep are both expressed as an event on the
    /// root, so if the filter ever swallowed those the watcher would lose the
    /// only coverage it has for its own dropped events.
    #[test]
    fn the_git_directorys_contents_are_filtered_but_the_root_and_the_git_node_are_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        tx.send(WatchEvent {
            path: p.local_path.join(".git/objects/pack/pack-abc.idx"),
            close_write: true,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            !engine.watch_wake_pending(&p.id),
            "our own object writes must not each buy a walk of the worktree"
        );

        // The rescan escalation: `fold_batch` expresses a dropped event queue
        // as an event on the root, and the periodic sweep uses the same shape.
        tx.send(WatchEvent {
            path: p.local_path.clone(),
            close_write: false,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            engine.watch_wake_pending(&p.id),
            "an overflow rescan is the one event that must never be filtered"
        );

        engine.clear_watch_wake(&p.id);
        tx.send(WatchEvent {
            path: p.local_path.join(".git"),
            close_write: false,
        })
        .expect("the supervisor is listening");
        engine.fold_watch_events(&p).expect("fold");
        assert!(
            engine.watch_wake_pending(&p.id),
            "a repository appearing or vanishing is worth one look"
        );
    }

    /// The reported pathology, at the cadence it actually happens at.
    ///
    /// Ten minutes of a build writing into the regenerable trees, one burst per
    /// 1 Hz supervisor tick. The loop is the shape of
    /// [`Engine::tick_profile`]: fold what the watcher delivered, ask
    /// [`Engine::scan_due`], and let the walk spend the wake. The only walks
    /// that may happen are the paced ones — 600 s at one every 15 s, plus the
    /// walk a profile gets on first sight — so the count is exact rather than a
    /// bound. Before the filter this counted 600: a full `git status` re-stat
    /// of the tree every second for as long as the build ran.
    #[test]
    fn ten_minutes_of_excluded_write_bursts_cost_not_one_walk_beyond_the_paced_ones() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.poll_interval_ms = 15_000;
        engine.upsert_profile(&p).expect("upsert");

        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        let watcher = FolderWatcher::start(
            &p.local_path,
            WatchConfig {
                debounce_ms: 50,
                rescan_interval_ms: 0,
                force_poll: false,
            },
            tx.clone(),
        )
        .expect("a watcher arms over a real directory");
        Engine::lock(&engine.watchers).insert(p.id.clone(), ProfileWatch::Live { watcher, events });

        const TICKS: i64 = 600;
        let mut walks = 0_usize;
        for tick in 0..TICKS {
            for name in [
                "node_modules/.package-lock.json",
                ".venv/lib/python3.12/site-packages/mod.py",
                "src/__pycache__/build.cpython-312.pyc",
                ".next/cache/webpack/0.pack",
                ".cache/turbo/blob",
            ] {
                tx.send(WatchEvent {
                    path: p.local_path.join(name),
                    // Some of them close, the way a compiler's output does.
                    close_write: tick % 3 == 0,
                })
                .expect("the supervisor is listening");
            }
            engine.fold_watch_events(&p).expect("fold");
            if engine.scan_due(&p) {
                walks += 1;
                engine.clear_watch_wake(&p.id);
            }
            platform.advance_ms(TICK_MS as i64);
        }

        assert_eq!(
            walks, 40,
            "the build must not buy a single walk: 600 s of ticks at one paced \
             walk every 15 s is 40, and every extra one is a full re-stat of \
             the tree that nothing could ever have committed"
        );
    }

    /// AD-34-10: the count the walk computes has to reach the line the user
    /// reads, not a `debug!` nobody sees.
    #[test]
    fn a_walk_that_holds_files_says_how_many_instead_of_up_to_date() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = adoptable(dir.path());
        // Content, so `open_repo` adopts the folder instead of taking the clone
        // path to an unreachable remote. The seed ignores itself.
        std::fs::write(p.local_path.join(".gitignore"), b".gitignore\n").expect("seed");
        engine.upsert_profile(&p).expect("upsert");

        assert!(engine.collect_stable_changes(&p).expect("scan").is_empty());
        assert_eq!(
            crate::progress::status_line(&engine.status(&p.id).expect("status")),
            format!("{} — up to date", p.name),
            "a genuinely clean folder is up to date"
        );

        // Two files inside their settle window. Neither has a journal row, so
        // `pending` is zero — which is exactly how "up to date" came to be
        // printed over a folder that was still being written.
        std::fs::write(p.local_path.join("a.bin"), b"still writing").expect("write");
        std::fs::write(p.local_path.join("b.bin"), b"still writing").expect("write");
        assert!(engine.collect_stable_changes(&p).expect("scan").is_empty());
        let snapshot = engine.status(&p.id).expect("status");
        assert_eq!(snapshot.pending, 0, "nothing is queued yet");
        assert_eq!(snapshot.settling, 2);
        assert_eq!(
            crate::progress::status_line(&snapshot),
            format!("{} — 2 waiting for writes to stop", p.name)
        );

        // And the claim is retired by the walk that finds them settled, not left
        // standing until something else happens to refresh it.
        platform.advance_ms(p.effective_settle_ms() as i64 + 1);
        assert!(!engine.collect_stable_changes(&p).expect("scan").is_empty());
        assert_eq!(engine.status(&p.id).expect("status").settling, 0);
    }

    /// AD-34-15: an excluded path is invisible everywhere, and the Pending list
    /// was the one place that broke `crate::exclude`'s own contract.
    #[tokio::test]
    async fn an_excluded_file_never_appears_in_the_pending_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        // Deliberately no `.gitignore`: git has been told about none of these,
        // so tier 0 is the only thing that can keep them out of the list.
        std::fs::write(p.local_path.join("notes.md"), b"real").expect("write");
        std::fs::write(p.local_path.join(".DS_Store"), b"finder").expect("write");
        std::fs::write(p.local_path.join("movie.mkv.part"), b"half").expect("write");
        let dep = p.local_path.join("node_modules/left-pad");
        std::fs::create_dir_all(&dep).expect("dependency tree");
        std::fs::write(dep.join("index.js"), b"module").expect("write");
        engine.upsert_profile(&p).expect("upsert");
        // Adopts the folder, so git can classify it, and opens a quiescence
        // episode for everything tier 0 did not reject.
        engine.collect_stable_changes(&p).expect("adopt and scan");

        let pending = engine.pending(&p.id).await.expect("pending");
        let paths: Vec<&str> = pending.iter().map(|file| file.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["notes.md"],
            "only the user's own file is waiting; got {paths:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Story 41.5 — committed at close, pushed on policy
    // -----------------------------------------------------------------------

    /// A folder that holds recordings, bound to a remote a test may reach.
    ///
    /// `remote` is a path rather than a URL because the only remote a test is
    /// allowed to touch is a bare repository on disk — and because the same
    /// path, not yet initialized, is exactly what an unreachable remote looks
    /// like.
    fn recordings_profile(dir: &Path, remote: &Path, push: PushPolicy) -> SyncProfile {
        let mut p = adoptable(dir);
        p.remote_url = remote.to_string_lossy().into_owned();
        p.recordings = Some(crate::profile::RecordingsConfig {
            push,
            ..crate::profile::RecordingsConfig::default()
        });
        std::fs::create_dir_all(p.recordings_root().expect("a recordings root"))
            .expect("create the recordings root");
        p
    }

    /// Close one segment: write it and assert it finished, as the sink does.
    ///
    /// Committing is deliberately left to the caller — which of the two things
    /// commits a segment (the supervisor's pass, or the push itself) is what
    /// half of these tests are about.
    fn close_segment(engine: &Engine, p: &SyncProfile, name: &str, bytes: usize) -> PathBuf {
        let path = p.recordings_root().expect("a recordings root").join(name);
        // Filled from its own name, so two segments are two LFS objects. Equal
        // content would be one oid, one journal unit after `enqueue_unique`
        // dedups them, and a count that silently measured something else.
        let content: Vec<u8> = name.bytes().cycle().take(bytes).collect();
        std::fs::write(&path, content).expect("write a segment");
        assert!(
            engine.note_finished_path(&p.id, &path).expect("assert"),
            "a closed segment inside the recordings root is assertable (Story 41.4)"
        );
        path
    }

    /// Push the index's mtime past the worktree's, so a just-committed LFS
    /// entry is not *racily clean* on the next pass.
    ///
    /// Everything in a test happens inside one wall-clock second, which is
    /// exactly git's racily-clean condition: the entry is re-read regardless of
    /// its stat, and re-reading an LFS entry goes through `filter.lfs.clean` —
    /// which under `cargo test` is the libtest harness, answering a filter
    /// request with its own argument-parser error. In production the index is
    /// always written after the file was last touched, so this restores the
    /// ordinary state rather than arranging a special one. A second of sleeping
    /// per rotation would say the same thing far more slowly.
    fn unrace_the_index(p: &SyncProfile) {
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(p.local_path.join(".git").join("index"))
            .and_then(|index| index.set_modified(later))
            .expect("age the index past the worktree");
    }

    /// Has this branch reached the remote?
    fn published(remote: &Path, branch: &str) -> bool {
        gix::open(remote)
            .expect("open the remote")
            .find_reference(&format!("refs/heads/{branch}"))
            .is_ok()
    }

    /// Move the injected clock forward to the next `hour:minute` LOCAL.
    ///
    /// Forward only, because the platform clock only advances — which is also
    /// what makes "the session started at noon and ended at 23:00" one
    /// continuous session rather than two fixtures.
    fn advance_to_local(platform: &TestPlatform, hour: i64, minute: i64) {
        let target = (hour * 60 + minute) * 60_000;
        let local = platform.now_ms() + i64::from(platform.utc_offset_minutes()) * 60_000;
        platform.advance_ms((target - local.rem_euclid(86_400_000)).rem_euclid(86_400_000));
    }

    /// FR-137: the working tree must not change under a running recorder, so
    /// the session's LFS rule is written once at the start and no rotation
    /// rewrites it.
    #[test]
    fn the_session_lfs_rule_is_written_once_and_no_segment_commit_rewrites_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = recordings_profile(dir.path(), dir.path(), PushPolicy::SessionEnd);
        // Low enough that a segment is genuinely an LFS candidate: without
        // that, the commit path would consult `ensure_attributes` with no
        // patterns at all and this test would prove nothing.
        p.lfs_threshold_bytes = 1024;
        engine.upsert_profile(&p).expect("upsert");

        // Session start.
        assert!(
            engine.ensure_lfs_rule(&p.id, "mp4").expect("rule"),
            "the first call is the one write this session gets"
        );
        let attributes = p.local_path.join(".gitattributes");
        let written = std::fs::read(&attributes).expect("read .gitattributes");
        assert!(
            String::from_utf8_lossy(&written).contains("*.mp4 filter=lfs"),
            "the rule has to be the one git reads, got: {}",
            String::from_utf8_lossy(&written)
        );

        // Idempotent to the byte, including the spelling with the dot on it.
        assert!(!engine.ensure_lfs_rule(&p.id, "mp4").expect("rule"));
        assert!(!engine.ensure_lfs_rule(&p.id, ".mp4").expect("rule"));
        assert_eq!(
            std::fs::read(&attributes).expect("read"),
            written,
            "a second call must not touch the file at all"
        );
        assert_eq!(engine.counters(&p.id).attribute_writes, 1);

        // The rule itself is committed, once, before anything is recorded.
        assert_eq!(commit_after_settling(&engine, &platform, &p), 1);

        // Now a segment closes. Its extension is the one the rule covers and it
        // is over the threshold, so this is precisely the commit that WOULD
        // have written `.gitattributes` had the session not already done it.
        let segment = close_segment(&engine, &p, "clip.mp4", 4_096);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit the segment"),
            1,
            "the segment, and nothing else"
        );
        assert_eq!(
            engine.counters(&p.id).attribute_writes,
            1,
            "a rotation must not rewrite the working tree"
        );
        assert_eq!(
            std::fs::read(&attributes).expect("read"),
            written,
            "byte-identical: the rule was already there"
        );
        assert!(segment.exists());
        assert_eq!(
            engine.counters(&p.id).commits,
            2,
            "one commit for the rule at session start, one for the segment"
        );
    }

    /// Every refusal `ensure_lfs_rule` can make is a log line and `Ok(false)`,
    /// because a recorder has nothing useful to do with an error (NFR-31).
    #[test]
    fn an_lfs_rule_that_cannot_be_written_is_refused_rather_than_failed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = recordings_profile(dir.path(), dir.path(), PushPolicy::SessionEnd);
        let attributes = p.local_path.join(".gitattributes");

        // Nothing stored under that id at all.
        assert!(
            !engine
                .ensure_lfs_rule("01NOSUCHPROFILE", "mp4")
                .expect("no such profile"),
            "a profile that does not exist is refused, not failed"
        );

        engine.upsert_profile(&p).expect("upsert");
        for extension in ["", ".", "   ", "mp 4", "a/b"] {
            assert!(
                !engine.ensure_lfs_rule(&p.id, extension).expect("refused"),
                "{extension:?} is not an extension and must not reach .gitattributes"
            );
        }

        engine.set_enabled(&p.id, false).expect("pause");
        assert!(
            !engine.ensure_lfs_rule(&p.id, "mp4").expect("paused"),
            "a paused folder's working tree is left exactly as the user left it"
        );
        engine.set_enabled(&p.id, true).expect("resume");

        let mut without_lfs = p.clone();
        without_lfs.lfs_mode = LfsMode::Disabled;
        engine.upsert_profile(&without_lfs).expect("upsert");
        assert!(
            !engine.ensure_lfs_rule(&p.id, "mp4").expect("lfs off"),
            "an LFS rule would promise something this folder will not honour"
        );

        // The folder itself is gone — an unplugged drive, from here.
        engine.upsert_profile(&p).expect("upsert");
        std::fs::remove_dir_all(&p.local_path).expect("unplug");
        assert!(!engine.ensure_lfs_rule(&p.id, "mp4").expect("no folder"));

        assert!(
            !attributes.exists(),
            "not one of those refusals may have written a thing"
        );
        assert_eq!(engine.counters(&p.id).attribute_writes, 0);
    }

    /// FR-136: the default policy holds everything until the session ends, so
    /// the meeting keeps the uplink it is running on.
    #[tokio::test]
    async fn the_default_policy_publishes_once_at_session_end_and_never_before() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = recordings_profile(dir.path(), &remote, PushPolicy::SessionEnd);
        engine.upsert_profile(&p).expect("upsert");

        for rotation in 0..3 {
            close_segment(&engine, &p, &format!("clip-{rotation}.mkv"), 512);
            assert_eq!(
                engine
                    .commit_local(&p, SyncSource::Watch, None)
                    .expect("commit at close"),
                1,
                "durability is not the thing being deferred: every close commits"
            );
            assert!(
                !engine
                    .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                    .await
                    .expect("policy"),
                "rotation {rotation} must not publish"
            );
        }
        assert_eq!(engine.counters(&p.id).commits, 3);
        assert_eq!(engine.counters(&p.id).pushes, 0, "not one attempt");
        assert!(!published(&remote, &p.branch));

        assert!(
            engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("finalize"),
            "and at the end it publishes"
        );
        assert_eq!(engine.counters(&p.id).pushes, 1, "exactly one");
        assert_eq!(
            engine.counters(&p.id).commits,
            3,
            "the finalize push has nothing left to commit"
        );
        assert!(published(&remote, &p.branch));
    }

    /// Epic 41's headline acceptance, in the counts it is stated in: a
    /// four-hour session of 48 rotations is 48 commits, ONE `.gitattributes`
    /// write, one push, and a journal that does not grow with the session.
    ///
    /// The `manifest.json` half of that count is the shell's (FR-146); this is
    /// everything the engine is accountable for.
    #[tokio::test]
    async fn a_four_hour_session_commits_every_rotation_and_writes_the_rule_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = recordings_profile(dir.path(), &remote, PushPolicy::SessionEnd);
        // A recorded segment is a large file, which is the whole reason the
        // session writes an LFS rule at all. Below the threshold the rule would
        // govern paths keeper stages as ordinary blobs, and this test would be
        // measuring a configuration nobody records with.
        p.lfs_threshold_bytes = 1024;
        engine.upsert_profile(&p).expect("upsert");

        // Session start: the one moment the working tree is allowed to change.
        assert!(engine.ensure_lfs_rule(&p.id, "mkv").expect("rule"));

        // Four hours at five minutes a segment.
        const ROTATIONS: u64 = 48;
        let mut committed_paths = 0;
        for rotation in 0..ROTATIONS {
            close_segment(&engine, &p, &format!("clip-{rotation:02}.mkv"), 4_096);
            platform.advance_ms(5 * 60_000);
            let paths = engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit at close");
            assert!(paths >= 1, "rotation {rotation} must commit as it closes");
            committed_paths += paths;
            // The session's own rule makes every `*.mkv` a filtered path, so a
            // racily-clean entry would send git to `filter.lfs.clean` — the
            // libtest harness, here — 48 times over. See `unrace_the_index`.
            unrace_the_index(&p);
            assert_eq!(
                engine.counters(&p.id).commits,
                rotation + 1,
                "one commit per rotation, never a batch"
            );
            assert!(
                !engine
                    .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                    .await
                    .expect("policy"),
                "and must not publish mid-session"
            );
        }
        assert_eq!(
            committed_paths,
            ROTATIONS + 1,
            "48 segments and the session's one rule — the rule rides in whichever \
             commit follows its own settle window, and in no other"
        );

        let counters = engine.counters(&p.id);
        assert_eq!(counters.commits, ROTATIONS, "one commit per rotation");
        assert_eq!(
            counters.attribute_writes, 1,
            "the working tree changed once, at the start, and never under the recorder"
        );
        assert_eq!(counters.pushes, 0, "nothing published during the meeting");
        assert_eq!(
            engine.pending_for(&p.id).expect("pending"),
            ROTATIONS as u32,
            "one upload per segment and not one row more: the journal tracks the \
             work, never the rotations"
        );

        // The objects go before the pointers that name them, which is the gate
        // the finalize push is waiting on (Story 34.15). Drained to quiescence
        // the way `sync_once` does it — one call claims `CLAIM_LIMIT` — and
        // bounded by a strictly decreasing count rather than by hope.
        let mut previous = u32::MAX;
        loop {
            engine
                .drain_journal(&p, false, SyncSource::Watch)
                .await
                .expect("drain");
            let outstanding = engine.lfs_uploads_outstanding(&p).expect("count");
            if outstanding == 0 || outstanding >= previous {
                break;
            }
            previous = outstanding;
        }
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);

        assert!(
            engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("finalize"),
            "and the whole session goes out once, at the end"
        );
        let counters = engine.counters(&p.id);
        assert_eq!(counters.pushes, 1);
        assert_eq!(counters.commits, ROTATIONS, "finalize commits nothing new");
        assert_eq!(counters.attribute_writes, 1);
        assert!(published(&remote, &p.branch));
    }

    /// `Immediate` is the opposite trade: one push per rotation, for the person
    /// who wants the bytes off this machine before the lid can be shut on it.
    #[tokio::test]
    async fn the_immediate_policy_publishes_every_segment_as_it_closes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = recordings_profile(dir.path(), &remote, PushPolicy::Immediate);
        engine.upsert_profile(&p).expect("upsert");

        for rotation in 0..3 {
            close_segment(&engine, &p, &format!("clip-{rotation}.mkv"), 512);
            assert!(
                engine
                    .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                    .await
                    .expect("policy"),
                "rotation {rotation} publishes as it closes"
            );
            assert_eq!(engine.counters(&p.id).pushes, rotation + 1);
            // The push commits what it is about to publish, so each rotation is
            // its own commit rather than a batch at the end.
            assert_eq!(engine.counters(&p.id).commits, rotation + 1);
        }
        assert!(published(&remote, &p.branch));
    }

    /// `Window` reads the LOCAL wall clock, and the commits simply accumulate
    /// outside it.
    #[tokio::test]
    async fn a_quiet_window_holds_the_commits_until_the_window_opens() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = recordings_profile(
            dir.path(),
            &remote,
            PushPolicy::Window {
                quiet_from: "22:00".to_owned(),
                quiet_to: "06:00".to_owned(),
            },
        );
        engine.upsert_profile(&p).expect("upsert");

        // A meeting in the middle of the working day, on a machine three hours
        // east of UTC — so the answer cannot come from the UTC clock alone.
        platform.set_utc_offset_minutes(180);
        advance_to_local(&platform, 14, 0);
        for rotation in 0..3 {
            close_segment(&engine, &p, &format!("clip-{rotation}.mkv"), 512);
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit at close");
            assert!(
                !engine
                    .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                    .await
                    .expect("policy"),
                "14:00 is not in the quiet hours"
            );
        }
        assert!(
            !engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("policy"),
            "and neither is the end of a session held during the day"
        );
        assert_eq!(engine.counters(&p.id).commits, 3, "the commits accumulate");
        assert_eq!(engine.counters(&p.id).pushes, 0);
        assert!(!published(&remote, &p.branch));

        // The evening comes.
        advance_to_local(&platform, 23, 0);
        assert!(
            engine
                .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                .await
                .expect("policy"),
            "inside the window the accumulated work goes out"
        );
        assert_eq!(engine.counters(&p.id).pushes, 1);
        assert!(published(&remote, &p.branch));
    }

    /// A remote that is not there for the whole session costs nothing but the
    /// publication, and the outstanding work drains when it comes back — with
    /// the pointer-ahead-of-its-object gate applying throughout (Story 34.15).
    #[tokio::test]
    async fn an_unreachable_remote_costs_the_push_and_nothing_else() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Deliberately not initialized: this is what an unreachable remote
        // looks like from here.
        let remote = dir.path().join("remote.git");
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let mut p = recordings_profile(dir.path(), &remote, PushPolicy::Immediate);
        p.lfs_threshold_bytes = 1024;
        engine.upsert_profile(&p).expect("upsert");

        let mut segments = Vec::new();
        for rotation in 0..2 {
            segments.push(close_segment(
                &engine,
                &p,
                &format!("clip-{rotation}.mp4"),
                4_096,
            ));
            assert!(
                !engine
                    .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                    .await
                    .expect("a dead remote is not an error the recorder handles"),
                "rotation {rotation} cannot publish"
            );
            unrace_the_index(&p);
        }
        assert_eq!(
            engine.counters(&p.id).commits,
            2,
            "every segment is committed locally regardless"
        );
        assert_eq!(
            engine.counters(&p.id).pushes,
            0,
            "the outstanding objects hold the push before it is even attempted, \
             so no pointer can be published ahead of its object"
        );
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 2);
        assert!(
            segments.iter().all(|path| path.exists()),
            "and nothing is deleted while the remote is away"
        );

        // The remote returns. The objects go first — that is the gate — and
        // only then does the branch.
        if gix::init_bare(&remote).is_err() {
            return;
        }
        engine
            .drain_journal(&p, false, SyncSource::Watch)
            .await
            .expect("drain");
        assert_eq!(engine.lfs_uploads_outstanding(&p).expect("count"), 0);
        assert!(
            engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("finalize"),
            "the session's work drains on reconnect"
        );
        assert_eq!(engine.counters(&p.id).pushes, 1);
        assert!(published(&remote, &p.branch));

        let store = lfs::local::remote_store(&p.remote_url).expect("a filesystem remote");
        let repo = engine.open_repo(&p).expect("open");
        for rotation in 0..2 {
            let rela = PathBuf::from(format!("recordings/clip-{rotation}.mp4"));
            let pointer =
                lfs::stage::indexed_pointer(&repo, &rela).expect("every segment is a pointer");
            assert!(
                store.contains(&pointer.oid, pointer.size),
                "the published pointer's object has to be on the remote too: {}",
                store.object_path(&pointer.oid).display()
            );
        }
    }

    /// A paused profile and an unplugged drive are refusals, not failures, and
    /// neither one loses a segment (AD-48, NFR-31).
    #[tokio::test]
    async fn a_paused_profile_and_an_absent_volume_refuse_and_delete_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = recordings_profile(dir.path(), &remote, PushPolicy::Immediate);
        engine.upsert_profile(&p).expect("upsert");
        let segment = close_segment(&engine, &p, "clip.mkv", 512);

        engine.set_enabled(&p.id, false).expect("pause");
        assert!(
            !engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("a paused folder is not an error"),
            "a paused folder publishes nothing and says so"
        );
        assert_eq!(engine.counters(&p.id).pushes, 0);
        assert!(segment.exists(), "pausing is not deleting");
        assert!(!published(&remote, &p.branch));

        // Resumed, but the drive it lives on is gone: `removable` with no
        // marker anywhere above the folder is exactly an unplugged stick.
        engine.set_enabled(&p.id, true).expect("resume");
        let mut removable = p.clone();
        removable.removable = true;
        engine.upsert_profile(&removable).expect("upsert");
        assert!(
            !engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("an absent volume is not an error either"),
            "an unplugged drive publishes nothing and says so"
        );
        assert_eq!(engine.counters(&p.id).pushes, 0);
        assert_eq!(
            engine.status(&p.id).expect("status").state,
            ProfileState::MediaAbsent
        );
        assert!(
            segment.exists(),
            "an unplugged drive must never be read as a deletion"
        );
    }

    /// The quiet window is local wall-clock, half-open, and wraps midnight —
    /// which is the shape people actually configure.
    #[test]
    fn a_quiet_window_wraps_midnight_and_follows_the_local_clock() {
        /// `hh:mm` on the epoch's own day, in UTC milliseconds.
        fn utc(hour: i64, minute: i64) -> i64 {
            (hour * 60 + minute) * 60_000
        }
        let wrapping = |now: i64, offset: i32| within_quiet_window(now, offset, "22:00", "06:00");

        assert_eq!(wrapping(utc(23, 0), 0), Some(true), "after midnight's eve");
        assert_eq!(wrapping(utc(2, 0), 0), Some(true), "and after midnight");
        assert_eq!(wrapping(utc(22, 0), 0), Some(true), "the start is inside");
        assert_eq!(wrapping(utc(6, 0), 0), Some(false), "the end is not");
        assert_eq!(wrapping(utc(12, 0), 0), Some(false), "the middle of a day");

        // The same instant, read on two different wall clocks. This is the
        // whole reason the offset is a port method rather than an assumption.
        assert_eq!(wrapping(utc(20, 0), 0), Some(false), "20:00 in London");
        assert_eq!(wrapping(utc(20, 0), 180), Some(true), "23:00 in Istanbul");

        // A window that does not wrap is read as written.
        let plain = |now: i64| within_quiet_window(now, 0, "01:00", "05:00");
        assert_eq!(plain(utc(3, 0)), Some(true));
        assert_eq!(plain(utc(0, 30)), Some(false));
        assert_eq!(plain(utc(6, 0)), Some(false));

        // West of UTC before the epoch: the local millisecond count is
        // negative, and a `%` here would answer with a negative minute-of-day
        // that compares below every window ever configured.
        assert_eq!(
            within_quiet_window(-1, -300, "18:00", "19:00"),
            Some(true),
            "one millisecond before the epoch, five hours west, is 18:59 local"
        );

        // Unreadable rather than guessed. `RecordingsConfig::validate` refuses
        // all of these, so reaching one means a row edited outside keeper.
        for bad in ["", "noon", "24:00", "12:60", "12", "12:0x"] {
            assert_eq!(
                within_quiet_window(utc(23, 0), 0, bad, "06:00"),
                None,
                "{bad:?} says nothing about when to push"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Story 41.6 — durability you can read
    // -----------------------------------------------------------------------

    /// A second folder bound to the SAME remote.
    ///
    /// The cheapest honest way to make a remote that has moved on without this
    /// folder, which is all a rejected push is: two unrelated histories aimed
    /// at one branch. Nothing here fakes a git answer — the second push is
    /// refused by real git for the real reason.
    fn rival_profile(dir: &Path, remote: &Path) -> SyncProfile {
        let mut p = recordings_profile(dir, remote, PushPolicy::Immediate);
        p.id = "01JTESTRIVAL".to_owned();
        p.name = "rival".to_owned();
        p.local_path = dir.join("rival");
        std::fs::create_dir_all(p.recordings_root().expect("a recordings root"))
            .expect("the rival's recordings root");
        p
    }

    /// FR-138: the state a recording surface reads advances local → committed
    /// → pushed, from local facts only.
    #[tokio::test]
    async fn a_segment_reads_local_then_committed_then_pushed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };
        let p = recordings_profile(dir.path(), &remote, PushPolicy::SessionEnd);
        engine.upsert_profile(&p).expect("upsert");

        // A segment that has just closed is on this disk and nowhere else.
        let first = close_segment(&engine, &p, "clip-0.mkv", 512);
        let local = engine.path_durability(&p.id, &first).expect("read");
        assert_eq!(
            local,
            PathDurability::default(),
            "an uncommitted segment promises nothing beyond the drive it is on"
        );

        // Committed at close (Story 41.5) is the first real promise.
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit at close"),
            1
        );
        let committed = engine.path_durability(&p.id, &first).expect("read");
        assert!(committed.committed, "the segment is in a commit now");
        assert!(
            !committed.pushed,
            "and nothing has been published, so it must not claim to be"
        );
        assert!(!committed.verified);
        assert_eq!(committed.problem, None, "nothing has gone wrong");

        // Published. The claim is branch-level, and the branch is now on the
        // remote, so the segment it carries is too.
        assert!(
            engine
                .push_recordings_if_due(&p.id, PushTrigger::SessionEnd)
                .await
                .expect("finalize"),
            "the session-end push publishes"
        );
        assert!(published(&remote, &p.branch));
        let pushed = engine.path_durability(&p.id, &first).expect("read");
        assert!(
            pushed.committed && pushed.pushed,
            "the segment's commit has reached the remote: {pushed:?}"
        );
        assert!(
            !pushed.verified,
            "`verified` is the verification pass's word, never guessed from a push"
        );

        // Mid-session mix: the next segment closes and is not committed yet.
        // The engine reports each path honestly — the never-regress floor is
        // the surface's job, and it can only keep a floor if the readings
        // underneath it are true.
        let second = close_segment(&engine, &p, "clip-1.mkv", 512);
        let fresh = engine.path_durability(&p.id, &second).expect("read");
        assert!(
            !fresh.committed && !fresh.pushed,
            "a segment that just closed is local, whatever its predecessors reached"
        );
        let older = engine.path_durability(&p.id, &first).expect("read");
        assert!(
            older.committed && older.pushed,
            "and a segment already on the remote does not stop being on it: {older:?}"
        );
    }

    /// A recording whose publication was refused reads "committed", and the
    /// reason is available in the words the engine recorded — not as a generic
    /// sync error, and never as a failure of the recording.
    #[tokio::test]
    async fn a_rejected_push_stays_committed_and_carries_the_recorded_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let remote = dir.path().join("remote.git");
        if gix::init_bare(&remote).is_err() {
            return;
        }
        let platform = Arc::new(TestPlatform::new(dir.path()));
        let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
            return;
        };

        // Someone else's history gets to the branch first.
        let rival = rival_profile(dir.path(), &remote);
        engine.upsert_profile(&rival).expect("upsert the rival");
        close_segment(&engine, &rival, "theirs.mkv", 512);
        assert!(
            engine
                .push_recordings_if_due(&rival.id, PushTrigger::SessionEnd)
                .await
                .expect("the rival publishes"),
            "the remote has to hold work this folder does not, or nothing is rejected"
        );

        // Now our own session records and tries to publish.
        let p = recordings_profile(dir.path(), &remote, PushPolicy::Immediate);
        engine.upsert_profile(&p).expect("upsert");
        let segment = close_segment(&engine, &p, "ours.mkv", 512);
        assert!(
            !engine
                .push_recordings_if_due(&p.id, PushTrigger::SegmentCommitted)
                .await
                .expect("a refused push is not an error the recorder handles"),
            "the remote refuses this push"
        );

        let durability = engine.path_durability(&p.id, &segment).expect("read");
        assert!(
            durability.committed,
            "the recording did not fail — its publication did: {durability:?}"
        );
        assert!(!durability.pushed, "and it is not on the remote");
        // No sentence, and that is the answer now (DW-207). This profile is
        // bidirectional, so being overtaken by the rival is a race rather than
        // a decision: it comes back as `RemoteMoved`, which is transient, and
        // `record_failure` deliberately hands no words to a recorder about work
        // that is about to be retried. The recording is a step behind, not
        // broken, and saying so in red would be a lie that recurs every time
        // two machines are awake at once.
        assert!(
            durability.problem.is_none(),
            "a race that heals itself must not be reported as a failed publication: {durability:?}"
        );
        // The reconcile itself runs inline in `reconcile_and_retry_push`, so
        // there is no queue entry to look for here; that ordering is what
        // `a_bidirectional_push_reconciles_before_it_retries` pins down. What
        // this test owns is the durability contract, and all three parts of it
        // hold: the recording is committed, it is not yet on the remote, and
        // nobody is told a publication failed.
        assert!(!durability.verified);
        // The recording itself is untouched by any of it.
        assert!(segment.exists());
    }

    /// Every refusal is `Ok` with nothing claimed, because a durability read
    /// runs on the poll path of a live capture (NFR-31).
    #[test]
    fn a_durability_read_refuses_rather_than_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = recordings_profile(dir.path(), dir.path(), PushPolicy::SessionEnd);
        let root = p.recordings_root().expect("a recordings root");

        // Nothing stored under that id at all.
        assert_eq!(
            engine
                .path_durability("01NOSUCHPROFILE", &root.join("clip.mkv"))
                .expect("no such profile"),
            PathDurability::default()
        );

        // The same folder with its recordings block removed, the way
        // `an_lfs_rule_that_cannot_be_written_is_refused_rather_than_failed`
        // removes LFS: there is no recordings root to be inside, so there is no
        // promise to make about anything in it.
        let mut without_recordings = p.clone();
        without_recordings.recordings = None;
        engine.upsert_profile(&without_recordings).expect("upsert");
        assert_eq!(
            engine
                .path_durability(&p.id, &root.join("clip.mkv"))
                .expect("no recordings"),
            PathDurability::default()
        );

        engine.upsert_profile(&p).expect("upsert");
        // Outside the recordings root: elsewhere in the folder, outside the
        // folder, relative, and reaching back out with `..`.
        for outside in [
            p.local_path.join("notes.md"),
            dir.path().join("elsewhere/clip.mkv"),
            PathBuf::from("clip.mkv"),
            root.join("../notes.md"),
        ] {
            assert_eq!(
                engine
                    .path_durability(&p.id, &outside)
                    .expect("outside is a refusal, not an error"),
                PathDurability::default(),
                "{} must be refused",
                outside.display()
            );
        }

        // Paused: what is on the drive is all there is to say.
        engine.set_enabled(&p.id, false).expect("pause");
        assert_eq!(
            engine
                .path_durability(&p.id, &root.join("clip.mkv"))
                .expect("paused"),
            PathDurability::default()
        );
    }

    /// The read is local-only: a folder whose remote is not there answers
    /// exactly as fast and exactly as honestly as one whose remote is.
    ///
    /// Proved the way this module proves offline behaviour elsewhere — a remote
    /// that was never initialized, which is what an unreachable one looks like
    /// from here — plus a wall-clock bound, because "cheap on a 1 Hz poll" is
    /// the constraint and a call that reached out would spend its timeout here.
    #[test]
    fn a_durability_read_answers_promptly_with_the_remote_unreachable() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Deliberately not initialized, and a host that resolves to nothing:
        // any attempt to consult the remote would either fail or hang, and both
        // are visible in what this asserts.
        let unreachable = dir.path().join("never-created.git");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = recordings_profile(dir.path(), &unreachable, PushPolicy::SessionEnd);
        p.remote_url = "https://git.invalid/x/y.git".to_owned();
        engine.upsert_profile(&p).expect("upsert");
        let segment = close_segment(&engine, &p, "clip.mkv", 512);
        assert_eq!(
            engine
                .commit_local(&p, SyncSource::Watch, None)
                .expect("commit at close"),
            1
        );

        // Ten reads, the poll rate of ten seconds of recording.
        let started = std::time::Instant::now();
        for _ in 0..10 {
            let durability = engine.path_durability(&p.id, &segment).expect("read");
            assert!(
                durability.committed,
                "the commit is local, so an absent remote cannot make it unknown"
            );
            assert!(
                !durability.pushed,
                "and nothing may be claimed about a remote nobody has spoken to"
            );
        }
        let elapsed = started.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "ten reads took {elapsed:?}; a read that consults the remote cannot be polled"
        );
        assert_eq!(
            engine.counters(&p.id).pushes,
            0,
            "reading durability must never publish anything"
        );
    }

    /// Under `PointerOnly`, a REQUESTED download publishes and a background one
    /// does not (Story 56.3).
    ///
    /// The one decision point the story turns on, asserted directly on the
    /// completion arm because nothing else can see it. `materialize_landed`
    /// opens with `lfs_mode != Materialize → return Ok(())`, which is right for
    /// arrival and fatal for a request: `PointerOnly` is the mode in which
    /// paths are virtual in the first place, so without the flag asking for a
    /// file under it would fetch the object into the store, return `Ok`, and
    /// leave the worktree holding pointer text — a green test over an inert
    /// feature.
    ///
    /// Both directions are asserted in one test on purpose: "it publishes" is
    /// only half the claim, and a flag that published unconditionally would
    /// pass the first half while silently retiring `PointerOnly` for everybody.
    #[test]
    fn a_requested_download_publishes_under_pointer_only_and_a_background_one_does_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_mode = LfsMode::PointerOnly;
        engine.upsert_profile(&p).expect("upsert");
        // A real repository first, before anything writes into `.git`: the
        // completion arm refreshes the index stat through `open_repo`, and a
        // `.git` that exists and holds no `HEAD` is the half-made state that
        // refuses to open. The folder has to be non-empty for adoption to be
        // the path taken — `open_repo` clones an EMPTY directory, and this
        // fixture's remote does not resolve — and adoption is `git init` plus a
        // remote config, never network.
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.ensure_repo(&p).expect("adopt the fixture folder");

        // The state a download leaves behind: the object in this machine's
        // store, the worktree still holding the pointer git checked out.
        let content = vec![9u8; 4_096];
        let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
        store.ensure_layout().expect("store layout");
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(content.clone()))
            .expect("seed the object");
        let pointer = lfs::pointer::Pointer::new(oid.clone(), size);
        let target = p.local_path.join("clip.mp4");
        std::fs::write(&target, pointer.render()).expect("check out the pointer");

        // The unit itself, in the journal, because the journal is where the
        // answer is read from: a literal `true` here would assert the
        // destination and leave the source — `claim_ready` → `promote_unit` →
        // `unit_urgency` — untested, which is how a request that arrives while
        // its download is already running got dropped.
        let unit = WorkKind::LfsDownload {
            oid: oid.clone(),
            size,
        };
        let unit_id = engine
            .with_db(|conn| db::enqueue_unique(conn, &p.id, &unit, 1, 1))
            .expect("queue the download");

        // Arrival policy, unchanged: `PointerOnly` means "leave what arrives as
        // pointers", and nobody has asked for this one.
        engine
            .materialize_landed(&p, &store, &oid, Some("clip.mp4"), Some(unit_id))
            .expect("a background arrival is not an error");
        assert_eq!(
            std::fs::read(&target).expect("read"),
            pointer.render().into_bytes(),
            "a background download under `PointerOnly` still leaves the pointer"
        );

        // The request arrives — and it arrives against a row that is already
        // being executed, which is the case `covered_while_running` makes
        // ordinary and a claim-time snapshot cannot see.
        engine
            .with_db(|conn| db::promote_unit(conn, unit_id, 2))
            .expect("promote the row somebody asked for");
        engine
            .materialize_landed(&p, &store, &oid, Some("clip.mp4"), Some(unit_id))
            .expect("a requested arrival publishes");
        assert_eq!(
            std::fs::read(&target).expect("read"),
            content,
            "the path somebody asked for holds the real bytes, which is the \
             whole point of the verb under the mode that makes paths virtual"
        );

        // `Disabled` is NOT bypassed: nothing routes the path through the clean
        // filter there, so real bytes in the worktree would become a new blob
        // in the next commit.
        let mut disabled = p.clone();
        disabled.lfs_mode = LfsMode::Disabled;
        std::fs::write(&target, pointer.render()).expect("back to the pointer");
        engine
            .materialize_landed(&disabled, &store, &oid, Some("clip.mp4"), Some(unit_id))
            .expect("still not an error");
        assert_eq!(
            std::fs::read(&target).expect("read"),
            pointer.render().into_bytes(),
            "a request cannot turn LFS back on for a folder whose owner turned it off"
        );
    }

    /// Every path the requested object sits at is published, not just the one
    /// whose name the row happens to carry (Story 56.3 review).
    ///
    /// Two paths holding identical bytes share one oid, so they share one
    /// download and one journal row — and `label_unit` is first-writer-wins, so
    /// the row keeps the name of whichever was asked for first. Publishing only
    /// the label meant the second request reported a unit id and never
    /// delivered: under `PointerOnly` the whole-tree sweep is switched off, so
    /// nothing would ever have repaired it.
    ///
    /// The background half is asserted in the same test, because "publish
    /// everything" would retire `PointerOnly`'s arrival policy for duplicated
    /// content.
    #[test]
    fn a_requested_object_publishes_every_path_it_is_committed_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let mut p = adoptable(dir.path());
        p.lfs_mode = LfsMode::PointerOnly;
        engine.upsert_profile(&p).expect("upsert");
        std::fs::write(p.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.ensure_repo(&p).expect("adopt the fixture folder");

        let content = vec![7u8; 8_192];
        let store = lfs::store::LfsStore::in_git_dir(p.local_path.join(".git"));
        store.ensure_layout().expect("store layout");
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(content.clone()))
            .expect("seed the object");
        let pointer = lfs::pointer::Pointer::new(oid.clone(), size);
        // Both paths hold the same pointer text, and both are committed, so the
        // index records the same oid twice — which is what makes them one unit.
        for name in ["clip.mp4", "clip-copy.mp4"] {
            std::fs::write(p.local_path.join(name), pointer.render()).expect("check out");
        }
        let repo = git::repo::open(&p.local_path, false).expect("open");
        let changes = git::commit::StagedChange {
            added: vec![
                PathBuf::from("clip.mp4"),
                PathBuf::from("clip-copy.mp4"),
                PathBuf::from("keep.txt"),
            ],
            ..Default::default()
        };
        let signature = gix::actor::Signature {
            name: "t".into(),
            email: "t@example.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        };
        let provenance = Provenance::new(
            &p.name,
            "dev",
            "01JDEV",
            "host",
            crate::provenance::SyncSource::Cli,
        );
        git::commit::stage_and_commit(
            &repo,
            &changes,
            &provenance,
            &p,
            &signature,
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit both pointers");
        drop(repo);

        let unit = WorkKind::LfsDownload {
            oid: oid.clone(),
            size,
        };
        let unit_id = engine
            .with_db(|conn| {
                let id = db::enqueue_unique(conn, &p.id, &unit, 1, 1)?;
                // The first request names the first path, and the second one
                // cannot rename it.
                db::label_unit(conn, id, "clip.mp4")?;
                db::promote_unit(conn, id, 1)?;
                Ok(id)
            })
            .expect("queue and promote one unit for both paths");

        engine
            .materialize_landed(&p, &store, &oid, Some("clip.mp4"), Some(unit_id))
            .expect("publish");
        assert_eq!(
            std::fs::read(p.local_path.join("clip.mp4")).expect("read"),
            content,
            "the labelled path holds its content"
        );
        assert_eq!(
            std::fs::read(p.local_path.join("clip-copy.mp4")).expect("read"),
            content,
            "and so does the path the row could not name — the request was for \
             this content, and this is where the folder commits it"
        );

        // The background half: the same fixture, the same object, no promotion.
        for name in ["clip.mp4", "clip-copy.mp4"] {
            std::fs::write(p.local_path.join(name), pointer.render()).expect("back to pointers");
        }
        let second = engine
            .with_db(|conn| {
                let id = db::enqueue(conn, &p.id, &unit, 2, 2)?;
                db::label_unit(conn, id, "clip.mp4")?;
                Ok(id)
            })
            .expect("queue a background unit");
        engine
            .materialize_landed(&p, &store, &oid, Some("clip.mp4"), Some(second))
            .expect("a background arrival is not an error");
        assert_eq!(
            std::fs::read(p.local_path.join("clip.mp4")).expect("read"),
            pointer.render().into_bytes(),
            "under `PointerOnly` a background arrival still publishes nothing"
        );
        assert_eq!(
            std::fs::read(p.local_path.join("clip-copy.mp4")).expect("read"),
            pointer.render().into_bytes(),
            "and it certainly does not reach for the sibling"
        );
    }

    /// Every refusal the door itself produces, on one fixture (Story 56.3
    /// review).
    ///
    /// These were the nine matrix rows nothing exercised: each is a sentence a
    /// user meets and a branch a refactor can silently invert, and none of them
    /// needs a committed pointer to reach. Asserted on the refusal's *variant*,
    /// which is what "a typed error the caller can distinguish" has to mean.
    #[test]
    fn the_door_refuses_by_name_before_it_touches_a_worktree() {
        use lfs::hydrate::ContentRefusal;

        let dir = tempfile::tempdir().expect("tempdir");
        let Some(engine) = engine(dir.path()) else {
            return;
        };
        let p = adoptable(dir.path());
        engine.upsert_profile(&p).expect("upsert");

        let refusal = |subpath: &str| match engine.materialize_entry(&p.id, subpath) {
            Err(SyncError::Refused(refusal)) => refusal,
            other => panic!("expected a refusal for {subpath:?}, got {other:?}"),
        };

        // Containment, both halves of it, and the root itself.
        assert!(matches!(
            refusal("../etc/passwd"),
            ContentRefusal::Escapes(_)
        ));
        assert!(matches!(refusal("/etc/passwd"), ContentRefusal::Escapes(_)));
        assert!(matches!(refusal(""), ContentRefusal::Escapes(_)));
        // Refused whatever the folder is doing: the containment answer must not
        // depend on whether a tick happens to hold the reservation, which is
        // what putting it after `reserve` did.
        {
            let _held = engine.reserve(&p.id).expect("hold the reservation");
            assert!(matches!(
                refusal("../etc/passwd"),
                ContentRefusal::Escapes(_)
            ));
            assert!(
                matches!(
                    engine.materialize_entry(&p.id, "clip.mp4"),
                    Err(SyncError::Busy(_))
                ),
                "and a path that IS askable answers busy while the folder is"
            );
        }

        // A folder with no repository in it has no committed pointer, so there
        // is nothing to ask for — and no `.git` is created by asking.
        assert!(matches!(
            refusal("clip.mp4"),
            ContentRefusal::NotTracked { .. }
        ));
        assert!(
            !p.local_path.join(".git").exists(),
            "asking must not make a repository"
        );

        // Paused: keeper is not writing into this working tree at all, and a
        // queued unit would never be claimed either.
        let mut paused = p.clone();
        paused.enabled = false;
        engine.upsert_profile(&paused).expect("pause");
        assert!(matches!(refusal("clip.mp4"), ContentRefusal::Paused { .. }));
        engine.upsert_profile(&p).expect("resume");

        // LFS off for the folder: real bytes here would become a new blob in
        // the next commit, so it is refused by the mode's own name.
        let mut disabled = p.clone();
        disabled.lfs_mode = LfsMode::Disabled;
        engine.upsert_profile(&disabled).expect("disable lfs");
        assert!(matches!(
            refusal("clip.mp4"),
            ContentRefusal::LfsDisabled { .. }
        ));
        engine.upsert_profile(&p).expect("re-enable lfs");

        // Outside the cone this profile exists to keep away — asserted through
        // a real repository, because the cone is checked after the index, and
        // in a SUBDIRECTORY, because git's cone matching (and so
        // `SparseCone::includes`) always keeps the repository root's own files.
        let mut coned = p.clone();
        coned.subpaths = vec!["docs".to_owned()];
        engine.upsert_profile(&coned).expect("cone");
        std::fs::write(coned.local_path.join("keep.txt"), b"ordinary").expect("write");
        engine.ensure_repo(&coned).expect("adopt");
        let content = vec![3u8; 2_048];
        let store = lfs::store::LfsStore::in_git_dir(coned.local_path.join(".git"));
        store.ensure_layout().expect("store layout");
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(content))
            .expect("seed");
        let pointer = lfs::pointer::Pointer::new(oid, size);
        std::fs::create_dir_all(coned.local_path.join("media")).expect("zone");
        std::fs::write(coned.local_path.join("media/clip.mp4"), pointer.render())
            .expect("write pointer");
        let repo = git::repo::open(&coned.local_path, false).expect("open");
        let changes = git::commit::StagedChange {
            added: vec![PathBuf::from("media/clip.mp4"), PathBuf::from("keep.txt")],
            ..Default::default()
        };
        let signature = gix::actor::Signature {
            name: "t".into(),
            email: "t@example.invalid".into(),
            time: gix::date::Time::new(1_700_000_000, 0),
        };
        let provenance = Provenance::new(
            &coned.name,
            "dev",
            "01JDEV",
            "host",
            crate::provenance::SyncSource::Cli,
        );
        git::commit::stage_and_commit(
            &repo,
            &changes,
            &provenance,
            &coned,
            &signature,
            &std::collections::BTreeMap::new(),
            None,
        )
        .expect("commit the pointer");
        drop(repo);
        assert!(matches!(
            match engine.materialize_entry(&coned.id, "media/clip.mp4") {
                Err(SyncError::Refused(refusal)) => refusal,
                other => panic!("expected a refusal, got {other:?}"),
            },
            ContentRefusal::OutsideSubpaths { .. }
        ));

        // A plain tracked file has no pointer, and a path in the index that is
        // gone from the worktree must not be resurrected.
        engine.upsert_profile(&p).expect("no cone");
        assert!(matches!(
            refusal("keep.txt"),
            ContentRefusal::NotTracked { .. }
        ));
        std::fs::remove_file(p.local_path.join("media/clip.mp4")).expect("delete it locally");
        assert!(matches!(
            refusal("media/clip.mp4"),
            ContentRefusal::Missing { .. }
        ));

        // An unknown profile is a configuration error, not a refusal: the
        // caller named something that does not exist.
        assert!(matches!(
            engine.materialize_entry("01NOSUCHPROFILE", "clip.mp4"),
            Err(SyncError::Config(_))
        ));
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
