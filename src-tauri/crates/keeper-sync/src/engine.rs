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
use std::time::Instant;

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
            device: Mutex::new(device),
            git,
            http,
            status: Mutex::new(HashMap::new()),
            gates: Mutex::new(HashMap::new()),
            busy: Mutex::new(HashMap::new()),
            collapsed_reported: Mutex::new(HashSet::new()),
            transient_failures: Mutex::new(HashMap::new()),
            next_scan_ms: Mutex::new(HashMap::new()),
            // `current_exe` is the daemon in a CLI run and the app binary in a
            // desktop run; both understand `lfs clean|smudge`.
            filter_program: std::env::current_exe().ok(),
            sinks: Mutex::new(Vec::new()),
            next_sink: AtomicU64::new(1),
            interrupt: Arc::new(AtomicBool::new(false)),
            transferred: Mutex::new(HashMap::new()),
            watchers: Mutex::new(HashMap::new()),
            watch_wake: Mutex::new(HashSet::new()),
            lfs_ssh_credentials: Mutex::new(HashMap::new()),
            watch_tap: OnceLock::new(),
            finished_tap: OnceLock::new(),
            counters: Mutex::new(HashMap::new()),
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
        if enabled {
            // Work deferred while paused becomes eligible again immediately;
            // making the user wait out a backoff they did not cause is rude.
            let now = self.platform.now_ms();
            self.with_db(|conn| db::undefer_profile(conn, id, now))?;
            // ...and neither is making them wait out a scan window: resuming is
            // a request to look now.
            Self::lock(&self.next_scan_ms).remove(id);
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
    ///   words, and a refused push is one — a rejected ref classifies as
    ///   [`SyncError::Diverged`], which is permanent and lands there verbatim.
    ///   A failure still being retried deliberately carries no sentence yet
    ///   (`record_failure`'s policy, not this one), which is right: work that
    ///   is about to be retried is not something to hand a recorder words
    ///   about.
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
        let claimed = self.with_db(|conn| db::claim_ready(conn, &profile.id, now, CLAIM_LIMIT))?;
        if claimed.is_empty() {
            if scan_when_idle {
                self.scan_and_enqueue(profile, source)?;
            }
            return Ok(());
        }

        for item in claimed {
            match self.execute(profile, &item.kind, item.id, source).await {
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

    /// `unit_id` is the journal row being executed. Only the push leg uses it:
    /// the activity rows its commit records name it as the unit whose success
    /// delivers them (Story 34.16).
    async fn execute(
        &self,
        profile: &SyncProfile,
        kind: &WorkKind,
        unit_id: i64,
        source: SyncSource,
    ) -> Result<()> {
        match kind {
            // The conflict copies are already recorded and warned about;
            // a journaled pull has no caller to hand them back to.
            WorkKind::Pull => {
                self.do_pull(profile, source).await?;
                self.mark_synced(profile);
                Ok(())
            }
            WorkKind::Push => {
                self.do_push(profile, source, Some(unit_id)).await?;
                self.mark_synced(profile);
                Ok(())
            }
            WorkKind::LfsDownload { oid, size } => self.do_lfs(profile, oid, *size, false).await,
            WorkKind::LfsUpload { oid, size } => self.do_lfs(profile, oid, *size, true).await,
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
    fn mark_synced(&self, profile: &SyncProfile) {
        if let Some(snapshot) = Self::lock(&self.status).get_mut(&profile.id) {
            snapshot.last_sync_ms = Some(self.platform.now_ms());
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
        git::repo::enforce_local_config_with_filter(&repo, self.filter_program.as_deref())?;
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
    async fn do_pull(&self, profile: &SyncProfile, source: SyncSource) -> Result<Vec<String>> {
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
        let conflicts = tokio::task::spawn_blocking(move || {
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
            let rows: Vec<db::ActivityEntry> = conflicts
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

        let git = self.git.clone();
        let repo_path = profile.local_path.clone();
        // A lane publishes ONLY its own branch. The base branch is never in
        // the refspec, so pushing over a human's work is impossible by
        // construction rather than by care (AD-50).
        let working = Self::working_branch(profile);
        let refspec = format!("refs/heads/{working}:refs/heads/{working}");
        let credential = self.credential(profile)?;
        // Counted where the invocation is, not where it returns: an attempt
        // against an unreachable remote is still this policy firing, and a
        // session that claims "one push at the end" has to be able to show it
        // even when that push failed (Story 41.5).
        self.bump_counters(&profile.id, |counters| counters.pushes += 1);
        tokio::task::spawn_blocking(move || {
            git.push(&repo_path, "origin", &refspec, credential.as_ref())
        })
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
        let status = git::repo::status_paths(&repo)?;
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
            if let Some(size) = Self::removed_size(&repo, rela) {
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
        let staging = lfs::stage::prepare(profile, &store, &candidates)?;

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

    /// How big a deleted path used to be, if the repository still knows.
    ///
    /// The index entry git kept for the path names the blob it last recorded,
    /// and a blob's length *is* the file's length — except for an LFS-tracked
    /// path, where the blob is a pointer and the real size is the number
    /// written inside it. Both are answered here. Anything else — a path whose
    /// index entry an earlier, half-finished stage already removed — answers
    /// `None`: a guessed figure in a size column is worse than a blank one.
    fn removed_size(repo: &gix::Repository, rela: &Path) -> Option<u64> {
        let index = repo.index_or_empty().ok()?;
        // The index keys paths with forward slashes on every platform, the
        // same conversion `lfs::stage::indexed_pointer` makes.
        let key = rela.to_string_lossy().replace('\\', "/");
        let entry = index.entry_by_path(gix::bstr::BStr::new(key.as_bytes()))?;
        let blob = repo.find_header(entry.id).ok()?.size();
        // A pointer is under 1 KiB by specification, so a larger blob cannot
        // be one and must not be loaded to find that out.
        if blob > lfs::pointer::MAX_POINTER_BYTES as u64 {
            return Some(blob);
        }
        let object = repo.find_object(entry.id).ok()?;
        Some(lfs::pointer::Pointer::parse(&object.data).map_or(blob, |pointer| pointer.size))
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
        // The direction is in the phase because that is what the tray reads: an
        // upload and a download are different glyphs, so they have to be
        // different phases (see `SyncPhase::direction`).
        let phase = if upload {
            SyncPhase::UploadingLfs
        } else {
            SyncPhase::DownloadingLfs
        };
        self.publish(self.progress(profile, phase));

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
                    .copy_lfs_object(profile, remote, oid, size, upload)
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
            lfs::basic::BasicTransfer::new(self.http.clone(), store.clone())
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

        if !upload {
            // The object is in the store; the worktree still holds the pointer
            // that was checked out. Replacing it is what makes a peer's clone
            // contain real bytes rather than a text stub.
            self.materialize_pending(profile, &store)?;
        }
        Ok(())
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
    async fn copy_lfs_object(
        &self,
        profile: &SyncProfile,
        remote: lfs::store::LfsStore,
        oid: &str,
        size: u64,
        upload: bool,
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

        if !upload {
            // The object is in the store; the worktree still holds the pointer
            // that was checked out. Replacing it is what makes this clone contain
            // real bytes rather than a text stub.
            self.materialize_pending(profile, &local)?;
        }
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
    fn materialize_pending(
        &self,
        profile: &SyncProfile,
        store: &lfs::store::LfsStore,
    ) -> Result<()> {
        if profile.lfs_mode == LfsMode::Disabled {
            return Ok(());
        }
        let repo = self.open_repo(profile)?;
        let cone = SparseCone::new(&profile.subpaths);
        let tracked: Vec<PathBuf> = git::repo::tracked_paths(&repo)?
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
            if store.contains(&smudge.pointer.oid, smudge.pointer.size) {
                // Pointer-only mode leaves content as a pointer on purpose: it
                // is the whole-profile lever, where `subpaths[]` above is the
                // per-path one. Both exist because git-lfs is entirely
                // sparse-checkout-unaware.
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

        if profile.direction.pulls() {
            outcome.conflicts = self.do_pull(&profile, source).await?;
            outcome.pulled = true;
            // Whatever the apply checked out may include pointers. Materialize
            // what is already local and queue the rest, BEFORE the push leg —
            // otherwise the scan would see a pointer-sized file where the index
            // records the full length and call it an edit.
            let store = lfs::store::LfsStore::in_git_dir(profile.local_path.join(".git"));
            self.materialize_pending(&profile, &store)?;
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
        outcome.bytes = self
            .transferred_bytes(&profile.id)
            .saturating_sub(transferred_before);

        // Every leg this profile's direction calls for ran and none of them
        // raised: that is the whole definition of a successful pass, and it is
        // the only definition under which a pull-only profile — or a push with
        // nothing staged — can ever record that it worked.
        self.mark_synced(&profile);

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
        // The walk must happen now, not at the end of the poll window: pressing
        // this is a request to look, and a folder that sat unchanged for a minute
        // afterwards would read as the button having done nothing.
        Self::lock(&self.next_scan_ms).remove(id);
        self.note_watch_wake(id);
        tracing::info!(profile = id, "forgot the remembered tree state on request");
        Ok(())
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
        .map_err(|err| SyncError::Journal(format!("verify task failed: {err}")));

        // Published before the `?`, not after it. `verify` takes no reservation
        // — it only reads — so it is the one pass `Reservation::drop` does not
        // cover, and an unreadable tree used to leave `Verifying` on the tray
        // and a bar on the card until the process ended (Story 34.8).
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
        let settling = self.with_db(|conn| db::load_file_state(conn, profile_id))?;
        let mut out: Vec<PendingFile> = Vec::with_capacity(settling.len());
        let mut named: std::collections::HashSet<String> =
            std::collections::HashSet::with_capacity(settling.len());
        for (path, entry) in settling {
            let relative = path.strip_prefix(&profile.local_path).unwrap_or(&path);
            // A row written before a pattern was added to the profile: the gate
            // will drop it on the next walk, and until then it must not be
            // shown.
            if excludes.is_excluded(relative) {
                continue;
            }
            let relative = relative.to_string_lossy().into_owned();
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
            let filter = excludes.clone();
            // The status walk and the untracked expansion are both blocking
            // filesystem work on a tree that may hold a hundred thousand
            // files; running them on the async runtime would stall every other
            // profile while a UI poll finished.
            let (status, untracked) = tokio::task::spawn_blocking(
                move || -> Result<(git::repo::RepoStatus, Vec<PathBuf>)> {
                    let repo = git::repo::open(&repo_path, removable)?;
                    let status = git::repo::status_paths(&repo)?;
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
                    if excludes.is_excluded(rela) {
                        continue;
                    }
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
        let problem = durability
            .problem
            .expect("a refused publication must be able to say why");
        assert_eq!(
            Some(problem.clone()),
            engine.status(&p.id).expect("status").error,
            "verbatim: the sentence the engine recorded, not a second wording of it"
        );
        assert!(
            problem.to_ascii_lowercase().contains("failed to push"),
            "the reason has to be git's own account of the refused publication — the recording \
             did not fail — got: {problem}"
        );
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
